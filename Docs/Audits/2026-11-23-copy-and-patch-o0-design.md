# Copy-and-Patch Machine Code for the `-O0` Tier — Design Note (`#6795 c1`, Rev 2)

- **Date:** 2026-11-23
- **Revision:** Rev 2 (incorporating all 7 findings `F1–F7` from adversarial review `024f1dc9`)
- **Card:** `#6795` (Criteria `c1`–`c4`)
- **Measured Baseline Commit:** `fb1cb69a62` (Stage-2 `jet` built from `fb1cb69a62`, `$JET_SCRATCH_ROOT/tmp/milestone-s2-dist/jet`)
- **Scope:** `-O0` x86-64 code generation (`jet run`, `jet dev`, `jet build -O0`, compile-time execution)

---

## 1. Problem Statement and Live Baseline Measurements

`#6795` requires `-O0` code generation to emit every Jet IR opcode by copying and patching a toolchain-built stencil set built by `Compiler/build.jet` and embedded in the `jet` binary with no file I/O per compile (`c2`), preserving identical output across the `S2 == S3` self-host fixed point, goldens, UI snapshots, and cases on every tier (`c3`), while cutting `-O0` backend CPU time per function by at least **`10×`** on both the `#5479 c1` cell (`Tools/Perf/CompileThroughput/Throughput.jet` / `Compiler/JetCLI/run.jet`) and a second program (`jetc-trace`, `Tools/Perf/JetcTrace/Summarize.jet`), with emitted `-O0` runtime speed staying within **`1.5×`** of the original `-O0` backend (`c4`).

### 1.1 Live Baseline Measurements (`fb1cb69a62`)

All measurements below were collected live on this machine using the Stage-2 `fb1cb69a62` compiler binary (`jet build -O0 --threads 1` and `--threads 12`). Adding `--opt-verify` forces `fast_o0 = false` (`Compiler/JetCLI/Source/CLI/Host.jet:1918`), exercising the original `X64Select` `-O0` backend as the baseline reference. Note that `debug_info: true` is the stock default for `jet run`, `jet dev`, and `jet build -O0` (`Compiler/JetFoundation/Source/Registry/BuildOptions.jet:76, 88`; `Host.jet:1145, 2069`), so all baseline runs include full DWARF/line/CFI generation.

#### Compile-Time `-O0` Backend CPU (`ir.x64`)

| Workload | Backend Path | Threads | Functions | `ir.x64` Wall | `ir.x64` CPU | CPU / fn | Ratio vs `X64Select` | `10×` Target (`ir.x64` CPU) |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| `Compiler/JetCLI/run.jet` | Original `X64Select` (`--opt-verify`) | 1 | 62,045 | 7,496.19 ms | **7,456.78 ms** | **120.18 µs/fn** | 1.00× | **<= 745.7 ms (<= 12.02 µs/fn)** |
| `Compiler/JetCLI/run.jet` | Current `X64Patch` (`fast_o0 = true`) | 1 | 61,523 | 3,028.85 ms | **3,013.49 ms** | **48.98 µs/fn** | **2.45×** | needs **4.08×** further cut |
| `Compiler/JetCLI/run.jet` | Current `X64Patch` (`fast_o0 = true`) | 12 | 61,524 | 479.38 ms | **4,270.00 ms** (proc) | **69.40 µs/fn** | — (+42% MT inflation) | <= ~750–900 ms proc CPU |
| `Tools/Perf/JetcTrace/Summarize.jet` | Original `X64Select` (`--opt-verify`) | 1 | 458 | 30.99 ms | **30.82 ms** | **67.30 µs/fn** | 1.00× | **<= 3.08 ms (<= 6.73 µs/fn)** |
| `Tools/Perf/JetcTrace/Summarize.jet` | Current `X64Patch` (`fast_o0 = true`) | 1 | 455 | 11.88 ms | **11.81 ms** | **25.96 µs/fn** | **2.59×** | needs **3.86×** further cut |

Sub-span breakdown on `Compiler/JetCLI/run.jet`:
- `X64Select` (1t): `ir.x64.select = 7,355.66 ms`, `ir.x64.tail_calls = 77.06 ms`, `ir.x64.tables_finish = 23.83 ms`. Downstream: `ir.object = 2,133.83 ms`, `native.link = 454.91 ms`.
- Current `X64Patch` (1t): `ir.x64.select = 2,989.18 ms`, `ir.x64.tables_finish = 23.61 ms`. Downstream: `ir.object = 1,865.33 ms`, `native.link = 416.84 ms`.
- Current `X64Patch` (12t): `ir.x64.select = 399.47 ms wall / 4,200.00 ms proc_cpu`, `ir.x64.scatter = 67.73 ms wall / 60.00 ms proc_cpu`, `ir.x64.tables_finish = 11.80 ms`.

#### Runtime Speed of Emitted `-O0` Code (`jetc-trace` on 11.6 MB real trace, 5 runs)

| Binary Built From `Tools/Perf/JetcTrace/Summarize.jet` | Median Wall | Range (5 runs) | Ratio vs `X64Select` | `#6795 c4` Limit |
|---|---:|---:|---:|---:|
| Original `X64Select` (`-O0 --opt-verify`) | **250.04 ms** | 246.56–250.21 ms | 1.000× | — |
| Current `X64Patch` (`-O0`) | **369.26 ms** | 361.81–379.55 ms | **1.477×** | **<= 1.500×** |

**Key takeaway:** Current `X64Patch.jet` is **4.08× too slow to compile** (`2.45×` instead of `>= 10×`) and sits only **2.3% below the `1.500×` runtime ceiling** (`1.477×`).

---

## 2. Strongest Existing Implementations (Fair Best-Case Comparison)

