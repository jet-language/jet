# Generic bounds by a closed type set or a struct shape

Date: 2026-10-31. Card #5983 C1 (was #5038), from finding F13 of
`Docs/research/mine-for-jet-2026-10-16-vlang.md`.

V lets a generic parameter be bounded three ways: by an interface, by a sum
type (`fn total[T Number](xs []T) T` with `type Number = int | f64`), or by a
struct (`fn name_of[T Named](x T)` takes any struct that has `Named`'s
fields). Jet bounds generics only by traits (S45, `fn f<T: Bound>(…)`). This
note checks what the other two V forms compile to in Jet today, and what Jet
offers instead.

## Probes

The probes are in `$JET_SCRATCH_ROOT/lanes/CardClose/p/b5983_*.jet`. Each one ran
with `jet check` and `jet run`, using the wave-12 compiler
(`q/sg/jet-w12-wave/jet`) with `JET_TOOLCHAIN_ROOT` set to the wave-12 tree.

| Probe | Form | Result |
| --- | --- | --- |
| `union_bound` | `fn total<T: Int \| Float>(xs: [T]) -> T` with `sum + x` | E0360 "No `+` operator is defined for `T`"; each call gives E0904 "Cannot resolve bound `Int` for `T`" |
| `add_bound` | `fn total<T: Add>(…)` with `sum + x` | E0360 "No `+` operator is defined for `T`" |
| `numeric_bound` | `fn total<T: Numeric>(…)` with `sum + x` | E0360, same text |
| `closed_trait` | `trait Number { fn plus(self, other: Self) -> Self }` with `impl Int.Number` and `impl Float.Number` | E0301 "`impl Int.Number` needs a user-defined type" (and the same for `Float`) |
| `union_value` | `fn describe(v: Int \| Float) -> String` matching `.Int(n)` and `.Float(f)` | runs: `int 3`, `float 2.5` |
| `trait_bound` | `fn largest<T: Comparable>(xs: [T]) -> T` | runs: `9`, `1.5`, `pear` |
| `shape_bound` | `struct Named { name: String }` used as `fn name_of<T: Named>(x: T)` | E0301 "The generic bound `Named` is not a trait"; E0302 on `x.name`; E0904 at the call |
| `shape_trait` | `trait Named { fn name(self) -> String }` with an impl for `User` and one for `Pet` | runs: `ada`, `rex` |

## Findings

1. **Struct-shape bounds are excluded by design.** S28 says traits are
   "explicit named capabilities, never structural". A struct in bound position
   is rejected with E0301, and the diagnostic is accurate. The Jet form is
   one named trait plus one `impl` for each struct (`shape_trait`), which
   compiles and runs. V's struct constraint is the structural typing that S28
   rules out. No change is proposed.

2. **A closed set of values already exists; a closed set as a bound does
   not.** D-UNIONTYPE1=A makes `A | B` a closed structural enum. As a
   parameter type it works (`union_value`): one function body, one runtime
   match, exhaustive over the members. That covers V's sum-type constraint
   whenever the body may branch per member.

3. **The real gap is one generic body over the built-in numbers.** V's
   `T Number` gives one monomorphised body that uses `+` for both `int` and
   `f64`. In Jet every route to that is closed today:
   - A union in bound position parses but is not a bound. It is reported as
     E0904 about the first member (`Int`), which is a misleading diagnostic,
     not a feature.
   - `Add` and `Numeric` as bounds do not make `+` available on `T` (E0360:
     "User arithmetic dispatches only through one `impl T.Add` hook").
   - A user trait cannot be implemented on `Int` or `Float` (E0301), so the
     V pattern cannot be rebuilt by hand.
   `Comparable` is the only built-in capability that works as a bound on the
   built-in scalars (`trait_bound`).

## Disposition

- Struct-shape bounds: no action. S28 already decides this, and the trait
  form compiles.
- Closed type set as a value: no action. D-UNIONTYPE1 covers it.
- Generic arithmetic over built-in numbers: this needs an owner ballot. The
  choices are a union bound with monomorphisation, built-in operator
  capabilities usable as bounds (`T: Add`), or keeping the current "write it
  per type or match on a union" answer. Any of these choices requires a
  language decision, so this note does not propose code.
- Side finding: `<T: Int | Float>` should get its own diagnostic ("a bound
  must be a trait") instead of E0904 on the first union member.
