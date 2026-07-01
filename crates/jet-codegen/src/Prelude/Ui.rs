// D-RENDERTGT1=A + D-RENDERTGT2=A (c133 M1): render-target backend trait seam.

use std::cell::RefCell;
use std::rc::Rc;

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

/// D-RENDERTGT2=A: portable backend seam between Jet UI and platform renderers.
pub trait JetBackend {
    fn measure(&mut self, node: &JetUiNode, constraint: JetSizeConstraint) -> JetSize;
    fn layout(&mut self, node: &JetUiNode, frame: JetRect);
    fn paint(&mut self, node: &JetUiNode);
    fn on_event(&mut self, event: JetInputEvent) -> JetEventResult;
}

#[derive(Clone)]
pub struct JetNullBackend {
    state: Rc<RefCell<JetNullBackendState>>,
}

impl JetNullBackend {
    pub fn new() -> Self {
        JetNullBackend {
            state: Rc::new(RefCell::new(JetNullBackendState {
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
        let mut state = self.state.borrow_mut();
        state.focused_index = if nodes.is_empty() { None } else { Some(0) };
        state.focus_nodes = nodes;
    }

    /// D-A11YGATE1=B: the accessible label of the currently focused node, or
    /// `""` when nothing is focused.
    pub fn focused_label(&self) -> String {
        let state = self.state.borrow();
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
        let mut state = self.state.borrow_mut();
        JetBackend::measure(&mut *state, &node, constraint)
    }

    pub fn layout_node(&self, node: JetUiNode, frame: JetRect) {
        let mut state = self.state.borrow_mut();
        JetBackend::layout(&mut *state, &node, frame);
    }

    pub fn paint_node(&self, node: JetUiNode) {
        let mut state = self.state.borrow_mut();
        JetBackend::paint(&mut *state, &node);
    }

    pub fn dispatch_event(&self, event: JetInputEvent) -> JetEventResult {
        let mut state = self.state.borrow_mut();
        JetBackend::on_event(&mut *state, event)
    }

    pub fn paint_commands(&self) -> Vec<String> {
        self.state
            .borrow()
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
        let width = node.width.clamp(constraint.min_width, constraint.max_width);
        let height = node.height.clamp(constraint.min_height, constraint.max_height);
        let size = JetSize { width, height };
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
        self.commands.push(JetPaintCmd::FillRect {
            rect: frame,
            color: node.color.clone().unwrap_or_else(|| "#000000".to_string()),
        });
        self.commands.push(JetPaintCmd::Text {
            rect: frame,
            text: node.label.clone(),
        });
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
    state: Rc<RefCell<JetTuiBackendState>>,
}

impl JetTuiBackend {
    pub fn new() -> Self {
        JetTuiBackend {
            state: Rc::new(RefCell::new(JetTuiBackendState {
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
        let mut state = self.state.borrow_mut();
        state.focused_index = if nodes.is_empty() { None } else { Some(0) };
        state.focus_nodes = nodes;
    }

    /// D-A11YGATE1=B: the accessible label of the currently focused node, or
    /// `""` when nothing is focused.
    pub fn focused_label(&self) -> String {
        let state = self.state.borrow();
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
        let mut state = self.state.borrow_mut();
        JetBackend::measure(&mut *state, &node, constraint)
    }

    pub fn layout_node(&self, node: JetUiNode, frame: JetRect) {
        let mut state = self.state.borrow_mut();
        JetBackend::layout(&mut *state, &node, frame);
    }

    pub fn paint_node(&self, node: JetUiNode) {
        let mut state = self.state.borrow_mut();
        JetBackend::paint(&mut *state, &node);
    }

    pub fn dispatch_event(&self, event: JetInputEvent) -> JetEventResult {
        let mut state = self.state.borrow_mut();
        JetBackend::on_event(&mut *state, event)
    }

    pub fn frame_lines(&self) -> Vec<String> {
        self.state
            .borrow()
            .grid
            .iter()
            .map(|row| row.iter().collect::<String>().trim_end().to_string())
            .collect()
    }

    pub fn render_count(&self) -> i64 {
        self.state.borrow().render_count as i64
    }
}

impl Default for JetTuiBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl JetBackend for JetTuiBackendState {
    fn measure(&mut self, node: &JetUiNode, constraint: JetSizeConstraint) -> JetSize {
        let width = node.width.clamp(constraint.min_width, constraint.max_width);
        let height = node.height.clamp(constraint.min_height, constraint.max_height);
        let size = JetSize { width, height };
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
        tui_write_label(&mut self.grid, &frame, &node.label);
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
pub fn jet_ui_reactive_render<F: Fn() + 'static>(body: F) {
    jet_std::jet_reactive_effect(body);
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
        role: None,
        color: Some(color.to_string()),
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
        let constraint = jet_ui_constraint(0.0, 0.0, 80.0, 24.0);
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
}
