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
        TFuncKind::TraitMethod { is_unsafe, self_conv } => {
            emit_tir_trait_method(tir, *is_unsafe, *self_conv, cx, out)
        }
        TFuncKind::Delegation { sig, fwd, has_return } => {
            emit_tir_delegation(tir, sig, fwd, *has_return, cx, out)
        }
    }
}

/// A module-level free function: `pub fn name(params) -> ret { … }` (or `fn main`).
/// Byte-identical to `emit_func`'s output.
pub(crate) fn emit_tir_toplevel(tir: &TFunc, cx: &Cx, out: &mut String) {
    let ret_clause = match &tir.ret {
        Some(t) => format!(" -> {}", rust_return_type(cx, t, tir.is_view)),
        None => String::new(),
    };
    let params = tir
        .params
        .iter()
        .map(|(rust_name, ty, conv)| {
            format!("{}: {}", rust_name, rust_param_type(cx, *conv, ty))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let vis = if tir.is_main { "" } else { "pub " };
    // c109 Phase 18: an `#Unsafe fn` lowers to `unsafe fn` — the prefix sits right after
    // `vis`, exactly as `emit_func` (`{vis}{unsafe_kw}fn …`). I1: emitted ONLY when the
    // source was `#Unsafe fn` (`tir.is_unsafe`).
    let unsafe_kw = if tir.is_unsafe { "unsafe " } else { "" };
    // E2-M12 D-OBS1: track the current function name for rich panic reports —
    // matches `emit_func` so panic output is identical.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    out.push_str(&format!(
        "{vis}{unsafe_kw}fn {name}{gen}({params}){ret} {{\n",
        name = cx.mangle_name(&tir.name),
        gen = tir.generics,
        params = params,
        ret = ret_clause,
    ));
    // D-COV1: probe at the function head (skip the synthetic `main`).
    if cx.coverage && !tir.is_main {
        out.push_str(&format!("    jet_cov({});\n", tir.line));
    }
    emit_tir_stmts(&tir.body, cx, out, 1);
    out.push_str("}\n\n");
}

