//! Jetpack package overlays and overrides (D-JPK-OVERLAY1=A).
//!
//! Pure data types and parse/strip helpers live in `jet-pkg-model::Overlay`;
//! this module re-exports them and adds the engine-only operations (patch
//! application, semantic lock records, override drafting) that need
//! `SemanticLock` / filesystem write access.

pub use jet_pkg_model::Overlay::{
    balanced_with_len, find_workspace_body_start, parse_workspace_policy, strip_overlay_policy,
    top_level_commas, unquote, OverlayError, OverlayPolicy, OverlaySet, OverrideProvenance,
    PackageOverride, PatchApplication, ProviderOverride, ResolvedPackageOverride,
};

use super::SemanticLock::{LockIdentity, LockRationale, LockRecordKind, SemanticRecord};
use jet_pkg_model::Authority::AuthorityResolver;
use std::collections::BTreeMap;
use std::fs::{File, Metadata};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

pub fn apply_overlay_patches(
    workspace_root: &Path,
    source_root: &Path,
    package: &PackageOverride,
) -> Result<Vec<PatchApplication>, OverlayError> {
    let workspace = OverlayAuthority::open(workspace_root, "workspace root")?;
    let source = OverlayAuthority::open(source_root, "source root")?;
    let mut staged = BTreeMap::new();
    let mut applied = Vec::new();
    for patch in &package.patches {
        let patch_file = workspace.open_file(patch, "patch")?;
        let text = String::from_utf8(patch_file.bytes).map_err(|_| {
            OverlayError::IO(format!(
                "could not read patch `{}`: patch is not valid UTF-8",
                patch_file.path.display()
            ))
        })?;
        applied.extend(apply_unified_patch_staged(&source, &text, &mut staged)?);
    }
    commit_staged(&mut staged)?;
    Ok(applied)
}

#[cfg(test)]
fn apply_unified_patch(
    source_root: &Path,
    patch_text: &str,
) -> Result<Vec<PatchApplication>, OverlayError> {
    let source = OverlayAuthority::open(source_root, "source root")?;
    let mut staged = BTreeMap::new();
    let applications = apply_unified_patch_staged(&source, patch_text, &mut staged)?;
    commit_staged(&mut staged)?;
    Ok(applications)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InputIdentity {
    length: u64,
    modified_ns: Option<u128>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    links: u64,
}

fn file_identity(metadata: &Metadata) -> Result<InputIdentity, &'static str> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if metadata.nlink() != 1 {
            return Err("is hard-linked (multiple directory entries)");
        }
        return Ok(InputIdentity {
            length: metadata.len(),
            modified_ns: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos()),
            device: metadata.dev(),
            inode: metadata.ino(),
            links: metadata.nlink(),
        });
    }
    #[cfg(not(unix))]
    {
        Ok(InputIdentity {
            length: metadata.len(),
            modified_ns: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos()),
        })
    }
}

fn permissions_mode(metadata: &Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.permissions().mode() & 0o7777
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0
    }
}

#[derive(Debug)]
struct HeldFile {
    path: PathBuf,
    parent: Arc<File>,
    name: String,
    handle: Arc<File>,
    identity: InputIdentity,
    mode: u32,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct StagedFile {
    path: PathBuf,
    root: Arc<File>,
    parent: Arc<File>,
    name: String,
    input: Arc<File>,
    identity: InputIdentity,
    mode: u32,
    original: Vec<u8>,
    output: Vec<u8>,
}

#[derive(Debug)]
struct OverlayAuthority {
    root: PathBuf,
    resolver: AuthorityResolver,
    root_handle: Arc<File>,
}

impl OverlayAuthority {
    fn open(path: &Path, label: &str) -> Result<Self, OverlayError> {
        let resolver = AuthorityResolver::open(path).map_err(|error| {
            OverlayError::IO(format!(
                "could not open {label} authority `{}`: {error}",
                path.display()
            ))
        })?;
        let root = resolver.root().to_path_buf();
        let checked = resolver
            .checked_directory(Path::new(""))
            .map_err(|error| {
                OverlayError::IO(format!(
                    "could not hold {label} authority `{}`: {error}",
                    root.display()
                ))
            })?;
        Ok(Self {
            root,
            resolver,
            root_handle: Arc::clone(&checked.handle),
        })
    }

    fn open_file(&self, raw: &str, label: &str) -> Result<HeldFile, OverlayError> {
        let relative = safe_relative_path(raw, label)?;
        self.open_relative_file(&relative, label)
    }

    fn open_relative_file(
        &self,
        relative: &Path,
        label: &str,
    ) -> Result<HeldFile, OverlayError> {
        let parent_relative = relative.parent().unwrap_or_else(|| Path::new(""));
        let parent = self
            .resolver
            .checked_directory(parent_relative)
            .map_err(|error| {
                OverlayError::IO(format!(
                    "could not open {label} parent `{}`: {error}",
                    self.root.join(parent_relative).display()
                ))
            })?;
        let name = relative
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                OverlayError::Patch(format!(
                    "{label} `{}` has no ordinary final name",
                    relative.display()
                ))
            })?
            .to_string();
        let path = self.root.join(relative);
        let mut file = overlay_fs::open_read(&parent.handle, &name).map_err(|error| {
            if is_symlink_error(&error) {
                OverlayError::Patch(format!("{label} `{}` contains a symlink", path.display()))
            } else {
                OverlayError::IO(format!(
                    "could not read {label} `{}`: {error}",
                    path.display()
                ))
            }
        })?;
        let metadata = file.metadata().map_err(|error| {
            OverlayError::IO(format!(
                "could not inspect {label} `{}`: {error}",
                path.display()
            ))
        })?;
        if !metadata.is_file() {
            return Err(OverlayError::Patch(format!(
                "{label} `{}` must be a regular file",
                path.display()
            )));
        }
        let identity = file_identity(&metadata).map_err(|detail| {
            OverlayError::Patch(format!("{label} `{}` {detail}", path.display()))
        })?;
        let mode = permissions_mode(&metadata);
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(|error| {
            OverlayError::IO(format!(
                "could not read {label} `{}`: {error}",
                path.display()
            ))
        })?;
        let final_metadata = file.metadata().map_err(|error| {
            OverlayError::IO(format!(
                "could not revalidate {label} `{}`: {error}",
                path.display()
            ))
        })?;
        if !final_metadata.is_file()
            || file_identity(&final_metadata).ok() != Some(identity.clone())
        {
            return Err(OverlayError::IO(format!(
                "{label} `{}` changed while it was read",
                path.display()
            )));
        }
        Ok(HeldFile {
            path,
            parent: Arc::clone(&parent.handle),
            name,
            handle: Arc::new(file),
            identity,
            mode,
            bytes,
        })
    }
}

