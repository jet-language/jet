# M9 — Generics & traits

**Decisions:** S45, S28, S48, S55 (hybrid derive) ratified. S26/S57
(comptime) ratified as fully separate: traits own all polymorphism;
comptime computes values only and lands in M9.5. Depends on M8 (function
values inform inference work).
**Error codes:** E0901+.

## Goal

User-written generic functions/types and traits (named capabilities) —
the feature that makes Jet viable for real library code and Rust
rewrites. Monomorphized like Rust, so zero runtime cost. Scope is
deliberately tight: single-param-style simplicity, no associated types,
no default methods in v1 of traits.

## Surface (ratified S28/S45/S48)

```jet
trait Shape {
    fn area(self) -> Float;
    fn name(self) -> String;
}

struct Circle {
    radius: Float;

    impl Shape {
        fn area(self) -> Float { return 3.14159 * self.radius * self.radius; }
        fn name(self) -> String { return "circle"; }
    }
}

struct Square { side: Float; }

impl Square: Shape {
    fn area(self) -> Float { return self.side * self.side; }
    fn name(self) -> String { return "square"; }
}

fn largest<T: Comparable>(xs: List<T>) -> (T?) { … }

fn print_area(s: Shape) { print("{s.name()}: {s.area()}"); }

struct Pair<T> { first: T; second: T; }

struct Score {
    points: Int;
    derive Comparable;   // S55: explicit — field order affects sort/Map
}

fn main() {
    val shapes: List<Shape> = [Circle { radius: 1.0 }, Square { side: 2.0 }];
    shapes.each((s) => { print_area(s); });
}
```

- **Generic params** in angle brackets after the name (S45, same as S33
  `List<T>`): `fn f<T>(…)`, `struct Pair<T> { … }`, `enum Tree<T> { … }`.
  Bounds: `<T: Trait>`, multiple `<T: A + B>`. Unbounded `T` supports only
  move/clone-by-rule and being passed along (E0901).
- **Traits (S28):** `trait Name { fn sig(self) -> T; … }` — signatures
  only. Implement **inside** the type (`impl Trait { … }`) or **outside**
  as `impl Type: Trait { … }` — qualify foreign types with the module
  path: `impl other.Point: Serialize { … }`. Orphan rule unchanged (E0902).
  `.` walks namespaces and calls methods (S16, S27, S30); `:` attaches a
  trait to a type (same as bounds and annotations). `::` reserved for
  `extern rust` paths (S50).
- **Trait as a type (S48):** a trait name in type position (`List<Shape>`,
  `fn f(s: Shape)`) means dynamic dispatch with invisible boxing.
  `<T: Shape>` means monomorphization. Post-1.0: expert opt-in to explicit
  `dyn`/allocation control; beginners keep the default.
- **Built-in traits** (S55 hybrid derive policy):
  - **Auto-derive (silent):** `Printable` (`print("{p}")`, interpolation),
    `Equatable` (`==`) — whenever every field qualifies; hand-written
    `impl` overrides.
  - **Explicit opt-in:** `derive Comparable;` / `derive Serialize;` in the
    type body — required before `<`, `largest`, `Map` ordering, or
    `to_json` work on user types.
  - Primitives (`Int`, `Float`, `String`, `Char`, `Bool`) always implement
    all four. Custom `impl` overrides any derive. User-written
    `Comparable`/`Equatable` without `derive` → E0903 staged until policy
    widens.

## Sema rules

1. Type variables enter the existing type representation (extend M5's
   `Type::Generic` groundwork). Inference at call sites: unify argument
   types against parameter types; ambiguous/unconstrained → E0904 with
   a turbofish-free fix ("annotate the binding: `val p: Pair<Int> = …`").
   No explicit call-site type arguments in v1 — if inference fails, an
   annotation somewhere always suffices (keep it that way).
2. Bound checking: calling a method on `T` requires the bound (E0901);
   passing a type that doesn't implement the trait → E0905 ("`Square`
   isn't `Comparable`; it would need `impl Square: Comparable`,
   which isn't available yet" — message aware of E0903 staging).
3. Every `impl` block must implement every trait signature exactly
   (E0906 lists missing methods; E0907 signature mismatch shows both).
   Duplicate impls → E0908.
4. Trait-as-type values: method calls dispatch dynamically; such values
   are non-clonable in v1 (no `Clone` for dyn — E0201's path explains),
   can't be compared or printed unless the trait includes those
   capabilities (E0312 path).
5. Ownership composes: generic params follow M2 rules with `T`'s
   clonability unknown → treated as non-clonable unless bounded by the
   internal Clonable rule (auto-bound inferred when the body needs a
   clone: sema adds the requirement and reports it in E0905 text).
6. Monomorphization happens conceptually in sema (instantiation table so
   errors point at Jet source with the concrete types named), but
   codegen emits real Rust generics and lets rustc monomorphize — sema
   must therefore prove every instantiation valid itself (R2; never
   lean on rustc).
7. Recursive generic instantiation depth-limit 64 → E0909 (prevents
   infinite monomorphization; show the chain).

## Codegen lowering

| Jet                      | Rust                                        |
|--------------------------|---------------------------------------------|
| `fn f<T: Shape>(x: T)`   | `fn user_f<T: user_Shape>(x: &T)`           |
| `trait Shape { … }`      | `trait user_Shape { … }`                    |
| `impl Circle: Shape`     | `impl user_Shape for user_Circle`           |
| trait in type position   | `Box<dyn user_Shape>` (+ auto-box at construction sites sema marked) |
| `Printable` override     | `impl Display for user_T` delegating to `user_to_text` |
| built-in bounds          | `PartialOrd`/`PartialEq`/`Clone` bounds as recorded by sema |

## Diagnostics to register

E0901 method needs a bound · E0902 orphan impl · E0903 staged: custom
`Comparable`/`Equatable` impls · E0904 can't infer a type argument ·
E0905 type doesn't implement the trait · E0906 impl missing methods ·
E0907 impl signature mismatch · E0908 duplicate impl · E0909
instantiation too deep.
Teaching: E0021 (`interface` staged) upgrades to point at `trait` for
real; E0034 `[T]` square brackets → `<T>` (S33); E0035 `where` clauses →
inline bounds; E0036 `dyn`/`Box` → just write the trait name.

## Examples & tests

- `examples/features/25_traits.jet` — shapes (the canonical demo), mixed
  `List<Shape>`, plus a generic `largest` over `Comparable`.
- `examples/features/26_generic_types.jet` — `Pair<T>`, a generic `Stack<T>`
  struct wrapping `List<T>`.
- ui fixtures for every E09xx; inference-failure fixtures with the
  annotation fix shown; golden tests including dyn-dispatch output and
  a Printable override.
- Soundness battery: every fixture that passes sema must build under
  rustc — generic instantiation is the highest-risk area of the whole
  compiler; add a fuzz-ish matrix test (each builtin type × each generic
  example).

## Out of scope

Associated types/consts, default method bodies, generic methods inside
traits, higher-kinded anything, trait inheritance, blanket impls,
specialization, const generics, explicit `dyn`, user-visible `Box`,
comptime (S26 ratified: value-level only, lands in M9.5 — never in
generics), variance annotations. `Map` custom-key traits.
