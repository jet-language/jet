//! Resident JIT process adapters.
//!
//! Process policy and lifecycle semantics live in the shared Prelude included
//! by the interpreter. This module only stores opaque handles and marshals
//! values and typed errors across the Cranelift ABI.

use std::cell::RefCell;

use super::Concurrency;
use crate::ProcessPrelude::process_prelude;
use crate::Marshal::{clone_string, result_ok};
use jet_codegen::Comptime::{AmbientMirHandleResult, DevSink};
use jet_foundation::AST::{CtValue, Type};
use jet_foundation::Diagnostics::{Diagnostic, Span};
use jet_foundation::MIR::{
    MirHandleToken, MirRuntimeValue, MirType, MirTypeKind, MirNominalRef,
};
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

fn alloc_process_result(out: &process_prelude::ProcessReceipt) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        // Field order mirrors ProcessReceipt in JetStd/Open.rs. The first six
        // fields preserve the shipped ProcessResult layout; audit facts are
        // appended so old field lowering cannot silently change ABI slots.
        let record = rt.heap.alloc_record(18);
        let _ = rt
            .heap
            .record_set_int(record, 0, out.code.to_i64().unwrap_or(-1));
        let output = rt.heap.alloc_string(out.output.clone());
        let _ = rt.heap.record_set_string(record, 1, output);
        let errors = rt.heap.alloc_string(out.errors.clone());
        let _ = rt.heap.record_set_string(record, 2, errors);
        let _ = rt.heap.record_set_bool(record, 3, out.success);
        // A present narrow value is encoded as handle+1; zero is absent.
        let signal = out
            .signal
            .as_ref()
            .map(|value| value.to_i64().unwrap_or(0).wrapping_add(1))
            .unwrap_or(0);
        let _ = rt.heap.record_set_int(record, 4, signal);
        let _ = rt.heap.record_set_bool(record, 5, out.timed_out);
        let executable = rt.heap.alloc_string(out.executable_identity.clone());
        let _ = rt.heap.record_set_string(record, 6, executable);
        let argv_values = out
            .argv
            .iter()
            .map(|word| rt.heap.alloc_string(word.clone()))
            .collect::<Vec<_>>();
        let argv = rt.heap.alloc_int_list(argv_values);
        let _ = rt.heap.record_set_int(record, 7, argv);
        let input_digest = rt.heap.alloc_string(out.input_digest.clone());
        let _ = rt.heap.record_set_string(record, 8, input_digest);
        let digest = rt.heap.alloc_string(out.policy_digest.clone());
        let _ = rt.heap.record_set_string(record, 9, digest);
        let backend = rt.heap.alloc_string(out.backend.clone());
        let _ = rt.heap.record_set_string(record, 10, backend);
        let authority_values = out
            .authority
            .iter()
            .map(|right| rt.heap.alloc_string(right.clone()))
            .collect::<Vec<_>>();
        let authority = rt.heap.alloc_int_list(authority_values);
        let _ = rt.heap.record_set_int(record, 11, authority);
        let descendants = rt.heap.alloc_string(out.descendants.clone());
        let _ = rt.heap.record_set_string(record, 12, descendants);
        let limit_values = out
            .limits
            .iter()
            .map(|fact| rt.heap.alloc_string(fact.clone()))
            .collect::<Vec<_>>();
        let limits = rt.heap.alloc_int_list(limit_values);
        let _ = rt.heap.record_set_int(record, 13, limits);
        let output_values = out
            .outputs
            .iter()
            .map(|fact| rt.heap.alloc_string(fact.clone()))
            .collect::<Vec<_>>();
        let outputs = rt.heap.alloc_int_list(output_values);
        let _ = rt.heap.record_set_int(record, 14, outputs);
        let _ = rt.heap.record_set_bool(record, 15, out.redacted);
        let _ = rt
            .heap
            .record_set_int(record, 16, out.pid.to_i64().unwrap_or(0));
        let limit_hit = out
            .limit_hit
            .map(|limit| process_resource_limit_variant(limit).wrapping_add(1))
            .unwrap_or(0);
        let _ = rt.heap.record_set_int(record, 17, limit_hit);
        record
    })
}

fn alloc_process_plan(plan: &process_prelude::ProcessPlan) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        // Field order mirrors ProcessPlan in JetStd/CommonTypes.rs.
        let record = rt.heap.alloc_record(9);
        let executable = rt.heap.alloc_string(plan.executable_identity.clone());
        let _ = rt.heap.record_set_string(record, 0, executable);
        let argv_values = plan
            .argv
            .iter()
            .map(|word| rt.heap.alloc_string(word.clone()))
            .collect::<Vec<_>>();
        let argv = rt.heap.alloc_int_list(argv_values);
        let _ = rt.heap.record_set_int(record, 1, argv);
        let input_digest = rt.heap.alloc_string(plan.input_digest.clone());
        let _ = rt.heap.record_set_string(record, 2, input_digest);
        let digest = rt.heap.alloc_string(plan.policy_digest.clone());
        let _ = rt.heap.record_set_string(record, 3, digest);
        let backend = rt.heap.alloc_string(plan.backend.clone());
        let _ = rt.heap.record_set_string(record, 4, backend);
        let authority_values = plan
            .authority
            .iter()
            .map(|right| rt.heap.alloc_string(right.clone()))
            .collect::<Vec<_>>();
        let authority = rt.heap.alloc_int_list(authority_values);
        let _ = rt.heap.record_set_int(record, 5, authority);
        let descendants = rt.heap.alloc_string(plan.descendants.clone());
        let _ = rt.heap.record_set_string(record, 6, descendants);
        let limit_values = plan
            .limits
            .iter()
            .map(|fact| rt.heap.alloc_string(fact.clone()))
            .collect::<Vec<_>>();
        let limits = rt.heap.alloc_int_list(limit_values);
        let _ = rt.heap.record_set_int(record, 7, limits);
        let output_values = plan
            .outputs
            .iter()
            .map(|fact| rt.heap.alloc_string(fact.clone()))
            .collect::<Vec<_>>();
        let outputs = rt.heap.alloc_int_list(output_values);
        let _ = rt.heap.record_set_int(record, 8, outputs);
        record
    })
}

fn outcome_to_result(out: process_prelude::ProcessReceipt) -> i64 {
    result_ok(alloc_process_result(&out) as u64)
}

fn process_prelude_variant_index(enum_name: &str, variant: &str) -> i64 {
    crate::types_meta::prelude_enum_variant_index(enum_name, variant).unwrap_or_else(|| {
        panic!("Prelude enum `{enum_name}` variant `{variant}` is missing metadata")
    })
}

fn process_io_operation_bits(operation: process_prelude::IOOperation) -> i64 {
    let variant = match operation {
        process_prelude::IOOperation::Read => "Read",
        process_prelude::IOOperation::Write => "Write",
        process_prelude::IOOperation::Flush => "Flush",
        process_prelude::IOOperation::Connect => "Connect",
        process_prelude::IOOperation::Accept => "Accept",
        process_prelude::IOOperation::Close => "Close",
        process_prelude::IOOperation::Resolve => "Resolve",
        process_prelude::IOOperation::Codec => "Codec",
    };
    process_prelude_variant_index(jet_foundation::Syntax::TYPE_IO_OPERATION, variant)
}

fn process_io_error_variant(variant: &str) -> i64 {
    process_prelude_variant_index(jet_foundation::Syntax::TYPE_IO_ERROR, variant)
}

fn process_resource_limit_variant(limit: process_prelude::ProcessResourceLimit) -> i64 {
    let variant = match limit {
        process_prelude::ProcessResourceLimit::WallTime => "WallTime",
        process_prelude::ProcessResourceLimit::CpuTime => "CpuTime",
        process_prelude::ProcessResourceLimit::Memory => "Memory",
        process_prelude::ProcessResourceLimit::OpenFiles => "OpenFiles",
        process_prelude::ProcessResourceLimit::Output => "Output",
    };
    process_prelude_variant_index(
        jet_foundation::Syntax::TYPE_PROCESS_RESOURCE_LIMIT,
        variant,
    )
}