fn apply_unified_patch_staged(
    source: &OverlayAuthority,
    patch_text: &str,
    staged: &mut BTreeMap<PathBuf, StagedFile>,
) -> Result<Vec<PatchApplication>, OverlayError> {
    let mut applications = Vec::new();
    let mut lines = patch_text.lines().peekable();
    while let Some(line) = lines.next() {
        if !line.starts_with("--- ") {
            continue;
        }
        let Some(next) = lines.next() else {
            return Err(OverlayError::Patch("patch missing `+++` line".to_string()));
        };
        if !next.starts_with("+++ ") {
            return Err(OverlayError::Patch("patch missing `+++` line".to_string()));
        }
        let target = normalize_patch_path(next.trim_start_matches("+++ ").trim());
        let relative = safe_relative_path(&target, "patched file")?;
        if !staged.contains_key(&relative) {
            let opened = source.open_relative_file(&relative, "patched file")?;
            let original = opened.bytes.clone();
            staged.insert(
                relative.clone(),
                StagedFile {
                    path: opened.path,
                    root: Arc::clone(&source.root_handle),
                    parent: opened.parent,
                    name: opened.name,
                    input: opened.handle,
                    identity: opened.identity,
                    mode: opened.mode,
                    output: original.clone(),
                    original,
                },
            );
        }
        let entry = staged
            .get_mut(&relative)
            .expect("staged file was inserted or already present");
        let original = entry.output.clone();
        let mut file_lines: Vec<String> = String::from_utf8(original.clone())
            .map_err(|_| {
                OverlayError::Patch(format!("patched file `{target}` is not valid UTF-8"))
            })?
            .lines()
            .map(|s| s.to_string())
            .collect();
        let trailing_newline = original.ends_with(b"\n");
        let mut added = 0usize;
        let mut removed = 0usize;
        let mut had_hunk = false;
        while matches!(lines.peek(), Some(l) if l.starts_with("@@ ")) {
            had_hunk = true;
            let header = lines.next().ok_or_else(|| {
                OverlayError::Patch("patch hunk header ended unexpectedly".to_string())
            })?;
            let (old_start, old_count, new_count) = parse_hunk_range(header)?;
            let mut idx = old_start.saturating_sub(1);
            let mut old_seen = 0usize;
            let mut new_seen = 0usize;
            loop {
                if old_seen == old_count && new_seen == new_count {
                    break;
                }
                let hline = lines.next().ok_or_else(|| {
                    OverlayError::Patch("patch hunk body ended unexpectedly".to_string())
                })?;
                if hline == r"\ No newline at end of file" {
                    continue;
                }
                let Some(tag) = hline.chars().next() else {
                    return Err(OverlayError::Patch(
                        "unsupported empty patch line".to_string(),
                    ));
                };
                let text = &hline[tag.len_utf8()..];
                match tag {
                    ' ' => {
                        if file_lines.get(idx).map(String::as_str) != Some(text) {
                            return Err(OverlayError::Patch(format!(
                                "patch context did not match `{target}`"
                            )));
                        }
                        idx += 1;
                        old_seen += 1;
                        new_seen += 1;
                    }
                    '-' => {
                        if file_lines.get(idx).map(String::as_str) != Some(text) {
                            return Err(OverlayError::Patch(format!(
                                "patch removal did not match `{target}`"
                            )));
                        }
                        file_lines.remove(idx);
                        removed += 1;
                        old_seen += 1;
                    }
                    '+' => {
                        if idx > file_lines.len() {
                            return Err(OverlayError::Patch(format!(
                                "patch insertion is outside `{target}`"
                            )));
                        }
                        file_lines.insert(idx, text.to_string());
                        idx += 1;
                        added += 1;
                        new_seen += 1;
                    }
                    _ => {
                        return Err(OverlayError::Patch(format!(
                            "unsupported patch line `{hline}`"
                        )));
                    }
                }
            }
        }
        if !had_hunk {
            return Err(OverlayError::Patch(format!(
                "patch for `{target}` has no hunks"
            )));
        }
        let mut output = file_lines.join("\n");
        if trailing_newline {
            output.push('\n');
        }
        entry.output = output.into_bytes();
        applications.push(PatchApplication {
            path: target,
            added,
            removed,
        });
    }
    if applications.is_empty() {
        return Err(OverlayError::Patch(
            "patch contains no file hunks".to_string(),
        ));
    }
    Ok(applications)
}

fn safe_relative_path(raw: &str, label: &str) -> Result<PathBuf, OverlayError> {
    let path = Path::new(raw);
    if raw.is_empty()
        || path.is_absolute()
        || raw.contains('\\')
        || raw.contains('\0')
        || raw.chars().any(|character| character.is_control())
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(OverlayError::Patch(format!(
            "{label} path `{raw}` must be a non-empty relative path without `..`"
        )));
    }
    Ok(path.to_path_buf())
}

fn commit_staged(staged: &mut BTreeMap<PathBuf, StagedFile>) -> Result<(), OverlayError> {
    let mut noop = |_staged: &mut BTreeMap<PathBuf, StagedFile>| {};
    commit_staged_inner(staged, &mut noop)
}

