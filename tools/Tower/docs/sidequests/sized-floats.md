# Plan: Sized floats `F32`/`F64` — implement + precision math (D-FLOATW1)

**Status: plan — sized-float spelling is RATIFIED (D-SG9) but UNIMPLEMENTED; one
open precision/math question (D-FLOATW1).**

Unblocks: **Marcus** (numerical computing — control scalar precision and memory
layout).

---

## Goal

`F32`/`F64` (and the full sized menu) are **already ratified** under **D-SG9** as
the expert/FFI sized-type spelling, with `Float = F64` as the beginner default.
But they are **not implemented** — `grep F32 Source/` finds nothing. Marcus's
"single Float, no precision control" gap is therefore an *implementation* gap on a
*ratified* decision, plus one genuinely-new question: what does precision-correct
math (`sqrt`, `sin`, …) look like across `F32`/`F64`?

So this card is mostly **build the ratified D-SG9 sized floats**, with a small
open decision on the math/precision surface.

Verified: D-SG9 lists `F32`/`F64` (`syntax-decisions.md:581`); `Float`=f64
(line 578); conversions are named methods (`x.to_i32()` etc., already specified);
no `F32` in `Source/`.

## Pipeline touch points (ratified D-SG9 build — no new decision)

- **parser/sema** (`Syntax.rs`, `Sema/`): recognize `F32` (and the sized-int menu)
  as types; literal inference defaults to `Float`/`Int`; explicit annotation picks
  the sized type. `to_f32()`/`to_f64()`/`to_i32()`… conversion methods.
- **codegen** (`Codegen/Context.rs`): lower `F32`→`f32`, `F64`→`f64`, sized ints
  to their Rust widths.
- **diagnostics**: E0030 (`as` rejected → named conversions) already specified;
  ensure it covers the sized forms. A narrowing-conversion teaching diagnostic.

## Open question (needs owner decision — D-FLOATW1)

The only *undecided* piece: **precision-correct math on sized floats.**

1. **Math surface across widths** — does `core.math.sqrt(x)` work on both `F32`
   and `F64` (overloaded/generic over the float type, returning the same width),
   or is there an `F32`-specific path? Today `core.math` is `Float`(f64)-only.
2. **Literal precision** — does an `F32` binding accept a literal that loses
   precision silently, warn, or require an explicit `.to_f32()`? (precision-loss
   is a real numerical-computing footgun Marcus cares about).
3. **Mixed-width arithmetic** — `f32 + f64`: error requiring explicit conversion
   (consistent with "no implicit widening", D-SG9), confirmed for the float case.

(The *type spellings* are settled by D-SG9; D-FLOATW1 only fixes the math/precision
policy on top.)

## Invariants in play

- **I7** the sized spellings already live in `Syntax.rs`/D-SG9 — implement against
  them, don't re-mint.
- **D-SG9** ratified: no `as`, no implicit widening, named-method conversions.
  Honor all of it.
- **I5** example using `F32` data + math; golden output.

## Test plan

1. `examples/features/sized_floats.jet` — `F32` array, math over it, print a
   checksum; golden output (I5).
2. Conversion-method tests (`to_f32`/`to_f64` round-trips, precision loss visible).
3. Mixed-width `f32 + f64` → diagnostic snapshot.
4. Codegen test: `F32` lowers to Rust `f32` (memory width assertion).
