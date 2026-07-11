use super::*;

/// D-MARKER-FAMILY1: the contract-plane prefix — sibling of `ATTR_PREFIX` ("#").
/// `@` markers attach only to declarations (fn, type, field) plus the
/// D-MARKERMOVE2 function-type bound carve-out, never to other expressions or
/// type positions. Loop-label suffix `@` (D-LOOPLABEL2) is a different
/// grammatical slot and unaffected.
pub const CONTRACT_PREFIX: &str = "@"; // D-MARKER-FAMILY1

/// D-PREPOST1 / D-CONTRACTCASE1: precondition contract on a function
/// signature — `@Pre(cond, "msg")`. The condition is a pure expression (same
/// checker as `@Pure`). Checked in every build by default; per-module
/// build-policy strip is the explicit opt-out.
pub const CONTRACT_PRE: &str = "Pre"; // D-PREPOST1
/// D-PREPOST1 / D-CONTRACTCASE1: postcondition contract — `@Post(cond,
/// "msg")`; `result` names the return value inside `cond`.
pub const CONTRACT_POST: &str = "Post"; // D-PREPOST1

/// D-PERSIST1 / D-CONTRACTCASE1: dev-tier contract on a module-level
/// binding — the value survives `jet dev` hot reloads (identity = module
/// path + binding name). Inert in release builds.
pub const CONTRACT_PERSIST: &str = "Persist"; // D-PERSIST1

/// D-METHODMACRO1=A / D-CONTRACTCASE1: `@Inline fn` / `@Inline` method — a
/// soft hint that this function/method should be inlined (`#[inline]` in
/// codegen). Never rejected by sema; the compiler is free to ignore it.
/// Methods stay ordinary functions — no macro-rewrite hooks (D-METHODMACRO1).
pub const CONTRACT_INLINE: &str = "Inline"; // D-METHODMACRO1
/// D-METHODMACRO1=A / D-CONTRACTCASE1: `@InlineAlways fn` / method — a
/// checked promise (`#[inline(always)]` in codegen). Sema rejects it
/// (E0917 self-recursive / E0918 address-taken / E0919 too large) when the
/// compiler can prove it genuinely cannot inline the call — a compile error
/// naming why, never a silent miss. Mutually exclusive with `CONTRACT_INLINE`
/// on the same declaration (E0920).
pub const CONTRACT_INLINE_ALWAYS: &str = "InlineAlways"; // D-METHODMACRO1

/// D-CAPBUNDLE1 / D-CONTRACTCASE1: capability bundles on a nominal distinct
/// type — each re-exposes a curated slice of the base type's operations
/// while keeping nominal identity. Stackable. The `numeric` bundle merged
/// into `ATTR_NUMERIC` (`@Numeric`, D-MARKERMOVE1) — there is no
/// `CONTRACT_BUNDLE_NUMERIC` constant.
pub const CONTRACT_BUNDLE_COMPARABLE: &str = "Comparable"; // D-CAPBUNDLE1
pub const CONTRACT_BUNDLE_PRINTABLE: &str = "Printable"; // D-CAPBUNDLE1
pub const CONTRACT_BUNDLE_CODABLE_AS_BASE: &str = "CodableAsBase"; // D-CAPBUNDLE1

/// D-CLIFLAG1 (rides D-CONTRACTCASE1/D-MARKERMOVE1, plane+casing fixed
/// 2026-07-02): struct-level CLI-derive marker — `@Cli`. The CLI-generation
/// feature itself is a separate card (c7cliflag); this constant exists so
/// that card builds against a fixed name. Never shipped as `#`, so no
/// teaching error.
pub const CONTRACT_CLI: &str = "Cli"; // D-CLIFLAG1
/// D-PATCH1 (card #181): struct-level derive — generates nested `T.Patch` with
/// `apply`/`diff`/`merge`, Codable by construction (Encode+Decode on Patch).
pub const CONTRACT_PATCHABLE: &str = "Patchable"; // D-PATCH1
/// D-CLIFLAG1: field-level doc marker for CLI-derived help text — `@Doc`.
/// Same status as `CONTRACT_CLI`: registered here, feature built elsewhere.
pub const CONTRACT_DOC: &str = "Doc"; // D-CLIFLAG1

