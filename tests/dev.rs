//! E2-M4 — `jet dev` interpreter tests.
//!
//! The crux is the **differential battery** (D-DEV, I2): for each supported
//! program, the interpreter's stdout MUST be byte-for-byte identical to the
//! compiled native binary's stdout. Any divergence is a P0 miscompile-class
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

use jet::interp::{dev_iteration, RunOutcome};

/// Supported programs whose interpreted stdout must equal compiled stdout.
/// These use only the deterministic, pure-enough subset the dev interpreter
/// covers (control flow, math, strings, lists, structs/enums, fan-out, plain
/// `print`). Programs using `??`, tasks, FFI, files, etc. are intentionally
/// out of the battery (they hit E0956/E2201, tested separately).
const BATTERY: &[&str] = &[
    "01_hello",
    "02_functions",
    "03_values",
    "04_branches",
    "05_fizzbuzz",
    "06_compound",
    "07_switch",
    "17_strings",
    "34_digits",
    "35_numbers",
    "36_range_step",
    "37_if_expression",
    "38_method_chain",
    "39_multiline_string",
    "41_fan_out",
];

fn example_path(stem: &str) -> String {
    format!("examples/features/{}.jet", stem)
}

/// I2 differential: interpreted stdout == compiled-binary stdout, byte for
/// byte, for every supported program.
#[test]
fn interpreter_matches_compiled_binary() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping jet dev differential battery");
        return;
    }
    let dir = std::env::temp_dir();
    for (i, stem) in BATTERY.iter().enumerate() {
        let file = example_path(stem);

        // Interpreter side.
        let interpreted = match dev_iteration(&file, false) {
            RunOutcome::Ran { stdout, .. } => stdout,
            RunOutcome::Problems(diags) => {
                let src = fs::read_to_string(&file).unwrap_or_default();
                panic!(
                    "`{}` did not run in the interpreter (battery programs must):\n{}",
                    stem,
                    jet::render_diagnostics(&file, &src, &diags)
                );
            }
        };

        // Compiled side: front end -> rustc -> run.
        let src = fs::read_to_string(&file).unwrap();
        let compiled = match jet::compile_with_path(&src, &file) {
            Ok(c) => c,
            Err(diags) => panic!(
                "`{}` failed the front end:\n{}",
                stem,
                jet::render_diagnostics(&file, &src, &diags)
            ),
        };
        let rs = dir.join(format!("jet_dev_diff_{}.rs", i));
        let bin = dir.join(format!("jet_dev_diff_{}", i));
        fs::write(&rs, &compiled.rust).unwrap();
        let out = Command::new("rustc")
            .args(["--edition", "2021"])
            .arg(&rs)
            .arg("-o")
            .arg(&bin)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "I2 violated: rustc rejected generated code for `{}`:\n{}",
            stem,
            String::from_utf8_lossy(&out.stderr)
        );
        let run = Command::new(&bin).output().unwrap();
        let compiled_stdout = String::from_utf8_lossy(&run.stdout).to_string();

        assert_eq!(
            interpreted, compiled_stdout,
            "DIVERGENCE for `{}`: interpreter and compiled binary disagree — this is a P0 miscompile",
            stem
        );
    }
}

/// The interpreter's output also matches the checked-in `expected/*.out`
/// golden (the executable spec, I5) — a cheap check that needs no rustc.
#[test]
fn interpreter_matches_expected_golden() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for stem in BATTERY {
        let file = example_path(stem);
        let expected_path = root.join(format!("examples/features/expected/{}.out", stem));
        let expected = fs::read_to_string(&expected_path)
            .unwrap_or_else(|_| panic!("missing expected output for `{}`", stem));
        match dev_iteration(&file, false) {
            RunOutcome::Ran { stdout, .. } => {
                assert_eq!(
                    stdout, expected,
                    "`{}`: interpreter output differs from expected golden",
                    stem
                );
            }
            RunOutcome::Problems(diags) => {
                let src = fs::read_to_string(&file).unwrap_or_default();
                panic!(
                    "`{}` did not run in the interpreter:\n{}",
                    stem,
                    jet::render_diagnostics(&file, &src, &diags)
                );
            }
        }
    }
}

/// D-DEV1 honest boundary: a program that spawns a task stops with E2201
/// naming the feature and `jet build`/`jet run`. The expected note is pinned
/// in `tests/dev/unsupported.txt`.
#[test]
fn task_program_hits_e2201_boundary() {
    let file = "examples/features/32_tasks.jet";
    match dev_iteration(file, false) {
        RunOutcome::Problems(diags) => {
            assert_eq!(diags.len(), 1, "expected exactly one boundary note");
            let d = &diags[0];
            assert_eq!(d.code, "E2201");
            assert!(
                d.what.contains("spawns a task"),
                "E2201 should name the task feature, got: {}",
                d.what
            );
            assert!(
                d.fix.contains("jet build") || d.fix.contains("jet run"),
                "E2201 fix must point at the real build path, got: {}",
                d.fix
            );
        }
        RunOutcome::Ran { .. } => {
            panic!("a task program must not run in the dev interpreter (I2 boundary)")
        }
    }
}

/// D-DEV1 "try anyway": the opt-in flag skips the boundary scan and attempts
/// execution. For a task program it then fails honestly at the unsupported
/// construct (E0956) rather than refusing up front — no guarantees, but it
/// tried.
#[test]
fn try_anyway_skips_the_boundary_scan() {
    let file = "examples/features/32_tasks.jet";
    match dev_iteration(file, true) {
        RunOutcome::Problems(diags) => {
            // It got past the E2201 pre-scan and hit a real unsupported
            // construct during interpretation.
            assert!(
                diags.iter().all(|d| d.code != "E2201"),
                "try-anyway must skip the E2201 pre-scan"
            );
        }
        // If a future evaluator can run it, that's fine too — the point is the
        // pre-scan was skipped.
        RunOutcome::Ran { .. } => {}
    }
}

/// The dev iteration surfaces front-end errors identically to batch
/// compilation (D-DEV: same diagnostics).
#[test]
fn front_end_errors_surface_in_dev_iteration() {
    // Write a broken program to a temp file.
    let dir = std::env::temp_dir();
    let file = dir.join("jet_dev_broken.jet");
    fs::write(&file, "fn main() {\n    print(nope);\n}\n").unwrap();
    let shown = file.to_string_lossy().to_string();
    match dev_iteration(&shown, false) {
        RunOutcome::Problems(diags) => {
            assert!(!diags.is_empty(), "broken program must report problems");
            assert!(
                diags.iter().all(|d| matches!(d.severity, jet::diag::Severity::Error)),
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
    let file = "examples/features/16_wordcount.jet";
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
