# Type inference: where Jet works out a type today

Date: 2026-10-30. Card #6325. Probes live in
`$JET_SCRATCH_ROOT/lanes/SurfaceDesign/probes/` (`m6325`, `m6325b`, `m6325c`).
Every probe printed the same output on o0, o1 and o2 with the base compiler at
bd83fe6361.

## The ratified rules

- Signatures and fields always write their types. An omitted result means
  Unit, so a value body is E0433 (`syntax-decisions.md:8062-8065`).
- Bindings carry no type. A type rides the value, as `U8{250}` or an inferred
  `{…}`; `name: Type :: value` is retired with E0393 (`syntax-decisions.md:244-248`,
  `spec.md:191-193`).
- D-INFER-LEVEL1=A (2026-10-11): an empty list, map or None starts with its
  type. The error should read the next line, name the implied type and let
  `jet fix` write it.
- Lambdas passed to calls take their parameter types from the call
  (D-LAMBDA-INFER1, `syntax-decisions.md:441-447`). A stored lambda follows
  the declaration rule (D-LAMBDA-STORED1=A, 2026-10-30).
- Calls infer generic arguments; `call<T>(…)` is always allowed
  (S33, D-GENERIC-CALL1=A, `syntax-decisions.md:1144-1147`).
- `.new(…)` may omit its receiver when the expected type fixes it (D-SHAPE3a=A,
  line 4945).
- Effects are inferred and stored in public API snapshots (D-EFFECT-OMIT1=A,
  lines 3260-3268).
- One numeric widening law covers operators, arguments, returns and
  assignments. Jet "never narrows implicitly" (D-INTLIT-WIDTH1=F,
  D-NUMWIDEN-CROSS1=E, lines 1097-1100).
- Checking is one pass and never picks a function by its result type
  (D-INFER-LINEAR1=A, 2026-10-29).

## Where Jet infers, probed

| Site | Probe | Result |
| --- | --- | --- |
| `x :: 1` | i01 | `Int` |
| `d :: 2.5` | i02 | `Decimal`; passing it to a `Float` parameter is E0108 |
| `half(2.5)` with `f: Float` | i03 | the literal becomes `Float`; prints 1.25 |
| `[1, 2].map(n -> n * 2)` | i11 | parameter from the call |
| `n -> "{n}!"` passed to `map` | i38 | result `[String]` |
| `same(3)` for `fn same<T>` | i15 | `T = Int` |
| `empty()` for `fn empty<T>() -> [T]` | j14 | E0904 "Can't infer `T`"; `empty<Int>()` works (j15) |
| `show({n: 4})` with `p: P` | i19 | `{…}` takes `P` |
| `name(.Green)`, `c == .Red` | j02, j03 | dot case takes the enum |
| `use_it(.new())` | j04 | receiver from the parameter |
| `fn none() -> [Int] { [] }`, `count([])` | j09, j10 | `[]` from the result or parameter |
| `["a": 1]`, `[1, 2]`, `Val(3)` | i22, i28, i23 | map, list, optional from the items |
| `loop i in 0..<3 -> i * 2` | i28 | `[Int]` |
| `Box{value: 3}` for `Box<T>` | i27 | `Box<Int>` |
| `f :: twice` | j17 | function value |
| `(n: Int) -> n + base` stored | j13 | result inferred; runs |
| `fn greet() { print("hi") }` | i33 | effects inferred |

## Where Jet requires a written type, probed

| Site | Probe | Result |
| --- | --- | --- |
| Function parameter | i17 | E0003 "Expected `:` and a type" |
| Struct field | i18 | E0003 |
| Function result with a value body | i16 | E0433 |
| `names := []` | i04 | E0501; `[String]{}` works (i05) |
| `o := None` | i07 | E0308; `Int?{None}` works (i08) |
| `c :: .Red` | j01 | E0330 "Can't infer the enum type" |
| `p :: {n: 4}` | i20 | E0119; the fix names `Point{ x: 1 }` |
| `r :: Ok(3)` | i31 | E0404 |
| `sq :: (n) -> n * n` | i12 | E0801 "Tell me the type of `n`" |
| `[1, 2.5]` | i21 | E0504; no common type is searched |
| `if c -> 1 else -> 2.5` | i29 | E0124 |
| `x + 0.5` with `x :: 1` | i30 | E0133 |

