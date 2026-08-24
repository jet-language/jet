// D-REL3 / D-STRUCT-LIFE1=A: package-edition state and the one lifecycle
// diagnostic path. The payload comes from a user marker or a synthetic Core
// marker application; this module does not own a deprecation registry.
#![allow(dead_code)]

use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Syntax;
use crate::AST::Deprecation;

pub(crate) fn with_package_edition<R>(edition: &str, f: impl FnOnce() -> R) -> R {
    jet_foundation::PackageEdition::with_package_edition(edition, f)
}

pub(crate) fn package_edition() -> String {
    jet_foundation::PackageEdition::package_edition()
}

pub(crate) fn edition_at_least(baseline: &str) -> bool {
    jet_foundation::PackageEdition::package_edition_at_least(baseline)
}

fn is_edition_label(label: &str) -> bool {
    let trimmed = label.trim();
    trimmed.len() == 4 && trimmed.bytes().all(|byte| byte.is_ascii_digit())
}

fn version_phrase(label: &str) -> String {
    if is_edition_label(label) {
        format!("edition {label}")
    } else {
        label.to_string()
    }
}

/// `jet fix` owns the mechanical part of a lifecycle migration: replace the
/// used member with the final path's leaf. The diagnostic span is the call
/// member/name, so replacing `encode` with `to_bytes` keeps `cbor.to_bytes`
/// well-formed while a plain `use: "parse"` remains a plain rename.
fn replacement_edit(dep: &Deprecation, span: Option<Span>) -> Option<TextEdit> {
    let span = span?;
    let replacement = dep.replacement.rsplit('.').next()?.trim();
    if replacement.is_empty()
        || !replacement.bytes().enumerate().all(|(index, byte)| {
            byte == b'_'
                || (byte.is_ascii_alphanumeric()
                    && (index > 0 || byte.is_ascii_alphabetic() || byte == b'_'))
        })
    {
        return None;
    }
    Some(TextEdit {
        span,
        new_text: replacement.to_string(),
    })
}

pub(crate) fn deprecation_phase(dep: &Deprecation) -> DeprecationPhase {
    let edition = package_edition();
    if dep.removed_in.as_deref().is_some_and(|removed| {
        is_edition_label(removed)
            && jet_foundation::PackageEdition::edition_at_least(&edition, removed)
    }) {
        return DeprecationPhase::Removed;
    }
    if is_edition_label(&dep.since)
        && !jet_foundation::PackageEdition::edition_at_least(&edition, &dep.since)
    {
        DeprecationPhase::Active
    } else {
        DeprecationPhase::Deprecated
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeprecationPhase {
    Active,
    Deprecated,
    Removed,
}

pub(crate) fn e2002(item: &str, dep: &Deprecation, span: Option<Span>) -> Diagnostic {
    let removed_in = dep.removed_in.as_deref().unwrap_or("this edition");
    let diagnostic = Diagnostic::error(
        "E2002",
        format!("`{item}` was removed in {}", version_phrase(removed_in)),
        format!(
            "`{item}` was deprecated in {} and no longer exists in this edition; it has reached the end of its migration window.",
            version_phrase(&dep.since),
        ),
        format!(
            "use `{}` instead, or run `{} fix` to migrate automatically.",
            dep.replacement,
            Syntax::BINARY_NAME,
        ),
        span,
    );
    replacement_edit(dep, span)
        .map(|edit| diagnostic.clone().with_edit(edit))
        .unwrap_or(diagnostic)
}

pub(crate) fn l2001(item: &str, dep: &Deprecation, span: Option<Span>) -> Diagnostic {
    let why = match dep.removed_in.as_deref() {
        Some(removed_in) => format!(
            "`{item}` was deprecated in {} and will be removed in {}; it still works for now but should be migrated.",
            version_phrase(&dep.since),
            version_phrase(removed_in),
        ),
        None => format!(
            "`{item}` was deprecated in {}; it still works for now but should be migrated.",
            version_phrase(&dep.since),
        ),
    };
    let diagnostic = Diagnostic::lint(
        "L2001",
        format!("`{item}` is deprecated"),
        why,
        format!(
            "use `{}` instead, or run `{} fix` to migrate automatically.",
            dep.replacement,
            Syntax::BINARY_NAME,
        ),
        span,
    );
    replacement_edit(dep, span)
        .map(|edit| diagnostic.clone().with_edit(edit))
        .unwrap_or(diagnostic)
}

impl<'a> super::Checker<'a> {
    /// Emit the lifecycle result for one resolved item. Every user and Core
    /// call site enters here, so severity and copy cannot drift by caller.
    pub(crate) fn check_deprecation(&mut self, item: &str, dep: &Deprecation, span: Span) {
        let phase = deprecation_phase(dep);
        let status = match phase {
            DeprecationPhase::Active => "active",
            DeprecationPhase::Deprecated => "deprecated",
            DeprecationPhase::Removed => "removed",
        };
        let detail = match dep.removed_in.as_deref() {
            Some(removed_in) => format!(
                "since {}; use `{}`; removed in {}",
                dep.since, dep.replacement, removed_in
            ),
            None => format!("since {}; use `{}`", dep.since, dep.replacement),
        };
        self.name_ledger
            .record_structure_fact(jet_foundation::Names::StructureFact::new(
                jet_foundation::Names::StructureFactKind::Lifecycle,
                item,
                self.module_path,
                span,
                status,
                detail,
                None,
            ));
        match phase {
            DeprecationPhase::Removed => self.diags.push(e2002(item, dep, Some(span))),
            DeprecationPhase::Deprecated => self.diags.push(l2001(item, dep, Some(span))),
            DeprecationPhase::Active => {}
        }
    }
}
