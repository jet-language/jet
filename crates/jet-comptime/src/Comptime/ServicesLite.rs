//! D-SERVICE1=D / I9: the typed `core.service` slice includes Prelude
//! `Services.rs`; older procedural helpers remain private migration machinery.

use super::Diagnostics::unsupported;
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{CtFloat, CtReport, CtValue, Type};
use crate::MIR::{MirNominalRef, MirRuntimeValue, MirType, MirTypeKind};

trait JetShow {
    fn jet_show(&self) -> String;
}

trait JetDisplay {
    fn jet_display(&self) -> String;
}

trait JetDebug {
    fn jet_debug(&self) -> String;
}

pub type WorkflowWaitHook = fn(i64) -> JetServiceWorkflowWait<()>;

thread_local! {
    static WORKFLOW_WAIT_HOOK: std::cell::Cell<Option<WorkflowWaitHook>> =
        const { std::cell::Cell::new(None) };
}
thread_local! {
    static INTERPRETER_JOB_PAYLOADS:
        std::cell::RefCell<Vec<(String, Vec<u8>, MirRuntimeValue)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}
thread_local! {
    static SERVICE_TREES: std::cell::RefCell<Vec<MirRuntimeValue>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

const SERVICE_TREE_SLOT: &str = "__slot";

fn is_service_tree(value: &MirRuntimeValue) -> bool {
    matches!(value, MirRuntimeValue::Struct { type_name, .. } if type_name == "ServiceTree")
}

pub fn intern_slot(value: &MirRuntimeValue) -> Option<usize> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != "ServiceTree" {
        return None;
    }
    fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        (SERVICE_TREE_SLOT, MirRuntimeValue::Int(slot)) if *slot >= 0 => {
            usize::try_from(*slot).ok()
        }
        _ => None,
    })
}

fn tag_tree_slot(value: MirRuntimeValue, slot: usize) -> MirRuntimeValue {
    let MirRuntimeValue::Struct { type_name, mut fields } = value else {
        return value;
    };
    if type_name != "ServiceTree" {
        return MirRuntimeValue::Struct { type_name, fields };
    }
    fields.retain(|(name, _)| name != SERVICE_TREE_SLOT);
    fields.push((
        SERVICE_TREE_SLOT.to_string(),
        MirRuntimeValue::Int(slot as i64),
    ));
    MirRuntimeValue::Struct { type_name, fields }
}

fn intern_new_tree(tree: MirRuntimeValue) -> MirRuntimeValue {
    SERVICE_TREES.with(|trees| {
        let mut trees = trees.borrow_mut();
        let slot = trees.len();
        let tagged = tag_tree_slot(tree, slot);
        trees.push(tagged.clone());
        tagged
    })
}

pub fn intern_resolve(value: MirRuntimeValue) -> MirRuntimeValue {
    let Some(slot) = intern_slot(&value) else {
        return value;
    };
    SERVICE_TREES.with(|trees| trees.borrow().get(slot).cloned().unwrap_or(value))
}

fn intern_store(slot: Option<usize>, tree: MirRuntimeValue) {
    if matches!(tree, MirRuntimeValue::Unit) || !is_service_tree(&tree) {
        return;
    }
    SERVICE_TREES.with(|trees| {
        let mut trees = trees.borrow_mut();
        if let Some(slot) = slot.filter(|slot| *slot < trees.len()) {
            trees[slot] = tag_tree_slot(tree, slot);
            return;
        }
        let slot = trees.len();
        trees.push(tag_tree_slot(tree, slot));
    });
}

fn intern_tree_value(value: MirRuntimeValue) -> MirRuntimeValue {
    match value {
        MirRuntimeValue::Present(inner) if is_service_tree(&inner) && intern_slot(&inner).is_none() => {
            MirRuntimeValue::Present(Box::new(intern_new_tree(*inner)))
        }
        value if is_service_tree(&value) && intern_slot(&value).is_none() => intern_new_tree(value),
        value => value,
    }
}

pub fn intern_commit(slot: Option<usize>, value: MirRuntimeValue) -> MirRuntimeValue {
    match take_mut_runtime(value) {
        Ok((tree, result)) => {
            intern_store(slot.or_else(|| intern_slot(&tree)), tree);
            intern_tree_value(result)
        }
        Err(value) => intern_tree_value(value),
    }
}

pub struct JobPayloadScope(usize);

impl Drop for JobPayloadScope {
    fn drop(&mut self) {
        INTERPRETER_JOB_PAYLOADS.with(|payloads| {
            payloads.borrow_mut().truncate(self.0);
        });
    }
}

pub fn job_payload_scope() -> JobPayloadScope {
    let mark = INTERPRETER_JOB_PAYLOADS.with(|payloads| payloads.borrow().len());
    JobPayloadScope(mark)
}

fn remember_interpreter_job_payload(payload: &JetJobPayload, value: &MirRuntimeValue) {
    INTERPRETER_JOB_PAYLOADS.with(|payloads| {
        let mut payloads = payloads.borrow_mut();
        if let Some(existing) = payloads
            .iter_mut()
            .find(|(type_id, bytes, _)| type_id == &payload.type_id && bytes == &payload.bytes)
        {
            existing.2 = value.clone();
        } else {
            payloads.push((payload.type_id.clone(), payload.bytes.clone(), value.clone()));
        }
    });
}

pub fn interpreter_job_payload(payload: &JetJobPayload) -> Option<MirRuntimeValue> {
    INTERPRETER_JOB_PAYLOADS.with(|payloads| {
        payloads
            .borrow()
            .iter()
            .rev()
            .find(|(type_id, bytes, _)| type_id == &payload.type_id && bytes == &payload.bytes)
            .map(|(_, _, value)| value.clone())
    })
}

struct WorkflowWaitHookGuard(Option<WorkflowWaitHook>);

impl Drop for WorkflowWaitHookGuard {
    fn drop(&mut self) {
        WORKFLOW_WAIT_HOOK.with(|hook| hook.set(self.0.take()));
    }
}

/// Install the resident engine's scheduler adapter around one canonical
/// ServicesLite call. Direct Comptime use falls back to a blocking timer.
pub fn with_workflow_wait<F, R>(wait: WorkflowWaitHook, f: F) -> R
where
    F: FnOnce() -> R,
{
    let previous = WORKFLOW_WAIT_HOOK.with(|hook| hook.replace(Some(wait)));
    let _guard = WorkflowWaitHookGuard(previous);
    f()
}

fn jet_services_workflow_sleep_wait(nanos: i64) -> JetServiceWorkflowWait<()> {
    if let Some(wait) = WORKFLOW_WAIT_HOOK.with(|hook| hook.get()) {
        return wait(nanos);
    }
    std::thread::sleep(std::time::Duration::from_nanos(nanos.max(0) as u64));
    JetServiceWorkflowWait::Ready(())
}

#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../../jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs");
use jet_crypto_entropy::{jet_crypto_entropy_fill, JetCryptoEntropyError};
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../../jet-codegen/src/Prelude/CoreLib/Top/SHA256Raw.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../../jet-codegen/src/Prelude/TaskGroup.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../../jet-codegen/src/Prelude/JobQueueTypes.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../../jet-codegen/src/Prelude/CoreLib/Top/ServiceAuthority.rs");


#[allow(unused_imports)]
pub use jet_foundation::Devtools::{
    jet_devtools_publish_event, JetDevtoolsEvent, JET_DEVTOOLS_MAX_HISTORY,
};
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../../jet-codegen/src/Prelude/WorkflowWait.rs");
include!("../../../jet-codegen/src/Prelude/Core/DevtoolsTopologyPanel.rs");
include!("../../../jet-codegen/src/Prelude/CoreLib/Top/Services.rs");

fn mir_compat_type(ty: &Type) -> MirType {
    MirType::from_kind(MirTypeKind::Apply {
        name: MirNominalRef::from_name(ty.name()),
        args: Vec::new(),
    })
}

fn mir_absent(name: &str) -> MirRuntimeValue {
    MirRuntimeValue::Absent {
        element: MirType::from_kind(MirTypeKind::Apply {
            name: MirNominalRef::from_name(name),
            args: Vec::new(),
        }),
    }
}

fn restart_to_runtime(r: JetServiceRestart) -> MirRuntimeValue {
    MirRuntimeValue::Enum {
        type_name: "ServiceRestart".to_string(),
        variant: match r {
            JetServiceRestart::OneForOne => "OneForOne".to_string(),
            JetServiceRestart::OneForAll => "OneForAll".to_string(),
            JetServiceRestart::RestForOne => "RestForOne".to_string(),
        },
        args: Vec::new(),
    }
}

fn runtime_to_restart(v: &MirRuntimeValue, span: Span) -> Result<JetServiceRestart, Diagnostic> {
    match v {
        MirRuntimeValue::Enum {
            type_name, variant, ..
        } if type_name == "ServiceRestart" => match variant.as_str() {
            "OneForOne" => Ok(JetServiceRestart::OneForOne),
            "OneForAll" => Ok(JetServiceRestart::OneForAll),
            "RestForOne" => Ok(JetServiceRestart::RestForOne),
            _ => Err(unsupported("ServiceRestart", span)),
        },
        _ => Err(unsupported("ServiceRestart", span)),
    }
}

/// D-SERVICE1=D: the restart budget crosses the tier boundary as data through
/// this one pair, so a group's policy row cannot mean one thing in an AOT local
/// and another in a `MirRuntimeValue`. `is_valid` is the Prelude's own range check.
fn restart_budget_to_runtime(b: &JetServiceRestartBudget) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "ServiceRestartBudget".to_string(),
        fields: vec![
            ("max".to_string(), MirRuntimeValue::Int(b.max)),
            ("per_ms".to_string(), MirRuntimeValue::Int(b.per_ms)),
        ],
    }
}

fn runtime_to_restart_budget(v: &MirRuntimeValue, span: Span) -> Result<JetServiceRestartBudget, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = v else {
        return Err(unsupported("ServiceRestartBudget", span));
    };
    if type_name != "ServiceRestartBudget" {
        return Err(unsupported("ServiceRestartBudget", span));
    }
    let int = |name: &str| match fields.iter().find(|(n, _)| n == name).map(|(_, v)| v) {
        Some(MirRuntimeValue::Int(n)) => Ok(*n),
        _ => Err(unsupported("restart budget field", span)),
    };
    let budget = JetServiceRestartBudget {
        max: int("max")?,
        per_ms: int("per_ms")?,
    };
    if !budget.is_valid() {
        return Err(unsupported("restart budget range", span));
    }
    Ok(budget)
}

fn delivery_to_runtime(d: JetServiceDelivery) -> MirRuntimeValue {
    MirRuntimeValue::Enum {
        type_name: "ServiceDelivery".to_string(),
        variant: match d {
            JetServiceDelivery::AtMostOnce => "AtMostOnce".to_string(),
            JetServiceDelivery::DurableAtLeastOnce => "DurableAtLeastOnce".to_string(),
        },
        args: Vec::new(),
    }
}

fn runtime_to_delivery(v: &MirRuntimeValue, span: Span) -> Result<JetServiceDelivery, Diagnostic> {
    match v {
        MirRuntimeValue::Enum {
            type_name, variant, ..
        } if type_name == "ServiceDelivery" => match variant.as_str() {
            "AtMostOnce" => Ok(JetServiceDelivery::AtMostOnce),
            "DurableAtLeastOnce" => Ok(JetServiceDelivery::DurableAtLeastOnce),
            _ => Err(unsupported("ServiceDelivery", span)),
        },
        _ => Err(unsupported("ServiceDelivery", span)),
    }
}

fn task_outcome_to_runtime(outcome: &JetTaskOutcome) -> MirRuntimeValue {
    match outcome {
        JetTaskOutcome::Finished => MirRuntimeValue::Enum {
            type_name: "TaskOutcome".to_string(),
            variant: "Finished".to_string(),
            args: Vec::new(),
        },
        JetTaskOutcome::Panicked(reason) => MirRuntimeValue::Enum {
            type_name: "TaskOutcome".to_string(),
            variant: "Panicked".to_string(),
            args: vec![(None, MirRuntimeValue::String(reason.clone()))],
        },
        JetTaskOutcome::Cancelled => MirRuntimeValue::Enum {
            type_name: "TaskOutcome".to_string(),
            variant: "Cancelled".to_string(),
            args: Vec::new(),
        },
        JetTaskOutcome::DeadlineBlown => MirRuntimeValue::Enum {
            type_name: "TaskOutcome".to_string(),
            variant: "DeadlineBlown".to_string(),
            args: Vec::new(),
        },
    }
}

fn runtime_to_task_outcome(value: &MirRuntimeValue, span: Span) -> Result<JetTaskOutcome, Diagnostic> {
    let MirRuntimeValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(unsupported("TaskOutcome", span));
    };
    if type_name != "TaskOutcome" {
        return Err(unsupported("TaskOutcome", span));
    }
    let outcome = match variant.as_str() {
        "Finished" if args.is_empty() => JetTaskOutcome::Finished,
        "Cancelled" if args.is_empty() => JetTaskOutcome::Cancelled,
        "DeadlineBlown" if args.is_empty() => JetTaskOutcome::DeadlineBlown,
        "Panicked" if args.len() == 1 => match &args[0].1 {
            MirRuntimeValue::String(reason) => JetTaskOutcome::Panicked(reason.clone()),
            _ => return Err(unsupported("TaskOutcome.Panicked reason", span)),
        },
        _ => return Err(unsupported("TaskOutcome variant", span)),
    };
    if !matches!(&outcome, JetTaskOutcome::Panicked(reason) if reason.is_empty()) {
        Ok(outcome)
    } else {
        Err(unsupported("TaskOutcome panic reason", span))
    }
}

fn task_status_to_runtime(status: &JetTaskStatus) -> MirRuntimeValue {
    MirRuntimeValue::Enum {
        type_name: "TaskStatus".to_string(),
        variant: match status {
            JetTaskStatus::Running => "Running",
            JetTaskStatus::Paused => "Paused",
            JetTaskStatus::CancelRequested => "CancelRequested",
        }
        .to_string(),
        args: Vec::new(),
    }
}

fn runtime_to_task_status(value: &MirRuntimeValue, span: Span) -> Result<JetTaskStatus, Diagnostic> {
    match value {
        MirRuntimeValue::Enum {
            type_name,
            variant,
            args,
        } if type_name == "TaskStatus" && args.is_empty() => match variant.as_str() {
            "Running" => Ok(JetTaskStatus::Running),
            "Paused" => Ok(JetTaskStatus::Paused),
            "CancelRequested" => Ok(JetTaskStatus::CancelRequested),
            _ => Err(unsupported("TaskStatus variant", span)),
        },
        _ => Err(unsupported("TaskStatus", span)),
    }
}

fn endpoint_to_runtime(e: &JetServiceEndpoint) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "ServiceEndpoint".to_string(),
        fields: vec![
            ("tree".to_string(), MirRuntimeValue::String(e.tree.clone())),
            ("worker".to_string(), MirRuntimeValue::String(e.worker.clone())),
            ("generation".to_string(), MirRuntimeValue::Int(e.generation)),
            ("authority".to_string(), MirRuntimeValue::String(e.authority.clone())),
        ],
    }
}

fn runtime_to_service_string(
    value: &MirRuntimeValue,
    max_len: usize,
    label: &str,
    span: Span,
) -> Result<String, Diagnostic> {
    let MirRuntimeValue::String(value) = value else {
        return Err(unsupported(label, span));
    };
    if value.len() > max_len || value.chars().any(char::is_control) {
        return Err(unsupported(label, span));
    }
    Ok(value.clone())
}

fn runtime_to_endpoint(v: &MirRuntimeValue, span: Span) -> Result<JetServiceEndpoint, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = v else {
        return Err(unsupported("ServiceEndpoint", span));
    };
    if type_name != "ServiceEndpoint" {
        return Err(unsupported("ServiceEndpoint", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| unsupported("ServiceEndpoint field", span))
    };
    let tree = match field("tree")? {
        MirRuntimeValue::String(s) => s.clone(),
        _ => return Err(unsupported("endpoint tree", span)),
    };
    let worker = match field("worker")? {
        MirRuntimeValue::String(s) => s.clone(),
        _ => return Err(unsupported("endpoint worker", span)),
    };
    let generation = match field("generation")? {
        MirRuntimeValue::Int(n) => *n,
        _ => return Err(unsupported("endpoint generation", span)),
    };
    let authority = match field("authority")? {
        MirRuntimeValue::String(s) => s.clone(),
        _ => return Err(unsupported("endpoint authority", span)),
    };
    if tree.trim().is_empty()
        || worker.trim().is_empty()
        || tree.chars().any(char::is_control)
        || worker.chars().any(char::is_control)
        || tree.len() > MAX_SERVICE_NAME
        || worker.len() > MAX_SERVICE_NAME
        || generation < 1
        || authority.trim().is_empty()
        || authority.chars().any(char::is_control)
        || authority.len() > MAX_SERVICE_NAME
    {
        return Err(unsupported("ServiceEndpoint value", span));
    }
    jet_services_authority_endpoint(tree, worker, generation, authority)
        .map_err(|_| unsupported("ServiceEndpoint value", span))
}
/// Decode a checked service endpoint for a host adapter that must execute a
/// resident callback outside the runtime lock.
pub fn service_endpoint_runtime(value: &MirRuntimeValue) -> Option<JetServiceEndpoint> {
    runtime_to_endpoint(value, Span::new(0, 0)).ok()
}

