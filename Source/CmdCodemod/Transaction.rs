use std::fs::{self, File, OpenOptions};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::{fail, hash_bytes, json_escape};

pub struct Lock {
    _path: PathBuf,
    _file: File,
}
impl Drop for Lock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            extern "C" {
                fn flock(fd: i32, operation: i32) -> i32;
            }
            const LOCK_UN: i32 = 8;
            let _ = unsafe { flock(self._file.as_raw_fd(), LOCK_UN) };
        }
        #[cfg(all(not(unix), not(windows)))]
        let _ = fs::remove_file(&self._path);
    }
}

#[derive(Clone)]
pub struct Change {
    pub path: PathBuf,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
}

pub fn validate_destinations(project: &Path, paths: &[PathBuf]) {
    let project = canonical_project(project);
    let mut names = BTreeSet::new();
    #[cfg(unix)]
    let mut identities = BTreeSet::new();
    for path in paths {
        let canonical = validate_destination(&project, path, true)
            .unwrap_or_else(|e| fail(&format!("unsafe codemod destination `{}`: {e}", path.display())));
        if !names.insert(canonical.clone()) {
            fail(&format!("duplicate codemod destination `{}`", path.display()))
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = fs::metadata(&canonical)
                .unwrap_or_else(|e| fail(&format!("could not inspect `{}`: {e}", canonical.display())));
            if !identities.insert((metadata.dev(), metadata.ino())) {
                fail(&format!("codemod destinations alias the same file at `{}`", path.display()))
            }
        }
    }
}

pub fn read_destination(project: &Path, path: &Path) -> std::io::Result<Vec<u8>> {
    let project = canonical_project_io(project)?;
    let path = validate_destination(&project, path, true)?;
    let relative = path.strip_prefix(&project).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "destination escapes project")
    })?;
    read_beneath(&project, relative)
}

pub fn read_replay_log(project: &Path, path: &Path) -> std::io::Result<String> {
    let project = canonical_project_io(project)?;
    let codemods = validate_codemods_dir(&project, false)?;
    if path.parent() != Some(codemods.as_path()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "replay log is not directly beneath the opened project codemod directory",
        ));
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "replay log is not a regular non-link file",
        ));
    }
    let name = path.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "replay log has no file name")
    })?;
    let bytes = read_beneath(&codemods, Path::new(name))?;
    String::from_utf8(bytes)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "replay log is not UTF-8"))
}

fn canonical_project(project: &Path) -> PathBuf {
    canonical_project_io(project)
        .unwrap_or_else(|e| fail(&format!("could not open codemod project `{}`: {e}", project.display())))
}

fn canonical_project_io(project: &Path) -> std::io::Result<PathBuf> {
    let metadata = fs::symlink_metadata(project)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "codemod project is not a real directory",
        ));
    }
    fs::canonicalize(project)
}

fn validate_destination(project: &Path, path: &Path, must_exist: bool) -> std::io::Result<PathBuf> {
    let relative = path.strip_prefix(project).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "destination escapes project")
    })?;
    let components = relative.components().collect::<Vec<_>>();
    if components.iter().any(|component| !matches!(component, std::path::Component::Normal(_))) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "non-normal destination path"));
    }
    let allowed = relative.starts_with("examples")
        || relative.starts_with(Path::new("tests").join("ui"));
    if !allowed || components.len() < 2 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "destination must be beneath examples/ or tests/ui/",
        ));
    }
    let extension = path.extension().and_then(|value| value.to_str());
    if extension != Some("jet") && !(relative.starts_with(Path::new("tests").join("ui")) && extension == Some("stderr")) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "destination must be a .jet source or paired tests/ui .stderr snapshot",
        ));
    }
    let mut current = project.to_path_buf();
    for component in relative.parent().into_iter().flat_map(Path::components) {
        let std::path::Component::Normal(name) = component else { unreachable!() };
        current.push(name);
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "destination parent contains a link or non-directory",
            ));
        }
    }
    if must_exist {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "destination is not a regular non-link file",
            ));
        }
        let canonical = fs::canonicalize(path)?;
        if canonical != path {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "destination aliases a different canonical path",
            ));
        }
        Ok(canonical)
    } else {
        Ok(path.to_path_buf())
    }
}

