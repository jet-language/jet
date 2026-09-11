//! Typed bridge from the resident devtools session to the native overlay host.
//!
//! The session is the only source of canonical events.  This adapter keeps
//! their typed identity and body intact, projects them into an explicit Event
//! Log panel, and carries separately supplied typed panels through unchanged.
//! It never parses `devtools_json`, chooses a panel policy, or executes an action.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use jet_foundation::Devtools::JetDevtoolsEvent;
use crate::NativeOverlayHost::{
    NativeOverlayAction, NativeOverlayEvent, NativeOverlayGrant, NativeOverlayNode,
    NativeOverlayNodeKind, NativeOverlayPanelFact, JET_DEVTOOLS_PROTOCOL, MAX_OVERLAY_DEPTH,
    MAX_OVERLAY_NODES, MAX_OVERLAY_PANELS, MAX_OVERLAY_TEXT_BYTES,
};
use crate::Session::{DevtoolsProjection, ResidentDevSession};
use crate::TerminalHost::{
    panel_availability as project_panel_availability, JetDevtoolsHostKind,
    JetDevtoolsPanelAvailability, JetDevtoolsPanelCapability, JetDevtoolsPanelDescriptor,
};


/// Stable panel identifier for the adapter's lossless generic event view.
pub const NATIVE_OVERLAY_EVENT_LOG_PANEL_ID: &str = "event-log";
/// Stable title for the adapter's lossless generic event view.
pub const NATIVE_OVERLAY_EVENT_LOG_TITLE: &str = "Event Log";
/// Stable status for the adapter's lossless generic event view.
pub const NATIVE_OVERLAY_EVENT_LOG_STATUS: &str = "canonical events";
/// Maximum number of canonical events retained by this adapter.
pub const MAX_NATIVE_OVERLAY_EVENTS: usize = 256;

const EVENT_LOG_NODES_PER_EVENT: usize = 7;

/// A reconnect-safe cursor for the canonical session stream.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NativeOverlayCursor {
    pub sequence: u64,
}

impl NativeOverlayCursor {
    pub const fn new(sequence: u64) -> Self {
        Self { sequence }
    }
}

/// Identity attached to every adapter update and request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeOverlayIdentity {
    pub protocol: String,
    pub session_id: String,
    pub revision: String,
    pub cursor: Option<NativeOverlayCursor>,
}

/// A reason a projection value is unavailable to the native overlay.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NativeOverlayUnavailableReason {
    EmptyStream,
    Reset,
    Truncated,
    EventLimit,
    NodeLimit,
    SelectionPanelUnavailable,
    NoCursor,
}

impl NativeOverlayUnavailableReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EmptyStream => "empty-stream",
            Self::Reset => "reset",
            Self::Truncated => "truncated",
            Self::EventLimit => "event-limit",
            Self::NodeLimit => "node-limit",
            Self::SelectionPanelUnavailable => "selection-panel-unavailable",
            Self::NoCursor => "no-cursor",
        }
    }
}

/// An explicit unavailable fact.  No fallback value is substituted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeOverlayUnavailableFact {
    pub subject: String,
    pub reason: NativeOverlayUnavailableReason,
}

/// A typed action request for the caller's existing Session action path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeOverlayActionRequest {
    pub protocol: String,
    pub session_id: String,
    pub revision: String,
    pub sequence: u64,
    pub panel_id: String,
    pub node_id: String,
    pub action_id: String,
    pub required_grant: NativeOverlayGrant,
}

impl NativeOverlayActionRequest {
    pub fn new(
        protocol: impl Into<String>,
        session_id: impl Into<String>,
        revision: impl Into<String>,
        sequence: u64,
        panel_id: impl Into<String>,
        node_id: impl Into<String>,
        action_id: impl Into<String>,
        required_grant: NativeOverlayGrant,
    ) -> Self {
        Self {
            protocol: protocol.into(),
            session_id: session_id.into(),
            revision: revision.into(),
            sequence,
            panel_id: panel_id.into(),
            node_id: node_id.into(),
            action_id: action_id.into(),
            required_grant,
        }
    }

    pub const fn cursor(&self) -> NativeOverlayCursor {
        NativeOverlayCursor::new(self.sequence)
    }
}

/// A typed selection request for the caller's existing Session selection path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeOverlaySelectionRequest {
    pub protocol: String,
    pub session_id: String,
    pub revision: String,
    pub sequence: u64,
    pub panel_id: String,
    pub item_key: String,
}

impl NativeOverlaySelectionRequest {
    pub fn new(
        protocol: impl Into<String>,
        session_id: impl Into<String>,
        revision: impl Into<String>,
        sequence: u64,
        panel_id: impl Into<String>,
        item_key: impl Into<String>,
    ) -> Self {
        Self {
            protocol: protocol.into(),
            session_id: session_id.into(),
            revision: revision.into(),
            sequence,
            panel_id: panel_id.into(),
            item_key: item_key.into(),
        }
    }

