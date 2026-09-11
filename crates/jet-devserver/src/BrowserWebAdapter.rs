//! HTTP-facing adapter for the typed browser devtools host.
//!
//! `ResidentDevSession` owns the canonical event stream.  This module only
//! converts its typed projection into `BrowserHost` facts and renders those
//! facts for the in-app and workbench routes.  It does not decode the
//! canonical JSON envelope or assign panel meaning.

use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;

use jet_foundation::DataTree::DataTree;
use jet_foundation::Devtools::jet_observe_live_value_update;
use jet_foundation::DevtoolsControl::JetDevtoolsCommandReceipt;
use jet_foundation::HotSwap::HotSwapDecision;
use jet_foundation::JSON::{json_escape, parse_json_with_limit};
use jet_foundation::SHA256::sha256_hex;

use crate::BrowserHost::{
    BrowserActionGrant, BrowserDock, BrowserHost, BrowserHostAction, BrowserHostEffect,
    BrowserHostError, BrowserHostEvent, BrowserHtmlElement, BrowserNode, BrowserNodeKind,
    BrowserPanelFact, BrowserPanelView, BrowserPlacement, BrowserReconnectCursor, BrowserSelection,
    BrowserSessionState, BrowserSourceGrant, BrowserSourceLocation, MAX_BROWSER_TEXT,
    MAX_BROWSER_VALUE,
};
use crate::Devtools::{
    JetDevtoolsHostKind, JetDevtoolsPanelCapability, JetDevtoolsPanelProjection,
};
use crate::Session::{
    DevtoolsProjection, DevtoolsSessionState, DevtoolsStatusFact, ResidentDevSession,
};
use crate::WebModuleSwap::{
    WebFocusFact, WebFormDraftFact, WebIslandFact, WebModuleFact, WebModuleSwapIdentity,
    WebModuleSwapPlan, WebModuleSwapReceipt, WebModuleSwapSnapshot, WebModuleSwapState,
    WebModuleSwapStateFact, WebPageFact, WebQueryCacheFact, WebScrollFact, WebSignalFact,
    WebStateValue, WebStoreFact,
};

const BROWSER_PAGE_SCHEMA: &str = "jet.devtools.browser.v1";

#[derive(Clone, Debug, Eq, PartialEq)]
struct WebLiveValueUpdate {
    key: String,
    type_identity: String,
    rendered_value: Option<String>,
}

/// What the devtools page may really do on this host.  Every `false` carries
/// the one reason the page shows; the page never infers a tool from a panel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserTools {
    /// Canvas source authority (inspect, checked preview, revision-bound apply).
    pub source: bool,
    pub source_reason: Option<String>,
    /// A resident rebuild executor is registered for `ProjectRebuild`.
    pub rebuild: bool,
    pub rebuild_reason: Option<String>,
}

/// One placement's flattened typed browser frame plus canonical session
/// identity/status metadata.  The host view remains the source of panel,
/// element, and selection facts; the adapter adds no semantic panel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserDevtoolsFrame {
    pub placement: BrowserPlacement,
    pub route: String,
    pub protocol: String,
    pub session_id: String,
    pub source_id: Option<String>,
    pub build_id: Option<String>,
    pub revision: String,
    pub world_id: Option<String>,
    pub sequence: Option<u64>,
    pub cursor: Option<BrowserReconnectCursor>,
    pub state: BrowserSessionState,
    pub status: DevtoolsStatusFact,
    pub tools: BrowserTools,
    pub receipts: Vec<JetDevtoolsCommandReceipt>,
    pub selection: Option<BrowserSelection>,
    pub panels: Vec<BrowserPanelView>,
    pub elements: Vec<BrowserHtmlElement>,
}

/// One shared browser state for both browser placements.
#[derive(Debug)]
pub(crate) struct BrowserWebState {
    host: BrowserHost,
    facts: BTreeMap<String, BrowserPanelFact>,
    panel_order: VecDeque<String>,
    source_id: Option<String>,
    build_id: Option<String>,
    revision: String,
    world_id: Option<String>,
    status: DevtoolsStatusFact,
    tools: BrowserTools,
    receipts: Vec<JetDevtoolsCommandReceipt>,
    swap: Option<WebModuleSwapState>,
    last_swap: Option<WebModuleSwapReceipt>,
    swap_error: Option<String>,
    dom_state: Vec<WebModuleSwapStateFact>,
}

impl BrowserWebState {
    fn new(session_id: String) -> Self {
        Self {
            host: BrowserHost::new(session_id),
            facts: BTreeMap::new(),
            panel_order: VecDeque::new(),
            source_id: None,
            build_id: None,
            revision: String::new(),
            world_id: None,
            status: starting_status(),
            tools: BrowserTools {
                source: false,
                source_reason: None,
                rebuild: false,
                rebuild_reason: None,
            },
            receipts: Vec::new(),
            swap: None,
            last_swap: None,
            swap_error: None,
            dom_state: Vec::new(),
        }
    }

    pub(crate) fn host(&self) -> &BrowserHost {
        &self.host
    }

    pub(crate) fn frame(&self, placement: BrowserPlacement) -> BrowserDevtoolsFrame {
        let model = self.host.view_model(placement);
        BrowserDevtoolsFrame {
            placement: model.placement,
            route: model.route,
            protocol: model.protocol,
            session_id: model.session_id,
            source_id: self.source_id.clone(),
            build_id: self.build_id.clone(),
            revision: self.revision.clone(),
            world_id: self.world_id.clone(),
            sequence: model.sequence,
            cursor: model.cursor,
            state: browser_session_state(&self.status.state),
            status: self.status.clone(),
            tools: self.tools.clone(),
            receipts: self.receipts.clone(),
            selection: model.selection,
            panels: model.panels,
            elements: model.elements,
        }
    }

    fn reset(&mut self, session_id: String) {
        self.host = BrowserHost::new(session_id);
        self.facts.clear();
        self.panel_order.clear();
        self.source_id = None;
        self.build_id = None;
        self.revision.clear();
        self.world_id = None;
        self.status = starting_status();
        self.receipts.clear();
        self.swap = None;
        self.last_swap = None;
        self.swap_error = None;
        self.dom_state.clear();
    }

    fn sync(
        &mut self,
        projection: DevtoolsProjection,
        canonical_panels: Vec<JetDevtoolsPanelProjection>,
    ) -> Result<(), String> {
        if projection.protocol != crate::Session::JET_DEVTOOLS_PROTOCOL {
            return Err("devtools projection protocol does not match jet.devtools.v1".to_string());
        }
        if projection.session_id != self.host.session_id() {
            self.reset(projection.session_id.clone());
        }
        if projection.reset {
            self.facts.clear();
            self.panel_order.clear();
            self.host = BrowserHost::new(projection.session_id.clone());
        }

        let source_id = bounded_optional(projection.source_id.clone(), MAX_BROWSER_TEXT);
        let build_id = bounded_optional(projection.build_id.clone(), MAX_BROWSER_TEXT);

        self.source_id = source_id;
        self.build_id = build_id;
        self.revision = bounded_text(&projection.revision, "", MAX_BROWSER_TEXT);
        self.world_id = bounded_optional(projection.world_id, MAX_BROWSER_TEXT);
        self.status = normalize_status(projection.status);
        self.tools.rebuild = projection.command_capabilities.project_rebuild;
        self.tools.rebuild_reason = bounded_optional(
            projection.command_capabilities.project_rebuild_reason,
            MAX_BROWSER_TEXT,
        );
        self.receipts = projection.command_receipts;

        self.facts.clear();
        self.panel_order.clear();
        for panel in canonical_panels {
            let fact = panel_from_projection(&panel);
            let panel_id = fact.panel_id.clone();
            self.panel_order.push_back(panel_id.clone());
            self.facts.insert(panel_id, fact);
        }

        let sequence = projection.sequence;
        let should_apply = self
            .host
            .sequence()
            .map_or(true, |current| sequence > current);
        if should_apply {
            let revision = self
                .host
                .revision()
                .unwrap_or(0)
                .max(parse_revision(&projection.revision).unwrap_or(sequence));
            let panels = self
                .panel_order
                .iter()
                .filter_map(|panel_id| self.facts.get(panel_id))
                .cloned()
                .collect();
            let event = BrowserHostEvent::new(
                self.host.session_id(),
                revision,
                sequence,
                projection.observed_at,
                panels,
            );
            self.host
                .apply_event(event)
                .map_err(|error| format!("browser projection rejected event: {error}"))?;
        }

        match projection.selection {
            Some(selection) => {
                if self.host.panel(&selection.panel_id).is_some() {
                    if selection.item_key.is_empty()
                        || self
                            .host
                            .select(&selection.panel_id, Some(selection.item_key.clone()))
                            .is_ok()
                    {
                        if selection.item_key.is_empty() {
                            self.host
                                .select(&selection.panel_id, Option::<String>::None)
                                .map_err(|error| error.to_string())?;
                        }
                    } else {
                        self.host
                            .select(&selection.panel_id, Option::<String>::None)
                            .map_err(|error| error.to_string())?;
                    }
                } else {
                    self.host.clear_selection();
                }
            }
            None => self.host.clear_selection(),
        }
        Ok(())
    }
}

fn starting_status() -> DevtoolsStatusFact {
    DevtoolsStatusFact {
        state: DevtoolsSessionState::Starting,
        diagnostic_code: None,
        diagnostic: None,
        accepted_revision: None,
        last_good_revision: None,
        run_target: None,
        run_output: None,
        test_state: None,
    }
}

fn normalize_status(mut status: DevtoolsStatusFact) -> DevtoolsStatusFact {
    status.diagnostic_code = bounded_optional(status.diagnostic_code, MAX_BROWSER_TEXT);
    status.diagnostic = bounded_optional(status.diagnostic, MAX_BROWSER_VALUE);
    status.accepted_revision = bounded_optional(status.accepted_revision, MAX_BROWSER_TEXT);
    status.last_good_revision = bounded_optional(status.last_good_revision, MAX_BROWSER_TEXT);
    status.run_target = bounded_optional(status.run_target, MAX_BROWSER_TEXT);
    status.run_output = bounded_optional(status.run_output, MAX_BROWSER_VALUE);
    status.test_state = bounded_optional(status.test_state, MAX_BROWSER_TEXT);
    status
}

fn bounded_optional(value: Option<String>, limit: usize) -> Option<String> {
    value.and_then(|value| {
        let value = bounded_text(&value, "", limit);
        (!value.is_empty()).then_some(value)
    })
}

fn browser_session_state(state: &DevtoolsSessionState) -> BrowserSessionState {
    match state {
        DevtoolsSessionState::Starting | DevtoolsSessionState::Ready => BrowserSessionState::Idle,
        DevtoolsSessionState::Building => BrowserSessionState::Building,
        DevtoolsSessionState::Error => BrowserSessionState::BuildFailed,
        DevtoolsSessionState::Unavailable | DevtoolsSessionState::Stopped => {
            BrowserSessionState::RuntimeFailure
        }
    }
}

/// Advance one shared browser state from the session's typed projection.
pub(crate) fn sync_session(
    state: &mut Option<BrowserWebState>,
    session: &ResidentDevSession,
) -> Result<(), String> {
    let cursor = state.as_ref().and_then(|state| state.host().sequence());
    let projection = session.devtools_events_since(cursor);
    let canonical_panels = session
        .devtools_panels(
            JetDevtoolsHostKind::BrowserInApp,
            &JetDevtoolsPanelCapability::ALL,
        )
        .map_err(|error| format!("browser panel catalog rejected projection: {error}"))?;
    if state.is_none() {
        *state = Some(BrowserWebState::new(projection.session_id.clone()));
    }
    let state = state.as_mut().expect("browser web state initialized");
    // Source tools exist only where this host holds Canvas source authority;
    // the page shows this one reason instead of a disabled-tool wall.
    state.tools.source = session.canvas_port() != 0;
    state.tools.source_reason = (!state.tools.source).then(|| SOURCE_TOOLS_OFF_REASON.to_string());
    state.sync(projection, canonical_panels)
}

/// Shown once by the devtools page (and returned by the host's 404) when a
/// host has no Canvas source authority.
pub(crate) const SOURCE_TOOLS_OFF_REASON: &str =
    "This host runs without Canvas, so source is read and edited elsewhere.";

fn object_map(value: &DataTree) -> Result<BTreeMap<String, DataTree>, String> {
    match value {
        DataTree::Object(fields) => Ok(fields.iter().cloned().collect()),
        _ => Err("expected a JSON object".to_string()),
    }
}

