# Compiler self-speed: beating Rust compile times

Goal: `jet` compiles much faster than `cargo`/`rustc`. Plan of record, 2026-07-16.

## Where the time goes today

Jet's front end (lex → parse → sema → codegen) is milliseconds. 90%+ of
`jet run`/`jet build` wall-time is the shelled-out rustc — and today the
default profile hands rustc **optimized** flags.

- **Default profile is optimized.** `Source/main.rs:223-330`: `BuildProfile::Default`
  = opt-level 2 + thin-LTO + strip on every everyday build. A `--profile=debug`
  (opt-level 0) exists but is opt-in.
- **rustc runs un-tuned.** `Source/CmdCompile.rs::build`: no codegen-units, no
  incremental, no fast linker (mold/lld), no target-cpu.
- **Stdlib recompiles every build.** The prelude is emitted as source text into
  the generated program (`crates/jet-codegen/src/Prelude/`); no precompiled
  rlib/metadata artifact exists.
- **Cache is all-or-nothing.** `Source/BuildCache.rs` keys
  sha256(generated rust + profile) → whole binary; any edit = full rustc rebuild.
- **rustc time is unmeasured.** `PhaseTiming` (`JET_TIMING=1`) laps
  load/sema/ffi/codegen but never the rustc/link step that dominates.
- **Front end is serial**; `jet-queries` demand cache is wired into the LSP only,
  not the batch compile path.

## Why Jet can structurally win

Sema owns all checking (R1/R2, I3): every type, ownership fact, and mangle is
resolved in TIR before emission. rustc's borrow-check, trait solving, and
inference on the generated code are 100% redundant verification (R5/I2 keeps it
as a hidden verifier). A native backend drops TIR straight to object code and
deletes that whole phase; short-run, we stop asking rustc to optimize when we
only want to run.

## Stages

**Stage 0 — instrument (un-gated, first).** Lap rustc + link in PhaseTiming.
Add a compiler self-speed benchmark corpus (hello / 1k / 10k / 50k LOC, single-
and multi-module, clean + incremental) measured against equivalent cargo builds
in CI. Exit: 100% of wall-time attributed; medians recorded. (The perf.<role>
budget system measures user programs, not compiler self-speed — reuse its
vocabulary only.)

**Stage 1 — dev-loop dominance.**
- 1a *(owner-gate D-BUILD-DEFAULT ballot)*: default `jet run`/`jet dev`/`jet build`
  → opt-level 0, codegen-units 256, no LTO; optimized moves behind `--release`.
  Exit: hello clean build ≤150 ms; default build beats `cargo build` (debug) on
  every corpus entry.
- 1b *(un-gated)*: prefer mold → lld → system linker; tune rustc flags.
  Exit: ≥2× link-time cut where available.
- 1c *(un-gated)*: precompiled stdlib object store keyed on
  (prelude, rustc identity, profile) under ~/.cache/jet; user code `--extern`s
  it. Constraint R10 pay-for-what-you-call. Exit: prelude cost ~0 warm.
- 1d *(un-gated)*: promote jet-queries onto the batch path — per-module
  memoized lex/parse/check, invalidate dependents only. Constraint R12 parity +
  identical diagnostics to clean check. Exit: incremental re-check ≤20 ms @10k LOC.
- 1e *(un-gated)*: hand-rolled (I6) scoped-thread parallel per-module front end;
  deterministic diagnostic order. Exit: front end scales with cores.
- 1f: widen JIT TIR coverage so more `jet dev` reloads never touch rustc.

**Stage 2 — AOT dominance.**
- 2a *(owner-gate)*: Cranelift AOT debug tier — `jet build` (debug) lowers
  TIR → Cranelift → object → link; rustc leaves the debug path. Falls back to
  rustc by named unsupported reason (I2). R12: third TIR consumer, full parity
  proof. Exit: debug builds ≥5× faster than cargo debug; zero golden diffs.
- 2b *(owner-gate, post-2a)*: native optimizing release path; until it lands,
  rustc stays the release backend.

## Honest physics

"Much faster" means compile time. Debug/dev loop: 5–20× is realistic (strictly
less work). Release while rustc is the optimizer: parity at best — same LLVM
plus a transpile hop; claiming otherwise is dishonest. The generated-Rust
round-trip has a floor (rustc re-parses everything); only 2a/2b remove it.

## Gates queued

Ballots: default-profile flip (1a) and Cranelift AOT debug tier (2a) — both on
the plan's Tower card. mold/lld are subprocess tools like rustc, not linked
crates (I6-clean). Cranelift is already I6-ratified for the JIT (D-JITDEP1);
AOT is a new scope, hence the 2a ballot.
