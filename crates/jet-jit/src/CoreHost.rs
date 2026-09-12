//! Host shims for `core.sys`, `core.log`, `core.math`, and `core.files`,
//! plus `core.sys` and `core.process` CoreCalls (#729). Behavior
//! mirrors AOT helpers in the CoreLib prelude (`jet_std_os_*`, `jet_ring_log_*`,
//! `jet_std_math_*`, `jet_std_fs_*`, `jet_std_path_*`, `jet_std_env_*`,
//! `jet_std_process_*`) — thin std wrappers, not a third algorithm.
//! parity: guard tests/dev_tier_parity.rs::io_cli_terminal_and_time_match_interpreter_jit_and_aot

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use super::runtime_host::{self, contract_kernel, jit_result_parts};
use super::Concurrency;
use crate::Marshal::{alloc_byte_list, clone_bytes, clone_string, result_err_msg, result_ok};
use jet_foundation::Devtools::*;
use std::sync::{mpsc, OnceLock};

mod path_kernel {
    include!("../../jet-codegen/src/Prelude/Core/Path.rs");
}

mod fs_walk_kernel {
    include!("../../jet-codegen/src/Prelude/Core/FSIgnore.rs");
    include!("../../jet-codegen/src/Prelude/Core/FSWalk.rs");
}

mod fs_ops_kernel {
    use crate::Collections::authority_semantics::JetFileScope;
    include!("../../jet-codegen/src/Prelude/Core/FSOps.rs");
}

mod keep_kernel {
    include!("../../jet-codegen/src/Prelude/Core/Keep.rs");
}
pub(crate) mod jet_std {
    #[derive(Clone, Debug, PartialEq)]
    pub(crate) struct LogField {
        pub(crate) key: String,
        pub(crate) value: String,
        pub(crate) kind: String,
        pub(crate) redacted: bool,
    }

    #[derive(Clone, Debug, PartialEq, Default)]
    pub(crate) struct LogSpan {
        pub(crate) id: i64,
        pub(crate) name: String,
    }
}

fn jet_log_write_line(line: &str) {
    let _ = super::runtime_host::write_jit_stderr(
        &super::IO::term_prelude::jet_term_print_frame(line),
        false,
    );
}

fn jet_log_process_exit(code: i64) {
    jet_jit_process_exit(code);
}

include!("../../jet-codegen/src/Prelude/Core/LogState.rs");
include!("../../jet-codegen/src/Prelude/CoreLib/Top/Log.rs");

// #2027 / I8+I9: the resident host reaches the one signal mechanism through the
// single in-binary instance of `Prelude/CoreLib/Top/Interrupt.rs` that the TIR
// evaluator ambient also uses. A private `include!` here compiled a second
// pending count, and its `signal(SIGINT, …)` install disarmed whichever tier
// armed first.
use jet_codegen::interrupt_runtime;

// The Prelude owns every core.sys fact. This module supplies only the small
// type/ambient surface needed to include that exact source; wrappers below
// marshal its Rust values to the resident heap ABI.
pub(crate) mod os_rt {
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/SHA256Raw.rs");

    pub(crate) mod jet_std {
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
        pub(crate) struct Stat {
            pub(crate) size: i64,
            pub(crate) modified_ms: i64,
            pub(crate) created_ms: i64,
            pub(crate) readonly: bool,
            pub(crate) is_file: bool,
            pub(crate) is_dir: bool,
            pub(crate) is_symlink: bool,
            pub(crate) kind: String,
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

        pub(crate) fn io_error_at(
            operation: IOOperation,
            path: &str,
            error: std::io::Error,
        ) -> IOError {
            let context = IOContext {
                operation,
                resource: Some(path.to_string()),
                os_code: error.raw_os_error().map(i64::from),
                cause: Some(error.to_string()),
            };
            match error.kind() {
                std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => {
                    IOError::InvalidInput(context)
                }
                std::io::ErrorKind::NotFound => IOError::NotFound(context),
                std::io::ErrorKind::PermissionDenied => IOError::PermissionDenied(context),
                std::io::ErrorKind::TimedOut => IOError::TimedOut(context),
                std::io::ErrorKind::NotConnected | std::io::ErrorKind::BrokenPipe => {
                    IOError::Closed(context)
                }
                _ => IOError::Other(context),
            }
        }
        include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/MappedFile.rs");
    }

    fn jet_std_env_get(name: &String) -> Option<String> {
        super::jit_env_value(name)
    }

    fn jet_std_process_exit(code: i64) {
        super::jet_jit_process_exit(code);
    }

    mod prelude_impl {
        use super::{jet_std, jet_std_env_get, jet_std_process_exit};

        include!("../../jet-codegen/src/Prelude/CoreLib/Top/PlatformFamily.rs");
        include!("../../jet-codegen/src/Prelude/CoreLib/Top/OsExtra.rs");

        pub(crate) fn jet_std_env_current_dir() -> Result<String, jet_std::IOError> {
            std::env::current_dir()
                .map(|path| path.to_string_lossy().to_string())
                .map_err(|error| {
                    jet_std::IOError::other(jet_std::IOOperation::Resolve, None, error)
                })
        }

        pub(crate) fn jet_std_env_home_dir() -> Option<String> {
            jet_std_env_get(&"HOME".to_string())
                .or_else(|| jet_std_env_get(&"USERPROFILE".to_string()))
        }

        pub(crate) fn jet_std_os_set_current_dir(path: &String) -> Result<(), jet_std::IOError> {
            std::env::set_current_dir(path).map_err(|error| {
                jet_std::IOError::other(jet_std::IOOperation::Resolve, Some(path.clone()), error)
            })
        }
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
            let _ = rt
                .heap
                .record_set_int(record, 0, operation_index(&context.operation));
            let resource = context
                .resource
                .map(|value| rt.heap.alloc_string(value).wrapping_add(1))
                .unwrap_or(0);
            let _ = rt.heap.record_set_int(record, 1, resource);
            let _ = rt.heap.record_set_int(
                record,
                2,
                context.os_code.map(|code| code + 1).unwrap_or(0),
            );
            let cause = context
                .cause
                .map(|value| rt.heap.alloc_string(value).wrapping_add(1))
                .unwrap_or(0);
            let _ = rt.heap.record_set_int(record, 3, cause);
            let packed = ((record << 8) | error_index(name)) as u64;
            rt.results.push(crate::JitResultValue {
                ok: false,
                bits: packed,
            });
            rt.results.len() as i64
        })
    }

    pub(super) fn marshal_result<T>(
        result: Result<T, jet_std::IOError>,
        ok: impl FnOnce(T) -> u64,
    ) -> i64 {
        match result {
            Ok(value) => super::result_ok(ok(value)),
            Err(error) => marshal_error(error),
        }
    }

    pub(super) use prelude_impl::{
        jet_std_env_current_dir, jet_std_env_home_dir, jet_std_os_arch, jet_std_os_close_fd,
        jet_std_os_cpu_count, jet_std_os_executable, jet_std_os_exitcode, jet_std_os_expand,
        jet_std_os_family, jet_std_os_fork, jet_std_os_getegid, jet_std_os_geteuid,
        jet_std_os_getgid, jet_std_os_getgroups, jet_std_os_getpgid, jet_std_os_getpgrp,
        jet_std_os_getppid, jet_std_os_getpriority, jet_std_os_getsid, jet_std_os_getuid,
        jet_std_os_hostname, jet_std_os_initgroups, jet_std_os_kill, jet_std_os_loadavg,
        jet_std_os_mkfifo, jet_std_os_name, jet_std_os_pid, jet_std_os_pipe, jet_std_os_release,
        jet_std_os_set_current_dir, jet_std_os_setgid, jet_std_os_setpgid, jet_std_os_setpgrp,
        jet_std_os_setpriority, jet_std_os_setsid, jet_std_os_setuid, jet_std_os_success,
        jet_std_os_sync, jet_std_os_temp_dir, jet_std_os_times, jet_std_os_umask,
        jet_std_os_uptime, jet_std_os_username, jet_std_os_utime, jet_std_os_version,
        jet_std_os_wait, jet_std_os_waitpid,
    };
}

// FSRuntimeOps.rs is the one policy-bearing filesystem fragment. The
// resident host supplies only these raw kernels and its result marshaller.
mod fs_prelude {
    use crate::Collections::authority_semantics::{JetAuthority, JetFileScope};
    use crate::Text::text_rt::{jet_view_iter_from_iter, JetViewIter};
    use super::fs_ops_kernel::{
        jet_fs_canonicalize, jet_fs_glob, jet_fs_rename, jet_fs_scope_read,
    };
    use super::os_rt::jet_std;
    use crate::fault_injection::jet_fault_should_fail;

    include!("../../jet-codegen/src/Prelude/CoreLib/Top/FSRuntimeOps.rs");
}
mod fs_write_prelude {
    use super::os_rt::jet_std;
    use crate::fault_injection::jet_fault_should_fail;

    pub(crate) struct JetStdinReader {
        inner: std::io::BufReader<std::io::Stdin>,
    }

    fn jet_std_io_stdin() -> JetStdinReader {
        JetStdinReader {
            inner: std::io::BufReader::new(std::io::stdin()),
        }
    }

    include!("../../jet-codegen/src/Prelude/CoreLib/Top/FSWriteOps.rs");
}

// The resident JIT cannot hand a Rust `Rc` callback to the process signal
// boundary. TIR gives it one Send-safe record containing a function address and
// environment handle. This adapter owns only that raw-code invocation boundary
// and the handler storage; the pending count, the platform handler, the arm path
// and the count-first additive ordering all come from the shared Prelude owner.
mod jit_os_interrupt {
    use super::{interrupt_runtime, mpsc, Concurrency, OnceLock};

    static DISPATCH: OnceLock<Result<mpsc::Sender<DispatchCommand>, String>> = OnceLock::new();

