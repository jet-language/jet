//! U12: supervised dev services (card c9jetpackgates).
//!
//! Covers:
//!   * `jetpack services up/health/logs/down` round-trips a fixture daemon;
//!   * `jetpack dev`'s health gate blocks until a service is healthy, and
//!     reports a clean E1261 (not a hang) when it never becomes so;
//!   * a `services:` entry with a field jetpack's dev-runtime tier doesn't
//!     recognize is a clean E1262, both from `jetpack dev` and from
//!     `jetpack services up` directly.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use std::sync::OnceLock;

#[cfg(target_os = "linux")]
use jet_env_model::ModuleEval::{DevServicePlan, PromptPathMode, PromptStripMode};
#[cfg(target_os = "linux")]
use jetpack::Shell::Env as ShellEnv;

mod common;
use common::{jetpack_bin, Scratch};

fn jetpack() -> Command {
    let mut command = Command::new(jetpack_bin());
    #[cfg(target_os = "linux")]
    {
        let fake = fake_systemd();
        let current_path = std::env::var_os("PATH").unwrap_or_default();
        let path = format!(
            "{}:{}",
            fake.bin.display(),
            current_path.to_string_lossy()
        );
        command
            .env("PATH", path)
            .env("JETPACK_FAKE_SYSTEMD_STATE", &fake.state);
    }
    command
}

#[cfg(target_os = "linux")]
struct FakeSystemd {
    bin: PathBuf,
    state: PathBuf,
}

#[cfg(target_os = "linux")]
fn fake_systemd() -> &'static FakeSystemd {
    static FAKE: OnceLock<FakeSystemd> = OnceLock::new();
    FAKE.get_or_init(|| {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "jpk-fake-systemd-{}",
            std::process::id()
        ));
        let bin = root.join("bin");
        let state = root.join("state");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&state).unwrap();
        let systemd_run = r##"#!/bin/sh
set -eu
state=${JETPACK_FAKE_SYSTEMD_STATE:-$(dirname "$0")/../state}
unit=
workdir=
saw_user=0
saw_scope=0
saw_collect=0
saw_quiet=0
saw_delegate=0
saw_kill_mode=0
while [ "$#" -gt 0 ]; do
    case "$1" in
        --user) saw_user=1 ;;
        --scope) saw_scope=1 ;;
        --collect) saw_collect=1 ;;
        --quiet) saw_quiet=1 ;;
        --property=Delegate=yes) saw_delegate=1 ;;
        --property=KillMode=control-group) saw_kill_mode=1 ;;
        --unit=*) unit=${1#--unit=} ;;
        --working-directory=*) workdir=${1#--working-directory=} ;;
        --setenv=*) export "${1#--setenv=}" ;;
        --unsetenv=*) unset "${1#--unsetenv=}" ;;
        --) shift; break ;;
    esac
    shift
done
[ -n "$unit" ]
[ "$saw_user" -eq 1 ]
[ "$saw_scope" -eq 1 ]
[ "$saw_collect" -eq 1 ]
[ "$saw_quiet" -eq 1 ]
[ "$saw_delegate" -eq 1 ]
[ "$saw_kill_mode" -eq 1 ]
mkdir -p "$state"
printf '%s\n' "$$" > "$state/$unit.pid"
[ -z "$workdir" ] || cd "$workdir"
exec "$@"
"##;
        let systemd_run_path = bin.join("systemd-run");
        fs::write(&systemd_run_path, systemd_run).unwrap();
        let mut permissions = fs::metadata(&systemd_run_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&systemd_run_path, permissions).unwrap();

        let systemctl = r##"#!/bin/sh
set -eu
state=${JETPACK_FAKE_SYSTEMD_STATE:-$(dirname "$0")/../state}
signal=TERM
unit=
operation=kill
for arg in "$@"; do
    case "$arg" in
        is-active) operation=active ;;
        --signal=*) signal=${arg#--signal=} ;;
        *.scope) unit=$arg ;;
    esac