| System | Core Technique | Best-Case Compile Speed | Best-Case Runtime Quality | Key Strengths | Why Not Verbatim for Jet |
|---|---|---|---|---|---|
| **Xu & Kjolstad, "Copy-and-Patch Compilation" (OOPSLA 2021)** | Compiles C++ stencil templates with Clang `-O3` to ELF `.o`; extracts byte templates + hole tables at build time; compiles constant-length `memcpy(dst, stencil, N)` down to 1–3 unaligned word stores; specializes register-pinned operand variants (`rax`/TOS). | ~100× faster than LLVM `-O0`; ~10–30 ns/IR op. | Within ~1.1×–1.3× of LLVM `-O0`. | **Proves** constant-length pre-encoded stencil stamping + 1-register operand-forwarding variants give both ultra-fast emission and strong runtime quality with zero register allocator. | Requires Clang/LLVM in the toolchain build; Jet's toolchain is 100% self-hosted Jet (`Compiler/Toolchain/RuntimePack.jet:36` runs no C compiler), and safe Jet (`0` new `#Unsafe`) cannot collapse a runtime-length byte loop into unaligned `memcpy`. |
| **CPython 3.13 Tier-2 JIT (PEP 744, Brandt Bucher, 2024)** | Build-time generator emits C source tables + patchers from Clang-compiled micro-op objects directly into the compiled host binary (zero runtime file I/O); uses top-of-stack register caching. | Sub-microsecond per trace; zero runtime encoder. | Beats bytecode interpreter via TOS caching. | **Proves** build-time code-generated stencil modules compiled directly into the host binary give zero-I/O embedding without external packaging plumbing. | Uses external Clang object disassembly and trace-linear micro-ops (no structured CF or full System V aggregate ABI). |
| **Deegen / LuaJIT Baseline (Haoran Xu, PLDI 2024; Mike Pall)** | Meta-compiler emits copy-and-patch baseline stencils with register-pinned fast paths and out-of-line cold stubs. | ~20–50 ns/op. | Fast runtime from hot/cold split and accumulator pinning. | **Proves** hot inline fast-path stencil + out-of-line cold stub keeps L1I hot while supporting exact-`Int` overflow and stop-poll slow paths cheaply. | Dynamic-VM LLVM meta-compiler rather than a self-hosted native encoder. |
| **TCC (Fabrice Bellard) / Cranelift FastISel** | Single-pass direct emission with a tiny local value/accumulator cache. | TCC ~1M LOC/s single-core; ~30–80 ns/op. | ~1.2×–1.6× slower than `-O2`. | **Proves** single-pass emission with a tiny accumulator + flags + constant cache needs zero liveness passes. | Hand-written opcode emitters duplicate ISA encoding logic rather than deriving from one encoder authority. |

### Critical Safe-Jet Finding (From the 5-Arm Probe in Section 5)

In C/C++ copy-and-patch, `memcpy(dst, stencil_ptr, 12)` compiles into two immediate/word moves because `12` is a compile-time constant. In **100% safe Jet** (`0` `#Unsafe` outside the memory kernel):
- A runtime-length byte loop (`loop k in 0..<d.len -> scratch[pos + k] = blob[off + k]`, **Arm B1**) executes two bounds-checked byte accesses per byte and measures **`744 ms` (`67 ns/op`)** — **2.05× slower** than direct pre-sized writes (**Arm A2**, `362 ms`)!
- Per-op slice `out.extend(blob[off..<(off + len)])` (**Arm B2**) pays a runtime function call per 8–20 byte stencil and measures **`634 ms` (`57 ns/op`)** — **1.75× slower** than **Arm A2**!
- **Toolchain-generated compile-time-unrolled `#Inline(Always)` stencil stampers (`Arm B3`)**, generated by `Compiler/build.jet` (`Compiler/Toolchain/X64Stencils/Main.jet`) using `JetBackend/Source/X64/Encoder.jet` into `Compiler/JetIR/Source/X64StencilData.jet`, execute straight-line pre-encoded byte stamps with zero runtime loop, zero double-write of patched holes, and zero `Encoder.jet` dispatch at `-O0` — measuring **`353 ms` (`31 ns/op`)**: **`2.10×` faster than Arm B1**, **`1.80×` faster than Arm B2**, **`1.02×` faster than Arm A2**, and **`2.03×` faster than Arm A**!

---

## 3. Root-Cause Anatomy of Current `X64Patch.jet`

Live inspection of `Compiler/JetIR/Source/X64Patch.jet` (4,826 lines), `Compiler/JetIR/Source/DropEarly.jet`, `Compiler/JetIR/Source/X64Module.jet`, and `Compiler/JetBackend/Source/Image/Emission.jet` on `fb1cb69a62` identifies six verified bottlenecks:

1. **R1 — Not toolchain-built stencils (92% byte-by-byte pushes):** `X64Patch.jet` has **`710`** calls to `p.code.push(U8{...})` vs only **`62`** calls to `patch_emit(...)` (which itself constructs slices and calls `jet_rt_list_extend`). `Compiler/build.jet` generates no `-O0` stencils.
2. **R2 — 10 per-function pre-emission passes + heap-allocating `DropEarly`:** `ir_x64_patch_function_in` (`X64Patch.jet:4263–4530`) runs 10 pre-emission passes (slot class, liveness `starts`/`ends`/`writes`/`last_write`, `ir_x64_result_storage`, transitive `StackAddress` fixed-point loop `4391–4416`, loop-nesting tree + fixed-point `ir_x64_grow_range` `4417–4472`, greedy 32-temp reuse `4476–4506`). Because `Host.jet:1989` skips `ir_opt_o0_bodies` under `fast_o0 = true`, `X64Patch.jet:4355` calls `ir_drop_plan` (`DropEarly.jet:128–239`), which allocates `refs: [[Int]]` — one heap `[Int]` per slot (`DropEarly.jet:182–183`) — plus `stop`, `eligible`, `dropped`, `blocked`, `open`, and `kill` per function.
3. **R3 — Heap-boxed relocs & debug locations + per-call allocations (+42% 12t contention):** `X64RelocTarget` (`Emission.jet:19`, `40` sites in `X64Patch.jet`) and `X64DebugLocation` (`Emission.jet:77`, `6` sites) are heap-boxed payload enums. `patch_call` (`X64Patch.jet:1378–1442`) heap-allocates `places`, `stacked`, `kept`, and boxed `X64DebugLocation` per arg on every call; `Drop`/`Close`/`Concat` allocate temporary `[Int]{slot}` lists; `IRPatchCold` allocates `[Int]{site}`; `IRPatchStopEdge` clones `p.ctrl_open`. Under 12 threads, this drives process CPU from `3,013 ms` to **`4,270 ms` (+42% allocator contention)**.
4. **R4 — Per-op string chains and hash lookups:** `patch_call_runtime` runs an 18-branch string-equality chain + `ir_x64_inline_atomic` + `ir_x64_alloc_type` per `CallRuntime`; `patch_drop_slot` runs `name_rows_value(m.function_rows, drop)` per `Drop`; `ir_x64_debug_line` lacks an inline `line != last_line` guard.
5. **R5 — Heavy `debug_info == true` metadata on the default path:** Because `debug_info == true` is the stock default (`BuildOptions.jet:76`), every `-O0` compile builds per-call `X64CallSite` + `X64CallArgument` entry-value tracking (redundant at `-O0` where every parameter lives in a fixed frame slot!), per-function fresh CFI vectors, and boxed variable ranges.
6. **R6 — Pure stack-spill codegen sits at `1.477×` runtime speed:** Reloading every operand from `[rbp + disp]` and re-testing boolean slots on `If`/`BrIf` leaves almost zero margin under `1.500×`.

