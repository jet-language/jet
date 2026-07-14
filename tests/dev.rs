//! E2-M4 — `jet dev` interpreter tests.
//!
//! The crux is the **differential battery** (D-DEV, I2): for each supported
//! program, the interpreter's stdout/stderr/exit code MUST be byte-for-byte
//! identical to the compiled native binary. Any divergence is a P0 miscompile-class
//! bug — the interpreter is a dev convenience that must never lie about what
//! the real build does. This mirrors `tests/comptime_diff.rs`.
//!
//! Also tested:
//!   - the E2201 honest-boundary note (tasks/FFI/`#Unsafe`/native std),
//!   - the per-iteration `dev_iteration` function the watch loop is built on,
//!   - the save-to-diagnostic latency budget (D-DEV3, <200ms check-only).

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

mod common;
use common::{have_rustc, panic_message, test_worker_count, FfiBridgeLock};
use jet::Interpreter::{dev_iteration, run_named_task, RunOutcome};
use jet::JitBackend::{InterpreterBackend, JitBackend};
use jet_jit::CraneliftBackend;

const DEV_DIFF_TIMEOUT: Duration = Duration::from_secs(30);

fn dev_diff_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn skip_if_cranelift_host_unsupported() -> bool {
    if jet_jit::cranelift_host_supported() {
        false
    } else {
        eprintln!("note: cranelift-jit host path unsupported on this architecture; skipping resident JIT assertion");
        true
    }
}

/// c77: the differential battery covers EVERY `examples/features/*.jet`, not a
/// hand-curated subset — so the battery can never quietly shrink. Each example
/// either runs in the interpreter (and its stdout/stderr/exit code must match
/// the compiled binary; stdout also matches its golden `.out`) or stops at a named boundary
/// (E2201 pre-scan, E2202 fuel, E0956 unsupported-at-runtime). A silent skip is
/// a test failure.
fn example_path(stem: &str) -> String {
    format!("examples/features/{}.jet", stem)
}

fn host_expected_stdout(stem: &str) -> Option<&'static str> {
    match stem {
        "lowlevel/os_target_gating" if cfg!(target_os = "macos") => Some("macos: appkit\n"),
        "lowlevel/os_target_gating" if cfg!(target_os = "windows") => Some("windows: win32\n"),
        _ => None,
    }
}

fn uses_ffi_bridge(stem: &str) -> bool {
    matches!(
        stem,
        "lowlevel/ffi"
            | "io/archive"
            | "io/db"
            | "crypto/crypto_envelope"
            | "crypto/crypto_sign"
            | "crypto/crypto_migration"
            | "crypto/crypto_suite"
            | "crypto/vault_secret"
            | "io/compress_gzip"
            | "io/compress_zstd"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProgramOutput {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

impl ProgramOutput {
    fn ran(stdout: String, stderr: String, exit_code: i32) -> Self {
        Self {
            stdout,
            stderr,
            exit_code,
        }
    }
}

fn compiled_binary_output(
    dir: &std::path::Path,
    tag: &str,
    i: usize,
    stem: &str,
    file: &str,
) -> ProgramOutput {
    compiled_binary_output_with_stdin(dir, tag, i, stem, file, None)
}

fn compiled_binary_output_with_stdin(
    dir: &std::path::Path,
    tag: &str,
    i: usize,
    stem: &str,
    file: &str,
    stdin: Option<&std::path::Path>,
) -> ProgramOutput {
    let src = fs::read_to_string(file).unwrap();
    let compiled = match jet::compile_with_path(&src, file) {
        Ok(c) => c,
        Err(diags) => panic!(
            "`{}` ran in dev but failed the front end:\n{}",
            stem,
            jet::render_diagnostics(file, &src, &diags)
        ),
    };
    let rs = dir.join(format!("jet_{tag}_{}.rs", i));
    let bin = dir.join(format!("jet_{tag}_{}", i));
    fs::create_dir_all(dir).unwrap();
    fs::write(&rs, &compiled.rust).unwrap();
    let mut rustc_cmd = Command::new("rustc");
    rustc_cmd
        .args(["--edition", "2021"])
        // Match default `jet run` optimization. Parity tests compare product
        // behavior, including cfg(debug_assertions)-gated stderr.
        .arg("-O")
        .arg(&rs)
        .arg("-o")
        .arg(&bin);
    if let Some(link) = &compiled.ffi {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        if link.deps_dir.is_dir() {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", link.deps_dir.display()));
        }
    }
    let clinks = jet::resolve_c_links(file).unwrap_or_else(|diags| {
        panic!(
            "`{}` ran in dev but AOT C-link resolution failed:\n{}",
            stem,
            jet::render_diagnostics(file, &src, &diags)
        )
    });
    for arg in clinks {
        rustc_cmd.arg(arg);
    }
    let out = command_output_with_timeout(
        rustc_cmd,
        DEV_DIFF_TIMEOUT,
        &format!("rustc build for `{stem}`"),
    );
    if !out.status.success() {
        panic!(
            "`{}` ran in dev but generated Rust failed to build (status: {}):\nstdout:\n{}\nstderr:\n{}",
            stem,
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let mut run_cmd = Command::new(&bin);
    if let Some(path) = stdin {
        run_cmd.stdin(fs::File::open(path).unwrap());
    }
    let run = command_output_with_timeout(
        run_cmd,
        DEV_DIFF_TIMEOUT,
        &format!("compiled binary run for `{stem}`"),
    );
    ProgramOutput::ran(
        String::from_utf8_lossy(&run.stdout).to_string(),
        String::from_utf8_lossy(&run.stderr).to_string(),
        run.status.code().unwrap_or(1),
    )
}

fn command_output_with_timeout(mut cmd: Command, timeout: Duration, label: &str) -> Output {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base =
        std::env::temp_dir().join(format!("jet_dev_timeout_{}_{}", std::process::id(), stamp));
    let stdout_path = base.with_extension("stdout");
    let stderr_path = base.with_extension("stderr");
    let stdout_file = fs::File::create(&stdout_path)
        .unwrap_or_else(|e| panic!("{label}: failed to create stdout temp file: {e}"));
    let stderr_file = fs::File::create(&stderr_path)
        .unwrap_or_else(|e| panic!("{label}: failed to create stderr temp file: {e}"));
    let mut child = cmd
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .unwrap_or_else(|e| panic!("{label}: failed to spawn: {e}"));
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let status = child
                    .wait()
                    .unwrap_or_else(|e| panic!("{label}: failed to collect status: {e}"));
                let stdout = fs::read(&stdout_path).unwrap_or_default();
                let stderr = fs::read(&stderr_path).unwrap_or_default();
                let _ = fs::remove_file(&stdout_path);
                let _ = fs::remove_file(&stderr_path);
                return Output {
                    status,
                    stdout,
                    stderr,
                };
            }
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let stdout = fs::read(&stdout_path).unwrap_or_default();
                let stderr = fs::read(&stderr_path).unwrap_or_default();
                let _ = fs::remove_file(&stdout_path);
                let _ = fs::remove_file(&stderr_path);
                panic!(
                    "{label}: timed out after {:?}\nstdout:\n{}\nstderr:\n{}",
                    timeout,
                    String::from_utf8_lossy(&stdout),
                    String::from_utf8_lossy(&stderr)
                );
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => panic!("{label}: failed to poll child: {e}"),
        }
    }
}

fn dev_iteration_with_timeout(stem: &str, file: &str, use_interpreter: bool) -> RunOutcome {
    let stem = stem.to_string();
    let file = file.to_string();
    let worker_file = file.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let out = dev_iteration(&worker_file, false, use_interpreter);
        let _ = tx.send(out);
    });
    rx.recv_timeout(DEV_DIFF_TIMEOUT).unwrap_or_else(|_| {
        panic!(
            "dev_iteration timed out after {:?} for `{}` ({}) with use_interpreter={}",
            DEV_DIFF_TIMEOUT, stem, file, use_interpreter
        )
    })
}

/// All `.jet` files directly under a topic directory of `examples/features/`
/// (one level: `examples/features/<topic>/<name>.jet`). Skips `expected/`
/// and skips project-directory examples (`<topic>/<name>/main.jet`) — those
/// have their own multi-file drivers and are not single-entry dev targets.
fn topic_jet_files(root: &std::path::Path) -> Vec<PathBuf> {
    let ex_dir = root.join("examples/features");
    let mut files = Vec::new();
    for topic_entry in fs::read_dir(&ex_dir).unwrap().flatten() {
        let topic_path = topic_entry.path();
        if !topic_path.is_dir() {
            continue;
        }
        let topic_name = topic_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if topic_name == "expected" {
            continue;
        }
        for e in fs::read_dir(&topic_path).unwrap().flatten() {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) == Some("jet") {
                files.push(path);
            }
        }
    }
    files
}

/// The "topic/name" stem for a `.jet` file found via `topic_jet_files`.
fn stem_of(root: &std::path::Path, path: &std::path::Path) -> String {
    let ex_dir = root.join("examples/features");
    let rel = path.strip_prefix(&ex_dir).unwrap().with_extension("");
    rel.to_string_lossy().replace('\\', "/")
}

/// Every top-level example stem under `examples/features/<topic>/`, sorted
/// for determinism. (Subdirectory examples — imports, modules, packages —
/// have their own multi-file drivers and are not single-entry dev targets.)
fn all_example_stems() -> Vec<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut stems: Vec<String> = topic_jet_files(&root)
        .iter()
        .map(|p| stem_of(&root, p))
        .collect();
    stems.sort();
    stems
}

fn collect_jit_coverage() -> (Vec<String>, Vec<String>) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut covered = Vec::new();
    let mut gaps = Vec::new();
    for path in topic_jet_files(&root) {
        let file = path.to_string_lossy();
        let mut bundle = match jet::Loader::load_entry(&file) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
        let errors: Vec<_> = diags
            .into_iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .collect();
        if !errors.is_empty() {
            continue;
        }
        let stem = stem_of(&root, &path);
        match jet_jit::try_compile_bundle(&bundle) {
            Ok(()) => covered.push(stem),
            Err(reason) => gaps.push(format!("{stem}: {reason}")),
        }
    }
    covered.sort();
    gaps.sort();
    (covered, gaps)
}

fn parse_jit_gap_manifest() -> (Vec<String>, Vec<String>, Vec<String>) {
    enum Section {
        None,
        Covered,
        Gaps,
        ParityDivergences,
    }

    let mut section = Section::None;
    let mut covered = Vec::new();
    let mut gaps = Vec::new();
    let mut parity_divergences = Vec::new();
    for raw in include_str!("jit_gaps.txt").lines() {
        let line = raw.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        match trimmed {
            "covered:" => {
                section = Section::Covered;
                continue;
            }
            "gaps:" => {
                section = Section::Gaps;
                continue;
            }
            "parity_divergences:" => {
                section = Section::ParityDivergences;
                continue;
            }
            _ => {}
        }
        match section {
            Section::Covered => covered.push(trimmed.to_string()),
            Section::Gaps => gaps.push(trimmed.to_string()),
            Section::ParityDivergences => parity_divergences.push(trimmed.to_string()),
            Section::None => panic!("manifest entry outside a section: {trimmed}"),
        }
    }
    covered.sort();
    gaps.sort();
    parity_divergences.sort();
    assert!(
        parity_divergences.is_empty(),
        "parity_divergences must stay empty: default dev and interpreter output must exactly match default AOT output; fix the shared semantics or use transparent fallback"
    );
    (covered, gaps, parity_divergences)
}

