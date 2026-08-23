//! Native PTY/ConPTY support shared by the emitted Core process prelude and the
//! JIT host.
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

impl Default for PtyConfig {
    // Card #1751: reads the one 80x24 terminal default instead of hand-typing
    // it, the same fact CommonTypes.rs's `TerminalPolicy::default` reads.
    fn default() -> Self {
        Self {
            cols: super::terminal_default::JET_TERMINAL_DEFAULT_COLS,
            rows: super::terminal_default::JET_TERMINAL_DEFAULT_ROWS,
            raw: false,
        }
    }
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
    use std::sync::Mutex;

    // `ptsname` returns libc-owned static storage. Serialize only that lookup
    // and copy the name before another PTY can replace it.
    static PTY_NAME_LOCK: Mutex<()> = Mutex::new(());

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
    const ESRCH: i32 = 3;

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
        fn fcntl(fd: i32, command: i32, ...) -> i32;
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
        let slave_path = {
            let _name_guard = PTY_NAME_LOCK.lock().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "PTY name lock poisoned")
            })?;
            let name = unsafe { ptsname(master_fd) };
            if name.is_null() {
                return Err(last_os_error("ptsname"));
            }
            unsafe { CStr::from_ptr(name) }.to_bytes().to_vec()
        };
        // The slave is an extra descriptor in the parent while Command builds
        // the child stdio. Keep that one close-on-exec so descendants cannot
        // keep the PTY open after the intended child exits.
        mark_close_on_exec(&master)?;
        let slave = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(OsStr::from_bytes(&slave_path))?;
        mark_close_on_exec(&slave)?;
        configure(&slave, cols, rows, config.raw)?;
        Ok(PtyPair { master, slave })
    }

    fn mark_close_on_exec(file: &File) -> io::Result<()> {
        const F_SETFD: i32 = 2;
        const FD_CLOEXEC: i32 = 1;
        // SAFETY: `file` owns a live descriptor and fcntl only updates its
        // descriptor flags.
        if unsafe { fcntl(file.as_raw_fd(), F_SETFD, FD_CLOEXEC) } != 0 {
            return Err(last_os_error("fcntl(FD_CLOEXEC)"));
        }
        Ok(())
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

    pub(super) fn attach_process_group(command: &mut Command) -> io::Result<()> {
        use std::os::unix::process::CommandExt;
        // SAFETY: `pre_exec` only installs a session boundary; `setsid` is
        // async-signal-safe and the closure captures nothing.
        unsafe {
            command.pre_exec(|| {
                if setsid() < 0 {
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
            // The child may have exited after wait observed it and before the
            // final group sweep. ESRCH means the requested tree is already
            // gone, so cleanup has succeeded.
            if io::Error::last_os_error().raw_os_error() == Some(ESRCH) {
                return Ok(());
            }
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

#[cfg(windows)]
mod windows {
    use super::{File, PtyConfig};
    use std::ffi::c_void;
    use std::io::{self, Write};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};

    type Handle = RawHandle;

    const CREATE_SUSPENDED: u32 = 0x0000_0004;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
    const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
    const PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE: usize = 0x0002_0016;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: u32 = 9;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
    const CTRL_BREAK_EVENT: u32 = 1;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 258;
    const WAIT_FAILED: u32 = 0xffff_ffff;
    const INFINITE: u32 = 0xffff_ffff;
    const STILL_ACTIVE: u32 = 259;
    const INVALID_THREAD_RESUME: u32 = 0xffff_ffff;
    const HANDLE_FLAG_INHERIT: u32 = 1;

    #[repr(C)]
    struct Coord {
        x: i16,
        y: i16,
    }

    #[repr(C)]
    struct SecurityAttributes {
        length: u32,
        descriptor: *mut c_void,
        inherit: i32,
    }

    #[repr(C)]
    struct StartupInfoW {
        cb: u32,
        reserved: *mut u16,
        desktop: *mut u16,
        title: *mut u16,
        x: u32,
        y: u32,
        x_size: u32,
        y_size: u32,
        x_count_chars: u32,
        y_count_chars: u32,
        fill_attribute: u32,
        flags: u32,
        show_window: u16,
        reserved2: u16,
        reserved2_ptr: *mut u8,
        std_input: Handle,
        std_output: Handle,
        std_error: Handle,
    }

    #[repr(C)]
    struct StartupInfoExW {
        startup_info: StartupInfoW,
        attribute_list: *mut c_void,
    }

    #[repr(C)]
    struct ProcessInformation {
        process: Handle,
        thread: Handle,
        process_id: u32,
        thread_id: u32,
    }

    #[repr(C)]
    struct BasicLimitInformation {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    struct IoCounters {
        read_operations: u64,
        write_operations: u64,
        other_operations: u64,
        read_bytes: u64,
        write_bytes: u64,
        other_bytes: u64,
    }

    #[repr(C)]
    struct ExtendedLimitInformation {
        basic: BasicLimitInformation,
        io: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
        fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> Handle;
        fn CreatePipe(
            read_pipe: *mut Handle,
            write_pipe: *mut Handle,
            attributes: *mut SecurityAttributes,
            size: u32,
        ) -> i32;
        fn CreateProcessW(
            application_name: *const u16,
            command_line: *mut u16,
            process_attributes: *mut c_void,
            thread_attributes: *mut c_void,
            inherit_handles: i32,
            creation_flags: u32,
            environment: *mut c_void,
            current_directory: *const u16,
            startup_info: *mut StartupInfoW,
            process_information: *mut ProcessInformation,
        ) -> i32;
        fn CreatePseudoConsole(
            size: Coord,
            input: Handle,
            output: Handle,
            flags: u32,
            console: *mut Handle,
        ) -> i32;
        fn DeleteProcThreadAttributeList(attribute_list: *mut c_void);
        fn GenerateConsoleCtrlEvent(event: u32, process_group_id: u32) -> i32;
        fn GetExitCodeProcess(process: Handle, exit_code: *mut u32) -> i32;
        fn InitializeProcThreadAttributeList(
            attribute_list: *mut c_void,
            attribute_count: u32,
            flags: u32,
            size: *mut usize,
        ) -> i32;
        fn ResizePseudoConsole(console: Handle, size: Coord) -> i32;
        fn ResumeThread(thread: Handle) -> u32;
        fn SetHandleInformation(handle: Handle, mask: u32, flags: u32) -> i32;
        fn SetInformationJobObject(
            job: Handle,
            information_class: u32,
            information: *mut c_void,
            length: u32,
        ) -> i32;
        fn TerminateJobObject(job: Handle, exit_code: u32) -> i32;
        fn TerminateProcess(process: Handle, exit_code: u32) -> i32;
        fn UpdateProcThreadAttribute(
            attribute_list: *mut c_void,
            flags: u32,
            attribute: usize,
            value: *const c_void,
            size: usize,
            previous_value: *mut c_void,
            return_size: *mut usize,
        ) -> i32;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        fn ClosePseudoConsole(console: Handle);
    }

    fn null_handle() -> Handle {
        std::ptr::null_mut()
    }

    fn invalid_handle(handle: Handle) -> bool {
        handle.is_null() || handle == (-1isize as Handle)
    }

    fn error(operation: &str) -> io::Error {
        let cause = io::Error::last_os_error();
        io::Error::new(cause.kind(), format!("{operation}: {cause}"))
    }

    fn hresult_error(operation: &str, status: i32) -> io::Error {
        io::Error::new(
            io::ErrorKind::Other,
            format!("{operation} failed with HRESULT 0x{:08x}", status as u32),
        )
    }

    fn validate_size(config: PtyConfig) -> io::Result<Coord> {
        if !(1..=i16::MAX as i64).contains(&config.cols)
            || !(1..=i16::MAX as i64).contains(&config.rows)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "terminal size must be between 1 and {}, got {}x{}",
                    i16::MAX,
                    config.cols,
                    config.rows
                ),
            ));
        }
        Ok(Coord {
            x: config.cols as i16,
            y: config.rows as i16,
        })
    }

    struct HandleGuard(Handle);

    impl HandleGuard {
        fn new(handle: Handle) -> io::Result<Self> {
            if invalid_handle(handle) {
                Err(error("invalid Windows handle"))
            } else {
                Ok(Self(handle))
            }
        }

        fn raw(&self) -> Handle {
            self.0
        }

        fn into_file(mut self) -> File {
            let handle = self.0;
            self.0 = null_handle();
            // SAFETY: ownership moves from this guard to exactly one File.
            unsafe { File::from_raw_handle(handle) }
        }
    }

    impl Drop for HandleGuard {
        fn drop(&mut self) {
            if !invalid_handle(self.0) {
                // SAFETY: this guard owns the live handle and closes it once.
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    struct ConsoleGuard(Handle);

    impl ConsoleGuard {
        fn create(config: PtyConfig, input: Handle, output: Handle) -> io::Result<Self> {
            let size = validate_size(config)?;
            let mut console = null_handle();
            // SAFETY: pipe handles and output storage remain live through the
            // documented CreatePseudoConsole call.
            let status = unsafe { CreatePseudoConsole(size, input, output, 0, &mut console) };
            if status != 0 {
                return Err(hresult_error("CreatePseudoConsole", status));
            }
            if invalid_handle(console) {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "CreatePseudoConsole returned no console handle",
                ));
            }
            Ok(Self(console))
        }

        fn raw(&self) -> Handle {
            self.0
        }

        fn into_raw(mut self) -> usize {
            let handle = self.0 as usize;
            self.0 = null_handle();
            handle
        }
    }

    impl Drop for ConsoleGuard {
        fn drop(&mut self) {
            if !invalid_handle(self.0) {
                // SAFETY: this guard owns the live HPCON and closes it once.
                unsafe { ClosePseudoConsole(self.0) };
            }
        }
    }

    struct AttributeList {
        list: *mut c_void,
        storage: Vec<u8>,
    }

    impl AttributeList {
        fn create() -> io::Result<Self> {
            let mut size = 0;
            // SAFETY: documented size probe; Windows writes only `size`.
            unsafe {
                InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size);
            }
            if size == 0 {
                return Err(error("InitializeProcThreadAttributeList size probe"));
            }
            let mut storage = vec![0_u8; size];
            let list = storage.as_mut_ptr().cast::<c_void>();
            // SAFETY: storage has the exact probed size and remains live.
            if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut size) } == 0 {
                return Err(error("InitializeProcThreadAttributeList"));
            }
            Ok(Self { list, storage })
        }

        fn update(&mut self, attribute: usize, value: *const c_void, size: usize) -> io::Result<()> {
            // SAFETY: `value` points to a live attribute value for this call.
            if unsafe {
                UpdateProcThreadAttribute(
                    self.list,
                    0,
                    attribute,
                    value,
                    size,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(error("UpdateProcThreadAttribute"));
            }
            Ok(())
        }
    }

    impl Drop for AttributeList {
        fn drop(&mut self) {
            // SAFETY: the list was initialized and storage remains live.
            unsafe { DeleteProcThreadAttributeList(self.list) };
            let _ = &self.storage;
        }
    }

    struct Job(HandleGuard);

    impl Job {
        fn create() -> io::Result<Self> {
            // SAFETY: null attributes/name request one private unnamed job.
            let handle = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
            let guard = HandleGuard::new(handle)?;
            let mut limits = ExtendedLimitInformation {
                basic: BasicLimitInformation {
                    per_process_user_time_limit: 0,
                    per_job_user_time_limit: 0,
                    limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                    minimum_working_set_size: 0,
                    maximum_working_set_size: 0,
                    active_process_limit: 0,
                    affinity: 0,
                    priority_class: 0,
                    scheduling_class: 0,
                },
                io: IoCounters {
                    read_operations: 0,
                    write_operations: 0,
                    other_operations: 0,
                    read_bytes: 0,
                    write_bytes: 0,
                    other_bytes: 0,
                },
                process_memory_limit: 0,
                job_memory_limit: 0,
                peak_process_memory_used: 0,
                peak_job_memory_used: 0,
            };
            // SAFETY: structure and size match the documented information class.
            if unsafe {
                SetInformationJobObject(
                    guard.raw(),
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                    (&mut limits as *mut ExtendedLimitInformation).cast(),
                    std::mem::size_of::<ExtendedLimitInformation>() as u32,
                )
            } == 0
            {
                return Err(error("SetInformationJobObject"));
            }
            Ok(Self(guard))
        }

        fn assign(&self, process: Handle) -> io::Result<()> {
            // SAFETY: both handles remain live through the call.
            if unsafe { AssignProcessToJobObject(self.0.raw(), process) } == 0 {
                return Err(error("AssignProcessToJobObject"));
            }
            Ok(())
        }

        fn into_file(self) -> File {
            self.0.into_file()
        }
    }

    fn create_pipe() -> io::Result<(HandleGuard, HandleGuard)> {
        let mut read = null_handle();
        let mut write = null_handle();
        let mut attributes = SecurityAttributes {
            length: std::mem::size_of::<SecurityAttributes>() as u32,
            descriptor: std::ptr::null_mut(),
            inherit: 1,
        };
        // SAFETY: output handles and security attributes are valid for the call.
        if unsafe { CreatePipe(&mut read, &mut write, &mut attributes, 0) } == 0 {
            return Err(error("CreatePipe"));
        }
        let read = HandleGuard::new(read)?;
        let write = match HandleGuard::new(write) {
            Ok(write) => write,
            Err(error) => return Err(error),
        };
        Ok((read, write))
    }

    fn make_wide(value: &str) -> io::Result<Vec<u16>> {
        if value.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Windows process value contains NUL",
            ));
        }
        Ok(value.encode_utf16().chain(std::iter::once(0)).collect())
    }

    fn quote_arg(value: &str) -> io::Result<Vec<u16>> {
        if value.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Windows process argument contains NUL",
            ));
        }
        let quoted = value.is_empty() || value.chars().any(char::is_whitespace) || value.contains('"');
        let mut output = String::new();
        if quoted {
            output.push('"');
        }
        let mut slashes = 0;
        for character in value.chars() {
            if character == '\\' {
                slashes += 1;
                continue;
            }
            if character == '"' {
                output.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
            } else {
                output.extend(std::iter::repeat_n('\\', slashes));
            }
            output.push(character);
            slashes = 0;
        }
        if quoted {
            output.extend(std::iter::repeat_n('\\', slashes * 2));
            output.push('"');
        } else {
            output.extend(std::iter::repeat_n('\\', slashes));
        }
        make_wide(&output)
    }

    fn command_line(executable: &str, args: &[String]) -> io::Result<Vec<u16>> {
        let mut output = quote_arg(executable)?;
        output.pop();
        for arg in args {
            output.push(' ' as u16);
            let mut quoted = quote_arg(arg)?;
            quoted.pop();
            output.extend(quoted);
        }
        output.push(0);
        Ok(output)
    }

    fn environment_block(env: &[(std::ffi::OsString, std::ffi::OsString)]) -> Vec<u16> {
        let mut block = Vec::new();
        for (name, value) in env {
            block.extend(name.encode_wide());
            block.push('=' as u16);
            block.extend(value.encode_wide());
            block.push(0);
        }
        if block.is_empty() {
            block.push(0);
        }
        block.push(0);
        block
    }

    pub struct WindowsPtyProcess {
        pub process: File,
        pub job: File,
        pub pid: u32,
        pub input: File,
        pub output: File,
        pub console: usize,
    }

    pub fn spawn(
        config: PtyConfig,
        executable: &str,
        args: &[String],
        cwd: Option<&str>,
        env: &[(std::ffi::OsString, std::ffi::OsString)],
    ) -> io::Result<WindowsPtyProcess> {
        let (input_read, input_write) = create_pipe()?;
        let (output_read, output_write) = create_pipe()?;
        // The parent ends never belong in a child handle table.
        if unsafe { SetHandleInformation(input_write.raw(), HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(error("SetHandleInformation(input)"));
        }
        if unsafe { SetHandleInformation(output_read.raw(), HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(error("SetHandleInformation(output)"));
        }
        // ConPTY owns the two opposite pipe ends after creation. `raw` is
        // consumed by the child console's own mode; the pipe remains binary.
        let console = ConsoleGuard::create(config, input_read.raw(), output_write.raw())?;
        drop(input_read);
        drop(output_write);

        let job = Job::create()?;
        let application = make_wide(executable)?;
        let mut command = command_line(executable, args)?;
        let current_directory = cwd.map(make_wide).transpose()?;
        let mut environment = environment_block(env);
        let mut attributes = AttributeList::create()?;
        let console_handle = console.raw();
        attributes.update(
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
            (&console_handle as *const Handle).cast(),
            std::mem::size_of::<Handle>(),
        )?;
        let mut startup = StartupInfoExW {
            startup_info: StartupInfoW {
                cb: std::mem::size_of::<StartupInfoExW>() as u32,
                reserved: std::ptr::null_mut(),
                desktop: std::ptr::null_mut(),
                title: std::ptr::null_mut(),
                x: 0,
                y: 0,
                x_size: 0,
                y_size: 0,
                x_count_chars: 0,
                y_count_chars: 0,
                fill_attribute: 0,
                flags: 0,
                show_window: 0,
                reserved2: 0,
                reserved2_ptr: std::ptr::null_mut(),
                std_input: null_handle(),
                std_output: null_handle(),
                std_error: null_handle(),
            },
            attribute_list: attributes.list,
        };
        let mut information = ProcessInformation {
            process: null_handle(),
            thread: null_handle(),
            process_id: 0,
            thread_id: 0,
        };
        // SAFETY: every pointer stays live until CreateProcessW returns; the
        // child is suspended until its Job Object boundary is installed.
        if unsafe {
            CreateProcessW(
                application.as_ptr(),
                command.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                EXTENDED_STARTUPINFO_PRESENT
                    | CREATE_SUSPENDED
                    | CREATE_NEW_PROCESS_GROUP
                    | CREATE_UNICODE_ENVIRONMENT,
                environment.as_mut_ptr().cast(),
                current_directory
                    .as_ref()
                    .map_or(std::ptr::null(), Vec::as_ptr),
                &mut startup.startup_info,
                &mut information,
            )
        } == 0
        {
            return Err(error("CreateProcessW(ConPTY)"));
        }
        let process = HandleGuard::new(information.process)?;
        let thread = match HandleGuard::new(information.thread) {
            Ok(thread) => thread,
            Err(error) => {
                // SAFETY: the process handle is live and the suspended child is
                // terminated before the guard closes it.
                unsafe {
                    TerminateProcess(process.raw(), 1);
                    WaitForSingleObject(process.raw(), INFINITE);
                }
                return Err(error);
            }
        };
        if let Err(error) = job.assign(process.raw()) {
            // SAFETY: the suspended process is still owned by `process`.
            unsafe {
                TerminateProcess(process.raw(), 1);
                WaitForSingleObject(process.raw(), INFINITE);
            }
            return Err(error);
        }
        // SAFETY: the primary thread is live and suspended by CreateProcessW.
        if unsafe { ResumeThread(thread.raw()) } == INVALID_THREAD_RESUME {
            unsafe {
                TerminateProcess(process.raw(), 1);
                WaitForSingleObject(process.raw(), INFINITE);
            }
            return Err(error("ResumeThread(ConPTY)"));
        }

        Ok(WindowsPtyProcess {
            process: process.into_file(),
            job: job.into_file(),
            pid: information.process_id,
            input: input_write.into_file(),
            output: output_read.into_file(),
            console: console.into_raw(),
        })
    }

    pub fn attach_job(process: RawHandle) -> io::Result<File> {
        let job = Job::create()?;
        job.assign(process)?;
        Ok(job.into_file())
    }

    pub fn try_wait(process: &File) -> io::Result<Option<u32>> {
        // SAFETY: the process handle is owned by the caller and remains live.
        let result = unsafe { WaitForSingleObject(process.as_raw_handle(), 0) };
        match result {
            WAIT_OBJECT_0 => exit_code(process).map(Some),
            WAIT_TIMEOUT => Ok(None),
            WAIT_FAILED => Err(error("WaitForSingleObject")),
            other => Err(io::Error::new(
                io::ErrorKind::Other,
                format!("WaitForSingleObject returned {other}"),
            )),
        }
    }

    pub fn wait(process: &File) -> io::Result<u32> {
        // SAFETY: the process handle is owned by the caller and remains live.
        if unsafe { WaitForSingleObject(process.as_raw_handle(), INFINITE) } != WAIT_OBJECT_0 {
            return Err(error("WaitForSingleObject"));
        }
        exit_code(process)
    }

    fn exit_code(process: &File) -> io::Result<u32> {
        let mut code = STILL_ACTIVE;
        // SAFETY: Windows writes one exit-code value to live storage.
        if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
            return Err(error("GetExitCodeProcess"));
        }
        Ok(code)
    }

    pub fn terminate(job: &File) -> io::Result<()> {
        // SAFETY: the caller owns a live Job Object handle.
        if unsafe { TerminateJobObject(job.as_raw_handle(), 1) } == 0 {
            return Err(error("TerminateJobObject"));
        }
        Ok(())
    }

    pub fn interrupt(pid: u32, job: &File, input: Option<&File>) -> io::Result<()> {
        // A ConPTY child is its own console process group. CTRL_BREAK reaches
        // the group without stealing the parent's console input.
        // SAFETY: the group id is the live ConPTY child PID.
        if unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) } != 0 {
            return Ok(());
        }
        if let Some(input) = input {
            let mut input = input.try_clone()?;
            input.write_all(&[3])?;
            return Ok(());
        }
        terminate(job)
    }

    pub fn resize(console: usize, config: PtyConfig) -> io::Result<()> {
        let size = validate_size(config)?;
        // SAFETY: the HPCON is owned by TerminalSession and remains live.
        let status = unsafe { ResizePseudoConsole(console as Handle, size) };
        if status != 0 {
            return Err(hresult_error("ResizePseudoConsole", status));
        }
        Ok(())
    }

    pub fn is_terminal_eof(error: &io::Error) -> bool {
        matches!(
            error.kind(),
            io::ErrorKind::BrokenPipe | io::ErrorKind::UnexpectedEof
        )
    }

    pub fn supported() -> bool {
        true
    }
}

