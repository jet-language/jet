//! D-GAME*: resident-JIT host for `core.game` (mirrors prelude `Game.rs`).

use super::Concurrency;
use crate::runtime_host::{alloc_jit_result, JitRuntime};

#[derive(Default)]
pub(crate) struct GameSceneState {
    pub(crate) name: String,
    pub(crate) assets: Vec<(String, String)>,
    pub(crate) bindings: Vec<(String, String)>,
    pub(crate) components: Vec<String>,
    pub(crate) asset_bytes: u64,
    pub(crate) callbacks: Vec<GameFrameCb>,
}

#[derive(Clone, Copy)]
pub(crate) struct GameFrameCb {
    pub(crate) fn_ptr: u64,
    pub(crate) caps: [i64; 4],
    pub(crate) n_caps: u8,
}

#[derive(Clone)]
pub(crate) struct GameFrameState {
    pub(crate) index: i64,
    pub(crate) pressed: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct GameReplayState {
    pub(crate) path: String,
}

#[derive(Clone)]
pub(crate) struct GameBackendState {
    pub(crate) renderer: String,
    pub(crate) audio: String,
    pub(crate) editor: String,
}

fn with_rt<F, R>(f: F) -> R
where
    F: FnOnce(&mut JitRuntime) -> R,
    R: Default,
{
    Concurrency::with_runtime_mut(f)
}

extern "C" fn jet_jit_game_scene_new(name: i64) -> i64 {
    with_rt(|rt| {
        let name = rt.heap.clone_string(name).unwrap_or_default();
        rt.game_scenes.push(GameSceneState {
            name,
            ..GameSceneState::default()
        });
        (rt.game_scenes.len()) as i64 // 1-based
    })
}

extern "C" fn jet_jit_game_replay_record(path: i64) -> i64 {
    with_rt(|rt| {
        let path = rt.heap.clone_string(path).unwrap_or_default();
        rt.game_replays.push(GameReplayState { path });
        rt.game_replays.len() as i64
    })
}

extern "C" fn jet_jit_game_backend_headless() -> i64 {
    with_rt(|rt| {
        rt.game_backends.push(GameBackendState {
            renderer: "headless".into(),
            audio: "none".into(),
            editor: "none".into(),
        });
        rt.game_backends.len() as i64
    })
}

/// Register `on_frame` callback: `fn_ptr` with `n_caps` captures then frame handle.
extern "C" fn jet_jit_game_scene_on_frame(
    scene: i64,
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
) {
    with_rt(|rt| {
        let scene = rt
            .game_scenes
            .get_mut(scene.saturating_sub(1) as usize)
            .expect("jit game on_frame: bad scene");
        let n = n_caps.clamp(0, 4) as u8;
        scene.callbacks.push(GameFrameCb {
            fn_ptr: fn_ptr as u64,
            caps: [c0, c1, c2, c3],
            n_caps: n,
        });
    });
}

extern "C" fn jet_jit_game_scene_component(scene: i64, name: i64) {
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

extern "C" fn jet_jit_game_scene_query(scene: i64, names: i64) -> i64 {
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
            vec![rt.heap.alloc_string(wanted.join("+"))]
        } else {
            Vec::new()
        };
        rt.heap.alloc_int_list(out)
    })
}