fn is_manifested_parity_divergence(stem: &str, entries: &[String]) -> bool {
    entries.iter().any(|entry| {
        entry
            .strip_prefix(stem)
            .is_some_and(|rest| rest.starts_with(':'))
    })
}

fn normalize_for_parity(stem: &str, mut out: ProgramOutput) -> ProgramOutput {
    if matches!(stem, "io/log" | "io/log_structured" | "serde/json_coerce") {
        out.stderr = normalize_json_log_timestamps(&out.stderr);
    }
    if stem == "io/log_human" {
        out.stderr = normalize_text_log_timestamps(&out.stderr);
    }
    if stem == "ui/ui_native_linux" {
        out.stderr = normalize_gtk_loader_path(&out.stderr);
    }
    out
}

fn normalize_gtk_loader_path(s: &str) -> String {
    let marker = ": symbol lookup error: ";
    if !s.contains("libgtk-4.so.1") || !s.contains("undefined symbol") {
        return s.to_string();
    }
    if let Some((_, rest)) = s.split_once(marker) {
        format!("<jet-ui-binary>{marker}{rest}")
    } else {
        s.to_string()
    }
}

fn normalize_json_log_timestamps(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find("\"ts\":") {
        out.push_str(&rest[..pos]);
        out.push_str("\"ts\":<ts>");
        let mut tail = &rest[pos + "\"ts\":".len()..];
        let digits = tail.bytes().take_while(|b| b.is_ascii_digit()).count();
        tail = &tail[digits..];
        rest = tail;
    }
    out.push_str(rest);
    out
}

fn normalize_text_log_timestamps(s: &str) -> String {
    s.lines()
        .map(|line| {
            let Some((level, rest)) = line.split_once(' ') else {
                return line.to_string();
            };
            let Some((_, msg)) = rest.split_once(" | ") else {
                return line.to_string();
            };
            format!("{level} <ts> | {msg}")
        })
        .collect::<Vec<_>>()
        .join("\n")
        + if s.ends_with('\n') { "\n" } else { "" }
}

fn print_jit_op_report() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut ops: std::collections::BTreeMap<String, (usize, usize)> =
        std::collections::BTreeMap::new();
    for path in topic_jet_files(&root) {
        let file = path.to_string_lossy();
        let mut bundle = match jet::Loader::load_entry(&file) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
        if diags
            .iter()
            .any(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        {
            continue;
        }
        let covered = jet_jit::try_compile_bundle(&bundle).is_ok();
        for tag in jet_jit::jit_dump_main_ops(&bundle) {
            let entry = ops.entry(tag).or_default();
            if covered {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }
        }
    }
    eprintln!("jit observed TIR op compile coverage:");
    for (op, (covered, gaps)) in ops {
        eprintln!("  {op}: compile_covered_examples={covered} gap_examples={gaps}");
    }
}

/// The recognized honest-boundary / terminal codes the interpreter may stop at
/// instead of producing run-to-completion stdout (c77 / D-DEV1):
///   - E2201: a feature the dev interpreter doesn't cover (pre-scan boundary),
///   - E2202 / E0952: the step/fuel budget was exhausted,
///   - E0956: a construct not yet supported at comptime (hit during execution),
///   - E0953: a deliberate user-authored panic (`require(false, …)`), which is
///     the program legitimately failing, not a silent skip.
///   - E3410 / E3411: a D-CTEFFECT1 Tier-2 comptime effect (`core.files`/`core.io`/
///     `core.env`/…) reached with no `#Impure` gate, or a gate present but
///     `--allow-impure` not passed — an honest, named boundary (the golden
///     corpus runs with neither), not a silent skip.
///   - E1265 (U13, D-JPK-SECRETCRYPTO1): `core.vault.get` reached through the
///     same comptime/interpreter evaluation path — unconditionally denied
///     (no `#Impure` escape hatch), so an example exercising it always stops
///     here under the interpreter/JIT tiers even though the AOT-compiled
///     binary runs it fine (it never goes through this evaluator).
const BOUNDARY_CODES: &[&str] = &[
    "E2201", "E2202", "E0952", "E0956", "E0953", "E3410", "E3411", "E1265",
];

// The in-process harness cannot close or inject its own ambient stdin without
// changing product behavior. `stdin_filter` gets a real piped-stdin CLI parity
// test below; keeping it here as an exact boundary prevents a hanging battery.
const DEFAULT_BACKEND_EXPECTED_BOUNDARIES: &[&str] =
    &["io/stdin_filter", "ui/ui_native_linux"];

#[derive(Default, Clone)]
struct DevBatteryStats {
    ran: usize,
    boundary: usize,
    manifested: usize,
    boundary_stems: Vec<String>,
}

impl DevBatteryStats {
    fn add(&mut self, mut other: DevBatteryStats) {
        self.ran += other.ran;
        self.boundary += other.boundary;
        self.manifested += other.manifested;
        self.boundary_stems.append(&mut other.boundary_stems);
    }
}

fn is_named_dev_boundary(stem: &str, diags: &[jet::Diagnostics::Diagnostic]) -> bool {
    diags.iter().any(|d| BOUNDARY_CODES.contains(&d.code.as_str()))
        // D-DBDRIVER1/D-DBMIGRATE1: the checked-SQL DB example is an AOT-backed
        // core.db surface today; the dev default tier stops before execution.
        || (stem == "io/db_checked_sql" && diags.iter().all(|d| d.code == "E1004"))
}

fn check_dev_default_stem(
    i: usize,
    stem: &str,
    dir: &std::path::Path,
    manifested_divergences: &[String],
) -> DevBatteryStats {
    let _ffi_lock = uses_ffi_bridge(stem).then(FfiBridgeLock::acquire);
    let file = example_path(stem);
    eprintln!("dev-default checking {stem}");
    let outcome = dev_iteration_with_timeout(stem, &file, false);
    let interpreted = match outcome {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => {
            eprintln!(
                "default boundary: {stem}: {}",
                diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>().join(",")
            );
            assert!(
                is_named_dev_boundary(stem, &diags),
                "`{}` neither ran nor stopped at a named boundary {:?} under the default \
                 jet dev backend; codes were {:?}",
                stem,
                BOUNDARY_CODES,
                diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
            );
            return DevBatteryStats {
                boundary: 1,
                boundary_stems: vec![stem.to_string()],
                ..DevBatteryStats::default()
            };
        }
    };

    let compiled = compiled_binary_output(dir, "dev_default_diff", i, stem, &file);
    let interpreted = normalize_for_parity(stem, interpreted);
    let compiled = normalize_for_parity(stem, compiled);
    if interpreted != compiled {
        if is_manifested_parity_divergence(stem, manifested_divergences) {
            eprintln!("manifested parity divergence: {stem}");
            return DevBatteryStats {
                ran: 1,
                manifested: 1,
                ..DevBatteryStats::default()
            };
        }
        assert_eq!(
            interpreted, compiled,
            "DIVERGENCE for `{}` under the default jet dev backend: JIT/fallback and compiled \
             binary disagree on stdout/stderr/exit code — this is a P0 miscompile",
            stem
        );
    }
    DevBatteryStats {
        ran: 1,
        ..DevBatteryStats::default()
    }
}

fn run_dev_default_battery_parallel(
    stems: Vec<String>,
    dir: PathBuf,
    manifested_divergences: Vec<String>,
) -> DevBatteryStats {
    let jobs = Arc::new(Mutex::new(
        stems
            .into_iter()
            .enumerate()
            .collect::<std::collections::VecDeque<_>>(),
    ));
    let dir = Arc::new(dir);
    let manifested_divergences = Arc::new(manifested_divergences);
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));
    let worker_count = test_worker_count(16);
    let mut handles = Vec::new();
    for _ in 0..worker_count {
        let jobs = Arc::clone(&jobs);
        let dir = Arc::clone(&dir);
        let manifested_divergences = Arc::clone(&manifested_divergences);
        let failures = Arc::clone(&failures);
        handles.push(std::thread::spawn(move || {
            let mut stats = DevBatteryStats::default();
            loop {
                let Some((i, stem)) = jobs.lock().unwrap().pop_front() else {
                    break;
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    check_dev_default_stem(i, &stem, &dir, &manifested_divergences)
                }));
                match result {
                    Ok(next) => stats.add(next),
                    Err(payload) => failures
                        .lock()
                        .unwrap()
                        .push(format!("{stem}: {}", panic_message(payload))),
                }
            }
            stats
        }));
    }

    let mut stats = DevBatteryStats::default();
    for handle in handles {
        stats.add(handle.join().expect("dev default worker panicked outside harness"));
    }
    let failures = failures.lock().unwrap();
    assert!(
        failures.is_empty(),
        "dev default parity failures:\n{}",
        failures.join("\n\n")
    );
    stats
}

fn check_interpreter_stem(
    i: usize,
    stem: &str,
    dir: &std::path::Path,
    manifested_divergences: &[String],
) -> DevBatteryStats {
    let _ffi_lock = uses_ffi_bridge(stem).then(FfiBridgeLock::acquire);
    let file = example_path(stem);
    eprintln!("interpreter checking {stem}");
    let interpreted = match dev_iteration_with_timeout(stem, &file, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => {
            assert!(
                is_named_dev_boundary(stem, &diags),
                "`{}` neither ran nor stopped at a named boundary {:?}; codes were {:?}",
                stem,
                BOUNDARY_CODES,
                diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
            );
            return DevBatteryStats {
                boundary: 1,
                ..DevBatteryStats::default()
            };
        }
    };

    let compiled = compiled_binary_output(dir, "dev_diff", i, stem, &file);
    let interpreted = normalize_for_parity(stem, interpreted);
    let compiled = normalize_for_parity(stem, compiled);
    if interpreted != compiled {
        if is_manifested_parity_divergence(stem, manifested_divergences) {
            eprintln!("manifested parity divergence: {stem}");
            return DevBatteryStats {
                ran: 1,
                manifested: 1,
                ..DevBatteryStats::default()
            };
        }
        assert_eq!(
            interpreted, compiled,
            "DIVERGENCE for `{}`: interpreter and compiled binary disagree on stdout/stderr/exit code — this is a P0 miscompile",
            stem
        );
    }
    DevBatteryStats {
        ran: 1,
        ..DevBatteryStats::default()
    }
}

