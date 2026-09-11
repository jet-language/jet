//! Typed browser-host projection for the shared `jet.devtools.v1` session.
//!
//! `Session` owns protocol decoding and supplies [`BrowserHostEvent`] values to
//! this module.  This module deliberately has no wire decoder, renderer, or
//! browser runtime.  It keeps the two browser placements on one state machine,
//! retains a bounded reconnect history, and accepts source/action requests only
//! when their typed grants still belong to the current session and revision.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

/// The one protocol accepted by every devtools host.
pub const BROWSER_DEVTOOLS_PROTOCOL: &str = "jet.devtools.v1";
/// Compatibility spelling for callers that name the protocol after the wire.
pub const JET_DEVTOOLS_PROTOCOL: &str = BROWSER_DEVTOOLS_PROTOCOL;

/// Maximum number of event snapshots retained for a reconnecting browser.
pub const MAX_RECONNECT_EVENTS: usize = 256;
/// Maximum number of panels in one event.
pub const MAX_BROWSER_PANELS: usize = 128;
/// Maximum number of nodes in one event, including descendants.
pub const MAX_BROWSER_NODES: usize = 4096;
/// Maximum nesting depth of a typed node tree.
pub const MAX_BROWSER_NODE_DEPTH: usize = 64;
/// Maximum length of an identity, title, label, or status string.
pub const MAX_BROWSER_TEXT: usize = 256;
/// Maximum length of an explicitly published node value.
pub const MAX_BROWSER_VALUE: usize = 4096;

/// Browser placement backed by the same host state.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum BrowserPlacement {
    /// The compact surface injected into the running application.
    #[default]
    InApp,
    /// The full `jet dev` workbench page.
    Workbench,
}

impl BrowserPlacement {
    /// Stable wire/view spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InApp => "in-app",
            Self::Workbench => "workbench",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::InApp => 0,
            Self::Workbench => 1,
        }
    }
}

/// Closed node vocabulary for browser view facts.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum BrowserNodeKind {
    /// A non-interactive text fact.
    Text,
    /// A status fact whose status field is meaningful to the host.
    Status,
    /// A tabular row or table summary.
    Table,
    /// A hierarchical inspector node.
    Tree,
    /// A chart summary or chart datum.
    Chart,
    /// An explicitly granted action affordance.
    Action,
}

impl BrowserNodeKind {
    /// Stable view spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Status => "status",
            Self::Table => "table",
            Self::Tree => "tree",
            Self::Chart => "chart",
            Self::Action => "action",
        }
    }
}

/// Deterministic place in a browser panel layout.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum BrowserDock {
    /// Left rail in the workbench, or the default dock for an in-app panel.
    #[default]
    Left,
    /// Right inspector rail.
    Right,
    /// Bottom timeline rail.
    Bottom,
    /// Floating lens/drawer.
    Floating,
}

impl BrowserDock {
    /// Stable view spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Bottom => "bottom",
            Self::Floating => "floating",
        }
    }
}

/// The six browser-visible session states from the devtools UX.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum BrowserSessionState {
    /// No build/runtime state has been published yet.
    #[default]
    Idle,
    /// A source rebuild is in progress.
    Building,
    /// The last build failed, so the last-good page remains visible.
    BuildFailed,
    /// The running program reported a failure.
    RuntimeFailure,
    /// A compatible hot reload is in progress or has just completed.
    HotReload,
    /// The test panel reports a failing test run.
    TestsFailing,
}

impl BrowserSessionState {
    /// Stable view spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Building => "building",
            Self::BuildFailed => "build-failed",
            Self::RuntimeFailure => "runtime-failure",
            Self::HotReload => "hot-reload",
            Self::TestsFailing => "tests-failing",
        }
    }
}

/// A source identity that a host may open after checking a grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserSourceLocation {
    /// Project-relative Jet source identity.
    pub source_id: String,
    /// One-based source line.
    pub line: u32,
    /// Zero-based source column.
    pub column: u32,
    /// Optional function identity at the source anchor.
    pub function: Option<String>,
}

impl BrowserSourceLocation {
    /// Construct a source location.  [`BrowserSourceLocation::validate`] is
    /// run when the location is used by a host action.
    pub fn new(
        source_id: impl Into<String>,
        line: u32,
        column: u32,
        function: Option<impl Into<String>>,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            line,
            column,
            function: function.map(Into::into),
        }
    }

    /// Validate that the location is safe to hand to a source navigator.
    pub fn validate(&self) -> Result<(), BrowserHostError> {
        validate_source_id(&self.source_id)?;
        if self.line == 0 {
            return Err(BrowserHostError::InvalidSource(
                "source line must be one-based".to_string(),
            ));
        }
        if let Some(function) = &self.function {
            validate_text(function, "source function", MAX_BROWSER_TEXT)?;
        }
        Ok(())
    }
}

/// A source-navigation capability minted by the canonical session projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserSourceGrant {
    /// Session that minted this grant.
    pub session_id: String,
    /// Source revision for which the grant is valid.
    pub revision: u64,
    /// Panel owning the source anchor.
    pub panel_id: String,
    /// Node owning the source anchor.
    pub node_id: String,
    /// Safe source target.
    pub source: BrowserSourceLocation,
}

impl BrowserSourceGrant {
    /// Construct a grant for one source anchor.
    pub fn new(
        session_id: impl Into<String>,
        revision: u64,
        panel_id: impl Into<String>,
        node_id: impl Into<String>,
        source: BrowserSourceLocation,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            revision,
            panel_id: panel_id.into(),
            node_id: node_id.into(),
            source,
        }
    }
}

/// Explicit alias used by callers that spell out the navigation capability.
pub type SourceNavigationGrant = BrowserSourceGrant;
/// Short alias for source-navigation callers.
pub type BrowserNavigationGrant = BrowserSourceGrant;

/// An action capability minted by the canonical session projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserActionGrant {
    /// Session that minted this grant.
    pub session_id: String,
    /// Source revision for which the grant is valid.
    pub revision: u64,
    /// Panel owning the action.
    pub panel_id: String,
    /// Node carrying the action affordance.
    pub node_id: String,
    /// Canonical action identity.
    pub action_id: String,
}

