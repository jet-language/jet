// core.os identity, facts, and POSIX control (ledger #1465 / D-OSFACTS1).
// Included after FSIoEnvOsTesting.rs at crate root (same level as jet_std).

fn jet_os_unsupported(op: &str) -> jet_std::IOError {
    jet_std::IOError::other(
        jet_std::IOOperation::Resolve,
        None,
        std::io::Error::new(std::io::ErrorKind::Unsupported, format!("{op} is POSIX-only")),
    )
}

fn jet_std_os_release() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(text) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        if let Ok(text) = std::fs::read_to_string("/etc/os-release") {
            for line in text.lines() {
                if let Some(value) = line.strip_prefix("VERSION_ID=") {
                    return value.trim_matches('"').to_string();
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("uname").arg("-r").output() {
            if let Ok(s) = String::from_utf8(output.stdout) {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
    }
    #[cfg(windows)]
    {
        if let Ok(v) = std::env::var("OS") {
            if !v.is_empty() {
                return v;
            }
        }
    }
    String::new()
}

fn jet_std_os_version() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(text) = std::fs::read_to_string("/etc/os-release") {
            for line in text.lines() {
                if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
                    return value.trim_matches('"').to_string();
                }
            }
        }
        return jet_std_os_release();
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
        {
            if let Ok(s) = String::from_utf8(output.stdout) {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    return format!("macOS {trimmed}");
                }
            }
        }
        return jet_std_os_release();
    }
    #[cfg(windows)]
    {
        return std::env::var("OS").unwrap_or_default();
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        String::new()
    }
}

fn jet_std_os_getpid() -> i64 {
    jet_std_os_pid()
}

