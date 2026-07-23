use super::*;
use crate::Syntax;
use crate::AST::{
    ElseBranch, EnumLitArg, Expr, ForKind, IfStmt, LValue, LambdaBody, OrFallback, Pattern, Stmt,
    StrPart, StructPatField,
};
use std::collections::HashSet;

pub(crate) fn walk_stmts_for_const_refs(
    stmts: &[Stmt],
    const_names: &[String],
    taken: &mut HashSet<String>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(e) | Stmt::Yield(e, _) => walk_expr_for_const_refs(e, const_names, taken),
            Stmt::Val(b) => walk_expr_for_const_refs(&b.init, const_names, taken),
            Stmt::Assign { value, .. } => walk_expr_for_const_refs(value, const_names, taken),
            Stmt::Return(Some(e), _) => walk_expr_for_const_refs(e, const_names, taken),
            Stmt::Return(None, _) => {}
            Stmt::If(ifs) => walk_if_for_const_refs(ifs, const_names, taken),
            Stmt::While { cond, body, .. } => {
                walk_expr_for_const_refs(cond, const_names, taken);
                walk_stmts_for_const_refs(body, const_names, taken);
            }
            Stmt::For { kind, body, .. } => {
                match kind {
                    ForKind::Range { start, end, step } => {
                        walk_expr_for_const_refs(start, const_names, taken);
                        walk_expr_for_const_refs(end, const_names, taken);
                        if let Some(step) = step {
                            walk_expr_for_const_refs(step, const_names, taken);
                        }
                    }
                    ForKind::In { collection, step } => {
                        walk_expr_for_const_refs(collection, const_names, taken);
                        if let Some(step) = step {
                            walk_expr_for_const_refs(step, const_names, taken);
                        }
                    }
                }
                walk_stmts_for_const_refs(body, const_names, taken);
            }
            Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            }
            | Stmt::ComptimeSwitch {
                subject,
                arms,
                else_body,
                ..
            } => {
                walk_expr_for_const_refs(subject, const_names, taken);
                for a in arms {
                    walk_expr_for_const_refs(&a.cond, const_names, taken);
                    walk_stmts_for_const_refs(&a.body, const_names, taken);
                }
                walk_stmts_for_const_refs(else_body.as_deref().unwrap_or(&[]), const_names, taken);
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => {
            }
            Stmt::CountedLoop {
                init,
                cond,
                step,
                body: inner,
                ..
            } => {
                walk_expr_for_const_refs(&init.init, const_names, taken);
                walk_expr_for_const_refs(cond, const_names, taken);
                walk_stmts_for_const_refs(inner, const_names, taken);
                if let Some(step) = step {
                    walk_stmts_for_const_refs(std::slice::from_ref(step.as_ref()), const_names, taken);
                }
            }
            Stmt::Loop { body: inner, .. }
            | Stmt::Unsafe { body: inner, .. }
            | Stmt::Impure { body: inner, .. }
            | Stmt::Reactive { body: inner, .. }
            | Stmt::Shield { body: inner, .. }
            | Stmt::Off { body: inner, .. }
            | Stmt::DebugOnly { body: inner, .. }
            | Stmt::Region { body: inner, .. }
            | Stmt::Policy { body: inner, .. }
            | Stmt::TaskGroup { body: inner, .. }
            | Stmt::Layout { body: inner, .. }
            | Stmt::Caps { body: inner, .. }
            | Stmt::Grant { body: inner, .. }
            | Stmt::Transact { body: inner, .. }
            | Stmt::AssumeDet { body: inner, .. } => {
                walk_stmts_for_const_refs(inner, const_names, taken);
            }
            // D-CTMARKER1: walk comptime block body for const refs.
            Stmt::ComptimeBlock { body, .. } => walk_stmts_for_const_refs(body, const_names, taken),
            Stmt::ComptimeIf {
                cond,
                then_body,
                else_body,
                ..
            } => {
                walk_expr_for_const_refs(cond, const_names, taken);
                walk_stmts_for_const_refs(then_body, const_names, taken);
                if let Some(eb) = else_body {
                    walk_stmts_for_const_refs(eb, const_names, taken);
                }
            }
            Stmt::ContextBlock { fields, body, .. } => {
                for (_, e, _) in fields {
                    walk_expr_for_const_refs(e, const_names, taken);
                }
                walk_stmts_for_const_refs(body, const_names, taken);
            }
            // D-TERM1 (ratified 2026-06-22): walk live block body for const refs.
            Stmt::Live { body, .. } => {
                walk_stmts_for_const_refs(body, const_names, taken);
            }
            // D-DOTSCOPE1: walk a scope-member region body for const address refs.
            Stmt::ScopeMember { body, .. } => {
                walk_stmts_for_const_refs(body, const_names, taken);
            }
        }
    }
}