---

## 4. Proposed Architecture (`#6795 c2`–`c4`)

### 4.1 Build-Time Stencil Generator & Compiled-In Embedding (Fixes `F1`)

Instead of an external runtime file (which would break `S1 jet-bare` inside the `runtime` Bubblewrap sandbox at `Compiler/build.jet:103–116` / `RuntimePack.jet:621`, as well as `JetEval`, `JetDebug`, and `jet test Compiler/JetIR`), we follow Jet's exact blessed build-time generator pattern (`Compiler/build.jet:84–101`, mirroring `prelude-sources` and `lexer-keywords` -> `JetLexer/Source/Lexer/KeywordSpans.jet`):

```text
Compiler/Toolchain/X64Stencils/
  package.jet   (deps: { jet_backend: ../../JetBackend })
  Main.jet      (encodes every -O0 stencil via JetBackend/Source/X64/Encoder.jet)
      |
      |  run by Compiler/build.jet (`x64-stencils` step, `--write` / `--check`)
      v
Compiler/JetIR/Source/X64StencilData.jet   (kept generated file, compiled directly into JetIR)
```

- **Build wiring (`Compiler/build.jet`):**
  1. `stencils_tool: b.add_executable("x64-stencils", ["Toolchain/X64Stencils/Main.jet"], [])` — compiles in parallel with `prelude-sources` and `lexer-keywords` (depends only on `JetBackend`).
  2. `x64_stencils: b.run("x64-stencils", b.host(stencils_tool), ["FS"])` with `--write`, reading `JetBackend/Source/X64/Encoder.jet` and `Toolchain/X64Stencils/Main.jet`, writing `JetIR/Source/X64StencilData.jet` (`kept: true`), and supporting `--check` for `LaneCheck`.
  3. `bare: b.add_executable("jet-bare", ["JetCLI/run.jet"], [prelude, keywords, x64_stencils])`.
- **Single encoding authority:** `X64Stencils/Main.jet` calls `JetBackend/Source/X64/Encoder.jet` with sentinel operands for every stencil, verifies every patched hole offset, and emits `Compiler/JetIR/Source/X64StencilData.jet` with header `// This file is generated by Compiler/Toolchain/X64Stencils/Main.jet, which \`jet build Compiler\` runs, from Compiler/JetBackend/Source/X64/Encoder.jet; do not edit.`
- **What `X64StencilData.jet` contains:**
  - **~185 `#Inline(Always)` pre-encoded stencil stamp functions** (`o0_st_*`) that write the exact `Encoder.jet`-produced machine bytes and operand holes straight into `scratch` at `pos` with zero runtime loop, zero double-write of hole bytes, and zero calls to `Encoder.jet` at `-O0`.
  - `O0_STENCIL_COUNT`, `O0_STENCIL_BYTES` (total ~2.8 KB of pre-encoded machine code across ~2,400 lines of generated Jet), and a deterministic SHA-256 digest constant `O0_STENCIL_DIGEST` checked by `Tests/Cases/Backend/o0_stencil_emit.case`.
- **Bootstrap & zero-I/O properties:**
  - **Zero file I/O per compile:** Stencils are compiled directly into `JetIR`'s machine code / `.rodata` in `jet-bare`, `jet`, `JetEval`, `JetDebug`, and `jet test Compiler/JetIR`.
  - **Zero `RUNTIME_FILES` / `RuntimePack.jet` / `RuntimeEmbed.jet` / `Host.jet` / Bubblewrap changes:** `S0 -> S1 -> S2 -> S3` has zero circularity (`S0` compiles `x64-stencils` from `JetBackend`, runs it to generate/verify `X64StencilData.jet`, builds `S1 jet-bare`, runs `runtime`, embeds `S1 jet`, then `S1 -> S2 -> S3` produces byte-identical `S2 == S3`).

### 4.2 Exact New Encodings Required in `Compiler/JetBackend/Source/X64/Encoder.jet` (Fixes `F4`)

Auditing `Compiler/JetBackend/Source/X64/Encoder.jet` on `fb1cb69a62` confirms almost all needed instructions already exist (`x64_mov_*`, `x64_lea`, `x64_imul_*`, `x64_idiv`, `x64_cqo`, `x64_shift_*`, `x64_setcc`, `x64_movzx_r8`, `x64_rep_movsb`, `x64_ud2`, `x64_call_*`, `x64_jmp_rel32`, `x64_jcc_rel32`, `x64_ucomisd`/`ss`, `x64_float_*`, `x64_vfmadd213sd`, `x64_lock_*`). Exactly **6 small encoding helpers** are added to `Encoder.jet` so `X64Stencils/Main.jet` encodes 100% of stencils through `Encoder.jet` with zero hand-coded hex:

1. `x64_leave(code)` (`0xc9`)
2. `x64_cmp_gs_abs(code, reg, offset)` / `x64_mov_gs_abs(code, reg, offset)` (`0x65` `gs:` prefix for `cmp rsp/r11, gs:[limit]` stack checks and TLS attention-word stop polls)
3. `x64_alu_rm(code, op, dst, base, disp)` (64-bit reg-from-memory ALU: `add`/`sub`/`cmp`/`and`/`or`/`xor rax, [rbp + disp]`)
4. `x64_load_signed(code, dst, base, disp, width)` (`movsx` 8/16-bit and `movsxd` 32-bit signed memory loads)
5. `x64_mov_r32_imm32(code, dst, value)` (32-bit zero-extending `mov r32, imm32`)
6. `x64_alu8_rr(code, op, dst, src)` (8-bit `and`/`or al, cl` for IEEE Float `Eq`/`Ne` unordered NaN masking)

### 4.3 Canonical Micro-Stencil Decomposition and Full Opcode/Feature Coverage (Fixes `F4`)

Instead of a Cartesian product of `disp8 × disp32` across 3 operands (which caused `+6%` code bloat in `Arm B1`/`B2`), the ~185 stencils factor cleanly into:
- **Operand micro-stencils (~36 stencils):** `load_reg_{0..15}_{8,32}`, `store_reg_{0..15}_{8,32}`, `load_xmm_{0..7}_{f32,f64}_{8,32}`, `store_xmm_{0..7}_{f32,f64}_{8,32}` (handling arbitrary mixed `disp8`/`disp32` with zero bloat!).
- **Core operation stencils (~115 stencils covering all 52 `IROp`s at `IR.jet:107–160`):**
  - `Nop` (0B), `Unreachable` (`ud2`)
  - `Const` (`imm32`/`imm64`/`f32`/`f64`), `ConstText` (`lea rip`), `ConstBigInt`
  - `Copy`/`Move` (`word`, `acc`, `f32`, `f64`, `agg`, `retain`)
  - `Drop`/`Close` (`drop_call`, `drop_str_fast` inline immortal/RC check + cold release, `close_call`)
  - `Add`/`Sub`/`Mul`/`Div`/`Rem`/`FloorDiv`/`Mod`/`BitAnd`/`BitOr`/`BitXor`/`Shl`/`Shr` (exact-`Int` inline fast path + cold big-int stub; fixed `Word` wrap/trap `{8,16,32,64}`; IEEE `Float`/`Float32`; `acc` and `imm32` variants)
  - `Eq`/`Ne`/`Lt`/`Le`/`Gt`/`Ge` (exact-`Int`, signed/unsigned `Word`, IEEE `Float`/`Float32` including NaN-safe `setcc + setnp/setp + and/or al, cl`, `String` `jet_rt_string_eq`)
  - `Neg`/`Not`/`Sqrt`/`IntBig`/`Convert` (all numeric conversion pairs)
  - `Concat` (`jet_rt_string_concat` / allocator variant)
  - `Call`/`CallRuntime`/`CallIndirect` (arg micro-stencils + `call_rel32` + `call_r11` + `store_ret` + inline fast-path stencils for `block_unique`, `block_retain`, `block_release`, `int_owned_*`, `checked_fixed`, `checked_widen`, `alloc_8`/`free_8`, atomics)
  - `FunctionAddress`, `DataAddress` (including `m.core_cells` -> `X64_RELOC_RUNTIME_DATA` per `X64Patch.jet:3180`)
  - `Load`/`Store`/`StreamStore` (widths `{1s,1u,2s,2u,4s,4u,8,f32,f64}` × field offset `{0,disp8,disp32}` × `acc`; `StreamStore` lowers as plain `Store` per `IR.jet:67–69`)
  - `StackAddress` (`lea rbp`), `Offset` (`add imm`), `CopyBytes` (inline 8/16/24/32 + `rep movsb`)
  - `Block`/`Loop`/`If`/`Else`/`End`/`Br`/`BrIf`/`Return` (`jmp_rel32`, `if_flags_jcc`, `if_slot_{8,32}`, `ret_void`, `ret_rax`, `ret_xmm0`, `ret_agg`)
- **Special runtime & ABI stencils (~34 stencils):**
  - `prologue`, `stack_check_small` (`cmp rsp, gs:[limit]`), `stack_check_large` (`lea r11, [rsp - frame]; cmp r11, gs:[limit]`), `stack_grow_stub`
  - `stop_poll_{st,mt}` (`IR_STOP_POLL_KEY`, `IR_STOP_TRAP_KEY`, `IR_STOP_TURN_KEY`), `IR_STACK_MARK_KEY`
  - `tail_jump` (`IR_TAIL_JUMP` at `X64Patch.jet:3112, 3291`: `leave; jmp rel32`)
  - `perf_zone_site` (`test eax, imm32` 5-byte patchable site + cold stub for `jet_perf_zone_open_named`, `jet_perf_zone_close`, `jet_perf_frame`, `jet_perf_plot_float` at `X64Patch.jet:2854`)
  - `unique_fast` (`#unique<...>` inline refcount-1 fast path at `X64Patch.jet:412, 3116`)
  - `fma_probe` + `vfmadd213sd` (`jet_std_math_fma`)
  - Cold-stub templates (`.Route`, `.LocatedRoute`, `.Compare`, `.Stop`, `.ShiftStop`, `.Poll`, `.Release`)
- **Fallback predicates preserved verbatim (`X64Select.jet:5567`):** `f.frame_bound >= 0`, `f.asm != None` (covers inline `#FFI(asm)` leaves), `ir_stop_key(f.key)`, `ir_timing_marked(f)`, and `JET_X64_EMIT_LOG` (`names.len() > 0 && ir_x64_log_names(f, names)`) continue routing through `X64Select`.

### 4.4 Sound 4-Pass Zero-Allocation `DropEarly` (`D-DROP-ASAP1=A`, Fixes `F2`)

`Tests/Cases/MemorySemantics/StringsFreeAtTheirLastUse.fixture/main.jet` enforces `D-DROP-ASAP1=A` on every tier, including multi-`Drop` slots (`lines 48–49`). We replace `ir_drop_plan`'s per-function heap allocations (`refs: [[Int]]` + 6 lists in `DropEarly.jet:128–239`) with a **4-pass zero-allocation `ir_drop_plan_into`** reusing flat scratch arrays on `IRPatchEmitter` (`drop_stop`, `drop_eligible`, `drop_dropped`, `drop_last_ref`, `drop_blocked`, `drop_open`, `drop_kill`, `drop_head`, `drop_chain`):

