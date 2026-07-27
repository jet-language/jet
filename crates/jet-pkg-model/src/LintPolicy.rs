//! D-LINTPOLICY1=A (the override law, ratified 2026-07-11, card #505).
//!
//! Warnings and lints never fail a build by default — errors stay reserved
//! for programs Jet cannot compile safely or unambiguously (I1 memory/type
//! safety is never in scope here; it has no override). A team opts into a
//! wall through the one policy surface: `policy: { lints: { deny: […] } }`
//! in `pkg.jet` (joining `policy.trust` under D-JPK-POLICYSURFACE1). When a
//! denied lint fires, the build fails naming the exact lint and site instead
//! of only printing a warning — the bypass a solo run would have taken is
//! recorded as a build failure instead, never a silent downgrade.

use crate::Diagnostics::Diagnostic;
use crate::PackageManifest::PackManifest;

/// Whole-program enforcement: for every emitted lint whose code is listed in
/// `pkg.jet`'s `policy.lints.deny`, fail the build with E1293 instead of only
/// warning. `manifest.lints_deny` being `None` (no `policy.lints` block at
/// all) is the default — the returned list is always empty and every lint
/// stays a warning (I1/D-LINTPOLICY1: warn-never-block by default).
pub fn enforce(lints: &[Diagnostic], manifest: &PackManifest) -> Vec<Diagnostic> {
    let Some(deny) = manifest.lints_deny.as_ref() else {
        return Vec::new();
    };
    if deny.is_empty() {
        return Vec::new();
    }
    lints
        .iter()
        .filter(|d| deny.iter().any(|code| code == &d.code))
        .map(e1293)
        .collect()
}

/// E1293: a lint policy.lints denies fired. Carries the original lint's
/// site, `what`, and `why` forward so the build failure still teaches what
/// the code needs to fix — only the severity and the `policy.lints` framing
/// are new.
fn e1293(original: &Diagnostic) -> Diagnostic {
    Diagnostic::error(
        "E1293",
        format!(
            "lint `{}` is denied by policy: {}",
            original.code, original.what
        ),
        format!(
            "{} This team's `policy.lints.deny` in `pkg.jet` turns this warning into a build failure (D-LINTPOLICY1 — the override law); it stays a warning everywhere `pkg.jet` doesn't opt in.",
            original.why.trim_end_matches('.').to_string() + "."
        ),
        original.fix.clone(),
        original.span,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Diagnostics::Span;
    use crate::PackageManifest;

    fn money_lint() -> Diagnostic {
        Diagnostic::lint(
            "L0504",
            "field `price` looks like money but has type `Float`".to_string(),
            "floating-point money loses cents on common values like `0.1 + 0.2`".to_string(),
            "use `Decimal` for exact money, or suppress with `#[allow(float_money)]` on the field"
                .to_string(),
            Some(Span::new(0, 5)),
        )
    }

    #[test]
    fn no_policy_block_never_denies() {
        let manifest = PackageManifest::parse(
            r#"payload: { name: "x", version: "1" }"#,
        )
        .unwrap();
        let out = enforce(&[money_lint()], &manifest);
        assert!(out.is_empty(), "warn-never-block is the default (I1/D-LINTPOLICY1)");
    }

    #[test]
    fn denied_lint_becomes_e1293() {
        let manifest = PackageManifest::parse(
            r#"
payload: { name: "x", version: "1" }
policy: { lints: { deny: [L0504] } }
"#,
        )
        .unwrap();
        let out = enforce(&[money_lint()], &manifest);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].code, "E1293");
        assert!(out[0].what.contains("L0504"));
    }

    #[test]
    fn undenied_lint_stays_a_warning() {
        let manifest = PackageManifest::parse(
            r#"
payload: { name: "x", version: "1" }
policy: { lints: { deny: [L0504] } }
"#,
        )
        .unwrap();
        let out = enforce(&[money_lint()], &manifest);
        assert!(out.is_empty());
    }
}
