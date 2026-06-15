//! M6 phase 4: `jet build --small` produces a smaller binary than default (S15).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

#[test]
fn small_profile_binary_is_smaller_than_default() {
    let jet = jet_bin();
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc || !jet.exists() {
        eprintln!("note: skipping --small size test (need jet + rustc)");
        return;
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("examples/features/16_wordcount.jet");
    assert!(
        example.is_file(),
        "examples/features/16_wordcount.jet must exist"
    );

    let dir = std::env::temp_dir().join(format!("jet_small_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(dir.join("build")).unwrap();

    let build_default = Command::new(&jet)
        .args(["build", example.to_str().unwrap()])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        build_default.status.success(),
        "default build failed:\n{}",
        String::from_utf8_lossy(&build_default.stderr)
    );
    fs::rename(
        dir.join("build/16_wordcount"),
        dir.join("build/16_wordcount_default"),
    )
    .unwrap();

    let build_small = Command::new(&jet)
        .args(["build", "--small", example.to_str().unwrap()])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        build_small.status.success(),
        "--small build failed:\n{}",
        String::from_utf8_lossy(&build_small.stderr)
    );
    fs::rename(
        dir.join("build/16_wordcount"),
        dir.join("build/16_wordcount_small"),
    )
    .unwrap();

    let default_size = fs::metadata(dir.join("build/16_wordcount_default"))
        .unwrap()
        .len();
    let small_size = fs::metadata(dir.join("build/16_wordcount_small"))
        .unwrap()
        .len();

    assert!(
        small_size < default_size,
        "--small binary ({small_size} bytes) should be smaller than default ({default_size} bytes)"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// SL9: `01_hello.jet --small` stays under a pinned cross-platform byte budget.
#[test]
fn hello_world_small_binary_stays_under_budget() {
    let jet = jet_bin();
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc || !jet.exists() {
        eprintln!("note: skipping hello size budget test (need jet + rustc)");
        return;
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("examples/01_hello.jet");
    assert!(example.is_file(), "examples/01_hello.jet must exist");

    let dir = std::env::temp_dir().join(format!("jet_hello_budget_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(dir.join("build")).unwrap();
    fs::copy(&example, dir.join("01_hello.jet")).unwrap();

    let build = Command::new(&jet)
        .args(["build", "--small", "01_hello.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "--small hello build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let size = fs::metadata(dir.join("build/01_hello")).unwrap().len();
    const MAX_HELLO_SMALL_BYTES: u64 = 512_000;
    assert!(
        size <= MAX_HELLO_SMALL_BYTES,
        "01_hello --small binary ({size} bytes) exceeds budget ({MAX_HELLO_SMALL_BYTES} bytes)"
    );

    let _ = fs::remove_dir_all(&dir);
}
