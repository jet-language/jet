//! `core.io` stdout/stderr/stdin + terminal hosts (#1219). Writes go to the
//! resident `JitRuntime` capture buffers so ProgramOutput matches AOT under
//! the process harness (real stdio would bypass capture).

use super::Concurrency;
use super::CoreHost::{jit_env_key_eq, jit_env_snapshot_raw};
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::io::{BufRead, IsTerminal, Write};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use crate::Marshal::{clone_string, result_err_msg, result_ok};

mod progress_semantics {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/Core/Progress.rs");
}

// #1480: literal Prelude source for line/byte stdin primitives + trivial
// text passthroughs (readline/read_until/take/sprint/repr). The extern "C"
// wrappers below only marshal jit heap i64 handles to/from Rust values and
// call these included functions — no logic is re-encoded here (I9). The
// nested `jet_std` mirrors only the IOError shape these functions construct
// via `.other(...)`; it carries no behavior of its own.
mod io_line_stream {
    #[allow(dead_code)]
    mod jet_std {
        #[derive(Debug)]
        pub enum IOOperation {
            Read,
            Flush,
        }
        #[derive(Debug)]
        pub struct IOContext {
            pub operation: IOOperation,
            pub resource: Option<String>,
            pub os_code: Option<i64>,
            pub cause: Option<String>,
        }
        #[derive(Debug)]
        pub enum IOError {
            Other(IOContext),
        }
        impl IOError {
            pub fn other(operation: IOOperation, resource: Option<String>, cause: impl ToString) -> Self {
                Self::Other(IOContext {
                    operation,
                    resource,
                    os_code: None,
                    cause: Some(cause.to_string()),
                })
            }
        }
    }

    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/IoLineStream.rs");

    pub(super) extern "C" fn jet_jit_io_sprint(text: i64) -> i64 {
        let s = super::clone_string(text);
        let out = jet_std_io_sprint(&s);
        super::Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(out))
    }

    pub(super) extern "C" fn jet_jit_io_repr(text: i64) -> i64 {
        let s = super::clone_string(text);
        let out = jet_std_io_repr(&s);
        super::Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(out))
    }

    pub(super) extern "C" fn jet_jit_io_take(n: i64) -> i64 {
        match jet_std_io_take(n) {
            Ok(buf) => {
                let list = super::Concurrency::with_runtime_mut(|rt| {
                    let list = rt.heap.alloc_empty_list();
                    for b in buf {
                        let _ = rt.heap.list_push_int(list, b as i64);
                    }
                    list
                });
                super::result_ok(list as u64)
            }
            Err(e) => super::result_err(&format!("{e:?}")),
        }
    }

    pub(super) extern "C" fn jet_jit_io_read_until(delim: i64) -> i64 {
        let needle = super::clone_string(delim);
        match jet_std_io_read_until(&needle) {
            Ok(s) => {
                let id = super::Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
                super::result_ok(id as u64)
            }
            Err(e) => super::result_err(&format!("{e:?}")),
        }
    }

    pub(super) extern "C" fn jet_jit_io_readline() -> i64 {
        match jet_std_io_readline() {
            Ok(s) => {
                let id = super::Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
                super::result_ok(id as u64)
            }
            Err(e) => super::result_err(&format!("{e:?}")),
        }
    }
}

#[derive(Clone)]
struct JitProgressState {
    description: String,
    format: String,
    total: Option<usize>,
    started: std::time::Instant,
    count: usize,
    /// `None` means the caller supplies raw source-pull counts (a direct
    /// loop). `Some` maps each materialized adapter output to the number of
    /// source pulls needed to produce it.
    plan: Option<Vec<usize>>,
    yielded: usize,
    tail: usize,
    displayed: bool,
}

fn progress_states() -> &'static Mutex<HashMap<i64, JitProgressState>> {
    static STATES: OnceLock<Mutex<HashMap<i64, JitProgressState>>> = OnceLock::new();
    STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn known_iter_lists() -> &'static Mutex<HashSet<i64>> {
    static KNOWN: OnceLock<Mutex<HashSet<i64>>> = OnceLock::new();
    KNOWN.get_or_init(|| Mutex::new(HashSet::new()))
}

