# A greenfield FFI design, and what Jet should adopt

Card #6366. Status: proposal. Owner choices are in the ballots listed in §9.
They are not decided here.

## Plain summary

- **The idea.** Every foreign boundary is described once, as a *boundary
  contract*. That is a typed list of functions and types, plus the facts a C
  header cannot say: who owns memory, how long a view lives, how errors come
  back, which thread may call, and whether a call is async.
- **Readers and writers.** A reader package builds the contract from whatever
  the other side has (a C header, a Rust crate, a `.d.ts`, a WIT file, a JVM
  class, Windows metadata, a GPU shader). A writer package turns the same
  contract into bindings for whoever calls *us*. Both directions share one
  model, so N languages cost N readers plus N writers, not N×N bridges.
- **Two layers.** A small, complete, unsafe *primitives* layer can express
  every boundary: layout, symbols, calls, memory, callbacks, errors, threads,
  async, isolation. Pareto *wrappers* sit on top: `use "png.h" as png`,
  `#Export`, and typed imports for Rust, Python and JS. Every wrapper
  expands into primitives that you can inspect, so the expert path is a ladder,
  not a cliff.
- **Safe by default.** A call is safe when its facts are checked, vouched for
  in a named `#Unsafe("reason")`, or *isolated*. Isolated means the same import
  runs in a wasm sandbox or a separate process, with no source changes.
- **Zero cost.** A crossing whose types already match costs one direct call.
  When the foreign source is visible, the optimizer can inline across the
  boundary.
- **For Jet.** Most of this is already ratified: header import, C source import,
  binder packages, ownership sigils at the border, vouches, both embedding
  artifacts, and the hardened profile. The new pieces are a public primitives
  layer, isolation as a placement option, and rich exports. Each is one ballot.
  Inline C stays, and becomes a thin wrapper over C-source import, which
  removes the "two ways to bring in C" loss that D-FFI-ONE1 recorded.