    struct Command {
        callback: usize,
        env: i64,
        has_env: bool,
        ready: mpsc::SyncSender<()>,
    }

    enum DispatchCommand {
        Register(Command),
        Reset(mpsc::SyncSender<()>),
    }

    fn dispatcher() -> Result<&'static mpsc::Sender<DispatchCommand>, String> {
        match DISPATCH.get_or_init(|| {
            interrupt_runtime::jet_interrupt_arm()?;
            let (tx, rx) = mpsc::channel::<DispatchCommand>();
            std::thread::Builder::new()
                .name("jet-jit-interrupt".to_string())
                .spawn(move || {
                    let mut handlers: Vec<(usize, i64, bool)> = Vec::new();
                    loop {
                        match rx.recv_timeout(interrupt_runtime::jet_interrupt_poll_interval()) {
                            Ok(DispatchCommand::Register(command)) => {
                                handlers.push((command.callback, command.env, command.has_env));
                                let _ = command.ready.send(());
                            }
                            Ok(DispatchCommand::Reset(ready)) => {
                                handlers.clear();
                                interrupt_runtime::jet_interrupt_clear();
                                let _ = ready.send(());
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                        }
                        interrupt_runtime::jet_interrupt_dispatch(
                            &handlers,
                            |&(callback, environment, has_env)| {
                                Concurrency::with_http_jet_runtime(|| {
                                    unsafe {
                                        if has_env {
                                            let callback: extern "C" fn(i64) =
                                                std::mem::transmute(callback);
                                            callback(environment);
                                        } else {
                                            let callback: extern "C" fn() =
                                                std::mem::transmute(callback);
                                            callback();
                                        }
                                    }
                                });
                            },
                        );
                    }
                })
                .map_err(interrupt_runtime::jet_interrupt_dispatcher_start_error)?;
            Ok(tx)
        }) {
            Ok(tx) => Ok(tx),
            Err(message) => Err(message.clone()),
        }
    }

    fn register_parts(callback: usize, env: i64, has_env: bool) -> Result<(), String> {
        if callback == 0 {
            return Err(interrupt_runtime::jet_interrupt_invalid_callback_value_error().to_string());
        }
        let tx = dispatcher()?;
        let (ready_tx, ready_rx) = mpsc::sync_channel(0);
        tx.send(DispatchCommand::Register(Command {
            callback,
            env,
            has_env,
            ready: ready_tx,
        }))
        .map_err(|_| interrupt_runtime::jet_interrupt_dispatcher_stopped_error().to_string())?;
        ready_rx.recv().map_err(|_| {
            interrupt_runtime::jet_interrupt_dispatcher_stopped_error().to_string()
        })
    }

    pub(super) fn register(callback_record: i64) {
        let result = (|| {
            let (callback, environment) = Concurrency::with_runtime_mut(|rt| {
                (
                    rt.heap.record_get_int(callback_record, 0).unwrap_or(0),
                    rt.heap.record_get_int(callback_record, 1).unwrap_or(0),
                )
            });
            register_parts(callback as usize, environment, true).map_err(|message| {
                if callback == 0 {
                    interrupt_runtime::jet_interrupt_invalid_callback_record_error().to_string()
                } else {
                    message
                }
            })
        })();
        if let Err(message) = result {
            Concurrency::with_runtime_mut(|rt| {
                rt.set_trap(&interrupt_runtime::jet_interrupt_core_error(&message));
            });
        }
    }

    pub(super) fn register_callable(callable: i64) -> i64 {
        let slot = Concurrency::with_runtime_mut(|rt| {
            super::super::runtime_host::jit_callable_parts(rt, callable)
        });
        let Some(slot) = slot else {
            Concurrency::with_runtime_mut(|rt| {
                rt.set_host_fault("MIR interrupt closure has an invalid callable handle");
                true
            });
            return 0;
        };
        if let Err(message) = register_parts(slot.fn_ptr as usize, slot.env, slot.has_env) {
            Concurrency::with_runtime_mut(|rt| {
                rt.set_host_fault(&interrupt_runtime::jet_interrupt_core_error(&message));
                true
            });
        }
        0
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


fn jet_jit_core_os_on_interrupt(callable: i64) -> i64 {
    jit_os_interrupt::register_callable(callable)
}
fn jet_jit_os_on_interrupt(callback_record: i64) {
    jit_os_interrupt::register(callback_record);
}

pub(crate) fn reset_jit_interrupts() {
    jit_os_interrupt::reset();
}

// D-BENCH-KEEP1=A: each wrapper is only a carrier-shaped ABI adapter. The
// sink itself is the shared Prelude `jet_keep` used by generated AOT code.
fn jet_jit_keep_i64(value: i64) -> i64 {
    keep_kernel::jet_keep(value)
}

fn jet_jit_keep_f64(value: f64) -> f64 {
    keep_kernel::jet_keep(value)
}

fn jet_jit_keep_i8(value: i8) -> i8 {
    keep_kernel::jet_keep(value)
}

fn jet_jit_keep_i32(value: i32) -> i32 {
    keep_kernel::jet_keep(value)
}

fn jet_jit_keep_unit() {
    keep_kernel::jet_keep(());
}

// ── core.sys (Prelude facts; these functions only marshal values) ─────────────

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
fn jet_jit_env_current_dir() -> i64 {
    os_rt::marshal_result(os_rt::jet_std_env_current_dir(), |value| {
        Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value)) as u64
    })
}
fn jet_jit_env_home_dir() -> i64 {
    option_string_bits(os_rt::jet_std_env_home_dir())
}

fn jet_jit_os_name() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_name()))
}
fn jet_jit_os_family() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_family()))
}
fn jet_jit_os_arch() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_arch()))
}
fn jet_jit_os_cpu_count() -> i64 {
    os_rt::jet_std_os_cpu_count()
}
fn jet_jit_os_temp_dir() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_temp_dir()))
}
fn jet_jit_os_executable() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_executable()))
}
fn jet_jit_os_pid() -> i64 {
    os_rt::jet_std_os_pid()
}
fn jet_jit_os_hostname() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_hostname()))
}
fn jet_jit_os_username() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_username()))
}
fn jet_jit_os_release() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_release()))
}
fn jet_jit_os_version() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(os_rt::jet_std_os_version()))
}
fn jet_jit_os_getppid() -> i64 {
    os_rt::jet_std_os_getppid()
}
fn jet_jit_os_getuid() -> i64 {
    os_rt::jet_std_os_getuid()
}
fn jet_jit_os_geteuid() -> i64 {
    os_rt::jet_std_os_geteuid()
}
fn jet_jit_os_getgid() -> i64 {
    os_rt::jet_std_os_getgid()
}
fn jet_jit_os_getegid() -> i64 {
    os_rt::jet_std_os_getegid()
}
fn jet_jit_os_getpgrp() -> i64 {
    os_rt::jet_std_os_getpgrp()
}
fn jet_jit_os_getgroups() -> i64 {
    alloc_i64_list(&os_rt::jet_std_os_getgroups())
}
fn jet_jit_os_uptime() -> f64 {
    os_rt::jet_std_os_uptime()
}
fn jet_jit_os_loadavg() -> i64 {
    alloc_f64_list_os(&os_rt::jet_std_os_loadavg())
}
fn jet_jit_os_times() -> i64 {
    alloc_f64_list_os(&os_rt::jet_std_os_times())
}
fn jet_jit_os_success(status: i64) -> i8 {
    i8::from(os_rt::jet_std_os_success(status))
}
fn jet_jit_os_exitcode(status: i64) -> i64 {
    os_rt::jet_std_os_exitcode(status)
}
fn jet_jit_os_expand(template: i64) -> i64 {
    let template = clone_string(template);
    let value = os_rt::jet_std_os_expand(&template);
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value))
}
fn jet_jit_os_getpgid(pid: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_getpgid(pid), |value| value as u64)
}
fn jet_jit_os_getsid(pid: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_getsid(pid), |value| value as u64)
}
fn jet_jit_os_setpgid(pid: i64, pgid: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_setpgid(pid, pgid), |_| 0)
}
fn jet_jit_os_setpgrp() -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_setpgrp(), |_| 0)
}
fn jet_jit_os_umask(mask: i64) -> i64 {
    os_rt::jet_std_os_umask(mask)
}
fn jet_jit_os_sync() {
    os_rt::jet_std_os_sync()
}
fn jet_jit_os_getpriority(who: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_getpriority(who), |value| value as u64)
}
fn jet_jit_os_setpriority(who: i64, priority: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_setpriority(who, priority), |_| 0)
}
fn jet_jit_os_kill(pid: i64, signal: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_kill(pid, signal), |_| 0)
}
fn jet_jit_os_pipe() -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_pipe(), |values| {
        alloc_i64_list(&values) as u64
    })
}
fn jet_jit_os_close_fd(fd: i64) {
    os_rt::jet_std_os_close_fd(fd)
}
fn jet_jit_os_mkfifo(path: i64, mode: i64) -> i64 {
    let path = clone_path_arg(path);
    os_rt::marshal_result(os_rt::jet_std_os_mkfifo(&path, mode), |_| 0)
}
fn jet_jit_os_set_current_dir(path: i64) -> i64 {
    let path = clone_path_arg(path);
    os_rt::marshal_result(os_rt::jet_std_os_set_current_dir(&path), |_| 0)
}

