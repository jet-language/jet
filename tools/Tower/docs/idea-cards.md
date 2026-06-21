# Idea cards — for the owner to keep or discard

**What this is.** Three research rounds (6 source files, ~118 raw idea cards) were
mined for things Jet doesn't already have. Most of it turned out to be *already
ratified, already built, or already in the ballot* — the research mostly validates
bets we already made. This document strips out the dupes and leaves you the parts
that actually need your input.

**How to use it.** Read top to bottom and mark `Keep ☐` / `Discard ☐` on each item
in sections 2 and 3. Section 2 is the handful of genuinely strategic forks — where
your call changes direction. Section 3 is the new-but-smaller ideas, grouped by
theme, quick wins first. Section 4 is everything already handled (skim and trust).
Section 5 is the safety check that lets us delete the research files.

**Headline finding.** There is one real fork: **core reactivity** (section 2.1).
Board card c64 *rejects* reactivity as a runtime model, but the "replace
Swift/TypeScript" thesis from the newer research *requires* it. Everything else is
either a clean small decision or already decided. Net new ideas needing a call: **~20**.

---

## 2. Decisions / forks worth your attention first

These are where your input matters most — strategic or self-conflicting choices the
research surfaced.

### 2.1 The core-reactivity fork (the one real contradiction)
Board card **c64** + proposal P3 already decided: adopt the dataflow graph only as a
*tooling artifact* and an opt-in `std.reactive` library, and **reject** reactivity as
the core evaluation model (it collides with priorities #3/#4 and move semantics). But
the newer Swift/TS research builds its *entire* "replace everything" pitch — reactive
UI, motion-as-state, time-travel debugging — on reactivity living *in the runtime*.
You can't have both. **Trade-off:** core reactivity unlocks the whole replace-the-UI-
ecosystem story but reintroduces hidden machinery we explicitly ruled out. Everything
in 3-WebUI below is downstream of this one call.
`Keep core-reactivity stance as-is (library only) ☐ / Reopen for the UI thesis ☐`

### 2.2 Typed full-stack protocol ("replace TypeScript")
Describe a client/server protocol once (`protocol Orders { … }`) and the compiler
generates both sides, so a mismatched API can't ship — replacing the tRPC/Zod/two-
synced-stacks dance. A stub exists (**D-PROTO1**, deferred) but it's parked behind
linear types + typestate. **Trade-off:** strategically central to displacing
TypeScript, but a whole full-stack subsystem and post-v1.
`Keep (promote from deferred stub) ☐ / Discard ☐`

### 2.3 Checked integer overflow by default
`255 + 1` on a byte would trap instead of silently wrapping; experts opt into
`wrapping`/`saturating`. Not ratified today. **Trade-off:** classic safety win (Rust
ships it in debug); needs a small spelling decision for the escape hatch. Fits our
safe-by-default identity squarely.
`Keep ☐ / Discard ☐`

### 2.4 `Decimal` money type + float-for-money lint
A built-in exact base-10 number type so people stop using floats for currency
(`0.1 + 0.2 != 0.3`), plus a lint that flags float arithmetic on money. Flagged by
*two* separate research files as the single highest-value safety add. **Trade-off:**
prevents a notorious bug class at low syntax risk; adds a Core type.
`Keep ☐ / Discard ☐`

### 2.5 Injected Clock & Rng capabilities
Make `now()` and randomness *powers passed in*, not globals — real clock in prod, fake
in tests, so tests never flake on time or entropy. The mechanism (scoped capabilities,
D-SCAP1) is already ratified; this is modeling `Clock`/`Rng` as grantable values on top
of it. Research calls it "highest leverage for little syntax." **Trade-off:** natural
follow-on once the effect/capability engine lands; sequence after D-EFF1.
`Keep ☐ / Discard ☐`

### 2.6 `pub(package)` middle visibility tier
A visibility level between private and fully public (Rust's `pub(crate)`): visible
across this package, hidden outside. **Trade-off:** S18 deliberately kept visibility to
just two levels (private / `pub`) for simplicity. Adding a third is plausibly useful but
cuts against a decision we made on purpose — only worth it on real boilerplate evidence.
`Keep ☐ / Discard ☐`

### 2.7 Semantic-index query API (the one cross-file new idea)
A compiler-provided, queryable map of the program — "which type owns this method? list
every member of T; where can a balance go negative?" This appeared in *two* research
files independently (A3 = B23). The compiler already builds this internally for the LSP;
the new part is exposing it as a stable external API that third-party dev tools (impact
analysis, codemods) can ride. **Trade-off:** small core hook, high ecosystem leverage.
`Keep ☐ / Discard ☐`

### 2.8 In-file `namespace { }` sub-grouping
An optional block to group items inside one file without splitting it into a new file.
**Trade-off:** Jet's model is module = file = namespace; this adds C++-style surface for
a niche need and cuts against the simplicity ratchet (I8). Likely a skip unless users ask.
`Keep ☐ / Discard ☐`

---

## 3. New ideas to keep or discard

The ~20 genuinely new ideas not promoted above, deduped across all three files and
grouped by theme. Quick wins first within each group.

### 3-Safety — lints & safety nets (cheap, high-value)

**Ban `=` in conditions.** Make `if x = 5` a compile error (the classic typo for `==`).
One diagnostic, near-zero risk. *Worth confirming Jet's grammar even permits `=` there —
if not, this closes itself.*
`Keep ☐ / Discard ☐`

**Smell-detector lint pack.** Gentle warnings for code that looks right but isn't:
identical if/else branches, always-true conditions, comparing floats with `==`, unused
results. Each is a diagnostic + snapshot; extends our existing error-quality strength
with no new syntax. *High value, good momentum.*
`Keep ☐ / Discard ☐`

**Same-scope confusable-name lint.** Warn when two near-identical live names coexist
(`users` vs `user`, `l` vs `1`). Did-you-mean on *typos* already ships; this is the
in-scope-confusion variant. Small; bundle with the smell pack.
`Keep ☐ / Discard ☐`

### 3-Lang — language ergonomics

**Failure-aware comprehension.** A list/collection builder that auto-skips elements where
a step fails (filter-as-you-go) — a natural, beginner-friendly payoff of our existing
failure-as-control-flow (`?`). *The deep "logic programming" version is correctly a skip;
only this gentle slice is in scope.*
`Keep ☐ / Discard ☐`

**Structured-concurrency `scope` / nursery.** Concurrent tasks live in a lexical block
that can't exit until all children finish, with a `Context` for deadlines/cancellation.
A real upgrade over today's manual `spawn`/`join` + the dropped-handle warning (L1101).
Needs syntax; medium effort. *Conflict note: builds on, doesn't replace, the ratified
S53 concurrency surface.*
`Keep ☐ / Discard ☐`

**General refinement types.** Types carrying an extra constraint (non-empty list,
positive int) for more compile-time catches. The narrow array-length form `[T#N]` already
ships; the general version is a deferred stub (**D-REFINE1**) needing an SMT/proof layer.
*Heavy machinery; low priority unless explicitly asked.*
`Keep ☐ / Discard ☐`

### 3-Stdlib — library modules (mostly no syntax)

**Misuse-resistant crypto API.** A high-level `seal`/`sign` envelope (libsodium/Tink
style): "encrypt this blob with this key" returns authenticated ciphertext with nonce
handled, raw primitives demoted to the basement. The blueprint's "strongest opinion."
Layers over our existing `jet.crypto` primitives; no language syntax. *Prerequisite for
post-quantum below.*
`Keep ☐ / Discard ☐`

**Tiny utility fill-ins.** uuid (v4 + time-sortable v7), base64/hex encoding, arbitrary-
precision bigint, grapheme-cluster string iteration (so a family emoji counts as 1),
url/WebSocket gaps if any. Each is a trivial stdlib addition, no decision needed. *Bundle
as one "utilities" fill-in; low risk, low urgency.*
`Keep ☐ / Discard ☐`

**Stdlib API-design rubric.** Write the "ten ergonomic laws" (easy path = safe path, name
your boolean args, make illegal states not compile, scary names for dangerous ops…) as a
short stdlib-review checklist. Most are already Jet policy; this just pins them so new
modules stay consistent. *A style note, not a ballot item.*
`Keep ☐ / Discard ☐`

### 3-Tooling — dev tools (ride the semantic index)

**Impact / blast-radius analyzer.** Show what a change can actually affect downstream
("touch pricing → hits checkout, invoices, 2 reports"). Cheap and high-value *once the
semantic-index API (2.7) exists*; bundle with it.
`Keep ☐ / Discard ☐`

**Replayable codemod objects.** Refactors as named, shippable, reversible operations.
Nice-to-have dev tool; pairs with the LSP. Not urgent.
`Keep ☐ / Discard ☐`

**Structural-duplication lint.** Flag "you updated 3 of 4 copies" copy-paste drift. A
tooling project; park behind the lint packs above.
`Keep ☐ / Discard ☐`

**Write down the formal kernel.** Define a minimal Jet "kernel" + a desugaring map so
the language stays small as it grows. *Mostly restates our existing discipline (I5/I8);
the only new ask is producing the actual document. Cheap insurance, a process card not a
feature.*
`Keep ☐ / Discard ☐`

### 3-WebUI — the "replace Swift/TS" stack (all downstream of fork 2.1)

These only make sense if 2.1 reopens core reactivity. Treat as one strategic track, not
separate near-term cards.

**Reactive UI stack + ownable component kit.** Layered stack (reactivity → view model →
typed styling → headless behavior → copy-in-and-own kit → motion → app kit); bottom
layers are the compiler's job, the kit is shadcn-style not a locked theme. The heart of
the replace-SwiftUI/React bet.
`Keep ☐ / Discard ☐`

**Render-target abstraction.** UI targets an abstract renderer (web/native/embedded/TUI
as pluggable backends). Research flags this as *time-sensitive*: sketch the trait now or
marry one backend forever. *The one piece of this track worth a cheap early card even if
the UI itself waits.*
`Keep ☐ / Discard ☐`

**Supporting UI payoffs.** Typed styles (CSS that won't compile with a typo or wrong-unit
value), accessibility on by default, motion as derived reactive state. All downstream of
core reactivity; bundle with the stack.
`Keep ☐ / Discard ☐`

**Web backend (JS DOM + WASM).** Emit views as plain JS DOM ops, send only logic to WASM
— the concrete delivery path for "replace TypeScript." Heavy (two backends).
`Keep ☐ / Discard ☐`

**App backend (FFI then own-renderer).** FFI to native widgets first for true native
look, add a Skia-style own-renderer later. The most expensive frontier.
`Keep ☐ / Discard ☐`

**JS/npm + Swift interop, day one.** Calling into existing ecosystems from day one is
flagged as the single most adoption-critical decision (only C FFI is on the roadmap
today). Far out, but worth registering so the architecture stays interop-friendly now.
`Keep ☐ / Discard ☐`

### 3-Horizon — long-horizon / research-grade (flag, don't card yet)

**Serde-style unified Serialize.** One derive, every format (JSON/CSV/TOML/binary)
through one mechanism — the blueprint's highest-leverage idea. *Blocked on user-defined
derives (S56, Epoch 3); a north-star, not actionable now. Would unify the existing
c89/c90 typed-row work.*
`Keep ☐ / Discard ☐`

**Post-quantum crypto by default.** Hybrid X25519+ML-KEM behind the safe API so the
ecosystem upgrades with zero call-site edits. Real threat ("harvest now, decrypt later",
NIST 2030), not v1-blocking. *Sequence after the misuse-resistant API.*
`Keep ☐ / Discard ☐`

**Embedded "one library, swappable I/O engine."** Ring-layer the stdlib (no-heap core ⊂
alloc ⊂ std) so the same code runs from a server to a 32 KB microcontroller. Aligns with
our no-async-color design and existing `--freestanding` work, but a major architecture
track. *The ring-layering question is the concrete first decision.*
`Keep ☐ / Discard ☐`

**Living-graph value provenance.** Every value can explain its own origin ("why is total
7?"); variables keep history. Research calls building this into the runtime "a genuine
moat." Large, research-y, distinct from logging. *Note: this is the same engine as the
core-reactivity fork (2.1) — keep them consistent.*
`Keep ☐ / Discard ☐`

**Transactional global state north-star.** Verse's "global state just works,
transactionally, auto-distributed across machines." *Conflicts with our no-GC,
transpile-to-Rust foundation; a someday/maybe note, nothing more.*
`Keep ☐ / Discard ☐`

---

## 4. Already covered — nothing to decide

Skim and trust. These were all considered and live somewhere already. (A few dedup
claims re-verified against `syntax-decisions.md` and `decision-ballots.md` — confirmed.)

### Ratified / implemented
| Idea | Where it lives |
|---|---|
| Failure as control flow (`?` prop, `T ? E`, `??`, no null) | S7, S34, S71, S35 |
| No null; forced-handle `T?` optionals, `?.` chaining | S35, S71 |
| Inline methods (write methods in the type body) | S27 (verified) |
| Trait impls in-type or top-level | S28 (verified) |
| File = module, folder = package, no `mod` boilerplate | S16, D-MOD1–4 |
| Private by default, `pub` to export | S18 (verified) |
| `as` import aliasing | S16 |
| Prelude (`print`/`panic`/`require` builtins) | S9, S36 |
| Units of measure as a tag (`#unit(usd)`, `9.99.usd`) | D-UNIT1 (verified) |
| Distinct types / units bundle | D-DIST1/2/3, c23, c68 |
| Taint tracking (`#tainted` + sanitizers → E0721) | D-TAINT1 opt A (verified) |
| Capability vocabulary (`view`/`edit`/`take`/`share`) | D-CAP1–6 (verified) |
| Scoped capabilities (`#grant(fs){…}`, RAII-revoked) | D-SCAP1 (verified) |
| Linear / must-use values | D-LIN1 (`#SingleUse`/`#MustUse`) |
| RAII cleanup | S63 |
| Reject dual indentation/brace syntax | S1 |
| Friendly compiler errors (code + what/why/fix + snapshot) | invariant I4 |
| First-class "unknown" (loading/pending) — just an enum | S30, S32 |
| Safety ladder (beginner→working→expert) | invariant I1 |
| Two-tier library (frozen Core + versioned ring) | core-library.md, E2-M2 |
| No-color concurrency (`spawn`/`join`/channels) | E2-M1, S53 |
| Errors as values w/ cause chain + `?` conversion | D-ERR-CONV |
| Composable Reader/Writer/Iterator + streaming I/O | E2-M7 |
| First-party ring (http/regex/csv/toml/log/time/crypto/archive/db) | E2-M9/M10/M12 |
| Linear-time ReDoS-safe regex (only engine) | D-REGEX1, c79 |
| TLS verified by default | E2-M10 |
| Safe subprocess (arg list, never a shell string) | core.process |
| Split fast-PRNG vs secure RNG | core.random, jet.crypto |
| Editions/epochs for evolution | E2-M2 |
| Structured logging / tracing / metrics | jet.log, E2-M12, D-LOGFMT1/c92 |
| Path objects + atomic write + dir-walk | E2-M7, D-LSDIR1/c88 |
| Arbitrary-precision / explicit allocator at boundary | c05, c26 (sidequests) |

### In ballot / on the board (live decisions, already queued for you)
| Idea | Card / decision |
|---|---|
| Inferred effect system (annotate at boundaries) | D-EFF1 / c66 — *the linchpin* |
| Scoped auto-rollback (`#transact`, `Rollback` trait) | D-TXN1 / c72 |
| Safe schema migration (breaking change won't compile) | D-MIGRATE1 / c73 / E0901 |
| Typestate (order-of-events, "charge before ship") | D-STATE1 / c71 |
| Reactive / dataflow (as tooling + opt-in library) | c64 / P3 — *see fork 2.1* |
| Content-addressed defs (cache layer only) | c63 / P2 |
| Doctests | D-TEST4 / c51 / E2-M11 |
| Property-based testing + shrinking | D-TEST1 / c51 / E2-M11 |
| HTTP routing + middleware | D-ROUTE1 / c83 |
| Declarative CLI arg parsing (clap-shaped) | D-ARGS1 / c91 |
| Typed CSV row / typed JSON output | D-CSVROW1/c89, D-JSONOUT1/c90 |
| Strict-vs-lenient JSON modes | c10 |
| Sized floats F32/F64 | c93 |
| Task-detach idiom | D-DETACH1 / c84 |

### In ballot — deferred stubs (captured, parked behind prerequisites)
| Idea | Stub |
|---|---|
| Information-flow / compliance ("EU data can't leave EU") | D-IFC1 |
| Deterministic record-and-replay | D-REPLAY1 |
| Reversible computation / solve-for-input | D-REVERSE1 |
| Budgets as types (time/memory caps break the build) | D-BUDGET1 |
| Formal verification / proof integration | D-VERIFY1 |
| Effect prohibitions (`#(no_net)` propagation) | D-PROP1 |
| Time-varying roles | D-ROLE1 |

### Already expressible / pure library — no language work
TTL/expiring values, schema→generate-everything framework, honest-numbers (uncertainty)
type, adaptive runtime, latency-budget context value, approximate-algorithm library,
auto-instrumentation (rides D-EFF1), bare `undo` keyword (extra surface — covered by
D-TXN1). All marked library-able by the research's own test.

### Flagged as conflicting with a ratified rejection — *not* silently adopted
| Idea | Conflicts with |
|---|---|
| `pub use` re-export facades | S16 rejected re-export chains (verified) |
| Brace-selective imports (`use m {X,Y}`) | S16 rejected selective forms |
| Glob imports (`use a::*`) | not in S16; would need a lint-gated owner call |
| `extend` blocks (add inherent methods from afar) | redundant with S27/S28; I8 |
| Auto-parallelize sequential code | collides with zero-hidden-machinery priority #3 |

---

## 5. Coverage map

Every substantive idea in each of the 6 research files is captured above — as a new
card, a fork, or an already-covered row.

| Research file | Coverage |
|---|---|
| `jet-borrowing-from-verse.md` | Failure-as-control-flow, effect system, auto-rollback, structured concurrency, dual-syntax reject → already-covered. Formal kernel, failure-aware comprehension, global-state north-star → new cards (3-Lang / 3-Horizon). **100%** |
| `jet-vs-swift-typescript.md` | Migration gate, units, taint, capabilities, friendly errors → already-covered. Core reactivity → fork 2.1; typed protocol → fork 2.2; refinement types, UI stack, typed styles, a11y, motion, render-target, web/app backends, interop, auto-parallel → new cards (3-WebUI) or flagged-conflict. **100%** |
| `jet-code-organization.md` | Inline methods, file=module, private-by-default, aliasing, prelude → already-covered. Semantic index → fork 2.7; `pub(package)` → fork 2.6; `namespace {}` → fork 2.8; dossier/outline views → ride semantic index; `extend`/`pub use`/glob/brace imports → flagged-conflict. **100%** |
| `language-ideas-core-vs-library.md` | Living graph, effects, taint, scoped caps, units, IFC, linear, typestate, migration, txn, replay, reverse, protocol, refine, budget, verify, prohibitions, content-addressed, doctests, fuzz → already-covered (carded/balloted). Semantic-index query → fork 2.7 (= A3); blast-radius, codemods, structural-dup → 3-Tooling; auto-parallelize → flagged-conflict; TTL/schema-gen/honest-numbers/adaptive/deadlines/approx/auto-trace → library, no work. **100%** |
| `ideal-stdlib-blueprint.md` | Two-tier lib, composable interfaces, no-color concurrency, value-errors, safe defaults, observability, editions, collections, fmt, time, random, io/fs, process, regex, json/csv/toml, db(sqlite), property test, cli, arena → already-covered. Decimal → fork 2.4; structured-concurrency scope → 3-Lang; misuse-resistant crypto → 3-Stdlib; post-quantum → 3-Horizon; serde-unified → 3-Horizon; embedded ring-layering → 3-Horizon; ergonomic-laws rubric, uuid/base64/bigint/grapheme/url/ws fill-ins → 3-Stdlib. **100%** |
| `jet-bug-prevention-field-guide.md` | Bad-states-impossible bundle, no-null, capabilities, effects, taint, must-use, schema-drift, examples=tests, budgets, safety ladder → already-covered. Injected Clock/Rng → fork 2.5; checked overflow → fork 2.3; Decimal+money-lint → fork 2.4 (= A24); smell lints, `=`-in-condition, confusable-name, structural-dup → 3-Safety/3-Tooling; living-graph provenance → 3-Horizon; out-of-bounds prove-in-range → niche expert tier (noted, defer). **100%** |

**NOT yet captured — do not delete the source until resolved:** none. Coverage is
100%; the 6 research files are safe to delete.