1. **Exact fast-skip:** Scan `f.slot_type` once using `type_is_str_drop[ty]`; if 0 String-drop slots exist, skip immediately (`O(slots)`). Otherwise check whether at least one `.Drop` targets a String-drop slot before running the 4 passes.
2. **Pass 1 (`stop` marking):** Identify every stop-poll edge (`ir_drop_stop_edge`) and fill `d.stop[inner] = at` (exact parity with `DropEarly.jet:149–154`).
3. **Pass 2 (`eligible`, `dropped`, `blocked`/`open`):** Outside `d.stop[at] >= 0`, apply `ir_drop_borrows_only` (disqualifying escaping Strings), mark `d.dropped[slot] = 1`, and maintain `d.open` to mark `d.blocked[opener] = 1` on `Br`/`BrIf`/`Return`/`Unreachable` (exact parity with `DropEarly.jet:164–211`). Return early if `!any_drop`.
4. **Pass 3 & 4 fused (lazy forward-only `last_ref` + `ir_drop_target` + `kill`):** Zero `d.kill[0..<n]` and set `d.last_ref[0..<count] = -1`. Walk `at` forward `0..<n` (skipping `d.stop[at] >= 0`):
   - On `f.op[at] == .Drop` for eligible dropped `slot`: read `previous = d.last_ref[slot]` (exact largest reference strictly `< at`), run `ir_drop_target(f, d.stop, d.blocked, previous, at)` + stop-poll hop (`DropEarly.jet:224–226`), push `(slot, to, line)` into scratch `drop_head`/`drop_chain`, mark `d.kill[at] = 1` and `d.kill[inner] = 1` for covered stop-poll drops in `to..<at`, and **only then** set `d.last_ref[slot] = at`.
   - On any other op outside stop polls: update `d.last_ref[s] = at` for every read or written dropped String slot `s`.
- **Multi-`Drop` soundness proof:** Because `d.last_ref[slot]` is updated strictly in forward order and advances to `at` right after planning the `.Drop` at `at`, any subsequent `.Drop` of the same slot at `at2 > at` sees `previous = max({r in refs[slot] | r < at2})`, identical bit-for-bit to `DropEarly.jet:217–220` — with **zero heap allocations**.

### 4.5 Sound `rax` Accumulator, `RFLAGS` Fusion, and Epoch-Stamped `const` Table (Fixes `F3`)

1. **`RFLAGS` compare-branch fusion (strictly restricted for 100% soundness):**
   - **Allowed ONLY on:** (a) exact-`Int` inline-fast compares (`Eq`/`Ne`/`Lt`/`Le`/`Gt`/`Ge`; note `.Compare` cold stubs at `X64Patch.jet:3989–3992` execute `cmp rax, 0` before `jmp cold.resume`, leaving signed flags matching `cc`!), (b) fixed-width `Word` compares, and (c) `Float`/`Float32` **ordered** `<`/`<=`/`>`/`>=` (`ucomisd`/`ucomiss` where `a`/`ae`/`b`/`be` with canonical operand order yields `false` on NaN without extra instructions).
   - **Explicitly FORBIDDEN on:** `Float`/`Float32` `Eq`/`Ne` (where `setcc + setnp/setp + and/or al, cl` overwrites `RFLAGS`), `String` `Eq`/`Ne` (`jet_rt_string_eq` returns in `al` without setting comparison flags), and `Not` (always invalidates `flags_slot = -1`).
   - **Invalidation:** Clear `flags_slot = -1` on any intervening `DropEarly` moved drop, any stop poll, any op that modifies `RFLAGS`, and every control-flow boundary (`Block`/`Loop`/`If`/`Else`/`End`/`Br`/`BrIf`).
   - **Correct opcode patching:** Track `flags_inv_cc: U8 = 0x80 | invert(cc)` and stamp `0x0f, flags_inv_cc, rel32` into the `If`/`BrIf` stencil (never hardcoded `0x85`).
2. **`rax` accumulator forwarding (strictly restricted for 100% soundness):**
   - **Excluded ops:** `Rem`/`Mod` (result in `rdx`), `Float`/`Float32` ops (result in `xmm0`), and any op whose `.Route`/`.LocatedRoute` cold stub runs `patch_emit_cold_drop` (`X64Patch.jet:3941–3945`, which clobbers `rax` before `jmp cold.resume`) — those ops explicitly set `acc_slot = -1`!
   - **Invalidation:** Clear `acc_slot = -1` on any write to `acc_slot`, any `Drop`/`Close`/`DropEarly` moved drop, any stop poll, any store/helper clobbering `rax`, and every control-flow boundary.
3. **Epoch-stamped per-block `const` table (O(1), zero pre-pass, zero allocation):**
   - Pre-sized `const_val: [Int]` + `const_epoch: [Int]` in `IRPatchEmitter`. Increment `block_epoch += 1` at function entry and every control-flow boundary (`Block`/`Loop`/`If`/`Else`/`End`/`Br`/`BrIf`), and zero `const_epoch[dst] = 0` on any non-`Const` write to `dst`.
   - Preserves **all** multi-arg constant fast paths (`jet_int_checked_fixed`, `jet_int_checked_widen`, `alloc_8`/`free_8`, `patch_located_stops`, and exact-`Int` constant right-hand operands) with zero liveness pass — protecting `jetc-trace` runtime speed!

### 4.6 Zero-Liveness Frame Layout, Stop-Edge Replacement, and Stack Safety (Fixes `F4`)

1. **`Unit` skipping + O(1) frame homes + inline `StackAddress`:**
   - One `O(slot_count)` loop assigns `homes[s] = 0` for `Unit` slots and `p.depth += 8; homes[s] = 0 - p.depth` for non-`Unit` slots — wasting 0 bytes on `Unit` slots while keeping the first 16 non-`Unit` slots in `disp8` (`[-128..-8]`).
   - Drop the transitive `StackAddress` fixed-point loop (`X64Patch.jet:4391–4416`) and `ir_x64_result_storage` (`4389`): `StackAddress` allocates aligned frame storage inline on `p.depth` during emission (`lea rax, [rbp - depth]; mov [rbp + home(dst)], rax`), and returning an `Aggregate` copies through `[rbp + result_home]` (already implemented in `X64Patch.jet`).