#[cfg(test)]
fn commit_staged_with_hook(
    staged: &mut BTreeMap<PathBuf, StagedFile>,
    hook: &mut dyn FnMut(&mut BTreeMap<PathBuf, StagedFile>),
) -> Result<(), OverlayError> {
    commit_staged_inner(staged, hook)
}

fn commit_staged_inner(
    staged: &mut BTreeMap<PathBuf, StagedFile>,
    after_commit: &mut dyn FnMut(&mut BTreeMap<PathBuf, StagedFile>),
) -> Result<(), OverlayError> {
    let keys = staged.keys().cloned().collect::<Vec<_>>();
    let mut committed = Vec::new();
    for key in keys {
        let unchanged = staged
            .get(&key)
            .map(|entry| entry.output == entry.original)
            .unwrap_or(true);
        if unchanged {
            continue;
        }
        let write_error = {
            let entry = staged
                .get(&key)
                .expect("staged file key disappeared");
            write_staged_file(entry).err()
        };
        if let Some(error) = write_error {
            let mut rollback_errors = Vec::new();
            for committed_key in committed.iter().rev() {
                let committed_entry = staged
                    .get(committed_key)
                    .expect("committed file key disappeared");
                if let Err(rollback) =
                    write_bytes_atomically(committed_entry, &committed_entry.original, false)
                {
                    rollback_errors.push(format!(
                        "{}: {rollback}",
                        committed_entry.path.display()
                    ));
                }
            }
            let suffix = if rollback_errors.is_empty() {
                String::new()
            } else {
                format!("; rollback failed: {}", rollback_errors.join("; "))
            };
            let path = staged
                .get(&key)
                .expect("staged file key disappeared")
                .path
                .display()
                .to_string();
            return Err(OverlayError::IO(format!(
                "could not commit patched file `{path}`: {error}{suffix}"
            )));
        }
        committed.push(key);
        after_commit(staged);
    }
    Ok(())
}

fn write_staged_file(entry: &StagedFile) -> io::Result<()> {
    write_bytes_atomically(entry, &entry.output, true)
}

fn validate_staged_target(entry: &StagedFile) -> io::Result<()> {
    let root_metadata = entry.root.metadata()?;
    if !root_metadata.is_dir() {
        return Err(io::Error::other("source root authority is no longer a directory"));
    }
    let current = overlay_fs::open_read(&entry.parent, &entry.name).map_err(|error| {
        if is_symlink_error(&error) {
            io::Error::other("patched target contains a symlink")
        } else {
            error
        }
    })?;
    let current_metadata = current.metadata()?;
    if !current_metadata.is_file() {
        return Err(io::Error::other("patched target is no longer a regular file"));
    }
    let current_identity = file_identity(&current_metadata)
        .map_err(|detail| io::Error::other(format!("patched target {detail}")))?;
    if current_identity != entry.identity {
        return Err(io::Error::other("patched target was replaced before commit"));
    }
    let input_metadata = entry.input.metadata()?;
    let input_identity = file_identity(&input_metadata)
        .map_err(|detail| io::Error::other(format!("patched target {detail}")))?;
    if input_identity != entry.identity {
        return Err(io::Error::other("patched target changed after it was read"));
    }
    Ok(())
}

