//! D-GAME*: resident-JIT host for `core.game` (mirrors prelude `Game.rs`).

use super::Concurrency;
use crate::runtime_host::{alloc_jit_result, JitRuntime};
use jet_codegen::Codegen::MIREval::game_dev_protocol;
use jet_foundation::Game::{
    JetGameFrameBudget, JetGameReplay, JetGameTranscriptHasher,
};

#[derive(Default)]
pub(crate) struct GameSceneState {
    pub(crate) name: String,
    pub(crate) assets: Vec<(String, String)>,
    pub(crate) bindings: Vec<(String, String)>,
    pub(crate) components: Vec<String>,
    pub(crate) asset_bytes: u64,
    pub(crate) callbacks: Vec<GameFrameCb>,
    pub(crate) dev_session: Option<game_dev_protocol::GameDevSession>,
    pub(crate) policy: game_dev_protocol::GameDevPolicy,
}
const MAX_GAME_SCENE_CALLBACKS: usize = 256;

pub(crate) struct GameFrameCb {
    pub(crate) fn_ptr: u64,
    pub(crate) caps: [i64; 4],
    pub(crate) n_caps: u8,
    pub(crate) schedule: Option<jet_foundation::ResourceSchedule::JetFrameSchedule>,
    pub(crate) derivation: Option<String>,
    pub(crate) completion: Option<
        std::sync::Arc<
            std::sync::Mutex<jet_foundation::ResourceSchedule::JetFrameCompletionState>,
        >,
    >,
}

impl Clone for GameFrameCb {
    fn clone(&self) -> Self {
        Self {
            fn_ptr: self.fn_ptr,
            caps: self.caps,
            n_caps: self.n_caps,
            schedule: self.schedule.clone(),
            derivation: self.derivation.clone(),
            completion: self.completion.as_ref().map(|state| {
                std::sync::Arc::new(std::sync::Mutex::new(
                    state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone(),
                ))
            }),
        }
    }
}

