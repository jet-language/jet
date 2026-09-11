//! Deterministic terminal projection for the shared `jet.devtools.v1` stream.
//!
//! This module owns terminal facts only: bounded nodes, viewport/capability
//! state, keyboard routing, and frame diffs.  It deliberately does not know
//! what a Build, Query, Job, or any other domain panel means.  Session and
//! protocol adapters convert their canonical event into [`TerminalHostEvent`]
//! before calling [`TerminalHost::sync_projection`] or [`TerminalHost::sync_session`].

use std::collections::{BTreeMap, BTreeSet, VecDeque};
pub use crate::Devtools::{
    JetDevtoolsEvent, JetDevtoolsHostKind, JetDevtoolsPanelAvailability,
    JetDevtoolsPanelAvailabilityState, JetDevtoolsPanelCapability, JetDevtoolsPanelCatalog,
    JetDevtoolsPanelCatalogError, JetDevtoolsPanelDescriptor, JetDevtoolsPanelFreshnessPolicy,
    JetDevtoolsPanelId, JetDevtoolsPanelUnavailableReason,
};





fn first_panel_id() -> &'static str {
    crate::Devtools::catalog::descriptors()
        .first()
        .map(|descriptor| descriptor.id.as_str())
        .unwrap_or_default()
}

/// Project explicit availability rows instead of silently dropping panels.
pub fn panel_availability(
    host: JetDevtoolsHostKind,
    grants: &[JetDevtoolsPanelCapability],
) -> Vec<JetDevtoolsPanelAvailability> {
    JetDevtoolsPanelCatalog::canonical()
        .map(|catalog| catalog.available_for(host, grants))
        .unwrap_or_default()
}

/// Terminal release policy strips local detail capabilities while retaining
/// the complete catalog and its typed unavailable reasons.
pub fn terminal_panel_availability(
    access: TerminalHostAccess,
) -> Vec<JetDevtoolsPanelAvailability> {
    let grants: &[JetDevtoolsPanelCapability] = if access.local_details_enabled() {
        &JetDevtoolsPanelCapability::ALL
    } else {
        &[]
    };
    panel_availability(JetDevtoolsHostKind::Terminal, grants)
}


use crate::Session::{
    DevtoolsProjection, DevtoolsSessionState, DevtoolsStatusFact, ResidentDevSession,
};

/// Access facts are supplied by the owning command.  The adapter never
/// infers local/release/read-only policy from environment or process state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalHostAccess {
    pub local: bool,
    pub release: bool,
    pub read_only: bool,
}

impl TerminalHostAccess {
    pub const fn new(local: bool, release: bool, read_only: bool) -> Self {
        Self {
            local,
            release,
            read_only,
        }
    }

    pub const fn interactive() -> Self {
        Self::new(true, false, false)
    }

    pub const fn local_details_enabled(self) -> bool {
        self.local && !self.release
    }

    pub const fn actions_enabled(self) -> bool {
        !self.read_only
    }
}
impl Default for TerminalHostAccess {
    fn default() -> Self {
        Self::interactive()
    }
}

/// Explicit terminal facts supplied by the process boundary.
///
/// The host never consults process environment or terminal handles.  Callers
/// resolve TTY, ANSI, `NO_COLOR`, width, verbosity, and access once, then pass
/// those facts here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalHostCapabilities {
    pub tty: bool,
    pub ansi: bool,
    pub no_color: bool,
    pub width: usize,
    pub verbose: bool,
    pub quiet: bool,
    pub access: TerminalHostAccess,
}

impl TerminalHostCapabilities {
    pub const fn new(
        tty: bool,
        ansi: bool,
        no_color: bool,
        width: usize,
        verbose: bool,
        quiet: bool,
        access: TerminalHostAccess,
    ) -> Self {
        Self {
            tty,
            ansi,
            no_color,
            width: if width == 0 { 1 } else { width },
            verbose,
            quiet,
            access,
        }
    }

    pub const fn with_no_color(color_capable: bool, no_color: bool) -> Self {
        Self::new(
            color_capable,
            color_capable,
            no_color,
            DEFAULT_TERMINAL_COLUMNS,
            false,
            false,
            TerminalHostAccess::interactive(),
        )
    }

    pub const fn color() -> Self {
        Self::new(
            true,
            true,
            false,
            DEFAULT_TERMINAL_COLUMNS,
            false,
            false,
            TerminalHostAccess::interactive(),
        )
    }

    pub const fn plain() -> Self {
        Self::new(
            false,
            false,
            true,
            DEFAULT_TERMINAL_COLUMNS,
            false,
            false,
            TerminalHostAccess::interactive(),
        )
    }

    pub const fn with_width(mut self, width: usize) -> Self {
        self.width = if width == 0 { 1 } else { width };
        self
    }

    pub const fn color_enabled(self) -> bool {
        self.tty && self.ansi && !self.no_color
    }
}
impl Default for TerminalHostCapabilities {
    fn default() -> Self {
        Self::plain()
    }
}

/// The only protocol accepted by this host projection.
pub const JET_DEVTOOLS_PROTOCOL: &str = "jet.devtools.v1";

/// Shared Prelude bounds retained at the terminal boundary.
pub const MAX_TERMINAL_EVENTS: usize = 256;
pub const MAX_TERMINAL_FACTS: usize = 64;
pub const MAX_TERMINAL_TEXT_BYTES: usize = 16 * 1024;
pub const MAX_TERMINAL_NODES: usize = 2048;
pub const DEFAULT_TERMINAL_COLUMNS: usize = 80;
pub const DEFAULT_TERMINAL_ROWS: usize = 24;


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalBounds {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl TerminalBounds {
    pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn right(self) -> usize {
        self.x.saturating_add(self.width)
    }

    pub const fn bottom(self) -> usize {
        self.y.saturating_add(self.height)
    }

    pub const fn contains(self, x: usize, y: usize) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }
}

/// A normalized terminal viewport.  Zero-sized terminal reports still get a
/// one-cell logical viewport, matching the headless terminal contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalViewport {
    pub width: usize,
    pub height: usize,
    pub scroll: usize,
}

impl TerminalViewport {
    pub const fn new(width: usize, height: usize) -> Self {
        Self {
            width: if width == 0 { 1 } else { width },
            height: if height == 0 { 1 } else { height },
            scroll: 0,
        }
    }

    pub const fn with_scroll(width: usize, height: usize, scroll: usize) -> Self {
        Self {
            width: if width == 0 { 1 } else { width },
            height: if height == 0 { 1 } else { height },
            scroll,
        }
    }

    pub const fn columns(self) -> usize {
        self.width
    }

    pub const fn rows(self) -> usize {
        self.height
    }

    pub const fn bounds(self) -> TerminalBounds {
        TerminalBounds::new(0, 0, self.width, self.height)
    }

    pub const fn resize(self, width: usize, height: usize) -> Self {
        Self::with_scroll(width, height, self.scroll)
    }
}

impl Default for TerminalViewport {
    fn default() -> Self {
        Self::new(DEFAULT_TERMINAL_COLUMNS, DEFAULT_TERMINAL_ROWS)
    }
}


#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalHostConfig {
    pub viewport: TerminalViewport,
    pub capabilities: TerminalHostCapabilities,
    pub max_events: usize,
    pub max_nodes: usize,
}

impl TerminalHostConfig {
    pub fn new(viewport: TerminalViewport, capabilities: TerminalHostCapabilities) -> Self {
        Self {
            viewport,
            capabilities,
            max_events: MAX_TERMINAL_EVENTS,
            max_nodes: MAX_TERMINAL_NODES,
        }
    }

    pub fn with_limits(mut self, max_events: usize, max_nodes: usize) -> Self {
        self.max_events = max_events.clamp(1, MAX_TERMINAL_EVENTS);
        self.max_nodes = max_nodes.clamp(1, MAX_TERMINAL_NODES);
        self
    }

    pub fn with_viewport(mut self, viewport: TerminalViewport) -> Self {
        self.viewport = viewport;
        self
    }

