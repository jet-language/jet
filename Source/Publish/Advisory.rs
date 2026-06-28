use crate::Diagnostics::Diagnostic;
use crate::Lock::{LockFile, LockedPackage};
use std::path::Path;

use super::SemVer::{SemVer, VersionReq};

// ──────────────────────────────────────────────
// Advisory database
// ──────────────────────────────────────────────

/// Advisory severity (D-SUPPLY1). `jet audit` exits nonzero only when a
/// `Critical` advisory matches; lower severities are advisory and exit 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    /// Parse a severity word (case-insensitive). Unknown / empty → `Medium`,
    /// so a database that omits the field is treated as advisory, not fatal.
    pub fn parse(s: &str) -> Severity {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" => Severity::Low,
            "high" => Severity::High,
            "critical" | "crit" => Severity::Critical,
            _ => Severity::Medium,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
            Severity::Critical => "critical",
        }
    }
}

/// One advisory entry. In v1 the database is a list of these structs, loaded
/// from a plain text format (one JSON-like record per advisory).
#[derive(Debug, Clone)]
pub struct Advisory {
    /// Unique identifier, e.g. `JET-2026-0001` or a CVE ID.
    pub id: String,
    pub package: String,
    /// Version range where the vulnerability is present.
    pub affected: VersionReq,
    /// First version where the fix is available, if known.
    pub fixed: Option<SemVer>,
    pub title: String,
    /// Advisory severity — only `Critical` makes `jet audit` exit nonzero.
    pub severity: Severity,
}

impl Advisory {
    /// Does `version` fall within the affected range?
    pub fn affects(&self, version: &SemVer) -> bool {
        self.affected.matches(version) && self.fixed.as_ref().map(|f| version < f).unwrap_or(true)
    }
}

/// Parse advisories from the line-based format:
/// `id|package|affected_req|fixed_version_or_empty|title[|severity]`
/// The trailing `severity` field (low|medium|high|critical) is optional; a
/// missing or unknown severity is treated as `medium` (advisory, not fatal).
pub fn parse_advisory_db(text: &str) -> Vec<Advisory> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(6, '|').collect();
            if parts.len() < 5 {
                return None;
            }
            let id = parts[0].trim().to_string();
            let package = parts[1].trim().to_string();
            let affected = VersionReq::parse(parts[2].trim())?;
            let fixed = if parts[3].trim().is_empty() {
                None
            } else {
                SemVer::parse(parts[3].trim())
            };
            let title = parts[4].trim().to_string();
            let severity = parts
                .get(5)
                .map(|s| Severity::parse(s))
                .unwrap_or(Severity::Medium);
            Some(Advisory {
                id,
                package,
                affected,
                fixed,
                title,
                severity,
            })
        })
        .collect()
}

/// One advisory that matched a locked package, paired with its severity so the
/// caller can decide the exit code (`jet audit` exits nonzero on CRITICAL).
pub struct AuditMatch {
    pub severity: Severity,
    pub diagnostic: Diagnostic,
}

/// Check a set of locked packages against the advisory database.
/// Returns one match (severity + E2603) per advisory that applies.
pub fn audit_lockfile(lock: &LockFile, advisories: &[Advisory]) -> Vec<AuditMatch> {
    let mut matches = Vec::new();
    for pkg in &lock.packages {
        let ver = match SemVer::parse(&pkg.version) {
            Some(v) => v,
            None => continue,
        };
        for adv in advisories {
            if adv.package == pkg.name && adv.affects(&ver) {
                matches.push(AuditMatch {
                    severity: adv.severity,
                    diagnostic: e2603(
                        &adv.id,
                        &pkg.name,
                        &pkg.version,
                        &adv.title,
                        adv.severity,
                        adv.fixed.as_ref(),
                    ),
                });
            }
        }
    }
    matches
}

/// E2603 — advisory match.
pub fn e2603(
    id: &str,
    package: &str,
    version: &str,
    title: &str,
    severity: Severity,
    fixed: Option<&SemVer>,
) -> Diagnostic {
    let fix_msg = match fixed {
        Some(v) => format!("upgrade `{}` to >= {}. Run `jet audit --explain {}` for details.", package, v, id),
        None => format!("no fixed version is known; monitor `{}` for a patch. Run `jet audit --explain {}` for details.", package, id),
    };
    Diagnostic::error(
        "E2603",
        format!("[{}] advisory {} matches `{}` {}: {}", severity.label(), id, package, version, title),
        format!(
            "the advisory database flags `{}` {} as having a known vulnerability, exposed interface, or supply-chain risk.",
            package, version
        ),
        fix_msg,
        None,
    )
}

// ──────────────────────────────────────────────
// Integrity verification → E2604
// ──────────────────────────────────────────────

/// E2604 — integrity check failed.
pub fn e2604(package: &str, version: &str, expected: &str, actual: &str) -> Diagnostic {
    Diagnostic::error(
        "E2604",
        format!("integrity check failed for `{}` {}", package, version),
        format!(
            "expected hash {}, got {}. The artifact changed after it was locked — this may indicate accidental or deliberate tampering.",
            expected, actual
        ),
        format!(
            "re-run `jet fetch` after removing the corrupt store entry (`jet gc --force`). If the problem persists, the upstream source may have been altered; audit the change before proceeding."
        ),
        None,
    )
}

/// Verify a locked package's store entry against its recorded hash.
pub fn verify_package_integrity(pkg: &LockedPackage, store_entry: &Path) -> Result<(), Diagnostic> {
    use crate::SHA256::tree_hash;
    let actual = tree_hash(store_entry);
    if actual != pkg.fingerprint {
        return Err(e2604(&pkg.name, &pkg.version, &pkg.fingerprint, &actual));
    }
    Ok(())
}