fn result_ok_unit() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(super::JitResultValue { ok: true, bits: 0 });
        rt.results.len() as i64
    })
}

fn result_err(msg: &str) -> i64 {
    result_err_msg(msg)
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
    let s = clone_string(text);
    Concurrency::with_runtime_mut(|rt| rt.stdout.push_str(&s));
    result_ok_unit()
}

extern "C" fn jet_jit_stdout_write_line(_h: i64, text: i64) -> i64 {
    let s = clone_string(text);
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
    let s = clone_string(text);
    Concurrency::with_runtime_mut(|rt| rt.stderr.push_str(&s));
    result_ok_unit()
}

extern "C" fn jet_jit_stderr_write_line(_h: i64, text: i64) -> i64 {
    let s = clone_string(text);
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
    let style = clone_string(style);
    let text = clone_string(text);
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
    let style = clone_string(style);
    let text = clone_string(text);
    let out = match style_code(style.as_str()) {
        Some(code) => format!("\x1b[{code}m{text}\x1b[0m"),
        None => text,
    };
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(out))
}

extern "C" fn jet_jit_io_progress(text: i64) -> i64 {
    let s = clone_string(text);
    Concurrency::with_runtime_mut(|rt| {
        rt.stdout.push_str(&s);
        rt.stdout.push('\n');
    });
    result_ok_unit()
}

fn jet_jit_io_progress_iter_with_total(
    list: i64,
    description: i64,
    format: i64,
    total: Option<usize>,
) -> i64 {
    let description = clone_string(description);
    let format = clone_string(format);
    let wrapped = Concurrency::with_runtime_mut(|rt| {
        let wrapped = rt.heap.clone_list(list).unwrap_or(list);
        wrapped
    });
    progress_states()
        .lock()
        .expect("JIT progress state poisoned")
        .insert(
            wrapped,
            JitProgressState {
                description,
                format,
                total,
                started: std::time::Instant::now(),
                count: 0,
                plan: None,
                yielded: 0,
                tail: 0,
                displayed: false,
            },
        );
    wrapped
}

extern "C" fn jet_jit_io_progress_iter(list: i64, description: i64, format: i64) -> i64 {
    let known_total = known_iter_lists()
        .lock()
        .expect("JIT progress known-iter state poisoned")
        .remove(&list);
    let total = known_total.then(|| {
        Concurrency::with_runtime_mut(|rt| rt.heap.list_len(list).unwrap_or(0) as usize)
    });
    jet_jit_io_progress_iter_with_total(list, description, format, total)
}

extern "C" fn jet_jit_io_progress_list(list: i64, description: i64, format: i64) -> i64 {
    known_iter_lists()
        .lock()
        .expect("JIT progress known-iter state poisoned")
        .remove(&list);
    let total = Concurrency::with_runtime_mut(|rt| rt.heap.list_len(list).unwrap_or(0) as usize);
    jet_jit_io_progress_iter_with_total(list, description, format, Some(total))
}

extern "C" fn jet_jit_io_mark_exact_iter(list: i64) -> i64 {
    known_iter_lists()
        .lock()
        .expect("JIT progress known-iter state poisoned")
        .insert(list);
    list
}

pub(crate) fn jet_jit_io_progress_pull_n(list: i64, pulls: i64) {
    let requested = pulls.max(0) as usize;
    if requested == 0 {
        return;
    }
    let tty = std::io::stdout().is_terminal();
    let Some(texts) = (|| {
        let mut states = progress_states()
            .lock()
            .expect("JIT progress state poisoned");
        let state = states.get_mut(&list)?;
        let pulls = if let Some(plan) = &state.plan {
            let end = (state.yielded + requested).min(plan.len());
            let pulls = plan[state.yielded..end].iter().sum();
            state.yielded = end;
            pulls
        } else {
            requested
        };
        let pulls = state
            .total
            .map(|total| pulls.min(total.saturating_sub(state.count)))
            .unwrap_or(pulls);
        if pulls == 0 {
            return Some(Vec::new());
        }
        let mut texts = Vec::with_capacity(pulls);
        for _ in 0..pulls {
            state.count = state.count.saturating_add(1);
            texts.push(progress_semantics::jet_progress_render(
                &state.description,
                &state.format,
                state.count,
                state.total,
                state.started.elapsed().as_secs_f64(),
                env_value("NO_COLOR").is_some(),
            ));
        }
        state.displayed = true;
        Some(texts)
    })() else {
        return;
    };
    Concurrency::with_runtime_mut(|rt| {
        for text in texts {
            if tty {
                rt.stdout.push('\r');
            }
            rt.stdout.push_str(&text);
            if !tty {
                rt.stdout.push('\n');
            }
        }
    });
}

