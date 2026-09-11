// D-DX-GAMEOVERLAY1 / card #2485: one deterministic, typed state kernel for
// an in-game console and stat overlay. The module owns facts and transitions
// only. It does not render, call a native API, or own the devtools envelope.
//
// Output rows carry ordinary, explicitly supplied console text as their
// category/message facts. A protocol payload is a separate, private option and
// can only be populated through `publish_text`; absent publication serializes
// as `null`. Hosts can therefore project the same state without discovering
// values in arbitrary runtime objects.

pub const JET_GAME_OVERLAY_MAX_COMMANDS: usize = 256;
pub const JET_GAME_OVERLAY_MAX_HISTORY: usize = 256;
pub const JET_GAME_OVERLAY_MAX_TRANSITIONS: usize = 256;
pub const JET_GAME_OVERLAY_MAX_SELECTED_METRICS: usize = 8;
pub const JET_GAME_OVERLAY_MAX_METRIC_VALUES: usize = 16;
pub const JET_GAME_OVERLAY_MAX_TEXT_BYTES: usize = 16 * 1024;
pub const JET_GAME_OVERLAY_MAX_AUTOCOMPLETE_RESULTS: usize = 64;

/// Autocomplete is a typed projection, not an execution request. The command
/// list is supplied by the producer so the overlay never evaluates arbitrary
/// text or discovers commands in runtime objects.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameOverlayAutocompleteFact {
    pub query: String,
    pub matches: Vec<String>,
}

impl JetGameOverlayAutocompleteFact {
    pub fn new(
        query: impl Into<String>,
        mut matches: Vec<String>,
    ) -> Result<Self, JetGameOverlayError> {
        let query = query.into();
        jet_game_overlay_validate_text(&query, "autocomplete query", false)?;
        matches.sort();
        matches.dedup();
        matches.truncate(JET_GAME_OVERLAY_MAX_AUTOCOMPLETE_RESULTS);
        for command in &matches {
            jet_game_overlay_validate_text(command, "autocomplete command", true)?;
        }
        Ok(Self { query, matches })
    }

