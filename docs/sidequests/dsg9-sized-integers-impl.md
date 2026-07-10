# Implementation plan: D-SG9 — sized integers (I8/I16/I32/I64/U8/U16/U32/U64)

**Status: spellings RATIFIED, UNIMPLEMENTED.** The deepest type-system change of the
four. Companion to the existing `sized-floats.md` stub (F32/F64 ride the same machinery).

## 1. Ratified decision + spec ref

- **D-SG9 / S42** — `syntax-decisions.md:573–588`: `Int`/`Float` are the beginner
  defaults (`Int` = i64, `Float` = f64). A full **sized menu** for experts/FFI/binary:
  **`I8 I16 I32 I64 U8 U16 U32 U64 F32 F64`** (line 580–581). Conversions are
  **named methods only** — `x.to_i32()`, `n.to_float()`, `Int.parse(s)` — **no `as`
  keyword** (E0030 teaches the named forms), no C/Go cast punctuation. **No implicit
  widening** (line 587). Lowercase Rust spellings (`i64`) are rejected.
- Underpins **D-NUMOPS1** and unblocks **embed_bytes (c75)** (needs `U8`/`[U8]`).
- Today: `Type` enum is `Int`/`Float` only (`Source/AST.rs:18`). `U8` is special-cased
  as a `Type::Named("U8")` → `u8` in `Codegen/Context.rs:175` — the *only* sized type
  with any support, and it's a hack to be subsumed.

## 2. Failing-test-first targets

1. **`examples/features/76_sized_ints.jet`** + `expected/76_sized_ints.out` (golden,
   I5): bind `a: I32 :: 7`, `b: U8 :: 255`, do width-correct arithmetic, convert
   (`a.to_i64()`, `b.to_i32()`), print. Golden output proves widths behave (e.g. `U8`
   wrap/overflow behaviour documented; arithmetic on `I32` stays i32).
2. **`tests/ui/sized_no_implicit_widen.rs`**: `i32_val + i64_val` → **E0151** "these are
   different number types" requiring an explicit `.to_i64()` (no implicit widening,
   D-SG9).
3. **`tests/ui/sized_narrowing.rs`**: assigning a wider value to a narrower type without
   a conversion → **E0152** (narrowing must be explicit + may lose data).
4. **`tests/ui/sized_as_rejected.rs`**: `x as I32` → existing **E0030**, confirm it
   names the sized conversion methods (`.to_i32()`).
5. **Codegen width test**: `I32` lowers to Rust `i32`, `U64` to `u64`, etc. (assert in a
   codegen/golden test).
6. **`tests/ui/sized_lowercase_rejected.rs`**: `x: i64` → teaching error (rejected
   lowercase spellings, D-SG9). Reuse the E0013-style "the type is called `Int`/`I64`"
   pattern in `Parser/Types.rs`.

## 3. Pipeline work, in order

This is the type-system spine, so changes ripple. Touch points by file:

### Syntax — `Source/Syntax.rs`
Add (I7) consts for all eight sized-int spellings (and F32/F64 — coordinate with
`sized-floats.md`), each with the D-SG9 comment. Add them to the all-keywords/type-name
array near line 896 (the `TYPE_INT, TYPE_FLOAT, …` list).

### AST — `Source/AST.rs` (the foundational change)
Extend the `Type` enum. Two viable shapes:
- **Preferred:** one variant carrying width+signedness:
  `Int { bits: u8, signed: bool }` plus keep `Int` (= i64 default) — OR fold the default
  `Int` into `Int { bits: 64, signed: true }` and make `Type::Int` an alias constructor.
- **Simpler/mechanical:** add discrete variants `I8, I16, I32, U8, U16, U32, U64`
  (I64 == existing `Int`, U… new), and `F32` (F64 == existing `Float`).
Pick the parameterised form — it keeps `Type::show()`, cloneability/comparability checks
(`Codegen/Context.rs:587,624`), and future widths from exploding into match arms.
Whichever shape: update **every** `match` on `Type` — `show()`/`name()` in `AST.rs`,
`rust_type` in `Codegen/Context.rs:149`, the `type_is_cloneable`/`type_is_comparable`
scalar arms (587/624), and any exhaustive matches in `Sema/`. The compiler will list
them once the enum changes — that's the work-finder.

### Lexer — `Source/Lexer/`
No new token kind (these are idents). Width-suffixed literals are **not** in D-SG9
(conversions are named methods, literals stay untyped per S42) — do **not** add `7i32`
literal lexing. Literal type is fixed by the binding's annotation via inference.

### Parser — `Source/Parser/Types.rs`
In the `TokKind::Ident(name)` type match (line 336–342, alongside `TYPE_INT`/`TYPE_FLOAT`)
add arms for each sized spelling → the new `Type` variant. Add the lowercase-rejection
teaching error (E0013-style) for `i64`/`u8`/`f32` etc.

### Sema — `Source/Sema/` (the careful part)
- **Inference** (`Source/Sema/CheckerInfer.rs`): an untyped integer literal defaults to
  `Int` (i64); when the binding/param/field is annotated `I32` etc., the literal takes
  that type (range-check the literal fits the width → **E0153** literal-out-of-range).
- **Arithmetic / mixed-width** (`CheckerCore.rs`, `Collections.rs` method table): binary
  ops require **both operands the same sized type**; `I32 + I64` → **E0151** (no implicit
  widening). Same-width op yields that width. `Int` (i64) and `I64` are the same type.
