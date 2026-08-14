//! dev / repl / doctor / explain / completions / bind / eval / emit / bench
//! developer-tooling subcommand handlers.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

use jet::Diagnostics::ColorChoice;
use jet::ExitCodes;
use jet_foundation::JSON::json_escape;

use crate::CmdCompile::{build, collect_source_files_recursive, stem};
use crate::{report_problems, BuildProfile, OutputMode};
pub(crate) use jet_devserver::{watch_policy_from, WatchPolicy};

/// Preserve the Prelude's stream order when a run outcome crosses the CLI
/// adapter as separate stdout/stderr buffers.
fn emit_run_output(stdout: &str, stderr: &str) {
    print!("{stdout}");
    if !stderr.is_empty() {
        let _ = std::io::stdout().flush();
        eprint!("{stderr}");
    }
}

/// `jet dev <file>` — the E2-M4 watch/interpret loop (D-DEV4), extended by c77
/// with three-mode routing (D-DEVMODE1=A) and hot-swap/restart (D-HOTSWAP1=B).
/// Re-checks and re-runs on dependency-aware invalidation (#439 / E3-UL6),
/// streaming output. The per-iteration work lives in
/// `jet::Interpreter::dev_iteration` (so it can be golden-tested); this is the
/// thin std-only watcher around the shared `WatchSession` engine (I6: no
/// `notify` crate).
pub(crate) fn run_dev(
    file: &str,
    try_anyway: bool,
    policy: WatchPolicy,
    gates: jet::Policy::GateSet,
    mode: OutputMode,
    use_interpreter: bool,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) {
    let path = Path::new(file);
    if !path.exists() {
        crate::cli_error!(@fix "E2105", format!("can't find the file `{}`", file), format!("check the spelling, or run {} from the folder that contains it", jet::Syntax::BINARY_NAME));
        exit(ExitCodes::USER_ERROR);
    }

    // `--watch=off`: run once and exit (no loop).
    if policy == WatchPolicy::Once {
        let outcome = jet::Interpreter::dev_iteration_with_gates_profile_and_settings(
            file,
            try_anyway,
            use_interpreter,
            gates,
            profile,
            setting_overrides,
        );
        render_dev_outcome(&outcome, file, mode);
        exit_dev_outcome(outcome);
    }

    if !mode.quiet {
        println!("watching {} … (Ctrl-C to stop)", file);
    }

    // The bundle from the last successful load, kept so a resident edit can be
    // diffed against it for type stability (D-HOTSWAP1).
    let mut prev_bundle = render_dev_iteration(
        file,
        try_anyway,
        gates,
        mode,
        use_interpreter,
        profile,
        setting_overrides,
    );
    // #439 / E3-UL6: dependency-aware watch session shared with `jet run --watch`.
    let mut watch = match jet_devserver::WatchSession::open(path) {
        Ok(watch) => watch,
        Err(diagnostic) => {
            eprint!(
                "{}",
                jet::render_all_colored(file, "", &[diagnostic], mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    };
    // D-SCHEDULE1 (card #505): due `#Job #Every(…)` fns fire on their own
    // schedule, independent of file-change ticks.
    let mut clock = TaskClock::new();
    let mut persist = jet_devserver::PersistStore::new();
    let mut session = jet_devserver::SessionSnapshot {
        generation: 0,
        artifact_token: "gen-0".into(),
        persist: persist.clone(),
    };

    loop {
        std::thread::sleep(std::time::Duration::from_millis(120));
        if let Some(bundle) = &prev_bundle {
            run_due_tasks(bundle, file, try_anyway, mode, &mut clock);
            sync_persist_bindings(bundle, &mut persist);
        }
        if let Some(receipt) = watch.poll() {
            if receipt.change_kinds.iter().all(|k| *k == "stale") {
                continue;
            }
            let next = render_dev_change(
                file,
                try_anyway,
                policy,
                prev_bundle.as_ref(),
                gates,
                mode,
                use_interpreter,
                profile,
                setting_overrides,
            );
            // Transactional hot replacement: commit only when the new bundle
            // loaded; otherwise keep the prior session valid.
            let mut txn = jet_devserver::HotReplaceTxn::begin(session.clone());
            match &next {
                Some(bundle) => {
                    sync_persist_bindings(bundle, &mut persist);
                    txn.mark_server_ready();
                    txn.mark_client_ready();
                    match txn.commit() {
                        Ok(snap) => {
                            session = snap;
                            session.persist = persist.clone();
                            prev_bundle = next;
                        }
                        Err((prior, reason)) => {
                            eprintln!("[hot-replace] {reason}");
                            session = prior;
                        }
                    }
                }
                None => {
                    txn.fail("reload failed; prior session kept");
                    let _ = txn.commit();
                }
            }
            if let Err(diagnostic) = watch.acknowledge(&receipt) {
                eprint!(
                    "{}",
                    jet::render_all_colored(file, "", &[diagnostic], mode.color_stderr())
                );
                exit(ExitCodes::USER_ERROR);
            }
            if let Some(ms) = receipt.edit_to_visible_ms {
                if !jet_devserver::within_budget(&receipt) {
                    eprintln!(
                        "[watch] edit-to-visible {ms}ms exceeded budget {}ms",
                        jet_devserver::EDIT_TO_VISIBLE_BUDGET_MS
                    );
                }
            }
        }
    }
}

/// D-PERSIST1: refresh `#Persist` bindings from the loaded bundle into the
/// shared runtime-heap persist store (typed migration on shape change).
fn sync_persist_bindings(
    bundle: &jet::AST::ProgramBundle,
    store: &mut jet_devserver::PersistStore,
) {
    let prep = jet_foundation::Persist::prepare_bundle(bundle);
    for msg in &prep.messages {
        eprintln!("{msg}");
    }
    *store = jet_foundation::Persist::shared_clone();
}

/// D-SCHEDULE1 (ratified 2026-07-11, card #505): the `jet dev` consumer of
/// schedule-as-code — check every `#Job #Every(…)` fn in `bundle` against
/// `clock`, and run whichever are due through the same interpreter tier the
/// rest of the dev loop uses (`jet::Interpreter::run_named_job`). This is
/// the dev-loop tier only (D-DEV3); the service runtime (D-SERVICE1) and a
/// jetos timer projection are the production/OS consumers of the identical
/// `#Every(…)` declaration — see the D-SCHEDULE1 row in
/// docs/spec/syntax-decisions.md for the full three-consumer law.
fn run_due_tasks(
    bundle: &jet::AST::ProgramBundle,
    file: &str,
    try_anyway: bool,
    mode: OutputMode,
    clock: &mut TaskClock,
) {
    let tasks = jet::Interpreter::scheduled_tasks(bundle);
    if tasks.is_empty() {
        return;
    }
    for name in clock.due(&tasks) {
        if !mode.quiet {
            println!("\n— due job `{}` —", name);
        }
        match jet::Interpreter::run_named_job(bundle, &name, try_anyway) {
            jet::Interpreter::RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                emit_run_output(&stdout, &stderr);
                if exit_code != 0 {
                    eprintln!("job `{name}` exited with code {exit_code}");
                }
            }
            jet::Interpreter::RunOutcome::Problems(diags) => {
                let src = fs::read_to_string(file).unwrap_or_default();
                report_problems(mode, file, &src, &diags);
            }
        }
    }
}

/// D-SCHEDULE1: per-job last-run bookkeeping for the due-job tick. An
/// `Interval` schedule tracks the `Instant` it last ran; a `DailyAt`
/// schedule tracks the UTC day index (days since the Unix epoch) it last
/// ran, so it fires once inside its matching minute, not on every 120ms
/// tick within that minute. UTC only — D-SCHEDULE1's own law text carves
/// timezone-aware calendars out to "the runtime API or jetos timers"; this
/// is the lightweight dev-loop convenience tier, not that.
pub(crate) struct TaskClock {
    last_interval_run: std::collections::HashMap<String, std::time::Instant>,
    last_daily_run_day: std::collections::HashMap<String, u64>,
}

impl TaskClock {
    pub(crate) fn new() -> Self {
        TaskClock {
            last_interval_run: std::collections::HashMap::new(),
            last_daily_run_day: std::collections::HashMap::new(),
        }
    }

    /// Which job names are due right now — records the firing so the same
    /// job doesn't fire again on the very next tick.
    pub(crate) fn due(&mut self, tasks: &[(String, jet::AST::EverySchedule)]) -> Vec<String> {
        let unix_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.due_at(tasks, unix_secs)
    }

    /// The testable core of `due`: `unix_secs` is injected so the day/window
    /// arithmetic can be checked without racing real wall-clock time.
    fn due_at(&mut self, tasks: &[(String, jet::AST::EverySchedule)], unix_secs: u64) -> Vec<String> {
        let now = std::time::Instant::now();
        let day = unix_secs / 86_400;
        let secs_of_day = unix_secs % 86_400;
        let mut fired = Vec::new();
        for (name, schedule) in tasks {
            match *schedule {
                jet::AST::EverySchedule::Interval { nanos } => {
                    let due = match self.last_interval_run.get(name) {
                        None => true,
                        Some(last) => now.duration_since(*last).as_nanos() >= nanos,
                    };
                    if due {
                        self.last_interval_run.insert(name.clone(), now);
                        fired.push(name.clone());
                    }
                }
                jet::AST::EverySchedule::DailyAt { hour, minute } => {
                    let target_secs = hour as u64 * 3600 + minute as u64 * 60;
                    // Due once inside the matching minute — a 120ms poll tick
                    // easily lands inside a 60-second window.
                    let in_window =
                        secs_of_day >= target_secs && secs_of_day < target_secs + 60;
                    let already_ran_today = self.last_daily_run_day.get(name) == Some(&day);
                    if in_window && !already_ran_today {
                        self.last_daily_run_day.insert(name.clone(), day);
                        fired.push(name.clone());
                    }
                }
            }
        }
        fired
    }
}

/// Handle one detected file change: pick swap vs rerun vs restart and render.
/// Returns the freshly loaded bundle (or `None` if it failed to load) for the
/// next diff.
fn render_dev_change(
    file: &str,
    try_anyway: bool,
    policy: WatchPolicy,
    prev: Option<&jet::AST::ProgramBundle>,
    gates: jet::Policy::GateSet,
    mode: OutputMode,
    use_interpreter: bool,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> Option<jet::AST::ProgramBundle> {
    // Load+check the new bundle so we can both diff its type surface and run it.
    let new_bundle = match jet::Loader::load_entry(file) {
        Ok(mut b) => {
            if let Err(diags) = jet::Driver::seed_build_facts(
                &mut b,
                profile,
                false,
                setting_overrides,
            ) {
                let src = fs::read_to_string(file).unwrap_or_default();
                println!("\n— {} changed —", file);
                report_problems(mode, file, &src, &diags);
                return None;
            }
            let diags = jet::Sema::check_bundle_gates(&mut b, jet::Sema::CompileMode::Run, gates);
            let errs: Vec<_> = diags
                .iter()
                .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
                .cloned()
                .collect();
            if !errs.is_empty() {
                let src = fs::read_to_string(file).unwrap_or_default();
                println!("\n— {} changed —", file);
                report_problems(mode, file, &src, &errs);
                // Keep the previous bundle as the swap baseline; the bad edit
                // never became the running version.
                return None;
            }
            render_dev_lints(file, mode, &diags);
            b
        }
        Err(diags) => {
            let src = fs::read_to_string(file).unwrap_or_default();
            println!("\n— {} changed —", file);
            report_problems(mode, file, &src, &diags);
            return None;
        }
    };

    // Decide whether this save uses the swap path.
    let resident = match policy {
        WatchPolicy::Swap => true,
        WatchPolicy::Restart => false,
        WatchPolicy::Once => false, // unreachable here (handled in run_dev)
        WatchPolicy::Auto => {
            jet::Interpreter::detect_dev_mode(&new_bundle) == jet::Interpreter::DevMode::Resident
        }
    };

    if resident {
        // The hot-reload unit is the entry module (D-HOTSWAP1).
        let module_name = new_bundle
            .modules
            .get(new_bundle.entry)
            .map(|m| m.display.clone())
            .unwrap_or_else(|| file.to_string());

        match prev {
            Some(old) => {
                match jet::Sema::HotSwap::type_stable_check(old, &new_bundle, &module_name) {
                    Ok(()) => {
                        if !mode.quiet {
                            println!(
                                "\n[hot-swap] {} — types stable, code re-applied",
                                module_name
                            );
                        }
                        if !run_resident_swap(
                            &new_bundle,
                            try_anyway,
                            &module_name,
                            file,
                            mode,
                            use_interpreter,
                        ) {
                            return None;
                        }
                    }
                    Err(diags) => {
                        // E2210 names what changed; surface it on the restart line.
                        let what = diags.first().map(|d| d.what.clone()).unwrap_or_default();
                        if !mode.quiet {
                            println!("\n[restart] {} — {}", module_name, what);
                        }
                        if !run_resident_restart(&new_bundle, try_anyway, file, mode, use_interpreter)
                        {
                            return None;
                        }
                    }
                }
            }
            None => {
                // No baseline yet (first run after an error): a clean restart.
                if !mode.quiet {
                    println!("\n[restart] {} — first run", module_name);
                }
                if !run_resident_restart(&new_bundle, try_anyway, file, mode, use_interpreter) {
                    return None;
                }
            }
        }
    } else {
        // Run-to-completion (default / `--restart`): plain rerun.
        if !mode.quiet {
            println!("\n— {} changed, re-running —", file);
        }
        let outcome = jet::Interpreter::dev_iteration_with_gates_profile_and_settings(
            file,
            try_anyway,
            use_interpreter,
            gates,
            profile,
            setting_overrides,
        );
        render_dev_outcome(&outcome, file, mode);
    }

    Some(new_bundle)
}

/// Hot-swap via the strict Cranelift backend (`--interpret` uses tier-0).
fn run_resident_swap(
    bundle: &jet::AST::ProgramBundle,
    try_anyway: bool,
    module_name: &str,
    file: &str,
    mode: OutputMode,
    use_interpreter: bool,
) -> bool {
    use jet::JitBackend::{InterpreterBackend, JitBackend};
    use jet_jit::CraneliftBackend;
    let outcome = if use_interpreter {
        let mut b = InterpreterBackend::new();
        b.hot_swap(module_name, bundle, try_anyway)
    } else {
        let mut b = CraneliftBackend::new();
        b.hot_swap(module_name, bundle, try_anyway)
    };
    match outcome {
        Ok(o) => {
            render_outcome(o, file, mode);
            true
        }
        Err(diags) => {
            let src = fs::read_to_string(file).unwrap_or_default();
            report_problems(mode, file, &src, &diags);
            false
        }
    }
}

/// Clean restart via the strict Cranelift backend.
fn run_resident_restart(
    bundle: &jet::AST::ProgramBundle,
    try_anyway: bool,
    file: &str,
    mode: OutputMode,
    use_interpreter: bool,
) -> bool {
    use jet::JitBackend::{InterpreterBackend, JitBackend};
    use jet_jit::CraneliftBackend;
    let outcome = if use_interpreter {
        let mut b = InterpreterBackend::new();
        b.restart(bundle, try_anyway)
    } else {
        let mut b = CraneliftBackend::new();
        b.restart(bundle, try_anyway)
    };
    let ok = matches!(&outcome, jet::Interpreter::RunOutcome::Ran { .. });
    render_outcome(outcome, file, mode);
    ok
}

/// `jet repl` — interactive REPL session (E2-M18, D-REPL3=A).
/// `project_dir` sets the base for `:load` paths and (eventually) import
/// context (D-REPL10=A sandbox; `--project <dir>` enables project mode).
pub(crate) fn run_repl(
    project_dir: Option<&str>,
    allow: &[String],
    deny: &[String],
    color: ColorChoice,
) {
    let flags = jet::REPL::ReplFlags::new(allow, deny).with_color(color);
    let code = jet::REPL::run(project_dir, flags);
    exit(code);
}

/// Run one dev iteration and render its outcome to the terminal in the active
/// output mode. Diagnostics use the SAME renderer as batch compilation
/// (D-DEV), so a problem looks identical whether seen via `jet check` or
/// `jet dev`.
fn render_dev_iteration(
    file: &str,
    try_anyway: bool,
    gates: jet::Policy::GateSet,
    mode: OutputMode,
    use_interpreter: bool,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> Option<jet::AST::ProgramBundle> {
    let started = std::time::Instant::now();
    let outcome = jet::Interpreter::dev_iteration_with_gates_profile_and_settings(
        file,
        try_anyway,
        use_interpreter,
        gates,
        profile,
        setting_overrides,
    );
    let elapsed = started.elapsed();
    let ran_ok = matches!(outcome, jet::Interpreter::RunOutcome::Ran { .. });
    let bundle = if ran_ok {
        match jet::Loader::load_entry(file) {
            Ok(mut bundle) => {
                if let Err(diags) = jet::Driver::seed_build_facts(
                    &mut bundle,
                    profile,
                    false,
                    setting_overrides,
                ) {
                    let source = fs::read_to_string(file).unwrap_or_default();
                    report_problems(mode, file, &source, &diags);
                    None
                } else {
                    let diagnostics = jet::Sema::check_bundle_gates(
                        &mut bundle,
                        jet::Sema::CompileMode::Run,
                        gates,
                    );
                    render_dev_lints(file, mode, &diagnostics);
                    Some(bundle)
                }
            }
            Err(_) => None,
        }
    } else {
        None
    };
    render_outcome_timed(outcome, file, Some(elapsed), mode);
    if let Some(bundle) = bundle {
        run_dev_budget_refresh(file, &bundle, mode);
        Some(bundle)
    } else {
        None
    }
}

fn render_dev_outcome(
    outcome: &jet::Interpreter::RunOutcome,
    file: &str,
    mode: OutputMode,
) {
    match outcome {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, .. } => {
            emit_run_output(stdout, stderr);
        }
        jet::Interpreter::RunOutcome::Problems(diags) => {
            let src = fs::read_to_string(file).unwrap_or_default();
            report_problems(mode, file, &src, diags);
        }
    }
}

fn exit_dev_outcome(outcome: jet::Interpreter::RunOutcome) {
    match outcome {
        jet::Interpreter::RunOutcome::Ran { exit_code, .. } => exit(exit_code),
        jet::Interpreter::RunOutcome::Problems(_) => {
            exit(ExitCodes::USER_ERROR);
        }
    }
}

fn render_dev_lints(file: &str, mode: OutputMode, diagnostics: &[jet::Diagnostics::Diagnostic]) {
    let lints = visible_lints(diagnostics);
    if !lints.is_empty() {
        let source = fs::read_to_string(file).unwrap_or_default();
        report_problems(mode, file, &source, &lints);
    }
}

/// After a successful dev iteration, collect ServiceProbe/SceneProbe evidence
/// for any active dev-owned budgets and trigger a report refresh.
fn run_dev_budget_refresh(file: &str, bundle: &jet::AST::ProgramBundle, mode: OutputMode) {
    let specs = match jet::Sema::collect_located_budget_specs_bundle(bundle) {
        Ok(specs) => specs,
        Err(_) => return,
    };
    let has_service = specs.iter().any(|s| {
        s.spec.provider.split_once('(').map(|(k, _)| k).unwrap_or(&s.spec.provider) == "ServiceProbe"
    });
    let has_scene = specs.iter().any(|s| {
        s.spec.provider.split_once('(').map(|(k, _)| k).unwrap_or(&s.spec.provider) == "SceneProbe"
    });
    if !has_service && !has_scene {
        return;
    }
    let root = Path::new(file)
        .canonicalize()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .and_then(|d| jet::Loader::find_manifest_root(&d).or_else(|| Some(d)))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => return,
    };
    let service_evidence = if has_service {
        collect_service_evidence(&root, &specs)
    } else {
        Vec::new()
    };
    let scene_evidence = if has_scene {
        collect_scene_evidence(file, &src, mode, &specs)
    } else {
        Vec::new()
    };
    let status = crate::CmdBudget::run_dev_refresh(file, &service_evidence, &scene_evidence);
    if status != 0 {
        eprintln!("budget: dev refresh failed with status {status}");
    }
}

/// Render a run outcome with no timing line (used for re-runs/swaps).
fn render_outcome(outcome: jet::Interpreter::RunOutcome, file: &str, mode: OutputMode) {
    render_outcome_timed(outcome, file, None, mode);
}

fn render_outcome_timed(
    outcome: jet::Interpreter::RunOutcome,
    file: &str,
    elapsed: Option<std::time::Duration>,
    mode: OutputMode,
) {
    match outcome {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, .. } => {
            emit_run_output(&stdout, &stderr);
            if let Some(e) = elapsed {
                if !mode.quiet {
                    println!("✓ ran in {} ms", e.as_millis());
                }
            }
        }
        jet::Interpreter::RunOutcome::Problems(diags) => {
            let src = fs::read_to_string(file).unwrap_or_default();
            report_problems(mode, file, &src, &diags);
        }
    }
}

/// `jet self completions SHELL [--for PROGRAM]` (D-DX4,
/// D-SHAPE-CLI-COMPLETE1=A). External schemas are read statically from the
/// executable; no application code runs.
pub(crate) fn run_completions(args: &[String]) {
    const USAGE: &str = "jet self completions <bash|zsh|fish|powershell> [--for PROGRAM]";
    let shell = args.first().map(String::as_str);
    if !matches!(shell, Some("bash" | "zsh" | "fish" | "powershell"))
        || !matches!(args.len(), 1 | 3)
        || (args.len() == 3 && args[1] != jet::Syntax::CLI_COMPLETIONS_FOR)
    {
        eprintln!("Error [E2102]: completions arguments don't match `{USAGE}`.");
        eprintln!(" Why: a completion script needs one supported shell and, optionally, one compiled Jet program.");
        eprintln!(" Fix: run `{USAGE}`.");
        exit(ExitCodes::USAGE);
    }
    let shell = shell.unwrap();
    let out = if args.len() == 1 {
        match shell {
            "bash" => jet::CLI::completions_bash(),
            "zsh" => jet::CLI::completions_zsh(),
            "fish" => jet::CLI::completions_fish(),
            "powershell" => jet::CLI::completions_powershell(),
            _ => unreachable!(),
        }
    } else {
        let program = &args[2];
        let command_name = jet::CLI::completion_command_name(program)
            .unwrap_or_else(|error| completion_metadata_error(program, error));
        let mut file = open_completion_program(program).unwrap_or_else(|error| completion_metadata_error(program, &format!("the program could not be opened ({error})")));
        let metadata = file.metadata().unwrap_or_else(|error| completion_metadata_error(program, &format!("the opened program could not be inspected ({error})")));
        if !metadata.file_type().is_file() {
            completion_metadata_error(program, "the program is not a regular file");
        }
        const MAX_PROGRAM_BYTES: u64 = 512 * 1024 * 1024;
        if metadata.len() > MAX_PROGRAM_BYTES {
            completion_metadata_error(program, "the program is larger than the 512 MiB metadata-reader limit");
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.by_ref().take(MAX_PROGRAM_BYTES + 1).read_to_end(&mut bytes)
            .unwrap_or_else(|error| completion_metadata_error(program, &format!("the opened program could not be read ({error})")));
        if bytes.len() as u64 > MAX_PROGRAM_BYTES {
            completion_metadata_error(program, "the program grew beyond the 512 MiB metadata-reader limit while being read");
        }
        let schema = jet_foundation::CLISchema::read_executable(&bytes)
            .unwrap_or_else(|error| completion_metadata_error(program, &error.to_string()));
        jet::CLI::completions_for_program(shell, &command_name, &schema).unwrap()
    };
    print!("{}", out);
}

#[cfg(unix)]
fn open_completion_program(path: &str) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const NONBLOCK: i32 = 0o4000;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    const NONBLOCK: i32 = 0x0004;
    OpenOptions::new().read(true).custom_flags(NONBLOCK).open(path)
}

#[cfg(not(unix))]
fn open_completion_program(path: &str) -> std::io::Result<File> {
    OpenOptions::new().read(true).open(path)
}

fn completion_metadata_error(program: &str, why: &str) -> ! {
    let program = program.chars().flat_map(char::escape_default).collect::<String>();
    eprintln!("Error [E2103]: couldn't read command metadata from `{program}`.");
    eprintln!(" Why: {why}.");
    eprintln!(" Fix: rebuild the program with this Jet toolchain, then try again.");
    exit(ExitCodes::USER_ERROR);
}

/// Every `jet self devtools` subcommand name, for the usage line and typo errors.
const DEVTOOLS_SUBCOMMANDS: &str =
    "grammars | reduce | ice-report | new-example | new-ui | check-fixture-paths | bless";

/// `jet self devtools grammars` — D-HL1 generated lexical base for editor grammars.
/// c450 (D-DEVTOOLS1=A): extended with maintainer-facing minimizer/scaffolding
/// tools, all under this same hidden namespace (never top-level commands).
pub(crate) fn run_devtools(args: &[&String], mode: OutputMode) {
    match args.first().map(|s| s.as_str()) {
        Some("grammars") => {
            write_generated_section(
                "editors/vscode/syntaxes/jet.tmLanguage.json",
                &jet::Syntax::render_vscode_generated_highlights(),
                mode.quiet,
            );
            write_generated_section(
                "editors/jet.tmGrammar",
                &jet::Syntax::render_vscode_generated_highlights(),
                mode.quiet,
            );
            write_generated_section(
                "editors/tree-sitter/grammar.js",
                &jet::Syntax::render_tree_sitter_generated_highlights(),
                mode.quiet,
            );
            write_generated_section(
                "editors/zed/languages/jet/highlights.scm",
                &jet::Syntax::render_zed_generated_highlights(),
                mode.quiet,
            );
            // #1659 criterion 3: the files were still written; `--quiet`
            // only mutes this confirmation line.
            if !mode.quiet {
                println!("regenerated editor grammar sections");
            }
        }
        Some("reduce") => run_devtools_reduce(&args[1..]),
        Some("ice-report") => run_devtools_ice_report(&args[1..]),
        Some("new-example") => run_devtools_new_example(&args[1..]),
        Some("new-ui") => run_devtools_new_ui(&args[1..]),
        Some("check-fixture-paths") => run_devtools_check_fixture_paths(),
        Some("bless") => run_devtools_bless(&args[1..]),
        Some("probe") => run_devtools_probe(&args[1..]),
        Some(other) => {
            crate::cli_error!("E2101", "unknown `devtools` subcommand `{}`", other);
            eprintln!("usage: {} devtools <{}>", jet::Syntax::BINARY_NAME, DEVTOOLS_SUBCOMMANDS);
            exit(ExitCodes::USAGE);
        }
        None => {
            eprintln!("usage: {} devtools <{}>", jet::Syntax::BINARY_NAME, DEVTOOLS_SUBCOMMANDS);
            exit(ExitCodes::USAGE);
        }
    }
}

// ──────────────────────────────────────────────
// c450: `jet self devtools reduce` — delta-debugging minimizer.
// ──────────────────────────────────────────────

/// `jet self devtools reduce <file.jet> [--code EXXXX]`.
///
/// Oracle (what makes a candidate "still interesting"):
///   - default: the front end accepts the file AND rustc rejects the
///     generated Rust (an I2 repro — invariant I2 says this must never
///     happen in a shipped compiler, so a minimal repro is worth having);
///   - `--code EXXXX`: the front end emits diagnostic `EXXXX` (from either
///     an error result or a lint on a successful compile).
///
/// Shrinks by removing line-chunks (a simplified Zeller ddmin: try removing
/// progressively smaller contiguous chunks, re-trying the whole file at a
/// finer granularity whenever a whole pass makes no progress) and writes the
/// smallest failing case to `<file>.reduced.<ext>`.
pub(crate) fn run_devtools_reduce(args: &[&String]) {
    if args.is_empty() {
        eprintln!(
            "usage: {} devtools reduce <file.{}> [--code EXXXX]",
            jet::Syntax::BINARY_NAME,
            jet::Syntax::FILE_EXT
        );
        exit(ExitCodes::USAGE);
    }
    let file = args[0].as_str();
    let mut code_filter: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--code" => {
                code_filter = args.get(i + 1).map(|s| s.to_string());
                i += 2;
            }
            other => {
                crate::cli_error!("E2102", "unknown `reduce` flag `{}`", other);
                exit(ExitCodes::USAGE);
            }
        }
    }

    let src = fs::read_to_string(file).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't read `{}`: {}", file, e);
        exit(ExitCodes::USER_ERROR);
    });

    // `compile_with_path` reads its *file* argument straight off disk (its
    // `src` parameter is display-only), so every candidate has to actually
    // exist on disk under the same extension before we can ask the oracle
    // about it. One scratch path, rewritten each try.
    let ext = Path::new(file)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or(jet::Syntax::FILE_EXT);
    let scratch =
        std::env::temp_dir().join(format!("jet_devtools_reduce_{}.{}", std::process::id(), ext));
    let interesting = |text: &str| reduce_oracle(&scratch, text, code_filter.as_deref());

    if !interesting(&src) {
        let why = code_filter.as_ref().map_or_else(
            || "either the front end already rejects it, or rustc accepts the generated Rust".to_string(),
            |code| format!("the front end never emits `{code}` for this file"),
        );
        crate::cli_error!(@full "E2104", format!("`{}` doesn't reproduce the target oracle as given", file), why, "confirm the case fails the way you expect, then reduce it");
        exit(ExitCodes::USER_ERROR);
    }

    let lines: Vec<String> = src.lines().map(|s| s.to_string()).collect();
    println!("reduce: starting at {} line(s)", lines.len());
    let reduced = ddmin(lines, &interesting);
    println!("reduce: finished at {} line(s)", reduced.len());

    let mut out_text = reduced.join("\n");
    if !out_text.is_empty() {
        out_text.push('\n');
    }
    let out_path = reduced_path(file);
    fs::write(&out_path, &out_text).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't write `{}`: {}", out_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });
    let _ = fs::remove_file(&scratch);
    println!("wrote {}", out_path.display());
}