fn run_interpreter_battery_parallel(
    stems: Vec<String>,
    dir: PathBuf,
    manifested_divergences: Vec<String>,
) -> DevBatteryStats {
    let jobs = Arc::new(Mutex::new(
        stems
            .into_iter()
            .enumerate()
            .collect::<std::collections::VecDeque<_>>(),
    ));
    let dir = Arc::new(dir);
    let manifested_divergences = Arc::new(manifested_divergences);
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));
    let worker_count = test_worker_count(16);
    let mut handles = Vec::new();
    for _ in 0..worker_count {
        let jobs = Arc::clone(&jobs);
        let dir = Arc::clone(&dir);
        let manifested_divergences = Arc::clone(&manifested_divergences);
        let failures = Arc::clone(&failures);
        handles.push(std::thread::spawn(move || {
            let mut stats = DevBatteryStats::default();
            loop {
                let Some((i, stem)) = jobs.lock().unwrap().pop_front() else {
                    break;
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    check_interpreter_stem(i, &stem, &dir, &manifested_divergences)
                }));
                match result {
                    Ok(next) => stats.add(next),
                    Err(payload) => failures
                        .lock()
                        .unwrap()
                        .push(format!("{stem}: {}", panic_message(payload))),
                }
            }
            stats
        }));
    }

    let mut stats = DevBatteryStats::default();
    for handle in handles {
        stats.add(handle.join().expect("interpreter worker panicked outside harness"));
    }
    let failures = failures.lock().unwrap();
    assert!(
        failures.is_empty(),
        "interpreter parity failures:\n{}",
        failures.join("\n\n")
    );
    stats
}

/// c77 widened battery: EVERY example either runs (interpreted stdout/stderr/exit
/// code == compiled-binary stdout/stderr/exit code, byte for byte — I2) or stops at a named boundary
/// (E2201/E2202/E0956 — never a silent skip). Reports the run/boundary split so
/// the coverage can't quietly shrink.
#[test]
fn interpreter_matches_compiled_binary() {
    let _guard = dev_diff_lock().lock().unwrap();
    let have_rustc = have_rustc();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping jet dev differential battery");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_dev_diff_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let (_, _, manifested_divergences) = parse_jit_gap_manifest();
    let stats = run_interpreter_battery_parallel(all_example_stems(), dir, manifested_divergences);
    eprintln!(
        "c77 battery: {} ran ({} interp==compiled, {} manifested divergences), {} boundary-asserted, {} total",
        stats.ran,
        stats.ran - stats.manifested,
        stats.manifested,
        stats.boundary,
        stats.ran + stats.boundary
    );
    assert!(
        stats.ran > 0,
        "expected at least some examples to run in the interpreter"
    );
}

/// c125 M4 exit gate: the DEFAULT `jet dev` path (Cranelift JIT with
/// per-construct interpreter fallback — `use_interpreter: false`, what the
/// `jet dev` CLI actually runs) must be functionally identical to the AOT
/// compiled binary for every example, same as `interpreter_matches_compiled_binary`
/// above but exercising the real default backend instead of the forced
/// interpreter-only tier. This is the owner's "JIT and `jet run` must be
/// functionally identical on every program the AOT path covers" requirement
/// (2026-06-30) — distinct from the interpreter-only invariant above, which
/// stays green even when JIT coverage regresses because it never touches
/// Cranelift. A stray boundary here (that the interpreter-only test doesn't
/// also hit) means the JIT fallback dropped correctness the plain interpreter
/// had — a P0 regression, not a coverage gap.
#[test]
fn dev_default_matches_compiled_binary() {
    let _guard = dev_diff_lock().lock().unwrap();
    let have_rustc = have_rustc();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping jet dev (default backend) differential battery");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_dev_default_diff_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let (_, _, manifested_divergences) = parse_jit_gap_manifest();
    let stats = run_dev_default_battery_parallel(all_example_stems(), dir, manifested_divergences);
    eprintln!(
        "c125 default-backend battery: {} ran ({} default==compiled, {} manifested divergences), {} boundary-asserted, {} total",
        stats.ran,
        stats.ran - stats.manifested,
        stats.manifested,
        stats.boundary,
        stats.ran + stats.boundary
    );
    assert!(
        stats.ran > 0,
        "expected at least some examples to run via the default jet dev backend"
    );
    let mut observed_boundaries = stats.boundary_stems;
    observed_boundaries.sort();
    assert_eq!(
        observed_boundaries,
        DEFAULT_BACKEND_EXPECTED_BOUNDARIES,
        "default jet dev boundary set must stay exact and every non-manifested boundary must execute"
    );
    assert_eq!(
        stats.manifested, 0,
        "default jet dev must not carry manifested stdout/stderr/exit-code divergences"
    );
}

#[test]
fn stdin_filter_cli_uses_transparent_aot_fallback() {
    let dir = std::env::temp_dir().join(format!(
        "jet_dev_stdin_boundary_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("stdin.txt");
    fs::write(&input, "jet one\nnope\njet two\n").unwrap();
    let file = example_path("io/stdin_filter");
    let compiled = compiled_binary_output_with_stdin(
        &dir,
        "stdin_filter_aot",
        0,
        "io/stdin_filter",
        &file,
        Some(&input),
    );

    let mut dev_cmd = Command::new(env!("CARGO_BIN_EXE_jet"));
    dev_cmd
        .args(["dev", &file, "--watch=off"])
        .stdin(fs::File::open(&input).unwrap());
    let dev = command_output_with_timeout(dev_cmd, DEV_DIFF_TIMEOUT, "piped-input jet dev");
    let dev_stdout = String::from_utf8_lossy(&dev.stdout);
    let (runtime_stdout, status) = dev_stdout
        .rsplit_once("✓ ran in ")
        .expect("one-shot jet dev must print its completion status");
    assert!(
        status
            .strip_suffix(" ms\n")
            .is_some_and(|millis| !millis.is_empty() && millis.bytes().all(|b| b.is_ascii_digit())),
        "unexpected one-shot jet dev completion status: {status:?}"
    );
    assert_eq!(runtime_stdout, compiled.stdout);
    assert_eq!(String::from_utf8_lossy(&dev.stderr), compiled.stderr);
    assert_eq!(dev.status.code().unwrap_or(1), compiled.exit_code);
}

#[test]
fn former_parity_divergences_match_default_aot() {
    let dir = std::env::temp_dir().join(format!(
        "jet_dev_former_parity_divergences_{}",
        std::process::id()
    ));
    for (i, stem) in ["errors/typed_error_families", "serde/json_coerce"]
        .into_iter()
        .enumerate()
    {
        let stats = check_dev_default_stem(i, stem, &dir, &[]);
        assert_eq!(stats.ran, 1, "{stem} must execute through the fallback ladder");
        assert_eq!(stats.boundary, 0, "{stem} must not be a dev boundary");
        assert_eq!(stats.manifested, 0, "{stem} must exactly match default AOT");
    }
}

#[test]
fn previously_manifested_execution_skips_use_transparent_fallback() {
    let dir = std::env::temp_dir().join(format!(
        "jet_dev_unmasked_fallbacks_{}",
        std::process::id()
    ));
    for (i, stem) in [
        "io/db_checked_sql",
        "io/path",
        "tooling/data_pipeline",
    ]
    .into_iter()
    .enumerate()
    {
        let stats = check_dev_default_stem(i, stem, &dir, &[]);
        assert_eq!(
            stats.ran, 1,
            "{stem} must execute through the fallback ladder"
        );
        assert_eq!(stats.boundary, 0, "{stem} must not be a dev boundary");
        assert_eq!(stats.manifested, 0, "{stem} must not diverge from AOT");
    }
}

#[test]
fn sh_typed_text_default_matches_compiled_binary() {
    let dir = std::env::temp_dir().join(format!(
        "jet_dev_sh_typed_text_{}",
        std::process::id()
    ));
    let stats = check_dev_default_stem(0, "safety/sh_typed_text", &dir, &[]);
    assert_eq!(stats.ran, 1);
    assert_eq!(stats.boundary, 0);
    assert_eq!(stats.manifested, 0);
}

/// Every example that runs in the interpreter and has a checked-in
/// `expected/*.out` golden (the executable spec, I5) must match it byte for
/// byte — a cheap check that needs no rustc. Examples that hit a boundary, or
/// that have no golden (error/panic demos), are asserted as boundaries here too
/// so nothing is silently skipped.
#[test]
fn interpreter_matches_expected_golden() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0usize;
    for stem in all_example_stems() {
        let file = example_path(&stem);
        // D-JPK-TASKRUN1 / R12 (card #476): task_runner's meaningful entries are
        // its `#Task` fns, not the `fn run()` usage hint. Mirror golden.rs's
        // AOT `--task` battery on the interpreter tier via `run_named_task`,
        // proving the same TIR dispatches each task identically. The bare
        // `fn run()` output is not a golden.
        if stem == "jetpack/task_runner" {
            let mut bundle = jet::Loader::load_entry(&file).expect("task_runner loads");
            let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
                .into_iter()
                .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
                .collect();
            assert!(errors.is_empty(), "task_runner front end: {errors:?}");
            for (task, expected_name) in [("greet", "task_runner.greet"), ("seed", "task_runner.seed")] {
                let expected = fs::read_to_string(
                    root.join(format!("examples/features/expected/jetpack/{expected_name}.out")),
                )
                .unwrap_or_else(|_| panic!("missing expected/jetpack/{expected_name}.out"));
                match run_named_task(&bundle, task, false) {
                    RunOutcome::Ran { stdout, .. } => assert_eq!(
                        stdout, expected,
                        "interpreter --task={task} differs from golden"
                    ),
                    RunOutcome::Problems(diags) => panic!(
                        "interpreter --task={task} did not run: {:?}",
                        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
                    ),
                }
                checked += 1;
            }
            continue;
        }
        let expected_path = root.join(format!("examples/features/expected/{}.out", stem));
        match dev_iteration_with_timeout(&stem, &file, true) {
            RunOutcome::Ran { stdout, .. } => {
                if let Some(expected) = host_expected_stdout(&stem) {
                    assert_eq!(
                        stdout, expected,
                        "`{}`: interpreter output differs from host expected output",
                        stem
                    );
                    checked += 1;
                } else if let Ok(expected) = fs::read_to_string(&expected_path) {
                    assert_eq!(
                        stdout, expected,
                        "`{}`: interpreter output differs from expected golden",
                        stem
                    );
                    checked += 1;
                }
                // No golden (e.g. a panic demo) → nothing to compare; the
                // compiled-binary battery still covers it.
            }
            RunOutcome::Problems(diags) => {
                assert!(
                    is_named_dev_boundary(&stem, &diags),
                    "`{}` neither ran nor stopped at a named boundary; codes were {:?}",
                    stem,
                    diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
                );
            }
        }
    }
    assert!(checked > 0, "expected at least some golden comparisons");
}

/// E2202 fuel stop: a program whose top-level `loop` never breaks exhausts the
/// dev interpreter's step budget and stops with E2202 (an honest boundary, not
/// a hang). Driven through the comptime engine with a tiny fuel cap so the test
/// hits the same `burn()` path the watch loop uses, without burning the full
/// billion-step production budget.
#[test]
fn infinite_loop_hits_e2202_fuel_stop() {
    use std::collections::HashMap;
    let src = "fn run() {\n    n := 0\n    loop {\n        n = n + 1\n    }\n}\n";
    let prog = jet::Parser::parse(&jet::Lexer::lex(src).0).expect("fixture should parse");
    let mut funcs: HashMap<String, &jet::AST::Func> = HashMap::new();
    for item in &prog.items {
        if let jet::AST::Item::Func(f) = item {
            funcs.insert(f.name.clone(), f);
        }
    }
    let run = funcs.get("run").copied().expect("fixture has run");
    let mut sink = jet::Comptime::DevSink::new();
    let err = jet::Comptime::run_main_with_fuel(
        run,
        &funcs,
        std::path::Path::new("."),
        &mut sink,
        10_000,
    )
    .expect_err("an unbounded loop must exhaust the step budget");
    assert_eq!(
        err.code, "E2202",
        "an unbounded loop must stop with E2202, got: {}",
        err.code
    );
}

#[test]
fn task_program_hits_e2201_in_interpreter_mode() {
    let file = "examples/features/concurrency/tasks.jet";
    match dev_iteration(file, false, true) {
        RunOutcome::Problems(diags) => {
            assert_eq!(diags.len(), 1, "expected exactly one boundary note");
            let d = &diags[0];
            assert_eq!(d.code, "E2201");
            assert!(
                d.what.contains("spawns a task"),
                "E2201 should name the task feature, got: {}",
                d.what
            );
        }
        RunOutcome::Ran { .. } => {
            panic!("interpreter mode must not run task programs (E2201 boundary)")
        }
    }
}

/// c139 M4: task programs inside `resident_jit_safe` run via default `jet dev` (Cranelift), not E2201.
#[test]
fn task_program_runs_via_jit() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let file = "examples/features/concurrency/tasks.jet";
    let mut bundle = jet::Loader::load_entry(file).expect("tasks bundle should load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "tasks must type-check");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "tasks must be resident-safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle)
        .unwrap_or_else(|e| panic!("tasks JIT compile failed: {e}"));

    let mut backend = CraneliftBackend::new(InterpreterBackend::new());
    let jit = match backend.run(&bundle, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => panic!("tasks must run via JIT backend, got: {ds:?}"),
    };

    let got = match dev_iteration(file, false, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => panic!("tasks must run via default dev/JIT, got: {ds:?}"),
    };
    assert_eq!(got.trim(), "5050", "tasks expected sum");
    assert_eq!(
        jit.trim(),
        got.trim(),
        "JIT output drifted from dev_iteration"
    );
}