/// Finish a naturally exhausted JIT iterator. A stepped iterator can consume
/// trailing source items while looking for its next yielded item, so account
/// for the final unreported pulls before removing the sidecar state. Early
/// break/return paths call `progress_finish_state` directly and must not claim
/// those source pulls.
pub(crate) fn progress_exhaust_state(list: i64) {
    let remaining = progress_states()
        .lock()
        .expect("JIT progress state poisoned")
        .get(&list)
        .map(|state| {
            if state.plan.is_some() {
                state.tail
            } else {
                state
                    .total
                    .map(|total| total.saturating_sub(state.count))
                    .unwrap_or(0)
            }
        })
        .unwrap_or_default();
    if remaining != 0 {
        // A plan's tail is already a raw source-pull count. The direct-loop
        // form has no plan and uses the same argument as a raw count.
        let tty = std::io::stdout().is_terminal();
        let Some(texts) = (|| {
            let mut states = progress_states()
                .lock()
                .expect("JIT progress state poisoned");
            let state = states.get_mut(&list)?;
            let pulls = state
                .total
                .map(|total| remaining.min(total.saturating_sub(state.count)))
                .unwrap_or(remaining);
            let mut texts = Vec::with_capacity(pulls);
            for _ in 0..pulls {
                state.count = state.count.saturating_add(1);
                texts.push(progress_semantics::jet_progress_render(
                    &state.description,
                    &state.format,
                    state.count,
                    state.total,
                    state.started.elapsed().as_secs_f64(),
                    env_value("NO_COLOR").is_some(),
                ));
            }
            state.displayed |= !texts.is_empty();
            Some(texts)
        })() else {
            return;
        };
        Concurrency::with_runtime_mut(|rt| {
            for text in texts {
                if tty {
                    rt.stdout.push('\r');
                }
                rt.stdout.push_str(&text);
                if !tty {
                    rt.stdout.push('\n');
                }
            }
        });
    }
    progress_finish_state(list);
}

/// Move progress ownership when the JIT materializes a lazy adapter into a
/// fresh list handle. Ordinary lists have no state, so this is a no-op for all
/// other callers.
pub(crate) fn progress_transfer_state(source: i64, target: i64) {
    if source == target {
        return;
    }
    let mut states = progress_states()
        .lock()
        .expect("JIT progress state poisoned");
    if let Some(state) = states.remove(&source) {
        states.remove(&target);
        let target_len = Concurrency::with_runtime_mut(|rt| rt.heap.list_len(target).unwrap_or(0))
            as usize;
        let (plan, tail) = match state.plan.as_ref() {
            Some(source_plan) => {
                let mut plan = source_plan.clone();
                plan.truncate(target_len);
                if plan.len() < target_len {
                    plan.resize(target_len, 1);
                }
                (plan, state.tail)
            }
            None => (vec![1; target_len], 0),
        };
        states.insert(
            target,
            JitProgressState {
                plan: Some(plan),
                yielded: 0,
                tail,
                ..state
            },
        );
    }
}

fn source_plan(source: i64) -> Option<(Vec<usize>, usize)> {
    let states = progress_states()
        .lock()
        .expect("JIT progress state poisoned");
    let state = states.get(&source)?;
    let plan = match &state.plan {
        Some(plan) => plan.clone(),
        None => {
            let len = Concurrency::with_runtime_mut(|rt| rt.heap.list_len(source).unwrap_or(0))
                as usize;
            vec![1; len]
        }
    };
    Some((plan, state.tail))
}

/// Read the source-pull plan while a lazy progress adapter is being lowered.
/// A missing state means the value is an ordinary list and needs no sidecar.
pub(crate) fn progress_source_plan(source: i64) -> Option<(Vec<usize>, usize)> {
    source_plan(source)
}

