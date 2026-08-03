//! Managed environment files (D-ENV-FILES1=A).
//!
//! Evaluation produces `ManagedFile` facts. This module is the only runtime
//! writer for those facts: it resolves all bytes and conflicts first, prints a
//! complete plan, then applies the plan with content-addressed objects and a
//! rollback path. The state file records ownership metadata only; it never
//! stores file contents.

use jet_env_model::ModuleEval::{FileMode, ManagedFile};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

const FILES_DIR: &str = "files";
const OBJECTS_DIR: &str = "objects";
const STATE_FILE: &str = "state";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileActionKind {
    Create,
    ReplaceOwned,
    Preserve,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAction {
    pub destination: String,
    pub digest: String,
    pub mode: FileMode,
    pub permissions: Option<u32>,
    pub sensitive: bool,
    pub kind: FileActionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePlan {
    pub actions: Vec<FileAction>,
    files: Vec<PlannedFile>,
    state_before: State,
    state_after: State,
    state_path: PathBuf,
    objects_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedFile {
    declaration: ManagedFile,
    bytes: Vec<u8>,
    digest: String,
    destination: PathBuf,
    action: FileActionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct State {
    entries: BTreeMap<String, StateEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StateEntry {
    digest: String,
    mode: FileMode,
    permissions: Option<u32>,
    sensitive: bool,
    generation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileSyncReport {
    pub applied: usize,
    pub preserved: usize,
    pub unchanged: usize,
}

impl FilePlan {
    pub fn has_changes(&self) -> bool {
        self.actions
            .iter()
            .any(|action| matches!(action.kind, FileActionKind::Create | FileActionKind::ReplaceOwned))
            || self.state_before != self.state_after
    }

    /// Hash the exact source bytes captured by plan(). apply() consumes this
    /// same snapshot, so trust cannot approve a path while a different file
    /// is applied later.
    pub fn source_snapshot_hash(&self) -> String {
        let mut canonical = b"jet-managed-source-snapshot-v1\n".to_vec();
        for file in &self.files {
            canonical.extend_from_slice(file.declaration.destination.as_bytes());
            canonical.push(0);
            canonical.extend_from_slice(&(file.bytes.len() as u64).to_le_bytes());
            canonical.extend_from_slice(&file.bytes);
        }
        crate::SHA256::sha256_hex(&canonical)
    }

    pub fn apply(&self) -> Result<FileSyncReport, String> {
        apply_plan(self)
    }
}

/// Resolve every managed file, including source bytes and all destination
/// conflicts, without changing the project.
pub fn plan(project_dir: &Path, declarations: &[ManagedFile]) -> Result<FilePlan, String> {
    let project_root = project_dir
        .canonicalize()
        .map_err(|error| format!("couldn't resolve project root `{}`: {error}", project_dir.display()))?;
    let files_root = project_root.join(".jet").join(FILES_DIR);
    let objects_dir = files_root.join(OBJECTS_DIR);
    let state_path = files_root.join(STATE_FILE);
    let state_before = load_state(&state_path)?;
    let mut sorted = declarations.to_vec();
    sorted.sort_by(|left, right| left.destination.cmp(&right.destination));

    let mut files = Vec::with_capacity(sorted.len());
    let mut actions = Vec::with_capacity(sorted.len());
    let mut state_after = state_before.clone();
    for declaration in sorted {
        let destination = safe_project_path(&project_root, &declaration.destination, "destination")?;
        let bytes = resolve_bytes(&project_root, &declaration)?;
        let digest = crate::SHA256::sha256_hex(&bytes);
        let kind = classify_action(
            &destination,
            &declaration,
            &digest,
            &state_before.entries,
            &objects_dir,
        )?;
        let action = FileAction {
            destination: declaration.destination.clone(),
            digest: digest.clone(),
            mode: declaration.mode,
            permissions: declaration.permissions,
            sensitive: declaration.sensitive,
            kind,
        };
        if matches!(kind, FileActionKind::Create | FileActionKind::ReplaceOwned | FileActionKind::Unchanged)
            && !matches!(declaration.mode, FileMode::Seed)
        {
            state_after.entries.insert(
                declaration.destination.clone(),
                StateEntry {
                    digest: digest.clone(),
                    mode: declaration.mode,
                    permissions: declaration.permissions,
                    sensitive: declaration.sensitive,
                    generation: declaration.generation.clone(),
                },
            );
        } else if matches!(kind, FileActionKind::Preserve) || matches!(declaration.mode, FileMode::Seed) {
            state_after.entries.remove(&declaration.destination);
        }
        files.push(PlannedFile {
            declaration,
            bytes,
            digest,
            destination,
            action: kind,
        });
        actions.push(action);
    }

    Ok(FilePlan {
        actions,
        files,
        state_before,
        state_after,
        state_path,
        objects_dir,
    })
}

fn resolve_bytes(project_root: &Path, declaration: &ManagedFile) -> Result<Vec<u8>, String> {
    if let Some(content) = &declaration.content {
        return Ok(content.clone());
    }
    let Some(source) = declaration.source.as_deref() else {
        return Err(format!(
            "managed file `{}` has neither content nor source",
            declaration.destination
        ));
    };
    let path = safe_project_path(project_root, source, "source")?;
    fs::read(&path).map_err(|error| {
        format!(
            "couldn't read managed file source `{}`: {error}",
            path.display()
        )
    })
}

fn classify_action(
    destination: &Path,
    declaration: &ManagedFile,
    digest: &str,
    state: &BTreeMap<String, StateEntry>,
    objects_dir: &Path,
) -> Result<FileActionKind, String> {
    let existing = match fs::symlink_metadata(destination) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("couldn't inspect `{}`: {error}", destination.display())),
    };
    let owned = state.get(&declaration.destination);
    let Some(existing) = existing else {
        return Ok(FileActionKind::Create);
    };
    if existing.is_dir() {
        return Err(format!(
            "managed file destination `{}` is a directory",
            declaration.destination
        ));
    }
    if matches!(declaration.mode, FileMode::Seed) {
        return Ok(FileActionKind::Preserve);
    }

    match (owned, existing.file_type().is_symlink(), declaration.mode) {
        (None, _, _) => Err(format!(
            "refusing to overwrite unmanaged destination `{}`",
            declaration.destination
        )),
        (Some(owner), true, FileMode::Symlink) if owner.mode == FileMode::Symlink => {
            let object = object_path(objects_dir, declaration, digest);
            let current = fs::read_link(destination)
                .map_err(|error| format!("couldn't inspect managed link `{}`: {error}", declaration.destination))?;
            if same_link_target(destination, &current, &object)
                && object_permissions_match(&object, declaration)?
                && owner.permissions == declaration.permissions
                && owner.sensitive == declaration.sensitive
            {
                Ok(FileActionKind::Unchanged)
            } else {
                Ok(FileActionKind::ReplaceOwned)
            }
        }
        (Some(owner), false, FileMode::Copy) if owner.mode == FileMode::Copy => {
            let current_digest = file_digest(destination)?;
            let permissions_match = owner.permissions == declaration.permissions;
            if current_digest == digest
                && permissions_match
                && owner.sensitive == declaration.sensitive
            {
                Ok(FileActionKind::Unchanged)
            } else {
                Ok(FileActionKind::ReplaceOwned)
            }
        }
        (Some(owner), true, _) if owner.mode != FileMode::Symlink => Err(format!(
            "managed destination `{}` changed type; refusing to replace it",
            declaration.destination
        )),
        (Some(owner), false, _) if owner.mode == FileMode::Symlink => Err(format!(
            "managed destination `{}` changed from a symlink; refusing to replace it",
            declaration.destination
        )),
        (Some(_), _, _) => Err(format!(
            "managed destination `{}` is owned by a different file mode",
            declaration.destination
        )),
    }
}

fn file_digest(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("couldn't read managed destination `{}`: {error}", path.display()))?;
    Ok(crate::SHA256::sha256_hex(&bytes))
}

fn same_link_target(link: &Path, current: &Path, desired: &Path) -> bool {
    let current = if current.is_absolute() {
        current.to_path_buf()
    } else {
        link.parent().unwrap_or_else(|| Path::new(".")).join(current)
    };
    current == desired || current.canonicalize().ok().as_deref() == desired.canonicalize().ok().as_deref()
}

fn object_path(objects_dir: &Path, declaration: &ManagedFile, digest: &str) -> PathBuf {
    let identity = format!(
        "jet-managed-object-v2\ncontent={digest}\nmode={}\npermissions={:?}\nsensitive={}\n",
        declaration.mode.as_str(),
        declaration.permissions,
        declaration.sensitive,
    );
    objects_dir.join(crate::SHA256::sha256_hex(identity.as_bytes()))
}

fn object_permissions(declaration: &ManagedFile) -> u32 {
    declaration.permissions.unwrap_or_else(|| {
        if declaration.sensitive {
            0o400
        } else {
            0o444
        }
    })
}

fn object_permissions_match(path: &Path, declaration: &ManagedFile) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("couldn't inspect managed object `{}`: {error}", path.display())),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        Ok(metadata.permissions().mode() & 0o7777 == object_permissions(declaration))
    }
    #[cfg(not(unix))]
    {
        Ok(metadata.permissions().readonly() == (object_permissions(declaration) & 0o222 == 0))
    }
}

