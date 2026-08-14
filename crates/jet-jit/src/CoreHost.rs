//! Host shims for `core.os`, `core.log`, `core.math`, and `core.files`,
//! plus `core.env` and `core.process` CoreCalls (#729). Behavior
//! mirrors AOT helpers in the CoreLib prelude (`jet_std_os_*`, `jet_ring_log_*`,
//! `jet_std_math_*`, `jet_std_fs_*`, `jet_std_path_*`, `jet_std_env_*`,
//! `jet_std_process_*`) — thin std wrappers, not a third algorithm.
//! parity: guard tests/dev.rs::io_cli_terminal_and_time_match_interpreter_jit_and_aot

use super::Concurrency;
use std::cell::{Cell, RefCell};
use crate::Marshal::{clone_string, clone_bytes, alloc_byte_list, result_ok, result_err_msg};
use std::sync::{mpsc, OnceLock};

mod path_kernel {
    include!("../../jet-codegen/src/Prelude/Core/Path.rs");
}

mod interrupt_queue {
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/Interrupt.rs");
}

// The Prelude owns every core.os fact. This module supplies only the small
// type/ambient surface needed to include that exact source; wrappers below
// marshal its Rust values to the resident heap ABI.
mod os_rt {
    pub(super) mod jet_std {
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub(crate) enum IOOperation {
            Read,
            Write,
            Flush,
            Connect,
            Accept,
            Close,
            Resolve,
            Codec,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub(crate) struct IOContext {
            pub(crate) operation: IOOperation,
            pub(crate) resource: Option<String>,
            pub(crate) os_code: Option<i64>,
            pub(crate) cause: Option<String>,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub(crate) enum IOError {
            InvalidInput(IOContext),
            NotFound(IOContext),
            PermissionDenied(IOContext),
            TimedOut(IOContext),
            Cancelled(IOContext),
            Closed(IOContext),
            Protocol(IOContext),
            Other(IOContext),
        }

        impl IOError {
            pub(crate) fn other(
                operation: IOOperation,
                resource: Option<String>,
                cause: impl ToString,
            ) -> Self {
                Self::Other(IOContext {
                    operation,
                    resource,
                    os_code: None,
                    cause: Some(cause.to_string()),
                })
            }
        }
    }

    fn jet_std_env_get(name: &String) -> Option<String> {
        let key = std::ffi::OsStr::new(name);
        super::jit_env_snapshot_raw()
            .into_iter()
            .find(|(candidate, _)| super::jit_env_key_eq(candidate.as_os_str(), key))
            .and_then(|(_, value)| value.into_string().ok())
    }

    fn jet_std_process_exit(code: i64) {
        super::jet_jit_process_exit(code);
    }

    mod prelude_impl {
        use super::{jet_std, jet_std_env_get, jet_std_process_exit};

        include!("../../jet-codegen/src/Prelude/CoreLib/Top/OsExtra.rs");
    }

    fn operation_index(operation: &jet_std::IOOperation) -> i64 {
        let name = match operation {
            jet_std::IOOperation::Read => "Read",
            jet_std::IOOperation::Write => "Write",
            jet_std::IOOperation::Flush => "Flush",
            jet_std::IOOperation::Connect => "Connect",
            jet_std::IOOperation::Accept => "Accept",
            jet_std::IOOperation::Close => "Close",
            jet_std::IOOperation::Resolve => "Resolve",
            jet_std::IOOperation::Codec => "Codec",
        };
        jet_foundation::Syntax::IO_OPERATION_VARIANTS
            .iter()
            .position(|candidate| *candidate == name)
            .map(|index| index as i64)
            .expect("Prelude IOOperation must be registered")
    }

    fn error_index(name: &str) -> i64 {
        jet_foundation::Syntax::IO_ERROR_VARIANTS
            .iter()
            .position(|candidate| *candidate == name)
            .map(|index| index as i64)
            .expect("Prelude IOError must be registered")
    }

    pub(super) fn marshal_error(error: jet_std::IOError) -> i64 {
        let (name, context) = match error {
            jet_std::IOError::InvalidInput(context) => ("InvalidInput", context),
            jet_std::IOError::NotFound(context) => ("NotFound", context),
            jet_std::IOError::PermissionDenied(context) => ("PermissionDenied", context),
            jet_std::IOError::TimedOut(context) => ("TimedOut", context),
            jet_std::IOError::Cancelled(context) => ("Cancelled", context),
            jet_std::IOError::Closed(context) => ("Closed", context),
            jet_std::IOError::Protocol(context) => ("Protocol", context),
            jet_std::IOError::Other(context) => ("Other", context),
        };
        super::Concurrency::with_runtime_mut(|rt| {
            let record = rt.heap.alloc_record(4);
            let _ = rt.heap.record_set_int(record, 0, operation_index(&context.operation));
            let resource = context
                .resource
                .map(|value| rt.heap.alloc_string(value).wrapping_add(1))
                .unwrap_or(0);
            let _ = rt.heap.record_set_int(record, 1, resource);
            let _ = rt.heap.record_set_int(record, 2, context.os_code.map(|code| code + 1).unwrap_or(0));
            let cause = context
                .cause
                .map(|value| rt.heap.alloc_string(value).wrapping_add(1))
                .unwrap_or(0);
            let _ = rt.heap.record_set_int(record, 3, cause);
            let packed = ((record << 8) | error_index(name)) as u64;
            rt.results.push(crate::JitResultValue { ok: false, bits: packed });
            rt.results.len() as i64
        })
    }

    pub(super) fn marshal_result<T>(result: Result<T, jet_std::IOError>, ok: impl FnOnce(T) -> u64) -> i64 {
        match result {
            Ok(value) => super::result_ok(ok(value)),
            Err(error) => marshal_error(error),
        }
    }

    pub(super) use prelude_impl::{
            jet_std_os_arch, jet_std_os_cpu_count, jet_std_os_exitcode,
            jet_std_os_executable, jet_std_os_expand, jet_std_os_family, jet_std_os_fork,
            jet_std_os_geteuid, jet_std_os_getegid, jet_std_os_getgid, jet_std_os_getgroups,
            jet_std_os_getpgid, jet_std_os_getpgrp, jet_std_os_getppid, jet_std_os_getpriority,
            jet_std_os_getsid, jet_std_os_getuid, jet_std_os_hostname, jet_std_os_initgroups,
            jet_std_os_kill, jet_std_os_loadavg, jet_std_os_mkfifo, jet_std_os_name,
            jet_std_os_pipe, jet_std_os_release, jet_std_os_setgid, jet_std_os_setpgid,
            jet_std_os_setpgrp, jet_std_os_setpriority, jet_std_os_setsid, jet_std_os_setuid,
            jet_std_os_success, jet_std_os_sync, jet_std_os_temp_dir,
            jet_std_os_times, jet_std_os_umask, jet_std_os_uptime, jet_std_os_username,
            jet_std_os_utime, jet_std_os_version, jet_std_os_wait, jet_std_os_waitpid,
            jet_std_os_close_fd, jet_std_os_pid,
    };
}

// The resident JIT cannot hand a Rust `Rc` callback to the process signal
// boundary. TIR gives it one Send-safe record containing a function address and
// environment handle. This adapter owns only the raw-code invocation boundary;
// pending counts and additive ordering come from the shared Prelude queue.
mod jit_os_interrupt {
    use super::{interrupt_queue::{self, JetInterruptQueue}, mpsc, Concurrency, OnceLock};

    static QUEUE: JetInterruptQueue = JetInterruptQueue::new();
    static DISPATCH: OnceLock<Result<mpsc::Sender<DispatchCommand>, String>> = OnceLock::new();

    struct Command {
        callback: usize,
        env: i64,
        ready: mpsc::SyncSender<()>,
    }

    enum DispatchCommand {
        Register(Command),
        Reset(mpsc::SyncSender<()>),
    }

    fn note_interrupt() {
        QUEUE.note();
    }

    #[cfg(unix)]
    extern "C" fn unix_mark(_: i32) {
        note_interrupt();
    }

    #[cfg(unix)]
    fn install_platform_handler() -> Result<(), String> {
        interrupt_queue::jet_interrupt_install_unix_handler(unix_mark)
    }

    #[cfg(windows)]
    unsafe extern "system" fn windows_mark(kind: u32) -> i32 {
        if kind == 0 {
            note_interrupt();
            1
        } else {
            0
        }
    }

    #[cfg(windows)]
    fn install_platform_handler() -> Result<(), String> {
        interrupt_queue::jet_interrupt_install_windows_handler(Some(windows_mark))
    }

    #[cfg(not(any(unix, windows)))]
    fn install_platform_handler() -> Result<(), String> {
        Err(interrupt_queue::jet_interrupt_unavailable_error().to_string())
    }

