//! Typed native GUI/game overlay projection over `jet.devtools.v1`.
//!
//! The host consumes canonical typed session events and returns bounded draw
//! commands to the real game-window backend.  It does not parse the wire
//! envelope or execute actions; `NativeOverlayAdapter` remains the only typed
//! projection bridge.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

use crate::Devtools::{
    JetDevtoolsEvent, JetDevtoolsNativeColor, JetDevtoolsNativeDrawCommand,
    JetDevtoolsNativeFrame, JetDevtoolsNativeHost, JetDevtoolsNativeInput,
    JetDevtoolsNativeWindow, JetDevtoolsSelection, JetDevtoolsViewState,
};
use crate::NativeOverlayAdapter::NativeOverlayAdapter;
use crate::Session::ResidentDevSession;
use crate::TerminalHost::{
    panel_availability as project_panel_availability, JetDevtoolsHostKind,
    JetDevtoolsPanelAvailability, JetDevtoolsPanelCapability, JetDevtoolsPanelDescriptor,
};


/// The only protocol accepted by this host projection.
pub const JET_DEVTOOLS_PROTOCOL: &str = "jet.devtools.v1";

/// Bounded panel count for one native projection.
pub const MAX_OVERLAY_PANELS: usize = 64;
/// Bounded node count across all panel trees in one event/frame.
pub const MAX_OVERLAY_NODES: usize = 512;
/// Bounded tree depth accepted from a panel producer.
pub const MAX_OVERLAY_DEPTH: usize = 32;
/// Bounded retained frame snapshots.
pub const MAX_OVERLAY_FRAMES: usize = 64;
/// Shared devtools text budget for typed labels and identifiers.
pub const MAX_OVERLAY_TEXT_BYTES: usize = 16 * 1024;

/// The closed node vocabulary understood by a native overlay renderer.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NativeOverlayNodeKind {
    Text,
    Status,
    Table,
    Tree,
    Chart,
    Action,
}

/// A grant required before a typed action can be executed by a later host
/// integration.  This module only projects the requirement; it never grants or
/// executes it.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NativeOverlayGrant {
    Observe,
    Interact,
    Control,
}

/// The complete action metadata carried by an action node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeOverlayAction {
    pub action_id: String,
    pub required_grant: NativeOverlayGrant,
}

impl NativeOverlayAction {
    pub fn new(action_id: impl Into<String>, required_grant: NativeOverlayGrant) -> Self {
        Self {
            action_id: action_id.into(),
            required_grant,
        }
    }
}

/// A backend-neutral node.  Values are optional by design: the projection does
/// not invent or recover payload values that the observation site did not
/// publish.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeOverlayNode {
    pub id: String,
    pub kind: NativeOverlayNodeKind,
    pub label: String,
    pub value: Option<String>,
    pub status: Option<String>,
    pub children: Vec<NativeOverlayNode>,
    pub action: Option<NativeOverlayAction>,
}

impl NativeOverlayNode {
    pub fn new(
        id: impl Into<String>,
        kind: NativeOverlayNodeKind,
        label: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            label: label.into(),
            value: None,
            status: None,
            children: Vec::new(),
            action: None,
        }
    }

    pub fn text(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(id, NativeOverlayNodeKind::Text, label)
    }

    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    pub fn with_status(mut self, status: impl Into<String>) -> Self {
        self.status = Some(status.into());
        self
    }

    pub fn with_children(mut self, children: Vec<NativeOverlayNode>) -> Self {
        self.children = children;
        self
    }

    pub fn with_action(mut self, action: NativeOverlayAction) -> Self {
        self.action = Some(action);
        self
    }

    fn validate(
        &self,
        depth: usize,
        count: &mut usize,
        ids: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        if depth > MAX_OVERLAY_DEPTH {
            return Err("native overlay node tree exceeds its depth budget".to_string());
        }
        validate_text(&self.id, "native overlay node id", false)?;
        validate_text(&self.label, "native overlay node label", false)?;
        if !ids.insert(self.id.clone()) {
            return Err(format!(
                "native overlay node id `{}` is duplicated",
                self.id
            ));
        }
        *count = count
            .checked_add(1)
            .ok_or_else(|| "native overlay node count overflowed".to_string())?;
        if *count > MAX_OVERLAY_NODES {
            return Err("native overlay frame exceeds its node budget".to_string());
        }
        if let Some(value) = self.value.as_deref() {
            validate_text(value, "native overlay node value", true)?;
        }
        if let Some(status) = self.status.as_deref() {
            validate_text(status, "native overlay node status", true)?;
        }
        match (&self.kind, &self.action) {
            (NativeOverlayNodeKind::Action, None) => {
                return Err(format!(
                    "native overlay action node `{}` has no typed action",
                    self.id
                ));
            }
            (NativeOverlayNodeKind::Action, Some(action)) => {
                validate_text(&action.action_id, "native overlay action id", false)?;
            }
            (_, Some(_)) => {
                return Err(format!(
                    "native overlay node `{}` carries an action but is not an action node",
                    self.id
                ));
            }
            (_, None) => {}
        }
        for child in &self.children {
            child.validate(depth + 1, count, ids)?;
        }
        Ok(())
    }
}

