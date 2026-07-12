//! E0927 (card #518): the closed marker vocabulary.
//!
//! `#Name`/`@Name` markers are structurally accepted by the parser for any
//! PascalCase identifier (`at_single_type_marker`/`at_single_contract_type_marker`
//! in `jet-parser`, plus the `#[…]`/`@[…]` bracket-list paths) — the parser
//! only knows "this looks like a marker," not "this is a marker Jet knows
//! about." An unregistered name used to silently do nothing (I3: codegen
//! never saw it, so nothing rejected it either). This module is the one
//! place that closes the vocabulary: every marker name is checked against
//! the registered `@` (contract) or `#` (directive) plane — `Marker.sigil`
//! says which — plus, on the `@` plane, any `derive T.Name { … }` provider
//! visible in this build (D-METADERIVE1=A user derives are a legal, dynamic
//! addition to the contract vocabulary, not typos).
//!
//! `Debug` is deliberately never flagged here on the `@` plane: E0922
//! (`crates/jet-foundation/src/Traits.rs`) already owns that retired name
//! end to end, with its own text. Duplicating it here would double-report.

use crate::AST::{Item, Marker};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use std::collections::HashSet;

/// D-MARK-DEBUG1=A: owned by E0922 already; never re-flag it here.
const DEBUG_OWNED_ELSEWHERE: &str = "Debug";

/// Retired marker spellings that get a targeted fix instead of a bare
/// nearest-match guess — each one used to mean something and now means
/// nothing, so "did you mean" would be misleading.
fn retired_marker_fix(name: &str) -> Option<&'static str> {
    match name {
        "Wasm" => Some(
            "write `#Target(Wasm)` instead — one target-marker family covers every backend \
             (D-MARK-TARGET1=A).",
        ),
        "Js" => Some(
            "write `#Target(Js)` instead — one target-marker family covers every backend \
             (D-MARK-TARGET1=A).",
        ),
        "Suppress" => Some(
            "call `.drop(\"reason\")` on the unused value instead — it's the one discard \
             spelling (D-MARK-DISCARD1=A).",
        ),
        "Uninit" => Some(
            "give the field a real initial value — stored uninitialized-sentinel fields were \
             retired outright (D-UNINIT-SENTINEL1).",
        ),
        "Ref" => Some(
            "hold an owned value instead — stored-reference fields were deleted outright \
             (D-MEM1/S3).",
        ),
        _ => None,
    }
}

/// E0927: `name` (written with `sigil`) isn't a registered marker on its
/// plane. `plane` is `"contract (@)"` or `"directive (#)"`, for the message.
/// `vocab` is the candidate list this plane accepts, for the "did you mean"
/// suggestion.
fn e0927_unknown_marker(sigil: char, name: &str, plane: &str, vocab: &[String], span: Span) -> Diagnostic {
    if let Some(fix) = retired_marker_fix(name) {
        return Diagnostic::error(
            "E0927",
            format!("`{sigil}{name}` is retired — it no longer does anything"),
            format!(
                "`{sigil}{name}` used to be a real marker; it was removed and nothing takes \
                 its place under that name, so writing it here silently did nothing before \
                 this check existed."
            ),
            fix.to_string(),
            Some(span),
        );
    }
    let fix = match crate::Sema::Diagnostics::suggest_field(name, vocab) {
        Some(s) => format!("did you mean `{sigil}{s}`?"),
        None => format!(
            "check the spelling, or see docs/spec/syntax-decisions.md for the full {plane} \
             marker list."
        ),
    };
    Diagnostic::error(
        "E0927",
        format!("`{sigil}{name}` isn't a known marker"),
        format!("`{name}` isn't registered on the {plane} plane — Jet markers are a closed, \
                 registered vocabulary (I7), not any PascalCase word."),
        fix,
        Some(span),
    )
}

/// True when `name` is legal on the `@` (contract) plane: a built-in
/// contract marker, or a `derive T.name { … }` provider visible in this
/// build.
fn is_legal_contract_name(name: &str, known_derive_names: &HashSet<String>) -> bool {
    Syntax::is_contract_marker(name) || known_derive_names.contains(name)
}

/// Check one marker against its sigil's plane. Returns `None` when it's
/// legal, or already reported elsewhere:
/// - a name known on the OTHER plane already got E0062/E0063 from the
///   parser (`check_marker_plane` in `jet-parser`) — never double-report.
/// - `@Debug` is E0922's job (see module docs).
fn check_one(m: &Marker, known_derive_names: &HashSet<String>) -> Option<Diagnostic> {
    match m.sigil {
        '@' => {
            if m.name == DEBUG_OWNED_ELSEWHERE {
                return None;
            }
            if is_legal_contract_name(&m.name, known_derive_names) {
                return None;
            }
            if Syntax::is_directive_marker(&m.name) {
                // Already E0063 ("write it with #, not @") from the parser.
                return None;
            }
            let vocab: Vec<String> = Syntax::CONTRACT_MARKERS
                .iter()
                .map(|s| s.to_string())
                .chain(known_derive_names.iter().cloned())
                .collect();
            Some(e0927_unknown_marker('@', &m.name, "contract (@)", &vocab, m.name_span))
        }
        '#' => {
            if Syntax::is_directive_marker(&m.name) {
                return None;
            }
            if Syntax::is_contract_marker(&m.name) {
                // Already E0062 ("write it with @, not #") from the parser.
                return None;
            }
            let vocab: Vec<String> = Syntax::DIRECTIVE_MARKERS.iter().map(|s| s.to_string()).collect();
            Some(e0927_unknown_marker('#', &m.name, "directive (#)", &vocab, m.name_span))
        }
        _ => None,
    }
}

/// D-MARK-VOCAB1 (card #518): validate every marker name on `items` against
/// its plane's registered vocabulary (E0927). Covers type-level markers
/// (`s.type_markers`/`e.type_markers`, the full pre-classification list —
/// `Syntax.rs` module docs — so plane info from `Marker.sigil` survives)
/// and field/variant-level bracket markers (`f.serde_markers`,
/// `v.serde_markers`, which keep their `Marker`s whole; only `@Redact` is
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
