//! OWNER-CONTROLLED SURFACE.
//!
//! Every keyword, sigil, and built-in name a user can type lives in this
//! file and nowhere else (invariant I7). Each constant maps to a decision
//! ID in docs/spec/syntax-decisions.md. Changing a provisional choice means:
//! change it here, update docs/spec/syntax-decisions.md, re-bless the ui snapshots. Done.
//!
//! Agents: do NOT add an entry here without a decision ID approved by the
//! owner in docs/spec/syntax-decisions.md.
// D-META-REG1=A / D-META-NAME1=A / D-META-FORM1=A: KW_MARKER is the one
// declaring word, and `Registry::rows` is the one registration table behind it —
// a marker rule, a knowledge plane, a right, and a build fact are rows of the
// same table, separated only by what they attach to. Facts about a rule ride the
// declaration's own named-parameter list under the compile-time mark
// (`$sites: [Site]`, `$repeatable: true`); no clause form and no second keyword
// enter the grammar. Every row states its safe direction and its gate words
// (D-FACT-LAW1=B); a prover may publish a read-only row (D-FACT-OWN1=A).
// Marker-plane reconciliation anchors: MARKER_PUB_FILE, MARKER_NO_PRELUDE, MARKER_TARGET,
// MARKER_LAYOUT, MARKER_CODABLE, Policy::APPLIED_RULES, Registry::rows, KW_CAPS, KW_GRANT,
// KW_COMPTIME, KW_DERIVE, MARKER_TRACK, MARKER_LOCAL, MARKER_SHARED. Constants live in the private modules
// below; keep this root file mentioning them so I7 audits can check one
// canonical surface entrypoint.
// D-GENERIC-CALL1=A: GENERIC_CALL_OPEN and GENERIC_CALL_CLOSE own the adjacent
// call-site type-argument markers; they reuse the existing angle tokens.
// D-FAIL-ERROR1=A: core_surface::TYPE_ERR owns the shared default-error type
// and constructor name. core_surface::RETIRED_TYPE_ERROR exists only for the
// E0432 teaching diagnostic; it never resolves as a type.
//
// D-APILABEL1=A adds the two parameter-zone separators
// PARAM_ZONE_POSITIONAL_ONLY (`/`) and PARAM_ZONE_LABEL_ONLY (`*`), written in
// a parameter list rather than as operators. It also gives a parameter an
// optional public label ahead of its local name (`timeout seconds: Int`),
// which needs no new token. Retires the S61 fixed-position label rule.
//
// D-CONF-WORD1=A gives the word `profile` one meaning, the optimize bundle
// behind `--profile` and its `--release` sugar. The machine axis is
// `--target`, which now takes a declared machine name (`board.<name>`) beside
// a rustc triple. A named environment composition is a preset:
// ENV_FIELD_PRESETS (`presets:`) and ENV_FLAG_PRESET (`--preset`) replace the
// retired `profiles:` field and `--profile` flag, and package/user profiles
// read as generations in prose. Retires ENV_FIELD_PROFILES and the env
// namespace's `--profile`; ENV_FLAG_PROFILE_RETIRED exists only to teach.
//
// D-TRAILBLOCK2=A adds no token: retires D-TRAILBLOCK1 trailing `{ }` sugar.
// Code arguments are ordinary `() => { … }` lambdas inside call parentheses;
// a bare `{` after a call is E0335.
// D-BIND-BARE1=A adds no token: bindings are always bare `name :: value` /
// `name := value`; types ride values (`Type.{ … }`) or live on signatures
// and fields. Retires `name: Type ::` / `name: Type :=`.
// D-UNINIT-SENTINEL2=A amends D-UNINIT-SENTINEL1: `uninit` is legal only as
// the whole body of a `Type.{ uninit }` head (`name := Type.{ uninit }`).
// Retires annotated `name: Type := uninit`. KW_UNINIT stays the same token.
// D-SHAPE-CLI1 reuses the existing `fn run` / `#CLI` surface: a resolved
// entry-parameter type owns typed shell inputs, while zero-parameter `fn run()`
// stays valid. D-CLI-POS1=A adds field marker `Flag` (`MARKER_FLAG`): required
// value fields fill positionally by declaration order; `#[Flag]` keeps a field
// flag-only. D-CLI-FIELD-MARKERS1=A adds `Short` (`MARKER_SHORT`) and `Env`
// (`MARKER_ENV`) to that same field marker list. D-SHAPE6 adds no Jet source
// token; grouped tool commands remain
// owned by the single registry in crates/jet-cli/src/CLI.rs.
// D-ECO-DECL1=A adds no spelling: ecosystem entries reuse ordinary named
// fields and D-DOTCTOR1 `Type.{ ... }` construction. D-ECO-ROOTNAME1 still
// owns the root noun; #560 owns executable source and tooling behavior.
// D-MEM-VIEWRET1=B adds no token, sigil, lifetime spelling, or grammar rule.
// It reuses the existing named-type spellings `View`, `ViewMut`, and the
// restricted `str` element spelling at public string-view boundaries; sema
// infers and publishes their owner provenance.
// D-MEMPROVENANCE3=A adds contextual `from` after a return type (VIEW_FROM),
// with optional `static.<path>` sources (VIEW_FROM_STATIC). Inference stays
// the beginner default; the clause is opt-in expert declaration.
// D-SPREAD1=A reuses `.[` (OP_MEMBER_SPREAD) for member spread
// `prefix.[a, b, c]` → `[prefix.a, prefix.b, prefix.c]`. Call fan-out stays
// removed (D-VERDICT-1324-1).
// D-SHAPE-RESOURCE2=A adds contextual `defer` only at statement head in the
// exact form `defer close(^resource)`; KW_DEFER/RESOURCE_CLOSE are canonical.
// D-SHAPE3a=A adds no token: expected-type `.new(...)` reuses MEM_ALLOC_NEW
// and ordinary call punctuation, with the receiver resolved by sema.
// D-GENERIC-CALL1=A adds no token: `call<T>(...)` reuses the existing angle and
// call tokens for explicit type arguments on every generic call family.
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
// items. Effect-only `if` and `loop` bodies use no arrow. D-BRACE1=A
// (ratified 2026-07-30, card #1335) requires braces around every effect
// `if`/`else`/`loop` body and makes fmt collapse fitting simple bodies.
// D-LOOP-COMMA1=A (ratified 2026-07-30, card #1336) uses commas between loop
// clauses and `(key, value)` for a two-name source binding.
// D-LOOPSTATE1 owns break/next target arguments, and
// D-COMPREHENSION1 fixes yielding-loop results to eager List.
// D-IFGUARD1=A adds no spelling: subjectless statement/value guard tables
// reuse KW_IF, KW_ELSE, OP_ARM_ARROW, braces, and ordinary Bool expressions.
// D-IFDIST1=A (ratified 2026-07-28, card #1305) adds no token: any comparison
// (`== != < > <= >=`) may mark `if subject OP { … }` dispatch. Bare arm atoms
// desugar to `subject OP atom`; `|` unions those atoms; `&&`/`||` combine.
// The same table is a ()-or-value expression in expression position.
// D-BRANCH-PREF1=A / D-BRANCH-ONELINE1=A / D-BRANCH-ELSEIF1=A /
// D-BRANCH-LINT1=A / D-BRANCH-VALUE1=A / D-BRANCH-FMT1=C /
// D-BRANCH-TEACH1=A (ratified 2026-07-28, card #1259) add no token:
// multi-line braced branches and `else if` chains get L0507, while one-line
// effect and value forms stay quiet. Fmt preserves the author's branch shape.
// D-EFFECT-DECL1=A (ratified 2026-07-28, card #1299) mints KW_EFFECT_DECL:
// `effect Root.Leaf` adds one package-view fact and erases before TIR.
// D-EACH1=C (ratified 2026-07-28, card #1239) mints SIGIL_FENCE_OPEN /
// SIGIL_FENCE_CLOSE. D-VERDICT-1320-1 (card #1320) respells them
// `$[ a, b ]$` and opens expression-position fences to expression entries:
// the statement is copied once per entry, fences advance in lock-step.
// D-SHAPE-CONVERT1=A adds no punctuation: explicit conversion is always a
// destination-owned `Target.from_source(value)` static method. Text remains
// the existing `Target.parse(text)` operation; source-owned `to_*` aliases are
// not part of the language surface.
// D-OPDEF1=A adds no punctuation: `impl Type.Add`/`.Sub`/`.Mul`/`.Div`,
// `.Equatable`, and `.Comparable` reuse ordinary trait-impl dot syntax.
// D-AUTODERIVE1=E / D-AUTODERIVE-SYNTAX1=D (ratified 2026-07-29,
// card #1267) amend S55 and D-MARK-DEBUG1. Printable, Equatable, and Debug
// auto-derive when fields qualify. MANIFEST_POLICY_AUTO_DERIVE controls the
// package default. At a type site, the ordinary marker names opt in and a
// leading Bang opts out (`#!Printable`, `#[!Debug, Equatable]`). Missing
// traits follow the package default; a hand-written impl always wins.
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
// D-REGEX-LIT1=D adds no punctuation. Regex patterns use the universal
// `Type.{ body }` / inferred `.{ body }` form from D-DOTCTOR3.
// D-RANGE-VALUE1=A makes `a..b` and `a..<b` construct one nominal Range
// value. Range carries end inclusivity; arm heads and distinct constraints
// keep their literal-only grammar.
// D-FMT-INTERP1=A adds `Fixed` to the closed interpolation-selector set:
// `{value#Fixed(n)}` reuses `#` and ordinary integer-call parentheses.
// D-FMT-INTERP2=A: trailing `=` in a hole reprints the expression source,
// then " = ", then the value — `{count=}` → `count = 3`. Composes with
// selectors: `{count=#Debug}`.
// D-QUANTITY-PRINT1 adds `Unit(name)` and `Unit(bare)` to that same selector
// rail. Bare interpolation keeps the declared symbol as the default.
// D-FACTMODEL1=A: tag, state, taint-kind, and effect leaves are one erased
// compile-time fact model with one segment-aware subsumption rule.
// D-TAG-SURFACE1=A: `tag Name { deny: [...], from: [...] }`, direct `#Name`
// value/type facts, and `#Scrub(Name)` are the sole dataflow-tag surface.
// D-STATE-NS1=A: state facts have the reserved `T.State.Name` qualified plane;
// bare names are sugar only inside `#State` and `#Transition`.
// D-RULEARG-TYPES1=A + D-LANGNS-NAME1=A: compiler marker vocabularies are
// generated enums in `core.lang`, derived from Policy::APPLIED_RULES.
// D-MARKER-NAME-HYGIENE1=A: `#Discriminant("field")` owns serde internal
// discriminants and `#Job fn` owns scheduled entry functions. `#Tag` and
// `#Task` are retired spellings.
pub const HTTP_ROUTE_PARAM_PREFIX: &str = ":";
pub const HTTP_ROUTE_CATCH_ALL_PREFIX: &str = "*";

