//! D-SERVICE1=D / I9: the typed `core.service` slice includes Prelude
//! `Services.rs`; older procedural helpers remain private migration machinery.

use super::Diagnostics::unsupported;
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{CtReport, CtValue, Type};

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
include!("../../../jet-codegen/src/Prelude/CoreLib/Top/ServiceAuthority.rs");
#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../../jet-codegen/src/Prelude/WorkflowWait.rs");
include!("../../../jet-codegen/src/Prelude/CoreLib/Top/Services.rs");

fn restart_to_ct(r: JetServiceRestart) -> CtValue {
    CtValue::Enum {
        type_name: "ServiceRestart".to_string(),
        variant: match r {
            JetServiceRestart::OneForOne => "OneForOne".to_string(),
            JetServiceRestart::OneForAll => "OneForAll".to_string(),
            JetServiceRestart::RestForOne => "RestForOne".to_string(),
        },
        args: Vec::new(),
    }
}

fn ct_to_restart(v: &CtValue, span: Span) -> Result<JetServiceRestart, Diagnostic> {
    match v {
        CtValue::Enum {
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
/// and another in a `CtValue`. `is_valid` is the Prelude's own range check.
fn restart_budget_to_ct(b: &JetServiceRestartBudget) -> CtValue {
    CtValue::Struct {
        type_name: "ServiceRestartBudget".to_string(),
        fields: vec![
            ("max".to_string(), CtValue::Int(b.max)),
            ("per_ms".to_string(), CtValue::Int(b.per_ms)),
        ],
    }
}

fn ct_to_restart_budget(v: &CtValue, span: Span) -> Result<JetServiceRestartBudget, Diagnostic> {
    let CtValue::Struct { type_name, fields } = v else {
        return Err(unsupported("ServiceRestartBudget", span));
    };
    if type_name != "ServiceRestartBudget" {
        return Err(unsupported("ServiceRestartBudget", span));
    }
    let int = |name: &str| match fields.iter().find(|(n, _)| n == name).map(|(_, v)| v) {
        Some(CtValue::Int(n)) => Ok(*n),
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

fn delivery_to_ct(d: JetServiceDelivery) -> CtValue {
    CtValue::Enum {
        type_name: "ServiceDelivery".to_string(),
        variant: match d {
            JetServiceDelivery::AtMostOnce => "AtMostOnce".to_string(),
            JetServiceDelivery::DurableAtLeastOnce => "DurableAtLeastOnce".to_string(),
        },
        args: Vec::new(),
    }
}

fn ct_to_delivery(v: &CtValue, span: Span) -> Result<JetServiceDelivery, Diagnostic> {
    match v {
        CtValue::Enum {
            type_name, variant, ..
        } if type_name == "ServiceDelivery" => match variant.as_str() {
            "AtMostOnce" => Ok(JetServiceDelivery::AtMostOnce),
            "DurableAtLeastOnce" => Ok(JetServiceDelivery::DurableAtLeastOnce),
            _ => Err(unsupported("ServiceDelivery", span)),
        },
        _ => Err(unsupported("ServiceDelivery", span)),
    }
}

fn task_outcome_to_ct(outcome: &JetTaskOutcome) -> CtValue {
    match outcome {
        JetTaskOutcome::Finished => CtValue::Enum {
            type_name: "TaskOutcome".to_string(),
            variant: "Finished".to_string(),
            args: Vec::new(),
        },
        JetTaskOutcome::Panicked(reason) => CtValue::Enum {
            type_name: "TaskOutcome".to_string(),
            variant: "Panicked".to_string(),
            args: vec![(None, CtValue::Str(reason.clone()))],
        },
        JetTaskOutcome::Cancelled => CtValue::Enum {
            type_name: "TaskOutcome".to_string(),
            variant: "Cancelled".to_string(),
            args: Vec::new(),
        },
        JetTaskOutcome::DeadlineBlown => CtValue::Enum {
            type_name: "TaskOutcome".to_string(),
            variant: "DeadlineBlown".to_string(),
            args: Vec::new(),
        },
    }
}

fn ct_to_task_outcome(value: &CtValue, span: Span) -> Result<JetTaskOutcome, Diagnostic> {
    let CtValue::Enum {
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
            CtValue::Str(reason) => JetTaskOutcome::Panicked(reason.clone()),
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

fn task_status_to_ct(status: &JetTaskStatus) -> CtValue {
    CtValue::Enum {
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

fn ct_to_task_status(value: &CtValue, span: Span) -> Result<JetTaskStatus, Diagnostic> {
    match value {
        CtValue::Enum {
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

fn endpoint_to_ct(e: &JetServiceEndpoint) -> CtValue {
    CtValue::Struct {
        type_name: "ServiceEndpoint".to_string(),
        fields: vec![
            ("tree".to_string(), CtValue::Str(e.tree.clone())),
            ("worker".to_string(), CtValue::Str(e.worker.clone())),
            ("generation".to_string(), CtValue::Int(e.generation)),
            ("authority".to_string(), CtValue::Str(e.authority.clone())),
        ],
    }
}

fn ct_to_service_string(
    value: &CtValue,
    max_len: usize,
    label: &str,
    span: Span,
) -> Result<String, Diagnostic> {
    let CtValue::Str(value) = value else {
        return Err(unsupported(label, span));
    };
    if value.len() > max_len || value.chars().any(char::is_control) {
        return Err(unsupported(label, span));
    }
    Ok(value.clone())
}

fn ct_to_endpoint(v: &CtValue, span: Span) -> Result<JetServiceEndpoint, Diagnostic> {
    let CtValue::Struct { type_name, fields } = v else {
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
        CtValue::Str(s) => s.clone(),
        _ => return Err(unsupported("endpoint tree", span)),
    };
    let worker = match field("worker")? {
        CtValue::Str(s) => s.clone(),
        _ => return Err(unsupported("endpoint worker", span)),
    };
    let generation = match field("generation")? {
        CtValue::Int(n) => *n,
        _ => return Err(unsupported("endpoint generation", span)),
    };
    let authority = match field("authority")? {
        CtValue::Str(s) => s.clone(),
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

fn mailbox_to_ct(m: &JetServiceMailbox) -> CtValue {
    // The tree Prelude mutates this mailbox directly, including rollback after
    // a failed receive. Keep that post-call local snapshot here. `ct_to_mailbox`
    // rehydrates from the authority channel before the next call when an
    // endpoint operation changed the queue outside this tree value.
    let messages = m.channel.snapshot();
    CtValue::Struct {
        type_name: "ServiceMailbox".to_string(),
        fields: vec![
            ("endpoint".to_string(), endpoint_to_ct(&m.endpoint)),
            ("capacity".to_string(), CtValue::Int(m.capacity)),
            ("depth".to_string(), CtValue::Int(messages.len() as i64)),
            (
                "messages".to_string(),
                CtValue::List(messages.into_iter().map(CtValue::Str).collect()),
            ),
        ],
    }
}

fn ct_to_mailbox(v: &CtValue, span: Span) -> Result<JetServiceMailbox, Diagnostic> {
    let CtValue::Struct { type_name, fields } = v else {
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
        CtValue::Int(n) => *n,
        _ => return Err(unsupported("mailbox capacity", span)),
    };
    let messages = match field("messages")? {
        CtValue::List(xs) => {
            if xs.len() > MAX_SERVICE_MESSAGES || capacity <= 0 || xs.len() > capacity as usize {
                return Err(unsupported("mailbox message limit", span));
            }
            xs.iter()
                .map(|x| ct_to_service_string(x, MAX_SERVICE_MESSAGE, "mailbox message", span))
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => return Err(unsupported("mailbox messages", span)),
    };
    let depth = match field("depth")? {
        CtValue::Int(n) => *n,
        _ => return Err(unsupported("mailbox depth", span)),
    };
    let endpoint = ct_to_endpoint(field("endpoint")?, span)?;
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

fn worker_to_ct(w: &JetServiceWorker) -> CtValue {
    CtValue::Struct {
        type_name: "ServiceWorker".to_string(),
        fields: vec![
            ("name".to_string(), CtValue::Str(w.name.clone())),
            ("handler".to_string(), CtValue::Str(w.handler.clone())),
            ("endpoint".to_string(), endpoint_to_ct(&w.endpoint)),
            ("mailbox".to_string(), mailbox_to_ct(&w.mailbox)),
            (
                "restarts".to_string(),
                CtValue::List(w.restarts.iter().copied().map(CtValue::Int).collect()),
            ),
            ("running".to_string(), CtValue::Bool(w.running)),
        ],
    }
}

fn ct_to_worker(v: &CtValue, span: Span) -> Result<JetServiceWorker, Diagnostic> {
    let CtValue::Struct { type_name, fields } = v else {
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
        CtValue::Bool(b) => *b,
        _ => return Err(unsupported("worker running state", span)),
    };
    let name = ct_to_service_string(field("name")?, MAX_SERVICE_NAME, "worker name", span)?;
    let handler = match field("handler") {
        Ok(value) => ct_to_service_string(value, MAX_SERVICE_NAME, "worker handler", span)?,
        // Existing internal Core values predate handler metadata. Preserve
        // their real worker identity while the typed surface migrates; this is
        // not a second user-facing declaration form.
        Err(_) => name.clone(),
    };
    let declared_endpoint = ct_to_endpoint(field("endpoint")?, span)?;
    let mailbox = ct_to_mailbox(field("mailbox")?, span)?;
    if declared_endpoint != mailbox.endpoint {
        return Err(unsupported("worker endpoint/mailbox mismatch", span));
    }
    let endpoint = mailbox.endpoint.clone();
    Ok(JetServiceWorker {
        name,
        handler,
        endpoint,
        mailbox,
        restarts: match field("restarts")? {
            CtValue::List(xs) => {
                if xs.len() as i64 > MAX_SERVICE_RESTART_BUDGET {
                    return Err(unsupported("worker restart budget", span));
                }
                xs.iter()
                    .map(|x| match x {
                        CtValue::Int(at) if *at >= 0 => Ok(*at),
                        _ => Err(unsupported("worker restart instant", span)),
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
            _ => return Err(unsupported("worker restarts", span)),
        },
        running,
        task: std::sync::Arc::new(std::sync::Mutex::new(JetServiceSupervisorState::new(
            if running {
                JetServiceSupervisorStatus::Running
            } else {
                JetServiceSupervisorStatus::Stopped
            },
        ))),
    })
}

fn state_adapter_to_ct(a: JetServiceStateAdapter) -> CtValue {
    CtValue::Enum {
        type_name: "ServiceStateAdapter".to_string(),
        variant: match a {
            JetServiceStateAdapter::Empty => "Empty".to_string(),
            JetServiceStateAdapter::Snapshot => "Snapshot".to_string(),
            JetServiceStateAdapter::EventLog => "EventLog".to_string(),
        },
        args: Vec::new(),
    }
}

fn ct_to_state_adapter(v: &CtValue, span: Span) -> Result<JetServiceStateAdapter, Diagnostic> {
    match v {
        CtValue::Enum {
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

fn state_store_to_ct(store: &JetServiceStateStore) -> CtValue {
    CtValue::Struct {
        type_name: "ServiceStateStore".to_string(),
        fields: vec![("path".to_string(), CtValue::Str(store.path.clone()))],
    }
}

fn ct_to_state_store(value: &CtValue, span: Span) -> Result<JetServiceStateStore, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
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
    let path = ct_to_service_string(path, MAX_SERVICE_STATE_STORE, "service state store", span)?;
    jet_services_state_store(path).map_err(|error| unsupported(&error.jet_show(), span))
}

fn state_authority_to_ct(authority: &JetServiceStateAuthority) -> CtValue {
    CtValue::Struct {
        type_name: "ServiceStateAuthority".to_string(),
        fields: vec![
            ("store".to_string(), CtValue::Str(authority.store.clone())),
            ("schema".to_string(), CtValue::Str(authority.schema.clone())),
            ("version".to_string(), CtValue::Int(authority.version)),
            (
                "migration".to_string(),
                CtValue::Str(authority.migration.clone()),
            ),
            (
                "adapter".to_string(),
                state_adapter_to_ct(authority.adapter.clone()),
            ),
        ],
    }
}

fn ct_to_state_authority(
    value: &CtValue,
    span: Span,
) -> Result<JetServiceStateAuthority, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
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
        store: ct_to_service_string(
            field("store")?,
            MAX_SERVICE_STATE_STORE,
            "service state store",
            span,
        )?,
        schema: ct_to_service_string(
            field("schema")?,
            MAX_SERVICE_STATE_SCHEMA,
            "service state schema",
            span,
        )?,
        version: match field("version")? {
            CtValue::Int(version) => *version,
            _ => return Err(unsupported("service state version", span)),
        },
        migration: ct_to_service_string(
            field("migration")?,
            MAX_SERVICE_STATE_SCHEMA,
            "service state migration",
            span,
        )?,
        adapter: ct_to_state_adapter(field("adapter")?, span)?,
    };
    jet_services_attach_state_authority(&candidate, candidate.adapter.clone())
        .map_err(|error| unsupported(&error.jet_show(), span))
}

fn workflow_to_ct(w: &JetServiceWorkflow) -> CtValue {
    CtValue::Struct {
        type_name: "ServiceWorkflow".to_string(),
        fields: vec![
            ("id".to_string(), CtValue::Str(w.id.clone())),
            ("run_id".to_string(), CtValue::Int(w.run_id)),
            ("version".to_string(), CtValue::Int(w.version)),
            (
                "steps".to_string(),
                CtValue::List(w.steps.iter().cloned().map(CtValue::Str).collect()),
            ),
            (
                "history".to_string(),
                CtValue::List(w.history.iter().cloned().map(CtValue::Str).collect()),
            ),
            (
                "replay_cursor".to_string(),
                CtValue::Int(w.replay_cursor as i64),
            ),
            ("status".to_string(), task_status_to_ct(&w.status)),
            (
                "activity_outcomes".to_string(),
                CtValue::List(
                    w.activity_outcomes
                        .iter()
                        .map(|(key, outcome)| CtValue::Struct {
                            type_name: "ServiceActivityOutcome".to_string(),
                            fields: vec![
                                ("key".to_string(), CtValue::Str(key.clone())),
                                ("outcome".to_string(), task_outcome_to_ct(outcome)),
                            ],
                        })
                        .collect(),
                ),
            ),
            (
                "outcome".to_string(),
                w.outcome.as_ref().map_or_else(
                    || CtValue::absent(Type::Named("TaskOutcome".to_string())),
                    |outcome| CtValue::Present(Box::new(task_outcome_to_ct(outcome))),
                ),
            ),
        ],
    }
}

fn workflow_handle_to_ct(handle: &JetWorkflowHandle, span: Span) -> Result<CtValue, Diagnostic> {
    let workflow = handle
        .state
        .lock()
        .map_err(|_| unsupported("workflow handle state", span))?
        .clone();
    let CtValue::Struct {
        type_name,
        mut fields,
    } = workflow_to_ct(&workflow)
    else {
        return Err(unsupported("workflow handle state", span));
    };
    fields.push((
        "authority".to_string(),
        state_authority_to_ct(&handle.authority),
    ));
    Ok(CtValue::Struct { type_name, fields })
}

fn ct_to_workflow_handle(value: &CtValue, span: Span) -> Result<JetWorkflowHandle, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
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
        .and_then(|value| ct_to_state_authority(value, span))?;
    let workflow = ct_to_workflow(
        &CtValue::Struct {
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

fn ct_to_workflow_run_id(value: &CtValue, span: Span) -> Result<i64, Diagnostic> {
    match value {
        CtValue::Int(run_id) => Ok(*run_id),
        CtValue::Struct { type_name, fields } if type_name == "ServiceWorkflow" => fields
            .iter()
            .find(|(name, _)| name == "run_id")
            .and_then(|(_, value)| match value {
                CtValue::Int(run_id) => Some(*run_id),
                _ => None,
            })
            .ok_or_else(|| unsupported("workflow run id", span)),
        _ => Err(unsupported("workflow run id", span)),
    }
}

fn ct_to_workflow(v: &CtValue, span: Span) -> Result<JetServiceWorkflow, Diagnostic> {
    let CtValue::Struct { type_name, fields } = v else {
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
            CtValue::List(xs) => {
                if xs.len() > MAX_SERVICE_WORKFLOW_STEPS {
                    return Err(unsupported("workflow string limit", span));
                }
                xs.iter()
                    .map(|x| ct_to_service_string(x, MAX_SERVICE_MESSAGE, "workflow string", span))
                    .collect()
            }
            _ => Err(unsupported("workflow string list", span)),
        }
    };
    Ok(JetServiceWorkflow {
        id: ct_to_service_string(field("id")?, MAX_SERVICE_NAME, "workflow id", span)?,
        run_id: match field("run_id")? {
            CtValue::Int(n) => *n,
            _ => return Err(unsupported("workflow run id", span)),
        },
        version: match field("version")? {
            CtValue::Int(n) => *n,
            _ => return Err(unsupported("workflow version", span)),
        },
        steps: str_list("steps")?,
        history: str_list("history")?,
        replay_cursor: match field("replay_cursor")? {
            CtValue::Int(n) if *n >= 0 => {
                usize::try_from(*n).map_err(|_| unsupported("workflow replay cursor", span))?
            }
            _ => return Err(unsupported("workflow replay cursor", span)),
        },
        status: ct_to_task_status(field("status")?, span)?,
        activity_outcomes: match field("activity_outcomes")? {
            CtValue::List(entries) => entries
                .iter()
                .map(|entry| {
                    let CtValue::Struct { type_name, fields } = entry else {
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
                            ct_to_service_string(
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
                        .and_then(|value| ct_to_task_outcome(value, span))?;
                    Ok((key, outcome))
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err(unsupported("activity outcomes", span)),
        },
        outcome: match field("outcome")? {
            CtValue::Failed(CtReport::Clean(_)) => None,
            CtValue::Present(value) => Some(ct_to_task_outcome(value, span)?),
            _ => return Err(unsupported("workflow outcome", span)),
        },
    })
}

fn upgrade_receipt_to_ct(receipt: &JetServiceUpgradeReceipt) -> CtValue {
    CtValue::Struct {
        type_name: "ServiceUpgradeReceipt".to_string(),
        fields: vec![
            (
                "from_generation".to_string(),
                CtValue::Int(receipt.from_generation),
            ),
            (
                "to_generation".to_string(),
                CtValue::Int(receipt.to_generation),
            ),
            (
                "migration".to_string(),
                CtValue::Str(receipt.migration.clone()),
            ),
            (
                "rollback_store".to_string(),
                CtValue::Str(receipt.rollback_store.clone()),
            ),
            (
                "rollback_available".to_string(),
                CtValue::Bool(receipt.rollback_available),
            ),
            (
                "pinned_shards".to_string(),
                CtValue::List(
                    receipt
                        .pinned_shards
                        .iter()
                        .cloned()
                        .map(CtValue::Str)
                        .collect(),
                ),
            ),
        ],
    }
}

fn ct_to_upgrade_receipt(v: &CtValue, span: Span) -> Result<JetServiceUpgradeReceipt, Diagnostic> {
    let CtValue::Struct { type_name, fields } = v else {
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
            CtValue::Int(n) => *n,
            _ => return Err(unsupported("upgrade from generation", span)),
        },
        to_generation: match field("to_generation")? {
            CtValue::Int(n) => *n,
            _ => return Err(unsupported("upgrade to generation", span)),
        },
        migration: ct_to_service_string(field("migration")?, 32, "upgrade migration", span)?,
        rollback_store: ct_to_service_string(
            field("rollback_store")?,
            MAX_SERVICE_STATE_STORE,
            "upgrade rollback store",
            span,
        )?,
        rollback_available: match field("rollback_available")? {
            CtValue::Bool(v) => *v,
            _ => return Err(unsupported("upgrade rollback availability", span)),
        },
        pinned_shards: ct_str_list(field("pinned_shards")?, span)?,
    })
}

fn tree_to_ct(tree: &JetServiceTree) -> CtValue {
    CtValue::Struct {
        type_name: "ServiceTree".to_string(),
        fields: vec![
            ("name".to_string(), CtValue::Str(tree.name.clone())),
            (
                "authority".to_string(),
                CtValue::Str(tree.authority.clone()),
            ),
            ("generation".to_string(), CtValue::Int(tree.generation)),
            (
                "delivery".to_string(),
                delivery_to_ct(tree.delivery.clone()),
            ),
            ("restart".to_string(), restart_to_ct(tree.restart.clone())),
            (
                "restart_budget".to_string(),
                restart_budget_to_ct(&tree.restart_budget),
            ),
            (
                "workers".to_string(),
                CtValue::List(tree.workers.iter().map(worker_to_ct).collect()),
            ),
            (
                "groups".to_string(),
                CtValue::List(
                    tree.groups
                        .iter()
                        .map(|g| CtValue::Struct {
                            type_name: "ServiceGroup".to_string(),
                            fields: vec![
                                ("name".to_string(), CtValue::Str(g.name.clone())),
                                ("restart".to_string(), restart_to_ct(g.restart.clone())),
                                ("budget".to_string(), restart_budget_to_ct(&g.budget)),
                                (
                                    "workers".to_string(),
                                    CtValue::List(
                                        g.workers.iter().cloned().map(CtValue::Str).collect(),
                                    ),
                                ),
                            ],
                        })
                        .collect(),
                ),
            ),
            ("started".to_string(), CtValue::Bool(tree.started)),
            (
                "state_adapter".to_string(),
                state_adapter_to_ct(tree.state_adapter.clone()),
            ),
            (
                "state_authority".to_string(),
                tree.state_authority.as_ref().map_or_else(
                    || CtValue::absent(Type::Named("ServiceStateAuthority".to_string())),
                    |authority| CtValue::Present(Box::new(state_authority_to_ct(authority))),
                ),
            ),
            (
                "snapshot".to_string(),
                match &tree.snapshot {
                    Some(s) => CtValue::Str(s.clone()),
                    None => CtValue::Str(String::new()),
                },
            ),
            (
                "event_log".to_string(),
                CtValue::List(tree.event_log.iter().cloned().map(CtValue::Str).collect()),
            ),
            (
                "dead_letters".to_string(),
                CtValue::List(
                    tree.dead_letters
                        .iter()
                        .cloned()
                        .map(CtValue::Str)
                        .collect(),
                ),
            ),
            (
                "idempotency_seen".to_string(),
                CtValue::List(
                    tree.idempotency_seen
                        .iter()
                        .map(|(key, endpoint, message)| CtValue::Struct {
                            type_name: "ServiceIdempotencyEntry".to_string(),
                            fields: vec![
                                ("key".to_string(), CtValue::Str(key.clone())),
                                ("endpoint".to_string(), endpoint_to_ct(endpoint)),
                                ("message".to_string(), CtValue::Str(message.clone())),
                            ],
                        })
                        .collect(),
                ),
            ),
            (
                "directory".to_string(),
                CtValue::List(
                    tree.directory
                        .iter()
                        .map(|(n, ep, signature)| CtValue::Struct {
                            type_name: "ServiceDirectoryEntry".to_string(),
                            fields: vec![
                                ("name".to_string(), CtValue::Str(n.clone())),
                                ("endpoint".to_string(), endpoint_to_ct(ep)),
                                ("signature".to_string(), CtValue::Str(signature.clone())),
                            ],
                        })
                        .collect(),
                ),
            ),
            (
                "draining".to_string(),
                CtValue::List(tree.draining.iter().cloned().map(CtValue::Str).collect()),
            ),
            (
                "partitioned".to_string(),
                CtValue::List(tree.partitioned.iter().cloned().map(CtValue::Str).collect()),
            ),
            (
                "workflows".to_string(),
                CtValue::List(tree.workflows.iter().map(workflow_to_ct).collect()),
            ),
            ("chaos_fails".to_string(), CtValue::Int(tree.chaos_fails)),
            (
                "previous_generation".to_string(),
                CtValue::Int(tree.previous_generation),
            ),
            (
                "last_upgrade".to_string(),
                tree.last_upgrade.as_ref().map_or_else(
                    || CtValue::absent(Type::Named("ServiceUpgradeReceipt".to_string())),
                    |receipt| CtValue::Present(Box::new(upgrade_receipt_to_ct(receipt))),
                ),
            ),
        ],
    }
}

fn ct_str_list(v: &CtValue, span: Span) -> Result<Vec<String>, Diagnostic> {
    match v {
        CtValue::List(xs) => {
            if xs.len() > MAX_SERVICE_STATE_RECORDS {
                return Err(unsupported("string list length", span));
            }
            xs.iter()
                .map(|x| match x {
                    CtValue::Str(s)
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

fn ct_to_tree(v: &CtValue, span: Span) -> Result<JetServiceTree, Diagnostic> {
    let CtValue::Struct { type_name, fields } = v else {
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
        CtValue::List(xs) => {
            if xs.len() > MAX_SERVICE_WORKERS {
                return Err(unsupported("tree worker limit", span));
            }
            xs.iter()
                .map(|x| ct_to_worker(x, span))
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => return Err(unsupported("tree workers", span)),
    };
    let groups = match field("groups")? {
        CtValue::List(xs) => {
            if xs.len() > MAX_SERVICE_WORKERS {
                return Err(unsupported("tree group limit", span));
            }
            xs.iter()
                .map(|x| {
                    let CtValue::Struct { type_name, fields } = x else {
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
                            ct_to_service_string(value, MAX_SERVICE_NAME, "group name", span)
                        })?;
                    let restart = fields
                        .iter()
                        .find(|(n, _)| n == "restart")
                        .map(|(_, v)| ct_to_restart(v, span))
                        .ok_or_else(|| unsupported("group restart", span))??;
                    let budget = fields
                        .iter()
                        .find(|(n, _)| n == "budget")
                        .map(|(_, v)| ct_to_restart_budget(v, span))
                        .ok_or_else(|| unsupported("group restart budget", span))??;
                    let workers = match fields.iter().find(|(n, _)| n == "workers") {
                        Some((_, CtValue::List(xs))) => {
                            if xs.len() > MAX_SERVICE_WORKERS {
                                return Err(unsupported("group worker limit", span));
                            }
                            xs.iter()
                                .map(|x| {
                                    ct_to_service_string(x, MAX_SERVICE_NAME, "group worker", span)
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
        CtValue::List(xs) => {
            if xs.len() > MAX_SERVICE_WORKERS {
                return Err(unsupported("service directory limit", span));
            }
            xs.iter()
                .map(|x| {
                    let CtValue::Struct { type_name, fields } = x else {
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
                            ct_to_service_string(value, MAX_SERVICE_NAME, "directory name", span)
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
                            ct_to_service_string(value, 64, "directory signature", span)
                        })?;
                    Ok((name, ct_to_endpoint(&endpoint.1, span)?, signature))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => return Err(unsupported("service directory", span)),
    };
    let workflows = match field("workflows")? {
        CtValue::List(xs) => {
            if xs.len() > MAX_SERVICE_WORKFLOW_STEPS {
                return Err(unsupported("service workflow limit", span));
            }
            xs.iter()
                .map(|x| ct_to_workflow(x, span))
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => return Err(unsupported("service workflows", span)),
    };
    let snapshot = match field("snapshot")? {
        value => {
            let value = ct_to_service_string(value, MAX_SERVICE_MESSAGE, "service snapshot", span)?;
            (!value.is_empty()).then_some(value)
        }
    };
    let state_authority = match fields.iter().find(|(name, _)| name == "state_authority") {
        None | Some((_, CtValue::Failed(CtReport::Clean(_)))) => None,
        Some((_, CtValue::Present(value))) => Some(ct_to_state_authority(value, span)?),
        Some(_) => return Err(unsupported("service state authority", span)),
    };
    let last_upgrade = match fields.iter().find(|(name, _)| name == "last_upgrade") {
        None | Some((_, CtValue::Failed(CtReport::Clean(_)))) => None,
        Some((_, CtValue::Present(value))) => Some(ct_to_upgrade_receipt(value, span)?),
        Some(_) => return Err(unsupported("service upgrade receipt", span)),
    };
    let partitioned = match fields.iter().find(|(name, _)| name == "partitioned") {
        Some((_, value)) => ct_str_list(value, span)?,
        None => Vec::new(),
    };
    let idempotency_seen = match field("idempotency_seen")? {
        CtValue::List(entries) => {
            if entries.len() > MAX_SERVICE_IDEMPOTENCY {
                return Err(unsupported("service idempotency limit", span));
            }
            entries
                .iter()
                .map(|entry| {
                    let CtValue::Struct { type_name, fields } = entry else {
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
                    let key = ct_to_service_string(
                        field("key")?,
                        MAX_SERVICE_NAME,
                        "idempotency key",
                        span,
                    )?;
                    let endpoint = ct_to_endpoint(field("endpoint")?, span)?;
                    let message = ct_to_service_string(
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
        name: ct_to_service_string(field("name")?, MAX_SERVICE_NAME, "tree name", span)?,
        authority: ct_to_service_string(
            field("authority")?,
            MAX_SERVICE_NAME,
            "tree authority",
            span,
        )?,
        generation: match field("generation")? {
            CtValue::Int(n) => *n,
            _ => return Err(unsupported("tree generation", span)),
        },
        delivery: ct_to_delivery(field("delivery")?, span)?,
        restart: ct_to_restart(field("restart")?, span)?,
        restart_budget: ct_to_restart_budget(field("restart_budget")?, span)?,
        workers,
        groups,
        started: match field("started")? {
            CtValue::Bool(b) => *b,
            _ => return Err(unsupported("tree started state", span)),
        },
        state_adapter: ct_to_state_adapter(field("state_adapter")?, span)?,
        state_authority,
        snapshot,
        event_log: ct_str_list(field("event_log")?, span)?,
        dead_letters: ct_str_list(field("dead_letters")?, span)?,
        idempotency_seen,
        directory,
        draining: ct_str_list(field("draining")?, span)?,
        partitioned,
        workflows,
        task_group: std::sync::Arc::new(JetTaskGroupRuntime::new()),
        supervisor_tasks: Vec::new(),
        chaos_fails: match field("chaos_fails")? {
            CtValue::Int(n) => *n,
            _ => return Err(unsupported("service chaos counter", span)),
        },
        previous_generation: match field("previous_generation")? {
            CtValue::Int(n) => *n,
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

fn map_err(err: JetServiceError) -> CtValue {
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
    CtValue::Enum {
        type_name: "ServiceError".to_string(),
        variant: variant.to_string(),
        args: vec![(None, CtValue::Str(message))],
    }
}

fn delivery_state_to_ct(state: JetDeliveryState) -> CtValue {
    CtValue::Enum {
        type_name: "DeliveryState".to_string(),
        variant: state.jet_show(),
        args: Vec::new(),
    }
}

fn delivery_record_to_ct(delivery: JetDelivery) -> CtValue {
    CtValue::Struct {
        type_name: "Delivery".to_string(),
        fields: vec![
            ("id".to_string(), CtValue::Str(delivery.id)),
            ("store".to_string(), CtValue::Str(delivery.store)),
            ("duplicate".to_string(), CtValue::Bool(delivery.duplicate)),
            ("authority".to_string(), CtValue::Str(delivery.authority)),
            ("generation".to_string(), CtValue::Int(delivery.generation)),
        ],
    }
}

fn ct_to_delivery_record(value: &CtValue, span: Span) -> Result<JetDelivery, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
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
        CtValue::Str(value) => value.clone(),
        _ => return Err(unsupported("Delivery.id", span)),
    };
    let store = match field("store")? {
        CtValue::Str(value) => value.clone(),
        _ => return Err(unsupported("Delivery.store", span)),
    };
    let duplicate = match field("duplicate")? {
        CtValue::Bool(value) => *value,
        _ => return Err(unsupported("Delivery.duplicate", span)),
    };
    let authority = match field("authority")? {
        CtValue::Str(value) => value.clone(),
        _ => return Err(unsupported("Delivery.authority", span)),
    };
    let generation = match field("generation")? {
        CtValue::Int(value) => *value,
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

fn delivery_receipt_to_ct(receipt: JetDeliveryReceipt) -> CtValue {
    CtValue::Struct {
        type_name: "DeliveryReceipt".to_string(),
        fields: vec![
            ("id".to_string(), CtValue::Str(receipt.id)),
            ("state".to_string(), delivery_state_to_ct(receipt.state)),
            ("attempts".to_string(), CtValue::Int(receipt.attempts)),
            ("retention_until".to_string(), CtValue::Int(receipt.retention_until)),
            ("deadline".to_string(), CtValue::Int(receipt.deadline)),
            ("idempotency_key".to_string(), CtValue::Str(receipt.idempotency_key)),
            ("duplicate".to_string(), CtValue::Bool(receipt.duplicate)),
            ("authority".to_string(), CtValue::Str(receipt.authority)),
            ("generation".to_string(), CtValue::Int(receipt.generation)),
            ("signature".to_string(), CtValue::Str(receipt.signature)),
        ],
    }
}

fn delivery_event_to_ct(event: JetDeliveryEvent) -> CtValue {
    CtValue::Struct {
        type_name: "DeliveryEvent".to_string(),
        fields: vec![
            ("sequence".to_string(), CtValue::Int(event.sequence)),
            ("state".to_string(), delivery_state_to_ct(event.state)),
            ("attempts".to_string(), CtValue::Int(event.attempts)),
            ("timestamp".to_string(), CtValue::Int(event.timestamp)),
            ("signature".to_string(), CtValue::Str(event.signature)),
        ],
    }
}

fn service_ct_field<'a>(
    fields: &'a [(String, CtValue)],
    type_name: &str,
    name: &str,
    span: Span,
) -> Result<&'a CtValue, Diagnostic> {
    fields
        .iter()
        .find_map(|(field, value)| (field == name).then_some(value))
        .ok_or_else(|| unsupported(&format!("{type_name}.{name}"), span))
}

fn service_ct_text(
    fields: &[(String, CtValue)],
    type_name: &str,
    name: &str,
    span: Span,
) -> Result<String, Diagnostic> {
    match service_ct_field(fields, type_name, name, span)? {
        CtValue::Str(value) => Ok(value.clone()),
        _ => Err(unsupported(&format!("{type_name}.{name}"), span)),
    }
}

fn service_ct_int(
    fields: &[(String, CtValue)],
    type_name: &str,
    name: &str,
    span: Span,
) -> Result<i64, Diagnostic> {
    match service_ct_field(fields, type_name, name, span)? {
        CtValue::Int(value) => Ok(*value),
        _ => Err(unsupported(&format!("{type_name}.{name}"), span)),
    }
}

fn service_ct_bool(
    fields: &[(String, CtValue)],
    type_name: &str,
    name: &str,
    span: Span,
) -> Result<bool, Diagnostic> {
    match service_ct_field(fields, type_name, name, span)? {
        CtValue::Bool(value) => Ok(*value),
        _ => Err(unsupported(&format!("{type_name}.{name}"), span)),
    }
}

fn ct_to_delivery_state(value: &CtValue, span: Span) -> Result<JetDeliveryState, Diagnostic> {
    let CtValue::Enum {
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

fn ct_to_delivery_receipt(
    value: &CtValue,
    span: Span,
) -> Result<JetDeliveryReceipt, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("DeliveryReceipt", span));
    };
    if type_name != "DeliveryReceipt" {
        return Err(unsupported("DeliveryReceipt", span));
    }
    Ok(JetDeliveryReceipt {
        id: service_ct_text(fields, "DeliveryReceipt", "id", span)?,
        state: ct_to_delivery_state(service_ct_field(
            fields,
            "DeliveryReceipt",
            "state",
            span,
        )?, span)?,
        attempts: service_ct_int(fields, "DeliveryReceipt", "attempts", span)?,
        retention_until: service_ct_int(fields, "DeliveryReceipt", "retention_until", span)?,
        deadline: service_ct_int(fields, "DeliveryReceipt", "deadline", span)?,
        idempotency_key: service_ct_text(fields, "DeliveryReceipt", "idempotency_key", span)?,
        duplicate: service_ct_bool(fields, "DeliveryReceipt", "duplicate", span)?,
        authority: service_ct_text(fields, "DeliveryReceipt", "authority", span)?,
        generation: service_ct_int(fields, "DeliveryReceipt", "generation", span)?,
        signature: service_ct_text(fields, "DeliveryReceipt", "signature", span)?,
    })
}

fn ct_to_delivery_event(value: &CtValue, span: Span) -> Result<JetDeliveryEvent, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("DeliveryEvent", span));
    };
    if type_name != "DeliveryEvent" {
        return Err(unsupported("DeliveryEvent", span));
    }
    Ok(JetDeliveryEvent {
        sequence: service_ct_int(fields, "DeliveryEvent", "sequence", span)?,
        state: ct_to_delivery_state(service_ct_field(
            fields,
            "DeliveryEvent",
            "state",
            span,
        )?, span)?,
        attempts: service_ct_int(fields, "DeliveryEvent", "attempts", span)?,
        timestamp: service_ct_int(fields, "DeliveryEvent", "timestamp", span)?,
        signature: service_ct_text(fields, "DeliveryEvent", "signature", span)?,
    })
}

fn ct_to_service_error(value: &CtValue, span: Span) -> Result<JetServiceError, Diagnostic> {
    let CtValue::Enum {
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
    let CtValue::Str(message) = &args[0].1 else {
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
pub fn service_display_value(value: &CtValue) -> Option<String> {
    let span = Span::new(0, 0);
    match value {
        CtValue::Enum { type_name, .. } if type_name == "DeliveryState" => {
            Some(ct_to_delivery_state(value, span).ok()?.jet_show())
        }
        CtValue::Enum { type_name, .. } if type_name == "ServiceError" => {
            Some(ct_to_service_error(value, span).ok()?.jet_show())
        }
        CtValue::Struct { type_name, .. } if type_name == "Delivery" => {
            Some(ct_to_delivery_record(value, span).ok()?.jet_show())
        }
        CtValue::Struct { type_name, .. } if type_name == "DeliveryReceipt" => {
            Some(ct_to_delivery_receipt(value, span).ok()?.jet_show())
        }
        CtValue::Struct { type_name, .. } if type_name == "DeliveryEvent" => {
            Some(ct_to_delivery_event(value, span).ok()?.jet_show())
        }
        CtValue::Struct { type_name, .. } if type_name == "ServiceEndpoint" => {
            Some(ct_to_endpoint(value, span).ok()?.jet_show())
        }
        CtValue::Struct { type_name, .. } if type_name == "ServiceRuntime" => {
            Some(ct_to_runtime(value, span).ok()?.jet_show())
        }
        CtValue::Struct { type_name, .. } if type_name == "ServiceStateStore" => {
            Some(ct_to_state_store(value, span).ok()?.jet_show())
        }
        _ => None,
    }
}

fn ct_to_runtime(value: &CtValue, span: Span) -> Result<JetServiceRuntime, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
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
            ct_to_service_string(value, SERVICE_AUTH_MAX_STORE, "ServiceRuntime.store", span)
        })?;
    let retention_ms = fields
        .iter()
        .find_map(|(name, value)| (name == "retention_ms").then_some(value))
        .ok_or_else(|| unsupported("ServiceRuntime.retention_ms", span))
        .and_then(|value| match value {
            CtValue::Int(value) => Ok(*value),
            _ => Err(unsupported("ServiceRuntime.retention_ms", span)),
        })?;
    Ok(jet_services_runtime(store, retention_ms))
}

fn runtime_to_ct(runtime: &JetServiceRuntime) -> CtValue {
    CtValue::Struct {
        type_name: "ServiceRuntime".to_string(),
        fields: vec![
            ("store".to_string(), CtValue::Str(runtime.store.clone())),
            (
                "retention_ms".to_string(),
                CtValue::Int(runtime.retention_ms),
            ),
        ],
    }
}

/// Apply a `ServiceRuntime` handle method through the same Prelude authority
/// functions used by AOT and the runtime ambient adapter. The TIR evaluator is
/// only responsible for CtValue conversion at this boundary.
pub fn apply_runtime_method(
    receiver: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let runtime = ct_to_runtime(receiver, span)?;
    let one = |index: usize| {
        args.get(index)
            .ok_or_else(|| unsupported(&format!("ServiceRuntime.{method} arg {index}"), span))
    };
    match method {
        "send" => {
            let endpoint = ct_to_endpoint(one(0)?, span)?;
            let message = ct_to_service_string(
                one(1)?,
                SERVICE_AUTH_MAX_MESSAGE,
                "ServiceRuntime.send message",
                span,
            )?;
            let key = ct_to_service_string(
                one(2)?,
                SERVICE_AUTH_MAX_KEY,
                "ServiceRuntime.send key",
                span,
            )?;
            Ok(
                match jet_services_runtime_send(&runtime, &endpoint, &message, &key) {
                    Ok(delivery) => CtValue::Present(Box::new(delivery_record_to_ct(delivery))),
                    Err(error) => CtValue::failed(Box::new(map_err(error))),
                },
            )
        }
        "retry" | "dead_letter" | "retain" => {
            let delivery = ct_to_delivery_record(one(0)?, span)?;
            let result = match method {
                "retry" => jet_services_runtime_retry(&runtime, &delivery),
                "dead_letter" => jet_services_runtime_dead_letter(&runtime, &delivery),
                "retain" => jet_services_runtime_retain(&runtime, &delivery),
                _ => unreachable!(),
            };
            Ok(match result {
                Ok(delivery) => CtValue::Present(Box::new(delivery_record_to_ct(delivery))),
                Err(error) => CtValue::failed(Box::new(map_err(error))),
            })
        }
        "commit" => {
            let delivery = ct_to_delivery_record(one(0)?, span)?;
            Ok(match jet_services_runtime_commit(&runtime, &delivery) {
                Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
                Err(error) => CtValue::failed(Box::new(map_err(error))),
            })
        }
        _ => Err(unsupported(&format!("ServiceRuntime.{method}"), span)),
    }
}

fn mutate_ok(tree: JetServiceTree, value: CtValue) -> CtValue {
    CtValue::Struct {
        type_name: "__JetServiceMut".to_string(),
        fields: vec![
            ("tree".to_string(), tree_to_ct(&tree)),
            ("value".to_string(), value),
        ],
    }
}

fn mutate_err(tree: JetServiceTree, error: CtValue) -> CtValue {
    CtValue::failed(Box::new(CtValue::Struct {
        type_name: "__JetServiceMutErr".to_string(),
        fields: vec![
            ("tree".to_string(), tree_to_ct(&tree)),
            ("error".to_string(), error),
        ],
    }))
}

fn mutate_handle_ok(
    handle: &JetWorkflowHandle,
    value: CtValue,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    Ok(CtValue::Struct {
        type_name: "__JetServiceMut".to_string(),
        fields: vec![
            ("tree".to_string(), workflow_handle_to_ct(handle, span)?),
            ("value".to_string(), value),
        ],
    })
}

fn mutate_handle_err(
    handle: &JetWorkflowHandle,
    error: CtValue,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    Ok(CtValue::failed(Box::new(CtValue::Struct {
        type_name: "__JetServiceMutErr".to_string(),
        fields: vec![
            ("tree".to_string(), workflow_handle_to_ct(handle, span)?),
            ("error".to_string(), error),
        ],
    })))
}

pub fn take_mut_ok(value: CtValue) -> Result<(CtValue, CtValue), CtValue> {
    match value {
        CtValue::Present(inner) => match *inner {
            CtValue::Struct { type_name, fields } if type_name == "__JetServiceMut" => {
                let tree = fields
                    .iter()
                    .find(|(n, _)| n == "tree")
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| {
                        CtValue::failed(Box::new(CtValue::Str(
                            "core.services: missing tree write-back".to_string(),
                        )))
                    })?;
                let val = fields
                    .iter()
                    .find(|(n, _)| n == "value")
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| {
                        CtValue::failed(Box::new(CtValue::Str(
                            "core.services: missing value write-back".to_string(),
                        )))
                    })?;
                Ok((tree, CtValue::Present(Box::new(val))))
            }
            other => Ok((CtValue::Unit, CtValue::Present(Box::new(other)))),
        },
        CtValue::Failed(CtReport::Told(inner)) => match *inner {
            CtValue::Struct { type_name, fields } if type_name == "__JetServiceMutErr" => {
                let tree = fields
                    .iter()
                    .find(|(n, _)| n == "tree")
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| {
                        CtValue::failed(Box::new(CtValue::Str(
                            "core.services: missing tree write-back".to_string(),
                        )))
                    })?;
                let error = fields
                    .iter()
                    .find(|(n, _)| n == "error")
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| {
                        CtValue::failed(Box::new(CtValue::Str(
                            "core.services: missing error write-back".to_string(),
                        )))
                    })?;
                Ok((tree, CtValue::failed(Box::new(error))))
            }
            other => Err(CtValue::failed(Box::new(other))),
        },
        other => Err(other),
    }
}

pub fn apply(method: &str, args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let one = |i: usize| {
        args.get(i)
            .ok_or_else(|| unsupported(&format!("core.services.{method} arg {i}"), span))
    };
    match method {
        "tree" => {
            let name = ct_to_service_string(one(0)?, MAX_SERVICE_NAME, "tree name", span)?;
            Ok(tree_to_ct(&jet_services_tree(name)))
        }
        "runtime" => {
            let store = ct_to_service_string(
                one(0)?,
                SERVICE_AUTH_MAX_STORE,
                "ServiceRuntime.store",
                span,
            )?;
            let retention_ms = match one(1)? {
                CtValue::Struct { type_name, fields } if type_name == "Duration" => fields
                    .iter()
                    .find_map(|(name, value)| match (name.as_str(), value) {
                        ("ns", CtValue::Int(ns)) => Some(*ns / 1_000_000),
                        _ => None,
                    })
                    .ok_or_else(|| unsupported("ServiceRuntime.retention_ms", span))?,
                _ => return Err(unsupported("ServiceRuntime.retention_ms", span)),
            };
            Ok(runtime_to_ct(&jet_services_runtime(store, retention_ms)))
        }
        "restart_one_for_one" => Ok(restart_to_ct(jet_services_restart_one_for_one())),
        "restart_one_for_all" => Ok(restart_to_ct(jet_services_restart_one_for_all())),
        "restart_rest_for_one" => Ok(restart_to_ct(jet_services_restart_rest_for_one())),
        "delivery_at_most_once" => Ok(delivery_to_ct(jet_services_delivery_at_most_once())),
        "delivery_durable" => Ok(delivery_to_ct(jet_services_delivery_durable())),
        "state_store" => {
            let path = ct_to_service_string(
                one(0)?,
                MAX_SERVICE_STATE_STORE,
                "service state store",
                span,
            )?;
            Ok(match jet_services_state_store(path) {
                Ok(store) => CtValue::Present(Box::new(state_store_to_ct(&store))),
                Err(error) => CtValue::failed(Box::new(map_err(error))),
            })
        }
        "set_restart" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let restart = ct_to_restart(one(1)?, span)?;
            Ok(match jet_services_set_restart(&mut tree, restart) {
                Ok(()) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "set_delivery" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let delivery = ct_to_delivery(one(1)?, span)?;
            Ok(match jet_services_set_delivery(&mut tree, delivery) {
                Ok(()) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "worker" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let name = match one(1)? {
                value => ct_to_service_string(value, MAX_SERVICE_NAME, "worker name", span)?,
            };
            let handler = match one(2)? {
                value => ct_to_service_string(value, MAX_SERVICE_NAME, "worker handler", span)?,
            };
            let capacity = match one(3)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("capacity", span)),
            };
            Ok(
                match jet_services_worker(&mut tree, name, handler, capacity) {
                    Ok(ep) => CtValue::Present(Box::new(mutate_ok(tree, endpoint_to_ct(&ep)))),
                    Err(e) => mutate_err(tree, map_err(e)),
                },
            )
        }
        "group" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let name = match one(1)? {
                value => ct_to_service_string(value, MAX_SERVICE_NAME, "group name", span)?,
            };
            let workers = match one(2)? {
                CtValue::List(xs) => {
                    if xs.len() > MAX_SERVICE_WORKERS {
                        return Err(unsupported("group worker limit", span));
                    }
                    xs.iter()
                        .map(|x| ct_to_service_string(x, MAX_SERVICE_NAME, "group worker", span))
                        .collect::<Result<Vec<_>, _>>()?
                }
                _ => return Err(unsupported("group workers", span)),
            };
            Ok(match jet_services_group(&mut tree, name, workers) {
                Ok(()) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "start" | "stop" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let result = if method == "start" {
                jet_services_start(&mut tree)
            } else {
                jet_services_stop(&mut tree)
            };
            Ok(match result {
                Ok(()) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "send" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let endpoint = ct_to_endpoint(one(1)?, span)?;
            let message = match one(2)? {
                value => ct_to_service_string(value, MAX_SERVICE_MESSAGE, "message", span)?,
            };
            Ok(match jet_services_send(&mut tree, &endpoint, message) {
                Ok(()) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "receive" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let endpoint = ct_to_endpoint(one(1)?, span)?;
            Ok(match jet_services_receive(&mut tree, &endpoint) {
                Ok(msg) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Str(msg)))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "endpoint_send" => {
            let endpoint = ct_to_endpoint(one(0)?, span)?;
            let message = ct_to_service_string(one(1)?, MAX_SERVICE_MESSAGE, "message", span)?;
            Ok(match jet_services_endpoint_send(&endpoint, message) {
                Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
                Err(error) => CtValue::failed(Box::new(map_err(error))),
            })
        }
        "endpoint_receive" => {
            let endpoint = ct_to_endpoint(one(0)?, span)?;
            Ok(match jet_services_endpoint_receive(&endpoint) {
                Ok(message) => CtValue::Present(Box::new(CtValue::Str(message))),
                Err(error) => CtValue::failed(Box::new(map_err(error))),
            })
        }
        "mailbox_depth" => {
            let tree = ct_to_tree(one(0)?, span)?;
            let endpoint = ct_to_endpoint(one(1)?, span)?;
            Ok(match jet_services_mailbox_depth(&tree, &endpoint) {
                Ok(n) => CtValue::Present(Box::new(CtValue::Int(n))),
                Err(e) => CtValue::failed(Box::new(map_err(e))),
            })
        }
        "restarts" => {
            let tree = ct_to_tree(one(0)?, span)?;
            let endpoint = ct_to_endpoint(one(1)?, span)?;
            Ok(match jet_services_restarts(&tree, &endpoint) {
                Ok(n) => CtValue::Present(Box::new(CtValue::Int(n))),
                Err(e) => CtValue::failed(Box::new(map_err(e))),
            })
        }
        "fail_worker" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let endpoint = ct_to_endpoint(one(1)?, span)?;
            Ok(match jet_services_fail_worker(&mut tree, &endpoint) {
                Ok(()) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "endpoint_show" => Ok(CtValue::Str(jet_services_endpoint_show(&ct_to_endpoint(
            one(0)?,
            span,
        )?))),
        "tree_show" => Ok(CtValue::Str(jet_services_tree_show(&ct_to_tree(
            one(0)?,
            span,
        )?))),
        "send_durable" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let endpoint = ct_to_endpoint(one(1)?, span)?;
            let message = match one(2)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("message", span)),
            };
            let key = match one(3)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("idempotency key", span)),
            };
            Ok(
                match jet_services_send_durable(&mut tree, &endpoint, message, key) {
                    Ok(receipt) => {
                        CtValue::Present(Box::new(mutate_ok(tree, delivery_record_to_ct(receipt))))
                    }
                    Err(e) => mutate_err(tree, map_err(e)),
                },
            )
        }
        "delivery_wait"
        | "delivery_status"
        | "delivery_retry"
        | "delivery_cancel"
        | "delivery_receipt"
        | "delivery_events" => {
            let delivery = ct_to_delivery_record(one(0)?, span)?;
            let result = match method {
                "delivery_wait" => jet_services_delivery_wait(&delivery)
                    .map(delivery_state_to_ct),
                "delivery_status" => jet_services_delivery_status(&delivery)
                    .map(delivery_state_to_ct),
                "delivery_retry" => jet_services_delivery_retry(&delivery)
                    .map(delivery_record_to_ct),
                "delivery_cancel" => jet_services_delivery_cancel(&delivery)
                    .map(delivery_record_to_ct),
                "delivery_receipt" => jet_services_delivery_receipt(&delivery)
                    .map(delivery_receipt_to_ct),
                "delivery_events" => jet_services_delivery_events(&delivery).map(|events| {
                    CtValue::List(events.into_iter().map(delivery_event_to_ct).collect())
                }),
                _ => unreachable!(),
            };
            Ok(match result {
                Ok(value) => CtValue::Present(Box::new(value)),
                Err(error) => CtValue::failed(Box::new(map_err(error))),
            })
        }
        "dead_letter_count" => Ok(CtValue::Int(jet_services_dead_letter_count(&ct_to_tree(
            one(0)?,
            span,
        )?))),
        "drain_dead_letters" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            Ok(match jet_services_drain_dead_letters(&mut tree) {
                Ok(n) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Int(n)))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "set_state_empty" | "set_state_snapshot" | "set_state_event_log" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let result = match method {
                "set_state_empty" => jet_services_set_state_empty(&mut tree),
                "set_state_snapshot" => {
                    let store = ct_to_state_store(one(1)?, span)?;
                    let schema = ct_to_service_string(
                        one(2)?,
                        MAX_SERVICE_STATE_SCHEMA,
                        "service state schema",
                        span,
                    )?;
                    let version = match one(3)? {
                        CtValue::Int(version) => *version,
                        _ => return Err(unsupported("service state version", span)),
                    };
                    let migration = ct_to_service_string(
                        one(4)?,
                        MAX_SERVICE_STATE_SCHEMA,
                        "service state migration policy",
                        span,
                    )?;
                    jet_services_set_state_snapshot(&mut tree, store, schema, version, migration)
                }
                _ => {
                    let store = ct_to_state_store(one(1)?, span)?;
                    let schema = ct_to_service_string(
                        one(2)?,
                        MAX_SERVICE_STATE_SCHEMA,
                        "service state schema",
                        span,
                    )?;
                    let version = match one(3)? {
                        CtValue::Int(version) => *version,
                        _ => return Err(unsupported("service state version", span)),
                    };
                    let migration = ct_to_service_string(
                        one(4)?,
                        MAX_SERVICE_STATE_SCHEMA,
                        "service state migration policy",
                        span,
                    )?;
                    jet_services_set_state_event_log(&mut tree, store, schema, version, migration)
                }
            };
            Ok(match result {
                Ok(()) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "commit_snapshot" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let payload = match one(1)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("snapshot payload", span)),
            };
            Ok(match jet_services_commit_snapshot(&mut tree, payload) {
                Ok(()) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "restore_snapshot" => {
            let tree = ct_to_tree(one(0)?, span)?;
            Ok(match jet_services_restore_snapshot(&tree) {
                Ok(s) => CtValue::Present(Box::new(CtValue::Str(s))),
                Err(e) => CtValue::failed(Box::new(map_err(e))),
            })
        }
        "append_event" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let event = match one(1)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("event", span)),
            };
            Ok(match jet_services_append_event(&mut tree, event) {
                Ok(()) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "event_count" => Ok(CtValue::Int(jet_services_event_count(&ct_to_tree(
            one(0)?,
            span,
        )?))),
        "replay_events" => Ok(CtValue::Str(jet_services_replay_events(&ct_to_tree(
            one(0)?,
            span,
        )?))),
        "workflow_start" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let id = match one(1)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("workflow id", span)),
            };
            let version = match one(2)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("workflow version", span)),
            };
            Ok(match jet_services_workflow_start(&mut tree, id, version) {
                Ok(handle) => CtValue::Present(Box::new(mutate_ok(
                    tree,
                    workflow_handle_to_ct(&handle, span)?,
                ))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "workflow_sleep" => {
            let handle = ct_to_workflow_handle(one(0)?, span)?;
            let nanos = match one(1)? {
                CtValue::Struct { type_name, fields } if type_name == "Duration" => fields
                    .iter()
                    .find_map(|(name, value)| match (name.as_str(), value) {
                        ("ns", CtValue::Int(ns)) => Some(*ns),
                        _ => None,
                    })
                    .ok_or_else(|| unsupported("workflow sleep duration", span))?,
                _ => return Err(unsupported("workflow sleep duration", span)),
            };
            Ok(match jet_services_workflow_sleep(&handle, nanos) {
                Ok(()) => {
                    CtValue::Present(Box::new(mutate_handle_ok(&handle, CtValue::Unit, span)?))
                }
                Err(e) => mutate_handle_err(&handle, map_err(e), span)?,
            })
        }
        "workflow_activity_wait" => {
            let handle = ct_to_workflow_handle(one(0)?, span)?;
            let activity =
                ct_to_service_string(one(1)?, MAX_SERVICE_NAME, "workflow activity", span)?;
            let argument = ct_to_service_string(
                one(2)?,
                MAX_SERVICE_MESSAGE,
                "workflow activity argument",
                span,
            )?;
            Ok(
                match jet_services_workflow_activity_wait(&handle, activity, argument) {
                    Ok(value) => CtValue::Present(Box::new(mutate_handle_ok(
                        &handle,
                        CtValue::Str(value),
                        span,
                    )?)),
                    Err(e) => mutate_handle_err(&handle, map_err(e), span)?,
                },
            )
        }
        "workflow_all" => {
            let handle = ct_to_workflow_handle(one(0)?, span)?;
            let values = match one(1)? {
                CtValue::List(values) => values
                    .iter()
                    .map(|value| {
                        ct_to_service_string(value, MAX_SERVICE_MESSAGE, "workflow all value", span)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                _ => return Err(unsupported("workflow all values", span)),
            };
            Ok(match jet_services_workflow_all(&handle, values) {
                Ok(values) => CtValue::Present(Box::new(mutate_handle_ok(
                    &handle,
                    CtValue::List(values.into_iter().map(CtValue::Str).collect()),
                    span,
                )?)),
                Err(e) => mutate_handle_err(&handle, map_err(e), span)?,
            })
        }
        "workflow_step" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let run_id = ct_to_workflow_run_id(one(1)?, span)?;
            let step = match one(2)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("step", span)),
            };
            Ok(match jet_services_workflow_step(&mut tree, run_id, step) {
                Ok(()) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "workflow_activity" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let run_id = ct_to_workflow_run_id(one(1)?, span)?;
            let activity = match one(2)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("workflow activity", span)),
            };
            let key = match one(3)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("activity idempotency key", span)),
            };
            let max_attempts = match one(4)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("activity retry limit", span)),
            };
            Ok(
                match jet_services_workflow_activity(&mut tree, run_id, activity, key, max_attempts)
                {
                    Ok(status) => {
                        CtValue::Present(Box::new(mutate_ok(tree, task_status_to_ct(&status))))
                    }
                    Err(e) => mutate_err(tree, map_err(e)),
                },
            )
        }
        "workflow_activity_retry" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let run_id = ct_to_workflow_run_id(one(1)?, span)?;
            let key = match one(2)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("activity idempotency key", span)),
            };
            let outcome = ct_to_task_outcome(one(3)?, span)?;
            Ok(
                match jet_services_workflow_activity_retry(&mut tree, run_id, key, outcome) {
                    Ok(status) => {
                        CtValue::Present(Box::new(mutate_ok(tree, task_status_to_ct(&status))))
                    }
                    Err(e) => mutate_err(tree, map_err(e)),
                },
            )
        }
        "workflow_activity_complete" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let run_id = ct_to_workflow_run_id(one(1)?, span)?;
            let key = match one(2)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("activity idempotency key", span)),
            };
            let outcome = ct_to_task_outcome(one(3)?, span)?;
            Ok(
                match jet_services_workflow_activity_complete(&mut tree, run_id, key, outcome) {
                    Ok(outcome) => {
                        CtValue::Present(Box::new(mutate_ok(tree, task_outcome_to_ct(&outcome))))
                    }
                    Err(e) => mutate_err(tree, map_err(e)),
                },
            )
        }
        "workflow_history" => {
            let tree = ct_to_tree(one(0)?, span)?;
            let run_id = ct_to_workflow_run_id(one(1)?, span)?;
            Ok(match jet_services_workflow_history(&tree, run_id) {
                Ok(s) => CtValue::Present(Box::new(CtValue::Str(s))),
                Err(e) => CtValue::failed(Box::new(map_err(e))),
            })
        }
        "workflow_outcome" => {
            let tree = ct_to_tree(one(0)?, span)?;
            let run_id = ct_to_workflow_run_id(one(1)?, span)?;
            Ok(match jet_services_workflow_outcome(&tree, run_id) {
                Ok(outcome) => CtValue::Present(Box::new(task_outcome_to_ct(&outcome))),
                Err(e) => CtValue::failed(Box::new(map_err(e))),
            })
        }
        "directory_register" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let name = match one(1)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("directory name", span)),
            };
            let endpoint = ct_to_endpoint(one(2)?, span)?;
            Ok(
                match jet_services_directory_register(&mut tree, name, endpoint) {
                    Ok(()) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Unit))),
                    Err(e) => mutate_err(tree, map_err(e)),
                },
            )
        }
        "directory_resolve" => {
            let tree = ct_to_tree(one(0)?, span)?;
            let name = match one(1)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("directory name", span)),
            };
            Ok(match jet_services_directory_resolve(&tree, &name) {
                Ok(ep) => CtValue::Present(Box::new(endpoint_to_ct(&ep))),
                Err(e) => CtValue::failed(Box::new(map_err(e))),
            })
        }
        "directory_generation" => Ok(CtValue::Int(jet_services_directory_generation(
            &ct_to_tree(one(0)?, span)?,
        ))),
        "drain_worker" | "partition_worker" | "reconcile_worker" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let endpoint = ct_to_endpoint(one(1)?, span)?;
            let result = match method {
                "drain_worker" => jet_services_drain_worker(&mut tree, &endpoint),
                "partition_worker" => jet_services_partition_worker(&mut tree, &endpoint),
                _ => jet_services_reconcile_worker(&mut tree, &endpoint),
            };
            Ok(match result {
                Ok(()) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "handoff_generation" | "rollback_generation" | "chaos_fail" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let result = match method {
                "handoff_generation" => jet_services_handoff_generation(&mut tree),
                "rollback_generation" => jet_services_rollback_generation(&mut tree),
                _ => jet_services_chaos_fail(&mut tree),
            };
            Ok(match result {
                Ok(n) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Int(n)))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "upgrade_receipt" => Ok(
            match jet_services_upgrade_receipt(&ct_to_tree(one(0)?, span)?) {
                Ok(receipt) => CtValue::Present(Box::new(upgrade_receipt_to_ct(&receipt))),
                Err(error) => CtValue::failed(Box::new(map_err(error))),
            },
        ),
        "observe" => Ok(CtValue::Str(jet_services_observe(&ct_to_tree(
            one(0)?,
            span,
        )?))),
        _ => Err(unsupported(&format!("`core.services.{method}()`"), span)),
    }
}

#[allow(dead_code)]
fn _type_anchor() -> Type {
    Type::Named("ServiceTree".to_string())
}
