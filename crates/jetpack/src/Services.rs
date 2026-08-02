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

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;

use jet_env_model::ModuleEval::{DevServicePlan, ReadyProbe, RestartPolicy, ShutdownPolicy};
use super::Shell::Env as ShellEnv;
use crate::Syntax;

/// A well-known dev dependency's default package, port, start command, and
/// readiness probe — used only when the author's `services:` entry doesn't
/// override `run`/`ready`/`ports` itself. Seed content (U12 does not ratify
/// a fixed list): add an entry here for any other common dev dependency; a
/// name with no entry and no explicit `run:` is a plain "don't know how to
/// start this" error, not a silent no-op.
struct Catalog {
    /// The `<package>@<source>` ref to realize before spawning (D-JPK-REF1
    /// ref grammar) — added to the project's package refs automatically.
    pkg_ref: &'static str,
    port: i64,
    run: fn(port: i64, data_dir: &Path) -> Vec<String>,
    ready: fn(port: i64) -> String,
}

/// A catalog entry that can be shown to an author or extended by a typed
/// contribution. The runtime still requires a real executable for every
/// preset; this record is metadata, not a fake service implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServicePreset {
    pub name: String,
    pub package: String,
    pub default_port: i64,
}

fn catalog(name: &str) -> Option<Catalog> {
    match name {
        "redis" => Some(Catalog {
            pkg_ref: "redis@nixpkgs",
            port: 6379,
            run: |port, data_dir| vec![
                "redis-server".to_string(),
                "--port".to_string(),
                port.to_string(),
                "--daemonize".to_string(),
                "no".to_string(),
                "--dir".to_string(),
                data_dir.display().to_string(),
            ],
            ready: |port| format!("redis-cli -p {port} ping"),
        }),
        "postgres" | "postgresql" => Some(Catalog {
            pkg_ref: "postgresql@nixpkgs",
            port: 5432,
            run: |port, data_dir| vec![
                "postgres".to_string(),
                "-D".to_string(),
                data_dir.display().to_string(),
                "-p".to_string(),
                port.to_string(),
            ],
            ready: |port| format!("pg_isready -h 127.0.0.1 -p {port}"),
        }),
        "mysql" => Some(Catalog {
            pkg_ref: "mysql@nixpkgs",
            port: 3306,
            run: |port, data_dir| vec![
                "mysqld".to_string(),
                format!("--datadir={}", data_dir.display()),
                format!("--port={port}"),
                "--skip-networking=0".to_string(),
            ],
            ready: |port| format!("mysqladmin ping -h 127.0.0.1 -P {port}"),
        }),
        "mariadb" => Some(Catalog {
            pkg_ref: "mariadb@nixpkgs",
            port: 3306,
            run: |port, data_dir| vec![
                "mariadbd".to_string(),
                format!("--datadir={}", data_dir.display()),
                format!("--port={port}"),
                "--skip-networking=0".to_string(),
            ],
            ready: |port| format!("mariadb-admin ping -h 127.0.0.1 -P {port}"),
        }),
        "nginx" => Some(Catalog {
            pkg_ref: "nginx@nixpkgs",
            port: 8080,
            run: |port, data_dir| vec![
                "nginx".to_string(),
                "-p".to_string(),
                data_dir.join(format!("nginx-{port}")).display().to_string(),
                "-g".to_string(),
                "daemon off;".to_string(),
            ],
            ready: |port| format!("curl -fsS http://127.0.0.1:{port}/"),
        }),
        "minio" => Some(Catalog {
            pkg_ref: "minio@nixpkgs",
            port: 9000,
            run: |port, data_dir| vec![
                "minio".to_string(),
                "server".to_string(),
                data_dir.display().to_string(),
                "--address".to_string(),
                format!(":{port}"),
            ],
            ready: |port| format!("curl -fsS http://127.0.0.1:{port}/minio/health/live"),
        }),
        "mail" | "mailpit" => Some(Catalog {
            pkg_ref: "mailpit@nixpkgs",
            port: 8025,
            run: |port, data_dir| vec![
                "mailpit".to_string(),
                "--database".to_string(),
                data_dir.join("mailpit.db").display().to_string(),
                "--listen".to_string(),
                format!("127.0.0.1:{port}"),
            ],
            ready: |port| format!("curl -fsS http://127.0.0.1:{port}/api/v1/info"),
        }),
        "adminer" => Some(Catalog {
            pkg_ref: "adminer@nixpkgs",
            port: 8081,
            run: |port, data_dir| vec![
                "adminer".to_string(),
                "--port".to_string(),
                port.to_string(),
                "--root".to_string(),
                data_dir.display().to_string(),
            ],
            ready: |port| format!("curl -fsS http://127.0.0.1:{port}/"),
        }),
        _ => None,
    }
}

/// The catalog's package ref for `name`, if any — `evaluate_env`'s caller
/// folds this into the project's realized packages so a bare `redis: {
/// enable: true }` (no explicit `run`) actually has `redis-server` on PATH.
pub fn catalog_pkg_ref(name: &str) -> Option<&'static str> {
    catalog(name).map(|c| c.pkg_ref)
}

/// Built-in service presets exposed by the typed contribution/catalog path.
/// Keep this list derived from the same `catalog` matcher used at runtime.
pub fn catalog_presets() -> Vec<ServicePreset> {
    ["redis", "postgres", "mysql", "mariadb", "nginx", "minio", "mailpit", "adminer"]
        .into_iter()
        .filter_map(|name| {
            catalog(name).map(|entry| ServicePreset {
                name: name.to_string(),
                package: entry.pkg_ref.to_string(),
                default_port: entry.port,
            })
        })
        .collect()
}

