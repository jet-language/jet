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

- Instrument the rustc/link step (`PhaseTiming` now records it) + a
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

## #1027 default profile routing canary

`tests/cli_compiler_speed.rs::compiler_speed_default_profile_routing_is_removal_sensitive_and_production_backed`
is the slice-specific canary for this card. It proves:

- `jet run` and `jet dev` use the fast profile and the default native tier;
- `jet build` uses the optimized default profile.

The test mutates this section and checks the real production routes. Removing
or bypassing this plan section must fail the canary.

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
warmup/sample counts, process CPU-time variance, peak RSS, and phase totals.
The `latency_ns` field is aggregate user plus system CPU time; wall-clock load
and affinity remain receipt identity so host contention stays visible. Clean,
no-change, and representative-edit measurements remain separate.
Missing, partial, stale, or incompatible evidence is unavailable/failure; it
never passes by changing the corpus, hiding a cold run, or increasing warmups.

## #1023 checked corpus and phase-timing canary

`tests/build_entry.rs::compiler_speed_phase_timing_reports_real_release_build`
checks the production release path and its frontend (`cache_key` included) and
backend phase reports.
The corpus and plan canary is
`tests/build_entry.rs::compiler_speed_plan_and_corpus_are_removal_sensitive`.

The corpus includes a representative project witness,
`examples/features/devloop/job_runner.jet`, with a distinct source edit that
changes the selected `greet` job output and its checked golden. Its warm
default-tier edit-to-output is measured by the `jet run run.jet -- greet`
no-change and representative-edit rows; the same source/edit pair is replayed
through `jet dev` by the parity rail. The named-job contract also runs
`seed_data` through default `jet run`, `jet dev --watch=off`, and
`jet run --interpret`, checking its cross-job output; `jet jobs` pins the
schedule and visibility, while release runs `greet` and rejects the stripped
Dev job. A hello-only row cannot establish a speed claim. Removing or
bypassing this section must fail the corpus and plan canary.

`tests/cli_compiler_speed.rs::compiler_speed_named_job_dev_matches_run_and_interpreter`
is the focused named-job differential check. It drives the real CLI for the
default JIT, `jet dev`, and the interpreter, then sends an unknown dev job
through the same path and requires E1294.

The report may establish a measured baseline only for the pinned machine and
toolchain identity. It does not claim the clean/incremental JIT and optimized
AOT budgets until those rows and their variance are present. The same report
must retain the failure rail for cache invalidation, nondeterminism, pathological
inputs, and unstable samples.

The checked-corpus dashboard emits six rows per active corpus row:
`jit-clean`, `jit-no-change`, `jit-representative-edit`,
`aot-release-clean`, `aot-release-no-change`, and
`aot-release-representative-edit`. The current `tools/perf/corpus.tsv` has five active
rows, so the minimum matrix is 30 rows. Each row uses one warmup and twenty
measured samples. Its policy allows at most 15% latency or peak-RSS regression
against the pinned row, an interquartile spread of 100% or less, and at most
five Tukey-fence outliers. The report is schema version 4. The top-level
receipt must report verified semantic, diagnostic, effect, and tier parity with
its case count. Output, phase, and workload identity records must remain exact.
Each workload identity includes the content digest of `tools/perf/package.jet`
and the canonical machine toolchain digest. `ci-perf-check.sh` derives the
required row count from the report corpus count and rejects duplicate or
missing program/state identities or changed manifest/toolchain identity.
The active corpus rows are ordered `examples/features/basics/hello.jet`,
`examples/features/collections/wordcount.jet`, `examples/features/serde/json.jet`,
`examples/features/basics/pattern_matching.jet`, and
`examples/features/devloop/job_runner.jet`; the representative job is fifth.

The focused checker canary is `bash tools/perf/test-ci-perf-check.sh`. It uses
temporary synthetic reports, so it proves rejection behavior without changing
the production baseline or running the live measurement, and pins the exact
five-row corpus order.

## #1025 cache removal canary

`tests/cli_compiler_speed.rs::production_build_reuses_and_repairs_stdlib_objects`
is the production removal canary for this slice. It proves that:

- an unchanged build restores its final binary from BuildCache;
- a corrupted final binary is rejected by its digest and rebuilt;
- a program edit reuses the runtime/Core objects; and
- a corrupted runtime object is rebuilt before link.

The test also checks that the native cache log exposes the relevant runtime and
Core digests. Removing or bypassing this plan section must fail the canary.

## #1026 incremental batch sema canary

`crates/jet-driver/src/QueryService.rs::tests::batch_disk_interface_change_keeps_unrelated_module_warm`
is the production removal canary for the batch sema slice. It proves that a
changed module interface rechecks that module and its importer, while an
unrelated module remains a cache hit. Removing the batch cache handoff or
dependent-only invalidation must fail the check.

The same production batch uses the bounded staged frontend proved by
`crates/jet-driver/src/Loader.rs::stale_manifest_name_tests::staged_frontend_is_bounded_and_deterministic`:
source preparation uses at most eight workers, and the loader consumes staged
results serially in stable module order. Removing or bypassing that bounded
frontend path must fail its focused check; this card owns both proofs.