/// Whether `src` still triggers the target oracle. `code`: `None` = default
/// I2 oracle (front end accepts, rustc rejects); `Some(code)` = the front end
/// emits that diagnostic code (error or lint). Writes `src` to `scratch`
/// first since `compile_with_path` reads its file argument off disk.
fn reduce_oracle(scratch: &Path, src: &str, code: Option<&str>) -> bool {
    if fs::write(scratch, src).is_err() {
        return false;
    }
    let shown = scratch.to_string_lossy().into_owned();
    match jet::compile_with_path(src, &shown) {
        Ok(out) => match code {
            Some(c) => out.lints.iter().any(|d| d.code == c),
            None => rustc_rejects(&out.rust),
        },
        Err(diags) => match code {
            Some(c) => diags.iter().any(|d| d.code == c),
            None => false, // default oracle needs the front end to accept
        },
    }
}

/// Simplified Zeller ddmin over line-chunks: repeatedly try removing a
/// contiguous chunk of the current granularity; on any removal that keeps the
/// oracle interesting, keep the shrunk version and tighten the granularity;
/// on a full pass with no progress, double the chunk count (finer chunks)
/// until we're at single-line granularity, then stop.
fn ddmin(lines: Vec<String>, interesting: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut current = lines;
    let mut n: usize = 2;
    loop {
        if current.len() < 2 {
            break;
        }
        let chunk_size = (current.len() + n - 1) / n;
        if chunk_size == 0 {
            break;
        }
        let mut made_progress = false;
        let mut start = 0;
        while start < current.len() {
            let end = (start + chunk_size).min(current.len());
            let mut candidate = current.clone();
            candidate.drain(start..end);
            if !candidate.is_empty() && interesting(&candidate.join("\n")) {
                println!(
                    "reduce: dropped lines {}..{} -> {} line(s) left",
                    start + 1,
                    end,
                    candidate.len()
                );
                current = candidate;
                n = if n > 2 { n - 1 } else { 2 };
                made_progress = true;
                break;
            }
            start = end;
        }
        if !made_progress {
            if n >= current.len() {
                break;
            }
            n = (n * 2).min(current.len());
        }
    }
    current
}

/// `<file>.reduced.<ext>` sibling path for the reducer's output.
fn reduced_path(file: &str) -> PathBuf {
    let p = Path::new(file);
    let stem_name = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("reduced");
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or(jet::Syntax::FILE_EXT);
    let name = format!("{}.reduced.{}", stem_name, ext);
    match p.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

/// Run rustc over generated Rust source with no linking (`--emit=metadata`,
/// I2's oracle only cares whether rustc accepts the code, not whether it
/// links). Returns `(accepted, stderr)`; a rustc that can't even be invoked
/// counts as "accepted" so a missing toolchain never manufactures a false
/// I2 repro.
fn rustc_probe(rust_code: &str) -> (bool, String) {
    let tmp_dir = std::env::temp_dir().join(format!("jet_devtools_rustc_{}", std::process::id()));
    if fs::create_dir_all(&tmp_dir).is_err() {
        return (true, String::new());
    }
    let rs_path = tmp_dir.join("check.rs");
    if fs::write(&rs_path, rust_code).is_err() {
        let _ = fs::remove_dir_all(&tmp_dir);
        return (true, String::new());
    }
    let meta_path = tmp_dir.join("check.rmeta");
    let result = Command::new("rustc")
        .arg("--edition")
        .arg("2021")
        .arg("--crate-name")
        .arg("jet_devtools_check")
        .arg("--emit=metadata")
        .arg(&rs_path)
        .arg("-o")
        .arg(&meta_path)
        .output();
    let outcome = match result {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        ),
        Err(_) => (true, String::new()),
    };
    let _ = fs::remove_dir_all(&tmp_dir);
    outcome
}

fn rustc_rejects(rust_code: &str) -> bool {
    !rustc_probe(rust_code).0
}

// ──────────────────────────────────────────────
// c450: `jet self devtools ice-report` — bundle an I2 repro for a bug report.
// ──────────────────────────────────────────────

/// `jet self devtools ice-report <file.jet>` — bundles the source, generated Rust,
/// rustc's stderr, and both tool versions into one directory under
/// `.jet/ice-report/<stem>-<unix-time>/` so a bug report has everything
/// attached in one place. Prints the bundle path.
pub(crate) fn run_devtools_ice_report(args: &[&String]) {
    if args.is_empty() {
        eprintln!(
            "usage: {} devtools ice-report <file.{}>",
            jet::Syntax::BINARY_NAME,
            jet::Syntax::FILE_EXT
        );
        exit(ExitCodes::USAGE);
    }
    let file = args[0].as_str();
    let src = fs::read_to_string(file).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't read `{}`: {}", file, e);
        exit(ExitCodes::USER_ERROR);
    });

    let out = match jet::compile_with_path(&src, file) {
        Ok(o) => o,
        Err(diags) => {
            crate::cli_error!(@full "E2105", format!("`{}` doesn't reach codegen — the front end already rejects it", file), "ice-report bundles a case that compiles to Rust (an I2 repro)", format!("fix the front-end errors first, or use `{} devtools reduce --code <CODE>` to shrink a front-end diagnostic instead", jet::Syntax::BINARY_NAME));
            eprint!("{}", jet::render_diagnostics(file, &src, &diags));
            exit(ExitCodes::USER_ERROR);
        }
    };

    let (accepted, rustc_stderr) = rustc_probe(&out.rust);
    if accepted {
        println!(
            "note: rustc accepted the generated Rust for `{}` — bundling anyway, but this isn't an I2 repro",
            file
        );
    }

    let rustc_version = Command::new("rustc")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "rustc: not found".to_string());
    let jet_version = env!("CARGO_PKG_VERSION");

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let bundle_dir = PathBuf::from(".jet")
        .join("ice-report")
        .join(format!("{}-{}", stem(file), ts));
    fs::create_dir_all(&bundle_dir).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't create `{}`: {}", bundle_dir.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    let write = |name: &str, content: &str| {
        fs::write(bundle_dir.join(name), content).unwrap_or_else(|e| {
            crate::cli_error!("E2105", "couldn't write `{}`: {}", name, e);
            exit(ExitCodes::USER_ERROR);
        });
    };
    write(&format!("source.{}", jet::Syntax::FILE_EXT), &src);
    write("generated.rs", &out.rust);
    write("rustc.stderr", &rustc_stderr);
    write(
        "versions.txt",
        &format!("jet {}\n{}\n", jet_version, rustc_version),
    );

    println!("wrote ICE report bundle to {}", bundle_dir.display());
}

// ──────────────────────────────────────────────
// c450: `jet self devtools new-example` / `new-ui` — scaffold I5/I4 fixtures.
// ──────────────────────────────────────────────

/// `jet self devtools new-example <topic>/<name>` — scaffolds
/// `examples/features/<topic>/<name>.jet` and
/// `examples/features/expected/<topic>/<name>.out`, matching the layout
/// `tests/golden.rs` walks exactly. The stub is a real, passing example (I5:
/// no example ships broken) that the author edits to demonstrate the feature.
pub(crate) fn run_devtools_new_example(args: &[&String]) {
    if args.is_empty() {
        eprintln!(
            "usage: {} devtools new-example <topic>/<name>",
            jet::Syntax::BINARY_NAME
        );
        exit(ExitCodes::USAGE);
    }
    let spec = args[0].as_str();
    let (topic, name) = match spec.split_once('/') {
        Some((t, n)) if !t.is_empty() && !n.is_empty() => (t, n),
        _ => {
            crate::cli_error!("E2104", "expected `<topic>/<name>`, got `{}`", spec);
            exit(ExitCodes::USER_ERROR);
        }
    };

    let ext = jet::Syntax::FILE_EXT;
    let example_dir = PathBuf::from("examples/features").join(topic);
    let expected_dir = PathBuf::from("examples/features/expected").join(topic);
    let example_path = example_dir.join(format!("{}.{}", name, ext));
    let expected_path = expected_dir.join(format!("{}.out", name));

    if example_path.exists() {
        crate::cli_error!("E2104", "`{}` already exists", example_path.display());
        exit(ExitCodes::USER_ERROR);
    }

    fs::create_dir_all(&example_dir).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't create `{}`: {}", example_dir.display(), e);
        exit(ExitCodes::USER_ERROR);
    });
    fs::create_dir_all(&expected_dir).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't create `{}`: {}", expected_dir.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    let greeting = format!("scaffold: {}/{}", topic, name);
    let src = format!(
        "// TODO: describe examples/features/{}/{}.{}\nfn run() {{\n    print(\"{}\")\n}}\n",
        topic, name, ext, greeting
    );
    fs::write(&example_path, &src).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't write `{}`: {}", example_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });
    fs::write(&expected_path, format!("{}\n", greeting)).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't write `{}`: {}", expected_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    println!("wrote {}", example_path.display());
    println!("wrote {}", expected_path.display());
}

