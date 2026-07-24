//! E2-M4 / D-LENS-RUN1 — shared JIT-lens execution driver.
//!
//! Default `jet run` and `jet dev` execute through strict Cranelift. The
//! tier-0 interpreter is explicit (`jet dev --interpret`) only.

use std::collections::HashMap;

use crate::Diagnostics::Diagnostic;
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
    if let Some(entry) = selected_entry(bundle, &funcs) {
        for stmt in &entry.body {
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

/// An explicit Output wins. Otherwise the legacy `run` spelling or sema's
/// checked default names the exact function; dev never re-resolves it.
fn selected_entry<'a>(
    bundle: &'a ProgramBundle,
    funcs: &'a HashMap<String, &'a Func>,
) -> Option<&'a Func> {
    let output = bundle
        .modules
        .get(bundle.entry)?
        .items
        .iter()
        .find_map(|item| {
            let Item::Const(value) = item else {
                return None;
            };
            value
                .resolved_output
                .as_ref()
                .filter(|output| output.selected)
        });
    if let Some(output) = output {
        let module = bundle.modules.get(output.module)?;
        return function_at(&module.items, output.definition);
    }
    funcs.get("run").copied()
}

fn function_at(items: &[Item], definition: crate::Diagnostics::Span) -> Option<&Func> {
    items.iter().find_map(|item| match item {
        Item::Func(function) if function.name_span == definition => Some(function),
        Item::CodeModule(module) => module
            .body
            .as_deref()
            .and_then(|items| function_at(items, definition)),
        _ => None,
    })
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
    // `comptime name = …` into `ConstDef::ct` while checking the program (the
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
                info.distinct_bases
                    .entry(d.name.clone())
                    .or_insert_with(|| d.base.clone());
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
                    info.distinct_bases
                        .entry(d.name.clone())
                        .or_insert(d.base);
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
        if let Some(diagnostic) = jet_driver::InterpreterBoundary::dev_boundary_scan(bundle) {
            return RunOutcome::Problems(vec![diagnostic]);
        }
    }
    let (funcs, program) = collect_funcs_and_info(bundle);
    let main = match selected_entry(bundle, &funcs) {
        Some(f) => f,
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
        if let Some(diagnostic) = jet_driver::InterpreterBoundary::dev_boundary_scan(bundle) {
            return RunOutcome::Problems(vec![diagnostic]);
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
    jet_driver::InterpreterBoundary::dev_boundary_diagnostic(
        format!("uses {detail}, which isn't covered by the dev interpreter yet"),
        d.span,
    )
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
fn checked_bundle(file: &str) -> Result<ProgramBundle, Vec<Diagnostic>> {
    match crate::Loader::load_entry_with_overlay(file, None, false) {
        Ok(mut bundle) => {
            let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Run);
            let errors: Vec<Diagnostic> = diags
                .into_iter()
                .filter(|d| matches!(d.severity, crate::Diagnostics::Severity::Error))
                .collect();
            if !errors.is_empty() {
                return Err(errors);
            }
            Ok(bundle)
        }
        Err(diags) => Err(diags),
    }
}

/// D-LENS-RUN1: load, check, and execute one native program through strict JIT.
pub fn run_jit_once(file: &str) -> RunOutcome {
    run_jit_once_with_args(file, &[])
}

/// D-LENS-RUN1: strict Cranelift run with the same argv shape AOT would see.
pub fn run_jit_once_with_args(file: &str, program_args: &[&str]) -> RunOutcome {
    match checked_bundle(file) {
        Ok(bundle) => {
            let mut args = Vec::with_capacity(program_args.len() + 1);
            args.push(file.to_string());
            args.extend(program_args.iter().map(|arg| (*arg).to_string()));
            jet_jit::with_program_args(&args, || {
                use crate::JitBackend::JitBackend;
                let mut backend = jet_jit::CraneliftBackend::new();
                backend.run(&bundle, false)
            })
        }
        Err(diags) => RunOutcome::Problems(diags),
    }
}

pub fn dev_iteration(file: &str, try_anyway: bool, use_interpreter: bool) -> RunOutcome {
    match checked_bundle(file) {
        Ok(bundle) => dev_run_bundle(&bundle, try_anyway, use_interpreter),
        Err(diags) => RunOutcome::Problems(diags),
    }
}

/// Run an already-checked bundle through the dev backend seam.
pub fn dev_run_bundle(
    bundle: &ProgramBundle,
    try_anyway: bool,
    use_interpreter: bool,
) -> RunOutcome {
    use crate::JitBackend::{InterpreterBackend, JitBackend};
    if use_interpreter {
        let mut backend = InterpreterBackend::new();
        backend.run(bundle, try_anyway)
    } else {
        use crate::JitBackend::JitBackend;
        let mut backend = jet_jit::CraneliftBackend::new();
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
