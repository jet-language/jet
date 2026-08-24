//! E2-M4 / D-LENS-RUN2 — shared JIT-lens execution driver.
//!
//! Default `jet run` and `jet dev` execute through tiered Cranelift with
//! silent interpreter deopt on named coverage gaps. Explicit
//! `jet run --interpret` and `jet dev --interpret` force tier-0 only.
//! Experts use `--trace-tiers`.

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use crate::Diagnostics::Diagnostic;
use crate::AST::{Expr, Func, Item, ProgramBundle, Stmt};

// c139: RunOutcome moved to jet-foundation so the jet-jit/ sibling crate
// can implement JitBackend without a dep cycle. Re-exported here so callers
// using `jet::Interpreter::RunOutcome` still work unchanged.
pub use jet_foundation::JitBackend::RunOutcome;

/// The run result plus the non-denied diagnostics produced by the same sema
/// check. Runtime output stays in `RunOutcome`; command front ends render these
/// diagnostics separately so warnings can never enter a program's streams.
#[derive(Debug)]
pub struct RunWithLints {
    pub outcome: RunOutcome,
    pub lints: Vec<Diagnostic>,
}

struct CheckedBundle {
    bundle: ProgramBundle,
    lints: Vec<Diagnostic>,
}

/// c77 (D-DEVMODE1=A): how `jet dev` should react to a save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevMode {
    /// A program that finishes on its own — rerun it from scratch on each save.
    RunToCompletion,
    /// A program that stays up (a top-level `loop`, or a canonical `task`) — a
    /// type-stable edit takes the swap path, a type-changing edit announces a
    /// clean restart (D-HOTSWAP1).
    Resident,
}

/// c77 (D-DEVMODE1=A): auto-detect whether `run` runs to completion or stays
/// resident. A `run` whose body contains a top-level `loop { … }` or a
/// a compiler-lowered `task` spawn is `Resident`;
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
/// or any statement whose expression is (or contains, top-level) a compiler-
/// lowered `task` spawn method call.
fn stmt_is_resident(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Loop { .. } => true,
        Stmt::Expr(e) | Stmt::DeferClose { close: e, .. } => expr_has_spawn(e),
        Stmt::Val(b) => expr_has_spawn(&b.init),
        Stmt::Assign { value, .. } => expr_has_spawn(value),
        _ => false,
    }
}

/// True when `e` is, at this level, a compiler-lowered `task` spawn method
/// call. The parser lowers the one-word surface to the compiler-private
/// dispatch name (`Syntax::INTERNAL_TASK_SPAWN_METHOD`) before resident-mode
/// checks, so this reads the constant rather than a user-visible spelling.
fn expr_has_spawn(e: &Expr) -> bool {
    matches!(e, Expr::MethodCall { method, .. } if method == crate::Syntax::INTERNAL_TASK_SPAWN_METHOD)
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
    let started = Instant::now();
    if !try_anyway {
        if let Some(diagnostic) = jet_driver::InterpreterBoundary::dev_boundary_scan(bundle) {
            return RunOutcome::Problems(vec![diagnostic]);
        }
    }
    if let Err(diagnostics) = jet_jit::bind_interpreter_ffi(bundle) {
        return RunOutcome::Problems(diagnostics);
    }
    let (scheduled_stdout, scheduled_stderr) = if bundle_has_service_output(bundle) {
        match run_scheduled_jobs_once(bundle, try_anyway) {
            Ok(output) => output,
            Err(outcome) => return outcome,
        }
    } else {
        (String::new(), String::new())
    };
    let mut sink = crate::Comptime::DevSink::new();
    // Per-run buffer, cleared like the sink: the E3002 journey now drains at the
    // report edge, so a recovered failure must not leak into a later run.
    jet_foundation::Outcome::jet_journey_reset();
    let cap_bytes = match &bundle.program_allocator {
        jet_foundation::TargetMachine::AllocatorPolicy::Counting { cap } => {
            Some(cap.map_or(0, |size| size.bytes))
        }
        _ => None,
    };
    let (outcome, _) = crate::program_allocator::jet_with_host_program_allocator(cap_bytes, || {
        match crate::Comptime::TirBridge::run_bundle(
            bundle,
            &mut sink,
            jet_foundation::Policy::GateSet::allow(jet_foundation::Policy::PolicyKey::Impure),
        ) {
            Ok(crate::Comptime::CtValue::Failed(crate::Comptime::CtReport::Told(error))) => {
                let rendered = error
                    .to_jet_err()
                    .map(|error| jet_foundation::Outcome::jet_render_err(&error))
                    .unwrap_or_else(|| {
                        crate::Comptime::display_core_pure_value(&error)
                            .unwrap_or_else(|| error.jet_show())
                    });
                // Same report edge as AOT's `jet_entry_report` and the resident
                // tier: this error leads and the accumulated E3002 trail follows.
                sink.stderr
                    .push_str(&jet_foundation::Outcome::jet_journey_report(&rendered));
                RunOutcome::Ran {
                    stdout: format!("{scheduled_stdout}{}", sink.stdout),
                    stderr: format!("{scheduled_stderr}{}", sink.stderr),
                    exit_code: 1,
                }
            }
            Ok(_) => RunOutcome::Ran {
                stdout: format!("{scheduled_stdout}{}", sink.stdout),
                stderr: format!("{scheduled_stderr}{}", sink.stderr),
                exit_code: sink.exit_code.unwrap_or(0),
            },
            Err(d) if sink.exit_code.is_some() || d.code == "SOFT_EXIT" => RunOutcome::Ran {
                stdout: format!("{scheduled_stdout}{}", sink.stdout),
                stderr: format!("{scheduled_stderr}{}", sink.stderr),
                exit_code: sink
                    .exit_code
                    .unwrap_or_else(|| d.what.parse().unwrap_or(0)),
            },
            // Whole-program interpret traps are live-program stops. The fallback
            // still enters the Foundation renderer when an older E0953 boundary
            // reaches this adapter.
            Err(d) if d.code == "E0953" => runtime_trap_from_e0953(sink, d),
            Err(d) => RunOutcome::Problems(vec![dev_boundary_from_comptime(d)]),
        }
    });
    if jet_jit::trace_tiers_enabled() && matches!(&outcome, RunOutcome::Ran { .. }) {
        jet_jit::record_trace(vec![jet_jit::TierRow {
            function: "run".to_string(),
            tier: jet_jit::Tier::Interp,
            reason: String::new(),
            millis: started.elapsed().as_secs_f64() * 1000.0,
        }]);
    }
    outcome
}

