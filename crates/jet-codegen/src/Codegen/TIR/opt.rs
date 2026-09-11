use super::{
    TCoreClosureKind, TExpr, TExprKind, TFnValueKind, TIfCond, TJitSpawnBody, TLetTy, TLocal,
    TPlace, TStmt, TLambda, TLambdaBody, TirProgram,
};
use super::{canonical_identity, canonical_payload};
use jet_foundation::CanonicalPass;
use crate::AST::Type;
use std::sync::Arc;

/// Canonicalize loop spellings once, after the complete checked TIR program is built.
///
/// Backends consume only the resulting TIR/MIR shape.  This pass intentionally
/// keeps dynamic conditions, source-backed ranges, non-positive strides, and
/// fixed-width integer expressions in their original forms because equivalence
/// with the counted shape is not proved for those cases here.
pub(crate) fn canonicalize_loop_forms(program: &mut TirProgram) {
    let before = canonical_payload(program);
    let before_identity = canonical_identity(program);
    let mut shared_slots = Vec::new();
    for function in &mut program.funcs {
        visit_shared_slots(&mut function.body, &mut |body| {
            let shared = std::mem::replace(body, empty_shared_body());
            shared_slots.push(SharedSlot { body: shared });
        });
    }
    for spawn in &mut program.spawn_lambdas {
        if let TJitSpawnBody::SharedBlock { body, .. } = &mut spawn.body {
            let shared = std::mem::replace(body, empty_shared_body());
            shared_slots.push(SharedSlot { body: shared });
        }
    }
    canonicalize_shared_slots(&mut shared_slots);
    let mut cursor = 0;
    for function in &mut program.funcs {
        visit_shared_slots(&mut function.body, &mut |body| {
            *body = Arc::clone(&shared_slots[cursor].body);
            cursor += 1;
        });
    }
    for spawn in &mut program.spawn_lambdas {
        if let TJitSpawnBody::SharedBlock { body, .. } = &mut spawn.body {
            *body = Arc::clone(&shared_slots[cursor].body);
            cursor += 1;
        }
    }
    debug_assert_eq!(cursor, shared_slots.len());
    for function in &mut program.funcs {
        canonicalize_stmts(&mut function.body);
    }
    CanonicalPass::record(
        "lowering",
        "tir.canonicalize-loop-forms",
        "crates/jet-codegen/src/Codegen/TIR/opt.rs",
        "tir",
        before,
        before_identity,
        "tir",
        canonical_payload(program),
        canonical_identity(program),
        "preserve",
    );
}

/// Run the checked TIR normalization pass.
///
/// Loop canonicalization happens before lowering. `#Inline(Always)` expansion
/// happens only in the canonical MIR optimizer so all execution adapters share
/// the same control-flow and cleanup semantics.
pub(crate) fn optimize_program(program: &mut TirProgram) {
    canonicalize_loop_forms(program);
}

struct SharedSlot {
    body: Arc<[TStmt]>,
}

fn empty_shared_body() -> Arc<[TStmt]> {
    Arc::from(Vec::<TStmt>::new().into_boxed_slice())
}

fn visit_shared_slots(
    stmts: &mut [TStmt],
    visit: &mut impl FnMut(&mut Arc<[TStmt]>),
) {
    for stmt in stmts {
        match stmt {
            TStmt::Reactive { executable } => {
                if let TLambdaBody::SharedBlock(body) = &mut executable.executable {
                    visit(body);
                }
            }
            TStmt::ContractScope { body, .. }
            | TStmt::TaskGroup { body, .. }
            | TStmt::Loop { body, .. }
            | TStmt::While { body, .. }
            | TStmt::Inline(body)
            | TStmt::DebugOnly(body)
            | TStmt::Unsafe { body, .. }
            | TStmt::SentryPolicy { body, .. }
            | TStmt::Impure(body)
            | TStmt::Region(body)
            | TStmt::Layout { body, .. }
            | TStmt::ContextBlock { body, .. }
            | TStmt::Live { body }
            | TStmt::Shield { body }
            | TStmt::ScopeMember { body, .. }
            | TStmt::Transact { body, .. } => visit_shared_slots(body, visit),
            TStmt::RefutableBind { fallback, .. } => visit_shared_slots(fallback, visit),
            TStmt::GcEdit { stmt, .. } => {
                visit_shared_slots(std::slice::from_mut(stmt.as_mut()), visit)
            }
            TStmt::CountedLoop {
                init, step, body, ..
            } => {
                visit_shared_slots(std::slice::from_mut(init.as_mut()), visit);
                if let Some(step) = step {
                    visit_shared_slots(std::slice::from_mut(step.as_mut()), visit);
                }
                visit_shared_slots(body, visit);
            }
            TStmt::Range { body, .. } | TStmt::ForIn { body, .. } => {
                visit_shared_slots(body, visit)
            }
            TStmt::If {
                then_body,
                else_body,
                ..
            } => {
                visit_shared_slots(then_body, visit);
                if let Some(else_body) = else_body {
                    visit_shared_slots(else_body, visit);
                }
            }
            TStmt::EnumMatch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    visit_shared_slots(&mut arm.body, visit);
                }
                if let Some(else_body) = else_body {
                    visit_shared_slots(else_body, visit);
                }
            }
            TStmt::RangeSwitch {
                arms, else_body, ..
            } => {
                for (_, _, body) in arms {
                    visit_shared_slots(body, visit);
                }
                visit_shared_slots(else_body, visit);
            }
            TStmt::MixedSwitch {
                arms, else_body, ..
            } => {
                for (_, body) in arms {
                    visit_shared_slots(body, visit);
                }
                if let Some(else_body) = else_body {
                    visit_shared_slots(else_body, visit);
                }
            }
            _ => {}
        }
    }
}

