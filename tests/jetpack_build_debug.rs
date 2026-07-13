//! U27 failed-build debuggability process tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::jetpack_bin;

fn jetpack() -> Command {
    Command::new(jetpack_bin())
}

fn jet() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jet"))
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
            "jpk-build-debug-{tag}-{nanos}-{:?}",
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

fn write_project(dir: &Path) {
    let vendor = dir.join("vendor/weirdctl");
    fs::create_dir_all(&vendor).unwrap();
    fs::write(vendor.join("README"), "source exists, binary missing").unwrap();
    fs::write(
        dir.join("env.jet"),
        r#"
module dev {
    env.dev: Env.{
        packages: [
            Pkg.adapt(
                name: "weirdctl",
                source: path@vendor/weirdctl,
                recipe: Recipe.prebuilt(bin: "missing-bin", as: "weirdctl")
            )
        ],
    }
}
"#,
    )
    .unwrap();
}

#[test]
fn failed_adapter_preserves_scratch_and_json_logs() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    write_project(&project.path);

    let out = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1273"), "stderr: {stderr}");
    assert!(stderr.contains("jet logs weirdctl"), "stderr: {stderr}");
    assert!(stderr.contains("--shell-on-fail"), "stderr: {stderr}");

    let logs = jetpack()
        .args(["logs", "weirdctl", "--json", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(logs.status.success());
    let stdout = String::from_utf8_lossy(&logs.stdout);
    assert!(stdout.contains("\"package\":\"weirdctl\""), "{stdout}");
    assert!(stdout.contains("\"status\":\"failed\""), "{stdout}");
    assert!(stdout.contains("\"scratch_dir\":\""), "{stdout}");
    assert!(stdout.contains("missing-bin"), "{stdout}");

    let scratch = root.path.join("hangar/failed-scratch");
    assert!(
        fs::read_dir(&scratch).unwrap().next().is_some(),
        "failed scratch should be preserved under hangar"
    );
}

/// `explain` stayed a flat top-level command (D-CLI-SURFACE3=B did not move
/// it — verified against the `COMMANDS`/inspect-action registry in
/// `crates/jet-cli/src/CLI.rs`: only `inspect explain-build` exists there, bare
/// `explain` is unrelated and still dispatches directly).
#[test]
fn top_level_explain_dispatches_to_jetpack() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    write_project(&project.path);

    let _ = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();

    let explain = jet()
        .args(["explain", "weirdctl", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(explain.status.success());
    let stdout = String::from_utf8_lossy(&explain.stdout);
    assert!(
        stdout.contains("ref      adapt:weirdctl:path@vendor/weirdctl"),
        "{stdout}"
    );
    assert!(stdout.contains("failed"), "{stdout}");
    assert!(stdout.contains("jet logs weirdctl"), "{stdout}");
}

/// D-CLI-SURFACE3=B: `logs` moved under `jet inspect` — bare `jet logs` is
/// now a teaching error (E2101) naming the new spelling.
#[test]
fn bare_jet_logs_is_a_teaching_error_naming_jet_inspect_logs() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    write_project(&project.path);

    let _ = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();

    let logs = jet()
        .args(["logs", "weirdctl", "--json", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(logs.status.code(), Some(2), "bare `jet logs` must be rejected");
    // E2101 in --json mode reports on stdout, not stderr (see cli.rs's
    // every_moved_bare_action_is_e2101_in_human_and_json_modes).
    let stdout = String::from_utf8_lossy(&logs.stdout);
    assert!(stdout.contains("\"code\":\"E2101\""), "stdout: {stdout}");
    assert!(stdout.contains("jet inspect logs"), "stdout: {stdout}");
}

/// `jet inspect logs` is the canonical top-level spelling and still
/// dispatches through to the jetpack build-debug engine.
#[test]
fn jet_inspect_logs_dispatches_to_jetpack() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    write_project(&project.path);

    let _ = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();

    let logs = jet()
        .args(["inspect", "logs", "weirdctl", "--json", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        logs.status.success(),
        "code: {:?} stderr: {} stdout: {}",
        logs.status.code(),
        String::from_utf8_lossy(&logs.stderr),
        String::from_utf8_lossy(&logs.stdout)
    );
    assert!(String::from_utf8_lossy(&logs.stdout).contains("\"steps\""));
}

#[test]
fn shell_on_fail_uses_preserved_scratch_with_fake_shell() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    let marker = root.path.join("shell-marker.txt");
    let shell = root.path.join(if cfg!(windows) {
        "fake-shell.bat"
    } else {
        "fake-shell"
    });
    #[cfg(windows)]
    fs::write(&shell, "cd > %JETPACK_FAILED_SCRATCH_MARKER%\r\n").unwrap();
    #[cfg(not(windows))]
    {
        fs::write(
            &shell,
            "#!/bin/sh\npwd > \"$JETPACK_FAILED_SCRATCH_MARKER\"\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&shell, fs::Permissions::from_mode(0o755)).unwrap();
    }
    write_project(&project.path);

    let out = jetpack()
        .args(["build", "--shell-on-fail", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_SHELL_ON_FAIL", &shell)
        .env("JETPACK_FAILED_SCRATCH_MARKER", &marker)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let cwd = fs::read_to_string(&marker).expect("fake shell should run");
    assert!(cwd.contains("failed-scratch"), "marker: {cwd}");
}