    pub const fn cursor(&self) -> NativeOverlayCursor {
        NativeOverlayCursor::new(self.sequence)
    }
}

/// One adapter update.  `event` is absent only when the canonical cursor is
/// empty; callers must not synthesize a sequence to submit to the host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeOverlayUpdate {
    pub identity: NativeOverlayIdentity,
    pub event: Option<NativeOverlayEvent>,
    pub reset: bool,
    pub truncation: bool,
    pub selection: Option<NativeOverlaySelectionRequest>,
    pub unavailable: Vec<NativeOverlayUnavailableFact>,
}

/// Alias used by callers that name the result a projection.
pub type NativeOverlayProjection = NativeOverlayUpdate;

/// Errors at the typed adapter boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeOverlayAdapterError {
    InvalidProtocol { expected: String, received: String },
    SessionMismatch { expected: String, received: String },
    InvalidRevision,
    InvalidSequence(u64),
    StaleRevision { expected: String, received: String },
    StaleSequence { expected: u64, received: u64 },
    InvalidText { field: String },
    DuplicatePanel(String),
    ReservedPanel(String),
    InvalidPanel(String),
    InvalidNode(String),
    HostRejected(String),
    MissingProjection,
    UnknownPanel(String),
    UnknownNode(String),
    ActionUnavailable(String),
    ActionMismatch(String),
}

impl std::fmt::Display for NativeOverlayAdapterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidProtocol { expected, received } => write!(
                formatter,
                "native overlay protocol `{received}` does not match `{expected}`"
            ),
            Self::SessionMismatch { expected, received } => write!(
                formatter,
                "native overlay session `{received}` does not match `{expected}`"
            ),
            Self::InvalidRevision => formatter.write_str("native overlay revision is empty, too long, or contains control text"),
            Self::InvalidSequence(sequence) => {
                write!(formatter, "native overlay sequence {sequence} is invalid")
            }
            Self::StaleRevision { expected, received } => write!(
                formatter,
                "native overlay revision `{received}` is stale; current revision is `{expected}`"
            ),
            Self::StaleSequence { expected, received } => write!(
                formatter,
                "native overlay sequence {received} is stale; current sequence is {expected}"
            ),
            Self::InvalidText { field } => {
                write!(formatter, "native overlay {field} is empty, too long, or contains control text")
            }
            Self::DuplicatePanel(panel_id) => {
                write!(formatter, "native overlay panel `{panel_id}` is duplicated")
            }
            Self::ReservedPanel(panel_id) => write!(
                formatter,
                "native overlay panel `{panel_id}` is reserved for the event log"
            ),
            Self::InvalidPanel(error) => formatter.write_str(error),
            Self::InvalidNode(error) => formatter.write_str(error),
            Self::HostRejected(error) => formatter.write_str(error),
            Self::MissingProjection => {
                formatter.write_str("native overlay has no current projection")
            }
            Self::UnknownPanel(panel_id) => {
                write!(formatter, "native overlay panel `{panel_id}` is unavailable")
            }
            Self::UnknownNode(node_id) => {
                write!(formatter, "native overlay node `{node_id}` is unavailable")
            }
            Self::ActionUnavailable(node_id) => {
                write!(formatter, "native overlay node `{node_id}` has no action")
            }
            Self::ActionMismatch(action_id) => write!(
                formatter,
                "native overlay action `{action_id}` is stale or no longer matches the node"
            ),
        }
    }
}

impl std::error::Error for NativeOverlayAdapterError {}

/// Stateful bridge bound to one resident session identity.
#[derive(Clone, Debug)]
pub struct NativeOverlayAdapter {
    session_id: String,
    revision: Option<String>,
    sequence: Option<u64>,
    release_build: bool,
    event_facts: VecDeque<JetDevtoolsEvent>,
    typed_panels: BTreeMap<String, NativeOverlayPanelFact>,
    history_truncated: bool,
    last_update: Option<NativeOverlayUpdate>,
}