fn write_bytes_atomically(
    entry: &StagedFile,
    bytes: &[u8],
    verify_target: bool,
) -> io::Result<()> {
    if verify_target {
        validate_staged_target(entry)?;
    } else {
        let root_metadata = entry.root.metadata()?;
        if !root_metadata.is_dir() {
            return Err(io::Error::other("source root authority is no longer a directory"));
        }
    }
    let mut collision = None;
    for _ in 0..16 {
        let serial = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let temporary = format!(
            ".{}.jet-overlay-{}-{serial}",
            entry.name,
            std::process::id()
        );
        let mut file = match overlay_fs::create_exclusive(&entry.parent, &temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                collision = Some(error);
                continue;
            }
            Err(error) => return Err(error),
        };
        let result = (|| {
            file.write_all(bytes)?;
            overlay_fs::set_mode(&file, entry.mode)?;
            file.sync_all()?;
            overlay_fs::sync_directory(&entry.parent)?;
            overlay_fs::rename(&entry.parent, &temporary, &entry.name)?;
            // The file is durable before publication. A directory fsync after
            // rename is best effort so a reporting failure cannot make commit
            // report failure after the destination has already changed.
            let _ = overlay_fs::sync_directory(&entry.parent);
            Ok(())
        })();
        drop(file);
        if result.is_ok() {
            return Ok(());
        }
        let cleanup = overlay_fs::unlink(&entry.parent, &temporary);
        return match cleanup {
            Ok(()) => result,
            Err(error) if error.kind() == io::ErrorKind::NotFound => result,
            Err(cleanup) => {
                let write_error = result
                    .err()
                    .expect("failed atomic write has an error");
                Err(io::Error::other(format!(
                    "{write_error}; temporary cleanup failed: {cleanup}"
                )))
            }
        };
    }
    Err(collision.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate an exclusive overlay temporary file",
        )
    }))
}
fn is_symlink_error(error: &io::Error) -> bool {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        return error.raw_os_error() == Some(40);
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        return error.raw_os_error() == Some(62);
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    )))]
    {
        let _ = error;
        false
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
mod overlay_fs {
    use super::*;
    use std::ffi::{c_char, CString};
    use std::os::fd::{AsRawFd as _, FromRawFd as _};

    const O_RDONLY: i32 = 0;
    const O_WRONLY: i32 = 1;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_CLOEXEC: i32 = 0o2000000;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_CLOEXEC: i32 = 0x01000000;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_NOFOLLOW: i32 = 0o400000;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_NOFOLLOW: i32 = 0x0100;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_NONBLOCK: i32 = 0o4000;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_NONBLOCK: i32 = 0x0004;
    const O_CREAT: i32 = 0o100;
    const O_EXCL: i32 = 0o200;

    unsafe extern "C" {
        fn openat(directory: i32, path: *const c_char, flags: i32, ...) -> i32;
        fn renameat(
            old_directory: i32,
            old_path: *const c_char,
            new_directory: i32,
            new_path: *const c_char,
        ) -> i32;
        fn unlinkat(directory: i32, path: *const c_char, flags: i32) -> i32;
        fn fchmod(file: i32, mode: u32) -> i32;
    }

    fn component(name: &str, detail: &'static str) -> io::Result<CString> {
        CString::new(name)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, detail))
    }

    pub(super) fn open_read(parent: &File, name: &str) -> io::Result<File> {
        let name = component(name, "overlay file name contains NUL")?;
        // SAFETY: `parent` is a live held directory descriptor and `name` is a
        // live single-component NUL-terminated string.
        let fd = unsafe {
            openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK,
                0,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the successful call returned one uniquely owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    pub(super) fn create_exclusive(parent: &File, name: &str) -> io::Result<File> {
        let name = component(name, "overlay temporary name contains NUL")?;
        // SAFETY: `parent` is a live held directory descriptor and `name` is a
        // live single-component NUL-terminated string.
        let fd = unsafe {
            openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the successful call returned one uniquely owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    pub(super) fn set_mode(file: &File, mode: u32) -> io::Result<()> {
        // SAFETY: `file` owns a live descriptor and `fchmod` does not retain
        // the borrowed descriptor after this call.
        if unsafe { fchmod(file.as_raw_fd(), mode) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn rename(parent: &File, old: &str, new: &str) -> io::Result<()> {
        let old = component(old, "overlay source name contains NUL")?;
        let new = component(new, "overlay destination name contains NUL")?;
        // SAFETY: both names are single components and both directories are
        // held descriptors, so no pathname component can be redirected.
        if unsafe {
            renameat(
                parent.as_raw_fd(),
                old.as_ptr(),
                parent.as_raw_fd(),
                new.as_ptr(),
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn unlink(parent: &File, name: &str) -> io::Result<()> {
        let name = component(name, "overlay cleanup name contains NUL")?;
        // SAFETY: `parent` is a live held directory descriptor and unlinkat
        // removes only this directory entry; it never follows a symlink.
        if unsafe { unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn sync_directory(parent: &File) -> io::Result<()> {
        parent.sync_all()
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
)))]
mod overlay_fs {
    use super::*;

    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "descriptor-relative overlay authority is unavailable on this platform",
        )
    }

    pub(super) fn open_read(_parent: &File, _name: &str) -> io::Result<File> {
        Err(unsupported())
    }

    pub(super) fn create_exclusive(_parent: &File, _name: &str) -> io::Result<File> {
        Err(unsupported())
    }

    pub(super) fn set_mode(_file: &File, _mode: u32) -> io::Result<()> {
        Err(unsupported())
    }

    pub(super) fn rename(_parent: &File, _old: &str, _new: &str) -> io::Result<()> {
        Err(unsupported())
    }

    pub(super) fn unlink(_parent: &File, _name: &str) -> io::Result<()> {
        Err(unsupported())
    }

    pub(super) fn sync_directory(_parent: &File) -> io::Result<()> {
        Err(unsupported())
    }
}


fn parse_hunk_range(header: &str) -> Result<(usize, usize, usize), OverlayError> {
    let old = header
        .split_whitespace()
        .find(|part| part.starts_with('-'))
        .ok_or_else(|| OverlayError::Patch("patch hunk missing old range".to_string()))?;
    let new = header
        .split_whitespace()
        .find(|part| part.starts_with('+'))
        .ok_or_else(|| OverlayError::Patch("patch hunk missing new range".to_string()))?;
    let (old_start, old_count) = parse_hunk_side(old, '-')?;
    let (_, new_count) = parse_hunk_side(new, '+')?;
    Ok((old_start, old_count, new_count))
}

fn parse_hunk_side(raw: &str, prefix: char) -> Result<(usize, usize), OverlayError> {
    let mut parts = raw.trim_start_matches(prefix).split(',');
    let start = parts
        .next()
        .unwrap_or_default()
        .parse::<usize>()
        .map_err(|_| OverlayError::Patch("patch hunk has bad range".to_string()))?;
    let count = parts
        .next()
        .map(str::parse::<usize>)
        .transpose()
        .map_err(|_| OverlayError::Patch("patch hunk has bad range".to_string()))?
        .unwrap_or(1);
    if parts.next().is_some() {
        return Err(OverlayError::Patch("patch hunk has bad range".to_string()));
    }
    Ok((start, count))
}

fn normalize_patch_path(raw: &str) -> String {
    raw.split_whitespace()
        .next()
        .unwrap_or(raw)
        .trim_start_matches("a/")
        .trim_start_matches("b/")
        .to_string()
}

pub fn semantic_records(
    policy: &OverlayPolicy,
    owner: &str,
    platform: &str,
) -> Result<Vec<SemanticRecord>, OverlayError> {
    let mut records = Vec::new();
    for overlay in &policy.overlays {
        for package in &overlay.packages {
            let resolved = policy
                .resolve_package_override_checked(&package.package)?
                .ok_or_else(|| {
                    OverlayError::Malformed(format!(
                        "overlay `{}` package `{}` disappeared during resolution",
                        overlay.name, package.package
                    ))
                })?;
            let exact = resolved
                .version
                .clone()
                .or_else(|| resolved.source.clone())
                .unwrap_or_else(|| "source-policy".to_string());
            let policy_fingerprint = resolved_policy_fingerprint(&resolved, overlay);
            let mut rationales = resolved
                .provenance
                .iter()
                .map(|fact| {
                    let source_overlay = policy.overlay(&fact.overlay);
                    let provider = source_overlay
                        .and_then(|overlay| overlay.provider.as_ref())
                        .map(|provider| provider.provider.clone())
                        .unwrap_or_else(|| "unchanged".to_string());
                    let channel = source_overlay
                        .and_then(|overlay| overlay.provider.as_ref())
                        .and_then(|provider| provider.channel.clone())
                        .unwrap_or_default();
                    LockRationale {
                        owner_package: owner.to_string(),
                        reason: format!(
                            "overlay `{}` contributed `{}` = `{}` at priority {}",
                            fact.overlay, fact.field, fact.value, fact.priority
                        ),
                        source_ref: resolved
                            .source
                            .clone()
                            .unwrap_or_else(|| resolved.package.clone()),
                        provider,
                        channel_input: channel,
                        exact_output: exact.clone(),
                        policy_fingerprint: policy_fingerprint.clone(),
                        recipe_id: resolved.patches.join(","),
                        adapter_id: "workspace.overlay".to_string(),
                        signature: String::new(),
                        cache_provenance: "workspace.jet".to_string(),
                        update_command: "jetpack override draft".to_string(),
                    }
                })
                .collect::<Vec<_>>();
            if rationales.is_empty() {
                rationales.push(LockRationale {
                    owner_package: owner.to_string(),
                    reason: "workspace package overlay".to_string(),
                    source_ref: resolved
                        .source
                        .clone()
                        .unwrap_or_else(|| resolved.package.clone()),
                    provider: overlay
                        .provider
                        .as_ref()
                        .map(|provider| provider.provider.clone())
                        .unwrap_or_else(|| "unchanged".to_string()),
                    channel_input: overlay
                        .provider
                        .as_ref()
                        .and_then(|provider| provider.channel.clone())
                        .unwrap_or_default(),
                    exact_output: exact.clone(),
                    policy_fingerprint: policy_fingerprint.clone(),
                    recipe_id: resolved.patches.join(","),
                    adapter_id: "workspace.overlay".to_string(),
                    signature: String::new(),
                    cache_provenance: "workspace.jet".to_string(),
                    update_command: "jetpack override draft".to_string(),
                });
            }
            let record = SemanticRecord {
                identity: LockIdentity {
                    kind: LockRecordKind::PackageOverlay,
                    key: format!("{}:{}", overlay.name, package.package),
                    exact,
                    hash: policy_fingerprint,
                    platform: platform.to_string(),
                },
                rationales,
                future_fields: resolved_fact_fields(&resolved, &overlay.name),
            };
            records.push(record);
        }
    }
    Ok(records)
}

fn resolved_policy_fingerprint(resolved: &ResolvedPackageOverride, overlay: &OverlaySet) -> String {
    let provider = overlay
        .provider
        .as_ref()
        .map(|provider| {
            provider
                .channel
                .as_ref()
                .map(|channel| format!("{}#{channel}", provider.provider))
                .unwrap_or_else(|| provider.provider.clone())
        })
        .unwrap_or_else(|| "provider:unchanged".to_string());
    format!("{}:{}", resolved.policy_fingerprint(), provider)
}

fn resolved_fact_fields(
    resolved: &ResolvedPackageOverride,
    overlay: &str,
) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    fields.insert("overlay-fact-overlay".to_string(), overlay.to_string());
    fields.insert("overlay-fact-package".to_string(), resolved.package.clone());
    fields.insert(
        "overlay-fact-source".to_string(),
        resolved.source.clone().unwrap_or_default(),
    );
    fields.insert(
        "overlay-fact-version".to_string(),
        resolved.version.clone().unwrap_or_default(),
    );
    fields.insert("overlay-fact-flags".to_string(), resolved.flags.join(","));
    fields.insert(
        "overlay-fact-env".to_string(),
        resolved
            .env
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(","),
    );
    fields.insert(
        "overlay-fact-patches".to_string(),
        resolved.patches.join(","),
    );
    fields.insert(
        "overlay-fact-allow-unfree".to_string(),
        resolved.allow_unfree.to_string(),
    );
    fields.insert(
        "overlay-fact-priority".to_string(),
        resolved.priority.to_string(),
    );
    for (field, priority) in &resolved.field_priorities {
        fields.insert(
            format!("overlay-field-priority.{field}"),
            priority.to_string(),
        );
    }
    for (index, fact) in resolved.provenance.iter().enumerate() {
        let prefix = format!("overlay-provenance.{index}");
        fields.insert(format!("{prefix}.overlay"), fact.overlay.clone());
        fields.insert(format!("{prefix}.order"), fact.order.to_string());
        fields.insert(format!("{prefix}.field"), fact.field.clone());
        fields.insert(format!("{prefix}.value"), fact.value.clone());
        fields.insert(format!("{prefix}.priority"), fact.priority.to_string());
    }
    fields
}

/// Map `(overlay, package)` → policy fingerprint for exact invalidation diffs.
pub fn policy_fingerprints(
    policy: &OverlayPolicy,
) -> Result<BTreeMap<(String, String), String>, OverlayError> {
    let mut out = BTreeMap::new();
    for overlay in &policy.overlays {
        for package in &overlay.packages {
            let resolved = policy
                .resolve_package_override_checked(&package.package)?
                .ok_or_else(|| {
                    OverlayError::Malformed(format!(
                        "overlay `{}` package `{}` disappeared during resolution",
                        overlay.name, package.package
                    ))
                })?;
            out.insert(
                (overlay.name.clone(), package.package.clone()),
                resolved_policy_fingerprint(&resolved, overlay),
            );
        }
    }
    Ok(out)
}

/// Diff overlay policies and return exact action invalidations (E4-JP13).
pub fn invalidations_against(
    before: &OverlayPolicy,
    after: &OverlayPolicy,
    actions_by_package: &std::collections::BTreeMap<String, Vec<String>>,
) -> Result<Vec<super::SemanticLock::OverlayInvalidation>, OverlayError> {
    Ok(super::SemanticLock::overlay_invalidations(
        &policy_fingerprints(before)?,
        &policy_fingerprints(after)?,
        actions_by_package,
    ))
}

pub fn draft_overlay_source(
    existing: Option<&str>,
    overlay: &str,
    package: &str,
    patch: Option<&str>,
    provider: Option<&str>,
    channel: Option<&str>,
    allow_unfree: bool,
) -> String {
    let block = draft_block(overlay, package, patch, provider, channel, allow_unfree);
    match existing {
        Some(src) if find_workspace_body_start(src).is_some() => {
            let idx = src.rfind('}').unwrap_or(src.len());
            let mut out = String::new();
            out.push_str(src[..idx].trim_end());
            out.push_str("\n\n");
            out.push_str(&block);
            out.push('\n');
            out.push_str(&src[idx..]);
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out
        }
        _ => {
            let mut out = String::new();
            out.push_str("module workspace {\n");
            out.push_str("    members: []\n\n");
            out.push_str(&block);
            out.push_str("}\n");
            out
        }
    }
}

fn draft_block(
    overlay: &str,
    package: &str,
    patch: Option<&str>,
    provider: Option<&str>,
    channel: Option<&str>,
    allow_unfree: bool,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("    overlay {overlay} {{\n"));
    if let Some(provider) = provider {
        match channel {
            Some(channel) => out.push_str(&format!(
                "        provider: Provider.{provider}(channel: \"{channel}\")\n"
            )),
            None => out.push_str(&format!("        provider: Provider.{provider}\n")),
        }
    }
    out.push_str("        overrides: {\n");
    out.push_str(&format!("            \"{package}\": .{{\n"));
    if let Some(patch) = patch {
        out.push_str(&format!("                patches: [patch(\"{patch}\")],\n"));
    }
    if allow_unfree {
        out.push_str(&format!("                allowUnfree: true,\n"));
    }
    out.push_str("            },\n        }\n");
    out.push_str("    }\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_records_capture_policy_reason() {
        let policy = parse_workspace_policy(
            r#"module workspace {
    overlay beta {
        provider: Provider.nixpkgs(channel: "unstable")
        package("foo").version: "1.2.3"
    }
}"#,
        )
        .unwrap();
        let records = semantic_records(&policy, "app", "x86_64-linux").unwrap();
        assert_eq!(records.len(), 1);
        let rec = &records[0];
        assert_eq!(rec.identity.kind, LockRecordKind::PackageOverlay);
        assert_eq!(rec.identity.key, "beta:foo");
        assert_eq!(rec.rationales[0].provider, "nixpkgs");
        assert_eq!(rec.rationales[0].channel_input, "unstable");
        assert_eq!(rec.rationales[0].update_command, "jetpack override draft");
    }

    #[test]
    fn applies_unified_patch() {
        let root = std::env::temp_dir().join(format!("jet-overlay-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/file.txt"), "one\ntwo\nthree\n").unwrap();
        let patch = "\
--- a/src/file.txt
+++ b/src/file.txt
@@ -1,3 +1,3 @@
 one
-two
+TWO
 three
";
        let applied = apply_unified_patch(&root, patch).unwrap();
        assert_eq!(applied[0].path, "src/file.txt");
        assert_eq!(applied[0].added, 1);
        assert_eq!(applied[0].removed, 1);
        assert_eq!(
            std::fs::read_to_string(root.join("src/file.txt")).unwrap(),
            "one\nTWO\nthree\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn malformed_hunk_header_returns_patch_error() {
        let root =
            std::env::temp_dir().join(format!("jet-overlay-bad-hunk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/file.txt"), "one\n").unwrap();
        let patch = "--- a/src/file.txt\n+++ b/src/file.txt\n@@ malformed\n";

        let err = apply_unified_patch(&root, patch).unwrap_err();
        assert_eq!(err.message(), "patch hunk missing old range");
        assert_eq!(
            std::fs::read_to_string(root.join("src/file.txt")).unwrap(),
            "one\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn adjacent_file_boundary_preserves_both_valid_patches() {
        let root =
            std::env::temp_dir().join(format!("jet-overlay-boundary-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/one.txt"), "one\n").unwrap();
        std::fs::write(root.join("src/two.txt"), "two\n").unwrap();
        let patch = "--- a/src/one.txt\n+++ b/src/one.txt\n@@ -1 +1 @@\n-one\n+ONE\n--- a/src/two.txt\n+++ b/src/two.txt\n@@ -1 +1 @@\n-two\n+TWO\n";

        let applied = apply_unified_patch(&root, patch).unwrap();
        assert_eq!(applied.len(), 2);
        assert_eq!(
            std::fs::read_to_string(root.join("src/one.txt")).unwrap(),
            "ONE\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/two.txt")).unwrap(),
            "TWO\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_hunk_body_line_returns_patch_error() {
        let root = std::env::temp_dir().join(format!(
            "jet-overlay-empty-hunk-line-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/file.txt"), "one\n").unwrap();
        let patch = "--- a/src/file.txt\n+++ b/src/file.txt\n@@ -1 +1 @@\n\n";

        let err = apply_unified_patch(&root, patch).unwrap_err();
        assert_eq!(err.message(), "unsupported empty patch line");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unicode_hunk_body_tag_returns_patch_error() {
        let root = std::env::temp_dir().join(format!(
            "jet-overlay-unicode-hunk-tag-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/file.txt"), "one\n").unwrap();
        let patch = "--- a/src/file.txt\n+++ b/src/file.txt\n@@ -1 +1 @@\néone\n";

        let err = apply_unified_patch(&root, patch).unwrap_err();
        assert_eq!(err.message(), "unsupported patch line `éone`");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn removal_content_starting_with_file_header_marker_is_not_a_boundary() {
        let root =
            std::env::temp_dir().join(format!("jet-overlay-removal-marker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/file.txt"), "-- text\n").unwrap();
        let patch = "--- a/src/file.txt\n+++ b/src/file.txt\n@@ -1 +1 @@\n--- text\n+replacement\n";

        let applied = apply_unified_patch(&root, patch).unwrap();
        assert_eq!(applied[0].removed, 1);
        assert_eq!(applied[0].added, 1);
        assert_eq!(
            std::fs::read_to_string(root.join("src/file.txt")).unwrap(),
            "replacement\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn out_of_range_insertion_returns_patch_error() {
        let root = std::env::temp_dir().join(format!(
            "jet-overlay-insertion-range-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/file.txt"), "one\n").unwrap();
        let patch = "--- a/src/file.txt\n+++ b/src/file.txt\n@@ -999,0 +999,1 @@\n+replacement\n";

        let err = apply_unified_patch(&root, patch).unwrap_err();
        assert_eq!(err.message(), "patch insertion is outside `src/file.txt`");
        assert_eq!(
            std::fs::read_to_string(root.join("src/file.txt")).unwrap(),
            "one\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn patch_paths_cannot_escape_and_leave_source_unchanged() {
        let root =
            std::env::temp_dir().join(format!("jet-overlay-traversal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/file.txt"), "one\n").unwrap();
        let patch = "--- a/src/file.txt\n+++ b/../outside.txt\n@@ -1 +1 @@\n-one\n+ONE\n";

        let err = apply_unified_patch(&root, patch).unwrap_err();
        assert!(err.message().contains("must be a non-empty relative path"));
        assert_eq!(
            std::fs::read_to_string(root.join("src/file.txt")).unwrap(),
            "one\n"
        );
        assert!(!root.join("outside.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn hardlinked_patch_input_is_rejected_without_writing_source() {
        let root =
            std::env::temp_dir().join(format!("jet-overlay-patch-hardlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let workspace = root.join("workspace");
        let source = root.join("source");
        let outside = root.join("outside");
        std::fs::create_dir_all(workspace.join("patches")).unwrap();
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let patch = "--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-one\n+ONE\n";
        std::fs::write(outside.join("change.patch"), patch).unwrap();
        std::fs::hard_link(
            outside.join("change.patch"),
            workspace.join("patches/change.patch"),
        )
        .unwrap();
        std::fs::write(source.join("file.txt"), "one\n").unwrap();
        let package = PackageOverride {
            package: "pkg".to_string(),
            source: None,
            version: None,
            flags: Vec::new(),
            priority: 0,
            field_priorities: std::collections::BTreeMap::new(),
            env: Vec::new(),
            patches: vec!["patches/change.patch".to_string()],
            allow_unfree: false,
        };

        let error = apply_overlay_patches(&workspace, &source, &package).unwrap_err();
        assert!(error.message().contains("hard-linked"));
        assert_eq!(
            std::fs::read_to_string(source.join("file.txt")).unwrap(),
            "one\n"
        );
        assert_eq!(
            std::fs::read_to_string(outside.join("change.patch")).unwrap(),
            patch
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn hardlinked_target_input_is_rejected_without_writing_outside() {
        let root =
            std::env::temp_dir().join(format!("jet-overlay-target-hardlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let source = root.join("source");
        let outside = root.join("outside");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("file.txt"), "one\n").unwrap();
        std::fs::hard_link(outside.join("file.txt"), source.join("file.txt")).unwrap();
        let patch = "--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-one\n+ONE\n";

        let error = apply_unified_patch(&source, patch).unwrap_err();
        assert!(error.message().contains("hard-linked"));
        assert_eq!(
            std::fs::read_to_string(source.join("file.txt")).unwrap(),
            "one\n"
        );
        assert_eq!(
            std::fs::read_to_string(outside.join("file.txt")).unwrap(),
            "one\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn patch_ancestor_swap_keeps_read_on_held_parent() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("jet-overlay-patch-ancestor-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let workspace = root.join("workspace");
        let source = root.join("source");
        let outside = root.join("outside");
        let moved = root.join("moved-patches");
        std::fs::create_dir_all(workspace.join("patches")).unwrap();
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("sentinel"), "keep").unwrap();
        let patch = "--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-one\n+ONE\n";
        std::fs::write(workspace.join("patches/change.patch"), patch).unwrap();
        std::fs::write(source.join("file.txt"), "one\n").unwrap();

        let workspace_authority = OverlayAuthority::open(&workspace, "workspace root").unwrap();
        let patch_parent = workspace_authority
            .resolver
            .checked_directory(Path::new("patches"))
            .unwrap();
        let mut patch_file = overlay_fs::open_read(&patch_parent.handle, "change.patch").unwrap();
        std::fs::rename(workspace.join("patches"), &moved).unwrap();
        symlink(&outside, workspace.join("patches")).unwrap();
        let mut patch_text = String::new();
        patch_file.read_to_string(&mut patch_text).unwrap();
        assert_eq!(patch_text, patch);

        let source_authority = OverlayAuthority::open(&source, "source root").unwrap();
        let mut staged = std::collections::BTreeMap::new();
        apply_unified_patch_staged(&source_authority, &patch_text, &mut staged).unwrap();
        commit_staged(&mut staged).unwrap();
        assert_eq!(
            std::fs::read_to_string(source.join("file.txt")).unwrap(),
            "ONE\n"
        );
        assert_eq!(
            std::fs::read_to_string(outside.join("sentinel")).unwrap(),
            "keep"
        );
        assert!(!outside.join("file.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn final_target_swap_to_symlink_is_rejected_without_touching_outside() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("jet-overlay-final-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let source = root.join("source");
        let outside = root.join("outside");
        std::fs::create_dir_all(source.join("tree")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("sentinel"), "keep\n").unwrap();
        std::fs::write(source.join("tree/file.txt"), "one\n").unwrap();
        let patch = "--- a/tree/file.txt\n+++ b/tree/file.txt\n@@ -1 +1 @@\n-one\n+ONE\n";

        let authority = OverlayAuthority::open(&source, "source root").unwrap();
        let mut staged = std::collections::BTreeMap::new();
        apply_unified_patch_staged(&authority, patch, &mut staged).unwrap();
        std::fs::remove_file(source.join("tree/file.txt")).unwrap();
        symlink(outside.join("sentinel"), source.join("tree/file.txt")).unwrap();

        let error = commit_staged(&mut staged).unwrap_err();
        assert!(error.message().contains("symlink"));
        assert_eq!(
            std::fs::read_to_string(outside.join("sentinel")).unwrap(),
            "keep\n"
        );
        assert_eq!(
            std::fs::read_link(source.join("tree/file.txt")).unwrap(),
            outside.join("sentinel")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn target_ancestor_swap_keeps_write_on_held_parent() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("jet-overlay-target-ancestor-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let source = root.join("source");
        let outside = root.join("outside");
        let moved = root.join("moved-target");
        std::fs::create_dir_all(source.join("tree")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("sentinel"), "keep").unwrap();
        std::fs::write(source.join("tree/file.txt"), "one\n").unwrap();
        let patch = "--- a/tree/file.txt\n+++ b/tree/file.txt\n@@ -1 +1 @@\n-one\n+ONE\n";

        let authority = OverlayAuthority::open(&source, "source root").unwrap();
        let mut staged = std::collections::BTreeMap::new();
        apply_unified_patch_staged(&authority, patch, &mut staged).unwrap();
        std::fs::rename(source.join("tree"), &moved).unwrap();
        symlink(&outside, source.join("tree")).unwrap();
        commit_staged(&mut staged).unwrap();
        assert_eq!(
            std::fs::read_to_string(moved.join("file.txt")).unwrap(),
            "ONE\n"
        );
        assert_eq!(
            std::fs::read_to_string(outside.join("sentinel")).unwrap(),
            "keep"
        );
        assert!(!outside.join("file.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn target_ancestor_swap_rollback_failure_stays_on_held_parent() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "jet-overlay-target-ancestor-rollback-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let source = root.join("source");
        let outside = root.join("outside");
        let moved = root.join("moved-target");
        std::fs::create_dir_all(source.join("tree")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("sentinel"), "keep").unwrap();
        std::fs::write(source.join("tree/one.txt"), "one\n").unwrap();
        std::fs::write(source.join("tree/two.txt"), "two\n").unwrap();
        let patch = "--- a/tree/one.txt\n+++ b/tree/one.txt\n@@ -1 +1 @@\n-one\n+ONE\n--- a/tree/two.txt\n+++ b/tree/two.txt\n@@ -1 +1 @@\n-two\n+TWO\n";

        let authority = OverlayAuthority::open(&source, "source root").unwrap();
        let mut staged = std::collections::BTreeMap::new();
        apply_unified_patch_staged(&authority, patch, &mut staged).unwrap();
        std::fs::rename(source.join("tree"), &moved).unwrap();
        symlink(&outside, source.join("tree")).unwrap();
        staged
            .get_mut(Path::new("tree/two.txt"))
            .expect("second target was staged")
            .name = ".".to_string();
        let mut after_first = |entries: &mut std::collections::BTreeMap<PathBuf, StagedFile>| {
            entries
                .get_mut(Path::new("tree/one.txt"))
                .expect("first target was staged")
                .name = ".".to_string();
        };

        let error = commit_staged_with_hook(&mut staged, &mut after_first).unwrap_err();
        assert!(error.message().contains("could not commit patched file"));
        assert!(error.message().contains("rollback failed"));
        assert_eq!(
            std::fs::read_to_string(moved.join("one.txt")).unwrap(),
            "ONE\n"
        );
        assert_eq!(
            std::fs::read_to_string(moved.join("two.txt")).unwrap(),
            "two\n"
        );
        assert_eq!(
            std::fs::read_to_string(outside.join("sentinel")).unwrap(),
            "keep"
        );
        assert!(!outside.join("one.txt").exists());
        assert!(!outside.join("two.txt").exists());
        assert!(std::fs::read_dir(&moved)
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".jet-overlay-")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn multi_file_patch_failure_is_transactional() {
        let root =
            std::env::temp_dir().join(format!("jet-overlay-transaction-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/one.txt"), "one\n").unwrap();
        std::fs::write(root.join("src/two.txt"), "two\n").unwrap();
        let patch = "--- a/src/one.txt\n+++ b/src/one.txt\n@@ -1 +1 @@\n-one\n+ONE\n--- a/src/two.txt\n+++ b/src/two.txt\n@@ -1 +1 @@\n-wrong\n+TWO\n";

        assert!(apply_unified_patch(&root, patch).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("src/one.txt")).unwrap(),
            "one\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/two.txt")).unwrap(),
            "two\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn repeated_file_patches_compose_in_staging_order() {
        let root = std::env::temp_dir().join(format!("jet-overlay-repeat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/file.txt"), "one\ntwo\n").unwrap();
        let patch = "--- a/src/file.txt\n+++ b/src/file.txt\n@@ -1 +1 @@\n-one\n+ONE\n--- a/src/file.txt\n+++ b/src/file.txt\n@@ -2 +2 @@\n-two\n+TWO\n";

        apply_unified_patch(&root, patch).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("src/file.txt")).unwrap(),
            "ONE\nTWO\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_writes_workspace_source() {
        let src = draft_overlay_source(
            None,
            "plasma_beta",
            "kdePackages.plasma-desktop",
            Some("patches/focus.patch"),
            Some("nixpkgs"),
            Some("plasma-beta"),
            true,
        );
        assert!(src.contains("module workspace"));
        assert!(src.contains("overlay plasma_beta"));
        assert!(src.contains("Provider.nixpkgs(channel: \"plasma-beta\")"));
        assert!(src.contains("patch(\"patches/focus.patch\")"));
        assert!(src.contains("allowUnfree: true"));
    }
}
