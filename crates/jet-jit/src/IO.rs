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
    // Canonical enter restores via leave; under non-TTY tests tcgetattr fails
    // and AOT is a no-op. Keep pairing via a depth counter only.
    TERM_DEPTH.with(|d| d.set(d.get().saturating_add(1)));
}

extern "C" fn jet_jit_term_leave() {
    TERM_DEPTH.with(|d| {
        let n = d.get();
        if n > 0 {
            d.set(n - 1);
        }
    });
}

thread_local! {
    static TERM_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
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
