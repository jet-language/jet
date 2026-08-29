/// D-JOB-SUBCMD1=C: one job table and selector serve AOT binaries and the
/// generated argv boundary. Engines only marshal argv into this Prelude API.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JetJobScope {
    Dev,
    Ship,
    Internal,
}

/// D-SCHEDULE1: the checked `EverySchedule` value carried into the runtime.
/// The generated table stores this value instead of re-reading marker text;
/// every consumer uses the same resolved duration or wall-clock minute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetJobSchedule {
    Duration { nanos: i64 },
    WallClockTime { hour: u8, minute: u8 },
}

/// One shared due clock for dev and service lifecycle consumers. The timer
/// wake-up itself belongs to `Prelude/Scheduler.rs`; this value owns only the
/// schedule decision and last-fire facts.
pub struct JetJobClock {
    last_interval_run: std::collections::HashMap<String, std::time::Instant>,
    last_daily_run_day: std::collections::HashMap<String, u64>,
}

impl JetJobClock {
    pub fn new() -> Self {
        Self {
            last_interval_run: std::collections::HashMap::new(),
            last_daily_run_day: std::collections::HashMap::new(),
        }
    }

    /// Return the names due at the current wall time. A duration schedule fires
    /// immediately on its first check, then after its resolved interval.
    pub fn due(&mut self, jobs: &[(&str, JetJobSchedule)]) -> Vec<String> {
        let unix_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.due_at(jobs, unix_secs)
    }

    /// Testable due decision with the wall-clock seconds supplied by the
    /// caller. The monotonic instant still measures duration intervals.
    pub fn due_at(&mut self, jobs: &[(&str, JetJobSchedule)], unix_secs: u64) -> Vec<String> {
        let now = std::time::Instant::now();
        let day = unix_secs / 86_400;
        let secs_of_day = unix_secs % 86_400;
        let mut fired = Vec::new();
        for (name, schedule) in jobs {
            match *schedule {
                JetJobSchedule::Duration { nanos } => {
                    let due = match self.last_interval_run.get(*name) {
                        None => true,
                        Some(last) => now.duration_since(*last).as_nanos() >= nanos.max(0) as u128,
                    };
                    if due {
                        self.last_interval_run.insert((*name).to_string(), now);
                        fired.push((*name).to_string());
                    }
                }
                JetJobSchedule::WallClockTime { hour, minute } => {
                    let target_secs = hour as u64 * 3600 + minute as u64 * 60;
                    let in_window =
                        secs_of_day >= target_secs && secs_of_day < target_secs + 60;
                    let already_ran_today = self.last_daily_run_day.get(*name) == Some(&day);
                    if in_window && !already_ran_today {
                        self.last_daily_run_day.insert((*name).to_string(), day);
                        fired.push((*name).to_string());
                    }
                }
            }
        }
        fired
    }
}

#[derive(Clone, Copy)]
pub struct JetJobEntry {
    pub name: &'static str,
    pub scope: JetJobScope,
    pub schedule: Option<JetJobSchedule>,
    pub invoke: fn(&str, &[String]),
}

/// One service-runtime tick. The service lifecycle owns when this is called;
/// the due arithmetic remains in `JetJobClock`, and invocation remains the
/// checked job table's callback. There is no second timer loop here.
pub fn jet_job_service_tick(clock: &mut JetJobClock, jobs: &[JetJobEntry], program: &str) {
    let schedules = jobs
        .iter()
        .filter(|job| jet_job_schedule_enabled(job.scope))
        .filter_map(|job| job.schedule.map(|schedule| (job.name, schedule)))
        .collect::<Vec<_>>();
    for name in jet_job_schedule_due(clock, &schedules) {
        if let Some(job) = jobs.iter().find(|job| job.name == name.as_str()) {
            (job.invoke)(program, &[]);
        }
    }
}

/// Shared schedule decision used by AOT, the resident JIT adapter, the TIR
/// evaluator, and `jet dev`.
pub fn jet_job_schedule_due(
    clock: &mut JetJobClock,
    jobs: &[(&str, JetJobSchedule)],
) -> Vec<String> {
    clock.due(jobs)
}

fn jet_job_schedule_enabled(scope: JetJobScope) -> bool {
    match scope {
        JetJobScope::Dev => !cfg!(jet_release),
        JetJobScope::Ship | JetJobScope::Internal => true,
    }
}

fn jet_args_program_name(prog: &str) -> String {
    if prog.is_empty() {
        return "program".to_string();
    }
    std::path::Path::new(prog)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(prog)
        .to_string()
}

/// Normalize a source-file argv[0] to the name used by its built program.
pub(crate) fn jet_args_source_program_name(prog: &str) -> String {
    let (program, suffix) = prog.split_once(' ').unwrap_or((prog, ""));
    let name = jet_args_program_name(program);
    let source = name.strip_suffix(".jet").unwrap_or(&name);
    if suffix.is_empty() {
        source.to_string()
    } else {
        format!("{source} {suffix}")
    }
}

