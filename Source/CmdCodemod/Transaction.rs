use std::fs::{self, File, OpenOptions};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use super::{fail, hash_bytes, json_escape};

pub struct Lock {
    project: PathBuf,
    dir: PathBuf,
    #[cfg(unix)]
    project_file: File,
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
        #[cfg(windows)]
        let _ = fs::remove_file(&self._path);
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
    let mut identities = BTreeSet::new();
    for path in paths {
        let canonical = validate_destination(&project, path, true)
            .unwrap_or_else(|e| fail(&format!("unsafe codemod destination `{}`: {e}", path.display())));
        if !names.insert(canonical.clone()) {
            fail(&format!("duplicate codemod destination `{}`", path.display()))
        }
        let identity = path_identity(&canonical)
            .unwrap_or_else(|e| fail(&format!("could not inspect `{}`: {e}", canonical.display())));
        if !identities.insert(identity) {
            fail(&format!("codemod destinations alias the same file at `{}`", path.display()))
        }
    }
}

pub fn validate_replay_aliases(project: &Path, log: &Path, paths: &[PathBuf]) {
    let project = canonical_project(project);
    let mut identities = BTreeSet::new();
    identities.insert(path_identity(log).unwrap_or_else(|e| fail(&format!("could not inspect replay log: {e}"))));
    for path in paths {
        let path = validate_destination(&project, path, true)
            .unwrap_or_else(|e| fail(&format!("unsafe replay destination `{}`: {e}", path.display())));
        if !identities.insert(path_identity(&path).unwrap_or_else(|e| fail(&format!("could not inspect replay destination: {e}")))) {
            fail("replay log and destinations contain a file-ID alias")
        }
    }
}

#[cfg(unix)]
fn path_identity(path: &Path) -> std::io::Result<(u64, u64)> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "file is not regular or has multiple hard links"));
    }
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
fn path_identity(path: &Path) -> std::io::Result<(u64, u64)> {
    use std::ffi::c_void;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    struct ByHandleFileInformation {
        attributes: u32, creation_low: u32, creation_high: u32, access_low: u32,
        access_high: u32, write_low: u32, write_high: u32, volume: u32,
        size_high: u32, size_low: u32, links: u32, index_high: u32, index_low: u32,
    }
    extern "system" { fn GetFileInformationByHandle(file: *mut c_void, info: *mut ByHandleFileInformation) -> i32; }
    let file = OpenOptions::new().read(true).custom_flags(0x0020_0000).open(path)?;
    let mut info = ByHandleFileInformation { attributes: 0, creation_low: 0, creation_high: 0, access_low: 0, access_high: 0, write_low: 0, write_high: 0, volume: 0, size_high: 0, size_low: 0, links: 0, index_high: 0, index_low: 0 };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 { return Err(std::io::Error::last_os_error()); }
    if info.links != 1 { return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "file has multiple hard links")); }
    Ok((info.volume as u64, ((info.index_high as u64) << 32) | info.index_low as u64))
}

#[cfg(all(not(unix), not(windows)))]
fn path_identity(path: &Path) -> std::io::Result<(u64, u64)> {
    let canonical = fs::canonicalize(path)?;
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical.hash(&mut hasher);
    Ok((0, hasher.finish()))
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

#[cfg(all(not(unix), not(windows)))]
fn read_nofollow(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true);
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
    #[allow(unused_mut)]
    let mut file = {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }
        const LOCK_EX: i32 = 2;
        const LOCK_NB: i32 = 4;
        const O_CLOEXEC: i32 = 0o2000000;
        const O_DIRECTORY: i32 = 0o200000;
        const O_NOFOLLOW: i32 = 0o400000;
        // Lock opened codemods directory, not a mutable pathname. An attacker
        // cannot pre-place/hardlink a lock inode that we later truncate, and
        // directory handle remains authority even if its name is swapped.
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(O_CLOEXEC | O_NOFOLLOW | O_DIRECTORY)
            .open(&dir)
            .unwrap_or_else(|e| fail(&format!("could not open codemod directory lock: {e}")));
        if unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } != 0 {
            fail(&format!("another codemod holds `{}`", dir.display()));
        }
        file
    };
    #[cfg(unix)]
    let project_file = {
        use std::os::unix::fs::OpenOptionsExt;
        const O_CLOEXEC: i32 = 0o2000000;
        const O_DIRECTORY: i32 = 0o200000;
        const O_NOFOLLOW: i32 = 0o400000;
        OpenOptions::new().read(true).custom_flags(O_CLOEXEC | O_NOFOLLOW | O_DIRECTORY)
            .open(&project).unwrap_or_else(|e| fail(&format!("could not retain codemod project directory: {e}")))
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
    #[cfg(not(unix))]
    {
        writeln!(file, "pid={}", std::process::id())
            .unwrap_or_else(|e| fail(&format!("could not write codemod lock: {e}")));
        file.sync_all()
            .unwrap_or_else(|e| fail(&format!("could not sync codemod lock: {e}")));
        if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
            fail("codemod lock is not a regular file")
        }
    }
    Lock {
        project,
        dir,
        #[cfg(unix)]
        project_file,
        _path: path,
        _file: file,
    }
}