impl BrowserActionGrant {
    /// Construct an action capability.
    pub fn new(
        session_id: impl Into<String>,
        revision: u64,
        panel_id: impl Into<String>,
        node_id: impl Into<String>,
        action_id: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            revision,
            panel_id: panel_id.into(),
            node_id: node_id.into(),
            action_id: action_id.into(),
        }
    }
}

/// Short alias for callers that use the generic grant name.
pub type ActionGrant = BrowserActionGrant;

/// One typed browser node.  It intentionally has no generic payload field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserNode {
    /// Stable node identity within its panel.
    pub id: String,
    /// Closed node kind understood by every browser host.
    pub kind: BrowserNodeKind,
    /// Human-readable label.
    pub label: String,
    /// Explicitly published display value, if any.
    pub value: Option<String>,
    /// Optional typed status text.
    pub status: Option<String>,
    /// Child facts for tree-shaped nodes.
    pub children: Vec<BrowserNode>,
}

impl BrowserNode {
    /// Construct a node with no optional value, status, or children.
    pub fn new(id: impl Into<String>, kind: BrowserNodeKind, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind,
            label: label.into(),
            value: None,
            status: None,
            children: Vec::new(),
        }
    }

    /// Add an explicitly published display value.
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Add typed status text.
    pub fn with_status(mut self, status: impl Into<String>) -> Self {
        self.status = Some(status.into());
        self
    }

    /// Add child facts.
    pub fn with_children(mut self, children: Vec<BrowserNode>) -> Self {
        self.children = children;
        self
    }
}

/// A typed panel snapshot supplied by canonical Session parsing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserPanelFact {
    /// Stable panel identity.
    pub panel_id: String,
    /// Human-readable panel title.
    pub title: String,
    /// Age of the panel fact at publication time.
    pub freshness_ms: u64,
    /// Typed status spelling for this panel.
    pub status: String,
    /// Canonical recording cursor for this panel, if the panel has time
    /// cursor semantics.  This is distinct from the enclosing reconnect
    /// cursor held by `BrowserHost`.
    pub cursor: Option<u64>,
    /// Typed node facts shown by the panel.
    pub nodes: Vec<BrowserNode>,
}

impl BrowserPanelFact {
    /// Construct a panel fact.
    pub fn new(
        panel_id: impl Into<String>,
        title: impl Into<String>,
        freshness_ms: u64,
        status: impl Into<String>,
        nodes: Vec<BrowserNode>,
    ) -> Self {
        Self {
            panel_id: panel_id.into(),
            title: title.into(),
            freshness_ms,
            status: status.into(),
            cursor: None,
            nodes,
        }
    }

    /// Attach the canonical recording cursor for this panel.
    pub fn with_cursor(mut self, cursor: Option<u64>) -> Self {
        self.cursor = cursor;
        self
    }

    /// Identity spelling used by catalog consumers.
    pub fn identity(&self) -> &str {
        &self.panel_id
    }
}

/// One canonical, already-parsed devtools event for browser projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserHostEvent {
    /// Protocol identifier.  Must equal [`BROWSER_DEVTOOLS_PROTOCOL`].
    pub protocol: String,
    /// Resident session identity.
    pub session_id: String,
    /// Monotonic source revision.
    pub revision: u64,
    /// Monotonic event sequence within the session.
    pub sequence: u64,
    /// Canonical publication timestamp.
    pub observed_at_ms: u64,
    /// Typed panel facts; no generic JSON payload is accepted here.
    pub panels: Vec<BrowserPanelFact>,
}

impl BrowserHostEvent {
    /// Construct an event with the canonical protocol identifier.
    pub fn new(
        session_id: impl Into<String>,
        revision: u64,
        sequence: u64,
        observed_at_ms: u64,
        panels: Vec<BrowserPanelFact>,
    ) -> Self {
        Self {
            protocol: BROWSER_DEVTOOLS_PROTOCOL.to_string(),
            session_id: session_id.into(),
            revision,
            sequence,
            observed_at_ms,
            panels,
        }
    }

    /// Validate this event without mutating a host.
    pub fn validate(&self) -> Result<(), BrowserHostError> {
        if self.protocol != BROWSER_DEVTOOLS_PROTOCOL {
            return Err(BrowserHostError::InvalidEvent(
                "browser event protocol does not match jet.devtools.v1".to_string(),
            ));
        }
        validate_text(&self.session_id, "event session", MAX_BROWSER_TEXT)?;
        if self.panels.len() > MAX_BROWSER_PANELS {
            return Err(BrowserHostError::InvalidEvent(
                "browser event exceeds the panel bound".to_string(),
            ));
        }
        let mut panel_ids = BTreeSet::new();
        let mut node_count = 0usize;
        for panel in &self.panels {
            validate_panel(panel, &mut node_count)?;
            if !panel_ids.insert(panel.panel_id.as_str()) {
                return Err(BrowserHostError::InvalidEvent(format!(
                    "browser event repeats panel `{}`",
                    panel.panel_id
                )));
            }
        }
        Ok(())
    }
}

/// A reconnect cursor measured in canonical event sequence numbers.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct BrowserReconnectCursor {
    /// Last sequence acknowledged by the browser.
    pub sequence: u64,
}

impl BrowserReconnectCursor {
    /// Construct a cursor.
    pub const fn new(sequence: u64) -> Self {
        Self { sequence }
    }
}

impl From<u64> for BrowserReconnectCursor {
    fn from(sequence: u64) -> Self {
        Self::new(sequence)
    }
}

/// Bounded event response for a reconnecting browser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserReconnect {
    /// Session to which this response belongs.
    pub session_id: String,
    /// Cursor supplied by the caller, if any.
    pub from: Option<BrowserReconnectCursor>,
    /// Highest sequence represented by this host.
    pub cursor: Option<BrowserReconnectCursor>,
    /// Retained events after `from`, in sequence order.
    pub events: Vec<BrowserHostEvent>,
    /// True when no retained events were evicted between `from` and now.
    pub complete: bool,
}

/// Shared selection reflected by both browser placements.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserSelection {
    /// Panel identity.
    pub panel_id: String,
    /// Node identity, if a node is selected.
    pub node_id: Option<String>,
}