fn canonicalize_shared_slots(slots: &mut [SharedSlot]) {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (index, slot) in slots.iter().enumerate() {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| Arc::ptr_eq(&slots[group[0]].body, &slot.body))
        {
            group.push(index);
        } else {
            groups.push(vec![index]);
        }
    }
    for group in groups {
        let first = group[0];
        let mut shared = std::mem::replace(&mut slots[first].body, empty_shared_body());
        for &index in group.iter().skip(1) {
            let duplicate =
                std::mem::replace(&mut slots[index].body, empty_shared_body());
            drop(duplicate);
        }
        if let Some(body) = Arc::get_mut(&mut shared) {
            canonicalize_fixed_stmts(body);
        }
        slots[first].body = shared;
        let shared = Arc::clone(&slots[first].body);
        for &index in group.iter().skip(1) {
            slots[index].body = Arc::clone(&shared);
        }
    }
}

fn counted_loop(
    label: Option<String>,
    var: String,
    start: TExpr,
    end: TExpr,
    step: Option<TExpr>,
    exclusive: bool,
    auto_vectorization: Option<crate::AST::AutoVectorizationFacts>,
    body: Vec<TStmt>,
) -> TStmt {
    let lhs = TExpr {
        ty: Type::Int,
        kind: TExprKind::Local(TLocal::user(var.clone())),
    };
    let step_expr = step.unwrap_or_else(|| TExpr {
        ty: Type::Int,
        kind: TExprKind::IntLit(1, None),
    });
    let cond = TExpr {
        ty: Type::Bool,
        kind: TExprKind::Binary {
            op: if exclusive {
                crate::AST::BinOp::Lt
            } else {
                crate::AST::BinOp::Le
            },
            overflow: false,
            line: 0,
            lhs: Box::new(lhs),
            rhs: Box::new(end),
        },
    };
    TStmt::CountedLoop {
        label,
        init: Box::new(TStmt::Let {
            name: var.clone(),
            kw: "let mut",
            let_ty: TLetTy::Inferred,
            init: start,
            gc_promotion: None,
            gc_transferred: false,
        }),
        cond,
        step: Some(Box::new(TStmt::Assign {
            place: TPlace::Local(TLocal::user(var).as_mutable()),
            op: Some(crate::AST::BinOp::Add),
            value: step_expr,
            clone_value: false,
            line: 0,
        })),
        auto_vectorization,
        body,
    }
}
fn canonicalize_fixed_stmts(stmts: &mut [TStmt]) {
    for stmt in stmts {
        canonicalize_stmt_children(stmt);
        let current = std::mem::replace(stmt, TStmt::Inline(Vec::new()));
        *stmt = match current {
            TStmt::While { label, cond, body } => match &cond.kind {
                TExprKind::BoolLit(true) => TStmt::Loop { label, body },
                TExprKind::BoolLit(false) => TStmt::Inline(Vec::new()),
                _ if invariant_bool_condition(&cond) => TStmt::If {
                    cond: TIfCond::Plain(cond),
                    then_body: vec![TStmt::Loop { label, body }],
                    else_body: None,
                    else_is_elseif: false,
                },
                _ => TStmt::While { label, cond, body },
            },
            TStmt::Range {
                label,
                var,
                source,
                start,
                end,
                step,
                exclusive,
                auto_vectorization,
                body,
            } => match integer_range_step(&source, &start, &end, step.as_ref()) {
                Some(_) => counted_loop(
                    label,
                    var,
                    start,
                    end,
                    step,
                    exclusive,
                    auto_vectorization,
                    body,
                ),
                None => TStmt::Range {
                    label,
                    var,
                    source,
                    start,
                    end,
                    step,
                    exclusive,
                    auto_vectorization,
                    body,
                },
            },
            other => other,
        };
    }
}
fn canonicalize_stmts(stmts: &mut Vec<TStmt>) {
    let mut index = 0;
    while index < stmts.len() {
        canonicalize_stmt_children(&mut stmts[index]);
        let current = std::mem::replace(&mut stmts[index], TStmt::Inline(Vec::new()));
        let replacement = match current {
            TStmt::While { label, cond, body } => match &cond.kind {
                TExprKind::BoolLit(true) => Some(TStmt::Loop { label, body }),
                TExprKind::BoolLit(false) => None,
                _ if invariant_bool_condition(&cond) => Some(TStmt::If {
                    cond: TIfCond::Plain(cond),
                    then_body: vec![TStmt::Loop { label, body }],
                    else_body: None,
                    else_is_elseif: false,
                }),
                _ => Some(TStmt::While { label, cond, body }),
            },
            TStmt::Range {
                label,
                var,
                source,
                start,
                end,
                step,
                exclusive,
                auto_vectorization,
                body,
            } => {
                let canonical_step = integer_range_step(&source, &start, &end, step.as_ref());
                match canonical_step {
                    Some(_) => {
                        let lhs = TExpr {
                            ty: Type::Int,
                            kind: TExprKind::Local(TLocal::user(var.clone())),
                        };
                        let step_expr = step.unwrap_or_else(|| TExpr {
                            ty: Type::Int,
                            kind: TExprKind::IntLit(1, None),
                        });
                        let cond = TExpr {
                            ty: Type::Bool,
                            kind: TExprKind::Binary {
                                op: if exclusive {
                                    crate::AST::BinOp::Lt
                                } else {
                                    crate::AST::BinOp::Le
                                },
                                overflow: false,
                                line: 0,
                                lhs: Box::new(lhs),
                                rhs: Box::new(end),
                            },
                        };
                        let counted_step = TStmt::Assign {
                            place: TPlace::Local(TLocal::user(var.clone()).as_mutable()),
                            op: Some(crate::AST::BinOp::Add),
                            value: step_expr,
                            clone_value: false,
                            line: 0,
                        };
                        Some(TStmt::CountedLoop {
                            label,
                            init: Box::new(TStmt::Let {
                                name: var,
                                kw: "let mut",
                                let_ty: TLetTy::Inferred,
                                init: start,
                                gc_promotion: None,
                                gc_transferred: false,
                            }),
                            cond,
                            step: Some(Box::new(counted_step)),
                            auto_vectorization,
                            body,
                        })
                    }
                    None => Some(TStmt::Range {
                        label,
                        var,
                        source,
                        start,
                        end,
                        step,
                        exclusive,
                        auto_vectorization,
                        body,
                    }),
                }
            }
            other => Some(other),
        };
        if let Some(replacement) = replacement {
            stmts[index] = replacement;
            index += 1;
        } else {
            // SourceSpan is a separate preceding node and remains intact.
            stmts.remove(index);
        }
    }
}

