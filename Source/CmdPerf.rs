//! D-PERFSESSION1=D: `jet perf` family over one versioned `.jettrace` truth.
//!
//! `run`/`test`/`bench` spawn the exact base-intent driver (`jet run|test|bench …`)
//! with observe enabled, poll live facts while the child runs, then write one
//! `.jettrace` with wall/alloc/tasks/locks/io/native/task-observation spans before
//! exiting with the child's code.
//! `attach`/`view`/`compare`/`export` share the same artifact verify seam.
//! Capture reuses the observe live snapshot (D-OBSERVE-LIVE1) attributed to a
//! Jet source symbol.

use jet_foundation::JetTrace::{
    artifact_extension, build_skeleton_bytes, entrypoint_name_from_source, fn_names_from_source,
    trace_id, verify_jettrace, CapturePolicy, JetSymbolRef, SourceIdentity, TraceAllocation,
    TraceBrowser, TraceHardware, TraceIo, TraceLock, TraceNative, TraceSample, TraceSkeleton,
    TraceSourceMap, TraceSpan, TraceTask, TraceToolchain, DEFAULT_EXCLUSIONS, TRACE_SCHEMA,
    TRACE_IO_ROW_LIMIT, TRACE_SPAN_ROW_LIMIT, TRACE_TASK_ROW_LIMIT, TRACE_VERSION,
};
use jet_foundation::PerformanceBudget::CanonicalJson;
use jet_foundation::SHA256;
use jet_foundation::Syntax::ARTIFACT_EXT_TRACE;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const USAGE: &str = "usage: jet perf <run|test|bench|attach|view|compare|export> …";

pub(crate) enum Outcome {
    Exit(i32),
}

struct CaptureBundle {
    samples: Vec<TraceSample>,
    allocations: Vec<TraceAllocation>,
    browser: Vec<TraceBrowser>,
    browser_rows_truncated: bool,
    tasks: Vec<TraceTask>,
    locks: Vec<TraceLock>,
    io: Vec<TraceIo>,
    io_rows_truncated: bool,
    native: Vec<TraceNative>,
    native_rows_truncated: bool,
    spans: Vec<TraceSpan>,
    span_rows_truncated: bool,
    source_identity: Vec<SourceIdentity>,
    source_maps: Vec<TraceSourceMap>,
    task_rows_truncated: bool,
}

impl CaptureBundle {
    fn empty() -> Self {
        Self {
            samples: Vec::new(),
            allocations: Vec::new(),
            browser: Vec::new(),
            browser_rows_truncated: false,
            tasks: Vec::new(),
            locks: Vec::new(),
            io: Vec::new(),
            io_rows_truncated: false,
            native: Vec::new(),
            native_rows_truncated: false,
            spans: Vec::new(),
            span_rows_truncated: false,
            source_identity: Vec::new(),
            source_maps: Vec::new(),
            task_rows_truncated: false,
        }
    }
}

pub(crate) fn run(raw: &[String]) -> Outcome {
    let Some(action) = raw.get(1).map(String::as_str) else {
        eprintln!("Error [E2102]: `jet perf` needs a subcommand");
        eprintln!(" Fix: {USAGE}");
        return Outcome::Exit(2);
    };
    match action {
        "run" | "test" | "bench" => Outcome::Exit(run_session(action, &raw[2..])),
        "attach" => Outcome::Exit(attach(&raw[2..])),
        "view" => Outcome::Exit(view(&raw[2..])),
        "compare" => Outcome::Exit(compare(&raw[2..])),
        "export" => Outcome::Exit(export(&raw[2..])),
        other => {
            eprintln!("Error [E2101]: `{other}` isn't a jet perf command.");
            eprintln!(" Why: jet perf accepts only commands in its named area.");
            eprintln!(" Fix: run `jet perf help`.");
            Outcome::Exit(2)
        }
    }
}

/// Spawn exact base intent with observe, capture while live, write `.jettrace`.
fn run_session(action: &str, args: &[String]) -> i32 {
    let parsed = match parse_session_args(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("Error [E2102]: {message}");
            eprintln!(" Fix: jet perf {action} <file.jet> [--out <path.jettrace>]");
            return 2;
        }
    };
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("Error [E2102]: cannot resolve jet executable: {error}");
            return 2;
        }
    };
    let mut child_argv = vec![action.to_string()];
    child_argv.extend(parsed.child_args.iter().cloned());

    let mut child = match Command::new(&exe)
        .args(&child_argv)
        .env("JET_OBSERVE", "1")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            eprintln!("Error [E2102]: cannot start `jet {action}`: {error}");
            return 2;
        }
    };
    let pid = child.id();
    let started = Instant::now();
    let mut last_snapshot = None;
    let mut io_timeline = IOTimeline::default();
    let mut native_timing = NativeTimingInput::unavailable(0);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if parsed.source.is_some() {
                    // Observe publishes under the program PID, not the jet host.
                    if let Some(snapshot) = poll_observe_snapshot(pid) {
                        let observed_at_ns = elapsed_ns(started);
                        io_timeline.observe(&snapshot.tasks, observed_at_ns);
                        native_timing = snapshot
                            .process_cpu_ns
                            .map(|duration_ns| NativeTimingInput::Captured {
                                duration_ns,
                                observed_at_ns,
                                process_id: snapshot.process_id,
                            })
                            .unwrap_or_else(|| NativeTimingInput::unavailable(observed_at_ns));
                        last_snapshot = Some(snapshot.text);
                    }
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                eprintln!("Error [E2102]: cannot wait for `jet {action}`: {error}");
                let _ = child.kill();
                let _ = child.wait();
                return 2;
            }
        }
    };
    let wall_ns = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    io_timeline.finish(wall_ns);
    let capture = match &parsed.source {
        Some(source) => match capture_from_source(
            source,
            last_snapshot.as_deref(),
            wall_ns,
            native_timing.duration_ns(),
            Some(&io_timeline),
            Some(&native_timing),
        ) {
            Ok(bundle) => bundle,
            // Missing/unreadable source still gets a schema-valid skeleton.
            Err(_) => CaptureBundle::empty(),
        },
        None => CaptureBundle::empty(),
    };
    let mut argv = vec![action.to_string()];
    argv.extend(args.iter().cloned());
    match write_session_trace(
        action,
        &argv,
        parsed.out.as_deref(),
        capture,
        &parsed.capture_allowlist,
    ) {
        Ok(path) => eprintln!("trace: {}", path.display()),
        Err(message) => {
            eprintln!("Error [E2102]: {message}");
            return 2;
        }
    }
    status.code().unwrap_or(1)
}

struct SessionArgs {
    out: Option<String>,
    source: Option<String>,
    /// Expert `--capture` allowlist (subset of D-PERFSESSION1 default exclusions).
    capture_allowlist: Vec<String>,
    /// Base-intent argv after the action name (perf-only flags stripped).
    child_args: Vec<String>,
}

fn parse_session_args(args: &[String]) -> Result<SessionArgs, String> {
    let mut out = None;
    let mut source = None;
    let mut capture_allowlist = Vec::new();
    let mut child_args = Vec::new();
    let mut i = 0usize;
    let mut passthrough = false;
    while i < args.len() {
        let arg = args[i].as_str();
        if passthrough {
            child_args.push(args[i].clone());
            i += 1;
            continue;
        }
        if arg == "--" {
            passthrough = true;
            child_args.push(args[i].clone());
            i += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--out=") {
            out = Some(value.to_string());
            i += 1;
            continue;
        }
        if arg == "--out" {
            let Some(value) = args.get(i + 1) else {
                return Err("`--out` needs a path".into());
            };
            out = Some(value.clone());
            i += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--capture=") {
            capture_allowlist = parse_capture_allowlist(value)?;
            i += 1;
            continue;
        }
        if arg == "--capture" {
            let Some(value) = args.get(i + 1) else {
                return Err("`--capture` needs a comma-separated allowlist".into());
            };
            capture_allowlist = parse_capture_allowlist(value)?;
            i += 2;
            continue;
        }
        if arg.starts_with('-') {
            child_args.push(args[i].clone());
            i += 1;
            continue;
        }
        if source.is_none()
            && (arg.ends_with(".jet")
                || Path::new(arg).extension().and_then(|e| e.to_str()) == Some("jet"))
        {
            source = Some(arg.to_string());
        }
        child_args.push(args[i].clone());
        i += 1;
    }
    Ok(SessionArgs {
        out,
        source,
        capture_allowlist,
        child_args,
    })
}

fn parse_capture_allowlist(raw: &str) -> Result<Vec<String>, String> {
    let mut items = Vec::new();
    for part in raw.split(',') {
        let item = part.trim();
        if item.is_empty() {
            continue;
        }
        if !DEFAULT_EXCLUSIONS.contains(&item) {
            return Err(format!(
                "`--capture` item `{item}` is not a D-PERFSESSION1 privacy field; allowed: {}",
                DEFAULT_EXCLUSIONS.join(", ")
            ));
        }
        items.push(item.to_string());
    }
    if items.is_empty() {
        return Err("`--capture` needs at least one privacy field".into());
    }
    items.sort();
    items.dedup();
    Ok(items)
}

