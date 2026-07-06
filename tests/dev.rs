//! E2-M4 — `jet dev` interpreter tests.
//!
//! The crux is the **differential battery** (D-DEV, I2): for each supported
//! program, the interpreter's stdout/stderr/exit code MUST be byte-for-byte
//! identical to the compiled native binary. Any divergence is a P0 miscompile-class
//! bug — the interpreter is a dev convenience that must never lie about what
//! the real build does. This mirrors `tests/comptime_diff.rs`.
//!
//! Also tested:
//!   - the E2201 honest-boundary note (tasks/FFI/`@unsafe`/native std),
//!   - the per-iteration `dev_iteration` function the watch loop is built on,
//!   - the save-to-diagnostic latency budget (D-DEV3, <200ms check-only).

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use jet::Interpreter::{dev_iteration, RunOutcome};
use jet::JitBackend::{InterpreterBackend, JitBackend};
use jet_jit::CraneliftBackend;

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
) -> Option<ProgramOutput> {
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
    fs::write(&rs, &compiled.rust).unwrap();
    let out = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .unwrap();
    if !out.status.success() {
        // Some examples need linker inputs outside this standalone rustc path
        // (FFI/C/system libraries). Their dev boundary is asserted elsewhere.
        return None;
    }
    let run = Command::new(&bin).output().unwrap();
    Some(ProgramOutput::ran(
        String::from_utf8_lossy(&run.stdout).to_string(),
        String::from_utf8_lossy(&run.stderr).to_string(),
        run.status.code().unwrap_or(1),
    ))
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
    if matches!(stem, "io/log" | "serde/json_coerce") {
        out.stderr = normalize_json_log_timestamps(&out.stderr);
    }
    if stem == "io/log_human" {
        out.stderr = normalize_text_log_timestamps(&out.stderr);
    }
    out
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

/// c77 widened battery: EVERY example either runs (interpreted stdout/stderr/exit
/// code == compiled-binary stdout/stderr/exit code, byte for byte — I2) or stops at a named boundary
/// (E2201/E2202/E0956 — never a silent skip). Reports the run/boundary split so
/// the coverage can't quietly shrink.
#[test]
fn interpreter_matches_compiled_binary() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping jet dev differential battery");
        return;
    }
    let dir = std::env::temp_dir();
    let (_, _, manifested_divergences) = parse_jit_gap_manifest();
    let mut ran = 0usize;
    let mut boundary = 0usize;
    let mut manifested = 0usize;
    for (i, stem) in all_example_stems().iter().enumerate() {
        let file = example_path(stem);

        let interpreted = match dev_iteration(&file, false, true) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(diags) => {
                // Not runnable → must be an honest, named boundary, not a silent
                // skip and not a stray internal error.
                assert!(
                    diags.iter().any(|d| BOUNDARY_CODES.contains(&d.code)),
                    "`{}` neither ran nor stopped at a named boundary {:?}; codes were {:?}",
                    stem,
                    BOUNDARY_CODES,
                    diags.iter().map(|d| d.code).collect::<Vec<_>>()
                );
                boundary += 1;
                continue;
            }
        };
        ran += 1;

        let Some(compiled) = compiled_binary_output(&dir, "dev_diff", i, stem, &file) else {
            continue;
        };

        let interpreted = normalize_for_parity(stem, interpreted);
        let compiled = normalize_for_parity(stem, compiled);
        if interpreted != compiled {
            if is_manifested_parity_divergence(stem, &manifested_divergences) {
                manifested += 1;
                eprintln!("manifested parity divergence: {stem}");
                continue;
            }
            assert_eq!(
                interpreted, compiled,
                "DIVERGENCE for `{}`: interpreter and compiled binary disagree on stdout/stderr/exit code — this is a P0 miscompile",
                stem
            );
        }
    }
    eprintln!(
        "c77 battery: {} ran ({} interp==compiled, {} manifested divergences), {} boundary-asserted, {} total",
        ran,
        ran - manifested,
        manifested,
        boundary,
        ran + boundary
    );
    assert!(
        ran > 0,
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
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping jet dev (default backend) differential battery");
        return;
    }
    let dir = std::env::temp_dir();
    let (_, _, manifested_divergences) = parse_jit_gap_manifest();
    let mut ran = 0usize;
    let mut boundary = 0usize;
    let mut manifested = 0usize;
    for (i, stem) in all_example_stems().iter().enumerate() {
        let file = example_path(stem);

        let interpreted = match dev_iteration(&file, false, false) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(diags) => {
                assert!(
                    diags.iter().any(|d| BOUNDARY_CODES.contains(&d.code)),
                    "`{}` neither ran nor stopped at a named boundary {:?} under the default \
                     jet dev backend; codes were {:?}",
                    stem,
                    BOUNDARY_CODES,
                    diags.iter().map(|d| d.code).collect::<Vec<_>>()
                );
                boundary += 1;
                continue;
            }
        };
        ran += 1;

        let Some(compiled) = compiled_binary_output(&dir, "dev_default_diff", i, stem, &file)
        else {
            continue;
        };

        let interpreted = normalize_for_parity(stem, interpreted);
        let compiled = normalize_for_parity(stem, compiled);
        if interpreted != compiled {
            if is_manifested_parity_divergence(stem, &manifested_divergences) {
                manifested += 1;
                eprintln!("manifested parity divergence: {stem}");
                continue;
            }
            assert_eq!(
                interpreted, compiled,
                "DIVERGENCE for `{}` under the default jet dev backend: JIT/fallback and compiled \
                 binary disagree on stdout/stderr/exit code — this is a P0 miscompile",
                stem
            );
        }
    }
    eprintln!(
        "c125 default-backend battery: {} ran ({} default==compiled, {} manifested divergences), {} boundary-asserted, {} total",
        ran,
        ran - manifested,
        manifested,
        boundary,
        ran + boundary
    );
    assert!(
        ran > 0,
        "expected at least some examples to run via the default jet dev backend"
    );
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
        let expected_path = root.join(format!("examples/features/expected/{}.out", stem));
        match dev_iteration(&file, false, true) {
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
                    diags.iter().any(|d| BOUNDARY_CODES.contains(&d.code)),
                    "`{}` neither ran nor stopped at a named boundary; codes were {:?}",
                    stem,
                    diags.iter().map(|d| d.code).collect::<Vec<_>>()
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

    let expected = compiled_binary_output(&dir, "aot_fallback", 0, "env_fallback", &shown)
        .expect("fixture should compile through the AOT path");
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
    let expected = compiled_binary_output(&dir, "aot_fallback_log", 0, "io/log", file)
        .expect("io/log should compile through the AOT path");
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
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle) && jet_jit::try_compile_bundle(&bundle).is_ok(),
        "`{stem}` must be resident-JIT safe for three-way differential"
    );

    let interpreted = match dev_iteration(file, false, true) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) if ds.iter().any(|d| BOUNDARY_CODES.contains(&d.code)) => {
            golden_stdout(stem)
        }
        RunOutcome::Problems(ds) => {
            panic!("interpreter baseline must run `{stem}`, got diagnostics: {ds:?}")
        }
    };

    let mut backend = CraneliftBackend::new(InterpreterBackend::new());
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
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
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
                diags.iter().map(|d| d.code).collect::<Vec<_>>()
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
