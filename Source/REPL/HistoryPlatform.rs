//! Platform-secure storage backend for REPL history.

#[cfg(any(unix, windows))]
use super::render;
use std::io;
#[cfg(windows)]
use std::path::PathBuf;

#[cfg(unix)]
pub(super) struct Backend {
    dir: std::fs::File,
}

#[cfg(unix)]
pub(super) struct BackendLock {
    file: std::fs::File,
}

#[cfg(unix)]
impl Drop for BackendLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }
        const LOCK_UN: i32 = 8;
        let _ = unsafe { flock(self.file.as_raw_fd(), LOCK_UN) };
    }
}

#[cfg(unix)]
impl Backend {
    pub(super) fn open(root: &std::path::Path) -> io::Result<Self> {
        use std::ffi::{CString, OsStr};
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        use std::path::Component;

        const O_RDONLY: i32 = 0;
        const O_CLOEXEC: i32 = 0o2000000;
        const O_DIRECTORY: i32 = 0o200000;
        const O_NOFOLLOW: i32 = 0o400000;
        unsafe extern "C" {
            fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32;
            fn mkdirat(dirfd: i32, pathname: *const i8, mode: u32) -> i32;
        }
        fn name(value: &OsStr) -> io::Result<CString> {
            CString::new(value.as_bytes())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in state path"))
        }
        fn descend(parent: &std::fs::File, part: &OsStr, create: bool) -> io::Result<std::fs::File> {
            let part = name(part)?;
            let flags = O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC;
            let mut fd = unsafe { openat(parent.as_raw_fd(), part.as_ptr(), flags, 0) };
            if fd < 0 && create && io::Error::last_os_error().kind() == io::ErrorKind::NotFound {
                if unsafe { mkdirat(parent.as_raw_fd(), part.as_ptr(), 0o700) } != 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(error);
                    }
                }
                fd = unsafe { openat(parent.as_raw_fd(), part.as_ptr(), flags, 0) };
            }
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(unsafe { std::fs::File::from_raw_fd(fd) })
        }

        if !root.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "platform state directory is not absolute",
            ));
        }
        let mut current = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            .open("/")?;
        for component in root.components() {
            match component {
                Component::RootDir => {}
                Component::Normal(part) => current = descend(&current, part, true)?,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "platform state path has a non-normal component",
                    ))
                }
            }
        }
        let dir = descend(&current, OsStr::new("jet"), true)?;
        dir.set_permissions(std::fs::Permissions::from_mode(0o700))?;
        Ok(Self { dir })
    }

    pub(super) fn lock(&self) -> io::Result<BackendLock> {
        use std::os::fd::AsRawFd;
        use std::time::{Duration, Instant};
        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }
        const LOCK_EX: i32 = 2;
        const LOCK_NB: i32 = 4;
        let file = self.dir.try_clone()?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } == 0 {
                return Ok(BackendLock { file });
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "history transaction lock timed out after 2 seconds",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub(super) fn read(&self) -> io::Result<Option<Vec<u8>>> {
        use std::ffi::CString;
        use std::io::Read;
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        const O_RDONLY: i32 = 0;
        const O_CLOEXEC: i32 = 0o2000000;
        const O_NOFOLLOW: i32 = 0o400000;
        unsafe extern "C" {
            fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32;
        }
        let name = CString::new("repl-history").unwrap();
        let fd = unsafe {
            openat(
                self.dir.as_raw_fd(),
                name.as_ptr(),
                O_RDONLY | O_NOFOLLOW | O_CLOEXEC,
                0,
            )
        };
        if fd < 0 {
            let error = io::Error::last_os_error();
            return if error.kind() == io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(error)
            };
        }
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "history object is linked or not a regular file",
            ));
        }
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }

    pub(super) fn rewrite(&self, entries: &[String]) -> io::Result<()> {
        use std::ffi::CString;
        use std::io::Write;
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::sync::atomic::{AtomicU64, Ordering};
        const O_WRONLY: i32 = 1;
        const O_CREAT: i32 = 0o100;
        const O_EXCL: i32 = 0o200;
        const O_CLOEXEC: i32 = 0o2000000;
        const O_NOFOLLOW: i32 = 0o400000;
        static NEXT_TMP: AtomicU64 = AtomicU64::new(0);
        unsafe extern "C" {
            fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32;
            fn renameat(olddirfd: i32, old: *const i8, newdirfd: i32, new: *const i8) -> i32;
            fn unlinkat(dirfd: i32, pathname: *const i8, flags: i32) -> i32;
            fn fsync(fd: i32) -> i32;
        }
        let temp = CString::new(format!(
            ".repl-history.{}.{}.tmp",
            std::process::id(),
            NEXT_TMP.fetch_add(1, Ordering::Relaxed)
        ))
        .unwrap();
        let destination = CString::new("repl-history").unwrap();
        let fd = unsafe {
            openat(
                self.dir.as_raw_fd(),
                temp.as_ptr(),
                O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        let result = (|| {
            file.write_all(&render(entries))?;
            file.sync_all()?;
            if unsafe {
                renameat(
                    self.dir.as_raw_fd(),
                    temp.as_ptr(),
                    self.dir.as_raw_fd(),
                    destination.as_ptr(),
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            if unsafe { fsync(self.dir.as_raw_fd()) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = unsafe { unlinkat(self.dir.as_raw_fd(), temp.as_ptr(), 0) };
        }
        result
    }

    pub(super) fn clear(&self) -> io::Result<()> {
        use std::ffi::CString;
        use std::os::fd::AsRawFd;
        unsafe extern "C" {
            fn unlinkat(dirfd: i32, pathname: *const i8, flags: i32) -> i32;
            fn fsync(fd: i32) -> i32;
        }
        let name = CString::new("repl-history").unwrap();
        if unsafe { unlinkat(self.dir.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::NotFound {
                return Err(error);
            }
        }
        if unsafe { fsync(self.dir.as_raw_fd()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(windows)]
pub(super) struct Backend {
    dir: PathBuf,
    dir_file: std::fs::File,
    dir_final: String,
}

#[cfg(windows)]
pub(super) struct BackendLock {
    file: std::fs::File,
    overlapped: WindowsOverlapped,
}

#[cfg(windows)]
impl Drop for BackendLock {
    fn drop(&mut self) {
        use std::ffi::c_void;
        use std::os::windows::io::AsRawHandle;
        unsafe extern "system" {
            fn UnlockFileEx(
                file: *mut c_void,
                reserved: u32,
                bytes_low: u32,
                bytes_high: u32,
                overlapped: *mut WindowsOverlapped,
            ) -> i32;
        }
        let _ = unsafe {
            UnlockFileEx(
                self.file.as_raw_handle(),
                0,
                1,
                0,
                &mut self.overlapped,
            )
        };
    }
}

#[cfg(windows)]
#[repr(C)]
struct WindowsOverlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: *mut std::ffi::c_void,
}

#[cfg(windows)]
impl WindowsOverlapped {
    fn zeroed() -> Self {
        Self {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            event: std::ptr::null_mut(),
        }
    }
}

#[cfg(windows)]
impl Backend {
    pub(super) fn open(root: &std::path::Path) -> io::Result<Self> {
        use std::path::Component;
        if !root.is_absolute() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "state path is not absolute"));
        }
        let mut current = PathBuf::new();
        for component in root.components() {
            match component {
                Component::Prefix(_) | Component::RootDir => current.push(component.as_os_str()),
                Component::Normal(part) => {
                    current.push(part);
                    if current.exists() {
                        reject_windows_reparse(&current, true)?;
                    } else {
                        std::fs::create_dir(&current)?;
                        reject_windows_reparse(&current, true)?;
                    }
                }
                _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "non-normal state path")),
            }
        }
        current.push("jet");
        if current.exists() {
            reject_windows_reparse(&current, true)?;
        } else {
            std::fs::create_dir(&current)?;
            reject_windows_reparse(&current, true)?;
        }
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        let dir_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .access_mode(0x8000_0000 | 0x4000_0000)
            .share_mode(0x0000_0007)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&current)?;
        let dir_final = windows_final_path(&dir_file)?;
        Ok(Self {
            dir: current,
            dir_file,
            dir_final,
        })
    }

    fn verify(&self) -> io::Result<()> {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        reject_windows_reparse(&self.dir, true)?;
        let current = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x0000_0007)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&self.dir)?;
        if windows_final_path(&current)? != self.dir_final {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "state directory identity changed",
            ));
        }
        Ok(())
    }

    pub(super) fn lock(&self) -> io::Result<BackendLock> {
        use std::ffi::c_void;
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use std::time::{Duration, Instant};
        unsafe extern "system" {
            fn LockFileEx(
                file: *mut c_void,
                flags: u32,
                reserved: u32,
                bytes_low: u32,
                bytes_high: u32,
                overlapped: *mut WindowsOverlapped,
            ) -> i32;
        }
        const LOCKFILE_FAIL_IMMEDIATELY: u32 = 1;
        const LOCKFILE_EXCLUSIVE_LOCK: u32 = 2;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        self.verify()?;
        let path = self.dir.join(".repl-history.lock");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .share_mode(0x0000_0007)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)?;
        reject_windows_reparse(&path, false)?;
        self.verify_child(&file)?;
        let mut overlapped = WindowsOverlapped::zeroed();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            overlapped = WindowsOverlapped::zeroed();
            if unsafe {
                LockFileEx(
                    file.as_raw_handle(),
                    LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                    0,
                    1,
                    0,
                    &mut overlapped,
                )
            } != 0
            {
                return Ok(BackendLock { file, overlapped });
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "history transaction lock timed out after 2 seconds",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub(super) fn read(&self) -> io::Result<Option<Vec<u8>>> {
        use std::io::Read;
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        self.verify()?;
        let path = self.dir.join("repl-history");
        if !path.exists() {
            return Ok(None);
        }
        reject_windows_reparse(&path, false)?;
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x0000_0007)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        self.verify_child(&file)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }

    pub(super) fn rewrite(&self, entries: &[String]) -> io::Result<()> {
        use std::ffi::c_void;
        use std::io::Write;
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_TMP: AtomicU64 = AtomicU64::new(0);
        self.verify()?;
        let temp = self.dir.join(format!(
            ".repl-history.{}.{}.tmp",
            std::process::id(),
            NEXT_TMP.fetch_add(1, Ordering::Relaxed)
        ));
        const GENERIC_WRITE: u32 = 0x4000_0000;
        const DELETE: u32 = 0x0001_0000;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .access_mode(GENERIC_WRITE | DELETE)
            .share_mode(0x0000_0007)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&temp)?;
        self.verify_child(&file)?;
        file.write_all(&render(entries))?;
        file.sync_all()?;
        reject_windows_reparse(&temp, false)?;
        const FILE_RENAME_INFO_CLASS: i32 = 3;
        #[repr(C)]
        struct FileRenameInfo {
            replace_if_exists: i32,
            root_directory: *mut c_void,
            file_name_length: u32,
            file_name: [u16; 1],
        }
        unsafe extern "system" {
            fn SetFileInformationByHandle(
                file: *mut c_void,
                class: i32,
                info: *mut c_void,
                size: u32,
            ) -> i32;
            fn FlushFileBuffers(file: *mut c_void) -> i32;
        }
        let file_name = std::ffi::OsStr::new("repl-history")
            .encode_wide()
            .collect::<Vec<_>>();
        let name_bytes = file_name.len() * std::mem::size_of::<u16>();
        let size = std::mem::size_of::<FileRenameInfo>() + name_bytes.saturating_sub(2);
        let mut buffer = vec![0u64; size.div_ceil(std::mem::size_of::<u64>())];
        let info = buffer.as_mut_ptr().cast::<FileRenameInfo>();
        unsafe {
            (*info).replace_if_exists = 1;
            (*info).root_directory = self.dir_file.as_raw_handle();
            (*info).file_name_length = name_bytes as u32;
            std::ptr::copy_nonoverlapping(
                file_name.as_ptr(),
                (*info).file_name.as_mut_ptr(),
                file_name.len(),
            );
        }
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FILE_RENAME_INFO_CLASS,
                buffer.as_mut_ptr().cast(),
                size as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if unsafe { FlushFileBuffers(self.dir_file.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn clear(&self) -> io::Result<()> {
        use std::ffi::c_void;
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        const DELETE: u32 = 0x0001_0000;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const FILE_DISPOSITION_INFO_CLASS: i32 = 4;
        #[repr(C)]
        struct FileDispositionInfo {
            delete_file: u8,
        }
        unsafe extern "system" {
            fn SetFileInformationByHandle(
                file: *mut c_void,
                class: i32,
                info: *mut c_void,
                size: u32,
            ) -> i32;
        }
        self.verify()?;
        let path = self.dir.join("repl-history");
        let file = match std::fs::OpenOptions::new()
            .write(true)
            .access_mode(DELETE)
            .share_mode(0x0000_0007)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        self.verify_child(&file)?;
        let mut disposition = FileDispositionInfo { delete_file: 1 };
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FILE_DISPOSITION_INFO_CLASS,
                (&mut disposition as *mut FileDispositionInfo).cast(),
                std::mem::size_of::<FileDispositionInfo>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn verify_child(&self, file: &std::fs::File) -> io::Result<()> {
        let child = windows_final_path(file)?;
        let parent = child
            .rsplit_once(['\\', '/'])
            .map(|(parent, _)| parent)
            .unwrap_or("");
        if parent.trim_end_matches(['\\', '/']) != self.dir_final.trim_end_matches(['\\', '/']) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "history object escaped held state directory",
            ));
        }
        Ok(())
    }
}

