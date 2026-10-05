# Compiler and Core library boundary: evidence for D-CORE-BOUNDARY1 (2026-10-04)

This dated research supports the owner question on card #4565 and ballot
**D-CORE-BOUNDARY1**. It does not ratify anything or own any work; Tower owns
the plan. Every web source below was read on **2026-10-04**. The only local
measurements are a Rust hello-world check and build in section 5.3. Nothing was
built or run against the Jet compiler; Jet numbers come from earlier dated
audits that are cited by path.

Owner request (2026-10-04, verbatim): "card 3983 concerns me -> taking
inspiration from mojo, we want as much as possible of the compiler to be driven
by corelibs so that we can add things like hardware support with a pr rather
than a full new compiler release. the Core/*.jet is where I want our core
library to live. i want performance to be flawless and excellent structure to
the corelibs and dynamically loading the corelibs when they are actually used.
exactly where the boundary betweeen compiler and corelib is up for debate, you
can research write a report and open a ballot for my review to recommend the
best boundary."

## Plain summary

1. Most of the request is already ratified law. D-CORE-SOURCE-AUTHORITY1=A says
   Core behavior is ordinary Jet source and only "unexpressible operations" stay
   in a small audited kernel. The owner's 2026-09-28 direction on #3690 says
   Rust-native Core code is phased out. Reachable-only emission and start-on-use
   services are also law (D-CORE-SOURCE-AUTHORITY1, D-EFF-CORE-INIT1), and
   compile-once Core is law (D-EXEC1). Only two things are open: where hardware
   knowledge lives, and whether Core ships as its own bundle so that a library
   pull request can reach users without a compiler release.
2. Today the code is nearly the opposite of that law. The compiler crate embeds
   about 223k lines of Rust runtime ("Prelude" and "CoreLib"). It also embeds all
   110 Core source files as text (`include_str!`), hard-codes 220 built-in
   collection and string operations, and carries 1,757 Core-call routing rows.
   Core/*.jet holds 40.6k lines, much of it thin wrappers over those rows. Every
   kind of hardware or platform change, and every Core edit, therefore needs a
   new compiler binary.
3. Every surveyed language keeps a closed kernel of roughly 90–270 primitive
   operations in the compiler: Julia 92, Zig about 121, Rust 205, Swift 264 and
   Odin 266. The library is built on top. Mojo, Swift and Carbon define even
   `Int` in the library; Julia and Nim define integer arithmetic there.
   Targets stay in the compiler everywhere: triples, register kinds, calling
   conventions and object formats.
4. Hardware reaches users without a compiler release in two ways. The first is a
   generic "name any LLVM operation" door: Mojo `llvm_intrinsic`, Swift
   `Builtin.int_*`, Julia `llvmcall`, Terra `terralib.intrinsic`, Rust
   `link_name = "llvm.…"`. The second is library code over portable lane
   operations plus a per-function target-feature attribute, as in Odin. Rust
   shows the trap: its AVX-512 intrinsics live in the library, but the feature
   names live in the compiler. Stable AVX-512 therefore still waited for the
   Rust 1.89 release.
5. Jet differs from those languages in one way that decides the question. Under
   D-EXEC1, Jet's own code generator runs the fast-start and default levels
   (O0/O1), and LLVM serves only O2. An LLVM-only door would give new
   instructions to release builds alone.
6. Recommendation: **a small compiler kernel, plus a typed hardware catalog
   written in Core**. Each catalog entry names one machine instruction. It
   carries its checked Jet signature, the chip feature it needs, the encoding
   for Jet's own code generator, the operation LLVM uses, and a plain-Jet body
   that defines its meaning. Core ships as its own signed bundle, versioned
   against a numbered kernel. The compiler reads only the parts a program
   uses, from memory-mapped interface records. New instructions, CPU levels,
   OS adapters, allocators and GPU libraries then arrive by Core pull request.
   A compiler change is still needed only for a new kind of register, a new
   target triple, a new calling convention, or a new kernel operation.
7. Performance rule: kernel operations, including the exact-`Int` fast path,
   bounds checks and lane math, are expanded inline at every level, as
   operators are today. Other Core bodies are compiled once (D-EXEC1). A Core
   function made only of kernel operations is expanded at the call site.

## 1. What is already decided, and what is open

| Topic | Status | Source |
|---|---|---|
| Core behavior is Jet source; only unexpressible operations stay in an audited kernel; the toolchain carries source and verified package metadata | Ratified | D-CORE-SOURCE-AUTHORITY1=A |
| Phase out Rust-native Core; Rust emission stays for now | Owner direction 2026-09-28 | #3690 body |
| Emit only reachable Core; start Core services only on use | Ratified | D-CORE-SOURCE-AUTHORITY1=A, D-EFF-CORE-INIT1=A |
| One runtime; Prelude and Core compiled once at O2; every level calls the same compiled functions | Ratified | D-EXEC1=A |
| Caller-specific Core specialization limited to thin layout/drop/format adapters | Recommended reading, withdrawn ballot | `Docs/research/efficiency-architecture-2026-10-02.md:297-302,735-741`; `~/.cache/jet-luna/ballots/rev2/D-EFF-CORE-SPECIALIZE1.withdraw.md` |
| One MIR lowering that every back end reads | Ratified | D-TIER-ONEIR1=A, D-TIER-FORM1=A |
| Source is the authority; a linked artifact is a cache keyed by full identity | Ratified | D-FREESTAND-ARTIFACT1=C |
| Release key signs runtime and Core prebuilt objects | Ratified | D-BUILD-PREBUILT1=D |
| Native tools private to bundled Core; outside providers admitted by the owner | Ratified | D-RT-NATIVE-CAP1=A, D-UNSAFE-DEPS1=A |
| `#Multiversion`, one copy per CPU level; typed `core.arch` intrinsics "generated from vendor tables"; forced-level bit tests | Ratified, not built | D-CPU-DISPATCH1=A, D-SIMD-NATIVE1=A, cards #4167/#4168 |
| Per-system OS door; Jet-written allocator | Ratified | D-OS-FLOOR1=A, D-ALLOC-DEFAULT1=A |
| `core ⊂ alloc ⊂ hosted` layering; closed target fact menu | Ratified | D-RINGLAYER1=A, D-FREESTAND-PRELUDE1=A, D-FREESTAND-FACTS1=A |
| String keeps a built-in value; `Core/text/string.jet` owns methods; compiler supplies typed storage operations | Ratified | D-CORE-STRING-HOME1=A |
| Precompiled, memory-mapped Core interface record; startup ≤ 30 ms | Speed plan SP12 (implementation) | `~/.cache/jet-dev/speed/SPEED-PLAN.md:245,355` |
| **Where hardware knowledge lives; whether a Core pull request can add it without a compiler release** | **Open** | this report, ballot D-CORE-BOUNDARY1 |
| **Whether Core ships and versions as its own bundle, separate from the compiler binary** | **Open** (changes D-EXEC1's "for each Jet version" to "for each Core bundle") | this report |

Per the ballot clarity rules, the ballot asks only the two open rows. Everything
else is recorded as settled or as an implementation choice.

## 2. Today's boundary, with counts

All counts are from `git ls-files` and `wc -l` on 2026-10-04 at the current
checkout. Line counts include comments.

### 2.1 Where the code lives

| Home | Files | Lines | What it is |
|---|---:|---:|---|
| `Core/**/*.jet` | 115 | 40,628 | Core source: 114 declared modules, 110 with a source module (`crates/jet-codegen/src/Prelude/Core.jet`) |
| `crates/jet-codegen/src/Prelude/**/*.rs` | 239 | 222,864 | Rust runtime embedded in the compiler. Of this, `CoreLib/` holds 104 files and 133,937 lines; `Prelude/Core/*.rs` holds 101 files and 54,636 lines |
| `crates/jet-codegen/src/Prelude/**/*.js` | 35 | 15,446 | Web runtime |
| `crates/jet-codegen/src/Prelude/*.jet` | 9 | 4,873 | Compiler-language declarations in Jet: Core twin table, derives, diagnostics, effects, facts, markers, units |
| `crates/jet-foundation/src/**/*.rs` | 122 | 146,152 | Shared foundation. Some files are spliced into the runtime (`Codegen/mod.rs:408-419`) |
| `Compiler/JetFoundation/Source/Registry/*.jet` | 16 | 23,567 (1.58 MB) | Jet-compiler registry: `CoreCallRows.jet` 338 KB, `DiagnosticRows.jet` 828 KB, signature tables |
| `crates/jet-sema/src/Sema/CheckerCoreLib/` | 26 | 26,960 | Hand-written Rust signatures per Core call (for example `fixed_sigs.rs:5539-5718` types `core.archive`) |
| `crates/jet-jit/src/` | 62 | 161,473 | Cranelift tier; #3690 counts 88.8k lines of JIT host shims |
| `crates/jet-comptime/.../core_calls/` | 7 | 6,443 | Compile-time re-routes of Core calls |
| `Compiler/JetBackend/` | 23 | 7,861 | Jet's own back end; the x86-64 encoder (`X64/Encoder.jet`, 392 lines) covers the integer and control-flow subset only |

The 2026-09-28 inventory for #3690 classifies the Rust as follows (file
`~/.cache/jet-luna/jetpack-port/rust-core-inventory-plan.md:9-14`):

- semantic code that must move to Jet: 27 groups, 181 files, about 188.9k lines;
- provider leaves (syscalls, sockets, vetted engines): 6 groups, 18 files, 19.5k
  lines;
- emission (runtime ABI, operators, scheduler, allocator, web): 59.5k lines of
  Rust and 15.4k of JS;
- compiler and tooling: 134.4k lines, including 88.8k of JIT shims.

### 2.2 Registries that bind Core names to compiler code

- `crates/jet-codegen/src/Prelude/Core.jet` is the declared twin (D-OPENTABLE1).
  It holds 1,757 `dispatcher_row`s (1,424 plain, 264 receiver, 69 adapter), 338
  `ambient_route`s, 114 `module` rows, 110 `source_module` rows, and 7
  `format_row`s. Of the dispatcher rows, 599 carry a separate JIT symbol, 351 an
  interpreter route, 216 a pure route, 137 opt out of AOT and 126 opt out of the
  JIT. Rows cover 94 modules.
- Generated projections: Rust `CORE_CALLS` (1,306 rows,
  `crates/jet-foundation/src/Syntax/core_calls.rs:1824`); Jet `CORE_CALLS`
  (1,351 rows, `Compiler/JetFoundation/Source/Registry/CoreCallRows.jet`, which
  also appends 45 TIR rows from `method_calls.rs`).
- #3690 records that 1,440 of the dispatcher rows name a member that Core source
  already owns, so two routes exist for the same operation (inventory plan
  lines 18-22).

### 2.3 Built-in types and operations

- `Type` in `crates/jet-foundation/src/AST/types.rs:939-1069` has 23 variants.
  `Int`, `Float`, `Bool`, `String`, `Char`, `List`, `Map`, `Option`, `Result`,
  `IntN`, `Float32` and others are compiler enums.
- `RESERVED_TYPES` (`crates/jet-foundation/src/Collections.rs:9-69`) reserves 42
  more built-in names (E0106), for example `Set`, `PriorityQueue`, `LRU`,
  `Queue`, `Decimal`, `Duration` and `Cell`.
- `TBuiltinOp` has 220 variants in Jet (`Compiler/JetFoundation/Source/TIR/TIR.jet:1227-1448`)
  and 219 in Rust (`crates/jet-codegen/src/Codegen/TIR/mod.rs:13128-13587`).
  Grouped by prefix: 42 string or text, 39 list or scalar, 27 iterator
  adapters, 25 map, 24 set, 15 deque, 11 sorted set, 6 LRU, 6 byte buffer, 6
  bit set, 5 priority queue, 5 bag, 9 other. **[INFERENCE from reading the
  enum]** At least about 180 of these are library algorithms; roughly 30–40
  are storage or view primitives (length, push, get, view creation, atomics).
- About 200 built-in method rows (`Registry/BuiltinMethods.jet`), about 60
  built-in static rows (`Registry/BuiltinStatics.jet`), and 251 collection
  operation rows (`Compiler/JetFoundation/Source/Collections.jet`).
- Derive capabilities are already Jet source: `Prelude/Derives.jet` holds 108
  lines (D-ONCE-DERIVE1). Fact planes, including the 14 `Target.*` rows, are
  Jet rows in `Prelude/Facts.jet` (D-FACTDECL1). Both are compiled into the
  compiler.

### 2.4 Hardware, targets, allocators and OS

- **SIMD.** `Prelude/Core/SimdLanes.rs` (Rust inside the compiler crate)
  implements lane math, with `#[target_feature(enable = "avx")]` kernels guarded
  by `is_x86_feature_detected!` on each operation (lines 814-1191). The lane
  family is closed (D-SIMD3=B: F32x4 up to U8x32;
  `Examples/features/lowlevel/simd_wide.jet`). `#Multiversion` and `core.arch`
  do not exist in code yet: no match in `crates/`, `Compiler/`, `Core/` or
  `Examples/`.
- **GPU.** `CoreLib/Top/Compute.rs` (10,497 lines) embeds hand-written PTX
  (line 1160) and SPIR-V kernels (line 2200) as Rust string constants;
  `Prelude/Core/ComputeWebGpu.js` has 612 lines.
- **Targets.** `crates/jet-foundation/src/TargetMachine.rs` (4,864 lines) holds
  closed Rust enums for capabilities, allocator, panic, clock, entropy,
  scheduler, MMIO and startup policy. `OSTarget.rs` is a closed enum of `Linux`,
  `MacOS` and `Windows` (lines 12-17). `RingLayer.rs:214-300` hard-codes each
  Core module's layer.
- **Allocators.** `Prelude/ProgramAllocator.rs`, `PortableAlloc.rs` and
  `Core/FixedAllocator.rs` are Rust. `Core/mem/mem.jet:20-29` declares `Arena`,
  `Bump`, `Fixed`, `Pool` and `Pin` as empty "compiler-owned nominal surfaces".
- **Escape hatches that exist.** `#FFI(asm)` and `#FFI(c)` inline functions
  behind `#Unsafe` (`Docs/spec/spec.md:2456-2468`). `extern rust "std" { fn … =
  "path" }` (`spec.md:2470-2479`). `__core_intrinsic` is a compiler-only usage
  marker for runtime reachability (`CoreUsage.rs:11-60`), not a declaration
  surface.

### 2.5 How Core is loaded today

- **Rust compiler.** `crates/jet-driver/src/Loader.rs:2640-2726` schedules Core
  source modules lazily: those a user module imports, plus the transitive
  imports of each Core module, breadth first, plus the private String part. The
  text comes from `jet_sema::CoreSources::core_source_text`. That function reads
  110 `include_str!` entries (`crates/jet-sema/src/CoreSourceTexts.rs`) that
  are compiled into the binary. Each build parses, checks and lowers the
  reachable Core bodies again; there is no Core interface record.
  `Codegen/mod.rs` holds 348 `include_str!` uses that splice runtime source
  into emitted Rust (for example `PRELUDE_PARTS` from line 309 and
  `EMBEDDED_PRELUDE_PARTS` at lines 456-577).
- **Jet compiler (#3983).** The card body records that `Loader.jet` loaded only
  authorized package roots and `Native.rs` required a `package.jet` that Core
  lacks. As a result, `sema_call_core_source_target`
  (`Compiler/JetSema/Source/Sema/Calls/Core.jet:139-173`) found no `core.*`
  graph module and every Core call became a host row. The visible symptom is
  that `core.perf.override_fidelity` fails with `String`
  (`CorePlatformSignatures.jet:332`), not `PerfError`, so a program cannot match
  `.Err(.OutOfRange(x))`. Card logs from 2026-09-30 to 2026-10-01 show the
  loader port landed (`Loader.jet:1969-2151`, `<corelib>` identities), with
  Core-body pruning in `Pipeline.jet:2423-2438`. It has not been proven end to
  end. Checking Core under JetSema needed more than 26 GB per JIT run.
- **Root cause.** Core is half data (rows the compiler interprets) and half
  source (bodies the compiler re-checks). Each compiler must therefore
  re-implement both the row semantics and the source loader. #3983 is the
  second compiler reaching parity with the first on that split. It is not a
  missing Core feature.

### 2.6 What forces a compiler release today

| Change | Why the compiler binary must change |
|---|---|
| New CPU instructions (for example AVX-512 VNNI) | Lane kernels are Rust in `SimdLanes.rs`, embedded by `include_str!`; the lane type family is a closed compiler set; no `core.arch` |
| New GPU target | Device kernels are PTX/SPIR-V string constants in `Compute.rs`; Jet has no device code generation |
| New OS (for example FreeBSD) | `OSTarget` closed enum; `TargetMachine` closed menus; Rust std-based runtime |
| New allocator | Allocators are Rust Prelude parts; `Core/mem` types are compiler-owned shells |
| New numeric type (for example `F16` per D-LOWFLOAT1) | `Type` enum, reserved names, `BuiltinMethods` rows, `TBuiltinOp`, `NumericRuntime.rs` |
| Any edit to `Core/*.jet` | `CoreSourceTexts.rs` embeds the text in `jet-sema` |
| New Core function backed by Rust | Dispatcher row, generated tables, Rust symbol, JIT shim, and often a `CheckerCoreLib` signature |

## 3. How other languages draw the line

Each entry gives what stays in the compiler, what the library owns, how
hardware is added without a compiler release, and the primary source.

### Mojo

- **Compiler:** parser and elaborator; MLIR dialects `POPDialect`,
  `KGENDialect` and `LITDialect`; target lowering in `Mojo/lib/Target` and
  `KGENToLLVM`. Source: `github.com/modular/modular` at `24f4ceb2ff`, listing
  of `Mojo/lib`.
- **Library:** every scalar and vector, written over MLIR types and ops. Bool is
  `var _mlir_value: __mlir_type.`!kgen.scalar<bool>`` with
  `__mlir_op.`pop.simd.xor`` (`Mojo/stdlib/std/builtin/bool.mojo:87,376`).
  `struct SIMD[dtype, length]` is at `std/simd.mojo:445`, and
  `comptime Int8 = Scalar[DType.int8]` at line 122. The Mojo manual says
  standard types "aren't privileged ... even basic types like Int and String"
  (mojolang.org/docs/manual/types).
- **Hardware without a compiler release:** CPU feature queries are library code
  over a compiler attribute: `#kgen.param.expr<target_has_feature,…>` and
  `has_avx512f`, `has_vnni`, `has_neon`, `is_nvidia_gpu` and `is_amd_gpu`
  (`std/sys/info.mojo:249,412-725`). Any LLVM intrinsic is reachable through
  `llvm_intrinsic` (`std/sys/intrinsics.mojo:38-76`, `pop.call_llvm_intrinsic`),
  and inline assembly through `inlined_assembly` (`std/sys/_assembly.mojo:26`).
  GPU intrinsics are library code with a plain fallback, for example `mulhi` in
  `std/_gpu/intrinsics.mojo:348-376`:
  `comptime if is_nvidia_gpu(): return llvm_intrinsic["llvm.nvvm.mulhi.us", UInt32, has_side_effect=False](a, b)`,
  otherwise a 32-bit multiply. New target triples need `Mojo/lib/Target`.
- **Packaging:** `mojo precompile` makes a `.mojoc` of "non-elaborated code ...
  architecture-specific only after it's imported" (mojolang.org/docs/manual/packages).
  Library generics are therefore specialized per program. Jet's D-EXEC1 forbids
  that for Core semantic bodies.

### Swift

- **Compiler:** the `Builtin` module, with 264 `BUILTIN_*` rows in
  `include/swift/AST/Builtins.def` (swiftlang/swift `b9c404b6fa`).
  `getLLVMIntrinsicID` maps any `Builtin.int_<name>` to the LLVM intrinsic of
  that name (`lib/AST/Builtins.cpp:2633-2649`).
- **Library:** `Int` and friends are `@frozen public struct` with
  `public var _value: Builtin.Int${bits}`, and operators call
  `Builtin.cmp_eq_Int…` (`stdlib/public/core/IntegerTypes.swift.gyb:87-95,229`).
  SIMD storage is `Builtin.Vec${n}x…` (`SIMDVectorTypes.swift.gyb:246-261`).
- **Cross-module performance:** module interfaces plus `@inlinable` bodies,
  which clients may inline (swift.org/blog/library-evolution).
- **Hardware:** the standard library can name LLVM intrinsics, but the
  `Builtin` module is stdlib-only, so user packages cannot.

### Zig

- **Compiler:** about 121 `@builtin` functions (count of builtin headings in
  ziglang.org/documentation/master). Vector types, code generation and LLVM or
  self-hosted back ends.
- **Library:** `std`, shipped as source with the compiler. CPU features are
  library data: `lib/std/Target/x86.zig` (5,042 lines) is "auto-generated by
  tools/update_cpu_features.zig" (GitHub mirror `738d2be9d6`, 2025-11-27;
  development has since moved to Codeberg).
- **Loading:** "Zig uses lazy analysis for top-level declarations" (language
  reference, C translation section).
- **Hardware:** feature lists are library data, but every instruction is
  produced by compiler code generation. A new instruction needs a compiler (or
  LLVM) change.

### Rust

- **Compiler:** 205 `#[rustc_intrinsic]` functions in
  `library/core/src/intrinsics/mod.rs`. Lang items are "pluggable operations
  ... implemented in libraries, with a special marker", and they "are loaded
  lazily by the compiler" (Unstable Book, `lang_items`). Target features live in
  `compiler/rustc_target/src/target_features.rs:471`
  (`("avx512f", Stable, &["avx2", "fma", "f16c"])`).
- **Library:** `core`, `alloc` and `std`; `std::arch` comes from stdarch.
  `crates/core_arch/src/x86/avx512f.rs` is 62,676 lines with 369
  `#[link_name = "llvm.x86.avx512…"]` bindings (stdarch `0b2bd100de`).
- **Hardware:** intrinsics are library code, yet stable AVX-512 target features
  arrived with the Rust 1.89.0 release (blog.rust-lang.org, 2025-08-07: "a
  number of avx512 intrinsics and target features are also supported on x86").
  Library-only intrinsics did not free Rust from toolchain releases, because the
  feature vocabulary is a compiler table.
- **Loading:** crate metadata is memory-mapped
  (`compiler/rustc_metadata/src/locator.rs:944`, `Mmap::map(file)`). See
  section 5.3 for numbers.

### Julia

- **Compiler:** 92 intrinsic rows (`src/intrinsics.h`, JuliaLang/julia
  `10896e7ad5`). Most primitive types are declared in Julia source in
  `base/boot.jl` (`primitive type Float64 <: AbstractFloat 64 end`, line 272);
  `Bool`, `Int32`, `Int64` and the matching unsigned types are commented out
  there (lines 276-287) **[INFERENCE: created by the C runtime instead]**.
- **Library:** `Base` defines arithmetic over intrinsics:
  `(+)(x::T, y::T) where {T<:BitInteger} = add_int(x, y)` (`base/int.jl:87`).
- **Hardware without a Julia release:** GPU support is packages. CUDA.jl's
  "Exposing new GPU intrinsics" page shows a new PTX instruction added in the
  package:
  `fma_rp(x::Float64, y::Float64, z::Float64) = ccall("llvm.nvvm.fma.rp.d", llvmcall, Cdouble, (Cdouble, Cdouble, Cdouble), x, y, z)`
  (cuda.juliagpu.org/stable/hacking/exposing_new_intrinsics).

### Odin

- **Compiler:** 266 intrinsic procedures (`base/intrinsics/intrinsics.odin`,
  `16de4f5103`).
- **Library:** `core:simd/x86` has one file per instruction family, from
  `sse` to `avx512bw` and `avx512f`. `avx512f.odin:10-15` writes
  `_mm512_srlv_epi32` as library code over portable lane intrinsics
  (`simd.lanes_lt`, `simd.select`, `simd.shr`) with
  `@(enable_target_feature="avx512f,evex512")`. A new instruction set is a
  library change whenever portable lane operations can express it.

### Nim

- **Compiler:** "magics". **Library:** `system` binds procedures to them, for
  example `proc `+`*(x, y: int): int {.magic: "AddI", noSideEffect.}`
  (`lib/system/arithmetics.nim:77`) and `len` with `magic: "LengthStr"`
  (`lib/system.nim:679`).

### Terra

- Terra is "embedded in and meta-programmed by the Lua programming language"
  (terralang.org). `terralib.intrinsic("llvm.sqrt.f32", float -> float)`
  reaches any LLVM intrinsic. Its documentation warns: "the precise sets of
  available intrinsics depends on the LLVM version and the target platform, and
  is not under Terra's control" (terralang.org/api.html).

### Carbon

- The prelude is Carbon source that binds to named compiler builtins:
  `private fn MakeInt(size: IntLiteral) -> type = "int.make_type_signed";`
  `class Int(N: IntLiteral) { adapt MakeInt(N); … }` and
  `fn Convert(self) -> Int(To) = "int.convert_checked";`
  (`core/prelude/types/int.carbon:15-36`, `d31a8b67d3`). This is the closest
  precedent for a typed kernel-binding declaration in library source.

### C++

- Compiler builtins (`__builtin_*`), with vendor intrinsic headers on top.
  `std::simd` is a library type since C++26 (cppreference "Data-parallel types
  (SIMD) (since C++26)"; WG21 P1928R15). New instructions need compiler support
  for the builtin.

### Triton and MLIR

- Triton finds back ends as plugins:
  `entry_points().select(group="triton.backends")` for "out-of-tree/downstream
  plugins" (`python/triton/backends/__init__.py:38-58`, `fa8415bdf8`). In-tree
  back ends are `third_party/nvidia` and `third_party/amd`; Intel ships
  `intel-xpu-backend-for-triton` as a separate repository. Each plugin is a
  whole compiler back end, not a library of instructions.

### Jai

- Not accessible: the compiler is proprietary and has no public source.

### Lessons for Jet

1. The kernel is small and closed everywhere: about 90 to 270 primitive
   operations. Most named types are library code; in Mojo, Swift and Carbon
   that includes `Int`.
2. Hardware without a compiler release always uses one of two forms. One is a
   typed declaration in the library that maps to a backend operation, with a
   plain fallback (Mojo `mulhi`, Odin x86 files). The other is an untyped
   backend name that only one backend understands (Terra's warning).
3. The vocabulary of features and CPU levels must live with the instructions.
   Otherwise the Rust 1.89 trap repeats.
4. Targets stay in the compiler: triples, register kinds, calling conventions
   and object formats. Even Triton, whose plugins are whole compilers, follows
   this.
5. Lazy, memory-mapped library interfaces are universal and cheap. See section 5.3.
6. Speed across the boundary comes from compiler-known primitives that inline,
   plus either exported bodies (Swift `@inlinable`, Rust generic metadata, Mojo
   elaboration) or a compile-once library. D-EXEC1 chose compile-once, so the
   hot paths must be kernel operations.

## 4. Proposed boundary

### 4.1 The kernel: what must stay in the compiler

1. **Language.** Lexer, parser, sema (I3), effects, ownership, diagnostics,
   the checking-changing markers (D-MARKER-LAW1), compile-time evaluation, MIR
   lowering (D-TIER-ONEIR1), the optimizer, the back ends and the linker.
   Compiler-language declarations stay compiler tables (D-OPENTABLE1): markers,
   facts, diagnostics and effect roots.
2. **Machine value kinds.** Fixed integers (`I8`–`I128`, `U8`–`U128`), floats
   (`F16`, `BF16`, `F32`, `F64`, plus the 8-bit storage formats from
   D-LOWFLOAT1), `Bool`, raw pointers, lane vectors (fixed and scalable,
   D-LANES1), and the exact-`Int` small/big representation (D-INTBIG1).
3. **Kernel operations.** One typed, closed, numbered table, generated from a
   declared Prelude twin as D-OPENTABLE1 requires. It holds arithmetic in each
   overflow mode, compare, convert, bit operations, IEEE rounding and square
   root, and the exact-`Int` fast path with a slow-path call. It also holds
   load, store, copy and fill; owned-buffer storage operations (length,
   capacity, grow, checked index), which are the F3 operations of
   D-CORE-STRING-HOME1; atomics (D-PLACE1, D-ATOMIC-WIDTH1); and lane splat,
   shuffle, select and fixed-order reduce (D-FRED1). Finally: fences, the
   syscall instruction per architecture (D-OS-FLOOR1), C calls, inline
   assembly, the CPU feature probe, stops and traps.
   **[INFERENCE]** This lands at about 150–300 operations, the same order as
   Zig, Rust, Swift and Odin.
4. **ABI and layout.** Calling conventions, struct and enum layout, niches, the
   C ABI, object formats, target triples, data layouts and register kinds per
   target.
5. **Roles.** A small closed set of role tags by which Core types bind language
   syntax and optimizer knowledge, like Rust lang items. Examples: literal
   types, `?`/`!` carriers, the iteration protocol, drop and close,
   interpolation display, equality, order and hash, ranges. Roles load lazily.
6. **The catalog engine** (recommended option A only). It validates Core
   instruction rows against a closed set of operand forms and register kinds,
   and selects and encodes them in a table-driven way.

### 4.2 What Core owns

Core owns every named type and method: `Int`, `Float`, `String`, `List`, `Map`,
`Set` and the other collections, `Option`/`Result` surfaces, `Decimal`,
`Fraction`, `Duration` and so on. It owns every collection algorithm, which
moves about 180 `TBuiltinOp` variants into Jet. It owns text, formatting and
Unicode tables (F2); the big-integer slow path; allocators (`mem.Heap`,
`Fixed`, `Arena`, `Pool`); the task scheduler over kernel atomics and syscalls;
OS adapters per system and CPU family (D-OS-FLOOR1); CPU level definitions and
feature detection; instruction catalogs (`core.arch`); GPU device libraries and
compute providers; target profile data such as board register files
(D-FOUND-BOARD1); runtime stop reports; and effect leaf declarations.

### 4.3 Performance model

- **Kernel operations expand inline at every level**, exactly as operators do
  today. This is where `Int` arithmetic, bounds-check elimination, lane
  registers and atomics stay compiler-known for the optimizer. Today every
  `Int` operation is an out-of-line `JetInt` call; that is root cause 2 in
  `Docs/audits/compile-cost-hello-2026-10-02.md:84-94`.
- **Kernel-bound Core declarations have no Jet body.** A Core method such as
  `List.len` is declared in Core and bound to a kernel operation, the way
  Carbon binds `"int.convert_checked"`. Expanding it copies no semantic body,
  so D-EXEC1 holds.
- **Transparent Core functions expand at the call site.** These are functions
  whose bodies are only kernel operations and other transparent calls, with no
  loops, under D-OPT-INLINE1's size limit, carried as MIR in the interface
  record. All levels expand the same MIR (D-TIER-ONEIR1). This clarifies
  D-EXEC1 for bodies that hold no algorithm. It needs owner confirmation and
  is listed in the ballot.
- **Other Core bodies are compiled once** at O2 into prebuilt runtime objects
  per target ABI (D-EXEC1). Generic bodies use checked thin adapters (the
  efficiency research's B7 reading). Calls are direct symbol calls, with no
  dynamic dispatch. Indirect adapter calls are reported and measured, as the
  withdrawal note requires.
- **Hot Core kernels use `#Multiversion`** inside the prebuilt runtime, with
  one copy per CPU level selected once at start (D-SIMD-NATIVE1). New CPU
  levels are Core rows.
- **I9.** One table of kernel operations, each with one MIR lowering; Core
  bodies compiled once; catalog rows carry a plain-Jet meaning checked by
  forced-level tests. Every level means the same thing, and only speed differs.

## 5. Loading Core only when it is used

### 5.1 Design

1. **Core bundle per Core version.** The bundle has five parts:
   1. the sources `Core/**/*.jet`, the semantic authority (D-FREESTAND-ARTIFACT1=C);
   2. one interface record per module, in the `JetFoundation/Source/Record`
      format that packages also use (#2517, R8);
   3. prebuilt O2 runtime objects per target ABI, as archive members with
      per-function sections, so the linker pulls only reached symbols
      (`efficiency-architecture-2026-10-02.md:442-449`);
   4. a manifest naming the kernel version it needs;
   5. the release signature (D-BUILD-PREBUILT1).

   The interface record holds signatures, types, layouts, effects, roles,
   kernel bindings, catalog rows, MIR of transparent bodies, export tables,
   dependency edges and layer.
2. **Start-up** maps only the bundle index. `use core.x` resolves to a record
   offset; pages are touched on first reference. Diagnostic rows load only when
   one renders (SP12). The budget is ≤ 30 ms from process start to the first
   parse span (`SPEED-PLAN.md:355`).
3. **Checking.** Core bodies are checked once, when the bundle is built. A user
   build checks only user code against Core interfaces. This removes the
   per-build Core re-check that the Rust loader does today and that #3983 is
   porting.
4. **Linking.** One typed link plan pulls only reached runtime functions
   (#4308 criterion 6).
5. **Compile-time code.** `prep` code calls the same prebuilt runtime through
   O0, loaded into the compiler process on the first `prep` use (D-EXEC1).
6. **Runtime services** start on first use (D-EFF-CORE-INIT1).

### 5.2 Cost of not having it (Jet, measured earlier)

- The Jet-written compiler spends 0–18 s and grows from 0.8 to 11.6 GB
  "restoring the embedded compiler image ... decoded into about 11 GB of owned
  objects" for hello world (`compile-cost-hello-2026-10-02.md:63`).
- `jet build` hello: 7.03 s cold, 1.33 s warm, 179 MB. It writes "6.3 MB of
  Rust: the runtime and CoreLib prefix, identical for every program" (same
  audit, lines 40 and 65).
- Startup today is 5.9 s against a 0.03 s budget, a 197× gap
  (`SPEED-PLAN.md:133`).

### 5.3 Peers

Local measurement on 2026-10-04 (rustc 1.97.1, this machine):

- The sysroot metadata the compiler can read is `libcore` 64.1 MB, `liballoc`
  7.4 MB and `libstd` 8.7 MB of `.rmeta`.
- `rustc --emit=metadata hello.rs` (full name resolution and type check
  against that metadata): 0.04 s and 122 MB peak, in each of three runs.
- A full debug build of hello: 0.17–0.20 s and about 138 MB.

The peer audit records Zig 0.16.0 ReleaseSmall hello at 2.41 s cold and 1.35 s
warm, and Go 2.12 s cold and 0.12 s warm (`compile-cost-hello-2026-10-02.md:36-38`).
Zig analyzes declarations lazily and Go reads compact export data
(`SPEED-PLAN.md:277-278`). Swift module interfaces and Mojo `.mojoc` packages
follow the same idea. No published Swift or Mojo load-time numbers were found;
see the unavailable sources.

## 6. Structure of `Core/*.jet`

### 6.1 Layers

Namespaces stay as ratified (D-CORE-TREE1, D-CORE-MODULE-MAP1). A layer is a
declared fact in each Core package header, reusing the existing `runtime:`
manifest field (`RingLayer.rs:3-6`). The compiler only enforces it; it no
longer hard-codes `layer_of`.

| Layer | Contents | May import |
|---|---|---|
| 0 kernel bindings | private parts next to their owning module, as `Core/text/string.jet` is today | kernel only |
| 1 core (no heap, no OS) | math, units, lanes, `mem` views and `Fixed`, text and byte views, time arithmetic, encoding views, `perf`, `core.arch` catalogs and CPU levels | 0 |
| 2 alloc | `String`, `List`, `Map` and the other collections, owned text and encoders, regex, crypto, data, CPU compute, log | 0–1 |
| 3 os adapters (private) | one module per system and CPU family: Linux syscalls, macOS libSystem, Windows kernel32/ntdll; `mem.Heap` page source; start-up providers | 0–2 |
| 4 hosted | files, process, net, http, task, term, clock time, db, test | 0–3 |
| 5 batteries | web, ui, game, data loaders, email, auth, GPU compute providers | 0–4 |

Rules:

- Imports go only downward.
- Batteries are never imported below layer 5.
- `core.arch` rows are reachable only from `#Multiversion` copies or from Core
  kernels (D-SIMD-NATIVE1).
- OS adapters and kernel bindings are private to bundled Core and to
  owner-admitted providers (D-RT-NATIVE-CAP1).
- The existing `depends_on` edges stay acyclic.

### 6.2 Versioning

- The kernel has one integer version. A Core bundle declares the kernel version
  it needs, and a compiler implements exactly one. Under greenfield rules
  there are no ranges and no shims.
- The bundle identity is the digest of its source closure.
- The toolchain pins a default bundle, and a project lock pins the digest.
- A Core-only release changes the bundle digest, not the compiler. A kernel
  change ships a compiler and a matching bundle together, as every release does
  today.
- The compiler is itself a Jet program, so it links its own pinned Core copy.
  User programs use the bundle. Stage-2 bootstrap uses the pinned copy, so a
  Core-only pull request never changes bootstrap inputs.

### 6.3 How a hardware pull request is shaped

1. **More AVX-512 instructions once the compiler knows the 512-bit registers**
   (for example VNNI). This is a Core-only pull request:
   - generated rows in `Core/arch/x86/avx512vnni.jet`, each with a typed
     signature, the needed feature, the encoding form, the LLVM operation and a
     plain-Jet body;
   - a feature row and an `x86_64_v4` level change;
   - Core kernels that use the rows in `#Multiversion` copies;
   - generated forced-level bit tests (D-SIMD-NATIVE1);
   - a paired performance cell (AGENTS.md performance gate).

   The signed bundle then ships with no compiler change.
2. **First AVX-512 support.** One compiler change adds the 512-bit and mask
   register kinds and the EVEX encoding form to the x86-64 target description.
   The instructions follow as the Core pull request in item 1.
3. **RISC-V vectors.** One compiler change adds the `riscv64` target, vector
   registers and vector-length state. Core then adds RVV rows, the scalable
   `Lanes` mapping (D-LANES1), kernels, and the `riscv64` Linux syscall module.
4. **A new GPU target.** The compiler gains the device triple, address spaces,
   kernel calling convention and device object output. Core supplies device
   instruction rows, the compute provider with capability negotiation
   (D-COMPUTE-BACKEND1), driver bindings over the C ABI (F5), and a
   conformance suite against the CPU oracle.
5. **A new OS on a known CPU and object format** (for example FreeBSD on
   x86-64). This is a Core pull request: the syscall module, a start-up
   provider (D-FREESTAND-START1), adapters, and the target row. The compiler
   changes only if a new object format or calling convention is needed.
6. **A new allocator.** Core only (D-ALLOC-DEFAULT1, D-ALLOC-PROGRAM1).
7. **A new numeric type.** Core only when its machine kind exists. A new machine
   kind is a compiler change.

Today the x86-64 encoder of Jet's own back end is 392 lines covering integers
and control flow (`Compiler/JetBackend/Source/X64/Encoder.jet:1-4`). Building
SIMD support table-driven from the start costs no rework.

## 7. Options for the ballot

All three options share the following work:

- Core ships as its own signed, versioned bundle, read lazily from interface
  records.
- Kernel operations and kernel-bound declarations expand inline; transparent
  bodies expand at the call site; other bodies are compiled once.
- The layer rules above, and completing #3983 parity, then the #3690 port.

The options differ only in where hardware knowledge lives.

### A. Typed hardware catalog in Core (recommended)

- **Exact behavior.** A Core row declares one instruction with
  `#Instruction(needs:, encode:, llvm:)` over a function whose Jet body is its
  meaning. The compiler validates every field against its closed operand
  forms, register kinds and LLVM intrinsic table when the bundle is built. At
  every level it emits the real instruction inside matching `#Multiversion`
  copies, and the body elsewhere.
- **Beginner path.** Nothing changes; list sums get faster when Core learns a
  chip.
- **Expert path.** `use core.arch.x86 as x86` and
  `prep if $build.cpu.has(.AVX512F)` exactly as D-CPU-DISPATCH1 shows. Rows
  are generated from vendor tables.
- **Edge cases.**
  - A row used outside a matching copy is a build error (D-SIMD-NATIVE1).
  - A row whose instruction and body disagree fails the bundle's forced-level
    tests; users never receive it.
  - A row needing an unknown register kind is rejected at bundle build with a
    registered diagnostic naming the missing compiler support.
  - Memory-touching rows stay `#Unsafe` at call sites.
  - Rows are private to bundled Core and admitted providers.
- **#3983.** Parity first. The Jet compiler then reads interface records
  instead of re-checking Core, and the host rows disappear with #3690.
- **D-EXEC1.** Compile-once is unchanged. "For each Jet version" becomes "for
  each Core bundle". The transparent-body clarification is stated.
- **I9.** Every level runs either the instruction or the identical body, with
  bit equality enforced by tests.
- **Stage 2.** No effect on bootstrap inputs.
- **Port.** Rows replace `SimdLanes.rs`; PTX and SPIR-V constants become
  device rows and kernels.
- **Speed plan.** SP12's record gains catalog rows; the startup budget is
  unchanged.
- **New mechanism.** One marker (I7, I8). It reuses `#Unsafe` declaration
  gating and the D-RT-NATIVE-CAP1 registry.

### B. Open backend door (Mojo-style)

- **Exact behavior.** Core, and owner-admitted providers, may name any LLVM
  operation or type inside audited `#Unsafe`, with a required plain-Jet body.
  Release builds (O2, LLVM) emit the named operation. Jet's own levels
  (O0/O1) run the plain body until the Jet back end learns the instruction in
  a compiler change.
- **Gain.** Reach. Any instruction or type LLVM knows works in release builds
  on day one, including new register kinds and GPU intrinsics, with no
  compiler change.
- **Cost.** Default and fast-start builds stay slow for new hardware. Core is
  tied to LLVM's names, and Terra documents that those are "not under Terra's
  control". This touches I6.
- **Safety.** The compiler validates names against LLVM's intrinsic table at
  bundle build, as Swift does in `getLLVMIntrinsicID`, so a typo never reaches
  users as a back-end rejection (I2).
- **I9.** Meaning is identical on every level, enforced by tests; speed differs
  by level.

### C. Hardware stays in the compiler

- **Exact behavior.** Library semantics move to Core as already ruled. The
  `core.arch` vendor tables, CPU levels, OS identities and target menus are
  generated into compiler tables. Each new instruction or target is a compiler
  pull request and a compiler release.
- **Gain.** Simplest. Every level gets native code at once, and nothing new
  goes into Core.
- **Cost.** Exactly what the owner asked to end. This is how Zig, Go and
  stable Rust work (section 3).

### Security model for any instruction row or backend door

- **I1.** Declaration only in bundled Core or owner-admitted providers
  (D-RT-NATIVE-CAP1, D-UNSAFE-DEPS1). Option A rows are typed,
  compiler-validated data, so they need no `#Unsafe` declaration. Option B's
  untyped backend name does, with a reason. In both options, rows that read
  or write memory also stay `#Unsafe` at call sites, and lane-only rows are
  safe only inside a matching `#Multiversion` copy (D-SIMD-NATIVE1).
- **Meaning.** A plain body is mandatory. Forced-level differential tests at
  bundle build follow D-SIMD-NATIVE1's result law: no FMA, approximate or
  reordering instructions unless the effect row admits non-reproducible
  results.
- **Validation.** Every backend name and encoding is checked at bundle build,
  so a user build never sees a back-end rejection (I2).
- **Hardened builds** and dev sentries apply unchanged (D-MEM-HARDEN1,
  D-MEM-SENTRY1).

## 8. Migration sequence and size

This is a proposed order for Tower cards. It does not own the plan.

1. **#3983 parity** (in flight). The Jet compiler loads Core source as the
   Rust compiler does.
2. **Kernel table and roles.** Declare the kernel twin; bind Core types through
   private kernel parts (F3); move the about 180 library-algorithm
   `TBuiltinOp` variants into Jet, each with a paired performance cell. This
   retires most hand-written Core signatures (26,960 Rust lines in
   `CheckerCoreLib`; the Jet signature tables in `Registry/`). **[INFERENCE]**
3. **#3690 waves.** About 188.9k semantic Rust lines move to Jet. F4 provider
   leaves replace the 1,757 dispatcher rows, and F5 brings C-ABI calls. The
   88.8k JIT shim lines leave with D-EXEC1's Cranelift retirement (#4013).
4. **Core bundle.** Interface records (SP12, #2517), prebuilt runtime objects
   per target (#4013, absorbing #3954), and the lazy loader. Remove the 110
   embedded Core texts and the 348 `include_str!` runtime splices.
5. **Hardware catalog** (option A). The marker and validator; a table-driven
   x86-64 encoder; a row generator from vendor tables; #4167 and #4168 build on
   it.
6. **Targets, OS and allocators as Core data.** `OSTarget`, `layer_of` and the
   `TargetMachine` menus become Core rows read by the compiler. The fact schema
   stays compiler-owned (D-FREESTAND-FACTS1).

Doc follow-up after the cutover: I9 names "the embedded Prelude and the
ratified CoreLib" as its home. That text should then name the kernel table and
the Core bundle (compare D-I9-HOME1).

## 9. Recommendation and trade-off

Choose **A**.

- It is the only option in which a library pull request gives new
  instructions native speed on every level. It keeps the meaning in plain Jet,
  so I9 is enforced by tests, not trust.
- It reuses ratified pieces: `#Multiversion`, `core.arch`, the vendor-table
  generation, `#Unsafe`, the provider registry and the signed prebuilt
  runtime.
- **Trade-off.** A new kind of register, target triple or calling convention
  still needs one compiler change per family. Each row also carries more than
  a name: an encoding and a plain-Jet body. Both are mitigated: the compiler
  part stays a small target description, and rows and tests are generated from
  vendor tables.
- **B** buys release-build reach at the cost of slow default builds and an
  LLVM-name dependency. **C** keeps every hardware change behind a compiler
  release.

## 10. Sources

Read 2026-10-04:

- mojolang.org/docs/manual/types; mojolang.org/docs/manual/packages.md;
  github.com/modular/modular `24f4ceb2ff`: `Mojo/stdlib/std/builtin/{int,bool}.mojo`,
  `std/simd.mojo`, `std/sys/{info,intrinsics,_assembly}.mojo`,
  `std/_gpu/intrinsics.mojo`, `Mojo/lib` listing.
- github.com/swiftlang/swift `b9c404b6fa`: `include/swift/AST/Builtins.def`,
  `lib/AST/Builtins.cpp`, `stdlib/public/core/IntegerTypes.swift.gyb`,
  `SIMDVectorTypes.swift.gyb`; swift.org/blog/library-evolution.
- ziglang.org/documentation/master; github.com/ziglang/zig `738d2be9d6`
  `lib/std/Target/x86.zig`.
- github.com/rust-lang/stdarch `crates/core_arch/src/x86/avx512f.rs`;
  rust-lang/rust `compiler/rustc_target/src/target_features.rs`,
  `compiler/rustc_metadata/src/locator.rs`, `library/core/src/intrinsics/mod.rs`;
  doc.rust-lang.org/unstable-book/language-features/lang-items.html;
  blog.rust-lang.org/2025/08/07/Rust-1.89.0.
- JuliaLang/julia `10896e7ad5`: `src/intrinsics.h`, `base/int.jl`, `base/boot.jl`;
  cuda.juliagpu.org/stable/hacking/exposing_new_intrinsics.
- odin-lang/Odin `16de4f5103`: `base/intrinsics/intrinsics.odin`,
  `core/simd/x86/{avx2,avx512f}.odin`.
- nim-lang/Nim devel: `lib/system.nim`, `lib/system/arithmetics.nim`.
- terralang.org; terralang.org/api.html.
- carbon-language/carbon-lang `d31a8b67d3`: `core/prelude/types/int.carbon`.
- en.cppreference.com/w/cpp/numeric/simd; WG21 P1928R15.
- triton-lang/triton `fa8415bdf8`: `python/triton/backends/__init__.py`,
  `third_party/`; github.com/intel/intel-xpu-backend-for-triton.
- Tower (read-only): card #3983, #3690, #3548, #4013, #4167, #4168, #4308,
  #2517, #4565; decisions listed in section 1.

Not accessible or not obtained:

- GitHub code search (requires authentication).
- The current path of Rust's lang-item table; the count of lang items was not
  obtained.
- Published load-time numbers for Swift module interfaces, Mojo `.mojoc` and Go
  export data.
- The history of when Mojo added each GPU vendor.
- Jai (proprietary).
- Zig's current Codeberg tree; the GitHub mirror is frozen at 2025-11-27.
