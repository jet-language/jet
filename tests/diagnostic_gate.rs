//! #1463 — `jet build` and `jet run` share one diagnostic gate.
//!
//! Recoverable parse teaching must surface identically on every execution
//! entry. The single call site is `Driver::gate_diagnostics`.

use std::collections::BTreeSet;
use std::fs;
use std::process::Command;

mod common;

use jet::Interpreter::{dev_iteration, run_jit_once, RunOutcome};

fn codes(diags: &[jet::Diagnostics::Diagnostic]) -> BTreeSet<String> {
    diags.iter().map(|d| d.code.clone()).collect()
}

/// `#Pure` is a recoverable E0927 teaching diagnostic that lives only in
/// `parse_teaching` (sema does not re-emit it). That is the gap `jet run`
/// used to drop while `jet build` reported it.
const PURE_TEACHING: &str = r#"
#Pure
fn work() {}
fn run() {
    print("hi")
}
"#;

/// Driver-level: compile path and JIT checked path report the same error codes
/// for a recoverable parse-only diagnostic.
#[test]
fn build_and_run_share_one_diagnostic_gate() {
    let dir = common::unique_tmp("diag_gate_1463");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("pure.jet");
    fs::write(&file, PURE_TEACHING).unwrap();
    let path = file.to_str().unwrap();

    let build_err = jet::compile_with_path("", path)
        .err()
        .expect("build path must reject retired #Pure");
    assert!(
        build_err.iter().any(|d| d.code == "E0927"),
        "build gate must include E0927, got: {:?}",
        codes(&build_err)
    );

    match run_jit_once(path) {
        RunOutcome::Problems(run_err) => {
            assert_eq!(
                codes(&build_err),
                codes(&run_err),
                "build and run must share one diagnostic set\nbuild: {:?}\nrun: {:?}",
                codes(&build_err),
                codes(&run_err)
            );
        }
        other => panic!("jet run must surface Problems, got: {other:?}"),
    }
}

/// CLI: retired-marker repro reports E0927 with non-zero exit under both
/// `jet build` and default `jet run`.
#[test]
fn retired_marker_fails_build_and_default_run() {
    let dir = common::unique_tmp("diag_gate_cli");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("retired.jet");
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/ui/E0927_retired_marker.jet"
    );
    fs::copy(fixture, &file).unwrap();
    let path = file.to_str().unwrap();
    let jet = env!("CARGO_BIN_EXE_jet");

    let build = Command::new(jet)
        .args(["build", path])
        .output()
        .expect("jet build");
    let build_text = format!(
        "{}{}",
        String::from_utf8_lossy(&build.stderr),
        String::from_utf8_lossy(&build.stdout)
    );
    assert!(
        !build.status.success(),
        "jet build must exit non-zero for E0927"
    );
    assert!(
        build_text.contains("E0927") && build_text.contains("retired"),
        "jet build must report E0927 retired wording, got:\n{build_text}"
    );

    let run = Command::new(jet)
        .args(["run", path])
        .output()
        .expect("jet run");
    let run_text = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stderr),
        String::from_utf8_lossy(&run.stdout)
    );
    assert!(
        !run.status.success(),
        "jet run must exit non-zero for E0927"
    );
    assert!(
        run_text.contains("E0927") && run_text.contains("retired"),
        "jet run must report E0927 retired wording, got:\n{run_text}"
    );
    assert!(
        build_text.contains("[E0927]") && run_text.contains("[E0927]"),
        "both paths must name [E0927]"
    );
}

/// Parse-only teaching (`#Pure`) must also fail default `jet run` — not only
/// markers that sema re-emits.
#[test]
fn pure_teaching_fails_default_run() {
    let dir = common::unique_tmp("diag_gate_pure_cli");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("pure.jet");
    fs::write(&file, PURE_TEACHING).unwrap();
    let path = file.to_str().unwrap();
    let jet = env!("CARGO_BIN_EXE_jet");

    let build = Command::new(jet)
        .args(["build", path])
        .output()
        .expect("jet build");
    let run = Command::new(jet)
        .args(["run", path])
        .output()
        .expect("jet run");
    assert!(!build.status.success(), "jet build must reject #Pure");
    assert!(!run.status.success(), "jet run must reject #Pure");
    let build_text = format!(
        "{}{}",
        String::from_utf8_lossy(&build.stderr),
        String::from_utf8_lossy(&build.stdout)
    );
    let run_text = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stderr),
        String::from_utf8_lossy(&run.stdout)
    );
    assert!(build_text.contains("E0927") && run_text.contains("E0927"));
    assert!(!run_text.contains("hi"), "jet run must not execute past the gate");
}

/// `jet dev` / interpreter deopt entry uses the same gate via `checked_bundle`.
#[test]
fn dev_path_reports_parse_teaching() {
    let dir = common::unique_tmp("diag_gate_dev");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("pure.jet");
    fs::write(&file, PURE_TEACHING).unwrap();
    let path = file.to_str().unwrap();
    match dev_iteration(path, false, true) {
        RunOutcome::Problems(diags) => {
            assert!(
                diags.iter().any(|d| d.code == "E0927"),
                "dev/interpret gate must include E0927, got: {:?}",
                codes(&diags)
            );
        }
        other => panic!("dev path must surface Problems for E0927, got: {other:?}"),
    }
}
