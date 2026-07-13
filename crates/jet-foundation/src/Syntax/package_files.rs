pub use crate::OsTarget::{
    os_target_build_context, os_target_dispatch_arm, os_target_dispatch_exhaustive,
    os_target_mixed_axis, os_target_unmatched_call, OsTarget,
};
pub use crate::RingLayer::{
    core_module_layer, core_usage_layer, layer_ceiling_exceeded, RuntimeLayer,
};
pub use crate::WebPartition::{
    is_abi_safe_type, web_abi_type, web_cross_partition, web_target_browser, WebBucket,
    WebPartitionMarker,
};

/// S52 (ratified M12; amended 2026-06-16, U2): the unified single lockfile lives
/// inside the `.jet/` managed folder (SOURCE_ROOT_DIR). Replaces `jet.lock`
/// (and `pack.lock`); the manifest reshape chunk migrates the old paths.
pub const UNIFIED_LOCK_FILE: &str = ".jet/lock";

/// S52 (ratified M12; amended 2026-06-16, U2): the single shared, content-
/// addressed store ("hangar"), global and never relocated.
pub const HANGAR_DIR: &str = "/etc/jet/hangar";

/// D-JPK-FILES (ratified 2026-06-18): repo metadata and source defaults at
/// repo root. TOML format; holds `[repo]` and `[sources]`. D-WORKSPACE1 moved
/// the old `[packages]` monorepo index to `workspace.jet`.
pub const JETPACK_TOML: &str = "jetpack.toml";

/// D-JPK-FILES (ratified 2026-06-18): `[repo]` table in `jetpack.toml`.
pub const JTOML_TABLE_REPO: &str = "repo";

/// D-JPK-FILES (ratified 2026-06-18): `[sources]` table in `jetpack.toml` —
/// named source refs (`name = "provider@target#ver"`).
pub const JTOML_TABLE_SOURCES: &str = "sources";

/// D-WORKSPACE1: retired `[packages]` table in `jetpack.toml`. Kept only so
/// the parser can emit E1225 with a targeted migration hint.
pub const JTOML_TABLE_PACKAGES: &str = "packages";

/// D-JPK-FILES (ratified 2026-06-18): `name` key in `[repo]`.
pub const JTOML_KEY_NAME: &str = "name";

/// D-JPK-FILES (ratified 2026-06-18): `version` key in `[repo]`.
pub const JTOML_KEY_VERSION: &str = "version";

/// D-DOTCTOR1 (ratified 2026-06-25): named struct construction `Type.{ field: val }`.
/// The dot immediately before `{` is the canonical construction sigil.
/// Inferred form (type from context): `.{ field: val }` — leading dot with no type name.
/// Both are parser-level adjacency of `.` and `{`; the lexer emits no dedicated token.
/// The old dotless `Type { … }` form is teaching error E0320, auto-fixed by `jet fmt`.
/// D-UITREE1 (ratified 2026-06-30): the same sigil also constructs a named-payload
/// enum variant — `.Variant.{ field: val }` / `Type.Variant.{ field: val }` (S30
/// multi-field variants). No new token; `enum_lit_named_fields` in
/// `Parser/Expressions.rs` reuses this `.{` adjacency after a leading-dot variant name.
pub const OP_NAMED_CTOR: &str = ".{";

/// S75 (ratified 2026-06-16): the fan-out operator — `f.[a, b, c]` desugars to
/// `[f(a), f(b), f(c)]`. `.[` is a parser-level adjacency of `.` and `[`;
/// there is no dedicated two-character lexer token (the parser detects `.`
/// immediately followed by `[`). This constant documents the user-visible sigil.
pub const OP_FAN_OUT: &str = ".[";

/// D-VARIADIC1 (ratified 2026-06-27): spread/rest sigil — `name: ...T` variadic
/// parameters (last position only), `f(...xs)` call spread, `[...a, x, ...b]` list spread.
pub const SIGIL_SPREAD: &str = "...";

/// S76 (ratified 2026-06-16): the fixed-size separator in `[T#N]` type
/// position, e.g. `[Int#3]`. Amended by VERSION-# (2026-06-16): `#` also
/// introduces pinned version numbers in package references (`pkg#1.2.0`).
/// Same token, two contexts: `[T#N]` is the type-level size form; `name#ver`
/// is the package version-pin form. No dedicated two-character token in either
/// case — the parser resolves by position.
pub const TYPE_FIXED_SIZE_SEP: &str = "#";

