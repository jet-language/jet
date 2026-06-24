//! M6 phase 2: `jet test` output shape and fail-then-fix flow.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

#[test]
fn jet_test_example_output() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    assert!(jet.exists(), "build the jet binary first (cargo build)");
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping jet test integration");
        return;
    }

    let example = root.join("examples/features/20_tests.jet");
    let out = Command::new(&jet)
        .arg("test")
        .arg(&example)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "jet test examples/features/20_tests.jet failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let expected = fs::read_to_string(root.join("examples/features/expected/20_tests.test.out"))
        .expect("examples/features/expected/20_tests.test.out");
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}

#[test]
fn jet_bench_example_regions() {
    // D-BENCH1: `jet bench` on a file with `#Bench` blocks times each region
    // and reports `<name>  <ns> ns/iter (...)  <ops> ops/sec`. Timing values
    // are non-deterministic, so this asserts structure: every block runs and
    // every name + the ns/iter and ops/sec labels appear.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc || !jet.exists() {
        eprintln!("note: rustc not found; skipping jet bench integration");
        return;
    }

    let example = root.join("examples/features/105_bench.jet");
    let out = Command::new(&jet).arg("bench").arg(&example).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "jet bench examples/features/105_bench.jet failed:\nstdout: {}\nstderr: {}",
        stdout,
        String::from_utf8_lossy(&out.stderr)
    );
    for needle in ["fib(10)", "sum to 100", "ns/iter", "ops/sec"] {
        assert!(
            stdout.contains(needle),
            "bench output missing `{}`:\n{}",
            needle,
            stdout
        );
    }
    // One report line per `#Bench` block.
    assert_eq!(
        stdout.lines().filter(|l| l.contains("ns/iter")).count(),
        2,
        "expected exactly two bench region lines:\n{}",
        stdout
    );
}

#[test]
fn jet_test_fail_then_fixed() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc || !jet.exists() {
        return;
    }

    let fail = root.join("tests/fixtures/test_fail.jet");
    let fixed = root.join("tests/fixtures/test_fail.fixed.jet");

    let bad = Command::new(&jet).arg("test").arg(&fail).output().unwrap();
    assert!(!bad.status.success());
    assert!(
        String::from_utf8_lossy(&bad.stdout).contains("FAIL"),
        "expected a FAIL line, got: {}",
        String::from_utf8_lossy(&bad.stdout)
    );
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("left:"),
        "require_eq should print both values on stderr"
    );

    let good = Command::new(&jet).arg("test").arg(&fixed).output().unwrap();
    assert!(good.status.success());
    assert!(
        String::from_utf8_lossy(&good.stdout).contains("pass"),
        "fixed tests should pass"
    );
}

#[test]
fn jet_new_creates_project() {
    let jet = jet_bin();
    let dir = std::env::temp_dir().join(format!("jet_new_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let name = dir.file_name().unwrap().to_string_lossy();
    let out = Command::new(&jet)
        .arg("new")
        .arg(&*name)
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap();
    assert!(out.status.success(), "jet new failed");
    // M12.1: jet new creates .jet/main.jet (source root is the .jet/ folder).
    assert!(
        dir.join(".jet/main.jet").exists() || dir.join("main.jet").exists(),
        ".jet/main.jet or main.jet must be created by jet new"
    );
    assert!(dir.join(".gitignore").exists());
    let _ = fs::remove_dir_all(&dir);
}