done
[ -n "$unit" ]
pid=$(cat "$state/$unit.pid")
if [ "$operation" = active ]; then
    for stat in /proc/[0-9]*/stat; do
        [ -r "$stat" ] || continue
        line=$(cat "$stat") || continue
        rest=${line##*)}
        set -- $rest
        state_code=$1
        process_group=$3
        if [ "$process_group" = "$pid" ] && [ "$state_code" != Z ]; then
            exit 0
        fi
    done
    exit 3
fi
kill "-$signal" -- "-$pid" 2>/dev/null || kill "-$signal" "$pid" 2>/dev/null || true
exit 0
"##;
        let systemctl_path = bin.join("systemctl");
        fs::write(&systemctl_path, systemctl).unwrap();
        let mut permissions = fs::metadata(&systemctl_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&systemctl_path, permissions).unwrap();
        FakeSystemd { bin, state }
    })
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

#[test]
fn services_from_nested_project_directory_use_the_project_state_root() {
    let proj = Scratch::new("nested-services");
    let root = Scratch::new("nested-services-root");
    let home = Scratch::new("nested-services-home");
    write_project(
        &proj.path,
        r#"fixture: { run: ["sh", "-c", "echo nested-started; sleep 30"] }"#,
        "fn run() {}\n",
    );
    let nested = proj.path.join("src/nested");
    fs::create_dir_all(&nested).unwrap();
    let env = [
        ("JETPACK_ROOT", root.path.display().to_string()),
        ("HOME", home.path.display().to_string()),
    ];

    let up = jetpack()
        .args(["services", "up", "--no-color"])
        .current_dir(&nested)
        .envs(env.clone())
        .output()
        .unwrap();
    assert!(up.status.success(), "{}", String::from_utf8_lossy(&up.stderr));
    assert!(proj.path.join(".jet/services/fixture/pid").is_file());
    assert!(!nested.join(".jet/services/fixture/pid").exists());

    let health = jetpack()
        .args(["services", "health", "--no-color"])
        .current_dir(&nested)
        .envs(env.clone())
        .output()
        .unwrap();
    assert!(health.status.success(), "{}", String::from_utf8_lossy(&health.stderr));

    let down = jetpack()
        .args(["services", "down", "--no-color"])
        .current_dir(&nested)
        .envs(env)
        .output()
        .unwrap();
    assert!(down.status.success(), "{}", String::from_utf8_lossy(&down.stderr));
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

#[cfg(target_os = "linux")]
fn process_is_alive(pid: u32) -> bool {
    if fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| stat.rsplit_once(')').map(|(_, rest)| rest.to_string()))
        .and_then(|rest| rest.split_whitespace().next().map(str::to_string))
        .as_deref()
        == Some("Z")
    {
        return false;
    }
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
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

fn wait_for_file_contains(path: &std::path::Path, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if fs::read_to_string(path)
            .map(|text| text.contains(expected))
            .unwrap_or(false)
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for `{expected}` in {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(target_os = "linux")]
fn write_executable(path: &std::path::Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

#[cfg(target_os = "linux")]
fn catalog_tool_env(root: &std::path::Path, systemd_bin: Option<&std::path::Path>) -> ShellEnv {
    let mut bin_dirs = vec![root.join("bin").display().to_string()];
    if let Some(systemd_bin) = systemd_bin {
        bin_dirs.push(systemd_bin.display().to_string());
    }
    ShellEnv {
        bin_dirs,
        vars: BTreeMap::new(),
        unset_vars: Vec::new(),
        refs: Vec::new(),
        label: "catalog-test".to_string(),
        prompt_path: PromptPathMode::Short,
        prompt_strip: PromptStripMode::Off,
        cache_leases: Vec::new(),
    }
}

#[cfg(target_os = "linux")]
fn install_catalog_tools(root: &std::path::Path) {
    fs::create_dir_all(root.join("bin")).unwrap();
    let tool = r##"#!/bin/sh
set -eu
name=${0##*/}
case "$name" in
initdb)
    data=
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --pgdata) shift; data=$1 ;;
            --pgdata=*) data=${1#--pgdata=} ;;
        esac
        shift
    done
    [ -n "$data" ]
    mkdir -p "$data"
    printf '16\n' > "$data/PG_VERSION"
    ;;
redis-server)
    data=
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --dir) shift; data=$1 ;;
        esac
        shift
    done
    [ -n "$data" ]
    mkdir -p "$data"
    : > "$data/.service-running"
    exec sleep 30
    ;;
mysqld)
    if [ "${1:-}" = "--initialize-insecure" ]; then
        data=
        for arg in "$@"; do
            case "$arg" in --datadir=*) data=${arg#--datadir=} ;; esac
        done
        [ -n "$data" ]
        mkdir -p "$data/mysql"
    else
        data=
        for arg in "$@"; do
            case "$arg" in --datadir=*) data=${arg#--datadir=} ;; esac
        done
        [ -n "$data" ]
        : > "$data/.service-running"
        exec sleep 30
    fi
    ;;
mariadb-install-db)
    data=
    for arg in "$@"; do
        case "$arg" in --datadir=*) data=${arg#--datadir=} ;; esac
    done
    [ -n "$data" ]
    mkdir -p "$data/mysql"
    ;;
postgres)
    data=
    while [ "$#" -gt 0 ]; do
        case "$1" in -D) shift; data=$1 ;; esac
        shift
    done
    [ -n "$data" ]
    : > "$data/.service-running"
    exec sleep 30
    ;;