/// `jet self devtools new-ui <name>` — scaffolds `tests/ui/<name>.jet` and its
/// `<name>.stderr` snapshot, matching the layout `tests/diagnostic_snapshots.rs`
/// walks exactly. The stub triggers a real (if generic) diagnostic and its
/// `.stderr` is computed with the SAME calls the harness uses, so the pair is
/// valid the moment it's written — edit the `.jet` to demonstrate the real
/// diagnostic, then re-bless with `jet self devtools bless diagnostic_snapshots`.
pub(crate) fn run_devtools_new_ui(args: &[&String]) {
    if args.is_empty() {
        eprintln!("usage: {} devtools new-ui <name>", jet::Syntax::BINARY_NAME);
        exit(ExitCodes::USAGE);
    }
    let name = args[0].as_str();
    if name.is_empty() || name.contains('/') || name.contains('.') {
        crate::cli_error!("E2104", "`new-ui` takes a bare fixture name, no path or extension: got `{}`", name);
        exit(ExitCodes::USER_ERROR);
    }

    let ext = jet::Syntax::FILE_EXT;
    let dir = PathBuf::from("tests/ui");
    let jet_path = dir.join(format!("{}.{}", name, ext));
    let stderr_path = dir.join(format!("{}.stderr", name));

    if jet_path.exists() {
        crate::cli_error!("E2104", "`{}` already exists", jet_path.display());
        exit(ExitCodes::USER_ERROR);
    }
    fs::create_dir_all(&dir).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't create `{}`: {}", dir.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    let src = "// TODO: describe what this diagnostic demonstrates.\n\
fn run() {\n    print(definitely_undefined_scaffold_symbol)\n}\n"
        .to_string();
    fs::write(&jet_path, &src).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't write `{}`: {}", jet_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    // Mirror `tests/diagnostic_snapshots.rs`'s `ui_snapshots` exactly, so the
    // pair this writes is already a valid harness fixture.
    let shown_path = format!("tests/ui/{}.{}", name, ext);
    let actual = match jet::compile_with_path(&src, &shown_path) {
        Err(diags) => jet::render_diagnostics(&shown_path, &src, &diags),
        Ok(_) => "(no errors)\n".to_string(),
    };
    fs::write(&stderr_path, &actual).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't write `{}`: {}", stderr_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    println!("wrote {}", jet_path.display());
    println!("wrote {}", stderr_path.display());
    if actual == "(no errors)\n" {
        println!(
            "note: scaffold compiled cleanly — edit `{}` to trigger the diagnostic you want, \
             then `{} devtools bless diagnostic_snapshots`",
            jet_path.display(),
            jet::Syntax::BINARY_NAME
        );
    }
}

// ──────────────────────────────────────────────
// c450: `jet self devtools check-fixture-paths` — validate hardcoded path fixtures.
// ──────────────────────────────────────────────

/// `jet self devtools check-fixture-paths` — greps every `tests/**/*.rs` file for
/// hardcoded fixture path literals (`examples/features/...`, `docs/spec/...`,
/// `tests/ui/...`, etc.) and confirms each one exists on disk relative to the
/// current directory (run from the repo root). Path-embedding fixtures rot
/// silently when an example moves; this is the check that catches it.
pub(crate) fn run_devtools_check_fixture_paths() {
    let tests_dir = PathBuf::from("tests");
    if !tests_dir.is_dir() {
        crate::cli_error!(@fix "E2104", "no `tests/` directory here", "run from the repo root");
        exit(ExitCodes::USER_ERROR);
    }

    let mut rs_files = Vec::new();
    collect_rs_files(&tests_dir, &mut rs_files);

    let mut checked = 0usize;
    let mut missing: Vec<(PathBuf, String)> = Vec::new();
    for rs in &rs_files {
        let text = fs::read_to_string(rs).unwrap_or_default();
        for candidate in extract_hardcoded_paths(&text) {
            checked += 1;
            if !Path::new(&candidate).exists() {
                missing.push((rs.clone(), candidate));
            }
        }
    }

    if missing.is_empty() {
        println!(
            "check-fixture-paths: {} embedded path(s) across {} file(s), all present",
            checked,
            rs_files.len()
        );
    } else {
        eprintln!("check-fixture-paths: {} missing path(s):", missing.len());
        for (file, path) in &missing {
            eprintln!("  {} -> `{}` does not exist", file.display(), path);
        }
        exit(ExitCodes::USER_ERROR);
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rs_files(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

/// Pull out string-literal path fixtures from Rust source text. Deliberately
/// conservative: only whole quoted literals (never `format!` templates with a
/// `{`) whose prefix marks them as a fixture path and whose suffix is a known
/// fixture extension, so dynamically-joined paths (already covered by their
/// own `read_to_string`/`unwrap_or_else` panics at test time) are left alone.
fn extract_hardcoded_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'"' {
                if bytes[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
            if j <= text.len() {
                let lit = &text[start..j.min(text.len())];
                // `render_diagnostics`/`render_all_*` take a "shown" display
                // label as their file-name argument, not a path that must
                // exist (tests/net_tls.rs feeds one a made-up name purely so
                // the rendered banner looks like a real fixture path).
                let mut before_start = start.saturating_sub(40);
                while before_start < i && !text.is_char_boundary(before_start) {
                    before_start += 1;
                }
                let before = &text[before_start..i];
                let is_display_label = before.contains("render_diagnostics(")
                    || before.contains("render_all_colored(")
                    || before.contains("render_all_json(");
                if !is_display_label && is_hardcoded_fixture_path(lit) {
                    out.push(lit.to_string());
                }
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

fn is_hardcoded_fixture_path(lit: &str) -> bool {
    let known_prefix = lit.starts_with("examples/features/")
        || lit.starts_with("docs/spec/")
        || lit.starts_with("tests/ui/")
        || lit.starts_with("tests/ui_lint/")
        || lit.starts_with("tests/cli/")
        || lit.starts_with("tests/release/");
    if !known_prefix || lit.contains('{') || lit.contains('}') {
        return false;
    }
    let ext = format!(".{}", jet::Syntax::FILE_EXT);
    lit.ends_with(&ext)
        || lit.ends_with(".md")
        || lit.ends_with(".out")
        || lit.ends_with(".stderr")
        || lit.ends_with(".warn")
        || lit.ends_with(".txt")
        || lit.ends_with(".snapshot")
}

// ──────────────────────────────────────────────
// c450: `jet self devtools bless` — wrapper over the UPDATE_EXPECT re-bless convention.
// ──────────────────────────────────────────────

/// Every test binary that owns `UPDATE_EXPECT`-blessable snapshots (I4). Kept
/// as one list so `bless` never drifts from what actually re-blesses on
/// `UPDATE_EXPECT=1`.
pub(crate) const BLESS_TARGETS: &[&str] = &[
    "cli",
    "cross",
    "diagnostic_snapshots",
    "diagnostics_coverage",
    "release_gates",
];

/// `jet self devtools bless [target...] [--dry-run]` — runs
/// `UPDATE_EXPECT=1 cargo test --test <target>` for each named target (all of
/// `BLESS_TARGETS` when none is given). This is the one re-bless mechanism in
/// the repo (see each test file's own "bless with UPDATE_EXPECT=1" doc
/// comment) — `bless` just names and runs it so nobody has to remember the
/// env var or the target list. `--dry-run` prints the commands without
/// running them (never mutates a snapshot file).
pub(crate) fn run_devtools_bless(args: &[&String]) {
    let mut dry_run = false;
    let mut requested: Vec<String> = Vec::new();
    for a in args {
        if a.as_str() == jet::CLI::DRY_RUN_FLAG {
            dry_run = true;
        } else {
            requested.push(a.to_string());
        }
    }

    let targets = match resolve_bless_targets(&requested) {
        Ok(t) => t,
        Err(unknown) => {
            crate::cli_error!("E2104", "unknown bless target(s): {}", unknown);
            eprintln!(
                "usage: {} devtools bless [target...] [--dry-run]",
                jet::Syntax::BINARY_NAME
            );
            eprintln!("known targets: {}", BLESS_TARGETS.join(", "));
            exit(ExitCodes::USAGE);
        }
    };

    if dry_run {
        for t in &targets {
            println!("would run: UPDATE_EXPECT=1 cargo test --test {}", t);
        }
        return;
    }

    let mut any_failed = false;
    for t in &targets {
        println!("bless: UPDATE_EXPECT=1 cargo test --test {}", t);
        match bless_command(t).status() {
            Ok(s) if s.success() => {}
            Ok(s) => {
                any_failed = true;
                eprintln!("bless: `{}` exited with {}", t, s);
            }
            Err(e) => {
                any_failed = true;
                eprintln!("bless: couldn't run `cargo test --test {}`: {}", t, e);
            }
        }
    }
    if any_failed {
        exit(ExitCodes::USER_ERROR);
    }
    println!(
        "bless: done ({} target{})",
        targets.len(),
        if targets.len() == 1 { "" } else { "s" }
    );
}

/// Resolve requested target names against `BLESS_TARGETS`; empty `requested`
/// means "all of them". `Err` carries the comma-joined unknown names.
pub(crate) fn resolve_bless_targets(requested: &[String]) -> Result<Vec<&'static str>, String> {
    if requested.is_empty() {
        return Ok(BLESS_TARGETS.to_vec());
    }
    let mut out = Vec::new();
    let mut unknown = Vec::new();
    for r in requested {
        match BLESS_TARGETS.iter().find(|k| **k == r.as_str()) {
            Some(t) => out.push(*t),
            None => unknown.push(r.clone()),
        }
    }
    if !unknown.is_empty() {
        return Err(unknown.join(", "));
    }
    Ok(out)
}

/// Build (never spawns) the `UPDATE_EXPECT=1 cargo test --test <target>`
/// command for one bless target.
pub(crate) fn bless_command(target: &str) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.env("UPDATE_EXPECT", "1");
    cmd.args(["test", "--test", target]);
    cmd
}

fn write_generated_section(path: &str, fresh: &str, quiet: bool) {
    let text = fs::read_to_string(path).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't read `{}`: {}", path, e);
        exit(ExitCodes::USER_ERROR);
    });
    let start = text
        .find(jet::Syntax::HIGHLIGHT_GENERATED_START)
        .unwrap_or_else(|| {
            crate::cli_error!("E2105", "`{}` has no `{}` marker", path, jet::Syntax::HIGHLIGHT_GENERATED_START);
            exit(ExitCodes::USER_ERROR);
        });
    let prefix_start = text[..start].rfind('\n').map_or(0, |idx| idx + 1);
    let after_start = &text[start..];
    let end_rel = after_start
        .find(jet::Syntax::HIGHLIGHT_GENERATED_END)
        .unwrap_or_else(|| {
            crate::cli_error!("E2105", "`{}` has no `{}` marker", path, jet::Syntax::HIGHLIGHT_GENERATED_END);
            exit(ExitCodes::USER_ERROR);
        });
    let end_marker = start + end_rel + jet::Syntax::HIGHLIGHT_GENERATED_END.len();
    let suffix_start = text[end_marker..]
        .find('\n')
        .map_or(text.len(), |idx| end_marker + idx + 1);

    let mut out = String::new();
    out.push_str(&text[..prefix_start]);
    out.push_str(fresh.trim_end());
    out.push('\n');
    out.push_str(&text[suffix_start..]);
    fs::write(path, out).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't write `{}`: {}", path, e);
        exit(ExitCodes::USER_ERROR);
    });
    // #1659 criterion 3 (round 2): the file is still written; `--quiet` only
    // mutes this per-file progress line (same rule as the summary line below).
    if !quiet {
        println!("wrote {}", path);
    }
}

/// `jet self doctor` — environment self-diagnosis with actionable fixes (D-DX2,
/// D-BUILD1). Offline by default; `--online` enables network checks; `--fix`
/// applies the auto-fixable problems. The advisory code for rustc/cache/PATH
/// problems is L2101.
pub(crate) fn run_doctor(online: bool, apply: bool, mode: OutputMode) {
    // E2-M15: `jet self doctor --target=<triple>` checks cross-compilation readiness.
    let cross_target =
        std::env::args().find_map(|a| a.strip_prefix("--target=").map(str::to_string));
    let checks = jet::Doctor::run(jet::Doctor::Options {
        online,
        cross_target,
    });
    let color = mode.color_stderr_for(std::io::stdout().is_terminal());

    if apply {
        let fixed = jet::Doctor::apply_fixes(&checks);
        if fixed.is_empty() {
            println!("doctor: nothing to auto-fix");
        } else {
            for f in &fixed {
                println!("fixed: {}", f);
            }
        }
        // Re-run so the report reflects the world after fixes.
        return run_doctor(online, false, mode);
    }

    use jet::Doctor::Health;
    let bold = |s: &str| {
        if color {
            format!("\x1b[1m{}\x1b[0m", s)
        } else {
            s.to_string()
        }
    };
    let green = |s: &str| {
        if color {
            format!("\x1b[32m{}\x1b[0m", s)
        } else {
            s.to_string()
        }
    };
    let yellow = |s: &str| {
        if color {
            format!("\x1b[33m{}\x1b[0m", s)
        } else {
            s.to_string()
        }
    };

    println!("{}", bold(&format!("{} doctor", jet::Syntax::BINARY_NAME)));
    let mut last_section = "";
    for c in &checks {
        if c.section != last_section {
            println!();
            println!("{}", bold(c.section));
            last_section = c.section;
        }
        let (mark, label) = match c.health {
            Health::Ok => (green("ok  "), c.label.clone()),
            Health::Note => ("note".to_string(), c.label.clone()),
            Health::Problem => (yellow("warn"), c.label.clone()),
        };
        println!("  [{}] {}: {}", mark, label, c.detail);
        if let Some(fix) = &c.fix {
            println!("        Fix: {}", fix);
            if c.auto_fixable {
                println!(
                    "        (auto-fixable: run `{} doctor --fix`)",
                    jet::Syntax::BINARY_NAME
                );
            }
        }
    }
    println!();
    if jet::Doctor::has_problem(&checks) {
        println!("Warning [L2101] (doctor_advisory): toolchain checks need attention");
        println!(" Why: one or more required tools or paths are unavailable");
        println!(" Fix: follow the fixes above, then run `jet self doctor` again");
        println!(" {}", jet::Explain::pointer_line("L2101", color));
        exit(ExitCodes::USER_ERROR);
    } else {
        println!("everything looks good.");
    }
}

/// `jet explain --web-graph <file>` — print the sema-known application graph.
pub(crate) fn run_explain_web_graph(args: &[String], mode: OutputMode) {
    let mut file: Option<&str> = None;
    for arg in args {
        if arg.starts_with('-') {
            continue;
        }
        file = Some(arg.as_str());
        break;
    }
    let Some(path) = file else {
        eprintln!(
            "usage: {} explain --web-graph <file.{}>",
            jet::Syntax::BINARY_NAME,
            jet::Syntax::FILE_EXT
        );
        exit(ExitCodes::USAGE);
    };
    let abs = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let entry = abs.display().to_string();
    let (diags, _bundle, facts) =
        jet::Driver::check_file_with_effect_facts(&entry, None, false);
    let Some(graph) = facts.web_app.as_ref() else {
        if diags
            .iter()
            .any(|d| d.severity == jet::Diagnostics::Severity::Error)
        {
            for d in &diags {
                eprintln!(
                    "{}",
                    jet::render_diagnostics(&entry, "", std::slice::from_ref(d))
                );
            }
            exit(ExitCodes::USER_ERROR);
        }
        if mode.json {
            println!("{{\"web_app\": null}}");
        } else {
            println!("(none)");
        }
        return;
    };
    let empty = graph.entry_file.is_empty()
        && graph.routes.is_empty()
        && graph.actions.is_empty()
        && graph.mounts.is_empty()
        && graph.routes_from.is_empty();
    if empty {
        if diags
            .iter()
            .any(|d| d.severity == jet::Diagnostics::Severity::Error)
        {
            for d in &diags {
                eprintln!(
                    "{}",
                    jet::render_diagnostics(&entry, "", std::slice::from_ref(d))
                );
            }
            exit(ExitCodes::USER_ERROR);
        }
        if mode.json {
            println!("{{\"web_app\": null}}");
        } else {
            println!("(none)");
        }
        return;
    }
    if mode.json {
        println!("{}", graph.to_json());
    } else {
        for line in graph.explain_lines() {
            println!("{line}");
        }
    }
}

/// `jet explain <CODE|FACT> [file]` — print a diagnostic essay or one complete
/// build-fact writer chain.
pub(crate) fn run_explain(code: Option<&str>, fact_file: Option<&str>, mode: OutputMode) {
    let code = match code {
        Some(c) => c,
        None => {
            eprintln!(
                "usage: {} explain <CODE|FACT> [file]\n       {} explain marker <file>:<line> <policy-key>",
                jet::Syntax::BINARY_NAME,
                jet::Syntax::BINARY_NAME
            );
            exit(ExitCodes::USAGE);
        }
    };
    if code.eq_ignore_ascii_case("Build.Profile") || code == "@build.profile" {
        let file = fact_file
            .map(PathBuf::from)
            .or_else(|| {
                let cwd = std::env::current_dir().ok()?;
                crate::resolve_bare_entry("run", &cwd, None)
            })
            .unwrap_or_else(|| {
                crate::cli_error!("E2104", "a source file is required to explain this build fact");
                exit(ExitCodes::USAGE);
            });
        let mut bundle = jet::Loader::load_entry(file.to_string_lossy().as_ref()).unwrap_or_else(|diags| {
            for diagnostic in diags {
                eprintln!("{}", diagnostic.what);
            }
            exit(ExitCodes::USER_ERROR);
        });
        if let Err(diags) = jet::Driver::seed_build_facts(
            &mut bundle,
            "dev",
            false,
            &BTreeMap::new(),
        ) {
            for diagnostic in diags {
                eprintln!("{}", diagnostic.what);
            }
            exit(ExitCodes::USER_ERROR);
        }
        let Some(fact) = bundle.build_facts.contribution("Build.Profile") else {
            crate::cli_error!("E2104", "the selected build has no `Build.Profile` fact");
            exit(ExitCodes::USER_ERROR);
        };
        let Some(explanation) = jet::Explain::lookup_fact(fact.key.clone(), fact.provenance.clone()) else {
            crate::cli_error!(
                @full "E3521",
                "the build fact contribution chain could not be resolved",
                "the selected fact writers must pass the shared contribution law",
                "remove the conflicting writer or choose one explicit contribution"
            );
            exit(ExitCodes::USER_ERROR);
        };
        let color = ColorChoice::resolve(mode.color, std::io::stdout().is_terminal());
        print!("{}", jet::Explain::render(&explanation, color));
        return;
    }
    if let Some(key) = code.strip_prefix("build.settings.") {
        run_explain_setting(key, mode);
        return;
    }
    match jet::Explain::lookup(code) {
        Some(ex) => {
            let color = ColorChoice::resolve(mode.color, std::io::stdout().is_terminal());
            print!("{}", jet::Explain::render(&ex, color));
        }
        None => {
            crate::cli_error!(@fix "E2104", format!("no diagnostic code `{}` exists", code), format!("run a command that reports an error to see its code, e.g. `{} check file.{}`", jet::Syntax::BINARY_NAME, jet::Syntax::FILE_EXT));
            exit(ExitCodes::USER_ERROR);
        }
    }
}

