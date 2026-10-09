//! Resident JIT process adapters.
//!
//! Process policy and lifecycle semantics live in the shared Prelude included
//! by the interpreter. This module only stores opaque handles and marshals
//! values and typed errors across the Cranelift ABI.

use std::cell::RefCell;

use crate::ProcessPrelude::process_prelude;
use jet_codegen::Comptime::{AmbientMirHandleResult, DevSink};
use jet_foundation::AST::{CtValue, Type};
use jet_foundation::Diagnostics::{Diagnostic, Span};
use jet_foundation::MIR::{
    MirHandleToken, MirRuntimeValue, MirType, MirTypeKind, MirNominalRef,
};
use jet_foundation::Outcome::JetAbsent;

pub(crate) type JitProcessSpec = process_prelude::ProcessSpec;
pub(crate) type JitProcessChild = process_prelude::ProcessChild;


#[derive(Default)]
struct InterpreterProcessState {
    specs: Vec<Option<JitProcessSpec>>,
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

/// Scope the opaque process carriers used by one interpreter run.
///
/// Process semantics remain in the shared Prelude. This scope only gives the
/// interpreter a carrier space; handles are negative indices into it.
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
    INTERPRETER_PROCESS_STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.specs.push(Some(spec));
        -(state.specs.len() as i64)
    })
}

fn update_ambient_spec(
    handle: i64,
    f: impl FnOnce(JitProcessSpec) -> JitProcessSpec,
) -> Option<i64> {
    let index = local_process_index(handle)?;
    INTERPRETER_PROCESS_STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        let spec = state.specs.get_mut(index)?;
        let current = spec.take()?;
        *spec = Some(f(current));
        Some(handle)
    })
}

fn with_ambient_spec<R>(
    handle: i64,
    f: impl FnOnce(&JitProcessSpec) -> R,
) -> Option<R> {
    let index = local_process_index(handle)?;
    INTERPRETER_PROCESS_STATE.with(|slot| slot.borrow().specs.get(index).and_then(Option::as_ref).map(f))
}

fn push_ambient_child(child: JitProcessChild) -> i64 {
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
    let index = local_process_index(handle)?;
    INTERPRETER_PROCESS_STATE.with(|slot| slot.borrow().children.get(index).map(f))
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
        "__spec_fields" => Some(ct_process_spec_fields(args, span)),
        "__with_directory_raw" => Some(ct_process_with_directory_raw(args, span)),
        "pipeline" => Some(ct_process_pipeline(&args, span)),
        _ => None,
    }
}

fn ct_process_spec_fields(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let args = args.into_iter().map(|value| jet_codegen::Comptime::MirBridge::ct_to_mir_value(value, span))
        .collect::<Result<Vec<_>, _>>()?;
    let [cmd, has_cwd, cwd, env_clear, names, values, remove, has_stdin, stdin, stdout, stderr,
        has_timeout, timeout, has_output_limit, output_limit, has_cpu_limit, cpu_limit,
        has_memory_limit, memory_limit, has_open_limit, open_limit, detached,
        has_terminal, terminal, has_policy, policy] = args.as_slice() else {
        return Err(process_ambient_diag("process field transfer has the wrong checked arity", span));
    };
    let operation = "core.process.__spec_fields";
    let boolean = |value: &MirRuntimeValue| match value {
        MirRuntimeValue::Bool(value) => Ok(*value),
        _ => Err(process_ambient_diag("process field transfer expects a Bool carrier", span)),
    };
    let spec = process_prelude::spec_fields(
        mir_process_string_list(cmd, operation, "argv", span)?,
        boolean(has_cwd)?, mir_process_string(cwd, operation, span)?,
        boolean(env_clear)?,
        mir_process_string_list(names, operation, "environment key", span)?,
        mir_process_string_list(values, operation, "environment value", span)?,
        mir_process_string_list(remove, operation, "environment removal", span)?,
        boolean(has_stdin)?, mir_process_stream_mode(stdin, span)?,
        mir_process_stream_mode(stdout, span)?, mir_process_stream_mode(stderr, span)?,
        boolean(has_timeout)?, mir_process_int(timeout, operation, span)?,
        boolean(has_output_limit)?, mir_process_int(output_limit, operation, span)?,
        boolean(has_cpu_limit)?, mir_process_int(cpu_limit, operation, span)?,
        boolean(has_memory_limit)?, mir_process_int(memory_limit, operation, span)?,
        boolean(has_open_limit)?, mir_process_int(open_limit, operation, span)?,
        boolean(detached)?,
        boolean(has_terminal)?, mir_process_terminal_policy(terminal, operation, span)?,
        boolean(has_policy)?, mir_process_string(policy, operation, span)?,
    );
    let token = MirHandleToken::new(jet_foundation::MIR::MirHandleId(u64::MAX), push_ambient_spec(spec));
    Ok(CtValue::Closure(std::sync::Arc::new(jet_foundation::AST::ClosureData {
        lambda: jet_foundation::AST::Lambda {
            take_names: Vec::new(), params: Vec::new(), effects: None,
            body: jet_foundation::AST::LambdaBody::Block(Vec::new()),
            span: Span::new(0, 0), meta: jet_foundation::AST::LambdaMeta::default(),
        },
        captured: std::collections::HashMap::new(),
        return_type: None, opaque: Some(jet_foundation::AST::CtOpaque::new(token)),
    })))
}

