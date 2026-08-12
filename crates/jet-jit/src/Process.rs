//! Resident JIT process adapters.
//!
//! Process policy and lifecycle semantics live in the shared Prelude included
//! by the interpreter. This module only stores opaque handles and marshals
//! values and typed errors across the Cranelift ABI.

use super::Concurrency;
use crate::ambient_interp::process_prelude;
use crate::Marshal::{clone_string, result_ok};
use jet_foundation::Outcome::JetAbsent;

pub(crate) type JitProcessSpec = process_prelude::ProcessSpec;
pub(crate) type JitProcessChild = process_prelude::ProcessChild;

fn clone_string_list(list: i64) -> Vec<String> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let sid = rt.heap.list_get_int(list, i).unwrap_or(0);
            out.push(rt.heap.clone_string(sid).unwrap_or_default());
        }
        out
    })
}

fn alloc_process_result(out: &process_prelude::ProcessResult) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        // Field order: code, output, errors, success, signal, timed_out.
        let record = rt.heap.alloc_record(6);
        let _ = rt.heap.record_set_int(record, 0, out.code);
        let output = rt.heap.alloc_string(out.output.clone());
        let _ = rt.heap.record_set_string(record, 1, output);
        let errors = rt.heap.alloc_string(out.errors.clone());
        let _ = rt.heap.record_set_string(record, 2, errors);
        let _ = rt.heap.record_set_bool(record, 3, out.success);
        // A present narrow value is encoded as handle+1; zero is absent.
        let signal = out.signal.map(|value| value.wrapping_add(1)).unwrap_or(0);
        let _ = rt.heap.record_set_int(record, 4, signal);
        let _ = rt.heap.record_set_bool(record, 5, out.timed_out);
        record
    })
}

fn outcome_to_result(out: process_prelude::ProcessResult) -> i64 {
    result_ok(alloc_process_result(&out) as u64)
}

fn process_io_operation_bits(operation: process_prelude::IOOperation) -> i64 {
    match operation {
        process_prelude::IOOperation::Read => 0,
        process_prelude::IOOperation::Write => 1,
        process_prelude::IOOperation::Flush => 2,
        process_prelude::IOOperation::Connect => 3,
        process_prelude::IOOperation::Accept => 4,
        process_prelude::IOOperation::Close => 5,
        process_prelude::IOOperation::Resolve => 6,
        process_prelude::IOOperation::Codec => 7,
    }
}

fn process_io_error_result(error: process_prelude::IOError) -> i64 {
    let (variant, context) = match error {
        process_prelude::IOError::InvalidInput(context) => (0, context),
        process_prelude::IOError::NotFound(context) => (1, context),
        process_prelude::IOError::PermissionDenied(context) => (2, context),
        process_prelude::IOError::TimedOut(context) => (3, context),
        process_prelude::IOError::Cancelled(context) => (4, context),
        process_prelude::IOError::Closed(context) => (5, context),
        process_prelude::IOError::Protocol(context) => (6, context),
        process_prelude::IOError::Other(context) => (7, context),
    };
    Concurrency::with_runtime_mut(|rt| {
        let process_prelude::IOContext {
            operation,
            resource,
            os_code,
            cause,
        } = context;
        let record = rt.heap.alloc_record(4);
        let _ = rt
            .heap
            .record_set_int(record, 0, process_io_operation_bits(operation));
        let resource = match resource {
            Ok(resource) => rt.heap.alloc_string(resource).wrapping_add(1),
            Err(JetAbsent) => 0,
        };
        let _ = rt.heap.record_set_int(record, 1, resource);
        let os_code = match os_code {
            Ok(os_code) => os_code.wrapping_add(1),
            Err(JetAbsent) => 0,
        };
        let _ = rt.heap.record_set_int(record, 2, os_code);
        let cause = match cause {
            Ok(cause) => rt.heap.alloc_string(cause).wrapping_add(1),
            Err(JetAbsent) => 0,
        };
        let _ = rt.heap.record_set_int(record, 3, cause);
        rt.results.push(super::JitResultValue {
            ok: false,
            bits: record.wrapping_shl(8).wrapping_add(variant) as u64,
        });
        rt.results.len() as i64
    })
}

