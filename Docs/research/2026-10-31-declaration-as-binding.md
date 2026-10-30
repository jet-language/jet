# Declarations as bindings, and the lambda shape

Cards #4735 (D-DECL-FORM2, now D-DECL-BINDING1) and #6251 (D-LAMBDA-SHAPE1,
D-LAMBDA-STORED1). Design note for two ballots. Nothing here is implemented.

The owner asked that the lambda shape align with "the planned conversion to the
Jai-style bindings", where a function is declared as `my_function :: (...)`.
No Tower decision or card ratifies that conversion. D-DECL-FORM2=A (2026-10-07)
ratified the opposite: `struct Point` and `fn double` keep their leading word.
So the conversion needs its own vote, D-DECL-BINDING1, and D-LAMBDA-SHAPE1 is
rewritten to follow it.

## 1. Tower decisions that bear on this

- D-DECL-FORM2=A, card c04j9ahx: named structs, enums, traits, functions and
  modules start with their word; `Point :: struct { … }` is refused with a fix.
  D-DECL-BINDING1=A reverses it.
- D-LAMBDA-SHAPE1, open, card c0rqt0kq: version 1 proposed `fn(n) { n * n }`.
  Version 2 asks whether a lambda is exactly the bound value `(n) { n * n }`.
- D-LAMBDA-STORED1=A: a lambda stored with no expected type writes parameter
  and result types. The rule stays; only the spelling changes.
- D-LAMBDA-IFACE2=A: a lambda writes no result type. Both new ballots amend it.
- D-LAMBDA-INFER1=A and D-LAMBDAINFER1=A: parameter types may be left out when
  the expected type gives them. Unchanged.
- D-CALLABLE-ONE1=A: one callable shape, "a function is a named lambda".
  D-DECL-BINDING1=A makes that literal.
- D-SIG-AFTER1=A: one arrow after the inputs, braces on every named body, and
  `-> Point {` always opens the body. The head shape stays; the name moves.
- D-GENERIC-TYPEFN1=A: generics are prep parameters and `prep fn` recipes.
  Under D-DECL-BINDING1=A a recipe is `Pair :: prep (T: Type) -> Type { … }`.
- D-BIND-BARE1=A: only `::` and `:=` bind. Unchanged.
- D-CASE-LAW1=A: fixed module-level bindings are SCREAMING_SNAKE. A amends it,
  so a function binding stays snake_case and a type binding PascalCase.
- D-ALIAS-BIND2=B (keep `alias`) and D-VIS-REEXPORT1=A (`clamp ::
  core.math.clamp`) already put the name first. Unchanged.
- D-MOD1 (spec-only import): `module math { … }` becomes `math :: module { … }`
  under A; the file form `module math;` stays.
- D-TUPLE-DESTRUCT1=A: tuples have named members, `(x: 1, y: 2)`. This is the
  value a function head must be told apart from.
- D-DOTCTOR3=A: `Type{ body }` is a typed literal. It is why
  `(n: Int) -> Int { n * n }` runs today as a typed value.
- D-CALLVALUE1=B and D-MARKER-DECL1=A: unchanged; markers stay above the
  binding.

## 2. What happens today

Probes live in `/mnt/jetscratch/lanes/SurfaceDesign/probes/m-decl`. Each ran
with the dev jet at `--optimize=none` and `full`, and d8 and d10 also at
`basic`.

- `d1_lambda_today.jet`: `square :: (n: Int) -> n * n`, `n -> n * 2` and
  `(n) -> { n + 1 }` all run.
- `d2_tuple.jet`: `p :: (x: 1, y: 2)` is a tuple and prints 3.
- `d3_bind_fn.jet`: a module-level `square :: (n: Int) -> Int { n * n }` is
  E0357, "Constant `square` must be ALL_CAPS".
