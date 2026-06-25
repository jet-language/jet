//! M9.5 — Comptime v1 (CTFE). A tree-walking interpreter over the typed
//! AST that evaluates a pure, deterministic Jet subset at compile time and
//! bakes the answer into the binary. See tools/Tower/docs/plans/m095-comptime.md.
//!
//! One law (S26): comptime computes *values* only — it never creates,
//! parameterizes, or selects a type, and never affects dispatch.
//!
//! Diagnostics: E0951 impurity (with call path) · E0952 fuel exhausted ·
//! E0953 comptime panic (user message verbatim, overflow, divide-by-zero) ·
//! E0955 embed_file errors · E0956 construct not yet supported at comptime.
//!
//! Semantics are bit-for-bit identical to the compiled runtime (the
//! differential battery in tests/comptime_diff.rs is the enforcement):
//! i64 `Int`, IEEE f64 `Float` (S21 display via `{:?}`), char-counted
//! `String` (S41), and `BTreeMap` ordering (S38).

mod Builtins;
mod Diagnostics;
mod Interpreter;
mod Methods;
mod Purity;
mod Value;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::AST::Func;
use crate::Diagnostics::Diagnostic;

pub use Interpreter::{DebugHook, DevSink, REPL_FUEL_BUDGET};
pub use Purity::walk_calls;
pub use Value::CtValue;

use Interpreter::{Interp, DEV_FUEL_BUDGET, FUEL_BUDGET};
use Purity::check_purity;

// An empty core_imports map for paths that don't have `use` declarations.
static EMPTY_IMPORTS: std::sync::OnceLock<HashMap<String, String>> =
    std::sync::OnceLock::new();
fn empty_imports() -> &'static HashMap<String, String> {
    EMPTY_IMPORTS.get_or_init(HashMap::new)
}

// --- public entry ---------------------------------------------------------

/// Type-check happens elsewhere (every function body goes through sema);
/// this checks purity then evaluates `init` to a constant value.
pub fn evaluate(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, &Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
) -> Result<CtValue, Diagnostic> {
    evaluate_with_imports(init, funcs, extern_names, base_dir, globals, &HashMap::new())
}

/// Like `evaluate` but with module alias map for D-CTCORE1 whitelisted Core calls.
pub fn evaluate_with_imports(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, &Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
) -> Result<CtValue, Diagnostic> {
    check_purity(init, funcs, extern_names)?;
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel: FUEL_BUDGET,
        sink: None,
        core_imports,
        debugger: None,
        depth: 0,
        cur_func: "main".to_string(),
    };
    let mut scope = globals.clone();
    interp.eval(init, &mut scope)
}

/// Whole-program dev interpretation (E2-M4 `jet dev`). Runs `main`'s body
/// with a buffered stdout/stderr sink, reusing the exact same evaluator the
/// M9.5 comptime path uses — there is no second interpreter. Output bytes are
/// produced via `CtValue::jet_show()` + `\n`, identical to the compiled
/// program (the differential battery in `tests/dev.rs` enforces this, I2).
///
/// The caller (src/interp.rs) is responsible for the E2201 boundary scan
/// (FFI/tasks/`@unsafe`); this function simply runs and may itself return
/// E0956 (`unsupported`) when it reaches a construct the evaluator can't run,
/// or E2202 when the fuel budget is exhausted.
pub fn run_main(
    main: &Func,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
) -> Result<(), Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel: DEV_FUEL_BUDGET,
        sink: Some(sink),
        core_imports: empty_imports(),
        debugger: None,
        depth: 0,
        cur_func: "main".to_string(),
    };
    let mut scope = HashMap::new();
    interp.exec_block(&main.body, &mut scope)?;
    Ok(())
}

/// D-DBG3: whole-program interpretation under the source-level debugger.
/// Identical to [`run_main`] (same evaluator, same buffered sink, same I2
/// bytes) except a [`DebugHook`] is attached: the driver is notified before
/// every statement and may pause to run its `(jet)` prompt. The driver shows
/// only Jet lines/locals — it never sees generated Rust. Returns the same
/// E2202 (fuel) / E0956 (unsupported) stops, plus any abort the driver raises
/// (e.g. the user typed `quit`, surfaced as E2204).
pub fn run_main_debug(
    main: &Func,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
    debugger: &mut dyn DebugHook,
) -> Result<(), Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel: DEV_FUEL_BUDGET,
        sink: Some(sink),
        core_imports: empty_imports(),
        debugger: Some(debugger),
        depth: 0,
        cur_func: "main".to_string(),
    };
    let mut scope = HashMap::new();
    interp.exec_block(&main.body, &mut scope)?;
    Ok(())
}

