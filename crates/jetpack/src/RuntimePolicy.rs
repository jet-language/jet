//! U28 (D-JPK-NODAEMON1=A): jetpack runtime policy.
//!
//! Jetpack is a user-owned, one-shot process: no resident daemon, no root
//! requirement, cross-process coordination through lock files, and honest
//! native child-sandbox reporting. Executable actions use a substitute-first,
//! fail-closed boundary: non-executing reuse may proceed, but an unavailable
//! native backend never becomes an unsandboxed launch. The only privileged
//! future path is
//! transient `jetpack os switch` / jetos activation.

use super::Output::Theme;
use super::JSON;
use crate::Syntax;
use std::cell::RefCell;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const LOCK_DIR: &str = ".locks";
const LOCK_WAIT: Duration = Duration::from_secs(30);
// Short polling prevents a hot reacquirer from starving an already-open
// contender while retaining deterministic timeout control.
const LOCK_POLL: Duration = Duration::from_millis(1);
const SANDBOX_POLICY_FILE: &str = "sandbox-policy";

#[cfg(unix)]
#[path = "RuntimePolicy/Lock/unix.rs"]
mod lock_platform;
#[cfg(windows)]
#[path = "RuntimePolicy/Lock/windows.rs"]
mod lock_platform;
#[cfg(not(any(unix, windows)))]
#[path = "RuntimePolicy/Lock/unsupported.rs"]
mod lock_platform;

#[path = "RuntimePolicy/ExecutableLease.rs"]
pub(crate) mod executable_lease;
pub(crate) use executable_lease::{ExecutableLeaseProtocol, LeaseMember};