fn attach(args: &[String]) -> i32 {
    let mut pid = None;
    let mut out = None;
    let mut source = None;
    let mut capture_allowlist = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        let arg = args[i].as_str();
        if let Some(value) = arg.strip_prefix("--out=") {
            out = Some(value.to_string());
            i += 1;
            continue;
        }
        if arg == "--out" {
            let Some(value) = args.get(i + 1) else {
                eprintln!("Error [E2102]: `--out` needs a path");
                return 2;
            };
            out = Some(value.clone());
            i += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--source=") {
            source = Some(value.to_string());
            i += 1;
            continue;
        }
        if arg == "--source" {
            let Some(value) = args.get(i + 1) else {
                eprintln!("Error [E2102]: `--source` needs a `.jet` path");
                return 2;
            };
            source = Some(value.clone());
            i += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--capture=") {
            match parse_capture_allowlist(value) {
                Ok(items) => capture_allowlist = items,
                Err(message) => {
                    eprintln!("Error [E2102]: {message}");
                    return 2;
                }
            }
            i += 1;
            continue;
        }
        if arg == "--capture" {
            let Some(value) = args.get(i + 1) else {
                eprintln!("Error [E2102]: `--capture` needs a comma-separated allowlist");
                return 2;
            };
            match parse_capture_allowlist(value) {
                Ok(items) => capture_allowlist = items,
                Err(message) => {
                    eprintln!("Error [E2102]: {message}");
                    return 2;
                }
            }
            i += 2;
            continue;
        }
        if arg.starts_with('-') {
            eprintln!("Error [E2102]: unknown `jet perf attach` flag `{arg}`");
            return 2;
        }
        if pid.is_some() {
            eprintln!("Error [E2102]: `jet perf attach` takes one process id");
            return 2;
        }
        let Ok(value) = arg.parse::<u32>() else {
            eprintln!("Error [E2102]: `{arg}` isn't a process id");
            return 2;
        };
        pid = Some(value);
        i += 1;
    }
    let Some(pid) = pid else {
        eprintln!("Error [E2102]: `jet perf attach` needs a process id");
        eprintln!(" Fix: jet perf attach <pid> --source <file.jet>");
        return 2;
    };
    if !process_exists(pid) {
        eprintln!("Error [E2102]: process {pid} is not running or not visible to this user");
        eprintln!(" Fix: start the program with `jet run --observe`, then attach to that pid");
        return 2;
    }

    let browser_capture = match read_browser_capture(pid, source.is_none()) {
        Ok(capture) => Some(capture),
        Err(_) if !jet::DevServer::BrowserTrace::relay_path(pid).exists() => None,
        Err(message) => {
            eprintln!("Error [E2102]: {message}");
            return 2;
        }
    };
    let capture = if let Some(browser_capture) = browser_capture {
        if source
            .as_deref()
            .is_some_and(|path| !browser_capture.sources.iter().any(|source| source.path == path))
        {
            eprintln!("Error [E2102]: `--source` does not match the devserver browser session");
            return 2;
        }
        let browser = match capture_browser(browser_capture) {
            Ok(bundle) => bundle,
            Err(message) => {
                eprintln!("Error [E2102]: {message}");
                return 2;
            }
        };
        // D-PERF-BROWSER-TRANSPORT1=A: one artifact merges host + browser facts.
        match merge_host_onto_browser(pid, source.as_deref(), browser) {
            Ok(bundle) => bundle,
            Err(message) => {
                eprintln!("Error [E2102]: {message}");
                return 2;
            }
        }
    } else {
        match (source.as_deref(), jet::DevServer::LiveInspect::read(pid)) {
            (Some(path), Ok(snapshot)) => {
                let timing = process_times(pid);
                let wall_ns = timing.map(|(wall_ns, _)| wall_ns).unwrap_or(0);
                let cpu_ns = timing.map(|(_, cpu_ns)| cpu_ns);
                let native_timing = timing
                    .map(|(_, duration_ns)| NativeTimingInput::Captured {
                        duration_ns,
                        // Attach is a point capture: trace-session origin is now.
                        observed_at_ns: 0,
                        process_id: pid,
                    })
                    .unwrap_or_else(|| NativeTimingInput::unavailable(wall_ns));
                match capture_from_source(
                    path,
                    Some(&snapshot),
                    wall_ns,
                    cpu_ns,
                    None,
                    Some(&native_timing),
                ) {
                    Ok(bundle) => bundle,
                    Err(message) => {
                        eprintln!("Error [E2102]: {message}");
                        return 2;
                    }
                }
            }
            (Some(_), Err(message)) => {
                eprintln!("Error [E2102]: {message}");
                eprintln!(" Fix: start the program with `jet run --observe`, then attach");
                return 2;
            }
            (None, Ok(_)) => {
                eprintln!("Error [E2102]: live observe snapshot found, but `--source` is required");
                eprintln!(" Why: domain capture must attribute facts to a Jet symbol identity");
                eprintln!(" Fix: jet perf attach {pid} --source <file.jet>");
                return 2;
            }
            (None, Err(_)) => CaptureBundle::empty(),
        }
    };

    let mut argv = vec!["attach".into(), pid.to_string()];
    if let Some(path) = &source {
        argv.push("--source".into());
        argv.push(path.clone());
    }
    if !capture_allowlist.is_empty() {
        argv.push("--capture".into());
        argv.push(capture_allowlist.join(","));
    }
    match write_session_trace(
        "attach",
        &argv,
        out.as_deref(),
        capture,
        &capture_allowlist,
    ) {
        Ok(path) => {
            eprintln!("trace: {}", path.display());
            0
        }
        Err(message) => {
            eprintln!("Error [E2102]: {message}");
            2
        }
    }
}