fn runtime_trap_from_e0953(mut sink: crate::Comptime::DevSink, d: Diagnostic) -> RunOutcome {
    // Same extraction `EvalCtx::route_runtime_panic` performs: a comptime
    // E0953 carries the panic message in `why` behind this prefix, and every
    // other E0953 carries it in `what`. Falling back to `why` here published
    // the registered row's explanation ("a child task panicked") as if it
    // were the program's own panic message.
    let msg = d
        .why
        .strip_prefix("while computing this value at compile time, the program panicked: ")
        .unwrap_or(d.what.as_str());
    let _ = crate::development_receipt::jet_production_failure_receipt_write("E3001", "", 0, "");
    let report =
        jet_foundation::Outcome::jet_render_runtime_stop("E3001", "", 0, "", "", 1, 1, msg, "");
    sink.stderr.push_str(&report.rendered);
    RunOutcome::Ran {
        stdout: sink.stdout,
        stderr: sink.stderr,
        exit_code: 70,
    }
}

/// D-SCHEDULE1 (ratified 2026-07-11, card #505): run one `#Job fn` by name,
/// the same way `run_checked` runs `fn run()` — the `jet dev` consumer
/// (`Source/CmdDevTools.rs`'s due-job tick) calls this to invoke a scheduled
/// job automatically. The caller has already filtered to `Func::is_job`
/// fns pulled from this same checked bundle, so a missing name here is an
/// internal-tooling mismatch, not a source error.
pub fn run_named_job(bundle: &ProgramBundle, name: &str, try_anyway: bool) -> RunOutcome {
    crate::boot_tir_eval();
    if !try_anyway {
        if let Some(diagnostic) = jet_driver::InterpreterBoundary::dev_boundary_scan(bundle) {
            return RunOutcome::Problems(vec![diagnostic]);
        }
    }
    // Jobs share the same TIR program; re-entry is by temporarily selecting
    // the named function as the program entry via a lowered copy.
    let Some(mut program) = crate::Codegen::TIR::lower_interp_program(bundle) else {
        return RunOutcome::Problems(vec![Diagnostic::error(
            "E2201",
            format!("`{name}` isn't a job in this file"),
            "the dev loop's due-job tick looks up the job by name in the checked bundle."
                .to_string(),
            "this is an internal-tooling mismatch, not a source error — please report it."
                .to_string(),
            None,
        )]);
    };
    if !program.funcs.iter().any(|f| f.name == name) {
        return RunOutcome::Problems(vec![Diagnostic::error(
            "E2201",
            format!("`{name}` isn't a job in this file"),
            "the dev loop's due-job tick looks up the job by name in the checked bundle."
                .to_string(),
            "this is an internal-tooling mismatch, not a source error — please report it."
                .to_string(),
            None,
        )]);
    }
    program.entry = name.to_string();
    let mut sink = crate::Comptime::DevSink::new();
    // Same per-run buffer discipline as `run_checked`: the E3002 journey drains
    // at the report edge, so a recovered failure must not leak into this run.
    jet_foundation::Outcome::jet_journey_reset();
    let mut globals = std::collections::HashMap::new();
    for module in &bundle.modules {
        for item in &module.items {
            if let Item::Const(c) = item {
                if let Some(v) = &c.ct {
                    globals.entry(c.name.clone()).or_insert_with(|| v.clone());
                }
            }
        }
    }
    let core_imports = crate::Codegen::core_imports_for_bundle(bundle);
    let cap_bytes = match &bundle.program_allocator {
        jet_foundation::TargetMachine::AllocatorPolicy::Counting { cap } => {
            Some(cap.map_or(0, |size| size.bytes))
        }
        _ => None,
    };
    let (outcome, _) = crate::program_allocator::jet_with_host_program_allocator(cap_bytes, || {
        match crate::Codegen::TIR::run_program_with_structs(
            &program,
            &bundle.project_root,
            &mut sink,
            globals,
            &core_imports,
            jet_foundation::Policy::GateSet::allow(jet_foundation::Policy::PolicyKey::Impure),
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
                    .unwrap_or_else(|| {
                        crate::Comptime::display_core_pure_value(&error)
                            .unwrap_or_else(|| error.jet_show())
                    });
                // D-FAIL-CTX1=A: the fourth entry report edge. A `#Job` entry that
                // lets a `?`-propagated failure escape reports the same journey AOT's
                // `jet_entry_report` and the resident and deopt boundaries report (I9).
                sink.stderr
                    .push_str(&jet_foundation::Outcome::jet_journey_report(&rendered));
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
    });
    outcome
}

/// D-SCHEDULE1: the `#Job`/`#Every(…)` facts the dev loop's due-job tick
/// needs — a job's name and its resolved schedule (`None` for a `#Job fn`
/// with no `#Every(…)`, i.e. manual-invocation-only). Scoped to the entry
/// module's top-level items only (D-JPK-TASKRUN1: a job lives "beside `fn
/// run()`" — the same file, not an imported one). Sema has already rejected
/// a bad `#Every(…)` value (E0926) by the time a bundle reaches `jet dev`.
/// Sema stores the checked schedule on the marker, so this path never
/// re-parses a duration suffix.
pub fn scheduled_jobs(bundle: &ProgramBundle) -> Vec<(String, crate::AST::EverySchedule)> {
    let Some(entry) = bundle.modules.get(bundle.entry) else {
        return Vec::new();
    };
    entry
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Func(f) if f.is_job => {
                if f.job_metadata
                    .as_ref()
                    .and_then(|metadata| {
                        metadata
                            .skip
                            .as_ref()
                            .and_then(|skip| skip.reason_for_host(&jetpack::Platform::host_key()))
                    })
                    .is_some()
                {
                    return None;
                }
                let schedule = f.every.as_ref()?.resolved?;
                Some((f.name.clone(), schedule))
            }
            _ => None,
        })
        .collect()
}

