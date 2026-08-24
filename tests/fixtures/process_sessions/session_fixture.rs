use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::process::{self, Command};
use std::thread;
use std::time::Duration;

fn main() {
    let mut args = env::args();
    let _program = args.next();
    match args.next().as_deref() {
        Some("terminal") => terminal(),
        Some("tree") => tree(args.next().expect("tree needs a pid file")),
        Some("descendant") => descendant(args.next().expect("descendant needs a pid file")),
        Some("output") => output(args.next().as_deref().unwrap_or("large")),
        _ => process::exit(2),
    }
}

fn output(size: &str) {
    let bytes = if size == "small" {
        b"ok\n".as_slice()
    } else {
        b"0123456789abcdef0123456789abcdef\n".as_slice()
    };
    io::stdout().write_all(bytes).expect("write process output");
}

fn terminal() {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        process::exit(41);
    }

    let mut stdout = io::stdout().lock();
    writeln!(stdout, "ready").expect("write terminal readiness");
    stdout.flush().expect("flush terminal readiness");

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("read terminal input");
    let input = input.trim_end_matches(['\r', '\n']);
    writeln!(stdout, "input:{input}").expect("write terminal input");
    stdout
        .write_all(b"\x1b[31mcontrol\x1b[0m\n")
        .expect("write terminal control bytes");
    let (rows, cols) = terminal_size().expect("read terminal size");
    writeln!(stdout, "size:{rows}x{cols}").expect("write terminal size");
}

fn tree(pid_file: String) {
    let executable = env::current_exe().expect("find process fixture");
    Command::new(executable)
        .args(["descendant", &pid_file])
        .spawn()
        .expect("spawn descendant");
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

fn descendant(pid_file: String) {
    fs::write(pid_file, process::id().to_string()).expect("write descendant pid");
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(unix)]
fn terminal_size() -> io::Result<(u16, u16)> {
    use std::ffi::c_void;
    use std::os::fd::AsRawFd;

    #[repr(C)]
    struct WinSize {
        rows: u16,
        cols: u16,
        xpixel: u16,
        ypixel: u16,
    }

    #[cfg(target_os = "linux")]
    const TIOCGWINSZ: u64 = 0x5413;
    #[cfg(not(target_os = "linux"))]
    const TIOCGWINSZ: u64 = 0x4008_7468;

    unsafe extern "C" {
        fn ioctl(fd: i32, request: u64, arg: *mut c_void) -> i32;
    }

    let stdout = io::stdout();
    let mut size = WinSize {
        rows: 0,
        cols: 0,
        xpixel: 0,
        ypixel: 0,
    };
    // SAFETY: stdout stays open, and ioctl writes exactly one kernel-sized
    // winsize value to the initialized local buffer.
    if unsafe {
        ioctl(
            stdout.as_raw_fd(),
            TIOCGWINSZ,
            (&mut size as *mut WinSize).cast::<c_void>(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok((size.rows, size.cols))
}

#[cfg(windows)]
fn terminal_size() -> io::Result<(u16, u16)> {
    use std::ffi::c_void;

    #[repr(C)]
    struct Coord {
        x: i16,
        y: i16,
    }
    #[repr(C)]
    struct SmallRect {
        left: i16,
        top: i16,
        right: i16,
        bottom: i16,
    }
    #[repr(C)]
    struct ConsoleScreenBufferInfo {
        size: Coord,
        cursor: Coord,
        attributes: u16,
        window: SmallRect,
        maximum: Coord,
    }

    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    unsafe extern "system" {
        fn GetConsoleScreenBufferInfo(
            handle: *mut c_void,
            info: *mut ConsoleScreenBufferInfo,
        ) -> i32;
        fn GetStdHandle(handle: u32) -> *mut c_void;
    }

    // SAFETY: the process owns its standard output handle, and Windows fills
    // one initialized screen-buffer record.
    let handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    let mut info = ConsoleScreenBufferInfo {
        size: Coord { x: 0, y: 0 },
        cursor: Coord { x: 0, y: 0 },
        attributes: 0,
        window: SmallRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        maximum: Coord { x: 0, y: 0 },
    };
    if unsafe { GetConsoleScreenBufferInfo(handle, &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let cols = (i32::from(info.window.right) - i32::from(info.window.left) + 1)
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "invalid console width"))?;
    let rows = (i32::from(info.window.bottom) - i32::from(info.window.top) + 1)
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "invalid console height"))?;
    Ok((rows, cols))
}
