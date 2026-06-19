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

Two decisions remain open. Both are about **how you build a value in Jet**, so
they share one explainer below — read it first, then the two cards. The other
six Next-Tasks ballots were decided 2026-06-19 and recorded in
[`syntax-decisions.md`](syntax-decisions.md); they ratified but are not yet
implemented (the code lands on your word).

---

### How Jet builds a value today (read before D-CTOR1 / D-ALLOC1)

Right now Jet gives you two ways to make a `Point`, and no `new` keyword:

```jet
struct Point { x: Float, y: Float }

p :: Point{x: 3.0, y: 4.0}   // 1. struct literal — name + its fields
u :: Point.unit()            // 2. named constructor — a plain fn that returns Point
```

The literal *is* the constructor: you name every field, you get the value. A
"constructor" like `unit()` is nothing magic — it's just a function with no
`self` that hands back a `Point`, named after what it makes. When the expected
type is already known (a field, a return, a call argument) you can even drop the
name and write the bare `{x: 3.0, y: 4.0}`.

(Spacing note: the flush `Point{…}` shown here is the form you just ratified in
S29-FLUSH; today's formatter still emits the spaced `Point {…}` until that change
lands. It doesn't affect either decision below.)

Languages split into two camps on this:

| Camp | How you construct | Examples |
|------|-------------------|----------|
| **Literal-first** — the record literal builds the value; "constructors" are ordinary named functions | `Point{x: 1, y: 2}`, `Point.unit()` | **Jet**, Zig (`Point{ .x = 1 }`), Go (`Point{X: 1}` + `NewPoint()`), Rust (`Point { x, y }` + `Point::new()` by convention) |
| **Constructor-method** — the type owns one or more special `init`/`new` members, often overloaded by signature | `Point(3, 4)` (Swift/Python), `new Point(r)` (C++/Java) | Swift (`init`), C++/Java (overloaded ctors), Python (`__init__`) |

Jet is literal-first and stays there. Both open decisions are spelling questions
inside that camp:

- **D-CTOR1** — when you want *several* ways to build a `Point`, do you give each
  one its own name (literal-first all the way), or let one name carry several
  shapes (borrow overloading from the other camp)?
- **D-ALLOC1** — an allocator is just a value you construct and use; which of the
  literal-first spellings should `Arena` wear?

---

### D-CTOR1 — When a type has several constructors, name each one or overload one name? (rec A)

**The scenario.** You're writing a `Point` and you want two ways to build it:
from x/y coordinates, and from a radius/angle. Both take two `Float`s. How do
callers tell them apart?

```jet
// You want both of these to exist:
a :: Point.cartesian(3.0, 4.0)   // x, y
b :: Point.polar(5.0, 0.9)       // radius, angle
```

The fork is whether the *name* does the disambiguating (you write `cartesian`
and `polar`) or the *signature* does it (you write `make` twice and the compiler
picks by the argument types). This is the one place Jet could borrow the
"constructor-method" camp's overloading.

**The tradeoff, plainly:**

| | Named constructors (A) | Overloading (B / C) |
|---|---|---|
| Reading a call | `Point.polar(5, 0.9)` says what it does | `Point.make(5, 0.9)` — you must know the overload set to know what you got |
| Two shapes with the *same* types (polar `(r, θ)` vs a hypothetical `(width, height)`) | Just works — different names | **Ambiguous** — overloading can't separate them; you're forced back to names anyway |
| Error when you get it wrong | "no constructor `poler`" — a typo'd name | "no `make` overload takes `(Float, Float, Float)`" — a resolution failure that's harder to teach |
| Compiler cost | Zero — already how Jet works | New name-mangling + overload-resolution pass in sema/codegen; every call site does candidate matching |
| Fits Jet's camp | Literal-first, names carry meaning | Imports the constructor-method camp's main complication |

The catch that sinks overloading: it only disambiguates when the signatures
*differ*. The moment you have two same-typed shapes (very common — `(Float,
Float)` is both cartesian and polar) you must name them anyway. So overloading
doesn't remove the need for names; it just adds a second, costlier mechanism
beside them.

- **Option A — Named constructors only (recommended).** Each shape is a plainly
  named function that returns the type. This already ships today.

  ```jet
  struct Point {
      fn cartesian(x: Float, y: Float) -> Point { Point{x: x, y: y} }
      fn polar(r: Float, theta: Float) -> Point { … }   // r, θ → x, y
  }

  a :: Point.cartesian(3.0, 4.0)
  b :: Point.polar(5.0, 0.9)

  // Two ctors sharing a name is a clear, early error:
  fn from(x: Float, y: Float) -> Point { … }
  fn from(r: Float) -> Point { … }   // Error [E0105]: `from` is defined twice
                                     //   fix: name each one (cartesian / polar)
  ```

- **Option B — Overload by argument count.** One name, the compiler picks by how
  many arguments you pass.

  ```jet
  fn make(x: Float, y: Float) -> Point { … }   // 2 args
  fn make(r: Float) -> Point { … }             // 1 arg
  // Point.make(3.0, 4.0) → the 2-arg one;  Point.make(5.0) → the 1-arg one
  // but polar(r) and radius(r) are both 1-arg → still collide → name them anyway
  ```

- **Option C — Overload by full signature.** One name, the compiler picks by
  argument *types* (this is the C++/Swift model).

  ```jet
  fn of(n: Int) -> Id { … }
  fn of(s: String) -> Id { … }
  Id.of(7)       // → the Int one
  Id.of("x7")    // → the String one
  Id.of(3.0)     // Error: no `of` overload accepts Float (hypothetical — Jet has no overloading)
  ```

**Recommendation:** A. It already works, costs no compiler machinery, keeps call
sites self-describing, and overloading wouldn't even let you delete the named
constructors (same-typed shapes still need names). Reject overloading with a
teaching error that points at named constructors plus S61 defaults.

### D-ALLOC1 — How should you spell "make an allocator" and "allocate from it"? (rec A)

**The scenario.** You're parsing a big file and want every node freed together
when you're done, so you reach for an arena (`core.mem.Arena`, already placed by
D-REF2). You need to (1) construct the arena and (2) allocate a value in it. The
only open question is what those two lines *look like*.

