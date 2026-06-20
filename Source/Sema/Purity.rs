use super::*;
use crate::AST::Func;
use crate::Diagnostics::Diagnostic;
use std::collections::HashMap;

/// Return E3401 if `fn_name` (which is marked `pure`) calls an impure function.
/// `funcs` is the full function-signature map; `call_name` is the callee;
/// `path` is the chain of calls that led here (for the trace message).
pub fn e3401(
    pure_fn_name: &str,
    call_name: &str,
    path: &[String],
    span: crate::Diagnostics::Span,
) -> Diagnostic {
    let why = if path.is_empty() {
        format!(
            "`{}` is impure, but `{}` is declared `pure fn`",
            call_name, pure_fn_name
        )
    } else {
        format!(
            "{} calls `{}`, which is impure — the whole call chain must be pure inside `{}`",
            path.join(" → "),
            call_name,
            pure_fn_name
        )
    };
    Diagnostic::error(
        "E3401",
        format!("`{}` calls the impure function `{}`", pure_fn_name, call_name),
        why,
        format!(
            "mark `{}` as `pure fn`, or remove the call from `{}`",
            call_name, pure_fn_name
        ),
        Some(span),
    )
}

/// E3402: ambient I/O or network access attempted during a sandboxed package build.
pub fn e3402(call_name: &str, span: Option<crate::Diagnostics::Span>) -> Diagnostic {
    Diagnostic::error(
        "E3402",
        format!("`{}` is not allowed during a sandboxed package build", call_name),
        "package builds run with ambient I/O and network access disabled (D-PURE2)".to_string(),
        "compute this value at compile time or pass it in as a parameter".to_string(),
        span,
    )
}

/// E3403: non-deterministic construct in pure evaluation context.
pub fn e3403(what: &str, span: Option<crate::Diagnostics::Span>) -> Diagnostic {
    Diagnostic::error(
        "E3403",
        format!("`{}` is non-deterministic and cannot appear in a pure evaluation", what),
        "pure evaluation must produce the same result on every machine (D-PURE2)".to_string(),
        "remove this call, or do not mark the enclosing function `pure`".to_string(),
        span,
    )
}

/// The builtins that are always impure (write to stdout/stderr or read input).
pub(crate) fn is_impure_builtin(name: &str) -> bool {
    matches!(
        name,
        "print" | "eprint" | "input" | "read_all_input"
    )
}

/// E3403: std calls that are non-deterministic — their result depends on wall
/// clock or RNG, so they cannot appear in a pure evaluation. Keyed on the
/// resolved `(module, method)` pair (std calls are method calls on a module
/// alias, not bare names). `jet.time.format` is pure (Int + pattern → String)
/// and intentionally excluded.
pub(crate) fn is_nondeterministic_std(module: &str, method: &str) -> bool {
    matches!(
        (module, method),
        ("core.time", "now" | "sleep" | "start")
            | ("jet.time", "now")
            | ("core.random", "int" | "float" | "pick" | "shuffle" | "seed")
    )
}

/// Walk the call graph rooted at `f`'s body; collect E3401 for the first
/// impure call found (with the call-trace path so the user sees exactly
/// what broke purity). Stops at the first violation per function to avoid
/// a flood of errors.
pub fn check_pure_fn(
    f: &Func,
    funcs: &HashMap<String, FuncSig>,
) -> Vec<Diagnostic> {
    if !f.is_pure {
        return Vec::new();
    }
    let mut diags = Vec::new();
    for stmt in &f.body {
        if let Some(d) = check_pure_stmt(stmt, &f.name, funcs) {
            diags.push(d);
        }
    }
    diags
}

