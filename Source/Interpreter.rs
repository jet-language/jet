//! E2-M4 / D-LENS-RUN2 — shared JIT-lens execution driver.
//!
//! Default `jet run` and `jet dev` execute through tiered Cranelift with
//! silent interpreter deopt on named coverage gaps. Explicit
//! `jet run --interpret` and `jet dev --interpret` force tier-0 only.
//! Experts use `--trace-tiers`.

use std::collections::{BTreeMap, HashMap};

use crate::Diagnostics::Diagnostic;
use crate::AST::{Expr, Func, Item, ProgramBundle, Stmt};
use jet_pkg_model::Package::ReleaseDevtoolsPolicy;
// c139: RunOutcome moved to jet-foundation so the jet-jit/ sibling crate
// can implement JitBackend without a dep cycle. Re-exported here so callers
// using `jet::Interpreter::RunOutcome` still work unchanged.
pub use jet_driver::InterpreterBoundary::InterpreterInvocation;
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
/// A resident console boot completed from one checked source closure.
///
/// The lease owns the live JIT runtime and the typed application router.  It
/// must remain alive for the entire attached `ConsoleSession`.
pub struct ConsoleBoot {
    pub lease: jet_jit::ResidentConsoleLease,
    pub lints: Vec<Diagnostic>,
}