/// D-DIST1 (ratified 2026-06-19): `UserId :: distinct Int` — declares a
/// distinct type (a separate nominal type sharing the base's representation).
/// Used in the value position of a `::` immutable binding at item level;
/// `distinct`-over-`distinct` chaining is rejected in v1.
pub const KW_DISTINCT: &str = "distinct";

/// D-DIST3 (ratified 2026-06-20): unwrap method for a distinct type —
/// `value.raw()` yields the base value. Named-cast family (S42).
pub const METHOD_DISTINCT_RAW: &str = "raw";

/// D-DYNARRAY1 (ratified 2026-07-01): zero-copy window constructor —
/// `list.view(a..b)` — the sole legal spelling of the `View<T>` constructor
/// (parsed specially: the `..` between the two ends is required, a
/// comma-separated arg list is rejected so there is exactly one way to write
/// it, I8). See `docs/spec/stdlib-api-laws.md` and `View<T>` in CoreLib.
pub const METHOD_VIEW: &str = "view";

/// D-SHIFT1 (ratified 2026-07-01, c7shift): `cursor.take_pattern("…")` reuses
/// the D-PARSESTR1 interpolation-literal-as-pattern grammar (`{hole}` /
/// `{hole:Type}`), but a typed hole isn't a legal ordinary interpolation
/// value expression, so the string-literal argument in THIS ONE call
/// position is parsed as a pattern (`try_str_match_pattern`'s engine, reused
/// not duplicated — I8) instead of an ordinary `Expr::Str`. Parsed specially
/// the same way `.view(a..b)` (above) is: one method name, one fixed
/// argument shape, no second call-argument grammar.
pub const METHOD_TAKE_PATTERN: &str = "take_pattern";

/// D-BINPAT1 (ratified 2026-07-12, card #506): a `b"…"` binary pattern
/// literal — the byte-mode sibling of the D-PARSESTR1 string pattern. The `b`
/// prefix on a string literal switches the ONE pattern engine into byte mode:
/// each `{name:U4}` hole reads a fixed-width bit field, an endian suffix
/// (`be`/`le`) picks byte order on a multi-byte read, and a final `{name:...}`
/// captures the remaining bytes as `[U8]`.
pub const BINPAT_PREFIX: char = 'b';
/// D-BINPAT1: multi-byte big-endian read suffix — `{len:U16be}`.
pub const BINPAT_ENDIAN_BIG: &str = "be";
/// D-BINPAT1: multi-byte little-endian read suffix — `{len:U16le}`.
pub const BINPAT_ENDIAN_LITTLE: &str = "le";

/// D-DIST3 / D-CAPBUNDLE1 / D-MARKERMOVE1 (ratified): `@Numeric` marker
/// enables same-type arithmetic on a distinct type. Written `@Numeric` on
/// the same line before the distinct-type name (contract-plane prefix,
/// D-MARKER-FAMILY1). This is the single merged spelling for what used to
/// be two markers doing the same job — the `@Numeric` distinct-type marker
/// (D-DIST3) and the `@numeric` capability bundle (D-CAPBUNDLE1) — folded
/// per D-MARKERMOVE1=B (I8: one way to mean it). `CONTRACT_BUNDLE_NUMERIC`
/// no longer exists as a separate constant; use this one.
pub const ATTR_NUMERIC: &str = "Numeric";

/// D-QUAL3 (ratified 2026-06-24): `#UnitFamily(currency) { usd, eur, gbp }` —
/// declares a family of units. Each member mints one distinct `@Numeric` type
/// (`usd` → `Usd`) that erases to `Float`, so signatures read plain English
/// (`fn subtotal(price: Usd, qty: Int) -> Usd`). The family is the
/// "upgrade to D-DIST2" framing of D-UNIT1: sugar over the distinct-type
/// machinery (D-DIST1/D-DIST3). PascalCase tag per D-CASING1.
pub const ATTR_UNIT_FAMILY: &str = "UnitFamily";

/// D-MIGRATE1 (ratified 2026-06-22): `@PublishedSchema` — marks a struct whose
/// field layout is snapshotted at release time. A breaking field change without
/// a declared migration is E0910. Written `@PublishedSchema` before `struct`.
pub const ATTR_PUBLISHED_SCHEMA: &str = "PublishedSchema"; // D-MIGRATE1

/// D-LIN1 (ratified 2026-06-21, option A; gated on D-QUAL2): `#SingleUse` — marks
/// a type whose values must be consumed exactly once on every reachable path
/// (moved to a `^` parameter or returned). Using one zero times is E0140
/// (unconsumed at scope end) / E0141 (unconsumed on one branch); aliasing one
/// with `&`/`view` is E0142. `#SingleUse` implies `#NoCopy`. The tag is
/// compile-time only and erases in codegen (I3). Written `#SingleUse` before the
/// `struct`/`enum`, same marker idiom as `@PublishedSchema`.
pub const ATTR_SINGLE_USE: &str = "SingleUse"; // D-LIN1