fn install_plan(source: i64, target: i64, plan: Vec<usize>, tail: usize) {
    if source == target {
        return;
    }
    let mut states = progress_states()
        .lock()
        .expect("JIT progress state poisoned");
    let Some(mut state) = states.remove(&source) else {
        return;
    };
    states.remove(&target);
    state.plan = Some(plan);
    state.yielded = 0;
    state.tail = tail;
    states.insert(target, state);
}

/// Install an operation-specific source-pull plan on a materialized result.
/// The operation owns the mapping because output values alone cannot recover
/// how many source items a lazy adapter consumed.
pub(crate) fn progress_transfer_plan(
    source: i64,
    target: i64,
    plan: Vec<usize>,
    tail: usize,
) {
    install_plan(source, target, plan, tail);
}

fn progress_remove_state(list: i64) {
    progress_states()
        .lock()
        .expect("JIT progress state poisoned")
        .remove(&list);
}

fn prefix_plan(plan: &[usize], n: usize) -> Vec<usize> {
    plan.iter().copied().take(n).collect()
}

pub(crate) fn progress_transfer_take_state(source: i64, target: i64, n: i64) {
    let Some((plan, old_tail)) = source_plan(source) else {
        return;
    };
    let n = n.max(0) as usize;
    let output_len = n.min(plan.len());
    let tail = if n != 0 && n >= plan.len() { old_tail } else { 0 };
    install_plan(source, target, prefix_plan(&plan, output_len), tail);
}

pub(crate) fn progress_transfer_skip_state(source: i64, target: i64, n: i64) {
    let Some((plan, old_tail)) = source_plan(source) else {
        return;
    };
    let n = n.max(0) as usize;
    if n >= plan.len() {
        install_plan(source, target, Vec::new(), plan.iter().sum::<usize>() + old_tail);
        return;
    }
    let mut output = plan[n..].to_vec();
    output[0] = plan[..=n].iter().sum();
    install_plan(source, target, output, old_tail);
}

pub(crate) fn progress_transfer_step_state(source: i64, target: i64, n: i64) {
    let Some((plan, old_tail)) = source_plan(source) else {
        return;
    };
    let n = n.max(1) as usize;
    let mut output = Vec::new();
    let mut index = 0;
    if !plan.is_empty() {
        output.push(plan[0]);
        index = 1;
        while index < plan.len() {
            let end = (index + n).min(plan.len());
            if end - index < n {
                break;
            }
            output.push(plan[index..end].iter().sum());
            index = end;
        }
    }
    install_plan(source, target, output, plan[index..].iter().sum::<usize>() + old_tail);
}

pub(crate) fn progress_transfer_filter_state(source: i64, target: i64) {
    let Some((plan, old_tail)) = source_plan(source) else {
        return;
    };
    let source_values = Concurrency::with_runtime_mut(|rt| rt.heap.clone_int_list(source).unwrap_or_default());
    let target_values = Concurrency::with_runtime_mut(|rt| rt.heap.clone_int_list(target).unwrap_or_default());
    let target_len = target_values.len();
    let mut output = Vec::with_capacity(target_len);
    let mut source_index = 0;
    for &target_value in &target_values {
        let Some(found) = source_values[source_index..]
            .iter()
            .position(|source_value| *source_value == target_value || string_handle_eq(*source_value, target_value))
            .map(|offset| source_index + offset)
        else {
            install_plan(source, target, vec![1; target_values.len()], old_tail);
            return;
        };
        output.push(plan[source_index..=found].iter().sum());
        source_index = found + 1;
    }
    let tail = plan[source_index..].iter().sum::<usize>() + old_tail;
    install_plan(source, target, output, tail);
}

fn progress_value_equal(a: i64, b: i64) -> bool {
    if a == b {
        return true;
    }
    Concurrency::with_runtime_mut(|rt| {
        matches!((rt.heap.get_string(a), rt.heap.get_string(b)), (Some(a), Some(b)) if a == b)
    })
}