/// D-SCHEDULE1: the service/runtime first tick consumes the same checked
/// `EverySchedule` facts as `jet dev`. This adapter only converts the AST
/// carrier to the Prelude carrier; due arithmetic belongs to `jet_job_schedule_due`.
pub fn scheduled_job_names_once(bundle: &ProgramBundle) -> Vec<String> {
    let jobs = scheduled_jobs(bundle);
    let schedules = jobs
        .iter()
        .map(|(name, schedule)| (name.as_str(), prelude_schedule(*schedule)))
        .collect::<Vec<_>>();
    let mut clock = jet_jit::Job::JetJobClock::new();
    jet_jit::Job::jet_job_schedule_due(&mut clock, &schedules)
}

fn bundle_has_service_output(bundle: &ProgramBundle) -> bool {
    bundle.modules.get(bundle.entry).is_some_and(|module| {
        module.items.iter().any(|item| {
            matches!(
                item,
                Item::Const(value)
                    if value
                        .resolved_output
                        .as_ref()
                        .is_some_and(|output| {
                            output.selected
                                && output.kind == crate::AST::OutputKind::Service
                        })
            )
        })
    })
}

fn prelude_schedule(schedule: crate::AST::EverySchedule) -> jet_jit::Job::JetJobSchedule {
    match schedule {
        crate::AST::EverySchedule::Duration { nanos } => {
            jet_jit::Job::JetJobSchedule::Duration { nanos }
        }
        crate::AST::EverySchedule::WallClockTime { hour, minute } => {
            jet_jit::Job::JetJobSchedule::WallClockTime { hour, minute }
        }
    }
}

fn run_scheduled_jobs_once(
    bundle: &ProgramBundle,
    try_anyway: bool,
) -> Result<(String, String), RunOutcome> {
    let mut stdout = String::new();
    let mut stderr = String::new();
    for name in scheduled_job_names_once(bundle) {
        match run_named_job(bundle, &name, try_anyway) {
            RunOutcome::Ran {
                stdout: job_stdout,
                stderr: job_stderr,
                ..
            } => {
                stdout.push_str(&job_stdout);
                stderr.push_str(&job_stderr);
            }
            problems @ RunOutcome::Problems(_) => return Err(problems),
        }
    }
    Ok((stdout, stderr))
}

