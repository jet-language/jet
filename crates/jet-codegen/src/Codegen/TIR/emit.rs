//! TIR emission: TIR -> Rust source (`emit_tir_*`).
//!
//! Split out of the original `TIR.rs` for maintainability; behavior unchanged.

use super::*;
use crate::AST::{AccessConvention, BinOp, Type, UnOp};

/// Emit a covered function from its TIR, reusing the same pure formatting helpers
/// as `emit_func` so the output is byte-identical to the AST path (golden parity).
/// The only difference is that every decision is *read off the TIR* rather than
/// recomputed — there is no `expr_jet_ty` / `operand_is_integer` call anywhere.
pub(crate) fn emit_tir_func(tir: &TFunc, cx: &Cx, out: &mut String) {
    match &tir.kind {
        TFuncKind::TopLevel => emit_tir_toplevel(tir, cx, out),
        TFuncKind::Method { self_conv } => emit_tir_method(tir, *self_conv, cx, out),
        TFuncKind::TraitMethod {
            is_unsafe,
            self_conv,
        } => emit_tir_trait_method(tir, *is_unsafe, *self_conv, cx, out),
        TFuncKind::Delegation {
            sig,
            fwd,
            has_return,
        } => emit_tir_delegation(tir, sig, fwd, *has_return, cx, out),
    }
}

/// A module-level free function: `pub fn name(params) -> ret { … }`.
/// Byte-identical to `emit_func`'s output.
pub(crate) fn emit_tir_toplevel(tir: &TFunc, cx: &Cx, out: &mut String) {
    let ret_clause = match &tir.ret {
        Some(t) => format!(" -> {}", rust_return_type(cx, t)),
        None => String::new(),
    };
    let params = tir
        .params
        .iter()
        .map(|(rust_name, ty, conv)| format!("{}: {}", rust_name, rust_param_type(cx, *conv, ty)))
        .collect::<Vec<_>>()
        .join(", ");
    let vis = if tir.is_main { "" } else { "pub " };
    // c109 Phase 18: an `#Unsafe fn` lowers to `unsafe fn` — the prefix sits right after
    // `vis`, exactly as `emit_func` (`{vis}{unsafe_kw}fn …`). I1: emitted ONLY when the
    // source was `#Unsafe fn` (`tir.is_unsafe`).
    let unsafe_kw = if tir.is_unsafe { "unsafe " } else { "" };
    // D-METHODMACRO1=A: `@Inline`/`@InlineAlways` lower to a Rust `#[inline]`/
    // `#[inline(always)]` attribute right above the signature. `is_inline_always`
    // is only ever `true` here once sema has confirmed the function can actually
    // inline (E0917/E0918/E0919 would have failed the build otherwise) — I3:
    // sema decides, codegen just emits.
    let inline_attr = if tir.is_inline_always {
        "#[inline(always)]\n"
    } else if tir.is_inline {
        "#[inline]\n"
    } else {
        ""
    };
    // E2-M12 D-OBS1: track the current function name for rich panic reports —
    // matches `emit_func` so panic output is identical.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    out.push_str(&format!(
        "{inline_attr}{vis}{unsafe_kw}fn {name}{gen}({params}){ret} {{\n",
        name = cx.mangle_name(&tir.name),
        gen = tir.generics,
        params = params,
        ret = ret_clause,
    ));
    // D-COV1: probe at the function head (skip the synthetic `main`).
    if cx.coverage && !tir.is_main {
        out.push_str(&format!("    jet_cov({});\n", tir.line));
    }
    if tir.is_reactive {
        emit_reactive_wrapped_body(&tir.body, cx, out, 1);
    } else if matches!(&tir.ret, Some(Type::Apply { name, .. }) if name == "Stream") {
        emit_generator_wrapped_body(&tir.body, cx, out, 1);
    } else {
        emit_tir_stmts(&tir.body, cx, out, 1);
    }
    if is_fallible_void_return(&tir.ret) {
        out.push_str("    Ok(())\n");
    }
    out.push_str("}\n\n");
}

fn is_fallible_void_return(ret: &Option<Type>) -> bool {
    matches!(
        ret,
        Some(Type::Result { ok, err })
            if matches!(ok.as_ref(), Type::Named(n) if n == crate::Syntax::TYPE_VOID)
                && matches!(err.as_ref(), Type::Named(n) if n == crate::Syntax::TYPE_ERROR)
    )
}

/// D-STREAMYIELD1: a generator (`-> Stream<T>`) spawns its body on its own
/// thread and hands the caller the channel receiver immediately — `yield`
/// (lowered to `__jet_yield_tx.send(...)`) blocks on the rendezvous channel
/// until the consumer's `loop x in stream { }` pulls the next value. No
/// coroutine/async machinery: a real OS thread IS the suspended generator.
fn emit_generator_wrapped_body(body: &[TStmt], cx: &Cx, out: &mut String, indent: usize) {
    let pad = "    ".repeat(indent);
    let inner = indent + 1;
    out.push_str(&format!(
        "{}let (__jet_yield_tx, __jet_yield_rx) = std::sync::mpsc::sync_channel(0);\n",
        pad
    ));
    out.push_str(&format!("{}std::thread::spawn(move || {{\n", pad));
    emit_tir_stmts(body, cx, out, inner);
    out.push_str(&format!("{}}});\n", pad));
    out.push_str(&format!("{}__jet_yield_rx\n", pad));
}

fn emit_reactive_wrapped_body(body: &[TStmt], cx: &Cx, out: &mut String, indent: usize) {
    let pad = "    ".repeat(indent);
    let inner = indent + 1;
    out.push_str(&format!(
        "{}{}jet_std::jet_reactive_effect({});\n",
        pad,
        cx.root_prefix,
        render_reactive_tir_closure(body, cx, inner)
    ));
}

fn render_reactive_tir_closure(body: &[TStmt], cx: &Cx, indent: usize) -> String {
    let mut inner = String::new();
    emit_tir_stmts(body, cx, &mut inner, indent);
    format!("move || {{ {} }}", inner)
}

/// c109 Phase 7: an inherent method, emitted INSIDE an `impl user_<T> { … }` block
/// (the caller `emit_type_impl` already opened it). Byte-identical to `emit_method`:
/// `    pub fn user_<name>(<self>, <params>) -> <ret> {\n … \n    }\n`. The `self`
/// receiver form comes from `self_conv` (`Read`→`&self`, `Mutate`→`&mut self`,
/// `Move`→`self`); a static method (`self_conv == None`) emits no receiver.
pub(crate) fn emit_tir_method(
    tir: &TFunc,
    self_conv: Option<AccessConvention>,
    cx: &Cx,
    out: &mut String,
) {
    let indent = 1;
    let pad = "    ".repeat(indent);
    let ret_clause = match &tir.ret {
        Some(t) => format!(" -> {}", rust_return_type(cx, t)),
        None => String::new(),
    };
    let mut params: Vec<String> = Vec::new();
    if let Some(conv) = self_conv {
        params.push(
            match conv {
                AccessConvention::Read | AccessConvention::Share | AccessConvention::Raw => "&self",
                AccessConvention::Write => "&mut self",
                AccessConvention::Move => "self",
            }
            .to_string(),
        );
    }
    for (rust_name, ty, conv) in &tir.params {
        params.push(format!("{}: {}", rust_name, rust_param_type(cx, *conv, ty)));
    }
    // c109 Phase 18: an `#Unsafe fn` inherent method lowers to `pub unsafe fn` — the
    // prefix sits between `pub ` and `fn`, exactly as `emit_method` (`pub {unsafe_kw}fn`).
    // I1: emitted ONLY for a source `#Unsafe fn` (`tir.is_unsafe`).
    let unsafe_kw = if tir.is_unsafe { "unsafe " } else { "" };
    // D-METHODMACRO1=A: `@Inline`/`@InlineAlways` on a method — same attribute,
    // indented to the method's own line (see `emit_tir_toplevel` for the free-
    // function form).
    let inline_attr = if tir.is_inline_always {
        format!("{pad}#[inline(always)]\n")
    } else if tir.is_inline {
        format!("{pad}#[inline]\n")
    } else {
        String::new()
    };
    // E2-M12 D-OBS1: track the current function name for rich panic reports.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    out.push_str(&format!(
        "{inline_attr}{pad}pub {unsafe_kw}fn {name}({params}){ret} {{\n",
        name = mangle(&tir.name),
        params = params.join(", "),
        ret = ret_clause,
    ));
    // D-COV1: probe at the method head.
    if cx.coverage {
        out.push_str(&format!("{pad}    jet_cov({});\n", tir.line));
    }
    if tir.is_reactive {
        emit_reactive_wrapped_body(&tir.body, cx, out, indent + 1);
    } else {
        emit_tir_stmts(&tir.body, cx, out, indent + 1);
    }
    out.push_str(&format!("{pad}}}\n"));
}

/// c109 Phase 12: a trait-impl method, emitted INSIDE an `impl Trait for user_<T> { … }`
/// block (the caller `emit_trait_impl`/`emit_external_trait_impl` opened it).
/// Byte-identical to `emit_trait_method` (Source/Codegen/Items.rs): a BARE method name
/// (no `user_` mangle — the trait owns it), NO `pub`, an always-`&self` receiver, and
/// an `unsafe ` prefix iff the source was an `#Unsafe fn`.
pub(crate) fn emit_tir_trait_method(
    tir: &TFunc,
    is_unsafe: bool,
    self_conv: AccessConvention,
    cx: &Cx,
    out: &mut String,
) {
    let indent = 1;
    let pad = "    ".repeat(indent);
    let ret_clause = match &tir.ret {
        // `emit_trait_method` computes `ret = rust_return_type(...)` then, if non-empty,
        // ` -> ret`. A unit return yields the empty clause.
        Some(t) => {
            let ret = rust_return_type(cx, t);
            if ret.is_empty() {
                String::new()
            } else {
                format!(" -> {}", ret)
            }
        }
        None => String::new(),
    };
    // D-MUTSELF1: the receiver honors the source convention — `&self` / `&mut self` /
    // `self` — matching `emit_trait_method` and the trait declaration (emit_trait_def).
    let self_recv = match self_conv {
        AccessConvention::Read | AccessConvention::Share | AccessConvention::Raw => "&self",
        AccessConvention::Write => "&mut self",
        AccessConvention::Move => "self",
    };
    let mut params: Vec<String> = vec![self_recv.to_string()];
    for (rust_name, ty, conv) in &tir.params {
        params.push(format!("{}: {}", rust_name, rust_param_type(cx, *conv, ty)));
    }
    let unsafe_kw = if is_unsafe { "unsafe " } else { "" };
    // E2-M12 D-OBS1: track the current function name for rich panic reports.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    out.push_str(&format!(
        "{pad}{unsafe_kw}fn {name}({params}){ret} {{\n",
        name = tir.name,
        params = params.join(", "),
        ret = ret_clause,
    ));
    // D-COV1: probe at the trait-method head.
    if cx.coverage {
        out.push_str(&format!("{pad}    jet_cov({});\n", tir.line));
    }
    emit_tir_stmts(&tir.body, cx, out, indent + 1);
    out.push_str(&format!("{pad}}}\n"));
}

/// c109 Phase 15: a DELEGATION trait method (`using field`), emitted INSIDE the
/// `impl Trait for user_<T> { … }` block `emit_external_trait_impl` opened. Byte-for-byte
/// `emit_delegation_method` (Source/Codegen/Items.rs): the pre-rendered signature line,
/// then the single forwarding call (`(self).<field>.<method>(args)`) at 8-space indent —
/// with a trailing `;` for a unit method, none for a returning one — then `    }`.
pub(crate) fn emit_tir_delegation(
    tir: &TFunc,
    sig: &str,
    fwd: &str,
    has_return: bool,
    cx: &Cx,
    out: &mut String,
) {
    // E2-M12 D-OBS1: track the current function name (parity with the AST path, though a
    // delegation body has no panic site of its own).
    *cx.current_fn.borrow_mut() = tir.name.clone();
    out.push_str(sig);
    if has_return {
        out.push_str(&format!("        {}\n", fwd));
    } else {
        out.push_str(&format!("        {};\n", fwd));
    }
    out.push_str("    }\n");
}

pub(crate) fn emit_tir_stmts(stmts: &[TStmt], cx: &Cx, out: &mut String, indent: usize) {
    for s in stmts {
        emit_tir_stmt(s, cx, out, indent);
    }
}

