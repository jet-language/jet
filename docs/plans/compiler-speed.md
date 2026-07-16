# Compiler speed: the two-lens law and the self-hosted era

Plan of record, 2026-07-16 (reframed same day per owner direction).

## The law (owner, 2026-07-16)

One compiler core, two lenses. The **JIT lens** gives the rapid dev loop people
love in Python/TypeScript. The **AOT lens** produces a highly optimized binary
when the program is ready — longer build time is an accepted cost, not a bug.
There is **never** a difference in supported features or behavior between the
lenses; both consume the same executable TIR from the same front end (R12 is
the enforcement mechanism). Compile-speed work therefore has two distinct
targets:

1. **Dev velocity** — won by JIT-lens coverage and latency, not by
   de-optimizing AOT.
2. **AOT build time in the self-hosted era** — won by architecture choices we
   bake in now, so the self-hosted compiler never inherits rustc's fate.

## Interim reality (transpile era)

While AOT rides generated Rust through hidden rustc, optimized AOT build time
has a floor we do not control (rustc re-parses and re-verifies everything, then
LLVM optimizes). Interim levers, all invariant-clean:

- Instrument the rustc/link step (`PhaseTiming` never laps it today) + a
  compiler self-speed benchmark corpus vs cargo in CI. You can't beat a number
  you don't record.
- Fast linker (mold → lld → system), tuned rustc flags.
- Precompiled stdlib object store keyed on (prelude, rustc identity, profile) —
  the prelude currently recompiles from source text on every build. R10
  pay-for-what-you-call preserved.
- Promote the jet-queries demand cache (LSP-only today) onto the batch compile
  path: per-module memoized lex/parse/check, invalidate dependents only.
- Hand-rolled (I6) parallel per-module front end; deterministic diagnostics.
- Widen JIT TIR coverage so `jet dev` reloads never touch rustc for pure-Jet
  programs.

D-BUILD-DEFAULT1=B settles everyday defaults: `jet run` and `jet dev` use the
fast profile; `jet build` remains optimized. The remaining open ballot on card
#666, D-AOT-CRANELIFT1, decides the AOT mechanism under the two-lens law.

## Self-hosted era: why Jet's compiler won't be slow like rustc

rustc is slow for identifiable, avoidable reasons. Each is a design bet the
self-hosted compiler makes now, while the architecture is still cheap to shape:

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

Exit criteria for the self-hosted compiler (measured by the Stage-0 corpus):
clean-build and incremental-build medians better than equivalent cargo builds
at every corpus size for the dev/JIT path; AOT optimized builds within a stated
factor of cargo release while producing competitive binaries; zero R12 parity
diffs across lenses on the golden suite.

## Honest physics

Dev loop: strictly less work than rustc (no re-verify, no optimizer, cached
stdlib, incremental sema) — large multiples are realistic. Optimized AOT in
the transpile era: parity with cargo release at best; claiming otherwise is
dishonest. Self-hosted optimized AOT: the optimizer pass is irreducible —
"faster than rustc release" comes from skipping redundant front-end work and
better incrementality, not from skipping optimization the owner asked for.

## Board

Card #666 (interim + instrumentation + ballots). Epoch e9 (Bootstrapping)
cards carry the self-hosted architecture bets; the e9 readiness wave folds
this plan's self-hosted section into its cards.