#[cfg(unix)]
pub fn supported() -> bool {
    unix::supported()
}

#[cfg(windows)]
pub use windows::WindowsPtyProcess;

#[cfg(windows)]
pub fn supported() -> bool {
    windows::supported()
}

#[cfg(not(any(unix, windows)))]
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

pub fn attach_process_group(command: &mut Command) -> io::Result<()> {
    #[cfg(unix)]
    {
        return unix::attach_process_group(command);
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

#[cfg(windows)]
pub fn spawn(
    config: PtyConfig,
    executable: &str,
    args: &[String],
    cwd: Option<&str>,
    env: &[(std::ffi::OsString, std::ffi::OsString)],
) -> io::Result<WindowsPtyProcess> {
    windows::spawn(config, executable, args, cwd, env)
}

#[cfg(windows)]
pub fn attach_job(process: std::os::windows::io::RawHandle) -> io::Result<File> {
    windows::attach_job(process)
}

#[cfg(windows)]
pub fn wait(process: &File) -> io::Result<u32> {
    windows::wait(process)
}

#[cfg(windows)]
pub fn try_wait(process: &File) -> io::Result<Option<u32>> {
    windows::try_wait(process)
}

#[cfg(windows)]
pub fn terminate(job: &File) -> io::Result<()> {
    windows::terminate(job)
}

#[cfg(windows)]
pub fn interrupt(pid: u32, job: &File, input: Option<&File>) -> io::Result<()> {
    windows::interrupt(pid, job, input)
}

#[cfg(windows)]
pub fn resize_console(console: usize, config: PtyConfig) -> io::Result<()> {
    windows::resize(console, config)
}

pub fn signal_group(pid: u32, signal: i32) -> io::Result<()> {
    #[cfg(unix)]
    {
        return unix::signal_group(pid, signal);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pid, signal);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "process groups are unavailable on this target",
        ))
    }
    #[cfg(windows)]
    {
        let _ = (pid, signal);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "POSIX process groups are unavailable on Windows",
        ))
    }
}

pub fn is_terminal_eof(error: &io::Error) -> bool {
    #[cfg(unix)]
    {
        return unix::is_terminal_eof(error);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = error;
        false
    }
    #[cfg(windows)]
    {
        windows::is_terminal_eof(error)
    }
}

#[cfg(unix)]
pub const SIGINT: i32 = unix::SIGINT;
#[cfg(unix)]
pub const SIGTERM: i32 = unix::SIGTERM;
#[cfg(unix)]
pub const SIGKILL: i32 = unix::SIGKILL;

#[cfg(windows)]
pub const SIGINT: i32 = 2;
#[cfg(windows)]
pub const SIGTERM: i32 = 15;
#[cfg(windows)]
pub const SIGKILL: i32 = 9;

#[cfg(not(any(unix, windows)))]
pub const SIGINT: i32 = 2;
#[cfg(not(any(unix, windows)))]
pub const SIGTERM: i32 = 15;
#[cfg(not(any(unix, windows)))]
pub const SIGKILL: i32 = 9;