fn jet_std_os_expand(template: &String) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            i += 1;
            if i < bytes.len() && bytes[i] == b'{' {
                i += 1;
                let start = i;
                while i < bytes.len() && bytes[i] != b'}' {
                    i += 1;
                }
                let name = &template[start..i];
                if i < bytes.len() {
                    i += 1;
                }
                out.push_str(&jet_std_env_get(&name.to_string()).unwrap_or_default());
            } else {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let name = &template[start..i];
                out.push_str(&jet_std_env_get(&name.to_string()).unwrap_or_default());
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

#[cfg(unix)]
#[repr(C)]
struct JetOsUtimbuf {
    actime: i64,
    modtime: i64,
}

#[cfg(unix)]
#[repr(C)]
struct JetOsTms {
    tms_utime: i64,
    tms_stime: i64,
    tms_cutime: i64,
    tms_cstime: i64,
}

#[cfg(unix)]
mod jet_os_sys {
    use super::{JetOsTms, JetOsUtimbuf};
    pub const _SC_CLK_TCK: i32 = 2;
    extern "C" {
        pub fn getppid() -> i32;
        pub fn getuid() -> u32;
        pub fn geteuid() -> u32;
        pub fn getgid() -> u32;
        pub fn getegid() -> u32;
        pub fn getgroups(size: i32, list: *mut u32) -> i32;
        pub fn getpgid(pid: i32) -> i32;
        pub fn getpgrp() -> i32;
        pub fn getsid(pid: i32) -> i32;
        pub fn fork() -> i32;
        pub fn setuid(uid: u32) -> i32;
        pub fn setgid(gid: u32) -> i32;
        pub fn setpgid(pid: i32, pgid: i32) -> i32;
        pub fn setpgrp() -> i32;
        pub fn setsid() -> i32;
        pub fn initgroups(user: *const i8, group: u32) -> i32;
        pub fn kill(pid: i32, sig: i32) -> i32;
        pub fn wait(status: *mut i32) -> i32;
        pub fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
        pub fn pipe(fds: *mut i32) -> i32;
        pub fn mkfifo(path: *const i8, mode: u32) -> i32;
        pub fn sync();
        pub fn umask(mask: u32) -> u32;
        pub fn getpriority(which: i32, who: u32) -> i32;
        pub fn setpriority(which: i32, who: u32, prio: i32) -> i32;
        pub fn getloadavg(loadavg: *mut f64, nelem: i32) -> i32;
        pub fn utime(path: *const i8, times: *const JetOsUtimbuf) -> i32;
        pub fn close(fd: i32) -> i32;
        pub fn times(buf: *mut JetOsTms) -> i64;
        pub fn sysconf(name: i32) -> i64;
        pub fn atexit(cb: extern "C" fn()) -> i32;
        #[cfg(target_os = "linux")]
        pub fn __errno_location() -> *mut i32;
        #[cfg(target_os = "macos")]
        pub fn __error() -> *mut i32;
    }
    pub unsafe fn errno_ptr() -> *mut i32 {
        #[cfg(target_os = "linux")]
        {
            __errno_location()
        }
        #[cfg(target_os = "macos")]
        {
            __error()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            static mut FALLBACK: i32 = 0;
            &raw mut FALLBACK
        }
    }
}

#[cfg(unix)]
fn jet_os_last_err(op: jet_std::IOOperation, path: Option<String>) -> jet_std::IOError {
    let errno = unsafe { *jet_os_sys::errno_ptr() };
    jet_std::IOError::other(op, path, std::io::Error::from_raw_os_error(errno))
}

fn jet_std_os_getppid() -> i64 {
    #[cfg(unix)]
    {
        return unsafe { jet_os_sys::getppid() as i64 };
    }
    #[cfg(not(unix))]
    {
        0
    }
}
fn jet_std_os_getuid() -> i64 {
    #[cfg(unix)]
    {
        return unsafe { jet_os_sys::getuid() as i64 };
    }
    #[cfg(not(unix))]
    {
        0
    }
}
fn jet_std_os_geteuid() -> i64 {
    #[cfg(unix)]
    {
        return unsafe { jet_os_sys::geteuid() as i64 };
    }
    #[cfg(not(unix))]
    {
        0
    }
}
fn jet_std_os_getgid() -> i64 {
    #[cfg(unix)]
    {
        return unsafe { jet_os_sys::getgid() as i64 };
    }
    #[cfg(not(unix))]
    {
        0
    }
}
fn jet_std_os_getegid() -> i64 {
    #[cfg(unix)]
    {
        return unsafe { jet_os_sys::getegid() as i64 };
    }
    #[cfg(not(unix))]
    {
        0
    }
}
fn jet_std_os_getgroups() -> Vec<i64> {
    #[cfg(unix)]
    {
        unsafe {
            let n = jet_os_sys::getgroups(0, std::ptr::null_mut());
            if n < 0 {
                return Vec::new();
            }
            let mut buf = vec![0u32; n as usize];
            let got = jet_os_sys::getgroups(n, buf.as_mut_ptr());
            if got < 0 {
                return Vec::new();
            }
            return buf.into_iter().take(got as usize).map(|g| g as i64).collect();
        }
    }
    #[cfg(not(unix))]
    {
        Vec::new()
    }
}
fn jet_std_os_getpgid(pid: i64) -> Result<i64, jet_std::IOError> {
    #[cfg(unix)]
    {
        let out = unsafe { jet_os_sys::getpgid(pid as i32) };
        if out < 0 {
            return Err(jet_os_last_err(jet_std::IOOperation::Resolve, None));
        }
        return Ok(out as i64);
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Err(jet_os_unsupported("getpgid"))
    }
}
fn jet_std_os_getpgrp() -> i64 {
    #[cfg(unix)]
    {
        return unsafe { jet_os_sys::getpgrp() as i64 };
    }
    #[cfg(not(unix))]
    {
        0
    }
}
fn jet_std_os_getsid(pid: i64) -> Result<i64, jet_std::IOError> {
    #[cfg(unix)]
    {
        let out = unsafe { jet_os_sys::getsid(pid as i32) };
        if out < 0 {
            return Err(jet_os_last_err(jet_std::IOOperation::Resolve, None));
        }
        return Ok(out as i64);
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Err(jet_os_unsupported("getsid"))
    }
}
fn jet_std_os_fork() -> Result<i64, jet_std::IOError> {
    #[cfg(unix)]
    {
        let pid = unsafe { jet_os_sys::fork() };
        if pid < 0 {
            return Err(jet_os_last_err(jet_std::IOOperation::Resolve, None));
        }
        return Ok(pid as i64);
    }
    #[cfg(not(unix))]
    {
        Err(jet_os_unsupported("fork"))
    }
}
fn jet_std_os_setuid(uid: i64) -> Result<(), jet_std::IOError> {
    #[cfg(unix)]
    {
        if unsafe { jet_os_sys::setuid(uid as u32) } == 0 {
            return Ok(());
        }
        return Err(jet_os_last_err(jet_std::IOOperation::Write, None));
    }
    #[cfg(not(unix))]
    {
        let _ = uid;
        Err(jet_os_unsupported("setuid"))
    }
}
fn jet_std_os_setgid(gid: i64) -> Result<(), jet_std::IOError> {
    #[cfg(unix)]
    {
        if unsafe { jet_os_sys::setgid(gid as u32) } == 0 {
            return Ok(());
        }
        return Err(jet_os_last_err(jet_std::IOOperation::Write, None));
    }
    #[cfg(not(unix))]
    {
        let _ = gid;
        Err(jet_os_unsupported("setgid"))
    }
}
fn jet_std_os_setpgid(pid: i64, pgid: i64) -> Result<(), jet_std::IOError> {
    #[cfg(unix)]
    {
        if unsafe { jet_os_sys::setpgid(pid as i32, pgid as i32) } == 0 {
            return Ok(());
        }
        return Err(jet_os_last_err(jet_std::IOOperation::Write, None));
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, pgid);
        Err(jet_os_unsupported("setpgid"))
    }
}
fn jet_std_os_setpgrp() -> Result<(), jet_std::IOError> {
    #[cfg(unix)]
    {
        if unsafe { jet_os_sys::setpgrp() } == 0 {
            return Ok(());
        }
        return Err(jet_os_last_err(jet_std::IOOperation::Write, None));
    }
    #[cfg(not(unix))]
    {
        Err(jet_os_unsupported("setpgrp"))
    }
}
fn jet_std_os_setsid() -> Result<i64, jet_std::IOError> {
    #[cfg(unix)]
    {
        let out = unsafe { jet_os_sys::setsid() };
        if out < 0 {
            return Err(jet_os_last_err(jet_std::IOOperation::Write, None));
        }
        return Ok(out as i64);
    }
    #[cfg(not(unix))]
    {
        Err(jet_os_unsupported("setsid"))
    }
}
fn jet_std_os_initgroups(user: &String, group: i64) -> Result<(), jet_std::IOError> {
    #[cfg(unix)]
    {
        let c_user = std::ffi::CString::new(user.as_str()).map_err(|_| {
            jet_std::IOError::other(
                jet_std::IOOperation::Write,
                None,
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "username contains NUL"),
            )
        })?;
        if unsafe { jet_os_sys::initgroups(c_user.as_ptr(), group as u32) } == 0 {
            return Ok(());
        }
        return Err(jet_os_last_err(jet_std::IOOperation::Write, None));
    }
    #[cfg(not(unix))]
    {
        let _ = (user, group);
        Err(jet_os_unsupported("initgroups"))
    }
}
fn jet_std_os_kill(pid: i64, sig: i64) -> Result<(), jet_std::IOError> {
    #[cfg(unix)]
    {
        if unsafe { jet_os_sys::kill(pid as i32, sig as i32) } == 0 {
            return Ok(());
        }
        return Err(jet_os_last_err(jet_std::IOOperation::Write, None));
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, sig);
        Err(jet_os_unsupported("kill"))
    }
}
fn jet_std_os_wait() -> Result<i64, jet_std::IOError> {
    #[cfg(unix)]
    {
        let mut status = 0i32;
        let got = unsafe { jet_os_sys::wait(&mut status) };
        if got < 0 {
            return Err(jet_os_last_err(jet_std::IOOperation::Close, None));
        }
        return Ok(status as i64);
    }
    #[cfg(not(unix))]
    {
        Err(jet_os_unsupported("wait"))
    }
}
fn jet_std_os_waitpid(pid: i64, options: i64) -> Result<i64, jet_std::IOError> {
    #[cfg(unix)]
    {
        let mut status = 0i32;
        let got = unsafe { jet_os_sys::waitpid(pid as i32, &mut status, options as i32) };
        if got < 0 {
            return Err(jet_os_last_err(jet_std::IOOperation::Close, None));
        }
        return Ok(status as i64);
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, options);
        Err(jet_os_unsupported("waitpid"))
    }
}
fn jet_std_os_pipe() -> Result<Vec<i64>, jet_std::IOError> {
    #[cfg(unix)]
    {
        let mut fds = [0i32; 2];
        if unsafe { jet_os_sys::pipe(fds.as_mut_ptr()) } == 0 {
            return Ok(vec![fds[0] as i64, fds[1] as i64]);
        }
        return Err(jet_os_last_err(jet_std::IOOperation::Resolve, None));
    }
    #[cfg(not(unix))]
    {
        Err(jet_os_unsupported("pipe"))
    }
}
fn jet_std_os_close_fd(fd: i64) {
    #[cfg(unix)]
    unsafe {
        let _ = jet_os_sys::close(fd as i32);
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
    }
}
fn jet_std_os_mkfifo(path: &String, mode: i64) -> Result<(), jet_std::IOError> {
    #[cfg(unix)]
    {
        let c_path = std::ffi::CString::new(path.as_str()).map_err(|_| {
            jet_std::IOError::other(
                jet_std::IOOperation::Resolve,
                Some(path.clone()),
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contains NUL"),
            )
        })?;
        if unsafe { jet_os_sys::mkfifo(c_path.as_ptr(), mode as u32) } == 0 {
            return Ok(());
        }
        return Err(jet_os_last_err(jet_std::IOOperation::Resolve, Some(path.clone())));
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Err(jet_os_unsupported("mkfifo"))
    }
}
fn jet_std_os_sync() {
    #[cfg(unix)]
    unsafe {
        jet_os_sys::sync();
    }
}
fn jet_std_os_umask(mask: i64) -> i64 {
    #[cfg(unix)]
    {
        return unsafe { jet_os_sys::umask(mask as u32) as i64 };
    }
    #[cfg(not(unix))]
    {
        let _ = mask;
        0
    }
}
fn jet_std_os_getpriority(who: i64) -> Result<i64, jet_std::IOError> {
    #[cfg(unix)]
    {
        unsafe {
            *jet_os_sys::errno_ptr() = 0;
            let got = jet_os_sys::getpriority(0, who as u32);
            if got == -1 && *jet_os_sys::errno_ptr() != 0 {
                return Err(jet_os_last_err(jet_std::IOOperation::Resolve, None));
            }
            return Ok(got as i64);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = who;
        Err(jet_os_unsupported("getpriority"))
    }
}
fn jet_std_os_setpriority(who: i64, prio: i64) -> Result<(), jet_std::IOError> {
    #[cfg(unix)]
    {
        if unsafe { jet_os_sys::setpriority(0, who as u32, prio as i32) } == 0 {
            return Ok(());
        }
        return Err(jet_os_last_err(jet_std::IOOperation::Write, None));
    }
    #[cfg(not(unix))]
    {
        let _ = (who, prio);
        Err(jet_os_unsupported("setpriority"))
    }
}
fn jet_std_os_loadavg() -> Vec<f64> {
    #[cfg(unix)]
    {
        let mut avg = [0.0f64; 3];
        let n = unsafe { jet_os_sys::getloadavg(avg.as_mut_ptr(), 3) };
        if n < 0 {
            return vec![0.0, 0.0, 0.0];
        }
        return avg[..n as usize].to_vec();
    }
    #[cfg(not(unix))]
    {
        vec![0.0, 0.0, 0.0]
    }
}
fn jet_std_os_utime(path: &String, atime: i64, mtime: i64) -> Result<(), jet_std::IOError> {
    #[cfg(unix)]
    {
        let c_path = std::ffi::CString::new(path.as_str()).map_err(|_| {
            jet_std::IOError::other(
                jet_std::IOOperation::Write,
                Some(path.clone()),
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contains NUL"),
            )
        })?;
        let times = JetOsUtimbuf {
            actime: atime,
            modtime: mtime,
        };
        if unsafe { jet_os_sys::utime(c_path.as_ptr(), &times) } == 0 {
            return Ok(());
        }
        return Err(jet_os_last_err(jet_std::IOOperation::Write, Some(path.clone())));
    }
    #[cfg(not(unix))]
    {
        let _ = (path, atime, mtime);
        Err(jet_os_unsupported("utime"))
    }
}
fn jet_std_os_success(status: i64) -> bool {
    #[cfg(unix)]
    {
        let s = status as i32;
        return (s & 0x7f) == 0 && ((s >> 8) & 0xff) == 0;
    }
    #[cfg(not(unix))]
    {
        status == 0
    }
}
fn jet_std_os_exitcode(status: i64) -> i64 {
    #[cfg(unix)]
    {
        let s = status as i32;
        if (s & 0x7f) == 0 {
            return ((s >> 8) & 0xff) as i64;
        }
        return -1;
    }
    #[cfg(not(unix))]
    {
        status
    }
}
fn jet_std_os_times() -> Vec<f64> {
    #[cfg(unix)]
    {
        let mut t = unsafe { std::mem::zeroed::<JetOsTms>() };
        let elapsed = unsafe { jet_os_sys::times(&mut t) };
        let ticks = unsafe { jet_os_sys::sysconf(jet_os_sys::_SC_CLK_TCK) }.max(1) as f64;
        return vec![
            t.tms_utime as f64 / ticks,
            t.tms_stime as f64 / ticks,
            t.tms_cutime as f64 / ticks,
            t.tms_cstime as f64 / ticks,
            if elapsed < 0 {
                0.0
            } else {
                elapsed as f64 / ticks
            },
        ];
    }
    #[cfg(not(unix))]
    {
        vec![0.0, 0.0, 0.0, 0.0, 0.0]
    }
}
fn jet_std_os_uptime() -> f64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(text) = std::fs::read_to_string("/proc/uptime") {
            if let Some(first) = text.split_whitespace().next() {
                if let Ok(v) = first.parse::<f64>() {
                    return v;
                }
            }
        }
    }
    0.0
}
fn jet_std_os_stop(code: i64) {
    jet_std_process_exit(code);
}

mod jet_os_atexit {
    use std::sync::{Mutex, OnceLock};
    static HANDLERS: OnceLock<Mutex<Vec<Box<dyn Fn() + Send + 'static>>>> = OnceLock::new();
    static INSTALLED: OnceLock<()> = OnceLock::new();
    fn handlers() -> &'static Mutex<Vec<Box<dyn Fn() + Send + 'static>>> {
        HANDLERS.get_or_init(|| Mutex::new(Vec::new()))
    }
    extern "C" fn run() {
        if let Ok(guard) = handlers().lock() {
            for handler in guard.iter() {
                handler();
            }
        }
    }
    pub fn register<F>(handler: F)
    where
        F: Fn() + Send + 'static,
    {
        INSTALLED.get_or_init(|| {
            #[cfg(unix)]
            unsafe {
                let _ = super::jet_os_sys::atexit(run);
            }
        });
        handlers()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Box::new(handler));
    }
}

fn jet_std_os_atexit<F>(handler: F)
where
    F: Fn() + Send + 'static,
{
    jet_os_atexit::register(handler);
}
