//! Purity check: walk the call graph reachable from a comptime `init` and
//! reject the first impure call (IO, FFI) with the path that reached it
//! (E0951). `embed_file`, `embed_bytes`, `find`, `panic`, and `require` are allowed.

use std::collections::{HashMap, HashSet};

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{
    EnumLitArg, Expr, Func, LambdaBody, LValue, OrFallback, Pattern, Stmt, StrPart,
    StructPatField,
};

use super::Diagnostics::impurity_diag;

/// Walk the call graph reachable from `init`; reject the first impure call
/// (IO, FFI) with the path that reached it (E0951). `embed_file`,
/// `embed_bytes`, `find`, `panic`, and `require` are allowed.
pub(super) fn check_purity_stmts(
    stmts: &[Stmt],
    funcs: &HashMap<String, &Func>,
    extern_names: &HashSet<String>,
) -> Result<(), Diagnostic> {
    let mut visited = HashSet::new();
    let mut path = Vec::new();
    for stmt in stmts {
        check_purity_stmt(stmt, funcs, extern_names, &mut visited, &mut path)?;
    }
    Ok(())
}

pub(super) fn check_purity(
    init: &Expr,
    funcs: &HashMap<String, &Func>,
    extern_names: &HashSet<String>,
) -> Result<(), Diagnostic> {
    let mut visited = HashSet::new();
    let mut path = Vec::new();
    check_purity_expr(init, funcs, extern_names, &mut visited, &mut path)
}

fn impure_builtin(name: &str) -> bool {
    crate::Syntax::IMPURE_BUILTINS.contains(&name)
}

fn check_purity_expr(
    e: &Expr,
    funcs: &HashMap<String, &Func>,
    externs: &HashSet<String>,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
) -> Result<(), Diagnostic> {
    let mut result = Ok(());
    crate::Comptime::walk_calls(e, &mut |name, span| {
        if result.is_err() {
            return;
        }
        if impure_builtin(name) || externs.contains(name) {
            result = Err(impurity_diag(name, path, span));
        } else if let Some(f) = funcs.get(name) {
            if visited.insert(name.to_string()) {
                path.push(name.to_string());
                for stmt in &f.body {
                    if result.is_err() {
                        break;
                    }
                    result = check_purity_stmt(stmt, funcs, externs, visited, path);
                }
                path.pop();
            }
        }
    });
    result
}

fn check_purity_stmt(
    s: &Stmt,
    funcs: &HashMap<String, &Func>,
    externs: &HashSet<String>,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
) -> Result<(), Diagnostic> {
    let mut result = Ok(());
    walk_stmt_expr_nodes(s, false, &mut |e| {
        if result.is_ok() {
            result = check_purity_expr(e, funcs, externs, visited, path);
        }
    });
    result
}

/// Visit every direct `Call` name in an expression tree (shallow over
/// nested functions — recursion is driven by the purity walker).
pub fn walk_calls(e: &Expr, f: &mut impl FnMut(&str, Span)) {
    walk_expr_nodes(e, false, &mut |expr| {
        if let Expr::Call(call) = expr {
            f(&call.name, call.name_span);
        }
    });
}

pub(super) fn reachable_owned_funcs(
    init: &Expr,
    funcs: &HashMap<String, Func>,
) -> HashMap<String, Func> {
    fn known_name(name: &str, funcs: &HashMap<String, Func>) -> Option<String> {
        if funcs.contains_key(name) {
            return Some(name.to_string());
        }
        name.split_once('.')
            .map(|(module, symbol)| format!("{module}::{symbol}"))
            .filter(|qualified| funcs.contains_key(qualified))
    }

    fn collect_name(
        name: String,
        funcs: &HashMap<String, Func>,
        visited: &mut HashSet<String>,
        reachable: &mut HashMap<String, Func>,
    ) {
        if !visited.insert(name.clone()) {
            return;
        }
        let Some(function) = funcs.get(&name) else {
            return;
        };
        reachable.insert(name, function.clone());

        let mut dependencies = Vec::new();
        for statement in &function.body {
            walk_stmt_expr_nodes(statement, true, &mut |expr| match expr {
                Expr::Call(call) => {
                    if let Some(name) = known_name(&call.name, funcs) {
                        dependencies.push(name);
                    }
                }
                Expr::Ident(name, _) => {
                    if let Some(name) = known_name(name, funcs) {
                        dependencies.push(name);
                    }
                }
                _ => {}
            });
        }
        for dependency in dependencies {
            collect_name(dependency, funcs, visited, reachable);
        }
    }

    let mut roots = Vec::new();
    walk_expr_nodes(init, true, &mut |expr| match expr {
        Expr::Call(call) => {
            if let Some(name) = known_name(&call.name, funcs) {
                roots.push(name);
            }
        }
        Expr::Ident(name, _) => {
            if let Some(name) = known_name(name, funcs) {
                roots.push(name);
            }
        }
        _ => {}
    });

    let mut visited = HashSet::new();
    let mut reachable = HashMap::new();
    for root in roots {
        collect_name(root, funcs, &mut visited, &mut reachable);
    }
    reachable
}

