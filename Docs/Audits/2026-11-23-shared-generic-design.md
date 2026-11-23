# Shared Generic Code at `-O0` & Per-Function `> 4 KB` Code Size Audit — Design Note (`#6797`, Rev 2)

**Date:** `2026-11-23` · **Base commit:** `f3b6b8f4f7` · **Card:** `#6797` (`c1`–`c5`) · **Sequencing:** Sequenced after `#6978` / `#6793 S5` (`TypeID` interner) · **Revised to address all 10 findings (`F1`–`F10`) of Adversarial Review 1 (`2d561eea`)**

## 1. Problem, Honest Live Denominators & Runnable Comparison Probe (`f3b6b8f4f7`)

Today `Compiler/JetIR/Source/Build.jet:520–521, 1759–1790` (`ir_function_row_id`) monomorphizes every generic function once per distinct concrete type argument list (`<base>__<16-hex-hash>`) on **all** optimization tiers, including `-O0` (`jet run`, `jet dev`, comptime).

### 1.1 Honest Denominator: Self-Host `jet-bare` (`-O0` vs `-O2`) Monomorphization Share
Inspecting the live ELF `.symtab` of `jet-bare` compiled from `Compiler/JetCLI/run.jet` on `f3b6b8f4f7` (`$JET_SCRATCH_ROOT/tmp/probe_6797/o0_jetbare/run` at `-O0` vs `$JET_SCRATCH_ROOT/tmp/milestone-s2-dist/jet-bare` at `-O2`) separates shareable user generic instantiations (`__<16hex>`) from compiler-synthesized per-type `jet_ir_*` runtime glue (which remains per-type):

| Function Category in `jet-bare -O0` (`115.95 MB` `.text`, `63,354` fns) | Function Count | `.text` Bytes | Share of `.text` |
|---|---:|---:|---:|
| Non-generic user functions | `31,313` | `102,438,619 B` | `88.34%` |
| Other `__`-named functions (module/closure helpers) | `2,237` | `2,842,598 B` | `2.45%` |
| **User generic instantiations (`__<16hex>`)** (`272` multi-copy fns = `1,035,078 B`, **`807,407 B` duplicate excess**) | **`503`** | **`1,790,111 B`** | **`1.54%`** |
| **Per-type synthesized `jet_ir_*` glue** (`drop` `9,932`, `last` `5,612`, `move` `3,903`, `copy` `3,773`, `unique` `2,111`, `map` `1,037`, `lazy` `845`, `render` `726`, `equal` `438`, `closure` `247`, `close` `183`, etc.) | **`28,670`** (`45.3%` of fns!) | **`8,250,496 B`** | **`7.11%`** |
| Runtime symbols (`jet_rt_*`) | `449` | `120,385 B` | `0.10%` |

*(For reference, `-O2` `jet-bare` has `134.77 MB` `.text`, `39,204` functions, and `265` `__<16hex>` user generic functions = `2,173,924 B` / `1.61%` of `.text`, where `-O0` shared generics do not apply.)*

**Honest scope statement:** Because the Jet compiler codebase itself is mostly concrete structs and monomorphic loops, user generics (`__<16hex>`) are only **`1.54%`** of `jet-bare -O0` `.text` (largest multi-copy families: `Collections.compiler_transfer_rows` `54× / 243 KB`, `Collections.compiler_take_rows` `49× / 224 KB`, `EnvPlanBuilder.envmodel_evaluate_env_with_source_loader` `2× / 139 KB`, `core.collections.add` `9× / 26 KB`). Accordingly, **`#6797`'s biggest compile-time and code-size wins are on generic-heavy user programs** (`20–50+` concrete instantiations of Core collections and generic libraries, where `.text` shrinks `62%–79%` below), while `#6795` (copy-and-patch `-O0`), `#6793` (data-oriented core), and `#6794` (parallel pipeline) carry the primary self-host compile-speed gains.

