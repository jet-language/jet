//! U12: supervised dev services (card c9jetpackgates).
//!
//! Covers:
//!   * `jetpack services up/health/logs/down` round-trips a fixture daemon;
//!   * `jetpack dev`'s health gate blocks until a service is healthy, and
//!     reports a clean E1261 (not a hang) when it never becomes so;
//!   * a `services:` entry with a field jetpack's dev-runtime tier doesn't
//!     recognize is a clean E1262, both from `jetpack dev` and from
//!     `jetpack services up` directly.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

mod common;
use common::jetpack_bin;

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
            "jpk-services-it-{tag}-{nanos}-{:?}",
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

fn jetpack() -> Command {
    Command::new(jetpack_bin())
}

/// A packageless project (never trust-sensitive) declaring one dev service.
fn write_project(dir: &std::path::Path, services_body: &str, main_src: &str) {
    fs::write(
        dir.join("env.jet"),
        format!("module env.dev {{ services: {{ {services_body} }} }}\n"),
    )
    .unwrap();
    fs::write(dir.join("main.jet"), main_src).unwrap();
}

/// `jetpack services up` starts a fixture "daemon" (a `sleep`), `health`
/// reports it healthy (process-alive contract, no `ports`/`ready`), `logs`
/// captures its stdout, and `down` stops it — the full lifecycle round-trip.
#[test]
fn services_up_health_logs_down_roundtrip() {
    let proj = Scratch::new("roundtrip");
    let root = Scratch::new("roundtrip-root");
    let home = Scratch::new("roundtrip-home");
    write_project(
        &proj.path,
        r#"fixture: { run: ["sh", "-c", "echo fixture-started; sleep 30"] }"#,
        "fn run() {}\n",
    );
    let env = [
        ("JETPACK_ROOT", root.path.display().to_string()),
        ("HOME", home.path.display().to_string()),
    ];

    let up = jetpack()
        .args(["services", "up", "--no-color"])
        .current_dir(&proj.path)
        .envs(env.clone())
        .output()
        .unwrap();
    assert!(
        up.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&up.stderr)
    );

    let health = jetpack()
        .args(["services", "health", "--no-color"])
        .current_dir(&proj.path)
        .envs(env.clone())
        .output()
        .unwrap();
    assert!(
        health.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&health.stderr)
    );

    let logs = jetpack()
        .args(["services", "logs", "fixture", "--no-color"])
        .current_dir(&proj.path)
        .envs(env.clone())
        .output()
        .unwrap();
    let logs_out = String::from_utf8_lossy(&logs.stdout);
    assert!(logs_out.contains("fixture-started"), "logs: {logs_out}");

    let down = jetpack()
        .args(["services", "down", "--no-color"])
        .current_dir(&proj.path)
        .envs(env.clone())
        .output()
        .unwrap();
    assert!(
        down.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&down.stderr)
    );

    // Stopped now — a fresh health check must report unhealthy (exit 1).
    let health_after = jetpack()
        .args(["services", "health", "--no-color"])
        .current_dir(&proj.path)
        .envs(env)
        .output()
        .unwrap();
    assert!(!health_after.status.success());
}

