use crate::jet_generated_format as jet_format;
use crate::AST::Type;
use crate::Codegen::Cx;
use crate::Codegen::escape_rust_str;
use crate::Codegen::mangle;
use crate::Codegen::mangle_path;
use crate::Codegen::TIR::core_struct_field_rust_name;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::RESOURCE_CLEANUP_MARKER;
use crate::Codegen::TIR::TCallArg;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TLetTy;
use crate::Codegen::TIR::TLetWrapper;
use crate::Codegen::TIR::TPanicLoc;
use crate::Codegen::TIR::TPattern;
use crate::Codegen::TIR::TPatternPosition;
use crate::Codegen::TIR::TPlace;
use crate::Codegen::TIR::TPreludeArg;
use crate::Codegen::TIR::TRequireKind;
use crate::Codegen::TIR::TStaticOwner;
use crate::Codegen::TIR::TStrPart;

/// The Rust pattern a `TPattern` spells. The position decides which of codegen's
/// three pattern shapes applies; the pattern and its owning enum are the only
/// other facts, both already resolved at lowering.
pub(crate) fn emit_tir_pattern(pattern: &TPattern, cx: &Cx) -> String {
    match &pattern.position {
        TPatternPosition::Binding => {
            crate::Codegen::emit_if_let_pattern(cx, &pattern.pattern)
        }
        TPatternPosition::OptionBinding => match &pattern.pattern {
            crate::AST::Pattern::Present { binding, .. } => {
                format!("Some({})", mangle(binding))
            }
            crate::AST::Pattern::Absent(_) => "None".to_string(),
            _ => crate::Codegen::emit_if_let_pattern(cx, &pattern.pattern),
        },
        TPatternPosition::Arm => crate::Codegen::emit_match_pattern(
            cx,
            &pattern.pattern,
            pattern.enum_type.as_deref(),
        ),
        // A variant of a resolved enum layout compares against the bare variant
        // path (no payload slots) — `tir_enum_lit_prefix` owns that spelling.
        TPatternPosition::VariantPath => {
            let owner = pattern.enum_type.as_deref().unwrap_or_default();
            let variant = pattern.variant().unwrap_or_default();
            crate::Codegen::TIR::tir_enum_lit_prefix(cx, owner, variant)
        }
        // D-ENC-DYN1: `DataTree::Object` binds its ordered entry vector to a temp;
        // the body's prefix `let` collects it into the user-visible map.
        TPatternPosition::DataEntries { temp } => {
            format!("{}jet_std::DataTree::Object({})", cx.root_prefix, temp)
        }
    }
}

/// The Rust place an assignment/increment writes. A local slot spells itself; a
/// structured place expression emits like any other node.
pub(crate) fn emit_tir_place(place: &TPlace, cx: &Cx) -> String {
    match place {
        TPlace::Local(slot) => slot.rust_place(),
        TPlace::Expr(expr) => emit_tir_expr(expr, cx),
    }
}

