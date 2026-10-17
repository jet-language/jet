//! D-RAYLIB1 / D-FLAGSHIP-RAYLIB1: resident-JIT `core.game.raylib` bridge.
//! Mirrors prelude `HandlesRaylib.rs` — headless by default (`JET_RAYLIB_DISPLAY=1`
//! enables native when the dynamic raylib API is available).

mod raylib_kernel {
    include!("../../jet-codegen/src/Prelude/Core/Raylib.rs");
}

/// Register the Raylib interpreter adapter in the shared ambient context.
///
/// The callback delegates directly to the Comptime Raylib adapter rather than
/// re-entering the public ambient dispatcher.
pub(crate) fn register_interpreter_ambient(context: &mut crate::InterpreterAmbientContext) {
    context.register_core_call(jet_raylib_ambient_core_call);
}

fn jet_raylib_ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<jet_foundation::AST::CtValue>,
    span: jet_foundation::Diagnostics::Span,
    resolved_ret: Option<jet_foundation::AST::Type>,
    sink: Option<&mut jet_codegen::Comptime::DevSink>,
) -> Option<
    Result<
        jet_foundation::AST::CtValue,
        jet_foundation::Diagnostics::Diagnostic,
    >,
> {
    let method = match (module, method) {
        ("core.game.raylib", "__window_open") => "window_open",
        ("core.game.raylib", "__window_should_close") => "window_should_close",
        ("core.game.raylib", "__window_ready") => "window_ready",
        ("core.game.raylib", "__begin_drawing") => "begin_drawing",
        ("core.game.raylib", "__clear_background") => "clear_background",
        ("core.game.raylib", "__draw_text") => "draw_text",
        ("core.game.raylib", "__draw_rectangle") => "draw_rectangle",
        ("core.game.raylib", "__end_drawing") => "end_drawing",
        ("core.game.raylib", "__close_window") => "close_window",
        ("core.game.raylib", "__key_down") => "key_down",
        ("core.game.raylib", "__set_target_fps") => "set_target_fps",
        ("core.game.raylib", "__gamepad_down") => "gamepad_down",
        ("core.game.raylib", "__gamepad_axis") => "gamepad_axis",
        ("core.game.raylib", "__load_texture_atlas") => "load_texture_atlas",
        ("core.game.raylib", "__draw_sprite") => "draw_sprite",
        _ => method,
    };
    jet_codegen::Comptime::apply_raylib_ambient_core_call(
        module,
        method,
        args,
        span,
        resolved_ret,
        sink,
    )
}

