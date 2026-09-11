# Compiler speed: the two-lens law

One compiler core, two lenses. The **JIT lens** gives the rapid development
loop people love in Python/TypeScript. The **AOT lens** produces a highly
optimized binary when the program is ready; longer build time is an accepted
cost, not a bug.

There is never a difference in supported features or behavior between the
lenses. Both consume the same executable TIR from the same front end (R12).
Compile-speed work therefore targets dev velocity through JIT coverage and
latency, and self-hosted AOT architecture that avoids rustc's redundant work.

## Build-speed constraints

When AOT lowers through a generated backend, backend/toolchain work remains an
irreducible cost. The two-lens contract therefore requires:

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
- JIT coverage grows so pure-Jet `jet dev` reloads do not touch rustc.

D-BUILD-DEFAULT1=B sets `jet run` and `jet dev` to the fast profile while
`jet build` remains optimized. D-AOT-CRANELIFT1 is ratified under this law.

## Anti-goals from Xcode / Swift

Jet must not recreate these failure modes:

1. **Incremental worse than clean or no-change multi-minute work.** Unchanged
   and representative-edit runs remain distinct, and unexplained slowdowns are
   compiler bugs.
2. **World rebuilds for a local edit.** Batch dirty sets use module-interface
   fingerprints and dependents only, with sealed package objects for link
   restore. Text sources remain the truth; no opaque IDE project database.
3. **Type checker “gave up” with a useless wide span.** Diagnostics stay
   bounded and pinpointed; users must not reshape working code to soothe the
   compiler.
4. **Tool debugging before code debugging.** Cache purge, clean, or reopen
   must not be required to make diagnostics stable and explainable.
5. **Dev profile silently becoming ship profile.** Cache identity includes
   profile, target, and backend; differential checks protect tier behavior.


## Self-hosted compiler principles

rustc is slow for identifiable, avoidable reasons. A self-hosted compiler uses
the following principles:

1. **No redundant verification.** rustc re-checks what its own front end
   already knows (and in our transpile era, re-checks what Jet's sema already
   proved). In the self-hosted compiler, sema remains the single gatekeeper
   (R2); the backend consumes proven TIR and emits code — no borrow-check, no
   trait-solve, no inference at emit (I3 carried forward).
2. **Query-based incrementality from day one.** rustc retrofitted incremental
   compilation onto a batch design and it still invalidates coarsely. Jet's
   front end is already organized around jet-queries; the self-hosted compiler
   keeps function/module-granular memoization as its spine, so an edit
   re-checks the touched item plus dependents, not the world.
3. **Parallel by construction.** Per-module lex/parse/sema fan-out with one
   serial cross-module resolve point; no global mutable context like rustc's.
4. **Monomorphization under our control.** Share generic instances, outline
   cold ones, cap duplication — the classic LLVM-input blowup rustc suffers is
   a policy choice we own in TIR lowering.
5. **Tiered backends off one TIR.** The JIT lens (Cranelift) and the AOT lens
   (optimizing backend) are two consumers of the same executable TIR. Feature
   parity is structural — one front end, one TIR — and enforced by the R12
   differential gates (same program, same output, every tier).
6. **Optimization budget is spent where the user said it matters.** AOT may be
   slow because it is the ship step; the perf.<role> budget vocabulary lets an
   expert dial optimization scope, while the beginner default just works.


## Honest constraints

The development loop can avoid optimizer work through JIT coverage, cached
stdlib/runtime objects, and incremental semantic checking. A release build still
owes the optimizer the work the owner requested; speed comes from avoiding
redundant front-end work and using precise incrementality, not from skipping
optimization.

