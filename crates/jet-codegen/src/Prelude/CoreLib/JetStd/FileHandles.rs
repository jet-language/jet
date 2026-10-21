// #3593 store-grade file leaves (D-CORE-FILEDIR1, D-CORE-FILEOPEN1,
// D-CORE-FILESTAT1, D-CORE-FILERENAME1, D-CORE-FILEXATTR1, D-CORE-HANDLEFD1).
//
// Each function here performs one operating-system call and maps its errno
// through the shared `io_error_at` mapper. Core/Files/Files.jet owns the
// public types, defaults, name validation, part-by-part walking, and error
// wording; AOT and the interpreter include this same file inside `jet_std`.
//
// A held directory lives in one process table keyed by an Int that only Core's
// opaque `Directory` value carries. Keys are never reused, so a stale key
// reports `IOError::Closed` instead of naming another directory.

struct JetFsHeld {
    file: std::fs::File,
    path: String,
}

fn jet_fs_held() -> &'static std::sync::Mutex<std::collections::HashMap<i64, JetFsHeld>> {
    static HELD: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<i64, JetFsHeld>>> =
        std::sync::OnceLock::new();
    HELD.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

static JET_FS_NEXT_HELD: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

fn jet_fs_context(operation: IOOperation, shown: &str, os_code: Option<i64>, cause: &str) -> IOContext {
    IOContext::new(operation, Some(shown.to_string()), os_code, Some(cause.to_string()))
}

/// The name a held-directory error reports: the held path joined to `name`.
fn jet_fs_shown(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else if name.is_empty() {
        base.to_string()
    } else if base.ends_with('/') {
        format!("{base}{name}")
    } else {
        format!("{base}/{name}")
    }
}

fn jet_fs_held_path(handle: i64, operation: IOOperation) -> Result<String, IOError> {
    if handle < 0 {
        return Ok(String::new());
    }
    let held = jet_fs_held().lock().unwrap_or_else(|poison| poison.into_inner());
    held.get(&handle).map(|entry| entry.path.clone()).ok_or_else(|| {
        IOError::Closed(IOContext::new(
            operation,
            None,
            None,
            Some("the directory handle is already closed".to_string()),
        ))
    })
}

#[allow(dead_code)]
fn jet_fs_unsupported(operation: IOOperation, shown: &str, what: &str) -> IOError {
    IOError::Unsupported(jet_fs_context(
        operation,
        shown,
        None,
        &format!("{what} is not available on this operating system"),
    ))
}

/// Store an opened directory in the held table and return its key.
fn jet_fs_hold(file: std::fs::File, path: String) -> i64 {
    let key = JET_FS_NEXT_HELD.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut held = jet_fs_held().lock().unwrap_or_else(|poison| poison.into_inner());
    held.insert(key, JetFsHeld { file, path });
    key
}

/// Close a held directory. Closing an unknown key is a no-op: the key was
/// already released by an earlier close.
pub(crate) fn jet_fs_directory_close(handle: i64) {
    let mut held = jet_fs_held().lock().unwrap_or_else(|poison| poison.into_inner());
    held.remove(&handle);
}

/// Flush a held directory so earlier renames inside it are durable.
pub(crate) fn jet_fs_directory_fsync(handle: i64) -> Result<(), IOError> {
    let held = jet_fs_held().lock().unwrap_or_else(|poison| poison.into_inner());
    let entry = held.get(&handle).ok_or_else(|| {
        IOError::Closed(IOContext::new(
            IOOperation::Flush,
            None,
            None,
            Some("the directory handle is already closed".to_string()),
        ))
    })?;
    entry
        .file
        .sync_all()
        .map_err(|error| io_error_at(IOOperation::Flush, &entry.path, error))
}

// JET_VETTED_UNSAFE_BEGIN: jet_fs_handles
// AUDIT: #3593. Raw *at(2) and extended-attribute calls on descriptors this
// table owns. Every pointer passed is a live CString or Vec owned by the
// calling frame; every returned descriptor is wrapped in an owning File at
// once, so no descriptor leaks or is closed twice.
#[cfg(unix)]
mod jet_fs_sys {
    use std::ffi::{c_char, c_int, c_void};

    #[cfg(target_os = "linux")]
    pub const AT_FDCWD: c_int = -100;
    #[cfg(not(target_os = "linux"))]
    pub const AT_FDCWD: c_int = -2;
    #[cfg(target_os = "linux")]
    pub const AT_REMOVEDIR: c_int = 0x200;
    #[cfg(not(target_os = "linux"))]
    pub const AT_REMOVEDIR: c_int = 0x80;

    pub const O_RDONLY: c_int = 0;
    pub const O_WRONLY: c_int = 1;
    #[cfg(target_os = "linux")]
    pub const O_CREAT: c_int = 0o100;
    #[cfg(target_os = "linux")]
    pub const O_EXCL: c_int = 0o200;
    #[cfg(target_os = "linux")]
    pub const O_TRUNC: c_int = 0o1000;
    #[cfg(target_os = "linux")]
    pub const O_APPEND: c_int = 0o2000;
    #[cfg(target_os = "linux")]
    pub const O_NONBLOCK: c_int = 0o4000;
    #[cfg(target_os = "linux")]
    pub const O_CLOEXEC: c_int = 0o2000000;
    #[cfg(target_os = "linux")]
    pub const O_PATH: c_int = 0o10000000;
    #[cfg(all(target_os = "linux", any(target_arch = "aarch64", target_arch = "arm")))]
    pub const O_DIRECTORY: c_int = 0o40000;
    #[cfg(all(target_os = "linux", any(target_arch = "aarch64", target_arch = "arm")))]
    pub const O_NOFOLLOW: c_int = 0o100000;
    #[cfg(all(target_os = "linux", not(any(target_arch = "aarch64", target_arch = "arm"))))]
    pub const O_DIRECTORY: c_int = 0o200000;
    #[cfg(all(target_os = "linux", not(any(target_arch = "aarch64", target_arch = "arm"))))]
    pub const O_NOFOLLOW: c_int = 0o400000;
    #[cfg(target_os = "linux")]
    pub const ELOOP: i32 = 40;
    #[cfg(target_os = "linux")]
    pub const ENOATTR: i32 = 61;
    #[cfg(target_os = "linux")]
    pub const RENAME_NOREPLACE: u32 = 1;
    #[cfg(target_os = "linux")]
    pub const RENAME_EXCHANGE: u32 = 2;