/// D-REPLAY1: `#Replayable fn` marks a function whose reachable effects must be
/// deterministic by default. Ambient `Time`/`Rand`/`Net`/`Io` are rejected unless
/// the work is routed through explicit deterministic/mockable capabilities.
pub const ATTR_REPLAYABLE: &str = "Replayable";

/// D-REFINE1: directive-plane invariant marker for distinct refinements.
/// First shipped form is `#Invariant("value >= lo && value < hi")` before a
/// `distinct Int` declaration; sema normalizes it to proof-carrying bounds.
pub const ATTR_INVARIANT: &str = "Invariant";

/// D-MUSTUSE1 (c18iwxqx): `@MustUse` — marks a type, function, or method whose
/// result cannot be silently ignored as a bare expression statement (E0419).
/// Explicit discard uses `.drop("reason")` only (D-IGNORERET2, amended by
/// D-MARK-DISCARD1=A). Compile-time only; erases in codegen (I3).
pub const ATTR_MUST_USE: &str = "MustUse"; // D-MUSTUSE1

/// D-MIGRATE1 (ratified 2026-06-22): contextual keyword `migration` — introduces
/// a migration block that declares how a `@PublishedSchema` struct changed between
/// releases. Used as `migration TypeName { rename old -> new }`.
/// D-VALIDATE1 (ratified 2026-07-12, card #506): contextual keyword `validate`
/// — introduces the in-body `validate { … }` block inside a struct definition
/// (S82 in-body grammar). Rules are `check(cond, at: field, "msg")`
/// statements; sema resolves `field` as a bare sibling reference
/// (D-FIELDPOL1) and purity-checks `cond`/`msg` (reuses the `@Pre`/`@Post`
/// checker). Failing `check`s accumulate into `[FieldError]`; `decode<T>()`
/// runs the block automatically on a successfully shape-decoded value, and
/// `Type.validate(value)` runs it standalone. `Validate.over(s)` is the
/// use-site escape for rules needing outside context.
pub const KW_VALIDATE_BLOCK: &str = "validate"; // D-VALIDATE1

/// D-VALIDATE1: the builtin call name inside a `validate { … }` block —
/// `check(cond, at: field, "msg")` records one `FieldError { path, reason }`
/// when `cond` is false. Contextual: recognized as this builtin only inside
/// a `validate { … }` block; an ordinary function named `check` elsewhere is
/// unaffected.
pub const VALIDATE_CHECK_FN: &str = "check"; // D-VALIDATE1

pub const KW_MIGRATION: &str = "migration"; // D-MIGRATE1

/// D-MIGRATE1 (ratified 2026-06-22): contextual keyword `rename` inside a
/// `migration { }` block — declares that field `old` was renamed to `new`.
pub const KW_RENAME: &str = "rename"; // D-MIGRATE1

/// D-MIGRATE2A (ratified): `add f: T = default` inside a `migration { }` block —
/// declares a new field plus the default old records are read with.
pub const KW_ADD: &str = "add"; // D-MIGRATE2

/// D-MIGRATE2D (ratified): `remove f` inside a `migration { }` block — deletes a
/// field. The verb is `remove`, NOT `drop` (a `drop` op is taught back to this).
pub const KW_REMOVE: &str = "remove"; // D-MIGRATE2

/// D-MIGRATE2E (ratified): `change f: Old -> New [via { … }]` inside a
/// `migration { }` block — a field type change with an optional inline converter.
pub const KW_CHANGE: &str = "change"; // D-MIGRATE2

/// D-MIGRATE2E (ratified): the `via { expr }` clause that supplies the inline
/// converter for a `change` op (`change price: Int -> Usd via { (c) => Usd(c) }`).
///
/// D-EFF2 (ratified 2026-06-22): also the pass-through marker in a `#(via f)`
/// signature annotation — a function whose published effect set IS whatever the
/// callback parameter `f` carries (a tight pass-through that holds when the value
/// escapes, vs. the conservative flow-through default). Erased in codegen (I3).
pub const KW_VIA: &str = "via"; // D-MIGRATE2 / D-EFF2

/// D-MIGRATE2F (ratified): the rejected `reorder` verb — field reordering is not
/// a tracked breaking change and needs no migration. Kept only to teach.
pub const KW_REORDER_RETIRED: &str = "reorder"; // D-MIGRATE2F