/// Consume a typed DOM/runtime snapshot captured from the live page.  The
/// browser may publish the seven `WebModuleSwap` state families plus raw
/// `persist_live_value` rows; only the former become swap host state, while
/// the latter go through Foundation's shared live-value policy.
pub(crate) fn record_dom_snapshot(
    state: &mut Option<BrowserWebState>,
    session_id: &str,
    payload: &[u8],
) -> Result<(), String> {
    let text = std::str::from_utf8(payload)
        .map_err(|_| "web DOM snapshot body must be UTF-8 JSON".to_string())?;
    let root = parse_json_with_limit(text, crate::MAX_REQUEST_BODY_BYTES)
        .map_err(|_| "web DOM snapshot body is not valid bounded JSON".to_string())?;
    let object =
        object_map(&root).map_err(|_| "web DOM snapshot must be a JSON object".to_string())?;
    let protocol = required_json_string(&object, "protocol")?;
    if protocol != crate::Session::JET_DEVTOOLS_PROTOCOL {
        return Err("web DOM snapshot protocol does not match jet.devtools.v1".to_string());
    }
    if required_json_string(&object, "kind")? != "web.dom-snapshot" {
        return Err("web DOM snapshot kind is not web.dom-snapshot".to_string());
    }
    let rows = object
        .get("state")
        .ok_or_else(|| "web DOM snapshot must include state".to_string())?
        .as_array()
        .map_err(|_| "web DOM snapshot state must be an array".to_string())?;
    let mut facts = Vec::with_capacity(rows.len());
    let mut live_value_updates = Vec::new();
    let mut seen = BTreeMap::new();
    for row in rows {
        let row = object_map(row)
            .map_err(|_| "web DOM snapshot state rows must be objects".to_string())?;
        let kind = required_json_string(&row, "kind")?;
        if kind == "persist_live_value" {
            let update = parse_live_value_update(&row)?;
            let seen_key = (kind.clone(), update.key.clone());
            if seen.insert(seen_key, ()).is_some() {
                return Err("web DOM snapshot state keys must be unique per family".to_string());
            }
            live_value_updates.push(update);
            continue;
        }
        let key = required_json_string(&row, "key")?;
        let type_fingerprint = required_json_string(&row, "type_fingerprint")?;
        let identity = row
            .get("identity")
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .map_err(|_| "web DOM island identity must be a string".to_string())
            })
            .transpose()?;
        let value = parse_dom_state_value(
            row.get("value")
                .ok_or_else(|| "web DOM snapshot state row has no value".to_string())?,
        )?;
        let seen_key = (kind.clone(), key.clone());
        if seen.insert(seen_key, ()).is_some() {
            return Err("web DOM snapshot state keys must be unique per family".to_string());
        }
        facts.push(match kind.as_str() {
            "signal" => {
                WebModuleSwapStateFact::Signal(WebSignalFact::new(key, type_fingerprint, value))
            }
            "store" => {
                WebModuleSwapStateFact::Store(WebStoreFact::new(key, type_fingerprint, value))
            }
            "query_cache" => WebModuleSwapStateFact::QueryCache(WebQueryCacheFact::new(
                key,
                type_fingerprint,
                value,
            )),
            "form_draft" => WebModuleSwapStateFact::FormDraft(WebFormDraftFact::new(
                key,
                type_fingerprint,
                value,
            )),
            "scroll" => {
                WebModuleSwapStateFact::Scroll(WebScrollFact::new(key, type_fingerprint, value))
            }
            "focus" => {
                WebModuleSwapStateFact::Focus(WebFocusFact::new(key, type_fingerprint, value))
            }
            "island" => WebModuleSwapStateFact::Island(WebIslandFact::new(
                key,
                identity.ok_or_else(|| "web DOM island state needs identity".to_string())?,
                type_fingerprint,
                value,
            )),
            _ => {
                return Err(format!(
                    "web DOM snapshot has unknown state family `{kind}`"
                ))
            }
        });
    }
    for update in live_value_updates {
        jet_observe_live_value_update(
            &update.key,
            &update.type_identity,
            update.rendered_value.as_deref(),
        );
    }
    let state = state.get_or_insert_with(|| BrowserWebState::new(session_id.to_string()));
    state.dom_state = facts;
    Ok(())
}

fn parse_live_value_update(
    object: &std::collections::BTreeMap<String, DataTree>,
) -> Result<WebLiveValueUpdate, String> {
    for field in object.keys() {
        if !matches!(
            field.as_str(),
            "kind" | "key" | "type_identity" | "rendered_value"
        ) {
            return Err(format!(
                "web DOM snapshot persist_live_value has unknown field `{field}`"
            ));
        }
    }
    if required_json_string(object, "kind")? != "persist_live_value" {
        return Err("web DOM snapshot live value row has the wrong kind".to_string());
    }
    let key = required_json_string(object, "key")?;
    let type_identity = required_json_string(object, "type_identity")?;
    let rendered_value = match object.get("rendered_value") {
        Some(DataTree::Null) => None,
        Some(DataTree::Text(value)) => Some(value.to_string()),
        Some(_) => {
            return Err(
                "web DOM snapshot persist_live_value `rendered_value` must be a string or null"
                    .to_string(),
            )
        }
        None => {
            return Err("web DOM snapshot persist_live_value has no `rendered_value`".to_string())
        }
    };
    Ok(WebLiveValueUpdate {
        key,
        type_identity,
        rendered_value,
    })
}

fn required_json_string(
    object: &std::collections::BTreeMap<String, DataTree>,
    key: &str,
) -> Result<String, String> {
    object
        .get(key)
        .ok_or_else(|| format!("web DOM snapshot has no `{key}`"))
        .and_then(|value| {
            value
                .as_str()
                .map(str::to_string)
                .map_err(|_| format!("web DOM snapshot `{key}` must be a string"))
        })
}

fn parse_dom_state_value(value: &DataTree) -> Result<WebStateValue, String> {
    let object = object_map(value)
        .map_err(|_| "web DOM snapshot state value must be an object".to_string())?;
    let kind = required_json_string(&object, "kind")?;
    match kind.as_str() {
        "text" => Ok(WebStateValue::Text(required_json_string(&object, "value")?)),
        "integer" => match object.get("value") {
            Some(DataTree::Int(value)) => Ok(WebStateValue::Integer(*value)),
            _ => Err("web DOM integer state value must be a JSON integer".to_string()),
        },
        "unsigned" => match object.get("value") {
            Some(DataTree::Int(value)) if *value >= 0 => Ok(WebStateValue::Unsigned(*value as u64)),
            _ => Err("web DOM unsigned state value must be a non-negative integer".to_string()),
        },
        "boolean" => match object.get("value") {
            Some(DataTree::Bool(value)) => Ok(WebStateValue::Boolean(*value)),
            _ => Err("web DOM boolean state value must be a JSON boolean".to_string()),
        },
        "bytes" => {
            let values = object
                .get("value")
                .ok_or_else(|| "web DOM bytes state value has no value".to_string())?
                .as_array()
                .map_err(|_| "web DOM bytes state value must be an array".to_string())?;
            let mut bytes = Vec::with_capacity(values.len());
            for value in values {
                match value {
                    DataTree::Int(value) if (0..=255).contains(value) => bytes.push(*value as u8),
                    _ => return Err("web DOM bytes state value has a non-byte element".to_string()),
                }
            }
            Ok(WebStateValue::Bytes(bytes))
        }
        _ => Err(format!("web DOM state value has unknown kind `{kind}`")),
    }
}

/// Return the JSON view model consumed by browser scripts and tests.
pub(crate) fn view_json(state: &BrowserWebState, placement: BrowserPlacement) -> String {
    frame_json(&state.frame(placement))
}
pub(crate) fn swap_json(state: &BrowserWebState) -> String {
    let mut output = String::with_capacity(2048);
    output.push('{');
    field_string(
        &mut output,
        "protocol",
        crate::Session::JET_DEVTOOLS_PROTOCOL,
        true,
    );
    field_string(&mut output, "kind", "web.module-swap", false);
    output.push_str(",\"active\":");
    match state.swap.as_ref() {
        Some(swap) => write_swap_snapshot(&mut output, swap.active()),
        None => output.push_str("null"),
    }
    output.push_str(",\"committed\":");
    match state.swap.as_ref() {
        Some(swap) => write_swap_snapshot(&mut output, swap.committed()),
        None => output.push_str("null"),
    }
    output.push_str(",\"phase\":");
    match state.swap.as_ref() {
        Some(swap) => quoted(swap.phase().as_str(), &mut output),
        None => output.push_str("null"),
    }
    output.push_str(",\"last_receipt\":");
    match state.last_swap.as_ref() {
        Some(receipt) => write_swap_receipt(&mut output, receipt),
        None => output.push_str("null"),
    }
    output.push_str(",\"error\":");
    match state.swap_error.as_deref() {
        Some(error) => quoted(error, &mut output),
        None => output.push_str("null"),
    }
    output.push('}');
    output
}

/// Render the release inspection surface without exposing event payloads.
/// Release builds may publish the canonical panel catalog and session
/// lineage, but never diagnostics, request bodies, source text, or actions.
pub(crate) fn release_json(
    projection: &DevtoolsProjection,
    panels: &[JetDevtoolsPanelProjection],
) -> String {
    let mut output = String::with_capacity(2048);
    output.push('{');
    field_string(&mut output, "protocol", &projection.protocol, true);
    field_string(&mut output, "kind", "devtools.release", false);
    field_string(&mut output, "session_id", &projection.session_id, false);
    field_optional_string(&mut output, "source_id", projection.source_id.as_deref());
    field_optional_string(&mut output, "build_id", projection.build_id.as_deref());
    field_string(&mut output, "revision", &projection.revision, false);
    field_string(
        &mut output,
        "lifecycle",
        projection.status.state.as_str(),
        false,
    );
    field_bool(&mut output, "read_only", true, false);
    field_bool(&mut output, "payloads_redacted", true, false);
    output.push_str(",\"panels\":[");
    for (index, panel) in panels.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        let descriptor = &panel.availability.descriptor;
        output.push('{');
        field_string(&mut output, "id", descriptor.id.as_str(), true);
        field_string(&mut output, "title", descriptor.title, false);
        field_bool(
            &mut output,
            "available",
            panel.availability.is_available(),
            false,
        );
        output.push_str(",\"capabilities\":[");
        for (capability_index, capability) in descriptor.required_capabilities.iter().enumerate() {
            if capability_index != 0 {
                output.push(',');
            }
            quoted(capability.as_str(), &mut output);
        }
        output.push_str("]}");
    }
    output.push(']');
    output.push('}');
    output
}

/// Build and apply one browser swap from the compiler's published Web
/// manifest.  The manifest is the source of module/page identity and
/// fingerprints; the browser contributes only its typed DOM state facts.
pub(crate) fn apply_web_swap_from_manifest(
    state: &mut Option<BrowserWebState>,
    session_id: &str,
    source_revision: &str,
    build_revision: &str,
    manifest: &str,
    decision: Option<&HotSwapDecision>,
) -> Result<(), String> {
    let dom_state = state
        .as_ref()
        .map(|state| state.dom_state.clone())
        .unwrap_or_default();
    let candidate =
        match snapshot_from_manifest(source_revision, build_revision, manifest, dom_state) {
            Ok(candidate) => candidate,
            Err(error) => {
                let state =
                    state.get_or_insert_with(|| BrowserWebState::new(session_id.to_string()));
                state.swap_error = Some(error.clone());
                return Err(error);
            }
        };
    apply_web_swap_snapshot(state, session_id, candidate, decision)
}
pub(crate) fn manifest_artifact_identity(manifest: &str) -> Result<String, String> {
    let root = parse_manifest(manifest)?;
    let root = object_map(&root).map_err(|_| "web manifest must be a JSON object".to_string())?;
    let package = required_json_string(&root, "package")?;
    if package.is_empty() {
        return Err("web manifest package identity is empty".to_string());
    }
    let artifact = root
        .get("artifact")
        .ok_or_else(|| "web manifest has no selected artifact".to_string())?;
    let artifact =
        object_map(artifact).map_err(|_| "web manifest artifact must be an object".to_string())?;
    let identity = required_json_string(&artifact, "artifact_identity")?;
    if identity.is_empty() {
        return Err("web manifest artifact identity is empty".to_string());
    }
    Ok(identity)
}

fn parse_manifest(manifest: &str) -> Result<DataTree, String> {
    let root = parse_json_with_limit(manifest, super::MAX_STATIC_RESPONSE_BYTES as usize)
        .map_err(|_| "web manifest is not valid bounded JSON".to_string())?;
    let object = object_map(&root).map_err(|_| "web manifest must be a JSON object".to_string())?;
    let schema = required_json_string(&object, "schema")?;
    if schema != "mir-web-v1" {
        return Err(format!("unsupported web manifest schema `{schema}`"));
    }
    Ok(root)
}

