//! `core.io` stdout/stderr/stdin + terminal hosts (#1219). Writes go to the
//! resident `JitRuntime` capture buffers so ProgramOutput matches AOT under
//! the process harness (real stdio would bypass capture).

use super::Concurrency;
use super::CoreHost::{jit_env_key_eq, jit_env_snapshot_raw};
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::io::{BufRead, IsTerminal, Write};

fn clone_str(id: i64) -> String {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id).unwrap_or_default())
}

fn result_ok_unit() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(super::JitResultValue { ok: true, bits: 0 });
        rt.results.len() as i64
    })
}

fn result_ok_bits(bits: u64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(super::JitResultValue { ok: true, bits });
        rt.results.len() as i64
    })
}

fn result_err(msg: &str) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let sid = rt.heap.alloc_string(msg.to_string());
        rt.results.push(super::JitResultValue {
            ok: false,
            bits: sid as u64,
        });
        rt.results.len() as i64
    })
}

fn list_from_lines(lines: Vec<String>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for line in lines {
            let sid = rt.heap.alloc_string(line);
            let _ = rt.heap.list_push_int(list, sid);
        }
        list
    })
}

fn env_value(name: &str) -> Option<std::ffi::OsString> {
    let name = std::ffi::OsStr::new(name);
    jit_env_snapshot_raw()
        .into_iter()
        .find(|(candidate, _)| jit_env_key_eq(candidate.as_os_str(), name))
        .map(|(_, value)| value)
}

fn env_int(name: &str) -> Option<i64> {
    env_value(name)?
        .to_str()?
        .parse::<i64>()
        .ok()
        .filter(|n| *n > 0)
}

fn style_code(name: &str) -> Option<&'static str> {
    match name {
        "black" => Some("30"),
        "red" => Some("31"),
        "green" => Some("32"),
        "yellow" => Some("33"),
        "blue" => Some("34"),
        "magenta" => Some("35"),
        "cyan" => Some("36"),
        "white" => Some("37"),
        "bold" => Some("1"),
        "dim" => Some("2"),
        _ => None,
    }
}

fn style_enabled() -> bool {
    env_value("NO_COLOR").is_none()
        && env_value("TERM")
            .and_then(|term| term.into_string().ok())
            .map(|term| term != "dumb")
            .unwrap_or(true)
        && std::io::stdout().is_terminal()
}

fn write_prompt(prompt: &str) -> Result<(), String> {
    print!("{prompt}");
    std::io::stdout()
        .flush()
        .map_err(|error| format!("flush stdout: {error}"))
}

fn read_line() -> Result<String, String> {
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|error| format!("read stdin: {error}"))?;
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    Ok(line)
}

pub(crate) fn prompt_confirm(prompt: &str) -> bool {
    let prompt = format!("{prompt} [y/N] ");
    let answer = write_prompt(&prompt)
        .and_then(|_| read_line())
        .unwrap_or_default();
    matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    )
}

pub(crate) fn prompt_choose(prompt: &str, values: &[String]) -> Result<String, String> {
    if values.is_empty() {
        return Err("choose needs at least one item".to_string());
    }
    println!("{prompt}");
    for (index, item) in values.iter().enumerate() {
        println!("  {}) {item}", index + 1);
    }
    loop {
        let answer = write_prompt("> ").and_then(|_| read_line())?;
        if let Ok(index) = answer.trim().parse::<usize>() {
            if let Some(item) = index.checked_sub(1).and_then(|index| values.get(index)) {
                return Ok(item.clone());
            }
        }
        println!("Enter a number from 1 to {}.", values.len());
    }
}

pub(crate) fn prompt_input_secret(prompt: &str) -> Result<String, String> {
    if !std::io::stdin().is_terminal() {
        return Err("secret input needs a terminal".to_string());
    }
    write_prompt(prompt)?;
    if !terminal_mode::enter(false) {
        println!();
        return Err("could not disable terminal echo".to_string());
    }
    let guard = TerminalModeGuard;
    let secret = read_line();
    drop(guard);
    println!();
    secret
}

#[cfg(unix)]
mod terminal_mode {
    const TCSANOW: i32 = 0;
    const ECHO: u32 = 0o0000010;
    const ICANON: u32 = 0o0000002;
    const VMIN: usize = 6;
    const VTIME: usize = 5;

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

    unsafe extern "C" {
        fn tcgetattr(fd: i32, termios: *mut Termios) -> i32;
        fn tcsetattr(fd: i32, optional_actions: i32, termios: *const Termios) -> i32;
    }

