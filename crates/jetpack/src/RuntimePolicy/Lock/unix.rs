use std::fs::File;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
))]
mod supported {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::os::fd::AsRawFd as _;
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};

    const F_GETFD: i32 = 1;
    const F_SETFD: i32 = 2;
    const FD_CLOEXEC: i32 = 1;
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    const LOCK_UN: i32 = 8;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_NOFOLLOW: i32 = 0o400000;
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    const O_NOFOLLOW: i32 = 0x0000_0100;

    unsafe extern "C" {
        fn fcntl(fd: i32, command: i32, ...) -> i32;
        fn flock(fd: i32, operation: i32) -> i32;
    }

    pub struct LockFile {
        // Authority lives on canonical managed-root directory inode. Replacing
        // every name under `.locks` cannot create second kernel lock domain.
        owner: File,
        marker: File,
        owner_path: PathBuf,
        marker_path: PathBuf,
    }

    pub fn open(path: &Path) -> io::Result<LockFile> {
        open_mode(path, true)
    }

    pub fn open_existing(path: &Path) -> io::Result<LockFile> {
        open_mode(path, false)
    }

    fn open_mode(path: &Path, create: bool) -> io::Result<LockFile> {
        let owner_path = canonical_owner(path)?;
        let owner = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW)
            .open(&owner_path)?;
        set_close_on_exec(&owner)?;
        let marker = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .custom_flags(O_NOFOLLOW)
            .open(path)?;
        set_close_on_exec(&marker)?;
        let lock = LockFile {
            owner,
            marker,
            owner_path,
            marker_path: path.to_path_buf(),
        };
        validate_path(&lock, path)?;
        Ok(lock)
    }

    fn canonical_owner(path: &Path) -> io::Result<PathBuf> {
        let lock_dir = path.parent().ok_or_else(|| {
            io::Error::new(ErrorKind::InvalidInput, "jetpack lock path has no directory")
        })?;
        let root = lock_dir.parent().ok_or_else(|| {
            io::Error::new(ErrorKind::InvalidInput, "jetpack lock path has no managed root")
        })?;
        fs::canonicalize(root)
    }

    fn validate_identity(file: &File, path: &Path, directory: bool) -> io::Result<()> {
        let path_metadata = fs::symlink_metadata(path)?;
        let file_metadata = file.metadata()?;
        let expected_kind = if directory {
            path_metadata.file_type().is_dir() && file_metadata.file_type().is_dir()
        } else {
            path_metadata.file_type().is_file() && file_metadata.file_type().is_file()
        };
        if !expected_kind
            || path_metadata.dev() != file_metadata.dev()
            || path_metadata.ino() != file_metadata.ino()
        {
            return Err(io::Error::other(format!(
                "jetpack lock ownership path `{}` changed",
                path.display()
            )));
        }
        Ok(())
    }

    pub fn validate_path(lock: &LockFile, path: &Path) -> io::Result<()> {
        if path != lock.marker_path {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "jetpack lock validation used a different marker path",
            ));
        }
        validate_identity(&lock.owner, &lock.owner_path, true)?;
        validate_identity(&lock.marker, path, false)
    }

    fn set_close_on_exec(file: &File) -> io::Result<()> {
        let fd = file.as_raw_fd();
        // SAFETY: fd belongs to live File; commands change descriptor flags.
        let flags = unsafe { fcntl(fd, F_GETFD) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fd remains live; F_SETFD consumes integer flags only.
        if unsafe { fcntl(fd, F_SETFD, flags | FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn try_lock(lock: &LockFile) -> io::Result<bool> {
        // SAFETY: owner is live canonical-root directory descriptor.
        if unsafe { flock(lock.owner.as_raw_fd(), LOCK_EX | LOCK_NB) } == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.kind() == ErrorKind::WouldBlock {
            Ok(false)
        } else {
            Err(error)
        }
    }

    pub fn unlock(lock: &LockFile) -> io::Result<()> {
        // SAFETY: owner is live descriptor locked by flock above.
        if unsafe { flock(lock.owner.as_raw_fd(), LOCK_UN) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
))]
pub(super) use supported::LockFile;

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
))]
pub(super) use supported::{open, open_existing, try_lock, unlock, validate_path};

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
)))]
pub(super) struct LockFile;

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
)))]
fn unsupported() -> io::Error {
    io::Error::new(
        ErrorKind::Unsupported,
        "Jetpack advisory locks are unsupported on this Unix target",
    )
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
)))]
pub(super) fn open(_path: &Path) -> io::Result<LockFile> {
    Err(unsupported())
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
)))]
pub(super) fn open_existing(_path: &Path) -> io::Result<LockFile> {
    Err(unsupported())
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
)))]
pub(super) fn validate_path(_file: &LockFile, _path: &Path) -> io::Result<()> {
    Err(unsupported())
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
)))]
pub(super) fn try_lock(_file: &LockFile) -> io::Result<bool> {
    Err(unsupported())
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
)))]
pub(super) fn unlock(_file: &LockFile) -> io::Result<()> {
    Err(unsupported())
}