#[cfg(unix)]
fn read_at_checked(dir: &File, file_name: &std::ffi::OsStr) -> std::io::Result<(File, (u64, u64), Vec<u8>)> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{FileExt, MetadataExt};
    const O_RDONLY: i32 = 0;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_NOFOLLOW: i32 = 0o400000;
    extern "C" { fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32; }
    let name = CString::new(file_name.as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in codemod file name"))?;
    let fd = unsafe { openat(dir.as_raw_fd(), name.as_ptr(), O_RDONLY | O_NOFOLLOW | O_CLOEXEC, 0) };
    if fd < 0 { return Err(std::io::Error::last_os_error()); }
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "opened codemod file is linked or not regular"));
    }
    let mut bytes = vec![0u8; usize::try_from(metadata.len()).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "file too large"))?];
    let mut offset = 0;
    while offset < bytes.len() {
        let read = file.read_at(&mut bytes[offset..], offset as u64)?;
        if read == 0 { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "codemod file changed while reading")); }
        offset += read;
    }
    Ok((file, (metadata.dev(), metadata.ino()), bytes))
}

#[cfg(unix)]
fn read_held(file: &File) -> std::io::Result<Vec<u8>> {
    use std::os::unix::fs::FileExt;
    let metadata = file.metadata()?;
    let mut bytes = vec![0u8; usize::try_from(metadata.len()).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "file too large"))?];
    let mut offset = 0;
    while offset < bytes.len() {
        let read = file.read_at(&mut bytes[offset..], offset as u64)?;
        if read == 0 { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "held file changed while reading")); }
        offset += read;
    }
    Ok(bytes)
}

pub fn recover(lock: &Lock) {
    #[cfg(unix)]
    recover_unix(lock);
    #[cfg(not(unix))]
    fail("codemod recovery is unavailable on this platform until native handle-relative transactions are implemented")
}

