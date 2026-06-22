use super::*;
use crate::AST::{
    ElseBranch, EnumLitArg,
    Expr, ForKind, IfStmt, LValue, LambdaBody,
    OrFallback, Pattern, Stmt, StrPart,
};
use crate::Diagnostics::Span;
use crate::Syntax;
use std::collections::HashSet;

pub(crate) fn walk_stmts_for_const_refs(stmts: &[Stmt], const_names: &[String], taken: &mut HashSet<String>) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(e) => walk_expr_for_const_refs(e, const_names, taken),
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
                    ForKind::In { collection } => {
                        walk_expr_for_const_refs(collection, const_names, taken);
                    }
                }
                walk_stmts_for_const_refs(body, const_names, taken);
            }
            Stmt::Switch {
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
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => {}
            Stmt::Loop { body: inner, .. } | Stmt::Unsafe { body: inner, .. } | Stmt::Region { body: inner, .. } => {
                walk_stmts_for_const_refs(inner, const_names, taken);
            }
            Stmt::ComptimeIf { cond, then_body, else_body, .. } => {
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
        }
    }
}

pub(crate) fn walk_if_for_const_refs(ifs: &IfStmt, const_names: &[String], taken: &mut HashSet<String>) {
    walk_expr_for_const_refs(&ifs.cond, const_names, taken);
    walk_stmts_for_const_refs(&ifs.then_body, const_names, taken);
    match &ifs.else_branch {
        Some(ElseBranch::Else(b)) => walk_stmts_for_const_refs(b, const_names, taken),
        Some(ElseBranch::ElseIf(next)) => walk_if_for_const_refs(next, const_names, taken),
        None => {}
    }
}

