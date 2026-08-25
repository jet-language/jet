//! Policy gate for the one-shot local Nix compatibility import.
//!
//! This module owns only the invocation policy and its receipt. The provider
//! still owns lock identity, output parsing, and Store publication. Keeping
//! the gate before executable discovery is the important safety property:
//! `--offline` cannot even inspect a Nix executable.

use crate::JSON;
use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const POLICY_ENV: &str = "JETPACK_NIX_FALLBACK_POLICY";
const ALLOW_VALUE: &str = "allow";
const RECEIPT_SCHEMA: &str = "jetpack.nix-fallback-policy.v1";
const MAX_STDOUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Offline,
    Ci,
    NonInteractive,
    Interactive,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::Ci => "ci",
            Self::NonInteractive => "non-interactive",
            Self::Interactive => "interactive",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PolicyReceipt {
    mode: Mode,
    allowed: bool,
    explicit: bool,
    policy_value: String,
    reason: String,
    invocations: u8,
}

impl PolicyReceipt {
    fn json(&self) -> String {
        format!(
            "{{\"schema\":{},\"mode\":{},\"allowed\":{},\"explicit\":{},\"policy\":{},\"reason\":{},\"nix_invocations\":{}}}",
            JSON::quote(RECEIPT_SCHEMA),
            JSON::quote(self.mode.label()),
            if self.allowed { "true" } else { "false" },
            if self.explicit { "true" } else { "false" },
            JSON::quote(&self.policy_value),
            JSON::quote(&self.reason),
            self.invocations,
        )
    }

    fn facts(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("nix.fallback.policy.schema".into(), RECEIPT_SCHEMA.into()),
            ("nix.fallback.policy.mode".into(), self.mode.label().into()),
            (
                "nix.fallback.policy.allowed".into(),
                self.allowed.to_string(),
            ),
            (
                "nix.fallback.policy.explicit".into(),
                self.explicit.to_string(),
            ),
            (
                "nix.fallback.policy.value".into(),
                self.policy_value.clone(),
            ),
            (
                "nix.fallback.policy.reason".into(),
                self.reason.clone(),
            ),
            (
                "nix.fallback.policy.invocations".into(),
                self.invocations.to_string(),
            ),
            ("nix.fallback.policy.receipt".into(), self.json()),
        ])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PolicyError {
    receipt: PolicyReceipt,
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(output, "{}; receipt={}", self.receipt.reason, self.receipt.json())
    }
}

impl std::error::Error for PolicyError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FallbackRun {
    pub(crate) stdout: String,
    pub(crate) facts: BTreeMap<String, String>,
}

