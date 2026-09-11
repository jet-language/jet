// core.game development messages are typed views over the shared game facts.
//
// The Devtools envelope owns transport, protocol/version, event sequencing,
// and serialization. The frame profiler owns frame identity and bounded
// history. The overlay, world inspector, and hot-swap modules own their state
// transitions. This adapter only groups those values for CoreLib callers.

/// Target-neutral game development policy. Hosts fold their actual release
/// policy into this carrier before constructing a session; generated AOT code
/// supplies the same value through `JET_GAME_DEBUG_DATA_ENABLED`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GameDevPolicy {
    pub debug_data_enabled: bool,
}

impl GameDevPolicy {
    pub const fn new(debug_data_enabled: bool) -> Self {
        Self { debug_data_enabled }
    }

    pub const fn game_debug_data_enabled(self) -> bool {
        self.debug_data_enabled
    }
}

impl Default for GameDevPolicy {
    fn default() -> Self {
        Self::new(false)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameDevStopped {
    pub reason: String,
    pub frame_index: u64,
}

impl GameDevStopped {
    pub fn new(reason: impl Into<String>, frame_index: u64) -> Self {
        Self {
            reason: reason.into(),
            frame_index,
        }
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"reason\":{},\"frame_index\":{}}}",
            game_dev_json_string(&self.reason),
            self.frame_index,
        )
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameDevRunMode {
    PlayInEditor,
    SimulateInEditor,
    Headless,
}

impl GameDevRunMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PlayInEditor => "play_in_editor",
            Self::SimulateInEditor => "simulate_in_editor",
            Self::Headless => "headless",
        }
    }