    #[cfg(not(target_os = "linux"))]
    pub const O_CREAT: c_int = 0x200;
    #[cfg(not(target_os = "linux"))]
    pub const O_EXCL: c_int = 0x800;
    #[cfg(not(target_os = "linux"))]
    pub const O_TRUNC: c_int = 0x400;
    #[cfg(not(target_os = "linux"))]
    pub const O_APPEND: c_int = 0x8;
    #[cfg(not(target_os = "linux"))]
    pub const O_NONBLOCK: c_int = 0x4;
    #[cfg(not(target_os = "linux"))]
    pub const O_CLOEXEC: c_int = 0x1000000;
    #[cfg(not(target_os = "linux"))]
    pub const O_DIRECTORY: c_int = 0x100000;
    #[cfg(not(target_os = "linux"))]
    pub const O_NOFOLLOW: c_int = 0x100;
    #[cfg(not(target_os = "linux"))]
    pub const O_SYMLINK: c_int = 0x200000;
    #[cfg(not(target_os = "linux"))]
    pub const ELOOP: i32 = 62;
    #[cfg(not(target_os = "linux"))]
    pub const ENOATTR: i32 = 93;
    #[cfg(not(target_os = "linux"))]
    pub const RENAME_SWAP: u32 = 0x2;
    #[cfg(not(target_os = "linux"))]
    pub const RENAME_EXCL: u32 = 0x4;
    #[cfg(not(target_os = "linux"))]
    pub const XATTR_NOFOLLOW: c_int = 0x1;

    pub const EEXIST: i32 = 17;
    pub const EINVAL: i32 = 22;
    #[cfg(target_os = "linux")]
    pub const ENOTSUP: i32 = 95;
    #[cfg(not(target_os = "linux"))]
    pub const ENOTSUP: i32 = 45;
    #[cfg(target_os = "linux")]
    pub const ENOSYS: i32 = 38;
    #[cfg(not(target_os = "linux"))]
    pub const ENOSYS: i32 = 78;

    // Byte offsets inside the platform `struct dirent` returned by readdir.
    #[cfg(target_os = "linux")]
    pub const DIRENT_TYPE: usize = 18;
    #[cfg(target_os = "linux")]
    pub const DIRENT_NAME: usize = 19;
    #[cfg(not(target_os = "linux"))]
    pub const DIRENT_TYPE: usize = 20;
    #[cfg(not(target_os = "linux"))]
    pub const DIRENT_NAME: usize = 21;
    pub const DT_UNKNOWN: u8 = 0;
    pub const DT_DIR: u8 = 4;

    // `openat` is variadic in C; a host crate may also declare it with a fixed
    // `mode` argument, which the clash lint would otherwise reject.
    #[allow(clashing_extern_declarations)]
    unsafe extern "C" {
        pub fn openat(dirfd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
        pub fn flock(fd: c_int, operation: c_int) -> c_int;
        pub fn mkdirat(dirfd: c_int, path: *const c_char, mode: u32) -> c_int;
        pub fn renameat(olddirfd: c_int, old: *const c_char, newdirfd: c_int, new: *const c_char) -> c_int;
        pub fn linkat(olddirfd: c_int, old: *const c_char, newdirfd: c_int, new: *const c_char, flags: c_int) -> c_int;
        pub fn unlinkat(dirfd: c_int, path: *const c_char, flags: c_int) -> c_int;
        pub fn readlinkat(dirfd: c_int, path: *const c_char, buf: *mut c_char, size: usize) -> isize;
        #[cfg_attr(all(target_os = "macos", target_arch = "x86_64"), link_name = "fdopendir$INODE64")]
        pub fn fdopendir(fd: c_int) -> *mut c_void;
        #[cfg_attr(all(target_os = "macos", target_arch = "x86_64"), link_name = "readdir$INODE64")]
        pub fn readdir(dir: *mut c_void) -> *const u8;
        pub fn closedir(dir: *mut c_void) -> c_int;
        #[cfg(target_os = "linux")]
        pub fn renameat2(olddirfd: c_int, old: *const c_char, newdirfd: c_int, new: *const c_char, flags: u32) -> c_int;
        #[cfg(not(target_os = "linux"))]
        pub fn renameatx_np(olddirfd: c_int, old: *const c_char, newdirfd: c_int, new: *const c_char, flags: u32) -> c_int;
        #[cfg(target_os = "linux")]
        pub fn llistxattr(path: *const c_char, list: *mut c_char, size: usize) -> isize;
        #[cfg(target_os = "linux")]
        pub fn lgetxattr(path: *const c_char, name: *const c_char, value: *mut c_void, size: usize) -> isize;
        #[cfg(target_os = "linux")]
        pub fn lsetxattr(path: *const c_char, name: *const c_char, value: *const c_void, size: usize, flags: c_int) -> c_int;
        #[cfg(not(target_os = "linux"))]
        pub fn listxattr(path: *const c_char, list: *mut c_char, size: usize, options: c_int) -> isize;
        #[cfg(not(target_os = "linux"))]
        pub fn getxattr(path: *const c_char, name: *const c_char, value: *mut c_void, size: usize, position: u32, options: c_int) -> isize;
        #[cfg(not(target_os = "linux"))]
        pub fn setxattr(path: *const c_char, name: *const c_char, value: *const c_void, size: usize, position: u32, options: c_int) -> c_int;
    }
}

/// Apply flock(2) to the descriptor, not its displayed pathname. Core owns the
/// lock mode and lifetime; closing the independent descriptor releases it.
pub(crate) fn jet_fs_flock(handle: i64, flags: i64) -> Result<(), IOError> {
    #[cfg(unix)]
    {
        let (fd, shown) = jet_fs_dir_fd(handle, IOOperation::Write)?;
        if unsafe { jet_fs_sys::flock(fd, flags as i32) } == 0 {
            Ok(())
        } else {
            Err(jet_fs_last_error(IOOperation::Write, &shown))
        }
    }
    #[cfg(not(unix))]
    {
        let shown = jet_fs_held_path(handle, IOOperation::Write)?;
        let _ = flags;
        Err(jet_fs_unsupported(IOOperation::Write, &shown, "advisory directory locks"))
    }
}

#[cfg(unix)]
fn jet_fs_cstring(text: &str, operation: IOOperation, shown: &str) -> Result<std::ffi::CString, IOError> {
    std::ffi::CString::new(text).map_err(|_| {
        IOError::InvalidInput(jet_fs_context(operation, shown, None, "the name contains a NUL byte"))
    })
}

/// The raw descriptor of a held directory, or the current directory for a
/// negative key. The table keeps the File open; Core's `Directory` owner
/// cannot close it while a method borrows it.
#[cfg(unix)]
fn jet_fs_dir_fd(handle: i64, operation: IOOperation) -> Result<(i32, String), IOError> {
    use std::os::fd::AsRawFd;
    if handle < 0 {
        return Ok((jet_fs_sys::AT_FDCWD, String::new()));
    }
    let held = jet_fs_held().lock().unwrap_or_else(|poison| poison.into_inner());
    held.get(&handle)
        .map(|entry| (entry.file.as_raw_fd(), entry.path.clone()))
        .ok_or_else(|| {
            IOError::Closed(IOContext::new(
                operation,
                None,
                None,
                Some("the directory handle is already closed".to_string()),
            ))
        })
}

#[cfg(unix)]
fn jet_fs_last_error(operation: IOOperation, shown: &str) -> IOError {
    io_error_at(operation, shown, std::io::Error::last_os_error())
}

/// The directory that holds the last part of `name`. For a held key, each
/// earlier part (Core already refused `..`, `.`, empty, and absolute parts) is
/// opened as a directory without following links; the walk's own descriptor
/// stays open in `_walk` while the caller uses `fd`. A negative key treats
/// `name` as an ordinary path.
#[cfg(unix)]
struct JetFsParent {
    fd: i32,
    _walk: Option<std::fs::File>,
    last: String,
    shown: String,
}

#[cfg(unix)]
fn jet_fs_parent(handle: i64, name: &str, operation: IOOperation) -> Result<JetFsParent, IOError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use jet_fs_sys as sys;
    let (fd, base) = jet_fs_dir_fd(handle, operation)?;
    let shown = jet_fs_shown(&base, name);
    if handle < 0 {
        return Ok(JetFsParent { fd, _walk: None, last: name.to_string(), shown });
    }
    let mut parts: Vec<&str> = name.split('/').collect();
    let last = parts.pop().unwrap_or("").to_string();
    let mut current = fd;
    let mut walk: Option<std::fs::File> = None;
    for part in parts {
        let c_part = jet_fs_cstring(part, operation, &shown)?;
        let flags = sys::O_RDONLY | sys::O_DIRECTORY | sys::O_NOFOLLOW | sys::O_CLOEXEC;
        let next = unsafe { sys::openat(current, c_part.as_ptr(), flags, 0 as std::ffi::c_uint) };
        if next < 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(sys::ELOOP) {
                return Err(IOError::InvalidInput(jet_fs_context(
                    operation,
                    &shown,
                    Some(sys::ELOOP as i64),
                    &format!("the part `{part}` is a symbolic link, and a held directory never follows links; name a real directory"),
                )));
            }
            return Err(io_error_at(operation, &shown, error));
        }
        let file = unsafe { std::fs::File::from_raw_fd(next) };
        current = file.as_raw_fd();
        walk = Some(file);
    }
    Ok(JetFsParent { fd: current, _walk: walk, last, shown })
}

