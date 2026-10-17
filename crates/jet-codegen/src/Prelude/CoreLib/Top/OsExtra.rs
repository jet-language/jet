// core.sys identity, facts, and POSIX control (ledger #1465 / D-OSFACTS1).
// Included after FSIoEnvOsTesting.rs at crate root (same level as jet_std).

pub(crate) fn jet_std_os_name() -> String {
    std::env::consts::OS.to_string()
}
pub(crate) fn jet_std_os_arch() -> String {
    std::env::consts::ARCH.to_string()
}
pub(crate) fn jet_sys_cpu_count_raw() -> Option<i64> {
    std::thread::available_parallelism().ok().map(|n| n.get() as i64)
}
pub(crate) fn jet_sys_executable_raw() -> Option<String> {
    std::env::current_exe().ok().map(|p| p.to_string_lossy().into_owned())
}
pub(crate) fn jet_std_os_pid() -> i64 {
    std::process::id() as i64
}
pub(crate) fn jet_sys_host_env_raw(name: &str) -> Option<Vec<u8>> {
    std::env::var_os(name).map(std::ffi::OsString::into_encoded_bytes)
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
        pub fn initgroups(user: *const std::ffi::c_char, group: u32) -> i32;
        pub fn kill(pid: i32, sig: i32) -> i32;
        pub fn wait(status: *mut i32) -> i32;
        pub fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
        pub fn pipe(fds: *mut i32) -> i32;
        pub fn mkfifo(path: *const std::ffi::c_char, mode: u32) -> i32;
        pub fn sync();
        pub fn umask(mask: u32) -> u32;
        pub fn getpriority(which: i32, who: u32) -> i32;
        pub fn setpriority(which: i32, who: u32, prio: i32) -> i32;
        pub fn getloadavg(loadavg: *mut f64, nelem: i32) -> i32;
        pub fn utime(path: *const std::ffi::c_char, times: *const JetOsUtimbuf) -> i32;
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


pub(crate) fn jet_sys_getppid_raw() -> Option<i64> {
    #[cfg(unix)]
    { Some(unsafe { jet_os_sys::getppid() as i64 }) }
    #[cfg(not(unix))]
    { None }
}
pub(crate) fn jet_sys_getuid_raw() -> Option<i64> {
    #[cfg(unix)]
    { Some(unsafe { jet_os_sys::getuid() as i64 }) }
    #[cfg(not(unix))]
    { None }
}
pub(crate) fn jet_sys_geteuid_raw() -> Option<i64> {
    #[cfg(unix)]
    { Some(unsafe { jet_os_sys::geteuid() as i64 }) }
    #[cfg(not(unix))]
    { None }
}
pub(crate) fn jet_sys_getgid_raw() -> Option<i64> {
    #[cfg(unix)]
    { Some(unsafe { jet_os_sys::getgid() as i64 }) }
    #[cfg(not(unix))]
    { None }
}
pub(crate) fn jet_sys_getegid_raw() -> Option<i64> {
    #[cfg(unix)]
    { Some(unsafe { jet_os_sys::getegid() as i64 }) }
    #[cfg(not(unix))]
    { None }
}
pub(crate) fn jet_sys_groups_count_raw() -> Option<i64> {
    #[cfg(unix)]
    { Some(unsafe { jet_os_sys::getgroups(0, std::ptr::null_mut()) as i64 }) }
    #[cfg(not(unix))]
    { None }
}

// Capacity has already been checked against the syscall's nonnegative i32
// range by Jet. Return the count and full buffer without interpreting failure.
pub(crate) fn jet_sys_groups_fill_raw(capacity: i64) -> Vec<jet_foundation::Numeric::JetInt> {
    #[cfg(unix)]
    {
        let mut buf = vec![0u32; capacity as usize];
        let count = unsafe { jet_os_sys::getgroups(capacity as i32, buf.as_mut_ptr()) };
        std::iter::once(jet_std::jet_int_owned_from_i64(count as i64))
            .chain(buf.into_iter().map(|value| jet_std::jet_int_owned_from_i64(value as i64))).collect()
    }
    #[cfg(not(unix))]
    { let _ = capacity; Vec::new() }
}
pub(crate) fn jet_sys_getpgid_raw(pid: i64) -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    { unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::getpgid(pid as i32); jet_sys_call_sample(value as i64) } }
    #[cfg(not(unix))]
    { let _ = pid; jet_sys_unavailable_sample() }
}
pub(crate) fn jet_sys_getpgrp_raw() -> Option<i64> {
    #[cfg(unix)]
    { Some(unsafe { jet_os_sys::getpgrp() as i64 }) }
    #[cfg(not(unix))]
    { None }
}
pub(crate) fn jet_sys_getsid_raw(pid: i64) -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    { unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::getsid(pid as i32); jet_sys_call_sample(value as i64) } }
    #[cfg(not(unix))]
    { let _ = pid; jet_sys_unavailable_sample() }
}
pub(crate) fn jet_sys_fork_raw() -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    { unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::fork(); jet_sys_call_sample(value as i64) } }
    #[cfg(not(unix))]
    { jet_sys_unavailable_sample() }
}
pub(crate) fn jet_sys_setuid_raw(uid: i64) -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    { unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::setuid(uid as u32); jet_sys_call_sample(value as i64) } }
    #[cfg(not(unix))]
    { let _ = uid; jet_sys_unavailable_sample() }
}
pub(crate) fn jet_sys_setgid_raw(gid: i64) -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    { unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::setgid(gid as u32); jet_sys_call_sample(value as i64) } }
    #[cfg(not(unix))]
    { let _ = gid; jet_sys_unavailable_sample() }
}
pub(crate) fn jet_sys_setpgid_raw(pid: i64, pgid: i64) -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    { unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::setpgid(pid as i32, pgid as i32); jet_sys_call_sample(value as i64) } }
    #[cfg(not(unix))]
    { let _ = (pid, pgid); jet_sys_unavailable_sample() }
}
pub(crate) fn jet_sys_setpgrp_raw() -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    { unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::setpgrp(); jet_sys_call_sample(value as i64) } }
    #[cfg(not(unix))]
    { jet_sys_unavailable_sample() }
}
pub(crate) fn jet_sys_setsid_raw() -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    { unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::setsid(); jet_sys_call_sample(value as i64) } }
    #[cfg(not(unix))]
    { jet_sys_unavailable_sample() }
}
pub(crate) fn jet_sys_initgroups_raw(user: &str, group: i64) -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    {
        // Jet checked NUL absence immediately before this private boundary.
        let c_user = unsafe { std::ffi::CString::from_vec_unchecked(user.as_bytes().to_vec()) };
        unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::initgroups(c_user.as_ptr(), group as u32); jet_sys_call_sample(value as i64) }
    }
    #[cfg(not(unix))]
    { let _ = (user, group); jet_sys_unavailable_sample() }
}
pub(crate) fn jet_sys_kill_raw(pid: i64, sig: i64) -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    { unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::kill(pid as i32, sig as i32); jet_sys_call_sample(value as i64) } }
    #[cfg(not(unix))]
    { let _ = (pid, sig); jet_sys_unavailable_sample() }
}
pub(crate) fn jet_sys_wait_raw() -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    {
        let mut status = 0;
        unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::wait(&mut status);
            let (available, value, errno) = jet_sys_call_sample(value as i64);
            (available, value, errno, jet_std::jet_int_owned_from_i64(status as i64)) }
    }
    #[cfg(not(unix))]
    { let (available, value, errno) = jet_sys_unavailable_sample(); (available, value, errno, jet_std::jet_int_owned_from_i64(0)) }
}
pub(crate) fn jet_sys_waitpid_raw(pid: i64, options: i64) -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    {
        let mut status = 0;
        unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::waitpid(pid as i32, &mut status, options as i32);
            let (available, value, errno) = jet_sys_call_sample(value as i64);
            (available, value, errno, jet_std::jet_int_owned_from_i64(status as i64)) }
    }
    #[cfg(not(unix))]
    { let _ = (pid, options); let (available, value, errno) = jet_sys_unavailable_sample(); (available, value, errno, jet_std::jet_int_owned_from_i64(0)) }
}
pub(crate) fn jet_sys_pipe_raw() -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    {
        let mut fds = [0i32; 2];
        unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::pipe(fds.as_mut_ptr());
            let (available, value, errno) = jet_sys_call_sample(value as i64);
            (available, value, errno, jet_std::jet_int_owned_from_i64(fds[0] as i64), jet_std::jet_int_owned_from_i64(fds[1] as i64)) }
    }
    #[cfg(not(unix))]
    { let (available, value, errno) = jet_sys_unavailable_sample(); (available, value, errno, jet_std::jet_int_owned_from_i64(0), jet_std::jet_int_owned_from_i64(0)) }
}
pub(crate) fn jet_std_os_close_fd(fd: i64) {
    #[cfg(unix)]
    unsafe {
        let _ = jet_os_sys::close(fd as i32);
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
    }
}
pub(crate) fn jet_sys_mkfifo_raw(path: &str, mode: i64) -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    {
        let c_path = unsafe { std::ffi::CString::from_vec_unchecked(path.as_bytes().to_vec()) };
        unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::mkfifo(c_path.as_ptr(), mode as u32); jet_sys_call_sample(value as i64) }
    }
    #[cfg(not(unix))]
    { let _ = (path, mode); jet_sys_unavailable_sample() }
}
pub(crate) fn jet_std_os_sync() {
    #[cfg(unix)]
    unsafe {
        jet_os_sys::sync();
    }
}
pub(crate) fn jet_sys_umask_raw(mask: i64) -> Option<i64> {
    #[cfg(unix)]
    { Some(unsafe { jet_os_sys::umask(mask as u32) as i64 }) }
    #[cfg(not(unix))]
    { let _ = mask; None }
}
pub(crate) fn jet_sys_getpriority_raw(who: i64) -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    { unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::getpriority(0, who as u32); jet_sys_call_sample(value as i64) } }
    #[cfg(not(unix))]
    { let _ = who; jet_sys_unavailable_sample() }
}
pub(crate) fn jet_sys_setpriority_raw(who: i64, priority: i64) -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    { unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::setpriority(0, who as u32, priority as i32); jet_sys_call_sample(value as i64) } }
    #[cfg(not(unix))]
    { let _ = (who, priority); jet_sys_unavailable_sample() }
}
pub(crate) fn jet_sys_loadavg_raw() -> (bool, jet_foundation::Numeric::JetInt, f64, f64, f64) {
    #[cfg(unix)]
    {
        let mut avg = [0.0; 3];
        let count = unsafe { jet_os_sys::getloadavg(avg.as_mut_ptr(), 3) };
        (true, jet_std::jet_int_owned_from_i64(count as i64), avg[0], avg[1], avg[2])
    }
    #[cfg(not(unix))]
    { (false, jet_std::jet_int_owned_from_i64(0), 0.0, 0.0, 0.0) }
}
pub(crate) fn jet_sys_utime_raw(path: &str, atime: i64, mtime: i64) -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    {
        let c_path = unsafe { std::ffi::CString::from_vec_unchecked(path.as_bytes().to_vec()) };
        let times = JetOsUtimbuf { actime: atime, modtime: mtime };
        unsafe { *jet_os_sys::errno_ptr() = 0; let value = jet_os_sys::utime(c_path.as_ptr(), &times); jet_sys_call_sample(value as i64) }
    }
    #[cfg(not(unix))]
    { let _ = (path, atime, mtime); jet_sys_unavailable_sample() }
}
pub(crate) fn jet_sys_times_raw() -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(unix)]
    {
        let mut t = JetOsTms { tms_utime: 0, tms_stime: 0, tms_cutime: 0, tms_cstime: 0 };
        let elapsed = unsafe { jet_os_sys::times(&mut t) };
        let ticks = unsafe { jet_os_sys::sysconf(jet_os_sys::_SC_CLK_TCK) };
        (true, jet_std::jet_int_owned_from_i64(t.tms_utime), jet_std::jet_int_owned_from_i64(t.tms_stime),
         jet_std::jet_int_owned_from_i64(t.tms_cutime), jet_std::jet_int_owned_from_i64(t.tms_cstime),
         jet_std::jet_int_owned_from_i64(elapsed), jet_std::jet_int_owned_from_i64(ticks))
    }
    #[cfg(not(unix))]
    { (false, jet_std::jet_int_owned_from_i64(0), jet_std::jet_int_owned_from_i64(0),
       jet_std::jet_int_owned_from_i64(0), jet_std::jet_int_owned_from_i64(0),
       jet_std::jet_int_owned_from_i64(0), jet_std::jet_int_owned_from_i64(0)) }
}

#[cfg(unix)]
fn jet_sys_call_sample(value: i64) -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    let errno = unsafe { *jet_os_sys::errno_ptr() };
    (true, jet_std::jet_int_owned_from_i64(value), jet_std::jet_int_owned_from_i64(errno as i64))
}
#[cfg(not(unix))]
fn jet_sys_unavailable_sample() -> (bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    (false, jet_std::jet_int_owned_from_i64(0), jet_std::jet_int_owned_from_i64(0))
}
pub(crate) fn jet_sys_errno_text_raw(errno: i64) -> String {
    std::io::Error::from_raw_os_error(errno as i32).to_string()
}
pub(crate) fn jet_sys_current_dir_raw() -> Result<String, String> {
    std::env::current_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| error.to_string())
}

pub(crate) fn jet_sys_set_current_dir_raw(path: &str) -> Result<(), String> {
    std::env::set_current_dir(path).map_err(|error| error.to_string())
}
