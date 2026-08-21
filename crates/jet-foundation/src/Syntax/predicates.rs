/// D-SHAPE2=A: is `name` a built-in applied rule? The I7/R3 dispatch
/// chokepoint — parser/formatter/sema/LSP ask here, never hand-roll the list.
pub fn is_applied_rule(name: &str) -> bool {
    crate::Policy::applied_rule(name)
        .is_some_and(|row| matches!(row.status, crate::Policy::RuleStatus::Active))
}

/// D-META-AUTO1=A: the one structural auto-derive engine vocabulary. `Codable`
/// is the marker-family spelling for the two codec traits, so it is included
/// by `is_signed_auto_derive` below rather than duplicated in this trait list.
pub const STRUCTURAL_AUTO_DERIVE_TRAITS: &[&str] = &[
    crate::Generics::PRINTABLE,
    crate::Generics::EQUATABLE,
    crate::Generics::DEBUG,
    crate::Generics::COMPARABLE,
    crate::Generics::ENCODE,
    crate::Generics::DECODE,
];

/// D-META-AUTO1=A / D-AUTODERIVE-SYNTAX1=D: `!` refuses any built-in
/// capability that the structural auto-derive engine can provide. Keep this
/// vocabulary in the owner-controlled Syntax surface so the parser, type
/// marker classifier, and engine cannot drift apart.
pub fn is_signed_auto_derive(name: &str) -> bool {
    STRUCTURAL_AUTO_DERIVE_TRAITS.contains(&name) || name == crate::Syntax::MARKER_CODABLE
}

// D-UNITLIT1: unit-suffix numeric literals (`500ms`) are not an enumerable
// keyword — the lexer resolves a literal's identifier suffix against
// #UnitFamily members in scope (MARKER_UNIT_FAMILY, D-QUAL3). One fixed rule:
/// D-UNITLIT1: a literal suffix shaped `e` + digits is reserved for float
/// exponent notation (`1e5`) and may never resolve as a unit name.
pub const UNIT_SUFFIX_EXPONENT_RESERVED: &str = "e"; // D-UNITLIT1
/// D-TYPE2-IMAG1=A (ratified 2026-08-06): numeric suffix `i` mints the
/// imaginary component of Complex through the existing unit-literal path;
/// bare `i` remains an ordinary identifier.
pub const UNIT_SUFFIX_IMAGINARY: &str = "i";

// D-TRAILBLOCK2=A (amends D-TRAILBLOCK1): no new token — code arguments are
// ordinary `() :> { … }` lambdas inside call parentheses. A bare `{` after a
// call is E0335 (retired trailing-block sugar), not a new lexical form.
// D-DESTRUCT1: no new token — reuses the D-DOTCTOR1 `.{` sigil in pattern
// position and `..` (OP_RANGE) as the now-mandatory partial-pattern rest
// marker.
// D-CHAINCMP1: no new token — same-direction `<`/`<=`/`>`/`>=` chains are a
// parser/sema desugaring (`0 <= sev < 10` → `0 <= sev && sev < 10`, middle
// operand evaluated once).
// D-CLIFLAG1: the struct-level CLI-derive marker and field-level doc marker
// spellings ride D-CONTRACTCASE1/D-VERDICT-732-1 (formerly D-MARKERMOVE1) —
// constants land with them.
// D-EFFBUDGET1: `effects`/`allow`/`deny`/`grants` are package.jet manifest
// keys (jet_pkg_model::Package), not language tokens; effect names reuse D-EFF4.
// D-ANY-JAI1 + D-VARARGBOUND1: reuses D-VARIADIC1 `...T`; multi-trait
// bounds use the owner-amended list form (`T: [A, B]`, `...items: [A, B]`).
// D-UFCS1 (B), D-POINTERCHAIN1 (A), D-ERRCTX1 (D): no typeable surface.

// ── Module name resolution helpers ───────────────────────────────────────────
//
// These are pure string functions used by both Sema and Codegen to identify
// compiler-known ("core") modules. They live here so that neither Sema nor
// Codegen need to depend on Loader (which does file I/O and belongs in the
// driver layer).

