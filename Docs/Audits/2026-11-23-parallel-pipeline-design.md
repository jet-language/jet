# Parallel Streaming Compiler Pipeline — Design Note (`#6794`, Rev 2)

**Date:** `2026-11-23` · **Base commit:** `fb1cb69a62` · **Card:** `#6794` (`c1`–`c3`) · **Revised to address all findings (`F1`–`F6`) of Adversarial Review 1 (`4febc8ba`)**

## 1. Problem & Measured `16t` vs `32t` Scaling Baseline (`fb1cb69a62`)

Live cold, uncached `CompileThroughput` runs (`1,089,102` lines, `--modes check,O0 --runs 1 --warmups 0`) on `fb1cb69a62` prove why the compiler currently stalls far short of `#6794 c3` (`>= 24×` average parallelism on 32 threads, `32t < 16t` wall, `>= 400K LOC/s`):

| Mode & Threads | Wall | Process CPU | Avg Parallelism | Throughput | Peak MiB | Scaling (`32t` vs `16t`) |
|---|---:|---:|---:|---:|---:|---|
| `check` (`16t`) | `13.66s` | `122.66s` (`112.6 µs/L`) | `9.0×` | `79,708 LOC/s` | `3,356` | Baseline |
| `check` (`32t`) | `13.65s` | `121.94s` (`112.0 µs/L`) | **`8.9×`** | `79,772 LOC/s` | `3,346` | **`1.00×` (flat — zero scaling >16t)** |
| `O0` (`16t`) | `32.76s` | `87.19s` (`80.1 µs/L`) | `2.7×` | `33,250 LOC/s` | `6,104` | Baseline |
| `O0` (`32t`) | `32.09s` | **`111.74s`** (`102.6 µs/L`) | **`3.5×`** | `33,938 LOC/s` | `5,940` | **`+24.55s` (+28%) contention CPU for only `0.67s` wall** |

### Live Phase Barriers, Hard Thread Caps & Measured `16t → 32t` Regression Table
From reproducible `JET=<s2-jet> <s2-jet> run Tools/Perf/CompileThroughput/Throughput.jet -- --modes check,O0 --runs 1 --warmups 0 --threads 16|32` traces and live source:

| # | Phase / Barrier | Live Source Location | `16t` Wall (CPU) | `32t` Wall (CPU) | Root Cause of Scaling Collapse |
|---:|---|---|---:|---:|---|
| 1 | `build.step` (`runtime` + `core-embed`) | `Compiler/build.jet:101–132`, `CLI/BuildExecute.jet:293–295`, `Toolchain/RuntimePack.jet:621`, `CLI/Request.jet:249–314` | `16.89s` (`runtime` `16.21s`: `runtime.core` `12.96s`, `core.object` `1.56s`, `core.interface` `1.35s`, `pack` `0.93s`, `web` `0.81s`) | `16.37s` (`runtime` `15.66s`: `runtime.core` `12.56s`) | **`51%` of total wall!** `runtime` executes `jet-bare`, does not forward `--threads` at `RuntimePack.jet:621`, `Request.jet:254` spawns `JetRuntime` (`-O1`) + `Core` (`-O2`) at full `cap` each (`2×` oversubscription), and `BuildExecute.jet:293` starts at most 1 `ready_target` per round. |
| 2 | `load` (parse + reach) | `Driver/Pipeline.jet:5712–5737`, `Driver/Loader.jet:5684` | `1.09s` (`6.00s` CPU) | `1.07s` (`7.80s` CPU) | Whole-graph parse barrier + giant single files serialized (`#6796`) + `Loader.jet:5684` passes `compiler_threads_single()`. |
| 3 | `register` + post-register mutations | `Driver/Pipeline.jet:6056–6147` | `0.81s` (`4.31s` CPU) | **`0.83s` (`7.44s` CPU)** | Slower at 32t; monolithic `registration_graph` + global `sema_output_resolve`/`sema_core_roles_check`/`sema_trait_laws_add`. |
| 4 | `sema.check` + `comptime` (`7` compiles across build) | `Driver/Pipeline.jet:6148–6204`, `JetEval/Source/IR/Native.jet:114–118, 286–326` | `2.59s` parent (`9.11s` across build; `25` prefetch/precheck waves, `21,252` `comptime.apply`, `1.28s` `land.splice`) | `2.73s` parent (`8.98s` across build) | Whole-graph comptime barrier + single `sealed: Shared<JetEvalNativeChild>` + global `image: Shared<JetEvalNativeImage>` bind lock. |
| 5 | `sema.finalize` (`bodies`, `failure`, `recheck`, `effects`, `taint`, `comptime.bodies`, `comptime.validate`) | `Driver/Pipeline.jet:6249–6463`, `JetSema/Source/Sema/CheckProgram.jet:13173–13945` | `3.81s` (`32.89s` CPU) | `3.58s` (**`45.86s` CPU**) | `+13.0s` wasted CPU at 32t from `96` `Shared` cells (`Tests/compiler_data.tsv`), `Alloc.jet` large-block lock (`word 216`) + slab lock (`word 736`) + cross-thread RC drops; hard caps `SEMA_FINALIZE_SIDE_EFFECT_PARTS = 16` (`CheckProgram.jet:13945`) and taint `threads / 4` (`13721`). |
| 6 | `ir.build` → `ir.opt` → `ir.x64` → `ir.object` → `native.link` | `CLI/Host.jet:1791–2161`, `JetIR/Source/BuildWaves.jet:120`, `X64Module.jet`, `Object.jet` | `ir.build 1.74s`, `ir.object 0.72s`, `ir.x64 0.43s`, `link 0.35s` | **`ir.build 1.93s`**, **`ir.object 0.84s`**, `ir.x64 0.43s`, `link 0.38s` | **SLOWER at 32t!** Hard cap **`IR_BUILD_MAX_PARTS: 12`** (`BuildWaves.jet:120`), `24` `Shared` cells in `BuildWaves.jet`, `13` in `X64Module.jet`, `12` in `Object.jet`, and whole-program merge barrier before `ir.x64`. |

### Honest Arithmetic for `#6794 c3` (`>= 24×` Parallelism, `>= 400K LOC/s`)
Reaching `400,000 LOC/s` on `1,089,102` lines requires total wall time **`<= 2.72s`**. As `#6794`'s card body explicitly notes, parallelism alone gives at most `~5×` wall reduction with CPU/line unchanged; hitting `<= 2.72s` requires **both**:
1. `#6793` reducing total CPU/line from `~80–112 µs/line` down to **`<= 5 µs/line`** (`<= 5.4s` total CPU across `1.09M` lines), **and**
2. `#6794` raising average parallelism across the ENTIRE `#5479 c1` build (including `runtime` / `CoreLibrary`!) from `2.7–3.5×` to **`>= 24×`** (`5.4s / 24 ≈ 0.23s` + fixed startup/link tail -> well under `2.72s`)!

## 2. Strongest Existing Implementations (Primary Sources & Trade-Offs)

