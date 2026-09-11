//! Web dev-server state, terminal/browser parity UI, routes, live reload, and
//! last-good artifact swapping — std-only (I6: no `notify`, HTTP-server, or
//! WebSocket crate).
//!
//! This is a different execution model from native `jet dev`, which
//! interprets/hot-swaps the program in process. Compiling to JS/WASM still
//! requires a rebuild, but the browser receives a typed module-swap receipt
//! after each accepted build so its runtime can preserve module-owned state
//! without a host-forced page restart. The mtime-poll watch pattern (and the
//! `file_mtime` helper itself) is shared with `run_dev`, not its interpreter or
//! JIT machinery.

use std::collections::HashMap;
use std::fs;
use std::io::{BufReader, IsTerminal, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::TerminalStyleReload::{
    TerminalCapabilityFacts, TerminalColorMode, TerminalStyleHostAdapter, TerminalStyleHostSource,
};
use jet_driver::Diagnostics::ColorChoice;
use jet_driver::Package::ReleaseDevtoolsPolicy;
use jet_foundation::DataTree::DataTree;
use jet_foundation::Devtools::{
    JetDevtoolsEnvelope, JetDevtoolsEvent, JetDevtoolsEventBody, JetDevtoolsFormState,
    JetDevtoolsFreshnessFact, JetDevtoolsFreshnessState, JetDevtoolsFunctionIdentity,
    JetDevtoolsGameLaunchPhase, JetDevtoolsLifecycleState, JetDevtoolsMutationLifecycle,
    JetDevtoolsPrivacy, JetDevtoolsQueryState, JetDevtoolsSourceIdentityFact,
    JetDevtoolsSourceSpan, JetDevtoolsTraceKind, JET_DEVTOOLS_MAX_ENVELOPE_BYTES,
    JET_DEVTOOLS_MAX_EVENTS,
};
use jet_foundation::DevtoolsControl::{
    JetDevtoolsCommand, JetDevtoolsCommandEnvelope, JetDevtoolsDatabaseExplainRequest,
    JetDevtoolsDecodedEnvelope, JetDevtoolsGameControlKind, JetDevtoolsGameControlRequest,
    JetDevtoolsProjectRebuildRequest,
};
use jet_foundation::HotSwap::HotSwapDecision;
use jet_foundation::JSON::{json_escape, parse_json_with_limit};

use crate::{
    content_type_for, query_param, read_static_file_bounded, static_relative_path, write_response,
    Request, MAX_REQUEST_BODY_BYTES,
};
#[path = "BrowserWebAdapter.rs"]
mod browser_web_adapter;

/// Application preview ports tried, in order, before giving up. 8080 is the
/// conventional static-dev-server port; a small bounded scan upward covers
/// "something else is already on 8080" without hunting indefinitely.
const APPLICATION_PORT_RANGE: std::ops::RangeInclusive<u16> = 8080..=8089;
const CANVAS_SESSION_BYTES: usize = 32;
const MAX_CONNECTION_THREADS: usize = 64;

/// How often the live-reload script in the browser polls `/__jet_dev_status`.
/// The watcher and in-process rebuild already fit inside the warm budget; this
/// short poll closes the remaining edit-to-visible gap without a refresh.
const LIVE_RELOAD_POLL_MS: u64 = 40;
const CLIENT_TTL_MS: u64 = crate::Session::CLIENT_TTL_MS;
const MAX_CLIENTS: usize = crate::Session::MAX_CLIENTS;
const REQUEST_DEADLINE: Duration = Duration::from_secs(10);
const MAX_STATIC_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;
static PUBLICATION_COUNTER: AtomicU64 = AtomicU64::new(0);
pub(crate) fn decode_devtools_envelope(
    payload: &str,
) -> Result<JetDevtoolsDecodedEnvelope, String> {
    let root = parse_json_with_limit(payload, JET_DEVTOOLS_MAX_ENVELOPE_BYTES)
        .map_err(|_| "devtools envelope is not valid bounded JSON".to_string())?;
    let object =
        object_map(&root).ok_or_else(|| "devtools envelope must be an object".to_string())?;
    let protocol = devtools_required_string(&object, "protocol")?;
    if protocol != jet_foundation::Devtools::JET_DEVTOOLS_PROTOCOL {
        return Err("devtools envelope protocol does not match jet.devtools.v1".to_string());
    }
    if object.contains_key("history") {
        return Err("devtools envelope must use the single `events` stream".to_string());
    }
    let session_id = devtools_required_string(&object, "session_id")?;
    let started_at_ms = devtools_required_u64(&object, "started_at_ms")?;
    match object.get("direction") {
        Some(direction) => match devtools_required_value_string(direction, "direction")?.as_str() {
            "host_to_runtime" => {
                if object.contains_key("events") {
                    return Err(
                        "devtools command envelope cannot contain an events stream".to_string()
                    );
                }
                Ok(JetDevtoolsDecodedEnvelope::Commands(
                    decode_devtools_command_envelope(&object, &session_id, started_at_ms)?,
                ))
            }
            "runtime_to_host" => {
                if object.contains_key("commands") {
                    return Err(
                        "devtools event envelope cannot contain a commands stream".to_string()
                    );
                }
                Ok(JetDevtoolsDecodedEnvelope::Events(decode_devtools_events(
                    &object,
                    &session_id,
                    started_at_ms,
                )?))
            }
            _ => Err("devtools envelope direction is not recognized".to_string()),
        },
        None => {
            if object.contains_key("commands") {
                return Err("devtools command envelope requires direction".to_string());
            }
            Ok(JetDevtoolsDecodedEnvelope::Events(decode_devtools_events(
                &object,
                &session_id,
                started_at_ms,
            )?))
        }
    }
}

fn decode_devtools_events(
    object: &std::collections::BTreeMap<String, DataTree>,
    session_id: &str,
    started_at_ms: u64,
) -> Result<JetDevtoolsEnvelope, String> {
    let events = match object.get("events") {
        Some(DataTree::Array(events)) => events,
        _ => return Err("devtools envelope `events` must be an array".to_string()),
    };
    if events.len() > JET_DEVTOOLS_MAX_EVENTS {
        return Err("devtools envelope exceeds the retained event limit".to_string());
    }
    let mut envelope = JetDevtoolsEnvelope::new(session_id.to_string(), started_at_ms);
    for value in events {
        let event = object_map(value)
            .ok_or_else(|| "devtools envelope event must be an object".to_string())?;
        let sequence = devtools_required_u64(&event, "sequence")?;
        if sequence == 0 {
            return Err("devtools envelope event sequence must be positive".to_string());
        }
        let timestamp_ms = devtools_required_u64(&event, "timestamp_ms")?;
        let source = devtools_required_string(&event, "source")?;
        let kind = devtools_required_string(&event, "kind")?;
        let entity = devtools_required_string(&event, "entity")?;
        let fields_value = event
            .get("fields")
            .ok_or_else(|| "devtools envelope missing `fields`".to_string())?;
        object_map(fields_value)
            .ok_or_else(|| "devtools envelope `fields` must be an object".to_string())?;
        let fields = crate::Session::render_json(fields_value);
        let payload = match event.get("payload") {
            None | Some(DataTree::Null) => None,
            Some(DataTree::Object(_)) => Some(devtools_required_object_json(&event, "payload")?),
            Some(_) => return Err("devtools event payload must be an object or null".to_string()),
        };
        let event = match decode_devtools_body(&kind, &entity, fields_value)? {
            Some(body) => {
                let typed = JetDevtoolsEvent::from_wire(
                    sequence,
                    timestamp_ms,
                    source,
                    body,
                    fields,
                    payload,
                )?;
                if typed.kind() != kind || typed.entity() != entity {
                    return Err("devtools event identity does not match its typed body".to_string());
                }
                typed
            }
            None => JetDevtoolsEvent::from_parts_with_payload(
                timestamp_ms,
                source,
                kind,
                entity,
                fields,
                payload,
            )?,
        };
        envelope.push(event);
    }
    if let Some(selection) = object.get("selection") {
        match selection {
            DataTree::Null => envelope.set_selection(None),
            DataTree::Object(selection) => {
                let selection = object_map_entries(selection);
                let panel_id = devtools_required_string(&selection, "panel_id")?;
                let item_key = devtools_optional_string(&selection, "item_key")?;
                envelope.set_selection(Some(jet_foundation::Devtools::JetDevtoolsSelection::new(
                    panel_id, item_key,
                )));
            }
            _ => return Err("devtools envelope selection must be an object or null".to_string()),
        }
    }
    if let Some(cursor) = object.get("cursor") {
        envelope.set_cursor(match cursor {
            DataTree::Null => None,
            DataTree::Int(value) if *value >= 0 => Some(*value as u64),
            DataTree::Float(value)
                if value.fract() == 0.0 && *value >= 0.0 && *value <= u64::MAX as f64 =>
            {
                Some(*value as u64)
            }
            _ => return Err("devtools envelope cursor must be a sequence".to_string()),
        });
    }
    if let Some(lifecycle) = object.get("lifecycle") {
        let lifecycle = devtools_required_value_string(lifecycle, "lifecycle")?;
        envelope.set_lifecycle(devtools_lifecycle(&lifecycle)?);
    }
    if let Some(freshness) = object.get("freshness") {
        let freshness = object_map(freshness)
            .ok_or_else(|| "devtools envelope freshness must be an object".to_string())?;
        let state = devtools_required_string(&freshness, "state")?;
        let observed_at_ms = devtools_required_u64(&freshness, "observed_at_ms")?;
        envelope.set_freshness(JetDevtoolsFreshnessFact {
            state: devtools_freshness_state(&state)?,
            observed_at_ms,
        });
    }
    if let Some(privacy) = object.get("privacy") {
        let privacy = object_map(privacy)
            .ok_or_else(|| "devtools envelope privacy must be an object".to_string())?;
        envelope.privacy = JetDevtoolsPrivacy {
            loopback_only: devtools_required_bool(&privacy, "loopback_only")?,
            payload_free_by_default: devtools_required_bool(&privacy, "payload_free_by_default")?,
            published_payloads: devtools_required_bool(&privacy, "published_payloads")?,
        };
    }
    if let Some(identity) = object.get("source_identity") {
        envelope.source_identity = match identity {
            DataTree::Null => None,
            DataTree::Object(identity) => {
                let identity = object_map_entries(identity);
                Some(JetDevtoolsSourceIdentityFact::new(
                    devtools_optional_string(&identity, "source_id")?,
                    devtools_optional_string(&identity, "build_id")?,
                    devtools_optional_string(&identity, "revision")?,
                    devtools_optional_string(&identity, "world_id")?,
                ))
            }
            _ => {
                return Err(
                    "devtools envelope source_identity must be an object or null".to_string(),
                )
            }
        };
    }
    if object.contains_key("reset") {
        envelope.reset = devtools_required_bool(object, "reset")?;
    }
    if object.contains_key("truncated") {
        envelope.truncated = devtools_required_bool(object, "truncated")?;
    }
    Ok(envelope)
}

fn decode_devtools_command_envelope(
    object: &std::collections::BTreeMap<String, DataTree>,
    session_id: &str,
    started_at_ms: u64,
) -> Result<JetDevtoolsCommandEnvelope, String> {
    let expected = [
        "protocol",
        "session_id",
        "started_at_ms",
        "direction",
        "commands",
    ];
    if object.len() != expected.len()
        || !object
            .keys()
            .all(|key| expected.iter().any(|expected_key| key == expected_key))
    {
        return Err("devtools command envelope has unexpected top-level fields".to_string());
    }
    let direction = devtools_required_value_string(
        object
            .get("direction")
            .ok_or_else(|| "devtools command envelope requires direction".to_string())?,
        "direction",
    )?;
    if direction != "host_to_runtime" {
        return Err("devtools command envelope direction must be host_to_runtime".to_string());
    }
    let commands = match object.get("commands") {
        Some(DataTree::Array(commands)) => commands,
        _ => return Err("devtools envelope `commands` must be an array".to_string()),
    };
    if commands.len() > jet_foundation::DevtoolsControl::JET_DEVTOOLS_MAX_COMMANDS {
        return Err("devtools envelope exceeds the retained command limit".to_string());
    }
    let mut envelope = JetDevtoolsCommandEnvelope::new(session_id.to_string(), started_at_ms);
    for value in commands {
        let command = object_map(value)
            .ok_or_else(|| "devtools envelope command must be an object".to_string())?;
        if command.len() != 2 || !command.contains_key("kind") || !command.contains_key("payload") {
            return Err("devtools envelope command must contain only kind and payload".to_string());
        }
        let kind = devtools_required_string(&command, "kind")?;
        let payload = command
            .get("payload")
            .and_then(object_map)
            .ok_or_else(|| "devtools envelope command payload must be an object".to_string())?;
        let payload = &payload;
        match kind.as_str() {
            "DatabaseExplain" => {
                let expected = [
                    "session_id",
                    "request_id",
                    "statement_identity",
                    "timeout_ms",
                    "max_rows",
                ];
                if payload.len() != expected.len()
                    || !payload
                        .keys()
                        .all(|key| expected.iter().any(|expected_key| key == expected_key))
                {
                    return Err(
                        "devtools envelope database command payload has unexpected fields"
                            .to_string(),
                    );
                }
                let request = JetDevtoolsDatabaseExplainRequest::new(
                    devtools_required_string(payload, "session_id")?,
                    devtools_required_string(payload, "request_id")?,
                    devtools_required_string(payload, "statement_identity")?,
                    devtools_required_u64(payload, "timeout_ms")?,
                    devtools_required_u64(payload, "max_rows")?,
                )?;
                envelope.enqueue_command(JetDevtoolsCommand::DatabaseExplain(request))?;
            }
            "GameControl" => {
                let expected = [
                    "session_id",
                    "request_id",
                    "kind",
                    "source_id",
                    "revision",
                    "world_id",
                    "actor_id",
                    "component_id",
                    "authored_instance_id",
                    "source_span_start",
                    "source_span_end",
                    "field",
                    "value",
                    "category",
                    "expression",
                    "required_authority",
                    "frame_id",
                    "budget",
                ];
                if payload.len() != expected.len()
                    || !payload
                        .keys()
                        .all(|key| expected.iter().any(|expected_key| key == expected_key))
                {
                    return Err(
                        "devtools envelope game control payload has unexpected fields".to_string(),
                    );
                }
                let request = JetDevtoolsGameControlRequest {
                    session_id: devtools_required_string(payload, "session_id")?,
                    request_id: devtools_required_string(payload, "request_id")?,
                    kind: JetDevtoolsGameControlKind::from_str(&devtools_required_string(
                        payload, "kind",
                    )?)?,
                    source_id: devtools_optional_string(payload, "source_id")?,
                    revision: devtools_optional_string(payload, "revision")?,
                    world_id: devtools_optional_string(payload, "world_id")?,
                    actor_id: devtools_optional_string(payload, "actor_id")?,
                    component_id: devtools_optional_string(payload, "component_id")?,
                    authored_instance_id: devtools_optional_string(
                        payload,
                        "authored_instance_id",
                    )?,
                    source_span_start: devtools_optional_u64(payload, "source_span_start")?,
                    source_span_end: devtools_optional_u64(payload, "source_span_end")?,
                    field: devtools_optional_string(payload, "field")?,
                    value: devtools_optional_string(payload, "value")?,
                    category: devtools_optional_string(payload, "category")?,
                    expression: devtools_optional_string(payload, "expression")?,
                    required_authority: devtools_optional_string(payload, "required_authority")?,
                    frame_id: devtools_optional_u64(payload, "frame_id")?,
                    budget: devtools_optional_u64(payload, "budget")?,
                };
                request.validate()?;
                envelope.enqueue_command(JetDevtoolsCommand::GameControl(request))?;
            }
            "ProjectRebuild" => {
                let expected = ["session_id", "request_id", "expected_revision"];
                if payload.len() != expected.len()
                    || !payload
                        .keys()
                        .all(|key| expected.iter().any(|expected_key| key == expected_key))
                {
                    return Err(
                        "devtools envelope project rebuild payload has unexpected fields"
                            .to_string(),
                    );
                }
                let request = JetDevtoolsProjectRebuildRequest::new(
                    devtools_required_string(payload, "session_id")?,
                    devtools_required_string(payload, "request_id")?,
                    devtools_required_string(payload, "expected_revision")?,
                )?;
                envelope.enqueue_command(JetDevtoolsCommand::ProjectRebuild(request))?;
            }
            _ => {
                return Err("devtools envelope contains an unsupported command kind".to_string());
            }
        }
    }
    envelope.validate()?;
    Ok(envelope)
}

fn decode_devtools_body(
    kind: &str,
    _entity: &str,
    fields: &DataTree,
) -> Result<Option<JetDevtoolsEventBody>, String> {
    let object =
        object_map(fields).ok_or_else(|| "devtools event fields must be an object".to_string())?;
    let object = &object;
    if let Some(expected) = devtools_expected_fields(kind) {
        if !devtools_has_exact_fields(object, expected) {
            return Ok(None);
        }
    }
    let body = match kind {
        "Build" => {
            devtools_exact_fields(object, &["status", "tier", "diagnostics", "kept", "reset"])?;
            JetDevtoolsEventBody::Build {
                status: devtools_required_string(object, "status")?,
                tier: devtools_required_string(object, "tier")?,
                diagnostics: devtools_required_string_array(object, "diagnostics")?,
                kept: devtools_required_u64(object, "kept")?,
                reset: devtools_required_u64(object, "reset")?,
            }
        }
        "Route" => {
            devtools_exact_fields(object, &["path", "matched", "render_mode", "reason"])?;
            JetDevtoolsEventBody::Route {
                path: devtools_required_string(object, "path")?,
                matched: devtools_required_bool(object, "matched")?,
                render_mode: devtools_required_string(object, "render_mode")?,
                reason: devtools_required_string(object, "reason")?,
            }
        }
        "Query" => {
            devtools_exact_fields(object, &["name", "state", "footprint"])?;
            let state = devtools_required_string(object, "state")?;
            JetDevtoolsEventBody::Query {
                name: devtools_required_string(object, "name")?,
                state: match state.as_str() {
                    "fetching" => JetDevtoolsQueryState::Fetching,
                    "fresh" => JetDevtoolsQueryState::Fresh,
                    "stale" => JetDevtoolsQueryState::Stale,
                    "invalidated" => JetDevtoolsQueryState::Invalidated,
                    _ => return Err("devtools Query state is not recognized".to_string()),
                },
                footprint: devtools_required_u64(object, "footprint")?,
            }
        }
        "Mutation" => {
            devtools_exact_fields(object, &["name", "lifecycle", "rollback"])?;
            let lifecycle = devtools_required_string(object, "lifecycle")?;
            JetDevtoolsEventBody::Mutation {
                name: devtools_required_string(object, "name")?,
                lifecycle: match lifecycle.as_str() {
                    "started" => JetDevtoolsMutationLifecycle::Started,
                    "committed" => JetDevtoolsMutationLifecycle::Committed,
                    "failed" => JetDevtoolsMutationLifecycle::Failed,
                    "rolled_back" => JetDevtoolsMutationLifecycle::RolledBack,
                    _ => return Err("devtools Mutation lifecycle is not recognized".to_string()),
                },
                rollback: devtools_required_bool(object, "rollback")?,
            }
        }
        "Form" => {
            devtools_exact_fields(object, &["name", "field", "state", "validation"])?;
            let state = devtools_required_string(object, "state")?;
            JetDevtoolsEventBody::Form {
                name: devtools_required_string(object, "name")?,
                field: devtools_required_string(object, "field")?,
                state: match state.as_str() {
                    "pristine" => JetDevtoolsFormState::Pristine,
                    "dirty" => JetDevtoolsFormState::Dirty,
                    "valid" => JetDevtoolsFormState::Valid,
                    "invalid" => JetDevtoolsFormState::Invalid,
                    _ => return Err("devtools Form state is not recognized".to_string()),
                },
                validation: devtools_required_string(object, "validation")?,
            }
        }
        "Table" => {
            devtools_exact_fields(object, &["name", "viewport", "sort", "filter"])?;
            JetDevtoolsEventBody::Table {
                name: devtools_required_string(object, "name")?,
                viewport: devtools_required_u64(object, "viewport")?,
                sort: devtools_required_string(object, "sort")?,
                filter: devtools_required_string(object, "filter")?,
            }
        }
        "Store" => {
            devtools_exact_fields(object, &["name", "transaction", "diff", "cursor"])?;
            JetDevtoolsEventBody::Store {
                name: devtools_required_string(object, "name")?,
                transaction: devtools_required_string(object, "transaction")?,
                diff: devtools_required_string(object, "diff")?,
                cursor: devtools_required_u64(object, "cursor")?,
            }
        }
        "Trace" => {
            devtools_exact_fields(
                object,
                &["name", "kind", "request", "duration_ms", "status"],
            )?;
            let trace_kind = devtools_required_string(object, "kind")?;
            JetDevtoolsEventBody::Trace {
                name: devtools_required_string(object, "name")?,
                kind: match trace_kind.as_str() {
                    "request" => JetDevtoolsTraceKind::Request,
                    "render" => JetDevtoolsTraceKind::Render,
                    "job" => JetDevtoolsTraceKind::Job,
                    "reload" => JetDevtoolsTraceKind::Reload,
                    "frame" => JetDevtoolsTraceKind::Frame,
                    _ => return Err("devtools Trace kind is not recognized".to_string()),
                },
                request: devtools_required_string(object, "request")?,
                duration_ms: devtools_required_u64(object, "duration_ms")?,
                status: devtools_required_string(object, "status")?,
            }
        }
        "Cost" => {
            devtools_exact_fields(object, &["name", "rule", "bytes", "allocations"])?;
            JetDevtoolsEventBody::Cost {
                name: devtools_required_string(object, "name")?,
                rule: devtools_required_string(object, "rule")?,
                bytes: devtools_required_u64(object, "bytes")?,
                allocations: devtools_required_u64(object, "allocations")?,
            }
        }
        "Gate" => {
            devtools_exact_fields(object, &["name", "status", "detail"])?;
            JetDevtoolsEventBody::Gate {
                name: devtools_required_string(object, "name")?,
                status: devtools_required_string(object, "status")?,
                detail: devtools_required_string(object, "detail")?,
            }
        }
        "Structure" => {
            devtools_exact_fields(object, &["name", "kind", "status"])?;
            JetDevtoolsEventBody::Structure {
                name: devtools_required_string(object, "name")?,
                kind: devtools_required_string(object, "kind")?,
                status: devtools_required_string(object, "status")?,
            }
        }
        "Test" => {
            devtools_exact_fields(object, &["name", "status", "duration_ms", "diagnostics"])?;
            JetDevtoolsEventBody::Test {
                name: devtools_required_string(object, "name")?,
                status: devtools_required_string(object, "status")?,
                duration_ms: devtools_required_u64(object, "duration_ms")?,
                diagnostics: devtools_required_string(object, "diagnostics")?,
            }
        }
        "Job" => {
            devtools_exact_fields(object, &["name", "status", "duration_ms", "diagnostics"])?;
            JetDevtoolsEventBody::Job {
                name: devtools_required_string(object, "name")?,
                status: devtools_required_string(object, "status")?,
                duration_ms: devtools_required_u64(object, "duration_ms")?,
                diagnostics: devtools_required_string(object, "diagnostics")?,
            }
        }
        "Frame" => {
            devtools_exact_fields(object, &["name", "status", "duration_ms"])?;
            JetDevtoolsEventBody::Frame {
                name: devtools_required_string(object, "name")?,
                status: devtools_required_string(object, "status")?,
                duration_ms: devtools_required_u64(object, "duration_ms")?,
            }
        }
        "GameFrameSample" => {
            devtools_exact_fields(
                object,
                &[
                    "sequence",
                    "frame_id",
                    "scene",
                    "frame_index",
                    "build",
                    "revision",
                    "trace_id",
                    "start_ns",
                    "cpu_ns",
                    "gpu_ns",
                    "responsible_function",
                ],
            )?;
            JetDevtoolsEventBody::GameFrameSample {
                sequence: devtools_required_u64(object, "sequence")?,
                frame_id: devtools_required_u64(object, "frame_id")?,
                scene: devtools_required_string(object, "scene")?,
                frame_index: devtools_required_u64(object, "frame_index")?,
                build: devtools_required_string(object, "build")?,
                revision: devtools_required_string(object, "revision")?,
                trace_id: devtools_required_string(object, "trace_id")?,
                start_ns: devtools_required_u64(object, "start_ns")?,
                cpu_ns: devtools_required_u64(object, "cpu_ns")?,
                gpu_ns: devtools_optional_u64(object, "gpu_ns")?,
                responsible_function: devtools_optional_function(object, "responsible_function")?,
            }
        }
        "GameDrawEvent" => {
            devtools_exact_fields(
                object,
                &[
                    "sequence",
                    "frame_id",
                    "scene",
                    "frame_index",
                    "build",
                    "revision",
                    "trace_id",
                    "event_index",
                    "event_id",
                    "function",
                    "phase",
                    "domain",
                    "start_ns",
                    "duration_ns",
                    "object_id",
                    "resource_id",
                ],
            )?;
            JetDevtoolsEventBody::GameDrawEvent {
                sequence: devtools_required_u64(object, "sequence")?,
                frame_id: devtools_required_u64(object, "frame_id")?,
                scene: devtools_required_string(object, "scene")?,
                frame_index: devtools_required_u64(object, "frame_index")?,
                build: devtools_required_string(object, "build")?,
                revision: devtools_required_string(object, "revision")?,
                trace_id: devtools_required_string(object, "trace_id")?,
                event_index: devtools_required_u64(object, "event_index")?,
                event_id: devtools_required_u64(object, "event_id")?,
                function: devtools_required_function(object, "function")?,
                phase: devtools_required_string(object, "phase")?,
                domain: devtools_required_string(object, "domain")?,
                start_ns: devtools_required_u64(object, "start_ns")?,
                duration_ns: devtools_required_u64(object, "duration_ns")?,
                object_id: devtools_optional_string(object, "object_id")?,
                resource_id: devtools_optional_string(object, "resource_id")?,
            }
        }
        "GameLaunchProfile" => {
            devtools_exact_fields(
                object,
                &["target", "run_mode", "headless_compatible", "phases"],
            )?;
            JetDevtoolsEventBody::GameLaunchProfile {
                target: devtools_required_string(object, "target")?,
                run_mode: devtools_required_string(object, "run_mode")?,
                headless_compatible: devtools_required_bool(object, "headless_compatible")?,
                phases: devtools_required_game_launch_phases(object, "phases")?,
            }
        }
        "GameSwap" => {
            devtools_exact_fields(
                object,
                &[
                    "sequence",
                    "path",
                    "kind",
                    "status",
                    "old_schema_id",
                    "new_schema_id",
                    "migration",
                    "reason",
                ],
            )?;
            let kind = devtools_required_string(object, "kind")?;
            if !matches!(kind.as_str(), "asset" | "script" | "world") {
                return Err("devtools GameSwap kind is not recognized".to_string());
            }
            let status = devtools_required_string(object, "status")?;
            if !matches!(status.as_str(), "applied" | "rejected") {
                return Err("devtools GameSwap status is not recognized".to_string());
            }
            let migration = devtools_required_string(object, "migration")?;
            if !matches!(
                migration.as_str(),
                "preserve" | "migrate" | "reset" | "reject"
            ) {
                return Err("devtools GameSwap migration is not recognized".to_string());
            }
            JetDevtoolsEventBody::GameSwap {
                sequence: devtools_required_u64(object, "sequence")?,
                path: devtools_required_string(object, "path")?,
                kind,
                status,
                old_schema_id: devtools_optional_string(object, "old_schema_id")?,
                new_schema_id: devtools_optional_string(object, "new_schema_id")?,
                migration,
                reason: devtools_required_string(object, "reason")?,
            }
        }
        "Request" => {
            devtools_exact_fields(object, &["id", "method", "path", "status"])?;
            JetDevtoolsEventBody::Request {
                id: devtools_required_string(object, "id")?,
                method: devtools_required_string(object, "method")?,
                path: devtools_required_string(object, "path")?,
                status: devtools_required_u16(object, "status")?,
            }
        }
        "Response" => {
            devtools_exact_fields(object, &["id", "status", "bytes"])?;
            JetDevtoolsEventBody::Response {
                id: devtools_required_string(object, "id")?,
                status: devtools_required_u16(object, "status")?,
                bytes: devtools_required_u64(object, "bytes")?,
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(body))
}

fn devtools_expected_fields(kind: &str) -> Option<&'static [&'static str]> {
    match kind {
        "Build" => Some(&["status", "tier", "diagnostics", "kept", "reset"]),
        "Route" => Some(&["path", "matched", "render_mode", "reason"]),
        "Query" => Some(&["name", "state", "footprint"]),
        "GameSwap" => Some(&[
            "sequence",
            "path",
            "kind",
            "status",
            "old_schema_id",
            "new_schema_id",
            "migration",
            "reason",
        ]),
        "Request" => Some(&["id", "method", "path", "status"]),
        "Store" => Some(&["name", "transaction", "diff", "cursor"]),
        "Trace" => Some(&["name", "kind", "request", "duration_ms", "status"]),
        "Cost" => Some(&["name", "rule", "bytes", "allocations"]),
        "Gate" => Some(&["name", "status", "detail"]),
        "Structure" => Some(&["name", "kind", "status"]),
        "Test" | "Job" => Some(&["name", "status", "duration_ms", "diagnostics"]),
        "Frame" => Some(&["name", "status", "duration_ms"]),
        "GameFrameSample" => Some(&[
            "sequence",
            "frame_id",
            "scene",
            "frame_index",
            "build",
            "revision",
            "trace_id",
            "start_ns",
            "cpu_ns",
            "gpu_ns",
            "responsible_function",
        ]),
        "GameDrawEvent" => Some(&[
            "sequence",
            "frame_id",
            "scene",
            "frame_index",
            "build",
            "revision",
            "trace_id",
            "event_index",
            "event_id",
            "function",
            "phase",
            "domain",
            "start_ns",
            "duration_ns",
            "object_id",
            "resource_id",
        ]),
        "GameLaunchProfile" => Some(&["target", "run_mode", "headless_compatible", "phases"]),
        "Response" => Some(&["id", "status", "bytes"]),
        _ => None,
    }
}

fn object_map(value: &DataTree) -> Option<std::collections::BTreeMap<String, DataTree>> {
    match value {
        DataTree::Object(fields) => Some(fields.iter().cloned().collect()),
        _ => None,
    }
}

fn object_map_entries(
    fields: &[(String, DataTree)],
) -> std::collections::BTreeMap<String, DataTree> {
    fields.iter().cloned().collect()
}

fn devtools_has_exact_fields(
    object: &std::collections::BTreeMap<String, DataTree>,
    expected: &[&str],
) -> bool {
    object.len() == expected.len()
        && object
            .keys()
            .all(|key| expected.iter().any(|name| *name == key))
}

fn devtools_exact_fields(
    object: &std::collections::BTreeMap<String, DataTree>,
    expected: &[&str],
) -> Result<(), String> {
    if !devtools_has_exact_fields(object, expected) {
        return Err("devtools event fields do not match the typed body".to_string());
    }
    Ok(())
}

fn devtools_required_game_launch_phases(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<Vec<JetDevtoolsGameLaunchPhase>, String> {
    let values = match object.get(key) {
        Some(DataTree::Array(values)) => values,
        _ => return Err(format!("devtools event `{key}` must be an array")),
    };
    if values.len() > 256 {
        return Err(format!("devtools event `{key}` exceeds 256 entries"));
    }
    values
        .iter()
        .map(|value| {
            let phase = object_map(value)
                .ok_or_else(|| format!("devtools event `{key}` must contain objects"))?;
            let phase = &phase;
            devtools_exact_fields(phase, &["phase", "status", "detail"])?;
            Ok(JetDevtoolsGameLaunchPhase::new(
                devtools_required_string(phase, "phase")?,
                devtools_required_string(phase, "status")?,
                devtools_required_string(phase, "detail")?,
            ))
        })
        .collect()
}
fn devtools_required_string_array(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<Vec<String>, String> {
    let values = match object.get(key) {
        Some(DataTree::Array(values)) => values,
        _ => return Err(format!("devtools event `{key}` must be an array")),
    };
    values
        .iter()
        .map(|value| match value {
            DataTree::Text(value) if !value.is_empty() => Ok(value.clone()),
            DataTree::Text(_) => Err(format!("devtools event `{key}` has an empty string")),
            _ => Err(format!("devtools event `{key}` must contain strings")),
        })
        .collect()
}

fn devtools_required_u16(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<u16, String> {
    u16::try_from(devtools_required_u64(object, key)?)
        .map_err(|_| format!("devtools event `{key}` is outside the u16 range"))
}

fn devtools_optional_u64(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<Option<u64>, String> {
    match object.get(key) {
        Some(DataTree::Null) | None => Ok(None),
        Some(DataTree::Int(value)) if *value >= 0 => u64::try_from(*value)
            .map(Some)
            .map_err(|_| format!("devtools event `{key}` must be non-negative")),
        Some(DataTree::Int(_)) => Err(format!("devtools event `{key}` must be non-negative")),
        Some(_) => Err(format!("devtools event `{key}` must be an integer or null")),
    }
}

fn devtools_required_function(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<JetDevtoolsFunctionIdentity, String> {
    let function = object
        .get(key)
        .and_then(object_map)
        .ok_or_else(|| format!("devtools event `{key}` must be an object"))?;
    let source = function
        .get("source")
        .and_then(object_map)
        .ok_or_else(|| format!("devtools event `{key}.source` must be an object"))?;
    Ok(JetDevtoolsFunctionIdentity::new(
        devtools_required_string(&function, "function_id")?,
        devtools_required_string(&function, "name")?,
        JetDevtoolsSourceSpan::new(
            devtools_required_string(&source, "source_id")?,
            devtools_required_string(&source, "file")?,
            devtools_required_u32(&source, "start_line")?,
            devtools_required_u32(&source, "start_column")?,
            devtools_required_u32(&source, "end_line")?,
            devtools_required_u32(&source, "end_column")?,
        ),
    ))
}

fn devtools_optional_function(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<Option<JetDevtoolsFunctionIdentity>, String> {
    match object.get(key) {
        None | Some(DataTree::Null) => Ok(None),
        Some(DataTree::Object(_)) => devtools_required_function(object, key).map(Some),
        Some(_) => Err(format!("devtools event `{key}` must be an object or null")),
    }
}

fn devtools_required_u32(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<u32, String> {
    u32::try_from(devtools_required_u64(object, key)?)
        .map_err(|_| format!("devtools event `{key}` is outside the u32 range"))
}

fn devtools_required_string(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<String, String> {
    let value = object
        .get(key)
        .ok_or_else(|| format!("devtools envelope missing `{key}`"))?;
    let DataTree::Text(value) = value else {
        return Err(format!("devtools envelope `{key}` must be a string"));
    };
    if value.is_empty() {
        return Err(format!("devtools envelope `{key}` must not be empty"));
    }
    Ok(value.clone())
}

fn devtools_required_value_string(value: &DataTree, key: &str) -> Result<String, String> {
    let DataTree::Text(value) = value else {
        return Err(format!("devtools envelope `{key}` must be a string"));
    };
    if value.is_empty() {
        return Err(format!("devtools envelope `{key}` must not be empty"));
    }
    Ok(value.clone())
}

fn devtools_optional_string(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<Option<String>, String> {
    match object.get(key) {
        None | Some(DataTree::Null) => Ok(None),
        Some(DataTree::Text(value)) if !value.is_empty() => Ok(Some(value.clone())),
        Some(DataTree::Text(_)) => Err(format!("devtools envelope `{key}` must not be empty")),
        Some(_) => Err(format!(
            "devtools envelope `{key}` must be a string or null"
        )),
    }
}

fn devtools_required_u64(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<u64, String> {
    let value = object
        .get(key)
        .ok_or_else(|| format!("devtools envelope missing `{key}`"))?;
    match value {
        DataTree::Int(value) => u64::try_from(*value)
            .map_err(|_| format!("devtools envelope `{key}` must be non-negative")),
        _ => Err(format!("devtools envelope `{key}` must be an integer")),
    }
}

fn devtools_required_bool(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<bool, String> {
    match object.get(key) {
        Some(DataTree::Bool(value)) => Ok(*value),
        Some(_) => Err(format!("devtools envelope `{key}` must be a boolean")),
        None => Err(format!("devtools envelope missing `{key}`")),
    }
}

fn devtools_required_object_json(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<String, String> {
    let value = object
        .get(key)
        .ok_or_else(|| format!("devtools envelope missing `{key}`"))?;
    if !matches!(value, DataTree::Object(_)) {
        return Err(format!("devtools envelope `{key}` must be an object"));
    }
    Ok(crate::Session::render_json(value))
}

fn devtools_lifecycle(value: &str) -> Result<JetDevtoolsLifecycleState, String> {
    match value {
        "starting" => Ok(JetDevtoolsLifecycleState::Starting),
        "building" => Ok(JetDevtoolsLifecycleState::Building),
        "ready" => Ok(JetDevtoolsLifecycleState::Ready),
        "error" => Ok(JetDevtoolsLifecycleState::Error),
        "unavailable" => Ok(JetDevtoolsLifecycleState::Unavailable),
        "stopped" => Ok(JetDevtoolsLifecycleState::Stopped),
        _ => Err("devtools envelope lifecycle is not recognized".to_string()),
    }
}

fn devtools_freshness_state(value: &str) -> Result<JetDevtoolsFreshnessState, String> {
    match value {
        "fresh" => Ok(JetDevtoolsFreshnessState::Fresh),
        "stale" => Ok(JetDevtoolsFreshnessState::Stale),
        "unknown" => Ok(JetDevtoolsFreshnessState::Unknown),
        _ => Err("devtools envelope freshness state is not recognized".to_string()),
    }
}

fn terminal_style_adapter(file: &str) -> Result<Arc<TerminalStyleHostAdapter>, String> {
    let is_tty = std::io::stderr().is_terminal();
    let capabilities = TerminalCapabilityFacts::from_environment(
        std::env::var("TERM").ok().as_deref(),
        std::env::var("COLORTERM").ok().as_deref(),
        is_tty,
        std::env::var_os("NO_COLOR").is_some(),
        TerminalColorMode::Auto,
    );
    let source = TerminalStyleHostSource::new(file, capabilities)
        .map_err(|error| format!("could not inject terminal style source: {error}"))?;
    let adapter = Arc::new(TerminalStyleHostAdapter::new());
    adapter.inject(source);
    Ok(adapter)
}

/// D-FE-DEVSRV1 outcome D (hybrid, owner-modified 2026-07-08: pinned parity
/// header required as the terminal anchor): a one-line terminal status and a
/// browser corner strip mirror the exact same words from one shared status
/// (`header_words`/`json`'s `message` field) — they cannot drift because both
/// read the same source. On error both sides show the identical verbatim
/// diagnostic (I4): terminal frames it in a box (TTY) or prints it plain (CI
/// floor); the browser expands its strip into a full overlay. `--verbose`
/// (or `-v`) adds a request/rebuild log under the still-pinned header.
#[derive(Clone)]
struct DevStatusSnapshot {
    state: String,
    code: String,
    diagnostic: String,
    last_build_ms: u128,
}

struct DevStatus {
    version: AtomicU64,
    clients: Mutex<HashMap<String, Instant>>,
    state: Mutex<DevStatusSnapshot>,
    browser_relay: Mutex<Option<crate::BrowserTrace::Relay>>,
    browser_trace_enabled: AtomicBool,
    command_receipt: Mutex<Option<String>>,
    /// The file `jet dev` is watching — used in the `building` parity line
    /// and the `save <file> → …` verbose log lines.
    watched_file: String,
    /// Port shown in status surfaces: application preview for hybrid web
    /// sessions, or Canvas control for Canvas-only sessions.
    port: AtomicU64,
    /// Canvas control port shown in the terminal detail line.
    canvas_port: AtomicU64,
    /// Static generator previews use the application listener only and must
    /// not advertise a Canvas URL that is not bound.
    canvas_enabled: AtomicBool,
    /// `--verbose`/`-v`: opt-in request/rebuild log under the parity header
    /// (dashboard's depth, D-FE-DEVSRV1=D "on-demand depth").
    verbose: AtomicBool,
    active: AtomicBool,
    exit_code: AtomicU64,
    /// Set only after raw-mode input succeeds. Verbose DECSTBM pinning must
    /// not start before its Ctrl-C/EOF cleanup guard exists.
    controls_ready: AtomicBool,
    /// Losing every leased browser after at least one was connected is a
    /// shared reconnect state until the next lease renewal.
    reconnecting: AtomicBool,
    /// Color and cursor control are separate capabilities. `NO_COLOR`
    /// changes the dot into a bracketed state word, but a real TTY still
    /// gets the pinned dashboard and live `v` control.
    color: bool,
    /// Redraw the parity line in place instead of appending a new one each
    /// time. This depends only on stderr being a real TTY; color is cosmetic.
    /// Non-TTY pipes use the plain append-only CI floor.
    pin: bool,
    /// Serializes every write to stderr from the status/log renderer so
    /// concurrent request threads and the rebuild-watch thread never
    /// interleave partial ANSI escape sequences.
    term_lock: Mutex<()>,
    /// How many lines the last pinned (non-verbose) redraw occupied — lets
    /// the next redraw move the cursor up and erase exactly that block
    /// (parity line, or parity line + framed diagnostic).
    last_block_lines: Mutex<usize>,
    /// Whether the verbose pinned header's scroll region has been set up yet
    /// (`\x1b[3r`, cursor parked in the scrolling log area below two pinned
    /// dashboard rows).
    header_started: Mutex<bool>,
}

impl DevStatus {
    fn new(file: &str, verbose: bool) -> DevStatus {
        let is_tty = std::io::stderr().is_terminal();
        // `NO_COLOR`/`FORCE_COLOR` resolution — the same one every other jet
        // command uses. Pinning (cursor redraw) additionally requires a real
        // TTY: `FORCE_COLOR` off-TTY still can't redraw in place.
        let color = ColorChoice::Auto.resolve(is_tty);
        DevStatus::new_with_terminal(file, verbose, is_tty, color)
    }

    fn new_with_terminal(file: &str, verbose: bool, is_tty: bool, color: bool) -> DevStatus {
        DevStatus {
            version: AtomicU64::new(1),
            clients: Mutex::new(HashMap::new()),
            state: Mutex::new(DevStatusSnapshot {
                state: "ready".to_string(),
                code: String::new(),
                diagnostic: String::new(),
                last_build_ms: 0,
            }),
            browser_relay: Mutex::new(None),
            browser_trace_enabled: AtomicBool::new(false),
            command_receipt: Mutex::new(None),
            watched_file: file.to_string(),
            port: AtomicU64::new(0),
            canvas_port: AtomicU64::new(0),
            canvas_enabled: AtomicBool::new(true),
            verbose: AtomicBool::new(verbose),
            active: AtomicBool::new(false),
            exit_code: AtomicU64::new(0),
            controls_ready: AtomicBool::new(false),
            reconnecting: AtomicBool::new(false),
            color,
            pin: is_tty,
            term_lock: Mutex::new(()),
            last_block_lines: Mutex::new(0),
            header_started: Mutex::new(false),
        }
    }

    fn set_port(&self, port: u16) {
        self.port.store(port as u64, Ordering::SeqCst);
    }

    fn set_canvas_port(&self, port: u16) {
        self.canvas_port.store(port as u64, Ordering::SeqCst);
    }

    fn set_canvas_enabled(&self, enabled: bool) {
        self.canvas_enabled.store(enabled, Ordering::SeqCst);
    }

    fn port(&self) -> u16 {
        self.port.load(Ordering::SeqCst) as u16
    }

    fn canvas_port(&self) -> u16 {
        self.canvas_port.load(Ordering::SeqCst) as u16
    }

    fn header_text_for(&self, snap: &DevStatusSnapshot) -> (String, String) {
        if self.reconnecting.load(Ordering::SeqCst) {
            return (
                "reconnecting".to_string(),
                "waiting for connection".to_string(),
            );
        }
        header_words(
            &snap.state,
            &self.watched_file,
            &snap.code,
            self.port(),
            self.client_count(),
            snap.last_build_ms,
        )
    }

    fn format_line(&self, word: &str, rest: &str) -> String {
        if self.color {
            format_line_colored(word, rest)
        } else {
            format_line_plain(word, rest)
        }
    }

    fn dashboard_detail_line(&self) -> String {
        if self.canvas_enabled.load(Ordering::SeqCst) {
            format!(
                "         watching {} · Canvas http://localhost:{}/canvas · v verbose",
                self.watched_file,
                self.canvas_port()
            )
        } else {
            format!("         watching {} · v verbose", self.watched_file)
        }
    }

    /// Recompute the parity words from current state and redraw the
    /// terminal's live region (single source shared with `json()`'s
    /// `message` field — the browser strip renders that exact string).
    fn refresh(&self) {
        if !self.active.load(Ordering::SeqCst) {
            return;
        }
        let snap = self.state.lock().unwrap().clone();
        let (word, rest) = self.header_text_for(&snap);
        let line = self.format_line(&word, &rest);

        if self.verbose() {
            if self.pin {
                self.write_header_verbose(&line);
            } else {
                let _g = self.term_lock.lock().unwrap();
                let mut out = std::io::stderr();
                let _ = writeln!(out, "{}", line);
                let _ = out.flush();
            }
            return;
        }

        if self.pin {
            let mut lines = vec![line, self.dashboard_detail_line()];
            if snap.state == "error" {
                lines.extend(frame_lines(&snap.code, &snap.diagnostic));
            }
            self.write_block(&lines);
            return;
        }

        // CI floor: append-only, plain, no framing — the diagnostic prints
        // byte-for-byte what `render_diagnostics` produced (I4).
        let _g = self.term_lock.lock().unwrap();
        let mut out = std::io::stderr();
        let _ = writeln!(out, "{}", line);
        if snap.state == "error" {
            let _ = writeln!(out, "{}", snap.diagnostic);
        }
        let _ = out.flush();
    }

    /// Redraw a pinned (non-verbose) block in place: move the cursor up to
    /// the top of the previous block, erase to end of screen, print the new
    /// block. Cursor ends on the block's last line, no trailing newline —
    /// every caller (including the next redraw) relies on that invariant.
    fn write_block(&self, lines: &[String]) {
        let _g = self.term_lock.lock().unwrap();
        let mut out = std::io::stderr();
        let mut prev = self.last_block_lines.lock().unwrap();
        if *prev > 0 {
            if *prev > 1 {
                let _ = write!(out, "\x1b[{}A", *prev - 1);
            }
            let _ = write!(out, "\r\x1b[0J");
        }
        let _ = write!(out, "{}", lines.join("\n"));
        let _ = out.flush();
        *prev = lines.len();
    }

    /// Pin the two-row dashboard to terminal rows 1–2 via a DECSTBM scroll
    /// region (`\x1b[3r`) so `log_line` can print request/rebuild lines below
    /// it forever without touching the shared parity row or watched-target
    /// row. Std ANSI only (I6) — no terminal-
    /// size query, which is why the region's bottom is left at the display
    /// default instead of being computed.
    fn write_header_verbose(&self, line: &str) {
        let _g = self.term_lock.lock().unwrap();
        let mut out = std::io::stderr();
        // Ability check belongs inside the terminal lock. A refresh that
        // waited behind EOF/Ctrl-C must not reinstall DECSTBM after cleanup.
        if !self.controls_ready.load(Ordering::SeqCst) {
            let _ = writeln!(out, "{}", line);
            let _ = out.flush();
            return;
        }
        let mut started = self.header_started.lock().unwrap();
        let detail = self.dashboard_detail_line();
        if !*started {
            let _ = write!(out, "\x1b[2J\x1b[H{}\n{}\n\x1b[3r\x1b[3;1H", line, detail);
            *started = true;
        } else {
            // DECSC/DECRC save+restore the log cursor across the jump to row 1.
            let _ = write!(out, "\x1b7\x1b[1;1H\x1b[2K{}\n\x1b[2K{}\x1b8", line, detail);
        }
        let _ = out.flush();
    }

    /// One line in the verbose request/rebuild log, printed under the pinned
    /// header. No-op unless `--verbose`/`-v` was passed.
    fn log_line(&self, text: &str) {
        if !self.active.load(Ordering::SeqCst) || !self.verbose() {
            return;
        }
        let _g = self.term_lock.lock().unwrap();
        let mut out = std::io::stderr();
        if self.pin && self.controls_ready.load(Ordering::SeqCst) {
            let _ = write!(out, "{}\r\n", text);
        } else {
            let _ = writeln!(out, "{}", text);
        }
        let _ = out.flush();
    }

    fn log_rebuild(&self, ok: bool, detail: &str) {
        let arrow = if ok { "rebuilt" } else { "error" };
        self.log_line(&format!(
            "{}  save {}  →  {} {}",
            clock_time(),
            self.watched_file,
            arrow,
            detail
        ));
    }

    fn log_request(&self, method: &str, path: &str, code: u16, elapsed: Duration) {
        self.log_line(&format!(
            "{}  {}  {:<20} {}  {}ms",
            clock_time(),
            method,
            path,
            code,
            elapsed.as_millis()
        ));
    }

    fn log_diagnostic(&self, code: &str, diagnostic: &str) {
        if !self.active.load(Ordering::SeqCst) || !self.verbose() {
            return;
        }
        let _g = self.term_lock.lock().unwrap();
        let mut out = std::io::stderr();
        if self.pin && self.controls_ready.load(Ordering::SeqCst) {
            for line in frame_lines(code, diagnostic) {
                let _ = write!(out, "{}\r\n", line);
            }
        } else {
            let _ = writeln!(out, "{}", diagnostic);
        }
        let _ = out.flush();
    }

    fn verbose(&self) -> bool {
        self.verbose.load(Ordering::SeqCst)
    }

    /// Toggle terminal depth without changing shared parity state. Transition
    /// between redraw strategies explicitly so stale pinned blocks/scroll
    /// regions never survive a `v` keypress.
    fn toggle_verbose(&self) {
        let verbose = !self.verbose.fetch_xor(true, Ordering::SeqCst);
        if self.pin {
            let _g = self.term_lock.lock().unwrap();
            let mut out = std::io::stderr();
            if verbose {
                let mut prev = self.last_block_lines.lock().unwrap();
                if *prev > 1 {
                    let _ = write!(out, "\x1b[{}A", *prev - 1);
                }
                let _ = write!(out, "\r\x1b[0J");
                *prev = 0;
            } else {
                let _ = write!(out, "\x1b[r\x1b[2J\x1b[H");
                *self.header_started.lock().unwrap() = false;
            }
            let _ = out.flush();
        }
        self.refresh();
        self.log_line(if verbose {
            "verbose request/rebuild log enabled · press v to collapse"
        } else {
            ""
        });
    }

    fn disable_terminal_controls(&self) {
        if !self.pin {
            self.controls_ready.store(false, Ordering::SeqCst);
            return;
        }
        let _g = self.term_lock.lock().unwrap();
        // Disable and restore in one critical section. Any renderer already
        // waiting for this lock observes false before it can write.
        self.controls_ready.store(false, Ordering::SeqCst);
        let mut out = std::io::stderr();
        let mut started = self.header_started.lock().unwrap();
        if *started {
            let _ = write!(out, "\x1b[r\x1b[999;1H\r\n");
        } else {
            let _ = writeln!(out);
        }
        *started = false;
        let _ = out.flush();
    }

    fn mark_building(&self) {
        self.browser_relay.lock().unwrap().take();
        let mut state = self.state.lock().unwrap();
        let last_build_ms = state.last_build_ms;
        *state = DevStatusSnapshot {
            state: "building".to_string(),
            code: String::new(),
            diagnostic: String::new(),
            last_build_ms,
        };
        drop(state);
        self.refresh();
    }

    fn mark_ready(&self, elapsed_ms: u128, is_rebuild: bool) {
        let mut browser_relay = self.browser_relay.lock().unwrap();
        if self.browser_trace_enabled.load(Ordering::SeqCst) {
            browser_relay.take();
            *browser_relay = read_web_manifest()
                .and_then(|manifest| crate::BrowserTrace::Relay::new(&manifest).ok());
        }
        drop(browser_relay);
        if is_rebuild {
            self.version.fetch_add(1, Ordering::SeqCst);
        }
        *self.state.lock().unwrap() = DevStatusSnapshot {
            state: "ready".to_string(),
            code: String::new(),
            diagnostic: String::new(),
            last_build_ms: elapsed_ms,
        };
        self.refresh();
        if is_rebuild {
            self.log_rebuild(true, &format_build_time(elapsed_ms));
        }
    }

    fn mark_error(&self, code: String, diagnostic: String, is_rebuild: bool) {
        let diagnostic_for_log = diagnostic.clone();
        let mut state = self.state.lock().unwrap();
        let last_build_ms = state.last_build_ms;
        *state = DevStatusSnapshot {
            state: "error".to_string(),
            code: code.clone(),
            diagnostic,
            last_build_ms,
        };
        drop(state);
        self.refresh();
        if is_rebuild {
            self.log_rebuild(false, &code);
            self.log_diagnostic(&code, &diagnostic_for_log);
        }
    }

    /// Typed handoff of the live error snapshot for
    /// `/__jet_devtools/error` — copies the exact `state`/`code`/
    /// `diagnostic` fields `mark_error` recorded, matching the parity
    /// terminal/browser strip (D-FE-DEVSRV1) instead of re-deriving them.
    fn web_error_facts(&self) -> crate::WebErrorPage::WebErrorDevStatusFacts {
        let snap = self.state.lock().unwrap().clone();
        crate::WebErrorPage::WebErrorDevStatusFacts::new(snap.state, snap.code, snap.diagnostic)
    }

    fn json(&self) -> String {
        let snap = self.state.lock().unwrap().clone();
        let (word, rest) = self.header_text_for(&snap);
        let message = format!("{} · {}", word, rest);
        format!(
            "{{\"version\":{},\"state\":\"{}\",\"message\":\"{}\",\"file\":\"{}\",\"code\":\"{}\",\"diagnostic\":\"{}\",\"clients\":{},\"last_build_ms\":{}}}",
            self.version.load(Ordering::SeqCst),
            json_escape(&word),
            json_escape(&message),
            json_escape(&self.watched_file),
            json_escape(&snap.code),
            json_escape(&snap.diagnostic),
            self.client_count(),
            snap.last_build_ms
        )
    }

    fn activate(&self) {
        self.active.store(true, Ordering::SeqCst);
        self.refresh();
    }

    fn client_count(&self) -> u64 {
        self.prune_expired_clients();
        self.clients.lock().unwrap().len() as u64
    }

    fn note_client(&self, id: &str) {
        if !crate::Session::client_id_is_valid(id) {
            return;
        }
        let pruned = self.prune_expired_clients();
        let mut clients = self.clients.lock().unwrap();
        let is_new = !clients.contains_key(id);
        if is_new && clients.len() >= MAX_CLIENTS {
            return;
        }
        let changed = clients.insert(id.to_string(), Instant::now()).is_none();
        drop(clients);
        let recovered = self.reconnecting.swap(false, Ordering::SeqCst);
        if pruned || changed || recovered {
            self.refresh();
        }
    }

    fn expire_clients(&self) {
        let changed = self.prune_expired_clients();
        if changed {
            self.refresh();
        }
    }

    fn prune_expired_clients(&self) -> bool {
        let cutoff = Duration::from_millis(CLIENT_TTL_MS);
        let now = Instant::now();
        let mut clients = self.clients.lock().unwrap();
        let before = clients.len();
        clients.retain(|_, seen| now.saturating_duration_since(*seen) <= cutoff);
        let changed = clients.len() != before;
        let disconnected = before > 0 && clients.is_empty();
        drop(clients);
        if disconnected {
            self.reconnecting.store(true, Ordering::SeqCst);
        }
        changed
    }

    fn drop_client(&self, id: &str) {
        if !crate::Session::client_id_is_valid(id) {
            return;
        }
        let mut clients = self.clients.lock().unwrap();
        let changed = clients.remove(id).is_some();
        drop(clients);
        if changed {
            // A pagehide beacon is a clean close/reload, not a broken poll.
            self.reconnecting.store(false, Ordering::SeqCst);
            self.refresh();
        }
    }

    fn record_command_receipt(&self, receipt: String) {
        *self.command_receipt.lock().unwrap() = Some(receipt);
    }

    fn command_receipt(&self) -> Option<String> {
        self.command_receipt.lock().unwrap().clone()
    }

    fn activate_browser_trace(&self, manifest: &str) -> Result<(), String> {
        let relay = crate::BrowserTrace::Relay::new(manifest)?;
        *self.browser_relay.lock().unwrap() = Some(relay);
        self.browser_trace_enabled.store(true, Ordering::SeqCst);
        self.version.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn activate_requested_browser_trace(&self) {
        if !matches!(crate::BrowserTrace::take_request(), Ok(true)) {
            return;
        }
        if let Some(manifest) = read_web_manifest() {
            let _ = self.activate_browser_trace(&manifest);
        }
    }
}

#[derive(Clone, Debug)]
pub struct CanvasHostOptions {
    pub host: String,
    pub port: Option<u16>,
    pub transport: String,
    pub authority: String,
    pub audit: bool,
    pub output: Option<String>,
    pub target: Option<String>,
}

impl Default for CanvasHostOptions {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: None,
            transport: "http".to_string(),
            authority: "loopback".to_string(),
            audit: false,
            output: None,
            target: None,
        }
    }
}

pub struct WebHost {
    listener: Mutex<Option<TcpListener>>,
    application_listener: Mutex<Option<TcpListener>>,
    started: AtomicBool,
    shutdown: Arc<AtomicBool>,
    server_threads: Mutex<Vec<thread::JoinHandle<()>>>,
    poll_thread: Mutex<Option<thread::JoinHandle<()>>>,
    terminal_thread: Mutex<Option<thread::JoinHandle<()>>>,
    status: Arc<DevStatus>,
    debug_sessions: Arc<crate::Canvas::DebugSessions>,
    session: Arc<crate::ResidentDevSession>,
    canvas_file: String,
    bind_host: String,
    session_secret: String,
    release_policy: ReleaseDevtoolsPolicy,
    static_root: PathBuf,
    browser_state: Arc<Mutex<Option<browser_web_adapter::BrowserWebState>>>,
    /// Whether this host owns the web application swap stream. Canvas/static
    /// native hosts still share the session but do not consume web manifests.
    web_swap_enabled: bool,
    /// The latest sema verdict is consumed by the next successful rebuild.
    hot_swap_decision: Mutex<Option<HotSwapDecision>>,
    paused_evaluate: Arc<Mutex<crate::PausedEvaluate::PausedEvaluateHost>>,
    terminal_style: Arc<crate::TerminalStyleReload::TerminalStyleHostAdapter>,
}

impl WebHost {
    /// Bind an application preview with a folded release-devtools policy.
    pub fn bind_with_policy(
        file: &str,
        verbose: bool,
        port: Option<u16>,
        release_policy: ReleaseDevtoolsPolicy,
    ) -> Result<Self, String> {
        Self::bind_application_only_with_policy(
            file,
            verbose,
            port,
            PathBuf::from("build"),
            release_policy,
            true,
        )
    }

    /// Bind a static preview around a caller-owned resident session.
    ///
    /// The caller binds the application listener first so the session can
    /// carry the exact listener port. Every host attached to that program can
    /// then project the same event, selection, and cursor state.
    pub fn bind_static_with_policy(
        file: &str,
        root: &Path,
        verbose: bool,
        application_listener: TcpListener,
        session: Arc<crate::ResidentDevSession>,
        release_policy: ReleaseDevtoolsPolicy,
    ) -> Result<Self, String> {
        let static_root = fs::canonicalize(root).map_err(|error| {
            format!(
                "error: couldn't open static output `{}`: {}",
                root.display(),
                error
            )
        })?;
        let application_port = application_listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0);
        if session.application_port() != application_port {
            return Err(format!(
                "error: shared devtools session application port {} does not match static preview port {}",
                session.application_port(),
                application_port
            ));
        }
        Self::bind_application_only_with_listener(
            file,
            verbose,
            static_root,
            release_policy,
            false,
            application_listener,
            session,
        )
    }

    /// Bind an application listener before constructing the shared session.
    pub fn bind_application_preview_listener(port: Option<u16>) -> Result<TcpListener, String> {
        bind_application_server(port)
    }

    fn bind_application_only_with_policy(
        file: &str,
        verbose: bool,
        port: Option<u16>,
        static_root: PathBuf,
        release_policy: ReleaseDevtoolsPolicy,
        web_swap_enabled: bool,
    ) -> Result<Self, String> {
        let application_listener = bind_application_server(port)?;
        let application_port = application_listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0);
        let session = Arc::new(crate::ResidentDevSession::new(file, 0, application_port));
        Self::bind_application_only_with_listener(
            file,
            verbose,
            static_root,
            release_policy,
            web_swap_enabled,
            application_listener,
            session,
        )
    }

    fn bind_application_only_with_listener(
        file: &str,
        verbose: bool,
        static_root: PathBuf,
        release_policy: ReleaseDevtoolsPolicy,
        web_swap_enabled: bool,
        application_listener: TcpListener,
        session: Arc<crate::ResidentDevSession>,
    ) -> Result<Self, String> {
        let application_port = application_listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0);
        let status = Arc::new(DevStatus::new(file, verbose));
        status.set_port(application_port);
        status.set_canvas_port(0);
        status.set_canvas_enabled(false);
        let session_secret = mint_session_secret()?;
        Ok(Self {
            listener: Mutex::new(None),
            application_listener: Mutex::new(Some(application_listener)),
            started: AtomicBool::new(false),
            shutdown: Arc::new(AtomicBool::new(false)),
            server_threads: Mutex::new(Vec::new()),
            poll_thread: Mutex::new(None),
            terminal_thread: Mutex::new(None),
            status,
            debug_sessions: Arc::new(crate::Canvas::DebugSessions::default()),
            session,
            canvas_file: file.to_string(),
            bind_host: "127.0.0.1".to_string(),
            session_secret,
            release_policy,
            static_root,
            browser_state: Arc::new(Mutex::new(None)),
            web_swap_enabled,
            hot_swap_decision: Mutex::new(None),
            paused_evaluate: Arc::new(Mutex::new(crate::PausedEvaluate::PausedEvaluateHost::new())),
            terminal_style: terminal_style_adapter(file)?,
        })
    }

    pub fn bind_web_with_canvas_options_and_policy(
        file: &str,
        verbose: bool,
        fallback_port: Option<u16>,
        options: &CanvasHostOptions,
        release_policy: ReleaseDevtoolsPolicy,
    ) -> Result<Self, String> {
        let host = validate_canvas_options(options)?;
        let (listener, application_listener) = if options.port.is_some() {
            let listener = bind_canvas_server(&host, options.port)?;
            let application_listener = bind_application_server(fallback_port)?;
            (listener, application_listener)
        } else {
            let application_listener = bind_application_server(fallback_port)?;
            let listener = bind_canvas_server(&host, None)?;
            (listener, application_listener)
        };
        let canvas_port = listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0);
        let application_port = application_listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0);
        let status = Arc::new(DevStatus::new(file, verbose || options.audit));
        status.set_port(application_port);
        status.set_canvas_port(canvas_port);
        let session_secret = mint_session_secret()?;
        let session = Arc::new(crate::ResidentDevSession::new_with_hosts(
            file,
            &host,
            canvas_port,
            "127.0.0.1",
            application_port,
        ));
        session.select_output_values(options.output.as_deref(), options.target.as_deref());
        Ok(Self {
            listener: Mutex::new(Some(listener)),
            application_listener: Mutex::new(Some(application_listener)),
            started: AtomicBool::new(false),
            shutdown: Arc::new(AtomicBool::new(false)),
            server_threads: Mutex::new(Vec::new()),
            poll_thread: Mutex::new(None),
            terminal_thread: Mutex::new(None),
            status,
            debug_sessions: Arc::new(crate::Canvas::DebugSessions::default()),
            session,
            canvas_file: file.to_string(),
            bind_host: host,
            session_secret,
            release_policy,
            static_root: PathBuf::from("build"),
            browser_state: Arc::new(Mutex::new(None)),
            web_swap_enabled: true,
            hot_swap_decision: Mutex::new(None),
            paused_evaluate: Arc::new(Mutex::new(crate::PausedEvaluate::PausedEvaluateHost::new())),
            terminal_style: terminal_style_adapter(file)?,
        })
    }

    pub fn bind_canvas_with_options_and_policy(
        file: &str,
        verbose: bool,
        options: &CanvasHostOptions,
        release_policy: ReleaseDevtoolsPolicy,
    ) -> Result<Self, String> {
        let host = validate_canvas_options(options)?;
        let listener = bind_canvas_server(&host, options.port)?;
        let bound_port = listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(0);
        let status = Arc::new(DevStatus::new(file, verbose || options.audit));
        status.set_port(bound_port);
        status.set_canvas_port(bound_port);
        let session_secret = mint_session_secret()?;
        let session = Arc::new(crate::ResidentDevSession::new_with_canvas_host(
            file, &host, bound_port, 0,
        ));
        Ok(Self {
            listener: Mutex::new(Some(listener)),
            application_listener: Mutex::new(None),
            started: AtomicBool::new(false),
            shutdown: Arc::new(AtomicBool::new(false)),
            server_threads: Mutex::new(Vec::new()),
            poll_thread: Mutex::new(None),
            terminal_thread: Mutex::new(None),
            status,
            debug_sessions: Arc::new(crate::Canvas::DebugSessions::default()),
            session,
            canvas_file: file.to_string(),
            bind_host: host,
            session_secret,
            release_policy,
            static_root: PathBuf::from("build"),
            browser_state: Arc::new(Mutex::new(None)),
            web_swap_enabled: false,
            hot_swap_decision: Mutex::new(None),
            paused_evaluate: Arc::new(Mutex::new(crate::PausedEvaluate::PausedEvaluateHost::new())),
            terminal_style: terminal_style_adapter(file)?,
        })
    }

    pub fn start(&self) {
        self.start_inner(true);
    }

    /// Start the HTTP/Canvas plane without taking terminal input. The native
    /// `jet dev --canvas` watcher owns its stdin and Ctrl-C lifecycle; both
    /// surfaces still share this one resident session.
    pub fn start_canvas(&self) {
        self.start_inner(false);
    }

    pub fn lock_source_transaction(&self) -> std::sync::MutexGuard<'_, ()> {
        self.session.lock_source_transaction()
    }

    /// The one resident session shared by every surface of this host.  The
    /// driver registers its rebuild executor and consumes queued
    /// `ProjectRebuild` commands through it.
    pub fn resident_session(&self) -> Arc<crate::ResidentDevSession> {
        Arc::clone(&self.session)
    }

    pub fn publish_hot_swap_decision(&self, decision: &HotSwapDecision) -> Result<(), String> {
        self.session.publish_hot_swap_decision(decision)?;
        *self.hot_swap_decision.lock().unwrap() = Some(decision.clone());
        Ok(())
    }

    pub fn canvas_url(&self) -> String {
        format!(
            "http://{}:{}/canvas?session={}",
            url_host(&self.bind_host),
            self.session.canvas_port(),
            self.session_secret
        )
    }

    fn start_inner(&self, terminal_controls: bool) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let has_canvas = self.listener.lock().unwrap().is_some();
        let listener = self.listener.lock().unwrap().take();
        if let Some(listener) = listener {
            let status = Arc::clone(&self.status);
            let debug_sessions = Arc::clone(&self.debug_sessions);
            let session = Arc::clone(&self.session);
            let canvas_file = self.canvas_file.clone();
            let bind_host = self.bind_host.clone();
            let session_secret = self.session_secret.clone();
            let release_policy = self.release_policy.clone();
            let static_root = self.static_root.clone();
            let shutdown = Arc::clone(&self.shutdown);
            let browser_state = Arc::clone(&self.browser_state);
            let paused_evaluate = Arc::clone(&self.paused_evaluate);
            let terminal_style = Arc::clone(&self.terminal_style);
            let handle = thread::spawn(move || {
                serve_forever(
                    listener,
                    status,
                    debug_sessions,
                    session,
                    canvas_file,
                    ListenerKind::Canvas,
                    bind_host,
                    session_secret,
                    shutdown,
                    static_root,
                    release_policy,
                    browser_state,
                    paused_evaluate,
                    terminal_style,
                )
            });
            self.server_threads.lock().unwrap().push(handle);
        }
        if let Some(listener) = self.application_listener.lock().unwrap().take() {
            let status = Arc::clone(&self.status);
            let debug_sessions = Arc::clone(&self.debug_sessions);
            let session = Arc::clone(&self.session);
            let canvas_file = self.canvas_file.clone();
            let static_root = self.static_root.clone();
            let session_secret = self.session_secret.clone();
            let application_host = self.session.application_host().to_string();
            let release_policy = self.release_policy.clone();
            let browser_state = Arc::clone(&self.browser_state);
            let paused_evaluate = Arc::clone(&self.paused_evaluate);
            let terminal_style = Arc::clone(&self.terminal_style);
            let shutdown = Arc::clone(&self.shutdown);
            let handle = thread::spawn(move || {
                serve_forever(
                    listener,
                    status,
                    debug_sessions,
                    session,
                    canvas_file,
                    ListenerKind::Application,
                    application_host,
                    session_secret,
                    shutdown,
                    static_root,
                    release_policy,
                    browser_state,
                    paused_evaluate,
                    terminal_style,
                )
            });
            self.server_threads.lock().unwrap().push(handle);
        }
        {
            let status = Arc::clone(&self.status);
            let shutdown = Arc::clone(&self.shutdown);
            let handle = thread::spawn(move || {
                while !wait_for_shutdown(&shutdown, Duration::from_millis(LIVE_RELOAD_POLL_MS)) {
                    status.activate_requested_browser_trace();
                    status.expire_clients();
                }
            });
            *self.poll_thread.lock().unwrap() = Some(handle);
        }
        if self.session_application_port() != 0 {
            println!(
                "App preview: http://{}:{}/",
                url_host(self.session.application_host()),
                self.session_application_port(),
            );
        }
        if has_canvas {
            println!("Canvas: {}", self.canvas_url());
        }
        let _ = std::io::stdout().flush();
        if terminal_controls {
            if let Some(handle) =
                start_terminal_controls(Arc::clone(&self.status), Arc::clone(&self.shutdown))
            {
                *self.terminal_thread.lock().unwrap() = Some(handle);
            }
        }
        self.status.activate();
    }

    pub fn mark_building(&self) {
        self.status.mark_building();
        self.session.mark_building();
    }

    pub fn mark_ready(&self, elapsed_ms: u128, is_rebuild: bool) {
        let decision = self.hot_swap_decision.lock().unwrap().take();
        if self.web_swap_enabled {
            let manifest = fs::read_to_string(self.static_root.join("web.manifest.json")).ok();
            let build_id = manifest.as_deref().and_then(|manifest| {
                browser_web_adapter::manifest_artifact_identity(manifest).ok()
            });
            let mut source_revision = None;
            let mut browser_error = None;
            if let Ok(source) =
                crate::Canvas::read_source_without_symlinks(Path::new(&self.canvas_file))
            {
                let revision = crate::Canvas::source_revision(&source);
                source_revision = Some(revision.clone());
                if let Some(build_id) = build_id.as_deref() {
                    let _ = self.session.set_devtools_source_identity(
                        Some(self.canvas_file.clone()),
                        Some(build_id.to_string()),
                        Some(revision.clone()),
                        None,
                    );
                }
                self.session.observe_source(&revision);
                if let (Some(manifest), Some(build_id)) = (manifest.as_deref(), build_id.as_deref())
                {
                    match self.browser_state.lock() {
                        Ok(mut browser_state) => {
                            if let Err(error) = browser_web_adapter::apply_web_swap_from_manifest(
                                &mut browser_state,
                                self.session.id(),
                                &revision,
                                build_id,
                                manifest,
                                decision.as_ref(),
                            ) {
                                browser_error = Some(error);
                            }
                        }
                        Err(_) => {
                            browser_error = Some(
                                "browser swap state lock was poisoned; rebuild was rejected"
                                    .to_string(),
                            );
                        }
                    }
                }
            } else if decision.is_some() {
                browser_error = Some(
                    "web rebuild has no readable source revision; compatibility application was rejected"
                        .to_string(),
                );
            }
            if decision.is_some() && (manifest.is_none() || build_id.is_none()) {
                browser_error.get_or_insert_with(|| {
                    "web rebuild has no published manifest/artifact identity; canonical HotSwapDecision was not applied"
                        .to_string()
                });
            }
            if let Some(error) = browser_error {
                self.status
                    .mark_error("E2210".to_string(), error.clone(), is_rebuild);
                self.session.mark_error("E2210", &error);
                return;
            }
            if let (Some(build_id), Some(revision)) =
                (build_id.as_deref(), source_revision.as_deref())
            {
                self.session.mark_last_good(revision, build_id);
            }
        }
        self.status.mark_ready(elapsed_ms, is_rebuild);
        self.session.mark_ready();
    }

    pub fn mark_error(&self, code: String, diagnostic: String, is_rebuild: bool) {
        self.hot_swap_decision.lock().unwrap().take();
        self.status
            .mark_error(code.clone(), diagnostic.clone(), is_rebuild);
        if let Ok(source) =
            crate::Canvas::read_source_without_symlinks(Path::new(&self.canvas_file))
        {
            self.session
                .observe_source(&crate::Canvas::source_revision(&source));
        }
        self.session.mark_error(&code, &diagnostic);
    }

    pub fn exit_code(&self) -> Option<i32> {
        let code = self.status.exit_code.load(Ordering::SeqCst);
        (code != 0).then_some(code as i32)
    }

    fn session_application_port(&self) -> u16 {
        self.session.application_port()
    }
}

impl Drop for WebHost {
    fn drop(&mut self) {
        self.session.mark_stopped();
        self.shutdown.store(true, Ordering::SeqCst);
        self.listener.lock().unwrap().take();
        self.application_listener.lock().unwrap().take();

        if let Some(handle) = self.terminal_thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.poll_thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        let handles = self
            .server_threads
            .lock()
            .unwrap()
            .drain(..)
            .collect::<Vec<_>>();
        for handle in handles {
            let _ = handle.join();
        }
        if let Ok(mut paused_evaluate) = self.paused_evaluate.lock() {
            let _ = self
                .debug_sessions
                .cancel_all_paused_sessions(&mut paused_evaluate, "web host stopped");
        }
        self.status.active.store(false, Ordering::SeqCst);
    }
}

fn wait_for_shutdown(shutdown: &AtomicBool, duration: Duration) -> bool {
    let deadline = Instant::now() + duration;
    while !shutdown.load(Ordering::SeqCst) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        thread::sleep(remaining.min(Duration::from_millis(10)));
    }
    true
}

fn start_terminal_controls(
    status: Arc<DevStatus>,
    shutdown: Arc<AtomicBool>,
) -> Option<thread::JoinHandle<()>> {
    if !status.pin {
        return None;
    }
    let Some(raw) = jet_repl::Term::RawGuard::enable() else {
        return None;
    };
    status.controls_ready.store(true, Ordering::SeqCst);
    Some(thread::spawn(move || {
        let mut keys = jet_repl::Term::KeyReader::new(std::io::stdin());
        loop {
            if shutdown.load(Ordering::SeqCst) {
                status.disable_terminal_controls();
                drop(raw);
                return;
            }
            match keys.read_key() {
                jet_repl::Term::Key::Char('v' | 'V') => status.toggle_verbose(),
                jet_repl::Term::Key::Idle => {}
                jet_repl::Term::Key::CtrlC => {
                    status.disable_terminal_controls();
                    drop(raw);
                    status.exit_code.store(130, Ordering::SeqCst);
                    return;
                }
                jet_repl::Term::Key::Eof => {
                    status.disable_terminal_controls();
                    drop(raw);
                    return;
                }
                _ => {}
            }
        }
    }))
}

/// Pure — the parity words shared verbatim by the terminal line and the
/// browser strip's `message` field (`(state_word, rest)`; callers join them
/// with " · "). No I/O, no locking: easy to unit test every state directly.
fn header_words(
    state: &str,
    file: &str,
    code: &str,
    port: u16,
    clients: u64,
    last_build_ms: u128,
) -> (String, String) {
    let plural = if clients == 1 { "" } else { "s" };
    match state {
        "building" => (
            "building".to_string(),
            format!("{} · {} client{}", file, clients, plural),
        ),
        "error" => (
            "error".to_string(),
            format!("{} · {} client{}", code, clients, plural),
        ),
        _ => {
            let mut rest = format!("localhost:{} · {} client{}", port, clients, plural);
            if last_build_ms > 0 {
                rest.push_str(&format!(" · built {}", format_build_time(last_build_ms)));
            }
            ("ready".to_string(), rest)
        }
    }
}

fn format_build_time(ms: u128) -> String {
    format!("{:.1}s", ms as f64 / 1000.0)
}

fn format_line_colored(word: &str, rest: &str) -> String {
    let sgr = match word {
        "ready" => "32",
        "building" | "reconnecting" => "33",
        "error" => "31",
        _ => "37",
    };
    format!("jet dev  \x1b[{}m\u{25CF}\x1b[0m {} · {}", sgr, word, rest)
}

fn format_line_plain(word: &str, rest: &str) -> String {
    format!("jet dev  [{}] {}", word, rest)
}

/// Frame a verbatim diagnostic in a box for the pinned (TTY) terminal
/// surface — border/frame chars only, never a changed word or code (I4).
/// Off-TTY (`refresh`'s CI-floor branch) prints the same diagnostic
/// unframed instead of calling this at all.
fn frame_lines(code: &str, diagnostic: &str) -> Vec<String> {
    let lines: Vec<&str> = diagnostic.lines().collect();
    let content_width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let width = content_width.max(code.len() + 2).clamp(20, 96);
    let dash_count = width.saturating_sub(code.len() + 3).max(1);
    let mut out = Vec::with_capacity(lines.len() + 2);
    out.push(format!("┌ {} {}", code, "─".repeat(dash_count)));
    for line in lines {
        out.push(format!("│ {}", line));
    }
    out.push(format!("└{}", "─".repeat(width + 2)));
    out
}

/// Wall-clock `HH:MM:SS` (UTC — std has no local-offset lookup without a
/// crate, I6) for verbose request/rebuild log lines. Cosmetic only, never
/// part of a diagnostic's verbatim text.
fn clock_time() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let sod = secs % 86400;
    format!("{:02}:{:02}:{:02}", sod / 3600, (sod % 3600) / 60, sod % 60)
}

/// A held output directory used by both the compiler and the publication
/// transaction.  The path is retained for diagnostics only; every filesystem
/// operation below it goes through the held directory authority.
pub struct WebOutputAuthority {
    path: PathBuf,
    directory: secure_output::Directory,
}

/// A compiler-owned temporary output.  It is created exclusively below a held
/// output authority and removed by its descriptor-relative drop path.
pub struct WebOutputTempFile {
    path: PathBuf,
    inner: secure_output::TempFile,
}

impl WebOutputAuthority {
    /// Create the directory tree without following links, then pin the final
    /// output directory before returning.
    pub fn open_or_create(path: &Path) -> std::io::Result<Self> {
        let real = ensure_real_output_dir(path)?;
        let directory = secure_output::open_root(&real)?;
        Ok(Self {
            path: real,
            directory,
        })
    }

    /// Open an already-created directory and retain its descriptor/handle.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let real = ensure_real_output_dir(path)?;
        let directory = secure_output::open_root(&real)?;
        Ok(Self {
            path: real,
            directory,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn path_for(&self, name: &str) -> std::io::Result<PathBuf> {
        let name = normal_output_name(name)?;
        Ok(self.path.join(name))
    }

    pub fn replace_file(&self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        let name = normal_output_name(name)?;
        secure_output::replace_file(&self.directory, name, bytes)
    }

    pub fn open_file(&self, name: &str) -> std::io::Result<std::fs::File> {
        let name = normal_output_name(name)?;
        secure_output::open_file(&self.directory, name)
    }

    pub fn validate_file(&self, name: &str, required: bool) -> std::io::Result<bool> {
        let name = normal_output_name(name)?;
        match secure_output::open_file(&self.directory, name) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn remove_file_if_exists(&self, name: &str) -> std::io::Result<()> {
        let name = normal_output_name(name)?;
        match secure_output::remove_file(&self.directory, name) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub fn rename_file_to(&self, name: &str, destination: &Self) -> std::io::Result<()> {
        let name = normal_output_name(name)?;
        secure_output::rename_file(&self.directory, name, &destination.directory, name)
    }
    pub fn open_existing_child(&self, name: &str) -> std::io::Result<Self> {
        let name = normal_output_name(name)?;
        let directory = secure_output::open_child(&self.directory, name)?;
        Ok(Self {
            path: self.path.join(name),
            directory,
        })
    }

    pub fn create_child(&self, path: &Path) -> std::io::Result<Self> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let real_parent = fs::canonicalize(parent)?;
        if real_parent != self.path {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "web child directory escapes its held output root",
            ));
        }
        let name =
            normal_output_name(path.file_name().and_then(|name| name.to_str()).ok_or_else(
                || {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "web child directory has no normal name",
                    )
                },
            )?)?;
        let directory = secure_output::open_child(&self.directory, name)?;
        Ok(Self {
            path: self.path.join(name),
            directory,
        })
    }

    pub fn create_unique_child(&self, prefix: &str) -> std::io::Result<(String, Self)> {
        for _ in 0..100 {
            let name = format!(
                "{prefix}-{}-{}",
                std::process::id(),
                PUBLICATION_COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            match secure_output::create_child(&self.directory, std::ffi::OsStr::new(&name)) {
                Ok(directory) => {
                    return Ok((
                        name.clone(),
                        Self {
                            path: self.path.join(&name),
                            directory,
                        },
                    ))
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate a web publication journal",
        ))
    }

    pub fn remove_child_directory(&self, name: &str) -> std::io::Result<()> {
        let name = normal_output_name(name)?;
        match secure_output::remove_directory(&self.directory, name) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Allocate a private file for a tool such as rustc.  The file is never
    /// opened by pathname by Jet again; its held descriptor remains the
    /// cleanup authority.
    pub fn create_temp_file(
        &self,
        prefix: &str,
        extension: &str,
    ) -> std::io::Result<WebOutputTempFile> {
        for _ in 0..100 {
            let name = format!(
                ".{prefix}-{}-{}{}",
                std::process::id(),
                PUBLICATION_COUNTER.fetch_add(1, Ordering::Relaxed),
                extension
            );
            match secure_output::create_temp_file(&self.directory, std::ffi::OsStr::new(&name)) {
                Ok(inner) => {
                    return Ok(WebOutputTempFile {
                        path: self.path.join(&name),
                        inner,
                    })
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate a web compiler temporary",
        ))
    }
}

impl WebOutputTempFile {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        secure_output::write_temp(&mut self.inner, bytes)
    }

    pub fn read_all(&mut self) -> std::io::Result<Vec<u8>> {
        secure_output::read_temp(&mut self.inner)
    }
}

fn normal_output_name(value: &str) -> std::io::Result<&std::ffi::OsStr> {
    let path = Path::new(value);
    let mut components = path.components();
    let Some(Component::Normal(name)) = components.next() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "web output name is not a normal component",
        ));
    };
    if components.next().is_some() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "web output name is not a single component",
        ));
    }
    Ok(name)
}

#[cfg(unix)]
mod secure_output {
    use std::ffi::{c_char, CString, OsStr};
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read, Seek, SeekFrom, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::{Component, Path, PathBuf};
    const O_RDONLY: i32 = 0;
    const O_WRONLY: i32 = 1;
    const O_RDWR: i32 = 2;
    const O_CREAT: i32 = 0o100;
    const O_EXCL: i32 = 0o200;
    const O_CLOEXEC: i32 = if cfg!(any(target_os = "linux", target_os = "android")) {
        0o2000000
    } else {
        0x01000000
    };
    const O_DIRECTORY: i32 = if cfg!(any(target_os = "linux", target_os = "android")) {
        0o200000
    } else {
        0x00100000
    };
    const O_NOFOLLOW: i32 = if cfg!(any(target_os = "linux", target_os = "android")) {
        0o400000
    } else {
        0x0100
    };
    const AT_REMOVEDIR: i32 = 0x200;
    const MODE_FILE: u32 = 0o600;

    unsafe extern "C" {
        fn mkdirat(directory: i32, path: *const c_char, mode: u32) -> i32;
        fn openat(directory: i32, path: *const c_char, flags: i32, ...) -> i32;
        fn renameat(
            old_directory: i32,
            old_path: *const c_char,
            new_directory: i32,
            new_path: *const c_char,
        ) -> i32;
        fn unlinkat(directory: i32, path: *const c_char, flags: i32) -> i32;
    }

    pub struct Directory {
        path: PathBuf,
        file: File,
    }

    pub struct TempFile {
        name: CString,
        file: File,
        parent: File,
    }

    fn c_name(value: &OsStr) -> io::Result<CString> {
        CString::new(value.as_bytes()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "web output name contains NUL")
        })
    }

    fn permission(message: &str) -> io::Error {
        io::Error::new(io::ErrorKind::PermissionDenied, message)
    }
    fn unlink_at(parent: &File, name: &CString, flags: i32) -> io::Result<()> {
        if unsafe { unlinkat(parent.as_raw_fd(), name.as_ptr(), flags) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn open_directory_at(parent: &File, name: &OsStr) -> io::Result<File> {
        let name = c_name(name)?;
        let fd = unsafe {
            openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
                0,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn open_file_at(parent: &File, name: &OsStr) -> io::Result<File> {
        let name = c_name(name)?;
        let fd = unsafe {
            openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                O_RDONLY | O_NOFOLLOW | O_CLOEXEC,
                0,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn check_regular(file: &File) -> io::Result<(u64, u64)> {
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(permission(
                "web output must be a singly-linked regular file",
            ));
        }
        Ok((metadata.dev(), metadata.ino()))
    }

    pub fn open_root(path: &Path) -> io::Result<Directory> {
        let cwd = std::fs::canonicalize(".")?;
        let path = std::fs::canonicalize(path)?;
        let relative = path
            .strip_prefix(&cwd)
            .map_err(|_| permission("web output root escapes the working directory"))?;
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            .open(&cwd)?;
        let mut walked = cwd.clone();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(permission("web output root is not a normal directory"));
            };
            let next = open_directory_at(&file, name)?;
            let expected = walked.join(name);
            let path_metadata = std::fs::symlink_metadata(&expected)?;
            let actual = next.metadata()?;
            if !path_metadata.is_dir()
                || !actual.is_dir()
                || path_metadata.dev() != actual.dev()
                || path_metadata.ino() != actual.ino()
            {
                return Err(permission("web output root changed during secure open"));
            }
            file = next;
            walked = expected;
        }
        let actual = file.metadata()?;
        if !actual.is_dir() {
            return Err(permission("web output root is not a directory"));
        }
        Ok(Directory { path, file })
    }

    pub fn open_child(parent: &Directory, name: &OsStr) -> io::Result<Directory> {
        let file = open_directory_at(&parent.file, name)?;
        let path = parent.path.join(name);
        let path_metadata = std::fs::symlink_metadata(&path)?;
        let actual = file.metadata()?;
        if !path_metadata.is_dir()
            || !actual.is_dir()
            || path_metadata.dev() != actual.dev()
            || path_metadata.ino() != actual.ino()
        {
            return Err(permission("web child directory changed during secure open"));
        }
        Ok(Directory { path, file })
    }

    pub fn create_child(parent: &Directory, name: &OsStr) -> io::Result<Directory> {
        let name_c = c_name(name)?;
        if unsafe { mkdirat(parent.file.as_raw_fd(), name_c.as_ptr(), 0o700) } != 0 {
            return Err(io::Error::last_os_error());
        }
        open_child(parent, name)
    }

    pub fn open_file(parent: &Directory, name: &OsStr) -> io::Result<File> {
        let file = open_file_at(&parent.file, name)?;
        check_regular(&file)?;
        Ok(file)
    }

    fn check_existing(parent: &Directory, name: &OsStr) -> io::Result<bool> {
        match open_file(parent, name) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn unique_temp_name(prefix: &OsStr) -> CString {
        let name = format!(
            ".{}-{}-{}",
            prefix.to_string_lossy(),
            std::process::id(),
            super::PUBLICATION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        CString::new(name).expect("generated web temporary name has no NUL")
    }

    pub fn create_temp_file(parent: &Directory, name: &OsStr) -> io::Result<TempFile> {
        let name = c_name(name)?;
        let fd = unsafe {
            openat(
                parent.file.as_raw_fd(),
                name.as_ptr(),
                O_RDWR | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
                MODE_FILE,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let parent_file = match parent.file.try_clone() {
            Ok(parent_file) => parent_file,
            Err(error) => {
                drop(file);
                let _ = unlink_at(&parent.file, &name, 0);
                return Err(error);
            }
        };
        Ok(TempFile {
            name,
            file,
            parent: parent_file,
        })
    }
    pub fn write_temp(temp: &mut TempFile, bytes: &[u8]) -> io::Result<()> {
        temp.file.write_all(bytes)?;
        temp.file.sync_all()
    }

    pub fn read_temp(temp: &mut TempFile) -> io::Result<Vec<u8>> {
        temp.file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        temp.file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    pub fn replace_file(parent: &Directory, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
        let _ = check_existing(parent, name)?;
        let temporary = unique_temp_name(name);
        let fd = unsafe {
            openat(
                parent.file.as_raw_fd(),
                temporary.as_ptr(),
                O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
                MODE_FILE,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        let result = (|| {
            file.write_all(bytes)?;
            file.sync_all()?;
            let temporary_id = check_regular(&file)?;
            let name_c = c_name(name)?;
            if unsafe {
                renameat(
                    parent.file.as_raw_fd(),
                    temporary.as_ptr(),
                    parent.file.as_raw_fd(),
                    name_c.as_ptr(),
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            let published = open_file(parent, name)?;
            if check_regular(&published)? != temporary_id {
                return Err(permission("published web output identity changed"));
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = unlink_at(&parent.file, &temporary, 0);
        }
        result
    }

    pub fn rename_file(
        source: &Directory,
        source_name: &OsStr,
        destination: &Directory,
        destination_name: &OsStr,
    ) -> io::Result<()> {
        let source_file = open_file(source, source_name)?;
        let source_id = check_regular(&source_file)?;
        let _ = check_existing(destination, destination_name)?;
        let source_name_c = c_name(source_name)?;
        let destination_name_c = c_name(destination_name)?;
        if unsafe {
            renameat(
                source.file.as_raw_fd(),
                source_name_c.as_ptr(),
                destination.file.as_raw_fd(),
                destination_name_c.as_ptr(),
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        let published = open_file(destination, destination_name)?;
        if check_regular(&published)? != source_id {
            return Err(permission("published web output identity changed"));
        }
        Ok(())
    }

    pub fn remove_file(parent: &Directory, name: &OsStr) -> io::Result<()> {
        let _ = open_file(parent, name)?;
        let name = c_name(name)?;
        unlink_at(&parent.file, &name, 0)
    }

    pub fn remove_directory(parent: &Directory, name: &OsStr) -> io::Result<()> {
        let name = c_name(name)?;
        unlink_at(&parent.file, &name, AT_REMOVEDIR)
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = unlink_at(&self.parent, &self.name, 0);
        }
    }
}

#[cfg(windows)]
mod secure_output {
    use std::ffi::{c_void, OsStr};
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read, Seek, SeekFrom, Write};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::{Path, PathBuf};

    type Handle = *mut c_void;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    const FILE_SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    const FILE_ATTRIBUTE_TAG_INFO_CLASS: i32 = 9;
    const FILE_RENAME_INFO_CLASS: i32 = 3;
    const FILE_DISPOSITION_INFO_CLASS: i32 = 4;

    #[repr(C)]
    struct FileAttributeTagInfo {
        attributes: u32,
        reparse_tag: u32,
    }

    #[repr(C)]
    struct FileRenameInfo {
        replace_if_exists: i32,
        root_directory: Handle,
        file_name_length: u32,
        file_name: [u16; 1],
    }

    #[repr(C)]
    struct FileDispositionInfo {
        delete_file: u8,
    }

    unsafe extern "system" {
        fn GetFileInformationByHandleEx(
            file: Handle,
            class: i32,
            info: *mut c_void,
            size: u32,
        ) -> i32;
        fn GetFinalPathNameByHandleW(
            file: Handle,
            path: *mut u16,
            path_len: u32,
            flags: u32,
        ) -> u32;
        fn SetFileInformationByHandle(
            file: Handle,
            class: i32,
            info: *mut c_void,
            size: u32,
        ) -> i32;

    }

    pub struct Directory {
        path: PathBuf,
        file: File,
        final_path: String,
    }

    pub struct TempFile {
        path: PathBuf,
        file: File,
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn normalize(value: String) -> String {
        value
            .replace('/', "\\")
            .trim_end_matches(['\\', '/'])
            .to_ascii_lowercase()
    }

    fn final_path(file: &File) -> io::Result<String> {
        let needed =
            unsafe { GetFinalPathNameByHandleW(file.as_raw_handle(), std::ptr::null_mut(), 0, 0) };
        if needed == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0u16; needed as usize + 1];
        let written = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                0,
            )
        };
        if written == 0 || written as usize >= buffer.len() {
            return Err(io::Error::last_os_error());
        }
        buffer.truncate(written as usize);
        Ok(normalize(String::from_utf16_lossy(&buffer)))
    }

    fn check_reparse(file: &File) -> io::Result<()> {
        let mut info = FileAttributeTagInfo {
            attributes: 0,
            reparse_tag: 0,
        };
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FILE_ATTRIBUTE_TAG_INFO_CLASS,
                (&mut info as *mut FileAttributeTagInfo).cast(),
                std::mem::size_of::<FileAttributeTagInfo>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "web output contains a Windows reparse point",
            ));
        }
        Ok(())
    }

    fn open_directory(path: &Path) -> io::Result<File> {
        let file = OpenOptions::new()
            .read(true)
            .access_mode(GENERIC_READ | GENERIC_WRITE)
            .share_mode(FILE_SHARE_ALL)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        check_reparse(&file)?;
        if !file.metadata()?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "web output root is not a directory",
            ));
        }
        Ok(file)
    }

    fn open_file_path(path: &Path, write: bool, create_new: bool) -> io::Result<File> {
        let mut options = OpenOptions::new();
        options.read(true).write(write);
        if create_new {
            options.create_new(true);
        }
        options
            .access_mode(GENERIC_READ | if write { GENERIC_WRITE } else { 0 })
            .share_mode(FILE_SHARE_ALL)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
    }

    fn check_regular(file: &File) -> io::Result<()> {
        check_reparse(file)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "web output is not a regular file",
            ));
        }
        Ok(())
    }

    pub fn open_root(path: &Path) -> io::Result<Directory> {
        let file = open_directory(path)?;
        let expected = normalize(
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()?.join(path)
            }
            .to_string_lossy()
            .into_owned(),
        );
        let actual = final_path(&file)?;
        if actual != expected {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "web output root changed during secure open",
            ));
        }
        Ok(Directory {
            path: path.to_path_buf(),
            file,
            final_path: actual,
        })
    }

    pub fn open_child(parent: &Directory, name: &OsStr) -> io::Result<Directory> {
        let path = parent.path.join(name);
        let file = open_directory(&path)?;
        let actual = final_path(&file)?;
        let expected = normalize(path.to_string_lossy().into_owned());
        if actual != expected || !actual.starts_with(&format!("{}\\", parent.final_path)) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "web child directory escaped its held output root",
            ));
        }
        Ok(Directory {
            path,
            file,
            final_path: actual,
        })
    }

    pub fn create_child(parent: &Directory, name: &OsStr) -> io::Result<Directory> {
        let path = parent.path.join(name);
        std::fs::create_dir(&path)?;
        open_child(parent, name)
    }

    pub fn open_file(parent: &Directory, name: &OsStr) -> io::Result<File> {
        let path = parent.path.join(name);
        let file = open_file_path(&path, false, false)?;
        check_regular(&file)?;
        if normalize(final_path(&file)?) != normalize(path.to_string_lossy().into_owned()) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "web output file escaped its held directory",
            ));
        }
        Ok(file)
    }

    fn replace_handle(source: &File, destination: &File, name: &OsStr) -> io::Result<()> {
        let file_name = name.encode_wide().collect::<Vec<_>>();
        let offset = std::mem::offset_of!(FileRenameInfo, file_name);
        let size = offset + file_name.len() * std::mem::size_of::<u16>();
        let mut storage = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
        let info = storage.as_mut_ptr().cast::<FileRenameInfo>();
        unsafe {
            (*info).replace_if_exists = 1;
            (*info).root_directory = destination.as_raw_handle();
            (*info).file_name_length = (file_name.len() * 2) as u32;
            std::ptr::copy_nonoverlapping(
                file_name.as_ptr(),
                (*info).file_name.as_mut_ptr(),
                file_name.len(),
            );
            if SetFileInformationByHandle(
                source.as_raw_handle(),
                FILE_RENAME_INFO_CLASS,
                info.cast(),
                size as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    fn delete_handle(file: &File) -> io::Result<()> {
        let mut info = FileDispositionInfo { delete_file: 1 };
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FILE_DISPOSITION_INFO_CLASS,
                (&mut info as *mut FileDispositionInfo).cast(),
                std::mem::size_of::<FileDispositionInfo>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn replace_file(parent: &Directory, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
        if let Ok(existing) = open_file(parent, name) {
            check_regular(&existing)?;
        }
        let path = parent.path.join(format!(
            ".{}-{}-{}",
            name.to_string_lossy(),
            std::process::id(),
            super::PUBLICATION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut temporary = open_file_path(&path, true, true)?;
        temporary.write_all(bytes)?;
        temporary.sync_all()?;
        check_regular(&temporary)?;
        replace_handle(&temporary, &parent.file, name)?;
        let published = open_file(parent, name)?;
        check_regular(&published)?;
        Ok(())
    }

    pub fn rename_file(
        source: &Directory,
        source_name: &OsStr,
        destination: &Directory,
        destination_name: &OsStr,
    ) -> io::Result<()> {
        let source_path = source.path.join(source_name);
        let source_file = open_file_path(&source_path, true, false)?;
        check_regular(&source_file)?;
        if let Ok(existing) = open_file(destination, destination_name) {
            check_regular(&existing)?;
        }
        replace_handle(&source_file, &destination.file, destination_name)
    }

    pub fn remove_file(parent: &Directory, name: &OsStr) -> io::Result<()> {
        let path = parent.path.join(name);
        let file = open_file_path(&path, true, false)?;
        check_regular(&file)?;
        delete_handle(&file)
    }

    pub fn remove_directory(parent: &Directory, name: &OsStr) -> io::Result<()> {
        let path = parent.path.join(name);
        let file = open_directory(&path)?;
        delete_handle(&file)
    }

    pub fn create_temp_file(parent: &Directory, name: &OsStr) -> io::Result<TempFile> {
        let path = parent.path.join(name);
        let file = open_file_path(&path, true, true)?;
        check_regular(&file)?;
        Ok(TempFile { path, file })
    }

    pub fn write_temp(temp: &mut TempFile, bytes: &[u8]) -> io::Result<()> {
        temp.file.write_all(bytes)?;
        temp.file.sync_all()
    }

    pub fn read_temp(temp: &mut TempFile) -> io::Result<Vec<u8>> {
        temp.file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        temp.file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = delete_handle(&self.file);
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod secure_output {
    use std::ffi::OsStr;
    use std::fs::File;
    use std::io;
    use std::path::Path;

    pub struct Directory;
    pub struct TempFile;

    fn unsupported<T>() -> io::Result<T> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "secure web output authority is unavailable on this platform",
        ))
    }

    pub fn open_root(_: &Path) -> io::Result<Directory> {
        unsupported()
    }
    pub fn open_child(_: &Directory, _: &OsStr) -> io::Result<Directory> {
        unsupported()
    }
    pub fn create_child(_: &Directory, _: &OsStr) -> io::Result<Directory> {
        unsupported()
    }
    pub fn open_file(_: &Directory, _: &OsStr) -> io::Result<File> {
        unsupported()
    }
    pub fn replace_file(_: &Directory, _: &OsStr, _: &[u8]) -> io::Result<()> {
        unsupported()
    }
    pub fn rename_file(_: &Directory, _: &OsStr, _: &Directory, _: &OsStr) -> io::Result<()> {
        unsupported()
    }
    pub fn remove_file(_: &Directory, _: &OsStr) -> io::Result<()> {
        unsupported()
    }
    pub fn remove_directory(_: &Directory, _: &OsStr) -> io::Result<()> {
        unsupported()
    }
    pub fn create_temp_file(_: &Directory, _: &OsStr) -> io::Result<TempFile> {
        unsupported()
    }
    pub fn write_temp(_: &mut TempFile, _: &[u8]) -> io::Result<()> {
        unsupported()
    }
    pub fn read_temp(_: &mut TempFile) -> io::Result<Vec<u8>> {
        unsupported()
    }
}

/// Publish one completed web bundle under the same lock used by static
/// readers. Preflight every member, journal the old bundle, then roll back the
/// journal if any replacement fails.
pub fn stage_and_swap(staging: &Path, out_dir: &Path) -> std::io::Result<()> {
    let _publication = crate::lock_static_publication()?;
    stage_and_swap_locked(staging, out_dir)
}

fn stage_and_swap_locked(staging: &Path, out_dir: &Path) -> std::io::Result<()> {
    const FILES: [&str; 6] = [
        "web.manifest.json",
        "jet_dom_runtime.js",
        "app.js",
        "app_wasm.rs",
        "app.wasm",
        "index.html",
    ];
    const MAP_FILES: [&str; 2] = ["app.js.map", "app.wasm.map"];

    let output_root = WebOutputAuthority::open(out_dir)?;
    let staging_name = staging
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "web staging directory has no normal name",
            )
        })?;
    let staging_parent = staging
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if fs::canonicalize(staging_parent)? != output_root.path() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "web staging directory escapes the output directory",
        ));
    }
    let staging_root = output_root.open_existing_child(staging_name)?;

    let mut names = Vec::with_capacity(FILES.len() + MAP_FILES.len());
    let mut staged_names = Vec::with_capacity(FILES.len() + MAP_FILES.len());
    for name in FILES {
        staging_root.validate_file(name, true)?;
        output_root.validate_file(name, false)?;
        names.push(name);
        staged_names.push(name);
    }
    for name in MAP_FILES {
        let staged = staging_root.validate_file(name, false)?;
        let current = output_root.validate_file(name, false)?;
        if staged || current {
            names.push(name);
        }
        if staged {
            staged_names.push(name);
        }
    }

    let (backup_name, backup) = output_root.create_unique_child(".jet-web-publication")?;
    let mut backed_up = Vec::new();
    let mut published = Vec::new();
    let result = (|| {
        for name in &names {
            if output_root.validate_file(name, false)? {
                backed_up.push(*name);
                output_root.rename_file_to(name, &backup)?;
            }
        }
        for name in &staged_names {
            published.push(*name);
            staging_root.rename_file_to(name, &output_root)?;
        }
        Ok::<(), std::io::Error>(())
    })();

    if let Err(error) = result {
        for name in published.iter().rev() {
            let _ = output_root.remove_file_if_exists(name);
        }
        for name in backed_up.iter().rev() {
            let _ = backup.rename_file_to(name, &output_root);
        }
        for name in &names {
            let _ = backup.remove_file_if_exists(name);
        }
        let _ = output_root.remove_child_directory(&backup_name);
        return Err(error);
    }

    for name in &names {
        let _ = backup.remove_file_if_exists(name);
    }
    let _ = output_root.remove_child_directory(&backup_name);
    Ok(())
}