fn process_error(
    operation: process_prelude::IOOperation,
    resource: Option<String>,
    cause: impl ToString,
) -> i64 {
    process_io_error_result(process_prelude::IOError::other(operation, resource, cause))
}

fn invalid_process_spec() -> i64 {
    process_error(
        process_prelude::IOOperation::Resolve,
        Some("ProcessSpec".to_string()),
        "invalid ProcessSpec",
    )
}

fn invalid_process_child() -> i64 {
    process_error(
        process_prelude::IOOperation::Close,
        Some("ProcessChild".to_string()),
        "invalid ProcessChild",
    )
}

fn push_spec(spec: JitProcessSpec) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.process_specs.push(spec);
        rt.process_specs.len() as i64
    })
}

fn update_spec(handle: i64, f: impl FnOnce(JitProcessSpec) -> JitProcessSpec) -> i64 {
    if handle <= 0 {
        return 0;
    }
    let idx = (handle as usize).saturating_sub(1);
    Concurrency::with_runtime_mut(|rt| {
        let Some(spec) = rt.process_specs.get_mut(idx) else {
            return 0;
        };
        let current = spec.clone();
        *spec = f(current);
        handle
    })
}

fn clone_spec(handle: i64) -> Option<JitProcessSpec> {
    if handle <= 0 {
        return None;
    }
    let idx = (handle as usize).saturating_sub(1);
    Concurrency::with_runtime_mut(|rt| rt.process_specs.get(idx).cloned())
}

fn process_stream_mode(disc: i64) -> process_prelude::ProcessStreamMode {
    match disc {
        1 => process_prelude::ProcessStreamMode::Inherit,
        2 => process_prelude::ProcessStreamMode::Capture,
        _ => process_prelude::ProcessStreamMode::Stream,
    }
}

fn process_duration(ns: i64) -> process_prelude::Duration {
    process_prelude::Duration { ns }
}

extern "C" fn jet_jit_process_cmd(cmd_list: i64) -> i64 {
    push_spec(process_prelude::spec_new(clone_string_list(cmd_list)))
}

extern "C" fn jet_jit_process_run(cmd_list: i64) -> i64 {
    let spec = process_prelude::spec_new(clone_string_list(cmd_list));
    match process_prelude::spec_run(&spec) {
        Ok(result) => outcome_to_result(result),
        Err(error) => process_io_error_result(error),
    }
}

extern "C" fn jet_jit_process_pipeline(spec_list: i64) -> i64 {
    let handles = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(spec_list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(spec_list, i).unwrap_or(0));
        }
        out
    });
    let mut specs = Vec::with_capacity(handles.len());
    for handle in handles {
        let Some(spec) = clone_spec(handle) else {
            return invalid_process_spec();
        };
        specs.push(spec);
    }
    match process_prelude::spec_pipeline(&specs) {
        Ok(result) => outcome_to_result(result),
        Err(error) => process_io_error_result(error),
    }
}

extern "C" fn jet_jit_process_spec_stdout(spec: i64, mode: i64) -> i64 {
    let mode = process_stream_mode(mode);
    update_spec(spec, |spec| process_prelude::spec_stdout(spec, &mode))
}

extern "C" fn jet_jit_process_spec_stderr(spec: i64, mode: i64) -> i64 {
    let mode = process_stream_mode(mode);
    update_spec(spec, |spec| process_prelude::spec_stderr(spec, &mode))
}

extern "C" fn jet_jit_process_spec_stdin(spec: i64, mode: i64) -> i64 {
    let mode = process_stream_mode(mode);
    update_spec(spec, |spec| process_prelude::spec_stdin(spec, &mode))
}

extern "C" fn jet_jit_process_spec_timeout(spec: i64, timeout: i64) -> i64 {
    let timeout = process_duration(timeout);
    update_spec(spec, |spec| process_prelude::spec_timeout(spec, &timeout))
}

extern "C" fn jet_jit_process_spec_output_limit(spec: i64, limit: i64) -> i64 {
    update_spec(spec, |spec| process_prelude::spec_output_limit(spec, limit))
}

extern "C" fn jet_jit_process_spec_run(spec: i64) -> i64 {
    let Some(spec) = clone_spec(spec) else {
        return invalid_process_spec();
    };
    match process_prelude::spec_run(&spec) {
        Ok(result) => outcome_to_result(result),
        Err(error) => process_io_error_result(error),
    }
}