impl NativeOverlayAdapter {
    /// Bind an adapter to a resident session id.  Identity is validated on the
    /// first projection instead of accepting a synthetic session fallback.
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            revision: None,
            sequence: None,
            release_build: false,
            event_facts: VecDeque::new(),
            typed_panels: BTreeMap::new(),
            history_truncated: false,
            last_update: None,
        }
    }

    pub fn from_projection(
        projection: &DevtoolsProjection,
        typed_panels: &[NativeOverlayPanelFact],
    ) -> Result<NativeOverlayUpdate, NativeOverlayAdapterError> {
        project_projection(projection, typed_panels)
    }

    pub fn for_session(session: &ResidentDevSession) -> Self {
        Self::new(session.id())
    }
    /// Bind an adapter whose devtools surface is excluded from a release
    /// build.  The catalog remains queryable so callers can explain absence.
    pub fn with_release_build(mut self, release_build: bool) -> Self {
        self.set_release_build(release_build);
        self
    }

    pub fn release_build(&self) -> bool {
        self.release_build
    }

    pub fn set_release_build(&mut self, release_build: bool) {
        self.release_build = release_build;
        if release_build {
            self.revision = None;
            self.sequence = None;
            self.event_facts.clear();
            self.typed_panels.clear();
            self.history_truncated = false;
            self.last_update = None;
        }
    }

    /// Native overlays use the shared descriptor catalog; typed panel payloads
    /// are an optional projection of those rows, never a second registry.
    pub fn panel_catalog(&self) -> &'static [JetDevtoolsPanelDescriptor] {
        crate::Devtools::catalog::descriptors()
    }

    pub fn panel_availability(&self) -> Vec<JetDevtoolsPanelAvailability> {
        let grants: &[JetDevtoolsPanelCapability] = if self.release_build {
            &[]
        } else {
            &JetDevtoolsPanelCapability::ALL
        };
        project_panel_availability(JetDevtoolsHostKind::NativeOverlay, grants)
    }


    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn sequence(&self) -> Option<u64> {
        self.sequence
    }

    pub fn revision(&self) -> Option<&str> {
        self.revision.as_deref()
    }

    pub fn cursor(&self) -> Option<NativeOverlayCursor> {
        self.sequence.map(NativeOverlayCursor::new)
    }

    pub fn last_update(&self) -> Option<&NativeOverlayUpdate> {
        self.last_update.as_ref()
    }

    /// Replace the separate, already-typed panel facts.  This method preserves
    /// every node and action; it does not derive panel semantics from names.
    pub fn set_typed_panels(
        &mut self,
        panels: &[NativeOverlayPanelFact],
    ) -> Result<(), NativeOverlayAdapterError> {
        if self.release_build {
            return Err(NativeOverlayAdapterError::HostRejected(
                "native overlay is excluded from release builds".to_string(),
            ));
        }
        let next = typed_panel_map(panels)?;
        self.typed_panels = next;
        Ok(())
    }

    pub fn clear_typed_panels(&mut self) {
        self.typed_panels.clear();
    }

    /// Apply one canonical typed projection.
    pub fn apply_projection(
        &mut self,
        projection: DevtoolsProjection,
    ) -> Result<NativeOverlayUpdate, NativeOverlayAdapterError> {
        self.apply_projection_internal(projection, false)
    }

    fn apply_projection_internal(
        &mut self,
        projection: DevtoolsProjection,
        retain_typed_panels: bool,
    ) -> Result<NativeOverlayUpdate, NativeOverlayAdapterError> {
        if self.release_build {
            return Err(NativeOverlayAdapterError::HostRejected(
                "native overlay is excluded from release builds".to_string(),
            ));
        }
        validate_projection_identity(&projection, &self.session_id)?;
        validate_revision(&projection.revision, projection.sequence == 0)?;
        self.check_projection_order(&projection)?;
        let revision_changed = self.revision.as_deref() != Some(projection.revision.as_str());
        let effective_reset = projection.reset || revision_changed;
        if effective_reset {
            self.event_facts.clear();
            self.history_truncated = false;
            if !retain_typed_panels {
                self.typed_panels.clear();
            }
        }
        let mut events = projection.panels.clone();
        events.sort_by(|left, right| {
            left.sequence()
                .cmp(&right.sequence())
                .then_with(|| left.timestamp_ms.cmp(&right.timestamp_ms))
                .then_with(|| left.source().cmp(right.source()))
                .then_with(|| left.kind().cmp(right.kind()))
                .then_with(|| left.entity().cmp(right.entity()))
        });
        let mut previous_sequence = None;
        for event in &events {
            validate_event(event, &projection)?;
            if previous_sequence == Some(event.sequence()) {
                return Err(NativeOverlayAdapterError::InvalidSequence(event.sequence()));
            }
            if !projection.reset && !revision_changed {
                if let Some(current_sequence) = self.sequence {
                    if event.sequence() <= current_sequence {
                        return Err(NativeOverlayAdapterError::StaleSequence {
                            expected: current_sequence,
                            received: event.sequence(),
                        });
                    }
                }
            }
            previous_sequence = Some(event.sequence());
        }
        for event in events {
            self.event_facts.push_back(event);
        }
        while self.event_facts.len() > MAX_NATIVE_OVERLAY_EVENTS {
            self.event_facts.pop_front();
            self.history_truncated = true;
        }

        self.revision = Some(projection.revision.clone());
        self.sequence = (projection.sequence != 0).then_some(projection.sequence);

        let mut unavailable = Vec::new();
        if effective_reset {
            unavailable.push(make_unavailable("session", NativeOverlayUnavailableReason::Reset));
        }
        if projection.truncation {
            unavailable.push(make_unavailable(
                "session",
                NativeOverlayUnavailableReason::Truncated,
            ));
        }
        if self.history_truncated {
            unavailable.push(make_unavailable(
                NATIVE_OVERLAY_EVENT_LOG_PANEL_ID,
                NativeOverlayUnavailableReason::EventLimit,
            ));
        }

        let event = if let Some(sequence) = self.sequence {
            let (panels, dropped_for_nodes) = self.render_panels(sequence)?;
            if dropped_for_nodes {
                unavailable.push(make_unavailable(
                    NATIVE_OVERLAY_EVENT_LOG_PANEL_ID,
                    NativeOverlayUnavailableReason::NodeLimit,
                ));
            }
            let event = NativeOverlayEvent::new(
                JET_DEVTOOLS_PROTOCOL,
                self.session_id.clone(),
                projection.revision.clone(),
                sequence,
                projection.observed_at,
                panels,
            );
            event
                .validate()
                .map_err(NativeOverlayAdapterError::HostRejected)?;
            Some(event)
        } else {
            unavailable.push(make_unavailable(
                "session",
                NativeOverlayUnavailableReason::EmptyStream,
            ));
            None
        };

        let selection = projection.selection.map(|selection| {
            let Some(sequence) = self.sequence else {
                unavailable.push(make_unavailable(
                    selection.panel_id.as_str(),
                    NativeOverlayUnavailableReason::NoCursor,
                ));
                return None;
            };
            if !self.typed_panels.contains_key(&selection.panel_id)
                && selection.panel_id != NATIVE_OVERLAY_EVENT_LOG_PANEL_ID
            {
                unavailable.push(make_unavailable(
                    selection.panel_id.as_str(),
                    NativeOverlayUnavailableReason::SelectionPanelUnavailable,
                ));
            }
            Some(NativeOverlaySelectionRequest::new(
                JET_DEVTOOLS_PROTOCOL,
                self.session_id.clone(),
                projection.revision.clone(),
                sequence,
                selection.panel_id,
                selection.item_key,
            ))
        });
        let selection = selection.flatten();
        let identity = NativeOverlayIdentity {
            protocol: JET_DEVTOOLS_PROTOCOL.to_string(),
            session_id: self.session_id.clone(),
            revision: projection.revision,
            cursor: self.cursor(),
        };
        let update = NativeOverlayUpdate {
            identity,
            event,
            reset: effective_reset,
            truncation: projection.truncation,
            selection,
            unavailable,
        };
        self.last_update = Some(update.clone());
        Ok(update)
    }

    /// Set separate typed panels and apply one canonical projection.
    pub fn apply_projection_with_panels(
        &mut self,
        projection: DevtoolsProjection,
        panels: &[NativeOverlayPanelFact],
    ) -> Result<NativeOverlayUpdate, NativeOverlayAdapterError> {
        self.set_typed_panels(panels)?;
        self.apply_projection_internal(projection, true)
    }

    /// Pull the next typed projection from the bound resident session.
    pub fn sync_session(
        &mut self,
        session: &ResidentDevSession,
    ) -> Result<NativeOverlayUpdate, NativeOverlayAdapterError> {
        if session.id() != self.session_id {
            return Err(NativeOverlayAdapterError::SessionMismatch {
                expected: self.session_id.clone(),
                received: session.id().to_string(),
            });
        }
        self.apply_projection(session.devtools_events_since(self.sequence))
    }

    pub fn sync(
        &mut self,
        session: &ResidentDevSession,
    ) -> Result<NativeOverlayUpdate, NativeOverlayAdapterError> {
        self.sync_session(session)
    }

    /// Create an action request from the current typed node metadata.
    pub fn action_request(
        &self,
        panel_id: &str,
        node_id: &str,
    ) -> Result<NativeOverlayActionRequest, NativeOverlayAdapterError> {
        let (revision, sequence) = self.current_context()?;
        let node = self.find_node(panel_id, node_id)?;
        let action = node
            .action
            .as_ref()
            .ok_or_else(|| NativeOverlayAdapterError::ActionUnavailable(node_id.to_string()))?;
        Ok(NativeOverlayActionRequest::new(
            JET_DEVTOOLS_PROTOCOL,
            self.session_id.clone(),
            revision,
            sequence,
            panel_id,
            node_id,
            action.action_id.clone(),
            action.required_grant,
        ))
    }

    /// Validate and return an action request for the caller to submit.
    pub fn request_action(
        &self,
        request: NativeOverlayActionRequest,
    ) -> Result<NativeOverlayActionRequest, NativeOverlayAdapterError> {
        self.validate_action_request(&request)?;
        Ok(request)
    }

    pub fn validate_action_request(
        &self,
        request: &NativeOverlayActionRequest,
    ) -> Result<(), NativeOverlayAdapterError> {
        self.validate_request_identity(
            &request.protocol,
            &request.session_id,
            &request.revision,
            request.sequence,
        )?;
        let node = self.find_node(&request.panel_id, &request.node_id)?;
        let action = node
            .action
            .as_ref()
            .ok_or_else(|| NativeOverlayAdapterError::ActionUnavailable(request.node_id.clone()))?;
        if action.action_id != request.action_id || action.required_grant != request.required_grant
        {
            return Err(NativeOverlayAdapterError::ActionMismatch(
                request.action_id.clone(),
            ));
        }
        Ok(())
    }

    /// Create a selection request with the current identity, preserving the
    /// supplied item key exactly.
    pub fn selection_request(
        &self,
        panel_id: &str,
        item_key: &str,
    ) -> Result<NativeOverlaySelectionRequest, NativeOverlayAdapterError> {
        validate_text(panel_id, "selection panel id", false)?;
        validate_text(item_key, "selection item key", true)?;
        let (revision, sequence) = self.current_context()?;
        Ok(NativeOverlaySelectionRequest::new(
            JET_DEVTOOLS_PROTOCOL,
            self.session_id.clone(),
            revision,
            sequence,
            panel_id,
            item_key,
        ))
    }

    pub fn request_selection(
        &self,
        request: NativeOverlaySelectionRequest,
    ) -> Result<NativeOverlaySelectionRequest, NativeOverlayAdapterError> {
        self.validate_selection_request(&request)?;
        Ok(request)
    }

    pub fn validate_selection_request(
        &self,
        request: &NativeOverlaySelectionRequest,
    ) -> Result<(), NativeOverlayAdapterError> {
        self.validate_request_identity(
            &request.protocol,
            &request.session_id,
            &request.revision,
            request.sequence,
        )?;
        validate_text(&request.panel_id, "selection panel id", false)?;
        validate_text(&request.item_key, "selection item key", true)?;
        if !self.typed_panels.contains_key(&request.panel_id)
            && request.panel_id != NATIVE_OVERLAY_EVENT_LOG_PANEL_ID
        {
            return Err(NativeOverlayAdapterError::UnknownPanel(
                request.panel_id.clone(),
            ));
        }
        Ok(())
    }

    fn current_context(&self) -> Result<(String, u64), NativeOverlayAdapterError> {
        let revision = self
            .revision
            .clone()
            .ok_or(NativeOverlayAdapterError::MissingProjection)?;
        let sequence = self.sequence.ok_or(NativeOverlayAdapterError::MissingProjection)?;
        Ok((revision, sequence))
    }

    fn validate_request_identity(
        &self,
        protocol: &str,
        session_id: &str,
        revision: &str,
        sequence: u64,
    ) -> Result<(), NativeOverlayAdapterError> {
        if protocol != JET_DEVTOOLS_PROTOCOL {
            return Err(NativeOverlayAdapterError::InvalidProtocol {
                expected: JET_DEVTOOLS_PROTOCOL.to_string(),
                received: protocol.to_string(),
            });
        }
        if session_id != self.session_id {
            return Err(NativeOverlayAdapterError::SessionMismatch {
                expected: self.session_id.clone(),
                received: session_id.to_string(),
            });
        }
        let (current_revision, current_sequence) = self.current_context()?;
        if revision != current_revision {
            return Err(NativeOverlayAdapterError::StaleRevision {
                expected: current_revision,
                received: revision.to_string(),
            });
        }
        if sequence != current_sequence {
            return Err(NativeOverlayAdapterError::StaleSequence {
                expected: current_sequence,
                received: sequence,
            });
        }
        Ok(())
    }

    fn check_projection_order(
        &self,
        projection: &DevtoolsProjection,
    ) -> Result<(), NativeOverlayAdapterError> {
        let Some(current_sequence) = self.sequence else {
            return Ok(());
        };
        if let Some(current_revision) = self.revision.as_deref() {
            if projection.revision != current_revision
                && projection.sequence <= current_sequence
            {
                return Err(NativeOverlayAdapterError::StaleRevision {
                    expected: current_revision.to_string(),
                    received: projection.revision.clone(),
                });
            }
        }
        if projection.sequence < current_sequence {
            return Err(NativeOverlayAdapterError::StaleSequence {
                expected: current_sequence,
                received: projection.sequence,
            });
        }
        if projection.sequence == current_sequence
            && self.revision.as_deref() == Some(projection.revision.as_str())
            && !projection.panels.is_empty()
            && !projection.reset
        {
            return Err(NativeOverlayAdapterError::StaleSequence {
                expected: current_sequence,
                received: projection.sequence,
            });
        }
        Ok(())
    }

    fn find_node(
        &self,
        panel_id: &str,
        node_id: &str,
    ) -> Result<&NativeOverlayNode, NativeOverlayAdapterError> {
        let panel = self
            .typed_panels
            .get(panel_id)
            .ok_or_else(|| NativeOverlayAdapterError::UnknownPanel(panel_id.to_string()))?;
        find_node_in(panel.nodes.as_slice(), node_id)
            .ok_or_else(|| NativeOverlayAdapterError::UnknownNode(node_id.to_string()))
    }

    fn render_panels(
        &self,
        sequence: u64,
    ) -> Result<(Vec<NativeOverlayPanelFact>, bool), NativeOverlayAdapterError> {
        let typed_node_count = self
            .typed_panels
            .values()
            .map(panel_node_count)
            .sum::<usize>();
        let mut panels: Vec<NativeOverlayPanelFact> = self.typed_panels.values().cloned().collect();
        let mut dropped_for_nodes = false;
        if !self.event_facts.is_empty() {
            let available = MAX_OVERLAY_NODES.saturating_sub(typed_node_count);
            let max_events = available / EVENT_LOG_NODES_PER_EVENT;
            let skip = self.event_facts.len().saturating_sub(max_events);
            if skip != 0 {
                dropped_for_nodes = true;
            }
            let facts = self
                .event_facts
                .iter()
                .skip(skip)
                .collect::<Vec<_>>();
            if !facts.is_empty() {
                panels.push(event_log_panel(facts.as_slice(), sequence)?);
            }
        }
        if panels.len() > MAX_OVERLAY_PANELS {
            return Err(NativeOverlayAdapterError::InvalidPanel(
                "native overlay projection exceeds its panel budget".to_string(),
            ));
        }
        Ok((panels, dropped_for_nodes))
    }
}