    std::thread_local! {
        static SAVED: std::cell::RefCell<Vec<Termios>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    pub fn enter(raw: bool) -> bool {
        unsafe {
            let mut mode = std::mem::zeroed::<Termios>();
            if tcgetattr(0, &mut mode) != 0 {
                return false;
            }
            let saved_mode = std::mem::transmute_copy(&mode);
            mode.c_lflag &= !ECHO;
            if raw {
                mode.c_lflag &= !ICANON;
                mode.c_cc[VMIN] = 1;
                mode.c_cc[VTIME] = 0;
            }
            if tcsetattr(0, TCSANOW, &mode) != 0 {
                return false;
            }
            SAVED.with(|saved| saved.borrow_mut().push(saved_mode));
            true
        }
    }

    pub fn leave() {
        unsafe {
            SAVED.with(|saved| {
                if let Some(mode) = saved.borrow_mut().pop() {
                    tcsetattr(0, TCSANOW, &mode);
                }
            });
        }
    }
}

#[cfg(windows)]
mod terminal_mode {
    unsafe extern "system" {
        fn GetStdHandle(nStdHandle: u32) -> *mut std::ffi::c_void;
        fn GetConsoleMode(hConsoleHandle: *mut std::ffi::c_void, lpMode: *mut u32) -> i32;
        fn SetConsoleMode(hConsoleHandle: *mut std::ffi::c_void, dwMode: u32) -> i32;
    }

    const STD_INPUT_HANDLE: u32 = 0xFFFFFFF6u32;
    const ENABLE_ECHO_INPUT: u32 = 0x0004;
    const ENABLE_LINE_INPUT: u32 = 0x0002;

    std::thread_local! {
        static SAVED: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    pub fn enter(raw: bool) -> bool {
        unsafe {
            let handle = GetStdHandle(STD_INPUT_HANDLE);
            let mut mode = 0;
            if GetConsoleMode(handle, &mut mode) == 0 {
                return false;
            }
            let mut next = mode & !ENABLE_ECHO_INPUT;
            if raw {
                next &= !ENABLE_LINE_INPUT;
            }
            if SetConsoleMode(handle, next) == 0 {
                return false;
            }
            SAVED.with(|saved| saved.borrow_mut().push(mode));
            true
        }
    }

    pub fn leave() {
        unsafe {
            let handle = GetStdHandle(STD_INPUT_HANDLE);
            SAVED.with(|saved| {
                if let Some(mode) = saved.borrow_mut().pop() {
                    SetConsoleMode(handle, mode);
                }
            });
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod terminal_mode {
    pub fn enter(_raw: bool) -> bool {
        false
    }
    pub fn leave() {}
}

struct TerminalModeGuard;
impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        terminal_mode::leave();
    }
}

extern "C" fn jet_jit_io_stdout() -> i64 {
    1
}

extern "C" fn jet_jit_io_stderr() -> i64 {
    2
}

extern "C" fn jet_jit_io_stdin() -> i64 {
    3
}

extern "C" fn jet_jit_stdout_write(_h: i64, text: i64) -> i64 {
    let s = clone_str(text);
    Concurrency::with_runtime_mut(|rt| rt.stdout.push_str(&s));
    result_ok_unit()
}

extern "C" fn jet_jit_stdout_write_line(_h: i64, text: i64) -> i64 {
    let s = clone_str(text);
    Concurrency::with_runtime_mut(|rt| {
        rt.stdout.push_str(&s);
        rt.stdout.push('\n');
    });
    result_ok_unit()
}

extern "C" fn jet_jit_stdout_write_bytes(_h: i64, list: i64) -> i64 {
    let bytes = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(list, i).unwrap_or(0) as u8);
        }
        out
    });
    Concurrency::with_runtime_mut(|rt| {
        rt.stdout.push_str(&String::from_utf8_lossy(&bytes));
    });
    result_ok_unit()
}

extern "C" fn jet_jit_stdout_flush(_h: i64) -> i64 {
    result_ok_unit()
}

extern "C" fn jet_jit_stdout_is_tty(_h: i64) -> i8 {
    i8::from(std::io::stdout().is_terminal())
}

extern "C" fn jet_jit_stderr_write(_h: i64, text: i64) -> i64 {
    let s = clone_str(text);
    Concurrency::with_runtime_mut(|rt| rt.stderr.push_str(&s));
    result_ok_unit()
}