pub(crate) fn progress_transfer_dedup_state(source: i64, target: i64, string_elems: bool) {
    let Some((plan, old_tail)) = source_plan(source) else {
        return;
    };
    let values = Concurrency::with_runtime_mut(|rt| rt.heap.clone_int_list(source).unwrap_or_default());
    let mut output = Vec::new();
    let mut pending = 0usize;
    let mut previous = None;
    for (index, value) in values.into_iter().enumerate() {
        let pull = plan.get(index).copied().unwrap_or(1);
        let duplicate = previous.is_some_and(|last| {
            if string_elems {
                progress_value_equal(last, value)
            } else {
                last == value
            }
        });
        if duplicate {
            pending += pull;
        } else {
            output.push(pending + pull);
            pending = 0;
            previous = Some(value);
        }
    }
    install_plan(source, target, output, pending + old_tail);
}

pub(crate) fn progress_transfer_chunks_state(source: i64, target: i64, n: i64) {
    let Some((plan, old_tail)) = source_plan(source) else {
        return;
    };
    let source_len = Concurrency::with_runtime_mut(|rt| rt.heap.list_len(source).unwrap_or(0)) as usize;
    let size = n.max(1) as usize;
    let mut output = Vec::new();
    let mut start = 0usize;
    while start < source_len {
        let end = (start + size).min(source_len);
        output.push(plan[start..end].iter().sum());
        start = end;
    }
    install_plan(source, target, output, old_tail);
}

pub(crate) fn progress_transfer_windows_state(source: i64, target: i64, n: i64) {
    let Some((plan, old_tail)) = source_plan(source) else {
        return;
    };
    let source_len = plan.len();
    let size = n.max(1) as usize;
    if source_len < size {
        install_plan(
            source,
            target,
            Vec::new(),
            plan.iter().sum::<usize>() + old_tail,
        );
        return;
    }
    let mut output = Vec::with_capacity(source_len - size + 1);
    output.push(plan[..size].iter().sum());
    output.extend(plan.iter().copied().skip(size).take(source_len - size));
    install_plan(source, target, output, old_tail);
}

pub(crate) fn progress_transfer_flatten_state(source: i64, target: i64) {
    let Some((plan, old_tail)) = source_plan(source) else {
        return;
    };
    let outer = Concurrency::with_runtime_mut(|rt| rt.heap.clone_int_list(source).unwrap_or_default());
    let mut output = Vec::new();
    let mut pending = 0usize;
    for (index, inner_handle) in outer.into_iter().enumerate() {
        let pull = plan.get(index).copied().unwrap_or(1);
        let inner = Concurrency::with_runtime_mut(|rt| rt.heap.clone_int_list(inner_handle).unwrap_or_default());
        if inner.is_empty() {
            pending += pull;
        } else {
            output.push(pending + pull);
            output.extend(std::iter::repeat(0).take(inner.len() - 1));
            pending = 0;
        }
    }
    install_plan(source, target, output, pending + old_tail);
}

pub(crate) fn progress_transfer_intersperse_state(source: i64, target: i64) {
    let Some((plan, old_tail)) = source_plan(source) else {
        return;
    };
    let source_len = plan.len();
    let mut output = Vec::with_capacity(source_len.saturating_mul(2));
    for pull in plan {
        output.push(pull);
        output.push(0);
    }
    if !output.is_empty() {
        output.pop();
    }
    install_plan(source, target, output, old_tail);
}

pub(crate) fn progress_transfer_take_while_state(source: i64, target: i64, is_skip: bool) {
    let Some((plan, old_tail)) = source_plan(source) else {
        return;
    };
    let source_len = Concurrency::with_runtime_mut(|rt| rt.heap.list_len(source).unwrap_or(0)) as usize;
    let target_len = Concurrency::with_runtime_mut(|rt| rt.heap.list_len(target).unwrap_or(0)) as usize;
    let target_len = target_len.min(source_len);
    if is_skip {
        if target_len == 0 {
            install_plan(source, target, Vec::new(), plan.iter().sum::<usize>() + old_tail);
            return;
        }
        let skipped = source_len.saturating_sub(target_len);
        let first = plan[..=skipped.min(plan.len().saturating_sub(1))]
            .iter()
            .sum();
        let mut output = Vec::with_capacity(target_len);
        output.push(first);
        output.extend(plan.iter().copied().skip(skipped + 1).take(target_len - 1));
        install_plan(source, target, output, old_tail);
    } else {
        let output = plan.iter().copied().take(target_len).collect::<Vec<_>>();
        let tail = if target_len < source_len {
            plan.get(target_len).copied().unwrap_or(0)
        } else {
            old_tail
        };
        install_plan(source, target, output, tail);
    }
}