fn run_explain_setting(key: &str, mode: OutputMode) {
    if key.is_empty() || key.contains('.') {
        crate::emit_cli_report(
            "E0302",
            format!("`build.settings.{key}` is not a declared setting"),
            "jet explain names declared package settings and their writers".to_string(),
            "use `jet explain build.settings.<name>` for one declared setting".to_string(),
            mode.json,
        );
        exit(ExitCodes::USER_ERROR);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let Some(root) = jet::Loader::find_manifest_root(&cwd) else {
        crate::emit_cli_report(
            "E0302",
            format!("`build.settings.{key}` is undeclared"),
            "settings are declared in the package manifest".to_string(),
            format!("add `{key}: Type = default` to `package.jet`"),
            mode.json,
        );
        exit(ExitCodes::USER_ERROR);
    };
    let Some(manifest_path) = jet::Loader::manifest_path(&root) else {
        crate::emit_cli_report(
            "E0302",
            format!("`build.settings.{key}` is undeclared"),
            "settings are declared in the package manifest".to_string(),
            format!("add `{key}: Type = default` to `package.jet`"),
            mode.json,
        );
        exit(ExitCodes::USER_ERROR);
    };
    let manifest = match jet::Package::PackageFacts::load(&root) {
        Some(Ok(manifest)) => manifest,
        Some(Err(error)) => {
            crate::emit_cli_report(
                "E1206",
                "package manifest is not valid".to_string(),
                error.to_string(),
                "fix `package.jet` before explaining a setting".to_string(),
                mode.json,
            );
            exit(ExitCodes::USER_ERROR);
        }
        None => unreachable!("manifest path was found but package facts were absent"),
    };
    let Some(declaration) = manifest.settings.get(key) else {
        crate::emit_cli_report(
            "E0302",
            format!("`build.settings.{key}` is undeclared"),
            format!("`{}` contains no declaration for this setting", manifest_path.display()),
            format!("add `{key}: Type = default` to `{}`", manifest_path.display()),
            mode.json,
        );
        exit(ExitCodes::USER_ERROR);
    };
    let profiles = manifest
        .build_profiles
        .iter()
        .filter_map(|profile| {
            profile
                .settings
                .get(key)
                .map(|value| (profile.name.as_str(), value.as_str()))
        })
        .collect::<Vec<_>>();
    let cli = format!("--set {key}=<value>");
    if mode.json {
        let profile_json = profiles
            .iter()
            .map(|(name, value)| {
                format!(
                    "{{\"name\":\"{}\",\"value\":\"{}\"}}",
                    json_escape(name),
                    json_escape(value)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{{\"setting\":\"{}\",\"type\":\"{}\",\"default\":\"{}\",\"cli\":\"{}\",\"profiles\":[{}]}}",
            json_escape(key),
            json_escape(&declaration.ty),
            json_escape(&declaration.default),
            json_escape(&cli),
            profile_json,
        );
    } else {
        println!("build.settings.{key}");
        println!("  CLI: {cli}");
        for (name, value) in profiles {
            println!("  profile.{name}: {value}");
        }
        println!("  default: {} = {}", declaration.ty, declaration.default);
    }
}

/// D-MARK-SCOPE1: `jet explain marker <file>:<line> <policy-key>`.
pub(crate) fn run_explain_marker(site: Option<&str>, key: Option<&str>, mode: OutputMode) {
    let (Some(site), Some(key)) = (site, key) else {
        eprintln!("usage: {} explain marker <file>:<line> <policy-key>", jet::Syntax::BINARY_NAME);
        exit(ExitCodes::USAGE);
    };
    let Some((file, line_text)) = site.rsplit_once(':') else {
        crate::cli_error!("E2104", "marker site must be `<file>:<line>`"); exit(ExitCodes::USAGE);
    };
    let line = line_text.parse::<usize>().ok().filter(|line| *line > 0).unwrap_or_else(|| { crate::cli_error!("E2104", "marker line must be a positive number"); exit(ExitCodes::USAGE) });
    let Some(policy_key) = jet::Policy::PolicyKey::parse(key) else { crate::cli_error!("E2104", "`{key}` is not a registered scoped policy"); exit(ExitCodes::USER_ERROR) };
    let bundle = jet::Loader::load_entry(file).unwrap_or_else(|diags| { for diag in diags { eprintln!("{}", diag.what); } exit(ExitCodes::USER_ERROR) });
    let module = &bundle.modules[0];
    let offset = if line == 1 { 0 } else { module.source.match_indices('\n').nth(line - 2).map(|(at, _)| at + 1).unwrap_or(module.source.len()) };
    let declarations = module.policy_declarations.iter().filter(|declaration| match declaration.scope {
        jet::Policy::PolicyScope::Organization | jet::Policy::PolicyScope::Package | jet::Policy::PolicyScope::Module => true,
        jet::Policy::PolicyScope::Function | jet::Policy::PolicyScope::Block => declaration.target.is_some_and(|target| target.start <= offset && offset <= target.end),
    }).cloned().collect::<Vec<_>>();
    let Some(explanation) = jet::Explain::lookup_policy(policy_key, declarations) else { crate::cli_error!("E2104", "`{key}` has no effective declaration at {site}"); exit(ExitCodes::USER_ERROR) };
    let color = ColorChoice::resolve(mode.color, std::io::stdout().is_terminal());
    print!("{}", jet::Explain::render(&explanation, color));
}

/// `jet inspect bind <header.h> [--pkg <lib>] [-o <out.jet>]` (S59 / E2-M14 Phase 4).
///
/// Generates a `#Bindgen module c.<lib>.__bindgen__` cache from a C header,
/// using the same native std-only backend the compiler invokes on a cache miss
/// (owner 2026-06-18, supersedes D-CBIND3=B). Parses C function prototypes
/// over the bindable type subset; skips and reports what it cannot map (I3).
/// **E3208** fires only when the header is unreadable or has no bindable
/// prototypes — use `#Extern module c.<lib>` for those declarations.
pub(crate) fn run_bind(args: &[&String]) {
    if matches!(
        args.first().map(|arg| arg.as_str()),
        Some("json" | "csv" | "sql" | "xml" | "proto")
    ) {
        let format = args[0].as_str();
        run_data_bind(format, &args[1..]);
        return;
    }
    if args.first().is_some_and(|arg| arg.as_str() == jet::Syntax::CPP_MODULE_ROOT) {
        run_cpp_bind(&args[1..]);
        return;
    }
    if args.first().is_some_and(|arg|arg.as_str()==jet::Syntax::COM_MODULE_ROOT){run_com_bind(&args[1..]);return;}
    if args.first().is_some_and(|arg|arg.as_str()==jet::Syntax::COBOL_MODULE_ROOT){run_cobol_bind(&args[1..]);return;}
    if args.first().is_some_and(|arg|arg.as_str()==jet::Syntax::PERL_MODULE_ROOT){run_perl_bind(&args[1..]);return;}
    if args.first().is_some_and(|arg|arg.as_str()==jet::Syntax::RUBY_MODULE_ROOT){run_ruby_bind(&args[1..]);return;}
    if args.first().is_some_and(|arg|arg.as_str()==jet::Syntax::PHP_MODULE_ROOT){run_php_bind(&args[1..]);return;}
    if args.first().is_some_and(|arg|arg.as_str()==jet::Syntax::R_MODULE_ROOT){run_r_bind(&args[1..]);return;}
    if args.first().is_some_and(|arg|arg.as_str()==jet::Syntax::PWSH_MODULE_ROOT){run_powershell_bind(&args[1..]);return;}
    if args.first().is_some_and(|arg|arg.as_str()==jet::Syntax::DART_MODULE_ROOT){run_dart_bind(&args[1..]);return;}
    if args.first().is_some_and(|arg|arg.as_str()==jet::Syntax::PASCAL_MODULE_ROOT){run_pascal_bind(&args[1..]);return;}
    if args.first().is_some_and(|arg|arg.as_str()==jet::Syntax::ADA_MODULE_ROOT){run_ada_bind(&args[1..]);return;}
    if args.first().is_some_and(|arg|arg.as_str()==jet::Syntax::TCL_MODULE_ROOT){run_tcl_bind(&args[1..]);return;}
    if args.first().is_some_and(|arg|arg.as_str()==jet::Syntax::LUA_MODULE_ROOT){run_lua_bind(&args[1..]);return;}
    if args.first().is_some_and(|arg| arg.as_str() == jet::Syntax::JAVA_MODULE_ROOT) {
        run_java_bind(&args[1..]);
        return;
    }
    if args.first().is_some_and(|arg| arg.as_str() == jet::Syntax::CS_MODULE_ROOT) {
        run_dotnet_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::GO_MODULE_ROOT)
    {
        run_go_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::FORTRAN_MODULE_ROOT)
    {
        run_fortran_bind(&args[1..]);
        return;
    }
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        eprintln!(
            "usage: {} inspect bind <header.h> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        );
        eprintln!(
            "       {} inspect bind <json|csv|sql|xml|proto> <input> [--type <Type>] [-o <output>]",
            jet::Syntax::BINARY_NAME
        );
        eprintln!();
        eprintln!("Generate a C binding cache from a header (S59). The output is");
        eprintln!("a `#Bindgen module c.<lib>.__bindgen__` file, by default written");
        eprintln!("to .jet/bindings/c/<lib>.jet. The compiler also runs this");
        eprintln!("automatically on a cache miss; `jet inspect bind` is the manual refresh.");
        exit(if args.is_empty() { ExitCodes::USAGE } else { ExitCodes::OK });
    }

    let header = args[0].as_str();
    let mut pkg: Option<String> = None;
    let mut out: Option<String> = None;
    let mut quiet = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                let Some(value) = args.get(i + 1) else {
                    crate::cli_error!("E2102", "`inspect bind` requires a value after `--pkg`");
                    exit(ExitCodes::USAGE);
                };
                pkg = Some(value.to_string());
                i += 2;
            }
            "-o" | "--out" => {
                let Some(value) = args.get(i + 1) else {
                    crate::cli_error!("E2102", "`inspect bind` requires a value after `{}`", args[i]);
                    exit(ExitCodes::USAGE);
                };
                out = Some(value.to_string());
                i += 2;
            }
            // #1659 c3: --quiet suppresses the `bound …` status line; the
            // skipped-declaration notice stays (it is a warning, not status).
            "--quiet" => {
                quiet = true;
                i += 1;
            }
            other => {
                crate::cli_error!("E2102", "unknown `inspect bind` flag `{}`", other);
                eprintln!(
                    "usage: {} inspect bind <header.h> [--pkg <lib>] [-o <out.jet>]",
                    jet::Syntax::BINARY_NAME
                );
                exit(ExitCodes::USAGE);
            }
        }
    }

    // Link key: --pkg if given, else the header basename (header→lib rule).
    let raw_lib = pkg.unwrap_or_else(|| {
        let base = header.rsplit('/').next().unwrap_or(header);
        base.strip_suffix(".h").unwrap_or(base).to_string()
    });
    let lib = jet::Syntax::sanitize_generated_name(
        &raw_lib,
        jet::Syntax::NameCase::Snake,
        "library",
    );

    let header_src = match std::fs::read_to_string(header) {
        Ok(s) => s,
        Err(e) => bind_e3208(
            format!("Could not generate bindings from `{header}`."),
            format!("the header file could not be read ({e})."),
            "check the path, or install the library's dev headers.".to_string(),
        ),
    };

    // E2-M14 (owner 2026-06-18, supersedes D-CBIND3=B): native std-only backend.
    let result = match jet::CBind::generate(&header_src, &lib) {
        Ok(r) => r,
        Err(why) => bind_e3208(
            format!("Could not generate bindings from `{header}`."),
            format!("{why}."),
            format!("hand-write `#Extern module c.{lib} {{ … }}` for the symbols you need."),
        ),
    };

    // Default cache path follows D-CBIND7: .jet/bindings/c/<lib>.jet.
    let out_path =
        out.unwrap_or_else(|| format!(".jet/bindings/c/{}.{}", lib, jet::Syntax::FILE_EXT));
    if let Some(parent) = std::path::Path::new(&out_path).parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            crate::cli_error!("E2105", "could not create `{}`: {}", parent.display(), e);
            exit(ExitCodes::USER_ERROR);
        }
    }
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        crate::cli_error!("E2105", "could not write `{}`: {}", out_path, e);
        exit(ExitCodes::USER_ERROR);
    }

    // Phase 3 (D-CBIND2): write a hash sidecar alongside the cache so the
    // compiler can detect stale caches on the next build (hash invalidation).
    // cflags are not yet threaded through `jet inspect bind`; pass "" for now.
    let _ = jet::CBind::write_bind_hash(std::path::Path::new(&out_path), &header_src, "");

    if !quiet {
        println!(
            "bound {} function{} from `{}` → {}",
            result.bound.len(),
            if result.bound.len() == 1 { "" } else { "s" },
            header,
            out_path
        );
    }
    if !result.skipped.is_empty() {
        println!(
            "skipped {} declaration{} outside the bindable subset (hand-write `#Extern` for these):",
            result.skipped.len(),
            if result.skipped.len() == 1 { "" } else { "s" }
        );
        for (name, why) in &result.skipped {
            println!("  - {} — {}", name, why);
        }
    }
}

fn bind_e3208(what: String, why: String, fix: String) -> ! {
    crate::emit_cli_report("E3208", what, why, fix, false);
    exit(ExitCodes::USER_ERROR);
}

fn run_data_bind(format: &str, args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind {} <input> [--type <Type>] [-o <output>]",
            jet::Syntax::BINARY_NAME,
            format
        );
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        });
    }
    let input_path = args[0].as_str();
    let mut root_type = None;
    let mut output = None;
    let mut index = 1usize;
    while index < args.len() {
        match args[index].as_str() {
            "--type" => {
                let Some(value) = args.get(index + 1) else {
                    crate::cli_error!("E2102", "inspect bind {format} requires a value after --type");
                    usage();
                    exit(ExitCodes::USAGE);
                };
                if value.is_empty() {
                    crate::cli_error!("E2102", "inspect bind {format} type name cannot be empty");
                    usage();
                    exit(ExitCodes::USAGE);
                }
                root_type = Some(value.to_string());
                index += 2;
            }
            "-o" | "--out" => {
                let Some(value) = args.get(index + 1) else {
                    crate::cli_error!("E2102", "inspect bind {format} requires a value after -o");
                    usage();
                    exit(ExitCodes::USAGE);
                };
                if value.is_empty() {
                    crate::cli_error!("E2102", "inspect bind {format} output path cannot be empty");
                    usage();
                    exit(ExitCodes::USAGE);
                }
                output = Some(value.to_string());
                index += 2;
            }
            other => {
                crate::cli_error!("E2102", "unknown inspect bind {format} argument {other}");
                usage();
                exit(ExitCodes::USAGE);
            }
        }
    }
    let input = match fs::read_to_string(input_path) {
        Ok(input) => input,
        Err(error) => data_bind_io_error(
            format,
            input_path,
            "read",
            &format!("the input could not be read ({error})"),
        ),
    };
    let default_output = output.is_none();
    let output_path = output.clone().unwrap_or_else(|| {
        let base = input_path
            .rsplit(|ch| ch == '/' || ch == '\\')
            .next()
            .unwrap_or(input_path);
        let stem = base
            .rsplit_once('.')
            .map(|(stem, _)| stem)
            .unwrap_or(base);
        let safe = jet::Syntax::sanitize_generated_name(
            stem,
            jet::Syntax::NameCase::Snake,
            "schema",
        );
        format!("bindings/{safe}.{}", jet::Syntax::FILE_EXT)
    });
    let mut command = vec![
        jet::Syntax::BINARY_NAME.to_string(),
        "inspect".to_string(),
        "bind".to_string(),
        format.to_string(),
        input_path.to_string(),
    ];
    if let Some(root_type) = &root_type {
        command.push("--type".to_string());
        command.push(root_type.clone());
    }
    if let Some(output) = &output {
        command.push("-o".to_string());
        command.push(output.clone());
    }
    let command = command.join(" ");
    let result = match jet::CBind::generate_data(
        format,
        input_path,
        &input,
        root_type.as_deref(),
        &command,
    ) {
        Ok(result) => result,
        Err(error) => data_bind_error(format, input_path, &error),
    };
    if default_output {
        match fs::read_to_string(&output_path) {
            Ok(existing) => {
                let generated_command = result
                    .source
                    .lines()
                    .find(|line| line.starts_with("// generated by: "));
                let existing_command = existing
                    .lines()
                    .find(|line| line.starts_with("// generated by: "));
                if generated_command != existing_command {
                    data_bind_output_collision(format, input_path, &output_path);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => data_bind_io_error(
                format,
                &output_path,
                "inspect",
                &format!("the existing output could not be checked ({error})"),
            ),
        }
    }
    if let Some(parent) = Path::new(&output_path).parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(error) = fs::create_dir_all(parent) {
                data_bind_io_error(
                    format,
                    &output_path,
                    "create",
                    &format!("could not create output directory {} ({error})", parent.display()),
                );
            }
        }
    }
    if let Err(error) = fs::write(&output_path, result.source) {
        data_bind_io_error(
            format,
            &output_path,
            "write",
            &format!("could not write {output_path} ({error})"),
        );
    }
    println!(
        "bound {} {} record{} from {input_path} → {output_path}",
        result.record_count,
        format,
        if result.record_count == 1 { "" } else { "s" }
    );
}

fn data_bind_error(format: &str, path: &str, why: &str) -> ! {
    bind_e3208(
        format!("Could not generate {format} bindings from {path}."),
        format!("{why}."),
        format!("provide a well-formed {format} schema and rerun `jet inspect bind {format}`."),
    )
}

fn data_bind_io_error(format: &str, path: &str, operation: &str, why: &str) -> ! {
    crate::emit_cli_report(
        "E2105",
        format!("Could not {operation} {format} bindings at {path}."),
        format!("{why}."),
        format!("check the input and output paths, permissions, and available disk space, then rerun `jet inspect bind {format}`."),
        false,
    );
    exit(ExitCodes::USER_ERROR);
}

fn data_bind_output_collision(format: &str, input_path: &str, output_path: &str) -> ! {
    crate::emit_cli_report(
        "E2104",
        format!("the default {format} binding output `{output_path}` is already in use"),
        format!("sanitizing `{input_path}` produces the same output name as an existing binding"),
        format!("rerun with `-o <different-output>` to choose an explicit path"),
        false,
    );
    exit(ExitCodes::USER_ERROR);
}