#[derive(Clone)]
pub(crate) struct GameFrameState {
    pub(crate) index: i64,
    pub(crate) pressed: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct GameReplayState {
    pub(crate) path: String,
    pub(crate) tape: JetGameReplay,
}

/// One `core.game` backend handle. The frame budget is the shared Foundation
/// plan (`JetGameFrameBudget`), so headless/requested-frame iteration is the
/// same sequence AOT `Prelude/Game.rs` and the MIR interpreter run.
#[derive(Clone)]
pub(crate) struct GameBackendState {
    pub(crate) renderer: String,
    pub(crate) audio: String,
    pub(crate) editor: String,
    pub(crate) frame_budget: JetGameFrameBudget,
}

fn with_rt<F, R>(f: F) -> R
where
    F: FnOnce(&mut JitRuntime) -> R,
    R: Default,
{
    Concurrency::with_runtime_mut(f)
}

fn jet_game_scene_new(name: i64) -> i64 {
    with_rt(|rt| {
        let name = rt.heap.clone_string(name).unwrap_or_default();
        let policy = game_dev_protocol::GameDevPolicy::new(rt.game_debug_data_enabled());
        let dev_session = if policy.game_debug_data_enabled() {
            match game_dev_protocol::new_session_with_policy(
                &name,
                "jit",
                policy.game_debug_data_enabled(),
            ) {
                Ok(session) => Some(session),
                Err(error) => {
                    rt.set_trap(&error);
                    return 0;
                }
            }
        } else {
            None
        };
        rt.game_scenes.push(GameSceneState {
            name,
            dev_session,
            policy,
            ..GameSceneState::default()
        });
        (rt.game_scenes.len()) as i64 // 1-based
    })
}

fn jet_game_replay_record(path: i64) -> i64 {
    with_rt(|rt| {
        let path = rt.heap.clone_string(path).unwrap_or_default();
        let tape = match JetGameReplay::from_path(&path) {
            Ok(tape) => tape,
            Err(error) => {
                rt.set_trap(&error);
                return 0;
            }
        };
        rt.game_replays.push(GameReplayState { path, tape });
        rt.game_replays.len() as i64
    })
}

fn jet_game_backend_headless() -> i64 {
    with_rt(|rt| {
        rt.game_backends.push(GameBackendState {
            renderer: "headless".into(),
            audio: "none".into(),
            editor: "none".into(),
            frame_budget: JetGameFrameBudget::default(),
        });
        rt.game_backends.len() as i64
    })
}

fn jet_game_backend_should_continue(backend: i64) -> i64 {
    with_rt(|rt| {
        let backend = rt
            .game_backends
            .get(backend.saturating_sub(1) as usize)
            .expect("jit game should_continue: bad backend");
        i64::from(backend.frame_budget.should_continue())
    })
}

fn jet_game_backend_present(backend: i64) {
    with_rt(|rt| {
        let Some(backend) = rt
            .game_backends
            .get_mut(backend.saturating_sub(1) as usize)
        else {
            rt.set_trap("jit game present: bad backend");
            return;
        };
        backend.frame_budget.present();
    });
}
/// Register `on_frame` callback: `fn_ptr` with `n_caps` captures then frame
/// handle. Capture and callback tables are bounded; malformed ABI counts trap
/// instead of being silently truncated. The final two pointers carry the
/// checked schedule and canonical derivation id; neither is re-derived here.
fn jet_game_scene_on_frame(
    scene: i64,
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
    schedule_json: i64,
    derivation_id: i64,
) {
    with_rt(|rt| {
        let schedule = if schedule_json == 0 {
            None
        } else {
            let Some(payload) = rt.heap.clone_string(schedule_json) else {
                rt.set_trap("jit game frame schedule payload is not a string");
                return;
            };
            match jet_foundation::ResourceSchedule::JetFrameSchedule::from_canonical_json(&payload) {
                Ok(schedule) => Some(schedule),
                Err(error) => {
                    rt.set_trap(&format!("invalid checked game frame schedule: {error}"));
                    return;
                }
            }
        };
        let derivation = if derivation_id == 0 {
            None
        } else {
            let Some(id) = rt.heap.clone_string(derivation_id) else {
                rt.set_trap("jit game frame derivation id is not a string");
                return;
            };
            (!id.is_empty()).then_some(id)
        };
        if schedule.is_some() && derivation.is_none() {
            rt.set_trap("checked game frame schedule has no canonical derivation");
            return;
        }
        let scene = rt
            .game_scenes
            .get_mut(scene.saturating_sub(1) as usize)
            .expect("jit game on_frame: bad scene");
        if scene.callbacks.len() >= MAX_GAME_SCENE_CALLBACKS {
            rt.set_trap("jit game scene callback limit exceeded");
            return;
        }
        let Ok(n) = u8::try_from(n_caps) else {
            rt.set_trap("jit game on_frame capture count is outside the ABI");
            return;
        };
        if n > 4 {
            rt.set_trap("jit game on_frame supports at most four captures");
            return;
        }
        scene.callbacks.push(GameFrameCb {
            fn_ptr: fn_ptr as u64,
            caps: [c0, c1, c2, c3],
            n_caps: n,
            completion: schedule.as_ref().map(|schedule| {
                std::sync::Arc::new(std::sync::Mutex::new(
                    jet_foundation::ResourceSchedule::JetFrameCompletionState::new(schedule),
                ))
            }),
            schedule,
            derivation,
        });
    });
}

fn jet_game_scene_component(scene: i64, name: i64) {
    with_rt(|rt| {
        let name = rt.heap.clone_string(name).unwrap_or_default();
        let scene = rt
            .game_scenes
            .get_mut(scene.saturating_sub(1) as usize)
            .expect("jit game component: bad scene");
        if !scene.components.iter().any(|c| c == &name) {
            scene.components.push(name);
        }
    });
}

fn jet_game_scene_query(scene: i64, names: i64) -> i64 {
    with_rt(|rt| {
        let names_s = rt.heap.clone_string(names).unwrap_or_default();
        let scene = rt
            .game_scenes
            .get(scene.saturating_sub(1) as usize)
            .expect("jit game query: bad scene");
        let wanted: Vec<&str> = names_s.split(',').filter(|s| !s.is_empty()).collect();
        let ok = wanted
            .iter()
            .all(|name| scene.components.iter().any(|c| c == name));
        let out = if ok {
            let row = wanted
                .iter()
                .map(|name| match *name {
                    "Position" => "Position{x:0}".to_string(),
                    "Velocity" => "Velocity{dx:0}".to_string(),
                    other => format!("{other}{{}}"),
                })
                .collect::<Vec<_>>()
                .join(",");
            vec![rt.heap.alloc_string(row)]
        } else {
            Vec::new()
        };
        rt.heap.alloc_int_list(out)
    })
}

fn jet_game_assets_image(scene: i64, path: i64) -> i64 {
    with_rt(|rt| {
        let path_s = rt.heap.clone_string(path).unwrap_or_default();
        if path_s.contains("missing") {
            let err = rt.heap.alloc_string(format!("asset not found: {path_s}"));
            return alloc_jit_result(rt, false, err as u64);
        }
        let size = std::fs::metadata(&path_s).map(|m| m.len()).unwrap_or(0);
        let scene = rt
            .game_scenes
            .get_mut(scene.saturating_sub(1) as usize)
            .expect("jit game image: bad scene");
        scene.assets.push(("image".into(), path_s.clone()));
        scene.asset_bytes = scene.asset_bytes.saturating_add(size);
        let img = rt.heap.alloc_string(path_s);
        alloc_jit_result(rt, true, img as u64)
    })
}

fn jet_game_assets_sound(scene: i64, path: i64) -> i64 {
    with_rt(|rt| {
        let path_s = rt.heap.clone_string(path).unwrap_or_default();
        if path_s.contains("missing") {
            let err = rt.heap.alloc_string(format!("asset not found: {path_s}"));
            return alloc_jit_result(rt, false, err as u64);
        }
        let size = std::fs::metadata(&path_s).map(|m| m.len()).unwrap_or(0);
        let scene = rt
            .game_scenes
            .get_mut(scene.saturating_sub(1) as usize)
            .expect("jit game sound: bad scene");
        scene.assets.push(("sound".into(), path_s.clone()));
        scene.asset_bytes = scene.asset_bytes.saturating_add(size);
        let snd = rt.heap.alloc_string(path_s);
        alloc_jit_result(rt, true, snd as u64)
    })
}

fn jet_game_input_bind(scene: i64, action: i64, key: i64) {
    with_rt(|rt| {
        let action_s = rt.heap.clone_string(action).unwrap_or_default();
        let key_s = rt.heap.clone_string(key).unwrap_or_default();
        let scene = rt
            .game_scenes
            .get_mut(scene.saturating_sub(1) as usize)
            .expect("jit game bind: bad scene");
        if !scene
            .bindings
            .iter()
            .any(|(a, k)| a == &action_s && k == &key_s)
        {
            scene.bindings.push((action_s, key_s));
        }
    });
}

fn jet_game_asset_show(kind: i64, path: i64) -> i64 {
    with_rt(|rt| {
        let path_s = rt.heap.clone_string(path).unwrap_or_default();
        let text = if kind == 0 {
            format!("GameImage({path_s})")
        } else {
            format!("GameSound({path_s})")
        };
        rt.heap.alloc_string(text)
    })
}

fn invoke_frame_cb(cb: &GameFrameCb, frame: i64) -> Result<(), &'static str> {
    if let Some(schedule) = &cb.schedule {
        if schedule
            .operations
            .windows(2)
            .any(|window| window[0].source_index > window[1].source_index)
        {
            return Err("checked game frame schedule is not in source order");
        }
        if cb.derivation.is_none() {
            return Err("checked game frame schedule has no canonical derivation");
        }
    }
    // SAFETY: fn_ptr is a JIT-compiled spawn body with matching capture arity.
    unsafe {
        match cb.n_caps {
            0 => {
                let f: unsafe extern "C" fn(i64) = std::mem::transmute(cb.fn_ptr);
                f(frame);
            }
            1 => {
                let f: unsafe extern "C" fn(i64, i64) = std::mem::transmute(cb.fn_ptr);
                f(cb.caps[0], frame);
            }
            2 => {
                let f: unsafe extern "C" fn(i64, i64, i64) = std::mem::transmute(cb.fn_ptr);
                f(cb.caps[0], cb.caps[1], frame);
            }
            3 => {
                let f: unsafe extern "C" fn(i64, i64, i64, i64) = std::mem::transmute(cb.fn_ptr);
                f(cb.caps[0], cb.caps[1], cb.caps[2], frame);
            }
            _ => {
                let f: unsafe extern "C" fn(i64, i64, i64, i64, i64) =
                    std::mem::transmute(cb.fn_ptr);
                f(cb.caps[0], cb.caps[1], cb.caps[2], cb.caps[3], frame);
            }
        }
    }
    Ok(())
}