fn string_handle_eq(a: i64, b: i64) -> bool {
    if a == b {
        return true;
    }
    Concurrency::with_runtime_mut(|rt| {
        matches!((rt.heap.get_string(a), rt.heap.get_string(b)), (Some(a), Some(b)) if a == b)
    })
}

pub(crate) fn progress_transfer_zip_state(left: i64, right: i64, target: i64) {
    let left_active = progress_states()
        .lock()
        .expect("JIT progress state poisoned")
        .contains_key(&left);
    let right_active = progress_states()
        .lock()
        .expect("JIT progress state poisoned")
        .contains_key(&right);
    let target_len = Concurrency::with_runtime_mut(|rt| rt.heap.list_len(target).unwrap_or(0)) as usize;
    let selected = if left_active { Some(left) } else if right_active { Some(right) } else { None };
    if let Some(source) = selected {
        if let Some((plan, old_tail)) = source_plan(source) {
            let consumed = target_len.min(plan.len());
            // `Iterator::zip` asks the receiver for one item before it asks
            // the other side. When the other side ends first, an exhaustive
            // zip therefore consumes one extra receiver item while probing
            // for the next pair. Preserve that source pull in the progress
            // sidecar; a right-only active source has no such probe.
            let tail = if target_len < plan.len() && source == left {
                plan.get(target_len).copied().unwrap_or(0)
            } else if target_len >= plan.len() {
                old_tail
            } else {
                0
            };
            install_plan(source, target, plan.into_iter().take(consumed).collect(), tail);
        }
    }
    if left_active && right_active {
        progress_remove_state(if selected == Some(left) { right } else { left });
    }
}

pub(crate) fn progress_finish_state(list: i64) {
    let Some(state) = progress_states()
        .lock()
        .expect("JIT progress state poisoned")
        .remove(&list)
    else {
        return;
    };
    if state.displayed && std::io::stdout().is_terminal() {
        Concurrency::with_runtime_mut(|rt| rt.stdout.push('\n'));
    }
}

extern "C" fn jet_jit_io_progress_pull(list: i64, pulls: i64) {
    jet_jit_io_progress_pull_n(list, pulls);
}

extern "C" fn jet_jit_io_progress_finish(list: i64) {
    progress_finish_state(list);
}

extern "C" fn jet_jit_io_progress_exhaust(list: i64) {
    progress_exhaust_state(list);
}

extern "C" fn jet_jit_io_progress_transfer(source: i64, target: i64) {
    progress_transfer_state(source, target);
}

extern "C" fn jet_jit_io_progress_transfer_filter(source: i64, target: i64) {
    progress_transfer_filter_state(source, target);
}

extern "C" fn jet_jit_io_progress_transfer_dedup(source: i64, target: i64, string_elems: i64) {
    progress_transfer_dedup_state(source, target, string_elems != 0);
}

extern "C" fn jet_jit_io_progress_transfer_chunks(source: i64, target: i64, n: i64) {
    progress_transfer_chunks_state(source, target, n);
}

extern "C" fn jet_jit_io_progress_transfer_windows(source: i64, target: i64, n: i64) {
    progress_transfer_windows_state(source, target, n);
}

extern "C" fn jet_jit_io_progress_transfer_flatten(source: i64, target: i64) {
    progress_transfer_flatten_state(source, target);
}

extern "C" fn jet_jit_io_progress_transfer_intersperse(source: i64, target: i64) {
    progress_transfer_intersperse_state(source, target);
}

extern "C" fn jet_jit_io_progress_transfer_take_while(
    source: i64,
    target: i64,
    is_skip: i64,
) {
    progress_transfer_take_while_state(source, target, is_skip != 0);
}