fn jet_jit_os_fork() -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_fork(), |value| value as u64)
}
fn jet_jit_os_setuid(uid: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_setuid(uid), |_| 0)
}
fn jet_jit_os_setgid(gid: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_setgid(gid), |_| 0)
}
fn jet_jit_os_setsid() -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_setsid(), |value| value as u64)
}
fn jet_jit_os_initgroups(user: i64, group: i64) -> i64 {
    let user = clone_string(user);
    os_rt::marshal_result(os_rt::jet_std_os_initgroups(&user, group), |_| 0)
}
fn jet_jit_os_wait() -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_wait(), |value| value as u64)
}
fn jet_jit_os_waitpid(pid: i64, options: i64) -> i64 {
    os_rt::marshal_result(os_rt::jet_std_os_waitpid(pid, options), |value| {
        value as u64
    })
}
fn jet_jit_os_utime(path: i64, atime: i64, mtime: i64) -> i64 {
    let path = clone_path_arg(path);
    os_rt::marshal_result(os_rt::jet_std_os_utime(&path, atime, mtime), |_| 0)
}
fn jet_jit_os_atexit(handler: i64) -> i64 {
    if crate::runtime_host::register_jit_atexit(handler) {
        result_ok(0)
    } else {
        result_err_msg("invalid resident atexit callback")
    }
}
fn jet_jit_os_stop(code: i64) {
    // D-FAIL-EXIT1: the resident host is an in-process boundary. Keep the
    // same soft exit record as `process.exit`; resident cleanup drains it
    // before returning the run outcome.
    jet_jit_process_exit(code)
}

// ── core.log (shared with the AOT Prelude) ───────────────────────────────────

pub(crate) fn set_cli_log_level(level: &str) {
    jet_ring_log_set_level(&level.to_string());
}

pub(crate) fn ambient_log_set_level(level: &str) {
    jet_ring_log_set_level(&level.to_string());
}
pub(crate) fn ambient_log_set_trace_id(trace_id: &str) {
    jet_ring_log_set_trace_id(trace_id);
}

pub(crate) fn ambient_log_enabled(level: &str) -> bool {
    jet_ring_log_enabled(&level.to_string())
}

pub(crate) fn ambient_log_fatal(message: &str) {
    jet_ring_log_fatal(&message.to_string());
}

fn jet_jit_log_set_level(msg: i64) {
    jet_ring_log_set_level(&clone_string(msg));
}

fn jet_jit_log_setup(msg: i64) {
    jet_ring_log_setup(&clone_string(msg));
}

fn jet_jit_log_set_sink(kind: i64, path: i64) {
    let kind = clone_string(kind);
    let path = clone_string(path);
    jet_ring_log_set_sink(&kind, &path);
}

fn jet_jit_log_sample_every(n: i64) {
    jet_ring_log_sample_every(n);
}

fn jet_jit_log_otlp_file(path: i64) {
    jet_ring_log_otlp_file(&clone_string(path));
}

fn jet_jit_log_debug(msg: i64) {
    jet_ring_log_debug(&clone_string(msg));
}

fn jet_jit_log_info(msg: i64) {
    jet_ring_log_info(&clone_string(msg));
}

fn jet_jit_log_warn(msg: i64) {
    jet_ring_log_warn(&clone_string(msg));
}

fn jet_jit_log_error(msg: i64) {
    jet_ring_log_error(&clone_string(msg));
}

fn jet_jit_log_critical(msg: i64) {
    jet_ring_log_critical(&clone_string(msg));
}

fn jet_jit_log_fatal(msg: i64) {
    jet_ring_log_fatal(&clone_string(msg));
}

fn jet_jit_log_disable() {
    jet_ring_log_disable();
}

fn jet_jit_log_flush() {
    jet_ring_log_flush();
}

fn jet_jit_log_enabled(level: i64) -> i8 {
    i8::from(jet_ring_log_enabled(&clone_string(level)))
}

fn jet_jit_log_set_trace_id(msg: i64) {
    jet_ring_log_set_trace_id(&clone_string(msg));
}

fn alloc_log_field(field: jet_std::LogField) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let rec = rt.heap.alloc_record(4);
        let key = rt.heap.alloc_string(field.key);
        let value = rt.heap.alloc_string(field.value);
        let kind = rt.heap.alloc_string(field.kind);
        let _ = rt.heap.record_set_string(rec, 0, key);
        let _ = rt.heap.record_set_string(rec, 1, value);
        let _ = rt.heap.record_set_string(rec, 2, kind);
        let _ = rt.heap.record_set_bool(rec, 3, field.redacted);
        rec
    })
}

fn jet_jit_log_field(key: i64, value: i64) -> i64 {
    let key = clone_string(key);
    let value = clone_string(value);
    alloc_log_field(jet_ring_log_field(&key, &value))
}

fn jet_jit_log_int(key: i64, value: i64) -> i64 {
    alloc_log_field(jet_ring_log_int(&clone_string(key), value))
}

fn jet_jit_log_float(key: i64, value: f64) -> i64 {
    alloc_log_field(jet_ring_log_float(&clone_string(key), value))
}

fn jet_jit_log_bool(key: i64, value: i8) -> i64 {
    alloc_log_field(jet_ring_log_bool(&clone_string(key), value != 0))
}

fn jet_jit_log_redact(key: i64) -> i64 {
    alloc_log_field(jet_ring_log_redact(&clone_string(key)))
}

fn jet_jit_log_counter(name: i64, value: i64) -> i64 {
    alloc_log_field(jet_ring_log_counter(&clone_string(name), value))
}

fn alloc_log_span(span: jet_std::LogSpan) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let rec = rt.heap.alloc_record(2);
        let name = rt.heap.alloc_string(span.name);
        let _ = rt.heap.record_set_int(rec, 0, span.id);
        let _ = rt.heap.record_set_string(rec, 1, name);
        rec
    })
}

fn jet_jit_log_span(name: i64) -> i64 {
    alloc_log_span(jet_ring_log_span(&clone_string(name)))
}

fn read_log_span(span: i64) -> jet_std::LogSpan {
    Concurrency::with_runtime_mut(|rt| jet_std::LogSpan {
        id: rt.heap.record_get_int(span, 0).unwrap_or(0),
        name: rt
            .heap
            .record_get_string(span, 1)
            .and_then(|id| rt.heap.clone_string(id))
            .unwrap_or_default(),
    })
}

fn jet_jit_log_enter(span: i64) {
    let span = read_log_span(span);
    jet_ring_log_enter(&span);
}

fn jet_jit_log_close(span: i64) {
    let span = read_log_span(span);
    jet_ring_log_close(&span);
}

fn read_log_fields(list: i64) -> Vec<jet_std::LogField> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut fields = Vec::with_capacity(len as usize);
        for index in 0..len {
            let record = rt.heap.list_get_int(list, index).unwrap_or(0);
            let key = rt
                .heap
                .record_get_string(record, 0)
                .and_then(|id| rt.heap.clone_string(id))
                .unwrap_or_default();
            let value = rt
                .heap
                .record_get_string(record, 1)
                .and_then(|id| rt.heap.clone_string(id))
                .unwrap_or_default();
            let kind = rt
                .heap
                .record_get_string(record, 2)
                .and_then(|id| rt.heap.clone_string(id))
                .unwrap_or_else(|| "string".to_string());
            let redacted = rt.heap.record_get_bool(record, 3).unwrap_or(false);
            fields.push(jet_std::LogField {
                key,
                value,
                kind,
                redacted,
            });
        }
        fields
    })
}

fn jet_jit_log_debug_fields(msg: i64, fields: i64) {
    let msg = clone_string(msg);
    let fields = read_log_fields(fields);
    jet_ring_log_debug_fields(&msg, &fields);
}

fn jet_jit_log_info_fields(msg: i64, fields: i64) {
    let msg = clone_string(msg);
    let fields = read_log_fields(fields);
    jet_ring_log_info_fields(&msg, &fields);
}

fn jet_jit_log_warn_fields(msg: i64, fields: i64) {
    let msg = clone_string(msg);
    let fields = read_log_fields(fields);
    jet_ring_log_warn_fields(&msg, &fields);
}

fn jet_jit_log_error_fields(msg: i64, fields: i64) {
    let msg = clone_string(msg);
    let fields = read_log_fields(fields);
    jet_ring_log_error_fields(&msg, &fields);
}

pub(crate) fn ambient_log_span(name: &str) -> i64 {
    jet_ring_log_span(&name.to_string()).id
}

pub(crate) fn ambient_log_enter(id: i64, name: &str) {
    jet_ring_log_enter(&jet_std::LogSpan {
        id,
        name: name.to_string(),
    });
}

pub(crate) fn ambient_log_close(id: i64, name: &str) {
    jet_ring_log_close(&jet_std::LogSpan {
        id,
        name: name.to_string(),
    });
}

pub(crate) fn ambient_log_set_sink(kind: &str, path: &str) {
    jet_ring_log_set_sink(&kind.to_string(), &path.to_string());
}

pub(crate) fn ambient_log_sample_every(n: i64) {
    jet_ring_log_sample_every(n);
}

pub(crate) fn ambient_log_otlp_file(path: &str) {
    jet_ring_log_otlp_file(&path.to_string());
}

pub(crate) fn ambient_log_disable() {
    jet_ring_log_disable();
}
pub(crate) fn ambient_log_emit(level: &str, message: &str, fields: &[(String, String, String)]) {
    let fields = fields
        .iter()
        .map(|(key, value, kind)| jet_std::LogField {
            key: key.clone(),
            value: value.clone(),
            kind: kind.clone(),
            redacted: kind == "redacted",
        })
        .collect::<Vec<_>>();
    let message = message.to_string();
    match level {
        "debug" => jet_ring_log_debug_fields(&message, &fields),
        "info" => jet_ring_log_info_fields(&message, &fields),
        "warn" => jet_ring_log_warn_fields(&message, &fields),
        "error" => jet_ring_log_error_fields(&message, &fields),
        "critical" => jet_ring_log_critical(&message),
        _ => {}
    }
}

pub(crate) fn ambient_log_flush() {
    jet_ring_log_flush();
}

// ── core.files and typed Path (mirrors jet_std_fs_* / jet_std_path_*) ────────

