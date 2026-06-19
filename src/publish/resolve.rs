use crate::diag::Diagnostic;
use std::collections::BTreeMap;

use super::semver::{SemVer, VersionReq};

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
                let req_a_str = format!("{:?}", a.req).replace("VersionReq::", "");
                let req_b_str = format!("{:?}", b.req).replace("VersionReq::", "");
                diags.push(e2602(pkg, &req_a_str, &a.from, &req_b_str, &b.from));
            }
        }
        // When no candidates at all: surface if two constraints' ranges exclude each other.
        if candidates.is_empty() && reqs.len() >= 2 {
            for i in 0..reqs.len() {
                for j in (i + 1)..reqs.len() {
                    if ranges_disjoint(&reqs[i].req, &reqs[j].req) {
                        let req_a_str = req_display(&reqs[i].req);
                        let req_b_str = req_display(&reqs[j].req);
                        diags.push(e2602(pkg, &req_a_str, &reqs[i].from, &req_b_str, &reqs[j].from));
                    }
                }
            }
        }
    }
    diags
}

fn req_display(r: &VersionReq) -> String {
    match r {
        VersionReq::Any => "*".into(),
        VersionReq::Exact(v) => v.to_string(),
        VersionReq::Caret { floor, .. } => format!("^{}", floor),
    }
}

fn ranges_disjoint(a: &VersionReq, b: &VersionReq) -> bool {
    match (a, b) {
        (VersionReq::Caret { floor: fa, precision: pa }, VersionReq::Caret { floor: fb, precision: pb }) => {
            // ^1.x and ^2.x are disjoint (different majors, or major=0 different minors).
            if fa.major == 0 && fb.major == 0 && *pa >= 2 && *pb >= 2 {
                fa.minor != fb.minor
            } else {
                fa.major != fb.major
            }
        }
        (VersionReq::Exact(va), VersionReq::Exact(vb)) => va != vb,
        (VersionReq::Exact(v), VersionReq::Caret { floor, precision }) => {
            !VersionReq::Caret { floor: floor.clone(), precision: *precision }.matches(v)
        }
        (VersionReq::Caret { floor, precision }, VersionReq::Exact(v)) => {
            !VersionReq::Caret { floor: floor.clone(), precision: *precision }.matches(v)
        }
        _ => false,
    }
}
