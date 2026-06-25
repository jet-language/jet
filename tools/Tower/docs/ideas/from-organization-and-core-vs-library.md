# Idea cards — mined from two research notes

Source files mined (read-only, unchanged):
- `docs/research/jet-code-organization.md` ("Method Co-location & Code Organization")
- `docs/research/language-ideas-core-vs-library.md` ("42 Ideas, Sorted: Library vs. Core")

Each card: what it is (plain English) · where it came from · dedup status against
ratified decisions (`syntax-decisions.md`), the open ballot
(`decision-ballots.md`), board cards (`board.json`), sidequests/proposals, and the
roadmap · one CEO note.

Status legend: `NEW` (not found anywhere) · `ALREADY A CARD/PLAN` · `ALREADY IN
BALLOT` · `ALREADY RATIFIED` · `ALREADY IMPLEMENTED`. Several ideas are *partially*
covered — flagged as "PARTIAL" with the gap called out.

---

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
