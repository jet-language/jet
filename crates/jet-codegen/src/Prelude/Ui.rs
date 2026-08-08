// D-RENDERTGT1=A + D-RENDERTGT2=A (c133 M1): render-target backend trait seam.
// Arc/Mutex (not Rc/RefCell): `jet_ui_reactive_render` requires Send+Sync
// closures that capture backends — fully-qualified paths avoid clashing with
// the AOT prelude's existing `use std::sync::{Arc, Mutex, …}`.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JetPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JetSize {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JetRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JetSizeConstraint {
    pub min_width: f64,
    pub min_height: f64,
    pub max_width: f64,
    pub max_height: f64,
}

/// D-A11YGATE1=B (c134 Phase 6): the accessible-role vocabulary. A small,
/// real ARIA-style role set — mirrors the four Phase-4 starter components
/// (Button, Input, Label, Container) rather than the full ARIA taxonomy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JetAriaRole {
    Button,
    TextInput,
    Label,
    Container,
}

impl JetAriaRole {
    /// Interactive roles are keyboard-focusable and need a real accessible
    /// label (E2930); `Label`/`Container` are structural/static, never
    /// focused.
    pub fn is_interactive(&self) -> bool {
        matches!(self, JetAriaRole::Button | JetAriaRole::TextInput)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetUiNodeKind {
    Custom,
    Text,
    Box,
    Button,
    TextInput,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetUiNode {
    /// Also the accessible name (WAI-ARIA accname model — one field serves
    /// both display and accessibility; no separate name field).
    pub label: String,
    pub width: f64,
    pub height: f64,
    /// D-A11YGATE1=B: `None` = decorative/non-interactive node.
    pub role: Option<JetAriaRole>,
    /// D-STYLESHAPE1=A (c134 Phase 3/7 wiring): explicit fill color as a
    /// `#RRGGBB` string, matching `JetPaintCmd::FillRect`'s existing color
    /// representation. `None` falls back to the default fill (`#000000`).
    pub color: Option<String>,
    /// D-UITREE1=A: every renderer consumes this same typed node kind/tree.
    pub kind: JetUiNodeKind,
    pub children: Vec<JetUiNode>,
    /// D-UI-NODE-ID1=C: optional author key; when set, identity is the key
    /// instead of the render path.
    pub key: Option<String>,
    /// D-UI-EVT-DISP1=E / D-WEB-CLICK-PORT1=D: handler slot id for portable
    /// `on_click`. `None` means no click handler. Slots live in
    /// `jet_ui_click_slots`; identity→slot binding happens at paint/mount.
    pub on_click: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetInputEvent {
    Key { code: String },
    Resize { size: JetSize },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JetEventResult {
    Handled,
    Ignored,
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetPaintCmd {
    FillRect { rect: JetRect, color: String },
    Text { rect: JetRect, text: String },
}

struct JetNullBackendState {
    measured: Option<JetSize>,
    layout_frame: Option<JetRect>,
    commands: Vec<JetPaintCmd>,
    last_event: Option<JetEventResult>,
    // D-A11YGATE1=B (c134 Phase 6): keyboard focus routing over a flat list
    // of interactive nodes. `focused_index` is `Some` whenever `focus_nodes`
    // is non-empty (registration always focuses the first node).
    focus_nodes: Vec<JetUiNode>,
    focused_index: Option<usize>,
}

/// D-A11YGATE1=B: advance focus on a `Tab` key. Returns `Some(Handled)` when
/// the key was consumed by focus routing; `None` means "not a focus event,
/// fall through to normal key handling".
fn jet_ui_advance_focus(
    focus_nodes: &[JetUiNode],
    focused_index: &mut Option<usize>,
    code: &str,
) -> Option<JetEventResult> {
    if code != "Tab" || focus_nodes.is_empty() {
        return None;
    }
    let next = match *focused_index {
        Some(i) => (i + 1) % focus_nodes.len(),
        None => 0,
    };
    *focused_index = Some(next);
    Some(JetEventResult::Handled)
}

/// Card #1658 (corpus say-it-once): the default mount viewport (classic
/// 80x24 terminal), named once so no host hand-types the literal. Every
/// `mount_node_default` — Rust and DomRuntime.js — renders from this pair.
pub const DEFAULT_MOUNT_COLS: f64 = 80.0;
pub const DEFAULT_MOUNT_ROWS: f64 = 24.0;

/// D-RENDERTGT2=A: portable backend seam between Jet UI and platform renderers.
pub trait JetBackend {
    fn measure(&mut self, node: &JetUiNode, constraint: JetSizeConstraint) -> JetSize;
    fn layout(&mut self, node: &JetUiNode, frame: JetRect);
    fn paint(&mut self, node: &JetUiNode);
    fn on_event(&mut self, event: JetInputEvent) -> JetEventResult;
}

fn jet_ui_measure_tree(node: &JetUiNode, constraint: JetSizeConstraint) -> JetSize {
    let natural = if node.kind == JetUiNodeKind::Box {
        JetSize {
            width: node
                .children
                .iter()
                .map(|child| child.width)
                .fold(0.0_f64, f64::max),
            height: node.children.iter().map(|child| child.height).sum(),
        }
    } else {
        JetSize {
            width: node.width,
            height: node.height,
        }
    };
    JetSize {
        width: natural
            .width
            .clamp(constraint.min_width, constraint.max_width),
        height: natural
            .height
            .clamp(constraint.min_height, constraint.max_height),
    }
}

fn jet_ui_collect_focus(node: &JetUiNode, out: &mut Vec<JetUiNode>) {
    if node.role.as_ref().is_some_and(JetAriaRole::is_interactive) {
        out.push(node.clone());
    }
    for child in &node.children {
        jet_ui_collect_focus(child, out);
    }
}

fn jet_ui_visit_tree(node: &JetUiNode, frame: JetRect, visit: &mut dyn FnMut(&JetUiNode, JetRect)) {
    if node.kind == JetUiNodeKind::Box {
        let mut y = frame.y;
        for child in &node.children {
            let child_frame = JetRect {
                x: frame.x,
                y,
                width: frame.width,
                height: child.height,
            };
            jet_ui_visit_tree(child, child_frame, visit);
            y += child.height;
        }
    } else {
        visit(node, frame);
    }
}

fn jet_ui_paint_tree(node: &JetUiNode, frame: JetRect, commands: &mut Vec<JetPaintCmd>) {
    jet_ui_visit_tree(node, frame, &mut |leaf, leaf_frame| {
        if leaf.kind != JetUiNodeKind::Text {
            commands.push(JetPaintCmd::FillRect {
                rect: leaf_frame,
                color: leaf
                    .color
                    .clone()
                    .unwrap_or_else(|| "#000000".to_string()),
            });
        }
        commands.push(JetPaintCmd::Text {
            rect: leaf_frame,
            text: leaf.label.clone(),
        });
    });
}

#[derive(Clone)]
pub struct JetNullBackend {
    state: std::sync::Arc<std::sync::Mutex<JetNullBackendState>>,
}

impl JetNullBackend {
    pub fn new() -> Self {
        JetNullBackend {
            state: std::sync::Arc::new(std::sync::Mutex::new(JetNullBackendState {
                measured: None,
                layout_frame: None,
                commands: Vec::new(),
                last_event: None,
                focus_nodes: Vec::new(),
                focused_index: None,
            })),
        }
    }

    /// D-A11YGATE1=B: register the interactive focus order. Always focuses
    /// the first node when the list is non-empty.
    pub fn set_focus_group(&self, nodes: Vec<JetUiNode>) {
        let mut state = self.state.lock().unwrap();
        state.focused_index = if nodes.is_empty() { None } else { Some(0) };
        state.focus_nodes = nodes;
    }

    /// D-A11YGATE1=B: the accessible label of the currently focused node, or
    /// `""` when nothing is focused.
    pub fn focused_label(&self) -> String {
        let state = self.state.lock().unwrap();
        state
            .focused_index
            .and_then(|i| state.focus_nodes.get(i))
            .map(|n| n.label.clone())
            .unwrap_or_default()
    }

    pub fn measure_node(
        &self,
        node: JetUiNode,
        constraint: JetSizeConstraint,
    ) -> JetSize {
        let mut state = self.state.lock().unwrap();
        JetBackend::measure(&mut *state, &node, constraint)
    }

    pub fn layout_node(&self, node: JetUiNode, frame: JetRect) {
        let mut state = self.state.lock().unwrap();
        JetBackend::layout(&mut *state, &node, frame);
    }

    pub fn paint_node(&self, node: JetUiNode) {
        let mut state = self.state.lock().unwrap();
        JetBackend::paint(&mut *state, &node);
    }

    /// D-UI-MOUNT1=A: measure → layout → paint in one call.
    pub fn mount_node(&self, node: JetUiNode, constraint: JetSizeConstraint) {
        {
            let mut state = self.state.lock().unwrap();
            state.commands.clear();
        }
        let size = self.measure_node(node.clone(), constraint);
        self.layout_node(
            node.clone(),
            jet_ui_rect(0.0, 0.0, size.width, size.height),
        );
        self.paint_node(node);
    }

    /// Default viewport for the two-arg beginner mount (`backend.mount(tree)`).
    pub fn mount_node_default(&self, node: JetUiNode) {
        self.mount_node(
            node,
            jet_ui_constraint(0.0, 0.0, DEFAULT_MOUNT_COLS, DEFAULT_MOUNT_ROWS),
        );
    }

    pub fn dispatch_event(&self, event: JetInputEvent) -> JetEventResult {
        let mut state = self.state.lock().unwrap();
        JetBackend::on_event(&mut *state, event)
    }

    pub fn paint_commands(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .commands
            .iter()
            .map(|cmd| match cmd {
                JetPaintCmd::FillRect { rect, color } => {
                    format!("fill({}, {})", rect.jet_show(), color)
                }
                JetPaintCmd::Text { rect, text } => {
                    format!("text({}, {})", rect.jet_show(), text)
                }
            })
            .collect()
    }
}

impl Default for JetNullBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl JetBackend for JetNullBackendState {
    fn measure(&mut self, node: &JetUiNode, constraint: JetSizeConstraint) -> JetSize {
        let size = jet_ui_measure_tree(node, constraint);
        self.measured = Some(size);
        size
    }

    fn layout(&mut self, node: &JetUiNode, frame: JetRect) {
        let _ = node;
        self.layout_frame = Some(frame);
    }

    fn paint(&mut self, node: &JetUiNode) {
        let frame = self.layout_frame.unwrap_or(JetRect {
            x: 0.0,
            y: 0.0,
            width: node.width,
            height: node.height,
        });
        jet_ui_paint_tree(node, frame, &mut self.commands);
        // D-UI-EVT-DISP1=E: bind portable on_click slots by identity. Headless
        // / TUI backends that cannot produce clicks still register (D-WEB-CLICK-PORT1=D
        // no-op until an event arrives).
        jet_ui_bind_tree_clicks(node, "null#0");
        let mut focus = Vec::new();
        jet_ui_collect_focus(node, &mut focus);
        if !focus.is_empty() {
            self.focused_index = Some(0);
            self.focus_nodes = focus;
        }
    }

    fn on_event(&mut self, event: JetInputEvent) -> JetEventResult {
        if let JetInputEvent::Key { code } = &event {
            if let Some(result) =
                jet_ui_advance_focus(&self.focus_nodes, &mut self.focused_index, code)
            {
                self.last_event = Some(result);
                return result;
            }
        }
        let result = match &event {
            JetInputEvent::Key { code } if code.is_empty() => JetEventResult::Ignored,
            JetInputEvent::Resize { size } if size.width <= 0.0 || size.height <= 0.0 => {
                JetEventResult::Ignored
            }
            _ => JetEventResult::Handled,
        };
        self.last_event = Some(result);
        result
    }
}

// ── D-RENDERTGT2=A (c133 M2): TUI backend — deterministic character-grid output ──

struct JetTuiBackendState {
    measured: Option<JetSize>,
    layout_frame: Option<JetRect>,
    grid: Vec<Vec<char>>,
    grid_cols: usize,
    grid_rows: usize,
    render_count: usize,
    last_event: Option<JetEventResult>,
    // D-A11YGATE1=B (c134 Phase 6): see `JetNullBackendState`.
    focus_nodes: Vec<JetUiNode>,
    focused_index: Option<usize>,
}

fn tui_grid_dims(frame: &JetRect) -> (usize, usize) {
    let cols = frame.width.max(1.0).floor() as usize;
    let rows = frame.height.max(1.0).floor() as usize;
    (cols, rows)
}

fn tui_blank_grid(cols: usize, rows: usize) -> Vec<Vec<char>> {
    (0..rows).map(|_| vec![' '; cols]).collect()
}

fn tui_write_label(grid: &mut [Vec<char>], frame: &JetRect, label: &str) {
    let (cols, rows) = (grid.first().map(|r| r.len()).unwrap_or(0), grid.len());
    let start_col = frame.x.max(0.0) as usize;
    let start_row = frame.y.max(0.0) as usize;
    for (i, ch) in label.chars().enumerate() {
        let col = start_col + i;
        if col < cols && start_row < rows {
            grid[start_row][col] = ch;
        }
    }
}

#[derive(Clone)]
pub struct JetTuiBackend {
    state: std::sync::Arc<std::sync::Mutex<JetTuiBackendState>>,
}

impl JetTuiBackend {
    pub fn new() -> Self {
        JetTuiBackend {
            state: std::sync::Arc::new(std::sync::Mutex::new(JetTuiBackendState {
                measured: None,
                layout_frame: None,
                grid: Vec::new(),
                grid_cols: 0,
                grid_rows: 0,
                render_count: 0,
                last_event: None,
                focus_nodes: Vec::new(),
                focused_index: None,
            })),
        }
    }

    /// D-A11YGATE1=B: register the interactive focus order. Always focuses
    /// the first node when the list is non-empty.
    pub fn set_focus_group(&self, nodes: Vec<JetUiNode>) {
        let mut state = self.state.lock().unwrap();
        state.focused_index = if nodes.is_empty() { None } else { Some(0) };
        state.focus_nodes = nodes;
    }

    /// D-A11YGATE1=B: the accessible label of the currently focused node, or
    /// `""` when nothing is focused.
    pub fn focused_label(&self) -> String {
        let state = self.state.lock().unwrap();
        state
            .focused_index
            .and_then(|i| state.focus_nodes.get(i))
            .map(|n| n.label.clone())
            .unwrap_or_default()
    }

    pub fn measure_node(
        &self,
        node: JetUiNode,
        constraint: JetSizeConstraint,
    ) -> JetSize {
        let mut state = self.state.lock().unwrap();
        JetBackend::measure(&mut *state, &node, constraint)
    }

    pub fn layout_node(&self, node: JetUiNode, frame: JetRect) {
        let mut state = self.state.lock().unwrap();
        JetBackend::layout(&mut *state, &node, frame);
    }

    pub fn paint_node(&self, node: JetUiNode) {
        let mut state = self.state.lock().unwrap();
        JetBackend::paint(&mut *state, &node);
    }

    /// D-UI-MOUNT1=A: measure → layout → paint in one call.
    pub fn mount_node(&self, node: JetUiNode, constraint: JetSizeConstraint) {
        let size = self.measure_node(node.clone(), constraint);
        self.layout_node(
            node.clone(),
            jet_ui_rect(0.0, 0.0, size.width, size.height),
        );
        self.paint_node(node);
    }

    pub fn mount_node_default(&self, node: JetUiNode) {
        self.mount_node(
            node,
            jet_ui_constraint(0.0, 0.0, DEFAULT_MOUNT_COLS, DEFAULT_MOUNT_ROWS),
        );
    }

    pub fn dispatch_event(&self, event: JetInputEvent) -> JetEventResult {
        let mut state = self.state.lock().unwrap();
        JetBackend::on_event(&mut *state, event)
    }

    pub fn frame_lines(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .grid
            .iter()
            .map(|row| row.iter().collect::<String>().trim_end().to_string())
            .collect()
    }

    pub fn render_count(&self) -> i64 {
        self.state.lock().unwrap().render_count as i64
    }
}

impl Default for JetTuiBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl JetBackend for JetTuiBackendState {
    fn measure(&mut self, node: &JetUiNode, constraint: JetSizeConstraint) -> JetSize {
        let size = jet_ui_measure_tree(node, constraint);
        self.measured = Some(size);
        size
    }

    fn layout(&mut self, node: &JetUiNode, frame: JetRect) {
        let _ = node;
        self.layout_frame = Some(frame);
        let (cols, rows) = tui_grid_dims(&frame);
        self.grid_cols = cols;
        self.grid_rows = rows;
        self.grid = tui_blank_grid(cols, rows);
    }

    fn paint(&mut self, node: &JetUiNode) {
        self.render_count += 1;
        let frame = self.layout_frame.unwrap_or(JetRect {
            x: 0.0,
            y: 0.0,
            width: node.width,
            height: node.height,
        });
        if self.grid.is_empty() {
            let (cols, rows) = tui_grid_dims(&frame);
            self.grid_cols = cols;
            self.grid_rows = rows;
            self.grid = tui_blank_grid(cols, rows);
        }
        for row in self.grid.iter_mut() {
            for cell in row.iter_mut() {
                *cell = ' ';
            }
        }
        jet_ui_visit_tree(node, frame, &mut |leaf, leaf_frame| {
            tui_write_label(&mut self.grid, &leaf_frame, &leaf.label);
        });
        jet_ui_bind_tree_clicks(node, "tui#0");
        let mut focus = Vec::new();
        jet_ui_collect_focus(node, &mut focus);
        if !focus.is_empty() {
            self.focused_index = Some(0);
            self.focus_nodes = focus;
        }
    }

    fn on_event(&mut self, event: JetInputEvent) -> JetEventResult {
        if let JetInputEvent::Key { code } = &event {
            if let Some(result) =
                jet_ui_advance_focus(&self.focus_nodes, &mut self.focused_index, code)
            {
                self.last_event = Some(result);
                return result;
            }
        }
        let result = match &event {
            JetInputEvent::Key { code } if code.is_empty() => JetEventResult::Ignored,
            JetInputEvent::Resize { size } if size.width <= 0.0 || size.height <= 0.0 => {
                JetEventResult::Ignored
            }
            JetInputEvent::Resize { size } => {
                let frame = JetRect {
                    x: 0.0,
                    y: 0.0,
                    width: size.width,
                    height: size.height,
                };
                let (cols, rows) = tui_grid_dims(&frame);
                self.layout_frame = Some(frame);
                self.grid_cols = cols;
                self.grid_rows = rows;
                self.grid = tui_blank_grid(cols, rows);
                JetEventResult::Handled
            }
            _ => JetEventResult::Handled,
        };
        self.last_event = Some(result);
        result
    }
}

/// D-RENDERTGT2=A (c133 M2): reactive UI render loop — re-runs the body when signals change.
pub fn jet_ui_reactive_render<F: Fn() + Send + Sync + 'static>(body: F) {
    jet_std::jet_reactive_effect_rooted(body);
}

pub fn jet_ui_null() -> JetNullBackend {
    JetNullBackend::new()
}

pub fn jet_ui_tui() -> JetTuiBackend {
    JetTuiBackend::new()
}

pub fn jet_ui_point(x: f64, y: f64) -> JetPoint {
    JetPoint { x, y }
}

pub fn jet_ui_size(width: f64, height: f64) -> JetSize {
    JetSize { width, height }
}

pub fn jet_ui_rect(x: f64, y: f64, width: f64, height: f64) -> JetRect {
    JetRect {
        x,
        y,
        width,
        height,
    }
}

pub fn jet_ui_constraint(
    min_width: f64,
    min_height: f64,
    max_width: f64,
    max_height: f64,
) -> JetSizeConstraint {
    JetSizeConstraint {
        min_width,
        min_height,
        max_width,
        max_height,
    }
}

pub fn jet_ui_node(label: &str, width: f64, height: f64) -> JetUiNode {
    JetUiNode {
        label: label.to_string(),
        width,
        height,
        role: None,
        color: None,
        kind: JetUiNodeKind::Custom,
        children: Vec::new(),
        key: None,
        on_click: None,
    }
}

/// D-A11YGATE1=B (c134 Phase 6): construct a `UiNode` with an explicit
/// accessible role — the entry point for interactive controls that
/// `jet lint --a11y` checks (E2930 unlabeled control, E2931 duplicate label).
pub fn jet_ui_node_role(label: &str, width: f64, height: f64, role: JetAriaRole) -> JetUiNode {
    JetUiNode {
        label: label.to_string(),
        width,
        height,
        role: Some(role),
        color: None,
        kind: if role == JetAriaRole::Button {
            JetUiNodeKind::Button
        } else if role == JetAriaRole::TextInput {
            JetUiNodeKind::TextInput
        } else {
            JetUiNodeKind::Custom
        },
        children: Vec::new(),
        key: None,
        on_click: None,
    }
}

/// D-STYLESHAPE1=A (c134 Phase 3/7 wiring): construct a `UiNode` with an
/// explicit fill color — makes the typed `Style`/`Color` built in Phase 3
/// actually reach the paint pipeline instead of the hardcoded `#000000`.
pub fn jet_ui_node_color(label: &str, width: f64, height: f64, color: &str) -> JetUiNode {
    JetUiNode {
        label: label.to_string(),
        width,
        height,
        // A styled node still presents text. Keep that semantic in the
        // canonical tree so every backend exposes the same accessible name.
        role: Some(JetAriaRole::Label),
        color: Some(color.to_string()),
        kind: JetUiNodeKind::Custom,
        children: Vec::new(),
        key: None,
        on_click: None,
    }
}

/// D-UITREE1=A: canonical typed beginner constructors. Component-kit source,
/// native, web, and TUI all hand this exact tree to `JetBackend`.
pub fn jet_ui_text(text: &str) -> JetUiNode {
    JetUiNode {
        label: text.to_string(),
        width: text.chars().count() as f64,
        height: 1.0,
        role: Some(JetAriaRole::Label),
        color: None,
        kind: JetUiNodeKind::Text,
        children: Vec::new(),
        key: None,
        on_click: None,
    }
}

pub fn jet_ui_button(label: &str) -> JetUiNode {
    JetUiNode {
        label: label.to_string(),
        width: label.chars().count() as f64 + 4.0,
        height: 1.0,
        role: Some(JetAriaRole::Button),
        color: None,
        kind: JetUiNodeKind::Button,
        children: Vec::new(),
        key: None,
        on_click: None,
    }
}

/// D-WEB-CLICK-PORT1=D / D-UI-EVT-DISP1=E: portable `ui.button(label, on_click:)`.
/// Registers the handler in the shared slot table and stores the slot id on
/// the node. Backends bind identity→slot at paint/mount.
pub fn jet_ui_button_on_click<F: Fn() + Send + Sync + 'static>(label: &str, handler: F) -> JetUiNode {
    let mut node = jet_ui_button(label);
    node.on_click = Some(jet_ui_register_click(handler));
    node
}

pub fn jet_ui_box(children: Vec<JetUiNode>) -> JetUiNode {
    let width = children
        .iter()
        .map(|child| child.width)
        .fold(0.0_f64, f64::max);
    let height = children.iter().map(|child| child.height).sum();
    JetUiNode {
        label: String::new(),
        width,
        height,
        role: Some(JetAriaRole::Container),
        color: None,
        kind: JetUiNodeKind::Box,
        children,
        key: None,
        on_click: None,
    }
}

// ── D-UI-EVT-DISP1=E: O(1) node-keyed click slots ───────────────────────────

type JetUiClickHandler = std::sync::Arc<dyn Fn() + Send + Sync + 'static>;

fn jet_ui_click_slots() -> &'static std::sync::Mutex<std::collections::HashMap<i64, JetUiClickHandler>>
{
    static SLOTS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<i64, JetUiClickHandler>>,
    > = std::sync::OnceLock::new();
    SLOTS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn jet_ui_click_bindings()
-> &'static std::sync::Mutex<std::collections::HashMap<String, i64>> {
    static BINDINGS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, i64>>,
    > = std::sync::OnceLock::new();
    BINDINGS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn jet_ui_next_click_id() -> i64 {
    static NEXT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

pub fn jet_ui_register_click<F: Fn() + Send + Sync + 'static>(handler: F) -> i64 {
    let id = jet_ui_next_click_id();
    jet_ui_click_slots()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, std::sync::Arc::new(handler));
    id
}

/// Bind a render-path / author-key identity to a handler slot (paint/mount).
pub fn jet_ui_bind_click(identity: &str, slot: i64) {
    jet_ui_click_bindings()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(identity.to_string(), slot);
}

/// Clear the handler binding for an unmounted identity.
pub fn jet_ui_unbind_click(identity: &str) {
    jet_ui_click_bindings()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(identity);
}

/// D-UI-EVT-DISP1=E: O(1) dispatch by stable node identity.
pub fn jet_ui_dispatch(identity: &str) {
    let slot = jet_ui_click_bindings()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(identity)
        .copied();
    let Some(slot) = slot else {
        return;
    };
    let handler = jet_ui_click_slots()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&slot)
        .cloned();
    if let Some(handler) = handler {
        handler();
    }
}

/// Walk a freshly painted tree and bind each `on_click` slot to its identity.
pub fn jet_ui_bind_tree_clicks(node: &JetUiNode, path: &str) {
    let identity = match &node.key {
        Some(key) if !key.is_empty() => format!("key:{key}"),
        _ => path.to_string(),
    };
    if let Some(slot) = node.on_click {
        jet_ui_bind_click(&identity, slot);
    }
    if node.kind == JetUiNodeKind::Box {
        for (index, child) in node.children.iter().enumerate() {
            jet_ui_bind_tree_clicks(child, &format!("{path}/{index}"));
        }
    }
}

pub fn jet_ui_aria_role_button() -> JetAriaRole {
    JetAriaRole::Button
}

pub fn jet_ui_aria_role_text_input() -> JetAriaRole {
    JetAriaRole::TextInput
}

pub fn jet_ui_aria_role_label() -> JetAriaRole {
    JetAriaRole::Label
}

pub fn jet_ui_aria_role_container() -> JetAriaRole {
    JetAriaRole::Container
}

pub fn jet_ui_key_event(code: &str) -> JetInputEvent {
    JetInputEvent::Key {
        code: code.to_string(),
    }
}

pub fn jet_ui_resize_event(width: f64, height: f64) -> JetInputEvent {
    JetInputEvent::Resize {
        size: JetSize { width, height },
    }
}

impl JetShow for JetPoint {
    fn jet_show(&self) -> String {
        format!("{{x:{},y:{}}}", self.x, self.y)
    }
}

impl JetShow for JetSize {
    fn jet_show(&self) -> String {
        format!("{{w:{},h:{}}}", self.width, self.height)
    }
}

impl JetShow for JetRect {
    fn jet_show(&self) -> String {
        format!(
            "{{x:{},y:{},w:{},h:{}}}",
            self.x, self.y, self.width, self.height
        )
    }
}

impl JetShow for JetEventResult {
    fn jet_show(&self) -> String {
        match self {
            JetEventResult::Handled => "Handled".to_string(),
            JetEventResult::Ignored => "Ignored".to_string(),
        }
    }
}

impl JetShow for JetAriaRole {
    fn jet_show(&self) -> String {
        match self {
            JetAriaRole::Button => "Button".to_string(),
            JetAriaRole::TextInput => "TextInput".to_string(),
            JetAriaRole::Label => "Label".to_string(),
            JetAriaRole::Container => "Container".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_backend_measure_layout_paint_roundtrip() {
        let backend = JetNullBackend::new();
        let node = jet_ui_node("hello", 100.0, 20.0);
        let constraint = jet_ui_constraint(0.0, 0.0, 200.0, 100.0);
        let size = backend.measure_node(node.clone(), constraint);
        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 20.0);

        let frame = jet_ui_rect(0.0, 0.0, size.width, size.height);
        backend.layout_node(node.clone(), frame);
        backend.paint_node(node);

        let cmds = backend.paint_commands();
        assert_eq!(cmds.len(), 2);
        assert!(cmds[0].starts_with("fill("));
        assert!(cmds[1].contains("hello"));

        let key = jet_ui_key_event("enter");
        assert_eq!(backend.dispatch_event(key), JetEventResult::Handled);
        let bad = jet_ui_key_event("");
        assert_eq!(backend.dispatch_event(bad), JetEventResult::Ignored);
    }

    #[test]
    fn tui_backend_reactive_paint_is_deterministic() {
        let backend = JetTuiBackend::new();
        let node = jet_ui_node("hi", 2.0, 1.0);
        let constraint = jet_ui_constraint(0.0, 0.0, DEFAULT_MOUNT_COLS, DEFAULT_MOUNT_ROWS);
        let size = backend.measure_node(node.clone(), constraint);
        let frame = jet_ui_rect(0.0, 0.0, size.width, size.height);
        backend.layout_node(node.clone(), frame);
        backend.paint_node(node);
        assert_eq!(backend.render_count(), 1);
        assert_eq!(backend.frame_lines(), vec!["hi".to_string()]);
    }

    // D-A11YGATE1=B (c134 Phase 6): keyboard focus routing.
    #[test]
    fn focus_group_tab_cycles_and_wraps() {
        let backend = JetNullBackend::new();
        let save = jet_ui_node_role("Save", 40.0, 10.0, JetAriaRole::Button);
        let cancel = jet_ui_node_role("Cancel", 40.0, 10.0, JetAriaRole::Button);
        assert_eq!(backend.focused_label(), "");

        backend.set_focus_group(vec![save, cancel]);
        assert_eq!(backend.focused_label(), "Save");

        let tab = jet_ui_key_event("Tab");
        assert_eq!(backend.dispatch_event(tab.clone()), JetEventResult::Handled);
        assert_eq!(backend.focused_label(), "Cancel");

        assert_eq!(backend.dispatch_event(tab.clone()), JetEventResult::Handled);
        assert_eq!(backend.focused_label(), "Save");

        // A non-Tab key doesn't disturb focus.
        let enter = jet_ui_key_event("enter");
        assert_eq!(backend.dispatch_event(enter), JetEventResult::Handled);
        assert_eq!(backend.focused_label(), "Save");
    }

    #[test]
    fn aria_role_button_is_interactive_label_is_not() {
        assert!(JetAriaRole::Button.is_interactive());
        assert!(JetAriaRole::TextInput.is_interactive());
        assert!(!JetAriaRole::Label.is_interactive());
        assert!(!JetAriaRole::Container.is_interactive());
    }

    // D-WEB-CLICK-PORT1=D / D-UI-EVT-DISP1=E: paint binds identity→slot; dispatch runs it.
    #[test]
    fn portable_button_on_click_dispatches_after_paint() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static HITS: AtomicUsize = AtomicUsize::new(0);
        HITS.store(0, Ordering::SeqCst);

        let backend = JetNullBackend::new();
        let button = jet_ui_button_on_click("Go", || {
            HITS.fetch_add(1, Ordering::SeqCst);
        });
        let tree = jet_ui_box(vec![button]);
        backend.mount_node(
            tree,
            jet_ui_constraint(0.0, 0.0, DEFAULT_MOUNT_COLS, DEFAULT_MOUNT_ROWS),
        );
        // NullBackend keys the root as `null#0`; the button is child 0.
        jet_ui_dispatch("null#0/0");
        assert_eq!(HITS.load(Ordering::SeqCst), 1);
        jet_ui_dispatch("missing");
        assert_eq!(HITS.load(Ordering::SeqCst), 1);
    }
}