mariadbd)
    data=
    for arg in "$@"; do
        case "$arg" in --datadir=*) data=${arg#--datadir=} ;; esac
    done
    [ -n "$data" ]
    : > "$data/.service-running"
    exec sleep 30
    ;;
minio)
    [ "${1:-}" = "server" ]
    data=${2:-}
    [ -n "$data" ]
    : > "$data/.service-running"
    exec sleep 30
    ;;
mailpit)
    database=
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --database) shift; database=$1 ;;
        esac
        shift
    done
    [ -n "$database" ]
    : > "$(dirname "$database")/.service-running"
    exec sleep 30
    ;;
adminer)
    root=
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --root) shift; root=$1 ;;
        esac
        shift
    done
    [ -n "$root" ]
    : > "$root/.service-running"
    exec sleep 30
    ;;
nginx)
    prefix=
    config=
    while [ "$#" -gt 0 ]; do
        case "$1" in
            -p) shift; prefix=$1 ;;
            -c) shift; config=$1 ;;
        esac
        shift
    done
    [ -n "$prefix" ]
    [ -f "$config" ]
    grep -F 'listen 127.0.0.1:' "$config" >/dev/null
    : > "$prefix/.service-running"
    exec sleep 30
    ;;
pg_isready)
    [ -f .jet/services/postgres/data/.service-running ]
    ;;
redis-cli)
    [ -f .jet/services/redis/data/.service-running ]
    ;;
mysqladmin)
    [ -f .jet/services/mysql/data/.service-running ]
    ;;
mariadb-admin)
    [ -f .jet/services/mariadb/data/.service-running ]
    ;;
curl)
    url=
    for arg in "$@"; do url=$arg; done
    case "$url" in
        */minio/health/live) [ -f .jet/services/minio/data/.service-running ] ;;
        */api/v1/info) [ -f .jet/services/mailpit/data/.service-running ] ;;
        *)
            healthy=1
            matched=0
            for service in nginx adminer; do
                port=$(cat ".jet/services/$service/ports" 2>/dev/null || true)
                [ -n "$port" ] || continue
                case "$url" in *":$port/"*)
                    matched=1
                    if [ "$service" = nginx ]; then
                        nginx_marker=
                        for marker in .jet/services/nginx/data/nginx-*/.service-running; do
                            [ -f "$marker" ] || continue
                            nginx_marker=1
                            break
                        done
                        [ -n "$nginx_marker" ] || healthy=0
                    else
                        [ -f .jet/services/adminer/data/.service-running ] || healthy=0
                    fi
                esac
            done
            [ "$matched" -eq 1 ]
            [ "$healthy" -eq 1 ]
            ;;
    esac
    ;;
*)
    exit 1
    ;;