pub(crate) fn walk_if_for_const_refs(
    ifs: &IfStmt,
    const_names: &[String],
    taken: &mut HashSet<String>,
) {
    walk_expr_for_const_refs(&ifs.cond, const_names, taken);
    walk_stmts_for_const_refs(&ifs.then_body, const_names, taken);
    match &ifs.else_branch {
        Some(ElseBranch::Else(b)) => walk_stmts_for_const_refs(b, const_names, taken),
        Some(ElseBranch::ElseIf(next)) => walk_if_for_const_refs(next, const_names, taken),
        None => {}
    }
}

pub(crate) fn walk_expr_for_const_refs(
    expr: &Expr,
    const_names: &[String],
    taken: &mut HashSet<String>,
) {
    match expr {
        Expr::PtrFromAddr { addr, .. } => walk_expr_for_const_refs(addr, const_names, taken),
        Expr::Ident(name, _) => {
            if const_names.iter().any(|c| c == name) {
                taken.insert(name.clone());
            }
        }
        Expr::Str(parts, _) => {
            for p in parts {
                if let StrPart::Interp(e, _) = p {
                    walk_expr_for_const_refs(e, const_names, taken);
                }
            }
        }
        Expr::Call(c) => {
            for a in &c.args {
                walk_expr_for_const_refs(&a.expr, const_names, taken);
            }
        }
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Field(inner, _, _) => {
            walk_expr_for_const_refs(inner, const_names, taken)
        }
        Expr::OptField { base, .. } => walk_expr_for_const_refs(base, const_names, taken),
        Expr::MethodCall { receiver, args, .. } => {
            walk_expr_for_const_refs(receiver, const_names, taken);
            for a in args {
                walk_expr_for_const_refs(&a.expr, const_names, taken);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, e) in fields {
                walk_expr_for_const_refs(e, const_names, taken);
            }
        }
        Expr::EnumLit { args, .. } => {
            for a in args {
                match a {
                    EnumLitArg::Positional(e) | EnumLitArg::Named { expr: e, .. } => {
                        walk_expr_for_const_refs(e, const_names, taken);
                    }
                }
            }
        }
        Expr::Tainted(inner, _, _) // D-TAINT1: tag erased; recurse into the value.
        | Expr::Present(inner, _) => walk_expr_for_const_refs(inner, const_names, taken),
        Expr::Absent(_) | Expr::ReduceMarker(_, _) | Expr::Todo { .. } | Expr::ComptimeSplice { .. }
        // D-SHIFT1 (c7shift) / D-BINPAT1 (card #506 follow-up): a leaf
        // literal, no nested `Expr` to recurse into.
        | Expr::StrMatchLit(_, _)
        | Expr::BinMatchLit(_, _) => {}
        Expr::Ok(inner, _) | Expr::Err(inner, _) | Expr::Try(inner, _, _) => {
            walk_expr_for_const_refs(inner, const_names, taken);
        }
        Expr::OrFallback {
            value,
            fallback,
            is_option,
            ..
        } => {
            walk_expr_for_const_refs(value, const_names, taken);
            match fallback {
                OrFallback::Value(e) => walk_expr_for_const_refs(e, const_names, taken),
                OrFallback::Return(Some(e), _) => walk_expr_for_const_refs(e, const_names, taken),
                OrFallback::Return(None, _)
                | OrFallback::Panic { .. }
                | OrFallback::Break(_)
                | OrFallback::Continue(_)
                | OrFallback::BreakLabel(..)
                | OrFallback::ContinueLabel(..) => {}
            }
            let _ = is_option;
        }
        Expr::PatternTest { subject, pattern, .. } => {
            walk_expr_for_const_refs(subject, const_names, taken);
            if let Pattern::Struct { fields, .. } = pattern {
                for field in fields {
                    if let StructPatField::Value { value, .. } = field {
                        walk_expr_for_const_refs(value, const_names, taken);
                    }
                }
            }
        }
        Expr::Binary(_, l, r, _) => {
            walk_expr_for_const_refs(l, const_names, taken);
            walk_expr_for_const_refs(r, const_names, taken);
        }
        Expr::CompareChain { operands, .. } => {
            for e in operands {
                walk_expr_for_const_refs(e, const_names, taken);
            }
        }
        Expr::Char(_, _)
        | Expr::Int(_, _, _, _)
        | Expr::Float(_, _, _)
        | Expr::Bool(_, _)
        | Expr::UnitLit { .. } => {}
        Expr::ListLit(elems, _) => {
            for e in elems {
                walk_expr_for_const_refs(e, const_names, taken);
            }
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, e) in fields {
                walk_expr_for_const_refs(e, const_names, taken);
            }
        }
        Expr::MapLit(entries, _) => {
            for (k, v) in entries {
                walk_expr_for_const_refs(k, const_names, taken);
                walk_expr_for_const_refs(v, const_names, taken);
            }
        }
        Expr::Index { base, index, .. } => {
            walk_expr_for_const_refs(base, const_names, taken);
            walk_expr_for_const_refs(index, const_names, taken);
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            walk_expr_for_const_refs(base, const_names, taken);
            walk_expr_for_const_refs(start, const_names, taken);
            walk_expr_for_const_refs(end, const_names, taken);
        }
        Expr::CallValue { callee, args, .. } => {
            walk_expr_for_const_refs(callee, const_names, taken);
            for a in args {
                walk_expr_for_const_refs(&a.expr, const_names, taken);
            }
        }
        Expr::Lambda(lam) => match &lam.body {
            LambdaBody::Expr(e) => walk_expr_for_const_refs(e, const_names, taken),
            LambdaBody::Block(stmts) => walk_stmts_for_const_refs(stmts, const_names, taken),
        },
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            walk_expr_for_const_refs(cond, const_names, taken);
            walk_stmts_for_const_refs(then_body, const_names, taken);
            walk_expr_for_const_refs(then_value, const_names, taken);
            walk_stmts_for_const_refs(else_body, const_names, taken);
            walk_expr_for_const_refs(else_value, const_names, taken);
        }
        Expr::FanOut { callee, items, .. } => {
            walk_expr_for_const_refs(callee, const_names, taken);
            for item in items {
                walk_expr_for_const_refs(item, const_names, taken);
            }
        }
        Expr::Paren(inner, _) => walk_expr_for_const_refs(inner, const_names, taken),
        Expr::Spread(inner, _) => walk_expr_for_const_refs(inner, const_names, taken),
    }
}

