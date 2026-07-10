# Plan: Linear algebra + SIMD in the math story (D-MATHLIB1, D-SIMD1)

**Status: ratified — D-MATHLIB1 = A and D-SIMD1 = A (2026-06-22). No owner decision open.
Native-vs-bootstrap-crate is an I6 implementation gate (decided per-package like regex),
not an owner syntax decision.**

Unblocks: **Marcus** (scientific/numerical computing — matrices, vectors, FFT,
vectorized kernels without dropping to Rust FFI).

---

## Goal

`core.math` covers basic `Float` ops only (verified persona note + no matrix/SIMD
code in `Source/`). Marcus must implement linear algebra from scratch or via Rust
FFI, and there are no SIMD intrinsics — the expert tier gives raw memory
(`#Unsafe("reason")`/`Ptr<T>`, verified `48_lowlevel.jet`) but no vectorized math.
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
  and c82 fixed-size lists `[T#N]` (SIMD-friendly layout).

## Invariants in play

- **I6** any bootstrap numerics crate needs owner approval + a native plan.
- **I1** SIMD must stay memory-safe by default; intrinsics that can violate
  bounds/alignment belong behind `#Unsafe("reason")`. Portable safe SIMD
  (`std::simd`) is the safe-by-default path.
- **I8** simplicity ratchet — these are large; each needs a roadmap slot / owner
  sign-off. Surfaced now so the next persona run isn't a surprise.
- **I5** examples for both (a matmul demo; a vectorized kernel).

## Resolved — D-MATHLIB1 = A, D-SIMD1 = A ratified 2026-06-22 (no owner decision open)

### D-MATHLIB1 — the numerics library (option A)
- **Home/naming.** Numerics ship as a first-party **`jet.linalg` ring package** (like
  regex/csv/toml), keeping Core small (I8). Not a `core.math` extension.
- **Scope.** Vectors, matrices, dot/cross/matmul now; decompositions/FFT later.
- **Dimensions.** Comptime-sized matrices ride D-FIXARR1/S76.
- **Implementation source.** Native-vs-bootstrap-crate is an **I6 impl gate** decided
  per-package like regex — not an owner syntax decision.

### D-SIMD1 — the SIMD primitive (option A)
- **Surface.** First-class portable lane types (`F32x4`/`F64x2`) with safe ops.
- **Safety tier.** Lowers to portable SIMD with scalar fallback — memory-safe by
  default (I1). Raw target-specific intrinsics stay available behind `#Unsafe`.

## Test plan

1. `examples/features/linalg_matmul.jet` — build two matrices, multiply, print a
   checksum; golden output (I5).
2. `examples/features/simd_kernel.jet` — vectorized add over an `F32` array vs a
   scalar reference, assert equality.
3. Safety: an out-of-lane/unsafe SIMD op requires `#Unsafe` → diagnostic snapshot.
