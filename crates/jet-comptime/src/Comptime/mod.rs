//! M9.5 — Comptime v1 (CTFE). A tree-walking interpreter over the typed
//! AST that evaluates a pure, deterministic Jet subset at compile time and
//! bakes the answer into the binary. See the comptime section of docs/spec/spec.md.
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

pub mod Build;
mod Builtins;
mod Diagnostics;
mod Interpreter;
mod JsonInterp;
mod Methods;
mod Purity;
mod Reflect;
mod RegexLite;
mod Value;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::Diagnostics::Diagnostic;
use crate::AST::{Expr, Func, StructDef};

pub use Interpreter::{DebugHook, DevSink, REPL_FUEL_BUDGET};
pub use Purity::walk_calls;
pub use Reflect::build_struct_type_info;
pub use Value::CtValue;

use Interpreter::{Interp, DEV_FUEL_BUDGET, FUEL_BUDGET};
use Purity::check_purity;

// An empty core_imports map for paths that don't have `use` declarations.
static EMPTY_IMPORTS: std::sync::OnceLock<HashMap<String, String>> = std::sync::OnceLock::new();
fn empty_imports() -> &'static HashMap<String, String> {
    EMPTY_IMPORTS.get_or_init(HashMap::new)
}

// c139: empty registries for evaluation contexts that don't thread the
// whole-program info a `jet dev` run collects (comptime bindings, `derive`
// bodies, the REPL) — user-method dispatch, computed fields, and
// distinct-type constructors reached from those contexts still surface their
// existing E0956 rather than silently no-op-ing.
static EMPTY_GLOBALS: std::sync::OnceLock<HashMap<String, CtValue>> = std::sync::OnceLock::new();
fn empty_globals() -> &'static HashMap<String, CtValue> {
    EMPTY_GLOBALS.get_or_init(HashMap::new)
}
static EMPTY_METHODS: std::sync::OnceLock<HashMap<(String, String), &'static Func>> =
    std::sync::OnceLock::new();
fn empty_methods() -> &'static HashMap<(String, String), &'static Func> {
    EMPTY_METHODS.get_or_init(HashMap::new)
}
static EMPTY_STRUCTS: std::sync::OnceLock<HashMap<String, &'static StructDef>> =
    std::sync::OnceLock::new();
fn empty_structs() -> &'static HashMap<String, &'static StructDef> {
    EMPTY_STRUCTS.get_or_init(HashMap::new)
}
static EMPTY_COMPUTED: std::sync::OnceLock<HashMap<(String, String), &'static Expr>> =
    std::sync::OnceLock::new();
fn empty_computed() -> &'static HashMap<(String, String), &'static Expr> {
    EMPTY_COMPUTED.get_or_init(HashMap::new)
}
static EMPTY_DISTINCT: std::sync::OnceLock<HashMap<String, Option<(i64, i64)>>> =
    std::sync::OnceLock::new();
fn empty_distinct() -> &'static HashMap<String, Option<(i64, i64)>> {
    EMPTY_DISTINCT.get_or_init(HashMap::new)
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
    evaluate_with_imports(
        init,
        funcs,
        extern_names,
        base_dir,
        globals,
        &HashMap::new(),
    )
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
    evaluate_with_imports_opts(
        init,
        funcs,
        extern_names,
        base_dir,
        globals,
        core_imports,
        false,
        0,
    )
}

/// Like `evaluate_with_imports` but with `allow_impure` and `initial_impure_depth`
/// for D-CTEFFECT1. When called from inside a sema `#Impure` block, pass
/// `initial_impure_depth: 1` (and `allow_impure: true`) so the interpreter
/// starts with the gate already open for Tier-2 calls.
pub fn evaluate_with_imports_opts(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, &Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
    initial_impure_depth: usize,
) -> Result<CtValue, Diagnostic> {
    // Only run the purity check when there is no active #Impure gate (i.e.
    // the expression is not nested inside a `#Impure` block at sema time).
    // When initial_impure_depth > 0, the gate is active — skip check_purity
    // so that Tier-2 calls fire E3411 ("gate present, flag absent") instead
    // of E0951 ("impure call at comptime"), giving a better fix message.
    if initial_impure_depth == 0 {
        check_purity(init, funcs, extern_names)?;
    }
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel: FUEL_BUDGET,
        sink: None,
        core_imports,
        debugger: None,
        depth: 0,
        cur_func: "main".to_string(),
        impure_depth: initial_impure_depth,
        allow_impure,
        repl_mode: false,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        globals,
        methods: empty_methods(),
        structs: empty_structs(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
    };
    let mut scope = globals.clone();
    interp.eval(init, &mut scope)
}