pub(crate) fn lambda_body_refs_name(body: &LambdaBody, name: &str) -> bool {
    match body {
        LambdaBody::Expr(e) => expr_refs_name(e, name),
        LambdaBody::Block(stmts) => stmts.iter().any(|s| stmt_refs_name(s, name)),
    }
}

pub(crate) fn expr_refs_name(e: &Expr, name: &str) -> bool {
    match e {
        Expr::PtrFromAddr { addr, .. } => expr_refs_name(addr, name),
        Expr::Ident(n, _) => n == name,
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _) => expr_refs_name(inner, name),
        Expr::Binary(_, l, r, _) => expr_refs_name(l, name) || expr_refs_name(r, name),
        Expr::CompareChain { operands, .. } => operands.iter().any(|e| expr_refs_name(e, name)),
        Expr::Call(c) => {
            c.name == name || c.args.iter().any(|a| expr_refs_name(&a.expr, name))
        }
        Expr::CallValue { callee, args, .. } => {
            expr_refs_name(callee, name) || args.iter().any(|a| expr_refs_name(&a.expr, name))
        }
        Expr::Field(inner, _, _)
        | Expr::Tainted(inner, _, _)
        | Expr::Present(inner, _)
        | Expr::Try(inner, _, _) => expr_refs_name(inner, name),
        Expr::OptField { base, .. } => expr_refs_name(base, name),
        Expr::MethodCall { receiver, args, .. } => {
            expr_refs_name(receiver, name) || args.iter().any(|a| expr_refs_name(&a.expr, name))
        }
        Expr::Index { base, index, .. } => {
            expr_refs_name(base, name) || expr_refs_name(index, name)
        }
        Expr::Slice {
            base, start, end, ..
        } => expr_refs_name(base, name) || expr_refs_name(start, name) || expr_refs_name(end, name),
        Expr::ListLit(elems, _) => elems.iter().any(|el| expr_refs_name(el, name)),
        Expr::TupleLit(fields, _, _) => fields.iter().any(|(_, e)| expr_refs_name(e, name)),
        Expr::MapLit(entries, _) => entries
            .iter()
            .any(|(k, v)| expr_refs_name(k, name) || expr_refs_name(v, name)),
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, _, f)| expr_refs_name(f, name)),
        Expr::EnumLit { args, .. } => args.iter().any(|a| match a {
            EnumLitArg::Positional(e) => expr_refs_name(e, name),
            EnumLitArg::Named { expr, .. } => expr_refs_name(expr, name),
        }),
        Expr::Ok(inner, _) | Expr::Err(inner, _) => expr_refs_name(inner, name),
        Expr::OrFallback {
            value, fallback, ..
        } => {
            expr_refs_name(value, name)
                || match fallback {
                    OrFallback::Value(e) => expr_refs_name(e, name),
                    OrFallback::Return(Some(e), _) => expr_refs_name(e, name),
                    _ => false,
                }
        }
        Expr::PatternTest {
            subject, pattern, ..
        } => {
            expr_refs_name(subject, name)
                || match pattern {
                    Pattern::Struct { fields, .. } => fields.iter().any(|field| match field {
                        StructPatField::Value { value, .. } => expr_refs_name(value, name),
                        StructPatField::Bind { .. } => false,
                    }),
                    _ => false,
                }
        }
        Expr::Lambda(_) => false,
        Expr::Str(parts, _) => parts.iter().any(|p| {
            if let StrPart::Interp(e, _) = p {
                expr_refs_name(e, name)
            } else {
                false
            }
        }),
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            expr_refs_name(cond, name)
                || expr_refs_name(then_value, name)
                || expr_refs_name(else_value, name)
                || then_body.iter().any(|s| stmt_refs_name(s, name))
                || else_body.iter().any(|s| stmt_refs_name(s, name))
        }
        Expr::FanOut { callee, items, .. } => {
            expr_refs_name(callee, name) || items.iter().any(|e| expr_refs_name(e, name))
        }
        Expr::Int(_, _, _, _)
        | Expr::Float(_, _, _)
        | Expr::Bool(_, _)
        | Expr::Char(_, _)
        | Expr::Absent(_)
        | Expr::ReduceMarker(_, _)
        | Expr::Todo { .. }
        | Expr::UnitLit { .. }
        | Expr::ComptimeSplice { .. }
        // D-SHIFT1 (c7shift) / D-BINPAT1 (card #506 follow-up): a leaf
        // literal, no nested `Expr` to recurse into.
        | Expr::StrMatchLit(_, _)
        | Expr::BinMatchLit(_, _) => false,
        Expr::Paren(inner, _) => expr_refs_name(inner, name),
        Expr::Spread(inner, _) => expr_refs_name(inner, name),
    }
}