fn process_io_error_result(error: process_prelude::IOError) -> i64 {
    let (variant, context) = match error {
        process_prelude::IOError::InvalidInput(context) => {
            (process_io_error_variant("InvalidInput"), context)
        }
        process_prelude::IOError::NotFound(context) => {
            (process_io_error_variant("NotFound"), context)
        }
        process_prelude::IOError::PermissionDenied(context) => {
            (process_io_error_variant("PermissionDenied"), context)
        }
        process_prelude::IOError::TimedOut(context) => {
            (process_io_error_variant("TimedOut"), context)
        }
        process_prelude::IOError::Cancelled(context) => {
            (process_io_error_variant("Cancelled"), context)
        }
        process_prelude::IOError::Closed(context) => {
            (process_io_error_variant("Closed"), context)
        }
        process_prelude::IOError::Protocol(context) => {
            (process_io_error_variant("Protocol"), context)
        }
        process_prelude::IOError::Other(context) => {
            (process_io_error_variant("Other"), context)
        }
        process_prelude::IOError::ResourceLimit(limit) => {
            return Concurrency::with_runtime_mut(|rt| {
                // ResourceLimit uses the same one-slot enum record that generic
                // MIR enum construction and field access use. The outer
                // IOError carrier only packs that record's handle.
                let limit_record = rt.heap.alloc_record(1);
                let _ = rt.heap.record_set_int(
                    limit_record,
                    0,
                    process_resource_limit_variant(limit),
                );
                let error_variant = process_io_error_variant("ResourceLimit");
                rt.results.push(super::JitResultValue {
                    ok: false,
                    bits: limit_record
                        .wrapping_shl(8)
                        .wrapping_add(error_variant) as u64,
                });
                rt.results.len() as i64
            });
        }
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
/// Render a packed `IOError` with the canonical Prelude wording.
/// `ResourceLimit` stores its `ProcessResourceLimit` enum-record handle in the
/// payload word; ordinary I/O errors store an `IOContext` heap record.
pub(crate) fn process_error_show_text(packed: i64, heap: &jet_rt::JetArena) -> String {
    let variant = packed & 0xff;
    if variant == process_io_error_variant("ResourceLimit") {
        let limit_record = packed >> 8;
        let limit_variant = heap.record_get_int(limit_record, 0).unwrap_or(-1);
        let name = jet_foundation::StructuralDebug::jet_show_process_resource_limit(limit_variant);
        return format!("process resource limit exceeded: {name}");
    }
    let context = packed >> 8;
    let resource = heap
        .record_get_int(context, 1)
        .and_then(|encoded| encoded.checked_sub(1))
        .and_then(|handle| heap.clone_string(handle));
    let cause = heap
        .record_get_int(context, 3)
        .and_then(|encoded| encoded.checked_sub(1))
        .and_then(|handle| heap.clone_string(handle));
    jet_foundation::StructuralDebug::jet_show_io_error(
        variant,
        heap.record_get_int(context, 0).unwrap_or(0),
        resource.as_deref(),
        cause.as_deref(),
    )
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
        // The handle owns the stored spec. Move it through the builder rather
        // than cloning every argv/env vector and stream policy on each call.
        let current = std::mem::replace(spec, process_prelude::spec_new(Vec::new()));
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
#[derive(Default)]
struct InterpreterProcessState {
    specs: Vec<JitProcessSpec>,
    children: Vec<JitProcessChild>,
    stdins: Vec<JitProcessChild>,
}


thread_local! {
    static INTERPRETER_PROCESS_STATE: RefCell<InterpreterProcessState> =
        const { RefCell::new(InterpreterProcessState { specs: Vec::new(), children: Vec::new(), stdins: Vec::new() }) };
}


struct InterpreterProcessStateGuard {
    previous: Option<InterpreterProcessState>,
}

impl Drop for InterpreterProcessStateGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            let current = INTERPRETER_PROCESS_STATE.with(|slot| slot.replace(previous));
            drop(current);
        }
    }
}

/// Scope the opaque process carriers used by one interpreter/deopt run.
///
/// Process semantics remain in the shared Prelude. This scope only gives
/// explicit interpreter execution a resident carrier space when no JIT heap is
/// active; positive handles continue to use the active JIT runtime below.
pub(crate) fn with_interpreter_process_state<R>(body: impl FnOnce() -> R) -> R {
    let previous = INTERPRETER_PROCESS_STATE.with(|slot| {
        slot.replace(InterpreterProcessState::default())
    });
    let _guard = InterpreterProcessStateGuard {
        previous: Some(previous),
    };
    body()
}

fn local_process_index(handle: i64) -> Option<usize> {
    handle
        .checked_neg()
        .and_then(|value| value.checked_sub(1))
        .and_then(|value| usize::try_from(value).ok())
}

fn push_ambient_spec(spec: JitProcessSpec) -> i64 {
    let resident = Concurrency::with_runtime_mut(|rt| {
        rt.process_specs.push(spec.clone());
        Some(rt.process_specs.len() as i64)
    });
    if let Some(handle) = resident {
        return handle;
    }
    INTERPRETER_PROCESS_STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.specs.push(spec);
        -(state.specs.len() as i64)
    })
}

fn update_ambient_spec(
    handle: i64,
    f: impl FnOnce(JitProcessSpec) -> JitProcessSpec,
) -> Option<i64> {
    if let Some(index) = local_process_index(handle) {
        return INTERPRETER_PROCESS_STATE.with(|slot| {
            let mut state = slot.borrow_mut();
            let spec = state.specs.get_mut(index)?;
            *spec = f(spec.clone());
            Some(handle)
        });
    }
    if handle <= 0 {
        return None;
    }
    let index = usize::try_from(handle).ok()?.checked_sub(1)?;
    Concurrency::with_runtime_mut(|rt| {
        let spec = rt.process_specs.get_mut(index)?;
        *spec = f(spec.clone());
        Some(handle)
    })
}

fn with_ambient_spec<R>(
    handle: i64,
    f: impl FnOnce(&JitProcessSpec) -> R,
) -> Option<R> {
    if let Some(index) = local_process_index(handle) {
        return INTERPRETER_PROCESS_STATE.with(|slot| {
            slot.borrow().specs.get(index).map(f)
        });
    }
    if handle <= 0 {
        return None;
    }
    let index = usize::try_from(handle).ok()?.checked_sub(1)?;
    Concurrency::with_runtime_mut(|rt| rt.process_specs.get(index).map(f))
}

fn push_ambient_child(child: JitProcessChild) -> i64 {
    let resident = Concurrency::with_runtime_mut(|rt| {
        rt.process_children.push(child.clone());
        Some(rt.process_children.len() as i64)
    });
    if let Some(handle) = resident {
        return handle;
    }
    INTERPRETER_PROCESS_STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.children.push(child);
        -(state.children.len() as i64)
    })
}

fn with_ambient_child<R>(
    handle: i64,
    f: impl FnOnce(&JitProcessChild) -> R,
) -> Option<R> {
    if let Some(index) = local_process_index(handle) {
        return INTERPRETER_PROCESS_STATE.with(|slot| {
            slot.borrow().children.get(index).map(f)
        });
    }
    if handle <= 0 {
        return None;
    }
    let index = usize::try_from(handle).ok()?.checked_sub(1)?;
    Concurrency::with_runtime_mut(|rt| rt.process_children.get(index).map(f))
}
fn push_ambient_stdin(child_handle: i64) -> Option<i64> {
    let child = with_ambient_child(child_handle, Clone::clone)?;
    INTERPRETER_PROCESS_STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        if let Some(index) = state
            .stdins
            .iter()
            .position(|candidate| candidate == &child)
        {
            return Some(-(index as i64 + 1));
        }
        state.stdins.push(child);
        Some(-(state.stdins.len() as i64))
    })
}

fn with_ambient_stdin<R>(
    handle: i64,
    f: impl FnOnce(&JitProcessChild) -> R,
) -> Option<R> {
    let index = local_process_index(handle)?;
    INTERPRETER_PROCESS_STATE.with(|slot| slot.borrow().stdins.get(index).map(f))
}

fn process_ambient_diag(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        message.into(),
        "the interpreter process adapter rejected the checked operation".to_string(),
        "report this as a compiler bug".to_string(),
        Some(span),
    )
}

/// The forced interpreter reaches plain process constructors through the typed
/// ambient Core-call seam. Keep workspace authority construction on the
/// shared authority carrier rather than inventing a process-only value.
fn ct_process_string_list(
    value: &CtValue,
    operation: &str,
    span: Span,
) -> Result<Vec<String>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(process_ambient_diag(
            format!("{operation} expects a List<String>"),
            span,
        ));
    };
    values
        .iter()
        .map(|value| match value {
            CtValue::Str(value) => Ok(value.clone()),
            _ => Err(process_ambient_diag(
                format!("{operation} expects every command word to be a String"),
                span,
            )),
        })
        .collect()
}
fn ct_process_spec_list(
    value: &CtValue,
    operation: &str,
    span: Span,
) -> Result<Vec<JitProcessSpec>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(process_ambient_diag(
            format!("{operation} expects a List<ProcessSpec>"),
            span,
        ));
    };
    values
        .iter()
        .map(|value| {
            let CtValue::Closure(data) = value else {
                return Err(process_ambient_diag(
                    format!("{operation} received a non-opaque ProcessSpec"),
                    span,
                ));
            };
            let Some(token) = data
                .opaque
                .as_ref()
                .and_then(|owner| owner.downcast_ref::<MirHandleToken>())
            else {
                return Err(process_ambient_diag(
                    format!("{operation} received a foreign opaque value"),
                    span,
                ));
            };
            let raw = token.raw().filter(|raw| *raw != 0).ok_or_else(|| {
                process_ambient_diag(
                    format!("{operation} received a moved ProcessSpec"),
                    span,
                )
            })?;
            with_ambient_spec(raw, |spec| spec.clone()).ok_or_else(|| {
                process_ambient_diag(
                    format!("{operation} received an unknown ProcessSpec identity"),
                    span,
                )
            })
        })
        .collect()
}

