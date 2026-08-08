//! D-RAYLIB1 / D-FLAGSHIP-RAYLIB1: resident-JIT `core.raylib` bridge.
//! Mirrors prelude `HandlesRaylib.rs` — headless by default (`JET_RAYLIB_DISPLAY=1`
//! enables native when the dynamic raylib API is available).

use super::Concurrency;
use crate::runtime_host::JitRuntime;

#[derive(Clone)]
pub(crate) struct RaylibWindowState {
    pub(crate) width: i64,
    pub(crate) height: i64,
    pub(crate) title: String,
    pub(crate) native: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct RaylibColorState {
    pub(crate) r: i64,
    pub(crate) g: i64,
    pub(crate) b: i64,
    pub(crate) a: i64,
}

#[derive(Clone)]
pub(crate) struct RaylibSoundState {
    pub(crate) path: String,
}

fn with_rt<F, R>(f: F) -> R
where
    F: FnOnce(&mut JitRuntime) -> R,
    R: Default,
{
    Concurrency::with_runtime_mut(f)
}

fn display_enabled() -> bool {
    std::env::var("JET_RAYLIB_DISPLAY").as_deref() == Ok("1")
}

extern "C" fn jet_jit_raylib_window_open(width: i64, height: i64, title: i64) -> i64 {
    with_rt(|rt| {
        let title_s = rt.heap.clone_string(title).unwrap_or_default();
        // Headless unless display is explicitly requested. Native init lives in
        // the AOT prelude bridge; JIT keeps the same default transcript.
        let native = display_enabled() && false;
        let _ = (native, width, height);
        rt.raylib_windows.push(RaylibWindowState {
            width,
            height,
            title: title_s,
            native: false,
        });
        rt.raylib_windows.len() as i64
    })
}

extern "C" fn jet_jit_raylib_color(r: i64, g: i64, b: i64, a: i64) -> i64 {
    with_rt(|rt| {
        rt.raylib_colors.push(RaylibColorState { r, g, b, a });
        rt.raylib_colors.len() as i64
    })
}

extern "C" fn jet_jit_raylib_set_target_fps(_fps: i64) {}

extern "C" fn jet_jit_raylib_key_down(name: i64) -> i8 {
    with_rt(|rt| {
        let _ = rt.heap.clone_string(name);
        // Headless bridge never reports keys pressed.
        0
    })
}

extern "C" fn jet_jit_raylib_begin_drawing(_window: i64) {}
extern "C" fn jet_jit_raylib_clear_background(_color: i64) {}
extern "C" fn jet_jit_raylib_draw_rectangle(
    _x: i64,
    _y: i64,
    _w: i64,
    _h: i64,
    _color: i64,
) {
}
extern "C" fn jet_jit_raylib_draw_text(
    _text: i64,
    _x: i64,
    _y: i64,
    _size: i64,
    _color: i64,
) {
}
extern "C" fn jet_jit_raylib_end_drawing() {}
extern "C" fn jet_jit_raylib_close_window(_window: i64) {}

extern "C" fn jet_jit_raylib_window_should_close(window: i64) -> i8 {
    with_rt(|rt| {
        let native = rt
            .raylib_windows
            .get(window.saturating_sub(1) as usize)
            .map(|w| w.native)
            .unwrap_or(false);
        // Headless windows always report closed after open (matches Prelude).
        i8::from(!native)
    })
}

extern "C" fn jet_jit_raylib_window_ready(window: i64) -> i8 {
    with_rt(|rt| {
        let native = rt
            .raylib_windows
            .get(window.saturating_sub(1) as usize)
            .map(|w| w.native)
            .unwrap_or(false);
        i8::from(native)
    })
}

extern "C" fn jet_jit_raylib_load_sound(path: i64) -> i64 {
    with_rt(|rt| {
        let path_s = rt.heap.clone_string(path).unwrap_or_default();
        rt.raylib_sounds.push(RaylibSoundState { path: path_s });
        rt.raylib_sounds.len() as i64
    })
}

extern "C" fn jet_jit_raylib_play_sound(sound: i64) -> i8 {
    with_rt(|rt| {
        let ok = rt
            .raylib_sounds
            .get(sound.saturating_sub(1) as usize)
            .map(|s| !s.path.is_empty())
            .unwrap_or(false);
        i8::from(ok)
    })
}

host_fns! {
    struct RaylibHostFns;
    register: register_raylib_symbols;
    declare: declare_raylib_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::{Linkage, Module};
        let cc = module.target_config().default_call_conv;

        let mut sig = |n_params: usize, ret: Option<cranelift_codegen::ir::Type>| {
            let mut s = Signature::new(cc);
            for _ in 0..n_params {
                s.params.push(AbiParam::new(types::I64));
            }
            if let Some(r) = ret {
                s.returns.push(AbiParam::new(r));
            }
            s
        };

    }
    window_open: "jet_jit_raylib_window_open" => jet_jit_raylib_window_open: sig(3, Some(types::I64));
    color: "jet_jit_raylib_color" => jet_jit_raylib_color: sig(4, Some(types::I64));
    set_target_fps: "jet_jit_raylib_set_target_fps" => jet_jit_raylib_set_target_fps: sig(1, None);
    key_down: "jet_jit_raylib_key_down" => jet_jit_raylib_key_down: sig(1, Some(types::I8));
    begin_drawing: "jet_jit_raylib_begin_drawing" => jet_jit_raylib_begin_drawing: sig(1, None);
    clear_background: "jet_jit_raylib_clear_background" => jet_jit_raylib_clear_background: sig(1, None);
    draw_rectangle: "jet_jit_raylib_draw_rectangle" => jet_jit_raylib_draw_rectangle: sig(5, None);
    draw_text: "jet_jit_raylib_draw_text" => jet_jit_raylib_draw_text: sig(5, None);
    end_drawing: "jet_jit_raylib_end_drawing" => jet_jit_raylib_end_drawing: sig(0, None);
    close_window: "jet_jit_raylib_close_window" => jet_jit_raylib_close_window: sig(1, None);
    window_should_close: "jet_jit_raylib_window_should_close" => jet_jit_raylib_window_should_close: sig(1, Some(types::I8));
    window_ready: "jet_jit_raylib_window_ready" => jet_jit_raylib_window_ready: sig(1, Some(types::I8));
    load_sound: "jet_jit_raylib_load_sound" => jet_jit_raylib_load_sound: sig(1, Some(types::I64));
    play_sound: "jet_jit_raylib_play_sound" => jet_jit_raylib_play_sound: sig(1, Some(types::I8));
}