fn mailbox_to_runtime(m: &JetServiceMailbox) -> MirRuntimeValue {
    // The tree Prelude mutates this mailbox directly, including rollback after
    // a failed receive. Keep that post-call local snapshot here. `runtime_to_mailbox`
    // rehydrates from the authority channel before the next call when an
    // endpoint operation changed the queue outside this tree value.
    let messages = m.channel.snapshot();
    MirRuntimeValue::Struct {
        type_name: "ServiceMailbox".to_string(),
        fields: vec![
            ("endpoint".to_string(), endpoint_to_runtime(&m.endpoint)),
            ("capacity".to_string(), MirRuntimeValue::Int(m.capacity)),
            ("depth".to_string(), MirRuntimeValue::Int(messages.len() as i64)),
            (
                "messages".to_string(),
                MirRuntimeValue::List(messages.into_iter().map(MirRuntimeValue::String).collect()),
            ),
        ],
    }
}

fn runtime_to_mailbox(v: &MirRuntimeValue, span: Span) -> Result<JetServiceMailbox, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = v else {
        return Err(unsupported("ServiceMailbox", span));
    };
    if type_name != "ServiceMailbox" {
        return Err(unsupported("ServiceMailbox", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| unsupported("mailbox field", span))
    };
    let capacity = match field("capacity")? {
        MirRuntimeValue::Int(n) => *n,
        _ => return Err(unsupported("mailbox capacity", span)),
    };
    let messages = match field("messages")? {
        MirRuntimeValue::List(xs) => {
            if xs.len() > MAX_SERVICE_MESSAGES || capacity <= 0 || xs.len() > capacity as usize {
                return Err(unsupported("mailbox message limit", span));
            }
            xs.iter()
                .map(|x| runtime_to_service_string(x, MAX_SERVICE_MESSAGE, "mailbox message", span))
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => return Err(unsupported("mailbox messages", span)),
    };
    let depth = match field("depth")? {
        MirRuntimeValue::Int(n) => *n,
        _ => return Err(unsupported("mailbox depth", span)),
    };
    let endpoint = runtime_to_endpoint(field("endpoint")?, span)?;
    // An endpoint method may enqueue through the authority between two
    // interpreter calls while the tree value itself is not the receiver.
    // Rehydrate from that one authority channel when it exists, or from the
    // serialized mailbox after a real process restart.
    let authoritative_messages = service_authority_channel(&endpoint)
        .ok()
        .map(|(_, _, channel)| channel.snapshot());
    let has_authority_channel = authoritative_messages.is_some();
    let messages = authoritative_messages.unwrap_or(messages);
    let mailbox = jet_services_new_mailbox(endpoint, capacity, messages)
        .map_err(|_| unsupported("ServiceMailbox channel", span))?;
    if depth < 0 || (!has_authority_channel && mailbox.channel.depth() as i64 != depth) {
        return Err(unsupported("mailbox depth", span));
    }
    Ok(mailbox)
}

fn worker_to_runtime(w: &JetServiceWorker) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "ServiceWorker".to_string(),
        fields: vec![
            ("name".to_string(), MirRuntimeValue::String(w.name.clone())),
            (
                "handler".to_string(),
                MirRuntimeValue::String(w.handler_name.clone()),
            ),
            ("endpoint".to_string(), endpoint_to_runtime(&w.endpoint)),
            ("mailbox".to_string(), mailbox_to_runtime(&w.mailbox)),
            (
                "restarts".to_string(),
                MirRuntimeValue::List(w.restarts.iter().copied().map(MirRuntimeValue::Int).collect()),
            ),
            ("running".to_string(), MirRuntimeValue::Bool(w.running)),
        ],
    }
}

fn runtime_to_worker(v: &MirRuntimeValue, span: Span) -> Result<JetServiceWorker, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = v else {
        return Err(unsupported("ServiceWorker", span));
    };
    if type_name != "ServiceWorker" {
        return Err(unsupported("ServiceWorker", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| unsupported("worker field", span))
    };
    let running = match field("running")? {
        MirRuntimeValue::Bool(b) => *b,
        _ => return Err(unsupported("worker running state", span)),
    };
    let name = runtime_to_service_string(field("name")?, MAX_SERVICE_NAME, "worker name", span)?;
    let handler_name = match field("handler") {
        Ok(value) => runtime_to_service_string(value, MAX_SERVICE_NAME, "worker handler", span)?,
        // Existing internal Core values predate handler metadata. Preserve
        // their real worker identity while the typed surface migrates; this is
        // not a second user-facing declaration form.
        Err(_) => name.clone(),
    };
    let declared_endpoint = runtime_to_endpoint(field("endpoint")?, span)?;
    let mailbox = runtime_to_mailbox(field("mailbox")?, span)?;
    if declared_endpoint != mailbox.endpoint {
        return Err(unsupported("worker endpoint/mailbox mismatch", span));
    }
    let endpoint = mailbox.endpoint.clone();
    Ok(JetServiceWorker {
        name,
        handler_name,
        handler: jet_services_noop_worker,
        endpoint,
        mailbox,
        restarts: match field("restarts")? {
            MirRuntimeValue::List(xs) => {
                if xs.len() as i64 > MAX_SERVICE_RESTART_BUDGET {
                    return Err(unsupported("worker restart budget", span));
                }
                xs.iter()
                    .map(|x| match x {
                        MirRuntimeValue::Int(at) if *at >= 0 => Ok(*at),
                        _ => Err(unsupported("worker restart instant", span)),
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
            _ => return Err(unsupported("worker restarts", span)),
        },
        running,
        received_bytes: 0,
        sent_bytes: 0,
        task: std::sync::Arc::new(std::sync::Mutex::new(JetServiceSupervisorState::new(
            if running {
                JetServiceSupervisorStatus::Running
            } else {
                JetServiceSupervisorStatus::Stopped
            },
        ))),
    })
}

fn state_adapter_to_runtime(a: JetServiceStateAdapter) -> MirRuntimeValue {
    MirRuntimeValue::Enum {
        type_name: "ServiceStateAdapter".to_string(),
        variant: match a {
            JetServiceStateAdapter::Empty => "Empty".to_string(),
            JetServiceStateAdapter::Snapshot => "Snapshot".to_string(),
            JetServiceStateAdapter::EventLog => "EventLog".to_string(),
        },
        args: Vec::new(),
    }
}

fn runtime_to_state_adapter(v: &MirRuntimeValue, span: Span) -> Result<JetServiceStateAdapter, Diagnostic> {
    match v {
        MirRuntimeValue::Enum {
            type_name, variant, ..
        } if type_name == "ServiceStateAdapter" => match variant.as_str() {
            "Empty" => Ok(JetServiceStateAdapter::Empty),
            "Snapshot" => Ok(JetServiceStateAdapter::Snapshot),
            "EventLog" => Ok(JetServiceStateAdapter::EventLog),
            _ => Err(unsupported("ServiceStateAdapter", span)),
        },
        _ => Err(unsupported("ServiceStateAdapter", span)),
    }
}

fn state_store_to_runtime(store: &JetServiceStateStore) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "ServiceStateStore".to_string(),
        fields: vec![("path".to_string(), MirRuntimeValue::String(store.path.clone()))],
    }
}

fn runtime_to_state_store(value: &MirRuntimeValue, span: Span) -> Result<JetServiceStateStore, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(unsupported("ServiceStateStore", span));
    };
    if type_name != "ServiceStateStore" {
        return Err(unsupported("ServiceStateStore", span));
    }
    let path = fields
        .iter()
        .find(|(name, _)| name == "path")
        .map(|(_, value)| value)
        .ok_or_else(|| unsupported("ServiceStateStore path", span))?;
    let path = runtime_to_service_string(path, MAX_SERVICE_STATE_STORE, "service state store", span)?;
    jet_services_state_store(path).map_err(|error| unsupported(&error.jet_show(), span))
}

fn state_authority_to_runtime(authority: &JetServiceStateAuthority) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "ServiceStateAuthority".to_string(),
        fields: vec![
            ("store".to_string(), MirRuntimeValue::String(authority.store.clone())),
            ("schema".to_string(), MirRuntimeValue::String(authority.schema.clone())),
            ("version".to_string(), MirRuntimeValue::Int(authority.version)),
            (
                "migration".to_string(),
                MirRuntimeValue::String(authority.migration.clone()),
            ),
            (
                "adapter".to_string(),
                state_adapter_to_runtime(authority.adapter.clone()),
            ),
        ],
    }
}

fn runtime_to_state_authority(
    value: &MirRuntimeValue,
    span: Span,
) -> Result<JetServiceStateAuthority, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(unsupported("ServiceStateAuthority", span));
    };
    if type_name != "ServiceStateAuthority" {
        return Err(unsupported("ServiceStateAuthority", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, value)| value)
            .ok_or_else(|| unsupported("ServiceStateAuthority field", span))
    };
    let candidate = JetServiceStateAuthority {
        store: runtime_to_service_string(
            field("store")?,
            MAX_SERVICE_STATE_STORE,
            "service state store",
            span,
        )?,
        schema: runtime_to_service_string(
            field("schema")?,
            MAX_SERVICE_STATE_SCHEMA,
            "service state schema",
            span,
        )?,
        version: match field("version")? {
            MirRuntimeValue::Int(version) => *version,
            _ => return Err(unsupported("service state version", span)),
        },
        migration: runtime_to_service_string(
            field("migration")?,
            MAX_SERVICE_STATE_SCHEMA,
            "service state migration",
            span,
        )?,
        adapter: runtime_to_state_adapter(field("adapter")?, span)?,
    };
    jet_services_attach_state_authority(&candidate, candidate.adapter.clone())
        .map_err(|error| unsupported(&error.jet_show(), span))
}

fn workflow_to_runtime(w: &JetServiceWorkflow) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "ServiceWorkflow".to_string(),
        fields: vec![
            ("id".to_string(), MirRuntimeValue::String(w.id.clone())),
            ("run_id".to_string(), MirRuntimeValue::Int(w.run_id)),
            ("version".to_string(), MirRuntimeValue::Int(w.version)),
            (
                "steps".to_string(),
                MirRuntimeValue::List(w.steps.iter().cloned().map(MirRuntimeValue::String).collect()),
            ),
            (
                "history".to_string(),
                MirRuntimeValue::List(w.history.iter().cloned().map(MirRuntimeValue::String).collect()),
            ),
            (
                "replay_cursor".to_string(),
                MirRuntimeValue::Int(w.replay_cursor as i64),
            ),
            ("status".to_string(), task_status_to_runtime(&w.status)),
            (
                "activity_outcomes".to_string(),
                MirRuntimeValue::List(
                    w.activity_outcomes
                        .iter()
                        .map(|(key, outcome)| MirRuntimeValue::Struct {
                            type_name: "ServiceActivityOutcome".to_string(),
                            fields: vec![
                                ("key".to_string(), MirRuntimeValue::String(key.clone())),
                                ("outcome".to_string(), task_outcome_to_runtime(outcome)),
                            ],
                        })
                        .collect(),
                ),
            ),
            (
                "outcome".to_string(),
                w.outcome.as_ref().map_or_else(
                    || mir_absent("TaskOutcome"),
                    |outcome| MirRuntimeValue::Present(Box::new(task_outcome_to_runtime(outcome))),
                ),
            ),
        ],
    }
}

fn workflow_handle_to_runtime(handle: &JetWorkflowHandle, span: Span) -> Result<MirRuntimeValue, Diagnostic> {
    let workflow = handle
        .state
        .lock()
        .map_err(|_| unsupported("workflow handle state", span))?
        .clone();
    let MirRuntimeValue::Struct {
        type_name,
        mut fields,
    } = workflow_to_runtime(&workflow)
    else {
        return Err(unsupported("workflow handle state", span));
    };
    fields.push((
        "authority".to_string(),
        state_authority_to_runtime(&handle.authority),
    ));
    Ok(MirRuntimeValue::Struct { type_name, fields })
}

fn runtime_to_workflow_handle(value: &MirRuntimeValue, span: Span) -> Result<JetWorkflowHandle, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(unsupported("ServiceWorkflow", span));
    };
    if type_name != "ServiceWorkflow" {
        return Err(unsupported("ServiceWorkflow", span));
    }
    let authority = fields
        .iter()
        .find(|(name, _)| name == "authority")
        .map(|(_, value)| value)
        .ok_or_else(|| unsupported("workflow handle authority", span))
        .and_then(|value| runtime_to_state_authority(value, span))?;
    let workflow = runtime_to_workflow(
        &MirRuntimeValue::Struct {
            type_name: type_name.clone(),
            fields: fields
                .iter()
                .filter(|(name, _)| name != "authority")
                .cloned()
                .collect(),
        },
        span,
    )?;
    Ok(JetWorkflowHandle {
        authority,
        state: std::sync::Arc::new(std::sync::Mutex::new(workflow)),
    })
}

fn runtime_to_workflow_run_id(value: &MirRuntimeValue, span: Span) -> Result<i64, Diagnostic> {
    match value {
        MirRuntimeValue::Int(run_id) => Ok(*run_id),
        MirRuntimeValue::Struct { type_name, fields } if type_name == "ServiceWorkflow" => fields
            .iter()
            .find(|(name, _)| name == "run_id")
            .and_then(|(_, value)| match value {
                MirRuntimeValue::Int(run_id) => Some(*run_id),
                _ => None,
            })
            .ok_or_else(|| unsupported("workflow run id", span)),
        _ => Err(unsupported("workflow run id", span)),
    }
}

fn runtime_to_workflow(v: &MirRuntimeValue, span: Span) -> Result<JetServiceWorkflow, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = v else {
        return Err(unsupported("ServiceWorkflow", span));
    };
    if type_name != "ServiceWorkflow" {
        return Err(unsupported("ServiceWorkflow", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| unsupported("workflow field", span))
    };
    let str_list = |name: &str| -> Result<Vec<String>, Diagnostic> {
        match field(name)? {
            MirRuntimeValue::List(xs) => {
                if xs.len() > MAX_SERVICE_WORKFLOW_STEPS {
                    return Err(unsupported("workflow string limit", span));
                }
                xs.iter()
                    .map(|x| runtime_to_service_string(x, MAX_SERVICE_MESSAGE, "workflow string", span))
                    .collect()
            }
            _ => Err(unsupported("workflow string list", span)),
        }
    };
    Ok(JetServiceWorkflow {
        id: runtime_to_service_string(field("id")?, MAX_SERVICE_NAME, "workflow id", span)?,
        run_id: match field("run_id")? {
            MirRuntimeValue::Int(n) => *n,
            _ => return Err(unsupported("workflow run id", span)),
        },
        version: match field("version")? {
            MirRuntimeValue::Int(n) => *n,
            _ => return Err(unsupported("workflow version", span)),
        },
        steps: str_list("steps")?,
        history: str_list("history")?,
        replay_cursor: match field("replay_cursor")? {
            MirRuntimeValue::Int(n) if *n >= 0 => {
                usize::try_from(*n).map_err(|_| unsupported("workflow replay cursor", span))?
            }
            _ => return Err(unsupported("workflow replay cursor", span)),
        },
        status: runtime_to_task_status(field("status")?, span)?,
        activity_outcomes: match field("activity_outcomes")? {
            MirRuntimeValue::List(entries) => entries
                .iter()
                .map(|entry| {
                    let MirRuntimeValue::Struct { type_name, fields } = entry else {
                        return Err(unsupported("ServiceActivityOutcome", span));
                    };
                    if type_name != "ServiceActivityOutcome" {
                        return Err(unsupported("ServiceActivityOutcome", span));
                    }
                    let key = fields
                        .iter()
                        .find(|(name, _)| name == "key")
                        .map(|(_, value)| value)
                        .ok_or_else(|| unsupported("activity outcome key", span))
                        .and_then(|value| {
                            runtime_to_service_string(
                                value,
                                MAX_SERVICE_NAME,
                                "activity outcome key",
                                span,
                            )
                        })?;
                    let outcome = fields
                        .iter()
                        .find(|(name, _)| name == "outcome")
                        .map(|(_, value)| value)
                        .ok_or_else(|| unsupported("activity outcome value", span))
                        .and_then(|value| runtime_to_task_outcome(value, span))?;
                    Ok((key, outcome))
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err(unsupported("activity outcomes", span)),
        },
        outcome: match field("outcome")? {
            MirRuntimeValue::Absent { .. } => None,
            MirRuntimeValue::Present(value) => Some(runtime_to_task_outcome(value, span)?),
            _ => return Err(unsupported("workflow outcome", span)),
        },
    })
}

fn upgrade_receipt_to_runtime(receipt: &JetServiceUpgradeReceipt) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "ServiceUpgradeReceipt".to_string(),
        fields: vec![
            (
                "from_generation".to_string(),
                MirRuntimeValue::Int(receipt.from_generation),
            ),
            (
                "to_generation".to_string(),
                MirRuntimeValue::Int(receipt.to_generation),
            ),
            (
                "migration".to_string(),
                MirRuntimeValue::String(receipt.migration.clone()),
            ),
            (
                "rollback_store".to_string(),
                MirRuntimeValue::String(receipt.rollback_store.clone()),
            ),
            (
                "rollback_available".to_string(),
                MirRuntimeValue::Bool(receipt.rollback_available),
            ),
            (
                "pinned_shards".to_string(),
                MirRuntimeValue::List(
                    receipt
                        .pinned_shards
                        .iter()
                        .cloned()
                        .map(MirRuntimeValue::String)
                        .collect(),
                ),
            ),
        ],
    }
}

