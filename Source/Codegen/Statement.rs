use super::*;
use crate::AST::{
    BinOp, BindPattern, ElseBranch, Expr,
    ForKind, IfStmt, IndexKind, LValue, OrFallback, Pattern, Stmt, Type, VariantPayload,
};
use crate::Diagnostics::{span_line_col, Span};
use crate::Syntax;
use std::collections::HashMap;
pub(crate) fn emit_stmts(
    cx: &Cx,
    stmts: &[Stmt],
    env: &mut HashMap<String, Slot>,
    out: &mut String,
    indent: usize,
    view_return: bool,
) {
    for stmt in stmts {
        emit_stmt(cx, stmt, env, out, indent, view_return);
    }
}

fn emit_stmt(
    cx: &Cx,
    stmt: &Stmt,
    env: &mut HashMap<String, Slot>,
    out: &mut String,
    indent: usize,
    view_return: bool,
) {
    let pad = "    ".repeat(indent);
    match stmt {
        Stmt::Val(b) if b.pattern.is_some() => {
            // S74: destructuring binding. Evaluate the initializer once into a
            // temp, then bind each name from a field/element of it.
            let pat = b.pattern.as_ref().unwrap();
            let tmp = format!("__jet_d{}", pat.span().start);
            let init = emit_expr(cx, &b.init, env);
            // Borrow the initializer (we clone the parts we bind) so that
            // destructuring a `view` parameter or any borrowed value never
            // moves out of a shared reference (I2).
            out.push_str(&format!("{}let {} = &({});\n", pad, tmp, init));
            let mut_kw = if b.mutable { "let mut" } else { "let" };
            match pat {
                BindPattern::Struct { fields, .. } => {
                    let field_tys: HashMap<String, Type> = expr_jet_ty(&b.init, env)
                        .as_ref()
                        .and_then(|t| match t {
                            Type::Named(n) | Type::Apply { name: n, .. } => {
                                cx.struct_fields.get(n)
                            }
                            _ => None,
                        })
                        .map(|fs| fs.iter().cloned().collect())
                        .unwrap_or_default();
                    for f in fields {
                        let m = mangle(&f.name);
                        out.push_str(&format!(
                            "{}{} {} = ({}).{}.clone();\n",
                            pad, mut_kw, m, tmp, m
                        ));
                        env.insert(
                            f.name.clone(),
                            Slot {
                                rust_name: m,
                                deref: false,
                                jet_ty: field_tys.get(&f.name).cloned(),
                            },
                        );
                    }
                }
                BindPattern::List { elems, span } => {
                    let elem_ty = match expr_jet_ty(&b.init, env) {
                        Some(Type::List(inner)) => Some(*inner),
                        _ => None,
                    };
                    let (line, _) = span_line_col(&cx.src, span.start);
                    let want = elems.len();
                    for (i, e) in elems.iter().enumerate() {
                        let m = mangle(&e.name);
                        out.push_str(&format!(
                            "{}{} {} = jet_unpack_vec({}, {}, {}, {:?}, {});\n",
                            pad, mut_kw, m, tmp, want, i, cx.file, line
                        ));
                        env.insert(
                            e.name.clone(),
                            Slot {
                                rust_name: m,
                                deref: false,
                                jet_ty: elem_ty.clone(),
                            },
                        );
                    }
                }
                BindPattern::Tuple { elems, .. } => {
                    let field_names = expr_jet_ty(&b.init, env)
                        .and_then(|t| match t {
                            Type::Tuple(fs) => Some(
                                fs.iter()
                                    .map(|(n, ty)| (n.clone(), (**ty).clone()))
                                    .collect::<Vec<_>>(),
                            ),
                            _ => None,
                        })
                        .unwrap_or_default();
                    for (e, (fname, fty)) in elems.iter().zip(field_names.iter()) {
                        let m = mangle(&e.name);
                        let field = mangle(fname);
                        out.push_str(&format!(
                            "{}{} {} = ({}).{}.clone();\n",
                            pad, mut_kw, m, tmp, field
                        ));
                        env.insert(
                            e.name.clone(),
                            Slot {
                                rust_name: m,
                                deref: false,
                                jet_ty: Some(fty.clone()),
                            },
                        );
                    }
                }
            }
        }
        Stmt::Val(b) => {
            let mut_fn = matches!(
                &b.init,
                Expr::Lambda(l) if l.meta.escapes && l.meta.needs_fn_mut
            );
            let mut init = if b.is_comptime {
                b.ct.as_ref()
                    .map(|v| v.serialize())
                    .unwrap_or_else(|| "Default::default()".to_string())
            } else {
                emit_expr(cx, &b.init, env)
            };
            if matches!(b.ty, Some(Type::Named(ref n)) if n == "U8")
                && matches!(b.init, Expr::Int(_, _))
            {
                init = format!("({}) as u8", init);
            }
            if mut_fn {
                if let Some(Type::Fn { params, ret }) = &b.ty {
                    init = format!(
                        "{} as {}",
                        init,
                        cx.rust_fn_trait(params, ret.as_deref(), true)
                    );
                }
            }
            // E2-M7: file handles need `let mut` even when bound with `val`,
            // because streaming reads and writes mutate the internal buffer state.
            // E2-M10: TcpStream also needs `let mut` for the same reason.
            let is_file_handle = matches!(
                &b.ty,
                Some(Type::Named(n)) if n == "FileReader" || n == "FileWriter" || n == "TcpStream"
            );
            let kw = if (b.mutable && !b.is_comptime) || mut_fn || is_file_handle {
                "let mut"
            } else {
                "let"
            };
            let ty =
                b.ty.as_ref()
                    .map(|t| {
                        if let Type::Fn { params, ret } = t {
                            format!(": {}", cx.rust_fn_trait(params, ret.as_deref(), mut_fn))
                        } else {
                            format!(": {}", cx.rust_type(t))
                        }
                    })
                    .unwrap_or_default();
            out.push_str(&format!(
                "{}{} {}{} = {};\n",
                pad,
                kw,
                mangle(&b.name),
                ty,
                init
            ));
            env.insert(
                b.name.clone(),
                Slot {
                    rust_name: mangle(&b.name),
                    deref: false,
                    jet_ty: b.ty.clone(),
                },
            );
        }
        Stmt::Assign {
            target, op, value, ..
        } => {
            let v = emit_expr(cx, value, env);
            match target {
                LValue::Local { name, .. } => {
                    let place = place_of(env, name);
                    match op {
                        Some(op) => {
                            out.push_str(&format!("{}{} {}= {};\n", pad, place, op.spell(), v))
                        }
                        None => out.push_str(&format!("{}{} = {};\n", pad, place, v)),
                    }
                }
                LValue::Index {
                    base, index, kind, ..
                } => {
                    // Sema resolved the collection kind (R2); fall back to
                    // the env type only for un-checked trees (tests).
                    let is_map = matches!(kind, IndexKind::Map)
                        || (matches!(kind, IndexKind::Unknown)
                            && matches!(expr_jet_ty(base, env), Some(Type::Map { .. })));
                    let b = emit_expr(cx, base, env);
                    let i = emit_expr(cx, index, env);
                    if is_map {
                        out.push_str(&format!(
                            "{pad}{{ let __jet_v = {v}; jet_map_insert(&mut ({b}), ({i}).clone(), __jet_v); }}\n",
                        ));
                    } else {
                        out.push_str(&format!(
                            "{pad}{{ let __jet_v = {v}; ({b})[{i} as usize] = __jet_v; }}\n",
                        ));
                    }
                }
            }
        }
        Stmt::Expr(e) => {
            out.push_str(&format!("{}{};\n", pad, emit_expr_stmt(cx, e, env)));
        }
        Stmt::Return(Some(e), _) => {
            let val = if view_return {
                emit_view_return(cx, e, env)
            } else {
                emit_expr(cx, e, env)
            };
            out.push_str(&format!("{}return {};\n", pad, val));
        }
        Stmt::Return(None, _) => {
            out.push_str(&format!("{}return;\n", pad));
        }
        Stmt::If(ifs) => emit_if(cx, ifs, env, out, indent, view_return),
        Stmt::While { cond, body, label, .. } => {
            out.push_str(&format!(
                "{}{}while {} {{\n",
                pad,
                loop_label_prefix(label),
                emit_expr(cx, cond, env)
            ));
            emit_stmts(cx, body, env, out, indent + 1, view_return);
            out.push_str(&format!("{}}}\n", pad));
        }
        Stmt::For {
            var,
            var2,
            kind,
            body,
            label,
            ..
        } => match kind {
            ForKind::Range { start, end, step } => {
                let s = emit_expr(cx, start, env);
                let e = emit_expr(cx, end, env);
                let lbl = loop_label_prefix(label);
                match step {
                    // S22 (D-SG8): `.step_by` takes a usize; sema has already
                    // checked the stride is a positive Int.
                    Some(step) => {
                        let st = emit_expr(cx, step, env);
                        out.push_str(&format!(
                            "{}{}for {} in (({})..=({})).step_by(({}) as usize) {{\n",
                            pad,
                            lbl,
                            mangle(var),
                            s,
                            e,
                            st
                        ));
                    }
                    None => {
                        out.push_str(&format!(
                            "{}{}for {} in ({})..=({}) {{\n",
                            pad,
                            lbl,
                            mangle(var),
                            s,
                            e
                        ));
                    }
                }
                let prev = env.insert(
                    var.clone(),
                    Slot {
                        rust_name: mangle(var),
                        deref: false,
                        jet_ty: Some(Type::Int),
                    },
                );
                emit_stmts(cx, body, env, out, indent + 1, view_return);
                match prev {
                    Some(p) => {
                        env.insert(var.clone(), p);
                    }
                    None => {
                        env.remove(var);
                    }
                }
                out.push_str(&format!("{}}}\n", pad));
            }
            ForKind::In { collection } => {
                emit_for_in(
                    cx,
                    var,
                    var2.as_ref(),
                    collection,
                    body,
                    env,
                    out,
                    indent,
                    view_return,
                    &loop_label_prefix(label),
                );
            }
        },
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => {
            if is_exhaustive_pattern_switch(cx, subject, arms) {
                emit_pattern_match_switch(cx, subject, arms, else_body, env, out, indent);
            } else {
                emit_mixed_switch(cx, subject, arms, else_body, env, out, indent, view_return);
            }
        }
        Stmt::Break(_) => out.push_str(&format!("{}break;\n", pad)),
        Stmt::Continue(_) => out.push_str(&format!("{}continue;\n", pad)),
        // D-LABEL1: labeled `break @name` / `continue @name`.
        Stmt::BreakLabel(name, _) => {
            out.push_str(&format!("{}break 'jet_{};\n", pad, name))
        }
        Stmt::ContinueLabel(name, _) => {
            out.push_str(&format!("{}continue 'jet_{};\n", pad, name))
        }
        Stmt::Loop { body: inner, label, .. } => {
            out.push_str(&format!("{}{}loop {{\n", pad, loop_label_prefix(label)));
            emit_stmts(cx, inner, env, out, indent + 1, view_return);
            out.push_str(&format!("{}}}\n", pad));
        }
        Stmt::Unsafe { body, .. } => {
            // S58 (E2-M13, D-LL1): codegen is dumb — a gated `@unsafe { … }`
            // region lowers straight to a Rust `unsafe { … }`. All safety
            // checking already happened in sema.
            out.push_str(&format!("{}unsafe {{\n", pad));
            emit_stmts(cx, body, env, out, indent + 1, view_return);
            out.push_str(&format!("{}}}\n", pad));
        }
    }
}

