# M9 — Generics & traits

**Blocked on decisions:** S45 (generic function/type syntax), S28 (trait
declaration & impl), S48 (dynamic dispatch policy). Resolves S26
(comptime: rejected in favor of traits+monomorphization — owner confirms
via S26 row). Depends on M8 (function values inform inference work).
**Error codes:** E0901+.

## Goal

User-written generic functions/types and traits (named capabilities) —
the feature that makes Jet viable for real library code and Rust
rewrites. Monomorphized like Rust, so zero runtime cost. Scope is
deliberately tight: single-param-style simplicity, no associated types,
no default methods in v1 of traits.

## Surface (uses ballot recommendations — substitute ratified choices)

```jet
trait Shape {
    fn area(self) -> Float;
    fn name(self) -> String;
}

struct Circle { radius: Float; }
struct Square { side: Float; }

impl Shape for Circle {
    fn area(self) -> Float { return 3.14159 * self.radius * self.radius; }
    fn name(self) -> String { return "circle"; }
}
impl Shape for Square {
    fn area(self) -> Float { return self.side * self.side; }
    fn name(self) -> String { return "square"; }
}

fn largest[T: Comparable](xs: List[T]) -> T? { … }     // generic fn

fn print_area(s: Shape) { print("{s.name()}: {s.area()}"); }  // trait as type

struct Pair[T] { first: T; second: T; }                // generic struct

fn main() {
    val shapes: List[Shape] = [Circle { radius: 1.0 }, Square { side: 2.0 }];
    shapes.each((s) => { print_area(s); });
}
```

- **Generic params** in square brackets after the name (S45, consistent
  with `List[T]`): `fn f[T](…)`, `struct Pair[T] { … }`,
  `enum Tree[T] { … }`. Bounds: `[T: Trait]`, multiple `[T: A + B]`.
  Unbounded `T` supports only move/clone-by-rule and being passed along
  (exactly Rust's implicit rules — E0901 explains "values of type `T`
  can only be moved or handed on; to call `.area()` say `T: Shape`").
- **Traits (S28):** `trait Name { fn sig(self) -> T; … }` — signatures
  only, with the usual access prefixes on `self`. Implement with
  `impl Trait for Type { … }` (orphan rule: at least one of trait/type
  defined in this program — relevant once packages exist, enforce now,
  E0902).
- **Trait as a type (S48):** writing a trait name in type position
  (param, field, `List[Shape]`) means dynamic dispatch; the compiler
  boxes invisibly (same policy as M3 recursion / M8 stored closures).
  Generic `[T: Shape]` means monomorphization. Plain-words doc rule:
  "a `Shape` parameter accepts any shape; `[T: Shape]` additionally
  promises every call uses the *same* shape."
- **Built-in traits** (compiler-known, implementable rules below):
  `Printable` (what interpolation/print need — auto-derived for M3 types,
  user-overridable with `fn to_text(self) -> String`),
  `Comparable` (`<` etc.; auto for Int/Float/String/Char),
  `Equatable` (`==`; auto-derived as in M3). Users may implement
  `Printable` for their types to customize printing; `Comparable`/
  `Equatable` user-impls are deferred (E0903 staged) to keep operator
  semantics predictable.

## Sema rules

1. Type variables enter the existing type representation (extend M5's
   `Type::Generic` groundwork). Inference at call sites: unify argument
   types against parameter types; ambiguous/unconstrained → E0904 with
   a turbofish-free fix ("annotate the binding: `val p: Pair[Int] = …`").
   No explicit call-site type arguments in v1 — if inference fails, an
   annotation somewhere always suffices (keep it that way).
2. Bound checking: calling a method on `T` requires the bound (E0901);
   passing a type that doesn't implement the trait → E0905 ("`Square`
   isn't `Comparable`; it would need `impl Comparable for Square`,
   which isn't available yet" — message aware of E0903 staging).
3. `impl Trait for Type` must implement every signature exactly
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
| `fn f[T: Shape](x: T)`   | `fn user_f<T: user_Shape>(x: &T)`           |
| `trait Shape { … }`      | `trait user_Shape { … }`                    |
| `impl Shape for Circle`  | `impl user_Shape for user_Circle`           |
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
real; E0034 `<T>` angle brackets → `[T]`; E0035 `where` clauses → inline
bounds; E0036 `dyn`/`Box` → just write the trait name.

## Examples & tests

- `examples/21_traits.jet` — shapes (the canonical demo), mixed
  `List[Shape]`, plus a generic `largest` over `Comparable`.
- `examples/22_generic_types.jet` — `Pair[T]`, a generic `Stack[T]`
  struct wrapping `List[T]`.
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
comptime (S26 closes), variance annotations. `Map` custom-key traits.