fn runtime_to_upgrade_receipt(v: &MirRuntimeValue, span: Span) -> Result<JetServiceUpgradeReceipt, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = v else {
        return Err(unsupported("ServiceUpgradeReceipt", span));
    };
    if type_name != "ServiceUpgradeReceipt" {
        return Err(unsupported("ServiceUpgradeReceipt", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| unsupported("ServiceUpgradeReceipt field", span))
    };
    Ok(JetServiceUpgradeReceipt {
        from_generation: match field("from_generation")? {
            MirRuntimeValue::Int(n) => *n,
            _ => return Err(unsupported("upgrade from generation", span)),
        },
        to_generation: match field("to_generation")? {
            MirRuntimeValue::Int(n) => *n,
            _ => return Err(unsupported("upgrade to generation", span)),
        },
        migration: runtime_to_service_string(field("migration")?, 32, "upgrade migration", span)?,
        rollback_store: runtime_to_service_string(
            field("rollback_store")?,
            MAX_SERVICE_STATE_STORE,
            "upgrade rollback store",
            span,
        )?,
        rollback_available: match field("rollback_available")? {
            MirRuntimeValue::Bool(v) => *v,
            _ => return Err(unsupported("upgrade rollback availability", span)),
        },
        pinned_shards: runtime_str_list(field("pinned_shards")?, span)?,
    })
}

fn tree_to_runtime(tree: &JetServiceTree) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "ServiceTree".to_string(),
        fields: vec![
            ("name".to_string(), MirRuntimeValue::String(tree.name.clone())),
            (
                "authority".to_string(),
                MirRuntimeValue::String(tree.authority.clone()),
            ),
            ("generation".to_string(), MirRuntimeValue::Int(tree.generation)),
            (
                "delivery".to_string(),
                delivery_to_runtime(tree.delivery.clone()),
            ),
            ("restart".to_string(), restart_to_runtime(tree.restart.clone())),
            (
                "restart_budget".to_string(),
                restart_budget_to_runtime(&tree.restart_budget),
            ),
            (
                "workers".to_string(),
                MirRuntimeValue::List(tree.workers.iter().map(worker_to_runtime).collect()),
            ),
            (
                "groups".to_string(),
                MirRuntimeValue::List(
                    tree.groups
                        .iter()
                        .map(|g| MirRuntimeValue::Struct {
                            type_name: "ServiceGroup".to_string(),
                            fields: vec![
                                ("name".to_string(), MirRuntimeValue::String(g.name.clone())),
                                ("restart".to_string(), restart_to_runtime(g.restart.clone())),
                                ("budget".to_string(), restart_budget_to_runtime(&g.budget)),
                                (
                                    "workers".to_string(),
                                    MirRuntimeValue::List(
                                        g.workers.iter().cloned().map(MirRuntimeValue::String).collect(),
                                    ),
                                ),
                            ],
                        })
                        .collect(),
                ),
            ),
            ("started".to_string(), MirRuntimeValue::Bool(tree.started)),
            (
                "state_adapter".to_string(),
                state_adapter_to_runtime(tree.state_adapter.clone()),
            ),
            (
                "state_authority".to_string(),
                tree.state_authority.as_ref().map_or_else(
                    || mir_absent("ServiceStateAuthority"),
                    |authority| MirRuntimeValue::Present(Box::new(state_authority_to_runtime(authority))),
                ),
            ),
            (
                "snapshot".to_string(),
                match &tree.snapshot {
                    Some(s) => MirRuntimeValue::String(s.clone()),
                    None => MirRuntimeValue::String(String::new()),
                },
            ),
            (
                "event_log".to_string(),
                MirRuntimeValue::List(tree.event_log.iter().cloned().map(MirRuntimeValue::String).collect()),
            ),
            (
                "dead_letters".to_string(),
                MirRuntimeValue::List(
                    tree.dead_letters
                        .iter()
                        .cloned()
                        .map(MirRuntimeValue::String)
                        .collect(),
                ),
            ),
            (
                "idempotency_seen".to_string(),
                MirRuntimeValue::List(
                    tree.idempotency_seen
                        .iter()
                        .map(|(key, endpoint, message)| MirRuntimeValue::Struct {
                            type_name: "ServiceIdempotencyEntry".to_string(),
                            fields: vec![
                                ("key".to_string(), MirRuntimeValue::String(key.clone())),
                                ("endpoint".to_string(), endpoint_to_runtime(endpoint)),
                                ("message".to_string(), MirRuntimeValue::String(message.clone())),
                            ],
                        })
                        .collect(),
                ),
            ),
            (
                "directory".to_string(),
                MirRuntimeValue::List(
                    tree.directory
                        .iter()
                        .map(|(n, ep, signature)| MirRuntimeValue::Struct {
                            type_name: "ServiceDirectoryEntry".to_string(),
                            fields: vec![
                                ("name".to_string(), MirRuntimeValue::String(n.clone())),
                                ("endpoint".to_string(), endpoint_to_runtime(ep)),
                                ("signature".to_string(), MirRuntimeValue::String(signature.clone())),
                            ],
                        })
                        .collect(),
                ),
            ),
            (
                "draining".to_string(),
                MirRuntimeValue::List(tree.draining.iter().cloned().map(MirRuntimeValue::String).collect()),
            ),
            (
                "partitioned".to_string(),
                MirRuntimeValue::List(tree.partitioned.iter().cloned().map(MirRuntimeValue::String).collect()),
            ),
            (
                "workflows".to_string(),
                MirRuntimeValue::List(tree.workflows.iter().map(workflow_to_runtime).collect()),
            ),
            ("chaos_fails".to_string(), MirRuntimeValue::Int(tree.chaos_fails)),
            (
                "previous_generation".to_string(),
                MirRuntimeValue::Int(tree.previous_generation),
            ),
            (
                "last_upgrade".to_string(),
                tree.last_upgrade.as_ref().map_or_else(
                    || mir_absent("ServiceUpgradeReceipt"),
                    |receipt| MirRuntimeValue::Present(Box::new(upgrade_receipt_to_runtime(receipt))),
                ),
            ),
        ],
    }
}

fn runtime_str_list(v: &MirRuntimeValue, span: Span) -> Result<Vec<String>, Diagnostic> {
    match v {
        MirRuntimeValue::List(xs) => {
            if xs.len() > MAX_SERVICE_STATE_RECORDS {
                return Err(unsupported("string list length", span));
            }
            xs.iter()
                .map(|x| match x {
                    MirRuntimeValue::String(s)
                        if s.len() <= MAX_SERVICE_MESSAGE && !s.chars().any(char::is_control) =>
                    {
                        Ok(s.clone())
                    }
                    _ => Err(unsupported("string list", span)),
                })
                .collect()
        }
        _ => Err(unsupported("string list", span)),
    }
}