pub(crate) fn stmt_refs_name(stmt: &Stmt, name: &str) -> bool {
    match stmt {
        Stmt::Expr(e) | Stmt::Yield(e, _) => expr_refs_name(e, name),
        Stmt::Val(b) => expr_refs_name(&b.init, name),
        Stmt::Assign { target, value, .. } => {
            lvalue_refs_name(target, name) || expr_refs_name(value, name)
        }
        Stmt::Return(Some(e), _) => expr_refs_name(e, name),
        Stmt::If(i) => {
            expr_refs_name(&i.cond, name)
                || i.then_body.iter().any(|s| stmt_refs_name(s, name))
                || i.else_branch
                    .as_ref()
                    .is_some_and(|e| else_refs_name(e, name))
        }
        Stmt::While { cond, body, .. } => {
            expr_refs_name(cond, name) || body.iter().any(|s| stmt_refs_name(s, name))
        }
        Stmt::For { kind, body, .. } => {
            let coll = match kind {
                ForKind::Range { start, end, step } => {
                    expr_refs_name(start, name)
                        || expr_refs_name(end, name)
                        || step.as_ref().is_some_and(|s| expr_refs_name(s, name))
                }
                ForKind::In { collection, step } => expr_refs_name(collection, name)
                    || step.as_ref().is_some_and(|s| expr_refs_name(s, name)),
            };
            coll || body.iter().any(|s| stmt_refs_name(s, name))
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        }
        | Stmt::ComptimeSwitch {
            subject,
            arms,
            else_body,
            ..
        } => {
            expr_refs_name(subject, name)
                || arms.iter().any(|a| {
                    expr_refs_name(&a.cond, name) || a.body.iter().any(|s| stmt_refs_name(s, name))
                })
                || else_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| stmt_refs_name(s, name)))
        }
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            expr_refs_name(&init.init, name)
                || expr_refs_name(cond, name)
                || body.iter().any(|s| stmt_refs_name(s, name))
                || step.as_ref().is_some_and(|step| stmt_refs_name(step, name))
        }
        Stmt::Loop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::DebugOnly { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::AssumeDet { body, .. } => body.iter().any(|s| stmt_refs_name(s, name)),
        Stmt::Off { .. } => false,
        Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::BreakLabel(..)
        | Stmt::ContinueLabel(..)
        | Stmt::Return(None, _) => false,
        // D-CTMARKER1: comptime block body may reference names.
        Stmt::ComptimeBlock { body, .. } => body.iter().any(|s| stmt_refs_name(s, name)),
        Stmt::ComptimeIf {
            cond,
            then_body,
            else_body,
            ..
        } => {
            expr_refs_name(cond, name)
                || then_body.iter().any(|s| stmt_refs_name(s, name))
                || else_body
                    .as_ref()
                    .is_some_and(|eb| eb.iter().any(|s| stmt_refs_name(s, name)))
        }
        Stmt::ContextBlock { fields, body, .. } => {
            fields.iter().any(|(_, e, _)| expr_refs_name(e, name))
                || body.iter().any(|s| stmt_refs_name(s, name))
        }
        // D-TERM1 (ratified 2026-06-22): live block references same as its body.
        Stmt::Live { body, .. } => body.iter().any(|s| stmt_refs_name(s, name)),
        Stmt::ScopeMember { body, .. } => body.iter().any(|s| stmt_refs_name(s, name)),
    }
}