pub(crate) fn emit_tir_stmt(s: &TStmt, cx: &Cx, out: &mut String, indent: usize) {
    let pad = "    ".repeat(indent);
    match s {
        TStmt::Let {
            name,
            kw,
            ty_clause,
            init,
            track_origin,
        } => {
            out.push_str(&format!(
                "{}{} {}{} = {};\n",
                pad,
                kw,
                mangle(name),
                ty_clause,
                emit_tir_expr(init, cx),
            ));
            if let Some(origin) = track_origin {
                out.push_str(&format!(
                    "{}{}jet_track_float_origin(&{}, {:?});\n",
                    pad,
                    cx.root_prefix,
                    mangle(name),
                    origin
                ));
            }
        }
        TStmt::Assign {
            place,
            op,
            value,
            clone_value,
        } => {
            let v = emit_tir_expr(value, cx);
            // c150: append `.clone()` when the value is a borrowed non-scalar (computed
            // at lowering). `({v}).clone()` matches how other clone sites in the AST
            // path parenthesise the receiver before the method call.
            let v = if *clone_value {
                format!("({}).clone()", v)
            } else {
                v
            };
            match op {
                Some(op) => out.push_str(&format!("{}{} {}= {};\n", pad, place, op.spell(), v)),
                None => out.push_str(&format!("{}{} = {};\n", pad, place, v)),
            }
        }
        // c109 Phase 23: tuple destructure. Mirrors `emit_stmt`'s `BindPattern::Tuple`
        // arm byte-for-byte: borrow the init into a temp, then bind each element from a
        // cloned canonical field of it.
        TStmt::TupleDestructure {
            tmp,
            init,
            kw,
            binds,
        } => {
            out.push_str(&format!(
                "{}let {} = &({});\n",
                pad,
                tmp,
                emit_tir_expr(init, cx)
            ));
            for (elem_rust, field_rust) in binds {
                out.push_str(&format!(
                    "{}{} {} = ({}).{}.clone();\n",
                    pad, kw, elem_rust, tmp, field_rust
                ));
            }
        }
        // c109 / D-DESTRUCT1: struct destructure. Mirrors `emit_stmt`'s
        // `BindPattern::Struct` arm byte-for-byte: borrow the init into a temp,
        // then bind each local from a cloned field of it. `local_rust`/
        // `field_rust` diverge for a renamed field (`severity: sev`).
        TStmt::StructDestructure {
            tmp,
            init,
            kw,
            binds,
        } => {
            out.push_str(&format!(
                "{}let {} = &({});\n",
                pad,
                tmp,
                emit_tir_expr(init, cx)
            ));
            for (local_rust, field_rust) in binds {
                out.push_str(&format!(
                    "{}{} {} = ({}).{}.clone();\n",
                    pad, kw, local_rust, tmp, field_rust
                ));
            }
        }
        // c109 Phase 26: list destructure. Mirrors `emit_stmt`'s `BindPattern::List`
        // arm byte-for-byte: borrow the init into a temp, then bind each element via
        // the runtime bounds-checked `jet_unpack_vec(tmp, want, i, file, line)` move.
        // `{file:?}` reproduces the AST's debug-formatted path; `kw`/`want`/`line` were
        // all resolved at lowering.
        TStmt::ListDestructure {
            tmp,
            init,
            kw,
            want,
            file,
            line,
            elems,
        } => {
            out.push_str(&format!(
                "{}let {} = &({});\n",
                pad,
                tmp,
                emit_tir_expr(init, cx)
            ));
            for (i, elem_rust) in elems.iter().enumerate() {
                out.push_str(&format!(
                    "{}{} {} = jet_unpack_vec({}, {}, {}, {:?}, {});\n",
                    pad, kw, elem_rust, tmp, want, i, file, line
                ));
            }
        }
        TStmt::Return(Some(e)) => {
            out.push_str(&format!("{}return {};\n", pad, emit_tir_expr(e, cx)));
        }
        TStmt::Return(None) => {
            out.push_str(&format!("{}return;\n", pad));
        }
        TStmt::ExprStmt(e) => {
            out.push_str(&format!("{}{};\n", pad, emit_tir_expr(e, cx)));
        }
        TStmt::If {
            cond,
            then_body,
            else_body,
            else_is_elseif,
        } => {
            // c109 Phase 22: render the head per the condition form, byte-for-byte
            // `emit_if` (Source/Codegen/Statement.rs).
            match cond {
                TIfCond::Plain(c) => {
                    out.push_str(&format!("{}if {} {{\n", pad, emit_tir_expr(c, cx)));
                }
                TIfCond::IfLet { pat_str, subj } => {
                    out.push_str(&format!(
                        "{}if let {} = {} {{\n",
                        pad,
                        pat_str,
                        emit_tir_expr(subj, cx)
                    ));
                }
                TIfCond::IsNone { subj } => {
                    out.push_str(&format!(
                        "{}if {}.is_none() {{\n",
                        pad,
                        emit_tir_expr(subj, cx)
                    ));
                }
                TIfCond::Matches { pat_str, subj } => {
                    out.push_str(&format!(
                        "{}if matches!(&({}), {}) {{\n",
                        pad,
                        emit_tir_expr(subj, cx),
                        pat_str
                    ));
                }
            }
            emit_tir_stmts(then_body, cx, out, indent + 1);
            match else_body {
                None => out.push_str(&format!("{}}}\n", pad)),
                Some(body) => {
                    // Match the AST path EXACTLY: it renders `} else if …` ONLY for a
                    // real `else if` chain (`ElseBranch::ElseIf` → `else_is_elseif`), and
                    // `} else { … }` for an explicit `else` block — even when the block
                    // holds a single `if` (do NOT flatten that, or parity drifts).
                    if *else_is_elseif {
                        out.push_str(&format!("{}}} else ", pad));
                        let mut nested = String::new();
                        emit_tir_stmt(&body[0], cx, &mut nested, indent);
                        out.push_str(nested.trim_start_matches(&pad as &str));
                    } else {
                        out.push_str(&format!("{}}} else {{\n", pad));
                        emit_tir_stmts(body, cx, out, indent + 1);
                        out.push_str(&format!("{}}}\n", pad));
                    }
                }
            }
        }
        // c109 Phase 2: control-flow loops. Each mirrors the AST emit path
        // (Statement.rs) byte-for-byte; all decisions are read off the TIR.
        TStmt::Loop { label, body } => {
            out.push_str(&format!("{}{}loop {{\n", pad, tir_label_prefix(label)));
            emit_tir_stmts(body, cx, out, indent + 1);
            out.push_str(&format!("{}}}\n", pad));
        }
        TStmt::While { label, cond, body } => {
            out.push_str(&format!(
                "{}{}while {} {{\n",
                pad,
                tir_label_prefix(label),
                emit_tir_expr(cond, cx)
            ));
            emit_tir_stmts(body, cx, out, indent + 1);
            out.push_str(&format!("{}}}\n", pad));
        }
        // D-LOOP-SEMICOLON1=A: `loop init; cond; step { body }` → scoped Rust block.
        TStmt::CountedLoop {
            label,
            init,
            cond,
            step,
            body,
        } => {
            // Outer scoping block to contain the init variable.
            out.push_str(&format!("{}{{\n", pad));
            emit_tir_stmt(init, cx, out, indent + 1);
            let inner_pad = "    ".repeat(indent + 1);
            out.push_str(&format!(
                "{}{}loop {{\n",
                inner_pad,
                tir_label_prefix(label),
            ));
            let body_pad = "    ".repeat(indent + 2);
            out.push_str(&format!(
                "{}if !({}) {{ break; }}\n",
                body_pad,
                emit_tir_expr(cond, cx)
            ));
            emit_tir_stmts(body, cx, out, indent + 2);
            emit_tir_stmt(step, cx, out, indent + 2);
            out.push_str(&format!("{}}}\n", inner_pad));
            out.push_str(&format!("{}}}\n", pad));
        }
        TStmt::Range {
            label,
            var,
            start,
            end,
            step,
            body,
        } => {
            let lbl = tir_label_prefix(label);
            let s = emit_tir_expr(start, cx);
            let e = emit_tir_expr(end, cx);
            // S22 (D-SG8): `..` is inclusive → `..=`; `step` becomes `.step_by`.
            match step {
                Some(step) => {
                    let st = emit_tir_expr(step, cx);
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
            emit_tir_stmts(body, cx, out, indent + 1);
            out.push_str(&format!("{}}}\n", pad));
        }
        TStmt::Break(label) => match label {
            Some(name) => out.push_str(&format!("{}break 'jet_{};\n", pad, name)),
            None => out.push_str(&format!("{}break;\n", pad)),
        },
        TStmt::Continue(label) => match label {
            Some(name) => out.push_str(&format!("{}continue 'jet_{};\n", pad, name)),
            None => out.push_str(&format!("{}continue;\n", pad)),
        },
        // c109 Phase 4: an exhaustive enum match. Mirrors `emit_pattern_match_switch`
        // (Statement.rs) byte-for-byte; every pattern/guard string was resolved at
        // lowering. Arm bodies emit at indent+2.
        TStmt::EnumMatch {
            scrutinee,
            arms,
            else_body,
            fallthrough,
        } => {
            out.push_str(&format!("{}match {} {{\n", pad, scrutinee));
            for arm in arms {
                match &arm.guard {
                    Some(guard) => {
                        out.push_str(&format!("{}    {} if {} => {{\n", pad, arm.pattern, guard))
                    }
                    None => out.push_str(&format!("{}    {} => {{\n", pad, arm.pattern)),
                }
                emit_tir_stmts(&arm.body, cx, out, indent + 2);
                out.push_str(&format!("{}    }}\n", pad));
            }
            match else_body {
                Some(body) => {
                    out.push_str(&format!("{}    _ => {{\n", pad));
                    emit_tir_stmts(body, cx, out, indent + 2);
                    out.push_str(&format!("{}    }}\n", pad));
                }
                None if *fallthrough => {
                    // Sema proved exhaustiveness (E0307); this dead arm exists only
                    // so rustc sees a complete match (I2/I3).
                    out.push_str(&format!(
                        "{}    _ => unreachable!(\"jet: exhaustiveness bug\"),\n",
                        pad
                    ));
                }
                None => {}
            }
            out.push_str(&format!("{}}}\n", pad));
        }
        // c109 Phase 4: an all-range scalar switch. Mirrors `emit_mixed_switch`
        // (Statement.rs): a wrapping block binds `_jet_switch_subject` (unused here,
        // emitted for parity), then an `if/else if … else` chain of range tests.
        TStmt::RangeSwitch {
            subject_str,
            arms,
            else_body,
        } => {
            out.push_str(&format!("{}{{\n", pad));
            let inner_pad = "    ".repeat(indent + 1);
            out.push_str(&format!(
                "{}let _jet_switch_subject = &({});\n",
                inner_pad, subject_str
            ));
            for (i, (lo, hi, body)) in arms.iter().enumerate() {
                let kw = if i == 0 { "if" } else { "} else if" };
                out.push_str(&format!(
                    "{}{} ({} >= {} && {} <= {}) {{\n",
                    inner_pad, kw, subject_str, lo, subject_str, hi
                ));
                emit_tir_stmts(body, cx, out, indent + 2);
            }
            out.push_str(&format!("{}}} else {{\n", inner_pad));
            emit_tir_stmts(else_body, cx, out, indent + 2);
            out.push_str(&format!("{}}}\n", inner_pad));
            out.push_str(&format!("{}}}\n", pad));
        }
        // c109 Phase 5: indexed assignment `coll[i] = v`. Mirrors the AST
        // `LValue::Index` form byte-for-byte: a map insert clones the key; a vec
        // assign casts the index to `usize`. Both wrap the value in a block.
        TStmt::IndexAssign {
            base,
            index,
            is_map,
            value,
        } => {
            let b = emit_tir_expr(base, cx);
            let i = emit_tir_expr(index, cx);
            let v = emit_tir_expr(value, cx);
            if *is_map {
                out.push_str(&format!(
                    "{pad}{{ let __jet_v = {v}; jet_map_insert(&mut ({b}), ({i}).clone(), __jet_v); }}\n",
                ));
            } else {
                out.push_str(&format!(
                    "{pad}{{ let __jet_v = {v}; ({b})[{i} as usize] = __jet_v; }}\n",
                ));
            }
        }
        TStmt::IndexHookAssign {
            type_name,
            base,
            index,
            value,
        } => {
            let ty = super::user_type_rust(type_name);
            let b = emit_tir_expr(base, cx);
            let i = emit_tir_expr(index, cx);
            let v = emit_tir_expr(value, cx);
            out.push_str(&format!(
                "{pad}{{ let __jet_v = {v}; <{ty} as user_IndexMut>::set(&mut ({b}), {i}, __jet_v); }}\n",
            ));
        }
        // D-SWIZZLE1: write swizzle — ordered lane stores into the backing array.
        TStmt::MathSwizzleAssign {
            base,
            type_name,
            lanes,
            value,
            clone_value,
        } => {
            let b = emit_tir_expr(base, cx);
            let mut v = emit_tir_expr(value, cx);
            if *clone_value {
                v = format!("({v}).clone()");
            }
            out.push_str(&format!(
                "{pad}{}\n",
                emit_math_swizzle_assign_stmt(&b, type_name, lanes, &v)
            ));
        }
        // c109 Phase 5: collection iteration. Mirrors `emit_for_in` for the two
        // plain `.iter()` shapes (method-call collections are excluded by the gate):
        //   single: `for _jet_item in (coll).iter().cloned() { let var = _jet_item; … }`
        //   map k,v: `for (_jet_k, _jet_v) in (coll).iter() { let k = _jet_k.clone();
        //             let v = _jet_v.clone(); … }`
        TStmt::ForIn {
            label,
            var,
            var2,
            collection_str,
            method_kind,
            columnar,
            by_value,
            body,
        } => {
            let lbl = tir_label_prefix(label);
            // c109 Phase 22: a method-call collection takes a distinct `emit_for_in`
            // branch (`collection_str` holds the RECEIVER for chars/lines). Only the
            // stdin form opens an extra block that needs an extra closing brace.
            let mut needs_extra_close = false;
            match method_kind {
                Some(TForInMethod::Chars) => {
                    out.push_str(&format!(
                        "{}{}for _jet_c in ({recv}).chars() {{\n    {}let {} = _jet_c;\n",
                        pad,
                        lbl,
                        pad,
                        mangle(var),
                        recv = collection_str
                    ));
                }
                Some(TForInMethod::LinesFile) => {
                    out.push_str(&format!(
                        "{}{}for _jet_raw_line in std::io::BufRead::lines(&mut ({}).inner) {{\n",
                        pad, lbl, collection_str
                    ));
                    out.push_str(&format!(
                        "{}    let {} = _jet_raw_line.unwrap_or_else(|_e| {}jet_panic({:?}, {}, &_e.to_string()));\n",
                        pad,
                        mangle(var),
                        cx.root_prefix,
                        cx.file,
                        0
                    ));
                }
                Some(TForInMethod::LinesStdin) => {
                    out.push_str(&format!(
                        "{}{{ let mut _jet_stdin_h = {};\n",
                        pad, collection_str
                    ));
                    needs_extra_close = true;
                    out.push_str(&format!(
                        "{}{}for _jet_raw_line in std::io::BufRead::lines(&mut _jet_stdin_h.inner) {{\n",
                        pad, lbl
                    ));
                    out.push_str(&format!(
                        "{}    let {} = _jet_raw_line.unwrap_or_else(|_e| {}jet_panic({:?}, {}, &_e.to_string()));\n",
                        pad,
                        mangle(var),
                        cx.root_prefix,
                        cx.file,
                        0
                    ));
                }
                Some(TForInMethod::Iterable {
                    coll_type,
                    iter_type,
                }) => {
                    let coll_rust = super::user_type_rust(coll_type);
                    let iter_rust = super::user_type_rust(iter_type);
                    out.push_str(&format!(
                        "{}{}{{ let mut _jet_it = <{coll_rust} as user_Iterable>::iter(({collection_str}));\n",
                        pad, lbl,
                    ));
                    out.push_str(&format!(
                        "{}    while let Some(_jet_item) = <{iter_rust} as user_Iterator>::next(&mut _jet_it) {{\n",
                        pad,
                    ));
                    out.push_str(&format!(
                        "{}        let {} = _jet_item;\n",
                        pad,
                        mangle(var)
                    ));
                    needs_extra_close = true;
                }
                None => match var2 {
                    Some(v2) => {
                        out.push_str(&format!(
                            "{}{}for (_jet_k, _jet_v) in ({}).iter() {{\n",
                            pad, lbl, collection_str
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
                    }
                    None => {
                        // D-SOA1: a columnar list iterates `iter_aos()` (owned S, no
                        // `.cloned()`); a plain list iterates `iter().cloned()`.
                        // D-STREAMYIELD1: a `Stream<T>` (`Receiver<T>`) iterates BY
                        // VALUE directly — it already yields owned `T`, no `.iter()`.
                        let iter_form = if *by_value {
                            format!("({})", collection_str)
                        } else if *columnar {
                            format!("({}).iter_aos()", collection_str)
                        } else {
                            format!("({}).iter().cloned()", collection_str)
                        };
                        out.push_str(&format!(
                            "{}{}for _jet_item in {} {{\n    {}let {} = _jet_item;\n",
                            pad,
                            lbl,
                            iter_form,
                            pad,
                            mangle(var)
                        ));
                    }
                },
            }
            emit_tir_stmts(body, cx, out, indent + 1);
            out.push_str(&format!("{}}}\n", pad));
            // D-STDIN1=A: close the outer block holding the JetStdinReader local.
            if needs_extra_close {
                out.push_str(&format!("{}}}\n", pad));
            }
        }
        // c109 Phase 15: a resolved comptime-if — emit ONLY the selected branch's
        // statements INLINE at the SAME indent, with no wrapper (no `if`, no block),
        // exactly as the AST `emit_stmts` does for `Stmt::ComptimeIf`.
        TStmt::Inline(stmts) => {
            emit_tir_stmts(stmts, cx, out, indent);
        }
        // c109 Phase 18: an audited `#Unsafe { … }` region — `unsafe { … }`, byte-for-byte
        // `emit_stmts`'s `Stmt::Unsafe` arm (the `#Audit` annotation emits nothing). I1:
        // emitted ONLY for a source `#Unsafe` gate.
        TStmt::Unsafe(body) => {
            out.push_str(&format!("{}unsafe {{\n", pad));
            emit_tir_stmts(body, cx, out, indent + 1);
            out.push_str(&format!("{}}}\n", pad));
        }
        // c109 Phase 19: an explicit `region r { … }` — a plain Rust block, byte-for-byte
        // `emit_stmts`'s `Stmt::Region` arm. The escape/RAII rules are sema's job (I3).
        // D-TERM1 (ratified 2026-06-22): `live { … }` block — enter terminal mode,
        // install a scope guard that restores it, then emit the body. The guard fires
        // on every exit path (normal, return, ?, panic unwind) via Rust's Drop ordering.
        TStmt::Live { body } => {
            let inner = indent + 1;
            let inner_pad = "    ".repeat(inner);
            out.push_str(&format!("{}{{\n", pad));
            out.push_str(&format!(
                "{}{}jet_term_enter();\n",
                inner_pad, cx.root_prefix
            ));
            out.push_str(&format!(
                "{}let _live_guard = {}jet_scope_guard(|| {{ {}jet_term_leave(); }});\n",
                inner_pad, cx.root_prefix, cx.root_prefix
            ));
            emit_tir_stmts(body, cx, out, inner);
            out.push_str(&format!("{}}}\n", pad));
        }
        // D-DOTSCOPE1: a `#Test` scope member, emitted inside `fn jet_test_N() ->
        // Result<(), String>`. Kind decides the framing; a `require` failure or
        // panic inside a region returns `Err`/unwinds, which each kind handles.
        TStmt::ScopeMember { kind, body } => {
            let inner = indent + 1;
            let ip = "    ".repeat(inner);
            let ip2 = "    ".repeat(inner + 1);
            match kind {
                // `.setup` — no new scope: statements splice inline so bindings
                // stay visible to the rest of the test. Runs first (sema pins it
                // to statement one); a failure returns `Err` like any statement.
                ScopeMemberKind::Setup => {
                    emit_tir_stmts(body, cx, out, indent);
                }
                // `.expect_fail` — the region MUST fail. Run it under a panic
                // boundary with a silenced hook; if it completes cleanly the test
                // fails. A failure lets execution continue past the region.
                ScopeMemberKind::ExpectFail => {
                    out.push_str(&format!("{}{{\n", pad));
                    out.push_str(&format!(
                        "{}let __prev_hook = std::panic::take_hook();\n",
                        ip
                    ));
                    out.push_str(&format!(
                        "{}std::panic::set_hook(Box::new(|_| {{}}));\n",
                        ip
                    ));
                    out.push_str(&format!(
                        "{}let __ef = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {{\n",
                        ip
                    ));
                    emit_tir_stmts(body, cx, out, inner + 1);
                    out.push_str(&format!("{}Ok(())\n", ip2));
                    out.push_str(&format!("{}}}));\n", ip));
                    out.push_str(&format!("{}std::panic::set_hook(__prev_hook);\n", ip));
                    out.push_str(&format!("{}if matches!(__ef, Ok(Ok(()))) {{\n", ip));
                    out.push_str(&format!(
                        "{}return Err(\"expected this region to fail, but it passed\".to_string());\n",
                        ip2
                    ));
                    out.push_str(&format!("{}}}\n", ip));
                    out.push_str(&format!("{}}}\n", pad));
                }
                // `.timeout(dur)` — v1 post-hoc: run to completion, then compare
                // elapsed against the budget. Does not interrupt a hang.
                ScopeMemberKind::Timeout(nanos) => {
                    out.push_str(&format!("{}{{\n", pad));
                    out.push_str(&format!("{}let __start = std::time::Instant::now();\n", ip));
                    emit_tir_stmts(body, cx, out, inner);
                    out.push_str(&format!("{}let __elapsed = __start.elapsed();\n", ip));
                    out.push_str(&format!(
                        "{}let __budget = std::time::Duration::from_nanos({});\n",
                        ip, nanos
                    ));
                    out.push_str(&format!("{}if __elapsed > __budget {{\n", ip));
                    out.push_str(&format!(
                        "{}return Err(format!(\"timeout: region took {{:?}}, over the {{:?}} budget\", __elapsed, __budget));\n",
                        ip2
                    ));
                    out.push_str(&format!("{}}}\n", ip));
                    out.push_str(&format!("{}}}\n", pad));
                }
                // `.skip` (region form) — not executed. `if false` keeps the body
                // type-checked but dead; whole-test skip is the harness's job.
                ScopeMemberKind::Skip => {
                    out.push_str(&format!("{}if false {{\n", pad));
                    emit_tir_stmts(body, cx, out, inner);
                    out.push_str(&format!("{}}}\n", pad));
                }
            }
        }
        // D-REACTCORE1: `#Reactive { … }` — register a reactive effect at this point.
        TStmt::Reactive { closure } => {
            out.push_str(&format!(
                "{}{}jet_std::jet_reactive_effect({});\n",
                pad, cx.root_prefix, closure
            ));
        }
        TStmt::Region(body) => {
            out.push_str(&format!("{}{{\n", pad));
            emit_tir_stmts(body, cx, out, indent + 1);
            out.push_str(&format!("{}}}\n", pad));
        }
        // D-LAYOUT1 / D-LAYOUT-GATES1: `layout NAME { … }`. NOT wrapped in a
        // nested Rust block — `name` must stay a live Rust local for
        // statements AFTER this one (`NAME.value(v)`, `NAME.suggest(…)`),
        // unlike `Region`/taskgroup, which are genuinely lexical.
        TStmt::Layout {
            rust_place,
            label,
            body,
        } => {
            out.push_str(&format!(
                "{}let {} = jet_layout::Handle::new({:?});\n",
                pad, rust_place, label
            ));
            emit_tir_stmts(body, cx, out, indent);
        }
        // D-TXN1–D-TXN4 (ratified 2026-06-24): `#Transact(name) { … }` block — open a
        // transaction guard, emit the body, then `commit()` on the clean fall-through
        // path. An early `?`/`return` skips `commit()`, so registered `on_commit` hooks
        // drop un-run (D-TXN3). Codegen is dumb (I3): no effect/rollback machinery here.
        TStmt::Transact {
            handle,
            snapshots,
            body,
        } => {
            let inner = indent + 1;
            let inner_pad = "    ".repeat(inner);
            out.push_str(&format!("{}{{\n", pad));
            // A named handle uses its mangled name; a bare block with auto-snapshots
            // needs a synthesized handle to register the snapshot restores on. A bare
            // block with neither handle nor snapshots erases to a plain block (its only
            // job was the D-TXN2 ceiling).
            let effective_handle: Option<String> = match handle {
                Some(h) => Some(h.clone()),
                None if !snapshots.is_empty() => Some("__jet_txn".to_string()),
                None => None,
            };
            match &effective_handle {
                Some(handle) => {
                    out.push_str(&format!(
                        "{}let mut {} = {}jet_transaction();\n",
                        inner_pad, handle, cx.root_prefix
                    ));
                    // D-TXN-ROLLBACK layer 1+2: snapshot each mutated root BEFORE
                    // the body runs. Clone-based (None) or custom Rollback (Some).
                    for (place_ref, rollback_ty) in snapshots {
                        match rollback_ty {
                            None => {
                                out.push_str(&format!(
                                    "{}{}jet_txn::snapshot(&mut {}, {});\n",
                                    inner_pad, cx.root_prefix, handle, place_ref
                                ));
                            }
                            Some(ty) => {
                                let bare = place_ref.trim_start_matches("&mut ").to_string();
                                out.push_str(&format!(
                                    "{}{{ let __snap = ({}).snapshot(); {}jet_txn::snapshot_custom(&mut {}, {}, __snap, {}::restore); }}\n",
                                    inner_pad, bare, cx.root_prefix, handle, place_ref, ty
                                ));
                            }
                        }
                    }
                    emit_tir_stmts(body, cx, out, inner);
                    out.push_str(&format!("{}{}.commit();\n", inner_pad, handle));
                }
                None => emit_tir_stmts(body, cx, out, inner),
            }
            out.push_str(&format!("{}}}\n", pad));
        }
        // c109 Phase 19: a `#Context(field: value) { … }` block — a plain block with one
        // RAII/no-op guard per field (declaration order) BEFORE the body, byte-for-byte
        // `emit_stmts`'s `Stmt::ContextBlock` arm.
        TStmt::ContextBlock { guards, body } => {
            out.push_str(&format!("{}{{\n", pad));
            let inner = indent + 1;
            let inner_pad = "    ".repeat(inner);
            for (i, (field_name, value)) in guards.iter().enumerate() {
                let val = emit_tir_expr(value, cx);
                match field_name.as_str() {
                    crate::Syntax::CTX_FIELD_ALLOCATOR => {
                        out.push_str(&format!(
                            "{}let _ctx_guard_{} = jet_mem::jet_ctx_push_alloc(&{});\n",
                            inner_pad, i, val
                        ));
                    }
                    crate::Syntax::CTX_FIELD_DEADLINE => {
                        out.push_str(&format!(
                            "{}let _ctx_deadline_{} = jet_ctx_push_deadline({});\n",
                            inner_pad, i, val
                        ));
                    }
                    _ => {
                        out.push_str(&format!("{}let _ctx_logger_{} = {};\n", inner_pad, i, val));
                    }
                }
            }
            emit_tir_stmts(body, cx, out, inner);
            out.push_str(&format!("{}}}\n", pad));
        }
        // c109 Phase 15: a mixed comparison/Bool switch — the general `emit_mixed_switch`
        // (Statement.rs) `if/else if … else` chain inside a block that binds
        // `_jet_switch_subject = &(subject)` (emitted for parity even when unused).
        TStmt::MixedSwitch {
            subject_str,
            arms,
            else_body,
        } => {
            out.push_str(&format!("{}{{\n", pad));
            let inner_pad = "    ".repeat(indent + 1);
            out.push_str(&format!(
                "{}let _jet_switch_subject = &({});\n",
                inner_pad, subject_str
            ));
            for (i, (cond, body)) in arms.iter().enumerate() {
                let kw = if i == 0 { "if" } else { "} else if" };
                out.push_str(&format!(
                    "{}{} {} {{\n",
                    inner_pad,
                    kw,
                    emit_tir_expr(cond, cx)
                ));
                emit_tir_stmts(body, cx, out, indent + 2);
            }
            // The `else`/fallthrough, byte-for-byte `emit_mixed_switch`: with arms and no
            // else → close the chain (`}`); with an else → `} else { … }`. (An empty
            // arm list is not reachable here — the gate requires at least one arm.)
            match else_body {
                None if !arms.is_empty() => {
                    out.push_str(&format!("{}}}\n", inner_pad));
                }
                None => {}
                Some(body) if arms.is_empty() => {
                    emit_tir_stmts(body, cx, out, indent + 1);
                }
                Some(body) => {
                    out.push_str(&format!("{}}} else {{\n", inner_pad));
                    emit_tir_stmts(body, cx, out, indent + 2);
                    out.push_str(&format!("{}}}\n", inner_pad));
                }
            }
            out.push_str(&format!("{}}}\n", pad));
        }
        // D-DBG3 step 2 (dap-debugger): a source line marker (only present when
        // `cx.debug_linemap` is set — see `TStmt::LineMarker`'s doc). A bare comment,
        // never affects the Rust it precedes; the native backend's line-table loader
        // reads the rust-line this comment sits on and pairs it with `n`.
        TStmt::LineMarker(n) => {
            out.push_str(&format!("{}// jet:line {}\n", pad, n));
        }
    }
}

/// Mirror `loop_label_prefix` (Codegen/Utils.rs) for a resolved label name:
/// `'jet_<name>: ` or empty. Kept here so the TIR emitter never reaches back
/// into the AST-side helper with an `Option<(String, Span)>`.
pub(crate) fn tir_label_prefix(label: &Option<String>) -> String {
    match label {
        Some(n) => format!("'jet_{}: ", n),
        None => String::new(),
    }
}

/// c109 Phase 16: emit one enum-literal payload arg, applying its resolved
/// `clone`/`boxed` wrappers — `(…).clone()` first, then `Box::new(…)`, exactly as
/// `emit_boxed_enum_arg` (Expression.rs) does.
pub(crate) fn emit_tir_enum_arg(a: &TEnumArg, cx: &Cx) -> String {
    let mut s = emit_tir_expr(&a.value, cx);
    if a.clone {
        s = format!("({}).clone()", s);
    }
    if a.boxed {
        s = format!("Box::new({})", s);
    }
    s
}

pub(crate) fn emit_tir_expr(e: &TExpr, cx: &Cx) -> String {
    match &e.kind {
        // D-SG9: width suffix is read straight off the literal — no re-inference.
        TExprKind::IntLit(n, width) => match width {
            Some((signed, bits)) => format!("{}{}{}", n, if *signed { 'i' } else { 'u' }, bits),
            None => format!("{}i64", n),
        },
        // D-FLOATW1: emit `f32` suffix when the sema-resolved width is F32.
        TExprKind::FloatLit(v) => {
            if matches!(&e.ty, Type::Float32) {
                format!("{:?}f32", v)
            } else {
                format!("{:?}f64", v)
            }
        }
        TExprKind::BoolLit(b) => b.to_string(),
        TExprKind::CharLit(c) => format!("{:?}", c),
        TExprKind::StrLit(parts) => emit_tir_str(parts, cx),
        TExprKind::Local(place) => place.clone(),
        // D-TAG1: binding-free enum variant/group pattern test.
        TExprKind::PatternMatches { subj, pat_str } => {
            format!("matches!(&({}), {})", emit_tir_expr(subj, cx), pat_str)
        }
        // c109 Phase 24: a comptime const inlined verbatim (the pre-rendered value).
        TExprKind::ConstInline(val) => val.clone(),
        TExprKind::Print(arg) => {
            format!(
                "println!(\"{{}}\", ({}).jet_show())",
                emit_tir_expr(arg, cx)
            )
        }
        // D-LIN1-DROP: `drop(x)` → Rust's safe `drop(x)`; the value moves in and
        // its `Drop` runs. No `unsafe` — the audit is sema-side (the `#Unsafe`
        // gate). The arg was lowered as a plain place/value (a move).
        TExprKind::Drop(arg) => {
            format!("drop({})", emit_tir_expr(arg, cx))
        }
        // c109 Phase 25: ambient prelude `input(...)`, byte-for-byte the `emit_call`
        // ambient-input branch (Source/Codegen/Expression.rs): a bare call with NO arg
        // emits `{root}jet_std_io_input(None)`; with a prompt arg `{root}jet_std_io_input(Some(&(arg)))`.
        TExprKind::AmbientInput { prompt } => {
            let helper = format!("{}jet_std_io_input", cx.root_prefix);
            match prompt {
                None => format!("{}(None)", helper),
                Some(p) => format!("{}(Some(&({})))", helper, emit_tir_expr(p, cx)),
            }
        }
        // c109 Phase 26: a `require`/`require_eq`/`panic` rich-report builtin. The whole
        // `{ … }` block emit string was rendered at lowering (byte-for-byte the AST
        // helper); emit reads it verbatim. As a statement-position call it is wrapped
        // `{ … };` by `TStmt::ExprStmt`, matching the AST `Stmt::Expr` `{ … };`.
        TExprKind::RequireStop(rendered) => rendered.clone(),
        TExprKind::Call { name, args } => {
            let arg_str = emit_tir_call_args(args, cx);
            format!("{}({})", cx.mangle_name(name), arg_str)
        }
        TExprKind::RangeCheckedCtor { name, arg } => {
            format!(
                "{}::try_new({})",
                cx.mangle_name(name),
                emit_tir_expr(arg, cx)
            )
        }
        // D-SIMD2 / D-LINALG1: a math constructor / static method → the prelude free
        // function `{root}jet_math_<T>_<func>(args)`. Args are plain values (floats or
        // a `[T#N]` array) — no borrow/clone decisions.
        TExprKind::MathBuiltin {
            type_name,
            func,
            args,
        } => {
            let parts: Vec<String> = args.iter().map(|a| emit_tir_expr(a, cx)).collect();
            format!(
                "{}jet_math_{}_{}({})",
                cx.root_prefix,
                type_name,
                func,
                parts.join(", ")
            )
        }
        TExprKind::PreciseBuiltin {
            type_name,
            func,
            args,
        } => {
            let parts: Vec<String> = args.iter().map(|a| emit_tir_expr(a, cx)).collect();
            let prefix = if type_name == "BigInt" {
                "jet_bigint"
            } else {
                "jet_decimal"
            };
            let call = if func == "from_str" {
                format!("{}{}_{}(&({}))", cx.root_prefix, prefix, func, parts[0])
            } else if func.starts_with("from_") {
                format!(
                    "{}{}_{}({})",
                    cx.root_prefix,
                    prefix,
                    func,
                    parts.join(", ")
                )
            } else if parts.len() == 1 {
                format!("{}{}_{}(&({}))", cx.root_prefix, prefix, func, parts[0])
            } else {
                format!(
                    "{}{}_{}(&({}), &({}))",
                    cx.root_prefix, prefix, func, parts[0], parts[1]
                )
            };
            call
        }
        // c109 Phase 6: the synthetic `.clone()`. Mirrors `emit_method_call`'s
        // `clone` early return: `(recv).clone()`, no deref/borrow decision (the
        // receiver was already lowered to the place the AST path would clone).
        TExprKind::Clone(recv) => {
            format!("({}).clone()", emit_tir_expr(recv, cx))
        }
        // D-MEM1 stage S5: `copy d` on a string-view local — `.to_string()`,
        // not `.clone()` (see the node's doc comment for why).
        TExprKind::MaterializeView(recv) => {
            format!("({}).to_string()", emit_tir_expr(recv, cx))
        }
        // c109 Phase 23: `.raw()` on a distinct type → `({recv}).0`. Mirrors
        // `emit_method_call`'s `METHOD_DISTINCT_RAW` early return byte-for-byte.
        TExprKind::DistinctRaw(recv) => {
            format!("({}).0", emit_tir_expr(recv, cx))
        }
        // c109 Phase 6: a user instance method call. Mirrors `emit_method_call`'s
        // final dispatch (`(recv).{method}({args})`): Rust's method autoref handles
        // the `&self`/`&mut self`/`self` receiver convention, so codegen emits the
        // receiver place as-is. The method name + arg wrappers were resolved at
        // lowering — emit only formats.
        TExprKind::MethodCall {
            recv,
            method_rust,
            args,
        } => {
            let arg_str = emit_tir_call_args(args, cx);
            format!("({}).{}({})", emit_tir_expr(recv, cx), method_rust, arg_str)
        }
        // c109 Phase 27: a call through a fn-typed struct field. Mirrors the AST
        // `emit_method_call` fn-field branch: `(({recv}).{field})({args})`.
        TExprKind::FnFieldCall {
            recv,
            field_rust,
            args,
        } => {
            let arg_str = emit_tir_call_args(args, cx);
            format!(
                "(({}).{})({})",
                emit_tir_expr(recv, cx),
                field_rust,
                arg_str
            )
        }
        // c109 Phase 7: a static method call. Mirrors the AST type-name dispatch:
        // `user_<Type>::user_<method>(args)`. All facts resolved at lowering.
        TExprKind::StaticCall {
            type_prefix,
            method_rust,
            args,
        } => {
            let arg_str = emit_tir_call_args(args, cx);
            format!("{}::{}({})", type_prefix, method_rust, arg_str)
        }
        // c109 Phase 9: a built-in collection/string method. The Map-vs-List-vs-String
        // branch was resolved into `op` at lowering; emit only formats, reproducing
        // `emit_builtin_method` (Source/Codegen/Expression.rs) byte-for-byte. Args are
        // emitted PLAINLY (no clone/borrow wrappers — `arg(i)` is a raw `emit_expr`).
        TExprKind::BuiltinMethod { recv, op, args } => {
            let recv = emit_tir_expr(recv, cx);
            let a = |i: usize| {
                args.get(i)
                    .map(|e| emit_tir_expr(e, cx))
                    .unwrap_or_default()
            };
            match op {
                TBuiltinOp::LenString => format!("jet_char_len(&({}))", recv),
                TBuiltinOp::LenList => format!("({}).len() as i64", recv),
                TBuiltinOp::IsEmpty => format!("({}).is_empty()", recv),
                TBuiltinOp::Push => format!("({}).push({})", recv, a(0)),
                TBuiltinOp::Pop => format!("({}).pop()", recv),
                TBuiltinOp::InsertMap => {
                    format!("({}).insert(({}).clone(), {})", recv, a(0), a(1))
                }
                TBuiltinOp::InsertList => {
                    format!("({}).insert({} as usize, {})", recv, a(0), a(1))
                }
                TBuiltinOp::RemoveMap => format!("({}).remove(&({}).clone())", recv, a(0)),
                TBuiltinOp::RemoveList { line } => format!(
                    "jet_list_remove(&mut ({}), {}, {:?}, {})",
                    recv,
                    a(0),
                    cx.file,
                    line
                ),
                TBuiltinOp::GetMap => format!("({}).get(&({}).clone()).cloned()", recv, a(0)),
                TBuiltinOp::GetList => format!("({}).get({} as usize).cloned()", recv, a(0)),
                TBuiltinOp::First => format!("({}).first().cloned()", recv),
                TBuiltinOp::Last => format!("({}).last().cloned()", recv),
                TBuiltinOp::Contains => format!("({}).contains(&{})", recv, a(0)),
                TBuiltinOp::IndexOf => format!(
                    "({}).iter().position(|x| *x == {}).map(|i| i as i64)",
                    recv,
                    a(0)
                ),
                TBuiltinOp::Reverse => format!("({}).reverse()", recv),
                TBuiltinOp::Sort => format!("({}).sort()", recv),
                TBuiltinOp::JoinSep => format!(
                    "({}).iter().map(|x| x.jet_show()).collect::<Vec<_>>().join(({}).as_str())",
                    recv,
                    a(0)
                ),
                TBuiltinOp::Sum => format!("jet_list_sum(({}).clone())", recv),
                TBuiltinOp::Product => format!("jet_list_product(({}).clone())", recv),
                TBuiltinOp::Min => format!("({}).iter().cloned().min()", recv),
                TBuiltinOp::Max => format!("({}).iter().cloned().max()", recv),
                TBuiltinOp::Flatten => format!("jet_list_flatten(({}).clone())", recv),
                TBuiltinOp::Intersperse => {
                    format!("jet_list_intersperse(({}).clone(), {})", recv, a(0))
                }
                TBuiltinOp::Unzip { tuple_struct } => format!(
                    "{{ let mut __a = Vec::new(); let mut __b = Vec::new(); for __x in ({}).clone() {{ __a.push(__x.user_a); __b.push(__x.user_b); }} {} {{ user_a: __a, user_b: __b }} }}",
                    recv, tuple_struct
                ),
                TBuiltinOp::Clear => format!("({}).clear()", recv),
                TBuiltinOp::Chars => format!("({}).chars().collect::<Vec<char>>()", recv),
                TBuiltinOp::Bytes => {
                    format!("{}jet_string_bytes(&({}))", cx.root_prefix, recv)
                }
                TBuiltinOp::Trim => format!("({}).trim().to_string()", recv),
                TBuiltinOp::Split => format!("jet_string_split(&({}), &{})", recv, a(0)),
                // c97/D-STRPARSE1: `lines()` → `jet_string_lines` (imported via MOD_USE,
                // like `jet_string_split` — emitted bare, no root prefix).
                TBuiltinOp::Lines => format!("jet_string_lines(&({}))", recv),
                // c97/D-STRPARSE1: `to_int()` on a String — fallible parse mirroring
                // `Int.parse`. The `Err` (a `ParseError`) lowers to a plain message string.
                TBuiltinOp::ToIntString => {
                    format!(
                        "({}).trim().parse::<i64>().map_err(|e| e.to_string())",
                        recv
                    )
                }
                TBuiltinOp::StartsWith => format!("({}).starts_with(&{})", recv, a(0)),
                TBuiltinOp::EndsWith => format!("({}).ends_with(&{})", recv, a(0)),
                TBuiltinOp::Replace => format!("({}).replace(&{}, &{})", recv, a(0), a(1)),
                TBuiltinOp::ToUpper => format!("({}).to_uppercase()", recv),
                TBuiltinOp::ToLower => format!("({}).to_lowercase()", recv),
                TBuiltinOp::Repeat => format!("({}).repeat({} as usize)", recv, a(0)),
                TBuiltinOp::Slice { line } => format!(
                    "jet_string_slice(&({}), {}, {}, {:?}, {})",
                    recv,
                    a(0),
                    a(1),
                    cx.file,
                    line
                ),
                // D-STR-AFTER1: `after`/`before` — bare calls, no root prefix (same
                // MOD_USE-imported convention as `jet_string_split`/`jet_string_lines`).
                TBuiltinOp::After => format!("jet_string_after(&({}), &{})", recv, a(0)),
                TBuiltinOp::Before => format!("jet_string_before(&({}), &{})", recv, a(0)),
                // D-MEM1 stage S5: zero-copy siblings, `Stmt::Val` lowering only
                // (see `lower.rs`'s `b.string_view` branch) — bare calls, no
                // `.to_string()`, no root prefix (same convention as `After`/`Before`).
                TBuiltinOp::TrimView => format!("jet_string_trim_view(&({}))", recv),
                TBuiltinOp::AfterView => format!("jet_string_after_view(&({}), &{})", recv, a(0)),
                TBuiltinOp::BeforeView => format!("jet_string_before_view(&({}), &{})", recv, a(0)),
                TBuiltinOp::Keys => {
                    format!("({}).keys().cloned().collect::<Vec<_>>()", recv)
                }
                TBuiltinOp::Values => {
                    format!("({}).values().cloned().collect::<Vec<_>>()", recv)
                }
                TBuiltinOp::ContainsKey => format!("({}).contains_key(&{})", recv, a(0)),
                TBuiltinOp::ToString => format!("({}).jet_show()", recv),
                // D-REGEXENGINE1=A: `Match.group(n)` on the std-only match value.
                TBuiltinOp::MatchGroup => {
                    format!("({}).group({})", recv, a(0))
                }
                // D-COLLBREADTH1=A: Set<T> operations.
                TBuiltinOp::SetFrom => {
                    format!(
                        "({}).into_iter().collect::<std::collections::HashSet<_>>()",
                        recv
                    )
                }
                TBuiltinOp::SetInsert => format!("({}).insert({})", recv, a(0)),
                TBuiltinOp::SetRemove => format!("{{({}).remove(&{});}}", recv, a(0)),
                TBuiltinOp::SetToList => {
                    format!("({}).iter().cloned().collect::<Vec<_>>()", recv)
                }
                TBuiltinOp::SetUnion => format!(
                    "({}).union(&({})).cloned().collect::<std::collections::HashSet<_>>()",
                    recv,
                    a(0)
                ),
                TBuiltinOp::SortedSetFrom => {
                    format!(
                        "({}).into_iter().collect::<std::collections::BTreeSet<_>>()",
                        recv
                    )
                }
                TBuiltinOp::SortedSetInsert => format!("({}).insert({})", recv, a(0)),
                TBuiltinOp::SortedSetRemove => format!("{{({}).remove(&{});}}", recv, a(0)),
                TBuiltinOp::SortedSetToList => {
                    format!("({}).iter().cloned().collect::<Vec<_>>()", recv)
                }
                TBuiltinOp::SortedSetUnion => format!(
                    "({}).union(&({})).cloned().collect::<std::collections::BTreeSet<_>>()",
                    recv,
                    a(0)
                ),
                TBuiltinOp::PriorityQueueFrom => {
                    format!(
                        "({}).into_iter().collect::<std::collections::BinaryHeap<_>>()",
                        recv
                    )
                }
                TBuiltinOp::PriorityQueuePeek => format!("({}).peek().cloned()", recv),
                TBuiltinOp::PriorityQueueToSortedList => {
                    format!("({}).clone().into_sorted_vec().into_iter().rev().collect::<Vec<_>>()", recv)
                }
                TBuiltinOp::LruPut => format!("({}).put({}, {})", recv, a(0), a(1)),
                TBuiltinOp::LruGet => format!("({}).get(&{})", recv, a(0)),
                TBuiltinOp::LruCapacity => format!("({}).capacity()", recv),
                TBuiltinOp::LruKeys => format!("({}).keys()", recv),
                TBuiltinOp::BitSetAdd => format!("({}).add({})", recv, a(0)),
                TBuiltinOp::BitSetRemove => format!("({}).remove(&{})", recv, a(0)),
                TBuiltinOp::BitSetCount => format!("({}).count()", recv),
                TBuiltinOp::BitSetToList => format!("({}).to_list()", recv),
                TBuiltinOp::BitSetNew => "JetBitSet::new()".to_string(),
                TBuiltinOp::ByteBufferNew => "JetByteBuffer::new()".to_string(),
                TBuiltinOp::ByteBufferFrom => format!("JetByteBuffer::from(&({}))", recv),
                TBuiltinOp::ByteBufferWrite { method } => {
                    if method == "write_bytes" {
                        format!("({}).{}(&{})", recv, method, a(0))
                    } else {
                        format!("({}).{}({})", recv, method, a(0))
                    }
                }
                TBuiltinOp::ByteBufferToBytes => format!("({}).to_bytes()", recv),
                // D-TAG1: Bag<T> counted multiset.
                TBuiltinOp::BagAdd => format!(
                    "{{ *({}).entry({}).or_insert(0) += 1; }}",
                    recv,
                    a(0)
                ),
                TBuiltinOp::BagRemove => format!(
                    "{{ if let Some(c) = ({recv}).get_mut(&{arg}) {{ *c -= 1; if *c == 0 {{ ({recv}).remove(&{arg}); }} }} }}",
                    recv = recv,
                    arg = a(0)
                ),
                TBuiltinOp::BagHas => format!(
                    "({}).get(&{}).copied().unwrap_or(0) > 0",
                    recv,
                    a(0)
                ),
                TBuiltinOp::BagCount => format!(
                    "({}).get(&{}).copied().unwrap_or(0) as i64",
                    recv,
                    a(0)
                ),
                TBuiltinOp::BagLen => format!(
                    "({}).values().sum::<usize>() as i64",
                    recv
                ),
                // D-COLLBREADTH1=A: Deque<T> operations.
                TBuiltinOp::DequePushFront => format!("({}).push_front({})", recv, a(0)),
                TBuiltinOp::DequePushBack => format!("({}).push_back({})", recv, a(0)),
                TBuiltinOp::DequePopFront => format!("({}).pop_front()", recv),
                TBuiltinOp::DequePopBack => format!("({}).pop_back()", recv),
                TBuiltinOp::DequePeekFront => format!("({}).front().cloned()", recv),
                TBuiltinOp::DequePeekBack => format!("({}).back().cloned()", recv),
                TBuiltinOp::TryCollect => format!("jet_list_try_collect(({}).clone())", recv),
                // D-DYNARRAY1: `list.view(a..b)` — zero-copy window constructor.
                // `&(recv)` (not `.clone()`): the window borrows the list's OWN
                // backing storage, it never makes a second copy of it.
                TBuiltinOp::ViewNew { line } => format!(
                    "jet_view_new(&({}), {}, {}, {:?}, {})",
                    recv,
                    a(0),
                    a(1),
                    cx.file,
                    line
                ),
                // D-ITER1: non-closure lazy adapters.
                TBuiltinOp::Take => format!("jet_list_take(({}).clone(), {})", recv, a(0)),
                TBuiltinOp::Skip => format!("jet_list_skip(({}).clone(), {})", recv, a(0)),
                TBuiltinOp::StepBy => format!("jet_list_step_by(({}).clone(), {})", recv, a(0)),
                TBuiltinOp::Dedup => format!("jet_list_dedup(({}).clone())", recv),
                TBuiltinOp::Chunks => format!("jet_list_chunks(({}).clone(), {})", recv, a(0)),
                TBuiltinOp::Windows => format!("jet_list_windows(({}).clone(), {})", recv, a(0)),
                TBuiltinOp::Enumerate { tuple_struct } => format!(
                    "({}).clone().into_iter().enumerate()\
                     .map(|(i, x)| {} {{ user_idx: i as i64, user_item: x }})\
                     .collect::<Vec<_>>()",
                    recv, tuple_struct
                ),
                TBuiltinOp::Zip { tuple_struct } => format!(
                    "({}).clone().into_iter().zip(({}).clone().into_iter())\
                     .map(|(x, y)| {} {{ user_a: x, user_b: y }})\
                     .collect::<Vec<_>>()",
                    recv,
                    a(0),
                    tuple_struct
                ),
                // D-HOLE1: `.zip` on `T?` — Rust's native `Option::zip`, wrapped into
                // the named-tuple struct (present only when both operands are).
                TBuiltinOp::OptionZip { tuple_struct, .. } => format!(
                    "({}).clone().zip(({}).clone())\
                     .map(|(x, y)| {} {{ user_a: x, user_b: y }})",
                    recv,
                    a(0),
                    tuple_struct
                ),
            }
        }
        // c109 Phase 12: a numeric predicate / bit-pop / width-conversion method. The
        // width source/target + widening-vs-narrowing branch were resolved into `op` at
        // lowering; emit only formats, reproducing `emit_builtin_method`'s numeric arms
        // + `numeric_conversion` (Source/Codegen/Expression.rs) byte-for-byte.
        TExprKind::NumericMethod { recv, op } => {
            let recv = emit_tir_expr(recv, cx);
            match op {
                TNumericOp::Predicate(m) => format!("({}).{}()", recv, m),
                TNumericOp::BitCount(m) => format!("(({}).{}() as i64)", recv, m),
                TNumericOp::ToShow => format!("({}).jet_show()", recv),
                TNumericOp::Origin => format!("{}jet_float_origin(&({}))", cx.root_prefix, recv),
                TNumericOp::CastAs { dst_rust } => format!("(({}) as {})", recv, dst_rust),
                TNumericOp::TryFrom {
                    dst_rust,
                    dst_spelling,
                } => format!(
                    "<{dst_rust}>::try_from(({recv}) as i128).map_err(|_| \
                     \"value doesn't fit in {dst_spelling}\".to_string())"
                ),
            }
        }
        // c109 Phase 28: an overflow opt-out builtin. `prefix`/`op` were resolved at
        // lowering; reproduce `emit_call`'s `(ls).{name}_{suffix}(rs)` byte-for-byte.
        TExprKind::OverflowOpt {
            prefix,
            op,
            lhs,
            rhs,
        } => {
            let ls = emit_tir_expr(lhs, cx);
            let rs = emit_tir_expr(rhs, cx);
            format!("({}).{}_{}({})", ls, prefix, op, rs)
        }
        // c109 Phase 10: a core/stdlib module call. Reproduces `emit_core_call`
        // (Source/Codegen/Expression.rs) byte-for-byte. `module`/`method` were
        // resolved at lowering; `cx.root_prefix`/`cx.ffi_crate` are program-level
        // (read here, like Phase 9's `cx.file`). Args were lowered PLAINLY — the
        // per-arm `&(…)`/`&mut (…)`/move wrappers are baked into each arm, exactly
        // as `emit_core_call` does (it ignores `CallArg.flags`).
        TExprKind::CoreCall {
            module,
            method,
            args,
        } => emit_tir_core_call(module, method, args, &e.ty, cx),
        TExprKind::Binary {
            op,
            overflow,
            line,
            lhs,
            rhs,
        } => {
            let ls = emit_tir_expr(lhs, cx);
            let rs = emit_tir_expr(rhs, cx);
            if *overflow {
                // Trapping helper: source location was resolved at lowering, so
                // the panic message matches the AST path exactly.
                let (file, line) = (&cx.file, *line);
                match op {
                    // D-NUMOPS1: shift-count traps. The count is widened to `i128`
                    // so a count of any integer width reaches `jet_shl`/`jet_shr`.
                    BinOp::Shl => {
                        format!("({}).jet_shl(({}) as i128, {:?}, {})", ls, rs, file, line)
                    }
                    BinOp::Shr => {
                        format!("({}).jet_shr(({}) as i128, {:?}, {})", ls, rs, file, line)
                    }
                    _ => {
                        let method = match op {
                            BinOp::Add => "jet_add",
                            BinOp::Sub => "jet_sub",
                            BinOp::Mul => "jet_mul",
                            BinOp::Div => "jet_div",
                            _ => unreachable!("overflow flag only set for +,-,*,/,<<,>>"),
                        };
                        format!("({}).{}(({}), {:?}, {})", ls, method, rs, file, line)
                    }
                }
            } else {
                format!("(({}) {} ({}))", ls, op.spell(), rs)
            }
        }
        // D-CHAINCMP1: `0 <= sev < 10` — a Rust block expression binds each
        // operand to a temp exactly once (single-evaluation for the shared
        // middle operands), then ANDs the adjacent-pair comparisons over
        // those temps: `{ let __jcc0 = (e0); let __jcc1 = (e1); …
        // (__jcc0 op0 __jcc1) && (__jcc1 op1 __jcc2) && … }`.
        TExprKind::CompareChain { operands, ops } => {
            let mut block = String::from("{ ");
            for (i, operand) in operands.iter().enumerate() {
                let os = emit_tir_expr(operand, cx);
                block.push_str(&format!("let __jcc{} = ({}); ", i, os));
            }
            let pairs: Vec<String> = ops
                .iter()
                .enumerate()
                .map(|(i, op)| format!("(__jcc{} {} __jcc{})", i, op.spell(), i + 1))
                .collect();
            block.push_str(&format!("({}) }}", pairs.join(" && ")));
            block
        }
        // D-LAYOUT1 / D-LAYOUT-GATES1 (GATE 1): `>=`/`<=`/`==` between
        // layout values register a `Constraint`, so it's a function call, not
        // a Rust operator.
        TExprKind::LayoutCompare { op, lhs, rhs } => {
            let ls = emit_tir_expr(lhs, cx);
            let rs = emit_tir_expr(rhs, cx);
            let func = match op {
                BinOp::Ge => "ge",
                BinOp::Le => "le",
                BinOp::Eq => "eq_",
                _ => unreachable!("layout comparisons are only >=, <=, =="),
            };
            format!("jet_layout::{}(({}), ({}))", func, ls, rs)
        }
        TExprKind::LayoutLit { inner } => {
            let i = emit_tir_expr(inner, cx);
            format!("jet_layout::LinExpr::from_const(({}) as f64)", i)
        }
        TExprKind::Unary { op, operand } => {
            let i = emit_tir_expr(operand, cx);
            match op {
                UnOp::Neg => format!("(-({}))", i),
                UnOp::Not => format!("(!({}))", i),
            }
        }
        TExprKind::IncDec {
            op, place, postfix, ..
        } => {
            let delta = match op {
                crate::AST::IncDecOp::Inc => "+",
                crate::AST::IncDecOp::Dec => "-",
            };
            if *postfix {
                format!("{{ let __jet_old = {place}; {place} {delta}= 1; __jet_old }}")
            } else {
                format!("{{ {place} {delta}= 1; {place} }}")
            }
        }
        // c109 Phase 3: `user_S { f: v, … }`. The Rust head and mangled field
        // names were resolved at lowering; values format like any other node.
        TExprKind::StructLit {
            rust_type,
            fields,
            extra,
            as_trait,
        } => {
            let mut parts = fields
                .iter()
                .map(|(field_rust, v, boxed)| {
                    let value = emit_tir_expr(v, cx);
                    // c109: a boxed (self-referential) field is wrapped `Box::new(…)`,
                    // exactly as `emit_struct_lit`. The `boxed` flag is total (resolved
                    // at lowering from `cx.boxed_edges`).
                    let value = if *boxed {
                        format!("Box::new({})", value)
                    } else {
                        value
                    };
                    format!("{}: {}", field_rust, value)
                })
                .collect::<Vec<_>>();
            // c109 Phase 17: a prelude struct's injected field (HttpRequest's `params`),
            // appended verbatim after the user fields, exactly as `emit_struct_lit` does.
            if let Some(extra) = extra {
                parts.push(extra.clone());
            }
            let lit = format!("{} {{ {} }}", rust_type, parts.join(", "));
            // c109 Phase 30: a trait-object coercion wraps the whole literal, byte-for-byte
            // `emit_struct_lit`'s `as_trait` branch (Source/Codegen/Expression.rs ~L342).
            match as_trait {
                Some(trait_rust) => format!("Box::new({lit}) as Box<dyn {trait_rust}>"),
                None => lit,
            }
        }
        // c109 Phase 3: `(recv).field`. Mirrors the AST `Expr::Field` emit form
        // exactly (no deref, no clone — owning reads were rewritten to a `.clone()`
        // MethodCall in sema and excluded from the subset).
        TExprKind::Field {
            recv,
            field_rust,
            boxed,
        } => {
            let read = format!("({}).{}", emit_tir_expr(recv, cx), field_rust);
            if *boxed {
                format!("(*{})", read)
            } else {
                read
            }
        }
        // c109 Phase 18: `mem.Ptr<T>.from_addr(addr)` — `(({addr}) as usize as *mut {T})`,
        // byte-for-byte `emit_expr`'s `PtrFromAddr` arm. The cast is safe Rust (no
        // `unsafe`); `elem_rust` was resolved at lowering.
        TExprKind::PtrFromAddr { elem_rust, addr } => {
            format!(
                "(({}) as usize as *mut {})",
                emit_tir_expr(addr, cx),
                elem_rust
            )
        }
        // D-CAP9: postfix `p.*` deref → Rust `(*(p))`. The `unsafe` is supplied by
        // the enclosing `#Unsafe` region (sema-gated); this node adds no `unsafe`.
        TExprKind::Deref(operand) => format!("(*({}))", emit_tir_expr(operand, cx)),
        // D-CAP9: prefix `*x` raw-of → `(&({}) as *const _ as *mut _)`. The result
        // is `*mut T` to match the canonical raw-pointer type (`Ptr<T>` lowers to
        // `*mut`). Forming the pointer is safe Rust; only dereferencing it needs
        // the surrounding `#Unsafe`. The const→mut cast is the standard idiom.
        TExprKind::RawOf(operand) => {
            format!("(&({}) as *const _ as *mut _)", emit_tir_expr(operand, cx))
        }
        // c109 Phase 19: the arena allocator constructor — the ctor tail was rendered whole
        // at lowering (`jet_mem::Jet<Alloc>::new()` / `::with_capacity(...)`), so emit just
        // splices it. Byte-for-byte `emit_method_call`'s arena constructor branch.
        TExprKind::AllocNew { ctor } => ctor.clone(),
        // c109 Phase 4/16: an enum literal. Prefix + payload were resolved at lowering;
        // emit applies each arg's resolved `clone`/`boxed` wrappers (mirroring
        // `emit_boxed_enum_arg`: `(…).clone()` first, then `Box::new(…)`).
        TExprKind::EnumLit { prefix, payload } => match payload {
            TEnumPayload::Unit => prefix.clone(),
            TEnumPayload::Positional(vals) => {
                let pos = vals
                    .iter()
                    .map(|a| emit_tir_enum_arg(a, cx))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({})", prefix, pos)
            }
            TEnumPayload::Named(fields) => {
                let parts = fields
                    .iter()
                    .map(|(name, a)| format!("{}: {}", name, emit_tir_enum_arg(a, cx)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{} {{ {} }}", prefix, parts)
            }
        },
        // c109 Phase 24: a JSON construction — `{root}jet_std::Json::<Variant>(<arg>)`.
        // Reproduces `emit_core_json_lit` (Expression.rs): the arg is wrapped in
        // `(…).clone()` iff its `implicit_clone` flag was set; `Null` has no arg.
        // D-ENC-DYN1=A+: a dynamic `Data` construction → `{root}jet_std::DataTree::
        // <Variant>(<arg>)`. The user-facing `Object` payload is a `Map<String, Data>`
        // (Rust `BTreeMap`); `DataTree::Object` is ordered `Vec<(String, DataTree)>`, so
        // the map is collected into pairs at the boundary (sorted-key order, matching the
        // old BTreeMap-backed dynamic value). Scalars/`Array` bind directly.
        TExprKind::JsonLit { variant, arg } => {
            let prefix = format!("{}jet_std::DataTree", cx.root_prefix);
            match arg {
                None => format!("{}::{}", prefix, variant),
                Some(boxed) => {
                    let (val, implicit_clone) = boxed.as_ref();
                    let s = emit_tir_expr(val, cx);
                    let arg_str = if *implicit_clone {
                        format!("({}).clone()", s)
                    } else {
                        s
                    };
                    if variant == "Object" {
                        format!(
                            "{}::{}(({}).into_iter().collect())",
                            prefix, variant, arg_str
                        )
                    } else {
                        format!("{}::{}({})", prefix, variant, arg_str)
                    }
                }
            }
        }
        // D-DBDRIVER1: a `DbValue` construction — `{root}jet_std::DbValue::<Variant>(<arg>)`.
        // Same shape as `JsonLit` (a foreign prelude enum), minus the recursive
        // `Array`/`Object` special-case (`DbValue` has no compound variants).
        TExprKind::DbValueLit { variant, arg } => {
            let prefix = format!("{}jet_std::DbValue", cx.root_prefix);
            match arg {
                None => format!("{}::{}", prefix, variant),
                Some(boxed) => {
                    let (val, implicit_clone) = boxed.as_ref();
                    let s = emit_tir_expr(val, cx);
                    let arg_str = if *implicit_clone {
                        format!("({}).clone()", s)
                    } else {
                        s
                    };
                    format!("{}::{}({})", prefix, variant, arg_str)
                }
            }
        }
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => {
            let c = emit_tir_expr(cond, cx);
            let then_block = emit_tir_value_block(then_body, then_value, cx);
            let else_block = emit_tir_value_block(else_body, else_value, cx);
            format!("if {} {} else {}", c, then_block, else_block)
        }
        // c109 Phase 5: `[a, b, c]` → `vec![a, b, c]` (growable) or `[a, b, c]` (fixed).
        // D-FIXARR1: if the expression type is FixedList, emit a Rust array literal `[…]`.
        TExprKind::ListLit(elems) => {
            let parts = elems
                .iter()
                .map(|e| emit_tir_expr(e, cx))
                .collect::<Vec<_>>()
                .join(", ");
            if matches!(&e.ty, Type::FixedList { .. }) {
                format!("[{}]", parts)
            } else {
                format!("vec![{}]", parts)
            }
        }
        TExprKind::ListSpread { parts } => {
            let mut s = String::from("{ let mut __jet_sp = Vec::new(); ");
            for part in parts {
                match part {
                    ListSpreadPart::Elem(elem) => {
                        s.push_str(&format!(
                            "__jet_sp.push(({}).clone()); ",
                            emit_tir_expr(elem, cx)
                        ));
                    }
                    ListSpreadPart::Spread(list) => {
                        s.push_str(&format!(
                            "__jet_sp.extend(({}).clone()); ",
                            emit_tir_expr(list, cx)
                        ));
                    }
                }
            }
            s.push_str("__jet_sp }");
            s
        }
        // D-SOA1: a columnar list literal → `user_<S>_columns::from_aos(vec![…])`.
        TExprKind::ColumnarListLit { columns_ty, elems } => {
            let parts = elems
                .iter()
                .map(|e| emit_tir_expr(e, cx))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}::from_aos(vec![{}])", columns_ty, parts)
        }
        // D-SOA1: `xs[i]` on a columnar list → bounds-checked gather of the logical S.
        TExprKind::ColumnarGather { base, index, line } => {
            let b = emit_tir_expr(base, cx);
            let i = emit_tir_expr(index, cx);
            format!("({}).gather_at({}, {:?}, {})", b, i, cx.file, line)
        }
        // D-SOA1: `xs[i].field` on a columnar list → direct column read.
        TExprKind::ColumnarColumnRead {
            base,
            index,
            column_rust,
            line,
        } => {
            let b = emit_tir_expr(base, cx);
            let i = emit_tir_expr(index, cx);
            format!(
                "jet_index_vec(&({}).{}, {}, {:?}, {})",
                b, column_rust, i, cx.file, line
            )
        }
        // c109 Phase 23: a named-tuple literal → `JetTup_<hash> { user_<f>: <v>, … }`.
        // Mirrors `emit_expr`'s `TupleLit` arm byte-for-byte (fields canonical-ordered,
        // resolved at lowering).
        TExprKind::TupleLit {
            struct_name,
            fields,
        } => {
            let parts = fields
                .iter()
                .map(|(n, v)| format!("{}: {}", n, emit_tir_expr(v, cx)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} {{ {} }}", struct_name, parts)
        }
        // c109 Phase 5: `[k: v, …]` / `[:]`. Mirrors the AST `Expr::MapLit` exactly:
        // empty → `BTreeMap::new()`; non-empty → the `_m.insert((k).clone(), v)` builder.
        TExprKind::MapLit(entries) => {
            if entries.is_empty() {
                "std::collections::BTreeMap::new()".to_string()
            } else {
                let mut s = String::from("{ let mut _m = std::collections::BTreeMap::new(); ");
                for (k, v) in entries {
                    s.push_str(&format!(
                        "_m.insert(({}).clone(), {}); ",
                        emit_tir_expr(k, cx),
                        emit_tir_expr(v, cx)
                    ));
                }
                s.push_str("_m }");
                s
            }
        }
        // c109 Phase 5: `coll[i]`. Dispatch on the total `is_map` fact (never
        // re-inferred). Mirrors the AST `Expr::Index` form: a map index borrows the
        // key (`&(i)`), a vec index does not.
        TExprKind::Index {
            base,
            index,
            is_map,
            line,
        } => {
            let b = emit_tir_expr(base, cx);
            let i = emit_tir_expr(index, cx);
            if *is_map {
                format!("jet_index_map(&({}), &({}), {:?}, {})", b, i, cx.file, line)
            } else {
                format!("jet_index_vec(&({}), {}, {:?}, {})", b, i, cx.file, line)
            }
        }
        TExprKind::IndexHook {
            type_name,
            base,
            index,
            line,
        } => {
            let ty = super::user_type_rust(type_name);
            let b = emit_tir_expr(base, cx);
            let i = emit_tir_expr(index, cx);
            format!(
                "{{ match <{ty} as user_Index>::get(&({b}), {i}) {{ Some(_jet_v) => _jet_v, None => {root}jet_panic({:?}, {}, \"index miss\") }} }}",
                cx.file,
                line,
                root = cx.root_prefix,
            )
        }
        // D-SIMD2: `v[i]` lane read → the bounds-checked prelude helper.
        TExprKind::MathLaneIndex {
            lane_ty,
            base,
            index,
            line,
        } => {
            let b = emit_tir_expr(base, cx);
            let i = emit_tir_expr(index, cx);
            format!(
                "{}jet_math_{}_lane(&({}), {}, {:?}, {})",
                cx.root_prefix, lane_ty, b, i, cx.file, line
            )
        }
        // D-SWIZZLE1: read swizzle `v.xyz` → lane extract (+ `VecN` ctor when N>1).
        TExprKind::MathSwizzleRead {
            type_name,
            recv,
            lanes,
        } => emit_math_swizzle_read(cx, type_name, recv, lanes),
        // c109 Phase 5: `coll[a..b]` → `jet_slice_vec`. Mirrors the AST `Expr::Slice`.
        TExprKind::Slice {
            base,
            start,
            end,
            line,
        } => {
            let b = emit_tir_expr(base, cx);
            let a = emit_tir_expr(start, cx);
            let e = emit_tir_expr(end, cx);
            format!(
                "jet_slice_vec(&({}), {}, {}, {:?}, {})",
                b, a, e, cx.file, line
            )
        }
        // c109 Phase 8: `value(x)` → `Some(x)` / `null` → `None`. Mirrors the AST
        // `Expr::Present`/`Expr::Absent` exactly.
        TExprKind::Present(inner) => format!("Some({})", emit_tir_expr(inner, cx)),
        TExprKind::Absent => "None".to_string(),
        // c109 Phase 23: a `#Todo` typed hole → diverging `todo!(…)`. Byte-for-byte the
        // AST `Expr::Todo` arm (Expression.rs): file/line/expected-type baked into the
        // panic string. `cx.file` is program-level (read here, like every other use).
        TExprKind::Todo {
            line,
            expected_type,
        } => format!(
            "todo!(\"#{} at {}:{} — expected {}\")",
            crate::Syntax::KW_TODO,
            cx.file,
            line,
            expected_type
        ),
        // c109 Phase 8: `ok(x)` → `Ok(x)` / `err(e)` → `Err(e)`. Mirrors the AST
        // `Expr::Ok`/`Expr::Err`.
        TExprKind::Ok(inner) => format!("Ok({})", emit_tir_expr(inner, cx)),
        TExprKind::Err(inner) => format!("Err({})", emit_tir_expr(inner, cx)),
        // c109 Phase 8: the `?` propagation operator. Mirrors `Expr::Try` byte-for-byte
        // (Expression.rs): a debug trace frame wraps the value, then the error is
        // converted per the total `TryConvert`, then `?` propagates. `file`/`fn_name`
        // were pre-escaped at lowering; `line` is plain.
        TExprKind::Try {
            inner,
            convert,
            file,
            line,
            fn_name,
        } => {
            let v = emit_tir_expr(inner, cx);
            match convert {
                // S80/D-LIB3: error implements Fallible → `.map_err(|e| e.to_error())`.
                TTryConvert::Fallible => format!(
                    "jet_trace_err({}.map_err(|e| e.to_error()), {}, {}, {})?",
                    v, file, line, fn_name
                ),
                // D-ERR-CONV: declared `impl Source -> Target` → `.map_err(<fn>)`.
                TTryConvert::Typed(conv_fn) => format!(
                    "jet_trace_err({}.map_err({}), {}, {}, {})?",
                    v, conv_fn, file, line, fn_name
                ),
                // Error types match — bare propagate.
                TTryConvert::None => {
                    format!("jet_trace_err({}, {}, {}, {})?", v, file, line, fn_name)
                }
            }
        }
        // c109 Phase 8: the `??` fallback operator. Mirrors `emit_or_fallback`
        // (Statement.rs): a `Result` value unwraps `Ok`, an `Option` value unwraps
        // `Some`; the fallback runs on `Err(_)`/`None`. Decision read off the total
        // `is_option` flag — no re-inference.
        TExprKind::OrFallback {
            value,
            fallback,
            is_option,
        } => {
            let v = emit_tir_expr(value, cx);
            let fb = emit_tir_orfallback_rhs(fallback, cx);
            if *is_option {
                format!("match {} {{ Some(__jet_v) => __jet_v, None => {} }}", v, fb)
            } else {
                format!(
                    "match {} {{ Ok(__jet_ok) => __jet_ok, Err(_) => {} }}",
                    v, fb
                )
            }
        }
        // c109 Phase 8: optional chaining `base?.member`. Mirrors `Expr::OptField`:
        // `(base).clone().{and_then|map}(|__optv| __optv.{member})`. The combinator is
        // the total `flatten` fact (flatten → `and_then`, else → `map`).
        TExprKind::OptField {
            base,
            member_rust,
            flatten,
        } => {
            let combinator = if *flatten { "and_then" } else { "map" };
            format!(
                "({}).clone().{}(|__optv| __optv.{})",
                emit_tir_expr(base, cx),
                combinator,
                member_rust
            )
        }
        // c109 Phase 11: a lambda/closure literal. All decisions (prep/move/box) were
        // resolved at lowering off `Lambda.meta`; emit only assembles, byte-for-byte
        // `emit_lambda`: `{move }|params| body`, wrapped `Box::new(…)` when it escapes,
        // and prefixed with the `{ <prep> … }` block when there are cloned captures.
        TExprKind::Lambda(lam) => {
            let move_kw = if lam.is_move { "move " } else { "" };
            let closure = format!("{}|{}| {}", move_kw, lam.params.join(", "), lam.body);
            let wrapped = if lam.boxed {
                format!("Box::new({})", closure)
            } else {
                closure
            };
            if lam.prep.is_empty() {
                wrapped
            } else {
                format!("{{ {} {} }}", lam.prep, wrapped)
            }
        }
        // c109 Phase 11: fan-out `f.[a, b, c]` → `vec![f(a), f(b), f(c)]`. The
        // per-item calls were lowered at lowering; emit only wraps them in `vec![…]`,
        // D-FIXARR1: fan-out `f.[a, b, c]` produces `[T#N]` — a Rust array literal `[…]`.
        TExprKind::FanOut { calls } => {
            let elems = calls
                .iter()
                .map(|c| emit_tir_expr(c, cx))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{}]", elems)
        }
        // D-HOLE1: `Option.lift2(f, a, b)` — `a.zip(b).map(|(x, y)| f(x, y))`. `f` is
        // any lowered function value (lambda or fn ident), called via Rust's
        // call-operator syntax on the (possibly boxed) closure.
        TExprKind::OptionLift2 { f, a, b } => {
            let f = emit_tir_expr(f, cx);
            let a = emit_tir_expr(a, cx);
            let b = emit_tir_expr(b, cx);
            format!(
                "({}).clone().zip(({}).clone()).map(|(x, y)| ({})(x, y))",
                a, b, f
            )
        }
        // c109 Phase 11: a closure-taking collection method. The receiver-type +
        // Fn-vs-FnMut dispatch was resolved into `op` at lowering; emit only formats,
        // reproducing `emit_builtin_method`'s closure arms byte-for-byte. Args (the
        // lambda + any seed) are emitted PLAINLY (raw `arg(i)`).
        TExprKind::ClosureMethod { recv, op, args } => {
            let recv = emit_tir_expr(recv, cx);
            let a = |i: usize| {
                args.get(i)
                    .map(|e| emit_tir_expr(e, cx))
                    .unwrap_or_default()
            };
            match op {
                TClosureOp::Map => format!("jet_list_map(({}).clone(), {})", recv, a(0)),
                TClosureOp::MapMut => format!("jet_list_map_mut(({}).clone(), {})", recv, a(0)),
                // D-HOLE1: `.map` on `T?` — Rust's native `Option::map`.
                TClosureOp::OptionMap => format!("({}).clone().map({})", recv, a(0)),
                TClosureOp::Filter => format!("jet_list_filter(({}).clone(), {})", recv, a(0)),
                TClosureOp::Each => format!("jet_list_each(({}).clone(), {})", recv, a(0)),
                TClosureOp::EachMut => format!("jet_list_each_mut(({}).clone(), {})", recv, a(0)),
                TClosureOp::EachRef => format!("jet_list_each_ref(&({}), {})", recv, a(0)),
                TClosureOp::EachMap => format!("jet_map_each(({}).clone(), {})", recv, a(0)),
                TClosureOp::Find => format!("jet_list_find(({}).clone(), {})", recv, a(0)),
                TClosureOp::Any => format!("jet_list_any(({}).clone(), {})", recv, a(0)),
                TClosureOp::All => format!("jet_list_all(({}).clone(), {})", recv, a(0)),
                TClosureOp::SortBy => format!("{{ jet_list_sort_by(&mut {}, {}); }}", recv, a(0)),
                TClosureOp::Reduce => {
                    format!("jet_list_reduce(({}).clone(), {}, {})", recv, a(0), a(1))
                }
                // D-ITER1: new closure adapters.
                TClosureOp::TakeWhile => {
                    format!("jet_list_take_while(({}).clone(), {})", recv, a(0))
                }
                TClosureOp::SkipWhile => {
                    format!("jet_list_skip_while(({}).clone(), {})", recv, a(0))
                }
                TClosureOp::FlatMap => {
                    format!("jet_list_flat_map(({}).clone(), {})", recv, a(0))
                }
                TClosureOp::FilterMap => {
                    format!("jet_list_filter_map(({}).clone(), {})", recv, a(0))
                }
                // D-AUTOPAR1=A: parallel adapters.
                TClosureOp::ParMap => format!("jet_list_par_map(({}).clone(), {})", recv, a(0)),
                TClosureOp::ParFilter => {
                    format!("jet_list_par_filter(({}).clone(), {})", recv, a(0))
                }
                TClosureOp::ParFold => {
                    format!("jet_list_par_fold(({}).clone(), {}, {})", recv, a(0), a(1))
                }
                TClosureOp::Scan => {
                    format!("jet_list_scan(({}).clone(), {}, {})", recv, a(0), a(1))
                }
                TClosureOp::Fold => {
                    format!("jet_list_fold(({}).clone(), {}, {})", recv, a(0), a(1))
                }
                // D-DYNARRAY1: `recv` is already a `&[T]` borrow — fold/map it
                // directly, no `.clone()`-to-owned-Vec (that would defeat the
                // zero-copy point of `.view(...)`).
                TClosureOp::ViewFold => {
                    format!("jet_view_fold(({}), {}, {})", recv, a(0), a(1))
                }
                TClosureOp::ViewMap => format!("jet_view_map(({}), {})", recv, a(0)),
                TClosureOp::Position => {
                    format!("jet_list_position(({}).clone(), {})", recv, a(0))
                }
                TClosureOp::MinBy => {
                    format!("jet_list_min_by(({}).clone(), {})", recv, a(0))
                }
                TClosureOp::MaxBy => {
                    format!("jet_list_max_by(({}).clone(), {})", recv, a(0))
                }
                TClosureOp::GroupBy => {
                    format!("jet_list_group_by(({}).clone(), {})", recv, a(0))
                }
                TClosureOp::CountBy => {
                    format!("jet_list_count_by(({}).clone(), {})", recv, a(0))
                }
                TClosureOp::Partition { tuple_struct } => {
                    // `partition` passes each element by value (T: Clone).
                    // The lambda `f` takes T by value, but `Iterator::partition`
                    // passes `&T` to its predicate. Use jet_list_partition helper.
                    format!(
                        "jet_list_partition(({}).clone(), {}, |__t, __f| \
                         {} {{ user_false_: __f, user_true_: __t }})",
                        recv,
                        a(0),
                        tuple_struct
                    )
                }
            }
        }
        // c109 Phase 13: a method ON a handle. The handle-receiver branch was resolved
        // into `op` at lowering; emit only formats, reproducing the handle arms of
        // `emit_builtin_method` (Source/Codegen/Expression.rs) byte-for-byte. Args are
        // emitted PLAINLY (raw `arg(i)`). `cx.root_prefix` is program-level.
        TExprKind::HandleMethod { recv, op, args } => {
            let recv = emit_tir_expr(recv, cx);
            let a = |i: usize| {
                args.get(i)
                    .map(|e| emit_tir_expr(e, cx))
                    .unwrap_or_default()
            };
            let root = &cx.root_prefix;
            let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
            match op {
                THandleOp::FileReaderReadLine => {
                    format!("{}jet_std_file_reader_read_line(&mut ({}))", root, recv)
                }
                THandleOp::FileWriterWriteLine => format!(
                    "{}jet_std_file_writer_write_line(&mut ({}), &({}))",
                    root,
                    recv,
                    a(0)
                ),
                THandleOp::FileWriterFlush => {
                    format!("{}jet_std_file_writer_flush(&mut ({}))", root, recv)
                }
                THandleOp::StdinReadLine => {
                    format!("{}jet_std_io_stdin_read_line(&mut ({}))", root, recv)
                }
                THandleOp::StdoutWrite => {
                    format!("{}jet_std_io_stdout_write(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::StdoutWriteLine => {
                    format!("{}jet_std_io_stdout_write_line(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::StdoutWriteBytes => {
                    format!("{}jet_std_io_stdout_write_bytes(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::StdoutFlush => {
                    format!("{}jet_std_io_stdout_flush(&mut ({}))", root, recv)
                }
                THandleOp::StdoutIsTty => {
                    format!("{}jet_std_io_stdout_is_tty(&({}))", root, recv)
                }
                THandleOp::StderrWrite => {
                    format!("{}jet_std_io_stderr_write(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::StderrWriteLine => {
                    format!("{}jet_std_io_stderr_write_line(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::StderrWriteBytes => {
                    format!("{}jet_std_io_stderr_write_bytes(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::StderrFlush => {
                    format!("{}jet_std_io_stderr_flush(&mut ({}))", root, recv)
                }
                THandleOp::StderrIsTty => {
                    format!("{}jet_std_io_stderr_is_tty(&({}))", root, recv)
                }
                THandleOp::StopwatchElapsedMillis => {
                    format!("{}jet_stopwatch_elapsed_millis(&({}))", root, recv)
                }
                // D-DET1: deterministic injected Clock/Rng capability methods.
                THandleOp::ClockNow => format!("{}jet_clock_now(&({}))", root, recv),
                THandleOp::ClockTick => {
                    format!("{}jet_clock_tick(&mut ({}), {})", root, recv, a(0))
                }
                // D-DET-CAPAPI: absolute set + Duration advance; widened Rng; Duration read.
                THandleOp::ClockAdvance => {
                    format!("{}jet_clock_advance(&mut ({}), {})", root, recv, a(0))
                }
                THandleOp::ClockWait => {
                    format!("{}jet_clock_wait(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::RngInt => {
                    format!("{}jet_rng_int(&mut ({}), {}, {})", root, recv, a(0), a(1))
                }
                THandleOp::RngFloat => format!("{}jet_rng_float(&mut ({}))", root, recv),
                THandleOp::RngFloatRange => {
                    format!("{}jet_rng_float_range(&mut ({}), {}, {})", root, recv, a(0), a(1))
                }
                THandleOp::RngBool => format!("{}jet_rng_bool(&mut ({}))", root, recv),
                THandleOp::RngBoolP => {
                    format!("{}jet_rng_bool_p(&mut ({}), {})", root, recv, a(0))
                }
                THandleOp::RngNormal => {
                    format!("{}jet_rng_normal(&mut ({}), {}, {})", root, recv, a(0), a(1))
                }
                THandleOp::RngExponential => {
                    format!("{}jet_rng_exponential(&mut ({}), {})", root, recv, a(0))
                }
                THandleOp::RngBytes => {
                    format!("{}jet_rng_bytes(&mut ({}), {})", root, recv, a(0))
                }
                THandleOp::RngSplit => format!("{}jet_rng_split(&mut ({}))", root, recv),
                THandleOp::RngPick => {
                    format!("{}jet_rng_pick(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::RngWeightedPick => {
                    format!("{}jet_rng_weighted_pick(&mut ({}), &({}), &({}))", root, recv, a(0), a(1))
                }
                THandleOp::RngSample => {
                    format!("{}jet_rng_sample(&mut ({}), &({}), {})", root, recv, a(0), a(1))
                }
                THandleOp::RngShuffle => {
                    format!("{}jet_rng_shuffle(&mut ({}), &mut ({}))", root, recv, a(0))
                }
                // D-SOLVER-LIB1=A: explicit finite solver state.
                THandleOp::SolverNew => format!("{}jet_solver_new({})", root, recv),
                THandleOp::SolverRequire => {
                    format!("{}jet_solver_require(&mut ({}), {})", root, recv, a(0))
                }
                THandleOp::SolverFailureCount => {
                    format!("{}jet_solver_failure_count(&({}))", root, recv)
                }
                THandleOp::SolverStatus => format!("{}jet_solver_status(&({}))", root, recv),
                THandleOp::GameSceneNew => format!("{}jet_game_scene_new(&({}))", root, recv),
                THandleOp::GameReplayRecord => {
                    format!("{}jet_game_replay_record(&({}))", root, recv)
                }
                THandleOp::GameBackendHeadless => format!("{}jet_game_backend_headless()", root),
                THandleOp::GameBudgetsNew => format!(
                    "{}jet_game_budgets_new({}, {}, {}, {})",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2)
                ),
                THandleOp::GameSceneOnFrame => {
                    format!("{}jet_game_scene_on_frame(&mut ({}), {})", root, recv, a(0))
                }
                THandleOp::GameSceneComponent => {
                    format!("{}jet_game_scene_component(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::GameSceneQuery => {
                    format!("{}jet_game_scene_query(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::GameAssetsImage => {
                    format!("{}jet_game_assets_image(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::GameAssetsSound => {
                    format!("{}jet_game_assets_sound(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::GameInputBind => {
                    format!("{}jet_game_input_bind(&({}), &({}), &({}))", root, recv, a(0), a(1))
                }
                THandleOp::GameBudgetsSet => {
                    format!("{}jet_game_budgets_set(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::GameInputPressed => {
                    format!("{}jet_game_input_pressed(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::DurationMillis => {
                    format!("{}jet_duration_millis(&({}))", root, recv)
                }
                THandleOp::DurationSeconds => {
                    format!("{}jet_duration_seconds(&({}))", root, recv)
                }
                THandleOp::PreciseMethod { type_name, method } => {
                    let prefix = if type_name == "BigInt" {
                        "jet_bigint"
                    } else {
                        "jet_decimal"
                    };
                    if method == "to_string" {
                        format!("{}{}_to_string(&({}))", root, prefix, recv)
                    } else if method == "neg" {
                        format!("{}{}_neg(&({}))", root, prefix, recv)
                    } else {
                        format!(
                            "{}{}_{}(&({}), &({}))",
                            root, prefix, method, recv, a(0)
                        )
                    }
                }
                THandleOp::TcpListenerAccept => format!("{}jet_net_tcp_accept(&({}))", root, recv),
                THandleOp::TcpListenerLocalAddr => {
                    format!("{}jet_net_listener_local_addr(&({}))", root, recv)
                }
                THandleOp::TcpStreamRead => format!("{}jet_net_tcp_read(&mut ({}))", root, recv),
                THandleOp::TcpStreamWrite => {
                    format!("{}jet_net_tcp_write(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::TcpStreamPeerAddr => {
                    format!("{}jet_net_tcp_peer_addr(&({}))", root, recv)
                }
                THandleOp::TcpStreamLocalAddr => {
                    format!("{}jet_net_tcp_local_addr(&({}))", root, recv)
                }
                THandleOp::TcpStreamClose => format!("{{ drop({}); }}", recv),
                // c109 Phase 19: arena allocator methods (byte-for-byte the AST arms).
                THandleOp::AllocAlloc => {
                    let a0 = emit_tir_expr(&args[0], cx);
                    format!("({}).alloc({})", recv, a0)
                }
                THandleOp::AllocReset => format!("({}).reset()", recv),
                THandleOp::AllocFree => format!("drop({})", recv),
                // c109 Phase 20: HttpRequest/HttpResponse accessors, byte-for-byte the
                // `emit_builtin_method` arms. The plain field accessors clone the field;
                // `header` does a map lookup; `param` calls the prelude helper.
                THandleOp::HttpReqField(field) | THandleOp::HttpRespField(field) => {
                    format!("({}).{}.clone()", recv, field)
                }
                THandleOp::HttpReqHeader | THandleOp::HttpRespHeader => {
                    format!("({}).headers.get(&{}).cloned()", recv, a(0))
                }
                THandleOp::HttpReqParam => {
                    format!("{}jet_http_request_param(&({}), &({}))", root, recv, a(0))
                }
                // c109 Phase 21: Task/Channel/Sender methods, byte-for-byte the
                // `emit_builtin_method` arms (Source/Codegen/Expression.rs). The handle
                // value's prelude methods take `&self`, so the receiver is emitted plainly
                // (Rust autoref); args are plain (raw `emit_expr`). `join` reuses the
                // no-arg `join` arm (`(recv).join()`); `detach` drops the handle (D-DETACH1).
                // D-ARGS1: ArgsSpec builder methods (consuming by value; builder is moved on each call).
                THandleOp::ArgsSpecFlag => {
                    format!("{}jet_args_flag({}, &({}), &({}))", root, recv, a(0), a(1))
                }
                THandleOp::ArgsSpecFlagShort => format!(
                    "{}jet_args_flag_short({}, &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2)
                ),
                THandleOp::ArgsSpecOption => format!(
                    "{}jet_args_option({}, &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2)
                ),
                THandleOp::ArgsSpecOptionShort => format!(
                    "{}jet_args_option_short({}, &({}), &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2),
                    a(3)
                ),
                THandleOp::ArgsSpecOptionDefault => format!(
                    "{}jet_args_option_default({}, &({}), &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2),
                    a(3)
                ),
                THandleOp::ArgsSpecOptionEnv => format!(
                    "{}jet_args_option_env({}, &({}), &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2),
                    a(3)
                ),
                THandleOp::ArgsSpecOptionInt => format!(
                    "{}jet_args_option_int({}, &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2)
                ),
                THandleOp::ArgsSpecOptionFloat => format!(
                    "{}jet_args_option_float({}, &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2)
                ),
                THandleOp::ArgsSpecOptionChoice => format!(
                    "{}jet_args_option_choice({}, &({}), &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2),
                    a(3)
                ),
                THandleOp::ArgsSpecRepeat => format!(
                    "{}jet_args_repeat({}, &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2)
                ),
                THandleOp::ArgsSpecRequiredOption => format!(
                    "{}jet_args_required_option({}, &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2)
                ),
                THandleOp::ArgsSpecPositional => format!(
                    "{}jet_args_positional({}, &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1)
                ),
                THandleOp::ArgsSpecSubcommand => format!(
                    "{}jet_args_subcommand({}, &({}), &({}), {})",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2)
                ),
                THandleOp::ArgsSpecVersion => {
                    format!("{}jet_args_version({}, &({}))", root, recv, a(0))
                }
                THandleOp::ArgsSpecCompletion => {
                    format!("{}jet_args_completion(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::ArgsSpecHelp => format!("({}).help()", recv),
                THandleOp::ArgsSpecParse => {
                    format!("{}jet_args_parse(&({}), &({}))", root, recv, a(0))
                }
                // D-ARGS1: ParsedArgs query methods.
                THandleOp::ParsedArgsFlag => {
                    format!("{}jet_parsed_flag(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::ParsedArgsOption => {
                    format!("{}jet_parsed_option(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::ParsedArgsOptionInt => {
                    format!("{}jet_parsed_option_int(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::ParsedArgsOptionFloat => {
                    format!("{}jet_parsed_option_float(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::ParsedArgsOptions => {
                    format!("{}jet_parsed_options(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::ParsedArgsPositional => {
                    format!("{}jet_parsed_positional(&({}), {})", root, recv, a(0))
                }
                THandleOp::ParsedArgsSubcommand => {
                    format!("{}jet_parsed_subcommand(&({}))", root, recv)
                }
                THandleOp::ProcessSpecMethod { method } => match method.as_str() {
                    "cwd" => format!("{}jet_process_spec_cwd({}, &({}))", root, recv, a(0)),
                    "env" => format!(
                        "{}jet_process_spec_env({}, &({}), &({}))",
                        root,
                        recv,
                        a(0),
                        a(1)
                    ),
                    "env_remove" => {
                        format!("{}jet_process_spec_env_remove({}, &({}))", root, recv, a(0))
                    }
                    "env_clear" => format!("{}jet_process_spec_env_clear({})", root, recv),
                    "stdin_text" => {
                        format!("{}jet_process_spec_stdin_text({}, &({}))", root, recv, a(0))
                    }
                    "stdout" => {
                        format!("{}jet_process_spec_stdout({}, &({}))", root, recv, a(0))
                    }
                    "stderr" => {
                        format!("{}jet_process_spec_stderr({}, &({}))", root, recv, a(0))
                    }
                    "stdout_capture" => {
                        format!("{}jet_process_spec_stdout_capture({})", root, recv)
                    }
                    "stdout_inherit" => {
                        format!("{}jet_process_spec_stdout_inherit({})", root, recv)
                    }
                    "stdout_discard" => {
                        format!("{}jet_process_spec_stdout_discard({})", root, recv)
                    }
                    "stderr_capture" => {
                        format!("{}jet_process_spec_stderr_capture({})", root, recv)
                    }
                    "stderr_inherit" => {
                        format!("{}jet_process_spec_stderr_inherit({})", root, recv)
                    }
                    "stderr_discard" => {
                        format!("{}jet_process_spec_stderr_discard({})", root, recv)
                    }
                    "timeout_ms" => {
                        format!("{}jet_process_spec_timeout_ms({}, {})", root, recv, a(0))
                    }
                    "output_limit" => {
                        format!("{}jet_process_spec_output_limit({}, {})", root, recv, a(0))
                    }
                    "detached" => format!("{}jet_process_spec_detached({})", root, recv),
                    "run" => format!("{}jet_process_spec_run(&({}))", root, recv),
                    "spawn" => format!("{}jet_process_spec_spawn(&({}))", root, recv),
                    _ => format!("/* unsupported ProcessSpec.{method} */ {{ unreachable!() }}"),
                },
                THandleOp::ProcessChildMethod { method } => match method.as_str() {
                    "id" => format!("{}jet_process_child_id(&({}))", root, recv),
                    "wait" => format!("{}jet_process_child_wait(&({}))", root, recv),
                    "kill" => format!("{}jet_process_child_kill(&({}))", root, recv),
                    "terminate" => {
                        format!("{}jet_process_child_terminate(&({}))", root, recv)
                    }
                    "interrupt" => format!("{}jet_process_child_interrupt(&({}))", root, recv),
                    "write_stdin" => {
                        format!("{}jet_process_child_write_stdin(&({}), &({}))", root, recv, a(0))
                    }
                    "read_stdout_line" => {
                        format!("{}jet_process_child_read_stdout_line(&({}))", root, recv)
                    }
                    "read_stderr_line" => {
                        format!("{}jet_process_child_read_stderr_line(&({}))", root, recv)
                    }
                    _ => format!("/* unsupported ProcessChild.{method} */ {{ unreachable!() }}"),
                },
                // D-ANY-JAI1 (c7jaiany §6): Value/Field are plain inherent-method
                // passthroughs, same shape as `ArgsSpecHelp`.
                THandleOp::ReflectValueTypeName => format!("({}).type_name()", recv),
                THandleOp::ReflectValueDisplay => format!("({}).display()", recv),
                THandleOp::ReflectValueFields => format!("({}).fields()", recv),
                THandleOp::ReflectFieldName => format!("({}).name()", recv),
                THandleOp::ReflectFieldValue => format!("({}).value()", recv),
                THandleOp::TaskJoin => format!("({}).join()", recv),
                THandleOp::TaskDetach => format!("{{ let _detach = ({}); }}", recv),
                THandleOp::TaskPause => format!("({}).pause()", recv),
                THandleOp::TaskResume => format!("({}).resume()", recv),
                THandleOp::TaskCancel => format!("({}).cancel()", recv),
                THandleOp::TaskTrace => format!("({}).trace()", recv),
                THandleOp::ChannelReceive => format!("({}).receive()", recv),
                THandleOp::SenderSend => format!("({}).send({})", recv, a(0)),
                // D-REACT1=B: reactive Signal/Derived reads and writes.
                THandleOp::ReactiveGet => format!("({}).get()", recv),
                THandleOp::ReactiveSet => format!("({}).set({})", recv, a(0)),
                // D-EVENT1=D: first-party typed Event/Hook runtime family.
                THandleOp::EventMethod { method } => match method.as_str() {
                    "on" | "once" => format!("({}).{}(&({}), {})", recv, method, a(0), a(1)),
                    "on_priority" => {
                        format!("({}).on_priority(&({}), {}, {})", recv, a(0), a(1), a(2))
                    }
                    "emit" | "emit_async" | "cancel" | "unsubscribe" | "active" | "active_count"
                    | "trace" | "listener_count" | "queued_count" | "summary" | "delivered"
                    | "queued" | "dropped" => {
                        if args.is_empty() {
                            format!("({}).{}()", recv, method)
                        } else {
                            format!("({}).{}({})", recv, method, a(0))
                        }
                    }
                    "run" => format!("({}).run({}, {})", recv, a(0), a(1)),
                    _ => format!("({}).{}()", recv, method),
                },
                // D-WATCH-SCOPE1: unified watcher handle/set runtime.
                THandleOp::WatchMethod { method } => match method.as_str() {
                    "on" | "once" => format!("({}).{}(&({}), {})", recv, method, a(0), a(1)),
                    "add" => format!("({}).add({})", recv, a(0)),
                    "poll" | "events" | "summary" | "active" | "cancel" => {
                        if args.is_empty() {
                            format!("({}).{}()", recv, method)
                        } else {
                            format!("({}).{}({})", recv, method, a(0))
                        }
                    }
                    _ => format!("({}).{}()", recv, method),
                },
                // D-HONESTNUM1=A: Measurement<Float> arithmetic + accessors.
                THandleOp::MeasurementMethod { method } => {
                    if args.is_empty() {
                        format!("({}).{}()", recv, method)
                    } else {
                        format!("({}).{}({})", recv, method, a(0))
                    }
                }
                // D-LAYOUT1 / D-LAYOUT-GATES1: `LayoutHandle`/`Constraint`
                // methods — every Jet method name IS the Rust method name.
                THandleOp::LayoutMethod { method } => {
                    let joined = (0..args.len()).map(a).collect::<Vec<_>>().join(", ");
                    format!("({}).{}({})", recv, method, joined)
                }
                // D-PENDING1=B: Loadable<T,E> methods.
                THandleOp::LoadableMethod { method } => {
                    if args.is_empty() {
                        format!("({}).{}()", recv, method)
                    } else {
                        format!("({}).{}({})", recv, method, a(0))
                    }
                }
                // D-TTLVAL1=A: Expiring<T> methods.
                THandleOp::ExpiringMethod { method } => match method.as_str() {
                    "get" => format!(
                        "{}jet_expiring_get(&({}), {}jet_clock_now(&({})))",
                        root, recv, root, a(0)
                    ),
                    "is_valid" => format!(
                        "({}).is_valid({}jet_clock_now(&({})))",
                        recv, root, a(0)
                    ),
                    _ => format!("({}).{}()", recv, method),
                },
                // D-TTLVAL1=A: Rotting<T> methods (mutating get zeroizes).
                THandleOp::RottingMethod { method } => match method.as_str() {
                    "get" => format!(
                        "{}jet_rotting_get(&mut ({}), {}jet_clock_now(&({})))",
                        root, recv, root, a(0)
                    ),
                    "is_valid" => format!(
                        "({}).is_valid({}jet_clock_now(&({})))",
                        recv, root, a(0)
                    ),
                    _ => format!("({}).{}()", recv, method),
                },
                // D-RENDERTGT2=A (c133 M1): NullBackend measure/layout/paint/on_event/commands.
                THandleOp::UiBackendMethod { method } => match method.as_str() {
                    "measure" => format!(
                        "({}).measure_node(({}).clone(), ({}).clone())",
                        recv,
                        a(0),
                        a(1)
                    ),
                    "layout" => format!(
                        "({}).layout_node(({}).clone(), ({}).clone())",
                        recv,
                        a(0),
                        a(1)
                    ),
                    "paint" => format!("({}).paint_node(({}).clone())", recv, a(0)),
                    "on_event" => format!("({}).dispatch_event(({}).clone())", recv, a(0)),
                    "commands" => format!("({}).paint_commands()", recv),
                    "frame_lines" => format!("({}).frame_lines()", recv),
                    "render_count" => format!("({}).render_count()", recv),
                    // D-A11YGATE1=B (c134 Phase 6): keyboard focus routing.
                    "set_focus_group" => {
                        format!("({}).set_focus_group(({}).clone())", recv, a(0))
                    }
                    "focused_label" => format!("({}).focused_label()", recv),
                    // D-UIDEVSHELL1=A (c134 Phase 8): native GTK4 retained widgets.
                    "label" => format!("({}).label(&({}))", recv, a(0)),
                    "button" => format!("({}).button(&({}))", recv, a(0)),
                    "set_text" => format!("({}).set_text({}, &({}))", recv, a(0), a(1)),
                    "set_size" => format!("({}).set_size({}, {}, {})", recv, a(0), a(1), a(2)),
                    "set_color" => format!("({}).set_color({}, &({}))", recv, a(0), a(1)),
                    "on_click" => format!("({}).on_click({}, {})", recv, a(0), a(1)),
                    "present" => format!("({}).present(&({}))", recv, a(0)),
                    _ => format!("({}).{}()", recv, method),
                },
                // c-devserver (owner-directed 2026-07-01): DevServer builder
                // methods — the Rust method names match the Jet ones exactly.
                THandleOp::DevServerMethod { method } => match method.as_str() {
                    "html" => format!("({}).html(({}).clone())", recv, a(0)),
                    "port" => format!("({}).port({})", recv, a(0)),
                    "serve" => format!("({}).serve()", recv),
                    _ => format!("({}).{}()", recv, method),
                },
                // D-NETDEP1=A / D-HTTPLIB1=A: HTTP client method call.
                // "body"/"header" dispatch by arity: 0-arg=response accessor, 1-arg=request builder.
                THandleOp::HttpClientMethod { kind, method } => {
                    let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
                    if kind == "HttpClientReq" {
                        match method.as_str() {
                            "header" => format!(
                                "{}jet_http_client_request_header({}, &({}), &({}))",
                                root,
                                recv,
                                a(0),
                                a(1)
                            ),
                            "body" => format!(
                                "{}jet_http_client_request_body({}, &({}))",
                                root,
                                recv,
                                a(0)
                            ),
                            "timeout" => format!(
                                "{}jet_http_client_request_timeout({}, {})",
                                root,
                                recv,
                                a(0)
                            ),
                            "connect_timeout" => format!(
                                "{}jet_http_client_request_connect_timeout({}, {})",
                                root,
                                recv,
                                a(0)
                            ),
                            "read_timeout" => format!(
                                "{}jet_http_client_request_read_timeout({}, {})",
                                root,
                                recv,
                                a(0)
                            ),
                            "total_timeout" => format!(
                                "{}jet_http_client_request_total_timeout({}, {})",
                                root,
                                recv,
                                a(0)
                            ),
                            "redirects" => format!(
                                "{}jet_http_client_request_redirects({}, {})",
                                root,
                                recv,
                                a(0)
                            ),
                            "proxy" => format!(
                                "{}jet_http_client_request_proxy({}, &({}))",
                                root,
                                recv,
                                a(0)
                            ),
                            "cookie" => format!(
                                "{}jet_http_client_request_cookie({}, &({}), &({}))",
                                root,
                                recv,
                                a(0),
                                a(1)
                            ),
                            "form" => format!(
                                "{}jet_http_client_request_form({}, &({}), &({}))",
                                root,
                                recv,
                                a(0),
                                a(1)
                            ),
                            "multipart_text" => format!(
                                "{}jet_http_client_request_multipart_text({}, &({}), &({}))",
                                root,
                                recv,
                                a(0),
                                a(1)
                            ),
                            "send" => {
                                // call bridge with req fields; assemble JetHttpClientResp
                                format!(
                                    "{{ let _r = &({}); {}::jet_http_client_send_impl(&_r.method, &_r.url, &_r.headers, _r.body.as_deref(), _r.timeout_ms, _r.connect_timeout_ms, _r.read_timeout_ms, _r.total_timeout_ms, _r.redirects, _r.proxy.as_deref(), &_r.cookies, &_r.form, &_r.multipart).map(|(s,b,h)| JetHttpClientResp{{status:s,body:b,headers:h}}) }}",
                                    recv, ffi
                                )
                            }
                            _ => format!("({}).{}()", recv, method),
                        }
                    } else {
                        match method.as_str() {
                            "status" => {
                                format!("{}jet_http_client_response_status(&({}))", root, recv)
                            }
                            "body" => format!("{}jet_http_client_response_body(&({}))", root, recv),
                            "header" => format!(
                                "{}jet_http_client_response_header(&({}), &({}))",
                                root,
                                recv,
                                a(0)
                            ),
                            "cookies" => {
                                format!("{}jet_http_client_response_cookies(&({}))", root, recv)
                            }
                            _ => format!("({}).{}()", recv, method),
                        }
                    }
                }
                // D-NETDEP1=A / D-HTTPLIB1=A: HTTP server method call.
                THandleOp::HttpServerMethod { kind, method } => {
                    match (kind.as_str(), method.as_str()) {
                        ("HttpMux", "get" | "post" | "put" | "delete" | "patch") => {
                            format!(
                                "{{ {}jet_http_mux_add(&({}), \"{}\", &({}), {}) }}",
                                root,
                                recv,
                                method.to_uppercase(),
                                a(0),
                                a(1)
                            )
                        }
                        ("HttpSrvReq", "method") => {
                            format!("{}jet_http_srv_req_method(&({}))", root, recv)
                        }
                        ("HttpSrvReq", "path") => {
                            format!("{}jet_http_srv_req_path(&({}))", root, recv)
                        }
                        ("HttpSrvReq", "body") => {
                            format!("{}jet_http_srv_req_body(&({}))", root, recv)
                        }
                        ("HttpSrvReq", "param") => {
                            format!("{}jet_http_srv_req_param(&({}), &({}))", root, recv, a(0))
                        }
                        ("HttpSrvReq", "header") => {
                            format!("{}jet_http_srv_req_header(&({}), &({}))", root, recv, a(0))
                        }
                        ("HttpSrvReq", "body_len") => {
                            format!("{}jet_http_srv_req_body_len(&({}))", root, recv)
                        }
                        ("HttpSrvReq", "under_limit") => format!(
                            "{}jet_http_srv_req_under_limit(&({}), {})",
                            root,
                            recv,
                            a(0)
                        ),
                        ("HttpSrvResp", "header") => format!(
                            "{}jet_http_srv_response_header({}, &({}), &({}))",
                            root,
                            recv,
                            a(0),
                            a(1)
                        ),
                        _ => {
                            if args.is_empty() {
                                format!("({}).{}()", recv, method)
                            } else {
                                format!("({}).{}({})", recv, method, a(0))
                            }
                        }
                    }
                }
                // D-TIMEDEPTH1=A: civil-time method call.
                THandleOp::CivilTimeMethod { kind: _, method } => match method.as_str() {
                    "add_days" => format!("({}).add_days({})", recv, a(0)),
                    "add_months" => format!("({}).add_months({})", recv, a(0)),
                    "add_period" => format!("({}).add_period(&({}))", recv, a(0)),
                    "add_duration" => {
                        format!("{}jet_zoned_add_duration(&({}), &({}))", root, recv, a(0))
                    }
                    "diff_days" => format!("({}).diff_days(&({}))", recv, a(0)),
                    "plus_duration" => {
                        format!("{}jet_datetime_plus_duration(&({}), &({}))", root, recv, a(0))
                    }
                    "in_zone" => format!("({}).in_zone(&({}))", recv, a(0)),
                    "truncate" | "round" => {
                        format!("({}).{}(&({}))", recv, method, a(0))
                    }
                    "format" => format!("({}).format_pattern(&({}))", recv, a(0)),
                    "to_string" => format!("({}).to_string_fmt()", recv),
                    _ => {
                        if args.is_empty() {
                            format!("({}).{}()", recv, method)
                        } else {
                            format!("({}).{}({})", recv, method, a(0))
                        }
                    }
                },
                // D-URL1=A: typed Url/Mime methods.
                THandleOp::UrlMimeMethod { kind: _, method } => match method.as_str() {
                    "join" | "param" => format!("({}).{}(&({}))", recv, method, a(0)),
                    "set_query" | "add_query" => {
                        format!("({}).{}(&({}), &({}))", recv, method, a(0), a(1))
                    }
                    "to_string" => format!("({}).to_string_value()", recv),
                    _ => {
                        if args.is_empty() {
                            format!("({}).{}()", recv, method)
                        } else {
                            format!("({}).{}({})", recv, method, a(0))
                        }
                    }
                },
                THandleOp::RegexMethod { kind: _, method } => match method.as_str() {
                    "match" => format!("({}).match_value(&({}))", recv, a(0)),
                    "is_match" | "find" | "find_all" | "matches" | "split" | "name" => {
                        format!("({}).{}(&({}))", recv, method, a(0))
                    }
                    "replace" | "replace_all" | "split_limit" => {
                        format!("({}).{}(&({}), {})", recv, method, a(0), a(1))
                    }
                    "replace_all_with" => {
                        format!("({}).replace_all_with(&({}), {})", recv, a(0), a(1))
                    }
                    "group" | "group_start" | "group_end" => {
                        format!("({}).{}({})", recv, method, a(0))
                    }
                    "start" | "end" => format!("({}).{}()", recv, method),
                    _ => {
                        if args.is_empty() {
                            format!("({}).{}()", recv, method)
                        } else {
                            format!("({}).{}({})", recv, method, a(0))
                        }
                    }
                },
                // D-APPROX1=A: sketch method call. `add` args may be string borrows;
                // `count`/`quantile` pass by value; `sample` returns Vec<String>.
                THandleOp::SketchMethod { sketch, method } => {
                    match method.as_str() {
                        "add" if sketch == "TDigest" => format!("({}).add({})", recv, a(0)),
                        "add" if sketch == "ReservoirSampler" => {
                            format!("({}).add(({}).clone())", recv, a(0))
                        }
                        "add" => format!("({}).add(&({}))", recv, a(0)),
                        // HLL.count() and CMS.count(key) — different arities.
                        "count" if args.is_empty() => format!("({}).count()", recv),
                        "count" => format!("({}).count(&({}))", recv, a(0)),
                        _ => {
                            if args.is_empty() {
                                format!("({}).{}()", recv, method)
                            } else {
                                format!("({}).{}({})", recv, method, a(0))
                            }
                        }
                    }
                }
                // c109 Phase 25: HttpRouter route registration, byte-for-byte the
                // `emit_builtin_method` router arm (Source/Codegen/Expression.rs ~L937).
                // `recv` is `&mut`-borrowed; the path is plain (args[0]); the handler is
                // the pre-rendered boxed closure.
                THandleOp::HttpRouterRegister {
                    verb,
                    handler,
                    file,
                    line,
                } => format!(
                    "{}jet_http_router_register(&mut ({}), \"{}\".to_string(), {}, {}, {:?}, {})",
                    root,
                    recv,
                    verb,
                    a(0),
                    handler,
                    file,
                    line
                ),
                // D-SIMD2 / D-LINALG1: a math-type instance method → the prelude free
                // function `jet_math_<type>_<method>(&(recv), <args>)`. `reduce`
                // dispatches on the validated marker op. All take `&recv` (immutable;
                // these types are value semantics — every op returns a fresh value).
                THandleOp::MathMethod {
                    type_name,
                    method,
                    reduce_op,
                } => {
                    let fname = match reduce_op {
                        Some(op) => format!("jet_math_{}_reduce_{}", type_name, op.to_lowercase()),
                        None => format!("jet_math_{}_{}", type_name, method),
                    };
                    let mut call = format!("{}{}(&({})", root, fname, recv);
                    for i in 0..args.len() {
                        call.push_str(&format!(", {}", a(i)));
                    }
                    call.push(')');
                    call
                }
                // D-SERDE-ACCESS=B: DataTree accessor methods.
                THandleOp::DataTreeField => format!("({}).field(&({}))", recv, a(0)),
                THandleOp::DataTreeAt => format!("({}).at({})", recv, a(0)),
                THandleOp::DataTreeInt => format!("({}).int()", recv),
                THandleOp::DataTreeText => format!("({}).text()", recv),
                THandleOp::DataTreeBool => format!("({}).bool()", recv),
                THandleOp::DataTreeFloat => format!("({}).float()", recv),
                // D-SERDE-ACCESS=B: same accessors on Json/Data.
                THandleOp::JsonField => format!("({}).field(&({}))", recv, a(0)),
                THandleOp::JsonAt => format!("({}).at({})", recv, a(0)),
                THandleOp::JsonInt => format!("({}).int()", recv),
                THandleOp::JsonText => format!("({}).text()", recv),
                THandleOp::JsonBool => format!("({}).bool()", recv),
                THandleOp::JsonFloat => format!("({}).float()", recv),
                // D-PATHFS1: Path object methods.
                THandleOp::PathFrom => format!("{}jet_path_from(&({}))", root, recv),
                THandleOp::PathJoin => format!("{}jet_path_join(&({}), &({}))", root, recv, a(0)),
                THandleOp::PathParent => format!("{}jet_path_parent(&({}))", root, recv),
                THandleOp::PathExtension => format!("{}jet_path_extension(&({}))", root, recv),
                THandleOp::PathStem => format!("{}jet_path_stem(&({}))", root, recv),
                THandleOp::PathToString => format!("({}).jet_show()", recv),
                THandleOp::PathWriteAtomic => {
                    format!("{}jet_path_write_atomic(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::PathWalk => format!("{}jet_path_walk(&({}))", root, recv),
                // D-SHIFT1 (c7shift): `binary.Reader` / `text.Cursor`. Every read is
                // fallible (`Result<T, String>`) — a bounds/match miss is an ordinary
                // `Err`, never a panic (I1/L2).
                THandleOp::ReaderOver => format!("{}jet_reader_over(&({}))", root, recv),
                THandleOp::ReaderReadU8 => format!("{}jet_reader_read_u8(&mut ({}))", root, recv),
                THandleOp::ReaderReadU16Le => {
                    format!("{}jet_reader_read_u16_le(&mut ({}))", root, recv)
                }
                THandleOp::ReaderReadU16Be => {
                    format!("{}jet_reader_read_u16_be(&mut ({}))", root, recv)
                }
                THandleOp::ReaderReadU32Le => {
                    format!("{}jet_reader_read_u32_le(&mut ({}))", root, recv)
                }
                THandleOp::ReaderReadU32Be => {
                    format!("{}jet_reader_read_u32_be(&mut ({}))", root, recv)
                }
                THandleOp::ReaderReadU64Le => {
                    format!("{}jet_reader_read_u64_le(&mut ({}))", root, recv)
                }
                THandleOp::ReaderReadU64Be => {
                    format!("{}jet_reader_read_u64_be(&mut ({}))", root, recv)
                }
                THandleOp::ReaderTake => {
                    format!("{}jet_reader_take(&mut ({}), {})", root, recv, a(0))
                }
                THandleOp::ReaderRemaining => format!("{}jet_reader_remaining(&({}))", root, recv),
                THandleOp::ReaderAtEnd => format!("{}jet_reader_at_end(&({}))", root, recv),
                THandleOp::CursorOver => format!("{}jet_cursor_over(&({}))", root, recv),
                THandleOp::CursorTakeUntil => {
                    format!("{}jet_cursor_take_until(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::CursorSkipWs => format!("{}jet_cursor_skip_ws(&mut ({}))", root, recv),
                // D-SHIFT1: `cursor.take_pattern("…")` — inline scan (I8: the
                // D-PARSESTR1 engine in consume mode, `str_match_scan_closure_ex`),
                // built entirely here since it needs `recv`'s already-emitted
                // Rust text (unlike the other `THandleOp` arms, which only
                // format-string it — this one embeds it in a bigger block).
                THandleOp::CursorTakePattern { parts, canonical } => {
                    let (closure, holes) =
                        str_match_scan_closure_ex(parts, cx, "__jet_tail", false);
                    let mut bind_vars: Vec<String> = holes
                        .iter()
                        .map(|(n, _)| format!("__jet_sm_{}", mangle(n)))
                        .collect();
                    bind_vars.push("__jet_consumed".to_string());
                    let bind_pat = tuple_join(&bind_vars);
                    let ok_val = if canonical.is_empty() {
                        "()".to_string()
                    } else {
                        let struct_name = crate::Codegen::Tuples::tuple_struct_name(canonical);
                        let field_inits: Vec<String> = canonical
                            .iter()
                            .zip(holes.iter())
                            .map(|((n, _), (hn, _))| {
                                format!("{}: __jet_sm_{}", mangle(n), mangle(hn))
                            })
                            .collect();
                        format!("{} {{ {} }}", struct_name, field_inits.join(", "))
                    };
                    format!(
                        "{{ let __jet_cur = &mut ({recv}); let __jet_tail: &str = &__jet_cur.buf[__jet_cur.pos..]; match {closure} {{ Some(({bind_pat})) => {{ __jet_cur.pos += __jet_consumed; Ok({ok_val}) }}, None => Err(format!(\"pattern did not match at cursor position {{}}\", __jet_cur.pos)) }} }}",
                        recv = recv,
                        closure = closure,
                        bind_pat = bind_pat,
                        ok_val = ok_val,
                    )
                }
                // D-DBDRIVER1: `DbConnection` instance methods. `query`/`query_one`/
                // `execute` cross the FFI bridge boundary as plain wire text (params
                // encoded, rows/count/error decoded) — see `Source/Prelude/Db.rs` and
                // `jet_std::jet_db_{encode_params,decode_query_result,decode_execute_result}`
                // in `Source/Prelude/CoreLib.rs`.
                THandleOp::DbQuery => format!(
                    "{root}jet_std::jet_db_decode_query_result(&{ffi}::jet_db_query(({recv}).handle, &({}), &{root}jet_std::jet_db_encode_params(&({}))))",
                    a(0),
                    a(1)
                ),
                THandleOp::DbQueryOne => format!(
                    "{root}jet_std::jet_db_decode_query_result(&{ffi}::jet_db_query(({recv}).handle, &({}), &{root}jet_std::jet_db_encode_params(&({})))).map(|__rows| __rows.into_iter().next())",
                    a(0),
                    a(1)
                ),
                THandleOp::DbExecute => format!(
                    "{root}jet_std::jet_db_decode_execute_result(&{ffi}::jet_db_execute(({recv}).handle, &({}), &{root}jet_std::jet_db_encode_params(&({}))))",
                    a(0),
                    a(1)
                ),
                THandleOp::DbBegin => format!("{ffi}::jet_db_begin(({recv}).handle)"),
                THandleOp::DbCommit => format!("{ffi}::jet_db_commit(({recv}).handle)"),
                THandleOp::DbRollback => format!("{ffi}::jet_db_rollback(({recv}).handle)"),
                THandleOp::DbClose => format!("{ffi}::jet_db_close(({recv}).handle)"),
                // D-DBDRIVER1: `DbValue` accessors — plain inherent Rust methods on
                // the always-compiled `jet_std::DbValue` enum (no FFI bridge involved).
                THandleOp::DbValueInt => format!("({}).int()", recv),
                THandleOp::DbValueFloat => format!("({}).float()", recv),
                THandleOp::DbValueText => format!("({}).text()", recv),
                THandleOp::DbValueBool => format!("({}).bool()", recv),
                THandleOp::DbValueIsNull => format!("({}).is_null()", recv),
                // D-DEP-WASM1=A / D-PLUGIN1=B (c81): `Plugin.call`/`.call_int` —
                // a homogeneous scalar call across the sandboxed Component
                // Model boundary, wire-encoded exactly like `DbQuery` above
                // (args encoded, result decoded; see `Prelude/Plugin.rs` and
                // `jet_std::jet_plugin_{encode_args_float,decode_result_float}`
                // in `Prelude/CoreLib.rs`).
                THandleOp::PluginCall => format!(
                    "{root}jet_std::jet_plugin_decode_result_float(&{ffi}::jet_plugin_call(({recv}).handle, &({}), &{root}jet_std::jet_plugin_encode_args_float(&({}))))",
                    a(0),
                    a(1)
                ),
                THandleOp::PluginCallInt => format!(
                    "{root}jet_std::jet_plugin_decode_result_int(&{ffi}::jet_plugin_call(({recv}).handle, &({}), &{root}jet_std::jet_plugin_encode_args_int(&({}))))",
                    a(0),
                    a(1)
                ),
            }
        }
        // c109 Phase 13: a closure-taking core call. The closure was rendered at
        // lowering; emit assembles the bespoke shape, byte-for-byte `emit_core_call`
        // (Source/Codegen/Expression.rs).
        TExprKind::CoreClosureCall { kind } => match kind {
            TCoreClosureKind::Spawn { spawn_closure } => {
                format!(
                    "{}jet_std::JetTask::spawn({})",
                    cx.root_prefix, spawn_closure
                )
            }
            TCoreClosureKind::Serve { addr, closure } => format!(
                "{}jet_http_serve(&({}), {})",
                cx.root_prefix,
                emit_tir_expr(addr, cx),
                closure
            ),
            TCoreClosureKind::Guard { closure } => {
                format!("{}jet_scope_guard({})", cx.root_prefix, closure)
            }
            // D-TXN3: register a post-commit hook on the transaction handle. Boxed so
            // hooks of differing closure types share one queue; run LIFO in Drop, but
            // only after `commit()` (the `JetTransaction` prelude type).
            TCoreClosureKind::OnCommit { handle, closure } => {
                format!("{}.on_commit(Box::new({}))", handle, closure)
            }
            // D-TXN-ROLLBACK (layer 3): the rollback-hook registration, run LIFO on a
            // `?`-failure and dropped un-run on commit (the `JetTransaction` prelude type).
            TCoreClosureKind::OnRollback { handle, closure } => {
                format!("{}.on_rollback(Box::new({}))", handle, closure)
            }
            // D-REACT1=B: a derived value recomputed from its signals.
            TCoreClosureKind::ReactiveDerived { closure } => {
                format!("{}jet_std::JetDerived::new({})", cx.root_prefix, closure)
            }
            // D-REACT1=B: an effect re-run when a signal it read changes.
            TCoreClosureKind::ReactiveEffect { closure } => {
                format!(
                    "{}jet_std::jet_reactive_effect({})",
                    cx.root_prefix, closure
                )
            }
            TCoreClosureKind::UiReactiveRender { closure } => {
                format!("{}jet_ui_reactive_render({})", cx.root_prefix, closure)
            }
        },
        // D-TASKSCOPE1=A: `g.all([h1, h2, …])` — join each handle in list order.
        TExprKind::TaskGroupAll { tasks } => {
            let list = emit_tir_expr(tasks, cx);
            format!("{}jet_std::jet_task_all({list})", cx.root_prefix)
        }
        TExprKind::TaskGroupRace { tasks } => {
            let list = emit_tir_expr(tasks, cx);
            format!("{}jet_std::jet_task_race({list})", cx.root_prefix)
        }
        TExprKind::TaskGroupAny { tasks } => {
            let list = emit_tir_expr(tasks, cx);
            format!("{}jet_std::jet_task_any({list})", cx.root_prefix)
        }
        TExprKind::SelectStart => {
            format!("{}jet_std::JetSelectBuilder::start()", cx.root_prefix)
        }
        TExprKind::SelectRecv { builder, channel } => {
            let b = emit_tir_expr(builder, cx);
            let ch = emit_tir_expr(channel, cx);
            format!("{b}.recv({ch})")
        }
        TExprKind::SelectAfter {
            builder,
            millis,
            value,
        } => {
            let b = emit_tir_expr(builder, cx);
            let ms = emit_tir_expr(millis, cx);
            if let Some(value) = value {
                let v = emit_tir_expr(value, cx);
                format!("{b}.after_value({ms}, {v})")
            } else {
                format!("{b}.after({ms})")
            }
        }
        TExprKind::SelectRead { builder, stream } => {
            let b = emit_tir_expr(builder, cx);
            let s = emit_tir_expr(stream, cx);
            format!("{b}.read({s})")
        }
        TExprKind::SelectWait { builder } => {
            let (recvs, afters) = collect_select_arms(builder, cx);
            let recv_list = if recvs.is_empty() {
                "&[]".to_string()
            } else {
                format!("&[&{}]", recvs.join(", &"))
            };
            let after_list = if afters.is_empty() {
                "Vec::new()".to_string()
            } else {
                format!("vec![{}]", afters.join(", "))
            };
            format!(
                "{}jet_std::jet_select_wait({}, {})",
                cx.root_prefix, recv_list, after_list
            )
        }
        // c109 Phase 13: a fn-typed value. A bare fn-name value echoes the
        // already-rendered `Box::new(move |…| …) as <fn-type>` wrapper; a call through
        // a fn-value emits `({callee})({args})`, byte-for-byte `emit_expr`'s
        // `Expr::CallValue` (Source/Codegen/Expression.rs).
        TExprKind::FnValue { kind } => match kind {
            TFnValueKind::NamedFn { wrapper } => wrapper.clone(),
            TFnValueKind::Call { callee, args } => {
                format!(
                    "({})({})",
                    emit_tir_expr(callee, cx),
                    emit_tir_call_args(args, cx)
                )
            }
        },
        // c109 Phase 14: a cross-module call. The path form was resolved at lowering;
        // emit prepends `cx.root_prefix` exactly where the AST path does (both the
        // qualified `{root}{mod}::{fn}` form and the inline `{root}user_{mangled}` form
        // prefix with root). Args were resolved into `TCallArg`s (`emit_tir_call_args`).
        TExprKind::ModuleCall { form, args } => {
            let arg_str = emit_tir_call_args(args, cx);
            match form {
                TModuleCallForm::Qualified { rust_mod, rust_fn } => {
                    format!("{}{}::{}({})", cx.root_prefix, rust_mod, rust_fn, arg_str)
                }
                TModuleCallForm::InlineMangled { mangled } => {
                    format!("{}user_{}({})", cx.root_prefix, mangled, arg_str)
                }
            }
        }
        // c109 Phase 14: an FFI extern call. Reproduces `emit_call`'s `extern_funcs`
        // arm: `{ffi_crate}::{wrapper}(args)`. `cx.ffi_crate` is program-level (read
        // here, like Phase 10's regex form); the AST falls back to "jet_ffi" when it is
        // `None` (always `Some` when an extern call is present, but mirror it exactly).
        // Args use the extern arg form (`(…).clone()` for a non-scalar Read).
        TExprKind::ExternCall { wrapper, args } => {
            let crate_name = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
            let arg_str = args
                .iter()
                .map(|a| {
                    let s = emit_tir_expr(&a.value, cx);
                    if a.clone {
                        format!("({}).clone()", s)
                    } else {
                        s
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}::{}({})", crate_name, wrapper, arg_str)
        }
    }
}

/// c109 Phase 8/15: format a `??` fallback right-hand side, mirroring
/// `emit_or_fallback_rhs` (Statement.rs). Value and early-`return` (Phase 8); the
/// `panic(…)` form (Phase 15) carries its fully-rendered statement string from lowering.
pub(crate) fn emit_tir_orfallback_rhs(fallback: &TOrFallback, cx: &Cx) -> String {
    match fallback {
        TOrFallback::Value(e) => emit_tir_expr(e, cx),
        TOrFallback::Return(None) => "return".to_string(),
        TOrFallback::Return(Some(e)) => format!("return {}", emit_tir_expr(e, cx)),
        TOrFallback::Panic(rendered) => rendered.clone(),
        TOrFallback::Break => "break".to_string(),
        TOrFallback::Continue => "continue".to_string(),
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
/// The Rust type a typed `decode<T>` constructs — `T` for json/toml/yaml, the element
/// `T` for CSV's `[T]`. Read from the resolved `Result<…, DecodeError>` return type.
pub(crate) fn enc_target_rust(ret_ty: &Type, cx: &Cx) -> String {
    if let Type::Result { ok, .. } = ret_ty {
        match &**ok {
            Type::List(elem) => cx.rust_type(elem),
            other => cx.rust_type(other),
        }
    } else {
        cx.rust_type(ret_ty)
    }
}
/// D-MIGRATE3=A: the Rust type a typed `decode_traced<T>` constructs — same
/// target as [`enc_target_rust`], one layer deeper through the resolved
/// `Result<DecodeResult<T | [T]>, DecodeError>` return type.
pub(crate) fn enc_target_rust_traced(ret_ty: &Type, cx: &Cx) -> String {
    if let Type::Result { ok, .. } = ret_ty {
        if let Type::Apply { args, .. } = &**ok {
            if let Some(inner) = args.first() {
                return match inner {
                    Type::List(elem) => cx.rust_type(elem),
                    other => cx.rust_type(other),
                };
            }
        }
    }
    cx.rust_type(ret_ty)
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
pub(crate) fn emit_tir_core_call(
    module: &str,
    method: &str,
    args: &[TExpr],
    ret_ty: &Type,
    cx: &Cx,
) -> String {
    let arg = |i: usize| {
        args.get(i)
            .map(|e| emit_tir_expr(e, cx))
            .unwrap_or_default()
    };
    let helper = |name: &str| format!("{}{}", cx.root_prefix, name);
    let regex_fn = |name: &str| {
        let crate_name = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
        format!("{}::{}", crate_name, name)
    };
    let normalized_module =
        crate::Syntax::normalize_core_module(module).unwrap_or_else(|| module.to_string());
    let module = normalized_module.as_str();
    match (module, method) {
        // c109 Phase 18 (S58, E2-M13): low-level pointer ops, byte-for-byte
        // `emit_core_call`. `address_of` is an inert address cast (no `unsafe`);
        // `volatile_read`/`volatile_write` access through a `Ptr<T>` — the volatile ops are
        // valid because the call only reaches codegen inside an `#Unsafe` region/fn (sema
        // E3101), already lowered to a Rust `unsafe` context.
        ("core.mem", "address_of") => format!("(&({}) as *const _ as usize as i64)", arg(0)),
        ("core.mem", "volatile_read") => format!("std::ptr::read_volatile({})", arg(0)),
        ("core.mem", "volatile_write") => {
            format!("std::ptr::write_volatile({}, {})", arg(0), arg(1))
        }
        // D-TUPLE-DESTRUCT1: the `tasks.channel<T>()` producer — returns the
        // `(Sender<T>, Receiver<T>)` pair as the same `JetTup_<hash>` named-tuple
        // struct every other `Type::Tuple` value uses (`enumerate`/`zip`/`partition`'s
        // convention — `Tuples::collect_tuple_shapes` already walks this call's
        // `resolved_ret` and declares the struct). `T` and the struct shape both come
        // from the call node's own resolved `ret_ty`, not a binding annotation
        // (there's no combined "Channel" value left to annotate).
        ("core.tasks", "channel") => {
            let fields = match ret_ty {
                Type::Tuple(fields) => crate::Codegen::Tuples::tuple_fields_plain(fields),
                _ => Vec::new(),
            };
            let elem = fields
                .first()
                .and_then(|(_, t)| match t {
                    Type::Apply { args, .. } => args.first().cloned(),
                    _ => None,
                })
                .unwrap_or(Type::Int);
            let struct_name = crate::Codegen::Tuples::tuple_struct_name(&fields);
            let ctor = if args.is_empty() {
                format!(
                    "{}jet_std::channel::<{}>()",
                    cx.root_prefix,
                    cx.rust_type(&elem)
                )
            } else {
                format!(
                    "{}jet_std::channel_bounded::<{}>({})",
                    cx.root_prefix,
                    cx.rust_type(&elem),
                    arg(0)
                )
            };
            format!(
                "{{ let __jet_ch = {}; {} {{ {}: __jet_ch.0, {}: __jet_ch.1 }} }}",
                ctor,
                struct_name,
                mangle("sender"),
                mangle("receiver"),
            )
        }
        ("core.tasks", "after") => {
            if args.len() == 1 {
                format!("{}jet_std::after({})", cx.root_prefix, arg(0))
            } else {
                format!(
                    "{}jet_std::after_value({}, {})",
                    cx.root_prefix,
                    arg(0),
                    arg(1)
                )
            }
        }
        ("core.tasks", "interval") => format!("{}jet_std::interval({})", cx.root_prefix, arg(0)),
        // D-REACT1=B: `reactive.signal(initial)` producer → a `JetSignal<T>`.
        ("jet.reactive", "signal") => {
            format!("{}jet_std::JetSignal::new({})", cx.root_prefix, arg(0))
        }
        // D-EVENT1=D: first-party typed Event/Hook constructors.
        ("core.event", "scope") => format!("{}jet_std::JetEventScope::new()", cx.root_prefix),
        ("core.event", "policy_sync") => {
            format!("{}jet_std::JetEventPolicy::sync()", cx.root_prefix)
        }
        ("core.event", "policy_async") => {
            format!(
                "{}jet_std::JetEventPolicy::async_buffered({})",
                cx.root_prefix,
                arg(0)
            )
        }
        ("core.event", "new") => {
            let elem = match ret_ty {
                Type::Apply { args, .. } => args.first().cloned().unwrap_or(Type::Int),
                _ => Type::Int,
            };
            format!(
                "{}jet_std::JetEvent::<{}>::new()",
                cx.root_prefix,
                cx.rust_type(&elem)
            )
        }
        ("core.event", "with_policy") => {
            let elem = match ret_ty {
                Type::Apply { args, .. } => args.first().cloned().unwrap_or(Type::Int),
                _ => Type::Int,
            };
            format!(
                "{}jet_std::JetEvent::<{}>::with_policy({})",
                cx.root_prefix,
                cx.rust_type(&elem),
                arg(0)
            )
        }
        ("core.event", "hook") => {
            let (payload, result) = match ret_ty {
                Type::Apply { args, .. } if args.len() >= 2 => (args[0].clone(), args[1].clone()),
                _ => (Type::Int, Type::Int),
            };
            format!(
                "{}jet_std::JetHook::<{}, {}>::new({})",
                cx.root_prefix,
                cx.rust_type(&payload),
                cx.rust_type(&result),
                arg(0)
            )
        }
        // D-HONESTNUM1=A: `M.from(value, uncertainty)` → a `JetMeasurement<f64>`.
        ("core.science.measurement", "from") => {
            format!(
                "{}jet_std::JetMeasurement::new({}, {})",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        }
        // D-DECIMAL1: `core.numeric.decimal(s)` → exact parse.
        ("core.numeric", "decimal") => {
            format!("{}jet_decimal_from_str(&({}))", cx.root_prefix, arg(0))
        }
        // D-PENDING1=B: Loadable<T,E> constructors.
        // idle/loading/loaded/failed need concrete type params for E: Clone bound satisfaction.
        ("core.async.loadable", "idle") => format!("JetLoadable::<(), ()>::Idle"),
        ("core.async.loadable", "loading") => format!("JetLoadable::<(), ()>::Loading"),
        ("core.async.loadable", "loaded") => {
            format!("JetLoadable::<_, ()>::Loaded({})", arg(0))
        }
        ("core.async.loadable", "failed") => {
            format!("JetLoadable::<(), _>::Failed({})", arg(0))
        }
        // D-FILES-WRITE1 (merge, was `core.fs`): whole-file convenience helpers now
        // live in `core.files` alongside the streaming handle constructors below.
        // D-FILES-APPEND1=A: whole-file one-shot is `append_all`, not `append` —
        // that name stays reserved for the streaming handle's `.append(text)`.
        ("core.files", "read") => format!("{}(&({}))", helper("jet_std_fs_read"), arg(0)),
        ("core.files", "read_bytes") => {
            format!("{}(&({}))", helper("jet_std_fs_read_bytes"), arg(0))
        }
        ("core.files", "write") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_write"),
            arg(0),
            arg(1)
        ),
        ("core.files", "append_all") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_append"),
            arg(0),
            arg(1)
        ),
        ("core.files", "exists") => format!("{}(&({}))", helper("jet_std_fs_exists"), arg(0)),
        ("core.files", "remove") => format!("{}(&({}))", helper("jet_std_fs_remove"), arg(0)),
        ("core.files", "remove_dir") => {
            format!("{}(&({}))", helper("jet_std_fs_remove_dir"), arg(0))
        }
        ("core.files", "remove_all") => {
            format!("{}(&({}))", helper("jet_std_fs_remove_all"), arg(0))
        }
        ("core.files", "list_dir") => format!("{}(&({}))", helper("jet_std_fs_list_dir"), arg(0)),
        ("core.files", "create_dir") => {
            format!("{}(&({}))", helper("jet_std_fs_create_dir"), arg(0))
        }
        ("core.files", "create_dir_all") => {
            format!("{}(&({}))", helper("jet_std_fs_create_dir_all"), arg(0))
        }
        ("core.files", "is_dir") => format!("{}(&({}))", helper("jet_std_fs_is_dir"), arg(0)),
        ("core.files", "copy") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_copy"),
            arg(0),
            arg(1)
        ),
        ("core.files", "copy_dir") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_copy_dir"),
            arg(0),
            arg(1)
        ),
        ("core.files", "rename") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_rename"),
            arg(0),
            arg(1)
        ),
        ("core.files", "symlink") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_symlink"),
            arg(0),
            arg(1)
        ),
        ("core.files", "read_link") => {
            format!("{}(&({}))", helper("jet_std_fs_read_link"), arg(0))
        }
        ("core.files", "hard_link") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_hard_link"),
            arg(0),
            arg(1)
        ),
        ("core.files", "stat") => format!("{}(&({}))", helper("jet_std_fs_stat"), arg(0)),
        ("core.files", "canonicalize") => {
            format!("{}(&({}))", helper("jet_std_fs_canonicalize"), arg(0))
        }
        ("core.files", "absolute") => {
            format!("{}(&({}))", helper("jet_std_fs_absolute"), arg(0))
        }
        ("core.files", "walk") => format!("{}(&({}))", helper("jet_std_fs_walk"), arg(0)),
        ("core.files", "glob") => format!("{}(&({}))", helper("jet_std_fs_glob"), arg(0)),
        ("core.files", "read_at") => format!(
            "{}(&({}), {}, {})",
            helper("jet_std_fs_read_at"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.files", "write_at") => format!(
            "{}(&({}), {}, &({}))",
            helper("jet_std_fs_write_at"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.files", "fsync") => format!("{}(&({}))", helper("jet_std_fs_fsync"), arg(0)),
        ("core.files", "write_atomic") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_write_atomic"),
            arg(0),
            arg(1)
        ),
        ("core.files", "temp_dir") => {
            format!("{}(&({}))", helper("jet_std_fs_temp_dir"), arg(0))
        }
        ("core.files", "temp_file") => {
            format!("{}(&({}))", helper("jet_std_fs_temp_file"), arg(0))
        }
        ("core.files", "lock") => format!("{}(&({}))", helper("jet_std_fs_lock"), arg(0)),
        ("core.watcher", "files") => format!("{}(&({}))", helper("jet_watcher_files"), arg(0)),
        ("core.watcher", "process_pid") => {
            format!("{}({})", helper("jet_watcher_process_pid"), arg(0))
        }
        ("core.watcher", "port") => {
            format!("{}(&({}), {})", helper("jet_watcher_port"), arg(0), arg(1))
        }
        ("core.watcher", "set") => format!("{}()", helper("jet_watcher_set")),
        ("core.io", "args") => format!("{}()", helper("jet_std_io_args")),
        // D-ARGS1: `args.spec()` → empty builder.
        ("core.args", "spec") => format!("{}()", helper("jet_args_spec")),
        // D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)` — built entirely at this call
        // site (no generic runtime trait needed, I3: sema already gated
        // legality via `is_displayable` in `CheckerCoreLib::infer_core_call`,
        // the SAME check `"{x}"` interpolation uses). `x` is bound once
        // (`__reflect_v`) so a side-effecting argument expression isn't
        // evaluated twice. `.display()` calls `jet_display()` (JetDisplay) —
        // never `jet_show()`/`{:?}` — so it shows exactly what `"{x}"` would,
        // never codegen's mangled Rust field names. `.fields()`'s per-field
        // values use `jet_show()` (universal — every type has it, primitives
        // included, so this never needs its own displayability check) and are
        // populated only when the arg's resolved sema type is a known user
        // struct (`cx.struct_fields`); every other shape (primitives, enums,
        // tuples, lists) gets an empty list, never a guess.
        ("core.reflect", "of") => {
            let arg_ty = args.first().map(|a| &a.ty);
            let type_name = arg_ty.map(|t| t.name()).unwrap_or_default();
            let fields_code = match arg_ty {
                Some(Type::Named(struct_name)) => match cx.struct_fields.get(struct_name) {
                    Some(fields) if !fields.is_empty() => {
                        let items: Vec<String> = fields
                            .iter()
                            .map(|(fname, _)| {
                                format!(
                                    "{root}JetReflectField {{ name: \"{fname}\".to_string(), value: (__reflect_v.{mangled}).jet_show() }}",
                                    root = cx.root_prefix,
                                    fname = fname,
                                    mangled = mangle(fname)
                                )
                            })
                            .collect();
                        format!("vec![{}]", items.join(", "))
                    }
                    _ => "Vec::new()".to_string(),
                },
                _ => "Vec::new()".to_string(),
            };
            format!(
                "{{ let __reflect_v = &({arg0}); {root}JetReflectValue {{ type_name: \"{type_name}\".to_string(), display: __reflect_v.jet_display(), fields: {fields_code} }} }}",
                arg0 = arg(0),
                root = cx.root_prefix,
                type_name = type_name,
                fields_code = fields_code
            )
        }
        // c109 Phase 29: qualified `io.input(prompt)`, byte-for-byte `emit_core_call`
        // (Expression.rs ~L1294): no arg → `jet_std_io_input(None)`; a prompt arg →
        // `jet_std_io_input(Some(&(prompt)))`. Same emitted helper as the ambient bare
        // `input(...)` (Phase 25), the only difference being the source node shape.
        ("core.io", "input") => {
            if args.is_empty() {
                format!("{}(None)", helper("jet_std_io_input"))
            } else {
                format!("{}(Some(&({})))", helper("jet_std_io_input"), arg(0))
            }
        }
        ("core.io", "read_all_input") => format!("{}()", helper("jet_std_io_read_all_input")),
        // D-STDIN1=A: io.stdin() → JetStdinReader handle.
        ("core.io", "stdin") => format!("{}()", helper("jet_std_io_stdin")),
        ("core.io", "stdout") => format!("{}()", helper("jet_std_io_stdout")),
        ("core.io", "stderr") => format!("{}()", helper("jet_std_io_stderr")),
        ("core.io", "terminal_width") => format!("{}()", helper("jet_std_io_terminal_width")),
        ("core.io", "terminal_height") => format!("{}()", helper("jet_std_io_terminal_height")),
        ("core.io", "style") => {
            format!(
                "{}(&({}), &({}))",
                helper("jet_std_io_style"),
                arg(0),
                arg(1)
            )
        }
        ("core.io", "style_force") => {
            format!(
                "{}(&({}), &({}))",
                helper("jet_std_io_style_force"),
                arg(0),
                arg(1)
            )
        }
        ("core.io", "progress") => {
            format!("{}(&({}))", helper("jet_std_io_progress"), arg(0))
        }
        ("core.env", "get") => format!("{}(&({}))", helper("jet_std_env_get"), arg(0)),
        ("core.env", "set") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_env_set"),
            arg(0),
            arg(1)
        ),
        ("core.env", "current_dir") => format!("{}()", helper("jet_std_env_current_dir")),
        ("core.env", "home_dir") => format!("{}()", helper("jet_std_env_home_dir")),
        ("core.os", "name") => format!("{}()", helper("jet_std_os_name")),
        ("core.os", "family") => format!("{}()", helper("jet_std_os_family")),
        ("core.os", "arch") => format!("{}()", helper("jet_std_os_arch")),
        ("core.os", "cpu_count") => format!("{}()", helper("jet_std_os_cpu_count")),
        ("core.os", "temp_dir") => format!("{}()", helper("jet_std_os_temp_dir")),
        ("core.os", "executable") => format!("{}()", helper("jet_std_os_executable")),
        ("core.os", "pid") => format!("{}()", helper("jet_std_os_pid")),
        ("core.os", "hostname") => format!("{}()", helper("jet_std_os_hostname")),
        ("core.os", "username") => format!("{}()", helper("jet_std_os_username")),
        ("core.os", "set_current_dir") => {
            format!("{}(&({}))", helper("jet_std_os_set_current_dir"), arg(0))
        }
        ("core.os", "on_interrupt") => {
            format!("{}({})", helper("jet_std_os_on_interrupt"), arg(0))
        }
        ("core.process", "exit") => format!("{}({})", helper("jet_std_process_exit"), arg(0)),
        ("core.process", "run") => format!("{}(&({}))", helper("jet_std_process_run"), arg(0)),
        ("core.process", "cmd") => format!("{}(&({}))", helper("jet_std_process_cmd"), arg(0)),
        ("core.process", "pipeline") => {
            format!("{}(&({}))", helper("jet_std_process_pipeline"), arg(0))
        }
        ("core.testing", "snap") => {
            format!("{}(&({}), &({}))", helper("jet_testing_snap"), arg(0), arg(1))
        }
        ("core.testing", "golden") => {
            format!("{}(&({}), &({}))", helper("jet_testing_golden"), arg(0), arg(1))
        }
        ("core.testing", "fixture") => format!("{}(&({}))", helper("jet_testing_fixture"), arg(0)),
        ("core.testing", "temp_dir") => format!("{}(&({}))", helper("jet_testing_temp_dir"), arg(0)),
        ("core.testing", "corpus") => format!("{}(&({}))", helper("jet_testing_corpus"), arg(0)),
        ("core.testing", "fake_clock") => format!("{}({})", helper("jet_std_clock_new"), arg(0)),
        ("core.testing", "fake_rng") => format!("{}({})", helper("jet_std_rng_new"), arg(0)),
        ("core.testing", "bench_budget") => {
            format!("{}(&({}), {})", helper("jet_testing_bench_budget"), arg(0), arg(1))
        }
        // D-FLOATW1: width-generic math — choose the f32 helper when the arg is F32.
        ("core.math", "sqrt") => {
            let f32_path = matches!(args.first().map(|a| &a.ty), Some(Type::Float32));
            if f32_path {
                format!("{}({})", helper("jet_std_math_sqrt_f32"), arg(0))
            } else {
                format!("{}({})", helper("jet_std_math_sqrt"), arg(0))
            }
        }
        ("core.math", "pow") => {
            let f32_path = matches!(args.first().map(|a| &a.ty), Some(Type::Float32));
            if f32_path {
                format!("{}({}, {})", helper("jet_std_math_pow_f32"), arg(0), arg(1))
            } else {
                format!("{}({}, {})", helper("jet_std_math_pow"), arg(0), arg(1))
            }
        }
        ("core.math", "floor") => {
            let f32_path = matches!(args.first().map(|a| &a.ty), Some(Type::Float32));
            if f32_path {
                format!("{}({})", helper("jet_std_math_floor_f32"), arg(0))
            } else {
                format!("{}({})", helper("jet_std_math_floor"), arg(0))
            }
        }
        ("core.math", "ceil") => {
            let f32_path = matches!(args.first().map(|a| &a.ty), Some(Type::Float32));
            if f32_path {
                format!("{}({})", helper("jet_std_math_ceil_f32"), arg(0))
            } else {
                format!("{}({})", helper("jet_std_math_ceil"), arg(0))
            }
        }
        ("core.math", "round") => format!("{}({})", helper("jet_std_math_round"), arg(0)),
        (
            "core.math",
            "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh" | "cosh" | "tanh"
            | "exp" | "ln" | "log2" | "log10" | "trunc" | "fract",
        ) => format!("({}).{}()", arg(0), method),
        ("core.math", "degrees") => format!("({}).to_degrees()", arg(0)),
        ("core.math", "radians") => format!("({}).to_radians()", arg(0)),
        ("core.math", "atan2" | "hypot") => format!("({}).{}({})", arg(0), method, arg(1)),
        ("core.math", "lerp") => {
            format!("(({}) + (({}) - ({})) * ({}))", arg(0), arg(1), arg(0), arg(2))
        }
        ("core.math", "is_nan") => format!("({}).is_nan()", arg(0)),
        ("core.math", "is_inf") => format!("({}).is_infinite()", arg(0)),
        ("core.math", "is_finite") => format!("({}).is_finite()", arg(0)),
        ("core.math", "sign") => format!("{}({})", helper("jet_std_math_sign"), arg(0)),
        ("core.math", "to_bits") => format!("(({}).to_bits() as i64)", arg(0)),
        ("core.math", "from_bits") => format!("f64::from_bits(({}) as u64)", arg(0)),
        ("core.math", "checked_add") => format!("({}).checked_add({})", arg(0), arg(1)),
        ("core.math", "checked_sub") => format!("({}).checked_sub({})", arg(0), arg(1)),
        ("core.math", "checked_mul") => format!("({}).checked_mul({})", arg(0), arg(1)),
        ("core.math", "checked_pow") => {
            format!("{}({}, {})", helper("jet_std_math_checked_pow"), arg(0), arg(1))
        }
        ("core.math", "saturating_add") => format!("({}).saturating_add({})", arg(0), arg(1)),
        ("core.math", "saturating_sub") => format!("({}).saturating_sub({})", arg(0), arg(1)),
        ("core.math", "saturating_mul") => format!("({}).saturating_mul({})", arg(0), arg(1)),
        ("core.math", "wrapping_add") => format!("({}).wrapping_add({})", arg(0), arg(1)),
        ("core.math", "wrapping_sub") => format!("({}).wrapping_sub({})", arg(0), arg(1)),
        ("core.math", "wrapping_mul") => format!("({}).wrapping_mul({})", arg(0), arg(1)),
        ("core.math", "int_pow") => format!("{}({}, {})", helper("jet_std_math_int_pow"), arg(0), arg(1)),
        ("core.math", "gcd") => format!("{}({}, {})", helper("jet_std_math_gcd"), arg(0), arg(1)),
        ("core.math", "lcm") => format!("{}({}, {})", helper("jet_std_math_lcm"), arg(0), arg(1)),
        ("core.random", "int") => {
            format!("{}({}, {})", helper("jet_std_random_int"), arg(0), arg(1))
        }
        ("core.random", "float") => format!("{}()", helper("jet_std_random_float")),
        ("core.random", "float_range") => {
            format!("{}({}, {})", helper("jet_std_random_float_range"), arg(0), arg(1))
        }
        ("core.random", "bool") => format!("{}({})", helper("jet_std_random_bool"), arg(0)),
        ("core.random", "normal") => {
            format!("{}({}, {})", helper("jet_std_random_normal"), arg(0), arg(1))
        }
        ("core.random", "exponential") => {
            format!("{}({})", helper("jet_std_random_exponential"), arg(0))
        }
        ("core.random", "seed") => format!("{}({})", helper("jet_std_random_seed"), arg(0)),
        // D-RANDSPLIT1=A: PRNG bytes — fast, NOT crypto-safe.
        ("core.random", "bytes") => format!("{}({})", helper("jet_std_random_bytes"), arg(0)),
        // D-RANDSPLIT1=A: CSPRNG bytes via /dev/urandom — cryptographically secure.
        ("core.crypto.random", "bytes") => {
            format!("{}({})", helper("jet_std_crypto_random_bytes"), arg(0))
        }
        // D-DET1: deterministic injected RNG capability constructor.
        ("core.random", "rng") => format!("{}({})", helper("jet_std_rng_new"), arg(0)),
        ("core.random", "split") => format!("{}({})", helper("jet_std_random_split"), arg(0)),
        ("core.time", "now") => format!("{}()", helper("jet_std_time_now")),
        ("core.time", "sleep") => format!("{}({})", helper("jet_std_time_sleep"), arg(0)),
        ("core.time", "start") => format!("{}()", helper("jet_std_time_start")),
        ("core.time", "instant") => format!("{}()", helper("jet_time_instant_now")),
        ("core.time", "now_utc") => format!("{}()", helper("jet_time_now_utc")),
        ("core.time", "from_unix_ms") => format!("JetDateTime::from_unix_ms({})", arg(0)),
        ("core.time", "today") => format!("{}()", helper("jet_time_today")),
        ("core.time", "parse_rfc3339") => {
            format!("{}(&({}))", helper("jet_time_parse_rfc3339"), arg(0))
        }
        ("core.time", "local_time") => {
            format!("JetLocalTime::new({}, {}, {})", arg(0), arg(1), arg(2))
        }
        ("core.time", "parse_time") => {
            format!("JetLocalTime::parse(&({})).map_err(|e| e)", arg(0))
        }
        ("core.time", "period") => format!(
            "{}({}, {}, {})",
            helper("jet_time_period"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.time", "period_days") => format!("{}({})", helper("jet_time_period_days"), arg(0)),
        ("core.time", "period_months") => {
            format!("{}({})", helper("jet_time_period_months"), arg(0))
        }
        ("core.time", "period_years") => {
            format!("{}({})", helper("jet_time_period_years"), arg(0))
        }
        ("core.time", "zone") => format!("{}(&({}))", helper("jet_time_zone_named"), arg(0)),
        ("core.time", "utc") => format!("{}()", helper("jet_time_zone_utc")),
        ("core.time", "zoned") => {
            format!("{}(&({}), &({}))", helper("jet_time_zoned"), arg(0), arg(1))
        }
        ("core.time", "zoned_local") => format!(
            "{}(&({}), &({}), &({}))",
            helper("jet_time_zoned_local"),
            arg(0),
            arg(1),
            arg(2)
        ),
        // D-DET1: deterministic injected Clock capability constructor.
        ("core.time", "clock") => format!("{}({})", helper("jet_std_clock_new"), arg(0)),
        // D-DET-CAPAPI: `Duration` constructors — pure value, ms/secs → ms span.
        ("core.time", "ms") => format!("{}({})", helper("jet_std_duration_ms"), arg(0)),
        ("core.time", "secs") => format!("{}({})", helper("jet_std_duration_secs"), arg(0)),
        ("core.time", "seconds") => format!("{}({})", helper("jet_std_duration_secs"), arg(0)),
        ("core.time", "minutes") => {
            format!("{}({})", helper("jet_std_duration_minutes"), arg(0))
        }
        ("core.time", "hours") => format!("{}({})", helper("jet_std_duration_hours"), arg(0)),
        ("core.game", "run") => {
            let replay = if args.len() >= 2
                && matches!(args[1].ty, Type::Named(ref n) if n == "GameReplay")
            {
                format!("Some(&({}))", arg(1))
            } else {
                "None".to_string()
            };
            let backend_idx = if args.len() >= 2
                && matches!(args[1].ty, Type::Named(ref n) if n == "GameBackend")
            {
                Some(1)
            } else if args.len() >= 3 {
                Some(2)
            } else {
                None
            };
            let backend = backend_idx
                .map(|i| format!("Some(&({}))", arg(i)))
                .unwrap_or_else(|| "None".to_string());
            format!(
                "{root}jet_game_run(&mut ({scene}), {replay}, {backend})",
                root = cx.root_prefix,
                scene = arg(0),
                replay = replay,
                backend = backend
            )
        }
        // D-ENC1 + D-JSONVERB1 + D-SERDE6: unified `core.encoding.*`. The dynamic forms
        // (`Json` tree / `[[String]]` / `Map`) keep their existing helpers; the typed
        // forms route through the Encode/Decode model, distinguished by the lowered arg
        // type (encode) or the resolved return type (decode). `is_json_value` etc. read
        // those total facts — codegen never re-infers (I3).
        ("core.encoding.json", "parse") => {
            format!("{}(&({}))", helper("jet_std_json_parse"), arg(0))
        }
        ("core.encoding.json", "decode") => {
            if enc_ok_is_json(ret_ty) {
                format!("{}(&({}))", helper("jet_std_json_decode_lenient"), arg(0))
            } else {
                format!(
                    "{}::<{}>(&({}))",
                    helper("jet_enc_json_decode"),
                    enc_target_rust(ret_ty, cx),
                    arg(0)
                )
            }
        }
        // D-MIGRATE3=A: `decode_traced<T>` — the traced sibling of `decode<T>`,
        // one wrapper deeper (`DecodeResult<T>`), same target-type plumbing.
        ("core.encoding.json", "decode_traced") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_json_decode_traced"),
                enc_target_rust_traced(ret_ty, cx),
                arg(0)
            )
        }
        ("core.encoding.json", "to_string") => {
            if enc_arg_is_json(args) {
                format!("{}(&({}))", helper("jet_std_json_render"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_enc_json_to_string"), arg(0))
            }
        }
        ("core.encoding.json", "to_string_pretty") => {
            if enc_arg_is_json(args) {
                format!("{}(&({}))", helper("jet_std_json_render_pretty"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_enc_json_to_string_pretty"), arg(0))
            }
        }
        ("core.encoding.json", "canonical") => {
            format!("{}(&({}))", helper("jet_std_json_render_canonical"), arg(0))
        }
        ("core.encoding.json", "events") => {
            format!("{}(&({}))", helper("jet_std_json_events"), arg(0))
        }
        ("core.encoding.jsonl", "parse") => {
            format!("{}(&({}))", helper("jet_std_jsonl_parse"), arg(0))
        }
        ("core.encoding.jsonl", "to_string") => {
            format!("{}(&({}))", helper("jet_std_jsonl_render"), arg(0))
        }
        ("core.encoding.csv", "parse") => {
            format!("{}(&({}))", helper("jet_ring_csv_parse"), arg(0))
        }
        ("core.encoding.csv", "decode") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_csv_decode"),
                enc_target_rust(ret_ty, cx),
                arg(0)
            )
        }
        ("core.encoding.csv", "decode_traced") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_csv_decode_traced"),
                enc_target_rust_traced(ret_ty, cx),
                arg(0)
            )
        }
        ("core.encoding.csv", "to_string") => {
            if enc_arg_is_string_rows(args) {
                format!("{}(&({}))", helper("jet_ring_csv_render"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_enc_csv_to_string"), arg(0))
            }
        }
        ("core.data", "csv") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_csv_decode"),
                enc_target_rust(ret_ty, cx),
                arg(0)
            )
        }
        ("core.data", "count") => format!("{}(&({}))", helper("jet_data_count"), arg(0)),
        ("core.data", "table") => format!("{}(&({}))", helper("jet_data_table"), arg(0)),
        ("core.data", "rows") => format!("{}(&({}))", helper("jet_data_rows"), arg(0)),
        ("core.data", "series") => format!("{}(&({}))", helper("jet_data_series"), arg(0)),
        ("core.data", "values") => format!("{}(&({}))", helper("jet_data_series_values"), arg(0)),
        ("core.data", "missing_count") => {
            format!("{}(&({}))", helper("jet_data_missing_count"), arg(0))
        }
        ("core.data", "lazy") => format!("{}(&({}))", helper("jet_data_lazy"), arg(0)),
        ("core.data", "collect") => format!("{}(&({}))", helper("jet_data_collect"), arg(0)),
        ("core.data", "plan") => format!("{}(&({}))", helper("jet_data_plan"), arg(0)),
        ("core.data", "lazy_filter") => {
            format!("{}(&({}), {})", helper("jet_data_lazy_filter"), arg(0), arg(1))
        }
        ("core.data", "lazy_sort_by") => {
            format!("{}(&({}), {})", helper("jet_data_lazy_sort_by"), arg(0), arg(1))
        }
        ("core.data", "group_count") => format!(
            "{}(&({}), {})",
            helper("jet_data_group_count"),
            arg(0),
            arg(1)
        ),
        ("core.data", "group_sum") => format!(
            "{}(&({}), {}, {})",
            helper("jet_data_group_sum"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.data", "group_mean") => format!(
            "{}(&({}), {}, {})",
            helper("jet_data_group_mean"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.data", "inner_join") => format!(
            "{}(&({}), &({}), {}, {})",
            helper("jet_data_inner_join"),
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        ("core.data", "left_join") => format!(
            "{}(&({}), &({}), {}, {})",
            helper("jet_data_left_join"),
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        ("core.data", "pivot_sum") => format!(
            "{}(&({}), {}, {}, {})",
            helper("jet_data_pivot_sum"),
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        ("core.data", "sum") => format!("{}(&({}))", helper("jet_data_sum"), arg(0)),
        ("core.data", "mean") => format!("{}(&({}))", helper("jet_data_mean"), arg(0)),
        ("core.data", "min") => format!("{}(&({}))", helper("jet_data_min"), arg(0)),
        ("core.data", "max") => format!("{}(&({}))", helper("jet_data_max"), arg(0)),
        ("core.data", "median") => format!("{}(&({}))", helper("jet_data_median"), arg(0)),
        ("core.data", "quantile") => {
            format!("{}(&({}), {})", helper("jet_data_quantile"), arg(0), arg(1))
        }
        ("core.data", "variance") => format!("{}(&({}))", helper("jet_data_variance"), arg(0)),
        ("core.data", "stddev") => format!("{}(&({}))", helper("jet_data_stddev"), arg(0)),
        ("core.data", "rolling_mean") => {
            format!("{}(&({}), {})", helper("jet_data_rolling_mean"), arg(0), arg(1))
        }
        ("core.data", "describe") => format!("{}(&({}))", helper("jet_data_describe"), arg(0)),
        ("core.data", "status") => format!("{}()", helper("jet_data_status")),
        ("core.data", "bar_text") => format!("{}(&({}))", helper("jet_data_bar_text"), arg(0)),
        ("core.data", "bar_svg") => format!("{}(&({}))", helper("jet_data_bar_svg"), arg(0)),
        ("core.fmt", "number") => format!("{}({})", helper("jet_fmt_number"), arg(0)),
        ("core.fmt", "decimal") => {
            format!("{}({}, {})", helper("jet_fmt_decimal"), arg(0), arg(1))
        }
        ("core.fmt", "percent") => {
            format!("{}({}, {})", helper("jet_fmt_percent"), arg(0), arg(1))
        }
        ("core.fmt", "bytes") => format!("{}({})", helper("jet_fmt_bytes"), arg(0)),
        ("core.fmt", "duration") => format!("{}({})", helper("jet_fmt_duration"), arg(0)),
        ("core.fmt", "ordinal") => format!("{}({})", helper("jet_fmt_ordinal"), arg(0)),
        ("core.fmt", "plural") => format!(
            "{}({}, &({}), &({}))",
            helper("jet_fmt_plural"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.fmt", "pad_left") => format!(
            "{}(&({}), {}, &({}))",
            helper("jet_fmt_pad_left"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.fmt", "pad_right") => format!(
            "{}(&({}), {}, &({}))",
            helper("jet_fmt_pad_right"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.fmt", "pad_center") => format!(
            "{}(&({}), {}, &({}))",
            helper("jet_fmt_pad_center"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.encoding.toml", "parse") => {
            format!("{}(&({}))", helper("jet_std_toml_parse"), arg(0))
        }
        ("core.encoding.toml", "decode") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_toml_decode"),
                enc_target_rust(ret_ty, cx),
                arg(0)
            )
        }
        ("core.encoding.toml", "decode_traced") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_toml_decode_traced"),
                enc_target_rust_traced(ret_ty, cx),
                arg(0)
            )
        }
        ("core.encoding.toml", "to_string") => {
            if enc_arg_is_json(args) {
                format!("{}(&({}))", helper("jet_std_toml_render"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_enc_toml_to_string"), arg(0))
            }
        }
        ("core.encoding.yaml", "parse") => {
            format!("{}(&({}))", helper("jet_std_yaml_parse"), arg(0))
        }
        ("core.encoding.yaml", "decode") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_yaml_decode"),
                enc_target_rust(ret_ty, cx),
                arg(0)
            )
        }
        ("core.encoding.yaml", "decode_traced") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_yaml_decode_traced"),
                enc_target_rust_traced(ret_ty, cx),
                arg(0)
            )
        }
        ("core.encoding.yaml", "to_string") => {
            if enc_arg_is_json(args) {
                format!("{}(&({}))", helper("jet_std_yaml_render"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_enc_yaml_to_string"), arg(0))
            }
        }
        ("core.encoding.xml", "parse") => {
            format!("{}(&({}))", helper("jet_std_xml_parse"), arg(0))
        }
        ("core.encoding.xml", "to_string") => {
            format!("{}(&({}))", helper("jet_std_xml_render"), arg(0))
        }
        ("core.encoding.cbor", "encode") => {
            format!("{}(&({}))", helper("jet_std_cbor_encode"), arg(0))
        }
        ("core.encoding.cbor", "decode") => {
            format!("{}(&({}))", helper("jet_std_cbor_decode"), arg(0))
        }
        // D-UUIDENC1=A: hex and base64 encode/decode.
        ("core.encoding.hex", "encode") => {
            format!("{}(&({}))", helper("jet_std_hex_encode"), arg(0))
        }
        ("core.encoding.hex", "decode") => {
            format!("{}(&({}))", helper("jet_std_hex_decode"), arg(0))
        }
        ("core.encoding.base64", "encode") => {
            format!("{}(&({}))", helper("jet_std_b64_encode"), arg(0))
        }
        ("core.encoding.base64", "decode") => {
            format!("{}(&({}))", helper("jet_std_b64_decode"), arg(0))
        }
        ("core.encoding.base64", "encode_url") => {
            format!("{}(&({}))", helper("jet_std_b64url_encode"), arg(0))
        }
        ("core.encoding.base64", "decode_url") => {
            format!("{}(&({}))", helper("jet_std_b64url_decode"), arg(0))
        }
        ("core.encoding.base32", "encode") => {
            format!("{}(&({}))", helper("jet_std_base32_encode"), arg(0))
        }
        ("core.encoding.base32", "decode") => {
            format!("{}(&({}))", helper("jet_std_base32_decode"), arg(0))
        }
        // D-UUIDENC1=A: UUID v4 (CSPRNG) and v7 (injectable Clock).
        ("core.uuid", "v4") => format!("{}()", helper("jet_std_uuid_v4")),
        ("core.uuid", "v7") => format!("{}(&({}))", helper("jet_std_uuid_v7"), arg(0)),
        ("core.gc", "collect") => format!("{}()", helper("jet_gc::gc_collect")),
        ("core.files", "open") => format!("{}(&({}))", helper("jet_std_files_open"), arg(0)),
        ("core.files", "create") => format!("{}(&({}))", helper("jet_std_files_create"), arg(0)),
        ("core.files", "append") => format!("{}(&({}))", helper("jet_std_files_append"), arg(0)),
        // E2-M7: std.path helpers (D-IO1).
        ("core.path", "join") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_path_join"),
            arg(0),
            arg(1)
        ),
        ("core.path", "parent") => format!("{}(&({}))", helper("jet_std_path_parent"), arg(0)),
        ("core.path", "extension") => {
            format!("{}(&({}))", helper("jet_std_path_extension"), arg(0))
        }
        ("core.path", "normalize") => {
            format!("{}(&({}))", helper("jet_std_path_normalize"), arg(0))
        }
        ("core.url", "parse") => format!("{}(&({}))", helper("jet_url_parse"), arg(0)),
        ("core.url", "from_parts") => format!(
            "{}(&({}), &({}), &({}), &({}), &({}))",
            helper("jet_url_from_parts"),
            arg(0),
            arg(1),
            arg(2),
            arg(3),
            arg(4)
        ),
        ("core.url", "file") => format!("{}(&({}))", helper("jet_url_file"), arg(0)),
        ("core.url", "data") => {
            format!("{}(&({}), &({}))", helper("jet_url_data"), arg(0), arg(1))
        }
        ("core.url", "query") => format!("{}(&({}))", helper("jet_url_query"), arg(0)),
        ("core.url", "percent_encode") => {
            format!(
                "{}(&({}))",
                helper("jet_url_percent_encode_component"),
                arg(0)
            )
        }
        ("core.url", "percent_decode") => {
            format!(
                "{}(&({}))",
                helper("jet_url_percent_decode_component"),
                arg(0)
            )
        }
        ("core.mime", "parse") => format!("{}(&({}))", helper("jet_mime_parse"), arg(0)),
        ("core.mime", "from_extension") => {
            format!("{}(&({}))", helper("jet_mime_from_extension"), arg(0))
        }
        ("core.mime", "extension") => format!("{}(&({}))", helper("jet_mime_extension"), arg(0)),
        // D-TEXTUNICODE1: std-only Unicode scalar helpers.
        ("core.text.unicode", "scalar_count") => {
            format!("{}(&({}))", helper("jet_text_unicode_scalar_count"), arg(0))
        }
        ("core.text.unicode", "byte_count") => {
            format!("{}(&({}))", helper("jet_text_unicode_byte_count"), arg(0))
        }
        ("core.text.unicode", "is_ascii") => {
            format!("{}(&({}))", helper("jet_text_unicode_is_ascii"), arg(0))
        }
        ("core.text.unicode", "lower") => {
            format!("{}(&({}))", helper("jet_text_unicode_lower"), arg(0))
        }
        ("core.text.unicode", "upper") => {
            format!("{}(&({}))", helper("jet_text_unicode_upper"), arg(0))
        }
        ("core.text.unicode", "scalars") => {
            format!("{}(&({}))", helper("jet_text_unicode_scalars"), arg(0))
        }
        ("core.text", "nfc") => format!("{}(&({}))", helper("jet_text_nfc"), arg(0)),
        ("core.text", "nfd") => format!("{}(&({}))", helper("jet_text_nfd"), arg(0)),
        ("core.text", "nfkc") => format!("{}(&({}))", helper("jet_text_nfkc"), arg(0)),
        ("core.text", "nfkd") => format!("{}(&({}))", helper("jet_text_nfkd"), arg(0)),
        ("core.text", "casefold") => format!("{}(&({}))", helper("jet_text_casefold"), arg(0)),
        ("core.text", "caseless_eq") => {
            format!("{}(&({}), &({}))", helper("jet_text_caseless_eq"), arg(0), arg(1))
        }
        ("core.text", "lower") => format!("({}).to_lowercase()", arg(0)),
        ("core.text", "upper") => format!("({}).to_uppercase()", arg(0)),
        ("core.text", "graphemes") => format!("{}(&({}))", helper("jet_text_graphemes"), arg(0)),
        ("core.text", "words") => format!("{}(&({}))", helper("jet_text_words"), arg(0)),
        ("core.text", "sentences") => format!("{}(&({}))", helper("jet_text_sentences"), arg(0)),
        ("core.text", "width") => format!("{}(&({}))", helper("jet_text_width"), arg(0)),
        ("core.text", "scalar_count") => format!("{}(&({}))", helper("jet_text_unicode_scalar_count"), arg(0)),
        ("core.text", "byte_count") => format!("{}(&({}))", helper("jet_text_unicode_byte_count"), arg(0)),
        ("core.text", "is_alphabetic") => format!("{}(&({}))", helper("jet_text_is_alphabetic"), arg(0)),
        ("core.text", "is_numeric") => format!("{}(&({}))", helper("jet_text_is_numeric"), arg(0)),
        ("core.text", "is_whitespace") => format!("{}(&({}))", helper("jet_text_is_whitespace"), arg(0)),
        ("core.text", "is_ascii") => format!("{}(&({}))", helper("jet_text_unicode_is_ascii"), arg(0)),
        ("core.text", "scalars") => format!("{}(&({}))", helper("jet_text_unicode_scalars"), arg(0)),
        ("core.text", "splitn") => {
            format!("{}(&({}), &({}), {})", helper("jet_text_splitn"), arg(0), arg(1), arg(2))
        }
        ("core.text", "rsplitn") => {
            format!("{}(&({}), &({}), {})", helper("jet_text_rsplitn"), arg(0), arg(1), arg(2))
        }
        ("core.text", "trim") => format!("({}).trim().to_string()", arg(0)),
        ("core.text", "trim_start") => format!("({}).trim_start().to_string()", arg(0)),
        ("core.text", "trim_end") => format!("({}).trim_end().to_string()", arg(0)),
        ("core.text", "pad_start") => {
            format!("{}(&({}), {}, &({}))", helper("jet_text_pad_start"), arg(0), arg(1), arg(2))
        }
        ("core.text", "pad_end") => {
            format!("{}(&({}), {}, &({}))", helper("jet_text_pad_end"), arg(0), arg(1), arg(2))
        }
        ("core.text", "center") => {
            format!("{}(&({}), {}, &({}))", helper("jet_text_center"), arg(0), arg(1), arg(2))
        }
        ("core.text", "starts_any") => {
            format!("{}(&({}), &({}))", helper("jet_text_starts_any"), arg(0), arg(1))
        }
        ("core.text", "ends_any") => {
            format!("{}(&({}), &({}))", helper("jet_text_ends_any"), arg(0), arg(1))
        }
        ("core.text", "char_indices") => format!("{}(&({}))", helper("jet_text_char_indices"), arg(0)),
        // E2-M9: first-party ring packages.
        ("jet.log", "info") => format!("{}(&({}))", helper("jet_ring_log_info"), arg(0)),
        ("jet.log", "warn") => format!("{}(&({}))", helper("jet_ring_log_warn"), arg(0)),
        ("jet.log", "error") => format!("{}(&({}))", helper("jet_ring_log_error"), arg(0)),
        ("jet.log", "debug") => format!("{}(&({}))", helper("jet_ring_log_debug"), arg(0)),
        ("jet.log", "field") => {
            format!("{}(&({}), &({}))", helper("jet_ring_log_field"), arg(0), arg(1))
        }
        ("jet.log", "int") => {
            format!("{}(&({}), {})", helper("jet_ring_log_int"), arg(0), arg(1))
        }
        ("jet.log", "float") => {
            format!("{}(&({}), {})", helper("jet_ring_log_float"), arg(0), arg(1))
        }
        ("jet.log", "bool") => {
            format!("{}(&({}), {})", helper("jet_ring_log_bool"), arg(0), arg(1))
        }
        ("jet.log", "redact") => format!("{}(&({}))", helper("jet_ring_log_redact"), arg(0)),
        ("jet.log", "info_fields") => {
            format!("{}(&({}), &({}))", helper("jet_ring_log_info_fields"), arg(0), arg(1))
        }
        ("jet.log", "warn_fields") => {
            format!("{}(&({}), &({}))", helper("jet_ring_log_warn_fields"), arg(0), arg(1))
        }
        ("jet.log", "error_fields") => {
            format!("{}(&({}), &({}))", helper("jet_ring_log_error_fields"), arg(0), arg(1))
        }
        ("jet.log", "debug_fields") => {
            format!("{}(&({}), &({}))", helper("jet_ring_log_debug_fields"), arg(0), arg(1))
        }
        ("jet.log", "span") => format!("{}(&({}))", helper("jet_ring_log_span"), arg(0)),
        ("jet.log", "enter") => format!("{}(&({}))", helper("jet_ring_log_enter"), arg(0)),
        ("jet.log", "close") => format!("{}(&({}))", helper("jet_ring_log_close"), arg(0)),
        ("jet.log", "set_sink") => {
            format!("{}(&({}), &({}))", helper("jet_ring_log_set_sink"), arg(0), arg(1))
        }
        ("jet.log", "sample_every") => format!("{}({})", helper("jet_ring_log_sample_every"), arg(0)),
        ("jet.log", "counter") => {
            format!("{}(&({}), {})", helper("jet_ring_log_counter"), arg(0), arg(1))
        }
        ("jet.log", "otlp_file") => format!("{}(&({}))", helper("jet_ring_log_otlp_file"), arg(0)),
        ("jet.log", "set_level") => format!("{}(&({}))", helper("jet_ring_log_set_level"), arg(0)),
        // E2-M12 D-OBS3: trace context for structured log records.
        ("jet.log", "set_trace_id") => {
            format!("{}(&({}))", helper("jet_ring_log_set_trace_id"), arg(0))
        }
        // D-LOGFMT1=A: explicit log format override.
        ("jet.log", "setup") => format!("{}(&({}))", helper("jet_ring_log_setup"), arg(0)),
        ("jet.time", "now") => format!("{}()", helper("jet_std_time_now")),
        ("jet.time", "format") => format!(
            "{}({}, &({}))",
            helper("jet_ring_time_format"),
            arg(0),
            arg(1)
        ),
        ("jet.crypto", "sha256") => format!("{}(&({}))", helper("jet_ring_crypto_sha256"), arg(0)),
        ("jet.crypto", "sha256_bytes") => {
            format!("{}(&({}))", helper("jet_ring_crypto_sha256_bytes"), arg(0))
        }
        ("jet.crypto", "sha512_bytes") => {
            format!("{}(&({}))", regex_fn("jet_crypto_sha512_impl"), arg(0))
        }
        ("jet.crypto", "blake3_bytes") => {
            format!("{}(&({}))", regex_fn("jet_crypto_blake3_impl"), arg(0))
        }
        ("jet.crypto", "constant_time_eq") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_constant_time_eq_impl"),
            arg(0),
            arg(1)
        ),
        ("jet.crypto", "hkdf_sha256") => format!(
            "{}(&({}), &({}), &({}), {})",
            regex_fn("jet_crypto_hkdf_sha256_impl"),
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        ("jet.crypto", "x25519_public") => {
            format!(
                "{}(&({}))",
                regex_fn("jet_crypto_x25519_public_impl"),
                arg(0)
            )
        }
        ("jet.crypto", "x25519_shared") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_x25519_shared_impl"),
            arg(0),
            arg(1)
        ),
        ("jet.crypto", "password_hash") => {
            format!(
                "{}(&({}))",
                regex_fn("jet_crypto_password_hash_impl"),
                arg(0)
            )
        }
        ("jet.crypto", "password_hash_with_salt") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_password_hash_with_salt_impl"),
            arg(0),
            arg(1)
        ),
        ("jet.crypto", "password_verify") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_password_verify_impl"),
            arg(0),
            arg(1)
        ),
        // D-CRYPTOENV1=A: misuse-resistant envelope (RustCrypto FFI bridge).
        ("jet.crypto", "seal") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_seal_impl"),
            arg(0),
            arg(1)
        ),
        ("jet.crypto", "file_seal") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_file_seal_impl"),
            arg(0),
            arg(1)
        ),
        ("jet.crypto", "open") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_open_impl"),
            arg(0),
            arg(1)
        ),
        ("jet.crypto", "file_open") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_file_open_impl"),
            arg(0),
            arg(1)
        ),
        ("jet.crypto", "sign") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_sign_impl"),
            arg(0),
            arg(1)
        ),
        ("jet.crypto", "verify") => format!(
            "{}(&({}), &({}), &({}))",
            regex_fn("jet_crypto_verify_impl"),
            arg(0),
            arg(1),
            arg(2)
        ),
        // D-CRYPTOENV1=A: expert-only raw AEAD (same bridge, explicit algorithm id).
        ("core.crypto.expert", "aes256_gcm_seal") => format!(
            "{}(&({}), &({}), 2i64)",
            regex_fn("jet_crypto_seal_algo_impl"),
            arg(0),
            arg(1)
        ),
        ("core.crypto.expert", "chacha20_seal") => format!(
            "{}(&({}), &({}), 1i64)",
            regex_fn("jet_crypto_seal_algo_impl"),
            arg(0),
            arg(1)
        ),
        ("core.crypto.expert", "aes256_gcm_open") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_open_impl"),
            arg(0),
            arg(1)
        ),
        ("core.crypto.expert", "chacha20_open") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_open_impl"),
            arg(0),
            arg(1)
        ),
        // U13 (D-JPK-SECRETCRYPTO1): `core.vault.get` — reads `.jet/secrets.age`
        // (project-relative) and decrypts with the local identity, via the
        // age-style crypto FFI bridge. Already the exact `Option<String>` shape
        // (`None` on any failure — missing file, missing entry, bad identity).
        ("core.vault", "get") => {
            format!("{}(&({}))", regex_fn("jet_vault_get_impl"), arg(0))
        }
        // D-TTLVAL1=A: Expiring<T> / Rotting<T> constructors.
        ("core.time.expiring", "new") => format!(
            "{}jet_expiring_new({}, {}jet_duration_millis(&({})), {}jet_clock_now(&({})))",
            helper(""),
            arg(0),
            helper(""),
            arg(1),
            helper(""),
            arg(2)
        ),
        ("core.secrets", "rotting_new") => format!(
            "{}jet_rotting_new({}, {}jet_duration_millis(&({})), {}jet_clock_now(&({})))",
            helper(""),
            arg(0),
            helper(""),
            arg(1),
            helper(""),
            arg(2)
        ),
        // D-NETSOCKET1=A: core.net — typed addresses, TCP/UDP/Unix/DNS, TLS handle.
        ("core.net", "ip_addr") => format!("{}(&({}))", helper("jet_net_ip_addr"), arg(0)),
        ("core.net", "ip_to_string") => {
            format!("{}(&({}))", helper("jet_net_ip_to_string"), arg(0))
        }
        ("core.net", "ip_is_ipv4") => format!("{}(&({}))", helper("jet_net_ip_is_ipv4"), arg(0)),
        ("core.net", "socket_addr") => {
            format!(
                "{}(&({}), {})",
                helper("jet_net_socket_addr"),
                arg(0),
                arg(1)
            )
        }
        ("core.net", "socket_addr_parse") => {
            format!("{}(&({}))", helper("jet_net_socket_addr_parse"), arg(0))
        }
        ("core.net", "socket_host") => format!("{}(&({}))", helper("jet_net_socket_host"), arg(0)),
        ("core.net", "socket_port") => format!("{}(&({}))", helper("jet_net_socket_port"), arg(0)),
        ("core.net", "socket_to_string") => {
            format!("{}(&({}))", helper("jet_net_socket_to_string"), arg(0))
        }
        ("core.net", "tcp_listen") => format!("{}(&({}))", helper("jet_net_tcp_listen"), arg(0)),
        ("core.net", "tcp_listen_addr") => {
            format!("{}(&({}))", helper("jet_net_tcp_listen_addr"), arg(0))
        }
        ("core.net", "tcp_accept") => format!("{}(&({}))", helper("jet_net_tcp_accept"), arg(0)),
        ("core.net", "tcp_connect") => format!("{}(&({}))", helper("jet_net_tcp_connect"), arg(0)),
        ("core.net", "tcp_connect_addr") => {
            format!("{}(&({}))", helper("jet_net_tcp_connect_addr"), arg(0))
        }
        ("core.net", "tcp_connect_timeout") => format!(
            "{}(&({}), {})",
            helper("jet_net_tcp_connect_timeout"),
            arg(0),
            arg(1)
        ),
        ("core.net", "tcp_connect_happy") => format!(
            "{}(&({}), {}, {})",
            helper("jet_net_tcp_connect_happy"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.net", "tcp_read") => format!("{}(&mut ({}))", helper("jet_net_tcp_read"), arg(0)),
        ("core.net", "tcp_write") => format!(
            "{}(&mut ({}), &({}))",
            helper("jet_net_tcp_write"),
            arg(0),
            arg(1)
        ),
        ("core.net", "tcp_local_addr") => {
            format!("{}(&({}))", helper("jet_net_tcp_local_addr"), arg(0))
        }
        ("core.net", "tcp_peer_addr") => {
            format!("{}(&({}))", helper("jet_net_tcp_peer_addr"), arg(0))
        }
        ("core.net", "tcp_local_socket_addr") => {
            format!("{}(&({}))", helper("jet_net_tcp_local_socket_addr"), arg(0))
        }
        ("core.net", "tcp_peer_socket_addr") => {
            format!("{}(&({}))", helper("jet_net_tcp_peer_socket_addr"), arg(0))
        }
        ("core.net", "listener_local_socket_addr") => format!(
            "{}(&({}))",
            helper("jet_net_listener_local_socket_addr"),
            arg(0)
        ),
        ("core.net", "set_timeout") => format!(
            "{}(&mut ({}), {})",
            helper("jet_net_set_timeout"),
            arg(0),
            arg(1)
        ),
        ("core.net", "set_read_timeout") => format!(
            "{}(&mut ({}), {})",
            helper("jet_net_set_read_timeout"),
            arg(0),
            arg(1)
        ),
        ("core.net", "set_write_timeout") => format!(
            "{}(&mut ({}), {})",
            helper("jet_net_set_write_timeout"),
            arg(0),
            arg(1)
        ),
        ("core.net", "tcp_reply") => format!(
            "{}({}, &({}), &({}))",
            helper("jet_net_tcp_reply"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.net", "udp_bind") => format!("{}(&({}))", helper("jet_net_udp_bind"), arg(0)),
        ("core.net", "udp_bind_addr") => {
            format!("{}(&({}))", helper("jet_net_udp_bind_addr"), arg(0))
        }
        ("core.net", "udp_local_addr") => {
            format!("{}(&({}))", helper("jet_net_udp_local_addr"), arg(0))
        }
        ("core.net", "udp_set_timeout") => format!(
            "{}(&({}), {})",
            helper("jet_net_udp_set_timeout"),
            arg(0),
            arg(1)
        ),
        ("core.net", "udp_send_to") => format!(
            "{}(&({}), &({}), &({}))",
            helper("jet_net_udp_send_to"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.net", "udp_recv_from") => format!(
            "{}(&({}), {})",
            helper("jet_net_udp_recv_from"),
            arg(0),
            arg(1)
        ),
        ("core.net", "udp_packet_data") => {
            format!("{}(&({}))", helper("jet_net_udp_packet_data"), arg(0))
        }
        ("core.net", "udp_packet_addr") => {
            format!("{}(&({}))", helper("jet_net_udp_packet_addr"), arg(0))
        }
        ("core.net", "unix_listen") => format!("{}(&({}))", helper("jet_net_unix_listen"), arg(0)),
        ("core.net", "unix_accept") => format!("{}(&({}))", helper("jet_net_unix_accept"), arg(0)),
        ("core.net", "unix_connect") => {
            format!("{}(&({}))", helper("jet_net_unix_connect"), arg(0))
        }
        ("core.net", "unix_read") => format!("{}(&mut ({}))", helper("jet_net_unix_read"), arg(0)),
        ("core.net", "unix_write") => format!(
            "{}(&mut ({}), &({}))",
            helper("jet_net_unix_write"),
            arg(0),
            arg(1)
        ),
        ("core.net", "dns_a") => format!("{}(&({}), {})", helper("jet_net_dns_a"), arg(0), arg(1)),
        ("core.net", "dns_aaaa") => {
            format!("{}(&({}), {})", helper("jet_net_dns_aaaa"), arg(0), arg(1))
        }
        ("core.net", "dns_a_at") => format!(
            "{}(&({}), &({}), {})",
            helper("jet_net_dns_a_at"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.net", "dns_aaaa_at") => format!(
            "{}(&({}), &({}), {})",
            helper("jet_net_dns_aaaa_at"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.net", "dns_txt") => {
            format!("{}(&({}), {})", helper("jet_net_dns_txt"), arg(0), arg(1))
        }
        ("core.net", "dns_txt_at") => format!(
            "{}(&({}), &({}), {})",
            helper("jet_net_dns_txt_at"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.net", "dns_srv") => {
            format!("{}(&({}), {})", helper("jet_net_dns_srv"), arg(0), arg(1))
        }
        ("core.net", "dns_srv_at") => format!(
            "{}(&({}), &({}), {})",
            helper("jet_net_dns_srv_at"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.net", "dns_srv_target") => {
            format!("{}(&({}))", helper("jet_net_dns_srv_target"), arg(0))
        }
        ("core.net", "dns_srv_port") => {
            format!("{}(&({}))", helper("jet_net_dns_srv_port"), arg(0))
        }
        ("core.net", "dns_srv_priority") => {
            format!("{}(&({}))", helper("jet_net_dns_srv_priority"), arg(0))
        }
        ("core.net", "dns_srv_weight") => {
            format!("{}(&({}))", helper("jet_net_dns_srv_weight"), arg(0))
        }
        ("core.net", "tls_connect") => format!(
            "{{ let _s = {}; {}(_s.inner, &({})).map(|id| JetTlsStream{{id}}) }}",
            arg(0),
            regex_fn("jet_net_tls_connect_impl"),
            arg(1)
        ),
        ("core.net", "tls_read") => {
            format!("{}(({}).id)", regex_fn("jet_net_tls_read_impl"), arg(0))
        }
        ("core.net", "tls_write") => format!(
            "{}(({}).id, &({}))",
            regex_fn("jet_net_tls_write_impl"),
            arg(0),
            arg(1)
        ),
        ("core.net", "tls_close") => {
            format!("{}(({}).id)", regex_fn("jet_net_tls_close_impl"), arg(0))
        }
        // E2-M10: jet.http — HTTP client.
        ("jet.http", "get") => format!("{}(&({}))", helper("jet_http_get"), arg(0)),
        ("jet.http", "post") => {
            format!("{}(&({}), &({}))", helper("jet_http_post"), arg(0), arg(1))
        }
        // c109 Phase 25: HttpRouter producer + parse/dispatch (D-ROUTE1=A), byte-for-byte
        // `emit_core_call` (Source/Codegen/Expression.rs ~L1411). `router()` is arg-free;
        // `parse(raw)` borrows the raw string; `dispatch(router, req)` borrows the router
        // and passes the request by value.
        ("jet.http", "router") => format!("{}()", helper("jet_http_router_new")),
        ("jet.http", "parse") => format!("{}(&({}))", helper("jet_http_parse_request"), arg(0)),
        ("jet.http", "dispatch") => format!(
            "{}(&({}), {})",
            helper("jet_http_router_dispatch"),
            arg(0),
            arg(1)
        ),
        // D-REGEXENGINE1=A: core.regex — std-only runtime in jet_std, no bridge dep.
        ("jet.regex", "flags") => {
            format!(
                "{}jet_std::jet_regex_flags({}, {}, {})",
                cx.root_prefix,
                arg(0),
                arg(1),
                arg(2)
            )
        }
        ("jet.regex", "compile") => {
            format!(
                "{}jet_std::jet_regex_compile(&({}))",
                cx.root_prefix,
                arg(0)
            )
        }
        ("jet.regex", "compile_with") => {
            format!(
                "{}jet_std::jet_regex_compile_with(&({}), &({}))",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        }
        ("jet.regex", "is_match") => {
            format!(
                "{}jet_std::jet_regex_is_match(&({}), &({}))",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        }
        ("jet.regex", "match") => {
            format!(
                "{}jet_std::jet_regex_match(&({}), &({}))",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        }
        ("jet.regex", "find") => {
            format!(
                "{}jet_std::jet_regex_find(&({}), &({}))",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        }
        ("jet.regex", "find_all") => {
            format!(
                "{}jet_std::jet_regex_find_all(&({}), &({}))",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        }
        ("jet.regex", "matches") => {
            format!(
                "{}jet_std::jet_regex_matches(&({}), &({}))",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        }
        ("jet.regex", "split") => {
            format!(
                "{}jet_std::jet_regex_split(&({}), &({}))",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        }
        ("jet.regex", "split_limit") => {
            format!(
                "{}jet_std::jet_regex_split_limit(&({}), &({}), {})",
                cx.root_prefix,
                arg(0),
                arg(1),
                arg(2)
            )
        }
        ("jet.regex", "replace") => format!(
            "{}jet_std::jet_regex_replace(&({}), &({}), &({}))",
            cx.root_prefix,
            arg(0),
            arg(1),
            arg(2)
        ),
        ("jet.regex", "replace_all") => format!(
            "{}jet_std::jet_regex_replace_all(&({}), &({}), &({}))",
            cx.root_prefix,
            arg(0),
            arg(1),
            arg(2)
        ),
        // D-DEP-ARCHIVE1=A: core.archive — gzip compress/decompress via the FFI bridge crate.
        // Arguments are `[U8]` (Vec<u8>); bridge functions take `&[u8]` (auto-coerce from &Vec<u8>).
        ("core.archive", "gzip_compress") => {
            format!("{}(&({}))", regex_fn("jet_archive_gzip_compress"), arg(0))
        }
        ("core.archive", "gzip_decompress") => {
            format!("{}(&({}))", regex_fn("jet_archive_gzip_decompress"), arg(0))
        }
        // D-DEP-ARCHIVE1=A: core.archive — zip compress/decompress via the `zip` crate FFI bridge.
        // zip_compress takes (&str, &[u8]); zip_decompress takes &[u8].
        ("core.archive", "zip_compress") => {
            format!(
                "{}(&({}), &({}))",
                regex_fn("jet_archive_zip_compress"),
                arg(0),
                arg(1)
            )
        }
        ("core.archive", "zip_decompress") => {
            format!("{}(&({}))", regex_fn("jet_archive_zip_decompress"), arg(0))
        }
        // D-DEP-ARCHIVE1=A: tar_add / tar_get / tar_names_json via the FFI bridge.
        // All three take &[u8] / &str args (non-scalar → borrow); none take scalars.
        ("core.archive", "tar_add") => {
            format!(
                "{}(&({}), &({}), &({}))",
                regex_fn("jet_archive_tar_add"),
                arg(0),
                arg(1),
                arg(2)
            )
        }
        ("core.archive", "tar_get") => {
            format!(
                "{}(&({}), &({}))",
                regex_fn("jet_archive_tar_get"),
                arg(0),
                arg(1)
            )
        }
        ("core.archive", "tar_names_json") => {
            format!("{}(&({}))", regex_fn("jet_archive_tar_names_json"), arg(0))
        }
        // D-RAYLIB1=A / D-FLAGSHIP-RAYLIB1=A: typed graphics bridge.
        ("core.raylib", "window_open") => {
            format!(
                "{}jet_raylib_window_open({}, {}, &({}))",
                cx.root_prefix,
                arg(0),
                arg(1),
                arg(2)
            )
        }
        ("core.raylib", "window_should_close") => {
            format!(
                "{}jet_raylib_window_should_close(&({}))",
                cx.root_prefix,
                arg(0)
            )
        }
        ("core.raylib", "begin_drawing") => {
            format!("{}jet_raylib_begin_drawing(&({}))", cx.root_prefix, arg(0))
        }
        ("core.raylib", "clear_background") => {
            format!(
                "{}jet_raylib_clear_background(&({}))",
                cx.root_prefix,
                arg(0)
            )
        }
        ("core.raylib", "draw_text") => {
            format!(
                "{}jet_raylib_draw_text(&({}), {}, {}, {}, &({}))",
                cx.root_prefix,
                arg(0),
                arg(1),
                arg(2),
                arg(3),
                arg(4)
            )
        }
        ("core.raylib", "end_drawing") => {
            format!("{}jet_raylib_end_drawing()", cx.root_prefix)
        }
        ("core.raylib", "close_window") => {
            format!("{}jet_raylib_close_window(&({}))", cx.root_prefix, arg(0))
        }
        ("core.raylib", "color") => {
            format!(
                "{}jet_raylib_color({}, {}, {}, {})",
                cx.root_prefix,
                arg(0),
                arg(1),
                arg(2),
                arg(3)
            )
        }
        // D-CODECS1: core.compress.gzip / core.compress.zstd — standalone codec APIs
        // (separate from core.archive) via the FFI bridge crate. `compress` is
        // infallible; `decompress` returns a Rust `Result<Vec<u8>, String>` which is
        // already the runtime shape of the Jet `Result<[U8], String>` value — no
        // extra wrapping needed (same pattern as `jet.crypto`'s seal/open).
        ("core.compress.gzip", "compress") => {
            format!("{}(&({}))", regex_fn("jet_compress_gzip_compress"), arg(0))
        }
        ("core.compress.gzip", "decompress") => {
            format!(
                "{}(&({}))",
                regex_fn("jet_compress_gzip_decompress"),
                arg(0)
            )
        }
        ("core.compress.zstd", "compress") => {
            format!("{}(&({}))", regex_fn("jet_compress_zstd_compress"), arg(0))
        }
        ("core.compress.zstd", "decompress") => {
            format!(
                "{}(&({}))",
                regex_fn("jet_compress_zstd_decompress"),
                arg(0)
            )
        }
        // D-DBDRIVER1: jet.db — SQLite via the FFI bridge crate. `open`/`open_memory`
        // are the only module-level entry points; they wrap the bridge's raw u64
        // handle in the Jet-visible `DbConnection` handle (`JetDbConnection`), so
        // every other operation dispatches by receiver TYPE as an instance method
        // (`THandleOp::DbQuery`/… in the `HandleMethod` arm below), not a second
        // module-call surface.
        ("jet.db", "open") => {
            format!(
                "{}JetDbConnection {{ handle: {}(&({})) }}",
                cx.root_prefix,
                regex_fn("jet_db_open"),
                arg(0)
            )
        }
        ("jet.db", "open_memory") => {
            format!(
                "{}JetDbConnection {{ handle: {}() }}",
                cx.root_prefix,
                regex_fn("jet_db_open_memory")
            )
        }
        ("jet.db", "params") => {
            format!("{}jet_std::jet_db_params_from_sql(&({}))", cx.root_prefix, arg(0))
        }
        ("jet.db", "row_value") => {
            format!("{}jet_std::jet_db_row_value(&({}), &({}))", cx.root_prefix, arg(0), arg(1))
        }
        ("jet.db", "row_int") => {
            format!("{}jet_std::jet_db_row_int(&({}), &({}))", cx.root_prefix, arg(0), arg(1))
        }
        ("jet.db", "row_float") => {
            format!("{}jet_std::jet_db_row_float(&({}), &({}))", cx.root_prefix, arg(0), arg(1))
        }
        ("jet.db", "row_text") => {
            format!("{}jet_std::jet_db_row_text(&({}), &({}))", cx.root_prefix, arg(0), arg(1))
        }
        ("jet.db", "row_bool") => {
            format!("{}jet_std::jet_db_row_bool(&({}), &({}))", cx.root_prefix, arg(0), arg(1))
        }
        ("jet.db", "transaction") => {
            let root = &cx.root_prefix;
            format!(
                "{{ let __jet_conn = ({}); let __jet_steps = ({}); let __jet_empty: Vec<{}jet_std::DbValue> = Vec::new(); if !{}(__jet_conn.handle) {{ Err({}jet_std::DbError {{ message: format!(\"could not begin transaction: {{}}\", {}) }}) }} else {{ let mut __jet_done: i64 = 0; let mut __jet_err: Option<{}jet_std::DbError> = None; for __jet_sql in __jet_steps.iter() {{ match {}jet_std::jet_db_decode_execute_result(&{}(__jet_conn.handle, __jet_sql, &{}jet_std::jet_db_encode_params(&__jet_empty))) {{ Ok(_) => __jet_done += 1, Err(e) => {{ __jet_err = Some(e); break; }} }} }} if let Some(e) = __jet_err {{ let _ = {}(__jet_conn.handle); Err(e) }} else if {}(__jet_conn.handle) {{ Ok(__jet_done) }} else {{ Err({}jet_std::DbError {{ message: \"could not commit transaction\".to_string() }}) }} }} }}",
                arg(0),
                arg(2),
                root,
                regex_fn("jet_db_begin"),
                root,
                arg(1),
                root,
                root,
                regex_fn("jet_db_execute"),
                root,
                regex_fn("jet_db_rollback"),
                regex_fn("jet_db_commit"),
                root,
            )
        }
        ("jet.db", "migrate") => {
            let root = &cx.root_prefix;
            format!(
                "{{ let __jet_conn = ({}); let __jet_name = ({}); let __jet_steps = ({}); let __jet_empty: Vec<{}jet_std::DbValue> = Vec::new(); if !{}(__jet_conn.handle) {{ Err({}jet_std::DbError {{ message: format!(\"could not begin migration `{{}}`\", __jet_name) }}) }} else {{ let __jet_create_sql = \"CREATE TABLE IF NOT EXISTS __jet_migrations (name TEXT PRIMARY KEY, checksum TEXT NOT NULL)\".to_string(); let __jet_create = {}jet_std::jet_db_decode_execute_result(&{}(__jet_conn.handle, &__jet_create_sql, &{}jet_std::jet_db_encode_params(&__jet_empty))); match __jet_create {{ Err(e) => {{ let _ = {}(__jet_conn.handle); Err(e) }}, Ok(_) => {{ let __jet_checksum = {}jet_std::jet_db_migration_checksum(&__jet_steps); let __jet_check_sql = \"SELECT checksum FROM __jet_migrations WHERE name = ?\".to_string(); let __jet_check_params = vec![{}jet_std::DbValue::Text(__jet_name.clone())]; let __jet_existing = {}jet_std::jet_db_decode_query_result(&{}(__jet_conn.handle, &__jet_check_sql, &{}jet_std::jet_db_encode_params(&__jet_check_params))); match __jet_existing {{ Err(e) => {{ let _ = {}(__jet_conn.handle); Err(e) }}, Ok(rows) => {{ if let Some(row) = rows.into_iter().next() {{ let old = row.get(\"checksum\").and_then(|v| v.text().ok()).unwrap_or_default(); if old == __jet_checksum {{ if {}(__jet_conn.handle) {{ Ok(0) }} else {{ Err({}jet_std::DbError {{ message: format!(\"could not commit migration `{{}}`\", __jet_name) }}) }} }} else {{ let _ = {}(__jet_conn.handle); Err({}jet_std::DbError {{ message: format!(\"migration `{{}}` checksum changed\", __jet_name) }}) }} }} else {{ let mut __jet_done: i64 = 0; let mut __jet_err: Option<{}jet_std::DbError> = None; for __jet_sql in __jet_steps.iter() {{ match {}jet_std::jet_db_decode_execute_result(&{}(__jet_conn.handle, __jet_sql, &{}jet_std::jet_db_encode_params(&__jet_empty))) {{ Ok(_) => __jet_done += 1, Err(e) => {{ __jet_err = Some(e); break; }} }} }} if let Some(e) = __jet_err {{ let _ = {}(__jet_conn.handle); Err(e) }} else {{ let __jet_insert_sql = \"INSERT INTO __jet_migrations (name, checksum) VALUES (?, ?)\".to_string(); let __jet_insert_params = vec![{}jet_std::DbValue::Text(__jet_name.clone()), {}jet_std::DbValue::Text(__jet_checksum)]; match {}jet_std::jet_db_decode_execute_result(&{}(__jet_conn.handle, &__jet_insert_sql, &{}jet_std::jet_db_encode_params(&__jet_insert_params))) {{ Err(e) => {{ let _ = {}(__jet_conn.handle); Err(e) }}, Ok(_) => {{ if {}(__jet_conn.handle) {{ Ok(__jet_done) }} else {{ Err({}jet_std::DbError {{ message: format!(\"could not commit migration `{{}}`\", __jet_name) }}) }} }} }} }} }} }} }} }} }} }} }}",
                arg(0),
                arg(1),
                arg(2),
                root,
                regex_fn("jet_db_begin"),
                root,
                root,
                regex_fn("jet_db_execute"),
                root,
                regex_fn("jet_db_rollback"),
                root,
                root,
                root,
                regex_fn("jet_db_query"),
                root,
                regex_fn("jet_db_rollback"),
                regex_fn("jet_db_commit"),
                root,
                regex_fn("jet_db_rollback"),
                root,
                root,
                root,
                regex_fn("jet_db_execute"),
                root,
                regex_fn("jet_db_rollback"),
                root,
                root,
                root,
                regex_fn("jet_db_execute"),
                root,
                regex_fn("jet_db_rollback"),
                regex_fn("jet_db_commit"),
                root,
            )
        }
        // D-DEP-WASM1=A / D-PLUGIN1=B (c81): core.plugin — sandboxed WASM
        // Component Model loader via the FFI bridge crate (wasmtime,
        // runtime-side only, I6). `load` is the only module-level entry
        // point; it wraps the bridge's wire-encoded handle in the Jet-visible
        // `Plugin` handle (`JetPlugin`), so `.call`/`.call_int` dispatch by
        // receiver TYPE as instance methods (`THandleOp::PluginCall{,Int}`
        // in the `HandleMethod` arm below), not a second module-call surface.
        ("jet.plugin", "load") => {
            format!(
                "{root}JetPlugin {{ handle: {root}jet_std::jet_plugin_load_handle(&{}(&({}))) }}",
                regex_fn("jet_plugin_load"),
                arg(0),
                root = cx.root_prefix,
            )
        }
        // c109 Phase 20: the polymorphic core specials — byte-for-byte `emit_core_call`.
        // Their return type is arg-type dependent (resolved by sema's bespoke
        // `infer_core_call` and written onto the node's `resolved_ret`, read at
        // lowering), but the EMITTED form is a fixed per-`(module, method)` string —
        // no type decision here (I3). Args are emitted PLAINLY, exactly `emit_core_call`.
        ("core.math", "abs") => format!("({}).abs()", arg(0)),
        ("core.math", "min") => format!("({}).min({})", arg(0), arg(1)),
        ("core.math", "max") => format!("({}).max({})", arg(0), arg(1)),
        ("core.math", "clamp") => format!("({}).clamp({}, {})", arg(0), arg(1), arg(2)),
        ("core.random", "pick") => format!("{}(&({}))", helper("jet_std_random_pick"), arg(0)),
        ("core.random", "weighted_pick") => {
            format!("{}(&({}), &({}))", helper("jet_std_random_weighted_pick"), arg(0), arg(1))
        }
        ("core.random", "sample") => {
            format!("{}(&({}), {})", helper("jet_std_random_sample"), arg(0), arg(1))
        }
        ("core.random", "shuffle") => {
            format!("{}(&mut ({}))", helper("jet_std_random_shuffle"), arg(0))
        }
        ("core.io", "eprint") => format!("eprintln!(\"{{}}\", ({}).jet_show())", arg(0)),
        // D-TERM1 (ratified 2026-06-22): terminal direct-input.
        ("core.term", "read_key") => format!("{}()", helper("jet_term_read_key")),
        // D-FIDELITY-API1=A: runtime-global fidelity signal.
        ("core.perf", "fidelity") => format!("jet_perf_fidelity()"),
        ("core.perf", "default_fidelity") => format!("jet_perf_default_fidelity()"),
        ("core.perf", "override_fidelity") => {
            format!("jet_perf_override_fidelity({})", arg(0))
        }
        ("core.perf", "reset_fidelity") => format!("jet_perf_reset_fidelity()"),
        // D-RENDERTGT2=A (c133 M1): UI backend seam constructors.
        ("core.ui", "null_backend") => format!("{}jet_ui_null()", cx.root_prefix),
        ("core.ui", "tui_backend") => format!("{}jet_ui_tui()", cx.root_prefix),
        // D-UIDEVSHELL1=A (c134 Phase 8): native Linux GTK4 backend constructor.
        ("core.ui", "gtk_backend") => format!("{}jet_ui_gtk()", cx.root_prefix),
        ("core.ui", "point") => format!("{}jet_ui_point({}, {})", cx.root_prefix, arg(0), arg(1)),
        ("core.ui", "size") => format!("{}jet_ui_size({}, {})", cx.root_prefix, arg(0), arg(1)),
        ("core.ui", "rect") => format!(
            "{}jet_ui_rect({}, {}, {}, {})",
            cx.root_prefix,
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        ("core.ui", "constraint") => format!(
            "{}jet_ui_constraint({}, {}, {}, {})",
            cx.root_prefix,
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        ("core.ui", "node") => format!(
            "{}jet_ui_node(&({}), {}, {})",
            cx.root_prefix,
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.ui", "key_event") => format!("{}jet_ui_key_event(&({}))", cx.root_prefix, arg(0)),
        ("core.ui", "resize_event") => format!(
            "{}jet_ui_resize_event({}, {})",
            cx.root_prefix,
            arg(0),
            arg(1)
        ),
        // D-A11YGATE1=B (c134 Phase 6): accessible-role node + role constants.
        ("core.ui", "node_role") => format!(
            "{}jet_ui_node_role(&({}), {}, {}, {})",
            cx.root_prefix,
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        // D-STYLESHAPE1=A wiring: a node carrying an explicit fill color.
        ("core.ui", "node_color") => format!(
            "{}jet_ui_node_color(&({}), {}, {}, &({}))",
            cx.root_prefix,
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        ("core.ui", "aria_role_button") => {
            format!("{}jet_ui_aria_role_button()", cx.root_prefix)
        }
        ("core.ui", "aria_role_text_input") => {
            format!("{}jet_ui_aria_role_text_input()", cx.root_prefix)
        }
        ("core.ui", "aria_role_label") => {
            format!("{}jet_ui_aria_role_label()", cx.root_prefix)
        }
        ("core.ui", "aria_role_container") => {
            format!("{}jet_ui_aria_role_container()", cx.root_prefix)
        }
        // D-FLAGSHIP-WEBAPI1=A: browser-only helpers. Native TIR emission stays
        // inert so web TIR validation can lower checked JS bodies without making
        // rustc the browser API checker.
        ("core.web", "on") => "{ let _ = || (); () }".to_string(),
        ("core.web", "value") => "String::new()".to_string(),
        ("core.web.storage.local" | "core.web.storage.session", "get") => {
            "None::<String>".to_string()
        }
        ("core.web.storage.local" | "core.web.storage.session", "set" | "remove" | "clear") => {
            "()".to_string()
        }
        // c-devserver (owner-directed 2026-07-01): `devserver.for_app(file)`
        // constructor — the builder methods dispatch through
        // `THandleOp::DevServerMethod` above, not here.
        ("core.devserver", "for_app") => {
            format!("{}jet_devserver_for_app(&({}))", cx.root_prefix, arg(0))
        }
        ("core.devserver", "app") => {
            format!("{}jet_devserver_app()", cx.root_prefix)
        }
        // D-APPROX1=A: sketch constructors.
        ("core.sketch.hll", "new") => format!("JetHyperLogLog::new()"),
        ("core.sketch.tdigest", "new") => format!("JetTDigest::new()"),
        ("core.sketch.cms", "new") => format!("JetCountMinSketch::new()"),
        ("core.sketch.reservoir", "new") => format!("JetReservoirSampler::new({})", arg(0)),
        // D-NETDEP1=A / D-HTTPLIB1=A: HTTP client constructors.
        // Bridge returns (i64, String, Vec<String>); CoreLib assembles JetHttpClientResp.
        ("core.http.client", "get") => {
            let u = if matches!(args.get(0).map(|e| &e.ty), Some(Type::Named(n)) if n == "Url") {
                format!("({}).to_string_value()", arg(0))
            } else {
                arg(0)
            };
            format!(
                "{}(&({})).map(|(s,b,h)| JetHttpClientResp{{status:s,body:b,headers:h}})",
                regex_fn("jet_http_client_get_impl"),
                u
            )
        }
        ("core.http.client", "post") => {
            let u = if matches!(args.get(0).map(|e| &e.ty), Some(Type::Named(n)) if n == "Url") {
                format!("({}).to_string_value()", arg(0))
            } else {
                arg(0)
            };
            format!(
                "{}(&({}), &({})).map(|(s,b,h)| JetHttpClientResp{{status:s,body:b,headers:h}})",
                regex_fn("jet_http_client_post_impl"),
                u,
                arg(1)
            )
        }
        ("core.http.client", "request") => {
            let u = if matches!(args.get(1).map(|e| &e.ty), Some(Type::Named(n)) if n == "Url") {
                format!("({}).to_string_value()", arg(1))
            } else {
                arg(1)
            };
            format!("jet_http_client_request_new(&({}), &({}))", arg(0), u)
        }
        // D-NETDEP1=A / D-HTTPLIB1=A: HTTP server constructors (CoreLib, no prefix needed).
        ("core.http.server", "mux") => format!("jet_http_mux_new()"),
        ("core.http.server", "serve") if args.len() == 3 => {
            let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
            format!(
                "jet_http_mux_serve_tls(&({}), {}, {}, |cert, key| {ffi}::jet_http_server_tls_validate_impl(cert, key), |cert, key, stream, handler| {ffi}::jet_http_server_tls_handle_impl(cert, key, stream, handler))",
                arg(0),
                arg(1),
                arg(2)
            )
        }
        ("core.http.server", "serve") => format!("jet_http_mux_serve(&({}), {})", arg(0), arg(1)),
        ("core.http.server", "serve_once") => {
            format!("jet_http_mux_serve_once(&({}), {})", arg(0), arg(1))
        }
        ("core.http.server", "serve_once_listener") => {
            format!(
                "jet_http_mux_serve_once_listener(&({}), &({}))",
                arg(0),
                arg(1)
            )
        }
        ("core.http.server", "response") => {
            format!("jet_http_srv_response({}, &({}))", arg(0), arg(1))
        }
        ("core.http.server", "tls") => format!("jet_http_srv_tls(&({}), &({}))", arg(0), arg(1)),
        ("core.http.server", "sse") => format!("jet_http_srv_sse(&({}))", arg(0)),
        ("core.http.server", "static_file") => {
            format!("jet_http_srv_static_file(&({}), &({}))", arg(0), arg(1))
        }
        ("core.http.server", "static_file_range") => format!(
            "jet_http_srv_static_file_range(&({}), &({}), &({}))",
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.http.server", "access_log") => {
            format!("jet_http_srv_access_log(&({}), {})", arg(0), arg(1))
        }
        // D-TIMEDEPTH1=A: civil-time constructors.
        ("core.time.date", "new") => format!("JetDate::new({}, {}, {})", arg(0), arg(1), arg(2)),
        ("core.time.date", "today") => format!("JetDate::today_utc()"),
        ("core.time.date", "parse") => format!("JetDate::parse(&({})).map_err(|e| e)", arg(0)),
        ("core.time.datetime", "from_timestamp") => {
            format!("JetDateTime::from_timestamp({})", arg(0))
        }
        ("core.time.datetime", "now") => format!("JetDateTime::now()"),
        _ => "/* unknown std call */".to_string(),
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