fn runtime_to_tree(v: &MirRuntimeValue, span: Span) -> Result<JetServiceTree, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = v else {
        return Err(unsupported("ServiceTree", span));
    };
    if type_name != "ServiceTree" {
        return Err(unsupported("ServiceTree", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| unsupported("tree field", span))
    };
    let workers = match field("workers")? {
        MirRuntimeValue::List(xs) => {
            if xs.len() > MAX_SERVICE_WORKERS {
                return Err(unsupported("tree worker limit", span));
            }
            xs.iter()
                .map(|x| runtime_to_worker(x, span))
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => return Err(unsupported("tree workers", span)),
    };
    let groups = match field("groups")? {
        MirRuntimeValue::List(xs) => {
            if xs.len() > MAX_SERVICE_WORKERS {
                return Err(unsupported("tree group limit", span));
            }
            xs.iter()
                .map(|x| {
                    let MirRuntimeValue::Struct { type_name, fields } = x else {
                        return Err(unsupported("ServiceGroup", span));
                    };
                    if type_name != "ServiceGroup" {
                        return Err(unsupported("ServiceGroup", span));
                    }
                    let name = fields
                        .iter()
                        .find(|(n, _)| n == "name")
                        .map(|(_, value)| value)
                        .ok_or_else(|| unsupported("group name", span))
                        .and_then(|value| {
                            runtime_to_service_string(value, MAX_SERVICE_NAME, "group name", span)
                        })?;
                    let restart = fields
                        .iter()
                        .find(|(n, _)| n == "restart")
                        .map(|(_, v)| runtime_to_restart(v, span))
                        .ok_or_else(|| unsupported("group restart", span))??;
                    let budget = fields
                        .iter()
                        .find(|(n, _)| n == "budget")
                        .map(|(_, v)| runtime_to_restart_budget(v, span))
                        .ok_or_else(|| unsupported("group restart budget", span))??;
                    let workers = match fields.iter().find(|(n, _)| n == "workers") {
                        Some((_, MirRuntimeValue::List(xs))) => {
                            if xs.len() > MAX_SERVICE_WORKERS {
                                return Err(unsupported("group worker limit", span));
                            }
                            xs.iter()
                                .map(|x| {
                                    runtime_to_service_string(x, MAX_SERVICE_NAME, "group worker", span)
                                })
                                .collect::<Result<Vec<_>, _>>()?
                        }
                        Some(_) => return Err(unsupported("group workers", span)),
                        None => return Err(unsupported("group workers", span)),
                    };
                    Ok(JetServiceGroup {
                        name,
                        restart,
                        budget,
                        workers,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => return Err(unsupported("tree groups", span)),
    };
    let directory = match field("directory")? {
        MirRuntimeValue::List(xs) => {
            if xs.len() > MAX_SERVICE_WORKERS {
                return Err(unsupported("service directory limit", span));
            }
            xs.iter()
                .map(|x| {
                    let MirRuntimeValue::Struct { type_name, fields } = x else {
                        return Err(unsupported("directory entry", span));
                    };
                    if type_name != "ServiceDirectoryEntry" {
                        return Err(unsupported("directory entry", span));
                    }
                    let name = fields
                        .iter()
                        .find(|(n, _)| n == "name")
                        .map(|(_, value)| value)
                        .ok_or_else(|| unsupported("directory name", span))
                        .and_then(|value| {
                            runtime_to_service_string(value, MAX_SERVICE_NAME, "directory name", span)
                        })?;
                    let endpoint = fields
                        .iter()
                        .find(|(n, _)| n == "endpoint")
                        .ok_or_else(|| unsupported("directory endpoint", span))?;
                    let signature = fields
                        .iter()
                        .find(|(n, _)| n == "signature")
                        .map(|(_, value)| value)
                        .ok_or_else(|| unsupported("directory signature", span))
                        .and_then(|value| {
                            runtime_to_service_string(value, 64, "directory signature", span)
                        })?;
                    Ok((name, runtime_to_endpoint(&endpoint.1, span)?, signature))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => return Err(unsupported("service directory", span)),
    };
    let workflows = match field("workflows")? {
        MirRuntimeValue::List(xs) => {
            if xs.len() > MAX_SERVICE_WORKFLOW_STEPS {
                return Err(unsupported("service workflow limit", span));
            }
            xs.iter()
                .map(|x| runtime_to_workflow(x, span))
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => return Err(unsupported("service workflows", span)),
    };
    let snapshot = match field("snapshot")? {
        value => {
            let value = runtime_to_service_string(value, MAX_SERVICE_MESSAGE, "service snapshot", span)?;
            (!value.is_empty()).then_some(value)
        }
    };
    let state_authority = match fields.iter().find(|(name, _)| name == "state_authority") {
        None | Some((_, MirRuntimeValue::Absent { .. })) => None,
        Some((_, MirRuntimeValue::Present(value))) => Some(runtime_to_state_authority(value, span)?),
        Some(_) => return Err(unsupported("service state authority", span)),
    };
    let last_upgrade = match fields.iter().find(|(name, _)| name == "last_upgrade") {
        None | Some((_, MirRuntimeValue::Absent { .. })) => None,
        Some((_, MirRuntimeValue::Present(value))) => Some(runtime_to_upgrade_receipt(value, span)?),
        Some(_) => return Err(unsupported("service upgrade receipt", span)),
    };
    let partitioned = match fields.iter().find(|(name, _)| name == "partitioned") {
        Some((_, value)) => runtime_str_list(value, span)?,
        None => Vec::new(),
    };
    let idempotency_seen = match field("idempotency_seen")? {
        MirRuntimeValue::List(entries) => {
            if entries.len() > MAX_SERVICE_IDEMPOTENCY {
                return Err(unsupported("service idempotency limit", span));
            }
            entries
                .iter()
                .map(|entry| {
                    let MirRuntimeValue::Struct { type_name, fields } = entry else {
                        return Err(unsupported("idempotency entry", span));
                    };
                    if type_name != "ServiceIdempotencyEntry" {
                        return Err(unsupported("idempotency entry", span));
                    }
                    let field = |name: &str| {
                        fields
                            .iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, value)| value)
                            .ok_or_else(|| unsupported("idempotency field", span))
                    };
                    let key = runtime_to_service_string(
                        field("key")?,
                        MAX_SERVICE_NAME,
                        "idempotency key",
                        span,
                    )?;
                    let endpoint = runtime_to_endpoint(field("endpoint")?, span)?;
                    let message = runtime_to_service_string(
                        field("message")?,
                        MAX_SERVICE_MESSAGE,
                        "idempotency message",
                        span,
                    )?;
                    Ok((key, endpoint, message))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => return Err(unsupported("service idempotency state", span)),
    };
    let mut tree = JetServiceTree {
        name: runtime_to_service_string(field("name")?, MAX_SERVICE_NAME, "tree name", span)?,
        authority: runtime_to_service_string(
            field("authority")?,
            MAX_SERVICE_NAME,
            "tree authority",
            span,
        )?,
        generation: match field("generation")? {
            MirRuntimeValue::Int(n) => *n,
            _ => return Err(unsupported("tree generation", span)),
        },
        delivery: runtime_to_delivery(field("delivery")?, span)?,
        restart: runtime_to_restart(field("restart")?, span)?,
        restart_budget: runtime_to_restart_budget(field("restart_budget")?, span)?,
        workers,
        groups,
        started: match field("started")? {
            MirRuntimeValue::Bool(b) => *b,
            _ => return Err(unsupported("tree started state", span)),
        },
        state_adapter: runtime_to_state_adapter(field("state_adapter")?, span)?,
        state_authority,
        snapshot,
        event_log: runtime_str_list(field("event_log")?, span)?,
        dead_letters: runtime_str_list(field("dead_letters")?, span)?,
        idempotency_seen,
        directory,
        draining: runtime_str_list(field("draining")?, span)?,
        partitioned,
        workflows,
        task_group: std::sync::Arc::new(JetTaskGroupRuntime::new()),
        supervisor_tasks: Vec::new(),
        chaos_fails: match field("chaos_fails")? {
            MirRuntimeValue::Int(n) => *n,
            _ => return Err(unsupported("service chaos counter", span)),
        },
        previous_generation: match field("previous_generation")? {
            MirRuntimeValue::Int(n) => *n,
            _ => return Err(unsupported("previous service generation", span)),
        },
        last_upgrade,
    };
    jet_services_validate_tree(&tree).map_err(|error| unsupported(&error.jet_show(), span))?;
    for worker in &tree.workers {
        let draining = tree.draining.iter().any(|name| name == &worker.name);
        let partitioned = tree.partitioned.iter().any(|name| name == &worker.name);
        let started = tree.started && !partitioned && (worker.running || draining);
        jet_services_authority_hydrate(&worker.endpoint, started)
            .map_err(|error| unsupported(&error.jet_show(), span))?;
        if partitioned {
            jet_services_authority_update_partitioned(&worker.endpoint, true)
                .map_err(|error| unsupported(&error.jet_show(), span))?;
        }
        if tree.started {
            jet_services_authority_update_draining(&worker.endpoint, draining)
                .map_err(|error| unsupported(&error.jet_show(), span))?;
        }
        if started {
            jet_services_bind_delivery_endpoint(
                &tree.delivery,
                tree.state_authority.as_ref(),
                &worker.endpoint,
            )
            .map_err(|error| unsupported(&error.jet_show(), span))?;
        }
    }
    if tree.started {
        jet_services_build_runtime_groups(&mut tree)
            .map_err(|error| unsupported(&error.jet_show(), span))?;
    }
    Ok(tree)
}

const JOB_QUEUE_DEFAULT_NAME: &str = "default";

fn queue_result_value<T>(
    result: Result<T, JetServiceError>,
    encode: impl FnOnce(T) -> MirRuntimeValue,
) -> MirRuntimeValue {
    match result {
        Ok(value) => MirRuntimeValue::Present(Box::new(encode(value))),
        Err(error) => MirRuntimeValue::FailedTold(Box::new(map_err(error))),
    }
}

fn queue_field<'a>(
    fields: &'a [(String, MirRuntimeValue)],
    type_name: &str,
    name: &str,
    span: Span,
) -> Result<&'a MirRuntimeValue, Diagnostic> {
    fields
        .iter()
        .find_map(|(field, value)| (field == name).then_some(value))
        .ok_or_else(|| unsupported(&format!("{type_name}.{name}"), span))
}

fn queue_text(
    value: &MirRuntimeValue,
    type_name: &str,
    name: &str,
    span: Span,
) -> Result<String, Diagnostic> {
    runtime_to_service_string(
        value,
        JET_JOB_QUEUE_MAX_NAME,
        &format!("{type_name}.{name}"),
        span,
    )
}

fn queue_int(
    value: &MirRuntimeValue,
    type_name: &str,
    name: &str,
    span: Span,
) -> Result<i64, Diagnostic> {
    match value {
        MirRuntimeValue::Int(value) => Ok(*value),
        _ => Err(unsupported(&format!("{type_name}.{name}"), span)),
    }
}

fn queue_bool(
    value: &MirRuntimeValue,
    type_name: &str,
    name: &str,
    span: Span,
) -> Result<bool, Diagnostic> {
    match value {
        MirRuntimeValue::Bool(value) => Ok(*value),
        _ => Err(unsupported(&format!("{type_name}.{name}"), span)),
    }
}

fn queue_optional_text(
    args: &[MirRuntimeValue],
    index: usize,
    type_name: &str,
    name: &str,
    span: Span,
) -> Result<Option<String>, Diagnostic> {
    let Some(value) = args.get(index) else {
        return Ok(None);
    };
    match value {
        MirRuntimeValue::Absent { .. } => Ok(None),
        MirRuntimeValue::Present(value) => queue_optional_text(
            &[value.as_ref().clone()],
            0,
            type_name,
            name,
            span,
        ),
        _ => queue_text(value, type_name, name, span).map(Some),
    }
}

fn queue_job_type(value: &MirRuntimeValue, span: Span) -> Result<String, Diagnostic> {
    let type_name = match value {
        MirRuntimeValue::String(value) => value.clone(),
        MirRuntimeValue::Struct { type_name, .. } | MirRuntimeValue::Enum { type_name, .. } => {
            type_name.clone()
        }
        _ => return Err(unsupported("Job identity", span)),
    };
    runtime_to_service_string(
        &MirRuntimeValue::String(type_name),
        JET_JOB_QUEUE_MAX_TYPE,
        "Job identity",
        span,
    )
}

fn queue_payload(
    job_type: &str,
    value: &MirRuntimeValue,
    span: Span,
) -> Result<JetJobPayload, Diagnostic> {
    if let MirRuntimeValue::Struct { type_name, fields } = value {
        if type_name == "JobPayload" {
            let type_id =
                queue_text(queue_field(fields, type_name, "type_id", span)?, type_name, "type_id", span)?;
            let bytes = match queue_field(fields, type_name, "bytes", span)? {
                MirRuntimeValue::Bytes(bytes) => bytes.clone(),
                _ => return Err(unsupported("JobPayload.bytes", span)),
            };
            let publish = queue_bool(
                queue_field(fields, type_name, "publish", span)?,
                type_name,
                "publish",
                span,
            )?;
            let payload = JetJobPayload::new(type_id, bytes, publish)
                .map_err(|_| unsupported("JobPayload", span))?;
            remember_interpreter_job_payload(&payload, value);
            return Ok(payload);
        }
    }
    let bytes = interpreter_job_cbor_bytes(value, span)?;
    let payload = JetJobPayload::new(job_type.to_string(), bytes, false)
        .map_err(|_| unsupported("Job payload", span))?;
    remember_interpreter_job_payload(&payload, value);
    Ok(payload)
}

fn queue_duration_ms(value: &MirRuntimeValue, span: Span) -> Result<i64, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(unsupported("queue delay duration", span));
    };
    if type_name != "Duration" {
        return Err(unsupported("queue delay duration", span));
    }
    let nanos = queue_int(queue_field(fields, type_name, "ns", span)?, type_name, "ns", span)?;
    if nanos < 0 {
        return Err(unsupported("queue delay duration", span));
    }
    Ok(nanos / 1_000_000)
}

fn queue_request(
    job_type: String,
    payload: JetJobPayload,
    args: &[MirRuntimeValue],
    span: Span,
) -> Result<JetJobEnqueue, Diagnostic> {
    let idempotency_key = queue_optional_text(args, 2, "JobQueue.enqueue", "key", span)?;
    Ok(JetJobEnqueue {
        job_type,
        payload,
        idempotency_key,
        request_id: None,
        delay_ms: 0,
    })
}

fn queue_to_runtime(endpoint: &JetServiceEndpoint, name: &str) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "JobQueue".to_string(),
        fields: vec![
            ("endpoint".to_string(), endpoint_to_runtime(endpoint)),
            ("queue".to_string(), MirRuntimeValue::String(name.to_string())),
        ],
    }
}


fn runtime_to_queue(
    value: &MirRuntimeValue,
    span: Span,
) -> Result<(JetServiceEndpoint, String), Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(unsupported("JobQueue", span));
    };
    if type_name != "JobQueue" {
        return Err(unsupported("JobQueue", span));
    }
    let endpoint = runtime_to_endpoint(queue_field(fields, type_name, "endpoint", span)?, span)?;
    let name = queue_text(queue_field(fields, type_name, "queue", span)?, type_name, "queue", span)?;
    Ok((endpoint, name))
}

fn queue_state_to_runtime(state: JetJobQueueState) -> MirRuntimeValue {
    MirRuntimeValue::Enum {
        type_name: "JobQueueState".to_string(),
        variant: match state {
            JetJobQueueState::Queued => "Queued",
            JetJobQueueState::Running => "Running",
            JetJobQueueState::Retrying => "Retrying",
            JetJobQueueState::Completed => "Completed",
            JetJobQueueState::Failed => "Failed",
            JetJobQueueState::DeadLettered => "DeadLettered",
            JetJobQueueState::Cancelled => "Cancelled",
        }
        .to_string(),
        args: Vec::new(),
    }
}

fn runtime_to_queue_state(
    value: &MirRuntimeValue,
    span: Span,
) -> Result<JetJobQueueState, Diagnostic> {
    let MirRuntimeValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(unsupported("JobQueueState", span));
    };
    if type_name != "JobQueueState" || !args.is_empty() {
        return Err(unsupported("JobQueueState", span));
    }
    match variant.as_str() {
        "Queued" => Ok(JetJobQueueState::Queued),
        "Running" => Ok(JetJobQueueState::Running),
        "Retrying" => Ok(JetJobQueueState::Retrying),
        "Completed" => Ok(JetJobQueueState::Completed),
        "Failed" => Ok(JetJobQueueState::Failed),
        "DeadLettered" => Ok(JetJobQueueState::DeadLettered),
        "Cancelled" => Ok(JetJobQueueState::Cancelled),
        _ => Err(unsupported("JobQueueState variant", span)),
    }
}

fn queue_optional_string(value: Option<String>, type_name: &str) -> MirRuntimeValue {
    value
        .map(|value| MirRuntimeValue::Present(Box::new(MirRuntimeValue::String(value))))
        .unwrap_or_else(|| mir_absent(type_name))
}

fn queue_optional_int(value: Option<i64>) -> MirRuntimeValue {
    value
        .map(|value| MirRuntimeValue::Present(Box::new(MirRuntimeValue::Int(value))))
        .unwrap_or_else(|| mir_absent("Int"))
}

fn queue_delivery_to_runtime(delivery: JetJobQueueDeliveryPolicy) -> MirRuntimeValue {
    MirRuntimeValue::Enum {
        type_name: "JobQueueDeliveryPolicy".to_string(),
        variant: match delivery {
            JetJobQueueDeliveryPolicy::AtLeastOnce => "AtLeastOnce",
        }
        .to_string(),
        args: Vec::new(),
    }
}

fn queue_receipt_to_runtime(receipt: JetJobQueueReceipt) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "JobQueueReceipt".to_string(),
        fields: vec![
            ("id".to_string(), MirRuntimeValue::String(receipt.id)),
            ("authority".to_string(), MirRuntimeValue::String(receipt.authority)),
            ("queue".to_string(), MirRuntimeValue::String(receipt.queue)),
            ("job_type".to_string(), MirRuntimeValue::String(receipt.job_type)),
            ("state".to_string(), queue_state_to_runtime(receipt.state)),
            ("sequence".to_string(), MirRuntimeValue::Int(receipt.sequence)),
            ("attempts".to_string(), MirRuntimeValue::Int(receipt.attempts as i64)),
            ("due_at_ms".to_string(), MirRuntimeValue::Int(receipt.due_at_ms)),
            ("accepted_at_ms".to_string(), MirRuntimeValue::Int(receipt.accepted_at_ms)),
            ("request_id".to_string(), queue_optional_string(receipt.request_id, "String")),
            ("idempotency_key".to_string(), queue_optional_string(receipt.idempotency_key, "String")),
            ("lease_until_ms".to_string(), queue_optional_int(receipt.lease_until_ms)),
            ("duration_ms".to_string(), queue_optional_int(receipt.duration_ms)),
            ("error_reason".to_string(), queue_optional_string(receipt.error_reason, "String")),
            ("delivery".to_string(), queue_delivery_to_runtime(receipt.delivery)),
            ("duplicate".to_string(), MirRuntimeValue::Bool(receipt.duplicate)),
            ("signature".to_string(), MirRuntimeValue::String(receipt.signature)),
        ],
    }
}

fn runtime_to_queue_receipt(
    value: &MirRuntimeValue,
    span: Span,
) -> Result<JetJobQueueReceipt, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(unsupported("JobQueueReceipt", span));
    };
    if type_name != "JobQueueReceipt" {
        return Err(unsupported("JobQueueReceipt", span));
    }
    let optional_text = |name: &str| match queue_field(fields, type_name, name, span)? {
        MirRuntimeValue::Absent { .. } => Ok(None),
        MirRuntimeValue::Present(value) => match value.as_ref() {
            MirRuntimeValue::String(value) => Ok(Some(value.clone())),
            _ => Err(unsupported(&format!("{type_name}.{name}"), span)),
        },
        MirRuntimeValue::String(value) => Ok(Some(value.clone())),
        _ => Err(unsupported(&format!("{type_name}.{name}"), span)),
    };
    let optional_int = |name: &str| match queue_field(fields, type_name, name, span)? {
        MirRuntimeValue::Absent { .. } => Ok(None),
        MirRuntimeValue::Present(value) => match value.as_ref() {
            MirRuntimeValue::Int(value) => Ok(Some(*value)),
            _ => Err(unsupported(&format!("{type_name}.{name}"), span)),
        },
        MirRuntimeValue::Int(value) => Ok(Some(*value)),
        _ => Err(unsupported(&format!("{type_name}.{name}"), span)),
    };
    Ok(JetJobQueueReceipt {
        id: queue_text(queue_field(fields, type_name, "id", span)?, type_name, "id", span)?,
        authority: queue_text(queue_field(fields, type_name, "authority", span)?, type_name, "authority", span)?,
        queue: queue_text(queue_field(fields, type_name, "queue", span)?, type_name, "queue", span)?,
        job_type: queue_text(queue_field(fields, type_name, "job_type", span)?, type_name, "job_type", span)?,
        state: runtime_to_queue_state(queue_field(fields, type_name, "state", span)?, span)?,
        sequence: queue_int(queue_field(fields, type_name, "sequence", span)?, type_name, "sequence", span)?,
        attempts: queue_int(queue_field(fields, type_name, "attempts", span)?, type_name, "attempts", span)?
            .try_into()
            .map_err(|_| unsupported("JobQueueReceipt.attempts", span))?,
        due_at_ms: queue_int(queue_field(fields, type_name, "due_at_ms", span)?, type_name, "due_at_ms", span)?,
        accepted_at_ms: queue_int(queue_field(fields, type_name, "accepted_at_ms", span)?, type_name, "accepted_at_ms", span)?,
        request_id: optional_text("request_id")?,
        idempotency_key: optional_text("idempotency_key")?,
        lease_until_ms: optional_int("lease_until_ms")?,
        duration_ms: optional_int("duration_ms")?,
        error_reason: optional_text("error_reason")?,
        delivery: JetJobQueueDeliveryPolicy::AtLeastOnce,
        duplicate: queue_bool(queue_field(fields, type_name, "duplicate", span)?, type_name, "duplicate", span)?,
        signature: queue_text(queue_field(fields, type_name, "signature", span)?, type_name, "signature", span)?,
    })
}

fn queue_payload_to_runtime(payload: JetJobPayload) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "JobPayload".to_string(),
        fields: vec![
            ("type_id".to_string(), MirRuntimeValue::String(payload.type_id)),
            ("bytes".to_string(), MirRuntimeValue::Bytes(payload.bytes)),
            ("publish".to_string(), MirRuntimeValue::Bool(payload.publish)),
        ],
    }
}

fn runtime_to_queue_payload(
    value: &MirRuntimeValue,
    span: Span,
) -> Result<JetJobPayload, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(unsupported("JobPayload", span));
    };
    if type_name != "JobPayload" {
        return Err(unsupported("JobPayload", span));
    }
    let type_id = queue_text(queue_field(fields, type_name, "type_id", span)?, type_name, "type_id", span)?;
    let bytes = match queue_field(fields, type_name, "bytes", span)? {
        MirRuntimeValue::Bytes(bytes) => bytes.clone(),
        _ => return Err(unsupported("JobPayload.bytes", span)),
    };
    let publish = queue_bool(queue_field(fields, type_name, "publish", span)?, type_name, "publish", span)?;
    JetJobPayload::new(type_id, bytes, publish).map_err(|_| unsupported("JobPayload", span))
}

fn queue_result_to_runtime(result: JetJobResult) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "JobResult".to_string(),
        fields: vec![
            ("type_id".to_string(), MirRuntimeValue::String(result.type_id)),
            ("bytes".to_string(), MirRuntimeValue::Bytes(result.bytes)),
            ("publish".to_string(), MirRuntimeValue::Bool(result.publish)),
        ],
    }
}

fn runtime_to_queue_result(
    value: &MirRuntimeValue,
    span: Span,
) -> Result<JetJobResult, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(unsupported("JobResult", span));
    };
    if type_name != "JobResult" {
        return Err(unsupported("JobResult", span));
    }
    let type_id = queue_text(queue_field(fields, type_name, "type_id", span)?, type_name, "type_id", span)?;
    let bytes = match queue_field(fields, type_name, "bytes", span)? {
        MirRuntimeValue::Bytes(bytes) => bytes.clone(),
        _ => return Err(unsupported("JobResult.bytes", span)),
    };
    let publish = queue_bool(queue_field(fields, type_name, "publish", span)?, type_name, "publish", span)?;
    JetJobResult::new(type_id, bytes, publish).map_err(|_| unsupported("JobResult", span))
}

