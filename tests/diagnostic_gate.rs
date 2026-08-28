//! #1463 — `jet build` and `jet run` share one diagnostic gate.
//!
//! Recoverable parse teaching must surface identically on every execution
//! entry. The single call site is `Driver::gate_diagnostics`.

use std::collections::BTreeSet;
use std::fs;
use std::process::{Command, Output};

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

const L0103_SOURCE: &str = include_str!("ui_lint/qualified_alias_import_liveness.jet");
const L0202_SOURCE: &str = include_str!("ui_lint/shared_loop.jet");
const DIAGNOSTIC_GATE_PACKAGE: &str =
    "name: \"diagnostic_gate\"\nversion: \"0.1.0\"\nauthority: .{ holds: { allow: [IO, Mem.Alloc] } }\n";
const L0103_SOURCE_OUTPUT: &str =
    "key=1 door=3\nkey=3 door=1\nwins=2\nstatus=failed\nfailures=7\n";
const DENIED_LINT_PACKAGE: &str =
    "name: \"diagnostic_gate_denied\"\nversion: \"0.1.0\"\nauthority: .{ holds: { allow: [IO, Mem.Alloc] } }\npolicy: .{ lints: .{ deny: [unused_import] } }\n";
const DENIED_LINT_SOURCE: &str = r#"
use core.files as files

fn run() {
    print("DENIED_SENTINEL")
}
"#;

fn diagnostic_gate_project(tag: &str, source: &str, denied: bool) -> common::Scratch {
    let project = common::Scratch::new(tag);
    fs::write(project.join("main.jet"), source).unwrap();
    fs::write(
        project.join("package.jet"),
        if denied {
            DENIED_LINT_PACKAGE
        } else {
            DIAGNOSTIC_GATE_PACKAGE
        },
    )
    .unwrap();
    project
}

fn run_cli(project: &common::Scratch, cache: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .current_dir(&project.path)
        .env(
            "JET_RUN_CACHE_DIR",
            project.path.join(format!("cache/{cache}/run")),
        )
        .env(
            "JET_CACHE_DIR",
            project.path.join(format!("cache/{cache}/build")),
        )
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("jet {args:?}: {error}"))
}

fn assert_warning_status(output: &Output, label: &str, code: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{label} exited nonzero:\nstdout={}\nstderr={stderr}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        stderr.contains(&format!("Warning [{code}]")),
        "{label} did not classify {code} as a warning:\n{stderr}"
    );
    assert!(
        !stderr.contains(&format!("Error [{code}]")),
        "{label} classified {code} as an error:\n{stderr}"
    );
}

fn assert_warning_execution(output: &Output, label: &str, code: &str, stdout: &str) {
    assert_warning_status(output, label, code);
    assert_eq!(
        output.stdout,
        stdout.as_bytes(),
        "{label} program stdout changed"
    );
}

#[test]
fn warnings_never_choose_execution_exit_code() {
    let cases = [
        (
            "l0103",
            L0103_SOURCE,
            "L0103",
            L0103_SOURCE_OUTPUT,
        ),
        ("l0202", L0202_SOURCE, "L0202", "0\n"),
    ];
    let modes: [(&str, &[&str]); 4] = [
        ("default", &["run", "main.jet"]),
        ("interpret", &["run", "--interpret", "main.jet"]),
        ("release", &["run", "--release", "main.jet"]),
        ("dev", &["dev", "main.jet", "--watch=off"]),
    ];

    for &(name, source, code, stdout) in &cases {
        for &(mode, args) in &modes {
            let project = diagnostic_gate_project(
                &format!("diag_gate_{name}_{mode}"),
                source,
                false,
            );
            let output = run_cli(&project, mode, args);
            assert_warning_execution(&output, &format!("{name} {mode}"), code, stdout);
        }

        let project = diagnostic_gate_project(&format!("diag_gate_{name}_build"), source, false);
        let build = run_cli(&project, "build", &["build", "main.jet"]);
        assert_warning_status(&build, &format!("{name} build"), code);
        let direct = Command::new(project.join("build/main"))
            .current_dir(&project.path)
            .output()
            .unwrap_or_else(|error| panic!("{name} direct binary: {error}"));
        assert_eq!(direct.status.code(), Some(0), "{name} direct binary failed");
        assert_eq!(direct.stdout, stdout.as_bytes(), "{name} direct stdout changed");
    }

    let project = diagnostic_gate_project("diag_gate_l0103_json", L0103_SOURCE, false);
    let json = run_cli(&project, "json", &["run", "--json", "main.jet"]);
    assert_eq!(json.status.code(), Some(0), "json run exited nonzero");
    assert_eq!(json.stdout, L0103_SOURCE_OUTPUT.as_bytes());
    let json_stderr = String::from_utf8_lossy(&json.stderr);
    assert!(json_stderr.contains("\"severity\":\"warning\""), "{json_stderr}");
    assert!(json_stderr.contains("\"code\":\"L0103\""), "{json_stderr}");
    assert!(!json_stderr.contains("\"severity\":\"error\""), "{json_stderr}");

    let project = diagnostic_gate_project("diag_gate_l0103_quiet", L0103_SOURCE, false);
    let quiet = run_cli(&project, "quiet", &["run", "--quiet", "main.jet"]);
    assert_eq!(quiet.status.code(), Some(0), "quiet run exited nonzero");
    assert_eq!(
        quiet.stdout,
        L0103_SOURCE_OUTPUT.as_bytes(),
        "quiet run program stdout changed"
    );
}

#[test]
fn denied_lint_still_blocks_execution() {
    for (mode, args) in [
        ("build", &["build", "main.jet"][..]),
        ("run", &["run", "main.jet"][..]),
        ("dev", &["dev", "main.jet", "--watch=off"][..]),
    ] {
        let project = diagnostic_gate_project(
            &format!("diag_gate_denied_{mode}"),
            DENIED_LINT_SOURCE,
            true,
        );
        let output = run_cli(&project, mode, args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_ne!(output.status.code(), Some(0), "{mode} ran denied lint");
        assert!(stderr.contains("Error [E1293]"), "{mode} missing E1293:\n{stderr}");
        assert!(stderr.contains("L0103"), "{mode} missing denied L0103:\n{stderr}");
        assert!(!stderr.contains("Warning [L0103]"), "{mode} kept denied warning:\n{stderr}");
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("DENIED_SENTINEL"),
            "{mode} executed denied program"
        );
    }
}

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
    assert!(
        !run_text.contains("hi"),
        "jet run must not execute past the gate"
    );
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
