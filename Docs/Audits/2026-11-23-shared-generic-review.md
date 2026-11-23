# Adversarial Design Review: Shared Generic Code at `-O0` & Per-Function `> 4 KB` Code Size Audit (`#6797 c1`)

**Date:** `2026-11-23`  
**Design reviewed:** `Docs/Audits/2026-11-23-shared-generic-design.md` (Rev 2, base `f3b6b8f4f7`)  
**Card:** `#6797` (`c1`–`c5`) · **Authority:** `AGENTS.md` ("Adversarial design review", "Invariants" I1/I2/I8/I9, Personal-Info guard, `UnsafeRatchet`), `#6797` (`Tools/Tower/.tower/tower.json:52773–52850`), `#6978` / `#6793 S5`  
**Review passes:** Pass 1 failed Rev 1 on 10 concrete defects (`F1`–`F10`: 6 blockers, 4 majors); Pass 2 independently verified every revision in Rev 2 against live repository source on `f3b6b8f4f7`.  
**Reviewer tool scope:** Read-only file inspection (no shell execution tool in this subagent). Every file path, symbol, ABI, object-format, and pass invariant was verified against live source; Rev 2's reported probe timings and `.symtab` counts were audited for methodology and completeness and remain gated on committed receipts in S3/S4 (`L5`).

---

## Verdict: **PASS — Holds Within Stated Limits**

Rev 2 resolves all ten findings (`F1`–`F10`) from Pass 1. Every cited file path, line range, ABI constraint, relocation mechanism, and optimization pass matches live code on `f3b6b8f4f7`. The design is sound to build in S1→S4 order after `#6978` / `#6793 S5` (`TypeID`), **subject to the five binding limits and gates recorded in §3 below** (none of which may be silently bypassed).

---

## 1. Strongest Existing Solutions — Fair Best Case & What Jet Gives Up

| Peer & Source | Strongest Case | What It Does That This Design Does Not |
|---|---|---|
| **Go 1.18 Stenciling + Caller-Emitted Dictionaries** (`generics-implementation-dictionaries-go1.18.md`) | Stenciled bodies parameterized by static `.rodata`/relocated dictionaries holding concrete type descriptors, derived compound metadata, method pointers, and child sub-dictionaries for nested calls `f<T> -> g<Boxed<T>>`. | Go relies on a tracing GC and uniform pointer shapes across gc-shape classes, whereas Jet has non-uniform value-type structs with deterministic RC, `#Context(allocator:)` escape moves (`BuildMove.jet`), and consuming `Close`. Jet adopts Go's static `IRGenDict` + sub-dictionary architecture and pairs it with Swift-style value witnesses (`IRTypeDesc`). |
| **Swift Unspecialized Generics** (`docs/ABI/TypeMetadata.rst`, `docs/Generics.rst`) | Polymorphic body + `TypeMetadata*` + protocol witness tables (`PWT`), dynamic stack frames, runtime metadata instantiation caches, extra-inhabitant enum witnesses (`getEnumTagSinglePayload`), and reabstraction thunks. | Swift handles polymorphic recursion and niche `Option<T>`/`Result<T, E>` layouts inside unspecialized bodies via runtime metadata caches and single-payload enum witnesses. Jet V1 avoids runtime metadata allocation by using static `IRGenDict` with cycle-detected monomorphization fallback for polymorphic recursion and explicit V1 monomorphization fallback for non-uniform carriers (`Option<T>`, `Result<T, E>`, `Map<K, V>`, `[T#N]`, `Rows<T>`, ZSTs), extensible via witnesses in V2. |
| **OCaml / Java Erasure** | Uniform 64-bit boxed representation for all generic values. | Eliminates dynamic stack frames and field-offset tables, at the cost of heap boxing/unboxing at value-type boundaries. Jet preserves unboxed value semantics across all tiers (`I9`). |
| **Rust / C++ Full Monomorphization** | Specializes every concrete instantiation on all tiers. | Maximum runtime speed everywhere (`1.00×` vs `1.63×–1.84×` measured `-O0` overhead on the shared probe), which Jet **keeps** on `-O1` and `-O2` (`CodegenCeiling 9/9` untouched) while using shared bodies only on `-O0` (`fast_o0 = true`). |