fn snapshot_from_manifest(
    source_revision: &str,
    build_revision: &str,
    manifest: &str,
    state: Vec<WebModuleSwapStateFact>,
) -> Result<WebModuleSwapSnapshot, String> {
    if source_revision.is_empty() || build_revision.is_empty() {
        return Err("web swap requires non-empty source and build revisions".to_string());
    }
    let root = parse_manifest(manifest)?;
    let root = object_map(&root).map_err(|_| "web manifest must be a JSON object".to_string())?;
    let package = required_json_string(&root, "package")?;
    let artifact = root
        .get("artifact")
        .ok_or_else(|| "web manifest has no selected artifact".to_string())?;
    let artifact =
        object_map(artifact).map_err(|_| "web manifest artifact must be an object".to_string())?;
    let artifact_identity = required_json_string(&artifact, "artifact_identity")?;
    let selected_modules = artifact
        .get("modules")
        .ok_or_else(|| "web manifest artifact has no modules".to_string())?
        .as_array()
        .map_err(|_| "web manifest artifact modules must be an array".to_string())?;
    let modules = root
        .get("modules")
        .ok_or_else(|| "web manifest has no module table".to_string())?
        .as_array()
        .map_err(|_| "web manifest modules must be an array".to_string())?;
    let mut module_facts = Vec::with_capacity(selected_modules.len());
    for selected in selected_modules {
        let DataTree::Int(module_id) = selected else {
            return Err("web manifest module id must be an integer".to_string());
        };
        let module_value = modules
            .iter()
            .find(|module| {
                let Ok(module) = object_map(module) else {
                    return false;
                };
                matches!(module.get("id"), Some(DataTree::Int(id)) if *id == *module_id)
            })
            .ok_or_else(|| format!("web manifest selected module {module_id} is missing"))?;
        let module = object_map(module_value)
            .map_err(|_| "web manifest module must be an object".to_string())?;
        let identity = required_json_string(&module, "key")?;
        let path = required_json_string(&module, "path")?;
        let imports = module
            .get("imports")
            .ok_or_else(|| "web manifest module has no imports".to_string())?
            .as_array()
            .map_err(|_| "web manifest module imports must be an array".to_string())?
            .iter()
            .map(|id| match id {
                DataTree::Int(id) => Ok(id.to_string()),
                _ => Err("web manifest module import id must be an integer".to_string()),
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(",");
        let item_order = module
            .get("item_order")
            .ok_or_else(|| "web manifest module has no item_order".to_string())?
            .as_array()
            .map_err(|_| "web manifest module item_order must be an array".to_string())?
            .iter()
            .map(|item| {
                item.as_str().map(str::to_string).map_err(|_| {
                    "web manifest module item_order entries must be strings".to_string()
                })
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("\0");
        let interface_seed = format!("{identity}\0{path}\0{imports}\0{item_order}");
        let interface_fingerprint = format!("sha256-{}", sha256_hex(interface_seed.as_bytes()));
        let body_seed = format!("{artifact_identity}\0{module_id}\0{path}");
        let body_fingerprint = format!("sha256-{}", sha256_hex(body_seed.as_bytes()));
        module_facts.push(WebModuleFact::new(
            identity,
            interface_fingerprint,
            body_fingerprint,
        ));
    }
    WebModuleSwapSnapshot::new(
        WebModuleSwapIdentity::new(package, "application"),
        source_revision,
        build_revision,
        WebPageFact::new("application", artifact_identity),
        module_facts,
        state,
    )
    .map_err(|error| format!("web swap candidate rejected: {error}"))
}

fn apply_web_swap_snapshot(
    state: &mut Option<BrowserWebState>,
    session_id: &str,
    candidate: WebModuleSwapSnapshot,
    decision: Option<&HotSwapDecision>,
) -> Result<(), String> {
    let state = state.get_or_insert_with(|| BrowserWebState::new(session_id.to_string()));
    if state.swap.is_none() {
        if decision.is_some() {
            let error =
                "web swap decision has no resident snapshot; rebuild was rejected".to_string();
            state.swap_error = Some(error.clone());
            return Err(error);
        }
        state.swap = Some(
            WebModuleSwapState::new(candidate)
                .map_err(|error| format!("web swap state rejected: {error}"))?,
        );
        state.last_swap = None;
        state.swap_error = None;
        return Ok(());
    }

    let decision = decision.ok_or_else(|| {
        let error =
            "web rebuild has no canonical HotSwapDecision; compatibility planning was rejected"
                .to_string();
        state.swap_error = Some(error.clone());
        error
    })?;
    let swap = state.swap.as_mut().expect("web swap state initialized");
    let plan = WebModuleSwapPlan::from_hot_swap_decision(swap.committed(), &candidate, decision)
        .map_err(|error| format!("web swap plan rejected: {error}"))?;
    match swap.apply_decision(plan) {
        Ok(receipt) => {
            state.last_swap = Some(receipt);
            state.swap_error = None;
            Ok(())
        }
        Err(error) => {
            state.swap_error = Some(error.to_string());
            Err(format!("web swap transaction rejected: {error}"))
        }
    }
}

fn write_swap_snapshot(output: &mut String, snapshot: &WebModuleSwapSnapshot) {
    output.push('{');
    output.push_str("\"identity\":{");
    field_string(output, "module", &snapshot.identity.module, true);
    field_string(output, "page", &snapshot.identity.page, false);
    output.push('}');
    field_string(output, "source_revision", &snapshot.source_revision, false);
    field_string(output, "build_revision", &snapshot.build_revision, false);
    output.push_str(",\"page\":{");
    field_string(output, "identity", &snapshot.page.identity, true);
    field_string(output, "artifact", &snapshot.page.artifact, false);
    output.push('}');
    output.push_str(",\"modules\":[");
    for (index, module) in snapshot.modules.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push('{');
        field_string(output, "identity", &module.identity, true);
        field_string(
            output,
            "interface_fingerprint",
            &module.interface_fingerprint,
            false,
        );
        field_string(output, "body_fingerprint", &module.body_fingerprint, false);
        output.push('}');
    }
    output.push(']');
    field_u64(output, "state_count", snapshot.state.len() as u64, false);
    output.push('}');
}

fn write_swap_receipt(output: &mut String, receipt: &WebModuleSwapReceipt) {
    output.push('{');
    field_string(output, "operation", receipt.operation.as_str(), true);
    field_string(output, "status", receipt.status.as_str(), false);
    field_string(output, "phase", receipt.phase.as_str(), false);
    field_string(output, "source_revision", &receipt.source_revision, false);
    field_string(output, "build_revision", &receipt.build_revision, false);
    output.push_str(",\"identity\":{");
    field_string(output, "module", &receipt.identity.module, true);
    field_string(output, "page", &receipt.identity.page, false);
    output.push('}');
    output.push_str(",\"candidate_identity\":{");
    field_string(output, "module", &receipt.candidate_identity.module, true);
    field_string(output, "page", &receipt.candidate_identity.page, false);
    output.push('}');
    write_string_array(output, "changed_modules", &receipt.changed_modules);
    write_string_array(
        output,
        "reason_codes",
        &receipt
            .reason_codes
            .iter()
            .map(|reason| reason.as_str().to_string())
            .collect::<Vec<_>>(),
    );
    output.push_str(",\"preserved\":[");
    for (index, fact) in receipt.preserved.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push('{');
        field_string(output, "kind", fact.kind().as_str(), true);
        field_string(output, "key", fact.key(), false);
        field_string(output, "identity", fact.identity(), false);
        field_string(output, "type_fingerprint", fact.type_fingerprint(), false);
        output.push('}');
    }
    output.push(']');
    output.push_str(",\"reset\":[");
    for (index, fact) in receipt.reset.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push('{');
        field_string(output, "kind", fact.state.kind().as_str(), true);
        field_string(output, "key", fact.state.key(), false);
        field_string(output, "identity", fact.state.identity(), false);
        field_optional_string(output, "prior_identity", fact.prior_identity.as_deref());
        field_optional_string(
            output,
            "prior_type_fingerprint",
            fact.prior_type_fingerprint.as_deref(),
        );
        field_string(output, "reason", fact.reason.as_str(), false);
        output.push('}');
    }
    output.push(']');
    output.push_str(",\"committed_page\":{");
    field_string(output, "identity", &receipt.committed_page.identity, true);
    field_string(output, "artifact", &receipt.committed_page.artifact, false);
    output.push_str("},\"active_page\":{");
    field_string(output, "identity", &receipt.active_page.identity, true);
    field_string(output, "artifact", &receipt.active_page.artifact, false);
    output.push_str("}}");
}

fn write_string_array(output: &mut String, key: &str, values: &[String]) {
    output.push(',');
    quoted(key, output);
    output.push_str(":[");
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        quoted(value, output);
    }
    output.push(']');
}