/// A `services:` entry resolved to what actually gets run: the effective
/// start command (author's `run:`, else the catalog default), the effective
/// readiness probe, the ports, and the on-disk layout. `None` when neither
/// the author nor the catalog supplies a start command.
struct Resolved {
    run: Vec<String>,
    ready: Option<String>,
    ready_probe: Option<ReadyProbe>,
    ports: Vec<i64>,
    dir: PathBuf,
    data_dir: PathBuf,
    project_dir: PathBuf,
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

fn stopping_path(dir: &Path) -> PathBuf {
    dir.join(".stopping")
}

fn ports_path(dir: &Path) -> PathBuf {
    dir.join("ports")
}

/// Publish small runtime state files as one complete replacement. Readers
/// must never observe a half-written PID, port list, or watch baseline.
fn write_atomic(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("state");
    let temporary = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(contents.as_ref())?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
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
    validate_service_name(&plan.name)?;
    validate_ports(&plan.ports)?;
    validate_socket_names(&plan.sockets)?;
    validate_project_relative("data_dir", plan.data_dir.as_deref())?;
    for path in &plan.watch {
        validate_project_relative("watch", Some(path))?;
        ensure_project_path(project_dir, &project_dir.join(path), "watch")?;
    }
    for socket in &plan.sockets {
        ensure_project_path(project_dir, &project_dir.join(socket), "socket")?;
    }
    let dir = service_dir(project_dir, &plan.name);
    let cat = catalog(&plan.name);
    let data_dir = plan
        .data_dir
        .as_ref()
        .map(|path| project_dir.join(path))
        .unwrap_or_else(|| dir.join("data"));
    ensure_project_path(project_dir, &data_dir, "data_dir")?;
    let declared_ports = if !plan.ports.is_empty() {
        plan.ports.clone()
    } else if let Some(c) = &cat {
        vec![c.port]
    } else {
        Vec::new()
    };
    let ports = if declared_ports.iter().any(|port| *port == 0) {
        fs::read_to_string(ports_path(&dir))
            .ok()
            .map(|raw| {
                raw.lines()
                    .filter_map(|line| line.trim().parse::<i64>().ok())
                    .collect::<Vec<_>>()
            })
            .filter(|ports| ports.len() == declared_ports.len())
            .unwrap_or(declared_ports)
    } else {
        declared_ports
    };
    let run = match (&plan.run, &cat) {
        (Some(command), _) => command.clone(),
        (None, Some(c)) => (c.run)(ports.first().copied().unwrap_or(c.port), &data_dir),
        (None, None) => {
            return Err(format!(
                "service `{}` has no `run:` command and isn't a known built-in service",
                plan.name
            ))
        }
    };
    let ready = plan.ready.clone().or_else(|| {
        cat.as_ref()
            .map(|c| (c.ready)(ports.first().copied().unwrap_or(c.port)))
    });
    Ok(Resolved {
        run,
        ready,
        ready_probe: plan.ready_probe.clone(),
        ports,
        dir,
        data_dir,
        project_dir: project_dir.to_path_buf(),
    })
}

fn validate_project_relative(field: &str, value: Option<&str>) -> Result<(), String> {
    let Some(value) = value else { return Ok(()) };
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(format!("service {field} path `{value}` must stay inside the project"));
    }
    Ok(())
}

/// Check existing path components before any service directory is created or
/// a command is built. Missing final components are safe only when their
/// nearest existing ancestor remains inside the canonical project root.
fn ensure_project_path(project_dir: &Path, path: &Path, field: &str) -> Result<(), String> {
    let root = project_dir
        .canonicalize()
        .map_err(|error| format!("couldn't resolve project root for service {field}: {error}"))?;
    let relative = path.strip_prefix(project_dir).map_err(|_| {
        format!(
            "service {field} path `{}` must stay inside the project",
            path.display()
        )
    })?;
    let canonical_candidate = root.join(relative);
    let mut existing = canonical_candidate.clone();
    loop {
        match fs::symlink_metadata(&existing) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !existing.pop() {
                    return Err(format!(
                        "service {field} path `{}` has no project-local ancestor",
                        path.display()
                    ));
                }
            }
            Err(error) => {
                return Err(format!(
                    "couldn't inspect service {field} path `{}`: {error}",
                    path.display()
                ));
            }
        }
    }
    let canonical_existing = existing.canonicalize().map_err(|error| {
        format!(
            "couldn't resolve service {field} path ancestor `{}`: {error}",
            existing.display()
        )
    })?;
    if !canonical_existing.starts_with(&root) {
        return Err(format!(
            "service {field} path `{}` escapes the project through a symlink",
            path.display()
        ));
    }
    if fs::symlink_metadata(&canonical_candidate).is_ok() {
        let resolved = canonical_candidate.canonicalize().map_err(|error| {
            format!(
                "couldn't resolve service {field} path `{}`: {error}",
                path.display()
            )
        })?;
        if !resolved.starts_with(&root) {
            return Err(format!(
                "service {field} path `{}` escapes the project through a symlink",
                path.display()
            ));
        }
    }
    Ok(())
}

fn validate_service_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
    {
        return Err(format!("service name `{name}` is not a safe state-directory name"));
    }
    Ok(())
}

fn validate_ports(ports: &[i64]) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for port in ports {
        if !(0..=u16::MAX as i64).contains(port) {
            return Err(format!("service port `{port}` is outside 0..65535"));
        }
        if !seen.insert(*port) {
            return Err(format!("service declares port `{port}` more than once"));
        }
    }
    Ok(())
}

