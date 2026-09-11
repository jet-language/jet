// Render-target backend trait seam.
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
    /// D-FOUND-PLATFORM1=A: optional host-facing accessibility metadata.
    /// The metadata stays on this canonical node so hosts project facts by
    /// stable node identity without constructing a second UI tree.
    pub accessibility: Option<JetUiAccessibility>,
    /// D-FOUND-PLATFORM1=A: an explicit IME policy travels with the canonical
    /// text-input node. `None` preserves the default native policy for a
    /// textbox created through the role-only constructor.
    pub ime: Option<JetUiImeMode>,
    /// D-STYLESHAPE1=A (c134 Phase 3/7 wiring): explicit fill color as a
    /// `#RRGGBB` string, matching `JetPaintCmd::FillRect`'s existing color
    /// representation. `None` falls back to the default fill (`#000000`).
    pub color: Option<String>,
    /// D-TUIKIT1=A: optional typed presentation style travels with the same
    /// canonical node tree that every backend mounts.
    pub style: Option<JetTuiStyle>,
    /// D-UITREE1=A: every renderer consumes this same typed node kind/tree.
    pub kind: JetUiNodeKind,
    pub children: Vec<JetUiNode>,
    /// D-UI-NODE-ID1=C: optional author key; when set, identity is the key
    /// instead of the render path.
    pub key: Option<String>,
    /// D-UI-EVT-DISP1=E: node-keyed click handler slot using the shared
    /// portable callback registry. `None` means no click callback.
    pub on_click: Option<i64>,
    /// D-UI-DROP1=A: node-keyed drop handler slot using the shared typed
    /// `JetUiDropItem` payload. `None` means no drop callback.
    pub on_drop: Option<i64>,
    /// Optional host action binding carried by the canonical node.
    pub shortcut: Option<JetUiShortcut>,
}

/// D-FOUND-PLATFORM1=A: accessibility metadata belongs to the canonical node.
/// Host services attach and project this value; no parallel accessibility tree
/// or process-global registry is needed.
impl JetUiAccessibilityTarget for JetUiNode {
    fn attach_jet_ui_accessibility(
        &mut self,
        accessibility: JetUiAccessibility,
    ) -> Result<(), JetUiHostError> {
        self.accessibility = Some(accessibility);
        Ok(())
    }

    fn jet_ui_accessibility(&self) -> Option<&JetUiAccessibility> {
        self.accessibility.as_ref()
    }
}

/// Route accessibility through the explicit host while keeping metadata on the
/// canonical node. The host owns capability checks and the typed outcome.
pub fn jet_ui_attach_accessibility(
    host: &dyn JetUiHost,
    node: &mut JetUiNode,
    accessibility: JetUiAccessibility,
) -> JetUiServiceResult<()> {
    host.attach_accessibility(node, accessibility)
}

/// Project metadata for one explicit, stable node identity.
pub fn jet_ui_project_accessibility(
    host: &dyn JetUiHost,
    node: &JetUiNode,
    node_id: JetUiNodeId,
) -> JetUiServiceResult<Option<JetUiAccessibilityProjection>> {
    host.project_accessibility(node, node_id)
}

/// Read the host's capability facts without consulting ambient state.
pub fn jet_ui_capability_facts(host: &dyn JetUiHost) -> JetUiCapabilityFacts {
    host.capability_facts()
}
/// Expose the canonical devtools panel selection and time cursor to UI code.
/// The envelope owns the state; this seam only borrows it, so every host
/// observes the same typed value without a UI-local copy or ambient lookup.
pub fn jet_ui_devtools_view_state(
    envelope: &JetDevtoolsEnvelope,
) -> &JetDevtoolsViewState {
    envelope.view()
}


fn jet_ui_validate_file_dialog_selection(
    request: &JetUiFileDialogRequest,
    expected_kind: JetUiFileDialogKind,
    selection: &JetUiFileDialogSelection,
) -> Result<(), JetUiHostError> {
    if request.kind != expected_kind {
        let operation = match expected_kind {
            JetUiFileDialogKind::Open => "open_file",
            JetUiFileDialogKind::Save => "save_file",
        };
        return Err(JetUiHostError::InvalidRequest(format!(
            "{operation} requires a {:?} dialog request",
            expected_kind
        )));
    }
    request.validate()?;
    if selection.files.is_empty() {
        return Err(JetUiHostError::InvalidRequest(
            "a completed file dialog must select at least one file".to_string(),
        ));
    }
    if !request.allow_multiple && selection.files.len() > 1 {
        return Err(JetUiHostError::InvalidRequest(
            "a single-selection dialog returned multiple files".to_string(),
        ));
    }

    let access = match request.kind {
        JetUiFileDialogKind::Open => JetUiFsAccess::Read,
        JetUiFileDialogKind::Save => JetUiFsAccess::Write,
    };
    for selected in &selection.files {
        let scoped = request.grant.scope(selected.path(), access)?;
        if scoped.path() != selected.path()
            || scoped.grant_root() != selected.grant_root()
            || scoped.access() != selected.access()
        {
            return Err(JetUiHostError::ResourceDenied(format!(
                "file dialog selection `{}` is outside its grant",
                selected.path()
            )));
        }
    }
    Ok(())
}

fn jet_ui_checked_file_dialog_result(
    request: &JetUiFileDialogRequest,
    expected_kind: JetUiFileDialogKind,
    result: JetUiServiceResult<JetUiFileDialogSelection>,
) -> JetUiServiceResult<JetUiFileDialogSelection> {
    match result {
        Ok(selection) => {
            match jet_ui_validate_file_dialog_selection(request, expected_kind, &selection) {
                Ok(()) => Ok(selection),
                Err(error) => Err(error),
            }
        }
        other => other,
    }
}

/// Open a file through the explicit host and re-check every returned path
/// against the request's grant before exposing a completed result.
pub fn jet_ui_open_file(
    host: &mut dyn JetUiHost,
    request: JetUiFileDialogRequest,
) -> JetUiServiceResult<JetUiFileDialogSelection> {
    let checked_request = request.clone();
    jet_ui_checked_file_dialog_result(
        &checked_request,
        JetUiFileDialogKind::Open,
        host.open_file(request),
    )
}

/// Save a file through the explicit host and re-check every returned path
/// against the request's grant before exposing a completed result.
pub fn jet_ui_save_file(
    host: &mut dyn JetUiHost,
    request: JetUiFileDialogRequest,
) -> JetUiServiceResult<JetUiFileDialogSelection> {
    let checked_request = request.clone();
    jet_ui_checked_file_dialog_result(
        &checked_request,
        JetUiFileDialogKind::Save,
        host.save_file(request),
    )
}

pub fn jet_ui_read_clipboard_text(
    host: &mut dyn JetUiHost,
) -> JetUiServiceResult<JetUiClipboardText> {
    host.read_clipboard_text()
}

