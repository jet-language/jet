use std::fs::File;
use std::io::{self, ErrorKind};
use std::path::Path;

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
use std::fs::{self, OpenOptions};
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
use std::os::fd::AsRawFd as _;
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
use std::os::unix::fs::MetadataExt as _;

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

    const F_GETFD: i32 = 1;
    const F_SETFD: i32 = 2;
    const FD_CLOEXEC: i32 = 1;
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    const LOCK_UN: i32 = 8;

    unsafe extern "C" {
        fn fcntl(fd: i32, command: i32, ...) -> i32;
        fn flock(fd: i32, operation: i32) -> i32;
    }

    pub(super) fn open(path: &Path) -> io::Result<File> {
        if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("jetpack lock path `{}` is a symlink", path.display()),
            ));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        set_close_on_exec(&file)?;

        // A path swap between preflight and open must fail closed. Holding the
        // returned descriptor then pins this inode until unlock/close.
        let path_metadata = fs::metadata(path)?;
        let file_metadata = file.metadata()?;
        if path_metadata.dev() != file_metadata.dev() || path_metadata.ino() != file_metadata.ino() {
            return Err(io::Error::other(format!(
                "jetpack lock path `{}` changed while opening",
                path.display()
            )));
        }
        Ok(file)
    }

    fn set_close_on_exec(file: &File) -> io::Result<()> {
        let fd = file.as_raw_fd();
        // SAFETY: `fd` belongs to live `File`; F_GETFD takes no variadic arg and
        // does not access memory.
        let flags = unsafe { fcntl(fd, F_GETFD) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` remains live; F_SETFD consumes one integer flag value.
        if unsafe { fcntl(fd, F_SETFD, flags | FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn try_lock(file: &File) -> io::Result<bool> {
        // SAFETY: `file` owns a live descriptor. `flock` changes only kernel
        // advisory-lock state for that descriptor and touches no Rust memory.
        if unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.kind() == ErrorKind::WouldBlock {
            Ok(false)
        } else {
            Err(error)
        }
    }

    pub(super) fn unlock(file: &File) -> io::Result<()> {
        // SAFETY: `file` owns a live descriptor; LOCK_UN releases only its
        // kernel advisory lock.
        if unsafe { flock(file.as_raw_fd(), LOCK_UN) } == 0 {
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
pub(super) fn open(path: &Path) -> io::Result<File> {
    supported::open(path)
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
pub(super) fn try_lock(file: &File) -> io::Result<bool> {
    supported::try_lock(file)
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
pub(super) fn unlock(file: &File) -> io::Result<()> {
    supported::unlock(file)
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
pub(super) fn open(_path: &Path) -> io::Result<File> {
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
pub(super) fn try_lock(_file: &File) -> io::Result<bool> {
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
pub(super) fn unlock(_file: &File) -> io::Result<()> {
    Err(unsupported())
}
