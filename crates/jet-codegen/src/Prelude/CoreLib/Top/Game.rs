// -- core.game headless substrate (D-GAME1/2/3, D-WD10, D-GAME-*) -------------
fn jet_game_identity_from_env(key: &str, fallback: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

#[derive(Debug)]
struct GameState {
    assets: Vec<(String, String)>,
    bindings: Vec<(String, String)>,
    components: Vec<String>,
    /// Asset bytes and handles are host-supplied checked facts.  The headless
    /// Core adapter does not inspect paths or read files.
    asset_bytes: u64,
    asset_roots: JetGameAssetRootSet,
    asset_handles: std::collections::BTreeMap<String, GameAssetRuntimeHandle>,
    dev_session: Option<GameDevSession>,
}

impl GameState {
    fn new(scene_name: &str) -> Self {
        let root = JetGameAssetRootIdentity::new("game", ".")
            .expect("the built-in game asset root is valid");
        let asset_roots =
            JetGameAssetRootSet::new(vec![root]).expect("the built-in game asset root is unique");
        let dev_session = if JET_GAME_DEBUG_DATA_ENABLED {
            let fallback_source = if scene_name.is_empty() {
                "game"
            } else {
                scene_name
            };
            let session = jet_game_identity_from_env(
                "JET_DEVTOOLS_RELAY_SESSION_ID",
                &format!("game:{scene_name}"),
            );
            let source = jet_game_identity_from_env("JET_DEVTOOLS_RELAY_SOURCE_ID", fallback_source);
            let build = jet_game_identity_from_env("JET_DEVTOOLS_RELAY_BUILD_ID", "runtime");
            let revision = jet_game_identity_from_env("JET_DEVTOOLS_RELAY_REVISION", "runtime");
            let identity = GameDevRunIdentity::new(session, source, build, revision);
            let mut dev_session = GameDevSession::new_with_policy(
                identity,
                0,
                GameDevPolicy::new(JET_GAME_DEBUG_DATA_ENABLED),
            )
            .expect("the built-in game dev session is valid");
            jet_game_install_devtools_callback();
            dev_session
                .register_gameplay_debugger(0, "dev")
                .expect("the built-in gameplay debugger extension is valid");
            dev_session
                .configure_asset_roots(asset_roots.clone())
                .expect("the built-in game asset roots are valid");
            Some(dev_session)
        } else {
            None
        };
        Self {
            assets: Vec::new(),
            bindings: Vec::new(),
            components: Vec::new(),
            asset_bytes: 0,
            asset_roots,
            asset_handles: std::collections::BTreeMap::new(),
            dev_session,
        }
    }
}
impl Drop for GameState {
    fn drop(&mut self) {
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);
        if let Some(dev_session) = self.dev_session.as_mut() {
            let _ = dev_session.stop(timestamp_ms, "game state dropped");
        }
    }
}

impl Default for GameState {
    fn default() -> Self {
        Self::new("game")
    }
}

#[derive(Clone)]
struct GameAssets {
    state: std::rc::Rc<std::cell::RefCell<GameState>>,
}

#[derive(Clone)]
struct GameInputMap {
    state: std::rc::Rc<std::cell::RefCell<GameState>>,
}

struct GameScene {
    name: String,
    assets: GameAssets,
    user_assets: GameAssets,
    input: GameInputMap,
    user_input: GameInputMap,
    callbacks: std::rc::Rc<std::cell::RefCell<Vec<GameFrameCallback>>>,
}
const MAX_GAME_SCENE_CALLBACKS: usize = 256;

struct GameFrameCallback {
    callback: Box<dyn FnMut(GameFrame)>,
    schedule: Option<jet_foundation::ResourceSchedule::JetFrameSchedule>,
    derivation: Option<String>,
    completion: Option<
        std::sync::Arc<
            std::sync::Mutex<jet_foundation::ResourceSchedule::JetFrameCompletionState>,
        >,
    >,
}