fn validate_socket_names(sockets: &[String]) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for socket in sockets {
        let path = Path::new(socket);
        if path.is_absolute()
            || socket.is_empty()
            || path
                .components()
                .any(|component| component == std::path::Component::ParentDir)
        {
            return Err(format!(
                "service socket `{socket}` must be a non-empty project-relative path"
            ));
        }
        if !seen.insert(socket) {
            return Err(format!("service socket `{socket}` is declared more than once"));
        }
    }
    Ok(())
}

struct PortReservations {
    ports: Vec<u16>,
    _listeners: Vec<TcpListener>,
}

fn allocate_ports(ports: &[i64]) -> Result<PortReservations, String> {
    validate_ports(ports)?;
    let mut allocated = Vec::with_capacity(ports.len());
    let mut listeners = Vec::with_capacity(ports.len());
    for raw in ports {
            let requested = u16::try_from(*raw).map_err(|_| {
                format!("service port `{raw}` is outside the supported range")
            })?;
            let listener = TcpListener::bind(("127.0.0.1", requested)).map_err(|error| {
                if requested == 0 {
                    format!("couldn't allocate an ephemeral service port: {error}")
                } else {
                    format!("service port {requested} is unavailable: {error}")
                }
            })?;
            let port = listener
                .local_addr()
                .map_err(|error| format!("couldn't read allocated service port: {error}"))?
                .port();
            allocated.push(port);
            listeners.push(listener);
    }
    Ok(PortReservations {
        ports: allocated,
        _listeners: listeners,
    })
}

struct PreparedSockets {
    paths: Vec<PathBuf>,
    #[cfg(unix)]
    listeners: Vec<std::os::unix::net::UnixListener>,
}

impl Drop for PreparedSockets {
    fn drop(&mut self) {
        #[cfg(unix)]
        self.listeners.clear();
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
    }
}

impl PreparedSockets {
    /// Release the reservation immediately before the child is spawned. A
    /// service declares a path it will bind; keeping our listener open across
    /// `spawn` would make the service itself fail with `EADDRINUSE`.
    fn release(mut self) -> Result<(), String> {
        #[cfg(unix)]
        std::mem::take(&mut self.listeners);
        let mut first_error = None;
        for path in std::mem::take(&mut self.paths) {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    first_error.get_or_insert_with(|| {
                        format!(
                            "couldn't release service socket reservation `{}`: {error}",
                            path.display()
                        )
                    });
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

fn prepare_sockets(project_dir: &Path, sockets: &[String]) -> Result<PreparedSockets, String> {
    validate_socket_names(sockets)?;
    let mut paths = Vec::new();
    #[cfg(unix)]
    let mut listeners = Vec::new();
    let result = (|| {
        for socket in sockets {
            let path = project_dir.join(socket);
            ensure_project_path(project_dir, &path, "socket")?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    format!("couldn't prepare service socket directory `{}`: {error}", parent.display())
                })?;
            }
            #[cfg(unix)]
            {
                use std::os::unix::net::UnixListener;
                use std::os::unix::net::UnixStream;
                if path.exists() {
                    let metadata = fs::symlink_metadata(&path).map_err(|error| {
                        format!("couldn't inspect service socket `{}`: {error}", path.display())
                    })?;
                    if !metadata.file_type().is_socket() {
                        return Err(format!(
                            "service socket path `{}` already names a non-socket file",
                            path.display()
                        ));
                    }
                    if UnixStream::connect(&path).is_ok() {
                        return Err(format!("service socket `{}` is already in use", path.display()));
                    }
                    fs::remove_file(&path).map_err(|error| {
                        format!("couldn't remove stale service socket `{}`: {error}", path.display())
                    })?;
                }
                let listener = UnixListener::bind(&path).map_err(|error| {
                    format!("couldn't reserve service socket `{}`: {error}", path.display())
                })?;
                listeners.push(listener);
            }
            #[cfg(not(unix))]
            {
                return Err(format!(
                    "service socket activation is unsupported on this platform: `{socket}`"
                ));
            }
            paths.push(path);
        }
        Ok::<(), String>(())
    })();
    if let Err(error) = result {
        #[cfg(unix)]
        listeners.clear();
        for path in &paths {
            let _ = fs::remove_file(path);
        }
        return Err(error);
    }
    Ok(PreparedSockets {
        paths,
        #[cfg(unix)]
        listeners,
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
        if pid == 0 {
            return Err(format!("service process state `{}` is invalid", path.display()));
        }
        return Ok(Some(PersistedProcessState::LegacyPid(pid)));
    }
    if trimmed.is_empty() {
        return Err(format!("service process state `{}` is invalid", path.display()));
    }
    let mut version = None;
    let mut pid = None;
    let mut start_identity = None;
    let mut seen = BTreeSet::new();
    for line in trimmed.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("service process state `{}` is invalid", path.display()));
        };
        if !seen.insert(key) {
            return Err(format!("service process state `{}` is invalid", path.display()));
        }
        match key {
            "version" if value == "1" => version = Some(value),
            "pid" => {
                let parsed = value.parse::<u32>().ok().filter(|pid| *pid != 0);
                if parsed.is_none() {
                    return Err(format!("service process state `{}` is invalid", path.display()));
                }
                pid = parsed;
            }
            "start" if !value.is_empty() && !value.contains('\n') => {
                start_identity = Some(value.to_string())
            }
            _ => return Err(format!("service process state `{}` is invalid", path.display())),
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
    if let Err(error) = write_atomic(path, encoded.as_bytes()) {
        let cleanup = cleanup_child(child);
        return Err(format!(
            "couldn't publish service process state to `{}`: {error}{cleanup}",
            path.display()
        ));
    }
    Ok(state)
}