/// Page styling shared by both placements.  Obsidian ground, a quiet raised
/// surface, and Jet red only where something needs attention.  Groups stay
/// collapsed until their intent is relevant or the developer opens them.
const PAGE_CSS: &str = r#"
:root{color-scheme:dark;--bg:#0F1013;--raised:#17191D;--raised-2:#1D2026;--well:#0A0B0D;--line:#30343C;--ink:#F6F5F2;--dim:#9B9EA8;--red:#E8232A;--ok:#56C271;--amber:#E3A43B;--sans:-apple-system,BlinkMacSystemFont,"SF Pro Text","Segoe UI",system-ui,sans-serif;--mono:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}
*{box-sizing:border-box}html,body{height:100%}
body{margin:0;background:var(--bg);color:var(--ink);font:13px/1.45 var(--sans);-webkit-font-smoothing:antialiased}
button,select,textarea{font:inherit;color:inherit}button{background:none;border:0;padding:0;cursor:pointer;text-align:left}
:focus-visible{outline:2px solid var(--red);outline-offset:3px}textarea:focus-visible{outline:1px solid var(--dim);outline-offset:0}
code,pre,textarea,.mono{font:12px/1.55 var(--mono)}
main{height:100%;display:flex;flex-direction:column}
.bar{display:flex;align-items:center;flex-wrap:wrap;gap:10px;padding:11px 18px;border-bottom:1px solid var(--line);flex:none}
.embedded .bar{display:none}
.mark{display:grid;place-items:center;flex:none;width:40px;height:40px;border:1px solid var(--ok);border-radius:13px;background:var(--well);box-shadow:0 0 14px #56c27144}
.mark svg{display:block;width:32px;height:20px}
[data-state=building] .mark{border-color:var(--amber);border-style:dashed;box-shadow:0 0 14px #e3a43b55}
[data-state=error] .mark,[data-state=unavailable] .mark,[data-state=stopped] .mark{border-color:var(--red);border-style:double;box-shadow:0 0 14px #e8232a66}
[data-state=starting] .mark,[data-state=reconnecting] .mark{border-color:var(--dim);border-style:dotted;box-shadow:0 0 14px #9b9ea844}
.state{display:inline-flex;align-items:center;gap:8px;font-weight:600}
.status-mark{display:inline-block;width:10px;height:10px;border:2px solid var(--ok);border-radius:3px;flex:none}
[data-state=building] .status-mark{border-color:var(--amber);border-style:dashed}
[data-state=error] .status-mark,[data-state=unavailable] .status-mark,[data-state=stopped] .status-mark{border-color:var(--red);border-radius:2px}
[data-state=starting] .status-mark,[data-state=reconnecting] .status-mark{border-color:var(--dim);border-style:dotted}
.grow{flex:1}
.note{color:var(--dim);font-size:12px}.note:empty{display:none}
.link{color:var(--ink);text-decoration:none;border:1px solid var(--line);border-radius:9px;padding:6px 11px;font-size:12px}.link:hover{background:var(--raised)}
.link[hidden]{display:none}
.btn{display:inline-flex;align-items:center;gap:7px;padding:7px 12px;border:1px solid var(--line);border-radius:9px;background:var(--raised);font-size:12px;font-weight:500;white-space:nowrap}
.btn:hover{background:var(--raised-2)}.btn.primary{border-color:var(--red)}.btn[disabled]{opacity:.45;cursor:default}
.btn.quiet{background:none;border-color:transparent;color:var(--dim)}.btn.quiet:hover{color:var(--ink);background:var(--raised)}
.attention{flex:none;padding:16px 18px 14px;border-bottom:1px solid var(--line)}
.attention h2{margin:0;display:flex;align-items:center;gap:9px;font-size:14px;font-weight:600;line-height:1.3;flex-wrap:wrap}
.attention h2 small{font-weight:400;color:var(--dim);font-size:11px}
.attention .diag{margin:11px 0 0;padding:11px 13px;background:var(--well);border:1px solid var(--line);border-radius:10px;white-space:pre-wrap;overflow-wrap:anywhere;max-height:34vh;overflow:auto}
main:has(.source:not([hidden])) .attention .diag{max-height:88px}
.acts{display:flex;flex-wrap:wrap;gap:8px;align-items:center;margin-top:11px}.acts:empty{display:none}
.why{flex:1 1 100%;color:var(--dim);font-size:12px}
.receipt{color:var(--dim);font-size:12px}.receipt:empty{display:none}.receipt[data-status=failed]{color:var(--red)}.receipt[data-status=completed]{color:var(--ok)}
.source{flex:none;display:flex;flex-direction:column;border-bottom:1px solid var(--line);max-height:min(62vh,540px)}
.source[hidden]{display:none}
.source header{display:flex;align-items:center;gap:8px;padding:9px 18px;flex-wrap:wrap}
.source select{background:var(--raised);border:1px solid var(--line);border-radius:8px;padding:5px 8px;color:var(--ink);max-width:60%;font-family:var(--mono);font-size:12px}
.source .rev{color:var(--dim);font-size:11px;font-family:var(--mono)}
.source textarea{flex:1 1 auto;min-height:130px;margin:0 18px;padding:11px 13px;background:var(--well);color:var(--ink);border:1px solid var(--line);border-radius:10px;resize:vertical;tab-size:4;white-space:pre;overflow:auto}
.source textarea[data-dirty=true]{border-color:var(--amber)}
.source footer{display:flex;flex-wrap:wrap;gap:8px;align-items:center;padding:9px 18px}
.source .out{margin:0 18px 11px;padding:11px 13px;background:var(--well);border:1px solid var(--line);border-radius:10px;white-space:pre-wrap;overflow-wrap:anywhere;max-height:26vh;overflow:auto}
.source .out[hidden]{display:none}.source .out[data-ok=true]{border-color:var(--ok)}.source .out[data-ok=false]{border-color:var(--red)}
.result{color:var(--dim);font-size:12px;min-width:0;overflow-wrap:anywhere}.result:empty{display:none}.result[data-tone=bad]{color:var(--red)}.result[data-tone=good]{color:var(--ok)}
.body{flex:1;min-height:0;display:grid;grid-template-columns:minmax(0,1fr)}
[data-placement=workbench] .body{grid-template-columns:minmax(0,1fr) minmax(300px,.78fr)}
.facts,.inspector{min-height:0;overflow:auto}
.facts{padding:9px 0 28px}
.selected{margin:8px 18px 14px;padding:12px 14px;background:var(--raised);border:1px solid var(--line);border-radius:12px}
.selected .eyebrow{margin-bottom:5px;color:var(--dim);font-size:10px;font-weight:600;letter-spacing:.08em;text-transform:uppercase}
.crumbs{display:flex;flex-wrap:wrap;gap:6px;align-items:center;font-size:11px;color:var(--dim)}.crumbs button{color:var(--dim)}.crumbs button:hover{color:var(--ink)}
.selected h3{margin:5px 0 7px;font-size:13px;font-weight:600;overflow-wrap:anywhere}
.kv{display:grid;grid-template-columns:auto minmax(0,1fr);gap:4px 12px;margin:0;font-size:12px;color:var(--dim)}.kv dt,.kv dd{margin:0}.kv dd{color:var(--ink);font-family:var(--mono);white-space:pre-wrap;overflow-wrap:anywhere}
.group{margin:0 18px 8px;border:1px solid var(--line);border-radius:12px;background:rgba(23,25,29,.6);overflow:hidden}
.group>summary{display:flex;align-items:center;gap:10px;min-height:46px;padding:10px 13px;cursor:pointer;list-style:none}
.group>summary::-webkit-details-marker{display:none}
.group>summary::before{content:"";width:0;height:0;border:4px solid transparent;border-left:5px solid var(--dim);flex:none;transform-origin:2px 50%}
.group[open]>summary::before{transform:rotate(90deg)}
.group>summary:hover{background:var(--raised)}
.group[data-active=true]>summary{border-bottom:1px solid var(--line)}
.group-label{font-size:13px;font-weight:600}.group-meta{margin-left:auto;color:var(--dim);font-size:11px;white-space:nowrap}
.group-body{padding:5px 5px 8px}
.panel{margin:0 2px}.panel+ .panel{border-top:1px solid rgba(48,52,60,.6)}
.panel>header{display:flex;align-items:center;gap:8px;padding-right:7px}
.panel h2{margin:0;flex:1;min-width:0;font:inherit}
.toggle{display:flex;align-items:center;gap:8px;width:100%;padding:8px;border-radius:8px;font-weight:500}.toggle:hover{background:var(--raised)}
.chev{width:0;height:0;border:4px solid transparent;border-left:5px solid var(--dim);flex:none}
.panel[data-open=true] .chev{transform:rotate(90deg)}
.panel small{color:var(--dim);font-size:11px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:45%}
.nodes{list-style:none;margin:0 0 5px;padding:0 0 0 14px}
.node{display:flex;align-items:baseline;gap:8px;width:100%;padding:6px 8px;border-radius:8px;min-width:0}.node:hover{background:var(--raised)}
li[data-selected=true]>.node{background:var(--raised);box-shadow:inset 2px 0 0 var(--red)}
.node .lbl{overflow-wrap:anywhere;min-width:0;font-family:var(--mono);font-size:12px}.node b{font-weight:500;color:var(--dim);font-size:11px;margin-left:auto;white-space:nowrap}
.fold{margin:11px 18px}.fold>summary{list-style:none;cursor:pointer;color:var(--dim);font-size:12px;padding:7px 0}.fold>summary::-webkit-details-marker{display:none}.fold>summary:hover{color:var(--ink)}
.fold .u{display:flex;gap:8px;padding:4px 0;font-size:12px}.fold .u small{color:var(--dim)}
.group-body .fold{margin:6px 10px;padding-left:12px;border-left:1px solid var(--line)}
.group-body .u{flex-wrap:wrap;column-gap:10px}
.inspector{border-left:1px solid var(--line);padding:17px 18px}
.inspector h3{margin:6px 0 9px;font-size:14px;font-weight:600;overflow-wrap:anywhere}
.value{display:block;margin:7px 0;padding:9px 11px;background:var(--well);border:1px solid var(--line);border-radius:8px;white-space:pre-wrap;overflow-wrap:anywhere;max-height:240px;overflow:auto}
.empty{padding:32px 18px;color:var(--dim);text-align:center;font-size:12px}
@media(max-width:900px){[data-placement=workbench] .body{grid-template-columns:minmax(0,1fr);grid-template-rows:minmax(0,1fr) auto}.inspector{border-left:0;border-top:1px solid var(--line);max-height:40vh}}
@media(max-width:560px){.bar,.attention,.source header,.source footer{padding-left:14px;padding-right:14px}.selected,.group,.fold{margin-left:14px;margin-right:14px}.group>summary{padding-left:11px;padding-right:11px}.group-meta{font-size:10px}}
@media(prefers-reduced-motion:no-preference){.chev,.group>summary::before{transition:transform .12s}.selected,.source{animation:in .18s ease}@keyframes in{from{opacity:0;transform:translateY(3px)}to{opacity:1;transform:none}}}
"#;

/// Page script: real routes only.
///
/// Layout and selection post to the host, then reload with focus and scroll
/// restored; a 1s poll reloads when the shared sequence, state, selection, or
/// command receipts changed elsewhere.  The source tool reads and edits
/// through the existing Canvas endpoints (`/__jet_canvas/project`, `/source`,
/// `/command` checked preview, `/transaction` revision-bound apply); its draft
/// lives in `sessionStorage`, so polling never wipes an edit in progress and a
/// dirty editor defers the reload instead.  Rebuild queues one canonical
/// `ProjectRebuild` command and then only reports what the host's receipts
/// say.  Embedded in the in-app lens, Escape and Alt+J are forwarded to the
/// parent so the return path works from inside the frame.
const PAGE_JS: &str = r#"const session=new URL(location.href).searchParams.get('session')||'';const params=new URL(location.href).searchParams;const q=(p)=>p+'?session='+encodeURIComponent(session);const key='jet-devtools:'+S.placement;const DRAFT='jet-devtools:draft';const OPEN='jet-devtools:source';const $=(s)=>document.querySelector(s);const note=$('.note');const embedded=window.parent!==window;$('main').classList.toggle('embedded',embedded);document.querySelectorAll('a[data-route]').forEach((a)=>{a.href=q(a.dataset.route);a.hidden=embedded});
const store={get(k){try{return JSON.parse(sessionStorage.getItem(k)||'null')}catch(_){return null}},set(k,v){try{if(v==null)sessionStorage.removeItem(k);else sessionStorage.setItem(k,JSON.stringify(v))}catch(_){}}};
const intentKey='jet-devtools:intents:'+S.placement;const restoreIntentGroups=()=>{const saved=store.get(intentKey)||{};document.querySelectorAll('details[data-intent]').forEach((group)=>{if(group.dataset.active!=='true'&&Object.prototype.hasOwnProperty.call(saved,group.dataset.intent))group.open=!!saved[group.dataset.intent];group.addEventListener('toggle',()=>{const next=store.get(intentKey)||{};next[group.dataset.intent]=group.open;store.set(intentKey,next)})})};restoreIntentGroups();
const focusKey=()=>document.activeElement&&document.activeElement.dataset.focus||null;const scrollers=()=>[...document.querySelectorAll('[data-scroll]')];
const reload=(focus)=>{store.set(key,{focus,scroll:scrollers().map((e)=>e.scrollTop)});location.reload()};
{const m=store.get(key);if(m){store.set(key,null);scrollers().forEach((e,i)=>{if(m.scroll[i]!=null)e.scrollTop=m.scroll[i]});const f=[...document.querySelectorAll('[data-focus]')].find((e)=>e.dataset.focus===m.focus);if(f)f.focus({preventScroll:true})}}
const fail=(r)=>r.text().then((t)=>{throw new Error(t||('HTTP '+r.status))});
const post=(path,body)=>fetch(q(path),{method:'POST',cache:'no-store',headers:{'content-type':'application/json'},body:JSON.stringify(body)});
const send=(path,body,focus)=>post(path,body).then((r)=>r.ok?r.json():fail(r)).then(()=>reload(focus)).catch((e)=>{note.textContent=String(e.message||e)});
document.addEventListener('click',(ev)=>{const el=ev.target.closest('[data-select],[data-layout],[data-clear],[data-tool],[data-refresh]');if(!el)return;if(el.dataset.select)send('/__jet_devtools/selection',{selection:{panel_id:el.dataset.panel,item_key:el.dataset.node}},el.dataset.focus);else if(el.dataset.layout)send('/__jet_devtools/action',{action:el.dataset.layout==='open'?'open_panel':'close_panel',placement:S.placement,panel_id:el.dataset.panel},el.dataset.focus);else if(el.dataset.clear)send('/__jet_devtools/selection',{selection:null},null);else if(el.dataset.refresh)reload(focusKey());else if(el.dataset.tool==='source')openSource(el.dataset.file||null);else if(el.dataset.tool==='rebuild')rebuild(el)});
/* Rebuild: one canonical host_to_runtime command; the receipt the host publishes is the only progress shown. */
const receiptEl=$('.receipt');const rebuildResult=$('.acts .result');
function rebuild(button){button.disabled=true;rebuildResult.dataset.tone='';rebuildResult.textContent='Requesting rebuild…';const id='rebuild-'+Date.now().toString(36)+'-'+Math.random().toString(36).slice(2,8);post('/__jet_devtools',{protocol:'jet.devtools.v1',session_id:S.session_id,started_at_ms:Date.now(),direction:'host_to_runtime',commands:[{kind:'ProjectRebuild',payload:{session_id:S.session_id,request_id:id,expected_revision:S.revision}}]}).then((r)=>{if(!r.ok)return fail(r);rebuildResult.textContent='';return live(true)}).catch((e)=>{rebuildResult.dataset.tone='bad';rebuildResult.textContent=String(e.message||e)}).finally(()=>{button.disabled=false})}
const receiptText={queued:'Rebuild queued',building:'Rebuilding…',completed:'Rebuild finished',failed:'Rebuild failed'};
function renderReceipt(list){const r=(list||[]).filter((x)=>x.kind==='ProjectRebuild').pop();if(!receiptEl)return;if(!r){receiptEl.textContent='';receiptEl.removeAttribute('data-status');return}receiptEl.dataset.status=r.status;receiptEl.textContent=(receiptText[r.status]||r.status)+(r.error?': '+r.error:'')}
renderReceipt(S.receipts);
/* Source tool */
const src={box:$('.source'),file:$('.source select'),rev:$('.source .rev'),text:$('.source textarea'),out:$('.source .out'),result:$('.source .result'),apply:$('[data-src=apply]'),check:$('[data-src=check]'),discard:$('[data-src=discard]'),files:[],source_id:null,revision:null,base:'',loading:false};
const same=(a,b)=>JSON.stringify(a)===JSON.stringify(b);const dirty=()=>!!src.box&&!src.box.hidden&&src.text.value!==src.base;
function say(el,tone,text){el.dataset.tone=tone||'';el.textContent=text||''}
function showOut(ok,text){src.out.hidden=!text;src.out.dataset.ok=String(ok);src.out.textContent=text||''}
function fileFromDiagnostic(){const m=/-->\s*([^\s:]+\.jet)/.exec(S.status.diagnostic||'');return m?m[1]:null}
function pickFile(wanted){if(!src.files.length)return null;const norm=(p)=>String(p||'').replace(/^\.\//,'');if(wanted){const w=norm(wanted);const hit=src.files.find((f)=>norm(f)===w)||src.files.find((f)=>w.endsWith('/'+norm(f))||norm(f).endsWith('/'+w)||norm(f).split('/').pop()===w.split('/').pop());if(hit)return hit}return src.files.includes(src.entry)?src.entry:src.files[0]}
function openSource(file){if(!src.box)return;src.box.hidden=false;const draft=store.get(DRAFT);const want=file||(draft&&draft.source_id)||fileFromDiagnostic()||null;loadProject().then(()=>loadFile(pickFile(want),draft)).catch((e)=>say(src.result,'bad',String(e.message||e)))}
function loadProject(){return fetch(q('/__jet_canvas/project'),{cache:'no-store'}).then((r)=>r.ok?r.json():fail(r)).then((j)=>{const c=j.canvas||{};src.entry=String(c.entry||'').replace(/^\.\//,'');src.revisions={};src.files=(c.files||[]).filter((f)=>f.kind==='source').map((f)=>{const p=String(f.path).replace(/^\.\//,'');src.revisions[p]=f.revision;return p});src.file.innerHTML='';src.files.forEach((p)=>{const o=document.createElement('option');o.value=p;o.textContent=p;src.file.appendChild(o)});if(src.source_id)src.file.value=src.source_id})}
function loadFile(id,draft){if(!id)throw new Error('This project has no Jet source files to edit.');src.loading=true;src.file.value=id;return fetch(q('/__jet_canvas/source?source_id='+encodeURIComponent(id)),{cache:'no-store'}).then((r)=>r.ok?r.text():fail(r)).then((text)=>{src.source_id=id;src.revision=src.revisions[id]||null;src.base=text;src.rev.textContent=src.revision?src.revision.slice(0,15):'';showOut(true,'');say(src.result,'','');if(draft&&draft.source_id===id&&draft.text!==text){src.text.value=draft.text;if(draft.revision!==src.revision)say(src.result,'','The file changed on disk since this draft started; Apply checks against the current file.')}else{src.text.value=text;if(draft&&draft.source_id===id)store.set(DRAFT,null)}markDirty();store.set(OPEN,{source_id:id});if(draft&&draft.sel&&draft.source_id===id){try{src.text.setSelectionRange(draft.sel[0],draft.sel[1]);src.text.scrollTop=draft.scroll||0}catch(_){}}}).finally(()=>{src.loading=false})}
function markDirty(){const d=dirty();src.text.dataset.dirty=String(d);src.apply.disabled=!d;src.discard.hidden=!d;if(d)store.set(DRAFT,{source_id:src.source_id,revision:src.revision,text:src.text.value,sel:[src.text.selectionStart,src.text.selectionEnd],scroll:src.text.scrollTop});else if(!src.loading)store.set(DRAFT,null)}
function readCanvas(r){return r.text().then((t)=>{let j=null;try{j=JSON.parse(t)}catch(_){}if(!j)throw new Error(t||('HTTP '+r.status));return j.canvas||j})}
function diagText(c){const lines=[];if(c.message)lines.push(c.message);(c.diagnostics||[]).forEach((d)=>{if(typeof d==='string'){lines.push(d);return}const where=d.file?(d.file+(d.line?':'+d.line+(d.col?':'+d.col:''):'')):'';lines.push([d.code?'['+d.code+']':'',d.what||d.message||'',where?'— '+where:''].filter(Boolean).join(' '));if(d.fix)lines.push('  fix: '+d.fix)});return lines.join('\n')}
function conflict(c){src.revisions={};return loadProject().then(()=>{src.revision=src.revisions[src.source_id]||c.current_revision||src.revision;src.rev.textContent=src.revision?src.revision.slice(0,15):'';say(src.result,'bad',(c.message||'The file changed on disk.')+' Your draft is kept; Check or Apply again runs against the current file, or Discard reloads it.')})}
if(src.box){src.text.addEventListener('input',markDirty);src.text.addEventListener('select',markDirty);src.text.addEventListener('scroll',()=>{if(dirty())markDirty()});
src.file.addEventListener('change',()=>{if(dirty()){src.file.value=src.source_id;say(src.result,'','Apply or discard the current draft before switching files.');return}loadFile(src.file.value,null).catch((e)=>say(src.result,'bad',String(e.message||e)))});
src.discard.addEventListener('click',()=>{store.set(DRAFT,null);loadFile(src.source_id,null).catch((e)=>say(src.result,'bad',String(e.message||e)))});
$('[data-src=close]').addEventListener('click',()=>{if(dirty()&&!confirm('Close the source tool and keep the draft for later?'))return;src.box.hidden=true;store.set(OPEN,null);const u=new URL(location.href);u.searchParams.delete('tool');u.searchParams.delete('file');history.replaceState(null,'',u)});
src.check.addEventListener('click',()=>{src.check.disabled=true;say(src.result,'','Checking…');showOut(true,'');post('/__jet_canvas/command',{action_id:'canvas.command:check',source_id:src.source_id,revision:src.revision,source_text:src.text.value}).then((r)=>readCanvas(r).then((c)=>{if(c.ok===false&&c.kind==='conflict')return conflict(c);if(c.ok===false)throw new Error(c.message||'check refused');if(c.success){say(src.result,'good','No errors — Apply writes this exact text, formatted.')}else{say(src.result,'bad','Check found errors; nothing was written.');showOut(false,c.stderr||diagText(c))}})).catch((e)=>say(src.result,'bad',String(e.message||e))).finally(()=>{src.check.disabled=false})});
src.apply.addEventListener('click',()=>{src.apply.disabled=true;say(src.result,'','Applying…');showOut(true,'');post('/__jet_canvas/transaction',{schema_version:1,op:'replace_source',source_id:src.source_id,revision:src.revision,source:src.text.value}).then((r)=>readCanvas(r).then((c)=>{if(c.ok===false&&c.kind==='conflict')return conflict(c);if(c.ok===false){say(src.result,'bad','Not applied'+(c.message?': '+c.message:'.'));if((c.diagnostics||[]).length)showOut(false,diagText(c));return}src.base=c.source_text!=null?c.source_text:src.text.value;src.text.value=src.base;src.revision=c.revision||src.revision;src.revisions[src.source_id]=src.revision;src.rev.textContent=src.revision?src.revision.slice(0,15):'';store.set(DRAFT,null);markDirty();say(src.result,'good',c.changed===false?'No change to write.':'Applied — the host is rebuilding from this revision.')})).catch((e)=>say(src.result,'bad',String(e.message||e))).finally(()=>{markDirty()})});
const open=store.get(OPEN);const draft=store.get(DRAFT);if(params.get('tool')==='source'||open||draft)openSource(params.get('file')||(open&&open.source_id)||null)}
/* Live: reload when shared facts move, unless a draft is in progress; then only the receipt updates and a refresh chip appears. */
const refresh=$('[data-refresh]');
function live(once){return fetch(q('/__jet_devtools/state'),{cache:'no-store'}).then((r)=>r.json()).then((n)=>{const moved=n.sequence!==S.sequence||n.state!==S.state||n.status.state!==S.status.state||!same(n.selection,S.selection)||!same(n.receipts,S.receipts)||!same(n.tools,S.tools);if(!moved)return;if(dirty()||document.activeElement===src.text){renderReceipt(n.receipts);if(refresh)refresh.hidden=false;return}reload(focusKey())}).catch(()=>{}).finally(()=>{if(!once)setTimeout(live,1000)})}
setTimeout(live,1000);
const sel=S.selection&&S.panels.find((p)=>p.panel_id===S.selection.panel_id);if(sel&&!sel.open){const once='jet-devtools:reveal:'+S.placement+':'+sel.panel_id;let done=false;try{done=sessionStorage.getItem(once)===String(S.sequence);sessionStorage.setItem(once,String(S.sequence))}catch(_){}if(!done)send('/__jet_devtools/action',{action:'open_panel',placement:S.placement,panel_id:sel.panel_id},focusKey())}
addEventListener('keydown',(ev)=>{if(ev.key==='Escape'&&document.activeElement===src.text){src.text.blur();ev.preventDefault();return}if(!embedded)return;const alt=ev.altKey&&!ev.ctrlKey&&!ev.metaKey&&ev.key.toLowerCase()==='j';if(ev.key==='Escape'||alt){ev.preventDefault();parent.postMessage({jet:'devtools',key:ev.key,altKey:ev.altKey},location.origin)}});"#;

/// Panels grouped by the job a developer is trying to finish.  The page keeps
/// these groups collapsed until a selection, open panel, or active failure
/// makes one relevant.  Unknown panel ids stay in the final group.
const INTENT_GROUPS: &[(&str, &[&str])] = &[
    (
        "Build & source",
        &["build", "gates", "structure", "tests", "jobs"],
    ),
    (
        "Data & requests",
        &[
            "routes",
            "queries",
            "mutations",
            "forms",
            "table",
            "store",
            "database",
        ],
    ),
    (
        "Runtime & interface",
        &[
            "traces", "cost", "profiler", "ui-tree", "topology", "game", "world",
        ],
    ),
];

fn intent_group(panel_id: &str) -> usize {
    INTENT_GROUPS
        .iter()
        .position(|(_, ids)| ids.contains(&panel_id))
        .unwrap_or(INTENT_GROUPS.len() - 1)
}

fn panel_available(panel: &BrowserPanelView) -> bool {
    !panel.status.starts_with("unavailable")
}

/// Human wording for a canonical unavailability reason id; unknown ids stay
/// verbatim so the exact reason is never lost.
fn unavailable_reason_text(status: &str) -> String {
    status
        .trim_start_matches("unavailable:")
        .split(',')
        .map(|reason| match reason {
            "unsupported-host" | "host-kind" => "not offered by this host".to_string(),
            "missing-capability" => "needs a grant this session does not hold".to_string(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The `path` named by a diagnostic's `--> path:line:col` line, if any.
fn diagnostic_file(diagnostic: &str) -> Option<&str> {
    let rest = diagnostic.split("-->").nth(1)?.trim_start();
    let token = rest.split_whitespace().next()?;
    let file = token.split(':').next()?;
    file.ends_with(".jet").then_some(file)
}

/// Render one placement.  Both placements read the same `BrowserWebState` and
/// the same shared selection.  Lens is contextual: attention, the selected
/// fact, and collapsed intent groups.  Workbench uses the same groups but
/// opens its deeper inspector beside them.  Tools render only where the host
/// really has them; a missing tool is one sentence, not a disabled wall.
pub(crate) fn render_page(state: &BrowserWebState, placement: BrowserPlacement) -> String {
    let frame = state.frame(placement);
    let state_json = frame_json(&frame);
    let workbench = placement == BrowserPlacement::Workbench;
    let selection = frame.selection.as_ref();
    let any_open = frame
        .panels
        .iter()
        .any(|panel| panel.open && panel_available(panel));
    let mut html = String::with_capacity(16 * 1024);
    html.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; connect-src 'self'; style-src 'unsafe-inline'; script-src 'unsafe-inline'\"><title>Jet devtools");
    if workbench {
        html.push_str(" workbench");
    }
    html.push_str("</title><style>");
    html.push_str(PAGE_CSS);
    html.push_str("</style></head><body><main data-schema=\"");
    html.push_str(BROWSER_PAGE_SCHEMA);
    html.push_str("\" data-placement=\"");
    html.push_str(placement.as_str());
    html.push_str("\" data-session-id=\"");
    html.push_str(&html_escape(&frame.session_id));
    html.push_str("\" data-state=\"");
    let state_word = devtools_state_as_str(&frame.status.state);
    html.push_str(state_word);
    html.push_str("\"><header class=\"bar\"><span class=\"mark\">");
    html.push_str(include_str!("../../../site/assets/mark.svg"));
    html.push_str("</span><strong>Jet devtools</strong><span class=\"state\"><i class=\"status-mark\" aria-hidden=\"true\"></i>");
    html.push_str(state_word);
    html.push_str(
        "</span><span class=\"grow\"></span><span class=\"note\" role=\"status\"></span>",
    );
    if !workbench {
        html.push_str("<a class=\"link\" data-route=\"/__jet_devtools/workbench\">Workbench</a>");
    }
    html.push_str("</header>");
    render_attention(&mut html, &frame);
    render_source_tool(&mut html, &frame);
    html.push_str("<div class=\"body\">");
    html.push_str("<section class=\"facts\" data-scroll aria-label=\"Facts\">");
    if !workbench {
        render_selected(&mut html, &frame);
    }
    for (index, (title, _)) in INTENT_GROUPS.iter().enumerate() {
        let members = frame
            .panels
            .iter()
            .filter(|panel| intent_group(&panel.panel_id) == index)
            .collect::<Vec<_>>();
        if members.is_empty() {
            continue;
        }
        let available_count = members
            .iter()
            .filter(|panel| panel_available(panel))
            .count();
        let open_count = members
            .iter()
            .filter(|panel| panel.open && panel_available(panel))
            .count();
        let selected_here = selection.is_some_and(|selected| {
            members
                .iter()
                .any(|panel| panel.panel_id == selected.panel_id)
        });
        let forced = selected_here
            || open_count > 0
            || (!workbench
                && index == 0
                && matches!(
                    frame.status.state,
                    DevtoolsSessionState::Building | DevtoolsSessionState::Error
                ));
        let initially_open =
            forced || (workbench && index == 0 && !any_open && selection.is_none());
        html.push_str("<details class=\"group\" data-intent=\"");
        let _ = write!(html, "{}", index);
        if forced {
            html.push_str("\" data-active=\"true\"");
        } else {
            html.push_str("\"");
        }
        if initially_open {
            html.push_str(" open");
        }
        html.push_str("><summary data-focus=\"intent:");
        let _ = write!(html, "{}\"><span class=\"group-label\">", index);
        html.push_str(&html_escape(title));
        html.push_str("</span><span class=\"group-meta\">");
        if selected_here {
            html.push_str("Selected");
        } else if open_count > 0 {
            let _ = write!(
                html,
                "{} open · {} panel{}",
                open_count,
                available_count,
                if available_count == 1 { "" } else { "s" }
            );
        } else {
            let _ = write!(html, "{} available", available_count,);
        }
        html.push_str("</span></summary><div class=\"group-body\">");
        for panel in members.iter().filter(|panel| panel_available(panel)) {
            render_panel(&mut html, panel, selection);
        }
        let unavailable_count = members.len() - available_count;
        if unavailable_count > 0 {
            let _ = write!(
                html,
                "<details class=\"fold\"><summary>{} not available here</summary>",
                unavailable_count,
            );
            for panel in members.iter().filter(|panel| !panel_available(panel)) {
                html.push_str("<div class=\"u\"><span>");
                html.push_str(&html_escape(&panel.title));
                html.push_str("</span><small>");
                html.push_str(&html_escape(&unavailable_reason_text(&panel.status)));
                html.push_str("</small></div>");
            }
            html.push_str("</details>");
        }
        html.push_str("</div></details>");
    }
    if frame.panels.is_empty() {
        html.push_str("<p class=\"empty\">Waiting for the first devtools fact.</p>");
    } else if frame.panels.iter().all(|panel| !panel_available(panel)) {
        html.push_str("<p class=\"empty\">No live panels are available on this host.</p>");
    }
    html.push_str("</section>");
    if workbench {
        render_inspector(&mut html, &frame);
    }
    html.push_str("</div><script>const S=JSON.parse(\"");
    html.push_str(&js_string(&state_json));
    html.push_str("\");");
    html.push_str(PAGE_JS);
    html.push_str("</script></main></body></html>");
    html
}

/// What needs attention right now, with the real actions next to it.
fn render_attention(html: &mut String, frame: &BrowserDevtoolsFrame) {
    let status = &frame.status;
    let failing_file = status.diagnostic.as_deref().and_then(diagnostic_file);
    html.push_str("<section class=\"attention\" aria-label=\"Attention\"><h2><i class=\"status-mark\" aria-hidden=\"true\"></i>");
    match status.state {
        DevtoolsSessionState::Starting => html.push_str("Starting"),
        DevtoolsSessionState::Building => html.push_str("Building"),
        DevtoolsSessionState::Ready => html.push_str("Ready"),
        DevtoolsSessionState::Error => html.push_str("Build failed"),
        DevtoolsSessionState::Unavailable => html.push_str("Runtime unavailable"),
        DevtoolsSessionState::Stopped => html.push_str("Stopped"),
    }
    if let Some(code) = &status.diagnostic_code {
        html.push_str("<small class=\"mono\">");
        html.push_str(&html_escape(code));
        html.push_str("</small>");
    }
    if let Some(file) = failing_file {
        html.push_str("<small class=\"mono\">");
        html.push_str(&html_escape(file));
        html.push_str("</small>");
    }
    if let Some(tests) = &status.test_state {
        html.push_str("<small>tests ");
        html.push_str(&html_escape(tests));
        html.push_str("</small>");
    }
    html.push_str("</h2>");
    if let Some(diagnostic) = &status.diagnostic {
        // Verbatim diagnostic (I4): escaped for HTML, never reworded.
        html.push_str("<pre class=\"diag\">");
        html.push_str(&html_escape(diagnostic));
        html.push_str("</pre>");
    }
    html.push_str("<div class=\"acts\">");
    if frame.tools.source {
        if status.state == DevtoolsSessionState::Error {
            html.push_str("<button class=\"btn primary\" type=\"button\" data-tool=\"source\" data-focus=\"tool:source\"");
            if let Some(file) = failing_file {
                html.push_str(" data-file=\"");
                html.push_str(&html_escape(file));
                html.push('"');
            }
            html.push_str(">Fix in source</button>");
        } else {
            html.push_str("<button class=\"btn\" type=\"button\" data-tool=\"source\" data-focus=\"tool:source\">Source</button>");
        }
    }
    if frame.tools.rebuild {
        html.push_str("<button class=\"btn\" type=\"button\" data-tool=\"rebuild\" data-focus=\"tool:rebuild\">Rebuild</button>");
    }
    html.push_str("<span class=\"receipt\" role=\"status\"></span><span class=\"result\" role=\"status\"></span>");
    let mut reasons = Vec::new();
    if let Some(reason) = frame.tools.source_reason.as_deref() {
        reasons.push(reason);
    }
    if !frame.tools.rebuild {
        if let Some(reason) = frame.tools.rebuild_reason.as_deref() {
            reasons.push(reason);
        }
    }
    if !reasons.is_empty() {
        html.push_str("<span class=\"why\">");
        html.push_str(&html_escape(&reasons.join(" ")));
        html.push_str("</span>");
    }
    html.push_str("</div></section>");
}

/// The source tool: rendered only where Canvas source authority exists.  It
/// is hidden until opened; the script fills the file list from the project.
fn render_source_tool(html: &mut String, frame: &BrowserDevtoolsFrame) {
    if !frame.tools.source {
        return;
    }
    html.push_str(
        "<section class=\"source\" aria-label=\"Source\" hidden><header><select aria-label=\"Source file\"></select><span class=\"rev\"></span><span class=\"grow\"></span><button class=\"btn quiet\" type=\"button\" data-refresh hidden>Facts changed — refresh</button><button class=\"btn quiet\" type=\"button\" data-src=\"close\">Close</button></header><textarea spellcheck=\"false\" autocapitalize=\"off\" autocomplete=\"off\" aria-label=\"Source text\" data-focus=\"tool:source-text\"></textarea><footer><button class=\"btn\" type=\"button\" data-src=\"check\">Check</button><button class=\"btn primary\" type=\"button\" data-src=\"apply\" disabled>Apply</button><button class=\"btn quiet\" type=\"button\" data-src=\"discard\" hidden>Discard draft</button><span class=\"result\" role=\"status\"></span></footer><pre class=\"out\" hidden></pre></section>",
    );
}

/// One panel: a header that toggles the host's open state, and (when open)
/// its typed rows.  Rows show the entity and the event kind; the row's raw
/// fields are expert detail shown for the selected row only.
fn render_panel(html: &mut String, panel: &BrowserPanelView, selection: Option<&BrowserSelection>) {
    let node_id = selection
        .filter(|selection| selection.panel_id == panel.panel_id)
        .and_then(|selection| selection.node_id.as_deref());
    html.push_str("<article class=\"panel\" data-panel-id=\"");
    html.push_str(&html_escape(&panel.panel_id));
    html.push_str("\" data-open=\"");
    html.push_str(if panel.open { "true" } else { "false" });
    html.push_str("\"><header><h2><button class=\"toggle\" data-layout=\"");
    html.push_str(if panel.open { "close" } else { "open" });
    html.push_str("\" data-panel=\"");
    html.push_str(&html_escape(&panel.panel_id));
    html.push_str("\" data-focus=\"toggle:");
    html.push_str(&html_escape(&panel.panel_id));
    html.push_str("\" aria-expanded=\"");
    html.push_str(if panel.open { "true" } else { "false" });
    html.push_str("\"><span class=\"chev\"></span>");
    html.push_str(&html_escape(&panel.title));
    html.push_str("</button></h2><small>");
    html.push_str(&html_escape(&panel.status));
    html.push_str("</small></header>");
    if panel.open {
        html.push_str("<ul class=\"nodes\">");
        render_nodes_html(html, &panel.panel_id, &panel.nodes, node_id);
        html.push_str("</ul>");
    }
    html.push_str("</article>");
}

/// Lens: the shared selection as a compact context card above the panels.
fn render_selected(html: &mut String, frame: &BrowserDevtoolsFrame) {
    let Some(selection) = &frame.selection else {
        return;
    };
    let Some(panel) = frame
        .panels
        .iter()
        .find(|panel| panel.panel_id == selection.panel_id)
    else {
        return;
    };
    let mut path = Vec::new();
    let node = selection
        .node_id
        .as_deref()
        .filter(|node_id| find_path(&panel.nodes, node_id, &mut path))
        .and_then(|_| path.last().copied());
    html.push_str("<section class=\"selected\" aria-label=\"Selected\"><div class=\"eyebrow\">Selected fact</div>");
    render_crumbs(html, panel, &path);
    match node {
        Some(node) => render_node_detail(html, &panel.panel_id, node),
        None => {
            html.push_str("<h3>");
            html.push_str(&html_escape(&panel.title));
            html.push_str("</h3>");
        }
    }
    html.push_str("</section>");
}

fn render_crumbs(html: &mut String, panel: &BrowserPanelView, path: &[&BrowserNode]) {
    html.push_str("<div class=\"crumbs\"><span>");
    html.push_str(&html_escape(&panel.title));
    html.push_str("</span>");
    for ancestor in path.iter().take(path.len().saturating_sub(1)) {
        html.push_str("<span>›</span><button data-select=\"1\" data-panel=\"");
        html.push_str(&html_escape(&panel.panel_id));
        html.push_str("\" data-node=\"");
        html.push_str(&html_escape(&ancestor.id));
        html.push_str("\">");
        html.push_str(&html_escape(&ancestor.label));
        html.push_str("</button>");
    }
    html.push_str("<span class=\"grow\"></span><button class=\"btn quiet\" type=\"button\" data-clear=\"1\" data-focus=\"clear\">Clear</button></div>");
}

/// The selected node's facts: its own value, then its children as a key/value
/// sheet (an event's source/kind/entity/fields), deeper trees as rows.
fn render_node_detail(html: &mut String, panel_id: &str, node: &BrowserNode) {
    html.push_str("<h3>");
    html.push_str(&html_escape(&node.label));
    if let Some(status) = &node.status {
        html.push_str(" <small class=\"mono\">");
        html.push_str(&html_escape(status));
        html.push_str("</small>");
    }
    html.push_str("</h3>");
    if let Some(value) = &node.value {
        html.push_str("<code class=\"value\">");
        html.push_str(&html_escape(value));
        html.push_str("</code>");
    }
    let (leaves, trees): (Vec<_>, Vec<_>) = node
        .children
        .iter()
        .partition(|child| child.children.is_empty() && child.value.is_some());
    if !leaves.is_empty() {
        html.push_str("<dl class=\"kv\">");
        for leaf in leaves {
            html.push_str("<dt>");
            html.push_str(&html_escape(&leaf.label));
            html.push_str("</dt><dd>");
            html.push_str(&html_escape(leaf.value.as_deref().unwrap_or("")));
            html.push_str("</dd>");
        }
        html.push_str("</dl>");
    }
    if !trees.is_empty() {
        html.push_str("<ul class=\"nodes\">");
        let owned = trees.into_iter().cloned().collect::<Vec<_>>();
        render_nodes_html(html, panel_id, &owned, None);
        html.push_str("</ul>");
    }
}

/// Workbench inspector: the shared selection, with its ancestors as a
/// return path and its facts as a sheet.
fn render_inspector(html: &mut String, frame: &BrowserDevtoolsFrame) {
    html.push_str("<aside class=\"inspector\" data-scroll aria-label=\"Inspector\">");
    let Some(selection) = &frame.selection else {
        html.push_str("<p class=\"empty\">Select a fact to inspect it.</p></aside>");
        return;
    };
    let Some(panel) = frame
        .panels
        .iter()
        .find(|panel| panel.panel_id == selection.panel_id)
    else {
        html.push_str(
            "<p class=\"empty\">Selected panel is not in the current catalog.</p></aside>",
        );
        return;
    };
    let mut path = Vec::new();
    let node = selection
        .node_id
        .as_deref()
        .filter(|node_id| find_path(&panel.nodes, node_id, &mut path))
        .and_then(|_| path.last().copied());
    render_crumbs(html, panel, &path);
    match node {
        Some(node) => render_node_detail(html, &panel.panel_id, node),
        None => {
            html.push_str("<h3>");
            html.push_str(&html_escape(&panel.title));
            html.push_str("</h3><dl class=\"kv\"><dt>status</dt><dd>");
            html.push_str(&html_escape(&panel.status));
            let _ = write!(
                html,
                "</dd><dt>facts</dt><dd>{}</dd></dl>",
                panel.nodes.len()
            );
        }
    }
    html.push_str("</aside>");
}

/// Path from a panel's roots to `node_id`, inclusive.  Returns `false` and
/// leaves `path` empty when the node is not present.
fn find_path<'a>(nodes: &'a [BrowserNode], node_id: &str, path: &mut Vec<&'a BrowserNode>) -> bool {
    for node in nodes {
        path.push(node);
        if node.id == node_id || find_path(&node.children, node_id, path) {
            return true;
        }
        path.pop();
    }
    false
}

fn panel_from_projection(panel: &JetDevtoolsPanelProjection) -> BrowserPanelFact {
    let descriptor = &panel.availability.descriptor;
    let mut status = if panel.availability.is_available() {
        panel
            .events
            .last()
            .map(|event| event.kind().to_string())
            .unwrap_or_else(|| "available".to_string())
    } else {
        let reasons = panel
            .availability
            .unavailable_reasons()
            .iter()
            .map(|reason| reason.as_str())
            .collect::<Vec<_>>()
            .join(",");
        format!("unavailable:{reasons}")
    };
    status = bounded_text(&status, "unavailable", MAX_BROWSER_TEXT);

    let mut nodes = Vec::with_capacity(panel.events.len().max(1));
    for event in &panel.events {
        let prefix = format!("event:{}", event.sequence());
        let children = vec![
            BrowserNode::new(format!("{prefix}:source"), BrowserNodeKind::Text, "source")
                .with_value(bounded_value(event.source())),
            BrowserNode::new(format!("{prefix}:kind"), BrowserNodeKind::Text, "kind")
                .with_value(bounded_value(event.kind())),
            BrowserNode::new(format!("{prefix}:entity"), BrowserNodeKind::Text, "entity")
                .with_value(bounded_value(event.entity())),
            BrowserNode::new(format!("{prefix}:fields"), BrowserNodeKind::Text, "fields")
                .with_value(bounded_value(event.fields_json())),
            BrowserNode::new(
                format!("{prefix}:payload"),
                BrowserNodeKind::Text,
                "payload",
            )
            .with_value(bounded_value(event.payload_json().unwrap_or("(absent)"))),
        ];
        let label = bounded_text(event.entity(), event.kind(), MAX_BROWSER_TEXT);
        // The row shows entity and kind; source/fields/payload stay behind
        // selection as expert detail.
        nodes.push(
            BrowserNode::new(
                format!("event:{}", event.sequence()),
                BrowserNodeKind::Tree,
                label,
            )
            .with_status(bounded_text(event.kind(), "event", MAX_BROWSER_TEXT))
            .with_children(children),
        );
    }
    if nodes.is_empty() {
        nodes.push(
            BrowserNode::new("availability", BrowserNodeKind::Status, "availability")
                .with_status(status.clone()),
        );
    }
    BrowserPanelFact::new(descriptor.id.as_str(), descriptor.title, 0, status, nodes)
        .with_cursor(panel.cursor)
}

fn parse_revision(value: &str) -> Option<u64> {
    value.parse::<u64>().ok()
}

fn bounded_text(value: &str, fallback: &str, limit: usize) -> String {
    let source = if value.is_empty() { fallback } else { value };
    let mut output = String::new();
    for character in source.chars() {
        let character = if character.is_control() {
            ' '
        } else {
            character
        };
        if output.len() + character.len_utf8() > limit {
            break;
        }
        output.push(character);
    }
    if output.is_empty() {
        fallback.to_string()
    } else {
        output
    }
}

fn bounded_value(value: &str) -> String {
    bounded_text(value, "(none)", MAX_BROWSER_VALUE)
}

fn frame_json(frame: &BrowserDevtoolsFrame) -> String {
    let mut output = String::with_capacity(4096);
    output.push('{');
    field_string(&mut output, "schema", BROWSER_PAGE_SCHEMA, true);
    field_string(&mut output, "protocol", &frame.protocol, false);
    field_string(&mut output, "placement", frame.placement.as_str(), false);
    field_string(&mut output, "route", &frame.route, false);
    field_string(&mut output, "session_id", &frame.session_id, false);
    field_optional_string(&mut output, "source_id", frame.source_id.as_deref());
    field_optional_string(&mut output, "build_id", frame.build_id.as_deref());
    field_string(&mut output, "revision", &frame.revision, false);
    field_optional_string(&mut output, "world_id", frame.world_id.as_deref());
    field_u64_option(&mut output, "sequence", frame.sequence);
    field_u64_option(
        &mut output,
        "cursor",
        frame.cursor.map(|cursor| cursor.sequence),
    );
    field_string(&mut output, "state", frame.state.as_str(), false);
    output.push_str(",\"status\":");
    status_json(&mut output, &frame.status);
    output.push_str(",\"tools\":{\"source\":{");
    field_bool(&mut output, "available", frame.tools.source, true);
    field_optional_string(&mut output, "reason", frame.tools.source_reason.as_deref());
    output.push_str("},\"rebuild\":{");
    field_bool(&mut output, "available", frame.tools.rebuild, true);
    field_optional_string(&mut output, "reason", frame.tools.rebuild_reason.as_deref());
    output.push_str("}},\"receipts\":[");
    for (index, receipt) in frame.receipts.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push('{');
        field_u64(&mut output, "sequence", receipt.sequence, true);
        field_string(&mut output, "request_id", &receipt.request_id, false);
        field_string(&mut output, "kind", &receipt.kind, false);
        field_string(&mut output, "status", &receipt.status, false);
        field_optional_string(&mut output, "error", receipt.error.as_deref());
        output.push('}');
    }
    output.push(']');
    output.push_str(",\"selection\":");
    match &frame.selection {
        Some(selection) => {
            output.push('{');
            field_string(&mut output, "panel_id", &selection.panel_id, true);
            output.push_str(",\"node_id\":");
            match &selection.node_id {
                Some(node_id) => quoted(node_id, &mut output),
                None => output.push_str("null"),
            }
            output.push('}');
        }
        None => output.push_str("null"),
    }
    output.push_str(",\"panels\":[");
    for (index, panel) in frame.panels.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push('{');
        field_string(&mut output, "panel_id", &panel.panel_id, true);
        field_string(&mut output, "title", &panel.title, false);
        field_u64(&mut output, "freshness_ms", panel.freshness_ms, false);
        field_string(&mut output, "status", &panel.status, false);
        field_u64_option(&mut output, "cursor", panel.cursor);
        field_bool(&mut output, "open", panel.open, false);
        field_string(&mut output, "dock", panel.dock.as_str(), false);
        output.push_str(",\"nodes\":[");
        write_nodes_json(&mut output, &panel.nodes);
        output.push_str("]}");
    }
    output.push_str("],\"elements\":[");
    for (index, element) in frame.elements.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push('{');
        field_string(&mut output, "id", &element.id, true);
        field_string(&mut output, "kind", &element.kind, false);
        field_optional_string(&mut output, "panel_id", element.panel_id.as_deref());
        field_optional_string(&mut output, "node_id", element.node_id.as_deref());
        field_string(&mut output, "label", &element.label, false);
        field_optional_string(&mut output, "value", element.value.as_deref());
        field_optional_string(&mut output, "status", element.status.as_deref());
        output.push('}');
    }
    output.push_str("]}");
    output
}

fn status_json(output: &mut String, status: &DevtoolsStatusFact) {
    output.push('{');
    field_string(output, "state", devtools_state_as_str(&status.state), true);
    field_optional_string(output, "diagnostic_code", status.diagnostic_code.as_deref());
    field_optional_string(output, "diagnostic", status.diagnostic.as_deref());
    field_optional_string(
        output,
        "accepted_revision",
        status.accepted_revision.as_deref(),
    );
    field_optional_string(
        output,
        "last_good_revision",
        status.last_good_revision.as_deref(),
    );
    field_optional_string(output, "run_target", status.run_target.as_deref());
    field_optional_string(output, "run_output", status.run_output.as_deref());
    field_optional_string(output, "test_state", status.test_state.as_deref());
    output.push('}');
}

fn devtools_state_as_str(state: &DevtoolsSessionState) -> &'static str {
    match state {
        DevtoolsSessionState::Starting => "starting",
        DevtoolsSessionState::Building => "building",
        DevtoolsSessionState::Ready => "ready",
        DevtoolsSessionState::Error => "error",
        DevtoolsSessionState::Unavailable => "unavailable",
        DevtoolsSessionState::Stopped => "stopped",
    }
}

fn write_nodes_json(output: &mut String, nodes: &[BrowserNode]) {
    for (index, node) in nodes.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push('{');
        field_string(output, "id", &node.id, true);
        field_string(output, "kind", node.kind.as_str(), false);
        field_string(output, "label", &node.label, false);
        field_optional_string(output, "value", node.value.as_deref());
        field_optional_string(output, "status", node.status.as_deref());
        output.push_str(",\"children\":[");
        write_nodes_json(output, &node.children);
        output.push_str("]}");
    }
}

fn field_string(output: &mut String, key: &str, value: &str, first: bool) {
    if !first {
        output.push(',');
    }
    quoted(key, output);
    output.push(':');
    quoted(value, output);
}

fn field_optional_string(output: &mut String, key: &str, value: Option<&str>) {
    output.push(',');
    quoted(key, output);
    output.push(':');
    match value {
        Some(value) => quoted(value, output),
        None => output.push_str("null"),
    }
}

fn field_u64_option(output: &mut String, key: &str, value: Option<u64>) {
    output.push(',');
    quoted(key, output);
    output.push(':');
    match value {
        Some(value) => output.push_str(&value.to_string()),
        None => output.push_str("null"),
    }
}

fn field_u64(output: &mut String, key: &str, value: u64, first: bool) {
    if !first {
        output.push(',');
    }
    quoted(key, output);
    output.push(':');
    output.push_str(&value.to_string());
}

fn field_bool(output: &mut String, key: &str, value: bool, first: bool) {
    if !first {
        output.push(',');
    }
    quoted(key, output);
    output.push(':');
    output.push_str(if value { "true" } else { "false" });
}

fn quoted(value: &str, output: &mut String) {
    output.push('"');
    output.push_str(&json_escape(value));
    output.push('"');
}

/// Encode a bounded reconnect result.  Old cursors are explicit resets, not
/// silent partial views.
pub(crate) fn reconnect_json(
    state: &BrowserWebState,
    from: Option<u64>,
    reset: bool,
    truncation: bool,
) -> Result<String, String> {
    let mut output = String::with_capacity(4096);
    output.push('{');
    field_string(&mut output, "schema", BROWSER_PAGE_SCHEMA, true);
    field_string(
        &mut output,
        "protocol",
        crate::Session::JET_DEVTOOLS_PROTOCOL,
        false,
    );
    field_string(&mut output, "session_id", state.host().session_id(), false);
    field_u64_option(&mut output, "from", from);
    field_u64_option(&mut output, "cursor", state.host().sequence());
    field_bool(&mut output, "reset", reset, false);
    field_bool(&mut output, "truncation", truncation, false);
    output.push_str(",\"complete\":");
    if truncation {
        output.push_str("false");
    } else {
        output.push_str("true");
    }
    output.push_str(",\"events\":[");
    if !truncation {
        let events = state
            .host()
            .resume_from(from.map(BrowserReconnectCursor::new))
            .map_err(|error| error.to_string())?
            .events;
        for (index, event) in events.iter().enumerate() {
            if index != 0 {
                output.push(',');
            }
            event_json(&mut output, event);
        }
    }
    output.push_str("],\"view\":");
    output.push_str(&view_json(state, BrowserPlacement::Workbench));
    output.push('}');
    Ok(output)
}

fn event_json(output: &mut String, event: &BrowserHostEvent) {
    output.push('{');
    field_string(output, "protocol", &event.protocol, true);
    field_string(output, "session_id", &event.session_id, false);
    field_u64(output, "revision", event.revision, false);
    field_u64(output, "sequence", event.sequence, false);
    field_u64(output, "observed_at_ms", event.observed_at_ms, false);
    output.push_str(",\"panels\":[");
    for (index, panel) in event.panels.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        panel_json(output, panel);
    }
    output.push_str("]}");
}

fn panel_json(output: &mut String, panel: &BrowserPanelFact) {
    output.push('{');
    field_string(output, "panel_id", &panel.panel_id, true);
    field_string(output, "title", &panel.title, false);
    field_u64(output, "freshness_ms", panel.freshness_ms, false);
    field_string(output, "status", &panel.status, false);
    field_u64_option(output, "cursor", panel.cursor);
    output.push_str(",\"nodes\":[");
    write_nodes_json(output, &panel.nodes);
    output.push_str("]}");
}

/// Parse the canonical nested browser selection envelope.  Selection is
/// shared by both placements and therefore cannot carry title aliases or
/// legacy top-level fallback fields.
pub(crate) fn parse_selection(payload: &[u8]) -> Result<Option<(String, Option<String>)>, String> {
    let text = std::str::from_utf8(payload)
        .map_err(|_| "browser selection body must be UTF-8 JSON".to_string())?;
    let root = parse_json_with_limit(text, crate::MAX_REQUEST_BODY_BYTES)
        .map_err(|_| "browser selection body is not valid bounded JSON".to_string())?;
    let object =
        object_map(&root).map_err(|_| "browser selection body must be an object".to_string())?;
    let selection = object
        .get("selection")
        .ok_or_else(|| "browser selection body must include selection".to_string())?;
    if matches!(selection, DataTree::Null) {
        return Ok(None);
    }
    let selection = object_map(selection)
        .map_err(|_| "browser selection must be an object or null".to_string())?;
    let panel_id = selection
        .get("panel_id")
        .ok_or_else(|| "browser selection must include panel_id".to_string())?
        .as_str()
        .map_err(|_| "browser selection panel_id must be a string".to_string())?;
    if panel_id.is_empty() {
        return Err("browser selection panel_id must not be empty".to_string());
    }
    let item_key = match selection.get("item_key") {
        None | Some(DataTree::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .map_err(|_| "browser selection item_key must be a string".to_string())?
                .to_string(),
        ),
    };
    Ok(Some((panel_id.to_string(), item_key)))
}

/// Parse one browser action at the HTTP boundary with the shared foundation
/// JSON parser.  The resulting action is still checked by `BrowserHost`.
pub(crate) fn parse_action(
    payload: &[u8],
    default_placement: BrowserPlacement,
) -> Result<BrowserHostAction, String> {
    let text = std::str::from_utf8(payload)
        .map_err(|_| "browser action body must be UTF-8 JSON".to_string())?;
    let root = parse_json_with_limit(text, crate::MAX_REQUEST_BODY_BYTES)
        .map_err(|_| "browser action body is not valid bounded JSON".to_string())?;
    let object =
        object_map(&root).map_err(|_| "browser action body must be an object".to_string())?;
    let name = required_string(&object, "action").or_else(|_| required_string(&object, "op"))?;
    let placement = object
        .get("placement")
        .map(parse_placement)
        .transpose()?
        .unwrap_or(default_placement);
    match name {
        "open" | "open_panel" => Ok(BrowserHostAction::open(
            placement,
            required_string(&object, "panel_id")?,
        )),
        "close" | "close_panel" => Ok(BrowserHostAction::close(
            placement,
            required_string(&object, "panel_id")?,
        )),
        "dock" | "dock_panel" => Ok(BrowserHostAction::dock(
            placement,
            required_string(&object, "panel_id")?,
            parse_dock(required_string(&object, "dock")?)?,
        )),
        "navigate_source" | "navigate" => Ok(BrowserHostAction::navigate_source(
            placement,
            required_string(&object, "panel_id")?,
            required_string(&object, "node_id")?,
            parse_source_grant(&object)?,
        )),
        "invoke_action" | "invoke" => {
            let action_id = required_string(&object, "action_id")?;
            Ok(BrowserHostAction::invoke_action(
                placement,
                required_string(&object, "panel_id")?,
                required_string(&object, "node_id")?,
                action_id,
                parse_action_grant(&object)?,
            ))
        }
        _ => Err(format!("unknown browser action `{name}`")),
    }
}

fn parse_source_grant(object: &BTreeMap<String, DataTree>) -> Result<BrowserSourceGrant, String> {
    let grant = nested_object(object, "grant")?;
    let source = nested_object(&grant, "source")?;
    let function = optional_string(&source, "function");
    let location = BrowserSourceLocation::new(
        required_string(&source, "source_id")?,
        required_u32(&source, "line")?,
        optional_u32(&source, "column").unwrap_or(0),
        function.map(str::to_string),
    );
    Ok(BrowserSourceGrant::new(
        required_string(&grant, "session_id")?,
        required_u64(&grant, "revision")?,
        required_string(&grant, "panel_id")?,
        required_string(&grant, "node_id")?,
        location,
    ))
}

fn parse_action_grant(object: &BTreeMap<String, DataTree>) -> Result<BrowserActionGrant, String> {
    let grant = nested_object(object, "grant")?;
    Ok(BrowserActionGrant::new(
        required_string(&grant, "session_id")?,
        required_u64(&grant, "revision")?,
        required_string(&grant, "panel_id")?,
        required_string(&grant, "node_id")?,
        required_string(&grant, "action_id")?,
    ))
}

fn nested_object(
    object: &BTreeMap<String, DataTree>,
    key: &str,
) -> Result<BTreeMap<String, DataTree>, String> {
    let value = object
        .get(key)
        .ok_or_else(|| format!("browser action is missing `{key}`"))?;
    object_map(value).map_err(|_| format!("browser action `{key}` must be an object"))
}

fn required_string<'a>(
    object: &'a BTreeMap<String, DataTree>,
    key: &str,
) -> Result<&'a str, String> {
    object
        .get(key)
        .ok_or_else(|| format!("browser action is missing `{key}`"))?
        .as_str()
        .map_err(|_| format!("browser action `{key}` must be a string"))
}

fn optional_string<'a>(object: &'a BTreeMap<String, DataTree>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(|value| value.as_str().ok())
}

fn required_u64(object: &BTreeMap<String, DataTree>, key: &str) -> Result<u64, String> {
    match object.get(key) {
        Some(DataTree::Int(value)) if *value >= 0 => {
            u64::try_from(*value).map_err(|_| format!("browser action `{key}` is out of range"))
        }
        _ => Err(format!(
            "browser action `{key}` must be a non-negative integer"
        )),
    }
}

fn required_u32(object: &BTreeMap<String, DataTree>, key: &str) -> Result<u32, String> {
    required_u64(object, key).and_then(|value| {
        u32::try_from(value).map_err(|_| format!("browser action `{key}` is out of range"))
    })
}

fn optional_u32(object: &BTreeMap<String, DataTree>, key: &str) -> Option<u32> {
    object.get(key).and_then(|value| match value {
        DataTree::Int(value) if *value >= 0 => u32::try_from(*value).ok(),
        _ => None,
    })
}

fn parse_placement(value: &DataTree) -> Result<BrowserPlacement, String> {
    match value
        .as_str()
        .map_err(|_| "browser action placement must be a string".to_string())?
    {
        "in-app" | "in_app" => Ok(BrowserPlacement::InApp),
        "workbench" => Ok(BrowserPlacement::Workbench),
        value => Err(format!("unknown browser placement `{value}`")),
    }
}

fn parse_dock(value: &str) -> Result<BrowserDock, String> {
    match value {
        "left" => Ok(BrowserDock::Left),
        "right" => Ok(BrowserDock::Right),
        "bottom" => Ok(BrowserDock::Bottom),
        "floating" => Ok(BrowserDock::Floating),
        _ => Err(format!("unknown browser dock `{value}`")),
    }
}

fn effect_json(effect: &BrowserHostEffect) -> String {
    let mut output = String::from("{\"schema\":\"");
    output.push_str(BROWSER_PAGE_SCHEMA);
    output.push_str("\",\"effect\":");
    match effect {
        BrowserHostEffect::PanelOpened {
            placement,
            panel_id,
        } => {
            output.push_str("{\"kind\":\"panel-opened\",\"placement\":");
            quoted(placement.as_str(), &mut output);
            output.push_str(",\"panel_id\":");
            quoted(panel_id, &mut output);
        }
        BrowserHostEffect::PanelClosed {
            placement,
            panel_id,
        } => {
            output.push_str("{\"kind\":\"panel-closed\",\"placement\":");
            quoted(placement.as_str(), &mut output);
            output.push_str(",\"panel_id\":");
            quoted(panel_id, &mut output);
        }
        BrowserHostEffect::PanelDocked {
            placement,
            panel_id,
            dock,
        } => {
            output.push_str("{\"kind\":\"panel-docked\",\"placement\":");
            quoted(placement.as_str(), &mut output);
            output.push_str(",\"panel_id\":");
            quoted(panel_id, &mut output);
            output.push_str(",\"dock\":");
            quoted(dock.as_str(), &mut output);
        }
        BrowserHostEffect::SourceNavigated {
            placement,
            panel_id,
            node_id,
            source,
        } => {
            output.push_str("{\"kind\":\"source-navigated\",\"placement\":");
            quoted(placement.as_str(), &mut output);
            output.push_str(",\"panel_id\":");
            quoted(panel_id, &mut output);
            output.push_str(",\"node_id\":");
            quoted(node_id, &mut output);
            output.push_str(",\"source\":{");
            field_string(&mut output, "source_id", &source.source_id, true);
            field_u64(&mut output, "line", source.line as u64, false);
            field_u64(&mut output, "column", source.column as u64, false);
            field_optional_string(&mut output, "function", source.function.as_deref());
            output.push('}');
        }
        BrowserHostEffect::ActionInvoked {
            placement,
            panel_id,
            node_id,
            action_id,
        } => {
            output.push_str("{\"kind\":\"action-invoked\",\"placement\":");
            quoted(placement.as_str(), &mut output);
            output.push_str(",\"panel_id\":");
            quoted(panel_id, &mut output);
            output.push_str(",\"node_id\":");
            quoted(node_id, &mut output);
            output.push_str(",\"action_id\":");
            quoted(action_id, &mut output);
        }
    }
    output.push_str("}}");
    output
}

pub(crate) fn dispatch_action(
    state: &mut BrowserWebState,
    action: BrowserHostAction,
) -> Result<String, BrowserHostError> {
    state
        .host
        .dispatch(action)
        .map(|effect| effect_json(&effect))
}

/// Render flat node rows: label, then the typed status (an event's kind) as a
/// quiet chip.  `selected` is the shared selection inside this panel; a row
/// also reads as selected when it holds the selected descendant, because the
/// selected card or inspector carries that detail.
fn render_nodes_html(
    output: &mut String,
    panel_id: &str,
    nodes: &[BrowserNode],
    selected: Option<&str>,
) {
    for node in nodes {
        let is_selected = selected == Some(node.id.as_str());
        let holds = contains_node(&node.children, selected);
        output.push_str("<li data-selected=\"");
        output.push_str(if is_selected || holds {
            "true"
        } else {
            "false"
        });
        output.push_str("\"><button class=\"node\" data-select=\"1\" data-panel=\"");
        output.push_str(&html_escape(panel_id));
        output.push_str("\" data-node=\"");
        output.push_str(&html_escape(&node.id));
        output.push_str("\" data-focus=\"node:");
        output.push_str(&html_escape(panel_id));
        output.push(':');
        output.push_str(&html_escape(&node.id));
        output.push_str("\" aria-pressed=\"");
        output.push_str(if is_selected { "true" } else { "false" });
        output.push_str("\"><span class=\"lbl\">");
        output.push_str(&html_escape(&node.label));
        output.push_str("</span>");
        if let Some(status) = &node.status {
            output.push_str("<b>");
            output.push_str(&html_escape(status));
            output.push_str("</b>");
        }
        output.push_str("</button></li>");
    }
}

fn contains_node(nodes: &[BrowserNode], node_id: Option<&str>) -> bool {
    let Some(node_id) = node_id else {
        return false;
    };
    nodes
        .iter()
        .any(|node| node.id == node_id || contains_node(&node.children, Some(node_id)))
}

fn html_escape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            character if character.is_control() => output.push(' '),
            character => output.push(character),
        }
    }
    output
}

fn js_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{2028}' => output.push_str("\\u2028"),
            '\u{2029}' => output.push_str("\\u2029"),
            '<' => output.push_str("\\u003c"),
            '>' => output.push_str("\\u003e"),
            '&' => output.push_str("\\u0026"),
            character if character.is_control() => {
                let _ = write!(output, "\\u{:04x}", character as u32);
            }
            character => output.push(character),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Devtools::{JetDevtoolsEnvelope, JetDevtoolsEvent};

    fn projection(sequence: u64) -> (DevtoolsProjection, Vec<JetDevtoolsPanelProjection>) {
        let mut envelope = JetDevtoolsEnvelope::new("s1", 0);
        envelope.push(
            JetDevtoolsEvent::from_parts(
                100 + sequence,
                "devserver",
                "state",
                "session",
                "{\"state\":\"ready\"}",
            )
            .unwrap(),
        );
        let event = envelope.events().next().unwrap().clone();
        let panels = crate::Devtools::catalog::canonical().unwrap().project(
            &envelope,
            JetDevtoolsHostKind::BrowserInApp,
            &JetDevtoolsPanelCapability::ALL,
        );
        let projection = DevtoolsProjection {
            protocol: crate::Session::JET_DEVTOOLS_PROTOCOL.to_string(),
            session_id: "s1".to_string(),
            source_id: None,
            build_id: None,
            revision: "rev".to_string(),
            world_id: None,
            sequence,
            observed_at: 100 + sequence,
            panels: vec![event],
            selection: None,
            status: DevtoolsStatusFact {
                state: DevtoolsSessionState::Ready,
                diagnostic_code: None,
                diagnostic: None,
                accepted_revision: None,
                last_good_revision: None,
                run_target: None,
                run_output: None,
                test_state: None,
            },
            command_capabilities: crate::Session::DevtoolsCommandCapabilities {
                project_rebuild: false,
                project_rebuild_reason: Some(
                    "no rebuild executor is registered for this host".to_string(),
                ),
            },
            command_receipts: Vec::new(),
            reset: sequence == 1,
            truncation: false,
        };
        (projection, panels)
    }

    #[test]
    fn typed_projection_renders_both_placements_from_one_state() {
        let mut state = BrowserWebState::new("s1".to_string());
        let (projection, panels) = projection(1);
        state.sync(projection, panels).unwrap();
        let in_app = state.host().view_model(BrowserPlacement::InApp);
        let workbench = state.host().view_model(BrowserPlacement::Workbench);
        assert_eq!(in_app.session_id, workbench.session_id);
        assert_eq!(in_app.panels[0].panel_id, workbench.panels[0].panel_id);
        assert_ne!(in_app.route, workbench.route);
    }

    #[test]
    fn typed_status_and_identity_stay_frame_metadata() {
        let (mut projection, panels) = projection(1);
        projection.source_id = Some("src/main.jet".to_string());
        projection.build_id = Some("build-1".to_string());
        projection.world_id = Some("world-1".to_string());
        projection.status = DevtoolsStatusFact {
            state: DevtoolsSessionState::Building,
            diagnostic_code: Some("E0102".to_string()),
            diagnostic: Some("missing name".to_string()),
            accepted_revision: Some("accepted".to_string()),
            last_good_revision: Some("good".to_string()),
            run_target: Some("browser".to_string()),
            run_output: Some("web".to_string()),
            test_state: Some("idle".to_string()),
        };
        let mut state = BrowserWebState::new("s1".to_string());
        state.sync(projection, panels).unwrap();
        let frame = state.frame(BrowserPlacement::InApp);
        assert_eq!(frame.source_id.as_deref(), Some("src/main.jet"));
        assert_eq!(frame.state, BrowserSessionState::Building);
        assert!(!frame.panels.is_empty());
        assert_eq!(frame.status.diagnostic_code.as_deref(), Some("E0102"));
        assert!(!frame.tools.rebuild);
        let json = view_json(&state, BrowserPlacement::InApp);
        assert!(json.contains("\"source_id\":\"src/main.jet\""));
        assert!(json.contains("\"status\":{\"state\":\"building\""));
        assert!(json.contains("\"rebuild\":{\"available\":false,\"reason\":\"no rebuild executor is registered for this host\"}"));
        assert!(json.contains("\"receipts\":[]"));
        assert!(!json.contains("__session_status"));
    }

    #[test]
    fn page_offers_tools_only_where_the_host_has_them() {
        let (mut projection, panels) = projection(1);
        projection.status.state = DevtoolsSessionState::Error;
        projection.status.diagnostic_code = Some("E0102".to_string());
        projection.status.diagnostic =
            Some("Error [E0102]: unknown symbol\n  --> app.jet:12:9\n".to_string());
        let mut state = BrowserWebState::new("s1".to_string());
        state.sync(projection, panels).unwrap();
        let page = render_page(&state, BrowserPlacement::InApp);
        assert!(!page.contains("data-tool=\"source\""));
        assert!(!page.contains("data-tool=\"rebuild\""));
        assert!(page.contains("no rebuild executor is registered for this host"));
        assert!(page.contains("<pre class=\"diag\">Error [E0102]: unknown symbol"));

        state.tools.source = true;
        state.tools.source_reason = None;
        let page = render_page(&state, BrowserPlacement::InApp);
        assert!(page.contains(
            "data-tool=\"source\" data-focus=\"tool:source\" data-file=\"app.jet\">Fix in source"
        ));
        assert!(page.contains("class=\"source\""));
    }

    #[test]
    fn reconnect_reset_returns_bounded_snapshot_events() {
        let mut state = BrowserWebState::new("s1".to_string());
        let (projection, panels) = projection(1);
        state.sync(projection, panels).unwrap();
        let body = reconnect_json(&state, None, true, false).unwrap();
        assert!(body.contains("\"reset\":true"));
        assert!(body.contains("\"complete\":true"));
        assert!(body.contains("\"sequence\":1"));

        let body = reconnect_json(&state, Some(0), true, true).unwrap();
        assert!(body.contains("\"truncation\":true"));
        assert!(body.contains("\"complete\":false"));
        assert!(body.contains("\"events\":[]"));
    }

    #[test]
    fn action_parser_requires_typed_grant_for_source_navigation() {
        let error = parse_action(
            br#"{"action":"navigate_source","panel_id":"build","node_id":"source"}"#,
            BrowserPlacement::InApp,
        )
        .unwrap_err();
        assert!(error.contains("grant"));
    }
}