fn read_browser_capture(pid: u32, activate: bool) -> Result<jet::DevServer::BrowserTrace::Capture, String> {
    match jet::DevServer::BrowserTrace::read(pid) {
        Ok(capture) => return Ok(capture),
        Err(error) if jet::DevServer::BrowserTrace::relay_path(pid).exists() || !activate => return Err(error),
        Err(_) => {}
    }
    jet::DevServer::BrowserTrace::request(pid)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if jet::DevServer::BrowserTrace::relay_path(pid).exists() {
            std::thread::sleep(Duration::from_millis(1600));
            return jet::DevServer::BrowserTrace::read(pid);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    jet::DevServer::BrowserTrace::cancel_request(pid);
    Err(format!("process {pid} has no browser trace relay"))
}

fn capture_browser(capture: jet::DevServer::BrowserTrace::Capture) -> Result<CaptureBundle, String> {
    let mut bundle = CaptureBundle::empty();
    bundle.source_identity = capture
        .sources
        .iter()
        .map(|source| SourceIdentity {
            path: source.path.clone(),
            sha256: source.sha256.clone(),
            symbols: source.symbols.clone(),
        })
        .collect();
    if let Some(map) = &capture.source_map {
        bundle.source_maps.push(TraceSourceMap::from_map_bytes("js", "app.js", map));
    }
    let symbols = capture
        .sources
        .iter()
        .flat_map(|source| {
            source
                .symbols
                .iter()
                .map(move |(name, _)| (name.as_str(), source.path.as_str()))
        })
        .collect::<BTreeMap<_, _>>();
    for row in capture.rows {
        let path = symbols.get(row.symbol.as_str()).ok_or_else(|| {
            format!("browser trace named unknown compiler symbol `{}`", row.symbol)
        })?;
        bundle.browser.push(TraceBrowser {
            class: row.class,
            duration_ns: row.duration_ns,
            start_ns: row.start_ns,
            symbol: JetSymbolRef {
                path: (*path).to_string(),
                name: row.symbol,
            },
        });
    }
    bundle.browser_rows_truncated = capture.truncated;
    Ok(bundle)
}

/// Join host observe/native facts onto a browser capture without rereading
/// compiler source identities from disk (D-PERF-BROWSER-TRANSPORT1=A).
fn merge_host_onto_browser(
    pid: u32,
    source: Option<&str>,
    mut browser: CaptureBundle,
) -> Result<CaptureBundle, String> {
    let symbol = attribution_symbol(&browser.source_identity, source).ok_or_else(|| {
        "browser capture has no Jet symbol identity for host attribution".to_string()
    })?;
    let timing = process_times(pid);
    let wall_ns = timing.map(|(wall_ns, _)| wall_ns).unwrap_or(0);
    let cpu_ns = timing.map(|(_, cpu_ns)| cpu_ns);
    if wall_ns > 0 {
        browser.samples.push(TraceSample {
            domain: "wall".into(),
            duration_ns: wall_ns,
            symbol: symbol.clone(),
        });
    }
    if let Some(cpu_ns) = cpu_ns.filter(|ns| *ns > 0) {
        browser.samples.push(TraceSample {
            domain: "cpu".into(),
            duration_ns: cpu_ns,
            symbol: symbol.clone(),
        });
    }
    let native_timing = timing
        .map(|(_, duration_ns)| NativeTimingInput::Captured {
            duration_ns,
            observed_at_ns: 0,
            process_id: pid,
        })
        .unwrap_or_else(|| NativeTimingInput::unavailable(wall_ns));
    let snapshot = jet::DevServer::LiveInspect::read(pid).ok();
    let snapshot_tasks = snapshot
        .as_deref()
        .and_then(|snapshot| {
            let process_id = u32::try_from(json_u64(snapshot, "pid")?).ok()?;
            Some(observe_tasks(snapshot, process_id))
        })
        .unwrap_or_default();
    if let Some(snapshot) = snapshot.as_deref() {
        let (alloc_count, alloc_bytes) = observe_arena_resources(snapshot);
        if alloc_count > 0 || alloc_bytes > 0 {
            browser.allocations.push(TraceAllocation {
                count: alloc_count,
                bytes: alloc_bytes,
                symbol: symbol.clone(),
            });
        }
        for observed in observe_locks(snapshot) {
            browser.locks.push(TraceLock {
                kind: "channel".into(),
                id: observed.id,
                depth: observed.depth,
                capacity: observed.capacity,
                send_waiters: observed.send_waiters,
                recv_waiters: observed.recv_waiters,
                closed: observed.closed,
                symbol: symbol.clone(),
            });
        }
    }
    let mut observed_tasks = snapshot_tasks.rows.clone();
    observed_tasks.sort_by_key(ObservedTask::key);
    let trace_task_ids = trace_task_id_map(&observed_tasks);
    for observed in &observed_tasks {
        let id = trace_task_ids[&observed.key()];
        let parent = if observed.parent == 0 {
            0
        } else {
            *trace_task_ids
                .get(&(observed.process_id, observed.parent))
                .ok_or_else(|| {
                    format!(
                        "observed task {} in process {} has missing parent {}",
                        observed.id, observed.process_id, observed.parent
                    )
                })?
        };
        browser.tasks.push(TraceTask {
            id,
            parent,
            state: observed.state.clone(),
            wait: observed.wait.clone(),
            cancelled: observed.cancelled,
            symbol: symbol.clone(),
        });
    }
    browser.task_rows_truncated = snapshot_tasks.truncated;
    for observed in observe_io(&snapshot_tasks.rows) {
        let Some(task_id) = trace_task_ids.get(&observed.task).copied() else {
            continue;
        };
        browser.io.push(TraceIo {
            end_ns: wall_ns,
            kind: observed.kind,
            start_ns: wall_ns,
            task_id,
            wait: observed.wait,
            symbol: symbol.clone(),
        });
    }
    let root_task_id = match &native_timing {
        NativeTimingInput::Captured { process_id, .. } => observed_tasks
            .iter()
            .find(|task| task.process_id == *process_id && task.parent == 0)
            .and_then(|task| trace_task_ids.get(&task.key()))
            .copied(),
        NativeTimingInput::Unavailable { .. } => None,
    };
    browser.native.push(match (&native_timing, root_task_id) {
        (
            NativeTimingInput::Captured {
                duration_ns,
                observed_at_ns,
                ..
            },
            Some(task_id),
        ) => TraceNative {
            clock: "process_cpu".into(),
            duration_ns: Some(*duration_ns),
            observed_at_ns: *observed_at_ns,
            reason: String::new(),
            status: "captured".into(),
            symbol: symbol.clone(),
            target: env!("JET_BUILD_TARGET").into(),
            task_id: Some(task_id),
        },
        (NativeTimingInput::Captured { observed_at_ns, .. }, None) => TraceNative {
            clock: "process_cpu".into(),
            duration_ns: None,
            observed_at_ns: *observed_at_ns,
            reason: "root task causality was not observable".into(),
            status: "unavailable".into(),
            symbol: symbol.clone(),
            target: env!("JET_BUILD_TARGET").into(),
            task_id: None,
        },
        (NativeTimingInput::Unavailable { observed_at_ns, reason }, _) => TraceNative {
            clock: "process_cpu".into(),
            duration_ns: None,
            observed_at_ns: *observed_at_ns,
            reason: reason.clone(),
            status: "unavailable".into(),
            symbol: symbol.clone(),
            target: env!("JET_BUILD_TARGET").into(),
            task_id: None,
        },
    });
    Ok(browser)
}

fn attribution_symbol(
    sources: &[SourceIdentity],
    preferred_path: Option<&str>,
) -> Option<JetSymbolRef> {
    let source = preferred_path
        .and_then(|path| sources.iter().find(|source| source.path == path))
        .or_else(|| sources.first())?;
    let name = source
        .symbols
        .iter()
        .find(|(name, kind)| kind == "fn" && name == "run")
        .or_else(|| source.symbols.iter().find(|(_, kind)| kind == "fn"))
        .or_else(|| source.symbols.first())
        .map(|(name, _)| name.clone())?;
    Some(JetSymbolRef {
        path: source.path.clone(),
        name,
    })
}

fn capture_from_source(
    source_path: &str,
    snapshot: Option<&str>,
    wall_ns: u64,
    cpu_ns: Option<u64>,
    io_timeline: Option<&IOTimeline>,
    native_timing: Option<&NativeTimingInput>,
) -> Result<CaptureBundle, String> {
    let path = PathBuf::from(source_path);
    if path.extension().and_then(|e| e.to_str()) != Some("jet") {
        return Err(format!("source path must be a `.jet` file, got `{source_path}`"));
    }
    let bytes = fs::read(&path).map_err(|e| format!("cannot read source {source_path}: {e}"))?;
    let src = String::from_utf8_lossy(&bytes);
    let sha256 = SHA256::sha256_hex(&bytes);
    let path_text = source_path.to_string();
    let fn_names = fn_names_from_source(&src);
    let entry = entrypoint_name_from_source(&src);
    let symbol = entry.map(|name| JetSymbolRef {
        path: path_text.clone(),
        name,
    });
    let snapshot_tasks = snapshot
        .and_then(|snapshot| {
            let process_id = u32::try_from(json_u64(snapshot, "pid")?).ok()?;
            Some(observe_tasks(snapshot, process_id))
        })
        .unwrap_or_default();
    let mut samples = Vec::new();
    // Honest wall: omit when zero; never invent 1ns via max(1).
    if let (Some(symbol), true) = (symbol.as_ref(), wall_ns > 0) {
        samples.push(TraceSample {
            domain: "wall".into(),
            duration_ns: wall_ns,
            symbol: symbol.clone(),
        });
    }
    if let (Some(symbol), Some(cpu_ns)) = (symbol.as_ref(), cpu_ns.filter(|ns| *ns > 0)) {
        samples.push(TraceSample {
            domain: "cpu".into(),
            duration_ns: cpu_ns,
            symbol: symbol.clone(),
        });
    }
    let mut allocations = Vec::new();
    // Only record alloc facts when observe shows activity — never a zero scrape-success.
    if let (Some(symbol), Some(snapshot)) = (symbol.as_ref(), snapshot) {
        let (alloc_count, alloc_bytes) = observe_arena_resources(snapshot);
        if alloc_count > 0 || alloc_bytes > 0 {
            allocations.push(TraceAllocation {
                count: alloc_count,
                bytes: alloc_bytes,
                symbol: symbol.clone(),
            });
        }
    }
    // Tasks: only rows observe published. Never invent ids/parents/states.
    let mut observed_tasks = io_timeline
        .map(IOTimeline::tasks)
        .unwrap_or_else(|| snapshot_tasks.rows.clone());
    observed_tasks.sort_by_key(ObservedTask::key);
    let trace_task_ids = trace_task_id_map(&observed_tasks);
    let mut tasks = Vec::new();
    if let Some(symbol) = symbol.as_ref() {
        for observed in &observed_tasks {
            let id = trace_task_ids[&observed.key()];
            let parent = if observed.parent == 0 {
                0
            } else {
                *trace_task_ids
                    .get(&(observed.process_id, observed.parent))
                    .ok_or_else(|| {
                        format!(
                            "observed task {} in process {} has missing parent {}",
                            observed.id, observed.process_id, observed.parent
                        )
                    })?
            };
            tasks.push(TraceTask {
                id,
                parent,
                state: observed.state.clone(),
                wait: observed.wait.clone(),
                cancelled: observed.cancelled,
                symbol: symbol.clone(),
            });
        }
    }
    // Locks: only contended observe channels (waiters > 0). Idle scrape omitted.
    let mut locks = Vec::new();
    if let (Some(symbol), Some(snapshot)) = (symbol.as_ref(), snapshot) {
        for observed in observe_locks(snapshot) {
            locks.push(TraceLock {
                kind: "channel".into(),
                id: observed.id,
                depth: observed.depth,
                capacity: observed.capacity,
                send_waiters: observed.send_waiters,
                recv_waiters: observed.recv_waiters,
                closed: observed.closed,
                symbol: symbol.clone(),
            });
        }
    }
    // I/O: only blocked observe waits matching the live io classifier.
    let mut io = Vec::new();
    if let Some(symbol) = symbol.as_ref() {
        let observed = io_timeline
            .map(|timeline| timeline.completed.clone())
            .unwrap_or_else(|| {
                observe_io(&snapshot_tasks.rows)
                    .into_iter()
                    .map(|io| ObservedIoSpan {
                        end_ns: wall_ns,
                        kind: io.kind,
                        start_ns: wall_ns,
                        task: io.task,
                        wait: io.wait,
                    })
                    .collect()
            });
        for observed in observed {
            let Some(task_id) = trace_task_ids.get(&observed.task).copied() else {
                continue;
            };
            io.push(TraceIo {
                end_ns: observed.end_ns,
                kind: observed.kind,
                start_ns: observed.start_ns,
                task_id,
                wait: observed.wait,
                symbol: symbol.clone(),
            });
        }
    }
    let mut native = Vec::new();
    if let (Some(symbol), Some(timing)) = (symbol.as_ref(), native_timing) {
        let root_task_id = match timing {
            NativeTimingInput::Captured { process_id, .. } => observed_tasks
                .iter()
                .find(|task| task.process_id == *process_id && task.parent == 0)
                .and_then(|task| trace_task_ids.get(&task.key()))
                .copied(),
            NativeTimingInput::Unavailable { .. } => None,
        };
        native.push(match (timing, root_task_id) {
            (
                NativeTimingInput::Captured {
                    duration_ns,
                    observed_at_ns,
                    ..
                },
                Some(task_id),
            ) => TraceNative {
                clock: "process_cpu".into(),
                duration_ns: Some(*duration_ns),
                observed_at_ns: *observed_at_ns,
                reason: String::new(),
                status: "captured".into(),
                symbol: symbol.clone(),
                target: env!("JET_BUILD_TARGET").into(),
                task_id: Some(task_id),
            },
            (NativeTimingInput::Captured { observed_at_ns, .. }, None) => TraceNative {
                clock: "process_cpu".into(),
                duration_ns: None,
                observed_at_ns: *observed_at_ns,
                reason: "root task causality was not observable".into(),
                status: "unavailable".into(),
                symbol: symbol.clone(),
                target: env!("JET_BUILD_TARGET").into(),
                task_id: None,
            },
            (NativeTimingInput::Unavailable { observed_at_ns, reason }, _) => TraceNative {
                clock: "process_cpu".into(),
                duration_ns: None,
                observed_at_ns: *observed_at_ns,
                reason: reason.clone(),
                status: "unavailable".into(),
                symbol: symbol.clone(),
                target: env!("JET_BUILD_TARGET").into(),
                task_id: None,
            },
        });
    }
    let mut spans = Vec::new();
    if let Some(symbol) = symbol.as_ref() {
        match io_timeline {
            Some(timeline) if !timeline.task_spans.is_empty() => {
                for observed in timeline.spans() {
                    let Some(task_id) = trace_task_ids.get(&observed.task).copied() else {
                        continue;
                    };
                    let parent_task_id = if observed.parent_task_id == 0 {
                        None
                    } else {
                        Some(
                            *trace_task_ids
                                .get(&(observed.task.0, observed.parent_task_id))
                                .ok_or_else(|| {
                                    format!(
                                        "observed span task {} in process {} has missing parent {}",
                                        observed.task.1, observed.task.0, observed.parent_task_id
                                    )
                                })?,
                        )
                    };
                    spans.push(TraceSpan {
                        clock: "monotonic".into(),
                        end_ns: Some(observed.end_ns),
                        kind: "task_observed".into(),
                        parent_task_id,
                        reason: String::new(),
                        start_ns: Some(observed.start_ns),
                        status: "captured".into(),
                        symbol: symbol.clone(),
                        task_id: Some(task_id),
                    });
                }
            }
            Some(_) => spans.push(TraceSpan {
                clock: "monotonic".into(),
                end_ns: None,
                kind: "task_observed".into(),
                parent_task_id: None,
                reason: "task presence was not observable".into(),
                start_ns: None,
                status: "unavailable".into(),
                symbol: symbol.clone(),
                task_id: None,
            }),
            None => spans.push(TraceSpan {
                clock: "monotonic".into(),
                end_ns: None,
                kind: "task_observed".into(),
                parent_task_id: None,
                reason: "task span requires multiple live observations".into(),
                start_ns: None,
                status: "unavailable".into(),
                symbol: symbol.clone(),
                task_id: None,
            }),
        }
    }
    Ok(CaptureBundle {
        samples,
        allocations,
        browser: Vec::new(),
        browser_rows_truncated: false,
        tasks,
        locks,
        io,
        io_rows_truncated: io_timeline
            .map(|timeline| timeline.io_rows_truncated)
            .unwrap_or(snapshot_tasks.truncated),
        native,
        native_rows_truncated: false,
        spans,
        span_rows_truncated: io_timeline
            .map(|timeline| timeline.span_rows_truncated)
            .unwrap_or(false),
        source_identity: vec![SourceIdentity {
            path: path_text.clone(),
            sha256,
            symbols: fn_names
                .into_iter()
                .map(|name| (name, "fn".into()))
                .collect(),
        }],
        source_maps: vec![TraceSourceMap::jet_with_source(&path_text, &src)],
        task_rows_truncated: io_timeline
            .map(|timeline| timeline.task_rows_truncated)
            .unwrap_or(snapshot_tasks.truncated),
    })
}

enum NativeTimingInput {
    Captured {
        duration_ns: u64,
        observed_at_ns: u64,
        process_id: u32,
    },
    Unavailable { observed_at_ns: u64, reason: String },
}

impl NativeTimingInput {
    fn unavailable(observed_at_ns: u64) -> Self {
        Self::Unavailable {
            observed_at_ns,
            reason: native_unavailable_reason(),
        }
    }

    fn duration_ns(&self) -> Option<u64> {
        match self {
            Self::Captured { duration_ns, .. } => Some(*duration_ns),
            Self::Unavailable { .. } => None,
        }
    }
}

fn native_unavailable_reason() -> String {
    native_unavailable_reason_for(std::env::consts::OS, env!("JET_BUILD_TARGET"))
}

fn native_unavailable_reason_for(os: &str, target: &str) -> String {
    if native_process_cpu_supported(os) {
        "process CPU timing was not observable".into()
    } else {
        format!("process CPU timing is unavailable on target {target}")
    }
}

fn native_process_cpu_supported(os: &str) -> bool {
    matches!(os, "linux" | "android")
}

fn observe_arena_resources(snapshot: &str) -> (u64, u64) {
    let resources = snapshot
        .split_once("\"resources\":")
        .map(|(_, tail)| tail)
        .unwrap_or("");
    let count = json_u64(resources, "arena_allocations").unwrap_or(0);
    let bytes = json_u64(resources, "arena_bytes").unwrap_or(0);
    (count, bytes)
}

#[derive(Clone)]
struct ObservedTask {
    process_id: u32,
    id: u64,
    parent: u64,
    state: String,
    wait: String,
    cancelled: bool,
}

type ObservedTaskKey = (u32, u64);

impl ObservedTask {
    fn key(&self) -> ObservedTaskKey {
        (self.process_id, self.id)
    }
}

#[derive(Clone, Default)]
struct ObservedTasks {
    process_id: u32,
    rows: Vec<ObservedTask>,
    truncated: bool,
}

/// Parse at most the artifact policy's task-row cap, in id order.
fn observe_tasks(snapshot: &str, process_id: u32) -> ObservedTasks {
    let Some(inner) = json_array_inner(snapshot, "tasks") else {
        return ObservedTasks {
            process_id,
            ..ObservedTasks::default()
        };
    };
    if inner.is_empty() {
        return ObservedTasks {
            process_id,
            ..ObservedTasks::default()
        };
    }
    let mut out = Vec::new();
    let (objects, truncated) = split_json_objects(inner, TRACE_TASK_ROW_LIMIT as usize);
    for object in objects {
        let Some(id) = json_u64(object, "id") else {
            continue;
        };
        let Some(parent) = json_u64(object, "parent") else {
            continue;
        };
        let Some(state) = json_string(object, "state") else {
            continue;
        };
        if !matches!(state.as_str(), "running" | "queued" | "blocked" | "done") {
            continue;
        }
        let wait = json_string(object, "wait").unwrap_or_default();
        let cancelled = object.contains("\"cancelled\":true");
        out.push(ObservedTask {
            process_id,
            id,
            parent,
            state,
            wait,
            cancelled,
        });
    }
    out.sort_by_key(|task| task.id);
    ObservedTasks {
        process_id,
        rows: out,
        truncated,
    }
}

fn trace_task_id_map(tasks: &[ObservedTask]) -> BTreeMap<ObservedTaskKey, u64> {
    tasks
        .iter()
        .map(ObservedTask::key)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .enumerate()
        .map(|(index, key)| (key, index as u64 + 1))
        .collect()
}

struct ObservedLock {
    id: u64,
    depth: u64,
    capacity: Option<u64>,
    send_waiters: u64,
    recv_waiters: u64,
    closed: bool,
}

/// Contended observe channels only. Idle depth/capacity scrapes are not locks.
fn observe_locks(snapshot: &str) -> Vec<ObservedLock> {
    let Some(inner) = json_array_inner(snapshot, "channels") else {
        return Vec::new();
    };
    if inner.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for object in split_json_objects(inner, TRACE_TASK_ROW_LIMIT as usize).0 {
        let Some(id) = json_u64(object, "id") else {
            continue;
        };
        let Some(depth) = json_u64(object, "depth") else {
            continue;
        };
        let Some(send_waiters) = json_u64(object, "send_waiters") else {
            continue;
        };
        let Some(recv_waiters) = json_u64(object, "recv_waiters") else {
            continue;
        };
        if send_waiters == 0 && recv_waiters == 0 {
            continue;
        }
        let capacity = if object.contains("\"capacity\":null") {
            None
        } else {
            json_u64(object, "capacity")
        };
        let closed = object.contains("\"closed\":true");
        out.push(ObservedLock {
            id,
            depth,
            capacity,
            send_waiters,
            recv_waiters,
            closed,
        });
    }
    out.sort_by_key(|lock| lock.id);
    out
}

struct ObservedIo {
    kind: String,
    task: ObservedTaskKey,
    wait: String,
}

#[derive(Clone)]
struct ObservedIoSpan {
    end_ns: u64,
    kind: String,
    start_ns: u64,
    task: ObservedTaskKey,
    wait: String,
}

#[derive(Clone)]
struct ObservedTaskSpan {
    end_ns: u64,
    parent_task_id: u64,
    start_ns: u64,
    task: ObservedTaskKey,
}

#[derive(Default)]
struct IOTimeline {
    active: BTreeMap<(ObservedTaskKey, String), (String, u64)>,
    completed: Vec<ObservedIoSpan>,
    io_rows_truncated: bool,
    process_last_seen: BTreeMap<u32, u64>,
    span_rows_truncated: bool,
    task_spans: BTreeMap<ObservedTaskKey, ObservedTaskSpan>,
    task_rows_truncated: bool,
    tasks: BTreeMap<ObservedTaskKey, ObservedTask>,
}

impl IOTimeline {
    fn observe(&mut self, observed_tasks: &ObservedTasks, now_ns: u64) {
        self.task_rows_truncated |= observed_tasks.truncated;
        self.io_rows_truncated |= observed_tasks.truncated;
        self.span_rows_truncated |= observed_tasks.truncated;
        self.process_last_seen
            .insert(observed_tasks.process_id, now_ns);
        let current_ids = observed_tasks
            .rows
            .iter()
            .map(ObservedTask::key)
            .collect::<BTreeSet<_>>();
        for task in self.tasks.values_mut() {
            if task.process_id == observed_tasks.process_id && !current_ids.contains(&task.key()) {
                task.state = "done".into();
                task.wait.clear();
            }
        }
        for task in &observed_tasks.rows {
            let key = task.key();
            if self.tasks.contains_key(&key)
                || self.tasks.len() < TRACE_TASK_ROW_LIMIT as usize
            {
                self.tasks.insert(key, task.clone());
            } else {
                self.task_rows_truncated = true;
            }
            if self.task_spans.contains_key(&key) {
                if let Some(span) = self.task_spans.get_mut(&key) {
                    span.end_ns = now_ns.max(span.start_ns);
                }
            } else if self.task_spans.len() < TRACE_SPAN_ROW_LIMIT as usize {
                self.task_spans.insert(
                    key,
                    ObservedTaskSpan {
                        end_ns: now_ns,
                        parent_task_id: task.parent,
                        start_ns: now_ns,
                        task: key,
                    },
                );
            } else {
                self.span_rows_truncated = true;
            }
        }
        let observed_io = observe_io(&observed_tasks.rows);
        let current = observed_io
            .iter()
            .map(|row| (row.task, row.wait.clone()))
            .collect::<BTreeSet<_>>();
        let ended = self
            .active
            .keys()
            .filter(|key| key.0.0 == observed_tasks.process_id && !current.contains(*key))
            .cloned()
            .collect::<Vec<_>>();
        for (task, wait) in ended {
            if let Some((kind, start_ns)) = self.active.remove(&(task, wait.clone())) {
                self.push(ObservedIoSpan {
                    end_ns: now_ns.max(start_ns),
                    kind,
                    start_ns,
                    task,
                    wait,
                });
            }
        }
        for row in observed_io {
            let key = (row.task, row.wait);
            if !self.tasks.contains_key(&row.task) {
                self.io_rows_truncated = true;
            } else if self.active.contains_key(&key) {
                continue;
            } else if self.active.len() + self.completed.len() < TRACE_IO_ROW_LIMIT as usize {
                self.active.insert(key, (row.kind, now_ns));
            } else {
                self.io_rows_truncated = true;
            }
        }
    }

    fn finish(&mut self, now_ns: u64) {
        for task in self.tasks.values_mut() {
            task.state = "done".into();
            task.wait.clear();
        }
        let active = std::mem::take(&mut self.active);
        for ((task, wait), (kind, start_ns)) in active {
            let end_ns = self
                .process_last_seen
                .get(&task.0)
                .copied()
                .unwrap_or(now_ns)
                .max(start_ns);
            self.push(ObservedIoSpan {
                end_ns,
                kind,
                start_ns,
                task,
                wait,
            });
        }
    }

    fn push(&mut self, span: ObservedIoSpan) {
        if self.completed.len() < TRACE_IO_ROW_LIMIT as usize {
            self.completed.push(span);
        } else {
            self.io_rows_truncated = true;
        }
    }

    fn tasks(&self) -> Vec<ObservedTask> {
        self.tasks.values().cloned().collect()
    }

    fn spans(&self) -> Vec<ObservedTaskSpan> {
        self.task_spans.values().cloned().collect()
    }
}

fn elapsed_ns(started: Instant) -> u64 {
    started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
}

/// Blocked observe tasks with real I/O waits only. Idle/time/channel scrapes omitted.
fn observe_io(tasks: &[ObservedTask]) -> Vec<ObservedIo> {
    let mut out = Vec::new();
    for task in tasks {
        if task.state != "blocked" {
            continue;
        }
        let Some(kind) = TraceIo::kind_for_wait(&task.wait) else {
            continue;
        };
        out.push(ObservedIo {
            kind: kind.into(),
            task: task.key(),
            wait: task.wait.clone(),
        });
    }
    out.sort_by_key(|row| row.task);
    out
}

fn json_array_inner<'a>(snapshot: &'a str, key: &str) -> Option<&'a str> {
    let tail = snapshot.split_once(&format!("\"{key}\":["))?.1;
    let mut depth = 1usize;
    let mut i = 0usize;
    let bytes = tail.as_bytes();
    while i < bytes.len() {
        match bytes[i] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&tail[..i]);
                }
            }
            b'"' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'"' {
                        break;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn split_json_objects(inner: &str, limit: usize) -> (Vec<&str>, bool) {
    let mut out = Vec::new();
    let bytes = inner.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i] == b',' || bytes[i].is_ascii_whitespace()) {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        if out.len() == limit {
            return (out, true);
        }
        if bytes[i] != b'{' {
            break;
        }
        let start = i;
        let mut depth = 0usize;
        while i < bytes.len() {
            match bytes[i] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        out.push(&inner[start..=i]);
                        i += 1;
                        break;
                    }
                }
                b'"' => {
                    i += 1;
                    while i < bytes.len() {
                        if bytes[i] == b'\\' {
                            i += 2;
                            continue;
                        }
                        if bytes[i] == b'"' {
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
    (out, false)
}

fn json_string(object: &str, key: &str) -> Option<String> {
    let tail = object.split_once(&format!("\"{key}\":\""))?.1;
    let mut out = String::new();
    let mut chars = tail.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => out.push(other),
            },
            '"' => return Some(out),
            other => out.push(other),
        }
    }
    None
}

