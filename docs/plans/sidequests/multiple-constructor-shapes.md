# Sidequest: multiple constructor shapes / function signatures

## Goal

Let one type offer several ways to build itself — `Point.cartesian(x, y)`,
`Point.polar(r, theta)`, plus the struct literal `Point { x, y }` — without
forcing each shape into a separately-named free function or a wider parameter
list with sentinel defaults. Decide whether Jet supports **named constructors**
(distinct static methods, the path the language already half-walks with
`Point.unit()`) or **true overloading** (one name, many signatures, resolved by
argument shape), pin resolution rules, and reconcile with S61 labels/defaults
and U18 inferred constructors. This is design-heavy: the owner picks the model
before any code lands.

## Current state (verified)

- **Static methods already are named constructors.** `Source/AST.rs:563` `Func`
  carries `name`, `params`, `return_type`. A method with no `self` param is a
  static on the type: `Source/Sema.rs:121` `func_to_method_sig` sets
  `is_static = self_param.is_none()`. `examples/features/10_structs.jet` ships
  `fn unit() -> Point { … }` called as `Point.unit()`. So `Point.cartesian(…)` /
  `Point.polar(…)` **already work today** as ordinary distinct-named statics —
  the only thing missing is the owner blessing them as *the* constructor story
  and (optionally) one shared name.
- **Methods are keyed by name; same name twice is a hard error.** Both struct
  bodies (`Source/Sema.rs:600` `StructDef.methods: Vec<Func>`) and `impl` blocks
  (`Source/AST.rs:650`) feed `register_type_methods` / `register_impl_methods`
  (`Source/Sema.rs:1256`, `1282`). The store is `HashMap<String, MethodSig>`
  (`Source/Sema.rs:62`,`68`); a duplicate name pushes `method_defined_twice`
  (`Source/Sema.rs:1487`, code **E0105**) instead of overwriting. So **overloading
  is currently rejected** at registration — adding it means changing the key
  from `name` to `(name, arity/signature)` and threading overload-set resolution
  through `check_static_method` (`Source/Sema.rs:7073`) and the instance path.
- **Call resolution is name-only, single-sig.** `check_static_method`
  (`Source/Sema.rs:7080`) does `registry.method(type_name, method).cloned()` — one
  `MethodSig`, then `check_method_args` (`Source/Sema.rs:7105`) checks arity
  (E0104) and per-arg types (E0112). There is no candidate-set / best-match
  machinery anywhere.
- **Codegen lowers methods to Rust `impl` methods keyed by name**
  (`Source/Codegen/Items.rs`, `mangle(&f.name)`). Rust has no overloading, so any
  same-name overload model **requires name-mangling in codegen** (e.g.
  `Point.from(Int)` → `Point__from__Int`) plus a dispatch rewrite at every call
  site. Named constructors need **zero codegen change** — they already lower as
  distinct statics.
- **S61 (labels + defaults) is the cheaper expressivity lever.** Ratified
  (`syntax-decisions.md:667`): optional `name: value` labels, positional order
  fixed, trailing defaults — `fn f(x: Int, urgent: Bool = false)`. `Param.default`
  exists (`Source/AST.rs:589`). One function with defaulted trailing params already
  covers many "two shapes" cases without overloading or extra names. S61 is
  marked "post-1.0 pending," so confirm its status alongside this.
- **U18 is unrelated to type constructors.** Memory and `syntax-decisions.md:1568`
  / `Source/AST.rs:377` show U18 "inferred constructors" = the *module-config*
  feature (bare `{ … }` typed by expected type for `System`/`Service`/`Env`
  records). It does **not** touch struct/`impl` constructors. Don't conflate.
- **Struct literals stay the field-by-field path** (S29,
  `syntax-decisions.md:278`): `Point { x: …, y: … }`, every field once. That is
  the "raw" constructor; named/overloaded constructors are the *computed* ones
  that derive fields from other inputs.

## Owner signal

`docs/plans/owner-todo.md:15` — "Reconsider structure of constructors. Should
this just be an inherent dot operator method/function constructor call?" The
existing `Point.unit()` static pattern and this note both lean toward **named
constructors as the canonical model**, with overloading as the feature to
*reject with a workaround* (the simplicity ratchet, I8 / philosophy #4: one
mechanical path). The decisions below surface the choice honestly rather than
pre-deciding it.