#[derive(Clone, Debug)]
struct GameImage {
    path: String,
    runtime: Option<GameAssetRuntimeHandle>,
}

#[derive(Clone, Debug)]
struct GameSound {
    path: String,
    runtime: Option<GameAssetRuntimeHandle>,
}

#[derive(Clone, Debug)]
struct GameAssetRuntimePayload {
    artifact: JetGameAssetArtifactIdentity,
    bytes: std::sync::Arc<[u8]>,
    metadata: String,
}

#[derive(Clone, Debug)]
struct GameAssetRuntimeHandle {
    payload: std::sync::Arc<std::sync::RwLock<GameAssetRuntimePayload>>,
}

impl GameAssetRuntimeHandle {
    fn new(payload: GameAssetRuntimePayload) -> Self {
        Self {
            payload: std::sync::Arc::new(std::sync::RwLock::new(payload)),
        }
    }

    fn byte_len(&self) -> u64 {
        self.payload
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .bytes
            .len() as u64
    }

    fn replace(&self, payload: GameAssetRuntimePayload) -> u64 {
        let mut current = self
            .payload
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = current.bytes.len() as u64;
        *current = payload;
        previous
    }
}
#[derive(Clone, Debug)]
struct GameReplay {
    path: String,
    tape: JetGameReplay,
}

#[derive(Clone, Debug)]
struct GameBackend {
    renderer: String,
    audio: String,
    editor: String,
    /// The shared plan is the only owner of default, validation, and
    /// countdown state. `None` inside the plan means a live/unbounded backend.
    frame_budget: JetGameFrameBudget,
}

#[derive(Clone, Debug)]
struct GameInputSnapshot {
    pressed: Vec<String>,
}

#[derive(Clone, Debug)]
struct GameFrame {
    index: i64,
    user_index: i64,
    input: GameInputSnapshot,
    user_input: GameInputSnapshot,
}

impl JetShow for GameScene {
    fn jet_show(&self) -> String {
        format!("GameScene({})", self.name)
    }
}
impl JetShow for GameAssets {
    fn jet_show(&self) -> String {
        "GameAssets".to_string()
    }
}
impl JetShow for GameInputMap {
    fn jet_show(&self) -> String {
        "GameInputMap".to_string()
    }
}
impl JetShow for GameImage {
    fn jet_show(&self) -> String {
        format!("GameImage({})", self.path)
    }
}
impl JetDebug for GameImage {
    fn jet_debug(&self) -> String {
        self.jet_show()
    }
}
impl JetShow for GameSound {
    fn jet_show(&self) -> String {
        format!("GameSound({})", self.path)
    }
}
impl JetDebug for GameSound {
    fn jet_debug(&self) -> String {
        self.jet_show()
    }
}
impl JetShow for GameReplay {
    fn jet_show(&self) -> String {
        format!("GameReplay({})", self.path)
    }
}
impl JetShow for GameBackend {
    fn jet_show(&self) -> String {
        format!(
            "GameBackend(renderer: {}, audio: {}, editor: {})",
            self.renderer, self.audio, self.editor
        )
    }
}
impl JetShow for GameInputSnapshot {
    fn jet_show(&self) -> String {
        format!("GameInputSnapshot({})", self.pressed.join(","))
    }
}
impl JetShow for GameFrame {
    fn jet_show(&self) -> String {
        format!("GameFrame({})", self.index)
    }
}

fn jet_game_scene_new(name: &String) -> GameScene {
    let state = std::rc::Rc::new(std::cell::RefCell::new(GameState::new(name)));
    let assets = GameAssets {
        state: state.clone(),
    };
    let input = GameInputMap {
        state: state.clone(),
    };
    GameScene {
        name: name.clone(),
        assets: assets.clone(),
        user_assets: assets,
        input: input.clone(),
        user_input: input,
        callbacks: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
    }
}

