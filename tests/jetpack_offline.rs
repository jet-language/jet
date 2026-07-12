//! U29 (D-JPK-OFFLINE1=A): offline guarantee process tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::jetpack_bin;

fn jetpack() -> Command {
    Command::new(jetpack_bin())
}

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "jpk-offline-{tag}-{nanos}-{:?}",
            std::thread::current().id()
        ));
        fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_runnable_fixture(fixtures: &Path, out_dir: &Path) {
    fs::create_dir_all(fixtures).unwrap();
    let bin = out_dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let greet = bin.join(if cfg!(windows) { "greet.bat" } else { "greet" });
    #[cfg(windows)]
    fs::write(&greet, "@echo hello from offline cache\r\n").unwrap();
    #[cfg(not(windows))]
    {
        fs::write(&greet, "#!/bin/sh\necho hello from offline cache\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let json = format!(
        "[{{\"outputs\":{{\"out\":{:?}}}}}]",
        out_dir.to_string_lossy()
    );
    fs::write(fixtures.join("nixpkgs-greet.json"), json).unwrap();
}

#[test]
fn offline_build_and_run_use_hangar_cache_with_network_denied() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fixtures");
    let out_dir = Scratch::new("out");
    write_runnable_fixture(&fixtures.path, &out_dir.path);

    let first = jetpack()
        .args([
            "build",
            "nixpkgs:greet",
            "--no-color",
            "--offline",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let second = jetpack()
        .args(["build", "nixpkgs:greet", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_DENY_NETWORK", "1")
        .env_remove("JETPACK_FIXTURES")
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(stderr.contains("cached"), "stderr: {stderr}");
    assert!(!stderr.contains("fixture"), "stderr: {stderr}");

    let run = jetpack()
        .args([
            "run",
            "nixpkgs:greet",
            "--no-color",
            "--offline",
            "--",
            "greet",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_DENY_NETWORK", "1")
        .env_remove("JETPACK_FIXTURES")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.contains("hello from offline cache"),
        "stdout: {stdout}"
    );
}

/// D-JPK-OFFLINE2=B tamper gate: after a first build records the locked
/// identity + closure digest, corrupting the realized closure on disk must make
/// the offline reuse refuse loudly (integrity), naming the artifact — never
/// silently serve the stale/mismatched copy (card #418 trust hole stays closed).
#[test]
fn offline_reuse_refuses_a_tampered_closure() {
    let project = Scratch::new("tamper-project");
    let root = Scratch::new("tamper-root");
    let fixtures = Scratch::new("tamper-fixtures");
    let out_dir = Scratch::new("tamper-out");
    write_runnable_fixture(&fixtures.path, &out_dir.path);

    let first = jetpack()
        .args([
            "build",
            "nixpkgs:greet",
            "--no-color",
            "--offline",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    // Corrupt the realized closure on disk (a partial GC / bit-rot / tamper
    // stand-in): drop the executable the recorded digest covers.
    let greet = out_dir
        .path
        .join("bin")
        .join(if cfg!(windows) { "greet.bat" } else { "greet" });
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(
            out_dir.path.join("bin"),
            fs::Permissions::from_mode(0o755),
        );
    }
    fs::remove_file(&greet).unwrap();

    let second = jetpack()
        .args(["build", "nixpkgs:greet", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_DENY_NETWORK", "1")
        .env_remove("JETPACK_FIXTURES")
        .output()
        .unwrap();
    assert!(
        !second.status.success(),
        "a tampered closure must refuse, not serve stale; stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("greet"),
        "the refusal must name the artifact; stderr: {stderr}"
    );
    assert!(
        !stderr.contains("hello from offline cache"),
        "stale bytes must never be served; stderr: {stderr}"
    );
}

#[test]
fn offline_missing_object_is_loud_e1276() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    let out = jetpack()
        .args(["build", "nixpkgs:greet", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_DENY_NETWORK", "1")
        .env_remove("JETPACK_FIXTURES")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1276"), "stderr: {stderr}");
    assert!(stderr.contains("nixpkgs:greet"), "stderr: {stderr}");
}

#[test]
fn network_class_verbs_refuse_offline_immediately() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    for args in [
        &["update", "--no-color", "--offline"][..],
        &["outdated", "--no-color", "--offline"][..],
        &["add", "nixpkgs:ripgrep", "--no-color", "--offline"][..],
    ] {
        let out = jetpack()
            .args(args)
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &root.path)
            .env("JETPACK_DENY_NETWORK", "1")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "args: {args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("E1276"), "args: {args:?}\nstderr: {stderr}");
        assert!(
            stderr.contains("network-class command"),
            "args: {args:?}\nstderr: {stderr}"
        );
    }
}
