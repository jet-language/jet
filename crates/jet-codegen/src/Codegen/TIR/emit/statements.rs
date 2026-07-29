use crate::Codegen::Cx;
use crate::Codegen::mangle;
use crate::Codegen::user_type_rust;
use crate::Codegen::TIR::emit::emit_field_rust;
use crate::Codegen::TIR::emit::emit_let_ty_clause;
use crate::Codegen::TIR::emit::emit_math_swizzle_assign_stmt;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::emit_tir_pattern;
use crate::Codegen::TIR::emit_tir_place;
use crate::Codegen::TIR::tir_range_guard;
use crate::Codegen::TIR::ScopeMemberKind;
use crate::Codegen::TIR::TForInMethod;
use crate::Codegen::TIR::TIfCond;
use crate::Codegen::TIR::TStmt;
use crate::AST::Type;

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

fn emit_if_head(
    cond: &TIfCond,
    cx: &Cx,
    out: &mut String,
    indent: usize,
    active_cleanups: &[ActiveCleanup],
) {
    let pad = "    ".repeat(indent);
    match cond {
        TIfCond::Plain(cond) => out.push_str(&format!(
            "{}if {} {{\n",
            pad,
            emit_expr_with_cleanups(cond, cx, active_cleanups)
        )),
        TIfCond::IfLet { pattern, subj } => out.push_str(&format!(
            "{}if let {} = {} {{\n",
            pad,
            emit_tir_pattern(pattern, cx),
            emit_expr_with_cleanups(subj, cx, active_cleanups),
        )),
        TIfCond::IsNone { subj } => out.push_str(&format!(
            "{}if {}.is_none() {{\n",
            pad,
            emit_expr_with_cleanups(subj, cx, active_cleanups)
        )),
        TIfCond::Matches { pattern, subj } => out.push_str(&format!(
            "{}if matches!(&({}), {}) {{\n",
            pad,
            emit_expr_with_cleanups(subj, cx, active_cleanups),
            emit_tir_pattern(pattern, cx)
        )),
        TIfCond::And { .. } => unreachable!("conjunction heads are atomic"),
    }
}

fn emit_if_else(
    else_body: &Option<Vec<TStmt>>,
    else_is_elseif: bool,
    cx: &Cx,
    out: &mut String,
    indent: usize,
    active_cleanups: &[ActiveCleanup],
) {
    let pad = "    ".repeat(indent);
    match else_body {
        None => out.push_str(&format!("{}}}\n", pad)),
        // `else if` only when the else-body is exactly one nested `If`.
        // A multi-stmt residual (guard-table final `else`) must stay braced —
        // emitting only body[0] silently drops later statements.
        Some(body)
            if else_is_elseif
                && body.len() == 1
                && matches!(body.first(), Some(TStmt::If { .. })) =>
        {
            out.push_str(&format!("{}}} else ", pad));
            let mut nested = String::new();
            let mut branch_cleanups = active_cleanups.to_vec();
            emit_tir_stmt(&body[0], cx, &mut nested, indent, &mut branch_cleanups);
            out.push_str(nested.trim_start_matches(&pad as &str));
        }
        Some(body) => {
            out.push_str(&format!("{}}} else {{\n", pad));
            emit_tir_stmts_nested(body, cx, out, indent + 1, active_cleanups);
            out.push_str(&format!("{}}}\n", pad));
        }
    }
}