extern "C" fn jet_jit_stderr_write_line(_h: i64, text: i64) -> i64 {
    let s = clone_str(text);
    Concurrency::with_runtime_mut(|rt| {
        rt.stderr.push_str(&s);
        rt.stderr.push('\n');
    });
    result_ok_unit()
}

extern "C" fn jet_jit_stderr_write_bytes(_h: i64, list: i64) -> i64 {
    let bytes = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(list, i).unwrap_or(0) as u8);
        }
        out
    });
    Concurrency::with_runtime_mut(|rt| {
        rt.stderr.push_str(&String::from_utf8_lossy(&bytes));
    });
    result_ok_unit()
}

extern "C" fn jet_jit_stderr_flush(_h: i64) -> i64 {
    result_ok_unit()
}

extern "C" fn jet_jit_stderr_is_tty(_h: i64) -> i8 {
    i8::from(std::io::stderr().is_terminal())
}

extern "C" fn jet_jit_terminal_width() -> i64 {
    env_int("COLUMNS").unwrap_or(80)
}

extern "C" fn jet_jit_terminal_height() -> i64 {
    env_int("LINES").unwrap_or(24)
}

extern "C" fn jet_jit_io_style(style: i64, text: i64) -> i64 {
    let style = clone_str(style);
    let text = clone_str(text);
    let out = if style_enabled() {
        match style_code(style.as_str()) {
            Some(code) => format!("\x1b[{code}m{text}\x1b[0m"),
            None => text,
        }
    } else {
        text
    };
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(out))
}

extern "C" fn jet_jit_io_style_force(style: i64, text: i64) -> i64 {
    let style = clone_str(style);
    let text = clone_str(text);
    let out = match style_code(style.as_str()) {
        Some(code) => format!("\x1b[{code}m{text}\x1b[0m"),
        None => text,
    };
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(out))
}

extern "C" fn jet_jit_io_progress(text: i64) -> i64 {
    let s = clone_str(text);
    Concurrency::with_runtime_mut(|rt| {
        rt.stdout.push_str(&s);
        rt.stdout.push('\n');
    });
    result_ok_unit()
}

extern "C" fn jet_jit_io_confirm(prompt: i64) -> i8 {
    i8::from(prompt_confirm(&clone_str(prompt)))
}

extern "C" fn jet_jit_io_choose(prompt: i64, items: i64) -> i64 {
    let prompt = clone_str(prompt);
    let values = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(items).unwrap_or(0);
        (0..len)
            .map(|index| {
                rt.heap
                    .list_get_int(items, index)
                    .and_then(|id| rt.heap.clone_string(id))
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
    });
    match prompt_choose(&prompt, &values) {
        Ok(item) => {
            let id = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(item));
            result_ok_bits(id as u64)
        }
        Err(error) => result_err(&error),
    }
}

extern "C" fn jet_jit_io_input_secret(prompt: i64) -> i64 {
    match prompt_input_secret(&clone_str(prompt)) {
        Ok(secret) => {
            let id = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(secret));
            result_ok_bits(id as u64)
        }
        Err(error) => result_err(&error),
    }
}

/// Materialize stdin lines into a string list (for-in walk).
extern "C" fn jet_jit_stdin_lines(_h: i64) -> i64 {
    let mut lines = Vec::new();
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        match line {
            Ok(l) => lines.push(l),
            Err(_) => break,
        }
    }
    list_from_lines(lines)
}

/// Materialize FileReader lines into a string list without consuming the handle.
extern "C" fn jet_jit_file_lines(handle: i64) -> i64 {
    use super::enc_stream::FileReaderSlot;
    let lines = Concurrency::with_runtime_mut(|rt| {
        let idx = handle.saturating_sub(1) as usize;
        let Some(FileReaderSlot::Live(reader)) = rt.file_readers.get_mut(idx) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut buf = String::new();
        loop {
            buf.clear();
            match reader.inner.read_line(&mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    if buf.ends_with('\n') {
                        buf.pop();
                        if buf.ends_with('\r') {
                            buf.pop();
                        }
                    }
                    out.push(std::mem::take(&mut buf));
                }
                Err(_) => break,
            }
        }
        out
    });
    list_from_lines(lines)
}

