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
- Fast linker (mold → lld → system), tuned rustc flags. Native rustc builds
  honor explicit `RUSTC_LINKER`/`CC`; otherwise Jet selects mold, then lld
  through the C driver, and leaves the target's system linker alone when
  neither is available. The selected driver/backend enters cache and timing
  identity. Fast builds pass explicit `opt-level=0`, `codegen-units=256`, and
  `lto=off`; optimized AOT passes explicit `opt-level=2`, thin LTO, and strip.
- Reusable stdlib objects: native AOT splits the fixed Prelude/runtime and the
  reachable Core closure into content-addressed `jet_runtime` and
  `jet_runtime_core` rlibs. Their keys include the exact emitted/exported
  source, rustc identity, target/profile flags and environment, plus the
  runtime dependency key. A warm build links the objects instead of compiling
  them again; corruption, rejection, or malformed markers stay visible and
  fail open to the complete inline program. R10 pay-for-what-you-call is
  preserved.
- Relevant-digest cache: the native binary key carries the exact fixed runtime
  and reachable Core-closure digests, not a hash of the compiler executable.
  The fixed-runtime digest is memoized per compiler process and the Core digest
  cache is bounded and keyed by the sorted used-Core closure. Disk objects are
  digest-verified and the shared runtime cache is bounded.
- Promote the jet-queries demand cache onto the batch compile path: per-module
  memoized check, module-interface fingerprints, and dependent-only invalidation.
- Hand-rolled (I6) bounded staged-source lex/parse fan-out for open and
  already-discovered disk sources; deterministic diagnostics and stable module
  discovery order.
- Widen JIT TIR coverage so `jet dev` reloads never touch rustc for pure-Jet
  programs.

D-BUILD-DEFAULT1=B settles everyday defaults: `jet run` and `jet dev` use the
fast profile; `jet build` remains optimized. D-AOT-CRANELIFT1 is ratified on
card #666 under the two-lens law.

## Interim closeout contract (#666)

The closeout measures the production path, not a replacement benchmark path.
The cross-backend proof runs the same checked example through optimized AOT and
the default tiered lens, then compares exit status, stdout, and stderr. A
resident Cranelift row names resident execution; a deopt-interpreter row names
the interpreter path. A missing row, a newly broken oracle, a tier refusal, or
a divergent result is a failed differential, not a workload exclusion.

The speed proof uses the existing phase reports and typed `CompilerProbe`
provider. Its corpus pins each source and expected-output digest. The `dev`
profile invokes the production Cranelift JIT lens; optimized `release` invokes
the production rustc AOT lens. Each measured row records compiler and Core
identities, target, profile, backend, linker, host, cache state, fixed
warmup/sample counts, elapsed variance, peak RSS, and phase totals. Clean,
no-change, and representative-edit measurements remain separate.
Missing, partial, stale, or incompatible evidence is unavailable/failure; it
never passes by changing the corpus, hiding a cold run, or increasing warmups.

The report may establish a measured baseline only for the pinned machine and
toolchain identity. It does not claim the clean/incremental JIT and optimized
AOT budgets until those rows and their variance are present. The same report
must retain the failure rail for cache invalidation, nondeterminism, pathological
inputs, and unstable samples.

The checked-corpus dashboard uses one warmup and twenty measured samples for
each clean and incremental JIT/AOT row. Its committed policy allows at most
15% latency or peak-RSS regression against the pinned row and a range/median
sample variance of at most 100%; output and phase records must remain exact.

## Anti-goals from Xcode / Swift (Theo, 2026-08-05)

Source: https://youtu.be/zqOrriq20Tc (~30:50–42:00). Crosswalk card **#1498**.
Owner gates: **D-INCR-UNIT1**, **D-TYPECHECK-BOUND1**. Do not fork this plan.

Jet must not recreate these failure modes:

1. **Incremental worse than clean / no-change multi-minute work.** Unchanged and
   representative-edit runs belong in the #1023 corpus. Unexplained incremental
   slowdowns are bugs, not folklore.