fn switch_arm_pattern_owned(cx: &Cx, cond: &Expr, subject: &Expr) -> Option<Pattern> {
    match cond {
        Expr::PatternTest {
            subject: s,
            pattern,
            ..
        } if pattern_subjects_match(s, subject) => Some(pattern.clone()),
        Expr::Binary(crate::AST::BinOp::Eq, lhs, rhs, span)
            if pattern_subjects_match(lhs, subject) =>
        {
            if let Expr::Ident(variant, rhs_span) = rhs.as_ref() {
                if cx.variant_owner.contains_key(variant) {
                    return Some(Pattern::Variant {
                        variant: variant.clone(),
                        bindings: Vec::new(),
                        span: *rhs_span,
                    });
                }
            }
            None
        }
        _ => None,
    }
}

fn is_exhaustive_pattern_switch(cx: &Cx, subject: &Expr, arms: &[crate::AST::SwitchArm]) -> bool {
    !arms.is_empty()
        && arms
            .iter()
            .all(|a| switch_arm_pattern_owned(cx, &a.cond, subject).is_some())
}

fn pattern_subjects_match(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Ident(na, _), Expr::Ident(nb, _)) => na == nb,
        (Expr::Ident(n, _), _) if n == Syntax::KW_IT => true,
        _ => false,
    }
}