/// Actions accepted by the browser host.  Source navigation and invocation
/// carry a required, session/revision-bound grant; panel layout remains local
/// host state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrowserHostAction {
    /// Open a known panel in one placement.
    OpenPanel {
        /// Target placement.
        placement: BrowserPlacement,
        /// Panel identity.
        panel_id: String,
    },
    /// Close a known panel in one placement.
    ClosePanel {
        /// Target placement.
        placement: BrowserPlacement,
        /// Panel identity.
        panel_id: String,
    },
    /// Dock a known panel in one placement.
    DockPanel {
        /// Target placement.
        placement: BrowserPlacement,
        /// Panel identity.
        panel_id: String,
        /// New dock position.
        dock: BrowserDock,
    },
    /// Navigate to a source anchor with a required grant.
    NavigateSource {
        /// Placement issuing the navigation.
        placement: BrowserPlacement,
        /// Panel identity.
        panel_id: String,
        /// Node identity.
        node_id: String,
        /// Session/revision-bound source grant.
        grant: BrowserSourceGrant,
    },
    /// Invoke an explicitly granted Action node.
    InvokeAction {
        /// Placement issuing the invocation.
        placement: BrowserPlacement,
        /// Panel identity.
        panel_id: String,
        /// Action node identity.
        node_id: String,
        /// Canonical action identity.
        action_id: String,
        /// Session/revision-bound action grant.
        grant: BrowserActionGrant,
    },
}

impl BrowserHostAction {
    /// Construct an open action.
    pub fn open(placement: BrowserPlacement, panel_id: impl Into<String>) -> Self {
        Self::OpenPanel {
            placement,
            panel_id: panel_id.into(),
        }
    }

    /// Construct a close action.
    pub fn close(placement: BrowserPlacement, panel_id: impl Into<String>) -> Self {
        Self::ClosePanel {
            placement,
            panel_id: panel_id.into(),
        }
    }

    /// Construct a dock action.
    pub fn dock(
        placement: BrowserPlacement,
        panel_id: impl Into<String>,
        dock: BrowserDock,
    ) -> Self {
        Self::DockPanel {
            placement,
            panel_id: panel_id.into(),
            dock,
        }
    }

    /// Construct a source-navigation action.
    pub fn navigate_source(
        placement: BrowserPlacement,
        panel_id: impl Into<String>,
        node_id: impl Into<String>,
        grant: BrowserSourceGrant,
    ) -> Self {
        Self::NavigateSource {
            placement,
            panel_id: panel_id.into(),
            node_id: node_id.into(),
            grant,
        }
    }

    /// Construct an invocation action.
    pub fn invoke_action(
        placement: BrowserPlacement,
        panel_id: impl Into<String>,
        node_id: impl Into<String>,
        action_id: impl Into<String>,
        grant: BrowserActionGrant,
    ) -> Self {
        Self::InvokeAction {
            placement,
            panel_id: panel_id.into(),
            node_id: node_id.into(),
            action_id: action_id.into(),
            grant,
        }
    }
}

/// Result of an accepted host action.  Results carry identities only and never
/// leak an unrequested runtime payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrowserHostEffect {
    /// A panel became visible.
    PanelOpened {
        /// Placement changed.
        placement: BrowserPlacement,
        /// Panel identity.
        panel_id: String,
    },
    /// A panel became hidden.
    PanelClosed {
        /// Placement changed.
        placement: BrowserPlacement,
        /// Panel identity.
        panel_id: String,
    },
    /// A panel moved to another dock.
    PanelDocked {
        /// Placement changed.
        placement: BrowserPlacement,
        /// Panel identity.
        panel_id: String,
        /// New dock position.
        dock: BrowserDock,
    },
    /// A source target was approved.
    SourceNavigated {
        /// Placement issuing the navigation.
        placement: BrowserPlacement,
        /// Panel identity.
        panel_id: String,
        /// Node identity.
        node_id: String,
        /// Approved source target.
        source: BrowserSourceLocation,
    },
    /// An action was approved.
    ActionInvoked {
        /// Placement issuing the invocation.
        placement: BrowserPlacement,
        /// Panel identity.
        panel_id: String,
        /// Action node identity.
        node_id: String,
        /// Canonical action identity.
        action_id: String,
    },
}

/// Stable element facts consumed by a renderer.  This is a view model, not
/// HTML and not a second protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserHtmlElement {
    /// Deterministic element identity.
    pub id: String,
    /// Element role/fact kind.
    pub kind: String,
    /// Owning panel, if any.
    pub panel_id: Option<String>,
    /// Owning node, if any.
    pub node_id: Option<String>,
    /// Visible label.
    pub label: String,
    /// Explicitly published visible value, if any.
    pub value: Option<String>,
    /// Typed status, if any.
    pub status: Option<String>,
}

/// Deterministic browser view model for either placement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserHtmlViewModel {
    /// Placement represented by this model.
    pub placement: BrowserPlacement,
    /// Stable route expected by the eventual WebHost wiring.
    pub route: String,
    /// Canonical protocol identifier.
    pub protocol: String,
    /// Resident session identity.
    pub session_id: String,
    /// Current source revision, if an event has arrived.
    pub revision: Option<u64>,
    /// Current event sequence, if an event has arrived.
    pub sequence: Option<u64>,
    /// Latest event sequence used as the reconnect/bookmark cursor.  A
    /// recording cursor is carried independently by each panel view.
    pub cursor: Option<BrowserReconnectCursor>,
    /// Derived six-state session label.
    pub state: BrowserSessionState,
    /// Shared selected panel/node.
    pub selection: Option<BrowserSelection>,
    /// Panel views in deterministic panel-id order.
    pub panels: Vec<BrowserPanelView>,
    /// Flattened deterministic element facts for a renderer.
    pub elements: Vec<BrowserHtmlElement>,
}

/// Panel layout plus typed facts for a selected placement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserPanelView {
    /// Stable panel identity.
    pub panel_id: String,
    /// Human-readable panel title.
    pub title: String,
    /// Fact freshness age.
    pub freshness_ms: u64,
    /// Typed status text.
    pub status: String,
    /// Canonical recording cursor for this panel, if any.
    pub cursor: Option<u64>,
    /// Whether this placement currently displays the panel.
    pub open: bool,
    /// Current dock position.
    pub dock: BrowserDock,
    /// Typed node facts.
    pub nodes: Vec<BrowserNode>,
}

