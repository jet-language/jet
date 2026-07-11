//! Purity check: walk the call graph reachable from a comptime `init` and
//! reject the first impure call (IO, FFI) with the path that reached it
//! (E0951). `embed_file`, `embed_bytes`, `find`, `panic`, and `require` are allowed.

use std::collections::{HashMap, HashSet};

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{EnumLitArg, Expr, Func, Stmt, StrPart};

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
    walk_stmt_exprs(s, &mut |e| {
        if result.is_ok() {
            result = check_purity_expr(e, funcs, externs, visited, path);
        }
    });
    result
}

/// Visit every direct `Call` name in an expression tree (shallow over
/// nested functions — recursion is driven by the purity walker).
pub fn walk_calls(e: &Expr, f: &mut impl FnMut(&str, Span)) {
    match e {
        Expr::Call(c) => {
            f(&c.name, c.name_span);
            for a in &c.args {
                walk_calls(&a.expr, f);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            walk_calls(receiver, f);
            for a in args {
                walk_calls(&a.expr, f);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            walk_calls(callee, f);
            for a in args {
                walk_calls(&a.expr, f);
            }
        }
        Expr::Binary(_, l, r, _) => {
            walk_calls(l, f);
            walk_calls(r, f);
        }
        Expr::Unary(_, x, _)
        | Expr::Tainted(x, _) // D-TAINT1: tag erased; recurse into the value.
        | Expr::Present(x, _)
        | Expr::Try(x, _, _)
        | Expr::Deref(x, _)
        | Expr::RawOf(x, _) => walk_calls(x, f),
        Expr::Index { base, index, .. } => {
            walk_calls(base, f);
            walk_calls(index, f);
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            walk_calls(base, f);
            walk_calls(start, f);
            walk_calls(end, f);
        }
        Expr::ListLit(xs, _) => xs.iter().for_each(|x| walk_calls(x, f)),
        Expr::MapLit(es, _) => es.iter().for_each(|(k, v)| {
            walk_calls(k, f);
            walk_calls(v, f);
        }),
        Expr::Str(parts, _) => parts.iter().for_each(|p| {
            if let StrPart::Interp(e, _) = p {
                walk_calls(e, f)
            }
        }),
        Expr::Ok(x, _) | Expr::Err(x, _) => walk_calls(x, f),
        Expr::Field(x, _, _) => walk_calls(x, f),
        Expr::OrFallback { value, .. } => walk_calls(value, f),
        Expr::EnumLit { args, .. } => args.iter().for_each(|a| match a {
            EnumLitArg::Positional(e) | EnumLitArg::Named { expr: e, .. } => walk_calls(e, f),
        }),
        Expr::Paren(inner, _) => walk_calls(inner, f),
        Expr::IncDec { operand, .. } => walk_calls(operand, f),
        _ => {}
    }
}

fn walk_stmt_exprs(s: &Stmt, f: &mut impl FnMut(&Expr)) {
    match s {
        Stmt::Expr(e) | Stmt::Val(crate::AST::Binding { init: e, .. }) | Stmt::Yield(e, _) => f(e),
        Stmt::Assign { value, .. } => f(value),
        Stmt::Return(Some(e), _) => f(e),
        Stmt::Return(None, _) => {}
        Stmt::If(ifs) => walk_if_exprs(ifs, f),
        Stmt::While { cond, body, .. } => {
            f(cond);
            body.iter().for_each(|s| walk_stmt_exprs(s, f));
        }
        Stmt::For { kind, body, .. } => {
            match kind {
                crate::AST::ForKind::Range { start, end, step } => {
                    f(start);
                    f(end);
                    if let Some(step) = step {
                        f(step);
                    }
                }
                crate::AST::ForKind::In { collection } => f(collection),
            }
            body.iter().for_each(|s| walk_stmt_exprs(s, f));
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => {
            f(subject);
            for a in arms {
                f(&a.cond);
                a.body.iter().for_each(|s| walk_stmt_exprs(s, f));
            }
            if let Some(b) = else_body {
                b.iter().for_each(|s| walk_stmt_exprs(s, f));
            }
        }
        Stmt::CountedLoop {
            cond, step, body, ..
        } => {
            f(cond);
            walk_stmt_exprs(step, f);
            body.iter().for_each(|s| walk_stmt_exprs(s, f));
        }
        Stmt::Loop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::SuppressMustUse { body, .. }
        | Stmt::Off { body, .. }
        | Stmt::DebugOnly { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::Transact { body, .. } => body.iter().for_each(|s| walk_stmt_exprs(s, f)),
        // D-CTEFFECT1: `#Impure` body is allowed to be impure — skip purity
        // walk so its Tier-2 calls don't fire E0951.
        Stmt::Impure { .. } => {}
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => {}
        // D-WHEN1: walk both arms for purity analysis (conservative).
        Stmt::ComptimeIf {
            cond,
            then_body,
            else_body,
            ..
        } => {
            f(cond);
            then_body.iter().for_each(|s| walk_stmt_exprs(s, f));
            if let Some(eb) = else_body {
                eb.iter().for_each(|s| walk_stmt_exprs(s, f));
            }
        }
        // D-CTMARKER1 (ratified 2026-06-25, piece 2): walk comptime block body.
        Stmt::ComptimeBlock { body, .. } => {
            body.iter().for_each(|s| walk_stmt_exprs(s, f));
        }
        // D-CTX1: walk field values and body for purity analysis.
        Stmt::ContextBlock { fields, body, .. } => {
            for (_, e, _) in fields {
                f(e);
            }
            body.iter().for_each(|s| walk_stmt_exprs(s, f));
        }
        // D-TERM1 (ratified 2026-06-22): walk live block body for purity analysis.
        Stmt::Live { body, .. } => {
            body.iter().for_each(|s| walk_stmt_exprs(s, f));
        }
        // D-DOTSCOPE1: walk a `#Test` scope-member region body for purity analysis.
        Stmt::ScopeMember { body, .. } => {
            body.iter().for_each(|s| walk_stmt_exprs(s, f));
        }
        // D-DET1: `assume_deterministic { … }` suspends the determinism check for
        // its body — the comptime purity walk does not descend into it (footgun).
        Stmt::AssumeDet { .. } => {}
        // D-OSTARGET2=B: sema desugars `comptime if build.os == { … }` into a
        // `comptime if` chain before this pass runs, so this arm is unreachable
        // in practice; walk both the subject and arms conservatively anyway.
        Stmt::ComptimeSwitch {
            subject,
            arms,
            else_body,
            ..
        } => {
            f(subject);
            for arm in arms {
                arm.body.iter().for_each(|s| walk_stmt_exprs(s, f));
            }
            if let Some(eb) = else_body {
                eb.iter().for_each(|s| walk_stmt_exprs(s, f));
            }
        }
    }
}

fn walk_if_exprs(ifs: &crate::AST::IfStmt, f: &mut impl FnMut(&Expr)) {
    f(&ifs.cond);
    ifs.then_body.iter().for_each(|s| walk_stmt_exprs(s, f));
    match &ifs.else_branch {
        Some(crate::AST::ElseBranch::ElseIf(inner)) => walk_if_exprs(inner, f),
        Some(crate::AST::ElseBranch::Else(body)) => body.iter().for_each(|s| walk_stmt_exprs(s, f)),
        None => {}
    }
}