/// Single canonical source of truth for all known Core modules (c45).
///
/// `is_known_core_module` and `core_modules_list` both derive from this slice.
/// `core_module_items` in Sema/CheckerCoreLib.rs has per-module item data and
/// cannot collapse here, but a drift-guard test (tests/corelib.rs) asserts its
/// key set equals this slice.
pub const KNOWN_CORE_MODULES: &[&str] = &[
    "core",
    "core.prelude",
    // D-CORE-TREE1=A: one canonical nested Core tree. Keep this list in
    // lexical/domain order; deleted free namespaces do not get aliases here.
    "core.files",
    "core.term",
    "core.args",
    "core.log",
    "core.process",
    "core.sys",
    "core.math",
    "core.math.random",
    "core.time",
    "core.time.expiring",
    "core.tasks",
    "core.testing",
    "core.mem",
    "core.mem.scope",
    "core.mod",
    "core.reflect",
    "core.compiler",
    "core.compiler.lang",
    "core.encoding",
    "core.encoding.json",
    "core.encoding.jsonl",
    "core.encoding.csv",
    "core.encoding.toml",
    "core.encoding.yaml",
    "core.encoding.xml",
    "core.encoding.cbor",
    // D-UUIDENC1=A (ratified 2026-06-26): hex and base64 codecs (pure, no deps).
    "core.encoding.hex",
    "core.encoding.base64",
    "core.encoding.base32",
    "core.text",
    "core.text.fmt",
    "core.regex",
    "core.net",
    "core.net.tls",
    "core.net.ws",
    "core.net.url",
    "core.net.mime",
    "core.http",
    "core.http.client",
    "core.http.server",
    "core.crypto",
    "core.crypto.random",
    "core.crypto.uuid",
    "core.crypto.vault",
    "core.crypto.expert",
    "core.email",
    "core.data",
    "core.data.plot",
    "core.data.sketch.hll",
    "core.data.sketch.tdigest",
    "core.data.sketch.reservoir",
    "core.data.sketch.cms",
    "core.compute",
    "core.compute.solve",
    "core.db",
    "core.auth",
    "core.sync",
    "core.event",
    "core.reactive",
    "core.reactive.loadable",
    "core.service",
    "core.watcher",
    "core.game",
    "core.game.raylib",
    "core.ui",
    "core.web",
    "core.web.browser",
    "core.web.storage",
    "core.web.storage.local",
    "core.web.storage.session",
    "core.web.devserver",
    "core.archive",
    "core.archive.gzip",
    "core.archive.zstd",
    "core.plugin",
    "core.units",
    "core.perf",
    "app",
];

pub fn is_known_core_module(name: &str) -> bool {
    if KNOWN_CORE_MODULES.contains(&name) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{is_known_core_module, KNOWN_CORE_MODULES};

    #[test]
    fn core_module_keys_reject_internal_jet_prefix() {
        assert!(KNOWN_CORE_MODULES.iter().all(|name| !name.starts_with("jet.")));
        for ring in ["log", "crypto", "http", "regex", "reactive", "archive", "game", "db", "plugin", "time"] {
            assert!(is_known_core_module(&format!("core.{ring}")));
            assert!(!is_known_core_module(&format!("jet.{ring}")));
        }
    }
}

pub fn core_modules_list() -> String {
    KNOWN_CORE_MODULES.join(", ")
}

/// The canonical Core home for an old first-party ring spelling.
pub fn canonical_ring_module(name: &str) -> String {
    match name {
        "raylib" => "core.game.raylib".to_string(),
        _ => format!("core.{name}"),
    }
}

/// E2-M9: ring module names that resolve as compiler-known modules.
pub fn is_ring_module(name: &str) -> bool {
    matches!(
        name,
        "log" | "crypto" | "http" | "regex" | "reactive" | "archive" | "raylib" | "db" | "time"
            // D-DEP-WASM1=A (c81): `core.plugin` — the
            // wasmtime-backed plugin loader (`Plugin.load`/`.call`).
            | "plugin"
    )
}

/// The env var that names the active realized toolchain object directory
/// (D-JPK-TOOLCHAIN1 / #179). Tests set it to a fixture; #179's realizer points
/// it at the hangar object. The object carries prebuilt ring artifacts under
/// `<dir>/ring/<name>` (D-JPK-RINGSHIP1=C).
pub const TOOLCHAIN_OBJECT_ENV: &str = "JET_TOOLCHAIN_FIXTURE";