    pub fn render_json(&self) -> String {
        let matches = self
            .matches
            .iter()
            .map(|value| jet_game_overlay_json_string(value))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"query\":{},\"matches\":[{}]}}",
            jet_game_overlay_json_string(&self.query),
            matches,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum JetGameOverlayOutputCategory {
    Console,
    Info,
    Debug,
    Warning,
    Error,
    Stat,
}

impl JetGameOverlayOutputCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Console => "console",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Stat => "stat",
        }
    }

    pub const fn all() -> [Self; 6] {
        [
            Self::Console,
            Self::Info,
            Self::Debug,
            Self::Warning,
            Self::Error,
            Self::Stat,
        ]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetGameOverlayFilter {
    All,
    Category(JetGameOverlayOutputCategory),
}

impl JetGameOverlayFilter {
    pub fn matches(self, category: JetGameOverlayOutputCategory) -> bool {
        match self {
            Self::All => true,
            Self::Category(selected) => selected == category,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Category(category) => category.as_str(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetGameOverlayInputCapture {
    None,
    Overlay,
}

impl JetGameOverlayInputCapture {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Overlay => "overlay",
        }
    }

    pub const fn captures_input(self) -> bool {
        matches!(self, Self::Overlay)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetGameOverlayVisibility {
    pub visible: bool,
    pub input_capture: JetGameOverlayInputCapture,
}

impl JetGameOverlayVisibility {
    pub const fn hidden() -> Self {
        Self {
            visible: false,
            input_capture: JetGameOverlayInputCapture::None,
        }
    }

    pub const fn shown() -> Self {
        Self {
            visible: true,
            input_capture: JetGameOverlayInputCapture::Overlay,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetGameOverlayRunState {
    Running,
    Paused,
    ErrorPaused,
}

impl JetGameOverlayRunState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::ErrorPaused => "error_paused",
        }
    }
}

/// The fixed stat vocabulary keeps selected metrics typed and deterministic.
/// `FrameTime` is an integer value supplied by the caller (normally ns); units
/// are owned by the metric name rather than inferred from a floating payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum JetGameOverlayMetric {
    Fps,
    FrameTime,
    GameTime,
    GpuTime,
    DrawCalls,
    Entities,
    SceneAssetBytes,
    MemoryHighWater,
    Unit,
}

impl JetGameOverlayMetric {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fps => "Fps",
            Self::FrameTime => "FrameTime",
            Self::GameTime => "GameTime",
            Self::GpuTime => "GpuTime",
            Self::DrawCalls => "DrawCalls",
            Self::Entities => "Entities",
            Self::SceneAssetBytes => "SceneAssetBytes",
            Self::MemoryHighWater => "MemoryHighWater",
            Self::Unit => "Unit",
        }
    }

    pub const fn all() -> [Self; 9] {
        [
            Self::Fps,
            Self::FrameTime,
            Self::GameTime,
            Self::GpuTime,
            Self::DrawCalls,
            Self::Entities,
            Self::SceneAssetBytes,
            Self::MemoryHighWater,
            Self::Unit,
        ]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetGameOverlayMetricValue {
    pub metric: JetGameOverlayMetric,
    pub value: i64,
    pub sampled_at_ns: u64,
}

impl JetGameOverlayMetricValue {
    pub const fn new(metric: JetGameOverlayMetric, value: i64, sampled_at_ns: u64) -> Self {
        Self {
            metric,
            value,
            sampled_at_ns,
        }
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"metric\":{},\"value\":{},\"sampled_at_ns\":{}}}",
            jet_game_overlay_json_string(self.metric.as_str()),
            self.value,
            self.sampled_at_ns,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetGameOverlayError {
    EmptyText { field: &'static str },
    EmptyIdentity { field: &'static str },
    TextTooLong {
        field: &'static str,
        max: usize,
        actual: usize,
    },
    ControlIdentity { field: &'static str },
    MetricCapacity { max: usize },
}

impl JetGameOverlayError {
    pub fn message(&self) -> String {
        match self {
            Self::EmptyText { field } => format!("game overlay {field} must not be empty"),
            Self::EmptyIdentity { field } => {
                format!("game overlay {field} identity must not be empty")
            }
            Self::TextTooLong { field, max, actual } => format!(
                "game overlay {field} is {actual} bytes; maximum is {max} bytes"
            ),
            Self::ControlIdentity { field } => {
                format!("game overlay {field} identity contains a control character")
            }
            Self::MetricCapacity { max } => {
                format!("game overlay selected metric limit is {max}")
            }
        }
    }
}

fn jet_game_overlay_validate_text(
    value: &str,
    field: &'static str,
    require_nonempty: bool,
) -> Result<(), JetGameOverlayError> {
    if require_nonempty && value.is_empty() {
        return Err(JetGameOverlayError::EmptyText { field });
    }
    if value.len() > JET_GAME_OVERLAY_MAX_TEXT_BYTES {
        return Err(JetGameOverlayError::TextTooLong {
            field,
            max: JET_GAME_OVERLAY_MAX_TEXT_BYTES,
            actual: value.len(),
        });
    }
    Ok(())
}

fn jet_game_overlay_validate_identity(
    value: &str,
    field: &'static str,
) -> Result<(), JetGameOverlayError> {
    if value.is_empty() {
        return Err(JetGameOverlayError::EmptyIdentity { field });
    }
    jet_game_overlay_validate_text(value, field, true)?;
    if value.chars().any(char::is_control) {
        return Err(JetGameOverlayError::ControlIdentity { field });
    }
    Ok(())
}

/// Exact source/trace provenance captured when the overlay enters Error Pause.
/// IDs are never normalized or truncated: a host can link the fact directly to
/// the source and trace stores that produced it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameOverlayErrorPause {
    pub source_id: String,
    pub trace_id: String,
}

impl JetGameOverlayErrorPause {
    pub fn new(
        source_id: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Result<Self, JetGameOverlayError> {
        let source_id = source_id.into();
        let trace_id = trace_id.into();
        jet_game_overlay_validate_identity(&source_id, "source_id")?;
        jet_game_overlay_validate_identity(&trace_id, "trace_id")?;
        Ok(Self { source_id, trace_id })
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"source_id\":{},\"trace_id\":{}}}",
            jet_game_overlay_json_string(&self.source_id),
            jet_game_overlay_json_string(&self.trace_id),
        )
    }
}

/// One bounded command-history row. A row is a submitted command fact; it is
/// not an execution result and carries no implicit runtime value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameOverlayCommandRow {
    pub id: u64,
    pub timestamp_ns: u64,
    pub text: String,
}

impl JetGameOverlayCommandRow {
    pub fn new(
        timestamp_ns: u64,
        text: impl Into<String>,
    ) -> Result<Self, JetGameOverlayError> {
        let text = text.into();
        jet_game_overlay_validate_text(&text, "command", true)?;
        Ok(Self {
            id: 0,
            timestamp_ns,
            text,
        })
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"id\":{},\"timestamp_ns\":{},\"text\":{}}}",
            self.id,
            self.timestamp_ns,
            jet_game_overlay_json_string(&self.text),
        )
    }
}

/// One output/history row. The visible message and category are ordinary
/// output facts. `published_text` remains absent unless `publish_text` is
/// called explicitly, so host adapters cannot accidentally expose payload data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameOverlayOutputRow {
    pub id: u64,
    pub timestamp_ns: u64,
    pub category: JetGameOverlayOutputCategory,
    pub message: String,
    pub source_id: Option<String>,
    pub trace_id: Option<String>,
    published_text: Option<String>,
}

impl JetGameOverlayOutputRow {
    pub fn new(
        timestamp_ns: u64,
        category: JetGameOverlayOutputCategory,
        message: impl Into<String>,
    ) -> Result<Self, JetGameOverlayError> {
        let message = message.into();
        jet_game_overlay_validate_text(&message, "message", false)?;
        Ok(Self {
            id: 0,
            timestamp_ns,
            category,
            message,
            source_id: None,
            trace_id: None,
            published_text: None,
        })
    }

    pub fn with_identity(
        mut self,
        source_id: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Result<Self, JetGameOverlayError> {
        let identity = JetGameOverlayErrorPause::new(source_id, trace_id)?;
        self.source_id = Some(identity.source_id);
        self.trace_id = Some(identity.trace_id);
        Ok(self)
    }

    /// Explicit publication gate for payload text. The ordinary message is
    /// never promoted implicitly and no arbitrary JSON value can enter here.
    pub fn publish_text(mut self, text: impl Into<String>) -> Result<Self, JetGameOverlayError> {
        let text = text.into();
        jet_game_overlay_validate_text(&text, "published_text", false)?;
        self.published_text = Some(text);
        Ok(self)
    }

    pub fn published_text(&self) -> Option<&str> {
        self.published_text.as_deref()
    }

    pub fn payload_json(&self) -> Option<String> {
        self.published_text
            .as_deref()
            .map(jet_game_overlay_json_string)
    }

    pub fn render_json(&self) -> String {
        let payload = self
            .payload_json()
            .unwrap_or_else(|| "null".to_string());
        let source_id = self
            .source_id
            .as_deref()
            .map(jet_game_overlay_json_string)
            .unwrap_or_else(|| "null".to_string());
        let trace_id = self
            .trace_id
            .as_deref()
            .map(jet_game_overlay_json_string)
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"id\":{},\"timestamp_ns\":{},\"category\":{},\"message\":{},\"source_id\":{},\"trace_id\":{},\"payload\":{}}}",
            self.id,
            self.timestamp_ns,
            jet_game_overlay_json_string(self.category.as_str()),
            jet_game_overlay_json_string(&self.message),
            source_id,
            trace_id,
            payload,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetGameOverlayTransitionKind {
    VisibilityChanged,
    InputCaptureChanged,
    FilterChanged,
    Paused,
    Resumed,
    ErrorPauseChanged,
    ErrorPaused,
    MetricSelected,
    MetricDeselected,
}

impl JetGameOverlayTransitionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VisibilityChanged => "visibility_changed",
            Self::InputCaptureChanged => "input_capture_changed",
            Self::FilterChanged => "filter_changed",
            Self::Paused => "paused",
            Self::Resumed => "resumed",
            Self::ErrorPauseChanged => "error_pause_changed",
            Self::ErrorPaused => "error_paused",
            Self::MetricSelected => "metric_selected",
            Self::MetricDeselected => "metric_deselected",
        }
    }
}

/// A bounded transition receipt. `before` and `after` make no-op commands
/// observable without asking a host to infer state from a control message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameOverlayTransitionFact {
    pub sequence: u64,
    pub kind: JetGameOverlayTransitionKind,
    pub before: JetGameOverlayRunState,
    pub after: JetGameOverlayRunState,
    pub visibility: JetGameOverlayVisibility,
    pub filter: JetGameOverlayFilter,
    pub metric: Option<JetGameOverlayMetric>,
    pub error_pause_enabled: bool,
    pub error_pause: Option<JetGameOverlayErrorPause>,
    pub frame: Option<JetGameFrameIdentity>,
}

impl JetGameOverlayTransitionFact {
    pub fn render_json(&self) -> String {
        let metric = self
            .metric
            .map(|metric| jet_game_overlay_json_string(metric.as_str()))
            .unwrap_or_else(|| "null".to_string());
        let error_pause = self
            .error_pause
            .as_ref()
            .map(JetGameOverlayErrorPause::render_json)
            .unwrap_or_else(|| "null".to_string());
        let frame = self
            .frame
            .as_ref()
            .map(jet_game_overlay_frame_json)
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"sequence\":{},\"kind\":{},\"before\":{},\"after\":{},\"visible\":{},\"input_capture\":{},\"filter\":{},\"metric\":{},\"error_pause_enabled\":{},\"error_pause\":{},\"frame\":{}}}",
            self.sequence,
            jet_game_overlay_json_string(self.kind.as_str()),
            jet_game_overlay_json_string(self.before.as_str()),
            jet_game_overlay_json_string(self.after.as_str()),
            self.visibility.visible,
            jet_game_overlay_json_string(self.visibility.input_capture.as_str()),
            jet_game_overlay_json_string(self.filter.as_str()),
            metric,
            self.error_pause_enabled,
            error_pause,
            frame,
        )
    }
}

/// The complete deterministic overlay model. All ordering comes from caller
/// insertion order and monotonic local ids; no clock, renderer, thread, or
/// native backend is consulted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameOverlayState {
    visible: bool,
    input_capture: JetGameOverlayInputCapture,
    filter: JetGameOverlayFilter,
    run_state: JetGameOverlayRunState,
    error_pause_enabled: bool,
    error_pause: Option<JetGameOverlayErrorPause>,
    frame: Option<JetGameFrameIdentity>,
    selected_metrics: Vec<JetGameOverlayMetric>,
    metric_values: Vec<JetGameOverlayMetricValue>,
    commands: std::collections::VecDeque<JetGameOverlayCommandRow>,
    history: std::collections::VecDeque<JetGameOverlayOutputRow>,
    transitions: std::collections::VecDeque<JetGameOverlayTransitionFact>,
    next_command_id: u64,
    next_history_id: u64,
    next_transition_sequence: u64,
}


impl Default for JetGameOverlayState {
    fn default() -> Self {
        Self::new()
    }
}

impl JetGameOverlayState {
    pub fn new() -> Self {
        Self {
            visible: false,
            input_capture: JetGameOverlayInputCapture::None,
            filter: JetGameOverlayFilter::All,
            run_state: JetGameOverlayRunState::Running,
            error_pause_enabled: false,
            error_pause: None,
            frame: None,
            selected_metrics: Vec::new(),
            metric_values: Vec::new(),
            commands: std::collections::VecDeque::new(),
            history: std::collections::VecDeque::new(),
            transitions: std::collections::VecDeque::new(),
            next_command_id: 1,
            next_history_id: 1,
            next_transition_sequence: 1,
        }
    }

    pub const fn visibility(&self) -> JetGameOverlayVisibility {
        JetGameOverlayVisibility {
            visible: self.visible,
            input_capture: self.input_capture,
        }
    }

    pub const fn is_visible(&self) -> bool {
        self.visible
    }

    pub const fn input_capture(&self) -> JetGameOverlayInputCapture {
        self.input_capture
    }

    pub const fn captures_input(&self) -> bool {
        self.input_capture.captures_input()
    }

    pub const fn filter(&self) -> JetGameOverlayFilter {
        self.filter
    }

    pub const fn run_state(&self) -> JetGameOverlayRunState {
        self.run_state
    }

    pub fn active_error_pause(&self) -> Option<&JetGameOverlayErrorPause> {
        self.error_pause.as_ref()
    }

    pub const fn error_pause_enabled(&self) -> bool {
        self.error_pause_enabled
    }

    pub fn frame_identity(&self) -> Option<&JetGameFrameIdentity> {
        self.frame.as_ref()
    }

    /// The current frame identity is the overlay's target identity. It is
    /// updated by the game session whenever a frame fact is accepted.
    pub fn set_frame_identity(&mut self, frame: JetGameFrameIdentity) {
        self.frame = Some(frame);
    }

    pub fn selected_metrics(&self) -> impl Iterator<Item = &JetGameOverlayMetric> + '_ {
        self.selected_metrics.iter()
    }

    pub fn metric_values(&self) -> impl Iterator<Item = &JetGameOverlayMetricValue> + '_ {
        self.metric_values.iter()
    }

    pub fn selected_metric_values(&self) -> impl Iterator<Item = &JetGameOverlayMetricValue> + '_ {
        self.selected_metrics.iter().filter_map(|metric| {
            self.metric_values
                .iter()
                .find(|value| value.metric == *metric)
        })
    }

    pub fn metric_value(&self, metric: JetGameOverlayMetric) -> Option<&JetGameOverlayMetricValue> {
        self.metric_values
            .iter()
            .find(|value| value.metric == metric)
    }

    pub fn is_metric_selected(&self, metric: JetGameOverlayMetric) -> bool {
        self.selected_metrics.contains(&metric)
    }

    pub fn commands(&self) -> impl Iterator<Item = &JetGameOverlayCommandRow> + '_ {
        self.commands.iter()
    }

    pub fn history(&self) -> impl Iterator<Item = &JetGameOverlayOutputRow> + '_ {
        self.history.iter()
    }

    pub fn visible_history(&self) -> impl Iterator<Item = &JetGameOverlayOutputRow> + '_ {
        self.history
            .iter()
            .filter(|row| self.filter.matches(row.category))
    }

    pub fn transitions(&self) -> impl Iterator<Item = &JetGameOverlayTransitionFact> + '_ {
        self.transitions.iter()
    }

    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    pub fn history_count(&self) -> usize {
        self.history.len()
    }

    pub fn transition_count(&self) -> usize {
        self.transitions.len()
    }

    /// Toggle visibility. Showing the overlay captures input; hiding it always
    /// releases input so a stale capture cannot survive a toggle.
    pub fn toggle(&mut self) -> JetGameOverlayTransitionFact {
        self.set_visible(!self.visible)
    }

    pub fn set_visible(&mut self, visible: bool) -> JetGameOverlayTransitionFact {
        let before = self.run_state;
        self.visible = visible;
        self.input_capture = if visible {
            JetGameOverlayInputCapture::Overlay
        } else {
            JetGameOverlayInputCapture::None
        };
        self.push_transition(
            JetGameOverlayTransitionKind::VisibilityChanged,
            before,
            None,
        )
    }

    /// Change input focus without changing visibility. Hidden overlays cannot
    /// capture input, so the invariant is enforced at this one state boundary.
    pub fn set_input_capture(&mut self, capture: bool) -> JetGameOverlayTransitionFact {
        let before = self.run_state;
        self.input_capture = if self.visible && capture {
            JetGameOverlayInputCapture::Overlay
        } else {
            JetGameOverlayInputCapture::None
        };
        self.push_transition(
            JetGameOverlayTransitionKind::InputCaptureChanged,
            before,
            None,
        )
    }

    pub fn set_filter(&mut self, filter: JetGameOverlayFilter) -> JetGameOverlayTransitionFact {
        let before = self.run_state;
        self.filter = filter;
        self.push_transition(JetGameOverlayTransitionKind::FilterChanged, before, None)
    }

    /// Selecting the same category again returns to the all-output view.
    pub fn toggle_filter(
        &mut self,
        category: JetGameOverlayOutputCategory,
    ) -> JetGameOverlayTransitionFact {
        let filter = if self.filter == JetGameOverlayFilter::Category(category) {
            JetGameOverlayFilter::All
        } else {
            JetGameOverlayFilter::Category(category)
        };
        self.set_filter(filter)
    }

    pub fn pause(&mut self) -> JetGameOverlayTransitionFact {
        let before = self.run_state;
        if self.run_state == JetGameOverlayRunState::Running {
            self.run_state = JetGameOverlayRunState::Paused;
        }
        self.push_transition(JetGameOverlayTransitionKind::Paused, before, None)
    }

    pub fn resume(&mut self) -> JetGameOverlayTransitionFact {
        let before = self.run_state;
        self.run_state = JetGameOverlayRunState::Running;
        self.error_pause = None;
        self.push_transition(JetGameOverlayTransitionKind::Resumed, before, None)
    }
    /// Enable or disable automatic Error Pause handling. The setting remains
    /// visible in every transition so a host cannot infer policy from a
    /// diagnostic that may have arrived while the feature was disabled.
    pub fn set_error_pause_enabled(&mut self, enabled: bool) -> JetGameOverlayTransitionFact {
        let before = self.run_state;
        self.error_pause_enabled = enabled;
        self.push_transition(JetGameOverlayTransitionKind::ErrorPauseChanged, before, None)
    }

    /// Enter Error Pause and retain the exact source/trace identity in both
    /// current state and the transition receipt. Invalid identities do not
    /// mutate state or append a misleading transition.
    pub fn error_pause(
        &mut self,
        source_id: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Result<JetGameOverlayTransitionFact, JetGameOverlayError> {
        let pause = JetGameOverlayErrorPause::new(source_id, trace_id)?;
        let before = self.run_state;
        self.run_state = JetGameOverlayRunState::ErrorPaused;
        self.error_pause = Some(pause.clone());
        Ok(self.push_transition(
            JetGameOverlayTransitionKind::ErrorPaused,
            before,
            None,
        ))
    }

    pub fn select_metric(
        &mut self,
        metric: JetGameOverlayMetric,
    ) -> Result<JetGameOverlayTransitionFact, JetGameOverlayError> {
        let before = self.run_state;
        if !self.selected_metrics.contains(&metric) {
            if self.selected_metrics.len() == JET_GAME_OVERLAY_MAX_SELECTED_METRICS {
                return Err(JetGameOverlayError::MetricCapacity {
                    max: JET_GAME_OVERLAY_MAX_SELECTED_METRICS,
                });
            }
            self.selected_metrics.push(metric);
        }
        Ok(self.push_transition(
            JetGameOverlayTransitionKind::MetricSelected,
            before,
            Some(metric),
        ))
    }

    pub fn deselect_metric(
        &mut self,
        metric: JetGameOverlayMetric,
    ) -> JetGameOverlayTransitionFact {
        let before = self.run_state;
        self.selected_metrics.retain(|selected| *selected != metric);
        self.push_transition(
            JetGameOverlayTransitionKind::MetricDeselected,
            before,
            Some(metric),
        )
    }

    pub fn toggle_metric(
        &mut self,
        metric: JetGameOverlayMetric,
    ) -> Result<JetGameOverlayTransitionFact, JetGameOverlayError> {
        if self.is_metric_selected(metric) {
            Ok(self.deselect_metric(metric))
        } else {
            self.select_metric(metric)
        }
    }

    /// Record an integer metric sample. Values are retained even when their
    /// metric is not selected, allowing a later selection to reveal the latest
    /// deterministic sample without re-reading runtime state.
    pub fn observe_metric(
        &mut self,
        metric: JetGameOverlayMetric,
        value: i64,
        sampled_at_ns: u64,
    ) {
        let sample = JetGameOverlayMetricValue::new(metric, value, sampled_at_ns);
        if let Some(current) = self
            .metric_values
            .iter_mut()
            .find(|current| current.metric == metric)
        {
            *current = sample;
        } else if self.metric_values.len() < JET_GAME_OVERLAY_MAX_METRIC_VALUES {
            self.metric_values.push(sample);
        }
    }

    /// Return deterministic prefix matches for the console. Command discovery
    /// stays in the overlay kernel while execution remains a host concern.
    pub fn autocomplete(
        &self,
        query: impl Into<String>,
        commands: &[String],
    ) -> Result<JetGameOverlayAutocompleteFact, JetGameOverlayError> {
        let query = query.into();
        jet_game_overlay_validate_text(&query, "autocomplete query", false)?;
        let mut matches = commands
            .iter()
            .filter(|command| command.starts_with(&query))
            .cloned()
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        JetGameOverlayAutocompleteFact::new(query, matches)
    }

    pub fn record_command(
        &mut self,
        timestamp_ns: u64,
        text: impl Into<String>,
    ) -> Result<u64, JetGameOverlayError> {
        let mut row = JetGameOverlayCommandRow::new(timestamp_ns, text)?;
        row.id = self.next_command_id;
        self.next_command_id = self.next_command_id.saturating_add(1);
        if self.commands.len() == JET_GAME_OVERLAY_MAX_COMMANDS {
            self.commands.pop_front();
        }
        let id = row.id;
        self.commands.push_back(row);
        Ok(id)
    }

    pub fn record_output(&mut self, mut row: JetGameOverlayOutputRow) -> Result<u64, JetGameOverlayError> {
        jet_game_overlay_validate_text(&row.message, "message", false)?;
        match (&row.source_id, &row.trace_id) {
            (Some(source_id), Some(trace_id)) => {
                jet_game_overlay_validate_identity(source_id, "source_id")?;
                jet_game_overlay_validate_identity(trace_id, "trace_id")?;
            }
            (None, None) => {}
            _ => {
                return Err(JetGameOverlayError::EmptyIdentity {
                    field: "source_id/trace_id",
                });
            }
        }
        if let Some(payload) = row.published_text.as_deref() {
            jet_game_overlay_validate_text(payload, "published_text", false)?;
        }
        row.id = self.next_history_id;
        self.next_history_id = self.next_history_id.saturating_add(1);
        if self.history.len() == JET_GAME_OVERLAY_MAX_HISTORY {
            self.history.pop_front();
        }
        let id = row.id;
        self.history.push_back(row);
        Ok(id)
    }

    pub fn output(
        &mut self,
        timestamp_ns: u64,
        category: JetGameOverlayOutputCategory,
        message: impl Into<String>,
    ) -> Result<u64, JetGameOverlayError> {
        self.record_output(JetGameOverlayOutputRow::new(timestamp_ns, category, message)?)
    }

    /// Add one row with an explicitly published text payload. This is the only
    /// state-level path that attaches payload text to an output fact.
    pub fn publish_output(
        &mut self,
        timestamp_ns: u64,
        category: JetGameOverlayOutputCategory,
        message: impl Into<String>,
        published_text: impl Into<String>,
    ) -> Result<u64, JetGameOverlayError> {
        let row = JetGameOverlayOutputRow::new(timestamp_ns, category, message)?
            .publish_text(published_text)?;
        self.record_output(row)
    }

    fn push_transition(
        &mut self,
        kind: JetGameOverlayTransitionKind,
        before: JetGameOverlayRunState,
        metric: Option<JetGameOverlayMetric>,
    ) -> JetGameOverlayTransitionFact {
        let fact = JetGameOverlayTransitionFact {
            sequence: self.next_transition_sequence,
            kind,
            before,
            after: self.run_state,
            visibility: self.visibility(),
            filter: self.filter,
            metric,
            error_pause_enabled: self.error_pause_enabled,
            error_pause: self.error_pause.clone(),
            frame: self.frame.clone(),
        };
        self.next_transition_sequence = self.next_transition_sequence.saturating_add(1);
        if self.transitions.len() == JET_GAME_OVERLAY_MAX_TRANSITIONS {
            self.transitions.pop_front();
        }
        self.transitions.push_back(fact.clone());
        fact
    }

    /// Project only typed overlay facts. The outer `jet.devtools.v1` envelope
    /// remains the host boundary; this method never emits protocol/version
    /// fields or invents a second envelope.
    pub fn render_json(&self) -> String {
        let selected_metrics = self
            .selected_metrics
            .iter()
            .map(|metric| jet_game_overlay_json_string(metric.as_str()))
            .collect::<Vec<_>>()
            .join(",");
        let metric_values = self
            .metric_values
            .iter()
            .map(JetGameOverlayMetricValue::render_json)
            .collect::<Vec<_>>()
            .join(",");
        let commands = self
            .commands
            .iter()
            .map(JetGameOverlayCommandRow::render_json)
            .collect::<Vec<_>>()
            .join(",");
        let history = self
            .history
            .iter()
            .map(JetGameOverlayOutputRow::render_json)
            .collect::<Vec<_>>()
            .join(",");
        let transitions = self
            .transitions
            .iter()
            .map(JetGameOverlayTransitionFact::render_json)
            .collect::<Vec<_>>()
            .join(",");
        let error_pause = self
            .error_pause
            .as_ref()
            .map(JetGameOverlayErrorPause::render_json)
            .unwrap_or_else(|| "null".to_string());
        let frame = self
            .frame
            .as_ref()
            .map(jet_game_overlay_frame_json)
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"visible\":{},\"input_capture\":{},\"filter\":{},\"run_state\":{},\"error_pause_enabled\":{},\"error_pause\":{},\"frame\":{},\"selected_metrics\":[{}],\"metric_values\":[{}],\"commands\":[{}],\"history\":[{}],\"transitions\":[{}]}}",
            self.visible,
            jet_game_overlay_json_string(self.input_capture.as_str()),
            jet_game_overlay_json_string(self.filter.as_str()),
            jet_game_overlay_json_string(self.run_state.as_str()),
            self.error_pause_enabled,
            error_pause,
            frame,
            selected_metrics,
            metric_values,
            commands,
            history,
            transitions,
        )
    }

}
fn jet_game_overlay_frame_json(frame: &JetGameFrameIdentity) -> String {
    format!(
        "{{\"frame_id\":{},\"scene\":{},\"frame_index\":{},\"build\":{},\"revision\":{},\"trace_id\":{}}}",
        frame.frame_id,
        jet_game_overlay_json_string(&frame.scene),
        frame.frame_index,
        jet_game_overlay_json_string(&frame.build),
        jet_game_overlay_json_string(&frame.revision),
        jet_game_overlay_json_string(&frame.trace_id),
    )
}

