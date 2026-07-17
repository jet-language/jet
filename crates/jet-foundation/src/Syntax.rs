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

/// The two identifier tiers fixed by D-SHAPE-CASE1=C.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameCase {
    Pascal,
    Snake,
}

/// One compiler-owned category table. Parser/sema callers select the grammar
/// category; spelling policy never gets reimplemented at individual sites.
pub const NAME_CASE_CATEGORIES: &[(&str, NameCase)] = &[
    ("type", NameCase::Pascal),
    ("trait", NameCase::Pascal),
    ("enum variant", NameCase::Pascal),
    ("marker", NameCase::Pascal),
    ("unit family", NameCase::Pascal),
    ("function", NameCase::Snake),
    ("method", NameCase::Snake),
    ("field", NameCase::Snake),
    ("local", NameCase::Snake),
    ("module", NameCase::Snake),
    ("unit member", NameCase::Snake),
    ("constant", NameCase::Snake),
];

pub fn name_has_case(name: &str, case: NameCase) -> bool {
    if name == "_" { return true; }
    let name = if case == NameCase::Snake { name.strip_prefix('_').unwrap_or(name) } else { name };
    if name.is_empty() || name.starts_with('_') || name.ends_with('_') || name.contains("__") { return false; }
    match case {
        NameCase::Pascal => name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
            && name.bytes().all(|b| b.is_ascii_alphanumeric()),
        NameCase::Snake => name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
    }
}

pub fn canonical_name_case(name: &str, case: NameCase) -> String {
    match case {
        NameCase::Pascal => name.split('_').filter(|s| !s.is_empty()).map(|s| {
            let mut chars = s.chars();
            chars.next().map(|c| c.to_ascii_uppercase().to_string() + chars.as_str()).unwrap_or_default()
        }).collect(),
        NameCase::Snake => {
            let leading = name.starts_with('_');
            let chars: Vec<char> = name.trim_start_matches('_').chars().collect();
            let mut out = String::new();
            for (i, c) in chars.iter().copied().enumerate() {
                if c.is_ascii_uppercase() {
                    let prev_lower = i > 0 && chars[i - 1].is_ascii_lowercase();
                    let next_lower = chars.get(i + 1).is_some_and(char::is_ascii_lowercase);
                    if !out.is_empty() && (prev_lower || next_lower) && !out.ends_with('_') { out.push('_'); }
                    out.push(c.to_ascii_lowercase());
                } else { out.push(c); }
            }
            if leading { format!("_{out}") } else { out }
        }
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
