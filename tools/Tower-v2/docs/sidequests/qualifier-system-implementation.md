# Plan: Compile-time effect boundary (D-CTEFFECT1) + qualifier-system context

**Status:** planned. D-CTEFFECT1 ratified 2026-06-25 (option A), not yet implemented
(c157). The effect-tag machinery it reuses (D-EFF1/EFF2/EFF3) is built; the
`#Unsafe` audited-gate pattern it parallels is built; the comptime purity wall it
formalizes (D-CTCORE1, D-CTIO1) is built. The qualifier-taxonomy work that
originally lived in this file (D-QUAL1/QUAL2) is partly landed and tracked at the
end. Gates D-BUILDPROFILE1 (c159) hermeticity; the `comptime { }` block of
D-CTMARKER1 (c162) runs inside these tiers.

## Goal

Formalize the build-time / `comptime` effect surface into three tiers and open
Tier 2 behind an explicit, audited, CI-hermetic gate:

- **Tier 0 — pure** (always on): the D-CTCORE1 whitelist + pure Jet. No gate.
- **Tier 1 — hashed-reproducible**: effects whose *input* is hashed into
  `.jet/lock` so the build stays byte-reproducible — `@embed`, `find`,
  `fetch(url, sha256:)`. No gate; recorded, not blocked.
- **Tier 2 — ambient / non-deterministic**: every other effect (ambient `env`,
  wall-clock `time`, `random`, un-pinned `net`, `exec`). Allowed only with BOTH
  the `#Impure("reason")` audited gate in source AND `--allow-impure` at build,
  so CI is hermetic by default unless an expert opens it.

Today the comptime purity wall (D-CTCORE1) flatly rejects all Core I/O at comptime
(E0958) with `@embed`/`embed_bytes` as the one blessed exception. D-CTEFFECT1
replaces that binary wall with the tier model: Tier 1 becomes recorded-reproducible
and Tier 2 becomes reachable-but-gated.

## Current state (verified, file:line)