fn run_cpp_bind(args: &[&String]) {
    let usage = || eprintln!("usage: {} inspect bind cpp <header.hpp> --target <triple> --clang <absolute-path> --ar <absolute-path> [--pkg <lib>] [--namespace <name>] [--instantiate <qualified=type:jet-name>] [-I <dir>] [-L <dir>] [-l <lib>] [-o <out.jet>]", jet::Syntax::BINARY_NAME);
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() { ExitCodes::USAGE } else { ExitCodes::OK });
    }
    let header = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut target = None;
    let mut clang = None;
    let mut archiver = None;
    let mut include_dirs = Vec::new();
    let mut library_dirs = Vec::new();
    let mut libraries = Vec::new();
    let mut namespaces = Vec::new();
    let mut templates = Vec::new();
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--pkg" => { pkg = args.get(index + 1).map(|v| v.to_string()); if pkg.is_none() { usage(); exit(ExitCodes::USAGE); } index += 2; }
            "-o" | "--out" => { out = args.get(index + 1).map(|v| v.to_string()); if out.is_none() { usage(); exit(ExitCodes::USAGE); } index += 2; }
            "--target" => { target = args.get(index + 1).map(|v| v.to_string()); if target.is_none() { usage(); exit(ExitCodes::USAGE); } index += 2; }
            "--clang" => { clang = args.get(index + 1).map(|v| v.to_string()); if clang.is_none() { usage(); exit(ExitCodes::USAGE); } index += 2; }
            "--ar" => { archiver = args.get(index + 1).map(|v| v.to_string()); if archiver.is_none() { usage(); exit(ExitCodes::USAGE); } index += 2; }
            "-I" => { let Some(value) = args.get(index + 1) else { usage(); exit(ExitCodes::USAGE); }; include_dirs.push(std::path::PathBuf::from(value.as_str())); index += 2; }
            "-L" => { let Some(value) = args.get(index + 1) else { usage(); exit(ExitCodes::USAGE); }; library_dirs.push(std::path::PathBuf::from(value.as_str())); index += 2; }
            "-l" => { let Some(value) = args.get(index + 1) else { usage(); exit(ExitCodes::USAGE); }; libraries.push(value.to_string()); index += 2; }
            "--namespace" => { let Some(value) = args.get(index + 1) else { usage(); exit(ExitCodes::USAGE); }; namespaces.push(value.to_string()); index += 2; }
            "--instantiate" => {
                let Some(value) = args.get(index + 1) else { usage(); exit(ExitCodes::USAGE); };
                let Some((qualified_name, rest)) = value.split_once('=') else { usage(); exit(ExitCodes::USAGE); };
                let Some((cpp_types, jet_name)) = rest.rsplit_once(':') else { usage(); exit(ExitCodes::USAGE); };
                templates.push(jet::CppBind::TemplateInstantiation {
                    qualified_name: qualified_name.to_string(),
                    cpp_args: cpp_types.split(',').map(str::to_string).collect(),
                    jet_name: jet_name.to_string(),
                });
                index += 2;
            }
            flag => { crate::cli_error!("E2102", "unknown `inspect bind cpp` flag `{flag}`"); usage(); exit(ExitCodes::USAGE); }
        }
    }
    let (Some(target), Some(clang), Some(archiver)) = (target, clang, archiver) else { usage(); exit(ExitCodes::USAGE); };
    let lib = pkg.unwrap_or_else(|| { let base = header.rsplit('/').next().unwrap_or(header); base.rsplit_once('.').map(|v| v.0).unwrap_or(base).to_ascii_lowercase() });
    let out_path = out.unwrap_or_else(|| format!(".jet/bindings/{}/{}.{}", jet::Syntax::CPP_MODULE_ROOT, lib, jet::Syntax::FILE_EXT));
    let cache = std::path::Path::new(&out_path).parent().unwrap_or_else(|| std::path::Path::new("."));
    let canonicalize = |path: std::path::PathBuf| std::fs::canonicalize(&path).unwrap_or_else(|error| cpp_bind_error(header, &format!("could not resolve selected native path `{}` ({error})", path.display())));
    let options = jet::CppBind::BindOptions {
        lib: lib.clone(),
        target,
        clang: canonicalize(std::path::PathBuf::from(clang)),
        archiver: canonicalize(std::path::PathBuf::from(archiver)),
        include_dirs: include_dirs.into_iter().map(canonicalize).collect(),
        library_dirs: library_dirs.into_iter().map(canonicalize).collect(),
        libraries,
        namespaces,
        templates,
    };
    let result = jet::CppBind::bind(std::path::Path::new(header), cache, &options).unwrap_or_else(|error| cpp_bind_error(header, &error.to_string()));
    if let Err(error) = std::fs::write(&out_path, &result.source) { cpp_bind_error(header, &format!("the generated cache could not be written ({error})")); }
    if let Err(error) = std::fs::write(cache.join(format!("{lib}.provenance")), &result.provenance) { cpp_bind_error(header, &format!("the provenance could not be written ({error})")); }
    println!("bound {} C++ member{} from `{header}` → {out_path}", result.bound.len(), if result.bound.len() == 1 { "" } else { "s" });
}

fn cpp_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(
        format!("Could not generate C++ bindings from `{path}`."),
        format!("{why}."),
        "select an explicit target/toolchain, expose public scalar declarations, and request templates with `--instantiate`, then rerun `jet inspect bind cpp`.".to_string(),
    )
}

fn run_tcl_bind(args:&[&String]){let usage=||eprintln!("usage: {} inspect bind tcl <script.tcl> [--pkg <lib>] [-o <out.jet>]",jet::Syntax::BINARY_NAME);if args.is_empty()||jet::CLI::is_help_flag(args[0]){usage();exit(if args.is_empty(){ExitCodes::USAGE}else{ExitCodes::OK})}let path=args[0].as_str();let mut pkg=None;let mut out=None;let mut i=1;while i<args.len(){match args[i].as_str(){"--pkg"=>{pkg=args.get(i+1).map(|v|v.to_string());if pkg.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},"-o"|"--out"=>{out=args.get(i+1).map(|v|v.to_string());if out.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},flag=>{crate::cli_error!("E2102", "unknown `inspect bind tcl` flag `{flag}`");usage();exit(ExitCodes::USAGE)}}}let lib=pkg.unwrap_or_else(||{let b=path.rsplit('/').next().unwrap_or(path);b.rsplit_once('.').map(|v|v.0).unwrap_or(b).to_string()});let source=std::fs::read_to_string(path).unwrap_or_else(|e|tcl_bind_error(path,&format!("the script could not be read ({e})")));let out_path=out.unwrap_or_else(||format!(".jet/bindings/{}/{}.{}",jet::Syntax::TCL_MODULE_ROOT,lib,jet::Syntax::FILE_EXT));let cache=std::path::Path::new(&out_path).parent().unwrap_or_else(||std::path::Path::new("."));let result=jet::TclBind::bind(&source,&lib,cache).unwrap_or_else(|e|tcl_bind_error(path,&e.to_string()));if let Err(e)=std::fs::write(&out_path,&result.source){tcl_bind_error(path,&format!("the generated cache could not be written ({e})"))}if let Err(e)=std::fs::write(cache.join(format!("{lib}.tcl-path")),format!("{}\n",result.lib_dir.display())){tcl_bind_error(path,&format!("the Tcl runtime identity could not be written ({e})"))}if let Err(e)=std::fs::write(cache.join(format!("{lib}.provenance")),result.provenance){tcl_bind_error(path,&format!("the binding provenance could not be written ({e})"))}println!("bound in-process Tcl session from `{path}` → {out_path}")}
fn tcl_bind_error(path:&str,why:&str)->!{bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"use a valid Tcl initialization script and rerun `jet inspect bind tcl` inside the provisioned Jet environment.".to_string())}

fn run_lua_bind(args:&[&String]){
    let usage=||eprintln!("usage: {} inspect bind lua <script.lua> [--pkg <lib>] [-o <out.jet>]",jet::Syntax::BINARY_NAME);
    if args.is_empty()||jet::CLI::is_help_flag(args[0]){usage();exit(if args.is_empty(){ExitCodes::USAGE}else{ExitCodes::OK})}
    let path=args[0].as_str();let mut pkg=None;let mut out=None;let mut i=1;
    while i<args.len(){match args[i].as_str(){"--pkg"=>{pkg=args.get(i+1).map(|v|v.to_string());if pkg.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},"-o"|"--out"=>{out=args.get(i+1).map(|v|v.to_string());if out.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},flag=>{crate::cli_error!("E2102", "unknown `inspect bind lua` flag `{flag}`");usage();exit(ExitCodes::USAGE)}}}
    let lib=pkg.unwrap_or_else(||{let b=path.rsplit('/').next().unwrap_or(path);b.rsplit_once('.').map(|v|v.0).unwrap_or(b).to_string()});
    let source=std::fs::read_to_string(path).unwrap_or_else(|e|lua_bind_error(path,&format!("the script could not be read ({e})")));
    let out_path=out.unwrap_or_else(||format!(".jet/bindings/{}/{}.{}",jet::Syntax::LUA_MODULE_ROOT,lib,jet::Syntax::FILE_EXT));
    let cache=std::path::Path::new(&out_path).parent().unwrap_or_else(||std::path::Path::new("."));
    let result=jet::LuaBind::bind(std::path::Path::new(path),&source,&lib,cache).unwrap_or_else(|e|lua_bind_error(path,&e.to_string()));
    if let Err(e)=std::fs::write(&out_path,&result.source){lua_bind_error(path,&format!("the generated cache could not be written ({e})"))}
    if let Err(e)=std::fs::write(cache.join(format!("{lib}.lua-path")),format!("{}\n",result.lib_dir.display())){lua_bind_error(path,&format!("the Lua runtime identity could not be written ({e})"))}
    if let Err(e)=std::fs::write(cache.join(format!("{lib}.provenance")),result.provenance){lua_bind_error(path,&format!("the binding provenance could not be written ({e})"))}
    println!("bound {} in-process Lua function{} from `{path}` → {out_path}",result.bound.len(),if result.bound.len()==1{""}else{"s"});
}
fn lua_bind_error(path:&str,why:&str)->!{bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"define top-level `function name(input)` routines and rerun `jet inspect bind lua` inside the provisioned Jet environment.".to_string())}

/// D-FFI-ADA1=A: compile exported GNAT functions and preserve scalar subtype
/// ranges as checked Jet wrapper boundaries.
fn run_ada_bind(args:&[&String]){
    let usage=||eprintln!("usage: {} inspect bind ada <package.ads> [--pkg <lib>] [-o <out.jet>]",jet::Syntax::BINARY_NAME);
    if args.is_empty()||jet::CLI::is_help_flag(args[0]){usage();exit(if args.is_empty(){ExitCodes::USAGE}else{ExitCodes::OK})}
    let path=args[0].as_str();let mut pkg=None;let mut out=None;let mut i=1;
    while i<args.len(){match args[i].as_str(){"--pkg"=>{pkg=args.get(i+1).map(|v|v.to_string());if pkg.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},"-o"|"--out"=>{out=args.get(i+1).map(|v|v.to_string());if out.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},flag=>{crate::cli_error!("E2102", "unknown `inspect bind ada` flag `{flag}`");usage();exit(ExitCodes::USAGE)}}}
    let lib=pkg.unwrap_or_else(||{let base=path.rsplit('/').next().unwrap_or(path);base.rsplit_once('.').map(|v|v.0).unwrap_or(base).to_ascii_lowercase()});
    let spec=std::fs::read_to_string(path).unwrap_or_else(|e|ada_bind_error(path,&format!("the package spec could not be read ({e})")));
    let out_path=out.unwrap_or_else(||format!(".jet/bindings/{}/{}.{}",jet::Syntax::ADA_MODULE_ROOT,lib,jet::Syntax::FILE_EXT));let cache=std::path::Path::new(&out_path).parent().unwrap_or_else(||std::path::Path::new("."));
    let result=jet::AdaBind::bind(std::path::Path::new(path),&spec,&lib,cache).unwrap_or_else(|e|ada_bind_error(path,&e.to_string()));
    if let Err(e)=std::fs::write(&out_path,&result.source){ada_bind_error(path,&format!("the generated cache could not be written ({e})"))}
    if let Err(e)=std::fs::write(cache.join(format!("{lib}.ada-path")),format!("{}\n",result.runtime_dir.display())){ada_bind_error(path,&format!("the GNAT runtime identity could not be written ({e})"))}
    if let Err(e)=std::fs::write(cache.join(format!("{lib}.provenance")),result.provenance){ada_bind_error(path,&format!("the binding provenance could not be written ({e})"))}
    println!("bound {} GNAT export{} from `{path}` → {out_path}",result.bound.len(),if result.bound.len()==1{""}else{"s"});
}
fn ada_bind_error(path:&str,why:&str)->!{bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"export `Interfaces.C.long_long` or `Interfaces.C.double` functions with `Convention => C` and `External_Name`, then rerun `jet inspect bind ada`.".to_string())}

/// D-FFI-PASCAL1=A: compile FreePascal cdecl exports and generate bounded,
/// consuming opaque-handle wrappers for one Object Pascal class estate.
fn run_pascal_bind(args:&[&String]){
    let usage=||eprintln!("usage: {} inspect bind pascal <library.pas> [--pkg <lib>] [-o <out.jet>]",jet::Syntax::BINARY_NAME);
    if args.is_empty()||jet::CLI::is_help_flag(args[0]){usage();exit(if args.is_empty(){ExitCodes::USAGE}else{ExitCodes::OK})}
    let path=args[0].as_str();let mut pkg=None;let mut out=None;let mut i=1;while i<args.len(){match args[i].as_str(){"--pkg"=>{pkg=args.get(i+1).map(|v|v.to_string());if pkg.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},"-o"|"--out"=>{out=args.get(i+1).map(|v|v.to_string());if out.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},flag=>{crate::cli_error!("E2102", "unknown `inspect bind pascal` flag `{flag}`");usage();exit(ExitCodes::USAGE)}}}
    let lib=pkg.unwrap_or_else(||{let base=path.rsplit('/').next().unwrap_or(path);base.rsplit_once('.').map(|v|v.0).unwrap_or(base).to_ascii_lowercase()});let source=std::fs::read_to_string(path).unwrap_or_else(|e|pascal_bind_error(path,&format!("the Pascal source could not be read ({e})")));
    let out_path=out.unwrap_or_else(||format!(".jet/bindings/{}/{}.{}",jet::Syntax::PASCAL_MODULE_ROOT,lib,jet::Syntax::FILE_EXT));let cache=std::path::Path::new(&out_path).parent().unwrap_or_else(||std::path::Path::new("."));let result=jet::PascalBind::bind(std::path::Path::new(path),&source,&lib,cache).unwrap_or_else(|e|pascal_bind_error(path,&e.to_string()));
    if let Err(e)=std::fs::write(&out_path,&result.source){pascal_bind_error(path,&format!("the generated cache could not be written ({e})"))}if let Err(e)=std::fs::write(cache.join(format!("{lib}.provenance")),result.provenance){pascal_bind_error(path,&format!("the binding provenance could not be written ({e})"))}
    println!("bound {} FreePascal export{} from `{path}` → {out_path}",result.bound.len(),if result.bound.len()==1{""}else{"s"});
}
fn pascal_bind_error(path:&str,why:&str)->!{bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"export scalar cdecl routines plus `<class>_new`, `<class>_free`, and pointer-first method wrappers, then rerun `jet inspect bind pascal`.".to_string())}

/// D-FFI-DART1=A: Dart/Flutter owns the isolate. Generate and compile the
/// dart_api_dl callback bridge plus a native Jet plugin loaded by `dart:ffi`.
fn run_dart_bind(args:&[&String]){
    let usage=||eprintln!("usage: {} inspect bind dart <contract.dart> --jet <compute.jet> [--pkg <lib>] [-o <out.jet>]",jet::Syntax::BINARY_NAME);
    if args.is_empty()||jet::CLI::is_help_flag(args[0]){usage();exit(if args.is_empty(){ExitCodes::USAGE}else{ExitCodes::OK})}
    let path=args[0].as_str();let mut pkg=None;let mut out=None;let mut compute=None;let mut i=1;
    while i<args.len(){match args[i].as_str(){"--pkg"=>{pkg=args.get(i+1).map(|v|v.to_string());if pkg.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},"--jet"=>{compute=args.get(i+1).map(|v|v.to_string());if compute.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},"-o"|"--out"=>{out=args.get(i+1).map(|v|v.to_string());if out.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},flag=>{crate::cli_error!("E2102", "unknown `inspect bind dart` flag `{flag}`");usage();exit(ExitCodes::USAGE)}}}
    let Some(compute)=compute else{crate::cli_error!("E2104", "`inspect bind dart` requires `--jet <compute.jet>` so the Dart host has real native Jet code to load");usage();exit(ExitCodes::USAGE)};
    let lib=pkg.unwrap_or_else(||{let base=path.rsplit('/').next().unwrap_or(path);base.rsplit_once('.').map(|v|v.0).unwrap_or(base).to_ascii_lowercase()});
    let source=std::fs::read_to_string(path).unwrap_or_else(|e|dart_bind_error(path,&format!("the Dart contract could not be read ({e})")));
    let out_path=out.unwrap_or_else(||format!(".jet/bindings/{}/{}.{}",jet::Syntax::DART_MODULE_ROOT,lib,jet::Syntax::FILE_EXT));let cache=std::path::Path::new(&out_path).parent().unwrap_or_else(||std::path::Path::new("."));
    let mut result=jet::DartBind::bind(std::path::Path::new(path),&source,&lib,cache).unwrap_or_else(|e|dart_bind_error(path,&e.to_string()));
    if let Err(e)=std::fs::write(&out_path,&result.source){dart_bind_error(path,&format!("the generated cache could not be written ({e})"))}
    let host=cache.join(format!("{lib}_host.dart"));if let Err(e)=std::fs::write(&host,&result.host_source){dart_bind_error(path,&format!("the Dart host wrapper could not be written ({e})"))}
    let compute_source=std::fs::read_to_string(&compute).unwrap_or_else(|e|dart_bind_error(path,&format!("the Jet compute source could not be read ({e})")));result.provenance=jet::DartBind::bind_compute_provenance(&result.provenance,std::path::Path::new(&compute),&compute_source).unwrap_or_else(|e|dart_bind_error(path,&e.to_string()));
    let compiled=jet::compile_plugin(&compute).unwrap_or_else(|_|dart_bind_error(path,"the Jet compute source did not pass Jet front-end checks"));let plugin=compiled.plugin.as_ref().unwrap_or_else(||dart_bind_error(path,"the Jet compute source produced no plugin export artifact"));
    let native=jet::DartBind::build_compute(&plugin.guest_rust,&result.host_rust,compiled.ffi.as_ref(),&compiled.clinks,&lib,cache).unwrap_or_else(|e|dart_bind_error(path,&e.to_string()));
    if let Err(e)=std::fs::write(cache.join(format!("{lib}.provenance")),&result.provenance){dart_bind_error(path,&format!("the binding provenance could not be written ({e})"))}
    println!("bound {} Dart callback{} and native Jet compute `{}` from `{}` → {}",result.bound.len(),if result.bound.len()==1{""}else{"s"},native.display(),path,out_path);
}
fn dart_bind_error(path:&str,why:&str)->!{bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"mark top-level scalar Dart callbacks with `@pragma('vm:entry-point')`, pass a valid Jet plugin source with `--jet`, and rerun inside the provisioned Jet environment.".to_string())}

/// D-FFI-PWSH1=A: validate named script functions, then generate a persistent
/// PowerShell worker whose object pipeline crosses as canonical DataTree.
fn run_powershell_bind(args:&[&String]){
    let usage=||eprintln!("usage: {} inspect bind pwsh <script.ps1> [--pkg <lib>] [-o <out.jet>]",jet::Syntax::BINARY_NAME);
    if args.is_empty()||jet::CLI::is_help_flag(args[0]){usage();exit(if args.is_empty(){ExitCodes::USAGE}else{ExitCodes::OK})}
    let path=args[0].as_str();let mut pkg=None;let mut out=None;let mut i=1;while i<args.len(){match args[i].as_str(){"--pkg"=>{pkg=args.get(i+1).map(|v|v.to_string());if pkg.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},"-o"|"--out"=>{out=args.get(i+1).map(|v|v.to_string());if out.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},flag=>{crate::cli_error!("E2102", "unknown `inspect bind pwsh` flag `{flag}`");usage();exit(ExitCodes::USAGE)}}}
    let lib=pkg.unwrap_or_else(||{let base=path.rsplit('/').next().unwrap_or(path);base.rsplit_once('.').map(|v|v.0).unwrap_or(base).to_ascii_lowercase()});let source=std::fs::read_to_string(path).unwrap_or_else(|e|powershell_bind_error(path,&format!("the PowerShell script could not be read ({e})")));
    let out_path=out.unwrap_or_else(||format!(".jet/bindings/{}/{}.{}",jet::Syntax::PWSH_MODULE_ROOT,lib,jet::Syntax::FILE_EXT));let cache=std::path::Path::new(&out_path).parent().unwrap_or_else(||std::path::Path::new("."));let result=jet::PowerShellBind::bind(std::path::Path::new(path),&source,&lib,cache).unwrap_or_else(|e|powershell_bind_error(path,&e.to_string()));
    if let Err(e)=std::fs::write(&out_path,&result.source){powershell_bind_error(path,&format!("the generated cache could not be written ({e})"))}if let Err(e)=std::fs::write(cache.join(format!("{lib}.provenance")),&result.provenance){powershell_bind_error(path,&format!("the binding provenance could not be written ({e})"))}
    println!("bound {} persistent PowerShell function{} from `{path}` → {out_path}",result.bound.len(),if result.bound.len()==1{""}else{"s"});
}
fn powershell_bind_error(path:&str,why:&str)->!{bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"define named PowerShell functions with Jet-compatible identifiers and rerun `jet inspect bind pwsh` inside the provisioned Jet environment.".to_string())}