/// Project one standalone canonical projection without retaining adapter state.
pub fn project_projection(
    projection: &DevtoolsProjection,
    typed_panels: &[NativeOverlayPanelFact],
) -> Result<NativeOverlayUpdate, NativeOverlayAdapterError> {
    let mut adapter = NativeOverlayAdapter::new(projection.session_id.clone());
    adapter.apply_projection_with_panels(projection.clone(), typed_panels)
}

/// Convert one canonical event to a lossless Event Log node.
pub fn node_from_event(
    event: &JetDevtoolsEvent,
) -> Result<NativeOverlayNode, NativeOverlayAdapterError> {
    if event.sequence() == 0 {
        return Err(NativeOverlayAdapterError::InvalidSequence(event.sequence()));
    }
    validate_event_text(event)?;
    let mut children = vec![
        NativeOverlayNode::text("source", "source").with_value(event.source().to_string()),
        NativeOverlayNode::text("kind", "kind").with_value(event.kind().to_string()),
        NativeOverlayNode::text("entity", "entity").with_value(event.entity().to_string()),
        NativeOverlayNode::text("observed_at", "observed_at")
            .with_value(event.timestamp_ms.to_string()),
        NativeOverlayNode::text("fields", "fields").with_value(event.fields_json().to_string()),
    ];
    if let Some(payload) = event.payload_json() {
        children.push(NativeOverlayNode::text("payload", "payload").with_value(payload.to_string()));
    }
    Ok(
        NativeOverlayNode::new(
            format!("event-{}", event.sequence()),
            NativeOverlayNodeKind::Status,
            "event",
        )
        .with_value(event.entity().to_string())
        .with_status(event.kind().to_string())
        .with_children(children),
    )
}