You asked for the philosophy behind `Type.new()` vs `Type{}` — here it is, with
where each leads:

- **`Type{...}` (record literal).** "A value is its fields." Building the value
  means listing what it's made of. Great when the fields *are* the value
  (`Point{x, y}`); awkward when construction does real work (open a file, grab
  memory from the OS) — there are no honest "fields" to list, so the literal
  would expose internals you shouldn't touch. Jet uses this for plain data.
- **`Type.new()` (named constructor).** "Building is an action, not a field
  list." A plain function runs the setup and hands back a ready value, hiding
  internals. This is what `Point.unit()` already is. It's the right fit when
  construction *does* something — exactly the allocator case. (`new` here is just
  a conventional function name, not a keyword; D-CTOR1 decides whether the name
  can be reused.)
- **`Type(...)` (call-the-type).** "The type name *is* the constructor." Compact,
  but it's a third construction syntax Jet doesn't have today, and it blurs the
  line between "a type" and "a function" — adopting it is a real language
  addition, not just a stdlib choice.

An allocator is the textbook "construction does work" case, so it wants the
named-constructor spelling, not a literal. The options differ only in how you
spell the two lines:

- **Option A — Method style (recommended).** Construct with a named constructor,
  allocate with a method on the arena. Reads like the rest of Jet.

  ```jet
  use core.mem
  arena :: mem.Arena.new()
  node  :: arena.alloc(value)   // value lives in the arena, freed at scope end
  ```

