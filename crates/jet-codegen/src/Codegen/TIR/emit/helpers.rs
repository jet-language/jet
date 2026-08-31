use crate::jet_generated_format as jet_format;
use crate::Codegen::escape_rust_str;
use crate::Codegen::mangle;
use crate::Codegen::mangle_generated;
use crate::Codegen::mangle_path;
use crate::Codegen::Cx;
use crate::Codegen::TIR::core_struct_field_rust_name;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::TCallArg;
use crate::Codegen::TIR::TExclusivity;
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
use crate::Codegen::TIR::TTryConvert;
use crate::Codegen::TIR::RESOURCE_CLEANUP_MARKER;
use crate::AST::Type;
/// Qualify a generated root helper while preserving paths already rooted by a
/// lowering seam. Host-call lowering can run before the final module prefix is
/// known, so this keeps both root-level and imported-module output valid.
pub(crate) fn root_path(cx: &Cx, path: &str) -> String {
    let root = cx.root_prefix.as_str();
    if root.is_empty()
        || path.starts_with("crate::")
        || path.starts_with("self::")
        || path.starts_with("super::")
        || path.starts_with("::")
        || path.starts_with("std::")
        || path.starts_with("core::")
        || path.starts_with("alloc::")
        || path.starts_with(root)
    {
        path.to_string()
    } else {
        format!("{root}{path}")
    }
}

/// Test failures use the generated `String` ABI only in the harness entry
/// points. `Cx::test_mode` is a build-wide flag, so imported helper bodies must
/// not use it to select their error representation.
pub(crate) fn is_test_harness_fn(cx: &Cx) -> bool {
    let current_fn = cx.current_fn.borrow();
    current_fn.starts_with("jet_test_") || current_fn.starts_with("jet_prop_")
}

/// The error family a `?` propagates once its total conversion has run. Sema
/// resolved that conversion at lowering, so this reads the recorded fact and
/// never re-infers a type (I3).
pub(crate) fn try_carrier_error_type(inner: &TExpr, convert: &TTryConvert) -> Option<Type> {
    match convert {
        TTryConvert::DefaultErr => Some(Type::Named(crate::Syntax::TYPE_ERR.to_string())),
        TTryConvert::Typed { target, .. } => Some(target.clone()),
        TTryConvert::WidenUnion { enum_name, .. } => Some(Type::Named(enum_name.clone())),
        TTryConvert::None | TTryConvert::Never => match &inner.ty {
            Type::Result { err, .. } => Some((**err).clone()),
            _ => None,
        },
    }
}

/// The Prelude renderer that turns one error family into report text. This is
/// `entry_error`'s classification (Items.rs) - the shipped answer for an error
/// crossing a reporting boundary, reused verbatim so both boundaries render one
/// family the same way: the ambient `Err` family owns a `Display` report, a
/// declared `#Display` family renders through `JetDisplay`, and a codegen
/// printable family (`auto_printable`, the same fact that emits its `JetShow`)
/// through `JetShow`.
fn harness_error_renderer(cx: &Cx, err_ty: Option<&Type>) -> &'static str {
    // An implicitly fallible callee (`panic`/`assert` inside an otherwise plain
    // signature) never spells an error family, so lowering carries no `Result`
    // type for it. That is exactly the ambient `Err` family - `JetErr` - which
    // owns a `Display` report.
    let Some(err_ty) = err_ty else {
        return "jet_entry_error_text";
    };
    if matches!(err_ty, Type::Named(name) if name == crate::Syntax::TYPE_ERR) {
        return "jet_entry_error_text";
    }
    if matches!(
        err_ty,
        Type::List(inner)
            if matches!(inner.as_ref(), Type::Named(name) if name == "FieldError")
    ) {
        return "jet_entry_error_text_show";
    }
    let uses_jet_display = match err_ty {
        Type::Named(name) | Type::Apply { name, .. } => cx.has_display_type(name),
        _ => false,
    };
    if uses_jet_display {
        "jet_entry_error_text_jet"
    } else if crate::Codegen::jet_showable_type(cx, err_ty) {
        "jet_entry_error_text_show"
    } else {
        "jet_entry_error_text"
    }
}

/// #2350: a harness entry (`jet_test_N`/`jet_prop_N`) returns the generated
/// `String` failure ABI, so a carrier crossing that boundary becomes that report
/// before `?` propagates it. Ordinary functions - including every imported
/// helper - keep their own declared family, so this adapter is emitted only
/// inside a harness entry. An already-`String` family needs no adapter.
pub(crate) fn emit_harness_carrier_report(cx: &Cx, err_ty: Option<&Type>) -> String {
    if matches!(err_ty, Some(Type::String)) {
        return String::new();
    }
    let error = mangle_generated("harness_error");
    format!(
        ".map_err(|{error}| {}{}(&{error}))",
        cx.root_prefix,
        harness_error_renderer(cx, err_ty)
    )
}