/// `jet run` hosts compile then exec; observe publishes under the program PID.
struct PolledSnapshot {
    process_id: u32,
    process_cpu_ns: Option<u64>,
    tasks: ObservedTasks,
    text: String,
}

fn poll_observe_snapshot(root_pid: u32) -> Option<PolledSnapshot> {
    let mut stack = vec![root_pid];
    let mut seen = std::collections::BTreeSet::new();
    let mut best = None;
    let mut best_score = 0u64;
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        if let Ok(snapshot) = jet::DevServer::LiveInspect::read(pid) {
            let (count, bytes) = observe_arena_resources(&snapshot);
            let tasks = observe_tasks(&snapshot, pid);
            let locks = observe_locks(&snapshot);
            let io = observe_io(&tasks.rows);
            let child_tasks = tasks.rows.iter().filter(|task| task.parent > 0).count() as u64;
            let contended = locks.len() as u64;
            let io_n = io.len() as u64;
            let score = (if count > 0 || bytes > 0 { 100 } else { 0 })
                + child_tasks.saturating_mul(10)
                + contended.saturating_mul(20)
                + io_n.saturating_mul(20)
                + tasks.rows.len() as u64;
            if score > best_score {
                best_score = score;
                best = Some(PolledSnapshot {
                    process_id: pid,
                    process_cpu_ns: process_times(pid).map(|(_, cpu_ns)| cpu_ns),
                    tasks,
                    text: snapshot,
                });
            }
            // Prefer a snapshot that already shows alloc + spawned child + contention + I/O.
            if (count > 0 || bytes > 0) && child_tasks > 0 && contended > 0 && io_n > 0 {
                return best;
            }
        }
        stack.extend(process_children(pid));
    }
    best
}

fn process_children(pid: u32) -> Vec<u32> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let mut kids = std::collections::BTreeSet::new();
        if let Ok(entries) = fs::read_dir(format!("/proc/{pid}/task")) {
            for entry in entries.flatten() {
                if let Ok(text) = fs::read_to_string(entry.path().join("children")) {
                    for value in text.split_whitespace() {
                        if let Ok(child) = value.parse::<u32>() {
                            kids.insert(child);
                        }
                    }
                }
            }
        }
        if !kids.is_empty() {
            return kids.into_iter().collect();
        }
        let Ok(entries) = fs::read_dir("/proc") else {
            return Vec::new();
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(child) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
                continue;
            };
            let Ok(stat) = fs::read_to_string(format!("/proc/{child}/stat")) else {
                continue;
            };
            let Some(fields) = stat.rsplit_once(") ").map(|(_, tail)| tail) else {
                continue;
            };
            let mut parts = fields.split_whitespace();
            let _state = parts.next();
            let Some(ppid) = parts.next().and_then(|v| v.parse::<u32>().ok()) else {
                continue;
            };
            if ppid == pid {
                kids.insert(child);
            }
        }
        kids.into_iter().collect()
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = pid;
        Vec::new()
    }
}