/// `jet eval --pure` variant: runs `main()` and returns its return value as a
/// `CtValue` instead of buffering stdout. Used when the caller wants to render
/// the value (pretty or JSON) rather than capture print output. Any print
/// calls are still captured but discarded; the return value is authoritative.
pub fn run_main_value(
    main: &Func,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
) -> Result<CtValue, Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel: DEV_FUEL_BUDGET,
        sink: Some(sink),
        core_imports: empty_imports(),
        debugger: None,
        depth: 0,
        cur_func: "main".to_string(),
    };
    let mut scope = HashMap::new();
    match interp.exec_block(&main.body, &mut scope)? {
        Interpreter::Flow::Return(v) => Ok(v),
        _ => Ok(CtValue::Unit),
    }
}

/// REPL variant of `run_main`: uses a caller-supplied fuel cap so the REPL
/// can enforce D-REPL-FUEL without patching DEV_FUEL_BUDGET. Returns the
/// same E2202 (dev fuel stop) or E0956 (unsupported) errors; the REPL
/// intercepts E2202 and upgrades it to E1801 with REPL-specific wording.
pub fn run_main_with_fuel(
    main: &Func,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
    fuel: u64,
) -> Result<(), Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel,
        sink: Some(sink),
        core_imports: empty_imports(),
        debugger: None,
        depth: 0,
        cur_func: "main".to_string(),
    };
    let mut scope = HashMap::new();
    interp.exec_block(&main.body, &mut scope)?;
    Ok(())
}

/// REPL per-input step (E2-M18). Executes `stmts` inside a running REPL
/// session. Differs from `run_main_with_fuel` in two ways:
///
/// 1. The scope is passed in *and mutated* — accumulated bindings survive
///    across inputs (D-REPL7: one accumulating module).
/// 2. If the last statement is a bare `Stmt::Expr` (not `Stmt::Val`), the
///    evaluated value is returned so the caller can display
///    `x: T = v` (D-REPL16=B).
///
/// The `suppress` flag implements `;` at end of input — the caller strips the
/// trailing `;` to detect a bare expression and passes `suppress = false`; a
/// statement ending in `;` passes `suppress = true`.
pub fn run_repl_step(
    stmts: &[crate::AST::Stmt],
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
    scope: &mut HashMap<String, CtValue>,
    fuel: u64,
    suppress: bool,
) -> Result<Option<CtValue>, Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel,
        sink: Some(sink),
        core_imports: empty_imports(),
        debugger: None,
        depth: 0,
        cur_func: "main".to_string(),
    };
    // Split: run all statements except the last; then handle the last specially
    // if it is a bare expression (for display) and not suppressed.
    let (last, head) = match stmts.split_last() {
        Some(pair) => pair,
        None => return Ok(None),
    };
    interp.exec_block(head, scope)?;
    // Determine if the last statement should produce an echo value.
    // Case 1: `Stmt::Val` named `__repl_echo__` — the sentinel that `classify`
    //   injects for bare-expression inputs (e.g. `1 + 2` → `val __repl_echo__ = 1 + 2`).
    //   Evaluate but don't add to the persistent scope.
    // Case 2: bare `Stmt::Expr` — retained for forward-compat.
    let echo_bare = !suppress && matches!(last, crate::AST::Stmt::Expr(_));
    match last {
        crate::AST::Stmt::Val(b) if !suppress && b.name == "__repl_echo__" => {
            let v = interp.eval(&b.init, scope)?;
            Ok(Some(v))
        }
        crate::AST::Stmt::Val(b) => {
            let v = interp.eval(&b.init, scope)?;
            if let Some(pat) = &b.pattern {
                interp.bind_pattern(pat, v, scope)?;
            } else {
                scope.insert(b.name.clone(), v);
            }
            Ok(None)
        }
        crate::AST::Stmt::Expr(e) if echo_bare => {
            let v = interp.eval(e, scope)?;
            Ok(Some(v))
        }
        other => {
            interp.exec_stmt(other, scope)?;
            Ok(None)
        }
    }
}

/// Owned-function variant used while sema is mutating function bodies for
/// local `comptime` bindings. The cloned function map is a snapshot of the
/// already-parsed program; the interpreter still sees ordinary Jet AST.
pub fn evaluate_owned(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
) -> Result<CtValue, Diagnostic> {
    evaluate_owned_with_imports(init, funcs, extern_names, base_dir, globals, &HashMap::new())
}

/// Like `evaluate_owned` but with module alias map for D-CTCORE1 whitelisted Core calls.
pub fn evaluate_owned_with_imports(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
) -> Result<CtValue, Diagnostic> {
    let refs: HashMap<String, &Func> = funcs.iter().map(|(n, f)| (n.clone(), f)).collect();
    evaluate_with_imports(init, &refs, extern_names, base_dir, globals, core_imports)
}