/// Open `name` relative to `handle` (negative: the current directory).
/// `access`: 0 read, 1 write and truncate, 2 append, 3 directory.
/// `if_exists`: 0 open or replace, 1 refuse an existing name (exclusive).
/// `mode`: permission bits for a newly created file, or negative for 0o666.
/// `links`: 0 follow a final link, 1 refuse it.
pub(crate) fn jet_fs_open_at(handle: i64, name: &str, access: i64,
if_exists: i64,
mode: i64,
links: i64,) -> Result<std::fs::File, IOError> {
    let operation = if access == 0 || access == 3 { IOOperation::Read } else { IOOperation::Write };
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        use jet_fs_sys as sys;
        let parent = jet_fs_parent(handle, name, operation)?;
        let dir = parent.fd;
        let shown = parent.shown.clone();
        let c_name = jet_fs_cstring(&parent.last, operation, &shown)?;
        let mut flags = sys::O_CLOEXEC;
        flags |= match access {
            0 => sys::O_RDONLY,
            1 => sys::O_WRONLY | sys::O_CREAT | sys::O_TRUNC,
            2 => sys::O_WRONLY | sys::O_CREAT | sys::O_APPEND,
            _ => sys::O_RDONLY | sys::O_DIRECTORY,
        };
        if if_exists == 1 && (access == 1 || access == 2) {
            flags = (flags | sys::O_EXCL) & !sys::O_TRUNC;
        }
        if links == 1 {
            flags |= sys::O_NOFOLLOW;
        }
        let bits: u32 = if mode >= 0 { (mode as u32) & 0o7777 } else { 0o666 };
        let fd = unsafe { sys::openat(dir, c_name.as_ptr(), flags, bits as std::ffi::c_uint) };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            let code = error.raw_os_error().unwrap_or(0);
            if links == 1 && code == sys::ELOOP {
                return Err(IOError::InvalidInput(jet_fs_context(
                    operation,
                    &shown,
                    Some(code as i64),
                    "the path ends in a symbolic link, and this call never follows one; name the link's target instead",
                )));
            }
            if if_exists == 1 && code == sys::EEXIST {
                return Err(IOError::AlreadyExists(jet_fs_context(
                    operation,
                    &shown,
                    Some(code as i64),
                    "this call refuses to replace it; choose a new name or pass if_exists: .Replace",
                )));
            }
            return Err(io_error_at(operation, &shown, error));
        }
        return Ok(unsafe { std::fs::File::from_raw_fd(fd) });
    }
    #[cfg(not(unix))]
    {
        if handle >= 0 {
            let shown = jet_fs_shown(&jet_fs_held_path(handle, operation)?, name);
            return Err(jet_fs_unsupported(operation, &shown, "opening a name inside a held directory"));
        }
        let mut options = std::fs::OpenOptions::new();
        match access {
            0 | 3 => {
                options.read(true);
            }
            1 => {
                options.write(true).create(true).truncate(true);
            }
            _ => {
                options.append(true).create(true);
            }
        }
        if if_exists == 1 && (access == 1 || access == 2) {
            options.create_new(true);
        }
        if links == 1 {
            if let Ok(meta) = std::fs::symlink_metadata(name) {
                if meta.file_type().is_symlink() {
                    return Err(IOError::InvalidInput(jet_fs_context(
                        operation,
                        name,
                        None,
                        "the path ends in a symbolic link, and this call never follows one; name the link's target instead",
                    )));
                }
            }
        }
        let _ = mode;
        return options.open(name).map_err(|error| {
            if if_exists == 1 && error.kind() == std::io::ErrorKind::AlreadyExists {
                IOError::AlreadyExists(jet_fs_context(
                    operation,
                    name,
                    error.raw_os_error().map(i64::from),
                    "this call refuses to replace it; choose a new name or pass if_exists: .Replace",
                ))
            } else {
                io_error_at(operation, name, error)
            }
        });
    }
}