/// The one terminator for a banner a generated CLI writes to a stream: help,
/// usage and command errors. `JetArgsSpec::help()` renders the block, this
/// closes it with the single trailing newline `println!` would add. AOT emits
/// `print!`/`eprint!` around it, the Cranelift host and the interpreter push
/// the result into their own stdout/stderr buffers — no engine re-decides how
/// a banner ends.
#[allow(dead_code)]
pub(crate) fn jet_cli_banner(text: &str) -> String {
    format!("{text}\n")
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JetJobSelection {
    Ordinary,
    Help,
    Job(usize),
    Unknown,
}

fn jet_job_scope_visible(scope: JetJobScope) -> bool {
    match scope {
        JetJobScope::Dev => !cfg!(jet_release),
        JetJobScope::Ship => true,
        // Internal jobs remain callable by code and schedulers, never argv.
        JetJobScope::Internal => false,
    }
}

fn jet_job_scope_name(scope: JetJobScope) -> &'static str {
    match scope {
        JetJobScope::Dev => "dev",
        JetJobScope::Ship => "ship",
        JetJobScope::Internal => "internal",
    }
}

pub fn jet_job_has_visible(jobs: &[(&str, JetJobScope)]) -> bool {
    jobs.iter().any(|(_, scope)| jet_job_scope_visible(*scope))
}

pub fn jet_job_help_text(argv: &[String], jobs: &[(&str, JetJobScope)]) -> String {
    let program = jet_args_source_program_name(argv.first().map(String::as_str).unwrap_or(""));
    let mut text = format!("Usage: {program} <job> [options]\n\nJobs:");
    for (name, scope) in jobs {
        if jet_job_scope_visible(*scope) {
            text.push_str(&format!("\n  {:<20} {}", name, jet_job_scope_name(*scope)));
        }
    }
    text.push('\n');
    text
}

pub fn jet_job_help(argv: &[String], jobs: &[(&str, JetJobScope)]) {
    print!("{}", jet_job_help_text(argv, jobs));
}

/// I4: the one registered E1294 wording (the `E1294` row in
/// `Prelude/Diagnostics.jet`). The compiler tier renders it as a `Diagnostic`
/// and a built binary renders it to stderr through `jet_job_unknown` below —
/// one registered fact, two renderings, never two wordings.
#[allow(dead_code)]
pub const JET_JOB_UNKNOWN_WHY: &str =
    "The first program subcommand must name a function marked `#Job`.";
#[allow(dead_code)]
pub const JET_JOB_UNKNOWN_FIX: &str =
    "Mark a function `#Job`, or check the subcommand spelling.";

#[allow(dead_code)]
pub fn jet_job_unknown_what(name: &str) -> String {
    format!("No job named `{name}`")
}

/// The one label and separator a job refusal uses to advertise names. The
/// caller picks the list, because the honest list is tier-dependent: the
/// compiler tier names every declared job, while a built binary names only the
/// scopes it can still dispatch.
#[allow(dead_code)]
pub fn jet_job_declared_detail(names: &[&str]) -> String {
    format!("declared jobs: {}", names.join(", "))
}

fn jet_job_unknown(argv: &[String], jobs: &[(&str, JetJobScope)], name: &str) -> ! {
    let program = jet_args_source_program_name(argv.first().map(String::as_str).unwrap_or(""));
    let dispatchable = jobs
        .iter()
        .filter(|(_, scope)| jet_job_scope_visible(*scope))
        .map(|(name, _)| *name)
        .collect::<Vec<_>>();
    eprintln!("Error [E1294]: {}", jet_job_unknown_what(name));
    eprintln!(" Why: {JET_JOB_UNKNOWN_WHY}");
    eprintln!(" Fix: {JET_JOB_UNKNOWN_FIX}");
    eprintln!("More: jet-lang.dev/e/E1294");
    eprintln!("{}", jet_job_declared_detail(&dispatchable));
    eprintln!("\nUsage: {program} <job> [options]");
    std::process::exit(2)
}

/// Select the first argv word before any ordinary CLI parser sees it. This is
/// the shared decision kernel used by AOT and the JIT/interpreter adapters.
pub fn jet_job_select(argv: &[String], jobs: &[(&str, JetJobScope)]) -> JetJobSelection {
    let Some(name) = argv.get(1).map(String::as_str) else {
        return JetJobSelection::Ordinary;
    };
    if name == "--help" {
        return JetJobSelection::Help;
    }
    if name.starts_with('-') {
        return JetJobSelection::Ordinary;
    }
    for (index, (job_name, scope)) in jobs.iter().enumerate() {
        if *job_name == name && jet_job_scope_visible(*scope) {
            return JetJobSelection::Job(index);
        }
    }
    JetJobSelection::Unknown
}

/// Dispatch a named job before the ordinary `fn run` CLI parser sees argv.
/// A missing first positional leaves ordinary flags and the default run entry
/// alone; an unknown positional is a command error once jobs exist.
pub fn jet_job_dispatch(argv: &[String], jobs: &[JetJobEntry]) -> bool {
    let specs = jobs
        .iter()
        .map(|job| (job.name, job.scope))
        .collect::<Vec<_>>();
    if specs.is_empty() {
        return false;
    }
    if !jet_job_has_visible(&specs) {
        let Some(name) = argv.get(1).map(String::as_str) else {
            return false;
        };
        if name.starts_with('-') || !specs.iter().any(|(job_name, _)| *job_name == name) {
            return false;
        }
        jet_job_unknown(argv, &specs, name);
    }
    match jet_job_select(argv, &specs) {
        JetJobSelection::Ordinary => false,
        JetJobSelection::Help => {
            jet_job_help(argv, &specs);
            true
        }
        JetJobSelection::Job(index) => {
            (jobs[index].invoke)(
                argv.first().map(String::as_str).unwrap_or("program"),
                &argv[1..],
            );
            true
        }
        JetJobSelection::Unknown => {
            let name = argv.get(1).map(String::as_str).unwrap_or("");
            jet_job_unknown(argv, &specs, name);
        }
    }
}
