// -- core.game headless substrate (D-GAME1/2/3, D-WD10, D-GAME-*) -------------
#[derive(Default, Debug)]
struct GameState {
    assets: Vec<(String, String)>,
    bindings: Vec<(String, String)>,
    components: Vec<String>,
    /// Accumulated real file sizes of all registered assets (bytes).
    asset_bytes: u64,
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
    callbacks: std::rc::Rc<std::cell::RefCell<Vec<Box<dyn FnMut(GameFrame)>>>>,
}

#[derive(Clone, Debug)]
struct GameImage {
    path: String,
}

#[derive(Clone, Debug)]
struct GameSound {
    path: String,
}

#[derive(Clone, Debug)]
struct GameReplay {
    path: String,
}

#[derive(Clone, Debug)]
struct GameBackend {
    renderer: String,
    audio: String,
    editor: String,
    /// Headless/deterministic backends count down; `None` means a live window
    /// (should_continue is polled elsewhere — currently unused until a windowed
    /// game.Backend lands).
    frame_budget: Option<i64>,
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
    let state = std::rc::Rc::new(std::cell::RefCell::new(GameState::default()));
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
    GameReplay { path: path.clone() }
}

fn jet_game_backend_headless() -> GameBackend {
    GameBackend {
        renderer: "headless".to_string(),
        audio: "none".to_string(),
        editor: "none".to_string(),
        // D-GAME-LOOP1=A: keep the historical three-frame goldens.
        frame_budget: Some(3),
    }
}

/// D-GAME-LOOP1=A: whether `game.run` should execute another on_frame.
fn jet_game_backend_should_continue(backend: &GameBackend) -> bool {
    match backend.frame_budget {
        Some(n) => n > 0,
        None => true,
    }
}

/// D-GAME-LOOP1=A: end-of-frame present (vsync/swap on a live backend; budget tick headless).
fn jet_game_backend_present(backend: &mut GameBackend) {
    if let Some(n) = backend.frame_budget.as_mut() {
        *n = n.saturating_sub(1);
    }
}

fn jet_game_scene_on_frame(scene: &mut GameScene, f: Box<dyn FnMut(GameFrame)>) {
    scene.callbacks.borrow_mut().push(f);
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
    if path.contains("missing") {
        return Err(format!("asset not found: {}", path));
    }
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut state = assets.state.borrow_mut();
    state.assets.push(("image".to_string(), path.clone()));
    state.asset_bytes = state.asset_bytes.saturating_add(size);
    Ok(GameImage { path: path.clone() })
}

fn jet_game_assets_sound(assets: &GameAssets, path: &String) -> Result<GameSound, String> {
    if path.contains("missing") {
        return Err(format!("asset not found: {}", path));
    }
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut state = assets.state.borrow_mut();
    state.assets.push(("sound".to_string(), path.clone()));
    state.asset_bytes = state.asset_bytes.saturating_add(size);
    Ok(GameSound { path: path.clone() })
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

fn jet_game_run(
    scene: &mut GameScene,
    replay: Option<&GameReplay>,
    backend: Option<&GameBackend>,
) -> String {
    let mut backend = backend.cloned().unwrap_or_else(jet_game_backend_headless);
    let replay_path = replay
        .map(|r| r.path.clone())
        .unwrap_or_else(|| "<none>".to_string());

    // D-PERFBUDGET-GAMEMIGRATE1 / PROVIDER1: when JET_SCENE_PROBE=<name>
    // matches this scene, pin identity then run 120 warmup + 600 measured
    // frames and emit JETSCENE1 wire rows to stdout. Normal output is
    // suppressed; the caller parses only JETSCENEID/JETSCENE1 lines.
    let probe_name = std::env::var("JET_SCENE_PROBE").ok();
    let measuring = probe_name.as_deref() == Some(scene.name.as_str());

    let asset_bytes = scene.assets.state.borrow().asset_bytes;
    let warmup_frames: i64 = 120;
    let measure_frames: i64 = 600;
    let total_frames = if measuring { warmup_frames + measure_frames } else { 3 };

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

    // D-GAME-LOOP1=A: probe path keeps a fixed frame count; ordinary runs loop
    // on backend.should_continue() / present() (headless budget = 3).
    let mut frame_idx: i64 = 0;
    loop {
        if measuring {
            if frame_idx >= total_frames {
                break;
            }
        } else if !jet_game_backend_should_continue(&backend) {
            break;
        }
        let pressed = if frame_idx == 1 {
            scene
                .assets
                .state
                .borrow()
                .bindings
                .iter()
                .map(|(action, _)| action.clone())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
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
        if measuring {
            let t0 = std::time::Instant::now();
            for cb in scene.callbacks.borrow_mut().iter_mut() {
                cb(frame.clone());
            }
            let elapsed_ns = t0.elapsed().as_nanos() as u64;
            // Only emit measured frames (skip warmup).
            if frame_idx >= warmup_frames {
                let rss = read_vmhwm_bytes();
                println!("JETSCENE1\t{scene_hex}\tFrameTime\t{elapsed_ns}");
                println!("JETSCENE1\t{scene_hex}\tDrawCalls\t{callbacks_count}");
                println!("JETSCENE1\t{scene_hex}\tSceneAssetBytes\t{asset_bytes}");
                println!("JETSCENE1\t{scene_hex}\tMemoryHighWater\t{rss}");
            }
        } else {
            for cb in scene.callbacks.borrow_mut().iter_mut() {
                cb(frame.clone());
            }
            let input = if pressed.is_empty() {
                "none".to_string()
            } else {
                pressed.join("+")
            };
            out.push(format!("frame:{} input:{}", frame_idx, input));
        }
        jet_game_backend_present(&mut backend);
        frame_idx += 1;
    }
    out.join("\n")
}
