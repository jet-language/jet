# Compiler speed: the two-lens law

Jet has one compiler meaning and two execution lenses. The **JIT lens** serves
the edit/run loop; the **AOT lens** produces an optimized artifact when the
program is ready. This page is for compiler contributors and users reasoning
about build/run trade-offs. The executable perf schema and gate are in
[`tools/perf/ci-perf-check.sh`](../../../tools/perf/ci-perf-check.sh), with
scenario planning in
[`tools/perf/test-ci-perf-check.sh`](../../../tools/perf/test-ci-perf-check.sh);
[R12 and I9](../philosophy.md) are the semantic constraints.

## One core, two lenses

Both lenses consume the same executable TIR from the same front end (R12).
They must preserve the same supported features and behavior (I9). Compile-speed
work therefore targets development latency through JIT coverage and precise
incrementality, while AOT retains the optimization work requested by the
program's profile.

The Rust host remains the production and reference implementation boundary for
compiler work. A Jet-authored compiler path is a staged port contract: it may
replace redundant work only after the same front-end checks, TIR meaning,
diagnostics, and AOT/JIT parity are proven. This document does not claim that
the staged port has replaced the default compiler or that the Rust reference
boundary has been removed.

## Build-speed constraints

When AOT lowers through a generated backend, backend and toolchain work remains
an irreducible cost. The two-lens contract requires:

- Fast and optimized profiles use explicit linker, optimization, and LTO
  settings. Native builds honor explicit `RUSTC_LINKER`/`CC`; otherwise Jet
  selects an available fast linker without overriding the target's system
  linker.
- Native AOT may reuse content-addressed Prelude/runtime and reachable-Core
  objects. Keys include emitted source, compiler identity, target/profile flags,
  environment, and runtime dependency identity; invalid objects fail open to
  the complete inline program.
- Native binary identity carries exact fixed-runtime and reachable-Core
  digests. Disk objects are digest-verified and bounded.
- Incremental checking uses module interfaces and dependent-only invalidation.
  Staged source parsing is bounded and deterministic, with stable module
  discovery order.
- Pure-Jet `jet dev` reloads should not invoke the Rust host backend for work
  that the JIT can execute.

`D-BUILD-DEFAULT1=B` sets `jet run` and `jet dev` to the fast profile while
`jet build` remains optimized. `D-AOT-CRANELIFT1` is ratified under this law.
The command and backend sources, rather than this paragraph, define which
profile a particular invocation selects.

## Incremental and tier invariants

Jet must not recreate these failure modes:

1. An incremental edit must not become slower than an equivalent clean or
   unchanged run without a diagnostic reason. Unchanged and representative-edit
   cells remain distinct in the perf manifest.
2. A local edit must not rebuild the world. Dirty sets use module-interface
   fingerprints and dependents only; sealed package objects provide the link and
   restore layer. Text sources remain the truth.
3. A type-checking failure must retain a bounded, pinpointed diagnostic rather
   than a wide span that makes the user reshape working code.
4. Cache purge, clean, or reopen must not be required to make diagnostics stable
   and explainable.
5. A development profile must not silently become a ship profile. Cache identity
   includes profile, target, and backend; differential checks protect tier
   behavior.

## Port contract for a Jet-authored compiler

If Jet code takes over a compiler stage, it follows these principles. They are
architecture constraints, not a claim about which stage has already moved.

1. **No redundant verification.** Semantic analysis remains the single
   gatekeeper (R2). The backend consumes proven TIR; it does not rediscover
   user errors through a hidden host compiler.
2. **Query-based incrementality.** Function- and module-granular memoization is
   the spine. An edit rechecks the touched item and dependents rather than the
   whole world.
3. **Parallel by construction.** Per-module lex/parse/sema work fans out with
   one controlled cross-module resolve point and no global mutable context.
4. **Monomorphization is policy.** Generic instances may be shared, cold code
   outlined, and duplication capped during TIR lowering.
5. **Tiered backends consume one TIR.** The JIT lens and AOT lens are consumers
   of the same executable representation; R12 differential gates compare the
   same program and output across tiers.
6. **Optimization follows the declared budget.** AOT may spend longer on the
   ship step, while the default development path avoids work that does not
   affect the current edit.

## Performance gate and dated evidence

The repository gate compares each matched cell and metric independently. Every
non-Rust peer requires Jet/peer below `1.00`; Rust permits parity only at a
Jet/Rust ratio of `1.05` or lower, which is a noise band rather than a win.
Missing, wrong, unavailable, uncovered, mismatched, or inconclusive evidence
cannot be averaged away.

A dated receipt is historical evidence, not a live status claim:
[`performance-receipt.md`](../../audits/performance-receipt.md). The scripts
and their manifest define the measurement protocol; this page records the
meaning-preserving architecture law.
