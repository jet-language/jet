//! Host-neutral editor workbench transport for the `jet.devtools.v1` stream.
//!
//! VS Code and Zed are protocol clients, not separate evaluators.  This module
//! only validates request facts, projects the typed Session view, and returns
//! deterministic receipts.  It never starts a process or grants command
//! authority on behalf of an editor.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use jet_foundation::DataTree::DataTree;
use jet_foundation::JSON::{json_escape, parse_json_with_limit};

use crate::Session::{DevtoolsProjection, ResidentDevSession, JET_DEVTOOLS_PROTOCOL};
use crate::Devtools::{
    JetDevtoolsEvent, JetDevtoolsHostKind, JetDevtoolsPanelAvailability, JetDevtoolsPanelCapability,
    JetDevtoolsPanelDescriptor, JetDevtoolsSelection, JetDevtoolsTimeCursor, JetDevtoolsViewState,
};

pub const EDITOR_PROTOCOL_VERSION: u32 = 1;
pub const EDITOR_MAX_EVENTS: usize = 256;
pub const EDITOR_MAX_HISTORY: usize = 256;
pub const EDITOR_MAX_TEXT_BYTES: usize = 16 * 1024;
pub const EDITOR_WORKBENCH_METHOD: &str = "jet/devtools/open";
pub const EDITOR_RECONNECT_METHOD: &str = "jet/devtools/reconnect";
pub const EDITOR_NAVIGATE_METHOD: &str = "jet/devtools/navigate";
pub const EDITOR_SELECT_METHOD: &str = "jet/devtools/select";
pub const EDITOR_COMMAND_METHOD: &str = "jet/devtools/command";

/// The two editor clients share every transport rule and differ only in this
/// protocol identity.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum EditorHost {
    VsCode,
    Zed,
}

impl EditorHost {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::VsCode => "vscode",
            Self::Zed => "zed",
        }
    }

    pub fn parse(value: &str) -> Result<Self, EditorHostError> {
        match value {
            "vscode" => Ok(Self::VsCode),
            "zed" => Ok(Self::Zed),
            _ => Err(EditorHostError::UnsupportedEditor(value.to_string())),
        }
    }
}

impl fmt::Display for EditorHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.wire_name())
    }
}

/// Commands that an editor may request through the host boundary.  Execution
/// remains owned by the existing Jet command path after this module returns a
/// receipt.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum EditorCommand {
    Check,
    Build,
    Test,
    Run,
    Debug,
}

impl EditorCommand {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Build => "build",
            Self::Test => "test",
            Self::Run => "run",
            Self::Debug => "debug",
        }
    }

    pub fn required_grant(self) -> &'static str {
        match self {
            Self::Check => "editor.command:check",
            Self::Build => "editor.command:build",
            Self::Test => "editor.command:test",
            Self::Run => "editor.command:run",
            Self::Debug => "editor.command:debug",
        }
    }

    pub fn parse(value: &str) -> Result<Self, EditorHostError> {
        match value {
            "check" => Ok(Self::Check),
            "build" => Ok(Self::Build),
            "test" => Ok(Self::Test),
            "run" => Ok(Self::Run),
            "debug" => Ok(Self::Debug),
            _ => Err(EditorHostError::UnsupportedCommand(value.to_string())),
        }
    }
}

impl fmt::Display for EditorCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.wire_name())
    }
}


#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorSessionIdentity {
    pub session_id: String,
    pub source_id: String,
    pub source_path: String,
    pub revision: String,
}

