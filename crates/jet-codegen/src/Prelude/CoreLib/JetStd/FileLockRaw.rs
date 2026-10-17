// D-CORE-FILELOCK1=A. Seed syscall transports; replaced by Jet runtime L7,
// dies at cutover. Core/files/files.jet owns validation, errors, identity
// retry and descriptor ownership. Included once by AOT and the interpreter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct JetFsLockCall {
    pub(crate) value: i64,
    pub(crate) errno: i64,
    pub(crate) platform: i64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct JetFsLockIdentity {
    pub(crate) device: i64,
    pub(crate) low: i64,
    pub(crate) high: i64,
    pub(crate) regular: bool,
    pub(crate) reparse: bool,
    pub(crate) errno: i64,
    pub(crate) platform: i64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct JetFsLockWait {
    pub(crate) acquired: bool,
    pub(crate) timed_out: bool,
    pub(crate) errno: i64,
    pub(crate) platform: i64,
    pub(crate) deadline_ns: i64,
}

// JET_VETTED_UNSAFE_BEGIN: jet_file_lock_native
// AUDIT: the private source FileLock owns one live OS descriptor. Calls borrow
// it, and close consumes it exactly once. Temporary File views use ManuallyDrop
// and never close the borrowed descriptor. Windows metadata buffers have the
// documented FILE_ID_INFO layout and remain live through the synchronous call.
// User code cannot construct a FileLock or call these private Core leaves.
mod jet_file_lock_native {
    use super::{JetFsLockCall, JetFsLockIdentity, JetFsLockWait};
    #[cfg(target_os = "linux")]
    const PLATFORM: i64 = 1;
    #[cfg(target_os = "macos")]
    const PLATFORM: i64 = 2;
    #[cfg(windows)]
    const PLATFORM: i64 = 3;
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    const PLATFORM: i64 = 0;

    fn call(value: i64, errno: i64) -> JetFsLockCall {
        JetFsLockCall { value, errno, platform: PLATFORM }
    }
    fn error() -> JetFsLockCall {
        call(-1, i64::from(std::io::Error::last_os_error().raw_os_error().unwrap_or(0)))
    }
    fn failed_identity(errno: i64) -> JetFsLockIdentity {
        JetFsLockIdentity { device: 0, low: 0, high: 0, regular: false,
            reparse: false, errno, platform: PLATFORM }
    }

    pub(super) fn open(path: &str) -> JetFsLockCall {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::os::fd::IntoRawFd;
            use std::os::unix::fs::OpenOptionsExt;
            // Architecture-specific O_NOFOLLOW/O_CLOEXEC are the existing
            // vetted FileHandles constants. std's open also sets CLOEXEC.
            match std::fs::OpenOptions::new().read(true).write(true).create(true)
                .mode(0o600).custom_flags(super::jet_fs_sys::O_NOFOLLOW | super::jet_fs_sys::O_CLOEXEC)
                .open(path)
            {
                Ok(file) => call(i64::from(file.into_raw_fd()), 0),
                Err(e) => call(-1, i64::from(e.raw_os_error().unwrap_or(0))),
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use std::os::windows::io::IntoRawHandle;
            // FILE_SHARE_READ|WRITE, but not DELETE. OPEN_REPARSE_POINT lets
            // source inspect/refuse the final reparse point without following.
            match std::fs::OpenOptions::new().read(true).write(true).create(true)
                .share_mode(3).custom_flags(0x0020_0000).open(path)
            {
                Ok(file) => call(file.into_raw_handle() as isize as i64, 0),
                Err(e) => call(-1, i64::from(e.raw_os_error().unwrap_or(0))),
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        { let _ = path; call(-1, 0) }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    unsafe extern "C" {
        fn flock(fd: std::ffi::c_int, flags: std::ffi::c_int) -> std::ffi::c_int;
        fn close(fd: std::ffi::c_int) -> std::ffi::c_int;
    }
    #[cfg(windows)]
    #[repr(C)]
    struct Overlapped {
        internal: usize, internal_high: usize,
        offset: u32, offset_high: u32, event: *mut std::ffi::c_void,
    }
    #[cfg(windows)]
    #[repr(C)]
    struct FileIdInfo { volume: u64, id: [u8; 16] }
    #[cfg(windows)]
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LockFileEx(file: *mut std::ffi::c_void, flags: u32, reserved: u32,
            low: u32, high: u32, state: *mut Overlapped) -> i32;
        fn CloseHandle(file: *mut std::ffi::c_void) -> i32;
        fn GetFileInformationByHandleEx(file: *mut std::ffi::c_void, class: i32,
            info: *mut std::ffi::c_void, size: u32) -> i32;
    }

    pub(super) fn acquire(fd: i64, nonblocking: bool) -> JetFsLockCall {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            // LOCK_EX=2, LOCK_NB=4 on Linux and macOS.
            let result = unsafe { flock(fd as i32, 2 | if nonblocking { 4 } else { 0 }) };
            if result == 0 { call(0, 0) } else { error() }
        }
        #[cfg(windows)]
        {
            let mut state = Overlapped { internal: 0, internal_high: 0,
                offset: 0, offset_high: 0, event: std::ptr::null_mut() };
            let flags = 2 | if nonblocking { 1 } else { 0 };
            let result = unsafe { LockFileEx(fd as isize as *mut _, flags, 0, 1, 0, &mut state) };
            if result != 0 { call(0, 0) } else { error() }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        { let _ = (fd, nonblocking); call(-1, 0) }
    }

    pub(super) fn take(fd: i64) -> std::fs::File {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::os::fd::FromRawFd;
            unsafe { std::fs::File::from_raw_fd(fd as i32) }
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::FromRawHandle;
            unsafe { std::fs::File::from_raw_handle(fd as isize as *mut _) }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        { let _ = fd; unreachable!("an unsupported lock open cannot produce an owner") }
    }

    pub(super) fn release(fd: i64) -> i64 {
        if fd < 0 { return 0; }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        { if unsafe { close(fd as i32) } == 0 { 0 } else { error().errno } }
        #[cfg(windows)]
        { if unsafe { CloseHandle(fd as isize as *mut _) } != 0 { 0 } else { error().errno } }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        { let _ = fd; 0 }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn identity(meta: std::io::Result<std::fs::Metadata>) -> JetFsLockIdentity {
        use std::os::unix::fs::MetadataExt;
        match meta {
            Ok(meta) => JetFsLockIdentity { device: meta.dev() as i64,
                low: (meta.ino() & 0xffff_ffff) as i64, high: (meta.ino() >> 32) as i64,
                regular: meta.is_file(), reparse: meta.file_type().is_symlink(),
                errno: 0, platform: PLATFORM },
            Err(e) => failed_identity(i64::from(e.raw_os_error().unwrap_or(0))),
        }
    }
    #[cfg(windows)]
    fn identity(file: &std::fs::File) -> JetFsLockIdentity {
        use std::os::windows::{fs::MetadataExt, io::AsRawHandle};
        let meta = match file.metadata() {
            Ok(meta) => meta,
            Err(e) => return failed_identity(i64::from(e.raw_os_error().unwrap_or(0))),
        };
        let mut info = std::mem::MaybeUninit::<FileIdInfo>::uninit();
        // FileIdInfo = FILE_INFO_BY_HANDLE_CLASS 18; preserves all 128 id bits.
        if unsafe { GetFileInformationByHandleEx(file.as_raw_handle(), 18,
            info.as_mut_ptr().cast(), std::mem::size_of::<FileIdInfo>() as u32) } == 0
        { return failed_identity(error().errno); }
        let info = unsafe { info.assume_init() };
        JetFsLockIdentity { device: info.volume as i64,
            low: u64::from_le_bytes(info.id[..8].try_into().unwrap()) as i64,
            high: u64::from_le_bytes(info.id[8..].try_into().unwrap()) as i64,
            regular: meta.is_file(), reparse: meta.file_attributes() & 0x400 != 0,
            errno: 0, platform: PLATFORM }
    }

    pub(super) fn fd_identity(fd: i64) -> JetFsLockIdentity {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::os::fd::FromRawFd;
            let view = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd as i32) });
            identity(view.metadata())
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::FromRawHandle;
            let view = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_handle(fd as isize as *mut _) });
            identity(&view)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        { let _ = fd; failed_identity(0) }
    }
    pub(super) fn path_identity(path: &str) -> JetFsLockIdentity {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        { identity(std::fs::symlink_metadata(path)) }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // Metadata-only, no-follow view; this independent query does not
            // replace or transfer the lock's original descriptor.
            match std::fs::OpenOptions::new().access_mode(0).share_mode(7)
                .custom_flags(0x0020_0000 | 0x0200_0000).open(path)
            {
                Ok(file) => identity(&file),
                Err(e) => failed_identity(i64::from(e.raw_os_error().unwrap_or(0))),
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        { let _ = path; failed_identity(0) }
    }

    // A timed OS-acquisition primitive, not a second Core clock. Uses the
    // EXISTING host monotonic sampler. Rust oracle: RuntimePolicy.rs:170-184
    // (1ms polling, capped to the remaining deadline). Zero initializes the
    // deadline once; source reopens after identity changes and carries it back.
    pub(super) fn wait(fd: i64, timeout_ns: i64, bounded: bool, deadline_ns: i64) -> JetFsLockWait {
        let sample = || jet_foundation::Monotonic::jet_time_monotonic_host_sample().min(i64::MAX as u128) as i64;
        let deadline = if !bounded { 0 } else if deadline_ns != 0 { deadline_ns }
            else { sample().saturating_add(timeout_ns.max(0)).max(1) };
        let mut first = deadline_ns == 0;
        loop {
            if bounded && !first && sample() >= deadline {
                return JetFsLockWait { acquired: false, timed_out: true,
                    errno: 0, platform: PLATFORM, deadline_ns: deadline };
            }
            first = false;
            let result = acquire(fd, bounded);
            if result.value == 0 {
                return JetFsLockWait { acquired: true, timed_out: false,
                    errno: 0, platform: PLATFORM, deadline_ns: deadline };
            }
            // This is the kernel timed-wait implementation's WouldBlock rail,
            // not public IOError classification. EINTR retains this deadline.
            let interrupted = PLATFORM != 3 && result.errno == 4;
            let busy = (PLATFORM == 1 && result.errno == 11)
                || (PLATFORM == 2 && result.errno == 35)
                || (PLATFORM == 3 && result.errno == 33);
            if interrupted { continue; }
            if !bounded || !busy {
                return JetFsLockWait { acquired: false, timed_out: false,
                    errno: result.errno, platform: PLATFORM, deadline_ns: deadline };
            }
            let remaining = deadline.saturating_sub(sample());
            if remaining <= 0 {
                return JetFsLockWait { acquired: false, timed_out: true,
                    errno: 0, platform: PLATFORM, deadline_ns: deadline };
            }
            std::thread::sleep(std::time::Duration::from_nanos(remaining.min(1_000_000) as u64));
        }
    }
}
// JET_VETTED_UNSAFE_END: jet_file_lock_native

