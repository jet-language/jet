//! OWNER-CONTROLLED SURFACE.
//!
//! Every keyword, sigil, and built-in name a user can type lives in this
//! file and nowhere else (invariant I7). Each constant maps to a decision
//! ID in docs/spec/syntax-decisions.md. Changing a provisional choice means:
//! change it here, update docs/spec/syntax-decisions.md, re-bless the ui snapshots. Done.
//!
//! Agents: do NOT add an entry here without a decision ID approved by the
//! owner in docs/spec/syntax-decisions.md.
// Marker-plane reconciliation anchors: MARKER_PUB_FILE, MARKER_NO_PRELUDE, ATTR_TARGET,
// ATTR_LAYOUT, ATTR_CODABLE, APPLIED_RULES, KW_CAPS, KW_GRANT,
// KW_COMPTIME, KW_DERIVE, ATTR_TRACK. Constants live in the private modules
// below; keep this root file mentioning them so I7 audits can check one
// canonical surface entrypoint.
//
// D-SHAPE-CLI1 reuses the existing `fn run` / `@Cli` surface: a resolved
// entry-parameter type owns typed shell inputs, while zero-parameter `fn run()`
// stays valid. D-SHAPE6 adds no Jet source token; grouped tool commands remain
// owned by the single registry in crates/jet-cli/src/CLI.rs.
// D-ECO-DECL1=A adds no spelling: ecosystem entries reuse ordinary named
// fields and D-DOTCTOR1 `Type.{ ... }` construction. D-ECO-ROOTNAME1 still
// owns the root noun; #560 owns executable source and tooling behavior.
// D-MEM-VIEWRET1=B adds no token, sigil, lifetime spelling, or grammar rule.
// It reuses the existing named-type spellings `View`, `ViewMut`, and the
// restricted `str` element spelling at public string-view boundaries; sema
// infers and publishes their owner provenance.
// D-SHAPE-RESOURCE2=A adds contextual `defer` only at statement head in the
// exact form `defer close(^resource)`; KW_DEFER/RESOURCE_CLOSE are canonical.
// D-SHAPE3a=A adds no token: expected-type `.new(...)` reuses MEM_ALLOC_NEW
// and ordinary call punctuation, with the receiver resolved by sema.
// D-SHAPE-OPAQUE-INFER1=A adds no token: `Type.new(...)` may omit generic
// receiver arguments only when ordinary input/expected-type inference is unique.
// D-UNSAFE-OBLIG1=A adds contextual `assert valid_ptr, aligned, no_alias`,
// the `obligations: .Track/.Skip` @Unsafe field, and ENV_ORG_UNSAFE_POLICY.
// D-SHAPE-INTERNAL1=A and D-SHAPE-DUNDER2=A add no token: the canonical
// IdentifierClass prefix policy makes `_name` soft-public and reserves every
// source-written `__name` for Jet and generated tooling.
// D-SHAPE-CASE1=C owns the identifier category table and its two enforced
// shapes. D-SHAPE-CASE2=A exempts foreign names inside FFI binding modules.
// D-SHAPE-CONVERT1=A adds no punctuation: explicit conversion is always a
// destination-owned `Target.from_source(value)` static method. Text remains
// the existing `Target.parse(text)` operation; source-owned `to_*` aliases are
// not part of the language surface.

/// Compiler-owned numeric source names for D-SHAPE-CONVERT1=A.
pub const NUMERIC_CONVERSION_SOURCES: &[(&str, &str)] = &[
    ("from_i8", "I8"),
    ("from_i16", "I16"),
    ("from_i32", "I32"),
    ("from_int", "Int"),
    ("from_u8", "U8"),
    ("from_u16", "U16"),
    ("from_u32", "U32"),
    ("from_u64", "U64"),
    ("from_f32", "F32"),
    ("from_float", "Float"),
];

pub fn numeric_conversion_source(method: &str) -> Option<&'static str> {
    NUMERIC_CONVERSION_SOURCES
        .iter()
        .find_map(|(name, source)| (*name == method).then_some(*source))
}

pub fn numeric_conversion_method(source: &str) -> Option<&'static str> {
    NUMERIC_CONVERSION_SOURCES
        .iter()
        .find_map(|(method, name)| (*name == source).then_some(*method))
}

/// Canonical D-SHAPE-CONVERT1 method generated from a source type name.
/// Numeric defaults use their bounded aliases; nominal types use snake case.
pub fn conversion_method_for_source(source: &str) -> String {
    numeric_conversion_method(source)
        .map(str::to_string)
        .unwrap_or_else(|| format!("from_{}", canonical_name_case(source, NameCase::Snake)))
}

/// Migration-only lookup for source-owned numeric spellings retired by
/// D-SHAPE-CONVERT1=A. These are diagnostics, not accepted surface.
pub fn retired_numeric_conversion_target(method: &str) -> Option<&'static str> {
    Some(match method {
        "to_i8" => "I8",
        "to_i16" => "I16",
        "to_i32" => "I32",
        "to_i64" | "to_int" => "Int",
        "to_u8" => "U8",
        "to_u16" => "U16",
        "to_u32" => "U32",
        "to_u64" => "U64",
        "to_f32" => "F32",
        "to_f64" | "to_float" => "Float",
        _ => return None,
    })
}