fn ct_process_pipeline(args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let [value] = args else {
        return Err(process_ambient_diag(
            "core.process.pipeline expects one ProcessSpec list",
            span,
        ));
    };
    let specs = ct_process_spec_list(value, "core.process.pipeline", span)?;
    let value = match process_prelude::spec_pipeline(&specs) {
        Ok(receipt) => MirRuntimeValue::Present(Box::new(mir_process_receipt(receipt))),
        Err(error) => MirRuntimeValue::FailedTold(Box::new(mir_io_error(error))),
    };
    jet_codegen::Comptime::MirBridge::mir_to_ct_value(value, span)
}


fn ct_process_authority_wire(value: &CtValue, operation: &str, span: Span) -> Result<String, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(process_ambient_diag(
            format!("{operation} expects an Authority"),
            span,
        ));
    };
    if type_name != "Authority" {
        return Err(process_ambient_diag(
            format!("{operation} received {type_name}, expected Authority"),
            span,
        ));
    }
    let Some((_, CtValue::List(rights))) = fields.iter().find(|(name, _)| name == "rights") else {
        return Err(process_ambient_diag(
            format!("{operation} received an Authority without `rights`"),
            span,
        ));
    };
    rights
        .iter()
        .map(|right| match right {
            CtValue::Str(right) => Ok(right.clone()),
            _ => Err(process_ambient_diag(
                format!("{operation} expects Authority.rights to be a List<String>"),
                span,
            )),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|rights| rights.join("\n"))
}

fn ct_process_run(
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    if !matches!(args.len(), 1 | 2) {
        return Err(process_ambient_diag(
            "core.process.run expects one command list and an optional Authority",
            span,
        ));
    }
    let command = ct_process_string_list(&args[0], "core.process.run", span)?;
    let mut spec = process_prelude::spec_new(command);
    if let Some(authority) = args.get(1) {
        let wire = ct_process_authority_wire(authority, "core.process.run", span)?;
        spec = process_prelude::spec_under_wire(spec, &wire);
    }
    let value = match process_prelude::spec_run(&spec) {
        Ok(receipt) => MirRuntimeValue::Present(Box::new(mir_process_receipt(receipt))),
        Err(error) => MirRuntimeValue::FailedTold(Box::new(mir_io_error(error))),
    };
    jet_codegen::Comptime::MirBridge::mir_to_ct_value(value, span)
}

pub(crate) fn ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    _resolved_ret: Option<Type>,
    _sink: Option<&mut DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    if module != "core.process" {
        return None;
    }
    match method {
        "workspace" => {
            if !args.is_empty() {
                return Some(Err(process_ambient_diag(
                    "core.process.workspace expects no arguments",
                    span,
                )));
            }
            let rights = crate::Collections::authority_semantics::jet_authority_workspace_rights()
                .into_iter()
                .map(CtValue::Str)
                .collect();
            Some(Ok(CtValue::Struct {
                type_name: "Authority".to_string(),
                fields: vec![("rights".to_string(), CtValue::List(rights))],
            }))
        }
        "pipeline" => Some(ct_process_pipeline(&args, span)),
        "run" | "run_with_authority" => Some(ct_process_run(&args, span)),
        _ => None,
    }
}

fn mir_named(name: &str) -> MirType {
    MirType::from_kind(MirTypeKind::Apply { name: MirNominalRef::from_name(name), args: Vec::new() })
}

fn mir_absent(name: &str) -> MirRuntimeValue {
    MirRuntimeValue::Absent {
        element: mir_named(name),
    }
}

fn mir_enum(type_name: &str, variant: &str) -> MirRuntimeValue {
    MirRuntimeValue::Enum {
        type_name: type_name.to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
    }
}

fn mir_optional_string(value: Result<String, JetAbsent>) -> MirRuntimeValue {
    match value {
        Ok(value) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::String(value))),
        Err(JetAbsent) => mir_absent("String"),
    }
}

fn mir_optional_int(value: Result<i64, JetAbsent>) -> MirRuntimeValue {
    match value {
        Ok(value) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::Int(value))),
        Err(JetAbsent) => mir_absent("Int"),
    }
}

fn mir_string_list(values: &[String]) -> MirRuntimeValue {
    MirRuntimeValue::List(
        values
            .iter()
            .cloned()
            .map(MirRuntimeValue::String)
            .collect(),
    )
}

fn mir_process_resource_limit(
    limit: process_prelude::ProcessResourceLimit,
) -> MirRuntimeValue {
    let variant = match limit {
        process_prelude::ProcessResourceLimit::WallTime => "WallTime",
        process_prelude::ProcessResourceLimit::CpuTime => "CpuTime",
        process_prelude::ProcessResourceLimit::Memory => "Memory",
        process_prelude::ProcessResourceLimit::OpenFiles => "OpenFiles",
        process_prelude::ProcessResourceLimit::Output => "Output",
    };
    mir_enum("ProcessResourceLimit", variant)
}

fn mir_io_context(context: process_prelude::IOContext) -> MirRuntimeValue {
    let process_prelude::IOContext {
        operation,
        resource,
        os_code,
        cause,
    } = context;
    let operation = match operation {
        process_prelude::IOOperation::Read => "Read",
        process_prelude::IOOperation::Write => "Write",
        process_prelude::IOOperation::Flush => "Flush",
        process_prelude::IOOperation::Connect => "Connect",
        process_prelude::IOOperation::Accept => "Accept",
        process_prelude::IOOperation::Close => "Close",
        process_prelude::IOOperation::Resolve => "Resolve",
        process_prelude::IOOperation::Codec => "Codec",
    };
    MirRuntimeValue::Struct {
        type_name: "IOContext".to_string(),
        fields: vec![
            ("operation".to_string(), mir_enum("IOOperation", operation)),
            ("resource".to_string(), mir_optional_string(resource)),
            ("os_code".to_string(), mir_optional_int(os_code)),
            ("cause".to_string(), mir_optional_string(cause)),
        ],
    }
}

fn mir_io_error(error: process_prelude::IOError) -> MirRuntimeValue {
    let (variant, args) = match error {
        process_prelude::IOError::InvalidInput(context) => {
            ("InvalidInput", vec![(None, mir_io_context(context))])
        }
        process_prelude::IOError::NotFound(context) => {
            ("NotFound", vec![(None, mir_io_context(context))])
        }
        process_prelude::IOError::PermissionDenied(context) => {
            ("PermissionDenied", vec![(None, mir_io_context(context))])
        }
        process_prelude::IOError::TimedOut(context) => {
            ("TimedOut", vec![(None, mir_io_context(context))])
        }
        process_prelude::IOError::Cancelled(context) => {
            ("Cancelled", vec![(None, mir_io_context(context))])
        }
        process_prelude::IOError::Closed(context) => {
            ("Closed", vec![(None, mir_io_context(context))])
        }
        process_prelude::IOError::Protocol(context) => {
            ("Protocol", vec![(None, mir_io_context(context))])
        }
        process_prelude::IOError::Other(context) => {
            ("Other", vec![(None, mir_io_context(context))])
        }
        process_prelude::IOError::ResourceLimit(limit) => {
            ("ResourceLimit", vec![(None, mir_process_resource_limit(limit))])
        }
    };
    MirRuntimeValue::Enum {
        type_name: "IOError".to_string(),
        variant: variant.to_string(),
        args,
    }
}

fn mir_process_receipt(receipt: process_prelude::ProcessReceipt) -> MirRuntimeValue {
    let signal = match receipt.signal {
        Ok(value) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::Int(
            value.to_i64().unwrap_or(0),
        ))),
        Err(JetAbsent) => mir_absent("Int"),
    };
    let limit_hit = match receipt.limit_hit {
        Ok(value) => MirRuntimeValue::Present(Box::new(mir_process_resource_limit(value))),
        Err(JetAbsent) => mir_absent("ProcessResourceLimit"),
    };
    MirRuntimeValue::Struct {
        type_name: "ProcessReceipt".to_string(),
        fields: vec![
            (
                "code".to_string(),
                MirRuntimeValue::Int(receipt.code.to_i64().unwrap_or(-1)),
            ),
            ("output".to_string(), MirRuntimeValue::String(receipt.output)),
            ("errors".to_string(), MirRuntimeValue::String(receipt.errors)),
            ("success".to_string(), MirRuntimeValue::Bool(receipt.success)),
            ("signal".to_string(), signal),
            (
                "timed_out".to_string(),
                MirRuntimeValue::Bool(receipt.timed_out),
            ),
            (
                "executable_identity".to_string(),
                MirRuntimeValue::String(receipt.executable_identity),
            ),
            ("argv".to_string(), mir_string_list(&receipt.argv)),
            (
                "input_digest".to_string(),
                MirRuntimeValue::String(receipt.input_digest),
            ),
            (
                "policy_digest".to_string(),
                MirRuntimeValue::String(receipt.policy_digest),
            ),
            ("backend".to_string(), MirRuntimeValue::String(receipt.backend)),
            ("authority".to_string(), mir_string_list(&receipt.authority)),
            (
                "descendants".to_string(),
                MirRuntimeValue::String(receipt.descendants),
            ),
            ("limits".to_string(), mir_string_list(&receipt.limits)),
            ("outputs".to_string(), mir_string_list(&receipt.outputs)),
            ("redacted".to_string(), MirRuntimeValue::Bool(receipt.redacted)),
            (
                "pid".to_string(),
                MirRuntimeValue::Int(receipt.pid.to_i64().unwrap_or(0)),
            ),
            ("limit_hit".to_string(), limit_hit),
        ],
    }
}