/// Filesystem Core rows accept either `String` or the erased `Path` record.
/// Keep that coercion at the resident boundary, matching the shared
/// interpreter/AOT path normalization instead of treating a record as an
/// invalid string handle.
fn clone_path_arg(id: i64) -> String {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .record_clone_string(id, 0)
            .or_else(|| rt.heap.clone_string(id))
            .unwrap_or_default()
    })
}

fn jet_jit_fs_exists(path: i64) -> i8 {
    let p = clone_path_arg(path);
    i8::from(std::path::Path::new(&p).exists())
}

fn jet_jit_fs_read(path: i64) -> i64 {
    let p = clone_path_arg(path);
    if crate::fault_injection::jet_fault_should_fail("FS.Read") {
        return result_err_msg(&format!("fault injected: FS.Read for {p}"));
    }
    match std::fs::read_to_string(&p) {
        Ok(text) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text));

            result_ok(sid as u64)
        }
        Err(e) => result_err_msg(&format!("read {p}: {e}")),
    }
}
fn jet_jit_fs_scope(authority: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let scope = crate::Collections::authority_file_scope(rt, authority);
        let index = rt.file_scopes.len();
        rt.file_scopes.push(scope);
        runtime_host::file_scope_handle(index)
    })
}

fn jet_jit_fs_scope_read(scope: i64, path: i64) -> i64 {
    let path = clone_path_arg(path);
    Concurrency::with_runtime_mut(|rt| {
        let Some(index) = runtime_host::file_scope_index(rt, scope) else {
            rt.set_trap("invalid FileScope handle");
            return 0;
        };
        os_rt::marshal_result(
            fs_prelude::jet_std_fs_scope_read(&rt.file_scopes[index], &path),
            |text| rt.heap.alloc_string(text) as u64,
        )
    })
}

fn jet_jit_fs_read_bytes(path: i64) -> i64 {
    let p = clone_path_arg(path);
    if crate::fault_injection::jet_fault_should_fail("FS.Read") {
        return result_err_msg(&format!("fault injected: FS.Read for {p}"));
    }
    match std::fs::read(&p) {
        Ok(bytes) => result_ok(alloc_byte_list(&bytes) as u64),
        Err(e) => result_err_msg(&format!("read_bytes {p}: {e}")),
    }
}

fn jet_jit_fs_map(path: i64) -> i64 {
    let path = clone_path_arg(path);
    os_rt::marshal_result(fs_prelude::jet_std_fs_map(&path), |map| {
        Concurrency::with_runtime_mut(|rt| {
            let index = rt.mapped_files.len();
            rt.mapped_files.push(map);
            runtime_host::mapped_file_handle(index) as u64
        })
    })
}

fn jet_jit_fs_map_window_view(map: i64, start: i64, end: i64) -> i64 {
    let result = Concurrency::with_runtime_mut(|rt| {
        let Some(index) = runtime_host::mapped_file_index(rt, map) else {
            rt.set_trap("invalid MappedFile handle");
            return None;
        };
        Some(rt.mapped_files[index].window(start, end))
    });
    match result {
        Some(Ok(view)) => Concurrency::with_runtime_mut(|rt| {
            let index = rt.mapped_views.len();
            rt.mapped_views.push(view);
            let slot = rt.view_slots.len();
            rt.view_slots
                .push(runtime_host::JitViewSlot::Mapped { view: index });
            result_ok(runtime_host::view_handle(slot) as u64)
        }),
        Some(Err(error)) => os_rt::marshal_error(error),
        None => 0,
    }
}

fn jet_jit_fs_map_window_len_view(map: i64, offset: i64, length: i64) -> i64 {
    let result = Concurrency::with_runtime_mut(|rt| {
        let Some(index) = runtime_host::mapped_file_index(rt, map) else {
            rt.set_trap("invalid MappedFile handle");
            return None;
        };
        Some(fs_prelude::jet_std_fs_map_window(&rt.mapped_files[index], offset, length))
    });
    match result {
        Some(Ok(view)) => Concurrency::with_runtime_mut(|rt| {
            let index = rt.mapped_views.len();
            rt.mapped_views.push(view);
            let slot = rt.view_slots.len();
            rt.view_slots
                .push(runtime_host::JitViewSlot::Mapped { view: index });
            result_ok(runtime_host::view_handle(slot) as u64)
        }),
        Some(Err(error)) => os_rt::marshal_error(error),
        None => 0,
    }
}

fn jet_jit_fs_map_lines_view(map: i64) -> i64 {
    let lines = Concurrency::with_runtime_mut(|rt| {
        let Some(index) = runtime_host::mapped_file_index(rt, map) else {
            rt.set_trap("invalid MappedFile handle");
            return None;
        };
        Some(fs_prelude::jet_std_fs_map_lines(&rt.mapped_files[index]))
    });
    let Some(lines) = lines else {
        return 0;
    };
    Concurrency::with_runtime_mut(|rt| {
        let out = rt.heap.alloc_empty_list();
        for view in lines {
            let view_index = rt.mapped_views.len();
            rt.mapped_views.push(view);
            let slot = rt.view_slots.len();
            rt.view_slots
                .push(runtime_host::JitViewSlot::Mapped { view: view_index });
            let handle = runtime_host::view_handle(slot);
            rt.heap
                .list_push_int(out, handle)
                .expect("JIT mapped-file lines list");
        }
        out
    })
}

fn jet_jit_fs_map_len(map: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(index) = runtime_host::mapped_file_index(rt, map) else {
            rt.set_trap("invalid MappedFile handle");
            return 0;
        };
        rt.mapped_files[index].len()
    })
}

fn jet_jit_fs_map_is_empty(map: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(index) = runtime_host::mapped_file_index(rt, map) else {
            rt.set_trap("invalid MappedFile handle");
            return 0;
        };
        i8::from(rt.mapped_files[index].is_empty())
    })
}

fn jet_jit_fs_write(path: i64, text: i64) -> i64 {
    let p = clone_path_arg(path);
    let t = clone_string(text);
    if crate::fault_injection::jet_fault_should_fail("FS.Write") {
        return result_err_msg(&format!("fault injected: FS.Write for {p}"));
    }
    match std::fs::write(&p, t) {
        Ok(()) => result_ok(0),
        Err(e) => result_err_msg(&format!("write {p}: {e}")),
    }
}

fn jet_jit_fs_append(path: i64) -> i64 {
    super::enc_stream::jet_jit_fs_append(path)
}

fn jet_jit_fs_append_all(path: i64, text: i64) -> i64 {
    let p = clone_path_arg(path);
    let t = clone_string(text);
    if crate::fault_injection::jet_fault_should_fail("FS.Write") {
        return result_err_msg(&format!("fault injected: FS.Write for {p}"));
    }
    use std::io::Write;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .and_then(|mut file| file.write_all(t.as_bytes()))
    {
        Ok(()) => result_ok(0),
        Err(error) => result_err_msg(&format!("append {p}: {error}")),
    }
}

fn jet_jit_fs_write_bytes(path: i64, bytes: i64) -> i64 {
    let p = clone_path_arg(path);
    let data = clone_bytes(bytes);
    if crate::fault_injection::jet_fault_should_fail("FS.Write") {
        return result_err_msg(&format!("fault injected: FS.Write for {p}"));
    }
    match std::fs::write(&p, data) {
        Ok(()) => result_ok(0),
        Err(e) => result_err_msg(&format!("write_bytes {p}: {e}")),
    }
}

fn jet_jit_fs_create_dir(path: i64) -> i64 {
    let p = clone_path_arg(path);
    if crate::fault_injection::jet_fault_should_fail("FS.Write") {
        return result_err_msg(&format!("fault injected: FS.Write for {p}"));
    }
    match std::fs::create_dir_all(&p) {
        Ok(()) => result_ok(0),
        Err(e) => result_err_msg(&format!("create_dir {p}: {e}")),
    }
}
fn jet_jit_fs_is_dir(path: i64) -> i8 {
    i8::from(std::path::Path::new(&clone_path_arg(path)).is_dir())
}

fn jet_jit_fs_remove_dir(path: i64) -> i64 {
    let p = clone_path_arg(path);
    if crate::fault_injection::jet_fault_should_fail("FS.Write") {
        return result_err_msg(&format!("fault injected: FS.Write for {p}"));
    }
    match std::fs::remove_dir(&p) {
        Ok(()) => result_ok(0),
        Err(e) => result_err_msg(&format!("remove_dir {p}: {e}")),
    }
}

fn jet_jit_fs_copy(from: i64, to: i64) -> i64 {
    let src = clone_path_arg(from);
    let dst = clone_path_arg(to);
    if crate::fault_injection::jet_fault_should_fail("FS.Read") {
        return result_err_msg(&format!("fault injected: FS.Read for {src}"));
    }
    if crate::fault_injection::jet_fault_should_fail("FS.Write") {
        return result_err_msg(&format!("fault injected: FS.Write for {dst}"));
    }
    match std::fs::copy(&src, &dst) {
        Ok(_) => result_ok(0),
        Err(e) => result_err_msg(&format!("copy {src}: {e}")),
    }
}

// Card 1984: `core.files.create_dir_all` had no resident host, so lowering's
// registry projection missed and `io/files_depth` deopted. The Prelude makes
// both rows the same operation — `jet_std_fs_create_dir` and
// `jet_std_fs_create_dir_all` (Prelude/CoreLib/Top/Text.rs) both call
// `std::fs::create_dir_all` behind the same `FS.Write` fault gate — so this
// adapter marshals to the same call rather than inventing a second policy.
fn jet_jit_fs_create_dir_all(path: i64) -> i64 {
    let p = clone_path_arg(path);
    if crate::fault_injection::jet_fault_should_fail("FS.Write") {
        return result_err_msg(&format!("fault injected: FS.Write for {p}"));
    }
    match std::fs::create_dir_all(&p) {
        Ok(()) => result_ok(0),
        Err(e) => result_err_msg(&format!("create_dir_all {p}: {e}")),
    }
}