/// One panel's already-typed facts.  The native host keeps these fields intact
/// and does not infer panel meaning from their names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeOverlayPanelFact {
    pub panel_id: String,
    pub title: String,
    pub freshness: u64,
    pub status: String,
    pub nodes: Vec<NativeOverlayNode>,
}

impl NativeOverlayPanelFact {
    pub fn new(
        panel_id: impl Into<String>,
        title: impl Into<String>,
        freshness: u64,
        status: impl Into<String>,
        nodes: Vec<NativeOverlayNode>,
    ) -> Self {
        Self {
            panel_id: panel_id.into(),
            title: title.into(),
            freshness,
            status: status.into(),
            nodes,
        }
    }

    fn validate(&self, count: &mut usize) -> Result<(), String> {
        validate_text(&self.panel_id, "native overlay panel id", false)?;
        validate_text(&self.title, "native overlay panel title", false)?;
        validate_text(&self.status, "native overlay panel status", true)?;
        let mut ids = BTreeSet::new();
        for node in &self.nodes {
            node.validate(0, count, &mut ids)?;
        }
        Ok(())
    }
}

/// The typed projection adapter hands to [`NativeOverlayHost`].  It is
/// deliberately narrower than the wire envelope: protocol/session/revision
/// identity stays visible, while panel facts are the only renderable input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeOverlayEvent {
    pub protocol: String,
    pub session_id: String,
    pub revision: String,
    pub sequence: u64,
    pub observed_at_ms: u64,
    pub panels: Vec<NativeOverlayPanelFact>,
}

impl NativeOverlayEvent {
    pub fn new(
        protocol: impl Into<String>,
        session_id: impl Into<String>,
        revision: impl Into<String>,
        sequence: u64,
        observed_at_ms: u64,
        panels: Vec<NativeOverlayPanelFact>,
    ) -> Self {
        Self {
            protocol: protocol.into(),
            session_id: session_id.into(),
            revision: revision.into(),
            sequence,
            observed_at_ms,
            panels,
        }
    }

    pub fn current(
        session_id: impl Into<String>,
        revision: impl Into<String>,
        sequence: u64,
        observed_at_ms: u64,
        panels: Vec<NativeOverlayPanelFact>,
    ) -> Self {
        Self::new(
            JET_DEVTOOLS_PROTOCOL,
            session_id,
            revision,
            sequence,
            observed_at_ms,
            panels,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.protocol != JET_DEVTOOLS_PROTOCOL {
            return Err(format!(
                "native overlay protocol must be `{JET_DEVTOOLS_PROTOCOL}`"
            ));
        }
        validate_text(&self.revision, "native overlay revision", false)?;
        if self.sequence == 0 {
            return Err("native overlay event sequence must be greater than zero".to_string());
        }
        if self.panels.len() > MAX_OVERLAY_PANELS {
            return Err("native overlay event exceeds its panel budget".to_string());
        }
        let mut panel_ids = BTreeSet::new();
        let mut count = 0;
        for panel in &self.panels {
            if !panel_ids.insert(panel.panel_id.clone()) {
                return Err(format!(
                    "native overlay event repeats panel `{}`",
                    panel.panel_id
                ));
            }
            panel.validate(&mut count)?;
        }
        Ok(())
    }
}

/// Physical viewport and integer DPI scale.  `scale_milli = 1000` is 1x;
/// using an integer keeps headless geometry reproducible across hosts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeOverlayViewport {
    pub width: u32,
    pub height: u32,
    pub scale_milli: u32,
}

impl NativeOverlayViewport {
    pub fn new(width: u32, height: u32, scale_milli: u32) -> Result<Self, String> {
        let viewport = Self {
            width,
            height,
            scale_milli,
        };
        viewport.validate()?;
        Ok(viewport)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.width == 0 || self.height == 0 {
            return Err("native overlay viewport dimensions must be non-zero".to_string());
        }
        if self.width > i32::MAX as u32 || self.height > i32::MAX as u32 {
            return Err("native overlay viewport exceeds coordinate range".to_string());
        }
        if self.scale_milli == 0 || self.scale_milli > 10_000 {
            return Err("native overlay scale must be between 1 and 10000 milli-units".to_string());
        }
        Ok(())
    }

    pub fn scale(&self) -> f64 {
        f64::from(self.scale_milli) / 1000.0
    }

    pub fn logical_width(&self) -> u32 {
        ((u64::from(self.width) * 1000) / u64::from(self.scale_milli))
            .max(1)
            .min(u64::from(self.width)) as u32
    }

    pub fn logical_height(&self) -> u32 {
        ((u64::from(self.height) * 1000) / u64::from(self.scale_milli))
            .max(1)
            .min(u64::from(self.height)) as u32
    }