struct StartedService {
    child: Child,
    state: ProcessState,
}

/// Start `plan` if it isn't already running (idempotent). `env` composes the
/// same PATH the project's realized packages live on, so a catalog binary
/// (e.g. `redis-server`) resolves without the caller needing its own shell.
pub fn up_one(project_dir: &Path, env: &ShellEnv, plan: &DevServicePlan) -> Result<(), String> {
    let Some(started) = start_one(project_dir, env, plan)? else {
        return Ok(());
    };
    if plan.restart.is_some() || !plan.watch.is_empty() {
        let project_dir = project_dir.to_path_buf();
        let plan = plan.clone();
        let env = restart_env(env);
        std::thread::spawn(move || monitor_service(started, project_dir, env, plan));
    } else {
        // `services up` is a supervisor hand-off. Keep the child reaped
        // without making the CLI wait for a long-lived service to exit.
        std::thread::spawn(move || reap_child(started.child));
    }
    Ok(())
}

fn restart_env(env: &ShellEnv) -> ShellEnv {
    ShellEnv {
        bin_dirs: env.bin_dirs.clone(),
        vars: env.vars.clone(),
        unset_vars: env.unset_vars.clone(),
        refs: env.refs.clone(),
        label: env.label.clone(),
        prompt_path: env.prompt_path,
        prompt_strip: env.prompt_strip,
        cache_leases: Vec::new(),
    }
}

fn reap_child(mut child: Child) {
    let _ = child.wait();
}

