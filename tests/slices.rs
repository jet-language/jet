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

fn assert_failure(args: &[&str], needle: &str) {
    let out = run_jet(args);
    assert!(
        !out.status.success(),
        "jet {:?} unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains(needle),
        "jet {:?} missing `{needle}`\noutput:\n{combined}",
        args
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
fn jetgrep_reports_cli_errors() {
    assert_failure(
        &[
            "run",
            "examples/apps/jetgrep/main.jet",
            "[",
            "examples/apps/jetgrep/fixtures/api.log",
        ],
        "jetgrep: invalid regex [",
    );
    assert_failure(
        &[
            "run",
            "examples/apps/jetgrep/main.jet",
            "error",
            "examples/apps/jetgrep/fixtures/missing.txt",
        ],
        "jetgrep: missing file examples/apps/jetgrep/fixtures/missing.txt",
    );
}

#[test]
fn jetgrep_cli_modes_are_pinned() {
    let count = assert_success(&[
        "run",
        "examples/apps/jetgrep/main.jet",
        "--",
        "--count",
        "warning",
        "examples/apps/jetgrep/fixtures",
    ]);
    assert_eq!(
        count,
        concat!(
            "jetgrep pattern=warning\n",
            "examples/apps/jetgrep/fixtures/api.log:1\n",
            "examples/apps/jetgrep/fixtures/notes.txt:1\n",
            "matches=2\n",
        )
    );

    let files = assert_success(&[
        "run",
        "examples/apps/jetgrep/main.jet",
        "--",
        "--files",
        "TODO",
        "examples/apps/jetgrep/fixtures",
    ]);
    assert_eq!(
        files,
        concat!(
            "jetgrep pattern=TODO\n",
            "examples/apps/jetgrep/fixtures/nested/deploy.log\n",
            "examples/apps/jetgrep/fixtures/notes.txt\n",
            "matches=2\n",
        )
    );

    let ignored = assert_success(&[
        "run",
        "examples/apps/jetgrep/main.jet",
        "--",
        "--ignore",
        "nested",
        "TODO",
        "examples/apps/jetgrep/fixtures",
    ]);
    assert_eq!(
        ignored,
        concat!(
            "jetgrep pattern=TODO\n",
            "examples/apps/jetgrep/fixtures/notes.txt:2: TODO add benchmark corpus once owner greenlights docs/build gates\n",
            "matches=1\n",
        )
    );
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