    pub fn overlay_rect(&self, dock: NativeOverlayDock) -> NativeOverlayRect {
        let width = self.scaled_extent(480, self.width);
        let height = self.scaled_extent(360, self.height);
        match dock {
            NativeOverlayDock::Floating => NativeOverlayRect {
                x: ((self.width - width) / 2) as i32,
                y: ((self.height - height) / 2) as i32,
                width,
                height,
            },
            NativeOverlayDock::Left => NativeOverlayRect {
                x: 0,
                y: 0,
                width,
                height: self.height,
            },
            NativeOverlayDock::Right => NativeOverlayRect {
                x: (self.width - width) as i32,
                y: 0,
                width,
                height: self.height,
            },
            NativeOverlayDock::Top => NativeOverlayRect {
                x: 0,
                y: 0,
                width: self.width,
                height,
            },
            NativeOverlayDock::Bottom => NativeOverlayRect {
                x: 0,
                y: (self.height - height) as i32,
                width: self.width,
                height,
            },
        }
    }

    fn scaled_extent(&self, logical: u32, available: u32) -> u32 {
        (u64::from(logical)
            .saturating_mul(u64::from(self.scale_milli))
            .saturating_add(999)
            / 1000)
            .max(1)
            .min(u64::from(available)) as u32
    }
}

impl Default for NativeOverlayViewport {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            scale_milli: 1000,
        }
    }
}

/// A physical overlay hit rectangle.  It is geometry only; no backend is
/// selected here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeOverlayRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl NativeOverlayRect {
    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x
            && y >= self.y
            && (x as i64) < i64::from(self.x) + i64::from(self.width)
            && (y as i64) < i64::from(self.y) + i64::from(self.height)
    }
}

/// Docking choices are host layout state, not panel semantics.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum NativeOverlayDock {
    #[default]
    Floating,
    Left,
    Right,
    Top,
    Bottom,
}


/// Local open/focus state owned by the native host.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeOverlayState {
    pub open: bool,
    pub dock: NativeOverlayDock,
    pub focused: bool,
}

/// Input arriving from a GUI/game window.  The host returns a route rather than
/// invoking the game, so focused overlays cannot accidentally leak events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeOverlayInput {
    Toggle,
    Close,
    Dock(NativeOverlayDock),
    Focus(bool),
    Key { code: String, pressed: bool },
    Gamepad {
        gamepad: i64,
        control: String,
        pressed: bool,
    },
    Pointer { x: i32, y: i32, pressed: bool },
    Text(String),
    Activate { action_id: String },
}

/// Where the native host sends an input event after applying overlay policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeOverlayInputRoute {
    Overlay,
    Game,
}

/// One retained, bounded projection frame handed to a later `JetBackend`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeOverlayFrame {
    pub frame_id: u64,
    pub protocol: String,
    pub session_id: String,
    pub revision: String,
    pub sequence: u64,
    pub observed_at_ms: u64,
    pub open: bool,
    pub dock: NativeOverlayDock,
    pub focused: bool,
    pub viewport: NativeOverlayViewport,
    pub bounds: NativeOverlayRect,
    pub cursor: Option<u64>,
    pub selection: Option<JetDevtoolsSelection>,
    pub panels: Vec<NativeOverlayPanelFact>,
}

impl NativeOverlayFrame {
    pub fn panel_count(&self) -> usize {
        self.panels.len()
    }

    pub fn node_count(&self) -> usize {
        self.panels
            .iter()
            .map(|panel| panel.nodes.iter().map(node_count).sum::<usize>())
            .sum()
    }
}

fn node_count(node: &NativeOverlayNode) -> usize {
    1 + node.children.iter().map(node_count).sum::<usize>()
}
fn panel_node_count(panel: &NativeOverlayPanelFact) -> usize {
    panel.nodes.iter().map(node_count).sum()
}

/// Native overlay host state and bounded frame history.
pub struct NativeOverlayHost {
    state: NativeOverlayState,
    viewport: NativeOverlayViewport,
    toggle_key: String,
    release_build: bool,
    session_id: Option<String>,
    revision: Option<String>,
    sequence: Option<u64>,
    view: JetDevtoolsViewState,
    observed_at_ms: u64,
    panels: BTreeMap<String, NativeOverlayPanelFact>,
    next_frame_id: u64,
    max_frames: usize,
    frames: VecDeque<NativeOverlayFrame>,
    /// Last typed draw callback observed from the real game window.  Retaining
    /// one value keeps this boundary bounded while proving the callback is not
    /// a projection-only no-op.
    last_native_draw: Option<JetDevtoolsNativeDrawCommand>,
}

impl NativeOverlayHost {
    pub fn new() -> Self {
        Self::with_viewport(NativeOverlayViewport::default())
    }

