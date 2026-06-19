# Decision ballots — open owner queue

Every decision waiting on the owner, and **nothing else**. The instant a
decision is ratified it leaves this file: delete the row, implement it, and
build it into its destination doc/code. No "recently ratified" section, no
tables of decided history — that clutter is what this file exists to avoid.
The ratified record lives in the decision log in
[`syntax-decisions.md`](syntax-decisions.md).

**House rule for whoever edits this file:** every decision below carries a
worked, user-story example for each option. The owner decides from concrete
artifacts — what a real person types, sees, and hits as an error — not from
abstract option tables. A bare ballot is not ready to show him. If you add a
decision, add its examples in the same edit.

---

## Next Tasks — open ballots

Eight decisions block the five queued "Next Tasks" sidequests. Each carries a
worked before/after per option. Recommendations follow the plan; where the plan
deliberately surfaces a choice without a pick, the card is marked **(no rec)**.

### D-ATTR1 — Move the attribute sigil `@` → `#`? (no rec)

Why it matters: reverses two ratified rejections (**S55** derive policy, **S82**
marker sigil) and resurrects the Rust-style spelling we already declined; `#` is
also already a live `Hash` token (`[T#N]`, `name#ver`). Pervasive user-facing
swap. The plan surfaces this; the owner picks.

- **Option A — Keep `@` (status quo).** No reversal, no churn; `#` stays the
  fixed-size/version-pin char only.

  ```jet
  // BEFORE and AFTER (unchanged):
  @unsafe
  fn raw() { … }

  @Serialize
  struct Point { … }
  ```

- **Option B — Move to `#`.** Re-allows `#`-prefixed markers; positional
  disambiguation keeps `[T#N]` and `name#ver` working; teaching error flips to
  reject `@unsafe` and teach `#unsafe`.

  ```jet
  // BEFORE (@):           // AFTER (#):
  @unsafe                  #unsafe
  fn raw() { … }           fn raw() { … }

  @Serialize               #Serialize
  struct Point { … }       struct Point { … }
  ```

**Recommendation:** none — the plan states "agent surfaces the decision; owner
picks." Decisions D-ATTR2/D-ATTR3 only apply if this is Option B.

### D-ATTR2 — List form: bare `#Name` vs Rust-literal `#[derive(…)]`? (no rec)

Why it matters: only live if D-ATTR1 = B. Picks the multi-marker spelling, and
whether to resurrect the literal `#[derive(…)]` form S55 rejected.

- **Option A — Bare `#[Marker, …]`.** Markers listed plainly inside brackets.

  ```jet
  // BEFORE (@[…]):              // AFTER (bare):
  @[Serialize, Comparable]       #[Serialize, Comparable]
  struct Point { … }             struct Point { … }
  ```

- **Option B — Rust-literal `#[derive(…)]`.** The exact spelling S55 rejected,
  with the `derive(…)` wrapper.

  ```jet
  // BEFORE (@[…]):              // AFTER (Rust-literal):
  @[Serialize, Comparable]       #[derive(Serialize, Comparable)]
  struct Point { … }             struct Point { … }
  ```

**Recommendation:** none — surfaced with D-ATTR1.

### D-ATTR3 — Move loop labels to `#` too, or leave them `@`? (no rec)

Why it matters: only live if D-ATTR1 = B. Labels (D-LABEL1) reuse the `@` sigil.
If attributes move but labels stay, Jet source carries two marker sigils and fmt
prints both — the load-bearing "trap" the plan flags.

- **Option A — Move labels too (one sigil).** Uniform `#`; ref/host `@` (U6/U16)
  stays, since it lives in CLI/manifest strings, not source.

  ```jet
  #unsafe
  #outer loop {
      break #outer
  }
  ```

- **Option B — Attributes only (mixed sigils).** Labels keep `@`; two marker
  sigils coexist in source.

  ```jet
  #unsafe
  @outer loop {
      break @outer
  }
  ```

**Recommendation:** none — surfaced with D-ATTR1. The plan flags the mixed-sigil
outcome (Option B) as a trap but does not pick.

### D-NARG1 — Named args + defaults on methods/constructors now? (rec A)

Why it matters: S61 (labels + trailing defaults) is built for free functions but
methods get nothing — the label is parsed then silently dropped and method
defaults never fill. A real footgun, since the parser already accepts the syntax.

- **Option A — Yes, methods in scope (recommended).** Method calls behave like
  free functions: labels checked, defaults filled.

  ```jet
  fn draw(self, filled: Bool = false) { … }
  rect.draw(filled: true)   // label checked; default fills when omitted
  rect.draw()               // filled defaults to false
  ```

- **Option B — No, free functions only.** Methods keep today's behavior: label
  silently ignored, default never fills.

  ```jet
  fn draw(self, filled: Bool = false) { … }
  rect.draw(filled: true)   // label dropped; default unsupported on the method
  ```

**Recommendation:** A — leaving methods unsupported is a silent footgun and the
parser already accepts the syntax.

### D-NARG2 — Does fmt preserve or canonicalize call-site labels? (rec A)

Why it matters: labels are optional (S61). fmt must either leave the user's
choice as written or rewrite it (add/remove labels) on every format.

- **Option A — Preserve as written (recommended).** Label presence is the user's
  documentation choice; fmt never adds or strips.

  ```jet
  greet("world", loud: true)   // stays labeled
  greet("world", true)         // stays unlabeled — no auto-add
  ```

- **Option B — Canonicalize.** fmt enforces one form (e.g. always label, or
  always strip), rewriting calls.

  ```jet
  greet("world", true)         // canonicalize-to-labeled → greet("world", loud: true)
  ```

**Recommendation:** A — preserve in v1; revisit canonicalization with the LSP
quick-fix (S14 M6).