pub(crate) fn check_pure_stmt(
    s: &crate::AST::Stmt,
    pure_fn: &str,
    funcs: &HashMap<String, FuncSig>,
) -> Option<Diagnostic> {
    use crate::AST::Stmt;
    match s {
        Stmt::Val(b) => check_pure_expr(&b.init, pure_fn, funcs),
        Stmt::Assign { value, .. } => check_pure_expr(value, pure_fn, funcs),
        Stmt::Return(Some(e), _) => check_pure_expr(e, pure_fn, funcs),
        Stmt::Return(None, _) => None,
        Stmt::Expr(e) => check_pure_expr(e, pure_fn, funcs),
        Stmt::If(if_stmt) => check_pure_if(if_stmt, pure_fn, funcs),
        Stmt::While { cond, body, .. } => {
            if let Some(d) = check_pure_expr(cond, pure_fn, funcs) {
                return Some(d);
            }
            for st in body {
                if let Some(d) = check_pure_stmt(st, pure_fn, funcs) {
                    return Some(d);
                }
            }
            None
        }
        Stmt::For { kind, body, .. } => {
            use crate::AST::ForKind;
            match kind {
                ForKind::Range { start, end, step } => {
                    if let Some(d) = check_pure_expr(start, pure_fn, funcs) {
                        return Some(d);
                    }
                    if let Some(d) = check_pure_expr(end, pure_fn, funcs) {
                        return Some(d);
                    }
                    if let Some(s) = step {
                        if let Some(d) = check_pure_expr(s, pure_fn, funcs) {
                            return Some(d);
                        }
                    }
                }
                ForKind::In { collection } => {
                    if let Some(d) = check_pure_expr(collection, pure_fn, funcs) {
                        return Some(d);
                    }
                }
            }
            for st in body {
                if let Some(d) = check_pure_stmt(st, pure_fn, funcs) {
                    return Some(d);
                }
            }
            None
        }
        Stmt::Loop { body, .. } => {
            for st in body {
                if let Some(d) = check_pure_stmt(st, pure_fn, funcs) {
                    return Some(d);
                }
            }
            None
        }
        Stmt::Switch { subject, arms, else_body, .. } => {
            if let Some(d) = check_pure_expr(subject, pure_fn, funcs) {
                return Some(d);
            }
            for arm in arms {
                if let Some(d) = check_pure_expr(&arm.cond, pure_fn, funcs) {
                    return Some(d);
                }
                for st in &arm.body {
                    if let Some(d) = check_pure_stmt(st, pure_fn, funcs) {
                        return Some(d);
                    }
                }
            }
            if let Some(eb) = else_body {
                for st in eb {
                    if let Some(d) = check_pure_stmt(st, pure_fn, funcs) {
                        return Some(d);
                    }
                }
            }
            None
        }
        Stmt::Unsafe { body, .. } => {
            for st in body {
                if let Some(d) = check_pure_stmt(st, pure_fn, funcs) {
                    return Some(d);
                }
            }
            None
        }
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => None,
        // D-WHEN1: check both arms of a comptime if for purity (conservative).
        Stmt::ComptimeIf { cond, then_body, else_body, .. } => {
            if let Some(d) = check_pure_expr(cond, pure_fn, funcs) {
                return Some(d);
            }
            for st in then_body {
                if let Some(d) = check_pure_stmt(st, pure_fn, funcs) {
                    return Some(d);
                }
            }
            if let Some(eb) = else_body {
                for st in eb {
                    if let Some(d) = check_pure_stmt(st, pure_fn, funcs) {
                        return Some(d);
                    }
                }
            }
            None
        }
    }
}

pub(crate) fn check_pure_if(
    if_stmt: &crate::AST::IfStmt,
    pure_fn: &str,
    funcs: &HashMap<String, FuncSig>,
) -> Option<Diagnostic> {
    if let Some(d) = check_pure_expr(&if_stmt.cond, pure_fn, funcs) {
        return Some(d);
    }
    for st in &if_stmt.then_body {
        if let Some(d) = check_pure_stmt(st, pure_fn, funcs) {
            return Some(d);
        }
    }
    match &if_stmt.else_branch {
        Some(crate::AST::ElseBranch::Else(stmts)) => {
            for st in stmts {
                if let Some(d) = check_pure_stmt(st, pure_fn, funcs) {
                    return Some(d);
                }
            }
            None
        }
        Some(crate::AST::ElseBranch::ElseIf(nested)) => {
            check_pure_if(nested, pure_fn, funcs)
        }
        None => None,
    }
}