fn ensure_real_output_dir(path: &Path) -> std::io::Result<PathBuf> {
    let cwd = fs::canonicalize(".")?;
    ensure_directory_without_symlinks(path)?;
    let real = fs::canonicalize(path)?;
    if !real.starts_with(&cwd) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "web output directory escapes the working directory",
        ));
    }
    Ok(real)
}

fn ensure_directory_without_symlinks(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "web output directory must not be a symlink",
        )),
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "web output path is not a directory",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                ensure_directory_without_symlinks(parent)?;
            }
            fs::create_dir(path)
        }
        Err(error) => Err(error),
    }
}

/// Bind the application preview listener. `Some(port)` (from `--port=<N>`)
/// binds that exact port and fails loud if it's taken — an explicit choice
/// isn't a hint. `None` scans the stable application range for a free port.
fn bind_application_server(port: Option<u16>) -> Result<TcpListener, String> {
    if let Some(port) = port {
        return match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => Ok(listener),
            Err(e) => {
                let fix = if e.kind() == std::io::ErrorKind::AddrInUse {
                    format!(
                        "\n fix: stop whatever's using port {}, or pick another with --port=<N>",
                        port
                    )
                } else {
                    String::new()
                };
                Err(format!(
                    "error: couldn't bind application preview to port {}: {}{}",
                    port, e, fix
                ))
            }
        };
    }
    let mut last_err: Option<std::io::Error> = None;
    for port in APPLICATION_PORT_RANGE {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => return Ok(listener),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                last_err = Some(e);
                continue;
            }
            Err(e) => {
                return Err(format!(
                    "error: couldn't start the application preview: {}",
                    e
                ));
            }
        }
    }
    Err(format!(
        "error: every application preview port from {} to {} is already in use{}\n fix: free one of those ports, stop the other process using it, or pick one explicitly with --port=<N>",
        APPLICATION_PORT_RANGE.start(),
        APPLICATION_PORT_RANGE.end(),
        last_err.map(|e| format!(" ({})", e)).unwrap_or_default()
    ))
}

