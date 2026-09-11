mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::path::PathBuf;
use std::process::Command;

const SOURCE_PATH: &str = "examples/features/operators/mixed_types.jet";
const SOURCE: &str = include_str!("../examples/features/operators/mixed_types.jet");
const EXPECTED: &str = include_str!("../examples/features/expected/operators/mixed_types.out");

#[test]
fn mixed_operator_matrix_agrees_across_execution_tiers() {
    tir_support::assert_example_cli_tiers_agree("operators/mixed_types", EXPECTED);

    let (code, stdout, stderr) = tir_support::jit_run_traced("operator_matrix_native", SOURCE);
    assert_eq!(code, 0, "default `jet run` failed for {SOURCE_PATH}: {stderr}");
    assert_eq!(stdout, EXPECTED);
    assert!(
        stderr
            .lines()
            .any(|line| line.starts_with("run") && line.contains("tier1 native")),
        "{SOURCE_PATH} did not execute on the resident JIT: {stderr}"
    );
}


#[test]
fn mixed_operator_cli_types_projection_lists_reverse_owner() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join(SOURCE_PATH);
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["inspect", "types", path.to_str().unwrap()])
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .output()
        .expect("run `jet inspect types`");
    assert_eq!(
        output.status.code(),
        Some(0),
        "`jet inspect types` failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Int * Money -> Money")
            && stdout.contains("mirror of Money.Mul(Int)"),
        "types projection omitted the Money mirror under its owning type:\n{stdout}"
    );
}
