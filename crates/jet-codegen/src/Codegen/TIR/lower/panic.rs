use crate::AST::{Expr, Type};
use crate::Codegen::Cx;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::TPanicLoc;
use crate::Codegen::TIR::TRequireKind;
use crate::Diagnostics::Span;

pub(crate) const RESOURCE_CLEANUP_MARKER: &str = "__JET_RESOURCE_CLEANUP__";
/// A stream send that observes a closed consumer returns from the generator
/// after the active lexical cleanups have run. The emitter replaces this
/// marker with those cleanups before the return.
pub(crate) const STREAM_CANCEL_MARKER: &str = "__JET_STREAM_CANCEL_RETURN__";

pub(crate) fn expr_ast_jet_ty(e: &Expr, env: &LowerEnv) -> Option<Type> {
    match e {
        Expr::Ident(name, _) => env.ty_of(name),
        _ => None,
    }
}

pub(crate) fn clone_env(env: &LowerEnv) -> LowerEnv {
    LowerEnv {
        locals: env.locals.clone(),
        tracked_float_origins: env.tracked_float_origins.clone(),
        fn_name: env.fn_name.clone(),
        ret_ty: env.ret_ty.clone(),
        self_owner: env.self_owner.clone(),
        string_view_locals: env.string_view_locals.clone(),
        borrowed_locals: env.borrowed_locals.clone(),
        resource_locals: env.resource_locals.clone(),
        gc_locals: env.gc_locals.clone(),
        uninit_fixed_locals: env.uninit_fixed_locals.clone(),
        gc_return: env.gc_return,
        split_view_handles: env.split_view_handles.clone(),
        cloned_types: env.cloned_types.clone(),
    }
}

pub(crate) fn fork_panic(env: &LowerEnv) -> LowerEnv {
    clone_env(env)
}

pub(crate) fn tir_src_line_at(src: &str, offset: usize) -> (&str, u32, u32) {
    if src.is_empty() {
        return ("", 1, 1);
    }
    let offset = offset.min(src.len());
    let (line, col) = crate::Diagnostics::span_line_col(src, offset);
    let line_start = src[..offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let line_end = src[offset..]
        .find('\n')
        .map(|p| offset + p)
        .unwrap_or(src.len());
    (&src[line_start..line_end], line as u32, col as u32)
}

fn safe_locals_snapshot(env: &LowerEnv) -> Vec<(String, TLocal)> {
    let mut parts: Vec<(String, TLocal)> = env
        .locals
        .iter()
        .filter_map(|(name, (place, jet_ty))| {
            let safe = jet_ty
                .as_ref()
                .is_some_and(|t| matches!(t, Type::Int | Type::Float | Type::Bool));
            if !safe {
                return None;
            }
            Some((name.clone(), place.clone()))
        })
        .collect();
    parts.sort_by(|a, b| a.0.cmp(&b.0));
    parts
}

pub(crate) fn capture_panic_loc(span: &Span, cx: &Cx, env: &LowerEnv) -> TPanicLoc {
    let (src_line, line, col) = tir_src_line_at(&cx.src, span.start);
    TPanicLoc {
        file: cx.file.clone(),
        src_line: src_line.trim_end().to_string(),
        line,
        col,
        caret: (span.end - span.start) as u32,
        fn_name: env.fn_name.clone(),
        locals: safe_locals_snapshot(env),
    }
}

pub(crate) fn lower_panic_message_expr(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    lower_expr(e, cx, env)
}

pub(crate) fn lower_require_stop(
    call: &crate::AST::Call,
    cx: &Cx,
    env: &mut LowerEnv,
) -> (TRequireKind, TPanicLoc) {
    let loc = capture_panic_loc(&call.name_span, cx, env);
    let cond = Box::new(lower_expr(&call.args[0].expr, cx, env));
    let msg = if call.args.len() == 2 {
        Some(Box::new(lower_panic_message_expr(&call.args[1].expr, cx, env)))
    } else {
        None
    };
    (TRequireKind::Require { cond, msg }, loc)
}

pub(crate) fn lower_require_eq_stop(
    call: &crate::AST::Call,
    cx: &Cx,
    env: &mut LowerEnv,
) -> (TRequireKind, TPanicLoc) {
    let loc = capture_panic_loc(&call.name_span, cx, env);
    let left = Box::new(lower_expr(&call.args[0].expr, cx, env));
    let right = Box::new(lower_expr(&call.args[1].expr, cx, env));
    (TRequireKind::RequireEq { left, right }, loc)
}

pub(crate) fn lower_panic_stop(
    name_span: &Span,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    env: &mut LowerEnv,
) -> (TRequireKind, TPanicLoc) {
    let loc = capture_panic_loc(name_span, cx, env);
    let msg = Box::new(lower_panic_message_expr(&args[0].expr, cx, env));
    (TRequireKind::Panic { msg }, loc)
}