    pub fn with_viewport(viewport: NativeOverlayViewport) -> Self {
        assert!(viewport.validate().is_ok(), "invalid native overlay viewport");
        Self {
            state: NativeOverlayState::default(),
            viewport,
            toggle_key: "F12".to_string(),
            release_build: false,
            session_id: None,
            revision: None,
            sequence: None,
            view: JetDevtoolsViewState::default(),
            observed_at_ms: 0,
            last_native_draw: None,
            panels: BTreeMap::new(),
            next_frame_id: 1,
            max_frames: MAX_OVERLAY_FRAMES,
            frames: VecDeque::new(),
        }
    }

    pub fn with_frame_capacity(mut self, capacity: usize) -> Self {
        self.max_frames = capacity.clamp(1, MAX_OVERLAY_FRAMES);
        self
    }
    /// Build a host with the development overlay explicitly excluded.
    pub fn with_release_build(mut self, release_build: bool) -> Self {
        self.set_release_build(release_build);
        self
    }

    pub fn release_build(&self) -> bool {
        self.release_build
    }

    /// Toggle the release exclusion policy and discard retained devtools data
    /// when entering release mode.
    pub fn set_release_build(&mut self, release_build: bool) {
        self.release_build = release_build;
        if release_build {
            self.state.open = false;
            self.state.focused = false;
            self.session_id = None;
            self.revision = None;
            self.sequence = None;
            self.view = JetDevtoolsViewState::default();
            self.observed_at_ms = 0;
            self.panels.clear();
            self.frames.clear();
            self.last_native_draw = None;
        }
    }

