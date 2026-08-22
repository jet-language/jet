//! Registry trust tiers and publish gates (D-REGCURATE1=C, card #1911).

use crate::Diagnostics::Diagnostic;
use std::path::{Path, PathBuf};

use super::Index::IndexEntry;
use super::NamePolicyDecision;

/// The two registry channels. Core is human-reviewed. Community is open only
/// after every machine gate passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryTier {
    Core,
    Community,
}

impl RegistryTier {
    pub fn label(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Community => "community",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "core" => Some(Self::Core),
            "community" => Some(Self::Community),
            _ => None,
        }
    }

    pub fn from_environment() -> Self {
        std::env::var("JET_REGISTRY_TIER")
            .ok()
            .and_then(|value| Self::parse(&value))
            .unwrap_or(Self::Core)
    }
}

/// State for one publish gate. `NotRequired` is used for gates that apply only
/// to the community channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateState {
    Passed,
    Blocked,
    NotRequired,
}

impl GateState {
    fn label(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Blocked => "blocked",
            Self::NotRequired => "not-required",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "passed" => Some(Self::Passed),
            "blocked" => Some(Self::Blocked),
            "not-required" => Some(Self::NotRequired),
            _ => None,
        }
    }
}

/// The gate result recorded beside every registry package version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateStatus {
    pub signature: GateState,
    pub audit: GateState,
    pub name: GateState,
    pub liveness: GateState,
    pub review: GateState,
}

impl GateStatus {
    pub fn core_reviewed() -> Self {
        Self {
            signature: GateState::NotRequired,
            audit: GateState::NotRequired,
            name: GateState::NotRequired,
            liveness: GateState::NotRequired,
            review: GateState::Passed,
        }
    }

    fn community_with_name(signature: GateState, audit: GateState, name: GateState) -> Self {
        Self {
            signature,
            audit,
            name,
            liveness: GateState::Blocked,
            review: GateState::NotRequired,
        }
    }

    pub fn community_open(&self) -> bool {
        matches!(
            (self.signature, self.audit, self.name, self.liveness),
            (
                GateState::Passed,
                GateState::Passed,
                GateState::Passed,
                GateState::Passed
            )
        )
    }

    pub fn summary(&self) -> String {
        format!(
            "signature={};audit={};name={};liveness={};review={}",
            self.signature.label(),
            self.audit.label(),
            self.name.label(),
            self.liveness.label(),
            self.review.label(),
        )
    }

    pub fn parse(summary: &str) -> Option<Self> {
        let mut signature = None;
        let mut audit = None;
        let mut name = None;
        let mut liveness = None;
        let mut review = None;
        for field in summary.split(';') {
            let (key, value) = field.split_once('=')?;
            let state = GateState::parse(value)?;
            let slot = match key {
                "signature" => &mut signature,
                "audit" => &mut audit,
                "name" => &mut name,
                "liveness" => &mut liveness,
                "review" => &mut review,
                _ => return None,
            };
            if slot.replace(state).is_some() {
                return None;
            }
        }
        Some(Self {
            signature: signature?,
            audit: audit?,
            name: name?,
            liveness: liveness?,
            review: review?,
        })
    }

    pub fn blocked_reasons(&self) -> Vec<&'static str> {
        let mut reasons = Vec::new();
        if self.signature != GateState::Passed {
            reasons.push("#935 live signature chain");
        }
        if self.audit != GateState::Passed {
            reasons.push("#431 advisory audit");
        }
        if self.name != GateState::Passed {
            reasons.push("#1912 name policy");
        }
        if self.liveness != GateState::Passed {
            reasons.push("#1913 maintainer liveness");
        }
        reasons
    }
}

/// Check the human review receipt committed by the core registry maintainers.
/// The receipt is deliberately outside the package source tree, so a package
/// author cannot approve their own core publication.
pub fn require_core_review(repo: &Path, package: &str, version: &str) -> Result<(), Diagnostic> {
    let path = repo
        .join("reviews")
        .join(package)
        .join(format!("{version}.review"));
    let text = std::fs::read_to_string(&path).map_err(|_| review_error(package, version, &path))?;
    let mut package_field = None;
    let mut version_field = None;
    let mut reviewer = None;
    let mut decision = None;
    let mut lines = text.lines();
    if lines.next() != Some("jet-registry-core-review-v1") {
        return Err(review_error(package, version, &path));
    }
    for line in lines {
        let Some((key, value)) = line.split_once('=') else {
            return Err(review_error(package, version, &path));
        };
        match key {
            "package" if package_field.is_none() => package_field = Some(value),
            "version" if version_field.is_none() => version_field = Some(value),
            "reviewer" if reviewer.is_none() => reviewer = Some(value),
            "decision" if decision.is_none() => decision = Some(value),
            _ => return Err(review_error(package, version, &path)),
        }
    }
    let valid = package_field == Some(package)
        && version_field == Some(version)
        && reviewer.is_some_and(|value| !value.trim().is_empty())
        && decision == Some("approved");
    if valid {
        Ok(())
    } else {
        Err(review_error(package, version, &path))
    }
}