### S29-FLUSH — Flush constructor block `Point{x: 1}` (no space)? (rec A)

Why it matters: amends ratified **S29** (which shows `Point {x: 1}` with a
space). Owner asked for the flush form; the parser already accepts both, so this
is a formatter-canonical-style change behind an S29 amendment.

- **Option A — Flush construction (recommended).** Type name hugs its field
  block, the way a call's `(` hugs the callee. Colon spacing (`x: 1`) keeps the
  language-wide `: ` rule.

  ```jet
  // BEFORE (S29 today):        // AFTER (flush):
  p :: Point {x: 3.0, y: 4.0}   p :: Point{x: 3.0, y: 4.0}
  ```

- **Option B — Keep the space (status quo).** Reject the request; `Point {…}`
  stays canonical.

  ```jet
  p :: Point {x: 3.0, y: 4.0}   // unchanged
  ```

**Recommendation:** A — reads like a call, one canonical style, isolated fmt
change. Note: the plan also recommends extending the flush rule to destructuring
patterns (`Point{x, y} :: make()`) for build-vs-match symmetry; folded here as a
sub-point rather than its own card.

### D-CTOR1 — Named constructors vs. true overloading? (rec A)

Why it matters: the constructor-shapes fork. Today a no-`self` static *is* a
named constructor (`Point.unit()` ships already); a duplicate name is a hard
E0105. Overloading means changing the method key and adding name-mangling +
resolution machinery in codegen.

- **Option A — Named constructors only (recommended).** Many shapes = many
  distinct static names. Already works; formalize it and teach it on E0105.

  ```jet
  struct Point {
      fn cartesian(x: Float, y: Float) -> Point { … }
      fn polar(r: Float, theta: Float) -> Point { … }
  }
  // Point.cartesian(3.0, 4.0)   Point.polar(5.0, 0.9)

  // Attempting overload → teaching error:
  fn from(x: Float, y: Float) -> Point { … }
  fn from(r: Float) -> Point { … }   // Error [E0105]: `from` is defined twice
                                     // Fix: name each ctor, e.g. cartesian / unit
  ```

- **Option B — Overload by arity only.** One name; candidates differ in
  parameter count; resolve by counting args.

  ```jet
  fn make(x: Float, y: Float) -> Point { … }   // 2 args
  fn make(r: Float) -> Point { … }             // 1 arg
  // Point.make(3.0, 4.0) → 2-arg; Point.make(5.0) → 1-arg
  // two 1-arg shapes (polar(r) vs radius(r)) still collide → named ctors anyway
  ```

- **Option C — Overload by full signature (type-directed).** One name;
  candidates differ by arity or param types; resolve by matching arg types.

  ```jet
  fn of(n: Int) -> Id { … }
  fn of(s: String) -> Id { … }
  // Id.of(7) → Int overload; Id.of("x7") → String overload
  // Id.of(3.0) → Error [E0112]: no `of` overload accepts Float
  ```

**Recommendation:** A — already works, zero codegen change, one mechanical path,
matches the `Point.unit()` precedent. Reject overloading with a teaching error
pointing at named ctors + S61 defaults.

### D-ALLOC1 — Allocator constructor + allocate spelling? (no rec)

Why it matters: `Arena` placement is ratified (flat `core.mem.Arena`, D-REF2);
this picks the surface tokens for constructing an allocator and allocating from
it. The plan surfaces three spellings without a pick.

- **Option A — Method style.** Construct with `.new()`, allocate with a method.

  ```jet
  use core.mem
  arena :: mem.Arena.new()
  node :: arena.alloc(value)   // returns the stored value, freed at scope end
  ```

- **Option B — Allocator-parameter style.** Allocate via a builtin that takes the
  allocator as a named argument.

  ```jet
  use core.mem
  arena :: mem.Arena.new()
  node :: make(Node, in: arena)
  ```

- **Option C — Capacity-typed constructor.** Bake capacity/shape into the
  constructor type.

  ```jet
  use core.mem
  arena :: mem.Arena(capacity: 4096)
  node :: arena.alloc(value)
  ```

**Recommendation:** none — the plan lists three spellings without a pick. (The
related D-ALLOC-B — does an arena value need `@unsafe`? — recommends **no**, gate
only with `use core.mem`; it is a confirm, not surfaced as a card here.)

---

## Parked — not open ballots

Kept out of the queue deliberately so the owner sees only live decisions.

- **Loop unification (amends S19)** — decided: `loop` is the one form;
  `while`/`for` become teaching errors. No longer a decision — it is an
  implementation task tracked in `docs/plans/sidequests/s19-amend-loop-unification.md`.
- **jetos config surface (former D-OS2…D-OS6) and platform (D-NX1…D-NX6)** —
  **deferred to post-Epoch-3.** jetos is research-track until then; do not
  ratify its surface syntax during Epoch 2/3. Context lives in
  `docs/plans/jetpack-jetos/`.
- **Epoch-2 milestone ballots (D-REF2, D-LIB1/2, D-JSON1, D-IO2, D-PKGS4,
  D-TEST1, D-TOOL2/5, D-CROSS2/3) and all REPL refinements (D-REPL*)** — ratified
  2026-06-16/17; recorded in `syntax-decisions.md` and the relevant milestone
  plans. They left this queue per the house rule.
- **Sidequest language features (D-ILE1, D-BIND1, D-LABEL1, S6-R, D-IF1, D-IF2)**
  — ratified 2026-06-18; recorded in `syntax-decisions.md` and their sidequest
  plans (`docs/plans/sidequests/`). D-IF2 settled D-IF1's multi-arm `if` surface
  (`else` catch-all, braceless arm bodies, structural bare-value/condition mix).