fn apply_plan(plan: &FilePlan) -> Result<FileSyncReport, String> {
    let mut report = FileSyncReport::default();
    let has_mutations = plan.files.iter().any(|file| {
        matches!(file.action, FileActionKind::Create | FileActionKind::ReplaceOwned)
    });
    if !has_mutations && plan.state_before == plan.state_after {
        report.preserved = plan
            .files
            .iter()
            .filter(|file| file.action == FileActionKind::Preserve)
            .count();
        report.unchanged = plan
            .files
            .iter()
            .filter(|file| file.action == FileActionKind::Unchanged)
            .count();
        return Ok(report);
    }

    fs::create_dir_all(&plan.objects_dir)
        .map_err(|error| format!("couldn't create managed file object store: {error}"))?;
    let backup_dir = plan
        .objects_dir
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".backup-{}", std::process::id()));
    if has_mutations {
        fs::create_dir_all(&backup_dir)
            .map_err(|error| format!("couldn't create managed file rollback area: {error}"))?;
    }

    let mut backups = Vec::<(PathBuf, PathBuf)>::new();
    let mut created = Vec::<PathBuf>::new();
    let mut new_objects = Vec::<PathBuf>::new();
    let result = (|| -> Result<(), String> {
        for (index, file) in plan.files.iter().enumerate() {
            match file.action {
                FileActionKind::Preserve => report.preserved += 1,
                FileActionKind::Unchanged => report.unchanged += 1,
                FileActionKind::Create | FileActionKind::ReplaceOwned => {
                    let object = ensure_object(plan, file, &mut new_objects)?;
                    let destination = &file.destination;
                    if let Ok(metadata) = fs::symlink_metadata(destination) {
                        if metadata.is_dir() {
                            return Err(format!("managed destination `{}` became a directory", file.declaration.destination));
                        }
                        let backup = backup_dir.join(format!("{index}.old"));
                        fs::rename(destination, &backup).map_err(|error| {
                            format!("couldn't stage `{}` for atomic replacement: {error}", file.declaration.destination)
                        })?;
                        backups.push((destination.clone(), backup));
                    } else if !matches!(file.action, FileActionKind::Create) {
                        return Err(format!("managed destination `{}` disappeared during sync", file.declaration.destination));
                    }
                    install_file(file, &object)?;
                    created.push(destination.clone());
                    report.applied += 1;
                }
            }
        }
        write_state_atomic(&plan.state_path, &plan.state_after)?;
        Ok(())
    })();

    if let Err(error) = result {
        for destination in created.iter().rev() {
            let _ = remove_destination(destination);
        }
        for (destination, backup) in backups.iter().rev() {
            let _ = fs::rename(backup, destination);
        }
        for object in new_objects {
            let _ = fs::remove_file(object);
        }
        let _ = fs::remove_dir(&backup_dir);
        return Err(format!("environment file sync rolled back: {error}"));
    }
    for (_, backup) in backups {
        let _ = remove_destination(&backup);
    }
    let _ = fs::remove_dir(&backup_dir);
    Ok(report)
}