/// c139 JIT-parity fix (2026-07-03): the dev interpreter IS the comptime
/// tree-walker (see module doc), so a construct it can't run leaks the
/// comptime evaluator's own E0956 ("unsupported")/E3401 ("impurity",
/// D-META-EFFECT1 c3 — the retired E0951 redirects here; a genuine run-time
/// E3401 is a sema-time diagnostic that fails the build before this ever
/// runs, so any E3401 seen here is always the shared evaluator's own gate) /
/// E3410/E3412 (Tier-2 / live-net comptime) codes — correct for a real
/// `$ { }` block, but wrong voice here: the "compute this at runtime"
/// / "only fetch at comptime" fix advice is nonsense when the user is already
/// trying to run this at runtime via `jet dev`. Rewrap as the dev-loop's own
/// E2201 boundary diagnostic instead, preserving what construct tripped it.
fn dev_boundary_from_comptime(d: Diagnostic) -> Diagnostic {
    // Read the CONSTRUCT, never the sentence. Every one of these rows renders the
    // construct as its first backtick-quoted slot, so lifting that slot survives a
    // reword of the surrounding prose. A suffix strip does not: 071ef45dd reworded
    // E0956 from "… can't run at compile time yet" to "… isn't supported by the
    // current evaluator yet", migrated four consumers, and skipped this one -- so the
    // strip silently stopped matching and the whole clause got spliced into
    // "it uses <sentence>, which isn't covered …", three "yet"s in one report.
    let quoted = |what: &str| -> Option<String> {
        let rest = what.split_once('`')?.1;
        let (inner, _) = rest.split_once('`')?;
        (!inner.is_empty()).then(|| inner.to_string())
    };
    let construct = match d.code.as_str() {
        "E0956" | "E3412" | "E3410" => quoted(&d.what),
        // E3401 names no single construct: it refuses a whole capability class, so
        // the noun phrase is ours to supply rather than lift.
        "E3401" => Some(
            "code that touches the outside world (network, filesystem, or environment)".to_string(),
        ),
        _ => return d,
    };
    jet_driver::InterpreterBoundary::dev_boundary_for_refusal(construct.as_deref(), &d.what, d.span)
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
fn checked_bundle(
    file: &str,
    gates: jet_foundation::Policy::GateSet,
    setting_overrides: &BTreeMap<String, String>,
) -> Result<CheckedBundle, Vec<Diagnostic>> {
    checked_bundle_with_entry(file, gates, None, "dev", setting_overrides)
}

fn checked_bundle_with_entry(
    file: &str,
    gates: jet_foundation::Policy::GateSet,
    entry_fn: Option<&str>,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> Result<CheckedBundle, Vec<Diagnostic>> {
    jet_driver::run_compiler_work(|| {
        if let Some(Err(diags)) =
            crate::check_programmable_build_for_tier(file, gates, profile, setting_overrides)
        {
            return Err(diags);
        }
        crate::RunCache::note_parse();
        match crate::Loader::load_entry_with_overlay(file, None, false) {
            Ok(mut bundle) => {
                if let Err(diags) =
                    crate::Driver::seed_build_facts(&mut bundle, profile, false, setting_overrides)
                {
                    return Err(diags);
                }
                if let Some(entry_fn) = entry_fn {
                    let specs = job_specs(&bundle);
                    if jet_jit::Job::jet_job_has_visible(&specs) {
                        let argv = vec![String::new(), entry_fn.to_string()];
                        let selection = jet_jit::Job::jet_job_select(&argv, &specs);
                        if !matches!(selection, jet_jit::Job::JetJobSelection::Job(_)) {
                            return Err(vec![Diagnostic::error(
                                "E1294",
                                jet_jit::Job::jet_job_unknown_what(entry_fn),
                                jet_jit::Job::JET_JOB_UNKNOWN_WHY.to_string(),
                                jet_jit::Job::JET_JOB_UNKNOWN_FIX.to_string(),
                                None,
                            )
                            .with_detail(format!(
                                "{}\n",
                                jet_jit::Job::jet_job_declared_detail(
                                    &specs.iter().map(|(name, _)| *name).collect::<Vec<_>>()
                                )
                            ))]);
                        }
                        jet_driver::Driver::swap_entry_point(&mut bundle, entry_fn);
                    }
                }
                crate::RunCache::note_check();
                let diags = crate::Sema::check_bundle_gates(
                    &mut bundle,
                    crate::Sema::CompileMode::Run,
                    gates,
                );
                // Same gate as `jet build` / entry-swap: recoverable parse
                // teaching must not disappear on the default `jet run` path.
                // The canonical extension hook runs before this gate so its
                // findings receive the same project lint policy as sema lints.
                let extension_diags =
                    jet_driver::CompilerExtensionHook::post_sema_diagnostics(&bundle, None, &diags);
                let parse_teaching = std::mem::take(&mut bundle.parse_teaching);
                let lints = crate::Driver::gate_diagnostics(
                    &bundle,
                    parse_teaching,
                    diags,
                    extension_diags,
                )?;
                Ok(CheckedBundle { bundle, lints })
            }
            Err(diags) => Err(diags),
        }
    })
}

fn requested_job<'a>(program_args: &[&'a str]) -> Option<&'a str> {
    program_args
        .first()
        .copied()
        .filter(|arg| !arg.starts_with('-'))
}

