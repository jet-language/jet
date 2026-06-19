use crate::diag::Diagnostic;
use crate::lock::{LockFile, LockedPackage};
use std::path::Path;

use super::semver::{SemVer, VersionReq};

// ──────────────────────────────────────────────
// Advisory database
// ──────────────────────────────────────────────

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
}

impl Advisory {
    /// Does `version` fall within the affected range?
    pub fn affects(&self, version: &SemVer) -> bool {
        self.affected.matches(version)
            && self.fixed.as_ref().map(|f| version < f).unwrap_or(true)
    }
}

/// Parse advisories from the line-based format:
/// `id|package|affected_req|fixed_version_or_empty|title`
pub fn parse_advisory_db(text: &str) -> Vec<Advisory> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(5, '|').collect();
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
            Some(Advisory { id, package, affected, fixed, title })
        })
        .collect()
}

/// Check a set of locked packages against the advisory database.
/// Returns one E2603 per match.
pub fn audit_lockfile(
    lock: &LockFile,
    advisories: &[Advisory],
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for pkg in &lock.packages {
        let ver = match SemVer::parse(&pkg.version) {
            Some(v) => v,
            None => continue,
        };
        for adv in advisories {
            if adv.package == pkg.name && adv.affects(&ver) {
                diags.push(e2603(&adv.id, &pkg.name, &pkg.version, &adv.title, adv.fixed.as_ref()));
            }
        }
    }
    diags
}

/// E2603 — advisory match.
pub fn e2603(id: &str, package: &str, version: &str, title: &str, fixed: Option<&SemVer>) -> Diagnostic {
    let fix_msg = match fixed {
        Some(v) => format!("upgrade `{}` to >= {}. Run `jet audit --explain {}` for details.", package, v, id),
        None => format!("no fixed version is known; monitor `{}` for a patch. Run `jet audit --explain {}` for details.", package, id),
    };
    Diagnostic::error(
        "E2603",
        format!("advisory {} matches `{}` {}: {}", id, package, version, title),
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
pub fn verify_package_integrity(
    pkg: &LockedPackage,
    store_entry: &Path,
) -> Result<(), Diagnostic> {
    use crate::sha256::tree_hash;
    let actual = tree_hash(store_entry);
    if actual != pkg.fingerprint {
        return Err(e2604(&pkg.name, &pkg.version, &pkg.fingerprint, &actual));
    }
    Ok(())
}
