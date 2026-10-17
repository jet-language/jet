//! E2-M4 / D-EXEC2=A — shared in-process evaluator execution driver.
//!
//! `jet run` and `jet dev` build the program through the normal build path
//! and execute it. The in-process evaluator remains only for the dev
//! surfaces that host a program in process (Canvas); this module drives it.

use std::collections::{BTreeMap, HashMap};

use crate::Diagnostics::Diagnostic;
use crate::AST::{Expr, Func, Item, ProgramBundle, Stmt};
use jet_pkg_model::Package::ReleaseDevtoolsPolicy;
// RunOutcome lives in jet-foundation so the evaluator host crate can share
// it without a dependency cycle.
pub use jet_foundation::JitBackend::RunOutcome;

fn append_parked_task_report(mut outcome: RunOutcome) -> RunOutcome {
    if let RunOutcome::Ran { stderr, .. } = &mut outcome {
        if let Some(report) = crate::scheduler::jet_observe_parked_tasks_report() {
            stderr.push_str(&report.rendered);
        }
    }
    outcome
}

/// The run result plus the non-denied diagnostics produced by the same sema
/// check. Runtime output stays in `RunOutcome`; command front ends render these
/// diagnostics separately so warnings can never enter a program's streams.
pub struct RunWithLints {
    pub outcome: RunOutcome,
    pub lints: Vec<Diagnostic>,
    pub snapshot: Option<crate::CheckedMirSnapshot>,
}

struct CheckedSnapshot {
    snapshot: crate::CheckedMirSnapshot,
    lints: Vec<Diagnostic>,
    ffi_cdylib: Option<std::path::PathBuf>,
}

