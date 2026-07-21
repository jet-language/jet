//! D-PERFSESSION1=D: `jet perf` family over one versioned `.jettrace` truth.
//!
//! `run`/`test`/`bench` write a schema-identified skeleton then return control
//! so main can strip `perf` and execute the exact base-intent driver path.
//! `attach`/`view`/`compare`/`export` share the same artifact verify seam.

use jet_foundation::JetTrace::{
    artifact_extension, build_skeleton_bytes, trace_id, verify_jettrace, CapturePolicy,
    TraceSkeleton, TraceToolchain, TRACE_SCHEMA, TRACE_VERSION,
};
use jet_foundation::PerformanceBudget::CanonicalJson;
use jet_foundation::Syntax::ARTIFACT_EXT_TRACE;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const USAGE: &str = "usage: jet perf <run|test|bench|attach|view|compare|export> …";

pub(crate) enum Outcome {
    /// Skeleton written; caller strips `perf` and continues as the base intent.
    ForwardBase,
    Exit(i32),
}

pub(crate) fn run(raw: &[String]) -> Outcome {
    let Some(action) = raw.get(1).map(String::as_str) else {
        eprintln!("Error [E2102]: `jet perf` needs a subcommand");
        eprintln!(" Fix: {USAGE}");
        return Outcome::Exit(2);
    };
    match action {
        "run" | "test" | "bench" => match write_session_skeleton(action, &raw[1..], None) {
            Ok(path) => {
                eprintln!("trace: {}", path.display());
                Outcome::ForwardBase
            }
            Err(message) => {
                eprintln!("Error [E2102]: {message}");
                eprintln!(" Fix: {USAGE}");
                Outcome::Exit(2)
            }
        },
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

fn attach(args: &[String]) -> i32 {
    let mut pid = None;
    let mut out = None;
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
        eprintln!(" Fix: jet perf attach <pid>");
        return 2;
    };
    // Same-user observe surface: refuse foreign/missing pids with a Jet message.
    if !process_exists(pid) {
        eprintln!("Error [E2102]: process {pid} is not running or not visible to this user");
        eprintln!(" Fix: start the program with `jet run --observe`, then attach to that pid");
        return 2;
    }
    let argv = vec!["attach".into(), pid.to_string()];
    match write_session_skeleton("attach", &argv, out.as_deref()) {
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
            // Projection is always JSON today; flag reserved for future formats.
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
    // Loss-declared JSON projection of the one artifact truth.
    let projection = CanonicalJson::object([
        ("kind".into(), CanonicalJson::String("jet.trace.projection".into())),
        (
            "loss".into(),
            CanonicalJson::String(
                "json-envelope-only; no pprof/otel/chrome payloads in skeleton".into(),
            ),
        ),
        ("schema".into(), CanonicalJson::String(TRACE_SCHEMA.into())),
        ("trace".into(), trace),
        ("version".into(), CanonicalJson::Integer(TRACE_VERSION.into())),
    ])
    .expect("projection keys are unique");
    print!("{}", String::from_utf8_lossy(&projection.bytes()));
    0
}

fn write_session_skeleton(command: &str, argv: &[String], out: Option<&str>) -> Result<PathBuf, String> {
    let skeleton = TraceSkeleton {
        command: command.into(),
        argv: argv.to_vec(),
        toolchain: current_toolchain(),
        capture_policy: CapturePolicy::default_exclusions(),
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
    // Prove the on-disk bytes are the verified artifact, not a placeholder.
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
    // Compact UTC-ish stamp without pulling chrono: YYYYMMDDhhmmss from epoch is
    // not calendar-true; use epoch seconds for uniqueness and keep human prefix.
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