fn emit_tir_if(
    cond: &TIfCond,
    then_body: &[TStmt],
    else_body: &Option<Vec<TStmt>>,
    else_is_elseif: bool,
    cx: &Cx,
    out: &mut String,
    indent: usize,
    active_cleanups: &[ActiveCleanup],
) {
    if let TIfCond::And { left, right } = cond {
        emit_if_head(left, cx, out, indent, active_cleanups);
        emit_tir_if(
            right,
            then_body,
            else_body,
            else_is_elseif,
            cx,
            out,
            indent + 1,
            active_cleanups,
        );
        emit_if_else(
            else_body,
            else_is_elseif,
            cx,
            out,
            indent,
            active_cleanups,
        );
        return;
    }
    emit_if_head(cond, cx, out, indent, active_cleanups);
    emit_tir_stmts_nested(then_body, cx, out, indent + 1, active_cleanups);
    emit_if_else(
        else_body,
        else_is_elseif,
        cx,
        out,
        indent,
        active_cleanups,
    );
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
            let_ty,
            init,
            gc_promotion,
            gc_transferred: _,
        } => {
            let ty_clause = emit_let_ty_clause(let_ty, cx);
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
            elem_ty: _,
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
            let place = emit_tir_place(place, cx);
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
        } => emit_tir_if(
            cond,
            then_body,
            else_body,
            *else_is_elseif,
            cx,
            out,
            indent,
            active_deferred_closes,
        ),
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
            if step.is_some() {
                out.push_str(&format!("{}let mut _jet_loop_first = true;\n", inner_pad));
            }
            out.push_str(&format!(
                "{}{}loop {{\n",
                inner_pad,
                tir_label_prefix(label),
            ));
            let body_pad = "    ".repeat(indent + 2);
            // D-LOOP-CONTINUE2=A: both normal fallthrough and every `continue`
            // re-enter at the top. Skip the afterthought only on the first entry;
            // thereafter run it before retesting. Break/return/failure/panic leave
            // the loop and therefore never execute it.
            if let Some(step) = step {
                out.push_str(&format!("{}if _jet_loop_first {{ _jet_loop_first = false; }} else {{\n", body_pad));
                emit_tir_stmt(step, cx, out, indent + 3, &mut counted_deferred);
                out.push_str(&format!("{}}}\n", body_pad));
            }
            out.push_str(&format!(
                "{}if !({}) {{ break; }}\n",
                body_pad,
                emit_expr_with_cleanups(cond, cx, active_deferred_closes)
            ));
            emit_tir_stmts_nested(body, cx, out, indent + 2, &counted_deferred);
            out.push_str(&format!("{}}}\n", inner_pad));
            out.push_str(&format!("{}}}\n", pad));
        }
        TStmt::Range {
            label,
            var,
            source,
            start,
            end,
            step,
            exclusive,
            body,
        } => {
            let lbl = tir_label_prefix(label);
            if let Some(source) = source {
                let source = emit_expr_with_cleanups(source, cx, active_deferred_closes);
                out.push_str(&format!("{}{{ let _jet_range = {};\n", pad, source));
                let range_pad = "    ".repeat(indent + 1);
                let body_pad = "    ".repeat(indent + 2);
                let stride = step.as_ref().map(|step| {
                    emit_expr_with_cleanups(step, cx, active_deferred_closes)
                });
                if let Some(stride) = &stride {
                    out.push_str(&format!(
                        "{}let _jet_loop_stride = {};\n",
                        range_pad, stride
                    ));
                    out.push_str(&format!(
                        "{}if _jet_loop_stride <= 0 {{ {}jet_panic({:?}, 0, \"E0123: loop stride must be positive\"); }}\n",
                        range_pad, cx.root_prefix, cx.file
                    ));
                }
                for (condition, op) in [(true, ".."), (false, "..=")] {
                    out.push_str(&format!(
                        "{}{} _jet_range.exclusive {{\n",
                        range_pad,
                        if condition { "if" } else { "} else if !" }
                    ));
                    let step_suffix = if stride.is_some() {
                        ".step_by(_jet_loop_stride as usize)"
                    } else {
                        ""
                    };
                    out.push_str(&format!(
                        "{}{}for {} in (_jet_range.start{op}_jet_range.end){step_suffix} {{\n",
                        body_pad,
                        lbl,
                        mangle(var)
                    ));
                    emit_tir_stmts_nested(body, cx, out, indent + 3, active_deferred_closes);
                    out.push_str(&format!("{}}}\n", body_pad));
                }
                out.push_str(&format!("{}}}\n", range_pad));
                out.push_str(&format!("{}}}\n", pad));
                return;
            }
            let s = emit_expr_with_cleanups(start, cx, active_deferred_closes);
            let e = emit_expr_with_cleanups(end, cx, active_deferred_closes);
            // S22: `..` → `..=`; D-RANGE-EXCL1=C: `..<` → `..`.
            let range_op = if *exclusive { ".." } else { "..=" };
            match step {
                Some(step) => {
                    let st = emit_expr_with_cleanups(step, cx, active_deferred_closes);
                    out.push_str(&format!("{}{{ let _jet_loop_start = {};\n", pad, s));
                    out.push_str(&format!("{}    let _jet_loop_end = {};\n", pad, e));
                    out.push_str(&format!("{}    let _jet_loop_stride = {};\n", pad, st));
                    out.push_str(&format!("{}    if _jet_loop_stride <= 0 {{ {}jet_panic({:?}, 0, \"E0123: loop stride must be positive\"); }}\n", pad, cx.root_prefix, cx.file));
                    out.push_str(&format!(
                        "{}{}for {} in (_jet_loop_start{range_op}_jet_loop_end).step_by(_jet_loop_stride as usize) {{\n",
                        pad,
                        lbl,
                        mangle(var)
                    ));
                }
                None => {
                    out.push_str(&format!(
                        "{}{}for {} in ({}){range_op}({}) {{\n",
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
            if step.is_some() { out.push_str(&format!("{}}}\n", pad)); }
        }
        TStmt::Break(label) => match label {
            Some(name) => out.push_str(&format!("{}break 'jet_{};\n", pad, name)),
            None => out.push_str(&format!("{}break;\n", pad)),
        },
        TStmt::BreakValue { label, value } => {
            let value = emit_expr_with_cleanups(value, cx, active_deferred_closes);
            match label {
                Some(name) => {
                    out.push_str(&format!("{}break 'jet_{} {};\n", pad, name, value))
                }
                None => out.push_str(&format!("{}break {};\n", pad, value)),
            }
        }
        TStmt::Continue(label) => match label {
            Some(name) => out.push_str(&format!("{}continue 'jet_{};\n", pad, name)),
            None => out.push_str(&format!("{}continue;\n", pad)),
        },
        // c109 Phase 4: an exhaustive enum match. Mirrors `emit_pattern_match_switch`
        // (Statement.rs) byte-for-byte; every pattern/guard string was resolved at
        // lowering. Arm bodies emit at indent+2.
        TStmt::EnumMatch {
            scrutinee,
            clone_subject,
            arms,
            else_body,
            fallthrough,
        } => {
            let subject = emit_tir_expr(scrutinee, cx);
            let subject = if *clone_subject {
                format!("({subject}).clone()")
            } else {
                subject
            };
            out.push_str(&format!("{}match {} {{\n", pad, subject));
            for arm in arms {
                let pattern = emit_tir_pattern(&arm.pattern, cx);
                match tir_range_guard(&arm.pattern.pattern) {
                    Some(guard) => {
                        out.push_str(&format!("{}    {} if {} => {{\n", pad, pattern, guard))
                    }
                    None => out.push_str(&format!("{}    {} => {{\n", pad, pattern)),
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
            subject,
            arms,
            else_body,
        } => {
            let subject_str = emit_tir_expr(subject, cx);
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
        TStmt::IndexFieldAssign(assign) => {
            let b = emit_expr_with_cleanups(&assign.base, cx, active_deferred_closes);
            let i = emit_expr_with_cleanups(&assign.index, cx, active_deferred_closes);
            let mut v =
                emit_expr_with_cleanups(&assign.value, cx, active_deferred_closes);
            if assign.clone_value {
                v = format!("({v}).clone()");
            }
            let operator = assign.op.map_or("=".to_string(), |op| {
                format!("{}=", op.spell())
            });
            if assign.is_map {
                out.push_str(&format!(
                    "{pad}{{ let __jet_v = {v}; let __jet_k = ({i}).clone(); \
                     let Some(__jet_item) = ({b}).get_mut(&__jet_k) else {{ \
                     jet_panic({file:?}, {line}, \"map key not found\"); }}; \
                     __jet_item.{field} {operator} __jet_v; }}\n",
                    file = cx.file,
                    line = assign.line,
                    field = emit_field_rust(cx, &assign.base.ty, &assign.field),
                ));
            } else {
                let index = if assign.index_proven {
                    format!("({i}).0 as usize")
                } else {
                    format!("{i} as usize")
                };
                out.push_str(&format!(
                    "{pad}{{ let __jet_v = {v}; ({b})[{index}].{field} {operator} __jet_v; }}\n",
                    field = emit_field_rust(cx, &assign.base.ty, &assign.field),
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
            source,
            collection,
            step,
            method_kind,
            columnar,
            by_value,
            body,
        } => {
            let lbl = tir_label_prefix(label);
            let mut stride_wrapper = false;
            let source_storage;
            let stride_suffix;
            let source_rust = emit_tir_expr(source, cx);
            let collection_str = if let Some(step) = step {
                stride_wrapper = true;
                let stride = emit_expr_with_cleanups(step, cx, active_deferred_closes);
                out.push_str(&format!("{}{{ let _jet_loop_source = {};\n", pad, source_rust));
                out.push_str(&format!("{}    let _jet_loop_stride = {};\n", pad, stride));
                out.push_str(&format!("{}    if _jet_loop_stride <= 0 {{ {}jet_panic({:?}, 0, \"E0123: loop stride must be positive\"); }}\n", pad, cx.root_prefix, cx.file));
                source_storage = "_jet_loop_source".to_string();
                stride_suffix = ".step_by(_jet_loop_stride as usize)".to_string();
                source_storage.as_str()
            } else {
                stride_suffix = String::new();
                source_rust.as_str()
            };
            // c109 Phase 22: a method-call collection takes a distinct `emit_for_in`
            // branch (`source` holds the RECEIVER for chars/lines). Only the
            // stdin form opens an extra block that needs an extra closing brace.
            let mut needs_extra_close = false;
            match method_kind {
                Some(TForInMethod::Chars) => {
                    out.push_str(&format!(
                        "{}{}for _jet_c in ({recv}).chars(){} {{\n    {}let {} = _jet_c;\n",
                        pad,
                        lbl,
                        stride_suffix,
                        pad,
                        mangle(var),
                        recv = collection_str
                    ));
                }
                Some(TForInMethod::LinesFile) => {
                    out.push_str(&format!(
                        "{}{}for _jet_raw_line in std::io::BufRead::lines(&mut ({}).inner){} {{\n",
                        pad, lbl, collection_str, stride_suffix
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
                        "{}{}for _jet_raw_line in std::io::BufRead::lines(&mut _jet_stdin_h.inner){} {{\n",
                        pad, lbl, stride_suffix
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
                    // D-PROCESS1=A: adapt Result<Option<Line>> into an iterator so
                    // `.step_by` owns pull counting and `next` cannot skip stride pulls.
                    out.push_str(&format!(
                        "{}{}for _jet_line_result in std::iter::from_fn(|| {}jet_process_stream_next_line(&({})).transpose()){} {{\n",
                        pad,
                        lbl,
                        cx.root_prefix,
                        collection_str,
                        stride_suffix,
                    ));
                    out.push_str(&format!(
                        "{}    let {} = _jet_line_result.unwrap_or_else(|_e| {}jet_panic({:?}, {}, &format!(\"{{:?}}\", _e)));\n",
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
                    let coll_rust = user_type_rust(coll_type);
                    let iter_rust = user_type_rust(iter_type);
                    out.push_str(&format!(
                        "{}{{ let mut _jet_it = <{coll_rust} as user_Iterable>::iter(({collection_str}));\n",
                        pad,
                    ));
                    out.push_str(&format!(
                        "{}    {}for _jet_item in std::iter::from_fn(|| <{iter_rust} as user_Iterator>::next(&mut _jet_it)){} {{\n",
                        pad, lbl, stride_suffix,
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
                        if matches!(&collection.ty, Type::List(_) | Type::FixedList { .. })
                            || matches!(
                                &collection.ty,
                                Type::Apply { name, args }
                                    if matches!(name.as_str(), "View" | "ViewMut") && args.len() == 1
                            )
                        {
                            // D-RANGE-EXCL1=C: sequence two-binding → index then item.
                            let iter_form = if *by_value {
                                format!("({})", collection_str)
                            } else if *columnar {
                                format!("({}).iter_aos()", collection_str)
                            } else {
                                format!("({}).iter().cloned()", collection_str)
                            };
                            out.push_str(&format!(
                                "{}{}for (_jet_i, _jet_item) in {}{} .enumerate() {{\n",
                                pad, lbl, iter_form, stride_suffix
                            ));
                            out.push_str(&format!(
                                "{}    let {} = _jet_i as i64;\n",
                                pad,
                                mangle(var)
                            ));
                            out.push_str(&format!(
                                "{}    let {} = _jet_item;\n",
                                pad,
                                mangle(v2)
                            ));
                        } else {
                            out.push_str(&format!(
                                "{}{}for (_jet_k, _jet_v) in ({}).iter(){} {{\n",
                                pad, lbl, collection_str, stride_suffix
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
                    }
                    None => {
                        if let Type::Map { key, value, .. } = &collection.ty {
                            let fields = vec![
                                ("key".to_string(), (**key).clone()),
                                ("value".to_string(), (**value).clone()),
                            ];
                            let tuple = crate::Codegen::Tuples::tuple_struct_name(&fields);
                            out.push_str(&format!(
                                "{}{}for (_jet_k, _jet_v) in ({}).iter(){} {{\n",
                                pad, lbl, collection_str, stride_suffix
                            ));
                            out.push_str(&format!(
                                "{}    let {} = {} {{ {}: _jet_k.clone(), {}: _jet_v.clone() }};\n",
                                pad,
                                mangle(var),
                                tuple,
                                mangle("key"),
                                mangle("value")
                            ));
                            emit_tir_stmts_nested(body, cx, out, indent + 1, active_deferred_closes);
                            out.push_str(&format!("{}}}\n", pad));
                            if stride_wrapper { out.push_str(&format!("{}}}\n", pad)); }
                            return;
                        }
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
                            format!("{}{}", iter_form, stride_suffix),
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
            if stride_wrapper {
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
        // c109 Phase 18: an audited `#Unsafe { … }` region — `unsafe { … }`, byte-for-byte
        // `emit_stmts`'s `Stmt::Unsafe` arm (the `#Audit` annotation emits nothing). I1:
        // emitted ONLY for a source `#Unsafe` gate.
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
        // D-SHIELDNAME1=A: `#Shield { … }` — enter a cancellation-shield region, install
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
        // D-REACTCORE1: `#Reactive { … }` — register a reactive effect at this point.
        TStmt::Reactive { closure, .. } => {
            out.push_str(&format!(
                "{}{}jet_std::jet_reactive_effect_rooted({});\n",
                pad, cx.root_prefix, closure
            ));
        }
        TStmt::TaskGroup { group, body } => {
            out.push_str(&format!("{}{{\n", pad));
            out.push_str(&format!(
                "{}    let {} = {}jet_std::JetTaskGroup::new();\n",
                pad,
                group.rust_place(),
                cx.root_prefix
            ));
            emit_tir_stmts_nested(body, cx, out, indent + 1, active_deferred_closes);
            out.push_str(&format!("{}}}\n", pad));
        }
        TStmt::Region(body) | TStmt::Impure(body) => {
            out.push_str(&format!("{}{{\n", pad));
            emit_tir_stmts_nested(body, cx, out, indent + 1, active_deferred_closes);
            out.push_str(&format!("{}}}\n", pad));
        }
        // D-LAYOUT1 / D-LAYOUT-GATES1: `layout NAME { … }`. NOT wrapped in a
        // nested Rust block — `name` must stay a live Rust local for
        // statements AFTER this one (`NAME.value(v)`, `NAME.suggest(…)`),
        // unlike `Region`/taskgroup, which are genuinely lexical.
        TStmt::Layout {
            handle,
            label,
            body,
        } => {
            out.push_str(&format!(
                "{}let {} = jet_layout::Handle::new({:?});\n",
                pad,
                handle.rust_place(),
                label
            ));
            emit_tir_stmts_inline(body, cx, out, indent, active_deferred_closes);
        }
        // D-TXN1–D-TXN4 (ratified 2026-06-24): `#Transact(name) { … }` block — open a
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
                Some(h) => Some(h.rust_place()),
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
                    for (slot, rollback_ty) in snapshots {
                        let place = slot.rust_place();
                        match rollback_ty {
                            None => {
                                out.push_str(&format!(
                                    "{}{}jet_txn::snapshot(&mut {}, &mut {});\n",
                                    inner_pad, cx.root_prefix, handle, place
                                ));
                            }
                            Some(ty) => {
                                out.push_str(&format!(
                                    "{}{{ let __snap = ({}).snapshot(); {}jet_txn::snapshot_custom(&mut {}, &mut {}, __snap, {}::restore); }}\n",
                                    inner_pad, place, cx.root_prefix, handle, place, user_type_rust(&ty.name())
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
        // c109 Phase 19: a `#Context(field: value) { … }` block — a plain block with one
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
            subject,
            arms,
            else_body,
        } => {
            let subject_str = emit_tir_expr(subject, cx);
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
