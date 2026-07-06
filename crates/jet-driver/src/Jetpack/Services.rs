//! U12: dev-supervised services runtime (card c9jetpackgates).
//!
//! `env.<name>`'s `services:` map declares processes jetpack itself
//! supervises for the dev loop — distinct from the jetos `system.*.services`
//! tier (`ModuleEval::ServicePlan`, Phase D, untouched): no system service
//! manager, just plain child processes via `std::process` under
//! `.jet/services/<name>/`, no external crate (I6). Killing an arbitrary
//! (non-child) pid needs a real `kill` — Rust's std can only signal a
//! process it spawned itself — so `down` shells out to the `kill` binary,
//! the same technique `Shell.rs`/`Provider.rs` already use for `nix`/`git`.
//!
//! `jet dev`'s health gate and `jetpack services up/down/health/logs`
//! (`Jetpack::CLI::cmd_services`) are the only callers.

use std::fs::{self, File};
use std::io::Read as _;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::ModuleEval::DevServicePlan;
use super::Shell::Env as ShellEnv;
use crate::Syntax;

/// A well-known dev dependency's default package, port, start command, and
/// readiness probe — used only when the author's `services:` entry doesn't
/// override `init`/`ready`/`ports` itself. Seed content (U12 does not ratify
/// a fixed list): add an entry here for any other common dev dependency; a
/// name with no entry and no explicit `init:` is a plain "don't know how to
/// start this" error, not a silent no-op.
struct Catalog {
    /// The `<source>:<package>` ref to realize before spawning (D-JPK-… U6
    /// ref grammar) — added to the project's package refs automatically.
    pkg_ref: &'static str,
    port: i64,
    init: fn(port: i64, data_dir: &Path) -> String,
    ready: fn(port: i64) -> String,
}

fn catalog(name: &str) -> Option<Catalog> {
    match name {
        "redis" => Some(Catalog {
            pkg_ref: "default:redis",
            port: 6379,
            init: |port, data_dir| {
                format!(
                    "redis-server --port {port} --daemonize no --dir {}",
                    data_dir.display()
                )
            },
            ready: |port| format!("redis-cli -p {port} ping"),
        }),
        _ => None,
    }
}

/// The catalog's package ref for `name`, if any — `evaluate_env`'s caller
/// folds this into the project's realized packages so a bare `redis: {
/// enable: true }` (no explicit `init`) actually has `redis-server` on PATH.
pub fn catalog_pkg_ref(name: &str) -> Option<&'static str> {
    catalog(name).map(|c| c.pkg_ref)
}

/// A `services:` entry resolved to what actually gets run: the effective
/// start command (author's `init:`, else the catalog default), the effective
/// readiness probe, the ports, and the on-disk layout. `None` when neither
/// the author nor the catalog supplies a start command.
struct Resolved {
    init: String,
    ready: Option<String>,
    ports: Vec<i64>,
    dir: PathBuf,
    data_dir: PathBuf,
}

/// The `.jet/services/<name>/` directory for `name` under `project_dir`.
fn service_dir(project_dir: &Path, name: &str) -> PathBuf {
    project_dir
        .join(Syntax::CONFIG_DEFAULT_DIR)
        .join(Syntax::SERVICES_STATE_DIR)
        .join(name)
}

fn pid_path(dir: &Path) -> PathBuf {
    dir.join("pid")
}
fn stdout_path(dir: &Path) -> PathBuf {
    dir.join("stdout.log")
}
fn stderr_path(dir: &Path) -> PathBuf {
    dir.join("stderr.log")
}

/// U12 (E1262): the field names jetpack's dev-runtime tier recognizes.
/// `plan.extra` holds every field name it *didn't* recognize (either a
/// genuinely unknown key, or a known key with the wrong shape, e.g. `ports:
/// "5432"` instead of `[5432]`) — so a non-empty `extra` is always the E1262
/// condition, keyed off the first offending field for the diagnostic.
pub fn unknown_field(plan: &DevServicePlan) -> Option<&str> {
    plan.extra.first().map(|(name, _)| name.as_str())
}

