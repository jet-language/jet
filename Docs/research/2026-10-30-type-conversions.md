# Type conversions: exact braces, named lossy options

Date: 2026-10-30. Card #6313, with the #6318 narrowing and `Float.from_int`
findings. Probes live in `/mnt/jetscratch/lanes/SurfaceDesign/probes/`
(`m6313`, `m6313b`, `m6313c`, `m6313p`, `m6318b`). Every probe printed the same
output on o0, o1 and o2 with the dev compiler at abab9666d8.

## What the language does today

Spelled conversions are destination-owned methods (S42, D-SHAPE-CONVERT1). The
method names come from one table, `BUILTIN_NUMERIC_SOURCES`
(`Compiler/JetFoundation/Source/Registry/BuiltinStatics.jet:175`). Its return
rule is at line 212: a conversion that can lose returns `T String!`, and a widening
returns `T`.

A scalar head `Type{x}` converts only on the ratified lossless widening set
(D-NUMWIDEN-CROSS1). `Int{u8}` and `Float{n}` widen. `Int{f}` is E0112.
`String{n}`, `Meters{n}` and `Float{decimal}` are E0108, and `ANSI{s}` is E0107.
`Float{2^53 + 1}` stops with E3010.

A fixed-width head narrows by wrapping: `U8{U16{300}}` prints 44,
`U8{I32{-1}}` prints 255, `I32{I64{5000000000}}` prints 705032704, and
`I64{U64.max}` prints -1. `I8.from_u64(U64.max)` returns `Ok(-1)`.

Float to integer (probe `m6313p/p09`):

- `Int.from_float(3.7)` is `Ok(3)` and `Int.from_float(-3.7)` is `Ok(-3)`. The
  fraction is dropped silently.
- `Int.from_float(1e20)` fails, although `Int` has no upper bound.
- `Int.from_float(NaN)` fails. `I32.from_float(3e9)` fails.
- `round(2.5)` is 3, `round(-2.5)` is -3, and `round(3.5)` is 4. Ties go away
  from zero.
- `round(1e300)` is 9223372036854775807 and `round(NaN)` is 0. Both are silent
  (`Core/Math/Math.jet:259`).
- `math.floor`, `math.ceil` and `math.trunc` return `Float`. A `Float` value has
  no `floor`, `ceil`, `round` or `trunc` method (E0311).
- `math.round(x)` is E1004 ("exposes `round`, but not in this position"). Only
  `use core.math.[round]` reaches it.
- `2.7.floor()` types the literal as `Decimal` and prints 2. `(-2.5).round()` is
  an internal compiler error: "handle method PreciseMethod without a closed
  route" (`m6313p/p14`).

Int to Float (probe `m6313p/p10`): `Float.from_int(9007199254740993)` returns
`9007199254740992.0` silently, while `Float{9007199254740993}` stops. The two
spellings disagree on the same value.

Fallible conversions of a literal that fits need no handler:
`U8.from_int(200)` prints 200 with no `??` (`m6313p/p12`). The checker already
drops a failure that a known argument rules out.

Units already have the exact-then-explicit split (D-QUANTITY-CONVERT1,
`Docs/spec/syntax-decisions.md:1406`). An exact unit conversion returns
`Result`. `Destination.from_source_rounded(value, mode, digits: n)` rounds on
purpose with `.TowardZero`, `.Floor`, `.Ceiling` or `.NearestEven`. Non-finite
input is a carried error, never NaN or a trap. Sema keeps the mode as
`TUnitRoundingMode` (`Compiler/JetFoundation/Source/Facts/Checked.jet:389`).

## Parameter features the lossy options need

Each probe is in `m6313p` and passed on o0, o1 and o2.

| Feature | Probe | Result |
| --- | --- | --- |
| Labels in any order, skipped defaults, `/` and `*` zones, public label differing from the local name | p01 | works |
| Enum-typed default `Rounding{.Exact}`, passed by label or position as `.Variant` | p02, p15 | works |
| Optional parameter `Int?{None}`, passed as `Val(9)` | p03 | works |
| Optional parameter passed a bare value (`limit: 9`, `mode: .Floor`) | p16 | works |
| Defaulted slot then a variadic tail, plus spread | p04 | works |
| Labelled enum options on a user static `from_int` | p05 | works |
| A default that reads an earlier parameter, on a generic function | p06 | works |
| Labels and defaults through a function value `.call` | p07 | works |
| Label-only zone on an instance method | p08 | works |
| A trait with a type parameter (`trait ConvertFrom<S>`) | p11 | parse error E0003 |