    pub fn with_capabilities(mut self, capabilities: TerminalHostCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }
}

impl Default for TerminalHostConfig {
    fn default() -> Self {
        Self::new(TerminalViewport::default(), TerminalHostCapabilities::default())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalFactValue {
    Text(String),
    Integer(i64),
    Boolean(bool),
    DurationMs(u64),
}

impl TerminalFactValue {
    fn display(&self) -> String {
        match self {
            Self::Text(value) => value.clone(),
            Self::Integer(value) => value.to_string(),
            Self::Boolean(value) => value.to_string(),
            Self::DurationMs(value) => format!("{value}ms"),
        }
    }
}

/// One already-typed, payload-safe fact supplied by the canonical protocol
/// adapter.  A redacted fact never carries a value.  Published values are
/// opt-in and remain scalar; this host never accepts a generic JSON escape
/// hatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalHostFact {
    pub key: String,
    pub value: Option<TerminalFactValue>,
    pub published: bool,
}

impl TerminalHostFact {
    pub fn redacted(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: None,
            published: false,
        }
    }

    pub fn text(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: Some(TerminalFactValue::Text(value.into())),
            published: true,
        }
    }

    pub fn text_bounded(key: impl Into<String>, value: &str) -> Self {
        Self::text(key, bounded_text(value, MAX_TERMINAL_TEXT_BYTES))
    }

    pub const fn integer(key: String, value: i64) -> Self {
        Self {
            key,
            value: Some(TerminalFactValue::Integer(value)),
            published: true,
        }
    }

    pub const fn boolean(key: String, value: bool) -> Self {
        Self {
            key,
            value: Some(TerminalFactValue::Boolean(value)),
            published: true,
        }
    }

    pub const fn duration_ms(key: String, value: u64) -> Self {
        Self {
            key,
            value: Some(TerminalFactValue::DurationMs(value)),
            published: true,
        }
    }

    fn validate(&self) -> Result<(), String> {
        validate_text(&self.key, "fact key")?;
        if !self.published && self.value.is_some() {
            return Err(format!("terminal fact `{}` is redacted but carries a value", self.key));
        }
        if let Some(TerminalFactValue::Text(value)) = &self.value {
            validate_text(value, "fact value")?;
        }
        Ok(())
    }

    fn display(&self) -> String {
        match (&self.published, &self.value) {
            (true, Some(value)) => format!("{}={}", self.key, value.display()),
            _ => format!("{}=<redacted>", self.key),
        }
    }
}

/// Typed projection of one canonical `jet.devtools.v1` event.
///
/// `protocol`, `session_id`, and `revision` remain on this projection so the
/// host can reject a record from a different stream or source revision before
/// it enters the bounded ring.  `panel_id` is the canonical catalog identity,
/// and `observed_at` is the exact timestamp from the canonical event; no host
/// clock is consulted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalHostEvent {
    pub protocol: String,
    pub session_id: String,
    pub revision: String,
    pub sequence: u64,
    pub observed_at: u64,
    pub source: String,
    pub kind: String,
    pub entity: String,
    pub panel_id: JetDevtoolsPanelId,
    pub facts: Vec<TerminalHostFact>,
}

impl TerminalHostEvent {
    pub fn new(
        panel_id: JetDevtoolsPanelId,
        session_id: impl Into<String>,
        revision: impl Into<String>,
        sequence: u64,
        observed_at: u64,
        source: impl Into<String>,
        kind: impl Into<String>,
        entity: impl Into<String>,
        facts: Vec<TerminalHostFact>,
    ) -> Self {
        Self::from_projection(
            panel_id,
            JET_DEVTOOLS_PROTOCOL,
            session_id,
            revision,
            sequence,
            observed_at,
            source,
            kind,
            entity,
            facts,
        )
    }

    pub fn from_projection(
        panel_id: JetDevtoolsPanelId,
        protocol: impl Into<String>,
        session_id: impl Into<String>,
        revision: impl Into<String>,
        sequence: u64,
        observed_at: u64,
        source: impl Into<String>,
        kind: impl Into<String>,
        entity: impl Into<String>,
        facts: Vec<TerminalHostFact>,
    ) -> Self {
        Self {
            protocol: protocol.into(),
            session_id: session_id.into(),
            revision: revision.into(),
            sequence,
            observed_at,
            source: source.into(),
            kind: kind.into(),
            entity: entity.into(),
            panel_id,
            facts,
        }
    }

    pub fn from_devtools_event(
        event: &JetDevtoolsEvent,
        protocol: &str,
        session_id: &str,
        revision: &str,
        panel_id: JetDevtoolsPanelId,
    ) -> Self {
        let mut facts = vec![TerminalHostFact::text_bounded("fields", event.fields_json())];
        if let Some(payload) = event.payload_json() {
            facts.push(TerminalHostFact::text_bounded("payload", payload));
        }
        Self::from_projection(
            panel_id,
            protocol,
            session_id,
            revision,
            event.sequence(),
            event.timestamp_ms,
            event.source().to_string(),
            event.kind().to_string(),
            event.entity().to_string(),
            facts,
        )
    }

    pub fn published_fact_count(&self) -> usize {
        self.facts.iter().filter(|fact| fact.published).count()
    }

    fn validate(&self) -> Result<(), String> {
        if self.protocol != JET_DEVTOOLS_PROTOCOL {
            return Err(format!(
                "terminal event protocol must be {JET_DEVTOOLS_PROTOCOL}"
            ));
        }
        validate_text(&self.session_id, "session id")?;
        validate_optional_text(&self.revision, "revision", true)?;
        if self.sequence == 0 {
            return Err("terminal event sequence must be positive".to_string());
        }
        validate_text(&self.source, "event source")?;
        validate_text(&self.kind, "event kind")?;
        validate_text(&self.entity, "event entity")?;
        if self.facts.len() > MAX_TERMINAL_FACTS {
            return Err("terminal event exceeds its fact bound".to_string());
        }
        let mut keys = BTreeSet::new();
        for fact in &self.facts {
            fact.validate()?;
            if !keys.insert(fact.key.as_str()) {
                return Err(format!("terminal event repeats fact key `{}`", fact.key));
            }
        }
        Ok(())
    }
}


#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerminalFocus {
    pub panel_id: Option<String>,
    pub item_key: Option<String>,
}

impl TerminalFocus {
    pub fn panel(panel_id: impl Into<String>) -> Self {
        Self {
            panel_id: Some(panel_id.into()),
            item_key: None,
        }
    }

    pub fn item(panel_id: impl Into<String>, item_key: impl Into<String>) -> Self {
        Self {
            panel_id: Some(panel_id.into()),
            item_key: Some(item_key.into()),
        }
    }