/// Resolve `plan` to what actually gets run, applying the catalog default for
/// any field the author left unset. `Err` is a plain "don't know how to start
/// this" message (not E1262 — the fields are all recognized; there's just no
/// command to run).
fn resolve(project_dir: &Path, plan: &DevServicePlan) -> Result<Resolved, String> {
    let dir = service_dir(project_dir, &plan.name);
    let cat = catalog(&plan.name);
    let data_dir = plan
        .data_dir
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.join("data"));
    let ports = if !plan.ports.is_empty() {
        plan.ports.clone()
    } else if let Some(c) = &cat {
        vec![c.port]
    } else {
        Vec::new()
    };
    let init = match (&plan.init, &cat) {
        (Some(cmd), _) => cmd.clone(),
        (None, Some(c)) => (c.init)(ports.first().copied().unwrap_or(c.port), &data_dir),
        (None, None) => {
            return Err(format!(
                "service `{}` has no `init:` command and isn't a known built-in service",
                plan.name
            ))
        }
    };
    let ready = plan.ready.clone().or_else(|| {
        cat.as_ref()
            .map(|c| (c.ready)(ports.first().copied().unwrap_or(c.port)))
    });
    Ok(Resolved {
        init,
        ready,
        ports,
        dir,
        data_dir,
    })
}

fn read_pid(dir: &Path) -> Option<u32> {
    fs::read_to_string(pid_path(dir))
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Whether `pid` names a live process — the only signal std can't get any
/// other way for an arbitrary (non-child) pid, so this shells out to `kill
/// -0` (the POSIX "is it there" no-op signal), same rationale as `down`.
fn is_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        let filter = format!("PID eq {pid}");
        return Command::new("tasklist")
            .args(["/FI", &filter, "/FO", "CSV", "/NH"])
            .output()
            .map(|o| {
                o.status.success()
                    && String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\""))
            })
            .unwrap_or(false);
    }
    #[cfg(not(windows))]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// Start `plan` if it isn't already running (idempotent). `env` composes the