fn jet_game_replay_record(path: &String) -> GameReplay {
    let tape = JetGameReplay::from_path(path)
        .unwrap_or_else(|error| panic!("invalid game replay `{path}`: {error}"));
    GameReplay {
        path: path.clone(),
        tape,
    }
}

fn jet_game_backend_headless() -> GameBackend {
    GameBackend {
        renderer: "headless".to_string(),
        audio: "none".to_string(),
        editor: "none".to_string(),
        frame_budget: JetGameFrameBudget::default(),
    }
}

/// D-GAME-LOOP1=A: delegate frame availability and presentation to the
/// canonical Foundation plan.
fn jet_game_backend_should_continue(backend: &GameBackend) -> bool {
    backend.frame_budget.should_continue()
}

fn jet_game_backend_present(backend: &mut GameBackend) {
    backend.frame_budget.present();
}


fn jet_game_scene_on_frame(
    scene: &mut GameScene,
    f: Box<dyn FnMut(GameFrame)>,
    schedule_json: Option<&str>,
    derivation_id: Option<&str>,
) {
    let schedule = schedule_json.map(|payload| {
        jet_foundation::ResourceSchedule::JetFrameSchedule::from_canonical_json(payload)
            .unwrap_or_else(|error| panic!("invalid checked game frame schedule: {error}"))
    });
    if schedule.is_some() && derivation_id.is_none() {
        panic!("checked game frame schedule has no canonical derivation");
    }
    let mut callbacks = scene.callbacks.borrow_mut();
    if callbacks.len() >= MAX_GAME_SCENE_CALLBACKS {
        panic!("game scene callback limit exceeded");
    }
    callbacks.push(GameFrameCallback {
        completion: schedule.as_ref().map(|schedule| {
            std::sync::Arc::new(std::sync::Mutex::new(
                jet_foundation::ResourceSchedule::JetFrameCompletionState::new(schedule),
            ))
        }),
        callback: f,
        schedule,
        derivation: derivation_id.map(str::to_string),
    });
}

fn jet_game_execute_frame_callback(callback: &mut GameFrameCallback, frame: GameFrame) {
    if let Some(schedule) = &callback.schedule {
        if schedule
            .operations
            .windows(2)
            .any(|window| window[0].source_index > window[1].source_index)
        {
            panic!("checked game frame schedule is not in source order");
        }
        if callback.derivation.is_none() {
            panic!("checked game frame schedule has no canonical derivation");
        }
    }
    let completion = callback.completion.clone();
    if let Some(completion) = completion {
        jet_foundation::ResourceSchedule::with_frame_completion_scope(completion, || {
            (callback.callback)(frame)
        });
    } else {
        (callback.callback)(frame);
    }
    if let Some(schedule) = &callback.schedule {
        let completion = callback
            .completion
            .as_ref()
            .expect("checked game frame schedule has no completion state")
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        completion
            .assert_reuse_ready(schedule)
            .unwrap_or_else(|error| panic!("checked game frame reuse before completion: {error}"));
    }
}
pub(crate) fn jet_game_frame_schedule_explain(
    schedule: &jet_foundation::ResourceSchedule::JetFrameSchedule,
) -> String {
    schedule.explain()
}

fn jet_game_scene_component(scene: &mut GameScene, name: &String) {
    let mut state = scene.assets.state.borrow_mut();
    if !state.components.iter().any(|existing| existing == name) {
        state.components.push(name.clone());
    }
}

/// D-GAME-LOOP1=A: return one synthetic entity row of component *data*
/// (default-initialized Int fields), not bare type-name markers.
fn jet_game_scene_query(scene: &GameScene, names: &String) -> Vec<String> {
    let state = scene.assets.state.borrow();
    let wanted: Vec<&str> = names.split(',').filter(|s| !s.is_empty()).collect();
    if wanted
        .iter()
        .all(|name| state.components.iter().any(|c| c == name))
    {
        let row = wanted
            .iter()
            .map(|name| match *name {
                "Position" => "Position{x:0}".to_string(),
                "Velocity" => "Velocity{dx:0}".to_string(),
                other => format!("{other}{{}}"),
            })
            .collect::<Vec<_>>()
            .join(",");
        vec![row]
    } else {
        Vec::new()
    }
}

