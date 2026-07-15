//! U28 (D-JPK-NODAEMON1=A): jetpack runtime policy.
//!
//! Jetpack is a user-owned, one-shot process: no resident daemon, no root
//! requirement, cross-process coordination through lock files, and honest
//! sandbox fallback reporting. The only privileged future path is transient
//! `jetpack os switch` / jetos activation.

use super::Output::Theme;
use super::JSON;
use crate::Syntax;
use std::fs::{self, File};
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
    file: File,
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Close also releases advisory locks. Explicit unlock shortens the
        // handoff boundary and keeps that law visible at this abstraction.
        let _ = lock_platform::unlock(&self.file);
        // `LOCK_NB` waiters are not kernel-queued. Brief handoff grace stops a
        // hot loop from reacquiring before an already-polling peer can run.
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub fn acquire_lock(root: &Path, scope: &str) -> io::Result<FileLock> {
    acquire_lock_with_timing(root, scope, LOCK_WAIT, LOCK_POLL)
}

fn acquire_lock_with_timing(
    root: &Path,
    scope: &str,
    wait: Duration,
    poll: Duration,
) -> io::Result<FileLock> {
    let dir = root.join(LOCK_DIR);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.lock", sanitize_scope(scope)));
    let file = lock_platform::open(&path)?;
    let deadline = Instant::now() + wait;
    loop {
        match lock_platform::try_lock(&file) {
            Ok(true) => return Ok(FileLock { file }),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockState {
    Held,
    Idle,
}

/// Probe one persistent lock inode using kernel state. File contents, PID
/// reuse, timestamps, and path existence are never liveness evidence.
pub(crate) fn lock_state(path: &Path) -> io::Result<LockState> {
    let file = lock_platform::open(path)?;
    if lock_platform::try_lock(&file)? {
        lock_platform::unlock(&file)?;
        Ok(LockState::Idle)
    } else {
        Ok(LockState::Held)
    }
}

pub fn with_lock<T>(root: &Path, scope: &str, f: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let _lock = acquire_lock(root, scope)?;
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
    pub reason: String,
}

pub fn sandbox_policy_path() -> PathBuf {
    let home = std::env::var_os("HOME")
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
    crate::SHA256::sha256_hex(format!("jp0-policy-v1\n{sandbox}\n").as_bytes())
}

pub fn detect_sandbox() -> SandboxStatus {
    match std::env::var("JETPACK_FAKE_SANDBOX").ok().as_deref() {
        Some("available") => {
            return SandboxStatus {
                level: SandboxLevel::Fallback,
                mechanism: "fake-userns-available".to_string(),
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
                reason: "test override reports no sandbox support".to_string(),
            };
        }
        _ => {}
    }

    #[cfg(target_os = "linux")]
    {
        let userns = fs::read_to_string("/proc/sys/kernel/unprivileged_userns_clone")
            .map(|s| s.trim() != "0")
            .unwrap_or(true);
        if userns {
            return SandboxStatus {
                level: SandboxLevel::Fallback,
                mechanism: "linux-userns-available".to_string(),
                reason: concat!(
                    "unprivileged user namespaces are available, but the build ",
                    "child has not entered a Jetpack jail"
                )
                .to_string(),
            };
        }
        return SandboxStatus {
            level: SandboxLevel::Fallback,
            mechanism: "unsandboxed".to_string(),
            reason: "this kernel disables unprivileged user namespaces".to_string(),
        };
    }

    #[cfg(target_os = "macos")]
    {
        SandboxStatus {
            level: SandboxLevel::Fallback,
            mechanism: "unsandboxed".to_string(),
            reason: "macOS has no Jetpack-native unprivileged build sandbox yet".to_string(),
        }
    }

    #[cfg(target_os = "windows")]
    {
        SandboxStatus {
            level: SandboxLevel::Fallback,
            mechanism: "unsandboxed".to_string(),
            reason: "Windows has no Jetpack-native unprivileged build sandbox yet".to_string(),
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        SandboxStatus {
            level: SandboxLevel::Fallback,
            mechanism: "unsandboxed".to_string(),
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
                    "run `jetpack config sandbox allow` to permit fallback, or enable unprivileged sandbox support on this machine.",
                );
            }
            Err(2)
        }
        SandboxPolicy::AllowFallback => {
            if json {
                eprintln!("{}", sandbox_json("L0205", "warning", &status));
            } else {
                theme.warning_coded(
                    "L0205",
                    "build sandboxing unavailable; adapter builds will run unsandboxed",
                    &status.reason,
                    "run `jetpack config sandbox require` to refuse fallback.",
                );
            }
            Ok(())
        }
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
                "build sandboxing unavailable; adapter builds will run unsandboxed"
            },
        ),
        ("why", &status.reason),
        (
            "fix",
            if code == "E1275" {
                "run `jetpack config sandbox allow` to permit fallback, or enable unprivileged sandbox support on this machine"
            } else {
                "run `jetpack config sandbox require` to refuse fallback"
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
        assert!(path.exists(), "persistent lock inode must never be unlinked");
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
            "HANDLE_FLAG_INHERIT",
            ".share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)",
            "lockfileex_runtime_serializes_when_run_on_windows",
        ] {
            assert!(source.contains(required), "missing Windows lock law: {required}");
        }
        assert!(!source.contains("const FILE_SHARE_DELETE"));

        let unix = include_str!("RuntimePolicy/Lock/unix.rs");
        for required in ["F_GETFD", "F_SETFD", "FD_CLOEXEC", "LOCK_EX | LOCK_NB"] {
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
    fn capability_detection_never_claims_an_entered_sandbox() {
        let status = detect_sandbox();
        assert_eq!(status.level, SandboxLevel::Fallback);
        assert_ne!(status.mechanism, "linux-userns");
        assert!(status.reason.contains("not entered"));
    }
}