/// D-FFI-PERL1=A: compile named Perl subs into a supervised persistent worker.
fn run_perl_bind(args:&[&String]){
    let usage=||eprintln!("usage: {} inspect bind perl <script.pl> [--pkg <lib>] [-o <out.jet>]",jet::Syntax::BINARY_NAME);
    if args.is_empty()||jet::CLI::is_help_flag(args[0]){usage();exit(if args.is_empty(){ExitCodes::USAGE}else{ExitCodes::OK})}
    let path=args[0].as_str();let mut pkg=None;let mut out=None;let mut i=1;while i<args.len(){match args[i].as_str(){"--pkg"=>{pkg=args.get(i+1).map(|v|v.to_string());if pkg.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},"-o"|"--out"=>{out=args.get(i+1).map(|v|v.to_string());if out.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},flag=>{crate::cli_error!("E2102", "unknown `inspect bind perl` flag `{flag}`");usage();exit(ExitCodes::USAGE)}}}
    let lib=pkg.unwrap_or_else(||{let base=path.rsplit('/').next().unwrap_or(path);base.rsplit_once('.').map(|v|v.0).unwrap_or(base).to_ascii_lowercase()});let source=std::fs::read_to_string(path).unwrap_or_else(|e|perl_bind_error(path,&format!("the Perl script could not be read ({e})")));
    let out_path=out.unwrap_or_else(||format!(".jet/bindings/{}/{}.{}",jet::Syntax::PERL_MODULE_ROOT,lib,jet::Syntax::FILE_EXT));let cache=std::path::Path::new(&out_path).parent().unwrap_or_else(||std::path::Path::new("."));let result=jet::PerlBind::bind(std::path::Path::new(path),&source,&lib,cache).unwrap_or_else(|e|perl_bind_error(path,&e.to_string()));
    if let Err(e)=std::fs::write(&out_path,&result.source){perl_bind_error(path,&format!("the generated cache could not be written ({e})"))}if let Err(e)=std::fs::write(cache.join(format!("{lib}.provenance")),&result.provenance){perl_bind_error(path,&format!("the binding provenance could not be written ({e})"))}
    println!("bound {} persistent Perl function{} from `{path}` → {out_path}",result.bound.len(),if result.bound.len()==1{""}else{"s"});
}
fn perl_bind_error(path:&str,why:&str)->!{bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"define named main-package Perl functions with Jet-compatible identifiers and rerun `jet inspect bind perl` inside the provisioned Jet environment.".to_string())}

/// D-FFI-RUBY1=A: statically discover top-level methods and generate a
/// persistent supervised Ruby worker. Ruby source never runs during discovery.
fn run_ruby_bind(args:&[&String]){
    let usage=||eprintln!("usage: {} inspect bind ruby <script.rb> [--pkg <lib>] [-o <out.jet>]",jet::Syntax::BINARY_NAME);
    if args.is_empty()||jet::CLI::is_help_flag(args[0]){usage();exit(if args.is_empty(){ExitCodes::USAGE}else{ExitCodes::OK})}
    let path=args[0].as_str();let mut pkg=None;let mut out=None;let mut i=1;while i<args.len(){match args[i].as_str(){"--pkg"=>{pkg=args.get(i+1).map(|v|v.to_string());if pkg.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},"-o"|"--out"=>{out=args.get(i+1).map(|v|v.to_string());if out.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},flag=>{crate::cli_error!("E2102", "unknown `inspect bind ruby` flag `{flag}`");usage();exit(ExitCodes::USAGE)}}}
    let lib=pkg.unwrap_or_else(||{let base=path.rsplit('/').next().unwrap_or(path);base.rsplit_once('.').map(|v|v.0).unwrap_or(base).to_ascii_lowercase()});let source=std::fs::read_to_string(path).unwrap_or_else(|e|ruby_bind_error(path,&format!("the Ruby script could not be read ({e})")));
    let out_path=out.unwrap_or_else(||format!(".jet/bindings/{}/{}.{}",jet::Syntax::RUBY_MODULE_ROOT,lib,jet::Syntax::FILE_EXT));let cache=std::path::Path::new(&out_path).parent().unwrap_or_else(||std::path::Path::new("."));let result=jet::RubyBind::bind(std::path::Path::new(path),&source,&lib,cache).unwrap_or_else(|e|ruby_bind_error(path,&e.to_string()));
    if let Err(e)=std::fs::write(&out_path,&result.source){ruby_bind_error(path,&format!("the generated cache could not be written ({e})"))}if let Err(e)=std::fs::write(cache.join(format!("{lib}.provenance")),&result.provenance){ruby_bind_error(path,&format!("the binding provenance could not be written ({e})"))}
    println!("bound {} persistent Ruby method{} from `{path}` → {out_path}",result.bound.len(),if result.bound.len()==1{""}else{"s"});
}
fn ruby_bind_error(path:&str,why:&str)->!{bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"define top-level Ruby methods with one required positional argument and Jet-compatible names, then rerun `jet inspect bind ruby`.".to_string())}

/// D-FFI-PHP1=A: statically discover top-level functions and generate a
/// persistent supervised PHP worker pool. PHP source never runs during discovery.
fn run_php_bind(args:&[&String]){
    let usage=||eprintln!("usage: {} inspect bind php <script.php> [--pkg <lib>] [-o <out.jet>]",jet::Syntax::BINARY_NAME);
    if args.is_empty()||jet::CLI::is_help_flag(args[0]){usage();exit(if args.is_empty(){ExitCodes::USAGE}else{ExitCodes::OK})}
    let path=args[0].as_str();let mut pkg=None;let mut out=None;let mut i=1;while i<args.len(){match args[i].as_str(){"--pkg"=>{pkg=args.get(i+1).map(|v|v.to_string());if pkg.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},"-o"|"--out"=>{out=args.get(i+1).map(|v|v.to_string());if out.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},flag=>{crate::cli_error!("E2102", "unknown `inspect bind php` flag `{flag}`");usage();exit(ExitCodes::USAGE)}}}
    let lib=pkg.unwrap_or_else(||{let base=path.rsplit('/').next().unwrap_or(path);base.rsplit_once('.').map(|v|v.0).unwrap_or(base).to_ascii_lowercase()});let source=std::fs::read_to_string(path).unwrap_or_else(|e|php_bind_error(path,&format!("the PHP script could not be read ({e})")));
    let out_path=out.unwrap_or_else(||format!(".jet/bindings/{}/{}.{}",jet::Syntax::PHP_MODULE_ROOT,lib,jet::Syntax::FILE_EXT));let cache=std::path::Path::new(&out_path).parent().unwrap_or_else(||std::path::Path::new("."));let result=jet::PhpBind::bind(std::path::Path::new(path),&source,&lib,cache).unwrap_or_else(|e|php_bind_error(path,&e.to_string()));
    if let Err(e)=std::fs::write(&out_path,&result.source){php_bind_error(path,&format!("the generated cache could not be written ({e})"))}if let Err(e)=std::fs::write(cache.join(format!("{lib}.provenance")),&result.provenance){php_bind_error(path,&format!("the binding provenance could not be written ({e})"))}
    println!("bound {} PHP function{} into a four-worker pool from `{path}` → {out_path}",result.bound.len(),if result.bound.len()==1{""}else{"s"});
}
fn php_bind_error(path:&str,why:&str)->!{bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"define top-level PHP functions with one required positional argument and Jet-compatible names, then rerun `jet inspect bind php` inside the provisioned Jet environment.".to_string())}

/// D-FFI-R1=A: parse top-level function metadata without evaluating source,
/// then generate a persistent supervised R worker.
fn run_r_bind(args:&[&String]){
    let usage=||eprintln!("usage: {} inspect bind r <script.R> [--pkg <lib>] [-o <out.jet>]",jet::Syntax::BINARY_NAME);
    if args.is_empty()||jet::CLI::is_help_flag(args[0]){usage();exit(if args.is_empty(){ExitCodes::USAGE}else{ExitCodes::OK})}
    let path=args[0].as_str();let mut pkg=None;let mut out=None;let mut i=1;while i<args.len(){match args[i].as_str(){"--pkg"=>{pkg=args.get(i+1).map(|v|v.to_string());if pkg.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},"-o"|"--out"=>{out=args.get(i+1).map(|v|v.to_string());if out.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},flag=>{crate::cli_error!("E2102", "unknown `inspect bind r` flag `{flag}`");usage();exit(ExitCodes::USAGE)}}}
    let lib=pkg.unwrap_or_else(||{let base=path.rsplit('/').next().unwrap_or(path);base.rsplit_once('.').map(|v|v.0).unwrap_or(base).to_ascii_lowercase()});let source=std::fs::read_to_string(path).unwrap_or_else(|e|r_bind_error(path,&format!("the R script could not be read ({e})")));
    let out_path=out.unwrap_or_else(||format!(".jet/bindings/{}/{}.{}",jet::Syntax::R_MODULE_ROOT,lib,jet::Syntax::FILE_EXT));let cache=std::path::Path::new(&out_path).parent().unwrap_or_else(||std::path::Path::new("."));let result=jet::RBind::bind(std::path::Path::new(path),&source,&lib,cache).unwrap_or_else(|e|r_bind_error(path,&e.to_string()));
    if let Err(e)=std::fs::write(&out_path,&result.source){r_bind_error(path,&format!("the generated cache could not be written ({e})"))}if let Err(e)=std::fs::write(cache.join(format!("{lib}.provenance")),&result.provenance){r_bind_error(path,&format!("the binding provenance could not be written ({e})"))}
    println!("bound {} R function{} from `{path}` → {out_path}",result.bound.len(),if result.bound.len()==1{""}else{"s"});
}
fn r_bind_error(path:&str,why:&str)->!{bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"define top-level R functions with one required positional argument and Jet-compatible names, then rerun `jet inspect bind r`.".to_string())}

/// D-FFI-COM1=A: inspect a Windows type library and generate typed IDispatch
/// automation stubs. Non-Windows hosts reject before touching the input.
fn run_com_bind(args:&[&String]){
    let usage=||eprintln!("usage: {} inspect bind com <library.tlb> --pkg <lib>\n       {} inspect bind com --registered <guid> --major <n> --minor <n> [--lcid <n>] --pkg <lib>",jet::Syntax::BINARY_NAME,jet::Syntax::BINARY_NAME);
    if !cfg!(target_os="windows"){eprintln!("Error [E3260]: `com.*` needs a Windows host.");eprintln!(" Why: COM type libraries, apartments, and IDispatch are Windows facilities.");eprintln!(" Fix: run `jet inspect bind com` and build the COM module on a Windows host.");exit(ExitCodes::USER_ERROR)}
    if args.is_empty()||jet::CLI::is_help_flag(args[0]){usage();exit(if args.is_empty(){ExitCodes::USAGE}else{ExitCodes::OK})}
    let mut file=None;let mut guid=None;let mut major=None;let mut minor=None;let mut lcid=0u32;let mut pkg=None;let mut out=None;let mut i=0;while i<args.len(){match args[i].as_str(){"--registered"=>{guid=args.get(i+1).map(|v|v.to_string());i+=2},"--major"=>{major=args.get(i+1).and_then(|v|v.parse::<u16>().ok());i+=2},"--minor"=>{minor=args.get(i+1).and_then(|v|v.parse::<u16>().ok());i+=2},"--lcid"=>{lcid=args.get(i+1).and_then(|v|v.parse::<u32>().ok()).unwrap_or(u32::MAX);i+=2},"--pkg"=>{pkg=args.get(i+1).map(|v|v.to_string());i+=2},"-o"|"--out"=>{out=args.get(i+1).map(|v|v.to_string());i+=2},value if !value.starts_with('-')&&file.is_none()=>{file=Some(value.to_string());i+=1},_=>{usage();exit(ExitCodes::USAGE)}}}
    let Some(lib)=pkg else{usage();exit(ExitCodes::USAGE)};let input=if let Some(path)=file{if guid.is_some(){usage();exit(ExitCodes::USAGE)}jet::ComBind::TypeLibraryInput::File(path.into())}else{let(Some(guid),Some(major),Some(minor))=(guid,major,minor)else{usage();exit(ExitCodes::USAGE)};if lcid==u32::MAX{usage();exit(ExitCodes::USAGE)}jet::ComBind::TypeLibraryInput::Registered{guid,major,minor,lcid}};
    let out_path=out.unwrap_or_else(||format!(".jet/bindings/{}/{}.{}",jet::Syntax::COM_MODULE_ROOT,lib,jet::Syntax::FILE_EXT));let cache=std::path::Path::new(&out_path).parent().unwrap_or_else(||std::path::Path::new("."));let result=jet::ComBind::bind(&input,&lib,cache).unwrap_or_else(|e|com_bind_error(&e.to_string()));if let Err(e)=std::fs::write(&out_path,&result.source){com_bind_error(&format!("the generated cache could not be written ({e})"))}if let Err(e)=std::fs::write(cache.join(format!("{lib}.provenance")),&result.provenance){com_bind_error(&format!("the provenance could not be written ({e})"))}println!("bound {} typed COM member{} → {out_path}",result.methods.len(),if result.methods.len()==1{""}else{"s"});
}
fn com_bind_error(why:&str)->!{bind_e3208("Could not generate COM bindings.".to_string(),format!("{why}."),"select a registered or file-backed type library with IDispatch metadata and rerun on Windows.".to_string())}

/// D-FFI-JVM1=A: compile Java bytecode, discover its public ABI with javap,
/// then build an in-process JNI invocation bridge.
fn run_java_bind(args: &[&String]) {
    let usage = || eprintln!("usage: {} inspect bind java <source.java> [--pkg <lib>] [-o <out.jet>]", jet::Syntax::BINARY_NAME);
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) { usage(); exit(if args.is_empty(){ExitCodes::USAGE}else{ExitCodes::OK}); }
    let source_path=args[0].as_str(); let mut pkg=None; let mut out=None; let mut i=1;
    while i<args.len(){match args[i].as_str(){"--pkg"=>{pkg=args.get(i+1).map(|v|v.to_string());if pkg.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},"-o"|"--out"=>{out=args.get(i+1).map(|v|v.to_string());if out.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},flag=>{crate::cli_error!("E2102", "unknown `inspect bind java` flag `{flag}`");usage();exit(ExitCodes::USAGE)}}}
    let lib=pkg.unwrap_or_else(||{let base=source_path.rsplit('/').next().unwrap_or(source_path);base.rsplit_once('.').map(|v|v.0).unwrap_or(base).to_string()});
    let source=std::fs::read_to_string(source_path).unwrap_or_else(|e|java_bind_error(source_path,&format!("the source file could not be read ({e})")));
    let out_path=out.unwrap_or_else(||format!(".jet/bindings/{}/{}.{}",jet::Syntax::JAVA_MODULE_ROOT,lib,jet::Syntax::FILE_EXT));
    let cache=std::path::Path::new(&out_path).parent().unwrap_or_else(||std::path::Path::new("."));
    let result=jet::JavaBind::bind(std::path::Path::new(source_path),&source,&lib,cache).unwrap_or_else(|e|java_bind_error(source_path,&e.to_string()));
    if let Err(e)=std::fs::write(&out_path,&result.source){java_bind_error(source_path,&format!("the generated cache could not be written ({e})"))}
    if let Err(e)=std::fs::write(cache.join(format!("{lib}.jvm-path")),format!("{}\n",result.jvm_dir.display())){java_bind_error(source_path,&format!("the JVM runtime identity could not be written ({e})"))}
    if let Err(e)=std::fs::write(cache.join(format!("{lib}.provenance")),&result.provenance){java_bind_error(source_path,&format!("the binding provenance could not be written ({e})"))}
    println!("bound {} JVM member{} from `{}` → {}",result.bound.len(),if result.bound.len()==1{""}else{"s"},source_path,out_path);
}

fn java_bind_error(source:&str,why:&str)->!{bind_e3208(format!("Could not generate bindings from `{source}`."),format!("{why}."),"use a public Java class with one public long/double constructor and non-overloaded long/double methods, then rerun `jet inspect bind java`.".to_string())}

/// D-FFI-DOTNET1=A: reflect a C# assembly surface, then generate an in-process
/// hostfxr bridge. Managed state remains behind consuming GCHandle ownership.
fn run_dotnet_bind(args:&[&String]){
    let usage=||eprintln!("usage: {} inspect bind cs <source.cs> [--pkg <lib>] [-o <out.jet>]",jet::Syntax::BINARY_NAME);
    if args.is_empty()||jet::CLI::is_help_flag(args[0]){usage();exit(if args.is_empty(){ExitCodes::USAGE}else{ExitCodes::OK})}
    let path=args[0].as_str();let mut pkg=None;let mut out=None;let mut i=1;while i<args.len(){match args[i].as_str(){"--pkg"=>{pkg=args.get(i+1).map(|v|v.to_string());if pkg.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},"-o"|"--out"=>{out=args.get(i+1).map(|v|v.to_string());if out.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},flag=>{crate::cli_error!("E2102", "unknown `inspect bind cs` flag `{flag}`");usage();exit(ExitCodes::USAGE)}}}
    let lib=pkg.unwrap_or_else(||{let base=path.rsplit('/').next().unwrap_or(path);base.rsplit_once('.').map(|v|v.0).unwrap_or(base).to_ascii_lowercase()});let source=std::fs::read_to_string(path).unwrap_or_else(|e|dotnet_bind_error(path,&format!("the C# source could not be read ({e})")));
    let out_path=out.unwrap_or_else(||format!(".jet/bindings/{}/{}.{}",jet::Syntax::CS_MODULE_ROOT,lib,jet::Syntax::FILE_EXT));let cache=std::path::Path::new(&out_path).parent().unwrap_or_else(||std::path::Path::new("."));let result=jet::DotNetBind::bind(std::path::Path::new(path),&source,&lib,cache).unwrap_or_else(|e|dotnet_bind_error(path,&e.to_string()));
    if let Err(e)=std::fs::write(&out_path,&result.source){dotnet_bind_error(path,&format!("the generated cache could not be written ({e})"))}if let Err(e)=std::fs::write(cache.join(format!("{lib}.provenance")),&result.provenance){dotnet_bind_error(path,&format!("the provenance could not be written ({e})"))}println!("bound {} .NET member{} from `{path}` → {out_path}",result.bound.len(),if result.bound.len()==1{""}else{"s"});
}
fn dotnet_bind_error(path:&str,why:&str)->!{bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"use one public C# class with one public long/double constructor and non-overloaded public long/double methods, then rerun `jet inspect bind cs`.".to_string())}