---

## 2. Claim-by-Claim Verification Against Live Code

| Claim / Section | Live Source & Pass 2 Verification | Pass 2 Verdict |
|---|---|---|
| **§1.1 Honest `-O0` vs `-O2` `jet-bare` `.symtab` denominator** (`F8`) | Separates user generic instantiations (`__<16hex>`: `503` fns, `1.79 MB` = `1.54%` of `115.95 MB` `-O0` `.text`) from per-type `jet_ir_*` glue (`28,670` fns = `45.3%` of fns, `8.25 MB` = `7.11%`) and states honestly that `-O2` `jet-bare` (`134.77 MB`) is unaffected and `#6797`'s primary compile-speed/size gains land on generic-heavy user codebases (`20–50+` instantiations). | **Holds within stated limits** *(see Limit L5)* |
| **§1.2 Runnable mono vs shared comparison probe (`N = 20`, `50`)** (`F8`) | Compares monomorphized vs shared-body descriptor dispatch with 5-run cold uncached medians/spreads, reporting exact output match, compile-time wins (`-18.1%` / `-33.6%`), `.text` wins (`-62.3%` / `-79.3%`), and `-O0` runtime trade-off (`1.84×` / `1.63×`), plus empirical justification for the inline `is_pod` bulk-copy fast path (`5.08× -> 1.63×–1.84×`). | **Holds within stated limits** *(see Limit L5)* |
| **§3.1 `#6793 S5` `TypeID`, 72-byte `IRTypeDesc`, and Go 1.18-style `IRGenDict`** (`F1`, `F4`, `F9`) | Keys `IRTypeDesc` 1-to-1 by `#6978`/`#6793 S5` `TypeID` (`I8`), adds `move_fn` (`BuildMove.jet` `ir_move_stored` for `#Context(allocator:)` `I1` safety), `close_fn` (deterministic `Close`), and `is_copyable` flag. `IRGenDict` precomputes `IRFieldTable` offsets matching `ir_struct_members_in` alignment-descending layout (`BuildTypes.jet:628, 2259–2316`) and passes child sub-dictionaries for nested generic calls (`f<T> -> g<Boxed<T>>`) with cycle-detected monomorphization fallback for polymorphic recursion. | **Holds** |
| **§3.2 Tightened V1 Eligibility & `I9` Boundary** (`F2`, `F3`, `F4`) | Machine-checked `ir_generic_shared_eligible` excludes all non-uniform carriers (`Option<T>` `BuildTypes.jet:298–328, 1640–1675`; `Result<T, E>` & payload enums `BuildOwn.jet:264–282`; `Map<K, V>` `BuildMaps.jet:8–191`; `[T#N]` `BuildTypes.jet:400–413`; `Rows<T>` `BuildTypes.jet:2285–2289`; `ir_edge_wrapped` `BuildTypes.jet:2327`; zero-sized `T`), compile-time type branches/intrinsics (`Build.jet:1270–1272`), `#Inline(Always)`, `!Mem.Stack(above: N)` (`X64Select.jet:5567`, `X64Stack.jet:119, 192`), `timing: .Fixed`, `f.asm != None`, and `-O1`/`-O2`. | **Holds within stated limits** *(see Limit L2)* |
| **§3.3 Safe Dynamic Stack Frames in `X64Patch.jet` & `X64Select.jet`** (`F3`) | Entry-hoisted `IR_ALLOCA_DESC` eliminates per-iteration loop stack leaks; `and rsp, -align` fixes `align > 16`; dynamic `sret` avoids static `p.depth` collisions; prologue zero-init + null-guarded `IR_DROP_DESC` + skipping `DropEarly` prevents stop-edge UB (`I1`); `indirect: true` (`Emission.jet:209–216`) maps to `DW_OP_fbreg` + `DW_OP_deref`. | **Holds within stated limits** *(see Limits L1, L3)* |
| **§3.4 Relocated Data Descriptors & Dead-Strip Fix** (`F5`) | Correctly targets `IRDataReloc` (`IR.jet:545–550`), `X64DataReloc` (`Emission.jet:240–244`), `X64Module.jet:588–596`, `Object.jet:674–676, 1110–1165`, `StaticLink.jet:121` (`LINK_RELRO`), and `ir_x64_reached_only` (`X64Module.jet:1434–1493`) data-reloc traversal. | **Holds within stated limits** *(see Limit L4)* |
| **§3.5 Closure-Box Reabstraction Adapters & Witness Thunks** (`F6`) | Replaces broken "3-instruction thunk" with concrete marshaling adapters (`~60–100 B`) matching Jet's closure-box ABI (`BuildCoreClosures.jet:68–161`, `IR.jet:54–59`, `X64Select.jet:4668–4671` `edx` stop flag), and replaces direct trait method pointers with deduplicated `(TypeID, Trait, Method)` witness thunks (`~40–64 B`). | **Holds** |
| **§3.6 Symbolic `IRBuilder`, `BuildRoutes.jet` (`0` new `#Unsafe`), & Pass Coverage** (`F7`, `F9`) | Verified every named file exists (`BuildRoutes.jet`, `BuildMove.jet`, `BuildStatic.jet`, `Lint.jet`, `OptReturnSlot.jet`, `OptInline.jet`, `DropEarly.jet`, `X64Patch.jet`, `X64Select.jet`). Lowering generic-stride `[T]` ops in `BuildRoutes.jet` adds `0` `#Unsafe` regions (`0` outside `Compiler/JetRuntime/package.jet:14–16`). Unsupported expressions/`ir_adapt` cleanly fall back to monomorphization. | **Holds within stated limits** *(see Limit L2)* |
| **§4 Live `> 4 KB` Tool-Twin Audit & Shrink Rules (`#6797 c5`)** (`F8`) | Supplies live `f3b6b8f4f7` `.symtab` measurements across `MemorySafetyFuzz` (`60.2%`), `RunScoreboard` (`63.9%`), and `RunTools` (`38.0%`) with `-O2` vs `-O2 --inline=off` vs `-O0` attribution proving two concrete root causes (over-inlining `41.9%–99.0%` on `core.regex.*`/`Tree.read_json`/`json.parse_string`, and monolithic repeated string-interp/drop/stop sequences on `70 KB` bodies) plus 3 general shrink rules in `OptInline.jet`, `X64Select.jet`/`X64Patch.jet`, and `BuildStatic.jet`. | **Holds within stated limits** *(see Limit L5)* |
| **Hygiene & Sequencing (`F9`, `F10`)** | Zero `/mnt/jetscratch` paths remain (`$JET_SCRATCH_ROOT` used everywhere); explicit `#6978` / `#6793 S5` sequencing; real file paths throughout §5 S1–S4. | **Holds** |

