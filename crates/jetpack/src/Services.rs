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
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use jet_env_model::ModuleEval::DevServicePlan;
use super::Shell::Env as ShellEnv;
use crate::Syntax;

/// A well-known dev dependency's default package, port, start command, and
/// readiness probe — used only when the author's `services:` entry doesn't
/// override `init`/`ready`/`ports` itself. Seed content (U12 does not ratify
/// a fixed list): add an entry here for any other common dev dependency; a
/// name with no entry and no explicit `init:` is a plain "don't know how to
/// start this" error, not a silent no-op.
struct Catalog {
    /// The `<package>@<source>` ref to realize before spawning (D-JPK-REF1
    /// ref grammar) — added to the project's package refs automatically.
    pkg_ref: &'static str,
    port: i64,
    init: fn(port: i64, data_dir: &Path) -> String,
    ready: fn(port: i64) -> String,
}

fn catalog(name: &str) -> Option<Catalog> {
    match name {
        "redis" => Some(Catalog {
            pkg_ref: "redis@default",
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessState {
    pid: u32,
    start_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PersistedProcessState {
    Verified(ProcessState),
    /// State written by Jetpack versions that persisted only a PID. A PID
    /// can be reused, so this state may be removed but must never authorize a
    /// liveness claim or signal.
    LegacyPid(u32),
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

fn read_process_state(dir: &Path) -> Result<Option<PersistedProcessState>, String> {
    let path = pid_path(dir);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "couldn't read service process state `{}`: {error}",
                path.display()
            ))
        }
    };
    let trimmed = text.trim();
    if let Ok(pid) = trimmed.parse::<u32>() {
        return Ok(Some(PersistedProcessState::LegacyPid(pid)));
    }
    let mut version = None;
    let mut pid = None;
    let mut start_identity = None;
    for line in trimmed.lines() {
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("service process state `{}` is invalid", path.display()));
        };
        match key {
            "version" => version = Some(value),
            "pid" => pid = value.parse::<u32>().ok(),
            "start" if !value.is_empty() => start_identity = Some(value.to_string()),
            _ => {}
        }
    }
    if version != Some("1") {
        return Err(format!("service process state `{}` is invalid", path.display()));
    }
    let (Some(pid), Some(start_identity)) = (pid, start_identity) else {
        return Err(format!("service process state `{}` is invalid", path.display()));
    };
    Ok(Some(PersistedProcessState::Verified(ProcessState {
        pid,
        start_identity,
    })))
}

#[cfg(test)]
fn read_pid(dir: &Path) -> Option<u32> {
    match read_process_state(dir).ok().flatten()? {
        PersistedProcessState::Verified(state) => Some(state.pid),
        PersistedProcessState::LegacyPid(pid) => Some(pid),
    }
}

/// Whether `pid` names a live process — the only signal std can't get any
/// other way for an arbitrary (non-child) pid, so this shells out to `kill
/// -0` (the POSIX "is it there" no-op signal), same rationale as `down`.
#[cfg(test)]
fn is_alive(pid: u32) -> bool {
    process_alive(pid).unwrap_or(false)
}

fn process_alive(pid: u32) -> Result<bool, String> {
    #[cfg(windows)]
    {
        return windows_process::is_alive(pid)
            .map_err(|e| format!("couldn't probe service process {pid}: {e}"));
    }
    #[cfg(not(windows))]
    {
        let output = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .output()
            .map_err(|e| format!("couldn't probe service process {pid}: {e}"))?;
        if output.status.success() {
            return Ok(true);
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not permitted") || stderr.contains("Permission denied") {
            return Err(format!("couldn't probe service process {pid}: {}", stderr.trim()));
        }
        Ok(false)
    }
}