fn selected_job<'a>(bundle: &ProgramBundle, requested: Option<&'a str>) -> Option<&'a str> {
    let name = requested?;
    let specs = job_specs(bundle);
    let argv = vec![String::new(), name.to_string()];
    match jet_jit::Job::jet_job_select(&argv, &specs) {
        jet_jit::Job::JetJobSelection::Job(_) => Some(name),
        _ => None,
    }
}

fn job_specs(bundle: &ProgramBundle) -> Vec<(&str, jet_jit::Job::JetJobScope)> {
    bundle.modules[bundle.entry]
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Func(function) if function.is_job => {
                let scope = match function
                    .job_metadata
                    .as_ref()
                    .map(|metadata| metadata.scope)
                    .unwrap_or_default()
                {
                    jet_foundation::AST::JobScope::Dev => jet_jit::Job::JetJobScope::Dev,
                    jet_foundation::AST::JobScope::Ship => jet_jit::Job::JetJobScope::Ship,
                    jet_foundation::AST::JobScope::Internal => jet_jit::Job::JetJobScope::Internal,
                };
                Some((function.name.as_str(), scope))
            }
            _ => None,
        })
        .collect()
}

/// Run one execution tier on the compiler's sized stack.
///
/// Running a program walks the same deep recursive descent compiling it does —
/// TIR lowering for tier 0, Cranelift lowering for tier 1 — so a run entry
/// needs the same explicit stack as a compile entry, and for the same reason:
/// a caller's thread (`jet`, the dev server, an embedder, a test harness) does
/// not know the compiler's frame budget. `run_compiler_work` reuses an active
/// worker, so nesting these seams still crosses the boundary exactly once.
///
/// Three pieces of run state are thread-local on the caller's thread, so they
/// are carried across explicitly: the program argv a caller installed with
/// `with_program_args`, the tier-trace toggle read while lowering, and the
/// tier flags plus trace rows a caller reads after the run. Everything else a
/// run touches — resident module, JIT runtime, memory sentries, journey
/// frames, `#Persist` seeding — is established and consumed inside `work`.
fn on_compiler_stack<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    let trace_tiers = jet_jit::trace_tiers_enabled();
    let argv = crate::Comptime::runtime_argv();
    let (outcome, flags, rows) = jet_driver::run_compiler_work(move || {
        jet_jit::set_trace_tiers(trace_tiers);
        let outcome = match argv.as_deref() {
            Some(args) => jet_jit::with_program_args(args, work),
            None => work(),
        };
        (
            outcome,
            jet_jit::jit_trace_flags_for_test(),
            jet_jit::take_last_trace(),
        )
    });
    jet_jit::merge_jit_trace_flags_for_test(flags);
    jet_jit::publish_trace(rows);
    outcome
}

/// D-LENS-RUN1: load, check, and execute one native program through strict JIT.
pub fn run_jit_once(file: &str) -> RunOutcome {
    run_jit_once_with_args_and_settings(file, &[], &BTreeMap::new())
}

/// D-LENS-RUN1: strict Cranelift run with the same argv shape AOT would see.
pub fn run_jit_once_with_args(file: &str, program_args: &[&str]) -> RunOutcome {
    run_jit_once_with_args_and_settings(file, program_args, &BTreeMap::new())
}

/// Like [`run_jit_once_with_args`], with `json` suppressing the jet-dev signpost.
pub fn run_jit_once_with_args_opts(file: &str, program_args: &[&str], json: bool) -> RunOutcome {
    run_jit_once_with_args_opts_and_gates(
        file,
        program_args,
        json,
        jet_foundation::Policy::GateSet::default(),
    )
}

pub fn run_jit_once_with_args_opts_and_gates(
    file: &str,
    program_args: &[&str],
    json: bool,
    gates: jet_foundation::Policy::GateSet,
) -> RunOutcome {
    run_jit_once_with_args_opts_and_gates_and_settings(
        file,
        program_args,
        json,
        gates,
        &BTreeMap::new(),
    )
}

pub fn run_jit_once_with_args_and_settings(
    file: &str,
    program_args: &[&str],
    setting_overrides: &BTreeMap<String, String>,
) -> RunOutcome {
    run_jit_once_with_args_opts_and_gates_and_settings(
        file,
        program_args,
        false,
        jet_foundation::Policy::GateSet::default(),
        setting_overrides,
    )
}

pub fn run_jit_once_with_args_opts_and_gates_and_settings(
    file: &str,
    program_args: &[&str],
    json: bool,
    gates: jet_foundation::Policy::GateSet,
    setting_overrides: &BTreeMap<String, String>,
) -> RunOutcome {
    // The whole invocation stays on one worker: the warm-artifact probe, the
    // front end, the JIT run, and the tier-artifact store that reads what the
    // run just published all share the same thread-local run state.
    on_compiler_stack(|| {
        run_jit_once_on_compiler_stack(file, program_args, json, gates, setting_overrides, false)
    })
    .outcome
}