fn json_u64(object: &str, key: &str) -> Option<u64> {
    let tail = object.split_once(&format!("\"{key}\":"))?.1;
    let digits: String = tail
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn process_times(pid: u32) -> Option<(u64, u64)> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let ticks_per_second = proc_clock_ticks_per_second()?;
        let uptime = fs::read_to_string("/proc/uptime").ok()?;
        let now_ticks = uptime_ticks(&uptime, ticks_per_second)?;
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let fields = stat.rsplit_once(") ")?.1;
        let fields: Vec<&str> = fields.split_whitespace().collect();
        let utime: u64 = fields.get(11)?.parse().ok()?;
        let stime: u64 = fields.get(12)?.parse().ok()?;
        let starttime: u64 = fields.get(19)?.parse().ok()?;
        let wall_ns = ticks_to_ns(now_ticks.checked_sub(starttime)?, ticks_per_second)?;
        let cpu_ns = ticks_to_ns(utime.checked_add(stime)?, ticks_per_second)?;
        Some((wall_ns, cpu_ns))
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = pid;
        None
    }
}

fn proc_clock_ticks_per_second() -> Option<u64> {
    let auxv = fs::read("/proc/self/auxv").ok()?;
    clock_ticks_per_second_from_auxv(&auxv)
}

fn clock_ticks_per_second_from_auxv(auxv: &[u8]) -> Option<u64> {
    const AT_CLKTCK: u64 = 17;
    for pair in auxv.chunks_exact(std::mem::size_of::<usize>() * 2) {
        let (tag, value) = pair.split_at(std::mem::size_of::<usize>());
        let tag = native_word(tag)?;
        if tag == 0 {
            break;
        }
        if tag == AT_CLKTCK {
            return match native_word(value)? {
                0 => None,
                ticks => Some(ticks),
            };
        }
    }
    None
}

fn native_word(bytes: &[u8]) -> Option<u64> {
    let word: [u8; std::mem::size_of::<usize>()] = bytes.try_into().ok()?;
    u64::try_from(usize::from_ne_bytes(word)).ok()
}

fn uptime_ticks(uptime: &str, ticks_per_second: u64) -> Option<u64> {
    if ticks_per_second == 0 {
        return None;
    }
    let uptime = uptime.split_whitespace().next()?;
    let (seconds, fraction) = uptime.split_once('.').unwrap_or((uptime, ""));
    let seconds: u128 = seconds.parse().ok()?;
    let mut ticks = seconds.checked_mul(ticks_per_second as u128)?;
    if !fraction.is_empty() {
        let scale = 10u128.checked_pow(fraction.len() as u32)?;
        let fraction: u128 = fraction.parse().ok()?;
        ticks = ticks.checked_add(
            fraction
                .checked_mul(ticks_per_second as u128)?
                .checked_div(scale)?,
        )?;
    }
    u64::try_from(ticks).ok()
}

fn ticks_to_ns(ticks: u64, ticks_per_second: u64) -> Option<u64> {
    if ticks_per_second == 0 {
        return None;
    }
    let nanos = (ticks as u128)
        .checked_mul(1_000_000_000)?
        .checked_div(ticks_per_second as u128)?;
    u64::try_from(nanos).ok()
}

fn view(args: &[String]) -> i32 {
    let mut path = None;
    let mut mode = ViewMode::Text;
    let mut frames = FramesMode::Jet;
    let mut i = 0usize;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--json" {
            mode = ViewMode::JSON;
            i += 1;
            continue;
        }
        if arg == "--html" {
            mode = ViewMode::HTML;
            i += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--frames=") {
            frames = match value {
                "jet" => FramesMode::Jet,
                "all" => FramesMode::All,
                _ => {
                    eprintln!("Error [E2102]: `--frames` accepts only `jet` or `all`");
                    return 2;
                }
            };
            i += 1;
            continue;
        }
        if arg == "--frames" {
            let Some(value) = args.get(i + 1) else {
                eprintln!("Error [E2102]: `--frames` needs `jet` or `all`");
                return 2;
            };
            frames = match value.as_str() {
                "jet" => FramesMode::Jet,
                "all" => FramesMode::All,
                _ => {
                    eprintln!("Error [E2102]: `--frames` accepts only `jet` or `all`");
                    return 2;
                }
            };
            i += 2;
            continue;
        }
        if arg.starts_with('-') {
            eprintln!("Error [E2102]: unknown `jet perf view` flag `{arg}`");
            eprintln!(
                " Fix: jet perf view <path{ARTIFACT_EXT_TRACE}> [--json|--html] [--frames=jet|all]"
            );
            return 2;
        }
        if path.is_some() {
            eprintln!("Error [E2102]: `jet perf view` takes one trace path");
            return 2;
        }
        path = Some(arg.to_string());
        i += 1;
    }
    let Some(path) = path else {
        eprintln!("Error [E2102]: `jet perf view` needs a {ARTIFACT_EXT_TRACE} path");
        return 2;
    };
    let trace = match read_verified(&path) {
        Ok(trace) => trace,
        Err(message) => {
            eprintln!("Error [E2102]: {message}");
            return 2;
        }
    };
    match mode {
        ViewMode::Text => {
            render_view_text(&trace, frames, use_color());
            0
        }
        ViewMode::JSON => {
            print!("{}", String::from_utf8_lossy(&view_json(&trace, frames).bytes()));
            0
        }
        ViewMode::HTML => {
            print!("{}", view_html(&trace, frames));
            0
        }
    }
}

enum ViewMode {
    Text,
    JSON,
    HTML,
}

#[derive(Clone, Copy)]
enum FramesMode {
    Jet,
    All,
}

fn use_color() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var_os("JET_FORCE_COLOR").is_some() {
        return true;
    }
    std::io::IsTerminal::is_terminal(&std::io::stdout())
}

fn render_view_text(trace: &CanonicalJson, frames: FramesMode, color: bool) {
    let id = trace_id(trace).unwrap_or("unknown");
    let command = content_command(trace).unwrap_or("unknown");
    let bold = if color { "\u{1b}[1m" } else { "" };
    let reset = if color { "\u{1b}[0m" } else { "" };
    println!("{bold}schema{reset} {TRACE_SCHEMA} v{TRACE_VERSION}");
    println!("{bold}trace{reset} {id}");
    println!("{bold}command{reset} {command}");
    println!(
        "{bold}frames{reset} {}",
        match frames {
            FramesMode::Jet => "jet",
            FramesMode::All => "all",
        }
    );
    if let Some((domain, ns, symbol)) = first_sample(trace) {
        println!("sample {domain} {ns}ns · {symbol}");
    }
    if let Some((count, bytes, symbol)) = first_allocation(trace) {
        println!("alloc count={count} bytes={bytes} · {symbol}");
    }
    if let Some((n, symbol)) = browser_summary(trace) {
        println!("browser count={n} · {symbol}");
    }
    if let Some((n, child_n, symbol)) = task_summary(trace) {
        println!("tasks count={n} children={child_n} · {symbol}");
    }
    if let Some((n, waiters, symbol)) = lock_summary(trace) {
        println!("locks count={n} waiters={waiters} · {symbol}");
    }
    if let Some((n, symbol)) = io_summary(trace) {
        println!("io count={n} · {symbol}");
    }
    if let Some(summary) = native_summary(trace) {
        println!("{summary}");
    }
    if let Some(summary) = span_summary(trace) {
        println!("{summary}");
    }
    if frames_show_generated(frames) {
        println!("generated-frames: none retained in .jettrace (hidden unless captured under --frames=all)");
    }
    let flame = ascii_flame(trace);
    if !flame.is_empty() {
        println!("{bold}flamegraph{reset}");
        for line in flame {
            println!("{line}");
        }
    }
    let timeline = ascii_timeline(trace);
    if !timeline.is_empty() {
        println!("{bold}timeline{reset}");
        for line in timeline {
            println!("{line}");
        }
    }
}

fn frames_show_generated(frames: FramesMode) -> bool {
    matches!(frames, FramesMode::All)
}

fn ascii_flame(trace: &CanonicalJson) -> Vec<String> {
    let mut rows = Vec::new();
    if let Some((domain, ns, symbol)) = first_sample(trace) {
        let width = ((ns.parse::<u64>().unwrap_or(1).min(40)) as usize).max(1);
        rows.push(format!("{symbol} {domain} {}", "#".repeat(width)));
    }
    if let Some((n, symbol)) = browser_summary(trace) {
        let width = n.min(40).max(1);
        rows.push(format!("{symbol} browser {}", "#".repeat(width)));
    }
    rows
}

fn ascii_timeline(trace: &CanonicalJson) -> Vec<String> {
    let mut rows = Vec::new();
    if let Some(items) = content_array(trace, "browser") {
        for item in items.iter().take(8) {
            let CanonicalJson::Object(fields) = item else {
                continue;
            };
            let start = match fields.get("start_ns") {
                Some(CanonicalJson::Integer(text)) => text.clone(),
                _ => continue,
            };
            let dur = match fields.get("duration_ns") {
                Some(CanonicalJson::Integer(text)) => text.clone(),
                _ => continue,
            };
            let name = match fields.get("symbol").and_then(symbol_label) {
                Some(name) => name,
                None => "browser".into(),
            };
            rows.push(format!("[{start}+{dur}] {name}"));
        }
    }
    if let Some(items) = content_array(trace, "spans") {
        for item in items.iter().take(8) {
            let CanonicalJson::Object(fields) = item else {
                continue;
            };
            if fields.get("status") != Some(&CanonicalJson::String("captured".into())) {
                continue;
            }
            let start = match fields.get("start_ns") {
                Some(CanonicalJson::Integer(text)) => text.clone(),
                _ => continue,
            };
            let end = match fields.get("end_ns") {
                Some(CanonicalJson::Integer(text)) => text.clone(),
                _ => continue,
            };
            let name = match fields.get("symbol").and_then(symbol_label) {
                Some(name) => name,
                None => "span".into(),
            };
            rows.push(format!("[{start}..{end}] {name}"));
        }
    }
    rows
}

fn view_json(trace: &CanonicalJson, frames: FramesMode) -> CanonicalJson {
    CanonicalJson::object([
        (
            "flamegraph".into(),
            CanonicalJson::Array(
                ascii_flame(trace)
                    .into_iter()
                    .map(CanonicalJson::String)
                    .collect(),
            ),
        ),
        (
            "frames".into(),
            CanonicalJson::String(
                match frames {
                    FramesMode::Jet => "jet",
                    FramesMode::All => "all",
                }
                .into(),
            ),
        ),
        ("kind".into(), CanonicalJson::String("jet.trace.view".into())),
        ("schema".into(), CanonicalJson::String(TRACE_SCHEMA.into())),
        (
            "timeline".into(),
            CanonicalJson::Array(
                ascii_timeline(trace)
                    .into_iter()
                    .map(CanonicalJson::String)
                    .collect(),
            ),
        ),
        ("trace".into(), trace.clone()),
        ("version".into(), CanonicalJson::Integer(TRACE_VERSION.into())),
    ])
    .expect("view json keys are unique")
}

