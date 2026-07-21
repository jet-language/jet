//! D-PERFSESSION1=D: `jet perf` family over one versioned `.jettrace` truth.
//!
//! `run`/`test`/`bench` spawn the exact base-intent driver (`jet run|test|bench …`)
//! with observe enabled, poll live facts while the child runs, then write one
//! `.jettrace` with wall/alloc/tasks/locks/io before exiting with the child's code.
//! `attach`/`view`/`compare`/`export` share the same artifact verify seam.
//! Capture reuses the observe live snapshot (D-OBSERVE-LIVE1) attributed to a
//! Jet source symbol.

use jet_foundation::JetTrace::{
    artifact_extension, build_skeleton_bytes, entrypoint_name_from_source, fn_names_from_source,
    trace_id, verify_jettrace, CapturePolicy, JetSymbolRef, SourceIdentity, TraceAllocation,
    TraceIo, TraceLock, TraceSample, TraceSkeleton, TraceTask, TraceToolchain, TRACE_SCHEMA,
    TRACE_IO_ROW_LIMIT, TRACE_TASK_ROW_LIMIT, TRACE_VERSION,
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
    tasks: Vec<TraceTask>,
    locks: Vec<TraceLock>,
    io: Vec<TraceIo>,
    io_rows_truncated: bool,
    source_identity: Vec<SourceIdentity>,
    task_rows_truncated: bool,
}

