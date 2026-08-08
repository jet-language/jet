use super::*;
use crate::Diagnostics::Diagnostic;
use crate::AST::Func;
use std::collections::HashMap;

/// Return E3401 if `fn_name` (which declares `=[]=>`) calls an impure function.
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
            "`{}` is impure, but `{}` declares `=[]=>`",
            call_name,
            pure_fn_name
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
        format!(
            "`{}` calls the impure function `{}`",
            pure_fn_name, call_name
        ),
        why,
        format!(
            "give `{}` an explicit `=[]=>` bound, or remove the call from `{}`",
            call_name,
            pure_fn_name
        ),
        Some(span),
    )
}

/// E3402: ambient I/O or network access attempted during a sandboxed package build.
pub fn e3402(call_name: &str, span: Option<crate::Diagnostics::Span>) -> Diagnostic {
    Diagnostic::error(
        "E3402",
        format!(
            "`{}` is not allowed during a sandboxed package build",
            call_name
        ),
        "package builds run with ambient I/O and network access disabled (D-PURE2)".to_string(),
        "compute this value at compile time or pass it in as a parameter".to_string(),
        span,
    )
}

/// E3403: non-deterministic construct in pure evaluation context.
pub fn e3403(what: &str, span: Option<crate::Diagnostics::Span>) -> Diagnostic {
    Diagnostic::error(
        "E3403",
        format!(
            "`{}` is non-deterministic and cannot appear in a pure evaluation",
            what
        ),
        "pure evaluation must produce the same result on every machine (D-PURE2)".to_string(),
        "remove this call, or remove the enclosing function's explicit `=[]=>` bound"
            .to_string(),
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

/// D-STDIN1=A: std module calls that are impure (read from environment/stdin).
/// Unlike `is_nondeterministic_core` (E3403), these fire E3401 in pure context.
pub(crate) fn is_impure_core(module: &str, method: &str) -> bool {
    matches!(
        (module, method),
        (
            "core.io",
            "stdin" | "input" | "confirm" | "choose" | "input_secret" | "read_all_input"
        )
    )
}

/// E3403: std calls that are non-deterministic — their result depends on wall
/// clock or RNG, so they cannot appear in a pure evaluation. Keyed on the
/// resolved `(module, method)` pair (std calls are method calls on a module
/// alias, not bare names). Time formatting is pure (Int + pattern → String)
/// and intentionally excluded.
pub(crate) fn is_nondeterministic_core(module: &str, method: &str) -> bool {
    matches!(
        (module, method),
        ("core.time", "now" | "sleep" | "start")
            | (
                "core.random",
                "int" | "float" | "float_range" | "bool" | "normal" | "exponential" | "pick"
                    | "weighted_pick" | "sample" | "shuffle" | "seed" | "split" | "bytes"
            )
            | ("core.crypto.random", "bytes")
    )
}

/// D-META-EFFECT1 c3: the call-graph walk itself lives in
/// `jet-comptime/Comptime/Purity.rs` (`jet-sema` depends on `jet-comptime`,
/// not the other way around, so that is the one home both stages share).
/// This is the run-time `=[]=>` route: check `f`'s own body for a direct
/// impure-builtin or extern call. Empty `funcs` map passed to the shared
/// walker means it never recurses into a callee's body — a callee that
/// itself turns out impure is instead caught by the shared effect row
/// (`Sema::Effects::check_inferred_purity`), which doesn't need to
/// re-walk bodies because it already has every function's solved effect row.
pub fn check_pure_fn(f: &Func, funcs: &HashMap<String, FuncSig>) -> Vec<Diagnostic> {
    if !f.is_pure {
        return Vec::new();
    }
    let no_bodies: HashMap<String, &Func> = HashMap::new();
    let is_leaf_impure = |name: &str| {
        is_impure_builtin(name) || funcs.get(name).is_some_and(|sig| sig.is_extern)
    };
    match crate::Comptime::walk_purity_stmts(
        &f.body,
        &no_bodies,
        &is_leaf_impure,
        &|name, path, span| e3401(&f.name, name, path, span),
        crate::Comptime::PurityStage::RunTime,
    ) {
        Ok(()) => Vec::new(),
        Err(d) => vec![d],
    }
}

pub(crate) fn check_pure_expr(
    e: &crate::AST::Expr,
    pure_fn: &str,
    funcs: &HashMap<String, FuncSig>,
) -> Option<Diagnostic> {
    let no_bodies: HashMap<String, &Func> = HashMap::new();
    let is_leaf_impure = |name: &str| {
        is_impure_builtin(name) || funcs.get(name).is_some_and(|sig| sig.is_extern)
    };
    crate::Comptime::walk_purity_expr(
        e,
        &no_bodies,
        &is_leaf_impure,
        &|name, path, span| e3401(pure_fn, name, path, span),
        crate::Comptime::PurityStage::RunTime,
    )
    .err()
}

/// From-root transitive purity check for `jet eval --pure`.
///
/// Walks the call graph starting at `entry_fn` (typically `"run"`), following
/// calls into `ast_funcs` bodies. Fires E3401 on the first impure call with
/// the full transitive chain.
///
/// This is the correct checker for the eval context: intermediate functions
/// carry no `pure` annotation (so `check_pure_fn` would not flag them), but
/// the whole program must be pure because it runs under `--pure`.
pub fn check_pure_program_root(
    entry_fn: &str,
    funcs_sig: &HashMap<String, FuncSig>,
    ast_funcs: &HashMap<String, &Func>,
) -> Vec<Diagnostic> {
    let Some(f) = ast_funcs.get(entry_fn) else {
        return Vec::new();
    };
    let is_leaf_impure = |name: &str| {
        is_impure_builtin(name) || funcs_sig.get(name).is_some_and(|sig| sig.is_extern)
    };
    // Seed path/visited with entry_fn itself so a direct violation in the
    // root reads "`entry_fn` calls `x`" (matching the original wording),
    // and so entry_fn re-calling itself is cycle-guarded from the start.
    let mut visited = std::collections::HashSet::new();
    visited.insert(entry_fn.to_string());
    let mut path = vec![entry_fn.to_string()];
    match crate::Comptime::walk_purity_stmts_from(
        &f.body,
        ast_funcs,
        &is_leaf_impure,
        &|name, path, span| e3401(entry_fn, name, path, span),
        &mut visited,
        &mut path,
        crate::Comptime::PurityStage::RunTime,
    ) {
        Ok(()) => Vec::new(),
        Err(d) => vec![d],
    }
}
