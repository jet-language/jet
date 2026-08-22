use crate::Diagnostics::Diagnostic;
use std::collections::BTreeMap;

use super::SemVer::{SemVer, VersionReq};

/// The one resolution vocabulary shared by registry selection and update
/// proofs. Conservative preserves an existing lock when possible; the other
/// modes choose a compatible live candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveMode {
    Conservative,
    Latest,
    Lowest,
    LowestDirect,
}

impl Default for ResolveMode {
    fn default() -> Self {
        Self::Conservative
    }
}

impl ResolveMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Conservative => "conservative",
            Self::Latest => "latest",
            Self::Lowest => "lowest",
            Self::LowestDirect => "lowest-direct",
        }
    }
}

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
                    return Err(proof_diagnostic_refs(
                        package,
                        constraints,
                        "two requirements have no overlapping version",
                    ));
                }
            }
        }
        // All intersect syntactically but no candidate available — report first two.
        return Err(proof_diagnostic_refs(
            package,
            constraints,
            "the registry has no candidate in the overlapping range",
        ));
    }

    // Single constraint with no candidates (empty registry).
    Err(proof_diagnostic_refs(
        package,
        constraints,
        "the registry has no published candidate",
    ))
}

/// Select a compatible version under the ratified resolution vocabulary.
/// `Conservative` is handled by the caller's exact-lock check and falls back
/// to the latest live candidate when no lock exists.
pub fn select_compatible<'a>(
    package: &str,
    constraints: &[&VersionConstraint],
    available: &'a [SemVer],
    mode: ResolveMode,
) -> Result<&'a SemVer, Diagnostic> {
    if matches!(mode, ResolveMode::Conservative | ResolveMode::Latest) {
        return select_highest_compatible(package, constraints, available);
    }
    let winner = available
        .iter()
        .filter(|version| constraints.iter().all(|constraint| constraint.req.matches(version)))
        .min();
    winner.ok_or_else(|| {
        select_highest_compatible(package, constraints, available)
            .expect_err("no compatible candidate should produce a diagnostic")
    })
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

/// Candidate data passed from the registry loader to the graph solver. The
/// loader owns artifacts; the solver only owns versions and dependency edges.
#[derive(Debug, Clone)]
pub(crate) struct SolverCandidate {
    pub(crate) version: SemVer,
    pub(crate) dependencies: Vec<VersionConstraint>,
}

#[derive(Debug, Clone, Default)]
struct SolverState {
    constraints: BTreeMap<String, Vec<VersionConstraint>>,
    assignments: BTreeMap<String, SemVer>,
}

/// Resolve one registry graph with PubGrub-style incompatibility backtracking.
///
/// This is deliberately small: registry metadata already gives us concrete
/// candidates, so there is no need for a second package model or a dependency
/// solver crate. The returned assignments are stable for a given candidate
/// set, and failed branches retain their causal proof in E2602 detail.
pub(crate) fn solve_registry(
    roots: &[VersionConstraint],
    candidates: &BTreeMap<String, Vec<SolverCandidate>>,
    locked: &BTreeMap<String, String>,
    update_scope: &std::collections::BTreeSet<String>,
    mode: ResolveMode,
    direct: &std::collections::BTreeSet<String>,
) -> Result<BTreeMap<String, SemVer>, Diagnostic> {
    let mut state = SolverState::default();
    for root in roots {
        state
            .constraints
            .entry(root.package.clone())
            .or_default()
            .push(root.clone());
    }
    solve_registry_state(&state, candidates, locked, update_scope, mode, direct)
}