/// D-CALLDUAL1=E: sema-only metadata carried on `Expr::MethodCall` until TIR
/// lowers a resolved `#Root` call to the ordinary free-function/module-call
/// shape. These strings are never source syntax.
pub const INTERNAL_ROOT_CALL_LOCAL: &str = "__jet_root_call_local";
pub const INTERNAL_ROOT_CALL_IMPORT_PREFIX: &str = "__jet_root_call_import:";
pub const INTERNAL_ROOT_CALL_CORE_PREFIX: &str = "__jet_root_call_core:";

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
    ("variant group", NameCase::Pascal),
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
    // D-META-STAGE1=B: the compile-time mark is part of the name; case policy
    // reads the name under the mark.
    let name = name.strip_prefix('$').unwrap_or(name);
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
    // D-META-STAGE1=B: keep the compile-time mark, re-case the name under it.
    if let Some(rest) = name.strip_prefix('$') {
        return format!("${}", canonical_name_case(rest, case));
    }
    match case {
        NameCase::Pascal => {
            let leading = name.starts_with('_');
            let out: String = name.trim_start_matches('_').split('_').filter(|s| !s.is_empty()).map(|s| {
            let mut chars = s.chars();
            chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
            }).collect();
            if leading { format!("_{out}") } else { out }
        }
        // D-ACRO-CASE1=A: snake conversion uses the mechanical acronym split.
        NameCase::Snake => {
            let leading = name.starts_with('_');
            let body = name.trim_start_matches('_');
            let out = to_snake_acronym(body);
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

    #[test]
    fn acronym_pascal_to_snake() {
        assert_eq!(canonical_name_case("HTTPHeader", NameCase::Snake), "http_header");
        assert_eq!(canonical_name_case("HTTP_API", NameCase::Snake), "http_api");
        assert_eq!(canonical_name_case("MacOS", NameCase::Snake), "mac_os");
    }
}

mod acronyms;
pub use acronyms::*;
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
mod retirements;
pub use retirements::*;
mod sinks;
pub use sinks::*;
