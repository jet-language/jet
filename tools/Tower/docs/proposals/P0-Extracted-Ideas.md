# Integrated Research Ideas

**Status:** Owner Review

- Coroutines
- Async/Await
- Modules that support generics so you can instantiate a module with a type
- Allow manual Defer
- Built in vectors & swizzling
- Consider "def" keyword for alias - or "alias"
- Full STDLIB/CoreLIB available in REPL
- Consider #builtin marker to allow non-use of module prefix on calls
- Since we have modules -> Consider everything public by default OR Jai approach, having a "#scope_file" where everything in the file above that point is public, everything below is private. 
- Component-wise arithmetic by default (for vectors, etc)
- fn TYPE.method () - syntax for adding external methods to a type
- $ as indicator for macros or comptime?
- Analyze vs Typescript - how can we be better
- Use Jai-style build system
- Cross platform native raylib builtin
- Consider how to improve using concepts from Zig build system
- Cleanup old std.mem syntax (Ptr<T>)
- Relook what labeled ref field in struct
- Relook original ownership model - outdated example
- Relook if switch statements -> 07_switch still uses || to check multiple patterns on the input var -> should be | with || for additional expressions alongside the pattern
- Jet fmt should not drop parenthesis used for visual/functional grouping
- Use Jai . operator for Enum "expansion" and maybe for implied constructors?
- Broad gated build-time I/O: allow comptime code to read env vars, hit the network, run a subprocess, or codegen at build time (Jai's #run / Zig @embedFile-plus territory), behind a sandbox + an auditable .jet/build-io.lock of every accessed path + cache-invalidation on change. Powerful (full build scripting without a separate build step), but it adds a supply-chain attack surface that the S26 "no ambient I/O at comptime" law was written to refuse — the Nim/Jai evidence shows un-auditable spread once it ships.

# A. From `jet-code-organization.md`

This file is about **where you write a type's methods** and **how modules/namespaces
work** — ergonomics, not new runtime power. Much of it is already settled.

### A2. `extend` blocks — add methods to a type from another file
**What it is.** A keyword to bolt extra methods onto an existing type elsewhere
(another file/module), for cases that can't live in the home definition.
**Source.** Layer 0 escape hatch, `extend Account { … }` / `extend Account: Serializable`.
**Status.** PARTIAL. Cross-type trait impls are ratified (S28: `impl Type: Trait`,
in-type or top-level). A distinct **`extend`** keyword for adding *inherent* methods
in another file is NEW — S27/S28 cover trait impls and same-file methods but not a
named "add methods from afar" construct.
**CEO note.** Mostly redundant with `impl`; only worth it if cross-file method
addition is a real pain — likely a skip under the simplicity ratchet (I8).

### A3. Semantic index as a stable query API (the one "must be core" piece)
**What it is.** A compiler-provided, queryable map of the program — "which type owns
this method? list every member of T with its source location." Every viewer tool
below needs it; only the compiler can build it reliably.
**Source.** Part A, "The one piece that must be core" + M-TOOL roadmap slot.
**Status.** PARTIAL / NEW as an *exposed public API*. The compiler already builds
this internally for type-checking and the LSP (roadmap M13: go-to-definition,
hover). Shipping it as a *stable external query API* for third-party tools is NEW.
This is the same idea as core-vs-library #38 ("ask your codebase questions") — see B13.
**CEO note.** Interesting: a small core hook that turns all the viewer tools below
into a community ecosystem surface. Low cost, high leverage.

### A4. Unified "dossier"/outline views of a scattered type (T2–T7)
**What it is.** A family of editor/tool features that show a type's full shape in one
place even when its methods are spread across files: an outline list (T2), a
read-only stitched-together document (T3 "dossier"), collapsed phantom stubs inside
the braces (T4), breadcrumb hints (T5), generated doc pages (T6 "jetdoc"), and a
far-future projectional editor (T7).
**Source.** Part A, Layer 1 table (T2–T7) + the effort/fidelity map.
**Status.** PARTIAL / NEW. The closest existing items are the LSP (M13) and doc
tooling (S49 doc comments, M13). None of T2–T7 exists as a card; they're all
*library/tooling on top of A3*, not language changes.
**CEO note.** All deferrable tooling; the doc gen (T6) and breadcrumbs/outline (T5/T2)
are the cheap wins. Nothing here needs an owner syntax call — only A3 does.

### A7. `pub(package)` middle visibility tier
**What it is.** A visibility level between private and fully public: visible
anywhere in this package but not outside it (Rust's `pub(crate)`).
**Source.** Part B, "Visibility tiers (safe → open)."
**Status.** NEW. S18 ratified only private + `pub`; it explicitly considered and
declined grouped-visibility forms but did not add a package tier. Open question #3 in
the research file ("does `pub(package)` ship in v1?") is unanswered.
**CEO note.** Plausibly useful, but S18 deliberately kept visibility to two levels;
revisit only on real boilerplate evidence. Needs an owner call.

### A8. `pub use` re-export facades
**What it is.** Re-export items so the *public API* doesn't have to match the *file
layout* — callers' imports keep working when you move files around internally.
**Source.** Part B, "The power move: `pub use` facades."
**Status.** PARTIAL / NEW. S16 explicitly *rejected* "Rust `use a::b` re-export
chains." A first-class `pub use` facade is therefore not currently allowed and would
reopen that rejection. Folder auto-aggregation (research Open Q #2) is also unsettled.
**CEO note.** Conflicts with a ratified rejection in S16 — flag, don't silently adopt.
The refactorability win is real, though; worth an explicit reconsider if users hit it.

### A9. In-file `namespace { }` sub-grouping block
**What it is.** An optional block to group items inside one file without splitting it
into a new file — an escape hatch for C++-style namespaces.
**Source.** Part B, "collapse two concepts into one."
**Status.** NEW. No `namespace` construct is ratified or balloted; module = namespace
is the model, with no in-file sub-grouping block.
**CEO note.** Extra surface area for a niche need; likely a skip (I8) unless requested.

### A10. Selective / aliased imports `use a::b::{X, Y as Z}`
**Source.** Part B toolbox.
**Status.** PARTIAL. `as` aliasing is ratified (S16). Selective brace imports
(`use module { item }`) were explicitly **rejected** in S16.
**CEO note.** Aliasing done; the brace-selective form conflicts with a ratified
rejection — flag.

### A11. Glob imports `use a::*`, lint-gated
**Source.** Part B toolbox + seed diagnostic `W-IMP-010`.
**Status.** PARTIAL / NEW. S16 doesn't provide glob imports (rejected selective/glob
forms in the same spirit). The "allowed but lint-gated" recommendation (research
ballot row) is not ratified.
**CEO note.** S16 currently has no glob; adopting one is an owner decision. The
lint-gated middle path is sensible if it's wanted at all.

### A12. Prelude — auto-import a curated common set
**What it is.** Common names (print, etc.) are available without importing; opt out
with something like `#![no_prelude]`.
**Source.** Part B toolbox.
**Status.** ALREADY IMPLEMENTED (partial). A prelude exists — `print`, `panic`,
`require` are ratified prelude builtins (S9, S36). A user-facing *opt-out* and a
documented prelude-contents list (research Open Q #4) are not settled.
**CEO note.** Core idea done; only "what's in it / how to opt out" is open.

---

# B. From `language-ideas-core-vs-library.md`

This file sorts 42 capability ideas into library-able vs. must-be-core. Nearly every
one already has a board card or a deferred-ballot stub — this file appears to be the
*source* those cards were mined from. Cards below group by idea; near-duplicates merged.

### B1. The "living graph": self-updating values, ask-why, time-travel, return-a-hole (ideas 1–4)
**What it is.** Four headline features off one engine: (1) change an input and only
affected parts recompute (like a spreadsheet); (2) every value remembers where it came
from ("why is this value 7?"); (3) every variable keeps its history (time-travel
debugging); (4) a failure becomes a typed "missing/hole" that flows on instead of
crashing.
**Source.** "The living graph — one engine, four features," ideas 1–4 (flagged ★).
**Status.** ALREADY A CARD/PLAN. Board card **c64** (Reactive / dataflow) +
proposal **P3-reactive-dataflow.md**. The proposal (with the owner's own annotation)
concludes reactivity-as-evaluation-model is a **non-goal** (collides with priorities
#3/#4 and move semantics C1), and develops it instead as a *derived dataflow tool /
`.jet` artifact*. Idea 4 ("return a hole") overlaps the ratified optional/`T?` story (S32).
**CEO note.** Already triaged: the seductive "whole program is a spreadsheet" framing
is a stated non-goal; only the derived-graph *tooling* survives. Skip the runtime version.

### B2. Effect system — track what each function touches (idea 31, plus 30/33/35/42 family)
**What it is.** The compiler infers which functions touch the network, disk, clock,
etc., and can stop a function declared "pure" from silently gaining a side effect, or
wall a subtree off from the network.
**Source.** Idea 31 "Cap what code can do"; feeds 42 (auto-tracing), 30/33 (info-flow),
35 (taint).
**Status.** ALREADY IN BALLOT. **D-EFF1** (board card c66) — inferred effect tags,
recommended option B, explicitly flagged as reopening S60. Surface spelling pinned
against **D-QUAL1** (c62).
**CEO note.** Live decision; the linchpin that unlocks taint, capabilities, replay.

### B3. Taint tracking — untrusted input can't reach a sink (idea 35)
**Source.** Idea 35 "Meaning beyond shape."
**Status.** ALREADY A CARD + partly RATIFIED. Board card **c70**; **D-TAINT1 Option A**
(`#tainted` + sanitizers) was ratified 2026-06-21. The full information-flow version is B6.
**CEO note.** Option A landed; done in basic form.

### B4. Scoped capabilities — lend a power, revoke on scope exit (idea 32; +29 secrets that rot)
**Source.** Ideas 32, 29.
**Status.** ALREADY A CARD/BALLOT. Board card **c67** (D-SCAP1). Rides D-EFF1.
**CEO note.** Covered.

### B5. Units on every value — dollars ≠ euros, ms ≠ seconds (idea 34; +distinct IDs)
**Source.** Idea 34 (★) "Every value knows its unit."
**Status.** ALREADY A CARD. Board cards **c68** (Units as a first-class tag) and
**c23** (Distinct types & units); gated on the qualifier taxonomy D-QUAL2.
**CEO note.** Covered. The file's "nicer in core" lean matches the existing card.

### B6. Information-flow / compliance — "EU data can't leave EU" (ideas 30, 33)
**Source.** Ideas 30 "Rules travel with the data," 33 "Compliance as red squiggles."
**Status.** ALREADY IN BALLOT (deferred). **D-IFC1** — explicitly the full-IFC
generalization of D-TAINT1, owner-deferred to post-Epoch-3 on 2026-06-21.
**CEO note.** Captured and parked; needs the effect engine first.

### B7. Linear / must-use values — money that can't be silently dropped or copied (idea 16)
**Source.** Idea 16 "Money that can't leak."
**Status.** ALREADY A CARD. Board card **c69** (D-LIN1, `#linear`); gated on D-QUAL2.
**CEO note.** Covered.

### B8. Typestate — order-of-events, "charge before ship" (ideas 13, 14)
**Source.** Ideas 14 "Order-of-events types," 13 "Roles that change over time."
**Status.** ALREADY IN BALLOT. **D-STATE1** (board card c71); the time-varying-roles
variant (idea 13) is the deferred **D-ROLE1** stub on top of D-STATE1.
**CEO note.** Live decision (D-STATE1); roles deferred above it.

### B9. Safe schema changes — compiler blocks a breaking data-shape change (ideas 10, 11)
**Source.** Idea 10 "Safe schema changes," 11 "Self-versioning values."
**Status.** ALREADY IN BALLOT. **D-MIGRATE1** (board card c73), rec A, with the
conversion half assigned to a Build-tier versioning library (idea 11). Owner already
asked the bloat question (answered: squash-to-baseline).
**CEO note.** Live, well-developed decision.

### B10. Try-both-keep-the-winner + `#transact` rollback (ideas 27, 5, 6)
**What it is.** A block whose effects cleanly unwind if any step fails (idea 27
`maybe { } else { }`, idea 5 `undo` keyword, idea 6 rewind-to-last-known-good).
**Source.** Ideas 27, 5, 6.
**Status.** ALREADY IN BALLOT. **D-TXN1** (board card c72) — `#transact { }` rollback
over types that implement `Rollback`. Idea 6 (checkpoint/rollback framework) is the
library sibling the file itself flags. `undo` as a bare keyword (idea 5) is NEW/declined-
adjacent (defer keyword analog D-SUGAR5 was declined).
**CEO note.** The transactional core is covered by D-TXN1; the standalone `undo`
keyword is extra surface — skip.

### B11. Deterministic record-and-replay (idea 7)
**Source.** Idea 7 "Deterministic by default."
**Status.** ALREADY IN BALLOT (deferred). **D-REPLAY1** stub — gated on D-EFF1 + a
record/replay harness, not in v1 roadmap.
**CEO note.** Parked correctly.

### B12. Reversible computation / solve-for-the-input (idea 36)
**Source.** Idea 36 "Every function runs backward too."
**Status.** ALREADY IN BALLOT (deferred). **D-REVERSE1** stub — needs a solver/SMT
backend; no ergonomic prior art without macros.
**CEO note.** Parked. Research-grade; far horizon.

### B13. Generated client/server from one conversation; protocol/session types (idea 9)
**Source.** Idea 9 (★) "Write the conversation, not the services."
**Status.** ALREADY IN BALLOT (deferred). **D-PROTO1** stub — needs linear types
(D-LIN1) + typestate (D-STATE1) first.
**CEO note.** Parked behind its prerequisites.

### B14. Refinement types — "this Int is provably > 0" (idea 19)
**Source.** Idea 19 "Describe bad states, get a safe type."
**Status.** ALREADY IN BALLOT (deferred). **D-REFINE1** stub — needs an SMT/proof
layer; barred by I8 without a roadmap slot + owner sign-off.
**CEO note.** Parked. Heavy machinery.

### B15. Budgets as types — time/memory caps that break the build (idea 22)
**Source.** Idea 22 "Budgets as types."
**Status.** ALREADY IN BALLOT (deferred). **D-BUDGET1** stub — needs comptime
cost-bound inference; no ergonomic prior art.
**CEO note.** Parked.

### B16. Formal verification / proof integration (ideas 15, 17)
**Source.** Ideas 15 "Dial up correctness," 17 "Always-responds guarantee."
**Status.** ALREADY IN BALLOT (deferred). **D-VERIFY1** stub — post-v1, needs
proof-carrying-code / SMT.
**CEO note.** Parked. Explicit non-goal for v1.

### B17. Effect prohibitions — `#(no_net)` propagates through a call graph (idea 24/4 inverse)
**Source.** Idea 24 "Latency budgets flow downhill" framing + the inverse of 31.
**Status.** ALREADY IN BALLOT (deferred). **D-PROP1** stub — rides D-EFF1 +
D-QUAL1's `#(…)` surface.
**CEO note.** Parked behind the effect engine.

### B18. Content-addressed definitions / structural merge (Unison-style) (idea 41)
**What it is.** Identify a definition by the hash of its body, so renames are free and
merges can combine edits by meaning rather than text lines (idea 41 structural merge).
**Source.** Idea 41 "Merges that understand intent" (+ the file's general "living graph").
**Status.** ALREADY A CARD/PLAN. Board card **c63** + proposal
**P2-content-addressed-definitions.md**. L1 (invisible build cache) is the safe adopt
and already seeded (BuildCache.rs + SHA256.rs); L2/L3 (free rename, conflict-free
merge) are flagged as fighting the file-is-a-program tenet (U7).
**CEO note.** Already triaged: cache layer safe, the rename/merge magic conflicts with
a core tenet. Skip the ambitious part.

### B19. Auto-parallelize sequential-looking code (idea 26)
**Source.** Idea 26 "Parallel without the wiring."
**Status.** PARTIAL / NEW. Concurrency primitives are shipped (roadmap E2-M1: tasks +
channels, ownership proves sendability). *Automatic* parallelization of sequential code
(compiler proves it safe and schedules it) is NOT carded — and it's effectively a
non-goal: the reactive proposal P3 notes auto-scheduling is the hidden machinery
priority #3 forbids.
**CEO note.** Likely a skip — collides with the zero-hidden-machinery priority. Flag.

### B20. First-class "unknown" — loading / pending / never as distinct values (idea 21)
**Source.** Idea 21 "First-class unknown."
**Status.** ALREADY RATIFIED (as a library pattern). The file itself says "just a sum
type once the language has them" — Jet has enums (S30) and optionals (S32). No special
feature needed; a user writes `enum Unknown { Loading; Pending; Never }`.
**CEO note.** Already expressible; nothing to build. Skip.

### B21. Doctests — examples that double as tests and docs (idea 37)
**Source.** Idea 37 "Code = docs = tests in sync."
**Status.** ALREADY A CARD/ROADMAP. Board card **c51** (D-TEST4 doctest convention,
in ballot) + roadmap E2-M11 (doctests). The "can't go stale" *hard guarantee* the file
flags as core is the open part.
**CEO note.** Live decision (D-TEST4). The examples-as-spec ethos is already invariant I5.

### B22. Always-on self-fuzzing / property-based testing (idea 20)
**Source.** Idea 20 "Always-on self-fuzzing."
**Status.** ALREADY A CARD. Board card **c51** (D-TEST1 property-test surface, in
ballot) + roadmap E2-M11 (property testing).
**CEO note.** Live decision. Covered.

### B23. Ask-your-codebase code-query engine (idea 38)
**What it is.** Query the program structurally — "where can a balance go negative?",
"find paths where X." Same underlying need as the semantic index in A3.
**Source.** Idea 38 "Ask your codebase questions."
**Status.** PARTIAL / NEW. The compiler builds the needed graph internally; no
*user-facing query engine* is carded. Duplicate of A3 from the other file — they should
be treated as one item: ship the semantic index as a stable query API.
**CEO note.** Interesting low-cost ecosystem hook; the one genuinely-NEW idea that
appears in BOTH research files (A3 = B23) and isn't yet carded. Worth a card.

### B24. Impact / blast-radius analyzer (idea 40)
**What it is.** Show what a change can actually affect downstream ("touch pricing →
hits checkout, invoices, 2 reports").
**Source.** Idea 40 "See the true blast radius."
**Status.** NEW. Not carded. Pure tooling on top of the dependency graph (and A3/B23).
**CEO note.** Cheap, high-value dev tool once the query API (A3/B23) exists; bundle it.

### B25. Refactors as named, replayable, reversible objects / codemods (idea 39)
**Source.** Idea 39 "Refactors you can ship and undo."
**Status.** NEW. Not carded. Tooling (codemod engine), library-able.
**CEO note.** Nice-to-have dev tool; not urgent. Pairs with the LSP (M13).

### B26. TTL / expiring values (idea 8)
**Source.** Idea 8 "Memory with a shelf life."
**Status.** NEW (as a library). The file marks it library-able (a TTL cache type); no
card. The compiler-enforced *lifetimes* upgrade it mentions is already covered by Jet's
ownership model.
**CEO note.** Pure stdlib; build when a cache library is needed. Skip as a language idea.

### B27. Schema → generate everything (form, API, validation, storage) (idea 12)
**Source.** Idea 12 "Define data once, get everything."
**Status.** NEW (as a library/framework). Not carded. The file marks it library-able
(schema + codegen). Adjacent to ratified derives (S55 Serialize) but broader.
**CEO note.** A framework, not a language feature; ecosystem surface. Skip for core.

### B28. Honest numbers — value tracks its own precision/error (idea 18)
**Source.** Idea 18 "Honest numbers."
**Status.** NEW (as a library). Library-able uncertainty/interval type; the literal
syntax (`5.0±0.1`) the file wants in core is not carded.
**CEO note.** Niche; library unless a scientific-computing push wants the literal sugar.

### B29. Adaptive runtime — adjust to battery/network/load/carbon; fidelity-under-load (ideas 23, 28)
**Source.** Ideas 23 "Adapts to its surroundings," 28 "One knob for quality under load."
**Status.** NEW (as a library). Both marked library-able (adaptive-policy / load-aware
middleware); no card.
**CEO note.** Pure library/framework territory; out of core by the file's own test. Skip.

### B30. Latency budgets / deadlines that flow downhill (idea 24)
**Source.** Idea 24 "Latency budgets flow downhill."
**Status.** NEW (as a library). A deadline-carrying context value (Go's `context`).
Implicit propagation = a core upgrade that overlaps the effect system (B2/D-EFF1) and
Smart Context (c74). Not separately carded.
**CEO note.** Library version trivial; implicit-propagation version folds into the
effect/context work already in flight. Low priority standalone.

### B31. Trade accuracy for speed — approximate algorithms (idea 25)
**Source.** Idea 25 "Trade accuracy for speed."
**Status.** NEW (as a library). Approximate-algorithm library; auto-swapping = core,
not carded.
**CEO note.** Stdlib territory; skip as a language idea.

### B32. Auto-instrumentation / tracing at every external call (idea 42)
**Source.** Idea 42 "Logging you can't forget."
**Status.** NEW (as a library), but ENABLED-BY the effect system. The file says it's
"cleanest if the core marks effect points (see #31)" — i.e. it rides D-EFF1 (B2).
**CEO note.** Library once effects land; not a separate decision. Skip as standalone.

---

## Summary

**Ideas extracted: 43** (12 from file A: A1–A12; 31 from file B: B1–B32, with the
living-graph 1–4 and several near-duplicates merged into single cards).

**Already covered (ratified / implemented / carded / balloted): 25**
- Ratified/implemented: A1, A5, A6, A12, B3 (Option A), B20.
- In ballot / on the board / in a proposal: A4 (LSP/docs), B1 (c64/P3), B2 (D-EFF1),
  B4 (c67), B5 (c68/c23), B6 (D-IFC1), B7 (c69), B8 (D-STATE1), B9 (D-MIGRATE1),
  B10 (D-TXN1), B11 (D-REPLAY1), B12 (D-REVERSE1), B13 (D-PROTO1), B14 (D-REFINE1),
  B15 (D-BUDGET1), B16 (D-VERIFY1), B17 (D-PROP1), B18 (c63/P2), B21 (c51/D-TEST4),
  B22 (c51/D-TEST1).

**Genuinely NEW (no card): 18** — but most are low priority or flagged:
- **Worth a card:** A3 + B23 (the *same* idea, in both files — ship the semantic index
  as a stable query API), and B24 (impact/blast-radius analyzer, which rides it).
- **Conflicts with a ratified decision (flag, don't silently adopt):** A8 (`pub use`
  facades — S16 rejected re-export chains), A10 (brace-selective imports — rejected in
  S16), A11 (glob imports — not in S16), B19 (auto-parallelize — collides with the
  zero-hidden-machinery priority).
- **Needs an owner syntax call:** A7 (`pub(package)` tier), A9 (in-file `namespace {}`).
- **Skip / pure library by the file's own test:** A2 (`extend`), B10's bare `undo`,
  B20 (already expressible), B25 (codemods), B26 (TTL), B27 (schema codegen), B28
  (honest numbers), B29 (adaptive), B30 (deadlines), B31 (approx), B32 (auto-trace).

**Headline:** the core-vs-library file is essentially the *source* the existing board
cards (c63–c79) and deferred ballot stubs were mined from — it dedups almost perfectly.
The code-organization file is mostly already-ratified ergonomics (inline methods,
file=module, private-by-default). The one fresh, cross-file, low-cost idea is the
**semantic-index query API** (A3 = B23) and the dev tools that ride it (B24 impact,
B25 codemods).


# Idea cards — mined from two research notes

Source files mined (read-only):
- `docs/jet-borrow-from-spacetimedb.md` ("Borrowing from SpacetimeDB → Jet")
- `docs/jet-library-extensibility.md` ("How Far Can a Library Bend a Language?")

Each card: what it is · where it came from · dedup status against ratified
decisions (`syntax-decisions.md`), the open ballot (`decision-ballots.md`), board
cards (`board.json`), proposals, and the idea-cards screening doc
(`tools/Tower/docs/idea-cards.md`) · one CEO note.

Status legend: `NEW` · `ALREADY A CARD/PLAN` · `ALREADY IN BALLOT` · `ALREADY
RATIFIED` · `ALREADY IMPLEMENTED` · `PARTIAL` (gap called out).

**Headline.** The SpacetimeDB note yields **two real decisions** and one
philosophy-validation (no card). The extensibility note is a landscape that
collapses to **one strategic decision**. Net new ballots queued: **3** — D-DET1
(checked determinism), D-TXN2 (irreversible-effect guard inside `#transact`),
D-EXT1 (library extensibility ceiling). Everything else dedups into existing work.

---

# A. From `jet-borrow-from-spacetimedb.md`

The note's own verdict table puts most of SpacetimeDB out of scope (it's a
database). Three language-level borrows survive; one is already-handled, two need
a call.

### A1. Checked determinism — `pure fn` guarantees *reproducibility*
**What it is.** Strengthen `pure` from "no side effects" to "same inputs ⇒ same
output": inside `pure`, reject wall-clock, OS randomness, fs, net, and calls to
non-`pure` fns. Make it *usable* by injecting deterministic `Clock`/`Rng`
capabilities (seeded RNG, fixed invocation timestamp), with an explicit
`assume_deterministic { … }` escape for the few edges the checker can't prove.
**Source.** §2 "Primary borrow — checked determinism."
**Status.** PARTIAL / NEW as a guarantee. `pure fn` is **ratified** (S60) but as
a *purity/effect* tag, and D-EFF1 (c66) generalizes it to an inferred effect set —
neither pins *determinism* as the contract (a fn can be effect-free yet read a
global clock through an injected param). The injected-`Clock`/`Rng` half is fork
**2.5** in `idea-cards.md` (Keep/Discard, un-carded), noted to ride D-SCAP1 +
D-EFF1. The `assume_deterministic { }` escape hatch is NEW. → **D-DET1**.
**CEO note.** The genuinely new question: does `pure` mean *no effects* (D-EFF1's
framing) or the stricter *reproducible*? SpacetimeDB's evidence is that
reproducible is what unlocks caching / parallelism / replay. Subsumes fork 2.5.

### A2. Atomic blocks (STM-lite) — all-or-nothing mutation
**What it is.** A block whose mutations fully revert if any step fails.
**Source.** §3 "Secondary borrow — atomic blocks."
**Status.** ALREADY IN BALLOT. This is **D-TXN1** (board card **c72**):
`#transact { }` auto-rollback over types implementing `Rollback`. Near-exact dup;
the note's "heavier lift, sequence after determinism" matches the existing card's
sequencing. No new card for the block itself.
**CEO note.** Covered by c72. Don't re-open the surface — `#transact` is ratified
(S82). Decide it on the D-TXN1 ballot.

### A3. Irreversible-effect guard inside `atomic`
**What it is.** Reject irreversible side effects (`send_email`, a network POST)
*inside* an atomic/transactional block — "you can't un-send an email on rollback"
— and tell the user to fire it after commit. (SpacetimeDB forbids I/O in reducers
for exactly this reason.)
**Source.** §3 "The subtle rule worth stealing" (diagnostic JET-ATOMIC-002).
**Status.** NEW. D-TXN1 covers *reversible* mutations via the `Rollback` trait but
says nothing about effects that *can't* be rolled back. This guard rides D-EFF1
(it needs to know which calls are irreversible effects). → **D-TXN2**.
**CEO note.** Small, high-value safety rule that closes the obvious footgun in
auto-rollback. Pairs with D-TXN1; gated on D-EFF1 for the effect classification.

### A4. Reducer vs. procedure split (guardrailed default + opt-in escape)
**What it is.** SpacetimeDB's reducers (deterministic, transactional) vs.
procedures (may do I/O if you manage the transaction). The note frames this as
*validation* of Jet's "magic default + explicit escape hatch," not a feature.
**Source.** §0 "philosophy validation (free souvenir)."
**Status.** NEW but **not a feature** — it's external evidence the dual-facet
model works at scale (philosophy.md). No card.
**CEO note.** Cite it in philosophy/marketing; nothing to build.

---

# B. From `jet-library-extensibility.md`

A landscape, not a feature list: how deep into the pipeline a library may reach.
The note's own closing line names the one actionable ballot.

### B1. The extensibility tier model + the "global footgun" line
**What it is.** Five tiers of library power — 0 vocabulary, 1 blessed protocols, 2
marked DSL blocks, 3 compile-time codegen, 4 sigils/keywords/grammar — and a rule
for where Jet draws the line: allow **local** footguns (scope = your program),
reject **global** ones (scope = the shared language + every tool). Two banked
principles: *mark library-introduced syntax* (visually distinct from core) and
*diagnostics are the real ceiling* (Jet may only expose depth at which it can
still emit a clean, attributed error).
**Source.** §2, §5 (tier table, local-vs-global footgun, two principles).
**Status.** NEW as an explicit policy. The invariants already imply the ceiling
(human-ratifies-syntax, front-end-owns-diagnostics, simplicity ratchet) but no
card *states* the tier model or the third-party-vs-stdlib rope split. → **D-EXT1**.
**CEO note.** Worth ratifying as a standing policy so every future "can a library
do X?" has a one-table answer instead of re-litigation. Tier 4 = never; the live
question is how much rope Tiers 2–3 get and whether it differs for stdlib.

### B2. Tier 1 — blessed protocols (the workhorse)
**What it is.** Core defines a *fixed* piece of syntax + a hook; a library fills
it without inventing grammar: `for x in coll` via an iterator trait, `coll[i]` via
an index trait, `5km` via a literal-suffix trait. The note calls this "the
highest-value, lowest-risk extensibility Jet can ship" and the obvious first
ballot: *which surface forms get a hook, and is the hook-set open to third parties
or stdlib-only?*
**Source.** §5 (Tier 1 = workhorse), §7 (the obvious first ballot).
**Status.** PARTIAL. Literal suffixes exist for units (`9.99.usd`, D-UNIT1);
iteration and composable Iterator are shipped (E2-M7); user-defined derives attach
via `~~` (S56/S83). But these landed piecemeal — there is no *ratified hook-set*
nor a rule on third-party openness. Folded into **D-EXT1** (the openness question
is the same call).
**CEO note.** Mostly already real in pieces; D-EXT1 just makes the set and the
openness policy explicit so third parties know what they may implement.

### B3. Tiers 2–4 concretely (DSL blocks, proc macros, reader macros)
**What it is.** Tier 2 marked DSL blocks (`sql!{ … }`); Tier 3 AST/proc macros
(`#derive`-style codegen); Tier 4 reader macros / mutable grammar (Lisp/Raku).
**Source.** §2 table, §6 illustrative Jet.
**Status.** PARTIAL / flagged. Tier 4 conflicts with a ratified invariant
(human-ratifies-all-syntax) → **reject even for experts** (the global-footgun
rule). Tier 3 overlaps comptime (S26 law: value-only, *no macros*) and user
derives (S56/S83) — Jet deliberately has no proc macros. Tier 2 has no card.
These are the *options* inside D-EXT1's ceiling decision, not separate cards.
**CEO note.** S26's "no macros" law already pushes the ceiling below Tier 3's
general form. D-EXT1 ratifies that line explicitly and decides Tier 2's fate.

---

## Summary

**Ideas extracted: 7** (A1–A4, B1–B3).

**Already covered:** A2 (D-TXN1/c72), A4 (philosophy validation — no build).

**New ballots queued: 3**
- **D-DET1** — checked determinism: does `pure` guarantee reproducibility, with
  injected `Clock`/`Rng` + `assume_deterministic` escape (subsumes fork 2.5)?
- **D-TXN2** — reject irreversible effects inside `#transact { }` (rides D-EFF1).
- **D-EXT1** — library extensibility ceiling (tier model + local/global footgun
  rule) and whether Tier-1 hooks are open to third parties or stdlib-only.

**Coverage:** every substantive language-level idea in both files is captured above
as a card, a dedup, or a flagged-conflict. The two source files are safe to delete
once D-DET1 / D-TXN2 / D-EXT1 are screened.


# Idea cards — from the stdlib blueprint + bug-prevention field guide

Mined from `docs/research/ideal-stdlib-blueprint.md` and
`docs/research/jet-bug-prevention-field-guide.md`. Each card is one distinct
idea, deduped against ratified decisions (`docs/spec/syntax-decisions.md`),
the ballot (`tools/Tower/docs/ballots/decision-ballots.md`), the board
(`tools/Tower/board.json`), sidequests, the roadmap, and what Core already
ships (`docs/reference/core-library.md`).

**Headline finding:** the great majority of both research files is *already
shipped or already decided*. Jet has shipped a first-party ring (http, regex,
csv, toml, log, time, crypto, archive, db) and ratified the hard safety calls
(no-null, distinct types/units, taint, single-use/must-use, typestate,
capabilities, effects, scoped transactions, schema-migration). The genuinely
NEW items are a short list at the end of each section — flagged clearly so the
CEO isn't shown dupes.

Legend: `NEW` · `ALREADY RATIFIED (id)` · `ALREADY IN BALLOT (id)` ·
`ALREADY A CARD/PLAN (cNN)` · `ALREADY IMPLEMENTED (ref)`.

---

# A. From the standard-library blueprint

## A2 — Tiny composable interfaces (Reader/Writer/Iterator) underpin everything
**What it is:** Define a handful of small protocols (read bytes, write bytes,
iterate, close) once; every file, socket, compressor, and encoder implements
them, so one helper works on all of them. This is the "any pipe fits any pipe"
plumbing idea.
**Source:** blueprint, Principle 2; modules `io`, `iter`.
**Status:** ALREADY IMPLEMENTED — `Reader`/`Writer` + RAII cleanup shipped in
E2-M7 (streaming I/O); iterator pipelines exist.
**CEO note:** Skip — foundation is in place.

## A3 — No function "colors": sync-looking code that runs concurrently
**What it is:** No `async`/`await` keyword splitting the world in two; you write
plain blocking-looking code and a runtime multiplexes lightweight tasks. Avoids
the "every library written twice" tax.
**Source:** blueprint, Principle 3; module `concurrency`.
**Status:** ALREADY IMPLEMENTED — `core.tasks`: `spawn`, `join`, typed channels,
no async keyword (E2-M1).
**CEO note:** Skip — already a shipped, ratified design choice.

## A4 — Errors are values that carry context (cause chain + `?`)
**What it is:** Failures returned as values (not thrown), with `?` to propagate
and the ability to attach a human breadcrumb and the underlying cause.
**Source:** blueprint, Principle 4; module `error`.
**Status:** ALREADY RATIFIED/IMPLEMENTED — `T ? E`, `??`, structured `Error`
(message + code + source chain), `impl Source -> Target` conversion for `?`
(D-ERR-CONV, implemented).
**CEO note:** Skip — Jet's error model already matches this.

## A5 — Safe-by-default, sharp-on-request defaults (TLS verified, linear regex, etc.)
**What it is:** The correct/secure choice is the default; the fast/dangerous one
exists but you must reach for it deliberately.
**Source:** blueprint, Principle 5.
**Status:** ALREADY IMPLEMENTED/RATIFIED — TLS verification on (E2-M10, rustls);
linear-time ReDoS-safe regex is the only engine (jet.regex). Money-as-decimal is
the one gap (see A24).
**CEO note:** Skip the principle; the one missing piece (decimal) is carded below.

## A6 — One line for the common case, layers underneath for the 20%
**Source:** blueprint, Principle 6. **Status:** ALREADY IMPLEMENTED — Core's
one-liner helpers (`fs.read`, `json.parse`) with streaming added in E2-M7.
**CEO note:** Skip — design principle Jet follows.

## A7 — Observability in the box: structured logging, tracing, metrics
**Source:** blueprint, Principle 8 / module H. **Status:** ALREADY IMPLEMENTED —
`jet.log` (structured logging/tracing/metrics, E2-M12). Human-readable output
format already carded (D-LOGFMT1 / c92).
**CEO note:** Skip — shipped.

## A8 — Tested documentation (doc examples run in CI)
**Source:** blueprint, Principle 9. **Status:** ALREADY IN BALLOT / CARD —
doctests are D-TEST4 (ballot) and c51; doctest milestone E2-M11.
**CEO note:** Skip — already in the testing-ergonomics card.

## A9 — Editions for evolution (fix mistakes without breaking old code)
**Source:** blueprint, Principle 10. **Status:** ALREADY IMPLEMENTED —
editions/epochs ratified and shipped (E2-M2, `edition:` in `pkg.jet`).
**CEO note:** Skip.

## A10 — The ten "Ergonomic Laws" (pit of success, named args, types hold guardrails, …)
**What it is:** A named, testable checklist for making an API *feel* good: the
easy path is the safe path; name your boolean arguments; make illegal states
not compile; symmetric naming (encode/decode); errors that teach the fix;
dangerous ops get long scary names; zero-config first call.
**Source:** blueprint, Part 2½ (new in v2).
**Status:** Mostly ALREADY RATIFIED piecemeal — named args + defaults (S61/c02),
parameterized-query SQL (D-DB-style), distinct `Url`-style types (D-DIST1),
what/why/fix errors (diagnostics.md), did-you-mean (shipped). As an *explicit
written API-design rubric for the stdlib*, NEW.
**CEO note:** Worth a tiny doc: most laws are already Jet policy, but writing
them down as a "stdlib API review checklist" would keep new modules consistent.
Cheap, no syntax. Candidate for a short style note, not a ballot.

## A11 — `collections` — one obvious data structure per shape
**Source:** blueprint, module A `collections`. **Status:** ALREADY IMPLEMENTED —
list/map/set built in.
**CEO note:** Skip.

## A12 — `iter` — lazy pipelines (map/filter/fold over a stream, O(1) memory)
**Source:** blueprint, module A `iter`. **Status:** Partially shipped (iterator
adapters exist). The full adapter set (window/chunk/group_by) may have gaps.
**CEO note:** Mostly done; if any adapters are missing it's a small stdlib
fill-in, not a decision. Low priority.

## A13 — `text`/`string` — Unicode by grapheme cluster (so "👩‍👩‍👧".len == 1)
**What it is:** Iterate strings the way a human counts characters (grapheme
clusters), not by bytes or code units, to kill emoji/accent length bugs.
**Source:** blueprint, module A `text`.
**Status:** PARTIAL — Jet strings iterate by Unicode scalar (`s.chars()`, S-level
ratified), which fixes the UTF-16 surrogate bug but is *not* grapheme-cluster
aware (so a family emoji still counts >1).
**CEO note:** NEW-ish gap. Grapheme iteration is a real correctness nicety but
niche; a `graphemes()` method is a stdlib addition, no syntax. Park it.

## A14 — `fmt` — type-safe interpolation + Display/Debug split
**Source:** blueprint, module A `fmt`. **Status:** ALREADY IMPLEMENTED — `{name}`
interpolation (S8); print/format exist.
**CEO note:** Skip. (A Debug-vs-Display distinction may be a minor future refinement.)

## A15 — `time` — separate Instant/Duration/Date/ZonedDateTime + injectable Clock + IANA tz
**What it is:** Keep "a point on the timeline," "an amount of time," and
"wall-clock calendar date in a zone" as distinct types so DST and tz-rule
changes don't break date math.
**Source:** blueprint, module A `time` (flagship).
**Status:** ALREADY IMPLEMENTED — `jet.time` (calendar dates, zones, formatting,
E2-M9); `core.time` for monotonic ms. Injectable clock as a *capability* is the
bug-guide angle (see B7).
**CEO note:** Skip the module; the injectable-clock framing is covered in B7.

## A16 — `math`/`num` — arbitrary-precision integers
**Source:** blueprint, module A `math`. **Status:** PARTIAL — `core.math` ships
float/int math; bigint not documented as present.
**CEO note:** NEW-ish small gap. Bigint is a stdlib type addition (no syntax),
useful for crypto/finance. Low priority unless a user needs it.

## A17 — `random` — split fast-PRNG vs cryptographically-secure RNG
**What it is:** Two clearly-named generators so nobody seeds password tokens
with the predictable game-dice RNG.
**Source:** blueprint, module A `random`.
**Status:** ALREADY IMPLEMENTED — `core.random` (fast/seedable) + `jet.crypto`
ships "vetted random primitives" (secure RNG).
**CEO note:** Skip — both halves exist; could verify the naming makes the split
obvious, but no decision needed.

## A18 — `io` / `fs` — path objects with `/` joining, atomic write, dir-walk iterator
**Source:** blueprint, modules B `io`/`fs`. **Status:** ALREADY IMPLEMENTED /
CARDED — `Path` + streaming I/O (E2-M7); `fs.list_dir` full-paths + path join is
D-LSDIR1 / c88 (in ballot).
**CEO note:** Skip — shipped or carded.

## A19 — `os`/`process` — safe subprocess (arg list, never a shell string)
**Source:** blueprint, module B `os/process`. **Status:** ALREADY IMPLEMENTED —
`core.process.run(["git", …])` takes an arg list; no `shell=True` happy path.
**CEO note:** Skip — Jet already shipped the safe-by-default version.

## A20 — Structured concurrency: `scope`/nurseries that can't exit until children finish
**What it is:** Concurrent tasks live in a lexical scope that blocks until all
its children complete, so leaked/orphaned tasks are impossible — plus a
`Context` for deadlines/cancellation that propagates.
**Source:** blueprint, Principle 3 + module C.
**Status:** PARTIAL/NEW — Jet has `spawn`/`join`/channels and warns on dropped
task handles (L1101), but does **not** have a structured `scope {}` nursery or a
cancellation `Context`. Task-detach idiom is carded (D-DETACH1/c84) but that's
the opposite concern.
**CEO note:** **NEW and interesting.** A structured-concurrency `scope` (auto-join,
deadline-cancel-all) would be a real safety upgrade over today's manual join and
the L1101 warning. Worth a decision card (needs syntax). Medium effort.

## A21 — `serialize` — one derive, many formats (serde-style data-model / wire-format split)
**What it is:** Annotate a type once; read/write it as JSON, CSV, MessagePack,
TOML, binary — through one interface, no per-format hand-written parser.
**Source:** blueprint, module D `serialize` (called "highest-leverage").
**Status:** PARTIAL — Jet ships per-format modules (json/csv/toml) and typed
CSV-row + typed-JSON-output are carded (D-CSVROW1/c89, D-JSONOUT1/c90). A
*unified derivable Serialize/Deserialize across all formats* does **not** exist;
user-defined derives (S56) are explicitly deferred to Epoch 3.
**CEO note:** **NEW (big).** This is the serde architecture — the single
highest-leverage idea in the blueprint. Blocked on user-derives (S56, Epoch 3),
so not actionable now, but worth flagging as a north-star once derives land. It
would unify the c89/c90 typed-row work under one mechanism.

## A22 — `json` / `csv` / `toml` / `compress` modules
**Source:** blueprint, module D. **Status:** ALREADY IMPLEMENTED — `core.json`,
`jet.csv`, `jet.toml`, `jet.archive` (zip/tar/gzip). Streaming/strict-vs-lenient
JSON modes and surfacing lenient coercions are carded (c10).
**CEO note:** Skip — all shipped. (zstd/brotli/msgpack codecs are minor additions.)

## A23 — `regex` — RE2-style linear-time engine as the default
**Source:** blueprint, module E. **Status:** ALREADY IMPLEMENTED — `jet.regex` is
linear-time, no backtracking/backreferences by design (D-REGEX1); native
in-house engine carded (c79).
**CEO note:** Skip — this is exactly what Jet shipped.

## A24 — `Decimal` in Core (exact base-10 money math)
**What it is:** A built-in exact-decimal number type so people stop using floats
for currency (`0.1 + 0.2 != 0.3`). The blueprint calls Core decimal a
"public-health measure for financial code."
**Source:** blueprint, module A `math`; Principle 5; novel-bits #6.
**Status:** NEW (the *type*) — the bug-guide's float-for-money **lint** maps to
"new" too. No `Decimal` type ships today; sized floats F32/F64 are carded (c93)
but that's the opposite (more float, not decimal).
**CEO note:** **NEW and worth it.** A Decimal type + a "you used float for money"
lint is high-value, low-syntax-risk, and prevents a notorious bug class.
Candidate for a card. (Pairs with the bug-guide's money entry — same idea.)

## A25 — `net` / `http` / `url` / `ws` — networking crown jewels (client + routed server, TLS)
**Source:** blueprint, module F. **Status:** ALREADY IMPLEMENTED / CARDED —
`jet.http` client+server+TLS (E2-M10); routing+middleware is D-ROUTE1 / c83 (in
ballot). `url` parsing and `ws` may be partial.
**CEO note:** Skip core; **url** (WHATWG-correct parsing) and **WebSockets** may be
genuine small gaps — verify and, if missing, they're stdlib additions, not
decisions. Low priority unless asked.

## A26 — `crypto` — misuse-resistant high-level API (libsodium/Tink `seal`/`sign`)
**What it is:** The headline crypto call is "encrypt this blob with this key"
returning authenticated ciphertext with nonce handled — raw primitives demoted
to the basement so you can't foot-gun a reused nonce.
**Source:** blueprint, module G; novel-bits #2.
**Status:** PARTIAL — `jet.crypto` ships hash/HMAC/vetted-random *primitives*
(E2-M9). A high-level misuse-resistant `seal`/`sign` envelope API is **not**
documented.
**CEO note:** **NEW.** The misuse-resistant envelope is the blueprint's "strongest
opinion." It's a stdlib API-shape addition (no language syntax) layered over the
existing primitives. Worth a card — and it's the prerequisite for A27.

## A27 — Post-quantum + crypto-agility by default (hybrid X25519+ML-KEM behind the safe API)
**What it is:** Default to hybrid post-quantum crypto (classical + ML-KEM) so
traffic recorded today can't be decrypted later by a future quantum computer;
because callers say `seal.encrypt` not `aes_gcm`, the whole ecosystem upgrades
with zero call-site edits ("crypto-agility").
**Source:** blueprint, module G "Post-Quantum by default" (new in v2); novel-bits #8.
**Status:** NEW — nothing PQ in `jet.crypto` today; depends on A26's high-level
API existing first.
**CEO note:** **NEW, strategically interesting, not urgent.** The "harvest now,
decrypt later" threat and the NIST 2030 deadline are real, but this is a Tier-1
library upgrade, not v1-blocking. Flag as a forward-looking card; sequence it
after the misuse-resistant API (A26).

## A28 — `test` — property-based testing built into the standard box
**Source:** blueprint, module H; novel-bits #3. **Status:** ALREADY IN BALLOT /
CARD — D-TEST1 (property tests + shrinking) and c51; milestone E2-M11.
**CEO note:** Skip — already carded.

## A29 — `cli` — declarative arg parsing with auto `--help` (clap-shaped)
**Source:** blueprint, module I. **Status:** ALREADY IN BALLOT — D-ARGS1 /
c91 (structured flag/argument parsing).
**CEO note:** Skip — in the ballot.

## A30 — `uuid` (v4 + v7 time-sortable) and `encoding` (base64/hex)
**Source:** blueprint, module I. **Status:** PARTIAL/NEW — not in Core's eight
modules and not in the `jet.*` ring list; base64/hex/uuid don't appear to ship.
**CEO note:** NEW but trivial. uuid (esp. v7) and base64/hex are tiny,
no-decision stdlib additions. Bundle as a small "utilities" fill-in card. Low risk.

## A31 — `database/sql` — a driver *interface* with parameterized-only queries
**Source:** blueprint, module I. **Status:** ALREADY IMPLEMENTED (partial) —
`jet.db` ships SQLite (FFI-tier, E2-M9). A general *driver interface* (Go's
`database/sql` shape) over multiple DBs isn't documented; parameterized-only
queries align with the ratified taint model (B6).
**CEO note:** Skip the SQLite piece; a pluggable *driver interface* is a future
ecosystem question, not a v1 need.

## A32 — Embedded / no-runtime: one library, swappable I/O engine (core ⊂ alloc ⊂ std)
**What it is:** Instead of a separate "embedded stdlib," layer the library into
rings (no-heap `core` ⊂ heap `alloc` ⊂ OS `std`) and make "what waiting means" a
swappable engine chosen at link time — so the *same* code runs from a server to a
32 KB microcontroller with no `async` coloring and no second library.
**Source:** blueprint, Part 3½ (new in v2); novel-bits #9.
**Status:** PARTIAL — Jet already ships `--freestanding` cross-compilation
(E2-M15) and `use core.mem` low-level tier (E2-M13), and has no function colors
(A3). The *full* core/alloc/std ring layering + a pluggable colorblind I/O engine
is **not** designed.
**CEO note:** **NEW (large, strategic).** This is the "data center to doorbell"
ambition. Aligns with Jet's no-color design and existing freestanding work, but
it's a major architecture track, not a quick card. Flag as a long-horizon
direction; the ring-layering question is the concrete first decision.

## A33 — Explicit allocation at the boundary (caller-supplied buffer/allocator)
**What it is:** Functions that *can* avoid the heap take a caller-supplied
scratch buffer (`json.parse_into(input, buf)`) so they work in fixed-memory
environments — the convenience auto-allocating form stays the default.
**Source:** blueprint, Part 3½ Move 3.
**Status:** ALREADY A CARD/PLAN — arena/allocator work is c05 / D-ARENA-style
(`stdlib-allocators-arena.md` sidequest); arena inference is c26.
**CEO note:** Skip — the explicit-allocator direction is already carded.

---

# B. From the bug-prevention field guide

## B1 — Make bad states impossible with the type system (sum types, newtypes, typestate, linear)
**What it is:** Push invariants into types so whole bug classes won't compile:
can't add dollars to euros, can't pass an OrderId where a CustomerId is wanted,
can't read a closed file, can't leak a resource.
**Source:** field guide, Play A.
**Status:** ALREADY RATIFIED (bundle) — distinct types/units (D-DIST1/2/3),
units tag (D-UNIT1), typestate (D-STATE1), single-use/linear (D-LIN1 →
`#SingleUse`), RAII cleanup (S63). The "no-null + maybe type" half is its own
card (B2).
**CEO note:** Skip — this whole play is already ratified across c23/c68/c69/c71.

## B2 — No null; a "maybe" type the compiler forces you to handle
**Source:** field guide, Play A / menu row 1. **Status:** ALREADY RATIFIED/
IMPLEMENTED — no null; `T?` optionals with `value(x)`/`null`, forced handling,
`??` fallback, `?.` chaining (S35/S71). Jet's existing "#4 idea."
**CEO note:** Skip — shipped.

## B3 — Out-of-bounds index checked by default, prove-in-range to skip
**Source:** field guide, menu row "Out-of-bounds." **Status:** PARTIAL — bounds
checks exist (safe by default); a *prove-in-range to elide the check* tier is the
expert escape and isn't clearly ratified.
**CEO note:** Mostly done. The "unchecked-with-proof" fast path is a niche
expert-tier optimization; defer unless a perf user asks.

## B4 — Take away ambient superpowers: capability-based security
**What it is:** Code can't secretly read the disk, hit the network, or ask the
time — those powers must be *handed in*. Kills supply-chain surprises, injection,
and tz/flaky-time bugs in one stroke.
**Source:** field guide, Play B.
**Status:** ALREADY RATIFIED — scoped capabilities `#grant(fs){…}` revoked at
scope end (D-SCAP1), the c06 value-capability vocabulary (D-CAP1
`view`/`edit`/`take`/`share`), manifest capability surface (c07). Gated on the
effect system (D-EFF1/c66).
**CEO note:** Skip — this is one of Jet's signature ratified bets.

## B5 — Effect system (functions tagged with the effects they perform)
**Source:** field guide, Play B (effects). **Status:** ALREADY IN BALLOT —
D-EFF1 / c66 (effects as inferred tags); `#(no_net)` prohibition is the deferred
follow-on (D-PROP1).
**CEO note:** Skip — already the centerpiece of the qualifier ballot.

## B6 — Taint tracking: untrusted input can't reach a sink (SQL/exec/net)
**Source:** field guide, Play B / menu "Injection." **Status:** ALREADY RATIFIED
— D-TAINT1 option A: `#tainted` tag spreads, `sanitizer fn` strips it, reaching a
sink is E0721; full information-flow control (option B) deferred post-Epoch-3.
**CEO note:** Skip — ratified exactly as described.

## B7 — Injected clock & RNG (kills tz/DST bugs and flaky tests)
**What it is:** `now()` and randomness are powers passed in, not globals — real
clock in production, fake clock in tests, so tests never flake on time or
entropy.
**Source:** field guide, Play B / menu rows "Timezone/DST" + "Nondeterministic."
**Status:** PARTIAL — `core.time` has a test hook (`LEX_TEST_EPOCH`) and
`core.random.seed()` for determinism, and capabilities (B4) provide the
mechanism. But clock/RNG are **not** yet modeled as injected capability *values*
the way the guide describes; the guide lists this as "new (extends #7)."
**CEO note:** NEW framing on an existing mechanism. Once D-SCAP1 capabilities
land, modeling `Clock`/`Rng` as grantable capabilities is a natural, high-value
follow-on (the guide calls it "highest-leverage for little syntax"). Worth a card
sequenced after the effect/capability engine.

## B8 — Living graph: every value can explain its own origin (`why total`)
**What it is:** Instead of sprinkling print statements, the runtime keeps the
receipts — you can ask any value where it came from; variables keep history; a
failure becomes a typed "hole" that flows on instead of erasing the scene.
**Source:** field guide, Play C (the existing "#1–#4 living graph" track).
**Status:** NEW as a built decision (it's an aspirational track, not yet carded
in the ballot/board I can see). Related observability (log/trace) ships, but
value-provenance / `why?` is its own deep idea.
**CEO note:** **NEW, ambitious, signature.** The guide calls building this into
the runtime "a genuine moat." Large and research-y; flag as its own long-horizon
track, not a near-term card. Distinct from logging.

## B9 — Smell detector: warn on plausible-but-wrong code (dead branches, float `==`, etc.)
**What it is:** Gentle lints for code that looks right but isn't: identical
if/else branches, always-true conditions, comparing floats/decimals with `==`,
an unused result.
**Source:** field guide, Play D (called out as *new*, not in the 42-idea list).
**Status:** NEW — Jet has did-you-mean and what/why/fix errors, but no
"semantic smell" lint family. (Float-`==` overlaps with the decimal/money card A24.)
**CEO note:** **NEW, cheap, high-value.** A small lint pack (identical branches,
constant condition, float-equality) extends Jet's existing diagnostics strength
with no new syntax. Good momentum card — each lint is a diagnostic + snapshot.

## B10 — Confusable-name + did-you-mean lints (`users` vs `user`, `l` vs `1`)
**Source:** field guide, Play D / menu. **Status:** PARTIAL — did-you-mean on
typos ships (edit-distance ≤2 in diagnostics.md). A *confusable-name-in-same-scope*
warning (two near-identical live names) is NEW.
**CEO note:** NEW (small). The same-scope confusable warning is a cheap lint
addition; bundle with B9.

## B11 — Ignored results are errors, not warnings (must-use; opt out with `_ =`)
**Source:** field guide, Play D / menu "Swallowed error." **Status:** ALREADY
RATIFIED — `#MustUse` is the stepping-stone half of D-LIN1 (`#SingleUse`); the
guide's "must opt in with `_ =`" matches.
**CEO note:** Skip — ratified (may ship before full single-use).

## B12 — Ban assignment in conditions (`if x = 5` → error)
**Source:** field guide, Play D / menu; listed in the "suggested first wave."
**Status:** NEW — I found no ratified decision or diagnostic banning `=` in a
condition. (Jet uses `::`/`:=` for binding and `==` for equality, which already
reduces the risk, but a `=`-in-condition guard isn't documented.)
**CEO note:** **NEW, trivial, first-wave.** A single diagnostic; near-zero syntax
risk. Good quick win. (Worth confirming whether Jet's grammar even permits `=` in
a condition — if not, this is a non-issue and can be closed.)

## B13 — Integer overflow checked by default (opt into wrapping/saturating)
**What it is:** `255 + 1` on a byte traps instead of silently wrapping; experts
opt into `wrapping`/`saturating` explicitly.
**Source:** field guide, menu "Integer overflow" (new emphasis).
**Status:** NEW — not found ratified. Jet rejects out-of-range U8 *literals*
(E1003) but checked-arithmetic-by-default with wrapping/saturating escapes isn't
documented.
**CEO note:** **NEW, worth it.** Checked overflow is a classic safety default
(Rust debug-mode shipped it). Needs a small decision on the escape-hatch spelling
(`wrapping_add` vs a `#Wrapping` tag). Medium-low effort, high safety value.

## B14 — Money in floats: decimal type + float-for-money lint
**Source:** field guide, menu "Money in floats." **Status:** NEW — same idea as
blueprint A24 (Decimal). Dedup: **count once.**
**CEO note:** See A24 — merge. NEW, recommended.

## B15 — Schema-drift safety: no breaking data-shape change without a migration
**Source:** field guide, menu "Schema drift." **Status:** ALREADY IN BALLOT —
D-MIGRATE1 / c73 (compile-time migration check).
**CEO note:** Skip — carded.

## B16 — Copy-paste drift / structural-dup lint (updated 3 of 4 copies)
**Source:** field guide, menu "Copy-paste drift" (existing #40/#41). **Status:**
ALREADY a known idea (#40/#41); tooling, not carded in the ballot I can see.
**CEO note:** NEW-ish as a concrete card but lower priority; a structural-dup
lint is a tooling project. Park behind B9/B10.

## B17 — Examples = tests = docs + auto-fuzz (stale-docs / untested-error-path defense)
**Source:** field guide, menu "Stale docs / untested errors." **Status:** ALREADY
IMPLEMENTED/CARDED — golden examples enforce docs (I5); doctests + property/fuzz
testing in c51/D-TEST1/E2-M11.
**CEO note:** Skip — covered.

## B18 — Complexity hints / budgets-as-types (O(n²), N+1 queries)
**Source:** field guide, menu "Accidental slowness." **Status:** ALREADY IN
BALLOT (deferred) — D-BUDGET1 (budgets as types, deferred; needs comptime
cost inference).
**CEO note:** Skip — already deferred in the ballot.

## B19 — The safety ladder (Beginner → Working → Expert rungs)
**Source:** field guide, §4. **Status:** ALREADY IMPLEMENTED (philosophy) — this
is Jet's "safe by default, expert tier opt-in" (I1) made into a picture; matches
the ratified `@unsafe`/`#Audit`/capability tiers.
**CEO note:** Skip — it's the existing philosophy restated; useful framing for docs.

---

# Summary for the CEO

**Total distinct ideas extracted: 51** (A1–A33 = 33 from the blueprint;
B1–B19 = 19 from the bug guide; A24/B14 are the same Decimal idea, deduped to one
→ **50 unique**).

**Already covered (skip — shipped, ratified, or carded): ~36.** Jet has already
done the heavy lifting: two-tier library, no-color concurrency, value-errors,
linear regex, TLS-verified networking, the whole bug-prevention safety stack
(no-null, distinct types/units, taint, capabilities, effects, typestate,
single-use/must-use, schema-migration), the first-party ring, and the testing/
observability cards.

**Genuinely NEW and worth the CEO's eye (~14), ranked by value/effort:**
- **Quick wins, low syntax risk:** ban `=` in conditions (B12), smell lints +
  same-scope confusable lint (B9/B10).
- **High-value, small decision:** `Decimal` type + float-for-money lint (A24/B14);
  checked integer overflow by default (B13); injected Clock/Rng capabilities (B7,
  after the capability engine lands).
- **Medium, real decisions:** structured-concurrency `scope`/nursery + cancellation
  Context (A20); misuse-resistant high-level crypto API (A26).
- **Strategic / long-horizon (flag, don't card yet):** serde-style unified
  Serialize across all formats (A21, blocked on user-derives/Epoch 3); embedded
  "one library, swappable engine" ring layering (A32); the living-graph value-
  provenance engine (B8); post-quantum crypto by default (A27, after A26).
- **Trivial fill-ins (no decision):** uuid v4/v7 + base64/hex (A30); grapheme
  iteration (A13); bigint (A16); url-parse + WebSockets if missing (A25).

Output file: `tools/Tower/docs/ideas/from-stdlib-blueprint-and-bug-guide.md`.


# Idea cards — mined from the Verse and Swift/TS research notes

Source files mined:
- `docs/research/jet-borrowing-from-verse.md`
- `docs/research/jet-vs-swift-typescript.md`

Each card is one distinct idea (near-duplicates merged). **Status** says whether
we've already decided or built it, so you only spend time on the genuinely new
ones. Read the **CEO note** for the one-line keep/skip call.

Headline: almost everything in the Verse brief is **already ratified or in the
ballot** — that doc is more "outside validation of bets we made" than new ideas.
The Swift/TS brief has the real new material: a typed full-stack protocol with a
migration gate, and the whole reactive-UI/web-frontend story.

---

## From `jet-borrowing-from-verse.md`

### 4. Structured concurrency without the "async tax" (no function coloring)
**What it is.** A small set of concurrency words (run-all, race-and-take-winner,
background) where tasks can't outlive their scope, and concurrency lives in the
one effect system instead of a separate "async" world that splits functions into
two incompatible colors.
**Source.** Section 2.4 "Structured concurrency".

### 5. A tiny formal core that everything desugars to
**What it is.** Define a small Jet "kernel" and make every surface feature
shorthand for kernel operations, so the language stays small and honest as it
grows instead of sprawling like C++.
**Source.** Section 2.5 "A tiny formal core — a process borrow, not a feature".
**Status.** `NEW` (as an explicit process) — but it restates principles already
baked into CLAUDE.md (the "simplicity ratchet" I8, "examples are the executable
spec" I5). No formal kernel document exists.
**CEO note.** Mostly a re-statement of our existing discipline; the only new ask
is "write down an actual minimal kernel + desugaring map now." Low urgency, but
cheap insurance against feature sprawl — a possible process card, not a feature.

### 6. Logic-programming subset: list-builders that auto-skip failures
**What it is.** Verse's full logic programming (functions that run "backwards,"
try many answers) is too exotic — Epic itself stripped it from Fortnite. Keep
only the gentle slice: list/collection builders that automatically skip the
elements where a step fails (filter-as-you-go).
**CEO note.** Possibly worth a small card: a failure-aware comprehension is a
natural payoff of idea #1 (failure-as-control-flow) and feels beginner-friendly.

---

## From `jet-vs-swift-typescript.md`

### 9. Reactivity built into the language (signals/derived/effects in the runtime)
**What it is.** Change one input and only what depends on it recomputes — and
because every value tracks where it came from, time-travel debugging is free.
Frameworks (React/Solid/Svelte/Leptos) bolt this on; Jet would put it in the
runtime, with `freeze(x)` to opt a hot path out.
**Source.** "The five things only Jet does" #1 (#1–4); UI table L0 "Reactivity".
**Status.** `ALREADY A CARD (c64 "Reactive / dataflow")` — but note the card's
current stance: adopt the dataflow graph as *tooling* + an opt-in `std.reactive`
library, and **reject reactivity as the core evaluation model** because it
collides with priorities #3/#4 and move semantics (proposal P3-reactive-dataflow).
**CEO note.** Important tension: this research wants reactivity *in the core
runtime*; our existing c64 card explicitly rejects that and keeps it a library.
Worth a deliberate decision — the Swift/TS doc's whole "replace everything" thesis
rests on core reactivity, so this is the one place to consciously pick a side.

### 10. One type system across the wire — typed client/server protocol
**What it is.** Describe the client/server protocol once (`protocol Orders { … }`)
and the compiler generates both sides, so a mismatched API can't ship. Replaces
the tRPC/Zod/hand-synced-two-stacks dance.
**Source.** "The five things only Jet does" #2 (#9, #10); `protocol Orders` example.
**Status.** `NEW` — no protocol/over-the-wire card, ballot item, or ratified
decision. Roadmap only has generic networking (E2-M10) far out.
**CEO note.** Genuinely new and strategically central to "replace TypeScript." Big
scope (it's a whole full-stack story), but this is a headline differentiator worth
a real card/decision.

### 14. Refinement types (values constrained beyond their base type)
**What it is.** Types that carry an extra constraint (e.g. a non-empty list, a
positive int), so more mistakes are caught at compile time.
**Source.** "Five things" #4 (#19), listed alongside units/taint/capabilities.
**Status.** Partial — `ALREADY RATIFIED (limited)`: `[T#N]` exists as a
compile-time length refinement of a slice. General user-defined refinement types
are not in any decision/ballot/card.
**CEO note.** The narrow array-length form exists; general refinement types are
effectively `NEW` but only mentioned in passing here. Low priority unless someone
asks for it explicitly — flag, don't chase.

### 17. Reactive UI stack with an ownable, editable component kit
**What it is.** A layered UI stack (reactivity → view model → typed styling →
headless behavior → styled kit → motion → app kit) where the bottom layers are
the compiler's job and the kit is copy-in-and-own (shadcn-style), not a locked
theme. The pitch for "replace SwiftUI and the React ecosystem."
**Source.** "The UI stack" section, layer table L0–L6; "Four moves that are
Jet-only."
**Status.** `NEW` — no UI/view/component cards, ballots, or ratified decisions
(roadmap has nothing here; it's all post-v1 expansion in the doc's own agent
handoff). Depends on idea #9 (reactivity) landing in the core.

### 18. Typed styles — CSS that won't compile with a typo or wrong-unit value
**What it is.** Styles written in Jet with typed tokens: a misspelled property
(`paddng`) or a wrong-category unit (`delay: 200.px` — delay wants a time) is a
compile error instead of silently doing nothing.
**Source.** UI section "Typed styles that won't compile…"; diagnostics JET-E0460,
E0461.
**Status.** `NEW` — no styling/CSS decisions exist. (It reuses the ratified units
system from #12, but the styling layer itself is undecided.)
**CEO note.** New, and a clean demoable win, but it sits on top of the whole UI
stack (#17). Bundle with that track rather than as a standalone card.

### 19. Accessibility on by default in UI components
**What it is.** Headless components ship focus-trap, ARIA, and keyboard handling
for free; a11y attributes are applied for you (override per element), and a missing
accessible label warns then hard-errors on release builds.
**Source.** UI section "Accessible behavior you get for free"; diagnostic
JET-W0110; a11y-enforcement ballot row.
**Status.** `NEW` — no a11y decision/card exists.
**CEO note.** New, but part of the UI-stack track (#17) and post-v1. Nice safety
story; not a standalone near-term item.

### 20. Motion as derived reactive state (no separate animation runtime)
**What it is.** Animation is just reactive values changing over time (`opacity:
shown ? 1 : 0`, with a spring), instead of shipping a whole runtime like Framer
Motion.
**Source.** UI section "Motion is just reactive state."
**Status.** `NEW` — no decision/card. Depends entirely on core reactivity (#9).
**CEO note.** New, but a downstream payoff of the reactivity decision; part of the
UI track (#17), not a standalone item.

### 21. Render-target abstraction (web / native / embedded / TUI as backends)
**What it is.** UI components target an abstract renderer interface, not a specific
backend, so web (DOM+WASM), native (FFI or own-renderer), embedded, and TUI are all
just pluggable backends. The doc stresses deciding this *early* or you marry one
backend forever.
**Source.** "Plan early" ("Render-target abstraction"); ballot row "Render-target
abstraction" (rec: design the trait now).
**Status.** `NEW` — no render-target decision/card.
**CEO note.** New and time-sensitive per the research ("decide before the kit
exists"), even though delivery is post-v1. The cheap, high-leverage move is to
sketch the trait now; worth a small early card even if the UI itself waits.

### 22. Web backend: compile views to JS DOM ops, send logic/compute to WASM
**What it is.** WASM can't touch the DOM cheaply, so emit the view as plain JS DOM
operations (Svelte's move) and send only logic to WASM — dodging the boundary tax.
Cost: maintaining two backends (one emitting JS, one WASM).
**Source.** "Three frontiers — Web, vs JavaScript"; ballot "Web view architecture"
(rec b).
**Status.** `NEW` — nothing in roadmap/cards on a web/WASM/JS backend.
**CEO note.** New; the concrete delivery path for "replace TypeScript." Post-v1
and heavy (two backends), but it's the architectural choice the web story hinges on.

### 23. App backend: FFI to native widgets first, own-renderer canvas later
**What it is.** For apps-vs-Swift: start by FFI-ing to native widgets
(SwiftUI/UIKit, Jetpack Compose) for true native look, then add a Flutter/Skia-style
own-renderer for one-codebase-everywhere consistency.
**Source.** "Three frontiers — Apps, vs Swift" (two-route table); ballot "App UI
strategy" (rec c, native first).
**Status.** `NEW` — no card; general C FFI exists on the roadmap (E2-M14) but not a
native-UI strategy.
**CEO note.** New, post-v1, and the most expensive frontier (per-platform UI work).
Strategic direction only at this stage.

### 24. Interop as a first-class, day-one target (call JS/npm and Swift/native)
**What it is.** The cold-start problem (npm has millions of packages, Jet has zero)
kills new languages; the only fix is calling into JS/npm and Swift/native from day
one and migrating logic in gradually. The doc calls this the single most
adoption-critical decision.
**Source.** "Three frontiers — The cold start"; "Plan early" (interop first-class);
ballot "Interop priority" (rec a, JS/npm first).
**Status.** `NEW` (for JS/Swift) — only **C** FFI is on the roadmap (M7 `extern
rust`, E2-M14 C FFI). No JS/npm or Swift interop decision exists.
**CEO note.** New and flagged by the research as adoption-critical.

### 25. Auto-parallelism — sequential-looking code runs in parallel when proven safe
**What it is.** `users.map(fetchProfile)` runs in parallel automatically when the
compiler can prove there's no data race — Rust-level safety without fighting the
borrow checker. The doc calls it the hardest item and says ship no-GC speed first,
let auto-parallel land later.
**Source.** "Five things" #3 (#26); `let profiles = users.map(...)`; sequencing
section.
**Status.** `NEW` as *auto*-parallelism. Manual tasks/channels are ratified (S53,
deferred) and on the roadmap (E2-M1, data-race-free tasks via ownership). But
implicit auto-parallelization of `.map` is not in any decision/card.


# Library legitimacy survey — Core vs. the lauded standard libraries

Owner-requested 2026-06-22: compare Jet's libraries to popular, highly-regarded
libraries from other languages and propose improvements that make ours
substantially better and reinforce Jet as a serious language. This is the survey;
the concrete decisions it spawns are full ballots in `decision-ballots.md`.

**Method.** Benchmark Core against the libraries practitioners cite as best-in-class
(Rust std + serde/itertools/rayon, Python stdlib + requests/pathlib, Go stdlib,
Swift stdlib/Codable, C# LINQ). Keep only gaps that (a) experts hit daily, (b) fit
Jet's philosophy (safe-by-default, no hidden machinery), and (c) aren't already
ratified or carded. Dedup is aggressive — most of the obvious wins are already in
`idea-cards.md` §4.

## The three top gaps → full ballots queued (this batch)

| Area | Benchmark | Gap | Ballot |
|------|-----------|-----|--------|
| Numeric tower | Rust sized ints + checked/wrapping/saturating + `T::MAX`; Swift traps | sized-int spellings ratified (D-SG9) but unimplemented; no overflow policy, no per-type constants/bit-ops | **D-NUMOPS1** (c103) |
| Serialization | serde (one derive → every format + Deserialize) | `Serialize` exists (S55) but no format-agnostic data model, no `Deserialize`, no field attrs | **D-SERDE1** (c104) |
| Iterators | Rust `Iterator` (~70 lazy adapters), Python `itertools`, LINQ | only `map`/`filter`/`sum`; missing enumerate/zip/chunks/windows/group_by/flat_map/scan/… | **D-ITER1** (c105) |

These three are the highest-leverage legitimacy wins: a real numeric tower, serde-
grade serialization, and a rich lazy-iterator surface are precisely the libraries
practitioners check first when judging whether a language is serious for systems,
data, and services work.

## Already covered — no new ballot (dedup)

Most cross-language staples are ratified or already carded (see `idea-cards.md` §4):
errors-as-values + cause chains (D-ERR-CONV), no-color concurrency
(E2-M1/S53), composable Reader/Writer/streaming I/O (E2-M7), regex (ReDoS-safe,
D-REGEX1), http/tls/csv/toml/log/time/crypto/db ring (E2-M9/M10/M12), property +
doctest (c51), CLI parsing (D-ARGS1), arena/allocators (c05/c26), path objects +
atomic write + dir-walk (E2-M7/D-LSDIR1). Decimal money type and misuse-resistant
crypto and uuid/base64/hex/bigint/grapheme fill-ins are already idea-carded
(`idea-cards.md` forks 2.4 / §3-Stdlib). Don't re-card these.

## Remaining smaller gaps — stubs (expand to a ballot when reached)

- **Collections breadth.** Core has `List` + `Map` (BTreeMap, S38) but no first-class
  **`Set`** and no **`Deque`/ring buffer**. Rust (HashSet/BTreeSet/VecDeque), Python
  (set/deque), Swift (Set) all ship these as table-stakes. *Rec:* add `Set<T>` (Core)
  + `Deque<T>` (ring library); `Set` is the more urgent — its absence is conspicuous.
  Needs a small surface decision (literal? `{1,2,3}` collides with blocks — likely a
  constructor only, mirroring map's no-literal stance). → candidate ballot D-SET1.
- **Iterator terminal richness on collections** rides D-ITER1; no separate card.
- **Datetime ergonomics.** `jet.time` exists (E2-M12); verify it covers the chrono/
  `time`-crate surface experts expect (durations, formatting/parsing, timezone-aware
  instants, monotonic clock). If gaps, a focused enhancement — not a syntax decision.
  *Likely library work, no ballot.*
- **Text/Unicode.** Grapheme-cluster iteration + normalization is already an
  idea-cards §3-Stdlib fill-in; promote there, not here.

## Note on scope

These reinforce legitimacy without bending the simplicity ratchet: every item is
vocabulary or a blessed-protocol method (D-EXT1 Tier 0/1), not new grammar. The
numeric and iterator work needs no new syntax; serde needs the user-derive engine
(S56, Epoch 3) but its *shape* is decided by D-SERDE1 now.

# P1 — The qualifier system: leftover policies (maturity, uncertainty, cost)

## 2. Maturity tags (`experimental` / `tested` / `hardened`) — NOT YET CARDED

> *Scratchpad:* "Code tags: experimental, tested, or hardened — restricts
> hardened code from silently depending on experimental code."

A three-level lattice. The rule: **trust never flows downhill silently.**
Hardened code may not depend on tested or experimental code without an
explicit, visible acknowledgement.

```jet
#Hardened
fn settle_payment(order: Order) -> Receipt { ... }

#Experimental
fn estimate_fraud(order: Order) -> Float { ... }
```

If `settle_payment` calls `estimate_fraud`, the compiler stops:

```
Error [E07xx]: hardened code depends on experimental code
  ┌─ billing.jet:3:5
  │
3 │     let risk = estimate_fraud(order);
  │                ^^^^^^^^^^^^^^^ `estimate_fraud` is #Experimental
  │
Why: `settle_payment` is #Hardened — it must not silently rely on code that
     may still change or be wrong.
Fix: harden `estimate_fraud`, or opt in explicitly at the call site with
     `#Trusting(Experimental) { let risk = estimate_fraud(order); }`
```
*(Diagnostic codes here are illustrative; real codes get assigned in
diagnostics.md, which pins the `Error [E####]` / `Why:` / `Fix:` format.)*

This is a **tag** by the D-QUAL2 rule: it propagates (a function is as
trustworthy as its weakest call) and is checked relationally (hardened vs
experimental). It would ride the same engine as effects/taint — but it is its
own policy and has **no card or decision yet**.

Tradeoffs:

| For | Against |
|---|---|
| Trust boundaries become a compile error, not a code-review hope. | A thing to keep updated as code matures. |
| Refactors can't quietly pull unstable code into a stable path. | The lattice needs a default; unmarked code has to mean *something*. |
| Pairs with the package story — a registry can show a crate's maturity. | Over-tagging is noise; needs a light default and inference. |

**Open decision (un-carded):** what does unmarked code mean — `tested`,
`experimental`, or "untagged / unchecked"? Determines whether the rule bites
by default or only on opt-in.

---

## 4. Tracked value dimensions (uncertainty, cost) — NOT YET CARDED

> *Scratchpad:* uncertainty as a pervasive type dimension; cost/resource
> types.

These ride on **values** rather than code, but are the same tag kind by the
D-QUAL2 rule: a propagating, must-discharge property. Neither has a card or
decision. (`#Tainted` (D-TAINT1) is one *instance* of a value-tag, but the
general **uncertainty** axis — maybe-stale, untrusted, estimate ±5% — and
**cost** types are not carded.)

### 4a. Uncertainty

An axis on a value: "might be null," "came from untrusted input," "possibly
stale," "estimate ±5%." It propagates through expressions and must be
discharged before the value is used where it matters.

```jet
age: Int?Untrusted := form.field("age").to_int();   // untrusted + maybe-absent
// using it raw is an error:
charge_adult_rate(age);   // error: `age` is #Untrusted and possibly absent

when age {
    some(a) if a >= 18 -> charge_adult_rate(a);   // discharged: present + checked
    _ -> reject();
}
```

The whole class of "I assumed this was fresh/clean/present" bugs becomes a
type error. Jet already has `Option` for the present/absent axis (M3) — this
generalizes the *idea* to other axes (trust, freshness, precision) rather than
minting a new `Option`-like type per axis. `#Tainted` (D-TAINT1) handles the
untrusted-input sub-axis specifically; the broader freshness/precision axes
remain unexplored.

### 4b. Cost / resource

The type tracks a budget: time complexity, allocation, latency. Exceeding it
is a compile error.

```jet
#Budget(latency: 10ms)
fn on_keystroke(e: Key) -> Edit { ... }   // compile error if a callee blows 10ms
```


# P2 — Content-addressed definitions

**Status:** idea / proposal (not a plan).

> *Scratchpad:* "A function's true identity is the hash of its content, not its
> name or location. Names become local aliases you can change for free. Prior
> art: Unison. Renaming is instant and global with zero risk; no merge
> conflicts on unrelated code; dependency hell disappears because old code
> still resolves to the exact bytes it always did."

---

## 0. Glossary

- **Content address** — the identity of a definition is a hash of its
  (normalized) body, not its name or file path.
- **Name** — a local, mutable label that points at a content address. Renaming
  changes the label, not the thing.
- **Codebase store** — a content-addressed database of definitions that the
  compiler reads instead of (or alongside) parsing a tree of text files.

---

## 1. The idea in one breath

Today a definition's identity is "the function called `parse` in `parser.jet`."
Move the file or rename the function and every reference must be chased.
Content addressing flips it: identity is `#a3f9…` (the hash of the body);
`parse` is just a name you've pinned to that hash locally. Rename for free —
the hash never moved. Two branches that both edited *different* functions can
never conflict, because they touched different hashes.

```jet
// You write ordinary code:
fn greet(name: String) -> String { "hello, " + name }

// The store records it as:
//   #a3f9c1…  =  fn (String) -> String { "hello, " + $0 }
//   alias "greet" -> #a3f9c1…   (local, editable)
```

---

## 2. What falls out (the actual value)

**Read this with §4's levels in mind.** Most of these payoffs are *Level-3*
(full database) properties — they require names and bodies to live outside
canonical text. The one that survives with text canonical is caching. §4 maps
each payoff to the level that actually delivers it.

- **Renaming is instant, global, risk-free** *(Level 3)*. Identity is the
  alias row, so no downstream consumer can break. With text canonical (L1/L2),
  call sites still contain the callee's *name*, so a rename is an ordinary
  whole-tree LSP rewrite — not Unison's free rename.
- **No merge conflicts on unrelated code** *(Level 3)*. True when the codebase
  is the store. If bodies live in git as text, you merge text and get ordinary
  git conflicts.
- **Dependency hell shrinks** *(Level 2–3)*. Old code resolves to the exact
  bytes it always did; a new version is a new hash that coexists with the old.
  Two versions of a library in one build is not a conflict — it is two hashes.
  (Coexistence ≠ interop: the two hashes are *different types* and values can't
  flow across the boundary — same as Unison.)
- **Perfect incremental builds + caching** *(Level 1 — available now)*. A hash
  that hasn't changed never recompiles and never re-tests. This is the real
  near-term carrot, and the only payoff that holds with text canonical.
- **Psychology** *(Level 3)*, owner's framing: kills naming anxiety and loss
  aversion — but only once identity actually *is* the hash, which L1/L2 don't
  reach.

---

## 3. What it costs / fights

This is the most architecturally invasive idea in the scratchpad. It touches
the **"a file is a complete program"** tenet head-on.

| Tension | Detail |
|---|---|
| **Distribution tenet** (philosophy: "a file is a complete program"). | Unison's model replaces the text-file tree with a database. Jet has explicitly promised `jet run foo.jet` with no project, no store. These must be reconciled: text stays the source of truth; the store is a *derived cache*, not the canonical form. |
| **Tooling expectations.** | Diff, grep, code review, and git all assume text files. A pure Unison-style store breaks every one of them unless text projection is first-class. |
| **`jet fmt` / one mechanical path.** | Hashing must normalize formatting, comments, and local names, or trivial edits churn hashes. That normalization *is* a spec surface. |
| **Diagnostics (I2/I4).** | Errors must point at `file:line` in the text the user wrote, not at a hash. The store can't leak into error messages. |
| **Scope.** | Full Unison semantics (typed runtime, `ucm` codebase manager, no text files) is a different language. Jet would adopt the *idea*, not the whole model. |

---

## 4. Three honest framings (pick the altitude)

The owner's call is really *how much* of this to adopt. Three coherent levels:

**Level 1 — Content-addressed build cache (invisible).**
Hashing is an internal compiler optimization. Users see ordinary files; the
compiler keys incremental compilation and test-skipping on normalized-body
hashes. Zero language-surface change. **Jet already has the seed of this:**
`Source/BuildCache.rs` content-keys builds on `sha256_hex(generated source +
profile)` using the std-only `Source/SHA256.rs`, stored out-of-tree under
`~/.cache/jet/` (the same pattern `FFI.rs` uses). The one caveat that keeps L1
from being a *free* win: normalization correctness is load-bearing. If two
semantically-different bodies normalize to the same hash, a failing test gets
silently skipped — a soundness bug against priority #1. So even L1 needs the
normalization rule spec-pinned and tested like a diagnostic (I4); it is "no
*new syntax*," not "no decision."

**Level 2 — Content-addressed names (semi-visible).**
Add a notion of stable identity so renames and refactors are tracked as
alias moves, and two library versions can coexist by hash. Text files stay
canonical; the store is derived. Touches the package/version story
(`name#ver`, U6 source refs) and `jet fmt` normalization. Medium risk, large
payoff for the dependency story.

**Level 3 — Full Unison-style codebase (visible, invasive).**
Identity *is* the hash everywhere; names are pure projection; the codebase is
a database. Maximum payoff for the psychology and merge story, but collides
with the file-is-a-program tenet and every text-based tool. High risk; likely
post-v1 if ever.

---

## 5. Fit with Jet's existing decisions

- **Pairs with the package story — but isn't it yet.** `name#ver` pins are
  deliberately human-readable *semver strings* (VERSION-#), and U6
  `provider@target` is a *source location* — neither is a body hash, so don't
  call them content-addressed. Level 2's fit is narrower and friendlier: hashes
  could *back* a pin (verify the bytes behind `textkit#1.2.0`) without changing
  its spelling.
- **Pairs with P1 capability tags.** A hash is a perfect key for "this exact
  code was audited / is hardened" — maturity and capability facts attach to a
  content address and never go stale on rename.
- **Reinforces I3 (dumb codegen).** Hashing is a sema/front-end concern;
  codegen is unaffected.

---

## 6. Implementation sketch (not a plan)

- **Normalization pass** after parse: strip formatting/comments, alpha-rename
  locals to positional slots, canonicalize so semantically-identical bodies
  hash equal. (Hardest and most spec-worthy piece — and it overlaps `jet fmt`,
  which already canonicalizes formatting under one-mechanical-path; reuse that
  machinery rather than build a parallel normalizer.)
- **Hash + store**: content hash via the existing std-only `Source/SHA256.rs`
  (do **not** pull in `blake3`/`sha2` — I6); an out-of-tree store under
  `~/.cache/jet/` mapping hash → definition and name → hash. **Not `.jet/`** —
  U7 (file-is-a-program) forbids `jet run foo.jet` from needing any in-tree
  ecosystem dir; `.jet/` is project-mode only. `Source/BuildCache.rs` already
  works exactly this way and is the L1 seed.
- **Resolver**: name lookup goes name → hash → definition; unchanged hashes
  short-circuit compile and test.
- **Text projection** (Levels 2–3): render any hash back to canonical Jet
  text so diff/review/grep keep working.

External-crate rules (I6) are satisfied by reusing `SHA256.rs` — this stays
std-only compiler-internal Rust.

## 7. Open decisions for the owner (future ballot rows)

1. **Altitude.** Level 1 (build cache only), 2 (stable names + versions), or 3
   (full codebase)? Determines everything below.
2. **Canonical form.** Does text remain the source of truth (store is a
   derived cache) — strongly recommended to keep the file-is-a-program tenet —
   or does the store become canonical (Unison)?
3. **Normalization spec.** What does the hash ignore (formatting, comments,
   local names) and what does it preserve? This is a real, testable spec
   surface and must be pinned like a diagnostic.
4. **Version coexistence.** Should two hashes of "the same" library be allowed
   in one build (kills dependency hell) or rejected (one version per name)?
   Note coexistence ≠ interop: the two are different types and values can't
   cross between them.
5. **Hash-format stability.** If normalization or the AST changes between
   compiler versions, every hash moves. Harmless for an L1 cache (just
   invalidation) but fatal at L2/L3 where the hash *is* identity — it silently
   breaks "old code resolves to the exact bytes it always did." Unison versions
   its hash format explicitly; Jet must decide whether to pin/version it.

## 8. Recommendation

Adopt **Level 1 now** — an invisible win (caching, incremental builds,
test-skipping) that needs **no new syntax** and largely already exists in
`BuildCache.rs`. Its one gate is the normalization rule, which must be
spec-pinned and tested (decision #3) before it can be trusted not to skip a
failing test. Treat **Level 2** as a serious post-cache design tied to the
package story, gated on the canonical-form and hash-stability decisions (#2,
#5) so it never violates file-is-a-program. Hold **Level 3** as a north-star
reference, not a v1 target — its marquee wins (free rename, conflict-free
merges, the psychology payoff) are exactly the ones the file-is-a-program tenet
forecloses below L3. Each numbered decision above is a ballot row, not a build
step.

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