    fn dispatcher() -> Result<&'static mpsc::Sender<DispatchCommand>, String> {
        match DISPATCH.get_or_init(|| {
            install_platform_handler()?;
            let (tx, rx) = mpsc::channel::<DispatchCommand>();
            std::thread::Builder::new()
                .name("jet-jit-interrupt".to_string())
                .spawn(move || {
                    let mut handlers: Vec<(usize, i64)> = Vec::new();
                    loop {
                        match rx.recv_timeout(interrupt_queue::jet_interrupt_poll_interval()) {
                            Ok(DispatchCommand::Register(command)) => {
                                handlers.push((command.callback, command.env));
                                let _ = command.ready.send(());
                            }
                            Ok(DispatchCommand::Reset(ready)) => {
                                handlers.clear();
                                QUEUE.clear();
                                let _ = ready.send(());
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                        }
                        QUEUE.dispatch(&handlers, |&(callback, environment)| {
                            Concurrency::with_http_jet_runtime(|| {
                                // Every callback, including named and
                                // capture-free callbacks, uses this one ABI.
                                unsafe {
                                    let callback: extern "C" fn(i64) =
                                        std::mem::transmute(callback);
                                    callback(environment);
                                }
                            });
                        });
                    }
                })
                .map_err(interrupt_queue::jet_interrupt_dispatcher_start_error)?;
            Ok(tx)
        }) {
            Ok(tx) => Ok(tx),
            Err(message) => Err(message.clone()),
        }
    }

    pub(super) fn register(callback_record: i64) {
        let result = (|| {
            let (callback, environment) = Concurrency::with_runtime_mut(|rt| {
                (
                    rt.heap.record_get_int(callback_record, 0).unwrap_or(0),
                    rt.heap.record_get_int(callback_record, 1).unwrap_or(0),
                )
            });
            if callback == 0 {
                return Err(
                    interrupt_queue::jet_interrupt_invalid_callback_record_error().to_string(),
                );
            }
            let tx = dispatcher()?;
            let (ready_tx, ready_rx) = mpsc::sync_channel(0);
            tx.send(DispatchCommand::Register(Command {
                callback: callback as usize,
                env: environment,
                ready: ready_tx,
            }))
            .map_err(|_| {
                interrupt_queue::jet_interrupt_dispatcher_stopped_error().to_string()
            })?;
            ready_rx
                .recv()
                .map_err(|_| {
                    interrupt_queue::jet_interrupt_dispatcher_stopped_error().to_string()
                })
        })();
        if let Err(message) = result {
            Concurrency::with_runtime_mut(|rt| {
                rt.set_trap(&interrupt_queue::jet_interrupt_core_error(&message));
            });
        }
    }

    pub(super) fn reset() {
        let Some(Ok(tx)) = DISPATCH.get() else {
            return;
        };
        let (ready_tx, ready_rx) = mpsc::sync_channel(0);
        if tx.send(DispatchCommand::Reset(ready_tx)).is_ok() {
            let _ = ready_rx.recv();
        }
    }
}

extern "C" fn jet_jit_os_on_interrupt(callback_record: i64) {
    jit_os_interrupt::register(callback_record);
}

pub(crate) fn reset_jit_interrupts() {
    jit_os_interrupt::reset();
}

// ── core.os (Prelude facts; these functions only marshal values) ─────────────

fn alloc_i64_list(values: &[i64]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for &value in values {
            let _ = rt.heap.list_push_int(list, value);
        }
        list
    })
}

fn alloc_f64_list_os(values: &[f64]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for &value in values {
            let _ = rt.heap.list_push_float(list, value);
        }
        list
    })
}

extern "C" fn jet_jit_os_name() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_name()))
}
extern "C" fn jet_jit_os_family() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_family()))
}
extern "C" fn jet_jit_os_arch() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_arch()))
}
extern "C" fn jet_jit_os_cpu_count() -> i64 {
    os_rt::jet_std_os_cpu_count()
}
extern "C" fn jet_jit_os_temp_dir() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_temp_dir()))
}
extern "C" fn jet_jit_os_executable() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_executable()))
}
extern "C" fn jet_jit_os_pid() -> i64 {
    os_rt::jet_std_os_pid()
}
extern "C" fn jet_jit_os_hostname() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_hostname()))
}
extern "C" fn jet_jit_os_username() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_username()))
}
extern "C" fn jet_jit_os_release() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_release()))
}
extern "C" fn jet_jit_os_version() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_version()))
}
extern "C" fn jet_jit_os_getppid() -> i64 {
    os_rt::jet_std_os_getppid()
}
extern "C" fn jet_jit_os_getuid() -> i64 {
    os_rt::jet_std_os_getuid()
}
extern "C" fn jet_jit_os_geteuid() -> i64 {
    os_rt::jet_std_os_geteuid()
}
extern "C" fn jet_jit_os_getgid() -> i64 {
    os_rt::jet_std_os_getgid()
}
extern "C" fn jet_jit_os_getegid() -> i64 {
    os_rt::jet_std_os_getegid()
}
extern "C" fn jet_jit_os_getpgrp() -> i64 {
    os_rt::jet_std_os_getpgrp()
}
extern "C" fn jet_jit_os_getgroups() -> i64 {
    alloc_i64_list(&os_rt::jet_std_os_getgroups())
}
extern "C" fn jet_jit_os_uptime() -> f64 {
    os_rt::jet_std_os_uptime()
}
extern "C" fn jet_jit_os_loadavg() -> i64 {
    alloc_f64_list_os(&os_rt::jet_std_os_loadavg())
}
extern "C" fn jet_jit_os_times() -> i64 {
    alloc_f64_list_os(&os_rt::jet_std_os_times())
}
extern "C" fn jet_jit_os_success(status: i64) -> i8 {
    i8::from(os_rt::jet_std_os_success(status))
}
extern "C" fn jet_jit_os_exitcode(status: i64) -> i64 {
    os_rt::jet_std_os_exitcode(status)
}
extern "C" fn jet_jit_os_expand(template: i64) -> i64 {
    let template = clone_string(template);
    let value = os_rt::jet_std_os_expand(&template);
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value))
}
extern "C" fn jet_jit_os_getpgid(pid: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_getpgid(pid), |value| value as u64)
}
extern "C" fn jet_jit_os_getsid(pid: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_getsid(pid), |value| value as u64)
}
extern "C" fn jet_jit_os_setpgid(pid: i64, pgid: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_setpgid(pid, pgid), |_| 0)
}
extern "C" fn jet_jit_os_setpgrp() -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_setpgrp(), |_| 0)
}
extern "C" fn jet_jit_os_umask(mask: i64) -> i64 {
    os_rt::jet_std_os_umask(mask)
}
extern "C" fn jet_jit_os_sync() {
    os_rt::jet_std_os_sync()
}
extern "C" fn jet_jit_os_getpriority(who: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_getpriority(who), |value| value as u64)
}
extern "C" fn jet_jit_os_setpriority(who: i64, priority: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_setpriority(who, priority), |_| 0)
}
extern "C" fn jet_jit_os_kill(pid: i64, signal: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_kill(pid, signal), |_| 0)
}
extern "C" fn jet_jit_os_pipe() -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_pipe(), |values| alloc_i64_list(&values) as u64)
}
extern "C" fn jet_jit_os_close_fd(fd: i64) {
    os_rt::jet_std_os_close_fd(fd)
}
extern "C" fn jet_jit_os_mkfifo(path: i64, mode: i64) -> i64 {
    let path = clone_string(path);
    os_rt::marshal_result(os_rt::jet_std_os_mkfifo(&path, mode), |_| 0)
}
extern "C" fn jet_jit_os_fork() -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_fork(), |value| value as u64)
}
extern "C" fn jet_jit_os_setuid(uid: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_setuid(uid), |_| 0)
}
extern "C" fn jet_jit_os_setgid(gid: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_setgid(gid), |_| 0)
}
extern "C" fn jet_jit_os_setsid() -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_setsid(), |value| value as u64)
}
extern "C" fn jet_jit_os_initgroups(user: i64, group: i64) -> i64 {
    let user = clone_string(user);
    os_rt::marshal_result(os_rt::jet_std_os_initgroups(&user, group), |_| 0)
}
extern "C" fn jet_jit_os_wait() -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_wait(), |value| value as u64)
}
extern "C" fn jet_jit_os_waitpid(pid: i64, options: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_waitpid(pid, options), |value| value as u64)
}
extern "C" fn jet_jit_os_utime(path: i64, atime: i64, mtime: i64) -> i64 {
    let path = clone_string(path);
    os_rt::marshal_result(os_rt::jet_std_os_utime(&path, atime, mtime), |_| 0)
}
extern "C" fn jet_jit_os_atexit(handler: i64) -> i64 {
    if crate::runtime_host::register_jit_atexit(handler) {
        result_ok(0)
    } else {
        result_err_msg("invalid resident atexit callback")
    }
}
extern "C" fn jet_jit_os_stop(code: i64) {
    // D-FAIL-EXIT1: the resident host is an in-process boundary. Keep the
    // same soft exit record as `process.exit`; resident cleanup drains it
    // before returning the run outcome.
    jet_jit_process_exit(code)
}


// ── core.log (mirrors jet_ring_log_* in RingCsvLogTimeCrypto.rs) ───────────────
// Level: 0=debug, 1=info, 2=warn, 3=error. Format: 0=auto, 1=json, 2=text.

thread_local! {
    static JIT_LOG_LEVEL: Cell<u8> = const { Cell::new(1) };
    static JIT_LOG_DISABLED: Cell<bool> = const { Cell::new(false) };
    static JIT_LOG_FORMAT: Cell<u8> = const { Cell::new(0) };
    static JIT_LOG_TRACE_ID: RefCell<String> = const { RefCell::new(String::new()) };
    static JIT_LOG_SPANS: RefCell<Vec<(i64, String)>> = const { RefCell::new(Vec::new()) };
    static JIT_LOG_NEXT_SPAN: Cell<i64> = const { Cell::new(1) };
}

struct JitLogField {
    key: String,
    value: String,
    kind: String,
}