fn jet_game_assets_image(assets: &GameAssets, path: &String) -> Result<GameImage, String> {
    let resolved = assets
        .state
        .borrow()
        .asset_roots
        .resolve(path.clone())
        .map_err(|error| error.to_string())?;
    let asset_key = resolved.node().key();
    let logical_path = resolved.logical_path;
    let mut state = assets.state.borrow_mut();
    let runtime = state.asset_handles.get(&asset_key).cloned();
    state
        .assets
        .push(("image".to_string(), logical_path.clone()));
    Ok(GameImage {
        path: logical_path,
        runtime,
    })
}

fn jet_game_assets_sound(assets: &GameAssets, path: &String) -> Result<GameSound, String> {
    let resolved = assets
        .state
        .borrow()
        .asset_roots
        .resolve(path.clone())
        .map_err(|error| error.to_string())?;
    let asset_key = resolved.node().key();
    let logical_path = resolved.logical_path;
    let mut state = assets.state.borrow_mut();
    let runtime = state.asset_handles.get(&asset_key).cloned();
    state
        .assets
        .push(("sound".to_string(), logical_path.clone()));
    Ok(GameSound {
        path: logical_path,
        runtime,
    })
}
/// Apply one importer-checked runtime fact.  The core game layer never reads
/// paths or chooses an importer: the canonical plan/outcome/fact are validated
/// by the dev session before this handle becomes visible to image or sound.
fn jet_game_assets_reload(
    assets: &GameAssets,
    timestamp_ms: u64,
    plan: JetGameAssetImportPlan,
    outcome: JetGameAssetImportOutcome,
    fact: JetGameAssetRuntimeFact,
) -> Result<JetGameAssetTransactionReceipt, String> {
    if !JET_GAME_DEBUG_DATA_ENABLED {
        return Err("game asset reload is unavailable outside a development profile".to_string());
    }
    let asset_key = fact.artifact.source.node().key();
    let byte_len = fact.byte_len();
    let receipt = {
        let mut state = assets.state.borrow_mut();
        let dev_session = state
            .dev_session
            .as_mut()
            .ok_or_else(|| "game development data is disabled for this profile".to_string())?;
        dev_session.reload_asset_with_fact(timestamp_ms, plan, outcome, &fact)?
    };
    if !receipt.is_committed()
        || !receipt.entries.iter().any(|entry| {
            matches!(
                entry.status,
                JetGameAssetReloadStatus::Applied | JetGameAssetReloadStatus::Unchanged
            )
        })
    {
        return Ok(receipt);
    }

    let payload = GameAssetRuntimePayload {
        artifact: fact.artifact,
        bytes: std::sync::Arc::from(fact.bytes),
        metadata: fact.metadata,
    };
    let mut state = assets.state.borrow_mut();
    let previous_len = if let Some(existing) = state.asset_handles.get(&asset_key) {
        existing.replace(payload)
    } else {
        state
            .asset_handles
            .insert(asset_key, GameAssetRuntimeHandle::new(payload));
        0
    };
    state.asset_bytes = state
        .asset_bytes
        .saturating_sub(previous_len)
        .saturating_add(byte_len);
    Ok(receipt)
}


fn jet_game_input_bind(input: &GameInputMap, action: &String, key: &String) {
    let mut state = input.state.borrow_mut();
    if !state.bindings.iter().any(|(a, k)| a == action && k == key) {
        state.bindings.push((action.clone(), key.clone()));
    }
}

fn jet_game_input_pressed(input: &GameInputSnapshot, action: &String) -> bool {
    input.pressed.iter().any(|a| a == action)
}

