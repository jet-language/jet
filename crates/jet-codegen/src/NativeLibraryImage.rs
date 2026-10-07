//! Private dynamic-image transport shared by the parent runtime host.
//! Adapted from NativeSession's private Image: no public Lib grant policy.
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::path::Path;

// JET_VETTED_UNSAFE_BEGIN: parent_runtime_image
// AUDIT: callers validate pinned bytes/export/lifecycle seals before open_sealed.
// The immutable memfd closes the pathname race before constructors execute.
// Symbol addresses borrow the image; the resident owner never unloads it.
#[link(name = "dl")]
extern "C" {
    fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *const c_char;
    fn memfd_create(name: *const c_char, flags: u32) -> c_int;
    fn fcntl(fd: c_int, command: c_int, ...) -> c_int;
}

pub(crate) struct Image {
    handle: usize,
    _file: Option<File>,
}

impl Image {
    pub(crate) unsafe fn open(path: &Path, global: bool) -> io::Result<Self> {
        use std::os::unix::ffi::OsStrExt;
        let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "native image path contains NUL",
            )
        })?;
        // RTLD_NOW = 2; RTLD_GLOBAL = 0x100 on the Linux/x86-64 host.
        // SAFETY: path is terminated and retained through dlopen. The caller
        // vouches for code/dependencies; constructors execute during this call.
        let handle = unsafe { dlopen(path.as_ptr(), 2 | if global { 0x100 } else { 0 }) };
        if handle.is_null() {
            Err(loader_error())
        } else {
            Ok(Self {
                handle: handle as usize,
                _file: None,
            })
        }
    }

    pub(crate) fn symbol(&self, name: &CStr) -> io::Result<*mut c_void> {
        // SAFETY: self owns a live image and name is terminated.
        unsafe {
            dlerror();
        }
        let symbol = unsafe { dlsym(self.handle as *mut c_void, name.as_ptr()) };
        if symbol.is_null() {
            Err(loader_error())
        } else {
            Ok(symbol)
        }
    }
    pub(crate) fn open_sealed(bytes: &[u8]) -> io::Result<Self> {
        let name = CString::new("jet-parent-runtime").expect("literal contains no NUL");
        // SAFETY: terminated name is live; MFD_CLOEXEC|MFD_ALLOW_SEALING=3.
        let fd = unsafe { memfd_create(name.as_ptr(), 3) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: memfd_create returned a fresh descriptor owned by this File.
        let mut file = unsafe { File::from_raw_fd(fd) };
        file.write_all(bytes)?;
        // SAFETY: F_ADD_SEALS=1033; SEAL|SHRINK|GROW|WRITE=15 forbids mutation.
        if unsafe { fcntl(file.as_raw_fd(), 1033, 15 as c_int) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let path = CString::new(format!("/proc/self/fd/{}", file.as_raw_fd()))
            .expect("descriptor path contains no NUL");
        // SAFETY: this path names the immutable descriptor whose exact bytes
        // passed admission. Reuse the shared private image opening mechanism.
        let mut image = unsafe {
            Self::open(
                std::path::Path::new(path.to_str().expect("ASCII descriptor path")),
                false,
            )
        }?;
        image._file = Some(file);
        Ok(image)
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        // SAFETY: the image is no longer referenced; Host drops all slots first.
        unsafe {
            dlclose(self.handle as *mut c_void);
        }
    }
}

fn loader_error() -> io::Error {
    // SAFETY: dlerror returns null or a terminated string borrowed until the
    // next loader call on this thread; copy it before another call.
    let ptr = unsafe { dlerror() };
    let message = if ptr.is_null() {
        "native image has no requested symbol".into()
    } else {
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    };
    io::Error::other(message)
}

// JET_VETTED_UNSAFE_END: parent_runtime_image