- **Option B — Allocator-parameter style.** Construct the same way, but allocate
  through a free `make` builtin that takes the allocator as a labeled argument
  (the Zig-flavored "allocator is a parameter you pass around" style).

  ```jet
  use core.mem
  arena :: mem.Arena.new()
  node  :: make(Node, in: arena)
  ```

- **Option C — Capacity-typed constructor.** Same method style, but bake the
  capacity into construction so the size is visible at the call site.

  ```jet
  use core.mem
  arena :: mem.Arena(capacity: 4096)   // note: this is the "call-the-type" spelling
  node  :: arena.alloc(value)
  ```

**Recommendation:** A. It's the plain named-constructor + method shape Jet
already uses everywhere, nothing new to learn. Capacity (C's idea) can ride along
as an optional argument with an S61 default — `Arena.new(capacity: 4096)` — so you
don't have to choose between A and C. (Related confirm, **D-ALLOC-B**: an arena
value does *not* need `@unsafe` — reaching for `use core.mem` is the opt-in. Not
a card, just noting the recommendation is "no gate.")

---

## Open — captured, not yet drafted as full cards

Real open decisions found across the plans, surfaced here so none stays hidden in
prose. Each is a one-liner plus a recommendation; ask the dashboard to draft any
of these into a full worked card when you want to decide it. Format:
`ID — title — status — one-liner (rec).`

- **S83** — External definitions for structs/modules — *blocked* — define methods/items out-of-body, identical semantics; needs a fresh separator (`::` spent by D-BIND1, `.` by D-MOD1). Owner picks a separator or withdraws.
- **D-CTOR2** — Constructor marker — *open, ready* — none vs `new`/`init`/`@constructor`. (rec: none — a no-`self` static returning the type already *is* a constructor.)
- **D-CTOR3** — Overload × defaults collision — *conditional on D-CTOR1=B/C* — if overloading lands, forbid defaults on overloaded names so `make(5)` can't match two candidates. (rec: forbid; moot if D-CTOR1=A.)
- **D-ALLOC-C** — Which allocators ship + wider-API namespace — *open* — `Arena` is in; bundle `Bump`/`Pool`/`Fixed` now or stage them, and is the expert API flat in `core.mem` or grouped under `core.mem.alloc`? (rec: Arena now, others staged; flat.)
- **D-ALLOC-D** — Reset/free verb + use-after-reset wording — *open* — capability-vocabulary for cleanup (`reset`/`free`) and the diagnostic when you touch freed memory. (rec: settle with D-ALLOC1.)
- **D-NARG-D2** — Default referencing earlier params — *open* — allow `fn box(w: Int, h: Int = w)`? (rec: no in v1 — defaults are self-contained; teaching error.)
- **D-NARG-D4** — Dedicated label-mismatch diagnostic — *open* — transposed/unknown labels fold into E0104 today; give them their own teaching code? (rec: yes.)
- **D-NARG-D5** — Labels × future overloading — *blocked on D-CTOR1* — labels don't drive overload resolution; resolve constructor shapes first. (rec: revisit after D-CTOR1.)
- **D-JSON3** — Surface lenient JSON coercions — *open* — D-JSON1 coerces `"8080"`→`8080`; how is what-got-coerced shown (per-decode report? build log?). (rec: pick a surfacing, then card it.)
- **D-TOOL-SPLIT** — Split lsp/fmt/lint from the `jet` binary — *open, needs owner thought* — separate binaries/plugins vs one bundled tool. (no rec — owner call.)

## Parked — not open ballots

Kept out of the queue deliberately so the owner sees only live decisions.

- **Deferred-by-design language decisions** — **S53** (concurrency: tasks/channels,
  v2), **S56** (typed reflection / user derives, E3 — S26 Layer 3), **S60**
  (compile-time pure eval + data embedding, post-1.0). Ratified as deferred;
  recorded in `syntax-decisions.md`. No action — listed so they stay visible.

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