fn emit_pattern_match_switch(
    cx: &Cx,
    subject: &Expr,
    arms: &[crate::AST::SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    env: &mut HashMap<String, Slot>,
    out: &mut String,
    indent: usize,
) {
    let pad = "    ".repeat(indent);
    // When the subject is a by-reference slot (parameter convention), clone it
    // so the match owns the value.  This avoids both "move out of reference"
    // errors (deref-and-move) and `&&T` double-reference bindings that arise
    // when matching on `&T` via ergonomics.
    let subj = match subject {
        Expr::Ident(name, _) => match env.get(name.as_str()) {
            Some(slot) if slot.deref => format!("({}).clone()", slot.rust_name),
            _ => emit_expr(cx, subject, env),
        },
        _ => emit_expr(cx, subject, env),
    };
    let enum_type = arms.iter().find_map(|a| {
        switch_arm_pattern_owned(cx, &a.cond, subject).and_then(|pattern| {
            if let Pattern::Variant { variant, .. } = pattern {
                cx.variant_owner.get(&variant).cloned()
            } else {
                None
            }
        })
    });
    let subject_ty = expr_jet_ty_with_cx(cx, subject, env);
    out.push_str(&format!("{}match {} {{\n", pad, subj));
    for arm in arms {
        if let Some(pattern) = switch_arm_pattern_owned(cx, &arm.cond, subject) {
            let pat = emit_match_pattern(cx, &pattern, enum_type.as_deref());
            out.push_str(&format!("{}    {} => {{\n", pad, pat));
            let mut body_env = env.clone();
            add_pattern_bindings(cx, &pattern, &mut body_env, subject_ty.as_ref());
            emit_stmts(cx, &arm.body, &mut body_env, out, indent + 2, false);
            out.push_str(&format!("{}    }}\n", pad));
        }
    }
    if let Some(body) = else_body {
        out.push_str(&format!("{}    _ => {{\n", pad));
        emit_stmts(cx, body, env, out, indent + 2, false);
        out.push_str(&format!("{}    }}\n", pad));
    }
    out.push_str(&format!("{}}}\n", pad));
}

fn emit_mixed_switch(
    cx: &Cx,
    subject: &Expr,
    arms: &[crate::AST::SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    env: &mut HashMap<String, Slot>,
    out: &mut String,
    indent: usize,
    view_return: bool,
) {
    let pad = "    ".repeat(indent);
    out.push_str(&format!("{}{{\n", pad));
    let inner_pad = "    ".repeat(indent + 1);
    out.push_str(&format!(
        "{}let _jet_switch_subject = &({});\n",
        inner_pad,
        emit_expr(cx, subject, env)
    ));
    for (i, arm) in arms.iter().enumerate() {
        let kw = if i == 0 { "if" } else { "} else if" };
        out.push_str(&format!(
            "{}{} {} {{\n",
            inner_pad,
            kw,
            emit_switch_arm_cond(cx, &arm.cond, env)
        ));
        emit_stmts(cx, &arm.body, env, out, indent + 2, view_return);
    }
    match else_body {
        None if !arms.is_empty() => {
            out.push_str(&format!("{}}}\n", inner_pad));
        }
        None => {}
        Some(body) if arms.is_empty() => {
            emit_stmts(cx, body, env, out, indent + 1, view_return);
        }
        Some(body) => {
            out.push_str(&format!("{}}} else {{\n", inner_pad));
            emit_stmts(cx, body, env, out, indent + 2, view_return);
            out.push_str(&format!("{}}}\n", inner_pad));
        }
    }
    out.push_str(&format!("{}}}\n", pad));
}

fn emit_switch_arm_cond(cx: &Cx, cond: &Expr, env: &HashMap<String, Slot>) -> String {
    let subject = match cond {
        Expr::PatternTest { subject, .. } => subject.as_ref(),
        Expr::Binary(crate::AST::BinOp::Eq, lhs, _, _) => lhs.as_ref(),
        _ => return emit_expr(cx, cond, env),
    };
    if let Some(pattern) = switch_arm_pattern_owned(cx, cond, subject) {
        let subj = emit_expr(cx, subject, env);
        return emit_pattern_matches(cx, &subj, &pattern);
    }
    emit_expr(cx, cond, env)
}

pub(crate) fn emit_pattern_matches(cx: &Cx, subject: &str, pattern: &Pattern) -> String {
    match pattern {
        Pattern::Variant {
            variant, bindings, ..
        } => {
            let prefix = enum_type_prefix(cx, variant);
            if bindings.is_empty() {
                format!(
                    "matches!({}, {}::{})",
                    subject,
                    prefix,
                    variant_rust_name(variant)
                )
            } else if bindings.len() == 1 {
                format!(
                    "matches!({}, {}::{}({}))",
                    subject,
                    prefix,
                    variant_rust_name(variant),
                    mangle(&bindings[0])
                )
            } else {
                format!(
                    "matches!({}, {}::{} {{ {} }})",
                    subject,
                    prefix,
                    variant_rust_name(variant),
                    bindings
                        .iter()
                        .map(|n| format!("{}: {}", mangle(n), mangle(n)))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        Pattern::Present { binding, .. } => {
            format!("matches!({}, Some({}))", subject, mangle(binding))
        }
        Pattern::Absent(_) => format!("({}).is_none()", subject),
        Pattern::Ok { binding, .. } => {
            format!("matches!({}, Ok({}))", subject, mangle(binding))
        }
        Pattern::Err { binding, .. } => {
            format!("matches!({}, Err({}))", subject, mangle(binding))
        }
    }
}

fn emit_match_pattern(cx: &Cx, pattern: &Pattern, enum_type: Option<&str>) -> String {
    let is_json = enum_type.map(is_json_type_name).unwrap_or(false);
    let prefix = enum_type
        .map(|t| {
            if is_json_type_name(t) {
                format!("{}jet_std::Json", cx.root_prefix)
            } else if let Some(rust_mod) = cx.foreign_types.get(t) {
                format!("{}{}::user_{}", cx.root_prefix, rust_mod, t)
            } else {
                format!("user_{}", t)
            }
        })
        .unwrap_or_else(|| "user_TYPE".to_string());
    // Variant names are mangled for user enums, but JSON variants keep their
    // original Rust name (they are defined as plain Rust identifiers in std.rs).
    let vname = |v: &str| -> String {
        if is_json { v.to_string() } else { mangle(v) }
    };
    match pattern {
        Pattern::Variant {
            variant, bindings, ..
        } => {
            if bindings.is_empty() {
                format!("{}::{}", prefix, vname(variant))
            } else if bindings.len() == 1 {
                format!(
                    "{}::{}({})",
                    prefix,
                    vname(variant),
                    mangle(&bindings[0])
                )
            } else {
                let fields = bindings
                    .iter()
                    .map(|b| format!("{}: {}", mangle(b), mangle(b)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "{}::{} {{ {} }}",
                    prefix,
                    vname(variant),
                    fields
                )
            }
        }
        Pattern::Present { binding, .. } => format!("Some({})", mangle(binding)),
        Pattern::Absent(_) => "None".to_string(),
        Pattern::Ok { binding, .. } => format!("Ok({})", mangle(binding)),
        Pattern::Err { binding, .. } => format!("Err({})", mangle(binding)),
    }
}

pub(crate) fn emit_or_fallback(
    cx: &Cx,
    value: &Expr,
    fallback: &OrFallback,
    is_option: bool,
    env: &HashMap<String, Slot>,
) -> String {
    if is_option {
        return emit_or_fallback_option(cx, value, fallback, env);
    }
    let v = emit_expr(cx, value, env);
    let fallback_expr = emit_or_fallback_rhs(cx, fallback, env);
    format!(
        "match {} {{ Ok(__jet_ok) => __jet_ok, Err(_) => {} }}",
        v, fallback_expr
    )
}

fn emit_or_fallback_option(
    cx: &Cx,
    value: &Expr,
    fallback: &OrFallback,
    env: &HashMap<String, Slot>,
) -> String {
    let v = emit_expr(cx, value, env);
    let fallback_expr = emit_or_fallback_rhs(cx, fallback, env);
    format!(
        "match {} {{ Some(__jet_v) => __jet_v, None => {} }}",
        v, fallback_expr
    )
}

fn emit_or_fallback_rhs(cx: &Cx, fallback: &OrFallback, env: &HashMap<String, Slot>) -> String {
    match fallback {
        OrFallback::Value(e) => emit_expr(cx, e, env),
        OrFallback::Return(None, _) => "return".to_string(),
        OrFallback::Return(Some(e), _) => format!("return {}", emit_expr(cx, e, env)),
        OrFallback::Panic { name_span, args } => {
            let call = crate::AST::Call {
                name: Syntax::BUILTIN_PANIC.to_string(),
                name_span: *name_span,
                args: args.clone(),
            };
            emit_panic_stop(cx, &call, env)
        }
    }
}

/// E2-M12 D-OBS1: return the (source_line_text, line_number, col_1based) for a byte offset.
fn src_line_at(src: &str, offset: usize) -> (&str, u32, u32) {
    let (line, col) = span_line_col(src, offset);
    let line_start = src[..offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let line_end = src[offset..].find('\n').map(|p| offset + p).unwrap_or(src.len());
    (&src[line_start..line_end], line as u32, col as u32)
}

/// E2-M12 D-OBS2: build a Rust expression that evaluates to a comma-separated
/// "name = value" string for all safe (Copy scalar: Int/Float/Bool) locals.
/// Non-scalar locals are excluded because they might be moved before the panic
/// site, which would cause a compile error in the generated Rust.
/// In release builds the caller passes "" directly; this expression is only
/// used inside `if cfg!(debug_assertions) { … }` blocks.
fn safe_locals_expr(env: &HashMap<String, Slot>) -> String {
    let mut parts: Vec<(String, String)> = env
        .iter()
        .filter_map(|(name, slot)| {
            let safe = slot.jet_ty.as_ref().map_or(false, |t| {
                matches!(t, Type::Int | Type::Float | Type::Bool)
            });
            if !safe {
                return None;
            }
            let value_expr = if slot.deref {
                format!("(*{}).jet_show()", slot.rust_name)
            } else {
                format!("({}).jet_show()", slot.rust_name)
            };
            Some((name.clone(), value_expr))
        })
        .collect();
    // Stable sort so output is deterministic.
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

pub(crate) fn emit_panic_stop(cx: &Cx, call: &crate::AST::Call, env: &HashMap<String, Slot>) -> String {
    let msg = emit_panic_message(cx, &call.args[0].expr, env);
    let (src_line, line, col) = src_line_at(&cx.src, call.name_span.start);
    let caret_len = (call.name_span.end - call.name_span.start) as u32;
    let fn_name = cx.current_fn.borrow().clone();
    let locals_expr = safe_locals_expr(env);
    format!(
        "{{ jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{msg}, &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }}",
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

fn emit_panic_message(cx: &Cx, e: &Expr, env: &HashMap<String, Slot>) -> String {
    match e {
        Expr::Str(parts, _) => emit_str(cx, parts, env),
        other => format!("({}).jet_show()", emit_expr(cx, other, env)),
    }
}

pub(crate) fn emit_require(cx: &Cx, call: &crate::AST::Call, env: &HashMap<String, Slot>) -> String {
    let cond = emit_expr(cx, &call.args[0].expr, env);
    let msg = if call.args.len() == 2 {
        emit_panic_message(cx, &call.args[1].expr, env)
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
    let (src_line, line, col) = src_line_at(&cx.src, call.name_span.start);
    let caret_len = (call.name_span.end - call.name_span.start) as u32;
    let fn_name = cx.current_fn.borrow().clone();
    let locals_expr = safe_locals_expr(env);
    let msg_used = if call.args.len() == 2 { msg } else { "\"condition failed\".to_string()".to_string() };
    format!(
        "{{ if !({cond}) {{ jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{msg}, &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }} }}",
        cond = cond,
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

pub(crate) fn emit_require_eq(cx: &Cx, call: &crate::AST::Call, env: &HashMap<String, Slot>) -> String {
    let left = emit_expr(cx, &call.args[0].expr, env);
    let right = emit_expr(cx, &call.args[1].expr, env);
    if cx.test_mode {
        return format!(
            "{{ let _jet_left = ({}); let _jet_right = ({}); if !(_jet_left == _jet_right) {{ return Err(format!(\"left: {{}}, right: {{}}\", _jet_left.jet_show(), _jet_right.jet_show())); }} }}",
            left, right
        );
    }
    let (src_line, line, col) = src_line_at(&cx.src, call.name_span.start);
    let caret_len = (call.name_span.end - call.name_span.start) as u32;
    let fn_name = cx.current_fn.borrow().clone();
    let locals_expr = safe_locals_expr(env);
    format!(
        "{{ let _jet_left = ({left}); let _jet_right = ({right}); if !(_jet_left == _jet_right) {{ jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &format!(\"left: {{}}, right: {{}}\", _jet_left.jet_show(), _jet_right.jet_show()), &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }} }}",
        left = left,
        right = right,
        file = escape_rust_str(&cx.file),
        line = line,
        fn_name_esc = escape_rust_str(&fn_name),
        src_line_esc = escape_rust_str(src_line.trim_end()),
        col = col,
        caret = caret_len,
        locals = locals_expr,
    )
}

/// `-> view T` returns a reference; emit `&place` or the existing borrow.
fn emit_view_return(cx: &Cx, e: &Expr, env: &HashMap<String, Slot>) -> String {
    match e {
        Expr::Ident(name, _) => {
            if let Some(c) = cx.consts.get(name) {
                return format!("&{}", c);
            }
            if let Some(slot) = env.get(name) {
                if slot.deref {
                    return slot.rust_name.clone();
                }
                return format!("&{}", slot.rust_name);
            }
            place_of(env, name)
        }
        // A `view` into a field of an owned root (a parameter) hands back a
        // borrow of a place, so take its address. `emit_expr` yields the place
        // expression; `&` makes it the `&T` the signature promises. (E2-M5
        // generic / zero-copy cell — sema proved the root outlives the call in
        // `expr_ok_for_view_return`; index/slice are rejected by E2304 and
        // never reach codegen.)
        Expr::Field(..) => {
            format!("&{}", emit_expr(cx, e, env))
        }
        _ => emit_expr(cx, e, env),
    }
}

fn emit_if(
    cx: &Cx,
    ifs: &IfStmt,
    env: &mut HashMap<String, Slot>,
    out: &mut String,
    indent: usize,
    view_return: bool,
) {
    let pad = "    ".repeat(indent);
    if let Some((pat, subj)) = if_pattern_test(&ifs.cond) {
        let subj_expr = emit_expr(cx, subj, env);
        let pat_str = emit_if_let_pattern(cx, pat);
        out.push_str(&format!("{}if let {} = {} {{\n", pad, pat_str, subj_expr));
        let mut body_env = env.clone();
        let subj_ty = expr_jet_ty_with_cx(cx, subj, env);
        add_pattern_bindings(cx, pat, &mut body_env, subj_ty.as_ref());
        emit_stmts(
            cx,
            &ifs.then_body,
            &mut body_env,
            out,
            indent + 1,
            view_return,
        );
    } else if let Expr::PatternTest {
        subject,
        pattern: Pattern::Absent(_),
        ..
    } = &ifs.cond
    {
        let subj = emit_expr(cx, subject, env);
        out.push_str(&format!("{}if {}.is_none() {{\n", pad, subj));
        emit_stmts(cx, &ifs.then_body, env, out, indent + 1, view_return);
    } else {
        out.push_str(&format!("{}if {} {{\n", pad, emit_expr(cx, &ifs.cond, env)));
        emit_stmts(cx, &ifs.then_body, env, out, indent + 1, view_return);
    }
    match &ifs.else_branch {
        None => out.push_str(&format!("{}}}\n", pad)),
        Some(ElseBranch::Else(body)) => {
            out.push_str(&format!("{}}} else {{\n", pad));
            emit_stmts(cx, body, env, out, indent + 1, view_return);
            out.push_str(&format!("{}}}\n", pad));
        }
        Some(ElseBranch::ElseIf(next)) => {
            out.push_str(&format!("{}}} else ", pad));
            let mut nested = String::new();
            emit_if(cx, next, env, &mut nested, indent, view_return);
            let trimmed = nested.trim_start_matches(&pad).to_string();
            out.push_str(&trimmed);
        }
    }
}

fn if_pattern_test(cond: &Expr) -> Option<(&Pattern, &Expr)> {
    match cond {
        Expr::PatternTest {
            subject, pattern, ..
        } => match pattern {
            Pattern::Absent(_) => None,
            _ => Some((pattern, subject.as_ref())),
        },
        Expr::Binary(BinOp::And, l, r, _) => {
            if let Expr::PatternTest {
                subject, pattern, ..
            } = l.as_ref()
            {
                if matches!(pattern, Pattern::Absent(_)) {
                    return None;
                }
                if let Expr::PatternTest { .. } = r.as_ref() {
                    return None;
                }
                return Some((pattern, subject.as_ref()));
            }
            None
        }
        _ => None,
    }
}

/// The payload types a variant binds, so destructured names carry their type
/// into the body (needed so e.g. `.get` on a `Map` bound from `Object(root)`
/// lowers to a map lookup, not list indexing — B3). Mirrors sema's
/// `std_json_pattern_types` for the std `JSON` enum and reads `cx` for user
/// enums. Returns `None` when the types aren't known (binding stays untyped).
fn variant_binding_types(cx: &Cx, variant: &str) -> Option<Vec<Type>> {
    if is_json_variant(variant) {
        let json = Type::Named(Syntax::TYPE_JSON.to_string());
        return match variant {
            "Null" => Some(Vec::new()),
            "Boolean" => Some(vec![Type::Bool]),
            "Number" => Some(vec![Type::Float]),
            "Text" => Some(vec![Type::String]),
            "Array" => Some(vec![Type::List(Box::new(json))]),
            "Object" => Some(vec![Type::Map {
                key: Box::new(Type::String),
                value: Box::new(json),
            }]),
            _ => None,
        };
    }
    let owner = cx.variant_owner.get(variant)?;
    let variants = cx.enum_variants.get(owner)?;
    let (_, payload) = variants.iter().find(|(n, _)| n == variant)?;
    match payload {
        VariantPayload::Unit => Some(Vec::new()),
        VariantPayload::Single(t, _) => Some(vec![t.clone()]),
        VariantPayload::Named(fields) => {
            Some(fields.iter().map(|f| f.ty.clone()).collect())
        }
    }
}

fn add_pattern_bindings(
    cx: &Cx,
    pattern: &Pattern,
    env: &mut HashMap<String, Slot>,
    subject_ty: Option<&Type>,
) {
    match pattern {
        Pattern::Present { binding, .. } => {
            let inner_ty = match subject_ty {
                Some(Type::Option(inner)) => Some((**inner).clone()),
                _ => None,
            };
            env.insert(
                binding.clone(),
                Slot {
                    rust_name: mangle(binding),
                    deref: false,
                    jet_ty: inner_ty,
                },
            );
        }
        Pattern::Variant {
            variant, bindings, ..
        } => {
            let tys = variant_binding_types(cx, variant);
            for (i, b) in bindings.iter().enumerate() {
                let jet_ty = tys.as_ref().and_then(|ts| ts.get(i).cloned());
                env.insert(
                    b.clone(),
                    Slot {
                        rust_name: mangle(b),
                        deref: false,
                        jet_ty,
                    },
                );
            }
        }
        Pattern::Absent(_) => {}
        Pattern::Ok { binding, .. } => {
            let ok_ty = match subject_ty {
                Some(Type::Result { ok, .. }) => Some((**ok).clone()),
                _ => None,
            };
            env.insert(
                binding.clone(),
                Slot {
                    rust_name: mangle(binding),
                    deref: false,
                    jet_ty: ok_ty,
                },
            );
        }
        Pattern::Err { binding, .. } => {
            let err_ty = match subject_ty {
                Some(Type::Result { err, .. }) => Some((**err).clone()),
                _ => None,
            };
            env.insert(
                binding.clone(),
                Slot {
                    rust_name: mangle(binding),
                    deref: false,
                    jet_ty: err_ty,
                },
            );
        }
    }
}

pub(crate) fn emit_for_in(
    cx: &Cx,
    var: &str,
    var2: Option<&(String, Span)>,
    collection: &Expr,
    body: &[Stmt],
    env: &mut HashMap<String, Slot>,
    out: &mut String,
    indent: usize,
    view_return: bool,
    lbl: &str,
) {
    let pad = "    ".repeat(indent);
    let coll = emit_expr(cx, collection, env);
    if let Some((v2, _)) = var2 {
        out.push_str(&format!(
            "{}{}for (_jet_k, _jet_v) in ({coll}).iter() {{\n",
            pad, lbl
        ));
        out.push_str(&format!(
            "{}    let {} = _jet_k.clone();\n",
            pad,
            mangle(var)
        ));
        out.push_str(&format!(
            "{}    let {} = _jet_v.clone();\n",
            pad,
            mangle(v2)
        ));
    } else if let Expr::MethodCall {
        receiver, method, ..
    } = collection
    {
        if method == "chars" {
            let recv = emit_expr(cx, receiver, env);
            out.push_str(&format!(
                "{}{}for _jet_c in ({recv}).chars() {{\n    {}let {} = _jet_c;\n",
                pad,
                lbl,
                pad,
                mangle(var)
            ));
        } else if method == "lines"
            && matches!(expr_jet_ty(receiver, env), Some(Type::Named(n)) if n == "FileReader")
        {
            // E2-M7: streaming line iteration — `loop line in handle.lines()`.
            // BufRead::lines() on BufReader<File> is lazy and uses bounded memory.
            // Each line error is converted to a runtime panic naming the resource (E2502).
            let recv = emit_expr(cx, receiver, env);
            out.push_str(&format!(
                "{}{}for _jet_raw_line in std::io::BufRead::lines(&mut ({}).inner) {{\n",
                pad, lbl, recv
            ));
            out.push_str(&format!(
                "{}    let {} = _jet_raw_line.unwrap_or_else(|_e| {}jet_panic({:?}, {}, &_e.to_string()));\n",
                pad,
                mangle(var),
                cx.root_prefix,
                cx.file,
                0
            ));
        } else {
            out.push_str(&format!(
                "{}{}for _jet_item in ({coll}).iter().cloned() {{\n    {}let {} = _jet_item;\n",
                pad,
                lbl,
                pad,
                mangle(var)
            ));
        }
    } else {
        out.push_str(&format!(
            "{}{}for _jet_item in ({coll}).iter().cloned() {{\n    {}let {} = _jet_item;\n",
            pad,
            lbl,
            pad,
            mangle(var)
        ));
    }
    env.insert(
        var.to_string(),
        Slot {
            rust_name: mangle(var),
            deref: false,
            jet_ty: None,
        },
    );
    if let Some((v2, _)) = var2 {
        env.insert(
            v2.clone(),
            Slot {
                rust_name: mangle(v2),
                deref: false,
                jet_ty: None,
            },
        );
    }
    emit_stmts(cx, body, env, out, indent + 1, view_return);
    env.remove(var);
    if let Some((v2, _)) = var2 {
        env.remove(v2);
    }
    out.push_str(&format!("{}}}\n", pad));
}

fn emit_if_let_pattern(cx: &Cx, pattern: &Pattern) -> String {
    match pattern {
        Pattern::Variant {
            variant, bindings, ..
        } => {
            let prefix = enum_type_prefix(cx, variant);
            if bindings.is_empty() {
                format!("{}::{}", prefix, variant_rust_name(variant))
            } else if bindings.len() == 1 {
                format!(
                    "{}::{}({})",
                    prefix,
                    variant_rust_name(variant),
                    mangle(&bindings[0])
                )
            } else {
                let fields = bindings
                    .iter()
                    .map(|b| format!("{}: {}", mangle(b), mangle(b)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "{}::{} {{ {} }}",
                    prefix,
                    variant_rust_name(variant),
                    fields
                )
            }
        }
        Pattern::Present { binding, .. } => format!("Some({})", mangle(binding)),
        Pattern::Absent(_) => "None".to_string(),
        Pattern::Ok { binding, .. } => format!("Ok({})", mangle(binding)),
        Pattern::Err { binding, .. } => format!("Err({})", mangle(binding)),
    }
}

pub(crate) fn emit_named_fn_value(cx: &Cx, name: &str, ft: &Type) -> String {
    let rust_name = mangle(name);
    let Type::Fn { params, ret } = ft else {
        return rust_name;
    };
    let arg_decls: Vec<String> = params
        .iter()
        .enumerate()
        .map(|(i, p)| format!("__jet_a{}: {}", i, cx.rust_type(p)))
        .collect();
    let arg_calls: Vec<String> = (0..params.len()).map(|i| format!("__jet_a{i}")).collect();
    let _ = ret;
    format!(
        "Box::new(move |{}| {}({})) as {}",
        arg_decls.join(", "),
        rust_name,
        arg_calls.join(", "),
        cx.rust_type(ft)
    )
}

pub(crate) fn receiver_struct_type(receiver: &Expr, env: &HashMap<String, Slot>) -> Option<String> {
    match receiver {
        Expr::Ident(name, _) => env.get(name).and_then(|s| s.jet_ty.as_ref()).and_then(|t| {
            if let Type::Named(n) = t {
                Some(n.clone())
            } else {
                None
            }
        }),
        _ => None,
    }
}

pub(crate) fn place_of(env: &HashMap<String, Slot>, name: &str) -> String {
    match env.get(name) {
        Some(slot) if slot.deref => format!("(*{})", slot.rust_name),
        Some(slot) => slot.rust_name.clone(),
        None => mangle(name),
    }
}