/// Convert canonical events to the explicit generic Event Log panel.
pub fn event_log_panel(
    events: &[&JetDevtoolsEvent],
    sequence: u64,
) -> Result<NativeOverlayPanelFact, NativeOverlayAdapterError> {
    if sequence == 0 {
        return Err(NativeOverlayAdapterError::InvalidSequence(sequence));
    }
    if events.len() > MAX_NATIVE_OVERLAY_EVENTS
        || events.len() > MAX_OVERLAY_NODES / EVENT_LOG_NODES_PER_EVENT
    {
        return Err(NativeOverlayAdapterError::InvalidPanel(
            "native overlay event log exceeds its event or node budget".to_string(),
        ));
    }
    let nodes = events
        .iter()
        .map(|event| node_from_event(event))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(NativeOverlayPanelFact::new(
        NATIVE_OVERLAY_EVENT_LOG_PANEL_ID,
        NATIVE_OVERLAY_EVENT_LOG_TITLE,
        sequence,
        NATIVE_OVERLAY_EVENT_LOG_STATUS,
        nodes,
    ))
}

fn validate_projection_identity(
    projection: &DevtoolsProjection,
    session_id: &str,
) -> Result<(), NativeOverlayAdapterError> {
    if projection.protocol != JET_DEVTOOLS_PROTOCOL {
        return Err(NativeOverlayAdapterError::InvalidProtocol {
            expected: JET_DEVTOOLS_PROTOCOL.to_string(),
            received: projection.protocol.clone(),
        });
    }
    if projection.session_id != session_id {
        return Err(NativeOverlayAdapterError::SessionMismatch {
            expected: session_id.to_string(),
            received: projection.session_id.clone(),
        });
    }
    validate_text(&projection.session_id, "session id", false)?;
    Ok(())
}

