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
    jet_codegen::Comptime::apply_raylib_ambient_core_call(
        module,
        method,
        args,
        span,
        resolved_ret,
        sink,
    )
}

