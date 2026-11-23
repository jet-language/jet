# Adversarial Design Review: Parallel Streaming Compiler Pipeline (`#6794 c1`)

**Date:** `2026-11-23`  
**Design reviewed:** `Docs/Audits/2026-11-23-parallel-pipeline-design.md` (Rev 2, base `fb1cb69a62`)  
**Card:** `#6794` (`c1`–`c3`) · **Authority:** `AGENTS.md` ("Adversarial design review", "Invariants" I1/I8/I9, "Dogfood the debugger and profiler"), `#5479` (`c1`, `c6`, `c7`, 2026-10-23 GENERAL-first order), `#6793`  
**Review passes:** Pass 1 failed the Rev 1 design on 6 concrete defects (`F1`–`F6`); Pass 2 independently verified every revision in Rev 2 against live repository source and Tower records (`Tools/Tower/.tower/tower.json`).

---

## Verdict: **PASS — Holds Within Stated Limits**

Rev 2 resolves all six blocking findings from Pass 1 (`F1`–`F6`). Every cited file path, line number, hard thread cap, allocator lock word, and semantic convergence boundary matches live repository code on `fb1cb69a62`. The design is sound to build in S1→S5 order, **subject to the five explicit limits and gates recorded in §3 below** (none of which may be silently bypassed).

---

## 1. Strongest Existing Solutions — Fair Best Case & What Jet Gives Up

| Peer & Source | Strongest Case | What It Does That This Design Does Not |
|---|---|---|
| **Zig** (`src/Compilation.zig`, `src/Zcu.zig`, `src/Sema.zig`, `src/link/Elf.zig`) | Per-declaration (`AnalUnit`) dependency DAG, sharded `InternPool`, incremental in-place ELF patching with function padding. | Zig streams individual declarations straight to ELF without whole-program failure-union inference (`sema.failure`), whole-program effect solving (`sema.effects`), constant-time taint branch masking (`sema_mark_masked_branches`), or `SemaGraphTrail` rollback (`core_stand_in_rerun`). Jet keeps a compact Convergence Checkpoint between Tier 1 and Tier 2 to preserve those semantics. |
| **Go** (`src/cmd/compile/internal/gc/compile.go`, `src/cmd/go/internal/work/exec.go`) | Acyclic package graph with pre-summarized export data; per-function backend pool (`compileFunctions`) over independent SSA functions with deterministic object slots. | Go relies on a tracing GC for zero-RC shared reads of immutable AST/SSA across goroutines, and forbids cyclic package imports outright. Jet handles import cycles via SCC grouping and relies on lexical `task.group` read borrows (and a future `Frozen<T>` ballot) plus `#6793` zero-RC flat columns. |
| **rustc + rayon** (`compiler/rustc_query_system/src/query/plumbing.rs`, `rayon-core/src/job.rs`; Chase & Lev, SPAA 2005) | Demand-driven query engine with cycle detection (`ActiveJob`), Chase-Lev work-stealing, sharded interners, zero-lock `&T: Sync` cross-thread reads. | Rust's `&T: Sync` permits arbitrary-lifetime read-only sharing without mutexes or unsafe code. Jet's lexical `task.group` read borrow (`TaskGroups.jet:648–655`) is scoped to a lexical group block; persistent cross-phase read sharing waits on an owner ballot (`Frozen<T>`). |
| **Odin / Jai** (Odin `src/checker.cpp`; Jai data-oriented pipeline) | Per-thread linear bump arenas for flat POD AST/IR with bulk unmap and zero reference counting. | Neither enforces `I1` leak-freedom via RC on compiler structs nor runs iterative `CoreDemand` / `comptime.bodies` / `core_stand_in_rerun` loops. Jet therefore gates bulk `munmap` strictly behind `#6793 c3–c4`'s zero-RC flat scalar columns. |

---

## 2. Claim-by-Claim Verification Against Live Code