fn view_html(trace: &CanonicalJson, frames: FramesMode) -> String {
    let id = trace_id(trace).unwrap_or("unknown");
    let flame = ascii_flame(trace)
        .into_iter()
        .map(|line| format!("<li>{}</li>", html_escape(&line)))
        .collect::<Vec<_>>()
        .join("");
    let timeline = ascii_timeline(trace)
        .into_iter()
        .map(|line| format!("<li>{}</li>", html_escape(&line)))
        .collect::<Vec<_>>()
        .join("");
    let frames_label = match frames {
        FramesMode::Jet => "jet",
        FramesMode::All => "all",
    };
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>jettrace {id}</title>
<style>
body{{font:14px/1.4 ui-monospace,monospace;margin:1.5rem;background:#111;color:#eee}}
h1,h2{{font-weight:600}}
.bar{{display:inline-block;height:10px;background:#6cf;margin-left:.5rem}}
section{{margin:1rem 0}}
</style></head><body>
<h1>jettrace {id}</h1>
<p>schema {TRACE_SCHEMA} v{TRACE_VERSION} · frames={frames_label}</p>
<section><h2>flamegraph</h2><ul id="flame">{flame}</ul></section>
<section><h2>timeline</h2><ul id="timeline">{timeline}</ul></section>
<script>
const flame=document.getElementById('flame');
for (const li of flame.querySelectorAll('li')) {{
  const m=li.textContent.match(/#+/);
  if(!m) continue;
  const bar=document.createElement('span');
  bar.className='bar';
  bar.style.width=(m[0].length*4)+'px';
  li.appendChild(bar);
}}
</script>
</body></html>
"#
    )
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn compare(args: &[String]) -> i32 {
    let mut paths = Vec::new();
    let mut override_identity = false;
    let mut baseline_name = None;
    let mut i = 0usize;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--override-identity" {
            override_identity = true;
            i += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--baseline=") {
            baseline_name = Some(value.to_string());
            i += 1;
            continue;
        }
        if arg == "--baseline" {
            let Some(value) = args.get(i + 1) else {
                eprintln!("Error [E2102]: `--baseline` needs a pinned baseline name");
                return 2;
            };
            baseline_name = Some(value.clone());
            i += 2;
            continue;
        }
        if arg.starts_with('-') {
            eprintln!("Error [E2102]: unknown `jet perf compare` flag `{arg}`");
            eprintln!(
                " Fix: jet perf compare base{ARTIFACT_EXT_TRACE} head{ARTIFACT_EXT_TRACE} [--baseline <name>] [--override-identity]"
            );
            return 2;
        }
        paths.push(arg.to_string());
        i += 1;
    }
    if paths.len() != 2 {
        eprintln!("Error [E2102]: `jet perf compare` needs two {ARTIFACT_EXT_TRACE} paths");
        eprintln!(
            " Fix: jet perf compare base{ARTIFACT_EXT_TRACE} head{ARTIFACT_EXT_TRACE} [--baseline <name>] [--override-identity]"
        );
        return 2;
    }
    let base = match read_verified(&paths[0]) {
        Ok(trace) => trace,
        Err(message) => {
            eprintln!("Error [E2102]: base trace: {message}");
            return 2;
        }
    };
    let head = match read_verified(&paths[1]) {
        Ok(trace) => trace,
        Err(message) => {
            eprintln!("Error [E2102]: head trace: {message}");
            return 2;
        }
    };
    let base_tool = toolchain_digest(&base);
    let head_tool = toolchain_digest(&head);
    let base_hw = hardware_fingerprint(&base);
    let head_hw = hardware_fingerprint(&head);
    let identity_mismatch = base_tool != head_tool || base_hw != head_hw;
    if identity_mismatch && !override_identity {
        if base_tool != head_tool {
            eprintln!("Error [E2102]: toolchain identity mismatch between traces");
            eprintln!(" Why: compare requires matching toolchain digests (D-PERFSESSION1)");
        } else {
            eprintln!("Error [E2102]: hardware identity mismatch between traces");
            eprintln!(" Why: compare requires matching hardware fingerprints (D-PERFSESSION1)");
        }
        eprintln!(
            " Fix: recapture both traces on the same machine/toolchain, or pass `--override-identity`"
        );
        return 1;
    }
    if let Some(name) = &baseline_name {
        if let Err(message) = require_pinned_baseline(name) {
            eprintln!("Error [E2102]: {message}");
            eprintln!(" Why: `--baseline` selects a D-PERFBUDGET-BASELINE1 pinned name");
            eprintln!(" Fix: create the baseline with `jet budget update --baseline {name}`, or omit `--baseline`");
            return 1;
        }
    }
    let deltas = compare_domain_deltas(&base, &head);
    let budget_line = budget_compare_line(&base, &head, baseline_name.as_deref());
    let override_note = if identity_mismatch && override_identity {
        " · identity override"
    } else {
        ""
    };
    let baseline_note = baseline_name
        .as_ref()
        .map(|name| format!(" · baseline {name}"))
        .unwrap_or_default();
    println!(
        "compare ok · schema {TRACE_SCHEMA} v{TRACE_VERSION} · base {} · head {}{override_note}{baseline_note}",
        trace_id(&base).unwrap_or("unknown"),
        trace_id(&head).unwrap_or("unknown"),
    );
    if !deltas.is_empty() {
        println!("deltas: {}", deltas.join(" · "));
    }
    println!("{budget_line}");
    0
}

fn require_pinned_baseline(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.contains('.')
        || name.contains('\\')
        || name.starts_with('/')
        || name.split('/').any(|part| part.is_empty() || part == "." || part == "..")
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'/')
    {
        return Err(format!("baseline name `{name}` is not a pinned BaselineName"));
    }
    let cwd = std::env::current_dir().map_err(|e| format!("cannot resolve cwd: {e}"))?;
    let root = jet::Loader::find_manifest_root(&cwd).unwrap_or(cwd);
    let path = root
        .join(".jet")
        .join("perf")
        .join("baselines")
        .join("names")
        .join(format!("{name}.json"));
    if !path.is_file() {
        return Err(format!(
            "pinned baseline `{name}` is missing at {}",
            path.display()
        ));
    }
    Ok(())
}

fn hardware_fingerprint(trace: &CanonicalJson) -> Option<String> {
    content_object(trace)?
        .get("hardware")
        .and_then(|value| match value {
            CanonicalJson::Object(fields) => match fields.get("fingerprint") {
                Some(CanonicalJson::String(text)) => Some(text.clone()),
                _ => None,
            },
            _ => None,
        })
}

fn compare_domain_deltas(base: &CanonicalJson, head: &CanonicalJson) -> Vec<String> {
    let mut out = Vec::new();
    if let (Some(b), Some(h)) = (sample_duration(base, "wall"), sample_duration(head, "wall")) {
        out.push(format!("wall {b}->{h} ns"));
    }
    if let (Some(b), Some(h)) = (sample_duration(base, "cpu"), sample_duration(head, "cpu")) {
        out.push(format!("cpu {b}->{h} ns"));
    }
    if let (Some(b), Some(h)) = (allocation_bytes(base), allocation_bytes(head)) {
        out.push(format!("alloc {b}->{h} B"));
    }
    out
}

fn sample_duration(trace: &CanonicalJson, domain: &str) -> Option<u64> {
    let samples = match content_object(trace)?.get("samples")? {
        CanonicalJson::Array(items) => items,
        _ => return None,
    };
    for sample in samples {
        let CanonicalJson::Object(fields) = sample else {
            continue;
        };
        if fields.get("domain") == Some(&CanonicalJson::String(domain.into())) {
            if let Some(CanonicalJson::Integer(text)) = fields.get("duration_ns") {
                return text.parse().ok();
            }
        }
    }
    None
}

fn allocation_bytes(trace: &CanonicalJson) -> Option<u64> {
    let allocations = match content_object(trace)?.get("allocations")? {
        CanonicalJson::Array(items) => items,
        _ => return None,
    };
    let CanonicalJson::Object(fields) = allocations.first()? else {
        return None;
    };
    match fields.get("bytes")? {
        CanonicalJson::Integer(text) => text.parse().ok(),
        _ => None,
    }
}

fn budget_compare_line(base: &CanonicalJson, head: &CanonicalJson, baseline: Option<&str>) -> String {
    // #241 budgets: wall AbsoluteFrom/RelativeTo against the pinned base trace.
    match (sample_duration(base, "wall"), sample_duration(head, "wall")) {
        (Some(base_ns), Some(head_ns)) if base_ns > 0 => {
            let bad = head_ns.saturating_sub(base_ns);
            let bp = (bad as u128).saturating_mul(10_000) / base_ns as u128;
            let name = baseline.unwrap_or("trace-baseline");
            format!(
                "budgets: wall RelativeTo({name}) regression={bp}bp · base={base_ns}ns head={head_ns}ns"
            )
        }
        _ => "budgets: no comparable wall samples for #241 RelativeTo evaluation".into(),
    }
}

fn export(args: &[String]) -> i32 {
    let mut path = None;
    let mut mode = ExportMode::JSON;
    let mut i = 0usize;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--json" => {
                mode = ExportMode::JSON;
                i += 1;
                continue;
            }
            "--pprof" => {
                mode = ExportMode::Pprof;
                i += 1;
                continue;
            }
            "--otel" => {
                mode = ExportMode::Otel;
                i += 1;
                continue;
            }
            "--chrome" => {
                mode = ExportMode::Chrome;
                i += 1;
                continue;
            }
            "--emit-profile-map" => {
                mode = ExportMode::ProfileMap;
                i += 1;
                continue;
            }
            _ if arg.starts_with('-') => {
                eprintln!("Error [E2102]: unknown `jet perf export` flag `{arg}`");
                eprintln!(
                    " Fix: jet perf export <path{ARTIFACT_EXT_TRACE}> [--json|--pprof|--otel|--chrome|--emit-profile-map]"
                );
                return 2;
            }
            _ => {}
        }
        if path.is_some() {
            eprintln!("Error [E2102]: `jet perf export` takes one trace path");
            return 2;
        }
        path = Some(arg.to_string());
        i += 1;
    }
    let Some(path) = path else {
        eprintln!("Error [E2102]: `jet perf export` needs a {ARTIFACT_EXT_TRACE} path");
        return 2;
    };
    let trace = match read_verified(&path) {
        Ok(trace) => trace,
        Err(message) => {
            eprintln!("Error [E2102]: {message}");
            return 2;
        }
    };
    let projection = match mode {
        ExportMode::JSON => export_json_envelope(&trace),
        ExportMode::Pprof => export_pprof_projection(&trace),
        ExportMode::Otel => export_otel_projection(&trace),
        ExportMode::Chrome => export_chrome_projection(&trace),
        ExportMode::ProfileMap => export_profile_map_projection(&trace),
    };
    print!("{}", String::from_utf8_lossy(&projection.bytes()));
    0
}

enum ExportMode {
    JSON,
    Pprof,
    Otel,
    Chrome,
    ProfileMap,
}

fn export_json_envelope(trace: &CanonicalJson) -> CanonicalJson {
    let loss = if first_sample(trace).is_some()
        || first_allocation(trace).is_some()
        || browser_summary(trace).is_some()
        || task_summary(trace).is_some()
        || lock_summary(trace).is_some()
        || io_summary(trace).is_some()
        || native_summary(trace).is_some()
        || span_summary(trace).is_some()
    {
        "json-envelope; domains present are wall/cpu/alloc/browser/tasks/locks/io/native/spans only — no pprof/otel/chrome payloads"
    } else {
        "json-envelope-only; no pprof/otel/chrome payloads in skeleton"
    };
    CanonicalJson::object([
        ("kind".into(), CanonicalJson::String("jet.trace.projection".into())),
        ("loss".into(), CanonicalJson::String(loss.into())),
        ("schema".into(), CanonicalJson::String(TRACE_SCHEMA.into())),
        ("trace".into(), trace.clone()),
        ("version".into(), CanonicalJson::Integer(TRACE_VERSION.into())),
    ])
    .expect("projection keys are unique")
}