extern "C" fn jet_jit_file_writer_write_line(handle: i64, line: i64) -> i64 {
    use super::enc_stream::FileWriterSlot;
    use std::io::Write;
    let text = clone_str(line);
    let err = Concurrency::with_runtime_mut(|rt| {
        let idx = handle.saturating_sub(1) as usize;
        let Some(FileWriterSlot::Live(writer)) = rt.file_writers.get_mut(idx) else {
            return Some("bad FileWriter".to_string());
        };
        writer
            .inner
            .write_all(text.as_bytes())
            .and_then(|_| writer.inner.write_all(b"\n"))
            .and_then(|_| writer.inner.flush())
            .err()
            .map(|e| format!("write {}: {e}", writer.path))
    });
    match err {
        None => result_ok_unit(),
        Some(msg) => result_err(&msg),
    }
}

extern "C" fn jet_jit_file_writer_flush(handle: i64) -> i64 {
    use super::enc_stream::FileWriterSlot;
    let err = Concurrency::with_runtime_mut(|rt| {
        let idx = handle.saturating_sub(1) as usize;
        let Some(FileWriterSlot::Live(writer)) = rt.file_writers.get_mut(idx) else {
            return Some("bad FileWriter".to_string());
        };
        writer
            .inner
            .flush()
            .err()
            .map(|e| format!("flush {}: {e}", writer.path))
    });
    match err {
        None => result_ok_unit(),
        Some(msg) => result_err(&msg),
    }
}

extern "C" fn jet_jit_term_enter() {
    let _ = terminal_mode::enter(true);
}

extern "C" fn jet_jit_term_leave() {
    terminal_mode::leave();
}

pub(crate) struct IOHostFns {
    pub stdout: FuncId,
    pub stderr: FuncId,
    pub stdin: FuncId,
    pub stdout_write: FuncId,
    pub stdout_write_line: FuncId,
    pub stdout_write_bytes: FuncId,
    pub stdout_flush: FuncId,
    pub stdout_is_tty: FuncId,
    pub stderr_write: FuncId,
    pub stderr_write_line: FuncId,
    pub stderr_write_bytes: FuncId,
    pub stderr_flush: FuncId,
    pub stderr_is_tty: FuncId,
    pub terminal_width: FuncId,
    pub terminal_height: FuncId,
    pub style: FuncId,
    pub style_force: FuncId,
    pub progress: FuncId,
    pub confirm: FuncId,
    pub choose: FuncId,
    pub input_secret: FuncId,
    pub stdin_lines: FuncId,
    pub file_lines: FuncId,
    pub file_writer_write_line: FuncId,
    pub file_writer_flush: FuncId,
    pub file_writer_close: FuncId,
    pub file_reader_close: FuncId,
    pub term_enter: FuncId,
    pub term_leave: FuncId,
}

pub(crate) fn register_io_symbols(builder: &mut JITBuilder) {
    builder.symbol("jet_jit_io_stdout", jet_jit_io_stdout as *const u8);
    builder.symbol("jet_jit_io_stderr", jet_jit_io_stderr as *const u8);
    builder.symbol("jet_jit_io_stdin", jet_jit_io_stdin as *const u8);
    builder.symbol("jet_jit_stdout_write", jet_jit_stdout_write as *const u8);
    builder.symbol("jet_jit_stdout_write_line", jet_jit_stdout_write_line as *const u8);
    builder.symbol("jet_jit_stdout_write_bytes", jet_jit_stdout_write_bytes as *const u8);
    builder.symbol("jet_jit_stdout_flush", jet_jit_stdout_flush as *const u8);
    builder.symbol("jet_jit_stdout_is_tty", jet_jit_stdout_is_tty as *const u8);
    builder.symbol("jet_jit_stderr_write", jet_jit_stderr_write as *const u8);
    builder.symbol("jet_jit_stderr_write_line", jet_jit_stderr_write_line as *const u8);
    builder.symbol("jet_jit_stderr_write_bytes", jet_jit_stderr_write_bytes as *const u8);
    builder.symbol("jet_jit_stderr_flush", jet_jit_stderr_flush as *const u8);
    builder.symbol("jet_jit_stderr_is_tty", jet_jit_stderr_is_tty as *const u8);
    builder.symbol("jet_jit_terminal_width", jet_jit_terminal_width as *const u8);
    builder.symbol("jet_jit_terminal_height", jet_jit_terminal_height as *const u8);
    builder.symbol("jet_jit_io_style", jet_jit_io_style as *const u8);
    builder.symbol("jet_jit_io_style_force", jet_jit_io_style_force as *const u8);
    builder.symbol("jet_jit_io_progress", jet_jit_io_progress as *const u8);
    builder.symbol("jet_jit_io_confirm", jet_jit_io_confirm as *const u8);
    builder.symbol("jet_jit_io_choose", jet_jit_io_choose as *const u8);
    builder.symbol(
        "jet_jit_io_input_secret",
        jet_jit_io_input_secret as *const u8,
    );
    builder.symbol("jet_jit_stdin_lines", jet_jit_stdin_lines as *const u8);
    builder.symbol("jet_jit_file_lines", jet_jit_file_lines as *const u8);
    builder.symbol(
        "jet_jit_file_writer_write_line",
        jet_jit_file_writer_write_line as *const u8,
    );
    builder.symbol(
        "jet_jit_file_writer_flush",
        jet_jit_file_writer_flush as *const u8,
    );
    builder.symbol(
        "jet_jit_file_writer_close",
        super::enc_stream::jet_jit_file_writer_close as *const u8,
    );
    builder.symbol(
        "jet_jit_file_reader_close",
        super::enc_stream::jet_jit_file_reader_close as *const u8,
    );
    builder.symbol("jet_jit_term_enter", jet_jit_term_enter as *const u8);
    builder.symbol("jet_jit_term_leave", jet_jit_term_leave as *const u8);
}