pub(crate) fn walk_expr_for_const_refs(expr: &Expr, const_names: &[String], taken: &mut HashSet<String>) {
    match expr {
        Expr::PtrFromAddr { addr, .. } => walk_expr_for_const_refs(addr, const_names, taken),
        Expr::Ident(name, _) => {
            if const_names.iter().any(|c| c == name) {
                taken.insert(name.clone());
            }
        }
        Expr::Str(parts, _) => {
            for p in parts {
                if let StrPart::Interp(e) = p {
                    walk_expr_for_const_refs(e, const_names, taken);
                }
            }
        }
        Expr::Call(c) => {
            for a in &c.args {
                walk_expr_for_const_refs(&a.expr, const_names, taken);
            }
        }
        Expr::Unary(_, inner, _) | Expr::Deref(inner, _) | Expr::Field(inner, _, _) => {
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
        Expr::Present(inner, _) => walk_expr_for_const_refs(inner, const_names, taken),
        Expr::Absent(_) | Expr::Todo { .. } => {}
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
                OrFallback::Return(None, _) | OrFallback::Panic { .. } => {}
            }
            let _ = is_option;
        }
        Expr::PatternTest { subject, .. } => walk_expr_for_const_refs(subject, const_names, taken),
        Expr::Binary(_, l, r, _) => {
            walk_expr_for_const_refs(l, const_names, taken);
            walk_expr_for_const_refs(r, const_names, taken);
        }
        Expr::Char(_, _) | Expr::Int(_, _, _) | Expr::Float(_, _) | Expr::Bool(_, _) => {}
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
        Expr::Unary(_, inner, _) => expr_refs_name(inner, name),
        Expr::Binary(_, l, r, _) => expr_refs_name(l, name) || expr_refs_name(r, name),
        Expr::Call(c) => c.args.iter().any(|a| expr_refs_name(&a.expr, name)),
        Expr::CallValue { callee, args, .. } => {
            expr_refs_name(callee, name) || args.iter().any(|a| expr_refs_name(&a.expr, name))
        }
        Expr::Field(inner, _, _) | Expr::Present(inner, _) | Expr::Try(inner, _, _) => {
            expr_refs_name(inner, name)
        }
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
        Expr::PatternTest { subject, .. } => expr_refs_name(subject, name),
        Expr::Lambda(_) => false,
        Expr::Str(parts, _) => parts.iter().any(|p| {
            if let StrPart::Interp(e) = p {
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
        Expr::Int(_, _, _)
        | Expr::Float(_, _)
        | Expr::Bool(_, _)
        | Expr::Char(_, _)
        | Expr::Absent(_)
        | Expr::Todo { .. }
        | Expr::Deref(_, _) => false,
    }
}

pub(crate) fn stmt_refs_name(stmt: &Stmt, name: &str) -> bool {
    match stmt {
        Stmt::Expr(e) => expr_refs_name(e, name),
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
                ForKind::In { collection } => expr_refs_name(collection, name),
            };
            coll || body.iter().any(|s| stmt_refs_name(s, name))
        }
        Stmt::Switch {
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
        Stmt::Loop { body, .. } | Stmt::Unsafe { body, .. } | Stmt::Region { body, .. } => body.iter().any(|s| stmt_refs_name(s, name)),
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) | Stmt::Return(None, _) => false,
        Stmt::ComptimeIf { cond, then_body, else_body, .. } => {
            expr_refs_name(cond, name)
                || then_body.iter().any(|s| stmt_refs_name(s, name))
                || else_body.as_ref().is_some_and(|eb| eb.iter().any(|s| stmt_refs_name(s, name)))
        }
        Stmt::ContextBlock { fields, body, .. } => {
            fields.iter().any(|(_, e, _)| expr_refs_name(e, name))
                || body.iter().any(|s| stmt_refs_name(s, name))
        }
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

pub(crate) fn lambda_body_view_return_span(checker: &Checker<'_>, body: &LambdaBody) -> Option<Span> {
    match body {
        LambdaBody::Expr(e) => checker.is_view_call(e).then(|| e.span()),
        LambdaBody::Block(stmts) => {
            for stmt in stmts {
                if let Some(span) = stmt_view_return_span(checker, stmt) {
                    return Some(span);
                }
            }
            for stmt in stmts.iter().rev() {
                if let Stmt::Expr(e) = stmt {
                    return checker.is_view_call(e).then(|| e.span());
                }
            }
            None
        }
    }
}

pub(crate) fn stmt_view_return_span(checker: &Checker<'_>, stmt: &Stmt) -> Option<Span> {
    match stmt {
        Stmt::Return(Some(e), _) if checker.is_view_call(e) => Some(e.span()),
        Stmt::If(i) => {
            for stmt in &i.then_body {
                if let Some(span) = stmt_view_return_span(checker, stmt) {
                    return Some(span);
                }
            }
            i.else_branch
                .as_ref()
                .and_then(|branch| else_view_return_span(checker, branch))
        }
        Stmt::While { body, .. } | Stmt::Loop { body, .. } | Stmt::Unsafe { body, .. } | Stmt::Region { body, .. } => body
            .iter()
            .find_map(|stmt| stmt_view_return_span(checker, stmt)),
        Stmt::For { body, .. } => body
            .iter()
            .find_map(|stmt| stmt_view_return_span(checker, stmt)),
        Stmt::Switch {
            arms, else_body, ..
        } => arms
            .iter()
            .find_map(|arm| {
                arm.body
                    .iter()
                    .find_map(|stmt| stmt_view_return_span(checker, stmt))
            })
            .or_else(|| {
                else_body.as_ref().and_then(|body| {
                    body.iter()
                        .find_map(|stmt| stmt_view_return_span(checker, stmt))
                })
            }),
        _ => None,
    }
}

pub(crate) fn else_view_return_span(checker: &Checker<'_>, branch: &ElseBranch) -> Option<Span> {
    match branch {
        ElseBranch::Else(stmts) => stmts
            .iter()
            .find_map(|stmt| stmt_view_return_span(checker, stmt)),
        ElseBranch::ElseIf(i) => {
            for stmt in &i.then_body {
                if let Some(span) = stmt_view_return_span(checker, stmt) {
                    return Some(span);
                }
            }
            i.else_branch
                .as_ref()
                .and_then(|branch| else_view_return_span(checker, branch))
        }
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
        Expr::Binary(_, l, r, _) => {
            expr_collect_captures(l, bound, read, mut_cap);
            expr_collect_captures(r, bound, read, mut_cap);
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
        | Expr::Present(inner, _)
        | Expr::Try(inner, _, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _) => expr_collect_captures(inner, bound, read, mut_cap),
        Expr::MethodCall { receiver, args, .. } => {
            expr_collect_captures(receiver, bound, read, mut_cap);
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
        Expr::PatternTest { subject, .. } => expr_collect_captures(subject, bound, read, mut_cap),
        Expr::Str(parts, _) => {
            for p in parts {
                if let StrPart::Interp(ex) = p {
                    expr_collect_captures(ex, bound, read, mut_cap);
                }
            }
        }
        Expr::Lambda(_) => {}
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
        Stmt::Expr(e) => expr_collect_captures(e, bound, read, mut_cap),
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
                ForKind::In { collection } => {
                    expr_collect_captures(collection, bound, read, mut_cap);
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
        } => {
            expr_collect_captures(subject, bound, read, mut_cap);
            // `it` is synthesised by the when-checker when the subject is a
            // non-ident fallible value; always treat it as bound so that the
            // `| it == ok(n)` pattern subjects are not treated as free vars.
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
                                if let crate::AST::PatSlot::Bind(b) = slot {
                                    arm_bound.insert(b.clone());
                                }
                            }
                        }
                        Pattern::Absent(_) | Pattern::Range { .. } => {}
                        Pattern::Or(alts, _) => {
                            // Insert bindings from first alt (all alts bind same names).
                            if let Some(first) = alts.first() {
                                if let Pattern::Variant { bindings, .. } = first {
                                    for slot in bindings {
                                        if let crate::AST::PatSlot::Bind(b) = slot {
                                            arm_bound.insert(b.clone());
                                        }
                                    }
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
        Stmt::Loop { body, .. } | Stmt::Unsafe { body, .. } | Stmt::Region { body, .. } => {
            let mut body_bound = bound.clone();
            block_collect_captures(body, &mut body_bound, read, mut_cap);
        }
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) | Stmt::Return(None, _) => {}
        Stmt::ComptimeIf { then_body, else_body, .. } => {
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