fn bind_canvas_server(host: &str, port: Option<u16>) -> Result<TcpListener, String> {
    let requested = port.unwrap_or(0);
    match TcpListener::bind((host, requested)) {
        Ok(listener) => Ok(listener),
        Err(error) => {
            let fix = if error.kind() == std::io::ErrorKind::AddrInUse {
                format!(
                    "\n fix: stop whatever's using port {}, or pick another with --canvas-port=<N>",
                    requested
                )
            } else {
                String::new()
            };
            Err(format!(
                "error: couldn't bind Canvas host {}:{}: {}{}",
                host, requested, error, fix
            ))
        }
    }
}

fn validate_canvas_options(options: &CanvasHostOptions) -> Result<String, String> {
    if options.transport != "http" {
        return Err(format!(
            "error: Canvas transport `{}` is unsupported; use `http`",
            options.transport
        ));
    }
    if !matches!(options.authority.as_str(), "loopback" | "remote") {
        return Err(format!(
            "error: Canvas authority `{}` is invalid; use `loopback` or `remote`",
            options.authority
        ));
    }
    let host = normalize_bind_host(&options.host)?;
    if !is_loopback_host(&host) && options.authority != "remote" {
        return Err(format!(
            "error: Canvas host `{host}` is not loopback; set `authority = remote` explicitly"
        ));
    }
    Ok(host)
}

