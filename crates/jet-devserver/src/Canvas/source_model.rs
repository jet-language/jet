use std::io;
use std::path::Path;
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
use std::sync::atomic::AtomicU64;
use std::sync::{Mutex, OnceLock};

use jet_driver::SHA256;

static SOURCE_TRANSACTION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Stable source revision used by Canvas source transactions and projections.
pub fn source_revision(src: &str) -> String {
    format!("sha256-{}", SHA256::sha256_hex(src.as_bytes()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplaceResult {
    Applied,
    Conflict,
}

/// Read Canvas source from the object opened below a pinned directory.
pub(crate) fn read_source_without_symlinks(path: &Path) -> io::Result<String> {
    let bytes = read_file_without_symlinks(path)?;
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Read an arbitrary file from the object opened below a pinned directory.
/// Binary web artifacts use the same no-follow boundary as source reads.
pub(crate) fn read_file_without_symlinks(path: &Path) -> io::Result<Vec<u8>> {
    read_file_without_symlinks_bounded(path, crate::MAX_RESPONSE_BODY_BYTES as u64)
}

/// Read an arbitrary file without buffering more than the caller's limit.
pub(crate) fn read_file_without_symlinks_bounded(
    path: &Path,
    max_bytes: u64,
) -> io::Result<Vec<u8>> {
    secure_fs::read_bounded(path, max_bytes)
}

/// Create a directory tree through pinned directory descriptors.
pub(super) fn ensure_no_symlink_directory(path: &Path) -> io::Result<()> {
    secure_fs::ensure_directory(path)
}

/// Create and write a new file below a pinned parent. Callers that need
/// collision handling can retry on AlreadyExists.
pub(super) fn write_new_file_without_symlinks(path: &Path, bytes: &[u8]) -> io::Result<()> {
    secure_fs::write_new(path, bytes)
}

#[derive(Debug)]
pub(super) enum SourceWriteError {
    Conflict,
    Io(io::Error),
}

impl From<io::Error> for SourceWriteError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(super) fn with_source_transaction<T>(
    action: impl FnOnce() -> Result<T, SourceWriteError>,
) -> Result<T, SourceWriteError> {
    let lock = SOURCE_TRANSACTION_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().map_err(|_| {
        SourceWriteError::Io(io::Error::new(
            io::ErrorKind::Other,
            "Canvas source transaction lock was poisoned",
        ))
    })?;
    action()
}

pub(super) fn write_source_if_unchanged(
    path: &Path,
    expected: &str,
    candidate: &str,
) -> Result<(), SourceWriteError> {
    with_source_transaction(|| {
        replace_source_if_unchanged_locked(path, Some(expected), Some(candidate))
    })
}

pub(super) fn replace_source_if_unchanged_locked(
    path: &Path,
    expected: Option<&str>,
    candidate: Option<&str>,
) -> Result<(), SourceWriteError> {
    match secure_fs::replace_if_unchanged(
        path,
        expected.map(str::as_bytes),
        candidate.map(str::as_bytes),
    )
    .map_err(SourceWriteError::Io)?
    {
        ReplaceResult::Applied => Ok(()),
        ReplaceResult::Conflict => Err(SourceWriteError::Conflict),
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
mod secure_fs {
    use super::{io, Path, ReplaceResult, TEMP_FILE_SEQUENCE};
    use std::ffi::{c_char, CString, OsStr};
    use std::fs::{self, File};
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::Component;
    use std::sync::atomic::Ordering;

    const O_RDONLY: i32 = 0;
    const O_WRONLY: i32 = 1;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_CREAT: i32 = 0o100;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_EXCL: i32 = 0o200;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_CLOEXEC: i32 = 0o2000000;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_CREAT: i32 = 0x0200;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_EXCL: i32 = 0x0800;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_CLOEXEC: i32 = 0x01000000;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_DIRECTORY: i32 = 0o200000;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_DIRECTORY: i32 = 0x00100000;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_NOFOLLOW: i32 = 0o400000;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_NOFOLLOW: i32 = 0x0100;
    const MODE_NEW_FILE: u32 = 0o666;

    unsafe extern "C" {
        fn openat(directory: i32, path: *const c_char, flags: i32, ...) -> i32;
        fn mkdirat(directory: i32, path: *const c_char, mode: u32) -> i32;
        fn renameat(
            old_directory: i32,
            old_path: *const c_char,
            new_directory: i32,
            new_path: *const c_char,
        ) -> i32;
        fn unlinkat(directory: i32, path: *const c_char, flags: i32) -> i32;
    }

    struct CurrentFile {
        bytes: Vec<u8>,
        mode: u32,
    }

    struct Target {
        parent: File,
        name: CString,
        current: Option<CurrentFile>,
    }

    pub(super) fn read_bounded(path: &Path, max_bytes: u64) -> io::Result<Vec<u8>> {
        let (parent, name) = open_parent(path)?;
        let mut file = open_file_at(&parent, &name, O_RDONLY)?;
        Ok(read_opened_file_bounded(&mut file, max_bytes, || Ok(()))?.bytes)
    }

    pub(super) fn ensure_directory(path: &Path) -> io::Result<()> {
        let mut directory = open_base(path.is_absolute())?;
        for component in path.components() {
            match component {
                Component::RootDir | Component::CurDir => {}
                Component::ParentDir => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Canvas paths must not contain parent components",
                    ));
                }
                Component::Normal(name) => {
                    let c_name = c_name(name)?;
                    match open_directory_at(&directory, name) {
                        Ok(child) => directory = child,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {
                            if unsafe { mkdirat(directory.as_raw_fd(), c_name.as_ptr(), 0o777) }
                                != 0
                            {
                                let create_error = io::Error::last_os_error();
                                if create_error.kind() != io::ErrorKind::AlreadyExists {
                                    return Err(map_nofollow(create_error));
                                }
                            }
                            directory = open_directory_at(&directory, name)?;
                        }
                        Err(error) => return Err(error),
                    }
                }
                Component::Prefix(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Canvas path has an unsupported prefix",
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
        let (parent, name) = open_parent(path)?;
        let mut file = create_new_at(&parent, &name, MODE_NEW_FILE)?;
        if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
            let _ = remove_at(&parent, &name);
            return Err(error);
        }
        require_regular(&file)
    }

    pub(super) fn replace_if_unchanged(
        path: &Path,
        expected: Option<&[u8]>,
        candidate: Option<&[u8]>,
    ) -> io::Result<ReplaceResult> {
        let target = open_target(path)?;
        let current = target.current.as_ref();
        let matches = match expected {
            Some(expected) => current.is_some_and(|current| current.bytes.as_slice() == expected),
            None => current.is_none(),
        };
        if !matches {
            return Ok(ReplaceResult::Conflict);
        }
        match candidate {
            Some(candidate)
                if current.is_some_and(|current| current.bytes.as_slice() != candidate) =>
            {
                target.replace(candidate, current.map(|current| current.mode))?;
            }
            Some(_) => {}
            None if current.is_some() => target.remove()?,
            None => {}
        }
        Ok(ReplaceResult::Applied)
    }

    fn open_target(path: &Path) -> io::Result<Target> {
        let (parent, name) = open_parent(path)?;
        let current = match open_file_at(&parent, &name, O_RDONLY) {
            Ok(mut file) => Some(read_opened_file(&mut file, || Ok(()))?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(Target {
            parent,
            name,
            current,
        })
    }

    impl Target {
        fn replace(&self, bytes: &[u8], mode: Option<u32>) -> io::Result<()> {
            let mut temporary = None;
            for _ in 0..100 {
                let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
                let name = format!(
                    ".{}.canvas-{}.tmp",
                    self.name.to_string_lossy(),
                    sequence
                );
                let name = c_name(OsStr::new(&name))?;
                match create_new_at(
                    &self.parent,
                    &name,
                    mode.map_or(MODE_NEW_FILE, |mode| mode & 0o7777),
                ) {
                    Ok(mut file) => {
                        let result = file.write_all(bytes).and_then(|_| file.sync_all());
                        let result = result.and_then(|()| {
                            mode.map_or(Ok(()), |mode| {
                                file.set_permissions(fs::Permissions::from_mode(mode & 0o7777))
                                    .and_then(|()| file.sync_all())
                            })
                        });
                        if let Err(error) = result {
                            let _ = remove_at(&self.parent, &name);
                            return Err(error);
                        }
                        temporary = Some(name);
                        break;
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                }
            }
            let Some(temporary) = temporary else {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "could not allocate a Canvas source temporary file",
                ));
            };
            if unsafe {
                renameat(
                    self.parent.as_raw_fd(),
                    temporary.as_ptr(),
                    self.parent.as_raw_fd(),
                    self.name.as_ptr(),
                )
            } != 0
            {
                let error = map_nofollow(io::Error::last_os_error());
                let _ = remove_at(&self.parent, &temporary);
                return Err(error);
            }
            let _ = self.parent.sync_all();
            Ok(())
        }

        fn remove(&self) -> io::Result<()> {
            remove_at(&self.parent, &self.name)
        }
    }

    fn open_base(absolute: bool) -> io::Result<File> {
        let base = if absolute { Path::new("/") } else { Path::new(".") };
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            .open(base)
    }

    fn open_parent(path: &Path) -> io::Result<(File, CString)> {
        let components = path.components().collect::<Vec<_>>();
        let Some(Component::Normal(name)) = components.last() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Canvas source path has no file name",
            ));
        };
        let mut parent = open_base(path.is_absolute())?;
        for component in &components[..components.len() - 1] {
            parent = match component {
                Component::RootDir | Component::CurDir => parent,
                Component::ParentDir => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Canvas paths must not contain parent components",
                    ));
                }
                Component::Normal(name) => open_directory_at(&parent, name)?,
                Component::Prefix(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Canvas source path has an unsupported prefix",
                    ));
                }
            };
        }
        Ok((parent, c_name(name)?))
    }

    fn c_name(name: &OsStr) -> io::Result<CString> {
        CString::new(name.as_bytes()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "Canvas path contains NUL")
        })
    }

    fn open_directory_at(parent: &File, name: &OsStr) -> io::Result<File> {
        open_file_at(parent, &c_name(name)?, O_DIRECTORY)
    }

    fn open_file_at(parent: &File, name: &CString, extra_flags: i32) -> io::Result<File> {
        let fd = unsafe {
            openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                extra_flags | O_NOFOLLOW | O_CLOEXEC,
                0,
            )
        };
        if fd < 0 {
            return Err(map_nofollow(io::Error::last_os_error()));
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn create_new_at(parent: &File, name: &CString, mode: u32) -> io::Result<File> {
        let fd = unsafe {
            openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
                mode,
            )
        };
        if fd < 0 {
            return Err(map_nofollow(io::Error::last_os_error()));
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn remove_at(parent: &File, name: &CString) -> io::Result<()> {
        if unsafe { unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(map_nofollow(io::Error::last_os_error()));
        }
        Ok(())
    }

    fn require_regular(file: &File) -> io::Result<()> {
        require_regular_metadata(&file.metadata()?)
    }

    fn read_opened_file(
        file: &mut File,
        before_read: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<CurrentFile> {
        read_opened_file_bounded(file, crate::MAX_RESPONSE_BODY_BYTES as u64, before_read)
    }

    fn read_opened_file_bounded(
        file: &mut File,
        max_bytes: u64,
        before_read: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<CurrentFile> {
        let metadata = file.metadata()?;
        require_regular_metadata(&metadata)?;
        if metadata.len() > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file exceeds the devserver response limit",
            ));
        }
        let mode = metadata.permissions().mode();
        before_read()?;
        let mut bytes = Vec::new();
        file.take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file exceeds the devserver response limit",
            ));
        }
        let final_metadata = file.metadata()?;
        if !same_file(&metadata, &final_metadata) {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Canvas source changed while it was being read",
            ));
        }
        Ok(CurrentFile { bytes, mode })
    }

    fn require_regular_metadata(metadata: &std::fs::Metadata) -> io::Result<()> {
        if metadata.is_file() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Canvas source path is not a regular file",
            ))
        }
    }

    fn same_file(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
        left.is_file()
            && right.is_file()
            && left.dev() == right.dev()
            && left.ino() == right.ino()
    }

    fn map_nofollow(error: io::Error) -> io::Error {
        let symlink_errno = if cfg!(any(target_os = "linux", target_os = "android")) {
            40
        } else {
            62
        };
        if error.raw_os_error() == Some(symlink_errno) {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Canvas source paths must not traverse symlinks",
            )
        } else {
            error
        }
    }

    #[cfg(test)]
    pub(super) fn test_replace_after_final_symlink_swap(
        path: &Path,
        outside: &Path,
        candidate: &[u8],
    ) -> io::Result<()> {
        use std::os::unix::fs::symlink;

        let target = open_target(path)?;
        let moved = path.with_file_name("main.jet.canvas-test-held");
        std::fs::rename(path, &moved)?;
        symlink(outside, path)?;
        target.replace(
            candidate,
            target.current.as_ref().map(|current| current.mode),
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn test_replace_after_ancestor_symlink_swap(
        path: &Path,
        outside: &Path,
        candidate: &[u8],
    ) -> io::Result<()> {
        use std::os::unix::fs::symlink;

        let target = open_target(path)?;
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "test path has no parent")
        })?;
        let moved = parent.with_file_name("canvas-test-held");
        std::fs::rename(parent, &moved)?;
        symlink(outside, parent)?;
        target.replace(
            candidate,
            target.current.as_ref().map(|current| current.mode),
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn test_read_after_final_symlink_swap(
        path: &Path,
        outside: &Path,
    ) -> io::Result<Vec<u8>> {
        use std::os::unix::fs::symlink;

        let (parent, name) = open_parent(path)?;
        let mut file = open_file_at(&parent, &name, O_RDONLY)?;
        let moved = path.with_file_name("main.jet.canvas-test-held");
        let current = read_opened_file(&mut file, || {
            std::fs::rename(path, &moved)?;
            symlink(outside, path)
        })?;
        Ok(current.bytes)
    }

    #[cfg(test)]
    pub(super) fn test_remove_after_final_symlink_swap(
        path: &Path,
        outside: &Path,
    ) -> io::Result<()> {
        use std::os::unix::fs::symlink;

        let target = open_target(path)?;
        let moved = path.with_file_name("main.jet.canvas-test-held");
        std::fs::rename(path, &moved)?;
        symlink(outside, path)?;
        target.remove()
    }

    #[cfg(test)]
    pub(super) fn test_remove_after_ancestor_symlink_swap(
        path: &Path,
        outside: &Path,
    ) -> io::Result<()> {
        use std::os::unix::fs::symlink;

        let target = open_target(path)?;
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "test path has no parent")
        })?;
        let moved = parent.with_file_name("canvas-test-held");
        std::fs::rename(parent, &moved)?;
        symlink(outside, parent)?;
        target.remove()
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
)))]
mod secure_fs {
    use super::{io, Path, ReplaceResult};

