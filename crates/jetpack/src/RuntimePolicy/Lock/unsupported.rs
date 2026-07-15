use std::io::{self, ErrorKind};
use std::path::Path;

pub(super) struct LockFile;

fn unsupported() -> io::Error {
    io::Error::new(
        ErrorKind::Unsupported,
        "Jetpack advisory locks are unsupported on this target",
    )
}

pub(super) fn open(_path: &Path) -> io::Result<LockFile> {
    Err(unsupported())
}

pub(super) fn open_existing(_path: &Path) -> io::Result<LockFile> {
    Err(unsupported())
}

pub(super) fn validate_path(_file: &LockFile, _path: &Path) -> io::Result<()> {
    Err(unsupported())
}

pub(super) fn try_lock(_file: &LockFile) -> io::Result<bool> {
    Err(unsupported())
}

pub(super) fn unlock(_file: &LockFile) -> io::Result<()> {
    Err(unsupported())
}
