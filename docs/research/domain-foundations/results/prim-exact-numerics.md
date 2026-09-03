# Exact numerics foundations probe

## What I built

I built a real Jet package with arbitrary-precision Rational, scaled Decimal, and Money values. The package uses user-defined arithmetic and comparison operators, JSON codecs, comptime values, generic bounds, rounding modes, and a 1,000,000-value sum. I also ran focused fixtures for literal syntax, generic Numeric bounds, comptime parity, helper calls in operator implementations, and bigint timing.

Files:

- `pkg/package.jet` — package identity and `IO`/`Mem.Alloc` authority.
- `pkg/run.jet` — Rational, ScaledDecimal, Money, generic functions, JSON, comptime, and million-value run.
- `fixtures/comptime_exact.jet` — AOT/interpreter exact-division comparison.
- `fixtures/comptime_custom.jet` — custom struct comptime value.
- `fixtures/custom_literal.jet` and `fixtures/custom_suffix.jet` — literal failure probes.
- `fixtures/generic_numeric.jet` and `fixtures/generic_add_int.jet` — generic-bound probes.
- `fixtures/struct_impl.jet` — helper-call diagnostic repro.
- `bench/bigint.jet` — 20,000-iteration arbitrary-Int timing input.

Archived research agrees with the run: `M-EXACT-NUMERIC` calls exact `Int`, `Decimal`, and `Fraction` the shared substrate, while symbolic algebraic values and certified approximation remain absent. Finance and accounting reports confirm exact Decimal money and typed JSON scale; science-numerics calls the exact substrate shipped but unbenchmarked.

## What worked

- Package scope: `jet check pkg` passed with run-entry, module-graph, Core-closure, and AOT/JIT/interpreter lowering proofs; only L0520/L2510 warnings remained.
- User-defined operators: `jet run pkg` printed `rational 5/6 1/6 1/6 3/2`; Add, Sub, Mul, Div, Equatable, Comparable, and Display all worked.
- Arbitrary Int inside a library type: the same run printed `big rational 123456789012345678901234567891/7`.
- Scaled Decimal: the same run printed `scaled decimal 12940e-3 74040e-4 true`; scale alignment, multiplication, comparison, and JSON round trip worked.
- Money rounding: the same run printed `money 10007 cents 1751 cents 1752 cents 1751 cents` for Down, Up, and HalfUp.
- Codable values: the same run printed `scaled roundtrip true` and `money json {"cents":10007} true`.
- Built-in exact Decimal literals: the same run printed `decimal literal 0.3 true` for `0.1 + 0.2 == 0.3`.
- Generic operation bounds: `jet run fixtures/generic_add_int.jet` printed `3`; `T:Add` works for core `Int`, and `T:Comparable` works for custom Rational in the package.
- Custom comptime data: `jet run fixtures/comptime_custom.jet` printed `custom 1/2`.
- Million-value sum: the package printed `million money 49500000 cents` after summing one million Money values.
- Built-in typed JSON: the package printed `built-in json 12.340 123456789012345678901234567890 {"amount":"12.340","whole":123456789012345678901234567890}`; Decimal scale and large Int survived decoding and re-encoding.

## Gaps

### G1 — exact comptime division changes by tier (`defect`, blocks)

Capability needed: comptime arithmetic must preserve exact Fraction and Decimal semantics across AOT, interpreter, and runtime evaluation.

Evidence:

- `jet run fixtures/comptime_exact.jet` printed `comptime 0/1 0` and `runtime 10/3 10`.
- `jet run --interpret fixtures/comptime_exact.jet` printed `comptime 10/3 10` and `runtime 10/3 10`.

The same source expression, `@third :: @ten / 3`, gives a wrong AOT value and a correct interpreter value. Data analysis, backend services, and science share this risk.

### G2 — helper calls in operator implementations (`defect`, `boilerplate`, hurts)

Capability needed: non-fallible library helpers must be callable from user-defined Add, Sub, Mul, Div, Equatable, and Comparable implementations.

