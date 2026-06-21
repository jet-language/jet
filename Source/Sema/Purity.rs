use super::*;
use crate::AST::Func;
use crate::Diagnostics::Diagnostic;
use std::collections::{HashMap, HashSet};

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
///
/// Derives from `Syntax::IMPURE_BUILTINS` (c44 consolidation). Add new impure
/// builtins to Syntax.rs; the comptime purity checker uses the same list.
pub(crate) fn is_impure_builtin(name: &str) -> bool {
    crate::Syntax::IMPURE_BUILTINS.contains(&name)
}

/// E3403: std calls that are non-deterministic — their result depends on wall
/// clock or RNG, so they cannot appear in a pure evaluation. Keyed on the
/// resolved `(module, method)` pair (std calls are method calls on a module
/// alias, not bare names). `jet.time.format` is pure (Int + pattern → String)
/// and intentionally excluded.
pub(crate) fn is_nondeterministic_core(module: &str, method: &str) -> bool {
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
    check_pure_expr_with_path(e, pure_fn, funcs, &[], &mut HashSet::new())
}

/// Internal version that carries the transitive call-chain path and visited
/// set for cycle detection. Used both by `check_pure_expr` (path=[]) and by
/// `check_pure_program_root` (path seeded from the root).
fn check_pure_expr_with_path(
    e: &crate::AST::Expr,
    pure_fn: &str,
    funcs: &HashMap<String, FuncSig>,
    path: &[String],
    visited: &mut HashSet<String>,
) -> Option<Diagnostic> {
    use crate::AST::Expr;
    // Helper macro to recurse with the same path + visited.
    macro_rules! rec {
        ($expr:expr) => {
            check_pure_expr_with_path($expr, pure_fn, funcs, path, visited)
        };
    }
    macro_rules! rec_stmt {
        ($stmt:expr) => {
            check_pure_stmt_with_path($stmt, pure_fn, funcs, path, visited)
        };
    }
    match e {
        Expr::Call(c) => {
            let name = &c.name;
            if is_impure_builtin(name) {
                return Some(e3401(pure_fn, name, path, c.name_span));
            }
            if let Some(sig) = funcs.get(name.as_str()) {
                if sig.is_extern || !sig.is_pure {
                    return Some(e3401(pure_fn, name, path, c.name_span));
                }
            }
            // Recurse into args.
            for arg in &c.args {
                if let Some(d) = rec!(&arg.expr) {
                    return Some(d);
                }
            }
            None
        }
        Expr::MethodCall { receiver, args, .. } => {
            if let Some(d) = rec!(receiver) {
                return Some(d);
            }
            for arg in args {
                if let Some(d) = rec!(&arg.expr) {
                    return Some(d);
                }
            }
            None
        }
        Expr::Binary(_, left, right, _) => {
            rec!(left).or_else(|| rec!(right))
        }
        Expr::Unary(_, operand, _) => rec!(operand),
        Expr::Index { base, index, .. } => {
            rec!(base).or_else(|| rec!(index))
        }
        Expr::Slice { base, start, end, .. } => {
            rec!(base).or_else(|| rec!(start)).or_else(|| rec!(end))
        }
        Expr::Field(inner, _, _) | Expr::Deref(inner, _) => rec!(inner),
        Expr::OptField { base, .. } => rec!(base),
        Expr::If { cond, then_body, then_value, else_body, else_value, .. } => {
            rec!(cond)
                .or_else(|| {
                    for st in then_body {
                        if let Some(d) = rec_stmt!(st) {
                            return Some(d);
                        }
                    }
                    None
                })
                .or_else(|| rec!(then_value))
                .or_else(|| {
                    for st in else_body {
                        if let Some(d) = rec_stmt!(st) {
                            return Some(d);
                        }
                    }
                    None
                })
                .or_else(|| rec!(else_value))
        }
        Expr::ListLit(items, _) => {
            for item in items {
                if let Some(d) = rec!(item) {
                    return Some(d);
                }
            }
            None
        }
        Expr::MapLit(pairs, _) => {
            for (k, v) in pairs {
                if let Some(d) = rec!(k) {
                    return Some(d);
                }
                if let Some(d) = rec!(v) {
                    return Some(d);
                }
            }
            None
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, v) in fields {
                if let Some(d) = rec!(v) {
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
                if let Some(d) = rec!(expr) {
                    return Some(d);
                }
            }
            None
        }
        Expr::Present(inner, _) | Expr::Ok(inner, _) | Expr::Err(inner, _) => rec!(inner),
        Expr::Try(inner, _, _) => rec!(inner),
        Expr::OrFallback { value, fallback, .. } => {
            rec!(value).or_else(|| {
                use crate::AST::OrFallback as OF;
                match fallback {
                    OF::Value(fe) => rec!(fe),
                    OF::Return(..) | OF::Panic { .. } => None,
                }
            })
        }
        Expr::CallValue { callee, args, .. } => {
            if let Some(d) = rec!(callee) {
                return Some(d);
            }
            for arg in args {
                if let Some(d) = rec!(&arg.expr) {
                    return Some(d);
                }
            }
            None
        }
        Expr::FanOut { callee, items, .. } => {
            if let Some(d) = rec!(callee) {
                return Some(d);
            }
            for item in items {
                if let Some(d) = rec!(item) {
                    return Some(d);
                }
            }
            None
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, v) in fields {
                if let Some(d) = rec!(v) {
                    return Some(d);
                }
            }
            None
        }
        // PatternTest, PtrFromAddr, Lambda, Ident, literals, Absent, Todo are leaf/irrelevant.
        _ => None,
    }
}