#[cfg(unix)]
fn recover_unix(lock: &Lock) {
    let journal_name = std::ffi::OsStr::new("transaction.journal");
    let (_, journal_id, journal_bytes) = match read_at_checked(&lock._file, journal_name) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => fail(&format!("could not read recovery journal: {error}")),
    };
    let raw = String::from_utf8(journal_bytes)
        .unwrap_or_else(|_| fail("recovery journal is not UTF-8"));
    let parsed = parse_journal(&raw);
    let handles = prepare_recovery_handles(lock, &parsed);
    let records = &parsed.records;
    let all_after = records.iter().zip(&handles).all(|(record, held)| {
        read_held(&held.destination).is_ok_and(|current| current == record.after)
    });
    if all_after {
        let log_name = parsed.log_path.file_name().unwrap();
        match read_at_checked(&lock._file, log_name) {
            Ok((_, _, current)) if current != parsed.log => {
                fail("recovered replay log conflicts with transaction journal; journal preserved")
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                write_new_at(&lock._file, log_name, &parsed.log)
                    .unwrap_or_else(|e| fail(&format!("could not publish recovered replay log: {e}")));
            }
            Err(error) => fail(&format!("could not inspect recovered replay log: {error}")),
        }
        for (record, held) in records.iter().zip(&handles) {
            cleanup_recovery_temp(record, held)
                .unwrap_or_else(|e| fail(&format!("could not remove recovered temp: {e}")));
        }
        unlink_at_checked(&lock._file, journal_name, journal_id)
            .unwrap_or_else(|e| fail(&format!("could not remove completed journal: {e}")));
        return;
    }
    let mut conflict = Vec::new();
    for (record, held) in records.iter().zip(&handles) {
        let current = read_held(&held.destination).unwrap_or_default();
        if current == record.before {
            continue;
        }
        if current == record.after {
            atomic_restore_held(record, held, &record.before)
                .unwrap_or_else(|e| fail(&format!("could not recover `{}`: {e}", record.path.display())));
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
    for (record, held) in records.iter().zip(&handles) {
        cleanup_recovery_temp(record, held)
            .unwrap_or_else(|e| fail(&format!("could not remove recovered temp: {e}")));
    }
    unlink_at_checked(&lock._file, journal_name, journal_id)
        .unwrap_or_else(|e| fail(&format!("could not remove recovered journal: {e}")));
}

#[cfg(unix)]
fn prepare_recovery_handles(lock: &Lock, parsed: &Journal) -> Vec<UnixRecoveryHandles> {
    use std::ffi::{CString, OsStr};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    const O_RDONLY: i32 = 0;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_DIRECTORY: i32 = 0o200000;
    const O_NOFOLLOW: i32 = 0o400000;
    extern "C" { fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32; }
    fn name(value: &OsStr) -> std::io::Result<CString> {
        CString::new(value.as_bytes()).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in recovery path"))
    }
    if parsed.log_path.parent() != Some(lock.dir.as_path())
        || !parsed.log_path.file_name().and_then(|value| value.to_str()).is_some_and(|value| value.ends_with(".log.json"))
    {
        fail("recovery journal log path escapes retained codemod directory; journal preserved")
    }
    let mut identities = BTreeSet::new();
    let mut result = Vec::new();
    for record in &parsed.records {
        if record.temp.parent() != record.path.parent() {
            fail("recovery temp is not beside destination; journal preserved")
        }
        let relative = record.path.strip_prefix(&lock.project)
            .unwrap_or_else(|_| fail("recovery destination escapes retained project; journal preserved"));
        let components = relative.components().collect::<Vec<_>>();
        let allowed = relative.starts_with("examples") || relative.starts_with(Path::new("tests").join("ui"));
        let extension = record.path.extension().and_then(|value| value.to_str());
        let allowed_extension = extension == Some("jet")
            || (relative.starts_with(Path::new("tests").join("ui")) && extension == Some("stderr"));
        if !allowed || !allowed_extension || components.len() < 2 {
            fail("recovery destination leaves allowed roots; journal preserved")
        }
        let mut held = Vec::<OwnedFd>::new();
        let mut dirfd = lock.project_file.as_raw_fd();
        for component in relative.parent().into_iter().flat_map(Path::components) {
            let std::path::Component::Normal(part) = component else { fail("recovery path is not normal; journal preserved") };
            let part = name(part).unwrap_or_else(|e| fail(&format!("invalid recovery path: {e}")));
            let fd = unsafe { openat(dirfd, part.as_ptr(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0) };
            if fd < 0 { fail(&format!("could not retain recovery parent: {}", std::io::Error::last_os_error())); }
            held.push(unsafe { OwnedFd::from_raw_fd(fd) });
            dirfd = held.last().unwrap().as_raw_fd();
        }
        let parent_fd = if let Some(last) = held.pop() {
            last
        } else {
            let dot = CString::new(".").unwrap();
            let fd = unsafe { openat(lock.project_file.as_raw_fd(), dot.as_ptr(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0) };
            if fd < 0 { fail(&format!("could not clone retained project handle: {}", std::io::Error::last_os_error())); }
            unsafe { OwnedFd::from_raw_fd(fd) }
        };
        let parent = File::from(parent_fd);
        let destination_name = name(record.path.file_name().unwrap()).unwrap();
        let destination_fd = unsafe { openat(parent.as_raw_fd(), destination_name.as_ptr(), O_RDONLY | O_NOFOLLOW | O_CLOEXEC, 0) };
        if destination_fd < 0 { fail(&format!("could not retain recovery destination: {}", std::io::Error::last_os_error())); }
        let destination = unsafe { File::from_raw_fd(destination_fd) };
        let destination_meta = destination.metadata().unwrap_or_else(|e| fail(&format!("could not identify recovery destination: {e}")));
        let destination_id = (destination_meta.dev(), destination_meta.ino());
        if !destination_meta.is_file() || destination_meta.nlink() != 1
            || (destination_id != record.destination_id && destination_id != record.temp_id)
            || !identities.insert(destination_id)
        {
            fail("recovery destination identity changed or aliases another row; journal preserved")
        }
        let temp_name = name(record.temp.file_name().unwrap()).unwrap();
        let temp_fd = unsafe { openat(parent.as_raw_fd(), temp_name.as_ptr(), O_RDONLY | O_NOFOLLOW | O_CLOEXEC, 0) };
        let temp = if temp_fd < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::NotFound { None }
            else { fail(&format!("could not retain recovery temp: {error}")) }
        } else {
            let file = unsafe { File::from_raw_fd(temp_fd) };
            let metadata = file.metadata().unwrap_or_else(|e| fail(&format!("could not identify recovery temp: {e}")));
            let identity = (metadata.dev(), metadata.ino());
            if !metadata.is_file() || metadata.nlink() != 1 || identity != record.temp_id || !identities.insert(identity) {
                fail("recovery temp identity changed or aliases another row; journal preserved")
            }
            Some(file)
        };
        result.push(UnixRecoveryHandles { parent, destination, destination_id, temp, temp_id: record.temp_id });
    }
    result
}

pub fn commit(lock: &Lock, changes: &[Change], log_path: &Path, log: &[u8]) {
    #[cfg(unix)]
    commit_unix(lock, changes, log_path, log);
    #[cfg(not(unix))]
    fail("codemod commit is unavailable on this platform until native handle-relative transactions are implemented")
}

#[cfg(unix)]
fn commit_unix(lock: &Lock, changes: &[Change], log_path: &Path, log: &[u8]) {
    if changes.is_empty() {
        fail("codemod has no file edits");
    }
    let project = lock.project.clone();
    let paths = changes.iter().map(|change| change.path.clone()).collect::<Vec<_>>();
    validate_destinations(&project, &paths);
    let dir = lock.dir.clone();
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
    let tx = format!("{}-{}", std::process::id(), now_nanos());
    let mut records = Vec::new();
    for (i, c) in changes.iter().enumerate() {
        let parent = c.path.parent().unwrap_or(&project);
        let temp = parent.join(format!(".jet-codemod-{tx}-{i}.tmp"));
        let mut record = Record {
            path: c.path.clone(),
            temp,
            before: c.before.clone(),
            after: c.after.clone(),
            destination_id: (0, 0),
            temp_id: (0, 0),
            #[cfg(unix)]
            handles: None,
        };
        #[cfg(unix)]
        {
            record.handles = Some(prepare_unix_record(&lock.project_file, &project, &record.path, &record.temp, &record.after)
                .unwrap_or_else(|e| fail(&format!("could not securely stage `{}`: {e}", record.path.display()))));
            let handles = record.handles.as_ref().unwrap();
            record.destination_id = handles.destination_id;
            record.temp_id = handles.temp_id;
            if read_held(&handles.destination).unwrap_or_else(|e| fail(&format!("could not read held destination: {e}"))) != record.before {
                fail(&format!("observed drift for `{}` before commit; no files written", record.path.display()));
            }
        }
        #[cfg(not(unix))]
        {
            write_new_sync(&record.temp, &record.after);
            record.temp_id = path_identity(&record.temp)
                .unwrap_or_else(|e| fail(&format!("could not identify temp: {e}")));
        }
        records.push(record);
    }
    let journal_text = render_journal(&tx, 0, &records, log_path, log);
    let mut journal_id = replace_journal_generation(lock, &tx, 0, journal_text.as_bytes(), None);
    if std::env::var("JET_CODEMOD_CRASH_AFTER_JOURNAL").ok().as_deref() == Some("1") {
        std::process::exit(87);
    }
    for (i, record) in records.iter().enumerate() {
        secure_replace_record(&project, record, &record.before).unwrap_or_else(|e| {
            rollback(&project, &records, lock, journal_id);
            fail(&format!(
                "codemod rename failed for `{}`: {e}",
                record.path.display()
            ))
        });
        journal_id = replace_journal_generation(
            lock,
            &tx,
            i + 1,
            render_journal(&tx, i + 1, &records, log_path, log).as_bytes(),
            Some(journal_id),
        );
        if std::env::var("JET_CODEMOD_CRASH_AFTER_RENAME")
            .ok()
            .as_deref()
            == Some(&(i + 1).to_string())
        {
            std::process::exit(86);
        }
    }
    write_new_at(&lock._file, log_path.file_name().unwrap(), log)
        .unwrap_or_else(|e| fail(&format!("could not publish replay log: {e}")));
    unlink_at_checked(&lock._file, std::ffi::OsStr::new("transaction.journal"), journal_id)
        .unwrap_or_else(|e| fail(&format!("could not remove transaction journal: {e}")));
}

#[cfg(unix)]
fn prepare_unix_record(
    project_file: &File,
    project: &Path,
    path: &Path,
    temp: &Path,
    bytes: &[u8],
) -> std::io::Result<UnixRecordHandles> {
    use std::ffi::{CString, OsStr};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    const O_RDONLY: i32 = 0;
    const O_WRONLY: i32 = 1;
    const O_CREAT: i32 = 0o100;
    const O_EXCL: i32 = 0o200;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_DIRECTORY: i32 = 0o200000;
    const O_NOFOLLOW: i32 = 0o400000;
    extern "C" {
        fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32;
    }
    fn name(value: &OsStr) -> std::io::Result<CString> {
        CString::new(value.as_bytes())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in codemod path"))
    }

    if temp.parent() != path.parent() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "temp is not beside destination"));
    }
    let relative = path.strip_prefix(project)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::PermissionDenied, "destination escapes project"))?;
    let project_file = project_file.try_clone()?;
    let mut held = Vec::<OwnedFd>::new();
    let mut dirfd = project_file.as_raw_fd();
    for component in relative.parent().into_iter().flat_map(Path::components) {
        let std::path::Component::Normal(part) = component else {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "non-normal codemod path"));
        };
        let part = name(part)?;
        let fd = unsafe { openat(dirfd, part.as_ptr(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0) };
        if fd < 0 { return Err(std::io::Error::last_os_error()); }
        held.push(unsafe { OwnedFd::from_raw_fd(fd) });
        dirfd = held.last().unwrap().as_raw_fd();
    }
    let parent_owned = if let Some(last) = held.pop() {
        last
    } else {
        let dot = CString::new(".").unwrap();
        let fd = unsafe { openat(project_file.as_raw_fd(), dot.as_ptr(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0) };
        if fd < 0 { return Err(std::io::Error::last_os_error()); }
        unsafe { OwnedFd::from_raw_fd(fd) }
    };
    let parent = File::from(parent_owned);
    let destination_name = name(path.file_name().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "destination has no name"))?)?;
    let destination_fd = unsafe { openat(parent.as_raw_fd(), destination_name.as_ptr(), O_RDONLY | O_NOFOLLOW | O_CLOEXEC, 0) };
    if destination_fd < 0 { return Err(std::io::Error::last_os_error()); }
    let destination = unsafe { File::from_raw_fd(destination_fd) };
    let destination_meta = destination.metadata()?;
    if !destination_meta.is_file() || destination_meta.nlink() != 1 {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "destination is linked or not regular"));
    }
    let temp_name = name(temp.file_name().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "temp has no name"))?)?;
    let temp_fd = unsafe { openat(parent.as_raw_fd(), temp_name.as_ptr(), O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600) };
    if temp_fd < 0 { return Err(std::io::Error::last_os_error()); }
    let mut temp_file = unsafe { File::from_raw_fd(temp_fd) };
    temp_file.write_all(bytes)?;
    temp_file.sync_all()?;
    let temp_meta = temp_file.metadata()?;
    if !temp_meta.is_file() || temp_meta.nlink() != 1 {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "temp is linked or not regular"));
    }
    Ok(UnixRecordHandles {
        _project: project_file,
        parent,
        destination,
        temp: temp_file,
        destination_id: (destination_meta.dev(), destination_meta.ino()),
        temp_id: (temp_meta.dev(), temp_meta.ino()),
    })
}