/// Open the path `name` for reading with `O_NONBLOCK` on Unix, so opening a
/// FIFO or device never waits for a peer. A regular file reads as usual. The
/// caller inspects the opened file's metadata before reading it.
pub(crate) fn jet_fs_open_nonblocking(name: &str) -> Result<std::fs::File, IOError> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(jet_fs_sys::O_NONBLOCK);
    }
    options.open(name).map_err(|error| io_error_at(IOOperation::Read, name, error))
}

/// Open and hold the directory `name` relative to `handle`. A final link is
/// refused. Returns the held key.
pub(crate) fn jet_fs_directory_open(handle: i64, name: &str) -> Result<i64, IOError> {
    let base = jet_fs_held_path(handle, IOOperation::Read)?;
    let file = jet_fs_open_at(handle, name, 3, 0, -1, 1)?;
    Ok(jet_fs_hold(file, jet_fs_shown(&base, name)))
}

/// Create the directory `name` relative to `handle`; an existing name is
/// `AlreadyExists`. `mode` < 0 means 0o777 before the process umask.
pub(crate) fn jet_fs_directory_create(handle: i64, name: &str, mode: i64) -> Result<(), IOError> {
    #[cfg(unix)]
    {
        let parent = jet_fs_parent(handle, name, IOOperation::Write)?;
        let dir = parent.fd;
        let shown = parent.shown.clone();
        let c_name = jet_fs_cstring(&parent.last, IOOperation::Write, &shown)?;
        let bits: u32 = if mode >= 0 { (mode as u32) & 0o7777 } else { 0o777 };
        if unsafe { jet_fs_sys::mkdirat(dir, c_name.as_ptr(), bits) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(jet_fs_sys::EEXIST) {
                return Err(IOError::AlreadyExists(jet_fs_context(
                    IOOperation::Write,
                    &shown,
                    error.raw_os_error().map(i64::from),
                    "a directory entry with this name already exists; choose a new name or open the existing directory",
                )));
            }
            return Err(io_error_at(IOOperation::Write, &shown, error));
        }
        return Ok(());
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
        let shown = jet_fs_shown(&jet_fs_held_path(handle, IOOperation::Write)?, name);
        Err(jet_fs_unsupported(IOOperation::Write, &shown, "creating a directory inside a held directory"))
    }
}

/// List a held directory. Each row is one entry: its kind byte (1 directory,
/// 0 anything else; entries are never followed, so a link to a directory is
/// 0) followed by the entry's raw name bytes. Rows are in raw-byte order.
/// Core's `Directory.list` decodes the rows.
pub(crate) fn jet_fs_directory_list(handle: i64) -> Result<Vec<Vec<u8>>, IOError> {
    #[cfg(unix)]
    {
        use jet_fs_sys as sys;
        let base = jet_fs_held_path(handle, IOOperation::Read)?;
        // A fresh descriptor starts reading at the first entry and is owned by
        // the DIR stream below.
        let fresh = jet_fs_open_at(handle, &".".to_string(), 3, 0, -1, 0)?;
        let fd = std::os::fd::IntoRawFd::into_raw_fd(fresh);
        let stream = unsafe { sys::fdopendir(fd) };
        if stream.is_null() {
            let error = jet_fs_last_error(IOOperation::Read, &base);
            drop(unsafe { <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(fd) });
            return Err(error);
        }
        let mut out = Vec::new();
        loop {
            let entry = unsafe { sys::readdir(stream) };
            if entry.is_null() {
                break;
            }
            let kind = unsafe { *entry.add(sys::DIRENT_TYPE) };
            let raw = unsafe { std::ffi::CStr::from_ptr(entry.add(sys::DIRENT_NAME) as *const std::ffi::c_char) };
            let bytes = raw.to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            let is_dir = if kind == sys::DT_UNKNOWN {
                // The file system gave no type: describe the entry itself,
                // never a link target, through the stream's own descriptor.
                #[cfg(target_os = "linux")]
                let flags = sys::O_PATH | sys::O_NOFOLLOW | sys::O_CLOEXEC;
                #[cfg(not(target_os = "linux"))]
                let flags = sys::O_RDONLY | sys::O_SYMLINK | sys::O_NONBLOCK | sys::O_CLOEXEC;
                let probe = unsafe { sys::openat(fd, raw.as_ptr(), flags, 0 as std::ffi::c_uint) };
                if probe < 0 {
                    false
                } else {
                    let file = unsafe { <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(probe) };
                    file.metadata().map(|meta| meta.is_dir()).unwrap_or(false)
                }
            } else {
                kind == sys::DT_DIR
            };
            let mut row = Vec::with_capacity(bytes.len() + 1);
            row.push(is_dir as u8);
            row.extend_from_slice(bytes);
            out.push(row);
        }
        unsafe { sys::closedir(stream) };
        return Ok(out);
    }
    #[cfg(not(unix))]
    {
        let base = jet_fs_held_path(handle, IOOperation::Read)?;
        Err(jet_fs_unsupported(IOOperation::Read, &base, "listing a held directory"))
    }
}

fn jet_fs_time_ns(time: std::io::Result<std::time::SystemTime>) -> Option<i64> {
    let time = time.ok()?;
    Some(match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(after) => after.as_nanos() as i64,
        Err(before) => -(before.duration().as_nanos() as i64),
    })
}

/// One metadata row, in the field order Core's `Stat` constructor reads:
/// kind, size, mode, readonly, device, inode, links, owner (-1: none),
/// size_on_disk, modified_ns, changed_ns, created present (0/1), created_ns.
/// kind: 0 file, 1 directory, 2 symlink, 3 fifo, 4 socket, 5 character
/// device, 6 block device, 7 other.
fn jet_fs_stat_row(meta: &std::fs::Metadata) -> Vec<i64> {
    let file_type = meta.file_type();
    let mut kind = if file_type.is_symlink() {
        2
    } else if file_type.is_dir() {
        1
    } else if file_type.is_file() {
        0
    } else {
        7
    };
    let created = jet_fs_time_ns(meta.created());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        if file_type.is_fifo() {
            kind = 3;
        } else if file_type.is_socket() {
            kind = 4;
        } else if file_type.is_char_device() {
            kind = 5;
        } else if file_type.is_block_device() {
            kind = 6;
        }
        let changed = meta.ctime().saturating_mul(1_000_000_000).saturating_add(meta.ctime_nsec());
        return vec![
            kind,
            meta.len() as i64,
            (meta.mode() & 0o7777) as i64,
            meta.permissions().readonly() as i64,
            meta.dev() as i64,
            meta.ino() as i64,
            meta.nlink() as i64,
            meta.uid() as i64,
            (meta.blocks() as i64).saturating_mul(512),
            meta.mtime().saturating_mul(1_000_000_000).saturating_add(meta.mtime_nsec()),
            changed,
            created.is_some() as i64,
            created.unwrap_or(0),
        ];
    }
    #[cfg(not(unix))]
    {
        let _ = &mut kind;
        let readonly = meta.permissions().readonly();
        let modified = jet_fs_time_ns(meta.modified()).unwrap_or(0);
        vec![
            kind,
            meta.len() as i64,
            if readonly { 0o444 } else { 0o666 },
            readonly as i64,
            0,
            0,
            1,
            -1,
            meta.len() as i64,
            modified,
            modified,
            created.is_some() as i64,
            created.unwrap_or(0),
        ]
    }
}