fn invariant_bool_condition(cond: &TExpr) -> bool {
    let TExprKind::Local(local) = &cond.kind else {
        return false;
    };
    matches!(cond.ty.without_user_tags(), Type::Bool)
        && !local.mutable
        && !local.deref
        && !local.is_persistent()
}

fn integer_range_step(
    source: &Option<TExpr>,
    start: &TExpr,
    end: &TExpr,
    step: Option<&TExpr>,
) -> Option<i64> {
    if source.is_some() {
        return None;
    }
    integer_literal(start)?;
    let end_value = integer_literal(end)?;
    let step_value = match step {
        Some(step) => integer_literal(step).filter(|value| *value > 0)?,
        None => 1,
    };
    // The range cursor exhausts on an overflowing final advance; counted
    // assignment must not turn that defined stop into an arithmetic trap.
    end_value.checked_add(step_value)?;
    Some(step_value)
}

fn integer_literal(expr: &TExpr) -> Option<i64> {
    if !matches!(expr.ty.without_user_tags(), Type::Int) {
        return None;
    }
    match &expr.kind {
        TExprKind::IntLit(value, _) => Some(*value),
        _ => None,
    }
}

fn canonicalize_stmt_children(stmt: &mut TStmt) {
    match stmt {
        TStmt::Contract { contract } => {
            canonicalize_expr(&mut contract.condition);
            canonicalize_expr(&mut contract.message);
        }
        TStmt::ContractScope {
            pre, body, post, ..
        } => {
            for contract in pre.iter_mut().chain(post.iter_mut()) {
                canonicalize_expr(&mut contract.condition);
                canonicalize_expr(&mut contract.message);
            }
            canonicalize_stmts(body);
        }
        TStmt::Let { init, .. } => canonicalize_expr(init),
        TStmt::RefutableBind { init, fallback, .. } => {
            canonicalize_expr(init);
            canonicalize_stmts(fallback);
        }
        TStmt::GcEdit {
            index_temp, stmt, ..
        } => {
            if let Some((_, index)) = index_temp {
                canonicalize_expr(index);
            }
            canonicalize_stmt_children(stmt);
        }
        TStmt::SplitViews { owner, .. } => {
            if let Some(owner) = owner {
                canonicalize_expr(owner);
            }
        }
        TStmt::TupleDestructure { init, .. }
        | TStmt::StructDestructure { init, .. }
        | TStmt::ListDestructure { init, .. } => canonicalize_expr(init),
        TStmt::Assign { place, value, .. } => {
            if let TPlace::Expr(place) = place {
                canonicalize_expr(place);
            }
            canonicalize_expr(value);
        }
        TStmt::Return(value) => {
            if let Some(value) = value {
                canonicalize_expr(value);
            }
        }
        TStmt::ExprStmt(value) | TStmt::BreakValue { value, .. } => canonicalize_expr(value),
        TStmt::TaskGroup { limit, body, .. } => {
            if let Some(limit) = limit {
                canonicalize_expr(limit);
            }
            canonicalize_stmts(body);
        }
        TStmt::DeferClose { close, .. } => canonicalize_expr(close),
        TStmt::If {
            cond,
            then_body,
            else_body,
            ..
        } => {
            canonicalize_cond(cond);
            canonicalize_stmts(then_body);
            if let Some(else_body) = else_body {
                canonicalize_stmts(else_body);
            }
        }
        TStmt::Loop { body, .. }
        | TStmt::Inline(body)
        | TStmt::DebugOnly(body)
        | TStmt::Unsafe { body, .. }
        | TStmt::SentryPolicy { body, .. }
        | TStmt::Impure(body)
        | TStmt::Region(body)
        | TStmt::Live { body }
        | TStmt::Shield { body }
        | TStmt::ScopeMember { body, .. }
        | TStmt::Layout { body, .. } => canonicalize_stmts(body),
        TStmt::While { cond, body, .. } => {
            canonicalize_expr(cond);
            canonicalize_stmts(body);
        }
        TStmt::ContextBlock { guards, body } => {
            for (_, guard) in guards {
                canonicalize_expr(guard);
            }
            canonicalize_stmts(body);
        }
        TStmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            canonicalize_stmt_children(init);
            canonicalize_expr(cond);
            if let Some(step) = step {
                canonicalize_stmt_children(step);
            }
            canonicalize_stmts(body);
        }
        TStmt::Range {
            source,
            start,
            end,
            step,
            body,
            ..
        } => {
            if let Some(source) = source {
                canonicalize_expr(source);
            }
            canonicalize_expr(start);
            canonicalize_expr(end);
            if let Some(step) = step {
                canonicalize_expr(step);
            }
            canonicalize_stmts(body);
        }
        TStmt::EnumMatch {
            scrutinee,
            arms,
            else_body,
            ..
        } => {
            canonicalize_expr(scrutinee);
            for arm in arms {
                canonicalize_stmts(&mut arm.body);
            }
            if let Some(else_body) = else_body {
                canonicalize_stmts(else_body);
            }
        }
        TStmt::RangeSwitch {
            subject,
            arms,
            else_body,
        } => {
            canonicalize_expr(subject);
            for (_, _, body) in arms {
                canonicalize_stmts(body);
            }
            canonicalize_stmts(else_body);
        }
        TStmt::IndexAssign {
            base, index, value, ..
        } => {
            canonicalize_expr(base);
            canonicalize_expr(index);
            canonicalize_expr(value);
        }
        TStmt::IndexFieldAssign(assign) => {
            canonicalize_expr(&mut assign.base);
            canonicalize_expr(&mut assign.index);
            canonicalize_expr(&mut assign.value);
        }
        TStmt::IndexHookAssign {
            base, index, value, ..
        } => {
            canonicalize_expr(base);
            canonicalize_expr(index);
            canonicalize_expr(value);
        }
        TStmt::MathSwizzleAssign { base, value, .. } => {
            canonicalize_expr(base);
            canonicalize_expr(value);
        }
        TStmt::ForIn {
            source,
            collection,
            step,
            body,
            ..
        } => {
            canonicalize_expr(source);
            canonicalize_expr(collection);
            if let Some(step) = step {
                canonicalize_expr(step);
            }
            canonicalize_stmts(body);
        }
        TStmt::MixedSwitch {
            subject,
            arms,
            else_body,
            ..
        } => {
            canonicalize_expr(subject);
            for (condition, body) in arms {
                canonicalize_expr(condition);
                canonicalize_stmts(body);
            }
            if let Some(else_body) = else_body {
                canonicalize_stmts(else_body);
            }
        }
        TStmt::Reactive { executable } => canonicalize_lambda(executable),
        TStmt::Transact { body, .. } => canonicalize_stmts(body),
        _ => {}
    }
}