fn export_pprof_projection(trace: &CanonicalJson) -> CanonicalJson {
    let mut samples = Vec::new();
    if let Some(wall) = sample_duration(trace, "wall") {
        samples.push(CanonicalJson::object([
            ("location".into(), CanonicalJson::String("wall".into())),
            ("value".into(), CanonicalJson::Integer(wall.to_string())),
        ]).unwrap());
    }
    if let Some(cpu) = sample_duration(trace, "cpu") {
        samples.push(CanonicalJson::object([
            ("location".into(), CanonicalJson::String("cpu".into())),
            ("value".into(), CanonicalJson::Integer(cpu.to_string())),
        ]).unwrap());
    }
    CanonicalJson::object([
        ("kind".into(), CanonicalJson::String("jet.trace.pprof-projection".into())),
        (
            "loss".into(),
            CanonicalJson::String(
                "pprof-json; samples are Jet wall/cpu only — no native stack frames, no gzip proto, no labels beyond domain"
                    .into(),
            ),
        ),
        ("samples".into(), CanonicalJson::Array(samples)),
        ("schema".into(), CanonicalJson::String(TRACE_SCHEMA.into())),
        ("trace_id".into(), CanonicalJson::String(trace_id(trace).unwrap_or("unknown").into())),
        ("version".into(), CanonicalJson::Integer(TRACE_VERSION.into())),
    ])
    .expect("pprof projection keys are unique")
}

fn export_otel_projection(trace: &CanonicalJson) -> CanonicalJson {
    let mut spans = Vec::new();
    if let Some(content) = content_object(trace) {
        if let Some(CanonicalJson::Array(items)) = content.get("spans") {
            for item in items {
                spans.push(item.clone());
            }
        }
        if let Some(CanonicalJson::Array(items)) = content.get("browser") {
            for item in items {
                spans.push(item.clone());
            }
        }
    }
    CanonicalJson::object([
        ("kind".into(), CanonicalJson::String("jet.trace.otel-projection".into())),
        (
            "loss".into(),
            CanonicalJson::String(
                "otel-json; Jet spans/browser only — no resource attributes, no OTLP protobuf, no baggage/traceparent wire"
                    .into(),
            ),
        ),
        ("schema".into(), CanonicalJson::String(TRACE_SCHEMA.into())),
        ("spans".into(), CanonicalJson::Array(spans)),
        ("trace_id".into(), CanonicalJson::String(trace_id(trace).unwrap_or("unknown").into())),
        ("version".into(), CanonicalJson::Integer(TRACE_VERSION.into())),
    ])
    .expect("otel projection keys are unique")
}

fn export_chrome_projection(trace: &CanonicalJson) -> CanonicalJson {
    let mut events = Vec::new();
    if let Some(wall) = sample_duration(trace, "wall") {
        events.push(
            CanonicalJson::object([
                ("dur".into(), CanonicalJson::Integer(wall.to_string())),
                ("name".into(), CanonicalJson::String("wall".into())),
                ("ph".into(), CanonicalJson::String("X".into())),
                ("pid".into(), CanonicalJson::Integer("1".into())),
                ("tid".into(), CanonicalJson::Integer("1".into())),
                ("ts".into(), CanonicalJson::Integer("0".into())),
            ])
            .unwrap(),
        );
    }
    if let Some(content) = content_object(trace) {
        if let Some(CanonicalJson::Array(items)) = content.get("browser") {
            for item in items {
                let CanonicalJson::Object(fields) = item else {
                    continue;
                };
                let start = match fields.get("start_ns") {
                    Some(CanonicalJson::Integer(text)) => text.clone(),
                    _ => continue,
                };
                let dur = match fields.get("duration_ns") {
                    Some(CanonicalJson::Integer(text)) => text.clone(),
                    _ => continue,
                };
                let name = match fields.get("symbol") {
                    Some(CanonicalJson::Object(symbol)) => match symbol.get("name") {
                        Some(CanonicalJson::String(name)) => name.clone(),
                        _ => "browser".into(),
                    },
                    _ => "browser".into(),
                };
                events.push(
                    CanonicalJson::object([
                        ("dur".into(), CanonicalJson::Integer(dur)),
                        ("name".into(), CanonicalJson::String(name)),
                        ("ph".into(), CanonicalJson::String("X".into())),
                        ("pid".into(), CanonicalJson::Integer("1".into())),
                        ("tid".into(), CanonicalJson::Integer("2".into())),
                        ("ts".into(), CanonicalJson::Integer(start)),
                    ])
                    .unwrap(),
                );
            }
        }
    }
    CanonicalJson::object([
        (
            "kind".into(),
            CanonicalJson::String("jet.trace.chrome-projection".into()),
        ),
        (
            "loss".into(),
            CanonicalJson::String(
                "chrome-trace-json; X events from wall/browser only — no thread names, no async flows, no screenshot/counter tracks"
                    .into(),
            ),
        ),
        ("schema".into(), CanonicalJson::String(TRACE_SCHEMA.into())),
        ("traceEvents".into(), CanonicalJson::Array(events)),
        ("trace_id".into(), CanonicalJson::String(trace_id(trace).unwrap_or("unknown").into())),
        ("version".into(), CanonicalJson::Integer(TRACE_VERSION.into())),
    ])
    .expect("chrome projection keys are unique")
}

fn export_profile_map_projection(trace: &CanonicalJson) -> CanonicalJson {
    let mut symbols = Vec::new();
    let mut maps = Vec::new();
    if let Some(content) = content_object(trace) {
        if let Some(CanonicalJson::Array(items)) = content.get("source_identity") {
            for item in items {
                symbols.push(item.clone());
            }
        }
        if let Some(CanonicalJson::Array(items)) = content.get("source_maps") {
            for item in items {
                maps.push(item.clone());
            }
        }
    }
    CanonicalJson::object([
        (
            "kind".into(),
            CanonicalJson::String("jet.trace.profile-map-projection".into()),
        ),
        (
            "loss".into(),
            CanonicalJson::String(
                "profile-map; source_identity+source_maps only — no Rust/LLVM frames, no address ranges, no DWARF"
                    .into(),
            ),
        ),
        ("schema".into(), CanonicalJson::String(TRACE_SCHEMA.into())),
        ("source_identity".into(), CanonicalJson::Array(symbols)),
        ("source_maps".into(), CanonicalJson::Array(maps)),
        ("trace_id".into(), CanonicalJson::String(trace_id(trace).unwrap_or("unknown").into())),
        ("version".into(), CanonicalJson::Integer(TRACE_VERSION.into())),
    ])
    .expect("profile-map projection keys are unique")
}

fn write_session_trace(
    command: &str,
    argv: &[String],
    out: Option<&str>,
    capture: CaptureBundle,
    capture_allowlist: &[String],
) -> Result<PathBuf, String> {
    let mut capture_policy = CapturePolicy::default_exclusions();
    capture_policy.allowlist = capture_allowlist.to_vec();
    capture_policy.io_rows_truncated = capture.io_rows_truncated;
    capture_policy.browser_rows_truncated = capture.browser_rows_truncated;
    capture_policy.native_rows_truncated = capture.native_rows_truncated;
    capture_policy.span_rows_truncated = capture.span_rows_truncated;
    capture_policy.task_rows_truncated = capture.task_rows_truncated;
    let skeleton = TraceSkeleton {
        command: command.into(),
        argv: argv.to_vec(),
        toolchain: current_toolchain(),
        hardware: TraceHardware::current(),
        capture_policy,
        samples: capture.samples,
        allocations: capture.allocations,
        browser: capture.browser,
        tasks: capture.tasks,
        locks: capture.locks,
        io: capture.io,
        native: capture.native,
        spans: capture.spans,
        source_identity: capture.source_identity,
        source_maps: capture.source_maps,
    };
    let bytes = build_skeleton_bytes(&skeleton)?;
    let path = match out {
        Some(path) => PathBuf::from(path),
        None => default_trace_path(&bytes)?,
    };
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
    }
    write_trace_file(&path, &bytes)?;
    let written = fs::read(&path).map_err(|e| format!("cannot re-read {}: {e}", path.display()))?;
    verify_jettrace(&written).map_err(|e| format!("wrote unverifiable jettrace: {e}"))?;
    Ok(path)
}

fn default_trace_path(bytes: &[u8]) -> Result<PathBuf, String> {
    let verified = verify_jettrace(bytes)?;
    let id = trace_id(&verified)?.to_string();
    let stamp = utc_stamp();
    let short = &id[..8];
    let cwd = std::env::current_dir().map_err(|e| format!("cannot resolve cwd: {e}"))?;
    let root = jet::Loader::find_manifest_root(&cwd).unwrap_or(cwd);
    Ok(root.join(".jet").join("perf").join(format!("{stamp}-{short}{}", artifact_extension())))
}

fn write_trace_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)
            .map_err(|e| format!("cannot create temp trace: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = file.metadata().map_err(|e| e.to_string())?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&tmp, perms).map_err(|e| e.to_string())?;
        }
        file.write_all(bytes).map_err(|e| format!("cannot write temp trace: {e}"))?;
        file.sync_all().map_err(|e| format!("cannot durable-write temp trace: {e}"))?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("cannot install trace {}: {e}", path.display())
    })?;
    Ok(())
}

fn read_verified(path: &str) -> Result<CanonicalJson, String> {
    if !path.ends_with(ARTIFACT_EXT_TRACE) {
        return Err(format!("trace path must end with {ARTIFACT_EXT_TRACE}"));
    }
    let bytes = fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    verify_jettrace(&bytes)
}

fn current_toolchain() -> TraceToolchain {
    TraceToolchain {
        jet_version: env!("CARGO_PKG_VERSION").into(),
        compiler_build_id: option_env!("JET_COMPILER_BUILD_ID")
            .unwrap_or(env!("CARGO_PKG_VERSION"))
            .into(),
        stdlib_id: "jet-stdlib".into(),
        runner_id: "jet-perf".into(),
    }
}

fn utc_stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // ponytail: epoch stamp, calendar UTC when capture clock lands.
    format!("{secs}")
}

fn process_exists(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

fn content_command(trace: &CanonicalJson) -> Option<&str> {
    let CanonicalJson::Object(fields) = trace else { return None };
    let CanonicalJson::Object(content) = fields.get("content")? else { return None };
    match content.get("command")? {
        CanonicalJson::String(command) => Some(command.as_str()),
        _ => None,
    }
}

fn toolchain_digest(trace: &CanonicalJson) -> Option<&str> {
    let CanonicalJson::Object(fields) = trace else { return None };
    let CanonicalJson::Object(content) = fields.get("content")? else { return None };
    let CanonicalJson::Object(toolchain) = content.get("toolchain")? else { return None };
    match toolchain.get("digest")? {
        CanonicalJson::String(digest) => Some(digest.as_str()),
        _ => None,
    }
}

fn content_object(trace: &CanonicalJson) -> Option<&BTreeMap<String, CanonicalJson>> {
    let CanonicalJson::Object(fields) = trace else {
        return None;
    };
    match fields.get("content")? {
        CanonicalJson::Object(content) => Some(content),
        _ => None,
    }
}

fn content_array<'a>(trace: &'a CanonicalJson, key: &str) -> Option<&'a [CanonicalJson]> {
    match content_object(trace)?.get(key)? {
        CanonicalJson::Array(items) => Some(items.as_slice()),
        _ => None,
    }
}