| Claim / Area | Live Source & Measurement Verification | Pass 2 Verdict |
|---|---|---|
| **§1 `16t` vs `32t` scaling collapse & hard thread caps** | Verified live caps in source: `Compiler/JetIR/Source/BuildWaves.jet:120` (`IR_BUILD_MAX_PARTS: 12`), `Compiler/JetSema/Source/Sema/CheckProgram.jet:13945` (`SEMA_FINALIZE_SIDE_EFFECT_PARTS: 16`), `CheckProgram.jet:13721` (taint walk capped at `threads / 4`), `Compiler/JetDriver/Source/Driver/Loader.jet:5684` (`compiler_threads_single()`), `Compiler/JetCLI/Source/CLI/Request.jet:254` (runtime + Core both take full `cap` → 2× oversubscription), `Compiler/Toolchain/RuntimePack.jet:621` (no `--threads` forwarded to `jet-bare doc unit.jet`), `Compiler/JetCLI/Source/CLI/BuildExecute.jet:293–295` (at most 1 `ready_target` per round). `384` `Shared` cells verified exact in `Tests/compiler_data.tsv`. Live `16t→32t` `CompileThroughput` baseline confirms zero scaling >16t (`check` `13.66s → 13.65s`; `O0` `32.76s / 87.19s CPU → 32.09s / 111.74s CPU`; `ir.build` `1.74s → 1.93s`; `ir.object` `0.72s → 0.84s`). | **Holds** |
| **§1 Honest `400K LOC/s` arithmetic** | `1,089,102 / 400,000 = 2.72s` total cold `jet build` wall. Explicitly credits `#6793` (`<= 5 µs/line` CPU) + `#6794` (`>= 24×` parallelism across full build including `runtime`), matching `#6794`'s card body (`Tools/Tower/.tower/tower.json:52546`). | **Holds** |
| **§3.1 GENERAL (`I8`) work-stealing & allocator sharding (S1)** | Eliminated the uncompilable Rev 1 `JetFoundation/.../WorkQueue.jet` (`JetFoundation/package.jet:1–4` has no dep on `JetBackend` and no `policy.memory` kernel). Work-stealing moves into `Compiler/JetRuntime/Source/Scheduler.jet` (GENERAL, I8); compiler-side queues use safe `Atomic<Int>` (`Compiler/JetFoundation/Source/Registry/BuiltinMethods.jet:311–318`, already proven in `CheckProgram.jet:13858` and `BuildWaves.jet:78–86`, `0` new `#Unsafe`). Allocator contention targets actual `Compiler/JetRuntime/Source/Alloc.jet:8–65` locks (`word 216` large-block cache, `word 736` slab-region lock) rather than the already-sharded 32 small-block arenas / TCB caches. | **Holds within stated limits** *(see Limit L1)* |
| **§3.1 Cross-thread read sharing & bulk arenas** | Removed unratified raw-pointer `ImmSlice`. Phase-scoped reads use shipped lexical `task.group` read borrows (`Compiler/JetSema/Source/Sema/Calls/TaskGroups.jet:648–655, 720–757`, proven in `CheckProgram.jet:13877–13889, 14207–14231`); persistent cross-phase sharing is routed to a `Frozen<T>` owner ballot (`#6793` 2026-11-14 gap); bulk `munmap` is gated behind `#6793 c3–c4` flat scalar columns (`I1` leak-free). | **Holds within stated limits** *(see Limit L2)* |
| **§3.2 Two-Tier Streaming Pipeline (S3–S4)** | Correctly preserves all 8 live whole-program invariants: (1) root `#loop` two-pass reload (`Pipeline.jet:5722–5741`), (2) Header Barrier for canonical sorted `ModuleID` after `use_driven` discovery + `sema_output_resolve` (`Outputs.jet:601–651`) + `sema_core_roles_check` (`CoreRoles.jet:131–173`) + `sema_trait_laws_add` (`TraitLaws.jet:68–118`) + `sema_registration_blocking_error` (`Pipeline.jet:6143`), (3) `comptime.bodies` (`BodyComptime.jet:262–400`) / `comptime.validate` / `core_stand_in_rerun` (`Pipeline.jet:6305–6323`) with `SemaGraphTrail` intact, (4) `sema.failure` + `sema.recheck` (`CheckProgram.jet:13173–13221`), (5) `sema.effects` + `sema_mark_masked_branches` (`CheckProgram.jet:13368, 13461–13463`), (6) `dead_code: remove` (`Pipeline.jet:6867–6905`) + `single_thread` (`Host.jet:1915`) + `#Inline(Always)` O0 body index (`Host.jet:1982–1994`), (7) `(pass_id, module_id, ordinal)` diagnostic tagging + canonical dedupe (`CheckProgram.jet:13257–13262, 13460, 13624–13632`), (8) relocatable per-function emission (`X64Module.jet:127–199`) + `ir_x64_hot_order` (`Host.jet:2068`). | **Holds within stated limits** *(see Limit L3)* |
| **§3.3 Barrier #6 (`build.step` / `runtime`: `16.3s` = `51%` wall) (S2)** | Verified `Compiler/build.jet:101–132`, `Toolchain/RuntimePack.jet:621`, `CLI/Request.jet:249–314`, `CLI/CoreLibrary.jet:116–160`, `CLI/BuildExecute.jet:293–295`. Forwarding `--threads`, unifying `JetRuntime` + `Core` thread budgets, parallelizing ready targets, and lifting `IR_BUILD_MAX_PARTS`/`SEMA_FINALIZE_SIDE_EFFECT_PARTS` directly attack the measured `16.21s` bottleneck without breaking self-hosting (`runtime` still executes `jet-bare`). | **Holds within stated limits** *(see Limit L4)* |
| **§3.4 Parallel Warm Comptime Pool (S5)** | Fixed paths to `Compiler/JetBackend/Source/Image/SessionHost.jet` and `Compiler/JetEval/Source/IR/Native.jet`. Correctly identifies both real locks (`sealed: Shared<JetEvalNativeChild>` at `Native.jet:116`, `image: Shared<JetEvalNativeImage>` at `Native.jet:115, 286–326`), deterministic `(ModuleID, item_idx)` output/trace/input-log ordering, lazy spawning (`K` only when `>= 2` independent DAG items are ready), and explicitly gates S5 implementation on a runnable DAG-width probe. | **Holds within stated limits** *(see Limit L5)* |