/// Path-aware statement walker used by `check_pure_expr_with_path`.
fn check_pure_stmt_with_path(
    s: &crate::AST::Stmt,
    pure_fn: &str,
    funcs: &HashMap<String, FuncSig>,
    path: &[String],
    visited: &mut HashSet<String>,
) -> Option<Diagnostic> {
    use crate::AST::Stmt;
    macro_rules! rec {
        ($expr:expr) => {
            check_pure_expr_with_path($expr, pure_fn, funcs, path, visited)
        };
    }
    macro_rules! rec_s {
        ($stmt:expr) => {
            check_pure_stmt_with_path($stmt, pure_fn, funcs, path, visited)
        };
    }
    match s {
        Stmt::Val(b) => rec!(&b.init),
        Stmt::Assign { value, .. } => rec!(value),
        Stmt::Return(Some(e), _) => rec!(e),
        Stmt::Return(None, _) => None,
        Stmt::Expr(e) => rec!(e),
        Stmt::If(if_stmt) => check_pure_if_with_path(if_stmt, pure_fn, funcs, path, visited),
        Stmt::While { cond, body, .. } => {
            if let Some(d) = rec!(cond) {
                return Some(d);
            }
            for st in body {
                if let Some(d) = rec_s!(st) {
                    return Some(d);
                }
            }
            None
        }
        Stmt::For { kind, body, .. } => {
            use crate::AST::ForKind;
            match kind {
                ForKind::Range { start, end, step } => {
                    if let Some(d) = rec!(start) {
                        return Some(d);
                    }
                    if let Some(d) = rec!(end) {
                        return Some(d);
                    }
                    if let Some(s) = step {
                        if let Some(d) = rec!(s) {
                            return Some(d);
                        }
                    }
                }
                ForKind::In { collection } => {
                    if let Some(d) = rec!(collection) {
                        return Some(d);
                    }
                }
            }
            for st in body {
                if let Some(d) = rec_s!(st) {
                    return Some(d);
                }
            }
            None
        }
        Stmt::Loop { body, .. } => {
            for st in body {
                if let Some(d) = rec_s!(st) {
                    return Some(d);
                }
            }
            None
        }
        Stmt::Switch { subject, arms, else_body, .. } => {
            if let Some(d) = rec!(subject) {
                return Some(d);
            }
            for arm in arms {
                if let Some(d) = rec!(&arm.cond) {
                    return Some(d);
                }
                for st in &arm.body {
                    if let Some(d) = rec_s!(st) {
                        return Some(d);
                    }
                }
            }
            if let Some(eb) = else_body {
                for st in eb {
                    if let Some(d) = rec_s!(st) {
                        return Some(d);
                    }
                }
            }
            None
        }
        Stmt::Unsafe { body, .. } => {
            for st in body {
                if let Some(d) = rec_s!(st) {
                    return Some(d);
                }
            }
            None
        }
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => None,
        Stmt::ComptimeIf { cond, then_body, else_body, .. } => {
            if let Some(d) = rec!(cond) {
                return Some(d);
            }
            for st in then_body {
                if let Some(d) = rec_s!(st) {
                    return Some(d);
                }
            }
            if let Some(eb) = else_body {
                for st in eb {
                    if let Some(d) = rec_s!(st) {
                        return Some(d);
                    }
                }
            }
            None
        }
    }
}