pub(crate) fn check_pure_expr(
    e: &crate::AST::Expr,
    pure_fn: &str,
    funcs: &HashMap<String, FuncSig>,
) -> Option<Diagnostic> {
    use crate::AST::Expr;
    match e {
        Expr::Call(c) => {
            let name = &c.name;
            if is_impure_builtin(name) {
                return Some(e3401(pure_fn, name, &[], c.name_span));
            }
            if let Some(sig) = funcs.get(name.as_str()) {
                if sig.is_extern || !sig.is_pure {
                    return Some(e3401(pure_fn, name, &[], c.name_span));
                }
            }
            // Recurse into args.
            for arg in &c.args {
                if let Some(d) = check_pure_expr(&arg.expr, pure_fn, funcs) {
                    return Some(d);
                }
            }
            None
        }
        Expr::MethodCall { receiver, args, .. } => {
            if let Some(d) = check_pure_expr(receiver, pure_fn, funcs) {
                return Some(d);
            }
            for arg in args {
                if let Some(d) = check_pure_expr(&arg.expr, pure_fn, funcs) {
                    return Some(d);
                }
            }
            None
        }
        Expr::Binary(_, left, right, _) => {
            check_pure_expr(left, pure_fn, funcs)
                .or_else(|| check_pure_expr(right, pure_fn, funcs))
        }
        Expr::Unary(_, operand, _) => check_pure_expr(operand, pure_fn, funcs),
        Expr::Index { base, index, .. } => {
            check_pure_expr(base, pure_fn, funcs)
                .or_else(|| check_pure_expr(index, pure_fn, funcs))
        }
        Expr::Slice { base, start, end, .. } => {
            check_pure_expr(base, pure_fn, funcs)
                .or_else(|| check_pure_expr(start, pure_fn, funcs))
                .or_else(|| check_pure_expr(end, pure_fn, funcs))
        }
        Expr::Field(inner, _, _) | Expr::Deref(inner, _) => {
            check_pure_expr(inner, pure_fn, funcs)
        }
        Expr::OptField { base, .. } => check_pure_expr(base, pure_fn, funcs),
        Expr::If { cond, then_body, then_value, else_body, else_value, .. } => {
            check_pure_expr(cond, pure_fn, funcs)
                .or_else(|| {
                    for st in then_body {
                        if let Some(d) = check_pure_stmt(st, pure_fn, funcs) {
                            return Some(d);
                        }
                    }
                    None
                })
                .or_else(|| check_pure_expr(then_value, pure_fn, funcs))
                .or_else(|| {
                    for st in else_body {
                        if let Some(d) = check_pure_stmt(st, pure_fn, funcs) {
                            return Some(d);
                        }
                    }
                    None
                })
                .or_else(|| check_pure_expr(else_value, pure_fn, funcs))
        }
        Expr::ListLit(items, _) => {
            for item in items {
                if let Some(d) = check_pure_expr(item, pure_fn, funcs) {
                    return Some(d);
                }
            }
            None
        }
        Expr::MapLit(pairs, _) => {
            for (k, v) in pairs {
                if let Some(d) = check_pure_expr(k, pure_fn, funcs) {
                    return Some(d);
                }
                if let Some(d) = check_pure_expr(v, pure_fn, funcs) {
                    return Some(d);
                }
            }
            None
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, v) in fields {
                if let Some(d) = check_pure_expr(v, pure_fn, funcs) {
                    return Some(d);
                }
            }
            None
        }
        Expr::EnumLit { args, .. } => {
            for arg in args {
                let expr = match arg {
                    crate::AST::EnumLitArg::Positional(e) => e,
                    crate::AST::EnumLitArg::Named { expr, .. } => expr,
                };
                if let Some(d) = check_pure_expr(expr, pure_fn, funcs) {
                    return Some(d);
                }
            }
            None
        }
        Expr::Present(inner, _) | Expr::Ok(inner, _) | Expr::Err(inner, _) => {
            check_pure_expr(inner, pure_fn, funcs)
        }
        Expr::Try(inner, _, _) => check_pure_expr(inner, pure_fn, funcs),
        Expr::OrFallback { value, fallback, .. } => {
            check_pure_expr(value, pure_fn, funcs).or_else(|| {
                use crate::AST::OrFallback as OF;
                match fallback {
                    OF::Value(fe) => check_pure_expr(fe, pure_fn, funcs),
                    OF::Return(..) | OF::Panic { .. } => None,
                }
            })
        }
        Expr::CallValue { callee, args, .. } => {
            if let Some(d) = check_pure_expr(callee, pure_fn, funcs) {
                return Some(d);
            }
            for arg in args {
                if let Some(d) = check_pure_expr(&arg.expr, pure_fn, funcs) {
                    return Some(d);
                }
            }
            None
        }
        Expr::FanOut { callee, items, .. } => {
            if let Some(d) = check_pure_expr(callee, pure_fn, funcs) {
                return Some(d);
            }
            for item in items {
                if let Some(d) = check_pure_expr(item, pure_fn, funcs) {
                    return Some(d);
                }
            }
            None
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, v) in fields {
                if let Some(d) = check_pure_expr(v, pure_fn, funcs) {
                    return Some(d);
                }
            }
            None
        }
        // PatternTest, PtrFromAddr, Lambda, Ident, literals, Absent, Todo are leaf/irrelevant.
        _ => None,
    }
}
