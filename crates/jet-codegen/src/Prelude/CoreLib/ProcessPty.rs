//! Unix PTY support shared by the emitted Core process prelude and the JIT host.
//!
//! The module deliberately uses the small POSIX interface directly. Jet's
//! compiler seams do not take a new native dependency for one OS primitive.

use std::fs::File;
use std::io;
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtyConfig {
    pub cols: i64,
    pub rows: i64,
    pub raw: bool,
}

pub struct PtyPair {
    pub master: File,
    pub slave: File,
}

#[cfg(unix)]
mod unix {
    use super::{File, PtyConfig, PtyPair};
    use std::ffi::{CStr, OsStr};
    use std::io;
    use std::os::raw::{c_char, c_void};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::process::Command;

    #[repr(C)]
    struct WinSize {
        rows: u16,
        cols: u16,
        xpixel: u16,
        ypixel: u16,
    }

    // Linux and the BSD/Darwin family use the same termios fields used by the
    // existing `core.term` prelude. Keep this local so the PTY backend remains
    // independent of the terminal-input backend.
    #[repr(C)]
    struct Termios {
        c_iflag: u32,
        c_oflag: u32,
        c_cflag: u32,
        c_lflag: u32,
        #[cfg(target_os = "linux")]
        c_line: u8,
        c_cc: [u8; 32],
        #[cfg(target_os = "linux")]
        c_ispeed: u32,
        #[cfg(target_os = "linux")]
        c_ospeed: u32,
        #[cfg(not(target_os = "linux"))]
        _pad: [u8; 12],
    }

    #[cfg(target_os = "linux")]
    const PTY_OPEN_FLAGS: i32 = 0x0002 | 0x0100 | 0x80000; // O_RDWR|O_NOCTTY|O_CLOEXEC
    #[cfg(not(target_os = "linux"))]
    const PTY_OPEN_FLAGS: i32 = 0x0002 | 0x20000 | 0x01000000; // Darwin/BSD values

    #[cfg(target_os = "linux")]
    const TIOCSCTTY: u64 = 0x540e;
    #[cfg(not(target_os = "linux"))]
    const TIOCSCTTY: u64 = 0x20007461;

    #[cfg(target_os = "linux")]
    const TIOCSWINSZ: u64 = 0x5414;
    #[cfg(not(target_os = "linux"))]
    const TIOCSWINSZ: u64 = 0x80087467;

    const TCSANOW: i32 = 0;
    const VTIME: usize = 5;
    const VMIN: usize = 6;
    const EIO: i32 = 5;

    #[link(name = "c")]
    extern "C" {
        fn posix_openpt(flags: i32) -> i32;
        fn grantpt(fd: i32) -> i32;
        fn unlockpt(fd: i32) -> i32;
        fn ptsname(fd: i32) -> *mut c_char;
        fn ioctl(fd: i32, request: u64, arg: *const c_void) -> i32;
        fn tcgetattr(fd: i32, termios: *mut Termios) -> i32;
        fn tcsetattr(fd: i32, actions: i32, termios: *const Termios) -> i32;
        fn cfmakeraw(termios: *mut Termios);
        fn setsid() -> i32;
        fn kill(pid: i32, signal: i32) -> i32;
    }