/// D-JPK-TOOLCHAIN1=A (#179): re-exec guard marker. Before `jet` execs a
/// version-pinned toolchain, it sets this env var to the pinned version. The
/// child, seeing its own version match the marker, runs natively and never
/// re-realizes or re-execs — this breaks the exec loop and lets the pinned
/// toolchain run the program directly.
pub const TOOLCHAIN_EXEC_MARKER_ENV: &str = "JET_TOOLCHAIN_EXEC";

/// D-JPK-RINGSHIP1=C: is this ring lib present as a realized hangar object for
/// the active toolchain? True when the active toolchain object carries a
/// prebuilt artifact for `name` on this platform; false otherwise (the loader
/// then falls back to the compiler-embedded template — rung-0 magic preserved).
pub fn is_ring_module_staged(name: &str) -> bool {
    staged_ring_artifact(name).is_some()
}

/// The prebuilt ring artifact path for `name` in the active toolchain object, or
/// `None` when there is no active object or it carries no artifact for `name`.
pub fn staged_ring_artifact(name: &str) -> Option<std::path::PathBuf> {
    if !is_ring_module(name) {
        return None;
    }
    let dir = std::env::var_os(TOOLCHAIN_OBJECT_ENV)?;
    let artifact = std::path::Path::new(&dir).join("ring").join(name);
    artifact.exists().then_some(artifact)
}

pub fn is_legacy_std_import(name: &str) -> bool {
    name == "std" || name.starts_with("std.") || name == "jet.std" || name.starts_with("jet.std.")
}

/// D-ALLOC1/D-ALLOC-C: allocator opaque types → jet_mem Rust types.
pub fn alloc_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "Arena" => Some("jet_mem::JetArena"),
        "Bump" => Some("jet_mem::JetBump"),
        "Pool" => Some("jet_mem::JetPool"),
        "Fixed" => Some("jet_mem::JetFixed"),
        _ => None,
    }
}

/// D-ARGS1: ArgsSpec/ParsedArgs → prelude Rust types.
pub fn args_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "ArgsSpec" => Some("JetArgsSpec"),
        "ParsedArgs" => Some("JetParsedArgs"),
        _ => None,
    }
}

/// D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)`'s handle types are top-level
/// prelude structs, same shape as `args_handle_rust_type` above.
pub fn reflect_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "Value" => Some("JetReflectValue"),
        "Field" => Some("JetReflectField"),
        _ => None,
    }
}

/// D-SHIFT1 (c7shift): `binary.Reader`/`text.Cursor` handle types are
/// top-level prelude structs, same shape as `reflect_handle_rust_type`
/// above — including the caller's `!type_names.contains(name)` collision
/// guard, since "Reader"/"Cursor" are plausible user type names.
pub fn binary_text_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "Reader" => Some("JetReader"),
        "Cursor" => Some("JetCursor"),
        TYPE_BITS => Some("JetBitSet"),
        TYPE_BYTES => Some("JetByteBuffer"),
        _ => None,
    }
}

/// True when a crate spec string needs an explicit version (i.e. it's not
/// "std" and doesn't already contain `@`).
pub fn crate_spec_needs_version(spec: &str) -> bool {
    spec != "std" && spec.split_once('@').is_none()
}