fn validate_codemods_dir(project: &Path, create: bool) -> std::io::Result<PathBuf> {
    let jet = project.join(".jet");
    if !jet.exists() && create {
        fs::create_dir(&jet)?;
    }
    let jet_metadata = fs::symlink_metadata(&jet)?;
    if jet_metadata.file_type().is_symlink() || !jet_metadata.is_dir() {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, ".jet is not a real directory"));
    }
    let codemods = jet.join("codemods");
    if !codemods.exists() && create {
        fs::create_dir(&codemods)?;
    }
    let metadata = fs::symlink_metadata(&codemods)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "codemods is not a real directory"));
    }
    Ok(codemods)
}

fn read_nofollow(path: &Path) -> std::io::Result<Vec<u8>> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    #[cfg(windows)]
    use std::os::windows::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(0o400000);
    #[cfg(windows)]
    options.custom_flags(0x0020_0000);
    let mut file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "opened object is not a regular file"));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(unix)]
fn read_beneath(root: &Path, relative: &Path) -> std::io::Result<Vec<u8>> {
    use std::ffi::{CString, OsStr};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    const O_RDONLY: i32 = 0;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_DIRECTORY: i32 = 0o200000;
    const O_NOFOLLOW: i32 = 0o400000;
    extern "C" {
        fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32;
    }
    fn c_name(value: &OsStr) -> std::io::Result<CString> {
        CString::new(value.as_bytes())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in codemod path"))
    }
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        .open(root)?;
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty relative path"));
    }
    let mut held = Vec::<OwnedFd>::new();
    let mut dirfd = root.as_raw_fd();
    for component in &components[..components.len() - 1] {
        let std::path::Component::Normal(name) = component else {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "non-normal relative path"));
        };
        let name = c_name(name)?;
        let fd = unsafe { openat(dirfd, name.as_ptr(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        held.push(unsafe { OwnedFd::from_raw_fd(fd) });
        dirfd = held.last().unwrap().as_raw_fd();
    }
    let std::path::Component::Normal(file_name) = components.last().unwrap() else {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "non-normal file name"));
    };
    let file_name = c_name(file_name)?;
    let fd = unsafe { openat(dirfd, file_name.as_ptr(), O_RDONLY | O_NOFOLLOW | O_CLOEXEC, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "opened object is not a regular file"));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(windows)]
fn read_beneath(root: &Path, relative: &Path) -> std::io::Result<Vec<u8>> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    type Handle = *mut c_void;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    extern "system" {
        fn GetFinalPathNameByHandleW(file: Handle, path: *mut u16, path_len: u32, flags: u32) -> u32;
    }
    fn final_path(handle: Handle) -> std::io::Result<String> {
        let needed = unsafe { GetFinalPathNameByHandleW(handle, std::ptr::null_mut(), 0, 0) };
        if needed == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut buffer = vec![0u16; needed as usize];
        let written = unsafe { GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as u32, 0) };
        if written == 0 || written as usize >= buffer.len() {
            return Err(std::io::Error::last_os_error());
        }
        buffer.truncate(written as usize);
        Ok(String::from_utf16_lossy(&buffer).to_lowercase())
    }
    let path = root.join(relative);
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "opened object is not a regular file"));
    }
    let root_final = fs::canonicalize(root)?.to_string_lossy().to_lowercase();
    let file_final = final_path(file.as_raw_handle())?;
    let root_tail = root_final.trim_start_matches(r"\\?\").trim_end_matches(['\\', '/']);
    let file_tail = file_final.trim_start_matches(r"\\?\");
    let prefix = format!("{root_tail}\\");
    if !file_tail.starts_with(&prefix) {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "opened file escapes root handle"));
    }
    let mut file = file;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(all(not(unix), not(windows)))]
fn read_beneath(root: &Path, relative: &Path) -> std::io::Result<Vec<u8>> {
    read_nofollow(&root.join(relative))
}