    fn invalid(message: impl Into<String>) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidInput, message.into())
    }

    fn last_os_error(operation: &str) -> io::Error {
        let error = io::Error::last_os_error();
        io::Error::new(error.kind(), format!("{operation}: {error}"))
    }

    fn ioctl_error(operation: &str) -> io::Error {
        last_os_error(operation)
    }

    fn validate_size(config: PtyConfig) -> io::Result<(u16, u16)> {
        if !(1..=u16::MAX as i64).contains(&config.cols)
            || !(1..=u16::MAX as i64).contains(&config.rows)
        {
            return Err(invalid(format!(
                "terminal size must be between 1 and {}, got {}x{}",
                u16::MAX,
                config.cols,
                config.rows
            )));
        }
        Ok((config.cols as u16, config.rows as u16))
    }

    pub(super) fn open(config: PtyConfig) -> io::Result<PtyPair> {
        let (cols, rows) = validate_size(config)?;
        // SAFETY: `posix_openpt` returns a new owned descriptor or -1. The
        // successful descriptor is immediately wrapped by `File` exactly once.
        let master_fd = unsafe { posix_openpt(PTY_OPEN_FLAGS) };
        if master_fd < 0 {
            return Err(last_os_error("posix_openpt"));
        }
        // SAFETY: `master_fd` is the one descriptor returned above and is now
        // owned by this File. All error paths drop it.
        let master = unsafe { File::from_raw_fd(master_fd) };
        // SAFETY: the calls operate on the valid PTY master descriptor.
        if unsafe { grantpt(master_fd) } != 0 {
            return Err(last_os_error("grantpt"));
        }
        // SAFETY: see `grantpt` above.
        if unsafe { unlockpt(master_fd) } != 0 {
            return Err(last_os_error("unlockpt"));
        }
        // SAFETY: `ptsname` returns a pointer owned by the C library and valid
        // until the next PTY-name operation in this process. Copy the bytes
        // before opening the slave.
        let name = unsafe { ptsname(master_fd) };
        if name.is_null() {
            return Err(last_os_error("ptsname"));
        }
        let slave_path = unsafe { CStr::from_ptr(name) }.to_bytes().to_vec();
        let slave = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(OsStr::from_bytes(&slave_path))?;
        configure(&slave, cols, rows, config.raw)?;
        Ok(PtyPair { master, slave })
    }

    fn configure(slave: &File, cols: u16, rows: u16, raw: bool) -> io::Result<()> {
        let size = WinSize {
            rows,
            cols,
            xpixel: 0,
            ypixel: 0,
        };
        // SAFETY: `slave` is a live PTY descriptor and `size` is a valid
        // kernel-sized winsize object for TIOCSWINSZ.
        if unsafe {
            ioctl(
                slave.as_raw_fd(),
                TIOCSWINSZ,
                (&size as *const WinSize).cast::<c_void>(),
            )
        } != 0
        {
            return Err(ioctl_error("TIOCSWINSZ"));
        }
        // SAFETY: `termios` is zeroed storage for the platform ABI and the
        // kernel fills it through the valid PTY slave descriptor.
        let mut termios = unsafe { std::mem::zeroed::<Termios>() };
        if unsafe { tcgetattr(slave.as_raw_fd(), &mut termios) } != 0 {
            return Err(last_os_error("tcgetattr"));
        }
        if raw {
            // SAFETY: `termios` was initialized by tcgetattr and remains a
            // valid mutable termios object.
            unsafe { cfmakeraw(&mut termios) };
            termios.c_cc[VMIN] = 1;
            termios.c_cc[VTIME] = 0;
        }
        // SAFETY: the pointer references the initialized local termios value.
        if unsafe { tcsetattr(slave.as_raw_fd(), TCSANOW, &termios) } != 0 {
            return Err(last_os_error("tcsetattr"));
        }
        Ok(())
    }

    pub(super) fn attach_command(command: &mut Command) -> io::Result<()> {
        use std::os::unix::process::CommandExt;
        // SAFETY: `pre_exec` is the standard Rust boundary for the small set of
        // async-signal-safe session calls required between fork and exec. The
        // closure captures nothing and allocates nothing.
        unsafe {
            command.pre_exec(|| {
                if setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                if ioctl(0, TIOCSCTTY, std::ptr::null()) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Ok(())
    }

    pub(super) fn resize(master: &File, config: PtyConfig) -> io::Result<()> {
        let (cols, rows) = validate_size(config)?;
        let size = WinSize {
            rows,
            cols,
            xpixel: 0,
            ypixel: 0,
        };
        // SAFETY: `master` is a live PTY master and `size` is a valid winsize.
        if unsafe {
            ioctl(
                master.as_raw_fd(),
                TIOCSWINSZ,
                (&size as *const WinSize).cast::<c_void>(),
            )
        } != 0
        {
            return Err(ioctl_error("TIOCSWINSZ"));
        }
        Ok(())
    }

    pub(super) fn signal_group(pid: u32, signal: i32) -> io::Result<()> {
        let pid = i32::try_from(pid).map_err(|_| invalid("process id is out of range"))?;
        // A negative pid targets the PTY session's process group. This keeps a
        // shell and its descendants under one kill/terminate/interrupt action.
        // SAFETY: `kill` receives a valid negative process-group id and signal.
        if unsafe { kill(-pid, signal) } != 0 {
            return Err(last_os_error("kill process group"));
        }
        Ok(())
    }

    pub(super) fn is_terminal_eof(error: &io::Error) -> bool {
        error.raw_os_error() == Some(EIO)
    }

    pub(super) fn supported() -> bool {
        true
    }

    pub(super) const SIGINT: i32 = 2;
    pub(super) const SIGTERM: i32 = 15;
    pub(super) const SIGKILL: i32 = 9;
}

#[cfg(unix)]
pub fn supported() -> bool {
    unix::supported()
}

#[cfg(not(unix))]
pub fn supported() -> bool {
    false
}

pub fn open(config: PtyConfig) -> io::Result<PtyPair> {
    #[cfg(unix)]
    {
        return unix::open(config);
    }
    #[cfg(not(unix))]
    {
        let _ = config;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Unix PTY backend is unavailable on this target",
        ))
    }
}

pub fn attach_command(command: &mut Command) -> io::Result<()> {
    #[cfg(unix)]
    {
        return unix::attach_command(command);
    }
    #[cfg(not(unix))]
    {
        let _ = command;
        Ok(())
    }
}

pub fn resize(master: &File, config: PtyConfig) -> io::Result<()> {
    #[cfg(unix)]
    {
        return unix::resize(master, config);
    }
    #[cfg(not(unix))]
    {
        let _ = (master, config);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Unix PTY backend is unavailable on this target",
        ))
    }
}

pub fn signal_group(pid: u32, signal: i32) -> io::Result<()> {
    #[cfg(unix)]
    {
        return unix::signal_group(pid, signal);
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, signal);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "process groups are unavailable on this target",
        ))
    }
}

pub fn is_terminal_eof(error: &io::Error) -> bool {
    #[cfg(unix)]
    {
        return unix::is_terminal_eof(error);
    }
    #[cfg(not(unix))]
    {
        let _ = error;
        false
    }
}

#[cfg(unix)]
pub const SIGINT: i32 = unix::SIGINT;
#[cfg(unix)]
pub const SIGTERM: i32 = unix::SIGTERM;
#[cfg(unix)]
pub const SIGKILL: i32 = unix::SIGKILL;

#[cfg(not(unix))]
pub const SIGINT: i32 = 2;
#[cfg(not(unix))]
pub const SIGTERM: i32 = 15;
#[cfg(not(unix))]
pub const SIGKILL: i32 = 9;
