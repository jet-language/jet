# Card #676 — Native Compiler Handoff Crosswalk

Date: 2026-11-26
Status: Completed handoff per owner direction (Option 1 — unfreeze, map all remaining lessons to live native owner cards, retire Rust/cache-only items, and close `#676`).

## Context

Card `#676` (*Video mine: compiler-latency lessons cross-check and gap closure*, source `M9UWgw_aW28`, plus follow-up log notes from `vFp4IbC2aZ0` on 2026-08-17 and `aPL6y2oJjMw` on 2026-08-28) was prepared in August 2026 against the Rust/LLVM-backed compiler. Its earlier crosswalk (`Docs/Audits/compiler-latency-video-crosswalk-2026-08-24.md`) remains preserved untouched for historical provenance.

All six original receiving cards (`#666`, `#669`, `#441`, `#241`, `#126`, `#2859`) are closed and retired, the compiler is now natively self-hosted in Jet (with no Rust/LLVM codegen or rlibs), and compile caches are off by default (`#6966`, cold compile speed target 30M LOC/s).

## Native Handoff Table

| `#676` item / source lesson | Current status in native Jet | Live canonical owner |
| --- | --- | --- |
| `c1` Source ledger (`M9UWgw_aW28`) | Retained as historical mine provenance (`Docs/Audits/compiler-latency-video-crosswalk-2026-08-24.md`). | `#676` (done) |
| `c2` Clean / unchanged / edit compile-latency corpus across phases | Live native cold-compile corpus (`Tools/Perf/CompileThroughput`, `Tools/Perf/Budget/Check`) with per-phase lines/s budgets (`P1`–`P15`). Warm dev-loop latency tracked separately. | `#5479` (compile speed), `#2756` (`jet dev` live reload), `#6759` (cold script latency), `#6966` (caches off by default) |
| `c3` Hostile cache invalidation (`CmdCompile.rs`, `RuntimeCache.rs`, Rust rlibs) | **Superseded** for local builds by `#6966` (caches off by default; every run and build compiles live) and retirement of Rust/LLVM emission. Shared/remote cache invalidation lives on `#6026`. | `#6966` (default live compile), `#6026` (remote/shared build cache) |
| `c4` Construct-scale matrix & superlinear growth detection (generics, variadics, derives, matches, closures, taskgroups, Drop) | Covered by the native linear-scaling gate and generic-sharing work; x64 jump selection linearity (`#5479 c38`) already met. | `#4601` (compiler scaling gate), `#5479 c10` (duplicated generic instances), `#6797` (shared generic O0) |
| `c5` Speed without silent boxing, dynamic dispatch, feature loss, weaker diagnostics, or tier divergence | Standing repository invariant (`I9` and Performance Standard in `AGENTS.md`) plus `#5479 c3` identical-output gate. | `AGENTS.md` (`I9`), `#5479 c3` |
| `c6` Linker, libc, allocator, backend, toolchain, and hardware provenance | Native linker and runtime-pack work tracked per phase; peer-matched tool latency tracked under `#6960`. | `#5479 c26, c30, c32`, `#6960` (tool latency vs peers) |
| `c7` Bounded phase explanation & `aPL6y2oJjMw` sema subphase breakdown | Native compile-speed pipeline breaks sema into `P3 register`, `P4 comptime`/`sema.check`, `P5a+P5b sema.bodies`, `P5c sema.effects`, `P5d sema.failure`, and `P6 finalize`, with all-thread CPU attribution and `jet perf` instruction/cycle profiling. | `#5479 c13–c20, c36, c39, c40` |
| `vFp4IbC2aZ0` log lessons (parallel frontend, fast dev backend, fast linking, low-overhead comptime) | Mapped directly to the native G7 compile-speed pillars and data-oriented core. | `#6793` (data-oriented core), `#6794` (parallel pipeline), `#6797` (shared generic O0), `#6798` (warm comptime child), `#6996` (fused pipeline) |

## Verdict

Every still-applicable lesson from `#676` has an active, concrete canonical owner on the live native compiler board, and every Rust/rlib-cache-specific row is superseded by native self-hosting and `#6966`. No uncovered gap remains orphaned, and no duplicate subsystem is introduced. Measured before/after compile-speed evidence remains with `#5479`, `#4601`, and `#6793`–`#6996` per `#676`'s plan.
