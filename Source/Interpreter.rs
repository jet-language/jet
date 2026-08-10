//! E2-M4 / D-LENS-RUN2 — shared JIT-lens execution driver.
//!
//! Default `jet run` and `jet dev` execute through tiered Cranelift with
//! silent interpreter deopt on named coverage gaps. Explicit
//! `jet run --interpret` and `jet dev --interpret` force tier-0 only.
//! Experts use `--trace-tiers`.

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

/// Run a *checked* bundle in the interpreter (E2-M4). The caller has already
/// run the front end and confirmed there are no errors. `try_anyway` (D-DEV1)
/// skips the E2201 boundary scan and attempts execution with no guarantees.
pub fn run_checked(bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
    crate::boot_tir_eval();
    if !try_anyway {
        if let Some(diagnostic) = jet_driver::InterpreterBoundary::dev_boundary_scan(bundle) {
            return RunOutcome::Problems(vec![diagnostic]);
        }
    }
    let mut sink = crate::Comptime::DevSink::new();
    match crate::Comptime::TirBridge::run_bundle(bundle, &mut sink, true) {
        Ok(crate::Comptime::CtValue::Failed(crate::Comptime::CtReport::Told(error))) => {
            let rendered = error
                .to_jet_err()
                .map(|error| jet_foundation::Outcome::jet_render_err(&error))
                .unwrap_or_else(|| error.jet_show());
            sink.stderr.push_str(&rendered);
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
            exit_code: sink.exit_code.unwrap_or(0),
        },
        Err(d) if sink.exit_code.is_some() || d.code == "SOFT_EXIT" => RunOutcome::Ran {
            stdout: sink.stdout,
            stderr: sink.stderr,
            exit_code: sink
                .exit_code
                .unwrap_or_else(|| d.what.parse().unwrap_or(0)),
        },
        // Whole-program interpret traps are live-program panics (I9 / #1483),
        // not comptime build failures — match AOT exit 70 + `panic:` wording.
        Err(d) if d.code == "E0953" => runtime_trap_from_e0953(sink, d),
        Err(d) => RunOutcome::Problems(vec![dev_boundary_from_comptime(d)]),
    }
}

fn runtime_trap_from_e0953(mut sink: crate::Comptime::DevSink, d: Diagnostic) -> RunOutcome {
    let msg = d
        .why
        .strip_prefix("while computing this value at compile time, the program panicked: ")
        .unwrap_or(d.why.as_str());
    if !sink.stderr.is_empty() && !sink.stderr.ends_with('\n') {
        sink.stderr.push('\n');
    }
    sink.stderr.push_str("panic: ");
    sink.stderr.push_str(msg);
    sink.stderr.push('\n');
    RunOutcome::Ran {
        stdout: sink.stdout,
        stderr: sink.stderr,
        exit_code: 70,
    }
}

/// D-SCHEDULE1 (ratified 2026-07-11, card #505): run one `#Job fn` by name,
/// the same way `run_checked` runs `fn run()` — the `jet dev` consumer
/// (`Source/CmdDevTools.rs`'s due-task tick) calls this to invoke a scheduled
/// task automatically. The caller has already filtered to `Func::is_task`
/// fns pulled from this same checked bundle, so a missing name here is an
/// internal-tooling mismatch, not a source error.
pub fn run_named_task(bundle: &ProgramBundle, name: &str, try_anyway: bool) -> RunOutcome {
    crate::boot_tir_eval();
    if !try_anyway {
        if let Some(diagnostic) = jet_driver::InterpreterBoundary::dev_boundary_scan(bundle) {
            return RunOutcome::Problems(vec![diagnostic]);
        }
    }
    // Tasks share the same TIR program; re-entry is by temporarily selecting
    // the named function as the program entry via a lowered copy.
    let Some(mut program) = crate::Codegen::TIR::lower_interp_program(bundle) else {
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
    if !program.funcs.iter().any(|f| f.name == name) {
        return RunOutcome::Problems(vec![Diagnostic::error(
            "E2201",
            format!("`{name}` isn't a task in this file"),
            "the dev loop's due-task tick looks up the task by name in the checked bundle."
                .to_string(),
            "this is an internal-tooling mismatch, not a source error — please report it."
                .to_string(),
            None,
        )]);
    }
    program.entry = name.to_string();
    let mut sink = crate::Comptime::DevSink::new();
    let mut globals = std::collections::HashMap::new();
    let mut core_imports = std::collections::HashMap::new();
    for module in &bundle.modules {
        for item in &module.items {
            if let Item::Const(c) = item {
                if let Some(v) = &c.ct {
                    globals.entry(c.name.clone()).or_insert_with(|| v.clone());
                }
            }
        }
        for imp in &module.imports {
            if let Some(core_module) = imp.core_module_path() {
                core_imports
                    .entry(imp.import_alias())
                    .or_insert(core_module);
            }
        }
    }
    match crate::Codegen::TIR::run_program_with_structs(
        &program,
        &bundle.project_root,
        &mut sink,
        globals,
        &core_imports,
        true,
        {
            let mut fields = std::collections::HashMap::new();
            for module in &bundle.modules {
                for item in &module.items {
                    if let Item::Struct(s) = item {
                        fields.insert(
                            s.name.clone(),
                            s.fields
                                .iter()
                                .map(|f| (f.name.clone(), f.redact))
                                .collect(),
                        );
                    }
                }
            }
            fields
        },
        {
            let mut fields = std::collections::HashMap::new();
            for module in &bundle.modules {
                for item in &module.items {
                    if let Item::Struct(s) = item {
                        fields.insert(
                            s.name.clone(),
                            s.fields
                                .iter()
                                .map(|f| (f.name.clone(), f.ty.clone()))
                                .collect(),
                        );
                    }
                }
            }
            fields
        },
    ) {
        Ok(crate::Comptime::CtValue::Failed(crate::Comptime::CtReport::Told(error))) => {
            let rendered = error
                .to_jet_err()
                .map(|error| jet_foundation::Outcome::jet_render_err(&error))
                .unwrap_or_else(|| error.jet_show());
            sink.stderr.push_str(&rendered);
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
        Err(d) if d.code == "E0953" => runtime_trap_from_e0953(sink, d),
        Err(d) => RunOutcome::Problems(vec![dev_boundary_from_comptime(d)]),
    }
}

/// D-SCHEDULE1: the `#Job`/`#Every(…)` facts the dev loop's due-task tick
/// needs — a task's name and its resolved schedule (`None` for a `#Job fn`
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
                if f.task_metadata.as_ref().and_then(|metadata| {
                    metadata
                        .skip
                        .as_ref()
                        .and_then(|skip| skip.reason_for_host(&jetpack::Platform::host_key()))
                }).is_some() {
                    return None;
                }
                let schedule = f.every.as_ref()?.arg.resolve().ok()?;
                Some((f.name.clone(), schedule))
            }
            _ => None,
        })
        .collect()
}

