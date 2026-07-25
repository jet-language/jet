// Staged edition-deprecation helpers; Bundle uses `with_package_edition` today.
#![allow(dead_code)]

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;

pub(crate) fn with_package_edition<R>(edition: &str, f: impl FnOnce() -> R) -> R {
    jet_foundation::PackageEdition::with_package_edition(edition, f)
}

pub(crate) fn package_edition() -> String {
    jet_foundation::PackageEdition::package_edition()
}

pub(crate) fn edition_at_least(baseline: &str) -> bool {
    jet_foundation::PackageEdition::package_edition_at_least(baseline)
}

/// Mirrors `jet_pkg_model::Manifest::Deprecation` — keep in sync with `DEPRECATIONS` there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Deprecation {
    pub(crate) item: &'static str,
    pub(crate) since_edition: &'static str,
    pub(crate) replacement: &'static str,
    pub(crate) removed_in_edition: &'static str,
}

const DEPRECATIONS: &[Deprecation] = &[
    Deprecation {
        item: "cbor.encode",
        since_edition: "2027",
        replacement: "cbor.to_bytes",
        removed_in_edition: "2028",
    },
    Deprecation {
        item: "cbor.decode",
        since_edition: "2027",
        replacement: "cbor.parse",
        removed_in_edition: "2028",
    },
];

fn lookup_deprecation(item: &str) -> Option<&'static Deprecation> {
    DEPRECATIONS.iter().find(|dep| dep.item == item)
}

pub(crate) fn check_core_deprecation(module: &str, name: &str) -> Option<Deprecation> {
    let short = crate::Syntax::normalize_core_module(module)
        .and_then(|m| m.strip_prefix("core.encoding.").map(|rest| rest.to_string()))
        .unwrap_or_else(|| module.to_string());
    let item = format!("{short}.{name}");
    lookup_deprecation(&item).copied()
}

pub(crate) fn deprecation_phase(dep: &Deprecation) -> DeprecationPhase {
    let edition = package_edition();
    if jet_foundation::PackageEdition::edition_at_least(&edition, dep.removed_in_edition) {
        DeprecationPhase::Removed
    } else if jet_foundation::PackageEdition::edition_at_least(&edition, dep.since_edition) {
        DeprecationPhase::Deprecated
    } else {
        DeprecationPhase::Active
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeprecationPhase {
    Active,
    Deprecated,
    Removed,
}

pub(crate) fn e2002(dep: &Deprecation, span: Option<Span>) -> Diagnostic {
    Diagnostic::error(
        "E2002",
        format!(
            "`{}` was removed in edition {}",
            dep.item, dep.removed_in_edition
        ),
        format!(
            "`{}` was deprecated in edition {} and no longer exists in this edition; it has reached the end of its migration window.",
            dep.item, dep.since_edition,
        ),
        format!(
            "use `{}` instead, or run `{} fix` to migrate automatically.",
            dep.replacement,
            Syntax::BINARY_NAME,
        ),
        span,
    )
}

pub(crate) fn l2001(dep: &Deprecation, span: Option<Span>) -> Diagnostic {
    Diagnostic::lint(
        "L2001",
        format!("`{}` is deprecated", dep.item),
        format!(
            "`{}` was deprecated in edition {} and will be removed in edition {}; it still works for now but should be migrated.",
            dep.item, dep.since_edition, dep.removed_in_edition,
        ),
        format!(
            "use `{}` instead, or run `{} fix` to migrate automatically.",
            dep.replacement,
            Syntax::BINARY_NAME,
        ),
        span,
    )
}
