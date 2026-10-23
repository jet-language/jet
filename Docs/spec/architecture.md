# Architecture

Vocabulary: [Jet vocabulary](vocabulary.md).

## Pipeline

```text
Jet source
  -> lexer -> tokens with byte spans
  -> parser -> AST
  -> sema -> checked bundle and semantic facts
  -> TIR -> canonical, validated and optimized MIR
  -> Jet code generator (Compiler/JetBackend), one runtime
       O0: machine code in memory (`jet run`, `jet dev`, comptime, REPL)
       O1: executable file (`jet build` default)
       O2: executable file (`--release`)
```

This is the ratified shape (D-EXEC1, D-TIER-FORM1, D-BACKEND-OWN1). Every
level reads the same Jet-owned MIR and calls one runtime: the Prelude and Core,
compiled once per Jet version and shipped with Jet. O0, O1 and O2 are all the
Jet back end; LLVM survives only as an explicit, measured `--backend=llvm`
fallback until Jet O2 beats it on every required cell and covers every
supported target (card #4013). The Cranelift JIT is retired (D-EXEC2).

Rust emission (MIRRust -> rustc) left with the Rust implementation (owner
directive 19): `jet build`, `jet run` and `jet dev` compile through the native
back end. The MIR evaluator is a transitional path, not a consumer of the
design; it serves comptime only until native O0 comptime lands.

Bootstrap MIR optimization verifies legality in both development and release
builds. `JET_DEBUG_LOWER_ALL=1` additionally reports all global identity
collisions and independent legality checks for each function. An invalid input
still stops the pipeline. If a pass introduces invalid MIR, measurement mode
reports its operation/pass and structured violations, discards its entire
output, and runs the following passes from the last valid program. It retains
only one snapshot at a time and ultimately returns the original first failure;
no rejected pipeline is sealed or emitted. Normal mode remains fail-fast and
takes no snapshots. Checks that require valid component prerequisites still
reject that component rather than reading invalid references. With `JET_DUMP_MIR`
also set, the checked lowering pipeline saves the pre-verification program at
`<dump-path>.before-legality`; the usual post-optimization dump remains unchanged.

The native runtime object's `jet_rt_alloc` and `jet_rt_free` own C-carrier
allocation. Generated Rust adapters allocate List result buffers through that
same allocator because native List paths release them with `jet_rt_free`.
Native adapters import the Jet definitions and consumers retain the object's
allocator `.init_array` entry. Rust-tier allocator bodies remain only as
ledgered seed/verification paths that die at Rust cutover; native packs never
link those bodies.

Internal runtime C symbols use the acronym-aware snake projection owned by
`Tools/agent/runtime-abi-symbols.mjs`; the route generator emits that same
projection for Lower. For example, the `HTTPClient` symbol atom is
`http_client`. Runtime type names, declaration keys, variant identities and
layout metadata remain nominal: only the ABI symbol atoms are projected.
Isolated runtime-generator fixtures copy that shared projection module alongside
the real generator and registry inputs; they do not replace its module boundary.
Providers, generated adapters and compiler callers use one spelling, with
collision rejection before a symbol cutover.
Inside the runtime's assembled source unit, calls use existing Jet provider
definitions directly instead of importing their own exported C addresses.
External C addresses retain one declaration owner, including the libc errno
seam shared by resolver and process code. This preserves strict C-symbol
collision admission; it does not grant import/export aliases.

Bootstrap Rust nominal carriers use their exact Prelude declaration spelling,
including acronym case; the ABI atom projection never supplies a type name.
Jet's checked consuming `Debug` implementation retains its owned body. The
borrowed Rust formatting protocol invokes that body through a private adapter,
copying the owner only when its checked Clone capability permits it.

Native TCP providers use canonical Socket56/Stream56/Listener8 carriers and the
shared nonblocking FD72/poller seam. Their syscall error scratch is ErrorOut32;
the positive errno is at byte 8, not the domain/stage word at byte 0. Private
route Word parameters/results are raw i64 (including millisecond timeouts and
TTL), while the independent exact-Int carrier helpers retain tagged ownership.
Happy Eyeballs preserves resolver order within each family, alternates families
starting with the resolver's first family, staggers attempts by 250ms, and
accelerates the next launch after failure. Losing descriptors are dropped on
success, cancellation, or deadline; native C witnesses link the actual composed
runtime rather than mock socket providers.
`Compiler/JetRuntime/Tests/run-native-tcp-routes.mjs OUT STAGE2_JET RC_RUNTIME_DIR`
consumes the first RC pack's admitted standalone object without rebuilding the
runtime. It compiles the C witness fresh, binds the U8 stride to the pack's
source identity, and records admission, compile, native link, and run receipts.

The native [Duration constructor](../../Compiler/JetRuntime/Source/Duration.jet)
borrows the exact-Int input, checks narrowing and nanosecond scaling without a
panic, and writes only the selected owned Duration or RangeError result. It uses
the shared time/text carriers. The compiler's stopwatch timing follows this
same route; its range and six-unit ABI [fixture](../../Compiler/JetRuntime/Tests/duration-fixture.c)
links only the admitted Jet runtime pack.

Native runtime cutover packaging checks the pinned compiler with the stage0
front end before declaring the cutover READY, in addition to the narrow trusted
syntax parse and manifest checks. Record the frontend command's own exit and
diagnostic summary; syntax-only receipts do not establish borrow correctness.
A held stage0-only job being READY means it is prepared for scheduling, not
that its semantic check passed. In Codegen poll and comptime-budget emission,
`span := builder.current_span` snapshots the source span before write-access to
`&builder`; `span ::` would retain a builder view across those mutations.

Runtime process-edge fixtures decode the committed Trap/Scheduler report words
as signed little-endian I64 values through one
[`report-literals` reader](../../Compiler/JetRuntime/Tests/report-literals.mjs).
The scheduler fixture writes its complete root-panic golden beside its proof
receipts, checks exact stderr, exit 70, no signal and no stdout, and does not
depend on a Rust-tier reference producer. Task-local panics still require
process survival and empty stderr.
An aggregate fixture object already contains the full Jet runtime and links
without a second provider. Standalone C witnesses use `jet_runtime_jet.o`
exactly once; the startup-bearing `jet_runtime.pack.o` is a program-link input,
not an additional runtime object for a C program that defines its own `main`.
Native scan proofs require the current native `scan.cases` receipt and the
fresh Jet object/pack. Saved reference and evaluator TSVs are optional,
explicitly named `--frozen-reference` and `--frozen-evaluator` inputs; a live
retired tier is never a prerequisite for the raw ABI and Float-bit witnesses.
Official BLAKE3 fixtures regenerate from the team vectors alone, outside the
immutable proof source, without reading a retired runtime implementation.
Native runtime exit corpora accept `--results` pointing to the one Jet-front
golden union's `results.tsv`; symbol, raw ABI, ownership and process-edge
witnesses still execute independently. Shared results reject duplicate rows.
Trap proofs compare the native status, signal and exact stdout/stderr against
the committed golden, not a retired MIR/Rust execution. The freestanding
display witness links the sealed C-client object and its provenance-listed C
platform archives, rather than the program pack's startup.
Native fixture and registry feeds share
[`compiler-native-transport`](../../Tools/agent/compiler-native-transport.mjs):
the production CLI's `check FILE` library-object seam and `build FILE` program
path. Library feeds require fresh object, export and ABI files; command
completion alone is not a native pass. Checked-MIR observers are explicit Jet
drivers compiled from the canonical compiler aggregate, cached per pinned
toolchain, rather than a bootstrap factory or a retired Rust observer.
The [S5 drivers](../../Tools/agent/s5/make-jobs.sh) execute from the immutable
source export. Queue completion is not a pass: consumers require the real
`driver.rc`, a completed same-pin phase receipt and its decisive PASS rows.
Receipts are published atomically; interruption preserves partial evidence
and records a driver failure. A pack publication binds its runtime key and
digest to `native.key` and the selected object. Prepared golden corpora are
hash-checked and consumed without rerunning preparation.
`S5_SHARED_HOST_JOB` and `S5_SHARED_PACK_JOB` select the producer job names
when drafting holds; artifact schemas and source-pin checks remain unchanged.

The compiler frontend receipt must retain the full foreign Cargo diagnostics,
including cached bridge fingerprint output when an inline-FFI error hides
Cargo's stderr. `os_syscall_stack4_i64` saves and restores the six argument
registers just like the loader's `os_syscall` and runtime's `os_syscall_i64`;
its seven generic inputs then need only the return register and two syscall
clobbers, rather than sixteen simultaneous general-purpose register slots.
Before scheduling a cutover job, syntax-check its shell script and check every
referenced input script/path exists. Outputs created by earlier steps are
identified separately; the runtime assembly driver is
`Compiler/JetBackend/Tests/run-runtime.mjs`, not a JetRuntime test script.

The bootstrap completion owner may initiate Source resource retirement after
its caller consumes the factory result and grants retirement permission; a
prior request is not required. Callback jobs and retained roots still block
retirement. The hosted runtime library-object feed instead consumes the
driver's checked MIR directly; it has no factory frontend receipt dependency.
Lowering and linking failures refuse the pack rather than publishing an artifact.

The Rust-debug MIR conversion feed preserves the bootstrap codec's field-name
mapping for all NameLedger rows, including display paths: the checked Rust
`NameDisplayPath.module` becomes Jet `module_index`. Internal and display
paths remain distinct checked strings; conversion must not infer or discard
either path.

The native C-ABI fixtures declare each allocator export once and link the
same checked Jet runtime object that packaging uses. Actual lowered signature
layouts are recorded in `runtime.abi.json`; its symbols must match the checked
export inventory. Compiler crashes and failed ABI witnesses remain failed
evidence, never a native-runtime semantic verdict.

Runtime-pack roots are the public defined C-ABI globals of standalone Jet
runtime objects and explicit extra roots. The Jet object writer publishes
checked exports as globals and private functions as locals. Undefined symbols,
hidden/internal definitions and implementation names do not become export
roots; C archive members pulled to satisfy exports remain private dependencies.
Rust-mangled symbols and Rust allocator providers are rejected. A pack requires
a standalone runtime object, never just archives.
`Compiler/JetBackend/Tests/run-runtime-pack-roots.mjs` exercises this boundary
with real objects, an archive and a consumer, including an unreferenced export.

`Tools/packaging/prebuild-runtime.sh <hosted-jet>` assembles the canonical
`Compiler/JetRuntime/sources.list`, checks it through the Jet-hosted CLI's
library entry, and lowers its checked exports plus `lifecycle.list` into one
native object. Jet's linker resolves its permitted musl/libgcc C dependencies;
no Rust runtime, standard-library rlibs, rustc or system linker participates.
Packaging writes `jet-runtime/native.key` beside the compiler and installs
`jet-runtime/<key>/jet_runtime.pack.o`. The key hashes the source inventory,
checked ABI, lifecycle, backend inputs and C platform archives. The accompanying
`provenance.json` records each input digest, the pack digest and zero Rust or
LLVM-generated Jet objects, with permitted C platform archives listed separately.
Packaging first checks a separate recipe digest of the runtime sources, backend,
lifecycle, C archive bytes and compiler executable. Neither the consumer program
nor its build profile enters that recipe. A matching recipe reuses the produced
pack without checking or compiling the runtime again; changed producer inputs
rebuild it. Reuse requires held regular-file reads, matching object/pack/ABI
digests, the checked export inventory, the content-addressed identity and the
existing no-Rust-symbol check. Native compiler staging uses the same receipt
reader. This cache does not bypass semantic checks on user programs.

`RuntimeRoutes.jet` is the native lowering table of runtime C-ABI routes and
its only home: its route, record and import rows are edited by hand, sorted,
together with the runtime `#Export(c)` they name. After provider admission
changes, run `Tools/agent/lane-check.sh --runtime-generators` to check the
generated runtime message tables.

Native `List<U8>` payloads are packed one-byte elements, matching checked
`StorageLayout(U8)` and the C ABI's `u8` buffer. The runtime's
`RT_HOST_U8_STRIDE` is 1; byte-list allocation, indexing, copying and freeing
share that fact across Dyn, files, networking, text, crypto and codecs.
HTTP Body chunk buffers use the same layout, including borrowed input windows.
`run-native-codec-file-leaves.mjs` gates the admitted RC object with a C client
that checks all 256 byte values through List pack/unpack and multi-chunk HTTP
Body transfer; it rejects an object whose pinned source declares another stride.

A nested `List<List<U8>>` crosses a route only to a Jet runtime provider: the
outer data pointer and length address native 24-byte `[len, cap, data]` headers
over packed bytes, and a result returns such a buffer with exact capacities.
`&mut Vec<u8>` kernels likewise lend the caller's own data and length for an
in-place write. These rows name Jet runtime providers only, and Lower refuses
a nested carrier without `native_lists`.
The checked-build transport (`HostBuildHTTP.jet`, HTTP.rs:5536-5712) uses this
carrier for its DNS answers and response rows; plain `http://` runs over the
native socket and readiness providers, while the `https://` branch connects and
then returns the runtime's canonical TLS refusal text (D-NATIVE-TLS1=B) as its
String error without sending a request.

Held-directory listing reuses the one FD72-backed held-directory table. Each
call reads a fresh stream, returns packed raw filename rows with a no-follow
directory-kind byte, and transfers the nested List headers and buffers as
native owners. Terminal read-all serializes stdin reads with its lifecycle
mutex, assembles complete UTF-8 across syscall chunk boundaries, and retains
the existing empty-text/optional-error result rail. The admitted C witness
also covers independent directory cursors, raw non-UTF-8 names, symlink kinds,
closed directories, multi-chunk UTF-8 stdin, EOF and read/encoding errors.
Native C consumers and pack/link drivers invoke the shared
`Tools/packaging/native-c-compiler.mjs` policy. It appends
`-U_FORTIFY_SOURCE -D_FORTIFY_SOURCE=0` and removes only Nix's `fortify` and
`fortify3` features from the C compiler child's hardening environment; all
other hardening and the parent environment remain unchanged. Glibc's fortified
`*_chk` and `__*_2` entry points are not supplied by the admitted musl ABI.
The archive-symbol census regression compiles the real process/TCP C witnesses
and requires every platform import to resolve in the exact musl/libgcc archives.

The Jet-hosted native CLI uses checked MIR directly for build, run and test in
both profiles. Dev remains LIR O0, including the native compiler fixed-point
chain; release uses the existing LIR O2 passes and the same runtime pack. Native
release does not emit a Rust program, compile an inline runtime or invoke rustc.
Release tests use the same selected native harness, framed outcome transport,
fatal-child handling and capture policy as dev tests.

The all-parts runtime includes the native test harness
([`TestHarness.jet`](../../Compiler/JetRuntime/Source/TestHarness.jet)) once;
the compiler-call preparation step must not append a second copy.
Test callbacks transfer owned E4 text errors through the shared carrier `take`
operation, outcome wire output uses `handle`, and finish-wire parsing borrows
the caller-owned carrier through `view`. No Rust `Box<String>` layout crosses
these C doors.

The front end owns language checks and user-facing compiler diagnostics.
Backends consume checked facts; a back end that rejects checked MIR has hit an
internal compiler error, not another language checker.
Checked compile-time `reject(code:, what:, why:, fix:)` calls use the canonical
`core.compiler.__reject` intrinsic row, declared in `Prelude/Core.jet` and
projected into both Core inventories. Its four diagnostic fields remain in
that order; the evaluator reports the registered project diagnostic rather
than treating a missing row as an unsupported construct.

Callable-tail, value-block and fallback-block parsing first distinguishes
guard tables with direct `-> return` or `-> break` statement arms from value
tables. Such arms are checked by the ordinary statement parser without a
doomed value parse; braced control arms remain valid value branches. This
shared Rust/Jet lexical distinction prevents recursively balanced generated
tables from being reparsed exponentially without weakening their diagnostics.

Native compiler call images retain their runtime and writable static storage
until all calls and owning argument/result carriers have retired. Scalar
decoding detaches the compiler value; it does not consume the native owner.
The host releases each owner using its canonical runtime drop before unloading
the image. A callable's checked, zero-argument Unit finalizer then releases its
runtime-owned statics before the loader unmaps code and data. Initialization
and finalization share a live flag, so repeated calls reuse the same statics
without duplicating owners. Calls through the x86-64 assembly host boundary
declare all System V caller-saved vector registers as clobbered.

### Build graph and compiler nodes

The checked compiler publishes one `BuildPlan` graph. It contains declared
actions and compiler-owned `check`, `compile`, and `link` nodes. Each compiler
node has a logical subject, content-digest inputs, and a SHA-256 content key.
Keys and ordering do not contain checkout-specific paths.

Build execution records durations and cache reasons in the store under the
content key. `jet inspect explain-build <file.jet>` reads the workspace's
last recorded run of the program and emits the nodes in graph order, followed
by one `package` row per package of the checked program. A package row
carries the package check key's inputs and its reuse reason against the
previous run: `green:key` (same check key), `red:source`,
`red:dependency:<identity>`, `red:toolchain`, or `red:new`, with `+cutoff`
when a rechecked package kept its interface digest. The JSON form is
`jet.explain-build/v1`; text output contains one node per line. The compiler
owns node identity and dependencies, while the store owns run evidence.

### Single slot IR (target middle end)

D-ARCH-IR1=A replaces the three middle representations, TIR→MIR, the MIR
optimizer and lint, MIR→LIR, and LIR-based instruction selection and register
allocation, with one IR per function. The target pipeline is:

```text
checked program (TIR + sema facts) -> IR (slots, structured control) -> x86-64 selection
  -> existing encoder, object writer, DWARF, static linker, runtime pack
```

**Frozen boundaries.** Both ends of the middle stay exactly as they are; only
the internals between them are reworked. Changing either boundary needs the
owner's explicit approval.

- *Front-end boundary:* the IR builder consumes exactly what codegen consumes
  today: the checked program (TIR `TFunc` rows and the sema facts they carry).
  The parser, sema and every front-end API stay unchanged. A data-oriented
  front end (flat AST, facts in tables keyed by node id) is a later,
  separate step; when it lands, it changes the builder's input reader and
  nothing below it.
- *Emission boundary:* the IR path produces `X64CompiledFunction` and
  `X64FunctionDebug` rows and an `X64ObjectModule` (data objects, debug files
  and types, per-function source facts), all declared in
  `Image/Emission.jet`, for the existing encoder (`X64/Encoder.jet`), object
  writer (`Image/Object.jet`), DWARF writer (`Image/Dwarf.jet`) and static
  linker (`Image/StaticLink.jet`). It calls the same runtime symbols with the
  same System V ABI and links the same runtime pack, so a program built on
  either path behaves identically.

**Data model** ([`Compiler/JetIR/Source/IR.jet`](../../Compiler/JetIR/Source/IR.jet)).
An `IRModule` holds interned type rows (the checked `Type`, its kind, size,
alignment, ABI class and drop symbol), interned names, read-only data rows and
the functions, keyed by their stable function key. An `IRFunction` is one flat
arena in struct-of-arrays form: slot columns (type, kind, name, line) and
instruction columns (`op`, `dst`, `a`, `b`, `c`, line), plus one shared
operand pool for calls. Slots `0..<param_count` are the parameters. There is
no per-instruction or per-slot heap allocation, and every reference is an
integer id.

- **Not SSA.** A slot is a mutable typed variable. An instruction reads slots
  and writes at most one. Loops need no block parameters, so the back-edge
  copies that made up half of all LIR instructions do not exist.
- **Structured control.** `Block`, `Loop`, `If`/`Else`, `End`, `Br depth`,
  `BrIf`, `Return` and `Unreachable`; there are no arbitrary jumps. Every
  opener records the index of its `End`, so a walker skips a region in O(1).
  The form maps one-to-one onto Wasm blocks and locals.
- **Explicit ownership.** `Copy`, `Move`, `Drop` and `Close` are instructions.
  The builder emits them from the checked ownership facts, and only for types
  that have a drop.
- **Semantic operations where Jet semantics live.** For example, `Add` on an
  exact `Int` slot means inline arithmetic with a big-integer spill, and
  `Concat` means String concatenation. The slot's type row selects the
  meaning, and the machine layer chooses the instruction sequence. The
  runtime routes are the old backend's symbols.
- **Content hash.** A function's key is the SHA-256 of its text form, in which
  every interned id is spelled out by content. That key is the unit for the
  store, caching and per-function hot swap, and it replaces the MIR digest.

**SSA only on demand.** A pass that needs SSA (value numbering, constant
propagation across joins) builds an SSA view of one function, uses it and
discards it. A view is never stored, never spans functions and never becomes
the program's representation. O0 builds never construct one.

**Consumers and their replacements.**

| Consumer today | Reads | Replacement |
|---|---|---|
| TIR→MIR lowering and assembly (`JetCodegen/Source/Codegen`, Driver assemble) | TIR | IR builder (`JetIR/Source/Build*.jet`), from the same checked rows |
| MIR legality and lint (deleted with `JetOptimizer`) | MIR | one structural IR verifier that runs only in verify mode; sema remains the single semantic check |
| MIR optimizer (deleted with `JetOptimizer`) and LIR passes (`JetBackend/Source/Opt`) | MIR, LIR | none at O0; O1/O2 passes are rewritten on the IR (inlining splices slot arenas; loop work uses the structured loops; SSA views where needed) |
| MIR→LIR lowering (`JetBackend/Source/Lower`) and LIR selection/allocation (`X64/Select.jet`, `X64/RegAlloc.jet`) | MIR, LIR | IR x86-64 selection with linear scan over slot live ranges (`JetIR/Source/X64*.jet`) |
| Encoder, object writer, DWARF, static linker | x64 rows | unchanged (emission boundary); DWARF variables are slots |
| Comptime evaluator (`JetEval`) and in-memory native comptime | MIR | runs the IR (in-memory machine code from the same selector), one meaning for comptime and run time |
| Web emission (`JetCodegen/Source/Emit/Web`) | MIR | structured IR maps to Wasm and JS control directly |
| Rust emission (`MIRRust`) | MIR | removed with Rust, not ported |
| REPL, session host and hot swap | MIR, LIR | per-function IR units keyed by content hash |
| Test harness lowering, canonical pass journal, cost reports, acceleration ledger | MIR | read the IR and its facts |

**Migration order.** Each step lands only with its proof, and each cutover
deletes the old path: there are no permanent dual paths.

1. *Spike, then O0 corpus.* The IR builder, x86-64 selection and a
   differential runner sit beside the old path. They are not part of the
   self-hosting unit until cutover. Coverage grows by collect-all censuses:
   one keep-going run over the corpus, Core and the compiler unit yields
   one histogram of unsupported constructs, which is fixed as a batch.
2. *O0 cutover* (`jet run`, `jet dev`, native comptime). This step deletes
   the MIR→LIR O0 route.
3. *Self-host on the IR.* The compiler built through the IR rebuilds itself
   to a byte-identical fixed point.
4. *O1/O2 on the IR* (`jet build`, `--release`). This step deletes
   `JetOptimizer`, the LIR and `JetBackend/Source/Lower`.
5. *Comptime evaluator and web emission on the IR.* This step deletes the MIR
   and its evaluator.
6. *Data-oriented front end* (flat AST and facts by node id), the separate
   later step. It changes the builder's input reader only.

**Differential oracle.** Every cutover requires the following:

- *Corpus:* each program the old backend passes produces byte-identical
  stdout, stderr and exit status on both paths. A program the old backend
  fails but the IR passes must match its golden output.
- *Self-host:* the compiler built through the IR rebuilds itself to a
  byte-identical stage, and it produces the old-path compiler's output on
  the corpus.
- *Comptime:* the evaluator on the IR agrees with the MIR evaluator on every
  comptime program.

The phase trace sets the old path's `lower`, `assemble`, `optimize`,
`native.lower` and `native.object` against the IR path's `ir.build`, `ir.x64`
and `ir.object` on the same corpus and machine.

### Typed IR (TIR) — the codegen seam

The [IR builder](../../Compiler/JetIR/Source/) accepts the checked bundle,
carries sema-approved types, access conventions and effects through TIR into
the [IR](../../Compiler/JetIR/Source/IR.jet), and gives those facts canonical
type, function, slot and artifact identities there. The resulting IR is
validated and optimized before execution or emission. Neither lowering infers
new language permissions or falls back to an unchecked AST emitter.

Registered source-owned record declarations are retained even when a native
runtime carrier shares their leaf name.
In particular, `core.term`'s `Stdout`, `Stderr`, and `StdinHandle` retain
their qualified source identities and `fd` record fields; they are not
aliases for the Prelude's opaque stream carriers. Compiler-owned Core-only
record literals, such as `RangeError { reason }`, likewise retain owner rows
whose fields come from the canonical Core record oracle.

Qualified Core closure intrinsics and imported aliases enter the same
checked closure lowering before ordinary Core-call record lookup. Thus
`core.mem.scope.guard` retains its executable callback and lexical cleanup
contract instead of being mistaken for a scalar Core call.

An empty failure set does not by itself erase a checked value carrier.
`T Never!` calls can still name `Result<T, Never>` in checked expressions;
automatic propagation consumes that carrier to produce `T`, including inside
prepared enum payloads. Lowering may omit propagation only when the operand
is already the plain success value, not merely when it cannot fail.
Concrete closure inference preserves an existing `Result` or `Option` by its
shape, not by whether its error set is inhabited. A nested pure closure's
`Result<T, Never>` must not acquire a second `Never` rail around that carrier.

Close lowering distinguishes boxed/handle pointer identity from scalar presence
with an ordered representation arm table. Non-inline scalar receivers use a
presence sentinel; pointer receivers retain their actual identity. The
representation is evaluated once, only after ruling out inline receivers.

The [driver's source compilation path](../../Compiler/JetDriver/Source/Driver/Pipeline.jet)
lowers the checked bundle once and passes its selected artifact to the back
end: the native code generator in `Compiler/JetBackend` or the Web emitter in
`Compiler/JetCodegen/Source/Emit/Web`. These are consumers of one checked
meaning, not separate front ends.

Trait declaration projection retains the original checked Prelude owner for an
implicit default error domain. Its effective return type and failure carrier
come from the same resolved contract, so an implementation and its trait do not
acquire different nominal `Err` identities. Authored explicit error domains
remain authored; native emission still checks exact normalized signature
identity rather than accepting matching display names.

### Native representation boundaries

The [Jet x86-64 selector](../../Compiler/JetIR/Source/X64Select.jet)
normalizes incoming and returned `Bool` values from the low System V ABI byte;
unspecified upper register or stack-slot bits must not change Jet truth.
Image origins preserve the linker's relative alignment: relocatable read-only
sections advertise and use their maximum datum alignment, and static ELF text
starts after header padding to the maximum function/data alignment.

Hosted program and runtime objects share the [section-table writer](../../Compiler/JetBackend/Source/Image/Object.jet).
Its content rows also determine section names, offsets, links and header count;
DWARF and unwind relocations use those same indices. Each compiled function has
one matching debug row, including handwritten entry adapters. Lifecycle exports
append priority-named init/fini sections and their relocations, with equal
priorities ordered by export symbol.

Runtime fixed-width negation uses `wrapping(I64{0} - value)`: the per-operation
overflow opt-in admits a binary integer operation (D-NUMOPS1), not unary
`wrapping(-value)`. This preserves the two's-complement minimum word as well as
ordinary errno and magnitude conversions without broadening arithmetic policy.

Runtime representation is not the checked field type. In particular, default
`Err.cause` value and place reads use the shared
cause projection to expose
`Err?`, not the runtime's boxed recursive storage. The
[default error example](../../Examples/features/errors/default_err_value.jet)
covers present, absent and repeated cause reads.
Returned `View` and `ViewMut` signatures retain the checked owner provenance:
both Rust emitters attach an explicit lifetime to the returned reference and
its owner parameter, not to unrelated borrowed parameters. Static-only views
use `'static`. The [borrowed view example](../../Examples/features/memory/borrowed_view_len.jet)
covers a multi-parameter return, chained and stored length reads, indexing,
and iteration without converting the borrowed slice to a list.

Read-only text and collection leaves accept `&str`, whether the checked caller
holds an owned String or a sema-proven String view. Both Rust emitters lend
that carrier directly to builtin receivers and operands; they do not
materialize a view for those reads. A leaf that stores text owns it at that
storage boundary: a string-keyed map update copies its borrowed key only when
inserting a missing entry.
User-defined `String` parameters retain their owned-String ABI, so lending a
view to those parameters still materializes it.


### One reflection model

`StructDef::reflection_fields()` is the declaration-owned stored-field-row source for
comptime `TypeInfo`, runtime `reflect.of`, TIR evaluation, and generated field
projection. Comptime reads facts from those rows; runtime wraps each projected
field in a typed `Value` and adds display text only as a view. Every back end
and the transitional evaluator therefore preserve the same names, types, and nested value
shape instead of maintaining a second string-field model.

The runtime-retained subset is exact: stored field names and order, typed field
values, the leaf type name, the canonical type path, and display text. Runtime
reflection does not retain markers, visibility, source spans, methods, type
parameters, layout facts, or other compile-time facts. Those facts guide code
generation and have no runtime use. A non-struct value keeps its type name, path,
and display text, but its field list is empty. This makes `reflect.of` a read-only
snapshot, not a dynamic type registry.

### Structure fact plane

D-STRUCT-PLANE1=A puts liveness, lifecycle, and import-edge observations in the
same `StructureFact` shape in `jet-foundation::Names`, with one registry row per
kind. Each row carries its safe direction and gate. Sema writes the facts while
it checks names, lifecycle markers, and resolved imports; `GateLedger` projects
each gate-bearing fact once with its source span and provenance. The CLI command
`jet inspect structure <file.jet>` is a read-only projection of those rows and
that ledger. Its JSON form has the same data; it does not create an inspection
ledger of its own.

The structure plane ends before TIR. AOT emit, the interpreter, and
web codegen receive the checked program after structure facts and gates have
been erased. This preserves I2 and I3 (back ends and engines do not decide policy),
I8 (one registry and one gate mechanism), and I9 (one meaning across applicable
tiers). The decision adds no user-typeable spelling and no lint plane or
external analyzer.

## Compiler package map

D-COMPILERSEAMS1/2 define the compiler's seams as packages under `Compiler/`.
`Compiler/JetCli` builds the `jet` executable over their APIs; Jetpack's
packages live under `Jetpack/`.

| Package | Job | May emit diagnostics? |
|---------|-----|-----------------------|
| `JetFoundation` | shared leaf types and policies: syntax registry (`Data/Syntax`), `Diagnostics`, `AST`, `TIR`, `MIR`, `Span`, target machines, stable exit codes, JSON | renders diagnostics |
| `JetLexer` | text to tokens | yes (E00xx) |
| `JetParser` | tokens to AST, formatter | yes (E00xx) |
| `JetSema` | all semantic checks, collects all front-end diagnostics | yes (E01xx+) |
| `JetCodegen` | checked bundle to TIR and canonical MIR; Web emission | **never** |
| `JetOptimizer` | MIR optimization and legality verification | **never** |
| `JetIR` | the slot IR: builder from checked TIR, optimizer, x86-64 selection | **never** |
| `JetBackend` | native code generation, object and image writing | **never** (a failure is an ICE, R5) |
| `JetEval` | in-process evaluator for comptime and the REPL | no user-facing surface by itself |
| `JetRuntime` | the native runtime linked into every program | no |
| `JetDriver` | front-end orchestration and compile outputs; depends on `Jetpack/PackageModel` (never Jetpack's engine) for manifest/lock/FFI preparation | front-end and interpreter-boundary diagnostics |
| `JetBuildSession` | build sessions over the driver | no new diagnostics |
| `JetQueries` | demand cache for incremental inputs and derived query values | no |
| `JetSemIndex` | stable semantic index over checked programs for tooling | no new diagnostics |
| `JetImpact` | blast-radius reports over `JetSemIndex` | no |
| `JetRepl` | interactive shell and notebook over `JetDriver`, `JetSemIndex`, and leaf policy | no new diagnostics |
| `JetDebug` | source debugger and DAP over `JetDriver` plus leaf JSON/exit policy | debugger diagnostics only |
| `JetLSP` | language server over the same front end (D-LSP1) | renders existing diagnostics only |
| `JetCli` | the `jet` executable: canonical command/flag registry, completions, man page, diagnostic reference, and help UI | renders existing diagnostics only |
| `JetCanvas` | Canvas browser HTML/JS projection assets | no |
| `JetDevserver` | watch/HTTP/static policy, Canvas routes and semantic/edit service, browser-client leases, terminal/browser status parity, live reload, and atomic last-good artifact swapping | renders existing diagnostics only |
| `Jetpack/PackageModel` | **L1**, shared read-only package/config data model: `package.jet` manifests and the inline `package { … }` carrier, lock, store listing, ref classification, FFI bridge construction, structural `Merge`, the `BuildRecipe` data shape, plus pure effect-budget/lint-policy computation (no network/provider/shell) | package/FFI diagnostics |
| `Jetpack/EnvModel` | **L2**, shared pure environment plan model: `ModuleEval` and its typed plan outputs; no provider/store/network/shell | plan-evaluation diagnostics |
| Jetpack engine (`Jetpack/CLI`, `Store`, `Environment`, …) | **L3**, package manager engine: provider/network/shell realization, JetOS, CLI; reads L1 data and realizes L2 plans | package/JetOS diagnostics |

### Compiler implementation boundary

The compiler is written in Jet. The Rust host that bootstrapped it is gone
(D-EXEC2, owner directive 19); its last implementation is frozen on the `rust`
branch. A pass changes at its existing seam, with the same inputs, identities,
ordering and diagnostics. It must not introduce a third front end, a second
query verdict, or a new execution lens.

The compiler-owned native child has a bounded transport contract:
`JetDriverExecutionHost.execute` and its CLI implementation declare
`-[Exec, IO, FFI.C, FFI.Asm, Time.Wait]>`. The row covers child lifecycle,
private framed I/O, raw C/assembly host leaves, and shared-resource
serialization, not the selected program's ambient authority. Program admission
checks that authority separately. The CLI package grants only the two foreign
language leaves its host uses, not the `FFI` root or unrelated Browser, DB, GPU,
Log, or Secret authority. An omitted trait row would instead make dynamic
dispatch maximal and cannot stand in for this checked boundary.

Driver Core build carriers use the public module's member-import route, such as
`use core.build.[BuildGraphNode]`; the carrier name is not a Core module path.
The bootstrap aggregate keeps the first occurrence of each Core member binding,
including overlapping grouped imports. Removed members and redundant separators
are blanked in place; surviving members, newlines, and provenance byte offsets
remain unchanged. Independently checked package sources retain their own imports.

The Jet semantic checker separates read-only registration/program tables from
mutable scope, ownership, and diagnostic state. Read-only graph probes match
the optional registration graph in place; they do not materialize an owning
graph snapshot. Ownership branch forks retain indexed declaration/reference
counts and small path facts. Their undo trail restores binding freeze
provenance, and a joined binding is frozen only when every reaching path
proves it; freeze source sites remain available for diagnostics after the join.
Read-pattern scalar bindings copy their values rather than carrying an owner
view; explicit write-pattern bindings and non-scalar reads retain their view
origins and mutation restrictions.

Semantic checking APIs thread `SemaProgramTables` explicitly alongside
`SemaScopeEnvironment`; callers pass the tables rather than shifting mutable
scope arguments into a missing table slot. An owned pass environment transfers
its registration graph and checked-type indexes into the one program-table
owner before checking; both comptime staging and finalization reinitialize every
moved field before handing the complete environment owner to shared state.
Module retargeting reads that graph in place and replaces only module-local
scope facts. It neither extracts fields from borrowed owners nor copies the
graph or rebuilds the checked-type indexes. Staging and speculative landed
values use the graph mutation trail rather than registration-graph copies.
Inference unification and assignability read their context in place; only the
separate binding and conflict outputs mutate. Expected type trees do not need
copies merely to pass through those read-only helpers.

Output finalization publishes each callable's settled return type, failure
contract, and sorted solved effects into checked constants, retained source
programs, and the ordered graph-result Output rows. The shared publication
walk lazily indexes only referenced modules; it does not clone function bodies.

Dimension registration resolves its dependency worklist with explicit indexed
read/write assignments for pending counts; indexed compound updates are not
part of Jet's assignment contract (E0164).

Semantic pattern validation passes program tables into list/map slots, and
effect traversal visits nested payload patterns and map keys in place. Dynamic
dispatch reachability is solved once across reverse call edges; bounded-call
diagnostics reuse that reachability without copying expression children.
Gate inspection visitors collect new facts in an owned ledger and merge them
through the canonical deduplicating publication function. The expression
visitor never consults pre-existing ledger rows when deciding which facts to emit.

Generic collection static constructors preserve explicit owner type arguments;
an omitted argument can be inferred from the expected success value, including
through the constructor's failure rail. Context never replaces an explicitly
written element type.

The compiler is the package graph `jet build Compiler` builds: the CLI package
and every package its `package.jet` path dependencies reach, each package's
members found by the driver's source walk. The runtime aggregate keeps its own
explicit inventory,
[`Compiler/JetRuntime/sources.list`](../../Compiler/JetRuntime/sources.list):
the compiler leaves runtime-only units out, while a source that is both a
compiler package member and listed there (for example `OS/Linux.jet` or
`ErrLayout.jet`) is deliberately shared.
The compiler's private `Core/Build` kernel uses the same declared enum payload
labels and write-access calls as ordinary Jet. Stable plan keys use integer
quotients and sorted, collected map keys. Legacy declaration validation copies
its transactional trial before opening the final state write window.

Core's declaration twin distinguishes exported names from source-owned bodies.
A source-module `owns` row names an actual Jet implementation, not an intrinsic,
native carrier, or retired helper; source receiver impls may still extend native
nominals. Generated Core tables preserve that distinction rather than inventing
source shadows for compiler-provided operations.

Source-owned Core value hooks are also checked capability providers. MIR resolves
each private hook by the exact nominal identity and read-access method, never a
short name. Native emission selects those manual Clone/Debug implementations
before considering structural derives; enclosing rows inherit their real
capabilities without treating private Shared or opaque state as Debug-capable.
For example, Clock's `__fork` preserves semantic-copy isolation and `__show`
preserves its nominal rendering. Show and Debug routes are independent; a
Show-only hook does not grant Debug, and native bridges do not introduce public
Jet trait implementations. A missing or ambiguous hook remains a schema failure,
not a reason to fall back to a structural derive.
Exact registered hook owners also participate in the existing source-ownership
lookup, so fields and call signatures cannot silently project them onto a
same-named Prelude carrier. Native bridges consume the checked `Result<_, Never>`
carrier before returning the provider's success value. A checked Show provider
also supplies the private native interpolation bridge unless a selected Display
implementation already does; this adds no public Jet Display implementation.
Executable reachability roots these exact registered private methods and retains
their ordinary checked callee closure. It does not rely on an explicit formatting
or aggregate-copy call in the user's AST, nor retain unrelated Core free functions.
Registered hooks on generic Core carriers retain the checked owner's binders;
their native bridges use those declared bounds, not invented Printable or Debug
bounds. Method emission inherits owner binders from the impl instead of
redeclaring them. ExpiringValue's Show-only hook can therefore render its
deadline without requiring its untouched payload to implement Printable.
Clock's private raw monotonic sample has a pure Core-call row, matching the
checked source signature: injected manual clocks do not acquire ambient Time
through their shared helper. Ambient wall/host sampling and system Clock
construction keep their Time authority; this is not a package-budget grant.

Authored Core `Close`, `Display` and `Debug` implementations are implicit
executable roots: cleanup and formatting need their checked bodies even when
the caller's AST has no method reference. Their ordinary callee closure remains
subject to the same source-body reachability walk. The Jet lowerer already
retains authored Core methods; bootstrap lowering preserves these implicit
targets too, rather than losing a formatter merely because arithmetic produced
its receiver.

Precise operations may also be synthesized after authored-call reachability,
including implicit `Int / Int` arithmetic inside a string interpolation.
Bootstrap lowering roots the exact canonical Core/Math helpers registered for
those operations, and their checked callee closure, even without an explicit
Fraction type in the caller. An arbitrary private helper sharing a prefix is
not a provider. The Jet MIR reachability walk retains the same registered
helpers from its checked `PreciseBuiltin` operations.

Collection callbacks returning `T Never!` select the non-fallible operation:
their empty failure carrier is normalized before invocation. In particular,
two-argument sort comparators, whether named or literal, use the checked
comparator route rather than attempting a fallible key-selector route.

The native backend lowers canonical `List.reverse`, `List.extend`, `List.last`
and `List.try_reserve` directly over the List header. Reverse swaps packed
scalars or complete inline owner layouts without cloning or allocation. Extend
consumes the source buffer, adopting it into an empty destination or relocating
its elements into one geometrically grown destination; it never closes moved
elements. Last uses the ordinary checked List-get clone and optional carrier.
Fallible reserve retains the original header and element owners on failure and
consults the allocation fault rail exactly once for each nonnegative request,
including requests satisfied by existing capacity. Its error is the canonical
`AllocError`, not a backend-private layout. The pack-free `--list-routes`
fixture mode executes production lowering and LIR lint for all four witnesses;
native image execution remains part of the admitted-pack fixture census.

Runtime-route rejection queries borrow their handle-table rows. Repeated
queries, including short-circuit expressions, must not consume that table.
The bootstrap Rust emitter stores eligible pattern payloads as references
without copying their owners, but retires those reference slots before a
checked operation changes the underlying owning storage. A payload reached
through another reference binding retains the original owner's identity;
hoisting the reference slot must not extend its borrow across a later loop
iteration's owner assignment.

When a checked handle operation requests write access through an indexed
read-window alias, lowering updates every inherited index accessor to its
exact mutable getter and setter rows. Nested fields and indexes retain the
original owner and path; the access decoration cannot disagree with those
rows. This changes no source-level permission or authority decision.

Enum tests and payload reads keep their checked declaration owner when the
subject is a structural union. A union's shape identity is not a nominal
definition ID. Only an actual nominal application, possibly under a tag,
can refine that owner to its checked instance identity.

A callable's value tail may sit inside an audited `#Unsafe` region; lowering
returns that region's last line exactly as it returns a table arm's. The miss
path of a covered else-less pattern table is an unreachable dead end on every
producer, never a fall-through to the callable's end. A checked scalar head
(`F32{x}`, `U8{y}`) lowers to the conversion its source and target name:
integer-to-float widening, checked `Int`-to-fixed, `F64`-to-`F32` narrowing,
or the fixed-width carrier cast. Native Core enums such as `EnvError` are
cataloged enum leaves, so their checked MIR owner rows always exist.

Only `JET_DEBUG_LOWER_ALL=1` enables the bootstrap Rust emission census.
Each selected type, trait, implementation, top-level function, constant,
foreign item, callback/export provider, C-ABI physical type or signature
discovery, or synthesized support batch is an attempted item. A failure
records its kind, stable ID, name, source byte span,
and original reason; absent source spans are `0..0`. Names and reasons use
JSON string escaping. Failed item text becomes a clearly marked, escaped
line comment rather than invented code, and later items are attempted.
Failed traits and impls also own their skipped method bodies.
The final `emission failures N of M` counts failed and attempted items,
independently of MIR's function census. Collect mode also writes a separate
diagnostic-only partial Rust monolith: `JET_DUMP_RUST` selects its path,
otherwise a process/sequence-named file in the temporary directory is used.
The log announces its JSON-quoted path. Its banner says it is not an admitted
build artifact; it lets the same failing emission run type-check all surviving
items with `rustc --emit=metadata` without claiming complete emission proof.
Any emission failure still discards the normal returned Rust fragment and
propagates the first original panic through the existing ICE/exit-101 path.
Normal emission remains fail-fast and never writes that diagnostic file.

A bare registered Core-carrier trait reference resolves through that carrier's
one source-module owner and the exact trait/method identity. It cannot match an
unrelated same-named method, a different trait or an arbitrary suffix alias.
Missing or ambiguous checked targets remain lowering failures.

For `T? Never!`, the executable return is the one optional carrier, not a
nested `Result<Option<T>, Never>`. Reconciliation compares its success type
under the declaring module's checked nominal identities: a local source leaf
and that leaf's imported qualified owner are one type. It unwraps the empty
failure layer once while preserving the optional presence/absence layer.

The Jet Driver carries each installed Core row's runtime classification into
its parse job. Runtime dependencies retain all functions but omit their test
controls; only an entry with the installed unit's exact path and bytes keeps
those controls. Nested runtime imports bind to the longest installed unit, and
member imports are rewritten on the load thread, not in parallel parse jobs.
User imports cannot gain access merely because a runtime unit was already
loaded. Inline-package masking is unchanged. Core-library loading roots the
whole installed catalog; program loading begins with the selected real entry.
[`RuntimeUnitImports.jet`](../../Compiler/JetDriver/Tests/RuntimeUnitImports.jet)
records these loader contracts independently of backend linking.

Keep-going compilation preserves the caller's emission choice. Its explicit
`emit_rust` argument reaches the same staged compiler as ordinary compilation;
checks and native library objects use `false`, so collecting diagnostics never
silently selects the retired Rust emitter. The historical Bootstrap host passes
`true` only while its Rust-emitting bootstrap entry remains installed.

Proof chains use one shared
[`git-archive source export`](../../Tools/agent/proof-source.mjs) per full commit,
not a moving checkout. `node Tools/agent/proof-source.mjs <full-sha> [repository]`
prints its source directory; the sibling manifest records the commit, tree,
tracked blob hashes and executable modes. Each phase calls
`assertPinnedSource(source, sha)` before consuming it. Regeneration uses a
writable copy outside the shared export. `proofTargetDirectory(sha)` provides
the corresponding external Cargo target; set `CARGO_TARGET_DIR` only after
export creation, before invoking the exported `jet-env`. When creating the
export through the live checkout's `jet-env`, unset `CARGO_TARGET_DIR` so that
checkout does not stamp the proof target as its own. The
[`proof source guard`](../../Tools/agent/proof-source.test.mjs) checks tracked
bytes, symlink targets and executable modes without requiring dev to stand still.
The exporter completes a temporary tar file before extraction, removes that
file, and publishes the verified source and manifest with one atomic rename.
Archive or extraction failures publish nothing; consumer EOF cannot race the
archive producer through a pipe.


The private startup-output factory packages supplied environment bytes and
terminal facts as an ordinary checked Source value, retaining owned typed bytes.
Native suppliers must match that value's checked layout and ownership.
Compiler-internal byte READ helpers operate on already supplied arrays, retain
the shared diagnostics scalar decoder and existing bounds behavior, and leave
source lifetime and output ownership with the caller. They do not change public
Core or String APIs or eliminate the copy made by String.bytes(). Declaring these
values and including their source files does not activate the host startup path
or qualify a native supplier.

Compilation keeps the `JetDriverCompileRequest` field `comptime_stop_points`
false: compile-time code runs in the in-process evaluator, which refuses stop
points, until native in-memory O0 lands.

Build-plan queries move their invocation request into one query and return that
same owner beside the query facts. Staged MIR checks borrow it; only the small
entry/selection strings and scalar capture settings are saved and restored,
including on an escaped stage error. The final checked bundle removes only the
generated-source and output-selection metadata temporarily added by that query.
It does not duplicate the authorized source tree, evaluator or execution host.
Raw filesystem and HTTP build transports live in invocation-scoped `Shared`
cells. Adapter clones copy those cell handles, retaining the original provider
and its authority rather than duplicating an opaque trait value. Each call
borrows the provider under its cell's read guard and keeps the existing exact
effect row, destination checks and explicit missing-provider refusals.

The native Shared constructor adopts a fresh or explicitly moved payload under
the canonical MIR ownership-transfer rules. It must not clone an opaque
provider merely to put it into the cell; copying a handle never copies that
provider. Borrowed copyable payloads retain their checked materialization.

Compiler packages depend inward through their `package.jet` path dependencies:
`JetFoundation` and `JetQueries` are leaves, and the front end (`JetLexer`,
`JetParser`, `JetSema`) does not depend on driver, codegen or tooling
packages. A private Jet pass does not justify a new external compiler
dependency or a cycle back to `JetCli`.

Typed error propagation retains the canonical conversion symbol recorded by
Sema. TIR materializes checked conversion bodies from imported modules as well
as the entry module; repeated shipped conversions share one source/target
definition. Dropping an imported conversion creates a missing checked MIR
target, an internal compiler failure rather than a source diagnostic.

The MIR interpreter indexes nominal declarations once per immutable program
and memoizes source-value-hook reachability by operation and instantiated type.
Shared nominal subgraphs are visited once within a query. Complete negative
queries can publish their visited rows; a successful cyclic query publishes
only its root, because an intermediate negative may depend on an ancestor
cycle cut. Generic cycles with changing arguments retain the nominal recursion
guard without publishing context-dependent intermediate negatives.

The transitional MIR evaluator's field writeback mirrors field reads through
successful carriers: it updates the struct payload and preserves the carrier,
including nested field writes on an owned local moved from a parameter.
Runtime aggregates retain field IDs and their runtime payload representation;
materialized `Present` values retain their wrapper. Failed results do not
expose success fields, and opaque native handles remain non-projectable.

Lowering shares a checked nominal-key index across the frozen program's
functions and lambdas. Exact-key ambiguity, anonymous-union carriers, Core
source ownership, declaring-scope resolution, and generic-parameter shadowing
remain semantic checks rather than global leaf-name guesses. Interpreter
machines, resumed generators, and standalone callbacks share immutable type,
Core-file-owner, and field-position indices. Duplicate IDs retain first-row
selection; checked struct and named enum fields precede the legacy flat field
table, without copying the declarations into the indices.

CLI default-target discovery scans raw tokens and parses only top-level target
markers and target-bearing marker groups with the registered marker parser.
It does not construct an AST for function, module or type bodies; the actual
front end remains responsible for their diagnostics.

#### Terminator pass

[`Lexer/Terminators.jet`](../../Compiler/JetLexer/Source/Lexer/Terminators.jet)
owns `insert_terminators` policy and all its decision helpers (Tower #3581).
Its input is source bytes plus exhaustive indexed raw-token facts, including
the Unicode-uppercase primitive. Its output is ordered
`InsertSemi`/`SplitHeader` events. The raw scanner and parser are outside that
pass. The terminator pass moves the `LexedTokens` payload carrier.
`lex_with_terminators` attaches its scanner's `TokenTables` to produce `Lexed`;
the terminator pass does not create or copy those tables.

The subsequent self-host proof homes remain separate obligations: #814 is the
pinned stage0-to-stage1 build, #815 is byte-identical stage1-to-stage2 output,
and #816 is the Jet-built compiler full-suite closeout. The policy-family
homes #808–#813 and the boundary inventory #218 cannot substitute for those
bootstrap and fixed-point proofs.

The fixed point is byte identity of the compiler executable: the compiler
that the stage-2 compiler builds equals the stage-2 compiler. A chain stage's
compiler identity (`JET_COMPILER_IDENTITY`, read in
[`Request.jet`](../../Compiler/JetCli/Source/Cli/Request.jet)) keys its receipts;
an identity string is not proof that two compiler binaries are identical.

Behavioral parity between any two compiler binaries (#670) is a separate,
reusable proof: [`Tools/agent/compiler-diff.mjs`](../../Tools/agent/compiler-diff.mjs)
runs the golden, UI and differential corpora through a pinned `--reference`
and a `--candidate` compiler under deterministic roots and a scrubbed
environment. It compares panics, exit status, diagnostics, stdout, stderr and
side-effect files phase by phase (`check`, `run`, `build`)
and reports the first divergence with its case, phase, expected and candidate
values and a repro directory. A missing binary or tool exits 2 and is never a
match. Each compiler file is pinned by identity at startup and re-verified
around every invocation, and a `systemd-run` launch failure is not an
observation: a binary deleted or replaced mid-run also exits 2. `--canary`
runs the checked cases in `tests/compiler-diff/` through a wrapper that plants
known divergences, proving detection without editing source. The comparator
claims behavior only; artifact identity stays with
#815.


### Machine-wide artifact store

D-BUILD-STORE1=E defines [`Jetpack/Store/Source/ArtifactStore`](../../Jetpack/Store/Source/ArtifactStore/) as the one machine-wide artifact
store for compiler and runtime reuse. It owns content-addressed blobs, action
records, ThinLTO entries,
checksummed last-use metadata, atomic publication, digest verification, live
leases, pruning, and capacity admission. The default cap is the smaller of
20 GiB and 10% of the filesystem that contains the store. A 2 GiB free-space
reserve is checked before each write. A stricter host cap is persisted in the
store's host policy and applies to every project on that machine.

The root `jet` binary is only a command surface over this API. `jet cache status`
reports the store root, footprint, effective cap, reserve, entries, leases, and
tiers. `jet cache prune --to <size>` performs an explicit LRU prune, while
`jet cache limit --host <size>` changes the host policy. These commands do not
implement filesystem or eviction rules themselves. `jet self doctor` reads the
same status and reports one artifact-store row.

Source-store Git launches retain their held directory descriptors in the existing
`ProcessSpec` carrier. Descriptor copies are placed above every requested child
slot before fork; the child performs only `dup2` and `fchdir`, without reopening
source paths, allocating, or taking locks. An attached held cwd supersedes the
lexical cwd; launches without that attachment keep the ordinary cwd behavior.
The descriptor and attachment rows carry `Int` descriptor/errno facts and the
existing `ProcessSpec` type, with read-convention inputs. Their named-field type
projection is not a native-ABI proof: each adapter must also preserve the mapping
between canonical semantic fields and the kernel's physical tuple slots.

The native process-spec carrier is 136 bytes: the original fields keep their
offsets, and the private tail at byte 128 owns the inherited-directory rows.
Cloning retains each descriptor owner; dropping releases it without reopening
the path. The raw C adapter duplicates a lent descriptor with
`F_DUPFD_CLOEXEC`, preserving the canonical one-byte Bool outputs and raw I64
descriptor/errno outputs. Environment result adapters use the existing logical
environment writer, never a second OS-environment mutation path. Terminal
sessions retain the same native child descriptor owner and resize its held PTY
under the child lock, validating dimensions before the ioctl.

Package-to-JetOS projection stays on this split. jet-pkg-model reads the
canonical Package outputs and computes the semantic graph identity.
jet-env-model::ModuleEval::project_package_outputs lowers System and Fleet
payloads into SystemPlan and FleetPlan, including service open-record fields.
jetpack loads that plan for jet os plan and the existing generation writer
carries the same identity and service facts into plan.json and the source proof.
Hangar realization and atomic generation
publication remain in jetpack; the projection adds no store or rollout path.
System, Fleet, and host names are validated as safe generation path components
before Fleet deploy facts are staged. Fleet deploy plans and host proof scripts
carry the same graph identity; duplicate Fleet names or host paths fail before
any generation files are published.

Package splits and folds use the same checked graph and one reversible journal.
The journal is published before a multi-file mutation, so a stale, corrupt, or
partially applied transition fails closed and `jet fold` can restore only the
recorded before/after states. Journal file states contain the original bytes as
hex, not content digests; an absent file and an empty file remain distinct.
Atomic file publication and the package lock protect live bytes from concurrent
writers and path escapes.
Host extraction keeps the checked System Output intact and binds the sole-host
Fleet to that System's name, not to its output-map key.

Environment OCI projection follows the same split. `jet-env-model` owns the
pure `ImagePlan`; `jetpack::Image` is the one deterministic OCI realization
path. A successful environment projection writes `projection.json`,
`plan.json`, and `dossier.json` as secret-free, atomically recovered read-only
views. Hangar owns content, cache, archive, signing, and publish; `.jet/lock` owns locked
inputs and platforms; D-JPK-REMOTE1 owns remote bindings and grants. The
image path does not create a second container record, lock, or Dockerfile
mechanism.

### Read-only compiler API

`core.compiler` is the one typed, compile-time-only front-end surface
(D-FRONTENDAPI1=A). `lex(source)`, `parse(source)`, `check(parsed)`, and
`source_map(rust)` return immutable compiler values. The values preserve the
source text, spans, diagnostics, checked functions/effects, and the semantic
index produced by the same lexer, parser, sema, and `jet-semindex` paths used
by the compiler. Build code may inspect them; runtime code receives E0956.

The CLI mirror (`jet inspect compiler lex|parse|check|source-map`) uses the
same versioned JSON envelope (`schema_version: 1`, `api_version: 1`). The
root compiler owns the small deterministic encoder so the compiler seam does
not acquire a serialization dependency. No API method mutates or executes a
source tree, and no backend reimplements these facts.

### Concurrency boundary safety

A data race is two tasks that access the same memory at the same time when at
least one access writes and the accesses do not use a synchronization rule.

D-DATARACE1=C is law. Safe Jet must make a data race impossible to compile for
the covered concurrency surface. Sema owns every user-facing check (I3); a
native build must not lean on rustc `Send` as the backstop (I2).

Covered mechanisms (`tests/concurrency_boundaries.rs` and matching UI
snapshots):

- A child created by `task` owns or copies its captures.
  Values accepted by Jet's ordinary copy law are copied when the closure is
  created, so the source binding remains available without a preparatory
  binding. Owned values that cannot be copied move. `freeze(x)` creates a
  deeply immutable owned snapshot, and `task ^name { ... }` explicitly consumes
  one owned binding into the child. Both use the same crossing prover; a later
  use of a consumed name is ordinary E0121.
  Sema rejects a mutable capture, a borrowed view, or another value that cannot
  cross a task boundary. A frozen write is E1113 and names its freeze site.
- A channel moves a sendable owned value. The sender cannot keep an alias that
  permits unsynchronized writes after the send.
- A task group changes child lifetimes and cancellation only. Its children use
  the same capture and result checks as ordinary tasks.
- `para_map`, `para_filter`, `para_partition`, and `para_fold` reject mutable
  captures and values that workers cannot safely share or transfer.
- `Shared<T>` is the explicit shared-mutation path. `shared x` builds the
  value; a field read is one locked read, a field write is one locked write,
  and each statement is one atomic step. `#Transact` groups several statements
  and commits them atomically, taking every touched value's write lock in
  stable address order. Expert guards hold one lock across helper calls.
- `Cell<T>` is the private local-mutation path. It uses one-thread dynamic
  loans without `Arc` or operating-system locks. Sema rejects cells and their
  guards across task, channel, `Shared<T>`, and parallel boundaries.
- `Signal<T>`, `Derived<T>`, and `Computed<T>` use lock-ordered `Arc` storage
  with the same public API. A handle may cross a task, task group, channel, or
  parallel adapter without a data race and without a rustc `Send` ICE.
- `#Local` pins the fast one-thread form; a crossing is E1102.
- `#Shared` pins the synchronized form (today every reactive box already uses
  that form). Crossing sites are recorded for the upgrade report.

`freeze` does not add a backend policy rail. Its `Frozen` flow fact is lowered
to the existing owned clone/materialization TIR nodes, so AOT, the
interpreter, and any applicable web path consume one checked meaning. The
REPL remains an interpreter-only surface and rejects task execution with E1802;
pure `freeze(x)` evaluation is still a value operation.

These boundary guarantees do not imply deadlock freedom. See the [Deadlock stance](spec.md#deadlock-stance)
for the narrow `#Transact` lock-order guarantee and the non-guarantee for
arbitrary task and channel wait cycles. The M:N scheduler parks tasks at
`channel` waits; `task` and `task.group` handles define join duties. These
mechanisms do not detect arbitrary wait cycles.

At the process-exit boundary, the observe registry emits the registered E3013
runtime diagnostic when tasks remain parked. The report is bounded by both task
cardinality and rendered bytes; each row carries the task identity, state, wait
target, and compiler spawn site, while channel payloads and task locals stay
outside the report. A direct named task call uses that function name plus the
spawn site for identity; an arbitrary task expression uses the bounded
`task@<site>` fallback. A child panic reaches `TaskFailure.Panicked(reason)` with
the same identity prefix and the original child text.

Code inside an `#Unsafe("reason")` region, a foreign implementation, or a
vetted runtime internal must also uphold its boundary contract.

### Private traced collector substrate

D-DEP-GC1=A has one dependency-free collector. Its Rust-tier implementation
left with Rust; the native runtime does not carry it yet, and the Web target's
routes are in [`Gc.js`](../../Compiler/Prelude/Web/Core/Gc.js). The collector
has no source-facing wrapper, constructor, or module; only sema-proven
automatic promotions call its traced allocation entry.

Sema closes each escaping payload over the promoted bindings stored inside it.
Codegen translates those proven source relations to collector object IDs at
creation and updates the same graph on bare assignments and mutations. Cycles
therefore live in the collector-owned graph without changing Jet's bare value
syntax. Collector failures cross one E2110 runtime boundary; generated code
never exposes `Fault` through `expect` or a raw panic.

Object identities are monotonic and never reused. RAII root handles keep an
object live; traced edges are sorted, deduplicated, bounded, and accepted only
when every target is present in the same heap. A safepoint marks from current
roots, follows that metadata, and reclaims unreachable objects in identity
order; the private automatic collector runs that safepoint when a lexical root
is released. Active object access is a temporary mark root, so its transitive children
survive the safepoint too. A mutation reserves its source version, pins current
and proposed targets, edits the payload, then commits metadata; reentrant or
concurrent rewrites fail, and conflict, type failure, or unwind leaves the old
graph intact. Finalizers run at most once in identity order. Finalizer and
value-drop panics are caught and reported internally; a payload poisoned by
unwinding is never given to its finalizer. Dropping the heap drains remaining
objects under the same policy. Handles may cross tasks or threads; all payload
access is serialized, and conflicting, stale, malformed, poisoned, over-limit,
or impossible state fails closed through the private `Fault` result.

This substrate defines no Jet source type. D-OPTGC1's shared policy ladder owns
scoped promotion; #659 owns tracing and reports.

I6: compiler packages under `Compiler/` depend only on repository path
packages. Runtime and tool packages keep their own owner-approved dependency
posture; that does not permit an external dependency to leak into a compiler
package.

Compiler front-end packages may not grow back-edges into driver/codegen
clients; tooling and runtime packages stay outside the compiler seam unless
their dependency row changes here. The package, environment-plan, and JetOS
layers are described in the package map above. D-ARCH-SOURCE1=A puts command
and interactive product ownership behind real package seams. `JetCli` owns the
command/flag registry, completion/man generation, diagnostic reference, and
help UI; `JetRepl` owns the REPL and terminal implementation; `JetDebug` owns
the source debugger, native adapter, line map, and DAP server; `JetDevserver`
owns the web server, Canvas routes, status surfaces, client leases, live
reload, and last-good swap. These products depend inward on compiler packages.
Stable exit codes and the JSON codec live in dependency-free `JetFoundation`.

### Browser automation protocol core

D-BROWSER-AUTO1=A puts the portable automation API in `core.web.browser`. The
generated runtime uses the existing std-only WebSocket and strict JSON codecs
to speak WebDriver BiDi. It does not require Node, Playwright, or a Canvas
facade. Browser installation is Jetpack work: `jetpack browser lock|provision`
writes `[[browser]]` entries into `.jet/lock`, and `browser.locked(engine)`
reads that pin (FS) so launch resolves a deterministic binary.

`BrowserProfile` pins a client command contract; it is not a claim about a
server version. The runtime gates raw commands against that contract. A
connection checks `session.status`, then creates the session with
`session.new`; only that command's matched feature map enables optional
protocols such as CDP. `Browser.context()` creates an isolated BiDi user
context. `context.page()` and `context.tab()` both create a BiDi tab under
that context; the context owns its pages. `page.main_frame()` and
`page.frames()` expose the browsing-context tree (main plus child frames)
without auto-closing listed frames on drop — frame close is explicit.
`frame.close()` on the main frame marks the page closed. The beginner path
uses semantic accessibility locators (`get_by_role`, `get_by_label`), text and
CSS helpers (`get_by_text`, `get_by_placeholder`, `get_by_test_id`, `get_by_css`),
locator actions (`click`, `hover`, `fill`, `press`), and bounded deterministic
waits (`wait`, `wait_gone`). `Browser.subscribe` / `next_event` deliver BiDi
events; network events expose redacted inspection facts (`request_id`,
`request_method`, `url_hash`, `is_blocked`, `status_code`) without putting URLs
or payloads in the trace. Download events expose `download_id` and
`suggested_filename_hash` the same way. `add_intercept` / `add_intercept_url` register BiDi
network intercepts; paused requests use `continue_request`, `fail_request`, or
`fulfill_request`, and `BrowserIntercept.remove` is explicit and idempotent.
`allow_downloads` sets BiDi download behavior to a destination folder.
Page cookie APIs (`set_cookie`, `cookie`, `clear_cookies`) speak the BiDi
`storage` module against the page partition. `storage_get` / `storage_set` /
`storage_clear` read and write `local` or `session` web storage through
`script.callFunction`. `page.screenshot` and `page.pdf` return base64 PNG/PDF
bytes to the caller (never into the trace). Locators upload files with
`set_files` (`input.setFiles`). The expert path
exposes raw BiDi and explicitly checked CDP commands through
`Browser.protocol`.

CDP is an explicit expert supplement (#1192), not a silent BiDi fallback.
`session.new` must advertise `goog:cdp: true` before `protocol("cdp")`
succeeds. CDP `send` accepts only audited `Domain.command` shapes (PascalCase
domain). Raw BiDi `send` rejects `goog:cdp.*` so experts cannot smuggle CDP
through the portable profile path.

The protocol core accepts only strict JSON objects and exact response IDs.
Malformed messages, protocol errors, unsupported profiles, unavailable expert
protocols, and timeouts return `BrowserError`. Commands and waits use one
absolute deadline even while events arrive. The event queue has 256 slots, and
the trace has an 8 KiB byte budget. Entries record only sequence IDs, hashed
method facts, and status. They never record endpoints, raw method names, command
parameters, results, event payloads, secrets, or page data. Page, context, and
session cleanup is explicit, idempotent, and retry-safe after protocol failure.
Last-owner drops perform the same cleanup as a best effort; page leases keep
their isolated user context alive until the final child is gone.

`Browser.privacy()` reports the session defaults: isolated profiles on, shared
profiles denied, and receipt redaction on. `Browser.receipt()` is the normal
audit receipt — the same redacted fact stream as the trace, plus `isolated` and
`cleaned` flags — and never carries endpoints, secrets, or page data.
`BrowserContext.isolated()` is always true for BiDi user contexts;
`user_hash()` exposes only a hashed context id for correlation.

The AOT back end and default development tier use the same
[`Browser.jet`](../../Core/Web/Browser.jet), strict-JSON behavior, and
extracted RFC6455 client source. Browser session
handles are thread-confined. Programs that use them select the canonical TIR
tier-0 network host; the tier trace reports that choice and no AOT fallback is
hidden behind `jet dev`.

Browser automation coverage includes provision, lifecycle, locators, network,
artifacts, checked CDP, and privacy, with examples under
`Examples/features/net/browser_*.jet` and hostile proofs in
`tests/browser_bidi.rs` / `tests/browser_lock.rs`. Live browser binaries remain
host-supplied; the product path always speaks BiDi over the existing WebSocket
transport.

### FFI bridge boundary

Foreign dependencies stay behind the runtime boundary; they never become
dependencies of compiler workspace crates. A bridge starts from a ratified
interop surface and dependency approval, and declarations are parsed and
checked by the ordinary front end before codegen.

D-UPSTREAM-SCOPE1=A makes upstream-first ownership the default for optional
tooling and compatibility integrations, with upstream vendors or applications
owning those integrations; D-NEST-UPSTREAM1=A remains the unchanged
service-binding baseline. This preference approves no provider, dependency,
version, command, public API or deletion: each concrete adoption needs its own
exact choice and source, version, license, hash, target and boundary proof,
including values, callbacks, ownership, authority, errors and deployment. No
ambient provider or silent fallback is permitted. Existing Core contracts,
ratified bindings and approved native implementations remain until a named
cutover is approved; mandatory language operations cannot become optional host
features. Safety, authority, I6 dependency seams, pure planning, native build
and boot, lifecycle, I9 execution meaning and matched-performance gates remain
obligations of the concrete implementation cards, not evidence supplied by this
policy.

[`Jetpack/PackageModel/Source/Foreign/Bridge`](../../Jetpack/PackageModel/Source/Foreign/Bridge/)
prepares a generated, content-addressed bridge; a user Rust bridge's generated
`Cargo.toml` owns its foreign dependencies
([`RustBridge`](../../Jetpack/Integrations/Source/Foreign/RustBridge/)).
Inline `#FFI(c|cpp|asm)` bodies use the same bridge
and include their checked signature, target, raw body, and schema in the cache
identity. C/C++ wrappers and asm lower only after sema proves their boundary
contracts.
Inline C/C++ bodies remain separate translation units. The generated build
script compiles every selected body before proving the complete bridge with
the target's strict undefined-symbol linker flag, using the C++ linker when
the closure contains C++. Shared globals may resolve from a sibling body;
references missing from the full closure still fail. Only a successful proof
is archived. The cache identity includes the build script itself, so changing
this proof invalidates stale bridge artifacts without changing the ABI schema.
`Compiler/JetIR/Tests/run-inline-foreign-link.mjs` executes all three
production build-script templates, including shared mutex state, mixed C/C++
state and rejection of an actually missing definition.

The Nix shell keeps ordinary Jet compilation and hosted runtime Rust on the
existing nixpkgs compiler. Native user-Rust helpers have a separate pinned
upstream sysroot including `x86_64-unknown-linux-musl`, exposed only through
`JET_RUST_MUSL_TOOLCHAIN`, `JET_RUST_MUSL_LINKER`, and `JET_RUST_MUSL_AR`.
These opt-in paths do not change `PATH`, `RUSTC`, or the default Cargo target;
only a declared user-Rust helper build selects them. A native helper is a musl
`staticlib`, with its explicit artifact mode included in the content key, never
a GNU Rust archive mixed into Jet's static-musl executable.

The Jet Driver retains checked user-Rust signature facts in source order,
without retaining default-expression ASTs in helper realization.
Its pure planner emits the manifest, safe wrappers, descriptor slot adapters,
and ordered length-delimited bridge key; the CLI realizes that helper before
native lowering and attaches its exact static archive to the checked MIR link
unit without rewriting the foreign symbol; native lowering calls the helper's
`jet_ffi_<declared name>_cabi` descriptor adapter. The 24-byte slot ABI,
caught-panic markers, UTF-8 String lengths, and returned-buffer free operation
match the hosted bridge. Direct C scalar lowering remains a separate unchanged path.
Cargo metadata and the helper build run under the mandatory Linux native
sandbox, never an unsandboxed fallback. Input manifests/source, descriptor,
toolchain, target, artifact mode, native tools/link inputs, artifact digest,
and provenance govern cache reuse. A per-key OS lock outside the key directory
and a shared-output publication lock retain one helper build under concurrency.
`check`, `run` and `build` share this realization and its
`E0703`/`E0704`/`E0705` errors, including canonical declaration hints and separate
Cargo detail. Cargo is resolved only from `PATH`; the selected musl rustc,
linker and archiver belong only to its helper child. Programs without Rust
declarations perform no helper or toolchain probes.
An explicit `JET_FFI_CACHE_DIR` is used verbatim by both hosts and retained by
the native test marker, so a case can inspect its own shared-cache concurrency
contract. `CARGO_NET_OFFLINE` is forwarded only to the isolated metadata child;
an offline resolution failure still follows the shared `E0704` run/build route.
The actual Cargo build keeps the host's explicit `CARGO_BUILD_JOBS` worker
bound and private Cargo/home/target environment, never ambient Cargo settings.
The native metadata sandbox deliberately forwards only `CARGO_NET_OFFLINE`,
not the hosted metadata command's other ambient `CARGO_*` settings; this is a
narrower metadata behavior, not an expansion of build authority.
The outer test-trace wrapper alone sets `JET_FFI_TRACE_WORKSPACE=1`: private
0700 workspaces remain beneath the full key's `.trace-workspaces/<nonce>` until
the audit reads the actual sandbox-mapped manifests. Normal builds still use
an OS TempDir with lexical cleanup; neither mode changes source-read-only or
output-private mounts, locks, dependency identity, or the cache's root key count.

Manifest-less `use package#version` admission reuses the package model's local,
offline inline-dependency resolver: `.jet/inline-deps` is authoritative, with
the explicit fixture root as a fallback only when the package is absent.
The CLI snapshots the selected no-follow authority root; the Driver binds its
recorded name, selector, selected version, and content digest into the source
and package receipts. Loose selectors retain `L0203`, unresolved dependencies
retain `E1253`, and neither resolution path invokes Cargo or rustc.
Admission explicitly handles the fallible optional inline-Package parse before
choosing the manifestless path. Both a present carrier and a malformed carrier
remain owned by the normal package loader; inline-dependency resolution must
not replace malformed-package diagnostics with a dependency refusal.

Checked `#Layout(c)` records preserve native address-sized raw pointer fields
in their generated `repr(C)` definitions. A `Ptr<T>` field lowers to
`*mut core::ffi::c_void`; its pointee need not itself have a C representation.
Fixed-size lists of pointers use that same element representation.

The native C path consumes the same checked direct/transitive library closure
and object/archive link units. A local `c@path` source directory is realized
with the installed C toolchain into a content-addressed static archive; source
and header bytes, preprocessed translation units (including system headers),
exact compiler flags, target and tool identities determine its key. Cache hits
check the archive digest. System V scalar calls preserve
their declared widths and floating-point register classes, and writable scalar
arguments use C-width temporary slots followed by native writeback.

SQLite uses the sanctioned 3.45.0 amalgamation from `libsqlite3-sys` 0.28.0 in
`Core/Vendor/Sqlite3`, with its license and the seed's bundled feature flags,
never a system SQLite. The upstream `sqlite3.c` SHA-256 is
`7956a38f236a6be6c0bb30c96ba4f85f19e5a69f6beb6d2c62c9d246972a6775`.
Packaging ships those sources beside the runtime. User foreign
dependencies are program link inputs; a runtime pack includes only the foreign
closure reached by its own declared sources, keeping the shared pack independent
of user projects.
User C closures and prepared user-Rust helper archives also link the existing
toolchain's musl `libc.a` (including math and pthread), the `libgcc_eh.a`
unwinder and the C compiler builtins as program-only platform inputs. Packaging
places them in `jet-runtime/foreign/platform`. Such a program links the Jet-only
runtime object (`<key>/jet_runtime_jet.o`) with them, not the runtime pack: the
pack holds its own private, initialized libc, and a second copy beside it would
run uninitialized. A program with no foreign closure receives no additional link
inputs and keeps the pack. Platform archives are never runtime-pack exports.

The private `Core.mem.__string_data` / `__string_byte_len` pair lends the
existing UTF-8 String home, not a NUL-terminated or copied carrier. The caller
keeps that home alive and unmodified through its audited byte-span use.
Evaluator routes walk the original frame/place and read-parameter provenance;
comptime expression fragments borrow the original scope before snapshotting.
Neither path keeps a second String/pointer registry. `__bytes_pack` allocates
one packed byte buffer (empty sentinel 1), `__bytes_unpack` makes an independent
owned copy, and `__bytes_free` releases the pack once with its original length.
C String parameters instead use per-call NUL-terminated copies; their String
results are copied before those temporaries are released.
`Core/Mem/Mem.jet` keeps private-loan and packed-byte tests in the trusted
embedded Core lane; ordinary source cannot spell reserved `__` identifiers.
`Compiler/JetCli/Tests/run-c-ffi.mjs` covers declared-source scalar ABI and the
bundled SQLite open/prepare/select/finalize path against exact tier-parity output.

`jet inspect bind cpp` is owned by
[`Cpp.jet`](../../Jetpack/Integrations/Source/Foreign/Binders/Cpp.jet): clang
AST discovery produces a deterministic Jet module and C-linkage shim/archive
under `.jet/bindings/cpp/`, with provenance for the declaration, toolchain,
target, generated sources, and schema.

Generated wrappers keep ownership, errors, layout, callbacks, and task crossings
explicit. Safe Jet cannot acquire an ungated unsafe operation through a bridge.
Executable bridge proof is in `tests/cffi.rs`, the scoped C++ system proof, and
`tests/golden.rs`.

### Jet as guest: embedding contract

A native `Library` is a loadable artifact, not a second Jet process. `#Export(c)`
publishes the entry module's checked C surface, and `#Import(c)` names a C
function that the library calls. This follows D-ADOPT-GUEST1=A: both directions
use the same sema-owned C-safe type law. D-FFI-UNIFY1=A keeps the generated
bridge, descriptor, and native symbol as one mechanism. Where a declaration
uses D-FFI-CAP1=A, `&` is exclusive for that call and `^` transfers ownership;
the host must obey that declaration rather than infer ownership from a header.

The native Library surface maps each parameter and the result of an export,
independently, to one C primitive: `Int` (`int64_t`), `Float` (`double`),
`Bool`, `Text` (JetText), `I8`–`I64`, `U8`–`U64`, or `F32`; a unit result is
`void`. The Jet compiler implements this law; the frozen Rust compiler still
admits only one homogeneous `Int`, `Float`, `Bool`, or `Text` shape.
Function-valued exports and a Library-wide `init`/`shutdown` protocol are not
part of this surface. The loader owns mapping and unmapping. The host owns
process signals, thread creation, and isolation for a failing call.

| Surface | Guarantee | Invalid use | Observable failure |
| --- | --- | --- | --- |
| Initialization | A successful `dlopen`/`dlsym` (or the platform equivalent) is the admission point. The first export call needs no `jet_init`; the artifact has no process-global init hook. | Call a missing symbol or invent a lifecycle symbol. | The loader returns its normal missing-symbol error; the generated Library does not run. |
| Shutdown | There is no `jet_shutdown`. The host waits for all calls and frees returned values before unmapping. | Unmap while a call or worker is active. | No Jet result is defined; the host violated the loader contract. |
| Thread entry | Any host thread may call a live scalar export. Calls may run concurrently; the host joins workers before unmapping. | Pass a non-C-safe value or let a worker outlive `dlclose`. | Sema rejects the former; the latter is invalid C/loader use. |
| TLS | Runtime stack and foreign-failure markers are thread-local; generated call guards restore their depth and markers on normal return. A call does not borrow another thread's state. | Share a call-local capability or use a stale thread-bound value after return. | The declaration is rejected or the host has left the defined contract. |
| Re-entry | A host function named by `#Import(c)` may call a second live export while the first export is active. The nested call uses the same scalar boundary. | Expect a function-valued `#Export(c)` callback; it is outside the primitive Library surface. | Library validation reports `E1341`; no callback ABI is emitted. |
| Signals | The Library does not install, replace, or restore process signal handlers. Signal policy stays with the host. | Assume a signal handler belongs to the Library or call during unsynchronised host signal mutation. | The host owns the resulting signal behavior; the Library reports no signal protocol. |
| Allocator ownership | `Text` results are allocated by the Library and are released exactly once with its generated `jet_text_free`, before unmapping. | Call C `free`, double-free, retain after release, or release after `dlclose`. | These are outside the C contract and may be undefined behavior; the host must prevent them. |
| Panic containment | A panic cannot unwind into C. The boundary emits `Stop [E3001]` and terminates the calling process with status `70`; it never fabricates a scalar success value. | Expect an error return or continued execution in the same process. | The process exits `70`; a host that must continue forks the call and checks the child status. |
| Repeated load/unload | A host may repeat map → resolve → call → free → join → unmap when every cycle obeys the rows above. | Reuse function pointers or `JetText` after unmapping. | Behavior is invalid C/loader use, not a recoverable Jet call. |

The executable proof in `tests/library_outputs.rs` exercises two concurrent
threads, nested `#Import(c)` re-entry, signal preservation, allocator release,
three load/unload cycles, and a forked panic call whose parent continues.

### Incremental Compiler Service

The package is the unit of incremental checking. This follows the owner
ruling of 2026-09-30, which amends layer 2 of D-INCR-UNIT1 from the module to
the package. A package is a directory with `package.jet` (recursive, minus
nested packages) or a single file with a leading `package { … }` header. Its
files share one namespace (D-MOD-CYCLE1=A), and packages form an acyclic
graph. Design and prior art:
[incremental-checking-design-2026-09-30](../research/incremental-checking-design-2026-09-30.md).

**What a package check reads.** Only its own sources and manifest, the
compiler and Core identity, the target facts sema reads, the interface
records of its direct dependencies, and the inputs it discovers through the
sealed reader. It never reads dependency bodies beyond the templates an
interface publishes, and it never reads the root program's policy.

**The interface record.** A check produces a versioned record containing:

- exported declarations with bodies erased;
- the inferred facts importers depend on: effect rows, failure sets,
  ownership and memory summaries, OS and web facts, the trait and impl
  table, and constant values;
- templates (generic, inline, and comptime-evaluated bodies);
- per-item fingerprints;
- the usages it read from each dependency.

Declaration locations are kept outside the interface digest, and importers
refer to them symbolically.

**Keys and reuse.** A package's check key covers:

- its source digest;
- the sema-visible target facts;
- the compiler and Core identity;
- its direct dependencies' interface digests.

A package is green when a record exists for that key and its discovered
inputs still verify. It is also green when every dependency item in its
usages kept the same fingerprint. Otherwise it is rechecked. When the
recheck yields the same interface digest, importers stay green: this is
early cutoff. Inside a rechecked package, items reuse their previous results
by red/green over recorded, ordered reads.

**Program phase.** Facts that flow down the graph run once per program over
package records, never bodies. These are:

- outputs and the entry;
- authority and effect budgets checked at dependency edges;
- the web partition, app graph, and job graph;
- unreachable exports;
- the `used_core` closure;
- diagnostic ordering.

**Storage.** One `.jet/` folder at the workspace root holds all per-workspace
state: the lock, last-run (priors) pointers, the stamp table, the records and
receipts index, reports, build outputs, and logs. Packages never get their own
`.jet/` folder, and code that creates `.jet/` paths resolves the workspace root
first (owner ruling, 2026-09-30). The
content-addressed records stay in the machine-wide `jet-store`, shared across
workspaces (D-BUILD-STORE1=E; record kinds in `jet_store::records`), and use
one std-only binary codec with a trailing SHA-256
(`jet_foundation::RecordCodec`). A missing or corrupt record is a miss, never
an error.

**Sealed reads.** Every file, directory listing, and environment variable that
loading, sema, or compile-time evaluation reads while checking goes through
`jet_foundation::CheckReads`. A recorded result declares those reads with
their digests and re-verifies them before reuse; a read that changed during
the check, or a network input, leaves the result unrecorded. The read audit
(`JET_CHECK_READS_AUDIT`) proves the declaration complete: a verified record
does not replay, the program is checked fresh, and any read of the fresh
check that the record does not declare with the same digest fails as an
internal compiler error. `JET_CHECK_READS_INJECT` injects one undeclared read
so tests can show the audit catches it.

**Self-hosted compiler.** JetFoundation implements the same codec
(`Record/RecordCodec.jet`) and key framing (`Record/PackageIdentity.jet`);
`tests/record_conformance.rs` holds both implementations to identical bytes.
JetDriver reaches the store only through the typed `JetDriverRecordStore`
the host passes in the compile request. Without a store every lookup is a
miss and the package is checked.

**Diagnostic replay.** Diagnostics are stored typed and rendered for each
invocation. Warm output, text and `--json`, is byte-identical to
`--no-cache`. `--verify` recomputes and compares.

**Build lenses.** They reuse per-package compiled output keyed by the
package's body digest and its dependencies' interface digests.

**Regression gates.** Deterministic counters are the gates: packages and
items checked and reused, and records decoded. Timings are observations
under the compiler-speed method below.

**Editor tooling.** D-LSP1 makes editor tooling a client of the same front
end, not a second checker. `Compiler/JetQueries` is a demand cache
for file inputs and derived query values. The bounded loader prepares open or
discovered sources with at most eight workers and consumes them in stable
order. The server records cancellation concurrently with
request execution and replaces a cancelled in-flight result with JSON-RPC
`-32800`. D-LSP2 requires every advertised LSP feature to have named coverage
in `tests/lsp.rs`; the server must not advertise speculative features.

### Compiler-speed evidence boundary (#666)

Compiler-speed evidence stays on the production compiler path. The differential
gate sends the same checked example to optimized AOT and the default `jet run`
(normal build path, D-EXEC2=A) and to the forced evaluator, then compares exit
status, stdout, and stderr; a refusal, newly broken AOT oracle, missing record,
or divergent result remains visible as a failure. Named-job parity uses the
same checked program-argument slice for `jet run`, `jet dev`, and interpreter
execution; an unknown job remains an E1294 failure.

The timing path uses `PhaseTiming` and the typed `CompilerProbe` provider. A
checked corpus pins source and expected-output digests. A measurement records
compiler/Core identities, target, profile, backend, linker, host, cache state,
fixed warmups and samples, process CPU-time variance, peak RSS, and phase totals. The
`dev` and optimized `release` profiles both build through the native back
end (D-EXEC2=A). Clean, no-change, and representative-
edit runs are distinct. Partial timing, changed inputs, incompatible
identities, nondeterminism, pathological inputs, and unstable samples are
unavailable/failure; they cannot be made green by changing the workload or
hiding cold-cache work. The checked-corpus report is schema version 4 and emits
six rows for each active corpus row: `jit-clean`, `jit-no-change`,
`jit-representative-edit`, `aot-release-clean`, `aot-release-no-change`, and
`aot-release-representative-edit`. The `jit-*` rows measure `jet run`, which
today runs the transitional build-and-execute path (D-EXEC2); their names
predate the Cranelift retirement. The current corpus has five active rows, so
the matrix has 30 rows. Each row uses one warmup and twenty measured samples.
Its top-level receipt records verified semantic, diagnostic, effect, and tier
parity. Its pinned baseline allows at most 15% latency or peak-RSS regression,
an interquartile spread of 100% or less, and at most five Tukey-fence outliers;
exact stdout/stderr hashes, phase totals, and workload identity remain part of
each row. The five active corpus rows are ordered
`Examples/features/basics/hello.jet`,
`Examples/features/collections/wordcount.jet`,
`Examples/features/serde/json.jet`,
`Examples/features/basics/pattern_matching.jet`, and
`Examples/features/devloop/job_runner.jet`; `job_runner` is fifth.
A baseline is valid only for its pinned machine and toolchain.

The code-action engine uses the same semantic index. It ships unique workspace
imports, binding and function extraction, and immutable-binding inline actions.
Each action returns edits for the current document version. The engine rejects
effects, possible traps, unstable mutable reads, and dirty import sources.
Function extraction accepts only `Bool`, `Char`, `Int`, and `Float` parameters
and result expressions because the index proves these values are safe to copy.
The code-action test rejects nominal `Clock` inputs. The helper-level
`code_actions_reject_non_scalar_return_at_type_gate` test passes a sema-checked,
total, pure `String` result to return inference. Inference rejects that result,
and the function-extract helper returns no action. Other non-scalar parameters
and results stay unsupported until sema supplies a complete ownership contract
for reads, writes, takes, and returned values.

## Application plugin host boundary (D-PLUGIN1 / D-PLUGIN-EXPORT1 / D-DEP-WASM1)

An application sandbox is a prebuilt WASM Component Model guest loaded by the
`core.plugin` host; the compiler has no sandbox build target. The native library boundary is
different: a native library is trusted code and has no Wasmtime sandbox or
implicit capability reduction. Both surfaces keep the same checked export
shape, but only the Component path makes an isolation guarantee.

The Component loader reads the module below the caller's canonical,
resource-scoped `FS.Read` authority, then preflights every import as a typed
`HostImportFact`. The declared `authority.needs` and the one authority lent to
the load must cover each fact before the linker registers it or instantiation
can occur. An unknown or denied import fails closed; no adapter can reach the
filesystem, network, process, or another external effect first. An empty
grant is explicit zero authority, never an ambient fallback. Host adapters
re-check the typed decision at the call edge and apply the same resource scope
to path/endpoint arguments.

The host applies fuel, linear-memory, table, wire-size, and wall-call budgets.
The failure envelope records which budget was actually consumed; a guest
memory/table request is not relabeled as fuel exhaustion, and an oversized
wire is rejected before an unbounded host encoding. A call trap is classified
as user/guest failure, denied authority, budget exhaustion, or internal host
defect from typed boundary state, not by parsing backend error text.
D-PLUGIN-FAILURE1=A carries that cause as `core.plugin.PluginError`, with the
export name, original display message, and up to 32 innermost-first Wasm frames.
Budget failures name Fuel, Memory, Table, Time, or the 16 MiB Wire limit. A
`PluginFrame` locates the function by name (empty without a name section), the
Wasm `component` it ran in, and the offset; debug-file/line loading is
intentionally outside this transport. Failures before guest execution carry no
frames. A failed load leaves a handle whose first call returns the load fault
with export `load`, without stopping unrelated host work. A propagated failure
records the host call site in the ordinary failure journey.

The authority, guest-failure, and resource-budget witnesses run on all hosted
execution routes: default `jet run`, forced `--interpret`, and release/AOT
`--release`. Each route is required; unavailable release tooling is not a
passing or skipped authority witness.

Handles and Wasmtime stores are owner-thread state. Cross-thread use and
unload of an active call are rejected by the existing handle boundary, and
nested host/guest entry is explicitly rejected rather than borrowing the
thread-local store map recursively. Bounded external adapters (network and
process) use the same call deadline; cancellation stops the deadline worker
before the store becomes idle. Component values are copied while the call
owns guest memory and before `post_return`; no borrowed guest or host view
escapes its call/owner.
A failed guest returns a Jet error frame and keeps any failure scoped to its
own instance; unrelated native work is not terminated.

This boundary does not turn a native library into a sandbox and does not make
the sandbox an ambient host. New host capabilities or a new public error API
require a ratified ballot rather than an adapter-local policy.

## Compiler-extension plugins (D-DX5-HOOK1=A)

After sema, the compiler may freeze a **versioned typed read-only snapshot** and
send it to an isolated WASM Component Model guest. The guest returns structured
findings and edit proposals; the host validates every response and remains the
only semantic authority (I2/I3).

The boundary owner is
[`CompilerExtension.jet`](../../Jetpack/Integrations/Source/CompilerExtension/CompilerExtension.jet),
which owns the versioned snapshot, response validation, and lifecycle.
Ordinary `jet` processes never link or initialize Wasmtime.

The WIT world is `compiler-extension-v1`
(`package jet:compiler-extension@0.1.0`, export `analyze`), distinct from
application plugins' fixed world `jetplugin` (D-PLUGIN1 / D-PLUGIN-EXPORT1).
The host does not load PATH-discovered `jet-*` helpers or application
`target: sandbox` / `core.plugin` loaders.


### Protocol / schema (exact)

`analyze` carries opaque `list<u8>` payloads. Host-owned wire format is UTF-8
JSON with lexicographic key order and no insignificant whitespace
(`CompilerExtension::{TypedSnapshot,AnalyzeResponse}`).

**Snapshot** (`protocol=1`, `stage="typed"`, `trust="untrusted"`):

| Field | Meaning |
|-------|---------|
| `abilities` | Negotiated subset of `read_types`, `read_symbols`, `read_effects`, `read_spans`, `read_provenance`, `emit_finding`, `propose_edit` |
| `limits` | `max_fuel`, `max_memory_bytes`, `max_table_elements`, `max_findings`, `max_edits`, `max_response_bytes`, `timeout_ms` |
| `types` | `{id, repr}` |
| `symbols` | `{id, name, kind, type_id, span_id, effects, provenance}` |
| `spans` | `{id, file, start, end}` |

**Response:** `{protocol, findings, proposed_edits, artifacts}`. Findings are
`{rule, span_id, message, severity}` with `severity ∈ {error,warning,note}`.
Edits are `{span_id, replacement, rationale}`. V1 requires `artifacts: []`.

Unknown keys are rejected. Span/type refs must resolve. Findings need
`emit_finding`; edits need `propose_edit`. Counts and raw byte length must
fit `limits`. Successful validation **stages** output only — the host alone
may accept; guests never mutate compiler facts or expose rustc (I2/I3).

### Limits, trust, lifecycle, rollback

- **Defaults:** fuel `10_000_000`, memory `16 MiB`, table `10_000`, findings
  `256`, edits `64`, response `256 KiB`, wall budget `timeout_ms=2000`.
  Loader applies fuel + `StoreLimits`; each `analyze` arms wasmtime epoch
  interruption and ticks the engine after `timeout_ms` (fail-closed interrupt
  trap). Snapshot declares the same caps.
- **Trust:** v1 admits only `untrusted` components (zero host imports).
- **Deterministic sandbox (D-DX5-HOOK1):** the host linker registers no
  imports, so guests get no ambient clock, random, filesystem, network, or
  process. Components that declare any host import (for example WASI random
  or clocks) fail closed at load — Jet-owned `E:` wire, no session commit,
  no rustc leak. Pure guests over a frozen snapshot are the only admitted
  shape; the host never supplies nondeterminism sources.
- **Lifecycle:** `ExtensionSession` Idle → Loaded → Closed. `stage_response`
  validates without commit; `rollback` discards staged output only (an
  accepted commit latch stays final — restage requires a new session);
  `close(close_guest)` invokes the WASM host closer
  (`jet_compiler_extension_close`) so guest Store/memory is dropped.
  Uncommitted work never reaches sema or codegen.
- **Process IPC:** when configured, `jet-driver` resolves `jetpack` only beside
  the current Jet executable (never PATH), invokes hidden versioned verb
  `__compiler-extension-v1`, writes at most `16 MiB` of snapshot bytes to
  stdin, and accepts at most the snapshot's `max_response_bytes` on stdout.
  Stderr is capped at `64 KiB`; the outer process deadline is `5000ms` and
  kills a stuck/crashed host. Nonzero exit, malformed output, timeout, and
  missing sibling all map to Jet-owned E1402.
- **E2E harness (C4 technical):** `Jetpack/CLI/Tests/CompilerExtensionHost.jet`
  loads real `compiler-extension-v1` component fixtures under
  `tests/fixtures/compiler_extension/` through the compiled compiler-extension
  host. Proves one custom-lint finding
  round-trip, fail-closed crash / malformed / incompatible / fuel-exhaust /
  WASI-random-import guests, wall-clock epoch `timeout_ms` interrupt, and
  byte-identical re-analyze of a pure guest (Jet-owned `E:` wires; no rustc
  leak; no auto-commit).
- **Post-sema driver wire:** when `JET_COMPILER_EXTENSION` names a component
  path (expert env registration — no new user syntax until a spelling ballot),
  `jet-driver::CompilerExtensionHook` freezes a typed snapshot after sema,
  exchanges it with the sibling host, then maps validated findings to `L1401`
  or host/process failures to `E1402`.

## Rules

- **R1 — Codegen is dumb.** No checks, no decisions, no "see if the back end
  accepts it". If codegen needs to know something, sema should have
  established it.

  **I1 amendment (D-LL1, ratified 2026-06-16, E2-M13).** I1 originally read
  *"no `unsafe` in the language or generated code, ever (v1)."* The expert
  low-level tier (S58) amends it: generated `unsafe` appears **only** inside
  user-written gated regions — an `#Unsafe("reason") { … }` block or an
  `#Unsafe("reason") fn` contract, both
  unlocked by `use core.mem` — plus vetted std/mem internals. Ordinary,
  memory-safe Jet still emits **zero** `unsafe`; the boundary is enforced by
  sema (E3101/E3102/E3103) and tested in `tests/golden.rs` (every example but
  the audited `48_lowlevel` must contain no `unsafe`, and even there every
  `unsafe` must be a gated `unsafe {`/`unsafe fn` form). Codegen stays dumb:
  it lowers an already-checked `#Unsafe` region straight to a Rust `unsafe`
  region and makes no safety decision of its own.
- **R2 — Sema is the gatekeeper.** Any program that passes sema must lower to
  MIR that every back end accepts. New language features land as: spec →
  parser → sema checks → codegen → tests, in that order.
- **R3 — Single surface.** User-typeable strings live in
  the syntax registry under `Compiler/JetFoundation/Data/Syntax/` only. Renaming a keyword starts there;
  parser/formatter tests, generated grammars, snapshots, and docs must move with
  it.
- **R4 — Spans everywhere.** Any AST node an error might point at carries
  its span. Adding a node without a span is a review-blocker.
- **R5 — ICE policy.** A back end failing on checked MIR is an internal
  compiler error: it prints the internal-compiler-error banner, exits 101, and
  is treated as a P0 bug. A missing linker or C library is a tool/user
  diagnostic, not an ICE.
- **R6 — Name mangling.** In generated Rust (the transitional path), user
  identifiers are emitted as `__jet_<name>` (`main` excepted) so user code can
  never collide with Rust keywords, macros, or std items. The sema-owned `NameLedger` is the single source for
  declaration paths, aliases, visibility, and reference origins; all Rust-name
  projections use its canonical mangle functions. This implements ratified
  D-NAME-TREE1 without adding a user-facing spelling.
  Selective file imports also carry the checked namespace target keyed by
  their declaration span. This includes sealed Core parts selected by their
  reserved catalog name even when the namespace import uses a different local
  alias. Rust emission consumes that ledger edge, rather than resolving the
  catalog again. The Jet driver carries the same canonical target through
  `SemaRegistrationImportEdge` and lowers checked member identities into MIR.
- **R7 — Backend is swappable.** Every back end reads the same MIR
  (D-TIER-FORM1): the native code generator in `Compiler/JetBackend` and the
  Web emitter in `Compiler/JetCodegen/Source/Emit/Web`. The lexer, parser, and
  sema do not depend on any back end; replacing one changes only the codegen
  and binary-build edges.
- **R8 — Small, self-contained binaries.** The native back end links each
  program's objects against the content-addressed native runtime pack
  (`jet-runtime/<key>/jet_runtime.pack.o`, see the Pipeline section); no Rust
  runtime, Rust std or rustc participates. The linker keeps only what the
  program uses ("only link what's needed"). The output is one self-contained
  native binary. A size-minimal profile is decision S15; the Jet CLI does not
  yet accept `jet build --small` (`JET_CLI_BUILD_UNPORTED_FLAGS` in
  `Compiler/JetCli/Source/Cli/Front.jet`). The default leans toward speed.
- **R9 — A file is a complete program.** `jet run foo.jet` compiles and
  runs a single file with no manifest, no project folder, and no config.
  Inside a package, a bare `jet run` resolves the same complete program from
  `run.jet`, `src/run.jet`, or `<package>.jet`; `jet dev`, `jet check`,
  `jet build`, and `jet test` share that resolver, while an explicit source
  path stays explicit.
  The build does not create or require a project for user code. Users may
  supply package structure when their package contract requires it; the
  compiler does not impose one.
- **R10 — Std is pay-for-what-you-call.** Core standard-library modules are
  compiler-known namespaces, but importing them is free. Sema records the
  core helpers that a checked program can call, and codegen emits only those
  helper templates. A program that imports every core module but calls none
  should stay in hello-world size territory.

  Compiler-known Core behavior lives in the source-owned Core modules under
  `Core/` and the prelude the compiler embeds from `Compiler/Prelude/` (through
  the generated registry files); rebuild `jet` before smoke-testing a change to
  an embedded part. A source-owned Core module such as `core.archive` is
  compiled from its canonical `.jet` module through the ordinary frontend; it
  does not keep a copied fallback template or a hidden semantic bridge.
- **R11 — Generated code enters the front end.** Every typed build-time
  generation step — a derive body, a comptime splice, or a metaprogram — parses
  its item template with the ordinary grammar, fills typed holes at expansion,
  and sends the filled items through sema exactly like hand-written code. A
  build materialization boundary may format those checked items into a `.jet`
  file, but no generation path may inject unchecked AST or source text past the
  sema gatekeeper (R2). The guarantee that buys:
  generated code is trustworthy-by-construction (R1 codegen-dumb, R2
  sema-gatekeeper, R5/I2 rustc-never-speaks all keep holding through
  generation), and any error in generated output surfaces as a **real sema
  diagnostic pinned to the user's trigger site** — the struct, field, or derive
  marker that caused it — never as raw rustc output. The `#Codable` derive
  follows this shape for derives and build-time steps (S56 user derives,
  comptime).
  (D-META-CODE1/D-META-BODY1 supersede the old source-reparse route.)
- **R12 — One semantic core, every engine a dumb exhaustive consumer.** TIR is
  the single structured IR after sema. Every executable variant carries semantic
  facts (places, types, patterns, method identities) — never pre-rendered target
  source text. **Core/runtime meaning lives only in the embedded Prelude parts /
  CoreLib** (`Compiler/Prelude/`, `Core/` and `Compiler/JetRuntime/`). Target
  spelling lives only in each back end as calls into that Prelude. Interpreter
  ambient bindings are the same
  kind of layer: marshal args, call the identical `jet_*` / Prelude function
  AOT would call, marshal results. They must not fork defaults, CORS/policy
  checks, error meaning, or other Core behavior. Every engine (the native back
  end, the canonical interpreter, and web when the surface applies) must consume TIR
  exhaustively with real lowering that preserves one meaning (AGENTS.md
  invariant I9 / `claim.tier-parity`). Every supported construct needs that
  lowering; an unsupported fall-through is a compiler defect, not a closed
  feature or an AOT-only exception. Parity evidence covers stdout, stderr, exit
  code, diagnostics, panics, and side effects across applicable tiers; semantic
  parity across AOT, interpreter, and web is mandatory. (D-ONECORE1=A, ratified
  2026-07-24; I9 owner-directed 2026-07-29; dumb-adapter rule owner-directed
  2026-07-29; D-EXEC2=A retired the Cranelift JIT.)

  Sema preserves borrowed pattern payload aliases in both ordinary
  `if` conditions and dispatch tables, while owned or taken subjects keep their
  payload ownership. Payload-derived views retain the matched parameter's
  projection origin; local owners and consumed parameters still cannot export
  those views (E2305). Mutable payloads reborrow only their actual
  parent capability: a lexical child suspends overlapping parent accesses,
  unrelated overlapping views remain conflicts, and scope exit restores the
  parent. Field assignment reinitializes only the written path and its
  descendants, never a moved ancestor.

  Source-owned Core policy must preserve the public provider carrier, including
  methods, nominal traits, ownership conventions, and labeled optional controls.
  A descriptor record or fieldless source struct is not a substitute for an
  opaque native handle. Native optional payloads are converted once at the Core
  call boundary to Jet's owned absence carrier; callers must not reinterpret that
  carrier as a foreign optional. Scheduler-gated host steps remain in the local
  runtime rather than being dispatched into a crypto-only FFI bridge.

  The comptime Core registry uses the following namespace classification. The
  table covers the namespaces of the generated
  [`CoreCallRows.jet`](../../Compiler/JetFoundation/Source/Registry/CoreCallRows.jet).
  `Kernel` means that comptime marshals `CtValue` into the exact Prelude part.
  `Intrinsic` means that the evaluator implements a language value operation,
  not a second Core policy. `Host` means that the call crosses an effect or
  authority boundary. Every listed semantic rule has one Prelude/Core home;
  engines only marshal values to it.

  | Namespace | Class | Semantic home |
  |---|---|---|
  | `core.archive` | Host | Package bridge; comptime rejects unsupported host work. |
  | `core.args` | Kernel | `Core/Args/Args.jet`; comptime marshals values. |
  | `core.auth` | Kernel | `Core/Auth/Auth.jet` and the crypto entropy kernel; comptime marshals values. |
  | `core.archive.gzip` | Host | Native compression boundary; comptime rejects unavailable calls. |
  | `core.archive.zstd` | Host | Native compression boundary; comptime rejects unavailable calls. |
  | `core.compute` | Kernel | `Core/Compute/Compute.jet`; comptime marshals values. |
  | `core.crypto` | Kernel | `Core/Crypto/` owns typed crypto rules; every engine marshals values. |
  | `core.crypto.expert` | Kernel | The audited raw-byte route shares the crypto Prelude symbols; the `#Unsafe` gate is policy, not a second engine implementation. |
  | `core.data` | Kernel | `Core/Math/Stats.jet` and `Core/Data/Plot.jet`; comptime marshals values. |
  | `core.data line renderers need groups` | Kernel | `Core/Data/Plot.jet` owns line-renderer argument validation. |
  | `core.data line renderers need options` | Kernel | `Core/Data/Plot.jet` owns line-renderer option validation. |
  | `core.data: argument must be `[DataGroup]`` | Kernel | `Core/Data/Plot.jet` owns `DataGroup` validation. |
  | `core.data: argument must be `[Float]`` | Kernel | `Core/Math/Stats.jet` owns numeric input validation. |
  | `core.email` | Kernel | `Core/Email/Email.jet`; comptime marshals values. |
  | `core.encoding` | Kernel | Encoding dispatch and value validation use the shared encoding Prelude/Core kernels. |
  | `core.encoding.jsonl.to_string: expected a list` | Kernel | JSONL value validation stays with the encoding kernel. |
  | `core.encoding.base32` | Kernel | `Core/Encoding/Base32.jet` owns encode and edition-aware decode; comptime marshals values. |
  | `core.encoding.base64` | Kernel | `Core/Encoding/Base64.jet` owns encode and edition-aware decode; comptime marshals values. |
  | `core.encoding.cbor` | Kernel | Typed CBOR marshals through the shared Dyn/codec kernel. |
  | `core.encoding.csv` | Kernel | Typed CSV marshals through the shared codec kernel. |
  | `core.encoding.hex` | Kernel | `Core/Encoding/Hex.jet` owns encode/decode; comptime marshals values. |
  | `core.encoding.json` | Kernel | Typed JSON uses the shared Dyn/codec kernel. |
  | `core.encoding.jsonl` | Kernel | JSONL uses the shared typed JSON kernel. |
  | `core.encoding.toml` | Kernel | Typed TOML uses the shared codec kernel. |
  | `core.encoding.xml` | Kernel | `Core/Encoding/XmlKernel.jet`; comptime marshals `Dyn` values. |
  | `core.encoding.yaml` | Kernel | Typed YAML uses the shared codec kernel. |
  | `core.builtin` | Intrinsic | String length and map projections marshal through the shared builtin value operations. |
  | `core.sys` | Host | Explicit comptime environment authority and policy boundary. |
  | `core.event` | Host | Callback registration crosses the runtime closure boundary; the host only marshals shared event values. |
  | `core.files` | Host | Explicit comptime filesystem authority and policy boundary. |
  | `core.text.fmt` | Kernel | `Core/Text/Fmt.jet` owns number, decimal, grouping, byte, duration, ordinal, plural, and padding rules; adapters marshal values. |
  | `core.term` | Host | I/O authority boundary (`Core/Term/Term.jet`). |
  | `core.math.abs: non-numeric argument` | Mixed | Numeric argument validation stays with the shared math rules. |
  | `core.math` | Mixed | `Core/Math/` owns residual integer, gcd/lcm, factorial, and erf-family rules; true numeric intrinsics remain intrinsic. |
  | `core.net.mime` | Kernel | `Core/Net/Mime.jet`; comptime marshals values. |
  | `core.net` | Mixed | `Core/Net/Ip.jet` owns IP/socket parse and field rules; sockets and DNS stay authority-bound. |
  | `Path` / `core.files` | Kernel + Host | `Core/Files/Path.jet` owns lexical join/parent/extension/stem/normalize rules; filesystem work stays host-bound and accepts `String | Path`. |
  | `core.perf` | Host | Runtime performance authority; comptime rejects unavailable calls. |
  | `core.process` | Host | Process authority boundary and REPL host adapter. |
  | `core.math.random` | Mixed | `Core/Math/Random.jet` owns deterministic seeded RNG rules; ambient RNG stays host-bound. |
  | `core.game.raylib` | Intrinsic | `CtValue` constructor only; runtime graphics work stays host-bound. |
  | `core.reactive.loadable` | Kernel | `Core/Reactive/Loadable.jet` owns tag/presence rules; typed payloads and handles remain adapters. |
  | `core.reflect` | Intrinsic | Compiler-owned type metadata construction. |
  | `core.regex` | Kernel | The generated regex engine exposes one shared matching and validation kernel. |
  | `core.units` | Kernel | `Core/Units/Units.jet`; all tiers marshal `(value, uncertainty)`. |
  | `core.service` | Kernel | Typed tree declarations and the shipped mailbox, lifecycle, authenticated directory/routing, handoff, rollback, partition, reconciliation, and typed observability paths use `Core/Service/Service.jet`; structured export uses the existing `core.log` sink. |
  | `core.data.sketch.cms` | Jet | `Core/Data/Sketch/Cms.jet` owns seeded hash selection, unit saturation and additive source merge over plain value storage. |
  | `core.data.sketch.hll` | Jet | `Core/Data/Sketch/Hll.jet` owns the 256-register estimator over plain value storage; seeded FNV lives in `Core/Data/Sketch.jet`. |
  | `core.data.sketch.reservoir` | Jet | `Core/Data/Sketch/Reservoir.jet` owns the shipped xorshift seed, Algorithm R and slot order over plain value storage. |
  | `core.data.sketch.tdigest` | Jet | `Core/Data/Sketch/Tdigest.jet` owns unit-weight insertion, compression and step quantiles over plain value centroid storage. |
  | `core.compute.solve` | Kernel | `Core/Compute/Solve.jet`; comptime marshals state. |
  | `core.sync` | Kernel | `Core/Sync/Sync.jet`; comptime marshals values. |
  | `core.testing` | Mixed | Shared deterministic helpers call Prelude; harness and host work stay adapters. |
  | `core.text` | Kernel | `Core/Text/` plus the Unicode tables under `Core/Text/Runtime/` owns text/Unicode rules, including scalar names; comptime marshals values. |
  | `core.text.ends_any: non-list argument` | Kernel | Text argument validation stays with the text kernel. |
  | `core.text.starts_any: non-list argument` | Kernel | Text argument validation stays with the text kernel. |
  | `core.time` | Mixed | `Core/Time/` owns civil, duration, and zone rules; clock reads remain host effects and every engine marshals the same Prelude symbols. |
  | `core.net.tls` | Host | Native TLS boundary; comptime rejects unavailable calls. |
  | `core.ui` | Intrinsic | `CtValue` constructors and field projection only. |
  | `core.ui.box() needs UINode children` | Intrinsic | UI node-shape validation stays with the intrinsic constructor path. |
  | `core.ui.box() needs [UINode]` | Intrinsic | UI list validation stays with the intrinsic constructor path. |
  | `core.ui.node_role(): missing role` | Intrinsic | UI role validation stays with the intrinsic constructor path. |
  | `core.net.url.data: first argument must be a Mime` | Kernel | URL data validation stays with the shared URL/MIME kernel. |
  | `core.net.url.data: mime.sub` | Kernel | URL MIME field validation stays with the shared kernel. |
  | `core.net.url.data: mime.top` | Kernel | URL MIME field validation stays with the shared kernel. |
  | `core.net.url` | Kernel | `Core/Net/Url.jet` owns URL parse/render/percent rules; comptime marshals values. |

- **R13 — An abort is never an outcome.** A host seam that runs inside an
  execution engine converts a runtime panic into the tier's **existing** status
  channel before returning: a cancel or a blown deadline lands on the
  pending-interrupt channel `#Shield` already uses, a program stop renders
  through the shared report boundary and exits `70`, and an engine defect takes
  the branded ICE rail and exits `101` (I2). No second error channel is
  introduced (I8), and the runtime [`Scheduler.jet`](../../Compiler/JetRuntime/Source/Scheduler.jet) stays the only place a caught
  payload is read as a status (I9). A process signal handler must be
  panic-free: its whole body notes one relaxed atomic.


## Exit codes (stable contract)

The `jet` driver returns one of a small, stable set of exit codes so
scripts and CI can branch on the outcome without parsing output. This
table is pinned by the `tests/cli/` transcripts and is part of the public
contract — codes are never repurposed.

| Code | Meaning | Who produces it |
|------|---------|-----------------|
| `0`   | Success. (Includes the no-args greeting — orientation, not an error.) | driver |
| `1`   | An unhandled entry error report, or a driver-reported user problem such as a failed `check`, missing file, or failed `test`. | fallible-by-default entry boundary or driver |
| `2`   | Usage error: unknown subcommand (E2101), unknown/ambiguous flag (E2102), or a missing required argument. | driver |
| `70`  | A built program breached or stopped at runtime (`panic`/`require`, `#Todo`, a raw Prelude panic, S36, or another program-side fault). Every producer enters the shared report and cleanup boundary; `jet run` forwards the result. | the user's program / Prelude boundary |
| `101` | Jet's own compiler defect (I2/R5): a back end failed on checked MIR or the compiler reached an impossible state. Never a user-program exit. | compiler |

The final boundary keeps the same report and changes only its transport.
Native command-line output prints the frame and uses code 1. A Web target
raises a typed error object, a Wasm module returns a host-readable error
value, and a Service writes one structured report record. The selected build
target chooses that boundary; runtime inspection never does.

Explicit `process.exit(code)` and `os.stop(code)` use the same cleanup law.
The boundary runs deferred closes in reverse declaration order, scope guards in
reverse registration order, and `atexit` handlers in registration order. A
host kill or abort skips all three mechanisms; finalizers registered after the
stop are never run.

Presentation is TTY-aware (E2-M3): color and progress appear only when
the relevant stream is a terminal. `NO_COLOR` and `--color=never` force
plain output; `FORCE_COLOR` and `--color=always` force color; `--color=auto`
(the default) defers to TTY detection. Piped or CI output is always plain,
deterministic, ANSI-free bytes — scripts never parse escape sequences.

## Testing strategy

The canonical [Jet testkit](../../Tools/testkit/run.jet) owns diagnostic
snapshots (`ui`), executable examples and runtime fixtures (`golden`), and CLI
scenarios (`cases`). Its package test entry is the fixture gate:
`JET=/absolute/path/to/jet jet test Tools/testkit` runs each suite as one test
(`suite golden`, `suite ui`, `suite cases`; a positional filter selects them),
and a failing test names every failing case. `TESTKIT_ARGS` passes testkit flags such as `--filter` or
`--tier`; scratch stays disk-backed outside the checkout. O0, O1 and O2 compare the real exit
and exact streams; diagnostic text remains a product surface, not a blessable
implementation accident.

`JET=/absolute/path/to/jet Tools/agent/verify-full.sh` runs the complete native
suite inventory and source-pinned compiler registries, collecting independent
failures. Missing, refused, skipped and non-PASS verdicts do not close the gate.
It uses an already-built compiler, never Cargo to build Jet's compiler/tests,
and never a Rust MIR producer. `Tools/agent/native-suite.mjs` exposes the shared,
pure post-closeout command descriptors and strict receipt validator; it has no
CLI or closeout-bypass flag. Under owner directive #59, native cutover executes
those runners directly and reports formal qualification/closeout separately as
**NOT MET (deferred)**, never as a suite pass.

Native cutover uses one outer process trace. Testkit's exec-only
`native-command.mjs` labels each case without changing its command PID or streams;
`native-trace.mjs` audits ancestry, cwd and successful executions after the trace
closes. Rust/LLVM defaults to a violation. The committed
`Tools/testkit/user-rust-ffi.allow` has four columns: case identity, exact reason,
compiled source provenance and declared dependencies. A `.case` names precisely
the fixture sources its commands compile; direct Jet cases name themselves.
Receipts retain source hashes. Pure-Jet fixtures receive no Rust exception.
Generated helper manifests must reside in the cache that the command actually
uses: an explicit `JET_FFI_CACHE_DIR` is preserved verbatim, otherwise the
external case/dependency-digest cache applies. Dependencies and admitted Jet
foreign ancestry must match. Cargo metadata is restricted to
`metadata --format-version 1 --manifest-path <generated helper manifest>`,
under the same checks as builds. The immutable tracing wrapper enables retained
private workspaces beneath `<cache>/<full-key>/.trace-workspaces/<pid>-<attempt>`;
the source is mounted readonly at `/work/source`, output writable at
`/work/output`. Audit maps those exact mounts back to retained manifests, rejects
overlays/escapes, and records their bytes and hashes. Case/tier scratch is retained
only for the outer audit; ordinary runs keep their existing cleanup.
The exact `rustc -vV` identity query is admitted only in an allowlisted case's
Jet foreign branch, and is recorded separately. Extra query arguments or sysroot
probes do not inherit that exception. Jet program compilation/linking/running
never gets a Rust
exception. Receipts distinguish zero-Rust cases, observed user-FFI builds and
violations; performance peers remain separate product comparisons.

UI `.fixed.jet` companions carry their own case marker and exact allowlist row,
not the negative fixture's identity. A source-declared `extern rust "std"`
uses the generated Cargo staticlib helper with an empty dependency set; it is
not a zero-Rust case. Empty sets are admitted only for actual extern-Rust source.
The missing-Cargo fixture always runs with a case-local PATH containing no
Cargo or rustup shim and a cleared case-private foreign cache. Installed tools
or a warmed helper cannot turn its E0703 contract into a skipped observation.

Proven UI support modules and route fixtures use `// @fixture_module`.
Only testkit's collection reads this marker; it is neither language syntax nor
a runtime directive. Collection excludes these files while preserving their
names and imports for the real parent fixtures. An absent expected-output file
never causes exclusion: unmarked UI fixtures remain discovered and require an
owner-reviewed snapshot.
The runner stages the complete `tests/ui` and `tests/ui_lint` trees once per run
in external scratch, then checks each case in that shared source tree. Sibling
imports, convention route roots and authored fixture state therefore resolve
as in the original Rust runner, without per-case same-stem copy gaps. When outer
tracing is enabled, run-wide scratch also remains until the final trace audit;
ordinary runs remove it after reporting.
UI `@jetpack_e1250` copies its program as `main.jet` into a private scratch
package with `name: "e1250_fixture"`, `version: "1"` and `jet: 0.9`; it runs
public `jet build main.jet --offline`, requires a nonzero refusal, and compares
combined stdout/stderr. `@web_target` uses public
`jet build FILE --target=web`, not an internal compiler hook. Target availability
prerequisites do not substitute for a fixture's intended web semantic diagnostic.

Pending user-Rust interoperability acceptance rows use `PRODUCT INTEROP RED:`
in the existing case-disposition table. They remain in the active testkit corpus
and block registry closure; they are not Rust implementation retirements.
Remove their pending rows only after the native acceptance verdicts pass.
Retired Rust oracle/witness modes report nonzero `UNCOVERED` with
`cardRequired=true`; native witnesses remain mandatory. Rust performance peers
and historical fixture provenance are not Jet implementation dependencies.

## Why Jet once transpiled to Rust (recorded rationale)

The front end is hand-built either way; only the back end was a choice. Rust
gave a soundness verifier for the ownership checker, LLVM optimization,
cross-compilation and std for free, at the cost of compile times stacked on
rustc's and debuggers showing generated Rust (precedent: cfront, Nim,
TypeScript, Gleam). That trade is over: Jet's own back end builds every level
(D-EXEC1, D-BACKEND-OWN1) and Rust leaves the toolchain (owner directive 19).