fn jit_log_level_rank(level: &str) -> Option<u8> {
    match level {
        "debug" => Some(0),
        "info" => Some(1),
        "warn" | "warning" => Some(2),
        "error" => Some(3),
        "critical" => Some(4),
        "fatal" => Some(5),
        _ => None,
    }
}

fn jit_log_set_level_str(level: &str) {
    let n: u8 = jit_log_level_rank(level).unwrap_or(1);
    JIT_LOG_LEVEL.with(|l| l.set(n));
}

fn jit_log_setup_str(format: &str) {
    let n: u8 = match format {
        "json" => 1,
        "text" => 2,
        _ => 0,
    };
    JIT_LOG_FORMAT.with(|f| f.set(n));
}

fn jit_log_format_active() -> u8 {
    let explicit = JIT_LOG_FORMAT.with(|f| f.get());
    if explicit != 0 {
        return explicit;
    }
    use std::io::IsTerminal;
    if std::io::stderr().is_terminal() {
        2
    } else {
        1
    }
}

fn jit_log_json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// Mirrors AOT `unix_to_ymdhms` in RingCsvLogTimeCrypto.rs.
/// parity: guard tests/dev.rs::io_cli_terminal_and_time_match_interpreter_jit_and_aot
fn unix_to_ymdhms(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let mut days = secs / 86400;
    let time_of_day = (secs % 86400).unsigned_abs();
    let h = (time_of_day / 3600) as u32;
    let mi = ((time_of_day % 3600) / 60) as u32;
    let s = (time_of_day % 60) as u32;
    let mut year: i32 = 1970;
    loop {
        let dy = if is_leap(year) { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        year += 1;
    }
    let month_days: [i64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month: u32 = 1;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, (days + 1) as u32, h, mi, s)
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

fn jit_log_emit(level: &str, msg: &str, fields: &[JitLogField]) {
    if JIT_LOG_DISABLED.with(|d| d.get()) {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let line = if jit_log_format_active() == 2 {
        let secs = ts / 1000;
        let (y, mo, d, h, mi, s) = unix_to_ymdhms(secs);
        let level_tag = match level {
            "debug" => "DEBUG",
            "info" => "INFO",
            "warn" => "WARN",
            "error" => "ERROR",
            "critical" => "CRITICAL",
            "fatal" => "FATAL",
            _ => level,
        };
        let mut line = format!("[{level_tag}] {y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z | {msg}");
        for field in fields {
            line.push_str(&format!(" {}={}", field.key, field.value));
        }
        line
    } else {
        let mut fields_json = String::new();
        for field in fields {
            fields_json.push_str(",\"");
            fields_json.push_str(&jit_log_json_escape(&field.key));
            fields_json.push_str("\":");
            if matches!(field.kind.as_str(), "int" | "float" | "bool" | "counter") {
                fields_json.push_str(&field.value);
            } else {
                fields_json.push('"');
                fields_json.push_str(&jit_log_json_escape(&field.value));
                fields_json.push('"');
            }
        }
        let spans_json = JIT_LOG_SPANS.with(|s| {
            let spans = s.borrow();
            if spans.is_empty() {
                String::new()
            } else {
                let names = spans
                    .iter()
                    .map(|(_, name)| format!("\"{}\"", jit_log_json_escape(name)))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(",\"spans\":[{names}]")
            }
        });
        let trace = JIT_LOG_TRACE_ID.with(|t| t.borrow().clone());
        if trace.is_empty() {
            format!(
                "{{\"level\":\"{}\",\"body\":\"{}\",\"ts\":{}{}{}}}",
                level,
                jit_log_json_escape(msg),
                ts,
                fields_json,
                spans_json
            )
        } else {
            format!(
                "{{\"level\":\"{}\",\"body\":\"{}\",\"trace_id\":\"{}\",\"ts\":{}{}{}}}",
                level,
                jit_log_json_escape(msg),
                jit_log_json_escape(&trace),
                ts,
                fields_json,
                spans_json
            )
        }
    };
    Concurrency::with_runtime_mut(|rt| {
        rt.stderr.push_str(&line);
        rt.stderr.push('\n');
    });
}

extern "C" fn jet_jit_log_set_level(msg: i64) {
    jit_log_set_level_str(&clone_string(msg));
}

extern "C" fn jet_jit_log_setup(msg: i64) {
    jit_log_setup_str(&clone_string(msg));
}

extern "C" fn jet_jit_log_debug(msg: i64) {
    if JIT_LOG_LEVEL.with(|l| l.get()) <= 0 {
        jit_log_emit("debug", &clone_string(msg), &[]);
    }
}

extern "C" fn jet_jit_log_info(msg: i64) {
    if JIT_LOG_LEVEL.with(|l| l.get()) <= 1 {
        jit_log_emit("info", &clone_string(msg), &[]);
    }
}

extern "C" fn jet_jit_log_warn(msg: i64) {
    if JIT_LOG_LEVEL.with(|l| l.get()) <= 2 {
        jit_log_emit("warn", &clone_string(msg), &[]);
    }
}

extern "C" fn jet_jit_log_error(msg: i64) {
    if JIT_LOG_LEVEL.with(|l| l.get()) <= 3 {
        jit_log_emit("error", &clone_string(msg), &[]);
    }
}

extern "C" fn jet_jit_log_critical(msg: i64) {
    if JIT_LOG_LEVEL.with(|l| l.get()) <= 4 {
        jit_log_emit("critical", &clone_string(msg), &[]);
    }
}

extern "C" fn jet_jit_log_fatal(msg: i64) {
    if JIT_LOG_LEVEL.with(|l| l.get()) <= 5 {
        jit_log_emit("fatal", &clone_string(msg), &[]);
    }
    let _ = std::io::Write::flush(&mut std::io::stderr());
    jet_jit_process_exit(1);
}

extern "C" fn jet_jit_log_disable() {
    JIT_LOG_DISABLED.with(|d| d.set(true));
}

extern "C" fn jet_jit_log_flush() {
    let _ = std::io::Write::flush(&mut std::io::stderr());
}

extern "C" fn jet_jit_log_enabled(level: i64) -> i8 {
    if JIT_LOG_DISABLED.with(|d| d.get()) {
        return 0;
    }
    let Some(rank) = jit_log_level_rank(&clone_string(level)) else {
        return 0;
    };
    if JIT_LOG_LEVEL.with(|l| l.get()) <= rank {
        1
    } else {
        0
    }
}

extern "C" fn jet_jit_log_set_trace_id(msg: i64) {
    let id = clone_string(msg);
    JIT_LOG_TRACE_ID.with(|t| *t.borrow_mut() = id);
}

fn alloc_log_field(key: String, value: String, kind: &str, redacted: bool) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let rec = rt.heap.alloc_record(4);
        let k = rt.heap.alloc_string(key);
        let v = rt.heap.alloc_string(value);
        let kd = rt.heap.alloc_string(kind.to_string());
        let _ = rt.heap.record_set_string(rec, 0, k);
        let _ = rt.heap.record_set_string(rec, 1, v);
        let _ = rt.heap.record_set_string(rec, 2, kd);
        let _ = rt.heap.record_set_bool(rec, 3, redacted);
        rec
    })
}

extern "C" fn jet_jit_log_field(key: i64, value: i64) -> i64 {
    alloc_log_field(clone_string(key), clone_string(value), "string", false)
}

extern "C" fn jet_jit_log_int_field(key: i64, value: i64) -> i64 {
    alloc_log_field(clone_string(key), value.to_string(), "int", false)
}

extern "C" fn jet_jit_log_bool_field(key: i64, value: i8) -> i64 {
    alloc_log_field(
        clone_string(key),
        if value != 0 { "true" } else { "false" }.to_string(),
        "bool",
        false,
    )
}

extern "C" fn jet_jit_log_counter(name: i64, value: i64) -> i64 {
    alloc_log_field(
        format!("metric.counter.{}", clone_string(name)),
        value.to_string(),
        "counter",
        false,
    )
}

extern "C" fn jet_jit_log_span(name: i64) -> i64 {
    let id = JIT_LOG_NEXT_SPAN.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    let name_s = clone_string(name);
    Concurrency::with_runtime_mut(|rt| {
        let rec = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(rec, 0, id);
        let sid = rt.heap.alloc_string(name_s);
        let _ = rt.heap.record_set_string(rec, 1, sid);
        rec
    })
}

extern "C" fn jet_jit_log_enter(span: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let id = rt.heap.record_get_int(span, 0).unwrap_or(0);
        let name = rt
            .heap
            .record_get_string(span, 1)
            .and_then(|sid| rt.heap.clone_string(sid))
            .unwrap_or_default();
        JIT_LOG_SPANS.with(|s| s.borrow_mut().push((id, name)));
    });
}

extern "C" fn jet_jit_log_close(span: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let id = rt.heap.record_get_int(span, 0).unwrap_or(0);
        JIT_LOG_SPANS.with(|s| {
            let mut spans = s.borrow_mut();
            if let Some(pos) = spans.iter().rposition(|(sid, _)| *sid == id) {
                spans.remove(pos);
            }
        });
    });
}

