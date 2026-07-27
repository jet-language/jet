//! D-BROWSER-AUTO1=A (#1187): locked browser provisioning proof.
//!
//! Jetpack pins exact browser binaries in `.jet/lock`. `core.browser.locked`
//! reads that pin through the product path with no skip/fallback.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn jetpack_bin() -> PathBuf {
    common::jetpack_bin().to_path_buf()
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn temp_root(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("jet-browser-provision-{label}-{nanos}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn write_executable(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

fn run_jetpack(root: &Path, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(jetpack_bin())
        .current_dir(root)
        .args(args)
        .output()
        .expect("jetpack");
    (
        output.status.code().unwrap_or(1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn run_jet_source(root: &Path, source: &str) -> (i32, String, String) {
    let path = root.join("main.jet");
    fs::write(&path, source).unwrap();
    let output = Command::new(jet_bin())
        .current_dir(root)
        .env("JET_PROJECT_ROOT", root)
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("jet run");
    (
        output.status.code().unwrap_or(1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn jetpack_browser_lock_resolve_list_success() {
    let root = temp_root("success");
    let binary = root.join("tools").join("chromium");
    write_executable(
        &binary,
        "#!/bin/sh\necho 'Chromium 131.0.6778.0'\n",
    );

    let (code, _out, err) = run_jetpack(
        &root,
        &[
            "browser",
            "lock",
            "chromium",
            "--binary",
            binary.to_str().unwrap(),
            "--version",
            "131.0.6778.0",
            "--protocol",
            "bidi-2025.5",
        ],
    );
    assert_eq!(code, 0, "lock failed: {err}");

    let lock = fs::read_to_string(root.join(".jet/lock")).expect("lock file");
    assert!(lock.contains("[[browser]]"), "{lock}");
    assert!(lock.contains("engine = \"chromium\""), "{lock}");
    assert!(lock.contains("protocol = \"bidi-2025.5\""), "{lock}");
    assert!(lock.contains("output-hash = \"sha256-"), "{lock}");

    let (code, out, err) = run_jetpack(&root, &["browser", "resolve", "chromium"]);
    assert_eq!(code, 0, "resolve failed: {err}");
    assert!(out.contains("engine=chromium"), "{out}");
    assert!(out.contains("protocol=bidi-2025.5"), "{out}");
    assert!(
        out.contains(&format!(
            "binary={}",
            binary.canonicalize().unwrap().display()
        )),
        "{out}"
    );

    let (code, out, err) = run_jetpack(&root, &["browser", "list"]);
    assert_eq!(code, 0, "list failed: {err}");
    assert!(out.contains("chromium"), "{out}");
    assert!(out.contains("bidi-2025.5"), "{out}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn jetpack_browser_reject_unknown_engine_and_missing_binary() {
    let root = temp_root("reject");
    let (code, _out, err) = run_jetpack(
        &root,
        &[
            "browser",
            "lock",
            "edge",
            "--binary",
            root.join("missing").to_str().unwrap(),
        ],
    );
    assert_ne!(code, 0);
    assert!(
        err.contains("not supported") || err.contains("edge"),
        "{err}"
    );

    let (code, _out, err) = run_jetpack(
        &root,
        &[
            "browser",
            "lock",
            "firefox",
            "--binary",
            root.join("no-such-bin").to_str().unwrap(),
        ],
    );
    assert_ne!(code, 0);
    assert!(err.contains("not found") || err.contains("failed"), "{err}");

    let (code, _out, err) = run_jetpack(&root, &["browser", "resolve", "firefox"]);
    assert_ne!(code, 0);
    assert!(
        err.contains("no locked browser") || err.contains("failed"),
        "{err}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn jetpack_browser_hash_drift_is_hostile() {
    let root = temp_root("drift");
    let binary = root.join("chromium");
    write_executable(&binary, "#!/bin/sh\necho Chromium 1\n");
    let (code, _out, err) = run_jetpack(
        &root,
        &[
            "browser",
            "lock",
            "chromium",
            "--binary",
            binary.to_str().unwrap(),
            "--version",
            "1",
        ],
    );
    assert_eq!(code, 0, "{err}");

    write_executable(&binary, "#!/bin/sh\necho Chromium TAMPERED\n");
    let (code, _out, err) = run_jetpack(&root, &["browser", "resolve", "chromium"]);
    assert_ne!(code, 0);
    assert!(
        err.contains("drift") || err.contains("hash") || err.contains("size"),
        "{err}"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn core_browser_locked_reads_project_pin() {
    let root = temp_root("core");
    let binary = root.join("chromium");
    write_executable(&binary, "#!/bin/sh\necho Chromium 140.0\n");
    let (code, _out, err) = run_jetpack(
        &root,
        &[
            "browser",
            "lock",
            "chromium",
            "--binary",
            binary.to_str().unwrap(),
            "--version",
            "140.0",
            "--protocol",
            "bidi-2025.5",
        ],
    );
    assert_eq!(code, 0, "{err}");

    let source = r#"
use core.browser as browser

fn run() =[FS, IO]=> {
    locked :: browser.locked("chromium") ?? panic("missing lock")
    print("engine:{locked.engine()}")
    print("version:{locked.version()}")
    print("protocol:{locked.protocol()}")
    locked.verify() ?? panic("verify")
    print("verified:true")
}
"#;
    let (code, out, err) = run_jet_source(&root, source);
    assert_eq!(code, 0, "jet run failed: {err}\n{out}");
    assert!(out.contains("engine:chromium"), "{out}");
    assert!(out.contains("version:140.0"), "{out}");
    assert!(out.contains("protocol:bidi-2025.5"), "{out}");
    assert!(out.contains("verified:true"), "{out}");

    fs::remove_file(&binary).unwrap();
    let hostile = r#"
use core.browser as browser

fn run() =[FS, IO]=> {
    if browser.locked("chromium") == .Err(_) {
        print("outcome:caught")
    } else {
        print("outcome:ok")
    }
}
"#;
    let (code, out, err) = run_jet_source(&root, hostile);
    assert_eq!(code, 0, "{err}\n{out}");
    assert!(out.contains("outcome:caught"), "{out}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn core_browser_locked_requires_fs_effect() {
    let missing_effect = r#"
use core.browser as browser

fn run() =[IO]=> {
    locked :: browser.locked("chromium") ?? return
}
"#;
    let diags = jet::compile(missing_effect).expect_err("locked requires FS");
    assert!(
        diags
            .iter()
            .any(|diag| diag.code == "E0740" && diag.what.contains("FS")),
        "{diags:?}"
    );
}
