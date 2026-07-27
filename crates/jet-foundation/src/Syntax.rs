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
// ATTR_LAYOUT, ATTR_CODABLE, Policy::APPLIED_RULES, KW_CAPS, KW_GRANT,
// KW_COMPTIME, KW_DERIVE, ATTR_TRACK, ATTR_LOCAL, ATTR_SHARED. Constants live in the private modules
// below; keep this root file mentioning them so I7 audits can check one
// canonical surface entrypoint.
//
// D-BIND-BARE1=A adds no token: bindings are always bare `name :: value` /
// `name := value`; types ride values (`Type.{ … }`) or live on signatures
// and fields. Retires `name: Type ::` / `name: Type :=`.
// D-UNINIT-SENTINEL2=A amends D-UNINIT-SENTINEL1: `uninit` is legal only as
// the whole body of a `Type.{ uninit }` head (`name := Type.{ uninit }`).
// Retires annotated `name: Type := uninit`. KW_UNINIT stays the same token.
// D-SHAPE-CLI1 reuses the existing `fn run` / `#Cli` surface: a resolved
// entry-parameter type owns typed shell inputs, while zero-parameter `fn run()`
// stays valid. D-CLI-POS1=A adds field marker `Flag` (`CONTRACT_FLAG`): required
// value fields fill positionally by declaration order; `#[Flag]` keeps a field
// flag-only. D-SHAPE6 adds no Jet source token; grouped tool commands remain
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
// D-SHAPE-CTORVERB1=C closes fresh-value construction under the `new` prefix:
// MEM_ALLOC_NEW owns deterministic construction; METHOD_FRESH_NEW_RANDOM owns
// entropy-drawing construction; CLOCK_TYPE and EXPIRING_VALUE_TYPE own the
// migrated type names. Lowercase module factories and `generate` are retired.
// D-UNSAFE-OBLIG1=A adds contextual `assert valid_ptr, aligned, no_alias`,
// the `obligations: .Track/.Skip` #Unsafe field, and ENV_ORG_UNSAFE_POLICY.
// D-SHAPE-INTERNAL1=A and D-SHAPE-DUNDER2=A add no token: the canonical
// IdentifierClass prefix policy makes `_name` soft-public and reserves every
// source-written `__name` for Jet and generated tooling.
// D-SHAPE-MODULEINTERNAL1=A adds no token: MODULE_INTERNAL_PREFIX is the
// automatic-discovery opt-out for `module _name`; explicit imports still use
// ordinary resolution and access rules.
// D-SHAPE-CASE1=C owns the identifier category table and its two enforced
// shapes. D-SHAPE-CASE2=A exempts foreign names inside FFI binding modules.
// D-ARROW-CONTROL1=A (ratified 2026-07-26, card #1209) splits callable and
// control syntax. OP_CALLABLE_ARROW (`=>`) defines callable results.
// EFFECT_ARROW_OPEN/CLOSE (`=[` / `]=>`) add effect ceilings. OP_ARM_ARROW
// (`->`) selects dispatch/guard values and D-LOOPEVAL1 yields finite-loop
// items. Effect-only `if` and `loop` bodies use no arrow. Braces only group
// multiline bodies. D-LOOPSTATE1 owns break/next target arguments, and
// D-COMPREHENSION1 fixes yielding-loop results to eager List.
// D-IFGUARD1=A adds no spelling: subjectless statement/value guard tables
// reuse KW_IF, KW_ELSE, OP_ARM_ARROW, braces, and ordinary Bool expressions.
// D-SHAPE-CONVERT1=A adds no punctuation: explicit conversion is always a
// destination-owned `Target.from_source(value)` static method. Text remains
// the existing `Target.parse(text)` operation; source-owned `to_*` aliases are
// not part of the language surface.
// D-OPDEF1=A adds no punctuation: `impl Type.Add`/`.Sub`/`.Mul`/`.Div`,
// `.Equatable`, and `.Comparable` reuse ordinary trait-impl dot syntax.
// D-HTTP-ROUTE-SYNTAX2=A owns the two route-pattern markers carried inside
// ordinary String values. They are not lexer tokens; the HTTP router consumes
// them after String evaluation.
// D-FLOWTYPE1=A adds no token: after a stable immutable `T?` name is checked
// with `x != None` (true) or `x == None` (false/else), sema refines that name
// to `T` for the proven branch and records an S31 Present unwrap for TIR.
// Mutable locals, fields, indexes, aliases, and calls stay `T?`; bind with
// `x == Val(v)` instead. Facts reach the right side of `&&` only, not `||`.
// D-UNIONTYPE1=A reuses the existing `|` token (TokKind::Pipe / BitOr) in type
// position as TYPE_UNION_SEP. `T ? E1 | E2` parses as `T ? (E1 | E2)`.
pub const HTTP_ROUTE_PARAM_PREFIX: &str = ":";
pub const HTTP_ROUTE_CATCH_ALL_PREFIX: &str = "*";