/// Run through the strict Cranelift tier and return the non-denied diagnostics
/// from the same front-end check. CLI command paths use this to render lints
/// without putting them into the program-owned output streams.
pub fn run_jit_once_with_args_opts_and_gates_and_settings_with_lints(
    file: &str,
    program_args: &[&str],
    json: bool,
    gates: jet_foundation::Policy::GateSet,
    setting_overrides: &BTreeMap<String, String>,
) -> RunWithLints {
    on_compiler_stack(|| {
        run_jit_once_on_compiler_stack(file, program_args, json, gates, setting_overrides, true)
    })
}

fn run_jit_once_on_compiler_stack(
    file: &str,
    program_args: &[&str],
    json: bool,
    gates: jet_foundation::Policy::GateSet,
    setting_overrides: &BTreeMap<String, String>,
    surface_lints: bool,
) -> RunWithLints {
    crate::RunCache::reset_phases();
    let started = std::time::Instant::now();
    let timing = crate::PhaseTiming::enabled();
    let mut timer = crate::PhaseTiming::PhaseTimer::new();
    let entry = std::path::Path::new(file);
    if let Some(result) = job_help_if_requested(file, program_args, gates, setting_overrides) {
        return result;
    }
    let requested = requested_job(program_args);
    // A cached tier-1 module has the ordinary `run` entry. A named job must
    // pass through entry selection first, so never let a warm artifact skip
    // the shared job selector.
    if !surface_lints && requested.is_none() && setting_overrides.is_empty() {
        if let Some(outcome) = crate::RunCache::try_warm_run(entry, program_args) {
            if timing {
                timer.metric("cache_hit", 1);
                timer.lap("jit_cache_hit");
                write_jit_timing(&timer);
            }
            return RunWithLints {
                outcome,
                lints: Vec::new(),
            };
        }
    }
    match checked_bundle_with_entry(file, gates, requested, "dev", setting_overrides) {
        Ok(checked) => {
            if timing {
                timer.lap("frontend");
            }
            let lints = checked.lints;
            let bundle = checked.bundle;
            if surface_lints && requested.is_none() && setting_overrides.is_empty() {
                if let Some(outcome) = crate::RunCache::try_warm_run(entry, program_args) {
                    if timing {
                        timer.metric("cache_hit", 1);
                        timer.lap("jit_cache_hit");
                        write_jit_timing(&timer);
                    }
                    return RunWithLints { outcome, lints };
                }
            }
            crate::RunCache::note_lower();
            crate::RunCache::note_codegen();
            let selected = selected_job(&bundle, requested);
            let runtime_args = if selected.is_some() {
                &program_args[1..]
            } else {
                program_args
            };
            let mut args = Vec::with_capacity(runtime_args.len() + 1);
            args.push(selected.map_or_else(|| file.to_string(), |name| format!("{file} {name}")));
            args.extend(runtime_args.iter().map(|arg| (*arg).to_string()));
            let mut scheduled_stdout = String::new();
            let mut scheduled_stderr = String::new();
            if selected.is_none() && bundle_has_service_output(&bundle) {
                for name in scheduled_job_names_once(&bundle) {
                    let job_bundle = match checked_bundle_with_entry(
                        file,
                        gates,
                        Some(&name),
                        "dev",
                        setting_overrides,
                    ) {
                        Ok(job_bundle) => job_bundle.bundle,
                        Err(diags) => {
                            return RunWithLints {
                                outcome: RunOutcome::Problems(diags),
                                lints,
                            }
                        }
                    };
                    let job_args = vec![format!("{file} {name}")];
                    let job_outcome = jet_jit::with_program_args(&job_args, || {
                        use crate::JitBackend::JitBackend;
                        let mut backend = jet_jit::CraneliftBackend::new();
                        backend.run(&job_bundle, false)
                    });
                    match job_outcome {
                        RunOutcome::Ran {
                            stdout,
                            stderr,
                            exit_code,
                        } => {
                            scheduled_stdout.push_str(&stdout);
                            scheduled_stderr.push_str(&stderr);
                            if exit_code != 0 {
                                return RunWithLints {
                                    outcome: RunOutcome::Ran {
                                        stdout: scheduled_stdout,
                                        stderr: scheduled_stderr,
                                        exit_code,
                                    },
                                    lints,
                                };
                            }
                        }
                        RunOutcome::Problems(diags) => {
                            return RunWithLints {
                                outcome: RunOutcome::Problems(diags),
                                lints,
                            }
                        }
                    }
                }
            }
            let outcome = jet_jit::with_program_args(&args, || {
                use crate::JitBackend::JitBackend;
                let mut backend = jet_jit::CraneliftBackend::new();
                backend.run(&bundle, false)
            });
            let outcome = match outcome {
                RunOutcome::Ran {
                    stdout,
                    stderr,
                    exit_code,
                } => RunOutcome::Ran {
                    stdout: format!("{scheduled_stdout}{stdout}"),
                    stderr: format!("{scheduled_stderr}{stderr}"),
                    exit_code,
                },
                RunOutcome::Problems(diags) => RunOutcome::Problems(diags),
            };
            if timing {
                timer.lap("jit");
            }
            if selected.is_none()
                && setting_overrides.is_empty()
                && matches!(outcome, RunOutcome::Ran { .. })
            {
                crate::RunCache::store_after_miss(entry, program_args);
            }
            if !json {
                crate::RunCache::maybe_signpost(started, crate::RunCache::stderr_is_tty());
            }
            if timing {
                write_jit_timing(&timer);
            }
            RunWithLints { outcome, lints }
        }
        Err(diags) => {
            if timing {
                timer.lap("frontend");
                write_jit_timing(&timer);
            }
            RunWithLints {
                outcome: RunOutcome::Problems(diags),
                lints: Vec::new(),
            }
        }
    }
}