fn first_sample(trace: &CanonicalJson) -> Option<(String, String, String)> {
    let sample = content_array(trace, "samples")?.first()?;
    let CanonicalJson::Object(fields) = sample else { return None };
    let domain = match fields.get("domain")? {
        CanonicalJson::String(domain) => domain.clone(),
        _ => return None,
    };
    let ns = match fields.get("duration_ns")? {
        CanonicalJson::Integer(ns) => ns.clone(),
        _ => return None,
    };
    let symbol = symbol_label(fields.get("symbol")?)?;
    Some((domain, ns, symbol))
}

fn first_allocation(trace: &CanonicalJson) -> Option<(String, String, String)> {
    let alloc = content_array(trace, "allocations")?.first()?;
    let CanonicalJson::Object(fields) = alloc else { return None };
    let count = match fields.get("count")? {
        CanonicalJson::Integer(count) => count.clone(),
        _ => return None,
    };
    let bytes = match fields.get("bytes")? {
        CanonicalJson::Integer(bytes) => bytes.clone(),
        _ => return None,
    };
    let symbol = symbol_label(fields.get("symbol")?)?;
    Some((count, bytes, symbol))
}

fn browser_summary(trace: &CanonicalJson) -> Option<(usize, String)> {
    let items = content_array(trace, "browser")?;
    let CanonicalJson::Object(first) = items.first()? else { return None };
    Some((items.len(), symbol_label(first.get("symbol")?)?))
}

fn task_summary(trace: &CanonicalJson) -> Option<(usize, usize, String)> {
    let items = content_array(trace, "tasks")?;
    if items.is_empty() {
        return None;
    }
    let mut children = 0usize;
    let mut symbol = None;
    for item in items {
        let CanonicalJson::Object(fields) = item else {
            continue;
        };
        if let Some(CanonicalJson::Integer(parent)) = fields.get("parent") {
            if parent != "0" {
                children += 1;
            }
        }
        if symbol.is_none() {
            if let Some(value) = fields.get("symbol") {
                symbol = symbol_label(value);
            }
        }
    }
    Some((items.len(), children, symbol.unwrap_or_else(|| "?".into())))
}

fn lock_summary(trace: &CanonicalJson) -> Option<(usize, u64, String)> {
    let items = content_array(trace, "locks")?;
    if items.is_empty() {
        return None;
    }
    let mut waiters = 0u64;
    let mut symbol = None;
    for item in items {
        let CanonicalJson::Object(fields) = item else {
            continue;
        };
        if let Some(CanonicalJson::Integer(send)) = fields.get("send_waiters") {
            waiters = waiters.saturating_add(send.parse().unwrap_or(0));
        }
        if let Some(CanonicalJson::Integer(recv)) = fields.get("recv_waiters") {
            waiters = waiters.saturating_add(recv.parse().unwrap_or(0));
        }
        if symbol.is_none() {
            if let Some(value) = fields.get("symbol") {
                symbol = symbol_label(value);
            }
        }
    }
    Some((items.len(), waiters, symbol.unwrap_or_else(|| "?".into())))
}

fn io_summary(trace: &CanonicalJson) -> Option<(usize, String)> {
    let items = content_array(trace, "io")?;
    if items.is_empty() {
        return None;
    }
    let mut symbol = None;
    for item in items {
        let CanonicalJson::Object(fields) = item else {
            continue;
        };
        if symbol.is_none() {
            if let Some(value) = fields.get("symbol") {
                symbol = symbol_label(value);
            }
        }
    }
    Some((items.len(), symbol.unwrap_or_else(|| "?".into())))
}

fn native_summary(trace: &CanonicalJson) -> Option<String> {
    let item = content_array(trace, "native")?.first()?;
    let CanonicalJson::Object(fields) = item else {
        return None;
    };
    let status = match fields.get("status")? {
        CanonicalJson::String(value) => value.as_str(),
        _ => return None,
    };
    let target = match fields.get("target")? {
        CanonicalJson::String(value) => value.as_str(),
        _ => return None,
    };
    let symbol = fields
        .get("symbol")
        .and_then(symbol_label)
        .unwrap_or_else(|| "?".into());
    match status {
        "captured" => {
            let duration = match fields.get("duration_ns")? {
                CanonicalJson::Integer(value) => value.as_str(),
                _ => return None,
            };
            let observed = match fields.get("observed_at_ns")? {
                CanonicalJson::Integer(value) => value.as_str(),
                _ => return None,
            };
            let task = match fields.get("task_id")? {
                CanonicalJson::Integer(value) => value.as_str(),
                _ => return None,
            };
            Some(format!(
                "native process_cpu={duration}ns observed_at={observed}ns target={target} task={task} · {symbol}"
            ))
        }
        "unavailable" => {
            let reason = match fields.get("reason")? {
                CanonicalJson::String(value) => value.as_str(),
                _ => return None,
            };
            Some(format!("native unavailable target={target} reason={reason} · {symbol}"))
        }
        _ => None,
    }
}

fn span_summary(trace: &CanonicalJson) -> Option<String> {
    let items = content_array(trace, "spans")?;
    let CanonicalJson::Object(first) = items.first()? else {
        return None;
    };
    let status = match first.get("status")? {
        CanonicalJson::String(value) => value.as_str(),
        _ => return None,
    };
    let symbol = first
        .get("symbol")
        .and_then(symbol_label)
        .unwrap_or_else(|| "?".into());
    if status == "unavailable" {
        let reason = match first.get("reason")? {
            CanonicalJson::String(value) => value.as_str(),
            _ => return None,
        };
        return Some(format!("spans unavailable reason={reason} · {symbol}"));
    }
    if status != "captured" {
        return None;
    }
    let mut start = u64::MAX;
    let mut end = 0u64;
    let mut children = 0usize;
    for item in items {
        let CanonicalJson::Object(fields) = item else {
            return None;
        };
        let CanonicalJson::Integer(item_start) = fields.get("start_ns")? else {
            return None;
        };
        let CanonicalJson::Integer(item_end) = fields.get("end_ns")? else {
            return None;
        };
        start = start.min(item_start.parse().ok()?);
        end = end.max(item_end.parse().ok()?);
        if !matches!(fields.get("parent_task_id"), Some(CanonicalJson::Null)) {
            children += 1;
        }
    }
    Some(format!(
        "spans count={} children={children} window={start}..{end}ns · {symbol}",
        items.len()
    ))
}

fn symbol_label(value: &CanonicalJson) -> Option<String> {
    let CanonicalJson::Object(fields) = value else { return None };
    let path = match fields.get("path")? {
        CanonicalJson::String(path) => path.as_str(),
        _ => return None,
    };
    let name = match fields.get("name")? {
        CanonicalJson::String(name) => name.as_str(),
        _ => return None,
    };
    Some(format!("{path}#{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_ingestion_is_capped_and_audits_possible_io_loss() {
        let tasks = (1..=TRACE_TASK_ROW_LIMIT + 1)
            .map(|id| {
                format!(
                    "{{\"id\":{id},\"parent\":0,\"state\":\"blocked\",\"wait\":\"tcp accept\",\"cancelled\":false}}"
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let snapshot = format!("{{\"tasks\":[{tasks}]}}");
        let observed = observe_tasks(&snapshot, 10);
        assert_eq!(observed.rows.len(), TRACE_TASK_ROW_LIMIT as usize);
        assert!(observed.truncated);

        let mut timeline = IOTimeline::default();
        timeline.observe(&observed, 10);
        timeline.finish(20);
        assert_eq!(timeline.tasks.len(), TRACE_TASK_ROW_LIMIT as usize);
        assert_eq!(timeline.completed.len(), TRACE_IO_ROW_LIMIT as usize);
        assert_eq!(timeline.task_spans.len(), TRACE_SPAN_ROW_LIMIT as usize);
        assert!(timeline.task_rows_truncated);
        assert!(timeline.io_rows_truncated);
        assert!(timeline.span_rows_truncated);
    }

    #[test]
    fn sequential_processes_with_reused_runtime_task_ids_remain_distinct() {
        let snapshot = "{\"tasks\":[{\"id\":1,\"parent\":0,\"state\":\"running\",\"wait\":\"\",\"cancelled\":false},{\"id\":2,\"parent\":1,\"state\":\"blocked\",\"wait\":\"tcp accept\",\"cancelled\":false}]}";
        let first = observe_tasks(snapshot, 100);
        let second = observe_tasks(snapshot, 200);
        let mut timeline = IOTimeline::default();
        timeline.observe(&first, 10);
        timeline.observe(&second, 20);
        timeline.finish(30);

        let tasks = timeline.tasks();
        let ids = trace_task_id_map(&tasks);
        assert_eq!(tasks.len(), 4);
        assert_eq!(timeline.spans().len(), 4);
        assert_eq!(timeline.completed.len(), 2);
        assert_eq!(timeline.completed[0].end_ns, 10);
        assert_eq!(timeline.completed[1].end_ns, 20);
        assert_ne!(ids[&(100, 1)], ids[&(200, 1)]);
        assert_ne!(ids[&(100, 2)], ids[&(200, 2)]);
        assert_eq!(ids[&(100, 1)], 1);
        assert_eq!(ids[&(100, 2)], 2);
        assert_eq!(ids[&(200, 1)], 3);
        assert_eq!(ids[&(200, 2)], 4);
        for process_id in [100, 200] {
            let child = tasks
                .iter()
                .find(|task| task.key() == (process_id, 2))
                .unwrap();
            assert_eq!(ids[&(process_id, child.parent)], ids[&(process_id, 1)]);
        }
    }

    #[test]
    fn native_process_cpu_support_matrix_is_explicit() {
        assert!(native_process_cpu_supported("linux"));
        assert!(native_process_cpu_supported("android"));
        assert!(!native_process_cpu_supported("macos"));
        assert!(!native_process_cpu_supported("windows"));
        assert!(!native_process_cpu_supported("wasi"));
        assert_eq!(
            native_unavailable_reason_for("windows", "x86_64-pc-windows-msvc"),
            "process CPU timing is unavailable on target x86_64-pc-windows-msvc"
        );
        assert_eq!(
            native_unavailable_reason_for("linux", "x86_64-unknown-linux-gnu"),
            "process CPU timing was not observable"
        );
    }

    #[test]
    fn proc_tick_conversion_uses_the_injected_rate_without_overflow() {
        let mut auxv = Vec::new();
        auxv.extend_from_slice(&17usize.to_ne_bytes());
        auxv.extend_from_slice(&250usize.to_ne_bytes());
        auxv.extend_from_slice(&0usize.to_ne_bytes());
        auxv.extend_from_slice(&0usize.to_ne_bytes());
        assert_eq!(clock_ticks_per_second_from_auxv(&auxv), Some(250));
        assert_eq!(uptime_ticks("2.50 1.00\n", 100), Some(250));
        assert_eq!(uptime_ticks("2.50 1.00\n", 250), Some(625));
        assert_eq!(ticks_to_ns(1, 100), Some(10_000_000));
        assert_eq!(ticks_to_ns(1, 250), Some(4_000_000));
        assert_eq!(ticks_to_ns(1, 1_000), Some(1_000_000));
        assert_eq!(ticks_to_ns(1, 0), None);
        assert_eq!(ticks_to_ns(u64::MAX, 1), None);
    }
}