struct CheckedSnapshot {
    snapshot: crate::CheckedMirSnapshot,
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

/// Run a checked optimized MIR program through the interpreter (E2-M4).
///
/// Front-end checking, policy gates, and the single TIR→MIR lowering happen
/// before this boundary. The evaluator receives only the canonical MIR
/// snapshot; it must not inspect source syntax or re-run sema.
pub fn run_checked(
    program: &jet_foundation::MIR::MirProgram,
    artifact: jet_foundation::MIR::MirArtifactId,
    try_anyway: bool,
    invocation: InterpreterInvocation,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> RunOutcome {
    let plan = jet_jit::plan_mir_tiers(program, artifact);
    let decision_ledger = jet_foundation::MIROptimization::decision_ledger(
        program,
        Some(artifact),
        "runtime",
        format!("interpreter:{}", invocation.command()),
        plan.decision_ledger_rows(program),
    )
    .canonical_json();
    crate::scheduler::jet_observe_runtime_start_with_decision_ledger_from_env(
        Vec::new(),
        Some(decision_ledger),
    );
    let config = mir_eval_config(program, try_anyway, release_devtools_policy);
    append_parked_task_report(mir_eval_outcome(
        crate::Codegen::MIREval::evaluate_mir_program_with_config(program, artifact, &config),
    ))
}

fn mir_eval_config(
    program: &jet_foundation::MIR::MirProgram,
    try_anyway: bool,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> crate::Codegen::MIREval::MirEvalConfig {
    crate::Codegen::MIREval::MirEvalConfig {
        base_dir: std::path::PathBuf::from(&program.facts.project_root),
        runtime_execution: true,
        try_anyway,
        release_devtools_policy: release_devtools_policy.clone(),
        ..Default::default()
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
    std::sync::Mutex<HashMap<String, jet_jit::Job::JetJobClock>>,
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
        .or_insert_with(jet_jit::Job::JetJobClock::new);
    jet_jit::Job::jet_job_schedule_due(clock, &schedules)
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

/// One iteration of the `jet dev` watch loop, factored out so it can be
/// golden-tested without the long-running file watcher (the outer loop is a
/// thin shell around this). Loads + checks the file exactly like batch
/// compilation (D-DEV: identical diagnostics), then runs via the selected
/// backend.
///
/// `use_interpreter` — D-JIT2=A: when false (default for `jet dev`), the
/// Cranelift tier-1 backend wraps the interpreter; when true (`--interpret`),
/// tier-0 interpreter only.
fn checked_snapshot(
    file: &str,
    gates: jet_foundation::Policy::GateSet,
    setting_overrides: &BTreeMap<String, String>,
    artifact_target: jet_foundation::MIR::MirArtifactTarget,
) -> Result<CheckedSnapshot, Vec<Diagnostic>> {
    checked_snapshot_with_entry(file, gates, None, "dev", setting_overrides, artifact_target)
}

fn checked_snapshot_with_entry(
    file: &str,
    gates: jet_foundation::Policy::GateSet,
    entry_fn: Option<&str>,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    artifact_target: jet_foundation::MIR::MirArtifactTarget,
) -> Result<CheckedSnapshot, Vec<Diagnostic>> {
    checked_snapshot_with_application_authority(
        file,
        gates,
        entry_fn,
        profile,
        setting_overrides,
        None,
        artifact_target,
    )
}

fn checked_snapshot_with_application_authority(
    file: &str,
    gates: jet_foundation::Policy::GateSet,
    entry_fn: Option<&str>,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    application_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
    artifact_target: jet_foundation::MIR::MirArtifactTarget,
) -> Result<CheckedSnapshot, Vec<Diagnostic>> {
    checked_snapshot_with_application_authority_and_entry(
        file,
        gates,
        entry_fn,
        None,
        profile,
        setting_overrides,
        application_authority,
        artifact_target,
    )
}

fn checked_snapshot_with_application_authority_and_entry(
    file: &str,
    gates: jet_foundation::Policy::GateSet,
    job_fn: Option<&str>,
    entry_fn: Option<&str>,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    application_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
    artifact_target: jet_foundation::MIR::MirArtifactTarget,
) -> Result<CheckedSnapshot, Vec<Diagnostic>> {
    checked_snapshot_with_application_authority_and_entry_with_overlays(
        file,
        gates,
        job_fn,
        entry_fn,
        profile,
        setting_overrides,
        application_authority,
        &[],
        artifact_target,
    )
}

fn checked_snapshot_with_application_authority_and_entry_with_overlays(
    file: &str,
    gates: jet_foundation::Policy::GateSet,
    job_fn: Option<&str>,
    entry_fn: Option<&str>,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    application_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
    overlays: &[(&std::path::Path, &str)],
    artifact_target: jet_foundation::MIR::MirArtifactTarget,
) -> Result<CheckedSnapshot, Vec<Diagnostic>> {
    crate::run_compiler_work(|| {
        if overlays.is_empty() {
            if let Some(Err(diags)) =
                crate::check_programmable_build_for_tier(file, gates, profile, setting_overrides)
            {
                return Err(diags);
            }
        }
        crate::RunCache::note_parse();
        match crate::Loader::load_entry_with_overlays(file, overlays, false) {
            Ok(mut bundle) => {
                if let Err(diags) =
                    crate::Driver::seed_build_facts(&mut bundle, profile, false, setting_overrides)
                {
                    return Err(diags);
                }
                let mut selected_job = false;
                if let Some(job_fn) = job_fn {
                    let specs = job_specs(&bundle);
                    if jet_jit::Job::jet_job_has_visible(&specs) {
                        let argv = vec![String::new(), job_fn.to_string()];
                        let selection = jet_jit::Job::jet_job_select(&argv, &specs);
                        if matches!(selection, jet_jit::Job::JetJobSelection::Job(_)) {
                            jet_driver::Driver::swap_entry_point(&mut bundle, job_fn);
                            selected_job = true;
                        } else if entry_fn.is_none() {
                            return Err(vec![Diagnostic::error(
                                "E1294",
                                jet_jit::Job::jet_job_unknown_what(job_fn),
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
                    }
                }
                if !selected_job {
                    if let Some(entry_fn) = entry_fn {
                        jet_driver::Driver::swap_entry_point(&mut bundle, entry_fn);
                    }
                }
                crate::RunCache::note_check();
                let (diags, effect_facts) = crate::Sema::check_bundle_gates_with_effect_facts(
                    &mut bundle,
                    crate::Sema::CompileMode::Run,
                    gates,
                );
                if let Some(application_authority) = application_authority {
                    // The CLI may have made an invocation-local decision. Keep
                    // the fresh sema projection's required row, and replace
                    // only the policy half of the one bundle carrier.
                    let required_effects = bundle
                        .package_guarantees
                        .application_authority
                        .required_effects
                        .clone();
                    let mut applied = application_authority.clone();
                    applied.required_effects = required_effects;
                    bundle.package_guarantees.application_authority = applied;
                }
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
                let (mir, artifact) = crate::lower_checked_semantic_mir_program_for(
                    &bundle,
                    artifact_request_for(&bundle, artifact_target, profile),
                );
                Ok(CheckedSnapshot {
                    snapshot: crate::CheckedMirSnapshot {
                        bundle,
                        facts: effect_facts,
                        mir,
                        artifact,
                    },
                    lints,
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
    match jet_jit::Job::jet_job_select(&argv, &specs) {
        jet_jit::Job::JetJobSelection::Job(_) => Some(name),
        _ => None,
    }
}

fn job_specs(bundle: &ProgramBundle) -> Vec<(&str, jet_jit::Job::JetJobScope)> {
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
    let (outcome, flags, rows) = crate::with_compiler_stack(move || {
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
    jet_jit::record_trace(rows);
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
        run_jit_once_on_compiler_stack(
            file,
            program_args,
            json,
            gates,
            setting_overrides,
            "dev",
            false,
            None,
            None,
        )
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
    run_jit_once_with_args_opts_and_gates_and_settings_with_lints_and_authority(
        file,
        program_args,
        json,
        gates,
        setting_overrides,
        None,
    )
}

/// Run the default JIT with the application decision already resolved by the
/// command boundary. `None` keeps the ordinary package policy carried by the
/// loader; `Some` is the invocation-local once approval.
pub fn run_jit_once_with_args_opts_and_gates_and_settings_with_lints_and_authority(
    file: &str,
    program_args: &[&str],
    json: bool,
    gates: jet_foundation::Policy::GateSet,
    setting_overrides: &BTreeMap<String, String>,
    application_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
) -> RunWithLints {
    run_jit_once_with_args_opts_and_gates_and_settings_with_lints_and_authority_and_entry(
        file,
        program_args,
        json,
        gates,
        setting_overrides,
        application_authority,
        None,
    )
}

pub fn run_jit_once_with_args_opts_and_gates_and_settings_with_lints_and_authority_and_entry(
    file: &str,
    program_args: &[&str],
    json: bool,
    gates: jet_foundation::Policy::GateSet,
    setting_overrides: &BTreeMap<String, String>,
    application_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
    entry_fn: Option<&str>,
) -> RunWithLints {
    on_compiler_stack(|| {
        run_jit_once_on_compiler_stack(
            file,
            program_args,
            json,
            gates,
            setting_overrides,
            "dev",
            true,
            application_authority,
            entry_fn,
        )
    })
}

/// Check and initialize one application for the in-process project console.
///
/// The source closure is the same immutable authority used by `jet run`.
/// Package authority is checked before the resident entry executes, and a
/// supplied invocation policy replaces only its policy half while preserving
/// the sema-required effects.
pub fn boot_console_with_source_closure(
    file: &str,
    source_closure: &[(std::path::PathBuf, String)],
    gates: jet_foundation::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    application_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
    entry_fn: Option<&str>,
) -> Result<ConsoleBoot, Vec<Diagnostic>> {
    let overlays = source_closure
        .iter()
        .map(|(path, source)| (path.as_path(), source.as_str()))
        .collect::<Vec<_>>();
    let CheckedSnapshot { snapshot, lints } =
        checked_snapshot_with_application_authority_and_entry_with_overlays(
            file,
            gates,
            None,
            entry_fn,
            profile,
            setting_overrides,
            application_authority,
            &overlays,
            jet_foundation::MIR::MirArtifactTarget::Cranelift,
        )?;
    let crate::CheckedMirSnapshot {
        bundle,
        mir,
        artifact,
        ..
    } = snapshot;
    let release_devtools_policy = release_devtools_policy_for_bundle(&bundle, profile);
    let authority = bundle.package_guarantees.application_authority;
    if let Some(diagnostic) = authority.policy_diagnostic() {
        return Err(vec![diagnostic]);
    }
    let lease = jet_jit::resident_boot_console(&mir, artifact, &release_devtools_policy).map_err(
        |error| {
            vec![Diagnostic::error(
                "E2105",
                "console application initialization failed".to_string(),
                format!("the resident JIT entry could not stay attached: {error}"),
                "repair the application entry and run the console again".to_string(),
                None,
            )]
        },
    )?;
    Ok(ConsoleBoot { lease, lints })
}

/// Run one JIT program from an authoritative immutable source closure.
///
/// `file` is a diagnostic/module identity only. Every entry and import is
/// resolved from `source_closure`; checked source paths are never reopened.
pub fn run_jit_once_with_source_closure(
    file: &str,
    source_closure: &[(std::path::PathBuf, String)],
    program_args: &[&str],
    json: bool,
    gates: jet_foundation::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    application_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
    entry_fn: Option<&str>,
) -> RunWithLints {
    let overlays = source_closure
        .iter()
        .map(|(path, source)| (path.as_path(), source.as_str()))
        .collect::<Vec<_>>();
    on_compiler_stack(|| {
        run_jit_once_on_compiler_stack_with_overlays(
            file,
            program_args,
            json,
            gates,
            profile,
            setting_overrides,
            true,
            application_authority,
            entry_fn,
            &overlays,
        )
    })
}
/// Run one JIT program from an authoritative entry source snapshot.
pub fn run_jit_once_with_source(
    file: &str,
    source: &str,
    program_args: &[&str],
    json: bool,
    gates: jet_foundation::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    application_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
    entry_fn: Option<&str>,
) -> RunWithLints {
    run_jit_once_with_source_closure(
        file,
        &[(std::path::PathBuf::from(file), source.to_owned())],
        program_args,
        json,
        gates,
        profile,
        setting_overrides,
        application_authority,
        entry_fn,
    )
}
fn run_jit_once_on_compiler_stack(
    file: &str,
    program_args: &[&str],
    json: bool,
    gates: jet_foundation::Policy::GateSet,
    setting_overrides: &BTreeMap<String, String>,
    profile: &str,
    surface_lints: bool,
    application_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
    entry_fn: Option<&str>,
) -> RunWithLints {
    run_jit_once_on_compiler_stack_with_overlays(
        file,
        program_args,
        json,
        gates,
        profile,
        setting_overrides,
        surface_lints,
        application_authority,
        entry_fn,
        &[],
    )
}

fn run_jit_once_on_compiler_stack_with_overlays(
    file: &str,
    program_args: &[&str],
    json: bool,
    gates: jet_foundation::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    surface_lints: bool,
    application_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
    entry_fn: Option<&str>,
    overlays: &[(&std::path::Path, &str)],
) -> RunWithLints {
    crate::RunCache::reset_phases();
    let started = std::time::Instant::now();
    let entry = std::path::Path::new(file);
    if let Some(result) = job_help_if_requested(
        file,
        program_args,
        gates,
        setting_overrides,
        jet_foundation::MIR::MirArtifactTarget::Cranelift,
    ) {
        return result;
    }
    let requested = requested_job(program_args);
    // A cached tier-1 module has the ordinary `run` entry. A named job must
    // pass through entry selection first, so never let a warm artifact skip
    // the shared job selector.
    if overlays.is_empty()
        && application_authority.is_none()
        && entry_fn.is_none()
        && !surface_lints
        && requested.is_none()
        && setting_overrides.is_empty()
        && !matches!(profile, "release" | "hardened")
    {
        let release_devtools_policy = ReleaseDevtoolsPolicy::development();
        if let Some(outcome) =
            crate::RunCache::try_warm_run(entry, program_args, None, None, &release_devtools_policy)
        {
            return RunWithLints {
                outcome,
                lints: Vec::new(),
                snapshot: None,
            };
        }
    }
    match checked_snapshot_with_application_authority_and_entry_with_overlays(
        file,
        gates,
        requested,
        entry_fn,
        profile,
        setting_overrides,
        application_authority,
        overlays,
        jet_foundation::MIR::MirArtifactTarget::Cranelift,
    ) {
        Ok(checked) => {
            let CheckedSnapshot { snapshot, lints } = checked;
            let bundle = &snapshot.bundle;
            let mir = &snapshot.mir;
            let selected = selected_job(bundle, requested);
            let release_devtools_policy = release_devtools_policy_for_bundle(bundle, profile);
            if overlays.is_empty()
                && application_authority.is_none()
                && entry_fn.is_none()
                && surface_lints
                && setting_overrides.is_empty()
            {
                if let Some(outcome) = crate::RunCache::try_warm_run(
                    entry,
                    program_args,
                    selected,
                    Some(snapshot.artifact),
                    &release_devtools_policy,
                ) {
                    return RunWithLints {
                        outcome,
                        lints,
                        snapshot: None,
                    };
                }
            }
            crate::RunCache::note_lower();
            crate::RunCache::note_codegen();
            let runtime_args = if selected.is_some() {
                &program_args[1..]
            } else {
                program_args
            };
            let mut args = Vec::with_capacity(runtime_args.len() + 1);
            args.push(selected.map_or_else(|| file.to_string(), |name| format!("{file} {name}")));
            args.extend(runtime_args.iter().map(|arg| (*arg).to_string()));
            let outcome = jet_jit::with_program_args(&args, || {
                use crate::JitBackend::JitBackend;
                let mut backend = jet_jit::CraneliftBackend::new();
                backend.run(mir, snapshot.artifact, false, &release_devtools_policy)
            });
            if overlays.is_empty()
                && entry_fn.is_none()
                && setting_overrides.is_empty()
                && matches!(outcome, RunOutcome::Ran { .. })
            {
                crate::RunCache::store_after_miss(entry, program_args);
            }
            if !json {
                crate::RunCache::maybe_signpost(started, crate::RunCache::stderr_is_tty());
            }
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
    run_interpreter_once_with_args_and_gates_profile_and_settings_with_lints_and_authority(
        file,
        program_args,
        gates,
        profile,
        setting_overrides,
        None,
    )
}

/// Run the tier-0 interpreter with the application decision already resolved
/// by the command boundary. `Some` carries an invocation-local once approval
/// into the exact checked bundle that the interpreter executes.
pub fn run_interpreter_once_with_args_and_gates_profile_and_settings_with_lints_and_authority(
    file: &str,
    program_args: &[&str],
    gates: jet_foundation::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    application_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
) -> RunWithLints {
    run_interpreter_once_with_args_and_gates_profile_and_settings_with_lints_and_authority_and_entry(
        file,
        program_args,
        gates,
        profile,
        setting_overrides,
        application_authority,
        None,
    )
}

pub fn run_interpreter_once_with_args_and_gates_profile_and_settings_with_lints_and_authority_and_entry(
    file: &str,
    program_args: &[&str],
    gates: jet_foundation::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    application_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
    entry_fn: Option<&str>,
) -> RunWithLints {
    run_interpreter_once_with_source_closure(
        file,
        &[],
        program_args,
        gates,
        profile,
        setting_overrides,
        application_authority,
        entry_fn,
        InterpreterInvocation::RunInterpret,
    )
}

pub fn run_interpreter_once_with_source_closure(
    file: &str,
    source_closure: &[(std::path::PathBuf, String)],
    program_args: &[&str],
    gates: jet_foundation::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    application_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
    entry_fn: Option<&str>,
    invocation: InterpreterInvocation,
) -> RunWithLints {
    crate::RunCache::reset_phases();
    if let Some(result) = job_help_if_requested(
        file,
        program_args,
        gates,
        setting_overrides,
        jet_foundation::MIR::MirArtifactTarget::Interpreter,
    ) {
        return result;
    }
    let overlays = source_closure
        .iter()
        .map(|(path, source)| (path.as_path(), source.as_str()))
        .collect::<Vec<_>>();
    on_compiler_stack(|| {
        let requested = requested_job(program_args);
        match checked_snapshot_with_application_authority_and_entry_with_overlays(
            file,
            gates,
            requested,
            entry_fn,
            profile,
            setting_overrides,
            application_authority,
            &overlays,
            jet_foundation::MIR::MirArtifactTarget::Interpreter,
        ) {
            Ok(checked) => {
                let CheckedSnapshot { snapshot, lints } = checked;
                let bundle = &snapshot.bundle;
                let release_devtools_policy = release_devtools_policy_for_bundle(bundle, profile);
                let selected = selected_job(bundle, requested);
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
                let outcome = jet_jit::with_program_args(&args, || {
                    dev_run_snapshot(
                        &snapshot.mir,
                        snapshot.artifact,
                        false,
                        invocation,
                        &release_devtools_policy,
                    )
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
    dev_iteration_with_args_and_gates_profile_and_settings_with_lints_and_entry(
        file,
        &[],
        try_anyway,
        use_interpreter,
        gates,
        profile,
        setting_overrides,
        None,
    )
}

/// Run one dev iteration with the same program argv and named-job selection
/// used by `jet run`. The CLI adapter owns argv marshalling; this shared
/// Prelude-backed path owns selection before the chosen dev backend runs.
pub fn dev_iteration_with_args_and_gates_profile_and_settings_with_lints_and_entry(
    file: &str,
    program_args: &[&str],
    try_anyway: bool,
    use_interpreter: bool,
    gates: jet_foundation::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    entry_fn: Option<&str>,
) -> RunWithLints {
    on_compiler_stack(|| {
        let requested = requested_job(program_args);
        match checked_snapshot_with_application_authority_and_entry(
            file,
            gates,
            requested,
            entry_fn,
            profile,
            setting_overrides,
            None,
            if use_interpreter {
                jet_foundation::MIR::MirArtifactTarget::Interpreter
            } else {
                jet_foundation::MIR::MirArtifactTarget::Cranelift
            },
        ) {
            Ok(checked) => {
                let CheckedSnapshot { snapshot, lints } = checked;
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
                let invocation = if use_interpreter {
                    InterpreterInvocation::DevInterpret
                } else {
                    InterpreterInvocation::DevDefault
                };
                let outcome = jet_jit::with_program_args(&args, || {
                    dev_run_snapshot(
                        &snapshot.mir,
                        snapshot.artifact,
                        try_anyway,
                        invocation,
                        &release_devtools_policy,
                    )
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

fn job_help_if_requested(
    file: &str,
    program_args: &[&str],
    gates: jet_foundation::Policy::GateSet,
    setting_overrides: &BTreeMap<String, String>,
    artifact_target: jet_foundation::MIR::MirArtifactTarget,
) -> Option<RunWithLints> {
    if program_args.first().copied() != Some("--help") {
        return None;
    }
    match checked_snapshot(file, gates, setting_overrides, artifact_target) {
        Ok(CheckedSnapshot { snapshot, lints }) => {
            let bundle = snapshot.bundle;
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
                    snapshot: None,
                })
            } else {
                None
            }
        }
        Err(diags) => Some(RunWithLints {
            outcome: RunOutcome::Problems(diags),
            lints: Vec::new(),
            snapshot: None,
        }),
    }
}

/// Run an already-checked optimized MIR program through the dev backend seam.
pub fn dev_run_snapshot(
    program: &jet_foundation::MIR::MirProgram,
    artifact: jet_foundation::MIR::MirArtifactId,
    try_anyway: bool,
    invocation: InterpreterInvocation,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> RunOutcome {
    on_compiler_stack(|| {
        dev_run_snapshot_on_compiler_stack(
            program,
            artifact,
            try_anyway,
            invocation,
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
    invocation: InterpreterInvocation,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> RunOutcome {
    let mut argv = Vec::with_capacity(task_args.len() + 3);
    argv.push(file.to_string());
    argv.push(jet_jit::Job::JET_JOB_PRIVATE_DISPATCH_FLAG.to_string());
    argv.push(name.to_string());
    argv.extend(task_args.iter().map(|arg| (*arg).to_string()));
    append_parked_task_report(jet_jit::with_program_args(&argv, || {
        dev_run_snapshot(
            &snapshot.mir,
            snapshot.artifact,
            try_anyway,
            invocation,
            release_devtools_policy,
        )
    }))
}

fn dev_run_snapshot_on_compiler_stack(
    program: &jet_foundation::MIR::MirProgram,
    artifact: jet_foundation::MIR::MirArtifactId,
    try_anyway: bool,
    invocation: InterpreterInvocation,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> RunOutcome {
    use crate::JitBackend::{InterpreterBackend, JitBackend};
    if invocation.uses_interpreter() {
        let mut backend = InterpreterBackend::new(invocation);
        backend.run(program, artifact, try_anyway, release_devtools_policy)
    } else {
        use crate::JitBackend::JitBackend;
        let mut backend = jet_jit::CraneliftBackend::new();
        backend.run(program, artifact, try_anyway, release_devtools_policy)
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
        // resident_jit_safe_program_detail walks large canonical MIR graphs;
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
                    let (mir, _artifact) = crate::lower_checked_semantic_mir_program_for(
                        &bundle,
                        artifact_request_for(
                            &bundle,
                            jet_foundation::MIR::MirArtifactTarget::Cranelift,
                            "dev",
                        ),
                    );
                    let detail = jet_jit::resident_jit_safe_program_detail(&mir);
                    if !detail.is_empty() {
                        eprintln!("{file}: {detail}");
                    }
                    assert!(detail.is_empty(), "{file} must be resident-safe: {detail}");
                }
            })
            .expect("spawn resident_jit_safe_job_examples thread")
            .join()
            .expect("resident_jit_safe_job_examples thread panicked");
    }
    /// The direct harness and the CLI must hand the JIT the same checked
    /// callable surface. Compare before selecting a backend so a tier trace
    /// cannot hide a front-end divergence behind whole-program fallback.
    #[test]
    fn direct_and_cli_bundle_paths_have_identical_lowering_coverage() {
        let src = r#"
fn flatten_words(contents: String) [String] -> {
    return contents.lines().map((line: String) -> line.split(" ").to_list()).flatten()
}
fn first(values: [Float]) Float -> values.first() ?? 0.0
fn run() {
    words :: flatten_words("one two\nthree four")
    values :: [Float]{1.0, 2.0}
    print(words.len())
    print(first(values))
}
"#;
        let file = std::env::temp_dir().join("jet_devmode_bundle_diff.jet");
        std::fs::write(&file, src).unwrap();
        let shown = file.to_string_lossy().into_owned();

        let mut direct = crate::Loader::load_entry(&shown).expect("direct bundle should load");
        let direct_diags = crate::Sema::check_bundle(&mut direct, crate::Sema::CompileMode::Run);
        assert!(
            direct_diags
                .iter()
                .all(|d| !matches!(d.severity, crate::Diagnostics::Severity::Error)),
            "direct bundle diagnostics: {direct_diags:#?}"
        );

        let cli = checked_snapshot_with_application_authority_and_entry(
            &shown,
            jet_foundation::Policy::GateSet::default(),
            None,
            None,
            "dev",
            &BTreeMap::new(),
            None,
            jet_foundation::MIR::MirArtifactTarget::Cranelift,
        )
        .expect("CLI checked snapshot")
        .snapshot;
        let (direct_mir, direct_artifact) = crate::lower_checked_semantic_mir_program_for(
            &direct,
            artifact_request_for(
                &direct,
                jet_foundation::MIR::MirArtifactTarget::Cranelift,
                "dev",
            ),
        );

        fn assert_sequence(label: &str, direct: &[String], cli: &[String]) {
            let shared = direct.len().min(cli.len());
            for index in 0..shared {
                if direct[index] != cli[index] {
                    panic!(
                        "first bundle divergence at {label}[{index}]: direct={:?}, cli={:?}",
                        direct[index], cli[index]
                    );
                }
            }
            assert_eq!(
                direct.len(),
                cli.len(),
                "first bundle divergence at {label}.len(): direct={}, cli={}",
                direct.len(),
                cli.len()
            );
        }

        fn callable_shape(bundle: &ProgramBundle) -> Vec<String> {
            bundle.modules[bundle.entry]
                .items
                .iter()
                .filter_map(|item| {
                    let Item::Func(function) = item else {
                        return None;
                    };
                    let params = function
                        .params
                        .iter()
                        .map(|param| format!("{:?}:{:?}", param.convention, param.ty))
                        .collect::<Vec<_>>()
                        .join(",");
                    Some(format!(
                        "{}({params})->{:?}",
                        function.name, function.return_type
                    ))
                })
                .collect()
        }

        fn lowering_shape(
            program: &jet_foundation::MIR::MirProgram,
            artifact: jet_foundation::MIR::MirArtifactId,
        ) -> (Vec<String>, Vec<String>, bool, Vec<String>) {
            let names = jet_jit::jit_program_func_names(program);
            let plan = jet_jit::plan_mir_tiers(program, artifact);
            let mut native = plan
                .native
                .into_iter()
                .map(|id| format!("{id:?}"))
                .collect::<Vec<_>>();
            native.sort();
            let rows = plan
                .rows
                .iter()
                .map(|row| format!("{:?}:{:?}:{}", row.function, row.tier, row.reason))
                .collect();
            (names, native, plan.whole_program_deopt, rows)
        }

        assert_eq!(
            direct.entry, cli.bundle.entry,
            "first bundle divergence: entry module direct={}, cli={}",
            direct.entry, cli.bundle.entry
        );
        assert_eq!(
            direct.modules.len(),
            cli.bundle.modules.len(),
            "first bundle divergence: module count direct={}, cli={}",
            direct.modules.len(),
            cli.bundle.modules.len()
        );
        let direct_callables = callable_shape(&direct);
        let cli_callables = callable_shape(&cli.bundle);
        assert_sequence("callable shape", &direct_callables, &cli_callables);
        let (direct_names, direct_native, direct_whole, direct_rows) =
            lowering_shape(&direct_mir, direct_artifact);
        let (cli_names, cli_native, cli_whole, cli_rows) = lowering_shape(&cli.mir, cli.artifact);
        assert_sequence("lowered function names", &direct_names, &cli_names);
        assert_sequence("native coverage", &direct_native, &cli_native);
        assert_eq!(
            direct_whole, cli_whole,
            "first bundle divergence: whole-program coverage direct={}, cli={}",
            direct_whole, cli_whole
        );
        assert_sequence("tier rows", &direct_rows, &cli_rows);
    }
}