    fn unsupported<T>() -> io::Result<T> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Canvas descriptor-relative no-follow filesystem access is unavailable on this platform",
        ))
    }

    pub(super) fn read_bounded(_: &Path, _: u64) -> io::Result<Vec<u8>> {
        unsupported()
    }

    pub(super) fn ensure_directory(_: &Path) -> io::Result<()> {
        unsupported()
    }

    pub(super) fn write_new(_: &Path, _: &[u8]) -> io::Result<()> {
        unsupported()
    }

    pub(super) fn replace_if_unchanged(
        _: &Path,
        _: Option<&[u8]>,
        _: Option<&[u8]>,
    ) -> io::Result<ReplaceResult> {
        unsupported()
    }
}

#[cfg(all(
    test,
    any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    )
))]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_path() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "jet-canvas-source-model-{}-{}",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root.join("main.jet")
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn compare_and_publish_preserves_source_on_conflict() {
        let path = test_path();
        fs::write(&path, "before\n").unwrap();

        write_source_if_unchanged(&path, "before\n", "after\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "after\n");

        let error = write_source_if_unchanged(&path, "before\n", "lost\n").unwrap_err();
        assert!(matches!(error, SourceWriteError::Conflict));
        assert_eq!(fs::read_to_string(&path).unwrap(), "after\n");

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn compare_and_publish_preserves_source_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let path = test_path();
        fs::write(&path, "before\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();

        replace_source_if_unchanged_locked(&path, Some("before\n"), Some("after\n"))
            .unwrap();

        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o7777, 0o640);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn compare_and_publish_supports_new_and_removed_source() {
        let path = test_path();
        replace_source_if_unchanged_locked(&path, None, Some("new\n")).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
        replace_source_if_unchanged_locked(&path, Some("new\n"), None).unwrap();
        assert!(!path.exists());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn compare_and_publish_rejects_symlinked_source() {
        use std::os::unix::fs::symlink;

        let path = test_path();
        let outside = path.parent().unwrap().join("outside.jet");
        fs::write(&outside, "must survive\n").unwrap();
        symlink(&outside, &path).unwrap();

        let error = write_source_if_unchanged(&path, "must survive\n", "attacker\n")
            .expect_err("Canvas source writes must not follow symlinks");
        assert!(matches!(error, SourceWriteError::Io(_)));
        assert_eq!(fs::read_to_string(&outside).unwrap(), "must survive\n");

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn compare_and_publish_rejects_symlinked_parent() {
        use std::os::unix::fs::symlink;

        let root_file = test_path();
        let root = root_file.parent().unwrap();
        let outside = root.with_file_name(format!(
            "jet-canvas-source-outside-{}",
            std::process::id()
        ));
        fs::create_dir_all(&outside).unwrap();
        let outside_file = outside.join("main.jet");
        fs::write(&outside_file, "must survive\n").unwrap();
        let linked = root.join("linked");
        symlink(&outside, &linked).unwrap();
        let path = linked.join("main.jet");

        let error = write_source_if_unchanged(&path, "must survive\n", "attacker\n")
            .expect_err("Canvas source writes must not traverse symlinked parents");
        assert!(matches!(error, SourceWriteError::Io(_)));
        assert_eq!(fs::read_to_string(&outside_file).unwrap(), "must survive\n");

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn compare_and_publish_keeps_final_swap_inside_pinned_parent() {
        let path = test_path();
        let outside = path.parent().unwrap().join("outside.jet");
        fs::write(&path, "before\n").unwrap();
        fs::write(&outside, "must survive\n").unwrap();

        secure_fs::test_replace_after_final_symlink_swap(&path, &outside, b"after\n")
            .unwrap();

        assert_eq!(fs::read_to_string(&outside).unwrap(), "must survive\n");
        assert_eq!(fs::read_to_string(&path).unwrap(), "after\n");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn compare_and_read_stays_on_opened_file_after_final_swap() {
        let path = test_path();
        let outside = path.parent().unwrap().join("outside.jet");
        fs::write(&path, "before\n").unwrap();
        fs::write(&outside, "must survive\n").unwrap();

        let bytes = secure_fs::test_read_after_final_symlink_swap(&path, &outside).unwrap();

        assert_eq!(bytes, b"before\n");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "must survive\n");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn compare_and_publish_keeps_ancestor_swap_inside_pinned_parent() {
        let root_file = test_path();
        let root = root_file.parent().unwrap();
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        let path = nested.join("main.jet");
        fs::write(&path, "before\n").unwrap();
        let outside = root.with_file_name(format!(
            "{}-outside",
            root.file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir_all(&outside).unwrap();
        let outside_file = outside.join("main.jet");
        fs::write(&outside_file, "must survive\n").unwrap();

        secure_fs::test_replace_after_ancestor_symlink_swap(&path, &outside, b"after\n")
            .unwrap();

        assert_eq!(fs::read_to_string(&outside_file).unwrap(), "must survive\n");
        assert_eq!(
            fs::read_to_string(root.join("canvas-test-held/main.jet")).unwrap(),
            "after\n"
        );
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn compare_and_remove_keeps_final_swap_inside_pinned_parent() {
        let path = test_path();
        let outside = path.parent().unwrap().join("outside.jet");
        fs::write(&path, "before\n").unwrap();
        fs::write(&outside, "must survive\n").unwrap();
        let moved = path.with_file_name("main.jet.canvas-test-held");

        secure_fs::test_remove_after_final_symlink_swap(&path, &outside).unwrap();

        assert!(fs::symlink_metadata(&path).is_err());
        assert_eq!(fs::read_to_string(&moved).unwrap(), "before\n");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "must survive\n");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn compare_and_remove_keeps_ancestor_swap_inside_pinned_parent() {
        let root_file = test_path();
        let root = root_file.parent().unwrap();
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        let path = nested.join("main.jet");
        fs::write(&path, "before\n").unwrap();
        let outside = root.with_file_name(format!(
            "{}-outside",
            root.file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir_all(&outside).unwrap();
        let outside_file = outside.join("main.jet");
        fs::write(&outside_file, "must survive\n").unwrap();

        secure_fs::test_remove_after_ancestor_symlink_swap(&path, &outside).unwrap();

        assert!(!root.join("canvas-test-held/main.jet").exists());
        assert_eq!(fs::read_to_string(&outside_file).unwrap(), "must survive\n");
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