extern "C" fn jet_jit_process_spec_run_checked(spec: i64) -> i64 {
    let Some(spec) = clone_spec(spec) else {
        return invalid_process_spec();
    };
    match process_prelude::spec_run_checked(&spec) {
        Ok(result) => outcome_to_result(result),
        Err(error) => process_io_error_result(error),
    }
}

extern "C" fn jet_jit_process_spec_spawn(spec: i64) -> i64 {
    let Some(spec) = clone_spec(spec) else {
        return invalid_process_spec();
    };
    let child = match process_prelude::spec_spawn(&spec) {
        Ok(child) => child,
        Err(error) => return process_io_error_result(error),
    };
    let handle = Concurrency::with_runtime_mut(|rt| {
        rt.process_children.push(child);
        rt.process_children.len() as i64
    });
    result_ok(handle as u64)
}

extern "C" fn jet_jit_process_spec_env_clear(spec: i64) -> i64 {
    update_spec(spec, process_prelude::spec_env_clear)
}

extern "C" fn jet_jit_process_spec_detached(spec: i64) -> i64 {
    update_spec(spec, process_prelude::spec_detached)
}

fn terminal_policy_from_handle(policy: i64) -> process_prelude::TerminalPolicy {
    let (cols, rows, raw) = Concurrency::with_runtime_mut(|rt| {
        let size = rt.heap.record_get_int(policy, 0).unwrap_or(0);
        (
            rt.heap.record_get_int(size, 0).unwrap_or(0),
            rt.heap.record_get_int(size, 1).unwrap_or(0),
            rt.heap.record_get_int(policy, 1).unwrap_or(1) == 0,
        )
    });
    process_prelude::TerminalPolicy {
        size: process_prelude::TerminalSize { cols, rows },
        mode: if raw {
            process_prelude::TerminalMode::Raw
        } else {
            process_prelude::TerminalMode::Cooked
        },
    }
}

fn terminal_size_from_handle(size: i64) -> process_prelude::TerminalSize {
    Concurrency::with_runtime_mut(|rt| process_prelude::TerminalSize {
        cols: rt.heap.record_get_int(size, 0).unwrap_or(0),
        rows: rt.heap.record_get_int(size, 1).unwrap_or(0),
    })
}

extern "C" fn jet_jit_process_spec_terminal(spec: i64) -> i64 {
    update_spec(spec, process_prelude::spec_terminal)
}

extern "C" fn jet_jit_process_spec_terminal_with_policy(spec: i64, policy: i64) -> i64 {
    let policy = terminal_policy_from_handle(policy);
    update_spec(spec, |spec| {
        process_prelude::spec_terminal_with_policy(spec, &policy)
    })
}

extern "C" fn jet_jit_process_spec_capabilities(spec: i64) -> i64 {
    let Some(spec) = clone_spec(spec) else {
        return 0;
    };
    let facts = process_prelude::spec_capabilities(&spec);
    Concurrency::with_runtime_mut(|rt| {
        let facts = facts
            .into_iter()
            .map(|fact| rt.heap.alloc_string(fact))
            .collect();
        rt.sets.push(facts);
        rt.set_string_kinds.push(true);
        rt.sets.len() as i64
    })
}