- **Conversions** (`Source/Collections.rs:74–141` is the method-type table; mirror in
  `Codegen/Expression.rs:853`): add `to_i8/to_i16/to_i32/to_i64/to_u8/to_u16/to_u32/
  to_u64/to_f32/to_f64/to_int/to_float` for every numeric type. Narrowing conversions are
  allowed *via the explicit method* (that's their purpose); plain assignment that narrows
  without a method → **E0152**.
- **`as` rejection**: E0030 already exists (`Parser/Expressions.rs:1049`); ensure its
  fix-it text enumerates the sized `.to_iN()` forms.
- Subsume the `U8` hack: remove the `Type::Named("U8")` special-case once `U8` is a real
  variant (`Codegen/Context.rs:175`, and the `embed`/`[U8]` paths that rely on it —
  grep `Named("U8")` and `"U8"` before deleting).

### Codegen — `Source/Codegen/Context.rs` + `Expression.rs`
- `rust_type`: map each sized variant to its Rust width (`I32`→`i32`, `U64`→`u64`,
  `F32`→`f32`, …). `Int`→`i64`, `Float`→`f64` unchanged.
- Conversion methods (`Codegen/Expression.rs:853`): emit `(expr) as <rustwidth>` for each
  `to_iN`/`to_uN`/`to_fN` (Rust `as` is the safe lowering here — sema already proved the
  conversion is an explicit named call, I3). Integer-literal codegen must emit the
  literal with the right suffix where Rust needs it for inference.
- **Comptime** (`Source/Comptime/Builtins.rs:150`): the interpreter (used by `jet dev`,
  `jet eval`, REPL) must mirror the same conversion + width semantics so interpreted
  stdout == compiled stdout (the I2 differential battery, E2-M4). This is an easy step to
  forget — add comptime arms for every new conversion and width-aware arithmetic.

## 4. Diagnostics (E013x–E015x block is EMPTY; E0140/E0141/E0150 reserved by c71)

Use **E0151+** to avoid colliding with c71's reserved E0140/E0141/E0150:

| Code | What | Why | Fix |
|------|------|-----|-----|
| **E0151** | "`<a>` and `<b>` are different number types (`I32` vs `I64`)" | no implicit widening — mixing widths is silent-bug territory (D-SG9). | "convert one explicitly: `a.to_i64() + b`" |
| **E0152** | "this would shrink an `<I64>` into an `<I32>` and could lose data" | narrowing must be explicit. | "convert with `value.to_i32()` if you mean to" |
| **E0153** | "`<n>` doesn't fit in `<U8>`" | a literal annotated to a width must fit it. | "`U8` holds 0–255; use a wider type or a fitting value" |

Existing **E0030** (no `as`) is reused, not re-minted. Each new code needs a `tests/ui/`
snapshot + `jet explain` (I4). Confirm E0151–E0153 are free (E013x–E015x is empty today;
only c71 reserves E0140/E0141/E0150 — coordinate so the two cards don't both grab E015x).

## 5. Examples

- `examples/features/76_sized_ints.jet` (the golden in §2.1) — sized bindings,
  width-correct arithmetic, explicit conversions, print. Expert-tier framing.
- Coordinate with `sized-floats.md` for the F32/F64 example (`sized_floats.jet`) — same
  machinery; consider shipping them together since the enum change is shared.

## 6. Exit criteria

- All §2 tests green; `76_sized_ints.jet` builds and matches golden output.
- `Type` enum carries sized widths; every `match Type` arm updated and compiles.
- The `Type::Named("U8")` hack is removed and `[U8]`/embed paths still pass.
- E0151–E0153 have ui snapshots + `jet explain`; E0030 fix-it mentions sized methods.
- Comptime/interpreter mirrors the semantics (I2 differential battery green).
- `tests/golden.rs` + full `cargo test` green; no `unsafe`.

## 7. Effort / risk + one-pass judgment

This is the **deepest type-system change** of the four. The `Type` enum is matched in
many places (codegen, sema cloneability/comparability, inference, comptime); changing it
forces an audit of every arm. The genuinely hard/easy-to-miss parts:
- **No-implicit-widening** arithmetic checking touches the core binary-op type rules.
- **Comptime mirroring** (I2) — interpreter must match compiled semantics for every width
  and conversion, or the `jet dev`/`eval`/REPL differential tests break. Easy to forget.
- **The U8 hack removal** ripples into embed/`[U8]` (c75's dependency) — must not regress.
- Range/overflow semantics for `U8` etc. must be documented and golden-pinned.

It is mechanical-but-broad rather than conceptually open — there is **no upstream gate**
(spellings are ratified, conversion policy is ratified, no-widening is ratified). Nothing
needs an owner decision. But the surface area is large and the comptime + U8-hack +
mixed-width-arithmetic interactions are exactly where a single pass tends to leave gaps.

**Completable in one focused agent pass? BORDERLINE — lean NO for a fully-correct,
end-to-end pass.** A disciplined agent could land the enum + parser + codegen + basic
conversions in one pass, but getting *all* of (mixed-width rules, every conversion arm,
comptime mirroring, U8-hack subsumption, range diagnostics) correct and tested in a
single unattended pass is unlikely. Recommended split: (a) enum + parser + codegen +
conversions + the F32/F64 floats (shared machinery), (b) no-widening/narrowing arithmetic
rules + E0151–E0153 + comptime mirroring as a second pass. If forced into one pass, scope
it to integers-only (drop floats to `sized-floats.md`) and budget heavily for the
comptime mirror.