/// Windows identity comes from an open handle: volume serial, 64-bit file
/// index, and link count.
// AUDIT: GetFileInformationByHandle writes one caller-owned record.
#[cfg(windows)]
fn jet_fs_windows_identity_of(file: &std::fs::File, row: &mut [i64]) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    struct ByHandleFileInformation {
        attributes: u32,
        creation: [u32; 2],
        access: [u32; 2],
        write: [u32; 2],
        volume_serial: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    unsafe extern "system" {
        fn GetFileInformationByHandle(handle: *mut std::ffi::c_void, info: *mut ByHandleFileInformation) -> i32;
    }
    let mut info = std::mem::MaybeUninit::<ByHandleFileInformation>::zeroed();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as *mut std::ffi::c_void, info.as_mut_ptr()) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let info = unsafe { info.assume_init() };
    row[4] = info.volume_serial as i64;
    row[5] = (((info.index_high as u64) << 32) | info.index_low as u64) as i64;
    row[6] = info.links as i64;
    Ok(())
}

/// The identity of `path`, opened without read access; `follow: false` opens
/// a final link itself.
#[cfg(windows)]
fn jet_fs_windows_identity(path: &str, follow: bool, row: &mut [i64]) -> std::io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let mut flags = FILE_FLAG_BACKUP_SEMANTICS;
    if !follow {
        flags |= FILE_FLAG_OPEN_REPARSE_POINT;
    }
    let file = std::fs::OpenOptions::new().access_mode(0).custom_flags(flags).open(path)?;
    jet_fs_windows_identity_of(&file, row)
}

/// D-CORE-FILESTAT1: the metadata row of an open file (fstat), in the same
/// order as `jet_fs_stat_at`. `shown` names the file in an error.
pub(crate) fn jet_fs_stat_file(file: &std::fs::File, shown: &str) -> Result<Vec<i64>, IOError> {
    let meta = file.metadata().map_err(|error| io_error_at(IOOperation::Read, shown, error))?;
    #[allow(unused_mut)]
    let mut row = jet_fs_stat_row(&meta);
    #[cfg(windows)]
    jet_fs_windows_identity_of(file, &mut row)
        .map_err(|error| io_error_at(IOOperation::Read, shown, error))?;
    Ok(row)
}

/// Raw metadata-only classification, without following the final link.
/// Unlike stat's identity row, this never opens a Windows handle; the caller
/// owns target/ancestor validation and wording. 0=file, 1=dir, 2=link, 7=other.
pub(crate) fn jet_fs_path_kind(path: &str) -> Result<i64, IOError> {
    let file_type = std::fs::symlink_metadata(path)
        .map_err(|error| io_error_at(IOOperation::Read, path, error))?
        .file_type();
    Ok(if file_type.is_symlink() {
        2
    } else if file_type.is_dir() {
        1
    } else if file_type.is_file() {
        0
    } else {
        7
    })
}

/// Metadata for `name` relative to `handle` (negative: a path). An empty
/// `name` with a held key describes the held directory. `follow` applies
/// only to a path; held-directory entries are never followed.
pub(crate) fn jet_fs_stat_at(handle: i64, name: &str, follow: bool) -> Result<Vec<i64>, IOError> {
    if handle < 0 {
        let meta = if follow { std::fs::metadata(name) } else { std::fs::symlink_metadata(name) }
            .map_err(|error| io_error_at(IOOperation::Read, name, error))?;
        #[allow(unused_mut)]
        let mut row = jet_fs_stat_row(&meta);
        #[cfg(windows)]
        jet_fs_windows_identity(name, follow, &mut row)
            .map_err(|error| io_error_at(IOOperation::Read, name, error))?;
        return Ok(row);
    }
    if name.is_empty() {
        let held = jet_fs_held().lock().unwrap_or_else(|poison| poison.into_inner());
        let entry = held.get(&handle).ok_or_else(|| {
            IOError::Closed(IOContext::new(
                IOOperation::Read,
                None,
                None,
                Some("the directory handle is already closed".to_string()),
            ))
        })?;
        let meta = entry
            .file
            .metadata()
            .map_err(|error| io_error_at(IOOperation::Read, &entry.path, error))?;
        return Ok(jet_fs_stat_row(&meta));
    }
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        use jet_fs_sys as sys;
        let parent = jet_fs_parent(handle, name, IOOperation::Read)?;
        let dir = parent.fd;
        let shown = parent.shown.clone();
        let c_name = jet_fs_cstring(&parent.last, IOOperation::Read, &shown)?;
        // Open the entry itself, never its link target, without read access.
        #[cfg(target_os = "linux")]
        let flags = sys::O_PATH | sys::O_NOFOLLOW | sys::O_CLOEXEC;
        #[cfg(not(target_os = "linux"))]
        let flags = sys::O_RDONLY | sys::O_SYMLINK | sys::O_NONBLOCK | sys::O_CLOEXEC;
        let fd = unsafe { sys::openat(dir, c_name.as_ptr(), flags, 0 as std::ffi::c_uint) };
        if fd < 0 {
            return Err(jet_fs_last_error(IOOperation::Read, &shown));
        }
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        let meta = file.metadata().map_err(|error| io_error_at(IOOperation::Read, &shown, error))?;
        return Ok(jet_fs_stat_row(&meta));
    }
    #[cfg(not(unix))]
    {
        let shown = jet_fs_shown(&jet_fs_held_path(handle, IOOperation::Read)?, name);
        Err(jet_fs_unsupported(IOOperation::Read, &shown, "inspecting a name inside a held directory"))
    }
}

