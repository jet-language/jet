//! D-LINTPOLICY1=A (the override law, ratified 2026-07-11, card #505).
//!
//! Warnings and lints never fail a build by default — errors stay reserved
//! for programs Jet cannot compile safely or unambiguously (I1 memory/type
//! safety is never in scope here; it has no override). A team opts into a
//! wall through the one policy surface: `policy: { lints: { deny: […] } }`
//! in `package.jet` (alongside `authority.trust` under the ratified authority
//! model). When a
//! denied lint fires, the build fails naming the exact lint and site instead
//! of only printing a warning — the bypass a solo run would have taken is
//! recorded as a build failure instead, never a silent downgrade.

use crate::Diagnostics::Diagnostic;
use crate::Package::PackageFacts;
use jet_foundation::LintPolicy as FoundationLintPolicy;

/// Whole-program enforcement: for every emitted lint whose name is listed in
/// `package.jet`'s `policy.lints.deny`, fail the build with E1293 instead of only
/// warning. `manifest.lints_deny` being `None` (no `policy.lints` block at
/// all) is the default — the returned list is always empty and every lint
/// stays a warning (I1/D-LINTPOLICY1: warn-never-block by default).
pub fn enforce(lints: &[Diagnostic], manifest: &PackageFacts) -> Vec<Diagnostic> {
    let deny = manifest.policy.lints_deny.as_deref().unwrap_or_default();
    FoundationLintPolicy::apply(deny, lints.to_vec())
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "E1293")
        .collect()
}

/// Return only findings that remain warnings under this package's policy.
/// Denied findings are rendered by `enforce` as E1293 instead; keeping them
/// out of the warning stream makes the policy wall a single, truthful report.
pub fn non_denied(lints: &[Diagnostic], manifest: &PackageFacts) -> Vec<Diagnostic> {
    let deny = manifest.policy.lints_deny.as_deref().unwrap_or_default();
    lints
        .iter()
        .filter(|lint| !FoundationLintPolicy::is_denied(deny, &lint.code))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Diagnostics::Span;
    use crate::Package;

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
        let manifest =
            Package::PackageFacts::parse(r#"name: "x"
version: "1"
"#, "test")
                .unwrap();
        let out = enforce(&[money_lint()], &manifest);
        assert!(out.is_empty(), "warn-never-block is the default (I1/D-LINTPOLICY1)");
    }

    #[test]
    fn denied_lint_becomes_e1293() {
        let manifest = Package::PackageFacts::parse(
            r#"
name: "x"
version: "1"
policy: .{ lints: .{ deny: [float_money] } }
"#,
            "test",
        )
        .unwrap();
        let out = enforce(&[money_lint()], &manifest);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].code, "E1293");
        assert!(out[0].what.contains("L0504"));
    }

    #[test]
    fn undenied_lint_stays_a_warning() {
        let manifest = Package::PackageFacts::parse(
            r#"
name: "x"
version: "1"
policy: .{ lints: .{ deny: [same_enum_guard_table] } }
"#,
            "test",
        )
        .unwrap();
        let out = enforce(&[money_lint()], &manifest);
        assert!(out.is_empty());
    }
}