fn canonicalize_cond(cond: &mut TIfCond) {
    match cond {
        TIfCond::Plain(expr)
        | TIfCond::IfLet { subj: expr, .. }
        | TIfCond::IsNone { subj: expr }
        | TIfCond::Matches { subj: expr, .. } => canonicalize_expr(expr),
        TIfCond::And { left, right } => {
            canonicalize_cond(left);
            canonicalize_cond(right);
        }
        TIfCond::WithPrelude { prelude, cond } => {
            canonicalize_stmts(prelude);
            canonicalize_cond(cond);
        }
    }
}

fn canonicalize_lambda(lambda: &mut TLambda) {
    match &mut lambda.executable {
        TLambdaBody::Expr(expr) => canonicalize_expr(expr),
        TLambdaBody::Block(body) => canonicalize_stmts(body),
        TLambdaBody::SharedBlock(body) => {
            if let Some(body) = Arc::get_mut(body) {
                canonicalize_fixed_stmts(body);
            }
        }
    }
}

fn canonicalize_core_closure(kind: &mut TCoreClosureKind) {
    match kind {
        TCoreClosureKind::Spawn {
            group, executable, ..
        } => {
            if let Some(group) = group {
                canonicalize_expr(group);
            }
            canonicalize_lambda(executable);
        }
        TCoreClosureKind::Realtime {
            rate,
            frames,
            executable,
        } => {
            canonicalize_expr(rate);
            canonicalize_expr(frames);
            canonicalize_lambda(executable);
        }
        TCoreClosureKind::Serve { addr, executable } => {
            canonicalize_expr(addr);
            canonicalize_lambda(executable);
        }
        TCoreClosureKind::OnInterrupt { callback } => canonicalize_expr(callback),
        TCoreClosureKind::Guard { executable }
        | TCoreClosureKind::OnCommit { executable, .. }
        | TCoreClosureKind::OnRollback { executable, .. }
        | TCoreClosureKind::ReactiveDerived { executable, .. }
        | TCoreClosureKind::ReactiveEffect { executable, .. }
        | TCoreClosureKind::UiReactiveRender { executable, .. } => canonicalize_lambda(executable),
        TCoreClosureKind::UiPreview {
            name,
            viewport,
            executable,
            ..
        } => {
            canonicalize_expr(name);
            if let Some(viewport) = viewport {
                canonicalize_expr(viewport);
            }
            canonicalize_lambda(executable);
        }
        TCoreClosureKind::UiButtonOnClick {
            label,
            shortcut,
            accessible_label,
            executable,
            ..
        } => {
            canonicalize_expr(label);
            canonicalize_expr(shortcut);
            canonicalize_expr(accessible_label);
            canonicalize_lambda(executable);
        }
        TCoreClosureKind::UiTextInputOnDrop {
            state,
            ime,
            executable,
            ..
        } => {
            canonicalize_expr(state);
            canonicalize_expr(ime);
            canonicalize_lambda(executable);
        }
    }
}