    pub const fn is_headless(self) -> bool {
        matches!(self, Self::Headless)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameDevPhase {
    Editing,
    Playing,
    Simulating,
    Paused,
    FrameAdvance,
}

impl GameDevPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Editing => "editing",
            Self::Playing => "playing",
            Self::Simulating => "simulating",
            Self::Paused => "paused",
            Self::FrameAdvance => "frame_advance",
        }
    }

    pub const fn is_running(self) -> bool {
        matches!(self, Self::Playing | Self::Simulating)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameDevTransitionOperation {
    Play,
    Simulate,
    Pause,
    Resume,
    FrameAdvance,
    Edit,
    Eject,
    Keep,
    Discard,
    SelectCategory,
}

impl GameDevTransitionOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Simulate => "simulate",
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::FrameAdvance => "frame_advance",
            Self::Edit => "edit",
            Self::Eject => "eject",
            Self::Keep => "keep",
            Self::Discard => "discard",
            Self::SelectCategory => "select_category",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameDevTransitionStatus {
    Applied,
    Rejected,
}

impl GameDevTransitionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameDevTransition {
    pub sequence: u64,
    pub operation: GameDevTransitionOperation,
    pub before: GameDevPhase,
    pub after: GameDevPhase,
    pub status: GameDevTransitionStatus,
    pub run_mode: GameDevRunMode,
    pub frame_index: u64,
    pub source_revision: String,
    pub selected_world_id: Option<String>,
    pub selected_actor_id: Option<String>,
    pub reason: String,
}

impl GameDevTransition {
    pub fn render_json(&self) -> String {
        format!(
            "{{\"sequence\":{},\"operation\":{},\"before\":{},\"after\":{},\
\"status\":{},\"run_mode\":{},\"frame_index\":{},\"source_revision\":{},\
\"selected_world_id\":{},\"selected_actor_id\":{},\"reason\":{}}}",
            self.sequence,
            game_dev_json_string(self.operation.as_str()),
            game_dev_json_string(self.before.as_str()),
            game_dev_json_string(self.after.as_str()),
            game_dev_json_string(self.status.as_str()),
            game_dev_json_string(self.run_mode.as_str()),
            self.frame_index,
            game_dev_json_string(&self.source_revision),
            game_dev_json_option(self.selected_world_id.as_deref()),
            game_dev_json_option(self.selected_actor_id.as_deref()),
            game_dev_json_string(&self.reason),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameDevPhaseResult {
    pub phase: GameDevPhase,
    pub status: GameDevTransitionStatus,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameDevLaunchProfile {
    pub target: String,
    pub run_mode: GameDevRunMode,
    pub headless_compatible: bool,
    pub phases: Vec<GameDevPhaseResult>,
}

impl GameDevLaunchProfile {
    pub fn new(target: impl Into<String>, run_mode: GameDevRunMode) -> Self {
        Self {
            target: target.into(),
            run_mode,
            headless_compatible: true,
            phases: Vec::new(),
        }
    }

    pub fn record_phase(
        &mut self,
        phase: GameDevPhase,
        status: GameDevTransitionStatus,
        detail: impl Into<String>,
    ) {
        if self.phases.len() < 256 {
            self.phases.push(GameDevPhaseResult {
                phase,
                status,
                detail: detail.into(),
            });
        }
    }

    pub fn headless_variant(&self) -> Self {
        let mut profile = self.clone();
        profile.run_mode = GameDevRunMode::Headless;
        profile
    }

    pub fn render_json(&self) -> String {
        let phases = self
            .phases
            .iter()
            .map(|phase| {
                format!(
                    "{{\"phase\":{},\"status\":{},\"detail\":{}}}",
                    game_dev_json_string(phase.phase.as_str()),
                    game_dev_json_string(phase.status.as_str()),
                    game_dev_json_string(&phase.detail),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"target\":{},\"run_mode\":{},\"headless_compatible\":{},\"phases\":[{}]}}",
            game_dev_json_string(&self.target),
            game_dev_json_string(self.run_mode.as_str()),
            self.headless_compatible,
            phases,
        )
    }
}

/// A first-party development extension is a checked protocol value, not a
/// host-supplied category string. Release profiles do not register it.
#[derive(Clone, Debug, PartialEq)]
pub struct GameDevExtension {
    category: &'static str,
    extension: &'static str,
    profile: String,
}

impl GameDevExtension {
    pub const GAMEPLAY_DEBUGGER_CATEGORY: &'static str = "Gameplay Debugger";
    pub const GAMEPLAY_DEBUGGER_NAME: &'static str = "jet.gameplay.debugger";

    /// Build the Gameplay Debugger extension under an explicit target policy.
    ///
    /// The policy is carried by the host/session rather than inferred from the
    /// compiler build profile. A release session has no extension value to
    /// expose or serialize.
    pub fn checked_gameplay_debugger_with_policy(
        profile: impl Into<String>,
        policy: GameDevPolicy,
    ) -> Result<Option<Self>, String> {
        if !policy.game_debug_data_enabled() {
            return Err(
                "Gameplay Debugger is unavailable outside a development profile".to_string(),
            );
        }
        let profile = profile.into();
        if profile.trim().is_empty() || profile.chars().any(char::is_control) {
            return Err("game dev extension profile must be non-empty and printable".to_string());
        }
        if profile == "release" {
            return Ok(None);
        }
        Ok(Some(Self {
            category: Self::GAMEPLAY_DEBUGGER_CATEGORY,
            extension: Self::GAMEPLAY_DEBUGGER_NAME,
            profile,
        }))
    }

    /// Build the Gameplay Debugger with no authorized target policy.
    /// Callers must opt into the policy-aware constructor explicitly.
    pub fn checked_gameplay_debugger(
        profile: impl Into<String>,
    ) -> Result<Option<Self>, String> {
        Self::checked_gameplay_debugger_with_policy(profile, GameDevPolicy::new(false))
    }


    pub const fn category(&self) -> &'static str {
        self.category
    }

    pub const fn extension(&self) -> &'static str {
        self.extension
    }

    pub fn profile(&self) -> &str {
        &self.profile
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"category\":{},\"extension\":{},\"profile\":{},\
\"development_only\":true,\"capabilities\":[\"Devtools.GameInspect\",\"Devtools.GameControl\"]}}",
            game_dev_json_string(self.category),
            game_dev_json_string(self.extension),
            game_dev_json_string(&self.profile),
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum GameDevEvent {
    Frame(JetGameFrameIdentity),
    FrameSample(JetGameFrameSample),
    Diagnostic(JetGameErrorPause),
    Control(GameDevControl),
    Transition(GameDevTransition),
    GameplayDebugger(GameDevExtension),
    Profile(JetGameFrameReceipt),
    ReloadStatus(JetGameReloadFact),
    ProfileComparison(GameProfileComparison),
    Overlay(JetGameOverlayTransitionFact),
    OverlayAutocomplete(JetGameOverlayAutocompleteFact),
    OverlayCommand(JetGameOverlayCommandRow),
    OverlayOutput(JetGameOverlayOutputRow),
    OverlayMetric {
        value: JetGameOverlayMetricValue,
        frame: Option<JetGameFrameIdentity>,
    },
    DrawEvent(JetGameDrawEvent),
    World(JetGameWorldFact),
    WorldSelection(JetGameWorldSelectionProjection),
    WorldEdit(JetGameWorldEditRequest),
    WorldEvaluation(JetGamePausedEvalResult),
    AssetWatch(JetGameAssetWatchFacts),
    AssetImportReport(JetGameAssetImportReport),
    AssetReload(JetGameAssetTransactionReceipt),
    GameSwap(JetGameSwapOutcome),
    HotSwap(JetGameHotSwapReceipt),
    HotSwapExplanation(JetGameHotSwapExplanation),
    Crash(JetGameCrashBundle),
    Stopped(GameDevStopped),
}

impl GameDevEvent {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Frame(_) => "Frame",
            Self::FrameSample(_) => "GameFrameSample",
            Self::Control(_) => "Control",
            Self::Transition(_) => "GameTransition",
            Self::Diagnostic(_) => "Diagnostic",
            Self::GameplayDebugger(_) => "GameplayDebugger",
            Self::Profile(_) => "Profile",
            Self::ReloadStatus(_) => "ReloadStatus",
            Self::ProfileComparison(_) => "ProfileComparison",
            Self::Overlay(_) => "Overlay",
            Self::OverlayAutocomplete(_) => "OverlayAutocomplete",
            Self::OverlayCommand(_) => "OverlayCommand",
            Self::OverlayOutput(_) => "OverlayOutput",
            Self::OverlayMetric { .. } => "OverlayMetric",
            Self::DrawEvent(_) => "GameDrawEvent",
            Self::World(_) => "World",
            Self::WorldSelection(_) => "WorldSelection",
            Self::WorldEdit(_) => "WorldEdit",
            Self::WorldEvaluation(_) => "WorldEvaluation",
            Self::AssetWatch(_) => "AssetWatch",
            Self::AssetImportReport(_) => "AssetImportReport",
            Self::AssetReload(_) => "AssetReload",
            Self::GameSwap(_) => "GameSwap",
            Self::HotSwap(_) => "HotSwap",
            Self::HotSwapExplanation(_) => "HotSwapExplanation",
            Self::Crash(_) => "Crash",
            Self::Stopped(_) => "Stopped",
        }
    }
}

/// Build the canonical devtools events for one watcher/import projection.
///
/// Hosts supply the already-authorized asset roots and typed watcher events.
/// Coalescing, skipped-count bounds, import planning, and wire projection stay
/// in this protocol module so a host cannot create a second asset report.
pub fn project_asset_watch_import_events(
    timestamp_ms: u64,
    source: impl Into<String>,
    roots: JetGameAssetRootSet,
    events: &[JetGameAssetWatchEvent],
    skipped_count: usize,
) -> Result<Vec<JetDevtoolsEvent>, String> {
    let source = source.into();
    let facts = JetGameAssetWatchFacts::coalesce_with_skipped(&roots, events, skipped_count)
        .map_err(|error| error.to_string())?;
    let report = GameAssetImportAdapter::new(roots).import_report(&facts);
    Ok(vec![
        GameDevEvent::AssetWatch(facts).to_devtools_event(timestamp_ms, source.clone())?,
        GameDevEvent::AssetImportReport(report).to_devtools_event(timestamp_ms, source)?,
    ])
}

impl GameDevEvent {
    /// Convert a typed producer event to the one shared devtools envelope
    /// event. Hosts consume this method rather than inventing a second game
    /// wire format.
    pub fn to_devtools_event(
        &self,
        timestamp_ms: u64,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        let source = source.into();
        if let Some(body) = game_dev_typed_trace_body(self) {
            return JetDevtoolsEvent::try_new(timestamp_ms, source, body);
        }
        if let Some(body) = game_dev_swap_body(self) {
            return JetDevtoolsEvent::try_new(timestamp_ms, source, body);
        }
        let (entity, fields, payload) = match self {
            Self::Frame(frame) => (
                frame.scene.clone(),
                format!(
                    "{{\"frame_id\":{},\"scene\":{},\"frame_index\":{},\"build\":{},\
\"revision\":{},\"trace_id\":{}}}",
                    frame.frame_id,
                    game_dev_json_string(&frame.scene),
                    frame.frame_index,
                    game_dev_json_string(&frame.build),
                    game_dev_json_string(&frame.revision),
                    game_dev_json_string(&frame.trace_id),
                ),
                None,
            ),
            Self::FrameSample(_) | Self::DrawEvent(_) | Self::GameSwap(_) => {
                unreachable!("typed game events are handled before field projection")
            }
            Self::Diagnostic(pause) => (
                pause.source.source_id.clone(),
                format!(
                    "{{\"code\":{},\"message\":{},\"source\":{}}}",
                    game_dev_json_string(&pause.code),
                    game_dev_json_string(&pause.message),
                    game_dev_source_span_json(&pause.source),
                ),
                None,
            ),
            Self::Transition(transition) => (
                "game".to_string(),
                format!(
                    "{{\"operation\":{},\"before\":{},\"after\":{},\"status\":{},\
\"run_mode\":{},\"frame_index\":{},\"source_revision\":{}}}",
                    game_dev_json_string(transition.operation.as_str()),
                    game_dev_json_string(transition.before.as_str()),
                    game_dev_json_string(transition.after.as_str()),
                    game_dev_json_string(transition.status.as_str()),
                    game_dev_json_string(transition.run_mode.as_str()),
                    transition.frame_index,
                    game_dev_json_string(&transition.source_revision),
                ),
                Some(transition.render_json()),
            ),
            Self::Control(control) => ("game".to_string(), control.render_json(), None),
            Self::GameplayDebugger(extension) => (
                "game.gameplay_debugger".to_string(),
                format!(
                    "{{\"category\":{},\"extension\":{},\"profile\":{},\
\"development_only\":true}}",
                    game_dev_json_string(extension.category()),
                    game_dev_json_string(extension.extension()),
                    game_dev_json_string(extension.profile()),
                ),
                Some(extension.render_json()),
            ),
            Self::Profile(receipt) => (
                receipt
                    .frame
                    .as_ref()
                    .map(|frame| frame.scene.clone())
                    .unwrap_or_else(|| "game".to_string()),
                format!(
                    "{{\"sequence\":{},\"kind\":{},\"before\":{},\"after\":{},\
\"dropped_samples\":{}}}",
                    receipt.sequence,
                    game_dev_json_string(receipt.kind.as_str()),
                    game_dev_json_string(receipt.before.as_str()),
                    game_dev_json_string(receipt.after.as_str()),
                    receipt.dropped_samples,
                ),
                None,
            ),
            Self::ReloadStatus(fact) => (
                "game.reload".to_string(),
                format!(
                    "{{\"frame_id\":{},\"domain\":{},\"status\":{},\"reason\":{}}}",
                    fact.frame.frame_id,
                    game_dev_json_string(&fact.domain),
                    game_dev_json_string(fact.status.as_str()),
                    game_dev_json_string(&fact.reason),
                ),
                Some(fact.render_json()),
            ),
            Self::ProfileComparison(comparison) => (
                comparison.left.identity.scene.clone(),
                format!(
                    "{{\"scene\":{},\"target\":{},\"delta_function_ns\":{},\
\"regression\":{}}}",
                    game_dev_json_string(&comparison.left.identity.scene),
                    game_dev_json_string(&comparison.left.identity.target),
                    comparison.total_function_ns_delta,
                    comparison.is_regression(),
                ),
                Some(format!(
                    "{{\"left\":{},\"right\":{}}}",
                    game_profile_snapshot_json(&comparison.left),
                    game_profile_snapshot_json(&comparison.right),
                )),
            ),
            Self::Overlay(fact) => (
                "game.overlay".to_string(),
                format!(
                    "{{\"sequence\":{},\"kind\":{},\"before\":{},\"after\":{},\
\"visible\":{},\"input_capture\":{},\"filter\":{},\"error_pause_enabled\":{},\
\"frame\":{}}}",
                    fact.sequence,
                    game_dev_json_string(fact.kind.as_str()),
                    game_dev_json_string(fact.before.as_str()),
                    game_dev_json_string(fact.after.as_str()),
                    fact.visibility.visible,
                    game_dev_json_string(fact.visibility.input_capture.as_str()),
                    game_dev_json_string(fact.filter.as_str()),
                    fact.error_pause_enabled,
                    fact.frame
                        .as_ref()
                        .map(game_dev_frame_identity_json)
                        .unwrap_or_else(|| "null".to_string()),
                ),
                Some(fact.render_json()),
            ),
            Self::OverlayAutocomplete(fact) => (
                "game.overlay".to_string(),
                format!(
                    "{{\"query\":{},\"match_count\":{}}}",
                    game_dev_json_string(&fact.query),
                    fact.matches.len(),
                ),
                Some(fact.render_json()),
            ),
            Self::OverlayCommand(command) => (
                "game.overlay".to_string(),
                format!(
                    "{{\"id\":{},\"timestamp_ns\":{},\"text\":{}}}",
                    command.id,
                    command.timestamp_ns,
                    game_dev_json_string(&command.text),
                ),
                None,
            ),
            Self::OverlayOutput(row) => {
                let payload = row
                    .published_text()
                    .map(|_| row.render_json());
                (
                    "game.overlay".to_string(),
                    format!(
                        "{{\"id\":{},\"timestamp_ns\":{},\"category\":{},\
\"message\":{},\"source_id\":{},\"trace_id\":{},\"published\":{}}}",
                        row.id,
                        row.timestamp_ns,
                        game_dev_json_string(row.category.as_str()),
                        game_dev_json_string(&row.message),
                        game_dev_json_option(row.source_id.as_deref()),
                        game_dev_json_option(row.trace_id.as_deref()),
                        row.published_text().is_some(),
                    ),
                    payload,
                )
            }
            Self::OverlayMetric { value, frame } => (
                "game.overlay".to_string(),
                format!(
                    "{{\"metric\":{},\"value\":{},\"sampled_at_ns\":{},\"frame\":{}}}",
                    game_dev_json_string(value.metric.as_str()),
                    value.value,
                    value.sampled_at_ns,
                    frame
                        .as_ref()
                        .map(game_dev_frame_identity_json)
                        .unwrap_or_else(|| "null".to_string()),
                ),
                None,
            ),
            Self::World(world) => (
                world.world_id.clone(),
                format!(
                    "{{\"world_id\":{},\"frame_id\":{},\"source_id\":{},\"revision\":{}}}",
                    game_dev_json_string(&world.world_id),
                    world.frame_id,
                    game_dev_json_string(&world.source_id),
                    game_dev_json_string(&world.revision),
                ),
                Some(world.render_json()),
            ),
            Self::WorldSelection(selection) => (
                selection.world_id.clone(),
                format!(
                    "{{\"world_id\":{},\"frame_id\":{},\"source_id\":{},\"revision\":{}}}",
                    game_dev_json_string(&selection.world_id),
                    selection.frame_id,
                    game_dev_json_string(&selection.source_id),
                    game_dev_json_string(&selection.revision),
                ),
                Some(selection.render_json()),
            ),
            Self::WorldEdit(request) => (
                "game.world".to_string(),
                format!(
                    "{{\"field\":{},\"target_count\":{}}}",
                    game_dev_json_string(&request.field_patch.field),
                    request.targets.len(),
                ),
                Some(request.render_json()),
            ),
            Self::WorldEvaluation(result) => (
                "game.world".to_string(),
                format!(
                    "{{\"request_id\":{},\"status\":{},\"type_name\":{}}}",
                    game_dev_json_string(&result.request_id),
                    game_dev_json_string(&result.status),
                    game_dev_json_string(&result.type_name),
                ),
                Some(result.render_json()),
            ),
            Self::AssetWatch(facts) => (
                "game.assets".to_string(),
                format!(
                    "{{\"created\":{},\"changed\":{},\"deleted\":{},\"events\":{},\
\"skipped_count\":{}}}",
                    facts.created.len(),
                    facts.changed.len(),
                    facts.deleted.len(),
                    facts.events.len(),
                    facts.skipped_count,
                ),
                Some(facts.render_json()),
            ),
            Self::AssetImportReport(report) => (
                "game.assets".to_string(),
                format!(
                    "{{\"imported\":{},\"dependencies\":{},\"created\":{},\
\"deleted\":{},\"skipped\":{},\"skipped_count\":{},\"diagnostics\":{}}}",
                    report.imported.len(),
                    report.dependencies.len(),
                    report.created.len(),
                    report.deleted.len(),
                    report.skipped.len(),
                    report.skipped_count,
                    report.diagnostics.len(),
                ),
                Some(report.render_json()),
            ),
            Self::AssetReload(receipt) => (
                "game.assets".to_string(),
                format!(
                    "{{\"transaction_id\":{},\"status\":{},\"entries\":{},\
\"revision_before\":{},\"revision_after\":{}}}",
                    receipt.transaction_id,
                    game_dev_json_string(receipt.status.as_str()),
                    receipt.entries.len(),
                    receipt.revision_before,
                    receipt.revision_after,
                ),
                Some(receipt.render_json()),
            ),
            Self::HotSwap(receipt) => (
                "game.hotswap".to_string(),
                format!(
                    "{{\"transaction_id\":{},\"phase\":{},\"committed\":{},\
\"rolled_back\":{},\"from\":{},\"to\":{}}}",
                    receipt.transaction_id,
                    game_dev_json_string(receipt.phase.as_str()),
                    receipt.is_committed(),
                    receipt.is_rolled_back(),
                    receipt.from_identity.render_json(),
                    receipt.to_identity.render_json(),
                ),
                Some(receipt.render_json()),
            ),
            Self::HotSwapExplanation(explanation) => (
                "game.hotswap".to_string(),
                format!(
                    "{{\"compatible\":{},\"code\":{},\"message\":{},\
\"transaction_id\":{}}}",
                    explanation.compatible,
                    game_dev_json_string(&explanation.code),
                    game_dev_json_string(&explanation.message),
                    explanation
                        .transaction_id
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "null".to_string()),
                ),
                Some(explanation.render_json()),
            ),
            Self::Crash(bundle) => (
                "game.crash".to_string(),
                format!(
                    "{{\"frame_id\":{},\"reason\":{},\"target\":{},\
\"upload_policy\":{}}}",
                    bundle.frame.frame_id,
                    game_dev_json_string(&bundle.reason),
                    game_dev_json_string(&bundle.target),
                    game_dev_json_string(bundle.upload_policy.as_str()),
                ),
                Some(bundle.render_json()),
            ),
            Self::Stopped(stopped) => (
                "game".to_string(),
                format!(
                    "{{\"reason\":{},\"frame_index\":{}}}",
                    game_dev_json_string(&stopped.reason),
                    stopped.frame_index,
                ),
                Some(stopped.render_json()),
            ),
        };
        JetDevtoolsEvent::from_parts_with_payload(
            timestamp_ms,
            source,
            self.kind(),
            entity,
            fields,
            payload,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GameDevInput {
    Key { code: String, pressed: bool },
    Button { name: String, pressed: bool },
    Axis { name: String, value: i64 },
}

impl GameDevInput {
    pub fn key(code: impl Into<String>, pressed: bool) -> Self {
        Self::Key {
            code: code.into(),
            pressed,
        }
    }

    pub fn button(name: impl Into<String>, pressed: bool) -> Self {
        Self::Button {
            name: name.into(),
            pressed,
        }
    }

    pub fn axis(name: impl Into<String>, value: i64) -> Self {
        Self::Axis {
            name: name.into(),
            value,
        }
    }

    pub fn render_json(&self) -> String {
        match self {
            Self::Key { code, pressed } => format!(
                "{{\"kind\":\"key\",\"code\":{},\"pressed\":{}}}",
                game_dev_json_string(code),
                pressed,
            ),
            Self::Button { name, pressed } => format!(
                "{{\"kind\":\"button\",\"name\":{},\"pressed\":{}}}",
                game_dev_json_string(name),
                pressed,
            ),
            Self::Axis { name, value } => format!(
                "{{\"kind\":\"axis\",\"name\":{},\"value\":{}}}",
                game_dev_json_string(name),
                value,
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GameDevControl {
    Play,
    Simulate,
    Pause,
    Step,
    FrameAdvance,
    Resume,
    Edit,
    Eject { world_id: String, actor_id: String },
    Keep(JetGameWorldEditRequest),
    Discard,
    SelectGameplayCategory(String),
    Input(GameDevInput),
    Console(String),
    Autocomplete {
        query: String,
        commands: Vec<String>,
    },
    ToggleOverlay,
    SetOverlayVisible(bool),
    SetInputCapture(bool),
    SetErrorPauseEnabled(bool),
    SetFilter(JetGameOverlayFilter),
    ToggleFilter(JetGameOverlayOutputCategory),
    SelectMetric(JetGameOverlayMetric),
    DeselectMetric(JetGameOverlayMetric),
    SelectWorld(JetGameWorldSelection),
    EditWorld(JetGameWorldEditRequest),
    EvaluateWorld(JetGamePausedEvalRequest),
}

impl GameDevControl {
    pub fn render_json(&self) -> String {
        match self {
            Self::Pause => "{\"kind\":\"pause\"}".to_string(),
            Self::Step => "{\"kind\":\"step\"}".to_string(),
            Self::Resume => "{\"kind\":\"resume\"}".to_string(),
            Self::Input(input) => format!(
                "{{\"kind\":\"input\",\"input\":{}}}",
                input.render_json()
            ),
            Self::Console(command) => format!(
                "{{\"kind\":\"console\",\"command\":{}}}",
                game_dev_json_string(command)
            ),
            Self::Autocomplete { query, commands } => {
                let commands = commands
                    .iter()
                    .map(|command| game_dev_json_string(command))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"kind\":\"autocomplete\",\"query\":{},\"commands\":[{}]}}",
                    game_dev_json_string(query),
                    commands,
                )
            }
            Self::Play => "{\"kind\":\"play\"}".to_string(),
            Self::Simulate => "{\"kind\":\"simulate\"}".to_string(),
            Self::FrameAdvance => "{\"kind\":\"frame_advance\"}".to_string(),
            Self::Edit => "{\"kind\":\"edit\"}".to_string(),
            Self::Eject { world_id, actor_id } => format!(
                "{{\"kind\":\"eject\",\"world_id\":{},\"actor_id\":{}}}",
                game_dev_json_string(world_id),
                game_dev_json_string(actor_id),
            ),
            Self::Keep(request) => format!(
                "{{\"kind\":\"keep\",\"request\":{}}}",
                request.render_json()
            ),
            Self::Discard => "{\"kind\":\"discard\"}".to_string(),
            Self::SelectGameplayCategory(category) => format!(
                "{{\"kind\":\"select_category\",\"category\":{}}}",
                game_dev_json_string(category)
            ),
            Self::ToggleOverlay => "{\"kind\":\"toggle_overlay\"}".to_string(),
            Self::SetOverlayVisible(visible) => {
                format!("{{\"kind\":\"set_overlay_visible\",\"visible\":{visible}}}")
            }
            Self::SetInputCapture(capture) => {
                format!("{{\"kind\":\"set_input_capture\",\"capture\":{capture}}}")
            }
            Self::SetFilter(filter) => format!(
                "{{\"kind\":\"set_filter\",\"filter\":{}}}",
                game_dev_json_string(filter.as_str())
            ),
            Self::ToggleFilter(category) => format!(
                "{{\"kind\":\"toggle_filter\",\"category\":{}}}",
                game_dev_json_string(category.as_str())
            ),
            Self::SetErrorPauseEnabled(enabled) => format!(
                "{{\"kind\":\"set_error_pause_enabled\",\"enabled\":{enabled}}}"
            ),
            Self::SelectMetric(metric) => format!(
                "{{\"kind\":\"select_metric\",\"metric\":{}}}",
                game_dev_json_string(metric.as_str())
            ),
            Self::DeselectMetric(metric) => format!(
                "{{\"kind\":\"deselect_metric\",\"metric\":{}}}",
                game_dev_json_string(metric.as_str())
            ),
            Self::SelectWorld(selection) => format!(
                "{{\"kind\":\"select_world\",\"selection\":{}}}",
                selection.render_json()
            ),
            Self::EditWorld(request) => format!(
                "{{\"kind\":\"edit_world\",\"request\":{}}}",
                request.render_json()
            ),
            Self::EvaluateWorld(request) => format!(
                "{{\"kind\":\"evaluate_world\",\"request\":{}}}",
                request.render_json()
            ),
        }
    }
}

/// The four-part run key is kept exactly as supplied by the host. `session`
/// is deliberately part of the adapter value even though the canonical frame
/// trace carries source/build/revision: a profile ring is never shared by two
/// Devtools sessions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameDevRunIdentity {
    pub session: String,
    pub source: String,
    pub build: String,
    pub revision: String,
}

impl GameDevRunIdentity {
    pub fn new(
        session: impl Into<String>,
        source: impl Into<String>,
        build: impl Into<String>,
        revision: impl Into<String>,
    ) -> Self {
        Self {
            session: session.into(),
            source: source.into(),
            build: build.into(),
            revision: revision.into(),
        }
    }

    pub fn trace_identity(&self) -> JetGameTraceIdentity {
        JetGameTraceIdentity::new(
            self.build.clone(),
            self.revision.clone(),
            self.source.clone(),
        )
    }

    pub fn validate(&self) -> Result<(), JetGameFrameProfilerError> {
        jet_game_frame_profiler_require_text(&self.session, "session")?;
        self.trace_identity().validate()
    }
}

/// One bounded profiler session. All sequencing, identity checks, pause
/// transitions, and retention are delegated to the canonical profiler.
#[derive(Clone, Debug)]
pub struct GameDevProfileRing {
    identity: GameDevRunIdentity,
    profiler: JetGameFrameProfiler,
}

impl GameDevProfileRing {
    pub fn new(
        identity: GameDevRunIdentity,
    ) -> Result<Self, JetGameFrameProfilerError> {
        identity.validate()?;
        let profiler = JetGameFrameProfiler::new(identity.trace_identity());
        Ok(Self { identity, profiler })
    }

    pub fn with_capacity(
        identity: GameDevRunIdentity,
        capacity: usize,
    ) -> Result<Self, JetGameFrameProfilerError> {
        identity.validate()?;
        let profiler = JetGameFrameProfiler::with_capacity(identity.trace_identity(), capacity);
        Ok(Self { identity, profiler })
    }

    pub fn identity(&self) -> &GameDevRunIdentity {
        &self.identity
    }

    pub fn profiler(&self) -> &JetGameFrameProfiler {
        &self.profiler
    }

    pub fn profiler_mut(&mut self) -> &mut JetGameFrameProfiler {
        &mut self.profiler
    }
    pub fn set_context(
        &mut self,
        context: JetGameTraceContext,
    ) -> Result<(), JetGameFrameProfilerError> {
        self.profiler.set_context(context)
    }

    pub fn context(&self) -> Option<&JetGameTraceContext> {
        self.profiler.context()
    }

    pub fn next_sequence(&self) -> u64 {
        self.profiler.history().next_sequence()
    }

    pub fn publish(
        &mut self,
        fact: JetGameSampleFact,
    ) -> Result<JetGameFrameReceipt, JetGameFrameProfilerError> {
        self.profiler.record(fact)
    }

    pub fn publish_frame(
        &mut self,
        sample: JetGameFrameSample,
    ) -> Result<JetGameFrameReceipt, JetGameFrameProfilerError> {
        self.profiler.record_frame(sample)
    }

    pub fn publish_phase(
        &mut self,
        sample: JetGamePhaseSample,
    ) -> Result<JetGameFrameReceipt, JetGameFrameProfilerError> {
        self.profiler.record_phase(sample)
    }

    pub fn publish_function(
        &mut self,
        sample: JetGameFunctionSample,
    ) -> Result<JetGameFrameReceipt, JetGameFrameProfilerError> {
        self.profiler.record_function(sample)
    }

    pub fn publish_draw(
        &mut self,
        event: JetGameDrawEvent,
    ) -> Result<JetGameFrameReceipt, JetGameFrameProfilerError> {
        self.profiler.record_draw(event)
    }

    pub fn pause(&mut self) -> JetGameFrameReceipt {
        self.profiler.pause()
    }

    pub fn resume(&mut self) -> JetGameFrameReceipt {
        self.profiler.resume()
    }

    pub fn error_pause(
        &mut self,
        source: JetGameSourceSpan,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<JetGameFrameReceipt, JetGameFrameProfilerError> {
        self.profiler.error_pause(source, code, message)
    }

    pub fn draw_cursor(
        &self,
        frame: JetGameFrameIdentity,
    ) -> Result<JetGameDrawCursor, JetGameFrameProfilerError> {
        self.profiler.draw_cursor(frame)
    }

    pub fn snapshot(
        &self,
        identity: GameProfileIdentity,
    ) -> Result<GameProfileSnapshot, GameProfileComparisonError> {
        GameProfileSnapshot::from_ring(self, identity)
    }

    pub fn bottleneck(&self, limit: usize) -> JetGameBottleneckProjection {
        self.profiler.bottleneck(limit)
    }

}
/// Identity carried by a completed profile.  Build identity is intentionally
/// not used as a comparison join key: comparing two builds is the point.  The
/// scene, target, and exact asset identities must match before their profiles
/// can be compared.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameProfileIdentity {
    pub scene: String,
    pub build: String,
    pub target: String,
    pub assets: Vec<JetGameAssetNodeIdentity>,
}

impl GameProfileIdentity {
    pub fn new(
        scene: impl Into<String>,
        build: impl Into<String>,
        target: impl Into<String>,
        assets: Vec<JetGameAssetNodeIdentity>,
    ) -> Self {
        Self {
            scene: scene.into(),
            build: build.into(),
            target: target.into(),
            assets,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GameProfileComparisonError {
    Incomplete {
        side: &'static str,
        state: JetGameFrameProfilerState,
    },
    IncompatibleIdentity {
        field: &'static str,
        left: String,
        right: String,
    },
}

impl GameProfileComparisonError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Incomplete { .. } => "profile_incomplete",
            Self::IncompatibleIdentity { .. } => "profile_identity_mismatch",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Incomplete { side, state } => {
                format!("{side} profile is not complete; profiler state is {}", state.as_str())
            }
            Self::IncompatibleIdentity { field, left, right } => {
                format!("profile identity mismatch for {field}: `{left}` versus `{right}`")
            }
        }
    }
}

/// A frozen, typed projection of one paused profiler session.  It owns no
/// ambient storage; taking a snapshot copies only canonical profiler facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameProfileSnapshot {
    pub identity: GameProfileIdentity,
    pub run: GameDevRunIdentity,
    pub trace_identity: JetGameTraceIdentity,
    pub projection: JetGameBottleneckProjection,
}

impl GameProfileSnapshot {
    pub fn from_profiler(
        profiler: &JetGameFrameProfiler,
        run: GameDevRunIdentity,
        identity: GameProfileIdentity,
    ) -> Result<Self, GameProfileComparisonError> {
        if profiler.state() != JetGameFrameProfilerState::Paused {
            return Err(GameProfileComparisonError::Incomplete {
                side: "profile",
                state: profiler.state(),
            });
        }
        let trace_identity = profiler.trace_identity().clone();
        if run.build != trace_identity.build {
            return Err(GameProfileComparisonError::IncompatibleIdentity {
                field: "build",
                left: run.build,
                right: trace_identity.build,
            });
        }
        if run.revision != trace_identity.revision {
            return Err(GameProfileComparisonError::IncompatibleIdentity {
                field: "revision",
                left: run.revision,
                right: trace_identity.revision,
            });
        }
        if run.source != trace_identity.trace_id {
            return Err(GameProfileComparisonError::IncompatibleIdentity {
                field: "source",
                left: run.source,
                right: trace_identity.trace_id,
            });
        }
        if trace_identity.build != identity.build {
            return Err(GameProfileComparisonError::IncompatibleIdentity {
                field: "build",
                left: identity.build,
                right: trace_identity.build,
            });
        }
        let projection = profiler.bottleneck(JET_GAME_FRAME_PROFILER_MAX_BOTTLENECKS);
        Ok(Self {
            identity,
            run,
            trace_identity,
            projection,
        })
    }

    pub fn from_ring(
        ring: &GameDevProfileRing,
        identity: GameProfileIdentity,
    ) -> Result<Self, GameProfileComparisonError> {
        Self::from_profiler(&ring.profiler, ring.identity.clone(), identity)
    }
}

/// Comparison of two completed profiles from independent sessions.  The
/// sessions and their source/revision keys remain visible in both snapshots;
/// no ring or profiler state is shared across the comparison.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameProfileComparison {
    pub left: GameProfileSnapshot,
    pub right: GameProfileSnapshot,
    pub total_function_ns_delta: i128,
}

impl GameProfileComparison {
    pub fn compare(
        left: GameProfileSnapshot,
        right: GameProfileSnapshot,
    ) -> Result<Self, GameProfileComparisonError> {
        if left.identity.scene != right.identity.scene {
            return Err(GameProfileComparisonError::IncompatibleIdentity {
                field: "scene",
                left: left.identity.scene.clone(),
                right: right.identity.scene.clone(),
            });
        }
        if left.identity.target != right.identity.target {
            return Err(GameProfileComparisonError::IncompatibleIdentity {
                field: "target",
                left: left.identity.target.clone(),
                right: right.identity.target.clone(),
            });
        }
        if left.identity.assets != right.identity.assets {
            let left_assets = left
                .identity
                .assets
                .iter()
                .map(JetGameAssetNodeIdentity::key)
                .collect::<Vec<_>>()
                .join(",");
            let right_assets = right
                .identity
                .assets
                .iter()
                .map(JetGameAssetNodeIdentity::key)
                .collect::<Vec<_>>()
                .join(",");
            return Err(GameProfileComparisonError::IncompatibleIdentity {
                field: "assets",
                left: left_assets,
                right: right_assets,
            });
        }
        let total_function_ns_delta =
            i128::from(right.projection.total_function_ns)
                - i128::from(left.projection.total_function_ns);
        Ok(Self {
            left,
            right,
            total_function_ns_delta,
        })
    }

    pub const fn is_regression(&self) -> bool {
        self.total_function_ns_delta > 0
    }
}

const JET_GAME_DEVTOOLS_COMMAND_FRAME_MAX: usize = 256;

fn jet_game_control_object(
    value: &jet_std::DataTree,
) -> Result<&[(String, jet_std::DataTree)], String> {
    match value {
        jet_std::DataTree::Object(fields) => Ok(fields),
        _ => Err("game control command payload must be an object".to_string()),
    }
}

fn jet_game_control_field<'a>(
    fields: &'a [(String, jet_std::DataTree)],
    name: &str,
) -> Option<&'a jet_std::DataTree> {
    fields
        .iter()
        .find_map(|(field_name, value)| (field_name == name).then_some(value))
}

fn jet_game_control_required_text(
    fields: &[(String, jet_std::DataTree)],
    name: &str,
) -> Result<String, String> {
    match jet_game_control_field(fields, name) {
        Some(jet_std::DataTree::Text(value) | jet_std::DataTree::TypedText(value))
            if !value.is_empty() =>
        {
            Ok(value.clone())
        }
        Some(_) => Err(format!("game control field `{name}` must be non-empty text")),
        None => Err(format!("game control field `{name}` is required")),
    }
}

fn jet_game_control_optional_text(
    fields: &[(String, jet_std::DataTree)],
    name: &str,
) -> Result<Option<String>, String> {
    match jet_game_control_field(fields, name) {
        None | Some(jet_std::DataTree::Null) => Ok(None),
        Some(jet_std::DataTree::Text(value) | jet_std::DataTree::TypedText(value))
            if !value.is_empty() =>
        {
            Ok(Some(value.clone()))
        }
        Some(_) => Err(format!("game control field `{name}` must be text or null")),
    }
}

fn jet_game_control_optional_u64(
    fields: &[(String, jet_std::DataTree)],
    name: &str,
) -> Result<Option<u64>, String> {
    match jet_game_control_field(fields, name) {
        None | Some(jet_std::DataTree::Null) => Ok(None),
        Some(jet_std::DataTree::Int(value)) if *value >= 0 => Ok(Some(*value as u64)),
        Some(jet_std::DataTree::Number(value)) => value
            .parse::<u64>()
            .map(Some)
            .map_err(|_| format!("game control field `{name}` is outside u64")),
        Some(_) => Err(format!("game control field `{name}` must be an unsigned integer")),
    }
}

fn jet_game_decode_control_request(
    session_id: &str,
    payload: &str,
) -> Result<JetDevtoolsGameControlRequest, String> {
    let value = jet_std::parse_json_strict(payload)
        .map_err(|_| "invalid game control payload".to_string())?;
    let fields = jet_game_control_object(&value)?;
    let expected = [
        "session_id",
        "request_id",
        "kind",
        "source_id",
        "revision",
        "world_id",
        "actor_id",
        "component_id",
        "authored_instance_id",
        "source_span_start",
        "source_span_end",
        "field",
        "value",
        "category",
        "expression",
        "required_authority",
        "frame_id",
        "budget",
    ];
    if fields.len() != expected.len()
        || !fields
            .iter()
            .map(|(key, _)| key)
            .all(|key| expected.iter().any(|expected_key| key == expected_key))
    {
        return Err("game control payload has unexpected fields".to_string());
    }
    let payload_session = jet_game_control_required_text(fields, "session_id")?;
    if payload_session != session_id {
        return Err("game control payload session identity mismatch".to_string());
    }
    let request_id = jet_game_control_required_text(fields, "request_id")?;
    let kind = JetDevtoolsGameControlKind::from_str(
        &jet_game_control_required_text(fields, "kind")?,
    )?;
    let mut request = JetDevtoolsGameControlRequest::new(payload_session, request_id, kind)?;
    request.source_id = jet_game_control_optional_text(fields, "source_id")?;
    request.revision = jet_game_control_optional_text(fields, "revision")?;
    request.world_id = jet_game_control_optional_text(fields, "world_id")?;
    request.actor_id = jet_game_control_optional_text(fields, "actor_id")?;
    request.component_id = jet_game_control_optional_text(fields, "component_id")?;
    request.authored_instance_id =
        jet_game_control_optional_text(fields, "authored_instance_id")?;
    request.source_span_start = jet_game_control_optional_u64(fields, "source_span_start")?;
    request.source_span_end = jet_game_control_optional_u64(fields, "source_span_end")?;
    request.field = jet_game_control_optional_text(fields, "field")?;
    request.value = jet_game_control_optional_text(fields, "value")?;
    request.category = jet_game_control_optional_text(fields, "category")?;
    request.expression = jet_game_control_optional_text(fields, "expression")?;
    request.required_authority =
        jet_game_control_optional_text(fields, "required_authority")?;
    request.frame_id = jet_game_control_optional_u64(fields, "frame_id")?;
    request.budget = jet_game_control_optional_u64(fields, "budget")?;
    request.validate()?;
    Ok(request)
}

fn jet_game_ingest_devtools_command_frame(
    frame: &str,
    expected_session_id: &str,
) -> Result<usize, String> {
    let value = jet_std::parse_json_strict(frame)
        .map_err(|_| "invalid game control command frame".to_string())?;
    let fields = jet_game_control_object(&value)?;
    let protocol = jet_game_control_required_text(fields, "protocol")?;
    if protocol != JET_DEVTOOLS_PROTOCOL {
        return Err("game control command frame protocol mismatch".to_string());
    }
    let session_id = jet_game_control_required_text(fields, "session_id")?;
    if session_id != expected_session_id {
        return Err("game control command frame session identity mismatch".to_string());
    }
    let _ = jet_game_control_optional_u64(fields, "started_at_ms")?
        .ok_or_else(|| "game control command frame requires started_at_ms".to_string())?;
    let direction = jet_game_control_required_text(fields, "direction")?;
    if direction != "host_to_runtime" {
        return Err("game control command frame direction mismatch".to_string());
    }
    let commands = match jet_game_control_field(fields, "commands") {
        Some(jet_std::DataTree::Array(commands)) => commands,
        _ => return Err("game control command frame requires commands".to_string()),
    };
    if commands.len() > JET_GAME_DEVTOOLS_COMMAND_FRAME_MAX {
        return Err("game control command frame exceeds command limit".to_string());
    }
    let mut accepted: usize = 0;
    for command in commands {
        let command_fields = jet_game_control_object(command)?;
        let kind = jet_game_control_required_text(command_fields, "kind")?;
        if kind != "GameControl" {
            return Err(format!("unsupported game control command `{kind}`"));
        }
        let payload = jet_game_control_field(command_fields, "payload")
            .ok_or_else(|| "game control command is missing payload".to_string())?;
        let payload = jet_std::render_json(payload, false, 0);
        let request = jet_game_decode_control_request(&session_id, &payload)?;
        jet_devtools_enqueue_game_control(request)?;
        accepted = accepted.saturating_add(1);
    }
    Ok(accepted)
}

fn jet_game_devtools_callback(session_id: &str, payload: &str) -> Result<(), String> {
    let request = jet_game_decode_control_request(session_id, payload)?;
    jet_devtools_enqueue_game_control(request)
}

static JET_GAME_DEVTOOLS_CALLBACK_GUARD: std::sync::LazyLock<
    Option<JetDevtoolsGameControlCallbackGuard>,
> = std::sync::LazyLock::new(|| {
    jet_devtools_install_game_control_callback(jet_game_devtools_callback).ok()
});

fn jet_game_install_devtools_callback() {
    // Callers invoke this only after constructing an authorized session. The
    // session policy, not a compiler build flag, owns whether game controls
    // are reachable.
    let _ = &*JET_GAME_DEVTOOLS_CALLBACK_GUARD;
}

fn jet_game_control_request(
    request: JetDevtoolsGameControlRequest,
    session: &GameDevSession,
) -> Result<GameDevControl, String> {
    let identity = session.profiler.identity();
    if request.session_id != identity.session {
        return Err("game control request session identity mismatch".to_string());
    }
    if let Some(source_id) = request.source_id.as_deref() {
        if source_id != identity.source {
            return Err("game control request source identity mismatch".to_string());
        }
    }
    if let Some(revision) = request.revision.as_deref() {
        if revision != identity.revision {
            return Err("game control request revision identity mismatch".to_string());
        }
    }
    match request.kind {
        JetDevtoolsGameControlKind::Play => Ok(GameDevControl::Play),
        JetDevtoolsGameControlKind::Simulate => Ok(GameDevControl::Simulate),
        JetDevtoolsGameControlKind::Pause => Ok(GameDevControl::Pause),
        JetDevtoolsGameControlKind::Step => Ok(GameDevControl::Step),
        JetDevtoolsGameControlKind::Resume => Ok(GameDevControl::Resume),
        JetDevtoolsGameControlKind::FrameAdvance => Ok(GameDevControl::FrameAdvance),
        JetDevtoolsGameControlKind::Edit => Ok(GameDevControl::Edit),
        JetDevtoolsGameControlKind::Discard => Ok(GameDevControl::Discard),
        JetDevtoolsGameControlKind::SelectCategory => Ok(
            GameDevControl::SelectGameplayCategory(
                request
                    .category
                    .ok_or_else(|| "select_category requires category".to_string())?,
            ),
        ),
        JetDevtoolsGameControlKind::SelectWorld => {
            let world_id = request
                .world_id
                .ok_or_else(|| "select_world requires world_id".to_string())?;
            let selection = JetGameWorldSelection::new(
                world_id,
                request.actor_id,
                request.component_id,
                request.field,
            )
            .map_err(|error| error.to_string())?;
            Ok(GameDevControl::SelectWorld(selection))
        }
        JetDevtoolsGameControlKind::EvaluateWorld => {
            let world_id = request
                .world_id
                .ok_or_else(|| "evaluate_world requires world_id".to_string())?;
            let frame_id = request
                .frame_id
                .ok_or_else(|| "evaluate_world requires frame_id".to_string())?;
            let actor_id = request
                .actor_id
                .ok_or_else(|| "evaluate_world requires actor_id".to_string())?;
            let component_id = request
                .component_id
                .ok_or_else(|| "evaluate_world requires component_id".to_string())?;
            let expression = request
                .expression
                .ok_or_else(|| "evaluate_world requires expression".to_string())?;
            let required_authority = request
                .required_authority
                .ok_or_else(|| "evaluate_world requires required_authority".to_string())?;
            let budget = request
                .budget
                .ok_or_else(|| "evaluate_world requires budget".to_string())?;
            JetGamePausedEvalRequest::new(
                request.request_id,
                identity.session.clone(),
                world_id,
                frame_id,
                actor_id,
                component_id,
                expression,
                required_authority,
                budget,
            )
            .map_err(|error| error.to_string())?
            .with_tier("game")
            .map(GameDevControl::EvaluateWorld)
            .map_err(|error| error.to_string())
        }
        JetDevtoolsGameControlKind::Eject => Ok(GameDevControl::Eject {
            world_id: request
                .world_id
                .ok_or_else(|| "eject requires world_id".to_string())?,
            actor_id: request
                .actor_id
                .ok_or_else(|| "eject requires actor_id".to_string())?,
        }),
        JetDevtoolsGameControlKind::Keep | JetDevtoolsGameControlKind::EditWorld => {
            let source_id = request
                .source_id
                .ok_or_else(|| "world edit requires source_id".to_string())?;
            let revision = request
                .revision
                .ok_or_else(|| "world edit requires revision".to_string())?;
            let authored_instance_id = request
                .authored_instance_id
                .ok_or_else(|| "world edit requires authored_instance_id".to_string())?;
            let start = request
                .source_span_start
                .ok_or_else(|| "world edit requires source_span_start".to_string())?;
            let end = request
                .source_span_end
                .ok_or_else(|| "world edit requires source_span_end".to_string())?;
            let field = request
                .field
                .ok_or_else(|| "world edit requires field".to_string())?;
            let value = request
                .value
                .ok_or_else(|| "world edit requires value".to_string())?;
            let span = JetGameWorldSourceSpan::new(
                usize::try_from(start)
                    .map_err(|_| "world edit source span is too large".to_string())?,
                usize::try_from(end)
                    .map_err(|_| "world edit source span is too large".to_string())?,
            )
            .map_err(|error| error.to_string())?;
            let target =
                JetGameWorldEditTarget::new(source_id, revision, authored_instance_id, span)
                    .map_err(|error| error.to_string())?
                    .with_session_identity(identity.session.clone(), "game")
                    .map_err(|error| error.to_string())?;
            let edit = JetGameWorldEditRequest::new(vec![target], field, value)
                .map_err(|error| error.to_string())?;
            if matches!(request.kind, JetDevtoolsGameControlKind::Keep) {
                Ok(GameDevControl::Keep(edit))
            } else {
                Ok(GameDevControl::EditWorld(edit))
            }
        }
    }
}

/// A live typed game-dev session. The envelope remains the only transport
/// owner; all game state is supplied by the canonical Core modules.
#[derive(Clone, Debug)]
pub struct GameDevSession {
    pub envelope: JetDevtoolsEnvelope,
    pub lifecycle: JetDevtoolsLifecycleState,
    pub profiler: GameDevProfileRing,
    pub overlay: JetGameOverlayState,
    pub inspector: JetGameWorldInspector,
    pub asset_roots: JetGameAssetRootSet,
    pub policy: GameDevPolicy,
    pub swap_session: JetGameDevSession,
    asset_import: GameAssetImportAdapter,
    asset_runtime: GameAssetRuntimeAdapter,
    pub last_asset_watch: Option<JetGameAssetWatchFacts>,
    pub last_asset_import_report: Option<JetGameAssetImportReport>,
    pub hot_swap_live: Option<JetGameHotSwapSnapshot>,
    pub last_input: Option<GameDevInput>,
    pub phase: GameDevPhase,
    pub run_mode: GameDevRunMode,
    pub frame_index: u64,
    pub selected_world_id: Option<String>,
    pub selected_actor_id: Option<String>,
    pub selected_debugger_category: Option<String>,
    pub launch_profile: GameDevLaunchProfile,
    pending_world_edits: Vec<JetGameWorldEditRequest>,
    next_transition_sequence: u64,
    frame_advance_pending: bool,
    draw_cursor: Option<JetGameDrawCursor>,
}

impl GameDevSession {
    pub fn new(
        identity: GameDevRunIdentity,
        started_at_ms: u64,
    ) -> Result<Self, JetGameFrameProfilerError> {
        Self::new_with_policy(identity, started_at_ms, GameDevPolicy::new(false))
    }

    pub fn new_with_policy(
        identity: GameDevRunIdentity,
        started_at_ms: u64,
        policy: GameDevPolicy,
    ) -> Result<Self, JetGameFrameProfilerError> {
        if !policy.game_debug_data_enabled() {
            return Err(JetGameFrameProfilerError::InvalidTraceContext {
                reason: "game development data is disabled for this profile",
            });
        }
        let profiler = GameDevProfileRing::new(identity.clone())?;
        let launch_profile =
            GameDevLaunchProfile::new(identity.source.clone(), GameDevRunMode::Headless);
        let asset_roots = JetGameAssetRootSet::default();
        let mut inspector = JetGameWorldInspector::default();
        inspector.set_session_identity(identity.session.clone(), "game");
        let mut envelope = JetDevtoolsEnvelope::new(identity.session.clone(), started_at_ms);
        envelope.lifecycle = JetDevtoolsLifecycleState::Ready;
        envelope.freshness = JetDevtoolsFreshnessFact::fresh(started_at_ms);
        envelope.source_identity = Some(JetDevtoolsSourceIdentityFact::new(
            Some(identity.source.clone()),
            Some(identity.build.clone()),
            Some(identity.revision.clone()),
            None,
        ));
        Ok(Self {
            envelope,
            lifecycle: JetDevtoolsLifecycleState::Ready,
            profiler,
            overlay: JetGameOverlayState::new(),
            inspector,
            swap_session: JetGameDevSession::new(),
            asset_import: GameAssetImportAdapter::new(asset_roots.clone()),
            asset_runtime: GameAssetRuntimeAdapter::new(),
            asset_roots,
            policy,
            last_asset_watch: None,
            last_asset_import_report: None,
            hot_swap_live: None,
            last_input: None,
            phase: GameDevPhase::Editing,
            run_mode: GameDevRunMode::Headless,
            frame_index: 0,
            selected_world_id: None,
            selected_actor_id: None,
            selected_debugger_category: None,
            launch_profile,
            pending_world_edits: Vec::new(),
            next_transition_sequence: 1,
            frame_advance_pending: false,
            draw_cursor: None,
        })
    }
    pub const fn game_debug_data_enabled(&self) -> bool {
        self.policy.game_debug_data_enabled()
    }

    /// Transfer host commands from both the in-process callback and the
    /// separate-process relay into the one typed queue, then apply them
    /// through `dispatch_control`, the sole transition/effect owner.
    pub fn dispatch_pending_devtools_controls(
        &mut self,
        timestamp_ms: u64,
    ) -> Result<usize, String> {
        const MAX_CONTROLS_PER_TICK: usize = 32;
        for frame in jet_devtools_take_game_control_relays(&self.envelope.session_id)? {
            jet_game_ingest_devtools_command_frame(&frame, &self.envelope.session_id)?;
        }
        let pending = jet_devtools_command_count(&self.envelope.session_id)
            .min(MAX_CONTROLS_PER_TICK);
        let mut dispatched = 0usize;
        for _ in 0..pending {
            let Some(request) = jet_devtools_poll_game_control(&self.envelope.session_id) else {
                break;
            };
            let control = jet_game_control_request(request, self)?;
            self.dispatch_control(timestamp_ms, control)?;
            dispatched = dispatched.saturating_add(1);
        }
        Ok(dispatched)
    }

    /// Register the checked Gameplay Debugger extension for a development
    /// profile. Release policies reject the request before any extension
    /// value or event payload is constructed.
    pub fn register_gameplay_debugger(
        &mut self,
        timestamp_ms: u64,
        profile: impl Into<String>,
    ) -> Result<Option<u64>, String> {
        if !self.policy.game_debug_data_enabled() {
            return Err(
                "Gameplay Debugger is unavailable outside a development profile".to_string(),
            );
        }
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Err("game dev session is stopped".to_string());
        }
        let Some(extension) =
            GameDevExtension::checked_gameplay_debugger_with_policy(profile, self.policy)?
        else {
            return Ok(None);
        };
        let source = self.profiler.identity().source.clone();
        self.push_game_event(
            timestamp_ms,
            source,
            GameDevEvent::GameplayDebugger(extension),
        )
        .map(Some)
    }

    pub fn record(
        &mut self,
        fact: JetGameSampleFact,
    ) -> Result<JetGameFrameReceipt, JetGameFrameProfilerError> {
        self.profiler.publish(fact)
    }
    pub fn set_trace_context(
        &mut self,
        context: JetGameTraceContext,
    ) -> Result<(), String> {
        self.profiler
            .set_context(context)
            .map_err(|error| error.to_string())
    }

    pub fn trace_context(&self) -> Option<&JetGameTraceContext> {
        self.profiler.context()
    }
    pub fn publish_reload_status(
        &mut self,
        timestamp_ms: u64,
        fact: JetGameReloadFact,
    ) -> Result<u64, String> {
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Err("game dev session is stopped".to_string());
        }
        fact.validate().map_err(|error| error.to_string())?;
        let source = self.profiler.identity().source.clone();
        self.push_game_event(timestamp_ms, source, GameDevEvent::ReloadStatus(fact))
    }

    /// Record one profiler fact and publish both its frame and receipt through
    /// the shared envelope. The frame is copied before recording because the
    /// profiler is the owner of sequence and pause validation.
    pub fn record_and_publish(
        &mut self,
        fact: JetGameSampleFact,
    ) -> Result<JetGameFrameReceipt, String> {
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Err("game dev session is stopped".to_string());
        }
        let frame = fact.frame_identity().clone();
        let frame_sample = match &fact {
            JetGameSampleFact::Frame(sample) => Some(sample.clone()),
            _ => None,
        };
        let timestamp_ms = fact.timestamp_ns() / 1_000_000;
        let receipt = self
            .record(fact)
            .map_err(|error| error.to_string())?;
        self.overlay.set_frame_identity(frame.clone());
        self.draw_cursor = None;
        let source = self.profiler.identity().source.clone();
        self.push_game_event(
            timestamp_ms,
            source.clone(),
            GameDevEvent::Frame(frame),
        )?;
        if let Some(mut sample) = frame_sample {
            if let Some(sequence) = receipt.sample_sequence {
                sample.sequence = sequence;
            }
            self.push_game_event(
                timestamp_ms,
                source.clone(),
                GameDevEvent::FrameSample(sample),
            )?;
        }
        self.push_game_event(timestamp_ms, source, GameDevEvent::Profile(receipt.clone()))?;
        Ok(receipt)
    }
    /// Publish a diagnostic and, when enabled by the overlay policy, enter
    /// the same Error Pause in both the profiler and overlay state. The
    /// diagnostic is still observable when Error Pause is disabled.
    pub fn publish_diagnostic(
        &mut self,
        timestamp_ms: u64,
        pause: JetGameErrorPause,
    ) -> Result<Vec<u64>, String> {
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Err("game dev session is stopped".to_string());
        }
        let source = self.profiler.identity().source.clone();
        let mut profile = None;
        let mut overlay = None;
        if self.overlay.error_pause_enabled()
            && self.profiler.profiler().state() == JetGameFrameProfilerState::Running
        {
            let receipt = self
                .profiler
                .error_pause(
                    pause.source.clone(),
                    pause.code.clone(),
                    pause.message.clone(),
                )
                .map_err(|error| error.to_string())?;
            let fact = self
                .overlay
                .error_pause(
                    pause.source.source_id.clone(),
                    self.profiler.identity().trace_identity().trace_id,
                )
                .map_err(|error| error.message())?;
            self.phase = GameDevPhase::Paused;
            self.frame_advance_pending = false;
            self.draw_cursor = None;
            self.envelope.lifecycle = JetDevtoolsLifecycleState::Error;
            self.envelope.freshness = JetDevtoolsFreshnessFact::fresh(timestamp_ms);
            profile = Some(receipt);
            overlay = Some(fact);
        }
        let mut sequences = vec![self.push_game_event(
            timestamp_ms,
            source.clone(),
            GameDevEvent::Diagnostic(pause),
        )?];
        if let Some(fact) = overlay {
            sequences.push(self.push_game_event(
                timestamp_ms,
                source.clone(),
                GameDevEvent::Overlay(fact),
            )?);
        }
        if let Some(receipt) = profile {
            sequences.push(self.push_game_event(
                timestamp_ms,
                source,
                GameDevEvent::Profile(receipt),
            )?);
        }
        Ok(sequences)
    }
    pub fn publish_crash(
        &mut self,
        timestamp_ms: u64,
        mut bundle: JetGameCrashBundle,
    ) -> Result<u64, String> {
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Err("game dev session is stopped".to_string());
        }
        if bundle.context.is_none() {
            bundle.context = self.trace_context().cloned();
        }
        let expected_trace = self.profiler.identity().trace_identity();
        if bundle.frame.trace_identity() != expected_trace {
            return Err("game crash frame identity does not match the live trace".to_string());
        }
        bundle.validate().map_err(|error| error.to_string())?;
        let source = self.profiler.identity().source.clone();
        self.push_game_event(timestamp_ms, source, GameDevEvent::Crash(bundle))
    }

    pub fn push_game_event(
        &mut self,
        timestamp_ms: u64,
        source: impl Into<String>,
        event: GameDevEvent,
    ) -> Result<u64, String> {
        let wire = event.to_devtools_event(timestamp_ms, source)?;
        let sequence = self.push_event(wire.clone());
        jet_devtools_publish_event_for_session(&self.envelope.session_id, wire);
        Ok(sequence)
    }

    /// Apply one checked asset change through the shared host/Prelude kernel.
    pub fn apply_asset_swap(
        &mut self,
        timestamp_ms: u64,
        fact: JetGameChangeFact,
    ) -> Result<JetGameSwapOutcome, String> {
        self.apply_game_swap(timestamp_ms, fact, JetGameChangeKind::Asset)
    }

    /// Apply one checked script reload through the shared host/Prelude kernel.
    pub fn apply_script_reload(
        &mut self,
        timestamp_ms: u64,
        fact: JetGameChangeFact,
    ) -> Result<JetGameSwapOutcome, String> {
        self.apply_game_swap(timestamp_ms, fact, JetGameChangeKind::Script)
    }

    /// Apply one checked world reload through the shared host/Prelude kernel.
    pub fn apply_world_reload(
        &mut self,
        timestamp_ms: u64,
        fact: JetGameChangeFact,
    ) -> Result<JetGameSwapOutcome, String> {
        self.apply_game_swap(timestamp_ms, fact, JetGameChangeKind::World)
    }

    fn apply_game_swap(
        &mut self,
        timestamp_ms: u64,
        fact: JetGameChangeFact,
        kind: JetGameChangeKind,
    ) -> Result<JetGameSwapOutcome, String> {
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Err("game dev session is stopped".to_string());
        }
        if fact.kind != kind {
            return Err(format!(
                "game swap operation `{}` received `{}` fact",
                kind.as_str(),
                fact.kind.as_str()
            ));
        }
        let outcome = match kind {
            JetGameChangeKind::Asset => self.swap_session.apply_asset_swap(fact)?,
            JetGameChangeKind::Script => self.swap_session.apply_script_reload(fact)?,
            JetGameChangeKind::World => self.swap_session.apply_world_reload(fact)?,
        };
        let source = self.profiler.identity().source.clone();
        self.push_game_event(timestamp_ms, source, GameDevEvent::GameSwap(outcome.clone()))?;
        Ok(outcome)
    }

    pub fn publish_overlay_metric(
        &mut self,
        timestamp_ms: u64,
        value: JetGameOverlayMetricValue,
    ) -> Result<u64, String> {
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Err("game dev session is stopped".to_string());
        }
        self.overlay
            .observe_metric(value.metric, value.value, value.sampled_at_ns);
        let frame = self.overlay.frame_identity().cloned();
        let source = self.profiler.identity().source.clone();
        self.push_game_event(
            timestamp_ms,
            source,
            GameDevEvent::OverlayMetric { value, frame },
        )
    }

    pub fn publish_overlay_output(
        &mut self,
        timestamp_ms: u64,
        row: JetGameOverlayOutputRow,
    ) -> Result<u64, String> {
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Err("game dev session is stopped".to_string());
        }
        self.overlay
            .record_output(row)
            .map_err(|error| error.message())?;
        let row = self
            .overlay
            .history()
            .last()
            .cloned()
            .ok_or_else(|| "game overlay output was not retained".to_string())?;
        let source = self.profiler.identity().source.clone();
        self.push_game_event(timestamp_ms, source, GameDevEvent::OverlayOutput(row))
    }

    fn publish_overlay_command(
        &mut self,
        timestamp_ms: u64,
        command: String,
    ) -> Result<u64, String> {
        self.overlay
            .record_command(timestamp_ms.saturating_mul(1_000_000), command)
            .map_err(|error| error.message())?;
        let command = self
            .overlay
            .commands()
            .last()
            .cloned()
            .ok_or_else(|| "game overlay command was not retained".to_string())?;
        let source = self.profiler.identity().source.clone();
        self.push_game_event(timestamp_ms, source, GameDevEvent::OverlayCommand(command))
    }

    fn step_draw_event(&mut self) -> Result<Option<JetGameDrawEvent>, String> {
        let frame = self
            .profiler
            .profiler()
            .paused_frame()
            .cloned()
            .ok_or_else(|| "game frame debugger requires a paused frame".to_string())?;
        let needs_cursor = self
            .draw_cursor
            .as_ref()
            .map(|cursor| cursor.frame_identity() != &frame)
            .unwrap_or(true);
        if needs_cursor {
            self.draw_cursor = Some(
                self.profiler
                    .draw_cursor(frame.clone())
                    .map_err(|error| error.to_string())?,
            );
        }
        let events = self
            .profiler
            .profiler()
            .history()
            .draw_events_for(&frame)
            .map_err(|error| error.to_string())?;
        self.draw_cursor
            .as_mut()
            .ok_or_else(|| "game frame debugger cursor is unavailable".to_string())?
            .step_forward(&events)
            .map_err(|error| error.to_string())
    }

    fn evaluate_world_request(
        &self,
        request: JetGamePausedEvalRequest,
    ) -> JetGamePausedEvalResult {
        let request_id = request.request_id.clone();
        if self.profiler.profiler().state() == JetGameFrameProfilerState::Running {
            return game_dev_eval_result(request_id, JET_GAME_PAUSED_EVAL_RUNNING);
        }
        let Some(frame) = self.profiler.profiler().paused_frame() else {
            return game_dev_eval_result(request_id, JET_GAME_PAUSED_EVAL_STALE);
        };
        if frame.frame_id != request.frame_id {
            return game_dev_eval_result(request_id, JET_GAME_PAUSED_EVAL_STALE);
        }
        self.inspector.evaluate_request(request)
    }

    fn publish_transition(
        &mut self,
        timestamp_ms: u64,
        operation: GameDevTransitionOperation,
        before: GameDevPhase,
        after: GameDevPhase,
        status: GameDevTransitionStatus,
        reason: impl Into<String>,
    ) -> Result<u64, String> {
        let reason = reason.into();
        let sequence = self.next_transition_sequence;
        self.next_transition_sequence = self.next_transition_sequence.saturating_add(1).max(1);
        self.launch_profile.record_phase(after, status, reason.clone());
        let transition = GameDevTransition {
            sequence,
            operation,
            before,
            after,
            status,
            run_mode: self.run_mode,
            frame_index: self.frame_index,
            source_revision: self.profiler.identity().revision.clone(),
            selected_world_id: self.selected_world_id.clone(),
            selected_actor_id: self.selected_actor_id.clone(),
            reason,
        };
        let source = self.profiler.identity().source.clone();
        self.push_game_event(timestamp_ms, source, GameDevEvent::Transition(transition))
    }


    /// Return whether the next simulation tick is admitted. Frame advance
    /// consumes exactly one pending tick and then waits at the paused boundary.
    pub fn begin_simulation_frame(&mut self) -> bool {
        match self.phase {
            GameDevPhase::Playing | GameDevPhase::Simulating => true,
            GameDevPhase::FrameAdvance if self.frame_advance_pending => {
                self.frame_advance_pending = false;
                true
            }
            _ => false,
        }
    }

    pub fn finish_simulation_frame(
        &mut self,
        timestamp_ms: u64,
        frame_index: u64,
    ) -> Result<Vec<u64>, String> {
        self.frame_index = frame_index;
        if self.phase != GameDevPhase::FrameAdvance {
            return Ok(Vec::new());
        }
        let source = self.profiler.identity().source.clone();
        let overlay = self.overlay.pause();
        let profile = self.profiler.pause();
        self.phase = GameDevPhase::Paused;
        self.draw_cursor = None;
        let mut sequences = vec![self.push_game_event(
            timestamp_ms,
            source.clone(),
            GameDevEvent::Overlay(overlay),
        )?];
        sequences.push(self.push_game_event(
            timestamp_ms,
            source.clone(),
            GameDevEvent::Profile(profile),
        )?);
        sequences.push(self.publish_transition(
            timestamp_ms,
            GameDevTransitionOperation::FrameAdvance,
            GameDevPhase::FrameAdvance,
            GameDevPhase::Paused,
            GameDevTransitionStatus::Applied,
            "one simulation frame completed",
        )?);
        Ok(sequences)
    }

    fn validate_control_phase(&self, control: &GameDevControl) -> Result<(), String> {
        let allowed = match control {
            GameDevControl::Play | GameDevControl::Simulate => {
                matches!(self.phase, GameDevPhase::Editing | GameDevPhase::Paused)
            }
            GameDevControl::Pause => {
                matches!(self.phase, GameDevPhase::Playing | GameDevPhase::Simulating)
            }
            GameDevControl::Resume => {
                matches!(self.phase, GameDevPhase::Paused | GameDevPhase::FrameAdvance)
            }
            GameDevControl::FrameAdvance
            | GameDevControl::Step
            | GameDevControl::SelectWorld(_)
            | GameDevControl::EditWorld(_)
            | GameDevControl::EvaluateWorld(_)
            | GameDevControl::Eject { .. }
            | GameDevControl::Keep(_)
            | GameDevControl::Discard => {
                matches!(self.phase, GameDevPhase::Paused | GameDevPhase::FrameAdvance)
            }
            GameDevControl::Edit
            | GameDevControl::SelectGameplayCategory(_)
            | GameDevControl::Input(_)
            | GameDevControl::Console(_)
            | GameDevControl::Autocomplete { .. }
            | GameDevControl::ToggleOverlay
            | GameDevControl::SetOverlayVisible(_)
            | GameDevControl::SetInputCapture(_)
            | GameDevControl::SetErrorPauseEnabled(_)
            | GameDevControl::SetFilter(_)
            | GameDevControl::ToggleFilter(_)
            | GameDevControl::SelectMetric(_)
            | GameDevControl::DeselectMetric(_) => true,
        };
        if allowed {
            Ok(())
        } else {
            Err(format!(
                "game control `{}` is not valid during `{}`",
                control.render_json(),
                self.phase.as_str()
            ))
        }
    }

    pub fn dispatch_control(
        &mut self,
        timestamp_ms: u64,
        control: GameDevControl,
    ) -> Result<Vec<u64>, String> {
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Err("game dev session is stopped".to_string());
        }
        self.validate_control_phase(&control)?;
        let source = self.profiler.identity().source.clone();
        let mut sequences = vec![self.push_game_event(
            timestamp_ms,
            source.clone(),
            GameDevEvent::Control(control.clone()),
        )?];
        let is_play = matches!(&control, GameDevControl::Play);
        match control {
            GameDevControl::Play | GameDevControl::Simulate => {
                let before = self.phase;
                if !matches!(before, GameDevPhase::Editing | GameDevPhase::Paused) {
                    return Err("game run can start only from editing or paused".to_string());
                }
                self.run_mode = if is_play {
                    GameDevRunMode::PlayInEditor
                } else {
                    GameDevRunMode::SimulateInEditor
                };
                self.phase = if is_play {
                    GameDevPhase::Playing
                } else {
                    GameDevPhase::Simulating
                };
                let overlay = self.overlay.resume();
                let profile = self.profiler.resume();
                self.lifecycle = JetDevtoolsLifecycleState::Ready;
                self.envelope.lifecycle = JetDevtoolsLifecycleState::Ready;
                self.envelope.freshness = JetDevtoolsFreshnessFact::fresh(timestamp_ms);
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source.clone(),
                    GameDevEvent::Overlay(overlay),
                )?);
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source.clone(),
                    GameDevEvent::Profile(profile),
                )?);
                sequences.push(self.publish_transition(
                    timestamp_ms,
                    if is_play {
                        GameDevTransitionOperation::Play
                    } else {
                        GameDevTransitionOperation::Simulate
                    },
                    before,
                    self.phase,
                    GameDevTransitionStatus::Applied,
                    "game run started",
                )?);
            }
            GameDevControl::Edit => {
                let before = self.phase;
                self.phase = GameDevPhase::Editing;
                self.run_mode = GameDevRunMode::Headless;
                self.frame_advance_pending = false;
                let overlay = self.overlay.pause();
                let profile = self.profiler.pause();
                self.draw_cursor = None;
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source.clone(),
                    GameDevEvent::Overlay(overlay),
                )?);
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source.clone(),
                    GameDevEvent::Profile(profile),
                )?);
                sequences.push(self.publish_transition(
                    timestamp_ms,
                    GameDevTransitionOperation::Edit,
                    before,
                    GameDevPhase::Editing,
                    GameDevTransitionStatus::Applied,
                    "returned to source editing",
                )?);
            }
            GameDevControl::Eject { world_id, actor_id } => {
                if world_id.is_empty() || actor_id.is_empty() {
                    return Err("eject requires non-empty world and actor ids".to_string());
                }
                let before = self.phase;
                self.selected_world_id = Some(world_id);
                self.selected_actor_id = Some(actor_id);
                sequences.push(self.publish_transition(
                    timestamp_ms,
                    GameDevTransitionOperation::Eject,
                    before,
                    before,
                    GameDevTransitionStatus::Applied,
                    "live actor selected",
                )?);
            }
            GameDevControl::Keep(request) => {
                let before = self.phase;
                if !matches!(before, GameDevPhase::Paused | GameDevPhase::FrameAdvance) {
                    return Err("keep requires a paused game state".to_string());
                }
                self.inspector
                    .validate_edit_request(&request)
                    .map_err(|error| error.to_string())?;
                self.pending_world_edits.clear();
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source.clone(),
                    GameDevEvent::WorldEdit(request),
                )?);
                sequences.push(self.publish_transition(
                    timestamp_ms,
                    GameDevTransitionOperation::Keep,
                    before,
                    before,
                    GameDevTransitionStatus::Applied,
                    "source-backed game change accepted",
                )?);
            }
            GameDevControl::Discard => {
                let before = self.phase;
                self.pending_world_edits.clear();
                sequences.push(self.publish_transition(
                    timestamp_ms,
                    GameDevTransitionOperation::Discard,
                    before,
                    before,
                    GameDevTransitionStatus::Applied,
                    "runtime changes discarded; authored source unchanged",
                )?);
            }
            GameDevControl::SelectGameplayCategory(category) => {
                if !self.policy.game_debug_data_enabled() {
                    return Err(
                        "Gameplay Debugger categories are unavailable outside a development profile"
                            .to_string(),
                    );
                }
                if category.trim().is_empty() || category.chars().any(char::is_control) {
                    return Err("gameplay debugger category must be printable".to_string());
                }
                let before = self.phase;
                self.selected_debugger_category = Some(category);
                sequences.push(self.publish_transition(
                    timestamp_ms,
                    GameDevTransitionOperation::SelectCategory,
                    before,
                    before,
                    GameDevTransitionStatus::Applied,
                    "Gameplay Debugger category selected",
                )?);
            }
            GameDevControl::Pause => {
                let before = self.phase;
                let overlay = self.overlay.pause();
                let profile = self.profiler.pause();
                self.phase = GameDevPhase::Paused;
                self.frame_advance_pending = false;
                self.draw_cursor = None;
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source.clone(),
                    GameDevEvent::Overlay(overlay),
                )?);
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source.clone(),
                    GameDevEvent::Profile(profile),
                )?);
                sequences.push(self.publish_transition(
                    timestamp_ms,
                    GameDevTransitionOperation::Pause,
                    before,
                    GameDevPhase::Paused,
                    GameDevTransitionStatus::Applied,
                    "simulation paused",
                )?);
            }
            GameDevControl::Resume => {
                let before = self.phase;
                let overlay = self.overlay.resume();
                let profile = self.profiler.resume();
                self.phase = match self.run_mode {
                    GameDevRunMode::PlayInEditor => GameDevPhase::Playing,
                    GameDevRunMode::SimulateInEditor => GameDevPhase::Simulating,
                    GameDevRunMode::Headless => GameDevPhase::Playing,
                };
                self.frame_advance_pending = false;
                self.draw_cursor = None;
                self.lifecycle = JetDevtoolsLifecycleState::Ready;
                self.envelope.lifecycle = JetDevtoolsLifecycleState::Ready;
                self.envelope.freshness = JetDevtoolsFreshnessFact::fresh(timestamp_ms);
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source.clone(),
                    GameDevEvent::Overlay(overlay),
                )?);
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source.clone(),
                    GameDevEvent::Profile(profile),
                )?);
                sequences.push(self.publish_transition(
                    timestamp_ms,
                    GameDevTransitionOperation::Resume,
                    before,
                    self.phase,
                    GameDevTransitionStatus::Applied,
                    "simulation resumed",
                )?);
            }
            GameDevControl::Step => {
                if let Some(event) = self.step_draw_event()? {
                    sequences.push(self.push_game_event(
                        timestamp_ms,
                        source,
                        GameDevEvent::DrawEvent(event),
                    )?);
                }
            }
            GameDevControl::FrameAdvance => {
                let before = self.phase;
                if before != GameDevPhase::Paused {
                    return Err("frame advance requires a paused game state".to_string());
                }
                self.frame_advance_pending = true;
                self.phase = GameDevPhase::FrameAdvance;
                let overlay = self.overlay.resume();
                let profile = self.profiler.resume();
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source.clone(),
                    GameDevEvent::Overlay(overlay),
                )?);
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source.clone(),
                    GameDevEvent::Profile(profile),
                )?);
                sequences.push(self.publish_transition(
                    timestamp_ms,
                    GameDevTransitionOperation::FrameAdvance,
                    before,
                    GameDevPhase::FrameAdvance,
                    GameDevTransitionStatus::Applied,
                    "one simulation frame requested",
                )?);
            }
            GameDevControl::Input(input) => self.last_input = Some(input),
            GameDevControl::Console(command) => {
                sequences.push(self.publish_overlay_command(timestamp_ms, command)?);
            }
            GameDevControl::Autocomplete { query, commands } => {
                let fact = self
                    .overlay
                    .autocomplete(query, &commands)
                    .map_err(|error| error.message())?;
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source,
                    GameDevEvent::OverlayAutocomplete(fact),
                )?);
            }
            GameDevControl::ToggleOverlay => {
                let fact = self.overlay.set_visible(!self.overlay.visibility().visible);
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source,
                    GameDevEvent::Overlay(fact),
                )?);
            }
            GameDevControl::SetOverlayVisible(visible) => {
                let fact = self.overlay.set_visible(visible);
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source,
                    GameDevEvent::Overlay(fact),
                )?);
            }
            GameDevControl::SetInputCapture(capture) => {
                let fact = self.overlay.set_input_capture(capture);
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source,
                    GameDevEvent::Overlay(fact),
                )?);
            }
            GameDevControl::SetErrorPauseEnabled(enabled) => {
                let fact = self.overlay.set_error_pause_enabled(enabled);
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source,
                    GameDevEvent::Overlay(fact),
                )?);
            }
            GameDevControl::SetFilter(filter) => {
                let fact = self.overlay.set_filter(filter);
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source,
                    GameDevEvent::Overlay(fact),
                )?);
            }
            GameDevControl::ToggleFilter(category) => {
                let fact = self.overlay.toggle_filter(category);
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source,
                    GameDevEvent::Overlay(fact),
                )?);
            }
            GameDevControl::SelectMetric(metric) => {
                let fact = self
                    .overlay
                    .select_metric(metric)
                    .map_err(|error| error.message())?;
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source,
                    GameDevEvent::Overlay(fact),
                )?);
            }
            GameDevControl::DeselectMetric(metric) => {
                let fact = self.overlay.deselect_metric(metric);
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source,
                    GameDevEvent::Overlay(fact),
                )?);
            }
            GameDevControl::SelectWorld(selection) => {
                self.selected_world_id = Some(selection.world_id.clone());
                self.selected_actor_id = selection.entity_id.clone();
                self.inspector
                    .select(selection)
                    .map_err(|error| error.to_string())?;
                if let Some(projection) = self.inspector.project() {
                    sequences.push(self.push_game_event(
                        timestamp_ms,
                        source,
                        GameDevEvent::WorldSelection(projection),
                    )?);
                }
            }
            GameDevControl::EditWorld(request) => {
                self.inspector
                    .validate_edit_request(&request)
                    .map_err(|error| error.to_string())?;
                if self.pending_world_edits.len() >= 256 {
                    self.pending_world_edits.remove(0);
                }
                self.pending_world_edits.push(request.clone());
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source,
                    GameDevEvent::WorldEdit(request),
                )?);
            }
            GameDevControl::EvaluateWorld(request) => {
                let result = self.evaluate_world_request(request);
                sequences.push(self.push_game_event(
                    timestamp_ms,
                    source,
                    GameDevEvent::WorldEvaluation(result),
                )?);
            }
        }
        Ok(sequences)
    }
    /// Stop the runtime-owned session and discard every pending command before
    /// the host tears down its transport. This is the only teardown boundary
    /// for the generated game protocol.
    pub fn stop(&mut self, timestamp_ms: u64, reason: impl Into<String>) -> Result<(), String> {
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Ok(());
        }
        let reason = reason.into();
        if reason.trim().is_empty() || reason.chars().any(char::is_control) {
            return Err("game stop reason must be printable".to_string());
        }
        let frame_index = self.frame_index;
        self.lifecycle = JetDevtoolsLifecycleState::Stopped;
        self.envelope.lifecycle = JetDevtoolsLifecycleState::Stopped;
        self.envelope.freshness = JetDevtoolsFreshnessFact::fresh(timestamp_ms);
        jet_devtools_clear_commands(&self.envelope.session_id);
        let _ = jet_devtools_clear_game_control_relays();
        self.push_game_event(
            timestamp_ms,
            self.profiler.identity().source.clone(),
            GameDevEvent::Stopped(GameDevStopped::new(reason, frame_index)),
        )
        .map(|_| ())
    }

    pub fn ingest_world(
        &mut self,
        world: JetGameWorldFact,
    ) -> Result<(), JetGameWorldInspectorError> {
        self.inspector.ingest(world)
    }

    pub fn ingest_world_and_publish(
        &mut self,
        timestamp_ms: u64,
        world: JetGameWorldFact,
    ) -> Result<u64, String> {
        self.ingest_world(world.clone())
            .map_err(|error| error.to_string())?;
        let source = self.profiler.identity().source.clone();
        self.push_game_event(timestamp_ms, source, GameDevEvent::World(world))
    }

    pub fn configure_asset_roots(
        &mut self,
        roots: JetGameAssetRootSet,
    ) -> Result<(), String> {
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Err("game dev session is stopped".to_string());
        }
        let runtime = GameAssetRuntimeAdapter::with_roots(roots.clone())
            .map_err(|error| error.to_string())?;
        self.asset_import = GameAssetImportAdapter::new(roots.clone());
        self.asset_runtime = runtime;
        self.asset_roots = roots;
        self.last_asset_watch = None;
        self.last_asset_import_report = None;
        Ok(())
    }

    pub fn ingest_asset_watch(
        &mut self,
        events: &[JetGameAssetWatchEvent],
    ) -> Result<JetGameAssetWatchFacts, String> {
        self.ingest_asset_watch_with_skipped(events, 0)
    }

    pub fn ingest_asset_watch_with_skipped(
        &mut self,
        events: &[JetGameAssetWatchEvent],
        skipped_count: usize,
    ) -> Result<JetGameAssetWatchFacts, String> {
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Err("game dev session is stopped".to_string());
        }
        let facts = JetGameAssetWatchFacts::coalesce_with_skipped(
            self.asset_import.roots(),
            events,
            skipped_count,
        )
        .map_err(|error| error.to_string())?;
        self.last_asset_watch = Some(facts.clone());
        Ok(facts)
    }
    pub fn ingest_asset_import_report(
        &mut self,
        events: &[JetGameAssetWatchEvent],
    ) -> Result<JetGameAssetImportReport, String> {
        self.ingest_asset_import_report_with_skipped(events, 0)
    }

    pub fn ingest_asset_import_report_with_skipped(
        &mut self,
        events: &[JetGameAssetWatchEvent],
        skipped_count: usize,
    ) -> Result<JetGameAssetImportReport, String> {
        let facts = self.ingest_asset_watch_with_skipped(events, skipped_count)?;
        let report = self.asset_import.import_report(&facts);
        self.last_asset_import_report = Some(report.clone());
        Ok(report)
    }

    pub fn register_asset_source(
        &mut self,
        source: JetGameAssetSourceIdentity,
    ) -> Result<JetGameAssetNodeIdentity, String> {
        self.asset_import
            .add_source(source)
            .map_err(|error| error.to_string())
    }

    pub fn set_asset_dependencies(
        &mut self,
        source: &JetGameAssetSourceIdentity,
        dependencies: Vec<JetGameAssetDependencyIdentity>,
    ) -> Result<(), String> {
        self.asset_import
            .set_dependencies(source, dependencies)
            .map_err(|error| error.to_string())
    }
    pub fn asset_dependencies(
        &self,
        node: &JetGameAssetNodeIdentity,
    ) -> Vec<JetGameAssetDependencyIdentity> {
        self.asset_import.graph().dependencies_for(node)
    }

    pub fn asset_revision(&self) -> u64 {
        self.asset_runtime.revision()
    }

    pub fn ready_assets(&self) -> Vec<JetGameAssetArtifactIdentity> {
        self.asset_runtime.ready_assets()
    }

    pub fn latest_asset_reload(
        &self,
        source: &JetGameAssetSourceIdentity,
    ) -> Option<JetGameAssetReloadReceipt> {
        self.asset_runtime.latest(source)
    }


    pub fn plan_asset_import(
        &self,
        source: JetGameAssetSourceIdentity,
        recipe: JetGameAssetRecipeIdentity,
        tool: JetGameAssetToolIdentity,
        version: JetGameAssetVersionIdentity,
        dependencies: Vec<JetGameAssetDependencyIdentity>,
    ) -> Result<JetGameAssetImportPlan, String> {
        self.asset_import
            .import_plan(source, recipe, tool, version, dependencies)
            .map_err(|error| error.to_string())
    }

    pub fn ready_asset_outcome(
        &self,
        plan: &JetGameAssetImportPlan,
        output_hash: impl Into<String>,
    ) -> Result<JetGameAssetImportOutcome, String> {
        self.asset_import
            .ready_outcome(plan, output_hash)
            .map_err(|error| error.to_string())
    }

    pub fn failed_asset_outcome(
        &self,
        plan: &JetGameAssetImportPlan,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<JetGameAssetImportOutcome, String> {
        self.asset_import
            .failed_outcome(plan, code, message)
            .map_err(|error| error.to_string())
    }

    pub fn ingest_asset_watch_and_publish(
        &mut self,
        timestamp_ms: u64,
        events: &[JetGameAssetWatchEvent],
    ) -> Result<u64, String> {
        self.ingest_asset_watch_and_publish_with_skipped(timestamp_ms, events, 0)
    }

    pub fn ingest_asset_watch_and_publish_with_skipped(
        &mut self,
        timestamp_ms: u64,
        events: &[JetGameAssetWatchEvent],
        skipped_count: usize,
    ) -> Result<u64, String> {
        let facts = self.ingest_asset_watch_with_skipped(events, skipped_count)?;
        let source = self.profiler.identity().source.clone();
        self.push_game_event(timestamp_ms, source, GameDevEvent::AssetWatch(facts))
    }

    pub fn ingest_asset_watch_and_import_publish(
        &mut self,
        timestamp_ms: u64,
        events: &[JetGameAssetWatchEvent],
    ) -> Result<Vec<u64>, String> {
        self.ingest_asset_watch_and_import_publish_with_skipped(timestamp_ms, events, 0)
    }

    pub fn ingest_asset_watch_and_import_publish_with_skipped(
        &mut self,
        timestamp_ms: u64,
        events: &[JetGameAssetWatchEvent],
        skipped_count: usize,
    ) -> Result<Vec<u64>, String> {
        let facts = self.ingest_asset_watch_with_skipped(events, skipped_count)?;
        let report = self.asset_import.import_report(&facts);
        self.last_asset_import_report = Some(report.clone());
        let source = self.profiler.identity().source.clone();
        let watch = self.push_game_event(
            timestamp_ms,
            source.clone(),
            GameDevEvent::AssetWatch(facts),
        )?;
        let report = self.push_game_event(
            timestamp_ms,
            source,
            GameDevEvent::AssetImportReport(report),
        )?;
        Ok(vec![watch, report])
    }

    pub fn reload_asset(
        &mut self,
        timestamp_ms: u64,
        plan: JetGameAssetImportPlan,
        outcome: JetGameAssetImportOutcome,
    ) -> Result<JetGameAssetTransactionReceipt, String> {
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Err("game dev session is stopped".to_string());
        }
        let receipt = self.asset_runtime.apply(plan, outcome);
        let source = self.profiler.identity().source.clone();
        self.push_game_event(
            timestamp_ms,
            source,
            GameDevEvent::AssetReload(receipt.clone()),
        )?;
        Ok(receipt)
    }
    pub fn reload_asset_with_fact(
        &mut self,
        timestamp_ms: u64,
        plan: JetGameAssetImportPlan,
        outcome: JetGameAssetImportOutcome,
        fact: &JetGameAssetRuntimeFact,
    ) -> Result<JetGameAssetTransactionReceipt, String> {
        fact.validate_against(&plan, &outcome)
            .map_err(|error| error.to_string())?;
        self.reload_asset(timestamp_ms, plan, outcome)
    }

    pub fn reload_assets(
        &mut self,
        timestamp_ms: u64,
        entries: Vec<(JetGameAssetImportPlan, JetGameAssetImportOutcome)>,
    ) -> Result<JetGameAssetTransactionReceipt, String> {
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Err("game dev session is stopped".to_string());
        }
        let receipt = self.asset_runtime.apply_transaction(entries)?;
        let source = self.profiler.identity().source.clone();
        self.push_game_event(
            timestamp_ms,
            source,
            GameDevEvent::AssetReload(receipt.clone()),
        )?;
        Ok(receipt)
    }

    pub fn set_hot_swap_live(
        &mut self,
        snapshot: JetGameHotSwapSnapshot,
    ) -> Result<(), String> {
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Err("game dev session is stopped".to_string());
        }
        snapshot.validate().map_err(|error| error.message())?;
        self.hot_swap_live = Some(snapshot);
        Ok(())
    }

    pub fn hot_swap_plan(
        &self,
        current: &JetGameHotSwapSnapshot,
        candidate: &JetGameHotSwapSnapshot,
        link_mode: JetGameLinkMode,
    ) -> Result<JetGameHotSwapPlan, JetGameHotSwapError> {
        JetGameHotSwapPlan::new(current, candidate, link_mode)
    }
    pub fn explain_hot_swap(
        &self,
        current: &JetGameHotSwapSnapshot,
        candidate: &JetGameHotSwapSnapshot,
        link_mode: JetGameLinkMode,
    ) -> JetGameHotSwapExplanation {
        JetGameHotSwapExplanation::explain(current, candidate, link_mode)
    }

    pub fn publish_hot_swap_explanation(
        &mut self,
        timestamp_ms: u64,
        current: &JetGameHotSwapSnapshot,
        candidate: &JetGameHotSwapSnapshot,
        link_mode: JetGameLinkMode,
    ) -> Result<u64, String> {
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Err("game dev session is stopped".to_string());
        }
        let explanation = self.explain_hot_swap(current, candidate, link_mode);
        let source = self.profiler.identity().source.clone();
        self.push_game_event(
            timestamp_ms,
            source,
            GameDevEvent::HotSwapExplanation(explanation),
        )
    }

    pub fn execute_hot_swap(
        &mut self,
        timestamp_ms: u64,
        candidate: &JetGameHotSwapSnapshot,
        link_mode: JetGameLinkMode,
    ) -> Result<JetGameHotSwapReceipt, String> {
        if self.lifecycle == JetDevtoolsLifecycleState::Stopped {
            return Err("game dev session is stopped".to_string());
        }
        let mut live = self
            .hot_swap_live
            .take()
            .ok_or_else(|| "game hot swap has no live snapshot".to_string())?;
        let plan = match self.hot_swap_plan(&live, candidate, link_mode) {
            Ok(plan) => plan,
            Err(error) => {
                self.hot_swap_live = Some(live);
                return Err(error.message());
            }
        };
        let result = plan.execute(&mut live);
        self.hot_swap_live = Some(live);
        let receipt = result.map_err(|error| error.message())?;
        let source = self.profiler.identity().source.clone();
        self.push_game_event(
            timestamp_ms,
            source,
            GameDevEvent::HotSwap(receipt.clone()),
        )?;
        Ok(receipt)
    }


    pub fn push_event(&mut self, event: JetDevtoolsEvent) -> u64 {
        self.envelope.push(event)
    }
    /// Return the retained typed frame/draw bodies for the canonical trace
    /// projection. Wire fields are intentionally not decoded.
    pub fn game_trace_bodies(&self) -> Vec<JetDevtoolsEventBody> {
        self.envelope
            .events()
            .filter_map(|event| {
                let body = event.body()?;
                match body {
                    JetDevtoolsEventBody::GameFrameSample { .. }
                    | JetDevtoolsEventBody::GameDrawEvent { .. } => Some(body.clone()),
                    _ => None,
                }
            })
            .collect()
    }
}