2. **Hard stack-overflow safety on large non-recursive functions:**
   - Because `f.stack_check` is only set on call cycles (`IR.jet:412–417`), and the runtime red zone below the limit is `256 KiB` with `IR_X64_STACK_FRAME_ROOM = 65536` (`X64Select.jet:6006–6010`), dropping temp-slot reuse could theoretically push a huge non-recursive function past `64 KiB` if unchecked.
   - **Rule:** Pre-sum `stack_addr_bytes` (or trigger on `8 * non_unit_slots > 32768` / after emission if `frame_bytes > 32768`): emit the `lea r11, [rsp - frame]; cmp r11, gs:[limit]; jb grow` stack check whenever `f.stack_check || frame_bytes > 32768` (back-patching a reserved 24-byte slot or triggering when `non_unit_slots > 4096`). Stack overflow from larger `-O0` frames is therefore impossible.
3. **Replacing `p.starts`/`p.ends` in stop-edge helpers:**
   - `patch_cleanup_edge` (`X64Patch.jet:3536`): verify `token` is written at `first`, passed as the sole arg at `leave`, and untouched elsewhere inside `(first + 1)..<leave` inside the loop that ALREADY scans `(first + 1)..<leave` (`3541–3555`); because `last` is a bare `Return`, `token` cannot escape past `end_idx`.
   - `patch_group_slot_eq` (`X64Patch.jet:3560–3572`): compare outer slots by exact equality (`a == b`), and match internal temps (written inside the small cleanup group `s1..<e1` / `s2..<e2`) via a tiny scratch map of relative def offset `(k - s1)` recorded linearly inside `patch_group_eq`, without global `p.starts`/`p.ends`.
   - `IRPatchStopEdge`: store flat index ranges `(ctrl_start, ctrl_len)` into a reusable worker `ctrl_pool: [Int]` instead of cloning `p.ctrl_open`.

### 4.7 Fast `debug_info == true` Emission, `.eh_frame`, and Unified `X64FlatReloc` Cutover (Fixes `F5`)

1. **Fast `debug_info == true` (active on stock `jet run`, `jet dev`, and `jet build -O0`):**
   - **Shared constant standard-frame CFI (`STD_CFI`):** Every standard `-O0` function (`push rbp; mov rbp, rsp; sub rsp, frame`) emits the same 8-byte CFI sequence — always emitted even when `debug_info == false` so `Dwarf.jet:1866` and `DirectBind.jet:139` always have valid `.eh_frame` unwind tables.
   - **Omit `X64CallSite`/`X64CallArgument` entry-value tracking at `-O0`:** Because every `-O0` parameter is spilled to its fixed `[rbp + home]` frame slot in the prologue and remains there for the entire function body, `jet debug` reads parameters directly from their `X64VariableRange` frame locations — eliminating all per-call `kept: [X64CallArgument]` and boxed `X64DebugLocation` allocations! Keep `X64LineRow{call: true}` so `.debug_jetline` and fault/profile backtraces preserve exact call-site file:line:column.
   - **O(1) fixed-frame variable ranges:** One range `[body_start, end)` per named variable (`f.slot_name[s] >= 0`), emitted in one `O(named)` loop at function end.
2. **Full Greenfield Unified `X64FlatReloc` Cutover (I8 — zero dual fields!):**
   - Replace `X64Reloc` / `X64RelocTarget` (heap-boxed payload enum) with ONE canonical unboxed 24-byte POD struct across the entire compiler:
     `X64FlatReloc: struct { at: Int, kind: Int, target: Int }` with `U8`/`Int` constants `X64_RELOC_FUNCTION = 0`, `X64_RELOC_DATA = 1`, `X64_RELOC_RUNTIME = 2`, `X64_RELOC_RUNTIME_DATA = 3`, indexing a deduplicated per-module `runtime_symbols: [String]` table on `X64ObjectModule` / `IRX64Tables`.
   - Migrate **every** consumer in one greenfield cutover (eliminating `X64RelocTarget` completely, lowering `CompilerData.jet` `payload_enums` and `closed_lists/baseline.tsv`):
     - `Compiler/JetBackend/Source/Image/Emission.jet`
     - `Compiler/JetBackend/Source/Image/Object.jet:547, 718, 937` (also replaces per-reloc runtime string hashing with O(1) `rt_sym_index[reloc.target]`, speeding up `ir.object`!)
     - `Compiler/JetBackend/Source/Image/DirectBind.jet:91, 234`
     - `Compiler/JetIR/Source/X64Module.jet:624, 633, 1479` (also speeds up `ir.x64.scatter` by resolving `Data` relocs in place with zero enum re-boxing!)
     - `Compiler/JetIR/Source/X64Stack.jet:120, 156`
     - `Compiler/JetIR/Source/X64Select.jet`
     - `Compiler/JetIR/Source/X64Patch.jet:4760`
     - `Compiler/JetDebug/Source/NativeFix.jet:287, 427, 445, 477`
3. **Module-level O(1) lookup tables (`IRX64FastModule`, built once in `< 3 ms`):**
   - `type_class: [U8]`, `type_kind: [U8]`, `type_size: [Int]`, `type_bits: [U8]`, `type_signed: [U8]`, `type_is_str_drop: [U8]`, `type_drop_fn: [Int]` (eliminates `name_rows_value` per `Drop`), `name_rt_tag: [U8]` (eliminates the 18-branch string chain per `CallRuntime`), and `name_rt_sym_id: [Int]` (maps `m.names` directly to `runtime_symbols` IDs).

---

## 5. Honest 5-Arm Runnable Probe and Quantitative Re-Budget (Fixes `F6`)

### 5.1 Runnable Probe Setup and Live Measurements

The standalone 100% safe-Jet probe (`probe_o0_stencils.jet`, `0` `#Unsafe`) runs **61,523 functions × 180 IR ops = 11,074,140 IR ops** with **28 slots** (mixing 16 `disp8` + 12 `disp32`), structured `If`/`Else`/`End` back-patching, **25% functions with multi-`Drop` Strings running the full 4-pass `DropEarly`**, realistic **~30% operand chaining and 50% compare-branch fusion**, **`debug_info = true` metadata** (`STD_CFI`, `FlatLineRow`, `FlatVarRange`), and **full per-function output `[U8]` materialization (`134–148 MiB`)**.