pub(crate) fn declare_io_host_fns(module: &mut JITModule) -> Result<IOHostFns, String> {
    let cc = module.target_config().default_call_conv;
    let mut nullary = Signature::new(cc);
    nullary.returns.push(AbiParam::new(types::I64));
    let mut nullary_void = Signature::new(cc);
    let mut unary = Signature::new(cc);
    unary.params.push(AbiParam::new(types::I64));
    unary.returns.push(AbiParam::new(types::I64));
    let mut unary_void = Signature::new(cc);
    unary_void.params.push(AbiParam::new(types::I64));
    let mut unary_i8 = Signature::new(cc);
    unary_i8.params.push(AbiParam::new(types::I64));
    unary_i8.returns.push(AbiParam::new(types::I8));
    let mut binary = Signature::new(cc);
    binary.params.push(AbiParam::new(types::I64));
    binary.params.push(AbiParam::new(types::I64));
    binary.returns.push(AbiParam::new(types::I64));
    let mut import = |name: &str, sig: &Signature| {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
    Ok(IOHostFns {
        stdout: import("jet_jit_io_stdout", &nullary)?,
        stderr: import("jet_jit_io_stderr", &nullary)?,
        stdin: import("jet_jit_io_stdin", &nullary)?,
        stdout_write: import("jet_jit_stdout_write", &binary)?,
        stdout_write_line: import("jet_jit_stdout_write_line", &binary)?,
        stdout_write_bytes: import("jet_jit_stdout_write_bytes", &binary)?,
        stdout_flush: import("jet_jit_stdout_flush", &unary)?,
        stdout_is_tty: import("jet_jit_stdout_is_tty", &unary_i8)?,
        stderr_write: import("jet_jit_stderr_write", &binary)?,
        stderr_write_line: import("jet_jit_stderr_write_line", &binary)?,
        stderr_write_bytes: import("jet_jit_stderr_write_bytes", &binary)?,
        stderr_flush: import("jet_jit_stderr_flush", &unary)?,
        stderr_is_tty: import("jet_jit_stderr_is_tty", &unary_i8)?,
        terminal_width: import("jet_jit_terminal_width", &nullary)?,
        terminal_height: import("jet_jit_terminal_height", &nullary)?,
        style: import("jet_jit_io_style", &binary)?,
        style_force: import("jet_jit_io_style_force", &binary)?,
        progress: import("jet_jit_io_progress", &unary)?,
        confirm: import("jet_jit_io_confirm", &unary_i8)?,
        choose: import("jet_jit_io_choose", &binary)?,
        input_secret: import("jet_jit_io_input_secret", &unary)?,
        stdin_lines: import("jet_jit_stdin_lines", &unary)?,
        file_lines: import("jet_jit_file_lines", &unary)?,
        file_writer_write_line: import("jet_jit_file_writer_write_line", &binary)?,
        file_writer_flush: import("jet_jit_file_writer_flush", &unary)?,
        file_writer_close: import("jet_jit_file_writer_close", &unary_void)?,
        file_reader_close: import("jet_jit_file_reader_close", &unary_void)?,
        term_enter: import("jet_jit_term_enter", &nullary_void)?,
        term_leave: import("jet_jit_term_leave", &nullary_void)?,
    })
}
