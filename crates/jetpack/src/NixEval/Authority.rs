//! Pinned, read-only authority for bounded project-relative Nix imports.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path};

const MAX_IMPORT_BYTES: u64 = 1 << 20;

pub(crate) struct ProjectImportAuthority(PlatformAuthority);

impl ProjectImportAuthority {
    pub(crate) fn open(root: &Path) -> io::Result<Self> {
        PlatformAuthority::open(root).map(Self)
    }

    pub(crate) fn read(&self, relative: &str) -> io::Result<String> {
        validate_relative_path(relative)?;
        let file = self
            .0
            .open_read(Path::new(relative))
            .map_err(reject_symlink_import)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::other("project import is not a regular file"));
        }
        if metadata.len() > MAX_IMPORT_BYTES {
            return Err(io::Error::other(
                "project import exceeds the 1 MiB evaluator limit",
            ));
        }
        let mut source = String::new();
        file.take(MAX_IMPORT_BYTES + 1).read_to_string(&mut source)?;
        if source.len() as u64 > MAX_IMPORT_BYTES {
            return Err(io::Error::other(
                "project import exceeds the 1 MiB evaluator limit",
            ));
        }
        Ok(source)
    }
}

fn reject_symlink_import(error: io::Error) -> io::Error {
    if is_symlink_error(&error) {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "project import symlinks are not allowed by project-root authority",
        )
    } else {
        error
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn is_symlink_error(error: &io::Error) -> bool {
    error.raw_os_error() == Some(40)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn is_symlink_error(error: &io::Error) -> bool {
    error.raw_os_error() == Some(62)
}

#[cfg(windows)]
fn is_symlink_error(_error: &io::Error) -> bool {
    false
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    windows
)))]
fn is_symlink_error(_error: &io::Error) -> bool {
    false
}

fn validate_relative_path(relative: &str) -> io::Result<()> {
    let path = Path::new(relative);
    if relative.is_empty()
        || path.is_absolute()
        || relative.contains('\\')
        || relative.contains('\0')
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "import path is not a normalized relative project path",
        ));
    }
    Ok(())
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
mod platform {
    use super::*;
    use std::ffi::{c_char, CString};
    use std::fs::OpenOptions;
    use std::os::fd::{AsRawFd as _, FromRawFd as _};
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    const O_RDONLY: i32 = 0;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_CLOEXEC: i32 = 0o2000000;
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

    unsafe extern "C" {
        fn openat(directory: i32, path: *const c_char, flags: i32, ...) -> i32;
    }

    pub(super) struct PlatformAuthority {
        root: File,
    }

    impl PlatformAuthority {
        pub(super) fn open(path: &Path) -> io::Result<Self> {
            let root = OpenOptions::new()
                .read(true)
                .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
                .open(path)?;
            if !root.metadata()?.is_dir() {
                return Err(io::Error::other("project import authority is not a directory"));
            }
            Ok(Self { root })
        }

        pub(super) fn open_read(&self, relative: &Path) -> io::Result<File> {
            let components = relative.components().collect::<Vec<_>>();
            let mut directory = self.root.try_clone()?;
            for (index, component) in components.iter().enumerate() {
                let Component::Normal(name) = component else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "import path has unsupported components",
                    ));
                };
                let name = CString::new(name.as_bytes()).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "import path contains NUL")
                })?;
                let last = index + 1 == components.len();
                let flags = if last {
                    O_RDONLY | O_NOFOLLOW | O_CLOEXEC
                } else {
                    O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC
                };
                let fd = unsafe { openat(directory.as_raw_fd(), name.as_ptr(), flags, 0) };
                if fd < 0 {
                    return Err(io::Error::last_os_error());
                }
                let opened = unsafe { File::from_raw_fd(fd) };
                if last {
                    return Ok(opened);
                }
                directory = opened;
            }
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "import path is empty",
            ))
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;

    // Windows has no stable standard-library handle-relative open primitive.
    // Keep this path fail-closed until one exists; never fall back to a
    // pathname traversal that can cross a reparse point during a race.
    pub(super) struct PlatformAuthority;

    impl PlatformAuthority {
        pub(super) fn open(_path: &Path) -> io::Result<Self> {
            Ok(Self)
        }

        pub(super) fn open_read(&self, _relative: &Path) -> io::Result<File> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "project-relative imports are unsupported on Windows until handle-relative authority is available",
            ))
        }
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    windows
)))]
mod platform {
    use super::*;

    pub(super) struct PlatformAuthority;

    impl PlatformAuthority {
        pub(super) fn open(_path: &Path) -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "project import authority is unsupported on this platform",
            ))
        }

        pub(super) fn open_read(&self, _relative: &Path) -> io::Result<File> {
            unreachable!("unsupported project import authority cannot read")
        }
    }
}

use platform::PlatformAuthority;