fn queue_error_to_runtime(error: JetJobError) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "JobError".to_string(),
        fields: vec![
            ("type_id".to_string(), MirRuntimeValue::String(error.type_id)),
            ("reason".to_string(), MirRuntimeValue::String(error.reason)),
            ("detail".to_string(), queue_optional_string(error.detail, "String")),
        ],
    }
}

fn runtime_to_queue_error(
    value: &MirRuntimeValue,
    span: Span,
) -> Result<JetJobError, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(unsupported("JobError", span));
    };
    if type_name != "JobError" {
        return Err(unsupported("JobError", span));
    }
    let detail = match queue_field(fields, type_name, "detail", span)? {
        MirRuntimeValue::Absent { .. } => None,
        MirRuntimeValue::Present(value) => match value.as_ref() {
            MirRuntimeValue::String(value) => Some(value.clone()),
            _ => return Err(unsupported("JobError.detail", span)),
        },
        MirRuntimeValue::String(value) => Some(value.clone()),
        _ => return Err(unsupported("JobError.detail", span)),
    };
    JetJobError::new(
        queue_text(queue_field(fields, type_name, "type_id", span)?, type_name, "type_id", span)?,
        queue_text(queue_field(fields, type_name, "reason", span)?, type_name, "reason", span)?,
        detail,
    )
    .map_err(|_| unsupported("JobError", span))
}

fn queue_claim_to_runtime(claim: JetJobQueueClaim) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "JobQueueClaim".to_string(),
        fields: vec![
            ("receipt".to_string(), queue_receipt_to_runtime(claim.receipt)),
            ("payload".to_string(), queue_payload_to_runtime(claim.payload)),
            ("worker".to_string(), MirRuntimeValue::String(claim.worker)),
            ("lease_token".to_string(), MirRuntimeValue::String(claim.lease_token)),
            ("lease_until_ms".to_string(), MirRuntimeValue::Int(claim.lease_until_ms)),
        ],
    }
}

fn runtime_to_queue_claim(
    value: &MirRuntimeValue,
    span: Span,
) -> Result<JetJobQueueClaim, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(unsupported("JobQueueClaim", span));
    };
    if type_name != "JobQueueClaim" {
        return Err(unsupported("JobQueueClaim", span));
    }
    Ok(JetJobQueueClaim {
        receipt: runtime_to_queue_receipt(queue_field(fields, type_name, "receipt", span)?, span)?,
        payload: runtime_to_queue_payload(queue_field(fields, type_name, "payload", span)?, span)?,
        worker: queue_text(queue_field(fields, type_name, "worker", span)?, type_name, "worker", span)?,
        lease_token: queue_text(queue_field(fields, type_name, "lease_token", span)?, type_name, "lease_token", span)?,
        lease_until_ms: queue_int(
            queue_field(fields, type_name, "lease_until_ms", span)?,
            type_name,
            "lease_until_ms",
            span,
        )?,
    })
}

fn queue_event_to_runtime(event: JetJobQueueEvent) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "JobQueueEvent".to_string(),
        fields: vec![
            ("sequence".to_string(), MirRuntimeValue::Int(event.sequence)),
            ("state".to_string(), queue_state_to_runtime(event.state)),
            ("attempts".to_string(), MirRuntimeValue::Int(event.attempts as i64)),
            ("timestamp_ms".to_string(), MirRuntimeValue::Int(event.timestamp_ms)),
            ("reason".to_string(), queue_optional_string(event.reason, "String")),
            ("duration_ms".to_string(), queue_optional_int(event.duration_ms)),
            ("worker".to_string(), queue_optional_string(event.worker, "String")),
        ],
    }
}

fn queue_record_to_runtime(record: JetJobQueueRecord) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "JobQueueRecord".to_string(),
        fields: vec![
            ("receipt".to_string(), queue_receipt_to_runtime(record.receipt)),
            (
                "payload".to_string(),
                record
                    .payload
                    .map(queue_payload_to_runtime)
                    .map(|value| MirRuntimeValue::Present(Box::new(value)))
                    .unwrap_or_else(|| mir_absent("JobPayload")),
            ),
            (
                "result".to_string(),
                record
                    .result
                    .map(queue_result_to_runtime)
                    .map(|value| MirRuntimeValue::Present(Box::new(value)))
                    .unwrap_or_else(|| mir_absent("JobResult")),
            ),
            (
                "error".to_string(),
                record
                    .error
                    .map(queue_error_to_runtime)
                    .map(|value| MirRuntimeValue::Present(Box::new(value)))
                    .unwrap_or_else(|| mir_absent("JobError")),
            ),
            ("started_at_ms".to_string(), queue_optional_int(record.started_at_ms)),
            ("finished_at_ms".to_string(), queue_optional_int(record.finished_at_ms)),
        ],
    }
}

fn queue_status_to_runtime(status: JetJobQueueStatus) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "JobQueueStatus".to_string(),
        fields: vec![
            ("queue".to_string(), MirRuntimeValue::String(status.queue)),
            ("authority".to_string(), MirRuntimeValue::String(status.authority)),
            ("queued".to_string(), MirRuntimeValue::Int(status.queued as i64)),
            ("running".to_string(), MirRuntimeValue::Int(status.running as i64)),
            ("retrying".to_string(), MirRuntimeValue::Int(status.retrying as i64)),
            ("completed".to_string(), MirRuntimeValue::Int(status.completed as i64)),
            ("failed".to_string(), MirRuntimeValue::Int(status.failed as i64)),
            ("dead_lettered".to_string(), MirRuntimeValue::Int(status.dead_lettered as i64)),
            ("cancelled".to_string(), MirRuntimeValue::Int(status.cancelled as i64)),
            ("depth".to_string(), MirRuntimeValue::Int(status.depth as i64)),
            ("wait_ms".to_string(), MirRuntimeValue::Int(status.wait_ms as i64)),
            ("throughput".to_string(), MirRuntimeValue::Int(status.throughput as i64)),
            ("capacity".to_string(), MirRuntimeValue::Int(status.capacity as i64)),
            ("paused".to_string(), MirRuntimeValue::Bool(status.paused)),
            ("freshness_ms".to_string(), MirRuntimeValue::Int(status.freshness_ms as i64)),
        ],
    }
}
fn queue_open(
    endpoint: &JetServiceEndpoint,
    name: &str,
) -> Result<JetJobQueue<'static>, JetServiceError> {
    JetJobQueue::open_default(endpoint, name.to_string(), JetJobQueuePolicy::default())
}

fn map_err(err: JetServiceError) -> MirRuntimeValue {
    let (variant, message) = match err {
        JetServiceError::Full(m) => ("Full", m),
        JetServiceError::Ambiguous(m) => ("Ambiguous", m),
        JetServiceError::Unknown(m) => ("Unknown", m),
        JetServiceError::NotStarted(m) => ("NotStarted", m),
        JetServiceError::Policy(m) => ("Policy", m),
        JetServiceError::Unavailable(m) => ("Unavailable", m),
        JetServiceError::Partitioned(m) => ("Partitioned", m),
        JetServiceError::Revoked(m) => ("Revoked", m),
        JetServiceError::Stale(m) => ("Stale", m),
        JetServiceError::Expired(m) => ("Expired", m),
    };
    MirRuntimeValue::Enum {
        type_name: "ServiceError".to_string(),
        variant: variant.to_string(),
        args: vec![(None, MirRuntimeValue::String(message))],
    }
}

fn delivery_state_to_runtime(state: JetDeliveryState) -> MirRuntimeValue {
    MirRuntimeValue::Enum {
        type_name: "DeliveryState".to_string(),
        variant: state.jet_show(),
        args: Vec::new(),
    }
}

fn delivery_record_to_runtime(delivery: JetDelivery) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "Delivery".to_string(),
        fields: vec![
            ("id".to_string(), MirRuntimeValue::String(delivery.id)),
            ("store".to_string(), MirRuntimeValue::String(delivery.store)),
            ("duplicate".to_string(), MirRuntimeValue::Bool(delivery.duplicate)),
            ("authority".to_string(), MirRuntimeValue::String(delivery.authority)),
            ("generation".to_string(), MirRuntimeValue::Int(delivery.generation)),
        ],
    }
}

fn runtime_to_delivery_record(value: &MirRuntimeValue, span: Span) -> Result<JetDelivery, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(unsupported("Delivery", span));
    };
    if type_name != "Delivery" {
        return Err(unsupported("Delivery", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find_map(|(field, value)| (field == name).then_some(value))
            .ok_or_else(|| unsupported(&format!("Delivery.{name}"), span))
    };
    let id = match field("id")? {
        MirRuntimeValue::String(value) => value.clone(),
        _ => return Err(unsupported("Delivery.id", span)),
    };
    let store = match field("store")? {
        MirRuntimeValue::String(value) => value.clone(),
        _ => return Err(unsupported("Delivery.store", span)),
    };
    let duplicate = match field("duplicate")? {
        MirRuntimeValue::Bool(value) => *value,
        _ => return Err(unsupported("Delivery.duplicate", span)),
    };
    let authority = match field("authority")? {
        MirRuntimeValue::String(value) => value.clone(),
        _ => return Err(unsupported("Delivery.authority", span)),
    };
    let generation = match field("generation")? {
        MirRuntimeValue::Int(value) => *value,
        _ => return Err(unsupported("Delivery.generation", span)),
    };
    Ok(JetDelivery {
        id,
        store,
        duplicate,
        authority,
        generation,
    })
}

fn delivery_receipt_to_runtime(receipt: JetDeliveryReceipt) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "DeliveryReceipt".to_string(),
        fields: vec![
            ("id".to_string(), MirRuntimeValue::String(receipt.id)),
            ("state".to_string(), delivery_state_to_runtime(receipt.state)),
            ("attempts".to_string(), MirRuntimeValue::Int(receipt.attempts)),
            (
                "retention_until".to_string(),
                MirRuntimeValue::Int(receipt.retention_until),
            ),
            ("deadline".to_string(), MirRuntimeValue::Int(receipt.deadline)),
            (
                "idempotency_key".to_string(),
                MirRuntimeValue::String(receipt.idempotency_key),
            ),
            ("duplicate".to_string(), MirRuntimeValue::Bool(receipt.duplicate)),
            ("authority".to_string(), MirRuntimeValue::String(receipt.authority)),
            ("generation".to_string(), MirRuntimeValue::Int(receipt.generation)),
            ("signature".to_string(), MirRuntimeValue::String(receipt.signature)),
        ],
    }
}

fn delivery_event_to_runtime(event: JetDeliveryEvent) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "DeliveryEvent".to_string(),
        fields: vec![
            ("sequence".to_string(), MirRuntimeValue::Int(event.sequence)),
            ("state".to_string(), delivery_state_to_runtime(event.state)),
            ("attempts".to_string(), MirRuntimeValue::Int(event.attempts)),
            ("timestamp".to_string(), MirRuntimeValue::Int(event.timestamp)),
            ("signature".to_string(), MirRuntimeValue::String(event.signature)),
        ],
    }
}

fn service_runtime_field<'a>(
    fields: &'a [(String, MirRuntimeValue)],
    type_name: &str,
    name: &str,
    span: Span,
) -> Result<&'a MirRuntimeValue, Diagnostic> {
    fields
        .iter()
        .find_map(|(field, value)| (field == name).then_some(value))
        .ok_or_else(|| unsupported(&format!("{type_name}.{name}"), span))
}

fn service_runtime_text(
    fields: &[(String, MirRuntimeValue)],
    type_name: &str,
    name: &str,
    span: Span,
) -> Result<String, Diagnostic> {
    match service_runtime_field(fields, type_name, name, span)? {
        MirRuntimeValue::String(value) => Ok(value.clone()),
        _ => Err(unsupported(&format!("{type_name}.{name}"), span)),
    }
}

fn service_runtime_int(
    fields: &[(String, MirRuntimeValue)],
    type_name: &str,
    name: &str,
    span: Span,
) -> Result<i64, Diagnostic> {
    match service_runtime_field(fields, type_name, name, span)? {
        MirRuntimeValue::Int(value) => Ok(*value),
        _ => Err(unsupported(&format!("{type_name}.{name}"), span)),
    }
}

fn service_runtime_bool(
    fields: &[(String, MirRuntimeValue)],
    type_name: &str,
    name: &str,
    span: Span,
) -> Result<bool, Diagnostic> {
    match service_runtime_field(fields, type_name, name, span)? {
        MirRuntimeValue::Bool(value) => Ok(*value),
        _ => Err(unsupported(&format!("{type_name}.{name}"), span)),
    }
}

fn runtime_to_delivery_state(value: &MirRuntimeValue, span: Span) -> Result<JetDeliveryState, Diagnostic> {
    let MirRuntimeValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(unsupported("DeliveryState", span));
    };
    if type_name != "DeliveryState" || !args.is_empty() {
        return Err(unsupported("DeliveryState", span));
    }
    match variant.as_str() {
        "Pending" => Ok(JetDeliveryState::Pending),
        "Accepted" => Ok(JetDeliveryState::Accepted),
        "Delivering" => Ok(JetDeliveryState::Delivering),
        "Delivered" => Ok(JetDeliveryState::Delivered),
        "DeadLettered" => Ok(JetDeliveryState::DeadLettered),
        "Cancelled" => Ok(JetDeliveryState::Cancelled),
        _ => Err(unsupported("DeliveryState variant", span)),
    }
}

fn runtime_to_delivery_receipt(value: &MirRuntimeValue, span: Span) -> Result<JetDeliveryReceipt, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(unsupported("DeliveryReceipt", span));
    };
    if type_name != "DeliveryReceipt" {
        return Err(unsupported("DeliveryReceipt", span));
    }
    Ok(JetDeliveryReceipt {
        id: service_runtime_text(fields, "DeliveryReceipt", "id", span)?,
        state: runtime_to_delivery_state(
            service_runtime_field(fields, "DeliveryReceipt", "state", span)?,
            span,
        )?,
        attempts: service_runtime_int(fields, "DeliveryReceipt", "attempts", span)?,
        retention_until: service_runtime_int(fields, "DeliveryReceipt", "retention_until", span)?,
        deadline: service_runtime_int(fields, "DeliveryReceipt", "deadline", span)?,
        idempotency_key: service_runtime_text(fields, "DeliveryReceipt", "idempotency_key", span)?,
        duplicate: service_runtime_bool(fields, "DeliveryReceipt", "duplicate", span)?,
        authority: service_runtime_text(fields, "DeliveryReceipt", "authority", span)?,
        generation: service_runtime_int(fields, "DeliveryReceipt", "generation", span)?,
        signature: service_runtime_text(fields, "DeliveryReceipt", "signature", span)?,
    })
}

fn runtime_to_delivery_event(value: &MirRuntimeValue, span: Span) -> Result<JetDeliveryEvent, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(unsupported("DeliveryEvent", span));
    };
    if type_name != "DeliveryEvent" {
        return Err(unsupported("DeliveryEvent", span));
    }
    Ok(JetDeliveryEvent {
        sequence: service_runtime_int(fields, "DeliveryEvent", "sequence", span)?,
        state: runtime_to_delivery_state(
            service_runtime_field(fields, "DeliveryEvent", "state", span)?,
            span,
        )?,
        attempts: service_runtime_int(fields, "DeliveryEvent", "attempts", span)?,
        timestamp: service_runtime_int(fields, "DeliveryEvent", "timestamp", span)?,
        signature: service_runtime_text(fields, "DeliveryEvent", "signature", span)?,
    })
}

fn runtime_to_service_error(value: &MirRuntimeValue, span: Span) -> Result<JetServiceError, Diagnostic> {
    let MirRuntimeValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(unsupported("ServiceError", span));
    };
    if type_name != "ServiceError" || args.len() != 1 {
        return Err(unsupported("ServiceError", span));
    }
    let MirRuntimeValue::String(message) = &args[0].1 else {
        return Err(unsupported("ServiceError message", span));
    };
    let message = message.clone();
    Ok(match variant.as_str() {
        "Full" => JetServiceError::Full(message),
        "Ambiguous" => JetServiceError::Ambiguous(message),
        "Unknown" => JetServiceError::Unknown(message),
        "NotStarted" => JetServiceError::NotStarted(message),
        "Policy" => JetServiceError::Policy(message),
        "Unavailable" => JetServiceError::Unavailable(message),
        "Partitioned" => JetServiceError::Partitioned(message),
        "Revoked" => JetServiceError::Revoked(message),
        "Stale" => JetServiceError::Stale(message),
        "Expired" => JetServiceError::Expired(message),
        _ => return Err(unsupported("ServiceError variant", span)),
    })
}