/// D-MARKER-FAMILY1 / D-MARKERMOVE1 / D-MARKERMOVE3 (I7/R3 chokepoint): every
/// name that lives on the `@` contract plane. Union of the D-MARKERMOVE1
/// move list (§2a), the D-CONTRACTCASE1 recase set (§2b), D-MARKERMOVE3's
/// three extra built-in derives, and the D-CLIFLAG1 placeholders (G4). One
/// source of truth for "which plane is this name" — parser, formatter, sema,
/// and LSP all dispatch off `is_contract_marker`/`is_directive_marker`
/// instead of hand-rolled match arms.
pub const CONTRACT_MARKERS: &[&str] = &[
    // D-MARKERMOVE1 move list (§2a)
    KW_PURE,
    ATTR_MUST_USE,
    ATTR_CODABLE,
    ATTR_ENCODE,
    ATTR_DECODE,
    ATTR_EXPERIMENTAL,
    ATTR_TESTED,
    ATTR_HARDENED,
    ATTR_PUBLISHED_SCHEMA,
    ATTR_REDACT,
    ATTR_NUMERIC,
    // D-MARKERMOVE3: built-in derive markers (user derives stay `#`).
    // ATTR_COMPARABLE ("Comparable") also names the D-CAPBUNDLE1 capability
    // bundle below — same spelling, disambiguated by declaration position
    // (struct/enum derive vs. distinct-type bundle), listed once here.
    TRAIT_DEBUG,
    ATTR_SUMMARIZE,
    ATTR_COMPARABLE,
    // D-CONTRACTCASE1 recase set (§2b) — pre/post/persist/bundles
    CONTRACT_PRE,
    CONTRACT_POST,
    CONTRACT_PERSIST,
    // D-METHODMACRO1=A — checked inline contracts
    CONTRACT_INLINE,
    CONTRACT_INLINE_ALWAYS,
    CONTRACT_BUNDLE_PRINTABLE,
    CONTRACT_BUNDLE_CODABLE_AS_BASE,
    // D-CLIFLAG1 (G4) — registered, feature not yet implemented
    CONTRACT_CLI,
    CONTRACT_DOC,
    // D-PATCH1 (card #181)
    CONTRACT_PATCHABLE,
];

/// D-MARKER-FAMILY1: every `#`-plane directive name that a moved-marker
/// reader might confuse for a contract, i.e. the E0063 "did you mean `#`"
/// set (§2c). Not exhaustive of every `#` spelling in the language — just
/// the ones with parser-visible dispatch that needs to reject a stray `@`.
pub const DIRECTIVE_MARKERS: &[&str] = &[
    KW_UNSAFE,
    KW_IMPURE,
    KW_REACTIVE,
    KW_TEST,
    KW_BENCH,
    KW_TODO,
    KW_TAINTED,
    KW_SANITIZER,
    KW_STATE,
    KW_TRANSITION,
    KW_CAPS,
    KW_GRANT,
    KW_TRANSACT,
    ATTR_TRACK,
    ATTR_OFF,
    ATTR_DEBUG_ONLY,
    ATTR_META,
    ATTR_TARGET,
    ATTR_WASM,
    ATTR_JS,
    ATTR_WASM_EXPORT,
    ATTR_HTML,
    DSL_BLOCK_SQL,
    // ATTR_UNINIT intentionally absent (D-UNINIT-SENTINEL1): `#Uninit` is
    // retired outright, not merely on the wrong plane, so `@Uninit` isn't
    // taught "add `#`" — it falls through to an ordinary unknown-marker error.
    // ATTR_REF intentionally absent (D-MEM1/S3): stored-reference fields are
    // deleted outright — `#Ref` falls through to an ordinary unknown-marker
    // error, same treatment as ATTR_UNINIT above.
    ATTR_UNIT_FAMILY,
    ATTR_SINGLE_USE,
    ATTR_REPLAYABLE,
    ATTR_INVARIANT,
    ATTR_LAYOUT,
    ATTR_SUPPRESS,
    ATTR_EXTERN_MODULE,
    ATTR_BINDGEN,
    "Caller",
    ATTR_RENAME,
    ATTR_SKIP,
    ATTR_DEFAULT,
    ATTR_FLATTEN,
    ATTR_RENAME_ALL,
    ATTR_DENY_UNKNOWN_FIELDS,
    ATTR_TAG,
    ATTR_UNTAGGED,
];
