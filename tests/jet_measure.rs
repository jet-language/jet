//! D-CLAIM-BENCH1=A: measurement is a test mode; the retired command points
//! at the measured-test spelling.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

mod common;

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

#[test]
fn jet_test_measure_selects_only_measure_claims() {
    if !common::have_rustc() {
        return;
    }
    let scratch = common::Scratch::new("test_measure");
    fs::write(
        scratch.join("claims.jet"),
        r#"fn run() {}

#Test("measured") { .measure {
    assert_eq(1, 1)
} }

#Test("ordinary") {
    assert_eq(2, 2)
}
"#,
    )
    .unwrap();

    let measured = Command::new(jet_bin())
        .args(["test", "claims.jet", "--measure"])
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        measured.status.success(),
        "measured test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&measured.stdout),
        String::from_utf8_lossy(&measured.stderr)
    );
    let measured_stdout = String::from_utf8_lossy(&measured.stdout);
    assert!(measured_stdout.contains("measured"), "{measured_stdout}");
    assert!(!measured_stdout.contains("ordinary"), "{measured_stdout}");

    let ordinary = Command::new(jet_bin())
        .args(["test", "claims.jet"])
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(ordinary.status.success(), "{ordinary:?}");
    let ordinary_stdout = String::from_utf8_lossy(&ordinary.stdout);
    assert!(ordinary_stdout.contains("measured"), "{ordinary_stdout}");
    assert!(ordinary_stdout.contains("ordinary"), "{ordinary_stdout}");
}

#[test]
fn plain_jet_test_fails_a_crashing_measure_claim() {
    if !common::have_rustc() {
        return;
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture = root.join("tests/fixtures/bench_fail.jet");
    let output = Command::new(jet_bin())
        .args(["test", "--show-default"])
        .arg(&fixture)
        .env("NO_COLOR", "1")
        .output()
        .expect("run plain jet test");

    assert!(
        !output.status.success(),
        "a failing .measure claim passed in plain test mode:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("expected measured failure"),
        "the measured claim failure was not reported:\n{stderr}"
    );
}

#[test]
fn retired_measurement_command_teaches_test_measure() {
    let output = Command::new(jet_bin())
        .args(["bench"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success(), "retired command unexpectedly ran");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("jet test --measure"), "{stderr}");
}