/// D-SERVICE-RECEIPT2=A / I9: the evaluator and resident JIT marshal service
/// values back into the Prelude types and call the same `JetShow` impls as AOT.
pub fn service_display_value_runtime(value: &MirRuntimeValue) -> Option<String> {
    let span = Span::new(0, 0);
    match value {
        MirRuntimeValue::Enum { type_name, .. } if type_name == "DeliveryState" => {
            Some(runtime_to_delivery_state(value, span).ok()?.jet_show())
        }
        MirRuntimeValue::Enum { type_name, .. } if type_name == "ServiceError" => {
            Some(runtime_to_service_error(value, span).ok()?.jet_show())
        }
        MirRuntimeValue::Struct { type_name, .. } if type_name == "Delivery" => {
            Some(runtime_to_delivery_record(value, span).ok()?.jet_show())
        }
        MirRuntimeValue::Struct { type_name, .. } if type_name == "DeliveryReceipt" => {
            Some(runtime_to_delivery_receipt(value, span).ok()?.jet_show())
        }
        MirRuntimeValue::Struct { type_name, .. } if type_name == "DeliveryEvent" => {
            Some(runtime_to_delivery_event(value, span).ok()?.jet_show())
        }
        MirRuntimeValue::Struct { type_name, .. } if type_name == "ServiceEndpoint" => {
            Some(runtime_to_endpoint(value, span).ok()?.jet_show())
        }
        MirRuntimeValue::Struct { type_name, .. } if type_name == "ServiceRuntime" => {
            Some(runtime_value_to_service_runtime(value, span).ok()?.jet_show())
        }
        _ => None,
    }
}

/// I9: extend the service display bridge for every named service value whose
/// Prelude `JetShow` implementation crosses a resident handle boundary.
pub fn service_show_value_runtime(value: &MirRuntimeValue) -> Option<String> {
    let span = Span::new(0, 0);
    match value {
        MirRuntimeValue::Enum { type_name, .. } if type_name == "ServiceRestart" => {
            Some(runtime_to_restart(value, span).ok()?.jet_show())
        }
        MirRuntimeValue::Enum { type_name, .. } if type_name == "ServiceDelivery" => {
            Some(runtime_to_delivery(value, span).ok()?.jet_show())
        }
        MirRuntimeValue::Enum { type_name, .. } if type_name == "TaskOutcome" => {
            Some(runtime_to_task_outcome(value, span).ok()?.jet_show())
        }
        MirRuntimeValue::Enum { type_name, .. } if type_name == "TaskStatus" => {
            Some(runtime_to_task_status(value, span).ok()?.jet_show())
        }
        MirRuntimeValue::Struct { type_name, .. } if type_name == "ServiceStateStore" => {
            Some(runtime_to_state_store(value, span).ok()?.jet_show())
        }
        MirRuntimeValue::Struct { type_name, .. } if type_name == "ServiceUpgradeReceipt" => {
            Some(runtime_to_upgrade_receipt(value, span).ok()?.jet_show())
        }
        _ => service_display_value_runtime(value),
    }
}

fn runtime_value_to_service_runtime(value: &MirRuntimeValue, span: Span) -> Result<JetServiceRuntime, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(unsupported("ServiceRuntime", span));
    };
    if type_name != "ServiceRuntime" {
        return Err(unsupported("ServiceRuntime", span));
    }
    let store = fields
        .iter()
        .find_map(|(name, value)| (name == "store").then_some(value))
        .ok_or_else(|| unsupported("ServiceRuntime.store", span))
        .and_then(|value| {
            runtime_to_service_string(value, SERVICE_AUTH_MAX_STORE, "ServiceRuntime.store", span)
        })?;
    let retention_ms = fields
        .iter()
        .find_map(|(name, value)| (name == "retention_ms").then_some(value))
        .ok_or_else(|| unsupported("ServiceRuntime.retention_ms", span))
        .and_then(|value| match value {
            MirRuntimeValue::Int(value) => Ok(*value),
            _ => Err(unsupported("ServiceRuntime.retention_ms", span)),
        })?;
    Ok(jet_services_runtime(store, retention_ms))
}

fn service_runtime_to_value(runtime: &JetServiceRuntime) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "ServiceRuntime".to_string(),
        fields: vec![
            ("store".to_string(), MirRuntimeValue::String(runtime.store.clone())),
            (
                "retention_ms".to_string(),
                MirRuntimeValue::Int(runtime.retention_ms),
            ),
        ],
    }
}

/// Apply a `ServiceRuntime` handle method through the same Prelude authority
/// functions used by AOT and the runtime ambient adapter. The TIR evaluator is
/// only responsible for MirRuntimeValue conversion at this boundary.
pub fn apply_runtime_method_mir(
    receiver: &MirRuntimeValue,
    method: &str,
    args: &[MirRuntimeValue],
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    let runtime = runtime_value_to_service_runtime(receiver, span)?;
    let one = |index: usize| {
        args.get(index)
            .ok_or_else(|| unsupported(&format!("ServiceRuntime.{method} arg {index}"), span))
    };
    match method {
        "send" => {
            let endpoint = runtime_to_endpoint(one(0)?, span)?;
            let message = runtime_to_service_string(
                one(1)?,
                SERVICE_AUTH_MAX_MESSAGE,
                "ServiceRuntime.send message",
                span,
            )?;
            let key = runtime_to_service_string(
                one(2)?,
                SERVICE_AUTH_MAX_KEY,
                "ServiceRuntime.send key",
                span,
            )?;
            Ok(
                match jet_services_runtime_send(&runtime, &endpoint, &message, &key) {
                    Ok(delivery) => MirRuntimeValue::Present(Box::new(delivery_record_to_runtime(delivery))),
                    Err(error) => MirRuntimeValue::FailedTold(Box::new(map_err(error))),
                },
            )
        }
        "retry" | "dead_letter" | "retain" => {
            let delivery = runtime_to_delivery_record(one(0)?, span)?;
            let result = match method {
                "retry" => jet_services_runtime_retry(&runtime, &delivery),
                "dead_letter" => jet_services_runtime_dead_letter(&runtime, &delivery),
                "retain" => jet_services_runtime_retain(&runtime, &delivery),
                _ => unreachable!(),
            };
            Ok(match result {
                Ok(delivery) => MirRuntimeValue::Present(Box::new(delivery_record_to_runtime(delivery))),
                Err(error) => MirRuntimeValue::FailedTold(Box::new(map_err(error))),
            })
        }
        "commit" => {
            let delivery = runtime_to_delivery_record(one(0)?, span)?;
            Ok(match jet_services_runtime_commit(&runtime, &delivery) {
                Ok(()) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::Unit)),
                Err(error) => MirRuntimeValue::FailedTold(Box::new(map_err(error))),
            })
        }
        _ => Err(unsupported(&format!("ServiceRuntime.{method}"), span)),
    }
}

fn mutate_ok(tree: JetServiceTree, value: MirRuntimeValue) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "__JetServiceMut".to_string(),
        fields: vec![
            ("tree".to_string(), tree_to_runtime(&tree)),
            ("value".to_string(), value),
        ],
    }
}

fn mutate_err(tree: JetServiceTree, error: MirRuntimeValue) -> MirRuntimeValue {
    MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::Struct {
        type_name: "__JetServiceMutErr".to_string(),
        fields: vec![
            ("tree".to_string(), tree_to_runtime(&tree)),
            ("error".to_string(), error),
        ],
    }))
}

fn mutate_handle_ok(
    handle: &JetWorkflowHandle,
    value: MirRuntimeValue,
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    Ok(MirRuntimeValue::Struct {
        type_name: "__JetServiceMut".to_string(),
        fields: vec![
            ("tree".to_string(), workflow_handle_to_runtime(handle, span)?),
            ("value".to_string(), value),
        ],
    })
}

fn mutate_handle_err(
    handle: &JetWorkflowHandle,
    error: MirRuntimeValue,
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    Ok(MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::Struct {
        type_name: "__JetServiceMutErr".to_string(),
        fields: vec![
            ("tree".to_string(), workflow_handle_to_runtime(handle, span)?),
            ("error".to_string(), error),
        ],
    })))
}

pub fn take_mut_runtime(value: MirRuntimeValue) -> Result<(MirRuntimeValue, MirRuntimeValue), MirRuntimeValue> {
    match value {
        MirRuntimeValue::Present(inner) => match *inner {

            MirRuntimeValue::Struct { type_name, fields } if type_name == "__JetServiceMut" => {
                let tree = fields
                    .iter()
                    .find(|(n, _)| n == "tree")
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| {
                        MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::String(
                            "core.service: missing tree write-back".to_string(),
                        )))
                    })?;
                let val = fields
                    .iter()
                    .find(|(n, _)| n == "value")
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| {
                        MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::String(
                            "core.service: missing value write-back".to_string(),
                        )))
                    })?;
                Ok((tree, MirRuntimeValue::Present(Box::new(val))))
            }
            other => Ok((MirRuntimeValue::Unit, MirRuntimeValue::Present(Box::new(other)))),
        },
        MirRuntimeValue::FailedTold(inner) => match *inner {
            MirRuntimeValue::Struct { type_name, fields } if type_name == "__JetServiceMutErr" => {
                let tree = fields
                    .iter()
                    .find(|(n, _)| n == "tree")
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| {
                        MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::String(
                            "core.service: missing tree write-back".to_string(),
                        )))
                    })?;
                let error = fields
                    .iter()
                    .find(|(n, _)| n == "error")
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| {
                        MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::String(
                            "core.service: missing error write-back".to_string(),
                        )))
                    })?;
                Ok((tree, MirRuntimeValue::FailedTold(Box::new(error))))
            }
            other => Err(MirRuntimeValue::FailedTold(Box::new(other))),
        },
        other => Err(other),
    }
}