fn mir_process_string_list(
    value: &MirRuntimeValue,
    span: Span,
) -> Result<Vec<String>, Diagnostic> {
    let MirRuntimeValue::List(values) = value else {
        return Err(process_ambient_diag(
            "core.process.cmd() expects a List<String>",
            span,
        ));
    };
    values
        .iter()
        .map(|value| match value {
            MirRuntimeValue::String(value) => Ok(value.clone()),
            _ => Err(process_ambient_diag(
                "core.process.cmd() expects every command word to be a String",
                span,
            )),
        })
        .collect()
}

fn mir_process_stream_mode(
    value: &MirRuntimeValue,
    span: Span,
) -> Result<process_prelude::ProcessStreamMode, Diagnostic> {
    let MirRuntimeValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(process_ambient_diag(
            "ProcessSpec.stdin() expects a ProcessStreamMode",
            span,
        ));
    };
    if type_name != "ProcessStreamMode" || !args.is_empty() {
        return Err(process_ambient_diag(
            "ProcessSpec.stdin() received a malformed ProcessStreamMode",
            span,
        ));
    }
    match variant.as_str() {
        "Stream" => Ok(process_prelude::ProcessStreamMode::Stream),
        "Inherit" => Ok(process_prelude::ProcessStreamMode::Inherit),
        "Capture" => Ok(process_prelude::ProcessStreamMode::Capture),
        _ => Err(process_ambient_diag(
            "ProcessSpec.stdin() received an unknown ProcessStreamMode variant",
            span,
        )),
    }
}

fn mir_process_string(
    value: &MirRuntimeValue,
    operation: &str,
    span: Span,
) -> Result<String, Diagnostic> {
    match value {
        MirRuntimeValue::String(value) => Ok(value.clone()),
        _ => Err(process_ambient_diag(
            format!("{operation} expects a String"),
            span,
        )),
    }
}

fn mir_process_int(
    value: &MirRuntimeValue,
    operation: &str,
    span: Span,
) -> Result<i64, Diagnostic> {
    match value {
        MirRuntimeValue::Int(value) => Ok(*value),
        _ => Err(process_ambient_diag(
            format!("{operation} expects an Int"),
            span,
        )),
    }
}
fn mir_process_terminal_size(
    value: &MirRuntimeValue,
    operation: &str,
    span: Span,
) -> Result<process_prelude::TerminalSize, Diagnostic> {
    let cols = mir_process_field(value, "TerminalSize", "cols", operation, span)?;
    let rows = mir_process_field(value, "TerminalSize", "rows", operation, span)?;
    Ok(process_prelude::TerminalSize {
        cols: mir_process_int(cols, operation, span)?,
        rows: mir_process_int(rows, operation, span)?,
    })
}

fn mir_process_field<'a>(
    value: &'a MirRuntimeValue,
    type_name: &str,
    field_name: &str,
    operation: &str,
    span: Span,
) -> Result<&'a MirRuntimeValue, Diagnostic> {
    let MirRuntimeValue::Struct {
        type_name: actual,
        fields,
    } = value
    else {
        return Err(process_ambient_diag(
            format!("{operation} received a non-{type_name} value"),
            span,
        ));
    };
    if actual != type_name {
        return Err(process_ambient_diag(
            format!("{operation} received {actual}, expected {type_name}"),
            span,
        ));
    }
    fields
        .iter()
        .find_map(|(name, value)| (name == field_name).then_some(value))
        .ok_or_else(|| {
            process_ambient_diag(
                format!("{operation} received a {type_name} without `{field_name}`"),
                span,
            )
        })
}

fn mir_process_duration(
    value: &MirRuntimeValue,
    operation: &str,
    span: Span,
) -> Result<process_prelude::Duration, Diagnostic> {
    let ns = mir_process_field(value, "Duration", "ns", operation, span)?;
    Ok(process_prelude::Duration {
        ns: mir_process_int(ns, operation, span)?,
    })
}

fn mir_process_authority_wire(
    value: &MirRuntimeValue,
    operation: &str,
    span: Span,
) -> Result<String, Diagnostic> {
    let rights = mir_process_field(value, "Authority", "rights", operation, span)?;
    let MirRuntimeValue::List(rights) = rights else {
        return Err(process_ambient_diag(
            format!("{operation} expects Authority.rights to be a List<String>"),
            span,
        ));
    };
    rights
        .iter()
        .map(|right| mir_process_string(right, operation, span))
        .collect::<Result<Vec<_>, _>>()
        .map(|rights| rights.join("\n"))
}

fn mir_process_terminal_policy(
    value: &MirRuntimeValue,
    operation: &str,
    span: Span,
) -> Result<process_prelude::TerminalPolicy, Diagnostic> {
    let size = mir_process_field(value, "TerminalPolicy", "size", operation, span)?;
    let cols = mir_process_field(size, "TerminalSize", "cols", operation, span)?;
    let rows = mir_process_field(size, "TerminalSize", "rows", operation, span)?;
    let mode = mir_process_field(value, "TerminalPolicy", "mode", operation, span)?;
    let MirRuntimeValue::Enum {
        type_name,
        variant,
        args,
    } = mode
    else {
        return Err(process_ambient_diag(
            format!("{operation} expects TerminalPolicy.mode to be a TerminalMode"),
            span,
        ));
    };
    if type_name != "TerminalMode" || !args.is_empty() {
        return Err(process_ambient_diag(
            format!("{operation} received a malformed TerminalMode"),
            span,
        ));
    }
    let mode = match variant.as_str() {
        "Raw" => process_prelude::TerminalMode::Raw,
        "Cooked" => process_prelude::TerminalMode::Cooked,
        _ => {
            return Err(process_ambient_diag(
                format!("{operation} received an unknown TerminalMode variant"),
                span,
            ))
        }
    };
    Ok(process_prelude::TerminalPolicy {
        size: process_prelude::TerminalSize {
            cols: mir_process_int(cols, operation, span)?,
            rows: mir_process_int(rows, operation, span)?,
        },
        mode,
    })
}

fn mir_process_plan(plan: process_prelude::ProcessPlan) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "ProcessPlan".to_string(),
        fields: vec![
            (
                "executable_identity".to_string(),
                MirRuntimeValue::String(plan.executable_identity),
            ),
            ("argv".to_string(), mir_string_list(&plan.argv)),
            (
                "input_digest".to_string(),
                MirRuntimeValue::String(plan.input_digest),
            ),
            (
                "policy_digest".to_string(),
                MirRuntimeValue::String(plan.policy_digest),
            ),
            ("backend".to_string(), MirRuntimeValue::String(plan.backend)),
            ("authority".to_string(), mir_string_list(&plan.authority)),
            (
                "descendants".to_string(),
                MirRuntimeValue::String(plan.descendants),
            ),
            ("limits".to_string(), mir_string_list(&plan.limits)),
            ("outputs".to_string(), mir_string_list(&plan.outputs)),
        ],
    }
}

fn mir_process_abilities(facts: std::collections::HashSet<String>) -> MirRuntimeValue {
    let mut facts = facts.into_iter().collect::<Vec<_>>();
    facts.sort();
    MirRuntimeValue::Struct {
        type_name: "Set".to_string(),
        fields: vec![("items".to_string(), mir_string_list(&facts))],
    }
}

fn process_ambient_spec_update(
    operation: &str,
    handle: Option<i64>,
    span: Span,
    update: impl FnOnce(process_prelude::ProcessSpec) -> process_prelude::ProcessSpec,
) -> Result<AmbientMirHandleResult, Diagnostic> {
    let Some(handle) = handle else {
        return Err(process_ambient_diag(
            format!("{operation} has no typed receiver handle"),
            span,
        ));
    };
    let Some(handle) = update_ambient_spec(handle, update) else {
        return Err(process_ambient_diag(
            format!("{operation} used an unavailable process handle"),
            span,
        ));
    };
    Ok(AmbientMirHandleResult::Handle(handle))
}

fn process_ambient_value(value: MirRuntimeValue) -> AmbientMirHandleResult { AmbientMirHandleResult::Value(value) }