/// D-EFFBUDGET1 (ratified 2026-07-01): the `effects { allow: […], deny: […] }`
/// block in `pkg.jet` that turns on whole-dependency-graph effect enforcement.
/// Manifest keys only — no language grammar (§0.4 DO-NOT).
pub const MANIFEST_BLOCK_EFFECTS: &str = "effects"; // D-EFFBUDGET1
/// D-EFFBUDGET1: the `allow:` field inside `effects { … }` — the closed list of
/// effect names the whole dependency graph may use.
pub const EFFECTS_FIELD_ALLOW: &str = "allow"; // D-EFFBUDGET1
/// D-EFFBUDGET1: the `deny:` field inside `effects { … }` — effect names the
/// dependency graph must never use.
pub const EFFECTS_FIELD_DENY: &str = "deny"; // D-EFFBUDGET1
/// D-EFFBUDGET1: the `grants { "dep": [Effect] }` block — an audited
/// per-dependency escape from the `authority.holds` budget, recorded in the lockfile.
pub const MANIFEST_BLOCK_GRANTS: &str = "grants"; // D-EFFBUDGET1
/// D-POLICY-WORD1=A: package floors and memory governance live under the
/// `policy: { … }` block in `package.jet`. Manifest keys only, no language
/// grammar.
pub const MANIFEST_BLOCK_POLICY: &str = "policy"; // D-POLICY-WORD1
/// D-MEM-GUARANTEE1=A: package-only dependency containment policy. These
/// fields intentionally do not enter the source `PolicyKey` registry.
pub const POLICY_FIELD_CONTAIN: &str = "contain";
pub const POLICY_FIELD_HARDEN: &str = "harden";
/// D-AUTHORITY-MANIFEST1=A / D-BOUND-PROV1=A: source-reviewed trust and
/// provider authority live under the one `authority: { … }` block.
pub const MANIFEST_BLOCK_AUTHORITY: &str = "authority"; // D-AUTHORITY-MANIFEST1
pub const AUTHORITY_FIELD_HOLDS: &str = "holds"; // D-AUTHORITY-MANIFEST1
pub const AUTHORITY_HOLDS_FIELD_ALLOW: &str = "allow"; // D-AUTHORITY-MANIFEST1
pub const AUTHORITY_HOLDS_FIELD_DENY: &str = "deny"; // D-AUTHORITY-MANIFEST1
pub const AUTHORITY_FIELD_GRANTS: &str = "grants"; // D-AUTHORITY-MANIFEST1
pub const AUTHORITY_FIELD_TRUST: &str = "trust"; // D-BOUND-PROV1
/// D-JPK-PROVIDERAUTH1=A: reviewed registry and fetch authority.
pub const AUTHORITY_FIELD_PROVIDERS: &str = "providers";
pub const PROVIDER_FIELD_REGISTRY: &str = "registry";
pub const PROVIDER_FIELD_ALLOW: &str = "allow";
pub const PROVIDER_FIELD_DENY: &str = "deny";
pub const AUTHORITY_TRUST_FIELD_DEFAULT: &str = "default"; // D-AUTHORITY-MANIFEST1
pub const AUTHORITY_TRUST_FIELD_CI: &str = "ci"; // D-AUTHORITY-MANIFEST1
pub const AUTHORITY_TRUST_FIELD_PROMPT: &str = "prompt"; // D-AUTHORITY-MANIFEST1
pub const AUTHORITY_TRUST_FIELD_SERVICES: &str = "services"; // D-AUTHORITY-MANIFEST1
pub const AUTHORITY_TRUST_FIELD_REQUIRE: &str = "require"; // D-BOUND-PROV1
pub const AUTHORITY_TRUST_REQUIRE_NONE: &str = "none"; // D-BOUND-PROV1
pub const AUTHORITY_TRUST_REQUIRE_LOGGED: &str = "logged"; // D-BOUND-PROV1
pub const AUTHORITY_TRUST_REQUIRE_ATTESTED: &str = "attested"; // D-BOUND-PROV1
pub const AUTHORITY_TRUST_DECISION_PROMPT: &str = "prompt"; // D-AUTHORITY-MANIFEST1
pub const AUTHORITY_TRUST_DECISION_DENY: &str = "deny"; // D-AUTHORITY-MANIFEST1
pub const AUTHORITY_TRUST_DECISION_ALLOW: &str = "allow"; // D-AUTHORITY-MANIFEST1
/// D-LINTPOLICY1=A (the override law, card #505): the `policy: { lints: { … } }`
/// sub-block. Warnings never fail a build by default (I1 memory/type safety is
/// never in scope here); a team opts into a wall by naming stable lints here.
/// Manifest keys only, no language grammar.
pub const POLICY_FIELD_LINTS: &str = "lints"; // D-LINTPOLICY1
/// D-LINTPOLICY1: the `deny:` field inside `policy.lints { … }` — stable lint
/// names (e.g. `float_money`) that fail the build when they fire, instead of
/// warning.
pub const LINTS_FIELD_DENY: &str = "deny"; // D-LINTPOLICY1