    pub fn panel_catalog(&self) -> &'static [JetDevtoolsPanelDescriptor] {
        crate::Devtools::catalog::descriptors()
    }
    pub fn panel_availability(&self) -> Vec<JetDevtoolsPanelAvailability> {
        let grants: &[JetDevtoolsPanelCapability] = if self.release_build {
            &[]
        } else {
            &JetDevtoolsPanelCapability::ALL
        };
        self.panel_availability_for(grants)
    }

    pub fn panel_availability_for(
        &self,
        grants: &[JetDevtoolsPanelCapability],
    ) -> Vec<JetDevtoolsPanelAvailability> {
        let grants = if self.release_build { &[] } else { grants };
        project_panel_availability(JetDevtoolsHostKind::NativeOverlay, grants)
    }


    pub fn state(&self) -> NativeOverlayState {
        self.state
    }

    pub fn viewport(&self) -> NativeOverlayViewport {
        self.viewport
    }

    pub fn toggle_key(&self) -> &str {
        &self.toggle_key
    }

    pub fn set_toggle_key(&mut self, key: impl Into<String>) -> Result<(), String> {
        let key = key.into();
        validate_text(&key, "native overlay toggle key", false)?;
        self.toggle_key = key;
        Ok(())
    }

    pub fn set_viewport(&mut self, viewport: NativeOverlayViewport) -> Result<(), String> {
        viewport.validate()?;
        self.viewport = viewport;
        self.push_frame();
        Ok(())
    }

    pub fn open(&mut self) {
        if self.release_build {
            return;
        }
        self.state.open = true;
        self.state.focused = true;
        self.push_frame();
    }

    pub fn close(&mut self) {
        self.state.open = false;
        self.state.focused = false;
    }
    pub fn toggle(&mut self) {
        if self.release_build {
            return;
        }
        if self.state.open {
            self.close();
        } else {
            self.open();
        }
    }

    pub fn set_dock(&mut self, dock: NativeOverlayDock) {
        self.state.dock = dock;
        self.push_frame();
    }

    pub fn set_focus(&mut self, focused: bool) {
        self.state.focused = self.state.open && focused;
        self.push_frame();
    }

    pub fn is_open(&self) -> bool {
        self.state.open
    }

    pub fn is_focused(&self) -> bool {
        self.state.open && self.state.focused
    }

    pub fn captures_input(&self) -> bool {
        self.is_focused()
    }

    /// Ingest one already-parsed canonical projection.  Sequence is strictly
    /// increasing; source revision and panel freshness are non-decreasing.
    pub fn ingest(&mut self, event: NativeOverlayEvent) -> Result<(), String> {
        if self.release_build {
            return Err("native overlay is excluded from release builds".to_string());
        }
        event.validate()?;
        if let Some(session_id) = self.session_id.as_deref() {
            if session_id != event.session_id {
                return Err("native overlay event belongs to a different session".to_string());
            }
        }
        if let Some(sequence) = self.sequence {
            if event.sequence <= sequence {
                if self.revision.as_deref() != Some(event.revision.as_str()) {
                    return Err("native overlay stale event revision identity".to_string());
                }
                return Err("native overlay event sequence is not increasing".to_string());
            }
        }
        let revision_changed = self
            .revision
            .as_deref()
            .is_some_and(|revision| revision != event.revision.as_str());
        let mut projected_panel_count = if revision_changed {
            0
        } else {
            self.panels.len()
        };
        let mut projected_node_count = if revision_changed {
            0
        } else {
            self.panels
                .values()
                .map(panel_node_count)
                .sum::<usize>()
        };
        for panel in &event.panels {
            if !revision_changed {
                if let Some(previous) = self.panels.get(&panel.panel_id) {
                    if panel.freshness < previous.freshness {
                        return Err(format!(
                            "native overlay panel `{}` freshness moved backwards",
                            panel.panel_id
                        ));
                    }
                    projected_node_count = projected_node_count
                        .saturating_sub(panel_node_count(previous))
                        .checked_add(panel_node_count(panel))
                        .ok_or_else(|| "native overlay node count overflowed".to_string())?;
                    continue;
                }
            }
            projected_panel_count = projected_panel_count
                .checked_add(1)
                .ok_or_else(|| "native overlay panel count overflowed".to_string())?;
            projected_node_count = projected_node_count
                .checked_add(panel_node_count(panel))
                .ok_or_else(|| "native overlay node count overflowed".to_string())?;
        }
        if projected_panel_count > MAX_OVERLAY_PANELS {
            return Err("native overlay frame exceeds its panel budget".to_string());
        }
        if projected_node_count > MAX_OVERLAY_NODES {
            return Err("native overlay frame exceeds its node budget".to_string());
        }
        if revision_changed {
            self.panels.clear();
        }

        let NativeOverlayEvent {
            protocol: _,
            session_id,
            revision,
            sequence,
            observed_at_ms,
            panels,
        } = event;
        self.session_id = Some(session_id);
        self.revision = Some(revision);
        self.sequence = Some(sequence);
        self.view.cursor = Some(sequence);
        self.observed_at_ms = observed_at_ms;
        for panel in panels {
            self.panels.insert(panel.panel_id.clone(), panel);
        }
        self.push_frame();
        Ok(())
    }

    /// Consume the adapter's canonical update without introducing a second
    /// event stream at the rendering boundary.
    pub fn ingest_update(
        &mut self,
        update: &crate::NativeOverlayAdapter::NativeOverlayUpdate,
    ) -> Result<(), String> {
        if self.release_build {
            return Err("native overlay is excluded from release builds".to_string());
        }
        self.view.cursor = update.identity.cursor.map(|cursor| cursor.sequence);
        self.view.selection = update
            .selection
            .as_ref()
            .map(|selection| {
                JetDevtoolsSelection::new(
                    &selection.panel_id,
                    (!selection.item_key.is_empty()).then(|| selection.item_key.clone()),
                )
            });
        if let Some(event) = update.event.clone() {
            self.ingest(event)?;
        } else if self.sequence.is_some() {
            self.push_frame();
        }
        Ok(())
    }

    pub fn frame(&self) -> Option<&NativeOverlayFrame> {
        self.frames.back()
    }

    pub fn frames(&self) -> impl Iterator<Item = &NativeOverlayFrame> {
        self.frames.iter()
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
    pub fn last_native_draw(&self) -> Option<&JetDevtoolsNativeDrawCommand> {
        self.last_native_draw.as_ref()
    }

    /// Retain one typed draw callback from the real game backend. The host
    /// keeps this bounded; overlay pixels are emitted by `native_draw_commands`.
    pub fn record_native_draw(&mut self, command: JetDevtoolsNativeDrawCommand) {
        if !self.release_build {
            self.last_native_draw = Some(command);
        }
    }

    /// Render the current typed projection through the canonical native draw
    /// command carrier. The raylib bridge executes these commands in its real
    /// `BeginDrawing`/`EndDrawing` window context.
    pub fn native_draw_commands(&self) -> Vec<JetDevtoolsNativeDrawCommand> {
        let Some(frame) = self.frame() else {
            return Vec::new();
        };
        if self.release_build || !frame.open {
            return Vec::new();
        }
        let bounds = frame.bounds;
        let bottom = i64::from(bounds.y) + i64::from(bounds.height);
        let mut commands = Vec::with_capacity(MAX_OVERLAY_NODES.min(64));
        commands.push(JetDevtoolsNativeDrawCommand::Rectangle {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width as i32,
            height: bounds.height as i32,
            color: JetDevtoolsNativeColor::new(16, 20, 28, 232),
        });
        commands.push(JetDevtoolsNativeDrawCommand::Text {
            text: format!("Jet Devtools [{}]", frame.session_id),
            x: bounds.x.saturating_add(12),
            y: bounds.y.saturating_add(22),
            size: 18,
            color: JetDevtoolsNativeColor::WHITE,
        });
        let mut y = i64::from(bounds.y).saturating_add(46);
        for panel in &frame.panels {
            if y >= bottom || commands.len() >= MAX_OVERLAY_NODES {
                break;
            }
            commands.push(JetDevtoolsNativeDrawCommand::Text {
                text: panel.title.clone(),
                x: bounds.x.saturating_add(12),
                y: y.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
                size: 15,
                color: JetDevtoolsNativeColor::new(255, 220, 120, 255),
            });
            y = y.saturating_add(18);
            if !panel.status.is_empty() && y < bottom {
                commands.push(JetDevtoolsNativeDrawCommand::Text {
                    text: panel.status.clone(),
                    x: bounds.x.saturating_add(18),
                    y: y.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
                    size: 12,
                    color: JetDevtoolsNativeColor::new(170, 190, 210, 255),
                });
                y = y.saturating_add(16);
            }
            for node in &panel.nodes {
                append_native_node_commands(
                    &mut commands,
                    node,
                    bounds.x.saturating_add(18),
                    &mut y,
                    bottom,
                    0,
                );
                if y >= bottom || commands.len() >= MAX_OVERLAY_NODES {
                    break;
                }
            }
        }
        commands
    }

    /// Route input without ever calling into the game.  A focused, open overlay
    /// captures every event, including pointer events outside its rectangle.
    pub fn route_input(&mut self, input: NativeOverlayInput) -> NativeOverlayInputRoute {
        match input {
            NativeOverlayInput::Toggle => {
                if self.release_build {
                    NativeOverlayInputRoute::Game
                } else {
                    self.toggle();
                    NativeOverlayInputRoute::Overlay
                }
            }
            NativeOverlayInput::Close => {
                if self.state.open {
                    self.close();
                    NativeOverlayInputRoute::Overlay
                } else {
                    NativeOverlayInputRoute::Game
                }
            }
            NativeOverlayInput::Dock(dock) => {
                if self.state.open {
                    self.set_dock(dock);
                    NativeOverlayInputRoute::Overlay
                } else {
                    NativeOverlayInputRoute::Game
                }
            }
            NativeOverlayInput::Focus(focused) => {
                if self.state.open {
                    self.set_focus(focused);
                    NativeOverlayInputRoute::Overlay
                } else {
                    NativeOverlayInputRoute::Game
                }
            }
            NativeOverlayInput::Key { code, pressed } => {
                if pressed && code == self.toggle_key {
                    if self.release_build {
                        NativeOverlayInputRoute::Game
                    } else {
                        self.toggle();
                        NativeOverlayInputRoute::Overlay
                    }
                } else if self.captures_input() {
                    NativeOverlayInputRoute::Overlay
                } else {
                    NativeOverlayInputRoute::Game
                }
            }
            NativeOverlayInput::Gamepad { .. } => {
                if self.captures_input() {
                    NativeOverlayInputRoute::Overlay
                } else {
                    NativeOverlayInputRoute::Game
                }
            }
            NativeOverlayInput::Pointer { x, y, .. } => {
                if self.captures_input()
                    || (self.state.open && self.viewport.overlay_rect(self.state.dock).contains(x, y))
                {
                    NativeOverlayInputRoute::Overlay
                } else {
                    NativeOverlayInputRoute::Game
                }
            }
            NativeOverlayInput::Text(_) | NativeOverlayInput::Activate { .. } => {
                if self.captures_input() {
                    NativeOverlayInputRoute::Overlay
                } else {
                    NativeOverlayInputRoute::Game
                }
            }
        }
    }

    /// Produce a deterministic headless frame without selecting a graphics
    /// backend.  Two calls with equal typed facts produce equal frames.
    pub fn headless_projection(event: &NativeOverlayEvent) -> Result<NativeOverlayFrame, String> {
        let mut host = Self::new();
        host.open();
        host.ingest(event.clone())?;
        host.frame()
            .cloned()
            .ok_or_else(|| "native overlay did not produce a headless frame".to_string())
    }

    fn push_frame(&mut self) {
        if self.release_build {
            return;
        }
        let Some(session_id) = self.session_id.as_deref() else {
            return;
        };
        let Some(revision) = self.revision.as_deref() else {
            return;
        };
        let Some(sequence) = self.sequence else {
            return;
        };
        let frame_id = self.next_frame_id;
        self.next_frame_id = self.next_frame_id.saturating_add(1).max(1);
        let frame = NativeOverlayFrame {
            frame_id,
            protocol: JET_DEVTOOLS_PROTOCOL.to_string(),
            session_id: session_id.to_string(),
            revision: revision.to_string(),
            sequence,
            observed_at_ms: self.observed_at_ms,
            open: self.state.open,
            dock: self.state.dock,
            focused: self.is_focused(),
            viewport: self.viewport,
            bounds: self.viewport.overlay_rect(self.state.dock),
            cursor: self.view.cursor,
            selection: self.view.selection.clone(),
            panels: self.panels.values().cloned().collect(),
        };
        if self.frames.len() == self.max_frames {
            self.frames.pop_front();
        }
        self.frames.push_back(frame);
    }
}