| System & Primary Source | Best-Case Mechanism | What It Does That Jet Cannot Copy Naively (Trade-Off) |
|---|---|---|
| **Zig** (`src/Compilation.zig`, `src/Zcu.zig`, `src/Sema.zig`, `src/link/Elf.zig` — ziglang/zig) | Per-declaration (`AnalUnit`) dependency DAG + sharded `InternPool` + concurrent codegen writing into pre-allocated ELF virtual blocks. | Zig has no whole-program inter-procedural failure-union inference (`sema.failure`), no whole-program effect solver (`sema.effects`), no constant-time taint branch masking (`sema_mark_masked_branches`), and no `SemaRegistrationGraph` rollback trail (`core_stand_in_rerun`). |
| **Go** (`src/cmd/compile/internal/gc/compile.go`, `src/cmd/go/internal/work/exec.go` — golang/go) | Acyclic package graph with compiled export data + lock-free/channel worker pool (`compileFunctions`) over independent SSA functions with fixed object indices. | Go runs backend concurrency **after** whole-package typechecking completes (never streaming mid-typecheck before ABI conventions settle) and uses a tracing GC so shared read-only SSA/types carry zero RC atomic traffic. |
| **rustc + rayon** (`compiler/rustc_query_system/src/query/plumbing.rs`, `rayon-core/src/job.rs` — Chase & Lev, SPAA 2005) | Demand-driven query engine with cycle detection + Chase-Lev work-stealing deque + sharded `DashMap` interners + zero-lock `&T: Sync` shared reads. | Rust's `&T: Sync` borrow checker gives safe zero-lock cross-thread reads at arbitrary lifetimes; Jet currently only permits lexical `task.group` read borrows (`TaskGroups.jet:648–655`) and requires either flat POD data + ratified `Frozen<T>` ballot or phase-scoped lexical `task.group` borrows. |
| **Odin / Jai** (Odin `src/checker.cpp`; Jai data-oriented compiler lectures) | Per-thread linear arenas for flat POD AST/IR (zero RC, bulk `munmap` free) + dependency-driven check/emit queues. | Neither enforces Jet's `I1` memory-safety + leak-freedom invariant via RC on compiler structs nor runs iterative `CoreDemand` / `comptime.bodies` / `core_stand_in_rerun` passes. |

## 3. Target Architecture (`c1`–`c3`) — Tagged GENERAL vs COMPILER-ONLY per `#5479` (2026-10-23)

