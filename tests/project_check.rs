//! Project-check witnesses for card #2389.
//!
//! These cases drive the real `jet check` command against frozen projects.  The
//! project proof rows are snapshotted from stdout, while induced failures use
//! stderr snapshots.  Paths are scrubbed because the fixture checkout location
//! is machine-specific; proof details and content-derived Core fingerprints
//! remain byte-exact.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/project_check/fixtures")
}

fn fixture(name: &str) -> PathBuf {
    fixture_root().join(name)
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn run_check(name: &str, args: &[&str]) -> Output {
    let dir = fixture(name);
    Command::new(jet_bin())
        .args(args)
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .output()
        .unwrap_or_else(|error| panic!("jet check {name} failed to start: {error}"))
}

fn scrub_fixture_path(text: &str, dir: &Path) -> String {
    let absolute = dir
        .canonicalize()
        .unwrap_or_else(|error| panic!("fixture {} is not canonical: {error}", dir.display()));
    text.replace(&*absolute.to_string_lossy(), "<fixture>")
}

fn proof_snapshot(output: &Output, dir: &Path) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut snapshot = stdout
        .lines()
        .filter(|line| line.starts_with("proof: "))
        .map(|line| scrub_fixture_path(line, dir))
        .collect::<Vec<_>>()
        .join("\n");
    if !snapshot.is_empty() {
        snapshot.push('\n');
    }
    snapshot
}

fn stderr_snapshot(output: &Output, dir: &Path) -> String {
    scrub_fixture_path(&String::from_utf8_lossy(&output.stderr), dir)
}

fn expected(name: &str, extension: &str) -> PathBuf {
    fixture(name).join(format!("expected.{extension}"))
}

fn assert_snapshot(name: &str, extension: &str, actual: &str) {
    let path = expected(name, extension);
    if std::env::var_os("UPDATE_EXPECT").is_some() {
        fs::write(&path, actual)
            .unwrap_or_else(|error| panic!("write snapshot {}: {error}", path.display()));
        return;
    }
    let want = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read snapshot {}: {error}", path.display()));
    assert_eq!(actual, want, "snapshot mismatch for {}", path.display());
}

fn assert_project_proof_rows(fixture_name: &str, proof: &str) {
    let rows = proof.lines().collect::<Vec<_>>();
    assert_eq!(
        rows.len(),
        4,
        "project check {fixture_name} must expose one row for each proof class:\n{proof}"
    );
    for (label, code) in [
        ("entry resolution", "E2389"),
        ("module graph", "E2390"),
        ("Core closure", "E2391"),
        ("tier lowering", "E2392"),
    ] {
        assert!(
            rows.iter()
                .any(|row| row.contains(label) && row.contains(code)),
            "project check {fixture_name} missing {label} ({code}):\n{proof}"
        );
    }
}

fn assert_clean_project(name: &str) {
    let output = run_check(name, &["check", "."]);
    assert!(
        output.status.success(),
        "project check {name} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let proof = proof_snapshot(&output, &fixture(name));
    assert_project_proof_rows(name, &proof);
    assert_snapshot(name, "stdout", &proof);
}

#[test]
fn in_package_file_uses_the_owning_project_graph() {
    let output = run_check("frozen_dogfood", &["check", "src/cli/main.jet"]);
    assert!(
        output.status.success(),
        "in-package check failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("scope=project"),
        "in-package check did not use the owning project graph:\n{stdout}"
    );
    let proof = proof_snapshot(&output, &fixture("frozen_dogfood"));
    assert_project_proof_rows("frozen_dogfood in-package", &proof);
    assert_snapshot("frozen_dogfood", "in-package.stdout", &proof);
}


#[test]
fn frozen_dogfood_and_each_project_proof_witness_are_snapshotted() {
    // The frozen package follows dogfood's package.jet -> entry.jet -> nested
    // source entry shape.  The other fixtures isolate the four proof classes:
    // named output entry resolution, a multi-hop module graph, and a direct
    // Core call.  All four clean checks must expose all named proof rows.
    for name in [
        "frozen_dogfood",
        "entry_resolution",
        "module_graph",
        "core_closure",
    ] {
        assert_clean_project(name);
    }
}

#[test]
fn missing_effect_facts_are_a_project_check_diagnostic() {
    let output = run_check("effect_facts", &["check", ".", "--gate=impure=allow"]);
    assert!(!output.status.success(), "E2391 witness unexpectedly passed");
    let stderr = stderr_snapshot(&output, &fixture("effect_facts"));
    assert!(stderr.contains("Error [E2391]"), "missing E2391:\n{stderr}");
    assert_snapshot("effect_facts", "stderr", &stderr);
}

#[test]
fn unsupported_web_tier_is_not_reported_clean() {
    let output = run_check("tier_lowering", &["check", ".", "--target=web"]);
    assert!(!output.status.success(), "E2392 witness unexpectedly passed");
    let stderr = stderr_snapshot(&output, &fixture("tier_lowering"));
    assert!(stderr.contains("Error [E2392]"), "missing E2392:\n{stderr}");
    assert_snapshot("tier_lowering", "stderr", &stderr);
}

#[test]
fn isolated_project_import_teaches_missing_context() {
    let output = run_check("missing_context", &["check", "orphan.jet"]);
    assert!(!output.status.success(), "E2393 witness unexpectedly passed");
    let stderr = stderr_snapshot(&output, &fixture("missing_context"));
    assert!(stderr.contains("Error [E2393]"), "missing E2393:\n{stderr}");
    assert!(
        stderr.contains("use project.<module>"),
        "missing canonical import teaching:\n{stderr}"
    );
    assert_snapshot("missing_context", "stderr", &stderr);
}