/// D-MIGRATE1: the `drop` verb a user might reach for instead of `remove` — kept
/// only to emit the E0911 teaching error pointing at `remove`.
pub const KW_DROP_RETIRED: &str = "drop"; // D-MIGRATE2D

/// D-MIGRATE2C (ratified): `jet inspect schema` subcommand and its verbs. `status`
/// reports each `@PublishedSchema` type's pinned shape; `squash --before <ver>`
/// re-baselines snapshots to the current shape. There is NO `check` verb —
/// `jet build`'s E0910 is already the CI gate.
pub const CMD_SCHEMA: &str = "schema"; // D-MIGRATE2C
pub const SCHEMA_VERB_STATUS: &str = "status"; // D-MIGRATE2C
pub const SCHEMA_VERB_SQUASH: &str = "squash"; // D-MIGRATE2C

/// D-DBG1 (ratified 2026-06-19 = A): `jet debug <file>` — the dedicated
/// source-level debugger entry verb, parallel to `jet run`/`jet test`. The
/// editor launches the same command.
pub const CMD_DEBUG: &str = "debug"; // D-DBG1

/// D-DBG3 (ratified 2026-06-24 = A): the in-session `jet debug` command
/// surface. The prompt is `(jet)`; the step verbs are lldb-familiar with
/// single-letter aliases. Only Jet frames/lines/safe-locals are shown by
/// default (I2; D-DBG2 `--raw-frames` is the expert carve-out).
pub const DBG_PROMPT: &str = "(jet)"; // D-DBG3
pub const DBG_STEP: &str = "step"; // D-DBG3 (alias `s`)
pub const DBG_NEXT: &str = "next"; // D-DBG3 (alias `n`)
pub const DBG_CONTINUE: &str = "continue"; // D-DBG3 (alias `c`)
pub const DBG_FINISH: &str = "finish"; // D-DBG3 (alias `f`)
pub const DBG_BREAK: &str = "break"; // D-DBG3 (alias `b`): set a line breakpoint
pub const DBG_LIST: &str = "list"; // D-DBG3 (alias `l`): show source around `here`
pub const DBG_PRINT: &str = "print"; // D-DBG3 (alias `p`): show one local's value
pub const DBG_LOCALS: &str = "locals"; // D-DBG3: dump all locals in the frame
pub const DBG_BACKTRACE: &str = "backtrace"; // D-DBG3 (alias `bt`): the Jet call stack
pub const DBG_HELP: &str = "help"; // D-DBG3 (alias `h`): list the verbs
pub const DBG_QUIT: &str = "quit"; // D-DBG3 (alias `q`): end the session (E2204)

/// D-MIGRATE1 (ratified 2026-06-22): subdirectory under the project `.jet/`
/// managed folder where schema snapshots are stored. Full path is
/// `<project_root>/.jet/cache/schema/<TypeName>.snapshot`.
pub const SCHEMA_CACHE_SUBDIR: &str = "cache/schema"; // D-MIGRATE1

/// S2/D-MEM1 (was c129/D-CAP4/D-CAP6/D-CAP8): subdirectory under the project
/// `.jet/` managed folder where public-fn signature snapshots are stored.
/// Full path is `<project_root>/.jet/cache/api/<package>.api`. Written at
/// `jet registry publish` time, unconditionally, for every library target; read by the
/// local pre-publish SemVer gate (E1218). Committed — it is a durable
/// interface contract, not a build artifact.
pub const API_CACHE_SUBDIR: &str = "cache/api";

/// D-DETACH1 (ratified; D-DETACH1 = A): consumes a Task handle without joining
/// — fire-and-forget daemon semantics. Main may return while the task runs.
/// Only valid on owned tasks; capturing a `view` borrow is rejected at spawn
/// time (E1102) and flagged again at the detach site (E1103).
pub const TASK_DETACH: &str = "detach"; // D-DETACH1

