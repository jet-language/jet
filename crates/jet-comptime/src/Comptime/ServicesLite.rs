//! D-SERVICE1=D / I9: `core.services` ambient includes Prelude `Services.rs`.

use crate::AST::{CtReport, CtValue, Type};
use crate::Diagnostics::{Diagnostic, Span};
use super::Diagnostics::unsupported;

trait JetShow {
    fn jet_show(&self) -> String;
}

trait JetDisplay {
    fn jet_display(&self) -> String;
}

trait JetDebug {
    fn jet_debug(&self) -> String;
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
            type_name,
            variant,
            ..
        } if type_name == "ServiceRestart" => match variant.as_str() {
            "OneForOne" => Ok(JetServiceRestart::OneForOne),
            "OneForAll" => Ok(JetServiceRestart::OneForAll),
            "RestForOne" => Ok(JetServiceRestart::RestForOne),
            _ => Err(unsupported("ServiceRestart", span)),
        },
        _ => Err(unsupported("ServiceRestart", span)),
    }
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
            type_name,
            variant,
            ..
        } if type_name == "ServiceDelivery" => match variant.as_str() {
            "AtMostOnce" => Ok(JetServiceDelivery::AtMostOnce),
            "DurableAtLeastOnce" => Ok(JetServiceDelivery::DurableAtLeastOnce),
            _ => Err(unsupported("ServiceDelivery", span)),
        },
        _ => Err(unsupported("ServiceDelivery", span)),
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