These errors share one shape. A headless value with no expected type is an
error that names the head to write. A number with no expected type takes its
default, `Int` or `Decimal`.

## Gaps against the ratified rules

- **Implicit narrowing at calls and returns.** An `Int` passes to a `U8`
  parameter: `take8(n)` with `n` read at run time prints 200 (`m6325c/k01`).
  With 300 it stops at run time with E3010 (k02). An `Int` parameter relays
  into `U8` (k04), and `-> U8 { n }` returns an `Int` (k07). The same value
  is E0108 in a struct field (k06), a list item (k08) and from `I64` (k12).
  This contradicts "never narrows implicitly".
- **The E0501 text** does not name the next line or the implied type yet.
  It still suggests `[Int]{}` (i04 output). D-INFER-LEVEL1=A is not built.
- **Stored lambdas** without a result type still run (`i13` prints 9).
  D-LAMBDA-STORED1=A is not built. The ratified `fn(n: Int) -> Int { … }`
  value form is E0003 (i14).
- **The spec still lists `[]: [T]`** as always accepted (S78,
  `syntax-decisions.md:1551-1552`). It is E0003 (i06). D-BIND-BARE1 and
  D-DOTCTOR3 replaced it with `[T]{}`.
- `[:]` is E0003 (i37). `[String: Int]{}` works (j11).

## Which inferences hurt

Readability suffers where a type comes from far away. Jet keeps that short:
every inference above reads from the same line or the signature being
called. No probe found a type fixed by a later line.

Error quality suffers where a name's type depends on later use. Rust's `let
x = 200; take8(x)` makes `x` a `u8` from its use. Its errors then point at
a use, not at the binding. Jet's binding keeps the default, so the error
points at the call. With the narrowing gap closed it would be E0108 there.

One Jet case surprises beginners. Naming a literal changes its type:
`half(2.5)` works but `d :: 2.5; half(d)` is E0108 (i02, i03). The same
refactor with an integer only works today through the narrowing gap (j07,
j08).

## Other languages

| Language | Locals | Named literal used as a smaller type | Signatures |
| --- | --- | --- | --- |
| Rust | inferred within the function, from later uses too | `let x = 200; take_u8(x)` compiles | written |
| Swift | inferred from the initializer and context | `let x = 200` is `Int`; passing to `UInt8` is an error | written; closures inferred |
| Zig | `const x = 200` is `comptime_int` and fits any width | works for `const`; `var` needs a type | written |
| Odin | `x := 200` is `int`; `X :: 200` is an untyped constant | works for `::` constants | written |
| Go | `x := 200` is `int`; `const x = 200` is untyped | works for `const` only | written |
| Kotlin | inferred from the initializer | `val x = 200` is `Int`; passing to `Byte` is an error | written for public API in explicit API mode |

Zig, Odin and Go split the two cases by kind of name. A compile-time constant
stays flexible; a run-time variable takes its default. Swift and Kotlin fix the
type at the name. Rust alone lets a later line decide.

## Ratified versus built

| Item | Ratified | Built |
| --- | --- | --- |
| Written signatures and fields | yes | yes |
| Empty list, map, None start typed (D-INFER-LEVEL1) | yes | yes, without the forward-scan message |
| Stored lambda declaration rule (D-LAMBDA-STORED1) | yes | no (i13) |
| `fn(…) -> T { … }` lambda value | yes | no (i14) |
| Never narrow implicitly (D-NUMWIDEN-CROSS1) | yes | no at calls and returns (k01-k07) |
| `[]: [T]` ascription (S78 text) | retired by later rulings | spec text stale |