/// Rename `from` (relative to `from_handle`) to `to` (relative to
/// `to_handle`); negative keys mean the current directory.
/// `if_exists`: 0 replace, 1 refuse an existing target, 2 swap both names.
pub(crate) fn jet_fs_rename_at(from_handle: i64, from: &str, to_handle: i64,
to: &str,
if_exists: i64,) -> Result<(), IOError> {
    #[cfg(unix)]
    {
        use jet_fs_sys as sys;
        let from_parent = jet_fs_parent(from_handle, from, IOOperation::Write)?;
        let to_parent = jet_fs_parent(to_handle, to, IOOperation::Write)?;
        let (from_dir, to_dir) = (from_parent.fd, to_parent.fd);
        let from_shown = from_parent.shown.clone();
        let to_shown = to_parent.shown.clone();
        let c_from = jet_fs_cstring(&from_parent.last, IOOperation::Write, &from_shown)?;
        let c_to = jet_fs_cstring(&to_parent.last, IOOperation::Write, &to_shown)?;
        let refuse_cause = "the target already exists and this rename refuses to replace it; choose a new name or pass if_exists: .Replace";
        let rc = match if_exists {
            0 => unsafe { sys::renameat(from_dir, c_from.as_ptr(), to_dir, c_to.as_ptr()) },
            #[cfg(target_os = "linux")]
            1 => unsafe { sys::renameat2(from_dir, c_from.as_ptr(), to_dir, c_to.as_ptr(), sys::RENAME_NOREPLACE) },
            #[cfg(target_os = "linux")]
            _ => unsafe { sys::renameat2(from_dir, c_from.as_ptr(), to_dir, c_to.as_ptr(), sys::RENAME_EXCHANGE) },
            #[cfg(not(target_os = "linux"))]
            1 => unsafe { sys::renameatx_np(from_dir, c_from.as_ptr(), to_dir, c_to.as_ptr(), sys::RENAME_EXCL) },
            #[cfg(not(target_os = "linux"))]
            _ => unsafe { sys::renameatx_np(from_dir, c_from.as_ptr(), to_dir, c_to.as_ptr(), sys::RENAME_SWAP) },
        };
        if rc == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        let code = error.raw_os_error().unwrap_or(0);
        if code == sys::EEXIST && if_exists == 1 {
            return Err(IOError::AlreadyExists(jet_fs_context(IOOperation::Write, &to_shown, Some(code as i64), refuse_cause)));
        }
        let flag_rejected = code == sys::EINVAL || code == sys::ENOSYS || code == sys::ENOTSUP;
        if if_exists == 1 && flag_rejected {
            // The file system lacks no-replace renames. A file can still be
            // published without replacement: link the new name, then drop the
            // old one. A directory cannot be hard-linked.
            let is_dir = jet_fs_stat_at(from_handle, from, false)
                .map(|row| row.first().copied() == Some(1))
                .unwrap_or(false);
            if is_dir {
                return Err(IOError::Unsupported(jet_fs_context(
                    IOOperation::Write,
                    &from_shown,
                    Some(code as i64),
                    "this file system cannot rename a directory without replacing the target; pass if_exists: .Replace after checking the target yourself",
                )));
            }
            if unsafe { sys::linkat(from_dir, c_from.as_ptr(), to_dir, c_to.as_ptr(), 0) } != 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(sys::EEXIST) {
                    return Err(IOError::AlreadyExists(jet_fs_context(IOOperation::Write, &to_shown, Some(sys::EEXIST as i64), refuse_cause)));
                }
                return Err(io_error_at(IOOperation::Write, &to_shown, error));
            }
            if unsafe { sys::unlinkat(from_dir, c_from.as_ptr(), 0) } != 0 {
                return Err(jet_fs_last_error(IOOperation::Write, &from_shown));
            }
            return Ok(());
        }
        if if_exists == 2 && flag_rejected {
            return Err(IOError::Unsupported(jet_fs_context(
                IOOperation::Write,
                &from_shown,
                Some(code as i64),
                "this file system cannot swap two names in one step; replace in two steps instead",
            )));
        }
        return Err(io_error_at(IOOperation::Write, &from_shown, error));
    }
    #[cfg(not(unix))]
    {
        if from_handle >= 0 || to_handle >= 0 {
            return Err(jet_fs_unsupported(IOOperation::Write, from, "renaming inside a held directory"));
        }
        if if_exists == 2 {
            return Err(jet_fs_unsupported(IOOperation::Write, from, "swapping two names in one step"));
        }
        if if_exists == 1 && std::fs::symlink_metadata(to).is_ok() {
            return Err(IOError::AlreadyExists(jet_fs_context(
                IOOperation::Write,
                to,
                None,
                "the target already exists and this rename refuses to replace it; choose a new name or pass if_exists: .Replace",
            )));
        }
        std::fs::rename(from, to).map_err(|error| io_error_at(IOOperation::Write, from, error))
    }
}

/// Remove `name` relative to a held directory: a non-directory when
/// `directory` is false, an empty directory when it is true.
pub(crate) fn jet_fs_remove_at(handle: i64, name: &str, directory: bool) -> Result<(), IOError> {
    #[cfg(unix)]
    {
        let parent = jet_fs_parent(handle, name, IOOperation::Write)?;
        let dir = parent.fd;
        let shown = parent.shown.clone();
        let c_name = jet_fs_cstring(&parent.last, IOOperation::Write, &shown)?;
        let flags = if directory { jet_fs_sys::AT_REMOVEDIR } else { 0 };
        if unsafe { jet_fs_sys::unlinkat(dir, c_name.as_ptr(), flags) } != 0 {
            return Err(jet_fs_last_error(IOOperation::Write, &shown));
        }
        return Ok(());
    }
    #[cfg(not(unix))]
    {
        let _ = directory;
        let shown = jet_fs_shown(&jet_fs_held_path(handle, IOOperation::Write)?, name);
        Err(jet_fs_unsupported(IOOperation::Write, &shown, "removing a name inside a held directory"))
    }
}

/// The text stored in the link `name` inside a held directory, unresolved.
pub(crate) fn jet_fs_read_link_at(handle: i64, name: &str) -> Result<String, IOError> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let parent = jet_fs_parent(handle, name, IOOperation::Read)?;
        let dir = parent.fd;
        let shown = parent.shown.clone();
        let c_name = jet_fs_cstring(&parent.last, IOOperation::Read, &shown)?;
        let mut buf = vec![0u8; 256];
        loop {
            let got = unsafe {
                jet_fs_sys::readlinkat(dir, c_name.as_ptr(), buf.as_mut_ptr() as *mut std::ffi::c_char, buf.len())
            };
            if got < 0 {
                return Err(jet_fs_last_error(IOOperation::Read, &shown));
            }
            let got = got as usize;
            if got < buf.len() {
                buf.truncate(got);
                break;
            }
            buf.resize(buf.len() * 2, 0);
        }
        return std::ffi::OsString::from_vec(buf).into_string().map_err(|_| {
            IOError::InvalidInput(jet_fs_context(IOOperation::Read, &shown, None, "the link text is not valid UTF-8"))
        });
    }
    #[cfg(not(unix))]
    {
        let shown = jet_fs_shown(&jet_fs_held_path(handle, IOOperation::Read)?, name);
        Err(jet_fs_unsupported(IOOperation::Read, &shown, "reading a link inside a held directory"))
    }
}