fn jet_jit_path_home() -> i64 {
    path_record(path_kernel::jet_std_path_home())
}

fn jet_jit_path_write_atomic(rec: i64, bytes: i64) -> i64 {
    let p = path_string_from_record(rec);
    let path_id = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(p));
    jet_jit_fs_write_atomic(path_id, bytes)
}

fn jet_jit_path_from(path: i64) -> i64 {
    path_record(clone_string(path))
}

fn jet_jit_path_join_handle(rec: i64, part: i64) -> i64 {
    let base = path_string_from_record(rec);
    let p = clone_string(part);
    path_record(path_kernel::jet_std_path_join(&base, &p))
}

fn jet_jit_path_parent(rec: i64) -> i64 {
    let s = path_string_from_record(rec);
    match path_kernel::jet_std_path_parent_opt(&s) {
        None => 0,
        Some(parent) => path_record(parent).wrapping_add(1),
    }
}

fn jet_jit_path_extension(rec: i64) -> i64 {
    let s = path_string_from_record(rec);
    option_string_bits(path_kernel::jet_std_path_extension_opt(&s))
}

fn jet_jit_path_stem(rec: i64) -> i64 {
    let s = path_string_from_record(rec);
    option_string_bits(path_kernel::jet_std_path_stem_opt(&s))
}

fn jet_jit_path_normalize(rec: i64) -> i64 {
    let s = path_string_from_record(rec);
    path_record(path_kernel::jet_std_path_normalize(&s))
}

fn jet_jit_path_is_within(path: i64, base: i64) -> i8 {
    let path = path_string_from_record(path);
    let base = path_string_from_record(base);
    path_kernel::jet_std_path_is_within(&path, &base) as i8
}

fn jet_jit_path_to_string(rec: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(path_string_from_record(rec)))
}

/// D-PATHFS1 / AOT `jet_path_walk` → `Vec<JetPath>` (bare list handle).
/// Must not wrap in `result_ok_bits` — callers treat the return as `List[Path]`
/// (`paths.len()` → `jet_jit_list_len`); a Result slot index panics across FFI
/// (`jit list len: bad handle` → non-unwinding abort).
fn jet_jit_path_walk(rec: i64) -> i64 {
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

fn jet_jit_fs_list_dir(path: i64) -> i64 {
    let p = clone_path_arg(path);
    if crate::fault_injection::jet_fault_should_fail("FS.Read") {
        return result_err_msg(&format!("fault injected: FS.Read for {p}"));
    }
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

fn path_record(path: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let rec = rt.heap.alloc_record(1);
        let sid = rt.heap.alloc_string(path);
        let _ = rt.heap.record_set_string(rec, 0, sid);
        rec
    })
}

fn path_string_from_record(rec: i64) -> String {
    clone_path_arg(rec)
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

pub(crate) fn jit_env_read() -> std::sync::RwLockReadGuard<'static, JitEnvEntries> {
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

// #2003 / D-ENV-MUTATE1: one raw-by-name read over Jet's logical environment
// table, for every in-process engine. The resident JIT host (`IO.rs`), the
// `core.sys` shims above, the extern "C" heap-ABI shims below and the
// interpreter ambient (`ambient_interp`) all call THIS — an engine marshals
// arguments and results, it never re-derives the lookup (I9). The AOT Prelude
// keeps its own definition because generated programs do not link jet-jit; see
// the comment on `jet_env_value_raw` in
// `jet-codegen/src/Prelude/CoreLib/Top/FSIoEnvOsTesting.rs`.
//
// Raw `OsString` on purpose: `NO_COLOR` counts by PRESENCE, so a value that is
// not valid Unicode must still read as present. Decoding here would turn
// "present but not UTF-8" into "absent" — the defect #1206's review caught in
// AOT. Callers that need a `String` go through `jit_env_value`.
pub(crate) fn jit_env_value_raw(name: &str) -> Option<std::ffi::OsString> {
    let name = std::ffi::OsStr::new(name);
    jit_env_read()
        .iter()
        .find(|(candidate, _)| jit_env_key_eq(candidate.as_os_str(), name))
        .map(|(_, value)| value.clone())
}

/// The decoding read behind `core.sys.get` (Prelude `jet_std_env_get`): a
/// non-Unicode value reads as absent to a `String` caller.
pub(crate) fn jit_env_value(name: &str) -> Option<String> {
    jit_env_value_raw(name).and_then(|value| value.into_string().ok())
}

/// The one write behind `core.sys.set` (Prelude `jet_std_env_set`): validate,
/// drop any earlier spelling of the key, then append so the last spelling
/// wins. Never touches the host process environment.
pub(crate) fn jit_env_set(name: &str, value: &str) -> Result<(), &'static str> {
    jit_env_validate_name(name)?;
    jit_env_validate_value(value)?;
    let key = std::ffi::OsString::from(name);
    let mut entries = jit_env_write();
    if let Some(old) = entries
        .iter()
        .position(|(candidate, _)| jit_env_key_eq(candidate.as_os_str(), key.as_os_str()))
    {
        entries.remove(old);
    }
    entries.push((key, std::ffi::OsString::from(value)));
    Ok(())
}
/// Remove one key from the logical environment table, preserving the
/// Prelude's validation and last-spelling-wins semantics.
pub(crate) fn jit_env_unset(name: &str) -> Result<bool, &'static str> {
    jit_env_validate_name(name)?;
    let key = std::ffi::OsStr::new(name);
    let mut entries = jit_env_write();
    let existed = entries
        .iter()
        .position(|(candidate, _)| jit_env_key_eq(candidate.as_os_str(), key))
        .map(|old| entries.remove(old))
        .is_some();
    Ok(existed)
}