/// D-FFI-GO1=A: compile exported scalar Go functions and move-only `uintptr`
/// handles into an in-process c-archive and emit a typed `go.<lib>` Jet module.
fn run_go_bind(args: &[&String]) {
    let usage = || eprintln!("usage: {} inspect bind go <source.go> [--pkg <lib>] [-o <out.jet>]", jet::Syntax::BINARY_NAME);
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        eprintln!();
        eprintln!("Generate typed Jet bindings for scalar and uintptr //export Go functions.");
        exit(if args.is_empty() { ExitCodes::USAGE } else { ExitCodes::OK });
    }
    let source_path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => { pkg = args.get(i + 1).map(|value| value.to_string()); if pkg.is_none() { usage(); exit(ExitCodes::USAGE); } i += 2; }
            "-o" | "--out" => { out = args.get(i + 1).map(|value| value.to_string()); if out.is_none() { usage(); exit(ExitCodes::USAGE); } i += 2; }
            flag => { crate::cli_error!("E2102", "unknown `inspect bind go` flag `{flag}`"); usage(); exit(ExitCodes::USAGE); }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let base = source_path.rsplit('/').next().unwrap_or(source_path);
        base.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(base).to_string()
    });
    let source = std::fs::read_to_string(source_path).unwrap_or_else(|error| go_bind_error(source_path, &format!("the source file could not be read ({error})")));
    let out_path = out.unwrap_or_else(|| format!(".jet/bindings/{}/{}.{}", jet::Syntax::GO_MODULE_ROOT, lib, jet::Syntax::FILE_EXT));
    let cache_dir = std::path::Path::new(&out_path).parent().unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::GoBind::bind(std::path::Path::new(source_path), &source, &lib, cache_dir)
        .unwrap_or_else(|error| go_bind_error(source_path, &error.to_string()));
    if let Err(error) = std::fs::write(&out_path, &result.source) { go_bind_error(source_path, &format!("the generated cache could not be written ({error})")); }
    println!("bound {} Go export{} from `{}` → {}", result.bound.len(), if result.bound.len() == 1 { "" } else { "s" }, source_path, out_path);
}

fn go_bind_error(source: &str, why: &str) -> ! {
    bind_e3208(
        format!("Could not generate bindings from `{source}`."),
        format!("{why}."),
        "export `int64`/`float64` scalars or move-only `uintptr` handles with `//export Name`, then rerun `jet inspect bind go`.".to_string(),
    )
}

/// D-FFI-FORTRAN1=A: discover scalar and fixed-shape ISO_C_BINDING functions, compile them
/// with the provisioned gfortran toolchain, and emit a typed `fortran.<lib>`
/// Jet module backed by the shared C ABI linker.
fn run_fortran_bind(args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind fortran <source.f90> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        );
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        eprintln!();
        eprintln!("Generate typed Jet bindings for scalar and fixed-shape input ISO_C_BINDING functions.");
        exit(if args.is_empty() { ExitCodes::USAGE } else { ExitCodes::OK });
    }

    let source_path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|value| value.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE);
                }
                i += 2;
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|value| value.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE);
                }
                i += 2;
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind fortran` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE);
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let base = source_path.rsplit('/').next().unwrap_or(source_path);
        base.rsplit_once('.')
            .map(|(stem, _)| stem)
            .unwrap_or(base)
            .to_string()
    });
    let source = match std::fs::read_to_string(source_path) {
        Ok(source) => source,
        Err(error) => fortran_bind_error(
            source_path,
            &format!("the source file could not be read ({error})"),
        ),
    };
    let out_path = out.unwrap_or_else(|| format!(
        ".jet/bindings/{}/{}.{}",
        jet::Syntax::FORTRAN_MODULE_ROOT,
        lib,
        jet::Syntax::FILE_EXT
    ));
    let cache_dir = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = match jet::FortranBind::bind(
        std::path::Path::new(source_path),
        &source,
        &lib,
        cache_dir,
    ) {
        Ok(result) => result,
        Err(error) => fortran_bind_error(source_path, &error.to_string()),
    };
    if let Err(error) = std::fs::write(&out_path, &result.source) {
        fortran_bind_error(
            source_path,
            &format!("the generated cache could not be written ({error})"),
        );
    }
    println!(
        "bound {} ISO_C_BINDING routine{} from `{}` → {}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" },
        source_path,
        out_path
    );
    for layout in &result.layouts {
        println!(
            "  layout: {}.{} {} {}",
            layout.routine,
            layout.parameter,
            layout.order,
            layout
                .extents
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join("x")
        );
    }
}

fn fortran_bind_error(source: &str, why: &str) -> ! {
    bind_e3208(
        format!("Could not generate bindings from `{source}`."),
        format!("{why}."),
        "use explicit `bind(C, name=\"...\")` routines with ISO_C_BINDING scalar `value` inputs or fixed-shape `intent(in)` arrays, then rerun `jet inspect bind fortran`.".to_string(),
    )
}

fn run_cobol_bind(args: &[&String]) {
    let usage = || eprintln!("usage: {} inspect bind cobol <program.cob> --copybook <record.cpy> [--pkg <lib>] [-o <out.jet>]", jet::Syntax::BINARY_NAME);
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) { usage(); exit(if args.is_empty(){ExitCodes::USAGE}else{ExitCodes::OK}) }
    let source_path=args[0].as_str(); let mut copybook=None; let mut pkg=None; let mut out=None; let mut i=1;
    while i<args.len(){match args[i].as_str(){"--copybook"=>{copybook=args.get(i+1).map(|v|v.to_string());if copybook.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},"--pkg"=>{pkg=args.get(i+1).map(|v|v.to_string());if pkg.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},"-o"|"--out"=>{out=args.get(i+1).map(|v|v.to_string());if out.is_none(){usage();exit(ExitCodes::USAGE)}i+=2},flag=>{crate::cli_error!("E2102", "unknown `inspect bind cobol` flag `{flag}`");usage();exit(ExitCodes::USAGE)}}}
    let Some(copybook_path)=copybook else{usage();exit(ExitCodes::USAGE)};
    let lib=pkg.unwrap_or_else(||{let b=source_path.rsplit('/').next().unwrap_or(source_path);b.rsplit_once('.').map(|v|v.0).unwrap_or(b).to_ascii_lowercase().replace('-',"_")});
    let source=std::fs::read_to_string(source_path).unwrap_or_else(|e|cobol_bind_error(source_path,&format!("the program could not be read ({e})")));
    let copybook=std::fs::read_to_string(&copybook_path).unwrap_or_else(|e|cobol_bind_error(&copybook_path,&format!("the copybook could not be read ({e})")));
    let out_path=out.unwrap_or_else(||format!(".jet/bindings/{}/{}.{}",jet::Syntax::COBOL_MODULE_ROOT,lib,jet::Syntax::FILE_EXT));
    let cache=std::path::Path::new(&out_path).parent().unwrap_or_else(||std::path::Path::new("."));
    let result=jet::CobolBind::bind(std::path::Path::new(source_path),&source,std::path::Path::new(&copybook_path),&copybook,&lib,cache).unwrap_or_else(|e|cobol_bind_error(source_path,&e.to_string()));
    if let Err(e)=std::fs::write(&out_path,&result.source){cobol_bind_error(source_path,&format!("the generated cache could not be written ({e})"))}
    if let Err(e)=std::fs::write(cache.join(format!("{lib}.cobol-path")),format!("{}\n",result.runtime_dir.display())){cobol_bind_error(source_path,&format!("the libcob runtime identity could not be written ({e})"))}
    println!("bound GnuCOBOL program `{}` and {}-byte copybook `{}` → {out_path}",result.program,result.layout.width,result.layout.name);
    for field in &result.layout.fields{println!("  layout: {} offset={} width={} type={}",field.name,field.offset,field.width,field.kind.jet_type())}
}

fn cobol_bind_error(path:&str,why:&str)->!{
    bind_e3208(
        format!("Could not generate bindings from `{path}`."),
        format!("{why}."),
        "use one GnuCOBOL PROGRAM-ID with a level-01 copybook containing level-05 X(n), COMP-5, or COMP-3 fields, then rerun `jet inspect bind cobol`.".to_string(),
    )
}