fn solve_registry_state(
    state: &SolverState,
    candidates: &BTreeMap<String, Vec<SolverCandidate>>,
    locked: &BTreeMap<String, String>,
    update_scope: &std::collections::BTreeSet<String>,
    mode: ResolveMode,
    direct: &std::collections::BTreeSet<String>,
) -> Result<BTreeMap<String, SemVer>, Diagnostic> {
    for (package, version) in &state.assignments {
        let requirements = state
            .constraints
            .get(package)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if !requirements
            .iter()
            .all(|constraint| constraint.req.matches(version))
        {
            return Err(proof_diagnostic(
                package,
                requirements,
                "the selected version is incompatible with a later dependency",
            ));
        }
    }

    let Some(package) = state
        .constraints
        .keys()
        .find(|package| !state.assignments.contains_key(*package))
        .cloned()
    else {
        return Ok(state.assignments.clone());
    };

    let requirements = state
        .constraints
        .get(&package)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut viable = candidates
        .get(&package)
        .into_iter()
        .flatten()
        .filter(|candidate| {
            requirements
                .iter()
                .all(|constraint| constraint.req.matches(&candidate.version))
        })
        .cloned()
        .collect::<Vec<_>>();
    // A selective update may move only the requested closure. An existing
    // lock outside that closure is therefore an exact constraint, not merely
    // a preference. Otherwise the solver can choose a different version and
    // the later lock-preservation pass can hide that mismatch.
    if matches!(mode, ResolveMode::Conservative)
        && !update_scope.is_empty()
        && !update_scope.contains(&package)
    {
        if let Some(locked_version) = locked.get(&package) {
            viable.retain(|candidate| candidate.version.to_string() == *locked_version);
        }
    }
    let low_first = matches!(mode, ResolveMode::Lowest)
        || matches!(mode, ResolveMode::LowestDirect) && direct.contains(&package);
    let preserve_lock = matches!(mode, ResolveMode::Conservative);
    viable.sort_by(|left, right| {
        let left_locked = locked.get(&package).is_some_and(|version| {
            preserve_lock
                && !update_scope.contains(&package)
                && version == &left.version.to_string()
        });
        let right_locked = locked.get(&package).is_some_and(|version| {
            preserve_lock
                && !update_scope.contains(&package)
                && version == &right.version.to_string()
        });
        right_locked
            .cmp(&left_locked)
            .then_with(|| {
                if low_first {
                    left.version.cmp(&right.version)
                } else {
                    right.version.cmp(&left.version)
                }
            })
            .then_with(|| left.version.to_string().cmp(&right.version.to_string()))
    });

    if viable.is_empty() {
        return Err(proof_diagnostic(
            &package,
            requirements,
            "no published candidate satisfies every requirement",
        ));
    }

    let mut branch_details = Vec::new();
    for candidate in viable {
        let mut next = state.clone();
        next.assignments
            .insert(package.clone(), candidate.version.clone());
        for dependency in candidate.dependencies {
            next.constraints
                .entry(dependency.package.clone())
                .or_default()
                .push(dependency);
        }
        match solve_registry_state(&next, candidates, locked, update_scope, mode, direct) {
            Ok(assignments) => return Ok(assignments),
            Err(error) => {
                let detail = error.detail.clone().unwrap_or_else(|| error.why.clone());
                branch_details.push(format!(
                    "  tried `{}` {}:\n{}",
                    package,
                    candidate.version,
                    indent_proof(&detail)
                ));
            }
        }
    }

    let mut diagnostic = proof_diagnostic(
        &package,
        requirements,
        "every candidate led to an incompatible dependency",
    );
    if !branch_details.is_empty() {
        let mut detail = diagnostic.detail.take().unwrap_or_default();
        if !detail.is_empty() {
            detail.push('\n');
        }
        detail.push_str("PubGrub branches:\n");
        detail.push_str(&branch_details.join("\n"));
        diagnostic.detail = Some(detail);
    }
    Err(diagnostic)
}

fn proof_diagnostic(
    package: &str,
    requirements: &[VersionConstraint],
    reason: &str,
) -> Diagnostic {
    let (first, second) = conflicting_pair(requirements);
    let mut diagnostic = match (first, second) {
        (Some(a), Some(b)) => e2602(
            package,
            a.req.display(),
            &a.from,
            b.req.display(),
            &b.from,
        ),
        (Some(a), None) => e2602(
            package,
            a.req.display(),
            &a.from,
            "(no compatible candidate)",
            "registry",
        ),
        (None, _) => e2602(
            package,
            "*",
            "root",
            "(no compatible candidate)",
            "registry",
        ),
    };
    let mut detail = format!("PubGrub proof tree:\n- {package}: {reason}");
    for constraint in requirements {
        detail.push_str(&format!(
            "\n  - `{}` requires `{}`",
            constraint.from,
            constraint.req.display()
        ));
    }
    if let (Some(a), Some(b)) = (first, second) {
        detail.push_str(&format!(
            "\nSmallest fixes: change `{}` or `{}` so their `{package}` requirements overlap.",
            a.from, b.from
        ));
    }
    diagnostic.detail = Some(detail);
    diagnostic
}

fn proof_diagnostic_refs(
    package: &str,
    requirements: &[&VersionConstraint],
    reason: &str,
) -> Diagnostic {
    let requirements = requirements
        .iter()
        .map(|constraint| (*constraint).clone())
        .collect::<Vec<_>>();
    proof_diagnostic(package, &requirements, reason)
}

fn conflicting_pair<'a>(
    requirements: &'a [VersionConstraint],
) -> (Option<&'a VersionConstraint>, Option<&'a VersionConstraint>) {
    for (index, left) in requirements.iter().enumerate() {
        if let Some(right) = requirements[index + 1..]
            .iter()
            .find(|right| !left.req.intersects(&right.req))
        {
            return (Some(left), Some(right));
        }
    }
    (requirements.first(), requirements.get(1))
}