fn bytes_to_ct(bytes: &[u8]) -> CtValue {
    CtValue::List(
        bytes
            .iter()
            .map(|byte| CtValue::Int(i64::from(*byte)))
            .collect(),
    )
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

fn ct_to_bytes(value: &CtValue, span: Span) -> Result<Vec<u8>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(unsupported("service directory key", span));
    };
    if values.len() > 32 {
        return Err(unsupported("service directory key length", span));
    }
    values
        .iter()
        .map(|value| match value {
            CtValue::Int(byte) if (0..=255).contains(byte) => Ok(*byte as u8),
            _ => Err(unsupported("service directory key byte", span)),
        })
        .collect()
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
    CtValue::Struct {
        type_name: "ServiceMailbox".to_string(),
        fields: vec![
            ("endpoint".to_string(), endpoint_to_ct(&m.endpoint)),
            ("capacity".to_string(), CtValue::Int(m.capacity)),
            ("depth".to_string(), CtValue::Int(m.depth)),
            (
                "messages".to_string(),
                CtValue::List(m.messages.iter().cloned().map(CtValue::Str).collect()),
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
    let mut mailbox = jet_services_new_mailbox(endpoint, capacity, messages)
        .map_err(|_| unsupported("ServiceMailbox channel", span))?;
    if depth < 0 || mailbox.depth != depth {
        return Err(unsupported("mailbox depth", span));
    }
    mailbox.depth = depth;
    Ok(mailbox)
}

fn worker_to_ct(w: &JetServiceWorker) -> CtValue {
    CtValue::Struct {
        type_name: "ServiceWorker".to_string(),
        fields: vec![
            ("name".to_string(), CtValue::Str(w.name.clone())),
            ("endpoint".to_string(), endpoint_to_ct(&w.endpoint)),
            ("mailbox".to_string(), mailbox_to_ct(&w.mailbox)),
            ("restarts".to_string(), CtValue::Int(w.restarts)),
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
    Ok(JetServiceWorker {
        name: ct_to_service_string(field("name")?, MAX_SERVICE_NAME, "worker name", span)?,
        endpoint: ct_to_endpoint(field("endpoint")?, span)?,
        mailbox: ct_to_mailbox(field("mailbox")?, span)?,
        restarts: match field("restarts")? {
            CtValue::Int(n) => *n,
            _ => return Err(unsupported("worker restarts", span)),
        },
        running: match field("running")? {
            CtValue::Bool(b) => *b,
            _ => return Err(unsupported("worker running state", span)),
        },
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
            type_name,
            variant,
            ..
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
        ],
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
                    .map(|x| {
                        ct_to_service_string(x, MAX_SERVICE_MESSAGE, "workflow string", span)
                    })
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
    })
}

fn upgrade_receipt_to_ct(receipt: &JetServiceUpgradeReceipt) -> CtValue {
    CtValue::Struct {
        type_name: "ServiceUpgradeReceipt".to_string(),
        fields: vec![
            ("from_generation".to_string(), CtValue::Int(receipt.from_generation)),
            ("to_generation".to_string(), CtValue::Int(receipt.to_generation)),
            ("migration".to_string(), CtValue::Str(receipt.migration.clone())),
            ("rollback_store".to_string(), CtValue::Str(receipt.rollback_store.clone())),
            ("rollback_available".to_string(), CtValue::Bool(receipt.rollback_available)),
            (
                "pinned_shards".to_string(),
                CtValue::List(receipt.pinned_shards.iter().cloned().map(CtValue::Str).collect()),
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
        fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
            .ok_or_else(|| unsupported("ServiceUpgradeReceipt field", span))
    };
    Ok(JetServiceUpgradeReceipt {
        from_generation: match field("from_generation")? { CtValue::Int(n) => *n, _ => return Err(unsupported("upgrade from generation", span)) },
        to_generation: match field("to_generation")? { CtValue::Int(n) => *n, _ => return Err(unsupported("upgrade to generation", span)) },
        migration: ct_to_service_string(field("migration")?, 32, "upgrade migration", span)?,
        rollback_store: ct_to_service_string(field("rollback_store")?, MAX_SERVICE_STATE_STORE, "upgrade rollback store", span)?,
        rollback_available: match field("rollback_available")? { CtValue::Bool(v) => *v, _ => return Err(unsupported("upgrade rollback availability", span)) },
        pinned_shards: ct_str_list(field("pinned_shards")?, span)?,
    })
}

fn tree_to_ct(tree: &JetServiceTree) -> CtValue {
    CtValue::Struct {
        type_name: "ServiceTree".to_string(),
        fields: vec![
            ("name".to_string(), CtValue::Str(tree.name.clone())),
            ("authority".to_string(), CtValue::Str(tree.authority.clone())),
            ("generation".to_string(), CtValue::Int(tree.generation)),
            ("delivery".to_string(), delivery_to_ct(tree.delivery.clone())),
            ("restart".to_string(), restart_to_ct(tree.restart.clone())),
            (
                "workers".to_string(),
                CtValue::List(tree.workers.iter().map(worker_to_ct).collect()),
            ),
            (
                "groups".to_string(),
                CtValue::List(
                    tree.groups
                        .iter()
                        .map(|g| {
                            CtValue::Struct {
                                type_name: "ServiceGroup".to_string(),
                                fields: vec![
                                    ("name".to_string(), CtValue::Str(g.name.clone())),
                                    ("restart".to_string(), restart_to_ct(g.restart.clone())),
                                    (
                                        "workers".to_string(),
                                        CtValue::List(
                                            g.workers.iter().cloned().map(CtValue::Str).collect(),
                                        ),
                                    ),
                                ],
                            }
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
                CtValue::List(tree.dead_letters.iter().cloned().map(CtValue::Str).collect()),
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
                        .map(|(n, ep, signature)| {
                            CtValue::Struct {
                                type_name: "ServiceDirectoryEntry".to_string(),
                                fields: vec![
                                    ("name".to_string(), CtValue::Str(n.clone())),
                                    ("endpoint".to_string(), endpoint_to_ct(ep)),
                                    ("signature".to_string(), CtValue::Str(signature.clone())),
                                ],
                            }
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
            ("directory_key".to_string(), bytes_to_ct(&tree.directory_key)),
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
                    CtValue::Str(s) if s.len() <= MAX_SERVICE_MESSAGE && !s.chars().any(char::is_control) => Ok(s.clone()),
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
                let workers = match fields.iter().find(|(n, _)| n == "workers") {
                    Some((_, CtValue::List(xs))) => {
                        if xs.len() > MAX_SERVICE_WORKERS {
                            return Err(unsupported("group worker limit", span));
                        }
                        xs.iter()
                            .map(|x| {
                                ct_to_service_string(
                                    x,
                                    MAX_SERVICE_NAME,
                                    "group worker",
                                    span,
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?
                    }
                    Some(_) => return Err(unsupported("group workers", span)),
                    None => return Err(unsupported("group workers", span)),
                };
                Ok(JetServiceGroup {
                    name,
                    restart,
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
    let tree = JetServiceTree {
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
        directory_key: ct_to_bytes(field("directory_key")?, span)?,
        draining: ct_str_list(field("draining")?, span)?,
        partitioned,
        workflows,
        task_group: std::sync::Arc::new(JetTaskGroupRuntime::new()),
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
    jet_services_validate_tree(&tree)
        .map_err(|error| unsupported(&error.jet_show(), span))?;
    for worker in &tree.workers {
        jet_services_authority_hydrate(&worker.endpoint, tree.started && worker.running)
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

fn receipt_to_ct(receipt: JetServiceReceipt) -> CtValue {
    match receipt {
        JetServiceReceipt::Accepted(id) => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "Accepted".to_string(),
            args: vec![(None, CtValue::Str(id))],
        },
        JetServiceReceipt::Duplicate(id) => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "Duplicate".to_string(),
            args: vec![(None, CtValue::Str(id))],
        },
        JetServiceReceipt::Retained { id, until } => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "Retained".to_string(),
            args: vec![
                (Some("id".to_string()), CtValue::Str(id)),
                (Some("until".to_string()), CtValue::Int(until)),
            ],
        },
        JetServiceReceipt::DeadLettered(id) => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "DeadLettered".to_string(),
            args: vec![(None, CtValue::Str(id))],
        },
        JetServiceReceipt::Rejected(reason) => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "Rejected".to_string(),
            args: vec![(None, CtValue::Str(reason))],
        },
        JetServiceReceipt::Unavailable(reason) => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "Unavailable".to_string(),
            args: vec![(None, CtValue::Str(reason))],
        },
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
            Ok(match jet_services_runtime_send(&runtime, &endpoint, &message, &key) {
                Ok(receipt) => CtValue::Present(Box::new(receipt_to_ct(receipt))),
                Err(error) => CtValue::failed(Box::new(map_err(error))),
            })
        }
        "retry" | "dead_letter" | "retain" => {
            let id = ct_to_service_string(
                one(0)?,
                SERVICE_AUTH_MAX_KEY,
                &format!("ServiceRuntime.{method} id"),
                span,
            )?;
            let result = match method {
                "retry" => jet_services_runtime_retry(&runtime, &id),
                "dead_letter" => jet_services_runtime_dead_letter(&runtime, &id),
                "retain" => jet_services_runtime_retain(&runtime, &id),
                _ => unreachable!(),
            };
            Ok(match result {
                Ok(receipt) => CtValue::Present(Box::new(receipt_to_ct(receipt))),
                Err(error) => CtValue::failed(Box::new(map_err(error))),
            })
        }
        "commit" => {
            let id = ct_to_service_string(
                one(0)?,
                SERVICE_AUTH_MAX_KEY,
                "ServiceRuntime.commit id",
                span,
            )?;
            Ok(match jet_services_runtime_commit(&runtime, &id) {
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
        "tree" => match one(0)? {
            value => {
                let name = ct_to_service_string(value, MAX_SERVICE_NAME, "tree name", span)?;
                Ok(tree_to_ct(&jet_services_tree(name)))
            }
        },
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
            let capacity = match one(2)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("capacity", span)),
            };
            Ok(match jet_services_worker(&mut tree, name, capacity) {
                Ok(ep) => CtValue::Present(Box::new(mutate_ok(tree, endpoint_to_ct(&ep)))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
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
                        .map(|x| {
                            ct_to_service_string(
                                x,
                                MAX_SERVICE_NAME,
                                "group worker",
                                span,
                            )
                        })
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
            Ok(match jet_services_send_durable(&mut tree, &endpoint, message, key) {
                Ok(()) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
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
                Ok(run_id) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Int(run_id)))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "workflow_step" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let run_id = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("run id", span)),
            };
            let step = match one(2)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("step", span)),
            };
            Ok(match jet_services_workflow_step(&mut tree, run_id, step) {
                Ok(()) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
        }
        "workflow_history" => {
            let tree = ct_to_tree(one(0)?, span)?;
            let run_id = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("run id", span)),
            };
            Ok(match jet_services_workflow_history(&tree, run_id) {
                Ok(s) => CtValue::Present(Box::new(CtValue::Str(s))),
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
            Ok(match jet_services_directory_register(&mut tree, name, endpoint) {
                Ok(()) => CtValue::Present(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => mutate_err(tree, map_err(e)),
            })
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
        "drain_worker" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let endpoint = ct_to_endpoint(one(1)?, span)?;
            Ok(match jet_services_drain_worker(&mut tree, &endpoint) {
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
        "upgrade_receipt" => Ok(match jet_services_upgrade_receipt(&ct_to_tree(one(0)?, span)?) {
            Ok(receipt) => CtValue::Present(Box::new(upgrade_receipt_to_ct(&receipt))),
            Err(error) => CtValue::failed(Box::new(map_err(error))),
        }),
        "observe" => Ok(CtValue::Str(jet_services_observe(&ct_to_tree(
            one(0)?,
            span,
        )?))),
        _ => Err(unsupported(
            &format!("`core.services.{method}()`"),
            span,
        )),
    }
}

#[allow(dead_code)]
fn _type_anchor() -> Type {
    Type::Named("ServiceTree".to_string())
}