fn with_ffi_cdylib<R>(path: Option<std::path::PathBuf>, work: impl FnOnce() -> R) -> R {
    jet_evalhost::set_bridge_cdylib(path);
    let result = work();
    jet_evalhost::set_bridge_cdylib(None);
    result
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

/// Run a checked optimized MIR program through the interpreter (E2-M4).
///
/// Front-end checking, policy gates, and the single TIR→MIR lowering happen
/// before this boundary. The evaluator receives only the canonical MIR
/// snapshot; it must not inspect source syntax or re-run sema.
pub fn run_checked(
    program: &jet_foundation::MIR::MirProgram,
    artifact: jet_foundation::MIR::MirArtifactId,
    try_anyway: bool,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> RunOutcome {
    let decision_ledger = jet_foundation::MIROptimization::decision_ledger(
        program,
        Some(artifact),
        "runtime",
        "interpreter:jet dev".to_string(),
        Vec::new(),
    )
    .canonical_json();
    crate::scheduler::jet_observe_runtime_start_with_decision_ledger_from_env(
        Vec::new(),
        Some(decision_ledger),
    );
    let config = mir_eval_config(program, try_anyway, release_devtools_policy);
    // Edition-gated Core kernels read the package edition at runtime; bind the
    // program's edition like AOT (`__JET_PACKAGE_EDITION`) does.
    jet_foundation::PackageEdition::with_package_edition(&program.facts.edition, || {
        append_parked_task_report(mir_eval_outcome(
            crate::Codegen::MIREval::evaluate_mir_program_with_config(program, artifact, &config),
        ))
    })
}

fn mir_eval_config(
    program: &jet_foundation::MIR::MirProgram,
    try_anyway: bool,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> crate::Codegen::MIREval::MirEvalConfig {
    // D-INTERP-BUDGET1: the in-process dev evaluator keeps its step budget.
    let defaults = crate::Codegen::MIREval::MirEvalConfig::default();
    crate::Codegen::MIREval::MirEvalConfig {
        base_dir: std::path::PathBuf::from(&program.facts.project_root),
        runtime_execution: true,
        try_anyway,
        release_devtools_policy: release_devtools_policy.clone(),
        ..defaults
    }
}

fn release_devtools_policy_for_bundle(
    bundle: &ProgramBundle,
    profile: &str,
) -> ReleaseDevtoolsPolicy {
    crate::Driver::release_devtools_policy_for_bundle(bundle, profile)
}

fn mir_eval_outcome(
    result: Result<crate::Codegen::MIREval::MirEvalResult, crate::Codegen::MIREval::MirEvalError>,
) -> RunOutcome {
    match result {
        Ok(result) => RunOutcome::Ran {
            stdout: result.stdout,
            stderr: result.stderr,
            exit_code: result.exit_code,
        },
        Err(error) => RunOutcome::Problems(vec![error.into_diagnostic()]),
    }
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

static SCHEDULED_JOB_CLOCKS: std::sync::LazyLock<
    std::sync::Mutex<HashMap<String, jet_evalhost::Job::JetJobClock>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

/// D-SCHEDULE1: the service/runtime first tick consumes the same checked
/// `EverySchedule` facts as `jet dev`. This adapter only converts the AST
/// carrier to the Prelude carrier; due arithmetic belongs to `jet_job_schedule_due`.
pub fn scheduled_job_names_once(bundle: &ProgramBundle) -> Vec<String> {
    let jobs = scheduled_jobs(bundle);
    let schedules = jobs
        .iter()
        .map(|(name, schedule)| (name.as_str(), prelude_schedule(*schedule)))
        .collect::<Vec<_>>();
    let key = bundle.project_root.to_string_lossy().into_owned();
    let mut clocks = SCHEDULED_JOB_CLOCKS
        .lock()
        .expect("scheduled job clocks poisoned");
    let clock = clocks
        .entry(key)
        .or_insert_with(jet_evalhost::Job::JetJobClock::new);
    jet_evalhost::Job::jet_job_schedule_due(clock, &schedules)
}

fn prelude_schedule(schedule: crate::AST::EverySchedule) -> jet_evalhost::Job::JetJobSchedule {
    match schedule {
        crate::AST::EverySchedule::Duration { nanos } => {
            jet_evalhost::Job::JetJobSchedule::Duration { nanos }
        }
        crate::AST::EverySchedule::WallClockTime { hour, minute } => {
            jet_evalhost::Job::JetJobSchedule::WallClockTime { hour, minute }
        }
    }
}

pub(crate) fn artifact_request_for(
    bundle: &ProgramBundle,
    target: jet_foundation::MIR::MirArtifactTarget,
    profile: &str,
) -> jet_foundation::MIR::MirArtifactRequest {
    let mode =
        crate::Driver::mir_artifact_build_mode_for(bundle, crate::Sema::CompileMode::Run, profile);
    let kind = if target == jet_foundation::MIR::MirArtifactTarget::Web {
        if matches!(
            mode,
            jet_foundation::MIR::MirArtifactBuildMode::Test
                | jet_foundation::MIR::MirArtifactBuildMode::Fuzz
                | jet_foundation::MIR::MirArtifactBuildMode::Coverage
        ) {
            jet_foundation::ice!(
                None,
                "unsupported web MIR artifact build mode for profile `{profile}`"
            );
        }
        jet_foundation::MIR::MirArtifactKind::WebApplication
    } else {
        match mode {
            jet_foundation::MIR::MirArtifactBuildMode::Test
            | jet_foundation::MIR::MirArtifactBuildMode::Coverage => {
                jet_foundation::MIR::MirArtifactKind::TestExecutable
            }
            jet_foundation::MIR::MirArtifactBuildMode::Fuzz => {
                jet_foundation::MIR::MirArtifactKind::FuzzExecutable
            }
            jet_foundation::MIR::MirArtifactBuildMode::Dev
            | jet_foundation::MIR::MirArtifactBuildMode::Release => {
                jet_foundation::MIR::MirArtifactKind::NativeExecutable
            }
        }
    };
    jet_foundation::MIR::MirArtifactRequest::new(target, kind, mode)
}

/// One iteration of the in-process dev loop, factored out so it can be
/// golden-tested without the long-running file watcher. Loads + checks the
/// file exactly like batch compilation (D-DEV: identical diagnostics), then
/// lowers the MIR the evaluator runs.
fn checked_snapshot(
    file: &str,
    gates: jet_foundation::Policy::GateSet,
    job_fn: Option<&str>,
    entry_fn: Option<&str>,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    artifact_target: jet_foundation::MIR::MirArtifactTarget,
) -> Result<CheckedSnapshot, Vec<Diagnostic>> {
    crate::run_compiler_work(|| {
        if let Some(Err(diags)) =
            crate::check_programmable_build_for_tier(file, gates, profile, setting_overrides)
        {
            return Err(diags);
        }
        match crate::Loader::load_entry_with_overlays(file, &[], false) {
            Ok(mut bundle) => {
                if let Err(diags) =
                    crate::Driver::seed_build_facts(&mut bundle, profile, false, setting_overrides)
                {
                    return Err(diags);
                }
                let mut selected_job = false;
                if let Some(job_fn) = job_fn {
                    let specs = job_specs(&bundle);
                    if jet_evalhost::Job::jet_job_has_visible(&specs) {
                        let argv = vec![String::new(), job_fn.to_string()];
                        let selection = jet_evalhost::Job::jet_job_select(&argv, &specs);
                        if matches!(selection, jet_evalhost::Job::JetJobSelection::Job(_)) {
                            jet_driver::Driver::swap_entry_point(&mut bundle, job_fn);
                            selected_job = true;
                        } else if entry_fn.is_none() {
                            return Err(vec![Diagnostic::error(
                                "E1294",
                                jet_evalhost::Job::jet_job_unknown_what(job_fn),
                                jet_evalhost::Job::JET_JOB_UNKNOWN_WHY.to_string(),
                                jet_evalhost::Job::JET_JOB_UNKNOWN_FIX.to_string(),
                                None,
                            )
                            .with_detail(format!(
                                "{}\n",
                                jet_evalhost::Job::jet_job_declared_detail(
                                    &specs.iter().map(|(name, _)| *name).collect::<Vec<_>>()
                                )
                            ))]);
                        }
                    }
                }
                if !selected_job {
                    if let Some(entry_fn) = entry_fn {
                        jet_driver::Driver::swap_entry_point(&mut bundle, entry_fn);
                    }
                }
                let (diags, effect_facts, checked_nominals) = crate::Sema::check_bundle_gates_with_effect_facts(
                    &mut bundle,
                    crate::Sema::CompileMode::Run,
                    gates,
                );
                // Same gate as `jet build` / entry-swap: recoverable parse
                // teaching must not disappear on the default `jet run` path.
                // The canonical extension hook runs before this gate so its
                // findings receive the same project lint policy as sema lints.
                let extension_diags = jet_driver::CompilerExtensionHook::post_sema_diagnostics(
                    &bundle,
                    Some(&effect_facts),
                    &diags,
                );
                let parse_teaching = std::mem::take(&mut bundle.parse_teaching);
                let lints = crate::Driver::gate_diagnostics(
                    &bundle,
                    parse_teaching,
                    diags,
                    extension_diags,
                )?;
                let ffi = if crate::FFI::collect_externs(&bundle).is_empty() {
                    None
                } else {
                    match crate::FFI::prepare(&bundle) {
                        Ok(link) => link,
                        Err(diags) => return Err(diags),
                    }
                };
                let requested_artifact =
                    artifact_request_for(&bundle, artifact_target, profile);
                let (mir, artifact) =
                    crate::lower_checked_semantic_mir_program_for(&bundle, &checked_nominals, requested_artifact);
                Ok(CheckedSnapshot {
                    snapshot: crate::CheckedMirSnapshot {
                        bundle,
                        facts: effect_facts,
                        checked_nominals,
                        mir,
                        artifact,
                    },
                    lints,
                    ffi_cdylib: ffi.map(|link| link.cdylib_path),
                })
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
    match jet_evalhost::Job::jet_job_select(&argv, &specs) {
        jet_evalhost::Job::JetJobSelection::Job(_) => Some(name),
        _ => None,
    }
}

fn job_specs(bundle: &ProgramBundle) -> Vec<(&str, jet_evalhost::Job::JetJobScope)> {
    bundle
        .modules
        .iter()
        .flat_map(|module| module.items.iter())
        .filter_map(|item| match item {
            Item::Func(function) if function.is_job => {
                let scope = match function
                    .job_metadata
                    .as_ref()
                    .map(|metadata| metadata.scope)
                    .unwrap_or_default()
                {
                    jet_foundation::AST::JobScope::Dev => jet_evalhost::Job::JetJobScope::Dev,
                    jet_foundation::AST::JobScope::Ship => jet_evalhost::Job::JetJobScope::Ship,
                    jet_foundation::AST::JobScope::Internal => jet_evalhost::Job::JetJobScope::Internal,
                };
                Some((function.name.as_str(), scope))
            }
            _ => None,
        })
        .collect()
}

/// Run one evaluator invocation on the compiler's sized stack.
///
/// Running a program walks the same deep recursive descent compiling it does,
/// so a run entry needs the same explicit stack as a compile entry, and for
/// the same reason: a caller's thread (`jet`, the dev server, an embedder, a
/// test harness) does not know the compiler's frame budget.
/// `run_compiler_work` reuses an active worker, so nesting these seams still
/// crosses the boundary exactly once. The program argv a caller installed with
/// `with_program_args` is thread-local on the caller's thread, so it is carried
/// across explicitly.
fn on_compiler_stack<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    let argv = crate::Comptime::runtime_argv();
    crate::with_compiler_stack(move || match argv.as_deref() {
        Some(args) => jet_evalhost::with_program_args(args, work),
        None => work(),
    })
}

pub fn dev_iteration(file: &str, try_anyway: bool) -> RunOutcome {
    dev_iteration_with_args_and_gates_profile_and_settings_with_lints_and_entry(
        file,
        &[],
        try_anyway,
        jet_foundation::Policy::GateSet::default(),
        "dev",
        &BTreeMap::new(),
        None,
    )
    .outcome
}

/// Run one dev iteration with the same program argv and named-job selection
/// used by `jet run`. The CLI adapter owns argv marshalling; this shared
/// Prelude-backed path owns selection before the evaluator runs.
pub fn dev_iteration_with_args_and_gates_profile_and_settings_with_lints_and_entry(
    file: &str,
    program_args: &[&str],
    try_anyway: bool,
    gates: jet_foundation::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    entry_fn: Option<&str>,
) -> RunWithLints {
    on_compiler_stack(|| {
        let requested = requested_job(program_args);
        match checked_snapshot(
            file,
            gates,
            requested,
            entry_fn,
            profile,
            setting_overrides,
            jet_foundation::MIR::MirArtifactTarget::Interpreter,
        ) {
            Ok(checked) => {
                let CheckedSnapshot { snapshot, lints, ffi_cdylib } = checked;
                let release_devtools_policy =
                    release_devtools_policy_for_bundle(&snapshot.bundle, profile);
                let selected = selected_job(&snapshot.bundle, requested);
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
                let outcome = jet_evalhost::with_program_args(&args, || {
                    with_ffi_cdylib(ffi_cdylib.clone(), || {
                        dev_run_snapshot(
                            &snapshot.mir,
                            snapshot.artifact,
                            try_anyway,
                            &release_devtools_policy,
                        )
                    })
                });
                RunWithLints {
                    outcome,
                    lints,
                    snapshot: Some(snapshot),
                }
            }
            Err(diagnostics) => RunWithLints {
                outcome: RunOutcome::Problems(diagnostics),
                lints: Vec::new(),
                snapshot: None,
            },
        }
    })
}

/// Run an already-checked optimized MIR program through the evaluator.
pub fn dev_run_snapshot(
    program: &jet_foundation::MIR::MirProgram,
    artifact: jet_foundation::MIR::MirArtifactId,
    try_anyway: bool,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> RunOutcome {
    on_compiler_stack(|| {
        dev_run_snapshot_on_compiler_stack(
            program,
            artifact,
            try_anyway,
            release_devtools_policy,
        )
    })
}

/// Execute one named job from an already-checked snapshot. The name stays in
/// argv so the generated Prelude dispatcher owns graph closure, skip, cache,
/// cwd, limits, and event semantics; callers must not run predecessors first.
pub fn run_checked_job_snapshot(
    snapshot: &crate::CheckedMirSnapshot,
    file: &str,
    name: &str,
    task_args: &[&str],
    try_anyway: bool,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> RunOutcome {
    let mut argv = Vec::with_capacity(task_args.len() + 3);
    argv.push(file.to_string());
    argv.push(jet_evalhost::Job::JET_JOB_PRIVATE_DISPATCH_FLAG.to_string());
    argv.push(name.to_string());
    argv.extend(task_args.iter().map(|arg| (*arg).to_string()));
    append_parked_task_report(jet_evalhost::with_program_args(&argv, || {
        dev_run_snapshot(
            &snapshot.mir,
            snapshot.artifact,
            try_anyway,
            release_devtools_policy,
        )
    }))
}

fn dev_run_snapshot_on_compiler_stack(
    program: &jet_foundation::MIR::MirProgram,
    artifact: jet_foundation::MIR::MirArtifactId,
    try_anyway: bool,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> RunOutcome {
    use crate::JitBackend::{InterpreterBackend, JitBackend};
    let mut backend = InterpreterBackend;
    backend.run(program, artifact, try_anyway, release_devtools_policy)
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
        let src = "fn job() -> Int {\n    return 1\n}\nfn run() {\n    h :: task job()\n    print(h.join() ?? 0)\n}\n";
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
}
