use crate::AST::{Type};
use crate::Codegen::Cx;
use crate::Codegen::escape_rust_str;
use crate::Codegen::TIR::emit::emit_panic_locals;
use crate::Codegen::TIR::emit::emit_panic_message_expr;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::emit_tir_stmts;
use crate::Codegen::TIR::RESOURCE_CLEANUP_MARKER;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TOrFallback;
use crate::Codegen::TIR::TStmt;

/// c109 Phase 8/15: format a `??` fallback right-hand side, mirroring
/// `emit_or_fallback_rhs` (Statement.rs). Value and early-`return` (Phase 8); the
/// `panic(…)` form (Phase 15) carries structured panic facts from lowering.
pub(crate) fn emit_tir_orfallback_rhs(fallback: &TOrFallback, cx: &Cx) -> String {
    match fallback {
        TOrFallback::Value(e) => emit_tir_expr(e, cx),
        TOrFallback::Return(None) => "return".to_string(),
        TOrFallback::Return(Some(e)) => format!("return {}", emit_tir_expr(e, cx)),
        TOrFallback::Panic { msg, loc } => {
            if cx.test_mode {
                return format!(
                    "{{ return Err({}); }}",
                    emit_panic_message_expr(msg, cx)
                );
            }
            format!(
                "{{ {cleanup} jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{msg}, &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }}",
                cleanup = RESOURCE_CLEANUP_MARKER,
                file = escape_rust_str(&loc.file),
                line = loc.line,
                fn_name_esc = escape_rust_str(&loc.fn_name),
                src_line_esc = escape_rust_str(&loc.src_line),
                col = loc.col,
                caret = loc.caret,
                msg = emit_panic_message_expr(msg, cx),
                locals = emit_panic_locals(loc, cx),
            )
        }
        TOrFallback::Break => "break".to_string(),
        TOrFallback::Continue => "continue".to_string(),
        TOrFallback::BreakLabel(name) => format!("break 'jet_{name}"),
        TOrFallback::ContinueLabel(name) => format!("continue 'jet_{name}"),
    }
}

pub(crate) fn emit_tir_value_block(stmts: &[TStmt], value: &TExpr, cx: &Cx) -> String {
    let mut inner = String::new();
    emit_tir_stmts(stmts, cx, &mut inner, 1);
    format!("{{ {} {} }}", inner, emit_tir_expr(value, cx))
}

/// c109 Phase 10: emit a core/stdlib module call, reproducing `emit_core_call`
/// (Source/Codegen/Expression.rs) byte-for-byte. The `(module, method)` dispatch is
/// a pure syntactic match on the two resolved strings — no type inference (I3). Args
/// were lowered PLAINLY; the per-arm `&(…)`/`&mut (…)`/move wrappers are applied here
/// exactly as the AST path applies them around its `arg(i)` = raw `emit_expr`.
/// `cx.root_prefix`/`cx.ffi_crate` are program-level. The gate only ever admits a
/// `(module, method)` with a matching arm here, so the `/* unknown std call */`
/// fallthrough is unreachable for a covered call (kept for parity with the AST path).
// D-SERDE: encoding-verb routing helpers — read the lowered arg type / resolved
// return type to pick the dynamic vs typed helper. Total facts, never re-inferred (I3).
pub(crate) fn enc_is_json_name(n: &str) -> bool {
    crate::Syntax::is_data_type_name(n)
}
/// A bare `decode` whose result OK arm is the dynamic `Data` tree (lenient path).
pub(crate) fn enc_ok_is_json(ret_ty: &Type) -> bool {
    matches!(ret_ty, Type::Result { ok, .. } if matches!(&**ok, Type::Named(n) if enc_is_json_name(n)))
}
/// The Rust type a typed whole-value `decode<T>` constructs.
pub(crate) fn enc_target_rust(ret_ty: &Type, cx: &Cx) -> String {
    if let Type::Result { ok, .. } = ret_ty {
        cx.rust_type(ok)
    } else {
        cx.rust_type(ret_ty)
    }
}
pub(crate) fn enc_row_target_rust(ret_ty: &Type, cx: &Cx) -> String {
    if let Type::Result { ok, .. } = ret_ty {
        if let Type::List(elem) = &**ok {
            return cx.rust_type(elem);
        }
    }
    enc_target_rust(ret_ty, cx)
}
/// D-MIGRATE3=A: the Rust type a typed `decode_traced<T>` constructs — same
/// target as [`enc_target_rust`], one layer deeper through the resolved
/// `Result<DecodeResult<T | [T]>, [FieldError]>` return type.
pub(crate) fn enc_target_rust_traced(ret_ty: &Type, cx: &Cx) -> String {
    if let Type::Result { ok, .. } = ret_ty {
        if let Type::Apply { args, .. } = &**ok {
            if let Some(inner) = args.first() {
                return cx.rust_type(inner);
            }
        }
    }
    cx.rust_type(ret_ty)
}
pub(crate) fn enc_row_target_rust_traced(ret_ty: &Type, cx: &Cx) -> String {
    if let Type::Result { ok, .. } = ret_ty {
        if let Type::Apply { args, .. } = &**ok {
            if let Some(Type::List(elem)) = args.first() {
                return cx.rust_type(elem);
            }
        }
    }
    enc_target_rust_traced(ret_ty, cx)
}
pub(crate) fn enc_arg_is_json(args: &[TExpr]) -> bool {
    matches!(args.first().map(|a| &a.ty), Some(Type::Named(n)) if enc_is_json_name(n))
}
/// `[[String]]` — the dynamic CSV form fed to the row renderer.
pub(crate) fn enc_arg_is_string_rows(args: &[TExpr]) -> bool {
    matches!(
        args.first().map(|a| &a.ty),
        Some(Type::List(inner))
            if matches!(&**inner, Type::List(s) if matches!(&**s, Type::String))
    )
}