fn secure_replace_record(project: &Path, record: &Record, expected: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    if let Some(handles) = &record.handles {
        return secure_replace_held(handles, &record.temp, &record.path, expected);
    }
    secure_replace(project, &record.temp, &record.path, expected)
}

#[cfg(unix)]
fn secure_replace_held(
    handles: &UnixRecordHandles,
    temp: &Path,
    path: &Path,
    expected: &[u8],
) -> std::io::Result<()> {
    use std::ffi::{CString, OsStr};
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    const O_RDONLY: i32 = 0;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_NOFOLLOW: i32 = 0o400000;
    extern "C" {
        fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32;
        fn renameat(olddirfd: i32, oldpath: *const i8, newdirfd: i32, newpath: *const i8) -> i32;
    }
    fn name(value: &OsStr) -> std::io::Result<CString> {
        CString::new(value.as_bytes())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in codemod path"))
    }
    let dest_name = name(path.file_name().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "destination has no name"))?)?;
    let temp_name = name(temp.file_name().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "temp has no name"))?)?;
    let reopen = |name: &CString| -> std::io::Result<File> {
        let fd = unsafe { openat(handles.parent.as_raw_fd(), name.as_ptr(), O_RDONLY | O_NOFOLLOW | O_CLOEXEC, 0) };
        if fd < 0 { return Err(std::io::Error::last_os_error()); }
        Ok(unsafe { File::from_raw_fd(fd) })
    };
    let mut current = reopen(&dest_name)?;
    let current_meta = current.metadata()?;
    if (current_meta.dev(), current_meta.ino()) != handles.destination_id
        || current_meta.nlink() != 1
    {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "destination name identity changed before rename"));
    }
    let temp_current = reopen(&temp_name)?;
    let temp_meta = temp_current.metadata()?;
    if (temp_meta.dev(), temp_meta.ino()) != handles.temp_id || temp_meta.nlink() != 1 {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "temp name identity changed before rename"));
    }
    let held_dest = handles.destination.metadata()?;
    let held_temp = handles.temp.metadata()?;
    if (held_dest.dev(), held_dest.ino()) != handles.destination_id
        || (held_temp.dev(), held_temp.ino()) != handles.temp_id
    {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "held codemod file identity changed"));
    }
    let mut bytes = Vec::new();
    current.read_to_end(&mut bytes)?;
    if bytes != expected {
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "destination drifted before handle-relative rename"));
    }
    if unsafe { renameat(handles.parent.as_raw_fd(), temp_name.as_ptr(), handles.parent.as_raw_fd(), dest_name.as_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let published = reopen(&dest_name)?;
    let published_meta = published.metadata()?;
    if (published_meta.dev(), published_meta.ino()) != handles.temp_id || published_meta.nlink() != 1 {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "published destination identity differs from retained temp"));
    }
    handles.parent.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn cleanup_recovery_temp(record: &Record, handles: &UnixRecoveryHandles) -> std::io::Result<()> {
    let Some(temp) = &handles.temp else { return Ok(()) };
    use std::os::unix::fs::MetadataExt;
    let metadata = temp.metadata()?;
    if (metadata.dev(), metadata.ino()) != handles.temp_id || metadata.nlink() != 1 {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "held recovery temp identity changed"));
    }
    unlink_at_checked(&handles.parent, record.temp.file_name().unwrap(), handles.temp_id)
}