## Decisions needed before coding (owner)

### D-CTOR1 — Named constructors vs. true overloading

The core fork. Everything else depends on it.

**Option A — Named constructors only (recommended).** Many shapes = many
distinct static names. Already works; formalize it as *the* story and add docs +
diagnostics that teach it when someone tries to overload.

```jet
// BEFORE (today): works, but unblessed and undocumented as "the way"
struct Point {
    x: Float
    y: Float
    fn cartesian(x: Float, y: Float) -> Point { return Point { x: x, y: y } }
    fn polar(r: Float, theta: Float) -> Point {
        return Point { x: r * cos(theta), y: r * sin(theta) }
    }
}
// Point.cartesian(3.0, 4.0)   Point.polar(5.0, 0.9)

// AFTER attempting overload → teaching error, points at named ctors:
struct Point {
    fn from(x: Float, y: Float) -> Point { … }
    fn from(r: Float) -> Point { … }   // Error [E0105]: `from` is defined twice
}                                       // Fix: give each constructor its own
                                        //      name, e.g. `cartesian` / `unit`
```
Behavior: zero codegen change, no resolution machinery, one mechanical path.
Cost: caller can't reuse one verb; expressivity comes from good names + S61
defaults.

**Option B — True overloading by arity only.** One name; candidates must differ
in **parameter count**. Resolve by counting args (no type-directed matching).

```jet
struct Point {
    fn make(x: Float, y: Float) -> Point { … }   // 2 args
    fn make(r: Float) -> Point { … }             // 1 arg
}
// Point.make(3.0, 4.0) -> 2-arg; Point.make(5.0) -> 1-arg
// Point.make(1.0, 2.0, 3.0) -> Error [E0104]: no `make` takes 3 arguments
```
Behavior: registry key becomes `(name, arity)`; codegen mangles
`make__2` / `make__1`. Ambiguity is impossible (arity is unique) but
two 1-arg shapes (`polar(r)` vs `radius(r)`) still collide → forces named
ctors anyway. Cost: a real new feature; the "differ only by type" case is
unreachable.

**Option C — True overloading by full signature (type-directed).** One name,
candidates differ by arity *or* parameter types; resolve by matching arg types.

```jet
struct Id {
    fn of(n: Int) -> Id { … }
    fn of(s: String) -> Id { … }
}
// Id.of(7) -> Int overload; Id.of("x7") -> String overload
// Id.of(3.0) -> Error [E0112]: no `of` overload accepts Float
//   Why: candidates are `of(Int)` and `of(String)`; Fix: convert with .to_int()
```
Behavior: most powerful, most complex. Needs candidate-set resolution, an
ambiguity rule (what if an arg matches two overloads after coercion?),
interaction with S25 value-distribution and S61 defaults, and codegen mangling
by type. Highest blast radius; collides hardest with priority #4 (one
mechanical path) and I8 (simplicity ratchet).

**Recommendation: A.** It already works, costs nothing in codegen, keeps one
mechanical path, and matches the owner's `Point.unit()` precedent. Overloading
(B/C) buys *name reuse*, which philosophy ranks below simplicity; reject it with
a teaching error that points at named constructors + S61 defaults.

### D-CTOR2 — Is there a privileged constructor keyword/sigil, or are they just statics?

Independent of D-CTOR1: do named constructors get a marker, or stay plain
no-`self` statics?

**Option A — No marker; a no-`self` static *is* a constructor (recommended).**
Status quo. `fn cartesian(…) -> Point` inside `Point` is a constructor by
position + return type. Nothing new in `Source/Syntax.rs`.

```jet
struct Point { fn cartesian(x: Float, y: Float) -> Point { … } }   // ctor, no keyword
```

**Option B — A `new`/`init`-style keyword or `@constructor` attribute.**
Marks intent, lets sema enforce "must return Self," enables editor affordances.

```jet
struct Point {
    init cartesian(x: Float, y: Float) { x: x, y: y }   // implicit Self return
}
```
Cost: a new ratified keyword (I7), parser + fmt + diagnostics work, and a second
way to write a static. Owner has historically declined ceremony (S29 rejected
required `new`).

**Recommendation: A.** No keyword. Keep constructors as ordinary statics; the
"is a constructor" property is *return type == enclosing type*, which sema can
already see if a diagnostic ever needs it.

