//! Flagship vertical-slice harness for `examples/apps/*`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn run_jet(args: &[&str]) -> std::process::Output {
    Command::new(jet_bin())
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run jet {:?}: {e}", args))
}

fn assert_success(args: &[&str]) -> String {
    let out = run_jet(args);
    assert!(
        out.status.success(),
        "jet {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn assert_expected(app: &str) {
    let main = format!("examples/apps/{app}/main.jet");
    let expected = fs::read_to_string(format!("examples/apps/{app}/expected/run.out")).unwrap();
    let actual = assert_success(&["run", &main]);
    assert_eq!(actual, expected, "{app} stdout drifted");
    let test_out = assert_success(&["test", &main]);
    assert!(
        test_out.contains("passed") || test_out.contains("ok"),
        "{app} test output should report success, got:\n{test_out}"
    );
}

fn assert_manifest(app: &str) {
    let dir = Path::new("examples/apps").join(app);
    assert!(dir.join("pkg.jet").is_file(), "{app} missing pkg.jet");
    assert!(dir.join("README.md").is_file(), "{app} missing README.md");
    assert!(
        dir.join("expected/run.out").is_file(),
        "{app} missing golden"
    );
}

#[test]
fn app_slices_run_tests_and_match_goldens() {
    for app in ["jetgrep", "jetpaste", "metal", "jettasks", "jetfighter"] {
        assert_manifest(app);
        assert_expected(app);
    }
}

#[test]
fn metal_freestanding_builds() {
    let out = run_jet(&[
        "build",
        "--emit-rust",
        "--freestanding",
        "examples/apps/metal/main.jet",
    ]);
    assert!(
        out.status.success(),
        "metal freestanding build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn jettasks_web_builds() {
    let out = run_jet(&["build", "--target=web", "examples/apps/jettasks/main.jet"]);
    assert!(
        out.status.success(),
        "jettasks web build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
