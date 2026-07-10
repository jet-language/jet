//! U28 (D-JPK-NODAEMON1=A): jetpack runtime policy.
//!
//! Jetpack is a user-owned, one-shot process: no resident daemon, no root
//! requirement, cross-process coordination through lock files, and honest
//! sandbox fallback reporting. The only privileged future path is transient
//! `jetpack os switch` / jetos activation.

use super::Output::Theme;
use super::JSON;
use crate::Syntax;
use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const LOCK_DIR: &str = ".locks";
const LOCK_WAIT: Duration = Duration::from_secs(30);
const LOCK_POLL: Duration = Duration::from_millis(20);
const SANDBOX_POLICY_FILE: &str = "sandbox-policy";

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
    path: PathBuf,
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn acquire_lock(root: &Path, scope: &str) -> io::Result<FileLock> {
    let dir = root.join(LOCK_DIR);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.lock", sanitize_scope(scope)));
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                let _ = writeln!(f, "pid={}", std::process::id());
                return Ok(FileLock { path });
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists && Instant::now() < deadline => {
                std::thread::sleep(LOCK_POLL);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                return Err(io::Error::new(
                    ErrorKind::TimedOut,
                    format!("timed out waiting for jetpack lock `{}`", path.display()),
                ));
            }
            Err(e) => return Err(e),
        }
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