fn game_option_payload(
    rt: &JitRuntime,
    raw: i64,
) -> Result<Option<i64>, &'static str> {
    if raw == 0 {
        return Ok(None);
    }
    let Some((present, bits)) = crate::runtime_host::jit_result_parts(rt, raw) else {
        return Err("jit game optional argument is not a result carrier");
    };
    Ok(present.then_some(bits as i64))
}
fn jit_game_session_mut(
    rt: &mut JitRuntime,
    scene_idx: usize,
) -> Result<&mut game_dev_protocol::GameDevSession, String> {
    rt.game_scenes
        .get_mut(scene_idx)
        .ok_or_else(|| "jit game run: bad scene".to_string())?
        .dev_session
        .as_mut()
        .ok_or_else(|| "jit game scene has no development session".to_string())
}
fn jit_game_debug_data_enabled(
    rt: &JitRuntime,
    scene_idx: usize,
) -> Result<bool, String> {
    rt.game_scenes
        .get(scene_idx)
        .map(|scene| scene.policy.game_debug_data_enabled())
        .ok_or_else(|| "jit game run: bad scene".to_string())
}

fn jit_game_start(rt: &mut JitRuntime, scene_idx: usize, backend: &str) -> Result<(), String> {
    if !jit_game_debug_data_enabled(rt, scene_idx)? {
        return Ok(());
    }
    let scene_name = rt
        .game_scenes
        .get(scene_idx)
        .ok_or_else(|| "jit game run: bad scene".to_string())?
        .name
        .clone();
    let session = jit_game_session_mut(rt, scene_idx)?;
    let identity = session.profiler.identity().clone();
    session.set_trace_context(game_dev_protocol::JetGameTraceContext::new(
        identity.session,
        scene_name,
        "jit",
        backend,
        1,
        1,
        "default",
    ))?;
    session.dispatch_pending_devtools_controls(0)?;
    if session.phase == game_dev_protocol::GameDevPhase::Editing {
        session.dispatch_control(0, game_dev_protocol::GameDevControl::Play)?;
    }
    Ok(())
}