fn validate_revision(
    revision: &str,
    allow_empty: bool,
) -> Result<(), NativeOverlayAdapterError> {
    if !allow_empty && revision.is_empty() {
        return Err(NativeOverlayAdapterError::InvalidRevision);
    }
    validate_text(revision, "revision", true)
}

fn validate_event(
    event: &JetDevtoolsEvent,
    projection: &DevtoolsProjection,
) -> Result<(), NativeOverlayAdapterError> {
    if event.sequence() == 0 || event.sequence() > projection.sequence {
        return Err(NativeOverlayAdapterError::InvalidSequence(event.sequence()));
    }
    validate_event_text(event)
}

fn validate_event_text(event: &JetDevtoolsEvent) -> Result<(), NativeOverlayAdapterError> {
    validate_text(event.source(), "event source", false)?;
    validate_text(event.kind(), "event kind", false)?;
    validate_text(event.entity(), "event entity", false)?;
    validate_text(event.fields_json(), "event fields", true)?;
    if let Some(payload) = event.payload_json() {
        validate_text(payload, "event payload", true)?;
    }
    Ok(())
}

fn typed_panel_map(
    panels: &[NativeOverlayPanelFact],
) -> Result<BTreeMap<String, NativeOverlayPanelFact>, NativeOverlayAdapterError> {
    if panels.len() > MAX_OVERLAY_PANELS {
        return Err(NativeOverlayAdapterError::InvalidPanel(
            "native overlay typed panels exceed their panel budget".to_string(),
        ));
    }
    let mut output = BTreeMap::new();
    let mut count = 0;
    for panel in panels {
        if panel.panel_id == NATIVE_OVERLAY_EVENT_LOG_PANEL_ID {
            return Err(NativeOverlayAdapterError::ReservedPanel(panel.panel_id.clone()));
        }
        if output.contains_key(&panel.panel_id) {
            return Err(NativeOverlayAdapterError::DuplicatePanel(panel.panel_id.clone()));
        }
        validate_panel(panel, &mut count)?;
        output.insert(panel.panel_id.clone(), panel.clone());
    }
    Ok(output)
}