fn ct_process_with_directory_raw(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let [spec, source_fd, child_fd, as_cwd]: [CtValue; 4] = args.try_into()
        .map_err(|_| process_ambient_diag("held directory transfer has the wrong checked arity", span))?;
    let CtValue::Closure(data) = &spec else {
        return Err(process_ambient_diag("held directory transfer expects an opaque ProcessSpec", span));
    };
    let raw = data.opaque.as_ref().and_then(|owner| owner.downcast_ref::<MirHandleToken>())
        .and_then(MirHandleToken::raw).filter(|raw| *raw != 0)
        .ok_or_else(|| process_ambient_diag("held directory transfer received a moved ProcessSpec", span))?;
    let operation = "core.process.__with_directory_raw";
    let source_fd = mir_process_int(&jet_codegen::Comptime::MirBridge::ct_to_mir_value(source_fd, span)?, operation, span)?;
    let child_fd = mir_process_int(&jet_codegen::Comptime::MirBridge::ct_to_mir_value(child_fd, span)?, operation, span)?;
    let CtValue::Bool(as_cwd) = as_cwd else {
        return Err(process_ambient_diag("held directory transfer expects a Bool", span));
    };
    let mut available = false;
    let mut assigned_fd = -1;
    let mut errno = 0;
    update_ambient_spec(raw, |plan| {
        let (plan, present, assigned, code) = process_prelude::spec_with_directory_raw(plan, source_fd, child_fd, as_cwd);
        available = present;
        assigned_fd = assigned.to_i64().expect("descriptor fd is i32");
        errno = code.to_i64().expect("descriptor errno is i32");
        plan
    }).ok_or_else(|| process_ambient_diag("held directory transfer received an unknown ProcessSpec", span))?;
    Ok(CtValue::Struct { type_name: "(spec,available,assigned_fd,errno)".to_string(), fields: vec![
        ("spec".to_string(), spec), ("available".to_string(), CtValue::Bool(available)),
        ("assigned_fd".to_string(), CtValue::Int(assigned_fd)),
        ("errno".to_string(), CtValue::Int(errno))] })
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

pub(crate) fn mir_io_error(error: process_prelude::IOError) -> MirRuntimeValue {
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
        process_prelude::IOError::AlreadyExists(context) => {
            ("AlreadyExists", vec![(None, mir_io_context(context))])
        }
        process_prelude::IOError::Unsupported(context) => {
            ("Unsupported", vec![(None, mir_io_context(context))])
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
    operation: &str,
    item_description: &str,
    span: Span,
) -> Result<Vec<String>, Diagnostic> {
    let MirRuntimeValue::List(values) = value else {
        return Err(process_ambient_diag(
            format!("{operation} expects a List<String>"),
            span,
        ));
    };
    values
        .iter()
        .map(|value| match value {
            MirRuntimeValue::String(value) => Ok(value.clone()),
            _ => Err(process_ambient_diag(
                format!("{operation} expects every {item_description} to be a String"),
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
        "process.spec.arg" => {
            let [extra] = args.as_slice() else {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec.arg() expects one String argument",
                    span,
                )));
            };
            let extra = match mir_process_string(extra, "ProcessSpec.arg()", span) {
                Ok(extra) => extra,
                Err(error) => return Some(Err(error)),
            };
            Some(process_ambient_spec_update(
                "ProcessSpec.arg()",
                handle,
                span,
                |spec| process_prelude::spec_arg(spec, &extra),
            ))
        }
        "process.spec.args_extend" => {
            let [extra] = args.as_slice() else {
                return Some(Err(process_ambient_diag(
                    "ProcessSpec.args_extend() expects one List<String> argument",
                    span,
                )));
            };
            let extra = match mir_process_string_list(
                extra,
                "ProcessSpec.args_extend()",
                "argument",
                span,
            ) {
                Ok(extra) => extra,
                Err(error) => return Some(Err(error)),
            };
            Some(process_ambient_spec_update(
                "ProcessSpec.args_extend()",
                handle,
                span,
                |spec| process_prelude::spec_args_extend(spec, &extra),
            ))
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