/// Scheduler workers catch user-task panics. Parallel tasks must never race by
/// swapping Rust's process-global panic hook and leak a raw worker panic line.
#[test]
fn caught_task_panics_keep_stderr_deterministic_under_parallel_repetition() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping scheduler panic-hook regression");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_dev_scheduler_panic_hook_{}",
        std::process::id()
    ));
    let file = "examples/features/concurrency/all_failfast.jet";
    let expected = ProgramOutput::ran(
        String::new(),
        "panic: a task panicked\n".to_string(),
        70,
    );
    let first = compiled_binary_output(&dir, "scheduler_panic_hook", 0, "all_failfast", file);
    assert_eq!(first, expected);

    for iteration in 0..8 {
        let fallback = match dev_iteration(file, false, false) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(diags) => {
                panic!("all_failfast fallback run {iteration} stopped at {diags:?}")
            }
        };
        assert_eq!(
            fallback, expected,
            "all_failfast fallback run {iteration} leaked non-Jet stderr"
        );
    }

    let binary = Arc::new(dir.join("jet_scheduler_panic_hook_0"));
    let failures = Arc::new(Mutex::new(Vec::new()));
    let mut workers = Vec::new();
    for worker in 0..8 {
        let binary = Arc::clone(&binary);
        let failures = Arc::clone(&failures);
        let expected = expected.clone();
        workers.push(std::thread::spawn(move || {
            for iteration in 0..8 {
                let run = command_output_with_timeout(
                    Command::new(binary.as_ref()),
                    DEV_DIFF_TIMEOUT,
                    &format!("scheduler panic run {worker}/{iteration}"),
                );
                let got = ProgramOutput::ran(
                    String::from_utf8_lossy(&run.stdout).into_owned(),
                    String::from_utf8_lossy(&run.stderr).into_owned(),
                    run.status.code().unwrap_or(1),
                );
                if got != expected {
                    failures.lock().unwrap().push(format!(
                        "run {worker}/{iteration}: expected {expected:?}, got {got:?}"
                    ));
                }
            }
        }));
    }
    for worker in workers {
        worker.join().expect("panic-hook regression worker panicked");
    }
    let failures = failures.lock().unwrap();
    assert!(
        failures.is_empty(),
        "caught task panic stderr drifted:\n{}",
        failures.join("\n")
    );
    let _ = fs::remove_dir_all(&dir);
}

/// c125 Phase 6 seed: uncovered effectful programs run under default dev via
/// transparent AOT subprocess fallback. Interpreter mode keeps the honest
/// E2201 boundary; the default backend owns the gap.
#[test]
fn dev_default_runs_env_program_via_aot_fallback() {
    let dir = std::env::temp_dir().join(format!("jet_dev_aot_fallback_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("env_fallback.jet");
    fs::write(
        &file,
        "use core.env as env\nfn run() {\n    print(env.current_dir())\n}\n",
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();

    match dev_iteration(&shown, false, true) {
        RunOutcome::Problems(diags) => {
            assert!(
                diags.iter().any(|d| d.code == "E2201"),
                "interpreter mode should still name the boundary: {diags:?}"
            );
        }
        RunOutcome::Ran { .. } => panic!("interpreter unexpectedly ran core.env program"),
    }

    let expected = compiled_binary_output(&dir, "aot_fallback", 0, "env_fallback", &shown);
    let got = match dev_iteration(&shown, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => {
            panic!("default dev should AOT-fallback-run core.env program: {diags:?}")
        }
    };
    let got = normalize_for_parity("env_fallback", got);
    let expected = normalize_for_parity("env_fallback", expected);
    assert_eq!(got, expected);
}

#[test]
fn dev_default_aot_fallback_matches_io_log() {
    let dir = std::env::temp_dir();
    let file = "examples/features/io/log.jet";
    let expected = compiled_binary_output(&dir, "aot_fallback_log", 0, "io/log", file);
    let got = match dev_iteration(file, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => {
            panic!("default dev should AOT-fallback-run io/log: {diags:?}")
        }
    };
    let got = normalize_for_parity("io/log", got);
    let expected = normalize_for_parity("io/log", expected);
    assert_eq!(got, expected);
}

#[test]
fn dev_default_aot_fallback_runs_resident_boundaries() {
    let dir = std::env::temp_dir();
    for stem in ["concurrency/task_controls", "memory/entity_tree"] {
        let file = example_path(stem);
        let expected = compiled_binary_output(&dir, "aot_fallback_resident", 0, stem, &file);
        let got = match dev_iteration(&file, false, false) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(diags) => {
                panic!("default dev should AOT-fallback-run {stem}: {diags:?}")
            }
        };
        assert_eq!(
            normalize_for_parity(stem, got),
            normalize_for_parity(stem, expected)
        );
    }
}

/// c139 M4: scheduler/channel spawn stress example is resident-safe and runs.
#[test]
fn scheduler_spawn_runs_via_jit() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let file = "examples/features/concurrency/scheduler_spawn.jet";
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "scheduler_spawn must type-check");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "scheduler_spawn must be resident-safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle)
        .unwrap_or_else(|e| panic!("scheduler_spawn JIT compile failed: {e}"));

    let got = match dev_iteration(file, false, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => {
            panic!("scheduler_spawn must run via default dev/JIT, got: {ds:?}")
        }
    };
    assert_eq!(got.trim(), "1000");
}

#[test]
fn dev_default_interprets_display_debug_interpolation() {
    let file = "examples/features/types/display_debug.jet";
    let got = match dev_iteration(file, false, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => {
            panic!("display/debug interpolation should run in default dev, got: {ds:?}")
        }
    };
    assert_eq!(got, golden_stdout("types/display_debug"));
}

/// D-DEV1 "try anyway": the opt-in flag skips the boundary scan and attempts
/// execution. For a task program it then fails honestly at whatever
/// unsupported construct it actually hits during interpretation, rather than
/// refusing up front at the pre-scan's (earlier, more conservative) report
/// site — no guarantees, but it tried.
///
/// c139 JIT-parity fix (2026-07-03): the dev interpreter's own comptime-leak
/// errors (E0956/E0951) are now rewrapped as E2201 for a consistent voice
/// (`Source/Interpreter.rs::dev_boundary_from_comptime`), so the diagnostic
/// CODE alone no longer distinguishes "blocked by the pre-scan" from "tried
/// and failed later" — both surface as E2201. Compare the failure SITE
/// instead: try-anyway must fail at a different span than the pre-scan's
/// (earlier / more conservative) report, proving real execution proceeded
/// past the boundary before hitting trouble.
#[test]
fn try_anyway_skips_the_boundary_scan() {
    let file = "examples/features/concurrency/tasks.jet";
    let RunOutcome::Problems(blocked) = dev_iteration(file, false, true) else {
        panic!("expected the E2201 pre-scan to block this program up front");
    };
    assert_eq!(blocked[0].code, "E2201", "pre-scan should report E2201");
    match dev_iteration(file, true, true) {
        RunOutcome::Problems(diags) => {
            assert_ne!(
                diags.first().and_then(|d| d.span),
                blocked.first().and_then(|d| d.span),
                "try-anyway must fail at a different site than the pre-scan, proving it skipped the scan and actually tried"
            );
        }
        // If a future evaluator can run it, that's fine too — the point is the
        // pre-scan was skipped.
        RunOutcome::Ran { .. } => {}
    }
}

/// c139 M1: the Cranelift tier-1 backend runs `basics/hello.jet` with byte-identical
/// stdout to the interpreter baseline.
#[test]
fn cranelift_backend_matches_hello() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let file = "examples/features/basics/hello.jet";
    let mut bundle = jet::Loader::load_entry(file).expect("hello bundle should load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "hello must type-check");

    let expected = match dev_iteration(file, false, true) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => {
            panic!("interpreter baseline must run hello, got diagnostics: {ds:?}")
        }
    };

    let mut backend = CraneliftBackend::new(InterpreterBackend::new());
    let got = match backend.run(&bundle, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => panic!("cranelift backend did not run hello: {ds:?}"),
    };
    assert_eq!(got, expected, "cranelift output drifted from interpreter");

    let src = fs::read_to_string(file).expect("hello source should read");
    let compiled = jet::compile_with_path(&src, file).expect("hello should compile");
    let rs = std::env::temp_dir().join("jet_dev_cranelift_hello.rs");
    let bin = std::env::temp_dir().join("jet_dev_cranelift_hello");
    fs::write(&rs, &compiled.rust).expect("write compiled rust");
    let rustc = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .expect("run rustc");
    assert!(
        rustc.status.success(),
        "rustc failed compiling hello fixture: {}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().expect("run compiled hello");
    let compiled_stdout = String::from_utf8_lossy(&run.stdout).to_string();
    assert_eq!(
        got, compiled_stdout,
        "cranelift output drifted from AOT binary"
    );
}

