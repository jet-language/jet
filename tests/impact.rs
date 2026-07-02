//! D-IMPACT1 integration tests for blast-radius queries.

use std::path::PathBuf;

#[test]
fn impact_report_upstream_main() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_jet"));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/effects/effects.jet");
    let out = std::process::Command::new(bin)
        .args([
            "impact",
            path.to_str().unwrap(),
            "report",
            "--depth=3",
        ])
        .output()
        .expect("jet impact");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("blast radius for `report`"));
    assert!(text.contains("upstream"));
}

#[test]
fn impact_json_output() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_jet"));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/effects/effects.jet");
    let out = std::process::Command::new(bin)
        .args([
            "impact",
            path.to_str().unwrap(),
            "square",
            "--json",
            "--depth=2",
        ])
        .output()
        .expect("jet impact --json");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("\"symbol\":\"square\""));
    assert!(text.contains("\"found\":true"));
}

#[test]
fn impact_unknown_symbol_exits_error() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_jet"));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/effects/effects.jet");
    let out = std::process::Command::new(bin)
        .args(["impact", path.to_str().unwrap(), "not_a_real_symbol_xyz"])
        .output()
        .expect("jet impact missing symbol");
    assert!(!out.status.success());
}