fn read_log_fields(list: i64) -> Vec<JitLogField> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let rec = rt.heap.list_get_int(list, i).unwrap_or(0);
            let key = rt
                .heap
                .record_get_string(rec, 0)
                .and_then(|sid| rt.heap.clone_string(sid))
                .unwrap_or_default();
            let value = rt
                .heap
                .record_get_string(rec, 1)
                .and_then(|sid| rt.heap.clone_string(sid))
                .unwrap_or_default();
            let kind = rt
                .heap
                .record_get_string(rec, 2)
                .and_then(|sid| rt.heap.clone_string(sid))
                .unwrap_or_else(|| "string".to_string());
            out.push(JitLogField { key, value, kind });
        }
        out
    })
}

extern "C" fn jet_jit_log_info_fields(msg: i64, fields: i64) {
    if JIT_LOG_LEVEL.with(|l| l.get()) <= 1 {
        let fs = read_log_fields(fields);
        jit_log_emit("info", &clone_string(msg), &fs);
    }
}

// ── core.files and typed Path (mirrors jet_std_fs_* / jet_std_path_*) ────────

extern "C" fn jet_jit_fs_exists(path: i64) -> i8 {
    let p = clone_string(path);
    i8::from(std::path::Path::new(&p).exists())
}

extern "C" fn jet_jit_fs_read(path: i64) -> i64 {
    let p = clone_string(path);
    match std::fs::read_to_string(&p) {
        Ok(text) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text));
            result_ok(sid as u64)
        }
        Err(e) => result_err_msg(&format!("read {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_read_bytes(path: i64) -> i64 {
    let p = clone_string(path);
    match std::fs::read(&p) {
        Ok(bytes) => result_ok(alloc_byte_list(&bytes) as u64),
        Err(e) => result_err_msg(&format!("read_bytes {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_write(path: i64, text: i64) -> i64 {
    let p = clone_string(path);
    let t = clone_string(text);
    match std::fs::write(&p, t) {
        Ok(()) => result_ok(0),
        Err(e) => result_err_msg(&format!("write {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_create_dir(path: i64) -> i64 {
    let p = clone_string(path);
    match std::fs::create_dir_all(&p) {
        Ok(()) => result_ok(0),
        Err(e) => result_err_msg(&format!("create_dir {p}: {e}")),
    }
}

extern "C" fn jet_jit_path_home() -> i64 {
    path_record(path_kernel::jet_std_path_home())
}

extern "C" fn jet_jit_path_write_atomic(rec: i64, bytes: i64) -> i64 {
    let p = path_string_from_record(rec);
    let path_id = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(p));
    jet_jit_fs_write_atomic(path_id, bytes)
}

extern "C" fn jet_jit_path_from(path: i64) -> i64 {
    path_record(clone_string(path))
}

extern "C" fn jet_jit_path_join_handle(rec: i64, part: i64) -> i64 {
    let base = path_string_from_record(rec);
    let p = clone_string(part);
    path_record(path_kernel::jet_std_path_join(&base, &p))
}

extern "C" fn jet_jit_path_parent(rec: i64) -> i64 {
    let s = path_string_from_record(rec);
    match path_kernel::jet_std_path_parent_opt(&s) {
        None => 0,
        Some(parent) => path_record(parent).wrapping_add(1),
    }
}

extern "C" fn jet_jit_path_extension(rec: i64) -> i64 {
    let s = path_string_from_record(rec);
    option_string_bits(path_kernel::jet_std_path_extension_opt(&s))
}

extern "C" fn jet_jit_path_stem(rec: i64) -> i64 {
    let s = path_string_from_record(rec);
    option_string_bits(path_kernel::jet_std_path_stem_opt(&s))
}

extern "C" fn jet_jit_path_normalize(rec: i64) -> i64 {
    let s = path_string_from_record(rec);
    path_record(path_kernel::jet_std_path_normalize(&s))
}

extern "C" fn jet_jit_path_to_string(rec: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(path_string_from_record(rec)))
}

/// D-PATHFS1 / AOT `jet_path_walk` → `Vec<JetPath>` (bare list handle).
/// Must not wrap in `result_ok_bits` — callers treat the return as `List[Path]`
/// (`paths.len()` → `jet_jit_list_len`); a Result slot index panics across FFI
/// (`jit list len: bad handle` → non-unwinding abort).
extern "C" fn jet_jit_path_walk(rec: i64) -> i64 {
    let root_s = path_string_from_record(rec);
    let result_paths = path_kernel::jet_std_path_walk(&root_s);
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for path in result_paths {
            let rec = rt.heap.alloc_record(1);
            let sid = rt.heap.alloc_string(path);
            let _ = rt.heap.record_set_string(rec, 0, sid);
            let _ = rt.heap.list_push_int(list, rec);
        }
        list
    })
}

extern "C" fn jet_jit_fs_list_dir(path: i64) -> i64 {
    let p = clone_string(path);
    let rd = match std::fs::read_dir(&p) {
        Ok(rd) => rd,
        Err(e) => return result_err_msg(&format!("list_dir {p}: {e}")),
    };
    let mut entries = Vec::new();
    for entry in rd {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => return result_err_msg(&format!("list_dir {p}: {e}")),
        };
        let name = entry.file_name().to_string_lossy().to_string();
        let full_path = std::path::Path::new(&p)
            .join(&name)
            .to_string_lossy()
            .to_string();
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        entries.push((name, full_path, is_dir));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for (name, full_path, is_dir) in entries {
            let rec = rt.heap.alloc_record(3);
            let n = rt.heap.alloc_string(name);
            let fp = rt.heap.alloc_string(full_path);
            let _ = rt.heap.record_set_string(rec, 0, n);
            let _ = rt.heap.record_set_string(rec, 1, fp);
            let _ = rt.heap.record_set_bool(rec, 2, is_dir);
            let _ = rt.heap.list_push_int(list, rec);
        }
        rt.results.push(super::JitResultValue {
            ok: true,
            bits: list as u64,
        });
        rt.results.len() as i64
    })
}

fn system_time_ms(t: std::time::SystemTime) -> Option<i64> {
    t.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

fn glob_match(pattern: &str, text: &str) -> bool {
    fn inner(p: &[u8], t: &[u8]) -> bool {
        if p.is_empty() {
            return t.is_empty();
        }
        match p[0] {
            b'*' => inner(&p[1..], t) || (!t.is_empty() && inner(p, &t[1..])),
            b'?' => !t.is_empty() && inner(&p[1..], &t[1..]),
            c => !t.is_empty() && c == t[0] && inner(&p[1..], &t[1..]),
        }
    }
    inner(pattern.as_bytes(), text.as_bytes())
}

fn walk_entries(root: &std::path::Path) -> Result<Vec<(String, String, bool, i64)>, String> {
    let mut out = Vec::new();
    fn walk_dir(
        root: &std::path::Path,
        dir: &std::path::Path,
        depth: i64,
        out: &mut Vec<(String, String, bool, i64)>,
    ) -> Result<(), String> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            entries.push(entry.map_err(|e| e.to_string())?);
        }
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let p = entry.path();
            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            let relative = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .to_string();
            out.push((p.to_string_lossy().to_string(), relative, is_dir, depth));
            if is_dir {
                walk_dir(root, &p, depth + 1, out)?;
            }
        }
        Ok(())
    }
    walk_dir(root, root, 0, &mut out)?;
    Ok(out)
}

fn path_record(path: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let rec = rt.heap.alloc_record(1);
        let sid = rt.heap.alloc_string(path);
        let _ = rt.heap.record_set_string(rec, 0, sid);
        rec
    })
}

fn path_string_from_record(rec: i64) -> String {
    Concurrency::with_runtime_mut(|rt| {
        let sid = rt.heap.record_get_string(rec, 0).unwrap_or(0);
        rt.heap.clone_string(sid).unwrap_or_default()
    })
}

pub(crate) fn show_path(rt: &crate::JitRuntime, rec: i64) -> String {
    rt.heap.record_clone_string(rec, 0).unwrap_or_default()
}

fn option_string_bits(s: Option<String>) -> i64 {
    match s {
        None => 0,
        Some(v) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(v));
            sid.wrapping_add(1)
        }
    }
}

pub(crate) type JitEnvEntries = Vec<(std::ffi::OsString, std::ffi::OsString)>;

fn jit_env_table() -> &'static std::sync::RwLock<JitEnvEntries> {
    static TABLE: std::sync::OnceLock<std::sync::RwLock<JitEnvEntries>> =
        std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut entries: JitEnvEntries = Vec::new();
        for (name, value) in std::env::vars_os() {
            if let Some(old) = entries
                .iter()
                .position(|(candidate, _)| jit_env_key_eq(candidate.as_os_str(), name.as_os_str()))
            {
                entries.remove(old);
            }
            entries.push((name, value));
        }
        std::sync::RwLock::new(entries)
    })
}

fn jit_env_read() -> std::sync::RwLockReadGuard<'static, JitEnvEntries> {
    jit_env_table()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn jit_env_write() -> std::sync::RwLockWriteGuard<'static, JitEnvEntries> {
    jit_env_table()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(unix)]