/// same PATH the project's realized packages live on, so a catalog binary
/// (e.g. `redis-server`) resolves without the caller needing its own shell.
pub fn up_one(project_dir: &Path, env: &ShellEnv, plan: &DevServicePlan) -> Result<(), String> {
    let _guard = super::RuntimePolicy::acquire_lock(
        &super::Store::managed_dir(project_dir),
        "services-state",
    )
    .map_err(|e| e.to_string())?;
    if !plan.enable {
        return Ok(());
    }
    let resolved = resolve(project_dir, plan)?;
    fs::create_dir_all(&resolved.dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&resolved.data_dir).map_err(|e| e.to_string())?;
    if let Some(pid) = read_pid(&resolved.dir) {
        if is_alive(pid) {
            return Ok(()); // already up
        }
    }
    let stdout = File::create(stdout_path(&resolved.dir)).map_err(|e| e.to_string())?;
    let stderr = File::create(stderr_path(&resolved.dir)).map_err(|e| e.to_string())?;
    let mut cmd = super::Platform::shell_command(&resolved.init);
    cmd.current_dir(&resolved.data_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    let base_path = std::env::var("PATH").unwrap_or_default();
    cmd.env("PATH", env.composed_path(&base_path));
    // A new process group (pgid = the child's own pid), not jetpack's —
    // `init` may be a multi-statement script ("mkdir …; exec redis-server
    // …"), so whether the shell tail-call-exec's its last command or forks a
    // child of its own, `down` signals the *group* (`kill -<pid>`) and
    // reaches whichever process is actually still running, without also
    // hitting jetpack's own process group.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let child = cmd
        .spawn()
        .map_err(|e| format!("couldn't start `{}`: {e}", plan.name))?;
    fs::write(pid_path(&resolved.dir), child.id().to_string()).map_err(|e| e.to_string())?;
    // Not `.wait()`-ed on this thread — it's a supervised background process
    // meant to outlive this command, and `wait()` would block for as long as
    // it runs. But an un-`wait()`-ed `Child` never gets reaped *by this
    // process* when it exits (it becomes a zombie until reaped, however
    // briefly) — harmless for a real one-shot `jetpack services up` (the
    // whole process exits right after, reparenting the child to `init`,
    // which reaps it), but real for anything longer-lived in the same
    // process (tests; a future daemon mode). A detached thread blocked on
    // `wait()` reaps it the moment it actually exits, at zero cost while
    // it's still running.
    std::thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
    Ok(())
}

/// Stop `plan` if a supervised pid is on record — `SIGTERM`, a short grace
/// wait, then `SIGKILL` if it's still alive.
pub fn down_one(project_dir: &Path, plan: &DevServicePlan) {
    let _guard = super::RuntimePolicy::acquire_lock(
        &super::Store::managed_dir(project_dir),
        "services-state",
    )
    .ok();
    let dir = service_dir(project_dir, &plan.name);
    let Some(pid) = read_pid(&dir) else { return };
    if !is_alive(pid) {
        let _ = fs::remove_file(pid_path(&dir));
        return;
    }
    if let Some(shutdown) = &plan.shutdown {
        let _ = super::Platform::shell_command(shutdown).status();
    } else {
        #[cfg(windows)]
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
        #[cfg(not(windows))]
        // Negative pid = the whole process group `up_one` created (pgid ==
        // the leader's own pid), so a multi-statement `init` script's
        // eventual child (e.g. `redis-server`, after some setup steps) is
        // signaled too, not just the shell leader. The `--` is load-bearing:
        // without it some `kill` implementations (procps) mis-parse a
        // negative-pid argument as another option and silently no-op.
        let _ = Command::new("kill")
            .args(["-TERM", "--", &format!("-{pid}")])
            .status();
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && is_alive(pid) {
        std::thread::sleep(Duration::from_millis(100));
    }
    if is_alive(pid) {
        #[cfg(windows)]
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
        #[cfg(not(windows))]
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .status();
    }
    let _ = fs::remove_file(pid_path(&dir));
}

/// A `services: health` result for one service.
pub enum Health {
    Disabled,
    NotRunning,
    Healthy,
    /// Enabled, running, but the readiness probe hasn't passed (yet).
    Unhealthy,
}

/// One-shot readiness check — does not wait or retry (see `wait_healthy` for
/// the polling gate `jet dev`/`services up` use).
pub fn health_one(project_dir: &Path, plan: &DevServicePlan) -> Health {
    if !plan.enable {
        return Health::Disabled;
    }
    let dir = service_dir(project_dir, &plan.name);
    let Some(pid) = read_pid(&dir) else {
        return Health::NotRunning;
    };
    if !is_alive(pid) {
        return Health::NotRunning;
    }
    let Ok(resolved) = resolve(project_dir, plan) else {
        return Health::Unhealthy;
    };
    if probe_ready(&resolved) {
        Health::Healthy
    } else {
        Health::Unhealthy
    }
}

fn probe_ready(resolved: &Resolved) -> bool {
    if let Some(ready) = &resolved.ready {
        return super::Platform::shell_command(ready)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    }
    if let Some(port) = resolved.ports.first() {
        let addr = format!("127.0.0.1:{port}");
        return addr
            .parse()
            .ok()
            .and_then(|a| TcpStream::connect_timeout(&a, Duration::from_millis(300)).ok())
            .is_some();
    }
    // No `ready:` and no port to probe — a live supervised process is the
    // only signal we have, and it already passed the `is_alive` check above.
    true
}

/// Poll `plan`'s readiness until it passes or `timeout` elapses. Returns
/// `false` on timeout (the caller renders E1261).
pub fn wait_healthy(project_dir: &Path, plan: &DevServicePlan, timeout: Duration) -> bool {
    if !plan.enable {
        return true;
    }
    let deadline = Instant::now() + timeout;
    loop {
        if matches!(health_one(project_dir, plan), Health::Healthy) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// The captured stdout+stderr for `name`, concatenated and labeled — `jetpack
/// services logs <name>`.
pub fn logs(project_dir: &Path, name: &str) -> String {
    let dir = service_dir(project_dir, name);
    let mut out = String::new();
    for (label, path) in [("stdout", stdout_path(&dir)), ("stderr", stderr_path(&dir))] {
        let mut buf = String::new();
        if File::open(&path)
            .and_then(|mut f| f.read_to_string(&mut buf))
            .is_ok()
            && !buf.is_empty()
        {
            out.push_str(&format!("── {label} ──\n"));
            out.push_str(&buf);
            if !buf.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!("jpk-services-{tag}-{nanos}"));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn env() -> ShellEnv {
        ShellEnv {
            bin_dirs: Vec::new(),
            refs: Vec::new(),
            label: "jetpack".to_string(),
        }
    }

    fn plan(name: &str, init: &str) -> DevServicePlan {
        DevServicePlan {
            name: name.to_string(),
            enable: true,
            init: Some(init.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn up_then_down_a_fixture_daemon() {
        let dir = scratch("up-down");
        // A trivial "daemon": sleeps, so it stays alive until we stop it.
        let p = plan("fixture", "sleep 30");
        up_one(&dir, &env(), &p).unwrap();
        let pid = read_pid(&service_dir(&dir, "fixture")).unwrap();
        assert!(is_alive(pid), "fixture daemon should be running after up");
        down_one(&dir, &p);
        assert!(!is_alive(pid), "fixture daemon should be gone after down");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn up_is_idempotent() {
        let dir = scratch("up-idempotent");
        let p = plan("fixture", "sleep 30");
        up_one(&dir, &env(), &p).unwrap();
        let pid1 = read_pid(&service_dir(&dir, "fixture")).unwrap();
        up_one(&dir, &env(), &p).unwrap();
        let pid2 = read_pid(&service_dir(&dir, "fixture")).unwrap();
        assert_eq!(pid1, pid2, "a second `up` must not spawn a duplicate");
        down_one(&dir, &p);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn disabled_service_is_a_no_op() {
        let dir = scratch("disabled");
        let mut p = plan("fixture", "sleep 30");
        p.enable = false;
        up_one(&dir, &env(), &p).unwrap();
        assert!(read_pid(&service_dir(&dir, "fixture")).is_none());
        assert!(matches!(health_one(&dir, &p), Health::Disabled));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn health_gate_probes_a_tcp_port() {
        // A fixture "service": a background `nc`-free TCP listener via a tiny
        // shell loop isn't portable enough for a test fixture, so this proves
        // the two easy-to-verify contracts instead: process-alive-only
        // health (no `ports`/`ready`) and a timeout when the port never opens.
        let dir = scratch("health-alive-only");
        let p = plan("fixture", "sleep 30");
        up_one(&dir, &env(), &p).unwrap();
        assert!(matches!(health_one(&dir, &p), Health::Healthy));
        down_one(&dir, &p);

        let mut unreachable = plan("fixture2", "sleep 30");
        unreachable.ports = vec![1]; // reserved port, never accepts
        up_one(&dir, &env(), &unreachable).unwrap();
        assert!(
            !wait_healthy(&dir, &unreachable, Duration::from_millis(500)),
            "a port that never accepts must time out, not hang"
        );
        down_one(&dir, &unreachable);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn logs_capture_stdout_and_stderr() {
        let dir = scratch("logs");
        let p = plan("fixture", "echo hello-stdout; echo hello-stderr 1>&2");
        up_one(&dir, &env(), &p).unwrap();
        // Give the (already-exited) process a moment to flush to the files.
        std::thread::sleep(Duration::from_millis(200));
        let out = logs(&dir, "fixture");
        assert!(out.contains("hello-stdout"), "logs: {out}");
        assert!(out.contains("hello-stderr"), "logs: {out}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unresolvable_service_is_a_plain_error() {
        let dir = scratch("unresolvable");
        let p = DevServicePlan {
            name: "not-a-known-service".to_string(),
            enable: true,
            ..Default::default()
        };
        let err = up_one(&dir, &env(), &p).unwrap_err();
        assert!(err.contains("no `init:`"), "{err}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_field_detection() {
        let mut p = plan("fixture", "sleep 1");
        assert_eq!(unknown_field(&p), None);
        p.extra.push(("prot".to_string(), "5432".to_string()));
        assert_eq!(unknown_field(&p), Some("prot"));
    }
}