impl Default for NativeOverlayHost {
    fn default() -> Self {
        Self::new()
    }
}

fn append_native_node_commands(
    commands: &mut Vec<JetDevtoolsNativeDrawCommand>,
    node: &NativeOverlayNode,
    x: i32,
    y: &mut i64,
    bottom: i64,
    depth: usize,
) {
    if *y >= bottom || commands.len() >= MAX_OVERLAY_NODES {
        return;
    }
    let indent = depth.saturating_mul(12).min(i32::MAX as usize) as i32;
    let mut text = node.label.clone();
    if let Some(value) = node.value.as_deref() {
        text.push_str(": ");
        text.push_str(value);
    }
    if let Some(status) = node.status.as_deref() {
        text.push_str(" [");
        text.push_str(status);
        text.push(']');
    }
    commands.push(JetDevtoolsNativeDrawCommand::Text {
        text,
        x: x.saturating_add(indent),
        y: (*y).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        size: 12,
        color: JetDevtoolsNativeColor::new(220, 230, 240, 255),
    });
    *y = (*y).saturating_add(16);
    for child in &node.children {
        append_native_node_commands(commands, child, x, y, bottom, depth.saturating_add(1));
        if *y >= bottom || commands.len() >= MAX_OVERLAY_NODES {
            break;
        }
    }
}