impl CaptureBundle {
    fn empty() -> Self {
        Self {
            samples: Vec::new(),
            allocations: Vec::new(),
            tasks: Vec::new(),
            locks: Vec::new(),
            io: Vec::new(),
            io_rows_truncated: false,
            source_identity: Vec::new(),
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
    let mut io_timeline = IoTimeline::default();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if parsed.source.is_some() {
                    // Observe publishes under the program PID, not the jet host.
                    if let Some(snapshot) = poll_observe_snapshot(pid) {
                        io_timeline.observe(&snapshot.tasks, elapsed_ns(started));
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
            None,
            Some(&io_timeline),
        ) {
            Ok(bundle) => bundle,
            // Missing/unreadable source still gets a schema-valid skeleton.
            Err(_) => CaptureBundle::empty(),
        },
        None => CaptureBundle::empty(),
    };
    let mut argv = vec![action.to_string()];
    argv.extend(args.iter().cloned());
    match write_session_trace(action, &argv, parsed.out.as_deref(), capture) {
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
    /// Base-intent argv after the action name (perf-only flags stripped).
    child_args: Vec<String>,
}

fn parse_session_args(args: &[String]) -> Result<SessionArgs, String> {
    let mut out = None;
    let mut source = None;
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
        child_args,
    })
}

fn attach(args: &[String]) -> i32 {
    let mut pid = None;
    let mut out = None;
    let mut source = None;
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

    let capture = match (source.as_deref(), jet::DevServer::LiveInspect::read(pid)) {
        (Some(path), Ok(snapshot)) => {
            let (wall_ns, cpu_ns) = process_times(pid).unwrap_or((0, 0));
            match capture_from_source(path, Some(&snapshot), wall_ns, Some(cpu_ns), None) {
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
    };

    let mut argv = vec!["attach".into(), pid.to_string()];
    if let Some(path) = &source {
        argv.push("--source".into());
        argv.push(path.clone());
    }
    match write_session_trace("attach", &argv, out.as_deref(), capture) {
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

fn capture_from_source(
    source_path: &str,
    snapshot: Option<&str>,
    wall_ns: u64,
    cpu_ns: Option<u64>,
    io_timeline: Option<&IoTimeline>,
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
    let snapshot_tasks = snapshot.map(observe_tasks).unwrap_or_default();
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
    let mut tasks = Vec::new();
    if let Some(symbol) = symbol.as_ref() {
        let observed = io_timeline
            .map(IoTimeline::tasks)
            .unwrap_or_else(|| snapshot_tasks.rows.clone());
        for observed in observed {
            tasks.push(TraceTask {
                id: observed.id,
                parent: observed.parent,
                state: observed.state,
                wait: observed.wait,
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
                        task_id: io.task_id,
                        wait: io.wait,
                    })
                    .collect()
            });
        for observed in observed {
            io.push(TraceIo {
                end_ns: observed.end_ns,
                kind: observed.kind,
                start_ns: observed.start_ns,
                task_id: observed.task_id,
                wait: observed.wait,
                symbol: symbol.clone(),
            });
        }
    }
    Ok(CaptureBundle {
        samples,
        allocations,
        tasks,
        locks,
        io,
        io_rows_truncated: io_timeline
            .map(|timeline| timeline.io_rows_truncated)
            .unwrap_or(snapshot_tasks.truncated),
        source_identity: vec![SourceIdentity {
            path: path_text,
            sha256,
            symbols: fn_names
                .into_iter()
                .map(|name| (name, "fn".into()))
                .collect(),
        }],
        task_rows_truncated: io_timeline
            .map(|timeline| timeline.task_rows_truncated)
            .unwrap_or(snapshot_tasks.truncated),
    })
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
    id: u64,
    parent: u64,
    state: String,
    wait: String,
    cancelled: bool,
}

#[derive(Clone, Default)]
struct ObservedTasks {
    rows: Vec<ObservedTask>,
    truncated: bool,
}

/// Parse at most the artifact policy's task-row cap, in id order.
fn observe_tasks(snapshot: &str) -> ObservedTasks {
    let Some(inner) = json_array_inner(snapshot, "tasks") else {
        return ObservedTasks::default();
    };
    if inner.is_empty() {
        return ObservedTasks::default();
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
            id,
            parent,
            state,
            wait,
            cancelled,
        });
    }
    out.sort_by_key(|task| task.id);
    ObservedTasks {
        rows: out,
        truncated,
    }
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
    task_id: u64,
    wait: String,
}

#[derive(Clone)]
struct ObservedIoSpan {
    end_ns: u64,
    kind: String,
    start_ns: u64,
    task_id: u64,
    wait: String,
}

#[derive(Default)]
struct IoTimeline {
    active: BTreeMap<(u64, String), (String, u64)>,
    completed: Vec<ObservedIoSpan>,
    io_rows_truncated: bool,
    task_rows_truncated: bool,
    tasks: BTreeMap<u64, ObservedTask>,
}

impl IoTimeline {
    fn observe(&mut self, observed_tasks: &ObservedTasks, now_ns: u64) {
        self.task_rows_truncated |= observed_tasks.truncated;
        self.io_rows_truncated |= observed_tasks.truncated;
        let current_ids = observed_tasks
            .rows
            .iter()
            .map(|task| task.id)
            .collect::<BTreeSet<_>>();
        for task in self.tasks.values_mut() {
            if !current_ids.contains(&task.id) {
                task.state = "done".into();
                task.wait.clear();
            }
        }
        for task in &observed_tasks.rows {
            if self.tasks.contains_key(&task.id)
                || self.tasks.len() < TRACE_TASK_ROW_LIMIT as usize
            {
                self.tasks.insert(task.id, task.clone());
            } else {
                self.task_rows_truncated = true;
            }
        }

        let observed_io = observe_io(&observed_tasks.rows);
        let current = observed_io
            .iter()
            .map(|row| (row.task_id, row.wait.clone()))
            .collect::<BTreeSet<_>>();
        let ended = self
            .active
            .keys()
            .filter(|key| !current.contains(*key))
            .cloned()
            .collect::<Vec<_>>();
        for (task_id, wait) in ended {
            if let Some((kind, start_ns)) = self.active.remove(&(task_id, wait.clone())) {
                self.push(ObservedIoSpan {
                    end_ns: now_ns.max(start_ns),
                    kind,
                    start_ns,
                    task_id,
                    wait,
                });
            }
        }
        for row in observed_io {
            let key = (row.task_id, row.wait);
            if !self.tasks.contains_key(&row.task_id) {
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
        for ((task_id, wait), (kind, start_ns)) in active {
            self.push(ObservedIoSpan {
                end_ns: now_ns.max(start_ns),
                kind,
                start_ns,
                task_id,
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
            task_id: task.id,
            wait: task.wait.clone(),
        });
    }
    out.sort_by_key(|row| row.task_id);
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
            let tasks = observe_tasks(&snapshot);
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
        // ponytail: Linux USER_HZ is almost always 100; sysconf later if needed.
        // Parse /proc/uptime as integer ticks — f64 loses precision on long uptime
        // and can make now_ticks < starttime → fabricated-looking zero wall.
        const CLK_TCK: u64 = 100;
        let uptime = fs::read_to_string("/proc/uptime").ok()?;
        let (secs_s, rest) = uptime.split_once('.')?;
        let secs: u64 = secs_s.parse().ok()?;
        let frac_s = rest.split_whitespace().next()?;
        let hundredths: u64 = frac_s
            .chars()
            .take(2)
            .collect::<String>()
            .parse()
            .ok()?;
        let now_ticks = secs.saturating_mul(CLK_TCK).saturating_add(hundredths);
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let fields = stat.rsplit_once(") ")?.1;
        let fields: Vec<&str> = fields.split_whitespace().collect();
        let utime: u64 = fields.get(11)?.parse().ok()?;
        let stime: u64 = fields.get(12)?.parse().ok()?;
        let starttime: u64 = fields.get(19)?.parse().ok()?;
        let wall_ticks = now_ticks.saturating_sub(starttime);
        let wall_ns = wall_ticks.saturating_mul(1_000_000_000 / CLK_TCK);
        let cpu_ns = utime
            .saturating_add(stime)
            .saturating_mul(1_000_000_000 / CLK_TCK);
        Some((wall_ns, cpu_ns))
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = pid;
        None
    }
}

fn view(args: &[String]) -> i32 {
    let Some(path) = args.first() else {
        eprintln!("Error [E2102]: `jet perf view` needs a {ARTIFACT_EXT_TRACE} path");
        return 2;
    };
    if args.len() != 1 {
        eprintln!("Error [E2102]: `jet perf view` takes exactly one trace path");
        return 2;
    }
    match read_verified(path) {
        Ok(trace) => {
            let id = trace_id(&trace).unwrap_or("unknown");
            let command = content_command(&trace).unwrap_or("unknown");
            println!("schema {TRACE_SCHEMA} v{TRACE_VERSION}");
            println!("trace {id}");
            println!("command {command}");
            if let Some((domain, ns, symbol)) = first_sample(&trace) {
                println!("sample {domain} {ns}ns · {symbol}");
            }
            if let Some((count, bytes, symbol)) = first_allocation(&trace) {
                println!("alloc count={count} bytes={bytes} · {symbol}");
            }
            if let Some((n, child_n, symbol)) = task_summary(&trace) {
                println!("tasks count={n} children={child_n} · {symbol}");
            }
            if let Some((n, waiters, symbol)) = lock_summary(&trace) {
                println!("locks count={n} waiters={waiters} · {symbol}");
            }
            if let Some((n, symbol)) = io_summary(&trace) {
                println!("io count={n} · {symbol}");
            }
            0
        }
        Err(message) => {
            eprintln!("Error [E2102]: {message}");
            2
        }
    }
}

fn compare(args: &[String]) -> i32 {
    if args.len() != 2 {
        eprintln!("Error [E2102]: `jet perf compare` needs two {ARTIFACT_EXT_TRACE} paths");
        eprintln!(" Fix: jet perf compare base{ARTIFACT_EXT_TRACE} head{ARTIFACT_EXT_TRACE}");
        return 2;
    }
    let base = match read_verified(&args[0]) {
        Ok(trace) => trace,
        Err(message) => {
            eprintln!("Error [E2102]: base trace: {message}");
            return 2;
        }
    };
    let head = match read_verified(&args[1]) {
        Ok(trace) => trace,
        Err(message) => {
            eprintln!("Error [E2102]: head trace: {message}");
            return 2;
        }
    };
    let base_tool = toolchain_digest(&base);
    let head_tool = toolchain_digest(&head);
    if base_tool != head_tool {
        eprintln!("Error [E2102]: toolchain identity mismatch between traces");
        eprintln!(" Why: compare requires matching toolchain digests (D-PERFSESSION1)");
        eprintln!(" Fix: recapture both traces with the same jet toolchain, or wait for an explicit override");
        return 1;
    }
    println!(
        "compare ok · schema {TRACE_SCHEMA} v{TRACE_VERSION} · base {} · head {}",
        trace_id(&base).unwrap_or("unknown"),
        trace_id(&head).unwrap_or("unknown")
    );
    0
}

fn export(args: &[String]) -> i32 {
    let mut path = None;
    let mut i = 0usize;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--json" {
            i += 1;
            continue;
        }
        if arg.starts_with('-') {
            eprintln!("Error [E2102]: unknown `jet perf export` flag `{arg}`");
            eprintln!(" Fix: jet perf export <path{ARTIFACT_EXT_TRACE}> [--json]");
            return 2;
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
    let loss = if first_sample(&trace).is_some()
        || first_allocation(&trace).is_some()
        || task_summary(&trace).is_some()
        || lock_summary(&trace).is_some()
        || io_summary(&trace).is_some()
    {
        "json-envelope; domains present are wall/cpu/alloc/tasks/locks/io only — no pprof/otel/chrome payloads"
    } else {
        "json-envelope-only; no pprof/otel/chrome payloads in skeleton"
    };
    let projection = CanonicalJson::object([
        ("kind".into(), CanonicalJson::String("jet.trace.projection".into())),
        ("loss".into(), CanonicalJson::String(loss.into())),
        ("schema".into(), CanonicalJson::String(TRACE_SCHEMA.into())),
        ("trace".into(), trace),
        ("version".into(), CanonicalJson::Integer(TRACE_VERSION.into())),
    ])
    .expect("projection keys are unique");
    print!("{}", String::from_utf8_lossy(&projection.bytes()));
    0
}

fn write_session_trace(
    command: &str,
    argv: &[String],
    out: Option<&str>,
    capture: CaptureBundle,
) -> Result<PathBuf, String> {
    let mut capture_policy = CapturePolicy::default_exclusions();
    capture_policy.io_rows_truncated = capture.io_rows_truncated;
    capture_policy.task_rows_truncated = capture.task_rows_truncated;
    let skeleton = TraceSkeleton {
        command: command.into(),
        argv: argv.to_vec(),
        toolchain: current_toolchain(),
        capture_policy,
        samples: capture.samples,
        allocations: capture.allocations,
        tasks: capture.tasks,
        locks: capture.locks,
        io: capture.io,
        source_identity: capture.source_identity,
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

fn content_array<'a>(trace: &'a CanonicalJson, key: &str) -> Option<&'a [CanonicalJson]> {
    let CanonicalJson::Object(fields) = trace else { return None };
    let CanonicalJson::Object(content) = fields.get("content")? else { return None };
    match content.get(key)? {
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
        let observed = observe_tasks(&snapshot);
        assert_eq!(observed.rows.len(), TRACE_TASK_ROW_LIMIT as usize);
        assert!(observed.truncated);

        let mut timeline = IoTimeline::default();
        timeline.observe(&observed, 10);
        timeline.finish(20);
        assert_eq!(timeline.tasks.len(), TRACE_TASK_ROW_LIMIT as usize);
        assert_eq!(timeline.completed.len(), TRACE_IO_ROW_LIMIT as usize);
        assert!(timeline.task_rows_truncated);
        assert!(timeline.io_rows_truncated);
    }
}