fn walk_expr_nodes(e: &Expr, include_suppressed: bool, f: &mut impl FnMut(&Expr)) {
    f(e);
    match e {
        Expr::Str(parts, _) => {
            for part in parts {
                if let StrPart::Interp(expr, _) = part {
                    walk_expr_nodes(expr, include_suppressed, f);
                }
            }
        }
        Expr::StrMatchLit(_, _)
        | Expr::BinMatchLit(_, _)
        | Expr::Int(_, _, _, _)
        | Expr::Float(_, _, _)
        | Expr::Bool(_, _)
        | Expr::Char(_, _)
        | Expr::Ident(_, _)
        | Expr::UnitLit { .. }
        | Expr::Absent(_)
        | Expr::Todo { .. }
        | Expr::ReduceMarker(_, _)
        | Expr::ComptimeSplice { .. } => {}
        Expr::ListLit(items, _) | Expr::CompareChain { operands: items, .. } => {
            for item in items {
                walk_expr_nodes(item, include_suppressed, f);
            }
        }
        Expr::Spread(inner, _)
        | Expr::Unary(_, inner, _)
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Field(inner, _, _)
        | Expr::Tainted(inner, _, _)
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _)
        | Expr::Paren(inner, _)
        | Expr::IncDec { operand: inner, .. } => {
            walk_expr_nodes(inner, include_suppressed, f);
        }
        Expr::OptField { base, .. } => walk_expr_nodes(base, include_suppressed, f),
        Expr::MapLit(entries, _) => {
            for (key, value) in entries {
                walk_expr_nodes(key, include_suppressed, f);
                walk_expr_nodes(value, include_suppressed, f);
            }
        }
        Expr::Index { base, index, .. } => {
            walk_expr_nodes(base, include_suppressed, f);
            walk_expr_nodes(index, include_suppressed, f);
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            walk_expr_nodes(base, include_suppressed, f);
            walk_expr_nodes(start, include_suppressed, f);
            walk_expr_nodes(end, include_suppressed, f);
        }
        Expr::Call(call) => {
            for arg in &call.args {
                walk_expr_nodes(&arg.expr, include_suppressed, f);
            }
        }
        Expr::Binary(_, left, right, _) => {
            walk_expr_nodes(left, include_suppressed, f);
            walk_expr_nodes(right, include_suppressed, f);
        }
        Expr::MethodCall { receiver, args, .. } => {
            walk_expr_nodes(receiver, include_suppressed, f);
            for arg in args {
                walk_expr_nodes(&arg.expr, include_suppressed, f);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, value) in fields {
                walk_expr_nodes(value, include_suppressed, f);
            }
        }
        Expr::TypedLit { body, .. } => {
            body.for_each_expr(|value| walk_expr_nodes(value, include_suppressed, f));
        }
        Expr::EnumLit { args, .. } => {
            for arg in args {
                let value = match arg {
                    EnumLitArg::Positional(value)
                    | EnumLitArg::Named { expr: value, .. } => value,
                };
                walk_expr_nodes(value, include_suppressed, f);
            }
        }
        Expr::PatternTest {
            subject, pattern, ..
        } => {
            walk_expr_nodes(subject, include_suppressed, f);
            walk_pattern_expr_nodes(pattern, include_suppressed, f);
        }
        Expr::OrFallback {
            value, fallback, ..
        } => {
            walk_expr_nodes(value, include_suppressed, f);
            match fallback {
                OrFallback::Value(value) | OrFallback::Return(Some(value), _) => {
                    walk_expr_nodes(value, include_suppressed, f);
                }
                OrFallback::Panic { args, .. } => {
                    for arg in args {
                        walk_expr_nodes(&arg.expr, include_suppressed, f);
                    }
                }
                OrFallback::Return(None, _)
                | OrFallback::Break(_)
                | OrFallback::Continue(_)
                | OrFallback::BreakLabel(..)
                | OrFallback::ContinueLabel(..) => {}
            }
        }
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            walk_expr_nodes(cond, include_suppressed, f);
            for stmt in then_body {
                walk_stmt_expr_nodes(stmt, include_suppressed, f);
            }
            walk_expr_nodes(then_value, include_suppressed, f);
            for stmt in else_body {
                walk_stmt_expr_nodes(stmt, include_suppressed, f);
            }
            walk_expr_nodes(else_value, include_suppressed, f);
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, value) in fields {
                walk_expr_nodes(value, include_suppressed, f);
            }
        }
        Expr::Lambda(lambda) => {
            if include_suppressed {
                match &lambda.body {
                    LambdaBody::Expr(body) => walk_expr_nodes(body, include_suppressed, f),
                    LambdaBody::Block(body) => {
                        for stmt in body {
                            walk_stmt_expr_nodes(stmt, include_suppressed, f);
                        }
                    }
                }
            }
        }
        Expr::CallValue { callee, args, .. } => {
            walk_expr_nodes(callee, include_suppressed, f);
            for arg in args {
                walk_expr_nodes(&arg.expr, include_suppressed, f);
            }
        }
        Expr::PtrFromAddr { addr, .. } => walk_expr_nodes(addr, include_suppressed, f),
        Expr::FanOut { callee, items, .. } => {
            walk_expr_nodes(callee, include_suppressed, f);
            for item in items {
                walk_expr_nodes(item, include_suppressed, f);
            }
        }
    }
}

