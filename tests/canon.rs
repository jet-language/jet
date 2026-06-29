//! Golden test for `examples/canon.jet` — the compiling syntax showcase (I5).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn canon_compiles_and_runs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = PathBuf::from(env!("CARGO_BIN_EXE_jet"));
    assert!(jet.exists(), "build the jet binary first (cargo build)");

    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping canon golden run");
        return;
    }

    let tool = root.join("examples/canon.jet");
    let out = Command::new(&jet)
        .arg("run")
        .arg(&tool)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "canon.jet failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let expected = fs::read_to_string(root.join("tests/fixtures/canon/expected.out"))
        .expect("tests/fixtures/canon/expected.out");
    let actual = String::from_utf8_lossy(&out.stdout);
    assert_eq!(actual, expected);
}