pub(crate) fn sandbox_backend_name() -> &'static str {
    #[cfg(windows)]
    {
        "windows-appcontainer"
    }
    #[cfg(target_os = "macos")]
    {
        "macos-seatbelt"
    }
    #[cfg(target_os = "linux")]
    {
        "linux-bwrap"
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        "unsandboxed-fallback"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerbPolicy {
    pub verb: &'static str,
    pub resident_daemon: bool,
    pub requires_root: bool,
    pub transient_sudo: bool,
}

pub fn verb_policy(verb: &str, args: &[&str]) -> VerbPolicy {
    let transient_sudo = verb == Syntax::OS_SUBCOMMAND && args.first().copied() == Some("switch");
    let verb = Syntax::JETPACK_VERBS
        .iter()
        .copied()
        .find(|v| *v == verb)
        .unwrap_or("unknown");
    VerbPolicy {
        verb,
        resident_daemon: false,
        requires_root: false,
        transient_sudo,
    }
}

pub fn all_verb_policies() -> Vec<VerbPolicy> {
    Syntax::JETPACK_VERBS
        .iter()
        .map(|v| verb_policy(v, &[]))
        .collect()
}

pub struct FileLock {
    file: lock_platform::LockFile,
    release_on_drop: bool,
}

thread_local! {
    static HELD_LOCK_ROOTS: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

struct LockGuard {
    root: PathBuf,
    lock: Option<FileLock>,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        HELD_LOCK_ROOTS.with(|roots| {
            let mut roots = roots.borrow_mut();
            if let Some(index) = roots.iter().rposition(|root| root == &self.root) {
                roots.remove(index);
            }
        });
        let _ = self.lock.take();
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        if self.release_on_drop {
            // Close also releases advisory locks. Explicit unlock shortens the
            // handoff boundary and keeps that law visible at this abstraction.
            let _ = lock_platform::unlock(&self.file);
            // `LOCK_NB` waiters are not kernel-queued. Brief handoff grace stops a
            // hot loop from reacquiring before an already-polling peer can run.
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

pub fn acquire_lock(root: &Path, scope: &str) -> io::Result<FileLock> {
    acquire_lock_with_timing(root, scope, LOCK_WAIT, LOCK_POLL)
}

pub(crate) fn lock_path_for_scope(root: &Path, scope: &str) -> PathBuf {
    root.join(LOCK_DIR)
        .join(format!("{}.lock", sanitize_scope(scope)))
}

fn acquire_lock_with_timing(
    root: &Path,
    scope: &str,
    wait: Duration,
    poll: Duration,
) -> io::Result<FileLock> {
    acquire_lock_with_timing_and_hook(root, scope, wait, poll, |_| Ok(()))
}

fn acquire_lock_with_timing_and_hook(
    root: &Path,
    scope: &str,
    wait: Duration,
    poll: Duration,
    before_open: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<FileLock> {
    let dir = root.join(LOCK_DIR);
    fs::create_dir_all(&dir)?;
    let path = lock_path_for_scope(root, scope);
    before_open(&path)?;
    let file = lock_platform::open(&path)?;
    let deadline = Instant::now() + wait;
    loop {
        match lock_platform::try_lock(&file) {
            Ok(true) => {
                if let Err(error) = lock_platform::validate_path(&file, &path) {
                    let _ = lock_platform::unlock(&file);
                    return Err(error);
                }
                return Ok(FileLock {
                    file,
                    release_on_drop: true,
                });
            }
            Ok(false) if Instant::now() < deadline => {
                std::thread::sleep(poll.min(deadline.saturating_duration_since(Instant::now())));
            }
            Ok(false) => {
                return Err(io::Error::new(
                    ErrorKind::TimedOut,
                    format!("timed out waiting for jetpack lock `{}`", path.display()),
                ));
            }
            Err(e) => return Err(e),
        }
    }
}

/// Acquire a lease lifetime lock and leave its handle inheritable by every
/// launched descendant. The owning process closes its copy after launch; the
/// kernel lock remains held until the last descendant closes its inherited
/// handle, which gives recovery a process-tree lifetime without PID guesses.
pub(crate) fn acquire_lease_lock(root: &Path, scope: &str) -> io::Result<FileLock> {
    let mut lock = acquire_lock(root, scope)?;
    lock_platform::set_inheritable(&lock.file)?;
    lock.release_on_drop = false;
    Ok(lock)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockState {
    Absent,
    Held,
    Idle,
}

/// Probe one persistent lock inode using kernel state. File contents, PID
/// reuse, timestamps, and path existence are never liveness evidence.
pub(crate) fn lock_state(path: &Path) -> io::Result<LockState> {
    let file = match lock_platform::open_existing(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(LockState::Absent),
        Err(error) => return Err(error),
    };
    if let Err(error) = lock_platform::validate_path(&file, path) {
        return if error.kind() == ErrorKind::NotFound {
            Ok(LockState::Absent)
        } else {
            Err(error)
        };
    }
    if lock_platform::try_lock(&file)? {
        lock_platform::unlock(&file)?;
        Ok(LockState::Idle)
    } else {
        Ok(LockState::Held)
    }
}

// All scopes for one managed root share one kernel lock domain; nested
// same-thread operations reuse the guard already held for that root.
pub fn with_lock<T>(root: &Path, scope: &str, f: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let key = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if HELD_LOCK_ROOTS.with(|roots| roots.borrow().iter().any(|held| held == &key)) {
        return f();
    }
    let lock = acquire_lock(root, scope)?;
    let key = fs::canonicalize(root).unwrap_or(key);
    HELD_LOCK_ROOTS.with(|roots| roots.borrow_mut().push(key.clone()));
    let _guard = LockGuard {
        root: key,
        lock: Some(lock),
    };
    f()
}

pub fn with_project_lock<T>(
    project: &Path,
    scope: &str,
    f: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    let root = super::Store::managed_dir(project);
    with_lock(&root, scope, f)
}

fn sanitize_scope(scope: &str) -> String {
    let mut out = String::new();
    for c in scope.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "global".to_string()
    } else {
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxPolicy {
    AllowFallback,
    Require,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxLevel {
    Strong,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxStatus {
    pub level: SandboxLevel,
    pub mechanism: String,
    pub policy: String,
    pub reason: String,
}

/// Validate the shape of a persisted child-boundary receipt before presenting
/// it as an isolation claim. The producer checksum proves record integrity;
/// this check proves that the recorded class and policy describe one of the
/// native backends this release can actually enforce.
pub(crate) fn sandbox_receipt_is_truthful(class: &str, policy: &str) -> bool {
    if class == "non-executing" {
        return policy == "no child launched"
            || policy.starts_with("no child launched (")
            || policy.starts_with("trusted substitution (");
    }
    let mut fields = std::collections::BTreeMap::new();
    for field in policy.split(';') {
        let Some((name, value)) = field.split_once('=') else {
            return false;
        };
        if name.is_empty() || value.is_empty() || fields.insert(name, value).is_some() {
            return false;
        }
    }
    let has =
        |name: &str, values: &[&str]| fields.get(name).is_some_and(|value| values.contains(value));
    match class {
        "linux-bwrap" => {
            fields.len() == 7
                && has(
                    "filesystem",
                    &[
                        "source-readonly,output-private-copy",
                        "private-workspace-readwrite",
                    ],
                )
                && has("process", &["private-pid,parent-death"])
                && has("network", &["isolated", "declared-shared"])
                && has("environment", &["clear"])
                && has("devices", &["private-dev"])
                && has("privilege", &["no-new-privs+cap-drop-all"])
                && has("resources", &["tmpfs-64MiB"])
        }
        "macos-seatbelt" => {
            fields.len() == 6
                && has(
                    "filesystem",
                    &[
                        "source-readonly,output-readwrite",
                        "private-workspace-readwrite",
                    ],
                )
                && has("process", &["declared-tool-and-fork"])
                && has("network", &["denied", "declared-shared"])
                && has("environment", &["clear"])
                && has("devices", &["denied"])
                && has("resources", &["none-declared"])
        }
        "windows-appcontainer" => {
            fields.len() == 6
                && has(
                    "filesystem",
                    &[
                        "private-workspace-readwrite",
                        "source-readonly,output-readwrite",
                        "source-readonly",
                        "filesystem-none",
                        "filesystem-write-only",
                    ],
                )
                && has(
                    "process",
                    &["appcontainer+job-kill-on-close+active-process=256"],
                )
                && has("network", &["denied", "declared-internet-client"])
                && has("environment", &["clear+declared"])
                && has("devices", &["appcontainer-default-deny"])
                && has("resources", &["memory-2GiB+active-process-256"])
        }
        _ => false,
    }
}

pub fn sandbox_policy_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(Syntax::CONFIG_DEFAULT_DIR)
        .join(SANDBOX_POLICY_FILE)
}

pub fn read_sandbox_policy() -> SandboxPolicy {
    match fs::read_to_string(sandbox_policy_path()) {
        Ok(s) if s.trim() == Syntax::CONFIG_SANDBOX_VERB_REQUIRE => SandboxPolicy::Require,
        _ => SandboxPolicy::AllowFallback,
    }
}

pub fn write_sandbox_policy(policy: SandboxPolicy) -> io::Result<PathBuf> {
    let path = sandbox_policy_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = match policy {
        SandboxPolicy::AllowFallback => Syntax::CONFIG_SANDBOX_VERB_ALLOW,
        SandboxPolicy::Require => Syntax::CONFIG_SANDBOX_VERB_REQUIRE,
    };
    fs::write(&path, format!("{text}\n"))?;
    Ok(path)
}

/// Identity of artifact-producing policy JP0 can currently enforce. Invocation
/// transport (`--offline`) is deliberately excluded: consuming identical local
/// bytes offline must not invalidate them. Policy changes make prior records
/// miss instead of silently crossing an authority boundary.
pub fn cache_policy_fingerprint(_offline: bool) -> String {
    let sandbox = match read_sandbox_policy() {
        SandboxPolicy::AllowFallback => "sandbox=fallback",
        SandboxPolicy::Require => "sandbox=require",
    };
    let backend = sandbox_backend_name();
    crate::SHA256::sha256_hex(format!("jp0-policy-v3\n{sandbox}\nbackend={backend}\n").as_bytes())
}

pub fn detect_sandbox() -> SandboxStatus {
    match std::env::var("JETPACK_FAKE_SANDBOX").ok().as_deref() {
        Some("available") => {
            return SandboxStatus {
                level: SandboxLevel::Fallback,
                mechanism: "fake-userns-available".to_string(),
                policy: "not-enforced".to_string(),
                reason: concat!(
                    "test override reports sandbox support, but the build child ",
                    "has not entered a jail"
                )
                .to_string(),
            };
        }
        Some("unavailable") => {
            return SandboxStatus {
                level: SandboxLevel::Fallback,
                mechanism: "unsandboxed".to_string(),
                policy: "not-enforced".to_string(),
                reason: "test override reports no sandbox support".to_string(),
            };
        }
        _ => {}
    }

    #[cfg(target_os = "linux")]
    {
        let native = jet_comptime::Comptime::Build::native_sandbox_status();
        return SandboxStatus {
            level: if native.available {
                SandboxLevel::Strong
            } else {
                SandboxLevel::Fallback
            },
            mechanism: native.mechanism,
            policy: native.policy,
            reason: native.reason,
        };
    }

    #[cfg(target_os = "macos")]
    {
        let native = jet_comptime::Comptime::Build::native_sandbox_status();
        SandboxStatus {
            level: if native.available {
                SandboxLevel::Strong
            } else {
                SandboxLevel::Fallback
            },
            mechanism: native.mechanism,
            policy: native.policy,
            reason: native.reason,
        }
    }

    #[cfg(target_os = "windows")]
    {
        let native = jet_comptime::Comptime::Build::native_sandbox_status();
        SandboxStatus {
            level: if native.available {
                SandboxLevel::Strong
            } else {
                SandboxLevel::Fallback
            },
            mechanism: native.mechanism,
            policy: native.policy,
            reason: native.reason,
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        SandboxStatus {
            level: SandboxLevel::Fallback,
            mechanism: "unsandboxed".to_string(),
            policy: "not-enforced".to_string(),
            reason: "this platform has no Jetpack-native unprivileged build sandbox yet"
                .to_string(),
        }
    }
}

pub fn enforce_sandbox_policy(theme: &Theme, json: bool) -> Result<(), i32> {
    let status = detect_sandbox();
    if status.level == SandboxLevel::Strong {
        return Ok(());
    }
    match read_sandbox_policy() {
        SandboxPolicy::Require => {
            if json {
                eprintln!("{}", sandbox_json("E1275", "error", &status));
            } else {
                theme.error_coded(
                    "E1275",
                    "build sandboxing is required but unavailable",
                    &status.reason,
                    "provide a trusted substitute or approved remote builder, or enable the native sandbox, then retry.",
                );
            }
            Err(2)
        }
        SandboxPolicy::AllowFallback => {
            warn_sandbox_fallback(theme);
            Ok(())
        }
    }
}

/// Report the allow-mode capability loss at an executable-action boundary.
/// This is a lint only: the Store may still satisfy the action from a
/// verified substitute, but a local child must fail with E1275 when no
/// substitute is available.
pub(crate) fn warn_sandbox_fallback(theme: &Theme) {
    let status = detect_sandbox();
    if status.level == SandboxLevel::Fallback
        && read_sandbox_policy() == SandboxPolicy::AllowFallback
    {
        theme.warning_coded(
            "L0205",
            "build sandboxing is unavailable; local executable actions will be refused after substitution/remote resolution",
            &status.reason,
            "provide a trusted substitute or approved remote builder, or enable the native sandbox",
        );
    }
}

fn sandbox_json(code: &str, severity: &str, status: &SandboxStatus) -> String {
    JSON::object_of(&[
        ("schema_version", "1"),
        ("code", code),
        ("severity", severity),
        (
            "message",
            if code == "E1275" {
                "build sandboxing is required but unavailable"
            } else {
                "build sandboxing unavailable; local executable actions will be refused after substitution/remote resolution"
            },
        ),
        ("why", &status.reason),
        ("policy", &status.policy),
        (
            "fix",
            if code == "E1275" {
                "provide a trusted substitute or approved remote builder, or enable the native sandbox, then retry"
            } else {
                "provide a trusted substitute or approved remote builder, or enable the native sandbox"
            },
        ),
        ("mechanism", &status.mechanism),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn scratch(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "jpk-runtime-policy-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn lock_serializes_threads() {
        let root = scratch("lock");
        let guard = acquire_lock(&root, "hangar").unwrap();
        let probe = root.clone();
        let handle = std::thread::spawn(move || {
            let _guard = acquire_lock(&probe, "hangar").unwrap();
            fs::write(probe.join("after"), "locked").unwrap();
        });
        std::thread::sleep(Duration::from_millis(60));
        assert!(!root.join("after").exists());
        drop(guard);
        handle.join().unwrap();
        assert_eq!(fs::read_to_string(root.join("after")).unwrap(), "locked");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn project_lock_allows_nested_same_thread_scopes() {
        let root = scratch("nested-project-lock");
        let result = with_project_lock(&root, "outer", || {
            with_project_lock(&root, "inner", || {
                fs::write(root.join("nested"), "ok")?;
                Ok(())
            })
        });
        result.unwrap();
        assert_eq!(fs::read_to_string(root.join("nested")).unwrap(), "ok");
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn killed_lock_holder_child() {
        if std::env::var_os("JETPACK_LOCK_CHILD").is_none() {
            return;
        }
        let root = PathBuf::from(std::env::var_os("JETPACK_LOCK_ROOT").unwrap());
        let ready = PathBuf::from(std::env::var_os("JETPACK_LOCK_READY").unwrap());
        let _guard = acquire_lock(&root, "killed-holder").unwrap();
        fs::write(ready, "held").unwrap();
        std::thread::sleep(Duration::from_secs(60));
    }

    #[test]
    fn killed_holder_is_immediately_recoverable() {
        let root = scratch("killed-holder");
        let ready = root.join("ready");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "RuntimePolicy::tests::killed_lock_holder_child",
                "--nocapture",
            ])
            .env("JETPACK_LOCK_CHILD", "1")
            .env("JETPACK_LOCK_ROOT", &root)
            .env("JETPACK_LOCK_READY", &ready)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(ready.exists(), "child never acquired advisory lock");
        child.kill().unwrap();
        child.wait().unwrap();

        let started = Instant::now();
        let guard = acquire_lock_with_timing(
            &root,
            "killed-holder",
            Duration::from_secs(1),
            Duration::from_millis(5),
        )
        .unwrap();
        assert!(started.elapsed() < Duration::from_millis(250));
        drop(guard);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn waiter_and_new_opener_never_split_lock_inode() {
        let root = scratch("one-inode");
        let first = acquire_lock(&root, "hangar").unwrap();
        let lock_path = root.join(LOCK_DIR).join("hangar.lock");
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::MetadataExt as _;
            let metadata = fs::metadata(&lock_path).unwrap();
            (metadata.dev(), metadata.ino())
        };
        let active = Arc::new(AtomicUsize::new(0));
        let enters = Arc::new(AtomicUsize::new(0));
        let spawn = |root: PathBuf, active: Arc<AtomicUsize>, enters: Arc<AtomicUsize>| {
            std::thread::spawn(move || {
                let _guard = acquire_lock(&root, "hangar").unwrap();
                assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                enters.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(20));
                assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
            })
        };
        let waiter = spawn(root.clone(), active.clone(), enters.clone());
        std::thread::sleep(Duration::from_millis(40));
        let new_opener = spawn(root.clone(), active.clone(), enters.clone());
        drop(first);
        waiter.join().unwrap();
        new_opener.join().unwrap();
        assert_eq!(enters.load(Ordering::SeqCst), 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            let metadata = fs::metadata(lock_path).unwrap();
            assert_eq!((metadata.dev(), metadata.ino()), inode);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn swap_before_open_is_rejected_atomically() {
        let root = scratch("swap-before-open");
        let target = root.join("attacker-target");
        fs::write(&target, "not a lock").unwrap();
        let error = acquire_lock_with_timing_and_hook(
            &root,
            "hangar",
            Duration::from_millis(50),
            Duration::from_millis(5),
            |path| std::os::unix::fs::symlink(&target, path),
        )
        .err()
        .expect("O_NOFOLLOW must reject a last-moment symlink swap");
        assert_ne!(error.kind(), ErrorKind::TimedOut);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dual_name_replacement_cannot_split_canonical_root_lock() {
        let root = scratch("dual-replace-fail-closed");
        let first = acquire_lock(&root, "hangar").unwrap();
        let path = root.join(LOCK_DIR).join("hangar.lock");
        let anchor = root.join(LOCK_DIR).join("hangar.lock.anchor");
        fs::hard_link(&path, &anchor).unwrap();
        let displaced = root.join(LOCK_DIR).join("displaced.lock");
        let displaced_anchor = root.join(LOCK_DIR).join("displaced.anchor");
        let waiter_file = lock_platform::open(&path).unwrap();

        fs::rename(&path, &displaced).unwrap();
        fs::rename(&anchor, &displaced_anchor).unwrap();
        fs::write(&path, "replacement").unwrap();
        fs::hard_link(&path, &anchor).unwrap();
        assert!(
            acquire_lock_with_timing(
                &root,
                "hangar",
                Duration::from_millis(50),
                Duration::from_millis(5),
            )
            .is_err(),
            "fresh opener must contend on canonical root despite replacing both old names"
        );

        drop(first);
        assert!(lock_platform::try_lock(&waiter_file).unwrap());
        assert!(lock_platform::validate_path(&waiter_file, &path).is_err());
        lock_platform::unlock(&waiter_file).unwrap();
        let replacement = acquire_lock(&root, "hangar").unwrap();
        drop(replacement);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_pid_text_and_file_age_are_not_liveness() {
        let root = scratch("stale-text");
        let dir = root.join(LOCK_DIR);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hangar.lock");
        fs::write(&path, "pid=4294967294\nprocess_start=1\n").unwrap();
        let guard = acquire_lock_with_timing(
            &root,
            "hangar",
            Duration::from_millis(100),
            Duration::from_millis(5),
        )
        .unwrap();
        assert_eq!(lock_state(&path).unwrap(), LockState::Held);
        drop(guard);
        assert_eq!(lock_state(&path).unwrap(), LockState::Idle);
        assert!(
            path.exists(),
            "persistent lock inode must never be unlinked"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn absent_lock_probe_is_read_only() {
        let root = scratch("absent-probe");
        let path = root.join(LOCK_DIR).join("missing.lock");
        assert_eq!(lock_state(&path).unwrap(), LockState::Absent);
        assert!(!path.exists());
        assert!(!root.join(LOCK_DIR).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lock_timeout_is_deterministic() {
        let root = scratch("timeout");
        let guard = acquire_lock(&root, "hangar").unwrap();
        let started = Instant::now();
        let error = acquire_lock_with_timing(
            &root,
            "hangar",
            Duration::from_millis(80),
            Duration::from_millis(5),
        )
        .err()
        .expect("contended advisory lock must time out");
        let elapsed = started.elapsed();
        assert_eq!(error.kind(), ErrorKind::TimedOut);
        assert!(elapsed >= Duration::from_millis(60), "elapsed: {elapsed:?}");
        assert!(elapsed < Duration::from_millis(500), "elapsed: {elapsed:?}");
        drop(guard);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lock_open_and_platform_failures_fail_closed() {
        let root = scratch("fail-closed");
        fs::write(root.join(LOCK_DIR), "not a directory").unwrap();
        assert!(acquire_lock(&root, "hangar").is_err());

        fs::remove_file(root.join(LOCK_DIR)).unwrap();
        fs::create_dir_all(root.join(LOCK_DIR).join("hangar.lock")).unwrap();
        assert!(acquire_lock(&root, "hangar").is_err());
        fs::remove_dir_all(root).unwrap();

        #[cfg(unix)]
        {
            let root = scratch("symlink-fail-closed");
            fs::create_dir_all(root.join(LOCK_DIR)).unwrap();
            fs::write(root.join("target"), "not a lock inode").unwrap();
            std::os::unix::fs::symlink(
                root.join("target"),
                root.join(LOCK_DIR).join("hangar.lock"),
            )
            .unwrap();
            assert!(acquire_lock(&root, "hangar").is_err());
            fs::remove_dir_all(root).unwrap();
        }

        let unsupported = include_str!("RuntimePolicy/Lock/unsupported.rs");
        assert!(unsupported.contains("ErrorKind::Unsupported"));
        assert!(!unsupported.contains("Ok(File"));
    }

    #[test]
    fn windows_lock_contract_is_explicit_and_fail_closed() {
        let source = include_str!("RuntimePolicy/Lock/windows.rs");
        for required in [
            "LockFileEx",
            "UnlockFileEx",
            "LOCKFILE_FAIL_IMMEDIATELY",
            "LOCKFILE_EXCLUSIVE_LOCK",
            "SetHandleInformation",
            "GetFileInformationByHandle",
            "HANDLE_FLAG_INHERIT",
            "FILE_FLAG_OPEN_REPARSE_POINT",
            "FILE_FLAG_BACKUP_SEMANTICS",
            "FILE_ATTRIBUTE_REPARSE_POINT",
            ".share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)",
            "pin_parent_components",
            "open_directory_raw",
            "parent_junction_is_rejected_before_lock_leaf_open",
            "parent_replacement_is_blocked_while_component_identity_is_pinned",
            "lockfileex_runtime_serializes_and_pins_file_identity",
            "reparse_points_fail_closed_when_creation_is_permitted",
        ] {
            assert!(
                source.contains(required),
                "missing Windows lock law: {required}"
            );
        }
        assert!(!source.contains("const FILE_SHARE_DELETE"));

        let unix = include_str!("RuntimePolicy/Lock/unix.rs");
        for required in [
            "F_GETFD",
            "F_SETFD",
            "FD_CLOEXEC",
            "LOCK_EX | LOCK_NB",
            "O_NOFOLLOW",
            "canonical_owner",
            "symlink_metadata",
            "Authority lives on canonical managed-root directory inode",
        ] {
            assert!(unix.contains(required), "missing Unix lock law: {required}");
        }
    }

    #[test]
    fn os_switch_is_only_transient_sudo_exception() {
        for policy in all_verb_policies() {
            assert!(!policy.resident_daemon);
            assert!(!policy.requires_root);
            assert!(!policy.transient_sudo);
        }
        let os_switch = verb_policy(Syntax::OS_SUBCOMMAND, &["switch"]);
        assert!(os_switch.transient_sudo);
        assert!(!os_switch.requires_root);
    }

    #[test]
    fn capability_detection_reports_only_a_verified_backend() {
        let status = detect_sandbox();
        match status.level {
            SandboxLevel::Strong => {
                assert!(status.policy.contains("filesystem="));
                #[cfg(target_os = "linux")]
                {
                    assert_eq!(status.mechanism, "linux-bwrap");
                    assert!(status.policy.contains("process=private-pid"));
                }
                #[cfg(target_os = "macos")]
                {
                    assert_eq!(status.mechanism, "macos-seatbelt");
                    assert!(status.policy.contains("network=denied"));
                }
                #[cfg(target_os = "windows")]
                {
                    assert_eq!(status.mechanism, "windows-appcontainer");
                    assert!(status.policy.contains("appcontainer"));
                    assert!(status.policy.contains("job-kill-on-close"));
                }
            }
            SandboxLevel::Fallback => {
                assert_eq!(status.policy, "not-enforced");
                assert_ne!(status.mechanism, "linux-userns");
                assert!(!status.reason.is_empty());
            }
        }
    }

    #[test]
    fn sandbox_receipts_require_a_native_backend_and_complete_policy() {
        assert!(sandbox_receipt_is_truthful(
            "linux-bwrap",
            "filesystem=private-workspace-readwrite;process=private-pid,parent-death;network=isolated;environment=clear;devices=private-dev;privilege=no-new-privs+cap-drop-all;resources=tmpfs-64MiB"
        ));
        assert!(sandbox_receipt_is_truthful(
            "macos-seatbelt",
            "filesystem=source-readonly,output-readwrite;process=declared-tool-and-fork;network=denied;environment=clear;devices=denied;resources=none-declared"
        ));
        assert!(sandbox_receipt_is_truthful(
            "windows-appcontainer",
            "filesystem=source-readonly,output-readwrite;process=appcontainer+job-kill-on-close+active-process=256;network=denied;environment=clear+declared;devices=appcontainer-default-deny;resources=memory-2GiB+active-process-256"
        ));
        assert!(sandbox_receipt_is_truthful(
            "non-executing",
            "no child launched (rlib already present)"
        ));
        assert!(!sandbox_receipt_is_truthful(
            "linux-userns",
            "filesystem=private-workspace-readwrite;process=private-pid,parent-death;network=isolated;environment=clear;devices=private-dev;privilege=no-new-privs+cap-drop-all;resources=tmpfs-64MiB"
        ));
        assert!(!sandbox_receipt_is_truthful(
            "linux-bwrap",
            "filesystem=private-workspace-readwrite"
        ));
        assert!(!sandbox_receipt_is_truthful("linux-bwrap", "not-enforced"));
        assert!(!sandbox_receipt_is_truthful(
            "linux-bwrap",
            "filesystem=private-workspace-readwrite;process=host;network=isolated;environment=clear;devices=private-dev;privilege=no-new-privs+cap-drop-all;resources=tmpfs-64MiB"
        ));
        assert!(!sandbox_receipt_is_truthful(
            "linux-bwrap",
            "filesystem=private-workspace-readwrite;process=private-pid,parent-death;network=isolated;environment=clear;devices=private-dev;privilege=no-new-privs+cap-drop-all;resources=unlimited"
        ));
        assert!(!sandbox_receipt_is_truthful(
            "macos-seatbelt",
            "filesystem=private-workspace-readwrite;process=private-pid,parent-death;network=isolated;environment=clear;devices=private-dev;privilege=no-new-privs+cap-drop-all;resources=tmpfs-64MiB"
        ));
    }
}