---

## 3. Attacks Still Live (Stated Limits & Non-Negotiable Gates)

Each limit below is binding on S1–S5 implementation:

- **L1 — `Scheduler.jet` quiescence & shutdown invariants (S1):** `Compiler/JetRuntime/Source/Scheduler.jet:2237–2520` guards carrier retirement, `scheduler_idle`, `jet_scheduler_drain`, and `jet_rt_scheduler_shutdown` (including `live == settled280` and `queued` at `root+256`) under `sync_thread_lock(root)`. Per-carrier Chase-Lev deques MUST maintain exact lock-free/atomic queued+live accounting so `jet_rt_scheduler_shutdown` (required for userland exec in `Image/Loader.jet`) and fiber teardown tests (`#6772 c13`) never observe false idle or hang.
- **L2 — No silent `Frozen<T>` bypass or premature `munmap` (S1/S3/S4):**
  - If S3 requires persistent read-only sharing across phases that lexical `task.group` borrows (`TaskGroups.jet:648–655`) cannot express, an owner ballot for `Frozen<T>` MUST be ratified first (`AGENTS.md:314–319`, `#6793` 2026-11-14 log). No `#Unsafe` raw-pointer read shim may be added outside a declared memory kernel (`UnsafeRatchet` stays at `0` outside kernel, `0` net `#Unsafe` growth).
  - Bulk `munmap` of `ModuleArena[m]` may ONLY be enabled for types proven to hold zero RC/heap children (`#6793 c3–c4`), verified by `I1` leak tests.
- **L3 — Tier-2 streaming scope across optimization tiers (`I9`, S4):** Direct `ir_build(fn_id) → x64_patch(fn_id)` without a whole-program IR merge applies to `-O0` (`fast_o0 = true`, `Host.jet:1918`). `-O1` and `-O2` keep module-level optimization (`ir_optimize_module`, cross-function inlining) and IR Lint (`Host.jet:1964–1979`), with identical observable output across `O0`/`O1`/`O2` (`I9`) and `S2 == S3` SHA-256 fixed point.
- **L4 — Never weaken `jet_core.o` optimization to game `CompileThroughput -O0` (S2):** §3.3 item 3 mentions `CoreLibrary.jet` compiling Core at `-O2` inside `jet build -O0 Compiler`. Per `AGENTS.md:272` (*"Never trade semantics, diagnostics, determinism, safety, or I9 parity for a score"*), `jet_core.o` (which `jet run` binds to) MUST NOT be downgraded to `-O0` just to lower `CompileThroughput -O0` wall time; reduce `runtime.core` wall via S1/S2 32-thread scaling, eliminating 2× oversubscription, and `#6793` CPU reduction (or obtain an explicit owner ruling if toolchain dev builds ever separate Core opt level).
- **L5 — S5 Comptime Pool gated on runnable DAG-width probe (`AGENTS.md:443–444`):** Before writing multi-child code in `SessionHost.jet` / `Native.jet`, run and record the DAG-width probe on `Compiler/JetCLI/run.jet` and `Core` proving (a) `>= 2` independent ready `#run` items per wave exist in sufficient volume, and (b) `K` children yield a net wall-time win after child fork/exec/seccomp/SHM setup cost. If the probe shows `comptime` wall is dominated by sequential `comptime.apply` (`21,252` rows) or `comptime.land.splice` (`1.28s`) rather than child execution, re-rank S5 work onto those bottlenecks instead (`AGENTS.md` "Re-rank on evidence").

---

## 4. Open Follow-Ups & Tracking

| Item | Tracking Home |
|---|---|
| Record §4 S1–S5 slice plan on card `#6794` (`plan` field) | Tower `#6794` (`c1`) |
| Safe immutable cross-phase sharing (`Frozen<T>`) if S3 needs it past lexical `task.group` | New Tower ballot linked from `#6793` / `#6794` |
| Giant single-file parse tail (`Statements.jet`, `Math.jet`) | Tower `#6796` (existing) |
| Zero-RC flat AST/IR columns & `<= 5 µs/line` CPU floor | Tower `#6793` (existing) |
| Any missing lock-contention counter in `jet perf` encountered during S1/S5 | Dogfooding card (`AGENTS.md:715–732`) |