pub fn lock(project: &Path) -> Lock {
    let project = canonical_project(project);
    let dir = validate_codemods_dir(&project, true)
        .unwrap_or_else(|e| fail(&format!("could not securely open codemod directory: {e}")));
    let path = dir.join("codemod.lock");
    #[cfg(unix)]
    let mut file = {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }
        const LOCK_EX: i32 = 2;
        const LOCK_NB: i32 = 4;
        const O_CLOEXEC: i32 = 0o2000000;
        const O_NOFOLLOW: i32 = 0o400000;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .custom_flags(O_CLOEXEC | O_NOFOLLOW)
            .open(&path)
            .unwrap_or_else(|e| fail(&format!("could not open codemod lock: {e}")));
        if unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } != 0 {
            fail(&format!("another codemod holds `{}`", path.display()));
        }
        file.set_len(0)
            .unwrap_or_else(|e| fail(&format!("could not reset codemod lock: {e}")));
        file
    };
    #[cfg(windows)]
    let mut file = {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x04000000;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x00200000;
        const GENERIC_READ: u32 = 0x8000_0000;
        const GENERIC_WRITE: u32 = 0x4000_0000;
        const DELETE: u32 = 0x0001_0000;
        OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE)
            .custom_flags(FILE_FLAG_DELETE_ON_CLOSE | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)
            .unwrap_or_else(|_| fail(&format!("another codemod holds `{}`", path.display())))
    };
    #[cfg(all(not(unix), not(windows)))]
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap_or_else(|_| fail(&format!("another codemod holds `{}`", path.display())));
    writeln!(file, "pid={}", std::process::id())
        .unwrap_or_else(|e| fail(&format!("could not write codemod lock: {e}")));
    file.sync_all()
        .unwrap_or_else(|e| fail(&format!("could not sync codemod lock: {e}")));
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        fail("codemod lock is not a regular file")
    }
    Lock { _path: path, _file: file }
}

pub fn recover(project: &Path) {
    let project = canonical_project(project);
    let dir = validate_codemods_dir(&project, true)
        .unwrap_or_else(|e| fail(&format!("could not securely open codemod directory: {e}")));
    let journal = dir.join("transaction.journal");
    if !journal.exists() {
        return;
    }
    let raw = String::from_utf8(read_nofollow(&journal)
        .unwrap_or_else(|e| fail(&format!("could not read recovery journal: {e}"))))
        .unwrap_or_else(|_| fail("recovery journal is not UTF-8"));
    let parsed = parse_journal(&raw);
    validate_journal_paths(&project, &parsed, &journal);
    let records = &parsed.records;
    let all_after = records.iter().all(|record| {
        read_destination(&project, &record.path)
            .map(|current| current == record.after)
            .unwrap_or(false)
    });
    if all_after {
        if parsed.log_path.exists() {
            let current = read_nofollow(&parsed.log_path)
                .unwrap_or_else(|e| fail(&format!("could not inspect recovered replay log: {e}")));
            if current != parsed.log {
                fail("recovered replay log conflicts with transaction journal; journal preserved")
            }
        } else {
            write_new_sync(&parsed.log_path, &parsed.log);
        }
        sync_dir(
            parsed
                .log_path
                .parent()
                .unwrap_or(journal.parent().unwrap()),
        );
        for record in records {
            let _ = fs::remove_file(&record.temp);
        }
        fs::remove_file(&journal)
            .unwrap_or_else(|e| fail(&format!("could not remove completed journal: {e}")));
        sync_dir(journal.parent().unwrap());
        return;
    }
    let mut conflict = Vec::new();
    for record in records {
        let current = read_destination(&project, &record.path).unwrap_or_default();
        if current == record.before {
            continue;
        }
        if current == record.after {
            atomic_restore(&project, &record.path, &record.before);
        } else {
            conflict.push(format!(
                "{} (current {}, before {}, after {})",
                record.path.display(),
                hash_bytes(&current),
                hash_bytes(&record.before),
                hash_bytes(&record.after)
            ));
        }
    }
    if !conflict.is_empty() {
        fail(&format!(
            "codemod recovery found concurrent drift; journal preserved:\n  {}",
            conflict.join("\n  ")
        ));
    }
    for record in records {
        let _ = fs::remove_file(&record.temp);
    }
    fs::remove_file(&journal)
        .unwrap_or_else(|e| fail(&format!("could not remove recovered journal: {e}")));
    sync_dir(journal.parent().unwrap());
}