fn jit_env_key_cmp(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> std::cmp::Ordering {
    use std::os::unix::ffi::OsStrExt;
    left.as_bytes().cmp(right.as_bytes())
}

// JET_VETTED_UNSAFE_BEGIN: jit_env_windows
#[cfg(windows)]
fn jit_env_key_cmp(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> std::cmp::Ordering {
    use std::os::windows::ffi::OsStrExt;
    extern "system" {
        fn CompareStringOrdinal(
            left: *const u16,
            left_len: i32,
            right: *const u16,
            right_len: i32,
            ignore_case: i32,
        ) -> i32;
    }
    let left: Vec<u16> = left.encode_wide().collect();
    let right: Vec<u16> = right.encode_wide().collect();
    let (Ok(left_len), Ok(right_len)) = (i32::try_from(left.len()), i32::try_from(right.len()))
    else {
        return left.cmp(&right);
    };
    let result =
        unsafe { CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) };
    match result {
        1 => std::cmp::Ordering::Less,
        2 => std::cmp::Ordering::Equal,
        3 => std::cmp::Ordering::Greater,
        _ => left.cmp(&right),
    }
}
// JET_VETTED_UNSAFE_END: jit_env_windows

#[cfg(not(any(unix, windows)))]
fn jit_env_key_cmp(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> std::cmp::Ordering {
    left.cmp(right)
}

pub(crate) fn jit_env_key_eq(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> bool {
    jit_env_key_cmp(left, right) == std::cmp::Ordering::Equal
}

pub(crate) fn jit_env_validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.contains('\0') || name.contains('=') {
        Err("invalid environment variable name")
    } else {
        Ok(())
    }
}

pub(crate) fn jit_env_validate_value(value: &str) -> Result<(), &'static str> {
    if value.contains('\0') {
        Err("invalid environment variable value")
    } else {
        Ok(())
    }
}

pub(crate) fn jit_env_snapshot_raw() -> JitEnvEntries {
    jit_env_read().clone()
}

fn jet_temp_path(prefix: &str) -> String {
    let clean: String = prefix
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir()
        .join(format!("{}_{}_{}", clean, std::process::id(), nanos))
        .to_string_lossy()
        .to_string()
}

extern "C" fn jet_jit_fs_remove(path: i64) -> i64 {
    let p = clone_string(path);
    let res = std::fs::remove_file(&p).or_else(|_| std::fs::remove_dir(&p));
    match res {
        Ok(()) => result_ok(0),
        Err(e) => result_err_msg(&format!("remove {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_remove_all(path: i64) -> i64 {
    let p = clone_string(path);
    let path = std::path::Path::new(&p);
    let res = if path.is_dir() {
        std::fs::remove_dir_all(&p)
    } else {
        std::fs::remove_file(&p)
    };
    match res {
        Ok(()) => result_ok(0),
        Err(e) => result_err_msg(&format!("remove_all {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_stat(path: i64) -> i64 {
    let p = clone_string(path);
    let meta = match std::fs::symlink_metadata(&p) {
        Ok(m) => m,
        Err(e) => return result_err_msg(&format!("stat {p}: {e}")),
    };
    let ft = meta.file_type();
    let kind = if ft.is_symlink() {
        "symlink"
    } else if ft.is_dir() {
        "dir"
    } else if ft.is_file() {
        "file"
    } else {
        "other"
    };
    let rec = Concurrency::with_runtime_mut(|rt| {
        let rec = rt.heap.alloc_record(8);
        let _ = rt.heap.record_set_int(rec, 0, meta.len() as i64);
        let _ = rt
            .heap
            .record_set_int(rec, 1, meta.modified().ok().and_then(system_time_ms).unwrap_or(0));
        let _ = rt
            .heap
            .record_set_int(rec, 2, meta.created().ok().and_then(system_time_ms).unwrap_or(0));
        let _ = rt
            .heap
            .record_set_bool(rec, 3, meta.permissions().readonly());
        let _ = rt.heap.record_set_bool(rec, 4, ft.is_file());
        let _ = rt.heap.record_set_bool(rec, 5, ft.is_dir());
        let _ = rt.heap.record_set_bool(rec, 6, ft.is_symlink());
        let kid = rt.heap.alloc_string(kind.to_string());
        let _ = rt.heap.record_set_string(rec, 7, kid);
        rec
    });
    result_ok(rec as u64)
}

extern "C" fn jet_jit_fs_read_at(path: i64, offset: i64, len: i64) -> i64 {
    use std::io::{Read, Seek, SeekFrom};
    let p = clone_string(path);
    let mut f = match std::fs::File::open(&p) {
        Ok(f) => f,
        Err(e) => return result_err_msg(&format!("read_at {p}: {e}")),
    };
    if let Err(e) = f.seek(SeekFrom::Start(offset.max(0) as u64)) {
        return result_err_msg(&format!("read_at {p}: {e}"));
    }
    let mut buf = vec![0u8; len.max(0) as usize];
    let n = match f.read(&mut buf) {
        Ok(n) => n,
        Err(e) => return result_err_msg(&format!("read_at {p}: {e}")),
    };
    buf.truncate(n);
    result_ok(alloc_byte_list(&buf) as u64)
}

extern "C" fn jet_jit_fs_write_at(path: i64, offset: i64, bytes: i64) -> i64 {
    use std::io::{Seek, SeekFrom, Write};
    let p = clone_string(path);
    let data = clone_bytes(bytes);
    let mut f = match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&p)
    {
        Ok(f) => f,
        Err(e) => return result_err_msg(&format!("write_at {p}: {e}")),
    };
    if let Err(e) = f.seek(SeekFrom::Start(offset.max(0) as u64)) {
        return result_err_msg(&format!("write_at {p}: {e}"));
    }
    match f.write_all(&data) {
        Ok(()) => result_ok(0),
        Err(e) => result_err_msg(&format!("write_at {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_fsync(path: i64) -> i64 {
    let p = clone_string(path);
    match std::fs::OpenOptions::new()
        .read(true)
        .open(&p)
        .and_then(|f| f.sync_all())
    {
        Ok(()) => result_ok(0),
        Err(e) => result_err_msg(&format!("fsync {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_write_atomic(path: i64, bytes: i64) -> i64 {
    let p = clone_string(path);
    let data = clone_bytes(bytes);
    let path = std::path::Path::new(&p);
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        Some(_) => std::path::Path::new("."),
        None => return result_err_msg(&format!("write_atomic {p}: path has no parent")),
    };
    let tmp = parent.join(format!(
        ".jet_atomic_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    if let Err(e) = std::fs::write(&tmp, &data) {
        return result_err_msg(&format!("write_atomic {p}: {e}"));
    }
    match std::fs::rename(&tmp, path) {
        Ok(()) => result_ok(0),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            result_err_msg(&format!("write_atomic {p}: {e}"))
        }
    }
}

extern "C" fn jet_jit_fs_walk(path: i64) -> i64 {
    let p = clone_string(path);
    let entries = match walk_entries(std::path::Path::new(&p)) {
        Ok(e) => e,
        Err(e) => return result_err_msg(&format!("walk {p}: {e}")),
    };
    let list = Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for (path, relative, is_dir, depth) in entries {
            let rec = rt.heap.alloc_record(4);
            let ps = rt.heap.alloc_string(path);
            let rs = rt.heap.alloc_string(relative);
            let _ = rt.heap.record_set_string(rec, 0, ps);
            let _ = rt.heap.record_set_string(rec, 1, rs);
            let _ = rt.heap.record_set_bool(rec, 2, is_dir);
            let _ = rt.heap.record_set_int(rec, 3, depth);
            let _ = rt.heap.list_push_int(list, rec);
        }
        list
    });
    result_ok(list as u64)
}

extern "C" fn jet_jit_fs_glob(pattern: i64) -> i64 {
    let pat = clone_string(pattern);
    let split = pat.find(['*', '?']).unwrap_or(pat.len());
    let base = pat[..split]
        .rsplit_once(std::path::MAIN_SEPARATOR)
        .map(|(dir, _)| if dir.is_empty() { "." } else { dir })
        .unwrap_or(".");
    let entries = match walk_entries(std::path::Path::new(base)) {
        Ok(e) => e,
        Err(e) => return result_err_msg(&format!("glob {pat}: {e}")),
    };
    let mut matches: Vec<String> = entries
        .into_iter()
        .map(|(path, _, _, _)| path)
        .filter(|path| glob_match(&pat, path))
        .collect();
    matches.sort();
    let list = Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for path in matches {
            let sid = rt.heap.alloc_string(path);
            let _ = rt.heap.list_push_int(list, sid);
        }
        list
    });
    result_ok(list as u64)
}

extern "C" fn jet_jit_fs_symlink(from: i64, to: i64) -> i64 {
    let src = clone_string(from);
    let dst = clone_string(to);
    #[cfg(unix)]
    let res = std::os::unix::fs::symlink(&src, &dst);
    #[cfg(windows)]
    let res = {
        let meta = std::fs::metadata(&src);
        match meta {
            Ok(m) if m.is_dir() => std::os::windows::fs::symlink_dir(&src, &dst),
            _ => std::os::windows::fs::symlink_file(&src, &dst),
        }
    };
    match res {
        Ok(()) => result_ok(0),
        Err(e) => result_err_msg(&format!("symlink {dst}: {e}")),
    }
}

extern "C" fn jet_jit_fs_read_link(path: i64) -> i64 {
    let p = clone_string(path);
    match std::fs::read_link(&p) {
        Ok(target) => {
            let sid = Concurrency::with_runtime_mut(|rt| {
                rt.heap
                    .alloc_string(target.to_string_lossy().to_string())
            });
            result_ok(sid as u64)
        }
        Err(e) => result_err_msg(&format!("read_link {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_hard_link(from: i64, to: i64) -> i64 {
    let src = clone_string(from);
    let dst = clone_string(to);
    match std::fs::hard_link(&src, &dst) {
        Ok(()) => result_ok(0),
        Err(e) => result_err_msg(&format!("hard_link {dst}: {e}")),
    }
}

extern "C" fn jet_jit_fs_canonicalize(path: i64) -> i64 {
    let p = clone_string(path);
    match std::fs::canonicalize(&p) {
        Ok(abs) => {
            let sid = Concurrency::with_runtime_mut(|rt| {
                rt.heap.alloc_string(abs.to_string_lossy().to_string())
            });
            result_ok(sid as u64)
        }
        Err(e) => result_err_msg(&format!("canonicalize {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_absolute(path: i64) -> i64 {
    let p = clone_string(path);
    let path = std::path::Path::new(&p);
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(path),
            Err(e) => return result_err_msg(&format!("absolute {p}: {e}")),
        }
    };
    let sid = Concurrency::with_runtime_mut(|rt| {
        rt.heap.alloc_string(abs.to_string_lossy().to_string())
    });
    result_ok(sid as u64)
}

extern "C" fn jet_jit_fs_copy_dir(from: i64, to: i64) -> i64 {
    let src = clone_string(from);
    let dst = clone_string(to);
    fn copy_tree(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
        std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
        for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            let ft = entry.file_type().map_err(|e| e.to_string())?;
            if ft.is_dir() {
                copy_tree(&src_path, &dst_path)?;
            } else if ft.is_file() {
                std::fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    match copy_tree(std::path::Path::new(&src), std::path::Path::new(&dst)) {
        Ok(()) => result_ok(0),
        Err(e) => result_err_msg(&format!("copy_dir {src}: {e}")),
    }
}

extern "C" fn jet_jit_fs_temp_dir(prefix: i64) -> i64 {
    let pref = clone_string(prefix);
    let path = jet_temp_path(&pref);
    match std::fs::create_dir(&path) {
        Ok(()) => result_ok(path_record(path) as u64),
        Err(e) => result_err_msg(&format!("temp_dir {path}: {e}")),
    }
}

extern "C" fn jet_jit_fs_temp_file(prefix: i64) -> i64 {
    let pref = clone_string(prefix);
    let path = jet_temp_path(&pref);
    match std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
    {
        Ok(_) => result_ok(path_record(path) as u64),
        Err(e) => result_err_msg(&format!("temp_file {path}: {e}")),
    }
}

extern "C" fn jet_jit_fs_lock(path: i64) -> i64 {
    let p = clone_string(path);
    match std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&p)
    {
        Ok(_) => result_ok(path_record(p) as u64),
        Err(e) => result_err_msg(&format!("lock {p}: {e}")),
    }
}

// ── core.math (mirrors jet_std_math_* / f64 methods in Process.rs emit) ───────

extern "C" fn jet_jit_math_sin(x: f64) -> f64 {
    x.sin()
}
extern "C" fn jet_jit_math_cos(x: f64) -> f64 {
    x.cos()
}
extern "C" fn jet_jit_math_exp(x: f64) -> f64 {
    x.exp()
}
extern "C" fn jet_jit_math_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}
extern "C" fn jet_jit_math_hypot(a: f64, b: f64) -> f64 {
    a.hypot(b)
}
extern "C" fn jet_jit_math_lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
extern "C" fn jet_jit_math_degrees(x: f64) -> f64 {
    x.to_degrees()
}
extern "C" fn jet_jit_math_radians(x: f64) -> f64 {
    x.to_radians()
}
extern "C" fn jet_jit_math_is_finite(x: f64) -> i8 {
    i8::from(x.is_finite())
}
extern "C" fn jet_jit_math_sign(x: f64) -> i64 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}

/// Packed Option<i64> ABI: `0` = None, else `bits.wrapping_add(1)`.
extern "C" fn jet_jit_math_checked_add(a: i64, b: i64) -> i64 {
    match a.checked_add(b) {
        Some(v) => v.wrapping_add(1),
        None => 0,
    }
}

extern "C" fn jet_jit_math_saturating_add(a: i64, b: i64) -> i64 {
    a.saturating_add(b)
}
extern "C" fn jet_jit_math_wrapping_add(a: i64, b: i64) -> i64 {
    a.wrapping_add(b)
}

/// Mirrors `jet_std_math_int_pow`.
extern "C" fn jet_jit_math_int_pow(base: i64, exp: i64) -> i64 {
    if exp < 0 {
        return 0;
    }
    base.saturating_pow(exp as u32)
}

/// Mirrors `jet_std_math_gcd`.
extern "C" fn jet_jit_math_gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}

/// Mirrors `jet_std_math_lcm`.
extern "C" fn jet_jit_math_lcm(a: i64, b: i64) -> i64 {
    if a == 0 || b == 0 {
        0
    } else {
        (a / jet_jit_math_gcd(a, b)).saturating_mul(b).abs()
    }
}

extern "C" fn jet_jit_math_sqrt(x: f64) -> f64 {
    x.sqrt()
}
extern "C" fn jet_jit_math_sqrt_f32(x: f64) -> f64 {
    ((x as f32).sqrt()) as f64
}
extern "C" fn jet_jit_math_pow(base: f64, exp: f64) -> f64 {
    base.powf(exp)
}
extern "C" fn jet_jit_math_pow_f32(base: f64, exp: f64) -> f64 {
    ((base as f32).powf(exp as f32)) as f64
}
extern "C" fn jet_jit_math_floor(x: f64) -> f64 {
    x.floor()
}
extern "C" fn jet_jit_math_floor_f32(x: f64) -> f64 {
    ((x as f32).floor()) as f64
}
extern "C" fn jet_jit_math_ceil(x: f64) -> f64 {
    x.ceil()
}
extern "C" fn jet_jit_math_ceil_f32(x: f64) -> f64 {
    ((x as f32).ceil()) as f64
}

// ── core.env / core.process (mirrors jet_std_env_get / jet_std_process_exit) ─

/// Option ABI: `0` = None, else string-handle+1 (same as list_get_opt).
extern "C" fn jet_jit_env_get(name: i64) -> i64 {
    let key = clone_string(name);
    let key = std::ffi::OsStr::new(&key);
    let value = jit_env_read()
        .iter()
        .find(|(candidate, _)| jit_env_key_eq(candidate.as_os_str(), key))
        .and_then(|(_, value)| value.to_str().map(str::to_string));
    match value {
        Some(value) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value));
            sid.wrapping_add(1)
        }
        None => 0,
    }
}

extern "C" fn jet_jit_env_set(name: i64, value: i64) -> i64 {
    let key = clone_string(name);
    let val = clone_string(value);
    if let Err(error) = jit_env_validate_name(&key) {
        return result_err_msg(error);
    }
    if let Err(error) = jit_env_validate_value(&val) {
        return result_err_msg(error);
    }
    let key = std::ffi::OsString::from(key);
    let mut entries = jit_env_write();
    if let Some(old) = entries
        .iter()
        .position(|(candidate, _)| jit_env_key_eq(candidate.as_os_str(), key.as_os_str()))
    {
        entries.remove(old);
    }
    entries.push((key, std::ffi::OsString::from(val)));
    result_ok(0)
}

extern "C" fn jet_jit_env_unset(name: i64) -> i64 {
    let key = clone_string(name);
    if let Err(error) = jit_env_validate_name(&key) {
        return result_err_msg(error);
    }
    let key = std::ffi::OsStr::new(&key);
    let mut entries = jit_env_write();
    let existed = entries
        .iter()
        .position(|(candidate, _)| jit_env_key_eq(candidate.as_os_str(), key))
        .map(|old| entries.remove(old))
        .is_some();
    result_ok(u64::from(existed))
}

extern "C" fn jet_jit_env_vars() -> i64 {
    let entries = jit_env_read();
    let mut names = Vec::with_capacity(entries.len());
    for (name, value) in entries.iter() {
        let Some(decoded) = name.to_str() else {
            return result_err_msg(
                "environment contains a name or value that is not valid Unicode",
            );
        };
        if value.to_str().is_none() {
            return result_err_msg(
                "environment contains a name or value that is not valid Unicode",
            );
        }
        names.push((name.clone(), decoded.to_string()));
    }
    names.sort_by(|(left, _), (right, _)| {
        let folded = jit_env_key_cmp(left.as_os_str(), right.as_os_str());
        if folded != std::cmp::Ordering::Equal {
            return folded;
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            return left.encode_wide().cmp(right.encode_wide());
        }
        #[cfg(not(windows))]
        std::cmp::Ordering::Equal
    });
    let list = Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for (_, name) in names {
            let sid = rt.heap.alloc_string(name);
            let _ = rt.heap.list_push_int(list, sid);
        }
        list
    });
    result_ok(list as u64)
}

extern "C" fn jet_jit_io_input(has_prompt: i8, prompt: i64) -> i64 {
    use std::io::Write;
    if has_prompt != 0 {
        let p = clone_string(prompt);
        print!("{p}");
        if let Err(e) = std::io::stdout().flush() {
            return result_err_msg(&format!("flush stdout: {e}"));
        }
    }
    let mut s = String::new();
    if let Err(e) = std::io::stdin().read_line(&mut s) {
        return result_err_msg(&format!("read stdin: {e}"));
    }
    while s.ends_with('\n') || s.ends_with('\r') {
        s.pop();
    }
    let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
    result_ok(sid as u64)
}

extern "C" fn jet_jit_process_exit(code: i64) {
    // Soft exit: set the code + trap so `resident_invoke` returns `Ran` with
    // that exit status. Never terminate the resident host — that would kill
    // the resident/test process (three-way battery, `jet serve`, …).
    Concurrency::with_runtime_mut(|rt| {
        rt.exit_code = Some(code as i32);
        rt.set_trap("__jet_process_exit__");
    });
}

// D-LIB-CALLGRANT1=A: the host only converts heap handles into the exact
// Prelude loader inputs. Identity/effect checks and native mapping stay in
// `jet_jit::Mod`, which includes the shared Mod Prelude.
extern "C" fn jet_jit_mod_load(path: i64, grant: i64) -> i64 {
    let path = clone_string(path);
    let read = Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.record_get_int(grant, 0)?;
        let len = rt.heap.list_len(list)?;
        (0..len)
            .map(|index| rt.heap.list_get_string(list, index))
            .collect::<Option<Vec<_>>>()
    });
    let Some(read) = read else {
        return result_err_msg("Mod.load expects a ModGrant.{ read: [String] }");
    };
    match crate::Mod::load(path, read) {
        Ok(handle) => result_ok(handle as u64),
        Err(error) => result_err_msg(&error),
    }
}

extern "C" fn jet_jit_mod_on_tick(module: i64, dt: i64) -> i64 {
    match crate::Mod::on_tick(module, dt) {
        Ok(value) => result_ok(value as u64),
        Err(error) => result_err_msg(&error),
    }
}

host_fns! {
    struct CoreHostFns;
    register: register_core_host_symbols;
    declare: declare_core_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::{Linkage, Module};
        let cc = module.target_config().default_call_conv;
        let mut sig_str = Signature::new(cc);
        sig_str.returns.push(AbiParam::new(types::I64));
        let mut sig_i64 = Signature::new(cc);
        sig_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_f64 = Signature::new(cc);
        sig_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_void = Signature::new(cc);
        let mut sig_void_str = Signature::new(cc);
        sig_void_str.params.push(AbiParam::new(types::I64));
        let mut sig_str_str_str = Signature::new(cc);
        sig_str_str_str.params.push(AbiParam::new(types::I64));
        sig_str_str_str.params.push(AbiParam::new(types::I64));
        sig_str_str_str.returns.push(AbiParam::new(types::I64));
        let mut sig_str_i64_str = Signature::new(cc);
        sig_str_i64_str.params.push(AbiParam::new(types::I64));
        sig_str_i64_str.params.push(AbiParam::new(types::I64));
        sig_str_i64_str.returns.push(AbiParam::new(types::I64));
        let mut sig_str_i8_str = Signature::new(cc);
        sig_str_i8_str.params.push(AbiParam::new(types::I64));
        sig_str_i8_str.params.push(AbiParam::new(types::I8));
        sig_str_i8_str.returns.push(AbiParam::new(types::I64));
        let mut sig_void_i64 = Signature::new(cc);
        sig_void_i64.params.push(AbiParam::new(types::I64));
        let mut sig_void_i64_i64 = Signature::new(cc);
        sig_void_i64_i64.params.push(AbiParam::new(types::I64));
        sig_void_i64_i64.params.push(AbiParam::new(types::I64));
        let mut sig_unary_i64 = Signature::new(cc);
        sig_unary_i64.params.push(AbiParam::new(types::I64));
        sig_unary_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_i64_i8 = Signature::new(cc);
        sig_i64_i8.params.push(AbiParam::new(types::I64));
        sig_i64_i8.returns.push(AbiParam::new(types::I8));
        let mut sig_f64_f64 = Signature::new(cc);
        sig_f64_f64.params.push(AbiParam::new(types::F64));
        sig_f64_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_f64_f64_f64 = Signature::new(cc);
        sig_f64_f64_f64.params.push(AbiParam::new(types::F64));
        sig_f64_f64_f64.params.push(AbiParam::new(types::F64));
        sig_f64_f64_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_lerp = Signature::new(cc);
        sig_lerp.params.push(AbiParam::new(types::F64));
        sig_lerp.params.push(AbiParam::new(types::F64));
        sig_lerp.params.push(AbiParam::new(types::F64));
        sig_lerp.returns.push(AbiParam::new(types::F64));
        let mut sig_f64_i8 = Signature::new(cc);
        sig_f64_i8.params.push(AbiParam::new(types::F64));
        sig_f64_i8.returns.push(AbiParam::new(types::I8));
        let mut sig_f64_i64 = Signature::new(cc);
        sig_f64_i64.params.push(AbiParam::new(types::F64));
        sig_f64_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_i64_i64_i64 = Signature::new(cc);
        sig_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_i64_i64_i64_i64 = Signature::new(cc);
        sig_i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_i8_i64_i64 = Signature::new(cc);
        sig_i8_i64_i64.params.push(AbiParam::new(types::I8));
        sig_i8_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i8_i64_i64.returns.push(AbiParam::new(types::I64));


    }
    os_name: "jet_jit_os_name" => jet_jit_os_name: sig_str;
    os_family: "jet_jit_os_family" => jet_jit_os_family: sig_str;
    os_arch: "jet_jit_os_arch" => jet_jit_os_arch: sig_str;
    os_cpu_count: "jet_jit_os_cpu_count" => jet_jit_os_cpu_count: sig_i64;
    os_temp_dir: "jet_jit_os_temp_dir" => jet_jit_os_temp_dir: sig_str;
    os_executable: "jet_jit_os_executable" => jet_jit_os_executable: sig_str;
    os_pid: "jet_jit_os_pid" => jet_jit_os_pid: sig_i64;
    os_hostname: "jet_jit_os_hostname" => jet_jit_os_hostname: sig_str;
    os_username: "jet_jit_os_username" => jet_jit_os_username: sig_str;
    os_release: "jet_jit_os_release" => jet_jit_os_release: sig_str;
    os_version: "jet_jit_os_version" => jet_jit_os_version: sig_str;
    os_getppid: "jet_jit_os_getppid" => jet_jit_os_getppid: sig_i64;
    os_getuid: "jet_jit_os_getuid" => jet_jit_os_getuid: sig_i64;
    os_geteuid: "jet_jit_os_geteuid" => jet_jit_os_geteuid: sig_i64;
    os_getgid: "jet_jit_os_getgid" => jet_jit_os_getgid: sig_i64;
    os_getegid: "jet_jit_os_getegid" => jet_jit_os_getegid: sig_i64;
    os_getpgrp: "jet_jit_os_getpgrp" => jet_jit_os_getpgrp: sig_i64;
    os_getgroups: "jet_jit_os_getgroups" => jet_jit_os_getgroups: sig_i64;
    os_uptime: "jet_jit_os_uptime" => jet_jit_os_uptime: sig_f64;
    os_loadavg: "jet_jit_os_loadavg" => jet_jit_os_loadavg: sig_i64;
    os_times: "jet_jit_os_times" => jet_jit_os_times: sig_i64;
    os_success: "jet_jit_os_success" => jet_jit_os_success: sig_i64_i8;
    os_exitcode: "jet_jit_os_exitcode" => jet_jit_os_exitcode: sig_unary_i64;
    os_expand: "jet_jit_os_expand" => jet_jit_os_expand: sig_unary_i64;
    os_getpgid: "jet_jit_os_getpgid" => jet_jit_os_getpgid: sig_unary_i64;
    os_getsid: "jet_jit_os_getsid" => jet_jit_os_getsid: sig_unary_i64;
    os_setpgid: "jet_jit_os_setpgid" => jet_jit_os_setpgid: sig_i64_i64_i64;
    os_setpgrp: "jet_jit_os_setpgrp" => jet_jit_os_setpgrp: sig_i64;
    os_umask: "jet_jit_os_umask" => jet_jit_os_umask: sig_unary_i64;
    os_sync: "jet_jit_os_sync" => jet_jit_os_sync: sig_void;
    os_getpriority: "jet_jit_os_getpriority" => jet_jit_os_getpriority: sig_unary_i64;
    os_setpriority: "jet_jit_os_setpriority" => jet_jit_os_setpriority: sig_i64_i64_i64;
    os_kill: "jet_jit_os_kill" => jet_jit_os_kill: sig_i64_i64_i64;
    os_pipe: "jet_jit_os_pipe" => jet_jit_os_pipe: sig_i64;
    os_close_fd: "jet_jit_os_close_fd" => jet_jit_os_close_fd: sig_void_i64;
    os_mkfifo: "jet_jit_os_mkfifo" => jet_jit_os_mkfifo: sig_i64_i64_i64;
    os_fork: "jet_jit_os_fork" => jet_jit_os_fork: sig_i64;
    os_setuid: "jet_jit_os_setuid" => jet_jit_os_setuid: sig_unary_i64;
    os_setgid: "jet_jit_os_setgid" => jet_jit_os_setgid: sig_unary_i64;
    os_setsid: "jet_jit_os_setsid" => jet_jit_os_setsid: sig_i64;
    os_initgroups: "jet_jit_os_initgroups" => jet_jit_os_initgroups: sig_i64_i64_i64;
    os_wait: "jet_jit_os_wait" => jet_jit_os_wait: sig_i64;
    os_waitpid: "jet_jit_os_waitpid" => jet_jit_os_waitpid: sig_i64_i64_i64;
    os_utime: "jet_jit_os_utime" => jet_jit_os_utime: sig_i64_i64_i64_i64;
    os_on_interrupt: "jet_jit_os_on_interrupt" => jet_jit_os_on_interrupt: sig_void_i64;
    os_atexit: "jet_jit_os_atexit" => jet_jit_os_atexit: sig_unary_i64;
    os_stop: "jet_jit_os_stop" => jet_jit_os_stop: sig_void_i64;
    log_set_level: "jet_jit_log_set_level" => jet_jit_log_set_level: sig_void_str;
    log_setup: "jet_jit_log_setup" => jet_jit_log_setup: sig_void_str;
    log_debug: "jet_jit_log_debug" => jet_jit_log_debug: sig_void_str;
    log_info: "jet_jit_log_info" => jet_jit_log_info: sig_void_str;
    log_warn: "jet_jit_log_warn" => jet_jit_log_warn: sig_void_str;
    log_error: "jet_jit_log_error" => jet_jit_log_error: sig_void_str;
    log_critical: "jet_jit_log_critical" => jet_jit_log_critical: sig_void_str;
    log_fatal: "jet_jit_log_fatal" => jet_jit_log_fatal: sig_void_str;
    log_disable: "jet_jit_log_disable" => jet_jit_log_disable: sig_void;
    log_flush: "jet_jit_log_flush" => jet_jit_log_flush: sig_void;
    log_enabled: "jet_jit_log_enabled" => jet_jit_log_enabled: sig_i64_i8;
    log_set_trace_id: "jet_jit_log_set_trace_id" => jet_jit_log_set_trace_id: sig_void_str;
    log_field: "jet_jit_log_field" => jet_jit_log_field: sig_str_str_str;
    log_int_field: "jet_jit_log_int_field" => jet_jit_log_int_field: sig_str_i64_str;
    log_bool_field: "jet_jit_log_bool_field" => jet_jit_log_bool_field: sig_str_i8_str;
    log_counter: "jet_jit_log_counter" => jet_jit_log_counter: sig_str_i64_str;
    log_span: "jet_jit_log_span" => jet_jit_log_span: sig_unary_i64;
    log_enter: "jet_jit_log_enter" => jet_jit_log_enter: sig_void_i64;
    log_close: "jet_jit_log_close" => jet_jit_log_close: sig_void_i64;
    log_info_fields: "jet_jit_log_info_fields" => jet_jit_log_info_fields: sig_void_i64_i64;
    fs_exists: "jet_jit_fs_exists" => jet_jit_fs_exists: sig_i64_i8;
    fs_read: "jet_jit_fs_read" => jet_jit_fs_read: sig_unary_i64;
    fs_read_bytes: "jet_jit_fs_read_bytes" => jet_jit_fs_read_bytes: sig_unary_i64;
    fs_write: "jet_jit_fs_write" => jet_jit_fs_write: sig_i64_i64_i64;
    fs_create_dir: "jet_jit_fs_create_dir" => jet_jit_fs_create_dir: sig_unary_i64;
    fs_list_dir: "jet_jit_fs_list_dir" => jet_jit_fs_list_dir: sig_unary_i64;
    fs_remove_all: "jet_jit_fs_remove_all" => jet_jit_fs_remove_all: sig_unary_i64;
    fs_remove: "jet_jit_fs_remove" => jet_jit_fs_remove: sig_unary_i64;
    fs_stat: "jet_jit_fs_stat" => jet_jit_fs_stat: sig_unary_i64;
    fs_read_at: "jet_jit_fs_read_at" => jet_jit_fs_read_at: sig_i64_i64_i64_i64;
    fs_write_at: "jet_jit_fs_write_at" => jet_jit_fs_write_at: sig_i64_i64_i64_i64;
    fs_fsync: "jet_jit_fs_fsync" => jet_jit_fs_fsync: sig_unary_i64;
    fs_write_atomic: "jet_jit_fs_write_atomic" => jet_jit_fs_write_atomic: sig_i64_i64_i64;
    fs_walk: "jet_jit_fs_walk" => jet_jit_fs_walk: sig_unary_i64;
    fs_glob: "jet_jit_fs_glob" => jet_jit_fs_glob: sig_unary_i64;
    fs_symlink: "jet_jit_fs_symlink" => jet_jit_fs_symlink: sig_i64_i64_i64;
    fs_read_link: "jet_jit_fs_read_link" => jet_jit_fs_read_link: sig_unary_i64;
    fs_hard_link: "jet_jit_fs_hard_link" => jet_jit_fs_hard_link: sig_i64_i64_i64;
    fs_canonicalize: "jet_jit_fs_canonicalize" => jet_jit_fs_canonicalize: sig_unary_i64;
    fs_absolute: "jet_jit_fs_absolute" => jet_jit_fs_absolute: sig_unary_i64;
    fs_copy_dir: "jet_jit_fs_copy_dir" => jet_jit_fs_copy_dir: sig_i64_i64_i64;
    fs_temp_dir: "jet_jit_fs_temp_dir" => jet_jit_fs_temp_dir: sig_unary_i64;
    fs_temp_file: "jet_jit_fs_temp_file" => jet_jit_fs_temp_file: sig_unary_i64;
    fs_lock: "jet_jit_fs_lock" => jet_jit_fs_lock: sig_unary_i64;
    mod_load: "jet_jit_mod_load" => jet_jit_mod_load: sig_i64_i64_i64;
    mod_on_tick: "jet_jit_mod_on_tick" => jet_jit_mod_on_tick: sig_i64_i64_i64;
    path_home: "jet_jit_path_home" => jet_jit_path_home: sig_unary_i64;
    path_from: "jet_jit_path_from" => jet_jit_path_from: sig_unary_i64;
    path_write_atomic: "jet_jit_path_write_atomic" => jet_jit_path_write_atomic: sig_i64_i64_i64;
    path_join_handle: "jet_jit_path_join_handle" => jet_jit_path_join_handle: sig_i64_i64_i64;
    path_parent: "jet_jit_path_parent" => jet_jit_path_parent: sig_unary_i64;
    path_extension: "jet_jit_path_extension" => jet_jit_path_extension: sig_unary_i64;
    path_stem: "jet_jit_path_stem" => jet_jit_path_stem: sig_unary_i64;
    path_normalize: "jet_jit_path_normalize" => jet_jit_path_normalize: sig_unary_i64;
    path_to_string: "jet_jit_path_to_string" => jet_jit_path_to_string: sig_unary_i64;
    path_walk: "jet_jit_path_walk" => jet_jit_path_walk: sig_unary_i64;
    math_sin: "jet_jit_math_sin" => jet_jit_math_sin: sig_f64_f64;
    math_cos: "jet_jit_math_cos" => jet_jit_math_cos: sig_f64_f64;
    math_exp: "jet_jit_math_exp" => jet_jit_math_exp: sig_f64_f64;
    math_atan2: "jet_jit_math_atan2" => jet_jit_math_atan2: sig_f64_f64_f64;
    math_hypot: "jet_jit_math_hypot" => jet_jit_math_hypot: sig_f64_f64_f64;
    math_lerp: "jet_jit_math_lerp" => jet_jit_math_lerp: sig_lerp;
    math_degrees: "jet_jit_math_degrees" => jet_jit_math_degrees: sig_f64_f64;
    math_radians: "jet_jit_math_radians" => jet_jit_math_radians: sig_f64_f64;
    math_is_finite: "jet_jit_math_is_finite" => jet_jit_math_is_finite: sig_f64_i8;
    math_sign: "jet_jit_math_sign" => jet_jit_math_sign: sig_f64_i64;
    math_checked_add: "jet_jit_math_checked_add" => jet_jit_math_checked_add: sig_i64_i64_i64;
    math_saturating_add: "jet_jit_math_saturating_add" => jet_jit_math_saturating_add: sig_i64_i64_i64;
    math_wrapping_add: "jet_jit_math_wrapping_add" => jet_jit_math_wrapping_add: sig_i64_i64_i64;
    math_int_pow: "jet_jit_math_int_pow" => jet_jit_math_int_pow: sig_i64_i64_i64;
    math_gcd: "jet_jit_math_gcd" => jet_jit_math_gcd: sig_i64_i64_i64;
    math_lcm: "jet_jit_math_lcm" => jet_jit_math_lcm: sig_i64_i64_i64;
    math_sqrt: "jet_jit_math_sqrt" => jet_jit_math_sqrt: sig_f64_f64;
    math_sqrt_f32: "jet_jit_math_sqrt_f32" => jet_jit_math_sqrt_f32: sig_f64_f64;
    math_pow: "jet_jit_math_pow" => jet_jit_math_pow: sig_f64_f64_f64;
    math_pow_f32: "jet_jit_math_pow_f32" => jet_jit_math_pow_f32: sig_f64_f64_f64;
    math_floor: "jet_jit_math_floor" => jet_jit_math_floor: sig_f64_f64;
    math_floor_f32: "jet_jit_math_floor_f32" => jet_jit_math_floor_f32: sig_f64_f64;
    math_ceil: "jet_jit_math_ceil" => jet_jit_math_ceil: sig_f64_f64;
    math_ceil_f32: "jet_jit_math_ceil_f32" => jet_jit_math_ceil_f32: sig_f64_f64;
    env_get: "jet_jit_env_get" => jet_jit_env_get: sig_unary_i64;
    env_set: "jet_jit_env_set" => jet_jit_env_set: sig_i64_i64_i64;
    env_unset: "jet_jit_env_unset" => jet_jit_env_unset: sig_unary_i64;
    env_vars: "jet_jit_env_vars" => jet_jit_env_vars: sig_i64;
    io_input: "jet_jit_io_input" => jet_jit_io_input: sig_i8_i64_i64;
    process_exit: "jet_jit_process_exit" => jet_jit_process_exit: sig_void_i64;
}