/// D-REPRC1 (ratified; D-REPRC1 = B): `#Layout(…)` struct attribute — controls
/// the memory layout of the generated Rust struct. `#Layout(c)` stamps
/// `#[repr(C)]` for C interop. Field order is preserved as written.
/// Growable fields (`[T]`, `Map`, `String`) are rejected (E1104).
/// PascalCase per D-MARKERCASE1=A.
pub const ATTR_LAYOUT: &str = "Layout"; // D-REPRC1 / D-MARKERCASE1
/// D-REPRC1: the C-compatible layout variant — `#layout(c)` → `#[repr(C)]`.
pub const LAYOUT_C: &str = "c"; // D-REPRC1
/// D-REPRC1: reserved layout variants — parse-and-error until their milestones ship.
pub const LAYOUT_PACKED: &str = "packed"; // D-REPRC1 (reserved)
pub const LAYOUT_ALIGN: &str = "align"; // D-REPRC1 (reserved)
/// D-SOA1 / D-SOA2A=C (implemented): the struct-of-arrays layout variant —
/// `#layout(columnar) struct S` stores a `[S]` collection column-per-field.
/// Whole-struct only in v1 (D-SOA2B); the partial form `#layout(columnar: …)`
/// is rejected (E1109) and the per-container prefix `columnar [T]` is reserved
/// (D-SOA2C, E1107).
pub const LAYOUT_COLUMNAR: &str = "columnar"; // D-SOA1 / D-SOA2A

// ── Serde derive markers + attributes (D-SERDE2–8, D-ENC1; bracket form D-ATTR2) ──
// Derive markers (PascalCase per D-CASING1, written `@[…]` before a struct/enum,
// D-MARKERMOVE1=B): `@[Codable]` derives BOTH directions (sugar for `@[Encode,
// Decode]`); `@[Encode]` is write-only; `@[Decode]` is read-only. Owner (D-SERDE4 = B,
// modified): the
// collapsed umbrella is `Codable`, with `Encode`/`Decode` as the one-way markers.
pub const ATTR_CODABLE: &str = "Codable"; // D-SERDE4
pub const ATTR_ENCODE: &str = "Encode"; // D-SERDE4
pub const ATTR_DECODE: &str = "Decode"; // D-SERDE4
                                        // D-MARKERMOVE3 (B, ratified 2026-07-02): the other built-in derive markers
                                        // that join Codable/Encode/Decode on the contract plane (`@`). `Debug` is
                                        // NOT one of these — D-MARK-DEBUG1=A retired the explicit derive spelling
                                        // outright (E0922, Traits.rs); it auto-derives instead (S55). User derives
                                        // (`derive T.Wire { … }`, applied as `#[Wire]`) stay `#` — the built-in/user
                                        // line is the `@`/`#` plane line.
pub const ATTR_SUMMARIZE: &str = "Summarize"; // D-MARKERMOVE3
pub const ATTR_COMPARABLE: &str = "Comparable"; // D-MARKERMOVE3
                                                // Per-field attributes (D-SERDE5 = A), written `#[…]` before a field.
pub const ATTR_RENAME: &str = "Rename"; // D-SERDE5  #[Rename("wire_key")]
pub const ATTR_SKIP: &str = "Skip"; // D-SERDE5  #[Skip]
pub const ATTR_DEFAULT: &str = "Default"; // D-SERDE5  #[Default] / #[Default(expr)]
pub const ATTR_FLATTEN: &str = "Flatten"; // D-SERDE5  #[Flatten]
                                          // Container attributes (D-SERDE3/7/8), written `#[…]` before a struct/enum.
pub const ATTR_RENAME_ALL: &str = "RenameAll"; // D-SERDE3  #[RenameAll(camel)]
pub const ATTR_DENY_UNKNOWN_FIELDS: &str = "DenyUnknownFields"; // D-SERDE8
pub const ATTR_TAG: &str = "Tag"; // D-SERDE7  #[Tag("type")] internal tagging
pub const ATTR_UNTAGGED: &str = "Untagged"; // D-SERDE7  #[Untagged]
                                            // D-SERDE3 (= C) RenameAll casing keywords — closed typed menu, own-case args.
pub const RENAME_ALL_CAMEL: &str = "camel"; // D-SERDE3
pub const RENAME_ALL_SNAKE: &str = "snake"; // D-SERDE3
pub const RENAME_ALL_PASCAL: &str = "pascal"; // D-SERDE3
pub const RENAME_ALL_KEBAB: &str = "kebab"; // D-SERDE3
pub const RENAME_ALL_SCREAMING: &str = "screaming"; // D-SERDE3

// ── Maturity metadata values (D-MARK-META1=B, ratified 2026-07-12) ──────────
// Closed values for `#Meta(maturity: .Experimental | .Tested | .Hardened)`.
// They are not standalone markers and therefore are absent from marker-plane
// registries. No sema/codegen effect.
pub const ATTR_EXPERIMENTAL: &str = "Experimental"; // D-MARK-META1
pub const ATTR_TESTED: &str = "Tested"; // D-MARK-META1
pub const ATTR_HARDENED: &str = "Hardened"; // D-MARK-META1