fn validate_journal_paths(project: &Path, parsed: &Journal, journal: &Path) {
    let project = fs::canonicalize(project)
        .unwrap_or_else(|e| fail(&format!("could not canonicalize codemod project: {e}")));
    let codemods = project.join(".jet/codemods");
    if !parsed.log_path.starts_with(&codemods)
        || parsed.log_path.parent() != Some(codemods.as_path())
        || !parsed.log_path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with(".log.json"))
    {
        fail("recovery journal log path escapes .jet/codemods; journal preserved");
    }
    let destinations = parsed.records.iter().map(|record| record.path.clone()).collect::<Vec<_>>();
    validate_destinations(&project, &destinations);
    #[cfg(unix)]
    let mut temp_identities = {
        use std::os::unix::fs::MetadataExt;
        parsed.records.iter().map(|record| {
            let metadata = fs::metadata(&record.path).unwrap_or_else(|e| {
                fail(&format!("could not inspect recovery destination `{}`: {e}", record.path.display()))
            });
            (metadata.dev(), metadata.ino())
        }).collect::<BTreeSet<_>>()
    };
    for record in &parsed.records {
        if !record.path.starts_with(&project)
            || record.temp.parent() != record.path.parent()
            || !record.temp.starts_with(&project)
        {
            fail("recovery journal file path escapes project or temp directory; journal preserved");
        }
        validate_destination(&project, &record.path, true).unwrap_or_else(|e| {
            fail(&format!("unsafe recovery destination `{}`: {e}; journal preserved", record.path.display()))
        });
        if record.temp.exists() {
            let temp_meta = fs::symlink_metadata(&record.temp).unwrap_or_else(|e| {
                fail(&format!("could not inspect recovery temp `{}`: {e}; journal preserved", record.temp.display()))
            });
            if temp_meta.file_type().is_symlink() || !temp_meta.is_file() {
                fail("recovery temp is not a regular non-link file; journal preserved")
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if !temp_identities.insert((temp_meta.dev(), temp_meta.ino())) {
                    fail("recovery temp aliases a destination or another temp; journal preserved")
                }
            }
        }
        let relative = record.path.strip_prefix(&project).unwrap();
        let mut current = project.clone();
        for component in relative.parent().into_iter().flat_map(Path::components) {
            let std::path::Component::Normal(part) = component else {
                fail("recovery journal contains a non-normal path; journal preserved")
            };
            current.push(part);
            let meta = fs::symlink_metadata(&current).unwrap_or_else(|e| {
                fail(&format!("could not inspect recovery path `{}`: {e}", current.display()))
            });
            if meta.file_type().is_symlink() || !meta.is_dir() {
                fail(&format!(
                    "recovery path contains symlink or non-directory `{}`; journal preserved",
                    current.display()
                ));
            }
        }
    }
    if journal.parent() != Some(codemods.as_path()) {
        fail("recovery journal itself is outside .jet/codemods")
    }
}

