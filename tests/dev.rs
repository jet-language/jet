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
use jet::Interpreter::{dev_iteration, dev_run_bundle, run_named_task, RunOutcome};
use jet::JitBackend::JitBackend;
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
            | "crypto/auth_tokens"
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
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
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

fn try_compiled_binary_output(
    dir: &std::path::Path,
    tag: &str,
    i: usize,
    stem: &str,
    file: &str,
) -> Option<ProgramOutput> {
    let src = fs::read_to_string(file).ok()?;
    let compiled = jet::compile_with_path(&src, file).ok()?;
    let rs = dir.join(format!("jet_{tag}_{i}.rs"));
    let bin = dir.join(format!("jet_{tag}_{i}"));
    fs::create_dir_all(dir).ok()?;
    fs::write(&rs, &compiled.rust).ok()?;
    let mut rustc_cmd = Command::new("rustc");
    rustc_cmd
        .args(["--edition", "2021"])
        .arg("-O")
        .arg(&rs)
        .arg("-o")
        .arg(&bin);
    if let Some(link) = &compiled.ffi {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let clinks = jet::resolve_c_links(file).ok()?;
    for arg in clinks {
        rustc_cmd.arg(arg);
    }
    let out = command_output_with_timeout(
        rustc_cmd,
        DEV_DIFF_TIMEOUT,
        &format!("rustc build for `{stem}`"),
    );
    if !out.status.success() {
        return None;
    }
    let run = command_output_with_timeout(
        Command::new(&bin),
        DEV_DIFF_TIMEOUT,
        &format!("compiled binary run for `{stem}`"),
    );
    Some(ProgramOutput::ran(
        String::from_utf8_lossy(&run.stdout).to_string(),
        String::from_utf8_lossy(&run.stderr).to_string(),
        run.status.code().unwrap_or(1),
    ))
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

fn dev_cli_output_with_stdin(
    file: &str,
    stdin: &std::path::Path,
    label: &str,
) -> ProgramOutput {
    let mut dev_cmd = Command::new(env!("CARGO_BIN_EXE_jet"));
    dev_cmd
        .args(["dev", file, "--watch=off"])
        .stdin(fs::File::open(stdin).unwrap());
    let dev = command_output_with_timeout(dev_cmd, DEV_DIFF_TIMEOUT, label);
    let stdout = String::from_utf8_lossy(&dev.stdout);
    let (runtime_stdout, status) = stdout
        .rsplit_once("✓ ran in ")
        .expect("one-shot jet dev must print its completion status");
    assert!(
        status
            .strip_suffix(" ms\n")
            .is_some_and(|millis| !millis.is_empty() && millis.bytes().all(|b| b.is_ascii_digit())),
        "unexpected one-shot jet dev completion status: {status:?}"
    );
    ProgramOutput::ran(
        runtime_stdout.to_string(),
        String::from_utf8_lossy(&dev.stderr).to_string(),
        dev.status.code().unwrap_or(1),
    )
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

fn typechecked_example_stems() -> Vec<String> {
    all_example_stems()
        .into_iter()
        .filter(|stem| {
            let diags = jet::check_with_path(&example_path(stem));
            !diags
                .iter()
                .any(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        })
        .collect()
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
    "E2201", "E2202", "E0952", "E0956", "E0953", "E3410", "E3411", "E1265", "E2211",
];

const DEFAULT_BACKEND_EXPECTED_BOUNDARIES: &[&str] = &[
    "collections/list_bounds",
    "concurrency/all_failfast",
];

fn jit_gap_stem_set() -> std::collections::HashSet<String> {
    let (_, gaps, _) = parse_jit_gap_manifest();
    gaps.iter()
        .map(|entry| entry.split(':').next().unwrap().to_string())
        .collect()
}

#[derive(Default, Clone)]
struct DevBatteryStats {
    ran: usize,
    boundary: usize,
    jit_gap: usize,
    manifested: usize,
    boundary_stems: Vec<String>,
}

impl DevBatteryStats {
    fn add(&mut self, mut other: DevBatteryStats) {
        self.ran += other.ran;
        self.boundary += other.boundary;
        self.jit_gap += other.jit_gap;
        self.manifested += other.manifested;
        self.boundary_stems.append(&mut other.boundary_stems);
    }
}

const DEV_BATTERY_STACK: usize = 8 * 1024 * 1024;

fn assert_default_dev_jit_gap(stem: &str, file: &str) {
    match dev_iteration_with_timeout(stem, file, false) {
        RunOutcome::Problems(diags) => {
            assert!(
                jet_jit::is_e2211(&diags),
                "`{stem}` should report E2211 under strict JIT, got {diags:?}"
            );
        }
        RunOutcome::Ran { .. } => panic!("`{stem}` must not run via default strict JIT"),
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
    let interpreted = match dev_iteration_with_timeout(stem, &file, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => {
            if jet_jit::is_e2211(&diags) {
                eprintln!("default jit gap: {stem}");
                return DevBatteryStats {
                    jit_gap: 1,
                    ..DevBatteryStats::default()
                };
            }
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
        handles.push(
            std::thread::Builder::new()
                .stack_size(DEV_BATTERY_STACK)
                .spawn(move || {
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
                })
                .expect("dev default worker"),
        );
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
    if uses_ffi_bridge(stem) {
        return DevBatteryStats::default();
    }
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
        handles.push(
            std::thread::Builder::new()
                .stack_size(DEV_BATTERY_STACK)
                .spawn(move || {
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
                })
                .expect("interpreter worker"),
        );
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

/// c125 M4 exit gate: the DEFAULT `jet dev` path uses strict Cranelift. JIT-covered
/// examples must match the AOT binary; uncovered examples report E2211 instead of
/// transparent AOT fallback (c728).
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
    let stats = run_dev_default_battery_parallel(typechecked_example_stems(), dir, manifested_divergences);
    eprintln!(
        "c125 default-backend battery: {} ran ({} default==compiled, {} manifested divergences), {} jit-gap, {} boundary-asserted, {} total",
        stats.ran,
        stats.ran - stats.manifested,
        stats.manifested,
        stats.jit_gap,
        stats.boundary,
        stats.ran + stats.jit_gap + stats.boundary
    );
    assert!(
        stats.ran > 0,
        "expected at least some examples to run via the default jet dev backend"
    );
    assert!(
        stats.jit_gap > 0,
        "expected strict JIT to report E2211 for uncovered examples instead of AOT fallback"
    );
    let mut observed_boundaries = stats.boundary_stems;
    observed_boundaries.sort();
    observed_boundaries.dedup();
    assert_eq!(
        observed_boundaries,
        DEFAULT_BACKEND_EXPECTED_BOUNDARIES,
        "default jet dev boundary set must stay exact"
    );
    assert_eq!(
        stats.manifested, 0,
        "default jet dev must not carry manifested stdout/stderr/exit-code divergences"
    );
}

#[test]
fn stdin_filter_default_dev_reports_jit_gap() {
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
    assert_eq!(compiled.stdout.trim(), "jet one\njet two");
    assert_default_dev_jit_gap("io/stdin_filter", &file);
}

#[test]
fn former_parity_divergences_report_jit_gap_on_default_dev() {
    let dir = std::env::temp_dir().join(format!(
        "jet_dev_former_parity_divergences_{}",
        std::process::id()
    ));
    for stem in ["errors/typed_error_families", "serde/json_coerce"] {
        let file = example_path(stem);
        assert_default_dev_jit_gap(stem, &file);
        let _ = compiled_binary_output(&dir, "former_parity_aot", 0, stem, &file);
    }
}

#[test]
fn previously_manifested_execution_reports_jit_gap_or_boundary() {
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
        assert!(
            stats.jit_gap == 1 || stats.boundary == 1,
            "{stem} must report E2211 or a named boundary under strict JIT"
        );
    }
}

#[test]
fn data_schema_empty_and_generic_rows_report_jit_gap_on_default_dev() {
    let file = example_path("tooling/data_json");
    assert_default_dev_jit_gap("tooling/data_json", &file);
}

#[test]
fn sh_typed_text_default_reports_jit_gap() {
    let file = example_path("safety/sh_typed_text");
    assert_default_dev_jit_gap("safety/sh_typed_text", &file);
}

#[test]
fn hidden_generic_constructor_default_dev_matches_aot() {
    if !have_rustc() {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_dev_generic_constructor_{}",
        std::process::id()
    ));
    let stats = check_dev_default_stem(
        0,
        "types/generic_constructor_inference",
        &dir,
        &[],
    );
    assert_eq!(stats.ran, 1);
    assert_eq!(stats.boundary, 0);
    assert_eq!(stats.manifested, 0);
}

fn check_task_runner_interpreter(root: &PathBuf, file: &str) {
    let mut bundle = jet::Loader::load_entry(file).expect("task_runner loads");
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "task_runner front end: {errors:?}");
    for (task, expected_name) in [
        ("greet", "task_runner.greet"),
        ("seed", "task_runner.seed"),
    ] {
        let expected = fs::read_to_string(
            root.join(format!("examples/features/expected/devloop/{expected_name}.out")),
        )
        .unwrap_or_else(|_| panic!("missing expected/devloop/{expected_name}.out"));
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
    }
}

#[test]
fn task_runner_named_tasks_match_expected_golden() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    check_task_runner_interpreter(&root, &example_path("devloop/task_runner"));
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
        if stem == "devloop/task_runner" {
            check_task_runner_interpreter(&root, &file);
            checked += 2;
            continue;
        }
        if uses_ffi_bridge(&stem) {
            continue;
        }
        // `examples/features/expected/web/web_wasm_*.out` are Node/browser
        // harness goldens (web_build), not interpreter `fn run()` print shape.
        if stem.starts_with("web/web_wasm_") {
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
fn fluent_method_chain_preserves_fuel_order_and_spans() {
    use std::collections::HashMap;

    let run_chain = |links: usize, fuel: u64| {
        let src = format!(
            "fn run() {{\n    print(\" x \"{})\n}}\n",
            ".trim()".repeat(links)
        );
        let prog = jet::Parser::parse(&jet::Lexer::lex(&src).0).expect("fixture should parse");
        let mut funcs: HashMap<String, &jet::AST::Func> = HashMap::new();
        for item in &prog.items {
            if let jet::AST::Item::Func(f) = item {
                funcs.insert(f.name.clone(), f);
            }
        }
        let run = funcs.get("run").copied().expect("fixture has run");
        let argument = match run.body.first() {
            Some(jet::AST::Stmt::Expr(jet::AST::Expr::Call(call))) => &call.args[0].expr,
            other => panic!("expected print call, got {other:?}"),
        };
        let mut method_spans = Vec::new();
        let mut cursor = argument;
        while let jet::AST::Expr::MethodCall {
            receiver,
            method_span,
            ..
        } = cursor
        {
            method_spans.push(*method_span);
            cursor = receiver;
        }
        let expected_exhaustion_span = method_spans.get(2).copied();
        let mut sink = jet::Comptime::DevSink::new();
        let result = jet::Comptime::run_main_with_fuel(
            run,
            &funcs,
            std::path::Path::new("."),
            &mut sink,
            fuel,
        );
        (result, sink.stdout, expected_exhaustion_span)
    };

    let (one, stdout, _) = run_chain(1, 4);
    one.expect("one method link should fit exactly inside four fuel steps");
    assert_eq!(stdout, "x\n");

    for links in [3, 100] {
        let (result, _, expected_span) = run_chain(links, 3);
        let err = result.expect_err("the chain should exhaust three fuel steps");
        assert_eq!(err.code, "E2202");
        assert_eq!(
            err.span, expected_span,
            "{links}-link chain must fail on the second inner method"
        );
    }
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

    let mut backend = CraneliftBackend::new();
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

    // Strict JIT no longer AOT-fallbacks all_failfast; parallel AOT runs keep
    // the panic-hook regression signal.

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

/// c728: uncovered effectful programs report E2211 under default dev; interpreter
/// mode keeps the honest E2201 boundary.
#[test]
fn dev_default_reports_jit_gap_for_env_program() {
    let dir = std::env::temp_dir().join(format!("jet_dev_jit_gap_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("env_gap.jet");
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

    match dev_iteration(&shown, false, false) {
        RunOutcome::Problems(diags) => {
            assert!(
                diags.iter().any(|d| d.code == "E2211"),
                "default dev should report a JIT gap, not fallback: {diags:?}"
            );
            assert!(
                diags.iter().any(|d| d.why.contains("JIT gap in run:")),
                "E2211 should name the function and gap detail: {diags:?}"
            );
        }
        RunOutcome::Ran { .. } => panic!("default dev must not AOT-fallback-run core.env program"),
    }
}

/// c728 C3: strict Cranelift traces JIT execution and never invokes tier-0 fallback.
#[test]
fn strict_jit_traces_execution_without_fallback() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    jet_jit::reset_jit_trace_for_test();
    let file = example_path("basics/hello");
    match dev_iteration(&file, false, false) {
        RunOutcome::Ran { .. } => {}
        other => panic!("hello must run via strict JIT: {other:?}"),
    }
    assert!(jet_jit::jit_executed_for_test(), "strict path must execute JIT");
    assert!(
        !jet_jit::fallback_invoked_for_test(),
        "strict path must not invoke interpreter fallback"
    );
}

/// c728 C6: one-shot `jet dev` reports E2211 and exits 101 for a JIT gap.
#[test]
fn one_shot_dev_reports_e2211_and_exits_101() {
    let dir = common::unique_tmp("jet_dev_one_shot_e2211");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("gap.jet");
    fs::write(
        &file,
        "use core.env as env\nfn run() {\n    print(env.current_dir())\n}\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["dev", file.to_str().unwrap(), "--watch=off"])
        .env("NO_COLOR", "1")
        .output()
        .expect("one-shot jet dev");
    assert_eq!(
        output.status.code(),
        Some(101),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("E2211"), "{combined}");
    let _ = fs::remove_dir_all(&dir);
}

/// c728 C6: watching `jet dev` reports E2211 on a gap edit and accepts a later valid edit.
#[test]
fn watching_dev_reports_e2211_and_recovers() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    use std::io::Read;

    fn read_until(
        reader: &mut impl Read,
        needle: &str,
        timeout: Duration,
        label: &str,
    ) -> String {
        let mut buf = String::new();
        let mut chunk = [0u8; 512];
        let start = Instant::now();
        while start.elapsed() < timeout {
            match reader.read(&mut chunk) {
                Ok(0) => std::thread::sleep(Duration::from_millis(20)),
                Ok(n) => {
                    buf.push_str(&String::from_utf8_lossy(&chunk[..n]));
                    if buf.contains(needle) {
                        return buf;
                    }
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        panic!("timed out waiting for {label}; got {buf}");
    }

    let dir = common::unique_tmp("jet_watch_e2211");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("watch_gap.jet");
    fs::write(&file, "fn run() {\n    print(\"ok1\")\n}\n").unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["dev", file.to_str().unwrap()])
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn watching jet dev");
    let mut stdout = child.stdout.take().expect("stdout pipe");
    let mut stderr = child.stderr.take().expect("stderr pipe");

    let _ = read_until(&mut stdout, "ok1", Duration::from_secs(20), "first good run");

    fs::write(
        &file,
        "use core.env as env\nfn run() {\n    print(env.current_dir())\n}\n",
    )
    .unwrap();
    let gap_out = read_until(
        &mut stderr,
        "E2211",
        Duration::from_secs(20),
        "JIT gap after bad edit",
    );
    assert!(gap_out.contains("JIT gap in run:"), "{gap_out}");

    fs::write(&file, "fn run() {\n    print(\"ok2\")\n}\n").unwrap();
    let _ = read_until(&mut stdout, "ok2", Duration::from_secs(20), "recovered good run");

    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_dir_all(&dir);
}

/// D-AUTH-TOKENPOLICY1=A: auth verification stays on the native path; strict JIT
/// reports E2211 instead of transparent AOT fallback.
#[test]
fn dev_default_reports_jit_gap_for_auth_verification() {
    let _guard = FfiBridgeLock::acquire();
    let dir = std::env::temp_dir().join(format!("jet_dev_auth_gap_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("auth_fallback.jet");
    fs::write(
        &file,
        r#"use core.auth as auth

fn run() {
    key: [U8] :: [48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102]
    token := "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJpc3MiOiJwYXJ0bmVyIiwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3MDAwMDAwMDB9.3gbnbn_u-GjiQuGusiLrnMUzlo5c9rPeqAO0iWZxhrY"
    claims :: auth.verify_jwt(token, key: key, audience: "gateway") ?? panic("verification failed")
    print(claims.audience)
}
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();

    match dev_iteration_with_timeout("auth_fallback_boundary", &shown, true) {
        RunOutcome::Problems(diags) => assert!(diags.iter().any(|d| d.code == "E2201"), "{diags:?}"),
        RunOutcome::Ran { .. } => panic!("interpreter unexpectedly ran core.auth"),
    }

    match dev_iteration_with_timeout("auth_fallback", &shown, false) {
        RunOutcome::Problems(diags) => {
            assert!(diags.iter().any(|d| d.code == "E2211"), "{diags:?}");
        }
        RunOutcome::Ran { .. } => panic!("default dev must not AOT-fallback-run core.auth"),
    }

    let expected = compiled_binary_output(&dir, "auth_fallback", 0, "auth_fallback", &shown);
    assert_eq!(expected.stdout.trim(), "gateway");
    let _ = fs::remove_dir_all(&dir);
}

/// D-SHAPE-PLACE1=A (#613): safe structural splitting follows the sema-proved
/// place identity and live range, not adjacent AST bindings. AOT and default
/// dev must preserve intervening effects and nested-field owner identity.
#[test]
fn place_split_planner_preserves_order_and_nested_owners() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping place split AOT/dev regression");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_place_split_planner_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("place_split.jet");
    fs::write(
        &file,
        r#"struct Holder { values: [Int] }
fn run() {
    values := [1, 2, 3, 4]
    first :: &values[0]
    print("between root")
    last :: &values[3]
    first = 8
    last = 9
    print("root: {first},{last}")

    adjacent := Holder.{ values: [10, 11, 12, 13] }
    adjacent_first :: &adjacent.values[0..1]
    adjacent_last :: &adjacent.values[2..3]
    adjacent_first[0] = 18
    adjacent_last[0] = 19
    print("nested adjacent: {adjacent_first[0]},{adjacent_last[0]}")

    interleaved := Holder.{ values: [20, 21, 22, 23] }
    interleaved_first :: &interleaved.values[0..1]
    print("between nested")
    interleaved_last :: &interleaved.values[2..3]
    interleaved_first[0] = 28
    interleaved_last[0] = 29
    print("nested interleaved: {interleaved_first[0]},{interleaved_last[0]}")

    reused := [30, 31]
    reused_first :: &reused[0]
    reused_bridge :: &reused[1]
    print("reuse first: {reused_first}")
    reused_again :: &reused[0]
    reused_bridge = 38
    reused_again = 39
    print("reuse final: {reused_again},{reused_bridge}")

    replaced := [40, 41]
    before_replace :: &replaced[0]
    print("before replace: {before_replace}")
    replaced = [42, 43]
    after_replace :: &replaced[1]
    #DebugOnly { print("unrelated debug") }
    after_replace = 49
    print("after replace: {after_replace}")
}
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let expected = compiled_binary_output(&dir, "place_split", 0, "place_split", &shown);
    assert_eq!(
        expected.stdout,
        "between root\nroot: 8,9\nnested adjacent: 18,19\nbetween nested\nnested interleaved: 28,29\nreuse first: 30\nreuse final: 39,38\nbefore replace: 40\nunrelated debug\nafter replace: 49\n"
    );
    assert_default_dev_jit_gap("place_split", &shown);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn returned_parameter_view_matches_aot_and_default_dev() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping returned-view AOT/dev regression");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_returned_parameter_view_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("returned_parameter_view.jet");
    fs::write(
        &file,
        r#"fn first(left: [Int], right: [Int]) -> View<Int> {
    return left[0..1]
}

fn run() {
    left := [7, 8]
    right := [9, 10]
    result :: first(left, right)
    print(result[0])
}
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let aot = compiled_binary_output(
        &dir,
        "returned_parameter_view",
        0,
        "returned_parameter_view",
        &shown,
    );
    assert_eq!(aot.stdout, "7\n");
    assert_default_dev_jit_gap("returned_parameter_view", &shown);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn returned_view_field_matches_aot_and_default_dev() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping returned-view-field AOT/dev regression");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_returned_view_field_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("returned_view_field.jet");
    fs::write(
        &file,
        r#"struct Window { values: View<Int> }

fn window(values: [Int]) -> Window {
    selected :: values[0..1]
    return Window.{ values: selected }
}

fn run() {
    values := [7, 8]
    result :: window(values)
    print(result.values[0])
}
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let aot = compiled_binary_output(
        &dir,
        "returned_view_field",
        0,
        "returned_view_field",
        &shown,
    );
    assert_eq!(aot.stdout, "7\n");
    assert_default_dev_jit_gap("returned_view_field", &shown);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn nested_returned_view_field_matches_aot_and_default_dev() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping nested returned-view AOT/dev regression");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_nested_returned_view_field_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("nested_returned_view_field.jet");
    fs::write(
        &file,
        r#"struct Inner { values: View<Int> }
struct Outer { inner: Inner }

fn outer(values: [Int]) -> Outer {
    selected :: values[0..1]
    return Outer.{ inner: Inner.{ values: selected } }
}

fn run() {
    values := [7, 8]
    result :: outer(values)
    print(result.inner.values[0])
}
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let aot = compiled_binary_output(
        &dir,
        "nested_returned_view_field",
        0,
        "nested_returned_view_field",
        &shown,
    );
    assert_eq!(aot.stdout, "7\n");
    assert_default_dev_jit_gap("nested_returned_view_field", &shown);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn wrapped_returned_view_fields_match_aot_and_default_dev() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping wrapped returned-view AOT/dev regression");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_wrapped_returned_view_fields_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("wrapped_returned_view_fields.jet");
    fs::write(
        &file,
        r#"struct Window { values: View<Int> }
struct Holder { maybe: Window? }
struct GenericHolder<T> { value: T, maybe: Window? }
struct Node { next: Node?, values: View<Int> }

fn maybe(values: [Int]) -> (Window?) {
    selected :: values[0..1]
    return Val(Window.{ values: selected })
}

fn result(values: [Int]) -> Window ? String {
    selected :: values[0..1]
    return Ok(Window.{ values: selected })
}

fn tuple(values: [Int]) -> (window: Window, count: Int) {
    selected :: values[0..1]
    return (window: Window.{ values: selected }, count: 1)
}

fn node(values: [Int]) -> Node {
    selected :: values[0..1]
    return Node.{ next: None, values: selected }
}

fn run() { print(0) }
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let aot = compiled_binary_output(
        &dir,
        "wrapped_returned_view_fields",
        0,
        "wrapped_returned_view_fields",
        &shown,
    );
    assert_eq!(aot.stdout, "0\n");
    assert_default_dev_jit_gap("wrapped_returned_view_fields", &shown);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn returned_string_view_field_matches_aot_and_default_dev() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping returned-string-view AOT/dev regression");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_returned_string_view_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("returned_string_view.jet");
    fs::write(
        &file,
        r#"struct Domain { value: View<str> }

fn domain(email: String) -> Domain {
    result :: email.after("@")
    return Domain.{ value: result }
}

fn run() {
    email := "user@example.com"
    result :: domain(email)
    print(result.value)
}
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let aot = compiled_binary_output(
        &dir,
        "returned_string_view",
        0,
        "returned_string_view",
        &shown,
    );
    assert_eq!(aot.stdout, "example.com\n");
    assert_default_dev_jit_gap("returned_string_view", &shown);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn returned_view_trait_method_matches_aot_and_default_dev() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping returned-view-trait AOT/dev regression");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_returned_view_trait_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("returned_view_trait.jet");
    fs::write(
        &file,
        r#"trait Select {
    fn select(self, left: [Int], right: [Int]) -> View<Int>
}

struct First { marker: Int }
impl First.Select {
    fn select(self, left: [Int], right: [Int]) -> View<Int> {
        return left[0..1]
    }
}

fn wrapper(selector: First, left: [Int], right: [Int]) -> View<Int> {
    return selector.select(left, right)
}

fn run() {
    selector :: First.{ marker: 0 }
    left := [7, 8]
    right := [9, 10]
    result :: wrapper(selector, left, right)
    print(result[0])
}
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let aot = compiled_binary_output(
        &dir,
        "returned_view_trait",
        0,
        "returned_view_trait",
        &shown,
    );
    assert_eq!(aot.stdout, "7\n");
    assert_default_dev_jit_gap("returned_view_trait", &shown);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn aggregate_trait_returns_match_aot_and_default_dev_in_both_impl_orders() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping aggregate trait-return AOT/dev regression");
        return;
    }
    let template = r#"struct Pair { left: View<Int>, right: View<Int> }
struct Envelope<T> { value: T, marker: Int }

trait Select {
    fn select(self, left: [Int], right: [Int]) -> Pair
    fn optional(self, left: [Int], right: [Int]) -> (Pair?)
    fn fallible(self, left: [Int], right: [Int]) -> Pair ? String
    fn tupled(self, left: [Int], right: [Int]) -> (pair: Pair, count: Int)
    fn generic(self, left: [Int], right: [Int]) -> Envelope<Pair>
}

fn wrapper(selector: First, left: [Int], right: [Int]) -> Pair {
    return selector.select(left, right)
}

$IMPLS

fn run() {
    left := [7, 8]
    right := [9, 10]
    pair :: wrapper(First.{ marker: 0 }, left, right)
    print(pair.left[0])
    print(pair.right[0])
}
"#;
    let implementation = |name: &str| {
        r#"struct $TYPE { marker: Int }
impl $TYPE.Select {
    fn select(self, left: [Int], right: [Int]) -> Pair {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return Pair.{ left: left_view, right: right_view }
    }
    fn optional(self, left: [Int], right: [Int]) -> (Pair?) {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return Val(Pair.{ left: left_view, right: right_view })
    }
    fn fallible(self, left: [Int], right: [Int]) -> Pair ? String {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return Ok(Pair.{ left: left_view, right: right_view })
    }
    fn tupled(self, left: [Int], right: [Int]) -> (pair: Pair, count: Int) {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return (pair: Pair.{ left: left_view, right: right_view }, count: 1)
    }
    fn generic(self, left: [Int], right: [Int]) -> Envelope<Pair> {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return Envelope<Pair>.{
            value: Pair.{ left: left_view, right: right_view },
            marker: 0,
        }
    }
}
"#
        .replace("$TYPE", name)
    };
    let first = implementation("First");
    let last = implementation("Last");
    for (index, implementations) in [
        format!("{first}{last}"),
        format!("{last}{first}"),
    ]
    .into_iter()
    .enumerate()
    {
        let dir = std::env::temp_dir().join(format!(
            "jet_aggregate_trait_returns_{}_{}",
            std::process::id(),
            index
        ));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("aggregate_trait_returns.jet");
        fs::write(&file, template.replace("$IMPLS", &implementations)).unwrap();
        let shown = file.to_string_lossy().to_string();
        let stem = format!("aggregate_trait_returns_{index}");
        let aot = compiled_binary_output(&dir, &stem, 0, &stem, &shown);
        assert_eq!(aot.stdout, "7\n9\n");
        assert_default_dev_jit_gap(&stem, &shown);
        let _ = fs::remove_dir_all(&dir);
    }
}

#[test]
fn json_coerce_audit_reports_jit_gap_on_default_dev() {
    let dir = std::env::temp_dir().join(format!(
        "jet_dev_json_coerce_fallback_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let stem = "serde/json_coerce";
    let file = example_path(stem);

    match dev_iteration_with_timeout(stem, &file, true) {
        RunOutcome::Problems(diags) => assert!(
            diags.iter().any(|d| d.code == "E2201"),
            "interpreter must name the coercion-audit boundary: {diags:?}"
        ),
        RunOutcome::Ran { .. } => {
            panic!("interpreter dropped the coercion audit effect instead of deferring to native")
        }
    }

    assert_default_dev_jit_gap(stem, &file);
    let expected = normalize_for_parity(
        stem,
        compiled_binary_output(&dir, "json_coerce_aot", 0, stem, &file),
    );
    assert_eq!(expected.stdout, "8081\napi\ntrue\n");
    assert_eq!(expected.exit_code, 0);
}

#[cfg(unix)]
#[test]
fn dev_default_socket_echo_reports_jit_gap() {
    let file = "examples/features/net/socket_echo.jet";
    assert_default_dev_jit_gap("net/socket_echo", file);
}

#[test]
fn dev_default_tls_deadline_reports_jit_gap() {
    let dir = std::env::temp_dir().join(format!(
        "jet_dev_tls_deadline_parity_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    // Accept with a deadline: default-dev may report E2211 without connecting, so
    // blocking forever on a second accept hangs the whole --test-threads=1 suite.
    let server = std::thread::spawn(move || {
        let _ = listener.set_nonblocking(true);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let mut peers = Vec::new();
        while peers.len() < 2 && std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((peer, _)) => peers.push(peer),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    });
    let file = dir.join("tls_deadline.jet");
    fs::write(
        &file,
        format!(
            r#"use core.net as net
use core.tls as tls

fn run() {{
    tcp := net.tcp_connect("{address}") ?? panic("tcp")
    net.set_timeout(&tcp, 25) ?? panic("timeout")
    if tls.client(^tcp, "localhost") == {{
        Ok(_) -> panic("stalled handshake succeeded")
        Err(error) -> print(net.error_message(error))
    }}
}}
"#
        ),
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let _ = compiled_binary_output(&dir, "tls_deadline", 0, "tls_deadline", &shown);
    assert_default_dev_jit_gap("tls_deadline", &shown);
    server.join().unwrap();
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn dev_default_io_log_reports_jit_gap() {
    let file = "examples/features/io/log.jet";
    assert_default_dev_jit_gap("io/log", file);
}

#[test]
fn dev_default_resident_boundaries_report_jit_gap() {
    for stem in [
        "concurrency/task_controls",
        "memory/entity_tree",
        "memory/expiring_secret",
    ] {
        let file = example_path(stem);
        assert_default_dev_jit_gap(stem, &file);
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
    assert_default_dev_jit_gap("types/display_debug", file);
    let got = match dev_iteration(file, false, true) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => {
            panic!("display/debug interpolation should run in interpreter dev, got: {ds:?}")
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

    let mut backend = CraneliftBackend::new();
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
    let mut backend = CraneliftBackend::new();
    let got = match backend.run(&bundle, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => panic!("cranelift backend did not run `{tag}`: {ds:?}"),
    };
    assert_eq!(
        got, expected,
        "cranelift output drifted from interpreter for `{tag}`"
    );
}

fn assert_cranelift_reports_jit_gap(src: &str, tag: &str) {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let p = std::env::temp_dir().join(format!("jet_jit_gap_{tag}.jet"));
    fs::write(&p, src).unwrap();
    let shown = p.to_string_lossy().to_string();
    let bundle = checked_bundle_from_path(&shown);
    let mut backend = CraneliftBackend::new();
    match backend.run(&bundle, false) {
        RunOutcome::Problems(ds) => {
            assert!(
                jet_jit::is_e2211(&ds),
                "strict cranelift should report E2211 for `{tag}`, got {ds:?}"
            );
        }
        RunOutcome::Ran { .. } => panic!("strict cranelift unexpectedly ran `{tag}`"),
    }
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
    let mut backend = CraneliftBackend::new();
    match backend.run(&bundle, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(ds) => panic!("`{tag}` JIT returned diagnostics: {ds:?}"),
    }
}

fn run_cranelift_outcome_without_fallback(src: &str, tag: &str) -> RunOutcome {
    let p = std::env::temp_dir().join(format!("jet_jit_no_fallback_{tag}.jet"));
    fs::create_dir_all(p.parent().unwrap()).unwrap();
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
    let mut backend = CraneliftBackend::new();
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
fn unicode_16_string_and_core_text_match_aot_comptime_and_resident_jit() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let upper_only_in_host_unicode_17 = char::from_u32(0xA7CE).unwrap();
    let lower_only_in_host_unicode_17 = char::from_u32(0xA7CF).unwrap();
    let whitespace = char::from_u32(0x2003).unwrap();
    let source = format!(
        r#"use core.text as text

fn run() {{
    print("{upper_only_in_host_unicode_17}".to_lower() == "{upper_only_in_host_unicode_17}")
    print("{lower_only_in_host_unicode_17}".to_upper() == "{lower_only_in_host_unicode_17}")
    print("{whitespace}jet{whitespace}".trim())
    print(text.lower("{upper_only_in_host_unicode_17}") == "{upper_only_in_host_unicode_17}")
    print(text.upper("{lower_only_in_host_unicode_17}") == "{lower_only_in_host_unicode_17}")
    print(text.trim("{whitespace}jet{whitespace}"))
}}
"#,
    );
    let expected = "true\ntrue\njet\ntrue\ntrue\njet\n";

    let resident = run_cranelift_without_fallback(&source, "unicode_16_public_string");
    assert_eq!(resident.stdout, expected);

    let dir = common::unique_tmp("jet_unicode_16_public_string_aot");
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join("main.jet");
    let rust_path = dir.join("main.rs");
    let binary = dir.join("main");
    fs::write(&jet_path, &source).unwrap();
    let compiled = jet::compile_with_path(&source, &jet_path.to_string_lossy())
        .expect("Unicode-16 String fixture should compile");
    fs::write(&rust_path, compiled.rust).unwrap();
    let rustc = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rust_path)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "Unicode-16 AOT fixture rejected:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let aot = Command::new(&binary).output().unwrap();
    assert!(aot.status.success());
    assert_eq!(String::from_utf8_lossy(&aot.stdout), expected);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn unsupported_core_text_is_not_claimed_by_resident_jit() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let dir = common::unique_tmp("jet_unicode_16_jit_boundary");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(
        &path,
        "use core.text as text\nfn run() { print(text.casefold(\"Straße\")) }\n",
    )
    .unwrap();
    let bundle = checked_bundle_from_path(&path.to_string_lossy());
    assert!(!jet_jit::resident_jit_safe_bundle(&bundle));
    assert!(jet_jit::resident_jit_safe_bundle_detail(&bundle).contains("entry not resident-safe"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn bigint_equality_matches_aot_in_resident_and_default_dev() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let src = r#"
fn run() {
    left :: BigInt("-999999999999999999999999999999")
    same :: BigInt("-999999999999999999999999999999")
    other :: BigInt("999999999999999999999999999999")
    print(left == same)
    print(left != same)
    print(left != other)
}
"#;
    let resident = run_cranelift_without_fallback(src, "bigint_value_equality");

    let dir = std::env::temp_dir().join(format!("jet_bigint_equality_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("bigint_value_equality.jet");
    fs::write(&file, src).unwrap();
    let shown = file.to_string_lossy().to_string();
    let default = match dev_iteration(&shown, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("default dev failed BigInt equality: {diags:?}"),
    };
    let aot = compiled_binary_output(
        &dir,
        "bigint_value_equality",
        0,
        "bigint_value_equality",
        &shown,
    );
    let expected = ProgramOutput::ran("true\nfalse\ntrue\n".to_string(), String::new(), 0);
    assert_eq!(resident, expected);
    assert_eq!(default, expected);
    assert_eq!(aot, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bigint_example_matches_interpreter_resident_jit_default_dev_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = dev_diff_lock().lock().unwrap();
    let file = "examples/features/text/bigint.jet";
    let expected = ProgramOutput::ran(golden_stdout("text/bigint"), String::new(), 0);

    let interpreted = match dev_iteration(file, false, true) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            ProgramOutput::ran(stdout, stderr, exit_code)
        }
        RunOutcome::Problems(diags) => {
            panic!("BigInt example must execute in interpreter tier: {diags:?}")
        }
    };

    let source = fs::read_to_string(file).unwrap();
    let resident = run_cranelift_without_fallback(&source, "bigint_example");
    let default = match dev_iteration(file, false, false) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            ProgramOutput::ran(stdout, stderr, exit_code)
        }
        RunOutcome::Problems(diags) => panic!("default dev failed BigInt example: {diags:?}"),
    };

    let dir = std::env::temp_dir().join(format!("jet_bigint_example_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let aot = compiled_binary_output(&dir, "bigint_example", 0, "bigint_example", file);

    assert_eq!(interpreted, expected);
    assert_eq!(resident, expected);
    assert_eq!(default, expected);
    assert_eq!(aot, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unified_loop_jit_tiers_are_explicit_and_match_aot() {
    let counted = "fn run() {\n    loop i := 0; i < 4; i += 1 {\n        if i == 1 { next }\n        print(i)\n    }\n}\n";
    if !skip_if_cranelift_host_unsupported() {
        let native = run_cranelift_without_fallback(counted, "counted_next");
        assert_eq!(native.stdout, "0\n2\n3\n");
    }

    let stride = "fn run() {\n    xs := [0, 1, 2, 3, 4]\n    loop x; xs; 2 {\n        print(x)\n        if x == 0 { next }\n    }\n}\n";
    if !skip_if_cranelift_host_unsupported() {
        let native = run_cranelift_without_fallback(stride, "source_stride_next");
        assert_eq!(native.stdout, "0\n2\n4\n");

        let invalid = "fn run() {\n    xs := [1, 2]\n    stride := 0\n    loop x; xs; stride {\n        print(x)\n    }\n}\n";
        let RunOutcome::Problems(diags) =
            run_cranelift_outcome_without_fallback(invalid, "source_stride_pre_pull")
        else {
            panic!("invalid dynamic stride must stop before the first source pull")
        };
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, "E0123");
    }
}

#[test]
fn subjectless_guards_match_aot_in_resident_jit() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let src = r#"
fn run() {
    n :: 7
    if n > 10 -> print("too big")
    if {
        n < 0 -> print("negative")
        n < 10 -> print("single digit")
    }
    label :: if {
        n < 5 -> "small"
        n < 10 -> "medium"
        else -> "large"
    }
    print(label)
}
"#;
    let jit = run_cranelift_without_fallback(src, "subjectless_guards");
    let dir = std::env::temp_dir().join(format!("jet_guard_jit_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("subjectless_guards.jet");
    fs::write(&file, src).unwrap();
    let aot = compiled_binary_output(
        &dir,
        "subjectless_guards",
        0,
        "subjectless_guards",
        file.to_str().unwrap(),
    );
    assert_eq!(jit, aot);
    assert_eq!(jit.stdout, "single digit\nmedium\n");
}

#[test]
fn resident_jit_numeric_methods_and_parse_are_native() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let src = r#"
fn run() {
    Int.parse("41").drop("native parse proof")
    n :: 41
    print(Float.from_int(n))
    print(n.count_ones())
    print(1.0.is_finite())
    print(n.to_string())
}
"#;
    assert_eq!(
        run_cranelift_without_fallback(src, "numeric_parse"),
        ProgramOutput::ran("41.0\n3\ntrue\n41\n".into(), "".into(), 0)
    );
}

#[test]
fn forced_interpreter_preserves_f32_width_like_aot() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping F32 dev differential");
        return;
    }
    let source = r#"fn pass(value: F32) -> F32 { return value }
fn run() {
    value: F32 :: 16777217.0
    one: F32 :: 1.0
    mutable: F32 := value
    mutable += one
    print(pass(value))
    print(mutable)
    print([value, mutable])
}"#;
    let dir = common::unique_tmp("jet_dev_f32_width");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("f32_width.jet");
    fs::write(&path, source).unwrap();
    let file = path.to_string_lossy();
    let expected = compiled_binary_output(&dir, "f32_width", 0, "f32-width", &file);
    let actual = match dev_iteration(&file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("forced F32 interpreter failed: {diags:?}"),
    };
    assert_eq!(actual, expected);
    assert_eq!(
        actual.stdout,
        "16777216.0\n16777216.0\n[16777216.0, 16777216.0]\n"
    );
}

#[test]
fn gzip_golden_matches_forced_interpreter_and_aot() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping gzip dev differential");
        return;
    }
    let source = r#"use core.compress.gzip as gzip

fn run() {
    bytes: [U8] :: [72, 101, 108, 108, 111]
    gz: [U8] :: gzip.decompress(gzip.compress(bytes)) ?? []
    golden: [U8] :: gzip.decompress([31, 139, 8, 0, 0, 0, 0, 0, 2, 3, 203, 72, 205, 201, 201, 7, 0, 134, 166, 16, 54, 5, 0, 0, 0]) ?? []
    bad_size: [U8] :: gzip.decompress([31, 139, 8, 0, 0, 0, 0, 0, 2, 3, 203, 72, 205, 201, 201, 7, 0, 134, 166, 16, 54, 6, 0, 0, 0]) ?? [255]
    h: U8 :: 72
    lower_h: U8 :: 104
    o: U8 :: 111
    max: U8 :: 255
    print(gz.len() == 5)
    print(gz[0] == h)
    print(golden.len() == 5)
    print(golden[0] == lower_h)
    print(golden[4] == o)
    print(bad_size[0] == max)
}
"#;
    let dir = common::unique_tmp("jet_dev_compression");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("compression.jet");
    fs::write(&path, source).unwrap();
    let file = path.to_string_lossy();
    let expected = compiled_binary_output(&dir, "compression", 0, "compression", &file);
    let actual = match dev_iteration(&file, false, true) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            ProgramOutput::ran(stdout, stderr, exit_code)
        }
        RunOutcome::Problems(diags) => panic!("forced compression interpreter failed: {diags:?}"),
    };
    assert_eq!(actual, expected);
    assert_eq!(actual.stdout, "true\ntrue\ntrue\ntrue\ntrue\ntrue\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn zstd_compress_runs_in_forced_interpreter_with_aot_wire_shape() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping zstd dev differential");
        return;
    }
    let source = r#"use core.compress.zstd as zstd

fn run() {
    frame :: zstd.compress([72, 101, 108, 108, 111])
    m0: U8 :: 40
    m1: U8 :: 181
    m2: U8 :: 47
    m3: U8 :: 253
    print(frame.len() > 9)
    print(frame[0] == m0 && frame[1] == m1 && frame[2] == m2 && frame[3] == m3)
}
"#;
    let dir = common::unique_tmp("jet_dev_zstd_compress");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("zstd_compress.jet");
    fs::write(&path, source).unwrap();
    let file = path.to_string_lossy();
    let expected = compiled_binary_output(&dir, "zstd_compress", 0, "zstd-compress", &file);
    let actual = match dev_iteration(&file, false, true) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            ProgramOutput::ran(stdout, stderr, exit_code)
        }
        RunOutcome::Problems(diags) => panic!("forced zstd compressor failed: {diags:?}"),
    };
    assert_eq!(actual, expected);
    assert_eq!(actual.stdout, "true\ntrue\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn resident_jit_checked_numeric_and_distinct_conversion_matrix_is_native() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let src = r#"
#Numeric UserId :: distinct Int
#Numeric Severity :: distinct Int(0..10)
#UnitFamily(Currency) { usd }

fn run() {
    print(I64.from_u8(255))
    byte_ok: I32 :: 100
    byte_bad: I32 :: 100000
    U8.from_i32(byte_ok).drop("checked conversion success proof")
    U8.from_i32(byte_bad).drop("checked conversion error proof")
    float_ok: Float :: 42.9
    float_bad: Float :: 300.0
    U8.from_float(float_ok).drop("checked float conversion success proof")
    U8.from_float(float_bad).drop("checked float conversion error proof")
    narrow_ok: Float :: 2.5
    narrow_bad: Float :: 1e100
    F32.from_float(narrow_ok).drop("checked F32 conversion success proof")
    F32.from_float(narrow_bad).drop("checked F32 conversion error proof")
    user_source :: U64.from_u8(8)
    UserId.from_u64(user_source).drop("checked distinct conversion proof")
    print(UserId.from_u8(8).raw())
    print(Severity.from_u8(8).raw())
    severity_source :: 7
    Severity.from_int(severity_source).drop("checked range conversion proof")
    print(Severity.from_int(7).raw())
    print(Usd.from_int(5).raw())
}
"#;
    assert_eq!(
        run_cranelift_without_fallback(src, "checked_numeric_distinct_matrix"),
        ProgramOutput::ran(
            "255\n8\n8\n7\n5.0\n".into(),
            "".into(),
            0,
        )
    );
}

#[test]
fn physical_quantities_run_in_resident_jit_without_fallback() {
    if skip_if_cranelift_host_unsupported() { return; }
    let out = run_cranelift_without_fallback(r#"
#UnitFamily(Length, base: meter) {
    meter
    millimeter(scale: 1/1000)
    thirdish(scale: 2/3)
}
#UnitFamily(Time) { second }
fn run() -> Void ? {
    distance :: 12meter
    elapsed :: 3second
    speed :: distance / elapsed
    recovered :: speed * elapsed
    ratio :: recovered / distance
    print(ratio)
    exact :: Meter.from_millimeter(3000millimeter)?
    rounded :: Meter.from_thirdish_rounded(1thirdish, .NearestEven, digits: 0)?
    print("{(exact.raw())} {(rounded.raw())}")
}
"#, "physical_quantity");
    assert_eq!(out.stdout, "1.0\n3.0 1.0\n");

    let failed = run_cranelift_without_fallback(r#"
#UnitFamily(Length, base: meter) {
    meter
    thirdish(scale: 2/3)
}
fn run() -> Void ? {
    Meter.from_thirdish(1thirdish)?
}
"#, "physical_quantity_inexact");
    assert_eq!(
        failed,
        ProgramOutput::ran("".into(), "unit conversion would round\n".into(), 1)
    );

    let beyond_f64 = run_cranelift_without_fallback(r#"
#UnitFamily(Length, base: meter) {
    meter
    almost(scale: 9007199254740993/9007199254740992)
}
fn run() -> Void ? {
    Meter.from_almost(1almost)?
}
"#, "physical_quantity_exact_rational_edge");
    assert_eq!(
        beyond_f64,
        ProgramOutput::ran("".into(), "unit conversion would round\n".into(), 1)
    );

    let rational_edges = run_cranelift_without_fallback(r#"
#UnitFamily(Length, base: meter) {
    meter
    almost(scale: 9007199254740993/9007199254740992)
    half(scale: 1/2)
    above_half(scale: 9007199254740993/18014398509481984)
    three_halves(scale: 3/2)
}
#UnitFamily(Temperature, base: kelvin) {
    kelvin
    tie_offset(scale: 1, offset: 1/2)
    above_offset(scale: 1, offset: 9007199254740993/18014398509481984)
    below_offset(scale: 1, offset: -9007199254740993/18014398509481984)
}
fn run() -> Void ? {
    tie :: Meter.from_half_rounded(1half, .NearestEven, digits: 0)?
    above :: Meter.from_above_half_rounded(1above_half, .NearestEven, digits: 0)?
    negative_source :: ThreeHalves.from_float(-1.0)
    negative :: Meter.from_three_halves_rounded(negative_source, .NearestEven, digits: 0)?
    tie_point :: TieOffsetPoint.from_float(0.0)
    above_point :: AboveOffsetPoint.from_float(0.0)
    below_point :: BelowOffsetPoint.from_float(0.0)
    affine_tie :: KelvinPoint.from_tie_offset_point_rounded(tie_point, .NearestEven, digits: 0)?
    affine_above :: KelvinPoint.from_above_offset_point_rounded(above_point, .NearestEven, digits: 0)?
    affine_below :: KelvinPoint.from_below_offset_point_rounded(below_point, .NearestEven, digits: 0)?
    print("{(tie.raw())} {(above.raw())} {(negative.raw())} {(affine_tie.raw())} {(affine_above.raw())} {(affine_below.raw())}")
}
"#, "physical_quantity_rational_edges");
    assert_eq!(
        rational_edges,
        ProgramOutput::ran("0.0 1.0 -2.0 0.0 1.0 -1.0\n".into(), "".into(), 0)
    );

    let overflow = r#"
#UnitFamily(Length, base: meter) { meter double(scale: 2) }
fn run() -> Void ? {
    source :: Double.from_float(1.7976931348623157e308)
    Meter.from_double_rounded(source, .NearestEven, digits: 0)?
}
"#;
    assert_eq!(
        run_cranelift_without_fallback(overflow, "physical_quantity_rounded_overflow"),
        ProgramOutput::ran(
            "".into(),
            "unit conversion overflows its runtime representation\n".into(),
            1,
        )
    );
}

#[test]
fn rounded_physical_quantities_match_resident_default_dev_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let src = r#"
#UnitFamily(Length, base: meter) {
    meter
    half(scale: 1/2)
    near_quarter(scale: 249/1000)
    near_three_quarters(scale: 751/1000)
}
#UnitFamily(Temperature, base: kelvin) {
    kelvin
    shifted(scale: 1, offset: 249/1000)
}
fn run() -> Void ? {
    positive :: Half.from_float(5.0)
    negative :: Half.from_float(-5.0)
    toward_zero :: Meter.from_half_rounded(positive, .TowardZero, digits: 0)?
    floor :: Meter.from_half_rounded(negative, .Floor, digits: 0)?
    ceiling :: Meter.from_half_rounded(positive, .Ceiling, digits: 0)?
    nearest_even :: Meter.from_near_quarter_rounded(1near_quarter, .NearestEven, digits: 2)?
    nearest_odd :: Meter.from_near_three_quarters_rounded(1near_three_quarters, .NearestEven, digits: 2)?
    point :: KelvinPoint.from_shifted_point_rounded(ShiftedPoint.from_float(0.0), .Ceiling, digits: 2)?
    delta :: KelvinDelta.from_shifted_delta_rounded(ShiftedDelta.from_float(0.0), .Ceiling, digits: 2)?
    print("{(toward_zero.raw())} {(floor.raw())} {(ceiling.raw())} {(nearest_even.raw())} {(nearest_odd.raw())} {(point.raw())} {(delta.raw())}")
}
"#;
    let expected = ProgramOutput::ran("2.0 -3.0 3.0 0.25 0.75 0.25 0.0\n".into(), "".into(), 0);
    let resident = run_cranelift_without_fallback(src, "rounded_quantity_parity");

    let dir = std::env::temp_dir().join(format!(
        "jet_rounded_quantity_parity_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("rounded_quantity_parity.jet");
    fs::write(&file, src).unwrap();
    let shown = file.to_string_lossy().to_string();
    let default = match dev_iteration(&shown, false, false) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            ProgramOutput::ran(stdout, stderr, exit_code)
        }
        RunOutcome::Problems(diags) => panic!("default dev failed rounded quantity parity: {diags:?}"),
    };
    let aot = compiled_binary_output(&dir, "rounded_quantity_parity", 0, "rounded_quantity_parity", &shown);
    assert_eq!(resident, expected);
    assert_eq!(default, expected);
    assert_eq!(aot, expected);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn generic_module_instance_runs_identically_in_resident_jit_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() { return; }
    let src = r#"
module value<n: Int> { pub fn get() -> Int { return n } }
module three = value<3>
module same = value<3>
fn run() { print(three.get()); print(same.get()) }
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
    assert_eq!(tir.funcs.iter().filter(|f| f.name == "three__get").count(), 1);
}

#[test]
fn generic_user_derive_multi_instantiation_matches_resident_default_dev_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let src = r#"
derive T.Access {
    info :: T.reflect()
    name :: info.name
    param :: info.type_params[0].name
    emit("impl $name {{ fn make(value: ^$param) -> $name<$param> {{ return $name<$param>.{{ value: value }} }} fn marker() -> Int {{ return 17 }} fn get_value(self) -> $param {{ return ~self.value }} fn type_name(self) -> String {{ return \"$name\" }} }}")
}

derive T.NumericAccess {
    info :: T.reflect()
    name :: info.name
    param :: info.type_params[0].name
    emit("""
impl $name {{
    fn replace(&self, value: ^$param) -> $param {{
        self.value = value
        return ~self.value
    }}
    fn plus(self, rhs: $param) -> $param {{ return self.value + rhs }}
    fn equal_to(self, rhs: $param) -> Bool {{ return self.value == rhs }}
}}
""")
}

#Access
struct Box<T: Printable> { value: T }
#Access
struct StaticOnly<T: Printable> { value: T }
struct Wrapper<U: Printable> { boxed: Box<U> }
#NumericAccess
struct NumericBox<T: [Printable, Add, Equatable]> { value: T }

fn run() {
    number := Box<Int>.make(7)
    decimal := Box<Float>.{ value: 2.5 }
    flag := Box<Bool>.{ value: true }
    letter := Box<Char>.{ value: 'J' }
    text := Box<String>.make("jet")
    numeric := NumericBox<Float>.{ value: 1.5 }
    print(number.get_value())
    print(decimal.get_value())
    print(flag.get_value())
    print(letter.get_value())
    print(text.get_value())
    print(text.get_value())
    print(number.type_name())
    print(numeric.replace(4.5))
    print(numeric.plus(0.5))
    print(numeric.equal_to(4.5))
    print(StaticOnly<Int>.marker())
    print(StaticOnly<String>.marker())
}
"#;
    let expected = ProgramOutput::ran(
        "7\n2.5\ntrue\nJ\njet\njet\nBox\n4.5\n5.0\ntrue\n17\n17\n".into(),
        "".into(),
        0,
    );
    let dir = std::env::temp_dir().join(format!(
        "jet_generic_user_derive_jit_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("generic_user_derive.jet");
    fs::write(&file, src).unwrap();
    let shown = file.to_string_lossy().to_string();
    let bundle = checked_bundle_from_path(&shown);
    let run_func = bundle.modules[bundle.entry]
        .items
        .iter()
        .find_map(|item| match item {
            jet::AST::Item::Func(func) if func.name == "run" => Some(func),
            _ => None,
        })
        .expect("run function");
    let numeric_binding = run_func
        .body
        .iter()
        .find_map(|stmt| match stmt {
            jet::AST::Stmt::Val(binding) if binding.name == "numeric" => Some(binding),
            _ => None,
        })
        .expect("numeric binding");
    assert_eq!(
        numeric_binding.ty,
        Some(jet::AST::Type::Apply {
            name: "NumericBox".to_string(),
            args: vec![jet::AST::Type::Float],
        }),
        "sema must retain the concrete generic binding identity"
    );
    let tir = jet::Codegen::TIR::lower_jit_program(&bundle)
        .expect("concrete generic derive lowers to resident JIT TIR");
    let numeric_init_ty = tir
        .funcs
        .iter()
        .find(|func| func.name == "run")
        .and_then(|func| {
            func.body.iter().find_map(|stmt| match stmt {
                jet::Codegen::TIR::TStmt::Let { name, init, .. } if name == "numeric" => {
                    Some(init.ty.clone())
                }
                _ => None,
            })
        })
        .expect("numeric TIR binding");
    assert_eq!(numeric_init_ty, numeric_binding.ty.clone().unwrap());
    assert!(tir.funcs.iter().any(|func| func.name == "Box<Int>::get_value"));
    assert!(tir.funcs.iter().any(|func| func.name == "Box<Float>::get_value"));
    assert!(tir.funcs.iter().any(|func| func.name == "Box<Bool>::get_value"));
    assert!(tir.funcs.iter().any(|func| func.name == "Box<Char>::get_value"));
    assert!(tir.funcs.iter().any(|func| func.name == "Box<String>::get_value"));
    assert!(tir.funcs.iter().any(|func| func.name == "Box<Int>::make"));
    assert!(tir.funcs.iter().any(|func| func.name == "Box<String>::make"));
    assert!(tir.funcs.iter().any(|func| func.name == "StaticOnly<Int>::marker"));
    assert!(tir.funcs.iter().any(|func| func.name == "StaticOnly<String>::marker"));
    assert!(tir.funcs.iter().any(|func| func.name == "NumericBox<Float>::replace"));
    assert!(tir.funcs.iter().any(|func| func.name == "NumericBox<Float>::plus"));
    assert!(tir.funcs.iter().any(|func| func.name == "NumericBox<Float>::equal_to"));
    assert!(
        tir.funcs.iter().all(|func| {
            !func.name.starts_with("Box<T>::") && !func.name.starts_with("Box<U>::")
        }),
        "abstract field types must not become fake JIT instances: {:?}",
        tir.funcs.iter().map(|func| &func.name).collect::<Vec<_>>()
    );
    let generic_method_file = dir.join("generic_method_shadow.jet");
    fs::write(
        &generic_method_file,
        r#"
derive T.GenericMethod {
    info :: T.reflect()
    name :: info.name
    emit("impl $name {{ fn keep<T>(self, value: ^T) -> T {{ return value }} }}")
}
#GenericMethod
struct Shadow<T: Printable> { value: T }
fn run() {
    item := Shadow<Int>.{ value: 1 }
    print(item.value)
}
"#,
    )
    .unwrap();
    let generic_method_bundle =
        checked_bundle_from_path(generic_method_file.to_str().unwrap());
    let generic_method_tir = jet::Codegen::TIR::lower_jit_program(&generic_method_bundle)
        .expect("generic-method fixture lowers around the unsupported method");
    assert!(
        generic_method_tir
            .funcs
            .iter()
            .all(|func| !func.name.ends_with("::keep")),
        "method-owned generic T must not be captured by the owner's T substitution"
    );
    let resident = run_cranelift_without_fallback(src, "generic_user_derive");
    let default = match dev_iteration(&shown, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("default dev failed generic user derive: {diags:?}"),
    };
    let aot = compiled_binary_output(
        &dir,
        "generic_user_derive",
        0,
        "generic_user_derive",
        &shown,
    );
    assert_eq!(resident, expected);
    assert_eq!(default, expected);
    assert_eq!(aot, expected);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn nested_generic_user_derive_reaches_resident_jit() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let src = r#"
derive T.Access {
    info :: T.reflect()
    name :: info.name
    param :: info.type_params[0].name
    emit("impl $name {{ fn get_value(self) -> $param {{ return ~self.value }} }}")
}

#Access
struct Inner<T: Printable> { value: T }

struct Outer<T: Printable> {
    value: T

    fn read(self) -> T {
        inner := Inner<T>.{ value: ~self.value }
        return inner.get_value()
    }
}

fn run() {
    outer := Outer<Int>.{ value: 7 }
    print(outer.read())
}
"#;
    let output = run_cranelift_without_fallback(src, "nested_generic_user_derive");
    assert_eq!(output, ProgramOutput::ran("7\n".into(), "".into(), 0));
}

#[test]
fn unused_expanding_generic_body_does_not_expand_jit_worklist() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let src = r#"
struct Grow<T: Printable> {
    value: T

    fn read(self) -> T { return ~self.value }
    fn unused(self) -> Int {
        nested := Grow<[T]>.{ value: [~self.value] }
        return nested.unused()
    }
}

fn run() {
    value := Grow<Int>.{ value: 7 }
    print(value.read())
}
"#;
    let output = run_cranelift_without_fallback(src, "unused_expanding_generic_body");
    assert_eq!(output, ProgramOutput::ran("7\n".into(), "".into(), 0));

    let reachable = src.replace("print(value.read())", "print(value.unused())");
    let file = std::env::temp_dir().join("reachable_expanding_generic_method.jet");
    fs::write(&file, reachable).unwrap();
    let bundle = checked_bundle_from_path(file.to_str().unwrap());
    let error = jet_jit::try_compile_bundle(&bundle).unwrap_err();
    assert!(error.contains("E0909: generic instantiation goes too deep"), "{error}");
}

#[test]
fn nested_ordinary_module_generic_instance_matches_resident_jit_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() { return; }
    let src = r#"
module outer<T, n: Int> {
    module plain {
        module inner<U> { pub fn total(value: U) -> Int { return n } }
        module closed = inner<T>
        pub fn result(value: T) -> Int { return closed.total(value) }
    }
    pub fn result(value: T) -> Int { return plain.result(value) }
}
module selected = outer<Int, 6>
fn run() { print(selected.result(1)) }
"#;
    let jit = run_cranelift_without_fallback(src, "nested_ordinary_generic_module");
    assert_eq!(jit.stdout, "6\n");

    let dir = std::env::temp_dir().join(format!(
        "jet_nested_ordinary_generic_module_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("nested_ordinary_generic_module.jet");
    fs::write(&file, src).unwrap();
    let aot = compiled_binary_output(
        &dir,
        "nested_ordinary_generic_module",
        0,
        "nested_ordinary_generic_module",
        file.to_str().unwrap(),
    );
    assert_eq!(jit, aot);
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
    return Ok(0.25)
}

fn choose_err() -> Float ? String {
    return Err("typed boom")
}

fn forward() -> Float ? String {
    value :: choose_ok()?
    return Ok(value + 0.25)
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
    return Ok(7)
}

fn run() -> Void ? {
    print(direct_ok()?)
    stop :: false
    if stop {
        return Err("one-arm stopped")
    }
    print("one-arm fallthrough")
}
"#;
    let nested_fallthrough = r#"
fn direct_ok() -> Int ? {
    return Ok(7)
}

fn run() -> Void ? {
    print(direct_ok()?)
    outer :: true
    inner :: false
    if outer {
        if inner {
            return Err("nested stopped")
        }
    }
    print("nested fallthrough")
}
"#;
    let neither_arm_terminates = r#"
fn direct_ok() -> Int ? {
    return Ok(7)
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
    return Ok(7)
}

fn run() -> Void ? {
    print(direct_ok()?)
    if true {
        return Err("left branch")
    } else {
        return Err("right branch")
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
use core.perf as perf

fn run() -> Void ? {
    perf.reset_fidelity()
    print(perf.default_fidelity())
    perf.override_fidelity(0.25)?
    print(perf.fidelity())
    perf.reset_fidelity()
    print(perf.fidelity())
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
            r#"use core.perf as perf
fn run() -> Void ? {{
    perf.reset_fidelity()
    perf.override_fidelity(0.375)?
    perf.override_fidelity({value})?
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
        let read = r#"use core.perf as perf
fn run() { print(perf.fidelity()) }"#;
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

    let mut backend = CraneliftBackend::new();
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
fn array_of_structs_field_mutation_three_way() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let file = "examples/features/collections/struct_list_mutation.jet";
    let expected = golden_stdout("collections/struct_list_mutation");
    match dev_iteration(file, false, true) {
        RunOutcome::Ran { stdout, .. } => assert_eq!(stdout, expected),
        RunOutcome::Problems(ds) => {
            panic!("interpreter must run indexed struct-field mutation: {ds:?}")
        }
    }
    assert_cranelift_three_way(file, "collections/struct_list_mutation");
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
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let src = r#"
fn run() {
    outer :: loop i := 0; i < 2; i++ {
        loop {
            if i == 0 {
                outer.next()
            }
            outer.break()
        }
    }
    print("done")
}
"#;
    assert_eq!(
        run_cranelift_without_fallback(src, "labeled_loop_control"),
        ProgramOutput::ran("done\n".into(), "".into(), 0)
    );
}

#[test]
fn resident_jit_named_or_fallback_loop_control() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let src = r#"
fn run() {
    values := [7]
    outer :: loop i := 0; i < 2; i++ {
        loop {
            value :: values.get(1 - i) ?? outer.next()
            print(value)
            values.get(99) ?? outer.break()
        }
    }
    print("done")
}
"#;
    assert_eq!(
        run_cranelift_without_fallback(src, "named_or_fallback_loop_control"),
        ProgramOutput::ran("7\ndone\n".into(), "".into(), 0)
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
fn resident_jit_safe_user_operator_traits() {
    let file = "examples/features/operators/user_defined.jet";
    let mut bundle = jet::Loader::load_entry(file).expect("load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags.into_iter().filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error)).collect();
    assert!(errors.is_empty(), "user operator example must type-check: {errors:#?}");
    assert!(jet_jit::resident_jit_safe_bundle(&bundle), "user operators should stay JIT-covered: {}", jet_jit::resident_jit_safe_bundle_detail(&bundle));
    let src = fs::read_to_string(file).expect("operator example");
    let output = run_cranelift_without_fallback(&src, "user_operator_traits");
    assert_eq!(output, ProgramOutput::ran("4,6 4,6 true true false\n".into(), "".into(), 0));
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
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(cranelift_three_way_differential_battery_inner)
        .expect("three-way battery thread")
        .join()
        .expect("three-way battery thread panicked");
}

fn cranelift_three_way_differential_battery_inner() {
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

// ── c727: differential example-corpus gate (D-LENS-RUN1 / #688 C1) ─────────

/// Stable exclusion for examples outside the native JIT↔AOT oracle lens.
fn corpus_gate_exclusion(stem: &str) -> Option<&'static str> {
    match stem {
        "game/raylib_window" => Some("interactive display required"),
        "io/watcher" => Some("filesystem watch loop"),
        "lowlevel/cross" => Some("cross-target demo"),
        "net/http_server" | "net/http_server_lifecycle" | "net/http_server_middleware"
        | "net/http_server_tasks" | "net/http_server_trailers" | "net/socket_echo" => {
            Some("network service")
        }
        "ui/ui_native_linux" => Some("native GTK shell"),
        _ => None,
    }
}

fn example_has_err_golden(stem: &str) -> bool {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("examples/features/expected/{stem}.err.out"))
        .is_file()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum CorpusGateClass {
    FrontendRejected,
    GateExcluded,
    NonRunnable,
    ExpectedExit,
    ResidentJit,
    E2211,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CorpusGateRecord {
    stem: String,
    class: CorpusGateClass,
    detail: String,
}

fn classify_corpus_stem(
    stem: &str,
    dir: &std::path::Path,
    worker: usize,
    have_rustc: bool,
) -> CorpusGateRecord {
    let file = example_path(stem);
    if let Some(reason) = corpus_gate_exclusion(stem) {
        return CorpusGateRecord {
            stem: stem.to_string(),
            class: CorpusGateClass::GateExcluded,
            detail: reason.to_string(),
        };
    }

    let mut bundle = match jet::Loader::load_entry(&file) {
        Ok(b) => b,
        Err(diags) => {
            let detail = diags
                .first()
                .map(|d| format!("{}: {}", d.code, d.what))
                .unwrap_or_else(|| "load failed".to_string());
            return CorpusGateRecord {
                stem: stem.to_string(),
                class: CorpusGateClass::FrontendRejected,
                detail,
            };
        }
    };
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    if !errors.is_empty() {
        let detail = errors
            .iter()
            .map(|d| format!("{}: {}", d.code, d.what))
            .collect::<Vec<_>>()
            .join("; ");
        return CorpusGateRecord {
            stem: stem.to_string(),
            class: CorpusGateClass::FrontendRejected,
            detail,
        };
    }

    if !jet_jit::resident_jit_safe_bundle(&bundle)
        && jet_jit::try_compile_bundle(&bundle).is_err()
        && matches!(stem, "cli/subcommands" | "cli/typed_entry_args" | "cli/positionals")
    {
        return CorpusGateRecord {
            stem: stem.to_string(),
            class: CorpusGateClass::NonRunnable,
            detail: "no zero-parameter runnable entry".to_string(),
        };
    }

    if !have_rustc {
        return CorpusGateRecord {
            stem: stem.to_string(),
            class: CorpusGateClass::ExpectedExit,
            detail: "rustc unavailable; oracle skipped".to_string(),
        };
    }

    if example_has_err_golden(stem) {
        return CorpusGateRecord {
            stem: stem.to_string(),
            class: CorpusGateClass::ExpectedExit,
            detail: "golden expects non-zero exit".to_string(),
        };
    }

    let _ffi_lock = uses_ffi_bridge(stem).then(FfiBridgeLock::acquire);
    let worker_dir = dir.join(format!("w{worker}"));
    let aot = try_compiled_binary_output(&worker_dir, "corpus_gate_aot", 0, stem, &file);
    let aot = match aot {
        Some(out) => out,
        None => {
            return CorpusGateRecord {
                stem: stem.to_string(),
                class: CorpusGateClass::ExpectedExit,
                detail: "AOT compile or run failed".to_string(),
            };
        }
    };
    if aot.exit_code != 0 {
        return CorpusGateRecord {
            stem: stem.to_string(),
            class: CorpusGateClass::ExpectedExit,
            detail: format!("AOT exit {}", aot.exit_code),
        };
    }

    jet_jit::reset_jit_trace_for_test();
    let jit = dev_run_bundle(&bundle, false, false);
    assert!(
        !jet_jit::fallback_invoked_for_test(),
        "`{stem}` must not invoke interpreter/AOT fallback under strict Cranelift"
    );

    match jit {
        RunOutcome::Problems(diags) => {
            assert!(
                jet_jit::is_e2211(&diags),
                "`{stem}` AOT-runnable but strict JIT did not report E2211: {diags:?}"
            );
            let detail = diags
                .iter()
                .find(|d| d.code == "E2211")
                .map(|d| d.why.clone())
                .unwrap_or_else(|| "E2211".to_string());
            CorpusGateRecord {
                stem: stem.to_string(),
                class: CorpusGateClass::E2211,
                detail,
            }
        }
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert!(
                jet_jit::jit_executed_for_test(),
                "`{stem}` must execute resident Cranelift when AOT succeeds"
            );
            let jit_out = normalize_for_parity(
                stem,
                ProgramOutput::ran(stdout, stderr, exit_code),
            );
            let aot_out = normalize_for_parity(stem, aot);
            assert_eq!(
                jit_out, aot_out,
                "`{stem}` resident JIT must match AOT stdout/stderr/exit byte-for-byte"
            );
            CorpusGateRecord {
                stem: stem.to_string(),
                class: CorpusGateClass::ResidentJit,
                detail: String::new(),
            }
        }
    }
}

fn collect_corpus_gate_records() -> Vec<CorpusGateRecord> {
    let dir = std::env::temp_dir().join(format!("jet_corpus_gate_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let have_rustc = have_rustc();
    let mut stems = all_example_stems();
    if let Ok(filter) = std::env::var("JET_CORPUS_GATE_FILTER") {
        stems.retain(|stem| stem.contains(&filter));
    }
    let dir = Arc::new(dir);
    let jobs = Arc::new(Mutex::new(std::collections::VecDeque::from(stems)));
    let records = Arc::new(Mutex::new(Vec::<CorpusGateRecord>::new()));
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut handles = Vec::new();
    for worker in 0..test_worker_count(8) {
        let jobs = Arc::clone(&jobs);
        let records = Arc::clone(&records);
        let failures = Arc::clone(&failures);
        let dir = Arc::clone(&dir);
        handles.push(std::thread::spawn(move || loop {
            let Some(stem) = jobs.lock().unwrap().pop_front() else {
                break;
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                classify_corpus_stem(&stem, &dir, worker, have_rustc)
            }));
            match result {
                Ok(record) => records.lock().unwrap().push(record),
                Err(payload) => failures
                    .lock()
                    .unwrap()
                    .push(format!("{stem}: {}", panic_message(payload))),
            }
        }));
    }
    for handle in handles {
        handle.join().expect("corpus gate worker panicked");
    }
    let failures = failures.lock().unwrap();
    assert!(
        failures.is_empty(),
        "corpus gate classification failures:\n{}",
        failures.join("\n")
    );
    let mut records = records.lock().unwrap().clone();
    records.sort_by(|left, right| left.stem.cmp(&right.stem));
    records
}

fn corpus_gate_manifest_from_records(records: &[CorpusGateRecord]) -> String {
    let mut out = String::from(
        "# c727: differential example-corpus gate manifest.\n\
         # Every top-level examples/features/<topic>/*.jet appears in exactly one section.\n\
         # Update only for intentional ratchet moves.\n\n",
    );
    let classes = [
        CorpusGateClass::FrontendRejected,
        CorpusGateClass::GateExcluded,
        CorpusGateClass::NonRunnable,
        CorpusGateClass::ExpectedExit,
        CorpusGateClass::ResidentJit,
        CorpusGateClass::E2211,
    ];
    for class in classes {
        let section = corpus_gate_section_name(&class);
        let mut section_records: Vec<_> = records
            .iter()
            .filter(|record| record.class == class)
            .collect();
        section_records.sort_by(|left, right| left.stem.cmp(&right.stem));
        out.push_str(section);
        out.push_str(":\n");
        for record in section_records {
            if record.detail.is_empty() {
                out.push_str("  ");
                out.push_str(&record.stem);
                out.push('\n');
            } else {
                out.push_str("  ");
                out.push_str(&record.stem);
                out.push_str(": ");
                out.push_str(&record.detail);
                out.push('\n');
            }
        }
        out.push('\n');
    }
    out
}

fn corpus_gate_section_name(class: &CorpusGateClass) -> &'static str {
    match class {
        CorpusGateClass::FrontendRejected => "frontend_rejected",
        CorpusGateClass::GateExcluded => "gate_excluded",
        CorpusGateClass::NonRunnable => "non_runnable",
        CorpusGateClass::ExpectedExit => "expected_exit",
        CorpusGateClass::ResidentJit => "resident_jit",
        CorpusGateClass::E2211 => "e2211",
    }
}

fn parse_corpus_gate_manifest() -> Vec<CorpusGateRecord> {
    enum Section {
        None,
        FrontendRejected,
        GateExcluded,
        NonRunnable,
        ExpectedExit,
        ResidentJit,
        E2211,
    }

    let mut section = Section::None;
    let mut records = Vec::new();
    for raw in include_str!("jit_corpus_gate.txt").lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        match trimmed {
            "frontend_rejected:" => {
                section = Section::FrontendRejected;
                continue;
            }
            "gate_excluded:" => {
                section = Section::GateExcluded;
                continue;
            }
            "non_runnable:" => {
                section = Section::NonRunnable;
                continue;
            }
            "expected_exit:" => {
                section = Section::ExpectedExit;
                continue;
            }
            "resident_jit:" => {
                section = Section::ResidentJit;
                continue;
            }
            "e2211:" => {
                section = Section::E2211;
                continue;
            }
            _ => {}
        }
        let (stem, detail) = match trimmed.split_once(": ") {
            Some((stem, detail)) => (stem.to_string(), detail.to_string()),
            None => (trimmed.to_string(), String::new()),
        };
        let class = match section {
            Section::FrontendRejected => CorpusGateClass::FrontendRejected,
            Section::GateExcluded => CorpusGateClass::GateExcluded,
            Section::NonRunnable => CorpusGateClass::NonRunnable,
            Section::ExpectedExit => CorpusGateClass::ExpectedExit,
            Section::ResidentJit => CorpusGateClass::ResidentJit,
            Section::E2211 => CorpusGateClass::E2211,
            Section::None => panic!("manifest entry outside a section: {trimmed}"),
        };
        records.push(CorpusGateRecord {
            stem,
            class,
            detail,
        });
    }
    records.sort_by(|left, right| left.stem.cmp(&right.stem));
    records
}

fn print_corpus_gate_manifest(records: &[CorpusGateRecord]) {
    let classes = [
        CorpusGateClass::FrontendRejected,
        CorpusGateClass::GateExcluded,
        CorpusGateClass::NonRunnable,
        CorpusGateClass::ExpectedExit,
        CorpusGateClass::ResidentJit,
        CorpusGateClass::E2211,
    ];
    for class in classes {
        let section = corpus_gate_section_name(&class);
        let mut section_records: Vec<_> = records
            .iter()
            .filter(|record| record.class == class)
            .collect();
        section_records.sort_by(|left, right| left.stem.cmp(&right.stem));
        if section_records.is_empty() {
            continue;
        }
        eprintln!("{section}:");
        for record in section_records {
            if record.detail.is_empty() {
                eprintln!("  {}", record.stem);
            } else {
                eprintln!("  {}: {}", record.stem, record.detail);
            }
        }
        eprintln!();
    }
}

/// c727 C1–C4: discover every top-level example, classify it, and ratchet the
/// manifest. AOT-oracle examples (exit 0) must resident-JIT with backend
/// attribution or report honest E2211 — never silent fallback.
///
/// c727 C5: wired into `.github/workflows/ci.yml` `jetpack-platform` matrix
/// (Linux, macOS, Windows) via `cargo test --test dev example_corpus_strict_jit_aot_differential_gate`.
#[test]
fn example_corpus_strict_jit_aot_differential_gate() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let records = collect_corpus_gate_records();
    if std::env::var("JET_DUMP_CORPUS_GATE").as_deref() == Ok("1") {
        print_corpus_gate_manifest(&records);
        if std::env::var("JET_WRITE_CORPUS_GATE").as_deref() == Ok("1") {
            let manifest = corpus_gate_manifest_from_records(&records);
            fs::write(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/jit_corpus_gate.txt"),
                manifest,
            )
            .expect("write tests/jit_corpus_gate.txt");
        }
        eprintln!(
            "c727 corpus gate: {} examples ({} resident JIT, {} E2211)",
            records.len(),
            records
                .iter()
                .filter(|r| r.class == CorpusGateClass::ResidentJit)
                .count(),
            records
                .iter()
                .filter(|r| r.class == CorpusGateClass::E2211)
                .count(),
        );
        return;
    }
    let expected = parse_corpus_gate_manifest();
    if std::env::var("JET_CORPUS_GATE_FILTER").is_err() {
        assert_eq!(
            records.len(),
            all_example_stems().len(),
            "corpus gate must classify every discovered example"
        );
    }
    assert_eq!(
        records, expected,
        "corpus gate manifest drifted; refresh tests/jit_corpus_gate.txt with \
         JET_DUMP_CORPUS_GATE=1 cargo test --test dev example_corpus_strict_jit_aot_differential_gate -- --exact --nocapture"
    );
    let aot_oracle: Vec<_> = records
        .iter()
        .filter(|r| matches!(r.class, CorpusGateClass::ResidentJit | CorpusGateClass::E2211))
        .collect();
    eprintln!(
        "c727 corpus gate: {} classified, {} AOT-oracle ({} resident JIT, {} E2211)",
        records.len(),
        aot_oracle.len(),
        records
            .iter()
            .filter(|r| r.class == CorpusGateClass::ResidentJit)
            .count(),
        records
            .iter()
            .filter(|r| r.class == CorpusGateClass::E2211)
            .count(),
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

#[test]
fn cranelift_matches_plain_parameter_read_write_and_take_modes() {
    assert_cranelift_reports_jit_gap(
        "fn read(text: String) { print(text) }\nfn edit(values: &[Int]) { values[0] = 9 }\nfn consume(text: ^String) { print(text) }\nfn run() {\n    text :: \"hello\"\n    values := [1, 2]\n    read(text)\n    edit(&values)\n    print(values[0])\n    consume(^text)\n}\n",
        "plain_parameter_modes",
    );
}

#[test]
fn interpreter_writeback_boundary_only_opens_for_resolved_user_functions() {
    let user = bundle_of(
        "fn edit(value: &Int) { value = 2 }\nfn run() { value := 1; edit(&value); print(value) }\n",
        "user_writeback_boundary",
    );
    assert!(
        jet_driver::InterpreterBoundary::dev_boundary_scan(&user).is_none(),
        "resolved user calls have interpreter writeback support"
    );

    let unresolved = bundle_of(
        "fn run() { value := 1; unsupported(&value) }\n",
        "unsupported_writeback_boundary",
    );
    let boundary = jet_driver::InterpreterBoundary::dev_boundary_scan(&unresolved)
        .expect("an unresolved/core/import-style direct call must keep the honest boundary");
    assert_eq!(boundary.code, "E2201");
    assert!(boundary.what.contains("writeback"), "{boundary:?}");

    let mut foreign = bundle_of(
        "fn edit(value: &Int) {}\nfn run() { value := 1; edit(&value) }\n",
        "foreign_writeback_boundary",
    );
    let edit = foreign.modules[0]
        .items
        .iter_mut()
        .find_map(|item| match item {
            jet::AST::Item::Func(function) if function.name == "edit" => Some(function),
            _ => None,
        })
        .expect("fixture has edit");
    let span = edit.name_span;
    edit.inline_foreign = Some(jet::AST::InlineForeign {
        lang: "c".to_string(),
        lang_span: span,
        marker_span: span,
        source: String::new(),
        source_span: span,
    });
    let boundary = jet_driver::InterpreterBoundary::dev_boundary_scan(&foreign)
        .expect("inline foreign functions are not interpreter writeback targets");
    assert_eq!(boundary.code, "E2201");
    assert!(boundary.what.contains("writeback"), "{boundary:?}");
}

#[test]
fn cranelift_matches_variadic_fixed_writeback() {
    assert_cranelift_reports_jit_gap(
        "fn edit(values: &[Int], extras: ...Int) { values[0] = extras.len() }\nfn run() {\n    values := [0]\n    edit(&values, 7, 8)\n    print(values[0])\n}\n",
        "variadic_fixed_writeback",
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
        "fn run() {\n    loop n; 1..3 {\n        print(n)\n    }\n}\n",
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
    assert_cranelift_reports_jit_gap(
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

    let mut backend = CraneliftBackend::new();
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

    let mut backend2 = CraneliftBackend::new();
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

    let mut backend = CraneliftBackend::new();
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

/// D-PERSIST1: `#Persist` is inert in a release build — the marker carries no
/// runtime-carry-across machinery yet (named gate: a module+name-keyed value
/// store in the JIT resident runtime, `crates/jet-jit`, doesn't exist). This
/// asserts that by construction: the generated Rust for a module-level
/// `const` is byte-for-byte identical with and without `#Persist`.
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
        "#Persist const counter = 0;\nfn run() {\n    print(counter)\n}\n",
        "counter.jet",
    ));
    assert_eq!(
        plain, persisted,
        "#Persist must not change generated Rust (inert in release)"
    );
}

// ── card #131 S1-bridge (D-SERDE2): hand codec dev-tier parity (R12) ──────────
// A hand `impl T.Encode`/`impl T.Decode` round-trips under the native build (see
// tests/corelib.rs::hand_written_encode_decode_round_trips). The dev interpreter
// does not cover the json typed-decode path — and it doesn't for a DERIVED
// `#[Codable]` either — so the honest behavior for BOTH is to stop at the E2201
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
        m: [String: DataTree] :: ["email": DataTree.Text(~self.addr)]
        return DataTree.Object(m)
    }
}

impl Email.Decode {
    fn decode(tree: DataTree) -> Email ? DecodeError {
        f := tree.field("email") ?? DataTree.Text("")
        s := f.text() ?? ""
        return Ok(Email.{addr: s})
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

/// c728 C6: a watching `jet dev` session reports E2211 on a JIT-gap edit, keeps
/// the last good bundle, accepts a later valid edit, and one-shot dev exits 101.
#[test]
fn watching_dev_keeps_last_good_on_jit_gap_and_recovers() {
    let dir = std::env::temp_dir().join(format!("jet_dev_watch_c6_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("app.jet");
    fs::write(&file, "fn run() {\n    print(\"good-v1\")\n}\n").unwrap();
    let shown = file.to_string_lossy().to_string();

    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"));
    child
        .arg("dev")
        .arg(&shown)
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = child.spawn().expect("spawn watching jet dev");

    std::thread::sleep(Duration::from_millis(800));
    fs::write(
        &file,
        "use core.env as env\nfn run() {\n    print(env.current_dir())\n}\n",
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(800));
    fs::write(&file, "fn run() {\n    print(\"good-v2\")\n}\n").unwrap();
    std::thread::sleep(Duration::from_millis(800));

    let _ = child.kill();
    let out = child.wait_with_output().expect("watching jet dev output");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("good-v1"), "stdout:\n{stdout}");
    assert!(
        stderr.contains("E2211") || stdout.contains("E2211"),
        "expected E2211 on JIT-gap edit\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("good-v2"), "stdout:\n{stdout}");

    fs::write(
        &file,
        "use core.env as env\nfn run() {\n    print(env.current_dir())\n}\n",
    )
    .unwrap();
    let once = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["dev", &shown, "--watch=off"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let once_stderr = String::from_utf8_lossy(&once.stderr);
    assert_eq!(once.status.code(), Some(101), "stderr={once_stderr}");
    assert!(once_stderr.contains("E2211"), "{once_stderr}");
}