#[cfg(windows)]
fn windows_final_path(file: &std::fs::File) -> io::Result<String> {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    unsafe extern "system" {
        fn GetFinalPathNameByHandleW(
            file: *mut c_void,
            path: *mut u16,
            path_len: u32,
            flags: u32,
        ) -> u32;
    }
    let needed = unsafe { GetFinalPathNameByHandleW(file.as_raw_handle(), std::ptr::null_mut(), 0, 0) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0u16; needed as usize];
    let written = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            0,
        )
    };
    if written == 0 || written as usize >= buffer.len() {
        return Err(io::Error::last_os_error());
    }
    buffer.truncate(written as usize);
    Ok(String::from_utf16_lossy(&buffer).to_lowercase())
}

#[cfg(windows)]
fn reject_windows_reparse(path: &std::path::Path, directory: bool) -> io::Result<()> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    type Handle = *mut c_void;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    unsafe extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            creation: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;
        fn GetFileAttributesW(name: *const u16) -> u32;
        fn CloseHandle(handle: Handle) -> i32;
    }
    let wide = path.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let attrs = unsafe { GetFileAttributesW(wide.as_ptr()) };
    if attrs == u32::MAX || attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "state path contains a Windows reparse point",
        ));
    }
    let flags = FILE_FLAG_OPEN_REPARSE_POINT
        | if directory { FILE_FLAG_BACKUP_SEMANTICS } else { 0 };
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            0x8000_0000,
            0x0000_0007,
            std::ptr::null_mut(),
            3,
            flags,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let _ = unsafe { CloseHandle(handle) };
    Ok(())
}

#[cfg(all(not(unix), not(windows)))]
pub(super) struct Backend;

#[cfg(all(not(unix), not(windows)))]
pub(super) struct BackendLock;

#[cfg(all(not(unix), not(windows)))]
impl Backend {
    pub(super) fn open(_root: &std::path::Path) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "secure history storage is unsupported on this platform",
        ))
    }

    pub(super) fn lock(&self) -> io::Result<BackendLock> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "history unavailable"))
    }

    pub(super) fn read(&self) -> io::Result<Option<Vec<u8>>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "history unavailable"))
    }

    pub(super) fn rewrite(&self, _entries: &[String]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "history unavailable"))
    }

    pub(super) fn clear(&self) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "history unavailable"))
    }
}
