//! D-SERVICE1=D / I9: `core.services` ambient includes Prelude `Services.rs`.

use crate::AST::{CtValue, Type};
use crate::Diagnostics::{Diagnostic, Span};
use super::Diagnostics::unsupported;

trait JetShow {
    fn jet_show(&self) -> String;
}

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
        _ => Ok(JetServiceDelivery::AtMostOnce),
    }
}

fn endpoint_to_ct(e: &JetServiceEndpoint) -> CtValue {
    CtValue::Struct {
        type_name: "ServiceEndpoint".to_string(),
        fields: vec![
            ("tree".to_string(), CtValue::Str(e.tree.clone())),
            ("worker".to_string(), CtValue::Str(e.worker.clone())),
            ("generation".to_string(), CtValue::Int(e.generation)),
        ],
    }
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
    Ok(JetServiceEndpoint {
        tree,
        worker,
        generation,
    })
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
    let messages = match field("messages")? {
        CtValue::List(xs) => xs
            .iter()
            .map(|x| match x {
                CtValue::Str(s) => Ok(s.clone()),
                _ => Err(unsupported("mailbox message", span)),
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(unsupported("mailbox messages", span)),
    };
    let capacity = match field("capacity")? {
        CtValue::Int(n) => *n,
        _ => return Err(unsupported("mailbox capacity", span)),
    };
    let depth = match field("depth")? {
        CtValue::Int(n) => *n,
        _ => messages.len() as i64,
    };
    Ok(JetServiceMailbox {
        endpoint: ct_to_endpoint(field("endpoint")?, span)?,
        capacity,
        depth,
        messages,
    })
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
        name: match field("name")? {
            CtValue::Str(s) => s.clone(),
            _ => return Err(unsupported("worker name", span)),
        },
        endpoint: ct_to_endpoint(field("endpoint")?, span)?,
        mailbox: ct_to_mailbox(field("mailbox")?, span)?,
        restarts: match field("restarts")? {
            CtValue::Int(n) => *n,
            _ => 0,
        },
        running: match field("running")? {
            CtValue::Bool(b) => *b,
            _ => false,
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
        _ => Ok(JetServiceStateAdapter::Empty),
    }
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
    let CtValue::Struct { fields, .. } = v else {
        return Err(unsupported("ServiceWorkflow", span));
    };
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| unsupported("workflow field", span))
    };
    let str_list = |name: &str| -> Result<Vec<String>, Diagnostic> {
        match field(name)? {
            CtValue::List(xs) => xs
                .iter()
                .map(|x| match x {
                    CtValue::Str(s) => Ok(s.clone()),
                    _ => Err(unsupported("workflow string", span)),
                })
                .collect(),
            _ => Ok(Vec::new()),
        }
    };
    Ok(JetServiceWorkflow {
        id: match field("id")? {
            CtValue::Str(s) => s.clone(),
            _ => return Err(unsupported("workflow id", span)),
        },
        run_id: match field("run_id")? {
            CtValue::Int(n) => *n,
            _ => 0,
        },
        version: match field("version")? {
            CtValue::Int(n) => *n,
            _ => 1,
        },
        steps: str_list("steps")?,
        history: str_list("history")?,
    })
}