fn jit_game_admit_frame(
    rt: &mut JitRuntime,
    scene_idx: usize,
    frame_idx: i64,
) -> Result<bool, String> {
    if !jit_game_debug_data_enabled(rt, scene_idx)? {
        return Ok(true);
    }
    let session = jit_game_session_mut(rt, scene_idx)?;
    session.dispatch_pending_devtools_controls(frame_idx as u64)?;
    Ok(session.begin_simulation_frame())
}

fn jit_game_finish_frame(
    rt: &mut JitRuntime,
    scene_idx: usize,
    scene_name: &str,
    frame_idx: i64,
    elapsed_ns: u64,
    callbacks_count: u64,
) -> Result<(), String> {
    if !jit_game_debug_data_enabled(rt, scene_idx)? {
        return Ok(());
    }
    let session = jit_game_session_mut(rt, scene_idx)?;
    let identity = session.profiler.identity().clone();
    if session.profiler.profiler().state()
        == game_dev_protocol::JetGameFrameProfilerState::Running
    {
        let frame_identity = game_dev_protocol::JetGameFrameIdentity::new(
            scene_name,
            frame_idx as u64,
            identity.build,
            identity.revision,
            identity.source,
        );
        session.record_and_publish(
            game_dev_protocol::JetGameFrameSample::new(
                frame_identity,
                frame_idx as u64,
                elapsed_ns,
                None,
            )
            .into(),
        )?;
        session.publish_overlay_metric(
            frame_idx as u64,
            game_dev_protocol::JetGameOverlayMetricValue::new(
                game_dev_protocol::JetGameOverlayMetric::FrameTime,
                elapsed_ns.min(i64::MAX as u64) as i64,
                frame_idx as u64,
            ),
        )?;
        session.publish_overlay_metric(
            frame_idx as u64,
            game_dev_protocol::JetGameOverlayMetricValue::new(
                game_dev_protocol::JetGameOverlayMetric::DrawCalls,
                callbacks_count.min(i64::MAX as u64) as i64,
                frame_idx as u64,
            ),
        )?;
    }
    // The published frame ids are the session's own bookkeeping; like the MIR
    // interpreter (`eval_game_run`), the host only propagates the failure.
    session
        .finish_simulation_frame(frame_idx as u64, frame_idx as u64)
        .map(|_| ())
}

