//! #2168 criterion 8: native `jet dev --canvas` owns one resident lifecycle.

mod common;

use std::path::PathBuf;
use std::process::Command;

#[test]
fn jet_dev_canvas_lifecycle_exit_reuse_and_cleanup() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let node = std::env::var_os("JET_CANVAS_NODE").unwrap_or_else(|| "node".into());
    let output = Command::new(node)
        .current_dir(&repo)
        .env("JET_BIN", env!("CARGO_BIN_EXE_jet"))
        .env(
            "JET_SOURCE",
            "examples/features/tooling/canvas_blueprint_demo.jet",
        )
        .env("TMPDIR", "/home/nate/.cache/jet-test-scratch")
        .arg("scripts/canvas-test/native-lifecycle.mjs")
        .output()
        .expect("run native Canvas lifecycle probe");
    assert!(
        output.status.success(),
        "native Canvas lifecycle failed\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("PASS native jet dev --canvas lifecycle"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}