extern "C" fn jet_jit_process_child_terminal(child: i64) -> i64 {
    if child <= 0 {
        return 0;
    }
    let idx = (child as usize).saturating_sub(1);
    Concurrency::with_runtime_mut(|rt| {
        rt.process_children
            .get(idx)
            .and_then(|child| child.terminal.as_ref().ok())
            .map(|_| child)
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_terminal_session_resize(session: i64, size: i64) -> i64 {
    if session <= 0 {
        return invalid_process_child();
    }
    let size = terminal_size_from_handle(size);
    let idx = (session as usize).saturating_sub(1);
    let result = Concurrency::with_runtime_mut(|rt| {
        let Some(child) = rt.process_children.get(idx) else {
            return None;
        };
        let Some(session) = child.terminal.as_ref().ok() else {
            return Some(Err(process_prelude::IOError::other(
                process_prelude::IOOperation::Resolve,
                Some("process terminal".to_string()),
                "this child has no terminal session",
            )));
        };
        Some(process_prelude::terminal_session_resize(&session, &size))
    });
    match result {
        Some(Ok(())) => result_ok(0),
        Some(Err(error)) => process_io_error_result(error),
        None => invalid_process_child(),
    }
}

extern "C" fn jet_jit_process_spec_cwd(spec: i64, cwd: i64) -> i64 {
    let cwd = clone_string(cwd);
    update_spec(spec, |spec| process_prelude::spec_cwd(spec, &cwd))
}

extern "C" fn jet_jit_process_spec_env(spec: i64, name: i64, value: i64) -> i64 {
    let name = clone_string(name);
    let value = clone_string(value);
    update_spec(spec, |spec| process_prelude::spec_env(spec, &name, &value))
}

extern "C" fn jet_jit_process_spec_env_remove(spec: i64, name: i64) -> i64 {
    let name = clone_string(name);
    update_spec(spec, |spec| process_prelude::spec_env_remove(spec, &name))
}

/// 0 selects stdout; 1 selects stderr.
extern "C" fn jet_jit_process_stream_lines(child: i64, tag: i64) -> i64 {
    let reader = if child <= 0 {
        None
    } else {
        let idx = (child as usize).saturating_sub(1);
        Concurrency::with_runtime_mut(|rt| {
            rt.process_children.get(idx).map(|child| {
                if tag == 1 {
                    std::rc::Rc::clone(&child.stderr)
                } else {
                    std::rc::Rc::clone(&child.stdout)
                }
            })
        })
    };
    let Some(reader) = reader else {
        return Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_list());
    };
    let mut lines = Vec::new();
    loop {
        match process_prelude::stream_next_line(&reader) {
            Ok(Some(line)) => lines.push(line),
            Ok(None) | Err(_) => break,
        }
    }
    *reader.borrow_mut() = None;
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for line in lines {
            let sid = rt.heap.alloc_string(line);
            let _ = rt.heap.list_push_int(list, sid);
        }
        list
    })
}