/// S60 / D-PURE1 (E2-M16): evaluate a `pure fn run()` program.
/// D-EVAL1=A: pretty output by default; `--json` for stable machine JSON.
///
/// When `--pure` is given, the entire call graph from `run` is checked for
/// purity violations (E3401 with the full transitive chain, not just the
/// direct callee). This replaces the old hand-rolled `impure_fns` check.
pub(crate) fn run_eval(file: &str, pure_required: bool, mode: OutputMode) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            crate::cli_error!("E2105", "couldn't read `{}`: {}", file, e);
            exit(ExitCodes::USER_ERROR);
        }
    };

    // Lex + parse (needed for purity walk and for eval).
    let (toks, lex_diags) = jet::Lexer::lex(&src);
    if !lex_diags.is_empty() {
        eprint!(
            "{}",
            jet::render_all_colored(file, &src, &lex_diags, mode.color_stderr())
        );
        exit(ExitCodes::USER_ERROR);
    }
    let prog = match jet::Parser::parse(&toks) {
        Ok(p) => p,
        Err(ds) => {
            eprint!(
                "{}",
                jet::render_all_colored(file, &src, &ds, mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    };

    // Transitive purity check via the real sema root-walker (E3401 with full
    // call chain). Replaces the old hand-rolled top-level `is_pure` scan.
    if pure_required {
        // Build a FuncSig map from the parsed program for purity flags.
        use std::collections::HashMap;
        let mut funcs_sig: HashMap<String, jet::Sema::FuncSig> = HashMap::new();
        let mut ast_funcs: HashMap<String, &jet::AST::Func> = HashMap::new();
        for item in &prog.items {
            if let jet::AST::Item::Func(f) = item {
                funcs_sig.insert(
                    f.name.clone(),
                    jet::Sema::FuncSig {
                        params: f
                            .params
                            .iter()
                            .map(|p| (p.convention.clone(), p.ty.clone()))
                            .collect(),
                        root_param: f.params.first().is_some_and(|p| p.root),
                        return_type: f.return_type.clone(),
                        return_view_provenance: f
                            .return_view_provenance
                            .clone()
                            .map(|provenance| {
                                let cell = jet::AST::ViewProvenanceCell::new();
                                cell.set(provenance);
                                cell
                            })
                            .unwrap_or_default(),
                        is_extern: false,
                        is_c_abi: false,
                        c_abi_name: None,
                        foreign_effect_root: None,
                        undo: None,
                        is_unsafe: f.is_unsafe,
                        is_pure: f.is_pure,
                        memo_bound: None,
                        is_foreign_thread_safe: false,
                        is_sanitizer: f.is_sanitizer,
                        is_must_use: f.is_must_use,
                        param_info: f
                            .params
                            .iter()
                            .map(|p| (p.name.clone(), p.default.is_some()))
                            .collect(),
                        param_call: f
                            .params
                            .iter()
                            .map(|p| (p.call_label().to_string(), p.zone))
                            .collect(),
                        defaults: f
                            .params
                            .iter()
                            .map(|p| p.default.as_ref().map(|d| *d.clone()))
                            .collect(),
                        param_variadic: f.params.iter().map(|p| p.variadic).collect(),
                        variadic_bounds: f
                            .params
                            .last()
                            .and_then(|p| p.variadic_bound_list.clone()),
                        param_view_from_names: f
                            .params
                            .iter()
                            .map(|p| p.declared_view_from_names.clone())
                            .collect(),
                        callable_policies: Default::default(),
                    },
                );
                ast_funcs.insert(f.name.clone(), f);
            }
        }
        let diags = jet::check_pure_program_root("run", &funcs_sig, &ast_funcs);
        if !diags.is_empty() {
            eprint!(
                "{}",
                jet::render_all_colored(file, &src, &diags, mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    }

    // Full sema type-check with CompileMode::Eval — runs all type/ownership
    // checks and accepts value-returning `pure fn run() => T`
    // is accepted. This ensures type errors (e.g. `"string" + 5`) surface with
    // their precise diagnostics rather than falling through to E0956.
    {
        let type_diags = jet::check_for_eval(&src, file);
        if !type_diags.is_empty() {
            eprint!(
                "{}",
                jet::render_all_colored(file, &src, &type_diags, mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    }

    // Evaluate via comptime and render. D-EVAL1=A: pretty by default, JSON with --json.
    match jet::eval_pure_program_value(&src, file) {
        Ok(value) => {
            if mode.json {
                println!("{}", value.to_json());
            } else {
                println!("{}", value.render_pretty());
            }
        }
        Err(diags) => {
            eprint!(
                "{}",
                jet::render_all_colored(file, &src, &diags, mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

/// D-A11YGATE1=B (c134 Phase 6): the a11y lint codes (E2930/E2931). These are
/// always computed during sema (same as any other `Severity::Lint`), but per
/// the ratified decision they only *surface* under `jet lint --a11y` — never
/// as ordinary build/run/emit warnings. `visible_lints` is the one filter
/// every normal compile-flavored command applies before printing `out.lints`.
const A11Y_LINT_CODES: [&str; 2] = ["E2930", "E2931"];

pub(crate) fn visible_lints(
    lints: &[jet::Diagnostics::Diagnostic],
) -> Vec<jet::Diagnostics::Diagnostic> {
    lints
        .iter()
        .filter(|d| !A11Y_LINT_CODES.contains(&d.code.as_str()))
        .cloned()
        .collect()
}

/// D-TOOL3 (E2-M11): `jet emit --rust` — print the generated Rust source for a
/// Jet file. This is the expert-window view: the hidden Jet→Rust translation
/// without compiling to native. Useful for debugging or learning what codegen
/// produces.
pub(crate) fn run_emit_rust(file: &str, mode: OutputMode) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            crate::cli_error!(@fix "E2105", format!("can't find the file `{}`", file), format!("check the spelling, or run {} from the folder that contains it", jet::Syntax::BINARY_NAME));
            exit(ExitCodes::USER_ERROR);
        }
    };
    match jet::compile_with_path(&src, file) {
        Ok(out) => {
            let lints = visible_lints(&out.lints);
            if !lints.is_empty() {
                eprint!(
                    "{}",
                    jet::render_all_colored(file, &src, &lints, mode.color_stderr())
                );
            }
            print!("{}", out.rust);
        }
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    }
}

/// D-A11YGATE1=B (c134 Phase 6): `jet lint --a11y <file>` — the opt-in
/// surface for accessibility lints (E2930 unlabeled control, E2931 duplicate
/// label). Never runs during `jet build`/`jet run`/`jet check`; exits nonzero
/// when it finds something so a project can gate CI on "zero a11y warnings"
/// without those warnings ever blocking ordinary compilation.
pub(crate) fn run_lint_a11y(file: &str, mode: OutputMode) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            crate::cli_error!(@fix "E2105", format!("can't find the file `{}`", file), format!("check the spelling, or run {} from the folder that contains it", jet::Syntax::BINARY_NAME));
            exit(ExitCodes::USER_ERROR);
        }
    };
    let diags = jet::check_with_path(file);
    let errors: Vec<jet::Diagnostics::Diagnostic> = diags
        .iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .cloned()
        .collect();
    if !errors.is_empty() {
        report_problems(mode, file, &src, &errors);
        exit(ExitCodes::USER_ERROR);
    }
    let a11y_lints: Vec<jet::Diagnostics::Diagnostic> = diags
        .into_iter()
        .filter(|d| A11Y_LINT_CODES.contains(&d.code.as_str()))
        .collect();
    if a11y_lints.is_empty() {
        if mode.json {
            let machine_file = crate::machine_report_path_for_process(file);
            print!("{}", jet::render_all_json(&machine_file, &src, &[]));
        } else {
            println!("ok: `{}` has no accessibility problems", file);
        }
        return;
    }
    if mode.json {
        let machine_file = crate::machine_report_path_for_process(file);
        eprint!("{}", jet::render_all_json(&machine_file, &src, &a11y_lints));
    } else {
        eprint!(
            "{}",
            jet::render_all_colored(file, &src, &a11y_lints, mode.color_stderr())
        );
        let n = a11y_lints.len();
        eprintln!(
            "\n{} accessibility warning{} found",
            n,
            if n == 1 { "" } else { "s" }
        );
    }
    exit(ExitCodes::USER_ERROR);
}

/// D-TOOL5 / D-BENCH-PARITY1=B: `jet bench` accepts one file, a recursive
/// directory target, or a project root. Region benches stay serial because
/// concurrent workloads would corrupt timing results.
#[derive(Clone, Default)]
pub(crate) struct BenchRunOpts {
    /// `--filter=<substr>` selects benchmark region names after discovery.
    pub(crate) filter: Option<String>,
    /// `--show-default` forces the stock harness when the entry defines `fn bench`.
    pub(crate) show_default: bool,
}

const BENCH_PROFILE_LABEL: &str = "release";

fn bench_path_label(path: &Path) -> String {
    let mut label = path.to_string_lossy().replace('\\', "/");
    while let Some(rest) = label.strip_prefix("./") {
        label = rest.to_string();
    }
    label
}

pub(crate) fn run_bench(path: &str, opts: BenchRunOpts, mode: OutputMode) {
    let target = Path::new(path);
    if !target.exists() {
        crate::cli_error!("E2105", "can't find the file `{}`", path);
        exit(ExitCodes::USER_ERROR);
    }
    let mut files = Vec::new();
    if target.is_dir() {
        collect_source_files_recursive(target, jet::Syntax::FILE_EXT, &mut files);
        files.sort();
        if files.is_empty() {
            crate::cli_error!("E2104", "no .{} files in `{}` (searched subdirectories too)", jet::Syntax::FILE_EXT, path);
            exit(ExitCodes::USER_ERROR);
        }
    } else {
        files.push(target.to_path_buf());
    }

    let multi_file = files.len() > 1;
    let mut any_fail = false;
    for file in files {
        let shown = bench_path_label(&file);
        if multi_file && !mode.json {
            println!("== {shown} ==");
        }
        if !run_bench_file(&file, &shown, &opts, mode) {
            any_fail = true;
        }
    }
    exit(if any_fail {
        ExitCodes::USER_ERROR
    } else {
        ExitCodes::OK
    });
}

fn run_bench_file(path: &Path, shown: &str, opts: &BenchRunOpts, mode: OutputMode) -> bool {
    let file = path.to_string_lossy();
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(error) => {
            crate::cli_error!("E2105", "couldn't read `{}`: {}", shown, error);
            return false;
        }
    };

    let override_entry = !opts.show_default && jet::has_entry_fn(&file, "bench");
    if override_entry && !mode.quiet && !mode.json {
        println!("jet bench: using fn bench override");
    }
    // D-BENCH1: region files use the generated harness. A filter applies only
    // to discovered regions; a file without regions contributes no result.
    if jet::has_bench_blocks(&file) {
        return run_bench_regions(
            &file,
            shown,
            &src,
            opts.filter.as_deref(),
            mode,
            override_entry,
        );
    }
    if override_entry {
        return run_bench_override_program(&file, shown, &src, opts.filter.as_deref(), mode);
    }
    if opts.filter.is_some() {
        return true;
    }

    let (rust_code, ffi_link, _capabilities) = match jet::compile_with_path(&src, &file) {
        Ok(out) => (out.rust, out.ffi, out.capabilities),
        Err(diags) => {
            report_problems(mode, &file, &src, &diags);
            return false;
        }
    };
    let bin = PathBuf::from("build").join(format!("bench_{}", stem(&file)));
    build(
        &file,
        &rust_code,
        bin.clone(),
        BuildProfile::Release,
        ffi_link.as_ref(),
        &[],
        false,
        None,
        None,
        None,
        mode,
        // Benchmark build; not content-cached (race-safe via `build`'s temp path).
        None,
    );

    let warmups = 5u32;
    let trials = 20u32;

    for _ in 0..warmups {
        Command::new(&bin).output().ok();
    }

    let mut times_ms: Vec<f64> = Vec::new();
    for _ in 0..trials {
        let t0 = std::time::Instant::now();
        let status = Command::new(&bin).status();
        let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
        match status {
            Ok(s) if s.success() => times_ms.push(elapsed),
            Ok(_) => {
                eprintln!("bench: program exited with non-zero status during trial");
                return false;
            }
            Err(error) => {
                eprintln!("bench: couldn't run `{}`: {}", bin.display(), error);
                return false;
            }
        }
    }

    let n = times_ms.len() as f64;
    let mean = times_ms.iter().sum::<f64>() / n;
    let variance = times_ms.iter().map(|time| (time - mean).powi(2)).sum::<f64>() / n;
    let stddev = variance.sqrt();
    let name = stem(&file);

    if mode.json {
        println!(
            "{{\"name\":{},\"file\":{},\"mean_ms\":{:.3},\"stddev_ms\":{:.3},\"trials\":{},\"warmups\":{},\"profile\":{}}}",
            jet::Diagnostics::json_str(&name),
            jet::Diagnostics::json_str(shown),
            mean,
            stddev,
            trials,
            warmups,
            jet::Diagnostics::json_str(BENCH_PROFILE_LABEL),
        );
    } else {
        println!(
            "{}  {:.2} ms ±{:.2}  ({} runs, {} warmup)  profile: {}",
            name, mean, stddev, trials, warmups, BENCH_PROFILE_LABEL
        );
    }
    true
}

/// D-BENCH1: build and run the per-region bench harness. The harness binary's
/// `main` auto-scales and records 20 serial samples per selected region.
fn run_bench_regions(
    file: &str,
    shown: &str,
    src: &str,
    filter: Option<&str>,
    mode: OutputMode,
    command_override: bool,
) -> bool {
    if !command_override && filter.is_none() && !mode.json {
        if let Some(status) = crate::CmdBudget::reuse_bench_report(file) {
            return status == 0;
        }
    }
    let evidence = collect_bench_evidence_with_filter(
        file,
        src,
        mode,
        false,
        filter,
        command_override,
    );
    for bench in &evidence {
        let samples = bench
            .samples
            .iter()
            .map(|(elapsed, iters)| *elapsed as f64 / *iters as f64)
            .collect::<Vec<_>>();
        let n = samples.len() as f64;
        let mean = samples.iter().sum::<f64>() / n;
        let variance = samples
            .iter()
            .map(|sample| (sample - mean) * (sample - mean))
            .sum::<f64>()
            / n;
        let ops = if mean > 0.0 { 1.0e9 / mean } else { 0.0 };
        let name = format!("{shown}::{}", bench.name);
        if mode.json {
            println!(
                "{{\"name\":{},\"file\":{},\"region\":{},\"mean_ns\":{:.1},\"stddev_ns\":{:.1},\"ops_per_sec\":{:.0},\"samples\":{},\"profile\":{}}}",
                jet::Diagnostics::json_str(&name),
                jet::Diagnostics::json_str(shown),
                jet::Diagnostics::json_str(&bench.name),
                mean,
                variance.sqrt(),
                ops,
                samples.len(),
                jet::Diagnostics::json_str(BENCH_PROFILE_LABEL),
            );
        } else {
            println!(
                "{}  {:.1} ns/iter (\u{00b1}{:.1})  {:.0} ops/sec  profile: {}",
                name,
                mean,
                variance.sqrt(),
                ops,
                BENCH_PROFILE_LABEL
            );
        }
    }
    if !command_override && filter.is_none() && crate::CmdBudget::run_bench_refresh(file, &evidence) != 0 {
        return false;
    }
    true
}

#[derive(Clone, Debug)]
pub(crate) struct BenchEvidence {
    pub(crate) name: String,
    pub(crate) samples: Vec<(u128, u64)>,
    /// One exact `(jet_mem allocation events, requested bytes, iterations)`
    /// row per measured trial. Calibration/warmup runs are outside the reset
    /// boundary and never enter these facts.
    pub(crate) allocation_samples: Vec<(u128, u128, u64)>,
}

/// Shared `#Bench` executor for human bench output and BenchMeasurement.
/// Wire rows are compiler-private; user-facing spelling stays `jet bench`.
pub(crate) fn collect_bench_evidence(
    file: &str,
    src: &str,
    mode: OutputMode,
    relay_output: bool,
) -> Vec<BenchEvidence> {
    collect_bench_evidence_with_filter(file, src, mode, relay_output, None, false)
}

fn collect_bench_evidence_with_filter(
    file: &str,
    src: &str,
    mode: OutputMode,
    relay_output: bool,
    filter: Option<&str>,
    command_override: bool,
) -> Vec<BenchEvidence> {
    let (rust_code, ffi_link) = match if command_override {
        jet::compile_bench_override_with_path(src, file)
    } else {
        jet::compile_benches_with_path(file)
    } {
        Ok(r) => r,
        Err(diags) => {
            report_problems(mode, file, src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let bin = PathBuf::from("build").join(format!("bench_{}", stem(file)));
    build(
        file,
        &rust_code,
        bin.clone(),
        BuildProfile::Release,
        ffi_link.as_ref(),
        &[],
        false,
        None,
        None,
        None,
        mode,
        // Benchmark build; not content-cached (race-safe via `build`'s temp path).
        None,
    );
    let mut command = Command::new(&bin);
    if let Some(filter) = filter {
        command.env("JET_BENCH_FILTER", filter);
    }
    let out = command.output().unwrap_or_else(|e| {
        eprintln!("bench: couldn't run `{}`: {}", bin.display(), e);
        exit(ExitCodes::USER_ERROR);
    });
    if !out.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
    }
    if !out.status.success() {
        exit(ExitCodes::USER_ERROR);
    }
    let stdout = String::from_utf8(out.stdout).unwrap_or_else(|_| {
        eprintln!("bench: harness emitted non-UTF-8 evidence");
        exit(ExitCodes::USER_ERROR);
    });
    let mut evidence: Vec<BenchEvidence> = Vec::new();
    for line in stdout.lines() {
        if let Some(wire) = line.strip_prefix("JETALLOC1\t") {
            let mut fields = wire.split('\t');
            let name = fields.next().and_then(decode_hex).unwrap_or_else(|| {
                eprintln!("bench: harness emitted malformed allocation workload identity");
                exit(ExitCodes::USER_ERROR);
            });
            let iters = fields.next().and_then(|value| value.parse::<u64>().ok()).filter(|value| *value > 0).unwrap_or_else(|| {
                eprintln!("bench: harness emitted invalid allocation iteration count");
                exit(ExitCodes::USER_ERROR);
            });
            let samples = fields.map(|value| {
                let (count, bytes) = value.split_once(':').ok_or(())?;
                Ok((count.parse::<u128>().map_err(|_| ())?, bytes.parse::<u128>().map_err(|_| ())?, iters))
            }).collect::<Result<Vec<_>, ()>>().unwrap_or_else(|_| {
                eprintln!("bench: harness emitted malformed allocation evidence");
                exit(ExitCodes::USER_ERROR);
            });
            if samples.len() != 20 {
                eprintln!("bench: harness emitted {} allocation samples; policy requires 20", samples.len());
                exit(ExitCodes::USER_ERROR);
            }
            let bench = evidence.iter_mut().find(|bench| bench.name == name).unwrap_or_else(|| {
                eprintln!("bench: allocation evidence preceded its named timing workload");
                exit(ExitCodes::USER_ERROR);
            });
            if !bench.allocation_samples.is_empty() {
                eprintln!("bench: harness emitted duplicate allocation evidence for `{name}`");
                exit(ExitCodes::USER_ERROR);
            }
            bench.allocation_samples = samples;
            continue;
        }
        let Some(wire) = line.strip_prefix("JETBENCH1\t") else {
            if relay_output { println!("{line}"); }
            continue;
        };
        let mut fields = wire.split('\t');
        let name = fields.next().and_then(decode_hex).unwrap_or_else(|| {
            eprintln!("bench: harness emitted malformed benchmark identity");
            exit(ExitCodes::USER_ERROR);
        });
        let iters = fields.next().and_then(|value| value.parse::<u64>().ok()).filter(|value| *value > 0).unwrap_or_else(|| {
            eprintln!("bench: harness emitted invalid iteration count");
            exit(ExitCodes::USER_ERROR);
        });
        let samples = fields.map(|value| value.parse::<u128>().map(|elapsed| (elapsed, iters))).collect::<Result<Vec<_>, _>>().unwrap_or_else(|_| {
            eprintln!("bench: harness emitted invalid exact sample");
            exit(ExitCodes::USER_ERROR);
        });
        if samples.len() != 20 {
            eprintln!("bench: harness emitted {} samples; policy requires 20", samples.len());
            exit(ExitCodes::USER_ERROR);
        }
        evidence.push(BenchEvidence { name, samples, allocation_samples: Vec::new() });
    }
    evidence
}

fn run_bench_override_program(
    file: &str,
    shown: &str,
    src: &str,
    filter: Option<&str>,
    mode: OutputMode,
) -> bool {
    let (rust_code, ffi_link) = match jet::compile_bench_override_with_path(src, file) {
        Ok(value) => value,
        Err(diags) => {
            report_problems(mode, file, src, &diags);
            return false;
        }
    };
    let bin = PathBuf::from("build").join(format!("bench_override_{}", stem(file)));
    build(
        file,
        &rust_code,
        bin.clone(),
        BuildProfile::Release,
        ffi_link.as_ref(),
        &[],
        false,
        None,
        None,
        None,
        mode,
        None,
    );
    let mut command = Command::new(&bin);
    if let Some(filter) = filter {
        command.env("JET_BENCH_FILTER", filter);
    }
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            eprintln!("bench: couldn't run `{}`: {}", shown, error);
            return false;
        }
    };
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    let _ = fs::remove_file(&bin);
    output.status.success()
}

/// Collect `ServiceProbe` evidence by cycling each named service down→up→ready
/// 20 times. Reads `env.jet` from the project root to resolve `DevServicePlan`.
/// Returns one `ServiceEvidence` entry per service name present in `specs`.
pub(crate) fn collect_service_evidence(
    root: &std::path::Path,
    specs: &[jet::Sema::LocatedBudgetSpec],
) -> Vec<crate::CmdBudget::ServiceEvidence> {
    // Names of services that have a ServiceProbe budget.
    let service_names: std::collections::BTreeSet<String> = specs
        .iter()
        .filter(|s| {
            let kind = s.spec.provider.split_once('(').map(|(k, _)| k).unwrap_or(&s.spec.provider);
            kind == "ServiceProbe"
        })
        .map(|s| {
            s.spec.provider
                .split_once('(')
                .and_then(|(_, rest)| rest.strip_suffix(')'))
                .unwrap_or("")
                .to_string()
        })
        .collect();

    if service_names.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    for name in &service_names {
        let argv = vec![
            "__service-probe".to_string(),
            name.clone(),
            "--no-color".to_string(),
        ];
        let output = match crate::EngineDispatch::capture(
            jet::Syntax::JETPACK_BINARY_NAME,
            "ServiceProbe",
            &argv,
            root,
        ) {
            Ok(output) => output,
            Err(_) => continue,
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("budget: ServiceProbe `{name}` measurement failed: {}", stderr.trim());
            continue;
        }
        let stdout = match String::from_utf8(output.stdout) {
            Ok(stdout) => stdout,
            Err(_) => {
                eprintln!("budget: ServiceProbe `{name}` returned non-UTF-8 evidence");
                continue;
            }
        };
        let expected_name: String = name.bytes().map(|byte| format!("{byte:02x}")).collect();
        let mut rows = stdout.lines();
        let Some(row) = rows.next() else {
            eprintln!("budget: ServiceProbe `{name}` returned no evidence");
            continue;
        };
        if rows.next().is_some() {
            eprintln!("budget: ServiceProbe `{name}` returned extra evidence rows");
            continue;
        }
        let mut fields = row.split('\t');
        if fields.next() != Some("JETSERVICE1") || fields.next() != Some(expected_name.as_str()) {
            eprintln!("budget: ServiceProbe `{name}` returned incompatible evidence");
            continue;
        }
        let samples_ns: Option<Vec<u64>> = fields.map(|field| field.parse().ok()).collect();
        let Some(samples_ns) = samples_ns.filter(|samples| samples.len() == 20) else {
            eprintln!("budget: ServiceProbe `{name}` did not return exactly 20 samples");
            continue;
        };
        result.push(crate::CmdBudget::ServiceEvidence {
            name: name.clone(),
            samples_ns,
        });
    }
    result
}

/// Collect `SceneProbe` evidence by compiling the entry file, running it with
/// `JET_SCENE_PROBE=<name>` for each named scene, and parsing JETSCENE1 rows.
/// Returns one `SceneEvidence` per scene present in `specs`.
pub(crate) fn collect_scene_evidence(
    file: &str,
    src: &str,
    mode: OutputMode,
    specs: &[jet::Sema::LocatedBudgetSpec],
) -> Vec<crate::CmdBudget::SceneEvidence> {
    // Names of scenes that have a SceneProbe budget.
    let scene_names: std::collections::BTreeSet<String> = specs
        .iter()
        .filter(|s| {
            let kind = s.spec.provider.split_once('(').map(|(k, _)| k).unwrap_or(&s.spec.provider);
            kind == "SceneProbe"
        })
        .map(|s| {
            s.spec.provider
                .split_once('(')
                .and_then(|(_, rest)| rest.strip_suffix(')'))
                .unwrap_or("")
                .to_string()
        })
        .collect();

    if scene_names.is_empty() {
        return Vec::new();
    }

    // Compile the program once.
    let compiled = match jet::compile_with_path(src, file) {
        Ok(out) => out,
        Err(diags) => {
            report_problems(mode, file, src, &diags);
            return Vec::new();
        }
    };
    let bin = PathBuf::from("build").join(format!("scene_probe_{}", stem(file)));
    build(
        file,
        &compiled.rust,
        bin.clone(),
        BuildProfile::Release,
        compiled.ffi.as_ref(),
        &[],
        false,
        None,
        None,
        None,
        mode,
        None,
    );

    let mut result = Vec::new();
    for scene_name in &scene_names {
        let out = match std::process::Command::new(&bin)
            .env("JET_SCENE_PROBE", scene_name)
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                eprintln!("budget: SceneProbe `{scene_name}` run failed: {e}");
                continue;
            }
        };
        if !out.status.success() {
            eprintln!("budget: SceneProbe `{scene_name}` exited with non-zero status");
            continue;
        }
        let stdout = String::from_utf8(out.stdout).unwrap_or_default();
        let mut frame_ns = Vec::new();
        let mut draw_calls = Vec::new();
        let mut asset_bytes = Vec::new();
        let mut rss_hwm = Vec::new();
        let expected_hex: String = scene_name.bytes().map(|b| format!("{:02x}", b)).collect();
        for line in stdout.lines() {
            let Some(wire) = line.strip_prefix("JETSCENE1\t") else { continue };
            let mut fields = wire.splitn(4, '\t');
            let hex = fields.next().unwrap_or("");
            let metric = fields.next().unwrap_or("");
            let value_str = fields.next().unwrap_or("");
            if hex != expected_hex { continue; }
            let value: u64 = match value_str.parse() {
                Ok(v) => v,
                Err(_) => { eprintln!("budget: SceneProbe `{scene_name}` malformed value `{value_str}`"); continue; }
            };
            match metric {
                "FrameTime" => frame_ns.push(value),
                "DrawCalls" => draw_calls.push(value),
                "SceneAssetBytes" => asset_bytes.push(value),
                "MemoryHighWater" => rss_hwm.push(value),
                _ => {}
            }
        }
        // Require exactly 600 measured samples per metric (120 warmup omitted).
        if frame_ns.len() != 600 || draw_calls.len() != 600 || asset_bytes.len() != 600 || rss_hwm.len() != 600 {
            eprintln!(
                "budget: SceneProbe `{scene_name}` emitted {}/{}/{}/{} samples; need 600 for each metric",
                frame_ns.len(), draw_calls.len(), asset_bytes.len(), rss_hwm.len()
            );
            continue;
        }
        result.push(crate::CmdBudget::SceneEvidence {
            name: scene_name.clone(),
            frame_ns,
            draw_calls,
            asset_bytes,
            rss_hwm,
        });
    }
    result
}

/// `jet devtools probe <file>` — internal test-only single-shot dev probe.
/// Collects SceneProbe/ServiceProbe evidence for the given file and triggers
/// a budget report refresh. Exits 0 if the report is built and all gates pass,
/// 1 otherwise. Not user-documented; used by CI tests.
pub(crate) fn run_devtools_probe(args: &[&String]) {
    use std::process::exit;
    let file = match args.first() {
        Some(f) => f.as_str(),
        None => {
            eprintln!("usage: jet devtools probe <file.jet>");
            exit(jet::ExitCodes::USAGE);
        }
    };
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("probe: cannot read `{file}`: {e}");
            exit(jet::ExitCodes::USER_ERROR);
        }
    };
    let mode = OutputMode { json: false, color: jet::Diagnostics::ColorChoice::Never, quiet: false };
    let bundle = match jet::Loader::load_entry(file) {
        Ok(mut b) => {
            let _ = jet::Sema::check_bundle(&mut b, jet::Sema::CompileMode::Run);
            b
        }
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(jet::ExitCodes::USER_ERROR);
        }
    };
    let specs = match jet::Sema::collect_located_budget_specs_bundle(&bundle) {
        Ok(s) => s,
        Err(_) => {
            exit(jet::ExitCodes::USER_ERROR);
        }
    };
    let root = Path::new(file)
        .canonicalize()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .and_then(|d| jet::Loader::find_manifest_root(&d).or_else(|| Some(d)))
        .unwrap_or_else(|| PathBuf::from("."));
    let service_evidence = collect_service_evidence(&root, &specs);
    let scene_evidence = collect_scene_evidence(file, &src, mode, &specs);
    let status = crate::CmdBudget::run_dev_refresh(file, &service_evidence, &scene_evidence);
    exit(status);
}

fn decode_hex(value: &str) -> Option<String> {
    if value.len() % 2 != 0 { return None; }
    let nibble = |byte: u8| match byte { b'0'..=b'9' => Some(byte - b'0'), b'a'..=b'f' => Some(byte - b'a' + 10), _ => None };
    let bytes = value.as_bytes().chunks_exact(2).map(|pair| Some((nibble(pair[0])? << 4) | nibble(pair[1])?)).collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod schedule_tests {
    use super::TaskClock;

    /// D-SCHEDULE1 (card #505): an interval task fires the first time it's
    /// checked (no prior run), then not again immediately after.
    #[test]
    fn interval_fires_once_then_waits() {
        let mut clock = TaskClock::new();
        let tasks = vec![(
            "prune".to_string(),
            jet::AST::EverySchedule::Interval {
                nanos: 60 * 1_000_000_000,
            },
        )];
        assert_eq!(clock.due(&tasks), vec!["prune".to_string()]);
        assert!(
            clock.due(&tasks).is_empty(),
            "must not re-fire on the very next tick"
        );
    }

    /// A daily task fires when `unix_secs` lands inside its target minute,
    /// stays quiet outside that window, and does not re-fire later the same
    /// day even if checked again inside the window.
    #[test]
    fn daily_fires_in_window_then_dedupes_same_day() {
        let mut clock = TaskClock::new();
        let tasks = vec![(
            "nightly".to_string(),
            jet::AST::EverySchedule::DailyAt { hour: 3, minute: 0 },
        )];
        let day0_before_window = 10 * 86_400 + 2 * 3600 + 59 * 60; // 02:59 on day 10
        let day0_in_window = 10 * 86_400 + 3 * 3600 + 0 * 60 + 30; // 03:00:30 on day 10
        let day0_after_window = 10 * 86_400 + 3 * 3600 + 5 * 60; // 03:05 on day 10
        let day1_in_window = 11 * 86_400 + 3 * 3600; // 03:00 on day 11

        assert!(
            clock.due_at(&tasks, day0_before_window).is_empty(),
            "must not fire before the target minute"
        );
        assert_eq!(
            clock.due_at(&tasks, day0_in_window),
            vec!["nightly".to_string()],
            "must fire inside the target minute"
        );
        assert!(
            clock.due_at(&tasks, day0_after_window).is_empty(),
            "must not re-fire later the same day, even outside the window"
        );
        assert_eq!(
            clock.due_at(&tasks, day1_in_window),
            vec!["nightly".to_string()],
            "must fire again the next day's matching window"
        );
    }
}
