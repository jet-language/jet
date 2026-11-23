# Adversarial Design Review — `#6795 c1` Copy-and-Patch `-O0` (Pass 2)

- **Date:** 2026-11-23
- **Card:** `#6795` (`c1` design review gate before `c2`)
- **Reviewed Artifacts:**
  - Design note (Rev 2): `Docs/Audits/2026-11-23-copy-and-patch-o0-design.md` (lines 1–286)
  - 5-arm runnable probe: `$JET_SCRATCH_ROOT` scratch `probe_o0_stencils.jet` (lines 1–2017)
- **Overall Verdict:** **PASS** — clears `#6795 c1` and unblocks `#6795 c2` (subject to 4 non-blocking `c2`/`c3` implementation guardrails logged in Section 4).

> **Verification scope note:** Every architectural, encoding, liveness, DWARF/CFI, relocation, build-graph, and guard claim in Rev 2 was verified directly against live repository files (`Compiler/build.jet`, `Compiler/package.jet`, `Compiler/test.jet`, `Compiler/JetCLI/Source/CLI/BuildExecute.jet`, `Compiler/JetIR/Source/X64Patch.jet`, `Compiler/JetIR/Source/X64Select.jet`, `Compiler/JetBackend/Source/Image/Emission.jet`, `Compiler/Toolchain/LexerKeywordRows.jet`, and `Tests/Guards/closed_lists.jet`).

---

## 1. Pass 1 Findings (`F1`–`F7`) Resolution Matrix

| Pass 1 Finding | Rev 2 Resolution | Status | Evidence / Verification |
|---|---|---|---|
| **F1 — `S0 -> S1` bootstrap & `o0.stencils` sandbox/wiring** | Replaced external `o0.stencils` runtime file with build-time generator `Compiler/Toolchain/X64Stencils/Main.jet` emitting `Compiler/JetIR/Source/X64StencilData.jet` (`kept: true`) before `jet-bare`. | **Resolved** | Matches `Compiler/build.jet:84–101` (`prelude-sources`/`lexer-keywords`). `Compiler/build.jet:67` already includes `"JetIR/Source"` in `RUNTIME_TREES`, so `runtime` automatically tracks `X64StencilData.jet` and `bare` orders `x64_stencils` strictly before `runtime`. Compiles directly into `jet-bare`, `jet`, `JetEval`, `JetDebug`, and `jet test Compiler/JetIR` with zero file I/O per compile. |
| **F2 — `DropEarly` (`D-DROP-ASAP1=A`) soundness & toy probe** | Specified full 4-pass zero-alloc `ir_drop_plan_into` (`stop`, `eligible`+`dropped`+`blocked`/`open`, lazy forward-only `last_ref` updated *after* planning each `.Drop` + `ir_drop_target` + `kill`) with exact `any_drop` fast-skip; implemented and benchmarked in `probe_o0_stencils.jet:527–601`. | **Resolved** | Forward lazy update (`probe_o0_stencils.jet:569, 589`) preserves exact multi-`Drop` semantics (`StringsFreeAtTheirLastUse.fixture/main.jet:48–49`) and all 4 safety guards with zero heap allocation. |
| **F3 — `rax` acc, `RFLAGS` fusion, 1-slot `is_const`** | Restricted `RFLAGS` fusion to exact-`Int`, `Word`, and ordered `Float`/`Float32` `<`/`<=`/`>`/`>=`; forbade `Float`/`Float32` `Eq`/`Ne`, `String` `Eq`/`Ne`, and `Not`; patched real `flags_inv_cc`; excluded `Rem`/`Mod`/`Float`/cold-drop `.Route` from `rax` acc; replaced 1-slot cache with O(1) epoch-stamped per-block `const_val`/`const_epoch`. | **Resolved** | Verified live in `X64Patch.jet:2165, 2185–2187` (Float `<`/`<=`/`>`/`>=` swap operands to emit `seta`/`setae`, which are NaN-safe for `RFLAGS` fusion) and `X64Patch.jet:3989–4002` (`.Compare` cold stubs end with `cmp rax, 0; setcc; movzx; mov; jmp`, preserving signed flags). |
| **F4 — Liveness removal, stack safety, missing `IROp`s, `Encoder.jet` gaps** | Replaced `p.starts`/`p.ends` in cleanup-edge helpers; kept all `X64Select.jet:5567` fallback predicates; added `frame_bytes > 32768` stack-check rule; canonical ~185 micro-stencil factoring; enumerated all 6 missing `Encoder.jet` helpers + special ops. | **Resolved (with Guardrails G1–G3)** | Two additional `p.starts`/`p.ends` sites (`X64Patch.jet:3427`, `3731/3735`), inline `#FFI(asm)` call sites (`X64Patch.jet:3136–3158`), and large `StackAddress` frame pre-summing are logged as concrete `c2` guardrails below — none alter the architecture. |
| **F5 — `debug_info == true` default, `.eh_frame`, `X64FlatReloc` consumers** | Shared constant `STD_CFI` always emitted (even when `!debug_info`); omit `-O0` `X64CallSite`/`X64CallArgument` entry-value tracking while preserving `X64LineRow{call: true}` and O(1) fixed-frame variable ranges; full greenfield unified `X64FlatReloc` cutover across all 8 consumers (deleting `X64RelocTarget`). | **Resolved** | Matches `BuildOptions.jet:76, 88`, `Dwarf.jet:1866`, `DirectBind.jet:139`, `X64Stack.jet:120, 156`, and `NativeFix.jet:287, 427, 445, 477`. Eliminating `X64RelocTarget` satisfies Greenfield cutover / I8 cleanly. |
| **F6 — Probe fairness & realistic budget** | Replaced 2-arm probe with honest 5-arm probe (`A`, `A2`, `B1`, `B2`, `B3`) including `debug_info=true`, 4-pass `DropEarly`, 28 slots `disp8+disp32`, structured CF back-patching, realistic 30% chaining / 50% fusion, and `134–148 MiB` materialized `.text`; explicitly attributed `A -> A2` vs `A2 -> B3`. | **Resolved** | Honest attribution (~95% compile speedup from `A -> A2`, +2–3% compile speedup + `-4%` `.text` + `-9%` stack loads from `B3` over `A2`, while rejecting `B1`/`B2` at `1.72×–2.00×` slower). Full 1t `ir.x64` budget (`~458–577 ms`, `12.9×–16.3×`) retains 23%–39% headroom below `745.7 ms` (`10×`). |
| **F7 — Repo guards & hygiene** | Plain `U8`/`Int` tags (`closed_lists.jet`), generated-file header on `X64StencilData.jet`, `0` `text_keys`/`Shared<`/`payload_enums` in `X64Patch.jet`, `0` `.copy()`, `0` `#Unsafe`, `$JET_SCRATCH_ROOT` hygiene. | **Resolved** | Verified `Tests/Guards/closed_lists.jet:109–113` skips files whose first 5 lines contain `"do not edit"` / `"generated by"`. |