fn canonicalize_fn_value(kind: &mut TFnValueKind) {
    match kind {
        TFnValueKind::NamedFn { lambda, .. } => {
            if let Some(lambda) = lambda {
                canonicalize_lambda(lambda);
            }
        }
        TFnValueKind::Policy {
            policy_args, callee, ..
        } => {
            for arg in policy_args {
                canonicalize_expr(&mut arg.value);
            }
            canonicalize_expr(callee);
        }
        TFnValueKind::Call { callee, args } => {
            canonicalize_expr(callee);
            for arg in args {
                canonicalize_expr(&mut arg.value);
            }
        }
        TFnValueKind::Send { value } => canonicalize_expr(value),
    }
}

fn canonicalize_expr(expr: &mut TExpr) {
    match &mut expr.kind {
        TExprKind::InlineBlock(body) => canonicalize_stmts(body),
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => {
            canonicalize_cond(cond);
            canonicalize_stmts(then_body);
            canonicalize_expr(then_value);
            canonicalize_stmts(else_body);
            canonicalize_expr(else_value);
        }
        TExprKind::Lambda(lambda) => canonicalize_lambda(lambda),
        TExprKind::CoreClosureCall { kind } => canonicalize_core_closure(kind),
        TExprKind::FnValue { kind } => canonicalize_fn_value(kind),
        TExprKind::Call { args, .. }
        | TExprKind::StaticCall { args, .. }
        | TExprKind::ModuleCall { args, .. } => {
            for arg in args {
                canonicalize_expr(&mut arg.value);
            }
        }
        TExprKind::ExternCall { args, .. } => {
            for arg in args {
                canonicalize_expr(&mut arg.value);
            }
        }
        TExprKind::MethodCall { recv, args, .. } => {
            canonicalize_expr(recv);
            for arg in args {
                canonicalize_expr(&mut arg.value);
            }
        }
        TExprKind::BuiltinMethod { args, .. }
        | TExprKind::HandleMethod { args, .. }
        | TExprKind::ClosureMethod { args, .. }
        | TExprKind::MathBuiltin { args, .. }
        | TExprKind::PreciseBuiltin { args, .. }
        | TExprKind::CoreCall { args, .. } => {
            for arg in args {
                canonicalize_expr(arg);
            }
        }
        TExprKind::OrFallback { value, fallback } => {
            canonicalize_expr(value);
            match fallback {
                super::TOrFallback::Value(value)
                | super::TOrFallback::Return(Some(value)) => canonicalize_expr(value),
                super::TOrFallback::Panic { msg, .. } => canonicalize_expr(msg),
                _ => {}
            }
        }
        TExprKind::Try { inner, note, .. } => {
            canonicalize_expr(inner);
            if let Some(note) = note {
                canonicalize_expr(note);
            }
        }
        TExprKind::Binary { lhs, rhs, .. }
        | TExprKind::LayoutCompare { lhs, rhs, .. }
        | TExprKind::OverflowOpt { lhs, rhs, .. }
        | TExprKind::NumericBinaryMethod {
            recv: lhs,
            arg: rhs,
            ..
        } => {
            canonicalize_expr(lhs);
            canonicalize_expr(rhs);
        }
        TExprKind::IncDec { place, .. } => {
            if let TPlace::Expr(place) = place {
                canonicalize_expr(place);
            }
        }
        TExprKind::Borrow { place, .. } => canonicalize_expr(place),
        TExprKind::Unary { operand, .. }
        | TExprKind::LayoutLit { inner: operand }
        | TExprKind::DistinctCtor { arg: operand, .. }
        | TExprKind::RangeCheckedCtor { arg: operand, .. }
        | TExprKind::DistinctConvert { arg: operand, .. }
        | TExprKind::Print(operand)
        | TExprKind::Drop(operand)
        | TExprKind::Close(operand)
        | TExprKind::ResourceNew(operand)
        | TExprKind::Deref(operand)
        | TExprKind::RawOf(operand)
        | TExprKind::Clone(operand)
        | TExprKind::ExplicitCopy(operand)
        | TExprKind::MaterializeView(operand)
        | TExprKind::DistinctRaw(operand)
        | TExprKind::Present(operand)
        | TExprKind::Ok(operand)
        | TExprKind::Err(operand)
        | TExprKind::OptField { base: operand, .. }
        | TExprKind::PatternMatches { subj: operand, .. }
        | TExprKind::NumericMethod { recv: operand, .. }
        | TExprKind::TaskGroupAll { tasks: operand }
        | TExprKind::TaskGroupRace { tasks: operand }
        | TExprKind::TaskGroupAny { tasks: operand }
        | TExprKind::HostBorrowCallback { callable: operand, .. } => canonicalize_expr(operand),
        TExprKind::SelectRecv { builder, channel } => {
            canonicalize_expr(builder);
            canonicalize_expr(channel);
        }
        TExprKind::SelectAfter {
            builder,
            duration,
            value,
        } => {
            canonicalize_expr(builder);
            canonicalize_expr(duration);
            if let Some(value) = value {
                canonicalize_expr(value);
            }
        }
        TExprKind::SelectWait { builder, .. } => canonicalize_expr(builder),
        TExprKind::AmbientInput { prompt } => {
            if let Some(prompt) = prompt {
                canonicalize_expr(prompt);
            }
        }
        TExprKind::OptionLift2 { f, a, b } => {
            canonicalize_expr(f);
            canonicalize_expr(a);
            canonicalize_expr(b);
        }
        TExprKind::RequireStop { kind, .. } => match kind {
            super::TRequireKind::Require { cond, msg } => {
                canonicalize_expr(cond);
                if let Some(msg) = msg {
                    canonicalize_expr(msg);
                }
            }
            super::TRequireKind::RequireEq { left, right } => {
                canonicalize_expr(left);
                canonicalize_expr(right);
            }
            super::TRequireKind::Panic { msg } => canonicalize_expr(msg),
        },
        TExprKind::CompareChain { operands, .. } => {
            for operand in operands {
                canonicalize_expr(operand);
            }
        }
        TExprKind::UnitConvert { arg, rounding, .. } => {
            canonicalize_expr(arg);
            if let Some((_, value)) = rounding {
                canonicalize_expr(value);
            }
        }
        TExprKind::EnumLit { payload, .. } => match payload {
            super::TEnumPayload::Unit => {}
            super::TEnumPayload::Positional(args) => {
                for arg in args {
                    canonicalize_expr(&mut arg.value);
                }
            }
            super::TEnumPayload::Named(args) => {
                for (_, arg) in args {
                    canonicalize_expr(&mut arg.value);
                }
            }
        },
        TExprKind::StructLit { fields, .. } => {
            for (_, value, _) in fields {
                canonicalize_expr(value);
            }
        }
        TExprKind::Field { recv, .. }
        | TExprKind::SharedGuardValue { guard: recv, .. }
        | TExprKind::SharedGuardMap { guard: recv, .. }
        | TExprKind::SharedGuardSplit { guard: recv, .. }
        | TExprKind::MathSwizzleRead { recv, .. } => canonicalize_expr(recv),
        TExprKind::SharedGuardWait {
            guard,
            condition,
            predicate,
        } => {
            canonicalize_expr(guard);
            canonicalize_expr(condition);
            canonicalize_lambda(predicate);
        }
        TExprKind::ConditionNotify { condition, .. } => canonicalize_expr(condition),
        TExprKind::PtrFromAddr { addr, .. } => canonicalize_expr(addr),
        TExprKind::ListLit(values) | TExprKind::ColumnarListLit { elems: values, .. } => {
            for value in values {
                canonicalize_expr(value);
            }
        }
        TExprKind::ListSpread { parts } => {
            for part in parts {
                match part {
                    super::ListSpreadPart::Elem(value) | super::ListSpreadPart::Spread(value) => {
                        canonicalize_expr(value)
                    }
                }
            }
        }
        TExprKind::ColumnarGather { base, index, .. }
        | TExprKind::ColumnarColumnRead { base, index, .. }
        | TExprKind::Index { base, index, .. }
        | TExprKind::IndexHook { base, index, .. }
        | TExprKind::MathLaneIndex { base, index, .. } => {
            canonicalize_expr(base);
            canonicalize_expr(index);
        }
        TExprKind::PoolSlot { pool, id, .. } => {
            canonicalize_expr(pool);
            canonicalize_expr(id);
        }
        TExprKind::Slice {
            base,
            start,
            end,
            range,
            ..
        } => {
            canonicalize_expr(base);
            canonicalize_expr(start);
            canonicalize_expr(end);
            if let Some(range) = range {
                canonicalize_expr(range);
            }
        }
        TExprKind::TupleLit { fields, .. } => {
            for (_, value) in fields {
                canonicalize_expr(value);
            }
        }
        TExprKind::MapLit(fields) => {
            for (left, right) in fields {
                canonicalize_expr(left);
                canonicalize_expr(right);
            }
        }
        TExprKind::StrLit(parts) => {
            for part in parts {
                if let super::TStrPart::Interp(value, _) = part {
                    canonicalize_expr(value);
                }
            }
        }
        TExprKind::JSONLit { arg, .. } | TExprKind::DBValueLit { arg, .. } => {
            if let Some(arg) = arg {
                canonicalize_expr(&mut arg.0);
            }
        }
        _ => {}
    }
    fold_expr(expr);
}
fn fold_expr(expr: &mut TExpr) {
    let folded = match &expr.kind {
        TExprKind::Unary { op, operand } => fold_unary(expr, *op, operand),
        TExprKind::Binary {
            op, lhs, rhs, ..
        } => fold_binary(expr, *op, lhs, rhs),
        _ => None,
    };
    if let Some(replacement) = folded {
        *expr = replacement;
    }
}