    pub fn clear_item(&mut self) {
        self.item_key = None;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalKey {
    Tab,
    BackTab,
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    Character(char),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalInput {
    Key(TerminalKey),
    Resize(TerminalViewport),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalAction {
    FocusPanel { panel_id: String },
    Inspect { panel_id: String, item_key: String },
    ReturnToPanel,
    MoveCursor { cursor: u64 },
    Rerun,
    Restart,
    FocusJob,
    Quit,
    Ignored,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalKeyboardState {
    pub last_key: Option<TerminalKey>,
    pub last_action: Option<TerminalAction>,
}

impl Default for TerminalKeyboardState {
    fn default() -> Self {
        Self {
            last_key: None,
            last_action: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalFreshness {
    Empty,
    Fresh,
    Stale,
}

impl TerminalFreshness {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Fresh => "fresh",
            Self::Stale => "stale",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalNodeKind {
    Root,
    Panel,
    Table,
    Text,
    Status,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalStatus {
    pub label: String,
    pub detail: String,
    pub freshness: TerminalFreshness,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalTableRow {
    pub key: String,
    pub cells: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalTable {
    pub columns: Vec<String>,
    pub rows: Vec<TerminalTableRow>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalNode {
    pub id: String,
    pub kind: TerminalNodeKind,
    pub label: String,
    pub bounds: TerminalBounds,
    pub freshness: TerminalFreshness,
    pub focusable: bool,
    pub text: Option<String>,
    pub status: Option<TerminalStatus>,
    pub table: Option<TerminalTable>,
    pub children: Vec<TerminalNode>,
}

impl TerminalNode {
    fn container(
        id: impl Into<String>,
        kind: TerminalNodeKind,
        label: impl Into<String>,
        bounds: TerminalBounds,
        freshness: TerminalFreshness,
        children: Vec<TerminalNode>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            label: label.into(),
            bounds,
            freshness,
            focusable: false,
            text: None,
            status: None,
            table: None,
            children,
        }
    }

    fn text(
        id: impl Into<String>,
        label: impl Into<String>,
        text: impl Into<String>,
        bounds: TerminalBounds,
        freshness: TerminalFreshness,
    ) -> Self {
        Self {
            id: id.into(),
            kind: TerminalNodeKind::Text,
            label: label.into(),
            bounds,
            freshness,
            focusable: false,
            text: Some(text.into()),
            status: None,
            table: None,
            children: Vec::new(),
        }
    }

    fn status(
        id: impl Into<String>,
        label: impl Into<String>,
        status: TerminalStatus,
        bounds: TerminalBounds,
        focusable: bool,
    ) -> Self {
        Self {
            id: id.into(),
            kind: TerminalNodeKind::Status,
            label: label.into(),
            bounds,
            freshness: status.freshness,
            focusable,
            text: None,
            status: Some(status),
            table: None,
            children: Vec::new(),
        }
    }

    fn table(
        id: impl Into<String>,
        label: impl Into<String>,
        table: TerminalTable,
        bounds: TerminalBounds,
        freshness: TerminalFreshness,
    ) -> Self {
        Self {
            id: id.into(),
            kind: TerminalNodeKind::Table,
            label: label.into(),
            bounds,
            freshness,
            focusable: false,
            text: None,
            status: None,
            table: Some(table),
            children: Vec::new(),
        }
    }

    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    pub fn find(&self, id: &str) -> Option<&Self> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(id))
    }

    fn flatten<'a>(&'a self, out: &mut BTreeMap<String, &'a TerminalNode>) {
        out.insert(self.id.clone(), self);
        for child in &self.children {
            child.flatten(out);
        }
    }

    fn count(&self) -> usize {
        1 + self.children.iter().map(Self::count).sum::<usize>()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalPanelTree {
    pub root: TerminalNode,
    pub bounds: TerminalBounds,
    pub node_count: usize,
    pub node_limit: usize,
    pub panel_count: usize,
}

impl TerminalPanelTree {
    pub fn panel(&self, panel_id: &str) -> Option<&TerminalNode> {
        self.root
            .children
            .iter()
            .find(|node| node.id == format!("panel:{panel_id}"))
    }

    pub fn nodes(&self) -> BTreeMap<String, &TerminalNode> {
        let mut nodes = BTreeMap::new();
        self.root.flatten(&mut nodes);
        nodes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalFrame {
    pub protocol: String,
    pub session_id: String,
    pub source_id: Option<String>,
    pub build_id: Option<String>,
    pub world_id: Option<String>,
    pub observed_at: Option<u64>,
    pub status: Option<DevtoolsStatusFact>,
    pub revision: String,
    pub cursor: Option<u64>,
    pub viewport: TerminalViewport,
    pub capabilities: TerminalHostCapabilities,
    pub focus: TerminalFocus,
    pub keyboard: TerminalKeyboardState,
    pub freshness: TerminalFreshness,
    pub event_count: usize,
    pub event_limit: usize,
    pub tree: TerminalPanelTree,
    pub lines: Vec<String>,
}

impl TerminalFrame {
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn plain_text(&self) -> String {
        self.lines
            .iter()
            .map(|line| strip_ansi(line))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn panel(&self, panel_id: &str) -> Option<&TerminalNode> {
        self.tree.panel(panel_id)
    }

    pub fn diff(&self, previous: &Self) -> TerminalDiff {
        let mut before = BTreeMap::new();
        let mut after = BTreeMap::new();
        previous.tree.root.flatten(&mut before);
        self.tree.root.flatten(&mut after);

        let ids = before
            .keys()
            .chain(after.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut operations = Vec::new();
        for id in ids {
            match (before.get(&id), after.get(&id)) {
                (None, Some(node)) => operations.push(TerminalDiffOperation::Insert {
                    id,
                    node: (*node).clone(),
                }),
                (Some(_), None) => operations.push(TerminalDiffOperation::Remove { id }),
                (Some(old), Some(new)) if *old != *new => {
                    operations.push(TerminalDiffOperation::Replace {
                        id,
                        node: (*new).clone(),
                    });
                }
                _ => {}
            }
        }
        TerminalDiff {
            from_revision: previous.revision.clone(),
            to_revision: self.revision.clone(),
            operations,
            viewport_changed: previous.viewport != self.viewport,
            focus_changed: previous.focus != self.focus,
            keyboard_changed: previous.keyboard != self.keyboard,
            metadata_changed: previous.protocol != self.protocol
                || previous.session_id != self.session_id
                || previous.source_id != self.source_id
                || previous.build_id != self.build_id
                || previous.world_id != self.world_id
                || previous.observed_at != self.observed_at
                || previous.status != self.status
                || previous.capabilities != self.capabilities
                || previous.cursor != self.cursor
                || previous.freshness != self.freshness
                || previous.event_count != self.event_count
                || previous.event_limit != self.event_limit,
            lines_changed: previous
                .lines
                .iter()
                .map(|line| strip_ansi(line))
                .collect::<Vec<_>>()
                != self
                    .lines
                    .iter()
                    .map(|line| strip_ansi(line))
                    .collect::<Vec<_>>(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalDiffOperation {
    Insert { id: String, node: TerminalNode },
    Remove { id: String },
    Replace { id: String, node: TerminalNode },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalDiff {
    pub from_revision: String,
    pub to_revision: String,
    pub operations: Vec<TerminalDiffOperation>,
    pub viewport_changed: bool,
    pub focus_changed: bool,
    pub keyboard_changed: bool,
    pub metadata_changed: bool,
    pub lines_changed: bool,
}

impl TerminalDiff {
    pub fn initial(frame: &TerminalFrame) -> Self {
        let mut nodes = BTreeMap::new();
        frame.tree.root.flatten(&mut nodes);
        Self {
            from_revision: String::new(),
            to_revision: frame.revision.clone(),
            operations: nodes
                .into_iter()
                .map(|(id, node)| TerminalDiffOperation::Insert {
                    id,
                    node: node.clone(),
                })
                .collect(),
            viewport_changed: true,
            focus_changed: true,
            keyboard_changed: true,
            metadata_changed: true,
            lines_changed: true,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
            && !self.viewport_changed
            && !self.focus_changed
            && !self.keyboard_changed
            && !self.metadata_changed
            && !self.lines_changed
    }
}

/// Terminal projection host.  It retains only the bounded typed event ring
/// and derives a fresh tree/frame on demand, so capability changes cannot
/// mutate semantic nodes or introduce a second panel model.
pub struct TerminalHost {
    config: TerminalHostConfig,
    events: VecDeque<TerminalHostEvent>,
    session_id: Option<String>,
    source_id: Option<String>,
    build_id: Option<String>,
    world_id: Option<String>,
    revision: Option<String>,
    cursor: Option<u64>,
    time_cursor: Option<u64>,
    observed_at: Option<u64>,
    status: Option<DevtoolsStatusFact>,
    focus: TerminalFocus,
    keyboard: TerminalKeyboardState,
}

impl TerminalHost {
    pub fn new(mut config: TerminalHostConfig) -> Self {
        config.max_events = config.max_events.clamp(1, MAX_TERMINAL_EVENTS);
        config.max_nodes = config.max_nodes.clamp(1, MAX_TERMINAL_NODES);
        Self {
            config,
            events: VecDeque::new(),
            session_id: None,
            source_id: None,
            build_id: None,
            world_id: None,
            revision: None,
            cursor: None,
            time_cursor: None,
            observed_at: None,
            status: None,
            focus: TerminalFocus::panel(first_panel_id()),
            keyboard: TerminalKeyboardState::default(),
        }
    }

    pub fn config(&self) -> &TerminalHostConfig {
        &self.config
    }

    pub fn capabilities(&self) -> TerminalHostCapabilities {
        self.config.capabilities
    }

    /// Return every canonical panel with an explicit capability verdict.
    pub fn panel_availability(&self) -> Vec<JetDevtoolsPanelAvailability> {
        terminal_panel_availability(self.config.capabilities.access)
    }


    pub fn viewport(&self) -> TerminalViewport {
        self.effective_viewport()
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    pub fn source_id(&self) -> Option<&str> {
        self.source_id.as_deref()
    }

    pub fn build_id(&self) -> Option<&str> {
        self.build_id.as_deref()
    }

    pub fn world_id(&self) -> Option<&str> {
        self.world_id.as_deref()
    }

    pub fn observed_at(&self) -> Option<u64> {
        self.observed_at
    }

    pub fn status(&self) -> Option<&DevtoolsStatusFact> {
        self.status.as_ref()
    }

    pub fn cursor(&self) -> Option<u64> {
        self.time_cursor.or(self.cursor)
    }
    pub fn events(&self) -> impl Iterator<Item = &TerminalHostEvent> {
        self.events.iter()
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn revision(&self) -> Option<&str> {
        self.revision.as_deref()
    }

    pub fn focus(&self) -> &TerminalFocus {
        &self.focus
    }

    pub fn keyboard(&self) -> &TerminalKeyboardState {
        &self.keyboard
    }

    /// Append one event after its canonical panel identity has been supplied by
    /// the Session projection.  There is no standalone raw event path.
    fn append_event(&mut self, event: TerminalHostEvent) -> Result<(), String> {
        event.validate()?;
        if let Some(session_id) = self.session_id.as_deref() {
            if session_id != event.session_id {
                return Err("terminal event session id does not match the host".to_string());
            }
        } else {
            self.session_id = Some(event.session_id.clone());
        }
        if let Some(revision) = self.revision.as_deref() {
            if revision != event.revision {
                return Err("terminal event revision does not match the host".to_string());
            }
        } else {
            self.revision = Some(event.revision.clone());
        }
        if self.cursor.is_some_and(|cursor| event.sequence <= cursor) {
            return Err("terminal event sequence is not newer than the host cursor".to_string());
        }

        self.observed_at = Some(event.observed_at);
        self.cursor = Some(event.sequence);
        if self.time_cursor.is_none() {
            self.time_cursor = Some(event.sequence);
        }
        let first_event = self.events.is_empty();
        let panel_id = event.panel_id.as_str().to_string();
        let item_key = event_key(&event);
        if self.events.len() == self.config.max_events {
            self.events.pop_front();
        }
        self.events.push_back(event);
        if first_event {
            self.focus = TerminalFocus::item(panel_id.clone(), item_key);
        } else if self.focus.panel_id.is_none() {
            self.focus.panel_id = Some(panel_id.clone());
        }
        self.keyboard.last_action = Some(TerminalAction::FocusPanel {
            panel_id: self
                .focus
                .panel_id
                .clone()
                .unwrap_or(panel_id),
        });
        Ok(())
    }

    /// Replace the terminal projection from canonical typed Session data.
    /// `panel_ids` is produced by the canonical catalog projection; events
    /// without a first-party panel are advanced past but not rendered.
    pub fn sync_projection(
        &mut self,
        projection: &DevtoolsProjection,
        panel_ids: &BTreeMap<u64, JetDevtoolsPanelId>,
    ) -> Result<TerminalFrame, String> {
        validate_projection(projection)?;
        let session_changed = self
            .session_id
            .as_deref()
            .is_some_and(|session_id| session_id != projection.session_id);
        let revision_changed = self
            .revision
            .as_deref()
            .is_some_and(|revision| revision != projection.revision);
        if session_changed || revision_changed || projection.reset || projection.truncation {
            self.time_cursor = None;
            self.events.clear();
            self.cursor = None;
            self.focus = TerminalFocus::panel(first_panel_id());
            self.keyboard.last_action = None;
        }

        self.session_id = Some(projection.session_id.clone());
        self.source_id = projection.source_id.clone();
        self.build_id = projection.build_id.clone();
        self.world_id = projection.world_id.clone();
        self.revision = Some(projection.revision.clone());
        self.observed_at = Some(projection.observed_at);
        self.status = Some(bounded_status(&projection.status)?);

        let previous_time_cursor = self.time_cursor;
        let previous_cursor = self.cursor;
        for event in &projection.panels {
            let sequence = event.sequence();
            if previous_cursor.is_some_and(|cursor| sequence <= cursor) {
                continue;
            }
            let Some(panel_id) = panel_ids.get(&sequence).copied() else {
                continue;
            };
            self.append_event(TerminalHostEvent::from_devtools_event(
                event,
                &projection.protocol,
                &projection.session_id,
                &projection.revision,
                panel_id,
            ))?;
        }
        self.cursor = Some(projection.sequence);
        if previous_time_cursor.is_none() {
            self.time_cursor = (projection.sequence != 0).then_some(projection.sequence);
        }
        self.apply_selection(projection.selection.as_ref());
        Ok(self.render())
    }

    /// Pull the next canonical projection from a resident Session.
    pub fn sync_session(
        &mut self,
        session: &ResidentDevSession,
    ) -> Result<TerminalFrame, String> {
        let projection = session.devtools_events_since(self.cursor);
        let grants: &[JetDevtoolsPanelCapability] =
            if self.config.capabilities.access.local_details_enabled() {
                &JetDevtoolsPanelCapability::ALL
            } else {
                &[]
            };
        let panels = session
            .devtools_panels(JetDevtoolsHostKind::Terminal, grants)
            .map_err(|error| error.to_string())?;
        let mut panel_ids = BTreeMap::new();
        for panel in panels {
            let panel_id = panel.availability.descriptor.id;
            for event in panel.events {
                panel_ids.entry(event.sequence()).or_insert(panel_id);
            }
        }
        self.sync_projection(&projection, &panel_ids)
    }

    fn apply_selection(&mut self, selection: Option<&crate::Session::DevtoolsSelectionFact>) {
        let Some(selection) = selection else {
            self.focus = TerminalFocus::panel(first_panel_id());
            return;
        };
        let Some(descriptor) = crate::Devtools::catalog::descriptors()
            .iter()
            .find(|descriptor| descriptor.id.as_str() == selection.panel_id)
        else {
            self.focus = TerminalFocus::panel(first_panel_id());
            return;
        };
        let panel_id = descriptor.id.as_str();
        if selection.item_key.is_empty() {
            self.focus_panel(panel_id);
        } else {
            self.focus_item(panel_id, &selection.item_key);
        }
    }

    pub fn set_viewport(&mut self, viewport: TerminalViewport) {
        self.config.viewport = viewport;
    }

    pub fn set_capabilities(&mut self, capabilities: TerminalHostCapabilities) {
        self.config.capabilities = capabilities;
    }

    pub fn set_cursor(&mut self, cursor: Option<u64>) {
        self.cursor = cursor;
        self.time_cursor = cursor;
    }

    pub fn focus_panel(&mut self, panel_id: &str) -> bool {
        let Some(descriptor) = crate::Devtools::catalog::descriptors()
            .iter()
            .find(|descriptor| descriptor.id.as_str() == panel_id)
        else {
            return false;
        };
        let panel_id = descriptor.id.as_str().to_string();
        self.focus = TerminalFocus::panel(panel_id.clone());
        self.keyboard.last_action = Some(TerminalAction::FocusPanel { panel_id });
        true
    }

    pub fn focus_item(&mut self, panel_id: &str, item_key: &str) -> bool {
        if !self.focus_panel(panel_id) {
            return false;
        }
        self.focus.item_key = Some(item_key.to_string());
        self.keyboard.last_action = Some(TerminalAction::Inspect {
            panel_id: self.focus.panel_id.clone().unwrap_or_default(),
            item_key: item_key.to_string(),
        });
        true
    }

    pub fn clear_item_focus(&mut self) {
        self.focus.clear_item();
        self.keyboard.last_action = Some(TerminalAction::ReturnToPanel);
    }

    pub fn handle(&mut self, input: TerminalInput) -> TerminalAction {
        match input {
            TerminalInput::Key(key) => self.handle_key(key),
            TerminalInput::Resize(viewport) => {
                self.set_viewport(viewport);
                let action = TerminalAction::Ignored;
                self.keyboard.last_action = Some(action.clone());
                action
            }
        }
    }

    /// Keyboard paths are explicit and side-effect free outside host focus and
    /// cursor state.  Rerun/restart/job/quit actions are returned to the real
    /// `jet dev` command; this adapter never executes them.
    pub fn handle_key(&mut self, key: TerminalKey) -> TerminalAction {
        self.keyboard.last_key = Some(key);
        let action = match key {
            TerminalKey::Tab | TerminalKey::Down => self.move_panel(1),
            TerminalKey::BackTab | TerminalKey::Up => self.move_panel(-1),
            TerminalKey::Enter | TerminalKey::Right => self.inspect_focused(),
            TerminalKey::Escape | TerminalKey::Left => {
                if self.focus.item_key.is_some() {
                    self.clear_item_focus();
                    TerminalAction::ReturnToPanel
                } else {
                    TerminalAction::Ignored
                }
            }
            TerminalKey::Character('r') if self.config.capabilities.access.actions_enabled() => {
                TerminalAction::Rerun
            }
            TerminalKey::Character('R') if self.config.capabilities.access.actions_enabled() => {
                TerminalAction::Restart
            }
            TerminalKey::Character('j') => {
                self.focus_panel("jobs");
                TerminalAction::FocusJob
            }
            TerminalKey::Character('q') => TerminalAction::Quit,
            TerminalKey::Character('t') => self.move_cursor(),
            TerminalKey::Character(_) => TerminalAction::Ignored,
        };
        self.keyboard.last_action = Some(action.clone());
        action
    }

    pub fn render(&self) -> TerminalFrame {
        let ordered = ordered_events(&self.events);
        let viewport = self.effective_viewport();
        let tree = self.build_tree(&ordered);
        let freshness = overall_freshness(&ordered, self.display_cursor());
        let revision = self
            .revision
            .clone()
            .or_else(|| ordered.last().map(|event| event.revision.clone()))
            .unwrap_or_default();
        let lines = render_lines(
            &tree,
            &viewport,
            self.config.capabilities,
            self.session_id.as_deref().unwrap_or(""),
            self.source_id.as_deref(),
            self.build_id.as_deref(),
            self.world_id.as_deref(),
            &revision,
            self.display_cursor(),
            self.observed_at,
            self.status.as_ref(),
            &self.focus,
            &self.keyboard,
            ordered.len(),
            self.config.max_events,
            freshness,
        );
        TerminalFrame {
            protocol: JET_DEVTOOLS_PROTOCOL.to_string(),
            session_id: self.session_id.clone().unwrap_or_default(),
            source_id: self.source_id.clone(),
            build_id: self.build_id.clone(),
            world_id: self.world_id.clone(),
            observed_at: self.observed_at,
            status: self.status.clone(),
            revision,
            cursor: self.display_cursor(),
            viewport,
            capabilities: self.config.capabilities,
            focus: self.focus.clone(),
            keyboard: self.keyboard.clone(),
            freshness,
            event_count: ordered.len(),
            event_limit: self.config.max_events,
            tree,
            lines,
        }
    }

    pub fn render_diff(&self, previous: Option<&TerminalFrame>) -> TerminalDiff {
        let frame = self.render();
        previous.map_or_else(|| TerminalDiff::initial(&frame), |old| frame.diff(old))
    }
    fn move_panel(&mut self, direction: isize) -> TerminalAction {
        let catalog = crate::Devtools::catalog::descriptors();
        if catalog.is_empty() {
            return TerminalAction::Ignored;
        }
        let current = self
            .focus
            .panel_id
            .as_deref()
            .and_then(|panel| {
                catalog
                    .iter()
                    .position(|descriptor| descriptor.id.as_str() == panel)
            })
            .unwrap_or(0);
        let len = catalog.len() as isize;
        let next = (current as isize + direction).rem_euclid(len) as usize;
        let panel_id = catalog[next].id.as_str().to_string();
        self.focus = TerminalFocus::panel(panel_id.clone());
        TerminalAction::FocusPanel { panel_id }
    }


    fn inspect_focused(&mut self) -> TerminalAction {
        let panel_id = self
            .focus
            .panel_id
            .clone()
            .unwrap_or_else(|| first_panel_id().to_string());
        if let Some(item_key) = self.focus.item_key.clone() {
            return TerminalAction::Inspect { panel_id, item_key };
        }
        let item_key = ordered_events(&self.events)
            .into_iter()
            .rev()
            .find(|event| self.panel_id_for_event(event) == panel_id)
            .map(|event| event_key(event));
        if let Some(item_key) = item_key {
            self.focus.item_key = Some(item_key.clone());
            return TerminalAction::Inspect { panel_id, item_key };
        }
        TerminalAction::FocusPanel { panel_id }
    }

    fn move_cursor(&mut self) -> TerminalAction {
        let mut sequences = ordered_events(&self.events)
            .into_iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>();
        sequences.sort_unstable();
        sequences.dedup();
        if sequences.is_empty() {
            return TerminalAction::Ignored;
        }
        let current = self.time_cursor.unwrap_or(sequences[0]);
        let next = sequences
            .iter()
            .copied()
            .find(|sequence| *sequence > current)
            .unwrap_or(sequences[0]);
        self.time_cursor = Some(next);
        TerminalAction::MoveCursor { cursor: next }
    }
    fn effective_viewport(&self) -> TerminalViewport {
        let configured = self.config.viewport;
        configured.resize(
            configured.width.min(self.config.capabilities.width.max(1)),
            configured.height,
        )
    }
    fn panel_id_for_event(&self, event: &TerminalHostEvent) -> &'static str {
        event.panel_id.as_str()
    }
    fn display_cursor(&self) -> Option<u64> {
        self.time_cursor.or(self.cursor)
    }


    fn build_tree(&self, ordered: &[&TerminalHostEvent]) -> TerminalPanelTree {
        let viewport = self.effective_viewport();
        let root_bounds = viewport.bounds();
        let mut children = Vec::new();
        let availability = self.panel_availability();

        let mut remaining = self.config.max_nodes.saturating_sub(1);
        let panel_height = 3usize;
        for (index, descriptor) in crate::Devtools::catalog::descriptors()
            .iter()
            .enumerate()
        {
            let panel_id = descriptor.id.as_str();
            let panel_title = descriptor.title;
            if remaining == 0 {
                break;
            }
            remaining -= 1;
            let panel_events = ordered
                .iter()
                .copied()
                .filter(|event| self.panel_id_for_event(event) == panel_id)
                .collect::<Vec<_>>();
            let freshness = panel_freshness(&panel_events, self.display_cursor());
            let y = index.saturating_mul(panel_height);
            let height = viewport.height.saturating_sub(y).min(panel_height);
            let panel_bounds = TerminalBounds::new(0, y, viewport.width, height);
            let mut panel_children = Vec::new();
            if remaining > 0 {
                let panel_state = availability
                    .iter()
                    .find(|row| row.descriptor.id == descriptor.id);
                let available = panel_state.map_or(true, JetDevtoolsPanelAvailability::is_available);
                let status = TerminalStatus {
                    label: if available {
                        freshness.label().to_string()
                    } else {
                        "unavailable".to_string()
                    },
                    detail: if available {
                        format!(
                            "{} event(s), retained {}/{}",
                            panel_events.len(),
                            ordered.len(),
                            self.config.max_events
                        )
                    } else {
                        panel_state
                            .map(|row| unavailable_panel_detail(row.unavailable_reasons()))
                            .unwrap_or_else(|| "unavailable".to_string())
                    },
                    freshness,
                };
                panel_children.push(TerminalNode::status(
                    format!("panel:{panel_id}:status"),
                    "freshness",
                    status,
                    TerminalBounds::new(0, y, viewport.width, height.min(1)),
                    self.focus.panel_id.as_deref() == Some(panel_id),
                ));
                remaining -= 1;
            }
            if remaining > 0 {
                let summary = panel_events
                    .last()
                    .map(|event| event_summary(event, self.config.capabilities))
                    .unwrap_or_else(|| "No recorded facts".to_string());
                panel_children.push(TerminalNode::text(
                    format!("panel:{panel_id}:summary"),
                    "summary",
                    summary,
                    TerminalBounds::new(0, y.saturating_add(1), viewport.width, height.saturating_sub(1).min(1)),
                    freshness,
                ));
                remaining -= 1;
            }
            if remaining > 0 {
                let rows = panel_events
                    .iter()
                    .map(|event| TerminalTableRow {
                        key: event_key(event),
                        cells: vec![
                            event.revision.clone(),
                            event.sequence.to_string(),
                            event.observed_at.to_string(),
                            event.kind.clone(),
                            event.entity.clone(),
                            event.source.clone(),
                            event.published_fact_count().to_string(),
                        ],
                    })
                    .collect::<Vec<_>>();
                panel_children.push(TerminalNode::table(
                    format!("panel:{panel_id}:events"),
                    "events",
                    TerminalTable {
                        columns: vec![
                            "revision".to_string(),
                            "sequence".to_string(),
                            "observed_at".to_string(),
                            "kind".to_string(),
                            "entity".to_string(),
                            "source".to_string(),
                            "published_facts".to_string(),
                        ],
                        rows,
                    },
                    TerminalBounds::new(0, y.saturating_add(2), viewport.width, height.saturating_sub(2)),
                    freshness,
                ));
                remaining -= 1;
            }
            children.push(TerminalNode::container(
                format!("panel:{panel_id}"),
                TerminalNodeKind::Panel,
                panel_title,
                panel_bounds,
                freshness,
                panel_children,
            ));
        }
        let root = TerminalNode::container(
            "terminal-root",
            TerminalNodeKind::Root,
            "jet devtools",
            root_bounds,
            overall_freshness(ordered, self.display_cursor()),
            children,
        );
        let panel_count = root.children.len();
        let node_count = root.count();
        TerminalPanelTree {
            root,
            bounds: root_bounds,
            node_count,
            node_limit: self.config.max_nodes,
            panel_count,
        }
    }
}

impl Default for TerminalHost {
    fn default() -> Self {
        Self::new(TerminalHostConfig::default())
    }
}
fn validate_text(value: &str, label: &str) -> Result<(), String> {
    validate_optional_text(value, label, false)
}

fn validate_optional_text(value: &str, label: &str, allow_empty: bool) -> Result<(), String> {
    if (!allow_empty && value.is_empty())
        || value.len() > MAX_TERMINAL_TEXT_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(format!("terminal {label} is empty, too long, or contains control text"));
    }
    Ok(())
}

fn validate_projection(projection: &DevtoolsProjection) -> Result<(), String> {
    if projection.protocol != JET_DEVTOOLS_PROTOCOL {
        return Err(format!(
            "devtools projection protocol must be {JET_DEVTOOLS_PROTOCOL}"
        ));
    }
    validate_text(&projection.session_id, "session id")?;
    validate_optional_text(&projection.revision, "revision", true)?;
    for (value, label) in [
        (projection.source_id.as_deref(), "source id"),
        (projection.build_id.as_deref(), "build id"),
        (projection.world_id.as_deref(), "world id"),
    ] {
        if let Some(value) = value {
            validate_text(value, label)?;
        }
    }
    validate_status(&projection.status)?;
    for event in &projection.panels {
        if event.sequence() == 0 {
            return Err("devtools event sequence must be positive".to_string());
        }
        if event.sequence() > projection.sequence {
            return Err("devtools event sequence exceeds the projection cursor".to_string());
        }
        validate_text(event.source(), "event source")?;
        validate_text(event.kind(), "event kind")?;
        validate_text(event.entity(), "event entity")?;
        validate_text(event.fields_json(), "event fields")?;
        if let Some(payload) = event.payload_json() {
            validate_text(payload, "event payload")?;
        }
    }
    if let Some(selection) = &projection.selection {
        let valid_panel = crate::Devtools::catalog::descriptors()
            .iter()
            .any(|descriptor| descriptor.id.as_str() == selection.panel_id);
        if !valid_panel {
            return Err(format!(
                "devtools selection panel `{}` is not canonical",
                selection.panel_id
            ));
        }
        validate_optional_text(&selection.item_key, "selection item", true)?;
    }
    Ok(())
}

fn validate_status(status: &DevtoolsStatusFact) -> Result<(), String> {
    for (value, label) in [
        (status.diagnostic_code.as_deref(), "diagnostic code"),
        (status.diagnostic.as_deref(), "diagnostic"),
        (status.accepted_revision.as_deref(), "accepted revision"),
        (status.last_good_revision.as_deref(), "last good revision"),
        (status.run_target.as_deref(), "run target"),
        (status.run_output.as_deref(), "run output"),
        (status.test_state.as_deref(), "test state"),
    ] {
        if let Some(value) = value {
            if value.is_empty() {
                return Err(format!("terminal {label} is empty"));
            }
        }
    }
    Ok(())
}

fn bounded_status(status: &DevtoolsStatusFact) -> Result<DevtoolsStatusFact, String> {
    validate_status(status)?;
    let mut bounded = status.clone();
    for value in [
        &mut bounded.diagnostic_code,
        &mut bounded.diagnostic,
        &mut bounded.accepted_revision,
        &mut bounded.last_good_revision,
        &mut bounded.run_target,
        &mut bounded.run_output,
        &mut bounded.test_state,
    ] {
        if let Some(value) = value {
            *value = bounded_text(value, MAX_TERMINAL_TEXT_BYTES);
        }
    }
    Ok(bounded)
}

fn bounded_text(value: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    let mut output = String::with_capacity(value.len().min(limit));
    for character in value.chars() {
        let replacement = match character {
            '\n' => "\\n",
            '\r' => "\\r",
            '\t' => "\\t",
            character if character.is_control() => "?",
            _ => {
                if output.len().saturating_add(character.len_utf8()) > limit {
                    break;
                }
                output.push(character);
                continue;
            }
        };
        if output.len().saturating_add(replacement.len()) > limit {
            break;
        }
        output.push_str(replacement);
    }
    output
}

fn display_optional(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else {
        value
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn access_label(access: TerminalHostAccess) -> &'static str {
    if access.read_only {
        "read-only"
    } else if access.release {
        "release"
    } else if access.local {
        "local"
    } else {
        "remote"
    }
}

fn session_state_label(state: &DevtoolsSessionState) -> &'static str {
    match state {
        DevtoolsSessionState::Starting => "starting",
        DevtoolsSessionState::Building => "building",
        DevtoolsSessionState::Ready => "ready",
        DevtoolsSessionState::Error => "error",
        DevtoolsSessionState::Unavailable => "unavailable",
        DevtoolsSessionState::Stopped => "stopped",
    }
}

fn status_summary(status: &DevtoolsStatusFact, reveal: bool) -> String {
    let mut fields = vec![format!("state={}", session_state_label(&status.state))];
    for (key, value) in [
        ("diagnostic_code", status.diagnostic_code.as_deref()),
        ("diagnostic", status.diagnostic.as_deref()),
        ("accepted_revision", status.accepted_revision.as_deref()),
        ("last_good_revision", status.last_good_revision.as_deref()),
        ("run_target", status.run_target.as_deref()),
        ("run_output", status.run_output.as_deref()),
        ("test_state", status.test_state.as_deref()),
    ] {
        let rendered = match (value, reveal) {
            (Some(value), true) => value,
            (Some(_), false) => "<redacted>",
            (None, _) => "-",
        };
        fields.push(format!("{key}={rendered}"));
    }
    fields.join(" ")
}


fn unavailable_panel_detail(
    reasons: &[JetDevtoolsPanelUnavailableReason],
) -> String {
    let details = reasons
        .iter()
        .map(|reason| match reason {
            JetDevtoolsPanelUnavailableReason::UnsupportedHost(host) => {
                format!("unsupported host {}", host.as_str())
            }
            JetDevtoolsPanelUnavailableReason::MissingCapability(capability) => {
                format!("missing {}", capability.grant())
            }
        })
        .collect::<Vec<_>>();
    if details.is_empty() {
        "unavailable".to_string()
    } else {
        details.join(", ")
    }
}

fn ordered_events<'a>(events: &'a VecDeque<TerminalHostEvent>) -> Vec<&'a TerminalHostEvent> {
    let mut ordered = events.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.sequence
            .cmp(&right.sequence)
            .then_with(|| left.observed_at.cmp(&right.observed_at))
            .then_with(|| left.revision.cmp(&right.revision))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.entity.cmp(&right.entity))
            .then_with(|| left.source.cmp(&right.source))
    });
    ordered
}

fn event_key(event: &TerminalHostEvent) -> String {
    format!("event:{}:{}:{}:{}", event.revision, event.sequence, event.kind, event.entity)
}

fn event_summary(event: &TerminalHostEvent, capabilities: TerminalHostCapabilities) -> String {
    let details_enabled = capabilities.access.local_details_enabled();
    let published = if details_enabled {
        event
            .facts
            .iter()
            .filter(|fact| fact.published)
            .map(TerminalHostFact::display)
            .collect::<Vec<_>>()
            .join(",")
    } else {
        "<redacted>".to_string()
    };
    let values = if published.is_empty() {
        "-"
    } else {
        &published
    };
    let verbose = if capabilities.verbose { " details=on" } else { "" };
    format!(
        "{} · {} · {} · {}ms · source={} · published_facts={} · values={}{}",
        event.kind,
        event.entity,
        if event.revision.is_empty() {
            "-"
        } else {
            &event.revision
        },
        event.observed_at,
        event.source,
        event.published_fact_count(),
        values,
        verbose,
    )
}

fn panel_freshness(events: &[&TerminalHostEvent], cursor: Option<u64>) -> TerminalFreshness {
    let Some(latest) = events.iter().map(|event| event.sequence).max() else {
        return TerminalFreshness::Empty;
    };
    if cursor.is_some_and(|cursor| cursor < latest) {
        TerminalFreshness::Stale
    } else {
        TerminalFreshness::Fresh
    }
}

fn overall_freshness(events: &[&TerminalHostEvent], cursor: Option<u64>) -> TerminalFreshness {
    if events.is_empty() {
        return TerminalFreshness::Empty;
    }
    if events
        .iter()
        .any(|event| cursor.is_some_and(|cursor| cursor < event.sequence))
    {
        TerminalFreshness::Stale
    } else {
        TerminalFreshness::Fresh
    }
}

fn render_lines(
    tree: &TerminalPanelTree,
    viewport: &TerminalViewport,
    capabilities: TerminalHostCapabilities,
    session_id: &str,
    source_id: Option<&str>,
    build_id: Option<&str>,
    world_id: Option<&str>,
    revision: &str,
    cursor: Option<u64>,
    observed_at: Option<u64>,
    status: Option<&DevtoolsStatusFact>,
    focus: &TerminalFocus,
    keyboard: &TerminalKeyboardState,
    event_count: usize,
    event_limit: usize,
    freshness: TerminalFreshness,
) -> Vec<String> {
    if capabilities.quiet {
        return Vec::new();
    }
    let mut raw = Vec::new();
    raw.push(format!(
        "jet devtools  session={}  source_id={}  build_id={}  world_id={}  revision={}  sequence={}  observed_at={}  state={}  freshness={}",
        display_optional(session_id),
        display_optional(source_id.unwrap_or("")),
        display_optional(build_id.unwrap_or("")),
        display_optional(world_id.unwrap_or("")),
        display_optional(revision),
        cursor.map_or_else(|| "-".to_string(), |value| value.to_string()),
        observed_at.map_or_else(|| "-".to_string(), |value| value.to_string()),
        status
            .map(|status| session_state_label(&status.state))
            .unwrap_or("-"),
        freshness.label(),
    ));
    raw.push(format!(
        "terminal={}x{}  tty={}  ansi={}  no_color={}  verbose={}  quiet={}  access={}  local={}  release={}  read_only={}  nodes={}/{}  events={}/{}  focus={}",
        viewport.width,
        viewport.height,
        yes_no(capabilities.tty),
        yes_no(capabilities.ansi),
        yes_no(capabilities.no_color),
        yes_no(capabilities.verbose),
        yes_no(capabilities.quiet),
        access_label(capabilities.access),
        yes_no(capabilities.access.local),
        yes_no(capabilities.access.release),
        yes_no(capabilities.access.read_only),
        tree.node_count,
        tree.node_limit,
        event_count,
        event_limit,
        focus.panel_id.as_deref().unwrap_or("-"),
    ));
    if let Some(status) = status {
        raw.push(format!(
            "status: {}",
            status_summary(status, capabilities.access.local_details_enabled())
        ));
    }
    for panel in &tree.root.children {
        let selected = focus.panel_id.as_deref().is_some_and(|panel_id| {
            panel
                .id
                .strip_prefix("panel:")
                .is_some_and(|candidate| candidate == panel_id)
        });
        let marker = if selected { ">" } else { " " };
        let panel_label = if selected {
            style(&format!("{marker}[{}]", panel.label), "36", capabilities)
        } else {
            format!("{marker}[{}]", panel.label)
        };
        raw.push(format!("{panel_label} {}", panel.freshness.label()));
        for child in &panel.children {
            match child.kind {
                TerminalNodeKind::Status => {
                    if let Some(status) = &child.status {
                        raw.push(format!(
                            "  status={}  {}",
                            style(&status.label, "33", capabilities),
                            status.detail
                        ));
                    }
                }
                TerminalNodeKind::Text => {
                    raw.push(format!("  {}", child.text.as_deref().unwrap_or("")));
                }
                TerminalNodeKind::Table => {
                    if let Some(table) = &child.table {
                        for row in &table.rows {
                            raw.push(format!("  {}", row.cells.join(" | ")));
                        }
                    }
                }
                TerminalNodeKind::Root | TerminalNodeKind::Panel => {}
            }
        }
    }
    let action_help = if capabilities.access.actions_enabled() {
        "r rerun  R restart"
    } else {
        "r/R disabled(read-only)"
    };
    raw.push(format!(
        "keys: Tab/Down next panel  Shift-Tab/Up previous  Enter inspect  Esc back  {action_help}  j jobs  t time  q quit  last={}",
        keyboard
            .last_action
            .as_ref()
            .map(action_label)
            .unwrap_or("none")
    ));
    let skip = viewport.scroll.min(raw.len());
    raw.into_iter()
        .skip(skip)
        .take(viewport.height)
        .map(|line| truncate_columns(&line, viewport.width))
        .collect()
}

fn action_label(action: &TerminalAction) -> &'static str {
    match action {
        TerminalAction::FocusPanel { .. } => "focus-panel",
        TerminalAction::Inspect { .. } => "inspect",
        TerminalAction::ReturnToPanel => "back",
        TerminalAction::MoveCursor { .. } => "time",
        TerminalAction::Rerun => "rerun",
        TerminalAction::Restart => "restart",
        TerminalAction::FocusJob => "jobs",
        TerminalAction::Quit => "quit",
        TerminalAction::Ignored => "ignored",
    }
}

fn style(text: &str, code: &str, capabilities: TerminalHostCapabilities) -> String {
    if capabilities.color_enabled() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn strip_ansi(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.next() == Some('[') {
            for code in chars.by_ref() {
                if ('@'..='~').contains(&code) {
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn truncate_columns(text: &str, width: usize) -> String {
    let limit = width.max(1);
    let mut output = String::with_capacity(text.len().min(limit.saturating_add(16)));
    let mut chars = text.chars().peekable();
    let mut visible = 0usize;
    let mut clipped = false;
    let has_ansi = text.contains('\x1b');
    while let Some(character) = chars.next() {
        if character == '\x1b' && chars.peek() == Some(&'[') {
            output.push(character);
            output.push(chars.next().expect("CSI introducer follows escape"));
            for code in chars.by_ref() {
                output.push(code);
                if ('@'..='~').contains(&code) {
                    break;
                }
            }
            continue;
        }
        if visible >= limit {
            clipped = true;
            break;
        }
        output.push(character);
        visible += 1;
    }
    if clipped && has_ansi {
        output.push_str("\x1b[0m");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        sequence: u64,
        panel_id: JetDevtoolsPanelId,
        kind: &str,
        entity: &str,
    ) -> TerminalHostEvent {
        TerminalHostEvent::new(
            panel_id,
            "session",
            "revision-a",
            sequence,
            sequence * 10,
            "src/main.jet",
            kind,
            entity,
            vec![TerminalHostFact::redacted("value")],
        )
    }

    fn panel_ids() -> BTreeMap<u64, JetDevtoolsPanelId> {
        [(1, JetDevtoolsPanelId::Build)].into_iter().collect()
    }

    fn projection() -> DevtoolsProjection {
        let mut envelope = crate::Devtools::JetDevtoolsEnvelope::new("session", 0);
        envelope.push(
            JetDevtoolsEvent::from_parts_with_payload(
                1200,
                "devserver",
                "Build",
                "build",
                "{\"accepted\":true}",
                Some("{\"output\":\"ok\"}".to_string()),
            )
            .unwrap(),
        );
        DevtoolsProjection {
            protocol: JET_DEVTOOLS_PROTOCOL.to_string(),
            session_id: "session".to_string(),
            source_id: Some("src/main.jet".to_string()),
            build_id: Some("build-42".to_string()),
            revision: "revision-a".to_string(),
            world_id: Some("world-7".to_string()),
            sequence: 1,
            observed_at: 1234,
            panels: envelope.events().cloned().collect(),
            selection: Some(crate::Session::DevtoolsSelectionFact {
                panel_id: "build".to_string(),
                item_key: "event:revision-a:1:Build:build".to_string(),
            }),
            status: DevtoolsStatusFact {
                state: DevtoolsSessionState::Ready,
                diagnostic_code: Some("E0001".to_string()),
                diagnostic: Some("diagnostic".to_string()),
                accepted_revision: Some("revision-a".to_string()),
                last_good_revision: Some("revision-a".to_string()),
                run_target: Some("native".to_string()),
                run_output: Some("ok".to_string()),
                test_state: Some("passed".to_string()),
            },
            command_capabilities: crate::Session::DevtoolsCommandCapabilities {
                project_rebuild: false,
                project_rebuild_reason: Some(
                    "terminal host test projection has no rebuild executor".to_string(),
                ),
            },
            command_receipts: Vec::new(),
            reset: true,
            truncation: false,
        }
    }

    #[test]
    fn sync_projection_preserves_canonical_identity_and_status() {
        let capabilities = TerminalHostCapabilities::new(
            false,
            false,
            true,
            32,
            true,
            false,
            TerminalHostAccess::new(true, false, true),
        );
        let mut host = TerminalHost::new(TerminalHostConfig::new(
            TerminalViewport::new(80, 24),
            capabilities,
        ));
        let frame = host
            .sync_projection(&projection(), &panel_ids())
            .unwrap();
        assert_eq!(frame.session_id, "session");
        assert_eq!(frame.source_id.as_deref(), Some("src/main.jet"));
        assert_eq!(frame.build_id.as_deref(), Some("build-42"));
        assert_eq!(frame.world_id.as_deref(), Some("world-7"));
        assert_eq!(frame.revision, "revision-a");
        assert_eq!(frame.cursor, Some(1));
        assert_eq!(frame.observed_at, Some(1234));
        assert_eq!(frame.status.as_ref().map(|status| &status.state), Some(&DevtoolsSessionState::Ready));
        let event = host.events().next().unwrap();
        assert_eq!(event.observed_at, 1200);
        assert_eq!(
            event.facts[0].value,
            Some(TerminalFactValue::Text("{\"accepted\":true}".to_string()))
        );
        assert_eq!(host.focus().item_key.as_deref(), Some("event:revision-a:1:Build:build"));
        assert!(host.handle_key(TerminalKey::Character('r')) == TerminalAction::Ignored);
        assert!(frame.lines.iter().all(|line| strip_ansi(line).chars().count() <= 32));
    }

    #[test]
    fn quiet_suppresses_text_without_dropping_typed_tree() {
        let capabilities = TerminalHostCapabilities::new(
            false,
            false,
            true,
            20,
            false,
            true,
            TerminalHostAccess::interactive(),
        );
        let mut host = TerminalHost::new(TerminalHostConfig::new(
            TerminalViewport::new(80, 24),
            capabilities,
        ));
        let frame = host
            .sync_projection(&projection(), &panel_ids())
            .unwrap();
        assert!(frame.lines.is_empty());
        assert_eq!(
            frame.tree.panel_count,
            crate::Devtools::catalog::descriptors().len()
        );
    }

    #[test]
    fn same_facts_keep_nodes_stable_across_capabilities() {
        let mut plain = TerminalHost::new(TerminalHostConfig::new(
            TerminalViewport::new(80, 24),
            TerminalHostCapabilities::plain(),
        ));
        let mut color = TerminalHost::new(TerminalHostConfig::new(
            TerminalViewport::new(80, 24),
            TerminalHostCapabilities::color(),
        ));
        plain
            .append_event(event(1, JetDevtoolsPanelId::Build, "Build", "build"))
            .unwrap();
        color
            .append_event(event(1, JetDevtoolsPanelId::Build, "Build", "build"))
            .unwrap();
        let plain_frame = plain.render();
        let color_frame = color.render();
        assert_eq!(plain_frame.tree, color_frame.tree);
        assert!(!plain_frame.text().contains('\x1b'));
        assert!(color_frame.text().contains('\x1b'));
        assert!(color_frame.diff(&plain_frame).metadata_changed);
    }

    #[test]
    fn keyboard_paths_update_explicit_focus_and_return_actions() {
        let mut host = TerminalHost::default();
        host
            .append_event(event(1, JetDevtoolsPanelId::Tests, "Tests", "fails"))
            .unwrap();
        assert_eq!(host.handle_key(TerminalKey::Enter), TerminalAction::Inspect {
            panel_id: "tests".to_string(),
            item_key: "event:revision-a:1:Tests:fails".to_string(),
        });
        assert_eq!(host.handle_key(TerminalKey::Escape), TerminalAction::ReturnToPanel);
        assert!(host.focus().item_key.is_none());
    }

    #[test]
    fn diff_operations_are_ordered_and_bounded() {
        let mut host = TerminalHost::default();
        host
            .append_event(event(1, JetDevtoolsPanelId::Build, "Build", "one"))
            .unwrap();
        let first = host.render();
        host
            .append_event(event(2, JetDevtoolsPanelId::Jobs, "Jobs", "two"))
            .unwrap();
        let second = host.render();
        let diff = second.diff(&first);
        assert!(!diff.is_empty());
        let ids = diff
            .operations
            .iter()
            .map(|operation| match operation {
                TerminalDiffOperation::Insert { id, .. }
                | TerminalDiffOperation::Remove { id }
                | TerminalDiffOperation::Replace { id, .. } => id,
            })
            .collect::<Vec<_>>();
        assert!(ids.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(second.tree.node_count <= second.tree.node_limit);
    }
}