fn jit_game_stop(rt: &mut JitRuntime, scene_idx: usize, frame_idx: i64) -> Result<(), String> {
    if !jit_game_debug_data_enabled(rt, scene_idx)? {
        return Ok(());
    }
    jit_game_session_mut(rt, scene_idx)?.stop(frame_idx as u64, "game run completed")
}


/// `core.game.run(scene, replay?: …, backend?: …, frames?: Int)` — every
/// optional slot is a canonical result carrier and every iteration comes from
/// the Foundation frame plan.
fn jet_game_run(scene: i64, replay: i64, backend: i64, frames: i64) -> i64 {
    with_rt(|rt| {
        let scene_idx = scene.saturating_sub(1) as usize;
        let name = rt
            .game_scenes
            .get(scene_idx)
            .expect("jit game run: bad scene")
            .name
            .clone();
        let replay = match game_option_payload(rt, replay) {
            Ok(value) => value,
            Err(error) => {
                rt.set_trap(error);
                return 0;
            }
        };
        let backend = match game_option_payload(rt, backend) {
            Ok(value) => value,
            Err(error) => {
                rt.set_trap(error);
                return 0;
            }
        };
        let requested_frames = match game_option_payload(rt, frames) {
            Ok(value) => value,
            Err(error) => {
                rt.set_trap(error);
                return 0;
            }
        };
        let backend_s = backend
            .and_then(|raw| {
                rt.game_backends
                    .get(raw.saturating_sub(1) as usize)
                    .cloned()
            })
            .unwrap_or_else(|| GameBackendState {
                renderer: "headless".into(),
                audio: "none".into(),
                editor: "none".into(),
                frame_budget: JetGameFrameBudget::default(),
            });
        let (replay_path, replay_tape) = replay
            .and_then(|raw| {
                rt.game_replays
                    .get(raw.saturating_sub(1) as usize)
                    .map(|r| (r.path.clone(), Some(r.tape.clone())))
            })
            .unwrap_or_else(|| ("<none>".into(), None::<JetGameReplay>));

        let scene_ref = rt.game_scenes.get(scene_idx).expect("scene");
        let assets = scene_ref
            .assets
            .iter()
            .map(|(k, p)| format!("{k}:{p}"))
            .collect::<Vec<_>>()
            .join(",");
        let bindings = scene_ref
            .bindings
            .iter()
            .map(|(a, k)| format!("{a}={k}"))
            .collect::<Vec<_>>()
            .join(",");
        let components = scene_ref.components.join(",");
        let mut callbacks = scene_ref.callbacks.clone();

        let mut backend_s = backend_s;
        if let Err(error) = backend_s.frame_budget.set_requested(requested_frames) {
            rt.set_trap(error);
            return 0;
        }
        let mut transcript = JetGameTranscriptHasher::new();
        let mut out = Vec::new();
        out.push(format!("scene:{name}"));
        out.push(format!(
            "backend:{}/{}/{}",
            backend_s.renderer, backend_s.audio, backend_s.editor
        ));
        out.push(format!("replay:{replay_path}"));
        out.push(format!(
            "assets:{}",
            if assets.is_empty() { "none" } else { assets.as_str() }
        ));
        out.push(format!(
            "input:{}",
            if bindings.is_empty() { "none" } else { bindings.as_str() }
        ));
        out.push(format!(
            "components:{}",
            if components.is_empty() { "none" } else { components.as_str() }
        ));

        let backend_label = format!(
            "{}/{}/{}",
            backend_s.renderer, backend_s.audio, backend_s.editor
        );
        if let Err(error) = jit_game_start(rt, scene_idx, &backend_label) {
            rt.set_trap(&error);
            return 0;
        }
        let mut last_frame = 0i64;
        while let Some(frame_idx) = backend_s.frame_budget.next_frame() {
            loop {
                match jit_game_admit_frame(rt, scene_idx, frame_idx) {
                    Ok(true) => break,
                    Ok(false) => {
                        // Keep this issued frame in flight while a paused
                        // resident session polls for its next control.
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    Err(error) => {
                        rt.set_trap(&error);
                        return 0;
                    }
                }
            }
            last_frame = frame_idx;
            let pressed = replay_tape
                .as_ref()
                .and_then(|tape| tape.actions_at(frame_idx))
                .map(|actions| actions.to_vec())
                .unwrap_or_default();
            transcript.push_frame(frame_idx, &pressed);
            rt.game_frames.push(GameFrameState {
                index: frame_idx,
                pressed: pressed.clone(),
            });
            let frame_h = rt.game_frames.len() as i64;
            let started = std::time::Instant::now();
            for cb in &mut callbacks {
                let completion = cb.completion.clone();
                let result = if let Some(completion) = completion {
                    jet_foundation::ResourceSchedule::with_frame_completion_scope(
                        completion,
                        || invoke_frame_cb(cb, frame_h),
                    )
                } else {
                    invoke_frame_cb(cb, frame_h)
                };
                if let Err(error) = result {
                    rt.set_trap(error);
                    return 0;
                }
                if let Some(schedule) = &cb.schedule {
                    let Some(completion) = cb.completion.as_ref() else {
                        rt.set_trap("jit game frame schedule has no completion state");
                        return 0;
                    };
                    let completion = completion.lock().unwrap_or_else(
                        std::sync::PoisonError::into_inner,
                    );
                    if let Err(error) = completion.assert_reuse_ready(schedule) {
                        rt.set_trap(&error);
                        return 0;
                    }
                }
            }
            if rt.trap_pending() {
                return 0;
            }
            let elapsed_ns = started.elapsed().as_nanos() as u64;
            let input = if pressed.is_empty() {
                "none".to_string()
            } else {
                pressed.join("+")
            };
            out.push(format!("frame:{frame_idx} input:{input}"));
            if let Err(error) = jit_game_finish_frame(
                rt,
                scene_idx,
                &name,
                frame_idx,
                elapsed_ns,
                callbacks.len() as u64,
            ) {
                rt.set_trap(&error);
                return 0;
            }
            backend_s.frame_budget.present();
        }
        if let Err(error) = jit_game_stop(rt, scene_idx, last_frame) {
            rt.set_trap(&error);
            return 0;
        }
        if requested_frames.is_some() {
            out.push(format!("transcript_hash:{}", transcript.finish()));
        }
        rt.heap.alloc_string(out.join("\n"))
    })
}

fn jet_game_input_pressed(scene: i64, action: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let action = rt.heap.clone_string(action).unwrap_or_default();
        let Some(frame) = rt.game_frames.last() else {
            return 0;
        };
        let bound = rt
            .game_scenes
            .get(scene.saturating_sub(1) as usize)
            .and_then(|scene| {
                scene
                    .bindings
                    .iter()
                    .find(|(bound_action, _)| bound_action == &action)
                    .map(|(_, key)| key.clone())
            });
        let needle = bound.unwrap_or(action);
        i8::from(frame.pressed.iter().any(|pressed| pressed == &needle))
    })
}

