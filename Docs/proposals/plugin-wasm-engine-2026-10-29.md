# Plugin WASM engine: Component guests through JetIR (#5487)

Status: design note, 2026-10-29. It is tied to the owner decision of
2026-10-23 recorded on #5487: replace wasmtime (RustBridge
`Runtime/Plugin.rs`, `Runtime/CompilerExtension.rs`) with our own pipeline.
A loader reads the Component Model guest's core wasm, translates it into
JetIR with sandbox guarantees, compiles it with the x64 backend, and runs it
in memory through the native loader. Section 7 lists the questions that need
an owner answer. Tower owns the batch plan.

The host contract does not change. The authority preflight, budgets, the
`PluginError` carrier, frames, handle lifetime and the compiler-extension
protocol stay as `Docs/spec/architecture.md` describes them ("Application
plugin host boundary" and "Compiler-extension plugins"). This note covers only
what replaces wasmtime underneath that contract.

## 1. Consumers and where translation runs

The engine translates and compiles a guest before it runs it. Each consumer
already has a compiler available at that point:

| Consumer | Artifact known | Translation runs in |
|---|---|---|
| `core.plugin.load("literal.wasm"/".wat", authority)` | at build time (sema requires a literal artifact and its frozen `.api` interface) | `jet build` / `jet run`. The translated guest image ships with the program |
| Compiler extensions (`CompilerExtension.jet`, `compiler-extension-v1`) | when `jet` starts the extension | the `jet` process |
| Jetpack `BuildPluginHost` / `CompilerExtensionHost` (runtime path, digest-pinned, #5487 log U7) | when the host runs | the `jet` process, which is the Jetpack CLI |

So no user program has to carry the compiler. A runtime-path
`plugin.load<Guest>(path, …, sha256:)` in an ordinary user program, which
nothing ships today, would need the backend at run time. That stays out of
scope until a ballot asks for it.

The compiler doesn't derive the interface from the component. Sema keeps
reading the frozen `.api` snapshot (`JET_API_CACHE_DIR` / `.jet/cache/api`).
At load, the engine checks that the component's exports match that snapshot,
by name and by canonical type. A mismatch is a load fault (`export == "load"`)
and never a call-time surprise.

## 2. Execution model

The translated guest is a static x64 image. It has its own `main`, a dispatch
loop that reads call frames from an inherited descriptor and writes results
back. The host starts it with the in-process loader's spawn path
(`Compiler/JetBackend/Source/Image/Loader.jet` `x64_image_spawn`: memfd, fork,
and the executor runs the image, so nothing touches disk). A release binary
starts the same image bytes with `fexecve` on a memfd. One plugin handle is
one child process plus its two pipes.

Why a separate process instead of mapping guest code into the host:

- Isolation has two layers. The translator's software checks (section 4) are
  the sandbox. The process boundary is defense in depth: a translator bug
  can't scribble on host memory.
- A guest that fails stays scoped to its own instance, and the host survives
  stack exhaustion or a backend defect in the guest without signal handling
  in the host runtime.
- The wall-clock budget is exact. When the deadline passes, the host kills
  the child and reports `Budget(Time)`. Translated code needs no epoch
  polling.
- Handles stay owner-thread state. `close` kills the child and reaps it.

The cost is one round trip over a pipe per call, measured in microseconds.
That's acceptable for the plugin and compiler-extension call rates. If a ballot
later wants in-process calls, the section 4 checks already make in-process
execution sound. Only the deadline would need translated polls.

The child runs with a seccomp filter: read/write on its two descriptors,
`exit_group`, and nothing else. Linear memory is preallocated at
`min(declared max, 16 MiB)` before the filter goes on, so `memory.grow` never
needs a system call.

## 3. Accepted input

A guest is a Component Model binary (`\0asm`, layer 1) or its text form. The
loader accepts text directly (D-PLUGIN-FAILURE1 fixtures, the
`tests/cases/authority/*.wat` fixtures). The text front end produces the same
in-memory component as the binary decoder. The supported subset is exactly
what the two worlds and today's fixtures use. Anything else fails the load
with a `Guest` fault that names the construct:

- **Component sections:** core module, core instance (instantiate with
  `with` arguments, or inline export bundles), core alias export, type
  (function types), import (functions only), canon `lift`/`lower` with
  `memory`, `realloc`, `post-return` and `string-encoding=utf8`, and export
  (functions). No nested components, resources (`own`/`borrow`), async,
  futures, or streams. At most 4 core instances, 1 memory and 1 table, as
  today (`PLUGIN_MAX_CORE_INSTANCES`).
- **Value types:** bool, s8–s64, u8–u64, f32, f64, char, string, list,
  record, tuple, variant, enum, option, result and flags. These are the same
  types `Plugin.rs` admits today.
- **Core wasm:** wasm 1.0 with mutable globals, sign extension, saturating
  float-to-int, multi-value, and bulk memory (`memory.copy`, `memory.fill`,
  plus passive data with `memory.init`/`data.drop`), because rustc and clang
  emit these by default. SIMD, threads, exceptions, GC, tail calls, memory64,
  multi-memory, and reference types beyond one funcref table are rejected.
- **Validation:** the full core validation algorithm (operand stack types,
  control frames, and immediates against the module's index spaces) runs
  before translation. The translator relies on validated input and never
  re-checks it.

## 4. Translation to JetIR and the sandbox obligations

JetIR's structured control (`Block`/`Loop`/`If`/`Br`/`BrIf`, `IR.jet`)
follows wasm's directly. The translator gives each wasm function a JetIR
function and keeps the operand stack abstract, so every stack position
becomes a slot. Locals and globals become slots and data-segment cells. The
following are proof obligations. Each one has a named adversarial test in the
corpus (section 6).

1. **Memory.** Every load and store computes `ea = zext(addr) + offset`, a
   64-bit add that can't overflow, and traps unless `ea + size <= current_len`.
   `memory.grow` past the cap traps with `Budget(Memory)`, carrying frames,
   which matches today's fixture expectation; it doesn't return -1.
   `memory.copy`/`fill`/`init` check the whole range before writing.
2. **Indirect calls.** `call_indirect` checks the index against the table
   length, rejects a null entry, and compares the entry's canonical signature
   id with the expected one before the call. Table growth past 10,000 elements
   traps with `Budget(Table)`.
3. **Fuel.** Each function entry and each loop back edge subtracts the static
   instruction count of the straight-line run it starts. When the counter goes
   negative, the guest traps with `Budget(Fuel)`. The default stays 10,000,000.
4. **Stack.** A call-depth counter traps before the native stack can
   overflow. The trap is classified `Guest`, as wasmtime's stack exhaustion is
   today.
5. **No ambient effects.** Translated code calls only the guest runtime shim:
   the import trampolines, trap, and the dispatch loop. The shim performs no
   effect itself. Every import is a message to the host, which preflights it
   as a `HostImportFact` against the declared needs and the lent `Authority`,
   then performs it (`read`/`write` → `FS.Read`/`FS.Write`, `connect`/`request`
   → `Net.Connect`, `run`/`exec` → `Exec`, the same mapping as
   `plugin_import_right`).
6. **Traps.** `unreachable`, integer divide by zero, signed overflow on
   division, invalid float-to-int conversion, out-of-bounds access, a bad
   indirect call, and stack exhaustion each end the call with a trap code.
   A wasm trap never becomes a host signal.
7. **Frames.** The translator keeps a shadow frame stack of
   (function index, wasm byte offset), updated at call sites and trap
   points. The trap path copies up to 32 innermost frames. Function names come
   from the name section (empty when absent) and component names from the
   component's core-module names. This gives today's
   `PluginFrame { function, component, offset }`.

Values cross the boundary as the existing host wire. The host lowers
arguments into guest memory through the guest's `realloc`, using the
canonical ABI with utf8 strings and the flat/indirect parameter rule (at most
16 flat params, at most 1 flat result, otherwise through memory). It lifts
results after the call and before `post_return`. The 16 MiB wire limit and
nesting depth 64 are checked before encoding, as now.

### JetIR additions

Wasm needs operations JetIR doesn't have today:

- integer: `clz`, `ctz`, `popcnt`, `rotl`, `rotr`;
- float: `sqrt`, `abs`, `neg`, `ceil`, `floor`, `trunc`, `nearest`, `min`/`max`
  with wasm NaN rules, `copysign`;
- bit reinterpretation between same-width ints and floats;
- trapping and saturating float-to-int conversion;
- a multi-way branch for `br_table`.

The plan is to add them as `IROp`s with X64Select lowerings: popcnt, lzcnt,
tzcnt, rol, ror, roundsd, sqrtsd and movq. Using asm leaves instead would
leave every one of them as an opaque call. They also serve Jet's own integer
and float library, which reaches several of these today through runtime calls.

## 5. Cutover

When the engine passes the corpus, one change routes `jet_plugin_load`,
`jet_plugin_call` and `jet_plugin_close`, and the `jet_compiler_extension_*`
loader, to it. The same change deletes `Runtime/Plugin.rs`,
`Runtime/CompilerExtension.rs`, `Support/jet-foundation` (Authority,
PluginWire, RootedFiles), their `RuntimeProviders` embeds, and the wasmtime
crate pin. It also removes the wasmtime wording from
`Docs/spec/architecture.md`.

## 6. Test corpus

- **Examples:** `Examples/features/packages/sandbox_mathkit`,
  `plugin_failure`, `library_loadable`, and `foundations/capabilities` (with
  its `hostile` guest), on O0/O1/O2.
- **Authority cases:** the seven `tests/cases/authority/plugin_*` cases on
  `jet run` and `--release`.
- **Compiler extensions:** `tests/fixtures/compiler_extension/*`
  (`crash`, `fuel_loop`, `imports_random`, `incompatible`, `lint_no_x`,
  `malformed`) and the `compiler_extension_*` ui rows.
- **E1257 and E1260 ui rows unchanged.** Both are sema diagnostics; E1258 and
  E1259 are retired.
- **Spec testsuite subset:** the WebAssembly spec `.wast` files for the
  accepted core features (i32, i64, f32, f64, conversions, int_exprs,
  float_exprs, memory, memory_grow, br_table, call_indirect, loop, block, if,
  local_*, global, select, traps, bulk memory). They run through a `.wast`
  runner in the engine's test package, which uses the same text front end.
- **Adversarial guests:** out-of-bounds load/store at `2^32 - 1 + offset`; a
  runaway loop (Fuel); infinite recursion (stack); a table index out of range
  and a signature mismatch; `memory.grow` past the cap; a guest that never
  returns from a host import; and malformed binaries (truncated sections, bad
  LEB128, out-of-range indexes). Each must be contained with the
  classification section 4 states.

## 7. Questions for the owner

1. **Process boundary (section 2).** The proposal runs each guest as a
   loader-spawned child, not mapped into the host. The decision text says
   "runs it in memory via the native O0 loader". The spawn path is in memory
   (memfd, nothing on disk), but the guest runs in a separate process.
2. **Runtime-path loading in user programs (section 1).** It stays out of
   scope, because it would put the backend in every program that uses
   `core.plugin`.