/// Errors raised before a browser action or event can change host state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrowserHostError {
    /// Event failed shape/protocol validation.
    InvalidEvent(String),
    /// An event belongs to another resident session.
    WrongSession { expected: String, received: String },
    /// An event moved backwards in source revision.
    StaleRevision { current: u64, received: u64 },
    /// An event reused or moved backwards in sequence.
    StaleSequence { current: u64, received: u64 },
    /// A reconnect cursor points before retained history.
    CursorExpired { cursor: u64, earliest: u64 },
    /// A reconnect cursor points beyond the current event.
    CursorAhead { cursor: u64, current: u64 },
    /// The requested panel is not in the canonical catalog.
    UnknownPanel(String),
    /// The requested node is not in the canonical panel facts.
    UnknownNode { panel_id: String, node_id: String },
    /// A source/action request carries no matching capability.
    ActionNotGranted(String),
    /// A source target is not safe to open.
    InvalidSource(String),
    /// A generic local action request is malformed.
    InvalidAction(String),
}

impl fmt::Display for BrowserHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEvent(message) => write!(formatter, "invalid browser event: {message}"),
            Self::WrongSession { expected, received } => write!(
                formatter,
                "browser event belongs to session `{received}`, expected `{expected}`"
            ),
            Self::StaleRevision { current, received } => write!(
                formatter,
                "stale browser revision {received}; current revision is {current}"
            ),
            Self::StaleSequence { current, received } => write!(
                formatter,
                "stale browser sequence {received}; current sequence is {current}"
            ),
            Self::CursorExpired { cursor, earliest } => write!(
                formatter,
                "browser reconnect cursor {cursor} predates retained sequence {earliest}"
            ),
            Self::CursorAhead { cursor, current } => write!(
                formatter,
                "browser reconnect cursor {cursor} is ahead of current sequence {current}"
            ),
            Self::UnknownPanel(panel_id) => write!(formatter, "unknown browser panel `{panel_id}`"),
            Self::UnknownNode { panel_id, node_id } => write!(
                formatter,
                "unknown browser node `{node_id}` in panel `{panel_id}`"
            ),
            Self::ActionNotGranted(message) => write!(formatter, "browser action not granted: {message}"),
            Self::InvalidSource(message) => write!(formatter, "invalid browser source: {message}"),
            Self::InvalidAction(message) => write!(formatter, "invalid browser action: {message}"),
        }
    }
}

impl std::error::Error for BrowserHostError {}

#[derive(Clone, Debug)]
struct PanelState {
    fact: BrowserPanelFact,
    layout: [PanelLayout; 2],
}

#[derive(Clone, Copy, Debug, Default)]
struct PanelLayout {
    open: bool,
    dock: BrowserDock,
}

/// One state machine shared by the in-app surface and workbench page.
#[derive(Clone, Debug)]
pub struct BrowserHost {
    session_id: String,
    revision: Option<u64>,
    sequence: Option<u64>,
    observed_at_ms: Option<u64>,
    state: BrowserSessionState,
    cursor: Option<BrowserReconnectCursor>,
    selection: Option<BrowserSelection>,
    panels: BTreeMap<String, PanelState>,
    history: VecDeque<BrowserHostEvent>,
}

