use crate::Codegen::Cx;
use crate::Codegen::mangle;
use crate::Codegen::user_type_rust;
use crate::Codegen::TIR::emit::emit_math_swizzle_assign_stmt;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::ScopeMemberKind;
use crate::Codegen::TIR::TForInMethod;
use crate::Codegen::TIR::TIfCond;
use crate::Codegen::TIR::TStmt;

#[derive(Clone)]
enum ActiveCleanup {
    Deferred(usize),
    Resource(String),
}

pub(crate) fn emit_tir_stmts(stmts: &[TStmt], cx: &Cx, out: &mut String, indent: usize) {
    let mut active_cleanups = Vec::new();
    emit_tir_stmts_inline(stmts, cx, out, indent, &mut active_cleanups);
}

/// Emit statements in the current lexical scope, retaining newly declared
/// deferred closes for following siblings.
fn emit_tir_stmts_inline(
    stmts: &[TStmt],
    cx: &Cx,
    out: &mut String,
    indent: usize,
    active_cleanups: &mut Vec<ActiveCleanup>,
) {
    for s in stmts {
        emit_tir_stmt(s, cx, out, indent, active_cleanups);
    }
}

/// Emit a nested lexical scope. Its deferred closes can see all enclosing
/// guards, but do not remain active after the nested block ends.
fn emit_tir_stmts_nested(
    stmts: &[TStmt],
    cx: &Cx,
    out: &mut String,
    indent: usize,
    inherited_cleanups: &[ActiveCleanup],
) {
    let mut active = inherited_cleanups.to_vec();
    emit_tir_stmts_inline(stmts, cx, out, indent, &mut active);
}

fn emit_cleanups_now(cleanups: &[ActiveCleanup], out: &mut String, indent: usize) {
    let pad = "    ".repeat(indent);
    for cleanup in cleanups.iter().rev() {
        match cleanup {
            ActiveCleanup::Deferred(id) => {
                out.push_str(&format!("{}_jet_deferred_close_{}.run();\n", pad, id));
            }
            ActiveCleanup::Resource(name) => {
                out.push_str(&format!("{}{}.close();\n", pad, name));
            }
        }
    }
}

fn emit_expr_with_cleanups(e: &crate::Codegen::TIR::TExpr, cx: &Cx, cleanups: &[ActiveCleanup]) -> String {
    let rendered = emit_tir_expr(e, cx);
    if !rendered.contains(crate::Codegen::TIR::RESOURCE_CLEANUP_MARKER) {
        return rendered;
    }
    let mut cleanup = String::new();
    for active in cleanups.iter().rev() {
        match active {
            ActiveCleanup::Deferred(id) => {
                cleanup.push_str(&format!("_jet_deferred_close_{}.run(); ", id));
            }
            ActiveCleanup::Resource(name) => cleanup.push_str(&format!("{}.close(); ", name)),
        }
    }
    rendered.replace(
        crate::Codegen::TIR::RESOURCE_CLEANUP_MARKER,
        &cleanup,
    )
}

/// Emit a closure block while preserving Jet's final-expression return rule.
/// Ordinary statement blocks terminate expression statements with `;`; a lambda's
/// final expression is its value and must remain a Rust tail expression. A final
/// non-expression statement (including an explicit return) keeps normal emission,
/// which also preserves unit-returning closures.
pub(crate) fn emit_tir_lambda_block(
    stmts: &[TStmt],
    cx: &Cx,
    out: &mut String,
    indent: usize,
) {
    let Some((last, prefix)) = stmts.split_last() else {
        return;
    };
    let mut active_cleanups = Vec::new();
    emit_tir_stmts_inline(prefix, cx, out, indent, &mut active_cleanups);
    if let TStmt::ExprStmt(expr) = last {
        let pad = "    ".repeat(indent);
        if matches!(
            expr.kind,
            crate::Codegen::TIR::TExprKind::RequireStop {
                always_stops: true,
                ..
            }
        ) {
            emit_cleanups_now(&active_cleanups, out, indent);
        }
        out.push_str(&format!(
            "{}{}\n",
            pad,
            emit_expr_with_cleanups(expr, cx, &active_cleanups)
        ));
    } else {
        emit_tir_stmt(last, cx, out, indent, &mut active_cleanups);
    }
}