// ── Explicit discard (D-IGNORERET2=A, ratified 2026-06-28; amended by
// D-MARK-DISCARD1=A, ratified 2026-07-11, card #498) ─────────────────────────
// `.drop("reason")` — method-style terminal that silences E0402 for a
// fallible or @MustUse result. It is now the SOLE discard spelling; the
// `#Suppress(MustUse) { … }` lexical-scope form is retired outright
// (ordinary unknown-marker error — no ATTR_SUPPRESS registration).
pub const METHOD_DROP: &str = "drop"; // D-IGNORERET2 method; distinct from consume builtin

// ──────────────────────────────────────────────────────────────────────────────
// Canonical keyword/type/builtin tables (c44: single source of truth).
//
// These slices are the authoritative lists for the LSP, formatter, TextMate
// grammar, and any other consumer that needs to enumerate Jet's keyword surface.
// Add a word here (with its decision ID) rather than in each consumer.
//
// Rules for each list:
//   JET_KEYWORD_LIST  — real Jet keywords a user can type; FOREIGN_* teaching
//                       words must NOT appear here.
//   JET_TYPE_LIST     — built-in primitive / collection type names for completion
//                       and rename-guard. Only types a user writes in source.
//   IMPURE_BUILTINS   — bare-name builtins that write to I/O or read from stdin;
//                       used by both Sema/Purity and Comptime/Purity.
// ──────────────────────────────────────────────────────────────────────────────

/// Canonical list of real Jet keywords for LSP completion and rename validation.
///
/// Every entry corresponds to a `KW_*`, `LIT_*`, or `BUILTIN_*` constant above.
/// FOREIGN_* teaching-error words must NOT appear here — they are recognized only
/// to emit a diagnostic, not valid syntax.
pub const JET_KEYWORD_LIST: &[&str] = &[
    // Core structure (S1, S18, D-VISDEFAULT2, S16, S50, U3)
    KW_FN,
    KW_PUB,
    KW_PRIV,
    KW_USE,
    KW_AS,
    KW_EXTERN,
    KW_MODULE,
    // Control flow (M1, S19, S23, M1/M2)
    KW_IF,
    KW_ELSE,
    KW_LOOP,
    KW_IN,
    KW_BREAK,
    KW_CONTINUE,
    KW_RETURN,
    // Types and declarations (M2, S30, S27, M2, S28, S55, S57, D-DIST1)
    KW_STRUCT,
    KW_ENUM,
    KW_ALIAS,
    KW_IMPL,
    KW_TRAIT,
    KW_TAG,
    KW_DERIVE,
    KW_CONST,
    KW_COMPTIME,
    KW_DISTINCT,
    // Schema migrations (D-MIGRATE1 / D-MIGRATE2)
    KW_MIGRATION,
    KW_RENAME,
    KW_ADD,
    KW_REMOVE,
    KW_CHANGE,
    KW_VIA,
    KW_COPY,
    // Ownership / borrow keywords (S10, M2). D-MEM1 retired KW_MUTATE/KW_MOVE
    // in favor of the `&`/`^` sigils — they live only as teaching errors
    // (E0056/E0057) now, so they are NOT in the keyword list. The retired
    // `ref[label]` field spelling (once taught via KW_STORED) is gone
    // outright with stored-reference fields (D-MEM1/S3) — `ref` is an
    // ordinary identifier again.
    KW_SELF,
    // Memory / expert tier (S58, D-REGION1, D-CTX1, D-TERM1, D-CTEFFECT1)
    KW_UNSAFE,
    KW_IMPURE,
    KW_TASKGROUP,
    CTX_BLOCK,
    // Transactions (D-TXN1–D-TXN4): `#Transact(name) { … }`
    KW_TRANSACT,
    // Schedule-as-code (D-SCHEDULE1, card #505): `#Task fn` — `#Every(…)`
    // stays out of this list, matching ATTR_TARGET/ATTR_META (paren-arg
    // config markers aren't bare completion words).
    KW_TASK,
    // Test / tooling (S43, S60, D-TOOL2, D-BENCH1)
    KW_TEST,
    KW_BENCH,
    KW_PURE,
    KW_TODO,
    // Taint tracking (D-TAINT1): value-fact tag + sanitizer modifier
    KW_TAINTED,
    KW_SANITIZER,
    // Typestate (D-STATE1 / D-STATE-DECL / D-STATE-REQ / D-STATE-TRANS)
    KW_STATE,
    KW_TRANSITION,
    KW_STATE_DECL,
    // Session/protocol types (D-PROTO1 / D-PROTO2)
    KW_PROTOCOL,
    PROTO_CLIENT,
    PROTO_SERVER,
    // In-body struct validation (D-VALIDATE1, card #506)
    KW_VALIDATE_BLOCK,
    VALIDATE_CHECK_FN,
    // Literals: boolean (S11), option (S32), result (S34), synthetic (M4)
    LIT_TRUE,
    LIT_FALSE,
    LIT_NULL,
    LIT_OK,
    LIT_ERR,
    KW_IT,
    // Binding sigils (SIGIL_BIND_IMMUT / SIGIL_BIND_MUT) are not words; omitted.
];

