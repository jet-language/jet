// JET_VETTED_UNSAFE_BEGIN: jetc_fd_seed
// #4821 syscall-only seed. Source owns fds, format, faults and publication.
// Dies when FD72 HostFiles supplies O_TMPFILE / linkat(AT_EMPTY_PATH).
// AUDIT: only reserved Core callers can lend these descriptors. A live
// move-only JetcFd keeps the fd open throughout each call and relinquishes it
// exactly once through close. Borrowed File wrappers are ManuallyDrop, so
// read/write/stat/seek/fsync never close or retain the fd. Unix descriptors
// are checked nonnegative i32 values; every FFI pathname is a live CString
// or C string for the duration of the syscall. Read counts and write spans
// are checked before access, and no borrowed pointer escapes a syscall.
#[derive(Debug)]
pub(crate) struct JetCryptoJetcRaw {
    pub(crate) value: i64, pub(crate) errno: i64, pub(crate) bytes: Vec<u8>,
    // Compile-time platform fact; source consumes it only from directory open.
    pub(crate) linux: bool,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct JetCryptoJetcStat {
    pub(crate) status: i64, pub(crate) errno: i64, pub(crate) dev: i64, pub(crate) ino: i64, pub(crate) len: i64,
    pub(crate) mtime: i64, pub(crate) mtime_ns: i64, pub(crate) nlink: i64, pub(crate) is_file: bool, pub(crate) is_dir: bool,
}
// Linux UAPI asm/fcntl.h overrides asm-generic/fcntl.h on ARM/PowerPC
// and SPARC. Keep these target facts here, not in source crypto policy.
#[cfg(all(target_os = "linux", any(target_arch = "arm", target_arch = "aarch64", target_arch = "powerpc", target_arch = "powerpc64")))]
const JETC_O_DIRECTORY: i32 = 1 << 14;
#[cfg(all(target_os = "linux", not(any(target_arch = "arm", target_arch = "aarch64", target_arch = "powerpc", target_arch = "powerpc64"))))]
const JETC_O_DIRECTORY: i32 = 1 << 16;
#[cfg(all(target_os = "linux", any(target_arch = "arm", target_arch = "aarch64", target_arch = "powerpc", target_arch = "powerpc64")))]
const JETC_O_NOFOLLOW: i32 = 1 << 15;
#[cfg(all(target_os = "linux", not(any(target_arch = "arm", target_arch = "aarch64", target_arch = "powerpc", target_arch = "powerpc64"))))]
const JETC_O_NOFOLLOW: i32 = 1 << 17;
#[cfg(all(target_os = "linux", any(target_arch = "sparc", target_arch = "sparc64")))]
const JETC_O_CLOEXEC: i32 = 1 << 22;
#[cfg(all(target_os = "linux", not(any(target_arch = "sparc", target_arch = "sparc64"))))]
const JETC_O_CLOEXEC: i32 = 1 << 19;
#[cfg(all(target_os = "linux", any(target_arch = "sparc", target_arch = "sparc64")))]
const JETC_O_TMPFILE: i32 = (1 << 25) | JETC_O_DIRECTORY;
#[cfg(all(target_os = "linux", not(any(target_arch = "sparc", target_arch = "sparc64"))))]
const JETC_O_TMPFILE: i32 = (1 << 22) | JETC_O_DIRECTORY;

fn jetc_fd_result(result: std::io::Result<i64>) -> JetCryptoJetcRaw {
    match result {
        Ok(value) => JetCryptoJetcRaw { value, errno: 0, bytes: Vec::new(), linux: cfg!(target_os = "linux") },
        Err(error) => JetCryptoJetcRaw { value: -1, errno: i64::from(error.raw_os_error().unwrap_or(5)), bytes: Vec::new(), linux: cfg!(target_os = "linux") },
    }
}
fn jetc_fd_path(path: &str) -> std::io::Result<std::ffi::CString> {
    std::ffi::CString::new(path).map_err(|_| std::io::Error::from_raw_os_error(22))
}
fn jetc_fd_guard(fd: i64) -> std::io::Result<i32> {
    i32::try_from(fd).ok().filter(|fd| *fd >= 0)
        .ok_or_else(|| std::io::Error::from_raw_os_error(9))
}
fn jetc_fd_into(file: std::fs::File) -> i64 {
    #[cfg(unix)] { use std::os::fd::IntoRawFd; i64::from(file.into_raw_fd()) }
    #[cfg(windows)] { use std::os::windows::io::IntoRawHandle; file.into_raw_handle() as i64 }
}
fn jetc_fd_borrow(fd: i64) -> std::io::Result<std::mem::ManuallyDrop<std::fs::File>> {
    if fd < 0 { return Err(std::io::Error::from_raw_os_error(9)); }
    #[cfg(unix)] {
        use std::os::fd::FromRawFd;
        let fd = jetc_fd_guard(fd)?;
        Ok(std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd) }))
    }
    #[cfg(windows)] {
        use std::os::windows::io::FromRawHandle;
        Ok(std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_handle(fd as _) }))
    }
}
fn jetc_fd_open(path: &str, directory: bool) -> std::io::Result<i64> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")] {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(JETC_O_CLOEXEC | JETC_O_NOFOLLOW | if directory { JETC_O_DIRECTORY } else { 0 });
    }
    #[cfg(not(target_os = "linux"))]
    if !directory && std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(std::io::Error::from_raw_os_error(22));
    }
    #[cfg(windows)] if directory {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x02000000);
    }
    Ok(jetc_fd_into(options.open(path)?))
}
pub(crate) fn jet_crypto_jetc_open_directory(path: &String) -> JetCryptoJetcRaw {
    jetc_fd_result(jetc_fd_open(path, true))
}
pub(crate) fn jet_crypto_jetc_open_source(path: &String) -> JetCryptoJetcRaw {
    jetc_fd_result(jetc_fd_open(path, false))
}
pub(crate) fn jet_crypto_jetc_create_named(path: &String) -> JetCryptoJetcRaw {
    jetc_fd_result((|| {
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
        Ok(jetc_fd_into(options.open(path)?))
    })())
}
pub(crate) fn jet_crypto_jetc_open_child_directory(parent: i64, name: &String) -> JetCryptoJetcRaw {
    #[cfg(target_os = "linux")]
    return jetc_fd_result((|| {
        let parent = jetc_fd_guard(parent)?;
        let name = jetc_fd_path(name)?;
        unsafe extern "C" { fn openat(fd: i32, name: *const std::ffi::c_char, flags: i32, mode: u32) -> i32; }
        // Source supplies one plain component and keeps the parent owner live.
        let fd = unsafe { openat(parent, name.as_ptr(), JETC_O_CLOEXEC | JETC_O_NOFOLLOW | JETC_O_DIRECTORY, 0) };
        if fd < 0 { Err(std::io::Error::last_os_error()) } else { Ok(i64::from(fd)) }
    })());
    #[cfg(not(target_os = "linux"))]
    { let _ = (parent, name); jetc_fd_result(Err(std::io::Error::from_raw_os_error(95))) }
}
pub(crate) fn jet_crypto_jetc_tmpfile(parent: i64) -> JetCryptoJetcRaw {
    #[cfg(target_os = "linux")]
    return jetc_fd_result((|| {
        let parent = jetc_fd_guard(parent)?;
        unsafe extern "C" { fn openat(fd: i32, name: *const std::ffi::c_char, flags: i32, mode: u32) -> i32; }
        let fd = unsafe { openat(parent, c".".as_ptr(), 2 | JETC_O_TMPFILE | JETC_O_CLOEXEC, 0o600) };
        if fd < 0 { Err(std::io::Error::last_os_error()) } else { Ok(i64::from(fd)) }
    })());
    #[cfg(not(target_os = "linux"))]
    { let _ = parent; jetc_fd_result(Err(std::io::Error::from_raw_os_error(95))) }
}
pub(crate) fn jet_crypto_jetc_read(fd: i64, count: i64) -> JetCryptoJetcRaw {
    use std::io::Read;
    let result = (|| {
        let mut file = jetc_fd_borrow(fd)?;
        let count = usize::try_from(count).map_err(|_| std::io::Error::from_raw_os_error(22))?;
        let mut bytes = vec![0; count];
        let read = file.read(&mut bytes)?;
        bytes.truncate(read);
        Ok::<_, std::io::Error>(JetCryptoJetcRaw { value: read as i64, errno: 0, bytes, linux: cfg!(target_os = "linux") })
    })();
    result.unwrap_or_else(|error| jetc_fd_result(Err(error)))
}
pub(crate) fn jet_crypto_jetc_write(fd: i64, bytes: &Vec<u8>, offset: i64, count: i64) -> JetCryptoJetcRaw {
    use std::io::Write;
    jetc_fd_result((|| {
        let mut file = jetc_fd_borrow(fd)?;
        let offset = usize::try_from(offset).map_err(|_| std::io::Error::from_raw_os_error(22))?;
        let count = usize::try_from(count).map_err(|_| std::io::Error::from_raw_os_error(22))?;
        let source = bytes.get(offset..).and_then(|bytes| bytes.get(..count))
            .ok_or_else(|| std::io::Error::from_raw_os_error(22))?;
        file.write(source).map(|written| written as i64)
    })())
}
pub(crate) fn jet_crypto_jetc_seek(fd: i64, offset: i64) -> JetCryptoJetcRaw {
    use std::io::{Seek, SeekFrom};
    jetc_fd_result((|| {
        let mut file = jetc_fd_borrow(fd)?;
        let offset = u64::try_from(offset).map_err(|_| std::io::Error::from_raw_os_error(22))?;
        file.seek(SeekFrom::Start(offset)).map(|position| position as i64)
    })())
}
pub(crate) fn jet_crypto_jetc_stat(fd: i64) -> JetCryptoJetcStat {
    let result = (|| {
        let file = jetc_fd_borrow(fd)?;
        let meta = file.metadata()?;
        #[cfg(unix)] {
            use std::os::unix::fs::MetadataExt;
            Ok::<_, std::io::Error>(JetCryptoJetcStat { status: 0, errno: 0,
                dev: meta.dev() as i64, ino: meta.ino() as i64, len: meta.len() as i64,
                mtime: meta.mtime(), mtime_ns: meta.mtime_nsec(), nlink: meta.nlink() as i64,
                is_file: meta.is_file(), is_dir: meta.is_dir() })
        }
        #[cfg(not(unix))] {
            let modified = meta.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok());
            Ok::<_, std::io::Error>(JetCryptoJetcStat { status: 0, errno: 0,
                dev: i64::from(meta.permissions().readonly()), ino: 0, len: meta.len() as i64,
                mtime: modified.map_or(0, |time| time.as_secs() as i64),
                mtime_ns: modified.map_or(0, |time| i64::from(time.subsec_nanos())), nlink: 0,
                is_file: meta.is_file(), is_dir: meta.is_dir() })
        }
    })();
    result.unwrap_or_else(|error| JetCryptoJetcStat { status: -1,
        errno: i64::from(error.raw_os_error().unwrap_or(5)), dev: 0, ino: 0, len: 0,
        mtime: 0, mtime_ns: 0, nlink: 0, is_file: false, is_dir: false })
}
pub(crate) fn jet_crypto_jetc_fsync(fd: i64) -> JetCryptoJetcRaw {
    jetc_fd_result((|| { jetc_fd_borrow(fd)?.sync_all()?; Ok(0) })())
}
pub(crate) fn jet_crypto_jetc_link(fd: i64, parent: i64, destination: &String) -> JetCryptoJetcRaw {
    #[cfg(target_os = "linux")]
    return jetc_fd_result((|| {
        let fd = jetc_fd_guard(fd)?;
        let parent = jetc_fd_guard(parent)?;
        let destination = jetc_fd_path(destination)?;
        unsafe extern "C" {
            fn linkat(oldfd: i32, old: *const std::ffi::c_char, newfd: i32, new: *const std::ffi::c_char, flags: i32) -> i32;
        }
        let result = unsafe { linkat(fd, c"".as_ptr(), parent, destination.as_ptr(), 0x1000) };
        if result != 0 { Err(std::io::Error::last_os_error()) } else { Ok(0) }
    })());
    #[cfg(not(target_os = "linux"))]
    { let _ = (fd, parent, destination); jetc_fd_result(Err(std::io::Error::from_raw_os_error(95))) }
}
pub(crate) fn jet_crypto_jetc_close(fd: i64) -> JetCryptoJetcRaw {
    jetc_fd_result((|| {
        let file = jetc_fd_borrow(fd)?;
        // This is the only transfer of borrowed fd ownership back to a File.
        drop(std::mem::ManuallyDrop::into_inner(file));
        Ok(0)
    })())
}
// JET_VETTED_UNSAFE_END: jetc_fd_seed
