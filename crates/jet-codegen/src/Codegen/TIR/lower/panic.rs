use crate::AST::{Expr, Type};
use crate::Codegen::Cx;
use crate::Codegen::escape_rust_str;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr;
use crate::Diagnostics::Span;

pub(crate) const RESOURCE_CLEANUP_MARKER: &str = "__JET_RESOURCE_CLEANUP__";

/// Resolve the subject's Jet type for binding payloads, mirroring `expr_jet_ty`'s
/// reach (only an Ident resolves via its slot). Enough for the covered subset (the
/// subject is an enum-typed local/param). Other forms resolve to `None` (the
/// payload types come from `cx.enum_variants` regardless).
pub(crate) fn expr_ast_jet_ty(e: &Expr, env: &LowerEnv) -> Option<Type> {
    match e {
        Expr::Ident(name, _) => env.ty_of(name),
        _ => None,
    }
}

/// Clone the env for a lexical child scope. Bindings added to the child remain visible
/// to panic context inside it, but cannot leak into a later panic in the parent.
pub(crate) fn clone_env(env: &LowerEnv) -> LowerEnv {
    LowerEnv {
        locals: env.locals.clone(),
        fn_name: env.fn_name.clone(),
        self_owner: env.self_owner.clone(),
        string_view_locals: env.string_view_locals.clone(),
        borrowed_locals: env.borrowed_locals.clone(),
        resource_locals: env.resource_locals.clone(),
        gc_locals: env.gc_locals.clone(),
        gc_return: env.gc_return,
        cloned_types: env.cloned_types.clone(),
    }
}

/// Clone the env for pattern arms and lambda bodies. Kept as a named boundary because
/// these scopes also have capture-specific lowering rules.
pub(crate) fn fork_panic(env: &LowerEnv) -> LowerEnv {
    LowerEnv {
        locals: env.locals.clone(),
        fn_name: env.fn_name.clone(),
        self_owner: env.self_owner.clone(),
        string_view_locals: env.string_view_locals.clone(),
        borrowed_locals: env.borrowed_locals.clone(),
        resource_locals: env.resource_locals.clone(),
        gc_locals: env.gc_locals.clone(),
        gc_return: env.gc_return,
        cloned_types: env.cloned_types.clone(),
    }
}

/// c109 Phase 15: render the `{ jet_panic_rich(…); }` statement string for a
/// `a ?? panic(msg)` fallback, byte-for-byte `emit_panic_stop`
/// (Source/Codegen/Statement.rs). Every input — the panic message (lowered from the
/// message expression), the source-line text / line / column / caret width (from
/// `cx.src` at the `panic` name span), the escaped file + enclosing function name, and
/// the sorted scalar-locals snapshot — is resolved here so emit reads nothing from
/// `cx.src`/`cx.current_fn` (I3).
pub(crate) fn render_panic_stop(
    name_span: &Span,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    env: &mut LowerEnv,
) -> String {
    let msg = render_panic_message(&args[0].expr, cx, env);
    let (src_line, line, col) = tir_src_line_at(&cx.src, name_span.start);
    let caret_len = (name_span.end - name_span.start) as u32;
    let fn_name = env.fn_name.clone();
    let locals_expr = render_safe_locals(env);
    format!(
        "{{ {cleanup} jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{msg}, &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }}",
        cleanup = RESOURCE_CLEANUP_MARKER,
        file = escape_rust_str(&cx.file),
        line = line,
        fn_name_esc = escape_rust_str(&fn_name),
        src_line_esc = escape_rust_str(src_line.trim_end()),
        col = col,
        caret = caret_len,
        msg = msg,
        locals = locals_expr,
    )
}

/// c109 Phase 26: render `require(cond[, msg])` (S36), byte-for-byte `emit_require`
/// (Source/Codegen/Statement.rs). The default build emits a guarded `jet_panic_rich`;
/// `cx.test_mode` emits a `return Err(<msg>)` form. The condition + message are lowered
/// via the TIR; every source-position/locals fact is resolved here (I3).
pub(crate) fn render_require(call: &crate::AST::Call, cx: &Cx, env: &mut LowerEnv) -> String {
    let cond = emit_tir_expr(&lower_expr(&call.args[0].expr, cx, env), cx);
    let msg = if call.args.len() == 2 {
        render_panic_message(&call.args[1].expr, cx, env)
    } else {
        "\"condition failed\"".to_string()
    };
    if cx.test_mode {
        let msg_expr = if call.args.len() == 2 {
            msg
        } else {
            "\"condition failed\".to_string()".to_string()
        };
        return format!("{{ if !({}) {{ return Err({}); }} }}", cond, msg_expr);
    }
    let (src_line, line, col) = tir_src_line_at(&cx.src, call.name_span.start);
    let caret_len = (call.name_span.end - call.name_span.start) as u32;
    let fn_name = env.fn_name.clone();
    let locals_expr = render_safe_locals(env);
    let msg_used = if call.args.len() == 2 {
        msg
    } else {
        "\"condition failed\".to_string()".to_string()
    };
    format!(
        "{{ if !({cond}) {{ {cleanup} jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{msg}, &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }} }}",
        cond = cond,
        cleanup = RESOURCE_CLEANUP_MARKER,
        file = escape_rust_str(&cx.file),
        line = line,
        fn_name_esc = escape_rust_str(&fn_name),
        src_line_esc = escape_rust_str(src_line.trim_end()),
        col = col,
        caret = caret_len,
        msg = msg_used,
        locals = locals_expr,
    )
}