// D-PARCAPTURE1=D (ratified 2026-07-20): every explicit parallel collection
// adapter uses the owner-selected `para_` prefix. These are a clean break from
// D-AUTOPAR1's provisional `par_` spellings; there are no aliases.
pub const METHOD_PARA_MAP: &str = "para_map";
pub const METHOD_PARA_FILTER: &str = "para_filter";
pub const METHOD_PARA_PARTITION: &str = "para_partition";
pub const METHOD_PARA_FOLD: &str = "para_fold";
pub const PARA_METHODS: &[&str] = &[
    METHOD_PARA_MAP,
    METHOD_PARA_FILTER,
    METHOD_PARA_PARTITION,
    METHOD_PARA_FOLD,
];

/// Validate a source-literal HTTP route before code generation. Runtime route
/// parsing repeats this check for computed Strings.
pub fn validate_http_route_pattern(pattern: &str) -> Result<(), String> {
    fn valid_name(name: &str) -> bool {
        let mut chars = name.chars();
        matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
            && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
    }

    fn decode_static(segment: &str) -> Result<(), String> {
        let bytes = segment.as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != b'%' {
                decoded.push(bytes[index]);
                index += 1;
                continue;
            }
            let hex = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            let Some(high) = bytes.get(index + 1).and_then(|byte| hex(*byte)) else {
                return Err("invalid percent escape".to_string());
            };
            let Some(low) = bytes.get(index + 2).and_then(|byte| hex(*byte)) else {
                return Err("invalid percent escape".to_string());
            };
            let byte = high * 16 + low;
            if byte == b'/' {
                return Err("encoded slash is ambiguous".to_string());
            }
            decoded.push(byte);
            index += 3;
        }
        let decoded = String::from_utf8(decoded)
            .map_err(|_| "route segment is not valid UTF-8".to_string())?;
        if decoded == "." || decoded == ".." {
            return Err("dot traversal segment is not allowed".to_string());
        }
        Ok(())
    }

    if !pattern.starts_with('/') {
        return Err("routes must start with `/`".to_string());
    }
    let raw_segments: Vec<&str> = pattern.split('/').skip(1).collect();
    let mut names = std::collections::BTreeSet::new();
    for (index, segment) in raw_segments.iter().enumerate() {
        if segment.is_empty() {
            if raw_segments.len() == 1 {
                continue;
            }
            return Err("empty path segments are not allowed".to_string());
        }
        if segment.contains('{') || segment.contains('}') {
            return Err("use `:name` or final `*name`; braces are not route markers".to_string());
        }
        if *segment == "*" {
            return Err("write a named catch-all such as `*wildcard`".to_string());
        }
        let name = segment.strip_prefix(':').or_else(|| segment.strip_prefix('*'));
        if let Some(name) = name {
            if segment.starts_with('*') && index + 1 != raw_segments.len() {
                return Err("`*name` catch-all must be final".to_string());
            }
            if !valid_name(name) {
                return Err(format!(
                    "{} names must match `[A-Za-z_][A-Za-z0-9_]*`",
                    if segment.starts_with('*') { "catch-all" } else { "parameter" }
                ));
            }
            if !names.insert(name) {
                return Err(format!("duplicate parameter `{name}`"));
            }
        } else {
            decode_static(segment)?;
        }
    }
    Ok(())
}

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

// D-SHAPE-QUANTITY1=A adds no source spelling. Physical dimensions use this
// unwriteable internal type marker and the compiler-owned identity table below.
pub const TYPE_QUANTITY: &str = "\0Quantity";
/// D-QUANTITY-TYPE1=A: the sole source-written quantity-bound constructor.
pub const BOUND_QUANTITY: &str = "Quantity";
/// D-QUANTITY-CONVERT1=B: the closed explicit unit-rounding policies.
pub const ROUND_TOWARD_ZERO: &str = "TowardZero";
pub const ROUND_FLOOR: &str = "Floor";
pub const ROUND_CEILING: &str = "Ceiling";
pub const ROUND_NEAREST_EVEN: &str = "NearestEven";

pub fn unit_rounding_mode(name: &str) -> Option<crate::UnitRoundingMode> {
    match name {
        ROUND_TOWARD_ZERO => Some(crate::UnitRoundingMode::TowardZero),
        ROUND_FLOOR => Some(crate::UnitRoundingMode::Floor),
        ROUND_CEILING => Some(crate::UnitRoundingMode::Ceiling),
        ROUND_NEAREST_EVEN => Some(crate::UnitRoundingMode::NearestEven),
        _ => None,
    }
}

/// Canonical `(Length, Time, Temperature)` exponent vectors for physical
/// dimension identities ratified by D-SHAPE-QUANTITY1=A. Currency is
/// deliberately absent: D-QUAL3 currency units remain nominal quantities.
pub const PHYSICAL_DIMENSIONS: &[(&str, [i32; 3])] = &[
    ("Length", [1, 0, 0]),
    ("Time", [0, 1, 0]),
    ("Speed", [1, -1, 0]),
    ("Area", [2, 0, 0]),
    ("Temperature", [0, 0, 1]),
];

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