#[cfg(unix)]
fn atomic_restore_held(record: &Record, handles: &UnixRecoveryHandles, bytes: &[u8]) -> std::io::Result<()> {
    replace_bytes_at(
        &handles.parent,
        record.path.file_name().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "destination has no name"))?,
        handles.destination_id,
        &record.after,
        bytes,
    )
}

#[cfg(unix)]
fn replace_bytes_at(parent: &File, destination_name: &std::ffi::OsStr, expected_id: (u64, u64), expected: &[u8], bytes: &[u8]) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    extern "C" { fn renameat(olddirfd: i32, oldpath: *const i8, newdirfd: i32, newpath: *const i8) -> i32; }
    let staged_name = format!(".jet-codemod-recover-{}-{}.tmp", std::process::id(), now_nanos());
    let staged_id = write_new_at(parent, std::ffi::OsStr::new(&staged_name), bytes)?;
    let (_, current_id, current) = read_at_checked(parent, destination_name)?;
    if current_id != expected_id || current != expected {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "destination changed before recovery rename"));
    }
    let staged_name = CString::new(std::ffi::OsStr::new(&staged_name).as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in recovery temp"))?;
    let destination_name = CString::new(destination_name.as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in recovery destination"))?;
    if unsafe { renameat(parent.as_raw_fd(), staged_name.as_ptr(), parent.as_raw_fd(), destination_name.as_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let destination_os = std::ffi::OsStr::from_bytes(destination_name.as_bytes());
    let (_, published_id, _) = read_at_checked(parent, destination_os)?;
    if published_id != staged_id {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "recovered destination identity differs from staged file"));
    }
    parent.sync_all()
}

#[cfg(unix)]
fn rollback(_project: &Path, records: &[Record], lock: &Lock, journal_id: (u64, u64)) {
    let mut conflict = false;
    for r in records {
        let Some(handles) = &r.handles else { conflict = true; continue };
        let destination_name = r.path.file_name().unwrap();
        match read_at_checked(&handles.parent, destination_name) {
            Ok((_, id, current)) if id == handles.temp_id && current == r.after => {
                if replace_bytes_at(&handles.parent, destination_name, id, &r.after, &r.before).is_err() {
                    conflict = true;
                }
            }
            Ok((_, id, current)) if id == handles.destination_id && current == r.before => {}
            _ => conflict = true,
        }
        if let Ok((_, id, _)) = read_at_checked(&handles.parent, r.temp.file_name().unwrap()) {
            if id != handles.temp_id || unlink_at_checked(&handles.parent, r.temp.file_name().unwrap(), id).is_err() {
                conflict = true;
            }
        }
    }
    if !conflict {
        let _ = unlink_at_checked(&lock._file, std::ffi::OsStr::new("transaction.journal"), journal_id);
    }
}

#[cfg(not(unix))]
fn rollback(_project: &Path, _records: &[Record], _lock: &Lock, _journal_id: (u64, u64)) {}

struct Record {
    path: PathBuf,
    temp: PathBuf,
    before: Vec<u8>,
    after: Vec<u8>,
    destination_id: (u64, u64),
    temp_id: (u64, u64),
    #[cfg(unix)]
    handles: Option<UnixRecordHandles>,
}

#[cfg(unix)]
struct UnixRecordHandles {
    _project: File,
    parent: File,
    destination: File,
    temp: File,
    destination_id: (u64, u64),
    temp_id: (u64, u64),
}

#[cfg(unix)]
struct UnixRecoveryHandles {
    parent: File,
    destination: File,
    destination_id: (u64, u64),
    temp: Option<File>,
    temp_id: (u64, u64),
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
                "{{\"path\":\"{}\",\"temp\":\"{}\",\"before\":\"{}\",\"after\":\"{}\",\"destination_id\":[{},{}],\"temp_id\":[{},{}]}}",
                json_escape(&r.path.display().to_string()),
                json_escape(&r.temp.display().to_string()),
                hex(&r.before),
                hex(&r.after), r.destination_id.0, r.destination_id.1, r.temp_id.0, r.temp_id.1
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
            let identity = |key| match o.get(key) {
                Some(super::Json::Value::Array(values)) if values.len() == 2 => {
                    let number = |value: &super::Json::Value| match value {
                        super::Json::Value::Number(value) => *value,
                        _ => fail(&format!("recovery journal `{key}` identity is invalid")),
                    };
                    (number(&values[0]), number(&values[1]))
                }
                _ => fail(&format!("recovery journal missing `{key}` identity")),
            };
            Record {
                path: PathBuf::from(s("path")),
                temp: PathBuf::from(s("temp")),
                before: unhex(&s("before")),
                after: unhex(&s("after")),
                destination_id: identity("destination_id"),
                temp_id: identity("temp_id"),
                #[cfg(unix)]
                handles: None,
            }
        })
        .collect();
    Journal {
        records,
        log_path: PathBuf::from(string("log_path")),
        log: unhex(&string("log")),
    }
}
#[cfg(not(unix))]
fn write_new_sync(path: &Path, bytes: &[u8]) {
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)
        .unwrap_or_else(|e| fail(&format!("could not write `{}`: {e}", path.display())));
    f.write_all(bytes)
        .unwrap_or_else(|e| fail(&format!("could not write `{}`: {e}", path.display())));
    f.sync_all()
        .unwrap_or_else(|e| fail(&format!("could not sync `{}`: {e}", path.display())));
}

