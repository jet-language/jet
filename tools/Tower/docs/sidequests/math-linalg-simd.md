# Plan: Linear algebra + SIMD in the math story (D-MATHLIB1, D-SIMD1)

**Status: plan — awaiting owner decisions D-MATHLIB1 and D-SIMD1.**

Unblocks: **Marcus** (scientific/numerical computing — matrices, vectors, FFT,
vectorized kernels without dropping to Rust FFI).

---

## Goal

`core.math` covers basic `Float` ops only (verified persona note + no matrix/SIMD
code in `Source/`). Marcus must implement linear algebra from scratch or via Rust
FFI, and there are no SIMD intrinsics — the expert tier gives raw memory
(`#Audit`/`#Unsafe`/`Ptr<T>`, verified `48_lowlevel.jet`) but no vectorized math.
Two distinct gaps:

- **A library**: vectors, matrices, decompositions, FFT — a numerics package.
- **A primitive**: SIMD vector types/intrinsics the library (and expert users)
  build on.

## Pipeline touch points

- **D-MATHLIB1 (the library)** — stdlib `jet.math`/`jet.linalg` ring package:
  `Vec3`, `Matrix`, dot/cross/matmul, decompositions, FFT. Likely a bootstrap
  external dep (BLAS/ndarray-style) → **I6 owner approval** (like regex c79),
  with a native-replacement plan before Epoch 3 ends.
- **D-SIMD1 (the primitive)** — a SIMD vector type (`F32x4`, `F64x2`…) and
  intrinsics. Touches **sema** (new types), **codegen** (lower to Rust
  `std::simd`/`core::arch`), and the safety story: are SIMD ops safe-by-default or
  expert-tier behind `#Unsafe`? Interacts with D-SG9 sized floats (the lane type)
  and c82 fixed arrays `[T;N]` (SIMD-friendly layout).

## Invariants in play

- **I6** any bootstrap numerics crate needs owner approval + a native plan.
- **I1** SIMD must stay memory-safe by default; intrinsics that can violate
  bounds/alignment belong behind `#Unsafe`/`#Audit`. Portable safe SIMD
  (`std::simd`) is the safe-by-default path.
- **I8** simplicity ratchet — these are large; each needs a roadmap slot / owner
  sign-off. Surfaced now so the next persona run isn't a surprise.
- **I5** examples for both (a matmul demo; a vectorized kernel).

## Open questions

### D-MATHLIB1 — the numerics library
1. **Scope of v1** — `Vec2/3/4` + `Matrix` + basic ops only, or full
   decompositions + FFT? (game/graphics needs the small vectors; physics sim
   needs matrices + solvers).
2. **Fixed vs dynamic dimensions** — `Matrix<3,3>` comptime-sized (rides c82/S76
   fixed arrays) vs runtime-sized `Matrix(rows, cols)`? Both?
3. **Implementation source (I6)** — native std-only vs bootstrap an external
   numerics crate then native-ize. Owner approval gate.
4. **Naming/home** — `core.math` extension vs a separate `jet.linalg` ring
   package (consistency with regex/csv/toml being ring packages).

### D-SIMD1 — the SIMD primitive
1. **Surface** — explicit lane types (`F32x4`) with intrinsic methods, an
   auto-vectorization hint on a loop, or both? (D-SOA1 layout feeds this.)
2. **Safety tier** — safe portable SIMD by default (`std::simd`) vs expert-only
   behind `#Unsafe`? Where's the line for target-specific intrinsics?
3. **Lane/width portability** — fixed lane counts vs target-detected width;
   fallback when a target lacks the ISA.

## Test plan

1. `examples/features/linalg_matmul.jet` — build two matrices, multiply, print a
   checksum; golden output (I5).
2. `examples/features/simd_kernel.jet` — vectorized add over an `F32` array vs a
   scalar reference, assert equality.
3. Safety: an out-of-lane/unsafe SIMD op requires `#Unsafe` → diagnostic snapshot.