esac
"##;
    for name in [
        "initdb",
        "redis-server",
        "mysqld",
        "mariadb-install-db",
        "postgres",
        "mariadbd",
        "minio",
        "mailpit",
        "adminer",
        "nginx",
        "pg_isready",
        "redis-cli",
        "mysqladmin",
        "mariadb-admin",
        "curl",
    ] {
        write_executable(&root.join("bin").join(name), tool);
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore]
fn production_catalog_service_worker() {
    let project = PathBuf::from(std::env::var_os("JETPACK_CATALOG_PROJECT").unwrap());
    let tools = PathBuf::from(std::env::var_os("JETPACK_CATALOG_TOOLS").unwrap());
    let systemd_bin = PathBuf::from(std::env::var_os("JETPACK_CATALOG_SYSTEMD_BIN").unwrap());
    let name = std::env::var("JETPACK_CATALOG_NAME").unwrap();
    let plan = DevServicePlan {
        name,
        enable: true,
        ports: vec![0],
        ..Default::default()
    };
    let env = catalog_tool_env(&tools, Some(&systemd_bin));
    jetpack::Services::up_one(&project, &env, &plan).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn production_catalog_services_prepare_state_and_pass_readiness() {
    let proj = Scratch::new("catalog-production");
    let tools = Scratch::new("catalog-production-tools");
    install_catalog_tools(&tools.path);
    let fake = fake_systemd();
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let path = format!(
        "{}:{}",
        fake.bin.display(),
        current_path.to_string_lossy()
    );
    let env = catalog_tool_env(&tools.path, None);

    for name in [
        "redis",
        "postgres",
        "mysql",
        "mariadb",
        "nginx",
        "minio",
        "mailpit",
        "adminer",
    ] {
        let plan = DevServicePlan {
            name: name.to_string(),
            enable: true,
            ports: vec![0],
            ..Default::default()
        };
        let supervisor = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "production_catalog_service_worker",
                "--ignored",
                "--nocapture",
            ])
            .env("JETPACK_CATALOG_PROJECT", &proj.path)
            .env("JETPACK_CATALOG_TOOLS", &tools.path)
            .env("JETPACK_CATALOG_SYSTEMD_BIN", &fake.bin)
            .env("JETPACK_CATALOG_NAME", name)
            .env("JETPACK_SERVICE_SUPERVISOR", "1")
            .env("PATH", &path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let service_dir = proj.path.join(".jet/services").join(name);
        wait_for_file_contains(&service_dir.join("pid"), "pid=");
        assert!(
            jetpack::Services::wait_healthy_with_env(
                &proj.path,
                Some(&env),
                &plan,
                Duration::from_secs(3),
            ),
            "catalog service `{name}` did not pass its production readiness command"
        );

        let data_dir = service_dir.join("data");
        match name {
            "redis" => assert!(data_dir.join(".service-running").is_file()),
            "postgres" => assert!(data_dir.join("PG_VERSION").is_file()),
            "mysql" | "mariadb" => assert!(data_dir.join("mysql").is_dir()),
            "minio" | "mailpit" | "adminer" => {
                assert!(data_dir.join(".service-running").is_file())
            }
            "nginx" => {
                let port = fs::read_to_string(service_dir.join("ports"))
                    .unwrap()
                    .lines()
                    .next()
                    .unwrap()
                    .parse::<u16>()
                    .unwrap();
                let config = data_dir.join(format!("nginx-{port}/conf/nginx.conf"));
                let config_text = fs::read_to_string(config).unwrap();
                assert!(config_text.contains(&format!("listen 127.0.0.1:{port};")));
            }
            _ => unreachable!(),
        }

        fs::write(service_dir.join(".stopping"), b"test\n").unwrap();
        let output = supervisor.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "catalog worker failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        wait_for_missing(&service_dir.join("pid"));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn production_path_persists_linux_authority_and_dependency_lifecycle() {
    let proj = Scratch::new("authority-lifecycle");
    let root = Scratch::new("authority-lifecycle-root");
    let home = Scratch::new("authority-lifecycle-home");
    write_project(
        &proj.path,
        r#"database: { run: ["sleep", "30"] }, api: { run: ["sleep", "30"], after: ["database"] }"#,
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

    let lifecycle = fs::read_to_string(proj.path.join(".jet/services/api/lifecycle")).unwrap();
    assert!(lifecycle.contains("backend=linux-systemd-user"), "{lifecycle}");
    assert!(lifecycle.contains("containment=delegated-cgroup"), "{lifecycle}");
    assert!(lifecycle.contains("phase=ready"), "{lifecycle}");
    assert!(lifecycle.contains("dependency=database"), "{lifecycle}");

    let health = jetpack()
        .args(["services", "health", "--json", "--no-color"])
        .current_dir(&proj.path)
        .envs(env.clone())
        .output()
        .unwrap();
    assert!(health.status.success(), "{}", String::from_utf8_lossy(&health.stderr));
    let json = String::from_utf8_lossy(&health.stdout);
    assert!(json.contains("linux-systemd-user"), "{json}");
    assert!(json.contains("\"after\":[\"database\"]"), "{json}");

    let down = jetpack()
        .args(["services", "down", "--no-color"])
        .current_dir(&proj.path)
        .envs(env)
        .output()
        .unwrap();
    assert!(down.status.success(), "{}", String::from_utf8_lossy(&down.stderr));
    let stopped = fs::read_to_string(proj.path.join(".jet/services/api/lifecycle")).unwrap();
    assert!(stopped.contains("phase=stopped"), "{stopped}");
}

#[cfg(target_os = "linux")]
#[test]
/// The fake systemd authority checks the production flags and exercises the
/// process-group cleanup path. A real systemd cgroup runtime is outside this
/// fixture's host capability, so this proves descendant cleanup semantics.
fn production_path_kills_descendants_in_the_supervisor_process_group() {
    let proj = Scratch::new("authority-descendants");
    let root = Scratch::new("authority-descendants-root");
    let home = Scratch::new("authority-descendants-home");
    write_project(
        &proj.path,
        r#"fixture: { run: ["sh", "-c", "sleep 30 & child=$!; echo $child > child.pid; wait"] }"#,
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

    let child_file = proj.path.join(".jet/services/fixture/data/child.pid");
    wait_for_file_lines(&child_file, 1);
    let child_pid = fs::read_to_string(&child_file)
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    let service_pid = fs::read_to_string(proj.path.join(".jet/services/fixture/pid"))
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("pid=")?.parse::<u32>().ok())
        .unwrap();
    assert_ne!(child_pid, service_pid, "fixture must have a real descendant");
    assert!(process_is_alive(child_pid), "descendant must be alive before down");

    let down = jetpack()
        .args(["services", "down", "--no-color"])
        .current_dir(&proj.path)
        .envs(env)
        .output()
        .unwrap();
    assert!(down.status.success(), "{}", String::from_utf8_lossy(&down.stderr));
    let deadline = Instant::now() + Duration::from_secs(3);
    while process_is_alive(child_pid) {
        assert!(Instant::now() < deadline, "descendant survived systemd scope shutdown");
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn production_supervisor_stops_descendants_after_leader_exit() {
    let proj = Scratch::new("authority-leader-exit");
    let root = Scratch::new("authority-leader-exit-root");
    let home = Scratch::new("authority-leader-exit-home");
    write_project(
        &proj.path,
        r#"fixture: { run: ["sh", "-c", "sleep 30 & child=$!; echo $child > child.pid; while [ ! -f exit.flag ]; do sleep 0.01; done; exit 0"] }"#,
        "fn run() {}\n",
    );
    let env = [
        ("JETPACK_ROOT", root.path.display().to_string()),
        ("HOME", home.path.display().to_string()),
    ];

    let up = jetpack()
        .args(["services", "up", "--no-color"])
        .current_dir(&proj.path)
        .envs(env)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&up.stderr);
    assert!(up.status.success(), "{stderr}");

    let service_dir = proj.path.join(".jet/services/fixture");
    let child_file = service_dir.join("data/child.pid");
    wait_for_file_lines(&child_file, 1);
    let child_pid = fs::read_to_string(&child_file)
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    assert!(process_is_alive(child_pid), "descendant must outlive the leader");
    fs::write(service_dir.join("data/exit.flag"), b"exit\n").unwrap();

    wait_for_file_contains(&service_dir.join("lifecycle"), "phase=failed");
    let deadline = Instant::now() + Duration::from_secs(3);
    while process_is_alive(child_pid) {
        assert!(
            Instant::now() < deadline,
            "supervisor left a descendant alive after leader exit: child={child_pid}, lifecycle={}, pid_state={}",
            fs::read_to_string(service_dir.join("lifecycle")).unwrap_or_default(),
            fs::read_to_string(service_dir.join("pid")).unwrap_or_default(),
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while service_dir.join("pid").exists() {
        assert!(
            Instant::now() < deadline,
            "supervisor retained PID after cleanup: lifecycle={}",
            fs::read_to_string(service_dir.join("lifecycle")).unwrap_or_default(),
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn production_path_rejects_retired_depends_on_field() {
    let proj = Scratch::new("depends-on-rejected");
    let root = Scratch::new("depends-on-rejected-root");
    let home = Scratch::new("depends-on-rejected-home");
    write_project(
        &proj.path,
        r#"database: { run: ["sleep", "30"] }, api: { run: ["sleep", "30"], depends_on: ["database"] }"#,
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
    assert!(stderr.contains("E1262"), "{stderr}");
    assert!(!proj.path.join(".jet/services/api/pid").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn production_path_bounds_readiness_probe_runtime() {
    let proj = Scratch::new("bounded-readiness-production");
    let root = Scratch::new("bounded-readiness-production-root");
    let home = Scratch::new("bounded-readiness-production-home");
    write_project(
        &proj.path,
        r#"fixture: { run: ["sleep", "30"], ready: "sleep 30" }"#,
        "fn run() {}\n",
    );
    let started = Instant::now();
    let out = jetpack()
        .args(["services", "up", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .env("JETPACK_SERVICE_HEALTH_TIMEOUT_MS", "200")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(started.elapsed() < Duration::from_secs(3), "readiness probe was not bounded: {:?}", started.elapsed());
    assert!(String::from_utf8_lossy(&out.stderr).contains("E1261"));
    assert!(!proj.path.join(".jet/services/fixture/pid").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn production_path_stops_dependents_after_dependency_failure() {
    let proj = Scratch::new("dependency-failure");
    let root = Scratch::new("dependency-failure-root");
    let home = Scratch::new("dependency-failure-home");
    write_project(
        &proj.path,
        r#"database: { run: ["sh", "-c", "sleep 1"] }, api: { run: ["sleep", "30"], after: ["database"] }"#,
        "fn run() {}\n",
    );
    let env = [
        ("JETPACK_ROOT", root.path.display().to_string()),
        ("HOME", home.path.display().to_string()),
        ("JETPACK_SERVICE_HEALTH_TIMEOUT_MS", "1500".to_string()),
    ];
    let up = jetpack()
        .args(["services", "up", "--no-color"])
        .current_dir(&proj.path)
        .envs(env.clone())
        .output()
        .unwrap();
    assert!(up.status.success(), "{}", String::from_utf8_lossy(&up.stderr));
    let api_pid = proj.path.join(".jet/services/api/pid");
    assert!(api_pid.is_file());

    wait_for_missing(&api_pid);
    let error = proj.path.join(".jet/services/api/supervisor.error");
    wait_for_file_contains(&error, "dependency `database` failed");
    let lifecycle = fs::read_to_string(proj.path.join(".jet/services/api/lifecycle")).unwrap();
    assert!(lifecycle.contains("recovery=dependency-failed"), "{lifecycle}");
    let down = jetpack()
        .args(["services", "down", "--no-color"])
        .current_dir(&proj.path)
        .envs(env)
        .output()
        .unwrap();
    assert!(down.status.success(), "{}", String::from_utf8_lossy(&down.stderr));
}