fn mint_session_secret() -> Result<String, String> {
    let mut bytes = [0u8; CANVAS_SESSION_BYTES];
    let mut source = fs::File::open("/dev/urandom")
        .map_err(|error| format!("could not open the system random source: {error}"))?;
    std::io::Read::read_exact(&mut source, &mut bytes)
        .map_err(|error| format!("could not read the system random source: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn normalize_bind_host(host: &str) -> Result<String, String> {
    let host = host.trim();
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    if host.is_empty() || host.chars().any(char::is_whitespace) || host.contains('/') {
        return Err(format!("error: Canvas host `{host}` is invalid"));
    }
    Ok(host.to_string())
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

fn url_host(host: &str) -> String {
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V6(_)) => format!("[{host}]"),
        _ => host.to_string(),
    }
}

fn canvas_api_path(path: &str, target: &str) -> bool {
    matches!(
        path,
        "/__jet_canvas"
            | "/__jet_canvas/"
            | "/__jet_canvas/app.js"
            | "/__jet_canvas/session"
            | "/__jet_canvas/live"
            | "/__jet_canvas/graph"
            | "/__jet_canvas/project"
            | "/__jet_canvas/source-control"
            | "/__jet_canvas/core-catalog"
            | "/__jet_canvas/proof"
            | "/__jet_canvas/source"
            | "/__jet_canvas/command"
            | "/__jet_canvas/transaction"
            | "/__jet_canvas/project/transaction"
            | "/__jet_canvas/query"
            | "/__jet_canvas/debug"
            | "/canvas"
            | "/canvas/"
            | "/canvas/app.js"
            | "/canvas/session"
            | "/canvas/graph"
            | "/canvas/project"
            | "/canvas/source-control"
            | "/canvas/core-catalog"
            | "/canvas/proof"
            | "/canvas/source"
            | "/canvas/command"
            | "/canvas/transaction"
            | "/canvas/project/transaction"
            | "/canvas/query"
            | "/canvas/debug"
            | "/panel"
            | "/panel/"
            | "/panel/app.js"
            | "/panel/session"
            | "/panel/graph"
            | "/panel/project"
            | "/panel/source-control"
            | "/panel/core-catalog"
            | "/panel/proof"
            | "/panel/source"
            | "/panel/command"
            | "/panel/transaction"
            | "/panel/project/transaction"
            | "/panel/query"
            | "/panel/debug"
            | "/__jet_dev_version"
            | "/__jet_dev_status"
            | "/__jet_dev_disconnect"
            | "/__jet_perf_browser"
            | "/__jet_web/stream"
    ) || devtools_path(path)
        || (path == "/"
            && (query_param(target, "jet_panel").as_deref() == Some("1")
                || query_param(target, "jet_panel_app").as_deref() == Some("1")
                || query_param(target, "jet_panel_graph").as_deref() == Some("1")))
}

fn canvas_request_authorized(
    request: &Request,
    target: &str,
    bind_host: &str,
    port: u16,
    session_secret: &str,
) -> bool {
    if session_secret.is_empty()
        || !matches!(request.method.as_str(), "GET" | "POST")
        || request.body.len() > MAX_REQUEST_BODY_BYTES
        || request.headers.contains_key("transfer-encoding")
        || (request.method == "POST" && !request.headers.contains_key("content-length"))
        || (request.method == "GET" && !request.body.is_empty())
    {
        return false;
    }
    let path = target.split('?').next().unwrap_or("");
    if !canvas_api_path(path, target) {
        return false;
    }
    let Some(host) = request.headers.get("host") else {
        return false;
    };
    if !host_header_allowed(host, bind_host, port) {
        return false;
    }
    let origin = request.headers.get("origin");
    let same_origin = request
        .headers
        .get("sec-fetch-site")
        .is_some_and(|site| site.eq_ignore_ascii_case("same-origin"));
    if let Some(origin) = origin {
        if !origin_allowed(origin, bind_host, port) {
            return false;
        }
    } else if !canvas_bootstrap_path(path, target) && !same_origin {
        return false;
    }
    let authorization = request.headers.get("authorization");
    let query_session = match unique_session_param(target) {
        Ok(session) => session,
        Err(()) => return false,
    };
    let session_valid = match (authorization, query_session.as_deref()) {
        (Some(authorization), Some(query_session)) => {
            authorization
                .strip_prefix("Bearer ")
                .is_some_and(|token| constant_time_equal(token, session_secret))
                && constant_time_equal(query_session, session_secret)
        }
        (Some(authorization), None) => authorization
            .strip_prefix("Bearer ")
            .is_some_and(|token| constant_time_equal(token, session_secret)),
        (None, Some(query_session)) => constant_time_equal(query_session, session_secret),
        (None, None) => false,
    };
    session_valid && (origin.is_some() || canvas_bootstrap_path(path, target) || same_origin)
}

fn unique_session_param(target: &str) -> Result<Option<String>, ()> {
    let Some((_, query)) = target.split_once('?') else {
        return Ok(None);
    };
    let mut found = false;
    for part in query.split('&') {
        let name = part.split_once('=').map(|(name, _)| name).unwrap_or(part);
        if name == "session" {
            if found {
                return Err(());
            }
            found = true;
        }
    }
    Ok(query_param(target, "session"))
}

fn canvas_bootstrap_path(path: &str, target: &str) -> bool {
    matches!(
        path,
        "/__jet_canvas"
            | "/__jet_canvas/"
            | "/__jet_canvas/app.js"
            | "/canvas"
            | "/canvas/"
            | "/canvas/app.js"
            | "/panel"
            | "/panel/"
            | "/panel/app.js"
            | "/__jet_devtools"
            | "/__jet_devtools/workbench"
    ) || matches!(target, "/?jet_panel=1" | "/?jet_panel_app=1")
}

fn requires_canvas_authorization(listener_kind: ListenerKind) -> bool {
    listener_kind == ListenerKind::Canvas
}

fn requires_session_authorization(listener_kind: ListenerKind, path: &str) -> bool {
    (path != "/__jet_dev_status" && requires_canvas_authorization(listener_kind))
        || (listener_kind == ListenerKind::Application
            && (devtools_path(path) || web_stream_path(path) || source_tool_path(path)))
}

fn host_header_allowed(value: &str, bind_host: &str, port: u16) -> bool {
    let value = value.trim().to_ascii_lowercase();
    if value == format!("{}:{}", url_host(bind_host), port).to_ascii_lowercase() {
        return true;
    }
    if !is_loopback_host(bind_host) {
        return false;
    }
    ["localhost", "127.0.0.1", "[::1]"]
        .iter()
        .any(|alias| value == format!("{alias}:{port}"))
}

fn origin_allowed(value: &str, bind_host: &str, port: u16) -> bool {
    let value = value.trim().to_ascii_lowercase();
    let expected = format!("http://{}:{}", url_host(bind_host), port).to_ascii_lowercase();
    if value == expected {
        return true;
    }
    is_loopback_host(bind_host)
        && ["localhost", "127.0.0.1", "[::1]"]
            .iter()
            .any(|alias| value == format!("http://{alias}:{port}"))
}

fn constant_time_equal(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= left.get(index).copied().unwrap_or(0) as usize
            ^ right.get(index).copied().unwrap_or(0) as usize;
    }
    difference == 0
}

#[allow(dead_code)]
mod web_stream_bridge {
    include!("../../jet-codegen/src/Prelude/Core/WebPending.rs");
}

fn web_stream_json_string(value: &str) -> String {
    format!("\"{}\"", json_escape(value))
}

fn web_stream_error(error: web_stream_bridge::JetWebPendingError) -> (String, String) {
    (error.code().to_string(), error.message())
}

fn web_stream_request_error(message: impl Into<String>) -> (String, String) {
    ("invalid_request".to_string(), message.into())
}