/// Extended-attribute names on `path` itself (a final link is not followed),
/// sorted. A file system without attributes, and Windows, report none.
pub(crate) fn jet_fs_xattr_names(path: &str) -> Result<Vec<String>, IOError> {
    #[cfg(unix)]
    {
        use jet_fs_sys as sys;
        let c_path = jet_fs_cstring(path, IOOperation::Read, path)?;
        let list = |buf: *mut std::ffi::c_char, size: usize| -> isize {
            #[cfg(target_os = "linux")]
            unsafe {
                sys::llistxattr(c_path.as_ptr(), buf, size)
            }
            #[cfg(not(target_os = "linux"))]
            unsafe {
                sys::listxattr(c_path.as_ptr(), buf, size, sys::XATTR_NOFOLLOW)
            }
        };
        let mut buf: Vec<u8> = Vec::new();
        loop {
            let size = list(std::ptr::null_mut(), 0);
            if size < 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(sys::ENOTSUP) {
                    return Ok(Vec::new());
                }
                return Err(io_error_at(IOOperation::Read, path, error));
            }
            buf.resize(size as usize, 0);
            if size == 0 {
                break;
            }
            let got = list(buf.as_mut_ptr() as *mut std::ffi::c_char, buf.len());
            if got >= 0 {
                buf.truncate(got as usize);
                break;
            }
            let error = std::io::Error::last_os_error();
            // ERANGE: the list grew between the two calls; size it again.
            if error.raw_os_error() != Some(34) {
                return Err(io_error_at(IOOperation::Read, path, error));
            }
        }
        let mut names = Vec::new();
        for raw in buf.split(|byte| *byte == 0).filter(|raw| !raw.is_empty()) {
            let name = String::from_utf8(raw.to_vec()).map_err(|_| {
                IOError::InvalidInput(jet_fs_context(
                    IOOperation::Read,
                    path,
                    None,
                    "an extended attribute name is not valid UTF-8",
                ))
            })?;
            names.push(name);
        }
        names.sort();
        return Ok(names);
    }
    #[cfg(not(unix))]
    {
        std::fs::symlink_metadata(path).map_err(|error| io_error_at(IOOperation::Read, path, error))?;
        Ok(Vec::new())
    }
}