fn jet_game_overlay_json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            character if character.is_control() => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod game_overlay_tests {
    use super::*;

    #[test]
    fn toggle_filter_and_pause_resume_are_deterministic() {
        let mut state = JetGameOverlayState::new();
        assert_eq!(state.visibility(), JetGameOverlayVisibility::hidden());
        let toggled = state.toggle();
        assert_eq!(toggled.sequence, 1);
        assert_eq!(state.visibility(), JetGameOverlayVisibility::shown());
        state.set_filter(JetGameOverlayFilter::Category(
            JetGameOverlayOutputCategory::Error,
        ));
        assert_eq!(state.filter(), JetGameOverlayFilter::Category(JetGameOverlayOutputCategory::Error));
        assert_eq!(state.pause().after, JetGameOverlayRunState::Paused);
        assert_eq!(state.resume().after, JetGameOverlayRunState::Running);
        assert_eq!(state.transition_count(), 4);
    }

    #[test]
    fn error_pause_preserves_exact_source_and_trace_identity() {
        let mut state = JetGameOverlayState::new();
        let source = "scene/player.jet:41:9";
        let trace = "trace/frame/00000042";
        let fact = state.error_pause(source, trace).expect("valid identity");
        assert_eq!(fact.kind, JetGameOverlayTransitionKind::ErrorPaused);
        assert_eq!(fact.after, JetGameOverlayRunState::ErrorPaused);
        assert_eq!(fact.error_pause.as_ref().unwrap().trace_id, trace);
        assert_eq!(state.active_error_pause().unwrap().trace_id, trace);
        assert_eq!(state.resume().after, JetGameOverlayRunState::Running);
        assert!(state.active_error_pause().is_none());
    }

    #[test]
    fn history_is_bounded_and_payload_requires_explicit_publication() {
        let mut state = JetGameOverlayState::new();
        let plain = JetGameOverlayOutputRow::new(
            1,
            JetGameOverlayOutputCategory::Info,
            "ordinary output",
        )
        .expect("valid output");
        assert!(plain.published_text().is_none());
        assert!(plain.render_json().contains("\"payload\":null"));
        state.record_output(plain).expect("record output");
        state
            .publish_output(
                2,
                JetGameOverlayOutputCategory::Stat,
                "fps",
                "60",
            )
            .expect("publish output");
        assert_eq!(state.history_count(), 2);
        assert!(state.history().nth(1).unwrap().published_text().is_some());

        for index in 0..(JET_GAME_OVERLAY_MAX_COMMANDS + 1) {
            state
                .record_command(index as u64, format!("command-{index}"))
                .expect("record command");
        }
        assert_eq!(state.command_count(), JET_GAME_OVERLAY_MAX_COMMANDS);
        assert_eq!(state.commands().next().unwrap().text, "command-1");
    }
}