pub fn apply_runtime_start_with_dispatcher<F>(
    value: &MirRuntimeValue,
    span: Span,
    dispatch: F,
) -> Result<MirRuntimeValue, Diagnostic>
where
    F: FnMut(&str, &JetServiceEndpoint) -> Result<(), JetServiceError>,
{
    let mut tree = runtime_to_tree(value, span)?;
    match jet_services_start_with_queue_dispatcher(&mut tree, dispatch) {
        Ok(()) => Ok(MirRuntimeValue::Present(Box::new(mutate_ok(
            tree,
            MirRuntimeValue::Unit,
        )))),
        Err(error) => Ok(mutate_err(tree, map_err(error))),
    }
}
pub fn apply_runtime(
    method: &str,
    args: &[MirRuntimeValue],
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    let one = |i: usize| {
        args.get(i)
            .ok_or_else(|| unsupported(&format!("core.service.{method} arg {i}"), span))
    };
    match method {
        "tree" => {
            let name = runtime_to_service_string(one(0)?, MAX_SERVICE_NAME, "tree name", span)?;
            Ok(tree_to_runtime(&jet_services_tree(name)))
        }
        "runtime" => {
            let store = runtime_to_service_string(
                one(0)?,
                SERVICE_AUTH_MAX_STORE,
                "ServiceRuntime.store",
                span,
            )?;
            let retention_ms = match one(1)? {
                MirRuntimeValue::Struct { type_name, fields } if type_name == "Duration" => fields
                    .iter()
                    .find_map(|(name, value)| match (name.as_str(), value) {
                        ("ns", MirRuntimeValue::Int(ns)) => Some(*ns / 1_000_000),
                        _ => None,
                    })
                    .ok_or_else(|| unsupported("ServiceRuntime.retention_ms", span))?,
                _ => return Err(unsupported("ServiceRuntime.retention_ms", span)),
            };
            Ok(service_runtime_to_value(&jet_services_runtime(store, retention_ms)))
        }
        "restart_one_for_one" => Ok(restart_to_runtime(jet_services_restart_one_for_one())),
        "restart_one_for_all" => Ok(restart_to_runtime(jet_services_restart_one_for_all())),
        "restart_rest_for_one" => Ok(restart_to_runtime(jet_services_restart_rest_for_one())),
        "delivery_at_most_once" => Ok(delivery_to_runtime(jet_services_delivery_at_most_once())),
        "delivery_durable" => Ok(delivery_to_runtime(jet_services_delivery_durable())),
        "state_store" => {
            let path = runtime_to_service_string(
                one(0)?,
                MAX_SERVICE_STATE_STORE,
                "service state store",
                span,
            )?;
            Ok(match jet_services_state_store(path) {
                Ok(store) => MirRuntimeValue::Present(Box::new(state_store_to_runtime(&store))),
                Err(error) => MirRuntimeValue::FailedTold(Box::new(map_err(error))),
            })
        }
        "set_restart" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let restart = runtime_to_restart(one(1)?, span)?;
            Ok(match jet_services_set_restart(&mut tree, restart) {
                Ok(()) => MirRuntimeValue::Present(Box::new(mutate_ok(tree, MirRuntimeValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "set_delivery" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let delivery = runtime_to_delivery(one(1)?, span)?;
            Ok(match jet_services_set_delivery(&mut tree, delivery) {
                Ok(()) => MirRuntimeValue::Present(Box::new(mutate_ok(tree, MirRuntimeValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "worker" => {
            if args.len() != 5 {
                return Err(unsupported("core.service.worker expects five arguments", span));
            }
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let name = runtime_to_service_string(one(1)?, MAX_SERVICE_NAME, "worker name", span)?;
            let handler_name =
                runtime_to_service_string(one(3)?, MAX_SERVICE_NAME, "worker handler", span)?;
            let capacity = match one(4)? {
                MirRuntimeValue::Int(n) => *n,
                _ => return Err(unsupported("capacity", span)),
            };
            Ok(match jet_services_worker(
                &mut tree,
                name,
                jet_services_noop_worker,
                handler_name,
                capacity,
            ) {
                Ok(ep) => MirRuntimeValue::Present(Box::new(mutate_ok(tree, endpoint_to_runtime(&ep)))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "group" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let name = match one(1)? {
                value => runtime_to_service_string(value, MAX_SERVICE_NAME, "group name", span)?,
            };
            let workers = match one(2)? {
                MirRuntimeValue::List(xs) => {
                    if xs.len() > MAX_SERVICE_WORKERS {
                        return Err(unsupported("group worker limit", span));
                    }
                    xs.iter()
                        .map(|x| runtime_to_service_string(x, MAX_SERVICE_NAME, "group worker", span))
                        .collect::<Result<Vec<_>, _>>()?
                }
                _ => return Err(unsupported("group workers", span)),
            };
            Ok(match jet_services_group(&mut tree, name, workers) {
                Ok(()) => MirRuntimeValue::Present(Box::new(mutate_ok(tree, MirRuntimeValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "stop" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            Ok(match jet_services_stop(&mut tree) {
                Ok(()) => MirRuntimeValue::Present(Box::new(mutate_ok(tree, MirRuntimeValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "send" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let endpoint = runtime_to_endpoint(one(1)?, span)?;
            let message = match one(2)? {
                value => runtime_to_service_string(value, MAX_SERVICE_MESSAGE, "message", span)?,
            };
            Ok(match jet_services_send(&mut tree, &endpoint, message) {
                Ok(()) => MirRuntimeValue::Present(Box::new(mutate_ok(tree, MirRuntimeValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "receive" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let endpoint = runtime_to_endpoint(one(1)?, span)?;
            Ok(match jet_services_receive(&mut tree, &endpoint) {
                Ok(msg) => MirRuntimeValue::Present(Box::new(mutate_ok(tree, MirRuntimeValue::String(msg)))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "endpoint_send" => {
            let endpoint = runtime_to_endpoint(one(0)?, span)?;
            let message = runtime_to_service_string(one(1)?, MAX_SERVICE_MESSAGE, "message", span)?;
            Ok(match jet_services_endpoint_send(&endpoint, message) {
                Ok(()) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::Unit)),
                Err(error) => MirRuntimeValue::FailedTold(Box::new(map_err(error))),
            })
        }
        "endpoint_receive" => {
            let endpoint = runtime_to_endpoint(one(0)?, span)?;
            Ok(match jet_services_endpoint_receive(&endpoint) {
                Ok(message) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::String(message))),
                Err(error) => MirRuntimeValue::FailedTold(Box::new(map_err(error))),
            })
        }
        "mailbox_depth" => {
            let tree = runtime_to_tree(one(0)?, span)?;
            let endpoint = runtime_to_endpoint(one(1)?, span)?;
            Ok(match jet_services_mailbox_depth(&tree, &endpoint) {
                Ok(n) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::Int(n))),
                Err(e) => MirRuntimeValue::FailedTold(Box::new(map_err(e))),
            })
        }
        "restarts" => {
            let tree = runtime_to_tree(one(0)?, span)?;
            let endpoint = runtime_to_endpoint(one(1)?, span)?;
            Ok(match jet_services_restarts(&tree, &endpoint) {
                Ok(n) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::Int(n))),
                Err(e) => MirRuntimeValue::FailedTold(Box::new(map_err(e))),
            })
        }
        "fail_worker" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let endpoint = runtime_to_endpoint(one(1)?, span)?;
            Ok(match jet_services_fail_worker(&mut tree, &endpoint) {
                Ok(()) => MirRuntimeValue::Present(Box::new(mutate_ok(tree, MirRuntimeValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "endpoint_show" => Ok(MirRuntimeValue::String(jet_services_endpoint_show(&runtime_to_endpoint(
            one(0)?,
            span,
        )?))),
        "delivery_state_show" => Ok(MirRuntimeValue::String(jet_services_delivery_state_show(
            &runtime_to_delivery_state(one(0)?, span)?,
        ))),
        "tree_show" => Ok(MirRuntimeValue::String(jet_services_tree_show(&runtime_to_tree(
            one(0)?,
            span,
        )?))),
        "send_durable" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let endpoint = runtime_to_endpoint(one(1)?, span)?;
            let message = match one(2)? {
                MirRuntimeValue::String(s) => s.clone(),
                _ => return Err(unsupported("message", span)),
            };
            let key = match one(3)? {
                MirRuntimeValue::String(s) => s.clone(),
                _ => return Err(unsupported("idempotency key", span)),
            };
            Ok(
                match jet_services_send_durable(&mut tree, &endpoint, message, key) {
                    Ok(receipt) => {
                        MirRuntimeValue::Present(Box::new(mutate_ok(tree, delivery_record_to_runtime(receipt))))
                    }
                    Err(e) => mutate_err(tree, map_err(e)),
                },
            )
        }
        "delivery_wait" | "delivery_status" | "delivery_retry" | "delivery_cancel"
        | "delivery_receipt" | "delivery_events" => {
            let delivery = runtime_to_delivery_record(one(0)?, span)?;
            let result = match method {
                "delivery_wait" => jet_services_delivery_wait(&delivery).map(delivery_state_to_runtime),
                "delivery_status" => {
                    jet_services_delivery_status(&delivery).map(delivery_state_to_runtime)
                }
                "delivery_retry" => {
                    jet_services_delivery_retry(&delivery).map(delivery_record_to_runtime)
                }
                "delivery_cancel" => {
                    jet_services_delivery_cancel(&delivery).map(delivery_record_to_runtime)
                }
                "delivery_receipt" => {
                    jet_services_delivery_receipt(&delivery).map(delivery_receipt_to_runtime)
                }
                "delivery_events" => jet_services_delivery_events(&delivery).map(|events| {
                    MirRuntimeValue::List(events.into_iter().map(delivery_event_to_runtime).collect())
                }),
                _ => unreachable!(),
            };
            Ok(match result {
                Ok(value) => MirRuntimeValue::Present(Box::new(value)),
                Err(error) => MirRuntimeValue::FailedTold(Box::new(map_err(error))),
            })
        }
        "dead_letter_count" => Ok(MirRuntimeValue::Int(jet_services_dead_letter_count(&runtime_to_tree(
            one(0)?,
            span,
        )?))),
        "drain_dead_letters" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            Ok(match jet_services_drain_dead_letters(&mut tree) {
                Ok(n) => MirRuntimeValue::Present(Box::new(mutate_ok(tree, MirRuntimeValue::Int(n)))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "set_state_empty" | "set_state_snapshot" | "set_state_event_log" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let result = match method {
                "set_state_empty" => jet_services_set_state_empty(&mut tree),
                "set_state_snapshot" => {
                    let store = runtime_to_state_store(one(1)?, span)?;
                    let schema = runtime_to_service_string(
                        one(2)?,
                        MAX_SERVICE_STATE_SCHEMA,
                        "service state schema",
                        span,
                    )?;
                    let version = match one(3)? {
                        MirRuntimeValue::Int(version) => *version,
                        _ => return Err(unsupported("service state version", span)),
                    };
                    let migration = runtime_to_service_string(
                        one(4)?,
                        MAX_SERVICE_STATE_SCHEMA,
                        "service state migration policy",
                        span,
                    )?;
                    jet_services_set_state_snapshot(&mut tree, store, schema, version, migration)
                }
                _ => {
                    let store = runtime_to_state_store(one(1)?, span)?;
                    let schema = runtime_to_service_string(
                        one(2)?,
                        MAX_SERVICE_STATE_SCHEMA,
                        "service state schema",
                        span,
                    )?;
                    let version = match one(3)? {
                        MirRuntimeValue::Int(version) => *version,
                        _ => return Err(unsupported("service state version", span)),
                    };
                    let migration = runtime_to_service_string(
                        one(4)?,
                        MAX_SERVICE_STATE_SCHEMA,
                        "service state migration policy",
                        span,
                    )?;
                    jet_services_set_state_event_log(&mut tree, store, schema, version, migration)
                }
            };
            Ok(match result {
                Ok(()) => MirRuntimeValue::Present(Box::new(mutate_ok(tree, MirRuntimeValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "commit_snapshot" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let payload = match one(1)? {
                MirRuntimeValue::String(s) => s.clone(),
                _ => return Err(unsupported("snapshot payload", span)),
            };
            Ok(match jet_services_commit_snapshot(&mut tree, payload) {
                Ok(()) => MirRuntimeValue::Present(Box::new(mutate_ok(tree, MirRuntimeValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "restore_snapshot" => {
            let tree = runtime_to_tree(one(0)?, span)?;
            Ok(match jet_services_restore_snapshot(&tree) {
                Ok(s) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::String(s))),
                Err(e) => MirRuntimeValue::FailedTold(Box::new(map_err(e))),
            })
        }
        "append_event" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let event = match one(1)? {
                MirRuntimeValue::String(s) => s.clone(),
                _ => return Err(unsupported("event", span)),
            };
            Ok(match jet_services_append_event(&mut tree, event) {
                Ok(()) => MirRuntimeValue::Present(Box::new(mutate_ok(tree, MirRuntimeValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "event_count" => Ok(MirRuntimeValue::Int(jet_services_event_count(&runtime_to_tree(
            one(0)?,
            span,
        )?))),
        "replay_events" => Ok(MirRuntimeValue::String(jet_services_replay_events(&runtime_to_tree(
            one(0)?,
            span,
        )?))),
        "workflow_start" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let id = match one(1)? {
                MirRuntimeValue::String(s) => s.clone(),
                _ => return Err(unsupported("workflow id", span)),
            };
            let version = match one(2)? {
                MirRuntimeValue::Int(n) => *n,
                _ => return Err(unsupported("workflow version", span)),
            };
            Ok(match jet_services_workflow_start(&mut tree, id, version) {
                Ok(handle) => MirRuntimeValue::Present(Box::new(mutate_ok(
                    tree,
                    workflow_handle_to_runtime(&handle, span)?,
                ))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "workflow_sleep" => {
            let handle = runtime_to_workflow_handle(one(0)?, span)?;
            let nanos = match one(1)? {
                MirRuntimeValue::Struct { type_name, fields } if type_name == "Duration" => fields
                    .iter()
                    .find_map(|(name, value)| match (name.as_str(), value) {
                        ("ns", MirRuntimeValue::Int(ns)) => Some(*ns),
                        _ => None,
                    })
                    .ok_or_else(|| unsupported("workflow sleep duration", span))?,
                _ => return Err(unsupported("workflow sleep duration", span)),
            };
            Ok(match jet_services_workflow_sleep(&handle, nanos) {
                Ok(()) => {
                    MirRuntimeValue::Present(Box::new(mutate_handle_ok(&handle, MirRuntimeValue::Unit, span)?))
                }
                Err(e) => mutate_handle_err(&handle, map_err(e), span)?,
            })
        }
        "workflow_activity_wait" => {
            let handle = runtime_to_workflow_handle(one(0)?, span)?;
            let activity =
                runtime_to_service_string(one(1)?, MAX_SERVICE_NAME, "workflow activity", span)?;
            let argument = runtime_to_service_string(
                one(2)?,
                MAX_SERVICE_MESSAGE,
                "workflow activity argument",
                span,
            )?;
            Ok(
                match jet_services_workflow_activity_wait(&handle, activity, argument) {
                    Ok(value) => MirRuntimeValue::Present(Box::new(mutate_handle_ok(
                        &handle,
                        MirRuntimeValue::String(value),
                        span,
                    )?)),
                    Err(e) => mutate_handle_err(&handle, map_err(e), span)?,
                },
            )
        }
        "workflow_all" => {
            let handle = runtime_to_workflow_handle(one(0)?, span)?;
            let values = match one(1)? {
                MirRuntimeValue::List(values) => values
                    .iter()
                    .map(|value| {
                        runtime_to_service_string(value, MAX_SERVICE_MESSAGE, "workflow all value", span)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                _ => return Err(unsupported("workflow all values", span)),
            };
            Ok(match jet_services_workflow_all(&handle, values) {
                Ok(values) => MirRuntimeValue::Present(Box::new(mutate_handle_ok(
                    &handle,
                    MirRuntimeValue::List(values.into_iter().map(MirRuntimeValue::String).collect()),
                    span,
                )?)),
                Err(e) => mutate_handle_err(&handle, map_err(e), span)?,
            })
        }
        "workflow_step" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let run_id = runtime_to_workflow_run_id(one(1)?, span)?;
            let step = match one(2)? {
                MirRuntimeValue::String(s) => s.clone(),
                _ => return Err(unsupported("step", span)),
            };
            Ok(match jet_services_workflow_step(&mut tree, run_id, step) {
                Ok(()) => MirRuntimeValue::Present(Box::new(mutate_ok(tree, MirRuntimeValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "workflow_activity" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let run_id = runtime_to_workflow_run_id(one(1)?, span)?;
            let activity = match one(2)? {
                MirRuntimeValue::String(s) => s.clone(),
                _ => return Err(unsupported("workflow activity", span)),
            };
            let key = match one(3)? {
                MirRuntimeValue::String(s) => s.clone(),
                _ => return Err(unsupported("activity idempotency key", span)),
            };
            let max_attempts = match one(4)? {
                MirRuntimeValue::Int(n) => *n,
                _ => return Err(unsupported("activity retry limit", span)),
            };
            Ok(
                match jet_services_workflow_activity(&mut tree, run_id, activity, key, max_attempts)
                {
                    Ok(status) => {
                        MirRuntimeValue::Present(Box::new(mutate_ok(tree, task_status_to_runtime(&status))))
                    }
                    Err(e) => mutate_err(tree, map_err(e)),
                },
            )
        }
        "workflow_activity_retry" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let run_id = runtime_to_workflow_run_id(one(1)?, span)?;
            let key = match one(2)? {
                MirRuntimeValue::String(s) => s.clone(),
                _ => return Err(unsupported("activity idempotency key", span)),
            };
            let outcome = runtime_to_task_outcome(one(3)?, span)?;
            Ok(
                match jet_services_workflow_activity_retry(&mut tree, run_id, key, outcome) {
                    Ok(status) => {
                        MirRuntimeValue::Present(Box::new(mutate_ok(tree, task_status_to_runtime(&status))))
                    }
                    Err(e) => mutate_err(tree, map_err(e)),
                },
            )
        }
        "workflow_activity_complete" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let run_id = runtime_to_workflow_run_id(one(1)?, span)?;
            let key = match one(2)? {
                MirRuntimeValue::String(s) => s.clone(),
                _ => return Err(unsupported("activity idempotency key", span)),
            };
            let outcome = runtime_to_task_outcome(one(3)?, span)?;
            Ok(
                match jet_services_workflow_activity_complete(&mut tree, run_id, key, outcome) {
                    Ok(outcome) => {
                        MirRuntimeValue::Present(Box::new(mutate_ok(tree, task_outcome_to_runtime(&outcome))))
                    }
                    Err(e) => mutate_err(tree, map_err(e)),
                },
            )
        }
        "workflow_history" => {
            let tree = runtime_to_tree(one(0)?, span)?;
            let run_id = runtime_to_workflow_run_id(one(1)?, span)?;
            Ok(match jet_services_workflow_history(&tree, run_id) {
                Ok(s) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::String(s))),
                Err(e) => MirRuntimeValue::FailedTold(Box::new(map_err(e))),
            })
        }
        "workflow_outcome" => {
            let tree = runtime_to_tree(one(0)?, span)?;
            let run_id = runtime_to_workflow_run_id(one(1)?, span)?;
            Ok(match jet_services_workflow_outcome(&tree, run_id) {
                Ok(outcome) => MirRuntimeValue::Present(Box::new(task_outcome_to_runtime(&outcome))),
                Err(e) => MirRuntimeValue::FailedTold(Box::new(map_err(e))),
            })
        }
        "directory_register" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let name = match one(1)? {
                MirRuntimeValue::String(s) => s.clone(),
                _ => return Err(unsupported("directory name", span)),
            };
            let endpoint = runtime_to_endpoint(one(2)?, span)?;
            Ok(
                match jet_services_directory_register(&mut tree, name, endpoint) {
                    Ok(()) => MirRuntimeValue::Present(Box::new(mutate_ok(tree, MirRuntimeValue::Unit))),
                    Err(e) => mutate_err(tree, map_err(e)),
                },
            )
        }
        "directory_resolve" => {
            let tree = runtime_to_tree(one(0)?, span)?;
            let name = match one(1)? {
                MirRuntimeValue::String(s) => s.clone(),
                _ => return Err(unsupported("directory name", span)),
            };
            Ok(match jet_services_directory_resolve(&tree, &name) {
                Ok(ep) => MirRuntimeValue::Present(Box::new(endpoint_to_runtime(&ep))),
                Err(e) => MirRuntimeValue::FailedTold(Box::new(map_err(e))),
            })
        }
        "directory_generation" => Ok(MirRuntimeValue::Int(jet_services_directory_generation(
            &runtime_to_tree(one(0)?, span)?,
        ))),
        "drain_worker" | "partition_worker" | "reconcile_worker" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let endpoint = runtime_to_endpoint(one(1)?, span)?;
            let result = match method {
                "drain_worker" => jet_services_drain_worker(&mut tree, &endpoint),
                "partition_worker" => jet_services_partition_worker(&mut tree, &endpoint),
                _ => jet_services_reconcile_worker(&mut tree, &endpoint),
            };
            Ok(match result {
                Ok(()) => MirRuntimeValue::Present(Box::new(mutate_ok(tree, MirRuntimeValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "handoff_generation" | "rollback_generation" | "chaos_fail" => {
            let mut tree = runtime_to_tree(one(0)?, span)?;
            let result = match method {
                "handoff_generation" => jet_services_handoff_generation(&mut tree),
                "rollback_generation" => jet_services_rollback_generation(&mut tree),
                _ => jet_services_chaos_fail(&mut tree),
            };
            Ok(match result {
                Ok(n) => MirRuntimeValue::Present(Box::new(mutate_ok(tree, MirRuntimeValue::Int(n)))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "upgrade_receipt" => Ok(
            match jet_services_upgrade_receipt(&runtime_to_tree(one(0)?, span)?) {
                Ok(receipt) => MirRuntimeValue::Present(Box::new(upgrade_receipt_to_runtime(&receipt))),
                Err(error) => MirRuntimeValue::FailedTold(Box::new(map_err(error))),
            },
        ),
        "observe" => Ok(MirRuntimeValue::String(jet_services_observe(&runtime_to_tree(
            one(0)?,
            span,
        )?))),
        _ => Err(unsupported(&format!("`core.service.{method}()`"), span)),
    }
}
fn legacy_conversion_error(label: &str) -> Diagnostic {
    unsupported(label, Span::new(0, 0))
}

pub fn ct_to_runtime_value(value: &CtValue) -> Result<MirRuntimeValue, Diagnostic> {
    match value {
        CtValue::Int(value) => Ok(MirRuntimeValue::Int(*value)),
        CtValue::Float(CtFloat::F32(value)) => Ok(MirRuntimeValue::Float {
            value: f64::from(*value),
            f32: true,
        }),
        CtValue::Float(CtFloat::F64(value)) => Ok(MirRuntimeValue::Float {
            value: *value,
            f32: false,
        }),
        CtValue::Bool(value) => Ok(MirRuntimeValue::Bool(*value)),
        CtValue::Char(value) => Ok(MirRuntimeValue::Char(*value)),
        CtValue::Str(value) => Ok(MirRuntimeValue::String(value.clone())),
        CtValue::BigInt(value) => Ok(MirRuntimeValue::BigInt(value.to_string_rep())),
        CtValue::Bytes(values) => Ok(MirRuntimeValue::Bytes(values.clone())),
        CtValue::List(values) => values
            .iter()
            .map(ct_to_runtime_value)
            .collect::<Result<Vec<_>, _>>()
            .map(MirRuntimeValue::List),
        CtValue::Map(values) => values
            .iter()
            .map(|(key, value)| {
                let key = super::MirBridge::ct_to_mir_key(key.clone());
                Ok((key, ct_to_runtime_value(value)?))
            })
            .collect::<Result<Vec<_>, Diagnostic>>()
            .map(MirRuntimeValue::Map),
        CtValue::Struct { type_name, fields } => Ok(MirRuntimeValue::Struct {
            type_name: type_name.clone(),
            fields: fields
                .iter()
                .map(|(name, value)| Ok((name.clone(), ct_to_runtime_value(value)?)))
                .collect::<Result<Vec<_>, Diagnostic>>()?,
        }),
        CtValue::Enum {
            type_name,
            variant,
            args,
        } => Ok(MirRuntimeValue::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
            args: args
                .iter()
                .map(|(name, value)| Ok((name.clone(), ct_to_runtime_value(value)?)))
                .collect::<Result<Vec<_>, Diagnostic>>()?,
        }),
        CtValue::Present(value) => Ok(MirRuntimeValue::Present(Box::new(ct_to_runtime_value(value)?))),
        CtValue::Failed(CtReport::Clean(ty)) => Ok(MirRuntimeValue::Absent {
            element: mir_compat_type(ty),
        }),
        CtValue::Failed(CtReport::Told(value)) => {
            Ok(MirRuntimeValue::FailedTold(Box::new(ct_to_runtime_value(value)?)))
        }
        CtValue::Unit => Ok(MirRuntimeValue::Unit),
        CtValue::Closure(_) => Err(legacy_conversion_error("legacy closure service value")),
    }
}

pub fn runtime_to_ct_value(value: &MirRuntimeValue) -> Result<CtValue, Diagnostic> {
    match value {
        MirRuntimeValue::Int(value) => Ok(CtValue::Int(*value)),
        MirRuntimeValue::BigInt(value) => crate::Numeric::CtBigInt::from_str(value)
            .map(CtValue::BigInt)
            .map_err(|_| legacy_conversion_error("runtime BigInt service value")),
        MirRuntimeValue::Float { value, f32 } => Ok(CtValue::Float(if *f32 {
            CtFloat::F32(*value as f32)
        } else {
            CtFloat::F64(*value)
        })),
        MirRuntimeValue::Bool(value) => Ok(CtValue::Bool(*value)),
        MirRuntimeValue::Char(value) => Ok(CtValue::Char(*value)),
        MirRuntimeValue::String(value) => Ok(CtValue::Str(value.clone())),
        MirRuntimeValue::Bytes(values) => Ok(CtValue::Bytes(values.clone())),
        MirRuntimeValue::List(values) => values
            .iter()
            .map(runtime_to_ct_value)
            .collect::<Result<Vec<_>, _>>()
            .map(CtValue::List),
        MirRuntimeValue::Map(values) => values
            .iter()
            .map(|(key, value)| {
                let key = super::MirBridge::mir_to_ct_key(key);
                Ok((key, runtime_to_ct_value(value)?))
            })
            .collect::<Result<std::collections::BTreeMap<_, _>, Diagnostic>>()
            .map(CtValue::Map),
        MirRuntimeValue::Struct { type_name, fields } => Ok(CtValue::Struct {
            type_name: type_name.clone(),
            fields: fields
                .iter()
                .map(|(name, value)| Ok((name.clone(), runtime_to_ct_value(value)?)))
                .collect::<Result<Vec<_>, Diagnostic>>()?,
        }),
        MirRuntimeValue::Enum {
            type_name,
            variant,
            args,
        } => Ok(CtValue::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
            args: args
                .iter()
                .map(|(name, value)| Ok((name.clone(), runtime_to_ct_value(value)?)))
                .collect::<Result<Vec<_>, Diagnostic>>()?,
        }),
        MirRuntimeValue::Present(value) => Ok(CtValue::Present(Box::new(runtime_to_ct_value(value)?))),
        MirRuntimeValue::FailedTold(value) => Ok(CtValue::Failed(CtReport::Told(Box::new(
            runtime_to_ct_value(value)?,
        )))),
        MirRuntimeValue::Absent { element } => Ok(CtValue::Failed(CtReport::Clean(Box::new(
            Type::Named(element.display_name()),
        )))),
        MirRuntimeValue::Unit => Ok(CtValue::Unit),
        MirRuntimeValue::Moved => Err(legacy_conversion_error("internal moved service value")),
        MirRuntimeValue::Closure(_) => Err(legacy_conversion_error("runtime closure service value")),
    }
}
/// Encode an interpreter job value with the same canonical CBOR kernel used by
/// generated queue adapters. Queue payloads retain their checked runtime shape
/// instead of falling back to a display-oriented AST fragment.
pub fn interpreter_job_cbor_bytes(
    value: &MirRuntimeValue,
    span: Span,
) -> Result<Vec<u8>, Diagnostic> {
    let value = runtime_to_ct_value(value)?;
    crate::Comptime::EncodingLite::cbor_encode_canonical(&value)
        .map_err(|error| unsupported(&format!("Job payload CBOR encoding: {error:?}"), span))
}


pub fn service_runtime_value(store: String, retention_ms: i64) -> MirRuntimeValue {
    service_runtime_to_value(&jet_services_runtime(store, retention_ms))
}

pub fn service_display_value(value: &CtValue) -> Option<String> {
    ct_to_runtime_value(value)
        .ok()
        .and_then(|value| service_display_value_runtime(&value))
}

pub fn service_show_value(value: &CtValue) -> Option<String> {
    ct_to_runtime_value(value)
        .ok()
        .and_then(|value| service_show_value_runtime(&value))
}

pub fn apply_runtime_method(
    receiver: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let receiver = ct_to_runtime_value(receiver)?;
    let args = args
        .iter()
        .map(ct_to_runtime_value)
        .collect::<Result<Vec<_>, _>>()?;
    apply_runtime_method_mir(&receiver, method, &args, span).and_then(|value| runtime_to_ct_value(&value))
}

pub fn take_mut_ok(value: CtValue) -> Result<(CtValue, CtValue), CtValue> {
    let runtime = match ct_to_runtime_value(&value) {
        Ok(value) => value,
        Err(_) => return Err(value),
    };
    match take_mut_runtime(runtime) {
        Ok((tree, result)) => match (runtime_to_ct_value(&tree), runtime_to_ct_value(&result)) {
            (Ok(tree), Ok(value)) => Ok((tree, value)),
            _ => Err(value),
        },
        Err(result) => runtime_to_ct_value(&result).map_or_else(|_| Err(value), Err),
    }
}

pub fn apply(
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let mut args = args
        .iter()
        .map(|value| match (method, value) {
            // Engines keep the callback in `service_callbacks`. The kernel
            // only needs arity plus the handler identity at args[3].
            ("worker", CtValue::Closure(_)) => Ok(MirRuntimeValue::Unit),
            _ => ct_to_runtime_value(value),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(tree) = args.first_mut() {
        *tree = intern_resolve(std::mem::replace(tree, MirRuntimeValue::Unit));
    }
    let slot = args.first().and_then(intern_slot);
    apply_runtime(method, &args, span)
        .map(|value| intern_commit(slot, value))
        .and_then(|value| runtime_to_ct_value(&value))
}

fn queue_failed(error: JetServiceError) -> MirRuntimeValue {
    MirRuntimeValue::FailedTold(Box::new(map_err(error)))
}

fn queue_one<'a>(
    args: &'a [MirRuntimeValue],
    index: usize,
    method: &str,
    span: Span,
) -> Result<&'a MirRuntimeValue, Diagnostic> {
    args.get(index)
        .ok_or_else(|| unsupported(&format!("core.jobs.{method} arg {index}"), span))
}

fn queue_timeout_ms(value: &MirRuntimeValue, span: Span) -> Result<i64, Diagnostic> {
    match value {
        MirRuntimeValue::Struct { type_name, .. } if type_name == "Duration" => {
            queue_duration_ms(value, span)
        }
        _ => Err(unsupported("JobQueue.wait timeout", span)),
    }
}

fn apply_jobs_runtime_for_endpoint(
    endpoint: &JetServiceEndpoint,
    method: &str,
    args: &[MirRuntimeValue],
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    match method {
        "queue" => {
            if !args.is_empty() {
                return Err(unsupported("`core.jobs.queue()` takes no arguments", span));
            }
            Ok(queue_result_value(
                queue_open(endpoint, JOB_QUEUE_DEFAULT_NAME)
                    .map(|_| queue_to_runtime(endpoint, JOB_QUEUE_DEFAULT_NAME)),
                |value| value,
            ))
        }
        _ => Err(unsupported(&format!("`core.jobs.{method}()`"), span)),
    }
}

/// Apply a direct `core.jobs` call for an explicitly checked endpoint.
/// Runtime adapters use this form when the graph entry has an endpoint value;
/// the ordinary wrapper below obtains the same endpoint from the active scope.
pub fn apply_jobs_runtime(
    method: &str,
    args: &[MirRuntimeValue],
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    let endpoint = match jet_services_active_execution_endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => return Ok(queue_failed(error)),
    };
    apply_jobs_runtime_for_endpoint(&endpoint, method, args, span)
}

pub fn apply_jobs_runtime_with_endpoint(
    endpoint: &JetServiceEndpoint,
    method: &str,
    args: &[MirRuntimeValue],
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    jet_services_authority_validate(endpoint)
        .map_err(|_| unsupported("core.jobs endpoint authority", span))?;
    apply_jobs_runtime_for_endpoint(endpoint, method, args, span)
}

/// Apply a `JobQueue` receiver method. The receiver retains the checked
/// endpoint identity and queue name; every method revalidates that endpoint
/// before opening the selected durable provider.
pub fn apply_jobs_method_mir(
    receiver: &MirRuntimeValue,
    method: &str,
    args: &[MirRuntimeValue],
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    let (endpoint, name) = runtime_to_queue(receiver, span)?;
    jet_services_authority_validate(&endpoint)
        .map_err(|_| unsupported("JobQueue endpoint authority", span))?;
    let mut queue = match queue_open(&endpoint, &name) {
        Ok(queue) => queue,
        Err(error) => return Ok(queue_failed(error)),
    };
    let exact = |expected: usize| {
        if args.len() == expected {
            Ok(())
        } else {
            Err(unsupported(&format!("JobQueue.{method} argument count"), span))
        }
    };
    match method {
        "enqueue" => {
            if !(2..=3).contains(&args.len()) {
                return Err(unsupported("JobQueue.enqueue(Job, payload, key?)", span));
            }
            let job_type = queue_job_type(queue_one(args, 0, "enqueue", span)?, span)?;
            let payload = queue_payload(&job_type, queue_one(args, 1, "enqueue", span)?, span)?;
            let request = queue_request(job_type, payload, args, span)?;
            Ok(queue_result_value(queue.enqueue(request), queue_receipt_to_runtime))
        }
        "delay" => {
            exact(3)?;
            let job_type = queue_job_type(queue_one(args, 0, "delay", span)?, span)?;
            let payload = queue_payload(&job_type, queue_one(args, 1, "delay", span)?, span)?;
            let delay_ms = queue_duration_ms(queue_one(args, 2, "delay", span)?, span)?;
            Ok(queue_result_value(
                queue.enqueue_delayed(job_type, payload, delay_ms, None, None),
                queue_receipt_to_runtime,
            ))
        }
        "receipt" => {
            exact(1)?;
            let id = queue_text(queue_one(args, 0, "receipt", span)?, "JobQueue", "id", span)?;
            Ok(queue_result_value(queue.receipt(&id), queue_receipt_to_runtime))
        }
        "inspect" => {
            exact(2)?;
            let limit = queue_int(queue_one(args, 0, "inspect", span)?, "JobQueue", "limit", span)?
                .try_into()
                .map_err(|_| unsupported("JobQueue.inspect limit", span))?;
            let include_payload =
                queue_bool(queue_one(args, 1, "inspect", span)?, "JobQueue", "include_payload", span)?;
            Ok(queue_result_value(
                queue.inspect(limit, include_payload),
                |records| MirRuntimeValue::List(records.into_iter().map(queue_record_to_runtime).collect()),
            ))
        }
        "events" => {
            exact(1)?;
            let id = queue_text(queue_one(args, 0, "events", span)?, "JobQueue", "id", span)?;
            Ok(queue_result_value(
                queue.events(&id),
                |events| MirRuntimeValue::List(events.into_iter().map(queue_event_to_runtime).collect()),
            ))
        }
        "claim" => {
            exact(2)?;
            let worker = queue_text(queue_one(args, 0, "claim", span)?, "JobQueue", "worker", span)?;
            let limit = queue_int(queue_one(args, 1, "claim", span)?, "JobQueue", "limit", span)?
                .try_into()
                .map_err(|_| unsupported("JobQueue.claim limit", span))?;
            Ok(queue_result_value(
                queue.claim(&worker, limit),
                |claims| MirRuntimeValue::List(claims.into_iter().map(queue_claim_to_runtime).collect()),
            ))
        }
        "heartbeat" => {
            exact(1)?;
            let claim = runtime_to_queue_claim(queue_one(args, 0, "heartbeat", span)?, span)?;
            Ok(queue_result_value(queue.heartbeat(&claim), queue_receipt_to_runtime))
        }
        "acknowledge" => {
            exact(2)?;
            let claim = runtime_to_queue_claim(queue_one(args, 0, "acknowledge", span)?, span)?;
            let result = runtime_to_queue_result(queue_one(args, 1, "acknowledge", span)?, span)?;
            Ok(queue_result_value(
                queue.acknowledge(&claim, result),
                queue_receipt_to_runtime,
            ))
        }
        "fail" => {
            exact(2)?;
            let claim = runtime_to_queue_claim(queue_one(args, 0, "fail", span)?, span)?;
            let error = runtime_to_queue_error(queue_one(args, 1, "fail", span)?, span)?;
            Ok(queue_result_value(queue.fail(&claim, error), queue_receipt_to_runtime))
        }
        "cancel" | "dead_letter" => {
            if !(2..=3).contains(&args.len()) {
                return Err(unsupported(
                    &format!("JobQueue.{method}(id, reason, lease_token?)"),
                    span,
                ));
            }
            let id = queue_text(queue_one(args, 0, method, span)?, "JobQueue", "id", span)?;
            let reason = queue_text(queue_one(args, 1, method, span)?, "JobQueue", "reason", span)?;
            let lease_token = queue_optional_text(args, 2, "JobQueue", "lease_token", span)?;
            let result = if method == "cancel" {
                queue.cancel(&id, reason, lease_token.as_deref())
            } else {
                queue.dead_letter(&id, reason, lease_token.as_deref())
            };
            Ok(queue_result_value(result, queue_receipt_to_runtime))
        }
        "recover_expired" => {
            exact(0)?;
            Ok(queue_result_value(queue.recover_expired(), |_| MirRuntimeValue::Unit))
        }
        "status" => {
            exact(0)?;
            Ok(queue_result_value(queue.status(), queue_status_to_runtime))
        }
        "pause" | "resume" => {
            exact(0)?;
            let result = if method == "pause" {
                queue.pause()
            } else {
                queue.resume()
            };
            Ok(queue_result_value(result, queue_status_to_runtime))
        }
        "wait" => {
            exact(1)?;
            let timeout_ms = queue_timeout_ms(queue_one(args, 0, "wait", span)?, span)?;
            Ok(queue_result_value(queue.wait(timeout_ms), queue_status_to_runtime))
        }
        "prune" => {
            exact(0)?;
            Ok(queue_result_value(queue.prune(), |count| MirRuntimeValue::Int(
                count.try_into().unwrap_or(i64::MAX),
            )))
        }
        _ => Err(unsupported(&format!("`JobQueue.{method}()`"), span)),
    }
}
pub fn apply_jobs_method(
    receiver: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let receiver = ct_to_runtime_value(receiver)?;
    let args = args
        .iter()
        .map(ct_to_runtime_value)
        .collect::<Result<Vec<_>, _>>()?;
    apply_jobs_method_mir(&receiver, method, &args, span).and_then(|value| runtime_to_ct_value(&value))
}

pub fn apply_jobs(
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let args = args
        .iter()
        .map(ct_to_runtime_value)
        .collect::<Result<Vec<_>, _>>()?;
    apply_jobs_runtime(method, &args, span).and_then(|value| runtime_to_ct_value(&value))
}