// D-GAME-WASM-BRIDGE1: the browser adapter is a transport shell around the
// canonical GameDevSession. It owns no transition, validation, or world policy.
// The scratch rails keep the bridge independent from generated ABI helper
// names: JavaScript writes a bounded frame, then copies the canonical event
// envelope before the next call.
#[cfg(target_arch = "wasm32")]
struct JetGameWasmBridge {
    policy: GameDevPolicy,
    session: Option<GameDevSession>,
    input: Vec<u8>,
    output: Vec<u8>,
    error: Vec<u8>,
    sent_sequence: u64,
}

#[cfg(target_arch = "wasm32")]
impl JetGameWasmBridge {
    fn new() -> Self {
        Self {
            policy: GameDevPolicy::new(JET_GAME_DEBUG_DATA_ENABLED),
            session: None,
            input: Vec::new(),
            output: Vec::new(),
            error: Vec::new(),
            sent_sequence: 0,
        }
    }

    fn set_error(&mut self, error: impl Into<String>) {
        self.error = error.into().into_bytes();
        self.output.clear();
    }

    fn set_error_with_wire(&mut self, error: impl Into<String>) {
        self.set_error(error);
        let _ = self.take_wire();
    }

    fn take_wire(&mut self) -> Result<(), String> {
        let Some(session) = self.session.as_ref() else {
            return Err("game devtools Wasm bridge is not initialized".to_string());
        };
        let latest = session.envelope.latest_sequence().unwrap_or(0);
        let oldest = session.envelope.oldest_sequence();
        let truncated = session.envelope.truncated
            || self.sent_sequence != 0
                && oldest.is_some_and(|sequence| self.sent_sequence.saturating_add(1) < sequence);
        let mut envelope =
            JetDevtoolsEnvelope::new(session.envelope.session_id.clone(), session.envelope.started_at_ms);
        envelope.lifecycle = session.envelope.lifecycle;
        envelope.freshness = session.envelope.freshness;
        envelope.privacy = session.envelope.privacy;
        envelope.source_identity = session.envelope.source_identity.clone();
        envelope.view = session.envelope.view.clone();
        envelope.reset = session.envelope.reset || truncated;
        envelope.truncated = session.envelope.truncated || truncated;
        for event in session.envelope.events_since((self.sent_sequence != 0).then_some(self.sent_sequence)) {
            envelope.push(event.clone());
        }
        let wire = envelope.serialize()?;
        self.sent_sequence = latest;
        self.output = wire.into_bytes();
        Ok(())
    }
}