fn ensure_object(
    plan: &FilePlan,
    file: &PlannedFile,
    new_objects: &mut Vec<PathBuf>,
) -> Result<PathBuf, String> {
    let object = object_path(&plan.objects_dir, &file.declaration, &file.digest);
    match open_existing_object(&object)? {
        Some(mut object_file) => {
            let mut existing = Vec::new();
            object_file
                .read_to_end(&mut existing)
                .map_err(|error| format!("couldn't read managed object `{}`: {error}", object.display()))?;
            if crate::SHA256::sha256_hex(&existing) != file.digest {
                return Err(format!("managed object `{}` failed its content hash", object.display()));
            }
            set_file_permissions(
                &object_file,
                &object,
                Some(object_permissions(&file.declaration)),
            )?;
        }
        None => {
            let temp = plan
                .objects_dir
                .join(format!(".{}.tmp-{}", file.digest, std::process::id()));
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|error| format!("couldn't create managed object `{}`: {error}", object.display()))?;
            output
                .write_all(&file.bytes)
                .and_then(|_| output.sync_all())
                .map_err(|error| format!("couldn't write managed object `{}`: {error}", object.display()))?;
            set_file_permissions(
                &output,
                &temp,
                Some(object_permissions(&file.declaration)),
            )?;
            output
                .sync_all()
                .map_err(|error| format!("couldn't sync managed object `{}`: {error}", object.display()))?;
            fs::rename(&temp, &object).map_err(|error| {
                let _ = fs::remove_file(&temp);
                format!("couldn't publish managed object `{}`: {error}", object.display())
            })?;
            new_objects.push(object.clone());
        }
    }
    Ok(object)
}