### 1.2 Runnable Comparison Probe: Monomorphized vs Shared-Body Descriptor Dispatch at `-O0` (`N = 20` & `N = 50`)
We compiled and executed an ephemeral Jet comparison probe (`$JET_SCRATCH_ROOT/tmp/probe_6797/compare2/{mono,shared}{20,50}.jet`, 5 cold uncached runs per configuration with `$JET_SCRATCH_ROOT/tmp/milestone-s2-dist/jet build -O0`) implementing a 6-function generic collection pipeline (`box_wrap`, `vec_fill`, `vec_reverse`, `vec_rotate`, `vec_take_stride`, `vec_concat_twice` + `pipeline`) over `N = 20` and `N = 50` distinct 24-byte POD element structs (`Elem0..Elem{N-1}`), comparing (A) current per-type monomorphization against (B) a single shared body per function parameterized by `TypeDesc{stride_words, is_pod}`:

| Variant | Output (exact match) | Cold `-O0` Compile Median `[min, max]` | `.text` Bytes | Executable Bytes | `-O0` Run Median `[min, max]` |
|---|---:|---:|---:|---:|---:|
| **`mono20`** (current) | `143200` | `56.52 ms` `[52.77, 60.37]` | `526,959 B` | `802,888 B` | `3.35 ms` `[3.27, 3.67]` |
| **`shared20`** (1 body) | `143200` | **`46.27 ms`** `[43.41, 48.65]` (**`-18.1%`**) | **`198,543 B`** (**`-62.3%`**, `2.65×` smaller) | **`322,920 B`** (**`-59.8%`**) | `6.18 ms` `[6.04, 7.29]` (**`1.84×`**) |
| **`mono50`** (current) | `418000` | `96.12 ms` `[87.53, 97.82]` | `1,071,919 B` | `1,597,704 B` | `9.02 ms` `[8.90, 9.56]` |
| **`shared50`** (1 body) | `418000` | **`63.78 ms`** `[63.03, 64.72]` (**`-33.6%`**) | **`222,255 B`** (**`-79.3%`**, `4.82×` smaller) | **`363,016 B`** (**`-77.3%`**) | `14.71 ms` `[14.43, 15.01]` (**`1.63×`**) |

*Note on runtime overhead:* An earlier probe calling an indirect closure per word inside inner loops ran `5.08×` slower (`26.82 ms` vs `5.28 ms`), proving that **inline bulk copy on `is_pod` (`memcpy` / word-loop fast path inside `IR_COPY_DESC` & `BuildRoutes.jet`) is mandatory** to keep `-O0` runtime overhead under `<= 1.6×–1.8×` (and `-O1`/`-O2` stay `1.00×` monomorphized!).

## 2. Strongest Existing Implementations (Primary Sources & Trade-Offs)