impl BrowserHost {
    /// Bind a host to one resident session.  Events from every other session
    /// are rejected before they can affect placement or cursor state.
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            revision: None,
            sequence: None,
            observed_at_ms: None,
            state: BrowserSessionState::Idle,
            cursor: None,
            selection: None,
            panels: BTreeMap::new(),
            history: VecDeque::new(),
        }
    }

    /// Alias emphasizing the session binding.
    pub fn for_session(session_id: impl Into<String>) -> Self {
        Self::new(session_id)
    }

    /// Resident session identity.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Current source revision.
    pub fn revision(&self) -> Option<u64> {
        self.revision
    }

    /// Current accepted event sequence.
    pub fn sequence(&self) -> Option<u64> {
        self.sequence
    }

    /// Last canonical publication timestamp.
    pub fn observed_at_ms(&self) -> Option<u64> {
        self.observed_at_ms
    }

    /// Derived browser-visible state.
    pub fn state(&self) -> BrowserSessionState {
        self.state
    }

    /// Latest event sequence used as the reconnect/bookmark cursor.  Panel
    /// recording cursors are stored on their individual facts.
    pub fn cursor(&self) -> Option<BrowserReconnectCursor> {
        self.cursor
    }

    /// Shared selection.
    pub fn selection(&self) -> Option<&BrowserSelection> {
        self.selection.as_ref()
    }

    /// Apply one already-parsed canonical event.
    pub fn apply_event(&mut self, event: BrowserHostEvent) -> Result<(), BrowserHostError> {
        event.validate()?;
        if event.session_id != self.session_id {
            return Err(BrowserHostError::WrongSession {
                expected: self.session_id.clone(),
                received: event.session_id,
            });
        }
        if let Some(current) = self.revision {
            if event.revision < current {
                return Err(BrowserHostError::StaleRevision {
                    current,
                    received: event.revision,
                });
            }
        }
        if let Some(current) = self.sequence {
            if event.sequence <= current {
                return Err(BrowserHostError::StaleSequence {
                    current,
                    received: event.sequence,
                });
            }
        }

        let next_ids = event
            .panels
            .iter()
            .map(|panel| panel.panel_id.as_str())
            .collect::<BTreeSet<_>>();
        self.panels.retain(|panel_id, _| next_ids.contains(panel_id.as_str()));
        for fact in &event.panels {
            if let Some(panel) = self.panels.get_mut(&fact.panel_id) {
                panel.fact = fact.clone();
            } else {
                self.panels.insert(
                    fact.panel_id.clone(),
                    PanelState {
                        fact: fact.clone(),
                        layout: [PanelLayout::default(), PanelLayout::default()],
                    },
                );
            }
        }
        self.revision = Some(event.revision);
        self.sequence = Some(event.sequence);
        self.observed_at_ms = Some(event.observed_at_ms);
        self.state = derive_session_state(&event.panels);
        self.cursor = Some(BrowserReconnectCursor::new(event.sequence));
        if let Some(selection) = &self.selection {
            if !self.node_exists(&selection.panel_id, selection.node_id.as_deref()) {
                self.selection = None;
            }
        }
        if self.history.len() == MAX_RECONNECT_EVENTS {
            self.history.pop_front();
        }
        self.history.push_back(event);
        Ok(())
    }

    /// Short alias for event application.
    pub fn apply(&mut self, event: BrowserHostEvent) -> Result<(), BrowserHostError> {
        self.apply_event(event)
    }

    /// Return the retained event stream after `cursor`.
    pub fn resume_from(
        &self,
        cursor: Option<BrowserReconnectCursor>,
    ) -> Result<BrowserReconnect, BrowserHostError> {
        let current = self.sequence;
        if let (Some(requested), Some(current)) = (cursor, current) {
            if requested.sequence > current {
                return Err(BrowserHostError::CursorAhead {
                    cursor: requested.sequence,
                    current,
                });
            }
        }
        if let Some(requested) = cursor {
            if let Some(earliest) = self.history.front().map(|event| event.sequence) {
                if requested.sequence.saturating_add(1) < earliest {
                    return Err(BrowserHostError::CursorExpired {
                        cursor: requested.sequence,
                        earliest,
                    });
                }
            } else if requested.sequence > 0 {
                return Err(BrowserHostError::CursorExpired {
                    cursor: requested.sequence,
                    earliest: 0,
                });
            }
        }
        let events = self
            .history
            .iter()
            .filter(|event| cursor.is_none_or(|from| event.sequence > from.sequence))
            .cloned()
            .collect::<Vec<_>>();
        Ok(BrowserReconnect {
            session_id: self.session_id.clone(),
            from: cursor,
            cursor: self.sequence.map(BrowserReconnectCursor::new),
            events,
            complete: true,
        })
    }

    /// Resume from a plain sequence number.
    pub fn events_since(&self, sequence: Option<u64>) -> Result<Vec<BrowserHostEvent>, BrowserHostError> {
        Ok(self
            .resume_from(sequence.map(BrowserReconnectCursor::new))?
            .events)
    }

    /// Set the shared time cursor.  A cursor must refer to retained history.
    pub fn set_cursor(&mut self, cursor: Option<BrowserReconnectCursor>) -> Result<(), BrowserHostError> {
        if let Some(cursor) = cursor {
            self.resume_from(Some(cursor))?;
        }
        self.cursor = cursor;
        Ok(())
    }

    /// Scrub the shared cursor from either placement.
    pub fn scrub(&mut self, sequence: Option<u64>) -> Result<(), BrowserHostError> {
        self.set_cursor(sequence.map(BrowserReconnectCursor::new))
    }

    /// Return the immutable panel catalog in deterministic identity order.
    pub fn panel_catalog(&self) -> Vec<BrowserPanelFact> {
        let mut panels = self
            .panels
            .values()
            .map(|panel| panel.fact.clone())
            .collect::<Vec<_>>();
        let catalog = crate::Devtools::catalog::descriptors();
        panels.sort_by_key(|panel| {
            catalog
                .iter()
                .position(|descriptor| descriptor.id.as_str() == panel.panel_id)
                .unwrap_or(usize::MAX)
        });
        panels
    }

    /// Return one panel fact from the canonical catalog.
    pub fn panel(&self, panel_id: &str) -> Option<&BrowserPanelFact> {
        self.panels.get(panel_id).map(|panel| &panel.fact)
    }

    /// Is a panel open in the selected placement?
    pub fn is_open(&self, placement: BrowserPlacement, panel_id: &str) -> bool {
        self.panels
            .get(panel_id)
            .is_some_and(|panel| panel.layout[placement.index()].open)
    }

    /// Current dock for a panel, if it is known.
    pub fn dock(&self, placement: BrowserPlacement, panel_id: &str) -> Option<BrowserDock> {
        self.panels
            .get(panel_id)
            .map(|panel| panel.layout[placement.index()].dock)
    }

    /// Open a known panel in one placement.
    pub fn open_panel(
        &mut self,
        placement: BrowserPlacement,
        panel_id: impl Into<String>,
    ) -> Result<BrowserHostEffect, BrowserHostError> {
        let panel_id = panel_id.into();
        let panel = self.require_panel(&panel_id)?;
        panel.layout[placement.index()].open = true;
        Ok(BrowserHostEffect::PanelOpened {
            placement,
            panel_id,
        })
    }

    /// Close a known panel in one placement.
    pub fn close_panel(
        &mut self,
        placement: BrowserPlacement,
        panel_id: impl Into<String>,
    ) -> Result<BrowserHostEffect, BrowserHostError> {
        let panel_id = panel_id.into();
        let panel = self.require_panel(&panel_id)?;
        panel.layout[placement.index()].open = false;
        if self
            .selection
            .as_ref()
            .is_some_and(|selection| selection.panel_id == panel_id)
        {
            self.selection = None;
        }
        Ok(BrowserHostEffect::PanelClosed {
            placement,
            panel_id,
        })
    }

    /// Dock a known panel in one placement.
    pub fn dock_panel(
        &mut self,
        placement: BrowserPlacement,
        panel_id: impl Into<String>,
        dock: BrowserDock,
    ) -> Result<BrowserHostEffect, BrowserHostError> {
        let panel_id = panel_id.into();
        let panel = self.require_panel(&panel_id)?;
        panel.layout[placement.index()].dock = dock;
        Ok(BrowserHostEffect::PanelDocked {
            placement,
            panel_id,
            dock,
        })
    }

    /// Select a known panel/node.  Selection is intentionally shared across
    /// placements, so a workbench and in-app model cannot drift.
    pub fn select(
        &mut self,
        panel_id: impl Into<String>,
        node_id: Option<impl Into<String>>,
    ) -> Result<(), BrowserHostError> {
        let panel_id = panel_id.into();
        let node_id = node_id.map(Into::into);
        self.require_panel(&panel_id)?;
        if let Some(node_id) = node_id.as_deref() {
            self.require_node(&panel_id, node_id)?;
        }
        self.selection = Some(BrowserSelection { panel_id, node_id });
        Ok(())
    }

    /// Clear shared selection.
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Apply a typed host action.
    pub fn dispatch(&mut self, action: BrowserHostAction) -> Result<BrowserHostEffect, BrowserHostError> {
        match action {
            BrowserHostAction::OpenPanel {
                placement,
                panel_id,
            } => self.open_panel(placement, panel_id),
            BrowserHostAction::ClosePanel {
                placement,
                panel_id,
            } => self.close_panel(placement, panel_id),
            BrowserHostAction::DockPanel {
                placement,
                panel_id,
                dock,
            } => self.dock_panel(placement, panel_id, dock),
            BrowserHostAction::NavigateSource {
                placement,
                panel_id,
                node_id,
                grant,
            } => self.navigate_source(placement, &panel_id, &node_id, grant),
            BrowserHostAction::InvokeAction {
                placement,
                panel_id,
                node_id,
                action_id,
                grant,
            } => self.invoke_action(placement, &panel_id, &node_id, &action_id, grant),
        }
    }

    /// Alias for callers that name action handling explicitly.
    pub fn handle_action(
        &mut self,
        action: BrowserHostAction,
    ) -> Result<BrowserHostEffect, BrowserHostError> {
        self.dispatch(action)
    }

    /// Approve a source navigation only when the grant is current and the
    /// node still exists in the current canonical event.
    pub fn navigate_source(
        &self,
        placement: BrowserPlacement,
        panel_id: &str,
        node_id: &str,
        grant: BrowserSourceGrant,
    ) -> Result<BrowserHostEffect, BrowserHostError> {
        self.validate_grant_context(panel_id, node_id, &grant.session_id, grant.revision)?;
        if grant.panel_id != panel_id || grant.node_id != node_id {
            return Err(BrowserHostError::ActionNotGranted(
                "source grant does not name the requested panel/node".to_string(),
            ));
        }
        grant.source.validate()?;
        Ok(BrowserHostEffect::SourceNavigated {
            placement,
            panel_id: panel_id.to_string(),
            node_id: node_id.to_string(),
            source: grant.source,
        })
    }

    /// Approve an action only when the grant is current and the target node is
    /// an explicit `Action` node with the same canonical action identity.
    pub fn invoke_action(
        &self,
        placement: BrowserPlacement,
        panel_id: &str,
        node_id: &str,
        action_id: &str,
        grant: BrowserActionGrant,
    ) -> Result<BrowserHostEffect, BrowserHostError> {
        self.validate_grant_context(panel_id, node_id, &grant.session_id, grant.revision)?;
        if grant.panel_id != panel_id || grant.node_id != node_id || grant.action_id != action_id {
            return Err(BrowserHostError::ActionNotGranted(
                "action grant does not name the requested panel/node/action".to_string(),
            ));
        }
        validate_text(action_id, "action identity", MAX_BROWSER_TEXT)
            .map_err(|error| BrowserHostError::ActionNotGranted(error.to_string()))?;
        let node = self.require_node(panel_id, node_id)?;
        if node.kind != BrowserNodeKind::Action || node.id != action_id {
            return Err(BrowserHostError::ActionNotGranted(
                "the requested node is not an explicitly granted Action node".to_string(),
            ));
        }
        Ok(BrowserHostEffect::ActionInvoked {
            placement,
            panel_id: panel_id.to_string(),
            node_id: node_id.to_string(),
            action_id: action_id.to_string(),
        })
    }

    /// Build deterministic view facts for one browser placement.
    pub fn view_model(&self, placement: BrowserPlacement) -> BrowserHtmlViewModel {
        let mut panels = self
            .panels
            .values()
            .map(|panel| BrowserPanelView {
                panel_id: panel.fact.panel_id.clone(),
                title: panel.fact.title.clone(),
                freshness_ms: panel.fact.freshness_ms,
                status: panel.fact.status.clone(),
                cursor: panel.fact.cursor,
                open: panel.layout[placement.index()].open,
                dock: panel.layout[placement.index()].dock,
                nodes: panel.fact.nodes.clone(),
            })
            .collect::<Vec<_>>();
        let catalog = crate::Devtools::catalog::descriptors();
        panels.sort_by_key(|panel| {
            catalog
                .iter()
                .position(|descriptor| descriptor.id.as_str() == panel.panel_id)
                .unwrap_or(usize::MAX)
        });
        let mut elements = Vec::new();
        elements.push(BrowserHtmlElement {
            id: format!("jet-devtools-{}", placement.as_str()),
            kind: "host".to_string(),
            panel_id: None,
            node_id: None,
            label: self.state.as_str().to_string(),
            value: self.sequence.map(|sequence| sequence.to_string()),
            status: Some(self.state.as_str().to_string()),
        });
        for panel in &panels {
            elements.push(BrowserHtmlElement {
                id: element_id(placement, &panel.panel_id, None),
                kind: "panel".to_string(),
                panel_id: Some(panel.panel_id.clone()),
                node_id: None,
                label: panel.title.clone(),
                value: None,
                status: Some(panel.status.clone()),
            });
            flatten_nodes(
                placement,
                &panel.panel_id,
                &panel.nodes,
                &mut elements,
            );
        }
        BrowserHtmlViewModel {
            placement,
            route: match placement {
                BrowserPlacement::InApp => "/__jet_devtools".to_string(),
                BrowserPlacement::Workbench => "/__jet_devtools/workbench".to_string(),
            },
            protocol: BROWSER_DEVTOOLS_PROTOCOL.to_string(),
            session_id: self.session_id.clone(),
            revision: self.revision,
            sequence: self.sequence,
            cursor: self.cursor,
            state: self.state,
            selection: self.selection.clone(),
            panels,
            elements,
        }
    }

    /// Explicitly named alias for renderers that consume an HTML view model.
    pub fn html_view_model(&self, placement: BrowserPlacement) -> BrowserHtmlViewModel {
        self.view_model(placement)
    }

    /// Alias matching host callers that call the model a view.
    pub fn html_view(&self, placement: BrowserPlacement) -> BrowserHtmlViewModel {
        self.view_model(placement)
    }

    fn require_panel(&mut self, panel_id: &str) -> Result<&mut PanelState, BrowserHostError> {
        self.panels
            .get_mut(panel_id)
            .ok_or_else(|| BrowserHostError::UnknownPanel(panel_id.to_string()))
    }

    fn require_node(&self, panel_id: &str, node_id: &str) -> Result<&BrowserNode, BrowserHostError> {
        let panel = self
            .panels
            .get(panel_id)
            .ok_or_else(|| BrowserHostError::UnknownPanel(panel_id.to_string()))?;
        find_node(&panel.fact.nodes, node_id).ok_or_else(|| BrowserHostError::UnknownNode {
            panel_id: panel_id.to_string(),
            node_id: node_id.to_string(),
        })
    }

    fn node_exists(&self, panel_id: &str, node_id: Option<&str>) -> bool {
        match node_id {
            Some(node_id) => self.require_node(panel_id, node_id).is_ok(),
            None => self.panels.contains_key(panel_id),
        }
    }

    fn validate_grant_context(
        &self,
        panel_id: &str,
        node_id: &str,
        grant_session: &str,
        grant_revision: u64,
    ) -> Result<(), BrowserHostError> {
        let Some(current_revision) = self.revision else {
            return Err(BrowserHostError::ActionNotGranted(
                "no canonical event has established a current revision".to_string(),
            ));
        };
        if grant_session != self.session_id {
            return Err(BrowserHostError::WrongSession {
                expected: self.session_id.clone(),
                received: grant_session.to_string(),
            });
        }
        if grant_revision != current_revision {
            return Err(BrowserHostError::StaleRevision {
                current: current_revision,
                received: grant_revision,
            });
        }
        self.require_node(panel_id, node_id)?;
        Ok(())
    }
}

