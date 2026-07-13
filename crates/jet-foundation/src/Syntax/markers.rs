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

/// D-CABI-PLATFORM1=A: per-function native calling-convention marker for C
/// declarations. C remains the implicit default; alternate ABIs never inherit.
pub const ATTR_ABI: &str = "Abi"; // D-CABI-PLATFORM1

/// D-LINTPOLICY1=A / D-DECIMAL1: per-site lint-suppression marker —
/// `#[allow(lint_name)]` on a struct or field (e.g. `#[allow(float_money)]`
/// silences the default-on money lint L0504). Deliberately lowercase: it
/// names a lint code, not a declaration-shaped feature, so it does not
/// follow the PascalCase marker convention. Struct/field site collection:
/// `collect_allow_markers` in `crates/jet-semindex/src/Build.rs`; the
/// float-money check itself: `allows_float_money` in
/// `crates/jet-foundation/src/Numeric.rs`.
pub const ATTR_ALLOW: &str = "allow"; // D-LINTPOLICY1

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
    ATTR_PUBLISHED_SCHEMA,
    ATTR_REDACT,
    ATTR_NUMERIC,
    // D-MARKERMOVE3: built-in derive markers (user derives stay `#`).
    // ATTR_COMPARABLE ("Comparable") also names the D-CAPBUNDLE1 capability
    // bundle below — same spelling, disambiguated by declaration position
    // (struct/enum derive vs. distinct-type bundle), listed once here.
    // TRAIT_DEBUG ("Debug") is deliberately ABSENT — D-MARK-DEBUG1=A (ratified
    // 2026-07-11, card #498) retired the opt-in `@Debug`/`@[.., Debug]`/
    // `derive Debug;` spellings outright (Debug auto-derives whenever every
    // field qualifies, S55; I8 one way to mean it). Writing it explicitly is
    // E0922 (crates/jet-foundation/src/Traits.rs), not a wrong-plane
    // teaching error — `Debug` is still a real `@`-plane trait name (a
    // hand-written `impl T.Debug { … }` override and `{value@Debug}`
    // reflection stay valid), it's just no longer a name you DERIVE.
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
    ATTR_REGION,
    ATTR_LIVE,
    ATTR_NONDETERMINISTIC,
    ATTR_POLICY,
    // D-SCHEDULE1 (ratified 2026-07-11, card #505): `#Task` / `#Every(…)`.
    KW_TASK,
    ATTR_EVERY,
    ATTR_TRACK,
    ATTR_OFF,
    ATTR_DEBUG_ONLY,
    ATTR_META,
    // D-MARK-TARGET1=A (ratified 2026-07-11, card #498): `#Target(Wasm|Js)`
    // is the one target-marker family (both the ceiling and the per-function
    // override use); the bare `#Wasm`/`#Js` spellings are retired and no
    // longer registered as directive markers (ordinary unknown-marker error).
    ATTR_TARGET,
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
    ATTR_ABI,
    // ATTR_SUPPRESS intentionally absent (D-MARK-DISCARD1=A, ratified
    // 2026-07-11, card #498): `#Suppress(MustUse) { … }` is retired outright
    // — `.drop("reason")` is the sole discard spelling — so `#Suppress`
    // falls through to an ordinary unknown-marker error, same treatment as
    // ATTR_UNINIT/ATTR_REF above.
    ATTR_EXTERN_MODULE,
    ATTR_BINDGEN,
    // D-FFI-INLINE1=A (ratified 2026-07-11, card #501): `#FFI(<lang>) fn`
    // inline foreign tier marker.
    ATTR_FFI,
    "Caller",
    ATTR_RENAME,
    ATTR_SKIP,
    ATTR_DEFAULT,
    ATTR_FLATTEN,
    ATTR_RENAME_ALL,
    ATTR_DENY_UNKNOWN_FIELDS,
    ATTR_TAG,
    ATTR_UNTAGGED,
    // D-LINTPOLICY1=A / D-DECIMAL1 — `#[allow(lint_name)]` per-site suppression.
    ATTR_ALLOW,
];
use super::{
    ATTR_BINDGEN, ATTR_CODABLE, ATTR_COMPARABLE, ATTR_DEBUG_ONLY,
    ATTR_DECODE, ATTR_DEFAULT,
    ATTR_DENY_UNKNOWN_FIELDS, ATTR_ENCODE, ATTR_EXTERN_MODULE, ATTR_FFI, ATTR_FLATTEN, ATTR_HTML,
    ATTR_INVARIANT, ATTR_LAYOUT, ATTR_META, ATTR_MUST_USE,
    ATTR_NUMERIC, ATTR_OFF, ATTR_PUBLISHED_SCHEMA, ATTR_REDACT, ATTR_RENAME, ATTR_RENAME_ALL,
    ATTR_REPLAYABLE, ATTR_SINGLE_USE, ATTR_SKIP, ATTR_SUMMARIZE, ATTR_TAG,
    ATTR_TARGET, ATTR_TRACK, ATTR_UNIT_FAMILY, ATTR_UNTAGGED,
    ATTR_WASM_EXPORT, ATTR_REGION, ATTR_LIVE, ATTR_NONDETERMINISTIC, ATTR_POLICY,
    DSL_BLOCK_SQL, KW_BENCH, KW_CAPS, KW_GRANT, KW_IMPURE, KW_PURE,
    KW_REACTIVE, KW_SANITIZER, KW_STATE, KW_TAINTED, KW_TASK, KW_TEST, KW_TODO, KW_TRANSACT,
    KW_TRANSITION, KW_UNSAFE, ATTR_EVERY,
};