fn start_one(
    project_dir: &Path,
    env: &ShellEnv,
    plan: &DevServicePlan,
) -> Result<Option<StartedService>, String> {
    let _guard = super::RuntimePolicy::acquire_lock(
        &super::Store::managed_dir(project_dir),
        "services-state",
    )
    .map_err(|e| e.to_string())?;
    if !plan.enable {
        return Ok(None);
    }
    let mut resolved = resolve(project_dir, plan)?;
    fs::create_dir_all(&resolved.dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&resolved.data_dir).map_err(|e| e.to_string())?;
    if let Some(state) = read_process_state(&resolved.dir)? {
        match state {
            PersistedProcessState::Verified(state) if process_matches_start(&state)? => {
                return Ok(None); // already up
            }
            PersistedProcessState::Verified(_) | PersistedProcessState::LegacyPid(_) => {
                // A dead/reused verified PID and a PID-only legacy record are
                // both stale authority. Remove state; never signal either.
                fs::remove_file(pid_path(&resolved.dir))
                    .map_err(|e| format!("couldn't remove stale service state: {e}"))?;
            }
        }
    }
    let allocated_ports = allocate_ports(&resolved.ports)?;
    let prepared_sockets = prepare_sockets(project_dir, &plan.sockets)?;
    if allocated_ports.ports != resolved.ports.iter().map(|port| *port as u16).collect::<Vec<_>>() {
        resolved.ports = allocated_ports.ports.iter().map(|port| i64::from(*port)).collect();
        if plan.run.is_none() {
            if let Some(catalog) = catalog(&plan.name) {
                resolved.run = (catalog.run)(
                    resolved.ports.first().copied().unwrap_or(catalog.port as i64),
                    &resolved.data_dir,
                );
            }
        }
        if plan.ready.is_none() && plan.ready_probe.is_none() {
            if let Some(catalog) = catalog(&plan.name) {
                resolved.ready = Some((catalog.ready)(
                    resolved.ports.first().copied().unwrap_or(catalog.port as i64),
                ));
            }
        }
    }
    let _ = fs::remove_file(stopping_path(&resolved.dir));
    let stdout = File::create(stdout_path(&resolved.dir)).map_err(|e| e.to_string())?;
    let stderr = File::create(stderr_path(&resolved.dir)).map_err(|e| e.to_string())?;
    if resolved.run.is_empty() {
        return Err(format!("service `{}` has an empty `run:` command", plan.name));
    }
    let mut cmd = Command::new(&resolved.run[0]);
    cmd.args(&resolved.run[1..]);
    cmd.current_dir(&resolved.data_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    env.apply_to(&mut cmd);
    let base_path = std::env::var("PATH").unwrap_or_default();
    cmd.env("PATH", env.composed_path(&base_path));
    if !resolved.ports.is_empty() {
        let ports = resolved
            .ports
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        cmd.env("JETPACK_SERVICE_PORTS", &ports);
        if let Some(port) = resolved.ports.first() {
            cmd.env("JETPACK_SERVICE_PORT", port.to_string());
        }
    }
    if !prepared_sockets.paths.is_empty() {
        let sockets = prepared_sockets
            .paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        cmd.env("JETPACK_SERVICE_SOCKETS", sockets);
        for (index, path) in prepared_sockets.paths.iter().enumerate() {
            cmd.env(
                format!("JETPACK_SERVICE_SOCKET_{index}"),
                path.to_string_lossy().as_ref(),
            );
        }
    }
    prepared_sockets.release()?;
    // Release the parent-side port listeners immediately before spawn so the
    // service can bind the reserved ports without an EADDRINUSE race.
    drop(allocated_ports);
    // A new process group (pgid = the child's own pid), not jetpack's —
    // The direct argv command is the process-group leader, so `down` signals
    // the whole group without also hitting jetpack's own process group.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("couldn't start `{}`: {e}", plan.name))?;
    let state = publish_process_state(&mut child, &pid_path(&resolved.dir))?;
    if !resolved.ports.is_empty() {
        let encoded = resolved
            .ports
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        if let Err(error) = write_atomic(&ports_path(&resolved.dir), encoded.as_bytes()) {
            let cleanup = cleanup_child(&mut child);
            let _ = fs::remove_file(pid_path(&resolved.dir));
            let _ = fs::remove_file(ports_path(&resolved.dir));
            return Err(format!("couldn't publish service ports: {error}{cleanup}"));
        }
    }
    Ok(Some(StartedService { child, state }))
}

fn monitor_service(
    mut started: StartedService,
    project_dir: PathBuf,
    env: ShellEnv,
    plan: DevServicePlan,
) {
    let (max_restarts, backoff_ms, exponential) = restart_budget(&plan);
    let watched = plan
        .watch
        .iter()
        .map(|path| project_dir.join(path))
        .collect::<Vec<_>>();
    let mut stamps = watch_stamps(&watched);
    let mut pending_watch: Option<(Vec<String>, Instant)> = None;
    let mut restarts = 0_u32;
    loop {
        match started.child.try_wait() {
            Err(_) => return,
            Ok(None) => {}
            Ok(Some(status)) => {
                let failed = !status.success();
                let restart = match plan.restart.as_ref() {
                    Some(RestartPolicy::Always { .. }) => true,
                    Some(RestartPolicy::OnFailure { .. }) => failed,
                    Some(RestartPolicy::Never) | None => false,
                };
                if !restart || restarts >= max_restarts || stopping_requested(&project_dir, &plan) {
                    return;
                }
                restarts += 1;
                std::thread::sleep(restart_delay(backoff_ms, exponential, restarts));
                match start_one(&project_dir, &env, &plan) {
                    Ok(Some(next)) => {
                        started = next;
                        stamps = watch_stamps(&watched);
                    }
                    Ok(None) | Err(_) => return,
                }
                continue;
            }
        }
        if !watched.is_empty()
            && watch_changed_debounced(&watched, &mut stamps, &mut pending_watch)
        {
            if restarts >= max_restarts || stopping_requested(&project_dir, &plan) {
                return;
            }
            if stop_process(&started.state, plan.shutdown.as_ref()).is_err() {
                return;
            }
            let _ = started.child.wait();
            restarts += 1;
            std::thread::sleep(restart_delay(backoff_ms, exponential, restarts));
            match start_one(&project_dir, &env, &plan) {
                Ok(Some(next)) => {
                    started = next;
                    stamps = watch_stamps(&watched);
                }
                Ok(None) | Err(_) => return,
            }
            continue;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn restart_budget(plan: &DevServicePlan) -> (u32, u64, bool) {
    match plan.restart.as_ref() {
        Some(RestartPolicy::OnFailure {
            max,
            backoff_ms,
            exponential,
        })
        | Some(RestartPolicy::Always {
            max,
            backoff_ms,
            exponential,
        }) => (*max, *backoff_ms, *exponential),
        Some(RestartPolicy::Never) => (0, 0, false),
        None if !plan.watch.is_empty() => (3, 250, false),
        None => (0, 0, false),
    }
}

fn restart_delay(backoff_ms: u64, exponential: bool, attempt: u32) -> Duration {
    let multiplier = if exponential {
        1_u64 << attempt.saturating_sub(1).min(20)
    } else {
        1
    };
    Duration::from_millis(backoff_ms.saturating_mul(multiplier))
}

fn stopping_requested(project_dir: &Path, plan: &DevServicePlan) -> bool {
    service_dir(project_dir, &plan.name).join(".stopping").is_file()
}

fn watch_stamps(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|path| match fs::metadata(path) {
            Ok(metadata) => format!(
                "{}:{}",
                metadata.len(),
                metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_nanos())
                    .unwrap_or_default()
            ),
            Err(_) => "missing".to_string(),
        })
        .collect()
}

fn watch_changed_debounced(
    paths: &[PathBuf],
    stamps: &mut Vec<String>,
    pending: &mut Option<(Vec<String>, Instant)>,
) -> bool {
    const DEBOUNCE: Duration = Duration::from_millis(250);
    let current = watch_stamps(paths);
    if current == *stamps {
        *pending = None;
        return false;
    }
    match pending {
        Some((candidate, started)) if *candidate == current => {
            if started.elapsed() < DEBOUNCE {
                return false;
            }
            *stamps = current;
            *pending = None;
            true
        }
        Some((candidate, started)) => {
            *candidate = current;
            *started = Instant::now();
            false
        }
        None => {
            *pending = Some((current, Instant::now()));
            false
        }
    }
}

/// Stop `plan` if a supervised pid is on record — `SIGTERM`, a short grace
/// wait, then `SIGKILL` if it's still alive.
pub fn down_one(project_dir: &Path, plan: &DevServicePlan) -> Result<(), String> {
    validate_service_name(&plan.name)?;
    let _guard = super::RuntimePolicy::acquire_lock(
        &super::Store::managed_dir(project_dir),
        "services-state",
    )
    .map_err(|e| format!("couldn't lock service state: {e}"))?;
    let dir = service_dir(project_dir, &plan.name);
    let Some(state) = read_process_state(&dir)? else {
        let _ = fs::remove_file(ports_path(&dir));
        return Ok(())
    };
    let PersistedProcessState::Verified(state) = state else {
        // Legacy PID-only state cannot prove which process owns the PID.
        // Migrate by dropping only the unverifiable record.
        let result = match fs::remove_file(pid_path(&dir)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("couldn't remove legacy service state: {error}")),
        };
        let _ = fs::remove_file(stopping_path(&dir));
        let _ = fs::remove_file(ports_path(&dir));
        return result;
    };
    if !process_matches_start(&state)? {
        match fs::remove_file(pid_path(&dir)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("couldn't remove stale service state: {error}")),
        }
        let _ = fs::remove_file(stopping_path(&dir));
        let _ = fs::remove_file(ports_path(&dir));
        return Ok(());
    }
    fs::write(stopping_path(&dir), b"down\n")
        .map_err(|error| format!("couldn't mark service `{}` for shutdown: {error}", plan.name))?;
    stop_process(&state, plan.shutdown.as_ref())?;
    let result = match fs::remove_file(pid_path(&dir)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("couldn't remove service state: {error}")),
    };
    let _ = fs::remove_file(ports_path(&dir));
    let _ = fs::remove_file(stopping_path(&dir));
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StopAction {
    Term,
    Kill,
}