---

## 2. Claim-by-Claim Verdict Table (Rev 2)

| Rev 2 Claim | Section | Verdict | Notes |
|---|---|---|---|
| `X64Stencils/Main.jet` -> `X64StencilData.jet` cleanly embeds stencils in `JetIR` with zero circularity & zero file I/O per compile | §4.1 | **holds** | Directly follows `Compiler/build.jet`'s `kept: true` generator pattern; `JetIR/Source` is already in `RUNTIME_TREES` (`build.jet:67`). |
| 6 `Encoder.jet` helpers close 100% of required x86-64 encodings with zero hand-coded hex | §4.2 | **holds** | Verified against full `Encoder.jet` (lines 1–1389). |
| ~185 micro-stencils (~2.8 KB) cover all 52 `IROp`s + special runtime/ABI paths without `disp8 × disp32` blowup | §4.3 | **holds only within stated limits** | Sound factoring; keep the 20-line inline `#FFI(asm)` caller branch (`X64Patch.jet:3136–3158`) intact (Guardrail G2). |
| 4-pass zero-alloc `ir_drop_plan_into` is bit-for-bit equivalent to `DropEarly.jet` (`D-DROP-ASAP1=A`) | §4.4 | **holds** | Lazy forward `last_ref` + all 4 safety guards match `DropEarly.jet:128–239`. |
| `RFLAGS` fusion, `rax` acc, and epoch-stamped `const` table are 100% sound and protect `jetc-trace` runtime (`<= 1.50×`) | §4.5 | **holds** | Verified against `X64Patch.jet:2165–2193, 3934–4002`. |
| Zero-liveness frame layout + `frame_bytes > 32768` stack check + stop-edge helper replacements remove all 5 liveness passes cleanly | §4.6 | **holds only within stated limits** | Also replace `p.ends[stopped]` at `X64Patch.jet:3427` and `p.starts[a]`/`p.starts[b]` at `X64Patch.jet:3731, 3735`, and include `StackAddress` bytes in the `> 32768` check (Guardrails G1 & G3). |
| `STD_CFI` + omitting `-O0` `X64CallSite`/`X64CallArgument` + unified `X64FlatReloc` keeps `.eh_frame`, `.debug_jetline`, `jet debug`, and `jet inspect memory` intact | §4.7 | **holds** | All 8 consumers accounted for; verify `jet debug` parameter inspection on `-O0` frames in `c3`. |
| 5-arm probe (`360 ms` Arm B3) & `~458–577 ms` 1t `ir.x64` budget (`12.9×–16.3×` vs `X64Select`) | §5.1–5.2 | **holds** | Fair 5-arm comparison with honest attribution; watch `#Inline(Always)` inliner budget on `X64Patch.jet` under `inline_census` (Guardrail G4). |
| Guard & ratchet compliance (`closed_lists`, `CompilerData`, `copy_ratchet`, `acronym_path_case`, `UnsafeRatchet`) | §6.1 | **holds** | Verified against live guard implementations. |

---

## 3. Assessment of Arm B3 Against `#6795 c2`

**Question:** Does Arm B3's toolchain-generated `X64StencilData.jet` satisfy `#6795 c2` ("build the stencils with the toolchain at build time and pack them into the compiler; emission copies bytes and patches offsets") cleanly?