/// One extended-attribute value on `path` itself, or None when absent or
/// when the file system has no attributes.
pub(crate) fn jet_fs_xattr_get(path: &str, name: &str) -> Result<Option<Vec<u8>>, IOError> {
    #[cfg(unix)]
    {
        use jet_fs_sys as sys;
        let c_path = jet_fs_cstring(path, IOOperation::Read, path)?;
        let c_name = jet_fs_cstring(name, IOOperation::Read, path)?;
        let get = |buf: *mut std::ffi::c_void, size: usize| -> isize {
            #[cfg(target_os = "linux")]
            unsafe {
                sys::lgetxattr(c_path.as_ptr(), c_name.as_ptr(), buf, size)
            }
            #[cfg(not(target_os = "linux"))]
            unsafe {
                sys::getxattr(c_path.as_ptr(), c_name.as_ptr(), buf, size, 0, sys::XATTR_NOFOLLOW)
            }
        };
        loop {
            let size = get(std::ptr::null_mut(), 0);
            if size < 0 {
                let error = std::io::Error::last_os_error();
                let code = error.raw_os_error().unwrap_or(0);
                if code == sys::ENOATTR || code == sys::ENOTSUP {
                    return Ok(None);
                }
                return Err(io_error_at(IOOperation::Read, path, error));
            }
            let mut buf = vec![0u8; size as usize];
            if size == 0 {
                return Ok(Some(buf));
            }
            let got = get(buf.as_mut_ptr() as *mut std::ffi::c_void, buf.len());
            if got >= 0 {
                buf.truncate(got as usize);
                return Ok(Some(buf));
            }
            let error = std::io::Error::last_os_error();
            let code = error.raw_os_error().unwrap_or(0);
            if code == sys::ENOATTR {
                return Ok(None);
            }
            if code != 34 {
                return Err(io_error_at(IOOperation::Read, path, error));
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = name;
        std::fs::symlink_metadata(path).map_err(|error| io_error_at(IOOperation::Read, path, error))?;
        Ok(None)
    }
}

/// Write one extended-attribute value on `path` itself. A file system or
/// platform without attributes reports `IOError::Unsupported`.
pub(crate) fn jet_fs_xattr_set(path: &str, name: &str, value: &Vec<u8>) -> Result<(), IOError> {
    #[cfg(unix)]
    {
        use jet_fs_sys as sys;
        let c_path = jet_fs_cstring(path, IOOperation::Write, path)?;
        let c_name = jet_fs_cstring(name, IOOperation::Write, path)?;
        #[cfg(target_os = "linux")]
        let rc = unsafe {
            sys::lsetxattr(c_path.as_ptr(), c_name.as_ptr(), value.as_ptr() as *const std::ffi::c_void, value.len(), 0)
        };
        #[cfg(not(target_os = "linux"))]
        let rc = unsafe {
            sys::setxattr(c_path.as_ptr(), c_name.as_ptr(), value.as_ptr() as *const std::ffi::c_void, value.len(), 0, sys::XATTR_NOFOLLOW)
        };
        if rc != 0 {
            return Err(jet_fs_last_error(IOOperation::Write, path));
        }
        return Ok(());
    }
    #[cfg(not(unix))]
    {
        let _ = (name, value);
        Err(jet_fs_unsupported(IOOperation::Write, path, "extended attributes"))
    }
}
// JET_VETTED_UNSAFE_END: jet_fs_handles

// A compiler-private descriptor-to-held-directory hard link. The caller
// guarantees a live open file and a single, NUL-free plain name. Availability
// and held-key presence are facts, not decisions about when to copy.
pub(crate) fn jet_fs_link_fd_at_raw(source: &std::fs::File, into: i64, name: &str, ) -> (bool, bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::fd::AsRawFd;
        use jet_foundation::Numeric::JetInt;
        let held = jet_fs_held().lock().unwrap_or_else(|poison| poison.into_inner());
        let Some(destination) = held.get(&into) else {
            return (true, false, JetInt::from_i64(0), JetInt::from_i64(0));
        };
        let name = unsafe { std::ffi::CString::from_vec_unchecked(name.as_bytes().to_vec()) };
        // Linux AT_EMPTY_PATH names the inode through the open descriptor.
        // In particular, do not fall back to reopening the old source path.
        let value = unsafe {
            jet_fs_sys::linkat(source.as_raw_fd(), c"".as_ptr(),
                destination.file.as_raw_fd(), name.as_ptr(), 0x1000)
        };
        let errno = std::io::Error::last_os_error().raw_os_error().expect("last_os_error contains the raw OS code");
        (true, true, JetInt::from_i64(value as i64), JetInt::from_i64(errno as i64))
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (source, into, name);
        (false, false, jet_foundation::Numeric::JetInt::from_i64(0), jet_foundation::Numeric::JetInt::from_i64(0))
    }
}

/// Raw following-metadata fact. Jet chooses the public false-on-error policy.
pub(crate) fn jet_fs_metadata_directory_raw(path: &str) -> Result<bool, String> {
    std::fs::metadata(path).map(|metadata| metadata.is_dir()).map_err(|error| error.to_string())
}

/// Host path grammar facts; Jet owns the current-directory/error policy.
pub(crate) fn jet_fs_host_path_is_absolute_raw(path: &str) -> bool {
    std::path::Path::new(path).is_absolute()
}

pub(crate) fn jet_fs_host_path_join_raw(base: &str, path: &str) -> String {
    std::path::Path::new(base).join(path).to_string_lossy().into_owned()
}

/// Unsigned Unix-nanosecond clock facts. The text carrier preserves the full
/// u128 sample; Jet chooses the unavailable fallback.
pub(crate) fn jet_fs_temp_path_sample_raw() -> (bool, String) {
    let clock = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    let (available, nanos) = match clock {
        Ok(duration) => (true, duration.as_nanos().to_string()),
        Err(_) => (false, String::new()),
    };
    (available, nanos)
}

pub(crate) fn jet_fs_temp_directory_raw() -> String {
    std::env::temp_dir().to_string_lossy().into_owned()
}

/// A borrowed descriptor fact; the audited Core caller retains the Directory
/// until the process attachment leaf duplicates it.
pub(crate) fn jet_fs_directory_fd_raw(handle: i64) -> (bool, bool, jet_foundation::Numeric::JetInt) {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::fd::AsRawFd;
        let held = jet_fs_held().lock().unwrap_or_else(|poison| poison.into_inner());
        match held.get(&handle) {
            Some(entry) => (true, true, jet_foundation::Numeric::JetInt::from_i64(entry.file.as_raw_fd() as i64)),
            None => (true, false, jet_foundation::Numeric::JetInt::from_i64(0)),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = handle;
        (false, false, jet_foundation::Numeric::JetInt::from_i64(0))
    }
}

// JET_VETTED_UNSAFE_BEGIN: jet_process_held_directory
// AUDIT: caller lends a live held fd. F_DUPFD_CLOEXEC returns the assigned
// descriptor fact; Jet admits only the requested free slot, retained by the
// carrier so spawn's private pipes cannot occupy it. Before fork the source
// copies move above every target; child code is async-signal-safe dup2/fchdir
// only, without paths, allocation, or locks.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod jet_process_directory_sys {
    unsafe extern "C" {
        pub fn fcntl(fd: i32, command: i32, ...) -> i32;
        pub fn dup2(source: i32, destination: i32) -> i32;
        pub fn fchdir(fd: i32) -> i32;
    }
    #[cfg(target_os = "linux")]
    pub const F_DUPFD_CLOEXEC: i32 = 1030;
    #[cfg(target_os = "macos")]
    pub const F_DUPFD_CLOEXEC: i32 = 67;
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn jet_fs_process_fd_duplicate(fd: i32, minimum: i32) -> std::io::Result<std::fs::File> {
    use std::os::fd::FromRawFd;
    let owned = unsafe { jet_process_directory_sys::fcntl(fd, jet_process_directory_sys::F_DUPFD_CLOEXEC, minimum) };
    if owned < 0 { return Err(std::io::Error::last_os_error()); }
    Ok(unsafe { std::fs::File::from_raw_fd(owned) })
}

pub(crate) fn jet_process_attach_held_directories(
    command: &mut std::process::Command, directories: &[JetProcessDirectory],
) -> std::io::Result<()> {
    if directories.is_empty() { return Ok(()); }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        let minimum = directories.iter().map(|entry| entry.child_fd).max().unwrap() + 1;
        let mappings = directories.iter().map(|entry| {
            jet_fs_process_fd_duplicate(entry.file.as_raw_fd(), minimum)
                .map(|file| (file, entry.child_fd, entry.as_cwd))
        }).collect::<std::io::Result<Vec<_>>>()?;
        unsafe {
            command.pre_exec(move || {
                for (file, child_fd, as_cwd) in &mappings {
                    if jet_process_directory_sys::dup2(file.as_raw_fd(), *child_fd) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if *as_cwd && jet_process_directory_sys::fchdir(*child_fd) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = command;
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }
}
// JET_VETTED_UNSAFE_END: jet_process_held_directory

#[cfg(test)]
mod path_kind_tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!(
                "jet-path-kind-{}-{now}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn metadata_kind_preserves_links_and_os_errors() {
        let fixture = Fixture::new();
        let root = fixture.0.to_string_lossy().into_owned();
        let file = fixture.0.join("state");
        std::fs::write(&file, b"state").unwrap();
        let shown = file.to_string_lossy().into_owned();
        assert_eq!(jet_fs_path_kind(&root).unwrap(), 1);
        assert_eq!(jet_fs_path_kind(&shown).unwrap(), 0);
        let missing = fixture.0.join("missing").to_string_lossy().into_owned();
        assert!(matches!(jet_fs_path_kind(&missing), Err(IOError::NotFound(_))));
        let obstructed = file.join("child").to_string_lossy().into_owned();
        let error = std::fs::symlink_metadata(&obstructed).unwrap_err();
        assert_eq!(
            format!("{:?}", jet_fs_path_kind(&obstructed).unwrap_err()),
            format!("{:?}", io_error_at(IOOperation::Read, &obstructed, error)),
        );
        #[cfg(unix)]
        {
            let link = fixture.0.join("link");
            std::os::unix::fs::symlink(&file, &link).unwrap();
            assert_eq!(jet_fs_path_kind(&link.to_string_lossy().into_owned()).unwrap(), 2);
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_exclusive_file_matches_metadata_without_an_identity_open() {
        use std::os::windows::fs::OpenOptionsExt;
        let fixture = Fixture::new();
        let path = fixture.0.join("state");
        std::fs::write(&path, b"state").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let shown = path.to_string_lossy().into_owned();
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                assert!(metadata.is_file());
                assert_eq!(jet_fs_path_kind(&shown).unwrap(), 0);
            }
            Err(error) => assert_eq!(
                format!("{:?}", jet_fs_path_kind(&shown).unwrap_err()),
                format!("{:?}", io_error_at(IOOperation::Read, &shown, error)),
            ),
        }
        drop(held);
    }
}