fn check_pure_if_with_path(
    if_stmt: &crate::AST::IfStmt,
    pure_fn: &str,
    funcs: &HashMap<String, FuncSig>,
    path: &[String],
    visited: &mut HashSet<String>,
) -> Option<Diagnostic> {
    if let Some(d) = check_pure_expr_with_path(&if_stmt.cond, pure_fn, funcs, path, visited) {
        return Some(d);
    }
    for st in &if_stmt.then_body {
        if let Some(d) = check_pure_stmt_with_path(st, pure_fn, funcs, path, visited) {
            return Some(d);
        }
    }
    match &if_stmt.else_branch {
        Some(crate::AST::ElseBranch::Else(stmts)) => {
            for st in stmts {
                if let Some(d) = check_pure_stmt_with_path(st, pure_fn, funcs, path, visited) {
                    return Some(d);
                }
            }
            None
        }
        Some(crate::AST::ElseBranch::ElseIf(nested)) => {
            check_pure_if_with_path(nested, pure_fn, funcs, path, visited)
        }
        None => None,
    }
}

/// From-root transitive purity check for `jet eval --pure`.
///
/// Walks the call graph starting at `entry_fn` (typically `"main"`), following
/// calls into `ast_funcs` bodies and accumulating the call chain in `path`.
/// Fires E3401 on the first impure call with the full transitive chain.
///
/// This is the correct checker for the eval context: intermediate functions
/// carry no `pure` annotation (so `check_pure_fn` would not flag them), but
/// the whole program must be pure because it runs under `--pure`.
///
/// Cycle detection via `visited` prevents infinite recursion on mutually
/// recursive functions; the first chain found is shortest (DFS order).
pub fn check_pure_program_root(
    entry_fn: &str,
    funcs_sig: &HashMap<String, FuncSig>,
    ast_funcs: &HashMap<String, &Func>,
) -> Vec<Diagnostic> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut path: Vec<String> = vec![entry_fn.to_string()];
    let mut diags = Vec::new();
    if let Some(f) = ast_funcs.get(entry_fn) {
        visited.insert(entry_fn.to_string());
        check_pure_fn_body_transitive(f, entry_fn, funcs_sig, ast_funcs, &mut path, &mut visited, &mut diags);
    }
    diags
}

/// Recursively walk `func`'s body for purity violations, building the
/// transitive path. Stops at the first violation.
fn check_pure_fn_body_transitive(
    func: &Func,
    root_fn: &str,
    funcs_sig: &HashMap<String, FuncSig>,
    ast_funcs: &HashMap<String, &Func>,
    path: &mut Vec<String>,
    visited: &mut HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    // Walk every statement in this function's body.
    for stmt in &func.body {
        if !diags.is_empty() {
            return;
        }
        walk_stmt_for_calls(stmt, root_fn, funcs_sig, ast_funcs, path, visited, diags);
    }
}