**Effect-tag system — built (D-EFF1/EFF2/EFF3).** `Source/Sema/Effects.rs`:
- Vocabulary `enum Effect { Net, Fs, Io, Db, Time, Rand, Env, Exec, Log, Gpu }`
  (`Effects.rs:22`), PascalCase per D-CASING1; `Effect::parse`/`name` (`:37`,`:53`).
  (The card's `#(fs,net,exec)` shorthand maps to `Net`/`Fs`/`Exec`.)
- Per-function `EffectSummary` (direct/edges/maximal/regions/callback_obligations,
  `:259`); whole-program fixpoint `solve` (`:334`); `core_effect(module, method)`
  maps Core calls to effects (`:185`), `builtin_effect` for ambient builtins
  (`:248`, keyed off `Syntax::IMPURE_BUILTINS`).
- Boundary diagnostics: E0740 over declared `#(…)` bound (`:538`), E0745
  `#Pure`+`#(…)` contradiction (`:902`), E0741/E0712 `#Caps`/`#grant` region
  overflow (`:592`,`:799`), E0711 capability escape (`:828`), E0747 callback bound
  (`:513`), E0742 trait-method bound (`:855`), E0119 unknown effect (`:842`).
- Effects are **fully erased in codegen** (I3) — compile-time proof only.

**`#Unsafe` audited gate — built (D-UNSAFE2), the pattern `#Impure` parallels.**
- `Syntax::KW_UNSAFE = "Unsafe"` (`Source/Syntax.rs:172`). `ATTR_AUDIT` is retired
  (`Syntax.rs:524`) — the reason is an inline string argument, not a separate
  `@audit`.
- AST `Stmt::Unsafe { audit: Option<String>, body, span }` (`Source/AST.rs:1384`);
  parsed at `Source/Parser/Statements.rs:78`.
- Reason required: **L3101** when missing (`Source/Sema/CheckerCore.rs:1265`,
  message `#Unsafe("why this is safe") { … }`).
- `#Unsafe fn` body is itself an audited region (`Source/Sema/Registration.rs:67`,
  `:1351`). Low-level op outside a gate → E3101 (`CheckerCoreLib.rs:1878`).

**Comptime surface — built (M9.5, D-CTCORE1, D-CTIO1).** `Source/Comptime/`:
- Tree-walking interpreter (`mod.rs`, `Interpreter.rs`); entry `evaluate` /
  `evaluate_with_imports` (`mod.rs:48`,`:62`). Fuel budget (`Interpreter.rs:21`).
- **Purity wall**: `Purity.rs:check_purity` walks the call graph and rejects the
  first impure call with its path → **E0951** (`Purity.rs:15`); impure = an
  `IMPURE_BUILTINS` name or an `extern`. `@embed`/`panic`/`require` allowed.
- **D-CTCORE1 whitelist** in `Comptime/Methods.rs:362` (`core.math`, `core.string`
  only; grows with tests); dispatched via `eval_whitelisted_core` (`:386`).
  Non-whitelisted I/O Core call at comptime → **E0958** (`docs/spec/diagnostics.md:300`).
  `core_imports` (alias→module) is threaded from each file's `use` decls
  (`Sema/Registration.rs:371`, `Sema/Bundle.rs:493`).
- **D-CTIO1 `@embed`** (Tier-1 candidate, already built): `BUILTIN_EMBED_FILE` /
  `BUILTIN_EMBED_BYTES` (`Syntax.rs:636`); `eval_embed_file`/`eval_embed_bytes`
  (`Methods.rs:234`,`:256`). Path must be a string literal, no `..`-escape
  (**E0957**), missing/unreadable → **E0955**. **Note:** `@embed` bakes the file
  bytes into the binary but does **not** currently record an input hash into
  `.jet/lock` — that recording is new D-CTEFFECT1 work.
- `comptime if` exists (`Stmt::ComptimeIf`); a `comptime { }` execution block
  does **not** yet exist (D-CTMARKER1=C, c162) — when it lands it runs inside
  these tiers.

**`find` — built as U4 import discovery, not yet effect-tiered.** `find("./path")`
auto-discovers `.jet` modules (U4; `imports: find(…)`), validated by **E0969**
(`docs/spec/diagnostics.md:309`); resolved in the loader/jetpack layer
(`Source/Loader.rs`, `Source/Jetpack/`). Its discovered-file set is **not** hashed
into the lock today. (See Open Owner-Q on whether `find` here means this directive.)

**`fetch(url, sha256:)` — does not exist.** Today `jet fetch` is *package* fetch
(`Source/Fetch.rs`, git-subprocess only, no compiler HTTP); a comptime
`fetch(url, sha256:)` Tier-1 builtin is unbuilt.

**`.jet/lock` — built for the package graph, no build-effect section.**
`Source/Lock.rs`: `UNIFIED_LOCK_FILE = ".jet/lock"` (`Syntax.rs:1121`), v1 schema
(`LockFile { version, packages, root_dependencies }`, `Lock.rs:50`). Per-package
SHA-256 `fingerprint` (`Lock.rs:28`); hashing via `Source/SHA256.rs`
(`sha256_hex`, `tree_hash`). No section recording Tier-1 build-effect inputs yet.

**CLI flags — ad-hoc, no `--allow-impure`.** `--locked` is parsed by a raw
`jet_argv.iter().any(...)` in `Source/main.rs:306` and declared in `CLI.rs:82`
(`FlagSpec`). No `--allow-impure`, no `--profile`/`--release` wiring yet (the
latter is D-BUILDPROFILE1 / c159).

**Diagnostics range.** Comptime band E0951–E0958 used; **E0959 is free**, then
E0960+ are jetpack codes (`docs/spec/diagnostics.md:293–309`). Lint **L3101** used;
**L3102 free**.

## Decision (ratified — verbatim intent)

D-CTEFFECT1 = A (`docs/spec/syntax-decisions.md:2941`). Three tiers for
build-time/`comptime` code: Tier 0 pure (always on); Tier 1 hashed-reproducible
(`@embed`, `find`, `fetch(url, sha256:)`) recorded in `.jet/lock`; Tier 2
ambient/non-deterministic gated behind BOTH `#Impure(reason)` AND `--allow-impure`,
so CI is hermetic by default. Reuses the `#Unsafe` audited-gate pattern and the
effect-tag machinery. **Owner add-on:** a project build file may alias/relax the
impure gate (a per-project config knob that drops the `--allow-impure`
requirement). **Gate name `#Impure`** is the default — alternatives `#Ambient` /
`#NonHermetic` / `#Effectful` / `#BuildEffect` / `#Untracked` were offered and are
confirmable; note them but do **not** ballot. Rejected: warn-only (B), pure-only
(C, denies the expert), Jai-ungated (D, footgun default).

## Implementation (staged)

Build right the first time, end-to-end. Each stage ships its own example + golden
(I5) and ui snapshot (I4); no stage is a placeholder.

**Stage 1 — tier classification of comptime ops.**
Add a single source of truth that classifies every comptime-reachable operation
into Tier 0/1/2. Extend `Comptime/Purity.rs` (or a new `Comptime/Tiers.rs`) so the
call-graph walk no longer answers a boolean pure/impure but a `Tier`:
- Tier 0: D-CTCORE1 whitelist + pure Jet (current "pure" verdict).
- Tier 1: `@embed`/`embed_bytes`, `find`, `fetch(url, sha256:)`.
- Tier 2: any other ambient Core call — classify by reusing `Effects::core_effect`
  / `builtin_effect` (ambient `Env`/`Time`/`Rand`/`Net`/`Exec`/`Fs`/`Io`). The
  comptime tier of a Core op is derived from its `Effect`, keeping one mechanism (I8).
Replace the E0958 hard wall: a Tier-2 op is now *conditionally* allowed (Stage 3),
not unconditionally rejected.

**Stage 2 — Tier 1 hashing into `.jet/lock`.**
Add a build-effect-inputs section to the lock schema (`Source/Lock.rs`,
read+write+`--locked` verify), recording each Tier-1 input by content hash:
- `@embed`/`embed_bytes`: hash the embedded file bytes (`SHA256::sha256_hex`).
- `find`: hash the sorted set of discovered file paths (+ their tree hash).
- `fetch(url, sha256:)`: the user-pinned `sha256:` *is* the recorded hash; the
  fetch verifies the downloaded bytes against it (reuse the `verify_entry` /
  E1204 mismatch pattern from `Fetch.rs`). `--locked` rejects any drift (a changed
  embed/find/fetch input without a lock update) — this is what makes CI hermetic.
- `fetch(url, sha256:)` itself is new: define the comptime builtin + its
  `sha256:`-pinned signature; no compiler HTTP beyond the existing fetch path (I6).

**Stage 3 — Tier 2 `#Impure(reason)` gate, reusing `#Unsafe` machinery.**
- Syntax (I7): add `KW_IMPURE = "Impure"` to `Source/Syntax.rs` with the
  D-CTEFFECT1 id. Mirror the `#Unsafe("reason")` shape exactly: a block
  `#Impure("reason") { … }` and a `#Impure("reason") fn`.
- AST + parser: add `Stmt::Impure { reason: Option<String>, body, span }` (clone
  the `Stmt::Unsafe` plumbing in `AST.rs` and `Parser/Statements.rs`); the `fn`
  form rides the existing item-attribute path the way `#Unsafe fn` does
  (`Registration.rs:1351`). Thread it through every `Stmt` match arm that lists
  `Unsafe { body, .. }` (Captures.rs, State.rs, Taint.rs, Sema/Purity.rs,
  Effects.rs handle-escape walk, Comptime/Purity.rs `walk_stmt_exprs`).
- Sema: reason required → new lint **L3102** (clone L3101). A Tier-2 comptime op
  *outside* a `#Impure` region → new error **E0959** ("this … is a Tier-2 build
  effect; wrap it in `#Impure(\"reason\")`"). The `#Impure` region authorizes
  Tier-2 ops in its body the way `#Unsafe` authorizes low-level ops.

**Stage 4 — `--allow-impure` build flag.**
- Declare the flag in `Source/CLI.rs` (`FlagSpec`) and plumb it like `--locked`
  (`main.rs:306`) down to the comptime evaluator / build driver.
- Without `--allow-impure`, a `#Impure` region that actually performs a Tier-2 op
  → new error **E0959**'s sibling (a distinct code, e.g. **E0970**: "`#Impure`
  build effect needs `--allow-impure`; CI is hermetic by default") so the two
  failure modes (no gate vs no flag) read differently. Both carry the fix line.
- A `#Impure` region present but performing no Tier-2 op should warn (unused gate),
  matching the `#Unsafe`-no-op posture if one exists; confirm against current
  `#Unsafe` behavior before adding.

**Stage 5 — per-project relax knob (owner add-on).**
A project build file may drop the `--allow-impure` requirement. The home for this
is the `build { }` surface of D-BUILDPROFILE1 (c159) — e.g. a `Build.allow_impure:
true` field — which is not yet built. Land it with/after c159's `build {}` surface;
if c157 ships first, an interim field on the package manifest (`pkg.jet`) is
acceptable as long as it's the same knob c159 later owns (no second mechanism, I8).
When set, the build behaves as if `--allow-impure` were passed for that project.

**Stage 6 — diagnostics, examples, tests, docs.**
- Diagnostics (I4): register E0959 (Tier-2 op without `#Impure`), E0970 (`#Impure`
  without `--allow-impure`), L3102 (`#Impure` missing reason) in
  `docs/spec/diagnostics.md` with what/why/fix; ship a `tests/ui` snapshot for
  each. No snapshot → the diagnostic doesn't exist.
- Examples (I5): `examples/features/` example exercising Tier 1 (`@embed` + a
  pinned `fetch`) with its `expected/*.out`; a `tests/ui` case for Tier 2 gated +
  allowed. Golden enforces them.
- Tests: tier-classification unit tests; lock round-trip incl. the new build-effect
  section + `--locked` drift; CLI flag plumbing; a decision-drift test asserting
  D-CTEFFECT1 references resolve.
- Formatter: `#Impure("reason") { … }` / `#Impure fn` must round-trip with emission
  + a fmt STABILITY test (new syntax requires this — idempotence alone misses
  dropped tokens).
- Docs: spec.md gets the three-tier section; flip D-CTEFFECT1 in
  `syntax-decisions.md` to implemented and log `#Impure` in `Source/Syntax.rs` /
  the keyword table; note the confirmable gate-name alternatives without balloting.

## Sequencing / gates

- **Within c157:** Stage 1 → Stage 2 (Tier 1 hashing) and Stage 3 (Tier 2 gate)
  are independent and can land in either order; Stage 4 needs Stage 3; Stage 5
  prefers c159's `build {}` surface (interim manifest field otherwise); Stage 6
  closes out per stage as it lands (not at the end).
- **Gates c159 (D-BUILDPROFILE1):** build-profile hermeticity is defined *against*
  this tier model — c159 cannot claim "byte-identical binary per flag" until
  Tier 2 is gated and Tier 1 is lock-recorded. Build c157 before/with c159.
- **Relates to c162 (D-CTMARKER1):** the ratified `comptime { }` execution block
  runs inside these tiers; whichever lands first, the other must respect the tier
  classifier (one mechanism, I8). No duplicate purity logic.
- **Reuses, does not fork:** `#Impure` clones the `#Unsafe` gate plumbing and the
  `Effects::Effect` vocabulary classifies the tiers — do not introduce a parallel
  effect enum or a second audited-gate mechanism (I8).

## Open Owner-Q

1. **What is `find` in Tier 1?** The ratified text lists `find` beside `@embed`
   and `fetch` as a hashed Tier-1 effect, but the only `find` built today is the
   **U4 import-discovery directive** `find("./path")` (resolved in the loader, not
   the comptime interpreter; E0969). Two readings: (a) it *is* U4 import discovery,
   and Tier 1 means hashing the discovered-file set into `.jet/lock`; or (b) a new
   comptime directory-listing/glob builtin usable inside `comptime { }`. They build
   in different layers. Reading (a) is assumed in this plan — confirm, or scope (b).

(The `#(fs,net,exec)` casing in the card is shorthand; effects are PascalCase
`Net`/`Fs`/`Exec` per D-CASING1 — not a question. Gate name `#Impure` is baked per
the ratified text; alternatives are noted, not balloted.)

---

## Appendix: qualifier-taxonomy context (D-QUAL1/QUAL2)

This file originally planned the broader qualifier taxonomy. Reconciled to current
reality:

- **Traits vs tags vs effects** distinction is live in parser/sema/docs.
- **Effects** (originally "slice 4, blocked on D-EFF2/EFF3") are now **built** —
  D-EFF1/EFF2/EFF3 land in `Source/Sema/Effects.rs` (see Current state). The
  blockers cleared.
- **Still open:** erased *value tags* that carry no methods and participate in type
  checking, and *parameterized tags* (e.g. `#unit(usd)`). These remain unbuilt and
  keep their own verification (tag/trait parser snapshots; sema tests for tag
  erasure / non-dispatch; D-QUAL1/QUAL2 drift test). Package/manifest policy stays
  separate from expression/type semantics. Track these under their own card if they
  are not already; they are independent of D-CTEFFECT1.