pub(crate) fn else_refs_name(e: &ElseBranch, name: &str) -> bool {
    match e {
        ElseBranch::Else(stmts) => stmts.iter().any(|s| stmt_refs_name(s, name)),
        ElseBranch::ElseIf(i) => {
            expr_refs_name(&i.cond, name)
                || i.then_body.iter().any(|s| stmt_refs_name(s, name))
                || i.else_branch
                    .as_ref()
                    .is_some_and(|e| else_refs_name(e, name))
        }
    }
}

pub(crate) fn lvalue_refs_name(lv: &LValue, name: &str) -> bool {
    match lv {
        LValue::Local { name: n, .. } => n == name,
        LValue::Index { base, index, .. } => {
            expr_refs_name(base, name) || expr_refs_name(index, name)
        }
        // D-MUTSELF1: `place.field = v` references whatever the base place references.
        LValue::Field { base, .. } => expr_refs_name(base, name),
    }
}

pub(crate) fn lambda_collect_captures(
    body: &LambdaBody,
    params: &HashSet<String>,
    read: &mut HashSet<String>,
    mut_cap: &mut HashSet<String>,
) {
    let mut bound = params.clone();
    match body {
        LambdaBody::Expr(e) => expr_collect_captures(e, &bound, read, mut_cap),
        LambdaBody::Block(stmts) => block_collect_captures(stmts, &mut bound, read, mut_cap),
    }
}

pub(crate) fn block_collect_captures(
    stmts: &[Stmt],
    bound: &mut HashSet<String>,
    read: &mut HashSet<String>,
    mut_cap: &mut HashSet<String>,
) {
    for s in stmts {
        stmt_collect_captures(s, bound, read, mut_cap);
    }
}