extern "C" fn jet_jit_io_progress_source_pull(source: i64, index: i64) -> i64 {
    progress_source_plan(source)
        .and_then(|(plan, _)| plan.get(index.max(0) as usize).copied())
        .unwrap_or(1) as i64
}

extern "C" fn jet_jit_io_progress_transfer_plan(
    source: i64,
    target: i64,
    plan: i64,
    tail: i64,
) {
    let plan = Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .clone_int_list(plan)
            .unwrap_or_default()
            .into_iter()
            .map(|value| value.max(0) as usize)
            .collect::<Vec<_>>()
    });
    progress_transfer_plan(source, target, plan, tail.max(0) as usize);
}

extern "C" fn jet_jit_io_progress_collect(list: i64) -> i64 {
    let active = progress_states()
        .lock()
        .expect("JIT progress state poisoned")
        .contains_key(&list);
    if active {
        let total = Concurrency::with_runtime_mut(|rt| rt.heap.list_len(list).unwrap_or(0));
        jet_jit_io_progress_pull_n(list, total);
        progress_exhaust_state(list);
    }
    list
}

extern "C" fn jet_jit_io_confirm(prompt: i64) -> i8 {
    i8::from(prompt_confirm(&clone_string(prompt)))
}

extern "C" fn jet_jit_io_choose(prompt: i64, items: i64) -> i64 {
    let prompt = clone_string(prompt);
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
            result_ok(id as u64)
        }
        Err(error) => result_err(&error),
    }
}