/// c139 JIT-parity fix (2026-07-03): the dev interpreter IS the comptime
/// tree-walker (see module doc), so a construct it can't run leaks the
/// comptime evaluator's own E0956 ("unsupported")/E0951 ("impurity") /
/// E3410/E3412 (Tier-2 / live-net comptime) codes — correct for a real
/// `$ { }` block, but wrong voice here: the "compute this at runtime"
/// / "only fetch at comptime" fix advice is nonsense when the user is already
/// trying to run this at runtime via `jet dev`. Rewrap as the dev-loop's own
/// E2201 boundary diagnostic instead, preserving what construct tripped it.
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
        // Live sockets / Tier-2 ambient I/O: same rewriter as E0956 — keep the
        // call site, drop the comptime-only framing (#1247 three-way battery).
        "E3412" => d
            .what
            .strip_suffix(" is not available at comptime")
            .unwrap_or(&d.what)
            .to_string(),
        "E3410" => d
            .what
            .split(" is a Tier-2")
            .next()
            .unwrap_or(&d.what)
            .to_string(),
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
    jet_driver::run_compiler_work(|| {
        crate::RunCache::note_parse();
        let source = std::fs::read_to_string(file).ok();
        let web_entry = source.as_ref().and_then(|source| {
            let (tokens, diagnostics) = crate::Lexer::lex(source);
            if !diagnostics.is_empty() {
                return None;
            }
            let program = crate::Parser::parse(&tokens).ok()?;
            let has_app = program.items.iter().any(|item| {
                matches!(
                    item,
                    crate::AST::Item::Func(function)
                        if function.name == "app"
                            && matches!(
                                function.return_type.as_ref(),
                                Some(crate::AST::Type::Named(name)) if name == "WebApp"
                            )
                )
            });
            let has_run = program.items.iter().any(
                |item| matches!(item, crate::AST::Item::Func(function) if function.name == "run"),
            );
            (has_app && !has_run)
                .then(|| format!("{source}\nfn run() {{ app().serve() }}\n"))
        });
        let overlay_path = std::fs::canonicalize(file).unwrap_or_else(|_| {
            std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(file)
        });
        let overlay = web_entry
            .as_deref()
            .map(|source| (overlay_path.as_path(), source));
        match crate::Loader::load_entry_with_overlay(file, overlay, false) {
            Ok(mut bundle) => {
                crate::RunCache::note_check();
                let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Run);
                // Same gate as `jet build` / entry-swap: recoverable parse
                // teaching must not disappear on the default `jet run` path.
                // Extension hooks are empty here (no plugin session on plain run);
                // `compile_bundle_path_opts_full` still passes them on the build path.
                let _lints = crate::Driver::gate_diagnostics(
                    std::mem::take(&mut bundle.parse_teaching),
                    diags,
                    Vec::new(),
                )?;
                Ok(bundle)
            }
            Err(diags) => Err(diags),
        }
    })
}

/// D-LENS-RUN1: load, check, and execute one native program through strict JIT.
pub fn run_jit_once(file: &str) -> RunOutcome {
    run_jit_once_with_args(file, &[])
}