fn emit_tir_stmt(
    s: &TStmt,
    cx: &Cx,
    out: &mut String,
    indent: usize,
    active_deferred_closes: &mut Vec<ActiveCleanup>,
) {
    let pad = "    ".repeat(indent);
    match s {
        TStmt::Let {
            name,
            kw,
            ty_clause,
            init,
            track_origin,
            gc_promotion,
            gc_transferred: _,
        } => {
            let fixed_bytes = match &init.kind {
                crate::Codegen::TIR::TExprKind::AllocNew { ctor } => {
                    ctor.strip_prefix("__JET_FIXED_INLINE:")
                }
                crate::Codegen::TIR::TExprKind::ResourceNew(inner) => match &inner.kind {
                    crate::Codegen::TIR::TExprKind::AllocNew { ctor } => {
                        ctor.strip_prefix("__JET_FIXED_INLINE:")
                    }
                    _ => None,
                },
                _ => None,
            };
            if let Some(bytes) = fixed_bytes {
                let backing = format!("{}__fixed_backing", mangle(name));
                out.push_str(&format!(
                    "{}let mut {} = [std::mem::MaybeUninit::<u8>::uninit(); {}];\n",
                    pad, backing, bytes
                ));
                let ctor = format!("jet_mem::JetFixed::over_uninit(&mut {})", backing);
                let value = if matches!(&init.kind, crate::Codegen::TIR::TExprKind::ResourceNew(_)) {
                    format!("JetResource::new({ctor})")
                } else {
                    ctor
                };
                out.push_str(&format!(
                    "{}{} {}{} = {};\n",
                    pad, kw, mangle(name), ty_clause, value
                ));
                if matches!(&init.kind, crate::Codegen::TIR::TExprKind::ResourceNew(_)) {
                    active_deferred_closes.push(ActiveCleanup::Resource(mangle(name)));
                }
                return;
            }
            if let Some(promotion) = gc_promotion {
                let local = mangle(name);
                let value = format!("_jet_gc_value_{local}");
                let site = format!("_jet_gc_site_{local}");
                out.push_str(&format!(
                    "{}let {} = {};\n",
                    pad,
                    value,
                    emit_expr_with_cleanups(init, cx, active_deferred_closes),
                ));
                out.push_str(&format!(
                    "{}let {} = jet_gc::PromotionSite {{ source: {:?}, span_start: {}, span_end: {}, scope: {:?}, policy_provenance: {:?}, reason: {:?}, type_name: std::any::type_name_of_val(&{}), bytes: std::mem::size_of_val(&{}) as u64 }};\n{}{} {}{} = jet_gc::runtime_or_exit(jet_gc::AutomaticRoot::promote({}, {}));\n",
                    pad,
                    site,
                    cx.file,
                    promotion.span.start,
                    promotion.span.end,
                    promotion.scope,
                    promotion.policy_provenance,
                    promotion.reason,
                    value,
                    value,
                    pad,
                    kw,
                    local,
                    ty_clause,
                    value,
                    site,
                ));
                if !promotion.edges.is_empty() || promotion.collection_len.is_some() {
                    let edges = promotion
                        .edges
                        .iter()
                        .map(|edge| {
                            format!(
                                "({:?}, {}, {}.id())",
                                edge.slot,
                                edge.group,
                                mangle(&edge.binding)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push_str(&format!(
                        "{}jet_gc::runtime_or_exit({}.replace_edge_slots(&[{}], {:?}));\n",
                        pad, local, edges, promotion.collection_len
                    ));
                }
                return;
            }
            out.push_str(&format!(
                "{}{} {}{} = {};\n",
                pad,
                kw,
                mangle(name),
                ty_clause,
                emit_expr_with_cleanups(init, cx, active_deferred_closes),
            ));
            if matches!(&init.kind, crate::Codegen::TIR::TExprKind::ResourceNew(_)) {
                active_deferred_closes.push(ActiveCleanup::Resource(mangle(name)));
            }
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
        TStmt::GcEdit {
            root,
            slot,
            edges,
            replace_all,
            index_temp,
            stmt,
        } => {
            if let Some((temp, value)) = index_temp {
                out.push_str(&format!(
                    "{}let {} = {};\n",
                    pad,
                    temp,
                    emit_expr_with_cleanups(value, cx, active_deferred_closes)
                ));
            }
            if *replace_all {
                out.push_str(&format!(
                    "{}jet_gc::runtime_or_exit({}.edit_replacing_all_edges(&[{}], |__jet_value| {{\n",
                    pad,
                    root,
                    edges.join(", ")
                ));
            } else if let Some((temp, _)) = index_temp {
                out.push_str(&format!(
                    "{}jet_gc::runtime_or_exit({}.edit_edge_slot_index(\"collection\", {} as usize, &[{}], |__jet_value| {{\n",
                    pad,
                    root,
                    temp,
                    edges.join(", ")
                ));
            } else {
                out.push_str(&format!(
                    "{}jet_gc::runtime_or_exit({}.edit_edge_slot({:?}, &[{}], |__jet_value| {{\n",
                    pad,
                    root,
                    slot,
                    edges.join(", ")
                ));
            }
            emit_tir_stmt(stmt, cx, out, indent + 1, active_deferred_closes);
            out.push_str(&format!("{}}}));\n", pad));
        }
        TStmt::SplitViews {
            owner,
            root,
            len,
            source,
            source_start,
            before,
            split_tail,
            segment,
            after,
            name,
            start,
            end,
            single,
            write,
            line,
        } => {
            if let Some(owner) = owner {
                let owner = emit_expr_with_cleanups(owner, cx, active_deferred_closes);
                out.push_str(&format!(
                    "{}let {} = &mut ({})[..];\n{}let {} = ({}).len() as i64;\n",
                    pad, root, owner, pad, len, root
                ));
            }
            out.push_str(&format!(
                "{}jet_check_view_bounds({}, {}, {}, {:?}, {});\n",
                pad, len, start, end, cx.file, line
            ));
            let relative_start = start - source_start;
            let width = end - start + 1;
            out.push_str(&format!(
                "{}let ({}, {}) = ({}).split_at_mut({}usize);\n",
                pad, before, split_tail, source, relative_start
            ));
            out.push_str(&format!(
                "{}let ({}, {}) = ({}).split_at_mut({}usize);\n",
                pad, segment, after, split_tail, width
            ));
            if *single && *write {
                out.push_str(&format!(
                    "{}let {} = &mut {}[0];\n",
                    pad,
                    mangle(name),
                    segment
                ));
            } else if *single {
                out.push_str(&format!(
                    "{}let {} = &{}[0];\n",
                    pad,
                    mangle(name),
                    segment
                ));
            } else if *write {
                out.push_str(&format!("{}let {} = {};\n", pad, mangle(name), segment));
            } else {
                out.push_str(&format!("{}let {} = &*{};\n", pad, mangle(name), segment));
            }
        }
        TStmt::Assign {
            place,
            op,
            value,
            clone_value,
        } => {
            let v = emit_expr_with_cleanups(value, cx, active_deferred_closes);
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
                emit_expr_with_cleanups(init, cx, active_deferred_closes)
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
                emit_expr_with_cleanups(init, cx, active_deferred_closes)
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
                emit_expr_with_cleanups(init, cx, active_deferred_closes)
            ));
            for (i, elem_rust) in elems.iter().enumerate() {
                out.push_str(&format!(
                    "{}{} {} = jet_unpack_vec({}, {}, {}, {:?}, {});\n",
                    pad, kw, elem_rust, tmp, want, i, file, line
                ));
            }
        }
        TStmt::Return(Some(e)) => {
            out.push_str(&format!("{}return {};\n", pad, emit_expr_with_cleanups(e, cx, active_deferred_closes)));
        }
        TStmt::Return(None) => {
            out.push_str(&format!("{}return;\n", pad));
        }
        TStmt::ExprStmt(e) => {
            if matches!(
                e.kind,
                crate::Codegen::TIR::TExprKind::RequireStop {
                    always_stops: true,
                    ..
                }
            ) {
                // `jet_panic` terminates the process instead of unwinding. Run
                // all lexical deferred closes explicitly before that boundary;
                // each guard drains its Option so its later Drop is a no-op.
                emit_cleanups_now(active_deferred_closes, out, indent);
            }
            out.push_str(&format!(
                "{}{};\n",
                pad,
                emit_expr_with_cleanups(e, cx, active_deferred_closes)
            ));
        }
        TStmt::DeferClose {
            close,
            resource,
            id,
        } => {
            out.push_str(&format!(
                "{}let mut _jet_deferred_close_{} = JetDeferredClose::new(move || {{ let _ = {}; }});\n",
                pad,
                id,
                emit_expr_with_cleanups(close, cx, active_deferred_closes)
            ));
            active_deferred_closes.retain(
                |cleanup| !matches!(cleanup, ActiveCleanup::Resource(name) if name == resource),
            );
            active_deferred_closes.push(ActiveCleanup::Deferred(*id));
        }
        TStmt::If {
            cond,
            then_body,
            else_body,
            else_is_elseif,
        } => {
            if let TIfCond::IfLet {
                pat_str,
                subj,
                pre_guard,
                guard,
            } = cond
            {
                if pre_guard.is_some() || guard.is_some() {
                    let match_indent = indent + usize::from(pre_guard.is_some());
                    let match_pad = "    ".repeat(match_indent);
                    if let Some(pre_guard) = pre_guard {
                        out.push_str(&format!(
                            "{}if {} {{\n",
                            pad,
                            emit_expr_with_cleanups(pre_guard, cx, active_deferred_closes),
                        ));
                    }
                    let guard = guard
                        .as_ref()
                        .map(|guard| {
                            format!(
                                " if {}",
                                emit_expr_with_cleanups(guard, cx, active_deferred_closes)
                            )
                        })
                        .unwrap_or_default();
                    out.push_str(&format!(
                        "{}match {} {{\n{}    {}{} => {{\n",
                        match_pad,
                        emit_expr_with_cleanups(subj, cx, active_deferred_closes),
                        match_pad,
                        pat_str,
                        guard,
                    ));
                    emit_tir_stmts_nested(
                        then_body,
                        cx,
                        out,
                        match_indent + 2,
                        active_deferred_closes,
                    );
                    out.push_str(&format!(
                        "{}    }},\n{}    _ => {{\n",
                        match_pad, match_pad
                    ));
                    if let Some(body) = else_body {
                        emit_tir_stmts_nested(
                            body,
                            cx,
                            out,
                            match_indent + 2,
                            active_deferred_closes,
                        );
                    }
                    out.push_str(&format!("{}    }},\n{}}}\n", match_pad, match_pad));
                    if pre_guard.is_some() {
                        out.push_str(&format!("{}}} else {{\n", pad));
                        if let Some(body) = else_body {
                            emit_tir_stmts_nested(
                                body,
                                cx,
                                out,
                                indent + 1,
                                active_deferred_closes,
                            );
                        }
                        out.push_str(&format!("{}}}\n", pad));
                    }
                    return;
                }
            }
            // c109 Phase 22: render the head per the condition form, byte-for-byte
            // `emit_if` (Source/Codegen/Statement.rs).
            match cond {
                TIfCond::Plain(c) => {
                    out.push_str(&format!("{}if {} {{\n", pad, emit_expr_with_cleanups(c, cx, active_deferred_closes)));
                }
                TIfCond::IfLet { pat_str, subj, pre_guard, guard } => {
                    debug_assert!(pre_guard.is_none());
                    debug_assert!(guard.is_none());
                    out.push_str(&format!(
                        "{}if let {} = {} {{\n",
                        pad,
                        pat_str,
                        emit_expr_with_cleanups(subj, cx, active_deferred_closes),
                    ));
                }
                TIfCond::IsNone { subj } => {
                    out.push_str(&format!(
                        "{}if {}.is_none() {{\n",
                        pad,
                        emit_expr_with_cleanups(subj, cx, active_deferred_closes)
                    ));
                }
                TIfCond::Matches { pat_str, subj } => {
                    out.push_str(&format!(
                        "{}if matches!(&({}), {}) {{\n",
                        pad,
                        emit_expr_with_cleanups(subj, cx, active_deferred_closes),
                        pat_str
                    ));
                }
            }
            emit_tir_stmts_nested(then_body, cx, out, indent + 1, active_deferred_closes);
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
                        let mut branch_deferred = active_deferred_closes.clone();
                        emit_tir_stmt(
                            &body[0],
                            cx,
                            &mut nested,
                            indent,
                            &mut branch_deferred,
                        );
                        out.push_str(nested.trim_start_matches(&pad as &str));
                    } else {
                        out.push_str(&format!("{}}} else {{\n", pad));
                        emit_tir_stmts_nested(
                            body,
                            cx,
                            out,
                            indent + 1,
                            active_deferred_closes,
                        );
                        out.push_str(&format!("{}}}\n", pad));
                    }
                }
            }
        }
        // c109 Phase 2: control-flow loops. Each mirrors the AST emit path
        // (Statement.rs) byte-for-byte; all decisions are read off the TIR.
        TStmt::Loop { label, body } => {
            out.push_str(&format!("{}{}loop {{\n", pad, tir_label_prefix(label)));
            emit_tir_stmts_nested(body, cx, out, indent + 1, active_deferred_closes);
            out.push_str(&format!("{}}}\n", pad));
        }
        TStmt::While { label, cond, body } => {
            out.push_str(&format!(
                "{}{}while {} {{\n",
                pad,
                tir_label_prefix(label),
                emit_expr_with_cleanups(cond, cx, active_deferred_closes)
            ));
            emit_tir_stmts_nested(body, cx, out, indent + 1, active_deferred_closes);
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
            let mut counted_deferred = active_deferred_closes.clone();
            emit_tir_stmt(init, cx, out, indent + 1, &mut counted_deferred);
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
                emit_expr_with_cleanups(cond, cx, active_deferred_closes)
            ));
            emit_tir_stmts_nested(body, cx, out, indent + 2, &counted_deferred);
            emit_tir_stmt(step, cx, out, indent + 2, &mut counted_deferred);
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
            let s = emit_expr_with_cleanups(start, cx, active_deferred_closes);
            let e = emit_expr_with_cleanups(end, cx, active_deferred_closes);
            // S22 (D-SG8): `..` is inclusive → `..=`; `step` becomes `.step_by`.
            match step {
                Some(step) => {
                    let st = emit_expr_with_cleanups(step, cx, active_deferred_closes);
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
            emit_tir_stmts_nested(body, cx, out, indent + 1, active_deferred_closes);
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
                emit_tir_stmts_nested(
                    &arm.body,
                    cx,
                    out,
                    indent + 2,
                    active_deferred_closes,
                );
                out.push_str(&format!("{}    }}\n", pad));
            }
            match else_body {
                Some(body) => {
                    out.push_str(&format!("{}    _ => {{\n", pad));
                    emit_tir_stmts_nested(
                        body,
                        cx,
                        out,
                        indent + 2,
                        active_deferred_closes,
                    );
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
                emit_tir_stmts_nested(
                    body,
                    cx,
                    out,
                    indent + 2,
                    active_deferred_closes,
                );
            }
            out.push_str(&format!("{}}} else {{\n", inner_pad));
            emit_tir_stmts_nested(
                else_body,
                cx,
                out,
                indent + 2,
                active_deferred_closes,
            );
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
            let b = emit_expr_with_cleanups(base, cx, active_deferred_closes);
            let i = emit_expr_with_cleanups(index, cx, active_deferred_closes);
            let v = emit_expr_with_cleanups(value, cx, active_deferred_closes);
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
            let ty = user_type_rust(type_name);
            let b = emit_expr_with_cleanups(base, cx, active_deferred_closes);
            let i = emit_expr_with_cleanups(index, cx, active_deferred_closes);
            let v = emit_expr_with_cleanups(value, cx, active_deferred_closes);
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
            let b = emit_expr_with_cleanups(base, cx, active_deferred_closes);
            let mut v = emit_expr_with_cleanups(value, cx, active_deferred_closes);
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
            collection: _,
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
                Some(TForInMethod::LinesProcessStream) => {
                    // D-PROCESS1=A: poll `jet_process_stream_next_line`, panicking on a
                    // read error (mirrors the `LinesFile`/`LinesStdin` mid-stream-error
                    // panic), `break`ing at EOF. One open brace (the `for`-equivalent
                    // `loop`), closed by the generic tail below — no extra block needed.
                    out.push_str(&format!("{}{}loop {{\n", pad, lbl));
                    out.push_str(&format!(
                        "{}    let _jet_line_opt = {}jet_process_stream_next_line(&({})).unwrap_or_else(|_e| {}jet_panic({:?}, {}, &format!(\"{{:?}}\", _e)));\n",
                        pad,
                        cx.root_prefix,
                        collection_str,
                        cx.root_prefix,
                        cx.file,
                        0
                    ));
                    out.push_str(&format!(
                        "{}    let Some(_jet_raw_line) = _jet_line_opt else {{ break; }};\n",
                        pad
                    ));
                    out.push_str(&format!(
                        "{}    let {} = _jet_raw_line;\n",
                        pad,
                        mangle(var)
                    ));
                }
                Some(TForInMethod::Iterable {
                    coll_type,
                    iter_type,
                }) => {
                    let coll_rust = user_type_rust(coll_type);
                    let iter_rust = user_type_rust(iter_type);
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
            emit_tir_stmts_nested(body, cx, out, indent + 1, active_deferred_closes);
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
            emit_tir_stmts_inline(stmts, cx, out, indent, active_deferred_closes);
        }
        // D-CANVASSTATE1=D: stripped from release builds by an internal cfg.
        // `jet run`/`jet dev`/default builds do not set `jet_release`, so debug
        // bodies execute there without exposing `build.profile` to Jet code.
        TStmt::DebugOnly(stmts) => {
            out.push_str(&format!("{}#[cfg(not(jet_release))]\n", pad));
            out.push_str(&format!("{}{{\n", pad));
            emit_tir_stmts_nested(stmts, cx, out, indent + 1, active_deferred_closes);
            out.push_str(&format!("{}}}\n", pad));
        }
        // c109 Phase 18: an audited `@Unsafe { … }` region — `unsafe { … }`, byte-for-byte
        // `emit_stmts`'s `Stmt::Unsafe` arm (the `#Audit` annotation emits nothing). I1:
        // emitted ONLY for a source `@Unsafe` gate.
        TStmt::Unsafe(body) => {
            out.push_str(&format!("{}unsafe {{\n", pad));
            emit_tir_stmts_nested(body, cx, out, indent + 1, active_deferred_closes);
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
            emit_tir_stmts_nested(body, cx, out, inner, active_deferred_closes);
            out.push_str(&format!("{}}}\n", pad));
        }
        // D-SHIELDNAME1=A: `@Shield { … }` — enter a cancellation-shield region, install
        // a scope guard that leaves it, then emit the body. The guard fires on every
        // exit path (normal, return, ?, panic unwind) via Rust's Drop ordering, so a
        // cancel/deadline deferred while shielded lands when the region exits.
        TStmt::Shield { body } => {
            let inner = indent + 1;
            let inner_pad = "    ".repeat(inner);
            out.push_str(&format!("{}{{\n", pad));
            out.push_str(&format!(
                "{}{}jet_scheduler_shield_enter();\n",
                inner_pad, cx.root_prefix
            ));
            out.push_str(&format!(
                "{}let _shield_guard = {}jet_scope_guard(|| {{ {}jet_scheduler_shield_leave(); }});\n",
                inner_pad, cx.root_prefix, cx.root_prefix
            ));
            emit_tir_stmts_nested(body, cx, out, inner, active_deferred_closes);
            out.push_str(&format!("{}}}\n", pad));
        }
        // D-DOTSCOPE1: a `@Test` scope member, emitted inside `fn jet_test_N() ->
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
                    emit_tir_stmts_inline(body, cx, out, indent, active_deferred_closes);
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
                    emit_tir_stmts_nested(
                        body,
                        cx,
                        out,
                        inner + 1,
                        active_deferred_closes,
                    );
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
                    emit_tir_stmts_nested(body, cx, out, inner, active_deferred_closes);
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
                    emit_tir_stmts_nested(body, cx, out, inner, active_deferred_closes);
                    out.push_str(&format!("{}}}\n", pad));
                }
            }
        }
        // D-REACTCORE1: `@Reactive { … }` — register a reactive effect at this point.
        TStmt::Reactive { closure } => {
            out.push_str(&format!(
                "{}{}jet_std::jet_reactive_effect({});\n",
                pad, cx.root_prefix, closure
            ));
        }
        TStmt::Region(body) => {
            out.push_str(&format!("{}{{\n", pad));
            emit_tir_stmts_nested(body, cx, out, indent + 1, active_deferred_closes);
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
            emit_tir_stmts_inline(body, cx, out, indent, active_deferred_closes);
        }
        // D-TXN1–D-TXN4 (ratified 2026-06-24): `@Transact(name) { … }` block — open a
        // transaction guard, emit the body, then `commit()` on the clean fall-through
        // path. An early `?`/`return` skips `commit()`, so registered `on_commit` hooks
        // drop un-run (D-TXN3). Codegen is dumb (I3): no effect/rollback machinery here.
        TStmt::Transact {
            handle,
            snapshots,
            uses_stm,
            body,
        } => {
            let inner = indent + 1;
            let inner_pad = "    ".repeat(inner);
            out.push_str(&format!("{}{{\n", pad));
            // D-STM1=A (card #506): a block that touched the Shared plane opens an STM
            // transaction; `edit_txn` calls in the body defer onto it, and `.commit()`
            // (emitted after the body, before any `<handle>.commit()`) applies every
            // deferred edit atomically under all the handles' locks at once. A `?`/early
            // return skips the commit, so the guard's Drop discards the deferred edits.
            if *uses_stm {
                out.push_str(&format!(
                    "{}let __jet_stm = {}jet_stm::begin();\n",
                    inner_pad, cx.root_prefix
                ));
            }
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
                    emit_tir_stmts_nested(body, cx, out, inner, active_deferred_closes);
                    if *uses_stm {
                        out.push_str(&format!("{}__jet_stm.commit();\n", inner_pad));
                    }
                    out.push_str(&format!("{}{}.commit();\n", inner_pad, handle));
                }
                None => {
                    emit_tir_stmts_nested(body, cx, out, inner, active_deferred_closes);
                    if *uses_stm {
                        out.push_str(&format!("{}__jet_stm.commit();\n", inner_pad));
                    }
                }
            }
            out.push_str(&format!("{}}}\n", pad));
        }
        // c109 Phase 19: a `@Context(field: value) { … }` block — a plain block with one
        // RAII/no-op guard per field (declaration order) BEFORE the body, byte-for-byte
        // `emit_stmts`'s `Stmt::ContextBlock` arm.
        TStmt::ContextBlock { guards, body } => {
            out.push_str(&format!("{}{{\n", pad));
            let inner = indent + 1;
            let inner_pad = "    ".repeat(inner);
            for (i, (field_name, value)) in guards.iter().enumerate() {
                let val = emit_expr_with_cleanups(value, cx, active_deferred_closes);
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
            emit_tir_stmts_nested(body, cx, out, inner, active_deferred_closes);
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
                    emit_expr_with_cleanups(cond, cx, active_deferred_closes)
                ));
                emit_tir_stmts_nested(
                    body,
                    cx,
                    out,
                    indent + 2,
                    active_deferred_closes,
                );
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
                    emit_tir_stmts_nested(
                        body,
                        cx,
                        out,
                        indent + 1,
                        active_deferred_closes,
                    );
                }
                Some(body) => {
                    out.push_str(&format!("{}}} else {{\n", inner_pad));
                    emit_tir_stmts_nested(
                        body,
                        cx,
                        out,
                        indent + 2,
                        active_deferred_closes,
                    );
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