/// A chain rooted at `#Todo` diverges before any adapter can run. Emit that
/// carrier directly so Rust does not try to type-check collection wrappers
/// around its never value.
pub(crate) fn emit_tir_stopping_receiver(recv: &TExpr, cx: &Cx) -> Option<String> {
    if matches!(&recv.ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_NEVER) {
        return Some(emit_tir_expr(recv, cx));
    }
    match &recv.kind {
        TExprKind::Todo { .. } => Some(emit_tir_expr(recv, cx)),
        TExprKind::BuiltinMethod { recv, .. } | TExprKind::ClosureMethod { recv, .. } => {
            emit_tir_stopping_receiver(recv, cx)
        }
        TExprKind::StrLit(parts) => parts.iter().find_map(|part| match part {
            TStrPart::Interp(expr, _) => emit_tir_stopping_receiver(expr, cx),
            TStrPart::Lit(_) => None,
        }),
        _ => None,
    }
}

/// The Rust pattern a `TPattern` spells. The position decides which of codegen's
/// three pattern shapes applies; the pattern and its owning enum are the only
/// other facts, both already resolved at lowering.
pub(crate) fn emit_tir_pattern(pattern: &TPattern, cx: &Cx) -> String {
    match &pattern.position {
        TPatternPosition::Binding => crate::Codegen::emit_if_let_pattern(cx, &pattern.pattern),
        TPatternPosition::OptionBinding => match &pattern.pattern {
            crate::AST::Pattern::Present { binding, .. } => {
                format!("Ok({})", mangle(binding))
            }
            crate::AST::Pattern::Absent(_) => "Err(_)".to_string(),
            _ => crate::Codegen::emit_if_let_pattern(cx, &pattern.pattern),
        },
        TPatternPosition::Arm => {
            crate::Codegen::emit_match_pattern(cx, &pattern.pattern, pattern.enum_type.as_deref())
        }
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
            let access = a.fact_channel().exclusivity;
            let uninit_borrow = match &a.value.kind {
                crate::Codegen::TIR::TExprKind::Local(local)
                    if local.uninit_fixed && matches!(access, TExclusivity::Exclusive) =>
                {
                    Some(format!("({}).as_array_mut()", local.rust_place()))
                }
                crate::Codegen::TIR::TExprKind::Local(local)
                    if local.uninit_fixed && matches!(access, TExclusivity::Shared) =>
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
            // S48: a concrete value entering a trait value slot boxes invisibly.
            // `rust_param_type` renders that slot as `Box<dyn __jet_<Trait>>`
            // (`&Box<dyn …>` under `Read`), so the wrapper goes on before the
            // borrow wrapper below. Same spelling as the `ListLit` element box.
            if let Some(trait_name) = &a.box_as_trait {
                s = format!(
                    "Box::new({s}) as Box<dyn {}>",
                    crate::Codegen::rust_trait_name(trait_name)
                );
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
                    s = format!("{wrap}({s}) as {rust_ty}");
                }
            }
            if matches!(access, TExclusivity::Shared) && uninit_borrow.is_none() {
                s = format!("&({})", s);
            } else if matches!(access, TExclusivity::Exclusive) && uninit_borrow.is_none() {
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
                if let Some(stop) = emit_tir_stopping_receiver(e, cx) {
                    return stop;
                }
                if matches!(e.ty, crate::AST::Type::Int) {
                    fmt.push_str("{}");
                    args.push(format!(
                        "{}jet_std::jet_int_to_string({})",
                        cx.root_prefix,
                        emit_tir_expr(e, cx)
                    ));
                    continue;
                }
                let method = match format {
                    crate::AST::StrFormat::Display => "jet_display",
                    crate::AST::StrFormat::Debug => "jet_debug",
                    crate::AST::StrFormat::Pretty => {
                        unreachable!("Pretty interpolation lowers to core.text.fmt.pretty")
                    }
                    crate::AST::StrFormat::Fixed(_) => {
                        unreachable!("Fixed interpolation lowers to core.text.fmt.decimal")
                    }
                    crate::AST::StrFormat::Grouped(_) => {
                        unreachable!("Grouped interpolation lowers to core.text.fmt.grouped")
                    }
                    crate::AST::StrFormat::Hex(_) => {
                        unreachable!("Hex interpolation lowers to core.text.fmt.hex")
                    }
                    crate::AST::StrFormat::Pad { .. } => {
                        unreachable!("Pad interpolation lowers to core.text.fmt.pad")
                    }
                    crate::AST::StrFormat::PadLeft { .. } => {
                        unreachable!("PadLeft interpolation lowers to core.text.fmt.pad_left")
                    }
                    crate::AST::StrFormat::Sci(_) => {
                        unreachable!("Sci interpolation lowers to core.text.fmt.sci")
                    }
                    crate::AST::StrFormat::Percent(_) => {
                        unreachable!("Percent interpolation lowers to core.text.fmt.percent")
                    }
                    crate::AST::StrFormat::Bin => {
                        unreachable!("Bin interpolation lowers to core.text.fmt.bin")
                    }
                    crate::AST::StrFormat::Oct => {
                        unreachable!("Oct interpolation lowers to core.text.fmt.oct")
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

/// Walk the compiler-private readiness-table chain and collect its arms.
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
                duration,
                value,
            } => {
                let duration = emit_tir_expr(duration, cx);
                let ms = format!(
                    "{}jet_std_time_duration_to_millis(({}).ns)",
                    cx.root_prefix, duration
                );
                let value = value
                    .as_ref()
                    .map(|v| emit_tir_expr(v, cx))
                    .unwrap_or_else(|| "()".to_string());
                afters.push(format!("({ms}, {value})"));
                cur = inner;
            }
            _ => break,
        }
    }
    (recvs, afters)
}

/// Collect only Duration expressions from a readiness wait, preserving source
/// arm order. The tagged Prelude door owns the ns-to-scheduler-time conversion.
pub(super) fn collect_select_after_durations(builder: &TExpr, cx: &Cx) -> Vec<String> {
    let mut durations = Vec::new();
    let mut cur = builder;
    loop {
        match &cur.kind {
            TExprKind::SelectStart => break,
            TExprKind::SelectRecv { builder: inner, .. } => cur = inner,
            TExprKind::SelectAfter {
                builder: inner,
                duration,
                ..
            } => {
                durations.push(format!("({}).ns", emit_tir_expr(duration, cx)));
                cur = inner;
            }
            _ => break,
        }
    }
    durations
}

/// D-SWIZZLE1: render a read swizzle as lane extract(s) and optional `VecN` ctor.
pub(super) fn emit_math_swizzle_read(
    cx: &Cx,
    type_name: &str,
    recv: &TExpr,
    lanes: &[u8],
) -> String {
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
pub(super) fn emit_math_swizzle_assign_stmt(
    base: &str,
    type_name: &str,
    lanes: &[u8],
    value: &str,
) -> String {
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
        TLetTy::Annotated {
            ty,
            mut_fn,
            wrapper,
        } => {
            let base = if let Type::Fn {
                params,
                ret,
                return_view_provenance,
                ..
            } = ty
            {
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
                TLetWrapper::Resource => format!("{}JetResource<{base}>", cx.root_prefix),
                TLetWrapper::AutomaticRoot => {
                    format!("{}jet_gc::AutomaticRoot<{base}>", cx.root_prefix)
                }
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
        "{{ if !({cond}) {{ {cleanup} {root}jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{msg}, &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }} }}",
        cleanup = RESOURCE_CLEANUP_MARKER,
        root = cx.root_prefix,
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

pub(crate) fn emit_require_stop(kind: &TRequireKind, loc: &TPanicLoc, cx: &Cx) -> String {
    match kind {
        TRequireKind::Require { cond, msg } => {
            let cond_s = emit_tir_expr(cond, cx);
            if cx.test_mode && is_test_harness_fn(cx) {
                let msg_s = match msg {
                    Some(m) => emit_panic_message_expr(m, cx),
                    None => "\"condition failed\".to_string()".to_string(),
                };
                return jet_format!(
                    "{{ if !({cond_s}) {{ let {jet_prefix}msg = {msg_s}; return Err({root}jet_test_failure({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{jet_prefix}msg)); }} }}",
                    root = cx.root_prefix,
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
            // Both operands are only compared and rendered, so bind them as
            // read windows. A borrowed non-scalar parameter emits `(*__jet_p)`,
            // and binding that by value would move out of a shared reference
            // (E0507); `&` keeps the caller's access convention intact and
            // copies nothing. `==` and `jet_debug` both work through the
            // reference, so the reported text is unchanged.
            if cx.test_mode && is_test_harness_fn(cx) {
                return jet_format!(
                    "{{ let {jet_prefix}left = &({left_s}); let {jet_prefix}right = &({right_s}); if !({jet_prefix}left == {jet_prefix}right) {{ let {jet_prefix}msg = format!(\"expected {{}}, got {{}}\", {jet_prefix}right.jet_debug(), {jet_prefix}left.jet_debug()); return Err({root}jet_test_failure({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{jet_prefix}msg)); }} }}",
                    root = cx.root_prefix,
                    file = escape_rust_str(&loc.file),
                    line = loc.line,
                    fn_name_esc = escape_rust_str(&loc.fn_name),
                    src_line_esc = escape_rust_str(&loc.src_line),
                    col = loc.col,
                    caret = loc.caret,
                );
            }
            jet_format!(
                "{{ let {jet_prefix}left = &({left_s}); let {jet_prefix}right = &({right_s}); if !({jet_prefix}left == {jet_prefix}right) {{ {cleanup} {root}jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &format!(\"expected: {{}}, got: {{}}\", {jet_prefix}right.jet_debug(), {jet_prefix}left.jet_debug()), &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }} }}",
                cleanup = RESOURCE_CLEANUP_MARKER,
                root = cx.root_prefix,
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
            // `assert` / `assert_eq` are caught harness assertions.
            format!(
                "{{ {cleanup} {root}jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{msg_s}, &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }}",
                cleanup = RESOURCE_CLEANUP_MARKER,
                root = cx.root_prefix,
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