fn fold_unary(expr: &TExpr, op: crate::AST::UnOp, operand: &TExpr) -> Option<TExpr> {
    match op {
        crate::AST::UnOp::Not => match &operand.kind {
            TExprKind::BoolLit(value) if expr.ty.without_user_tags() == &Type::Bool => Some(TExpr {
                ty: expr.ty.clone(),
                kind: TExprKind::BoolLit(!*value),
            }),
            _ => None,
        },
        crate::AST::UnOp::Neg => {
            let value = integer_literal_value(operand)?;
            let value = value.checked_neg()?;
            if !fits_integer_type(&expr.ty, value) {
                return None;
            }
            Some(integer_expr(expr.ty.clone(), value))
        }
    }
}

fn fold_binary(
    expr: &TExpr,
    op: crate::AST::BinOp,
    lhs: &TExpr,
    rhs: &TExpr,
) -> Option<TExpr> {
    match (&lhs.kind, &rhs.kind) {
        (TExprKind::BoolLit(left), TExprKind::BoolLit(right))
            if matches!(
                op,
                crate::AST::BinOp::And
                    | crate::AST::BinOp::Or
                    | crate::AST::BinOp::Eq
                    | crate::AST::BinOp::Ne
            ) =>
        {
            let value = match op {
                crate::AST::BinOp::And => *left && *right,
                crate::AST::BinOp::Or => *left || *right,
                crate::AST::BinOp::Eq => left == right,
                crate::AST::BinOp::Ne => left != right,
                _ => unreachable!(),
            };
            return Some(TExpr {
                ty: expr.ty.clone(),
                kind: TExprKind::BoolLit(value),
            });
        }
        (TExprKind::CharLit(left), TExprKind::CharLit(right))
            if matches!(
                op,
                crate::AST::BinOp::Eq
                    | crate::AST::BinOp::Ne
                    | crate::AST::BinOp::Lt
                    | crate::AST::BinOp::Gt
                    | crate::AST::BinOp::Le
                    | crate::AST::BinOp::Ge
            ) =>
        {
            let value = match op {
                crate::AST::BinOp::Eq => left == right,
                crate::AST::BinOp::Ne => left != right,
                crate::AST::BinOp::Lt => left < right,
                crate::AST::BinOp::Gt => left > right,
                crate::AST::BinOp::Le => left <= right,
                crate::AST::BinOp::Ge => left >= right,
                _ => unreachable!(),
            };
            return Some(TExpr {
                ty: expr.ty.clone(),
                kind: TExprKind::BoolLit(value),
            });
        }
        _ => {}
    }
    let left = integer_literal_value(lhs)?;
    let right = integer_literal_value(rhs)?;
    let value = fold_integer_binary(op, left, right)?;
    match op {
        crate::AST::BinOp::Eq
        | crate::AST::BinOp::Ne
        | crate::AST::BinOp::Lt
        | crate::AST::BinOp::Gt
        | crate::AST::BinOp::Le
        | crate::AST::BinOp::Ge => Some(TExpr {
            ty: expr.ty.clone(),
            kind: TExprKind::BoolLit(value != 0),
        }),
        _ if expr.ty.without_user_tags().is_integer() && fits_integer_type(&expr.ty, value) => {
            Some(integer_expr(expr.ty.clone(), value))
        }
        _ => None,
    }
}