pub fn commit(project: &Path, changes: &[Change], log_path: &Path, log: &[u8]) {
    if changes.is_empty() {
        fail("codemod has no file edits");
    }
    let project = canonical_project(project);
    let paths = changes.iter().map(|change| change.path.clone()).collect::<Vec<_>>();
    validate_destinations(&project, &paths);
    let dir = validate_codemods_dir(&project, true)
        .unwrap_or_else(|e| fail(&format!("could not securely open codemod directory: {e}")));
    if log_path.parent() != Some(dir.as_path()) {
        fail("codemod replay log must be directly beneath .jet/codemods")
    }
    if !log_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".log.json"))
    {
        fail("codemod replay log name must end in .log.json")
    }
    if log_path.exists() {
        fail(&format!("codemod replay log already exists: `{}`", log_path.display()))
    }
    for c in changes {
        let current = read_destination(&project, &c.path)
            .unwrap_or_else(|e| fail(&format!("could not re-read `{}`: {e}", c.path.display())));
        if current != c.before {
            fail(&format!(
                "observed drift for `{}` before commit; no files written",
                c.path.display()
            ));
        }
    }
    let journal = dir.join("transaction.journal");
    let tx = format!("{}-{}", std::process::id(), now_nanos());
    let mut records = Vec::new();
    for (i, c) in changes.iter().enumerate() {
        let parent = c.path.parent().unwrap_or(&project);
        let temp = parent.join(format!(".jet-codemod-{tx}-{i}.tmp"));
        write_new_sync(&temp, &c.after);
        records.push(Record {
            path: c.path.clone(),
            temp,
            before: c.before.clone(),
            after: c.after.clone(),
        });
    }
    let journal_text = render_journal(&tx, 0, &records, log_path, log);
    replace_journal_generation(&project, &dir, &journal, &tx, 0, journal_text.as_bytes());
    for (i, record) in records.iter().enumerate() {
        secure_replace(&project, &record.temp, &record.path, &record.before).unwrap_or_else(|e| {
            rollback(&project, &records, &journal);
            fail(&format!(
                "codemod rename failed for `{}`: {e}",
                record.path.display()
            ))
        });
        sync_dir(record.path.parent().unwrap_or(&project));
        replace_journal_generation(
            &project,
            &dir,
            &journal,
            &tx,
            i + 1,
            render_journal(&tx, i + 1, &records, log_path, log).as_bytes(),
        );
        if std::env::var("JET_CODEMOD_CRASH_AFTER_RENAME")
            .ok()
            .as_deref()
            == Some(&(i + 1).to_string())
        {
            std::process::exit(86);
        }
    }
    write_new_sync(log_path, log);
    sync_dir(log_path.parent().unwrap_or(&dir));
    fs::remove_file(&journal)
        .unwrap_or_else(|e| fail(&format!("could not remove transaction journal: {e}")));
    sync_dir(&dir);
}

fn rollback(project: &Path, records: &[Record], journal: &Path) {
    let mut conflict = false;
    for r in records {
        let current = fs::read(&r.path).unwrap_or_default();
        if current == r.after {
            atomic_restore(project, &r.path, &r.before);
        } else if current != r.before {
            conflict = true;
        }
        let _ = fs::remove_file(&r.temp);
    }
    if !conflict {
        let _ = fs::remove_file(journal);
    }
}