fn web_stream_required_string(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<String, (String, String)> {
    match object.get(key) {
        Some(DataTree::Text(value)) if !value.is_empty() => Ok(value.clone()),
        Some(DataTree::Text(_)) => Err(web_stream_request_error(format!(
            "web stream `{key}` must not be empty"
        ))),
        Some(_) => Err(web_stream_request_error(format!(
            "web stream `{key}` must be a string"
        ))),
        None => Err(web_stream_request_error(format!(
            "web stream request has no `{key}`"
        ))),
    }
}
fn web_stream_required_text(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<String, (String, String)> {
    match object.get(key) {
        Some(DataTree::Text(value)) => Ok(value.clone()),
        Some(_) => Err(web_stream_request_error(format!(
            "web stream `{key}` must be a string"
        ))),
        None => Err(web_stream_request_error(format!(
            "web stream request has no `{key}`"
        ))),
    }
}

fn web_stream_required_u64(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<u64, (String, String)> {
    match object.get(key) {
        Some(DataTree::Int(value)) if *value >= 0 => Ok(*value as u64),
        Some(DataTree::Int(_)) => Err(web_stream_request_error(format!(
            "web stream `{key}` must be non-negative"
        ))),
        Some(_) => Err(web_stream_request_error(format!(
            "web stream `{key}` must be an integer"
        ))),
        None => Err(web_stream_request_error(format!(
            "web stream request has no `{key}`"
        ))),
    }
}

fn web_stream_required_bool(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<bool, (String, String)> {
    match object.get(key) {
        Some(DataTree::Bool(value)) => Ok(*value),
        Some(_) => Err(web_stream_request_error(format!(
            "web stream `{key}` must be a boolean"
        ))),
        None => Err(web_stream_request_error(format!(
            "web stream request has no `{key}`"
        ))),
    }
}
fn web_stream_required_usize(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<usize, (String, String)> {
    let value = web_stream_required_u64(object, key)?;
    usize::try_from(value)
        .map_err(|_| web_stream_request_error(format!("web stream `{key}` is too large")))
}

fn web_stream_object(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<std::collections::BTreeMap<String, DataTree>, (String, String)> {
    let value = object
        .get(key)
        .ok_or_else(|| web_stream_request_error(format!("web stream request has no `{key}`")))?;
    object_map(value)
        .ok_or_else(|| web_stream_request_error(format!("web stream `{key}` must be an object")))
}

fn web_stream_identity(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<web_stream_bridge::JetWebIslandIdentity, (String, String)> {
    let identity = web_stream_object(object, key)?;
    let island_id = web_stream_required_string(&identity, "island_id")?;
    let source_id = web_stream_required_string(&identity, "source_id")?;
    let build_id = web_stream_required_string(&identity, "build_id")?;
    let revision = web_stream_required_string(&identity, "revision")?;
    web_stream_bridge::JetWebIslandIdentity::new(island_id, source_id, build_id, revision)
        .map_err(web_stream_error)
}

fn web_stream_boundary_id(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<web_stream_bridge::JetWebBoundaryId, (String, String)> {
    web_stream_bridge::JetWebBoundaryId::new(web_stream_required_string(object, key)?)
        .map_err(web_stream_error)
}

fn web_stream_id(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<web_stream_bridge::JetWebStreamId, (String, String)> {
    let stream = web_stream_object(object, key)?;
    let boundary_id = web_stream_bridge::JetWebBoundaryId::new(web_stream_required_string(
        &stream,
        "boundary_id",
    )?)
    .map_err(web_stream_error)?;
    let generation = web_stream_required_u64(&stream, "generation")?;
    let identity = web_stream_identity(&stream, "identity")?;
    web_stream_bridge::JetWebStreamId::from_parts(boundary_id, generation, identity)
        .map_err(web_stream_error)
}

fn web_stream_trigger(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<web_stream_bridge::JetWebHydrationTrigger, (String, String)> {
    let value = web_stream_required_string(object, key)?;
    web_stream_bridge::JetWebHydrationTrigger::parse(&value).ok_or_else(|| {
        web_stream_request_error(format!(
            "web stream `{key}` has an unknown hydration trigger"
        ))
    })
}

fn web_stream_target(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<web_stream_bridge::JetWebPendingProjectionTarget, (String, String)> {
    match web_stream_required_string(object, key)?.as_str() {
        "server" => Ok(web_stream_bridge::JetWebPendingProjectionTarget::Server),
        "client" => Ok(web_stream_bridge::JetWebPendingProjectionTarget::Client),
        _ => Err(web_stream_request_error(format!(
            "web stream `{key}` must be `server` or `client`"
        ))),
    }
}

fn web_stream_failure(
    object: &std::collections::BTreeMap<String, DataTree>,
) -> Result<web_stream_bridge::JetWebPendingError, (String, String)> {
    let error = web_stream_object(object, "error")?;
    match web_stream_required_string(&error, "code")?.as_str() {
        "missing_final_chunk" => Ok(web_stream_bridge::JetWebPendingError::MissingFinalChunk),
        "invalid_lifecycle" => Ok(web_stream_bridge::JetWebPendingError::InvalidLifecycle {
            state: web_stream_bridge::JetWebPendingState::Pending,
            operation: "web_stream_fail",
        }),
        "content_too_large" => Ok(web_stream_bridge::JetWebPendingError::ContentTooLarge {
            limit: web_stream_required_usize(&error, "limit")?,
            actual: web_stream_required_usize(&error, "actual")?,
        }),
        code => Err(web_stream_request_error(format!(
            "web stream failure code `{code}` is not supported by the host bridge"
        ))),
    }
}

fn web_stream_success(action: &str, payload: &str) -> String {
    format!(
        "{{\"ok\":true,\"action\":{},{} }}",
        web_stream_json_string(action),
        payload
    )
}

fn web_stream_failure_json(code: &str, message: &str) -> String {
    format!(
        "{{\"ok\":false,\"error\":{{\"code\":{},\"message\":{}}}}}",
        web_stream_json_string(code),
        web_stream_json_string(message)
    )
}

fn handle_web_stream_request(stream: &mut impl Write, request: &Request) -> std::io::Result<()> {
    if request.method != "POST" {
        return method_not_allowed(stream);
    }
    let text = match std::str::from_utf8(&request.body) {
        Ok(text) => text,
        Err(_) => {
            return write_response(
                stream,
                "400 Bad Request",
                "application/json; charset=utf-8",
                web_stream_failure_json("invalid_request", "web stream request must be UTF-8 JSON")
                    .as_bytes(),
            )
        }
    };
    let root = match parse_json_with_limit(text, MAX_REQUEST_BODY_BYTES) {
        Ok(root) => root,
        Err(_) => {
            return write_response(
                stream,
                "400 Bad Request",
                "application/json; charset=utf-8",
                web_stream_failure_json(
                    "invalid_request",
                    "web stream request is not bounded JSON",
                )
                .as_bytes(),
            )
        }
    };
    let object = match object_map(&root) {
        Some(object) => object,
        None => {
            return write_response(
                stream,
                "400 Bad Request",
                "application/json; charset=utf-8",
                web_stream_failure_json("invalid_request", "web stream request must be an object")
                    .as_bytes(),
            )
        }
    };
    let object = &object;
    let result: Result<String, (String, String)> = (|| {
        let action = web_stream_required_string(object, "action")?;
        match action.as_str() {
            "register" => {
                let boundary_id = web_stream_required_string(object, "boundary_id")?;
                let identity = web_stream_identity(object, "identity")?;
                let boundary = web_stream_bridge::jet_web_stream_register(boundary_id, identity)
                    .map_err(web_stream_error)?;
                Ok(web_stream_success(
                    &action,
                    &format!("\"boundary_id\":{}", boundary.render_json()),
                ))
            }
            "begin" => {
                let boundary_id = web_stream_boundary_id(object, "boundary_id")?;
                let identity = web_stream_identity(object, "identity")?;
                let at_ns = web_stream_required_u64(object, "at_ns")?;
                let stream_id =
                    web_stream_bridge::jet_web_stream_begin(&boundary_id, identity, at_ns)
                        .map_err(web_stream_error)?;
                Ok(web_stream_success(
                    &action,
                    &format!("\"stream_id\":{}", stream_id.render_json()),
                ))
            }
            "chunk" => {
                let stream_id = web_stream_id(object, "stream_id")?;
                let sequence = web_stream_required_u64(object, "sequence")?;
                let content = web_stream_required_text(object, "content")?;
                let is_final = web_stream_required_bool(object, "is_final")?;
                let at_ns = web_stream_required_u64(object, "at_ns")?;
                let receipt = web_stream_bridge::jet_web_stream_chunk(
                    &stream_id, sequence, content, is_final, at_ns,
                )
                .map_err(web_stream_error)?;
                Ok(web_stream_success(
                    &action,
                    &format!("\"receipt\":{}", receipt.render_json()),
                ))
            }
            "commit" => {
                let stream_id = web_stream_id(object, "stream_id")?;
                let at_ns = web_stream_required_u64(object, "at_ns")?;
                let receipt = web_stream_bridge::jet_web_stream_commit(&stream_id, at_ns)
                    .map_err(web_stream_error)?;
                Ok(web_stream_success(
                    &action,
                    &format!("\"receipt\":{}", receipt.render_json()),
                ))
            }
            "fail" => {
                let stream_id = web_stream_id(object, "stream_id")?;
                let error = web_stream_failure(object)?;
                let at_ns = web_stream_required_u64(object, "at_ns")?;
                let receipt = web_stream_bridge::jet_web_stream_fail(&stream_id, error, at_ns)
                    .map_err(web_stream_error)?;
                Ok(web_stream_success(
                    &action,
                    &format!("\"receipt\":{}", receipt.render_json()),
                ))
            }
            "cancel" => {
                let stream_id = web_stream_id(object, "stream_id")?;
                let at_ns = web_stream_required_u64(object, "at_ns")?;
                let receipt = web_stream_bridge::jet_web_stream_cancel(&stream_id, at_ns)
                    .map_err(web_stream_error)?;
                Ok(web_stream_success(
                    &action,
                    &format!("\"receipt\":{}", receipt.render_json()),
                ))
            }
            "rollback" => {
                let stream_id = web_stream_id(object, "stream_id")?;
                let at_ns = web_stream_required_u64(object, "at_ns")?;
                let receipt = web_stream_bridge::jet_web_stream_rollback(&stream_id, at_ns)
                    .map_err(web_stream_error)?;
                Ok(web_stream_success(
                    &action,
                    &format!("\"receipt\":{}", receipt.render_json()),
                ))
            }
            "hydration_start" => {
                let stream_id = web_stream_id(object, "stream_id")?;
                let trigger = web_stream_trigger(object, "trigger")?;
                let started_at_ns = web_stream_required_u64(object, "started_at_ns")?;
                let receipt = web_stream_bridge::jet_web_stream_hydration_start(
                    &stream_id,
                    trigger,
                    started_at_ns,
                )
                .map_err(web_stream_error)?;
                Ok(web_stream_success(
                    &action,
                    &format!("\"receipt\":{}", receipt.render_json()),
                ))
            }
            "hydration_complete" => {
                let stream_id = web_stream_id(object, "stream_id")?;
                let completed_at_ns = web_stream_required_u64(object, "completed_at_ns")?;
                let receipt = web_stream_bridge::jet_web_stream_hydration_complete(
                    &stream_id,
                    completed_at_ns,
                )
                .map_err(web_stream_error)?;
                Ok(web_stream_success(
                    &action,
                    &format!("\"receipt\":{}", receipt.render_json()),
                ))
            }
            "receipt" => {
                let boundary_id = web_stream_boundary_id(object, "boundary_id")?;
                let receipt = web_stream_bridge::jet_web_stream_receipt_json(&boundary_id)
                    .map_err(web_stream_error)?;
                Ok(web_stream_success(
                    &action,
                    &format!("\"receipt\":{receipt}"),
                ))
            }
            "projection" => {
                let boundary_id = web_stream_boundary_id(object, "boundary_id")?;
                let target = web_stream_target(object, "target")?;
                let projection =
                    web_stream_bridge::jet_web_stream_projection_json(&boundary_id, target)
                        .map_err(web_stream_error)?;
                Ok(web_stream_success(
                    &action,
                    &format!("\"projection\":{projection}"),
                ))
            }
            _ => Err(web_stream_request_error(format!(
                "web stream action `{action}` is not recognized"
            ))),
        }
    })();
    match result {
        Ok(body) => write_response(
            stream,
            "200 OK",
            "application/json; charset=utf-8",
            body.as_bytes(),
        ),
        Err((code, message)) => write_response(
            stream,
            "400 Bad Request",
            "application/json; charset=utf-8",
            web_stream_failure_json(&code, &message).as_bytes(),
        ),
    }
}

fn unauthorized(stream: &mut impl Write) -> std::io::Result<()> {
    write_response(
        stream,
        "401 Unauthorized",
        "text/plain; charset=utf-8",
        b"devserver session, host, or origin rejected",
    )
}
fn handle_release_devtools_request(
    stream: &mut impl Write,
    request: &Request,
    path: &str,
    bind_host: &str,
    port: u16,
    peer_addr: SocketAddr,
    session: &crate::ResidentDevSession,
    policy: &ReleaseDevtoolsPolicy,
) -> std::io::Result<()> {
    if path != "/__jet_devtools" || !policy.endpoint_enabled() {
        return write_response(
            stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"inspect endpoint absent",
        );
    }
    if request.method != "GET" {
        return method_not_allowed(stream);
    }
    if !policy.deployment_enabled() {
        return write_response(
            stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"inspect endpoint disabled",
        );
    }
    let Some(host) = request.headers.get("host") else {
        return write_response(
            stream,
            "403 Forbidden",
            "text/plain; charset=utf-8",
            b"inspect host rejected",
        );
    };
    if !host_header_allowed(host, bind_host, port) {
        return write_response(
            stream,
            "403 Forbidden",
            "text/plain; charset=utf-8",
            b"inspect host rejected",
        );
    }
    if request
        .headers
        .get("origin")
        .is_some_and(|origin| !origin_allowed(origin, bind_host, port))
    {
        return write_response(
            stream,
            "403 Forbidden",
            "text/plain; charset=utf-8",
            b"inspect origin rejected",
        );
    }
    let Some(expected_token) = policy.deployment.token() else {
        return write_response(
            stream,
            "401 Unauthorized",
            "application/json; charset=utf-8",
            br#"{"error":"JET_INSPECT_TOKEN required"}"#,
        );
    };
    let token_valid = request
        .headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| constant_time_equal(token, expected_token));
    if !token_valid {
        return write_response(
            stream,
            "401 Unauthorized",
            "application/json; charset=utf-8",
            br#"{"error":"JET_INSPECT_TOKEN required"}"#,
        );
    }
    if !policy.peer_allowed(peer_addr.ip()) {
        return write_response(
            stream,
            "403 Forbidden",
            "application/json; charset=utf-8",
            br#"{"error":"inspect network address not allowed"}"#,
        );
    }
    if !request.body.is_empty() || request.headers.contains_key("transfer-encoding") {
        return write_response(
            stream,
            "400 Bad Request",
            "text/plain; charset=utf-8",
            b"inspect endpoint accepts GET without a request body",
        );
    }
    let projection = session.devtools_events_since(None);
    let panels = match session.devtools_panels(
        crate::Devtools::JetDevtoolsHostKind::BrowserInApp,
        &crate::Devtools::JetDevtoolsPanelCapability::ALL,
    ) {
        Ok(panels) => panels,
        Err(error) => {
            return write_response(
                stream,
                "500 Internal Server Error",
                "text/plain; charset=utf-8",
                error.to_string().as_bytes(),
            )
        }
    };
    let body = browser_web_adapter::release_json(&projection, &panels);
    write_response(
        stream,
        "200 OK",
        "application/json; charset=utf-8",
        body.as_bytes(),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ListenerKind {
    Canvas,
    Application,
}

struct DeadlineStream {
    stream: TcpStream,
    deadline: Instant,
}

impl DeadlineStream {
    fn new(stream: TcpStream, deadline: Instant) -> Self {
        Self { stream, deadline }
    }

    fn try_clone(&self) -> std::io::Result<Self> {
        Ok(Self::new(self.stream.try_clone()?, self.deadline))
    }

    fn remaining(&self) -> std::io::Result<Duration> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "devserver request deadline exceeded",
            ))
        } else {
            Ok(remaining)
        }
    }
}

impl std::io::Read for DeadlineStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self.remaining()?;
        self.stream.set_read_timeout(Some(remaining))?;
        self.stream.read(buffer)
    }
}

impl Write for DeadlineStream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let remaining = self.remaining()?;
        self.stream.set_write_timeout(Some(remaining))?;
        self.stream.write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let remaining = self.remaining()?;
        self.stream.set_write_timeout(Some(remaining))?;
        self.stream.flush()
    }
}

/// Accept connections forever, one thread per connection (a dev tool serving
/// a handful of small files has no need for a worker pool).
fn serve_forever(
    listener: TcpListener,
    status: Arc<DevStatus>,
    debug_sessions: Arc<crate::Canvas::DebugSessions>,
    session: Arc<crate::ResidentDevSession>,
    canvas_file: String,
    listener_kind: ListenerKind,
    bind_host: String,
    session_secret: String,
    shutdown: Arc<AtomicBool>,
    static_root: PathBuf,
    release_policy: ReleaseDevtoolsPolicy,
    browser_state: Arc<Mutex<Option<browser_web_adapter::BrowserWebState>>>,
    paused_evaluate: Arc<Mutex<crate::PausedEvaluate::PausedEvaluateHost>>,
    terminal_style: Arc<crate::TerminalStyleReload::TerminalStyleHostAdapter>,
) {
    if listener.set_nonblocking(true).is_err() {
        return;
    }
    let active_connections = Arc::new(AtomicUsize::new(0));
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, peer_addr)) => {
                if !try_acquire_connection(&active_connections) {
                    drop(stream);
                    continue;
                }
                let status = Arc::clone(&status);
                let debug_sessions = Arc::clone(&debug_sessions);
                let session = Arc::clone(&session);
                let canvas_file = canvas_file.clone();
                let bind_host = bind_host.clone();
                let session_secret = session_secret.clone();
                let static_root = static_root.clone();
                let release_policy = release_policy.clone();
                let browser_state = Arc::clone(&browser_state);
                let paused_evaluate = Arc::clone(&paused_evaluate);
                let terminal_style = Arc::clone(&terminal_style);
                let active_connections = Arc::clone(&active_connections);
                thread::spawn(move || {
                    let _ = handle_connection_with_root_and_policy(
                        stream,
                        &status,
                        &debug_sessions,
                        &session,
                        &canvas_file,
                        listener_kind,
                        &bind_host,
                        &session_secret,
                        &static_root,
                        &browser_state,
                        &paused_evaluate,
                        &terminal_style,
                        &release_policy,
                        peer_addr,
                    );
                    active_connections.fetch_sub(1, Ordering::AcqRel);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let _ = wait_for_shutdown(&shutdown, Duration::from_millis(10));
            }
            Err(_) => return,
        }
    }
}

fn try_acquire_connection(active: &AtomicUsize) -> bool {
    let mut current = active.load(Ordering::Acquire);
    loop {
        if current >= MAX_CONNECTION_THREADS {
            return false;
        }
        match active.compare_exchange(current, current + 1, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return true,
            Err(next) => current = next,
        }
    }
}

/// Parse one GET request line + headers (no keep-alive, no request body —
/// this is a dev tool, not a production server) and serve a response.
#[cfg(test)]
fn handle_connection(
    stream: TcpStream,
    status: &DevStatus,
    debug_sessions: &crate::Canvas::DebugSessions,
    session: &crate::ResidentDevSession,
    canvas_file: &str,
    listener_kind: ListenerKind,
    bind_host: &str,
    session_secret: &str,
) -> std::io::Result<()> {
    handle_connection_with_root(
        stream,
        status,
        debug_sessions,
        session,
        canvas_file,
        listener_kind,
        bind_host,
        session_secret,
        Path::new("build"),
    )
}

#[cfg(test)]
fn handle_connection_with_root(
    stream: TcpStream,
    status: &DevStatus,
    debug_sessions: &crate::Canvas::DebugSessions,
    session: &crate::ResidentDevSession,
    canvas_file: &str,
    listener_kind: ListenerKind,
    bind_host: &str,
    session_secret: &str,
    static_root: &Path,
) -> std::io::Result<()> {
    let browser_state = Arc::new(Mutex::new(None));
    let paused_evaluate = Arc::new(Mutex::new(crate::PausedEvaluate::PausedEvaluateHost::new()));
    let terminal_style = Arc::new(crate::TerminalStyleReload::TerminalStyleHostAdapter::new());
    handle_connection_with_root_and_policy(
        stream,
        status,
        debug_sessions,
        session,
        canvas_file,
        listener_kind,
        bind_host,
        session_secret,
        static_root,
        &browser_state,
        &paused_evaluate,
        &terminal_style,
        &ReleaseDevtoolsPolicy::development(),
        SocketAddr::new(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 0),
    )
}

fn handle_connection_with_root_and_policy(
    stream: TcpStream,
    status: &DevStatus,
    debug_sessions: &crate::Canvas::DebugSessions,
    session: &crate::ResidentDevSession,
    canvas_file: &str,
    listener_kind: ListenerKind,
    bind_host: &str,
    session_secret: &str,
    static_root: &Path,
    browser_state: &Arc<Mutex<Option<browser_web_adapter::BrowserWebState>>>,
    paused_evaluate: &Arc<Mutex<crate::PausedEvaluate::PausedEvaluateHost>>,
    terminal_style: &Arc<crate::TerminalStyleReload::TerminalStyleHostAdapter>,
    release_policy: &ReleaseDevtoolsPolicy,
    peer_addr: SocketAddr,
) -> std::io::Result<()> {
    let deadline = Instant::now() + REQUEST_DEADLINE;
    let mut stream = DeadlineStream::new(stream, deadline);
    let mut reader = BufReader::new(stream.try_clone()?);
    let Some(request) = Request::read(&mut reader)? else {
        return Ok(());
    };

    let method = request.method.as_str();
    let target = request.target.as_str();
    let body = request.body.as_slice();
    let path = target.split('?').next().unwrap_or("/");
    if listener_kind == ListenerKind::Application
        && release_policy.is_release()
        && devtools_path(path)
    {
        return handle_release_devtools_request(
            &mut stream,
            &request,
            path,
            bind_host,
            session.application_port(),
            peer_addr,
            session,
            release_policy,
        );
    }
    if requires_session_authorization(listener_kind, path)
        && !canvas_request_authorized(
            &request,
            target,
            bind_host,
            if listener_kind == ListenerKind::Canvas {
                session.canvas_port()
            } else {
                session.application_port()
            },
            session_secret,
        )
    {
        return unauthorized(&mut stream);
    }

    if method != "GET" && method != "POST" {
        return write_response(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"jet dev's web server only handles GET and Canvas POST requests",
        );
    }

    // The devtools page runs on the application origin.  Its source tools
    // reach the existing Canvas source/command/transaction handlers below
    // through the same session authorization, but only while this host has
    // Canvas source authority; release builds and application-only hosts get
    // one explicit reason instead of a silent static 404.
    if listener_kind == ListenerKind::Application && source_tool_path(path) {
        if release_policy.is_release() {
            return write_response(
                &mut stream,
                "404 Not Found",
                "text/plain; charset=utf-8",
                b"source tools are absent in release builds",
            );
        }
        if session.canvas_port() == 0 {
            return write_response(
                &mut stream,
                "404 Not Found",
                "text/plain; charset=utf-8",
                browser_web_adapter::SOURCE_TOOLS_OFF_REASON.as_bytes(),
            );
        }
    }

    if listener_kind == ListenerKind::Canvas {
        if let Some(client) = query_param(target, "client_id") {
            session.note_client(&client);
        }
    }
    if listener_kind == ListenerKind::Application && !is_application_control_path(path) {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let started = Instant::now();
        let nonce = status
            .browser_relay
            .lock()
            .unwrap()
            .as_ref()
            .map(|relay| relay.nonce().to_string())
            .unwrap_or_default();
        let code = serve_static_from_root(&mut stream, static_root, path, &nonce, session_secret)?;
        status.log_request(method, path, code, started.elapsed());
        return Ok(());
    }
    if listener_kind == ListenerKind::Application && web_stream_path(path) {
        return handle_web_stream_request(&mut stream, &request);
    }
    if devtools_path(path) {
        if let Some(client) = query_param(target, "client_id") {
            session.note_client(&client);
        }
        let placement = match path {
            "/__jet_devtools/workbench" => crate::BrowserHost::BrowserPlacement::Workbench,
            _ => crate::BrowserHost::BrowserPlacement::InApp,
        };
        if method == "GET"
            && listener_kind == ListenerKind::Application
            && matches!(path, "/__jet_devtools" | "/__jet_devtools/workbench")
        {
            let mut state = browser_state.lock().unwrap();
            return match browser_web_adapter::sync_session(&mut state, session) {
                Ok(()) => {
                    let body = browser_web_adapter::render_page(
                        state.as_ref().expect("browser web state initialized"),
                        placement,
                    );
                    write_response(
                        &mut stream,
                        "200 OK",
                        "text/html; charset=utf-8",
                        body.as_bytes(),
                    )
                }
                Err(error) => write_response(
                    &mut stream,
                    "500 Internal Server Error",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                ),
            };
        }
        if path == "/__jet_devtools/reconnect" && method == "GET" {
            let cursor = match query_param(target, "cursor") {
                Some(value) => match value.parse::<u64>() {
                    Ok(cursor) => Some(cursor),
                    Err(_) => {
                        return write_response(
                            &mut stream,
                            "400 Bad Request",
                            "text/plain; charset=utf-8",
                            b"cursor must be a non-negative integer",
                        )
                    }
                },
                None => None,
            };
            let mut state = browser_state.lock().unwrap();
            return match browser_web_adapter::sync_session(&mut state, session) {
                Ok(()) => {
                    let projection = session.devtools_events_since(cursor);
                    let state = state.as_ref().expect("browser web state initialized");
                    match browser_web_adapter::reconnect_json(
                        state,
                        cursor,
                        projection.reset,
                        projection.truncation,
                    ) {
                        Ok(body) => write_response(
                            &mut stream,
                            "200 OK",
                            "application/json; charset=utf-8",
                            body.as_bytes(),
                        ),
                        Err(error) => write_response(
                            &mut stream,
                            "409 Conflict",
                            "text/plain; charset=utf-8",
                            error.as_bytes(),
                        ),
                    }
                }
                Err(error) => write_response(
                    &mut stream,
                    "500 Internal Server Error",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                ),
            };
        }
        if path == "/__jet_devtools/state" && method == "GET" {
            let mut state = browser_state.lock().unwrap();
            return match browser_web_adapter::sync_session(&mut state, session) {
                Ok(()) => {
                    let body = browser_web_adapter::view_json(
                        state.as_ref().expect("browser web state initialized"),
                        placement,
                    );
                    write_response(
                        &mut stream,
                        "200 OK",
                        "application/json; charset=utf-8",
                        body.as_bytes(),
                    )
                }
                Err(error) => write_response(
                    &mut stream,
                    "500 Internal Server Error",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                ),
            };
        }
        if path == "/__jet_devtools/swap" && method == "GET" {
            let mut state = browser_state.lock().unwrap();
            return match browser_web_adapter::sync_session(&mut state, session) {
                Ok(()) => write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    browser_web_adapter::swap_json(
                        state.as_ref().expect("browser web state initialized"),
                    )
                    .as_bytes(),
                ),
                Err(error) => write_response(
                    &mut stream,
                    "500 Internal Server Error",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                ),
            };
        }
        if path == "/__jet_devtools/swap" && method == "POST" {
            let mut state = browser_state.lock().unwrap();
            let result = browser_web_adapter::sync_session(&mut state, session).and_then(|()| {
                browser_web_adapter::record_dom_snapshot(&mut state, session.id(), body)
            });
            return match result {
                Ok(()) => write_response(
                    &mut stream,
                    "204 No Content",
                    "text/plain; charset=utf-8",
                    b"",
                ),
                Err(error) => write_response(
                    &mut stream,
                    "400 Bad Request",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                ),
            };
        }
        if path == "/__jet_devtools/recording" && release_policy.is_release() {
            return write_response(
                &mut stream,
                "404 Not Found",
                "text/plain; charset=utf-8",
                b"recording endpoint absent in release builds",
            );
        }
        if path == "/__jet_devtools/recording" && method == "GET" {
            let body = if let Some(value) = query_param(target, "sequence") {
                let sequence = match value.parse::<u64>() {
                    Ok(sequence) => sequence,
                    Err(_) => {
                        return write_response(
                            &mut stream,
                            "400 Bad Request",
                            "text/plain; charset=utf-8",
                            b"sequence must be a non-negative integer",
                        )
                    }
                };
                session.recording_control(&format!("{{\"op\":\"scrub\",\"sequence\":{sequence}}}"))
            } else if let Some(value) = query_param(target, "timestamp_ms") {
                let timestamp_ms = match value.parse::<u64>() {
                    Ok(timestamp_ms) => timestamp_ms,
                    Err(_) => {
                        return write_response(
                            &mut stream,
                            "400 Bad Request",
                            "text/plain; charset=utf-8",
                            b"timestamp_ms must be a non-negative integer",
                        )
                    }
                };
                session.recording_control(&format!(
                    "{{\"op\":\"scrub_time\",\"timestamp_ms\":{timestamp_ms}}}"
                ))
            } else {
                Ok(session.recording_json())
            };
            return match body {
                Ok(body) => write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    body.as_bytes(),
                ),
                Err(error) => write_response(
                    &mut stream,
                    "409 Conflict",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                ),
            };
        }
        if path == "/__jet_devtools/recording" && method == "POST" {
            let payload = std::str::from_utf8(body).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "recording control must be UTF-8 JSON",
                )
            })?;
            return match session.recording_control(payload) {
                Ok(body) => write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    body.as_bytes(),
                ),
                Err(error) => write_response(
                    &mut stream,
                    "409 Conflict",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                ),
            };
        }
        if path == "/__jet_devtools/action" && method == "POST" {
            let action = match browser_web_adapter::parse_action(body, placement) {
                Ok(action) => action,
                Err(error) => {
                    return write_response(
                        &mut stream,
                        "400 Bad Request",
                        "text/plain; charset=utf-8",
                        error.as_bytes(),
                    )
                }
            };
            let mut state = browser_state.lock().unwrap();
            return match browser_web_adapter::sync_session(&mut state, session) {
                Ok(()) => match browser_web_adapter::dispatch_action(
                    state.as_mut().expect("browser web state initialized"),
                    action,
                ) {
                    Ok(body) => write_response(
                        &mut stream,
                        "200 OK",
                        "application/json; charset=utf-8",
                        body.as_bytes(),
                    ),
                    Err(error) => write_response(
                        &mut stream,
                        "403 Forbidden",
                        "text/plain; charset=utf-8",
                        error.to_string().as_bytes(),
                    ),
                },
                Err(error) => write_response(
                    &mut stream,
                    "500 Internal Server Error",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                ),
            };
        }
        if path == "/__jet_devtools/commands" && method == "GET" {
            return match session.browser_game_control_frame() {
                Ok(body) => write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    body.as_bytes(),
                ),
                Err(error) => write_response(
                    &mut stream,
                    "500 Internal Server Error",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                ),
            };
        }

        if path == "/__jet_devtools/assets" && method == "GET" {
            return match session.browser_game_asset_frame() {
                Ok(body) => write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    body.as_bytes(),
                ),
                Err(error) => write_response(
                    &mut stream,
                    "500 Internal Server Error",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                ),
            };
        }

        if method == "GET"
            && matches!(
                path,
                "/__jet_devtools"
                    | "/__jet_canvas/devtools"
                    | "/canvas/devtools"
                    | "/panel/devtools"
            )
        {
            let mut state = browser_state.lock().unwrap();
            return match browser_web_adapter::sync_session(&mut state, session) {
                Ok(()) => {
                    let body = browser_web_adapter::view_json(
                        state.as_ref().expect("browser web state initialized"),
                        placement,
                    );
                    write_response(
                        &mut stream,
                        "200 OK",
                        "application/json; charset=utf-8",
                        body.as_bytes(),
                    )
                }
                Err(error) => write_response(
                    &mut stream,
                    "500 Internal Server Error",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                ),
            };
        }
        if path == "/__jet_devtools" && method == "POST" {
            let payload = std::str::from_utf8(body).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "devtools event must be UTF-8",
                )
            })?;
            return match decode_devtools_envelope(payload).and_then(|decoded| {
                session.ingest_devtools_envelope(decoded)?;
                session.dispatch_pending_database_explain().map(|_| ())
            }) {
                Ok(()) => {
                    let mut state = browser_state.lock().unwrap();
                    match browser_web_adapter::sync_session(&mut state, session) {
                        Ok(()) => {
                            let body = browser_web_adapter::view_json(
                                state.as_ref().expect("browser web state initialized"),
                                placement,
                            );
                            write_response(
                                &mut stream,
                                "200 OK",
                                "application/json; charset=utf-8",
                                body.as_bytes(),
                            )
                        }
                        Err(error) => write_response(
                            &mut stream,
                            "500 Internal Server Error",
                            "text/plain; charset=utf-8",
                            error.as_bytes(),
                        ),
                    }
                }
                Err(error) => write_response(
                    &mut stream,
                    "400 Bad Request",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                ),
            };
        }
        if path == "/__jet_devtools/selection" && method == "POST" {
            let selection = match browser_web_adapter::parse_selection(body) {
                Ok(selection) => selection,
                Err(error) => {
                    return write_response(
                        &mut stream,
                        "400 Bad Request",
                        "text/plain; charset=utf-8",
                        error.as_bytes(),
                    )
                }
            };
            if let Some((panel_id, _)) = &selection {
                let panels = match session.devtools_panels(
                    crate::Devtools::JetDevtoolsHostKind::BrowserInApp,
                    &crate::Devtools::JetDevtoolsPanelCapability::ALL,
                ) {
                    Ok(panels) => panels,
                    Err(error) => {
                        let error = error.to_string();
                        return write_response(
                            &mut stream,
                            "500 Internal Server Error",
                            "text/plain; charset=utf-8",
                            error.as_bytes(),
                        );
                    }
                };
                if !panels
                    .iter()
                    .any(|panel| panel.availability.descriptor.id.as_str() == panel_id)
                {
                    return write_response(
                        &mut stream,
                        "400 Bad Request",
                        "text/plain; charset=utf-8",
                        b"browser selection panel_id must match the canonical panel catalog",
                    );
                }
            }
            let payload = std::str::from_utf8(body).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "devtools selection must be UTF-8",
                )
            })?;
            return match session.set_devtools_selection(payload) {
                Ok(()) => {
                    let mut state = browser_state.lock().unwrap();
                    match browser_web_adapter::sync_session(&mut state, session) {
                        Ok(()) => {
                            let body = browser_web_adapter::view_json(
                                state.as_ref().expect("browser web state initialized"),
                                placement,
                            );
                            write_response(
                                &mut stream,
                                "200 OK",
                                "application/json; charset=utf-8",
                                body.as_bytes(),
                            )
                        }
                        Err(error) => write_response(
                            &mut stream,
                            "500 Internal Server Error",
                            "text/plain; charset=utf-8",
                            error.as_bytes(),
                        ),
                    }
                }
                Err(error) => write_response(
                    &mut stream,
                    "400 Bad Request",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                ),
            };
        }
        if path == "/__jet_devtools/cursor" && method == "POST" {
            let payload = std::str::from_utf8(body).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "devtools cursor must be UTF-8",
                )
            })?;
            return match session.set_devtools_cursor(payload) {
                Ok(()) => {
                    let mut state = browser_state.lock().unwrap();
                    match browser_web_adapter::sync_session(&mut state, session) {
                        Ok(()) => {
                            let body = browser_web_adapter::view_json(
                                state.as_ref().expect("browser web state initialized"),
                                placement,
                            );
                            write_response(
                                &mut stream,
                                "200 OK",
                                "application/json; charset=utf-8",
                                body.as_bytes(),
                            )
                        }
                        Err(error) => write_response(
                            &mut stream,
                            "500 Internal Server Error",
                            "text/plain; charset=utf-8",
                            error.as_bytes(),
                        ),
                    }
                }
                Err(error) => write_response(
                    &mut stream,
                    "400 Bad Request",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                ),
            };
        }
        if path == "/__jet_devtools/error" && method == "GET" {
            let facts = status.web_error_facts();
            let devtools_projection = session.devtools_events_since(None);
            let request_id = next_web_error_request_id();
            let source_id = devtools_projection
                .source_id
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| canvas_file.to_string());
            let build_id = devtools_projection
                .build_id
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    fs::read_to_string(static_root.join("web.manifest.json"))
                        .ok()
                        .and_then(|manifest| {
                            browser_web_adapter::manifest_artifact_identity(&manifest).ok()
                        })
                });
            let Some(build_id) = build_id else {
                return write_response(
                    &mut stream,
                    "404 Not Found",
                    "text/plain; charset=utf-8",
                    b"error identity unavailable",
                );
            };
            let accept = request
                .headers
                .get("accept")
                .map(String::as_str)
                .unwrap_or("*/*");
            let identity = crate::WebErrorPage::WebErrorPageIdentity::new(
                request_id,
                session.id(),
                source_id.clone(),
                build_id,
                devtools_projection.revision.clone(),
            )
            .map_err(|error| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
            })?;
            let rendered = if !facts.is_error() {
                Ok(None)
            } else {
                let source_excerpts =
                    crate::Canvas::read_source_without_symlinks(Path::new(canvas_file))
                        .ok()
                        .and_then(|source| {
                            let excerpt = source
                                .chars()
                                .take(crate::WebErrorPage::MAX_WEB_ERROR_SOURCE_BYTES)
                                .collect::<String>();
                            crate::WebErrorPage::WebErrorSourceExcerpt::without_span(
                                source_id.clone(),
                                "failure source",
                                Some(excerpt),
                            )
                            .ok()
                        })
                        .into_iter()
                        .collect::<Vec<_>>();
                let action_links = [
                    ("Open devtools", "/__jet_devtools"),
                    ("Reconnect", "/__jet_devtools/reconnect"),
                    ("Open recording", "/__jet_devtools/recording"),
                ]
                .into_iter()
                .filter_map(|(label, href)| {
                    crate::WebErrorPage::WebErrorActionLink::new(label, href).ok()
                })
                .collect::<Vec<_>>();
                let page = if facts.is_internal_failure() {
                    crate::WebErrorPage::WebErrorPage::from_internal(
                        identity,
                        facts.code.clone(),
                        facts.diagnostic.clone(),
                        action_links,
                    )
                    .and_then(|page| page.with_source_excerpts(source_excerpts))
                } else {
                    crate::WebErrorPage::WebErrorDiagnosticFact::from_dev_status(&facts).and_then(
                        |diagnostic| {
                            crate::WebErrorPage::WebErrorPage::new_with_status(
                                500,
                                identity,
                                diagnostic,
                                source_excerpts,
                                None,
                                action_links,
                            )
                        },
                    )
                };
                page.and_then(|page| {
                    page.project(accept, crate::WebErrorPage::WebErrorCapabilities::local())
                        .map(Some)
                })
                .map_err(|error| error.to_string())
            };
            return match rendered {
                Ok(Some(projection)) => {
                    let status = match projection.status {
                        200 => "200 OK",
                        400 => "400 Bad Request",
                        401 => "401 Unauthorized",
                        403 => "403 Forbidden",
                        404 => "404 Not Found",
                        405 => "405 Method Not Allowed",
                        409 => "409 Conflict",
                        413 => "413 Payload Too Large",
                        415 => "415 Unsupported Media Type",
                        422 => "422 Unprocessable Content",
                        429 => "429 Too Many Requests",
                        500 => "500 Internal Server Error",
                        501 => "501 Not Implemented",
                        502 => "502 Bad Gateway",
                        503 => "503 Service Unavailable",
                        504 => "504 Gateway Timeout",
                        _ if projection.status >= 400 && projection.status < 500 => {
                            "400 Bad Request"
                        }
                        _ => "500 Internal Server Error",
                    };
                    write_response(
                        &mut stream,
                        status,
                        projection.content_type.as_str(),
                        projection.body.as_bytes(),
                    )
                }
                Ok(None) => write_response(
                    &mut stream,
                    "204 No Content",
                    "text/plain; charset=utf-8",
                    b"",
                ),
                Err(error) => write_response(
                    &mut stream,
                    "500 Internal Server Error",
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                ),
            };
        }
        if path == "/__jet_devtools/paused/inspect" && method == "POST" {
            let payload = std::str::from_utf8(body).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "paused inspect body must be UTF-8",
                )
            })?;
            let request = match crate::PausedEvaluate::InspectRequest::from_json(payload) {
                Ok(request) => request,
                Err(error) => {
                    let error =
                        crate::PausedEvaluate::PausedEvaluateHostError::InvalidRequest(error);
                    return write_response(
                        &mut stream,
                        paused_evaluate_error_status(&error),
                        "application/json; charset=utf-8",
                        error.json().as_bytes(),
                    );
                }
            };
            let mut host = paused_evaluate.lock().unwrap();
            let adapter = match debug_sessions.paused_adapter(&request.identity) {
                Ok(adapter) => adapter,
                Err(_) => {
                    host.clear_session();
                    let error = crate::PausedEvaluate::PausedEvaluateHostError::SessionUnavailable;
                    return write_response(
                        &mut stream,
                        paused_evaluate_error_status(&error),
                        "application/json; charset=utf-8",
                        error.json().as_bytes(),
                    );
                }
            };
            if adapter.inject_into(&mut host).is_err() {
                host.clear_session();
                let error = crate::PausedEvaluate::PausedEvaluateHostError::SessionUnavailable;
                return write_response(
                    &mut stream,
                    paused_evaluate_error_status(&error),
                    "application/json; charset=utf-8",
                    error.json().as_bytes(),
                );
            }
            return match host.handle_inspect(payload) {
                Ok(json) => write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    json.as_bytes(),
                ),
                Err(error) => write_response(
                    &mut stream,
                    paused_evaluate_error_status(&error),
                    "application/json; charset=utf-8",
                    error.json().as_bytes(),
                ),
            };
        }
        if path == "/__jet_devtools/paused/evaluate" && method == "POST" {
            let payload = std::str::from_utf8(body).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "paused evaluate body must be UTF-8",
                )
            })?;
            let request = match crate::PausedEvaluate::EvaluateRequest::from_json(payload) {
                Ok(request) => request,
                Err(error) => {
                    let error =
                        crate::PausedEvaluate::PausedEvaluateHostError::InvalidRequest(error);
                    return write_response(
                        &mut stream,
                        paused_evaluate_error_status(&error),
                        "application/json; charset=utf-8",
                        error.json().as_bytes(),
                    );
                }
            };
            let mut host = paused_evaluate.lock().unwrap();
            let adapter = match debug_sessions.paused_adapter(&request.identity) {
                Ok(adapter) => adapter,
                Err(_) => {
                    host.clear_session();
                    let error = crate::PausedEvaluate::PausedEvaluateHostError::SessionUnavailable;
                    return write_response(
                        &mut stream,
                        paused_evaluate_error_status(&error),
                        "application/json; charset=utf-8",
                        error.json().as_bytes(),
                    );
                }
            };
            if adapter.inject_into(&mut host).is_err() {
                host.clear_session();
                let error = crate::PausedEvaluate::PausedEvaluateHostError::SessionUnavailable;
                return write_response(
                    &mut stream,
                    paused_evaluate_error_status(&error),
                    "application/json; charset=utf-8",
                    error.json().as_bytes(),
                );
            }
            host.set_evaluator_result(adapter.evaluate(&request));
            return match host.handle_evaluate(payload) {
                Ok(json) => write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    json.as_bytes(),
                ),
                Err(error) => write_response(
                    &mut stream,
                    paused_evaluate_error_status(&error),
                    "application/json; charset=utf-8",
                    error.json().as_bytes(),
                ),
            };
        }
        if path == "/__jet_devtools/style" && method == "GET" {
            let (_, event) = terminal_style.current_json();
            return write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                event.as_bytes(),
            );
        }
        if path == "/__jet_devtools/style" && method == "POST" {
            let payload = std::str::from_utf8(body).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "style request must be UTF-8",
                )
            })?;
            let (_, event) = terminal_style.apply_json(payload);
            return write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                event.as_bytes(),
            );
        }
        return method_not_allowed(&mut stream);
    }
    if path == "/__jet_canvas/session" || path == "/canvas/session" || path == "/panel/session" {
        let client = query_param(target, "client_id").unwrap_or_default();
        if !client.is_empty() {
            session.note_client(&client);
        }
        if method == "GET" {
            let body = session_response(session);
            return write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
            );
        }
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let body = session_response(session);
        return write_response(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            body.as_bytes(),
        );
    }
    if path == "/__jet_canvas/live" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let pid = query_param(target, "pid")
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing live pid")
            })?;
        let source_id = query_param(target, "source_id").unwrap_or_default();
        return match crate::LiveInspect::read(pid) {
            Ok(snapshot) => {
                let revision = current_revision_for_source_id(
                    canvas_file,
                    (!source_id.is_empty()).then_some(source_id.as_str()),
                );
                session.remember_last_good_view("runtime", &source_id, &revision, &snapshot);
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    snapshot.as_bytes(),
                )
            }
            Err(message) => match session.last_good_view("runtime", &source_id) {
                Some((_revision, snapshot)) => write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    snapshot.as_bytes(),
                ),
                None => write_response(
                    &mut stream,
                    "404 Not Found",
                    "text/plain; charset=utf-8",
                    message.as_bytes(),
                ),
            },
        };
    }
    if let Some(asset) = crate::canvas_asset(method, target, path) {
        let asset = inject_canvas_session(asset, session_secret);
        return write_response(
            &mut stream,
            asset.status,
            asset.content_type,
            asset.body.as_bytes(),
        );
    }
    if target == "/?jet_panel_graph=1" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        return match crate::Canvas::graph_json_for_file(Path::new(canvas_file)) {
            Ok(body) => {
                session.select_project_source_from_payload(&body);
                let source_id = source_id_from_payload(&body);
                let revision = current_revision_for_source_id(
                    canvas_file,
                    (!source_id.is_empty()).then_some(source_id.as_str()),
                );
                session.remember_last_good_view("graph", &source_id, &revision, &body);
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    with_session(body, session).as_bytes(),
                )
            }
            Err(diags) => {
                let body = with_session(
                    crate::Canvas::graph_json_error_for_file(Path::new(canvas_file), &diags),
                    session,
                );
                write_response(
                    &mut stream,
                    "409 Conflict",
                    "application/json; charset=utf-8",
                    body.as_bytes(),
                )
            }
        };
    }
    if path == "/__jet_canvas/graph" || path == "/canvas/graph" || path == "/panel/graph" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let source_id = query_param(target, "source_id");
        let graph = match query_param(target, "pid") {
            Some(pid) => crate::Canvas::graph_json_for_entry_source_with_live_pid(
                Path::new(canvas_file),
                source_id.as_deref(),
                pid.parse().unwrap_or(0),
            ),
            None => crate::Canvas::graph_json_for_entry_source(
                Path::new(canvas_file),
                source_id.as_deref(),
            ),
        };
        if let Some(source_id) = source_id.as_deref() {
            session.select_project_source(source_id);
        }
        return match graph {
            Ok(body) => {
                session.select_project_source_from_payload(&body);
                let source_id = source_id
                    .clone()
                    .unwrap_or_else(|| source_id_from_payload(&body));
                let revision = current_revision_for_source_id(
                    canvas_file,
                    (!source_id.is_empty()).then_some(source_id.as_str()),
                );
                session.remember_last_good_view("graph", &source_id, &revision, &body);
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    with_session(body, session).as_bytes(),
                )
            }
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/project" || path == "/canvas/project" || path == "/panel/project" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let body = with_session(
            crate::Canvas::project_json_for_entry(Path::new(canvas_file)),
            session,
        );
        return write_response(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            body.as_bytes(),
        );
    }
    if path == "/__jet_canvas/source-control"
        || path == "/canvas/source-control"
        || path == "/panel/source-control"
    {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let body = with_session(
            crate::Canvas::source_control_json_for_entry(Path::new(canvas_file)),
            session,
        );
        return write_response(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            body.as_bytes(),
        );
    }
    if path == "/__jet_canvas/core-catalog"
        || path == "/canvas/core-catalog"
        || path == "/panel/core-catalog"
    {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let query = query_param(target, "query").unwrap_or_default();
        return match crate::Canvas::core_catalog_json_for_entry(Path::new(canvas_file), &query) {
            Ok(body) => write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
            ),
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/proof" || path == "/canvas/proof" || path == "/panel/proof" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let source_id = query_param(target, "source_id");
        let receipt = status.command_receipt();
        return match crate::Canvas::proof_json_for_entry_with_receipt(
            Path::new(canvas_file),
            source_id.as_deref(),
            receipt.as_deref(),
        ) {
            Ok(body) => write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
            ),
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/source" || path == "/canvas/source" || path == "/panel/source" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let source_id = query_param(target, "source_id");
        let source_path = source_id
            .as_deref()
            .and_then(|id| crate::Canvas::project_path_for_source_id(Path::new(canvas_file), id))
            .unwrap_or_else(|| PathBuf::from(canvas_file));
        return match read_source_without_symlinks(&source_path) {
            Ok(source) => write_response(
                &mut stream,
                "200 OK",
                "text/plain; charset=utf-8",
                source.as_bytes(),
            ),
            Err(e) => write_response(
                &mut stream,
                "404 Not Found",
                "text/plain; charset=utf-8",
                format!("could not read Canvas source: {e}").as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/command" || path == "/canvas/command" || path == "/panel/command" {
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
        let request = String::from_utf8_lossy(&body);
        let _transaction = session.lock_source_transaction();
        return match crate::Canvas::command_receipt_json_for_entry(Path::new(canvas_file), &request)
        {
            Ok(body) => {
                status.record_command_receipt(body.clone());
                session.record_command(&request);
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    with_session(body, session).as_bytes(),
                )
            }
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/transaction"
        || path == "/canvas/transaction"
        || path == "/panel/transaction"
    {
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
        let request = String::from_utf8_lossy(&body);
        let _transaction = session.lock_source_transaction();
        return match crate::Canvas::apply_transaction_json(Path::new(canvas_file), &request) {
            Ok(body) => {
                status.version.fetch_add(1, Ordering::SeqCst);
                let revision = current_revision_for_request(canvas_file, &request);
                session.accept_transaction(&request, &revision);
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    with_session(body, session).as_bytes(),
                )
            }
            Err(body) => {
                let revision = current_revision_for_request(canvas_file, &request);
                session.refuse_transaction(&request, &revision);
                write_response(
                    &mut stream,
                    "409 Conflict",
                    "application/json; charset=utf-8",
                    with_session(body, session).as_bytes(),
                )
            }
        };
    }
    if path == "/__jet_canvas/project/transaction"
        || path == "/canvas/project/transaction"
        || path == "/panel/project/transaction"
    {
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
        let request = String::from_utf8_lossy(&body);
        let _transaction = session.lock_source_transaction();
        return match crate::Canvas::apply_project_transaction_json(Path::new(canvas_file), &request)
        {
            Ok(body) => {
                status.version.fetch_add(1, Ordering::SeqCst);
                let revision = current_revision_for_request(canvas_file, &request);
                session.accept_transaction(&request, &revision);
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    with_session(body, session).as_bytes(),
                )
            }
            Err(body) => {
                let revision = current_revision_for_request(canvas_file, &request);
                session.refuse_transaction(&request, &revision);
                write_response(
                    &mut stream,
                    "409 Conflict",
                    "application/json; charset=utf-8",
                    with_session(body, session).as_bytes(),
                )
            }
        };
    }
    if path == "/__jet_canvas/query" || path == "/canvas/query" || path == "/panel/query" {
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
        let request = String::from_utf8_lossy(&body);
        return match crate::Canvas::query_json_for_entry(Path::new(canvas_file), &request) {
            Ok(body) => write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
            ),
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
            ),
        };
    }
    if path == "/__jet_canvas/debug" || path == "/canvas/debug" || path == "/panel/debug" {
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
        let request = String::from_utf8_lossy(&body);
        let mut paused_evaluate = match paused_evaluate.lock() {
            Ok(host) => host,
            Err(_) => {
                return write_response(
                    &mut stream,
                    "500 Internal Server Error",
                    "text/plain; charset=utf-8",
                    b"paused evaluate host unavailable",
                );
            }
        };
        return match crate::Canvas::debug_session_json_for_entry_with_sessions_and_host(
            Path::new(canvas_file),
            &request,
            debug_sessions,
            &mut paused_evaluate,
        ) {
            Ok(body) => {
                session.record_debug_response(&request, &body);
                write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    with_session(body, session).as_bytes(),
                )
            }
            Err(body) => write_response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                with_session(body, session).as_bytes(),
            ),
        };
    }
    if path == "/__jet_dev_version" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let body = status.version.load(Ordering::SeqCst).to_string();
        return write_response(
            &mut stream,
            "200 OK",
            "text/plain; charset=utf-8",
            body.as_bytes(),
        );
    }
    if path == "/__jet_dev_status" {
        if method != "GET" {
            return method_not_allowed(&mut stream);
        }
        let status_authorized = canvas_request_authorized(
            &request,
            target,
            bind_host,
            if listener_kind == ListenerKind::Canvas {
                session.canvas_port()
            } else {
                session.application_port()
            },
            session_secret,
        );
        if status_authorized {
            if let Some(client) = query_param(target, "client") {
                status.note_client(&client);
                session.note_client(&client);
            }
        }
        let body = status.json();
        return write_response(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            body.as_bytes(),
        );
    }
    if path == "/__jet_dev_disconnect" {
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
        if let Some(client) = query_param(target, "client") {
            status.drop_client(&client);
            session.drop_client(&client);
        }
        return write_response(&mut stream, "200 OK", "text/plain; charset=utf-8", b"ok");
    }
    if path == "/__jet_perf_browser" {
        if method != "POST" {
            return method_not_allowed(&mut stream);
        }
        let relay = status.browser_relay.lock().unwrap();
        let Some(relay) = relay.as_ref() else {
            return write_response(
                &mut stream,
                "503 Service Unavailable",
                "text/plain; charset=utf-8",
                b"browser trace relay unavailable",
            );
        };
        if query_param(target, "nonce").as_deref() != Some(relay.nonce()) {
            return write_response(
                &mut stream,
                "403 Forbidden",
                "text/plain; charset=utf-8",
                b"stale or foreign browser trace session",
            );
        }
        return match relay.record(&body) {
            Ok(()) => write_response(
                &mut stream,
                "204 No Content",
                "text/plain; charset=utf-8",
                b"",
            ),
            Err(crate::BrowserTrace::RecordError::Oversized) => write_response(
                &mut stream,
                "413 Payload Too Large",
                "text/plain; charset=utf-8",
                b"browser trace envelope exceeds 512 bytes",
            ),
            Err(crate::BrowserTrace::RecordError::Malformed) => write_response(
                &mut stream,
                "400 Bad Request",
                "text/plain; charset=utf-8",
                b"browser trace envelope is malformed",
            ),
            Err(crate::BrowserTrace::RecordError::Unavailable) => write_response(
                &mut stream,
                "503 Service Unavailable",
                "text/plain; charset=utf-8",
                b"browser trace relay unavailable",
            ),
        };
    }
    if listener_kind == ListenerKind::Canvas {
        return write_response(
            &mut stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"Canvas control host does not serve program assets",
        );
    }
    // Only page/asset GETs are worth a request-log line (D-FE-DEVSRV1=D's
    // verbose log shows `GET / 200 2ms`, not the 400ms `/__jet_dev_version`
    // poll noise) — those are handled above and already returned.
    let started = Instant::now();
    let nonce = status
        .browser_relay
        .lock()
        .unwrap()
        .as_ref()
        .map(|relay| relay.nonce().to_string())
        .unwrap_or_default();
    let code = serve_static_from_root(&mut stream, static_root, path, &nonce, session_secret)?;
    status.log_request(method, path, code, started.elapsed());
    Ok(())
}