- `d4_point_struct_bind.jet`: `Point :: struct { x: Int }` is E0003.
- `d5_fn_value.jet`: a `:=` lambda can be reassigned and prints 3.
- `d6_local_recur.jet`: a local lambda calling itself is E0804.
- `d7_fn_literal.jet`: `fn(n) { n * n }` as a value is E0003.
- `d8_program_today.jet`: the D-DECL-BINDING1 program in today's spelling,
  minus the generic parts, prints 7, 4.0, [1, 4, 9], [1, 4, 9], 2, 120 and 7.
- `d10_lambda_today.jet`: the D-LAMBDA-SHAPE1 program in today's spelling
  prints [3, 4], [10, 20, 30, 40], [9, 16], `> done` and [3, 5, 7, 9].

Writing the program surfaced two checks worth knowing. `#Inline(Always)` on a
function that is also passed as a value is E0918. An error enum named in a
failure rail needs `#Error`, or the result is E2417.

## 3. The design

Every named thing is `name :: value`. A function value is
`(params) arrow result failure { body }`: today's head without `fn` and the
name. A type is `struct { … }`, `enum { … }`, `trait { … }` or another type on
the right. A lambda is the same value without a name, leaving out what the
call gives: `nums.map((n) { n * n })`.

A parenthesized list is a function only when `{`, `->`, `-[` or a failure rail
follows its `)`. So `origin :: (x: 0, y: 0)` stays a tuple, and
`Pt :: (x: Int, y: Int)` stays a tuple type. In `if` and `loop` heads the test
is off, as Rust does for struct literals. A scan of every `.jet` file found no
`if (name) {` or `loop (name) {` head in Jet code.

Inside a type body, `x: Int` is a field and `norm1 :: (self) -> Int { … }` a
method. `Shape.area :: (self) -> Float { … }` adds one outside, replacing
`fn Shape.area`. `impl Type { … }` keeps its head, because it names nothing
new. `::` binds a fixed function that may call itself; `:=` binds a changeable
function value that may not. `fn` remains only in function types.

D-DECL-BINDING1 also shows Odin's form, `square :: fn(n: Int) -> Int { … }`,
and keeping the words. D-LAMBDA-SHAPE1 v2 shows keeping a short arrow beside
the value form. Ballots: `/mnt/jetscratch/lanes/SurfaceDesign/ballot/`
`D-DECL-BINDING1.json` and `D-LAMBDA-SHAPE1.json`.

## 4. Size of the change

`/mnt/jetscratch/lanes/SurfaceDesign/b2/census-decl.mjs` over a dev export of 17,636 `.jet` files counted:

| site | count | under A |
|---|---|---|
| `fn name(` heads | 62,347 | `name :: (` |
| of those, indented members | 49,924 | `name :: (` inside the body |
| `struct Name` heads | 7,168 | `Name :: struct` |
| `enum Name` heads | 1,889 | `Name :: enum` |
| `trait Name` heads | 183 | `Name :: trait` |
| inline `module name {` | 576 | `name :: module {` |
| `impl` blocks | 1,612 | unchanged |
| `fn(` function types | 773 | unchanged |
| markers before a function | 4,808 | unchanged, above the binding |

Main's lambda census (`/mnt/jetscratch/lanes/Main/q/b4/lam/census.mjs`), rerun
on the same export, counts 4,812 lambdas in 1,197 files. Of these, 3,842 have
an expression body, 888 a bare parameter, and 220 are stored in a binding.
Version 1's `fn(…) { … }` form adds 12,437 characters. The bound-value form
adds 2,813, or 0.6 per lambda, because braced lambdas shrink.

## 5. Sources

- Jai: Docs/research/jai-vs-jet-syntax-briefing-2026-09-16.html:234-238 and
  https://github.com/Ivo-Balbaert/The_Way_to_Jai
- Odin: https://odin-lang.org/docs/overview/#procedures
- Zig, RFC to bind functions with `const`: https://github.com/ziglang/zig/issues/1717
- JavaScript arrow functions:
  https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Functions/Arrow_functions
- Kotlin lambdas: https://kotlinlang.org/docs/lambdas.html
- Earlier Jet review: Docs/research/jai-vs-jet-fable-brief-2026-09-16.md:106