fn validate_panel(
    panel: &NativeOverlayPanelFact,
    count: &mut usize,
) -> Result<(), NativeOverlayAdapterError> {
    validate_text(&panel.panel_id, "panel id", false)?;
    validate_text(&panel.title, "panel title", false)?;
    validate_text(&panel.status, "panel status", true)?;
    let mut ids = BTreeSet::new();
    for node in &panel.nodes {
        validate_node(node, 0, count, &mut ids)?;
    }
    Ok(())
}

fn validate_node(
    node: &NativeOverlayNode,
    depth: usize,
    count: &mut usize,
    ids: &mut BTreeSet<String>,
) -> Result<(), NativeOverlayAdapterError> {
    if depth > MAX_OVERLAY_DEPTH {
        return Err(NativeOverlayAdapterError::InvalidNode(
            "native overlay node tree exceeds its depth budget".to_string(),
        ));
    }
    validate_text(&node.id, "node id", false)?;
    validate_text(&node.label, "node label", false)?;
    if !ids.insert(node.id.clone()) {
        return Err(NativeOverlayAdapterError::InvalidNode(format!(
            "native overlay node id `{}` is duplicated",
            node.id
        )));
    }
    *count = count.saturating_add(1);
    if *count > MAX_OVERLAY_NODES {
        return Err(NativeOverlayAdapterError::InvalidNode(
            "native overlay projection exceeds its node budget".to_string(),
        ));
    }
    if let Some(value) = node.value.as_deref() {
        validate_text(value, "node value", true)?;
    }
    if let Some(status) = node.status.as_deref() {
        validate_text(status, "node status", true)?;
    }
    match (&node.kind, &node.action) {
        (NativeOverlayNodeKind::Action, None) => {
            return Err(NativeOverlayAdapterError::InvalidNode(format!(
                "native overlay action node `{}` has no typed action",
                node.id
            )))
        }
        (NativeOverlayNodeKind::Action, Some(action)) => {
            validate_action(action)?;
        }
        (_, Some(_)) => {
            return Err(NativeOverlayAdapterError::InvalidNode(format!(
                "native overlay node `{}` carries an action but is not an action node",
                node.id
            )))
        }
        (_, None) => {}
    }
    for child in &node.children {
        validate_node(child, depth + 1, count, ids)?;
    }
    Ok(())
}

fn validate_action(action: &NativeOverlayAction) -> Result<(), NativeOverlayAdapterError> {
    validate_text(&action.action_id, "action id", false)
}

fn validate_text(
    value: &str,
    field: &str,
    allow_empty: bool,
) -> Result<(), NativeOverlayAdapterError> {
    if (!allow_empty && value.is_empty())
        || value.len() > MAX_OVERLAY_TEXT_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(NativeOverlayAdapterError::InvalidText {
            field: field.to_string(),
        });
    }
    Ok(())
}

fn panel_node_count(panel: &NativeOverlayPanelFact) -> usize {
    panel.nodes.iter().map(node_count).sum()
}

fn node_count(node: &NativeOverlayNode) -> usize {
    1 + node.children.iter().map(node_count).sum::<usize>()
}