/// c109 Phase 26: render `require_eq(left, right)` (S36), byte-for-byte
/// `emit_require_eq` (Source/Codegen/Statement.rs). Binds the two operands into temps,
/// then compares; on inequality emits the test-mode `return Err(…)` or the default
/// `jet_panic_rich` with a `left: {}, right: {}` message.
pub(crate) fn render_require_eq(call: &crate::AST::Call, cx: &Cx, env: &mut LowerEnv) -> String {
    let left = emit_tir_expr(&lower_expr(&call.args[0].expr, cx, env), cx);
    let right = emit_tir_expr(&lower_expr(&call.args[1].expr, cx, env), cx);
    if cx.test_mode {
        return format!(
            "{{ let _jet_left = ({}); let _jet_right = ({}); if !(_jet_left == _jet_right) {{ return Err(format!(\"left: {{}}, right: {{}}\", _jet_left.jet_show(), _jet_right.jet_show())); }} }}",
            left, right
        );
    }
    let (src_line, line, col) = tir_src_line_at(&cx.src, call.name_span.start);
    let caret_len = (call.name_span.end - call.name_span.start) as u32;
    let fn_name = env.fn_name.clone();
    let locals_expr = render_safe_locals(env);
    format!(
        "{{ let _jet_left = ({left}); let _jet_right = ({right}); if !(_jet_left == _jet_right) {{ {cleanup} jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &format!(\"left: {{}}, right: {{}}\", _jet_left.jet_show(), _jet_right.jet_show()), &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }} }}",
        left = left,
        right = right,
        cleanup = RESOURCE_CLEANUP_MARKER,
        file = escape_rust_str(&cx.file),
        line = line,
        fn_name_esc = escape_rust_str(&fn_name),
        src_line_esc = escape_rust_str(src_line.trim_end()),
        col = col,
        caret = caret_len,
        locals = locals_expr,
    )
}

/// c109 Phase 15: reproduce `emit_panic_message` (Statement.rs): a `Str` literal emits
/// its interpolated form directly; any other expression is `({…}).jet_show()`. The
/// message expression is lowered + emitted via the TIR (= `emit_expr`).
pub(crate) fn render_panic_message(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> String {
    match e {
        Expr::Str(_, _) => emit_tir_expr(&lower_expr(e, cx, env), cx),
        other => format!(
            "({}).jet_show()",
            emit_tir_expr(&lower_expr(other, cx, env), cx)
        ),
    }
}

/// c109 Phase 15: reproduce `src_line_at` (Statement.rs) — the (line text, 1-based line,
/// 1-based column) for a byte offset.
pub(crate) fn tir_src_line_at(src: &str, offset: usize) -> (&str, u32, u32) {
    let (line, col) = crate::Diagnostics::span_line_col(src, offset);
    let line_start = src[..offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let line_end = src[offset..]
        .find('\n')
        .map(|p| offset + p)
        .unwrap_or(src.len());
    (&src[line_start..line_end], line as u32, col as u32)
}

/// c109 Phase 15: render rich-panic locals from the lexical lowering environment at
/// the panic site. Dumps in-scope scalar Int/Float/Bool slots, sorted by name, as a
/// `format!("name = {}, …", (place).jet_show(), …)` expression. A deref'd slot uses
/// `(*name).jet_show()` (the place already carries the `(*…)` wrapper, which is the bare
/// `(*name)` form, NOT a double-paren). Empty → `String::new()`.
pub(crate) fn render_safe_locals(env: &LowerEnv) -> String {
    let mut parts: Vec<(String, String)> = env
        .locals
        .iter()
        .filter_map(|(name, (place, jet_ty))| {
            let safe = jet_ty
                .as_ref()
                .map_or(false, |t| matches!(t, Type::Int | Type::Float | Type::Bool));
            if !safe {
                return None;
            }
            // `safe_locals_expr` builds `(*rust_name).jet_show()` for a deref'd slot and
            // `(rust_name).jet_show()` otherwise. The replica's `place` is exactly
            // `(*rust_name)` (deref) or `rust_name` — decode it back so the rendered
            // string is byte-identical (NOT `((*rust_name)).jet_show()`).
            let value_expr = if place.starts_with("(*") && place.ends_with(')') {
                let rust_name = &place[2..place.len() - 1];
                format!("(*{}).jet_show()", rust_name)
            } else {
                format!("({}).jet_show()", place)
            };
            Some((name.clone(), value_expr))
        })
        .collect();
    parts.sort_by(|a, b| a.0.cmp(&b.0));
    if parts.is_empty() {
        return "String::new()".to_string();
    }
    let fmt_str = parts
        .iter()
        .map(|(n, _)| format!("{} = {{}}", n))
        .collect::<Vec<_>>()
        .join(", ");
    let args = parts
        .iter()
        .map(|(_, e)| e.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!("format!(\"{}\", {})", fmt_str, args)
}