fn checked_bundle_from_path(file: &str) -> jet::AST::ProgramBundle {
    let mut b = jet::Loader::load_entry(file).expect("bundle should load");
    let diags = jet::Sema::check_bundle(&mut b, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "fixture must type-check: {errors:?}");
    b
}

fn assert_cranelift_matches_interpreter(src: &str, tag: &str) {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let p = std::env::temp_dir().join(format!("jet_jit_m3_{tag}.jet"));
    fs::write(&p, src).unwrap();
    let shown = p.to_string_lossy().to_string();

    let expected = match dev_iteration(&shown, false, true) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => {
            panic!("interpreter baseline must run `{tag}`, got diagnostics: {ds:?}")
        }
    };

    let bundle = checked_bundle_from_path(&shown);
    let mut backend = CraneliftBackend::new(InterpreterBackend::new());
    let got = match backend.run(&bundle, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => panic!("cranelift backend did not run `{tag}`: {ds:?}"),
    };
    assert_eq!(
        got, expected,
        "cranelift output drifted from interpreter for `{tag}`"
    );
}

fn run_cranelift_outcome(src: &str, tag: &str) -> ProgramOutput {
    let p = std::env::temp_dir().join(format!("jet_jit_result_{tag}.jet"));
    fs::write(&p, src).unwrap();
    let shown = p.to_string_lossy().to_string();
    let bundle = checked_bundle_from_path(&shown);
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "`{tag}` must use resident JIT, not fallback: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle)
        .unwrap_or_else(|e| panic!("`{tag}` JIT compile failed: {e}"));
    let mut backend = CraneliftBackend::new(InterpreterBackend::new());
    match backend.run(&bundle, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(ds) => panic!("`{tag}` JIT returned diagnostics: {ds:?}"),
    }
}

struct RejectJitFallback;

impl JitBackend for RejectJitFallback {
    fn run(&mut self, _: &jet::AST::ProgramBundle, _: bool) -> RunOutcome {
        panic!("resident JIT unexpectedly used its fallback")
    }

    fn hot_swap(
        &mut self,
        _: &str,
        _: &jet::AST::ProgramBundle,
        _: bool,
    ) -> Result<RunOutcome, Vec<jet::Diagnostics::Diagnostic>> {
        panic!("resident JIT unexpectedly used its fallback")
    }

    fn restart(&mut self, _: &jet::AST::ProgramBundle, _: bool) -> RunOutcome {
        panic!("resident JIT unexpectedly used its fallback")
    }
}

fn run_cranelift_outcome_without_fallback(src: &str, tag: &str) -> RunOutcome {
    let p = std::env::temp_dir().join(format!("jet_jit_no_fallback_{tag}.jet"));
    fs::write(&p, src).unwrap();
    let shown = p.to_string_lossy().to_string();
    let bundle = checked_bundle_from_path(&shown);
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "`{tag}` must use resident JIT: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle)
        .unwrap_or_else(|e| panic!("`{tag}` JIT compile failed: {e}"));
    let mut backend = CraneliftBackend::new(RejectJitFallback);
    backend.run(&bundle, false)
}

fn run_cranelift_without_fallback(src: &str, tag: &str) -> ProgramOutput {
    match run_cranelift_outcome_without_fallback(src, tag) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(ds) => panic!("`{tag}` JIT returned diagnostics: {ds:?}"),
    }
}

#[test]
fn generic_module_instance_runs_identically_in_resident_jit_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() { return; }
    let src = r#"
module Value<n: Int> { pub fn get() -> Int { return n } }
module Three = Value<3>
module Same = Value<3>
fn run() { print(Three.get()); print(Same.get()) }
"#;
    let jit = run_cranelift_without_fallback(src, "generic_module_instance");
    assert_eq!(jit.stdout, "3\n3\n");

    let dir = std::env::temp_dir().join(format!("jet_generic_module_jit_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("generic_module_instance.jet");
    fs::write(&file, src).unwrap();
    let aot = compiled_binary_output(&dir, "generic_module_instance", 0, "generic_module_instance", file.to_str().unwrap());
    assert_eq!(jit, aot);

    let bundle = checked_bundle_from_path(file.to_str().unwrap());
    let tir = jet::Codegen::TIR::lower_jit_program(&bundle).expect("generic instance lowers to JIT TIR");
    assert_eq!(tir.instance_provenance.len(), 1, "equivalent aliases share one canonical instance");
    assert_eq!(tir.funcs.iter().filter(|f| f.name == "Three__get").count(), 1);
}

#[test]
fn solver_state_transitions_match_aot_in_resident_jit() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let source_path = "examples/features/tooling/solve_puzzle.jet";
    let src = fs::read_to_string(source_path).expect("read solve_puzzle example");
    let jit = run_cranelift_without_fallback(&src, "solve_puzzle");

    let compiled = jet::compile_with_path(&src, source_path).expect("compile solve_puzzle");
    let dir = std::env::temp_dir().join(format!("jet_solver_jit_parity_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let rs = dir.join("solve_puzzle.rs");
    let bin = dir.join("solve_puzzle");
    fs::write(&rs, compiled.rust).unwrap();
    let rustc = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .expect("run rustc");
    assert!(
        rustc.status.success(),
        "rustc failed: {}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let output = Command::new(&bin).output().expect("run AOT solve_puzzle");
    let aot = ProgramOutput::ran(
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(1),
    );
    assert_eq!(jit, aot, "Solver state drifted between resident JIT and AOT");
    assert_eq!(
        jit.stdout,
        "key=1 door=3\nkey=3 door=1\nwins=2\nstatus=failed\nfailures=7\n"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn resident_jit_result_abi_covers_calls_ok_err_try_and_entry() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let success = r#"
fn choose_ok() -> Float ? String {
    return ok(0.25)
}

fn choose_err() -> Float ? String {
    return err("typed boom")
}

fn forward() -> Float ? String {
    value :: choose_ok()?
    return ok(value + 0.25)
}

fn run() -> Void ? {
    print(forward()?)
}
"#;
    let success_jit = run_cranelift_outcome(success, "result_success");
    assert_eq!(success_jit, ProgramOutput::ran("0.5\n".into(), "".into(), 0));

    let failure = success.replace("choose_ok()?", "choose_err()?");
    let failure_jit = run_cranelift_outcome(&failure, "result_failure");
    assert_eq!(
        failure_jit,
        ProgramOutput::ran("".into(), "typed boom\n".into(), 1)
    );

    for (src, tag, expected) in [
        (success, "result_success_interp", success_jit),
        (&failure, "result_failure_interp", failure_jit),
    ] {
        let p = std::env::temp_dir().join(format!("jet_jit_result_{tag}.jet"));
        fs::write(&p, src).unwrap();
        let shown = p.to_string_lossy().to_string();
        let interpreted = match dev_iteration(&shown, false, true) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(ds) => panic!("`{tag}` interpreter failed: {ds:?}"),
        };
        assert_eq!(interpreted, expected, "JIT/interpreter Result drift for `{tag}`");
    }
}

#[test]
fn resident_jit_fallible_void_cfg_fallthrough_matches_aot() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let one_arm_fallthrough = r#"
fn direct_ok() -> Int ? {
    return ok(7)
}

fn run() -> Void ? {
    print(direct_ok()?)
    stop :: false
    if stop {
        return err("one-arm stopped")
    }
    print("one-arm fallthrough")
}
"#;
    let nested_fallthrough = r#"
fn direct_ok() -> Int ? {
    return ok(7)
}

fn run() -> Void ? {
    print(direct_ok()?)
    outer :: true
    inner :: false
    if outer {
        if inner {
            return err("nested stopped")
        }
    }
    print("nested fallthrough")
}
"#;
    let neither_arm_terminates = r#"
fn direct_ok() -> Int ? {
    return ok(7)
}

fn run() -> Void ? {
    print(direct_ok()?)
    if true {
        print("left continues")
    } else {
        print("right continues")
    }
    print("neither terminated")
}
"#;
    let both_arms_terminate = r#"
fn direct_ok() -> Int ? {
    return ok(7)
}

fn run() -> Void ? {
    print(direct_ok()?)
    if true {
        return err("left branch")
    } else {
        return err("right branch")
    }
}
"#;

    let cases = [
        (
            "one_arm_fallthrough",
            one_arm_fallthrough,
            ProgramOutput::ran("7\none-arm fallthrough\n".into(), "".into(), 0),
        ),
        (
            "nested_fallthrough",
            nested_fallthrough,
            ProgramOutput::ran("7\nnested fallthrough\n".into(), "".into(), 0),
        ),
        (
            "neither_arm_terminates",
            neither_arm_terminates,
            ProgramOutput::ran(
                "7\nleft continues\nneither terminated\n".into(),
                "".into(),
                0,
            ),
        ),
        (
            "both_arms_terminate",
            both_arms_terminate,
            ProgramOutput::ran("7\n".into(), "left branch\n".into(), 1),
        ),
    ];

    for (i, (tag, src, expected)) in cases.into_iter().enumerate() {
        let jit = run_cranelift_without_fallback(src, tag);
        assert_eq!(jit, expected, "resident JIT CFG result drift for `{tag}`");

        let path = std::env::temp_dir().join(format!("jet_jit_{tag}.jet"));
        fs::write(&path, src).unwrap();
        let shown = path.to_string_lossy().to_string();
        let dir = std::env::temp_dir().join(format!("jet_jit_{tag}_{}", std::process::id()));
        let aot = compiled_binary_output(&dir, tag, i, tag, &shown);
        assert_eq!(aot, expected, "AOT CFG result drift for `{tag}`");
        assert_eq!(jit, aot, "AOT and resident JIT CFG semantics drift for `{tag}`");
    }
}

#[test]
fn resident_jit_fidelity_matches_runtime_contract() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let valid = r#"
use core.perf as Perf

fn run() -> Void ? {
    Perf.reset_fidelity()
    print(Perf.default_fidelity())
    Perf.override_fidelity(0.25)?
    print(Perf.fidelity())
    Perf.reset_fidelity()
    print(Perf.fidelity())
}
"#;
    let expected_valid = ProgramOutput::ran("1.0\n0.25\n1.0\n".into(), "".into(), 0);
    assert_eq!(run_cranelift_outcome(valid, "fidelity_valid"), expected_valid);
    let valid_path = std::env::temp_dir().join("jet_jit_fidelity_valid_interp.jet");
    fs::write(&valid_path, valid).unwrap();
    let valid_shown = valid_path.to_string_lossy().to_string();
    let interpreted = match dev_iteration(&valid_shown, false, true) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            ProgramOutput::ran(stdout, stderr, exit_code)
        }
        RunOutcome::Problems(ds) => panic!("fidelity interpreter failed: {ds:?}"),
    };
    assert_eq!(interpreted, expected_valid);
    let aot_dir = std::env::temp_dir().join(format!("jet_jit_fidelity_aot_{}", std::process::id()));
    assert_eq!(
        compiled_binary_output(&aot_dir, "fidelity", 0, "fidelity", &valid_shown),
        expected_valid
    );

    for (value, tag) in [
        ("-0.01", "negative"),
        ("1.01", "above_one"),
        ("(1.0 / 0.0)", "infinite"),
        ("(0.0 / 0.0)", "nan"),
    ] {
        let src = format!(
            r#"use core.perf as Perf
fn run() -> Void ? {{
    Perf.reset_fidelity()
    Perf.override_fidelity(0.375)?
    Perf.override_fidelity({value})?
}}"#
        );
        let got = run_cranelift_outcome(&src, tag);
        assert_eq!(got.exit_code, 1, "{tag} must fail");
        assert!(
            got.stderr
                .contains("core.perf.Perf.override_fidelity needs 0.0 through 1.0"),
            "{tag}: {:?}",
            got.stderr
        );
        let read = r#"use core.perf as Perf
fn run() { print(Perf.fidelity()) }"#;
        assert_eq!(
            run_cranelift_outcome(read, &format!("{tag}_state")),
            ProgramOutput::ran("0.375\n".into(), "".into(), 0),
            "{tag} changed fidelity before returning Err"
        );
    }
}