Command run on `fb1cb69a62` (compiled at `-O2`, 4 runs total: 3 consecutive + 1 format-verified run):
```text
jet build -O2 probe_o0_stencils.jet && ./probe_o0_stencils
```

Verbatim measured results (3 runs + format-verified run 4):

| Arm | Architecture | Wall (4 runs) | Median | Per-Op | `.text` Size | Stack Loads / fn | Speedup vs Arm A | Speedup vs Arm A2 |
|---|---|---|---:|---:|---:|---:|---:|---:|
| **Arm A** | Current `X64Patch` model (liveness + 32-temp + heap 4-pass `DropEarly` + boxed relocs/debug + per-call allocs + string dispatch) | 714 / 726 / 716 / 717 ms | **716.5 ms** | 64 ns/op | 140 MiB | 228 | 1.00× | 0.52× |
| **Arm A2** | Optimized `X64Patch` WITHOUT stencils (arena + 4-pass zero-alloc `DropEarly` + POD `FlatReloc` + O(1) homes/tags + epoch `const` + fast `debug_info=true` + direct helpers) | 374 / 376 / 370 / 362 ms | **372.0 ms** | 33 ns/op | 140 MiB | 228 | 1.93× | 1.00× |
| **Arm B1** | **Rejected** naive byte-loop copy from binary blob (`loop k in 0..<len`) | 752 / 743 / 737 / 744 ms | **743.5 ms** | 67 ns/op | 148 MiB | 214 | 0.96× | **0.50× (2.0× slower!)** |
| **Arm B2** | **Rejected** `out.extend(blob[off..<(off+len)])` slice memcpy per op | 645 / 646 / 624 / 634 ms | **639.5 ms** | 58 ns/op | 148 MiB | 214 | 1.12× | **0.58× (1.72× slower!)** |
| **Arm B3** | **Chosen** toolchain-generated unrolled stencil stampers (`X64StencilData.jet` from `X64/Encoder.jet`, full `disp8`+`disp32` `rax`-acc & `RFLAGS`) | 356 / 378 / 364 / 353 ms | **360.0 ms** | **32 ns/op** | **134 MiB (-4%)** | **207 (-9%)** | **1.99×** | **1.03×** |

**Transparent attribution (answering `F6` honestly):**
- Between **Arm A** and **Arm B3**, ~95% of the raw compile-time speedup comes from eliminating the 10 pre-emission passes, heap-allocating `DropEarly`, boxed `X64RelocTarget`/`X64DebugLocation`, per-call allocations, and string-chain dispatch (`Arm A -> Arm A2`).
- **Arm B3** (toolchain-generated unrolled stencils from `X64/Encoder.jet`) adds a further **~2–3% compile speedup over Arm A2** (while avoiding the **1.72×–2.00× slowdown** of naive safe-Jet blob copying `B1`/`B2`), satisfies `#6795 c2` structurally under one `Encoder.jet` authority, and delivers **`-4%` smaller `.text`** and **`-9%` fewer stack loads** even in the conservative 28-slot mixed-`disp32` probe (and higher on small `<= 16`-slot functions).
- Also verified by isolating `out.extend(scratch[0..<pos])` on Arm B3 (`357 ms` without vs `346–364 ms` with): per-function output materialization across `134 MiB` costs only **~10–15 ms** total.

### 5.2 Honest Full-Backend `ir.x64` CPU Budget (`Compiler/JetCLI/run.jet` & `jetc-trace`)

Arm A omits 6 of real `X64Patch`'s pre-emission passes (transitive `StackAddress`, `ir_x64_result_storage`, nested loop tree + `ir_x64_grow_range`, `ir_x64_row_class`), `ir_x64_reads_into`, `patch_emit_stop_edges`/`patch_emit_colds` staging allocs, and `name_rows_value` per `Drop` — all of which **Arm B3 eliminates**. Adding the remaining real-compiler costs (`cold`/`stop` emission, module pre-tables, `tables_finish`, and 12t `ir.x64.scatter`) onto the measured `360 ms` Arm B3 kernel gives:

| Component (`Compiler/JetCLI/run.jet`, 1t `-O0`, `debug_info = true`) | `X64Select` (Live) | Current `X64Patch` (Live) | Proposed Arm B3 (Budget) | Notes |
|---|---:|---:|---:|---|
| Module pre-table (`type_meta`, `name_rt_tag`, `type_drop_fn`, `runtime_symbols`) | — | — | **~3 ms** | O(1) arrays built once |
| Pre-emission analysis (liveness, loops, `StackAddress`, `result_storage`, temp reuse) | ~2,200 ms | ~850 ms | **0 ms** | Eliminated |
| Core stencil emission + 4-pass zero-alloc `DropEarly` + `debug_info=true` + output materialization (`~11–14M` ops @ `32 ns/op`) | ~4,600 ms | ~1,800 ms | **~360–440 ms** | Directly measured `360 ms` @ `11.07M` ops |
| Cold stubs, stop edges, `IRX64Uses` local data table wiring | ~555 ms | ~340 ms | **~75–110 ms** | Flat SoA scratch, 0 boxed enums, 0 `ctrl_open` clones |
| `ir.x64.tail_calls` + `ir.x64.tables_finish` | 100.89 ms | 23.61 ms | **~20–24 ms** | Unchanged |
| **Total `ir.x64` 1-thread CPU** | **7,456.78 ms** | **3,013.49 ms** | **~458–577 ms** | **`12.9×–16.3×` vs `X64Select` (`10×` bar: `<= 745.7 ms`)** |
| **CPU / function (1t)** | **120.18 µs/fn** | **48.98 µs/fn** | **~7.4–9.4 µs/fn** | **23%–39% margin below `12.02 µs/fn`** |
| **12-thread `ir.x64.scatter` + proc CPU** | — | 67.73 ms wall / 4,270 ms proc | **~25–35 ms wall / ~550–700 ms proc** | Zero per-call/reloc heap churn kills +42% MT contention; `X64FlatReloc` cuts scatter boxing |