fn process_ambient_child_unit(
    operation: &str,
    handle: Option<i64>,
    args: &[MirRuntimeValue],
    span: Span,
    action: fn(&process_prelude::ProcessChild) -> Result<(), process_prelude::IOError>,
) -> Option<Result<AmbientMirHandleResult, Diagnostic>> {
    let Some(handle) = handle else {
        return Some(Err(process_ambient_diag(
            format!("{operation} has no typed receiver handle"),
            span,
        )));
    };
    if !args.is_empty() {
        return Some(Err(process_ambient_diag(
            format!("{operation} received unexpected arguments"),
            span,
        )));
    }
    let Some(child) = with_ambient_child(handle, Clone::clone) else {
        let error = mir_io_error(process_prelude::IOError::other(
            process_prelude::IOOperation::Close,
            Some("ProcessChild".to_string()),
            "unavailable process handle",
        ));
        return Some(Ok(process_ambient_value(
            MirRuntimeValue::FailedTold(Box::new(error)),
        )));
    };
    let value = match action(&child) {
        Ok(()) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::Unit)),
        Err(error) => MirRuntimeValue::FailedTold(Box::new(mir_io_error(error))),
    };
    Some(Ok(process_ambient_value(value)))
}

/// Dispatch the checked process carrier operations for MIR interpreter/deopt.
///
/// The callback receives canonical MIR values and returns either an opaque
/// resident handle or a canonical typed outcome. It never exposes a process
/// object as a canonical integer.
pub(crate) fn ambient_mir_handle(
    operation: &str,
    handle: Option<i64>,
    args: Vec<MirRuntimeValue>,
    span: Span,
) -> Option<Result<AmbientMirHandleResult, Diagnostic>> {
    match operation {
        "process.cmd" => {
            if handle.is_some() || args.len() != 1 {
                return Some(Err(process_ambient_diag(
                    "core.process.cmd() received the wrong checked argument shape",
                    span,
                )));
            }
            let command = match mir_process_string_list(&args[0], span) {
                Ok(command) => command,
                Err(error) => return Some(Err(error)),
            };
            let spec = process_prelude::spec_new(command);
            Some(Ok(AmbientMirHandleResult::Handle(push_ambient_spec(spec))))
        }
        "process.spec.cwd" => {
            let [cwd] = args.as_slice() else {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec.cwd() expects one String argument",
                    span,
                )));
            };
            let cwd = match mir_process_string(cwd, "ProcessSpec.cwd()", span) {
                Ok(cwd) => cwd,
                Err(error) => return Some(Err(error)),
            };
            Some(process_ambient_spec_update(
                "ProcessSpec.cwd()",
                handle,
                span,
                |spec| process_prelude::spec_cwd(spec, &cwd),
            ))
        }
        "process.spec.env" => {
            let [name, value] = args.as_slice() else {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec.env() expects two String arguments",
                    span,
                )));
            };
            let name = match mir_process_string(name, "ProcessSpec.env()", span) {
                Ok(name) => name,
                Err(error) => return Some(Err(error)),
            };
            let value = match mir_process_string(value, "ProcessSpec.env()", span) {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            };
            Some(process_ambient_spec_update(
                "ProcessSpec.env()",
                handle,
                span,
                |spec| process_prelude::spec_env(spec, &name, &value),
            ))
        }
        "process.spec.env_remove" => {
            let [name] = args.as_slice() else {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec.env_remove() expects one String argument",
                    span,
                )));
            };
            let name = match mir_process_string(name, "ProcessSpec.env_remove()", span) {
                Ok(name) => name,
                Err(error) => return Some(Err(error)),
            };
            Some(process_ambient_spec_update(
                "ProcessSpec.env_remove()",
                handle,
                span,
                |spec| process_prelude::spec_env_remove(spec, &name),
            ))
        }
        "process.spec.env_clear" => {
            if !args.is_empty() {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec.env_clear() received unexpected arguments",
                    span,
                )));
            }
            Some(process_ambient_spec_update(
                "ProcessSpec.env_clear()",
                handle,
                span,
                process_prelude::spec_env_clear,
            ))
        }
        "process.spec.stdin" | "process.spec.stdout" | "process.spec.stderr" => {
            let [mode] = args.as_slice() else {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec stream builder expects one ProcessStreamMode argument",
                    span,
                )));
            };
            let mode = match mir_process_stream_mode(mode, span) {
                Ok(mode) => mode,
                Err(error) => return Some(Err(error)),
            };
            let operation_name = match operation {
                "process.spec.stdin" => "ProcessSpec.stdin()",
                "process.spec.stdout" => "ProcessSpec.stdout()",
                _ => "ProcessSpec.stderr()",
            };
            Some(process_ambient_spec_update(
                operation_name,
                handle,
                span,
                move |spec| match operation {
                    "process.spec.stdin" => process_prelude::spec_stdin(spec, &mode),
                    "process.spec.stdout" => process_prelude::spec_stdout(spec, &mode),
                    _ => process_prelude::spec_stderr(spec, &mode),
                },
            ))
        }
        "process.spec.timeout" | "process.spec.cpu_time_limit" => {
            let [duration] = args.as_slice() else {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec duration builder expects one Duration argument",
                    span,
                )));
            };
            let duration = match mir_process_duration(duration, operation, span) {
                Ok(duration) => duration,
                Err(error) => return Some(Err(error)),
            };
            let operation_name = if operation == "process.spec.timeout" {
                "ProcessSpec.timeout()"
            } else {
                "ProcessSpec.cpu_time_limit()"
            };
            Some(process_ambient_spec_update(
                operation_name,
                handle,
                span,
                move |spec| {
                    if operation == "process.spec.timeout" {
                        process_prelude::spec_timeout(spec, &duration)
                    } else {
                        process_prelude::spec_cpu_time_limit(spec, &duration)
                    }
                },
            ))
        }
        "process.spec.output_limit"
        | "process.spec.memory_limit"
        | "process.spec.open_file_limit" => {
            let [limit] = args.as_slice() else {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec limit builder expects one Int argument",
                    span,
                )));
            };
            let limit = match mir_process_int(limit, operation, span) {
                Ok(limit) => limit,
                Err(error) => return Some(Err(error)),
            };
            let operation_name = match operation {
                "process.spec.output_limit" => "ProcessSpec.output_limit()",
                "process.spec.memory_limit" => "ProcessSpec.memory_limit()",
                _ => "ProcessSpec.open_file_limit()",
            };
            Some(process_ambient_spec_update(
                operation_name,
                handle,
                span,
                move |spec| match operation {
                    "process.spec.output_limit" => {
                        process_prelude::spec_output_limit(spec, limit)
                    }
                    "process.spec.memory_limit" => {
                        process_prelude::spec_memory_limit(spec, limit)
                    }
                    _ => process_prelude::spec_open_file_limit(spec, limit),
                },
            ))
        }
        "process.spec.detached" => {
            if !args.is_empty() {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec.detached() received unexpected arguments",
                    span,
                )));
            }
            Some(process_ambient_spec_update(
                "ProcessSpec.detached()",
                handle,
                span,
                process_prelude::spec_detached,
            ))
        }
        "process.spec.terminal" => {
            if !args.is_empty() {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec.terminal() received unexpected arguments",
                    span,
                )));
            }
            Some(process_ambient_spec_update(
                "ProcessSpec.terminal()",
                handle,
                span,
                process_prelude::spec_terminal,
            ))
        }
        "process.spec.terminal_with_policy" => {
            let [policy] = args.as_slice() else {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec.terminal(policy) expects one TerminalPolicy argument",
                    span,
                )));
            };
            let policy = match mir_process_terminal_policy(policy, "ProcessSpec.terminal(policy)", span)
            {
                Ok(policy) => policy,
                Err(error) => return Some(Err(error)),
            };
            Some(process_ambient_spec_update(
                "ProcessSpec.terminal(policy)",
                handle,
                span,
                |spec| process_prelude::spec_terminal_with_policy(spec, &policy),
            ))
        }
        "process.spec.under" => {
            let [authority] = args.as_slice() else {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec.under() expects one Authority argument",
                    span,
                )));
            };
            let authority =
                match mir_process_authority_wire(authority, "ProcessSpec.under()", span) {
                    Ok(authority) => authority,
                    Err(error) => return Some(Err(error)),
                };
            Some(process_ambient_spec_update(
                "ProcessSpec.under()",
                handle,
                span,
                |spec| process_prelude::spec_under_wire(spec, &authority),
            ))
        }
        "process.spec.abilities" => {
            if !args.is_empty() {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec.abilities() received unexpected arguments",
                    span,
                )));
            }
            let Some(handle) = handle else {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec.abilities() has no typed receiver handle",
                    span,
                )));
            };
            let Some(spec) = with_ambient_spec(handle, Clone::clone) else {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec.abilities() used an unavailable process handle",
                    span,
                )));
            };
            Some(Ok(process_ambient_value(mir_process_abilities(
                process_prelude::spec_abilities(&spec),
            ))))
        }
        "process.spec.plan" | "process.spec.run" | "process.spec.run_checked" => {
            if !args.is_empty() {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec execution method received unexpected arguments",
                    span,
                )));
            }
            let Some(handle) = handle else {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec execution method has no typed receiver handle",
                    span,
                )));
            };
            let Some(spec) = with_ambient_spec(handle, Clone::clone) else {
                let error = mir_io_error(process_prelude::IOError::other(
                    process_prelude::IOOperation::Resolve,
                    Some("ProcessSpec".to_string()),
                    "unavailable process handle",
                ));
                return Some(Ok(process_ambient_value(
                    MirRuntimeValue::FailedTold(Box::new(error)),
                )));
            };
            let value = match operation {
                "process.spec.plan" => match process_prelude::spec_plan(&spec) {
                    Ok(plan) => MirRuntimeValue::Present(Box::new(mir_process_plan(plan))),
                    Err(error) => MirRuntimeValue::FailedTold(Box::new(mir_io_error(error))),
                },
                "process.spec.run" => match process_prelude::spec_run(&spec) {
                    Ok(receipt) => {
                        MirRuntimeValue::Present(Box::new(mir_process_receipt(receipt)))
                    }
                    Err(error) => MirRuntimeValue::FailedTold(Box::new(mir_io_error(error))),
                },
                _ => match process_prelude::spec_run_checked(&spec) {
                    Ok(receipt) => {
                        MirRuntimeValue::Present(Box::new(mir_process_receipt(receipt)))
                    }
                    Err(error) => MirRuntimeValue::FailedTold(Box::new(mir_io_error(error))),
                },
            };
            Some(Ok(process_ambient_value(value)))
        }
        "process.spec.spawn" => {
            let Some(handle) = handle else {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec.spawn() has no typed receiver handle",
                    span,
                )));
            };
            if !args.is_empty() {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec.spawn() received unexpected arguments",
                    span,
                )));
            }
            let Some(spec) = with_ambient_spec(handle, Clone::clone) else {
                let error = mir_io_error(process_prelude::IOError::other(
                    process_prelude::IOOperation::Resolve,
                    Some("ProcessSpec".to_string()),
                    "unavailable process handle",
                ));
                return Some(Ok(process_ambient_value(
                    MirRuntimeValue::FailedTold(Box::new(error)),
                )));
            };
            let value = match process_prelude::spec_spawn(&spec) {
                Ok(child) => {
                    let child = push_ambient_child(child);
                    MirRuntimeValue::Present(Box::new(MirRuntimeValue::Int(child)))
                }
                Err(error) => MirRuntimeValue::FailedTold(Box::new(mir_io_error(error))),
            };
            Some(Ok(process_ambient_value(value)))
        }
        "process.child.stdin" => {
            let Some(handle) = handle else {
                return Some(Err(process_ambient_diag(
                    "ProcessChild.stdin has no typed receiver handle",
                    span,
                )));
            };
            if !args.is_empty() {
                return Some(Err(process_ambient_diag(
                    "ProcessChild.stdin received unexpected arguments",
                    span,
                )));
            }
            let Some(stdin) = push_ambient_stdin(handle) else {
                return Some(Err(process_ambient_diag(
                    "ProcessChild.stdin used an unavailable process handle",
                    span,
                )));
            };
            Some(Ok(AmbientMirHandleResult::Handle(stdin)))
        }
        "process.child.id" => {
            let Some(handle) = handle else {
                return Some(Err(process_ambient_diag(
                    "ProcessChild.id() has no typed receiver handle",
                    span,
                )));
            };
            if !args.is_empty() {
                return Some(Err(process_ambient_diag(
                    "ProcessChild.id() received unexpected arguments",
                    span,
                )));
            }
            let id = with_ambient_child(handle, Clone::clone)
                .map(|child| process_prelude::child_id(&child))
                .unwrap_or(0);
            Some(Ok(process_ambient_value(MirRuntimeValue::Int(id))))
        }
        "process.child.exited" => {
            let Some(handle) = handle else {
                return Some(Err(process_ambient_diag(
                    "ProcessChild.exited() has no typed receiver handle",
                    span,
                )));
            };
            if !args.is_empty() {
                return Some(Err(process_ambient_diag(
                    "ProcessChild.exited() received unexpected arguments",
                    span,
                )));
            }
            let Some(child) = with_ambient_child(handle, Clone::clone) else {
                let error = mir_io_error(process_prelude::IOError::other(
                    process_prelude::IOOperation::Close,
                    Some("ProcessChild".to_string()),
                    "unavailable process handle",
                ));
                return Some(Ok(process_ambient_value(
                    MirRuntimeValue::FailedTold(Box::new(error)),
                )));
            };
            let value = match process_prelude::child_exited(&child) {
                Ok(exited) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::Bool(exited))),
                Err(error) => MirRuntimeValue::FailedTold(Box::new(mir_io_error(error))),
            };
            Some(Ok(process_ambient_value(value)))
        }
        "process.child.kill" => {
            process_ambient_child_unit(
                operation,
                handle,
                &args,
                span,
                process_prelude::child_kill,
            )
        }
        "process.child.terminate" => {
            process_ambient_child_unit(
                operation,
                handle,
                &args,
                span,
                process_prelude::child_terminate,
            )
        }
        "process.child.interrupt" => {
            process_ambient_child_unit(
                operation,
                handle,
                &args,
                span,
                process_prelude::child_interrupt,
            )
        }
        "process.child.terminal" => {
            let Some(handle) = handle else {
                return Some(Err(process_ambient_diag(
                    "ProcessChild.terminal has no typed receiver handle",
                    span,
                )));
            };
            if !args.is_empty() {
                return Some(Err(process_ambient_diag(
                    "ProcessChild.terminal received unexpected arguments",
                    span,
                )));
            }
            let present = with_ambient_child(handle, |child| child.terminal.is_ok()).unwrap_or(false);
            if present {
                Some(Ok(AmbientMirHandleResult::Handle(handle)))
            } else {
                Some(Ok(process_ambient_value(mir_absent("TerminalSession"))))
            }
        }
        "terminal.resize" => {
            let Some(handle) = handle else {
                return Some(Err(process_ambient_diag(
                    "TerminalSession.resize() has no typed receiver handle",
                    span,
                )));
            };
            let [size] = args.as_slice() else {
                return Some(Err(process_ambient_diag(
                    "TerminalSession.resize() expects one TerminalSize argument",
                    span,
                )));
            };
            let size = match mir_process_terminal_size(size, operation, span) {
                Ok(size) => size,
                Err(error) => return Some(Err(error)),
            };
            let Some(child) = with_ambient_child(handle, Clone::clone) else {
                return Some(Ok(process_ambient_value(MirRuntimeValue::FailedTold(
                    Box::new(mir_io_error(process_prelude::IOError::other(
                        process_prelude::IOOperation::Resolve,
                        Some("process terminal".to_string()),
                        "unavailable process handle",
                    ))),
                ))));
            };
            let value = match child.terminal.as_ref().ok() {
                Some(session) => match process_prelude::terminal_session_resize(session, &size) {
                    Ok(()) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::Unit)),
                    Err(error) => MirRuntimeValue::FailedTold(Box::new(mir_io_error(error))),
                },
                None => MirRuntimeValue::FailedTold(Box::new(mir_io_error(
                    process_prelude::IOError::other(
                        process_prelude::IOOperation::Resolve,
                        Some("process terminal".to_string()),
                        "this child has no terminal session",
                    ),
                ))),
            };
            Some(Ok(process_ambient_value(value)))
        }
        "process.child.wait" => {
            let Some(handle) = handle else {
                return Some(Err(process_ambient_diag(
                    "ProcessChild.wait() has no typed receiver handle",
                    span,
                )));
            };
            if !args.is_empty() {
                return Some(Err(process_ambient_diag(
                    "ProcessChild.wait() received unexpected arguments",
                    span,
                )));
            }
            let Some(child) = with_ambient_child(handle, Clone::clone) else {
                let error = mir_io_error(process_prelude::IOError::other(
                    process_prelude::IOOperation::Close,
                    Some("ProcessChild".to_string()),
                    "unavailable process handle",
                ));
                return Some(Ok(process_ambient_value(
                    MirRuntimeValue::FailedTold(Box::new(error)),
                )));
            };
            let value = match process_prelude::child_wait(&child) {
                Ok(receipt) => MirRuntimeValue::Present(Box::new(mir_process_receipt(receipt))),
                Err(error) => MirRuntimeValue::FailedTold(Box::new(mir_io_error(error))),
            };
            Some(Ok(process_ambient_value(value)))
        }
        "process.child.close" => {
            let Some(handle) = handle else {
                return Some(Err(process_ambient_diag(
                    "ProcessChild.close() has no typed receiver handle",
                    span,
                )));
            };
            if !args.is_empty() {
                return Some(Err(process_ambient_diag(
                    "ProcessChild.close() received unexpected arguments",
                    span,
                )));
            }
            let Some(child) = with_ambient_child(handle, Clone::clone) else {
                return Some(Err(process_ambient_diag(
                    "ProcessChild.close() used an unavailable process handle",
                    span,
                )));
            };
            process_prelude::child_close(&child);
            Some(Ok(process_ambient_value(MirRuntimeValue::Unit)))
        }

        "process.stdin_write" => {
            let Some(handle) = handle else {
                return Some(Err(process_ambient_diag(
                    "ProcessStdin.write() has no typed receiver handle",
                    span,
                )));
            };
            let [MirRuntimeValue::String(text)] = args.as_slice() else {
                return Some(Err(process_ambient_diag(
                    "ProcessStdin.write() expects one String argument",
                    span,
                )));
            };
            let Some(child) = with_ambient_stdin(handle, Clone::clone) else {
                let error = mir_io_error(process_prelude::IOError::other(
                    process_prelude::IOOperation::Write,
                    Some("ProcessStdin".to_string()),
                    "unavailable process handle",
                ));
                return Some(Ok(process_ambient_value(
                    MirRuntimeValue::FailedTold(Box::new(error)),
                )));
            };
            let value = match process_prelude::child_stdin_write(&child, text) {
                Ok(()) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::Unit)),
                Err(error) => MirRuntimeValue::FailedTold(Box::new(mir_io_error(error))),
            };
            Some(Ok(process_ambient_value(value)))
        }
        "process.stdin_close" => {
            let Some(handle) = handle else {
                return Some(Err(process_ambient_diag(
                    "ProcessStdin.close() has no typed receiver handle",
                    span,
                )));
            };
            if !args.is_empty() {
                return Some(Err(process_ambient_diag(
                    "ProcessStdin.close() received unexpected arguments",
                    span,
                )));
            }
            let Some(child) = with_ambient_stdin(handle, Clone::clone) else {
                return Some(Err(process_ambient_diag(
                    "ProcessStdin.close() used an unavailable process handle",
                    span,
                )));
            };
            process_prelude::child_stdin_close(&child);
            Some(Ok(process_ambient_value(MirRuntimeValue::Unit)))
        }
        _ => None,
    }
}

