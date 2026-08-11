//! End-to-end coverage for the `jet:` self-toolchain pin (D-JPK-TOOLCHAIN1=A,
//! card #179, U30). Drives the real `jet` binary through the verbs
//! (`init` / `toolchain` / `update jet`) and the version-dispatch guard on
//! `jet build`, exercising the E1249–E1252 diagnostics.
//!
//! DISTINCT from the Rust bridge build toolchain (D-JPK-BUILDTOOL1); this is
//! the Jet compiler pin.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn scratch(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "jet_toolchain_pin_{tag}_{}_{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&base).unwrap();
    base
}

fn run(args: &[&str], cwd: &Path) -> std::process::Output {
    Command::new(jet_bin())
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("jet binary should run")
}

fn write(dir: &Path, name: &str, body: &str) {
    std::fs::write(dir.join(name), body).unwrap();
}

/// `jet init` writes a `jet:` pin; `jet self toolchain` reports it; `jet update jet`
/// moves the lock's `[[toolchain]]` record.
#[test]
fn init_report_and_update_roundtrip() {
    let dir = scratch("roundtrip");

    let out = run(&["init"], &dir);
    assert!(out.status.success(), "init failed: {out:?}");
    let manifest = std::fs::read_to_string(dir.join("package.jet")).unwrap();
    assert!(manifest.contains("jet:"), "init wrote no pin:\n{manifest}");

    let out = run(&["self", "toolchain"], &dir);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("pin:"), "toolchain report:\n{text}");

    let out = run(&["update", "jet", "0.4"], &dir);
    assert!(out.status.success(), "update jet failed: {out:?}");
    let lock = std::fs::read_to_string(dir.join(".jet/lock")).unwrap();
    assert!(
        lock.contains("[[toolchain]]"),
        "lock has no toolchain:\n{lock}"
    );
    assert!(lock.contains("channel = \"0.4\""), "lock channel:\n{lock}");
    assert!(
        lock.contains("version = \"0.4.0\""),
        "lock version:\n{lock}"
    );

    // `jet self toolchain` now reports the locked exact version + object id.
    let out = run(&["self", "toolchain"], &dir);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("locked:   0.4.0"), "locked report:\n{text}");
    assert!(text.contains("object:   jet-0.4.0-"), "object id:\n{text}");

    std::fs::remove_dir_all(&dir).ok();
}

/// A malformed `jet:` pin (a version range, not a channel) is `E1249`, caught
/// by the dispatch guard before the build runs.
#[test]
fn bad_pin_reports_e1249() {
    let dir = scratch("badpin");
    // A malformed channel ref (not a version series, not a named channel). A
    // range form like `>=9` is instead the legacy E1208 compat constraint.
    write(
        &dir,
        "package.jet",
        "name: \"x\"\nversion: \"1\"\njet: 1.x\n",
    );
    write(&dir, "main.jet", "module x { fn run() { } }\n");
    let out = run(&["build", "main.jet"], &dir);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("E1249"), "expected E1249:\n{err}");
    assert!(!out.status.success());
    std::fs::remove_dir_all(&dir).ok();
}

/// An unlocked channel pin under `--offline` is `E1250` — offline can't resolve
/// a channel with no lock entry.
#[test]
fn offline_unlocked_channel_reports_e1250() {
    let dir = scratch("offline");
    // pin a channel other than the running toolchain's, with no lock present
    write(
        &dir,
        "package.jet",
        "name: \"x\"\nversion: \"1\"\njet: 0.9\n",
    );
    write(&dir, "main.jet", "module x { fn run() { } }\n");
    let out = run(&["build", "main.jet", "--offline"], &dir);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("E1250"), "expected E1250:\n{err}");
    std::fs::remove_dir_all(&dir).ok();
}

/// A version-mismatched pin whose prebuilt object isn't available for this
/// platform is `E1251` — never a source build, never a silent wrong `jet`.
#[test]
fn platform_miss_reports_e1251() {
    let dir = scratch("miss");
    // 0.9 differs from the running 1.x channel; online, no fixture object.
    write(
        &dir,
        "package.jet",
        "name: \"x\"\nversion: \"1\"\njet: 0.9\n",
    );
    write(&dir, "main.jet", "module x { fn run() { } }\n");
    let out = run(&["build", "main.jet"], &dir);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("E1251"), "expected E1251:\n{err}");
    std::fs::remove_dir_all(&dir).ok();
}

/// `jet init` refuses to clobber an existing manifest — `E1252`.
#[test]
fn init_refuses_to_clobber_reports_e1252() {
    let dir = scratch("clobber");
    write(
        &dir,
        "package.jet",
        "name: \"x\"\nversion: \"1\"\n",
    );
    let out = run(&["init"], &dir);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("E1252"), "expected E1252:\n{err}");
    assert!(!out.status.success());
    std::fs::remove_dir_all(&dir).ok();
}

/// A project pinned to the running toolchain's own channel builds natively —
/// no handoff, and no `.jet/lock` written into the tree by a plain build.
#[test]
fn in_channel_pin_runs_native_without_writing_lock() {
    let dir = scratch("native");
    let running_channel = {
        let v = env!("CARGO_PKG_VERSION");
        let mut it = v.split('.');
        format!("{}.{}", it.next().unwrap(), it.next().unwrap())
    };
    write(
        &dir,
        "package.jet",
        &format!("name: \"x\"\nversion: \"1\"\njet: {running_channel}\n"),
    );
    write(&dir, "main.jet", "fn run() { print(\"ok\") }\n");
    let out = run(&["build", "main.jet"], &dir);
    assert!(out.status.success(), "native build failed: {out:?}");
    assert!(
        !dir.join(".jet/lock").exists(),
        "a plain build must not write a toolchain lock into the tree"
    );
    std::fs::remove_dir_all(&dir).ok();
}