This builds on, and does not replace, the boundary audit
[`Docs/audits/ffi-owned-source-and-boundaries.md`](../audits/ffi-owned-source-and-boundaries.md)
(#2933), which covers project arrangements and workflows. This document is the
mechanism underneath them.

---

## 1. What "perfect" means: ten testable goals

| # | Goal | Test |
|---|------|------|
| G1 | **Any to any.** Call out to, and be called from, C, C++, Rust, Zig, Swift, Objective‑C, Java/Kotlin, C#, Python, JS/wasm, GPU kernels and OS ABIs. | Every language has a reader and a writer, or a documented reason why not. |
| G2 | **Zero cost when shapes match.** | A blittable call compiles to the same machine code as a C-to-C call. |
| G3 | **Safe by default, total control when asked.** | No safe code can corrupt memory through a binding. Every unchecked fact is a named, auditable vouch. |
| G4 | **No hand-written glue for the common case.** | One line binds a library. Zero lines of marshalling. |
| G5 | **No cliff.** | Every wrapper is a documented composition of public primitives (philosophy: *Ladders, Not Cliffs*). |
| G6 | **Facts, not guesses.** | Each fact (ownership, length, nullability, error style) carries its provenance. Unknown facts get the conservative projection, never a guessed one. |
| G7 | **One meaning everywhere.** | The same import behaves the same at every tier, on the web, and in compile-time code (I9). |
| G8 | **Versioned.** | A library upgrade that breaks the contract fails at build time or at load time, with a diff. It never shows up as a crash. |
| G9 | **Debuggable.** | Mixed-language backtraces, boundary tracing, and hardened checks at the edges. |
| G10 | **Placement independent.** | The same import can run in-process, in a wasm sandbox, or in another process. Only one option changes. |

No shipping system meets more than four of these goals today (§2).

## 2. State of the art

Each row lists the idea worth taking and the gap that stops it being the
answer. Sources are in §10.

| System | Model | Best idea | Gap |
|---|---|---|---|
| **Zig** `@cImport` / translate‑c | Compile-time header translation with a bundled Clang | The compiler is also a C compiler, so C and Zig cross-compile and link with no extra toolchain. | C only. No ownership or length facts. `@cImport` is moving to the build system (0.16 notes) because compile-time import tied the language to Clang's life cycle [Z1,Z2]. |
| **Rust bindgen / cxx / UniFFI / PyO3 / Diplomat** | Build step (bindgen), bridge macros (cxx), IDL or proc-macro (UniFFI, Diplomat), per-host crate (PyO3) | cxx: a safe *bridge* language that is checked on both sides. UniFFI and Diplomat: **one description, many host languages**. Rust 2024 `unsafe extern` lets each item be marked `safe` [R1–R6]. | Five tools with five models. Outward bindings come from Rust only. The stable ABI (`crabi`) is still experimental [R7]. |
| **Swift ↔ C/C++/ObjC** | Clang importer in the compiler | Imports headers directly. Clang *API notes* refine imported facts without editing the header. Non-escapable types and lifetime annotations make C++ views safe [S1–S3]. | Safety rests on annotations that most libraries lack. Template and virtual-class support is partial [S4]. |
| **Kotlin/Native cinterop, Swift export** | `.def` file plus a build-time tool | A declarative definition file that holds options, headers and filters. Direct Swift export without ObjC headers [K1–K3]. | Build step only. ObjC import is still Beta. |
| **.NET P/Invoke → `LibraryImport`** | Source generator at compile time | Marshalling code is *generated and visible*, works with AOT, and is extensible through custom marshallers [N1,N2]. `UnmanagedCallersOnly` handles the reverse direction. | One direction at a time. No lifetime facts. |
| **Java FFM (Panama)** | Runtime linker API plus `jextract` | A complete *primitives* API (Linker, FunctionDescriptor, MemoryLayout, Arena) with jextract as the wrapper. `captureCallState` reads errno. Confined arenas bound lifetimes [J1–J3]. | Verbose without jextract. Safety is a runtime check, not a type. |
| **LuaJIT FFI** | Parse C declarations at run time | The JIT compiles FFI calls inline. Close to zero overhead from a dynamic language [L1]. | No safety at all. C only. |
| **Python cffi / ctypes** | ABI mode (dlopen) or API mode (compile a shim) | Offers *both* modes behind one declaration language [P1,P2]. | Copies by default. No ownership model. |
| **Nim** `importc` / `importcpp` | Pragmas on declarations | Pattern strings for C++ calls (`#.method(@)`) reach templates without a shim [M1]. | Nothing is checked. A wrong pattern means undefined behaviour. |
| **Odin** `foreign` blocks / **Jai** `#foreign` | Declarations in the source, link by name | The shortest syntax there is. Calling convention and library on the block [O1,JA1]. | Hand-written. Nothing is read from headers. |
| **D** ImportC / `extern(C++)` | Compiler compiles C directly | `import foo;` on a `.c` file. Matches C++ mangling and vtables for single inheritance [D1,D2]. | C++ templates must be mirrored by hand. |
| **Carbon** | `import Cpp library "x.h"` | C++ interop is a *design mandate*. Bidirectional, with no wrapper layer [C1,C2]. | Experimental. |
| **Mojo** | Python objects in Mojo, plus `PythonModuleBuilder` for exports | Both directions with a single builder [MO1]. | CPython only. Exports are built by hand. |
| **GraalVM polyglot** | Shared VM, interop protocol | One object protocol across JS, Python, Ruby, Java and LLVM bitcode [G1]. | Needs the Graal VM. Not a native ABI. |
| **wasm Component Model / WIT** | IDL plus canonical ABI | **A language-neutral contract with resources (owned handles), results, and since WASI 0.3, async `future`/`stream`** [W1–W3]. Isolation comes free. | Copies across the boundary. Sandboxed only. |
| **WinRT / win32metadata** | Machine-readable metadata, then projections | Windows APIs described once, projected into C#, C++ and Rust [WM1]. | Windows only. |
| **RLBox** (Firefox) | Compile C libraries to wasm, then back to native | *Isolation without rewriting*: the same library is sandboxed, with "tainted" types at the edge [RB1]. | C++ only. Annotations by hand. |
| **Arrow C Data / DLPack** | ABI-stable structs for columns and tensors | Zero-copy data exchange across every runtime [A1,A2]. | Data only, no calls. |
| **Clang `-fbounds-safety`** | `counted_by` / `sized_by` annotations | Length facts live in the C header itself, so any reader can use them [B1]. | Only as good as adoption. |

**The pattern:** the best systems separate a *contract* (WIT, winmd, UniFFI
UDL, cxx bridge) from *projections*. The best ergonomics come from reading
sources the user already has (Zig, Swift, D, Carbon). The best expert control
comes from a public primitives API (Java FFM). Nobody combines all three, and
nobody makes placement (in-process versus sandbox) a single option.

## 3. The design in one picture

```
 foreign sources          readers (packages)        contract           writers (packages)        callers
 C/C++/ObjC headers ─┐                          ┌─ functions, types ─┐  C header ─────────────┐
 C/C++ source        ├─► c, cpp, objc ──────────┤  + facts with      ├─► C++ RAII header        │
 Rust crate (rustdoc)├─► rust                   │    provenance:     ├─► Swift package          │
 Swift .swiftinterface► swift                   │  ownership, life-  ├─► Kotlin/JVM (FFM, JNI)  ├─► any host
 JVM class / .NET md ├─► jvm, dotnet            │  time, length,     ├─► C# LibraryImport       │
 .pyi / .d.ts / WIT  ├─► python, js, wit        │  errors, threads,  ├─► Python ext (abi3)      │
 winmd / SDK .tbd    ├─► os                     │  async, effects,   ├─► Node-API / JS + WIT    │
 SPIR-V / PTX / WGSL ┘─► gpu                    └─ version, purity ──┘  Rust crate, WIT world ──┘
                                     ▲                     │
                                     │        lowered by the primitives layer
                       overlays (API-notes style)          ▼
                                            layout · symbol · call · memory · thunk ·
                                            error capture · thread attach · async bridge ·
                                            marshal · isolate
```

There are four layers. Each one is written only in terms of the layer below.

1. **Primitives (L0).** The complete, unsafe vocabulary (§4).
2. **Contract (L1).** Plain data: the rows and their facts. Readers produce it,
   overlays refine it, writers and the lowering pass consume it (§5).
3. **Readers and writers (L2).** Ordinary packages that run at compile time.
   They are sandboxed and have pinned tool versions (§6).
4. **Wrappers (L3).** The pareto one-liners (§7).

## 4. Layer 0: the primitives (complete set)

Every primitive is public. The ones that can break memory safety are callable
only inside `#Unsafe("reason")`. The comptime-only ones are safe. Together they
must express every boundary in G1. If a reader needs something they cannot
express, that is a bug in L0, not a reason for a private hook.

| Primitive | Purpose | Safe? |
|---|---|---|
| `layout<T>(target:)` → `Layout` (size, align, field offsets, bitfields, padding) | Ask the exact per-target layout. Pick a layout with `#Layout(.C | .Packed(n) | .Explicit | .Std430 | .Scalar)`. | yes (comptime) |
| `abi(target:)` → `CallConv` classifier (SysV x86‑64, Win64, AAPCS64, Apple arm64, RISC‑V, wasm32, Swift, Rust‑C, `.Vectorcall`, …) | Shows how each argument is passed (register, stack, sret, split). Used for the Apple arm64 rule that variadic arguments always go on the stack [AP1]. | yes (comptime) |
| `symbol<Sig>(name, from:)` (static), `Library.open(path)` and `lib.symbol<Sig>(name)` (dynamic) | Get the address of a foreign function or global. Weak or optional symbols return `T?`. | link: yes; dynamic: `#Unsafe` |
| `FnPtr<Sig>` call; `call_variadic(fp, fixed, extra: ...CArg)` | Direct call through a typed signature. C default argument promotions are applied by the compiler. `va_list` is a type. | `#Unsafe` unless the row is checked |
| Raw memory: `Ptr<T>`, offset, cast, `alloc_raw` / `free_raw`, `view(ptr, count)` | The memory model, with provenance kept. | `#Unsafe` |
| `thunk(f, conv:)` → `Callback<Sig>` (code pointer + context + owned registration) | Pass closures to C as a (function, user data) pair. `trampoline(f)` covers APIs that take no user data, using a fixed pool. | creating: yes; handing over a raw pointer: `#Unsafe` |
| `capture: [.Errno, .LastError, .NSError, .HResult]` on a call | Read thread-local error state atomically with the call (as in Java `captureCallState`). | yes |
| `unwind: .Abort | .Catch(.Cpp) | .Catch(.RustPanic) | .Propagate` | What happens when a foreign exception reaches the edge. The default is `.Abort`, because unwinding through frames that do not expect it is undefined behaviour (Rust `C-unwind` [R8]). | yes |
| `attach()` and `affinity(.Any | .Main | .Creator)` | Foreign threads enter the runtime automatically through thunks. Affinity is checked when a call is made. | yes |
| `completion<T>()` → (token, Future) and `poll_bridge(handle)` | Turn callback-style or poll-style foreign async into Jet tasks, and the reverse. | yes |
| `Marshal<F>` trait: `lower`, `lift`, `release`, and `BLITTABLE: Bool` | User-extensible conversion. Every built-in projection is a `Marshal` impl. | impl: yes; raw parts: `#Unsafe` |
| `Contract` values: `read`, `merge(overlay)`, `verify(lib)`, `hash` | The L1 data, available at compile time to every package. | yes |
| `isolate(contract, placement: .InProcess | .Wasm | .Process)` | Run the same contract behind a sandbox. Arguments are copied or shared according to their facts. | yes |

Why this set is complete: a boundary needs (a) a shape (layout and ABI),
(b) a target (symbol), (c) a transfer (call, memory, marshal), (d) the reverse
transfer (thunk, attach), (e) the out-of-band channels (error capture, unwind,
async), and (f) a trust decision (contract, isolate). Each primitive covers
exactly one of these.

## 5. Layer 1: the boundary contract

A contract is a list of rows: functions, types, constants and globals. Each
row has a signature plus **facts**. Every fact records where it came from:

| Provenance | Meaning | Callable from safe code? |
|---|---|---|
| `Read` | Stated by the source: a `counted_by`, `_Nonnull`, a Rust `&`, a WIT `own<T>`, Java `@NonNull`, or a `.d.ts` type. | yes |
| `Proved` | Checked by analysis. The C source is visible and the fact holds. | yes |
| `Vouched` | Written by a person inside `#Unsafe("reason") { … }`. Listed by `jet audit`. | yes, at the vouch |
| `Observed` | Asserted at run time by the hardened profile (D-HARDENED1). | adds checks only |
| `Unknown` | Nothing is known. | **only through the conservative projection** (copy in, copy out, opaque handle, `T?`) or isolation |

Fact kinds, one vocabulary for every language:

- **Ownership.** Lend for this call (`&`), give away (`^`), returns owned (with
  a `#Close(fn)` release), returns borrowed from parameter *p*, shared
  (refcounted: ObjC ARC, COM, `Arc`, `shared_ptr`).
- **Lifetime.** A view is valid while *p* lives, until the next call on *p*, or
  only for this call. This is the Swift `@lifetime` and non-escapable model [S2].
- **Length and termination.** `counted_by(n)`, `sized_by(n)`,
  NUL-terminated, sentinel-terminated.
- **Nullability and encoding.** Nullable or not. UTF-8, UTF-16, Latin-1,
  OS-native path, or raw bytes.
- **Errors.** Return code with a success set, status plus out-parameter, errno,
  `GetLastError`, `NSError**`, `HRESULT`, C++ exception, Rust panic, JS throw,
  Python exception. Each maps to a typed `T E!` result.
- **Threads.** Thread-safe, main-thread only, same thread as the creator,
  reentrant or not.
- **Async.** Completion callback (which argument, called how many times),
  promise or future, stream.
- **Effects and purity.** The Jet effects the call performs (`Net`, `FS`, …).
  `pure` allows compile-time evaluation and common-subexpression elimination.
- **Version.** Introduced in, deprecated in, optional symbol.

**Overlays** are the Clang API-notes idea generalised [S5]. A user module can
add or correct facts and rename rows without touching the foreign source. The
overlay wins, and a fact the overlay changes from `Read` to `Vouched` becomes
an audit entry. (In Jet this is `#Import module c.png { … }`, D-FFI-CMAP1 and
D-FFI-VOUCH1.)

**The conservative projection** is what makes "safe by default" real without
blocking work:

| Unknown fact | Safe projection |
|---|---|
| Pointer with no length | Opaque `Handle` (no indexing) |
| Pointer that might be null | `T?` |
| `char*` with no encoding | `Bytes` (copied) |
| Returned pointer with no owner | Borrowed for this call only. It is copied out before the call returns. |
| Callback with no thread fact | Delivered on the caller's executor through a queue |
| Error style unknown | Plain return value, with an audit note |

## 6. Layer 2: readers and writers

Readers and writers are ordinary packages written with comptime and the L0/L1
API. The compiler has no private hooks for them; this is philosophy's *The
Language Is A Library*. They run sandboxed with pinned tool versions, and their
output is cached by content (reader version, inputs, target). The same reader
runs as a build step (`jet bind`) when a team wants to commit or vendor the
result. **One mechanism serves both compile-time import and build-step
generation**, which answers the problem behind Zig's move of `@cImport` into
the build system [Z1].

| Language | Reader input | How a call crosses | Writer output (being called) |
|---|---|---|---|
| C | Headers and source, through a pinned Clang. Constant macros are evaluated. Function-like macros are instantiated as tiny typed C shims. | Direct C ABI | `.h`, plus `counted_by` and nullability annotations |
| C++ | Clang AST. Templates are instantiated on demand in a generated shim. Overloads are folded by label. Virtual classes can be subclassed through vtable thunks. | C ABI shim. Exceptions are caught in the shim. Inlined at O2 when the source is visible. | A C++ header of RAII classes over the C ABI (the cxx/Diplomat pattern [R2,R5]) |
| Objective‑C | Clang plus nullability, `NS_SWIFT_NAME`, ARC rules | Typed `objc_msgSend` per selector. Blocks are thunks. Retain and release map to ownership. | An ObjC header with `NS_*` annotations |
| Swift | `.swiftinterface`, using library-evolution layout rules [SW1] | The Swift calling convention on Apple targets, or `@_cdecl` shims elsewhere | A Swift package (C module plus a Swift overlay) |
| Rust | rustdoc JSON, plus a generated `extern "C"` shim crate. `crabi` when it is stable [R7]. | C ABI. Panics are caught in the shim (`C-unwind`). | A crate with an `unsafe extern` block whose items are marked `safe` [R6], plus safe wrappers |
| Zig | Exported C-ABI declarations | Direct C ABI | A `.h` (Zig consumes C headers) |
| Java/Kotlin | Class files and Kotlin metadata | An embedded JVM through JNI invocation, or a JVM sidecar | FFM bindings (jextract-shaped) on desktop and server, JNI on Android |
| C# / .NET | ECMA‑335 metadata | Hosting through `hostfxr`, or NativeAOT exports | A `LibraryImport` partial class with generated marshallers |
| Python | `.pyi` plus the C-API, HPy, or a supervised worker | In-process (CPython stable ABI), or isolated | A stable-ABI (`abi3`) extension module |
| JS / TS | `.d.ts` | Browser: direct imports. Native: an embedded engine in a wasm component. | Node-API addon, ES module plus wasm, a WIT world for jco |
| wasm components | WIT | Canonical ABI. Async uses WASI 0.3 `future`/`stream` [W2]. | A WIT world and a component |
| GPU | SPIR-V reflection, PTX, WGSL, Metal libraries | Launch descriptors and buffer-binding layouts (std430/scalar). DLPack or Arrow for zero-copy tensors and columns [A1,A2]. | SPIR-V/PTX modules with generated host bindings |
| OS ABIs | Linux syscall tables. winmd (Win32/WinRT/COM) [WM1]. Apple SDK headers and `.tbd` files. | Raw syscalls on Linux only, where that ABI is stable. Otherwise libc, ntdll or libSystem. COM vtables with refcounts mapped to ownership. | Not applicable |

**Embedding when we are called.** The writer emits an `init(capabilities)`
entry and lazy per-thread attach. The capability list comes from one contract,
for both a native library and a component (Jet already ratified this as
D-EMBED2=C).

**Garbage-collected hosts (JVM, CLR, JS, Python).** Foreign references become
owned handles with a `#Close` that releases a global reference. Nothing
collects cycles across two heaps. The writer documents weak handles for back
references, and the hardened profile reports leaked handles.

## 7. Layer 3: the pareto wrappers

| Need (most common first) | Wrapper | Expands to |
|---|---|---|
| Use a C library | `use "png.h" as png` | C reader → contract → lowering. Safe rows are callable, the rest need a vouch or isolation. |
| A few lines of C | an inline foreign body on a `fn` | An anonymous C source unit read by the **same** C reader. The prototype comes from the Jet signature. |
| Fix or rename imported rows | `#Import module c.png { … }` | An L1 overlay |
| Vouch for an unchecked row | `#Unsafe("reason") { fn … = "sym" }` | A `Vouched` fact plus an audit entry |
| Use a Rust, Python, JS or C++ package | `#Import module rust.serde_json`, `use py.numpy`, … | A reader package, a contract, and a placement |
| Sandbox an untrusted library | `use "libxml2.h" as xml, isolate: .Wasm` | `isolate(contract, .Wasm)`. No call site changes. |
| Be called from another language | `#Export` plus `bindings: [c, python, swift]` | Writer packages over the same contract |
| Share tensors or columns | `Tensor.from_dlpack(x)` / `.to_arrow()` | `Marshal` impls over DLPack and Arrow |

Every wrapper shows its expansion through `jet inspect bind <name>`. That
command prints the row, each fact and its provenance, the projection used, and
the primitives it lowered to.

## 8. Cross-cutting rules

**Zero cost (G2).** A crossing whose `Marshal` impls are all `BLITTABLE` is
one call instruction, with no prologue and no runtime transition. Jet has no GC
and no safepoints, so the JDK `critical` and .NET `SuppressGCTransition`
tricks are not needed [J4,N3]. Only thunks entered by foreign threads pay for
an attach check, which is one thread-local load. When C or C++ source is
visible, the pinned Clang emits LLVM IR into the optimizing tier's module, so
the optimizer can inline across languages (the Rust linker-plugin LTO
precedent [R9]). Inlining is an optimization, never a change of meaning (I9).

**Strings and buffers.** A lend (`&`) of `String` or `[T]` passes
pointer plus length with no copy. A NUL-terminated parameter gets a stack copy
only when the Jet value is not already terminated. Returned views are
zero-copy when a lifetime fact links them to a living owner, and copied
otherwise.

**C variadics.** Typed per call site. The compiler applies default promotions
and the ABI's variadic rule; on Apple arm64 every variadic argument goes on the
stack [AP1]. `printf`-style rows with a `format` attribute are format-checked
at compile time.

**Macros.** Constant macros are evaluated by Clang. A function-like macro
becomes a typed shim, `static inline T name(params) { return MACRO(params); }`,
compiled by the same Clang. Its parameter types come from an overlay, or are
inferred when the macro body is a single call. A statement macro that cannot
be typed is reported, not guessed.

**C++.** Classes are owned handles whose destructor runs as `close`. Movable
types map to `^`. Templates are instantiated on demand in a shim; explicit
`instantiate:` is for libraries with no visible use. Overloads fold into labels,
and ambiguity is a compile error. `std::string_view` and `std::span` become
lifetime-bound views. `unique_ptr` maps to an owned value, `shared_ptr` to a
shared one. Exceptions become `T CppError!`.

**Callbacks.** A closure lowers to a (code, context) pair. Registration returns
an owned token, and dropping the token unregisters it. A callback that a C API
keeps beyond the call needs a `retained` fact: either `Read` (with a matching
unregister) or `Vouched`.

**Threads.** Main-thread-only APIs (UIKit, AppKit, most GUI toolkits) carry
`affinity(.Main)`, and calling them off the main thread is a compile error
where it can be proved and a checked trap otherwise. Foreign threads that enter
Jet are attached on first entry and detached at thread exit.

**Async.** Callback-style APIs turn into `async` functions through
`completion<T>()`. A Jet `async` export becomes the host's native form: a JS
Promise, a Swift `async` function, a Kotlin `suspend` function, a .NET `Task`,
or a WIT `future<T>` [W2].

**Versioning.** Every contract has a hash, and the bound library is verified
at link or load time (symbols present, interface hash equal). `jet build`
diffs the contract against the last frozen one (`api: stable`) and reports
breaking changes. Foreign structs whose size can change between versions are
accessed through generated accessors, so an upgrade cannot shift field offsets
under compiled code (Swift library evolution [SW1]). Optional symbols are `T?`.

**Debugging.** Every tier emits DWARF, so backtraces mix languages. `--trace-ffi=png.*`
logs each crossing with its lowered arguments and results.
The hardened profile checks vouched facts at the edges. An isolated placement
turns the boundary into a message boundary, which can be recorded and replayed.

## 9. Adoption analysis: the design mapped onto Jet

The verdicts are: **Fits** (already ratified, or implementation under a ratified
rule), **Extends** (new capability that is consistent with ratified rules), and
**Replaces** (changes a ratified rule; needs an owner ballot).

| Idea | Jet today | Verdict | Migration | Owner ballot |
|---|---|---|---|---|
| Contract plus readers as packages | D-FFI-UNIFY1, D-FFI-BINDER1 (23 readers, about 22.8k lines), D-CIMPORT1 (pinned Clang), the `jet-ffi-descriptor-v1` stamp | Fits; the descriptor grows into the full fact vocabulary | Medium: descriptor schema v2, readers fill in more facts | none (implementation) |
| Facts with provenance; conservative projection | D-FFI-CMAP1 (nullable → `T?`, `counted_by` → lists), D-FFI-VOUCH1 (vouch blocks, `jet audit`), D-HARDENED1 | Fits; `Proved` and `Observed` become explicit labels | Small | none |
| Ownership across the border | D-FFI-CAP1 (`&`, `^`, `#Close` at the border) | Fits exactly | none | none |
| Public primitives layer (§4) | D-RAWPTR2 raw pointers; `jet_builtins` (D-CORE-BUILTINS1); readers use `foreign.read`; layout and ABI queries, dynamic symbols, thunks, error capture and `Marshal` are compiler-private or missing | **Extends** | Medium: about 12 primitives, most of them already exist internally | **D-FFI-PRIMS1** |
| Inline C as a wrapper over C-source import | D-FFI-ONE1=A keeps inline `#FFI(c)` bodies with the prototype written from the Jet signature. Its recorded loss is "two ways to bring in C". | Fits. The inline body is lowered as an anonymous unit through the D-FFI-CINPUT1 reader, so there is one mechanism and the recorded loss goes away. Syntax is unchanged. | Small, compiler-internal | none: implementation under I8 |
| `#Unsafe` and I1 | The only escape hatch. Vouches name a reason. | Fits. Isolation adds a third safe route that needs no vouch. | none | (see the next row) |
| Isolation as a placement option | Python and JS already run in supervised workers or JS-on-wasm (D-FFI-JS1). Plugins are sandboxed components (D-PLUGIN*). C libraries are in-process only. | **Extends** | Medium: a wasm-compiled C path through the pinned Clang (`wasm32-wasip2`), plus a process host | **D-FFI-ISOLATE1** |
| One meaning across tiers (I9; D-EXEC1, amended by D-EXEC3) | The comptime evaluator refuses foreign calls by name | Extends. `pure` rows run at compile time through the wasm placement, so they are deterministic and sandboxed. The web tier compiles the same C to wasm32. | Comes with the isolation work | covered by D-FFI-ISOLATE1 |
| Wasm and web | D-PORTABLE-WEB1, D-WASISRV1, WIT for plugins, D-EMBED2=C | Fits. WIT becomes one reader and one writer of the contract. WASI 0.3 async maps onto `completion`. | Small | none |
| Rich exports (being called) | `#Export(c)` allows C primitives only (spec, Embedding artifacts). D-EMBED2=C lists `bindings: [c, python]`. | **Extends**: records, lists, strings, errors, callbacks, handles and async, projected per language by writer packages | Medium to large: one writer per language, C first | **D-FFI-EXPORT2** |
| Zero-cost and cross-language inlining | O2 goes through LLVM. D-EXEC3 limits runtime-function inlining to the runtime's own source. | Fits. Visible C and C++ source is lowered to LLVM IR in the O2 module. | Small to medium | none |
| GPU | D-ACCEL-LIB1 (devices as libraries with CPU reference), D-GPU-COMPUTE-LAUNCH1, D-GPU-DEVICE-OPS1 | Fits. Adds std430/scalar layouts to `layout`, a SPIR-V/PTX reflection reader, and DLPack/Arrow `Marshal` impls. | Medium | none now |
| C++ depth | D-FFI-CPP1 (opaque classes, `CppError`, on-demand templates) | Fits. Adds lifetime-bound views and vtable subclassing. | Medium | none |
| Versioning | `api: stable` (D-CAP4), descriptor stamp, E3208 | Fits. Adds load-time verification and resilient accessors. | Small | none |
| Debugging | `jet inspect bind`, D-HARDENED1 | Fits. Adds `--trace-ffi` and mixed DWARF backtraces. | Small | none |

**Net effect on ratified decisions.** Nothing ratified is reversed. D-FFI-ONE1's
recorded loss goes away through implementation. Three ballots add capability:
D-FFI-PRIMS1, D-FFI-ISOLATE1 and D-FFI-EXPORT2. The ballot JSON files are in the
lane's `ballot/` directory.

## 10. Sources

- [Z1] Zig 0.16 release notes, "@cImport Moving to Build System": <https://ziglang.org/download/0.16.0/release-notes.html>
- [Z2] ziglang/zig#20630, "move `@cImport` to the build system": <https://github.com/ziglang/zig/issues/20630>
- [R1] rust-bindgen guide: <https://rust-lang.github.io/rust-bindgen/>
- [R2] cxx, safe interop between Rust and C++: <https://cxx.rs/>
- [R3] UniFFI user guide: <https://mozilla.github.io/uniffi-rs/>
- [R4] PyO3 user guide: <https://pyo3.rs/>
- [R5] The Diplomat Book: <https://rust-diplomat.github.io/diplomat/intro.html>; M. Goregaokar, "Diplomat: Multi-language FFI for Rust Libraries" (2026): <http://manishearth.github.io/blog/2026/06/14/diplomat-multi-language-ffi-for-rust-libraries>
- [R6] Rust 2024 edition guide, "Unsafe extern blocks": <https://doc.rust-lang.org/nightly/edition-guide/rust-2024/unsafe-extern.html>
- [R7] rust-lang/rust#111423, tracking issue for the experimental `crabi` ABI: <https://github.com/rust-lang/rust/issues/111423>
- [R8] RFC 2945, `C-unwind` ABI: <https://rust-lang.github.io/rfcs/2945-c-unwind-abi.html>
- [R9] rustc book, linker-plugin LTO (cross-language LTO): <https://doc.rust-lang.org/rustc/linker-plugin-lto.html>
- [S1] Swift.org, "Mixing Swift and C++": <https://www.swift.org/documentation/cxx-interop>
- [S2] Swift.org, "Safely Mixing Swift and C/C++": <https://www.swift.org/documentation/cxx-interop/safe-interop>; WWDC25 "Safely mix C, C++, and Swift": <https://developer.apple.com/videos/play/wwdc2025/311>
- [S3] Swift Forums pitch, "Safe and efficient C++ interoperability via non-escapable types and lifetimes": <https://forums.swift.org/t/safe-and-efficient-c-interoperability-via-non-escapable-types-and-lifetimes/75426>
- [S4] Swift.org, "Supported Features and Constraints of C++ Interoperability": <https://www.swift.org/documentation/cxx-interop/status/>
- [S5] Clang, API Notes: <https://clang.llvm.org/docs/APINotes.html>
- [SW1] Swift.org blog, "Library Evolution in Swift": <https://www.swift.org/blog/library-evolution/>
- [K1] Kotlin, "Interoperability with C": <https://kotlinlang.org/docs/native-c-interop.html>
- [K2] Kotlin, "Definition file": <https://kotlinlang.org/docs/native-definition-file.html>
- [K3] Kotlin, "Interoperability with Swift using Swift export": <https://kotlinlang.org/docs/native-swift-export.html>
- [N1] .NET, "P/Invoke source generation": <https://learn.microsoft.com/en-us/dotnet/standard/native-interop/pinvoke-source-generation>
- [N2] .NET, "Custom marshalling source generation": <https://learn.microsoft.com/en-us/dotnet/standard/native-interop/custom-marshalling-source-generation>
- [N3] .NET `SuppressGCTransitionAttribute`: <https://learn.microsoft.com/en-us/dotnet/api/system.runtime.interopservices.suppressgctransitionattribute>
- [J1] JEP 454, Foreign Function & Memory API: <https://openjdk.org/jeps/454>
- [J2] Oracle, "Calling Native Functions with jextract": <https://docs.oracle.com/en/java/javase/24/core/call-native-functions-jextract.html>
- [J3] `Linker.Option.captureCallState`: <https://docs.oracle.com/en/java/javase/22/docs/api/java.base/java/lang/foreign/Linker.Option.html>
- [J4] `Linker.Option.critical` (same page as J3)
- [L1] LuaJIT FFI library: <https://luajit.org/ext_ffi.html>
- [P1] CFFI documentation (ABI and API modes): <https://cffi.readthedocs.io/>
- [P2] Python `ctypes`: <https://docs.python.org/3/library/ctypes.html>
- [M1] Nim manual, `importc` and `importcpp`: <https://nim-lang.org/docs/manual.html#implementation-specific-pragmas-importcpp-pragma>
- [O1] Odin overview, foreign system: <https://odin-lang.org/docs/overview/#foreign-system>
- [JA1] "The Way to Jai", chapter on `#foreign` (community book; Jai has no public spec): <https://github.com/Ivo-Balbaert/The_Way_to_Jai>
- [D1] D, ImportC: <https://dlang.org/spec/importc.html>
- [D2] D, Interfacing to C++: <https://dlang.org/spec/cpp_interface.html>
- [C1] Carbon, "Bidirectional interoperability with C and C++": <https://docs.carbon-lang.dev/docs/design/interoperability>
- [C2] Carbon proposal #6358, "C++ Interop: API importing and semantics": <https://docs.carbon-lang.dev/proposals/p006358-c-interop-api-importing-and-semantics.html>
- [MO1] Mojo, "Calling Mojo from Python" (`PythonModuleBuilder`): <https://mojolang.org/docs/manual/python/mojo-from-python/>
- [G1] GraalVM, Polyglot Programming: <https://www.graalvm.org/latest/reference-manual/polyglot-programming/>
- [W1] Component Model, WIT reference: <https://component-model.bytecodealliance.org/design/wit.html>
- [W2] Component Model, "Async, Streams, and Futures" (WASI 0.3): <https://component-model.bytecodealliance.org/design/async.html>
- [W3] Bytecode Alliance, "WASI 0.3 Launched": <https://bytecodealliance.org/articles/WASI-0.3>
- [WM1] microsoft/win32metadata: <https://github.com/microsoft/win32metadata>
- [RB1] RLBox: <https://rlbox.dev/>
- [A1] Apache Arrow C Data Interface: <https://arrow.apache.org/docs/format/CDataInterface.html>
- [A2] DLPack: <https://dmlc.github.io/dlpack/latest/>
- [B1] Clang `-fbounds-safety`: <https://clang.llvm.org/docs/BoundsSafety.html>
- [AP1] Apple, "Writing ARM64 code for Apple platforms" (variadic arguments): <https://developer.apple.com/documentation/xcode/writing-arm64-code-for-apple-platforms>
- Jet: [`Docs/audits/ffi-owned-source-and-boundaries.md`](../audits/ffi-owned-source-and-boundaries.md), [`Docs/research/cpp-interop.md`](../research/cpp-interop.md), [`Docs/research/2026-10-30-gpu-one-meaning.md`](../research/2026-10-30-gpu-one-meaning.md), [`Docs/proposals/plugin-wasm-engine-2026-10-29.md`](plugin-wasm-engine-2026-10-29.md), and spec.md "Foreign code" and "Embedding artifacts".