---

## 3. Attacks Still Live (Stated Limits & Non-Negotiable Gates)

Each limit below is binding on S1–S4 implementation:

- **L1 — Pre-prologue stack-segment check & grow ordering in `X64Patch.jet` / `X64Select.jet` (S2):**
  In `Compiler/JetIR/Source/X64Patch.jet:4126–4253, 4535–4547`, `PATCH_STACK_CHECK` runs **before `PATCH_PROLOGUE` (`push rbp; mov rbp, rsp`)**, and `patch_stack_grow` saves all incoming argument registers (`rdi..r9, xmm0..7`), copies `stacked` stack arguments to the new segment, and re-invokes the function from its entry (`call row` at line `4188`). Therefore, for `f__shared`, `total_dyn_bytes` MUST be summed from `dict_ptr` (`rdi`, or `rsi` if `sret` occupies `rdi`) into a caller-saved scratch register (`r10`/`r11`) **before `PATCH_PROLOGUE`**, checked against `gs:[1024]`, and passed dynamically into the grow stub (never after `push rbp; mov rbp, rsp`, where `patch_stack_grow` cannot re-enter cleanly!). Likewise, dynamic `sret` inside `patch_call` (`X64Patch.jet:1365–1375, 1421`) must forward an already-allocated `IR_ALLOCA_DESC` pointer or `ret_addr` without mutating static `p.depth` or corrupting `pushed_bytes`.