extern "C" fn jet_jit_process_child_id(child: i64) -> i64 {
    if child <= 0 {
        return 0;
    }
    let idx = (child as usize).saturating_sub(1);
    Concurrency::with_runtime_mut(|rt| {
        rt.process_children
            .get(idx)
            .map(process_prelude::child_id)
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_process_child_exited(child: i64) -> i64 {
    if child <= 0 {
        return invalid_process_child();
    }
    let idx = (child as usize).saturating_sub(1);
    let result = Concurrency::with_runtime_mut(|rt| {
        rt.process_children
            .get(idx)
            .map(process_prelude::child_exited)
    });
    match result {
        Some(Ok(exited)) => result_ok(u64::from(exited)),
        Some(Err(error)) => process_io_error_result(error),
        None => invalid_process_child(),
    }
}

fn process_child_unit(
    child: i64,
    operation: fn(&process_prelude::ProcessChild) -> Result<(), process_prelude::IOError>,
) -> i64 {
    if child <= 0 {
        return invalid_process_child();
    }
    let idx = (child as usize).saturating_sub(1);
    let result = Concurrency::with_runtime_mut(|rt| {
        rt.process_children
            .get(idx)
            .map(|child| operation(child))
    });
    match result {
        Some(Ok(())) => result_ok(0),
        Some(Err(error)) => process_io_error_result(error),
        None => invalid_process_child(),
    }
}

extern "C" fn jet_jit_process_child_kill(child: i64) -> i64 {
    process_child_unit(child, process_prelude::child_kill)
}

extern "C" fn jet_jit_process_child_terminate(child: i64) -> i64 {
    process_child_unit(child, process_prelude::child_terminate)
}

extern "C" fn jet_jit_process_child_interrupt(child: i64) -> i64 {
    process_child_unit(child, process_prelude::child_interrupt)
}

extern "C" fn jet_jit_process_child_wait(child: i64) -> i64 {
    if child <= 0 {
        return invalid_process_child();
    }
    let idx = (child as usize).saturating_sub(1);
    let process = Concurrency::with_runtime_mut(|rt| rt.process_children.get(idx).cloned());
    match process {
        Some(process) => match process_prelude::child_wait(&process) {
            Ok(result) => outcome_to_result(result),
            Err(error) => process_io_error_result(error),
        },
        None => invalid_process_child(),
    }
}

host_fns! {
    struct ProcessHostFns;
    register: register_process_symbols;
    declare: declare_process_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::{Linkage, Module};
        let cc = module.target_config().default_call_conv;
        let mut sig_unary = Signature::new(cc);
        sig_unary.params.push(AbiParam::new(types::I64));
        sig_unary.returns.push(AbiParam::new(types::I64));
        let mut sig_binary = Signature::new(cc);
        sig_binary.params.push(AbiParam::new(types::I64));
        sig_binary.params.push(AbiParam::new(types::I64));
        sig_binary.returns.push(AbiParam::new(types::I64));
        let mut sig_ternary = Signature::new(cc);
        sig_ternary.params.push(AbiParam::new(types::I64));
        sig_ternary.params.push(AbiParam::new(types::I64));
        sig_ternary.params.push(AbiParam::new(types::I64));
        sig_ternary.returns.push(AbiParam::new(types::I64));


    }
    cmd: "jet_jit_process_cmd" => jet_jit_process_cmd: sig_unary;
    run: "jet_jit_process_run" => jet_jit_process_run: sig_unary;
    pipeline: "jet_jit_process_pipeline" => jet_jit_process_pipeline: sig_unary;
    spec_stdout: "jet_jit_process_spec_stdout" => jet_jit_process_spec_stdout: sig_binary;
    spec_stderr: "jet_jit_process_spec_stderr" => jet_jit_process_spec_stderr: sig_binary;
    spec_stdin: "jet_jit_process_spec_stdin" => jet_jit_process_spec_stdin: sig_binary;
    spec_timeout: "jet_jit_process_spec_timeout" => jet_jit_process_spec_timeout: sig_binary;
    spec_output_limit: "jet_jit_process_spec_output_limit" => jet_jit_process_spec_output_limit: sig_binary;
    spec_cwd: "jet_jit_process_spec_cwd" => jet_jit_process_spec_cwd: sig_binary;
    spec_env: "jet_jit_process_spec_env" => jet_jit_process_spec_env: sig_ternary;
    spec_env_remove: "jet_jit_process_spec_env_remove" => jet_jit_process_spec_env_remove: sig_binary;
    spec_env_clear: "jet_jit_process_spec_env_clear" => jet_jit_process_spec_env_clear: sig_unary;
    spec_detached: "jet_jit_process_spec_detached" => jet_jit_process_spec_detached: sig_unary;
    spec_terminal: "jet_jit_process_spec_terminal" => jet_jit_process_spec_terminal: sig_unary;
    spec_terminal_with_policy: "jet_jit_process_spec_terminal_with_policy" => jet_jit_process_spec_terminal_with_policy: sig_binary;
    spec_capabilities: "jet_jit_process_spec_capabilities" => jet_jit_process_spec_capabilities: sig_unary;
    spec_run: "jet_jit_process_spec_run" => jet_jit_process_spec_run: sig_unary;
    spec_run_checked: "jet_jit_process_spec_run_checked" => jet_jit_process_spec_run_checked: sig_unary;
    spec_spawn: "jet_jit_process_spec_spawn" => jet_jit_process_spec_spawn: sig_unary;
    child_id: "jet_jit_process_child_id" => jet_jit_process_child_id: sig_unary;
    child_exited: "jet_jit_process_child_exited" => jet_jit_process_child_exited: sig_unary;
    child_terminal: "jet_jit_process_child_terminal" => jet_jit_process_child_terminal: sig_unary;
    child_kill: "jet_jit_process_child_kill" => jet_jit_process_child_kill: sig_unary;
    child_terminate: "jet_jit_process_child_terminate" => jet_jit_process_child_terminate: sig_unary;
    child_interrupt: "jet_jit_process_child_interrupt" => jet_jit_process_child_interrupt: sig_unary;
    child_wait: "jet_jit_process_child_wait" => jet_jit_process_child_wait: sig_unary;
    terminal_resize: "jet_jit_terminal_session_resize" => jet_jit_terminal_session_resize: sig_binary;
    stream_lines: "jet_jit_process_stream_lines" => jet_jit_process_stream_lines: sig_binary;
}