/// Levenshtein edit distance between two strings (used for "did you mean?" suggestions).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur.push((prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost));
        }
        prev = cur;
    }
    prev[b.len()]
}
use super::core_surface::{
    CLOCK_TYPE, KW_CONST, KW_COPY, KW_MOVE, KW_MUTATE,
    KW_YIELD, LIT_NULL, RETIRED_TYPE_ERROR, TYPE_DATETIME, TYPE_ERR,
    TYPE_FRACTION, TYPE_INSTANT, TYPE_PATH, TYPE_REGEX, TYPE_URL,
};
use super::effects_surface::KW_STATE_DECL;
use super::math_layout::{
    FOREIGN_MATCH, KW_COMPTIME, KW_SWITCH, TYPE_BITS, TYPE_BYTES, TYPE_DATA, TYPE_JSON,
    TYPE_RESULT,
};
use super::package_files::{JET_KEYWORD_LIST, JET_TYPE_LIST};
use super::{canonical_name_case, NameCase};

/// D-NAME-SIGIL1=A: every compiler-visible generated symbol uses one reserved
/// machine prefix. Keep this helper below the syntax surface so sema, engines,
/// tools, and generated Rust share one naming law.
pub const GENERATED_NAME_PREFIX: &str = "__jet_";

pub fn generated_name(name: &str) -> String {
    if name.starts_with(GENERATED_NAME_PREFIX) {
        name.to_string()
    } else {
        format!("{GENERATED_NAME_PREFIX}{name}")
    }
}

pub fn generated_path(name: &str) -> String {
    generated_name(&name.replace('.', "__"))
}

pub fn generated_suffix(name: &str) -> &str {
    name.strip_prefix(GENERATED_NAME_PREFIX).unwrap_or(name)
}

/// Sanitize a generated Rust crate name from a source-file stem.
pub fn sanitize_crate_name(raw: &str) -> String {
    let mut name: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() {
        name.push_str("out");
    }
    if name.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        name.insert(0, '_');
    }
    name
}

/// D-SHAPE-INTERNAL1 / D-SHAPE-DUNDER2: the one prefix classification used by
/// the lexer, sema, publishing, and tools. Bare `_` remains the pattern/binding
/// spelling; only longer names participate in the public-name contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifierClass {
    Ordinary,
    SoftPublic,
    Reserved,
}

pub fn classify_identifier(name: &str) -> IdentifierClass {
    if name.starts_with("__") {
        IdentifierClass::Reserved
    } else if name.len() > 1 && name.starts_with('_') {
        IdentifierClass::SoftPublic
    } else {
        IdentifierClass::Ordinary
    }
}

/// The complete reserved-name policy for generated, user-visible source.
///
/// `JET_KEYWORD_LIST` is the canonical current surface. Contextual `state`
/// remains legal in ordinary declaration positions, while the additional
/// spellings below are lexer keywords, literals, or foreign teaching words
/// that must not be emitted. Generated source must obey the lexer and its
/// teaching redirects, not the smaller completion list. Type names also share
/// the canonical built-in/Core tables.
pub fn is_reserved_generated_name(name: &str) -> bool {
    (JET_KEYWORD_LIST.contains(&name) && name != KW_STATE_DECL)
        || matches!(
            name,
            KW_MUTATE
                | KW_MOVE
                | KW_COPY
                | KW_CONST
                | KW_COMPTIME
                | KW_SWITCH
                | KW_YIELD
                | FOREIGN_MATCH
                | LIT_NULL
        )
        || JET_TYPE_LIST.contains(&name)
        || crate::Collections::is_reserved_type(name)
        || crate::CoreModuleExports::is_core_type_name(name)
        || matches!(
            name,
            TYPE_DATA
                | TYPE_JSON
                | TYPE_REGEX
                | TYPE_URL
                | TYPE_PATH
                | TYPE_DATETIME
                | CLOCK_TYPE
                | TYPE_INSTANT
                | TYPE_FRACTION
                | TYPE_RESULT
                | TYPE_ERR
                | RETIRED_TYPE_ERROR
        )
}