pub fn jet_ui_write_clipboard_text(
    host: &mut dyn JetUiHost,
    text: String,
) -> JetUiServiceResult<JetUiClipboardWrite> {
    host.write_clipboard_text(text)
}

pub fn jet_ui_poll_ime_event(
    host: &mut dyn JetUiHost,
) -> JetUiServiceResult<Option<JetUiImeEvent>> {
    host.poll_ime_event()
}

pub fn jet_ui_poll_drag_event(
    host: &mut dyn JetUiHost,
) -> JetUiServiceResult<Option<JetUiDragEvent>> {
    host.poll_drag_event()
}

pub fn jet_ui_register_shortcut(
    host: &mut dyn JetUiHost,
    binding: JetUiShortcutBinding,
) -> JetUiServiceResult<JetUiShortcutBinding> {
    host.register_shortcut(binding)
}

pub fn jet_ui_dispatch_shortcut(
    host: &mut dyn JetUiHost,
    shortcut: JetUiShortcut,
) -> JetUiServiceResult<JetUiShortcutDispatch> {
    host.dispatch_shortcut(shortcut)
}
/// Route canonical-node metadata through the scoped current host.  The host
/// remains the capability authority; the node remains the only metadata store.
pub fn jet_ui_host_attach_accessibility(
    mut node: JetUiNode,
    accessibility: JetUiAccessibility,
) -> JetUiServiceResult<JetUiNode> {
    jet_ui_with_current_host(|host| {
        match host.attach_accessibility(&mut node, accessibility) {
            Ok(()) => Ok(node),
            Err(error) => Err(error),
        }
    })
}

pub fn jet_ui_host_project_accessibility(
    node: JetUiNode,
    node_id: JetUiNodeId,
) -> JetUiServiceResult<Option<JetUiAccessibilityProjection>> {
    jet_ui_with_current_host(|host| host.project_accessibility(&node, node_id))
}

pub fn jet_ui_node_accessibility(
    mut node: JetUiNode,
    accessibility: JetUiAccessibility,
) -> JetUiNode {
    node.accessibility = Some(accessibility);
    node
}

pub fn jet_ui_node_shortcut(
    mut node: JetUiNode,
    shortcut: JetUiShortcut,
) -> JetUiNode {
    node.shortcut = Some(shortcut);
    node
}

pub fn jet_ui_text_input(
    state: &str,
    ime: JetUiImeMode,
) -> JetUiNode {
    let mut node =
        jet_ui_node_role(state, state.chars().count() as f64, 1.0, JetAriaRole::TextInput);
    node.kind = JetUiNodeKind::TextInput;
    node.ime = Some(ime);
    node
}

/// D-UI-DROP1=A: the text-input drop callback is attached to the same
/// canonical node as the state and IME mode; it does not create a second tree.
pub fn jet_ui_text_input_on_drop<F: Fn(Vec<JetUiDropItem>) + Send + Sync + 'static>(
    state: &str,
    ime: JetUiImeMode,
    handler: F,
) -> JetUiNode {
    let mut node = jet_ui_text_input(state, ime);
    node.on_drop = Some(jet_ui_register_drop(handler));
    node
}

pub fn jet_font_shape_run(text: &str, face: &JetFontFace) -> JetGlyphRun {
    jet_font_shape(text, face)
}


#[derive(Clone, Debug, PartialEq)]
pub enum JetInputEvent {
    Key { code: String },
    Resize { size: JetSize },
}
/// Events delivered to a model-update-view program.  `JetInputEvent` remains
/// the small backend protocol; this richer event is the typed application
/// protocol and keeps timer/I/O messages on the same transition path as keys.
#[derive(Clone, Debug, PartialEq)]
pub enum JetTuiEvent {
    Key { code: String, modifiers: u8 },
    Resize { size: JetSize },
    Timer { id: String, elapsed_ms: i64 },
    Io { channel: String, payload: Vec<u8> },
    Focus { focused: bool },
    Interrupt,
    Close,
}

impl JetTuiEvent {
    pub fn key(code: &str) -> Self {
        Self::Key {
            code: code.to_string(),
            modifiers: 0,
        }
    }

    pub fn key_with_modifiers(code: &str, modifiers: u8) -> Self {
        Self::Key {
            code: code.to_string(),
            modifiers,
        }
    }

    pub fn resize(width: f64, height: f64) -> Self {
        Self::Resize {
            size: JetSize { width, height },
        }
    }

    pub fn timer(id: &str, elapsed_ms: i64) -> Self {
        Self::Timer {
            id: id.to_string(),
            elapsed_ms,
        }
    }

    pub fn io(channel: &str, payload: Vec<u8>) -> Self {
        Self::Io {
            channel: channel.to_string(),
            payload,
        }
    }

    pub fn focus(focused: bool) -> Self {
        Self::Focus { focused }
    }

}

/// Terminal presentation capability.  Presentation changes here; model,
/// messages, and the canonical `JetUiNode` tree do not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetTuiColorProfile {
    Ansi16,
    Ansi256,
    TrueColor,
    Ascii,
}