### D-CTOR3 — Overload interaction with S61 defaults (only if D-CTOR1 = B or C)

If overloading is chosen, defaults + overloads can both match one call.

```jet
fn make(x: Int) -> T { … }
fn make(x: Int, y: Int = 0) -> T { … }
// make(5) matches BOTH -> Error [E_AMBIG]: two `make` candidates accept (Int);
//   Fix: remove the default, or merge into one overload
```
**Option A — Forbid defaults on any overloaded name** (a name is overloaded XOR
defaulted). Simplest rule. **Recommendation if overloading lands.**
**Option B — Defaults expand to all arities, then dedupe; ambiguity → error.**
More permissive, more corner cases. Not recommended.

If D-CTOR1 = A, **this decision is moot** — defaults are the only multi-shape
mechanism and never compete with a same-name sibling.

## Implementation approach (workflow loop)

Two tracks; pick by D-CTOR1.

### Track A — Named constructors only (D-CTOR1 = A)

Mostly docs + one teaching diagnostic; near-zero engine change.

1. **Failing test first.** Add `examples/features/NN_constructors.jet` with
   `Point.cartesian` / `Point.polar` / `Point.unit` + expected output (I5). Add a
   `tests/ui/` fixture for the overload-rejection error (I4).
2. **Spec.** Note in `docs/spec/spec.md`: a no-`self` static whose return type is
   the enclosing type is the canonical "named constructor"; struct literal (S29)
   is the field-wise builder; same-name shapes are rejected (E0105) pointing at
   distinct names + S61 defaults.
3. **Parser.** No change (statics already parse).
4. **Sema.** Reuse E0105 (`method_defined_twice`, `Source/Sema.rs:1487`); optionally
   enrich its `fix` to mention "give each constructor a distinct name, or use a
   default parameter (S61)." Snapshot-bless.
5. **Codegen / fmt.** No change.
6. **Diagnostics.** Update the E0105 entry in `docs/spec/diagnostics.md` if the
   fix text changes; re-bless `tests/ui`.

### Track B — Overloading (D-CTOR1 = B or C) — larger

1. **Failing tests/examples** for the resolved + ambiguous + no-match cases.
2. **Spec** the overload model, resolution order, ambiguity rule.
3. **Parser:** no grammar change (same `fn`); the change is downstream.
4. **Sema:** change the method store key from `String` to an overload set
   (`HashMap<String, Vec<MethodSig>>`); update `register_type_methods` /
   `register_impl_methods` (`Source/Sema.rs:1256`/`1282`) to *append* unless a
   true duplicate (same arity for B; same arity+types for C). Rewrite
   `check_static_method` (`Source/Sema.rs:7073`) + the instance path into
   candidate resolution: filter by arity (B) or arity+arg-type assignability
   (C), then 0 → E0102/E0104, 1 → check, >1 → new ambiguity diagnostic
   (needs a new E-code in `diagnostics.md`, I4). Decide D-CTOR3.
5. **Codegen:** name-mangle overloaded methods (`Source/Codegen/Items.rs`) and
   rewrite call sites to the resolved mangled name (sema must annotate which
   candidate won so codegen stays dumb, I3).
6. **fmt:** unchanged (each `fn` formats independently).
7. **Diagnostics + examples:** new ambiguity/no-match snapshots.

## Test / acceptance checklist

- [ ] `examples/features/NN_constructors.jet` builds, runs, golden output
      matches (`Point.cartesian` / `Point.polar` / `Point.unit`). (I5)
- [ ] `tests/ui/` snapshot for the same-name case: Track A → E0105 teaching
      distinct names + S61 defaults; Track B → resolved-overload success +
      `E_AMBIG` ambiguity + no-match snapshots. (I4)
- [ ] No `unsafe` in generated code; no rustc error on generated output (I2).
- [ ] `docs/spec/spec.md` + `docs/spec/diagnostics.md` describe the chosen model;
      any new keyword/sigil recorded in `Source/Syntax.rs` with a decision ID (I7).
- [ ] `tests/decisions.rs` ratification check passes for any new D-CTOR row.
- [ ] If Track B: a struct method and an `impl`-block method with the same name
      participate in **one** overload set (both registration paths agree).
- [ ] S61 defaults still parse and behave; D-CTOR3 rule enforced if overloading.
- [ ] `jet fmt` round-trips the new example unchanged.
```