fn process_stream_mode(handle: i64) -> process_prelude::ProcessStreamMode {
    // Non-ordering core enums use the ordinary heap-record ABI in Cranelift:
    // slot zero is the checked variant discriminant.
    let disc = Concurrency::with_runtime_mut(|rt| rt.heap.record_get_int(handle, 0))
        .unwrap_or(-1);
    match disc {
        0 => process_prelude::ProcessStreamMode::Stream,
        1 => process_prelude::ProcessStreamMode::Inherit,
        2 => process_prelude::ProcessStreamMode::Capture,
        _ => process_prelude::ProcessStreamMode::Stream,
    }
}

fn process_duration(ns: i64) -> process_prelude::Duration {
    process_prelude::Duration { ns }
}

fn jet_jit_process_cmd(cmd_list: i64) -> i64 {
    push_spec(process_prelude::spec_new(clone_string_list(cmd_list)))
}

fn jet_jit_process_run(cmd_list: i64) -> i64 {
    let spec = process_prelude::spec_new(clone_string_list(cmd_list));
    match process_prelude::spec_run(&spec) {
        Ok(result) => outcome_to_result(result),
        Err(error) => process_io_error_result(error),
    }
}

fn jet_jit_process_run_with_authority(cmd_list: i64, _authority: i64) -> i64 {
    let spec = process_prelude::spec_new(clone_string_list(cmd_list));
    let authority_wire =
        Concurrency::with_runtime_mut(|rt| crate::Collections::authority_wire(rt, _authority));
    let spec = process_prelude::spec_under_wire(spec, &authority_wire);
    match process_prelude::spec_run(&spec) {
        Ok(result) => outcome_to_result(result),
        Err(error) => process_io_error_result(error),
    }
}