#[cfg(target_arch = "wasm32")]
fn jet_game_wasm_debug_data_enabled() -> bool {
    JET_GAME_WASM_BRIDGE.with(|cell| {
        let bridge = cell.borrow();
        bridge
            .session
            .as_ref()
            .map(GameDevSession::game_debug_data_enabled)
            .unwrap_or_else(|| bridge.policy.game_debug_data_enabled())
    })
}

#[cfg(target_arch = "wasm32")]
fn jet_game_asset_watch_events(
    frame: &str,
    expected_session_id: &str,
) -> Result<(Vec<JetGameAssetWatchEvent>, usize), String> {
    let value = jet_std::parse_json_strict(frame)
        .map_err(|error| format!("invalid game asset watch frame: {}", error.reason))?;
    let fields = jet_game_control_object(&value)?;
    if jet_game_control_required_text(fields, "protocol")? != JET_DEVTOOLS_PROTOCOL {
        return Err("game asset watch frame protocol mismatch".to_string());
    }
    if jet_game_control_required_text(fields, "session_id")? != expected_session_id {
        return Err("game asset watch frame session identity mismatch".to_string());
    }
    if jet_game_control_required_text(fields, "direction")? != "host_to_runtime" {
        return Err("game asset watch frame direction mismatch".to_string());
    }
    let _ = jet_game_control_optional_u64(fields, "started_at_ms")?
        .ok_or_else(|| "game asset watch frame requires started_at_ms".to_string())?;
    let skipped_count = jet_game_control_optional_u64(fields, "skipped")?
        .unwrap_or(0);
    let skipped_count = usize::try_from(skipped_count)
        .map_err(|_| "game asset watch frame skipped count is outside usize".to_string())?;
    if skipped_count > JET_GAME_ASSET_MAX_SKIPPED {
        return Err("game asset watch frame exceeds skipped count limit".to_string());
    }
    let values = match jet_game_control_field(fields, "events") {
        Some(jet_std::DataTree::Array(values)) => values,
        _ => return Err("game asset watch frame requires events".to_string()),
    };
    if values.len() > JET_GAME_ASSET_MAX_EVENTS {
        return Err("game asset watch frame exceeds event limit".to_string());
    }
    let events = values
        .iter()
        .map(|value| {
            let fields = jet_game_control_object(value)?;
            let root = match jet_game_control_field(fields, "root") {
                Some(jet_std::DataTree::Object(root)) => root,
                _ => return Err("game asset watch event requires root".to_string()),
            };
            let root_id = match jet_game_control_field(root, "id") {
                Some(jet_std::DataTree::Text(value) | jet_std::DataTree::TypedText(value)) => {
                    value.clone()
                }
                _ => return Err("game asset watch root requires id".to_string()),
            };
            let root_path = match jet_game_control_field(root, "path") {
                Some(jet_std::DataTree::Text(value) | jet_std::DataTree::TypedText(value)) => {
                    value.clone()
                }
                _ => return Err("game asset watch root requires path".to_string()),
            };
            let logical_path = jet_game_control_required_text(fields, "logical_path")?;
            let kind = match jet_game_control_required_text(fields, "kind")?.as_str() {
                "created" => JetGameAssetWatchEventKind::Created,
                "changed" => JetGameAssetWatchEventKind::Changed,
                "deleted" => JetGameAssetWatchEventKind::Deleted,
                _ => return Err("game asset watch event kind is unsupported".to_string()),
            };
            let root = JetGameAssetRootIdentity::new(root_id, root_path)
                .map_err(|error| error.to_string())?;
            JetGameAssetWatchEvent::new(root, logical_path, kind)
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((events, skipped_count))
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static JET_GAME_WASM_BRIDGE: std::cell::RefCell<JetGameWasmBridge> =
        std::cell::RefCell::new(JetGameWasmBridge::new());
}

#[cfg(target_arch = "wasm32")]
fn jet_game_wasm_bridge_input() -> Result<Vec<u8>, String> {
    if !jet_game_wasm_debug_data_enabled() {
        return Err("game devtools Wasm bridge is unavailable outside a development profile".to_string());
    }
    JET_GAME_WASM_BRIDGE.with(|cell| {
        let bridge = cell.borrow();
        if bridge.input.len() > jet_foundation::Devtools::JET_DEVTOOLS_MAX_ENVELOPE_BYTES {
            return Err("game devtools Wasm input exceeds the envelope byte limit".to_string());
        }
        Ok(bridge.input.clone())
    })
}

#[cfg(target_arch = "wasm32")]
fn jet_game_wasm_bridge_dispatch(
    session: &mut GameDevSession,
    timestamp_ms: u64,
) -> Result<usize, String> {
    const MAX_CONTROLS_PER_TICK: usize = 32;
    let pending = jet_devtools_command_count(&session.envelope.session_id)
        .min(MAX_CONTROLS_PER_TICK);
    let mut dispatched = 0usize;
    for _ in 0..pending {
        let Some(request) = jet_devtools_poll_game_control(&session.envelope.session_id) else {
            break;
        };
        let control = jet_game_control_request(request, session)?;
        session.dispatch_control(timestamp_ms, control)?;
        dispatched = dispatched.saturating_add(1);
    }
    Ok(dispatched)
}

#[cfg(target_arch = "wasm32")]
fn jet_game_wasm_bridge_init(timestamp_ms: u64) -> Result<(), String> {
    let bytes = jet_game_wasm_bridge_input()?;
    let values = bytes
        .split(|byte| *byte == 0)
        .map(|part| {
            String::from_utf8(part.to_vec())
                .map_err(|_| "game devtools Wasm identity must be UTF-8".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() != 4 || values.iter().any(|value| value.is_empty()) {
        return Err(
            "game devtools Wasm identity requires session, source, build, and revision".to_string(),
        );
    }
    let identity = GameDevRunIdentity::new(
        values[0].clone(),
        values[1].clone(),
        values[2].clone(),
        values[3].clone(),
    );
    let policy = JET_GAME_WASM_BRIDGE.with(|cell| cell.borrow().policy);
    let mut session =
        GameDevSession::new_with_policy(identity, timestamp_ms, policy)
            .map_err(|error| error.to_string())?;
    let root = JetGameAssetRootIdentity::new("game", ".").map_err(|error| error.to_string())?;
    let roots = JetGameAssetRootSet::new(vec![root]).map_err(|error| error.to_string())?;
    session.configure_asset_roots(roots)?;
    JET_GAME_WASM_BRIDGE.with(|cell| {
        let mut bridge = cell.borrow_mut();
        bridge.session = Some(session);
        bridge.sent_sequence = 0;
        bridge.error.clear();
        bridge.take_wire()
    })
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_game_devtools_wasm_input_alloc(length: u32) -> u32 {
    if !jet_game_wasm_debug_data_enabled() {
        return 0;
    }
    JET_GAME_WASM_BRIDGE.with(|cell| {
        let mut bridge = cell.borrow_mut();
        bridge.input.resize(length as usize, 0);
        bridge.input.as_mut_ptr() as u32
    })
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_game_devtools_wasm_init(timestamp_ms: u64) -> u32 {
    if !jet_game_wasm_debug_data_enabled() {
        return 0;
    }
    let result = jet_game_wasm_bridge_init(timestamp_ms);
    JET_GAME_WASM_BRIDGE.with(|cell| {
        let mut bridge = cell.borrow_mut();
        match result {
            Ok(()) => 1,
            Err(error) => {
                bridge.set_error(error);
                0
            }
        }
    })
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_game_devtools_wasm_submit(
    _length: u32,
    timestamp_ms: u64,
) -> u32 {
    if !jet_game_wasm_debug_data_enabled() {
        return 0;
    }
    let bytes = match jet_game_wasm_bridge_input() {
        Ok(bytes) => bytes,
        Err(error) => {
            JET_GAME_WASM_BRIDGE.with(|cell| cell.borrow_mut().set_error_with_wire(error));
            return 0;
        }
    };
    let frame = match String::from_utf8(bytes) {
        Ok(frame) => frame,
        Err(_) => {
            JET_GAME_WASM_BRIDGE.with(|cell| {
                cell.borrow_mut()
                    .set_error_with_wire("game devtools command frame must be UTF-8")
            });
            return 0;
        }
    };
    let result = JET_GAME_WASM_BRIDGE.with(|cell| {
        let mut bridge = cell.borrow_mut();
        let Some(session) = bridge.session.as_mut() else {
            return Err("game devtools Wasm bridge is not initialized".to_string());
        };
        jet_game_ingest_devtools_command_frame(&frame, &session.envelope.session_id)?;
        jet_game_wasm_bridge_dispatch(session, timestamp_ms)?;
        bridge.take_wire()
    });
    JET_GAME_WASM_BRIDGE.with(|cell| {
        let mut bridge = cell.borrow_mut();
        match result {
            Ok(()) => 1,
            Err(error) => {
                bridge.set_error_with_wire(error);
                0
            }
        }
    })
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_game_devtools_wasm_asset_watch(
    _length: u32,
    timestamp_ms: u64,
) -> u32 {
    if !jet_game_wasm_debug_data_enabled() {
        return 0;
    }
    let bytes = match jet_game_wasm_bridge_input() {
        Ok(bytes) => bytes,
        Err(error) => {
            JET_GAME_WASM_BRIDGE.with(|cell| cell.borrow_mut().set_error_with_wire(error));
            return 0;
        }
    };
    let frame = match String::from_utf8(bytes) {
        Ok(frame) => frame,
        Err(_) => {
            JET_GAME_WASM_BRIDGE.with(|cell| {
                cell.borrow_mut()
                    .set_error_with_wire("game asset watch frame must be UTF-8")
            });
            return 0;
        }
    };
    let result = JET_GAME_WASM_BRIDGE.with(|cell| {
        let mut bridge = cell.borrow_mut();
        let Some(session) = bridge.session.as_mut() else {
            return Err("game devtools Wasm bridge is not initialized".to_string());
        };
        let (events, skipped_count) =
            jet_game_asset_watch_events(&frame, &session.envelope.session_id)?;
        session.ingest_asset_watch_and_import_publish_with_skipped(
            timestamp_ms,
            &events,
            skipped_count,
        )?;
        bridge.take_wire()
    });
    JET_GAME_WASM_BRIDGE.with(|cell| {
        let mut bridge = cell.borrow_mut();
        match result {
            Ok(()) => 1,
            Err(error) => {
                bridge.set_error_with_wire(error);
                0
            }
        }
    })
}


#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_game_devtools_wasm_begin_frame() -> u32 {
    if !jet_game_wasm_debug_data_enabled() {
        return 0;
    }
    JET_GAME_WASM_BRIDGE.with(|cell| {
        let mut bridge = cell.borrow_mut();
        let Some(session) = bridge.session.as_mut() else {
            bridge.set_error("game devtools Wasm bridge is not initialized");
            return 0;
        };
        match session.begin_simulation_frame() {
            true => 1,
            false => 0,
        }
    })
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_game_devtools_wasm_finish_frame(
    timestamp_ms: u64,
    frame_index: u64,
) -> u32 {
    if !jet_game_wasm_debug_data_enabled() {
        return 0;
    }
    let result = JET_GAME_WASM_BRIDGE.with(|cell| {
        let mut bridge = cell.borrow_mut();
        let Some(session) = bridge.session.as_mut() else {
            return Err("game devtools Wasm bridge is not initialized".to_string());
        };
        session.finish_simulation_frame(timestamp_ms, frame_index)?;
        bridge.take_wire()
    });
    JET_GAME_WASM_BRIDGE.with(|cell| {
        let mut bridge = cell.borrow_mut();
        match result {
            Ok(()) => 1,
            Err(error) => {
                bridge.set_error_with_wire(error);
                0
            }
        }
    })
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_game_devtools_wasm_stop(
    _length: u32,
    timestamp_ms: u64,
) -> u32 {
    if !jet_game_wasm_debug_data_enabled() {
        return 0;
    }
    let bytes = match jet_game_wasm_bridge_input() {
        Ok(bytes) => bytes,
        Err(error) => {
            JET_GAME_WASM_BRIDGE.with(|cell| cell.borrow_mut().set_error_with_wire(error));
            return 0;
        }
    };
    let reason = match String::from_utf8(bytes) {
        Ok(reason) => reason,
        Err(_) => {
            JET_GAME_WASM_BRIDGE.with(|cell| {
                cell.borrow_mut()
                    .set_error_with_wire("game devtools stop reason must be UTF-8")
            });
            return 0;
        }
    };
    let result = JET_GAME_WASM_BRIDGE.with(|cell| {
        let mut bridge = cell.borrow_mut();
        let Some(session) = bridge.session.as_mut() else {
            return Err("game devtools Wasm bridge is not initialized".to_string());
        };
        session.stop(timestamp_ms, reason)?;
        bridge.take_wire()
    });
    JET_GAME_WASM_BRIDGE.with(|cell| {
        let mut bridge = cell.borrow_mut();
        match result {
            Ok(()) => 1,
            Err(error) => {
                bridge.set_error_with_wire(error);
                0
            }
        }
    })
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_game_devtools_wasm_frame(
    _length: u32,
    timestamp_ms: u64,
    frame_index: u64,
    cpu_ns: u64,
) -> u32 {
    if !jet_game_wasm_debug_data_enabled() {
        return 0;
    }
    let bytes = match jet_game_wasm_bridge_input() {
        Ok(bytes) => bytes,
        Err(error) => {
            JET_GAME_WASM_BRIDGE.with(|cell| cell.borrow_mut().set_error_with_wire(error));
            return 0;
        }
    };
    let scene = match String::from_utf8(bytes) {
        Ok(scene) if !scene.is_empty() => scene,
        Ok(_) => {
            JET_GAME_WASM_BRIDGE.with(|cell| {
                cell.borrow_mut()
                    .set_error_with_wire("game frame scene must be non-empty")
            });
            return 0;
        }
        Err(_) => {
            JET_GAME_WASM_BRIDGE.with(|cell| {
                cell.borrow_mut()
                    .set_error_with_wire("game frame scene must be UTF-8")
            });
            return 0;
        }
    };
    let result = JET_GAME_WASM_BRIDGE.with(|cell| {
        let mut bridge = cell.borrow_mut();
        let Some(session) = bridge.session.as_mut() else {
            return Err("game devtools Wasm bridge is not initialized".to_string());
        };
        let identity = session.profiler.identity();
        let frame = JetGameFrameIdentity::new(
            scene,
            frame_index,
            identity.build.clone(),
            identity.revision.clone(),
            identity.source.clone(),
        );
        let sample = JetGameFrameSample::new(
            frame,
            timestamp_ms.saturating_mul(1_000_000),
            cpu_ns,
            None,
        );
        session.record_and_publish(JetGameSampleFact::Frame(sample))?;
        bridge.take_wire()
    });
    JET_GAME_WASM_BRIDGE.with(|cell| {
        let mut bridge = cell.borrow_mut();
        match result {
            Ok(()) => 1,
            Err(error) => {
                bridge.set_error_with_wire(error);
                0
            }
        }
    })
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_game_devtools_wasm_output_ptr() -> u32 {
    if !jet_game_wasm_debug_data_enabled() {
        return 0;
    }
    JET_GAME_WASM_BRIDGE.with(|cell| cell.borrow().output.as_ptr() as u32)
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_game_devtools_wasm_output_len() -> u32 {
    if !jet_game_wasm_debug_data_enabled() {
        return 0;
    }
    JET_GAME_WASM_BRIDGE.with(|cell| cell.borrow().output.len() as u32)
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_game_devtools_wasm_error_ptr() -> u32 {
    if !jet_game_wasm_debug_data_enabled() {
        return 0;
    }
    JET_GAME_WASM_BRIDGE.with(|cell| cell.borrow().error.as_ptr() as u32)
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_game_devtools_wasm_error_len() -> u32 {
    if !jet_game_wasm_debug_data_enabled() {
        return 0;
    }
    JET_GAME_WASM_BRIDGE.with(|cell| cell.borrow().error.len() as u32)
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_game_devtools_wasm_error_clear() {
    if !jet_game_wasm_debug_data_enabled() {
        return;
    }
    JET_GAME_WASM_BRIDGE.with(|cell| cell.borrow_mut().error.clear());
}

fn game_dev_function_identity_body(
    function: &JetGameFunctionIdentity,
) -> JetDevtoolsFunctionIdentity {
    JetDevtoolsFunctionIdentity::new(
        function.function_id.clone(),
        function.name.clone(),
        JetDevtoolsSourceSpan::new(
            function.source.source_id.clone(),
            function.source.file.clone(),
            function.source.start_line,
            function.source.start_column,
            function.source.end_line,
            function.source.end_column,
        ),
    )
}

fn game_dev_typed_trace_body(event: &GameDevEvent) -> Option<JetDevtoolsEventBody> {
    match event {
        GameDevEvent::FrameSample(sample) => Some(JetDevtoolsEventBody::GameFrameSample {
            sequence: sample.sequence,
            frame_id: sample.frame.frame_id,
            scene: sample.frame.scene.clone(),
            frame_index: sample.frame.frame_index,
            build: sample.frame.build.clone(),
            revision: sample.frame.revision.clone(),
            trace_id: sample.frame.trace_id.clone(),
            start_ns: sample.start_ns,
            cpu_ns: sample.cpu_ns,
            gpu_ns: sample.gpu_ns,
            responsible_function: sample
                .responsible_function
                .as_ref()
                .map(game_dev_function_identity_body),
        }),
        GameDevEvent::DrawEvent(event) => Some(JetDevtoolsEventBody::GameDrawEvent {
            sequence: event.sequence,
            frame_id: event.frame.frame_id,
            scene: event.frame.scene.clone(),
            frame_index: event.frame.frame_index,
            build: event.frame.build.clone(),
            revision: event.frame.revision.clone(),
            trace_id: event.frame.trace_id.clone(),
            event_index: event.event_index,
            event_id: event.event_id,
            function: game_dev_function_identity_body(&event.function),
            phase: event.phase.as_str().to_string(),
            domain: event.domain.as_str().to_string(),
            start_ns: event.start_ns,
            duration_ns: event.duration_ns,
            object_id: event.object_id.clone(),
            resource_id: event.resource_id.clone(),
        }),
        _ => None,
    }
}

fn game_dev_swap_body(event: &GameDevEvent) -> Option<JetDevtoolsEventBody> {
    let GameDevEvent::GameSwap(outcome) = event else {
        return None;
    };
    Some(JetDevtoolsEventBody::GameSwap {
        sequence: outcome.sequence,
        path: outcome.fact.path.clone(),
        kind: outcome.fact.kind.as_str().to_string(),
        status: outcome.status.as_str().to_string(),
        old_schema_id: outcome.fact.old_schema_id.clone(),
        new_schema_id: outcome.fact.new_schema_id.clone(),
        migration: outcome.fact.migration.as_str().to_string(),
        reason: outcome.fact.reason.clone(),
    })
}

fn game_dev_eval_result(request_id: String, status: &str) -> JetGamePausedEvalResult {
    JetGamePausedEvalResult {
        request_id,
        status: status.to_string(),
        type_name: String::new(),
        published_value: None,
        source_span: None,
    }
}

fn game_dev_json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                output.push_str(&format!("\\u{:04x}", character as u32))
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

fn game_dev_json_option(value: Option<&str>) -> String {
    value
        .map(game_dev_json_string)
        .unwrap_or_else(|| "null".to_string())
}

fn game_dev_function_identity_json(function: &JetGameFunctionIdentity) -> String {
    format!(
        "{{\"function_id\":{},\"name\":{},\"source\":{}}}",
        game_dev_json_string(&function.function_id),
        game_dev_json_string(&function.name),
        game_dev_source_span_json(&function.source),
    )
}

fn game_dev_source_span_json(source: &JetGameSourceSpan) -> String {
    format!(
        "{{\"source_id\":{},\"file\":{},\"start_line\":{},\"start_column\":{},\
\"end_line\":{},\"end_column\":{}}}",
        game_dev_json_string(&source.source_id),
        game_dev_json_string(&source.file),
        source.start_line,
        source.start_column,
        source.end_line,
        source.end_column,
    )
}
fn game_dev_frame_identity_json(frame: &JetGameFrameIdentity) -> String {
    format!(
        "{{\"frame_id\":{},\"scene\":{},\"frame_index\":{},\"build\":{},\
\"revision\":{},\"trace_id\":{}}}",
        frame.frame_id,
        game_dev_json_string(&frame.scene),
        frame.frame_index,
        game_dev_json_string(&frame.build),
        game_dev_json_string(&frame.revision),
        game_dev_json_string(&frame.trace_id),
    )
}

fn game_profile_snapshot_json(snapshot: &GameProfileSnapshot) -> String {
    let assets = snapshot
        .identity
        .assets
        .iter()
        .map(JetGameAssetNodeIdentity::render_json)
        .collect::<Vec<_>>()
        .join(",");
    let hot_paths = snapshot
        .projection
        .hot_paths
        .iter()
        .map(|path| {
            format!(
                "{{\"function_id\":{},\"domain\":{},\"sample_count\":{},\
\"frame_count\":{},\"total_ns\":{},\"max_ns\":{}}}",
                game_dev_json_string(&path.function.function_id),
                game_dev_json_string(path.domain.as_str()),
                path.sample_count,
                path.frame_count,
                path.total_ns,
                path.max_ns,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"identity\":{{\"scene\":{},\"build\":{},\"target\":{},\"assets\":[{}]}},\
\"run\":{{\"session\":{},\"source\":{},\"build\":{},\"revision\":{}}},\
\"trace_identity\":{{\"build\":{},\"revision\":{},\"trace_id\":{}}},\
\"projection\":{{\"retained_samples\":{},\"capacity\":{},\"dropped_samples\":{},\
\"total_function_ns\":{},\"hot_paths\":[{}]}}}}",
        game_dev_json_string(&snapshot.identity.scene),
        game_dev_json_string(&snapshot.identity.build),
        game_dev_json_string(&snapshot.identity.target),
        assets,
        game_dev_json_string(&snapshot.run.session),
        game_dev_json_string(&snapshot.run.source),
        game_dev_json_string(&snapshot.run.build),
        game_dev_json_string(&snapshot.run.revision),
        game_dev_json_string(&snapshot.trace_identity.build),
        game_dev_json_string(&snapshot.trace_identity.revision),
        game_dev_json_string(&snapshot.trace_identity.trace_id),
        snapshot.projection.retained_samples,
        snapshot.projection.capacity,
        snapshot.projection.dropped_samples,
        snapshot.projection.total_function_ns,
        hot_paths,
    )
}
