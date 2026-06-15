# To-Do List

## Prompt

> Implement S73 (named-only tuples), ratified in `docs/spec/syntax-decisions.md`.
> Read the shared context above first.
>
> **Spec:** lightweight aggregates with **named members only**:
> `val p = (x: 1, y: 2);`, member access `p.x`, and usable in type position
> `fn bounds() -> (min: Int, max: Int)`. **Positional tuples and `.0` access are
> rejected** (`.0` collides with float-literal lexing). Field order is not
> significant for type identity — `(x: Int, y: Int)` and `(y: Int, x: Int)` are
> the same type; canonicalize by sorting fields by name.
>
> **Suggested design (keeps codegen dumb, I3):** lower each distinct tuple shape
> to a generated Rust struct so member access reuses ordinary struct-field
> codegen — no type-routing needed in the `Expr::Field` arm.
> - `ast.rs`: add `Type::Tuple(Vec<(String, Box<Type>)>)` (store sorted) and
>   `Expr::TupleLit(Vec<(String, Expr)>, Span)`; add to `Expr::span()`.
> - `parser.rs`: in the primary-expression `(` path, look ahead for
>   `ident :` to choose tuple-literal vs parenthesized-expr vs (if S74 lands)
>   tuple-destructuring. In type position, parse `( ident : Type , … )`.
>   Disallow a 1-element tuple if it's ambiguous with grouping — decide and
>   document. Reject positional form (`(1, 2)`) with a teaching error pointing at
>   named members.
> - `sema.rs`: infer `TupleLit` → `Type::Tuple` (sorted); extend `field_type`
>   with a `Type::Tuple` arm so `p.x` resolves; `Type` already derives
>   `PartialEq`/`Eq`, so equality is automatic once fields are sorted. Add a
>   teaching diagnostic for `.0`/positional access.
> - `codegen.rs`: maintain a registry of distinct tuple shapes; emit a
>   `#[derive(Clone, …)] struct JetTup_<stable-hash> { … }` per shape; lower
>   `TupleLit` to that struct literal and `Type::Tuple` to the struct name.
>   Member access falls through existing `Expr::Field` codegen unchanged.
> - `fmt.rs`, `lsp.rs`, `comptime.rs`: handle the new `Expr`/`Type` variants
>   (fmt prints `(x: 1, y: 2)` / `(x: Int, y: Int)`; lsp `collect_expr` recurses;
>   comptime can reject or eval as needed).
> - **Tests/docs:** `tests/fmt.rs` round-trip; a `tests/ui` snapshot for the
>   positional-tuple / `.0` teaching error (give it the next free `E00xx`, add it
>   to `diagnostics.md` and to `is_teaching_parse_diag` in `parser.rs` if it's a
>   recoverable parse diag); a runnable example (per owner: examples are
>   currently hands-off — confirm before adding under `examples/`); mark S73
>   **implemented** in `syntax-decisions.md`. Verify with `/tmp` scratch files
>   that literal, type position, member access, and equality all work and that
>   generated Rust compiles (I2).

- Change import to use
- Relook module implementation & pack.jet @docs/research/functional-pack-debrief.md
- REPL Support
- Pipelines (|>): F#, Elixir, Gleam, Elm, OCaml, Julia. (§15)
- Named + default arguments: Swift, Kotlin, Gleam (labels), Python, C#, Ruby. Big readability/beginner win. (§23)
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