/// Turn an external name into a legal, canonical Jet identifier and repair
/// every lexer/core-reserved result. All binders call this helper so their
/// generated fields and types cannot drift apart in their name policy.
pub fn sanitize_generated_name(raw: &str, case: NameCase, fallback: &str) -> String {
    // Clark-expanded names use `{uri}local` notation. The opening delimiter
    // is namespace syntax, not part of the generated source identifier.
    let raw = raw.trim_start_matches(['@', '$']);
    let raw = raw.strip_prefix('{').unwrap_or(raw);
    let mut normalized = String::new();
    let mut previous_lower_or_digit = false;
    for ch in raw.chars() {
        if ch == '_' {
            if !normalized.ends_with('_') {
                normalized.push('_');
            }
            previous_lower_or_digit = false;
        } else if ch.is_alphanumeric() {
            if case == NameCase::Snake
                && ch.is_uppercase()
                && previous_lower_or_digit
                && !normalized.ends_with('_')
            {
                normalized.push('_');
            }
            normalized.extend(ch.to_lowercase().take(if case == NameCase::Snake { 1 } else { 0 }));
            if case == NameCase::Pascal {
                normalized.push(ch);
            }
            previous_lower_or_digit = ch.is_lowercase() || ch.is_ascii_digit();
        } else if normalized.is_empty() {
            // Do not turn leading wire punctuation into a source-level
            // leading underscore. Preserve an explicit leading `_` above.
            previous_lower_or_digit = false;
        } else if !normalized.ends_with('_') {
            normalized.push('_');
            previous_lower_or_digit = false;
        }
    }
    while normalized.ends_with('_') {
        normalized.pop();
    }
    if normalized.is_empty() {
        normalized = fallback.to_string();
    }
    let mut result = canonical_name_case(&normalized, case);
    while result.ends_with('_') {
        result.pop();
    }
    if result.is_empty() {
        result = canonical_name_case(fallback, case);
    }

    // A preserved soft-public `_` is legal only before an alphabetic name.
    // Repair `_1name` and other non-identifier starts before the general
    // leading-character repair below.
    let body = result.strip_prefix('_').unwrap_or(&result);
    if body.chars().next().is_some_and(|ch| !ch.is_alphabetic()) {
        result = body.to_string();
    }
    if result.is_empty() {
        result = canonical_name_case(fallback, case);
    }
    let first = result.strip_prefix('_').unwrap_or(&result).chars().next();
    if first.is_some_and(|ch| !ch.is_alphabetic()) {
        let prefix = match case {
            NameCase::Pascal => "Type",
            NameCase::Snake => "field_",
        };
        result.insert_str(0, prefix);
    }
    let suffix = match case {
        NameCase::Pascal => "Type",
        NameCase::Snake => "_field",
    };
    while result == "_" || result.starts_with("__") || is_reserved_generated_name(&result) {
        result.push_str(suffix);
    }
    result
}

#[cfg(test)]
mod generated_name_tests {
    use super::{
        generated_name, generated_path, generated_suffix, is_reserved_generated_name,
        sanitize_crate_name,
    };
    use crate::Syntax::{classify_identifier, IdentifierClass};

    #[test]
    fn generated_names_have_one_machine_prefix() {
        assert_eq!(generated_name("run"), "__jet_run");
        assert_eq!(generated_name("__jet_run"), "__jet_run");
        assert_eq!(generated_path("grades.curve"), "__jet_grades__curve");
        assert_eq!(generated_path("__jet_grades__curve"), "__jet_grades__curve");
        assert_eq!(generated_path("__jet_grades.curve"), "__jet_grades__curve");
        assert_eq!(generated_suffix("__jet_run"), "run");
    }

    #[test]
    fn crate_names_are_rust_safe() {
        assert_eq!(
            sanitize_crate_name("panic_lexical_task.group"),
            "panic_lexical_task_group"
        );
        assert_eq!(sanitize_crate_name("3d-demo"), "_3d_demo");
        assert_eq!(sanitize_crate_name(""), "out");
    }
    #[test]
    fn canonical_core_names_are_reserved_for_generated_names() {
        for name in ["Decimal", "Duration", "Date", "LocalDate", "LocalTime", "JSONError"] {
            assert!(is_reserved_generated_name(name), "unreserved canonical Core name `{name}`");
        }
    }

    #[test]
    fn underscore_ladder_has_one_source_classification() {
        assert_eq!(classify_identifier("_"), IdentifierClass::Ordinary);
        assert_eq!(classify_identifier("_name"), IdentifierClass::SoftPublic);
        assert_eq!(classify_identifier("__name"), IdentifierClass::Reserved);
        assert_eq!(classify_identifier("__name__"), IdentifierClass::Reserved);
        assert_eq!(classify_identifier("name_"), IdentifierClass::Ordinary);
    }
}