- **L2 — Exclude `xs.pop()` from V1 generic `[T]` lowering & verify `Box<T>` box-size/checked-alloc accounting (S2):**
  `BuildRoutes.jet:265–283` (`ir_list_pop`) constructs and returns **`Option<T>`**, which `§3.2` explicitly excludes from V1 (clarified in Rev 2 before commit). S2 MUST keep `xs.pop()` (and `first`/`last`) in the V1 monomorphization fallback via `ir_generic_shared_eligible` until V2 adds `Option<T>` witnesses. For `Box<T>` in V1, allocation MUST match `ir_box_bytes` (`BuildOwn.jet:389–400`, 8-byte aligned payload size) and preserve `--checked-alloc` type-row attribution (`ir_alloc_row_of`).
- **L3 — `jet debug` / DWARF type fidelity for generic locals at `-O0` (S2):**
  `X64DebugVariable` (`Compiler/JetBackend/Source/Image/Emission.jet:209–216`) supports `indirect: true` (`DW_OP_fbreg` + `DW_OP_deref`), giving `jet debug` the live stack address of dynamic locals. However, `X64DebugVariable.ty` is a static `X64ModuleDebug.types` index, so symbolic `T` locals in `f__shared` have `X64DebugTypeKind.Opaque` in DWARF (no concrete struct fields unless `jet debug` decodes `IRTypeDesc` or `--debug` monomorphizes). Document this boundary explicitly when `#6841` (`jet debug` `print`/`watch`) exercises generic functions at `-O0`.
- **L4 — `Object.jet` `own_sections` mode & cyclic data reachability in `ir_x64_reached_only` (S1):**
  `Compiler/JetBackend/Source/Image/Object.jet:1134–1138, 1147–1164` has two emission modes (merged sections and `own_sections`, where `data_rela_ends` slices per-object relocations). S1's tagged `X64DataReloc` (`.Data`, `.Function`, `.Runtime`) and `.data.rel.ro` / `.data` placement MUST work identically in **both** modes (covering both `jet run` in-memory images and `jet build` executables, `I9`). `ir_x64_reached_only` (`X64Module.jet:1434–1493`) MUST guard `data_reached[d]` before enqueuing so cyclic data/function references terminate cleanly, and verify via unit test that a `copy_fn`/`drop_fn`/witness referenced *only* from `IRTypeDesc`/`IRTraitWitness` survives dead-stripping.
- **L5 — Independent execution proofs & non-regression gates (S2–S4):**
  Because this Pass 2 review verified live code without shell execution, S2–S4 MUST commit runnable proofs for all numeric claims: (a) `Backend/o0_shared_generic_body` (`c2`) checking ELF `.symtab` == `1` body for `20` concrete instantiations + `run`/`O0`/`O1`/`O2` parity (`c3`), (b) `S2 == S3` SHA-256 fixed point + `UnsafeRatchet` (`0` new `#Unsafe`, `0` outside `Compiler/JetRuntime/package.jet:14–16`), (c) committed cold uncached two-program receipt (`c4`) on V1-eligible 20-type Core collections + second generic-heavy program (+ `jet-bare -O0` delta), and (d) full dated tool-twin `> 4 KB` breakdown + two-program receipts for each S4 shrink rule with `CodegenCeiling 9/9` strictly preserved (`c5`).

---

## 4. Open Follow-Ups & Tracking

| Item | Tracking Home |
|---|---|
| Record §5 S1–S4 slice plan and Limits `L1`–`L5` on card `#6797` (`plan` / `c1`) | Tower `#6797` |
| Prerequisite `TypeID` interner sequencing | Tower `#6978` / `#6793 S5` (existing) |
| V2 witness extension for `Option<T>`, `Result<T, E>`, `Map<K, V>`, `[T#N]`, `Rows<T>` if profiling shows user generics blocked by V1 eligibility | Follow-up criterion on `#6797` after S3 receipt |
| `IRTypeDesc`-aware generic local decoding in `jet debug` (`L3`) | Tower `#6841` (existing) |