/// Return the logical environment's Unicode names in the Prelude's stable
/// platform-aware order. A non-Unicode entry is an EnvError, not an omission.
pub(crate) fn jit_env_vars() -> Result<Vec<String>, &'static str> {
    let entries = jit_env_read();
    let mut names = Vec::with_capacity(entries.len());
    for (name, value) in entries.iter() {
        let Some(decoded) = name.to_str() else {
            return Err("environment contains a name or value that is not valid Unicode");
        };
        if value.to_str().is_none() {
            return Err("environment contains a name or value that is not valid Unicode");
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
    Ok(names.into_iter().map(|(_, name)| name).collect())
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

fn jet_jit_fs_remove(path: i64) -> i64 {
    let p = clone_path_arg(path);
    if crate::fault_injection::jet_fault_should_fail("FS.Write") {
        return result_err_msg(&format!("fault injected: FS.Write for {p}"));
    }
    let res = std::fs::remove_file(&p).or_else(|_| std::fs::remove_dir(&p));
    match res {
        Ok(()) => result_ok(0),
        Err(e) => result_err_msg(&format!("remove {p}: {e}")),
    }
}

fn jet_jit_fs_remove_all(path: i64) -> i64 {
    let p = clone_path_arg(path);
    if crate::fault_injection::jet_fault_should_fail("FS.Write") {
        return result_err_msg(&format!("fault injected: FS.Write for {p}"));
    }
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

fn jet_jit_fs_stat(path: i64) -> i64 {
    let path = clone_path_arg(path);
    os_rt::marshal_result(fs_prelude::jet_fs_stat(&path), |stat| {
        Concurrency::with_runtime_mut(|rt| {
            let record = rt.heap.alloc_record(9);
            let kind = rt.heap.alloc_string(stat.kind);
            let _ = rt.heap.record_set_int(record, 0, stat.size);
            let _ = rt.heap.record_set_int(record, 1, stat.modified_ms);
            let _ = rt.heap.record_set_int(record, 2, stat.created_ms);
            let _ = rt.heap.record_set_bool(record, 3, stat.readonly);
            let _ = rt.heap.record_set_bool(record, 4, stat.is_file);
            let _ = rt.heap.record_set_bool(record, 5, stat.is_dir);
            let _ = rt.heap.record_set_bool(record, 6, stat.is_symlink);
            let _ = rt.heap.record_set_string(record, 7, kind);
            let _ = rt.heap.record_set_int(record, 8, stat.mode);
            record as u64
        })
    })
}

fn jet_jit_fs_set_mode(path: i64, mode: i64) -> i64 {
    let path = clone_path_arg(path);
    os_rt::marshal_result(fs_prelude::jet_std_fs_set_mode(&path, mode), |_| 0)
}

fn jet_jit_fs_read_at(path: i64, offset: i64, len: i64) -> i64 {
    use std::io::{Read, Seek, SeekFrom};
    let p = clone_path_arg(path);
    if crate::fault_injection::jet_fault_should_fail("FS.Read") {
        return result_err_msg(&format!("fault injected: FS.Read for {p}"));
    }
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

fn jet_jit_fs_write_at(path: i64, offset: i64, bytes: i64) -> i64 {
    use std::io::{Seek, SeekFrom, Write};
    let p = clone_path_arg(path);
    let data = clone_bytes(bytes);
    if crate::fault_injection::jet_fault_should_fail("FS.Write") {
        return result_err_msg(&format!("fault injected: FS.Write for {p}"));
    }
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

fn jet_jit_fs_fsync(path: i64) -> i64 {
    let p = clone_path_arg(path);
    os_rt::marshal_result(fs_prelude::jet_std_fs_fsync(&p), |_| 0)
}

fn jet_jit_fs_write_atomic(path: i64, bytes: i64) -> i64 {
    let p = clone_path_arg(path);
    let data = clone_bytes(bytes);
    if crate::fault_injection::jet_fault_should_fail("FS.Write") {
        return result_err_msg(&format!("fault injected: FS.Write for {p}"));
    }
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
fn jet_jit_io_binwrite(path: i64, bytes: i64) -> i64 {
    let path = clone_path_arg(path);
    let bytes = clone_bytes(bytes);
    os_rt::marshal_result(fs_write_prelude::jet_std_io_binwrite(&path, &bytes), |_| 0)
}

fn jet_jit_fs_walk(path: i64, ignore_name: i64) -> i64 {
    jet_jit_fs_walk_entries(path, ignore_name, false)
}

fn jet_jit_fs_rename(from: i64, to: i64) -> i64 {
    let from = clone_path_arg(from);
    let to = clone_path_arg(to);
    os_rt::marshal_result(fs_prelude::jet_std_fs_rename(&from, &to), |_| 0)
}

fn jet_jit_fs_walk_parallel(path: i64, ignore_name: i64) -> i64 {
    jet_jit_fs_walk_entries(path, ignore_name, false)
}

fn jet_jit_fs_walk_files(path: i64, ignore_name: i64) -> i64 {
    jet_jit_fs_walk_entries(path, ignore_name, true)
}

fn jet_jit_fs_walk_entries(path: i64, ignore_name: i64, files_only: bool) -> i64 {
    let p = clone_path_arg(path);
    let ignore_name = Concurrency::with_runtime_mut(|rt| {
        let Some((present, bits)) = jit_result_parts(rt, ignore_name) else {
            return None;
        };
        if present {
            rt.heap.clone_string(bits as i64).map(Some)
        } else {
            Some(None)
        }
    });
    let Some(ignore_name) = ignore_name else {
        return result_err_msg(&format!(
            "walk {p}: ignore argument is not a valid Option<String> carrier"
        ));
    };
    if crate::fault_injection::jet_fault_should_fail("FS.Read") {
        return result_err_msg(&format!("fault injected: FS.Read for {p}"));
    }
    let result = if files_only {
        fs_walk_kernel::jet_fs_walk_files_parallel_with_ignore(
            &p,
            &p,
            ignore_name.as_deref(),
            |path, relative, is_dir, depth| (path, relative, is_dir, depth),
            |_, error| error.to_string(),
        )
    } else {
        fs_walk_kernel::jet_fs_walk_parallel_with_ignore(
            &p,
            &p,
            ignore_name.as_deref(),
            |path, relative, is_dir, depth| (path, relative, is_dir, depth),
            |_, error| error.to_string(),
        )
    };
    let mut entries = match result {
        Ok(entries) => entries,
        Err(error) => return result_err_msg(&format!("walk {p}: {error}")),
    };
    entries.sort_by(|left, right| left.0.cmp(&right.0));
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

fn jet_jit_fs_glob(pattern: i64) -> i64 {
    let pat = clone_path_arg(pattern);
    os_rt::marshal_result(fs_prelude::jet_std_fs_glob(&pat), |matches| {
        Concurrency::with_runtime_mut(|rt| {
            let list = rt.heap.alloc_empty_list();
            for path in matches {
                let sid = rt.heap.alloc_string(path);
                let _ = rt.heap.list_push_int(list, sid);
            }
            list as u64
        })
    })
}

fn jet_jit_fs_symlink(from: i64, to: i64) -> i64 {
    let src = clone_path_arg(from);
    let dst = clone_path_arg(to);
    os_rt::marshal_result(fs_prelude::jet_std_fs_symlink(&src, &dst), |_| 0)
}

fn jet_jit_fs_read_link(path: i64) -> i64 {
    let p = clone_path_arg(path);
    if crate::fault_injection::jet_fault_should_fail("FS.Read") {
        return result_err_msg(&format!("fault injected: FS.Read for {p}"));
    }
    match std::fs::read_link(&p) {
        Ok(target) => {
            let sid = Concurrency::with_runtime_mut(|rt| {
                rt.heap.alloc_string(target.to_string_lossy().to_string())
            });
            result_ok(sid as u64)
        }
        Err(e) => result_err_msg(&format!("read_link {p}: {e}")),
    }
}

fn jet_jit_fs_hard_link(from: i64, to: i64) -> i64 {
    let src = clone_path_arg(from);
    let dst = clone_path_arg(to);
    if crate::fault_injection::jet_fault_should_fail("FS.Write") {
        return result_err_msg(&format!("fault injected: FS.Write for {dst}"));
    }
    match std::fs::hard_link(&src, &dst) {
        Ok(()) => result_ok(0),
        Err(e) => result_err_msg(&format!("hard_link {dst}: {e}")),
    }
}

fn jet_jit_fs_canonicalize(path: i64) -> i64 {
    let p = clone_path_arg(path);
    os_rt::marshal_result(fs_prelude::jet_std_fs_canonicalize(&p), |value| {
        Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value)) as u64
    })
}

fn jet_jit_fs_absolute(path: i64) -> i64 {
    let path = clone_path_arg(path);
    os_rt::marshal_result(fs_prelude::jet_std_fs_absolute(&path), |value| {
        Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value)) as u64
    })
}

fn jet_jit_fs_copy_dir(from: i64, to: i64) -> i64 {
    let src = clone_path_arg(from);
    let dst = clone_path_arg(to);
    if crate::fault_injection::jet_fault_should_fail("FS.Read") {
        return result_err_msg(&format!("fault injected: FS.Read for {src}"));
    }
    if crate::fault_injection::jet_fault_should_fail("FS.Write") {
        return result_err_msg(&format!("fault injected: FS.Write for {dst}"));
    }
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

fn jet_jit_fs_temp_dir(prefix: i64) -> i64 {
    let pref = clone_string(prefix);
    let path = jet_temp_path(&pref);
    if crate::fault_injection::jet_fault_should_fail("FS.Write") {
        return result_err_msg(&format!("fault injected: FS.Write for {path}"));
    }
    match std::fs::create_dir(&path) {
        Ok(()) => result_ok(path_record(path) as u64),
        Err(e) => result_err_msg(&format!("temp_dir {path}: {e}")),
    }
}

fn jet_jit_fs_temp_file(prefix: i64) -> i64 {
    let pref = clone_string(prefix);
    let path = jet_temp_path(&pref);
    if crate::fault_injection::jet_fault_should_fail("FS.Write") {
        return result_err_msg(&format!("fault injected: FS.Write for {path}"));
    }
    match std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
    {
        Ok(_) => result_ok(path_record(path) as u64),
        Err(e) => result_err_msg(&format!("temp_file {path}: {e}")),
    }
}

fn jet_jit_fs_lock(path: i64) -> i64 {
    let p = clone_path_arg(path);
    if crate::fault_injection::jet_fault_should_fail("FS.Write") {
        return result_err_msg(&format!("fault injected: FS.Write for {p}"));
    }
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

fn jet_jit_math_sin(x: f64) -> f64 {
    x.sin()
}
fn jet_jit_math_cos(x: f64) -> f64 {
    x.cos()
}
fn jet_jit_math_exp(x: f64) -> f64 {
    x.exp()
}
fn jet_jit_math_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}
fn jet_jit_math_hypot(a: f64, b: f64) -> f64 {
    a.hypot(b)
}
fn jet_jit_math_lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
fn jet_jit_math_degrees(x: f64) -> f64 {
    x.to_degrees()
}
fn jet_jit_math_radians(x: f64) -> f64 {
    x.to_radians()
}
fn jet_jit_math_sign(x: f64) -> i64 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}

/// Packed Option<i64> ABI: `0` = None, else `bits.wrapping_add(1)`.
fn jet_jit_math_checked_add(a: i64, b: i64) -> i64 {
    match a.checked_add(b) {
        Some(v) => v.wrapping_add(1),
        None => 0,
    }
}

fn jet_jit_math_saturating_add(a: i64, b: i64) -> i64 {
    a.saturating_add(b)
}
/// Mirrors `jet_std_math_int_pow`.
fn jet_jit_math_int_pow(base: i64, exp: i64) -> i64 {
    if exp < 0 {
        return 0;
    }
    base.saturating_pow(exp as u32)
}

/// Mirrors `jet_std_math_gcd`.
fn jet_jit_math_gcd(mut a: i64, mut b: i64) -> i64 {
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
fn jet_jit_math_lcm(a: i64, b: i64) -> i64 {
    if a == 0 || b == 0 {
        0
    } else {
        (a / jet_jit_math_gcd(a, b)).saturating_mul(b).abs()
    }
}

fn jet_jit_math_sqrt(x: f64) -> f64 {
    x.sqrt()
}
fn jet_jit_math_sqrt_f32(x: f64) -> f64 {
    ((x as f32).sqrt()) as f64
}
fn jet_jit_math_pow(base: f64, exp: f64) -> f64 {
    base.powf(exp)
}
fn jet_jit_math_pow_f32(base: f64, exp: f64) -> f64 {
    ((base as f32).powf(exp as f32)) as f64
}
fn jet_jit_math_floor(x: f64) -> f64 {
    x.floor()
}
fn jet_jit_math_floor_f32(x: f64) -> f64 {
    ((x as f32).floor()) as f64
}
fn jet_jit_math_ceil(x: f64) -> f64 {
    x.ceil()
}
fn jet_jit_math_ceil_f32(x: f64) -> f64 {
    ((x as f32).ceil()) as f64
}

// ── core.sys / core.process (mirrors jet_std_env_get / jet_std_process_exit) ─

/// Option ABI: `0` = None, else string-handle+1 (same as list_get_opt).
fn jet_jit_env_get(name: i64) -> i64 {
    option_string_bits(jit_env_value(&clone_string(name)))
}

fn jet_jit_env_set(name: i64, value: i64) -> i64 {
    match jit_env_set(&clone_string(name), &clone_string(value)) {
        Ok(()) => result_ok(0),
        Err(error) => result_err_msg(error),
    }
}

fn jet_jit_env_unset(name: i64) -> i64 {
    match jit_env_unset(&clone_string(name)) {
        Ok(existed) => result_ok(u64::from(existed)),
        Err(error) => result_err_msg(error),
    }
}

fn jet_jit_env_vars() -> i64 {
    let names = match jit_env_vars() {
        Ok(names) => names,
        Err(error) => return result_err_msg(error),
    };
    let list = Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for name in names {
            let sid = rt.heap.alloc_string(name);
            let _ = rt.heap.list_push_int(list, sid);
        }
        list
    });
    result_ok(list as u64)
}

fn jet_jit_io_input(has_prompt: i8, prompt: i64) -> i64 {
    let prompt = (has_prompt != 0).then(|| clone_string(prompt));
    let s = match super::IO::prompt_input(prompt.as_deref()) {
        Ok(s) => s,
        Err(error) => return result_err_msg(&error),
    };
    let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
    result_ok(sid as u64)
}

fn jet_jit_process_exit(code: i64) {
    // Soft exit: record the code + trap so `resident_invoke` returns `Ran` with
    // that exit status. Never terminate the resident host — that would kill
    // the resident/test process (three-way battery, `jet serve`, …). The
    // recorder writes BOTH fields (`set_explicit_exit`); writing `exit_code`
    // here and then calling `set_trap` left `trapped` empty, so generated code
    // sailed past every `emit_trap_check` and the program outlived its exit.
    Concurrency::with_runtime_mut(|rt| {
        rt.set_explicit_exit(contract_kernel::jet_runtime_exit_code(code));
    });
}

// D-LIB-CALLGRANT1=A: the host only converts heap handles into the exact
// Prelude loader inputs. Identity/effect checks and native mapping stay in
// `jet_jit::Mod`, which includes the shared Mod Prelude.
fn jet_jit_mod_load(path: i64, grant: i64) -> i64 {
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

fn jet_jit_mod_on_tick(module: i64, dt: i64) -> i64 {
    match crate::Mod::on_tick(module, dt) {
        Ok(value) => result_ok(value as u64),
        Err(error) => result_err_msg(&error),
    }
}

#[derive(Clone)]
struct JitCarrierReport {
    partial: i64,
    notes: Vec<i64>,
}

/// Marshal the error report into the shared Outcome readers. The result arena,
/// record fields, and list handles are ABI facts; `jet_partial` and `jet_notes`
/// remain the only owners of carrier meaning.
fn jet_jit_carrier_fact(result: i64, field: i64, notes: i8) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some((ok, bits)) = jit_result_parts(rt, result) else {
            return 0;
        };
        let error = bits as i64;
        let partial = rt.heap.record_get_int(error, field).unwrap_or(0);
        let note_values = if notes != 0 {
            let list = rt.heap.record_get_int(error, field).unwrap_or(0);
            let len = rt.heap.list_len(list).unwrap_or(0);
            (0..len)
                .map(|index| rt.heap.list_get_int(list, index).unwrap_or(0))
                .collect()
        } else {
            Vec::new()
        };
        let outcome: jet_foundation::Outcome::JetOutcome<(), JitCarrierReport> = if ok {
            Ok(())
        } else {
            Err(JitCarrierReport {
                partial,
                notes: note_values,
            })
        };
        if notes != 0 {
            let values =
                jet_foundation::Outcome::jet_notes(&outcome, |report| report.notes.clone());
            let list = rt.heap.alloc_empty_list();
            for value in values {
                let _ = rt.heap.list_push_int(list, value);
            }
            list
        } else {
            match jet_foundation::Outcome::jet_partial(&outcome, |report| report.partial) {
                Ok(value) => value.saturating_add(1),
                Err(_) => 0,
            }
        }
    })
}

