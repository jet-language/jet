# Sidequest — Smart Context (implicit allocator + logger bundle)

**Status: plan — awaiting owner decision D-CTX1.**

Ballot: `tools/Tower/docs/ballots/_jai-ctx.md` (D-CTX1). Do not start implementation
until D-CTX1's Q1 (replace / complement / reject) is ratified; the answer reshapes the
whole sema design.

## Goal

Give beginners zero-ceremony, memory-safe allocation — they never name an allocator —
while letting experts swap the ambient allocator and logger for a lexical/dynamic block,
auto-restored on exit, affecting all downstream calls *including library code they didn't
write*. Borrowed from Jai `push_context` / Odin implicit `context`. The magic is invisible
to beginners (I1, philosophy: beginner experience is the resource we don't spend); the
swap is an explicit expert opt-in.

## The S58 interaction (must resolve before code)

S58 ratified **explicit Zig-style allocators** ("allocating APIs take an allocator
parameter"); D-ALLOC1 ratified `arena :: mem.Arena.new()` / `arena.alloc(value)`. An
implicit context **partly reverses** that explicit stance. This sidequest assumes the
recommended **A2 (Complement)**: explicit passing stays ratified and **wins when present**;
the context is only the default used when nothing is passed. If the owner instead picks:

- **A1 (Replace):** allocating APIs lose their allocator parameter; D-ALLOC1's
  `arena.alloc` stays as the *explicit* escape but the parameter-threading of S58 is
  retired — larger sema + doc churn, and a reversal that must be logged in
  syntax-decisions.md, not done silently.
- **A3 (Reject):** this sidequest is closed; no work.

Precedence rule under A2 (one sentence, the teaching line): **a passed allocator always
beats the ambient context allocator.** Sema resolves an allocating call by: explicit `in:`
arg → else current ambient context.

## Sema work

Thread an **implicit context parameter** through the call graph, invisible in source:

1. **Context type.** An internal `Context` struct (not user-spellable in v1 beyond the
   swap form): `{ allocator, logger }`. Hold the field set minimal — see v1 scope.
2. **Implicit param injection.** Every Jet function gains a hidden context parameter in
   the lowered signature (sema-level, then codegen). Pure leaf functions that neither
   allocate nor log nor call anything that does **may skip it** (optimization; correctness
   never depends on it).
3. **Allocation/log resolution.** `[]`, `.push`, list/map growth, `print`/`log.*` resolve
   their allocator/logger from the ambient context unless an explicit allocator is passed
   (A2 precedence). Reuse the existing default-heap path as the *default context value*.
4. **Scoped swap (`#context(field = v, …) { … }`, pending Q2).** Lowers to: snapshot
   current context → overlay named fields (copy-on-write, Odin-style, so the swap is
   *local* and cannot back-propagate to the caller) → run block with the new context as
   ambient → restore on **every** exit path (normal, `?` propagation, `break`/`return`,
   panic-unwind — ties into S63 RAII and S15 unwinding).
5. **Capability/effect seam (NOT v1).** c06 capabilities and D-EFF1 effects will want the
   context as their carrier later. Design the `Context` struct so adding fields is additive,
   but ship none of it now.

## Codegen options

Codegen stays dumb (I3) — sema decides everything; codegen only emits the chosen carrier.

- **Option 1 — hidden parameter.** Pass `&Context` (or `&mut`) as a real extra Rust
  parameter on every lowered function. Pro: explicit, no global state, plays with threads
  trivially, matches Odin's "pass by pointer per call." Con: touches every signature;
  monomorphization/closure (S47) capture must carry it.
- **Option 2 — scoped thread-local.** A `thread_local!` holding the current context;
  `#context` saves/overlays/restores it with a guard whose `Drop` restores on unwind
  (S63/S15). Pro: zero signature churn; swap is a stack-discipline save/restore. Con:
  hidden global mental model (kept backend-only per S58 precedent — users never see it);
  must get unwind-safety and the future task/channel (S53) boundary right.

Lean: **Option 2** for v1 (smallest surface, no pervasive signature change), with the
thread-local strictly a backend detail. Revisit Option 1 if S53 concurrency exposes
cross-task context bleed.

## v1 scope (resist creep)

- Bundle is **allocator + logger only.** Nothing else. No telemetry spans, no permissions,
  no capability tokens, no effect rows — those are c06 / D-EFF1 and come back as their own
  cards.
- One swap form (Q2 winner). No user-defined context fields. No reading the context value
  as a first-class value beyond the swap.
- A2 precedence (explicit beats ambient) only; A1/A3 reshape or cancel this scope.

## Risks

- **R1 — Leaks to beginners.** The single hardest constraint: a beginner must *never* see
  "context", "allocator", or the hidden param in any error, hover, or formatter output
  (mirror S58 onboarding). A diagnostic that mentions the context for beginner-tier code
  is a P0. All `#context` material is expert-tier docs only.
- **R2 — Silent S58 reversal.** If A1 is chosen, the explicit-stance change MUST be logged
  in syntax-decisions.md as an amendment to S58/D-ALLOC1 — never an undocumented drift.
- **R3 — Unwind/early-exit restore.** Failing to restore on `?`, `return`, `break`, or
  panic corrupts the ambient context for the rest of the program. Restore must be RAII-guard
  based (S63), tested on every exit path.
- **R4 — Library transparency double-edge.** The feature's power (library code picks up the
  swap) is also a footgun if a library cached an allocator. v1 has no caching APIs; flag for
  when stdlib grows them.
- **R5 — Concurrency (S53).** Thread-local context + tasks/channels = which task sees which
  context? Out of v1 scope but must not be designed into a corner — keep the carrier swappable.

## Open questions

- Q-a: Under A2, does an explicit `arena.alloc(v)` *inside* a `#context(allocator = other)`
  block use `arena` (explicit wins — recommended) with zero ambiguity? Confirm the teaching
  line covers it.
- Q-b: Is the swap **dynamic-extent** (Racket `parameterize`, follows the call graph) or
  **lexical** (only syntactically-nested code)? Dynamic-extent is what makes library code
  reroute — almost certainly required; confirm against codegen Option 2.
- Q-c: Copy-on-write depth — does swapping `allocator` alone leave `logger` pointing at the
  outer one (yes, per A2 worked example)? Confirm partial overlay semantics.
- Q-d: Does `#context` nest, and does an inner block restore to the *immediately* enclosing
  context, not the global default? (Yes — stack discipline.)
- Q-e: Logger interface for v1 — what's the minimal `log.*` surface and a `Silent` logger,
  and does it belong in `core` now or wait?

## Test plan

- UI/golden: a beginner program that allocates and prints, with **no** context mention
  anywhere in output (R1 guard) — assert the words never appear.
- Example + expected output (I5): expert swaps allocator to an arena for a block; downstream
  library call allocates in the arena; arena freed; values built before the block unaffected.
- Restore-on-every-exit: tests that force `?` propagation, `return`, `break`, and a caught
  panic out of a `#context` block, each asserting the ambient context is the outer one
  afterward (R3).
- Explicit-beats-ambient (A2): `arena.alloc(v)` inside a `#context(allocator = other)` block
  lands in `arena`, not `other` (Q-a).
- Nesting: nested `#context` blocks restore to the correct enclosing context (Q-d).
- Differential (if any comptime path touches it): none expected in v1 — context is runtime.