fn jet_game_frame_index(frame: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.game_frames
            .get(frame.saturating_sub(1) as usize)
            .map(|frame| frame.index)
            .unwrap_or(0)
    })
}

host_fns! {
    struct GameHostFns;
    register: register_game_symbols;
    declare: declare_game_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::Module;
        let cc = module.target_config().default_call_conv;

        let mut sig_i64 = Signature::new(cc);
        sig_i64.params.push(AbiParam::new(types::I64));
        sig_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_void = Signature::new(cc);
        sig_void.params.push(AbiParam::new(types::I64));
        let mut sig_ii = Signature::new(cc);
        sig_ii.params.push(AbiParam::new(types::I64));
        sig_ii.params.push(AbiParam::new(types::I64));
        let mut sig_iii = Signature::new(cc);
        sig_iii.params.push(AbiParam::new(types::I64));
        sig_iii.params.push(AbiParam::new(types::I64));
        sig_iii.params.push(AbiParam::new(types::I64));
        let mut sig_ii_ret = sig_ii.clone();
        sig_ii_ret.returns.push(AbiParam::new(types::I64));
        let mut sig_ii_i8 = sig_ii.clone();
        sig_ii_i8.returns.push(AbiParam::new(types::I8));
        let mut sig_on_frame = Signature::new(cc);
        for _ in 0..9 {
            sig_on_frame.params.push(AbiParam::new(types::I64));
        }
        let mut sig_run = Signature::new(cc);
        for _ in 0..4 {
            sig_run.params.push(AbiParam::new(types::I64));
        }
        sig_run.returns.push(AbiParam::new(types::I64));
        let mut sig_new0 = Signature::new(cc);
        sig_new0.returns.push(AbiParam::new(types::I64));

    }
    scene_new: "jet_game_scene_new" => jet_game_scene_new: sig_i64;
    replay_record: "jet_game_replay_record" => jet_game_replay_record: sig_i64;
    backend_headless: "jet_game_backend_headless" => jet_game_backend_headless: sig_new0;
    backend_should_continue: "jet_game_backend_should_continue" => jet_game_backend_should_continue: sig_i64;
    backend_present: "jet_game_backend_present" => jet_game_backend_present: sig_void;
    on_frame: "jet_game_scene_on_frame" => jet_game_scene_on_frame: sig_on_frame;
    component: "jet_game_scene_component" => jet_game_scene_component: sig_ii;
    query: "jet_game_scene_query" => jet_game_scene_query: sig_ii_ret;
    assets_image: "jet_game_assets_image" => jet_game_assets_image: sig_ii_ret;
    assets_sound: "jet_game_assets_sound" => jet_game_assets_sound: sig_ii_ret;
    input_bind: "jet_game_input_bind" => jet_game_input_bind: sig_iii;
    input_pressed: "jet_game_input_pressed" => jet_game_input_pressed: sig_ii_i8;
    frame_index: "jet_game_frame_index" => jet_game_frame_index: sig_i64;
    asset_show: "jet_game_asset_show" => jet_game_asset_show: sig_ii_ret;
    run: "jet_game_run" => jet_game_run: sig_run;
}

