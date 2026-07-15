use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind};
use std::os::windows::fs::OpenOptionsExt as _;
use std::os::windows::io::AsRawHandle as _;
use std::path::Path;

type Handle = *mut c_void;

const FILE_SHARE_READ: u32 = 0x0000_0001;
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
const LOCKFILE_FAIL_IMMEDIATELY: u32 = 0x0000_0001;
const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x0000_0002;
const ERROR_LOCK_VIOLATION: i32 = 33;

#[repr(C)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: [u32; 2],
    event: Handle,
}

unsafe extern "system" {
    fn SetHandleInformation(handle: Handle, mask: u32, flags: u32) -> i32;
    fn LockFileEx(
        handle: Handle,
        flags: u32,
        reserved: u32,
        bytes_low: u32,
        bytes_high: u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn UnlockFileEx(
        handle: Handle,
        reserved: u32,
        bytes_low: u32,
        bytes_high: u32,
        overlapped: *mut Overlapped,
    ) -> i32;
}

fn overlapped() -> Overlapped {
    Overlapped {
        internal: 0,
        internal_high: 0,
        offset: [0, 0],
        event: std::ptr::null_mut(),
    }
}

pub(super) fn open(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        // Denying FILE_SHARE_DELETE pins the one persistent path/inode while
        // any contender has it open. No process may split waiters by unlinking.
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(path)?;
    // SAFETY: raw handle belongs to live `file`; clearing inheritance changes
    // handle metadata only and prevents lock leakage into child processes.
    if unsafe {
        SetHandleInformation(
            file.as_raw_handle().cast(),
            HANDLE_FLAG_INHERIT,
            0,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

pub(super) fn try_lock(file: &File) -> io::Result<bool> {
    let mut state = overlapped();
    // SAFETY: raw handle is live; `state` is initialized, writable, and lives
    // through this synchronous fail-immediately call. Lock range is byte 0.
    if unsafe {
        LockFileEx(
            file.as_raw_handle().cast(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            &mut state,
        )
    } != 0
    {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_LOCK_VIOLATION) {
        Ok(false)
    } else {
        Err(error)
    }
}

pub(super) fn unlock(file: &File) -> io::Result<()> {
    let mut state = overlapped();
    // SAFETY: raw handle is live; initialized OVERLAPPED identifies same byte
    // range used by LockFileEx and lives through synchronous unlock.
    if unsafe { UnlockFileEx(file.as_raw_handle().cast(), 0, 1, 0, &mut state) } != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockfileex_runtime_serializes_when_run_on_windows() {
        let root = std::env::temp_dir().join(format!(
            "jetpack-lockfileex-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("lock");
        let first = open(&path).unwrap();
        let second = open(&path).unwrap();
        assert!(try_lock(&first).unwrap());
        assert!(!try_lock(&second).unwrap());
        unlock(&first).unwrap();
        assert!(try_lock(&second).unwrap());
        unlock(&second).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
