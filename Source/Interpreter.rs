//! E2-M4 — `jet dev` whole-program interpreter driver.
//!
//! This is the dev-loop convenience layer (D-DEV1…D-DEV4): it re-checks and
//! re-runs the entry file on every save, streaming output, for sub-200ms
//! feedback. It does NOT introduce a second interpreter — it reuses the M9.5
//! comptime tree-walker (`crate::comptime`) to execute `fn run()`. The bytes
//! it produces are identical to the compiled program (I2); the differential
//! battery in `tests/dev.rs` is the enforcement.
//!
//! Hard line (I2/I3): nothing here ever produces a release artifact. `jet
//! build`/`jet run` never touch this path. When the interpreter can't run a
//! program (FFI, tasks/channels, `#Unsafe`/`core.mem`, native-only Core), it
//! stops with **E2201** naming the feature and `jet build`/`jet run` — unless
//! the user opted in with "try anyway" (D-DEV1), which runs past the boundary
//! with no guarantees.

use std::collections::HashMap;

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{Expr, Func, Item, ProgramBundle, Stmt};

// c139: RunOutcome moved to jet-foundation so the jet-jit/ sibling crate
// can implement JitBackend without a dep cycle. Re-exported here so callers
// using `jet::Interpreter::RunOutcome` still work unchanged.
pub use jet_foundation::JitBackend::RunOutcome;

/// c77 (D-DEVMODE1=A): how `jet dev` should react to a save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevMode {
    /// A program that finishes on its own — rerun it from scratch on each save.
    RunToCompletion,
    /// A program that stays up (a top-level `loop`, or a `task.spawn`) — a
    /// type-stable edit takes the swap path, a type-changing edit announces a
    /// clean restart (D-HOTSWAP1).
    Resident,
}

/// c77 (D-DEVMODE1=A): auto-detect whether `run` runs to completion or stays
/// resident. A `run` whose body contains a top-level `loop { … }` or a
/// `*.spawn(...)` call (the `core.tasks` spawn surface) is `Resident`;
/// everything else is `RunToCompletion`. The scan only looks at `run`'s own
/// statement list (top level) per the D-DEVMODE1 Q2 rule — a `loop` buried
/// inside a helper does not make a program resident.
pub fn detect_dev_mode(bundle: &ProgramBundle) -> DevMode {
    let funcs = collect_funcs(bundle);
    if let Some(run) = funcs.get("run") {
        for stmt in &run.body {
            if stmt_is_resident(stmt) {
                return DevMode::Resident;
            }
        }
    }
    DevMode::RunToCompletion
}

/// A single top-level statement that marks a program resident: a `loop { … }`
/// or any statement whose expression is (or contains, top-level) a `.spawn(…)`
/// method call.
fn stmt_is_resident(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Loop { .. } => true,
        Stmt::Expr(e) => expr_has_spawn(e),
        Stmt::Val(b) => expr_has_spawn(&b.init),
        Stmt::Assign { value, .. } => expr_has_spawn(value),
        _ => false,
    }
}

/// True when `e` is, at this level, a `*.spawn(...)` method call — the
/// `core.tasks` resident-task surface (`tasks.spawn(() => …)`).
fn expr_has_spawn(e: &Expr) -> bool {
    matches!(e, Expr::MethodCall { method, .. } if method == "spawn")
}

/// A named feature the dev interpreter cannot execute (D-DEV1). The boundary
/// scan returns the first one it finds so the E2201 note can name it.
struct Boundary {
    /// Plain-language feature name, e.g. "spawns a task".
    feature: String,
    /// Where in the source the feature appears (best-effort).
    span: Option<Span>,
}

/// Build the E2201 boundary diagnostic: name the feature and point at the
/// real execution path (`jet build` / `jet run`).
fn boundary_diag(b: &Boundary) -> Diagnostic {
    Diagnostic::error(
        "E2201",
        format!(
            "`jet dev` can't interpret this program yet — it {}",
            b.feature
        ),
        "`jet dev` runs your program in a built-in interpreter for instant feedback, but that interpreter doesn't cover every feature; this one needs the real native build"
            .to_string(),
        "run `jet build` then the binary, or `jet run <file>` to compile and run it; `jet dev` will keep showing checks live"
            .to_string(),
        b.span,
    )
}