fn inject_canvas_session(
    mut asset: crate::CanvasAsset,
    session_secret: &str,
) -> crate::CanvasAsset {
    if asset.content_type == "text/html; charset=utf-8" {
        if let Some(index) = asset.body.find("app.js?") {
            let insert_at = index + "app.js?".len();
            asset
                .body
                .insert_str(insert_at, &format!("session={session_secret}&"));
        }
    }
    asset
}

fn method_not_allowed(stream: &mut impl Write) -> std::io::Result<()> {
    write_response(
        stream,
        "405 Method Not Allowed",
        "text/plain; charset=utf-8",
        b"method not allowed",
    )
}

fn web_stream_path(path: &str) -> bool {
    path == "/__jet_web/stream"
}

/// The existing Canvas source endpoints the devtools page uses as its source
/// tools (inspect, checked preview, revision-bound apply).  They are served by
/// the same handlers as the Canvas listener; nothing here is a second editor.
fn source_tool_path(path: &str) -> bool {
    matches!(
        path,
        "/__jet_canvas/project"
            | "/__jet_canvas/source"
            | "/__jet_canvas/command"
            | "/__jet_canvas/transaction"
    )
}

fn devtools_path(path: &str) -> bool {
    matches!(
        path,
        "/__jet_devtools"
            | "/__jet_devtools/workbench"
            | "/__jet_devtools/commands"
            | "/__jet_devtools/assets"
            | "/__jet_devtools/state"
            | "/__jet_devtools/reconnect"
            | "/__jet_devtools/action"
            | "/__jet_devtools/selection"
            | "/__jet_devtools/cursor"
            | "/__jet_devtools/swap"
            | "/__jet_devtools/recording"
            | "/__jet_devtools/error"
            | "/__jet_devtools/paused/inspect"
            | "/__jet_devtools/paused/evaluate"
            | "/__jet_devtools/style"
            | "/__jet_canvas/devtools"
            | "/canvas/devtools"
            | "/panel/devtools"
    )
}

fn is_application_control_path(path: &str) -> bool {
    devtools_path(path)
        || web_stream_path(path)
        || source_tool_path(path)
        || matches!(
            path,
            "/__jet_dev_status"
                | "/__jet_dev_version"
                | "/__jet_dev_disconnect"
                | "/__jet_perf_browser"
        )
}

static WEB_ERROR_REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

/// A stable per-request identity for `/__jet_devtools/error` projections.
/// The counter is process-local and monotonic; it never reuses or derives
/// an id from request content.
fn next_web_error_request_id() -> String {
    format!(
        "werr-{}",
        WEB_ERROR_REQUEST_COUNTER.fetch_add(1, Ordering::SeqCst)
    )
}

/// Map one typed `/paused/*` host failure to its HTTP status line. The JSON
/// body itself always carries the authoritative `error` fact; this only
/// selects the transport-level status around it.
fn paused_evaluate_error_status(
    error: &crate::PausedEvaluate::PausedEvaluateHostError,
) -> &'static str {
    match error {
        crate::PausedEvaluate::PausedEvaluateHostError::SessionUnavailable
        | crate::PausedEvaluate::PausedEvaluateHostError::EvaluatorUnavailable => "409 Conflict",
        crate::PausedEvaluate::PausedEvaluateHostError::InvalidRequest(_) => "400 Bad Request",
        crate::PausedEvaluate::PausedEvaluateHostError::Rejected(_) => "403 Forbidden",
    }
}

fn session_response(session: &crate::ResidentDevSession) -> String {
    format!(
        "{{\"protocol\":\"jet.canvas.session\",\"schema_version\":1,\"session\":{}}}",
        session.json()
    )
}

fn with_session(body: String, session: &crate::ResidentDevSession) -> String {
    let marker = "\"canvas\":{";
    let Some(index) = body.find(marker) else {
        return body;
    };
    let insert_at = index + marker.len();
    let mut decorated = String::with_capacity(body.len() + session.json().len() + 16);
    decorated.push_str(&body[..insert_at]);
    decorated.push_str("\"session\":");
    decorated.push_str(&session.json());
    decorated.push(',');
    decorated.push_str(&body[insert_at..]);
    decorated
}

fn current_revision_for_request(canvas_file: &str, request: &str) -> String {
    let source_id = jet_foundation::JSON::parse_json(request)
        .ok()
        .and_then(|value| match value {
            DataTree::Object(object) => object.into_iter().find_map(|(name, value)| {
                (name == "source_id")
                    .then_some(value)
                    .and_then(|value| match value {
                        DataTree::Text(source_id) | DataTree::TypedText(source_id) => {
                            Some(source_id)
                        }
                        _ => None,
                    })
            }),
            _ => None,
        })
        .unwrap_or_default();
    current_revision_for_source_id(
        canvas_file,
        (!source_id.is_empty()).then_some(source_id.as_str()),
    )
}

fn source_id_from_payload(payload: &str) -> String {
    let Ok(value) = jet_foundation::JSON::parse_json(payload) else {
        return String::new();
    };

    fn find(value: &DataTree) -> Option<String> {
        match value {
            DataTree::Object(object) => {
                if let Some(source_id) = object.iter().find_map(|(name, value)| {
                    (name == "source_id")
                        .then_some(value)
                        .and_then(|value| match value {
                            DataTree::Text(source_id) | DataTree::TypedText(source_id) => {
                                Some(source_id.clone())
                            }
                            _ => None,
                        })
                }) {
                    return Some(source_id);
                }
                object.iter().find_map(|(_, value)| find(value))
            }
            DataTree::Array(values) => values.iter().find_map(find),
            _ => None,
        }
    }

    find(&value).unwrap_or_default()
}

fn current_revision_for_source_id(canvas_file: &str, source_id: Option<&str>) -> String {
    let source_path = source_id
        .and_then(|source_id| {
            crate::Canvas::project_path_for_source_id(Path::new(canvas_file), source_id)
        })
        .unwrap_or_else(|| PathBuf::from(canvas_file));
    crate::Canvas::read_source_without_symlinks(&source_path)
        .map(|source| crate::Canvas::source_revision(&source))
        .unwrap_or_default()
}

fn serve_static_from_root(
    stream: &mut impl Write,
    root: &Path,
    path: &str,
    nonce: &str,
    session_secret: &str,
) -> std::io::Result<u16> {
    // The generated output root is the complete application preview boundary.
    // Never fall back to the watched source directory: this listener has no
    // Canvas capability gate for ordinary application assets.
    let relative = match static_relative_path(path) {
        Ok(path) => path,
        Err(()) => {
            write_response(
                stream,
                "400 Bad Request",
                "text/plain; charset=utf-8",
                b"bad path",
            )?;
            return Ok(400);
        }
    };
    let content_type = content_type_for(&relative);
    let is_html = relative
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("html"));
    let mut reload_script = is_html.then(|| live_reload_script(nonce, session_secret));
    let raw_limit = match reload_script.as_mut() {
        Some(script) => {
            script.shrink_to_fit();
            if !html_script_fits_budget(script.capacity()) {
                write_response(
                    stream,
                    "500 Internal Server Error",
                    "text/plain; charset=utf-8",
                    b"live-reload script exceeds the response budget",
                )?;
                return Ok(500);
            }
            html_raw_response_limit(script.capacity())
        }
        None => MAX_STATIC_RESPONSE_BYTES,
    };
    let bytes = read_static_file_bounded(root, &relative, raw_limit);
    let bytes = match bytes {
        Ok(b) => b,
        Err(error) => {
            let (status, code, body) = match error.kind() {
                std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidData => {
                    ("404 Not Found", 404, format!("not found: {}", path))
                }
                _ => ("400 Bad Request", 400, "bad path".to_owned()),
            };
            write_response(stream, status, "text/plain; charset=utf-8", body.as_bytes())?;
            return Ok(code);
        }
    };

    if let Some(script) = reload_script {
        let html = match decode_html(bytes) {
            Ok(html) => html,
            Err(_) => {
                write_response(
                    stream,
                    "500 Internal Server Error",
                    "text/plain; charset=utf-8",
                    b"static HTML is not valid UTF-8",
                )?;
                return Ok(500);
            }
        };
        let Some(injected) = inject_live_reload_with_script(&html, &script) else {
            write_response(
                stream,
                "500 Internal Server Error",
                "text/plain; charset=utf-8",
                b"static HTML response is too large",
            )?;
            return Ok(500);
        };
        write_response(stream, "200 OK", content_type, injected.as_bytes())?;
        return Ok(200);
    }
    write_response(stream, "200 OK", content_type, &bytes)?;
    Ok(200)
}

fn read_source_without_symlinks(path: &Path) -> std::io::Result<String> {
    #[cfg(windows)]
    {
        let bytes = crate::read_file_without_symlinks_bounded(path, MAX_STATIC_RESPONSE_BYTES)?;
        return String::from_utf8(bytes).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "source is not UTF-8")
        });
    }
    #[cfg(not(windows))]
    {
        crate::Canvas::read_source_without_symlinks(path)
    }
}

fn read_web_manifest() -> Option<String> {
    let bytes = read_static_file_bounded(
        Path::new("build"),
        Path::new("web.manifest.json"),
        MAX_STATIC_RESPONSE_BYTES,
    )
    .ok()?;
    String::from_utf8(bytes).ok()
}

fn decode_html(bytes: Vec<u8>) -> std::io::Result<String> {
    String::from_utf8(bytes).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "static HTML is not valid UTF-8",
        )
    })
}

fn html_raw_response_limit(script_capacity: usize) -> u64 {
    let script_bytes = u64::try_from(script_capacity).unwrap_or(u64::MAX);
    MAX_STATIC_RESPONSE_BYTES.saturating_sub(script_bytes.saturating_mul(2)) / 2
}

fn html_script_fits_budget(script_capacity: usize) -> bool {
    let script_bytes = u64::try_from(script_capacity).unwrap_or(u64::MAX);
    script_bytes.saturating_mul(2) <= MAX_STATIC_RESPONSE_BYTES
}