/// Read peak RSS (VmHWM) from /proc/self/status in bytes.
/// Returns 0 if unavailable (non-Linux or parse failure).
fn read_vmhwm_bytes() -> u64 {
    let Ok(text) = std::fs::read_to_string("/proc/self/status") else { return 0 };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            // Value is in kibibytes: "VmHWM:    1234 kB"
            if let Some(kib) = rest.split_whitespace().next().and_then(|v| v.parse::<u64>().ok()) {
                return kib.saturating_mul(1024);
            }
        }
    }
    0
}

/// Replay consumption is owned by the shared Foundation tape. A missing frame
/// means that no action was recorded for that frame; it is not a synthetic
/// input.
fn jet_game_replay_input(
    replay: Option<&GameReplay>,
    frame_idx: i64,
    _bindings: &[(String, String)],
    replay_cursor: &mut i64,
) -> Vec<String> {
    *replay_cursor = (*replay_cursor).saturating_add(1);
    replay
        .and_then(|replay| replay.tape.actions_at(frame_idx))
        .map(|actions| actions.to_vec())
        .unwrap_or_default()
}

/// D-FOUND-COREAPI1=A: run a checked, deterministic frame budget through the
/// shared Prelude kernel. The Foundation plan owns validation and iteration;
/// this adapter only projects its facts into the game transcript.
fn jet_game_run(
    scene: &mut GameScene,
    replay: Option<&GameReplay>,
    backend: Option<&GameBackend>,
    frames: Option<&i64>,
) -> String {
    if let Some(frames) = frames {
        JetGameFrameBudget::validate(*frames).unwrap_or_else(|error| panic!("{error}"));
    }
    let mut backend = backend.cloned().unwrap_or_else(jet_game_backend_headless);
    backend
        .frame_budget
        .set_requested(frames.copied())
        .unwrap_or_else(|error| panic!("{error}"));
    let replay_path = replay
        .map(|r| r.path.clone())
        .unwrap_or_else(|| "<none>".to_string());
    // A probe matches this scene, pins identity, then runs 120 warmup plus
    // 600 measured frames and emits JETSCENE1 wire rows to stdout.
    let probe_name = std::env::var("JET_SCENE_PROBE").ok();
    let measuring = probe_name.as_deref() == Some(scene.name.as_str());

    let asset_bytes = scene.assets.state.borrow().asset_bytes;
    let warmup_frames: i64 = 120;
    let measure_frames: i64 = 600;
    let total_frames = warmup_frames + measure_frames;
    let mut frame_plan = if measuring {
        JetGameFrameBudget::with_frame_count(total_frames)
            .expect("the measured game frame plan must be positive")
    } else {
        backend.frame_budget.clone()
    };

    let mut out = Vec::new();
    if !measuring {
        out.push(format!("scene:{}", scene.name));
        out.push(format!(
            "backend:{}/{}/{}",
            backend.renderer, backend.audio, backend.editor
        ));
        out.push(format!("replay:{}", replay_path));
        {
            let state = scene.assets.state.borrow();
            let assets = state
                .assets
                .iter()
                .map(|(kind, path)| format!("{}:{}", kind, path))
                .collect::<Vec<_>>()
                .join(",");
            out.push(format!(
                "assets:{}",
                if assets.is_empty() { "none".to_string() } else { assets }
            ));
            let bindings = state
                .bindings
                .iter()
                .map(|(action, key)| format!("{}={}", action, key))
                .collect::<Vec<_>>()
                .join(",");
            out.push(format!(
                "input:{}",
                if bindings.is_empty() { "none".to_string() } else { bindings }
            ));
            let components = state.components.join(",");
            out.push(format!(
                "components:{}",
                if components.is_empty() { "none".to_string() } else { components }
            ));
        }
    }

    // Hex-encode scene name for the wire protocol (avoids tab/newline issues).
    let scene_hex: String = scene.name.bytes().map(|b| format!("{:02x}", b)).collect();

    if measuring {
        // D-PERFBUDGET-GAMEMIGRATE1: one SceneProbe pins backend, target,
        // device, replay/input, scene-ready, 120 warmup, 600 measured,
        // viewport, and settings before any sample row.
        println!(
            "JETSCENEID\t{scene_hex}\tbackend={}/{}/{}\treplay={}\tdevice=headless\tviewport=default\tsettings=default\twarmup={warmup_frames}\tmeasured={measure_frames}\tready=frame0",
            backend.renderer, backend.audio, backend.editor, replay_path
        );
    }
    if JET_GAME_DEBUG_DATA_ENABLED {
        let mut state = scene.assets.state.borrow_mut();
        let dev_session = state
            .dev_session
            .as_mut()
            .expect("development game session must exist when debug data is enabled");
        let identity = dev_session.profiler.identity().clone();
        dev_session
            .set_trace_context(JetGameTraceContext::new(
                identity.session,
                scene.name.clone(),
                "runtime",
                format!("{}/{}/{}", backend.renderer, backend.audio, backend.editor),
                1,
                1,
                "default",
            ))
            .expect("the game trace context must satisfy the profiler identity");
        dev_session
            .dispatch_pending_devtools_controls(0)
            .expect("game devtools command ingress must be valid");
        if dev_session.phase == GameDevPhase::Editing {
            dev_session
                .dispatch_control(0, GameDevControl::Play)
                .expect("the game devtools session must enter Play");
        }
    }

    // D-GAME-LOOP1=A: the Foundation plan controls both ordinary and probe
    // iteration. Replay consumption and transcript hashing happen once per
    // issued frame and cannot shorten the requested count.
    let mut frame_idx: i64 = 0;
    let mut replay_cursor: i64 = 0;
    let mut transcript = JetGameTranscriptHasher::new();
    while let Some(index) = frame_plan.next_frame() {
        loop {
            let admitted = if JET_GAME_DEBUG_DATA_ENABLED {
                let mut state = scene.assets.state.borrow_mut();
                let dev_session = state
                    .dev_session
                    .as_mut()
                    .expect("development game session must exist when debug data is enabled");
                dev_session
                    .dispatch_pending_devtools_controls(frame_idx as u64)
                    .expect("game devtools command ingress must be valid");
                dev_session.begin_simulation_frame()
            } else {
                true
            };
            if admitted {
                break;
            }
            // A paused session keeps this frame in flight until the host
            // supplies Resume or the one-shot FrameAdvance command.
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let pressed = {
            let state = scene.assets.state.borrow();
            jet_game_replay_input(replay, frame_idx, &state.bindings, &mut replay_cursor)
        };
        transcript.push_frame(frame_idx, &pressed);
        let frame = GameFrame {
            index: frame_idx,
            user_index: frame_idx,
            input: GameInputSnapshot {
                pressed: pressed.clone(),
            },
            user_input: GameInputSnapshot {
                pressed: pressed.clone(),
            },
        };
        let callbacks_count = scene.callbacks.borrow().len() as u64;
        let t0 = std::time::Instant::now();
        // The headless adapter is the serial reference execution: callback
        // registration order is source order, and it makes no physical
        // parallel/reuse decision from model-only facts.
        if measuring {
            for callback in scene.callbacks.borrow_mut().iter_mut() {
                jet_game_execute_frame_callback(callback, frame.clone());
            }
        } else {
            for callback in scene.callbacks.borrow_mut().iter_mut() {
                jet_game_execute_frame_callback(callback, frame.clone());
            }
            let input = if pressed.is_empty() {
                "none".to_string()
            } else {
                pressed.join("+")
            };
            out.push(format!("frame:{} input:{}", frame_idx, input));
        }
        let elapsed_ns = t0.elapsed().as_nanos() as u64;
        let rss = read_vmhwm_bytes();
        if measuring && frame_idx >= warmup_frames {
            println!("JETSCENE1\t{scene_hex}\tFrameTime\t{elapsed_ns}");
            println!("JETSCENE1\t{scene_hex}\tDrawCalls\t{callbacks_count}");
            println!("JETSCENE1\t{scene_hex}\tSceneAssetBytes\t{asset_bytes}");
            println!("JETSCENE1\t{scene_hex}\tMemoryHighWater\t{rss}");
        }

        if JET_GAME_DEBUG_DATA_ENABLED {
            let identity = {
                let state = scene.assets.state.borrow();
                state
                    .dev_session
                    .as_ref()
                    .expect("development game session must exist when debug data is enabled")
                    .profiler
                    .identity()
                    .clone()
            };
            let frame_identity = JetGameFrameIdentity::new(
                &scene.name,
                frame_idx as u64,
                identity.build,
                identity.revision,
                identity.source,
            );
            let sample = JetGameFrameSample::new(
                frame_identity,
                frame_idx as u64,
                elapsed_ns,
                None,
            );
            let mut state = scene.assets.state.borrow_mut();
            let dev_session = state
                .dev_session
                .as_mut()
                .expect("development game session must exist when debug data is enabled");
            if dev_session.profiler.profiler().state() == JetGameFrameProfilerState::Running {
                dev_session
                    .record_and_publish(sample.into())
                    .expect("the game frame producer must satisfy the profiler identity");
                let sample_time_ns = frame_idx as u64;
                let event_time_ms = frame_idx as u64;
                dev_session
                    .publish_overlay_metric(
                        event_time_ms,
                        JetGameOverlayMetricValue::new(
                            JetGameOverlayMetric::FrameTime,
                            elapsed_ns.min(i64::MAX as u64) as i64,
                            sample_time_ns,
                        ),
                    )
                    .expect("the game frame metric must publish");
                dev_session
                    .publish_overlay_metric(
                        event_time_ms,
                        JetGameOverlayMetricValue::new(
                            JetGameOverlayMetric::DrawCalls,
                            callbacks_count.min(i64::MAX as u64) as i64,
                            sample_time_ns,
                        ),
                    )
                    .expect("the draw-call metric must publish");
                if asset_bytes != 0 {
                    dev_session
                        .publish_overlay_metric(
                            event_time_ms,
                            JetGameOverlayMetricValue::new(
                                JetGameOverlayMetric::SceneAssetBytes,
                                asset_bytes.min(i64::MAX as u64) as i64,
                                sample_time_ns,
                            ),
                        )
                        .expect("the scene asset metric must publish");
                }
                if rss != 0 {
                    dev_session
                        .publish_overlay_metric(
                            event_time_ms,
                            JetGameOverlayMetricValue::new(
                                JetGameOverlayMetric::MemoryHighWater,
                                rss.min(i64::MAX as u64) as i64,
                                sample_time_ns,
                            ),
                        )
                        .expect("the memory metric must publish");
                }
            }
        }
        if JET_GAME_DEBUG_DATA_ENABLED {
            let mut state = scene.assets.state.borrow_mut();
            state
                .dev_session
                .as_mut()
                .expect("development game session must exist when debug data is enabled")
                .finish_simulation_frame(frame_idx as u64, frame_idx as u64)
                .expect("the game frame must satisfy the devtools phase");
        }
        frame_plan.present();
    }
    if JET_GAME_DEBUG_DATA_ENABLED {
        let mut state = scene.assets.state.borrow_mut();
        state
            .dev_session
            .as_mut()
            .expect("development game session must exist when debug data is enabled")
            .stop(frame_idx as u64, "game run completed")
            .expect("the game run must publish its stopped lifecycle");
    }
    if !measuring && frames.is_some() {
        out.push(format!("transcript_hash:{}", transcript.finish()));
    }
    out.join("\n")
}