fn stop_process(state: &ProcessState, shutdown: Option<&ShutdownPolicy>) -> Result<(), String> {
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
    shutdown: Option<&ShutdownPolicy>,
    grace: Duration,
    mut run: impl FnMut(StopAction) -> Result<(), String>,
    mut alive: impl FnMut() -> Result<bool, String>,
) -> Result<(), String> {
    let (action, wait) = match shutdown {
        Some(ShutdownPolicy::Kill) => (StopAction::Kill, Duration::ZERO),
        Some(ShutdownPolicy::Term { grace_ms }) => {
            (StopAction::Term, Duration::from_millis(*grace_ms))
        }
        None => (StopAction::Term, grace),
    };
    run(action)?;
    let deadline = Instant::now() + wait;
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
    health_one_with_env(project_dir, None, plan)
}

/// Readiness checks use the same realized PATH and variables as the service
/// process. The wrapper above keeps the low-level health API useful to callers
/// that only need process/TCP state.
pub fn health_one_with_env(
    project_dir: &Path,
    env: Option<&ShellEnv>,
    plan: &DevServicePlan,
) -> Health {
    if validate_service_name(&plan.name).is_err() {
        return Health::NotRunning;
    }
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
    if probe_ready(&resolved, env) {
        Health::Healthy
    } else {
        Health::Unhealthy
    }
}

fn probe_ready(resolved: &Resolved, env: Option<&ShellEnv>) -> bool {
    if let Some(probe) = &resolved.ready_probe {
        return match probe {
            ReadyProbe::Exec(command) => {
                let mut command = super::Platform::shell_command(command);
                command.current_dir(&resolved.project_dir);
                if let Some(env) = env {
                    env.apply_to(&mut command);
                }
                command
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|status| status.success())
                    .unwrap_or(false)
            }
            ReadyProbe::Http { url, status } => probe_http(url, *status),
            ReadyProbe::Notify { path } => {
                let path = Path::new(path);
                if path.is_absolute()
                    || path
                        .components()
                        .any(|component| component == std::path::Component::ParentDir)
                {
                    return false;
                }
                let path = resolved.project_dir.join(path);
                let Ok(project) = resolved.project_dir.canonicalize() else {
                    return false;
                };
                path.canonicalize()
                    .ok()
                    .is_some_and(|path| path.starts_with(project) && path.is_file())
            }
            ReadyProbe::Tcp { host, port } => probe_tcp(host, *port),
        };
    }
    if let Some(ready) = &resolved.ready {
        let mut command = super::Platform::shell_command(ready);
        command.current_dir(&resolved.project_dir);
        if let Some(env) = env {
            env.apply_to(&mut command);
        }
        return command
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

fn probe_tcp(host: &str, port: u16) -> bool {
    let addr = format!("{host}:{port}");
    addr.parse()
        .ok()
        .and_then(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(300)).ok())
        .is_some()
}

fn probe_http(url: &str, expected_status: Option<u16>) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = authority
        .rsplit_once(':')
        .and_then(|(host, port)| port.parse::<u16>().ok().map(|port| (host, port)))
        .unwrap_or((authority, 80));
    if host.is_empty() || host.contains('[') || host.contains(']') {
        return false;
    }
    let addr = format!("{host}:{port}");
    let Ok(addr) = addr.parse() else { return false };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(300)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
    let request = format!(
        "GET /{} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
        path
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut bytes = [0_u8; 1024];
    let Ok(size) = stream.read(&mut bytes) else { return false };
    let response = String::from_utf8_lossy(&bytes[..size]);
    let Some(status) = response
        .strip_prefix("HTTP/")
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
    else {
        return false;
    };
    expected_status.map_or((200..400).contains(&status), |expected| status == expected)
}

/// Poll `plan`'s readiness until it passes or `timeout` elapses. Returns
/// `false` on timeout (the caller renders E1261).
pub fn wait_healthy(project_dir: &Path, plan: &DevServicePlan, timeout: Duration) -> bool {
    wait_healthy_with_env(project_dir, None, plan, timeout)
}