/// Plain polling live-reload (no WebSocket/SSE — I6 + correct scoping for a
/// dev tool): fetch `/__jet_dev_status` on an interval, then request the
/// module-swap receipt and dispatch `jet:web-swap` without restarting the page.
///
/// D-FE-DEVSRV1=D (hybrid): the corner control mirrors the exact same
/// `message` string the terminal's parity line renders — same poll, same words,
/// cannot drift.  The closed state is a compact equal-sided squircle containing
/// only the canonical Jet mark; ready/building/error/reconnecting are carried
/// by distinct glowing ring patterns and a full accessible status name.  The
/// same container grows into lens, then workbench, around the real devtools
/// routes.  On `state:"error"` the lens opens by itself with the verbatim
/// diagnostic as its face while the page underneath stays visible as the
/// last-good page (I4 — `s.diagnostic` is written via `textContent`, never
/// re-escaped or reworded).  The face offers the real source tool and a
/// canonical ProjectRebuild command; a clean rebuild returns it to the pill.
fn live_reload_script(nonce: &str, session_secret: &str) -> String {
    let perf_script = if nonce.is_empty() {
        String::new()
    } else {
        format!(
            r#"  var jetPerfNonce = "{nonce}";
  self.__jetPerfNow = function () {{ return performance.now(); }};
  self.__jetPerfRecord = function (symbol, eventClass, startMs) {{
    if (!symbol || typeof performance === "undefined") return;
    var endMs = performance.now();
    var body = new URLSearchParams({{
      class: String(eventClass), symbol: String(symbol),
      start_ns: String(Math.max(0, Math.floor(startMs * 1000000))),
      duration_ns: String(Math.max(0, Math.floor((endMs - startMs) * 1000000))),
      clock_ns: String(Math.max(0, Math.floor(endMs * 1000000)))
    }});
    try {{ navigator.sendBeacon("/__jet_perf_browser?nonce=" + jetPerfNonce + "&session=" + encodeURIComponent(jetDevSession), body); }} catch (_) {{}}
  }};
"#
        )
    };
    format!(
        r##"<script>
(function () {{
  var jetDevSession = "{session_secret}";
  self.__jetDevSession = jetDevSession;
{perf_script}
  var jetDevVersion = null, reconnectAttempt = 0;
  var ui = null, lastStatus = null;
  var dismissedDiagnostic = null, errorFace = false, autoOpened = false;
  var jetDevClient = null;
  try {{ jetDevClient = sessionStorage.getItem("jet-dev-client"); }} catch (_) {{}}
  if (!jetDevClient) {{
    jetDevClient = (self.crypto && crypto.randomUUID) ? crypto.randomUUID() :
      String(Date.now()) + "-" + Math.random().toString(16).slice(2);
    try {{ sessionStorage.setItem("jet-dev-client", jetDevClient); }} catch (_) {{}}
  }}
  addEventListener("pagehide", function () {{
    try {{ navigator.sendBeacon("/__jet_dev_disconnect?client=" + encodeURIComponent(jetDevClient) + "&session=" + encodeURIComponent(jetDevSession)); }} catch (_) {{}}
  }});
  // One container, three depths.  The closed state is a square Jet mark with
  // no status bar: the ring pattern, accessible name, and live status carry
  // the state.  Lens and workbench are the same container grown in place.
  function devtoolsUrl(route) {{ return route + "?session=" + encodeURIComponent(jetDevSession); }}
  function ensureUi() {{
    if (ui) return;
    var style = document.createElement("style");
    style.textContent =
      "#jet-dev-lens{{position:fixed;left:16px;bottom:max(16px,env(safe-area-inset-bottom));z-index:2147483647;display:flex;flex-direction:column;width:76px;height:76px;aspect-ratio:1/1;max-width:calc(100vw - 32px);color:#F6F5F2;font:13px/1.4 -apple-system,BlinkMacSystemFont,'SF Pro Text','Segoe UI',system-ui,sans-serif;background:#101114;border:2px solid var(--ring);border-radius:26px;box-shadow:0 0 0 1px rgba(255,255,255,.05),0 0 24px var(--glow),0 12px 30px rgba(0,0,0,.44);overflow:hidden;box-sizing:border-box;--ring:#56C271;--glow:rgba(86,194,113,.34)}}" +
      "#jet-dev-lens::after{{content:'';position:absolute;inset:5px;border:1px solid transparent;border-radius:20px;pointer-events:none}}" +
      "#jet-dev-lens[data-state=building]{{--ring:#E3A43B;--glow:rgba(227,164,59,.36);border-style:dashed}}" +
      "#jet-dev-lens[data-state=building]::after{{border-color:rgba(227,164,59,.56);border-style:dashed}}" +
      "#jet-dev-lens[data-state=error],#jet-dev-lens[data-state=unavailable],#jet-dev-lens[data-state=stopped]{{--ring:#E8232A;--glow:rgba(232,35,42,.42);border-style:double}}" +
      "#jet-dev-lens[data-state=error]::after,#jet-dev-lens[data-state=unavailable]::after,#jet-dev-lens[data-state=stopped]::after{{border-color:rgba(232,35,42,.65)}}" +
      "#jet-dev-lens[data-state=reconnecting],#jet-dev-lens[data-state=starting]{{--ring:#9B9EA8;--glow:rgba(155,158,168,.25);border-style:dotted}}" +
      "#jet-dev-lens[data-state=reconnecting]::after,#jet-dev-lens[data-state=starting]::after{{border-color:rgba(155,158,168,.48);border-style:dotted}}" +
      "#jet-dev-lens[data-depth=lens]{{width:min(560px,calc(100vw - 32px));height:auto;aspect-ratio:auto;max-height:min(78vh,720px);border-radius:24px}}" +
      "#jet-dev-lens[data-depth=workbench]{{inset:16px;left:16px;right:16px;top:16px;bottom:16px;width:auto;height:auto;max-width:none;max-height:none;aspect-ratio:auto;border-radius:24px}}" +
      "#jet-dev-pill{{display:flex;align-items:stretch;flex:none;position:relative;z-index:1}}" +
      "#jet-dev-pill .head{{display:flex;align-items:center;justify-content:center;width:100%;height:72px;margin:0;padding:0;border:0;border-radius:22px;background:none;color:inherit;font:inherit;cursor:pointer}}" +
      "#jet-dev-lens:not([data-depth=pill]) #jet-dev-pill .head{{flex:1 1 auto;justify-content:flex-start;gap:11px;width:auto;height:52px;padding:0 16px;min-width:0}}" +
      "#jet-dev-pill .logo{{display:flex;flex:none;width:49px;height:28px;align-items:center;justify-content:center}}" +
      "#jet-dev-pill .logo svg{{display:block;width:49px;height:27px}}" +
      "#jet-dev-pill .word{{display:none;font-size:13px;font-weight:650;letter-spacing:.01em;line-height:1}}" +
      "#jet-dev-lens:not([data-depth=pill]) #jet-dev-pill .word{{display:inline}}" +
      "#jet-dev-pill .read{{display:none;font:11px/1 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;color:#9B9EA8}}" +
      "#jet-dev-lens:not([data-depth=pill]) #jet-dev-pill .read:not(:empty){{display:inline}}" +
      "#jet-dev-lens .sr{{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}}" +
      "#jet-dev-pill .tools{{display:none;align-items:center;gap:2px;padding-right:8px;flex:none;position:relative;z-index:1}}" +
      "#jet-dev-lens:not([data-depth=pill]) #jet-dev-pill .tools{{display:flex}}" +
      "#jet-dev-pill .tool{{margin:0;padding:6px 9px;border:1px solid transparent;border-radius:8px;background:none;color:#9B9EA8;font:inherit;font-size:12px;line-height:1.2;cursor:pointer;text-decoration:none;white-space:nowrap}}" +
      "#jet-dev-pill .tool:hover{{color:#F6F5F2;background:#1D2026}}" +
      "#jet-dev-lens[data-depth=lens] .tool[data-depth=lens],#jet-dev-lens[data-depth=workbench] .tool[data-depth=workbench]{{display:none}}" +
      "#jet-dev-lens :focus-visible{{outline:2px solid #E8232A;outline-offset:-2px}}" +
      "#jet-dev-body{{display:none;flex:1 1 auto;min-height:0;flex-direction:column;border-top:1px solid #30343C;background:#101114;position:relative;z-index:1}}" +
      "#jet-dev-lens:not([data-depth=pill]) #jet-dev-body{{display:flex}}" +
      "#jet-dev-overlay{{display:none;flex:1 1 auto;min-height:0;flex-direction:column;overflow:auto}}" +
      "#jet-dev-overlay h3{{margin:0;padding:17px 18px 8px;font-family:inherit;font-size:14px;font-weight:650;line-height:1.3;color:#F6F5F2}}" +
      "#jet-dev-overlay h3::before{{content:'';display:inline-block;width:9px;height:9px;border:2px solid #E8232A;border-radius:3px;margin-right:9px;vertical-align:0}}" +
      "#jet-dev-overlay pre{{flex:0 1 auto;margin:0 18px;padding:13px;overflow:auto;white-space:pre-wrap;overflow-wrap:anywhere;font:12px/1.55 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;color:#F6F5F2;background:#0A0B0D;border:1px solid #30343C;border-radius:10px}}" +
      "#jet-dev-overlay footer{{display:flex;flex-wrap:wrap;gap:8px;align-items:center;padding:13px 18px;color:#9B9EA8;font-size:11px}}" +
      "#jet-dev-overlay footer .parity{{flex:1 1 100%;overflow-wrap:anywhere}}" +
      "#jet-dev-overlay footer .act{{margin:0;padding:7px 12px;border:1px solid #30343C;border-radius:9px;background:#17191D;color:#F6F5F2;font:inherit;font-size:12px;cursor:pointer;text-decoration:none}}" +
      "#jet-dev-overlay footer .act.primary{{border-color:#E8232A}}#jet-dev-overlay footer .act:hover{{background:#1D2026}}#jet-dev-overlay footer .act[disabled]{{opacity:.5;cursor:default}}" +
      "#jet-dev-overlay footer .result{{flex:1 1 100%;min-height:1.4em;overflow-wrap:anywhere}}" +
      "#jet-dev-panels{{flex:1 1 auto;min-height:0;height:min(62vh,640px);width:100%;border:0;background:#101114}}" +
      "#jet-dev-panels[hidden]{{display:none}}" +
      "#jet-dev-shade{{position:fixed;inset:0;z-index:2147483645;display:none;pointer-events:none;background:rgba(16,17,20,.32)}}" +
      "@media(max-width:560px){{#jet-dev-lens[data-depth=lens]{{left:8px;right:8px;bottom:max(8px,env(safe-area-inset-bottom));width:auto;max-width:none;max-height:82vh;border-radius:22px}}#jet-dev-lens[data-depth=lens] #jet-dev-panels{{height:68vh}}#jet-dev-lens[data-depth=workbench]{{inset:0;max-width:none;border-radius:0}}#jet-dev-lens[data-depth=workbench] #jet-dev-panels{{height:auto}}}}" +
      "@media(prefers-reduced-motion:no-preference){{#jet-dev-lens{{transition:width .22s cubic-bezier(.2,.7,.2,1),height .22s cubic-bezier(.2,.7,.2,1),border-radius .22s,box-shadow .22s}}#jet-dev-body{{animation:jet-dev-in .18s ease}}@keyframes jet-dev-in{{from{{opacity:0}}to{{opacity:1}}}}#jet-dev-lens[data-state=building]::after{{animation:jet-dev-ring 1.2s ease-in-out infinite alternate}}@keyframes jet-dev-ring{{to{{opacity:.35}}}}}}" +
      "@media(prefers-reduced-motion:reduce){{#jet-dev-lens,#jet-dev-body{{transition:none!important;animation:none!important}}}}";
    document.head.appendChild(style);
    var root = document.createElement("div");
    root.id = "jet-dev-lens";
    root.setAttribute("data-depth", "pill");
    root.setAttribute("data-state", "ready");
    root.innerHTML =
      "<div id=\"jet-dev-pill\">" +
        "<button class=\"head\" type=\"button\" aria-expanded=\"false\" aria-controls=\"jet-dev-body\" title=\"Open Jet development status (Alt+J)\">" +
          "<span class=\"logo\" aria-hidden=\"true\"><svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 120 64\" focusable=\"false\"><path fill=\"#E8232A\" d=\"M112 32 L52 6 L66 32 L52 58 Z\"></path><rect fill=\"#E8232A\" x=\"26\" y=\"14\" width=\"18\" height=\"6\" rx=\"3\"></rect><rect fill=\"#E8232A\" x=\"4\" y=\"29\" width=\"36\" height=\"6\" rx=\"3\"></rect><rect fill=\"#E8232A\" x=\"16\" y=\"44\" width=\"24\" height=\"6\" rx=\"3\"></rect></svg></span>" +
          "<span class=\"word\" aria-hidden=\"true\"></span><span class=\"read\" aria-hidden=\"true\"></span>" +
        "</button>" +
        "<span class=\"tools\">" +
          "<button class=\"tool\" type=\"button\" data-depth=\"workbench\">Open workbench</button>" +
          "<button class=\"tool\" type=\"button\" data-depth=\"lens\">Lens</button>" +
          "<button class=\"tool\" type=\"button\" data-depth=\"pill\" aria-label=\"Close Jet development status (Escape)\" title=\"Close (Escape)\">&#215;</button>" +
        "</span>" +
      "</div>" +
      "<span class=\"sr\" id=\"jet-dev-status\" role=\"status\" aria-live=\"polite\"></span>" +
      "<div id=\"jet-dev-body\">" +
        "<div id=\"jet-dev-overlay\"><h3></h3><pre></pre><footer><span class=\"parity\"></span>" +
          "<button class=\"act primary\" type=\"button\" data-act=\"source\">Fix in source</button>" +
          "<button class=\"act\" type=\"button\" data-act=\"rebuild\">Rebuild</button>" +
          "<a class=\"act\" target=\"_blank\" rel=\"noopener\">Open error details</a><span class=\"result\" role=\"status\"></span></footer></div>" +
        "<iframe id=\"jet-dev-panels\" title=\"Jet devtools\" hidden></iframe>" +
      "</div>";
    document.body.appendChild(root);
    var shade = document.createElement("div");
    shade.id = "jet-dev-shade";
    shade.setAttribute("aria-hidden", "true");
    document.body.appendChild(shade);
    ui = {{
      root: root, shade: shade,
      head: root.querySelector(".head"), word: root.querySelector(".word"), read: root.querySelector(".read"),
      status: root.querySelector("#jet-dev-status"), overlay: root.querySelector("#jet-dev-overlay"), frame: root.querySelector("#jet-dev-panels")
    }};
    ui.overlayTitle = ui.overlay.querySelector("h3");
    ui.overlayBody = ui.overlay.querySelector("pre");
    ui.overlayFooter = ui.overlay.querySelector("footer .parity");
    ui.overlayResult = ui.overlay.querySelector("footer .result");
    ui.overlay.querySelector("footer a").href = devtoolsUrl("/__jet_devtools/error");
    ui.head.addEventListener("click", toggleLens);
    root.querySelectorAll(".tool[data-depth]").forEach(function (button) {{
      button.addEventListener("click", function () {{
        if (button.dataset.depth === "pill") back(true, true); else setDepth(button.dataset.depth, true);
      }});
    }});
    ui.overlay.querySelector("[data-act=source]").addEventListener("click", openSourceTool);
    ui.overlay.querySelector("[data-act=rebuild]").addEventListener("click", function (event) {{ rebuild(event.currentTarget); }});
    document.addEventListener("keydown", onKey);
    addEventListener("message", function (event) {{
      if (event.origin === location.origin && event.data && event.data.jet === "devtools") onKey(event.data);
    }});
  }}
  function onKey(event) {{
    if (event.key === "Escape") {{ back(true); return; }}
    if (event.altKey && !event.ctrlKey && !event.metaKey && (event.key === "j" || event.key === "J")) {{
      if (event.preventDefault) event.preventDefault();
      toggleLens();
    }}
  }}
  function depth() {{ return ui ? ui.root.getAttribute("data-depth") : "pill"; }}
  function toggleLens() {{
    if (depth() !== "pill") {{ back(true, true); return; }}
    dismissedDiagnostic = null;
    errorFace = !!lastStatus && lastStatus.state === "error";
    setDepth("lens", true);
  }}
  function back(byUser, toPill) {{
    var current = depth();
    if (current === "pill") return;
    if (current === "workbench" && !toPill) {{ setDepth("lens", byUser); return; }}
    if (errorFace) dismissedDiagnostic = ui.overlayBody.textContent;
    errorFace = false;
    autoOpened = false;
    setDepth("pill", byUser);
  }}
  function frameRoute(next, extra) {{
    var route = next === "workbench" ? "/__jet_devtools/workbench" : "/__jet_devtools";
    if (extra || ui.frame.getAttribute("data-route") !== route) {{
      ui.frame.setAttribute("data-route", route);
      ui.frame.src = devtoolsUrl(route) + (extra || "");
    }}
  }}
  function setDepth(next, byUser) {{
    ensureUi();
    var current = depth();
    if (next === current) return;
    ui.root.setAttribute("data-depth", next);
    ui.head.setAttribute("aria-expanded", next === "pill" ? "false" : "true");
    if (next === "pill") {{
      ui.frame.removeAttribute("data-route");
      ui.frame.src = "about:blank";
    }} else frameRoute(next);
    renderFaces();
    if (byUser) {{
      if (next === "pill") ui.head.focus();
      else ui.root.querySelector(".tool[data-depth=pill]").focus();
    }}
  }}
  function openSourceTool() {{
    dismissedDiagnostic = ui.overlayBody.textContent;
    errorFace = false;
    autoOpened = false;
    frameRoute(depth() === "workbench" ? "workbench" : "lens", "&tool=source");
    renderFaces();
    ui.frame.focus();
  }}
  function rebuild(button) {{
    button.disabled = true;
    ui.overlayResult.textContent = "Requesting rebuild…";
    fetch(devtoolsUrl("/__jet_devtools/state"), {{ cache: "no-store" }})
      .then(function (r) {{ if (!r.ok) throw new Error("devtools state unavailable (HTTP " + r.status + ")"); return r.json(); }})
      .then(function (st) {{
        if (st.tools && st.tools.rebuild && !st.tools.rebuild.available) throw new Error(st.tools.rebuild.reason || "rebuild is not available on this host");
        var id = "rebuild-" + Date.now().toString(36) + "-" + Math.random().toString(36).slice(2, 8);
        return fetch(devtoolsUrl("/__jet_devtools"), {{
          method: "POST", cache: "no-store", headers: {{ "content-type": "application/json" }},
          body: JSON.stringify({{ protocol: "jet.devtools.v1", session_id: st.session_id, started_at_ms: Date.now(), direction: "host_to_runtime",
            commands: [{{ kind: "ProjectRebuild", payload: {{ session_id: st.session_id, request_id: id, expected_revision: st.revision }} }}] }})
        }});
      }})
      .then(function (r) {{
        if (!r.ok) return r.text().then(function (t) {{ throw new Error(t || ("rebuild refused (HTTP " + r.status + ")")); }});
        ui.overlayResult.textContent = "Rebuild queued — this tile follows the build";
      }})
      .catch(function (e) {{ ui.overlayResult.textContent = String(e.message || e); }})
      .finally(function () {{ button.disabled = false; }});
  }}
  function renderFaces() {{
    var open = depth() !== "pill";
    ui.overlay.style.display = open && errorFace ? "flex" : "none";
    ui.frame.hidden = !(open && !errorFace);
  }}
  function renderStatus(s) {{
    ensureUi();
    lastStatus = s;
    var state = s.state || "ready";
    ui.root.setAttribute("data-state", state);
    // The hidden status is the non-color equivalent of the ring.  The
    // diagnostic itself remains byte-for-byte in the visible error face.
    ui.head.setAttribute("aria-label", "Jet development status: " + (s.message || state));
    ui.status.textContent = (s.message || state) + (s.diagnostic ? "\n" + s.diagnostic : "");
    ui.word.textContent = state;
    ui.read.textContent = state === "error" ? (s.code || "") :
      (state === "ready" && s.last_build_ms > 0 ? (s.last_build_ms / 1000).toFixed(1) + "s" : "");
    var stale = state === "building" || state === "reconnecting" || state === "error";
    ui.shade.style.display = stale ? "block" : "none";
    if (state === "error") {{
      var diagnostic = s.diagnostic || "";
      ui.overlayBody.textContent = diagnostic;
      ui.overlayTitle.textContent = "Build failed — " + (s.file || "Jet source");
      ui.overlayFooter.textContent = (s.message || state) + " — clears on the next clean build";
      if (diagnostic !== dismissedDiagnostic) {{
        if (depth() === "pill") {{ errorFace = true; autoOpened = true; setDepth("lens", false); }}
        else if (errorFace) {{ ui.overlayResult.textContent = ""; }}
        else dismissedDiagnostic = diagnostic;
      }}
    }} else {{
      errorFace = false;
      dismissedDiagnostic = null;
      if (autoOpened) {{ autoOpened = false; setDepth("pill", false); }}
    }}
    renderFaces();
  }}
  function notifyWebSwap() {{
    var snapshot = typeof globalThis.__jetWebSwapSnapshot === "function"
      ? globalThis.__jetWebSwapSnapshot() : null;
    var publish = snapshot
      ? fetch("/__jet_devtools/swap?session=" + encodeURIComponent(jetDevSession), {{
          method: "POST", cache: "no-store", headers: {{"content-type":"application/json"}}, body: JSON.stringify(snapshot)
        }})
      : Promise.resolve(null);
    publish
      .then(function (r) {{ if (r && !r.ok) throw new Error("web DOM snapshot rejected"); }})
      .then(function () {{ return fetch("/__jet_devtools/swap?session=" + encodeURIComponent(jetDevSession), {{ cache: "no-store" }}); }})
      .then(function (r) {{ if (!r.ok) throw new Error("web swap rejected"); return r.json(); }})
      .then(function (swap) {{
        dispatchEvent(new CustomEvent("jet:web-swap", {{ detail: swap }}));
        document.dispatchEvent(new CustomEvent("jet:web-swap", {{ detail: swap }}));
      }})
      .catch(function () {{}});
  }}
  function poll() {{
    fetch("/__jet_dev_status?client=" + encodeURIComponent(jetDevClient) + "&session=" + encodeURIComponent(jetDevSession), {{ cache: "no-store" }})
      .then(function (r) {{ return r.json(); }})
      .then(function (s) {{
        var recoveredConnection = reconnectAttempt > 0;
        reconnectAttempt = 0;
        renderStatus(s);
        var v = String(s.version || "");
        if (jetDevVersion === null) {{
          jetDevVersion = v;
          if (recoveredConnection) notifyWebSwap();
          return;
        }}
        if (recoveredConnection || v !== jetDevVersion) {{ jetDevVersion = v; notifyWebSwap(); }}
      }})
      .catch(function () {{
        reconnectAttempt += 1;
        renderStatus({{ state: "reconnecting", message: "reconnecting · waiting for connection" }});
      }})
      .finally(function () {{ setTimeout(poll, {poll_ms}); }});
  }}
  setTimeout(poll, 0);
}})();
</script>
"##,
        poll_ms = LIVE_RELOAD_POLL_MS,
        perf_script = perf_script,
        session_secret = json_escape(session_secret)
    )
}

#[cfg(test)]
fn inject_live_reload(html: &str, nonce: &str) -> String {
    let mut script = live_reload_script(nonce, "");
    script.shrink_to_fit();
    inject_live_reload_with_script(html, &script).unwrap_or_else(|| {
        let mut out = html.to_string();
        out.push_str(&script);
        out
    })
}