fn open_existing_object(path: &Path) -> Result<Option<fs::File>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!("couldn't inspect managed object `{}`: {error}", path.display()))
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(format!("managed object `{}` is a symlink", path.display()));
    }
    if !metadata.is_file() {
        return Err(format!("managed object `{}` is not a regular file", path.display()));
    }

    #[cfg(unix)]
    let result = {
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(any(target_os = "linux", target_os = "android"))]
        options.custom_flags(0o400000);
        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "openbsd",
            target_os = "netbsd"
        ))]
        options.custom_flags(0x0100);
        options.open(path)
    };
    #[cfg(not(unix))]
    let result = OpenOptions::new().read(true).open(path);

    match result {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("couldn't open managed object `{}`: {error}", path.display())),
    }
}

fn install_file(file: &PlannedFile, object: &Path) -> Result<(), String> {
    let parent = file.destination.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("couldn't create parent for `{}`: {error}", file.declaration.destination))?;
    let name = file
        .destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let temp = parent.join(format!(".{name}.jet-file-{}.tmp", std::process::id()));
    let result = match file.declaration.mode {
        FileMode::Symlink => make_symlink(object, &temp),
        FileMode::Seed | FileMode::Copy => {
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|error| format!("couldn't create `{}`: {error}", file.declaration.destination))?;
            output
                .write_all(&file.bytes)
                .and_then(|_| output.sync_all())
                .map_err(|error| format!("couldn't write `{}`: {error}", file.declaration.destination))?;
            set_permissions(&temp, file.declaration.permissions.or_else(|| {
                file.declaration.sensitive.then_some(0o600)
            }))?;
            Ok(())
        }
    };
    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    fs::rename(&temp, &file.destination).map_err(|error| {
        let _ = fs::remove_file(&temp);
        format!("couldn't install `{}` atomically: {error}", file.declaration.destination)
    })
}