fn walk_pattern_expr_nodes(
    pattern: &Pattern,
    include_suppressed: bool,
    f: &mut impl FnMut(&Expr),
) {
    match pattern {
        Pattern::Or(patterns, _) => {
            for pattern in patterns {
                walk_pattern_expr_nodes(pattern, include_suppressed, f);
            }
        }
        Pattern::Struct { fields, .. } => {
            for field in fields {
                if let StructPatField::Value { value, .. } = field {
                    walk_expr_nodes(value, include_suppressed, f);
                }
            }
        }
        Pattern::Variant { .. }
        | Pattern::Present { .. }
        | Pattern::Absent(_)
        | Pattern::Ok { .. }
        | Pattern::Err { .. }
        | Pattern::Range { .. }
        | Pattern::StrMatch { .. }
        | Pattern::BinMatch { .. } => {}
    }
}

fn walk_stmt_expr_nodes(s: &Stmt, include_suppressed: bool, f: &mut impl FnMut(&Expr)) {
    macro_rules! walk {
        ($expr:expr) => {
            walk_expr_nodes($expr, include_suppressed, f)
        };
    }
    match s {
        Stmt::Expr(expr)
        | Stmt::Val(crate::AST::Binding { init: expr, .. })
        | Stmt::Yield(expr, _) => {
            walk!(expr);
        }
        Stmt::Assign { target, value, .. } => {
            match target {
                LValue::Local { .. } => {}
                LValue::Index { base, index, .. } => {
                    walk!(base);
                    walk!(index);
                }
                LValue::Field { base, .. } => walk!(base),
            }
            walk!(value);
        }
        Stmt::Return(Some(expr), _) | Stmt::BreakValue(expr, _) => walk!(expr),
        Stmt::BreakLabelValue(_, _, expr, _) => walk!(expr),
        Stmt::Return(None, _)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::BreakLabel(..)
        | Stmt::ContinueLabel(..) => {}
        Stmt::If(if_stmt) => {
            walk!(&if_stmt.cond);
            for stmt in &if_stmt.then_body {
                walk_stmt_expr_nodes(stmt, include_suppressed, f);
            }
            match &if_stmt.else_branch {
                Some(crate::AST::ElseBranch::ElseIf(inner)) => {
                    walk_if_stmt_expr_nodes(inner, include_suppressed, f);
                }
                Some(crate::AST::ElseBranch::Else(body)) => {
                    for stmt in body {
                        walk_stmt_expr_nodes(stmt, include_suppressed, f);
                    }
                }
                None => {}
            }
        }
        Stmt::While { cond, body, .. } => {
            walk!(cond);
            walk_stmt_body_nodes(body, include_suppressed, f);
        }
        Stmt::For { kind, body, .. } => {
            match kind {
                crate::AST::ForKind::Range {
                    start, end, step, ..
                } => {
                    walk!(start);
                    walk!(end);
                    if let Some(step) = step {
                        walk!(step);
                    }
                }
                crate::AST::ForKind::In { collection, step } => {
                    walk!(collection);
                    if let Some(step) = step {
                        walk!(step);
                    }
                }
            }
            walk_stmt_body_nodes(body, include_suppressed, f);
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
            walk!(subject);
            for arm in arms {
                walk!(&arm.cond);
                walk_stmt_body_nodes(&arm.body, include_suppressed, f);
            }
            if let Some(body) = else_body {
                walk_stmt_body_nodes(body, include_suppressed, f);
            }
        }
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            walk!(&init.init);
            walk!(cond);
            if let Some(step) = step {
                walk_stmt_expr_nodes(step, include_suppressed, f);
            }
            walk_stmt_body_nodes(body, include_suppressed, f);
        }
        Stmt::Unsafe {
            audit_expr, body, ..
        } => {
            if let Some(audit) = audit_expr {
                walk!(audit);
            }
            walk_stmt_body_nodes(body, include_suppressed, f);
        }
        Stmt::Impure {
            reason_expr, body, ..
        } => {
            if include_suppressed {
                if let Some(reason) = reason_expr {
                    walk!(reason);
                }
                walk_stmt_body_nodes(body, include_suppressed, f);
            }
        }
        Stmt::AssumeDet {
            reason_expr, body, ..
        } => {
            if include_suppressed {
                walk!(reason_expr);
                walk_stmt_body_nodes(body, include_suppressed, f);
            }
        }
        Stmt::ComptimeIf {
            cond,
            then_body,
            else_body,
            ..
        } => {
            walk!(cond);
            walk_stmt_body_nodes(then_body, include_suppressed, f);
            if let Some(body) = else_body {
                walk_stmt_body_nodes(body, include_suppressed, f);
            }
        }
        Stmt::ContextBlock { fields, body, .. } => {
            for (_, value, _) in fields {
                walk!(value);
            }
            walk_stmt_body_nodes(body, include_suppressed, f);
        }
        Stmt::ScopeMember { args, body, .. } => {
            for arg in args {
                walk!(arg);
            }
            walk_stmt_body_nodes(body, include_suppressed, f);
        }
        Stmt::Loop { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Off { body, .. }
        | Stmt::DebugOnly { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::Transact { body, .. } => {
            walk_stmt_body_nodes(body, include_suppressed, f);
        }
    }
}

fn walk_if_stmt_expr_nodes(
    if_stmt: &crate::AST::IfStmt,
    include_suppressed: bool,
    f: &mut impl FnMut(&Expr),
) {
    walk_expr_nodes(&if_stmt.cond, include_suppressed, f);
    walk_stmt_body_nodes(&if_stmt.then_body, include_suppressed, f);
    match &if_stmt.else_branch {
        Some(crate::AST::ElseBranch::ElseIf(inner)) => {
            walk_if_stmt_expr_nodes(inner, include_suppressed, f);
        }
        Some(crate::AST::ElseBranch::Else(body)) => {
            walk_stmt_body_nodes(body, include_suppressed, f);
        }
        None => {}
    }
}

fn walk_stmt_body_nodes(
    body: &[Stmt],
    include_suppressed: bool,
    f: &mut impl FnMut(&Expr),
) {
    for stmt in body {
        walk_stmt_expr_nodes(stmt, include_suppressed, f);
    }
}