fn inject_live_reload_with_script(html: &str, script: &str) -> Option<String> {
    let output_len = html.len().checked_add(script.len())?;
    // Find the insertion point case-insensitively without allocating a
    // lowercase copy. HTML tags are ASCII, so the original byte offset stays
    // valid for the splice.
    let insertion = html.as_bytes().windows(b"</body>".len()).position(|tag| {
        tag.iter()
            .zip(b"</body>")
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    });
    let mut out = String::with_capacity(output_len);
    if let Some(index) = insertion {
        out.push_str(&html[..index]);
        out.push_str(script);
        out.push_str(&html[index..]);
    } else {
        out.push_str(html);
        out.push_str(script);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{
        bind_application_server, canvas_api_path, canvas_request_authorized, constant_time_equal,
        decode_html, devtools_path, format_build_time, format_line_colored, format_line_plain,
        frame_lines, handle_connection, handle_connection_with_root,
        handle_connection_with_root_and_policy, header_words, host_header_allowed,
        html_raw_response_limit, inject_canvas_session, inject_live_reload, mint_session_secret,
        origin_allowed, serve_forever, stage_and_swap, try_acquire_connection, CanvasHostOptions,
        DeadlineStream, DevStatus, ListenerKind, Ordering, ReleaseDevtoolsPolicy, WebHost,
        APPLICATION_PORT_RANGE, MAX_CONNECTION_THREADS, MAX_STATIC_RESPONSE_BYTES,
    };

    use crate::Request;
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    fn bind_canvas_for_test(file: &str, verbose: bool, port: Option<u16>) -> WebHost {
        let mut options = CanvasHostOptions::default();
        options.port = port;
        WebHost::bind_canvas_with_options_and_policy(
            file,
            verbose,
            &options,
            ReleaseDevtoolsPolicy::development(),
        )
        .unwrap()
    }

    fn bind_canvas_options_for_test(
        file: &str,
        verbose: bool,
        options: &CanvasHostOptions,
    ) -> Result<WebHost, String> {
        WebHost::bind_canvas_with_options_and_policy(
            file,
            verbose,
            options,
            ReleaseDevtoolsPolicy::development(),
        )
    }

    fn bind_web_canvas_options_for_test(
        file: &str,
        verbose: bool,
        fallback_port: Option<u16>,
        options: &CanvasHostOptions,
    ) -> Result<WebHost, String> {
        WebHost::bind_web_with_canvas_options_and_policy(
            file,
            verbose,
            fallback_port,
            options,
            ReleaseDevtoolsPolicy::development(),
        )
    }

    #[test]
    fn slowloris_request_hits_the_absolute_deadline_without_exhausting_admission() {
        let active = std::sync::atomic::AtomicUsize::new(MAX_CONNECTION_THREADS);
        assert!(
            !try_acquire_connection(&active),
            "the connection cap must reject the next slow client"
        );

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            let _ = stream.write_all(b"G");
            thread::sleep(Duration::from_millis(100));
        });
        let (stream, _) = listener.accept().unwrap();
        let started = Instant::now();
        let mut reader = std::io::BufReader::new(DeadlineStream::new(
            stream,
            started + Duration::from_millis(40),
        ));
        let error = Request::read(&mut reader).expect_err("slow request must time out");
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_millis(200));
        client.join().unwrap();
    }

    #[test]
    fn live_server_admission_drops_connections_after_cap() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let server_shutdown = Arc::clone(&shutdown);
        let status = Arc::new(DevStatus::new("admission.jet", false));
        let debug_sessions = Arc::new(crate::Canvas::DebugSessions::default());
        let session = Arc::new(crate::ResidentDevSession::new(
            "admission.jet",
            0,
            address.port(),
        ));
        let browser_state = Arc::new(Mutex::new(None));
        let paused_evaluate =
            Arc::new(Mutex::new(crate::PausedEvaluate::PausedEvaluateHost::new()));
        let terminal_style = Arc::new(crate::TerminalStyleReload::TerminalStyleHostAdapter::new());
        let server = thread::spawn(move || {
            serve_forever(
                listener,
                status,
                debug_sessions,
                session,
                "admission.jet".to_string(),
                ListenerKind::Application,
                "127.0.0.1".to_string(),
                "test-session".to_string(),
                server_shutdown,
                std::path::PathBuf::from("."),
                ReleaseDevtoolsPolicy::development(),
                browser_state,
                paused_evaluate,
                terminal_style,
            );
        });

        let mut clients = Vec::with_capacity(MAX_CONNECTION_THREADS);
        for _ in 0..MAX_CONNECTION_THREADS {
            let mut client = TcpStream::connect(address).unwrap();
            client.write_all(b"G").unwrap();
            clients.push(client);
        }

        let mut rejected = false;
        for _ in 0..100 {
            let mut candidate = TcpStream::connect(address).unwrap();
            candidate
                .set_read_timeout(Some(Duration::from_millis(20)))
                .unwrap();
            let _ = candidate.write_all(b"G");
            let mut byte = [0u8; 1];
            match candidate.read(&mut byte) {
                Ok(0) => {
                    rejected = true;
                    break;
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) => {}
                Err(_) => {
                    rejected = true;
                    break;
                }
                Ok(_) => {}
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert!(rejected, "live server must enforce the connection cap");
        drop(clients);
        shutdown.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn stage_and_swap_rejects_symlinked_destination() {
        use std::os::unix::fs::symlink;

        let root = std::env::current_dir()
            .unwrap()
            .join(format!(".jet-webhost-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let output = root.join("build");
        let staging = output.join(".staging");
        let outside = root.join("outside.js");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(&staging.join("web.manifest.json"), "staged").unwrap();
        std::fs::write(&outside, "must survive").unwrap();
        symlink(&outside, output.join("web.manifest.json")).unwrap();

        assert!(
            stage_and_swap(&staging, &output).is_err(),
            "web finalization must not replace a symlinked output"
        );
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "must survive");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn stage_and_swap_rejects_hardlinked_destination() {
        let root = std::env::current_dir()
            .unwrap()
            .join(format!(".jet-webhost-hardlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let output = root.join("build");
        let staging = output.join(".staging");
        let outside = root.join("outside.js");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(&staging.join("web.manifest.json"), "staged").unwrap();
        std::fs::write(&outside, "must survive").unwrap();
        std::fs::hard_link(&outside, output.join("web.manifest.json")).unwrap();

        assert!(
            stage_and_swap(&staging, &output).is_err(),
            "web finalization must reject hard-linked output members"
        );
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "must survive");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn stage_and_swap_rejects_symlinked_staging_ancestor() {
        use std::os::unix::fs::symlink;

        let root = std::env::current_dir()
            .unwrap()
            .join(format!(".jet-webhost-staging-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let output = root.join("build");
        let staging_target = root.join("staging-target");
        let staging = output.join(".staging");
        let outside = root.join("outside.js");
        std::fs::create_dir_all(&output).unwrap();
        std::fs::create_dir_all(&staging_target).unwrap();
        std::fs::write(staging_target.join("web.manifest.json"), "must not publish").unwrap();
        std::fs::write(&outside, "must survive").unwrap();
        symlink(&staging_target, &staging).unwrap();

        assert!(
            stage_and_swap(&staging, &output).is_err(),
            "web finalization must reject a symlinked staging directory"
        );
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "must survive");
        assert_eq!(
            std::fs::read_to_string(staging_target.join("web.manifest.json")).unwrap(),
            "must not publish"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn stage_and_swap_rejects_symlinked_output_root() {
        use std::os::unix::fs::symlink;

        let root = std::env::current_dir()
            .unwrap()
            .join(format!(".jet-webhost-output-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let real_output = root.join("real-build");
        let output = root.join("build");
        let staging = real_output.join(".staging");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("web.manifest.json"), "must not publish").unwrap();
        symlink(&real_output, &output).unwrap();

        assert!(
            stage_and_swap(&staging, &output).is_err(),
            "web finalization must reject a symlinked output root"
        );
        assert!(
            !real_output.join("web.manifest.json").exists(),
            "rejected output-root link must not receive bytes"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn canvas_source_route_rejects_symlinked_entry() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "jet-devserver-canvas-source-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let real = root.join("real.jet");
        let alias = root.join("alias.jet");
        std::fs::write(&real, "fn run() { print(\"secret\") }\n").unwrap();
        symlink(&real, &alias).unwrap();

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let secret = "canvas-source-secret";
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let request = format!(
            "GET /canvas/source?session={secret} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        );
        client.write_all(request.as_bytes()).unwrap();
        let (server, _) = listener.accept().unwrap();
        let status = DevStatus::new_with_terminal("alias.jet", false, false, false);
        status.set_port(port);
        let session = crate::ResidentDevSession::new("alias.jet", port, 0);
        let debug_sessions = crate::Canvas::DebugSessions::default();
        handle_connection(
            server,
            &status,
            &debug_sessions,
            &session,
            alias.to_str().unwrap(),
            ListenerKind::Canvas,
            "127.0.0.1",
            secret,
        )
        .unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 404 Not Found"), "{response}");
        assert!(
            !response.contains("secret"),
            "symlinked source was disclosed"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn application_preview_does_not_serve_files_from_source_directory() {
        let root = std::env::temp_dir().join(format!(
            "jet-devserver-source-disclosure-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let build = root.join("build");
        let source_dir = root.join("source");
        let source = source_dir.join("app.jet");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(&source, "fn main() {}\n").unwrap();
        std::fs::write(source_dir.join("private.txt"), "must not be served").unwrap();

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client
            .write_all(
                format!(
                    "GET /private.txt HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .unwrap();
        let (server, _) = listener.accept().unwrap();
        let status = DevStatus::new_with_terminal("app.jet", false, false, false);
        status.set_port(port);
        let debug_sessions = crate::Canvas::DebugSessions::default();
        let session = crate::ResidentDevSession::new("app.jet", port, port);
        handle_connection_with_root(
            server,
            &status,
            &debug_sessions,
            &session,
            source.to_str().unwrap(),
            ListenerKind::Application,
            "127.0.0.1",
            "",
            &build,
        )
        .unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 404 Not Found"), "{response}");
        assert!(!response.contains("must not be served"), "{response}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_utf8_html_fails_closed_without_lossy_replacement() {
        let root = std::env::temp_dir().join(format!(
            "jet-devserver-invalid-html-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.html"), [b'<', 0xff, b'>']).unwrap();

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client
            .write_all(
                format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .unwrap();
        let (server, _) = listener.accept().unwrap();
        let status = DevStatus::new_with_terminal("app.jet", false, false, false);
        status.set_port(port);
        let debug_sessions = crate::Canvas::DebugSessions::default();
        let session = crate::ResidentDevSession::new("app.jet", port, port);
        handle_connection_with_root(
            server,
            &status,
            &debug_sessions,
            &session,
            "app.jet",
            ListenerKind::Application,
            "127.0.0.1",
            "",
            &root,
        )
        .unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(
            response.starts_with("HTTP/1.1 500 Internal Server Error"),
            "{response}"
        );
        assert!(
            !response.contains('\u{fffd}'),
            "invalid HTML was lossy-decoded"
        );
        assert!(decode_html(vec![0xff]).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn transformed_html_uses_one_aggregate_response_budget() {
        let script_capacity = 4096;
        let raw_limit = html_raw_response_limit(script_capacity);
        assert!(
            2 * raw_limit + 2 * script_capacity as u64 <= MAX_STATIC_RESPONSE_BYTES,
            "raw HTML plus transformed HTML exceeds the shared response budget"
        );
        assert!(
            2 * (raw_limit + 1) + 2 * script_capacity as u64 > MAX_STATIC_RESPONSE_BYTES,
            "raw HTML limit leaves an unbounded transformed response"
        );
    }

    #[test]
    fn oversized_html_is_rejected_before_transformation() {
        let root = std::env::temp_dir().join(format!(
            "jet-devserver-oversized-html-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let file = std::fs::File::create(root.join("index.html")).unwrap();
        file.set_len(MAX_STATIC_RESPONSE_BYTES).unwrap();
        drop(file);

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client
            .write_all(
                format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .unwrap();
        let (server, _) = listener.accept().unwrap();
        let status = DevStatus::new_with_terminal("app.jet", false, false, false);
        status.set_port(port);
        let debug_sessions = crate::Canvas::DebugSessions::default();
        let session = crate::ResidentDevSession::new("app.jet", port, port);
        handle_connection_with_root(
            server,
            &status,
            &debug_sessions,
            &session,
            "app.jet",
            ListenerKind::Application,
            "127.0.0.1",
            "",
            &root,
        )
        .unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 404 Not Found"), "{response}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn injects_before_closing_body_tag() {
        let html = "<html><body><p>hi</p></BODY></html>";
        let out = inject_live_reload(html, "nonce");
        assert!(out.contains("<script>"));
        assert!(out.find("<script>").unwrap() < out.find("</BODY>").unwrap());
    }

    #[test]
    fn appends_when_no_body_tag() {
        let html = "<html><p>hi</p></html>";
        let out = inject_live_reload(html, "nonce");
        assert!(out.starts_with(html));
        assert!(out.contains("<script>"));
    }

    #[test]
    fn live_verbose_toggle_changes_depth_without_changing_status() {
        let status = DevStatus::new("app.jet", false);
        assert!(!status.verbose());
        let before = status.json();
        status.toggle_verbose();
        assert!(status.verbose());
        assert_eq!(status.json(), before);
        status.toggle_verbose();
        assert!(!status.verbose());
        assert_eq!(status.json(), before);
    }

    #[test]
    fn dashboard_detail_keeps_watched_target_and_canvas_route_pinned() {
        let status = DevStatus::new("app.jet", false);
        status.set_port(8123);
        status.set_canvas_port(8123);
        assert_eq!(
            status.dashboard_detail_line(),
            "         watching app.jet · Canvas http://localhost:8123/canvas · v verbose"
        );
    }

    #[test]
    fn browser_client_registry_counts_tabs_not_poll_connections() {
        let status = DevStatus::new("app.jet", false);
        status.note_client("tab-a");
        status.note_client("tab-a");
        assert_eq!(status.client_count(), 1);
        status.note_client("tab-b");
        assert_eq!(status.client_count(), 2);
        status.note_client("");
        status.note_client(&"x".repeat(129));
        assert_eq!(status.client_count(), 2);
    }

    #[test]
    fn expired_browser_lease_drives_shared_reconnect_then_ready() {
        let status = DevStatus::new("app.jet", false);
        status.set_port(8123);
        status.note_client("tab-a");
        status.clients.lock().unwrap().insert(
            "tab-a".to_string(),
            std::time::Instant::now() - std::time::Duration::from_millis(super::CLIENT_TTL_MS + 1),
        );
        status.expire_clients();
        let reconnecting = status.json();
        assert!(reconnecting.contains("\"state\":\"reconnecting\""));
        assert!(reconnecting.contains("reconnecting · waiting for connection"));

        status.note_client("tab-a");
        let ready = status.json();
        assert!(ready.contains("\"state\":\"ready\""));
        assert!(ready.contains("ready · localhost:8123 · 1 client"));
    }

    #[test]
    fn reconnect_overrides_ready_building_and_error_without_losing_snapshot() {
        let status = DevStatus::new("app.jet", false);
        status.set_port(8123);
        status.mark_ready(425, false);
        status.reconnecting.store(true, Ordering::SeqCst);

        let ready = status.json();
        assert!(ready.contains("\"state\":\"reconnecting\""));
        assert!(ready.contains("\"last_build_ms\":425"));

        status.mark_building();
        let building = status.json();
        assert!(building.contains("\"state\":\"reconnecting\""));
        assert!(building.contains("\"last_build_ms\":425"));

        status.mark_error(
            "E0102".to_string(),
            "Error [E0102]: missing".to_string(),
            true,
        );
        let error = status.json();
        assert!(error.contains("\"state\":\"reconnecting\""));
        assert!(error.contains("\"code\":\"E0102\""));
        assert!(error.contains("Error [E0102]: missing"));
        assert!(error.contains("\"last_build_ms\":425"));

        status.reconnecting.store(false, Ordering::SeqCst);
        let recovered = status.json();
        assert!(recovered.contains("\"state\":\"error\""));
        assert!(recovered.contains("\"code\":\"E0102\""));
    }

    #[test]
    fn blocked_verbose_refresh_cannot_reinstall_region_after_disable() {
        let status =
            std::sync::Arc::new(DevStatus::new_with_terminal("app.jet", true, true, false));
        status.controls_ready.store(true, Ordering::SeqCst);
        let terminal_guard = status.term_lock.lock().unwrap();
        let waiter = std::sync::Arc::clone(&status);
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            waiter.write_header_verbose("jet dev  [ready] localhost:8123 · 1 client");
        });
        started_rx.recv().unwrap();

        // This is the disable+restore critical section: capability flips
        // while the stale renderer is blocked on the same terminal lock.
        status.controls_ready.store(false, Ordering::SeqCst);
        *status.header_started.lock().unwrap() = false;
        drop(terminal_guard);
        handle.join().unwrap();

        assert!(!status.controls_ready.load(Ordering::SeqCst));
        assert!(!*status.header_started.lock().unwrap());
    }

    // --- D-FE-DEVSRV1=D: parity words shared verbatim by terminal + browser ---

    #[test]
    fn header_words_ready_includes_port_clients_and_build_time() {
        let (word, rest) = header_words("ready", "app.jet", "", 8080, 2, 420);
        assert_eq!(word, "ready");
        assert_eq!(rest, "localhost:8080 · 2 clients · built 0.4s");
    }

    #[test]
    fn header_words_ready_omits_build_time_before_first_build() {
        let (word, rest) = header_words("ready", "app.jet", "", 8080, 0, 0);
        assert_eq!(word, "ready");
        assert_eq!(rest, "localhost:8080 · 0 clients");
    }

    #[test]
    fn header_words_building_shows_watched_file_not_port() {
        let (word, rest) = header_words("building", "app.jet", "", 8080, 1, 0);
        assert_eq!(word, "building");
        assert_eq!(rest, "app.jet · 1 client");
    }

    #[test]
    fn header_words_error_shows_diagnostic_code_not_port() {
        let (word, rest) = header_words("error", "app.jet", "E0102", 8080, 2, 0);
        assert_eq!(word, "error");
        assert_eq!(rest, "E0102 · 2 clients");
    }

    #[test]
    fn header_words_singular_client_count() {
        let (_, rest) = header_words("ready", "app.jet", "", 8080, 1, 0);
        assert!(rest.ends_with("1 client"), "{rest}");
        assert!(!rest.contains("1 clients"), "{rest}");
    }

    #[test]
    fn format_build_time_renders_seconds_with_one_decimal() {
        assert_eq!(format_build_time(420), "0.4s");
        assert_eq!(format_build_time(1500), "1.5s");
        assert_eq!(format_build_time(0), "0.0s");
    }

    #[test]
    fn format_line_colored_carries_a_dot_and_the_full_parity_words() {
        let line = format_line_colored("ready", "localhost:8080 · 2 clients · built 0.4s");
        assert!(line.starts_with("jet dev  "));
        assert!(line.contains('\u{25CF}'), "{line}");
        assert!(line.contains("ready · localhost:8080 · 2 clients · built 0.4s"));
    }

    #[test]
    fn format_line_plain_uses_bracketed_state_word_no_color() {
        let line = format_line_plain("error", "E0102 · 2 clients");
        assert_eq!(line, "jet dev  [error] E0102 · 2 clients");
        assert!(
            !line.contains('\x1b'),
            "NO_COLOR/CI floor must carry no ANSI: {line}"
        );
    }

    #[test]
    fn frame_lines_keeps_diagnostic_words_verbatim() {
        let diagnostic = "Error [E0102]: nothing named `nonexistent_function_xyz` exists here\n  8 | nonexistent_function_xyz()\n    | ^^^^^^^^^^^^^^^^^^^^^^^^\nWhy: only defined names can be called\nFix: define it first";
        let framed = frame_lines("E0102", diagnostic);
        // Every diagnostic line survives byte-for-byte inside the frame —
        // only a "│ " border is added, never a reworded/retruncated line (I4).
        for line in diagnostic.lines() {
            assert!(
                framed.iter().any(|f| f == &format!("│ {}", line)),
                "missing verbatim line {:?} in {:#?}",
                line,
                framed
            );
        }
        assert!(framed.first().unwrap().starts_with("┌ E0102 "));
        assert!(framed.last().unwrap().starts_with('└'));
    }

    #[test]
    fn frame_lines_top_border_names_the_diagnostic_code() {
        let framed = frame_lines("E0204", "one line");
        assert!(framed[0].contains("E0204"));
    }

    #[test]
    fn canvas_default_is_loopback_ephemeral_and_session_bearing() {
        let options = CanvasHostOptions::default();
        assert_eq!(options.host, "127.0.0.1");
        assert_eq!(options.port, None);
        assert_eq!(options.transport, "http");
        assert_eq!(options.authority, "loopback");
        let host = bind_canvas_for_test("app.jet", false, None);
        let url = host.canvas_url();
        assert!(url.starts_with("http://127.0.0.1:"), "{url}");
        assert!(url.contains("/canvas?session="), "{url}");
        assert!(
            url.len() > 80,
            "session secret should not be a short marker: {url}"
        );
        let address = host
            .listener
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .local_addr()
            .unwrap();
        assert!(
            address.ip().is_loopback(),
            "default Canvas address: {address}"
        );
        assert!(
            !address.ip().is_unspecified(),
            "default Canvas address: {address}"
        );
    }

    #[test]
    fn application_default_scans_stable_range_and_skips_held_port() {
        let first = bind_application_server(None).expect("first application preview port");
        let first_port = first.local_addr().unwrap().port();
        let second = bind_application_server(None).expect("next application preview port");
        let second_port = second.local_addr().unwrap().port();

        assert!(APPLICATION_PORT_RANGE.contains(&first_port));
        assert!(APPLICATION_PORT_RANGE.contains(&second_port));
        assert!(
            second_port > first_port,
            "application port scan did not skip the held port: {first_port}, {second_port}"
        );
    }

    #[test]
    fn canvas_sessions_get_distinct_ephemeral_ports_and_secrets() {
        let first = bind_canvas_for_test("app.jet", false, None);
        let second = bind_canvas_for_test("app.jet", false, None);

        assert_ne!(first.status.port(), second.status.port());
        assert_ne!(first.session_secret, second.session_secret);
    }

    #[test]
    fn canvas_explicit_collision_and_invalid_overrides_fail_loudly() {
        let first = bind_canvas_for_test("app.jet", false, None);
        let port = first.status.port();
        let mut options = CanvasHostOptions::default();
        options.port = Some(port);
        let collision = bind_canvas_options_for_test("app.jet", false, &options)
            .err()
            .expect("a held explicit port must collide");
        assert!(
            collision.contains("couldn't bind Canvas host"),
            "{collision}"
        );

        options.transport = "udp".to_string();
        let transport = bind_canvas_options_for_test("app.jet", false, &options)
            .err()
            .expect("unsupported transport must fail");
        assert!(transport.contains("transport"), "{transport}");

        options.transport = "http".to_string();
        options.authority = "public".to_string();
        let authority = bind_canvas_options_for_test("app.jet", false, &options)
            .err()
            .expect("unknown authority must fail");
        assert!(authority.contains("authority"), "{authority}");
    }

    #[test]
    fn canvas_non_loopback_requires_explicit_remote_authority() {
        let mut options = CanvasHostOptions::default();
        options.host = "0.0.0.0".to_string();
        let error = bind_canvas_options_for_test("app.jet", false, &options)
            .err()
            .expect("non-loopback Canvas must require explicit authority");
        assert!(error.contains("authority = remote"), "{error}");

        options.authority = "remote".to_string();
        let host = bind_canvas_options_for_test("app.jet", false, &options)
            .expect("remote Canvas must bind only after explicit authority");
        let address = host
            .listener
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .local_addr()
            .unwrap();
        assert!(
            address.ip().is_unspecified(),
            "remote Canvas address: {address}"
        );
    }

    #[test]
    fn started_canvas_session_releases_its_port_on_shutdown() {
        let host = bind_canvas_for_test("app.jet", false, None);
        let port = host.status.port();
        host.start_canvas();
        drop(host);

        bind_canvas_for_test("app.jet", false, Some(port));
    }

    #[test]
    fn concurrent_canvas_clients_share_checked_revision_boundary() {
        let root = std::env::temp_dir().join(format!(
            "jet-devserver-concurrent-canvas-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("main.jet");
        std::fs::write(&path, "fn run() {\n    total := 1\n    print(total)\n}\n").unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        let revision = crate::Canvas::source_revision(&before);
        let host = bind_canvas_for_test(path.to_str().unwrap(), false, None);
        let port = host.status.port();
        let secret = host.session_secret.clone();
        host.start_canvas();

        let barrier = Arc::new(Barrier::new(3));
        let responses = [
            ("client-a", "first"),
            ("client-b", "second"),
        ]
        .into_iter()
        .map(|(client, name)| {
            let barrier = Arc::clone(&barrier);
            let secret = secret.clone();
            let body = format!(
                "{{\"schema_version\":1,\"op\":\"rename_binding\",\"revision\":\"{revision}\",\"from\":\"total\",\"to\":\"{name}\",\"client_id\":\"{client}\"}}"
            );
            thread::spawn(move || {
                barrier.wait();
                post_canvas_transaction(port, &secret, client, &body)
            })
        })
        .collect::<Vec<_>>();
        barrier.wait();
        let responses = responses
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(
            responses
                .iter()
                .filter(|response| response.starts_with("HTTP/1.1 200 OK"))
                .count(),
            1,
            "exactly one client must publish the shared revision"
        );
        let loser = responses
            .iter()
            .find(|response| response.starts_with("HTTP/1.1 409 Conflict"))
            .expect("one client must receive a conflict");
        let current = std::fs::read_to_string(&path).unwrap();
        let current_revision = crate::Canvas::source_revision(&current);
        assert!(loser.contains("\"kind\":\"conflict\""), "{loser}");
        assert!(
            loser.contains(&format!("\"current_revision\":\"{current_revision}\"")),
            "{loser}"
        );
        let session = host.session.json();
        assert!(
            session.contains(&format!("\"accepted_revision\":\"{current_revision}\"")),
            "{session}"
        );
        assert!(session.contains("\"status\":\"accepted\""));
        assert!(session.contains("\"status\":\"refused\""));
        drop(host);
        let _ = std::fs::remove_dir_all(root);
    }

    fn post_canvas_transaction(port: u16, secret: &str, client: &str, body: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let request = format!(
            "POST /canvas/transaction?client_id={client} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nAuthorization: Bearer {secret}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        String::from_utf8(response).unwrap()
    }

    #[test]
    fn web_canvas_override_keeps_program_listener_separate() {
        let mut options = CanvasHostOptions::default();
        options.output = Some("service@local#debug".to_string());
        options.target = Some("board.browser".to_string());
        options.audit = true;
        let host = bind_web_canvas_options_for_test("app.jet", false, None, &options)
            .expect("web Canvas host should bind independently");
        assert!(host.canvas_url().contains("/canvas?session="));
        assert!(host.status.verbose.load(Ordering::Relaxed));
        let session = host.session.json();
        assert!(session
            .contains("\"run\":{\"output\":\"service@local#debug\",\"target\":\"board.browser\"}"));
        assert!(session.contains("\"canvas\":{\"host\":\"127.0.0.1\""));
        assert!(!session.contains("\"application\":{\"host\":\"127.0.0.1\",\"port\":0"));
        assert_ne!(host.session.canvas_port(), host.session.application_port());
        assert!(session.contains("\"listener\":\"canvas\""));
        assert!(session.contains("\"listener\":\"application\""));
    }

    #[test]
    fn app_only_bind_has_no_canvas_listener_or_session_alias() {
        let host = WebHost::bind_with_policy(
            "app.jet",
            false,
            Some(0),
            ReleaseDevtoolsPolicy::development(),
        )
        .unwrap();
        assert!(host.listener.lock().unwrap().is_none());
        assert!(host.application_listener.lock().unwrap().is_some());
        assert_eq!(host.session.canvas_port(), 0);
        assert!(!host.status.canvas_enabled.load(Ordering::Relaxed));
        let session = host.session.json();
        assert!(session.contains("\"listeners\":{\"application\""));
        assert!(!session.contains("\"listeners\":{\"canvas\""));
        assert!(!session.contains("\"workbench\""));
    }

    #[test]
    fn canvas_session_post_cannot_change_program_selection() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let status = Arc::new(DevStatus::new_with_terminal("app.jet", false, false, false));
        status.set_port(port);
        let debug_sessions = crate::Canvas::DebugSessions::default();
        let session = Arc::new(crate::ResidentDevSession::new("app.jet", port, 0));
        session.select_output_values(Some("cli-output"), Some("cli-target"));
        let secret = mint_session_secret().unwrap();
        let server = thread::spawn({
            let session = Arc::clone(&session);
            let secret = secret.clone();
            move || {
                let (stream, _) = listener.accept().unwrap();
                handle_connection(
                    stream,
                    &status,
                    &debug_sessions,
                    &session,
                    "app.jet",
                    ListenerKind::Canvas,
                    "127.0.0.1",
                    &secret,
                )
                .unwrap();
            }
        });
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = r#"{"op":"select_output","output":"web","target":"browser"}"#;
        let request = format!(
            "POST /canvas/session HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nAuthorization: Bearer {secret}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        std::io::Write::write_all(&mut client, request.as_bytes()).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        server.join().unwrap();

        assert!(
            response.starts_with("HTTP/1.1 405 Method Not Allowed"),
            "{response}"
        );
        assert!(session
            .json()
            .contains("\"run\":{\"output\":\"cli-output\",\"target\":\"cli-target\"}"));
    }

    #[test]
    fn canvas_request_gate_requires_secret_and_strict_request_context() {
        let secret = mint_session_secret().unwrap();
        let mut headers = HashMap::new();
        headers.insert("host".to_string(), "localhost:8123".to_string());
        headers.insert("origin".to_string(), "http://localhost:8123".to_string());
        headers.insert("authorization".to_string(), format!("Bearer {secret}"));
        let mut request = Request {
            method: "GET".to_string(),
            target: "/canvas/graph".to_string(),
            headers,
            body: Vec::new(),
        };
        assert!(canvas_api_path("/canvas/graph", "/canvas/graph"));
        for path in [
            "/__jet_devtools/workbench",
            "/__jet_devtools/recording",
            "/__jet_devtools/paused/inspect",
            "/__jet_devtools/style",
            "/__jet_canvas/devtools",
            "/canvas/devtools",
            "/panel/devtools",
        ] {
            assert!(
                canvas_api_path(path, path),
                "{path} must be an authorized Canvas route"
            );
            assert!(
                devtools_path(path),
                "{path} must enter the devtools dispatcher"
            );
        }
        assert!(canvas_api_path("/__jet_dev_status", "/__jet_dev_status"));
        assert!(canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request.target = format!("/canvas/graph?session={secret}&session=wrong");
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request.target = "/canvas/graph".to_string();
        request.headers.remove("authorization");
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request
            .headers
            .insert("authorization".to_string(), format!("Bearer {secret}"));
        request.headers.remove("origin");
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request
            .headers
            .insert("sec-fetch-site".to_string(), "same-origin".to_string());
        assert!(canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request.target = format!("/canvas/graph?session={secret}");
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request
            .headers
            .insert("origin".to_string(), "http://localhost:8123".to_string());
        request
            .headers
            .insert("authorization".to_string(), format!("Basic {secret}"));
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request
            .headers
            .insert("authorization".to_string(), format!("Bearer {secret}"));
        request.method = "PUT".to_string();
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request.method = "GET".to_string();
        request.target = "/canvas/not-a-route".to_string();
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request.target = "/canvas/graph".to_string();
        request.body = vec![0; crate::MAX_REQUEST_BODY_BYTES + 1];
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8123,
            &secret
        ));
        request.body.clear();
        assert!(!canvas_request_authorized(
            &request,
            &request.target,
            "127.0.0.1",
            8124,
            &secret
        ));
        assert!(!host_header_allowed("evil.test:8123", "127.0.0.1", 8123));
        assert!(!origin_allowed("http://evil.test:8123", "127.0.0.1", 8123));
        assert!(!constant_time_equal(&secret, "wrong"));
    }

    #[test]
    fn canvas_page_and_bootstrap_reject_missing_session() {
        let secret = "canvas-test-secret";
        let response = canvas_response("/canvas", secret);
        assert!(
            response.starts_with("HTTP/1.1 401 Unauthorized"),
            "{response}"
        );

        let response = canvas_response("/canvas/app.js", secret);
        assert!(
            response.starts_with("HTTP/1.1 401 Unauthorized"),
            "{response}"
        );

        let response = canvas_response(&format!("/canvas?session={secret}"), secret);
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(
            response.contains(&format!("app.js?session={secret}&")),
            "{response}"
        );

        let response = canvas_response(&format!("/canvas/app.js?session={secret}"), secret);
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    }

    fn canvas_response(target: &str, secret: &str) -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        let raw =
            format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
        std::io::Write::write_all(&mut client, raw.as_bytes()).unwrap();

        let status = DevStatus::new_with_terminal("app.jet", false, false, false);
        status.set_port(port);
        let session =
            crate::ResidentDevSession::new_with_canvas_host("app.jet", "127.0.0.1", port, 0);
        let debug_sessions = crate::Canvas::DebugSessions::default();
        handle_connection(
            server,
            &status,
            &debug_sessions,
            &session,
            "app.jet",
            ListenerKind::Canvas,
            "127.0.0.1",
            secret,
        )
        .unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn canvas_session_injection_only_changes_html_bootstrap() {
        let html = crate::canvas_asset("GET", "/canvas", "/canvas").unwrap();
        let html = inject_canvas_session(html, "secret");
        assert!(html.body.contains("app.js?session=secret&"));

        let js = crate::canvas_asset("GET", "/canvas/app.js", "/canvas/app.js").unwrap();
        let js = inject_canvas_session(js, "secret");
        assert!(!js.body.contains("session=secret"));
    }
    #[test]
    fn release_devtools_route_is_read_only_and_token_gated() {
        use jet_driver::Package::{
            ReleaseDevtoolsDeployment, ReleaseDevtoolsPolicy, ReleaseInspect,
        };

        let policy = ReleaseDevtoolsPolicy::from_manifest_profile(ReleaseInspect::ReadOnly)
            .with_deployment(ReleaseDevtoolsDeployment::with_values(
                true,
                Some("release-token".to_string()),
                Default::default(),
            ));
        let unauthorized = release_response(&policy, "GET", None, "/__jet_devtools");
        assert!(
            unauthorized.starts_with("HTTP/1.1 401 Unauthorized"),
            "{unauthorized}"
        );
        assert!(
            unauthorized.contains("JET_INSPECT_TOKEN required"),
            "{unauthorized}"
        );

        let writable = release_response(
            &policy,
            "POST",
            Some("Bearer release-token"),
            "/__jet_devtools",
        );
        assert!(
            writable.starts_with("HTTP/1.1 405 Method Not Allowed"),
            "{writable}"
        );
        assert!(!writable.contains("\"protocol\""), "{writable}");

        let authorized = release_response(
            &policy,
            "GET",
            Some("Bearer release-token"),
            "/__jet_devtools",
        );
        assert!(authorized.starts_with("HTTP/1.1 200 OK"), "{authorized}");
        assert!(
            authorized.contains("\"protocol\":\"jet.devtools.v1\""),
            "{authorized}"
        );

        let local = ReleaseDevtoolsPolicy::from_manifest_profile(ReleaseInspect::Local)
            .with_deployment(ReleaseDevtoolsDeployment::with_values(
                true,
                Some("release-token".to_string()),
                Default::default(),
            ));
        let absent = release_response(
            &local,
            "GET",
            Some("Bearer release-token"),
            "/__jet_devtools",
        );
        assert!(absent.starts_with("HTTP/1.1 404 Not Found"), "{absent}");
    }

    fn release_response(
        policy: &jet_driver::Package::ReleaseDevtoolsPolicy,
        method: &str,
        authorization: Option<&str>,
        target: &str,
    ) -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(address).unwrap();
        let (server, peer_addr) = listener.accept().unwrap();
        let mut request = format!(
            "{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n",
            port = address.port()
        );
        if let Some(authorization) = authorization {
            request.push_str(&format!("Authorization: {authorization}\r\n"));
        }
        request.push_str("Connection: close\r\n\r\n");
        client.write_all(request.as_bytes()).unwrap();

        let status = DevStatus::new_with_terminal("release.jet", false, false, false);
        status.set_port(address.port());
        let session = crate::ResidentDevSession::new("release.jet", 0, address.port());
        let debug_sessions = crate::Canvas::DebugSessions::default();
        let browser_state = Arc::new(Mutex::new(None));
        let paused_evaluate =
            Arc::new(Mutex::new(crate::PausedEvaluate::PausedEvaluateHost::new()));
        let terminal_style = Arc::new(crate::TerminalStyleReload::TerminalStyleHostAdapter::new());
        handle_connection_with_root_and_policy(
            server,
            &status,
            &debug_sessions,
            &session,
            "release.jet",
            ListenerKind::Application,
            "127.0.0.1",
            "unused-session",
            std::path::Path::new("."),
            &browser_state,
            &paused_evaluate,
            &terminal_style,
            policy,
            peer_addr,
        )
        .unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        response
    }
}