fn jet_jit_process_pipeline(spec_list: i64) -> i64 {
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

fn jet_jit_process_spec_stdout(spec: i64, mode: i64) -> i64 {
    let mode = process_stream_mode(mode);
    update_spec(spec, |spec| process_prelude::spec_stdout(spec, &mode))
}

fn jet_jit_process_spec_stderr(spec: i64, mode: i64) -> i64 {
    let mode = process_stream_mode(mode);
    update_spec(spec, |spec| process_prelude::spec_stderr(spec, &mode))
}

fn jet_jit_process_spec_stdin(spec: i64, mode: i64) -> i64 {
    let mode = process_stream_mode(mode);
    update_spec(spec, |spec| process_prelude::spec_stdin(spec, &mode))
}

fn jet_jit_process_spec_timeout(spec: i64, timeout: i64) -> i64 {
    let timeout = process_duration(timeout);
    update_spec(spec, |spec| process_prelude::spec_timeout(spec, &timeout))
}

fn jet_jit_process_spec_output_limit(spec: i64, limit: i64) -> i64 {
    update_spec(spec, |spec| process_prelude::spec_output_limit(spec, limit))
}

fn jet_jit_process_spec_cpu_time_limit(spec: i64, timeout: i64) -> i64 {
    let timeout = process_duration(timeout);
    update_spec(spec, |spec| {
        process_prelude::spec_cpu_time_limit(spec, &timeout)
    })
}

fn jet_jit_process_spec_memory_limit(spec: i64, limit: i64) -> i64 {
    update_spec(spec, |spec| process_prelude::spec_memory_limit(spec, limit))
}

fn jet_jit_process_spec_open_file_limit(spec: i64, limit: i64) -> i64 {
    update_spec(spec, |spec| {
        process_prelude::spec_open_file_limit(spec, limit)
    })
}

fn jet_jit_process_spec_run(spec: i64) -> i64 {
    let Some(spec) = clone_spec(spec) else {
        return invalid_process_spec();
    };
    match process_prelude::spec_run(&spec) {
        Ok(result) => outcome_to_result(result),
        Err(error) => process_io_error_result(error),
    }
}

fn jet_jit_process_spec_run_checked(spec: i64) -> i64 {
    let Some(spec) = clone_spec(spec) else {
        return invalid_process_spec();
    };
    match process_prelude::spec_run_checked(&spec) {
        Ok(result) => outcome_to_result(result),
        Err(error) => process_io_error_result(error),
    }
}

fn jet_jit_process_spec_under(spec: i64, authority: i64) -> i64 {
    let authority_wire =
        Concurrency::with_runtime_mut(|rt| crate::Collections::authority_wire(rt, authority));
    update_spec(spec, |spec| {
        process_prelude::spec_under_wire(spec, &authority_wire)
    })
}

fn jet_jit_process_spec_plan(spec: i64) -> i64 {
    let Some(spec) = clone_spec(spec) else {
        return invalid_process_spec();
    };
    match process_prelude::spec_plan(&spec) {
        Ok(plan) => result_ok(alloc_process_plan(&plan) as u64),
        Err(error) => process_io_error_result(error),
    }
}

fn jet_jit_process_spec_spawn(spec: i64) -> i64 {
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

fn jet_jit_process_spec_env_clear(spec: i64) -> i64 {
    update_spec(spec, process_prelude::spec_env_clear)
}

fn jet_jit_process_spec_detached(spec: i64) -> i64 {
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

fn jet_jit_process_spec_terminal(spec: i64) -> i64 {
    update_spec(spec, process_prelude::spec_terminal)
}

fn jet_jit_process_spec_terminal_with_policy(spec: i64, policy: i64) -> i64 {
    let policy = terminal_policy_from_handle(policy);
    update_spec(spec, |spec| {
        process_prelude::spec_terminal_with_policy(spec, &policy)
    })
}

fn jet_jit_process_spec_abilities(spec: i64) -> i64 {
    let Some(spec) = clone_spec(spec) else {
        return 0;
    };
    let facts = process_prelude::spec_abilities(&spec);
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

fn jet_jit_process_child_terminal(child: i64) -> i64 {
    if child <= 0 {
        return 0;
    }
    let idx = (child as usize).saturating_sub(1);
    Concurrency::with_runtime_mut(|rt| {
        rt.process_children
            .get(idx)
            .and_then(|child| child.terminal.as_ref().ok())
            // `ProcessChild.terminal` is a packed Option in the Cranelift
            // ABI: zero is None and a present payload is stored as value + 1.
            // Return the child handle itself after the lowering unpacks it.
            .map(|_| child.wrapping_add(1))
            .unwrap_or(0)
    })
}

fn jet_jit_terminal_session_resize(session: i64, size: i64) -> i64 {
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

fn jet_jit_process_spec_cwd(spec: i64, cwd: i64) -> i64 {
    let cwd = clone_string(cwd);
    update_spec(spec, |spec| process_prelude::spec_cwd(spec, &cwd))
}

fn jet_jit_process_spec_env(spec: i64, name: i64, value: i64) -> i64 {
    let name = clone_string(name);
    let value = clone_string(value);
    update_spec(spec, |spec| process_prelude::spec_env(spec, &name, &value))
}

fn jet_jit_process_spec_env_remove(spec: i64, name: i64) -> i64 {
    let name = clone_string(name);
    update_spec(spec, |spec| process_prelude::spec_env_remove(spec, &name))
}

/// 0 selects stdout; 1 selects stderr.
fn jet_jit_process_stream_lines(child: i64, tag: i64) -> i64 {
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

fn jet_jit_process_stdin_write(child: i64, text: i64) -> i64 {
    if child <= 0 {
        return invalid_process_child();
    }
    let text = clone_string(text);
    let index = (child as usize).saturating_sub(1);
    let result = Concurrency::with_runtime_mut(|rt| {
        rt.process_children
            .get(index)
            .map(|child| process_prelude::child_stdin_write(child, &text))
    });
    match result {
        Some(Ok(())) => result_ok(0),
        Some(Err(error)) => process_io_error_result(error),
        None => invalid_process_child(),
    }
}

fn jet_jit_process_stdin_close(child: i64) -> i64 {
    if child <= 0 {
        Concurrency::with_runtime_mut(|rt| rt.set_host_fault("invalid ProcessChild for stdin close"));
        return 0;
    }
    let index = (child as usize).saturating_sub(1);
    let closed = Concurrency::with_runtime_mut(|rt| {
        rt.process_children
            .get(index)
            .map(process_prelude::child_stdin_close)
    });
    if closed.is_none() {
        Concurrency::with_runtime_mut(|rt| rt.set_host_fault("invalid ProcessChild for stdin close"));
    }
    0
}

fn jet_jit_process_on_signal(signal: i64) -> i64 {
    let discriminant = Concurrency::with_runtime_mut(|rt| rt.heap.record_get_int(signal, 0));
    let Some(signal) = discriminant.and_then(|value| match value {
        0 => Some(process_prelude::ProcessSignal::Term),
        1 => Some(process_prelude::ProcessSignal::Hup),
        2 => Some(process_prelude::ProcessSignal::Int),
        _ => None,
    }) else {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_host_fault("invalid ProcessSignal value");
        });
        return 0;
    };
    process_prelude::process_on_signal(signal);
    0
}

fn jet_jit_process_child_id(child: i64) -> i64 {
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

fn jet_jit_process_child_exited(child: i64) -> i64 {
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
        rt.process_children.get(idx).map(|child| operation(child))
    });
    match result {
        Some(Ok(())) => result_ok(0),
        Some(Err(error)) => process_io_error_result(error),
        None => invalid_process_child(),
    }
}

fn jet_jit_process_child_kill(child: i64) -> i64 {
    process_child_unit(child, process_prelude::child_kill)
}

fn jet_jit_process_child_terminate(child: i64) -> i64 {
    process_child_unit(child, process_prelude::child_terminate)
}

fn jet_jit_process_child_interrupt(child: i64) -> i64 {
    process_child_unit(child, process_prelude::child_interrupt)
}

fn jet_jit_process_child_wait(child: i64) -> i64 {
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

/// Marshal a packed process `IOError` into the shared Prelude display text.
fn jet_jit_process_error_show(packed: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = process_error_show_text(packed, &rt.heap);
        rt.heap.alloc_string(text)
    })
}

host_fns! {
    struct ProcessHostFns;
    register: register_process_symbols;
    declare: declare_process_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::Module;
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
    error_show: "jet_jit_process_error_show" => jet_jit_process_error_show: sig_unary;
    on_signal: "jet_jit_process_on_signal" => jet_jit_process_on_signal: sig_unary;
    cmd: "jet_jit_process_cmd" => jet_jit_process_cmd: sig_unary;
    run: "jet_jit_process_run" => jet_jit_process_run: sig_unary;
    run_with_authority: "jet_jit_process_run_with_authority" => jet_jit_process_run_with_authority: sig_binary;
    run_with_authority_shared: "jet_std_process_run_with_authority" => jet_jit_process_run_with_authority: sig_binary;
    pipeline: "jet_jit_process_pipeline" => jet_jit_process_pipeline: sig_unary;
    spec_stdout: "jet_jit_process_spec_stdout" => jet_jit_process_spec_stdout: sig_binary;
    spec_stderr: "jet_jit_process_spec_stderr" => jet_jit_process_spec_stderr: sig_binary;
    spec_stdin: "jet_jit_process_spec_stdin" => jet_jit_process_spec_stdin: sig_binary;
    spec_timeout: "jet_jit_process_spec_timeout" => jet_jit_process_spec_timeout: sig_binary;
    spec_output_limit: "jet_jit_process_spec_output_limit" => jet_jit_process_spec_output_limit: sig_binary;
    spec_cpu_time_limit: "jet_jit_process_spec_cpu_time_limit" => jet_jit_process_spec_cpu_time_limit: sig_binary;
    spec_memory_limit: "jet_jit_process_spec_memory_limit" => jet_jit_process_spec_memory_limit: sig_binary;
    spec_open_file_limit: "jet_jit_process_spec_open_file_limit" => jet_jit_process_spec_open_file_limit: sig_binary;
    spec_cwd: "jet_jit_process_spec_cwd" => jet_jit_process_spec_cwd: sig_binary;
    spec_env: "jet_jit_process_spec_env" => jet_jit_process_spec_env: sig_ternary;
    spec_env_remove: "jet_jit_process_spec_env_remove" => jet_jit_process_spec_env_remove: sig_binary;
    spec_env_clear: "jet_jit_process_spec_env_clear" => jet_jit_process_spec_env_clear: sig_unary;
    spec_detached: "jet_jit_process_spec_detached" => jet_jit_process_spec_detached: sig_unary;
    spec_terminal: "jet_jit_process_spec_terminal" => jet_jit_process_spec_terminal: sig_unary;
    spec_terminal_with_policy: "jet_jit_process_spec_terminal_with_policy" => jet_jit_process_spec_terminal_with_policy: sig_binary;
    spec_abilities: "jet_jit_process_spec_abilities" => jet_jit_process_spec_abilities: sig_unary;
    spec_under: "jet_jit_process_spec_under" => jet_jit_process_spec_under: sig_binary;
    spec_under_shared: "jet_std_process_spec_under" => jet_jit_process_spec_under: sig_binary;
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
    stdin_write: "jet_jit_process_stdin_write" => jet_jit_process_stdin_write: sig_binary;
    stdin_close: "jet_jit_process_stdin_close" => jet_jit_process_stdin_close: sig_unary;
    spec_stdin_shared: "jet_process_spec_stdin" => jet_jit_process_spec_stdin: sig_binary;
    spec_spawn_shared: "jet_process_spec_spawn" => jet_jit_process_spec_spawn: sig_unary;
    child_wait_shared: "jet_process_child_wait" => jet_jit_process_child_wait: sig_unary;
    stdin_write_shared: "jet_process_stdin_write" => jet_jit_process_stdin_write: sig_binary;
    stdin_close_shared: "jet_process_stdin_close" => jet_jit_process_stdin_close: sig_unary;
    spec_stdout_shared: "jet_process_spec_stdout" => jet_jit_process_spec_stdout: sig_binary;
    spec_stderr_shared: "jet_process_spec_stderr" => jet_jit_process_spec_stderr: sig_binary;
    spec_timeout_shared: "jet_process_spec_timeout" => jet_jit_process_spec_timeout: sig_binary;
    spec_output_limit_shared: "jet_process_spec_output_limit" => jet_jit_process_spec_output_limit: sig_binary;
    spec_cpu_time_limit_shared: "jet_process_spec_cpu_time_limit" => jet_jit_process_spec_cpu_time_limit: sig_binary;
    spec_memory_limit_shared: "jet_process_spec_memory_limit" => jet_jit_process_spec_memory_limit: sig_binary;
    spec_open_file_limit_shared: "jet_process_spec_open_file_limit" => jet_jit_process_spec_open_file_limit: sig_binary;
    spec_cwd_shared: "jet_process_spec_cwd" => jet_jit_process_spec_cwd: sig_binary;
    spec_env_shared: "jet_process_spec_env" => jet_jit_process_spec_env: sig_ternary;
    spec_env_remove_shared: "jet_process_spec_env_remove" => jet_jit_process_spec_env_remove: sig_binary;
    spec_env_clear_shared: "jet_process_spec_env_clear" => jet_jit_process_spec_env_clear: sig_unary;
    spec_detached_shared: "jet_process_spec_detached" => jet_jit_process_spec_detached: sig_unary;
    spec_terminal_shared: "jet_process_spec_terminal" => jet_jit_process_spec_terminal: sig_unary;
    spec_terminal_with_policy_shared: "jet_process_spec_terminal_with_policy" => jet_jit_process_spec_terminal_with_policy: sig_binary;
    spec_abilities_shared: "jet_process_spec_abilities" => jet_jit_process_spec_abilities: sig_unary;
    spec_plan_shared: "jet_process_spec_plan" => jet_jit_process_spec_plan: sig_unary;
    spec_run_shared: "jet_process_spec_run" => jet_jit_process_spec_run: sig_unary;
    spec_run_checked_shared: "jet_process_spec_run_checked" => jet_jit_process_spec_run_checked: sig_unary;
    process_cmd_shared: "jet_std_process_cmd" => jet_jit_process_cmd: sig_unary;
    process_run_shared: "jet_std_process_run" => jet_jit_process_run: sig_unary;
    process_pipeline_shared: "jet_std_process_pipeline" => jet_jit_process_pipeline: sig_unary;
    child_id_shared: "jet_process_child_id" => jet_jit_process_child_id: sig_unary;
    child_exited_shared: "jet_process_child_exited" => jet_jit_process_child_exited: sig_unary;
    child_kill_shared: "jet_process_child_kill" => jet_jit_process_child_kill: sig_unary;
    child_terminate_shared: "jet_process_child_terminate" => jet_jit_process_child_terminate: sig_unary;
    child_interrupt_shared: "jet_process_child_interrupt" => jet_jit_process_child_interrupt: sig_unary;
}