fn golden_stdout(stem: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(root.join(format!("examples/features/expected/{stem}.out")))
        .unwrap_or_else(|e| panic!("missing golden for `{stem}`: {e}"))
}

fn assert_cranelift_three_way(file: &str, stem: &str) {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "`{stem}` must type-check");
    let safety_detail = jet_jit::resident_jit_safe_bundle_detail(&bundle);
    let compile = jet_jit::try_compile_bundle(&bundle);
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle) && compile.is_ok(),
        "`{stem}` must be resident-JIT safe for three-way differential: safety={safety_detail:?}, compile={compile:?}"
    );

    let interpreted = match dev_iteration(file, false, true) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) if ds.iter().any(|d| BOUNDARY_CODES.contains(&d.code.as_str())) => {
            golden_stdout(stem)
        }
        RunOutcome::Problems(ds) => {
            panic!("interpreter baseline must run `{stem}`, got diagnostics: {ds:?}")
        }
    };

    let mut backend = CraneliftBackend::new(RejectJitFallback);
    let jit = match backend.run(&bundle, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => panic!("cranelift backend did not run `{stem}`: {ds:?}"),
    };
    assert_eq!(
        jit, interpreted,
        "JIT vs interpreter divergence for `{stem}`"
    );

    let src = fs::read_to_string(file).expect("read source");
    let compiled = jet::compile_with_path(&src, file).expect("compile");
    // stems are topic-relative paths (basics/compound) — flatten for temp names
    let flat = stem.replace('/', "_");
    let rs = std::env::temp_dir().join(format!("jet_jit_3way_{flat}.rs"));
    let bin = std::env::temp_dir().join(format!("jet_jit_3way_{flat}"));
    fs::write(&rs, &compiled.rust).unwrap();
    let rustc = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .expect("run rustc");
    assert!(
        rustc.status.success(),
        "rustc failed for `{stem}`: {}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().expect("run binary");
    let aot = String::from_utf8_lossy(&run.stdout).to_string();
    assert_eq!(jit, aot, "JIT vs AOT divergence for `{stem}`");
}

#[test]
fn generic_modules_full_example_matches_resident_jit_and_aot() {
    assert_cranelift_three_way(
        "examples/features/modules/generic_modules.jet",
        "modules/generic_modules",
    );
}

#[test]
fn resident_jit_safety_detail_smoke() {
    for stem in [
        "basics/compound",
        "basics/switch",
        "types/structs",
        "types/enums",
        "basics/branches",
        "concurrency/taskgroup",
    ] {
        let file = format!("examples/features/{stem}.jet");
        let mut bundle = jet::Loader::load_entry(&file).expect("load");
        jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
        let detail = jet_jit::resident_jit_safe_bundle_detail(&bundle);
        let stmts = jet_jit::jit_dump_main_stmts(&bundle);
        let funcs = jet_jit::jit_program_func_names(&bundle);
        eprintln!("{stem}: {detail}");
        if stem == "basics/switch" {
            eprintln!("  compile: {:?}", jet_jit::try_compile_bundle(&bundle));
        }
        if stem == "131_taskgroup" {
            let (sites, lams) = jet_jit::jit_spawn_stats(&bundle);
            eprintln!("  spawn: {sites} sites / {lams} lambdas");
            eprintln!(
                "  uncovered: {:?}",
                jet_jit::jit_main_uncovered_detail(&bundle)
            );
        }
        eprintln!("  funcs: {}", funcs.join(", "));
        eprintln!("  main: {}", stmts.join(", "));
        for c in jet_jit::jit_dump_mixed_switch_conds(&bundle) {
            eprintln!("  mixed: {c}");
        }
        for fn_name in ["show", "next", "label", "describe"] {
            if let Some(d) = jet_jit::resident_jit_func_safety_detail(&bundle, fn_name) {
                eprintln!("  {fn_name}: {d}");
            }
        }
        if let Err(e) = jet_jit::try_compile_bundle(&bundle) {
            eprintln!("  compile: {e}");
        }
    }
}

#[test]
fn resident_jit_safe_labeled_loop_control() {
    let file = "examples/features/basics/labeled_loops.jet";
    let mut bundle = jet::Loader::load_entry(file).expect("load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "labeled loop example must type-check");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "labeled break/continue should stay JIT-covered: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
}

#[test]
fn resident_jit_safe_increment_decrement() {
    let file = "examples/features/basics/increment.jet";
    let mut bundle = jet::Loader::load_entry(file).expect("load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "increment example must type-check");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "prefix/postfix ++/-- should stay JIT-covered: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
}

#[test]
fn resident_jit_safe_named_tuples() {
    let file = "examples/features/basics/tuples.jet";
    let mut bundle = jet::Loader::load_entry(file).expect("load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "tuple example must type-check");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "named tuple literal/access/equality/destructure should stay JIT-covered: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
}

#[test]
fn resident_jit_safe_chained_comparison() {
    let file = "examples/features/operators/chained_comparison.jet";
    let mut bundle = jet::Loader::load_entry(file).expect("load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "chained comparison example must type-check"
    );
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "same-direction chained comparisons should stay JIT-covered: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
}

#[test]
fn resident_jit_safe_string_method_chain() {
    let file = "examples/features/basics/method_chain.jet";
    let mut bundle = jet::Loader::load_entry(file).expect("load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "method-chain example must type-check");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "pure string method chains should stay JIT-covered: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
}

/// Audit which type-checked examples compile through the JIT lowerer. The committed manifest is
/// a ratchet baseline: any coverage movement is deliberate and reviewed.
#[test]
fn jit_coverage_audit() {
    let (covered, gaps) = collect_jit_coverage();
    let (expected_covered, expected_gaps, _) = parse_jit_gap_manifest();
    eprintln!("jit compile-covered ({}):", covered.len());
    for s in &covered {
        eprintln!("  {s}");
    }
    eprintln!("jit gaps ({}):", gaps.len());
    for g in &gaps {
        eprintln!("  {g}");
    }
    print_jit_op_report();
    assert_eq!(
        covered, expected_covered,
        "JIT covered set drifted; update tests/jit_gaps.txt only for an intentional ratchet move"
    );
    assert_eq!(
        gaps, expected_gaps,
        "JIT gap set drifted; update tests/jit_gaps.txt only for an intentional ratchet move"
    );
}

/// Stems of type-checked examples the resident JIT can run end-to-end.
fn jit_covered_example_stems() -> Vec<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut stems = Vec::new();
    for path in topic_jet_files(&root) {
        let file = path.to_string_lossy();
        let mut bundle = match jet::Loader::load_entry(&file) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
        if diags
            .iter()
            .any(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        {
            continue;
        }
        if jet_jit::resident_jit_safe_bundle(&bundle)
            && jet_jit::try_compile_bundle(&bundle).is_ok()
        {
            stems.push(stem_of(&root, &path));
        }
    }
    stems.sort();
    stems
}

/// c139 M3+: three-way differential (JIT == interpreter == AOT) on resident-safe examples.
#[test]
fn cranelift_three_way_differential_battery() {
    let _guard = dev_diff_lock().lock().unwrap();
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let have_rustc = have_rustc();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping three-way JIT differential battery");
        return;
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jit_covered_stems = jit_covered_example_stems();
    assert!(
        !jit_covered_stems.is_empty(),
        "expected at least one resident-safe example"
    );
    let mut ran = 0usize;
    for stem in &jit_covered_stems {
        let expected = root.join(format!("examples/features/expected/{stem}.out"));
        if !expected.exists() {
            continue;
        }
        assert_cranelift_three_way(&example_path(stem), stem);
        ran += 1;
    }
    eprintln!(
        "three-way battery: {} ran / {} resident-safe (with goldens)",
        ran,
        jit_covered_stems.len()
    );
    assert!(ran >= 9, "expected battery to grow beyond the M3 seed set");
}

/// Gate: the compile gate is the JIT coverage source of truth. A checked
/// example is either fully lowerable/compilable or has a ratcheted manifest
/// reason.
#[test]
fn jit_try_compile_manifest_matches() {
    let (covered, gaps) = collect_jit_coverage();
    let (expected_covered, expected_gaps, _) = parse_jit_gap_manifest();
    assert_eq!(
        covered, expected_covered,
        "JIT compile-covered set drifted; update tests/jit_gaps.txt only for an intentional ratchet move"
    );
    assert_eq!(
        gaps, expected_gaps,
        "JIT compile-gap set drifted; update tests/jit_gaps.txt only for an intentional ratchet move"
    );
}

/// c139 M3: string interpolation builds the same stdout as the interpreter.
#[test]
fn cranelift_covers_string_interpolation() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    n := 7\n    print(\"value {n}\")\n}\n",
        "str_interp",
    );
}

#[test]
fn cranelift_covers_shield_region() {
    let out = run_cranelift_without_fallback(
        "fn run() {\n    #Shield {\n        print(7)\n    }\n}\n",
        "shield_region",
    );
    assert_eq!(out.stdout, "7\n");
}

#[test]
fn cranelift_shield_defers_task_cancel_without_unwinding_native_frame() {
    let out = run_cranelift_without_fallback(
        r#"use core.tasks as tasks
fn run() {
    (sender, ch) :: tasks.channel<Int>()
    (ack_sender, ack) :: tasks.channel<Int>()
    slow :: tasks.spawn(take(ch, ack_sender) () => {
               #Shield {
                   value :: ch.receive() ?? panic("closed")
                   print(value)
                   ack_sender.send(1)
               }
               print(99)
       })
    slow.cancel()
    sender.send(42)
    ack.receive() ?? panic("closed")
}

"#,
        "shield_cancel",
    );
    assert_eq!(out.stdout, "42\n");
}

#[test]
fn cranelift_unshielded_receive_cancel_does_not_unwind_native_frame() {
    let out = run_cranelift_without_fallback(
        r#"use core.tasks as tasks
fn run() {
    (ready_sender, ready) :: tasks.channel<Int>()
    (sender, ch) :: tasks.channel<Int>()
    slow :: tasks.spawn(take(ch, ready_sender) () => {
        ready_sender.send(1)
        ch.receive() ?? panic("closed")
        print(99)
    })
    ready.receive() ?? panic("closed")
    slow.cancel()
    sender.send(42)
}
"#,
        "unshielded_receive_cancel",
    );
    assert_eq!(out.stdout, "");
}

#[test]
fn cranelift_unshielded_sleep_cancel_does_not_unwind_native_frame() {
    let out = run_cranelift_without_fallback(
        r#"use core.tasks as tasks
use core.time as time
fn run() {
    (ready_sender, ready) :: tasks.channel<Int>()
    slow :: tasks.spawn(take(ready_sender) () => {
        ready_sender.send(1)
        time.sleep(200)
        print(99)
    })
    ready.receive() ?? panic("closed")
    slow.cancel()
}
"#,
        "unshielded_sleep_cancel",
    );
    assert_eq!(out.stdout, "");
}

#[test]
fn cranelift_unshielded_select_cancel_does_not_unwind_native_frame() {
    let out = run_cranelift_without_fallback(
        r#"use core.tasks as tasks
fn run() {
    taskgroup g {
        (ready_sender, ready) :: tasks.channel<Int>()
        (_sender, ch) :: tasks.channel<Int>()
        slow :: tasks.spawn(take(g, ch, ready_sender) () => {
            ready_sender.send(1)
            g.select().recv(ch).wait()
            print(99)
        })
        ready.receive() ?? panic("closed")
        slow.cancel()
    }
}
"#,
        "unshielded_select_cancel",
    );
    assert_eq!(out.stdout, "");
}

#[test]
fn cranelift_wait_failures_return_compiler_diagnostics_not_process_exit() {
    let join_cancelled = r#"use core.tasks as tasks
use core.time as time
fn run() {
    child :: tasks.spawn(() => {
        time.sleep(200)
    })
    child.cancel()
    child.join()
}
"#;
    let RunOutcome::Problems(join_diags) =
        run_cranelift_outcome_without_fallback(join_cancelled, "join_cancelled")
    else {
        panic!("joining a cancelled task must report a compiler-owned diagnostic")
    };
    assert!(join_diags.iter().any(|d| d.code == "E0953"));

    let all_failfast = fs::read_to_string("examples/features/concurrency/all_failfast.jet")
        .expect("read all_failfast example");
    let RunOutcome::Problems(all_diags) =
        run_cranelift_outcome_without_fallback(&all_failfast, "all_failfast_boundary")
    else {
        panic!("all fail-fast must report its failure without exiting the test process")
    };
    assert!(all_diags.iter().any(|d| d.code == "E0953"));
}
/// c139 M3: checked integer arithmetic with overflow traps.
#[test]
fn cranelift_covers_checked_arithmetic() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    a := 10\n    b := 3\n    print(a + b)\n    print(a * b)\n    print(a - b)\n}\n",
        "arith",
    );
}