pub(crate) fn jet_fs_lock_open_raw(path: &str) -> JetFsLockCall { jet_file_lock_native::open(path) }
pub(crate) fn jet_fs_lock_try_raw(fd: i64) -> JetFsLockCall { jet_file_lock_native::acquire(fd, true) }
pub(crate) fn jet_fs_lock_close_raw(fd: i64) -> i64 { jet_file_lock_native::release(fd) }
pub(crate) fn jet_fs_lock_fd_identity_raw(fd: i64) -> JetFsLockIdentity { jet_file_lock_native::fd_identity(fd) }
pub(crate) fn jet_fs_lock_path_identity_raw(path: &str) -> JetFsLockIdentity { jet_file_lock_native::path_identity(path) }
pub(crate) fn jet_fs_lock_wait_raw(fd: i64, timeout_ns: i64, bounded: bool, deadline_ns: i64) -> JetFsLockWait {
    jet_file_lock_native::wait(fd, timeout_ns, bounded, deadline_ns)
}
// Infallible transfer after the source guard is disarmed. The final Rc<File>
// drop closes the descriptor, releasing the lock without deleting its name.
pub(crate) fn jet_fs_lock_owner_raw(path: String, fd: i64) -> super::JetFileLockOwner {
    super::JetFileLockOwner { path, file: std::rc::Rc::new(jet_file_lock_native::take(fd)) }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos", windows)))]
mod file_lock_leaf_tests {
    use super::*;
    fn fixture(name: &str) -> (std::path::PathBuf, String) {
        let root = std::env::temp_dir().join(format!("jet-lock-leaf-{name}-{}-{}",
            std::process::id(), std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("lock").to_str().unwrap().to_string();
        (root, path)
    }
    fn opened(path: &String) -> i64 {
        let row = jet_fs_lock_open_raw(path);
        assert_eq!(row.errno, 0);
        assert!(row.value >= 0);
        row.value
    }

    #[test]
    fn transfer_and_release_preserve_the_lock_file() {
        let (root, path) = fixture("transfer");
        let fd = opened(&path);
        assert_eq!(jet_fs_lock_try_raw(fd).value, 0);
        let identity = jet_fs_lock_fd_identity_raw(fd);
        assert_eq!(identity, jet_fs_lock_path_identity_raw(&path));
        let owner = jet_fs_lock_owner_raw(path.clone(), fd);
        // This is the source guard's disarmed close. It must not close fd.
        assert_eq!(jet_fs_lock_close_raw(-1), 0);
        let second = opened(&path);
        assert_ne!(jet_fs_lock_try_raw(second).value, 0);
        drop(owner);
        assert!(std::path::Path::new(&path).exists());
        assert_eq!(jet_fs_lock_try_raw(second).value, 0);
        assert_eq!(jet_fs_lock_fd_identity_raw(second), identity);
        assert_eq!(jet_fs_lock_close_raw(second), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn timed_wait_and_reopened_retry_share_one_deadline() {
        let (root, path) = fixture("deadline");
        let held = opened(&path);
        assert_eq!(jet_fs_lock_try_raw(held).value, 0);
        let contender = opened(&path);
        let failed = jet_fs_lock_wait_raw(contender, 2_000_000, true, 0);
        assert!(!failed.acquired && failed.timed_out && failed.errno == 0);
        // Pre-transfer failure closes its own descriptor, not the live owner.
        assert_eq!(jet_fs_lock_close_raw(contender), 0);
        let reopened = opened(&path);
        let repeated = jet_fs_lock_wait_raw(reopened, 1_000_000_000, true, failed.deadline_ns);
        assert!(repeated.timed_out && !repeated.acquired);
        assert_eq!(repeated.deadline_ns, failed.deadline_ns);
        assert_eq!(jet_fs_lock_close_raw(reopened), 0);
        assert_eq!(jet_fs_lock_close_raw(held), 0);
        let fresh = opened(&path);
        assert_eq!(jet_fs_lock_try_raw(fresh).value, 0);
        assert_eq!(jet_fs_lock_close_raw(fresh), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_final_symlink_never_opens_its_target() {
        let (root, path) = fixture("symlink");
        let target = root.join("target");
        std::fs::write(&target, b"unchanged").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let refused = jet_fs_lock_open_raw(&path);
        assert!(refused.value < 0);
        assert_eq!(refused.errno, if cfg!(target_os = "linux") { 40 } else { 62 });
        assert_eq!(std::fs::read(&target).unwrap(), b"unchanged");
        std::fs::remove_dir_all(root).unwrap();
    }
}