/// Canonical list of built-in type names for LSP completion and rename guard.
///
/// Only types a user writes in source. `Result` is the legacy fallible type
/// (S34) kept for teaching errors; it is intentionally excluded here since
/// `T ? E` is the current spelling.
pub const JET_TYPE_LIST: &[&str] = &[
    TYPE_INT,
    TYPE_FLOAT,
    TYPE_BOOL,
    TYPE_STRING,
    TYPE_VOID,
    TYPE_CHAR,
    TYPE_SHARED,
    TYPE_HASH_MAP,
    TYPE_BTREE_MAP,
    TYPE_DEQUE,
    TYPE_SET,
    TYPE_SORTED_SET,
    TYPE_PRIORITY_QUEUE,
    TYPE_LRU,
    TYPE_BIT_SET,
    TYPE_BYTE_BUFFER,
    TYPE_I8,
    TYPE_I16,
    TYPE_I32,
    TYPE_I64,
    TYPE_U8,
    TYPE_U16,
    TYPE_U32,
    TYPE_U64,
    TYPE_F32,
    TYPE_F64,
];

/// D-BUILDPROFILE1 (ratified 2026-06-25): the `build { }` block in `pkg.jet`
/// where named build profiles are defined. A profile is a `Build.{ optimize: … }`
/// value; blessed names `release`/`debug` carry built-in defaults; all others
/// must be declared here. Active profile is chosen by `--release` (sugar for
/// `--profile=release`) or `--profile=<name>` — never by ambient environment.
pub const MANIFEST_BLOCK_BUILD: &str = "build"; // D-BUILDPROFILE1

/// D-BUILDPROFILE1: the constructor type for a build profile value inside
/// `pkg.jet`'s `build { }` block — written as `Build.{ optimize: … }`.
pub const BUILD_CTOR: &str = "Build"; // D-BUILDPROFILE1

/// D-BUILDPROFILE1: the field inside a `Build.{ … }` profile value that sets
/// the optimization level.
pub const BUILD_FIELD_OPTIMIZE: &str = "optimize"; // D-BUILDPROFILE1

/// D-BUILDPROFILE1: blessed profile names — `release`, `debug`, and `ci`
/// carry built-in defaults and need no explicit declaration in `build { }`.
pub const BUILD_PROFILE_RELEASE: &str = "release"; // D-BUILDPROFILE1
pub const BUILD_PROFILE_DEBUG: &str = "debug"; // D-BUILDPROFILE1
pub const BUILD_PROFILE_CI: &str = "ci"; // D-BUILDPROFILE1

/// D-BUILDPROFILE1: optional fields inside `Build.{ … }` profile values.
pub const BUILD_FIELD_DEBUG_INFO: &str = "debug_info"; // D-BUILDPROFILE1
pub const BUILD_FIELD_SMALL: &str = "small"; // D-BUILDPROFILE1
pub const BUILD_FIELD_PANIC: &str = "panic"; // D-BUILDPROFILE1
pub const BUILD_FIELD_FEATURES: &str = "features"; // D-BUILDPROFILE1
pub const BUILD_FIELD_ENV: &str = "env"; // D-BUILDPROFILE1
/// D-BUILDSCOPE1=A: standing programmable-build authority grant.
pub const BUILD_FIELD_ALLOW: &str = "allow";

/// D-BUILDPROFILE1: `panic:` values for `Build.{ panic: … }`.
pub const BUILD_PANIC_ABORT: &str = "abort"; // D-BUILDPROFILE1
pub const BUILD_PANIC_UNWIND: &str = "unwind"; // D-BUILDPROFILE1

/// D-BUILDPROFILE1: `optimize:` levels for `Build.{ optimize: … }`:
/// `none` (no optimization, fastest compile), `basic` (opt-level=2, the
/// driver default), `full` (opt-level=3, maximum throughput).
pub const BUILD_OPTIMIZE_NONE: &str = "none"; // D-BUILDPROFILE1
pub const BUILD_OPTIMIZE_BASIC: &str = "basic"; // D-BUILDPROFILE1
pub const BUILD_OPTIMIZE_FULL: &str = "full"; // D-BUILDPROFILE1

