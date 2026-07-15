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
use std::os::unix::fs::OpenOptionsExt as _;

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
    const O_NOFOLLOW: i32 = 0x00000100;

    unsafe extern "C" {
        fn fcntl(fd: i32, command: i32, ...) -> i32;
        fn flock(fd: i32, operation: i32) -> i32;
    }

    pub(super) fn open(path: &Path) -> io::Result<File> {
        let file = open_file(path, true)?;
        validate_direct(&file, path)?;
        let anchor = anchor_path(path);
        match fs::hard_link(path, &anchor) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        validate_path(&file, path)?;
        Ok(file)
    }

    pub(super) fn open_existing(path: &Path) -> io::Result<File> {
        let file = open_file(path, false)?;
        validate_direct(&file, path)?;
        if anchor_path(path).exists() {
            validate_path(&file, path)?;
        }
        Ok(file)
    }

    fn open_file(path: &Path, create: bool) -> io::Result<File> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            // One atomic kernel operation: a symlink can never be followed
            // between a user-space preflight and open.
            .custom_flags(O_NOFOLLOW)
            .open(path)?;
        set_close_on_exec(&file)?;
        Ok(file)
    }

    fn anchor_path(path: &Path) -> std::path::PathBuf {
        let mut name = path.as_os_str().to_os_string();
        name.push(".anchor");
        std::path::PathBuf::from(name)
    }

    fn validate_direct(file: &File, path: &Path) -> io::Result<()> {
        let path_metadata = fs::symlink_metadata(path)?;
        let file_metadata = file.metadata()?;
        if !path_metadata.file_type().is_file()
            || !file_metadata.file_type().is_file()
            || path_metadata.dev() != file_metadata.dev()
            || path_metadata.ino() != file_metadata.ino()
        {
            return Err(io::Error::other(format!(
                "jetpack lock path `{}` is not the opened regular file",
                path.display()
            )));
        }
        Ok(())
    }

    pub(super) fn validate_path(file: &File, path: &Path) -> io::Result<()> {
        validate_direct(file, path)?;
        let anchor = anchor_path(path);
        let anchor_metadata = fs::symlink_metadata(&anchor)?;
        let file_metadata = file.metadata()?;
        if !anchor_metadata.file_type().is_file()
            || anchor_metadata.dev() != file_metadata.dev()
            || anchor_metadata.ino() != file_metadata.ino()
            || anchor_metadata.nlink() != 2
            || file_metadata.nlink() != 2
        {
            return Err(io::Error::other(format!(
                "jetpack lock path `{}` was linked or replaced",
                path.display()
            )));
        }
        Ok(())
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
pub(super) fn open_existing(path: &Path) -> io::Result<File> {
    supported::open_existing(path)
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
pub(super) fn validate_path(file: &File, path: &Path) -> io::Result<()> {
    supported::validate_path(file, path)
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
pub(super) fn open_existing(_path: &Path) -> io::Result<File> {
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
pub(super) fn validate_path(_file: &File, _path: &Path) -> io::Result<()> {
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