fn write_jit_timing(timer: &crate::PhaseTiming::PhaseTimer) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    timer.write_to(&cwd);
}

/// Run one program through the tier-0 interpreter with the same argv shape as
/// the default run path.
pub fn run_interpreter_once_with_args(file: &str, program_args: &[&str]) -> RunOutcome {
    run_interpreter_once_with_args_and_settings(
        file,
        program_args,
        jet_foundation::Policy::GateSet::default(),
        &BTreeMap::new(),
    )
}

pub fn run_interpreter_once_with_args_and_gates(
    file: &str,
    program_args: &[&str],
    gates: jet_foundation::Policy::GateSet,
) -> RunOutcome {
    run_interpreter_once_with_args_and_gates_profile_and_settings(
        file,
        program_args,
        gates,
        "dev",
        &BTreeMap::new(),
    )
}

pub fn run_interpreter_once_with_args_and_gates_profile(
    file: &str,
    program_args: &[&str],
    gates: jet_foundation::Policy::GateSet,
    profile: &str,
) -> RunOutcome {
    run_interpreter_once_with_args_and_gates_profile_and_settings(
        file,
        program_args,
        gates,
        profile,
        &BTreeMap::new(),
    )
}

pub fn run_interpreter_once_with_args_and_settings(
    file: &str,
    program_args: &[&str],
    gates: jet_foundation::Policy::GateSet,
    setting_overrides: &BTreeMap<String, String>,
) -> RunOutcome {
    run_interpreter_once_with_args_and_gates_profile_and_settings(
        file,
        program_args,
        gates,
        "dev",
        setting_overrides,
    )
}

