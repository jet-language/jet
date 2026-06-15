# Feature Considerations (cross-language idea bank)

**Status:** research / owner reading material. Nothing here is ratified.
This file surveys unique features from other languages and judges each one
against the ranked priorities in `00-philosophy.md` and the invariants in
`CLAUDE.md`. It exists so the owner can decide *what* to pull into Jet; it
does **not** invent surface syntax. Any item the owner likes becomes an Open
Decision row in `02-syntax-decisions.md` first, then code (syntax protocol).

How to read each entry:

- **What it is** — the feature, briefly.
- **Why it's interesting** — what problem it solves elegantly.
- **Fit for Jet** — measured against priorities 1–6 and invariants I1–I8.
- **Verdict** — `adopt` / `adapt` / `defer` / `decline`, with the reason.

Verdicts are recommendations to the owner, not decisions.

---

## 0. Owner's two flagged features (deep dives)

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

Decision points for the owner: keyword (`transact` vs `atomic` vs `rollback`);
block vs. function-level annotation (`fn ... transacts`); whether failure is
only `?`/`err`, or also `panic`/`require`; the I/O-prohibition rule above.
This belongs in `02` as a new `Sxx` row before any code is written.

### 0.2 Unison — content-addressed (hashed) code ★ owner likes this

**What it is.** In Unison, every definition is identified by the hash of its
*content* (its normalized syntax tree), not by its name. Names are just
metadata in a "codebase" database that maps human labels → hashes. Therefore:

- **Renaming is free and instantaneous** — you change a name→hash row; no
  callers change, because callers reference the hash.
- **No dependency conflicts** — two libraries can use different versions of the
  same function; they're different hashes, both present, no diamond problem.
- **No rebuilds of unchanged code** — if the hash is unchanged, it's already
  compiled/typechecked; the result is cached forever.
- **Trivial structural sharing / caching of test results** — a test keyed on a
  hash never needs re-running unless the hash changes.

```unison
-- You edit names, but the *reference* under the hood is a hash like #a8f3b2…
factorial : Nat -> Nat
factorial n = if n == 0 then 1 else n * factorial (n - 1)

-- `rename factorial fact` is an O(1) metadata edit. Zero call sites touched.
```

**Why it's interesting.** It dissolves three perennial pains — renaming churn,
dependency hell, and redundant rebuilds — by attacking *identity* itself.
That's a genuinely deep idea, and the owner is right to be drawn to it.

**Fit for Jet.** Philosophically appealing, architecturally enormous, and in
direct tension with a ratified Jet tenet.

- **Conflict with "a file is a complete program" (philosophy, R9).** Unison's
  model *requires* a codebase database (`.unison/`) and an interactive
  `ucm` tool; you don't really edit "files that are the program," you edit a
  scratch file and commit definitions into the database. Jet's distribution
  tenet is the opposite: `jet run foo.jet` with no project, no manifest, no
  store. Adopting content-addressing wholesale would *dictate a file/project
  structure*, which the owner has repeatedly declined (see the memory note on
  kill criteria; cf. the rejected `pub { }` grouping in S18). This is the
  single biggest reason not to copy it into the core language.