/// `jetpack dev` health-gates on its declared services (U19's `jetpack dev`
/// health-gate plug point, previously a no-op) — it must not run the
/// project's `fn dev()` until the service is healthy.
#[test]
fn dev_health_gate_waits_for_service_before_running() {
    let proj = Scratch::new("health-gate");
    let root = Scratch::new("health-gate-root");
    let home = Scratch::new("health-gate-home");
    write_project(
        &proj.path,
        r#"fixture: { run: ["sleep", "30"] }"#,
        "fn dev() { print(\"DEV-RAN\"); }\n",
    );
    let out = jetpack()
        .args(["dev", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("DEV-RAN"), "stdout: {stdout}");

    // Clean up the fixture daemon `dev` left running.
    jetpack()
        .args(["services", "down", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .ok();
}

/// A service whose readiness contract never passes is a clean E1261 — never a
/// hang. `JETPACK_SERVICE_HEALTH_TIMEOUT_MS` shortens the poll ceiling so this
/// test doesn't wait out the real 15s default.
#[test]
fn dev_service_never_healthy_is_e1261() {
    let proj = Scratch::new("e1261");
    let root = Scratch::new("e1261-root");
    let home = Scratch::new("e1261-home");
    write_project(
        &proj.path,
        // `ready: "false"` never passes (exit 1), so this never reports healthy.
        r#"fixture: { run: ["sleep", "30"], ready: "false" }"#,
        "fn dev() { print(\"DEV-RAN\"); }\n",
    );
    let out = jetpack()
        .args(["dev", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .env("JETPACK_SERVICE_HEALTH_TIMEOUT_MS", "500")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1261"), "stderr: {stderr}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("DEV-RAN"));

    jetpack()
        .args(["services", "down", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .ok();
}

/// A `services:` field jetpack's dev-runtime tier doesn't recognize (a typo,
/// e.g. `prot` instead of `ports`) is a clean E1262 from `jetpack dev` — the
/// `Service` record itself stays open at parse time (U12); this is a
/// supervision-time check, not a field-check-time rejection.
#[test]
fn dev_unrecognized_service_field_is_e1262() {
    let proj = Scratch::new("e1262");
    let root = Scratch::new("e1262-root");
    let home = Scratch::new("e1262-home");
    write_project(
        &proj.path,
        r#"fixture: { run: ["sleep", "30"], prot: 5432 }"#,
        "fn dev() { print(\"DEV-RAN\"); }\n",
    );
    let out = jetpack()
        .args(["dev", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1262"), "stderr: {stderr}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("DEV-RAN"));
}

/// Same E1262 from `jetpack services up` directly (not just the `dev` gate).
#[test]
fn services_up_unrecognized_field_is_e1262() {
    let proj = Scratch::new("services-e1262");
    let root = Scratch::new("services-e1262-root");
    let home = Scratch::new("services-e1262-home");
    write_project(
        &proj.path,
        r#"fixture: { run: ["sleep", "30"], prot: 5432 }"#,
        "fn run() {}\n",
    );
    let out = jetpack()
        .args(["services", "up", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1262"), "stderr: {stderr}");
}

/// A directory watch is recursive and uses the same supervisor as an
/// unexpected process exit. The real CLI path must restart the child after a
/// nested source-file change and leave the new child running.
#[test]
fn services_watch_restarts_after_nested_file_change() {
    let proj = Scratch::new("watch-nested");
    let root = Scratch::new("watch-nested-root");
    let home = Scratch::new("watch-nested-home");
    fs::create_dir_all(proj.path.join("src/nested")).unwrap();
    fs::write(proj.path.join("src/nested/input"), "one\n").unwrap();
    write_project(
        &proj.path,
        r#"fixture: { run: ["sh", "-c", "echo started >> restart-count; sleep 30"], watch: ["src"] }"#,
        "fn run() {}\n",
    );
    let env = [
        ("JETPACK_ROOT", root.path.display().to_string()),
        ("HOME", home.path.display().to_string()),
    ];
    let up = jetpack()
        .args(["services", "up", "--no-color"])
        .current_dir(&proj.path)
        .envs(env.clone())
        .output()
        .unwrap();
    assert!(up.status.success(), "{}", String::from_utf8_lossy(&up.stderr));

    let count = proj
        .path
        .join(".jet/services/fixture/data/restart-count");
    wait_for_file_lines(&count, 1);
    fs::write(proj.path.join("src/nested/input"), "two with a changed length\n").unwrap();
    wait_for_file_lines(&count, 2);

    let health = jetpack()
        .args(["services", "health", "--no-color"])
        .current_dir(&proj.path)
        .envs(env.clone())
        .output()
        .unwrap();
    assert!(health.status.success(), "{}", String::from_utf8_lossy(&health.stderr));
    let down = jetpack()
        .args(["services", "down", "--no-color"])
        .current_dir(&proj.path)
        .envs(env)
        .output()
        .unwrap();
    assert!(down.status.success(), "{}", String::from_utf8_lossy(&down.stderr));
}

/// Exhausting an explicit restart budget is a terminal state, not a stale
/// PID that later commands could mistake for a live service.
#[test]
fn services_restart_exhaustion_cleans_state() {
    let proj = Scratch::new("restart-exhaustion");
    let root = Scratch::new("restart-exhaustion-root");
    let home = Scratch::new("restart-exhaustion-home");
    write_project(
        &proj.path,
        r#"fixture: { run: ["sh", "-c", "exit 7"], restart: .OnFailure.{ max: 1, backoff_ms: 1 } }"#,
        "fn run() {}\n",
    );
    let env = [
        ("JETPACK_ROOT", root.path.display().to_string()),
        ("HOME", home.path.display().to_string()),
        ("JETPACK_SERVICE_HEALTH_TIMEOUT_MS", "500".to_string()),
    ];
    let up = jetpack()
        .args(["services", "up", "--no-color"])
        .current_dir(&proj.path)
        .envs(env.clone())
        .output()
        .unwrap();
    assert!(!up.status.success(), "{}", String::from_utf8_lossy(&up.stderr));
    assert!(
        String::from_utf8_lossy(&up.stderr).contains("E1261"),
        "stderr: {}",
        String::from_utf8_lossy(&up.stderr)
    );
    let pid = proj.path.join(".jet/services/fixture/pid");
    wait_for_missing(&pid);
    assert!(
        proj.path.join(".jet/services/fixture/supervisor.error").is_file(),
        "restart exhaustion must leave a supervisor error"
    );
    let health = jetpack()
        .args(["services", "health", "--no-color"])
        .current_dir(&proj.path)
        .envs(env.clone())
        .output()
        .unwrap();
    assert!(!health.status.success());
    jetpack()
        .args(["services", "down", "--no-color"])
        .current_dir(&proj.path)
        .envs(env)
        .output()
        .ok();
    assert!(!proj.path.join(".jet/services/fixture/.stopping").exists());
}

fn wait_for_file_lines(path: &std::path::Path, minimum: usize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let lines = fs::read_to_string(path)
            .map(|text| text.lines().count())
            .unwrap_or(0);
        if lines >= minimum {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {} lines in {}",
            minimum,
            path.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_missing(path: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {} to disappear",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
