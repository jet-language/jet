use crate::AST::{Expr, Type};
use crate::Codegen::Cx;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::{TExpr, TExprKind};
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::TPanicLoc;
use crate::Codegen::TIR::TRequireKind;
use crate::Codegen::TIR::lower_expr;
use crate::Diagnostics::Span;

pub(crate) fn clone_env(env: &LowerEnv) -> LowerEnv {
    env.clone()
}

pub(crate) fn fork_panic(env: &LowerEnv) -> LowerEnv {
    clone_env(env)
}

/// Text, 1-based line and 1-based column of the line holding `offset`;
/// `lines` indexes `src`.
pub(crate) fn tir_src_line_at<'s>(
    src: &'s str,
    lines: &jet_foundation::Diagnostics::LineIndex,
    offset: usize,
) -> (&'s str, u32, u32) {
    let (line, col) = lines.line_col(src, offset);
    (lines.line_text(src, line), line as u32, col as u32)
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
    let (src_line, line, col) = tir_src_line_at(&cx.src, cx.src_lines(), span.start);
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
        Some(Box::new(lower_panic_message_expr(
            &call.args[1].expr,
            cx,
            env,
        )))
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
    let mut right = Box::new(lower_expr(&call.args[1].expr, cx, env));
    // Sema checked a bare `None` against the left side's option type; the
    // lowered `Absent` carries only the `Int` placeholder (as `==` retypes it).
    if matches!(right.kind, TExprKind::Absent) && matches!(left.ty, Type::Option(_)) {
        right.ty = left.ty.clone();
    }
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