2. **World rebuilds for a local edit.** Batch dirty sets use module interface
   fingerprints and dependents only (#1026), with sealed package objects for
   link restore (D-LIB-REUSE1=B, #1422). Text sources only — no opaque IDE
   project databases.
3. **Type checker “gave up” with a useless wide span.** Users must not reshape
   working code to soothe the compiler. Bound + pinpoint diagnostics are the
   product shape under D-TYPECHECK-BOUND1 (pending). No guessed types (I3).
4. **Tool debugging before code debugging.** Cache purge / clean / reopen that
   “fixes” errors is a compiler bug. Diagnostics stay stable and explainable
   (I2/I4).
5. **Dev profile ≠ ship profile by accident.** Cache identity keeps
   profile/target/backend; #666 differentials catch archive-vs-run style drift.

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

Card #666 (interim + instrumentation + closeout). Cards #1023–#1028 are the
split delivery slices. #669 owns self-host architecture evidence. #1498 owns
Theo/Xcode anti-goal crosswalk and the D-INCR-UNIT1 / D-TYPECHECK-BOUND1
ballots. Frozen #676 stays scoped to its own video source. Epoch bootstrapping
cards carry the self-hosted architecture bets. Sealed package objects, library
export, and the no-stable-ABI stance live in
[`../sidequests/library-reuse-and-linking.md`](../sidequests/library-reuse-and-linking.md)
(#1421, ratified).

## Script-speed warm reuse (#741)

After D-ONECORE1 / D-LENS-RUN2 (#778) and WatchService (#439), default
`jet run` is tiered. Warm reuse sits at that tier boundary — not on the AOT
`BuildCache` path, and not as a second runner or daemon.

- **Key:** entry + WatchService watched paths (content + mtime/len) + cheap
  compiler identity + program args. Config: `JET_RUN_CACHE_DIR` or
  `~/.cache/jet/run`.
- **Hit (tier-1 native):** reload captured Cranelift machine code
  (`module.bin`) and invoke. Skip load, parse, check, TIR lower, and codegen.
  Trace: `JET_RUN_TRACE=1` prints `[run-cache] hit|store`.
- **Artifact contents:** machine code, compile-time string slots, and the
  entry's error rail — whether the entry returns a `Result`, whether the error
  is the default `Err` or a packed enum, and that error type's name. A warm run
  has no program left to ask, so the rail travels with the code. The artifact
  carries a FORMAT version and a reader refuses any other version: an artifact
  without a rail would render the error and still exit 0.
- **Miss / interp deopt:** ordinary tiered path; store a module only when a
  full native capture exists. Whole-program interpreter runs stay correct; they
  do not invent a parallel cache.
- **Signpost:** one stderr `jet dev` tip after a slow cold compile (≥200 ms),
  TTY-only, suppressed by `NO_COLOR` / JSON, once per process.
- **Budgets:** fixtures and method live under
  `tests/fixtures/script_speed/`. Provisional CI sanity: warm no-op under
  100 ms. Peer-parity threshold awaits `D-SCRIPT-BUDGET1`.

## Script-speed warm reuse (#741)

Default `jet run` stays on the JIT lens (D-LENS-RUN1 / D-LENS-RUN2). After
#778 tiered run and #439 WatchService, unchanged scripts reuse a **tier-1
module cache** at the boundary — not AOT `BuildCache`, not a second runner or
daemon.

- **Key:** source + WatchService dependency digests/stamps + compiler-build
  identity + argv/config. Hit loads `module.bin` via Cranelift
  `define_function_bytes` and skips load/parse/check/TIR lowering/codegen.
- **Trace:** `JET_RUN_TRACE=1` prints store/hit; in-process phase counters
  prove warm parse/check/lower/codegen/link = 0.
- **Signpost:** one stderr tip pointing at `jet dev` when a cold path is slow
  (≥200ms), TTY-only, suppressed by `NO_COLOR` / JSON mode.
- **Budget methodology (CI):** cold/warm medians and p90 over ≥5 samples for
  matched no-op, file, and JSON-text fixtures vs Bash/Python/Node; record
  `os`, `arch`, `cpus`, and hostname. Provisional gate: warm Jet no-op median
  `< 100ms` in-process. Absolute peer-parity ceiling awaits D-SCRIPT-BUDGET1.
  Subprocess Jet remains blocked on HostCall interp coverage (E0956); peers
  are still measured.