host_fns! {
    struct CoreHostFns;
    register: register_core_host_symbols;
    declare: declare_core_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::Module;
        let cc = module.target_config().default_call_conv;
        let mut sig_str = Signature::new(cc);
        sig_str.returns.push(AbiParam::new(types::I64));
        let mut sig_i64 = Signature::new(cc);
        sig_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_f64 = Signature::new(cc);
        sig_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_keep_i64 = Signature::new(cc);
        sig_keep_i64.params.push(AbiParam::new(types::I64));
        sig_keep_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_keep_f64 = Signature::new(cc);
        sig_keep_f64.params.push(AbiParam::new(types::F64));
        sig_keep_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_keep_i8 = Signature::new(cc);
        sig_keep_i8.params.push(AbiParam::new(types::I8));
        sig_keep_i8.returns.push(AbiParam::new(types::I8));
        let mut sig_keep_i32 = Signature::new(cc);
        sig_keep_i32.params.push(AbiParam::new(types::I32));
        sig_keep_i32.returns.push(AbiParam::new(types::I32));
        let sig_keep_unit = Signature::new(cc);
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
        let mut sig_str_f64_str = Signature::new(cc);
        sig_str_f64_str.params.push(AbiParam::new(types::I64));
        sig_str_f64_str.params.push(AbiParam::new(types::F64));
        sig_str_f64_str.returns.push(AbiParam::new(types::I64));
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
        let mut sig_i64_i64_i8 = Signature::new(cc);
        sig_i64_i64_i8.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i8.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i8.params.push(AbiParam::new(types::I8));
        sig_i64_i64_i8.returns.push(AbiParam::new(types::I64));
        let mut sig_path_is_within = Signature::new(cc);
        sig_path_is_within.params.push(AbiParam::new(types::I64));
        sig_path_is_within.params.push(AbiParam::new(types::I64));
        sig_path_is_within.returns.push(AbiParam::new(types::I8));


    }
    keep_i64: "jet_jit_keep_i64" => jet_jit_keep_i64: sig_keep_i64;
    checked_keep: "jet_keep" => jet_jit_keep_i64: sig_keep_i64;
    keep_f64: "jet_jit_keep_f64" => jet_jit_keep_f64: sig_keep_f64;
    keep_i8: "jet_jit_keep_i8" => jet_jit_keep_i8: sig_keep_i8;
    keep_i32: "jet_jit_keep_i32" => jet_jit_keep_i32: sig_keep_i32;
    keep_unit: "jet_jit_keep_unit" => jet_jit_keep_unit: sig_keep_unit;
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
    os_set_current_dir: "jet_jit_os_set_current_dir" => jet_jit_os_set_current_dir: sig_unary_i64;

    os_fork: "jet_jit_os_fork" => jet_jit_os_fork: sig_i64;
    os_setuid: "jet_jit_os_setuid" => jet_jit_os_setuid: sig_unary_i64;
    os_setgid: "jet_jit_os_setgid" => jet_jit_os_setgid: sig_unary_i64;
    os_setsid: "jet_jit_os_setsid" => jet_jit_os_setsid: sig_i64;
    os_initgroups: "jet_jit_os_initgroups" => jet_jit_os_initgroups: sig_i64_i64_i64;
    os_wait: "jet_jit_os_wait" => jet_jit_os_wait: sig_i64;
    core_os_on_interrupt: "jet_std_os_on_interrupt" => jet_jit_core_os_on_interrupt: sig_unary_i64;
    os_waitpid: "jet_jit_os_waitpid" => jet_jit_os_waitpid: sig_i64_i64_i64;
    os_utime: "jet_jit_os_utime" => jet_jit_os_utime: sig_i64_i64_i64_i64;
    os_on_interrupt: "jet_jit_os_on_interrupt" => jet_jit_os_on_interrupt: sig_void_i64;
    os_atexit: "jet_jit_os_atexit" => jet_jit_os_atexit: sig_unary_i64;
    os_stop: "jet_jit_os_stop" => jet_jit_os_stop: sig_void_i64;
    log_set_level: "jet_jit_log_set_level" => jet_jit_log_set_level: sig_void_str;
    log_set_trace_id: "jet_jit_log_set_trace_id" => jet_jit_log_set_trace_id: sig_void_str;
    log_setup: "jet_jit_log_setup" => jet_jit_log_setup: sig_void_str;
    log_set_sink: "jet_jit_log_set_sink" => jet_jit_log_set_sink: sig_void_i64_i64;
    log_sample_every: "jet_jit_log_sample_every" => jet_jit_log_sample_every: sig_void_i64;
    log_otlp_file: "jet_jit_log_otlp_file" => jet_jit_log_otlp_file: sig_void_str;
    log_debug: "jet_jit_log_debug" => jet_jit_log_debug: sig_void_str;
    log_info: "jet_jit_log_info" => jet_jit_log_info: sig_void_str;
    log_warn: "jet_jit_log_warn" => jet_jit_log_warn: sig_void_str;
    log_error: "jet_jit_log_error" => jet_jit_log_error: sig_void_str;
    log_critical: "jet_jit_log_critical" => jet_jit_log_critical: sig_void_str;
    log_fatal: "jet_jit_log_fatal" => jet_jit_log_fatal: sig_void_str;
    log_disable: "jet_jit_log_disable" => jet_jit_log_disable: sig_void;
    log_flush: "jet_jit_log_flush" => jet_jit_log_flush: sig_void;
    log_enabled: "jet_jit_log_enabled" => jet_jit_log_enabled: sig_i64_i8;
    log_field: "jet_jit_log_field" => jet_jit_log_field: sig_str_str_str;
    log_int: "jet_jit_log_int" => jet_jit_log_int: sig_str_i64_str;
    log_float: "jet_jit_log_float" => jet_jit_log_float: sig_str_f64_str;
    log_bool: "jet_jit_log_bool" => jet_jit_log_bool: sig_str_i8_str;
    log_redact: "jet_jit_log_redact" => jet_jit_log_redact: sig_unary_i64;
    log_counter: "jet_jit_log_counter" => jet_jit_log_counter: sig_str_i64_str;
    log_span: "jet_jit_log_span" => jet_jit_log_span: sig_unary_i64;
    log_enter: "jet_jit_log_enter" => jet_jit_log_enter: sig_void_i64;
    log_close: "jet_jit_log_close" => jet_jit_log_close: sig_void_i64;
    log_debug_fields: "jet_jit_log_debug_fields" => jet_jit_log_debug_fields: sig_void_i64_i64;
    log_info_fields: "jet_jit_log_info_fields" => jet_jit_log_info_fields: sig_void_i64_i64;
    log_warn_fields: "jet_jit_log_warn_fields" => jet_jit_log_warn_fields: sig_void_i64_i64;
    log_error_fields: "jet_jit_log_error_fields" => jet_jit_log_error_fields: sig_void_i64_i64;
    fs_exists: "jet_jit_fs_exists" => jet_jit_fs_exists: sig_i64_i8;
    fs_is_dir: "jet_jit_fs_is_dir" => jet_jit_fs_is_dir: sig_i64_i8;
    fs_remove_dir: "jet_jit_fs_remove_dir" => jet_jit_fs_remove_dir: sig_unary_i64;
    fs_read: "jet_jit_fs_read" => jet_jit_fs_read: sig_unary_i64;
    fs_scope: "jet_jit_fs_scope" => jet_jit_fs_scope: sig_unary_i64;
    fs_scope_read: "jet_jit_fs_scope_read" => jet_jit_fs_scope_read: sig_i64_i64_i64;
    fs_read_bytes: "jet_jit_fs_read_bytes" => jet_jit_fs_read_bytes: sig_unary_i64;
    fs_map: "jet_jit_fs_map" => jet_jit_fs_map: sig_unary_i64;
    fs_map_window_view: "jet_jit_fs_map_window_view" => jet_jit_fs_map_window_view: sig_i64_i64_i64_i64;
    fs_map_window_len_view: "jet_jit_fs_map_window_len_view" => jet_jit_fs_map_window_len_view: sig_i64_i64_i64_i64;
    fs_map_lines_view: "jet_jit_fs_map_lines_view" => jet_jit_fs_map_lines_view: sig_unary_i64;
    fs_map_len: "jet_jit_fs_map_len" => jet_jit_fs_map_len: sig_unary_i64;
    fs_map_is_empty: "jet_jit_fs_map_is_empty" => jet_jit_fs_map_is_empty: sig_i64_i8;
    fs_write: "jet_jit_fs_write" => jet_jit_fs_write: sig_i64_i64_i64;
    fs_append: "jet_jit_fs_append" => jet_jit_fs_append: sig_unary_i64;
    fs_append_all: "jet_jit_fs_append_all" => jet_jit_fs_append_all: sig_i64_i64_i64;
    fs_write_bytes: "jet_jit_fs_write_bytes" => jet_jit_fs_write_bytes: sig_i64_i64_i64;
    io_binwrite: "jet_jit_io_binwrite" => jet_jit_io_binwrite: sig_i64_i64_i64;
    fs_stat: "jet_jit_fs_stat" => jet_jit_fs_stat: sig_unary_i64;
    fs_set_mode: "jet_jit_fs_set_mode" => jet_jit_fs_set_mode: sig_i64_i64_i64;
    fs_create_dir: "jet_jit_fs_create_dir" => jet_jit_fs_create_dir: sig_unary_i64;
    fs_create_dir_all: "jet_jit_fs_create_dir_all" => jet_jit_fs_create_dir_all: sig_unary_i64;
    fs_list_dir: "jet_jit_fs_list_dir" => jet_jit_fs_list_dir: sig_unary_i64;
    fs_remove_all: "jet_jit_fs_remove_all" => jet_jit_fs_remove_all: sig_unary_i64;
    fs_remove: "jet_jit_fs_remove" => jet_jit_fs_remove: sig_unary_i64;
    fs_read_at: "jet_jit_fs_read_at" => jet_jit_fs_read_at: sig_i64_i64_i64_i64;
    fs_write_at: "jet_jit_fs_write_at" => jet_jit_fs_write_at: sig_i64_i64_i64_i64;
    fs_fsync: "jet_jit_fs_fsync" => jet_jit_fs_fsync: sig_unary_i64;
    fs_write_atomic: "jet_jit_fs_write_atomic" => jet_jit_fs_write_atomic: sig_i64_i64_i64;
    fs_walk: "jet_jit_fs_walk" => jet_jit_fs_walk: sig_i64_i64_i64;
    fs_walk_parallel: "jet_jit_fs_walk_parallel" => jet_jit_fs_walk_parallel: sig_i64_i64_i64;
    fs_walk_files: "jet_jit_fs_walk_files" => jet_jit_fs_walk_files: sig_i64_i64_i64;
    fs_rename: "jet_jit_fs_rename" => jet_jit_fs_rename: sig_i64_i64_i64;
    fs_glob: "jet_jit_fs_glob" => jet_jit_fs_glob: sig_unary_i64;
    fs_symlink: "jet_jit_fs_symlink" => jet_jit_fs_symlink: sig_i64_i64_i64;
    fs_read_link: "jet_jit_fs_read_link" => jet_jit_fs_read_link: sig_unary_i64;
    fs_hard_link: "jet_jit_fs_hard_link" => jet_jit_fs_hard_link: sig_i64_i64_i64;
    fs_canonicalize: "jet_jit_fs_canonicalize" => jet_jit_fs_canonicalize: sig_unary_i64;
    fs_absolute: "jet_jit_fs_absolute" => jet_jit_fs_absolute: sig_unary_i64;
    fs_copy_dir: "jet_jit_fs_copy_dir" => jet_jit_fs_copy_dir: sig_i64_i64_i64;
    fs_copy: "jet_jit_fs_copy" => jet_jit_fs_copy: sig_i64_i64_i64;
    fs_temp_dir: "jet_jit_fs_temp_dir" => jet_jit_fs_temp_dir: sig_unary_i64;
    fs_temp_file: "jet_jit_fs_temp_file" => jet_jit_fs_temp_file: sig_unary_i64;
    fs_lock: "jet_jit_fs_lock" => jet_jit_fs_lock: sig_unary_i64;
    mod_load: "jet_jit_mod_load" => jet_jit_mod_load: sig_i64_i64_i64;
    mod_on_tick: "jet_jit_mod_on_tick" => jet_jit_mod_on_tick: sig_i64_i64_i64;
    // #1992 io/path: `Path.home()` takes no argument at any tier. CoreLib's body
    // is `fn jet_path_home() -> JetPath` (jet-codegen Prelude/CoreLib/Top/
    // PathFiles.rs), AOT emits `jet_path_home()` and comptime eval calls
    // `path_kernel::jet_std_path_home()` — all receiver-free, safety.rs gates the
    // op on `args.is_empty()`, and the host itself is `jet_jit_path_home() -> i64`
    // (above). This row said `sig_unary_i64`, copy-pasted from the `path_from` row
    // below, so lowering's correct 0-arg call was invalid CLIF against a 1-param
    // declaration. Nullary like the other ambient-query hosts (`os_temp_dir`,
    // `env_vars`): the runtime reaches this host through
    // `Concurrency::with_runtime_mut`, never through a parameter.
    path_home: "jet_path_home" => jet_jit_path_home: sig_i64;
    path_from: "jet_path_from" => jet_jit_path_from: sig_unary_i64;
    path_write_atomic: "jet_path_write_atomic" => jet_jit_path_write_atomic: sig_i64_i64_i64;
    path_join_handle: "jet_path_join" => jet_jit_path_join_handle: sig_i64_i64_i64;
    path_parent: "jet_path_parent" => jet_jit_path_parent: sig_unary_i64;
    path_extension: "jet_path_extension" => jet_jit_path_extension: sig_unary_i64;
    path_stem: "jet_path_stem" => jet_jit_path_stem: sig_unary_i64;
    path_normalize: "jet_path_normalize" => jet_jit_path_normalize: sig_unary_i64;
    path_is_within: "jet_path_is_within" => jet_jit_path_is_within: sig_path_is_within;
    path_to_string: "jet_path_to_string" => jet_jit_path_to_string: sig_unary_i64;
    path_walk: "jet_path_walk" => jet_jit_path_walk: sig_unary_i64;
    math_sin: "jet_jit_math_sin" => jet_jit_math_sin: sig_f64_f64;
    math_cos: "jet_jit_math_cos" => jet_jit_math_cos: sig_f64_f64;
    math_exp: "jet_jit_math_exp" => jet_jit_math_exp: sig_f64_f64;
    math_atan2: "jet_jit_math_atan2" => jet_jit_math_atan2: sig_f64_f64_f64;
    math_hypot: "jet_jit_math_hypot" => jet_jit_math_hypot: sig_f64_f64_f64;
    math_lerp: "jet_jit_math_lerp" => jet_jit_math_lerp: sig_lerp;
    math_degrees: "jet_jit_math_degrees" => jet_jit_math_degrees: sig_f64_f64;
    math_radians: "jet_jit_math_radians" => jet_jit_math_radians: sig_f64_f64;
    math_sign: "jet_jit_math_sign" => jet_jit_math_sign: sig_f64_i64;
    math_checked_add: "jet_jit_math_checked_add" => jet_jit_math_checked_add: sig_i64_i64_i64;
    math_saturating_add: "jet_jit_math_saturating_add" => jet_jit_math_saturating_add: sig_i64_i64_i64;
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
    env_current_dir: "jet_jit_env_current_dir" => jet_jit_env_current_dir: sig_i64;
    env_home_dir: "jet_jit_env_home_dir" => jet_jit_env_home_dir: sig_i64;
    io_input: "jet_jit_io_input" => jet_jit_io_input: sig_i8_i64_i64;
    carrier_fact: "jet_jit_carrier_fact" => jet_jit_carrier_fact: sig_i64_i64_i8;
    process_exit: "jet_jit_process_exit" => jet_jit_process_exit: sig_void_i64;
}
