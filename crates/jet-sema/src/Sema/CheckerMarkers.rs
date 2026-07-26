//! E0927 (card #518): the closed marker vocabulary.
//!
//! `#Name` rules are structurally accepted by the parser for any PascalCase
//! identifier (including the `#[…]` bracket-list path) — the parser
//! only knows "this looks like a marker," not "this is a marker Jet knows
//! about." An unregistered name used to silently do nothing (I3: codegen
//! never saw it, so nothing rejected it either). This module is the one
//! place that closes the vocabulary: every marker name is checked against
//! the registered applied-rule vocabulary plus any `derive T.Name { … }` provider
//! visible in this build (D-METADERIVE1=A user derives are a legal, dynamic
//! addition to the contract vocabulary, not typos).
//!
//! A retired `Debug` registry row is deliberately not flagged here: E0922
//! (`crates/jet-foundation/src/Traits.rs`) already owns that retired name
//! end to end, with its own text. Duplicating it here would double-report.

use crate::AST::{Item, Marker};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use std::collections::HashSet;

/// E0927: `name` isn't a registered applied rule. `vocab` supplies nearest
/// spelling suggestions.
fn e0927_unknown_marker(name: &str, vocab: &[String], span: Span) -> Diagnostic {
    if let Some(crate::Policy::AppliedRule {
        status: crate::Policy::RuleStatus::Retired { replacement },
        ..
    }) = crate::Policy::applied_rule(name)
    {
        let fix = if replacement.starts_with('#') || replacement.starts_with('.') {
            format!("write `{replacement}` instead")
        } else {
            replacement.to_string()
        };
        return Diagnostic::error(
            "E0927",
            format!("`#{name}` is retired"),
            format!(
                "the registry keeps this old spelling only to teach its replacement; \
                 it no longer applies a rule"
            ),
            fix,
            Some(span),
        );
    }
    let fix = match crate::Sema::Diagnostics::suggest_field(name, vocab) {
        Some(s) => format!("did you mean `#{s}`?"),
        None => format!(
            "check the spelling, or see docs/spec/syntax-decisions.md for the full applied-rule list."
        ),
    };
    Diagnostic::error(
        "E0927",
        format!("`#{name}` isn't a known applied rule"),
        format!("`{name}` isn't registered as an applied rule — Jet rules are a closed, \
                 registered vocabulary (I7), not any PascalCase word."),
        fix,
        Some(span),
    )
}

/// True when `name` is a built-in rule or visible user derive.
fn is_legal_rule_name(name: &str, known_derive_names: &HashSet<String>) -> bool {
    Syntax::is_applied_rule(name) || known_derive_names.contains(name)
}

/// Check one marker against its sigil's plane. Returns `None` when it's
/// legal, or already reported elsewhere:
/// - a name known on the OTHER plane already got E0062/E0063 from the
///   parser's shared marker reader — never double-report.
/// - `#Debug` is E0922's job (see module docs).
fn check_one(m: &Marker, known_derive_names: &HashSet<String>) -> Option<Diagnostic> {
    let e0922_owns_debug = crate::Policy::applied_rule(&m.name).is_some_and(|row| {
        row.name == "Debug"
            && matches!(row.status, crate::Policy::RuleStatus::Retired { .. })
    });
    if e0922_owns_debug || is_legal_rule_name(&m.name, known_derive_names) {
        return None;
    }
    let vocab: Vec<String> = crate::Policy::APPLIED_RULES
        .iter()
        .filter(|row| matches!(row.status, crate::Policy::RuleStatus::Active))
        .map(|row| row.name.to_string())
        .chain(known_derive_names.iter().cloned())
        .collect();
    Some(e0927_unknown_marker(&m.name, &vocab, m.name_span))
}

/// D-MARK-VOCAB1 (card #518): validate every marker name on `items` against
/// its plane's registered vocabulary (E0927). Covers type-level markers
/// (`s.type_markers`/`e.type_markers`, the full pre-classification list —
/// `Syntax.rs` module docs — so plane info from `Marker.sigil` survives)
/// and field/variant-level bracket markers (`f.serde_markers`,
/// `v.serde_markers`, which keep their `Marker`s whole; only `#Redact` is
/// pulled out into `f.redact` upstream). `known_derive_names` is the set of
/// `derive T.Name { … }` providers visible to this build (bundle-wide in
/// `Bundle.rs`, so a cross-module user derive is never a false unknown).
pub(crate) fn check_marker_vocabulary(items: &[Item], known_derive_names: &HashSet<String>) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for item in items {
        match item {
            Item::Struct(s) => {
                for m in &s.type_markers {
                    if let Some(d) = check_one(m, known_derive_names) {
                        out.push(d);
                    }
                }
                for f in &s.fields {
                    for m in &f.serde_markers {
                        if let Some(d) = check_one(m, known_derive_names) {
                            out.push(d);
                        }
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.type_markers {
                    if let Some(d) = check_one(m, known_derive_names) {
                        out.push(d);
                    }
                }
                for v in &e.variants {
                    for m in &v.serde_markers {
                        if let Some(d) = check_one(m, known_derive_names) {
                            out.push(d);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}