impl Default for BrowserHost {
    fn default() -> Self {
        Self::new("")
    }
}

fn validate_panel(panel: &BrowserPanelFact, node_count: &mut usize) -> Result<(), BrowserHostError> {
    validate_text(&panel.panel_id, "panel identity", MAX_BROWSER_TEXT)?;
    validate_text(&panel.title, "panel title", MAX_BROWSER_TEXT)?;
    validate_text(&panel.status, "panel status", MAX_BROWSER_TEXT)?;
    let mut node_ids = BTreeSet::new();
    for node in &panel.nodes {
        validate_node(node, 0, node_count, &mut node_ids)?;
    }
    Ok(())
}

fn validate_node(
    node: &BrowserNode,
    depth: usize,
    node_count: &mut usize,
    node_ids: &mut BTreeSet<String>,
) -> Result<(), BrowserHostError> {
    if depth > MAX_BROWSER_NODE_DEPTH {
        return Err(BrowserHostError::InvalidEvent(
            "browser node tree exceeds the depth bound".to_string(),
        ));
    }
    *node_count = node_count.saturating_add(1);
    if *node_count > MAX_BROWSER_NODES {
        return Err(BrowserHostError::InvalidEvent(
            "browser event exceeds the node bound".to_string(),
        ));
    }
    validate_text(&node.id, "node identity", MAX_BROWSER_TEXT)?;
    validate_text(&node.label, "node label", MAX_BROWSER_TEXT)?;
    if let Some(value) = &node.value {
        validate_text(value, "node value", MAX_BROWSER_VALUE)?;
    }
    if let Some(status) = &node.status {
        validate_text(status, "node status", MAX_BROWSER_TEXT)?;
    }
    if !node_ids.insert(node.id.clone()) {
        return Err(BrowserHostError::InvalidEvent(format!(
            "browser panel repeats node `{}`",
            node.id
        )));
    }
    for child in &node.children {
        validate_node(child, depth + 1, node_count, node_ids)?;
    }
    Ok(())
}

