/// D-VERDICT-732-1: the sole prefix for applying a typed rule. A rule may target a
/// declaration, expression, or brace scope when that rule declares the target
/// legal. `@` is reserved for locations, addresses, and sources.
pub const RULE_PREFIX: &str = "#"; // D-VERDICT-732-1

/// D-PREPOST1 / D-CONTRACTCASE1: precondition contract on a function
/// signature — `#Pre(cond, "msg")`. The condition is a pure expression (same
/// checker as `#Pure`). Checked in every build by default; per-module
/// build-policy strip is the explicit opt-out.
pub const MARKER_PRE: &str = "Pre"; // D-PREPOST1
/// D-PREPOST1 / D-CONTRACTCASE1: postcondition contract — `#Post(cond,
/// "msg")`; `result` names the return value inside `cond`.
pub const MARKER_POST: &str = "Post"; // D-PREPOST1

/// D-COMPUTE-KERNEL-SURFACE1=B: explicit safe kernel declaration —
/// `#Kernel(.parallel) fn …`. The mode is intentionally a closed marker
/// argument, so adding another execution mode needs a new owner ruling.
pub const MARKER_KERNEL: &str = "Kernel"; // D-COMPUTE-KERNEL-SURFACE1

/// D-PERSIST1 / D-CONTRACTCASE1: dev-tier contract on a module-level
/// binding — the value survives `jet dev` hot reloads (identity = module
/// path + binding name). Inert in release builds.
pub const MARKER_PERSIST: &str = "Persist"; // D-PERSIST1

/// D-METHODMACRO1=A / D-CONTRACTCASE1: `#Inline fn` / `#Inline` method — a
/// soft hint that this function/method should be inlined (`#[inline]` in
/// codegen). Never rejected by sema; the compiler is free to ignore it.
/// Methods stay ordinary functions — no macro-rewrite hooks (D-METHODMACRO1).
pub const MARKER_INLINE: &str = "Inline"; // D-METHODMACRO1
/// D-CAPBUNDLE1 / D-CONTRACTCASE1: capability bundles on a nominal distinct
/// type — each re-exposes a curated slice of the base type's operations
/// while keeping nominal identity. Stackable. The `numeric` bundle merged
/// into `MARKER_NUMERIC` (`#Numeric`, D-MARKERMOVE1) — there is no
/// `MARKER_BUNDLE_NUMERIC` constant.
pub const MARKER_BUNDLE_COMPARABLE: &str = "Comparable"; // D-CAPBUNDLE1
pub const MARKER_BUNDLE_PRINTABLE: &str = "Printable"; // D-CAPBUNDLE1
pub const MARKER_BUNDLE_CODABLE_AS_BASE: &str = "CodableAsBase"; // D-CAPBUNDLE1

/// D-CLIFLAG1 / D-SHAPE-CLI1 (rides D-CONTRACTCASE1/D-MARKERMOVE1):
/// struct-level CLI derive marker — `#CLI`. A resolved `fn run(args: T)`
/// parameter type owns parsing, defaults, help, completion, validation, and
/// audit facts. The marker is optional because plain `fn run()` remains a
/// complete entry.
pub const MARKER_CLI: &str = "CLI"; // D-CLIFLAG1, D-SHAPE-CLI1
/// D-PATCH1 (card #181): struct-level derive — generates nested `T.Patch` with
/// `apply`/`diff`/`merge`, Codable by construction (Encode+Decode on Patch).
pub const MARKER_PATCHABLE: &str = "Patchable"; // D-PATCH1
/// D-CLIFLAG1: field help for CLI-derived arguments. D-TASKS-LIST1=A reuses
/// it only when the function marker group also contains `#Job`.
pub const MARKER_DOC: &str = "Doc"; // D-CLIFLAG1
/// D-CLI-POS1=A: field-level opt-out from positional filling on a `#[CLI]`
/// required value field — `#[Flag]`. Without it, required scalars fill from
/// bare argv in declaration order; with it, only `--field` is accepted.
pub const MARKER_FLAG: &str = "Flag"; // D-CLI-POS1
/// D-CLI-FIELD-MARKERS1=A: one-letter alias for a `#[CLI]` field.
pub const MARKER_SHORT: &str = "Short"; // D-CLI-FIELD-MARKERS1
/// D-CLI-FIELD-MARKERS1=A: environment fallback for a value `#[CLI]` field.
pub const MARKER_ENV: &str = "Env"; // D-CLI-FIELD-MARKERS1
/// D-CALLDUAL1=E: marks the first bare-read parameter of a free function as
/// the receiver for the equivalent `value.function(…)` spelling.
pub const MARKER_ROOT: &str = "Root";

/// D-CABI-PLATFORM1=A: per-function native calling-convention marker for C
/// declarations. C remains the implicit default; alternate ABIs never inherit.
pub const MARKER_ABI: &str = "ABI"; // D-CABI-PLATFORM1

/// D-LINTPOLICY1=A / D-DECIMAL1: per-site lint-suppression marker —
/// `#[allow(lint_name)]` on a struct or field (e.g. `#[allow(float_money)]`
/// silences the default-on money lint L0504). Deliberately lowercase: it
/// names a lint code, not a declaration-shaped feature, so it does not
/// follow the PascalCase marker convention. Struct/field site collection:
/// `collect_allow_markers` in `crates/jet-semindex/src/Build.rs`; the
/// float-money check itself: `allows_float_money` in
/// `crates/jet-foundation/src/Numeric.rs`.
pub const MARKER_ALLOW: &str = "allow"; // D-LINTPOLICY1

// D-MARKSIG1=A: marker rows live in `Policy::APPLIED_RULES`. Keep marker
// spelling constants here; do not add a second name/site/signature table.