extern "C" fn jet_jit_io_input_secret(prompt: i64) -> i64 {
    match prompt_input_secret(&clone_string(prompt)) {
        Ok(secret) => {
            let id = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(secret));
            result_ok(id as u64)
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
    let text = clone_string(line);
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

host_fns! {
    struct IOHostFns;
    register: register_io_symbols;
    declare: declare_io_host_fns(module) {
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
        let mut binary_void = Signature::new(cc);
        binary_void.params.push(AbiParam::new(types::I64));
        binary_void.params.push(AbiParam::new(types::I64));
        let mut ternary = Signature::new(cc);
        ternary.params.push(AbiParam::new(types::I64));
        ternary.params.push(AbiParam::new(types::I64));
        ternary.params.push(AbiParam::new(types::I64));
        ternary.returns.push(AbiParam::new(types::I64));
        let mut ternary_void = Signature::new(cc);
        ternary_void.params.push(AbiParam::new(types::I64));
        ternary_void.params.push(AbiParam::new(types::I64));
        ternary_void.params.push(AbiParam::new(types::I64));
        let mut quaternary_void = Signature::new(cc);
        quaternary_void.params.push(AbiParam::new(types::I64));
        quaternary_void.params.push(AbiParam::new(types::I64));
        quaternary_void.params.push(AbiParam::new(types::I64));
        quaternary_void.params.push(AbiParam::new(types::I64));


    }
    stdout: "jet_jit_io_stdout" => jet_jit_io_stdout: nullary;
    stderr: "jet_jit_io_stderr" => jet_jit_io_stderr: nullary;
    stdin: "jet_jit_io_stdin" => jet_jit_io_stdin: nullary;
    stdout_write: "jet_jit_stdout_write" => jet_jit_stdout_write: binary;
    stdout_write_line: "jet_jit_stdout_write_line" => jet_jit_stdout_write_line: binary;
    stdout_write_bytes: "jet_jit_stdout_write_bytes" => jet_jit_stdout_write_bytes: binary;
    stdout_flush: "jet_jit_stdout_flush" => jet_jit_stdout_flush: unary;
    stdout_is_tty: "jet_jit_stdout_is_tty" => jet_jit_stdout_is_tty: unary_i8;
    stderr_write: "jet_jit_stderr_write" => jet_jit_stderr_write: binary;
    stderr_write_line: "jet_jit_stderr_write_line" => jet_jit_stderr_write_line: binary;
    stderr_write_bytes: "jet_jit_stderr_write_bytes" => jet_jit_stderr_write_bytes: binary;
    stderr_flush: "jet_jit_stderr_flush" => jet_jit_stderr_flush: unary;
    stderr_is_tty: "jet_jit_stderr_is_tty" => jet_jit_stderr_is_tty: unary_i8;
    terminal_width: "jet_jit_terminal_width" => jet_jit_terminal_width: nullary;
    terminal_height: "jet_jit_terminal_height" => jet_jit_terminal_height: nullary;
    style: "jet_jit_io_style" => jet_jit_io_style: binary;
    style_force: "jet_jit_io_style_force" => jet_jit_io_style_force: binary;
    progress: "jet_jit_io_progress" => jet_jit_io_progress: unary;
    progress_iter: "jet_jit_io_progress_iter" => jet_jit_io_progress_iter: ternary;
    progress_list: "jet_jit_io_progress_list" => jet_jit_io_progress_list: ternary;
    progress_mark_exact: "jet_jit_io_mark_exact_iter" => jet_jit_io_mark_exact_iter: unary;
    progress_pull: "jet_jit_io_progress_pull" => jet_jit_io_progress_pull: binary_void;
    progress_finish: "jet_jit_io_progress_finish" => jet_jit_io_progress_finish: unary_void;
    progress_exhaust: "jet_jit_io_progress_exhaust" => jet_jit_io_progress_exhaust: unary_void;
    progress_transfer: "jet_jit_io_progress_transfer" => jet_jit_io_progress_transfer: binary_void;
    progress_transfer_filter: "jet_jit_io_progress_transfer_filter" => jet_jit_io_progress_transfer_filter: binary_void;
    progress_transfer_dedup: "jet_jit_io_progress_transfer_dedup" => jet_jit_io_progress_transfer_dedup: ternary_void;
    progress_transfer_chunks: "jet_jit_io_progress_transfer_chunks" => jet_jit_io_progress_transfer_chunks: ternary_void;
    progress_transfer_windows: "jet_jit_io_progress_transfer_windows" => jet_jit_io_progress_transfer_windows: ternary_void;
    progress_transfer_flatten: "jet_jit_io_progress_transfer_flatten" => jet_jit_io_progress_transfer_flatten: binary_void;
    progress_transfer_intersperse: "jet_jit_io_progress_transfer_intersperse" => jet_jit_io_progress_transfer_intersperse: binary_void;
    progress_transfer_take_while: "jet_jit_io_progress_transfer_take_while" => jet_jit_io_progress_transfer_take_while: ternary_void;
    progress_source_pull: "jet_jit_io_progress_source_pull" => jet_jit_io_progress_source_pull: binary;
    progress_transfer_plan: "jet_jit_io_progress_transfer_plan" => jet_jit_io_progress_transfer_plan: quaternary_void;
    progress_collect: "jet_jit_io_progress_collect" => jet_jit_io_progress_collect: unary;
    confirm: "jet_jit_io_confirm" => jet_jit_io_confirm: unary_i8;
    choose: "jet_jit_io_choose" => jet_jit_io_choose: binary;
    input_secret: "jet_jit_io_input_secret" => jet_jit_io_input_secret: unary;
    sprint: "jet_jit_io_sprint" => io_line_stream::jet_jit_io_sprint: unary;
    repr: "jet_jit_io_repr" => io_line_stream::jet_jit_io_repr: unary;
    take: "jet_jit_io_take" => io_line_stream::jet_jit_io_take: unary;
    read_until: "jet_jit_io_read_until" => io_line_stream::jet_jit_io_read_until: unary;
    readline: "jet_jit_io_readline" => io_line_stream::jet_jit_io_readline: nullary;
    stdin_lines: "jet_jit_stdin_lines" => jet_jit_stdin_lines: unary;
    file_lines: "jet_jit_file_lines" => jet_jit_file_lines: unary;
    file_writer_write_line: "jet_jit_file_writer_write_line" => jet_jit_file_writer_write_line: binary;
    file_writer_flush: "jet_jit_file_writer_flush" => jet_jit_file_writer_flush: unary;
    file_writer_close: "jet_jit_file_writer_close" => super::enc_stream::jet_jit_file_writer_close: unary_void;
    file_reader_close: "jet_jit_file_reader_close" => super::enc_stream::jet_jit_file_reader_close: unary_void;
    term_enter: "jet_jit_term_enter" => jet_jit_term_enter: nullary_void;
    term_leave: "jet_jit_term_leave" => jet_jit_term_leave: nullary_void;
}