fn remove_destination(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn safe_project_path(project_root: &Path, raw: &str, label: &str) -> Result<PathBuf, String> {
    let path = Path::new(raw);
    if raw.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
        })
    {
        return Err(format!("managed file {label} `{raw}` must stay inside the project"));
    }
    let candidate = project_root.join(path);
    let mut probe = candidate.clone();
    while !probe.exists() && probe != project_root {
        if !probe.pop() {
            break;
        }
    }
    let canonical = probe
        .canonicalize()
        .map_err(|error| format!("couldn't resolve managed file {label} `{raw}`: {error}"))?;
    if !canonical.starts_with(project_root) {
        return Err(format!("managed file {label} `{raw}` escapes the project"));
    }
    Ok(candidate)
}

fn load_state(path: &Path) -> Result<State, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(State::default()),
        Err(error) => return Err(format!("couldn't read managed file state `{}`: {error}", path.display())),
    };
    let mut state = State::default();
    for (line_number, line) in text.lines().enumerate() {
        if line.is_empty() || line == "jet-env-files-v1" {
            continue;
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 7 || fields[0] != "file" {
            return Err(format!("managed file state `{}` has invalid line {}", path.display(), line_number + 1));
        }
        let destination = unescape(fields[1])?;
        let digest = unescape(fields[2])?;
        let mode = match unescape(fields[3])?.as_str() {
            "symlink" => FileMode::Symlink,
            "seed" => FileMode::Seed,
            "copy" => FileMode::Copy,
            other => return Err(format!("managed file state has unknown mode `{other}`")),
        };
        let permissions = match fields[4] {
            "" => None,
            raw => Some(raw.parse::<u32>().map_err(|_| "managed file state has invalid permissions".to_string())?),
        };
        let sensitive = match fields[5] {
            "0" => false,
            "1" => true,
            _ => return Err("managed file state has invalid sensitivity".to_string()),
        };
        let generation = match fields[6] {
            "" => None,
            raw => Some(unescape(raw)?),
        };
        state.entries.insert(
            destination,
            StateEntry { digest, mode, permissions, sensitive, generation },
        );
    }
    Ok(state)
}

fn write_state_atomic(path: &Path, state: &State) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("couldn't create managed file state directory: {error}"))?;
    let temp = parent.join(format!(".state-{}.tmp", std::process::id()));
    let mut text = String::from("jet-env-files-v1\n");
    for (destination, entry) in &state.entries {
        text.push_str(&format!(
            "file\t{}\t{}\t{}\t{}\t{}\t{}\n",
            escape(destination),
            escape(&entry.digest),
            entry.mode.as_str(),
            entry.permissions.map_or_else(String::new, |value| value.to_string()),
            if entry.sensitive { "1" } else { "0" },
            entry.generation.as_deref().map_or_else(String::new, escape),
        ));
    }
    let result = (|| -> io::Result<()> {
        let mut output = OpenOptions::new().write(true).create_new(true).open(&temp)?;
        output.write_all(text.as_bytes())?;
        output.sync_all()?;
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        return Err(format!("couldn't write managed file state `{}`: {error}", path.display()));
    }
    Ok(())
}

fn escape(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\t', "%09")
        .replace('\n', "%0A")
        .replace('\r', "%0D")
}

fn unescape(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("managed file state has an invalid escape".to_string());
            }
            let code = &value[index + 1..index + 3];
            let byte = u8::from_str_radix(code, 16)
                .map_err(|_| "managed file state has an invalid escape".to_string())?;
            output.push(byte);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|_| "managed file state has invalid UTF-8".to_string())
}