/// Like [`evaluate_with_imports_opts`] but also returns the Tier-1 embed
/// inputs accumulated during evaluation (D-CTEFFECT1). The caller (sema
/// Checker) drains these into `CompileOutput.comptime_inputs`.
pub fn evaluate_with_imports_opts_collecting(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, &Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
    initial_impure_depth: usize,
) -> Result<(CtValue, Vec<crate::AST::ComptimeInput>), Diagnostic> {
    if initial_impure_depth == 0 {
        check_purity(init, funcs, extern_names)?;
    }
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel: FUEL_BUDGET,
        sink: None,
        core_imports,
        debugger: None,
        depth: 0,
        cur_func: "main".to_string(),
        impure_depth: initial_impure_depth,
        allow_impure,
        repl_mode: false,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        globals,
        methods: empty_methods(),
        structs: empty_structs(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
    };
    let mut scope = globals.clone();
    let val = interp.eval(init, &mut scope)?;
    Ok((val, interp.embed_inputs))
}

/// Whole-program dev interpretation (E2-M4 `jet dev`). Runs `main`'s body
/// with a buffered stdout/stderr sink, reusing the exact same evaluator the
/// M9.5 comptime path uses — there is no second interpreter. Output bytes are
/// produced via `CtValue::jet_show()` + `\n`, identical to the compiled
/// program (the differential battery in `tests/dev.rs` enforces this, I2).
///
/// The caller (src/interp.rs) is responsible for the E2201 boundary scan
/// (FFI/tasks/`#Unsafe`); this function simply runs and may itself return
/// E0956 (`unsupported`) when it reaches a construct the evaluator can't run,
/// or E2202 when the fuel budget is exhausted.
/// c139: everything the dev interpreter needs beyond the flat `funcs` map to
/// run whole programs at parity with the real build — pre-evaluated
/// top-level `const`/`comptime` bindings, user-method dispatch (`impl`/
/// in-struct methods and D-MOD2 code-module namespaced calls), D-FIELDPOL1
/// computed fields, and D-RANGETYPE1/D-DIST1 distinct-type constructors.
/// Built once per `jet dev` run by `Source/Interpreter.rs::collect_program_info`.
pub struct ProgramInfo<'a> {
    pub globals: HashMap<String, CtValue>,
    pub methods: HashMap<(String, String), &'a Func>,
    pub structs: HashMap<String, &'a StructDef>,
    pub computed_fields: HashMap<(String, String), &'a Expr>,
    pub distinct_ranges: HashMap<String, Option<(i64, i64)>>,
    pub core_imports: HashMap<String, String>,
}

impl<'a> ProgramInfo<'a> {
    pub fn empty() -> Self {
        ProgramInfo {
            globals: HashMap::new(),
            methods: HashMap::new(),
            structs: HashMap::new(),
            computed_fields: HashMap::new(),
            distinct_ranges: HashMap::new(),
            core_imports: HashMap::new(),
        }
    }
}

