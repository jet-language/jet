# Fan-out operator `f.[…]` + fixed-size lists `[T#N]`

**Status:** design proposal, owner-directed (2026-06-16). Substance decided in
discussion; this is the *measure-twice* artifact to ratify before code. Nothing
here is implemented yet. Motivated by the Blueprint north-star (type-directed
authoring — "the pin decides what fits, and the tool catches mismatches before
you run"). Supersedes the old "Stage 1b = Pkg sugar" step: package list sugar
becomes one instance of the general fan-out operator.

---

## 1. Why

Two interlocking features fall out of one idea — *apply a thing to several typed
inputs written inline*:

- **Fan-out operator** `f.[a, b, c]` ≡ `[f(a), f(b), f(c)]`. Reads like a
  Blueprint node fed several input pins. The ratified package sugar
  `default.[ripgrep, fd]` (U6) is just this with a source as `f`.
- **Fixed-size lists** `[T#N]`. Because a fan-out (and a list literal) has a
  statically-known length, its result is a *fixed-size, homogeneous* list — a
  new primitive distinct from dynamic `[T]` and heterogeneous tuples. This makes
  positional destructuring **compile-time length-checked** — a runtime surprise
  removed.

## 2. Fan-out operator

### Grammar
```
fanout = primary ".[" [ expr { "," expr } [","] ] "]" ;
```
`.[` is a postfix on any primary. `.` and `[` are existing tokens; `.[` is a new
adjacency the parser recognizes. `#` and `.[` do not collide with any current
syntax (comments are `//` `/* */`; `?.` and `a[i]` indexing are unaffected).

### Semantics
- Desugars to a list: `f.[a, b, c]` → `[f(a), f(b), f(c)]`.
- **`f` must be callable with exactly one argument** ("applyable"): user
  functions, **sources** (`default`, `unstable`), and **type/enum
  constructors**. One-arg only — `f.[a, b]` for a 2-arg `f` is ambiguous and
  rejected (unless a future tuple-items extension).
- **Items are typed by `f`'s parameter type** (expected-type-directed
  elaboration), reusing Jet's existing rule that a bare name resolves against the
  expected type (enum unit variants, syntax-decisions.md:254):
  - `default.[ripgrep, fd]` — param is a package name → bare names
  - `add.[1, 2, 3]` — param `Int` → integer literals
  - `paint.[Red, Blue]` — param `Color` → `Color.Red`, `Color.Blue`
- **Homogeneous:** every result shares one type `T` (it is a list). Mixing
  (`[add.[1], greet.["x"]]`) is the existing heterogeneous-list error.
- **No spread** (`f.[*xs]`) in v1 — literal items only.

### Splicing (source preserved)
A fan-out inside an enclosing list literal **flattens**, each result keeping what
it was applied to:
```
[default.[a, b], unstable.c]
  = [ default(a), default(b), unstable(c) ]
  = [ Pkg{default,a}, Pkg{default,b}, Pkg{unstable,c} ]   // one flat [Pkg#3]
```
Splicing fixed-size lists yields a fixed-size list (lengths add).

## 3. Fixed-size lists `[T#N]`

### Type
New `Type::FixedList { elem, len }`, spelled **`[T#N]`** (e.g. `[Point#2]`,
`[Int#3]`). `N` is a compile-time constant.

### Where they come from
- a **fan-out**: `f.[a, b]` : `[T#2]`
- a **list literal** bound to `val`: `val nums = [1, 2, 3]` : `[Int#3]`

### The track-length / auto-widen model (one breath)
- **`val` + literal/fan-out ⇒ `[T#N]`** (length known, checked).
- **`var` initialized from a literal/fan-out ⇒ widens to dynamic `[T]`** (`var`
  signals intent to change; growable).