#[cfg(target_os = "linux")]
fn process_start_identity(pid: u32) -> Result<Option<String>, String> {
    const ESRCH: i32 = 3;
    let path = PathBuf::from(format!("/proc/{pid}/stat"));
    let stat = match fs::read_to_string(&path) {
        Ok(stat) => stat,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                || error.raw_os_error() == Some(ESRCH) =>
        {
            // Linux may report ESRCH instead of ENOENT when the process
            // disappears while procfs resolves its stat entry.
            return Ok(None);
        }
        Err(error) => {
            return Err(format!(
                "couldn't read service process identity `{}`: {error}",
                path.display()
            ))
        }
    };
    // Field 2 (`comm`) may contain spaces and parentheses. Everything after
    // its final `)` begins at field 3; starttime is field 22, index 19 here.
    let fields = stat
        .rsplit_once(')')
        .map(|(_, rest)| rest.split_whitespace().collect::<Vec<_>>())
        .ok_or_else(|| format!("service process identity for {pid} is malformed"))?;
    let start = fields
        .get(19)
        .ok_or_else(|| format!("service process identity for {pid} is incomplete"))?;
    start
        .parse::<u64>()
        .map_err(|_| format!("service process identity for {pid} has an invalid start time"))?;
    Ok(Some(format!("linux-proc-start:{start}")))
}

#[cfg(target_os = "macos")]
fn process_start_identity(pid: u32) -> Result<Option<String>, String> {
    macos_process::start_identity(pid)
        .map_err(|e| format!("couldn't probe service process identity {pid}: {e}"))
}

#[cfg(target_os = "macos")]
mod macos_process {
    use std::ffi::c_int;
    use std::io;

    const PROC_PIDTBSDINFO: c_int = 3;
    const ESRCH: i32 = 3;

    #[repr(C)]
    struct ProcBsdInfo {
        flags: u32,
        status: u32,
        xstatus: u32,
        pid: u32,
        ppid: u32,
        uid: u32,
        gid: u32,
        ruid: u32,
        rgid: u32,
        svuid: u32,
        svgid: u32,
        reserved: u32,
        comm: [u8; 16],
        name: [u8; 32],
        nfiles: u32,
        pgid: u32,
        pjobc: u32,
        tty_device: u32,
        tty_pgid: u32,
        nice: i32,
        start_seconds: u64,
        start_microseconds: u64,
    }

    #[link(name = "proc")]
    unsafe extern "C" {
        fn proc_pidinfo(
            pid: c_int,
            flavor: c_int,
            arg: u64,
            buffer: *mut std::ffi::c_void,
            buffer_size: c_int,
        ) -> c_int;
    }

    pub(super) fn start_identity(pid: u32) -> io::Result<Option<String>> {
        let mut info = std::mem::MaybeUninit::<ProcBsdInfo>::uninit();
        let size = std::mem::size_of::<ProcBsdInfo>() as c_int;
        // SAFETY: output points to `size` writable bytes; flavor requires the
        // ProcBsdInfo layout declared by libproc.h.
        let written = unsafe {
            proc_pidinfo(
                pid as c_int,
                PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                size,
            )
        };
        if written == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ESRCH) {
                return Ok(None);
            }
            return Err(error);
        }
        if written != size {
            return Err(io::Error::other(format!(
                "proc_pidinfo returned {written} of {size} bytes"
            )));
        }
        // SAFETY: exact expected byte count was returned.
        let info = unsafe { info.assume_init() };
        Ok(Some(format!(
            "macos-proc-start:{}:{}",
            info.start_seconds, info.start_microseconds
        )))
    }
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn process_start_identity(pid: u32) -> Result<Option<String>, String> {
    // Other BSDs expose stable long-start output through ps. Persist the
    // normalized full timestamp, never elapsed time.
    let output = Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .output()
        .map_err(|e| format!("couldn't probe service process identity {pid}: {e}"))?;
    let start = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() && !start.is_empty() {
        return Ok(Some(format!("bsd-ps-lstart:{start}")));
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        return Err(format!(
            "couldn't probe service process identity {pid}: {}",
            stderr.trim()
        ));
    }
    Ok(None)
}

#[cfg(windows)]
fn process_start_identity(pid: u32) -> Result<Option<String>, String> {
    windows_process::start_identity(pid)
        .map_err(|e| format!("couldn't probe service process identity {pid}: {e}"))
}