/// Foundation callback adapter installed by `jet dev`. It joins one resident
/// session to the real window hooks without importing `jet-devserver` into the
/// generated Prelude or inventing another event stream.
pub struct NativeOverlayHostBridge {
    session: Arc<ResidentDevSession>,
    adapter: Mutex<NativeOverlayAdapter>,
    host: Mutex<NativeOverlayHost>,
    toggle_key_state: Mutex<BTreeMap<String, bool>>,
}

impl NativeOverlayHostBridge {
    pub fn new(session: Arc<ResidentDevSession>, release_build: bool) -> Self {
        let adapter = NativeOverlayAdapter::for_session(&session).with_release_build(release_build);
        let host = NativeOverlayHost::new().with_release_build(release_build);
        Self {
            session,
            adapter: Mutex::new(adapter),
            host: Mutex::new(host),
            toggle_key_state: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn frame(&self) -> Option<NativeOverlayFrame> {
        self.host.lock().ok()?.frame().cloned()
    }
}

impl JetDevtoolsNativeHost for NativeOverlayHostBridge {
    fn on_event(&self, session_id: &str, _event: JetDevtoolsEvent) {
        if session_id != self.session.id() {
            return;
        }
        let Ok(mut adapter) = self.adapter.lock() else {
            return;
        };
        let Ok(update) = adapter.sync_session(&self.session) else {
            return;
        };
        if let Ok(mut host) = self.host.lock() {
            let _ = host.ingest_update(&update);
        }
    }

    fn on_window_open(&self, window: JetDevtoolsNativeWindow) {
        if window.session_id != self.session.id() {
            return;
        }
        let Ok(viewport) = NativeOverlayViewport::new(window.width, window.height, 1000) else {
            return;
        };
        if let Ok(mut host) = self.host.lock() {
            let _ = host.set_viewport(viewport);
        }
    }

    fn on_window_close(&self, session_id: &str) {
        if session_id != self.session.id() {
            return;
        }
        if let Ok(mut host) = self.host.lock() {
            host.close();
        }
    }

    fn on_input(&self, session_id: &str, input: JetDevtoolsNativeInput) -> bool {
        if session_id != self.session.id() {
            return false;
        }
        let Ok(mut host) = self.host.lock() else {
            return false;
        };
        let input = match input {
            JetDevtoolsNativeInput::Key { code, pressed } => {
                let edge_pressed = if code == host.toggle_key() {
                    let Ok(mut states) = self.toggle_key_state.lock() else {
                        return false;
                    };
                    let was_pressed = states.insert(code.clone(), pressed).unwrap_or(false);
                    pressed && !was_pressed
                } else {
                    pressed
                };
                NativeOverlayInput::Key {
                    code,
                    pressed: edge_pressed,
                }
            }
            JetDevtoolsNativeInput::Gamepad {
                gamepad,
                control,
                pressed,
            } => NativeOverlayInput::Gamepad {
                gamepad,
                control,
                pressed,
            },
        };
        matches!(host.route_input(input), NativeOverlayInputRoute::Overlay)
    }

    fn on_frame_begin(&self, frame: JetDevtoolsNativeFrame) -> Vec<JetDevtoolsNativeDrawCommand> {
        if frame.session_id != self.session.id() {
            return Vec::new();
        }
        let Ok(viewport) = NativeOverlayViewport::new(frame.width, frame.height, 1000) else {
            return Vec::new();
        };
        let Ok(mut host) = self.host.lock() else {
            return Vec::new();
        };
        if host.viewport() != viewport {
            let _ = host.set_viewport(viewport);
        }
        host.native_draw_commands()
    }

    fn on_draw(&self, session_id: &str, command: JetDevtoolsNativeDrawCommand) {
        if session_id != self.session.id() {
            return;
        }
        if let Ok(mut host) = self.host.lock() {
            host.record_native_draw(command);
        }
    }

    fn on_frame_end(&self, session_id: &str) {
        if session_id != self.session.id() {
            return;
        }
        let _ = self.frame();
    }
}

fn validate_text(value: &str, label: &str, allow_empty: bool) -> Result<(), String> {
    if (!allow_empty && value.is_empty())
        || value.len() > MAX_OVERLAY_TEXT_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(format!("{label} is empty, too long, or contains control text"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(id: &str, freshness: u64) -> NativeOverlayPanelFact {
        NativeOverlayPanelFact::new(
            id,
            id,
            freshness,
            "ready",
            vec![NativeOverlayNode::text(format!("{id}:item"), "item")],
        )
    }

    #[test]
    fn projection_keeps_panel_identity_and_freshness() {
        let mut host = NativeOverlayHost::new();
        host.ingest(NativeOverlayEvent::current(
            "session",
            "revision",
            1,
            10,
            vec![panel("Build", 4)],
        ))
        .unwrap();
        let first = host.frame().unwrap().clone();
        host.ingest(NativeOverlayEvent::current(
            "session",
            "revision",
            2,
            11,
            vec![panel("Build", 5)],
        ))
        .unwrap();
        let second = host.frame().unwrap();
        assert_eq!(first.panels[0].panel_id, second.panels[0].panel_id);
        assert_eq!(second.panels[0].freshness, 5);
    }

    #[test]
    fn stale_revision_identity_is_rejected_at_equal_sequence() {
        let mut host = NativeOverlayHost::new();
        host.ingest(NativeOverlayEvent::current(
            "session",
            "revision-a",
            1,
            10,
            vec![panel("Build", 1)],
        ))
        .unwrap();
        let error = host
            .ingest(NativeOverlayEvent::current(
                "session",
                "revision-b",
                1,
                11,
                vec![panel("Build", 2)],
            ))
            .unwrap_err();
        assert!(error.contains("revision identity"));
    }

    #[test]
    fn revision_rollover_discards_old_panels() {
        let mut host = NativeOverlayHost::new();
        host.ingest(NativeOverlayEvent::current(
            "session",
            "revision-a",
            1,
            10,
            vec![panel("Old", 1)],
        ))
        .unwrap();
        host.ingest(NativeOverlayEvent::current(
            "session",
            "revision-b",
            2,
            11,
            vec![panel("New", 1)],
        ))
        .unwrap();
        let frame = host.frame().unwrap();
        assert_eq!(frame.panels.len(), 1);
        assert_eq!(frame.panels[0].panel_id, "New");
    }

    #[test]
    fn focused_overlay_never_routes_input_to_game() {
        let mut host = NativeOverlayHost::new();
        host.open();
        for input in [
            NativeOverlayInput::Key {
                code: "Space".to_string(),
                pressed: true,
            },
            NativeOverlayInput::Pointer {
                x: -100,
                y: -100,
                pressed: true,
            },
            NativeOverlayInput::Text("typed".to_string()),
            NativeOverlayInput::Activate {
                action_id: "inspect".to_string(),
            },
        ] {
            assert_eq!(host.route_input(input), NativeOverlayInputRoute::Overlay);
        }
        host.close();
        assert_eq!(
            host.route_input(NativeOverlayInput::Key {
                code: "Space".to_string(),
                pressed: true,
            }),
            NativeOverlayInputRoute::Game
        );
    }

    #[test]
    fn headless_projection_is_deterministic() {
        let event = NativeOverlayEvent::current(
            "session",
            "revision",
            7,
            99,
            vec![panel("Traces", 3), panel("Build", 3)],
        );
        assert_eq!(
            NativeOverlayHost::headless_projection(&event).unwrap(),
            NativeOverlayHost::headless_projection(&event).unwrap()
        );
    }

    #[test]
    fn merged_projection_stays_bounded() {
        let mut host = NativeOverlayHost::new();
        let nodes = (0..MAX_OVERLAY_NODES)
            .map(|index| NativeOverlayNode::text(format!("node-{index}"), "node"))
            .collect();
        host.ingest(NativeOverlayEvent::current(
            "session",
            "revision",
            1,
            1,
            vec![NativeOverlayPanelFact::new(
                "first",
                "first",
                1,
                "ready",
                nodes,
            )],
        ))
        .unwrap();
        let error = host
            .ingest(NativeOverlayEvent::current(
                "session",
                "revision",
                2,
                2,
                vec![panel("second", 1)],
            ))
            .unwrap_err();
        assert!(error.contains("node budget"));
        assert_eq!(host.frame().unwrap().sequence, 1);
    }

    #[test]
    fn frame_history_is_bounded() {
        let mut host = NativeOverlayHost::new().with_frame_capacity(2);
        for sequence in 1..=4 {
            host.ingest(NativeOverlayEvent::current(
                "session",
                "revision",
                sequence,
                sequence,
                vec![panel("Build", sequence)],
            ))
            .unwrap();
        }
        assert_eq!(host.frame_count(), 2);
        assert_eq!(host.frame().unwrap().sequence, 4);
    }
}