Evidence: `jet check fixtures/struct_impl.jet` reports `Error [E0403]` at `pair :: align(self, rhs)`, claiming the non-fallible helper call is fallible and requiring an error return. The working package must inline scale alignment; its factor loop repeats eight times.

Data analysis, backend services, and science all write exact scalar operators, so this is shared author cost. The workaround is to inline the helper body.

### G3 — no coherent generic Numeric contract (`call-site`, `boilerplate`, hurts)

Capability needed: one Numeric capability must cover core Int, Decimal, Fraction, and generic arithmetic operators.

Evidence: `jet check fixtures/generic_numeric.jet` reports E0109, ``+`` cannot combine `T` and `T`, and E0905, `Int` is not `Numeric`. A separate `T:Add` fixture runs and prints `3`, so operation-specific bounds work but do not form one numeric contract.

A generic numeric library must list separate operation bounds and still lacks a generic zero/one or conversion law. Data analysis, backend services, and science share this call-site burden.

### G4 — no library-defined exact scalar literals (`call-site`, hurts)

Capability needed: library-defined integer/decimal literal and suffix hooks for Rational and scaled Decimal values.

Evidence:

- `jet check fixtures/custom_literal.jet` reports E0112: `accept` wants `Rational`, but literal `1` is `Int`.
- `jet check fixtures/custom_suffix.jet` reports E0134: ``rat`` is not a unit in scope.

Users must write full constructors or factories. Suffixes are limited to declared UnitFamily members. Data analysis, backend services, and science share this ceremony.

### G5 — built-in Decimal lacks canonical Display (`call-site`, annoys)

Capability needed: built-in Decimal should provide Display so exact values interpolate without a migration warning.

Evidence: the package prints the correct `decimal literal 0.3 true`, then emits `Warning [L0520]`: ``Decimal`` has no ``Display`` impl, at `run.jet:281`, with a suggestion to add `impl Decimal.Display`.

The workaround is `Decimal.to_string()` at each interpolation site.

### G6 — arbitrary-Int loop speed is well below Python in this run (`slow`, hurts)

Capability needed: compiler/runtime help for arbitrary-precision Int loops to approach the incumbent bigint speed band from library code.

Evidence uses the same 20,000 iterations and 30-digit seed. Cached `jet run bench/bigint.jet` printed `2469135780246913578024691557790000` in real `0m10.329s` (first run `0m13.435s`). Python 3.13 through `nix shell nixpkgs#python3` printed the same value in real `0m0.697s`. Jet also emits L2510 for the loop.

These are end-to-end command timings, so compiler startup is included; they are directional, not a kernel-only benchmark. A one-million-iteration Jet bigint run exceeded the 120-second command limit, while the one-million-value Money sum completed. A fixed-width I64 is the only tested workaround when bounds permit. Data analysis, backend services, and science share this cost.

## Friction

- The package contains 17 explicit trait implementations: Rational has 7, ScaledDecimal has 6, and Money has 4.
- ScaledDecimal scale alignment repeats an eight-loop factor calculation because extracting it into a helper triggers E0403.
- Users call `rational(...) ?? ...` and write full field constructors instead of numeric literals.
- The package check reports L2510 at eight scale loops and two generic-comparison sites, plus L0520 for Decimal interpolation.

## Defects

- G1 is a wrong-answer cross-tier defect: AOT comptime `10 / 3` becomes `0/1`, while interpreter and runtime produce `10/3`.
- G2 is a false fallibility diagnostic: a helper returning a plain struct is rejected from `impl D.Add` as E0403. Inlining the same logic makes the package run.

## Verdict

Buildable today for a useful exact-numeric package: custom operators, arbitrary Int values, scaled arithmetic, rounding, JSON, generic operation bounds, comptime structs, and a million-value sum all run.

It is not safe to claim complete exact-number support until G1 is fixed; AOT comptime can emit a wrong Fraction.

G2 through G5 add shared library-author or call-site cost. G6 shows a large bigint timing gap, with a startup caveat and no Rust `num-bigint` comparison.

The overall result is **buildable with listed gaps fixed**, not a need for an exact-numeric niche library in Core.