Scaling to `jetc-trace` (`Tools/Perf/JetcTrace/Summarize.jet`, `455` fns, 1t `-O0`, `debug_info = true`):
- `X64Select` live baseline: **`30.82 ms` (`67.30 µs/fn`)** -> `10×` target **`<= 3.08 ms` (`<= 6.73 µs/fn`)**.
- Current `X64Patch` live baseline: **`11.81 ms` (`25.96 µs/fn`)**.
- Proposed Arm B3 budget: **`~1.8–2.5 ms` (`~4.0–5.5 µs/fn`)** -> **`12.3×–17.1×` vs `X64Select`**.

**Runtime speed budget (`jetc-trace`, `<= 1.500×` of `X64Select`'s `250.04 ms`):**
- Current `X64Patch` runs at `369.26 ms` (`1.477×`).
- Preserving ALL multi-arg constant fast paths via the epoch-stamped per-block `const_val`/`const_epoch` table, keeping `disp8` homes for the first 16 non-`Unit` slots, and adding sound `rax`-accumulator + `RFLAGS` compare-branch stencils (`-9%` stack loads even on 28-slot mixed `disp32` functions, `-15%` to `-25%` on `<= 16`-slot loops) projects `jetc-trace` runtime into **`~295–345 ms` (`1.18×–1.38×`)**, safely below `1.500×` (`375.06 ms`). If `c4` measurements show any hot loop above `1.38×`, extending the stencil generator with a 1-slot `xmm0` or secondary `rcx` accumulator is a localized `X64Stencils/Main.jet` addition.

---

## 6. Repo Guards, Risks, and Honest Limits (Fixes `F7`)

### 6.1 Guard & Ratchet Compliance

- **`Tests/Guards/closed_lists.jet`:** All new tags (`X64_RELOC_*`, `O0_RT_TAG_*`, `O0_COLD_*`) use plain `U8`/`Int` constants — zero new `>= 3`-variant enums or `[String]` lists. `Compiler/JetIR/Source/X64StencilData.jet` carries the generated-file header (`This file is generated by Compiler/Toolchain/X64Stencils/Main.jet...; do not edit.`), producing zero closed-list findings.
- **`Tools/Agent/CompilerData.jet`:** `X64Patch.jet` stays at `0` `text_keys`, `0` `Shared<` (including inside comments!), and `0` `payload_enums`; `X64Module.jet` stays `<= 13` `Shared<`; deleting `X64RelocTarget` reduces `Emission.jet` `payload_enums` by `1` (ledger lowered via `Tools/Agent/CompilerData.jet --write`).
- **`Tests/Guards/copy_ratchet.jet`:** `0` `.copy()` calls in `Compiler/Toolchain/X64Stencils/**`, `X64StencilData.jet`, and `X64Patch.jet`.
- **`Tests/Guards/acronym_path_case.jet`:** `Compiler/Toolchain/X64Stencils/` (`package.jet`, `Main.jet`) and `Compiler/JetIR/Source/X64StencilData.jet` follow TitleCase + `X64` naming conventions.
- **`UnsafeRatchet`:** `0` new `#Unsafe` blocks anywhere.
- **Path hygiene:** Zero hardcoded host scratch paths; all scratch paths use `$JET_SCRATCH_ROOT/...`.

### 6.2 Honest Limits (What `#6795` Does Not Fix)

- **Downstream object writing and linking:** `ir.object` (`1,865.33 ms` 1t) and `native.link` (`416.84 ms` 1t) sit outside `ir.x64`. The unified `X64FlatReloc` (eliminating per-reloc runtime string hashing in `Object.jet`) and `-4%` smaller `.text` will incidentally help `ir.object`, while full object/link optimization belongs to `#6796` / `#6798`.
- **Front-end / `ir.build`:** `ir.build` (`7,012.77 ms` 1t) and `sema` are separate cards (`#6978`, `#6793`, `#6794`, `#6797`).
- **`-O1` and `-O2` tiers:** `X64Select` remains the backend for `-O1` and `-O2` (migrated cleanly to unified `X64FlatReloc`), preserving identical semantics across all tiers per I9.

---

## 7. Implementation and Verification Plan (`c2`–`c4`)

1. **`c2` (After `c1` adversarial review passes):**
   - Add the 6 missing x86-64 encodings to `Compiler/JetBackend/Source/X64/Encoder.jet`.
   - Implement `Compiler/Toolchain/X64Stencils/` (`package.jet`, `Main.jet`), wire the `x64-stencils` build step in `Compiler/build.jet`, and generate `Compiler/JetIR/Source/X64StencilData.jet`.
   - Execute the greenfield unified `X64FlatReloc` cutover across `Emission.jet`, `Object.jet`, `DirectBind.jet`, `X64Module.jet`, `X64Stack.jet`, `X64Select.jet`, `X64Patch.jet`, and `Compiler/JetDebug/Source/NativeFix.jet`.
   - Rewrite `Compiler/JetIR/Source/X64Patch.jet` around `X64StencilData.jet`, `IRX64FastModule` O(1) tables, 4-pass zero-alloc `ir_drop_plan_into`, epoch-stamped `const` table, sound `rax`-accumulator & `RFLAGS` fusion, fast `debug_info = true` emission, stack-check safety for `> 32 KiB` frames, and zero per-call heap allocations.
   - Add `Tests/Cases/Backend/o0_stencil_emit.case` proving every IR opcode lowers from the toolchain-built stencils compiled into `jet` with zero file I/O per compile.
2. **`c3`:** Run full `S0 -> S1 -> S2 == S3` self-host chain fixed point plus goldens, UI snapshots, and cases on `-O0`, `-O1`, and `-O2` (explicitly verifying `StringsFreeAtTheirLastUse`, `NativeFix`, `jet debug`, `jet inspect memory`, and `Tests/Guards`).
3. **`c4`:** Capture and publish live `#5479 c1` (`Tools/Perf/CompileThroughput/Throughput.jet`) and `jetc-trace` (`Tools/Perf/JetcTrace/Summarize.jet`) receipts proving `>= 10×` `-O0` backend CPU reduction per function and `<= 1.50×` `-O0` runtime speed against the `X64Select` baseline.