fn validate_text(value: &str, label: &str, limit: usize) -> Result<(), BrowserHostError> {
    if value.is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return Err(BrowserHostError::InvalidEvent(format!(
            "{label} is empty, too long, or contains control text"
        )));
    }
    Ok(())
}

fn validate_source_id(source_id: &str) -> Result<(), BrowserHostError> {
    if source_id.is_empty()
        || source_id.len() > MAX_BROWSER_TEXT
        || source_id.chars().any(char::is_control)
        || source_id.starts_with('/')
        || source_id.starts_with('\\')
        || source_id.contains('\\')
        || source_id.split('/').any(|part| part == ".." || part.is_empty())
        || source_id.contains("://")
    {
        return Err(BrowserHostError::InvalidSource(
            "source identity must be a non-empty project-relative path".to_string(),
        ));
    }
    Ok(())
}

fn derive_session_state(panels: &[BrowserPanelFact]) -> BrowserSessionState {
    let mut state = BrowserSessionState::Idle;
    for panel in panels {
        let value = panel.status.trim().to_ascii_lowercase();
        let candidate = match value.as_str() {
            "building" => BrowserSessionState::Building,
            "build_failed" | "build-failed" | "build failed" => BrowserSessionState::BuildFailed,
            "runtime_failure" | "runtime-failure" | "runtime failure" => {
                BrowserSessionState::RuntimeFailure
            }
            "hot_reload" | "hot-reload" | "hot reload" => BrowserSessionState::HotReload,
            "tests_failing" | "tests-failing" | "tests failing" => {
                BrowserSessionState::TestsFailing
            }
            _ => continue,
        };
        state = prefer_state(state, candidate);
    }
    state
}

fn prefer_state(current: BrowserSessionState, candidate: BrowserSessionState) -> BrowserSessionState {
    fn rank(state: BrowserSessionState) -> u8 {
        match state {
            BrowserSessionState::Idle => 0,
            BrowserSessionState::HotReload => 1,
            BrowserSessionState::Building => 2,
            BrowserSessionState::TestsFailing => 3,
            BrowserSessionState::RuntimeFailure => 4,
            BrowserSessionState::BuildFailed => 5,
        }
    }
    if rank(candidate) >= rank(current) {
        candidate
    } else {
        current
    }
}

fn find_node<'a>(nodes: &'a [BrowserNode], node_id: &str) -> Option<&'a BrowserNode> {
    nodes.iter().find_map(|node| {
        if node.id == node_id {
            Some(node)
        } else {
            find_node(&node.children, node_id)
        }
    })
}

fn element_id(
    placement: BrowserPlacement,
    panel_id: &str,
    node_id: Option<&str>,
) -> String {
    let mut id = format!("jet-devtools-{}-panel-{}", placement.as_str(), stable_fragment(panel_id));
    if let Some(node_id) = node_id {
        id.push_str("-node-");
        id.push_str(&stable_fragment(node_id));
    }
    id
}

fn stable_fragment(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
            output.push(character);
        } else {
            output.push('-');
        }
    }
    if output.is_empty() {
        "item".to_string()
    } else {
        output
    }
}