/// c139 M3: `let` chains and plain `if`/`else`.
#[test]
fn cranelift_covers_let_and_if() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    n := 5\n    if n > 3 {\n        print(1)\n    } else {\n        print(0)\n    }\n    m := n + 1\n    print(m)\n}\n",
        "let_if",
    );
}

/// c139 M3: calls between JIT-covered helper functions.
#[test]
fn cranelift_covers_function_calls() {
    assert_cranelift_matches_interpreter(
        "fn double(n: Int) -> Int {\n    return n * 2\n}\nfn run() {\n    print(double(3))\n    print(double(0))\n}\n",
        "calls",
    );
}

/// c139 M3+: counted `loop init; cond; step` with compound assign.
#[test]
fn cranelift_covers_counted_loop() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    sum := 0\n    loop i := 0; i < 5; i += 1 {\n        sum += i\n    }\n    print(sum)\n}\n",
        "counted_loop",
    );
}

/// c139 M3+: `loop cond` while-form and compound assign.
#[test]
fn cranelift_covers_while_loop() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    fuel := 3\n    loop fuel > 0 {\n        print(fuel)\n        fuel -= 1\n    }\n}\n",
        "while_loop",
    );
}

/// c139 M3+: inclusive range loop.
#[test]
fn cranelift_covers_range_loop() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    loop n in 1..3 {\n        print(n)\n    }\n}\n",
        "range_loop",
    );
}

/// c139 M3+: short-circuit && / ||.
#[test]
fn cranelift_covers_logic_ops() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    a := true\n    b := false\n    if a && !b {\n        print(1)\n    }\n    if b || a {\n        print(2)\n    }\n}\n",
        "logic_ops",
    );
}

/// c139 M3: string literals and locals passed to `print`.
#[test]
fn cranelift_covers_string_print() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    msg := \"hello, jit\"\n    print(msg)\n    print(\"done\")\n}\n",
        "strings",
    );
}

/// c125 Phase 2: Float list values use the shared JetArena list path.
#[test]
fn cranelift_covers_float_lists() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    xs: [Float] := [1.5, 2.5]\n    xs.push(3.5)\n    print(xs.len())\n    print(xs[0])\n    xs[1] = 4.5\n    print(xs[1])\n    mid :: xs[1..2]\n    print(mid[0])\n}\n",
        "float_lists",
    );
}

/// c125 Phase 2: records keep mixed scalar/String fields in JetArena.
#[test]
fn cranelift_covers_mixed_record_fields() {
    assert_cranelift_matches_interpreter(
        "struct Card {\n    name: String\n    score: Float\n    ready: Bool\n    mark: Char\n}\nfn run() {\n    c :: Card.{name: \"jet\", score: 2.5, ready: true, mark: 'J'}\n    print(c.name)\n    print(c.score)\n    print(c.ready)\n    print(c.mark)\n}\n",
        "mixed_record_fields",
    );
}

/// c139 M2: type-stable hot_swap re-links in the resident JIT and preserves
/// live runtime state; restart tears it down.
#[test]
fn cranelift_hot_swap_preserves_live_state() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    fn checked_bundle(src: &str, tag: &str) -> jet::AST::ProgramBundle {
        let mut b = bundle_of(src, tag);
        let diags = jet::Sema::check_bundle(&mut b, jet::Sema::CompileMode::Run);
        let errors: Vec<_> = diags
            .into_iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "fixture must type-check: {errors:?}");
        b
    }

    let v1 = checked_bundle("fn run() {\n    print(\"v1\")\n}\n", "jit_swap_v1");
    let v2 = checked_bundle("fn run() {\n    print(\"v2\")\n}\n", "jit_swap_v2");

    let mut backend = CraneliftBackend::new(InterpreterBackend::new());
    let out1 = match backend.run(&v1, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => panic!("first run failed: {ds:?}"),
    };
    assert_eq!(out1, "v1\n");
    assert_eq!(jet_jit::resident_invocations_for_test(), 1);

    let out2 = match backend.hot_swap("run", &v2, false) {
        Ok(RunOutcome::Ran { stdout, .. }) => stdout,
        Ok(RunOutcome::Problems(ds)) => panic!("hot_swap produced diagnostics: {ds:?}"),
        Err(ds) => panic!("hot_swap failed: {ds:?}"),
    };
    assert_eq!(out2, "v2\n");
    assert_eq!(
        jet_jit::resident_invocations_for_test(),
        2,
        "hot_swap must preserve resident invocation count (live state)"
    );

    let mut backend2 = CraneliftBackend::new(InterpreterBackend::new());
    let out3 = match backend2.hot_swap("run", &v1, false) {
        Ok(RunOutcome::Ran { stdout, .. }) => stdout,
        Ok(RunOutcome::Problems(ds)) => panic!("second backend hot_swap failed: {ds:?}"),
        Err(ds) => panic!("second backend hot_swap errored: {ds:?}"),
    };
    assert_eq!(out3, "v1\n");
    assert_eq!(jet_jit::resident_invocations_for_test(), 3);

    let out4 = match backend.restart(&v2, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => panic!("restart failed: {ds:?}"),
    };
    assert_eq!(out4, "v2\n");
    assert_eq!(
        jet_jit::resident_invocations_for_test(),
        1,
        "restart must reset resident live state"
    );
}

/// c125 P0 regression guard: a runtime panic under the default JIT backend
/// (list index OOB here; the same trapped-flag mechanism covers checked-arith
/// overflow and the two concurrency panic sites) must report a clean E0953 —
/// not kill the resident process. The next hot-reload iteration in the SAME
/// process must then run cleanly, proving the trap didn't leak into the next
/// run's heap and the process is still alive to serve it. Before the fix,
/// every one of these host shims called `std::process::exit(70)` directly,
/// which took the whole `jet dev` server down with it.
#[test]
fn cranelift_trap_then_hot_swap_continues() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    fn checked_bundle(src: &str, tag: &str) -> jet::AST::ProgramBundle {
        let mut b = bundle_of(src, tag);
        let diags = jet::Sema::check_bundle(&mut b, jet::Sema::CompileMode::Run);
        let errors: Vec<_> = diags
            .into_iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "fixture must type-check: {errors:?}");
        b
    }

    let panics = checked_bundle(
        "fn run() {\n    xs: [Int] :: [1, 2, 3]\n    print(xs[99])\n}\n",
        "jit_trap_v1",
    );
    let recovers = checked_bundle("fn run() {\n    print(\"recovered\")\n}\n", "jit_trap_v2");

    assert!(
        jet_jit::resident_jit_safe_bundle(&panics),
        "trap fixture must be resident-safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&panics)
    );

    let mut backend = CraneliftBackend::new(InterpreterBackend::new());
    match backend.run(&panics, false) {
        RunOutcome::Problems(diags) => {
            assert!(
                diags.iter().any(|d| d.code == "E0953"),
                "expected E0953 for list index OOB, got {:?}",
                diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
            );
        }
        RunOutcome::Ran { stdout, .. } => {
            panic!("expected the list index OOB to trap, got stdout {stdout:?}")
        }
    }

    // Same resident process (thread-local module/runtime), next hot-reload
    // iteration: must run to completion, not carry over the trapped flag or
    // the crashed run's partial heap.
    let out = match backend.hot_swap("run", &recovers, false) {
        Ok(RunOutcome::Ran { stdout, .. }) => stdout,
        Ok(RunOutcome::Problems(ds)) => {
            panic!("hot_swap after a trap produced diagnostics: {ds:?}")
        }
        Err(ds) => panic!("hot_swap after a trap errored: {ds:?}"),
    };
    assert_eq!(out, "recovered\n");
}