#[cfg(unix)]
fn write_new_at(dir: &File, file_name: &std::ffi::OsStr, bytes: &[u8]) -> std::io::Result<(u64, u64)> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    const O_WRONLY: i32 = 1;
    const O_CREAT: i32 = 0o100;
    const O_EXCL: i32 = 0o200;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_NOFOLLOW: i32 = 0o400000;
    extern "C" { fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32; }
    let name = CString::new(file_name.as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in codemod file name"))?;
    let fd = unsafe { openat(dir.as_raw_fd(), name.as_ptr(), O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600) };
    if fd < 0 { return Err(std::io::Error::last_os_error()); }
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes)?;
    file.sync_all()?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "created codemod file is linked or not regular"));
    }
    dir.sync_all()?;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(unix)]
fn unlink_at_checked(dir: &File, file_name: &std::ffi::OsStr, expected: (u64, u64)) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    const O_RDONLY: i32 = 0;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_NOFOLLOW: i32 = 0o400000;
    extern "C" {
        fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32;
        fn unlinkat(dirfd: i32, pathname: *const i8, flags: i32) -> i32;
    }
    let name = CString::new(file_name.as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in codemod file name"))?;
    let fd = unsafe { openat(dir.as_raw_fd(), name.as_ptr(), O_RDONLY | O_NOFOLLOW | O_CLOEXEC, 0) };
    if fd < 0 { return Err(std::io::Error::last_os_error()); }
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata()?;
    if (metadata.dev(), metadata.ino()) != expected || metadata.nlink() != 1 {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "file identity changed before handle-relative removal"));
    }
    if unsafe { unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    dir.sync_all()
}