pub(crate) fn expr_collect_captures(
    e: &Expr,
    bound: &HashSet<String>,
    read: &mut HashSet<String>,
    mut_cap: &mut HashSet<String>,
) {
    match e {
        Expr::Ident(n, _) if !bound.contains(n) => {
            read.insert(n.clone());
        }
        Expr::Unary(_, inner, _) => expr_collect_captures(inner, bound, read, mut_cap),
        Expr::IncDec { operand, .. } => {
            expr_collect_captures(operand, bound, read, mut_cap);
            if let Expr::Ident(name, _) = operand.as_ref() {
                if !bound.contains(name) {
                    mut_cap.insert(name.clone());
                }
            } else if let Expr::Field(base, _, _) = operand.as_ref() {
                if let Some(root) = expr_root_ident(base) {
                    if !bound.contains(root) {
                        mut_cap.insert(root.to_string());
                    }
                }
            }
        }
        Expr::Binary(_, l, r, _) => {
            expr_collect_captures(l, bound, read, mut_cap);
            expr_collect_captures(r, bound, read, mut_cap);
        }
        Expr::CompareChain { operands, .. } => {
            for e in operands {
                expr_collect_captures(e, bound, read, mut_cap);
            }
        }
        Expr::Call(c) => {
            for a in &c.args {
                expr_collect_captures(&a.expr, bound, read, mut_cap);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            expr_collect_captures(callee, bound, read, mut_cap);
            for a in args {
                expr_collect_captures(&a.expr, bound, read, mut_cap);
            }
        }
        Expr::Field(inner, _, _)
        | Expr::Tainted(inner, _, _) // D-TAINT1: tag erased; recurse into the value.
        | Expr::Present(inner, _)
        | Expr::Try(inner, _, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _) => expr_collect_captures(inner, bound, read, mut_cap),
        Expr::MethodCall { receiver, args, .. } => {
            // A leading-capital identifier in receiver position is a static type
            // (`Int.parse`, `UserId.from_int`), not a value captured by the lambda.
            let static_type = matches!(
                receiver.as_ref(),
                Expr::Ident(name, _)
                    if name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            );
            if !static_type {
                expr_collect_captures(receiver, bound, read, mut_cap);
            }
            for a in args {
                expr_collect_captures(&a.expr, bound, read, mut_cap);
            }
        }
        Expr::Index { base, index, .. } => {
            expr_collect_captures(base, bound, read, mut_cap);
            expr_collect_captures(index, bound, read, mut_cap);
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            expr_collect_captures(base, bound, read, mut_cap);
            expr_collect_captures(start, bound, read, mut_cap);
            expr_collect_captures(end, bound, read, mut_cap);
        }
        Expr::ListLit(elems, _) => {
            for el in elems {
                expr_collect_captures(el, bound, read, mut_cap);
            }
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, e) in fields {
                expr_collect_captures(e, bound, read, mut_cap);
            }
        }
        Expr::MapLit(entries, _) => {
            for (k, v) in entries {
                expr_collect_captures(k, bound, read, mut_cap);
                expr_collect_captures(v, bound, read, mut_cap);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, f) in fields {
                expr_collect_captures(f, bound, read, mut_cap);
            }
        }
        Expr::EnumLit { args, .. } => {
            for a in args {
                match a {
                    EnumLitArg::Positional(ex) => expr_collect_captures(ex, bound, read, mut_cap),
                    EnumLitArg::Named { expr, .. } => {
                        expr_collect_captures(expr, bound, read, mut_cap);
                    }
                }
            }
        }
        Expr::OrFallback {
            value, fallback, ..
        } => {
            expr_collect_captures(value, bound, read, mut_cap);
            match fallback {
                OrFallback::Value(ex) => expr_collect_captures(ex, bound, read, mut_cap),
                OrFallback::Return(Some(ex), _) => {
                    expr_collect_captures(ex, bound, read, mut_cap);
                }
                _ => {}
            }
        }
        Expr::PatternTest { subject, pattern, .. } => {
            expr_collect_captures(subject, bound, read, mut_cap);
            if let Pattern::Struct { fields, .. } = pattern {
                for field in fields {
                    if let crate::AST::StructPatField::Value { value, .. } = field {
                        expr_collect_captures(value, bound, read, mut_cap);
                    }
                }
            }
        }
        Expr::Str(parts, _) => {
            for p in parts {
                if let StrPart::Interp(ex, _) = p {
                    expr_collect_captures(ex, bound, read, mut_cap);
                }
            }
        }
        Expr::Lambda(_) => {}
        Expr::Paren(inner, _) => expr_collect_captures(inner, bound, read, mut_cap),
        _ => {}
    }
}

