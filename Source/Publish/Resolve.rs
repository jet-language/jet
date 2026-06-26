use crate::Diagnostics::Diagnostic;
use std::collections::BTreeMap;

use super::SemVer::{SemVer, VersionReq};

// ──────────────────────────────────────────────
// Highest-compatible resolver (D-RESOLVE1=A)
// ──────────────────────────────────────────────

/// Select the highest version from `available` for `package` that satisfies
/// all `constraints`. Returns the winner, or E2602 if no candidate qualifies.
///
/// This implements the D-RESOLVE1=A policy: **highest-compatible** within the
/// intersection of every constraint's semver range, exactly as `npm` / `cargo`
/// do. When the available set is empty (no registry yet), the check reduces to
/// the syntactic intersection test (`intersects`).
pub fn select_highest_compatible<'a>(
    package: &str,
    constraints: &[&VersionConstraint],
    available: &'a [SemVer],
) -> Result<&'a SemVer, Diagnostic> {
    // Filter to candidates that satisfy every constraint.
    let winner = available
        .iter()
        .filter(|v| constraints.iter().all(|c| c.req.matches(v)))
        .max(); // SemVer: Ord derived via precedence

    if let Some(v) = winner {
        return Ok(v);
    }

    // Build an informative E2602: pick the first pair of constraints whose
    // ranges don't intersect, or fall back to the first two if all intersect
    // but no candidate exists.
    if constraints.len() >= 2 {
        // Try to find a genuinely contradictory pair first.
        for i in 0..constraints.len() {
            for j in (i + 1)..constraints.len() {
                if !constraints[i].req.intersects(&constraints[j].req) {
                    return Err(e2602(
                        package,
                        constraints[i].req.display(),
                        &constraints[i].from,
                        constraints[j].req.display(),
                        &constraints[j].from,
                    ));
                }
            }
        }
        // All intersect syntactically but no candidate available — report first two.
        return Err(e2602(
            package,
            constraints[0].req.display(),
            &constraints[0].from,
            constraints[1].req.display(),
            &constraints[1].from,
        ));
    }

    // Single constraint with no candidates (empty registry).
    Err(e2602(
        package,
        constraints.first().map(|c| c.req.display()).unwrap_or("*"),
        constraints.first().map(|c| c.from.as_str()).unwrap_or(""),
        "(no versions available)",
        "registry",
    ))
}

// ──────────────────────────────────────────────
// Registry resolver (PubGrub-style conflict detection)
// ──────────────────────────────────────────────

/// A single version constraint from one dependent.
#[derive(Debug, Clone)]
pub struct VersionConstraint {
    pub package: String,
    pub req: VersionReq,
    /// Where this constraint comes from (package name and version).
    pub from: String,
}

/// E2602 — resolver cannot satisfy two conflicting constraints.
pub fn e2602(
    package: &str,
    req_a: &str,
    from_a: &str,
    req_b: &str,
    from_b: &str,
) -> Diagnostic {
    Diagnostic::error(
        "E2602",
        format!("dependency resolver conflict: `{}` has incompatible version requirements", package),
        format!(
            "`{}` requires `{}` from `{}`, but `{}` from `{}`; no version satisfies both.",
            package, req_a, from_a, req_b, from_b,
        ),
        format!(
            "upgrade or downgrade one of the conflicting dependents so their `{}` constraints overlap, or ask the authors to release a compatible version.",
            package,
        ),
        None,
    )
}

/// A simplified resolver: given a set of constraints for each package name,
/// check whether any package has two mutually-incompatible constraints
/// (i.e. no candidate version in the registry satisfies all of them).
/// In v1 without a live registry, we detect the syntactic contradiction
/// (e.g. `^1.0` vs `^2.0`).
pub fn check_conflicts(
    constraints: &[VersionConstraint],
    available: &BTreeMap<String, Vec<SemVer>>,
) -> Vec<Diagnostic> {
    // Group constraints by package.
    let mut by_pkg: BTreeMap<&str, Vec<&VersionConstraint>> = BTreeMap::new();
    for c in constraints {
        by_pkg.entry(c.package.as_str()).or_default().push(c);
    }

    let mut diags = Vec::new();
    for (pkg, reqs) in &by_pkg {
        if reqs.len() < 2 {
            continue;
        }
        // Find any version that satisfies ALL constraints.
        let candidates = available.get(*pkg).map(|v| v.as_slice()).unwrap_or(&[]);
        let any_ok = candidates
            .iter()
            .any(|v| reqs.iter().all(|r| r.req.matches(v)));
        if !any_ok && !candidates.is_empty() {
            // Report first two conflicting.
            let a = reqs[0];
            let b = reqs.iter().skip(1).find(|r| {
                !candidates
                    .iter()
                    .any(|v| r.req.matches(v) && a.req.matches(v))
            });
            if let Some(b) = b {
                diags.push(e2602(pkg, a.req.display(), &a.from, b.req.display(), &b.from));
            }
        }
        // When no candidates at all: surface if two constraints' ranges exclude each other.
        if candidates.is_empty() && reqs.len() >= 2 {
            for i in 0..reqs.len() {
                for j in (i + 1)..reqs.len() {
                    if !reqs[i].req.intersects(&reqs[j].req) {
                        diags.push(e2602(
                            pkg,
                            reqs[i].req.display(),
                            &reqs[i].from,
                            reqs[j].req.display(),
                            &reqs[j].from,
                        ));
                    }
                }
            }
        }
    }
    diags
}