## #2346 cache-input identity

`tools/perf/dashboard.sh` hashes the `tools/perf/package.jet` bytes once
and carries that `manifest_sha256` through each matrix row, the matrix and
environment receipts, and the construct-scale input digest. Its workload
digest also includes the canonical machine `toolchain_sha256`. The CI checker
compares both the top-level manifest identity and the per-row manifest and
toolchain identities, so a manifest or toolchain edit cannot reuse an older
receipt.

The dashboard copies the pinned manifest once into the run fixture, then uses
that same fixture and shared cache for the AOT no-change warmup and samples.
`check_cache_state` still requires a no-change hit with zero misses and an edit
miss with a rebuild; no workload-specific cache bypass replaces those checks.

`tools/perf/test-ci-perf-check.sh` checks that either identity changes the
workload digest and that a changed manifest is rejected against a matching
baseline. The report remains version 4; `tools/perf/baseline.json` remains the
committed version-2 baseline and is not regenerated without the integrated
measurement pass.

## #2345 production receipt rows and edit parity

`tools/perf/dashboard.sh` publishes one canonical tab-separated row format for
the production table: the exact header is `program`, `state`, `stage`,
`latency_ns`, `memory_bytes`, `variance_pct`, `output_sha256:stderr_sha256`,
and `phases`. `tools/perf/ci-perf-check.sh` validates that header and every row
has exactly eight fields before applying the v4 identity, cache, parity, and
anti-cheat checks. `tools/perf/test-ci-perf-check.sh` feeds those exact
production-formatted rows and rejects the old whitespace form.

The edit parity rail uses the same state key as `measure_state`: the base
corpus program plus `jit-representative-edit`. The edit source path remains the
execution fixture, not the receipt key. The focused canary asserts this mapping
so an edit receipt cannot disappear because its lookup uses the edited path.

## #666 criterion evidence and removal checks

This is the exact evidence map for #666. The state column records what is
available in the current tree; an unrun command is not evidence.

| Criterion | Current evidence | State |
| --- | --- | --- |
| 1 | `tools/perf/dashboard.sh` functions `run_parity_checks`, `measure_state`, and `workload_digest`; `tests/dev_default_parity.rs::dev_default_matches_compiled_binary`; `tests/cli_compiler_speed.rs::compiler_speed_named_job_dev_matches_run_and_interpreter`; `tests/build_entry.rs::compiler_speed_phase_timing_reports_real_release_build` | Production rails and content identity are present; the one-corpus production rehearsal reached optimized AOT but hit the 120-second workload guard under concurrent shared builds, so no fresh differential receipt exists. |
| 2 | `tools/perf/corpus.tsv`; the six dashboard row calls; `tools/perf/ci-perf-check.sh`; `tools/perf/test-ci-perf-check.sh`; `tools/perf/baseline.json` | No measured baseline is generated in this lane. The current five-row corpus needs 30 rows; the committed baseline remains version 2 by design. |
| 3 | `tools/perf/dashboard.sh` functions `check_corpus`, `check_cache_state`, `variance_file`, and `outlier_file`; `tests/cli_compiler_speed.rs::production_build_reuses_and_repairs_stdlib_objects`; `tools/perf/test-ci-perf-check.sh` | Hit/miss and identity failure rails remain present; live AOT no-change proof was not reached because the clean AOT rehearsal hit the 120-second guard. |
| 4 | `tests/cli_compiler_speed.rs::compiler_speed_named_job_dev_matches_run_and_interpreter`; `tests/cli_compiler_speed.rs::production_build_follows_compiler_speed_plan_flags_and_linker`; `tests/cli_compiler_speed.rs::production_build_reports_missing_explicit_linker_as_tool_error`; `tests/build_entry.rs::compiler_speed_phase_timing_reports_real_release_build`; `tests/dev_default_parity.rs::dev_default_matches_compiled_binary` | Targeted production checks exist; the shell checker and direct JIT/dev/interpreter named-job smoke ran, while the Rust checks remain unrun in this lane. |
| 5 | `docs/spec/syntax-decisions.md`; `docs/spec/architecture.md`; `docs/spec/spec.md`; this plan; `tools/perf/corpus.tsv`; expected goldens; `tools/perf/baseline.json`; `docs/reference/compiler-speed-environment-2026-08-25.json` | Open. The committed baseline and environment receipt still predate the current five-row corpus and six-row state matrix; no production regeneration is performed in this lane. |
| 6 | `tests/cli_compiler_speed.rs::compiler_speed_closeout_is_backed_by_plan` | The removal canary is exact and removal-sensitive after this update; it is unrun in this lane. |

The #666 removal canary requires this heading, the current production evidence
names above, the six dashboard row names, the 30-row minimum, one warmup and
twenty measured samples, the 15% regression limit, the interquartile spread
limit, and the five-outlier limit. It mutates this heading to `(bypassed)` and
must fail. The #1025 cache canary is separate and does not satisfy #666.

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