/// D-DBG3: the debugger's boundary scan. The `jet debug` source-level stepper
/// drives this same dev interpreter, so it declines the same features — but
/// with **E2203** (debug-specific) so the message names `jet debug` and points
/// at the real build (the native-backend follow-on, D-DBG3 step 2). Returns
/// `None` when the whole program is steppable.
pub fn debug_boundary_scan(bundle: &ProgramBundle) -> Option<Diagnostic> {
    boundary_scan(bundle).map(|b| {
        Diagnostic::error(
            "E2203",
            format!("`jet debug` can't step through this program yet — it {}", b.feature),
            "`jet debug` steps your program in the same interpreter `jet dev` uses; this feature touches threads, foreign code, raw memory, or the outside world, which the source-level stepper doesn't cover yet"
                .to_string(),
            "run `jet build` then the binary, or `jet run <file>` to compile and run it; remove the unsupported feature to step the rest, or wait for the native-debugger milestone (D-DBG3 step 2)"
                .to_string(),
            b.span,
        )
    })
}

/// Scan the whole bundle for the first feature the interpreter can't run
/// (D-DEV1). Pure walk over the typed AST — no execution.
fn boundary_scan(bundle: &ProgramBundle) -> Option<Boundary> {
    for module in &bundle.modules {
        // Native Core modules whose results aren't pure/deterministic enough to
        // interpret. The interpreter supports `print`/`eprint` only; anything
        // that reaches the filesystem, network, clock, RNG, environment, or
        // process table needs the real build.
        for imp in &module.imports {
            if let crate::AST::ImportKind::Module(name, span) = &imp.kind {
                if let Some(feature) = native_module_feature(name) {
                    return Some(Boundary {
                        feature: feature.to_string(),
                        span: Some(*span),
                    });
                }
            }
        }
        for item in &module.items {
            match item {
                Item::ExternRust(b) => {
                    return Some(Boundary {
                        feature: "calls into Rust code through `extern rust`".to_string(),
                        span: Some(b.span),
                    });
                }
                Item::CModule(c) => {
                    return Some(Boundary {
                        feature: "calls into a C library".to_string(),
                        span: Some(c.span),
                    });
                }
                Item::Impl(i)
                    if matches!(i.trait_name.as_deref(), Some("Encode" | "Decode")) =>
                {
                    return Some(Boundary {
                        feature: "uses a typed encoding implementation".to_string(),
                        span: i.trait_span.or(Some(i.type_span)),
                    });
                }
                Item::Func(f) => {
                    if f.is_unsafe {
                        return Some(Boundary {
                            feature: "uses an `#Unsafe` function".to_string(),
                            span: Some(f.name_span),
                        });
                    }
                    // D-CLIFLAG1: `fn run(args: T)` — the typed entry-signature
                    // CLI surface. The real build parses `io.args()` into `T`
                    // before calling `run`; the interpreter has no argv to
                    // parse from and no synthesis for the defaults, so it has
                    // nothing to bind `args` to. Declining honestly here beats
                    // running with `args` unbound (which previously surfaced as
                    // a confusing, unrelated E0956 deep in the body, or — worse
                    // — silently printed the wrong value, c139 JIT/interpreter
                    // parity finding).
                    if f.name == "run" && !f.params.is_empty() {
                        return Some(Boundary {
                            feature: "uses a typed CLI entry signature (`fn run(args: T)`)"
                                .to_string(),
                            span: Some(f.name_span),
                        });
                    }
                    if let Some(b) = scan_stmts_for_unsafe(&f.body) {
                        return Some(b);
                    }
                    // c77 (Q2 hard rule): a call passing a `&` (write) argument
                    // asks for writeback the scalar-by-value tree-walker
                    // doesn't perform (the callee's frame is discarded, never
                    // written back to the caller's binding), so its output
                    // could diverge from the compiled build. Stop honestly at
                    // the boundary rather than risk a silent miscompile. (A
                    // `^` move argument is fine — see `expr_mut_arg`.)
                    if let Some(b) = scan_stmts_for_mut_arg(&f.body) {
                        return Some(b);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Map a `use core.<x>` module name to the boundary feature it represents, or
/// `None` if the interpreter can run it (only `core.io` reaches IO we support,
/// and even there `input`/`read_all_input` are non-deterministic — but those
/// surface naturally as E0956 if reached, keeping the scan conservative).
fn native_module_feature(name: &str) -> Option<&'static str> {
    match name {
        "core.tasks" => Some("spawns a task or uses a channel"),
        "core.mem" => Some("uses the low-level `core.mem` tier"),
        "core.files" => Some("reads or writes files"),
        "core.env" => Some("reads the environment"),
        "core.process" => Some("runs another process or exits early"),
        "core.random" => Some("uses random numbers"),
        "core.time" => Some("reads the clock or sleeps"),
        _ => None,
    }
}

/// Find the first `#Unsafe { … }` block anywhere in a statement list.
fn scan_stmts_for_unsafe(stmts: &[Stmt]) -> Option<Boundary> {
    for s in stmts {
        if let Some(b) = scan_stmt_for_unsafe(s) {
            return Some(b);
        }
    }
    None
}

fn scan_stmt_for_unsafe(s: &Stmt) -> Option<Boundary> {
    match s {
        Stmt::Unsafe { span, .. } => Some(Boundary {
            feature: "uses an `#Unsafe` block".to_string(),
            span: Some(*span),
        }),
        Stmt::If(ifs) => scan_if_for_unsafe(ifs),
        Stmt::While { body, .. } | Stmt::Loop { body, .. } | Stmt::CountedLoop { body, .. } => {
            scan_stmts_for_unsafe(body)
        }
        Stmt::For { body, .. } => scan_stmts_for_unsafe(body),
        Stmt::Switch {
            arms, else_body, ..
        } => {
            for a in arms {
                if let Some(b) = scan_stmts_for_unsafe(&a.body) {
                    return Some(b);
                }
            }
            else_body.as_ref().and_then(|b| scan_stmts_for_unsafe(b))
        }
        _ => None,
    }
}

fn scan_if_for_unsafe(ifs: &crate::AST::IfStmt) -> Option<Boundary> {
    if let Some(b) = scan_stmts_for_unsafe(&ifs.then_body) {
        return Some(b);
    }
    match &ifs.else_branch {
        Some(crate::AST::ElseBranch::ElseIf(inner)) => scan_if_for_unsafe(inner),
        Some(crate::AST::ElseBranch::Else(body)) => scan_stmts_for_unsafe(body),
        None => None,
    }
}

/// c77: find the first call passing a `mut` (Write-convention) argument — the
/// writeback the scalar-by-value tree-walker doesn't perform. Walks bodies and
/// the expressions inside them.
fn scan_stmts_for_mut_arg(stmts: &[Stmt]) -> Option<Boundary> {
    for s in stmts {
        if let Some(b) = scan_stmt_for_mut_arg(s) {
            return Some(b);
        }
    }
    None
}

fn scan_stmt_for_mut_arg(s: &Stmt) -> Option<Boundary> {
    match s {
        Stmt::Expr(e) => expr_mut_arg(e),
        Stmt::Val(b) => expr_mut_arg(&b.init),
        Stmt::Assign { value, .. } => expr_mut_arg(value),
        Stmt::Return(Some(e), _) => expr_mut_arg(e),
        Stmt::If(ifs) => scan_if_for_mut_arg(ifs),
        Stmt::While { cond, body, .. } => {
            expr_mut_arg(cond).or_else(|| scan_stmts_for_mut_arg(body))
        }
        Stmt::CountedLoop { cond, body, .. } => {
            expr_mut_arg(cond).or_else(|| scan_stmts_for_mut_arg(body))
        }
        Stmt::Loop { body, .. } => scan_stmts_for_mut_arg(body),
        Stmt::For { body, .. } => scan_stmts_for_mut_arg(body),
        Stmt::Switch {
            arms, else_body, ..
        } => {
            for a in arms {
                if let Some(b) = scan_stmts_for_mut_arg(&a.body) {
                    return Some(b);
                }
            }
            else_body.as_ref().and_then(|b| scan_stmts_for_mut_arg(b))
        }
        _ => None,
    }
}

fn scan_if_for_mut_arg(ifs: &crate::AST::IfStmt) -> Option<Boundary> {
    if let Some(b) = expr_mut_arg(&ifs.cond) {
        return Some(b);
    }
    if let Some(b) = scan_stmts_for_mut_arg(&ifs.then_body) {
        return Some(b);
    }
    match &ifs.else_branch {
        Some(crate::AST::ElseBranch::ElseIf(inner)) => scan_if_for_mut_arg(inner),
        Some(crate::AST::ElseBranch::Else(body)) => scan_stmts_for_mut_arg(body),
        None => None,
    }
}

/// Does this expression (or a subexpression) pass a `&` (write/edit) argument
/// *bound to a named variable* to a call? The scalar-by-value tree-walker
/// runs the callee in its own frame and never writes a mutated parameter
/// back into the caller's binding, so this would silently diverge from the
/// compiled build's real writeback. Stop honestly at the boundary instead.
///
/// A `^` (move) argument on a named variable is NOT a boundary: sema's move
/// checker (`CheckerOwnership`) already forbids reading `name` again on any
/// path after `^name` is passed, so the interpreter evaluating it exactly
/// like an ordinary read — same value, same one use — can never be observed
/// to differ from the compiled build's real ownership transfer.
fn expr_mut_arg(e: &Expr) -> Option<Boundary> {
    use crate::AST::AccessConvention;
    let boundary = |span: Span| {
        Some(Boundary {
            feature: "passes a `&` argument to a function (writeback isn't interpreted yet)"
                .to_string(),
            span: Some(span),
        })
    };
    // A call arg is a boundary when it is `&` on a named variable.
    let arg_boundary = |a: &crate::AST::CallArg| -> Option<Boundary> {
        if matches!(a.convention, AccessConvention::Write) && matches!(a.expr, Expr::Ident(..)) {
            return boundary(a.span);
        }
        expr_mut_arg(&a.expr)
    };
    match e {
        Expr::Call(c) => {
            for a in &c.args {
                if let Some(b) = arg_boundary(a) {
                    return Some(b);
                }
            }
            None
        }
        Expr::MethodCall { receiver, args, .. } => {
            if let Some(b) = expr_mut_arg(receiver) {
                return Some(b);
            }
            for a in args {
                if let Some(b) = arg_boundary(a) {
                    return Some(b);
                }
            }
            None
        }
        Expr::CallValue { callee, args, .. } => {
            if let Some(b) = expr_mut_arg(callee) {
                return Some(b);
            }
            for a in args {
                if let Some(b) = arg_boundary(a) {
                    return Some(b);
                }
            }
            None
        }
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Field(inner, _, _) => expr_mut_arg(inner),
        Expr::Binary(_, l, r, _) => expr_mut_arg(l).or_else(|| expr_mut_arg(r)),
        Expr::Index { base, index, .. } => expr_mut_arg(base).or_else(|| expr_mut_arg(index)),
        _ => None,
    }
}

/// Collect every top-level function across all modules into the flat name→func
/// map the comptime evaluator expects. (Module-qualified user functions aren't
/// dev-interpreted yet; they surface as E0956 if called.)
fn collect_funcs(bundle: &ProgramBundle) -> HashMap<String, &Func> {
    let mut funcs = HashMap::new();
    for module in &bundle.modules {
        for item in &module.items {
            if let Item::Func(f) = item {
                funcs.entry(f.name.clone()).or_insert(f);
            }
        }
    }
    funcs
}

/// c139 JIT/interpreter-parity: extend `collect_funcs` with everything else
/// `jet dev` needs to run whole programs at parity with the real build —
/// D-MOD2 code-module namespaced functions (both the real `alias__fn`
/// mangling and, when unclaimed, the bare name — covers a private sibling
/// call inside the module and a `use mod.item` selective import in one move,
/// with `use mod.item as alias` handled explicitly below), user-written
/// instance/associated methods, D-FIELDPOL1 computed fields, and
/// D-RANGETYPE1/D-DIST1 distinct-type constructors. Consts/`comptime`
/// bindings are pre-evaluated into `globals` from sema's own `ConstDef::ct`
/// (I2: the exact value baked into the real build, not a re-derivation).
fn collect_funcs_and_info<'a>(
    bundle: &'a ProgramBundle,
) -> (HashMap<String, &'a Func>, crate::Comptime::ProgramInfo<'a>) {
    let mut funcs: HashMap<String, &Func> = HashMap::new();
    let mut info = crate::Comptime::ProgramInfo::empty();
    for module in &bundle.modules {
        walk_items_for_interp(&module.items, &mut funcs, &mut info);
    }
    // D-SELIMPORT1=A: `use mod.item as alias` for a *local* code module (not
    // `core`/`jet`) — the bare-name fallback above already covers an
    // unaliased `use mod.item`.
    for module in &bundle.modules {
        for imp in &module.imports {
            let crate::AST::ImportKind::Unqualified {
                module_alias,
                items,
                ..
            } = &imp.kind
            else {
                continue;
            };
            if module_alias == "core" || module_alias == "jet" {
                continue;
            }
            for (orig, alias_opt) in items {
                let Some(local) = alias_opt else { continue };
                let qualified = format!("{}__{}", module_alias, orig);
                if let Some(f) = funcs.get(qualified.as_str()).copied() {
                    funcs.entry(local.clone()).or_insert(f);
                }
            }
        }
    }
    // Core module aliases (`use core.math as math`, `use core.{sqrt}`),
    // merged across every module the same way `funcs` already is — dev mode
    // has no per-module scoping (see the doc comment on `collect_funcs`).
    for module in &bundle.modules {
        for imp in &module.imports {
            if let Some(core_module) = imp.core_module_path() {
                info.core_imports
                    .entry(imp.import_alias())
                    .or_insert(core_module);
                continue;
            }
            if let crate::AST::ImportKind::Unqualified {
                module_alias,
                items,
                ..
            } = &imp.kind
            {
                if module_alias == "core" || module_alias == "jet" {
                    for (orig, alias_opt) in items {
                        let local = alias_opt.clone().unwrap_or_else(|| orig.clone());
                        let full = format!("core.{}", orig);
                        if crate::Syntax::is_known_core_module(&full) {
                            info.core_imports.entry(local).or_insert(full);
                        }
                    }
                }
            }
        }
    }
    // Top-level `const`/`comptime` bindings: sema already evaluated every
    // `comptime NAME = …` into `ConstDef::ct` while checking the program (the
    // caller of `run_checked` guarantees the front end already ran) — reuse
    // that value rather than re-evaluating, so a `jet dev` run of the const
    // matches the real build bit-for-bit (I2).
    for module in &bundle.modules {
        for item in &module.items {
            if let Item::Const(c) = item {
                if let Some(v) = &c.ct {
                    info.globals
                        .entry(c.name.clone())
                        .or_insert_with(|| v.clone());
                }
            }
        }
    }
    (funcs, info)
}

/// One item-list pass for [`collect_funcs_and_info`]: top-level functions,
/// `impl`/in-struct instance methods, computed fields, distinct-type
/// constructors, and one level of D-MOD2 inline/generic-instantiated code
/// modules (their own `Item::Func`s only — module-in-module nesting isn't a
/// shape any current example produces, so it isn't walked recursively).
fn walk_items_for_interp<'a>(
    items: &'a [Item],
    funcs: &mut HashMap<String, &'a Func>,
    info: &mut crate::Comptime::ProgramInfo<'a>,
) {
    for item in items {
        match item {
            Item::Func(f) => {
                funcs.entry(f.name.clone()).or_insert(f);
            }
            Item::Impl(i) => {
                for m in &i.methods {
                    info.methods
                        .entry((i.type_name.clone(), m.name.clone()))
                        .or_insert(m);
                }
            }
            Item::Struct(s) => {
                info.structs.entry(s.name.clone()).or_insert(s);
                for m in &s.methods {
                    info.methods
                        .entry((s.name.clone(), m.name.clone()))
                        .or_insert(m);
                }
                for blk in &s.trait_impls {
                    for m in &blk.methods {
                        info.methods
                            .entry((s.name.clone(), m.name.clone()))
                            .or_insert(m);
                    }
                }
                for field in &s.fields {
                    if let Some(expr) = &field.computed {
                        info.computed_fields
                            .entry((s.name.clone(), field.name.clone()))
                            .or_insert(expr.as_ref());
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    info.methods
                        .entry((e.name.clone(), m.name.clone()))
                        .or_insert(m);
                }
                for blk in &e.trait_impls {
                    for m in &blk.methods {
                        info.methods
                            .entry((e.name.clone(), m.name.clone()))
                            .or_insert(m);
                    }
                }
            }
            Item::Distinct(d) => {
                info.distinct_ranges
                    .entry(d.name.clone())
                    .or_insert(d.range.map(|(lo, hi, _)| (lo, hi)));
            }
            // Card #392 pass 5: `migration TypeName { … }` blocks, for
            // `decode_traced<T>`'s runtime chain-walker (`Interp::migrations`).
            Item::Migration(m) => {
                info.migrations.entry(m.type_name.clone()).or_default().push(m);
            }
            Item::UnitFamily(uf) => {
                for d in uf.distinct_defs() {
                    info.distinct_ranges
                        .entry(d.name.clone())
                        .or_insert(d.range.map(|(lo, hi, _)| (lo, hi)));
                }
            }
            Item::CodeModule(cm) => {
                if let Some(body) = &cm.body {
                    for it in body {
                        if let Item::Func(f) = it {
                            funcs.entry(format!("{}__{}", cm.name, f.name)).or_insert(f);
                            // Bare-name fallback: a private sibling call inside
                            // the module, or an unaliased `use mod.item`
                            // selective import — see `collect_funcs_and_info`.
                            funcs.entry(f.name.clone()).or_insert(f);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Run a *checked* bundle in the interpreter (E2-M4). The caller has already
/// run the front end and confirmed there are no errors. `try_anyway` (D-DEV1)
/// skips the E2201 boundary scan and attempts execution with no guarantees.
pub fn run_checked(bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
    if !try_anyway {
        if let Some(b) = boundary_scan(bundle) {
            return RunOutcome::Problems(vec![boundary_diag(&b)]);
        }
    }
    let (funcs, program) = collect_funcs_and_info(bundle);
    let main = match funcs.get("run") {
        Some(f) => *f,
        None => {
            return RunOutcome::Problems(vec![Diagnostic::error(
                "E2201",
                "`jet dev` needs a `run` function to run".to_string(),
                "`jet dev` runs a program; a library with no `run` has nothing to execute"
                    .to_string(),
                "add `fn run() { … }`, or use `jet check <file>` to look for problems without running"
                    .to_string(),
                None,
            )]);
        }
    };
    let base_dir = &bundle.project_root;
    let mut sink = crate::Comptime::DevSink::new();
    match crate::Comptime::run_main(main, &funcs, base_dir, &mut sink, &program) {
        Ok(crate::Comptime::CtValue::ResErr(error)) => {
            sink.stderr.push_str(&error.jet_show());
            sink.stderr.push('\n');
            RunOutcome::Ran {
                stdout: sink.stdout,
                stderr: sink.stderr,
                exit_code: 1,
            }
        }
        Ok(_) => RunOutcome::Ran {
            stdout: sink.stdout,
            stderr: sink.stderr,
            exit_code: 0,
        },
        Err(d) => RunOutcome::Problems(vec![dev_boundary_from_comptime(d)]),
    }
}

/// D-SCHEDULE1 (ratified 2026-07-11, card #505): run one `#Task fn` by name,
/// the same way `run_checked` runs `fn run()` — the `jet dev` consumer
/// (`Source/CmdDevTools.rs`'s due-task tick) calls this to invoke a scheduled
/// task automatically. The caller has already filtered to `Func::is_task`
/// fns pulled from this same checked bundle, so a missing name here is an
/// internal-tooling mismatch, not a source error.
pub fn run_named_task(bundle: &ProgramBundle, name: &str, try_anyway: bool) -> RunOutcome {
    if !try_anyway {
        if let Some(b) = boundary_scan(bundle) {
            return RunOutcome::Problems(vec![boundary_diag(&b)]);
        }
    }
    let (funcs, program) = collect_funcs_and_info(bundle);
    let Some(task_fn) = funcs.get(name) else {
        return RunOutcome::Problems(vec![Diagnostic::error(
            "E2201",
            format!("`{name}` isn't a task in this file"),
            "the dev loop's due-task tick looks up the task by name in the checked bundle."
                .to_string(),
            "this is an internal-tooling mismatch, not a source error — please report it."
                .to_string(),
            None,
        )]);
    };
    let base_dir = &bundle.project_root;
    let mut sink = crate::Comptime::DevSink::new();
    match crate::Comptime::run_main(task_fn, &funcs, base_dir, &mut sink, &program) {
        Ok(crate::Comptime::CtValue::ResErr(error)) => {
            sink.stderr.push_str(&error.jet_show());
            sink.stderr.push('\n');
            RunOutcome::Ran {
                stdout: sink.stdout,
                stderr: sink.stderr,
                exit_code: 1,
            }
        }
        Ok(_) => RunOutcome::Ran {
            stdout: sink.stdout,
            stderr: sink.stderr,
            exit_code: 0,
        },
        Err(d) => RunOutcome::Problems(vec![dev_boundary_from_comptime(d)]),
    }
}

/// D-SCHEDULE1: the `#Task`/`#Every(…)` facts the dev loop's due-task tick
/// needs — a task's name and its resolved schedule (`None` for a `#Task fn`
/// with no `#Every(…)`, i.e. manual-invocation-only). Scoped to the entry
/// module's top-level items only (D-JPK-TASKRUN1: a task lives "beside `fn
/// run()`" — the same file, not an imported one). Sema has already rejected
/// a bad `#Every(…)` value (E0926) by the time a bundle reaches `jet dev`,
/// so `resolve()` failing here is defensive, not a real path.
pub fn scheduled_tasks(bundle: &ProgramBundle) -> Vec<(String, crate::AST::EverySchedule)> {
    let Some(entry) = bundle.modules.get(bundle.entry) else {
        return Vec::new();
    };
    entry
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Func(f) if f.is_task => {
                let schedule = f.every.as_ref()?.arg.resolve().ok()?;
                Some((f.name.clone(), schedule))
            }
            _ => None,
        })
        .collect()
}

/// c139 JIT-parity fix (2026-07-03): the dev interpreter IS the comptime
/// tree-walker (see module doc), so a construct it can't run leaks the
/// comptime evaluator's own E0956 ("unsupported")/E0951 ("impurity") codes —
/// correct for a real `comptime { }` block, but wrong voice here: the "compute
/// this at runtime" fix advice is nonsense when the user is already trying to
/// run this at runtime via `jet dev`. Rewrap as the dev-loop's own E2201
/// boundary diagnostic instead, preserving what construct tripped it.
fn dev_boundary_from_comptime(d: Diagnostic) -> Diagnostic {
    let detail = match d.code.as_str() {
        "E0956" => d
            .what
            .strip_suffix(" can't run at compile time yet")
            .unwrap_or(&d.what)
            .replace(" at compile time", ""),
        "E0951" => {
            "code that touches the outside world (network, filesystem, or environment)".to_string()
        }
        _ => return d,
    };
    boundary_diag(&Boundary {
        feature: format!("uses {detail}, which isn't covered by the dev interpreter yet"),
        span: d.span,
    })
}

/// One iteration of the `jet dev` watch loop, factored out so it can be
/// golden-tested without the long-running file watcher (the outer loop is a
/// thin shell around this). Loads + checks the file exactly like batch
/// compilation (D-DEV: identical diagnostics), then runs via the selected
/// backend.
///
/// `use_interpreter` — D-JIT2=A: when false (default for `jet dev`), the
/// Cranelift tier-1 backend wraps the interpreter; when true (`--interpret`),
/// tier-0 interpreter only.
pub fn dev_iteration(file: &str, try_anyway: bool, use_interpreter: bool) -> RunOutcome {
    match crate::Loader::load_entry_with_overlay(file, None, false) {
        Ok(mut bundle) => {
            let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Run);
            let errors: Vec<Diagnostic> = diags
                .into_iter()
                .filter(|d| matches!(d.severity, crate::Diagnostics::Severity::Error))
                .collect();
            if !errors.is_empty() {
                return RunOutcome::Problems(errors);
            }
            dev_run_bundle(&bundle, try_anyway, use_interpreter)
        }
        Err(diags) => RunOutcome::Problems(diags),
    }
}

/// Run an already-checked bundle through the dev backend seam.
pub fn dev_run_bundle(
    bundle: &ProgramBundle,
    try_anyway: bool,
    use_interpreter: bool,
) -> RunOutcome {
    use crate::JitBackend::{AotFallbackBackend, InterpreterBackend, JitBackend};
    if use_interpreter {
        let mut backend = InterpreterBackend::new();
        backend.run(bundle, try_anyway)
    } else {
        let mut backend =
            jet_jit::CraneliftBackend::new(AotFallbackBackend::new(InterpreterBackend::new()));
        backend.run(bundle, try_anyway)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse `src` into a bundle via a temp file (the only loader entry point).
    fn bundle_from(src: &str, tag: &str) -> ProgramBundle {
        let p = std::env::temp_dir().join(format!("jet_devmode_{tag}.jet"));
        std::fs::write(&p, src).unwrap();
        crate::Loader::load_entry(p.to_str().unwrap()).expect("bundle should load")
    }

    #[test]
    fn run_to_completion_is_the_default() {
        let b = bundle_from("fn run() {\n    print(\"hi\")\n}\n", "rtc");
        assert_eq!(detect_dev_mode(&b), DevMode::RunToCompletion);
    }

    #[test]
    fn top_level_loop_is_resident() {
        let b = bundle_from("fn run() {\n    loop {\n        break\n    }\n}\n", "loop");
        assert_eq!(detect_dev_mode(&b), DevMode::Resident);
    }

    #[test]
    fn loop_inside_a_helper_is_not_resident() {
        // Only a top-level `loop` in `run` makes a program resident; a loop in
        // a callee runs to completion.
        let src = "fn work() {\n    loop {\n        break\n    }\n}\nfn run() {\n    work()\n}\n";
        let b = bundle_from(src, "helper");
        assert_eq!(detect_dev_mode(&b), DevMode::RunToCompletion);
    }

    #[test]
    fn task_spawn_is_resident() {
        let src = "use core.tasks as tasks\nfn job() -> Int {\n    return 1\n}\nfn run() {\n    h :: tasks.spawn(() => job())\n    print(h.join())\n}\n";
        let b = bundle_from(src, "spawn");
        assert_eq!(detect_dev_mode(&b), DevMode::Resident);
    }

    #[test]
    fn resident_jit_safe_task_examples() {
        for file in [
            "examples/features/concurrency/tasks.jet",
            "examples/features/concurrency/scheduler_spawn.jet",
        ] {
            let mut bundle =
                crate::Loader::load_entry(file).unwrap_or_else(|_| panic!("load {file}"));
            let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Run);
            assert!(
                diags
                    .iter()
                    .all(|d| !matches!(d.severity, crate::Diagnostics::Severity::Error)),
                "{file} must type-check"
            );
            let detail = jet_jit::resident_jit_safe_bundle_detail(&bundle);
            if !detail.is_empty() {
                eprintln!("{file}: {detail}");
                if file.contains("160") {
                    for line in jet_jit::jit_dump_main_stmts(&bundle) {
                        eprintln!("  {line}");
                    }
                }
            }
            assert!(detail.is_empty(), "{file} must be resident-safe: {detail}");
        }
    }
}