fn indent_proof(detail: &str) -> String {
    detail
        .lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// E2602 — resolver cannot satisfy two conflicting constraints.
pub fn e2602(package: &str, req_a: &str, from_a: &str, req_b: &str, from_b: &str) -> Diagnostic {
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
                diags.push(proof_diagnostic_refs(
                    pkg,
                    &[a, b],
                    "available versions cannot satisfy both requirements",
                ));
            }
        }
        // When no candidates at all: surface if two constraints' ranges exclude each other.
        if candidates.is_empty() && reqs.len() >= 2 {
            for i in 0..reqs.len() {
                for j in (i + 1)..reqs.len() {
                    if !reqs[i].req.intersects(&reqs[j].req) {
                        diags.push(proof_diagnostic_refs(
                            pkg,
                            &reqs,
                            "two requirements have no overlapping version",
                        ));
                    }
                }
            }
        }
    }
    diags
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn version(value: &str) -> SemVer {
        SemVer::parse(value).expect("test version is valid")
    }

    #[test]
    fn registry_solver_backtracks_and_keeps_a_causal_proof() {
        let roots = vec![VersionConstraint {
            package: "app".to_string(),
            req: VersionReq::parse("^1.0").expect("test requirement is valid"),
            from: "root".to_string(),
        }];
        let mut candidates = BTreeMap::new();
        candidates.insert(
            "app".to_string(),
            vec![
                SolverCandidate {
                    version: version("1.1.0"),
                    dependencies: vec![VersionConstraint {
                        package: "shared".to_string(),
                        req: VersionReq::parse("^2.0").expect("test requirement is valid"),
                        from: "app 1.1.0".to_string(),
                    }],
                },
                SolverCandidate {
                    version: version("1.0.0"),
                    dependencies: vec![VersionConstraint {
                        package: "shared".to_string(),
                        req: VersionReq::parse("^1.0").expect("test requirement is valid"),
                        from: "app 1.0.0".to_string(),
                    }],
                },
            ],
        );
        candidates.insert(
            "shared".to_string(),
            vec![SolverCandidate {
                version: version("1.0.0"),
                dependencies: Vec::new(),
            }],
        );

        let selected = solve_registry(
            &roots,
            &candidates,
            &BTreeMap::new(),
            &BTreeSet::new(),
            ResolveMode::Latest,
            &BTreeSet::from(["app".to_string()]),
        )
        .expect("the lower app candidate should satisfy the graph");
        assert_eq!(selected["app"], version("1.0.0"));
        assert_eq!(selected["shared"], version("1.0.0"));

        let mut impossible = candidates;
        impossible.insert(
            "shared".to_string(),
            vec![SolverCandidate {
                version: version("3.0.0"),
                dependencies: Vec::new(),
            }],
        );
        let error = solve_registry(
            &roots,
            &impossible,
            &BTreeMap::new(),
            &BTreeSet::new(),
            ResolveMode::Latest,
            &BTreeSet::from(["app".to_string()]),
        )
        .expect_err("the graph has no compatible shared version");
        let detail = error.detail.expect("conflict must carry its proof tree");
        assert!(detail.contains("PubGrub proof tree:"));
        assert!(detail.contains("PubGrub branches:"));
        assert!(detail.contains("app 1.1.0"));
        assert!(detail.contains("Smallest fixes:"));
    }

    #[test]
    fn selective_update_treats_unrelated_lock_as_exact() {
        let roots = vec![
            VersionConstraint {
                package: "app".to_string(),
                req: VersionReq::parse("*").expect("test requirement is valid"),
                from: "root".to_string(),
            },
            VersionConstraint {
                package: "shared".to_string(),
                req: VersionReq::parse("*").expect("test requirement is valid"),
                from: "root".to_string(),
            },
        ];
        let candidates = BTreeMap::from([
            (
                "app".to_string(),
                vec![SolverCandidate {
                    version: version("1.0.0"),
                    dependencies: Vec::new(),
                }],
            ),
            (
                "shared".to_string(),
                vec![SolverCandidate {
                    version: version("2.0.0"),
                    dependencies: Vec::new(),
                }],
            ),
        ]);
        let locked = BTreeMap::from([("shared".to_string(), "1.0.0".to_string())]);
        let update_scope = BTreeSet::from(["app".to_string()]);
        let direct = BTreeSet::from(["app".to_string(), "shared".to_string()]);

        let error = solve_registry(
            &roots,
            &candidates,
            &locked,
            &update_scope,
            ResolveMode::Conservative,
            &direct,
        )
        .expect_err("an unrelated locked package must not move");
        assert_eq!(error.code, "E2602");
        assert!(error.detail.unwrap_or_default().contains("shared"));
    }
}