### 3.1 GENERAL (`I8`): Wait-Free / Lock-Free Work-Stealing in `JetRuntime` + Zero-Contention Allocator
Instead of inventing a compiler-only deque in `JetFoundation` (which would violate `JetFoundation`'s zero-dependency package DAG, `UnsafeRatchet`, and `I8` one mechanism):
1. **Upgrade `Compiler/JetRuntime/Source/Scheduler.jet` (GENERAL — benefits ALL Jet programs, I8!):**
   - Replace centralized task-dispatch lock contention on 16–32 carriers with per-carrier Chase-Lev work-stealing deques inside the audited runtime (`Scheduler.jet` + `Sync.jet`, zero new `#Unsafe` outside the existing runtime kernel, using existing `os_cas_i64` / `os_fetch_add_i64`).
   - For compiler-internal work-stealing lists outside `Scheduler.jet`, use **only safe built-in `Atomic<Int>`** (`BuiltinMethods.jet:311–318`: `load`, `observe`, `store`, `publish`, `add`, `compare_exchange`, already proven in `CheckProgram.jet:13858` and `BuildWaves.jet:78–86`) — **0 `#Unsafe` regions, 0 package-DAG violations!**
2. **Eliminate real `Alloc.jet` contention at 32 threads (GENERAL):**
   - `Compiler/JetRuntime/Source/Alloc.jet:8–65` ALREADY has 32 sharded arenas and lock-free per-thread TCB slab caches (`TCB @320`). The real 32-thread bottlenecks are (a) the global large-block cache lock (`Alloc.jet:11`, word `216`, `> 4096 B` `List`/`Map` buffers), (b) the slab-region lock (`Alloc.jet:12`, word `736`), and (c) cross-thread atomic RC decrements.
   - Shard the large-block cache and slab-region free lists across arenas (`Alloc.jet`), cutting 32-thread contention for ALL Jet programs.
3. **Legal cross-thread sharing & per-thread arenas (respecting `I1`, `UnsafeRatchet`, and `#6793`):**
   - **Today (while AST/IR still hold RC structs):** Use Jet's **already-shipped lexical `task.group` read borrows** (`TaskGroups.jet:648–655, 720–757`, zero-copy, zero `Shared` mutex!) for every read-only parallel phase, AND eliminate the `384` `Shared` mutex cells in `CheckProgram.jet` (`96`), `BuildWaves.jet` (`24`), `Opt.jet` (`23`), `Loader.jet` (`22`), `X64Module.jet` (`13`), `Object.jet` (`12`) using `Atomic<Int>` work claims + pre-indexed per-slot output arrays.
   - **For persistent streaming across phases (`#6793` 2026-11-14 gap):** File a short owner ballot for a safe, general **`Frozen<T>`** (immutable shared read handle with zero mutexes and checked immutability) — NEVER use raw-pointer `ImmSlice` hacks over RC-bearing structs!
   - **Per-thread bump arenas (`ModuleArena[m]`):** Bulk `munmap` reclamation is **strictly gated behind `#6793 c3–c4`** (zero-RC flat scalar columns in `Core/Mem` regions), so no heap-owning RC child block can ever leak (`I1` preserved 100%).

### 3.2 Semantically Sound Two-Tier Streaming Pipeline (COMPILER-ONLY)
Rather than naively lowering modules before whole-program ABI/effect/taint invariants settle (which would miscompile), partition the driver into **two streamable tiers separated by compact, lock-free convergence checkpoints**:

```
TIER 1 — Streaming Frontend & Checking (per-module/per-SCC DAG on work-stealing pool):
  [Root #loop probe check: only when root `#loop` awaits `#run` (Pipeline.jet:5722–5741)]
  FileRead(m) ──▶ Parse(m) [intra-file chunks #6796]
              ──▶ DiscoverImports(m) [queues newly reached modules + CoreReach]
              ──▶ HeaderRegister(m) [per-module header shard; SCCs grouped for cyclic imports]
              ──▶ [Lightweight Header Barrier: assign canonical sorted ModuleID 0..<M,
                   run sema_output_resolve, sema_core_roles_check, sema_trait_laws_add,
                   check sema_registration_blocking_error — O(headers), < 5ms]
              ──▶ ModuleComptimeReady(m) [independent #run DAG nodes dispatch to K warm children]
              ──▶ BodyCheck(m) [parallel per-function type/borrow/typestate check; CoreDemand
                                pushes newly demanded Core fns dynamically onto the same pool]

CONVERGENCE CHECKPOINT (compact whole-call-graph fixpoints over summaries, NOT AST re-walks):
  1. `comptime.bodies` / `comptime.validate` / `core_stand_in_rerun` settle (only affected
     demand sets re-enter BodyCheck; rollback trail `SemaGraphTrail` stays intact until here)
  2. `sema.failure` (`sema_failure_solve` + `sema_failure_union_infer` + `sema.recheck`)
     fixes exact `T Never!` vs `Result` calling convention for every function
  3. `sema.effects` + `sema_mark_masked_branches` stamps final `FnBody.masked` / `fixed_tags`
  4. `dead_code: remove` reachability + `single_thread` summary + `#Inline(Always)` body index

TIER 2 — Streaming Per-Function Lowering → O0 Copy-and-Patch → Direct Object Emit:
  FunctionReady(fn_id) [every reached function across all modules, NO IR_BUILD_MAX_PARTS=12 cap!]
    ──▶ ir_build(fn_id) [per-worker scratch; #Inline(Always) reads immutable index]
    ──▶ x64_patch(fn_id) [#6795 copy-and-patch stencils, emitting directly into pre-indexed
                          slot[fn_id] — NO whole-program IR merge barrier!]
    ──▶ Parallel ELF / Direct-Link layout (`ir_x64_hot_order` over compact fn size/cold flags)
```

**Strict Determinism Guarantees (`S2 == S3` & `-j1 == -j32`):**
- **Canonical `ModuleID`:** Assigned in sorted module-path order at the lightweight Header Barrier (after `use_driven` discovery finishes), before any `TypeID`/`SymbolID` emission.
- **Diagnostics:** Every pass tags diagnostics with `(pass_id, module_id, local_ordinal)` and runs canonical post-merge deduplication (`sema_failure_domain_dedupe`, `sema_taint_join_reports`, `sema_crypto_misuse_precedence`, `jet_driver_comptime_diagnostics_add`), keeping UI snapshots 100% byte-identical across thread counts.
- **Relocatable code emission:** Tier 2 emits per-function code bytes + local symbol/data relocation records using canonical `(ModuleID, local_fn_idx)` IDs; `StaticLink` patches final offsets in deterministic index order.

### 3.3 Addressing Barrier #6 (`build.step` / `runtime`: `16.3s` = `51%` of Wall Time!)
The `#5479 c1` cell (`jet build Compiler` = `jet-bare` + `runtime` + `core-embed`) cannot reach `>= 24×` parallelism or `>= 400K LOC/s` unless `build.step` (`16.3s`) is redesigned too:
1. **Forward `--threads` & stop `2×` oversubscription (`Compiler/build.jet:103–116`, `Toolchain/RuntimePack.jet:621`, `CLI/Request.jet:254`):**
   - Pass `--threads=<N>` from `BuildExecute.jet` into `runtime-pack` and `jet-bare doc unit.jet` (`RuntimePack.jet:621`).
   - Split the thread budget cleanly in `Request.jet:254` (`JetRuntime` + `CoreLibrary` share one unified work-stealing `CompilerThreads` pool instead of spawning two independent pools each sized to `cap`!).
2. **Parallelize `BuildExecute.jet:293–295` ready targets:** Launch all ready build targets concurrently (`prelude-sources`, `lexer-keywords`, `runtime-pack`) without the 1-target-per-round cap.
3. **Eliminate internal `runtime` serialization (`Request.jet:249–314`, `CoreLibrary.jet`):**
   - Currently `runtime.core` (`12.56s`) compiles all 21,211 Core functions at **`-O2`** inside `jet build -O0 Compiler` (`CoreLibrary.jet`), dominating wall time with `-O2` `ir.opt` (`2.73s`) + `-O2` `ir.x64` + `runtime.core.object` (`1.43s`) + `runtime.core.interface` (`1.22s`) + `runtime.pack` (`0.90s`).
   - Remove the `IR_BUILD_MAX_PARTS = 12` cap (`BuildWaves.jet:120`), `SEMA_FINALIZE_SIDE_EFFECT_PARTS = 16` (`CheckProgram.jet:13945`), and `threads / 4` taint cap (`CheckProgram.jet:13721`) so `runtime.core` scales smoothly to 32 threads instead of regressing (`1.74s → 1.93s` on `ir.build`)!
   - When `jet build -O0 Compiler` builds an `-O0` toolchain, keep `runtime` Core at `-O2` ONLY where required by `CodegenCeiling` / I9 (or prove via Tier-2 streaming + `#6793` CPU cut + 32-thread scaling that `runtime.core` drops below `~1.0s` wall at 32 threads!).

### 3.4 Parallel Warm Comptime Pool (`Compiler/JetBackend/Source/Image/SessionHost.jet` + `Compiler/JetEval/Source/IR/Native.jet`)
- **Live baseline finding:** Across `jet build -O0 Compiler`, `comptime` runs 7 times (`9.11s` total across parent + child steps; `25` prefetch/precheck waves, `21,252` `comptime.apply` rows, `1.28s` `comptime.land.splice`).
- **Two real locks to remove in `Native.jet`:**
  1. `sealed: Shared<JetEvalNativeChild>` (`Native.jet:114–118`) serializes all sealed `#run` evaluations onto one child; replace with a lazy pool of `K` warm children (`K = min(4..8, independent_ready_items)` — spawning extra children ONLY when `>= 2` independent `#run` DAG items are ready, avoiding startup overhead on small compiles!).
  2. `image: Shared<JetEvalNativeImage>` (`Native.jet:286–326`) serializes batch binding; keep the base `jet_runtime.image` read-only and allow concurrent per-child relocation binding against independent `memfd` SHM regions (`SessionHost.jet`).
  3. Order `stdout`/`stderr`, `debug_trace`, and `JetDriverComptimeInputLog` deterministically by `(ModuleID, item_idx)` after batch join.
- **Required probe before building S5:** Instrument `jet_driver_run_comptime` to count the exact number of independent ready `#run` DAG items per wave on `Compiler/JetCLI/run.jet` and `Core`, proving `K` children net-win after spawn + SHM setup cost.

## 4. Ordered Slice Plan & Proofs (To Be Recorded on Tower `#6794`)

| Slice | Tag | Scope & Files | Exact Proof |
|---|---|---|---|
| **S1** | **GENERAL** | Remove hard thread caps (`BuildWaves.jet:120` `IR_BUILD_MAX_PARTS: 12 → threads`, `CheckProgram.jet:13945, 13721`), shard `Alloc.jet` large-block/slab locks (`words 216, 736`), upgrade `JetRuntime/Source/Scheduler.jet` to lock-free per-carrier work-stealing, replace compiler `Shared` queues with safe `Atomic<Int>` (`0` new `#Unsafe`). | `32t < 16t` wall on `ir.build`/`ir.object`/`register`, `jet perf` shows 0 queue/large-block lock dominance (or files a `jet perf` gap on the dogfooding card if a counter is missing), `UnsafeRatchet` 0 rises, `S2 == S3` exact match. |
| **S2** | **COMPILER-ONLY** | Fix Barrier #6 (`Compiler/build.jet:103–116`, `Toolchain/RuntimePack.jet:621`, `CLI/Request.jet:254`, `CLI/BuildExecute.jet:293`): forward `--threads`, unify `JetRuntime`+`Core` thread pool (eliminate 2× oversubscription), parallelize ready targets. | Measured `runtime` wall drop at `16t` and `32t`, `S2 == S3` exact match, full `golden`/`ui`/`cases`. |
| **S3** | **COMPILER-ONLY** | Tier-1 streaming module DAG (`Driver/Pipeline.jet`, `Loader.jet`, `CheckProgram.jet`): stream `Parse → HeaderRegister`, lightweight Header Barrier, stream `BodyCheck` + dynamic `CoreDemand` on work-stealing pool with `(pass_id, module_id, ordinal)` deterministic diagnostics; file `Frozen<T>` ballot if persistent cross-phase reads cannot use lexical `task.group`. | `S2 == S3`, `-j1 == -j16 == -j32` SHA-256 match, `jet test Compiler --profile golden`, `--profile ui`, `--profile cases` on every tier (`O0`/`O1`/`O2`). |
| **S4** | **COMPILER-ONLY** | Tier-2 streaming `ir_build(fn_id) → x64_patch(fn_id)` (`CLI/Host.jet`, `JetIR/BuildWaves.jet`, `X64Module.jet`, `StaticLink.jet`): eliminate whole-program IR merge barrier after the convergence checkpoint (`sema.failure`, `sema.effects`, `masked`, `#Inline(Always)` index); reclaim per-module flat arenas once `#6793 c3–c4` lands (`I1` leak-free). | `S2 == S3`, `CodegenCeiling 9/9`, full `golden`/`ui`/`cases` on every tier, Peak RSS drop. |
| **S5** | **COMPILER-ONLY** | Parallel warm comptime pool `K` children (`JetBackend/Source/Image/SessionHost.jet`, `JetEval/Source/IR/Native.jet`, `Driver/BodyComptime.jet`) gated on DAG-width probe; close `#6794 c3` scaling gate alongside `#6793`. | Committed `#5479 c1` receipt (`32` hardware threads, cold, `n=3` median) + `jet perf` profile: `CPU-s / wall >= 24`, `32t < 16t`, `>= 400K LOC/s`, zero global allocator/queue lock in profile. |
