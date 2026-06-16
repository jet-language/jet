# To-Do List

## Prompt

 You are implementing a ratified language feature in the jet compiler. Read these first, in order:
    1. CLAUDE.md (operating manual — invariants I1–I8, the nix dev-shell command rules, the workflow loop, the syntax-decision protocol)
    2. docs/plans/fan-out-and-fixed-size-lists.md  ← THIS IS THE SPEC. The owner has ratified it. Implement exactly what it says.
    3. docs/spec/diagnostics.md (error voice + the I4 snapshot rule)
    4. docs/spec/philosophy.md (settles any judgment call)

  Goal: ship the fan-out operator `f.[a, b, c]` ≡ `[f(a), f(b), f(c)]` and the fixed-size list type `[T#N]`, per the design doc. Package list sugar `default.[ripgrep, fd]` becomes one instance of this — it supersedes the old "Stage 1b = Pkg sugar" idea.

  Hard constraints (violating one = stop and fix):
    - Run everything through the nix dev shell: `nix develop -c cargo test`, etc. No host cargo/rustc.
    - Test-first: write the failing test (ui fixture or example) BEFORE the implementation, for every stage.
    - I1 no `unsafe`. I3 codegen stays dumb — `[T#N]` is a COMPILE-TIME refinement enforced in sema; at codegen it erases to the normal list (Vec). All fixed-size checks (destructure length, index bounds, no-grow) live in sema, never codegen.
    - I4 every new diagnostic (E0961–E0965 in the doc) needs a docs/spec/diagnostics.md entry + a tests/ui/*.stderr snapshot, blessed with `nix develop -c env UPDATE_EXPECT=1 cargo test` only after you read the output against the diagnostics voice.
    - I6 std-only, zero new crates.
    - I7 every new token/sigil (`.[`, `#`, the `[T#N]` type form) goes in src/syntax.rs with a decision ID.

  Step 0 — ratification bookkeeping: add the surface tokens to src/syntax.rs, add the decision rows + decision-log entries to docs/spec/syntax-decisions.md (cite the design doc), and extend tests/decisions.rs if it enforces the new IDs.

  Then build it test-first in the 5 stages in §6 of the design doc:
    1. Type + parser only (Type::FixedList; parse `[T#N]` in type position; parse `.[ … ]` into a new Expr::FanOut; add exhaustiveness arms — no-op/erase — across codegen/sema/fmt/lsp). The module-parser commit 2b3825e is a worked example of how to add an Item/Expr variant and chase down every match site.
    2. Fan-out sema (one-arg callable; items elaborated against the parameter type, mirroring the existing enum-unit-variant resolution at syntax-decisions.md:254; homogeneity; splicing). Diagnostics E0961/E0962.
    3. Fixed-size sema (val/var rule, one-way widening coercion, `.map` preserves N, `.len` const, destructure length check E0963, no-grow E0964, const-index bounds E0965).
    4. Codegen (fan-out → build a Vec by mapping f; `[T#N]` erases to Vec).
    5. An examples/features/*.jet example + expected output exercising fan-out, fixed-size destructure, `.map` preservation, and the package-list use (I5 golden-tested).

  Done means: `nix develop -c cargo test` fully green, the new example runs, docs (spec.md, syntax-decisions.md, diagnostics.md) match behavior, no invariant bent. Commit per stage with a clear message ending in the Co-Authored-By trailer from CLAUDE.md. If you hit a genuine surface-syntax gap not covered by the design doc, STOP and follow the syntax-decision protocol — do not invent syntax.



  You are continuing the jet "unified ecosystem" build (jetpack = the package manager / nix-shell-and-NixOS replacement, built on the jet language). Read these first, in order:
    1. CLAUDE.md (invariants, nix dev-shell rules, workflow, syntax-decision protocol)
    2. docs/plans/jetpack-jetos/README.md and docs/plans/jetpack-jetos/unified-ecosystem.md (the architecture, the U1–U7 decisions, env.jet/config.jet/pack.jet, the hangar store, the module tree)
    3. The agent memory index (MEMORY.md) — especially: packjet-migration-sequencing, computed-modules-pure-eval-shifted-up, jetpack-jetos-track, do-it-right-measure-twice, owner-design-kill-criteria.

  PRECONDITION: the fan-out operator + fixed-size lists (docs/plans/fan-out-and-fixed-size-lists.md) must already be merged — `packages: [default.[ripgrep, fd]]` depends on it. Verify `nix develop -c cargo test` is green before starting.

  What's already landed (don't redo): module-declaration parser (Item::Module, commit 2b3825e, Stage 1a); canonical §6 merge engine src/jetpack/merge.rs (sources/packages/scalars, default/force priority, conflict diagnostics); Jet-syntax pack.jet package-manifest PARSER src/jetpack/packmanifest.rs (unwired); provider@target classifier src/jetpack/refspec.rs.

  Owner-ratified decisions to honor:
    - The `pack.jet` → `env.jet` rename is a CLEAN BREAK — no back-compat alias (see packjet-migration-sequencing). Do the rename FIRST (frees the name), THEN retire jet.toml
  - The `pack.jet` → `env.jet` rename is a CLEAN BREAK — no back-compat alias (see packjet-migration-sequencing). Do the rename FIRST (frees the name), THEN retire jet.toml into the pack.jet manifest. Doing it the other way makes pack.jet mean two things at once.
  - Full COMPUTED modules: module fields may hold expressions, evaluated via pure-eval. Pure-eval = the existing M9.5 comptime tree-walking interpreter (src/comptime.rs) extended to whole module bodies — NOT a new engine; its differential battery (tests/comptime_diff.rs) is the safety net. (see computed-modules-pure-eval-shifted-up)
  - The unified lockfile is `.jet/lock` — reconcile with the existing graph format in src/lock.rs; do NOT invent a second lock format.

Work, in order, each test-first and as its own commit:
  1. Rename the jetpack env directive file pack.jet → env.jet (src/jetpack/packfile.rs and the ~1035 lines of tests in tests/pkg.rs). Clean break.
  2. Module evaluation (Stages 2–4 of the computed-modules arc): extend src/comptime.rs to reduce a module's contribution expressions (incl. computed `if … { } else { }`) to values; feed the evaluated env/system/image contributions into merge.rs; surface the U5 conflict diagnostics with I4 snapshots.
  3. Wire src/jetpack/packmanifest.rs in as the compiler manifest, retiring jet.toml/jet.lock into pack.jet + .jet/lock. Migrate tests/pkg.rs.
  4. Drive env.jet end-to-end: a real example project (env.jet with `imports: find("./modules")` and `packages: [default.[…]]`) realized through the hangar store.

Hard constraints: nix dev shell for all commands; std-only (I6); no `unsafe` (I1); codegen/eval checking lives in sema/comptime, never "try rustc and see" (I3); every diagnostic gets a code + ui snapshot (I4); examples are the executable spec (I5). The owner works "measure twice, cut once" — if a step turns out to need a surface-syntax or architecture decision that isn't already ratified in the docs, STOP and follow the syntax-decision protocol (add an Open Decisions row, build something else meanwhile). Don't guess on owner-facing syntax even when the feature was requested.

Done means each step: `nix develop -c cargo test` green, docs updated to match behavior, no invariant bent, committed with the CLAUDE.md Co-Authored-By trailer.

## End Prompt

- Change import to use
- Support for labeled loop "blocks"?
- Ensure we support multiple constructor types
- Relook module implementation & pack.jet @docs/plans/jetpack-jetos/unified-ecosystem.md
- Named + default arguments: Swift, Kotlin, Gleam (labels), Python, C#, Ruby. Big readability/beginner win. (§23)
- REPL Support
- Pipelines (|>): F#, Elixir, Gleam, Elm, OCaml, Julia. (§15)
- A cleanup primitive (defer/errdefer): Go, Zig, Odin, Swift, Nim, Hare. Recurs constantly; pairs naturally with transact. (§0.1)
- Optional-chaining / unwrap ergonomics (?., ??, guard/if let): Swift, Kotlin, C#, Dart. Jet has T?/or; round it out. (§12)
- Digit separators in numeric literals (1_000_000): Julia, Rust, Swift, Kotlin, Go, Ada, many. Free readability win. (§34)
- Atomic/transactional rollback (owner-flagged): Verse. (§0.1)
- Purity tracking (pure/func vs impure): Flix, Nim, Koka, D. Confirms S60. (§5)
- Content-addressed artifacts (not identifiers): Unison, Nix. Feeds jetpack. (§0.2)

### 0.1 Verse — `transact` / atomic functions with rollback ★ owner wants this

**What it is.** Verse (Epic's functional-logic language) has a *transactional*
effect. A function or block marked `transacts` may speculatively mutate state;
if execution **fails** (Verse's failure is first-class, not an exception),
every effect performed inside the transaction is rolled back as if it never
happened. It composes with Verse's `if`/`for` (which are choice/search
constructs): a branch that fails leaves zero side effects behind.

```verse
# Verse-ish: attempt a move; if any step fails, the whole thing undoes.
TryMove(player, target) : void = transacts:
    Spend(player.Stamina, 10)   # mutates
    Step(player, target)        # may fail
    # if Step fails, the Spend above is rolled back automatically
```

**Why it's interesting.** It turns "leave the world consistent on error" — the
single hardest thing to get right in imperative code — into a language
guarantee. No manual undo stacks, no `defer cleanup`, no half-applied state.
This is exactly the kind of *correctness made free* that Jet's beginner-first
priority loves: the classic bug (mutate three things, fourth step fails, now
your data is corrupt) simply cannot happen.

**Fit for Jet.** Strong philosophical fit, real implementation cost.

- Jet already has first-class fallibility: `T ? E` results and the `?`
  propagation operator (S7/S34). A transactional block is a natural partner —
  "if a `?` inside this block short-circuits, undo the mutations."
- Jet's value semantics make rollback *tractable* in a way it isn't for a
  reference-heavy language: mutation happens through `mut` borrows (S10) on
  owned values. A transaction can snapshot the bindings it will mutate
  (`clone` is already the explicit-copy primitive) and restore them on failure.
  No GC, no STM runtime, no hidden boxing required for the common case.
- **Honest costs / open questions** (these are why this is a *proposal*, not a
  ratified design):
  1. **What gets rolled back?** Pure in-memory bindings: yes, by snapshot.
     But I/O already performed (a file written, a line printed, a packet
     sent) cannot be un-done. Verse sidesteps this because its effect system
     forbids non-transactional effects inside a transaction. Jet would need a
     rule: a `transact` block may only mutate *local owned state*, and calling
     anything that does I/O inside it is a compile error (e.g. a new `E12xx`
     pointing the user at doing I/O after the block commits). That keeps the
     guarantee honest and the model explainable in two sentences.
  2. **Snapshot cost vs. priority #3 (performance).** Snapshotting clones the
     touched state. For big structures that's not zero-cost. Mitigation: only
     snapshot bindings the block actually mutates (the borrow checker already
     knows them), and document that `transact` trades a copy for safety —
     opt-in, never on the default path.
  3. **Interaction with `take` (move).** A value moved out inside a failed
     transaction must be moved back. The ownership checker already tracks
     moves precisely, so this is mechanical but must be specced.
- **Invariants:** clean. No `unsafe` needed (I1) — it's snapshot + restore in
  safe Rust. Codegen stays dumb (I3): sema decides what to snapshot, codegen
  emits the clone/restore scaffolding. Needs its own diagnostics + snapshots
  (I4).

**Verdict: `adapt` — strong candidate, owner-gated syntax.** Recommend opening
a decision row. Sketch of the surface (for the owner to react to, *not*
ratified):

```jet
// Sketch A — block form, ties into ? propagation:
fn try_move(player: mut Player, target: Point) -> Bool ? MoveError {
    transact {
        player.spend_stamina(10)?;   // mutates player
        player.step(target)?;        // may fail → whole block rolls back
    }
    return ok(true);
}
```

### 1. Stability and release policy (cheapest, highest leverage, pure docs)

Enterprises adopt promises, not features. Needed before anything else:

- A written backward-compatibility guarantee post-1.0.
- A deprecation policy and release cadence; eventually an LTS
  designation.
- Rust's **edition system** is the model worth stealing — it preserves
  the simplicity ratchet *and* allows fixing mistakes.
- Explicit licenses for the compiler and, critically, a statement that
  **generated code carries no license obligations**.

### 2. A debugger

DAP source maps are currently "deferred past v1.0" — for industry this
is the wrong shelf. No enterprise team ships a language its developers
cannot step through. Since Jet transpiles to Rust, the pragmatic v1 is
line-directive-style source mapping so gdb/lldb/VS Code show Jet source
lines, not generated Rust. Recommendation: promote to the
committed-additions list in docs/spec/roadmap.md.

### 3. Supply-chain features in M12 Phase 2

When the registry lands, enterprises will require:

- Private/internal registries and mirror support (Artifactory/Nexus
  proxying).
- Vendoring for air-gapped builds.
- SBOM emission (CycloneDX/SPDX — nearly free given the lockfile).
- Namespace ownership rules.
- An advisory database and a `jet audit` command.

None of this conflicts with the existing M12 design; it is mostly
Phase 2 scope.

### 4. Observability stdlib

M10 has fs/io/json but nothing for production operations. Minimum bar:
structured logging in `std/log`. Eventually metrics and trace-context
propagation — but logging alone covers most CLI/tool use cases, and it
should exist before anyone runs Jet in production.

### 5. A server-side story

Committed-addition item 5 (blocking sockets + HTTP *client*) covers
tools, but enterprise bread-and-butter is services:

- An HTTP **server**.
- TLS — bridge to rustls via the FFI tier; never hand-rolled.
- Database connectivity (Postgres first; FFI to a vetted Rust driver).

This is also where "no async, tasks + channels only" gets
stress-tested. Thread-per-connection is fine for internal services at
hundreds of connections; hold that line for v1.x rather than reopen
async, but write the positioning down explicitly ("Jet services scale
like Go circa 2012; if you need 100k connections, that's not us yet").

### 6. Cross-compilation surfaced as a feature

rustc provides the target matrix nearly free — `jet build --target
linux-arm64` would be a one-flag enterprise feature (build on CI for
the deploy target) that is mostly inherited. Cheap to add to M6 or M14
scope.


Consider The Following: 
1. Transparent alias — a second name for the same type. No new type; the compiler treats them as identical. Used to make long types readable:
type OrderBook = Map<String, [Order]>;   // alias
fn settle(book: OrderBook) -> Money { ... }   // vs Map<String, [Order]> everywhere
Liked: shortens noisy generic types, documents intent at a glance. Disliked: can over-abstract — a reader sees OrderBook and has to jump to its definition to learn it’s “just a map”. Because it’s transparent, it gives zero extra safety: you can still pass any Map<String, [Order]> where an OrderBook is expected.
2. Newtype (distinct type) — a brand-new type wrapping one value, not interchangeable with what it wraps. Used for safety:
struct UserId(Int);       // (Rust-style) — UserId and ProductId are now
struct ProductId(Int);    //   different types even though both wrap Int

fn ban(u: UserId) { ... }
ban(product.id);          // COMPILE ERROR — can't pass a ProductId
Liked: kills a whole bug class — you can’t accidentally swap two Ints that mean different things (user id vs product id, meters vs feet). Disliked: adds wrapping/unwrapping ceremony, and you often want to forward some operations (arithmetic on a Meters) which means writing trait impls.
Why Jet can wait on both. The alias is pure convenience; the newtype is the valuable one (safety, priority #1-adjacent), but Jet can already get newtype behavior with a one-field struct once construction sugar settles, so a dedicated keyword isn’t urgent. Recommendation: defer both for v1; if demand appears, add the newtype (single-field struct is the natural spelling) before a transparent type alias. (No code change; this is the explainer the owner asked for.)
