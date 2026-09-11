//! D-NEVER2=B: one declared Never return contract keeps its meaning on every
//! hosted execution tier, including `jet eval`.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const EXPECTED: &[u8] = b"41\n42\n";

#[test]
fn declared_never_example_is_byte_identical_on_aot_jit_and_interpreter() {
    tir_support::assert_example_cli_tiers_agree(
        "functions/never_return",
        std::str::from_utf8(EXPECTED).expect("Never example golden is UTF-8"),
    );
}

#[test]
fn declared_never_example_is_byte_identical_through_eval() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("examples/features/functions/never_return.jet");
    assert!(source.is_file(), "missing Never return example: {}", source.display());

    let scratch = common::test_scratch_root("never_return").join(format!("eval-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["eval", source.to_str().expect("example path is UTF-8")])
        .current_dir(&root)
        .env("JET_STORE_DIR", scratch.join("store"))
        .env("JETPACK_ROOT", scratch.join("jetpack"))
        .env("JETPACK_ENV", "1")
        .env("NO_COLOR", "1")
        .output()
        .expect("jet eval must start");
    let _ = fs::remove_dir_all(&scratch);

    assert!(
        output.status.success(),
        "jet eval failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, EXPECTED, "jet eval output changed");
    assert!(
        output.stderr.is_empty(),
        "jet eval emitted unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
