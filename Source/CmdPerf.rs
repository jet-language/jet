//! D-PERFSESSION1=D: `jet perf` family over one versioned `.jettrace` truth.
//!
//! `run`/`test`/`bench` spawn the exact base-intent driver (`jet run|test|bench …`)
//! with observe enabled, poll live facts while the child runs, then write one
//! `.jettrace` with wall/alloc samples before exiting with the child's code.
//! `attach`/`view`/`compare`/`export` share the same artifact verify seam.
//! Capture reuses the observe live snapshot (D-OBSERVE-LIVE1) attributed to a
//! Jet source symbol.

use jet_foundation::JetTrace::{
    artifact_extension, build_skeleton_bytes, trace_id, verify_jettrace, CapturePolicy,
    JetSymbolRef, SourceIdentity, TraceAllocation, TraceSample, TraceSkeleton, TraceToolchain,
    TRACE_SCHEMA, TRACE_VERSION,
};
use jet_foundation::PerformanceBudget::CanonicalJson;
use jet_foundation::SHA256;
use jet_foundation::Syntax::ARTIFACT_EXT_TRACE;
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
    source_identity: Vec<SourceIdentity>,
}

impl CaptureBundle {
    fn empty() -> Self {
        Self {
            samples: Vec::new(),
            allocations: Vec::new(),
            source_identity: Vec::new(),
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
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if parsed.source.is_some() {
                    // Observe publishes under the program PID, not the jet host.
                    if let Some(snapshot) = poll_observe_snapshot(pid) {
                        last_snapshot = Some(snapshot);
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
    let capture = match &parsed.source {
        Some(source) => match capture_from_source(
            source,
            last_snapshot.as_deref(),
            wall_ns.max(1),
            None,
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
            match capture_from_source(path, Some(&snapshot), wall_ns.max(1), Some(cpu_ns)) {
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
) -> Result<CaptureBundle, String> {
    let path = PathBuf::from(source_path);
    if path.extension().and_then(|e| e.to_str()) != Some("jet") {
        return Err(format!("source path must be a `.jet` file, got `{source_path}`"));
    }
    let bytes = fs::read(&path).map_err(|e| format!("cannot read source {source_path}: {e}"))?;
    let sha256 = SHA256::sha256_hex(&bytes);
    let path_text = source_path.to_string();
    let symbol = JetSymbolRef {
        path: path_text.clone(),
        name: "run".into(),
    };
    let mut samples = Vec::new();
    samples.push(TraceSample {
        domain: "wall".into(),
        duration_ns: wall_ns.max(1),
        symbol: symbol.clone(),
    });
    if let Some(cpu_ns) = cpu_ns.filter(|ns| *ns > 0) {
        samples.push(TraceSample {
            domain: "cpu".into(),
            duration_ns: cpu_ns,
            symbol: symbol.clone(),
        });
    }
    let mut allocations = Vec::new();
    if let Some(snapshot) = snapshot {
        let (alloc_count, alloc_bytes) = observe_arena_resources(snapshot);
        allocations.push(TraceAllocation {
            count: alloc_count,
            bytes: alloc_bytes,
            symbol: symbol.clone(),
        });
    }
    Ok(CaptureBundle {
        samples,
        allocations,
        source_identity: vec![SourceIdentity {
            path: path_text,
            sha256,
            symbols: vec![("run".into(), "fn".into())],
        }],
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

/// `jet run` hosts compile then exec; observe publishes under the program PID.
fn poll_observe_snapshot(root_pid: u32) -> Option<String> {
    let mut stack = vec![root_pid];
    let mut seen = std::collections::BTreeSet::new();
    let mut best = None;
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        if let Ok(snapshot) = jet::DevServer::LiveInspect::read(pid) {
            if snapshot.contains("\"arena_allocations\":") {
                return Some(snapshot);
            }
            best = Some(snapshot);
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
        const CLK_TCK: u64 = 100;
        let uptime = fs::read_to_string("/proc/uptime").ok()?;
        let uptime_secs: f64 = uptime.split_whitespace().next()?.parse().ok()?;
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let fields = stat.rsplit_once(") ")?.1;
        let fields: Vec<&str> = fields.split_whitespace().collect();
        let utime: u64 = fields.get(11)?.parse().ok()?;
        let stime: u64 = fields.get(12)?.parse().ok()?;
        let starttime: u64 = fields.get(19)?.parse().ok()?;
        let now_ticks = (uptime_secs * CLK_TCK as f64) as u64;
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
    let loss = if first_sample(&trace).is_some() || first_allocation(&trace).is_some() {
        "json-envelope; domains present are wall/cpu/alloc only — no pprof/otel/chrome payloads"
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
    let skeleton = TraceSkeleton {
        command: command.into(),
        argv: argv.to_vec(),
        toolchain: current_toolchain(),
        capture_policy: CapturePolicy::default_exclusions(),
        samples: capture.samples,
        allocations: capture.allocations,
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