/// The Rust type head a static call qualifies with. A user type resolves through
/// `cx.type_prefix`; a prelude/host owner spells its resolved symbol path plus
/// any generic arguments.
pub(crate) fn emit_static_owner(owner: &TStaticOwner, cx: &Cx) -> String {
    match owner {
        TStaticOwner::User(type_name) => cx.type_prefix(type_name),
        TStaticOwner::Prelude {
            rooted,
            path,
            generics,
        } => {
            let root = if *rooted { cx.root_prefix.as_str() } else { "" };
            if generics.is_empty() {
                return format!("{root}{path}");
            }
            let args = generics
                .iter()
                .map(|arg| match arg {
                    TPreludeArg::Jet(ty) => cx.rust_type(ty),
                    TPreludeArg::HostUsize => "usize".to_string(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{root}{path}::<{args}>")
        }
    }
}

/// c109 Phase 6: format call/method arguments from total TIR flags. The clone wrapper
/// (`.clone()` or
/// `Arc::clone(&…)`) is applied to the raw value first, then the borrow wrapper
/// (`&(…)` for a `Read` non-scalar, `&mut (…)` for a `Mutate`). All four decisions
/// are total TIR flags — emit makes no convention decision.
pub(crate) fn emit_tir_call_args(args: &[TCallArg], cx: &Cx) -> String {
    args.iter()
        .map(|a| {
            let uninit_borrow = match &a.value.kind {
                crate::Codegen::TIR::TExprKind::Local(local)
                    if local.uninit_fixed && a.mut_borrow =>
                {
                    Some(format!("({}).as_array_mut()", local.rust_place()))
                }
                crate::Codegen::TIR::TExprKind::Local(local)
                    if local.uninit_fixed && a.borrow =>
                {
                    Some(format!("({}).as_array()", local.rust_place()))
                }
                _ => None,
            };
            let mut s = uninit_borrow
                .clone()
                .unwrap_or_else(|| emit_tir_expr(&a.value, cx));
            // emit_call_args applies implicit_clone XOR shared_auto_clone (the AST
            // path uses `if … else if …`); the gate/lowering never set both.
            if a.clone {
                s = format!("({}).clone()", s);
            } else if a.arc_clone {
                // D-MEM1 S6: `Type::Shared` lowers to `JetShared<T>` (a newtype
                // wrapping `Arc<RwLock<T>>`, not a bare `Arc<T>`) since this
                // stage — `.clone()` (its own `impl Clone`, a cheap handle
                // clone) is the correct call now, not `Arc::clone(&…)`.
                s = format!("({}).clone()", s);
            }
            // D-FIXARR1: widen a [T#N] (Rust [T; N]) to [T] (Vec<T>) before passing.
            // Applied AFTER clone, BEFORE fn-coerce and borrow wrappers.
            if a.widen_to_vec {
                s = format!("({}).to_vec()", s);
            }
            // D-UNIONTYPE1=A: wrap a member value into the compiler-generated enum.
            if let Some(Type::Union(members)) = &a.widen_to_union {
                let enum_name = crate::AST::union_enum_name(members);
                let tag = crate::AST::union_member_tag(&a.value.ty);
                // Bare member-type tags — matches `emit_anonymous_unions` / match arms.
                s = format!("{}::{tag}({s})", mangle_path(&enum_name));
            }
            // Fn-typed coercion: wrap to match `cx.rust_type` (Rc / Arc / Box for FnMut).
            // Skip wrap when the value already emits Rc/Arc/Box::new (named fn / lambda).
            if let Some(fc) = &a.fn_coerce {
                if !fc.already_boxed {
                    let rust_ty = cx.rust_type(&fc.ty);
                    let wrap = if rust_ty.starts_with("std::sync::Arc<") {
                        "std::sync::Arc::new"
                    } else if rust_ty.starts_with("std::rc::Rc<") {
                        "std::rc::Rc::new"
                    } else {
                        "Box::new"
                    };
                    s = format!("{wrap}({s})");
                }
                s = format!("{} as {}", s, cx.rust_type(&fc.ty));
            }
            if a.borrow && uninit_borrow.is_none() {
                s = format!("&({})", s);
            } else if a.mut_borrow && uninit_borrow.is_none() {
                s = format!("&mut ({})", s);
            }
            s
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn emit_tir_str(parts: &[TStrPart], cx: &Cx) -> String {
    if parts.len() == 1 {
        if let TStrPart::Lit(s) = &parts[0] {
            return format!("{:?}.to_string()", s);
        }
    }
    // Do not bind a fixed Rust local here. User names are canonically mangled
    // under `__jet`, so a parameter such as `s` would otherwise collide with a
    // generated `__jet_s` accumulator. `format!` keeps the interpolation
    // expression entirely hygienic while preserving the same display/debug
    // conversion facts decided by sema and lowering.
    let mut fmt = String::from("format!(\"");
    let mut args = Vec::new();
    for part in parts {
        match part {
            TStrPart::Lit(s) => {
                let escaped = format!("{:?}", s.replace('{', "{{").replace('}', "}}"));
                fmt.push_str(&escaped[1..escaped.len() - 1]);
            }
            TStrPart::Interp(e, format) => {
                let method = match format {
                    crate::AST::StrFormat::Display => "jet_display",
                    crate::AST::StrFormat::Debug => "jet_debug",
                    crate::AST::StrFormat::Fixed(_) => {
                        unreachable!("Fixed interpolation lowers to core.fmt.decimal")
                    }
                    crate::AST::StrFormat::Unit(_) => {
                        unreachable!("Unit interpolation lowers to a String")
                    }
                };
                fmt.push_str("{}");
                args.push(format!("({}).{method}()", emit_tir_expr(e, cx)));
            }
        }
    }
    fmt.push_str("\"");
    if args.is_empty() {
        format!("{fmt}).to_string()")
    } else {
        format!("{fmt}, {})", args.join(", "))
    }
}

/// Walk a lowered select-builder chain and collect channel/timer arm expressions.
pub(super) fn collect_select_arms(builder: &TExpr, cx: &Cx) -> (Vec<String>, Vec<String>) {
    let mut recvs = Vec::new();
    let mut afters = Vec::new();
    let mut cur = builder;
    loop {
        match &cur.kind {
            TExprKind::SelectStart => break,
            TExprKind::SelectRecv {
                builder: inner,
                channel,
            } => {
                recvs.push(emit_tir_expr(channel, cx));
                cur = inner;
            }
            TExprKind::SelectAfter {
                builder: inner,
                millis,
                value,
            } => {
                let ms = emit_tir_expr(millis, cx);
                let value = value
                    .as_ref()
                    .map(|v| emit_tir_expr(v, cx))
                    .unwrap_or_else(|| "()".to_string());
                afters.push(format!("({ms}, {value})"));
                cur = inner;
            }
            TExprKind::SelectRead { builder: inner, .. } => {
                cur = inner;
            }
            _ => break,
        }
    }
    (recvs, afters)
}

/// D-SWIZZLE1: render a read swizzle as lane extract(s) and optional `VecN` ctor.
pub(super) fn emit_math_swizzle_read(cx: &Cx, type_name: &str, recv: &TExpr, lanes: &[u8]) -> String {
    let r = emit_tir_expr(recv, cx);
    if lanes.len() == 1 {
        return format!("({r}).0[{}]", lanes[0] as usize);
    }
    if (type_name == "F32x4" || type_name == "F64x2")
        && lanes.len()
            == match type_name {
                "F32x4" => 4,
                "F64x2" => 2,
                _ => 0,
            }
    {
        let comps: Vec<String> = lanes
            .iter()
            .map(|&l| format!("({r}).0[{}]", l as usize))
            .collect();
        return format!(
            "{}jet_math_{}_new({})",
            cx.root_prefix,
            type_name,
            comps.join(", ")
        );
    }
    let comps: Vec<String> = lanes
        .iter()
        .map(|&l| {
            let idx = l as usize;
            if type_name == "F32x4" {
                format!("({r}).0[{idx}] as f64")
            } else {
                format!("({r}).0[{idx}]")
            }
        })
        .collect();
    let result_ty = match lanes.len() {
        2 => "Vec2",
        3 => "Vec3",
        4 => "Vec4",
        _ => unreachable!("swizzle lane count 2..=4"),
    };
    format!(
        "{}jet_math_{}_new({})",
        cx.root_prefix,
        result_ty,
        comps.join(", ")
    )
}

/// D-SWIZZLE1: render a write swizzle as ordered lane stores.
pub(super) fn emit_math_swizzle_assign_stmt(base: &str, type_name: &str, lanes: &[u8], value: &str) -> String {
    if lanes.len() == 1 {
        let lane = lanes[0] as usize;
        let val = if type_name == "F32x4" {
            format!("({value}) as f32")
        } else {
            value.to_string()
        };
        return jet_format!("{{ let {jet_prefix}v = {val}; ({base}).0[{lane}] = {jet_prefix}v; }}");
    }
    let writes: Vec<String> = lanes
        .iter()
        .enumerate()
        .map(|(i, &l)| {
            let lane = l as usize;
            let comp = if type_name == "F32x4" {
                jet_format!("({jet_prefix}v).0[{i}] as f32")
            } else if type_name == "F64x2" && lanes.len() == 2 {
                jet_format!("({jet_prefix}v).0[{i}]")
            } else {
                jet_format!("({jet_prefix}v).0[{i}]")
            };
            format!("({base}).0[{lane}] = {comp}")
        })
        .collect();
    jet_format!("{{ let {jet_prefix}v = {value}; {}; }}", writes.join("; "))
}

pub(crate) fn emit_let_ty_clause(let_ty: &TLetTy, cx: &Cx) -> String {
    match let_ty {
        TLetTy::Inferred => String::new(),
        TLetTy::StrView => ": &str".to_string(),
        TLetTy::Tuple(types) => {
            let inner = types
                .iter()
                .map(|t| cx.rust_type(t))
                .collect::<Vec<_>>()
                .join(", ");
            // Rust `(T)` is grouping, not a 1-tuple — need the trailing comma.
            if types.len() == 1 {
                format!(": ({inner},)")
            } else {
                format!(": ({inner})")
            }
        }
        TLetTy::SendFn(ty) => {
            let Type::Fn {
                params,
                ret,
                return_view_provenance,
                ..
            } = ty
            else {
                return format!(": {}", cx.rust_type(ty));
            };
            let ordinary = cx.rust_fn_trait(
                params,
                ret.as_deref(),
                return_view_provenance.as_ref(),
                false,
            );
            let send = ordinary
                .strip_prefix("std::rc::Rc<")
                .and_then(|inner| inner.strip_suffix('>'))
                .map(|inner| format!("std::sync::Arc<{inner} + Send + Sync + 'static>"))
                .unwrap_or(ordinary);
            format!(": {send}")
        }
        TLetTy::Annotated { ty, mut_fn, wrapper } => {
            let base = if let Type::Fn {
                params,
                ret,
                return_view_provenance,
                ..
            } = ty {
                cx.rust_fn_trait(
                    params,
                    ret.as_deref(),
                    return_view_provenance.as_ref(),
                    *mut_fn,
                )
            } else {
                cx.rust_type(ty)
            };
            let annotated = match wrapper {
                TLetWrapper::None => base,
                TLetWrapper::Resource => format!("JetResource<{base}>"),
                TLetWrapper::AutomaticRoot => format!("jet_gc::AutomaticRoot<{base}>"),
            };
            format!(": {annotated}")
        }
    }
}

pub(crate) fn emit_field_rust(cx: &Cx, recv_ty: &Type, field: &str) -> String {
    core_struct_field_rust_name(cx, recv_ty, field).unwrap_or_else(|| mangle(field))
}

pub(crate) fn emit_panic_message_expr(msg: &TExpr, cx: &Cx) -> String {
    match &msg.kind {
        TExprKind::StrLit(_) => emit_tir_expr(msg, cx),
        _ => format!("({}).jet_show()", emit_tir_expr(msg, cx)),
    }
}

pub(crate) fn emit_panic_locals(loc: &TPanicLoc, _cx: &Cx) -> String {
    if loc.locals.is_empty() {
        return "String::new()".to_string();
    }
    let fmt_str = loc
        .locals
        .iter()
        .map(|(n, _)| format!("{n} = {{}}"))
        .collect::<Vec<_>>()
        .join(", ");
    let args = loc
        .locals
        .iter()
        .map(|(_, place)| {
            let rust_name = place.rust_name();
            if place.deref {
                format!("(*{rust_name}).jet_show()")
            } else {
                format!("({rust_name}).jet_show()")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("format!(\"{fmt_str}\", {args})")
}

fn emit_panic_rich_stmt(cond: &str, msg: &str, loc: &TPanicLoc, cx: &Cx) -> String {
    format!(
        "{{ if !({cond}) {{ {cleanup} jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{msg}, &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }} }}",
        cleanup = RESOURCE_CLEANUP_MARKER,
        file = escape_rust_str(&loc.file),
        line = loc.line,
        fn_name_esc = escape_rust_str(&loc.fn_name),
        src_line_esc = escape_rust_str(&loc.src_line),
        col = loc.col,
        caret = loc.caret,
        msg = msg,
        locals = emit_panic_locals(loc, cx),
    )
}

pub(crate) fn emit_require_stop(
    kind: &TRequireKind,
    loc: &TPanicLoc,
    cx: &Cx,
) -> String {
    match kind {
        TRequireKind::Require { cond, msg } => {
            let cond_s = emit_tir_expr(cond, cx);
            if cx.test_mode {
                let msg_s = match msg {
                    Some(m) => emit_panic_message_expr(m, cx),
                    None => "\"condition failed\".to_string()".to_string(),
                };
                return jet_format!(
                    "{{ if !({cond_s}) {{ let {jet_prefix}msg = {msg_s}; return Err(jet_test_failure({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{jet_prefix}msg)); }} }}",
                    file = escape_rust_str(&loc.file),
                    line = loc.line,
                    fn_name_esc = escape_rust_str(&loc.fn_name),
                    src_line_esc = escape_rust_str(&loc.src_line),
                    col = loc.col,
                    caret = loc.caret,
                );
            }
            let msg_s = match msg {
                Some(m) => emit_panic_message_expr(m, cx),
                None => "\"condition failed\".to_string()".to_string(),
            };
            emit_panic_rich_stmt(&cond_s, &msg_s, loc, cx)
        }
        TRequireKind::RequireEq { left, right } => {
            let left_s = emit_tir_expr(left, cx);
            let right_s = emit_tir_expr(right, cx);
            if cx.test_mode {
                return jet_format!(
                    "{{ let {jet_prefix}left = ({left_s}); let {jet_prefix}right = ({right_s}); if !({jet_prefix}left == {jet_prefix}right) {{ let {jet_prefix}msg = format!(\"expected {{}}, got {{}}\", {jet_prefix}right.jet_show(), {jet_prefix}left.jet_show()); return Err(jet_test_failure({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{jet_prefix}msg)); }} }}",
                    file = escape_rust_str(&loc.file),
                    line = loc.line,
                    fn_name_esc = escape_rust_str(&loc.fn_name),
                    src_line_esc = escape_rust_str(&loc.src_line),
                    col = loc.col,
                    caret = loc.caret,
                );
            }
            jet_format!(
                "{{ let {jet_prefix}left = ({left_s}); let {jet_prefix}right = ({right_s}); if !({jet_prefix}left == {jet_prefix}right) {{ {cleanup} jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &format!(\"left: {{}}, right: {{}}\", {jet_prefix}left.jet_show(), {jet_prefix}right.jet_show()), &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }} }}",
                cleanup = RESOURCE_CLEANUP_MARKER,
                file = escape_rust_str(&loc.file),
                line = loc.line,
                fn_name_esc = escape_rust_str(&loc.fn_name),
                src_line_esc = escape_rust_str(&loc.src_line),
                col = loc.col,
                caret = loc.caret,
                locals = emit_panic_locals(loc, cx),
            )
        }
        TRequireKind::Panic { msg } => {
            let msg_s = emit_panic_message_expr(msg, cx);
            // D-PROOF / proof-replay-decisions: `panic(...)` is an uncaught
            // runtime stop (E3001 / exit 70) even inside `#Test`. Only
            // `require` / `require_eq` are caught harness assertions.
            format!(
                "{{ {cleanup} jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{msg_s}, &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }}",
                cleanup = RESOURCE_CLEANUP_MARKER,
                file = escape_rust_str(&loc.file),
                line = loc.line,
                fn_name_esc = escape_rust_str(&loc.fn_name),
                src_line_esc = escape_rust_str(&loc.src_line),
                col = loc.col,
                caret = loc.caret,
                locals = emit_panic_locals(loc, cx),
            )
        }
    }
}