#[cfg(not(unix))]
fn write_new_at(_dir: &File, _file_name: &std::ffi::OsStr, _bytes: &[u8]) -> std::io::Result<(u64, u64)> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "handle-relative create unavailable"))
}

#[cfg(not(unix))]
fn unlink_at_checked(_dir: &File, _file_name: &std::ffi::OsStr, _expected: (u64, u64)) -> std::io::Result<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "handle-relative removal unavailable"))
}
#[cfg(unix)]
fn replace_journal_generation(
    lock: &Lock,
    tx: &str,
    generation: usize,
    bytes: &[u8],
    expected: Option<(u64, u64)>,
) -> (u64, u64) {
    use std::ffi::{CString, OsStr};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    const O_RDONLY: i32 = 0;
    const O_WRONLY: i32 = 1;
    const O_CREAT: i32 = 0o100;
    const O_EXCL: i32 = 0o200;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_NOFOLLOW: i32 = 0o400000;
    extern "C" {
        fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32;
        fn linkat(olddirfd: i32, oldpath: *const i8, newdirfd: i32, newpath: *const i8, flags: i32) -> i32;
        fn renameat(olddirfd: i32, oldpath: *const i8, newdirfd: i32, newpath: *const i8) -> i32;
        fn unlinkat(dirfd: i32, pathname: *const i8, flags: i32) -> i32;
    }
    fn name(value: &OsStr) -> std::io::Result<CString> {
        CString::new(value.as_bytes()).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in journal name"))
    }
    fn open(dir: &File, name: &CString) -> std::io::Result<File> {
        let fd = unsafe { openat(dir.as_raw_fd(), name.as_ptr(), O_RDONLY | O_NOFOLLOW | O_CLOEXEC, 0) };
        if fd < 0 { return Err(std::io::Error::last_os_error()); }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    let staged_name = name(OsStr::new(&format!(".transaction-{tx}-{generation}.journal.tmp")))
        .unwrap_or_else(|e| fail(&format!("invalid journal stage name: {e}")));
    let journal_name = name(OsStr::new("transaction.journal")).unwrap();
    let fd = unsafe { openat(lock._file.as_raw_fd(), staged_name.as_ptr(), O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600) };
    if fd < 0 { fail(&format!("could not create journal generation: {}", std::io::Error::last_os_error())); }
    let mut staged = unsafe { File::from_raw_fd(fd) };
    staged.write_all(bytes).unwrap_or_else(|e| fail(&format!("could not write journal generation: {e}")));
    staged.sync_all().unwrap_or_else(|e| fail(&format!("could not sync journal generation: {e}")));
    let staged_meta = staged.metadata().unwrap_or_else(|e| fail(&format!("could not identify journal generation: {e}")));
    if !staged_meta.is_file() || staged_meta.nlink() != 1 { fail("journal generation is linked or not regular") }
    let staged_id = (staged_meta.dev(), staged_meta.ino());
    if let Some(expected) = expected {
        let current = open(&lock._file, &journal_name).unwrap_or_else(|e| fail(&format!("could not reopen current journal: {e}")));
        let metadata = current.metadata().unwrap_or_else(|e| fail(&format!("could not identify current journal: {e}")));
        if (metadata.dev(), metadata.ino()) != expected || metadata.nlink() != 1 {
            fail("recovery journal identity changed before generation publish")
        }
        if unsafe { renameat(lock._file.as_raw_fd(), staged_name.as_ptr(), lock._file.as_raw_fd(), journal_name.as_ptr()) } != 0 {
            fail(&format!("could not publish recovery journal generation: {}", std::io::Error::last_os_error()));
        }
    } else {
        if unsafe { linkat(lock._file.as_raw_fd(), staged_name.as_ptr(), lock._file.as_raw_fd(), journal_name.as_ptr(), 0) } != 0 {
            fail(&format!("could not publish first recovery journal: {}", std::io::Error::last_os_error()));
        }
        if unsafe { unlinkat(lock._file.as_raw_fd(), staged_name.as_ptr(), 0) } != 0 {
            fail(&format!("could not remove linked journal stage: {}", std::io::Error::last_os_error()));
        }
    }
    let published = open(&lock._file, &journal_name).unwrap_or_else(|e| fail(&format!("could not reopen published journal: {e}")));
    let metadata = published.metadata().unwrap_or_else(|e| fail(&format!("could not identify published journal: {e}")));
    if (metadata.dev(), metadata.ino()) != staged_id || metadata.nlink() != 1 {
        fail("published recovery journal identity does not match staged generation")
    }
    lock._file.sync_all().unwrap_or_else(|e| fail(&format!("could not sync codemod directory: {e}")));
    staged_id
}

#[cfg(not(unix))]
fn replace_journal_generation(_lock: &Lock, _tx: &str, _generation: usize, _bytes: &[u8], _expected: Option<(u64, u64)>) -> (u64, u64) {
    fail("handle-relative journal publication is unavailable on this platform")
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

    let destination_identity = path_identity(path)?;
    let temp_identity = path_identity(temp)?;
    if destination_identity == temp_identity {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "temp aliases destination"));
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
    use std::os::unix::fs::MetadataExt;
    if !destination.metadata()?.is_file() || destination.metadata()?.nlink() != 1 {
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
    let temp_check = unsafe { openat(dirfd, temp_name.as_ptr(), O_RDONLY | O_NOFOLLOW | O_CLOEXEC, 0) };
    if temp_check < 0 { return Err(std::io::Error::last_os_error()); }
    let temp_check = unsafe { File::from_raw_fd(temp_check) };
    if temp_check.metadata()?.nlink() != 1 {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "temp has multiple links"));
    }
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
    if path_identity(path)? == path_identity(temp)? {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "temp aliases destination"));
    }
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