#[derive(Clone)]
struct Record {
    path: PathBuf,
    temp: PathBuf,
    before: Vec<u8>,
    after: Vec<u8>,
}
fn render_journal(
    tx: &str,
    completed: usize,
    records: &[Record],
    log_path: &Path,
    log: &[u8],
) -> String {
    let rows = records
        .iter()
        .map(|r| {
            format!(
                "{{\"path\":\"{}\",\"temp\":\"{}\",\"before\":\"{}\",\"after\":\"{}\"}}",
                json_escape(&r.path.display().to_string()),
                json_escape(&r.temp.display().to_string()),
                hex(&r.before),
                hex(&r.after)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":2,\"tx\":\"{}\",\"completed\":{},\"log_path\":\"{}\",\"log\":\"{}\",\"files\":[{}]}}\n",
        json_escape(tx),
        completed,
        json_escape(&log_path.display().to_string()),
        hex(log),
        rows
    )
}
struct Journal {
    records: Vec<Record>,
    log_path: PathBuf,
    log: Vec<u8>,
}
fn parse_journal(raw: &str) -> Journal {
    let root = super::Json::parse(raw)
        .and_then(|v| v.object())
        .unwrap_or_else(|e| fail(&format!("invalid recovery journal: {e}")));
    let files = match root.get("files") {
        Some(super::Json::Value::Array(v)) => v,
        _ => fail("invalid recovery journal files"),
    };
    let string = |key| match root.get(key) {
        Some(super::Json::Value::String(value)) => value.clone(),
        _ => fail(&format!("recovery journal missing `{key}`")),
    };
    let records = files
        .iter()
        .map(|v| {
            let o = match v {
                super::Json::Value::Object(o) => o,
                _ => fail("invalid recovery journal record"),
            };
            let s = |k| match o.get(k) {
                Some(super::Json::Value::String(s)) => s.clone(),
                _ => fail(&format!("recovery journal missing `{k}`")),
            };
            Record {
                path: PathBuf::from(s("path")),
                temp: PathBuf::from(s("temp")),
                before: unhex(&s("before")),
                after: unhex(&s("after")),
            }
        })
        .collect();
    Journal {
        records,
        log_path: PathBuf::from(string("log_path")),
        log: unhex(&string("log")),
    }
}
fn write_new_sync(path: &Path, bytes: &[u8]) {
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)
        .unwrap_or_else(|e| fail(&format!("could not write `{}`: {e}", path.display())));
    f.write_all(bytes)
        .unwrap_or_else(|e| fail(&format!("could not write `{}`: {e}", path.display())));
    f.sync_all()
        .unwrap_or_else(|e| fail(&format!("could not sync `{}`: {e}", path.display())));
}
fn replace_journal_generation(
    project: &Path,
    dir: &Path,
    journal: &Path,
    tx: &str,
    generation: usize,
    bytes: &[u8],
) {
    let staged = dir.join(format!(".transaction-{tx}-{generation}.journal.tmp"));
    write_new_sync(&staged, bytes);
    if journal.exists() {
        let current = read_nofollow(journal)
            .unwrap_or_else(|e| fail(&format!("could not inspect current recovery journal: {e}")));
        secure_replace(project, &staged, journal, &current)
            .unwrap_or_else(|e| fail(&format!("could not publish recovery journal generation: {e}")));
    } else {
        fs::rename(&staged, journal)
            .unwrap_or_else(|e| fail(&format!("could not publish first recovery journal: {e}")));
    }
    sync_dir(dir);
}
fn atomic_restore(project: &Path, path: &Path, bytes: &[u8]) {
    let tmp = path.with_extension(format!("recover-{}.tmp", std::process::id()));
    write_new_sync(&tmp, bytes);
    let current = read_destination(project, path)
        .unwrap_or_else(|e| fail(&format!("could not inspect recovery target `{}`: {e}", path.display())));
    secure_replace(project, &tmp, path, &current)
        .unwrap_or_else(|e| fail(&format!("could not recover `{}`: {e}", path.display())));
    sync_dir(path.parent().unwrap());
}

#[cfg(unix)]
fn secure_replace(project: &Path, temp: &Path, path: &Path, expected: &[u8]) -> std::io::Result<()> {
    use std::ffi::{CString, OsStr};
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;

    const O_RDONLY: i32 = 0;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_DIRECTORY: i32 = 0o200000;
    const O_NOFOLLOW: i32 = 0o400000;
    extern "C" {
        fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32;
        fn renameat(olddirfd: i32, oldpath: *const i8, newdirfd: i32, newpath: *const i8) -> i32;
    }
    fn name(value: &OsStr) -> std::io::Result<CString> {
        CString::new(value.as_bytes())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in codemod path"))
    }

    if temp.parent() != path.parent() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "codemod temp is not beside destination",
        ));
    }
    let relative = path.strip_prefix(project).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "destination escapes project")
    })?;
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        .open(project)?;
    let mut held = Vec::<OwnedFd>::new();
    let mut dirfd = root.as_raw_fd();
    for component in relative.parent().into_iter().flat_map(Path::components) {
        let std::path::Component::Normal(part) = component else {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "non-normal codemod path"));
        };
        let part = name(part)?;
        let fd = unsafe {
            openat(
                dirfd,
                part.as_ptr(),
                O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
                0,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        held.push(unsafe { OwnedFd::from_raw_fd(fd) });
        dirfd = held.last().unwrap().as_raw_fd();
    }
    let final_name = name(relative.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "destination has no name")
    })?)?;
    let fd = unsafe { openat(dirfd, final_name.as_ptr(), O_RDONLY | O_NOFOLLOW | O_CLOEXEC, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut destination = unsafe { File::from_raw_fd(fd) };
    if !destination.metadata()?.is_file() {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "destination is not regular file"));
    }
    let mut current = Vec::new();
    destination.read_to_end(&mut current)?;
    if current != expected {
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "destination drifted before handle-relative rename"));
    }
    let temp_name = name(temp.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "temp has no name")
    })?)?;
    if unsafe { renameat(dirfd, temp_name.as_ptr(), dirfd, final_name.as_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn secure_replace(project: &Path, temp: &Path, path: &Path, expected: &[u8]) -> std::io::Result<()> {
    use std::ffi::c_void;
    use std::io::Read;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

    type Handle = *mut c_void;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const DELETE: u32 = 0x0001_0000;
    const SHARE_ALL: u32 = 0x0000_0007;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    const FILE_ATTRIBUTE_TAG_INFO_CLASS: i32 = 9;
    const FILE_RENAME_INFO_CLASS: i32 = 3;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    #[repr(C)]
    struct FileAttributeTagInfo {
        attributes: u32,
        reparse_tag: u32,
    }
    #[repr(C)]
    struct FileRenameInfo {
        replace_if_exists: i32,
        root_directory: Handle,
        file_name_length: u32,
        file_name: [u16; 1],
    }
    extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            creation: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;
        fn GetFileInformationByHandleEx(
            file: Handle,
            class: i32,
            info: *mut c_void,
            size: u32,
        ) -> i32;
        fn SetFileInformationByHandle(
            file: Handle,
            class: i32,
            info: *mut c_void,
            size: u32,
        ) -> i32;
        fn GetFinalPathNameByHandleW(
            file: Handle,
            path: *mut u16,
            path_len: u32,
            flags: u32,
        ) -> u32;
        fn FlushFileBuffers(file: Handle) -> i32;
    }
    fn wide(path: &std::ffi::OsStr) -> Vec<u16> {
        path.encode_wide().chain(std::iter::once(0)).collect()
    }
    fn open_handle(path: &Path, access: u32, directory: bool) -> std::io::Result<OwnedHandle> {
        let name = wide(path.as_os_str());
        let mut flags = FILE_FLAG_OPEN_REPARSE_POINT;
        if directory {
            flags |= FILE_FLAG_BACKUP_SEMANTICS;
        }
        let raw = unsafe {
            CreateFileW(
                name.as_ptr(),
                access,
                SHARE_ALL,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                flags,
                std::ptr::null_mut(),
            )
        };
        if raw == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        let owned = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut tag = FileAttributeTagInfo {
            attributes: 0,
            reparse_tag: 0,
        };
        if unsafe {
            GetFileInformationByHandleEx(
                owned.as_raw_handle(),
                FILE_ATTRIBUTE_TAG_INFO_CLASS,
                (&mut tag as *mut FileAttributeTagInfo).cast(),
                std::mem::size_of::<FileAttributeTagInfo>() as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        if tag.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "codemod path contains a Windows reparse point",
            ));
        }
        Ok(owned)
    }
    fn final_path(handle: Handle) -> std::io::Result<String> {
        let needed = unsafe { GetFinalPathNameByHandleW(handle, std::ptr::null_mut(), 0, 0) };
        if needed == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut buffer = vec![0u16; needed as usize];
        let written = unsafe {
            GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as u32, 0)
        };
        if written == 0 || written as usize >= buffer.len() {
            return Err(std::io::Error::last_os_error());
        }
        buffer.truncate(written as usize);
        Ok(String::from_utf16_lossy(&buffer).to_lowercase())
    }

    let relative = path.strip_prefix(project).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "destination escapes project")
    })?;
    let mut current_path = project.to_path_buf();
    for component in relative.parent().into_iter().flat_map(Path::components) {
        let std::path::Component::Normal(part) = component else {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "non-normal codemod path"));
        };
        current_path.push(part);
        let meta = fs::symlink_metadata(&current_path)?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "codemod parent is not a real directory"));
        }
    }
    let project_handle = open_handle(project, GENERIC_READ | GENERIC_WRITE, true)?;
    let parent = open_handle(path.parent().unwrap_or(project), GENERIC_READ | GENERIC_WRITE, true)?;
    let destination = open_handle(path, GENERIC_READ, false)?;
    let project_final = final_path(project_handle.as_raw_handle())?;
    let parent_final = final_path(parent.as_raw_handle())?;
    let destination_final = final_path(destination.as_raw_handle())?;
    let project_prefix = format!("{}\\", project_final.trim_end_matches(['\\', '/']));
    if parent_final != project_final && !parent_final.starts_with(&project_prefix) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "opened destination parent escapes project handle",
        ));
    }
    let destination_parent = destination_final
        .rsplit_once(['\\', '/'])
        .map(|(parent, _)| parent)
        .unwrap_or("");
    if destination_parent.trim_end_matches(['\\', '/'])
        != parent_final.trim_end_matches(['\\', '/'])
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "destination does not belong to opened parent directory",
        ));
    }
    let mut destination = File::from(destination);
    let mut current = Vec::new();
    destination.read_to_end(&mut current)?;
    if current != expected {
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "destination drifted before rename"));
    }
    let temp = open_handle(temp, GENERIC_READ | DELETE, false)?;
    let temp_final = final_path(temp.as_raw_handle())?;
    let temp_parent = temp_final
        .rsplit_once(['\\', '/'])
        .map(|(parent, _)| parent)
        .unwrap_or("");
    if temp_parent.trim_end_matches(['\\', '/'])
        != parent_final.trim_end_matches(['\\', '/'])
        || (temp_parent != project_final && !temp_parent.starts_with(&project_prefix))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "opened temp does not belong to opened project parent directory",
        ));
    }
    let file_name = path
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "destination has no name"))?
        .encode_wide()
        .collect::<Vec<_>>();
    let bytes = file_name.len() * std::mem::size_of::<u16>();
    let size = std::mem::size_of::<FileRenameInfo>() + bytes.saturating_sub(2);
    let mut buffer = vec![0u64; size.div_ceil(std::mem::size_of::<u64>())];
    let info = buffer.as_mut_ptr().cast::<FileRenameInfo>();
    unsafe {
        (*info).replace_if_exists = 1;
        (*info).root_directory = parent.as_raw_handle();
        (*info).file_name_length = bytes as u32;
        std::ptr::copy_nonoverlapping(file_name.as_ptr(), (*info).file_name.as_mut_ptr(), file_name.len());
    }
    if unsafe {
        SetFileInformationByHandle(
            temp.as_raw_handle(),
            FILE_RENAME_INFO_CLASS,
            buffer.as_mut_ptr().cast(),
            size as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { FlushFileBuffers(parent.as_raw_handle()) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    if parent_final != project_final
        && unsafe { FlushFileBuffers(project_handle.as_raw_handle()) } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(all(not(unix), not(windows)))]
fn secure_replace(project: &Path, temp: &Path, path: &Path, expected: &[u8]) -> std::io::Result<()> {
    let relative = path.strip_prefix(project).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "destination escapes project")
    })?;
    let mut current_path = project.to_path_buf();
    for component in relative.parent().into_iter().flat_map(Path::components) {
        let std::path::Component::Normal(part) = component else {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "non-normal codemod path"));
        };
        current_path.push(part);
        let meta = fs::symlink_metadata(&current_path)?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "codemod parent is not a real directory"));
        }
    }
    if fs::read(path)? != expected {
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "destination drifted before rename"));
    }
    fs::rename(temp, path)
}
#[cfg(not(windows))]
fn sync_dir(path: &Path) {
    File::open(path)
        .and_then(|f| f.sync_all())
        .unwrap_or_else(|e| {
            fail(&format!(
                "could not sync directory `{}`: {e}",
                path.display()
            ))
        });
}

#[cfg(windows)]
fn sync_dir(path: &Path) {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    type Handle = *mut c_void;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const SHARE_ALL: u32 = 0x0000_0007;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            creation: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;
        fn FlushFileBuffers(file: Handle) -> i32;
    }
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let raw = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_WRITE,
            SHARE_ALL,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        fail(&format!("could not open directory `{}` for sync: {}", path.display(), std::io::Error::last_os_error()))
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    use std::os::windows::io::AsRawHandle;
    if unsafe { FlushFileBuffers(handle.as_raw_handle()) } == 0 {
        fail(&format!("could not sync directory `{}`: {}", path.display(), std::io::Error::last_os_error()))
    }
}
fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}
fn unhex(s: &str) -> Vec<u8> {
    if s.len() % 2 != 0 {
        fail("invalid recovery journal byte encoding");
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .unwrap_or_else(|_| fail("invalid recovery journal byte encoding"))
        })
        .collect()
}
