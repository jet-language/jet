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
fn collect_select_arms(builder: &TExpr, cx: &Cx) -> (Vec<String>, Vec<String>) {
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
fn emit_math_swizzle_read(cx: &Cx, type_name: &str, recv: &TExpr, lanes: &[u8]) -> String {
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
fn emit_math_swizzle_assign_stmt(base: &str, type_name: &str, lanes: &[u8], value: &str) -> String {
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