| System & Primary Source | Mechanism | Trade-Off for Jet |
|---|---|---|
| **Go 1.18 Stenciling + Caller-Emitted Static Dictionaries** (`go.googlesource.com/proposal`, `generics-implementation-dictionaries-go1.18.md`) | Emits one static dictionary per `(generic_fn, concrete_type_args)` holding concrete type descriptors, derived compound-type metadata, method pointers, and child sub-dictionary pointers for nested generic calls `f<T> -> g<Boxed<T>>`. | Eliminates runtime metadata instantiation caches (`Boxed<T>` offsets & nested `g<Boxed<T>>` sub-dicts are statically precomputed by the caller!), **adopted here as `IRGenDict`**. Because Jet has non-uniform value types with deterministic RC/allocator moves (not Go's tracing GC), Jet pairs `IRGenDict` with Swift-style value witnesses (`IRTypeDesc`). |
| **Swift Unspecialized Generics** (`docs/ABI/TypeMetadata.rst`, `docs/Generics.rst` — swiftlang/swift) | One polymorphic body per generic function taking hidden `TypeMetadata*` + protocol witness tables (`PWT`), indirect address passing for generic values, reabstraction thunks for function values, and dynamic stack frames. | Handles arbitrary value sizes without heap boxing and preserves value semantics (`I1`/`I9`), at the cost of indirect witness calls at `-O0` while Jet keeps full monomorphization at `-O1`/`-O2`. |
| **OCaml / Java Erasure** | Uniform boxed 64-bit representation for all generic values. | Violates Jet's value-type ABI and layout reflection (`T.reflect().layout.size`) unless every boundary boxes/unboxes. |
| **Rust / C++ Full Monomorphization** | Specializes every concrete instantiation on all tiers. | Maximum `-O2` runtime speed (which Jet **keeps** on `-O1` and `-O2`), but causes `O(N)` `-O0` compile-time and `.text` growth (`1.07 MB` vs `222 KB` at `N = 50`). |

## 3. Target Architecture for `-O0` Shared Generics (`c2`–`c4`, Resolving `F1`–`F7`, `F9`)

### 3.1 `#6793 S5` (`TypeID`) Integration, `IRTypeDesc`, and Go 1.18-Style `IRGenDict` (`F1`, `F2`, `F4`, `F9`)
- **Sequencing with `#6978` / `#6793 S5` (`TypeID`):** Every concrete type in `IRProgram` has one canonical `TypeID` (`#6978` / `#6793 S5`). `IRTypeDesc` is keyed 1-to-1 by `TypeID` (deduplicated once per `IRModule`), carrying `type_id: U64` so runtime type identity and `IRTypeDesc` share one canonical home (`I8`).
- **Static `IRTypeDesc` (72 bytes, 8-byte aligned, emitted in `.data.rel.ro` / `.data`):**
  ```
  IRTypeDesc (72 bytes, 8-byte aligned):
    +0:  type_id: U64        // canonical #6793 S5 TypeID
    +8:  size_bytes: U64     // byte size of value A (> 0 in V1)
    +16: stride_bytes: U64   // list/array element stride (aligned)
    +24: align_bytes: U64    // power-of-two alignment >= 1
    +32: flags: U64          // bit 0 = is_pod, bit 1 = is_copyable, bit 2 = needs_move_stored, bit 3 = has_close
    +40: copy_fn: Ptr        // fn(dst: Ptr, src: Ptr) -> Unit (0 when is_pod)
    +48: drop_fn: Ptr        // fn(ptr: Ptr) -> Unit (0 when is_pod)
    +56: move_fn: Ptr        // fn(dst: Ptr, src: Ptr) -> Unit — implements ir_move_stored (BuildMove.jet) for #Context(allocator:) safety! (0 when !needs_move_stored)
    +64: close_fn: Ptr       // fn(ptr: Ptr) -> Unit — deterministic Close cleanup (0 when !has_close)
  ```
- **Go 1.18-Style Caller-Emitted Static Dictionary (`IRGenDict`, resolves `F1`):**
  - Because `ir_struct_members_in` (`Compiler/JetIR/Source/BuildTypes.jet:2259–2316`) sorts non-explicit-layout struct fields by **descending alignment** (`jet_target_layout_member_order:2307`), forces aggregate align `8` (`:2304`), and clamps struct size `>= 8` (`ir_struct_row_laid:628`), field offsets of `Boxed<T>` **cannot** be computed from `T`'s `(size, align)` at runtime!
  - Instead, for each concrete call site `(generic_fn, concrete_type_args)`, `IRBuilder` emits one deduplicated static **`IRGenDict`** containing:
    1. Pointers to `IRTypeDesc` for each type parameter `T1..Tk` AND every **derived compound type** (`Boxed<T>`, `(T, Int)`, etc.) constructed or accessed inside `f`.
    2. Precomputed **struct/tuple field-offset tables** (`IRFieldTable`, flat `[U32]` byte offsets computed by `ir_struct_members_in` at compile time) for every parameterized struct/tuple accessed inside `f` (O(1) indexed load for `Boxed<T>.tag`!).
    3. Pointers to `IRTraitWitness` tables for every trait constraint `Ti: Trait` on `f`.
    4. **Child `IRGenDict*` sub-dictionary pointers** for every statically reachable nested generic call `g<Boxed<T>>(...)` inside `f`!
  - **Polymorphic recursion fallback:** If `f<T>` calls `f<Boxed<T>>` (infinite static sub-dictionary cycle), `IRBuilder` detects the recursion cycle during dictionary construction and falls back cleanly to monomorphizing that call graph (sound, zero infinite loop).

### 3.2 Tightened V1 Eligibility & Strict `I9` Boundary (`F2`, `F3`, `F4`, `F7`)
To guarantee 100% `I9` ABI parity (`-O0` shared == `-O1`/`-O2` monomorphized) without breaking non-uniform carrier layouts, V1 defines an **explicit, machine-checked eligibility predicate `ir_generic_shared_eligible(m, fn_decl, concrete_args)`**:
- **Shared at `-O0` (`fast_o0 = true`) in V1:**
  - Generic functions and methods whose type parameters appear as value parameters/returns/locals, `[T]` lists (all uniform 24-byte `{len, cap, ptr}` headers lowered via `BuildRoutes.jet`), `Box<T>` counted boxes, parameterized plain structs (`Boxed<T>`) and tuples whose fields have uniform compile-time-tabulated offsets, and trait-method calls via `IRTraitWitness` thunks.
  - Preserves exact `#Context(allocator:)` store-move semantics (`move_fn` calling `ir_move_stored` from `BuildMove.jet` when `flags & needs_move_stored != 0`, preventing arena-escaping UAF under `I1`!) and exact `Close` semantics (`close_fn`).
- **Explicit V1 monomorphization escape list (zero `I9` drift, zero safety compromise):**
  1. **Non-uniform element-dependent carriers (V1 excluded; V2 witness-extensible):** functions whose generic parameter `T` flows into `Option<T>` (`BuildTypes.jet:298–328, 1640–1675`: 3+ incompatible layouts — inline-null `*U`, inline-`2^62` `Int`/`Bool`/unsigned, and heap-boxed structs/`String`), `Result<T, E>` & payload enums (`BuildOwn.jet:264–282`, `ir_enum_variant_sized`), `Map<K, V>` (`BuildMaps.jet:8–191`: hash trailer vs binary search), `[T#N]` `FixedList` (`BuildTypes.jet:400–413`: inline vs 24-byte heap `List`), SoA `Rows<T>` (`BuildTypes.jet:2285–2289`), recursive boxed edges (`ir_edge_wrapped`, `BuildTypes.jet:2327`), or zero-sized `T` (`Unit`, `Never`, empty structs where `size_bytes == 0`).
  2. **Compile-time type-dependent / intrinsic bodies:** `#if T == ...`, `T.reflect()`, `Core/Mem` layout intrinsics (`ListAppend`/`TypeSize`/`PtrOffset` at `Build.jet:1270–1272`).
  3. **Hard backend/policy exclusions:** `#Inline(Always)` functions, `!Mem.Stack(above: N)` (`f.frame_bound >= 0`), `timing: .Fixed` (`ir_timing_marked(f)`), `f.asm != None`, and `-O1`/`-O2` (full monomorphization retained so `CodegenCeiling 9/9` is 100% untouched!).

### 3.3 Safe Dynamic Stack Frames in `X64Patch.jet` & `X64Select.jet` (`F3`)
Every generic-typed local slot in `f__shared` has a runtime size `desc.size_bytes` that is **loop-invariant** for the entire invocation of `f__shared`:
1. **Entry hoisting (zero per-iteration loop stack leak):** `IRBuilder` emits all `IR_ALLOCA_DESC(dict_slot)` ops strictly in the function entry block (never inside a loop body), so `rsp` decreases **once per call** by `total_dyn_bytes = sum(align_up(desc_i.size_bytes, max(16, desc_i.align_bytes)))` and is restored once on epilogue (`mov rsp, rbp`).
2. **`I1` stack-segment safety (`gs:[1024]` check + grow):** Immediately before `sub rsp, total_dyn_bytes`, emit a runtime stack-segment check `lea r11, [rsp - (16 + fixed_frame + total_dyn_bytes)]; cmp r11, gs:[1024]; jb .grow_dyn` invoking `jet_rt_stack_grow` with the dynamic byte count — matching `X64Patch.jet:4535–4540, 4653–4656` so fiber stack segments can **never** overflow!
3. **Over-aligned slots (`align > 16`):** When `desc.align_bytes > 16`, emit `and rsp, -align` after reserving `size_bytes + align`, storing the aligned slot pointer in a fixed frame slot `[rbp - home_i]`.
4. **Dynamic `sret` in `patch_call`:** Calls inside `f__shared` returning a generic type pass the target `IR_ALLOCA_DESC` pointer (or forwarded `ret_addr`) directly as the hidden `sret` pointer in `rdi`/`rsi` without assuming a constant `sret` byte size.
5. **Stop-edge & early-drop safety (`I1`):** Prologue zero-initializes every dynamic owned slot pointer (`[rbp - home_i] = 0`) and records a `initialized: Bool` flag (or non-null check after allocation/init); `patch_emit_stop_edges` invokes null-guarded `IR_DROP_DESC(ptr, desc)` (skipping `DropEarly.jet` hoisting on generic slots in V1 so lifetime boundaries are trivial and exact!).
6. **DWARF / `jet debug` at `-O0`:** Generic-typed local variables emit `DW_OP_fbreg` + `DW_OP_deref` (pointing through the fixed `[rbp - home_i]` slot address) so `jet debug` inspects live stack addresses cleanly.

### 3.4 Backend & Linker Relocated Descriptors + Dead-Strip Traversal (`F5`)
`IRTypeDesc`, `IRTraitWitness`, and `IRGenDict` hold function pointers and cross-data pointers, requiring three clean, general backend extensions:
1. **Extend `IRDataReloc` (`Compiler/JetIR/Source/IR.jet:545–550`) & `X64DataReloc` (`Compiler/JetBackend/Source/Image/Emission.jet:240–244`):** Generalize `target` from data-index-only `Int` / `X64DataID` to a tagged target (`.Data(id)` / `.Text(id)` / `.Function(fn_id)` / `.Runtime(name)`), updating `X64Module.jet:588–596` and `Object.jet:1150–1164` to emit `ELF64_R_X86_64_64` against the target function's `.text` section symbol + `text_offsets[row]` (or own-section symbol) or undefined runtime symbol.
2. **Relocated read-only placement (`Object.jet:674–676, 1110–1128` & `StaticLink.jet:121`):** Because `x64_object_read_only` restricts `.rodata` to `relocs.len() == 0`, place relocated `IRTypeDesc`/`IRTraitWitness`/`IRGenDict` objects in `.data.rel.ro` (`LINK_RELRO` already defined at `StaticLink.jet:121`, or `.data` under `Object.jet:1124–1128` with `.rela.data` already wired!) so zero `.rela.rodata` hacks are needed.
3. **Fix dead-stripping (`X64Module.jet:1434–1493`, `ir_x64_reached_only`):** Pass `data: &[X64DataObject]` into `ir_x64_reached_only` and maintain a `data_reached: [Bool]` worklist alongside `reached: [Bool]`. Whenever a reached function references `.Data(d)`, mark `d` reached and enqueue all `.Data(d2)` and `.Function(fn_id)` targets inside `data[d].relocs`. This guarantees `copy_fn`, `drop_fn`, `move_fn`, `close_fn`, and trait witness thunks are never dead-stripped!

### 3.5 Exact Closure-Box Adapters & Trait Witness Thunks (`F6`)
- **Function values of `f<Concrete>` (Swift-style reabstraction adapter, NOT a "3-instruction thunk"):** Because Jet function values are counted heap closure boxes (`[count@0, code@8, release@16, captures@24...]` at `BuildCoreClosures.jet:68–161`, `IR.jet:54–59`) where every `CallIndirect` passes `box` in `rdi`, scalar concrete args in registers (`rsi`/`xmm0`), returns in `rax`/`xmm0`, and signals stops in `edx` (`X64Select.jet:4668–4671`), taking a function value of `f<Concrete>` emits a **concrete marshaling adapter** (`~60–100 bytes` vs `~3–10 KB` for a full monomorphized body!) that: allocates stack slots for scalar args/return, stores `rsi`/`xmm0` to stack addresses, loads `&IRGenDict_Concrete`, calls `f__shared(dict, &args..., &ret)`, reloads `rax`/`xmm0` on `!edx`, and returns with `edx` preserved!
- **Trait witness thunks (`IRTraitWitness`):** Concrete trait methods (e.g. `Int.add(self: Int, other: Int) -> Int`) take register values, not indirect pointers. Each `(ConcreteType, Trait, Method)` slot in `IRTraitWitness` points to a tiny generated **witness thunk** (`~40–64 bytes`, deduplicated per `(TypeID, Trait, Method)` across the entire module!) that loads values from `(self_ptr, arg_ptrs...)`, calls the concrete method, stores the return into `ret_ptr`, and propagates `edx`.

### 3.6 `IRBuilder` Symbolic Lowering, List Routes, `UnsafeRatchet` & Pass Coverage (`F7`, `F9`)
- **Symbolic lowering in `IRBuilder` (`Build.jet`, `BuildExpr.jet`, `BuildTypes.jet`):** Add an `IR_TYPE_GENERIC_PARAM(param_idx)` sentinel type row (plus `IR_TYPE_GENERIC_COMPOUND(dict_entry_idx)` for derived structs/tuples) used **only** when lowering an eligible `f__shared` body at `-O0`. Expressions over generic rows emit the 6 explicit descriptor IR ops: `IR_ALLOCA_DESC`, `IR_COPY_DESC` (inline `memcpy` when `flags & is_pod`, else `call desc.copy_fn`), `IR_DROP_DESC`, `IR_MOVE_DESC` (`ir_move_stored`), `IR_FIELD_DESC` (O(1) load from `IRFieldTable`), and `IR_CALL_WITNESS`. Any unsupported expression/coercion (`ir_adapt` at `Build.jet:5238–5497`) triggers eligibility refusal and falls back cleanly to monomorphization!
- **Generic-stride `[T]` list lowering with `0` new `#Unsafe` (`F9`):** List operations (`push`/`index`/`copy`/`try_reserve`/`len`) live in **`Compiler/JetIR/Source/BuildRoutes.jet:44–283`** (NOT nonexistent `JetRuntime/List.jet` or `BuildCalls.jet`!). Methods returning `Option<T>` (`pop`/`first`/`last` at `BuildRoutes.jet:265–283`) stay in the V1 monomorphization fallback via `ir_generic_shared_eligible` until V2 adds `Option<T>` witnesses (`L2`). Generic-stride `[T]` ops lower directly in `BuildRoutes.jet` using the same IR `jet_rt_alloc_8`/`jet_rt_realloc_8`/bounds-check/length ops parameterized by `desc.stride_bytes`, `IR_COPY_DESC`, and `IR_MOVE_DESC` — adding **`0` `#Unsafe` regions** anywhere (`UnsafeRatchet <= 1654`, `0` outside `Compiler/JetRuntime/package.jet:14–16`!).
- **Full pass coverage (`F7`):** Update `Lint.jet` (verify all 6 descriptor opcodes & entry-only `IR_ALLOCA_DESC`), `OptReturnSlot.jet` (skip `retslot` rewriting on `IR_ALLOCA_DESC` slots), `OptInline.jet` (never inline `f__shared` at `-O0`), `DropEarly.jet` (do not hoist `IR_DROP_DESC`), `X64Patch.jet`, AND `X64Select.jet` (so `-O0` fallback under `JET_X64_EMIT_LOG` still lowers all 6 opcodes identically!).

## 4. Per-Function `> 4 KB` Code Size Audit on Tool-Twin Binaries (`#6797 c5`, Live `f3b6b8f4f7` Measurement, `F8`)

Live ELF `.symtab` inspection on `f3b6b8f4f7` across three shipped `-O2` tool-twin binaries (`MemorySafetyFuzz`, `RunScoreboard`, `RunTools`) — plus an exact `-O2` vs `-O2 --inline=off` vs `-O0` A/B attribution on `MemorySafetyFuzz`:

| Tool-Twin Binary (`-O2`, `f3b6b8f4f7`) | Total `.text` | Functions `> 4 KB` | Bytes in `> 4 KB` Fns | Share of `.text` |
|---|---:|---:|---:|---:|
| `MemorySafetyFuzz` | `1,686,287 B` | **`64`** | `1,015,090 B` | **`60.2%`** |
| `RunScoreboard` | `1,290,959 B` | **`44`** | `824,812 B` | **`63.9%`** |
| `RunTools` | `840,943 B` | **`24`** | `319,385 B` | **`38.0%`** |

### Exact Byte Attribution (`MemorySafetyFuzz`: `-O2` `1,686 KB` vs `-O2 --inline=off` `1,453 KB` vs `-O0` `1,691 KB`)
| Function (`> 4 KB`) | `-O2` Size | `-O2 --inline=off` | Inlining Share | `-O0` Size | Root Cause Class |
|---|---:|---:|---:|---:|---|
| `MemorySafetyFuzz.run_memory_safety_fuzz_axis` | `73,118 B` | `70,275 B` | `+2,843 B` (`3.9%`) | `82,630 B` | **Monolithic body + repeated string-interp / drop / error-stop blocks** |
| `MemorySafetyFuzz.run_stages` | `70,033 B` | `66,664 B` | `+3,369 B` (`4.8%`) | `74,042 B` | **Monolithic body + repeated string-interp / drop / error-stop blocks** |
| `MemorySafetyFuzz.hardening_payload` | `45,294 B` | `41,974 B` | `+3,320 B` (`7.3%`) | `62,700 B` | **Repeated inline struct/list/string-builder expansion** |
| `core.regex.scan` | `32,579 B` | `7,970 B` | **`+24,609 B` (`75.5%`)** | `9,606 B` | **Over-inlining of cold/large callees at `-O2`** |
| `__core_encoding_stream.jcs_number` | `27,149 B` | `15,763 B` | **`+11,386 B` (`41.9%`)** | `18,436 B` | **Over-inlining + repeated bounds/format checks** |
| `Tree.read_json` | `26,544 B` | `763 B` | **`+25,781 B` (`97.1%`)** | `1,052 B` | **Over-inlining whole JSON parser into thin wrapper** |
| `core.regex.look_tables` | `25,865 B` | `266 B` | **`+25,599 B` (`99.0%`)** | `388 B` | **Over-inlining + table initialization emitted as code** |
| `core.regex.replace_n` | `25,538 B` | `6,349 B` | **`+19,189 B` (`75.1%`)** | `4,776 B` | **Over-inlining** |
| `core.encoding.json.parse_string` | `21,187 B` | `9,404 B` | **`+11,783 B` (`55.6%`)** | `12,311 B` | **Over-inlining + repeated UTF-8/escape error checks** |

*(In `RunScoreboard -O2`, `Scoreboard.entry_record` `82.2 KB`, `Scoreboard.scoreboard` `75.7 KB`, `Comparison.peer_comparisons` `59.1 KB`, `Scoreboard.peer_record` `52.5 KB`, and `Result.result_shape_issues` `43.7 KB` exhibit the same two root causes.)*

### General Codegen & Core Shrink Rules (S4 — Each Proved on Two Programs Without Hurting `CodegenCeiling 9/9`)
1. **Caller-growth & cold-block inlining budget (`OptInline.jet`):** Cap total caller growth and forbid inlining multi-block callees into cold error/setup paths (`Tree.read_json` `26.5 KB -> ~1 KB`, `core.regex.look_tables` `25.9 KB -> <1 KB`, `core.regex.scan` `32.6 KB -> ~8–10 KB`) while keeping hot inner-loop inlining untouched (`CodegenCeiling 9/9` gate!).
2. **Cold stop/panic/bounds-trap outlining (`X64Select.jet` / `X64Patch.jet`):** Share/deduplicate repeated bounds/overflow/drop cold-stop stubs per function and outline cold string-interpolation assembly (`format` helpers instead of repeating 40+ inline `String` concat/drop sequences inside `70 KB` functions like `run_memory_safety_fuzz_axis` and `Scoreboard.entry_record`).
3. **Static `.rodata` constant tables instead of inline code (`BuildStatic.jet` / `Core/Unicode` / `Core/Regex`):** Lower large constant match/mapping tables (`unit_unicode_upper_mapping`/`lower_mapping`, `core.regex.look_tables`) to `.rodata` lookup tables instead of branch-and-store instruction sequences.

## 5. Ordered Slice Plan & Exact Proofs (Sequenced After `#6978` / `#6793 S5`)

| Slice | Scope & Real Files | Exact Proof |
|---|---|---|
| **S1: Backend Relocated Data & Dead-Strip Fix (`F5`)** | `Compiler/JetIR/Source/IR.jet`, `X64Module.jet`, `Compiler/JetBackend/Source/Image/{Emission,Object,StaticLink}.jet` — add `.Function`/`.Runtime` `IRDataReloc`/`X64DataReloc`, `.data.rel.ro`/`.data` placement, and data-reloc traversal in `ir_x64_reached_only`. | Unit tests in `JetBackend`/`JetIR` + `S2 == S3` fixed point. |
| **S2: `IRTypeDesc`, `IRGenDict`, Symbolic `IRBuilder`, Safe Dynamic Frames, Marshaling Adapters & `BuildRoutes.jet` Stride Ops (`c2`)** | `Compiler/JetIR/Source/{Build,BuildExpr,BuildTypes,BuildRoutes,BuildMove,BuildCoreClosures,Lint,OptReturnSlot,OptInline,DropEarly,X64Patch,X64Select}.jet` — implement V1 eligible shared generic lowering at `-O0` (`fast_o0 = true`), entry-hoisted stack-checked dynamic frames, `IRGenDict` sub-dicts, witness thunks, closure-box reabstraction adapters, and generic-stride `BuildRoutes.jet` (`0` new `#Unsafe`). | `Backend/o0_shared_generic_body` (`c2`) verifying **1** compiled body in ELF `.symtab` for 20 concrete instantiations at `-O0` and identical output across `run`/`O0`/`O1`/`O2`; `UnsafeRatchet <= 1654` (`0` outside kernel); `CodegenCeiling 9/9` unchanged; `S2 == S3` fixed point + `golden`/`ui`/`cases` on every tier (`c3`). |
| **S3: Two-Program `-O0` Compile-Time, `.text` & Runtime Receipt (`c4`)** | Committed cold, uncached receipt comparing monomorphized vs shared `-O0` bodies on (1) 20-element-type Core collections program and (2) second generic-heavy program (+ `jet-bare -O0` delta), reporting compile time, `.text` bytes, and `-O0` runtime ratio. | Committed receipt proving `c4`. |
| **S4: Full Tool-Twin `> 4 KB` Audit & General Shrink Rules (`c5`)** | Complete dated `> 4 KB` breakdown in `Docs/Audits/` across all tool-twin binaries, followed by general `OptInline.jet` cold/growth caps, cold-stop outlining, and `.rodata` table lowering — each proved on two programs with `CodegenCeiling 9/9` + `S2 == S3` + full tests. | Committed dated breakdown + two-program receipts proving `c5`. |