pub fn run_main(
    main: &Func,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
    program: &ProgramInfo,
) -> Result<(), Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel: DEV_FUEL_BUDGET,
        sink: Some(sink),
        core_imports: &program.core_imports,
        debugger: None,
        depth: 0,
        cur_func: "main".to_string(),
        impure_depth: 0,
        allow_impure: false,
        repl_mode: false,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        globals: &program.globals,
        methods: &program.methods,
        structs: &program.structs,
        computed_fields: &program.computed_fields,
        distinct_ranges: &program.distinct_ranges,
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
        impure_depth: 0,
        allow_impure: false,
        repl_mode: false,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        globals: empty_globals(),
        methods: empty_methods(),
        structs: empty_structs(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
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
        impure_depth: 0,
        allow_impure: false,
        repl_mode: false,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        globals: empty_globals(),
        methods: empty_methods(),
        structs: empty_structs(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
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
        impure_depth: 0,
        allow_impure: false,
        repl_mode: false,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        globals: empty_globals(),
        methods: empty_methods(),
        structs: empty_structs(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
    };
    let mut scope = HashMap::new();
    interp.exec_block(&main.body, &mut scope)?;
    Ok(())
}

/// REPL `:run` transcript path: like `run_main_with_fuel` but with the REPL
/// sandbox (Tier-2 I/O, accumulated `core_imports`) so materialized sessions
/// replay the same semantics as interactive inputs.
pub fn run_repl_main_with_fuel(
    main: &Func,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
    fuel: u64,
    core_imports: &HashMap<String, String>,
) -> Result<(), Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel,
        sink: Some(sink),
        core_imports,
        debugger: None,
        depth: 0,
        cur_func: "main".to_string(),
        impure_depth: 1,
        allow_impure: true,
        repl_mode: true,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        globals: empty_globals(),
        methods: empty_methods(),
        structs: empty_structs(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
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
///
/// D-CTCORE1: `core_imports` maps alias → Core module path (e.g. `"math"` →
/// `"core.math"`) from the session's accumulated `use` declarations, so
/// whitelisted pure Core calls (e.g. `math.sqrt(16.0)`) execute inline instead
/// of raising E0956. Pass `&HashMap::new()` when no imports are active.
pub fn run_repl_step(
    stmts: &[crate::AST::Stmt],
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
    scope: &mut HashMap<String, CtValue>,
    fuel: u64,
    suppress: bool,
    core_imports: &HashMap<String, String>,
) -> Result<Option<CtValue>, Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel,
        sink: Some(sink),
        core_imports,
        debugger: None,
        depth: 0,
        cur_func: "main".to_string(),
        // c133: REPL sandbox — Tier-2 I/O without user `#Impure` gates.
        impure_depth: 1,
        allow_impure: true,
        repl_mode: true,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        globals: empty_globals(),
        methods: empty_methods(),
        structs: empty_structs(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
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
    //   injects for bare-expression inputs (e.g. `1 + 2` → `__repl_echo__ :: 1 + 2`).
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

/// D-CTMARKER1 (ratified 2026-06-25, piece 2): run a `comptime { … }` block at
/// build time. Purity-checked (E0951/E0958) then tree-walked with fuel cap (E0952).
/// Pure path only (Stage A); effect tiers wire in c157 (D-CTEFFECT1).
pub fn run_block_with_imports(
    stmts: &[crate::AST::Stmt],
    funcs: &HashMap<String, Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
) -> Result<HashMap<String, CtValue>, Diagnostic> {
    let refs: HashMap<String, &Func> = funcs.iter().map(|(n, f)| (n.clone(), f)).collect();
    Purity::check_purity_stmts(stmts, &refs, extern_names)?;
    let mut interp = Interp {
        funcs: &refs,
        base_dir,
        fuel: FUEL_BUDGET,
        sink: None,
        core_imports,
        debugger: None,
        depth: 0,
        cur_func: "comptime block".to_string(),
        // D-CTEFFECT1: a `comptime { }` block is build-time code — hermetic by
        // default, so Tier-2 `#Impure` effects inside it require the normal gate
        // (E3411 until --allow-impure is plumbed through to block evaluation).
        allow_impure: false,
        impure_depth: 0,
        repl_mode: false,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        globals,
        methods: empty_methods(),
        structs: empty_structs(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
    };
    let mut scope = globals.clone();
    interp.exec_block(stmts, &mut scope)?;
    Ok(scope)
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
    evaluate_owned_with_imports(
        init,
        funcs,
        extern_names,
        base_dir,
        globals,
        &HashMap::new(),
    )
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
    evaluate_owned_with_imports_opts(
        init,
        funcs,
        extern_names,
        base_dir,
        globals,
        core_imports,
        false,
        0,
    )
}

/// Like `evaluate_owned_with_imports` but with D-CTEFFECT1 `allow_impure` flag
/// and `initial_impure_depth`. Pass `initial_impure_depth: 1` when evaluating a
/// comptime binding inside a `#Impure` block so the interpreter starts with the
/// gate already open for Tier-2 calls.
pub fn evaluate_owned_with_imports_opts(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
    initial_impure_depth: usize,
) -> Result<CtValue, Diagnostic> {
    let refs: HashMap<String, &Func> = funcs.iter().map(|(n, f)| (n.clone(), f)).collect();
    evaluate_with_imports_opts(
        init,
        &refs,
        extern_names,
        base_dir,
        globals,
        core_imports,
        allow_impure,
        initial_impure_depth,
    )
}

/// Like [`evaluate_owned_with_imports_opts`] but also returns Tier-1 embed
/// inputs (D-CTEFFECT1). Used by the sema Checker to collect embed hashes.
pub fn evaluate_owned_with_imports_opts_collecting(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
    initial_impure_depth: usize,
) -> Result<(CtValue, Vec<crate::AST::ComptimeInput>), Diagnostic> {
    let refs: HashMap<String, &Func> = funcs.iter().map(|(n, f)| (n.clone(), f)).collect();
    evaluate_with_imports_opts_collecting(
        init,
        &refs,
        extern_names,
        base_dir,
        globals,
        core_imports,
        allow_impure,
        initial_impure_depth,
    )
}

/// D-METADERIVE1=A: evaluate the body of a user-authored `derive T.Trait { … }`
/// block in a comptime scope where `type_param` is bound to `type_info`.
/// Returns the source fragments emitted by `emit(…)` calls (D-CTCODEGEN1=A).
pub fn evaluate_derive_body(
    body: &[crate::AST::Stmt],
    type_param: &str,
    type_info: CtValue,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
) -> Result<Vec<String>, Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel: FUEL_BUDGET,
        sink: None,
        core_imports: empty_imports(),
        debugger: None,
        depth: 0,
        cur_func: "derive".to_string(),
        impure_depth: 0,
        allow_impure: false,
        repl_mode: false,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        globals: empty_globals(),
        methods: empty_methods(),
        structs: empty_structs(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
    };
    let mut scope = HashMap::new();
    scope.insert(type_param.to_string(), type_info);
    interp.exec_block(body, &mut scope)?;
    Ok(interp.emitted_fragments)
}