/// Canonical list of impure builtins (write stdout/stderr or read stdin).
///
/// Used by Sema/Purity and Comptime/Purity to detect I/O calls inside
/// `pure fn` or comptime contexts. Both consumers must agree on this set;
/// having it here prevents silent divergence.
pub const IMPURE_BUILTINS: &[&str] = &[BUILTIN_PRINT, "eprint", "print", BUILTIN_INPUT, "read_all_input"];

// ── Marker family + syntax wave (ratified 2026-07-01, D-MARKERMOVE2/3 2026-07-02) ──
//
// D-MARKER-FAMILY1 (B): two-plane sigil law. `@` precedes a declaration and
// states a checkable contract about it; `#` instructs the compiler (modes,
// regions, effects, compile-time values) and may appear in type/expression
// position where `@` never does; `$` stays splice-only (D-CTMARKER1).
//
// D-CONTRACTCASE1 (A): PascalCase everywhere on the `@` plane — one casing
// rule for the whole language, extending D-CASING1's `#`-plane rule to `@`.
//
// D-MARKERMOVE1 (B): the fixed move list — `Pure`, `MustUse`, `Codable`,
// `Encode`, `Decode`, `Experimental`, `Tested`, `Hardened`, `PublishedSchema`,
// `Redact`, `Numeric` move from `#` to `@` (e.g. `@Pure` → `@Pure`), exact
// PascalCase spelling kept. `@Numeric` (D-DIST3) and the `@numeric` capability
// bundle (D-CAPBUNDLE1) are the same job (I8) and merge into one `@Numeric` —
// see `ATTR_NUMERIC` above; there is no separate bundle constant. Serde field +
// container markers (`#Rename`, `#Skip`, `#Default`, `#Flatten`,
// `#RenameAll`, `#DenyUnknownFields`, `#Tag`, `#Untagged`) are wire-format
// machinery, not promises, and stay on `#`.
//
// D-MARKERMOVE2 (B, ratified 2026-07-02): whole-move, no carve-out by
// position — `@Pure` is one spelling everywhere, including the type-position
// callback bound (`f: @Pure fn(T) -> U`). The plane law gains exactly one
// exception: a contract marker may prefix a function TYPE to state a bound;
// "declarations only" reads "declarations, plus contract bounds on function
// types".
//
// D-MARKERMOVE3 (B, ratified 2026-07-02): all built-in derive markers move,
// user derives stay `#`. `@Debug`, `@Summarize`, `@Comparable` join
// `@Codable`/`@Encode`/`@Decode` as contract-plane capability promises;
// `derive T.Wire { … }` bodies applied as `#[Wire]` remain `#` generation
// machinery — the built-in/user line IS the plane line.
use super::{
    BUILTIN_INPUT, BUILTIN_PRINT, CTX_BLOCK, KW_ALIAS, KW_AS, KW_BENCH,
    KW_BREAK, KW_COMPTIME, KW_CONST, KW_CONTINUE, KW_COPY, KW_DERIVE, KW_ELSE, KW_ENUM,
    KW_EXTERN, KW_FN, KW_IF, KW_IMPL, KW_IMPURE, KW_IN, KW_IT, KW_LOOP, KW_MODULE,
    KW_PRIV, KW_PROTOCOL, KW_PUB, KW_PURE, KW_RETURN, KW_SANITIZER,
    KW_SELF, KW_STATE, KW_STATE_DECL, KW_STRUCT, KW_TAG, KW_TAINTED, KW_TASK, KW_TASKGROUP, KW_TEST,
    KW_TODO, KW_TRAIT, KW_TRANSACT, KW_TRANSITION, KW_UNSAFE, KW_USE, LIT_ERR, LIT_FALSE,
    LIT_NULL, LIT_OK, LIT_TRUE, PROTO_CLIENT, PROTO_SERVER, TYPE_BIT_SET, TYPE_BOOL,
    TYPE_BTREE_MAP, TYPE_BYTE_BUFFER, TYPE_CHAR, TYPE_DEQUE, TYPE_F32, TYPE_F64, TYPE_FLOAT,
    TYPE_HASH_MAP, TYPE_I16, TYPE_I32, TYPE_I64, TYPE_I8, TYPE_INT, TYPE_LRU,
    TYPE_PRIORITY_QUEUE, TYPE_SET, TYPE_SHARED, TYPE_SORTED_SET, TYPE_STRING, TYPE_U16,
    TYPE_U32, TYPE_U64, TYPE_U8, TYPE_VOID,
};