pub fn wait_healthy_with_env(
    project_dir: &Path,
    env: Option<&ShellEnv>,
    plan: &DevServicePlan,
    timeout: Duration,
) -> bool {
    if !plan.enable {
        return true;
    }
    let deadline = Instant::now() + timeout;
    loop {
        if matches!(health_one_with_env(project_dir, env, plan), Health::Healthy) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// Return enabled service indexes in dependency-first order. A dependency
/// names another declared service; unknown names, duplicate service names, and
/// cycles fail before any process starts.
pub fn dependency_order(plans: &[DevServicePlan]) -> Result<Vec<usize>, String> {
    let mut indexes = std::collections::BTreeMap::new();
    for (index, plan) in plans.iter().enumerate() {
        validate_service_name(&plan.name)?;
        if indexes.insert(plan.name.clone(), index).is_some() {
            return Err(format!("service `{}` is declared more than once", plan.name));
        }
    }
    let mut states = vec![0_u8; plans.len()];
    let mut stack = Vec::new();
    let mut order = Vec::new();
    for index in 0..plans.len() {
        visit_dependency(index, plans, &indexes, &mut states, &mut stack, &mut order)?;
    }
    Ok(order)
}

fn visit_dependency(
    index: usize,
    plans: &[DevServicePlan],
    indexes: &std::collections::BTreeMap<String, usize>,
    states: &mut [u8],
    stack: &mut Vec<String>,
    order: &mut Vec<usize>,
) -> Result<(), String> {
    if !plans[index].enable {
        states[index] = 2;
        return Ok(());
    }
    match states[index] {
        2 => return Ok(()),
        1 => {
            let start = stack
                .iter()
                .position(|name| name == &plans[index].name)
                .unwrap_or(0);
            let mut cycle = stack[start..].to_vec();
            cycle.push(plans[index].name.clone());
            return Err(format!("service dependency cycle: {}", cycle.join(" -> ")));
        }
        _ => {}
    }
    states[index] = 1;
    stack.push(plans[index].name.clone());
    for dependency in dependency_names(&plans[index]) {
        let Some(&dependency_index) = indexes.get(&dependency) else {
            return Err(format!(
                "service `{}` depends on unknown service `{dependency}`",
                plans[index].name
            ));
        };
        if !plans[dependency_index].enable {
            return Err(format!(
                "service `{}` depends on disabled service `{dependency}`",
                plans[index].name
            ));
        }
        visit_dependency(dependency_index, plans, indexes, states, stack, order)?;
    }
    stack.pop();
    states[index] = 2;
    if plans[index].enable {
        order.push(index);
    }
    Ok(())
}

/// Return the canonical dependency list for a service. `after` is the
/// ratified spelling; `depends_on` remains readable for projects written
/// against the pre-ratification dev surface.
pub fn dependency_names(plan: &DevServicePlan) -> Vec<String> {
    let mut names = Vec::new();
    for name in plan.after.iter().chain(plan.depends_on.iter()) {
        if !names.iter().any(|existing| existing == name) {
            names.push(name.clone());
        }
    }
    names
}

/// Start and wait for every enabled service in dependency order. The CLI and
/// the dev health gate both use this path so a named dependency cannot be
/// started in a different order from the project run path.
pub fn up_ordered(
    project_dir: &Path,
    env: &ShellEnv,
    plans: &[DevServicePlan],
    timeout: Duration,
) -> Result<Vec<String>, String> {
    let order = dependency_order(plans)?;
    let mut started = Vec::new();
    for index in order {
        let plan = &plans[index];
        let was_running = matches!(
            health_one_with_env(project_dir, Some(env), plan),
            Health::Healthy | Health::Unhealthy
        );
        if let Err(error) = up_one(project_dir, env, plan) {
            let cleanup = started
                .iter()
                .filter_map(|name| plans.iter().find(|candidate| &candidate.name == name))
                .filter_map(|plan| down_one(project_dir, plan).err())
                .collect::<Vec<_>>();
            return Err(if cleanup.is_empty() {
                error
            } else {
                format!("{error}; cleanup failed: {}", cleanup.join("; "))
            });
        }
        if !wait_healthy_with_env(project_dir, Some(env), plan, timeout) {
            let current_error = format!("service `{}` did not become healthy", plan.name);
            let cleanup = (!was_running)
                .then_some(plan)
                .into_iter()
                .chain(
                    started
                        .iter()
                        .filter_map(|name| plans.iter().find(|candidate| &candidate.name == name)),
                )
                .filter_map(|plan| down_one(project_dir, plan).err())
                .collect::<Vec<_>>();
            return Err(if cleanup.is_empty() {
                current_error
            } else {
                format!("{current_error}; cleanup failed: {}", cleanup.join("; "))
            });
        }
        if !was_running {
            started.push(plan.name.clone());
        }
    }
    Ok(started)
}

/// Restart one service under its declared bounded restart budget. The normal
/// `up` path also keeps that policy active for unexpected exits; this explicit
/// operation is the deterministic CLI spelling for a source or config change.
pub fn restart_one(
    project_dir: &Path,
    env: &ShellEnv,
    plan: &DevServicePlan,
) -> Result<(), String> {
    let (max_restarts, backoff_ms, exponential) = restart_budget(plan);
    down_one(project_dir, plan)?;
    let mut last_error = String::new();
    for attempt in 0..=max_restarts {
        if attempt > 0 {
            std::thread::sleep(restart_delay(backoff_ms, exponential, attempt as u32));
        }
        match up_one(project_dir, env, plan) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = error,
        }
    }
    Err(if last_error.is_empty() {
        format!("service `{}` could not be restarted", plan.name)
    } else {
        last_error
    })
}

/// Check watched files once and restart only after the recorded fingerprint
/// changes. The first invocation records a baseline and does not restart.
pub fn watch_once(
    project_dir: &Path,
    env: &ShellEnv,
    plan: &DevServicePlan,
) -> Result<bool, String> {
    if plan.watch.is_empty() {
        return Err(format!("service `{}` has no watched files", plan.name));
    }
    validate_service_name(&plan.name)?;
    for path in &plan.watch {
        let path = Path::new(path);
        if path.is_absolute()
            || path
                .components()
                .any(|component| component == std::path::Component::ParentDir)
        {
            return Err(format!(
                "service watch path `{}` must be project-relative and cannot contain `..`",
                path.display()
            ));
        }
    }
    let paths = plan
        .watch
        .iter()
        .map(|path| {
            let path = project_dir.join(path);
            ensure_project_path(project_dir, &path, "watch").map(|()| path)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let state_path = service_dir(project_dir, &plan.name).join("watch.state");
    ensure_project_path(project_dir, &state_path, "watch state")?;
    if let Some(parent) = state_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let current = paths
        .iter()
        .zip(watch_stamps(&paths))
        .map(|(path, stamp)| format!("{}\t{stamp}", path.display()))
        .collect::<Vec<_>>()
        .join("\n");
    let previous = fs::read_to_string(&state_path).unwrap_or_default();
    write_atomic(&state_path, current.as_bytes()).map_err(|error| error.to_string())?;
    if previous.is_empty() || previous == current {
        return Ok(false);
    }
    restart_one(project_dir, env, plan)?;
    Ok(true)
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
    let resolved = resolve(project_dir, plan)?;
    if resolved.ready.is_none() && resolved.ready_probe.is_none() {
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
        if !wait_healthy_with_env(project_dir, Some(env), plan, timeout) {
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
    if validate_service_name(name).is_err() {
        return String::new();
    }
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
            unset_vars: Vec::new(),
            refs: Vec::new(),
            label: "jetpack".to_string(),
            prompt_path: PromptPathMode::Short,
            prompt_strip: PromptStripMode::Off,
            cache_leases: Vec::new(),
        }
    }

    fn plan(name: &str, command: &str) -> DevServicePlan {
        let run = if command.contains(';') {
            vec!["sh".to_string(), "-c".to_string(), command.to_string()]
        } else {
            command.split_whitespace().map(str::to_string).collect()
        };
        DevServicePlan {
            name: name.to_string(),
            enable: true,
            run: Some(run),
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

    #[cfg(unix)]
    #[test]
    fn data_dir_symlink_escape_is_rejected_before_start() {
        let dir = scratch("data-dir-symlink");
        let outside = scratch("data-dir-outside");
        std::os::unix::fs::symlink(&outside, dir.join("escape")).unwrap();
        let mut service = plan("fixture", "sleep 30");
        service.data_dir = Some("escape/data".to_string());
        let error = match resolve(&dir, &service) {
            Ok(_) => panic!("a data directory symlink escape must be rejected"),
            Err(error) => error,
        };
        assert!(error.contains("data_dir") && error.contains("escapes"), "{error}");
        fs::remove_dir_all(&dir).ok();
        fs::remove_dir_all(&outside).ok();
    }

    #[cfg(unix)]
    #[test]
    fn watch_and_socket_symlink_escapes_are_rejected_before_creation() {
        let dir = scratch("service-path-symlink");
        let outside = scratch("service-path-outside");
        std::os::unix::fs::symlink(&outside, dir.join("watch-root")).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("socket-root")).unwrap();

        let mut watched = plan("fixture", "sleep 30");
        watched.watch = vec!["watch-root/config".to_string()];
        let watch_error = match resolve(&dir, &watched) {
            Ok(_) => panic!("watch escape must be rejected"),
            Err(error) => error,
        };
        assert!(watch_error.contains("watch") && watch_error.contains("escapes"), "{watch_error}");

        let socket_error = match prepare_sockets(&dir, &["socket-root/service.sock".to_string()]) {
            Ok(_) => panic!("socket escape must be rejected"),
            Err(error) => error,
        };
        assert!(socket_error.contains("socket") && socket_error.contains("escapes"), "{socket_error}");
        assert!(!outside.join("service.sock").exists());

        fs::remove_dir_all(&dir).ok();
        fs::remove_dir_all(&outside).ok();
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
        unreachable.ports = vec![0]; // allocate an ephemeral port; sleep never accepts
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
    fn typed_shutdown_policy_kills_verified_process_group() {
        let dir = scratch("down-shutdown-kill");
        let mut p = plan("fixture", "sleep 30");
        up_one(&dir, &env(), &p).unwrap();
        let state_dir = service_dir(&dir, "fixture");
        let pid = read_pid(&state_dir).unwrap();
        p.shutdown = Some(ShutdownPolicy::Kill);
        down_one(&dir, &p).unwrap();
        assert!(!is_alive(pid));
        assert!(read_pid(&state_dir).is_none());
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
    fn process_state_rejects_unknown_duplicate_and_malformed_fields() {
        let dir = scratch("malformed-process-state");
        let state_path = pid_path(&dir);
        for raw in [
            "version=1\npid=42\nstart=x\nextra=y\n",
            "version=1\npid=42\npid=43\nstart=x\n",
            "version=1\npid=nope\nstart=x\n",
            "version=1\npid=0\nstart=x\n",
            "\n",
        ] {
            fs::write(&state_path, raw).unwrap();
            assert!(read_process_state(&dir).unwrap_err().contains("is invalid"), "{raw:?}");
        }
        fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn socket_preparation_failure_removes_prior_reservations() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = PathBuf::from("/tmp").join(format!("jpk-socket-cleanup-{nanos}"));
        let blocked = dir.join("blocked");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&blocked, "not a socket").unwrap();
        let error = match prepare_sockets(
            &dir,
            &["first.sock".to_string(), "blocked".to_string()],
        ) {
            Ok(_) => panic!("socket preparation should fail on a regular file"),
            Err(error) => error,
        };
        assert!(error.contains("non-socket file"), "{error}");
        assert!(!dir.join("first.sock").exists());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn legacy_pid_state_is_removed_without_running_shutdown_or_signaling() {
        let dir = scratch("legacy-pid");
        let mut p = plan("fixture", "unused");
        p.shutdown = None;
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
        assert!(err.contains("no `run:`"), "{err}");
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