- **Passing a `[T#N]` to a `[T]` slot widens** (param, annotated binding,
  return) — implicit, safe, **one-way**. `[T]` never narrows back to `[T#N]`
  (a dynamic length isn't statically known).
- **`.map` preserves N**: `[T#N].map → [U#N]`.
- **`.len` on `[T#N]` is a compile-time constant.**
- **Length-changing ops** (`push`, `pop`, `insert`) are **not** available on
  `[T#N]` → teaching error pointing at `[T]`.

### Worked examples
```jet
val pair = Point.[origin, corner]    // [Point#2]
val nums = [1, 2, 3]                  // [Int#3]

val [a, b]    = pair                  // ✅ 2==2, checked at COMPILE time (S74 list bind)
val [a, b, c] = pair                  // ✗ compile error: expected 3 items, found 2

fn render(items: [Widget]) { … }
render(button.["Save", "Cancel"])     // ✅ [Widget#2] widens to [Widget]

var queue = [1, 2, 3]                 // [Int]  (var ⇒ dynamic)
queue.push(4)                         // ✅

val mapped = pair.map((p) => p.x)     // [Float#2] — N preserved
val first  = pair[0]                  // ✅ ; pair[5] ✗ compile-time out-of-range
```

### Coercion table
| from | to | allowed |
|---|---|---|
| `[T#N]` | `[T]` | ✅ widen (implicit, safe) |
| `[T#N]` | `[T#M]` | only if `N == M` |
| `[T]` | `[T#N]` | ✗ (use positional destructure → runtime-checked) |

## 4. Runtime representation (keeps codegen dumb, I3)

`[T#N]` is a **compile-time refinement of `[T]`** — all the fixed-size
guarantees (destructure length, index bounds, no length-changing ops) are
enforced in **sema**. At codegen the type is erased to the existing list (Rust
`Vec<T>`); a fan-out lowers to building that list by calling `f` on each item.
No Rust arrays, no `unsafe` (I1), no codegen checking (I3).

## 5. New diagnostics (each needs what/why/fix + ui snapshot, I4)

| Code (proposed) | Phase | What |
|---|---|---|
| E0961 | sema | fan-out callee is not callable with exactly one argument |
| E0962 | sema | fan-out item doesn't fit the parameter type ("`add` expects an Int here, but `"hi"` is text") |
| E0963 | sema | positional destructure count ≠ fixed-size length |
| E0964 | sema | length-changing op (`push`/…) on a fixed-size `[T#N]` |
| E0965 | sema | compile-time index out of range on `[T#N]` |

(E0960 already claimed for the module-namespace error.)

## 6. Implementation plan (test-first per stage)

1. **Type + parser, no semantics.** `Type::FixedList`; parse `[T#N]` in type
   position; parse `.[ … ]` into `Expr::FanOut`. Exhaustiveness arms across
   codegen/sema/fmt/lsp (no-op/erase). Tests: parse round-trips.
2. **Fan-out sema.** Resolve callee, enforce one-arg, elaborate items against the
   parameter type (mirrors enum-literal resolution), homogeneity, produce
   `[T#N]`. Splicing in list literals. Diagnostics E0961/E0962.
3. **Fixed-size sema.** `val`/`var` rule, widening coercion, `.map` N-preservation,
   `.len` const, destructure length check (E0963), no-grow (E0964), const-index
   bounds (E0965).
4. **Codegen.** Fan-out → build a `Vec` by mapping `f`; `[T#N]` erases to `Vec`.
5. **Examples + golden** (I5): a feature example exercising fan-out, fixed-size
   destructure, `.map` preservation, and the package-list use.

## 7. Ratification checklist (owner)

- [ ] Fan-out operator surface `f.[ … ]` + semantics (§2)
- [ ] `[T#N]` spelling and the track/widen model (§3)
- [ ] Diagnostic codes E0961–E0965 (§5)
- [ ] On ratify: add `syntax.rs` tokens (`.[`, `#`, `[T#N]`), syntax-decisions
      rows + decision-log entries, then build per §6.

## 8. Relationship to other work

Supersedes "Stage 1b = Pkg sugar" in the module/computed-modules plan
(jetpack-jetos): `packages: [default.[ripgrep, fd]]` is now just fan-out +
splice producing `[Pkg#N]`. The module parser (Stage 1a) already landed; module
evaluation (Stage 2, pure-eval) consumes fan-out results like any other value.