extern "C" fn jet_jit_game_assets_image(scene: i64, path: i64) -> i64 {
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

extern "C" fn jet_jit_game_assets_sound(scene: i64, path: i64) -> i64 {
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

extern "C" fn jet_jit_game_input_bind(scene: i64, action: i64, key: i64) {
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

extern "C" fn jet_jit_game_input_pressed(frame: i64, action: i64) -> i8 {
    with_rt(|rt| {
        let action_s = rt.heap.clone_string(action).unwrap_or_default();
        let frame = rt
            .game_frames
            .get(frame.saturating_sub(1) as usize)
            .expect("jit game pressed: bad frame");
        i8::from(frame.pressed.iter().any(|a| a == &action_s))
    })
}

extern "C" fn jet_jit_game_frame_index(frame: i64) -> i64 {
    with_rt(|rt| {
        rt.game_frames
            .get(frame.saturating_sub(1) as usize)
            .expect("jit game frame index: bad frame")
            .index
    })
}

extern "C" fn jet_jit_game_asset_show(kind: i64, path: i64) -> i64 {
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

fn invoke_frame_cb(cb: GameFrameCb, frame: i64) {
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
}

/// `core.game.run(scene, replay?: …, backend?: …)` — headless 3-frame transcript.
extern "C" fn jet_jit_game_run(scene: i64, replay: i64, backend: i64) -> i64 {
    with_rt(|rt| {
        let scene_idx = scene.saturating_sub(1) as usize;
        let name = rt
            .game_scenes
            .get(scene_idx)
            .expect("jit game run: bad scene")
            .name
            .clone();
        let backend_s = if backend > 0 {
            rt.game_backends
                .get(backend.saturating_sub(1) as usize)
                .cloned()
                .unwrap_or(GameBackendState {
                    renderer: "headless".into(),
                    audio: "none".into(),
                    editor: "none".into(),
                })
        } else {
            GameBackendState {
                renderer: "headless".into(),
                audio: "none".into(),
                editor: "none".into(),
            }
        };
        let replay_path = if replay > 0 {
            rt.game_replays
                .get(replay.saturating_sub(1) as usize)
                .map(|r| r.path.clone())
                .unwrap_or_else(|| "<none>".into())
        } else {
            "<none>".into()
        };

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
        let callbacks = scene_ref.callbacks.clone();
        let binding_actions: Vec<String> = scene_ref
            .bindings
            .iter()
            .map(|(a, _)| a.clone())
            .collect();

        let mut out = Vec::new();
        out.push(format!("scene:{name}"));
        out.push(format!(
            "backend:{}/{}/{}",
            backend_s.renderer, backend_s.audio, backend_s.editor
        ));
        out.push(format!("replay:{replay_path}"));
        out.push(format!(
            "assets:{}",
            if assets.is_empty() {
                "none".into()
            } else {
                assets
            }
        ));
        out.push(format!(
            "input:{}",
            if bindings.is_empty() {
                "none".into()
            } else {
                bindings
            }
        ));
        out.push(format!(
            "components:{}",
            if components.is_empty() {
                "none".into()
            } else {
                components
            }
        ));

        for frame_idx in 0..3i64 {
            let pressed = if frame_idx == 1 {
                binding_actions.clone()
            } else {
                Vec::new()
            };
            rt.game_frames.push(GameFrameState {
                index: frame_idx,
                pressed: pressed.clone(),
            });
            let frame_h = rt.game_frames.len() as i64;
            for cb in &callbacks {
                invoke_frame_cb(*cb, frame_h);
            }
            let input = if pressed.is_empty() {
                "none".to_string()
            } else {
                pressed.join("+")
            };
            out.push(format!("frame:{frame_idx} input:{input}"));
        }
        rt.heap.alloc_string(out.join("\n"))
    })
}

pub(crate) struct GameHostFns {
    pub(crate) scene_new: cranelift_module::FuncId,
    pub(crate) replay_record: cranelift_module::FuncId,
    pub(crate) backend_headless: cranelift_module::FuncId,
    pub(crate) on_frame: cranelift_module::FuncId,
    pub(crate) component: cranelift_module::FuncId,
    pub(crate) query: cranelift_module::FuncId,
    pub(crate) assets_image: cranelift_module::FuncId,
    pub(crate) assets_sound: cranelift_module::FuncId,
    pub(crate) input_bind: cranelift_module::FuncId,
    pub(crate) input_pressed: cranelift_module::FuncId,
    pub(crate) frame_index: cranelift_module::FuncId,
    pub(crate) asset_show: cranelift_module::FuncId,
    pub(crate) run: cranelift_module::FuncId,
}

pub(crate) fn register_game_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_game_scene_new", jet_jit_game_scene_new as *const u8);
    builder.symbol(
        "jet_jit_game_replay_record",
        jet_jit_game_replay_record as *const u8,
    );
    builder.symbol(
        "jet_jit_game_backend_headless",
        jet_jit_game_backend_headless as *const u8,
    );
    builder.symbol(
        "jet_jit_game_scene_on_frame",
        jet_jit_game_scene_on_frame as *const u8,
    );
    builder.symbol(
        "jet_jit_game_scene_component",
        jet_jit_game_scene_component as *const u8,
    );
    builder.symbol(
        "jet_jit_game_scene_query",
        jet_jit_game_scene_query as *const u8,
    );
    builder.symbol(
        "jet_jit_game_assets_image",
        jet_jit_game_assets_image as *const u8,
    );
    builder.symbol(
        "jet_jit_game_assets_sound",
        jet_jit_game_assets_sound as *const u8,
    );
    builder.symbol(
        "jet_jit_game_input_bind",
        jet_jit_game_input_bind as *const u8,
    );
    builder.symbol(
        "jet_jit_game_input_pressed",
        jet_jit_game_input_pressed as *const u8,
    );
    builder.symbol(
        "jet_jit_game_frame_index",
        jet_jit_game_frame_index as *const u8,
    );
    builder.symbol("jet_jit_game_asset_show", jet_jit_game_asset_show as *const u8);
    builder.symbol("jet_jit_game_run", jet_jit_game_run as *const u8);
}

pub(crate) fn declare_game_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<GameHostFns, String> {
    use cranelift_codegen::ir::{types, AbiParam, Signature};
    use cranelift_module::{Linkage, Module};

    let cc = module.target_config().default_call_conv;
    let mut import = |name: &str, sig: &Signature| -> Result<cranelift_module::FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
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
    for _ in 0..7 {
        sig_on_frame.params.push(AbiParam::new(types::I64));
    }
    let mut sig_run = Signature::new(cc);
    for _ in 0..3 {
        sig_run.params.push(AbiParam::new(types::I64));
    }
    sig_run.returns.push(AbiParam::new(types::I64));
    let mut sig_new0 = Signature::new(cc);
    sig_new0.returns.push(AbiParam::new(types::I64));

    Ok(GameHostFns {
        scene_new: import("jet_jit_game_scene_new", &sig_i64)?,
        replay_record: import("jet_jit_game_replay_record", &sig_i64)?,
        backend_headless: import("jet_jit_game_backend_headless", &sig_new0)?,
        on_frame: import("jet_jit_game_scene_on_frame", &sig_on_frame)?,
        component: import("jet_jit_game_scene_component", &sig_ii)?,
        query: import("jet_jit_game_scene_query", &sig_ii_ret)?,
        assets_image: import("jet_jit_game_assets_image", &sig_ii_ret)?,
        assets_sound: import("jet_jit_game_assets_sound", &sig_ii_ret)?,
        input_bind: import("jet_jit_game_input_bind", &sig_iii)?,
        input_pressed: import("jet_jit_game_input_pressed", &sig_ii_i8)?,
        frame_index: import("jet_jit_game_frame_index", &sig_i64)?,
        asset_show: import("jet_jit_game_asset_show", &sig_ii_ret)?,
        run: import("jet_jit_game_run", &sig_run)?,
    })
}