- **Beginner experience (priority #2).** "Where did my code go? It's in a
  database I query with a tool" is a hard first-90-seconds story. Unison is
  loved by experts and bewildering to newcomers — the inverse of Jet's bar.
- **Where it genuinely fits Jet anyway:** *under the hood*, not in the user's
  face.
  1. **Incremental compilation cache.** Jet can content-hash normalized
     definitions internally to skip re-typechecking/re-codegen of unchanged
     functions — all the rebuild-avoidance win, none of the model change. The
     user still just edits files. Pure compiler optimization (priority #5),
     invisible.
  2. **`jetpack` content-addressing.** The package/dev-shell track already
     leans on a content-addressed store (Nix-style; see jetpack plan). Hashing
     *packages/build outputs* by content is squarely in scope and gives the
     "no rebuilds, perfect caching, no version conflicts at the artifact
     layer" benefits where they belong — at the package boundary, not the
     identifier boundary.
  3. **Refactor tooling.** "Rename is free" can be approximated for users by a
     rock-solid LSP rename (M13) that updates all call sites atomically. Not as
     elegant as hashes, but it delivers the felt benefit inside the file model
     Jet has chosen.

**Verdict: `decline` for the language core; `adapt` underneath.** Do not make
Jet identifiers content-addressed (it breaks the file-is-the-program tenet and
the beginner story). *Do* mine the idea for (a) an internal incremental-build
hash cache and (b) jetpack's content-addressed artifact store, and lean on LSP
rename to give users the felt "renaming is cheap" experience. Worth a short
note in the architecture doc rather than a syntax decision.

---

## 1. Gleam

Small, friendly, ML-family language on the BEAM (and JS). Closest sibling in
*spirit* to Jet: tiny, opinionated, beginner-respecting, great errors.

- **`use` for callback flattening.** Gleam's `use` desugars nested
  continuation callbacks into flat code — its answer to the pyramid of doom and
  to do-notation, without monad jargon.

  ```gleam
  pub fn main() {
    use user <- result.try(fetch_user())
    use posts <- result.try(fetch_posts(user))
    Ok(render(user, posts))
  }
  ```

  **Fit:** Jet already gets most of this from `?` propagation (S7). `use`
  generalizes beyond Result (any "take a callback last" function). Interesting
  but overlaps `?`; revisit only if Jet grows resource-scoping needs.
  **Verdict: `defer`.**

- **Everything is an expression; `case` is exhaustive.** Matches Jet's
  `switch` direction (S24). **Verdict: already aligned.**

- **Labelled arguments** (`f(name: x)` at the call site, with reorderable
  labels). Strong readability win and a known Jet open area. **Verdict:
  `adapt` — feed into the named-argument decision (see gallery §23).**

- **Pipelines `|>`.** Clean left-to-right data flow. See gallery §15; Jet
  should decide a pipe story. **Verdict: `adapt`.**

- **No early `return`, no exceptions, no null.** Gleam leans hard on
  exhaustiveness + Result. Jet keeps `return` (beginner familiarity) but shares
  the no-null/no-exceptions stance. **Verdict: aligned.**

## 2. Julia

Dynamic, scientific, JIT-compiled. Most of Julia's identity (multiple
dispatch, dynamism) cuts against Jet's static/AOT model, but a few surface
ideas are gold.

- **Multiple dispatch as the core paradigm.** Powerful but conflicts with
  Jet's smallness + static simplicity; trait-based dispatch (S28) covers the
  beginner-relevant slice. **Verdict: `decline` as a paradigm.**
- **Unicode + LaTeX identifiers/operators** (`α`, `∈`, `≈`). Lovely for math,
  a footgun for beginners and tooling. **Verdict: `decline`.**
- **Numeric literal ergonomics:** `1_000_000`, `2x` (implicit multiply),
  rational `3//4`, arbitrary-precision built in. Digit separators are a free
  win (gallery §34). Implicit multiply is too clever for priority #2.
  **Verdict: `adopt` digit separators; `decline` juxtaposition multiply.**
- **`1:10` ranges and broadcasting `f.(xs)`.** Broadcasting (apply elementwise
  with a dot) is elegant but novel; map/iteration covers it for beginners.
  **Verdict: `defer` broadcasting.**
- **First-class `@macro` metaprogramming.** Macros are a v1 non-goal.
  **Verdict: `decline` (v1).**

## 3. Verse

Beyond `transact` (§0.1), Verse has more worth knowing:

- **Failure as a first-class control-flow value.** Expressions can *fail*
  (produce no value) rather than throw; `if`, `for`, and `?` are built on it.
  This unifies "optional, search, validation, and control flow." Beautiful, but
  a whole paradigm; Jet's `Option`/`Result` + `switch` cover the beginner slice
  more legibly. **Verdict: `decline` the paradigm, `adapt` the transaction.**
- **Functional-logic `for` as comprehension + search.** Powerful, unusual,
  high learning cost. **Verdict: `defer`.**
- **Speculative execution / `rollback` semantics** — the part worth stealing,
  covered in §0.1.

## 4. Unison

Beyond content-addressing (§0.2):

- **Algebraic effects / *abilities*.** Effects (IO, state, exceptions, async)
  are tracked in the type as *abilities* and handled by *handlers* — like
  typed, composable, resumable exceptions. This is the most important "next
  big idea" in language design and shows up again in Koka, Flix, and Effekt
  (below). It can model async, generators, dependency injection, and
  transactions *with one mechanism*.

  ```unison
  -- `{IO}` in the type says this needs the IO ability.
  greet : Text ->{IO} ()
  greet name = printLine ("hi " ++ name)
  ```

  **Fit for Jet:** genuinely tempting (it could subsume `transact`, async, and
  more), but it's a large, expert-flavored feature that fights priority #2 and
  #4 (smallness). Async is already a v1 non-goal. **Verdict: `defer` — keep on
  the long-horizon radar; if Jet ever adds async/generators, evaluate an
  effects system *then* rather than bolting on `async` keywords. See §13/§14
  below and Koka/Flix.**

## 5. Flix

Principled research language: ML + first-class Datalog + an effect system that
distinguishes *pure* vs *impure* and tracks it in types.

- **Purity tracking in the type system.** Flix functions are pure unless
  marked, and the compiler enforces it — exactly the foundation Jet's
  `pure fn` (S60) is reaching for (the `jet eval --pure` config story). Flix is
  the reference design to study here. **Verdict: `adapt` — Flix is prior art
  for S60; mine its purity-effect rules.**
- **First-class Datalog (`solve`/fixpoint) constraints.** Astonishing for
  graph/relational logic, but a niche paradigm. **Verdict: `decline` (v1),
  fascinating later.**
- **Region-based local mutability.** Mutable state confined to a lexical
  region, statically. Conceptually close to Jet's owned-mutation model and
  relevant to how a `transact` block (§0.1) could be scoped. **Verdict:
  `adapt` as design input to `transact`.**

## 6. Strand

Strand is a 1980s **concurrent logic** language (a relative of Parlog /
Concurrent Prolog). Its signature idea:

- **Implicit, fine-grained dataflow concurrency via single-assignment logic
  variables.** You write what depends on what; any goal whose inputs are ready
  runs, in parallel, automatically. Synchronization is *the variable becoming
  bound* — no locks, no threads in the user's face. The "AND-parallelism"
  model influenced later dataflow systems.

  ```strand
  % Producer/consumer wired by a shared logic variable stream — no locks.
  main :- producer(S), consumer(S).
  producer([X|Xs]) :- X = 1, producer(Xs).
  consumer([X|Xs]) :- write(X), consumer(Xs).
  ```

  **Fit for Jet:** the *goal* — "concurrency that's safe and doesn't make
  beginners reason about locks" — is squarely on-philosophy, and Jet already
  has Shared-handle + concurrency work (see M11 foreign `async`/`Mutex`
  teaching errors). But Strand's logic-variable paradigm is alien to Jet's
  imperative/value model and to beginners. The *takeaway*, not the syntax:
  prefer **structured, dataflow-ish concurrency** (futures/channels with
  ownership-enforced safety) over raw threads + locks. **Verdict: `adapt` the
  philosophy (safe-by-construction concurrency, no manual locks), `decline` the
  logic paradigm.**

---

## 7. Other niche languages worth mining

### Roc (fast functional, descendant of Elm)
- **Tags / structural unions without declaration** — `Ok x`/`Err y` are just
  tags; functions accept any record/tag set with the right shape. Ergonomic,
  but structural typing fights Jet's nominal clarity for beginners.
  **`defer`.**
- **Best-in-class error messages and "no `null`, no exceptions"** — same north
  star as Jet. **Aligned.**
- **`!` for effect/Task desugaring** (Roc's take on flattening async/IO).
  Watch as prior art if Jet ever does effects. **`defer`.**

### Koka (effects-first research language)
- **Row-typed algebraic effects + handlers**, and **Perceus** reference
  counting that often achieves in-place mutation for functional code (zero-ish
  cost FP). Perceus is interesting for a future where Jet wants functional
  ergonomics without GC. **`defer`, study Perceus.**

### Grain (small WASM-first functional language)
- Tiny, friendly, ML-family, compiles to WASM. Good comparison point for Jet's
  "small + friendly + compiled" niche. No single must-steal feature.

### Nim
- **`func` (pure) vs `proc`** purity split (cf. S60). **Aligned/prior art.**
- **UFCS** — `x.f(y)` ≡ `f(x, y)`; any function callable as a method. Big
  ergonomics, big "two ways to do one thing" cost (priority #4). **`defer`,
  decide deliberately (gallery §3/§15).**
- **`a.f` / no-paren calls, significant-ish style flexibility, macros** — too
  many ways to write things for Jet's taste. **`decline`.**

### Crystal
- **Ruby-like syntax with full static typing + inference**, compiled, union
  types via flow analysis. Proof that "looks dynamic, is static and fast" sells
  — Jet's pitch too. **Aligned in spirit.** Union-by-inference is clever but
  less predictable than nominal sums; **`decline`** the implicit unions.

### Swift
- **Optionals + `if let`/`guard let`/optional chaining `?.`/`??`.** The most
  beginner-legible null-safety UX in industry, and Jet already shares the
  `T?` direction (S32). **`adapt`** the unwrap ergonomics (`?.`, `??`/`or` —
  Jet has `or`, S35; consider optional chaining). See gallery §12.
- **`guard` for early-exit with binding.** Very readable. **`adapt` candidate.**
- **Trailing closure syntax, `defer` blocks, named args by default.** `defer`
  (run on scope exit) is a clean cleanup primitive worth a decision; named args
  feed §23. **`adapt` candidates.**

### Kotlin
- **`when` expression** (Jet's `switch` cousin), **smart casts**, **data
  classes** (`derive`-like), **`?.`/`?:`**, **extension functions**, **named +
  default args**. A deep well of pragmatic ergonomics. **`adapt`** named/default
  args and smart-narrowing; **`defer`** extension functions (UFCS-adjacent
  smallness cost).

### Zig
- **`comptime`** (already Jet S57), **explicit allocators** (already S58
  expert tier), **`errdefer`**, **error sets `!T`** (Jet's `T ? E` cousin),
  **`defer`**, **labeled blocks/breaks that yield values**, **no hidden control
  flow / no hidden allocation**. Zig is the reference for the *expert low-level
  tier* the owner wants. **`adapt`**: `errdefer`/`defer` cleanup; labeled-block
  values; "no hidden allocation" as a stated principle.

### Odin
- **`or_else` / `or_return` / `or_continue`** — terse, readable error/Option
  fallbacks; Jet's `or` (S35) is in this family. **`adapt`** as prior art for
  the fallback family. **Multiple return values + named returns.** Jet uses
  Result/tuples instead; **`decline`** multi-return.
- **`defer`, `using`, distinct procedure groups.** `defer` again recurs —
  strong signal Jet should decide a cleanup primitive.

### Vale
- **Generational references / region borrowing** — a *third* memory model
  (between GC and borrow-checking) aimed at safety without lifetime ceremony.
  Directly relevant to Jet's "safe without lifetime syntax in Tier 1" bet.
  **`adapt` as research input** to the ownership model, not as syntax.

### Pony
- **Reference capabilities (`iso`/`val`/`ref`/`box`/`tag`)** for data-race-free
  actor concurrency, checked at compile time. The most rigorous "safe shared
  mutable concurrency" design out there; relevant to Jet's Shared-handle and
  concurrency story. **`adapt` as design input** (the capabilities themselves
  are too many concepts for beginners — priority #2/#4).

### Elm
- **Legendary compiler error messages** (the bar Jet's `04-diagnostics.md`
  aims at) and **enforced semantic versioning** (the compiler detects API
  breaks and forbids a non-major bump). Enforced semver is a brilliant idea for
  **jetpack**. **`adapt`** enforced-semver for the package manager; **aligned**
  on errors.

### F# / OCaml / ReasonML
- **`|>` pipelines** (F# popularized; see §15), **computation expressions**
  (F#'s general do-notation — too advanced for v1), **active patterns**,
  **structural records & variants**. F#'s `|>` is the cleanest pipe in
  industry — strong input to Jet's pipe decision. **`adapt`** pipe.

### Lean 4 / Idris (dependent types)
- **Dependent types, `do` notation that compiles to fast code, proof-carrying
  programs.** Far beyond Jet's scope, but Lean 4 proves a dependently-typed
  language can also be a *fast systems* language. Long-horizon curiosity only.
  **`decline` (v1).**

### Elixir / Erlang
- **Pattern matching everywhere (incl. function heads), `with` chains, the
  pipe `|>`, supervisors/let-it-crash.** `with` (Elixir) is another
  flatten-the-happy-path construct (cf. Gleam `use`, Jet `?`). Pipe again.
  **`adapt`** pipe; **`defer`** function-head pattern matching (overlaps
  `switch`, smallness cost).

### Carbon / Mojo (newer systems langs)
- **Carbon:** C++ interop-first, explicit over implicit; mostly a migration
  story, low novelty for Jet.
- **Mojo:** Python-superset + MLIR + `fn` (typed/strict) vs `def` (dynamic) +
  ownership (`borrowed`/`inout`/`owned`) + SIMD/`comptime`-ish parameters.
  Mojo's `fn`/ownership keywords (`inout`, `owned`) are a direct cousin of
  Jet's S10 (`mut`/`take`) — useful confirmation Jet's ownership-by-plain-words
  direction is industry-validated. **Aligned/confirmation.**

### V, Hare, C3
- **V:** opinionated, `mut` keyword for mutable params (like Jet S10), no
  null, no globals by default, fast compiles. Lots of surface overlap with
  Jet's goals — good sanity check that Jet's choices are reasonable.
- **Hare / C3:** minimalist C replacements; `defer`, tagged unions, `?`-style
  error handling recur yet again. Reinforces the same shortlist.

---

## 8. Recurring signals (what the survey keeps pointing at)

Features that showed up in *many* well-regarded languages and map cleanly onto
Jet's priorities — these are the highest-confidence candidates for Open
Decision rows:

1. **Pipelines** (`|>`): F#, Elixir, Gleam, Elm, OCaml, Julia. (§15)
2. **Named + default arguments:** Swift, Kotlin, Gleam (labels), Python, C#,
   Ruby. Big readability/beginner win. (§23)
3. **A cleanup primitive (`defer`/`errdefer`):** Go, Zig, Odin, Swift, Nim,
   Hare. Recurs constantly; pairs naturally with `transact`. (§0.1)
4. **Optional-chaining / unwrap ergonomics** (`?.`, `??`, `guard`/`if let`):
   Swift, Kotlin, C#, Dart. Jet has `T?`/`or`; round it out. (§12)
5. **Digit separators in numeric literals** (`1_000_000`): Julia, Rust, Swift,
   Kotlin, Go, Ada, many. Free readability win. (§34)
6. **Atomic/transactional rollback** (owner-flagged): Verse. (§0.1)
7. **Purity tracking** (`pure`/`func` vs impure): Flix, Nim, Koka, D. Confirms
   S60. (§5)
8. **Content-addressed *artifacts*** (not identifiers): Unison, Nix. Feeds
   jetpack. (§0.2)

For each, the next step is a row in `02-syntax-decisions.md` Open Decisions
with worked terminal/source examples per option (owner's decision-doc style),
then — only after ratification — code.

See `08-syntax-gallery.md` for side-by-side surface comparisons that inform the
*spelling* of any of these.
</content>
</invoke>
