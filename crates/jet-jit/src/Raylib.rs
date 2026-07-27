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

pub(crate) struct RaylibHostFns {
    pub(crate) window_open: cranelift_module::FuncId,
    pub(crate) color: cranelift_module::FuncId,
    pub(crate) set_target_fps: cranelift_module::FuncId,
    pub(crate) key_down: cranelift_module::FuncId,
    pub(crate) begin_drawing: cranelift_module::FuncId,
    pub(crate) clear_background: cranelift_module::FuncId,
    pub(crate) draw_rectangle: cranelift_module::FuncId,
    pub(crate) draw_text: cranelift_module::FuncId,
    pub(crate) end_drawing: cranelift_module::FuncId,
    pub(crate) close_window: cranelift_module::FuncId,
}

pub(crate) fn register_raylib_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol(
        "jet_jit_raylib_window_open",
        jet_jit_raylib_window_open as *const u8,
    );
    builder.symbol("jet_jit_raylib_color", jet_jit_raylib_color as *const u8);
    builder.symbol(
        "jet_jit_raylib_set_target_fps",
        jet_jit_raylib_set_target_fps as *const u8,
    );
    builder.symbol(
        "jet_jit_raylib_key_down",
        jet_jit_raylib_key_down as *const u8,
    );
    builder.symbol(
        "jet_jit_raylib_begin_drawing",
        jet_jit_raylib_begin_drawing as *const u8,
    );
    builder.symbol(
        "jet_jit_raylib_clear_background",
        jet_jit_raylib_clear_background as *const u8,
    );
    builder.symbol(
        "jet_jit_raylib_draw_rectangle",
        jet_jit_raylib_draw_rectangle as *const u8,
    );
    builder.symbol(
        "jet_jit_raylib_draw_text",
        jet_jit_raylib_draw_text as *const u8,
    );
    builder.symbol(
        "jet_jit_raylib_end_drawing",
        jet_jit_raylib_end_drawing as *const u8,
    );
    builder.symbol(
        "jet_jit_raylib_close_window",
        jet_jit_raylib_close_window as *const u8,
    );
}

pub(crate) fn declare_raylib_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<RaylibHostFns, String> {
    use cranelift_codegen::ir::{types, AbiParam, Signature};
    use cranelift_module::{Linkage, Module};

    let cc = module.target_config().default_call_conv;
    let mut import = |name: &str, sig: &Signature| -> Result<cranelift_module::FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
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
    Ok(RaylibHostFns {
        window_open: import("jet_jit_raylib_window_open", &sig(3, Some(types::I64)))?,
        color: import("jet_jit_raylib_color", &sig(4, Some(types::I64)))?,
        set_target_fps: import("jet_jit_raylib_set_target_fps", &sig(1, None))?,
        key_down: import("jet_jit_raylib_key_down", &sig(1, Some(types::I8)))?,
        begin_drawing: import("jet_jit_raylib_begin_drawing", &sig(1, None))?,
        clear_background: import("jet_jit_raylib_clear_background", &sig(1, None))?,
        draw_rectangle: import("jet_jit_raylib_draw_rectangle", &sig(5, None))?,
        draw_text: import("jet_jit_raylib_draw_text", &sig(5, None))?,
        end_drawing: import("jet_jit_raylib_end_drawing", &sig(0, None))?,
        close_window: import("jet_jit_raylib_close_window", &sig(1, None))?,
    })
}