impl EditorSessionIdentity {
    pub fn new(
        session_id: impl Into<String>,
        source_id: impl Into<String>,
        source_path: impl Into<String>,
        revision: impl Into<String>,
    ) -> Result<Self, EditorHostError> {
        let session_id = session_id.into();
        let source_id = source_id.into();
        let source_path = source_path.into();
        let revision = revision.into();
        validate_stable_text(&session_id, "session_id")?;
        validate_source_identity(&source_id)?;
        let source_path = clean_source_path(&source_path)?;
        validate_stable_text(&revision, "revision")?;
        Ok(Self {
            session_id,
            source_id,
            source_path,
            revision,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EditorFactValue {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    Text(String),
    Array(Vec<EditorFactValue>),
    Object(BTreeMap<String, EditorFactValue>),
}

impl EditorFactValue {
    pub fn text(value: impl Into<String>) -> Result<Self, EditorHostError> {
        let value = value.into();
        validate_stable_text(&value, "fact text")?;
        Ok(Self::Text(value))
    }


    fn render(&self, out: &mut String) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Self::Integer(value) => out.push_str(&value.to_string()),
            Self::Float(value) => out.push_str(&value.to_string()),
            Self::Text(value) => render_string(value, out),
            Self::Array(values) => {
                out.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        out.push(',');
                    }
                    value.render(out);
                }
                out.push(']');
            }
            Self::Object(values) => {
                out.push('{');
                for (index, (key, value)) in values.iter().enumerate() {
                    if index != 0 {
                        out.push(',');
                    }
                    render_string(key, out);
                    out.push(':');
                    value.render(out);
                }
                out.push('}');
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EditorStreamEvent {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub source: String,
    pub kind: String,
    pub entity: String,
    pub fields: BTreeMap<String, EditorFactValue>,
    pub payload: Option<BTreeMap<String, EditorFactValue>>,
}

impl EditorStreamEvent {
    pub fn new(
        sequence: u64,
        timestamp_ms: u64,
        source: impl Into<String>,
        kind: impl Into<String>,
        entity: impl Into<String>,
        fields: BTreeMap<String, EditorFactValue>,
        payload: Option<BTreeMap<String, EditorFactValue>>,
    ) -> Result<Self, EditorHostError> {
        let event = Self {
            sequence,
            timestamp_ms,
            source: source.into(),
            kind: kind.into(),
            entity: entity.into(),
            fields,
            payload,
        };
        event.validate()?;
        Ok(event)
    }

    fn validate(&self) -> Result<(), EditorHostError> {
        if self.sequence == 0 {
            return Err(EditorHostError::InvalidEnvelope(
                "event sequence must be greater than zero".to_string(),
            ));
        }
        validate_stable_text(&self.source, "event source")?;
        validate_stable_text(&self.kind, "event kind")?;
        validate_stable_text(&self.entity, "event entity")?;
        validate_fact_object(&self.fields, "event fields")?;
        if let Some(payload) = &self.payload {
            validate_fact_object(payload, "event payload")?;
        }
        Ok(())
    }

    pub fn serialize(&self) -> String {
        let mut out = String::from("{\"sequence\":");
        out.push_str(&self.sequence.to_string());
        out.push_str(",\"timestamp_ms\":");
        out.push_str(&self.timestamp_ms.to_string());
        out.push_str(",\"source\":");
        render_string(&self.source, &mut out);
        out.push_str(",\"kind\":");
        render_string(&self.kind, &mut out);
        out.push_str(",\"entity\":");
        render_string(&self.entity, &mut out);
        out.push_str(",\"fields\":");
        render_fact_object(&self.fields, &mut out);
        out.push_str(",\"payload\":");
        match &self.payload {
            Some(payload) => render_fact_object(payload, &mut out),
            None => out.push_str("null"),
        }
        out.push('}');
        out
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EditorStreamSnapshot {
    pub events: Vec<EditorStreamEvent>,
    pub resumed_from: Option<u64>,
    pub next_cursor: Option<u64>,
}

impl EditorStreamSnapshot {
    pub fn serialize(&self) -> String {
        let mut out = String::from("{\"events\":[");
        for (index, event) in self.events.iter().enumerate() {
            if index != 0 {
                out.push(',');
            }
            out.push_str(&event.serialize());
        }
        out.push_str("],\"resumed_from\":");
        render_optional_u64(self.resumed_from, &mut out);
        out.push_str(",\"next_cursor\":");
        render_optional_u64(self.next_cursor, &mut out);
        out.push('}');
        out
    }
}

fn serialize_editor_selection(selection: &JetDevtoolsSelection) -> String {
    let mut out = String::from("{\"panel_id\":");
    render_string(&selection.panel_id, &mut out);
    out.push_str(",\"item_key\":");
    match &selection.item_key {
        Some(value) => render_string(value, &mut out),
        None => out.push_str("null"),
    }
    out.push('}');
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct EditorWorkbenchSnapshot {
    pub host: EditorHost,
    pub protocol: &'static str,
    pub protocol_version: u32,
    pub workbench_url: String,
    pub identity: EditorSessionIdentity,
    pub started_at_ms: u64,
    pub panels: Vec<JetDevtoolsPanelDescriptor>,
    pub panel_availability: Vec<JetDevtoolsPanelAvailability>,

    pub selection: JetDevtoolsSelection,
    pub events: Vec<EditorStreamEvent>,
    pub resumed_from: Option<u64>,
    pub cursor: Option<u64>,
    pub time_cursor: Option<JetDevtoolsTimeCursor>,
}

impl EditorWorkbenchSnapshot {
    pub fn serialize(&self) -> String {
        let mut out = String::from("{\"protocol\":");
        render_string(self.protocol, &mut out);
        out.push_str(",\"protocol_version\":");
        out.push_str(&self.protocol_version.to_string());
        out.push_str(",\"editor\":");
        render_string(self.host.wire_name(), &mut out);
        out.push_str(",\"workbench_url\":");
        render_string(&self.workbench_url, &mut out);
        out.push_str(",\"session_id\":");
        render_string(&self.identity.session_id, &mut out);
        out.push_str(",\"source_id\":");
        render_string(&self.identity.source_id, &mut out);
        out.push_str(",\"source_path\":");
        render_string(&self.identity.source_path, &mut out);
        out.push_str(",\"revision\":");
        render_string(&self.identity.revision, &mut out);
        out.push_str(",\"started_at_ms\":");
        out.push_str(&self.started_at_ms.to_string());
        out.push_str(",\"panels\":[");
        for (index, panel) in self.panels.iter().enumerate() {
            if index != 0 {
                out.push(',');
            }
            out.push_str("{\"id\":");
            render_string(panel.id.as_str(), &mut out);
            out.push_str(",\"title\":");
            render_string(panel.title, &mut out);
            out.push_str(",\"ordinal\":");
            out.push_str(&panel.order.to_string());
            out.push('}');
        }
        out.push_str("],\"panel_availability\":[");
        for (index, row) in self.panel_availability.iter().enumerate() {
            if index != 0 {
                out.push(',');
            }
            out.push_str("{\"id\":");
            render_string(row.descriptor.id.as_str(), &mut out);
            out.push_str(",\"available\":");
            out.push_str(if row.is_available() { "true" } else { "false" });
            out.push_str(",\"reasons\":[");
            for (reason_index, reason) in row.unavailable_reasons().iter().enumerate() {
                if reason_index != 0 {
                    out.push(',');
                }
                render_string(reason.as_str(), &mut out);
            }
            out.push_str("]}");
        }
        out.push_str("],\"selection\":");
        out.push_str(&serialize_editor_selection(&self.selection));
        out.push_str(",\"events\":[");
        for (index, event) in self.events.iter().enumerate() {
            if index != 0 {
                out.push(',');
            }
            out.push_str(&event.serialize());
        }
        out.push_str("],\"resumed_from\":");
        render_optional_u64(self.resumed_from, &mut out);
        out.push_str(",\"cursor\":");
        render_optional_u64(self.cursor, &mut out);
        out.push_str(",\"time_cursor\":");
        render_optional_u64(self.time_cursor, &mut out);
        out.push('}');
        out
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorSourceNavigation {
    pub host: EditorHost,
    pub identity: EditorSessionIdentity,
    pub line: u32,
    pub column: u32,
    pub source_url: String,
}

impl EditorSourceNavigation {
    pub fn serialize(&self) -> String {
        let mut out = String::from("{\"protocol\":");
        render_string(JET_DEVTOOLS_PROTOCOL, &mut out);
        out.push_str(",\"editor\":");
        render_string(self.host.wire_name(), &mut out);
        out.push_str(",\"session_id\":");
        render_string(&self.identity.session_id, &mut out);
        out.push_str(",\"source_id\":");
        render_string(&self.identity.source_id, &mut out);
        out.push_str(",\"source_path\":");
        render_string(&self.identity.source_path, &mut out);
        out.push_str(",\"revision\":");
        render_string(&self.identity.revision, &mut out);
        out.push_str(",\"line\":");
        out.push_str(&self.line.to_string());
        out.push_str(",\"column\":");
        out.push_str(&self.column.to_string());
        out.push_str(",\"source_url\":");
        render_string(&self.source_url, &mut out);
        out.push('}');
        out
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorCommandReceipt {
    pub host: EditorHost,
    pub identity: EditorSessionIdentity,
    pub command: EditorCommand,
    pub required_grant: String,
}

impl EditorCommandReceipt {
    pub fn serialize(&self) -> String {
        let mut out = String::from("{\"protocol\":");
        render_string(JET_DEVTOOLS_PROTOCOL, &mut out);
        out.push_str(",\"editor\":");
        render_string(self.host.wire_name(), &mut out);
        out.push_str(",\"session_id\":");
        render_string(&self.identity.session_id, &mut out);
        out.push_str(",\"source_id\":");
        render_string(&self.identity.source_id, &mut out);
        out.push_str(",\"revision\":");
        render_string(&self.identity.revision, &mut out);
        out.push_str(",\"command\":");
        render_string(self.command.wire_name(), &mut out);
        out.push_str(",\"required_grant\":");
        render_string(&self.required_grant, &mut out);
        out.push_str(",\"status\":\"accepted\"}");
        out
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EditorHostResponse {
    Workbench(EditorWorkbenchSnapshot),
    Stream(EditorStreamSnapshot),
    Navigation(EditorSourceNavigation),
    Selection(JetDevtoolsSelection),
    Command(EditorCommandReceipt),
}

impl EditorHostResponse {
    pub fn serialize(&self) -> String {
        match self {
            Self::Workbench(value) => value.serialize(),
            Self::Stream(value) => value.serialize(),
            Self::Navigation(value) => value.serialize(),
            Self::Selection(value) => serialize_editor_selection(value),
            Self::Command(value) => value.serialize(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EditorHostError {
    InvalidRequest(String),
    InvalidEnvelope(String),
    UnsupportedEditor(String),
    UnsupportedCommand(String),
    UnsupportedMethod(String),
    LoopbackViolation(String),
    SourcePathRejected(String),
    SessionMismatch { expected: String, actual: String },
    SourceMismatch { expected: String, actual: String },
    RevisionMismatch { expected: String, actual: String },
    PanelNotFound(String),
    CursorExpired { requested: u64, oldest: u64 },
    CommandDenied { command: EditorCommand, required_grant: String },
}

impl fmt::Display for EditorHostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(f, "invalid editor host request: {message}"),
            Self::InvalidEnvelope(message) => write!(f, "invalid jet.devtools.v1 envelope: {message}"),
            Self::UnsupportedEditor(editor) => write!(f, "unsupported editor host `{editor}`"),
            Self::UnsupportedCommand(command) => write!(f, "unsupported editor command `{command}`"),
            Self::UnsupportedMethod(method) => write!(f, "unsupported editor host method `{method}`"),
            Self::LoopbackViolation(message) => write!(f, "editor workbench must stay on loopback: {message}"),
            Self::SourcePathRejected(path) => write!(f, "source path rejected: {path}"),
            Self::SessionMismatch { expected, actual } => {
                write!(f, "editor session mismatch: expected `{expected}`, got `{actual}`")
            }
            Self::SourceMismatch { expected, actual } => {
                write!(f, "editor source mismatch: expected `{expected}`, got `{actual}`")
            }
            Self::RevisionMismatch { expected, actual } => {
                write!(f, "editor revision mismatch: expected `{expected}`, got `{actual}`")
            }
            Self::PanelNotFound(panel) => write!(f, "unknown editor panel `{panel}`"),
            Self::CursorExpired { requested, oldest } => write!(
                f,
                "editor reconnect cursor `{requested}` is before the retained stream window `{oldest}`"
            ),
            Self::CommandDenied {
                command,
                required_grant,
            } => write!(
                f,
                "editor command `{command}` requires ungranted authority `{required_grant}`"
            ),
        }
    }
}

impl std::error::Error for EditorHostError {}

/// One bounded transport serves both editor clients.  It owns no evaluator and
/// stores only the bounded event projection and canonical foundation view state.
pub struct EditorHostTransport {
    workbench_url: String,
    identity: EditorSessionIdentity,
    grants: BTreeSet<String>,
    events: VecDeque<EditorStreamEvent>,
    next_sequence: u64,
    started_at_ms: u64,
    view: JetDevtoolsViewState,
}

impl EditorHostTransport {
    pub fn new<I, S>(
        workbench_url: impl Into<String>,
        session_id: impl Into<String>,
        source_id: impl Into<String>,
        source_path: impl Into<String>,
        revision: impl Into<String>,
        grants: I,
    ) -> Result<Self, EditorHostError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let workbench_url = workbench_url.into();
        validate_loopback_url(&workbench_url)?;
        let identity = EditorSessionIdentity::new(session_id, source_id, source_path, revision)?;
        let mut grant_set = BTreeSet::new();
        for grant in grants {
            let grant = grant.as_ref();
            validate_stable_text(grant, "command grant")?;
            grant_set.insert(grant.to_string());
        }
        Ok(Self {
            workbench_url,
            identity,
            grants: grant_set,
            events: VecDeque::new(),
            next_sequence: 1,
            started_at_ms: 0,
            view: JetDevtoolsViewState {
                selection: Some(JetDevtoolsSelection::new("build", None)),
                cursor: None,
            },
        })
    }
    /// Return the canonical panel catalog and its typed capability verdicts.
    pub fn panel_catalog(&self) -> &'static [JetDevtoolsPanelDescriptor] {
        crate::Devtools::catalog::descriptors()
    }

    pub fn panel_availability(&self) -> Vec<JetDevtoolsPanelAvailability> {
        let grants = JetDevtoolsPanelCapability::ALL
            .iter()
            .copied()
            .filter(|capability| self.grants.contains(capability.grant()))
            .collect::<Vec<_>>();
        crate::Devtools::catalog::available_for(JetDevtoolsHostKind::Editor, grants.as_slice())
            .unwrap_or_default()
    }


    pub fn from_session<I, S>(
        workbench_url: impl Into<String>,
        session: &ResidentDevSession,
        source_id: impl Into<String>,
        source_path: impl Into<String>,
        revision: impl Into<String>,
        grants: I,
    ) -> Result<Self, EditorHostError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut transport = Self::new(
            workbench_url,
            session.id().to_string(),
            source_id,
            source_path,
            revision,
            grants,
        )?;
        let projection = session.devtools_events_since(None);
        transport.replace_projection(&projection)?;
        Ok(transport)
    }

    /// Return the shared foundation view state carried by this editor host.
    pub fn view(&self) -> &JetDevtoolsViewState {
        &self.view
    }

    /// Replace this workbench from the typed Session projection.  Hosts share
    /// the projection cursor and selection without reparsing a wire envelope.
    pub fn replace_projection(
        &mut self,
        projection: &DevtoolsProjection,
    ) -> Result<(), EditorHostError> {
        if projection.protocol != JET_DEVTOOLS_PROTOCOL {
            return Err(EditorHostError::InvalidEnvelope(
                "projection protocol does not match jet.devtools.v1".to_string(),
            ));
        }
        if projection.session_id != self.identity.session_id {
            return Err(EditorHostError::SessionMismatch {
                expected: self.identity.session_id.clone(),
                actual: projection.session_id.clone(),
            });
        }
        if !projection.revision.is_empty() && projection.revision != self.identity.revision {
            return Err(EditorHostError::RevisionMismatch {
                expected: self.identity.revision.clone(),
                actual: projection.revision.clone(),
            });
        }
        if let Some(source_id) = projection.source_id.as_deref() {
            if source_id != self.identity.source_id {
                return Err(EditorHostError::SourceMismatch {
                    expected: self.identity.source_id.clone(),
                    actual: source_id.to_string(),
                });
            }
        }

        let mut events = VecDeque::new();
        for event in &projection.panels {
            events.push_back(editor_event_from_event(event)?);
        }
        while events.len() > EDITOR_MAX_EVENTS {
            events.pop_front();
        }
        self.events = events;
        self.next_sequence = self
            .events
            .back()
            .map(|event| event.sequence.saturating_add(1))
            .unwrap_or(1);
        self.started_at_ms = projection.observed_at;
        self.view.cursor = (projection.sequence != 0).then_some(projection.sequence);
        self.view.selection = projection
            .selection
            .as_ref()
            .and_then(|selection| {
                canonical_editor_panel_id(&selection.panel_id).map(|panel_id| {
                    JetDevtoolsSelection::new(
                        panel_id,
                        (!selection.item_key.is_empty()).then(|| selection.item_key.clone()),
                    )
                })
            })
            .or_else(|| Some(JetDevtoolsSelection::new("build", None)));
        Ok(())
    }

    /// Refresh from the same resident session projection consumed by the
    /// terminal and native adapters.
    pub fn sync_session(&mut self, session: &ResidentDevSession) -> Result<(), EditorHostError> {
        if session.id() != self.identity.session_id {
            return Err(EditorHostError::SessionMismatch {
                expected: self.identity.session_id.clone(),
                actual: session.id().to_string(),
            });
        }
        let projection = session.devtools_events_since(None);
        self.replace_projection(&projection)
    }

    /// Replace this editor view from one canonical devtools envelope.
    pub fn replace_devtools_json(&mut self, payload: &str) -> Result<(), EditorHostError> {
        let decoded = crate::WebHost::decode_devtools_envelope(payload)
            .map_err(EditorHostError::InvalidEnvelope)?;
        let envelope = match decoded {
            jet_foundation::DevtoolsControl::JetDevtoolsDecodedEnvelope::Events(envelope) => {
                envelope
            }
            jet_foundation::DevtoolsControl::JetDevtoolsDecodedEnvelope::Commands(_) => {
                return Err(EditorHostError::InvalidEnvelope(
                    "editor host accepts runtime event envelopes, not command envelopes".to_string(),
                ));
            }
        };
        if envelope.session_id != self.identity.session_id {
            return Err(EditorHostError::SessionMismatch {
                expected: self.identity.session_id.clone(),
                actual: envelope.session_id,
            });
        }
        if let Some(identity) = envelope.source_identity.as_ref() {
            if let Some(source_id) = identity.source_id.as_deref() {
                if source_id != self.identity.source_id {
                    return Err(EditorHostError::SourceMismatch {
                        expected: self.identity.source_id.clone(),
                        actual: source_id.to_string(),
                    });
                }
            }
            if let Some(revision) = identity.revision.as_deref() {
                if !revision.is_empty() && revision != self.identity.revision {
                    return Err(EditorHostError::RevisionMismatch {
                        expected: self.identity.revision.clone(),
                        actual: revision.to_string(),
                    });
                }
            }
        }
        let mut events = VecDeque::new();
        for event in envelope.events() {
            events.push_back(editor_event_from_event(event)?);
        }
        while events.len() > EDITOR_MAX_EVENTS {
            events.pop_front();
        }
        self.next_sequence = events
            .back()
            .map(|event| event.sequence.saturating_add(1))
            .unwrap_or(1);
        self.events = events;
        self.started_at_ms = envelope.started_at_ms;
        self.view.cursor = envelope.cursor();
        self.view.selection = envelope
            .selection()
            .and_then(|selection| {
                canonical_editor_panel_id(&selection.panel_id).map(|panel_id| {
                    JetDevtoolsSelection::new(panel_id, selection.item_key.clone())
                })
            })
            .or_else(|| Some(JetDevtoolsSelection::new("build", None)));
        Ok(())
    }

    /// Handle one bounded JSON-RPC request from an editor client.
    pub fn handle_lsp_json(&mut self, payload: &str) -> Result<String, EditorHostError> {
        let root = parse_json_with_limit(payload, EDITOR_MAX_TEXT_BYTES)
            .map_err(|_| EditorHostError::InvalidRequest("request is not valid bounded JSON".to_string()))?;
        let object = match root {
            DataTree::Object(object) => object.into_iter().collect::<BTreeMap<_, _>>(),
            _ => {
                return Err(EditorHostError::InvalidRequest(
                    "JSON-RPC request must be an object".to_string(),
                ));
            }
        };
        match object.get("jsonrpc") {
            Some(DataTree::Text(version)) if version == "2.0" => {}
            _ => {
                return Err(EditorHostError::InvalidRequest(
                    "JSON-RPC version must be `2.0`".to_string(),
                ));
            }
        }
        let id = object.get("id").ok_or_else(|| {
            EditorHostError::InvalidRequest("JSON-RPC request requires an id".to_string())
        })?;
        if !matches!(
            id,
            DataTree::Null | DataTree::Text(_) | DataTree::Int(_) | DataTree::Float(_)
        ) {
            return Err(EditorHostError::InvalidRequest(
                "JSON-RPC id must be null, a string, or a number".to_string(),
            ));
        }
        let method = match object.get("method") {
            Some(DataTree::Text(method)) => method.as_str(),
            _ => {
                return Err(EditorHostError::InvalidRequest(
                    "JSON-RPC request requires a method".to_string(),
                ));
            }
        };
        let params = match object.get("params") {
            Some(DataTree::Object(params)) => params.iter().cloned().collect::<BTreeMap<_, _>>(),
            _ => {
                return Err(EditorHostError::InvalidRequest(
                    "JSON-RPC request requires an object params value".to_string(),
                ));
            }
        };
        let field_text = |key: &str| -> Result<String, EditorHostError> {
            match params.get(key) {
                Some(DataTree::Text(value)) => Ok(value.clone()),
                _ => Err(EditorHostError::InvalidRequest(format!(
                    "editor request field `{key}` must be text"
                ))),
            }
        };
        let optional_text = |key: &str| -> Result<Option<String>, EditorHostError> {
            match params.get(key) {
                None | Some(DataTree::Null) => Ok(None),
                Some(DataTree::Text(value)) => Ok(Some(value.clone())),
                _ => Err(EditorHostError::InvalidRequest(format!(
                    "editor request field `{key}` must be text or null"
                ))),
            }
        };
        let field_u64 = |key: &str| -> Result<u64, EditorHostError> {
            match params.get(key) {
                Some(DataTree::Int(value)) if *value >= 0 => Ok(*value as u64),
                Some(DataTree::Float(value))
                    if value.is_finite()
                        && *value >= 0.0
                        && *value <= u64::MAX as f64
                        && value.fract() == 0.0 =>
                {
                    Ok(*value as u64)
                }
                _ => Err(EditorHostError::InvalidRequest(format!(
                    "editor request field `{key}` must be a nonnegative integer"
                ))),
            }
        };
        let common = || -> Result<(EditorHost, String, String, String, String), EditorHostError> {
            let host = EditorHost::parse(&field_text("editor")?)?;
            Ok((
                host,
                field_text("session_id")?,
                field_text("source_id")?,
                field_text("source_path")?,
                field_text("revision")?,
            ))
        };
        let response = match method {
            EDITOR_WORKBENCH_METHOD => {
                let (host, session_id, source_id, source_path, revision) = common()?;
                let mut request = EditorWorkbenchOpenRequest::new(
                    host,
                    session_id,
                    source_id,
                    source_path,
                    revision,
                );
                request.panel_id = optional_text("panel_id")?;
                request.item_key = optional_text("item_key")?;
                request.cursor = match params.get("cursor") {
                    None | Some(DataTree::Null) => None,
                    Some(_) => Some(field_u64("cursor")?),
                };
                EditorHostResponse::Workbench(self.open(&request)?)
            }
            EDITOR_RECONNECT_METHOD => {
                let (host, session_id, source_id, source_path, revision) = common()?;
                let cursor = match params.get("cursor") {
                    None | Some(DataTree::Null) => None,
                    Some(_) => Some(field_u64("cursor")?),
                };
                let request = EditorReconnectRequest::new(
                    host,
                    session_id,
                    source_id,
                    source_path,
                    revision,
                    cursor,
                );
                EditorHostResponse::Workbench(self.reconnect(&request)?)
            }
            EDITOR_NAVIGATE_METHOD => {
                let (host, session_id, source_id, source_path, revision) = common()?;
                let line = field_u64("line")?;
                let column = field_u64("column")?;
                let line = u32::try_from(line).map_err(|_| {
                    EditorHostError::InvalidRequest("editor navigation line is out of range".to_string())
                })?;
                let column = u32::try_from(column).map_err(|_| {
                    EditorHostError::InvalidRequest("editor navigation column is out of range".to_string())
                })?;
                let request = EditorSourceNavigationRequest::new(
                    host,
                    session_id,
                    source_id,
                    source_path,
                    revision,
                    line,
                    column,
                );
                EditorHostResponse::Navigation(self.navigate_source(&request)?)
            }
            EDITOR_SELECT_METHOD => {
                let (host, session_id, source_id, source_path, revision) = common()?;
                let request = EditorPanelSelectionRequest {
                    host,
                    session_id,
                    source_id,
                    source_path,
                    revision,
                    panel_id: field_text("panel_id")?,
                    item_key: optional_text("item_key")?,
                };
                EditorHostResponse::Selection(self.select_panel(&request)?)
            }
            EDITOR_COMMAND_METHOD => {
                let (host, session_id, source_id, source_path, revision) = common()?;
                let command = EditorCommand::parse(&field_text("command")?)?;
                let arguments = match params.get("arguments") {
                    None => Vec::new(),
                    Some(DataTree::Array(arguments)) => arguments
                        .iter()
                        .map(|argument| match argument {
                            DataTree::Text(argument) => Ok(argument.clone()),
                            _ => Err(EditorHostError::InvalidRequest(
                                "editor command arguments must be text".to_string(),
                            )),
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    Some(_) => {
                        return Err(EditorHostError::InvalidRequest(
                            "editor command arguments must be an array".to_string(),
                        ));
                    }
                };
                let request = EditorCommandRequest {
                    host,
                    session_id,
                    source_id,
                    source_path,
                    revision,
                    command,
                    arguments,
                };
                EditorHostResponse::Command(self.command(&request)?)
            }
            other => return Err(EditorHostError::UnsupportedMethod(other.to_string())),
        };
        let mut response_json = String::from("{\"jsonrpc\":\"2.0\",\"id\":");
        response_json.push_str(&crate::Session::render_json(id));
        response_json.push_str(",\"result\":");
        response_json.push_str(&response.serialize());
        response_json.push('}');
        Ok(response_json)
    }


    pub fn identity(&self) -> &EditorSessionIdentity {
        &self.identity
    }

    pub fn workbench_url(&self) -> &str {
        &self.workbench_url
    }

    pub fn grants(&self) -> impl Iterator<Item = &str> {
        self.grants.iter().map(String::as_str)
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    pub fn latest_cursor(&self) -> Option<u64> {
        self.events.back().map(|event| event.sequence)
    }


    pub fn append_event(&mut self, event: EditorStreamEvent) -> Result<(), EditorHostError> {
        event.validate()?;
        if let Some(previous) = self.events.back() {
            if event.sequence <= previous.sequence {
                return Err(EditorHostError::InvalidEnvelope(
                    "event sequence must increase monotonically".to_string(),
                ));
            }
        }
        let sequence = event.sequence;
        self.next_sequence = event.sequence.saturating_add(1);
        if self.events.len() == EDITOR_MAX_EVENTS {
            self.events.pop_front();
        }
        self.view.cursor = Some(sequence);
        self.events.push_back(event);
        Ok(())
    }


    pub fn stream_after(&self, cursor: Option<u64>) -> Result<EditorStreamSnapshot, EditorHostError> {
        if let (Some(requested), Some(oldest)) = (cursor, self.events.front().map(|event| event.sequence)) {
            if requested.saturating_add(1) < oldest {
                return Err(EditorHostError::CursorExpired { requested, oldest });
            }
        }
        let events = self
            .events
            .iter()
            .filter(|event| cursor.map_or(true, |value| event.sequence > value))
            .cloned()
            .collect::<Vec<_>>();
        let next_cursor = events
            .last()
            .map(|event| event.sequence)
            .or(cursor)
            .or_else(|| self.latest_cursor());
        Ok(EditorStreamSnapshot {
            events,
            resumed_from: cursor,
            next_cursor,
        })
    }

    pub fn open(
        &mut self,
        request: &EditorWorkbenchOpenRequest,
    ) -> Result<EditorWorkbenchSnapshot, EditorHostError> {
        self.validate_request(
            request.host,
            &request.session_id,
            &request.source_id,
            &request.source_path,
            &request.revision,
        )?;
        let stream = self.stream_after(request.cursor)?;
        let requested_panel_id = request
            .panel_id
            .as_deref()
            .or_else(|| self.view.selection.as_ref().map(|selection| selection.panel_id.as_str()))
            .unwrap_or("build");
        let panel_id = ensure_panel(requested_panel_id)?;
        let item_key = match &request.item_key {
            Some(item_key) => {
                validate_stable_text(item_key, "selection item_key")?;
                Some(item_key.clone())
            }
            None if request.panel_id.is_some() => None,
            None => self
                .view
                .selection
                .as_ref()
                .and_then(|selection| selection.item_key.clone()),
        };
        self.view.selection = Some(JetDevtoolsSelection::new(panel_id, item_key));
        Ok(self.snapshot(request.host, stream))
    }

    pub fn reconnect(
        &self,
        request: &EditorReconnectRequest,
    ) -> Result<EditorWorkbenchSnapshot, EditorHostError> {
        self.validate_request(
            request.host,
            &request.session_id,
            &request.source_id,
            &request.source_path,
            &request.revision,
        )?;
        let stream = self.stream_after(request.cursor)?;
        Ok(self.snapshot(request.host, stream))
    }

    pub fn navigate_source(
        &self,
        request: &EditorSourceNavigationRequest,
    ) -> Result<EditorSourceNavigation, EditorHostError> {
        self.validate_request(
            request.host,
            &request.session_id,
            &request.source_id,
            &request.source_path,
            &request.revision,
        )?;
        Ok(EditorSourceNavigation {
            host: request.host,
            identity: self.identity.clone(),
            line: request.line,
            column: request.column,
            source_url: source_url(&self.workbench_url, &self.identity.source_id, request.line, request.column),
        })
    }

    pub fn select_panel(
        &mut self,
        request: &EditorPanelSelectionRequest,
    ) -> Result<JetDevtoolsSelection, EditorHostError> {
        self.validate_request(
            request.host,
            &request.session_id,
            &request.source_id,
            &request.source_path,
            &request.revision,
        )?;
        let panel_id = ensure_panel(&request.panel_id)?;
        if let Some(item_key) = &request.item_key {
            validate_stable_text(item_key, "selection item_key")?;
        }
        self.view.selection = Some(JetDevtoolsSelection::new(
            panel_id,
            request.item_key.clone(),
        ));
        Ok(self
            .view
            .selection
            .clone()
            .unwrap_or_else(|| JetDevtoolsSelection::new("build", None)))
    }

    pub fn command(
        &self,
        request: &EditorCommandRequest,
    ) -> Result<EditorCommandReceipt, EditorHostError> {
        self.validate_request(
            request.host,
            &request.session_id,
            &request.source_id,
            &request.source_path,
            &request.revision,
        )?;
        for argument in &request.arguments {
            validate_stable_text(argument, "command argument")?;
        }
        let required_grant = request.command.required_grant();
        if !self.grants.contains(required_grant) {
            return Err(EditorHostError::CommandDenied {
                command: request.command,
                required_grant: required_grant.to_string(),
            });
        }
        Ok(EditorCommandReceipt {
            host: request.host,
            identity: self.identity.clone(),
            command: request.command,
            required_grant: required_grant.to_string(),
        })
    }


    fn validate_request(
        &self,
        _host: EditorHost,
        session_id: &str,
        source_id: &str,
        source_path: &str,
        revision: &str,
    ) -> Result<(), EditorHostError> {
        validate_stable_text(session_id, "session_id")?;
        validate_source_identity(source_id)?;
        let source_path = clean_source_path(source_path)?;
        validate_stable_text(revision, "revision")?;
        if session_id != self.identity.session_id {
            return Err(EditorHostError::SessionMismatch {
                expected: self.identity.session_id.clone(),
                actual: session_id.to_string(),
            });
        }
        if source_id != self.identity.source_id {
            return Err(EditorHostError::SourceMismatch {
                expected: self.identity.source_id.clone(),
                actual: source_id.to_string(),
            });
        }
        if source_path != self.identity.source_path {
            return Err(EditorHostError::SourceMismatch {
                expected: self.identity.source_path.clone(),
                actual: source_path,
            });
        }
        if revision != self.identity.revision {
            return Err(EditorHostError::RevisionMismatch {
                expected: self.identity.revision.clone(),
                actual: revision.to_string(),
            });
        }
        Ok(())
    }

    fn snapshot(&self, host: EditorHost, stream: EditorStreamSnapshot) -> EditorWorkbenchSnapshot {
        EditorWorkbenchSnapshot {
            host,
            protocol: JET_DEVTOOLS_PROTOCOL,
            protocol_version: EDITOR_PROTOCOL_VERSION,
            workbench_url: self.workbench_url.clone(),
            identity: self.identity.clone(),
            started_at_ms: self.started_at_ms,
            panels: crate::Devtools::catalog::descriptors().to_vec(),
            panel_availability: self.panel_availability(),

            selection: self
                .view
                .selection
                .clone()
                .unwrap_or_else(|| JetDevtoolsSelection::new("build", None)),
            events: stream.events,
            resumed_from: stream.resumed_from,
            cursor: stream.next_cursor,
            time_cursor: self.view.cursor,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorWorkbenchOpenRequest {
    pub host: EditorHost,
    pub session_id: String,
    pub source_id: String,
    pub source_path: String,
    pub revision: String,
    pub panel_id: Option<String>,
    pub item_key: Option<String>,
    pub cursor: Option<u64>,
}

impl EditorWorkbenchOpenRequest {
    pub fn new(
        host: EditorHost,
        session_id: impl Into<String>,
        source_id: impl Into<String>,
        source_path: impl Into<String>,
        revision: impl Into<String>,
    ) -> Self {
        Self {
            host,
            session_id: session_id.into(),
            source_id: source_id.into(),
            source_path: source_path.into(),
            revision: revision.into(),
            panel_id: None,
            item_key: None,
            cursor: None,
        }
    }

    pub fn serialize(&self) -> String {
        let mut out = request_context_json(
            self.host,
            &self.session_id,
            &self.source_id,
            &self.source_path,
            &self.revision,
        );
        out.push_str(",\"panel_id\":");
        render_optional_string(self.panel_id.as_deref(), &mut out);
        out.push_str(",\"item_key\":");
        render_optional_string(self.item_key.as_deref(), &mut out);
        out.push_str(",\"cursor\":");
        render_optional_u64(self.cursor, &mut out);
        out.push('}');
        out
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorReconnectRequest {
    pub host: EditorHost,
    pub session_id: String,
    pub source_id: String,
    pub source_path: String,
    pub revision: String,
    pub cursor: Option<u64>,
}

impl EditorReconnectRequest {
    pub fn new(
        host: EditorHost,
        session_id: impl Into<String>,
        source_id: impl Into<String>,
        source_path: impl Into<String>,
        revision: impl Into<String>,
        cursor: Option<u64>,
    ) -> Self {
        Self {
            host,
            session_id: session_id.into(),
            source_id: source_id.into(),
            source_path: source_path.into(),
            revision: revision.into(),
            cursor,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorSourceNavigationRequest {
    pub host: EditorHost,
    pub session_id: String,
    pub source_id: String,
    pub source_path: String,
    pub revision: String,
    pub line: u32,
    pub column: u32,
}

impl EditorSourceNavigationRequest {
    pub fn new(
        host: EditorHost,
        session_id: impl Into<String>,
        source_id: impl Into<String>,
        source_path: impl Into<String>,
        revision: impl Into<String>,
        line: u32,
        column: u32,
    ) -> Self {
        Self {
            host,
            session_id: session_id.into(),
            source_id: source_id.into(),
            source_path: source_path.into(),
            revision: revision.into(),
            line,
            column,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorPanelSelectionRequest {
    pub host: EditorHost,
    pub session_id: String,
    pub source_id: String,
    pub source_path: String,
    pub revision: String,
    pub panel_id: String,
    pub item_key: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorCommandRequest {
    pub host: EditorHost,
    pub session_id: String,
    pub source_id: String,
    pub source_path: String,
    pub revision: String,
    pub command: EditorCommand,
    pub arguments: Vec<String>,
}

impl EditorCommandRequest {
    pub fn new(
        host: EditorHost,
        session_id: impl Into<String>,
        source_id: impl Into<String>,
        source_path: impl Into<String>,
        revision: impl Into<String>,
        command: EditorCommand,
    ) -> Self {
        Self {
            host,
            session_id: session_id.into(),
            source_id: source_id.into(),
            source_path: source_path.into(),
            revision: revision.into(),
            command,
            arguments: Vec::new(),
        }
    }
}


fn validate_stable_text(value: &str, label: &str) -> Result<(), EditorHostError> {
    if value.is_empty() || value.len() > EDITOR_MAX_TEXT_BYTES || value.chars().any(char::is_control) {
        return Err(EditorHostError::InvalidRequest(format!(
            "{label} must be nonempty, bounded, and free of control characters"
        )));
    }
    Ok(())
}

fn validate_source_identity(value: &str) -> Result<(), EditorHostError> {
    validate_stable_text(value, "source_id")?;
    if value.contains('\\') || value.starts_with('/') || value.as_bytes().get(1) == Some(&b':') {
        return Err(EditorHostError::SourcePathRejected(value.to_string()));
    }
    for component in value.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(EditorHostError::SourcePathRejected(value.to_string()));
        }
    }
    Ok(())
}

fn clean_source_path(value: &str) -> Result<String, EditorHostError> {
    if value.is_empty()
        || value.len() > EDITOR_MAX_TEXT_BYTES
        || value.contains('\\')
        || value.starts_with('/')
        || value.as_bytes().get(1) == Some(&b':')
        || value.chars().any(char::is_control)
    {
        return Err(EditorHostError::SourcePathRejected(value.to_string()));
    }
    let mut components = Vec::new();
    for component in value.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(EditorHostError::SourcePathRejected(value.to_string()));
        }
        components.push(component);
    }
    let path = components.join("/");
    if !path.ends_with(".jet") {
        return Err(EditorHostError::SourcePathRejected(path));
    }
    Ok(path)
}

fn validate_loopback_url(value: &str) -> Result<(), EditorHostError> {
    if value.is_empty()
        || value.len() > EDITOR_MAX_TEXT_BYTES
        || value.chars().any(|character| character.is_control() || character.is_ascii_whitespace())
    {
        return Err(EditorHostError::LoopbackViolation(
            "URL must be bounded and contain no whitespace".to_string(),
        ));
    }
    let Some(rest) = value.strip_prefix("http://") else {
        return Err(EditorHostError::LoopbackViolation(
            "only an http loopback URL is accepted".to_string(),
        ));
    };
    if rest.contains('?') || rest.contains('#') || rest.contains('@') {
        return Err(EditorHostError::LoopbackViolation(
            "workbench URL cannot contain query, fragment, or userinfo".to_string(),
        ));
    }
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = if let Some(rest) = authority.strip_prefix("[::1]:") {
        ("[::1]", rest)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        (host, port)
    } else {
        return Err(EditorHostError::LoopbackViolation(
            "URL must include a loopback port".to_string(),
        ));
    };
    if !matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
        return Err(EditorHostError::LoopbackViolation(format!("host `{host}` is not loopback")));
    }
    let port = port.parse::<u16>().map_err(|_| {
        EditorHostError::LoopbackViolation("URL port must be an integer from 1 to 65535".to_string())
    })?;
    if port == 0 {
        return Err(EditorHostError::LoopbackViolation(
            "URL port must be greater than zero".to_string(),
        ));
    }
    if path.contains("//") {
        return Err(EditorHostError::LoopbackViolation(
            "workbench URL path must be canonical".to_string(),
        ));
    }
    Ok(())
}

fn canonical_editor_panel_id(panel_id: &str) -> Option<&'static str> {
    crate::Devtools::catalog::descriptors()
        .iter()
        .find(|panel| panel.id.as_str() == panel_id)
        .map(|panel| panel.id.as_str())
}

fn editor_event_from_event(event: &JetDevtoolsEvent) -> Result<EditorStreamEvent, EditorHostError> {
    let mut fields = BTreeMap::new();
    fields.insert(
        "fields".to_string(),
        EditorFactValue::text(event.fields_json())?,
    );
    let payload = match event.payload_json() {
        Some(payload) => {
            let mut values = BTreeMap::new();
            values.insert("payload".to_string(), EditorFactValue::text(payload)?);
            Some(values)
        }
        None => None,
    };
    EditorStreamEvent::new(
        event.sequence(),
        event.timestamp_ms,
        event.source(),
        event.kind(),
        event.entity(),
        fields,
        payload,
    )
}

fn ensure_panel(panel_id: &str) -> Result<&'static str, EditorHostError> {
    canonical_editor_panel_id(panel_id)
        .ok_or_else(|| EditorHostError::PanelNotFound(panel_id.to_string()))
}

fn source_url(workbench_url: &str, source_id: &str, line: u32, column: u32) -> String {
    let mut out = workbench_url.trim_end_matches('/').to_string();
    out.push_str("?source_id=");
    percent_encode(source_id, &mut out);
    out.push_str("&line=");
    out.push_str(&line.to_string());
    out.push_str("&column=");
    out.push_str(&column.to_string());
    out
}

fn percent_encode(value: &str, out: &mut String) {
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(hex_digit(byte >> 4));
            out.push(hex_digit(byte & 0x0f));
        }
    }
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        _ => (b'A' + value - 10) as char,
    }
}

fn validate_fact_object(
    values: &BTreeMap<String, EditorFactValue>,
    label: &str,
) -> Result<(), EditorHostError> {
    if values.len() > EDITOR_MAX_HISTORY {
        return Err(EditorHostError::InvalidEnvelope(format!(
            "{label} exceeds the bounded field limit"
        )));
    }
    for (key, value) in values {
        validate_stable_text(key, "fact key")?;
        validate_fact_value(value)?;
    }
    Ok(())
}

fn validate_fact_value(value: &EditorFactValue) -> Result<(), EditorHostError> {
    match value {
        EditorFactValue::Float(value) if !value.is_finite() => Err(EditorHostError::InvalidEnvelope(
            "fact value contains a non-finite number".to_string(),
        )),
        EditorFactValue::Text(value) => validate_stable_text(value, "fact text"),
        EditorFactValue::Array(values) => {
            if values.len() > EDITOR_MAX_HISTORY {
                return Err(EditorHostError::InvalidEnvelope(
                    "fact array exceeds the bounded history limit".to_string(),
                ));
            }
            for value in values {
                validate_fact_value(value)?;
            }
            Ok(())
        }
        EditorFactValue::Object(values) => validate_fact_object(values, "fact object"),
        _ => Ok(()),
    }
}

fn request_context_json(
    host: EditorHost,
    session_id: &str,
    source_id: &str,
    source_path: &str,
    revision: &str,
) -> String {
    let mut out = String::from("{\"editor\":");
    render_string(host.wire_name(), &mut out);
    out.push_str(",\"session_id\":");
    render_string(session_id, &mut out);
    out.push_str(",\"source_id\":");
    render_string(source_id, &mut out);
    out.push_str(",\"source_path\":");
    render_string(source_path, &mut out);
    out.push_str(",\"revision\":");
    render_string(revision, &mut out);
    out
}

fn render_string(value: &str, out: &mut String) {
    out.push('"');
    out.push_str(&json_escape(value));
    out.push('"');
}

fn render_optional_string(value: Option<&str>, out: &mut String) {
    match value {
        Some(value) => render_string(value, out),
        None => out.push_str("null"),
    }
}

fn render_optional_u64(value: Option<u64>, out: &mut String) {
    match value {
        Some(value) => out.push_str(&value.to_string()),
        None => out.push_str("null"),
    }
}

fn render_fact_object(values: &BTreeMap<String, EditorFactValue>, out: &mut String) {
    out.push('{');
    for (index, (key, value)) in values.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        render_string(key, out);
        out.push(':');
        value.render(out);
    }
    out.push('}');
}


#[cfg(test)]
mod tests {
    use super::*;

    fn transport(grants: &[&str]) -> EditorHostTransport {
        EditorHostTransport::new(
            "http://127.0.0.1:4312/__jet_devtools",
            "session-1",
            "src/main.jet",
            "src/main.jet",
            "rev-1",
            grants,
        )
        .expect("transport")
    }

    fn open_request(host: EditorHost) -> EditorWorkbenchOpenRequest {
        EditorWorkbenchOpenRequest::new(
            host,
            "session-1",
            "src/main.jet",
            "src/main.jet",
            "rev-1",
        )
    }

    fn envelope() -> &'static str {
        r#"{"protocol":"jet.devtools.v1","session_id":"session-1","started_at_ms":10,"events":[{"sequence":1,"timestamp_ms":11,"source":"test","kind":"Build","entity":"main","fields":{"status":"ready"},"payload":null},{"sequence":2,"timestamp_ms":12,"source":"test","kind":"Trace","entity":"main","fields":{"duration_ms":3},"payload":{"published":"yes"}}],"history":[{"sequence":1,"timestamp_ms":11,"source":"test","kind":"Build","entity":"main","fields":{"status":"ready"},"payload":null},{"sequence":2,"timestamp_ms":12,"source":"test","kind":"Trace","entity":"main","fields":{"duration_ms":3},"payload":{"published":"yes"}}],"selection":{"panel_id":"build","item_key":null},"cursor":2}"#
    }

    #[test]
    fn one_transport_opens_both_editor_hosts_from_one_envelope() {
        let mut transport = transport(&[]);
        transport.replace_devtools_json(envelope()).expect("envelope");
        let vscode = transport.open(&open_request(EditorHost::VsCode)).expect("vscode");
        let zed = transport.open(&open_request(EditorHost::Zed)).expect("zed");
        assert_eq!(vscode.protocol, JET_DEVTOOLS_PROTOCOL);
        assert_eq!(vscode.events, zed.events);
        assert_eq!(vscode.panels, zed.panels);
        assert_eq!(vscode.host, EditorHost::VsCode);
        assert_eq!(zed.host, EditorHost::Zed);
        assert_eq!(vscode.events[0].payload, None);
        assert!(vscode.events[1].payload.is_some());
    }

    #[test]
    fn reconnect_cursor_is_bounded_and_deterministic() {
        let mut transport = transport(&[]);
        transport.replace_devtools_json(envelope()).expect("envelope");
        let request = EditorReconnectRequest::new(
            EditorHost::Zed,
            "session-1",
            "src/main.jet",
            "src/main.jet",
            "rev-1",
            Some(1),
        );
        let resumed = transport.reconnect(&request).expect("resume");
        assert_eq!(resumed.resumed_from, Some(1));
        assert_eq!(resumed.cursor, Some(2));
        assert_eq!(resumed.events.len(), 1);
        assert_eq!(resumed.events[0].sequence, 2);
    }

    #[test]
    fn stale_identity_path_and_revision_are_rejected() {
        let mut transport = transport(&[]);
        let mut request = open_request(EditorHost::VsCode);
        request.source_path = "../secret.jet".to_string();
        assert!(matches!(transport.open(&request), Err(EditorHostError::SourcePathRejected(_))));
        request.source_path = "src/main.jet".to_string();
        request.revision = "rev-2".to_string();
        assert!(matches!(transport.open(&request), Err(EditorHostError::RevisionMismatch { .. })));
    }

    #[test]
    fn commands_require_the_transport_grant() {
        let request = EditorCommandRequest::new(
            EditorHost::VsCode,
            "session-1",
            "src/main.jet",
            "src/main.jet",
            "rev-1",
            EditorCommand::Build,
        );
        assert!(matches!(transport(&[]).command(&request), Err(EditorHostError::CommandDenied { .. })));
        let receipt = transport(&["editor.command:build"])
            .command(&request)
            .expect("authorized command");
        assert_eq!(receipt.required_grant, "editor.command:build");
    }

    #[test]
    fn source_navigation_is_project_relative_and_loopback() {
        let transport = transport(&[]);
        let request = EditorSourceNavigationRequest::new(
            EditorHost::Zed,
            "session-1",
            "src/main.jet",
            "src/main.jet",
            "rev-1",
            4,
            2,
        );
        let navigation = transport.navigate_source(&request).expect("navigation");
        assert_eq!(navigation.source_url, "http://127.0.0.1:4312/__jet_devtools?source_id=src%2Fmain.jet&line=4&column=2");
    }

    #[test]
    fn lsp_json_response_keeps_json_rpc_id_and_typed_result() {
        let mut transport = transport(&[]);
        let request = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"{EDITOR_WORKBENCH_METHOD}\",\"params\":{{\"editor\":\"vscode\",\"session_id\":\"session-1\",\"source_id\":\"src/main.jet\",\"source_path\":\"src/main.jet\",\"revision\":\"rev-1\"}}}}"
        );
        let response = transport.handle_lsp_json(&request).expect("response");
        assert!(response.starts_with("{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{"));
        assert!(response.contains("\"protocol\":\"jet.devtools.v1\""));
    }
}