pub fn run_interpreter_once_with_args_and_gates_profile_and_settings(
    file: &str,
    program_args: &[&str],
    gates: jet_foundation::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> RunOutcome {
    run_interpreter_once_with_args_and_gates_profile_and_settings_with_lints(
        file,
        program_args,
        gates,
        profile,
        setting_overrides,
    )
    .outcome
}

pub fn run_interpreter_once_with_args_and_gates_profile_and_settings_with_lints(
    file: &str,
    program_args: &[&str],
    gates: jet_foundation::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> RunWithLints {
    crate::RunCache::reset_phases();
    if let Some(result) = job_help_if_requested(file, program_args, gates, setting_overrides) {
        return result;
    }
    on_compiler_stack(|| {
        let requested = requested_job(program_args);
        match checked_bundle_with_entry(file, gates, requested, profile, setting_overrides) {
            Ok(checked) => {
                let lints = checked.lints;
                let bundle = checked.bundle;
                let selected = selected_job(&bundle, requested);
                let runtime_args = if selected.is_some() {
                    &program_args[1..]
                } else {
                    program_args
                };
                let mut args = Vec::with_capacity(runtime_args.len() + 1);
                args.push(
                    selected.map_or_else(|| file.to_string(), |name| format!("{file} {name}")),
                );
                args.extend(runtime_args.iter().map(|arg| (*arg).to_string()));
                RunWithLints {
                    outcome: jet_jit::with_program_args(&args, || {
                        dev_run_bundle(&bundle, false, true)
                    }),
                    lints,
                }
            }
            Err(diags) => RunWithLints {
                outcome: RunOutcome::Problems(diags),
                lints: Vec::new(),
            },
        }
    })
}

pub fn dev_iteration(file: &str, try_anyway: bool, use_interpreter: bool) -> RunOutcome {
    dev_iteration_with_gates(
        file,
        try_anyway,
        use_interpreter,
        jet_foundation::Policy::GateSet::default(),
    )
}

pub fn dev_iteration_with_gates(
    file: &str,
    try_anyway: bool,
    use_interpreter: bool,
    gates: jet_foundation::Policy::GateSet,
) -> RunOutcome {
    dev_iteration_with_gates_profile_and_settings(
        file,
        try_anyway,
        use_interpreter,
        gates,
        "dev",
        &BTreeMap::new(),
    )
}

pub fn dev_iteration_with_gates_profile(
    file: &str,
    try_anyway: bool,
    use_interpreter: bool,
    gates: jet_foundation::Policy::GateSet,
    profile: &str,
) -> RunOutcome {
    dev_iteration_with_gates_profile_and_settings(
        file,
        try_anyway,
        use_interpreter,
        gates,
        profile,
        &BTreeMap::new(),
    )
}

pub fn dev_iteration_with_gates_and_settings(
    file: &str,
    try_anyway: bool,
    use_interpreter: bool,
    gates: jet_foundation::Policy::GateSet,
    setting_overrides: &BTreeMap<String, String>,
) -> RunOutcome {
    dev_iteration_with_gates_profile_and_settings(
        file,
        try_anyway,
        use_interpreter,
        gates,
        "dev",
        setting_overrides,
    )
}

pub fn dev_iteration_with_gates_profile_and_settings(
    file: &str,
    try_anyway: bool,
    use_interpreter: bool,
    gates: jet_foundation::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> RunOutcome {
    dev_iteration_with_gates_profile_and_settings_with_lints(
        file,
        try_anyway,
        use_interpreter,
        gates,
        profile,
        setting_overrides,
    )
    .outcome
}

pub fn dev_iteration_with_gates_profile_and_settings_with_lints(
    file: &str,
    try_anyway: bool,
    use_interpreter: bool,
    gates: jet_foundation::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> RunWithLints {
    on_compiler_stack(|| {
        match checked_bundle_with_entry(file, gates, None, profile, setting_overrides) {
            Ok(checked) => RunWithLints {
                outcome: dev_run_bundle(&checked.bundle, try_anyway, use_interpreter),
                lints: checked.lints,
            },
            Err(diags) => RunWithLints {
                outcome: RunOutcome::Problems(diags),
                lints: Vec::new(),
            },
        }
    })
}

fn job_help_if_requested(
    file: &str,
    program_args: &[&str],
    gates: jet_foundation::Policy::GateSet,
    setting_overrides: &BTreeMap<String, String>,
) -> Option<RunWithLints> {
    if program_args.first().copied() != Some("--help") {
        return None;
    }
    match checked_bundle(file, gates, setting_overrides) {
        Ok(checked) => {
            let lints = checked.lints;
            let bundle = checked.bundle;
            let specs = job_specs(&bundle);
            if !jet_jit::Job::jet_job_has_visible(&specs) {
                return None;
            }
            let argv = vec![file.to_string(), "--help".to_string()];
            if matches!(
                jet_jit::Job::jet_job_select(&argv, &specs),
                jet_jit::Job::JetJobSelection::Help
            ) {
                Some(RunWithLints {
                    outcome: RunOutcome::Ran {
                        stdout: jet_jit::Job::jet_job_help_text(&argv, &specs),
                        stderr: String::new(),
                        exit_code: 0,
                    },
                    lints,
                })
            } else {
                None
            }
        }
        Err(diags) => Some(RunWithLints {
            outcome: RunOutcome::Problems(diags),
            lints: Vec::new(),
        }),
    }
}

/// Run an already-checked bundle through the dev backend seam.
pub fn dev_run_bundle(
    bundle: &ProgramBundle,
    try_anyway: bool,
    use_interpreter: bool,
) -> RunOutcome {
    on_compiler_stack(|| dev_run_bundle_on_compiler_stack(bundle, try_anyway, use_interpreter))
}

fn dev_run_bundle_on_compiler_stack(
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
        let src = "fn job() Int {\n    return 1\n}\nfn run() {\n    h :: task job()\n    print(h.join() ?? 0)\n}\n";
        let b = bundle_from(src, "spawn");
        assert_eq!(detect_dev_mode(&b), DevMode::Resident);
    }

    #[test]
    fn scheduled_jobs_filter_always_skipped_jobs() {
        let src = "#Job(.Dev, skip: \"disabled\") #Every(5min) fn skipped() {}\n#Job(.Dev, skip: .Unless(.Platform(.MacOS))) #Every(5min) fn mac_only() {}\n#Job #Every(5min) fn active() {}\nfn run() {}\n";
        let mut bundle = bundle_from(src, "scheduled_skip");
        // D-SCHEDULE1: sema resolves `#Every(…)` once and writes the schedule
        // onto the marker (`CheckerSchedule::check_every_marker`); `jet dev`
        // only ever asks a checked bundle, so the fixture checks it the same
        // way — on the compiler's sized stack, not a 2 MiB libtest thread.
        let _ = crate::run_compiler_work(|| {
            crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Run)
        });
        let mut names = scheduled_jobs(&bundle)
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
    fn resident_jit_safe_job_examples() {
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
            .expect("spawn resident_jit_safe_job_examples thread")
            .join()
            .expect("resident_jit_safe_job_examples thread panicked");
    }
}