/// c109 Phase 7: an inherent method, emitted INSIDE an `impl user_<T> { … }` block
/// (the caller `emit_type_impl` already opened it). Byte-identical to `emit_method`:
/// `    pub fn user_<name>(<self>, <params>) -> <ret> {\n … \n    }\n`. The `self`
/// receiver form comes from `self_conv` (`Read`→`&self`, `Mutate`→`&mut self`,
/// `Move`→`self`); a static method (`self_conv == None`) emits no receiver.
pub(crate) fn emit_tir_method(tir: &TFunc, self_conv: Option<AccessConvention>, cx: &Cx, out: &mut String) {
    let indent = 1;
    let pad = "    ".repeat(indent);
    let ret_clause = match &tir.ret {
        Some(t) => format!(" -> {}", rust_return_type(cx, t, tir.is_view)),
        None => String::new(),
    };
    let mut params: Vec<String> = Vec::new();
    if let Some(conv) = self_conv {
        params.push(
            match conv {
                AccessConvention::Read
                | AccessConvention::Infer
                | AccessConvention::Share
                | AccessConvention::Raw => "&self",
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
    // E2-M12 D-OBS1: track the current function name for rich panic reports.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    out.push_str(&format!(
        "{pad}pub {unsafe_kw}fn {name}({params}){ret} {{\n",
        name = mangle(&tir.name),
        params = params.join(", "),
        ret = ret_clause,
    ));
    // D-COV1: probe at the method head.
    if cx.coverage {
        out.push_str(&format!("{pad}    jet_cov({});\n", tir.line));
    }
    emit_tir_stmts(&tir.body, cx, out, indent + 1);
    out.push_str(&format!("{pad}}}\n"));
}

/// c109 Phase 12: a trait-impl method, emitted INSIDE an `impl Trait for user_<T> { … }`
/// block (the caller `emit_trait_impl`/`emit_external_trait_impl` opened it).
/// Byte-identical to `emit_trait_method` (Source/Codegen/Items.rs): a BARE method name
/// (no `user_` mangle — the trait owns it), NO `pub`, an always-`&self` receiver, and
/// an `unsafe ` prefix iff the source was an `@unsafe fn`.
pub(crate) fn emit_tir_trait_method(tir: &TFunc, is_unsafe: bool, self_conv: AccessConvention, cx: &Cx, out: &mut String) {
    let indent = 1;
    let pad = "    ".repeat(indent);
    let ret_clause = match &tir.ret {
        // `emit_trait_method` computes `ret = rust_return_type(...)` then, if non-empty,
        // ` -> ret`. A unit return yields the empty clause.
        Some(t) => {
            let ret = rust_return_type(cx, t, tir.is_view);
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
        AccessConvention::Read
        | AccessConvention::Infer
        | AccessConvention::Share
        | AccessConvention::Raw => "&self",
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
pub(crate) fn emit_tir_delegation(tir: &TFunc, sig: &str, fwd: &str, has_return: bool, cx: &Cx, out: &mut String) {
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
        } => {
            out.push_str(&format!(
                "{}{} {}{} = {};\n",
                pad,
                kw,
                mangle(name),
                ty_clause,
                emit_tir_expr(init, cx),
            ));
        }
        TStmt::Assign { place, op, value, clone_value } => {
            let v = emit_tir_expr(value, cx);
            // c150: append `.clone()` when the value is a borrowed non-scalar (computed
            // at lowering). `({v}).clone()` matches how other clone sites in the AST
            // path parenthesise the receiver before the method call.
            let v = if *clone_value { format!("({}).clone()", v) } else { v };
            match op {
                Some(op) => out.push_str(&format!("{}{} {}= {};\n", pad, place, op.spell(), v)),
                None => out.push_str(&format!("{}{} = {};\n", pad, place, v)),
            }
        }
        // c109 Phase 23: tuple destructure. Mirrors `emit_stmt`'s `BindPattern::Tuple`
        // arm byte-for-byte: borrow the init into a temp, then bind each element from a
        // cloned canonical field of it.
        TStmt::TupleDestructure { tmp, init, kw, binds } => {
            out.push_str(&format!("{}let {} = &({});\n", pad, tmp, emit_tir_expr(init, cx)));
            for (elem_rust, field_rust) in binds {
                out.push_str(&format!(
                    "{}{} {} = ({}).{}.clone();\n",
                    pad, kw, elem_rust, tmp, field_rust
                ));
            }
        }
        // c109: struct destructure. Mirrors `emit_stmt`'s `BindPattern::Struct` arm
        // byte-for-byte: borrow the init into a temp, then bind each field from a
        // cloned field of it (the field name is both the local and the `.field` read).
        TStmt::StructDestructure { tmp, init, kw, fields } => {
            out.push_str(&format!("{}let {} = &({});\n", pad, tmp, emit_tir_expr(init, cx)));
            for field_rust in fields {
                out.push_str(&format!(
                    "{}{} {} = ({}).{}.clone();\n",
                    pad, kw, field_rust, tmp, field_rust
                ));
            }
        }
        // c109 Phase 26: list destructure. Mirrors `emit_stmt`'s `BindPattern::List`
        // arm byte-for-byte: borrow the init into a temp, then bind each element via
        // the runtime bounds-checked `jet_unpack_vec(tmp, want, i, file, line)` move.
        // `{file:?}` reproduces the AST's debug-formatted path; `kw`/`want`/`line` were
        // all resolved at lowering.
        TStmt::ListDestructure { tmp, init, kw, want, file, line, elems } => {
            out.push_str(&format!("{}let {} = &({});\n", pad, tmp, emit_tir_expr(init, cx)));
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
        TStmt::ViewReturn { value, wrap } => {
            let v = emit_tir_expr(value, cx);
            let rendered = match wrap {
                ViewWrap::Addr => format!("&{}", v),
                ViewWrap::Bare => v,
            };
            out.push_str(&format!("{}return {};\n", pad, rendered));
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
                    out.push_str(&format!("{}{{ let mut _jet_stdin_h = {};\n", pad, collection_str));
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
                        let iter_form = if *columnar {
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
            out.push_str(&format!("{}{}jet_term_enter();\n", inner_pad, cx.root_prefix));
            out.push_str(&format!(
                "{}let _live_guard = {}jet_scope_guard(|| {{ {}jet_term_leave(); }});\n",
                inner_pad, cx.root_prefix, cx.root_prefix
            ));
            emit_tir_stmts(body, cx, out, inner);
            out.push_str(&format!("{}}}\n", pad));
        }
        TStmt::Region(body) => {
            out.push_str(&format!("{}{{\n", pad));
            emit_tir_stmts(body, cx, out, indent + 1);
            out.push_str(&format!("{}}}\n", pad));
        }
        // D-TXN1–D-TXN4 (ratified 2026-06-24): `#Transact(name) { … }` block — open a
        // transaction guard, emit the body, then `commit()` on the clean fall-through
        // path. An early `?`/`return` skips `commit()`, so registered `on_commit` hooks
        // drop un-run (D-TXN3). Codegen is dumb (I3): no effect/rollback machinery here.
        TStmt::Transact { handle, snapshots, body } => {
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
        // RAII guard per field (declaration order) BEFORE the body, byte-for-byte
        // `emit_stmts`'s `Stmt::ContextBlock` arm.
        TStmt::ContextBlock { guards, body } => {
            out.push_str(&format!("{}{{\n", pad));
            let inner = indent + 1;
            let inner_pad = "    ".repeat(inner);
            for (i, (is_alloc, value)) in guards.iter().enumerate() {
                let val = emit_tir_expr(value, cx);
                if *is_alloc {
                    out.push_str(&format!(
                        "{}let _ctx_guard_{} = jet_mem::jet_ctx_push_alloc(&{});\n",
                        inner_pad, i, val
                    ));
                } else {
                    out.push_str(&format!(
                        "{}let _ctx_logger_{} = {};\n",
                        inner_pad, i, val
                    ));
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
                out.push_str(&format!("{}{} {} {{\n", inner_pad, kw, cond));
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
        // c109 Phase 24: a comptime const inlined verbatim (the pre-rendered value).
        TExprKind::ConstInline(val) => val.clone(),
        TExprKind::Print(arg) => {
            format!("println!(\"{{}}\", ({}).jet_show())", emit_tir_expr(arg, cx))
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
        // D-SIMD2 / D-LINALG1: a math constructor / static method → the prelude free
        // function `{root}jet_math_<T>_<func>(args)`. Args are plain values (floats or
        // a `[T#N]` array) — no borrow/clone decisions.
        TExprKind::MathBuiltin { type_name, func, args } => {
            let parts: Vec<String> = args.iter().map(|a| emit_tir_expr(a, cx)).collect();
            format!(
                "{}jet_math_{}_{}({})",
                cx.root_prefix,
                type_name,
                func,
                parts.join(", ")
            )
        }
        // c109 Phase 6: the synthetic `.clone()`. Mirrors `emit_method_call`'s
        // `clone` early return: `(recv).clone()`, no deref/borrow decision (the
        // receiver was already lowered to the place the AST path would clone).
        TExprKind::Clone(recv) => {
            format!("({}).clone()", emit_tir_expr(recv, cx))
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
            format!("(({}).{})({})", emit_tir_expr(recv, cx), field_rust, arg_str)
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
            let a = |i: usize| args.get(i).map(|e| emit_tir_expr(e, cx)).unwrap_or_default();
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
                    recv, a(0), cx.file, line
                ),
                TBuiltinOp::GetMap => format!("({}).get(&({}).clone()).cloned()", recv, a(0)),
                TBuiltinOp::GetList => format!("({}).get({} as usize).cloned()", recv, a(0)),
                TBuiltinOp::First => format!("({}).first().cloned()", recv),
                TBuiltinOp::Last => format!("({}).last().cloned()", recv),
                TBuiltinOp::Contains => format!("({}).contains(&{})", recv, a(0)),
                TBuiltinOp::IndexOf => format!(
                    "({}).iter().position(|x| *x == {}).map(|i| i as i64)",
                    recv, a(0)
                ),
                TBuiltinOp::Reverse => format!("({}).reverse()", recv),
                TBuiltinOp::Sort => format!("({}).sort()", recv),
                TBuiltinOp::JoinSep => format!(
                    "({}).iter().map(|x| x.jet_show()).collect::<Vec<_>>().join(({}).as_str())",
                    recv, a(0)
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
                    format!("({}).trim().parse::<i64>().map_err(|e| e.to_string())", recv)
                }
                TBuiltinOp::StartsWith => format!("({}).starts_with(&{})", recv, a(0)),
                TBuiltinOp::EndsWith => format!("({}).ends_with(&{})", recv, a(0)),
                TBuiltinOp::Replace => format!("({}).replace(&{}, &{})", recv, a(0), a(1)),
                TBuiltinOp::ToUpper => format!("({}).to_uppercase()", recv),
                TBuiltinOp::ToLower => format!("({}).to_lowercase()", recv),
                TBuiltinOp::Repeat => format!("({}).repeat({} as usize)", recv, a(0)),
                TBuiltinOp::Slice { line } => format!(
                    "jet_string_slice(&({}), {}, {}, {:?}, {})",
                    recv, a(0), a(1), cx.file, line
                ),
                TBuiltinOp::Keys => {
                    format!("({}).keys().cloned().collect::<Vec<_>>()", recv)
                }
                TBuiltinOp::Values => {
                    format!("({}).values().cloned().collect::<Vec<_>>()", recv)
                }
                TBuiltinOp::ContainsKey => format!("({}).contains_key(&{})", recv, a(0)),
                TBuiltinOp::ToString => format!("({}).jet_show()", recv),
                // c109 Phase 24: `Match.group(n)` → indexing into the `Vec<Option<String>>`.
                TBuiltinOp::MatchGroup => {
                    format!("({}).get(({}) as usize).cloned().flatten()", recv, a(0))
                }
                // D-COLLBREADTH1=A: Set<T> operations.
                TBuiltinOp::SetFrom => {
                    format!("({}).into_iter().collect::<std::collections::HashSet<_>>()", recv)
                }
                TBuiltinOp::SetInsert => format!("({}).insert({})", recv, a(0)),
                TBuiltinOp::SetRemove => format!("{{({}).remove(&{});}}", recv, a(0)),
                TBuiltinOp::SetToList => {
                    format!("({}).iter().cloned().collect::<Vec<_>>()", recv)
                }
                TBuiltinOp::SetUnion => format!(
                    "({}).union(&({})).cloned().collect::<std::collections::HashSet<_>>()",
                    recv, a(0)
                ),
                // D-COLLBREADTH1=A: Deque<T> operations.
                TBuiltinOp::DequePushFront => format!("({}).push_front({})", recv, a(0)),
                TBuiltinOp::DequePushBack => format!("({}).push_back({})", recv, a(0)),
                TBuiltinOp::DequePopFront => format!("({}).pop_front()", recv),
                TBuiltinOp::DequePopBack => format!("({}).pop_back()", recv),
                TBuiltinOp::DequePeekFront => format!("({}).front().cloned()", recv),
                TBuiltinOp::DequePeekBack => format!("({}).back().cloned()", recv),
                TBuiltinOp::TryCollect => format!("jet_list_try_collect(({}).clone())", recv),
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
                    recv, a(0), tuple_struct
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
                TNumericOp::CastAs { dst_rust } => format!("(({}) as {})", recv, dst_rust),
                TNumericOp::TryFrom { dst_rust, dst_spelling } => format!(
                    "<{dst_rust}>::try_from(({recv}) as i128).map_err(|_| \
                     \"value doesn't fit in {dst_spelling}\".to_string())"
                ),
            }
        }
        // c109 Phase 28: an overflow opt-out builtin. `prefix`/`op` were resolved at
        // lowering; reproduce `emit_call`'s `(ls).{name}_{suffix}(rs)` byte-for-byte.
        TExprKind::OverflowOpt { prefix, op, lhs, rhs } => {
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
        TExprKind::CoreCall { module, method, args } => {
            emit_tir_core_call(module, method, args, &e.ty, cx)
        }
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
                    BinOp::Shl => format!("({}).jet_shl(({}) as i128, {:?}, {})", ls, rs, file, line),
                    BinOp::Shr => format!("({}).jet_shr(({}) as i128, {:?}, {})", ls, rs, file, line),
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
        TExprKind::Unary { op, operand } => {
            let i = emit_tir_expr(operand, cx);
            match op {
                UnOp::Neg => format!("(-({}))", i),
                UnOp::Not => format!("(!({}))", i),
            }
        }
        // c109 Phase 3: `user_S { f: v, … }`. The Rust head and mangled field
        // names were resolved at lowering; values format like any other node.
        TExprKind::StructLit { rust_type, fields, extra, as_trait } => {
            let mut parts = fields
                .iter()
                .map(|(field_rust, v, boxed)| {
                    let value = emit_tir_expr(v, cx);
                    // c109: a boxed (self-referential) field is wrapped `Box::new(…)`,
                    // exactly as `emit_struct_lit`. The `boxed` flag is total (resolved
                    // at lowering from `cx.boxed_edges`).
                    let value = if *boxed { format!("Box::new({})", value) } else { value };
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
        TExprKind::Field { recv, field_rust, boxed } => {
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
            format!("(({}) as usize as *mut {})", emit_tir_expr(addr, cx), elem_rust)
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
                        format!("{}::{}(({}).into_iter().collect())", prefix, variant, arg_str)
                    } else {
                        format!("{}::{}({})", prefix, variant, arg_str)
                    }
                }
            }
        },
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
        TExprKind::ColumnarColumnRead { base, index, column_rust, line } => {
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
        TExprKind::TupleLit { struct_name, fields } => {
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
                let mut s =
                    String::from("{ let mut _m = std::collections::BTreeMap::new(); ");
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
        TExprKind::Index { base, index, is_map, line } => {
            let b = emit_tir_expr(base, cx);
            let i = emit_tir_expr(index, cx);
            if *is_map {
                format!("jet_index_map(&({}), &({}), {:?}, {})", b, i, cx.file, line)
            } else {
                format!("jet_index_vec(&({}), {}, {:?}, {})", b, i, cx.file, line)
            }
        }
        // D-SIMD2: `v[i]` lane read → the bounds-checked prelude helper.
        TExprKind::MathLaneIndex { lane_ty, base, index, line } => {
            let b = emit_tir_expr(base, cx);
            let i = emit_tir_expr(index, cx);
            format!(
                "{}jet_math_{}_lane(&({}), {}, {:?}, {})",
                cx.root_prefix, lane_ty, b, i, cx.file, line
            )
        }
        // c109 Phase 5: `coll[a..b]` → `jet_slice_vec`. Mirrors the AST `Expr::Slice`.
        TExprKind::Slice { base, start, end, line } => {
            let b = emit_tir_expr(base, cx);
            let a = emit_tir_expr(start, cx);
            let e = emit_tir_expr(end, cx);
            format!("jet_slice_vec(&({}), {}, {}, {:?}, {})", b, a, e, cx.file, line)
        }
        // c109 Phase 8: `value(x)` → `Some(x)` / `null` → `None`. Mirrors the AST
        // `Expr::Present`/`Expr::Absent` exactly.
        TExprKind::Present(inner) => format!("Some({})", emit_tir_expr(inner, cx)),
        TExprKind::Absent => "None".to_string(),
        // c109 Phase 23: a `#Todo` typed hole → diverging `todo!(…)`. Byte-for-byte the
        // AST `Expr::Todo` arm (Expression.rs): file/line/expected-type baked into the
        // panic string. `cx.file` is program-level (read here, like every other use).
        TExprKind::Todo { line, expected_type } => format!(
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
        TExprKind::Try { inner, convert, file, line, fn_name } => {
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
        TExprKind::OrFallback { value, fallback, is_option } => {
            let v = emit_tir_expr(value, cx);
            let fb = emit_tir_orfallback_rhs(fallback, cx);
            if *is_option {
                format!("match {} {{ Some(__jet_v) => __jet_v, None => {} }}", v, fb)
            } else {
                format!("match {} {{ Ok(__jet_ok) => __jet_ok, Err(_) => {} }}", v, fb)
            }
        }
        // c109 Phase 8: optional chaining `base?.member`. Mirrors `Expr::OptField`:
        // `(base).clone().{and_then|map}(|__optv| __optv.{member})`. The combinator is
        // the total `flatten` fact (flatten → `and_then`, else → `map`).
        TExprKind::OptField { base, member_rust, flatten } => {
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
        // c109 Phase 11: a closure-taking collection method. The receiver-type +
        // Fn-vs-FnMut dispatch was resolved into `op` at lowering; emit only formats,
        // reproducing `emit_builtin_method`'s closure arms byte-for-byte. Args (the
        // lambda + any seed) are emitted PLAINLY (raw `arg(i)`).
        TExprKind::ClosureMethod { recv, op, args } => {
            let recv = emit_tir_expr(recv, cx);
            let a = |i: usize| args.get(i).map(|e| emit_tir_expr(e, cx)).unwrap_or_default();
            match op {
                TClosureOp::Map => format!("jet_list_map(({}).clone(), {})", recv, a(0)),
                TClosureOp::MapMut => format!("jet_list_map_mut(({}).clone(), {})", recv, a(0)),
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
                TClosureOp::ParFilter => format!("jet_list_par_filter(({}).clone(), {})", recv, a(0)),
                TClosureOp::ParFold => format!("jet_list_par_fold(({}).clone(), {}, {})", recv, a(0), a(1)),
                TClosureOp::Scan => {
                    format!("jet_list_scan(({}).clone(), {}, {})", recv, a(0), a(1))
                }
                TClosureOp::Fold => {
                    format!("jet_list_fold(({}).clone(), {}, {})", recv, a(0), a(1))
                }
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
                TClosureOp::Partition { tuple_struct } => {
                    // `partition` passes each element by value (T: Clone).
                    // The lambda `f` takes T by value, but `Iterator::partition`
                    // passes `&T` to its predicate. Use jet_list_partition helper.
                    format!(
                        "jet_list_partition(({}).clone(), {}, |__t, __f| \
                         {} {{ user_false_: __f, user_true_: __t }})",
                        recv, a(0), tuple_struct
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
            let a = |i: usize| args.get(i).map(|e| emit_tir_expr(e, cx)).unwrap_or_default();
            let root = &cx.root_prefix;
            match op {
                THandleOp::FileReaderReadLine => {
                    format!("{}jet_std_file_reader_read_line(&mut ({}))", root, recv)
                }
                THandleOp::FileWriterWriteLine => format!(
                    "{}jet_std_file_writer_write_line(&mut ({}), &({}))",
                    root, recv, a(0)
                ),
                THandleOp::FileWriterFlush => {
                    format!("{}jet_std_file_writer_flush(&mut ({}))", root, recv)
                }
                THandleOp::StdinReadLine => {
                    format!("{}jet_std_io_stdin_read_line(&mut ({}))", root, recv)
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
                THandleOp::RngBool => format!("{}jet_rng_bool(&mut ({}))", root, recv),
                THandleOp::RngPick => {
                    format!("{}jet_rng_pick(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::RngShuffle => {
                    format!("{}jet_rng_shuffle(&mut ({}), &mut ({}))", root, recv, a(0))
                }
                THandleOp::DurationMillis => {
                    format!("{}jet_duration_millis(&({}))", root, recv)
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
                THandleOp::ArgsSpecFlag => format!(
                    "{}jet_args_flag({}, &({}), &({}))",
                    root, recv, a(0), a(1)
                ),
                THandleOp::ArgsSpecOption => format!(
                    "{}jet_args_option({}, &({}), &({}), &({}))",
                    root, recv, a(0), a(1), a(2)
                ),
                THandleOp::ArgsSpecPositional => format!(
                    "{}jet_args_positional({}, &({}), &({}))",
                    root, recv, a(0), a(1)
                ),
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
                THandleOp::ParsedArgsPositional => {
                    format!("{}jet_parsed_positional(&({}), {})", root, recv, a(0))
                }
                THandleOp::TaskJoin => format!("({}).join()", recv),
                THandleOp::TaskDetach => format!("{{ let _detach = ({}); }}", recv),
                THandleOp::ChannelReceive => format!("({}).receive()", recv),
                THandleOp::ChannelSender => format!("({}).sender()", recv),
                THandleOp::SenderSend => format!("({}).send({})", recv, a(0)),
                // D-REACT1=B: reactive Signal/Derived reads and writes.
                THandleOp::ReactiveGet => format!("({}).get()", recv),
                THandleOp::ReactiveSet => format!("({}).set({})", recv, a(0)),
                // D-HONESTNUM1=A: Measurement<Float> arithmetic + accessors.
                THandleOp::MeasurementMethod { method } => {
                    if args.is_empty() {
                        format!("({}).{}()", recv, method)
                    } else {
                        format!("({}).{}({})", recv, method, a(0))
                    }
                }
                // D-PENDING1=B: Loadable<T,E> methods.
                THandleOp::LoadableMethod { method } => {
                    if args.is_empty() {
                        format!("({}).{}()", recv, method)
                    } else {
                        format!("({}).{}({})", recv, method, a(0))
                    }
                }
                // D-TIMEDEPTH1=A: civil-time method call.
                THandleOp::CivilTimeMethod { kind: _, method } => {
                    match method.as_str() {
                        "add_days" => format!("({}).add_days({})", recv, a(0)),
                        "add_months" => format!("({}).add_months({})", recv, a(0)),
                        "diff_days" => format!("({}).diff_days(&({}))", recv, a(0)),
                        "to_string" => format!("({}).to_string_fmt()", recv),
                        _ => {
                            if args.is_empty() {
                                format!("({}).{}()", recv, method)
                            } else {
                                format!("({}).{}({})", recv, method, a(0))
                            }
                        }
                    }
                }
                // D-APPROX1=A: sketch method call. `add` args may be string borrows;
                // `count`/`quantile` pass by value; `sample` returns Vec<String>.
                THandleOp::SketchMethod { sketch, method } => {
                    match method.as_str() {
                        "add" if sketch == "TDigest" => format!("({}).add({})", recv, a(0)),
                        "add" if sketch == "ReservoirSampler" => format!("({}).add(({}).clone())", recv, a(0)),
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
                THandleOp::HttpRouterRegister { verb, handler } => format!(
                    "{}jet_http_router_register(&mut ({}), \"{}\".to_string(), {}, {})",
                    root, recv, verb, a(0), handler
                ),
                // D-SIMD2 / D-LINALG1: a math-type instance method → the prelude free
                // function `jet_math_<type>_<method>(&(recv), <args>)`. `reduce`
                // dispatches on the validated marker op. All take `&recv` (immutable;
                // these types are value semantics — every op returns a fresh value).
                THandleOp::MathMethod { type_name, method, reduce_op } => {
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
                THandleOp::DataTreeAt    => format!("({}).at({})", recv, a(0)),
                THandleOp::DataTreeInt   => format!("({}).int()", recv),
                THandleOp::DataTreeText  => format!("({}).text()", recv),
                THandleOp::DataTreeBool  => format!("({}).bool()", recv),
                THandleOp::DataTreeFloat => format!("({}).float()", recv),
                // D-SERDE-ACCESS=B: same accessors on Json/Data.
                THandleOp::JsonField => format!("({}).field(&({}))", recv, a(0)),
                THandleOp::JsonAt    => format!("({}).at({})", recv, a(0)),
                THandleOp::JsonInt   => format!("({}).int()", recv),
                THandleOp::JsonText  => format!("({}).text()", recv),
                THandleOp::JsonBool  => format!("({}).bool()", recv),
                THandleOp::JsonFloat => format!("({}).float()", recv),
                // D-PATHFS1: Path object methods.
                THandleOp::PathFrom        => format!("{}jet_path_from(&({}))", root, recv),
                THandleOp::PathJoin        => format!("{}jet_path_join(&({}), &({}))", root, recv, a(0)),
                THandleOp::PathParent      => format!("{}jet_path_parent(&({}))", root, recv),
                THandleOp::PathExtension   => format!("{}jet_path_extension(&({}))", root, recv),
                THandleOp::PathStem        => format!("{}jet_path_stem(&({}))", root, recv),
                THandleOp::PathToString    => format!("({}).jet_show()", recv),
                THandleOp::PathWriteAtomic => format!("{}jet_path_write_atomic(&({}), &({}))", root, recv, a(0)),
                THandleOp::PathWalk        => format!("{}jet_path_walk(&({}))", root, recv),
            }
        }
        // c109 Phase 13: a closure-taking core call. The closure was rendered at
        // lowering; emit assembles the bespoke shape, byte-for-byte `emit_core_call`
        // (Source/Codegen/Expression.rs).
        TExprKind::CoreClosureCall { kind } => match kind {
            TCoreClosureKind::Spawn { spawn_closure } => {
                format!("{}jet_std::JetTask::spawn({})", cx.root_prefix, spawn_closure)
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
                format!("{}jet_std::jet_reactive_effect({})", cx.root_prefix, closure)
            }
        },
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
pub(crate) fn emit_tir_core_call(module: &str, method: &str, args: &[TExpr], ret_ty: &Type, cx: &Cx) -> String {
    let arg = |i: usize| args.get(i).map(|e| emit_tir_expr(e, cx)).unwrap_or_default();
    let helper = |name: &str| format!("{}{}", cx.root_prefix, name);
    let regex_fn = |name: &str| {
        let crate_name = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
        format!("{}::{}", crate_name, name)
    };
    match (module, method) {
        // c109 Phase 18 (S58, E2-M13): low-level pointer ops, byte-for-byte
        // `emit_core_call`. `address_of` is an inert address cast (no `unsafe`);
        // `volatile_read` reads through a `Ptr<T>` — `read_volatile` is valid because the
        // call only reaches codegen inside an `#Unsafe` region/fn (sema E3101), already
        // lowered to a Rust `unsafe` context.
        ("core.mem", "address_of") => format!("(&({}) as *const _ as usize as i64)", arg(0)),
        ("core.mem", "volatile_read") => format!("std::ptr::read_volatile({})", arg(0)),
        // c109 Phase 21: the `tasks.channel()` producer, byte-for-byte `emit_core_call`.
        ("core.tasks", "channel") => format!("{}jet_std::JetChannel::new()", cx.root_prefix),
        // D-REACT1=B: `reactive.signal(initial)` producer → a `JetSignal<T>`.
        ("jet.reactive", "signal") => {
            format!("{}jet_std::JetSignal::new({})", cx.root_prefix, arg(0))
        }
        // D-HONESTNUM1=A: `M.from(value, uncertainty)` → a `JetMeasurement<f64>`.
        ("core.science.measurement", "from") => {
            format!("{}jet_std::JetMeasurement::new({}, {})", cx.root_prefix, arg(0), arg(1))
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
        ("core.fs", "read") => format!("{}(&({}))", helper("jet_std_fs_read"), arg(0)),
        ("core.fs", "read_bytes") => format!("{}(&({}))", helper("jet_std_fs_read_bytes"), arg(0)),
        ("core.fs", "write") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_write"),
            arg(0),
            arg(1)
        ),
        ("core.fs", "append") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_append"),
            arg(0),
            arg(1)
        ),
        ("core.fs", "exists") => format!("{}(&({}))", helper("jet_std_fs_exists"), arg(0)),
        ("core.fs", "remove") => format!("{}(&({}))", helper("jet_std_fs_remove"), arg(0)),
        ("core.fs", "list_dir") => format!("{}(&({}))", helper("jet_std_fs_list_dir"), arg(0)),
        ("core.fs", "create_dir") => format!("{}(&({}))", helper("jet_std_fs_create_dir"), arg(0)),
        ("core.fs", "is_dir") => format!("{}(&({}))", helper("jet_std_fs_is_dir"), arg(0)),
        ("core.fs", "copy") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_copy"),
            arg(0),
            arg(1)
        ),
        ("core.fs", "rename") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_rename"),
            arg(0),
            arg(1)
        ),
        ("core.io", "args") => format!("{}()", helper("jet_std_io_args")),
        // D-ARGS1: `args.spec()` → empty builder.
        ("core.args", "spec") => format!("{}()", helper("jet_args_spec")),
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
        ("core.env", "get") => format!("{}(&({}))", helper("jet_std_env_get"), arg(0)),
        ("core.env", "set") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_env_set"),
            arg(0),
            arg(1)
        ),
        ("core.env", "current_dir") => format!("{}()", helper("jet_std_env_current_dir")),
        ("core.env", "home_dir") => format!("{}()", helper("jet_std_env_home_dir")),
        ("core.process", "exit") => format!("{}({})", helper("jet_std_process_exit"), arg(0)),
        ("core.process", "run") => format!("{}(&({}))", helper("jet_std_process_run"), arg(0)),
        // D-FLOATW1: width-generic math — choose the f32 helper when the arg is F32.
        ("core.math", "sqrt") => {
            let f32_path = matches!(args.first().map(|a| &a.ty), Some(Type::Float32));
            if f32_path { format!("{}({})", helper("jet_std_math_sqrt_f32"), arg(0)) }
            else { format!("{}({})", helper("jet_std_math_sqrt"), arg(0)) }
        }
        ("core.math", "pow") => {
            let f32_path = matches!(args.first().map(|a| &a.ty), Some(Type::Float32));
            if f32_path { format!("{}({}, {})", helper("jet_std_math_pow_f32"), arg(0), arg(1)) }
            else { format!("{}({}, {})", helper("jet_std_math_pow"), arg(0), arg(1)) }
        }
        ("core.math", "floor") => {
            let f32_path = matches!(args.first().map(|a| &a.ty), Some(Type::Float32));
            if f32_path { format!("{}({})", helper("jet_std_math_floor_f32"), arg(0)) }
            else { format!("{}({})", helper("jet_std_math_floor"), arg(0)) }
        }
        ("core.math", "ceil") => {
            let f32_path = matches!(args.first().map(|a| &a.ty), Some(Type::Float32));
            if f32_path { format!("{}({})", helper("jet_std_math_ceil_f32"), arg(0)) }
            else { format!("{}({})", helper("jet_std_math_ceil"), arg(0)) }
        }
        ("core.math", "round") => format!("{}({})", helper("jet_std_math_round"), arg(0)),
        ("core.random", "int") => {
            format!("{}({}, {})", helper("jet_std_random_int"), arg(0), arg(1))
        }
        ("core.random", "float") => format!("{}()", helper("jet_std_random_float")),
        ("core.random", "seed") => format!("{}({})", helper("jet_std_random_seed"), arg(0)),
        // D-DET1: deterministic injected RNG capability constructor.
        ("core.random", "rng") => format!("{}({})", helper("jet_std_rng_new"), arg(0)),
        ("core.time", "now") => format!("{}()", helper("jet_std_time_now")),
        ("core.time", "sleep") => format!("{}({})", helper("jet_std_time_sleep"), arg(0)),
        ("core.time", "start") => format!("{}()", helper("jet_std_time_start")),
        // D-DET1: deterministic injected Clock capability constructor.
        ("core.time", "clock") => format!("{}({})", helper("jet_std_clock_new"), arg(0)),
        // D-DET-CAPAPI: `Duration` constructors — pure value, ms/secs → ms span.
        ("core.time", "ms") => format!("{}({})", helper("jet_std_duration_ms"), arg(0)),
        ("core.time", "secs") => format!("{}({})", helper("jet_std_duration_secs"), arg(0)),
        // D-ENC1 + D-JSONVERB1 + D-SERDE6: unified `core.encoding.*`. The dynamic forms
        // (`Json` tree / `[[String]]` / `Map`) keep their existing helpers; the typed
        // forms route through the Encode/Decode model, distinguished by the lowered arg
        // type (encode) or the resolved return type (decode). `is_json_value` etc. read
        // those total facts — codegen never re-infers (I3).
        ("core.encoding.json", "parse") => format!("{}(&({}))", helper("jet_std_json_parse"), arg(0)),
        ("core.encoding.json", "decode") => {
            if enc_ok_is_json(ret_ty) {
                format!("{}(&({}))", helper("jet_std_json_decode_lenient"), arg(0))
            } else {
                format!("{}::<{}>(&({}))", helper("jet_enc_json_decode"), enc_target_rust(ret_ty, cx), arg(0))
            }
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
        ("core.encoding.csv", "parse") => format!("{}(&({}))", helper("jet_ring_csv_parse"), arg(0)),
        ("core.encoding.csv", "decode") => {
            format!("{}::<{}>(&({}))", helper("jet_enc_csv_decode"), enc_target_rust(ret_ty, cx), arg(0))
        }
        ("core.encoding.csv", "to_string") => {
            if enc_arg_is_string_rows(args) {
                format!("{}(&({}))", helper("jet_ring_csv_render"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_enc_csv_to_string"), arg(0))
            }
        }
        ("core.encoding.toml", "parse") => format!("{}(&({}))", helper("jet_std_toml_parse"), arg(0)),
        ("core.encoding.toml", "decode") => {
            format!("{}::<{}>(&({}))", helper("jet_enc_toml_decode"), enc_target_rust(ret_ty, cx), arg(0))
        }
        ("core.encoding.toml", "to_string") => {
            if enc_arg_is_json(args) {
                format!("{}(&({}))", helper("jet_std_toml_render"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_enc_toml_to_string"), arg(0))
            }
        }
        ("core.encoding.yaml", "parse") => format!("{}(&({}))", helper("jet_std_yaml_parse"), arg(0)),
        ("core.encoding.yaml", "decode") => {
            format!("{}::<{}>(&({}))", helper("jet_enc_yaml_decode"), enc_target_rust(ret_ty, cx), arg(0))
        }
        ("core.encoding.yaml", "to_string") => {
            if enc_arg_is_json(args) {
                format!("{}(&({}))", helper("jet_std_yaml_render"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_enc_yaml_to_string"), arg(0))
            }
        }
        // D-UUIDENC1=A: hex and base64 encode/decode.
        ("core.encoding.hex", "encode") => format!("{}(&({}))", helper("jet_std_hex_encode"), arg(0)),
        ("core.encoding.hex", "decode") => format!("{}(&({}))", helper("jet_std_hex_decode"), arg(0)),
        ("core.encoding.base64", "encode") => format!("{}(&({}))", helper("jet_std_b64_encode"), arg(0)),
        ("core.encoding.base64", "decode") => format!("{}(&({}))", helper("jet_std_b64_decode"), arg(0)),
        // D-UUIDENC1=A: UUID v4 (CSPRNG) and v7 (injectable Clock).
        ("core.uuid", "v4") => format!("{}()", helper("jet_std_uuid_v4")),
        ("core.uuid", "v7") => format!("{}(&({}))", helper("jet_std_uuid_v7"), arg(0)),
        // E2-M7: streaming file handles (D-IO2).
        ("core.files", "open") => format!("{}(&({}))", helper("jet_std_files_open"), arg(0)),
        ("core.files", "create") => format!("{}(&({}))", helper("jet_std_files_create"), arg(0)),
        ("core.files", "append") => format!("{}(&({}))", helper("jet_std_files_append"), arg(0)),
        // E2-M7: std.path helpers (D-IO1).
        ("core.path", "join") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_path_join"), arg(0), arg(1)
        ),
        ("core.path", "parent") => format!("{}(&({}))", helper("jet_std_path_parent"), arg(0)),
        ("core.path", "extension") => format!("{}(&({}))", helper("jet_std_path_extension"), arg(0)),
        ("core.path", "normalize") => format!("{}(&({}))", helper("jet_std_path_normalize"), arg(0)),
        // E2-M9: first-party ring packages.
        ("jet.log", "info") => format!("{}(&({}))", helper("jet_ring_log_info"), arg(0)),
        ("jet.log", "warn") => format!("{}(&({}))", helper("jet_ring_log_warn"), arg(0)),
        ("jet.log", "error") => format!("{}(&({}))", helper("jet_ring_log_error"), arg(0)),
        ("jet.log", "debug") => format!("{}(&({}))", helper("jet_ring_log_debug"), arg(0)),
        ("jet.log", "set_level") => format!("{}(&({}))", helper("jet_ring_log_set_level"), arg(0)),
        // E2-M12 D-OBS3: trace context for structured log records.
        ("jet.log", "set_trace_id") => format!("{}(&({}))", helper("jet_ring_log_set_trace_id"), arg(0)),
        // D-LOGFMT1=A: explicit log format override.
        ("jet.log", "setup") => format!("{}(&({}))", helper("jet_ring_log_setup"), arg(0)),
        ("jet.time", "now") => format!("{}()", helper("jet_std_time_now")),
        ("jet.time", "format") => format!("{}({}, &({}))", helper("jet_ring_time_format"), arg(0), arg(1)),
        ("jet.crypto", "sha256") => format!("{}(&({}))", helper("jet_ring_crypto_sha256"), arg(0)),
        ("jet.crypto", "sha256_bytes") => format!("{}(&({}))", helper("jet_ring_crypto_sha256_bytes"), arg(0)),
        // E2-M10: core.net — blocking TCP sockets.
        ("core.net", "tcp_listen") => format!("{}(&({}))", helper("jet_net_tcp_listen"), arg(0)),
        ("core.net", "tcp_accept") => format!("{}(&({}))", helper("jet_net_tcp_accept"), arg(0)),
        ("core.net", "tcp_connect") => format!("{}(&({}))", helper("jet_net_tcp_connect"), arg(0)),
        ("core.net", "tcp_read") => format!("{}(&mut ({}))", helper("jet_net_tcp_read"), arg(0)),
        ("core.net", "tcp_write") => {
            format!("{}(&mut ({}), &({}))", helper("jet_net_tcp_write"), arg(0), arg(1))
        }
        ("core.net", "tcp_local_addr") => format!("{}(&({}))", helper("jet_net_tcp_local_addr"), arg(0)),
        ("core.net", "tcp_peer_addr") => format!("{}(&({}))", helper("jet_net_tcp_peer_addr"), arg(0)),
        ("core.net", "set_timeout") => {
            format!("{}(&mut ({}), {})", helper("jet_net_set_timeout"), arg(0), arg(1))
        }
        ("core.net", "tcp_reply") => {
            format!("{}({}, &({}), &({}))", helper("jet_net_tcp_reply"), arg(0), arg(1), arg(2))
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
            helper("jet_http_router_dispatch"), arg(0), arg(1)
        ),
        // D-REGEX1: jet.regex — calls land in the FFI bridge crate.
        ("jet.regex", "is_match") => {
            format!("{}(&({}), &({}))", regex_fn("jet_regex_is_match"), arg(0), arg(1))
        }
        ("jet.regex", "match") => {
            format!("{}(&({}), &({}))", regex_fn("jet_regex_match"), arg(0), arg(1))
        }
        ("jet.regex", "find") => {
            format!("{}(&({}), &({}))", regex_fn("jet_regex_find"), arg(0), arg(1))
        }
        ("jet.regex", "find_all") => {
            format!("{}(&({}), &({}))", regex_fn("jet_regex_find_all"), arg(0), arg(1))
        }
        ("jet.regex", "split") => {
            format!("{}(&({}), &({}))", regex_fn("jet_regex_split"), arg(0), arg(1))
        }
        ("jet.regex", "replace") => format!(
            "{}(&({}), &({}), &({}))",
            regex_fn("jet_regex_replace"), arg(0), arg(1), arg(2)
        ),
        ("jet.regex", "replace_all") => format!(
            "{}(&({}), &({}), &({}))",
            regex_fn("jet_regex_replace_all"), arg(0), arg(1), arg(2)
        ),
        // D-DEP-ARCHIVE1=A: jet.archive — gzip compress/decompress via the FFI bridge crate.
        // Arguments are `[U8]` (Vec<u8>); bridge functions take `&[u8]` (auto-coerce from &Vec<u8>).
        ("jet.archive", "gzip_compress") => {
            format!("{}(&({}))", regex_fn("jet_archive_gzip_compress"), arg(0))
        }
        ("jet.archive", "gzip_decompress") => {
            format!("{}(&({}))", regex_fn("jet_archive_gzip_decompress"), arg(0))
        }
        // D-DEP-ARCHIVE1=A: jet.archive — zip compress/decompress via the `zip` crate FFI bridge.
        // zip_compress takes (&str, &[u8]); zip_decompress takes &[u8].
        ("jet.archive", "zip_compress") => {
            format!("{}(&({}), &({}))", regex_fn("jet_archive_zip_compress"), arg(0), arg(1))
        }
        ("jet.archive", "zip_decompress") => {
            format!("{}(&({}))", regex_fn("jet_archive_zip_decompress"), arg(0))
        }
        // D-DEP-ARCHIVE1=A: tar_add / tar_get / tar_names_json via the FFI bridge.
        // All three take &[u8] / &str args (non-scalar → borrow); none take scalars.
        ("jet.archive", "tar_add") => {
            format!("{}(&({}), &({}), &({}))", regex_fn("jet_archive_tar_add"), arg(0), arg(1), arg(2))
        }
        ("jet.archive", "tar_get") => {
            format!("{}(&({}), &({}))", regex_fn("jet_archive_tar_get"), arg(0), arg(1))
        }
        ("jet.archive", "tar_names_json") => {
            format!("{}(&({}))", regex_fn("jet_archive_tar_names_json"), arg(0))
        }
        // D-DEP-DB1: jet.db — SQLite via the FFI bridge crate.
        // The u64 handle is a scalar — passed by value (no &). String/slice args get &.
        ("jet.db", "open") => {
            format!("{}(&({}))", regex_fn("jet_db_open"), arg(0))
        }
        ("jet.db", "open_memory") => {
            format!("{}()", regex_fn("jet_db_open_memory"))
        }
        ("jet.db", "close") => {
            format!("{}({})", regex_fn("jet_db_close"), arg(0))
        }
        ("jet.db", "exec") => {
            format!("{}({}, &({}))", regex_fn("jet_db_exec"), arg(0), arg(1))
        }
        ("jet.db", "query_json") => {
            format!("{}({}, &({}))", regex_fn("jet_db_query_json"), arg(0), arg(1))
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
        ("core.random", "shuffle") => {
            format!("{}(&mut ({}))", helper("jet_std_random_shuffle"), arg(0))
        }
        ("core.io", "eprint") => format!("eprintln!(\"{{}}\", ({}).jet_show())", arg(0)),
        // D-TERM1 (ratified 2026-06-22): terminal direct-input.
        ("core.term", "read_key") => format!("{}()", helper("jet_term_read_key")),
        // D-ADAPTFID1=A: adaptive fidelity signal — global atomic f32.
        ("core.perf", "fidelity") => format!("jet_perf_fidelity()"),
        ("core.perf", "set_fidelity") => format!("jet_perf_set_fidelity({})", arg(0)),
        // D-APPROX1=A: sketch constructors.
        ("core.sketch.hll", "new") => format!("JetHyperLogLog::new()"),
        ("core.sketch.tdigest", "new") => format!("JetTDigest::new()"),
        ("core.sketch.cms", "new") => format!("JetCountMinSketch::new()"),
        ("core.sketch.reservoir", "new") => format!("JetReservoirSampler::new({})", arg(0)),
        // D-TIMEDEPTH1=A: civil-time constructors.
        ("core.time.date", "new") => format!("JetDate::new({}, {}, {})", arg(0), arg(1), arg(2)),
        ("core.time.date", "today") => format!("JetDate::today_utc()"),
        ("core.time.date", "parse") => format!("JetDate::parse(&({})).map_err(|e| e)", arg(0)),
        ("core.time.datetime", "from_timestamp") => format!("JetDateTime::from_timestamp({})", arg(0)),
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
                s = format!("std::sync::Arc::clone(&{})", s);
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
            TStrPart::Interp(e) => {
                body.push_str(&format!("_jet_s.push_str(&({}).jet_show()); ", emit_tir_expr(e, cx)));
            }
        }
    }
    body.push_str("_jet_s }");
    body
}