**Verdict: YES — cleanly and idiomatically.**
1. **Toolchain-built at build time:** `Compiler/Toolchain/X64Stencils/Main.jet` runs as an explicit `Compiler/build.jet` step (`x64-stencils`, `--write`/`--check`), invoking `JetBackend/Source/X64/Encoder.jet` as the sole x86-64 encoding authority to pre-encode every stencil at compiler build time.
2. **Packed into the compiler with zero file I/O per compile:** Generating `Compiler/JetIR/Source/X64StencilData.jet` compiles the pre-encoded stencils directly into `JetIR`'s binary image across `jet-bare`, `jet`, `JetEval`, `JetDebug`, and `jet test Compiler/JetIR` (mirroring both Jet's own `KeywordSpans.jet` generator in `Compiler/build.jet` and CPython 3.13's PEP 744 build-time-generated `jit_stencils.h`).
3. **Why compile-time-unrolled stampers are the right safe-Jet realization of "copies bytes and patches offsets":** In C/C++ copy-and-patch, `memcpy(dst, stencil, N)` compiles to unrolled constant-byte/word stores because `N` is a compile-time constant. The 5-arm probe proved empirically that under Jet's `0` new `#Unsafe` constraint, a runtime-length byte loop (`Arm B1`, `743.5 ms`) is **2.0× slower** and per-op slice `extend` (`Arm B2`, `639.5 ms`) is **1.72× slower** than direct writes (`Arm A2`, `372.0 ms`). Generating unrolled `#Inline(Always)` stampers (`Arm B3`, `360.0 ms`) from `Encoder.jet` achieves the exact same machine-level behavior as constant-length `memcpy` + hole patching while remaining 100% safe Jet and strictly faster than `Arm A2`.
4. **Proof bar for `c2`:** `X64Stencils/Main.jet` emits `O0_STENCIL_COUNT`, `O0_STENCIL_BYTES`, and `O0_STENCIL_DIGEST` from the exact `Encoder.jet` byte sequences, and `Tests/Cases/Backend/o0_stencil_emit.case` (plus a `#Test` in `Compiler/Toolchain/X64Stencils/`) verifies that every stamper emits the exact `Encoder.jet`-encoded bytes with patched holes.

---

## 4. Non-Blocking Implementation Guardrails for `#6795 c2` / `c3`

These 4 concrete items do not block `c1` sign-off, but must be handled during `c2`/`c3`:

- **G1 — Complete `p.starts`/`p.ends` removal in `X64Patch.jet`:** In addition to `patch_cleanup_edge` (`3536`) and `patch_group_slot_eq` (`3564–3565`), also replace:
  - `X64Patch.jet:3427` (`patch_stop_point`: `p.ends[stopped] != branch`) — replace with a local check confirming `stopped` is a temp not read after `branch`.
  - `X64Patch.jet:3731, 3735` (`patch_emit_stop_edges`: `p.starts[a]` / `p.starts[f.b[u]]` in `info.first..<start_k`) — replace with a local `edge_def[slot]` scratch array populated while scanning `info.first..<info.leave`.
  - In `patch_cleanup_edge` (`3536`), track `written_epoch[slot]` lazily during forward emission so `written_epoch[token] != fn_epoch` proves `token` was not written before `first`.
- **G2 — Preserve inline `#FFI(asm)` call sites (`X64Patch.jet:3136–3158`):** `X64Select.jet:5567` routes asm *definitions* (`f.asm != None`) to `X64Select`, but a normal caller (`f.asm == None`) invoking an inlinable asm leaf still executes `X64Patch.jet:3136–3158`. Keep that 20-line inlining branch intact in `X64Patch.jet`.
- **G3 — Include `StackAddress` bytes in the `> 32768` stack-check trigger:** A function with few slots but a large local aggregate `StackAddress` can still exceed `32 KiB`; trigger the stack check whenever `f.stack_check || 8 * non_unit_slots + stack_addr_bytes > 32768` (or reserve the 24-byte stack-check header whenever `StackAddress` ops exist and patch/skip once final `frame_bytes` is known).
- **G4 — Verify `inline_census` and `LaneCheck` wiring:**
  - `Compiler/test.jet:858–982` (`inline_census`) requires **0** remaining `#Inline(Always)` calls in `Compiler/JetCLI/run.jet` at `-O0`, `-O1`, and `-O2`. Keep `X64StencilData.jet` `#Inline(Always)` stampers flat/non-recursive and confirm `X64Patch.jet`'s own compile time stays fast (use plain non-`#Inline(Always)` stampers for rare/large cold-stub stencils if needed).
  - Register `x64-stencils --check` in `Tools/Agent/LaneCheck.jet` alongside `PreludeSources.jet` and `LexerKeywordRows.jet` so hand edits to `X64StencilData.jet` fail fast.
  - Run `jet debug` tests in `c3` to confirm omitting `X64CallSite`/`X64CallArgument` entry-value tracking at `-O0` preserves parameter inspection on fixed frame slots.