fn walk_expr_for_calls(
    e: &crate::AST::Expr,
    root_fn: &str,
    funcs_sig: &HashMap<String, FuncSig>,
    ast_funcs: &HashMap<String, &Func>,
    path: &mut Vec<String>,
    visited: &mut HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    if !diags.is_empty() {
        return;
    }
    use crate::AST::Expr;
    match e {
        Expr::Call(c) => {
            let name = &c.name;
            // Impure builtins are always impure.
            if is_impure_builtin(name) {
                diags.push(e3401(root_fn, name, path, c.name_span));
                return;
            }
            // Extern functions have no inspectable body: flag immediately.
            if let Some(sig) = funcs_sig.get(name.as_str()) {
                if sig.is_extern {
                    diags.push(e3401(root_fn, name, path, c.name_span));
                    return;
                }
            }
            // User-defined function: descend into its body to find the
            // transitive impurity (regardless of `is_pure` annotation —
            // the eval context requires the whole graph to be pure).
            if let Some(callee_ast) = ast_funcs.get(name.as_str()) {
                if visited.insert(name.to_string()) {
                    path.push(name.to_string());
                    check_pure_fn_body_transitive(callee_ast, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                    path.pop();
                    if !diags.is_empty() {
                        return;
                    }
                }
            }
            // Recurse into args.
            for arg in &c.args {
                walk_expr_for_calls(&arg.expr, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                if !diags.is_empty() {
                    return;
                }
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            walk_expr_for_calls(receiver, root_fn, funcs_sig, ast_funcs, path, visited, diags);
            if diags.is_empty() {
                for arg in args {
                    walk_expr_for_calls(&arg.expr, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                    if !diags.is_empty() {
                        return;
                    }
                }
            }
        }
        Expr::Binary(_, left, right, _) => {
            walk_expr_for_calls(left, root_fn, funcs_sig, ast_funcs, path, visited, diags);
            if diags.is_empty() {
                walk_expr_for_calls(right, root_fn, funcs_sig, ast_funcs, path, visited, diags);
            }
        }
        Expr::Unary(_, operand, _) => {
            walk_expr_for_calls(operand, root_fn, funcs_sig, ast_funcs, path, visited, diags);
        }
        Expr::Index { base, index, .. } => {
            walk_expr_for_calls(base, root_fn, funcs_sig, ast_funcs, path, visited, diags);
            if diags.is_empty() {
                walk_expr_for_calls(index, root_fn, funcs_sig, ast_funcs, path, visited, diags);
            }
        }
        Expr::Slice { base, start, end, .. } => {
            walk_expr_for_calls(base, root_fn, funcs_sig, ast_funcs, path, visited, diags);
            if diags.is_empty() { walk_expr_for_calls(start, root_fn, funcs_sig, ast_funcs, path, visited, diags); }
            if diags.is_empty() { walk_expr_for_calls(end, root_fn, funcs_sig, ast_funcs, path, visited, diags); }
        }
        Expr::Field(inner, _, _) | Expr::Deref(inner, _) => {
            walk_expr_for_calls(inner, root_fn, funcs_sig, ast_funcs, path, visited, diags);
        }
        Expr::OptField { base, .. } => {
            walk_expr_for_calls(base, root_fn, funcs_sig, ast_funcs, path, visited, diags);
        }
        Expr::If { cond, then_body, then_value, else_body, else_value, .. } => {
            walk_expr_for_calls(cond, root_fn, funcs_sig, ast_funcs, path, visited, diags);
            if diags.is_empty() {
                for st in then_body {
                    walk_stmt_for_calls(st, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                    if !diags.is_empty() { return; }
                }
            }
            if diags.is_empty() { walk_expr_for_calls(then_value, root_fn, funcs_sig, ast_funcs, path, visited, diags); }
            if diags.is_empty() {
                for st in else_body {
                    walk_stmt_for_calls(st, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                    if !diags.is_empty() { return; }
                }
            }
            if diags.is_empty() { walk_expr_for_calls(else_value, root_fn, funcs_sig, ast_funcs, path, visited, diags); }
        }
        Expr::ListLit(items, _) => {
            for item in items {
                walk_expr_for_calls(item, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                if !diags.is_empty() { return; }
            }
        }
        Expr::MapLit(pairs, _) => {
            for (k, v) in pairs {
                walk_expr_for_calls(k, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                if diags.is_empty() { walk_expr_for_calls(v, root_fn, funcs_sig, ast_funcs, path, visited, diags); }
                if !diags.is_empty() { return; }
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, v) in fields {
                walk_expr_for_calls(v, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                if !diags.is_empty() { return; }
            }
        }
        Expr::EnumLit { args, .. } => {
            for arg in args {
                let expr = match arg {
                    crate::AST::EnumLitArg::Positional(e) => e,
                    crate::AST::EnumLitArg::Named { expr, .. } => expr,
                };
                walk_expr_for_calls(expr, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                if !diags.is_empty() { return; }
            }
        }
        Expr::Present(inner, _) | Expr::Ok(inner, _) | Expr::Err(inner, _) => {
            walk_expr_for_calls(inner, root_fn, funcs_sig, ast_funcs, path, visited, diags);
        }
        Expr::Try(inner, _, _) => {
            walk_expr_for_calls(inner, root_fn, funcs_sig, ast_funcs, path, visited, diags);
        }
        Expr::OrFallback { value, fallback, .. } => {
            walk_expr_for_calls(value, root_fn, funcs_sig, ast_funcs, path, visited, diags);
            if diags.is_empty() {
                use crate::AST::OrFallback as OF;
                if let OF::Value(fe) = fallback {
                    walk_expr_for_calls(fe, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                }
            }
        }
        Expr::CallValue { callee, args, .. } => {
            walk_expr_for_calls(callee, root_fn, funcs_sig, ast_funcs, path, visited, diags);
            if diags.is_empty() {
                for arg in args {
                    walk_expr_for_calls(&arg.expr, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                    if !diags.is_empty() { return; }
                }
            }
        }
        Expr::FanOut { callee, items, .. } => {
            walk_expr_for_calls(callee, root_fn, funcs_sig, ast_funcs, path, visited, diags);
            if diags.is_empty() {
                for item in items {
                    walk_expr_for_calls(item, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                    if !diags.is_empty() { return; }
                }
            }
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, v) in fields {
                walk_expr_for_calls(v, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                if !diags.is_empty() { return; }
            }
        }
        // Leaf expressions (literals, ident, etc.) have no calls.
        _ => {}
    }
}

fn walk_stmt_for_calls(
    s: &crate::AST::Stmt,
    root_fn: &str,
    funcs_sig: &HashMap<String, FuncSig>,
    ast_funcs: &HashMap<String, &Func>,
    path: &mut Vec<String>,
    visited: &mut HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    if !diags.is_empty() {
        return;
    }
    use crate::AST::Stmt;
    match s {
        Stmt::Val(b) => walk_expr_for_calls(&b.init, root_fn, funcs_sig, ast_funcs, path, visited, diags),
        Stmt::Assign { value, .. } => walk_expr_for_calls(value, root_fn, funcs_sig, ast_funcs, path, visited, diags),
        Stmt::Return(Some(e), _) => walk_expr_for_calls(e, root_fn, funcs_sig, ast_funcs, path, visited, diags),
        Stmt::Return(None, _) => {}
        Stmt::Expr(e) => walk_expr_for_calls(e, root_fn, funcs_sig, ast_funcs, path, visited, diags),
        Stmt::If(if_stmt) => walk_if_for_calls(if_stmt, root_fn, funcs_sig, ast_funcs, path, visited, diags),
        Stmt::While { cond, body, .. } => {
            walk_expr_for_calls(cond, root_fn, funcs_sig, ast_funcs, path, visited, diags);
            if diags.is_empty() {
                for st in body {
                    walk_stmt_for_calls(st, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                    if !diags.is_empty() { return; }
                }
            }
        }
        Stmt::For { kind, body, .. } => {
            use crate::AST::ForKind;
            match kind {
                ForKind::Range { start, end, step } => {
                    walk_expr_for_calls(start, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                    if diags.is_empty() { walk_expr_for_calls(end, root_fn, funcs_sig, ast_funcs, path, visited, diags); }
                    if diags.is_empty() {
                        if let Some(step_e) = step {
                            walk_expr_for_calls(step_e, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                        }
                    }
                }
                ForKind::In { collection } => {
                    walk_expr_for_calls(collection, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                }
            }
            if diags.is_empty() {
                for st in body {
                    walk_stmt_for_calls(st, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                    if !diags.is_empty() { return; }
                }
            }
        }
        Stmt::Loop { body, .. } | Stmt::Unsafe { body, .. } => {
            for st in body {
                walk_stmt_for_calls(st, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                if !diags.is_empty() { return; }
            }
        }
        Stmt::Switch { subject, arms, else_body, .. } => {
            walk_expr_for_calls(subject, root_fn, funcs_sig, ast_funcs, path, visited, diags);
            if diags.is_empty() {
                for arm in arms {
                    walk_expr_for_calls(&arm.cond, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                    if diags.is_empty() {
                        for st in &arm.body {
                            walk_stmt_for_calls(st, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                            if !diags.is_empty() { return; }
                        }
                    }
                    if !diags.is_empty() { return; }
                }
            }
            if diags.is_empty() {
                if let Some(eb) = else_body {
                    for st in eb {
                        walk_stmt_for_calls(st, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                        if !diags.is_empty() { return; }
                    }
                }
            }
        }
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => {}
        Stmt::ComptimeIf { cond, then_body, else_body, .. } => {
            walk_expr_for_calls(cond, root_fn, funcs_sig, ast_funcs, path, visited, diags);
            if diags.is_empty() {
                for st in then_body {
                    walk_stmt_for_calls(st, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                    if !diags.is_empty() { return; }
                }
            }
            if diags.is_empty() {
                if let Some(eb) = else_body {
                    for st in eb {
                        walk_stmt_for_calls(st, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                        if !diags.is_empty() { return; }
                    }
                }
            }
        }
    }
}

fn walk_if_for_calls(
    if_stmt: &crate::AST::IfStmt,
    root_fn: &str,
    funcs_sig: &HashMap<String, FuncSig>,
    ast_funcs: &HashMap<String, &Func>,
    path: &mut Vec<String>,
    visited: &mut HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    walk_expr_for_calls(&if_stmt.cond, root_fn, funcs_sig, ast_funcs, path, visited, diags);
    if diags.is_empty() {
        for st in &if_stmt.then_body {
            walk_stmt_for_calls(st, root_fn, funcs_sig, ast_funcs, path, visited, diags);
            if !diags.is_empty() { return; }
        }
    }
    if diags.is_empty() {
        match &if_stmt.else_branch {
            Some(crate::AST::ElseBranch::Else(stmts)) => {
                for st in stmts {
                    walk_stmt_for_calls(st, root_fn, funcs_sig, ast_funcs, path, visited, diags);
                    if !diags.is_empty() { return; }
                }
            }
            Some(crate::AST::ElseBranch::ElseIf(nested)) => {
                walk_if_for_calls(nested, root_fn, funcs_sig, ast_funcs, path, visited, diags);
            }
            None => {}
        }
    }
}