/// Compute the community gate record before any artifact or index write.
pub fn community_gate_status(
    project_root: &Path,
    all_entries: &[IndexEntry],
    candidate: &IndexEntry,
    registry_name: &str,
    live_signature_chain: bool,
    name_policy: &NamePolicyDecision,
) -> GateStatus {
    let signature = if live_signature_chain
        && super::verify_index_entry(all_entries, candidate, true, registry_name).is_ok()
    {
        GateState::Passed
    } else {
        GateState::Blocked
    };
    let name = if name_policy.is_blocked() {
        GateState::Blocked
    } else {
        GateState::Passed
    };
    GateStatus::community_with_name(signature, advisory_gate_status(project_root), name)
}

pub fn community_gate_error(package: &str, version: &str, status: &GateStatus) -> Diagnostic {
    let reasons = status.blocked_reasons().join(", ");
    let required = "#935 live signature chain, #431 advisory audit, #1912 name policy, #1913 maintainer liveness";
    Diagnostic::error(
        "E2105",
        format!(
            "community-tier package `{package}` {version} is refused: {reasons} gate(s) are closed"
        ),
        format!(
            "the community tier requires all four gates ({required}); a package is accepted only after live signature, advisory, name, and maintainer checks pass"
        ),
        format!(
            "complete the closed gates ({reasons}); verify all four gates ({required}), then publish `{package}` {version} again"
        ),
        None,
    )
}

fn advisory_gate_status(project_root: &Path) -> GateState {
    let lock_path = project_root.join(".jet").join("lock");
    let Ok(lock_text) = std::fs::read_to_string(lock_path) else {
        return GateState::Blocked;
    };
    let Ok(lock) = crate::Lock::parse(&lock_text) else {
        return GateState::Blocked;
    };
    let path = std::env::var_os("JET_ADVISORY_DB")
        .map(PathBuf::from)
        .or_else(|| {
            let path = project_root.join(".jet").join("advisories.db");
            path.is_file().then_some(path)
        });
    let Some(path) = path else {
        return GateState::Blocked;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return GateState::Blocked;
    };
    let Ok(feed) = super::parse_advisory_feed(&text) else {
        return GateState::Blocked;
    };
    let trust = if let Some(public_key) = std::env::var_os("JET_ADVISORY_PUBLIC_KEY") {
        super::AdvisoryTrustRoot {
            public_key: public_key.to_string_lossy().trim().to_string(),
            ..Default::default()
        }
    } else {
        let trust_path = std::env::var_os("JET_ADVISORY_TRUST")
            .map(PathBuf::from)
            .unwrap_or_else(|| project_root.join(".jet").join("advisory-trust"));
        let Ok(trust_text) = std::fs::read_to_string(trust_path) else {
            return GateState::Blocked;
        };
        let Ok(trust) = super::parse_advisory_trust(&trust_text) else {
            return GateState::Blocked;
        };
        trust
    };
    let Ok(report) = super::audit_advisory_feed(&lock, &feed, &trust, super::advisory_now()) else {
        return GateState::Blocked;
    };
    if report.matches.is_empty() && report.maturity.is_empty() {
        GateState::Passed
    } else {
        GateState::Blocked
    }
}

fn review_error(package: &str, version: &str, path: &Path) -> Diagnostic {
    Diagnostic::error(
        "E2105",
        format!(
            "core-tier publish of `{package}` {version} has no approved review receipt"
        ),
        "the core tier contains only packages reviewed by a registry maintainer".to_string(),
        format!(
            "commit `jet-registry-core-review-v1` receipt `{}` with `decision=approved`, then publish again",
            path.display()
        ),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(label: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("jet_registry_tier_{label}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create tier test directory");
        path
    }

    #[test]
    fn gate_status_roundtrips_and_names_closed_gates() {
        let status = GateStatus::community_with_name(
            GateState::Blocked,
            GateState::Blocked,
            GateState::Blocked,
        );
        let parsed = GateStatus::parse(&status.summary()).expect("canonical gate status parses");
        assert_eq!(parsed, status);
        assert_eq!(
            status.blocked_reasons(),
            vec![
                "#935 live signature chain",
                "#431 advisory audit",
                "#1912 name policy",
                "#1913 maintainer liveness"
            ]
        );
        assert!(!status.community_open());
    }

    #[test]
    fn core_review_receipt_must_name_the_published_package() {
        let repo = scratch("review");
        let path = repo.join("reviews").join("textkit").join("1.0.0.review");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        assert!(require_core_review(&repo, "textkit", "1.0.0").is_err());
        fs::write(
            &path,
            "jet-registry-core-review-v1\npackage=textkit\nversion=1.0.0\nreviewer=owner\ndecision=approved\n",
        )
        .unwrap();
        require_core_review(&repo, "textkit", "1.0.0").expect("approved receipt passes");
        assert!(require_core_review(&repo, "textkit", "2.0.0").is_err());
        fs::remove_dir_all(repo).unwrap();
    }
}