/// The dev iteration surfaces front-end errors identically to batch
/// compilation (D-DEV: same diagnostics).
#[test]
fn front_end_errors_surface_in_dev_iteration() {
    // Write a broken program to a temp file.
    let dir = std::env::temp_dir();
    let file = dir.join("jet_dev_broken.jet");
    fs::write(&file, "fn run() {\n    print(nope);\n}\n").unwrap();
    let shown = file.to_string_lossy().to_string();
    match dev_iteration(&shown, false, true) {
        RunOutcome::Problems(diags) => {
            assert!(!diags.is_empty(), "broken program must report problems");
            assert!(
                diags
                    .iter()
                    .all(|d| matches!(d.severity, jet::Diagnostics::Severity::Error)),
                "dev should surface errors"
            );
        }
        RunOutcome::Ran { .. } => panic!("a broken program must not run"),
    }
}

/// D-DEV3 latency budget: a save-to-diagnostic round (check-only) must stay
/// under 200ms on the example set. We measure the front-end check, which is
/// the diagnostic-producing work the watch loop does on every save.
#[test]
fn check_latency_under_budget() {
    let file = "examples/features/collections/wordcount.jet";
    // Warm up (first load touches the filesystem and caches).
    let _ = jet::check_with_path(file);
    let mut best = u128::MAX;
    for _ in 0..5 {
        let started = Instant::now();
        let _ = jet::check_with_path(file);
        best = best.min(started.elapsed().as_millis());
    }
    assert!(
        best < 200,
        "save-to-diagnostic latency {} ms exceeds the 200ms budget (D-DEV3)",
        best
    );
}

// ── c77: hot-swap type-surface stability (D-HOTSWAP1 / E2210) ──────────

/// Parse `src` to a bundle via a temp file (the only loader entry point).
fn bundle_of(src: &str, tag: &str) -> jet::AST::ProgramBundle {
    let p = std::env::temp_dir().join(format!("jet_hotswap_{tag}.jet"));
    fs::write(&p, src).unwrap();
    jet::Loader::load_entry(p.to_str().unwrap()).expect("bundle should load")
}

const STRUCT_OLD: &str = "struct P {\n    x: Int\n}\nfn f(p: P) -> Int {\n    return p.x\n}\nfn run() {\n    print(f(P.{x: 1}))\n}\n";

/// A body-only edit keeps the type surface stable → swap (Ok).
#[test]
fn body_only_edit_is_type_stable() {
    let old = bundle_of(STRUCT_OLD, "stable_old");
    let new = bundle_of(
        "struct P {\n    x: Int\n}\nfn f(p: P) -> Int {\n    return p.x + 1\n}\nfn run() {\n    print(f(P.{x: 2}))\n}\n",
        "stable_new",
    );
    assert!(
        jet::Sema::HotSwap::type_stable_check(&old, &new, "run").is_ok(),
        "a body-only edit must be type-stable (swap path)"
    );
}

/// Adding a struct field changes the surface → restart, with E2210 naming it.
#[test]
fn struct_field_change_emits_e2210() {
    let old = bundle_of(STRUCT_OLD, "field_old");
    let new = bundle_of(
        "struct P {\n    x: Int\n    y: Int\n}\nfn f(p: P) -> Int {\n    return p.x\n}\nfn run() {\n    print(f(P.{x: 1, y: 2}))\n}\n",
        "field_new",
    );
    match jet::Sema::HotSwap::type_stable_check(&old, &new, "run") {
        Ok(()) => panic!("adding a struct field must force a restart"),
        Err(diags) => {
            assert_eq!(diags.len(), 1);
            assert_eq!(diags[0].code, "E2210");
            assert!(
                diags[0].what.contains("struct `P`"),
                "E2210 should name the changed struct, got: {}",
                diags[0].what
            );
        }
    }
}

/// Changing a function's return type changes the surface → E2210.
#[test]
fn fn_signature_change_emits_e2210() {
    let old = bundle_of(
        "fn g(a: Int) -> Int {\n    return a\n}\nfn run() {\n    print(g(1))\n}\n",
        "sig_old",
    );
    let new = bundle_of(
        "fn g(a: Int) -> Bool {\n    return a == 0\n}\nfn run() {\n    print(g(1))\n}\n",
        "sig_new",
    );
    match jet::Sema::HotSwap::type_stable_check(&old, &new, "run") {
        Ok(()) => panic!("a return-type change must force a restart"),
        Err(diags) => {
            assert_eq!(diags[0].code, "E2210");
            assert!(diags[0].what.contains("return type"));
        }
    }
}

/// Adding an enum variant changes the surface → E2210.
#[test]
fn enum_variant_change_emits_e2210() {
    let old = bundle_of(
        "enum E {\n    A\n    B\n}\nfn run() {\n    print(1)\n}\n",
        "enum_old",
    );
    let new = bundle_of(
        "enum E {\n    A\n    B\n    C\n}\nfn run() {\n    print(1)\n}\n",
        "enum_new",
    );
    match jet::Sema::HotSwap::type_stable_check(&old, &new, "run") {
        Ok(()) => panic!("adding an enum variant must force a restart"),
        Err(diags) => {
            assert_eq!(diags[0].code, "E2210");
            assert!(diags[0].what.contains("enum `E`"));
        }
    }
}

/// D-PERSIST1: `@Persist` is inert in a release build — the marker carries no
/// runtime-carry-across machinery yet (named gate: a module+name-keyed value
/// store in the JIT resident runtime, `crates/jet-jit`, doesn't exist). This
/// asserts that by construction: the generated Rust for a module-level
/// `const` is byte-for-byte identical with and without `@Persist`.
#[test]
fn persist_marker_is_codegen_inert() {
    let dir = std::env::temp_dir().join(format!("jet_persist_parity_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let compile = |src: &str, name: &str| {
        let path = dir.join(name);
        fs::write(&path, src).unwrap();
        let shown = path.to_string_lossy().to_string();
        jet::compile_with_path(src, &shown)
            .unwrap_or_else(|diags| {
                panic!(
                    "front end rejected fixture:\n{}",
                    jet::render_diagnostics(&shown, src, &diags)
                )
            })
            .rust
    };
    // Same file name for both variants so the only possible difference in the
    // generated `source=` comment is the content, not the path.
    let strip_source_map = |rust: String| -> String {
        rust.lines()
            .filter(|l| !l.starts_with("// jet:source-map"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let plain = strip_source_map(compile(
        "const counter = 0;\nfn run() {\n    print(counter)\n}\n",
        "counter.jet",
    ));
    fs::remove_file(dir.join("counter.jet")).ok();
    let persisted = strip_source_map(compile(
        "@Persist const counter = 0;\nfn run() {\n    print(counter)\n}\n",
        "counter.jet",
    ));
    assert_eq!(
        plain, persisted,
        "@Persist must not change generated Rust (inert in release)"
    );
}

// ── card #131 S1-bridge (D-SERDE2): hand codec dev-tier parity (R12) ──────────
// A hand `impl T.Encode`/`impl T.Decode` round-trips under the native build (see
// tests/corelib.rs::hand_written_encode_decode_round_trips). The dev interpreter
// does not cover the json typed-decode path — and it doesn't for a DERIVED
// `@[Codable]` either — so the honest behavior for BOTH is to stop at the E2201
// pre-scan boundary and defer to native, never emit a divergent result. This test
// pins that parity: the dev tier must not silently produce a wrong round trip.
#[test]
fn hand_written_codec_dev_tier_stops_at_honest_boundary() {
    let _guard = dev_diff_lock().lock().unwrap();
    const SRC: &str = r#"
use core.encoding.json as json

struct Email { addr: String }

impl Email.Encode {
    fn encode(self) -> DataTree {
        m: [String: DataTree] :: ["email": DataTree.Text(copy self.addr)]
        return DataTree.Object(m)
    }
}

impl Email.Decode {
    fn decode(tree: DataTree) -> Email ? DecodeError {
        f := tree.field("email") ?? DataTree.Text("")
        s := f.text() ?? ""
        return ok(Email.{addr: s})
    }
}

fn run() {
    e := Email.{addr: "a@b.com"}
    s := json.to_string(e)
    print(s)
    back := json.decode<Email>(s) ?? panic("decode failed")
    print(back.addr)
}
"#;
    let dir = std::env::temp_dir().join(format!("jet_hand_codec_dev_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("hand_codec.jet");
    fs::write(&file, SRC).unwrap();
    let outcome = dev_iteration_with_timeout("hand_codec", file.to_str().unwrap(), true);
    match outcome {
        RunOutcome::Problems(diags) => {
            assert!(
                diags.iter().any(|d| d.code == "E2201"),
                "hand codec should stop at the E2201 honest boundary; got codes {:?}",
                diags.iter().map(|d| d.code.clone()).collect::<Vec<_>>()
            );
        }
        RunOutcome::Ran { stdout, .. } => {
            panic!("dev interpreter unexpectedly ran the hand codec (must defer to native): {stdout}");
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

/// D-SCHEDULE1 (ratified 2026-07-11, card #505): `jet dev`'s due-task tick
/// consumer. `scheduled_tasks` must enumerate every `#Task #Every(…)` fn
/// with its resolved schedule (and skip a plain `#Task fn` with no
/// `#Every(…)`), and `run_named_task` must actually execute one by name
/// through the same interpreter tier `dev_iteration` uses — golden-testing
/// the loop's per-tick logic without the long-running file watcher, same
/// spirit as `dev_iteration` itself (see the module doc above).
#[test]
fn schedule_every_dev_loop_consumer() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let file = root.join("examples/features/devloop/schedule_every.jet");
    let src = fs::read_to_string(&file).unwrap();
    let mut bundle = jet::Loader::load_entry(file.to_str().unwrap())
        .unwrap_or_else(|diags| panic!("schedule_every.jet failed to load: {diags:?}"));
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "schedule_every.jet must compile clean:\n{}",
        jet::render_diagnostics("schedule_every.jet", &src, &diags)
    );

    let mut tasks = jet::Interpreter::scheduled_tasks(&bundle);
    tasks.sort_by(|a, b| a.0.cmp(&b.0));
    let names: Vec<&str> = tasks.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        vec!["nightly_backup", "prune_sessions"],
        "scheduled_tasks must list every #Task fn carrying #Every(…), and skip the \
         #Every(…)-less `manual_only` task"
    );
    let schedules: std::collections::HashMap<&str, &jet::AST::EverySchedule> =
        tasks.iter().map(|(n, s)| (n.as_str(), s)).collect();
    assert_eq!(
        *schedules["prune_sessions"],
        jet::AST::EverySchedule::Interval {
            nanos: 5 * 60 * 1_000_000_000
        },
        "`#Every(5min)` must resolve to a 5-minute interval"
    );
    assert_eq!(
        *schedules["nightly_backup"],
        jet::AST::EverySchedule::DailyAt { hour: 3, minute: 0 },
        "`#Every(\"03:00\")` must resolve to 03:00 daily"
    );

    // Actually invoking a named task runs it like an ordinary call.
    match jet::Interpreter::run_named_task(&bundle, "prune_sessions", false) {
        RunOutcome::Ran { stdout, exit_code, .. } => {
            assert_eq!(exit_code, 0);
            assert_eq!(stdout, "pruning sessions\n");
        }
        RunOutcome::Problems(diags) => panic!("run_named_task failed: {diags:?}"),
    }
}
