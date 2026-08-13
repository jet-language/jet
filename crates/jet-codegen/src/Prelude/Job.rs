/// D-JOB-SUBCMD1=C: one job table and selector serve AOT binaries and the
/// generated argv boundary. Engines only marshal argv into this Prelude API.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JetJobScope {
    Dev,
    Ship,
    Internal,
}

pub struct JetJobEntry {
    pub name: &'static str,
    pub scope: JetJobScope,
    pub invoke: fn(&[String]),
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
    let program = argv.first().map(String::as_str).unwrap_or("");
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

fn jet_job_unknown(argv: &[String], jobs: &[(&str, JetJobScope)], name: &str) -> ! {
    let program = argv.first().map(String::as_str).unwrap_or("");
    eprintln!("Error [E1294]: unknown command `{name}`");
    eprintln!("\nUsage: {program} <job> [options]");
    eprintln!("\nknown jobs:");
    for (name, scope) in jobs {
        if jet_job_scope_visible(*scope) {
            eprintln!("  {}", name);
        }
    }
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
            (jobs[index].invoke)(&argv[1..]);
            true
        }
        JetJobSelection::Unknown => {
            let name = argv.get(1).map(String::as_str).unwrap_or("");
            jet_job_unknown(argv, &specs, name);
        }
    }
}