pub(crate) fn stmt_collect_captures(
    stmt: &Stmt,
    bound: &mut HashSet<String>,
    read: &mut HashSet<String>,
    mut_cap: &mut HashSet<String>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Yield(e, _) => expr_collect_captures(e, bound, read, mut_cap),
        Stmt::Val(b) => {
            expr_collect_captures(&b.init, bound, read, mut_cap);
            bound.insert(b.name.clone());
        }
        Stmt::Assign { target, value, .. } => {
            if let LValue::Local { name, .. } = target {
                if !bound.contains(name) {
                    mut_cap.insert(name.clone());
                }
            } else if let LValue::Index { base, index, .. } = target {
                expr_collect_captures(base, bound, read, mut_cap);
                expr_collect_captures(index, bound, read, mut_cap);
                if let Expr::Ident(n, _) = base.as_ref() {
                    if !bound.contains(n) {
                        mut_cap.insert(n.clone());
                    }
                }
            }
            expr_collect_captures(value, bound, read, mut_cap);
        }
        Stmt::Return(Some(e), _) => expr_collect_captures(e, bound, read, mut_cap),
        Stmt::If(i) => {
            expr_collect_captures(&i.cond, bound, read, mut_cap);
            let mut then_bound = bound.clone();
            block_collect_captures(&i.then_body, &mut then_bound, read, mut_cap);
            if let Some(e) = &i.else_branch {
                let mut else_bound = bound.clone();
                else_collect_captures(e, &mut else_bound, read, mut_cap);
            }
        }
        Stmt::While { cond, body, .. } => {
            expr_collect_captures(cond, bound, read, mut_cap);
            let mut body_bound = bound.clone();
            block_collect_captures(body, &mut body_bound, read, mut_cap);
        }
        Stmt::For {
            var,
            var2,
            kind,
            body,
            ..
        } => {
            match kind {
                ForKind::Range { start, end, step } => {
                    expr_collect_captures(start, bound, read, mut_cap);
                    expr_collect_captures(end, bound, read, mut_cap);
                    if let Some(step) = step {
                        expr_collect_captures(step, bound, read, mut_cap);
                    }
                }
                ForKind::In { collection, step } => {
                    expr_collect_captures(collection, bound, read, mut_cap);
                    if let Some(step) = step {
                        expr_collect_captures(step, bound, read, mut_cap);
                    }
                }
            }
            let mut body_bound = bound.clone();
            body_bound.insert(var.clone());
            if let Some((name, _)) = var2 {
                body_bound.insert(name.clone());
            }
            block_collect_captures(body, &mut body_bound, read, mut_cap);
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        }
        | Stmt::ComptimeSwitch {
            subject,
            arms,
            else_body,
            ..
        } => {
            expr_collect_captures(subject, bound, read, mut_cap);
            // `it` is synthesised by the when-checker when the subject is a
            // non-ident fallible value; always treat it as bound so that the
            // `| it == Ok(n)` pattern subjects are not treated as free vars.
            let mut when_bound = bound.clone();
            when_bound.insert(Syntax::KW_IT.to_string());
            for a in arms {
                // Collect from the condition using the extended bound set so
                // that the synthesised `it` subject is not treated as a capture.
                expr_collect_captures(&a.cond, &when_bound, read, mut_cap);
                // Add any pattern bindings introduced by the arm condition so
                // they are not treated as captures inside the arm body.
                let mut arm_bound = when_bound.clone();
                if let Expr::PatternTest { pattern, .. } = &a.cond {
                    match pattern {
                        Pattern::Ok { binding, .. }
                        | Pattern::Err { binding, .. }
                        | Pattern::Present { binding, .. } => {
                            arm_bound.insert(binding.clone());
                        }
                        Pattern::Variant { bindings, .. } => {
                            for slot in bindings {
                                if let crate::AST::PatSlot::Bind { name, .. } = slot {
                                    arm_bound.insert(name.clone());
                                }
                            }
                        }
                        Pattern::Absent(_) | Pattern::Range { .. } => {}
                        Pattern::Struct { fields, .. } => {
                            for field in fields {
                                if let crate::AST::StructPatField::Bind { local, .. } = field {
                                    arm_bound.insert(local.clone());
                                }
                            }
                        }
                        Pattern::Or(alts, _) => {
                            // Insert bindings from first alt (all alts bind same names).
                            if let Some(first) = alts.first() {
                                if let Pattern::Variant { bindings, .. } = first {
                                    for slot in bindings {
                                        if let crate::AST::PatSlot::Bind { name, .. } = slot {
                                            arm_bound.insert(name.clone());
                                        }
                                    }
                                }
                            }
                        }
                        // D-PARSESTR1: every hole binds a name into the arm body.
                        Pattern::StrMatch { parts, .. } => {
                            for part in parts {
                                if let crate::AST::StrMatchPart::Hole { name, .. } = part {
                                    arm_bound.insert(name.clone());
                                }
                            }
                        }
                        // D-BINPAT1: every binary-pattern hole binds a name too.
                        Pattern::BinMatch { parts, .. } => {
                            for part in parts {
                                if let crate::AST::BinMatchPart::Hole { name, .. } = part {
                                    arm_bound.insert(name.clone());
                                }
                            }
                        }
                    }
                }
                block_collect_captures(&a.body, &mut arm_bound, read, mut_cap);
            }
            if let Some(b) = else_body {
                let mut else_bound = bound.clone();
                block_collect_captures(b, &mut else_bound, read, mut_cap);
            }
        }
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            expr_collect_captures(&init.init, bound, read, mut_cap);
            bound.insert(init.name.clone());
            expr_collect_captures(cond, bound, read, mut_cap);
            let mut body_bound = bound.clone();
            block_collect_captures(body, &mut body_bound, read, mut_cap);
            if let Some(step) = step {
                stmt_collect_captures(step, bound, read, mut_cap);
            }
        }
        Stmt::Loop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::DebugOnly { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::AssumeDet { body, .. } => {
            let mut body_bound = bound.clone();
            block_collect_captures(body, &mut body_bound, read, mut_cap);
        }
        Stmt::Off { .. } => {}
        Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::BreakLabel(..)
        | Stmt::ContinueLabel(..)
        | Stmt::Return(None, _) => {}
        // D-CTMARKER1: comptime block erases; still walk body for captures (conservative).
        Stmt::ComptimeBlock { body, .. } => {
            let mut body_bound = bound.clone();
            block_collect_captures(body, &mut body_bound, read, mut_cap);
        }
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            let mut then_bound = bound.clone();
            block_collect_captures(then_body, &mut then_bound, read, mut_cap);
            if let Some(eb) = else_body {
                let mut else_bound = bound.clone();
                block_collect_captures(eb, &mut else_bound, read, mut_cap);
            }
        }
        Stmt::ContextBlock { fields, body, .. } => {
            for (_, e, _) in fields {
                expr_collect_captures(e, bound, read, mut_cap);
            }
            let mut body_bound = bound.clone();
            block_collect_captures(body, &mut body_bound, read, mut_cap);
        }
        // D-TERM1 (ratified 2026-06-22): collect captures from live block body.
        Stmt::Live { body, .. } => {
            let mut body_bound = bound.clone();
            block_collect_captures(body, &mut body_bound, read, mut_cap);
        }
        // D-DOTSCOPE1: a scope-member region body is its own block for capture
        // analysis — except `.setup`, whose bindings leak; scanning a fresh clone
        // is conservative (over-captures nothing, under-captures nothing needed).
        Stmt::ScopeMember { body, .. } => {
            let mut body_bound = bound.clone();
            block_collect_captures(body, &mut body_bound, read, mut_cap);
        }
    }
}

pub(crate) fn else_collect_captures(
    e: &ElseBranch,
    bound: &mut HashSet<String>,
    read: &mut HashSet<String>,
    mut_cap: &mut HashSet<String>,
) {
    match e {
        ElseBranch::Else(stmts) => {
            block_collect_captures(stmts, bound, read, mut_cap);
        }
        ElseBranch::ElseIf(i) => {
            expr_collect_captures(&i.cond, bound, read, mut_cap);
            let mut then_bound = bound.clone();
            block_collect_captures(&i.then_body, &mut then_bound, read, mut_cap);
            if let Some(e) = &i.else_branch {
                let mut nested_bound = bound.clone();
                else_collect_captures(e, &mut nested_bound, read, mut_cap);
            }
        }
    }
}

// S62 + D-LIB2: inject synthesised Func nodes into ImplDef items in-place.
// Must run before register_impl_methods so the synthesised methods are visible
// when method lookup is registered.