#[cfg(not(any(unix, windows)))]
fn process_start_identity(_pid: u32) -> Result<Option<String>, String> {
    Err("service process identity is unsupported on this platform".to_string())
}

#[cfg(windows)]
mod windows_process {
    use std::ffi::c_void;
    use std::io;

    type Handle = *mut c_void;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const ERROR_INVALID_PARAMETER: i32 = 87;
    const STILL_ACTIVE: u32 = 259;

    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> Handle;
        fn GetProcessTimes(
            process: Handle,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn GetExitCodeProcess(process: Handle, exit_code: *mut u32) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    struct OwnedHandle(Handle);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            // SAFETY: handle was returned by OpenProcess and is closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    fn open(pid: u32) -> io::Result<Option<OwnedHandle>> {
        // SAFETY: scalar arguments; returned handle is owned on success.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER) {
                return Ok(None);
            }
            return Err(error);
        }
        Ok(Some(OwnedHandle(handle)))
    }

    pub(super) fn start_identity(pid: u32) -> io::Result<Option<String>> {
        let Some(handle) = open(pid)? else { return Ok(None) };
        let mut creation = FileTime { low: 0, high: 0 };
        let mut exit = FileTime { low: 0, high: 0 };
        let mut kernel = FileTime { low: 0, high: 0 };
        let mut user = FileTime { low: 0, high: 0 };
        // SAFETY: live process handle and initialized writable outputs.
        if unsafe {
            GetProcessTimes(
                handle.0,
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let ticks = (u64::from(creation.high) << 32) | u64::from(creation.low);
        Ok(Some(format!("windows-filetime:{ticks}")))
    }

    pub(super) fn is_alive(pid: u32) -> io::Result<bool> {
        let Some(handle) = open(pid)? else { return Ok(false) };
        let mut exit_code = 0;
        // SAFETY: live process handle and writable scalar output.
        if unsafe { GetExitCodeProcess(handle.0, &mut exit_code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(exit_code == STILL_ACTIVE)
    }
}

fn process_matches_start_with(
    expected: &str,
    mut identity: impl FnMut() -> Result<Option<String>, String>,
    mut alive: impl FnMut() -> Result<bool, String>,
) -> Result<bool, String> {
    let Some(actual) = identity()? else { return Ok(false) };
    if actual != expected {
        return Ok(false);
    }
    alive()
}

fn process_matches_start(state: &ProcessState) -> Result<bool, String> {
    process_matches_start_with(
        &state.start_identity,
        || process_start_identity(state.pid),
        || process_alive(state.pid),
    )
}

fn cleanup_child(child: &mut Child) -> String {
    let kill_error = child.kill().err();
    let wait_error = child.wait().err();
    match (kill_error, wait_error) {
        (None, None) => String::new(),
        (kill, wait) => format!("; child cleanup failed (kill: {kill:?}, wait: {wait:?})"),
    }
}

fn publish_process_state(child: &mut Child, path: &Path) -> Result<ProcessState, String> {
    let start_identity = match process_start_identity(child.id()) {
        Ok(Some(identity)) => identity,
        Ok(None) => {
            let cleanup = cleanup_child(child);
            return Err(format!(
                "couldn't capture service process identity for {}{cleanup}",
                child.id()
            ));
        }
        Err(error) => {
            let cleanup = cleanup_child(child);
            return Err(format!("{error}{cleanup}"));
        }
    };
    let state = ProcessState {
        pid: child.id(),
        start_identity,
    };
    let encoded = format!(
        "version=1\npid={}\nstart={}\n",
        state.pid, state.start_identity
    );
    if let Err(error) = fs::write(path, encoded) {
        let cleanup = cleanup_child(child);
        return Err(format!(
            "couldn't publish service process state to `{}`: {error}{cleanup}",
            path.display()
        ));
    }
    Ok(state)
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
    if let Some(state) = read_process_state(&resolved.dir)? {
        match state {
            PersistedProcessState::Verified(state) if process_matches_start(&state)? => {
                return Ok(()); // already up
            }
            PersistedProcessState::Verified(_) | PersistedProcessState::LegacyPid(_) => {
                // A dead/reused verified PID and a PID-only legacy record are
                // both stale authority. Remove state; never signal either.
                fs::remove_file(pid_path(&resolved.dir))
                    .map_err(|e| format!("couldn't remove stale service state: {e}"))?;
            }
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
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("couldn't start `{}`: {e}", plan.name))?;
    publish_process_state(&mut child, &pid_path(&resolved.dir))?;
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
pub fn down_one(project_dir: &Path, plan: &DevServicePlan) -> Result<(), String> {
    let _guard = super::RuntimePolicy::acquire_lock(
        &super::Store::managed_dir(project_dir),
        "services-state",
    )
    .map_err(|e| format!("couldn't lock service state: {e}"))?;
    let dir = service_dir(project_dir, &plan.name);
    let Some(state) = read_process_state(&dir)? else { return Ok(()) };
    let PersistedProcessState::Verified(state) = state else {
        // Legacy PID-only state cannot prove which process owns the PID.
        // Migrate by dropping only the unverifiable record.
        return match fs::remove_file(pid_path(&dir)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("couldn't remove legacy service state: {error}")),
        };
    };
    if !process_matches_start(&state)? {
        match fs::remove_file(pid_path(&dir)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("couldn't remove stale service state: {error}")),
        }
        return Ok(());
    }
    stop_process(&state, plan.shutdown.as_deref())?;
    match fs::remove_file(pid_path(&dir)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("couldn't remove service state: {error}")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StopAction {
    Shutdown(String),
    Term,
    Kill,
}

fn stop_process(state: &ProcessState, shutdown: Option<&str>) -> Result<(), String> {
    let pid = state.pid;
    stop_process_with(
        pid,
        shutdown,
        Duration::from_secs(3),
        |action| run_verified_stop_action(state, action),
        || process_matches_start(state),
    )
}

fn run_if_start_identity_matches(
    expected: &str,
    mut identity: impl FnMut() -> Result<Option<String>, String>,
    action: impl FnOnce() -> Result<(), String>,
) -> Result<bool, String> {
    if identity()?.as_deref() != Some(expected) {
        return Ok(false);
    }
    action()?;
    Ok(true)
}

fn run_verified_stop_action(state: &ProcessState, action: StopAction) -> Result<(), String> {
    let _ran = run_if_start_identity_matches(
        &state.start_identity,
        || process_start_identity(state.pid),
        || run_stop_action(state.pid, action),
    )?;
    Ok(())
}

fn stop_process_with(
    pid: u32,
    shutdown: Option<&str>,
    grace: Duration,
    mut run: impl FnMut(StopAction) -> Result<(), String>,
    mut alive: impl FnMut() -> Result<bool, String>,
) -> Result<(), String> {
    run(match shutdown {
        Some(command) => StopAction::Shutdown(command.to_string()),
        None => StopAction::Term,
    })?;
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline && alive()? {
        std::thread::sleep(Duration::from_millis(100));
    }
    if alive()? {
        run(StopAction::Kill)?;
        let deadline = Instant::now() + grace.min(Duration::from_secs(1));
        while Instant::now() < deadline && alive()? {
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    if alive()? {
        return Err(format!("service process {pid} is still alive after shutdown"));
    }
    Ok(())
}

fn run_stop_action(pid: u32, action: StopAction) -> Result<(), String> {
    let (label, status) = match action {
        StopAction::Shutdown(command) => (
            "shutdown command",
            super::Platform::shell_command(&command).status(),
        ),
        StopAction::Term => {
            #[cfg(windows)]
            let status = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T"])
                .status();
            #[cfg(not(windows))]
            let status = Command::new("kill")
                .args(["-TERM", "--", &format!("-{pid}")])
                .status();
            ("TERM", status)
        }
        StopAction::Kill => {
            #[cfg(windows)]
            let status = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .status();
            #[cfg(not(windows))]
            let status = Command::new("kill")
                .args(["-KILL", "--", &format!("-{pid}")])
                .status();
            ("KILL", status)
        }
    };
    let status = status.map_err(|e| format!("couldn't run service {label}: {e}"))?;
    if !status.success() {
        return Err(format!("service {label} exited with {status}"));
    }
    Ok(())
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
    let Ok(Some(PersistedProcessState::Verified(state))) = read_process_state(&dir) else {
        return Health::NotRunning;
    };
    if !process_matches_start(&state).unwrap_or(false) {
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

/// Measure service startup latency by cycling the service down → up →
/// wait_healthy exactly `trials` times and returning the elapsed nanoseconds
/// for each trial.
///
/// D-PERFBUDGET-PROVIDER1 / INTEGRATION1: this is the **only** measurement
/// path for `ServiceProbe`; no proxy or facade fact is ever substituted.
/// Each trial is fully isolated (process group torn down between trials) and
/// measures from the moment the start command is issued until the effective
/// declared `ready:` command passes. Port and process liveness are valid dev
/// health fallbacks, but are not ServiceReadiness measurement evidence.
///
/// Returns exactly `trials` nanosecond samples, or a `String` error if any
/// trial fails to start or times out.
pub fn measure_readiness(
    project_dir: &Path,
    env: &ShellEnv,
    plan: &DevServicePlan,
    trials: usize,
) -> Result<Vec<u64>, String> {
    measure_readiness_with_timeout(project_dir, env, plan, trials, Duration::from_secs(10))
}

fn measure_readiness_with_timeout(
    project_dir: &Path,
    env: &ShellEnv,
    plan: &DevServicePlan,
    trials: usize,
    timeout: Duration,
) -> Result<Vec<u64>, String> {
    if resolve(project_dir, plan)?.ready.is_none() {
        return Err(format!(
            "ServiceProbe `{}` has no declared `ready:` event; process and port liveness are not readiness evidence",
            plan.name
        ));
    }
    // Bring any leftover instance down before the first trial.
    down_one(project_dir, plan)?;
    let mut samples = Vec::with_capacity(trials);
    for trial in 0..trials {
        let t0 = Instant::now();
        up_one(project_dir, env, plan)
            .map_err(|e| format!("ServiceProbe trial {trial}: failed to start: {e}"))?;
        if !wait_healthy(project_dir, plan, timeout) {
            down_one(project_dir, plan)?;
            return Err(format!(
                "ServiceProbe trial {trial}: service did not become ready within {}s",
                timeout.as_secs()
            ));
        }
        samples.push(t0.elapsed().as_nanos() as u64);
        down_one(project_dir, plan)?;
    }
    Ok(samples)
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
    use jet_env_model::ModuleEval::{PromptPathMode, PromptStripMode};

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
            vars: std::collections::BTreeMap::new(),
            refs: Vec::new(),
            label: "jetpack".to_string(),
            prompt_path: PromptPathMode::Short,
            prompt_strip: PromptStripMode::Off,
            cache_leases: Vec::new(),
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
        down_one(&dir, &p).unwrap();
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
        down_one(&dir, &p).unwrap();
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
        down_one(&dir, &p).unwrap();

        let mut unreachable = plan("fixture2", "sleep 30");
        unreachable.ports = vec![1]; // reserved port, never accepts
        up_one(&dir, &env(), &unreachable).unwrap();
        assert!(
            !wait_healthy(&dir, &unreachable, Duration::from_millis(500)),
            "a port that never accepts must time out, not hang"
        );
        down_one(&dir, &unreachable).unwrap();
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn service_probe_requires_and_waits_for_declared_ready() {
        let dir = scratch("probe-ready");
        let p = plan("fixture", "sleep 30");
        let error = measure_readiness_with_timeout(
            &dir,
            &env(),
            &p,
            1,
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(error.contains("no declared `ready:` event"), "{error}");
        assert!(read_pid(&service_dir(&dir, "fixture")).is_none());

        let ready = service_dir(&dir, "fixture").join("data/ready");
        let observed = service_dir(&dir, "fixture").join("data/probed");
        let mut declared = p;
        declared.ready = Some(format!(
            "touch '{}'; test -f '{}'",
            observed.display(),
            ready.display()
        ));
        let signal_ready = ready.clone();
        let signal = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            fs::write(signal_ready, "ready").unwrap();
        });
        let samples = measure_readiness_with_timeout(
            &dir,
            &env(),
            &declared,
            1,
            Duration::from_secs(1),
        )
        .unwrap();
        signal.join().unwrap();
        assert_eq!(samples.len(), 1);
        assert!(observed.exists(), "ServiceProbe must execute the declared ready event");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn service_probe_timeout_stops_the_service() {
        let dir = scratch("probe-timeout");
        let mut p = plan("fixture", "sleep 30");
        p.ready = Some("false".to_string());
        let error = measure_readiness_with_timeout(
            &dir,
            &env(),
            &p,
            1,
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(error.contains("did not become ready"), "{error}");
        assert!(read_pid(&service_dir(&dir, "fixture")).is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn down_fails_closed_when_service_lock_cannot_open() {
        let dir = scratch("down-lock-failure");
        let p = plan("fixture", "sleep 30");
        up_one(&dir, &env(), &p).unwrap();
        let state_dir = service_dir(&dir, "fixture");
        let state_path = pid_path(&state_dir);
        let pid = read_pid(&state_dir).unwrap();
        let managed = super::super::Store::managed_dir(&dir);
        let locks = managed.join(".locks");
        let displaced = managed.join("locks-displaced");
        fs::rename(&locks, &displaced).unwrap();
        fs::write(&locks, "not a directory").unwrap();

        let error = down_one(&dir, &p).expect_err("lock failure must stop down");
        assert!(error.contains("couldn't lock"), "{error}");
        assert_eq!(read_pid(&state_dir), Some(pid));
        assert!(state_path.exists());
        assert!(is_alive(pid), "service must not be signaled without the lock");

        fs::remove_file(&locks).unwrap();
        fs::rename(displaced, locks).unwrap();
        down_one(&dir, &p).unwrap();
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn failed_shutdown_keeps_live_pid_state() {
        let dir = scratch("down-shutdown-failure");
        let mut p = plan("fixture", "sleep 30");
        up_one(&dir, &env(), &p).unwrap();
        let state_dir = service_dir(&dir, "fixture");
        let pid = read_pid(&state_dir).unwrap();
        p.shutdown = Some("exit 7".to_string());
        let error = down_one(&dir, &p).expect_err("nonzero shutdown must fail");
        assert!(error.contains("exited with"), "{error}");
        assert_eq!(read_pid(&state_dir), Some(pid));
        assert!(is_alive(pid));
        p.shutdown = None;
        down_one(&dir, &p).unwrap();
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn term_kill_and_final_liveness_failures_propagate() {
        let term = stop_process_with(
            42,
            None,
            Duration::ZERO,
            |action| match action {
                StopAction::Term => Err("TERM failed".to_string()),
                _ => Ok(()),
            },
            || Ok(true),
        )
        .unwrap_err();
        assert_eq!(term, "TERM failed");

        let kill = stop_process_with(
            42,
            None,
            Duration::ZERO,
            |action| match action {
                StopAction::Kill => Err("KILL failed".to_string()),
                _ => Ok(()),
            },
            || Ok(true),
        )
        .unwrap_err();
        assert_eq!(kill, "KILL failed");

        let alive = stop_process_with(
            42,
            None,
            Duration::ZERO,
            |_| Ok(()),
            || Ok(true),
        )
        .unwrap_err();
        assert!(alive.contains("still alive"), "{alive}");
    }

    #[test]
    fn process_state_round_trips_and_pid_only_state_stays_legacy() {
        let dir = scratch("process-state");
        let state = ProcessState {
            pid: 42,
            start_identity: "linux-proc-start:9001".to_string(),
        };
        fs::write(
            pid_path(&dir),
            format!(
                "version=1\npid={}\nstart={}\n",
                state.pid, state.start_identity
            ),
        )
        .unwrap();
        assert_eq!(
            read_process_state(&dir).unwrap(),
            Some(PersistedProcessState::Verified(state))
        );
        fs::write(pid_path(&dir), "42\n").unwrap();
        assert_eq!(
            read_process_state(&dir).unwrap(),
            Some(PersistedProcessState::LegacyPid(42))
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn incomplete_process_state_fails_closed() {
        let dir = scratch("incomplete-process-state");
        let state_path = pid_path(&dir);

        fs::write(&state_path, "version=1\npid=42\n").unwrap();
        assert!(read_process_state(&dir).unwrap_err().contains("is invalid"));

        fs::write(&state_path, "version=1\nstart=linux-proc-start:9001\n").unwrap();
        assert!(read_process_state(&dir).unwrap_err().contains("is invalid"));

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn legacy_pid_state_is_removed_without_running_shutdown_or_signaling() {
        let dir = scratch("legacy-pid");
        let mut p = plan("fixture", "unused");
        p.shutdown = Some("exit 99".to_string());
        let state_dir = service_dir(&dir, "fixture");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(pid_path(&state_dir), std::process::id().to_string()).unwrap();
        down_one(&dir, &p).expect("legacy state must be safely discarded");
        assert!(!pid_path(&state_dir).exists());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn pid_reuse_identity_mismatch_never_reaches_liveness_or_signal() {
        let liveness_called = std::cell::Cell::new(false);
        assert!(!process_matches_start_with(
            "linux-proc-start:old",
            || Ok(Some("linux-proc-start:reused".to_string())),
            || {
                liveness_called.set(true);
                Ok(true)
            },
        )
        .unwrap());
        assert!(!liveness_called.get());

        let signaled = std::cell::Cell::new(false);
        assert!(!run_if_start_identity_matches(
            "linux-proc-start:old",
            || Ok(Some("linux-proc-start:reused".to_string())),
            || {
                signaled.set(true);
                Ok(())
            },
        )
        .unwrap());
        assert!(!signaled.get());
    }

    #[test]
    fn process_identity_probe_errors_propagate_before_liveness() {
        let error = process_matches_start_with(
            "expected",
            || Err("identity probe failed".to_string()),
            || panic!("liveness must not run after an identity probe error"),
        )
        .unwrap_err();
        assert_eq!(error, "identity probe failed");
    }

    #[test]
    fn current_process_start_identity_is_stable() {
        let pid = std::process::id();
        let first = process_start_identity(pid).unwrap().unwrap();
        let second = process_start_identity(pid).unwrap().unwrap();
        assert_eq!(first, second);
        assert!(
            process_matches_start(&ProcessState {
                pid,
                start_identity: first,
            })
            .unwrap()
        );
    }

    #[test]
    fn platform_start_identity_contracts_are_explicit() {
        let source = include_str!("Services.rs");
        for required in [
            "target_os = \"linux\"",
            "/proc/{pid}/stat",
            "linux-proc-start",
            "target_os = \"macos\"",
            "proc_pidinfo",
            "PROC_PIDTBSDINFO",
            "macos-proc-start",
            "lstart=",
            "OpenProcess",
            "GetProcessTimes",
            "GetExitCodeProcess",
            "windows-filetime",
        ] {
            assert!(source.contains(required), "missing process identity law: {required}");
        }
    }

    #[test]
    fn pid_publication_failure_kills_and_reaps_child() {
        let dir = scratch("pid-publication-failure");
        let bad_path = dir.join("pid-as-directory");
        fs::create_dir_all(&bad_path).unwrap();
        let mut child = super::super::Platform::shell_command("sleep 30")
            .spawn()
            .unwrap();
        let error = publish_process_state(&mut child, &bad_path).unwrap_err();
        assert!(
            error.contains("couldn't publish service process state"),
            "{error}"
        );
        assert!(child.try_wait().unwrap().is_some(), "child must be reaped");
        fs::remove_dir_all(dir).ok();
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