The builtin `from_*` rows take exactly one positional parameter, so they accept
no options today. Jet traits take no angle-bracket type parameters (p11). The
ratified build-time generics (D-GENERIC-TYPEFN1) spell generic types and traits
as `prep fn` recipes, but the compiler does not parse them yet. Both
`prep fn Pair(T: Type) -> Type` (p17) and the trait recipe
`prep fn ConvertsFrom(S: Type) -> Trait` (p18) are E0003. A user type can
declare `fn from_float(value: Float)`, and `Celsius.from_float(x)` prints 2.5
(p19). `Celsius{reading}` is E0302 ("no field `reading`", p20), so braces on a
user type do not convert today.

A bare decimal literal binding (`reading :: 2.5`) is `Decimal`, not `Float`;
passing it to a `Float` parameter is E0108.

## How other languages convert a float to an integer

| Language | Spelling | Fraction | Out of range | NaN |
| --- | --- | --- | --- | --- |
| C, C++ | `(int)x` | toward zero | undefined behavior | undefined behavior |
| Java | `(int)x` | toward zero | saturates | 0 |
| C# | `(int)x` | toward zero | unchecked: saturates since .NET 9 (unspecified before); checked: `OverflowException` | unchecked: 0; checked: throws |
| C# | `Convert.ToInt32(x)` | nearest, ties to even | `OverflowException` | throws |
| Rust | `x as i32` | toward zero | saturates (undefined before 1.45) | 0 |
| Go | `int32(x)` | toward zero | implementation-defined; a constant is a compile error | implementation-defined |
| Python | `int(x)` | toward zero | none (unbounded); `inf` raises `OverflowError` | raises `ValueError` |
| Swift | `Int(x)` | toward zero | traps | traps |
| Swift | `Int(exactly: x)` | `nil` unless whole | `nil` | `nil` |
| Kotlin | `x.toInt()` | toward zero | saturates | 0 |
| Kotlin | `x.roundToInt()` | nearest, ties up | saturates | throws |
| Zig | `@intFromFloat(x)` | toward zero | panics in safe builds, undefined in ReleaseFast | same as out of range |
| JavaScript | `x \| 0` | toward zero | wraps modulo 2^32 | 0 |
| JavaScript | `BigInt(x)` | `RangeError` unless whole | none | `RangeError` |

Rounding helpers also disagree on ties. Rust `round` and Swift `rounded()` go away
from zero. Java `Math.round`, Kotlin `roundToInt` and JavaScript `Math.round` go
toward positive infinity. Python `round` and C# `Convert` go to even.

Only Swift `Int(exactly:)`, JavaScript `BigInt`, and Jet's unit conversions
refuse a fraction instead of dropping it. Swift `rounded(_:)` and C#
`Math.Round(x, MidpointRounding)` take the rounding rule as an argument; the
others pick it by function name. No language in the table lets one conversion
call choose both a rounding rule and an out-of-range rule.

The Ariane 5 Flight 501 inquiry report (1996) traced the loss of the vehicle to
an unprotected 64-bit float to 16-bit integer conversion of horizontal velocity.
Rust carried float-to-int undefined behavior from 2013 to 1.45 (issue #10184).

## Census

In t1 at 82d3369757:

- `Float.from_int(` appears 51 times: 19 Compiler, 15 Examples, 6 Tools, 8
  tests, 3 Jetpack.
- `<integer>.from_float(` appears 26 times: 10 Core, 3 Compiler, 4 Examples, 1
  Tools, 5 tests, 3 Jetpack.
- `from_*_rounded(` appears 64 times, 61 of them in tests.
- `round(` appears 63 times.

There are 18 `Int.from_float(` sites. Each one depends on silent truncation
today.

## Stale spec text found

- `Docs/spec/syntax-decisions.md` repeated the same #3739 amendment sentences at
  lines 1226-1229 and 1231-1234.
- The amendment's example `U8{value %% 256}` used `%%`. In Jet `%%` is the
  truncating remainder (`BinOp.Rem`), so a negative `value` gives a negative
  result and `U8{...}` stops. `%` is the floor modulo that wraps into 0..255
  (`m6313p/p13`).
- The E3010 text for an Int that has no exact Float says "cannot cross into the
  decimal", although the destination is `Float`, not `Decimal`.