fn find_node<'a>(nodes: &'a [NativeOverlayNode], node_id: &str) -> Option<&'a NativeOverlayNode> {
    nodes.iter().find_map(|node| {
        if node.id == node_id {
            Some(node)
        } else {
            find_node(node.children.as_slice(), node_id)
        }
    })
}

fn find_node_in<'a>(
    nodes: &'a [NativeOverlayNode],
    node_id: &str,
) -> Option<&'a NativeOverlayNode> {
    find_node(nodes, node_id)
}

fn make_unavailable(
    subject: impl Into<String>,
    reason: NativeOverlayUnavailableReason,
) -> NativeOverlayUnavailableFact {
    NativeOverlayUnavailableFact {
        subject: subject.into(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use crate::Session::{
        DevtoolsCommandCapabilities, DevtoolsSessionState, DevtoolsStatusFact,
    };
    use super::*;

    fn projection(
        revision: &str,
        sequence: u64,
        events: Vec<JetDevtoolsEvent>,
    ) -> DevtoolsProjection {
        DevtoolsProjection {
            protocol: JET_DEVTOOLS_PROTOCOL.to_string(),
            session_id: "session".to_string(),
            source_id: None,
            build_id: None,
            revision: revision.to_string(),
            world_id: None,
            sequence,
            observed_at: 99,
            panels: events,
            selection: None,
            status: DevtoolsStatusFact {
                state: DevtoolsSessionState::Unavailable,
                diagnostic_code: None,
                diagnostic: None,
                accepted_revision: None,
                last_good_revision: None,
                run_target: None,
                run_output: None,
                test_state: None,
            },
            command_capabilities: DevtoolsCommandCapabilities {
                project_rebuild: false,
                project_rebuild_reason: Some(
                    "native overlay test projection has no rebuild executor".to_string(),
                ),
            },
            command_receipts: Vec::new(),
            reset: false,
            truncation: false,
        }
    }

    fn fact(_revision: &str, sequence: u64) -> JetDevtoolsEvent {
        let event = JetDevtoolsEvent::from_parts(
            sequence,
            "session",
            "build",
            "program",
            "{\"ok\":true}",
        )
        .unwrap()
        .publish(jet_foundation::Devtools::JetDevtoolsPayload::one(
            "message",
            jet_foundation::Devtools::JetDevtoolsValue::text("opaque payload"),
        ))
        .unwrap();
        let mut envelope = jet_foundation::Devtools::JetDevtoolsEnvelope::new("session", 0);
        envelope.push(event);
        let events = envelope.events().cloned().collect::<Vec<_>>();
        events.into_iter().next().unwrap()
    }

    fn action_panel() -> NativeOverlayPanelFact {
        NativeOverlayPanelFact::new(
            "Build",
            "Build",
            1,
            "ready",
            vec![NativeOverlayNode::new(
                "run",
                NativeOverlayNodeKind::Action,
                "run",
            )
            .with_action(NativeOverlayAction::new(
                "build.run",
                NativeOverlayGrant::Control,
            ))],
        )
    }

    #[test]
    fn projects_canonical_event_without_json_parsing() {
        let update = project_projection(&projection("rev-1", 1, vec![fact("rev-1", 1)]), &[])
            .unwrap();
        let event = update.event.unwrap();
        let panel = event
            .panels
            .iter()
            .find(|panel| panel.panel_id == NATIVE_OVERLAY_EVENT_LOG_PANEL_ID)
            .unwrap();
        let root = &panel.nodes[0];
        assert_eq!(root.status.as_deref(), Some("build"));
        assert_eq!(root.children[4].value.as_deref(), Some("{\"ok\":true}"));
        assert_eq!(
            root.children[5].value.as_deref(),
            Some("{\"fields\":{\"message\":\"opaque payload\"}}")
        );
    }

    #[test]
    fn action_request_rejects_stale_revision() {
        let mut adapter = NativeOverlayAdapter::new("session");
        let update = adapter
            .apply_projection_with_panels(projection("rev-1", 1, vec![fact("rev-1", 1)]), &[action_panel()])
            .unwrap();
        let request = adapter.action_request("Build", "run").unwrap();
        let second = adapter
            .apply_projection_with_panels(projection("rev-2", 2, vec![fact("rev-2", 2)]), &[action_panel()])
            .unwrap();
        assert_eq!(update.identity.revision, "rev-1");
        assert!(second.reset);
        assert!(matches!(
            adapter.validate_action_request(&request),
            Err(NativeOverlayAdapterError::StaleRevision { .. })
        ));
    }

    #[test]
    fn reset_and_truncation_are_preserved() {
        let mut input = projection("rev-1", 1, vec![fact("rev-1", 1)]);
        input.reset = true;
        input.truncation = true;
        let update = project_projection(&input, &[]).unwrap();
        assert!(update.reset);
        assert!(update.truncation);
        assert!(update
            .unavailable
            .iter()
            .any(|fact| fact.reason == NativeOverlayUnavailableReason::Reset));
    }
}