/// Run one exact-lock compatibility evaluation after the policy gate passes.
/// The only Nix build process is spawned after `authorize_from_environment`.
pub(crate) fn run(
    project: &Path,
    source_name: &str,
    revision: &str,
    system: &str,
    attr: &[String],
    offline: bool,
) -> Result<FallbackRun, String> {
    let policy = authorize_from_environment(offline).map_err(|error| error.to_string())?;
    let executable = discover_executable()?;
    let source_name = locked_source_name(project, source_name, revision)?;
    let identity = crate::NixIdentity::NixFallbackIdentity::from_project(
        project,
        &source_name,
        &executable,
        system,
        attr,
    )
    .map_err(|error| error.to_string())?;
    if identity.nixpkgs_revision != revision {
        return Err("local Nix fallback revision differs from the catalog miss".into());
    }
    let lock_sha256 = crate::NixIdentity::NixFallbackIdentity::project_lock_sha256(
        project,
        &source_name,
    )
    .map_err(|error| error.to_string())?;
    identity
        .validate_request(
            &identity.locked_nixpkgs_input(),
            system,
            attr,
            &lock_sha256,
        )
        .map_err(|error| error.to_string())?;

    // The lock stores the exact revision as provenance. Nix's flake CLI uses
    // the same immutable revision in the path component before the attr.
    let input = format!(
        "github:NixOS/nixpkgs/{}#{}",
        identity.nixpkgs_revision,
        identity.attrpath()
    );
    let output = Command::new(&identity.executable)
        .args([
            "build",
            "--json",
            "--no-link",
            "--no-write-lock-file",
            "--system",
            &identity.system,
        ])
        .arg(input)
        .env_remove("NIX_PATH")
        .env_remove("NIX_CONFIG")
        .env_remove("NIX_USER_CONF_FILES")
        .env_remove("NIX_REMOTE")
        .output()
        .map_err(|error| format!("could not invoke the bound Nix executable: {error}"))?;
    if output.stdout.len() > MAX_STDOUT_BYTES {
        return Err(format!(
            "local Nix fallback output exceeded {MAX_STDOUT_BYTES} bytes"
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| "local Nix fallback output is not UTF-8".to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        let stderr = if stderr.len() > MAX_STDERR_BYTES {
            &stderr[..MAX_STDERR_BYTES]
        } else {
            stderr
        };
        return Err(format!(
            "local Nix fallback failed with status {}; {}",
            output.status,
            stderr
        ));
    }

    let mut facts = policy.facts();
    facts.insert(
        "nix.fallback.provenance".into(),
        identity.provenance(),
    );
    facts.insert(
        "nix.fallback.executable".into(),
        identity.executable.to_string_lossy().into_owned(),
    );
    facts.insert(
        "nix.fallback.executable.sha256".into(),
        identity.executable_sha256.clone(),
    );
    facts.insert("nix.fallback.version".into(), identity.version.clone());
    facts.insert(
        "nix.fallback.locked-input".into(),
        identity.locked_nixpkgs_input(),
    );
    facts.insert("nix.fallback.system".into(), identity.system.clone());
    facts.insert("nix.fallback.attr".into(), identity.attrpath());
    facts.insert("nix.fallback.lock.sha256".into(), identity.lock_sha256.clone());
    facts.insert("nix.fallback.invocation".into(), "one-shot".into());
    Ok(FallbackRun { stdout, facts })
}

fn locked_source_name(project: &Path, preferred: &str, revision: &str) -> Result<String, String> {
    let expected = format!("github:NixOS/nixpkgs#{revision}");
    let lock = crate::Lock::load(project)
        .ok_or_else(|| "project lock is unavailable for local Nix fallback".to_string())?;
    let mut candidates = vec![preferred];
    if preferred != "nixpkgs" {
        candidates.push("nixpkgs");
    }
    candidates
        .into_iter()
        .find(|name| {
            lock.source_channels
                .iter()
                .any(|channel| channel.name == *name && channel.exact == expected)
        })
        .map(str::to_string)
        .ok_or_else(|| {
            "project lock has no exact nixpkgs source matching the catalog miss".to_string()
        })
}

fn discover_executable() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("JETPACK_NIX_FALLBACK_BIN") {
        let path = PathBuf::from(path);
        if path.as_os_str().is_empty() {
            return Err("JETPACK_NIX_FALLBACK_BIN is empty".into());
        }
        return Ok(path);
    }
    let path = std::env::var_os("PATH").ok_or_else(|| {
        "local Nix fallback requires an installed `nix` executable on PATH".to_string()
    })?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join("nix");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err("local Nix fallback requires an installed `nix` executable on PATH".into())
}

fn authorize_from_environment(offline: bool) -> Result<PolicyReceipt, PolicyError> {
    let ci = std::env::var_os("CI").is_some_and(|value| !value.is_empty());
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let policy = std::env::var(POLICY_ENV).unwrap_or_default();
    authorize(offline, ci, interactive, &policy)
}

pub(crate) fn allowed_from_environment(offline: bool) -> bool {
    authorize_from_environment(offline).is_ok()
}

fn authorize(
    offline: bool,
    ci: bool,
    interactive: bool,
    policy: &str,
) -> Result<PolicyReceipt, PolicyError> {
    let explicit = !policy.is_empty();
    let mode = if offline {
        Mode::Offline
    } else if ci {
        Mode::Ci
    } else if interactive {
        Mode::Interactive
    } else {
        Mode::NonInteractive
    };
    let valid_allow = policy == ALLOW_VALUE;
    let allowed = !offline && ((!ci && interactive) || valid_allow);
    let reason = if offline {
        "--offline forbids local Nix fallback; no executable was inspected or invoked".into()
    } else if (ci || !interactive) && !valid_allow {
        format!(
            "CI and non-interactive local Nix fallback require {POLICY_ENV}={ALLOW_VALUE}"
        )
    } else if explicit && !valid_allow {
        format!("{POLICY_ENV} must be exactly {ALLOW_VALUE}")
    } else {
        "local Nix fallback allowed by interactive policy".into()
    };
    let receipt = PolicyReceipt {
        mode,
        allowed,
        explicit,
        policy_value: if policy.is_empty() {
            "unset".into()
        } else {
            policy.into()
        },
        reason,
        invocations: if allowed { 1 } else { 0 },
    };
    if allowed {
        Ok(receipt)
    } else {
        Err(PolicyError { receipt })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_policy_refuses_before_any_nix_invocation() {
        let error = authorize(true, false, true, ALLOW_VALUE).unwrap_err();
        assert_eq!(error.receipt.mode, Mode::Offline);
        assert_eq!(error.receipt.invocations, 0);
        assert!(error
            .receipt
            .reason
            .contains("no executable was inspected or invoked"));
        assert!(error.receipt.json().contains("\"nix_invocations\":0"));
    }

    #[test]
    fn ci_and_non_interactive_modes_need_the_explicit_allow_policy() {
        for (ci, interactive) in [(true, true), (true, false), (false, false)] {
            let error = authorize(false, ci, interactive, "").unwrap_err();
            assert_eq!(error.receipt.invocations, 0);
            assert!(error.receipt.reason.contains(POLICY_ENV));
            assert!(error.receipt.reason.contains(ALLOW_VALUE));
        }
        assert_eq!(authorize(false, true, false, ALLOW_VALUE).unwrap().mode, Mode::Ci);
        assert_eq!(
            authorize(false, false, false, ALLOW_VALUE)
                .unwrap()
                .mode,
            Mode::NonInteractive
        );
    }

    #[test]
    fn interactive_mode_can_use_local_fallback_without_ci_policy() {
        let receipt = authorize(false, false, true, "").unwrap();
        assert_eq!(receipt.mode, Mode::Interactive);
        assert!(!receipt.explicit);
        assert_eq!(receipt.invocations, 1);
        let facts = receipt.facts();
        let parsed = JSON::parse(
            facts
                .get("nix.fallback.policy.receipt")
                .expect("policy receipt fact"),
        )
        .expect("machine-readable policy receipt");
        assert_eq!(parsed.get("schema").unwrap().as_str().unwrap(), RECEIPT_SCHEMA);
        assert!(matches!(
            parsed.get("allowed").unwrap(),
            JSON::JSONValue::Bool(true)
        ));
    }

    #[test]
    fn invalid_policy_is_not_treated_as_allow() {
        let error = authorize(false, false, false, "yes").unwrap_err();
        assert_eq!(error.receipt.policy_value, "yes");
        assert_eq!(error.receipt.invocations, 0);
    }
}