fn flatten_nodes(
    placement: BrowserPlacement,
    panel_id: &str,
    nodes: &[BrowserNode],
    elements: &mut Vec<BrowserHtmlElement>,
) {
    for node in nodes {
        elements.push(BrowserHtmlElement {
            id: element_id(placement, panel_id, Some(&node.id)),
            kind: node.kind.as_str().to_string(),
            panel_id: Some(panel_id.to_string()),
            node_id: Some(node.id.clone()),
            label: node.label.clone(),
            value: node.value.clone(),
            status: node.status.clone(),
        });
        flatten_nodes(placement, panel_id, &node.children, elements);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(status: &str, nodes: Vec<BrowserNode>) -> BrowserPanelFact {
        BrowserPanelFact::new("build", "Build", 12, status, nodes)
    }

    fn event(session: &str, revision: u64, sequence: u64, status: &str) -> BrowserHostEvent {
        BrowserHostEvent::new(
            session,
            revision,
            sequence,
            100 + sequence,
            vec![panel(
                status,
                vec![
                    BrowserNode::new("go", BrowserNodeKind::Action, "Go"),
                    BrowserNode::new("source", BrowserNodeKind::Text, "Source"),
                ],
            )],
        )
    }

    #[test]
    fn event_validation_and_monotonic_projection() {
        let mut host = BrowserHost::new("s1");
        host.apply_event(event("s1", 1, 1, "ready")).unwrap();
        assert_eq!(host.revision(), Some(1));
        assert_eq!(host.sequence(), Some(1));
        assert_eq!(host.state(), BrowserSessionState::Idle);
        assert_eq!(host.panel_catalog().len(), 1);
        assert!(matches!(
            host.apply_event(event("s1", 0, 2, "ready")),
            Err(BrowserHostError::StaleRevision { .. })
        ));
        assert!(matches!(
            host.apply_event(event("s1", 1, 1, "ready")),
            Err(BrowserHostError::StaleSequence { .. })
        ));
        assert!(matches!(
            host.apply_event(event("s2", 2, 2, "ready")),
            Err(BrowserHostError::WrongSession { .. })
        ));
    }

    #[test]
    fn layout_and_selection_are_shared_but_open_state_is_placement_local() {
        let mut host = BrowserHost::new("s1");
        host.apply_event(event("s1", 1, 1, "ready")).unwrap();
        host.open_panel(BrowserPlacement::InApp, "build").unwrap();
        host.open_panel(BrowserPlacement::Workbench, "build").unwrap();
        host.select("build", Some("source")).unwrap();
        host.dock_panel(BrowserPlacement::Workbench, "build", BrowserDock::Right)
            .unwrap();
        let in_app = host.view_model(BrowserPlacement::InApp);
        let workbench = host.view_model(BrowserPlacement::Workbench);
        assert!(in_app.panels[0].open);
        assert!(workbench.panels[0].open);
        assert_eq!(in_app.panels[0].dock, BrowserDock::Left);
        assert_eq!(workbench.panels[0].dock, BrowserDock::Right);
        assert_eq!(in_app.selection, workbench.selection);
        assert_eq!(in_app.route, "/__jet_devtools");
        assert_eq!(workbench.route, "/__jet_devtools/workbench");
        assert_eq!(in_app.elements, workbench.elements.iter().map(|element| {
            BrowserHtmlElement {
                id: element.id.replace("workbench", "in-app"),
                kind: element.kind.clone(),
                panel_id: element.panel_id.clone(),
                node_id: element.node_id.clone(),
                label: element.label.clone(),
                value: element.value.clone(),
                status: element.status.clone(),
            }
        }).collect::<Vec<_>>());
    }

    #[test]
    fn reconnect_is_bounded_and_cursor_resumes() {
        let mut host = BrowserHost::new("s1");
        for sequence in 1..=(MAX_RECONNECT_EVENTS as u64 + 2) {
            host.apply_event(event("s1", sequence, sequence, "ready"))
                .unwrap();
        }
        let retained = host.resume_from(Some(254.into())).unwrap();
        assert_eq!(retained.events.len(), 4);
        assert_eq!(retained.events[0].sequence, 255);
        assert!(matches!(
            host.resume_from(Some(1.into())),
            Err(BrowserHostError::CursorExpired { .. })
        ));
        assert!(matches!(
            host.resume_from(Some((MAX_RECONNECT_EVENTS as u64 + 3).into())),
            Err(BrowserHostError::CursorAhead { .. })
        ));
    }

    #[test]
    fn stale_wrong_session_and_ungranted_actions_are_rejected() {
        let mut host = BrowserHost::new("s1");
        host.apply_event(event("s1", 7, 1, "ready")).unwrap();
        let source = BrowserSourceGrant::new(
            "s1",
            7,
            "build",
            "source",
            BrowserSourceLocation::new("src/main.jet", 4, 2, Some("main")),
        );
        let effect = host
            .dispatch(BrowserHostAction::navigate_source(
                BrowserPlacement::InApp,
                "build",
                "source",
                source.clone(),
            ))
            .unwrap();
        assert!(matches!(effect, BrowserHostEffect::SourceNavigated { .. }));
        let stale = BrowserSourceGrant::new(
            "s1",
            6,
            "build",
            "source",
            source.source.clone(),
        );
        assert!(matches!(
            host.dispatch(BrowserHostAction::navigate_source(
                BrowserPlacement::Workbench,
                "build",
                "source",
                stale,
            )),
            Err(BrowserHostError::StaleRevision { .. })
        ));
        let wrong = BrowserSourceGrant::new(
            "s2",
            7,
            "build",
            "source",
            source.source.clone(),
        );
        assert!(matches!(
            host.dispatch(BrowserHostAction::navigate_source(
                BrowserPlacement::InApp,
                "build",
                "source",
                wrong,
            )),
            Err(BrowserHostError::WrongSession { .. })
        ));
        let action = BrowserActionGrant::new("s1", 7, "build", "source", "source");
        assert!(matches!(
            host.dispatch(BrowserHostAction::invoke_action(
                BrowserPlacement::InApp,
                "build",
                "source",
                "source",
                action,
            )),
            Err(BrowserHostError::ActionNotGranted(_))
        ));
    }
}