/// The two identifier tiers fixed by D-SHAPE-CASE1=C.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameCase {
    Pascal,
    Snake,
}

/// One compiler-owned category table. Parser/sema callers select the grammar
/// category; spelling policy never gets reimplemented at individual sites.
pub const NAME_CASE_CATEGORIES: &[(&str, NameCase)] = &[
    ("associated type", NameCase::Pascal),
    ("distinct type", NameCase::Pascal),
    ("enum", NameCase::Pascal),
    ("enum variant", NameCase::Pascal),
    ("enum variant group", NameCase::Pascal),
    ("marker", NameCase::Pascal),
    ("protocol", NameCase::Pascal),
    ("protocol message", NameCase::Pascal),
    ("state", NameCase::Pascal),
    ("state type", NameCase::Pascal),
    ("struct", NameCase::Pascal),
    ("tag", NameCase::Pascal),
    ("trait", NameCase::Pascal),
    ("type", NameCase::Pascal),
    ("type alias", NameCase::Pascal),
    ("type parameter", NameCase::Pascal),
    ("unit family", NameCase::Pascal),
    ("config name", NameCase::Snake),
    ("constant", NameCase::Snake),
    ("field", NameCase::Snake),
    ("function", NameCase::Snake),
    ("generic module", NameCase::Snake),
    ("lambda parameter", NameCase::Snake),
    ("local", NameCase::Snake),
    ("local constant", NameCase::Snake),
    ("loop label", NameCase::Snake),
    ("message field", NameCase::Snake),
    ("method", NameCase::Snake),
    ("module", NameCase::Snake),
    ("module alias", NameCase::Snake),
    ("parameter", NameCase::Snake),
    ("pattern binding", NameCase::Snake),
    ("unit member", NameCase::Snake),
    ("value parameter", NameCase::Snake),
    ("variant field", NameCase::Snake),
];

pub fn name_case_for_category(category: &str) -> Option<NameCase> {
    NAME_CASE_CATEGORIES.iter().find_map(|(name, case)| (*name == category).then_some(*case))
}

pub fn name_has_case(name: &str, case: NameCase) -> bool {
    if name == "_" { return true; }
    let name = name.strip_prefix('_').unwrap_or(name);
    if name.is_empty() || name.starts_with('_') || name.ends_with('_') || name.contains("__") { return false; }
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false; };
    match case {
        NameCase::Pascal => (first.is_uppercase() || first.is_alphabetic() && !first.is_lowercase())
            && chars.all(char::is_alphanumeric),
        NameCase::Snake => (first.is_lowercase() || first.is_alphabetic() && !first.is_uppercase())
            && chars.all(|c| c == '_' || c.is_alphanumeric() && !c.is_uppercase()),
    }
}

pub fn canonical_name_case(name: &str, case: NameCase) -> String {
    match case {
        NameCase::Pascal => {
            let leading = name.starts_with('_');
            let out: String = name.trim_start_matches('_').split('_').filter(|s| !s.is_empty()).map(|s| {
            let mut chars = s.chars();
            chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
            }).collect();
            if leading { format!("_{out}") } else { out }
        }
        NameCase::Snake => {
            let leading = name.starts_with('_');
            let chars: Vec<char> = name.trim_start_matches('_').chars().collect();
            let mut out = String::new();
            for (i, c) in chars.iter().copied().enumerate() {
                if c.is_uppercase() {
                    let prev_lower = i > 0 && chars[i - 1].is_lowercase();
                    let next_lower = chars.get(i + 1).is_some_and(|c| c.is_lowercase());
                    if !out.is_empty() && (prev_lower || next_lower) && !out.ends_with('_') { out.push('_'); }
                    out.extend(c.to_lowercase());
                } else { out.push(c); }
            }
            if leading { format!("_{out}") } else { out }
        }
    }
}

#[cfg(test)]
mod casing_tests {
    use super::{canonical_name_case, name_has_case, NameCase};

    #[test]
    fn uncased_unicode_letters_obey_both_grammar_tiers() {
        assert!(name_has_case("日本語", NameCase::Pascal));
        assert!(name_has_case("日本語", NameCase::Snake));
        assert_eq!(canonical_name_case("日本語", NameCase::Pascal), "日本語");
        assert_eq!(canonical_name_case("日本語", NameCase::Snake), "日本語");
    }

    #[test]
    fn cased_unicode_letters_follow_the_selected_tier() {
        assert!(name_has_case("Éclair", NameCase::Pascal));
        assert!(name_has_case("éclair", NameCase::Snake));
        assert_eq!(canonical_name_case("Éclair", NameCase::Snake), "éclair");
        assert_eq!(canonical_name_case("éclair", NameCase::Pascal), "Éclair");
    }
}

mod core_surface;
pub use core_surface::*;
mod math_layout;
pub use math_layout::*;
mod effects_surface;
pub use effects_surface::*;
mod jetpack_config;
pub use jetpack_config::*;
mod package_files;
pub use package_files::*;
mod markers;
pub use markers::*;
mod highlights;
pub use highlights::*;
mod predicates;
pub use predicates::*;