fn tree_to_ct(tree: &JetServiceTree) -> CtValue {
    CtValue::Struct {
        type_name: "ServiceTree".to_string(),
        fields: vec![
            ("name".to_string(), CtValue::Str(tree.name.clone())),
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
                        .cloned()
                        .map(CtValue::Str)
                        .collect(),
                ),
            ),
            (
                "directory".to_string(),
                CtValue::List(
                    tree.directory
                        .iter()
                        .map(|(n, ep)| {
                            CtValue::Struct {
                                type_name: "ServiceDirectoryEntry".to_string(),
                                fields: vec![
                                    ("name".to_string(), CtValue::Str(n.clone())),
                                    ("endpoint".to_string(), endpoint_to_ct(ep)),
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
                "workflows".to_string(),
                CtValue::List(tree.workflows.iter().map(workflow_to_ct).collect()),
            ),
            ("chaos_fails".to_string(), CtValue::Int(tree.chaos_fails)),
            (
                "previous_generation".to_string(),
                CtValue::Int(tree.previous_generation),
            ),
        ],
    }
}

fn ct_str_list(v: &CtValue, span: Span) -> Result<Vec<String>, Diagnostic> {
    match v {
        CtValue::List(xs) => xs
            .iter()
            .map(|x| match x {
                CtValue::Str(s) => Ok(s.clone()),
                _ => Err(unsupported("string list", span)),
            })
            .collect(),
        _ => Ok(Vec::new()),
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
    let opt_field = |name: &str| fields.iter().find(|(n, _)| n == name).map(|(_, v)| v);
    let workers = match field("workers")? {
        CtValue::List(xs) => xs
            .iter()
            .map(|x| ct_to_worker(x, span))
            .collect::<Result<Vec<_>, _>>()?,
        _ => Vec::new(),
    };
    let groups = match field("groups")? {
        CtValue::List(xs) => xs
            .iter()
            .map(|x| {
                let CtValue::Struct { fields, .. } = x else {
                    return Err(unsupported("ServiceGroup", span));
                };
                let name = fields
                    .iter()
                    .find(|(n, _)| n == "name")
                    .and_then(|(_, v)| match v {
                        CtValue::Str(s) => Some(s.clone()),
                        _ => None,
                    })
                    .ok_or_else(|| unsupported("group name", span))?;
                let restart = fields
                    .iter()
                    .find(|(n, _)| n == "restart")
                    .map(|(_, v)| ct_to_restart(v, span))
                    .transpose()?
                    .unwrap_or(JetServiceRestart::OneForOne);
                let workers = fields
                    .iter()
                    .find(|(n, _)| n == "workers")
                    .and_then(|(_, v)| match v {
                        CtValue::List(xs) => Some(
                            xs.iter()
                                .filter_map(|x| match x {
                                    CtValue::Str(s) => Some(s.clone()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>(),
                        ),
                        _ => None,
                    })
                    .unwrap_or_default();
                Ok(JetServiceGroup {
                    name,
                    restart,
                    workers,
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => Vec::new(),
    };
    let directory = match opt_field("directory") {
        Some(CtValue::List(xs)) => xs
            .iter()
            .map(|x| {
                let CtValue::Struct { fields, .. } = x else {
                    return Err(unsupported("directory entry", span));
                };
                let name = fields
                    .iter()
                    .find(|(n, _)| n == "name")
                    .and_then(|(_, v)| match v {
                        CtValue::Str(s) => Some(s.clone()),
                        _ => None,
                    })
                    .ok_or_else(|| unsupported("directory name", span))?;
                let endpoint = fields
                    .iter()
                    .find(|(n, _)| n == "endpoint")
                    .ok_or_else(|| unsupported("directory endpoint", span))?;
                Ok((name, ct_to_endpoint(&endpoint.1, span)?))
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => Vec::new(),
    };
    let workflows = match opt_field("workflows") {
        Some(CtValue::List(xs)) => xs
            .iter()
            .map(|x| ct_to_workflow(x, span))
            .collect::<Result<Vec<_>, _>>()?,
        _ => Vec::new(),
    };
    let snapshot = match opt_field("snapshot") {
        Some(CtValue::Str(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    Ok(JetServiceTree {
        name: match field("name")? {
            CtValue::Str(s) => s.clone(),
            _ => return Err(unsupported("tree name", span)),
        },
        generation: match field("generation")? {
            CtValue::Int(n) => *n,
            _ => 1,
        },
        delivery: ct_to_delivery(field("delivery")?, span)?,
        restart: ct_to_restart(field("restart")?, span)?,
        workers,
        groups,
        started: match field("started")? {
            CtValue::Bool(b) => *b,
            _ => false,
        },
        state_adapter: match opt_field("state_adapter") {
            Some(v) => ct_to_state_adapter(v, span)?,
            None => JetServiceStateAdapter::Empty,
        },
        snapshot,
        event_log: match opt_field("event_log") {
            Some(v) => ct_str_list(v, span)?,
            None => Vec::new(),
        },
        dead_letters: match opt_field("dead_letters") {
            Some(v) => ct_str_list(v, span)?,
            None => Vec::new(),
        },
        idempotency_seen: match opt_field("idempotency_seen") {
            Some(v) => ct_str_list(v, span)?,
            None => Vec::new(),
        },
        directory,
        draining: match opt_field("draining") {
            Some(v) => ct_str_list(v, span)?,
            None => Vec::new(),
        },
        workflows,
        chaos_fails: match opt_field("chaos_fails") {
            Some(CtValue::Int(n)) => *n,
            _ => 0,
        },
        previous_generation: match opt_field("previous_generation") {
            Some(CtValue::Int(n)) => *n,
            _ => 0,
        },
    })
}

fn map_err(err: JetServiceError) -> CtValue {
    let (variant, message) = match err {
        JetServiceError::Full(m) => ("Full", m),
        JetServiceError::Ambiguous(m) => ("Ambiguous", m),
        JetServiceError::Unknown(m) => ("Unknown", m),
        JetServiceError::NotStarted(m) => ("NotStarted", m),
        JetServiceError::Policy(m) => ("Policy", m),
    };
    CtValue::Enum {
        type_name: "ServiceError".to_string(),
        variant: variant.to_string(),
        args: vec![(None, CtValue::Str(message))],
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

pub fn take_mut_ok(value: CtValue) -> Result<(CtValue, CtValue), CtValue> {
    match value {
        CtValue::ResOk(inner) => match *inner {
            CtValue::Struct { type_name, fields } if type_name == "__JetServiceMut" => {
                let tree = fields
                    .iter()
                    .find(|(n, _)| n == "tree")
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| {
                        CtValue::ResErr(Box::new(CtValue::Str(
                            "core.services: missing tree write-back".to_string(),
                        )))
                    })?;
                let val = fields
                    .iter()
                    .find(|(n, _)| n == "value")
                    .map(|(_, v)| v.clone())
                    .unwrap_or(CtValue::Unit);
                Ok((tree, CtValue::ResOk(Box::new(val))))
            }
            other => Ok((CtValue::Unit, CtValue::ResOk(Box::new(other)))),
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
            CtValue::Str(name) => Ok(tree_to_ct(&jet_services_tree(name.clone()))),
            _ => Err(unsupported("tree name", span)),
        },
        "restart_one_for_one" => Ok(restart_to_ct(jet_services_restart_one_for_one())),
        "restart_one_for_all" => Ok(restart_to_ct(jet_services_restart_one_for_all())),
        "restart_rest_for_one" => Ok(restart_to_ct(jet_services_restart_rest_for_one())),
        "delivery_at_most_once" => Ok(delivery_to_ct(jet_services_delivery_at_most_once())),
        "delivery_durable" => Ok(delivery_to_ct(jet_services_delivery_durable())),
        "set_restart" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let restart = ct_to_restart(one(1)?, span)?;
            Ok(match jet_services_set_restart(&mut tree, restart) {
                Ok(()) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
        "set_delivery" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let delivery = ct_to_delivery(one(1)?, span)?;
            Ok(match jet_services_set_delivery(&mut tree, delivery) {
                Ok(()) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
        "worker" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let name = match one(1)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("worker name", span)),
            };
            let capacity = match one(2)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("capacity", span)),
            };
            Ok(match jet_services_worker(&mut tree, name, capacity) {
                Ok(ep) => CtValue::ResOk(Box::new(mutate_ok(tree, endpoint_to_ct(&ep)))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
        "group" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let name = match one(1)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("group name", span)),
            };
            let workers = match one(2)? {
                CtValue::List(xs) => xs
                    .iter()
                    .map(|x| match x {
                        CtValue::Str(s) => Ok(s.clone()),
                        _ => Err(unsupported("group worker", span)),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                _ => return Err(unsupported("group workers", span)),
            };
            Ok(match jet_services_group(&mut tree, name, workers) {
                Ok(()) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
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
                Ok(()) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
        "send" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let endpoint = ct_to_endpoint(one(1)?, span)?;
            let message = match one(2)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("message", span)),
            };
            Ok(match jet_services_send(&mut tree, &endpoint, message) {
                Ok(()) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
        "receive" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let endpoint = ct_to_endpoint(one(1)?, span)?;
            Ok(match jet_services_receive(&mut tree, &endpoint) {
                Ok(msg) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Str(msg)))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
        "mailbox_depth" => {
            let tree = ct_to_tree(one(0)?, span)?;
            let endpoint = ct_to_endpoint(one(1)?, span)?;
            Ok(match jet_services_mailbox_depth(&tree, &endpoint) {
                Ok(n) => CtValue::ResOk(Box::new(CtValue::Int(n))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
        "restarts" => {
            let tree = ct_to_tree(one(0)?, span)?;
            let endpoint = ct_to_endpoint(one(1)?, span)?;
            Ok(match jet_services_restarts(&tree, &endpoint) {
                Ok(n) => CtValue::ResOk(Box::new(CtValue::Int(n))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
        "fail_worker" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let endpoint = ct_to_endpoint(one(1)?, span)?;
            Ok(match jet_services_fail_worker(&mut tree, &endpoint) {
                Ok(()) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
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
                Ok(()) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
        "dead_letter_count" => Ok(CtValue::Int(jet_services_dead_letter_count(&ct_to_tree(
            one(0)?,
            span,
        )?))),
        "drain_dead_letters" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            Ok(match jet_services_drain_dead_letters(&mut tree) {
                Ok(n) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Int(n)))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
        "set_state_empty" | "set_state_snapshot" | "set_state_event_log" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let result = match method {
                "set_state_empty" => jet_services_set_state_empty(&mut tree),
                "set_state_snapshot" => jet_services_set_state_snapshot(&mut tree),
                _ => jet_services_set_state_event_log(&mut tree),
            };
            Ok(match result {
                Ok(()) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
        "commit_snapshot" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let payload = match one(1)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("snapshot payload", span)),
            };
            Ok(match jet_services_commit_snapshot(&mut tree, payload) {
                Ok(()) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
        "restore_snapshot" => {
            let tree = ct_to_tree(one(0)?, span)?;
            Ok(match jet_services_restore_snapshot(&tree) {
                Ok(s) => CtValue::ResOk(Box::new(CtValue::Str(s))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
        "append_event" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let event = match one(1)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("event", span)),
            };
            Ok(match jet_services_append_event(&mut tree, event) {
                Ok(()) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
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
                Ok(run_id) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Int(run_id)))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
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
                Ok(()) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
        "workflow_history" => {
            let tree = ct_to_tree(one(0)?, span)?;
            let run_id = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("run id", span)),
            };
            Ok(match jet_services_workflow_history(&tree, run_id) {
                Ok(s) => CtValue::ResOk(Box::new(CtValue::Str(s))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
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
                Ok(()) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
        "directory_resolve" => {
            let tree = ct_to_tree(one(0)?, span)?;
            let name = match one(1)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("directory name", span)),
            };
            Ok(match jet_services_directory_resolve(&tree, &name) {
                Ok(ep) => CtValue::ResOk(Box::new(endpoint_to_ct(&ep))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
        "directory_generation" => Ok(CtValue::Int(jet_services_directory_generation(
            &ct_to_tree(one(0)?, span)?,
        ))),
        "drain_worker" => {
            let mut tree = ct_to_tree(one(0)?, span)?;
            let endpoint = ct_to_endpoint(one(1)?, span)?;
            Ok(match jet_services_drain_worker(&mut tree, &endpoint) {
                Ok(()) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Unit))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
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
                Ok(n) => CtValue::ResOk(Box::new(mutate_ok(tree, CtValue::Int(n)))),
                Err(e) => CtValue::ResErr(Box::new(map_err(e))),
            })
        }
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