impl JetTuiColorProfile {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ansi16 => "ansi16",
            Self::Ansi256 => "ansi256",
            Self::TrueColor => "truecolor",
            Self::Ascii => "ascii",
        }
    }

    fn kernel(self) -> jet_tui_kernel::ColorProfile {
        match self {
            Self::Ansi16 => jet_tui_kernel::ColorProfile::Ansi16,
            Self::Ansi256 => jet_tui_kernel::ColorProfile::Ansi256,
            Self::TrueColor => jet_tui_kernel::ColorProfile::TrueColor,
            Self::Ascii => jet_tui_kernel::ColorProfile::Ascii,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetTuiCapabilities {
    pub profile: JetTuiColorProfile,
    pub color: bool,
    pub unicode: bool,
    pub mouse: bool,
    pub resize: bool,
    pub clipboard: bool,
    pub width: usize,
    pub height: usize,
}

impl JetTuiCapabilities {
    pub fn plain(width: usize, height: usize) -> Self {
        Self::from_kernel(jet_tui_kernel::capabilities(
            jet_tui_kernel::ColorProfile::Ascii,
            width,
            height,
        ))
    }

    pub fn for_profile(profile: JetTuiColorProfile, width: usize, height: usize) -> Self {
        Self::from_kernel(jet_tui_kernel::capabilities(profile.kernel(), width, height))
    }

    fn from_kernel(capabilities: jet_tui_kernel::Capabilities) -> Self {
        Self {
            profile: match capabilities.profile {
                jet_tui_kernel::ColorProfile::Ansi16 => JetTuiColorProfile::Ansi16,
                jet_tui_kernel::ColorProfile::Ansi256 => JetTuiColorProfile::Ansi256,
                jet_tui_kernel::ColorProfile::TrueColor => JetTuiColorProfile::TrueColor,
                jet_tui_kernel::ColorProfile::Ascii => JetTuiColorProfile::Ascii,
            },
            color: capabilities.color,
            unicode: capabilities.unicode,
            mouse: capabilities.mouse,
            resize: capabilities.resize,
            clipboard: capabilities.clipboard,
            width: capabilities.width,
            height: capabilities.height,
        }
    }

    fn kernel(&self) -> jet_tui_kernel::Capabilities {
        jet_tui_kernel::Capabilities {
            profile: self.profile.kernel(),
            color: self.color,
            unicode: self.unicode,
            mouse: self.mouse,
            resize: self.resize,
            clipboard: self.clipboard,
            width: self.width,
            height: self.height,
        }
    }

    pub const fn supports_color(&self) -> bool {
        self.color
    }
}

fn jet_tui_env_dimension(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

fn jet_tui_env_flag(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| value != "0")
}

/// Resolve one presentation profile.  `NO_COLOR` is an unconditional
/// no-ANSI override; `FORCE_COLOR` only opts a real terminal into color.
pub fn jet_tui_capabilities() -> JetTuiCapabilities {
    let width = jet_tui_env_dimension("COLUMNS", 80);
    let height = jet_tui_env_dimension("LINES", 24);
    let term = std::env::var("TERM").unwrap_or_default().to_ascii_lowercase();
    let color_term = std::env::var("COLORTERM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let ascii = jet_tui_env_flag("JET_TUI_ASCII") || term == "dumb";
    let profile = if ascii {
        JetTuiColorProfile::Ascii
    } else if color_term == "truecolor"
        || color_term == "24bit"
        || term.contains("truecolor")
        || term.contains("24bit")
    {
        JetTuiColorProfile::TrueColor
    } else if term.contains("256color") {
        JetTuiColorProfile::Ansi256
    } else {
        JetTuiColorProfile::Ansi16
    };
    let no_color = std::env::var_os("NO_COLOR").is_some();
    let forced = jet_tui_env_flag("FORCE_COLOR");
    let terminal = std::io::IsTerminal::is_terminal(&std::io::stdout());
    let mut capabilities = JetTuiCapabilities::for_profile(profile, width, height);
    capabilities.color = !no_color && profile != JetTuiColorProfile::Ascii && (terminal || forced);
    if no_color {
        capabilities.mouse = false;
        capabilities.clipboard = false;
    }
    capabilities
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetTuiColor {
    Ansi16(u8),
    Ansi256(u8),
    Rgb(u8, u8, u8),
}

impl JetTuiColor {
    fn kernel(self) -> jet_tui_kernel::Color {
        match self {
            Self::Ansi16(index) => jet_tui_kernel::Color::Ansi16(index),
            Self::Ansi256(index) => jet_tui_kernel::Color::Ansi256(index),
            Self::Rgb(red, green, blue) => jet_tui_kernel::Color::Rgb(red, green, blue),
        }
    }

    fn from_kernel(color: jet_tui_kernel::Color) -> Self {
        match color {
            jet_tui_kernel::Color::Ansi16(index) => Self::Ansi16(index),
            jet_tui_kernel::Color::Ansi256(index) => Self::Ansi256(index),
            jet_tui_kernel::Color::Rgb(red, green, blue) => Self::Rgb(red, green, blue),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JetTuiStyle {
    pub foreground: Option<JetTuiColor>,
    pub background: Option<JetTuiColor>,
    pub bold: bool,
    pub dim: bool,
    pub underline: bool,
}

impl JetTuiStyle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn foreground(mut self, color: JetTuiColor) -> Self {
        self.foreground = Some(color);
        self
    }

    pub fn background(mut self, color: JetTuiColor) -> Self {
        self.background = Some(color);
        self
    }

    pub fn bold(mut self, enabled: bool) -> Self {
        self.bold = enabled;
        self
    }

    pub fn dim(mut self, enabled: bool) -> Self {
        self.dim = enabled;
        self
    }

    pub fn underline(mut self, enabled: bool) -> Self {
        self.underline = enabled;
        self
    }

    fn kernel(&self) -> jet_tui_kernel::Style {
        jet_tui_kernel::Style {
            foreground: self.foreground.map(JetTuiColor::kernel),
            background: self.background.map(JetTuiColor::kernel),
            bold: self.bold,
            dim: self.dim,
            underline: self.underline,
        }
    }

    pub fn render(&self, text: &str, capabilities: &JetTuiCapabilities) -> String {
        jet_tui_kernel::style_text(text, &self.kernel(), &capabilities.kernel())
    }
}

pub fn jet_tui_ascii(text: &str) -> String {
    jet_tui_kernel::ascii(text)
}

pub fn jet_tui_strip_ansi(text: &str) -> String {
    jet_tui_kernel::strip_ansi(text)
}

fn jet_tui_char_width(ch: char) -> usize {
    jet_tui_kernel::char_width(ch)
}

pub fn jet_tui_display_width(text: &str) -> usize {
    jet_tui_kernel::display_width(text)
}

pub fn jet_tui_truncate(text: &str, width: usize) -> String {
    jet_tui_kernel::truncate(text, width)
}

pub fn jet_tui_pad(text: &str, width: usize) -> String {
    jet_tui_kernel::pad(text, width)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JetTuiConstraint {
    Length(f64),
    Min(f64),
    Max(f64),
    Percent(f64),
    Fill(u16),
}

impl JetTuiConstraint {
    fn kernel(self) -> jet_tui_kernel::Constraint {
        match self {
            Self::Length(value) => jet_tui_kernel::Constraint::Length(value),
            Self::Min(value) => jet_tui_kernel::Constraint::Min(value),
            Self::Max(value) => jet_tui_kernel::Constraint::Max(value),
            Self::Percent(value) => jet_tui_kernel::Constraint::Percent(value),
            Self::Fill(weight) => jet_tui_kernel::Constraint::Fill(weight),
        }
    }

    fn from_kernel(constraint: jet_tui_kernel::Constraint) -> Self {
        match constraint {
            jet_tui_kernel::Constraint::Length(value) => Self::Length(value),
            jet_tui_kernel::Constraint::Min(value) => Self::Min(value),
            jet_tui_kernel::Constraint::Max(value) => Self::Max(value),
            jet_tui_kernel::Constraint::Percent(value) => Self::Percent(value),
            jet_tui_kernel::Constraint::Fill(weight) => Self::Fill(weight),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetTuiDirection {
    Horizontal,
    Vertical,
}

impl JetTuiDirection {
    fn kernel(self) -> jet_tui_kernel::Direction {
        match self {
            Self::Horizontal => jet_tui_kernel::Direction::Horizontal,
            Self::Vertical => jet_tui_kernel::Direction::Vertical,
        }
    }
}

/// Deterministic one-dimensional constraint allocation. Nested layouts call
/// this same function on each returned rectangle; no backend gets a second
/// solver or a different rounding policy.
pub fn jet_tui_layout(
    area: JetRect,
    direction: JetTuiDirection,
    constraints: Vec<JetTuiConstraint>,
) -> Vec<JetRect> {
    let area = jet_tui_kernel::Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height,
    };
    let constraints = constraints
        .into_iter()
        .map(JetTuiConstraint::kernel)
        .collect::<Vec<_>>();
    jet_tui_kernel::layout(area, direction.kernel(), &constraints)
        .into_iter()
        .map(|rect| JetRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        })
        .collect()
}

pub trait JetTuiWidget {
    fn render(&self, area: JetRect) -> JetUiNode;
}

pub trait JetTuiStatefulWidget {
    type State;

    fn render(&self, area: JetRect, state: &mut Self::State) -> JetUiNode;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JetTuiTextWidget {
    pub text: String,
}

impl JetTuiTextWidget {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
        }
    }
}

impl JetTuiWidget for JetTuiTextWidget {
    fn render(&self, area: JetRect) -> JetUiNode {
        jet_ui_text(&jet_tui_truncate(&self.text, area.width.max(0.0).floor() as usize))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JetTuiListState {
    selected: usize,
    offset: usize,
}

impl JetTuiListState {
    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn select(&mut self, selected: usize) {
        self.selected = selected;
    }

    pub fn scroll_to(&mut self, offset: usize) {
        self.offset = offset;
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JetTuiList {
    pub items: Vec<String>,
}

impl JetTuiList {
    pub fn new(items: Vec<String>) -> Self {
        Self { items }
    }
}

impl JetTuiStatefulWidget for JetTuiList {
    type State = JetTuiListState;

    fn render(&self, area: JetRect, state: &mut Self::State) -> JetUiNode {
        let rows = area.height.max(1.0).floor() as usize;
        let width = area.width.max(0.0).floor() as usize;
        let selected = state.selected.min(self.items.len().saturating_sub(1));
        state.selected = selected;
        if selected < state.offset {
            state.offset = selected;
        }
        if rows > 0 && selected >= state.offset + rows {
            state.offset = selected + 1 - rows;
        }
        let mut children = Vec::new();
        for (index, item) in self
            .items
            .iter()
            .enumerate()
            .skip(state.offset)
            .take(rows)
        {
            let prefix = if index == selected { "> " } else { "  " };
            children.push(jet_ui_text(&jet_tui_truncate(
                &format!("{prefix}{item}"),
                width,
            )));
        }
        if children.is_empty() {
            children.push(jet_ui_text(""));
        }
        jet_ui_box(children)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JetTuiTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl JetTuiTable {
    pub fn new(headers: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Self { headers, rows }
    }
}

fn jet_tui_table_line(values: &[String], columns: usize, width: usize) -> String {
    if columns == 0 {
        return String::new();
    }
    let separators = columns.saturating_sub(1) * 3;
    let column_width = width.saturating_sub(separators) / columns;
    let mut line = String::new();
    for index in 0..columns {
        if index > 0 {
            line.push_str(" | ");
        }
        let value = values.get(index).map(String::as_str).unwrap_or("");
        let value = jet_tui_truncate(value, column_width);
        if index + 1 < columns {
            line.push_str(&jet_tui_pad(&value, column_width));
        } else {
            line.push_str(&value);
        }
    }
    jet_tui_truncate(&line, width)
}

impl JetTuiWidget for JetTuiTable {
    fn render(&self, area: JetRect) -> JetUiNode {
        let columns = self
            .headers
            .len()
            .max(self.rows.iter().map(|row| row.len()).max().unwrap_or(0));
        if columns == 0 {
            return jet_ui_text("");
        }
        let width = area.width.max(0.0).floor() as usize;
        let mut children = Vec::new();
        if !self.headers.is_empty() {
            children.push(jet_ui_text(&jet_tui_table_line(
                &self.headers,
                columns,
                width,
            )));
        }
        for row in &self.rows {
            children.push(jet_ui_text(&jet_tui_table_line(row, columns, width)));
        }
        jet_ui_box(children)
    }
}

#[derive(Clone, Debug, Default)]
pub struct JetTuiBindings {
    registry: JetUiShortcutRegistry,
}

impl JetTuiBindings {
    pub fn register(
        &mut self,
        binding: JetUiShortcutBinding,
    ) -> Result<JetUiShortcutBinding, JetUiHostError> {
        self.registry.register(binding)
    }

    pub fn dispatch(&self, event: &JetTuiEvent) -> Option<JetUiShortcutBinding> {
        let JetTuiEvent::Key { code, modifiers } = event else {
            return None;
        };
        let shortcut =
            JetUiShortcut::new(code, JetUiShortcutModifiers::from_bits(*modifiers)).ok()?;
        match self.registry.dispatch(&shortcut) {
            JetUiShortcutDispatch::Dispatched(binding) => Some(binding),
            JetUiShortcutDispatch::Unhandled => None,
        }
    }

    pub fn bindings(&self) -> Vec<JetUiShortcutBinding> {
        self.registry.bindings()
    }

    pub fn help_lines(&self, width: usize) -> Vec<String> {
        self.registry
            .bindings()
            .into_iter()
            .map(|binding| {
                jet_tui_truncate(
                    &format!(
                        "{}  {}",
                        jet_tui_shortcut_label(&binding.shortcut),
                        binding.action
                    ),
                    width,
                )
            })
            .collect()
    }

    pub fn help_node(&self, width: usize) -> JetUiNode {
        jet_ui_box(
            self.help_lines(width)
                .into_iter()
                .map(|line| jet_ui_text(&line))
                .collect(),
        )
    }
}

fn jet_tui_shortcut_label(shortcut: &JetUiShortcut) -> String {
    let modifiers = shortcut.modifiers;
    let mut label = String::new();
    for (modifier, text) in [
        (JetUiShortcutModifier::Control, "Ctrl-"),
        (JetUiShortcutModifier::Alt, "Alt-"),
        (JetUiShortcutModifier::Shift, "Shift-"),
        (JetUiShortcutModifier::Meta, "Meta-"),
        (JetUiShortcutModifier::Command, "Cmd-"),
    ] {
        if modifiers.contains(modifier) {
            label.push_str(text);
        }
    }
    label.push_str(&shortcut.key);
    label
}

pub enum JetTuiCommand<Msg> {
    None,
    Task(Box<dyn FnOnce() -> Msg + Send + 'static>),
    Batch(Vec<JetTuiCommand<Msg>>),
}

impl<Msg> Default for JetTuiCommand<Msg> {
    fn default() -> Self {
        Self::None
    }
}

impl<Msg> JetTuiCommand<Msg> {
    pub fn none() -> Self {
        Self::None
    }

    pub fn task<F>(task: F) -> Self
    where
        F: FnOnce() -> Msg + Send + 'static,
    {
        Self::Task(Box::new(task))
    }

    pub fn batch(commands: Vec<Self>) -> Self {
        Self::Batch(commands)
    }
}

pub trait JetTuiRenderTarget {
    fn mount_tui(&self, node: JetUiNode, constraint: JetSizeConstraint);
    fn dispatch_tui(&self, event: JetInputEvent) -> JetEventResult;
    fn dispatch_tui_event(&self, event: JetTuiEvent) -> JetEventResult;
    fn frame_lines_tui(&self) -> Vec<String>;
}

pub struct JetTuiProgram<M, Msg, Update, View, B = JetTuiBackend> {
    model: Option<M>,
    update: Update,
    view: View,
    backend: B,
    constraint: JetSizeConstraint,
    event_mapper: Option<Box<dyn FnMut(JetTuiEvent) -> Option<Msg> + Send>>,
    bindings: JetTuiBindings,
    pending: Vec<std::sync::mpsc::Receiver<Msg>>,
    dirty: bool,
    stopped: bool,
}

impl<M, Msg, Update, View, B> JetTuiProgram<M, Msg, Update, View, B>
where
    Msg: Send + 'static,
    Update: FnMut(M, Msg) -> (M, JetTuiCommand<Msg>),
    View: Fn(&M) -> JetUiNode,
    B: JetTuiRenderTarget,
{
    pub fn with_backend(model: M, update: Update, view: View, backend: B) -> Self {
        Self {
            model: Some(model),
            update,
            view,
            backend,
            constraint: jet_ui_constraint(0.0, 0.0, DEFAULT_MOUNT_COLS, DEFAULT_MOUNT_ROWS),
            event_mapper: None,
            bindings: JetTuiBindings::default(),
            pending: Vec::new(),
            dirty: true,
            stopped: false,
        }
    }

    pub fn model(&self) -> Option<&M> {
        self.model.as_ref()
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn set_constraint(&mut self, constraint: JetSizeConstraint) {
        self.constraint = constraint;
        self.dirty = true;
    }

    pub fn set_event_mapper<F>(&mut self, mapper: F)
    where
        F: FnMut(JetTuiEvent) -> Option<Msg> + Send + 'static,
    {
        self.event_mapper = Some(Box::new(mapper));
    }

    pub fn set_bindings(&mut self, bindings: JetTuiBindings) {
        self.bindings = bindings;
    }

    pub fn bindings(&self) -> &JetTuiBindings {
        &self.bindings
    }

    pub fn redraw(&mut self) {
        let Some(model) = self.model.as_ref() else {
            return;
        };
        let node = (self.view)(model);
        self.backend.mount_tui(node, self.constraint);
        self.dirty = false;
    }

    pub fn dispatch_message(&mut self, message: Msg) -> bool {
        if self.stopped {
            return false;
        }
        let Some(model) = self.model.take() else {
            return false;
        };
        let (model, command) = (self.update)(model, message);
        self.model = Some(model);
        self.start_command(command);
        self.dirty = true;
        true
    }

    pub fn dispatch_event(&mut self, event: JetTuiEvent) -> JetEventResult {
        let resized = matches!(event, JetTuiEvent::Resize { .. });
        let backend_result = self.backend.dispatch_tui_event(event.clone());
        let mapped = self
            .event_mapper
            .as_mut()
            .and_then(|mapper| mapper(event.clone()));
        if let Some(message) = mapped {
            self.dispatch_message(message);
            JetEventResult::Handled
        } else if matches!(event, JetTuiEvent::Interrupt | JetTuiEvent::Close) {
            self.stopped = true;
            JetEventResult::Handled
        } else {
            if resized {
                self.dirty = true;
            }
            backend_result
        }
    }

    pub fn dispatch_bound_event<F>(&mut self, event: JetTuiEvent, map: F) -> bool
    where
        F: FnOnce(String) -> Option<Msg>,
    {
        let Some(binding) = self.bindings.dispatch(&event) else {
            return false;
        };
        map(binding.action).is_some_and(|message| self.dispatch_message(message))
    }

    pub fn pump_commands(&mut self) -> usize {
        let mut messages = Vec::new();
        self.pending.retain(|receiver| match receiver.try_recv() {
            Ok(message) => {
                messages.push(message);
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        let count = messages.len();
        for message in messages {
            self.dispatch_message(message);
        }
        count
    }

    pub fn step(&mut self) -> bool {
        let completed = self.pump_commands();
        if self.dirty {
            self.redraw();
            true
        } else {
            completed > 0
        }
    }

    pub fn run_headless<I>(&mut self, events: I) -> Vec<Vec<String>>
    where
        I: IntoIterator<Item = JetTuiEvent>,
    {
        self.redraw();
        let mut frames = vec![self.backend.frame_lines_tui()];
        for event in events {
            if !self.is_running() {
                break;
            }
            self.dispatch_event(event);
            self.step();
            frames.push(self.backend.frame_lines_tui());
        }
        frames
    }

    pub fn frame_lines(&self) -> Vec<String> {
        self.backend.frame_lines_tui()
    }

    pub fn stop(&mut self) {
        self.stopped = true;
    }

    pub fn is_running(&self) -> bool {
        !self.stopped
    }

    fn start_command(&mut self, command: JetTuiCommand<Msg>) {
        match command {
            JetTuiCommand::None => {}
            JetTuiCommand::Task(task) => {
                let (sender, receiver) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = sender.send(task());
                });
                self.pending.push(receiver);
            }
            JetTuiCommand::Batch(commands) => {
                for command in commands {
                    self.start_command(command);
                }
            }
        }
    }
}

impl<M, Msg, Update, View> JetTuiProgram<M, Msg, Update, View, JetTuiBackend>
where
    Msg: Send + 'static,
    Update: FnMut(M, Msg) -> (M, JetTuiCommand<Msg>),
    View: Fn(&M) -> JetUiNode,
{
    pub fn new(model: M, update: Update, view: View) -> Self {
        Self::with_backend(model, update, view, JetTuiBackend::new())
    }
}

pub fn jet_tui_text_widget(text: &str) -> JetTuiTextWidget {
    JetTuiTextWidget::new(text)
}

pub fn jet_tui_list(items: Vec<String>) -> JetUiNode {
    let capabilities = jet_tui_capabilities();
    let area = jet_ui_rect(
        0.0,
        0.0,
        capabilities.width as f64,
        capabilities.height as f64,
    );
    JetTuiList::new(items).render(area, &mut JetTuiListState::default())
}

pub fn jet_tui_table(headers: Vec<String>, rows: Vec<Vec<String>>) -> JetUiNode {
    let capabilities = jet_tui_capabilities();
    let area = jet_ui_rect(
        0.0,
        0.0,
        capabilities.width as f64,
        capabilities.height as f64,
    );
    JetTuiTable::new(headers, rows).render(area)
}

pub fn jet_ui_tui_bindings(
    bindings: Vec<JetUiShortcutBinding>,
) -> Result<JetTuiBindings, JetUiHostError> {
    let mut result = JetTuiBindings::default();
    for binding in bindings {
        result.register(binding)?;
    }
    Ok(result)
}

pub fn jet_ui_tui_help(bindings: &JetTuiBindings, width: f64) -> JetUiNode {
    bindings.help_node(width.max(0.0).floor() as usize)
}
pub fn jet_tui_key_event(code: &str) -> JetTuiEvent {
    JetTuiEvent::key(code)
}

pub fn jet_tui_key_event_modifiers(code: &str, modifiers: i64) -> JetTuiEvent {
    JetTuiEvent::key_with_modifiers(code, modifiers as u8)
}

pub fn jet_tui_resize_event(width: f64, height: f64) -> JetTuiEvent {
    JetTuiEvent::resize(width, height)
}

pub fn jet_tui_timer_event(id: &str, elapsed_ms: i64) -> JetTuiEvent {
    JetTuiEvent::timer(id, elapsed_ms)
}

pub fn jet_tui_io_event(channel: &str, payload: Vec<u8>) -> JetTuiEvent {
    JetTuiEvent::io(channel, payload)
}

pub fn jet_tui_focus_event(focused: bool) -> JetTuiEvent {
    JetTuiEvent::focus(focused)
}

pub fn jet_tui_interrupt_event() -> JetTuiEvent {
    JetTuiEvent::Interrupt
}

pub fn jet_tui_close_event() -> JetTuiEvent {
    JetTuiEvent::Close
}

pub fn jet_tui_color_ansi16(index: i64) -> JetTuiColor {
    JetTuiColor::from_kernel(jet_tui_kernel::color_ansi16(index))
}

pub fn jet_tui_color_ansi256(index: i64) -> JetTuiColor {
    JetTuiColor::from_kernel(jet_tui_kernel::color_ansi256(index))
}

pub fn jet_tui_color_rgb(red: i64, green: i64, blue: i64) -> JetTuiColor {
    JetTuiColor::from_kernel(jet_tui_kernel::color_rgb(red, green, blue))
}

pub fn jet_tui_style() -> JetTuiStyle {
    JetTuiStyle::new()
}

pub fn jet_tui_style_foreground(style: JetTuiStyle, color: JetTuiColor) -> JetTuiStyle {
    style.foreground(color)
}

pub fn jet_tui_style_background(style: JetTuiStyle, color: JetTuiColor) -> JetTuiStyle {
    style.background(color)
}

pub fn jet_tui_style_bold(style: JetTuiStyle, enabled: bool) -> JetTuiStyle {
    style.bold(enabled)
}

pub fn jet_tui_style_dim(style: JetTuiStyle, enabled: bool) -> JetTuiStyle {
    style.dim(enabled)
}

pub fn jet_tui_style_underline(style: JetTuiStyle, enabled: bool) -> JetTuiStyle {
    style.underline(enabled)
}

pub fn jet_tui_style_text(
    text: &str,
    style: JetTuiStyle,
    capabilities: JetTuiCapabilities,
) -> String {
    style.render(text, &capabilities)
}

pub fn jet_tui_display_width_int(text: &str) -> i64 {
    jet_tui_display_width(text) as i64
}

pub fn jet_tui_length(value: f64) -> JetTuiConstraint {
    JetTuiConstraint::from_kernel(jet_tui_kernel::length(value))
}

pub fn jet_tui_min(value: f64) -> JetTuiConstraint {
    JetTuiConstraint::from_kernel(jet_tui_kernel::min(value))
}

pub fn jet_tui_max(value: f64) -> JetTuiConstraint {
    JetTuiConstraint::from_kernel(jet_tui_kernel::max(value))
}

pub fn jet_tui_percent(value: f64) -> JetTuiConstraint {
    JetTuiConstraint::from_kernel(jet_tui_kernel::percent(value))
}

pub fn jet_tui_fill(weight: f64) -> JetTuiConstraint {
    JetTuiConstraint::from_kernel(jet_tui_kernel::fill(weight))
}

pub fn jet_tui_horizontal() -> JetTuiDirection {
    JetTuiDirection::Horizontal
}

pub fn jet_tui_vertical() -> JetTuiDirection {
    JetTuiDirection::Vertical
}

pub fn jet_tui_list_state() -> JetTuiListState {
    JetTuiListState::default()
}

pub fn jet_tui_list_state_select(mut state: JetTuiListState, index: i64) -> JetTuiListState {
    state.select(index.max(0) as usize);
    state
}

pub fn jet_tui_list_state_selected(state: JetTuiListState) -> i64 {
    state.selected() as i64
}

pub fn jet_tui_list_state_offset(mut state: JetTuiListState, offset: i64) -> JetTuiListState {
    state.scroll_to(offset.max(0) as usize);
    state
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JetEventResult {
    Handled,
    Ignored,
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetPaintCmd {
    FillRect { rect: JetRect, color: String },
    Text {
        rect: JetRect,
        text: String,
        style: Option<JetTuiStyle>,
    },
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
    _modifiers: u8,
) -> Option<JetEventResult> {
    if !code.eq_ignore_ascii_case("tab") || focus_nodes.is_empty() {
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

/// Portable backend seam between Jet UI and platform renderers.
pub trait JetBackend {
    fn measure(&mut self, node: &JetUiNode, constraint: JetSizeConstraint) -> JetSize;
    fn layout(&mut self, node: &JetUiNode, frame: JetRect);
    fn paint(&mut self, node: &JetUiNode);
    fn on_event(&mut self, event: JetInputEvent) -> JetEventResult;

    /// Rich TUI events stay typed through the backend seam. Legacy backends
    /// still receive the portable key/resize subset, while TUI adapters may
    /// inspect modifiers and non-terminal events before choosing a result.
    fn on_tui_event(&mut self, event: JetTuiEvent) -> JetEventResult {
        match event {
            JetTuiEvent::Key { code, .. } => self.on_event(JetInputEvent::Key { code }),
            JetTuiEvent::Resize { size } => self.on_event(JetInputEvent::Resize { size }),
            JetTuiEvent::Timer { .. }
            | JetTuiEvent::Io { .. }
            | JetTuiEvent::Focus { .. }
            | JetTuiEvent::Interrupt
            | JetTuiEvent::Close => JetEventResult::Handled,
        }
    }
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
            style: leaf.style.clone(),
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
                JetPaintCmd::Text { rect, text, .. } => {
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
                jet_ui_advance_focus(&self.focus_nodes, &mut self.focused_index, code, 0)
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

// ── TUI backend — deterministic character-grid output ───────────────────────────

struct JetTuiBackendState {
    measured: Option<JetSize>,
    layout_frame: Option<JetRect>,
    grid: Vec<Vec<char>>,
    grid_cols: usize,
    grid_rows: usize,
    render_count: usize,
    last_event: Option<JetEventResult>,
    capabilities: JetTuiCapabilities,
    /// Keep the semantic tree so direct resize events can repaint without
    /// asking a platform backend to invent a second view representation.
    last_node: Option<JetUiNode>,
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

fn tui_write_label(
    grid: &mut [Vec<char>],
    frame: &JetRect,
    label: &str,
    style: Option<&JetTuiStyle>,
    capabilities: &JetTuiCapabilities,
) {
    let (cols, rows) = (grid.first().map(|r| r.len()).unwrap_or(0), grid.len());
    let start_col = frame.x.max(0.0) as usize;
    let start_row = frame.y.max(0.0) as usize;
    let rendered = style
        .map(|style| style.render(label, capabilities))
        .unwrap_or_else(|| label.to_string());
    let label = if capabilities.unicode {
        jet_tui_strip_ansi(&rendered)
    } else {
        jet_tui_ascii(&jet_tui_strip_ansi(&rendered))
    };
    let mut col = start_col;
    for ch in label.chars() {
        let width = jet_tui_char_width(ch);
        if width == 0 {
            continue;
        }
        if col < cols && start_row < rows {
            grid[start_row][col] = ch;
            if width == 2 && col + 1 < cols {
                grid[start_row][col + 1] = ' ';
            }
        }
        col = col.saturating_add(width);
        if col >= cols {
            break;
        }
    }
}

#[derive(Clone)]
pub struct JetTuiBackend {
    state: std::sync::Arc<std::sync::Mutex<JetTuiBackendState>>,
}
impl JetTuiBackend {
    pub fn new() -> Self {
        Self::with_capabilities(jet_tui_capabilities())
    }

    pub fn with_capabilities(capabilities: JetTuiCapabilities) -> Self {
        JetTuiBackend {
            state: std::sync::Arc::new(std::sync::Mutex::new(JetTuiBackendState {
                measured: None,
                layout_frame: None,
                grid: Vec::new(),
                grid_cols: 0,
                grid_rows: 0,
                render_count: 0,
                last_event: None,
                capabilities,
                last_node: None,
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
    pub fn capabilities(&self) -> JetTuiCapabilities {
        self.state.lock().unwrap().capabilities.clone()
    }

    pub fn set_capabilities(&self, capabilities: JetTuiCapabilities) {
        self.state.lock().unwrap().capabilities = capabilities;
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
        self.last_node = Some(node.clone());
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
        let capabilities = self.capabilities.clone();
        jet_ui_visit_tree(node, frame, &mut |leaf, leaf_frame| {
            tui_write_label(
                &mut self.grid,
                &leaf_frame,
                &leaf.label,
                leaf.style.as_ref(),
                &capabilities,
            );
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
                jet_ui_advance_focus(&self.focus_nodes, &mut self.focused_index, code, 0)
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
    fn on_tui_event(&mut self, event: JetTuiEvent) -> JetEventResult {
        match event {
            JetTuiEvent::Key { code, modifiers } => {
                if let Some(result) = jet_ui_advance_focus(
                    &self.focus_nodes,
                    &mut self.focused_index,
                    &code,
                    modifiers,
                ) {
                    self.last_event = Some(result);
                    return result;
                }
                let result = if code.is_empty() {
                    JetEventResult::Ignored
                } else {
                    JetEventResult::Handled
                };
                self.last_event = Some(result);
                result
            }
            JetTuiEvent::Resize { size } => {
                let result = self.on_event(JetInputEvent::Resize { size });
                if result == JetEventResult::Handled {
                    if let Some(node) = self.last_node.clone() {
                        let frame = self.layout_frame.unwrap_or(JetRect {
                            x: 0.0,
                            y: 0.0,
                            width: size.width,
                            height: size.height,
                        });
                        self.layout(&node, frame);
                        self.paint(&node);
                    }
                }
                result
            }
            event => {
                let result = JetBackend::on_tui_event(self, event);
                self.last_event = Some(result);
                result
            }
        }
    }
}
impl JetTuiRenderTarget for JetNullBackend {
    fn mount_tui(&self, node: JetUiNode, constraint: JetSizeConstraint) {
        self.mount_node(node, constraint);
    }

    fn dispatch_tui(&self, event: JetInputEvent) -> JetEventResult {
        self.dispatch_event(event)
    }
    fn dispatch_tui_event(&self, event: JetTuiEvent) -> JetEventResult {
        let mut state = self.state.lock().unwrap();
        JetBackend::on_tui_event(&mut *state, event)
    }

    fn frame_lines_tui(&self) -> Vec<String> {
        self.paint_commands()
    }
}

impl JetTuiRenderTarget for JetTuiBackend {
    fn mount_tui(&self, node: JetUiNode, constraint: JetSizeConstraint) {
        self.mount_node(node, constraint);
    }

    fn dispatch_tui(&self, event: JetInputEvent) -> JetEventResult {
        self.dispatch_event(event)
    }
    fn dispatch_tui_event(&self, event: JetTuiEvent) -> JetEventResult {
        let mut state = self.state.lock().unwrap();
        JetBackend::on_tui_event(&mut *state, event)
    }

    fn frame_lines_tui(&self) -> Vec<String> {
        self.frame_lines()
    }
}

/// Reactive UI render loop — re-runs the body when signals change.
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
        accessibility: None,
        ime: None,
        color: None,
        style: None,
        kind: JetUiNodeKind::Custom,
        children: Vec::new(),
        key: None,
        on_click: None,
        on_drop: None,
        shortcut: None,
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
        accessibility: None,
        ime: None,
        color: None,
        style: None,
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
        on_drop: None,
        shortcut: None,
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
        accessibility: None,
        ime: None,
        color: Some(color.to_string()),
        style: None,
        kind: JetUiNodeKind::Custom,
        children: Vec::new(),
        key: None,
        on_click: None,
        on_drop: None,
        shortcut: None,
    }
}

pub fn jet_ui_node_style(mut node: JetUiNode, style: JetTuiStyle) -> JetUiNode {
    node.style = Some(style);
    node
}

/// D-UITREE1=A: canonical typed beginner constructors. Component-kit source,
/// native, web, and TUI all hand this exact tree to `JetBackend`.
pub fn jet_ui_text(text: &str) -> JetUiNode {
    JetUiNode {
        label: text.to_string(),
        width: jet_tui_display_width(text) as f64,
        height: 1.0,
        role: Some(JetAriaRole::Label),
        accessibility: None,
        ime: None,
        color: None,
        style: None,
        kind: JetUiNodeKind::Text,
        children: Vec::new(),
        key: None,
        on_click: None,
        on_drop: None,
        shortcut: None,
    }
}

pub fn jet_ui_button(label: &str) -> JetUiNode {
    JetUiNode {
        label: label.to_string(),
        width: jet_tui_display_width(label) as f64 + 4.0,
        height: 1.0,
        role: Some(JetAriaRole::Button),
        accessibility: None,
        ime: None,
        color: None,
        style: None,
        kind: JetUiNodeKind::Button,
        children: Vec::new(),
        key: None,
        on_click: None,
        on_drop: None,
        shortcut: None,
    }
}

/// D-UI-CLOSURE1=A: shared constructor facts for button metadata. The
/// callback-taking route and interpreter adapter both use this before adding
/// their tier-local callback carrier.
pub fn jet_ui_button_with_metadata(
    label: &str,
    shortcut: JetOutcome<JetUiShortcut, JetAbsent>,
    accessible_label: JetOutcome<String, JetAbsent>,
) -> JetUiNode {
    let mut node = jet_ui_button(label);
    node.shortcut = shortcut.ok();
    if let Ok(name) = accessible_label {
        node.accessibility = Some(JetUiAccessibility {
            name: Some(name),
            description: None,
            states: Vec::new(),
        });
    }
    node
}

/// D-UI-CLOSURE1=A: fixed `(display, shortcut, accessible_label, closure)`
/// callback ABI. Optional metadata uses the shared `JetOutcome` carrier, so
/// omitted labels are represented by `Err(JetAbsent)` before this seam.
pub fn jet_ui_button_on_click<F: Fn() + Send + Sync + 'static>(
    label: &str,
    shortcut: JetOutcome<JetUiShortcut, JetAbsent>,
    accessible_label: JetOutcome<String, JetAbsent>,
    handler: F,
) -> JetUiNode {
    let mut node = jet_ui_button_with_metadata(label, shortcut, accessible_label);
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
        accessibility: None,
        ime: None,
        color: None,
        style: None,
        kind: JetUiNodeKind::Box,
        children,
        key: None,
        on_click: None,
        on_drop: None,
        shortcut: None,
    }
}

/// Resolve the stable identity used by host projections and event bindings.
/// Author keys take precedence over render paths, matching every backend's
/// canonical tree traversal.
pub fn jet_ui_node_id(
    node: &JetUiNode,
    path: &str,
) -> Result<JetUiNodeId, JetUiHostError> {
    let identity = match &node.key {
        Some(key) if !key.is_empty() => format!("key:{key}"),
        _ => path.to_string(),
    };
    JetUiNodeId::new(&identity)
}

// ── D-UI-EVT-DISP1=E: O(1) node-keyed click slots ───────────────────────────

type JetUiClickHandler = std::sync::Arc<dyn Fn() + Send + Sync + 'static>;

fn jet_ui_click_slots() -> &'static std::sync::Mutex<std::collections::HashMap<i64, JetUiClickHandler>>
{
    static SLOTS: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<i64, JetUiClickHandler>>,
    > = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    &SLOTS
}

fn jet_ui_click_bindings()
-> &'static std::sync::Mutex<std::collections::HashMap<JetUiNodeId, i64>> {
    static BINDINGS: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<JetUiNodeId, i64>>,
    > = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    &BINDINGS
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

/// Bind a typed node identity to a handler slot (paint/mount).
pub fn jet_ui_bind_click(identity: JetUiNodeId, slot: i64) {
    jet_ui_click_bindings()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(identity, slot);
}

/// Clear the handler binding for an unmounted identity.
pub fn jet_ui_unbind_click(identity: &JetUiNodeId) {
    jet_ui_click_bindings()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(identity);
}

/// D-UI-EVT-DISP1=E: O(1) dispatch by stable typed node identity.
pub fn jet_ui_dispatch(identity: &JetUiNodeId) {
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
// ── D-UI-DROP1=A: O(1) node-keyed drop slots ─────────────────────────────────

type JetUiDropHandler = std::sync::Arc<dyn Fn(Vec<JetUiDropItem>) + Send + Sync + 'static>;

fn jet_ui_drop_slots()
-> &'static std::sync::Mutex<std::collections::HashMap<i64, JetUiDropHandler>> {
    static SLOTS: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<i64, JetUiDropHandler>>,
    > = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    &SLOTS
}

fn jet_ui_drop_bindings()
-> &'static std::sync::Mutex<std::collections::HashMap<JetUiNodeId, i64>> {
    static BINDINGS: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<JetUiNodeId, i64>>,
    > = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    &BINDINGS
}

fn jet_ui_next_drop_id() -> i64 {
    static NEXT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

pub fn jet_ui_register_drop<F: Fn(Vec<JetUiDropItem>) + Send + Sync + 'static>(
    handler: F,
) -> i64 {
    let id = jet_ui_next_drop_id();
    jet_ui_drop_slots()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, std::sync::Arc::new(handler));
    id
}

pub fn jet_ui_bind_drop(identity: JetUiNodeId, slot: i64) {
    jet_ui_drop_bindings()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(identity, slot);
}

pub fn jet_ui_unbind_drop(identity: &JetUiNodeId) {
    jet_ui_drop_bindings()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(identity);
}

pub fn jet_ui_dispatch_drop(identity: &JetUiNodeId, items: Vec<JetUiDropItem>) {
    let slot = jet_ui_drop_bindings()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(identity)
        .copied();
    let Some(slot) = slot else {
        return;
    };
    let handler = jet_ui_drop_slots()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&slot)
        .cloned();
    if let Some(handler) = handler {
        handler(items);
    }
}


/// Walk a freshly painted tree and bind each event slot to its identity.
pub fn jet_ui_bind_tree_clicks(node: &JetUiNode, path: &str) {
    let Ok(identity) = jet_ui_node_id(node, path) else {
        return;
    };
    if let Some(slot) = node.on_click {
        jet_ui_bind_click(identity.clone(), slot);
    }
    if let Some(slot) = node.on_drop {
        jet_ui_bind_drop(identity, slot);
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
mod ui_backend_tests {
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
        let button = jet_ui_button_on_click("Go", Err(JetAbsent), Err(JetAbsent), || {
            HITS.fetch_add(1, Ordering::SeqCst);
        });
        let tree = jet_ui_box(vec![button]);
        backend.mount_node(
            tree,
            jet_ui_constraint(0.0, 0.0, DEFAULT_MOUNT_COLS, DEFAULT_MOUNT_ROWS),
        );
        let button_id = JetUiNodeId::new("null#0/0").unwrap();
        jet_ui_dispatch(&button_id);
        assert_eq!(HITS.load(Ordering::SeqCst), 1);
        let missing_id = JetUiNodeId::new("missing").unwrap();
        jet_ui_dispatch(&missing_id);
        assert_eq!(HITS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn accessibility_attachment_projects_by_stable_identity() {
        let mut node = jet_ui_button("Save");
        assert_eq!(node.accessibility, None);
        assert_eq!(node.role, Some(JetAriaRole::Button));

        let metadata = JetUiAccessibility::new(Some("Save note"), Some("Writes the current note"))
            .unwrap()
            .with_state(JetUiAccessibilityState::Disabled);
        node.attach_jet_ui_accessibility(metadata.clone()).unwrap();

        let node_id = JetUiNodeId::new("key:save").unwrap();
        let first = jet_ui_accessibility_project(&node, node_id.clone()).unwrap();
        let second = jet_ui_accessibility_project(&node, node_id.clone()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.node(), &node_id);
        assert_eq!(first.metadata(), &metadata);
    }
}