fn set_permissions(path: &Path, permissions: Option<u32>) -> Result<(), String> {
    let Some(permissions) = permissions else { return Ok(()) };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(permissions))
            .map_err(|error| format!("couldn't set permissions on `{}`: {error}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let mut mode = fs::metadata(path)
            .map_err(|error| format!("couldn't inspect `{}`: {error}", path.display()))?
            .permissions();
        mode.set_readonly(permissions & 0o222 == 0);
        fs::set_permissions(path, mode)
            .map_err(|error| format!("couldn't set permissions on `{}`: {error}", path.display()))?;
    }
    Ok(())
}

fn set_file_permissions(
    file: &fs::File,
    path: &Path,
    permissions: Option<u32>,
) -> Result<(), String> {
    let Some(permissions) = permissions else { return Ok(()) };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(fs::Permissions::from_mode(permissions))
            .map_err(|error| format!("couldn't set permissions on `{}`: {error}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let mut mode = file
            .metadata()
            .map_err(|error| format!("couldn't inspect `{}`: {error}", path.display()))?
            .permissions();
        mode.set_readonly(permissions & 0o222 == 0);
        file.set_permissions(mode)
            .map_err(|error| format!("couldn't set permissions on `{}`: {error}", path.display()))?;
    }
    Ok(())
}

fn make_symlink(target: &Path, link: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
            .map_err(|error| format!("couldn't create managed symlink `{}`: {error}", link.display()))?;
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link)
            .map_err(|error| format!("couldn't create managed symlink `{}`: {error}", link.display()))?;
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link);
        return Err("managed symlinks are not supported on this platform".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_dir(label: &str) -> PathBuf {
        let suffix = format!(
            "{}-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(suffix)
    }

    fn declaration(destination: &str, sensitive: bool) -> ManagedFile {
        ManagedFile {
            destination: destination.to_string(),
            content: Some(b"same bytes".to_vec()),
            mode: FileMode::Symlink,
            sensitive,
            conflict: jet_env_model::ModuleEval::FileConflict::Refuse,
            ..ManagedFile::default()
        }
    }

    #[cfg(unix)]
    #[test]
    fn content_equal_sensitive_objects_have_separate_restricted_identity() {
        use std::os::unix::fs::PermissionsExt;

        let root = project_dir("jet-env-files");
        fs::create_dir_all(&root).unwrap();
        let declarations = vec![declaration("public", false), declaration("private", true)];
        let first = plan(&root, &declarations).unwrap();
        assert_eq!(first.apply().unwrap().applied, 2);

        let public_target = fs::read_link(root.join("public")).unwrap();
        let private_target = fs::read_link(root.join("private")).unwrap();
        assert_ne!(public_target, private_target);
        assert_eq!(fs::metadata(&public_target).unwrap().permissions().mode() & 0o7777, 0o444);
        assert_eq!(fs::metadata(&private_target).unwrap().permissions().mode() & 0o7777, 0o400);

        fs::set_permissions(&private_target, fs::Permissions::from_mode(0o644)).unwrap();
        let repaired = plan(&root, &declarations).unwrap();
        assert_eq!(
            repaired
                .actions
                .iter()
                .find(|action| action.destination == "private")
                .unwrap()
                .kind,
            FileActionKind::ReplaceOwned
        );
        repaired.apply().unwrap();
        assert_eq!(fs::metadata(&private_target).unwrap().permissions().mode() & 0o7777, 0o400);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn object_store_symlink_is_rejected_before_read_or_chmod() {
        let root = project_dir("jet-env-files-object-symlink");
        let outside = project_dir("jet-env-files-object-target");
        fs::create_dir_all(root.join(".jet/files/objects")).unwrap();
        fs::write(&outside, b"same bytes").unwrap();

        let file = declaration("config", false);
        let digest = crate::SHA256::sha256_hex(b"same bytes");
        let object = object_path(&root.join(".jet/files/objects"), &file, &digest);
        std::os::unix::fs::symlink(&outside, &object).unwrap();

        let error = plan(&root, &[file]).unwrap().apply().unwrap_err();
        assert!(error.contains("object") && error.contains("symlink"), "{error}");
        assert_eq!(fs::read(&outside).unwrap(), b"same bytes");

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(outside);
    }
}
