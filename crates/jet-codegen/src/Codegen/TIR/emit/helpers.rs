use crate::Codegen::Cx;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::TCallArg;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TPattern;
use crate::Codegen::TIR::TPatternPosition;
use crate::Codegen::TIR::TPlace;
use crate::Codegen::TIR::TPreludeArg;
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

/// c109 Phase 6: format call/method arguments, reproducing `emit_call_args`
/// (Source/Codegen/Expression.rs) byte-for-byte. The clone wrapper (`.clone()` or
/// `Arc::clone(&…)`) is applied to the raw value first, then the borrow wrapper
/// (`&(…)` for a `Read` non-scalar, `&mut (…)` for a `Mutate`). All four decisions
/// are total TIR flags — emit makes no convention decision.
pub(crate) fn emit_tir_call_args(args: &[TCallArg], cx: &Cx) -> String {
    args.iter()
        .map(|a| {
            let mut s = emit_tir_expr(&a.value, cx);
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
            // c109 Phase 13: the Fn-typed Box-coercion, applied AFTER the clone wrapper
            // and BEFORE the borrow wrapper — exactly `emit_call_args`' order. `Box::new`
            // is added only when the value isn't already boxed (resolved at lowering).
            if let Some(fc) = &a.fn_coerce {
                if !fc.already_boxed {
                    s = format!("Box::new({})", s);
                }
                s = format!("{} as {}", s, fc.fn_type_rust);
            }
            if a.borrow {
                s = format!("&({})", s);
            } else if a.mut_borrow {
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
    let mut body = String::from("{ let mut _jet_s = String::new(); ");
    for p in parts {
        match p {
            TStrPart::Lit(s) => {
                if !s.is_empty() {
                    body.push_str(&format!("_jet_s.push_str({:?}); ", s));
                }
            }
            TStrPart::Interp(e, fmt) => {
                let method = match fmt {
                    crate::AST::StrFormat::Display => "jet_display",
                    crate::AST::StrFormat::Debug => "jet_debug",
                };
                body.push_str(&format!(
                    "_jet_s.push_str(&({}).{method}()); ",
                    emit_tir_expr(e, cx)
                ));
            }
        }
    }
    body.push_str("_jet_s }");
    body
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
        return format!("{{ let __jet_v = {val}; ({base}).0[{lane}] = __jet_v; }}");
    }
    let writes: Vec<String> = lanes
        .iter()
        .enumerate()
        .map(|(i, &l)| {
            let lane = l as usize;
            let comp = if type_name == "F32x4" {
                format!("(__jet_v).0[{i}] as f32")
            } else if type_name == "F64x2" && lanes.len() == 2 {
                format!("(__jet_v).0[{i}]")
            } else {
                format!("(__jet_v).0[{i}]")
            };
            format!("({base}).0[{lane}] = {comp}")
        })
        .collect();
    format!("{{ let __jet_v = {value}; {}; }}", writes.join("; "))
}