fn integer_literal_value(expr: &TExpr) -> Option<i64> {
    expr.ty.without_user_tags().is_integer().then_some(())?;
    match &expr.kind {
        TExprKind::IntLit(value, _) => Some(*value),
        _ => None,
    }
}

fn integer_expr(ty: Type, value: i64) -> TExpr {
    let width = match ty.without_user_tags() {
        Type::IntN { signed, bits } => Some((*signed, *bits)),
        _ => None,
    };
    TExpr {
        ty,
        kind: TExprKind::IntLit(value, width),
    }
}

fn fits_integer_type(ty: &Type, value: i64) -> bool {
    match ty.without_user_tags() {
        Type::Int => true,
        Type::IntN { signed, bits } => {
            let (lo, hi) = crate::AST::int_range(*signed, *bits);
            (lo..=hi).contains(&(value as i128))
        }
        Type::InlineRange { lo, hi, .. } => (lo..=hi).contains(&&value),
        _ => false,
    }
}

fn fold_integer_binary(op: crate::AST::BinOp, left: i64, right: i64) -> Option<i64> {
    use crate::AST::BinOp;
    match op {
        BinOp::Add => left.checked_add(right),
        BinOp::Sub => left.checked_sub(right),
        BinOp::Mul => left.checked_mul(right),
        BinOp::Div => left.checked_div(right),
        BinOp::FloorDiv => floor_div(left, right),
        BinOp::Mod => {
            let quotient = floor_div(left, right)?;
            left.checked_sub(quotient.checked_mul(right)?)
        }
        BinOp::Rem => left.checked_rem(right),
        BinOp::Pow => {
            if right < 0 {
                None
            } else {
                left.checked_pow(u32::try_from(right).ok()?)
            }
        }
        BinOp::BitAnd => Some(left & right),
        BinOp::BitOr => Some(left | right),
        BinOp::BitXor => Some(left ^ right),
        BinOp::Shl => left.checked_shl(u32::try_from(right).ok()?),
        BinOp::Shr => left.checked_shr(u32::try_from(right).ok()?),
        BinOp::Eq => Some((left == right) as i64),
        BinOp::Ne => Some((left != right) as i64),
        BinOp::Lt => Some((left < right) as i64),
        BinOp::Gt => Some((left > right) as i64),
        BinOp::Le => Some((left <= right) as i64),
        BinOp::Ge => Some((left >= right) as i64),
        BinOp::And => Some((left != 0 && right != 0) as i64),
        BinOp::Or => Some((left != 0 || right != 0) as i64),
        BinOp::Compare => None,
    }
}

fn floor_div(left: i64, right: i64) -> Option<i64> {
    let quotient = left.checked_div(right)?;
    let remainder = left.checked_rem(right)?;
    if remainder != 0 && (left < 0) != (right < 0) {
        quotient.checked_sub(1)
    } else {
        Some(quotient)
    }
}