/// D-LENS-RUN1: strict Cranelift run with the same argv shape AOT would see.
pub fn run_jit_once_with_args(file: &str, program_args: &[&str]) -> RunOutcome {
    run_jit_once_with_args_opts(file, program_args, false)
}

/// Like [`run_jit_once_with_args`], with `json` suppressing the jet-dev signpost.
pub fn run_jit_once_with_args_opts(
    file: &str,
    program_args: &[&str],
    json: bool,
) -> RunOutcome {
    crate::RunCache::reset_phases();
    let started = std::time::Instant::now();
    let entry = std::path::Path::new(file);
    if let Some(outcome) = crate::RunCache::try_warm_run(entry, program_args) {
        return outcome;
    }
    match checked_bundle(file) {
        Ok(bundle) => {
            crate::RunCache::note_lower();
            crate::RunCache::note_codegen();
            let mut args = Vec::with_capacity(program_args.len() + 1);
            args.push(file.to_string());
            args.extend(program_args.iter().map(|arg| (*arg).to_string()));
            let outcome = jet_jit::with_program_args(&args, || {
                use crate::JitBackend::JitBackend;
                let mut backend = jet_jit::CraneliftBackend::new();
                backend.run(&bundle, false)
            });
            if matches!(outcome, RunOutcome::Ran { .. }) {
                crate::RunCache::store_after_miss(entry, program_args);
            }
            if !json {
                crate::RunCache::maybe_signpost(started, crate::RunCache::stderr_is_tty());
            }
            outcome
        }
        Err(diags) => RunOutcome::Problems(diags),
    }
}

/// Run one program through the tier-0 interpreter with the same argv shape as
/// the default run path.
pub fn run_interpreter_once_with_args(file: &str, program_args: &[&str]) -> RunOutcome {
    crate::RunCache::reset_phases();
    let trace_tiers = jet_jit::trace_tiers_enabled();
    let (outcome, flags, rows) = jet_driver::run_compiler_work(|| {
        jet_jit::set_trace_tiers(trace_tiers);
        let outcome = match checked_bundle(file) {
            Ok(bundle) => {
                let mut args = Vec::with_capacity(program_args.len() + 1);
                args.push(file.to_string());
                args.extend(program_args.iter().map(|arg| (*arg).to_string()));
                jet_jit::with_program_args(&args, || dev_run_bundle(&bundle, false, true))
            }
            Err(diags) => RunOutcome::Problems(diags),
        };
        let flags = jet_jit::jit_trace_flags_for_test();
        let rows = jet_jit::take_last_trace();
        (outcome, flags, rows)
    });
    jet_jit::merge_jit_trace_flags_for_test(flags);
    jet_jit::publish_trace(rows);
    outcome
}

pub fn dev_iteration(file: &str, try_anyway: bool, use_interpreter: bool) -> RunOutcome {
    let trace_tiers = jet_jit::trace_tiers_enabled();
    let (outcome, flags, rows) = jet_driver::run_compiler_work(|| {
        jet_jit::set_trace_tiers(trace_tiers);
        let outcome = match checked_bundle(file) {
            Ok(bundle) => dev_run_bundle(&bundle, try_anyway, use_interpreter),
            Err(diags) => RunOutcome::Problems(diags),
        };
        let flags = jet_jit::jit_trace_flags_for_test();
        let rows = jet_jit::take_last_trace();
        (outcome, flags, rows)
    });
    jet_jit::merge_jit_trace_flags_for_test(flags);
    jet_jit::publish_trace(rows);
    outcome
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
        let src = "use core.tasks as tasks\nfn job() => Int {\n    return 1\n}\nfn run() {\n    h :: tasks.spawn(() => job())\n    print(h.join())\n}\n";
        let b = bundle_from(src, "spawn");
        assert_eq!(detect_dev_mode(&b), DevMode::Resident);
    }

    #[test]
    fn scheduled_tasks_filter_always_skipped_tasks() {
        let src = "#Job(skip: \"disabled\") #Every(5min) fn skipped() {}\n#Job(skip: .Unless(.Platform(.MacOS))) #Every(5min) fn mac_only() {}\n#Job #Every(5min) fn active() {}\nfn run() {}\n";
        let bundle = bundle_from(src, "scheduled_skip");
        let mut names = scheduled_tasks(&bundle)
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        names.sort();
        let mut expected = vec!["active".to_string()];
        if jetpack::Platform::host_key().ends_with("-macos") {
            expected.push("mac_only".to_string());
        }
        assert_eq!(names, expected);
    }

    #[test]
    fn resident_jit_safe_task_examples() {
        // resident_jit_safe_bundle_detail walks large concurrency TIR graphs;
        // default test threads overflow after Epoch 3 JIT ratchet growth.
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                for file in [
                    "examples/features/concurrency/tasks.jet",
                    "examples/features/concurrency/scheduler_spawn.jet",
                ] {
                    let mut bundle =
                        crate::Loader::load_entry(file).unwrap_or_else(|_| panic!("load {file}"));
                    let diags =
                        crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Run);
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
            })
            .expect("spawn resident_jit_safe_task_examples thread")
            .join()
            .expect("resident_jit_safe_task_examples thread panicked");
    }
}
