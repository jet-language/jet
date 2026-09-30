# Architecture

Vocabulary: [Jet vocabulary](vocabulary.md).

## Pipeline

```text
Jet source
  -> lexer -> tokens with byte spans
  -> parser -> AST
  -> sema -> checked bundle and semantic facts
  -> TIR -> canonical, validated and optimized MIR
               |-> MIRRust -> Rust -> rustc/LLVM -> native artifact
               |-> Cranelift -> resident native execution
               `-> MIR evaluator -> interpreter/comptime execution
```

The front end owns language checks and user-facing compiler diagnostics.
Backends consume checked facts; a rustc rejection of generated Rust is an
internal compiler error, not another language checker.

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

### Typed IR (TIR) — the codegen seam

The [TIR lowering](../../crates/jet-codegen/src/Codegen/TIR/mod.rs) accepts the
checked bundle and carries sema-approved types, access conventions and effects.
[MIR lowering](../../crates/jet-codegen/src/Codegen/TIR/mir.rs) gives those facts
canonical type, function, place and artifact identities. The resulting MIR is
validated and optimized before execution or emission. Neither lowering infers
new language permissions or falls back to an unchecked AST emitter.

The [driver's source compilation path](../../crates/jet-driver/src/Driver/mod.rs)
lowers the checked bundle once and passes its selected artifact to
[`MIRRust`](../../crates/jet-codegen/src/Codegen/MIRRust.rs).
The [execution seam](../../crates/jet-foundation/src/JitBackend.rs) instead
passes canonical MIR and an artifact identity to the resident backends.
These are consumers of one checked meaning, not separate front ends.

### One reflection model

`StructDef::reflection_fields()` is the declaration-owned stored-field-row source for
comptime `TypeInfo`, runtime `reflect.of`, TIR evaluation, and generated field
projection. Comptime reads facts from those rows; runtime wraps each projected
field in a typed `Value` and adds display text only as a view. AOT, resident JIT,
and the interpreter therefore preserve the same names, types, and nested value
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

The structure plane ends before TIR. AOT emit, Cranelift, the interpreter, and
web codegen receive the checked program after structure facts and gates have
been erased. This preserves I2 and I3 (rustc and engines do not decide policy),
I8 (one registry and one gate mechanism), and I9 (one meaning across applicable
tiers). The decision adds no user-typeable spelling and no lint plane or
external analyzer.

## Compiler crate map

D-COMPILERSEAMS1/2 define workspace seam crates. The root `jet` crate is a
facade and binary host over these internal APIs.

| Crate | Job | May emit diagnostics? |
|-------|-----|-----------------------|
| `jet-foundation` | shared leaf types and policies: `Syntax`, `Diagnostics`, `AST`, `Span`, `Generics`, `JitBackend`, stable exit codes, std-only JSON | renders diagnostics |
| `jet-lexer` | text to tokens | yes (E00xx) |
| `jet-parser` | tokens to AST, formatter | yes (E00xx) |
| `jet-comptime` | comptime values and interpreter support | no user-facing surface by itself |
| `jet-sema` | all semantic checks, collects all front-end diagnostics | yes (E01xx+) |
| `jet-codegen` | checked bundle to TIR and canonical MIR; Rust emission and MIR evaluation | **never** |
| `jet-pkg-model` | **L1**, shared read-only package/config data model: `package.jet` manifest parsing and the optional leading inline `package { … }` carrier, lock, hangar store listing, ref classification, FFI bridge construction, inline script deps, §6 structural `Merge`, the `BuildRecipe` data shape, plus pure effect-budget/lint-policy computation over that data (no network/provider/shell) | package/FFI diagnostics |
| `jet-env-model` | **L2**, shared pure environment plan model: `ModuleEval` and its typed plan outputs (`EnvPlan`/`SystemPlan`/`ImagePlan`/`FleetPlan`/…). Depends on `jet-pkg-model` (L1) + `jet-codegen`; no provider/store/network/shell | plan-evaluation diagnostics |
| `jetpack` | **L3**, package manager engine: provider/network/shell realization, JetOS, CLI — depends on `jet-pkg-model` (L1) for read-only data and `jet-env-model` (L2) for the plan model it realizes; native Nix cache admission uses the inward `jet-net` streaming transport | package/JetOS diagnostics |
| `jetos` | `jetos` binary front door for OS workflows; dispatches into `jetpack`'s `os` verb, whose JetOS realization lives in `jetpack::JetOS` | package/JetOS diagnostics (via `jetpack`) |
| `jet-driver` | front-end orchestration and compile outputs; depends on `jet-pkg-model` (never `jetpack`'s engine) for manifest/lock/FFI preparation; owns the shared pure dev/debug interpreter-boundary classifier, fix application, and compatible budget-report projection | front-end and interpreter-boundary diagnostics |
| `jet-queries` | std-only demand cache for incremental inputs and derived query values | no |
| `jet-semindex` | stable semantic index over checked programs for tooling | no new diagnostics |
| `jet-impact` | blast-radius reports over `jet-semindex` | no |
| `jet-repl` | complete interactive shell product over `jet-driver`, `jet-semindex`, and leaf policy | no new diagnostics |
| `jet-debug` | complete source debugger and DAP product over `jet-driver` plus leaf JSON/exit policy | debugger diagnostics only |
| `jet-cli` | canonical command/flag registry, completions, man page, diagnostic reference, and hybrid help UI over leaf syntax policy plus `jet-repl` terminal/symbol support | renders existing diagnostics only |
| `jet-canvas` | Canvas browser HTML/JS projection assets over leaf JSON escaping | no |
| `jet-devserver` | watch/HTTP/static policy, Canvas routes and semantic/edit service, browser-client leases, terminal/browser status parity, live reload, and atomic last-good artifact swapping; the root retains only the R5 compile/rustc executor and process watch loop | renders existing diagnostics only |
| `jet-rt` | runtime helpers shared by generated code and JIT/dev paths | no |
| `jet-jit` | dev/JIT execution tier over codegen/TIR facts | internal fallback only |
| `jet-net` | runtime/comptime fetch helper with TLS diagnostics | yes, for fetch failures |

### Retained compiler host boundary

The compiler port changes the implementation language of policy, not the
ownership of language meaning. A Jet pass must replace its Rust policy at the
existing seam, with the same inputs, identities, ordering and diagnostics.
It must not introduce a third front end, a second query verdict, or a new
execution lens. Rust source emission, rustc/LLVM and Cranelift remain supported
consumers; self-hosting does not mean replacing them.

The table describes crossing responsibilities, not a list of completed ports.
Its source links own the executable contracts. The proof-home references are
Tower acceptance boundaries, not evidence supplied by this inventory.

| Crossing and executable contract | Policy that can live in Jet | Responsibility retained by the Rust host |
|---|---|---|
| [Foundation types and syntax](../../crates/jet-foundation/src/lib.rs) → [lexer](../../crates/jet-lexer/src/Lexer/Terminators.rs) | Token/terminator decisions over source facts; foundation and syntax proof home #808 | Source bytes, byte-span validity, raw token payloads and the existing Unicode primitive; no reinterpretation of Jet records as Rust memory |
| [Parser](../../crates/jet-parser/src/lib.rs) → canonical AST | Grammar and AST construction, #809 | Own and marshal token/AST storage at a private crossing; preserve original spans and diagnostic records rather than parse a second time for the host |
| [Sema](../../crates/jet-sema/src/lib.rs) → checked bundle/effect facts | Types, effects, ownership and semantic verdicts, #810 | Keep the checked bundle and its facts together; unchecked or mismatched facts must not enter lowering |
| [TIR/MIR lowering](../../crates/jet-codegen/src/Codegen/TIR/mod.rs) → [MIR consumers](../../crates/jet-foundation/src/JitBackend.rs) | Semantic lowering over already-checked facts, #811 | Canonical MIR validation, artifact selection and retained Rust/Cranelift execution adapters; backends cannot invent a source-level rule |
| [Comptime ambient bridge](../../crates/jet-comptime/src/Comptime/AmbientRuntime.rs) ↔ [compiler Core evaluator](../../Source/Compiler.rs) | Compiler queries and compile-time/build policy, #812 | Scoped evaluator installation, host capabilities and value/error transport; compiler authority is not a process-global callback |
| [Loader and Driver](../../crates/jet-driver/src/Driver/mod.rs) ↔ [root entry](../../Source/lib.rs) | Front-end orchestration, #813 | Filesystem/process access, worker lifetime and stack, toolchain invocation and panic transport; no Cargo dependency from an inward seam back to the facade |
| [Query service](../../crates/jet-driver/src/QueryService.rs) ↔ [query cache](../../crates/jet-queries/src/lib.rs) | Tooling projections of the checked result, #813 | Revision/dependency invalidation and cache storage; cached values retain the checked bundle and effect-fact association, not an independent analysis |
| [Diagnostics](../../crates/jet-foundation/src/Diagnostics.rs) ↔ [CLI](../../crates/jet-cli/src/lib.rs) | Diagnostic construction and user-facing compiler/tooling policy, #813 | Render registered diagnostics and apply existing exit policy; an internal backend failure must not become a new user-language diagnostic |

Compiler seams depend inward through local path dependencies, as in the
[driver manifest](../../crates/jet-driver/Cargo.toml); the
[foundation](../../crates/jet-foundation/Cargo.toml) and
[query cache](../../crates/jet-queries/Cargo.toml) are leaves. A private Jet pass
does not justify a new external compiler dependency or a cycle back to the
root. The [runtime-side JIT dependencies](../../crates/jet-jit/Cargo.toml) have
their own approved backend/bridge contracts; they are not permission to add
dependencies to compiler policy.

#### Scoped entry and failure containment

[`Source/lib.rs::run_compiler_work`](../../Source/lib.rs) installs the root's
`Compiler::eval_core_call_with_type` callback around work. The inward
[`jet_driver::run_compiler_work`](../../crates/jet-driver/src/lib.rs) carries
the ambient callbacks and scoped terminator driver onto the
[`CompilerStack`](../../crates/jet-foundation/src/CompilerStack.rs) worker and
installs the MIR evaluator. Nested entries reuse that worker. The root callback
provides the read-only `core.compiler` queries and checked `core.build` queries;
the lower worker primitive alone does not install this root-owned authority.
`Comptime::with_ambient` restores the previous callbacks on return and unwind.
Nothing in this crossing grants runtime code a compile-time compiler API.

This is a scoped Rust call boundary, not a C ABI or a promise about Rust
`Vec`, enum or pointer layout. The worker transports a Rust panic payload
unchanged back to its Rust caller; it does not relabel an ICE as a successful
pass. Machine-code host crossings are different:
[`jet-jit::host_seam`](../../crates/jet-jit/src/host_seam.rs) catches inside
each generated C shim and reports through the existing status channel before
returning to Cranelift code. No unwind may cross a JIT/C frame. A new private
compiler adapter must preserve these distinct containment rules.

The host, pass and private value contract are source-coupled in one build, with
no independently versioned public ABI or compatibility negotiation; changing
the crossing migrates both sides and its behavioral witness in one cutover.

The behavioral boundary witness is
[`selfhost_host_boundary`](../../tests/selfhost_host_boundary.rs): real
ambient parser queries return parsed items and malformed-source diagnostics,
nested calls reuse the worker, and unwinding preserves the payload without
leaking compiler authority. Bypassing the root wrapper while retaining only
the lower Driver worker removes the evaluator and makes that witness fail.
This tests the host boundary, not a Jet implementation of a compiler pass.

#### Terminator-pass crossing

The staged replacement in
[`Lexer/Terminators.jet`](../../Compiler/JetLexer/Source/Lexer/Terminators.jet)
owns `insert_terminators` policy and all its decision helpers (Tower #3581).
The [Rust adapter](../../crates/jet-lexer/src/Lexer/Terminators.rs) is reached
by `lex`, `lex_config` and `lex_generated` after raw scanning.
Its private input is source bytes plus
exhaustive indexed raw-token facts, including the existing Unicode-uppercase
primitive. Its output is ordered `InsertSemi`/`SplitHeader` events. Jet owns
the insertion and split-header decisions; the host validates bounds/order,
moves the original payload-bearing tokens and formats the existing diagnostic.
The raw scanner and parser are outside that pass. Keeping payload ownership
on the host avoids encoding a second token model or copying compiler objects
across a supposed stable ABI.

The private bootstrap adapter in
[`selfhost_terminators`](../../tests/selfhost_terminators.rs) produces checked,
optimized MIR through the Rust reference before installing the candidate.
Inside that explicit scope, the actual Loader/parser/compiler path invokes
`evaluate_mir_function_with_config`; the Driver and Loader carry the callback
across their worker-thread boundaries and restore the previous driver on
return or unwind. Evaluator, callback or event-ABI failure is an internal
failure, never permission to retry through Rust policy.

Outside the scope, the default compiler still uses the retained Rust reference.
This is a Jet-authored terminator pass interpreted by the retained host, not a
complete Jet lexer, a default CLI cutover, or a self-hosting/performance claim.
The bounded manifest, own-source comparison, six mutation witnesses and
same-pass AOT/default/interpreter/awaited-web golden live with that adapter;
their executable results, not this explanation, establish parity.

The subsequent self-host proof homes remain separate obligations: #814 is the
pinned stage0-to-stage1 build, #815 is byte-identical stage1-to-stage2 output,
and #816 is the Jet-built compiler full-suite closeout. The policy-family
homes #808–#813 and the boundary inventory #218 cannot substitute for those
bootstrap and fixed-point proofs.


### Machine-wide artifact store

D-BUILD-STORE1=E defines `crates/jet-store` as the one machine-wide artifact
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
recorded before/after states. Atomic file publication and the package lock
protect live bytes from concurrent writers and path escapes.

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
to the existing owned clone/materialization TIR nodes, so AOT, Cranelift,
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

D-DEP-GC1=A has one dependency-free collector implementation in
`crates/jet-rt/src/__gc.rs`. `jet-rt` exposes that module directly to dev/JIT;
codegen embeds the exact same source inside private `jet_gc` for AOT. Generated
startup initializes trace output even when no allocation is promoted. The
collector has no source-facing wrapper, constructor, or module; only sema-proven
automatic promotions call its traced allocation entry.

Sema closes each escaping payload over the promoted bindings stored inside it.
Codegen translates those proven source relations to collector object IDs at
creation and updates the same graph on bare assignments and mutations. Cycles
therefore live in the collector-owned graph without changing Jet's bare value
syntax. Collector failures cross one E2110 runtime boundary; generated Rust
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

I6 is machine-checked by `tests/truthfulness.rs`: the root compiler and named
compiler seams may use only workspace path dependencies. Runtime/tool siblings
such as `jet-jit` and `jet-net` are separate workspace members with their own
owner-approved dependency posture; that does not permit an external dependency
to leak into a checked compiler manifest.


`tests/workspace_crates.rs` pins the dependency direction. Compiler front-end crates
may not grow back-edges into driver/codegen clients; tooling and runtime crates
stay outside the compiler seam unless their dependency row changes here and in
the test. The package, environment-plan, and JetOS layers are described in the
crate map above. The root binary owns native build execution:
`Source/CmdCompile.rs` invokes rustc, classifies linker/tool failures, renders
the I2 ICE banner, and links prepared FFI artifacts.
D-ARCH-SOURCE1=A also puts command and interactive product ownership behind
real workspace seams. `crates/jet-cli` owns the command/flag registry,
completion/man generation, diagnostic reference, and hybrid help UI;
`crates/jet-repl` owns the REPL and terminal implementation;
`crates/jet-debug` owns the source debugger, native adapter, line map, and DAP
server. The root host wires command execution and re-exports `jet::CLI`,
`jet::Help`, `jet::Explain`, `jet::REPL`, and `jet::Debug`. These products
depend inward on compiler seams. Their shared
interpreter eligibility walk lives in `jet-driver`; stable exit codes and the
std-only JSON codec live in dependency-free `jet-foundation`. Neither product
depends on the root package, splices root source with `include!`, or owns rustc
invocation and ICE classification; R5 remains in `Source/CmdCompile.rs`.
`crates/jet-devserver` likewise owns the web server, Canvas routes, status
surfaces, client leases, live reload, and last-good swap. `CmdCompile.rs`
drives its build-state API while retaining compile/codegen/rustc execution;
there is no callback or dependency edge from the seam back to the root.

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

The AOT emitter and default development tier use the same `Browser.rs`,
strict-JSON behavior, and extracted RFC6455 client source. Browser session
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

`crates/jet-pkg-model/src/FFI.rs` (or `CFFI.rs` for C) prepares a generated,
content-addressed bridge. Its generated `Cargo.toml` owns foreign dependencies,
and `FfiLink` records the crate, rlib, selected-target runtime directory, and
host proc-macro directory. Inline `#FFI(c|cpp|asm)` bodies use the same bridge
and include their checked signature, target, raw body, and schema in the cache
identity. C/C++ wrappers and asm lower only after sema proves their boundary
contracts.

`jet inspect bind cpp` is owned by `CppBind.rs`: clang AST discovery produces a
deterministic Jet module and C-linkage shim/archive under
`.jet/bindings/cpp/`, with provenance for the declaration, toolchain, target,
generated sources, and schema. `FfiLink` is threaded through
`crates/jet-driver/src/Driver/mod.rs` and `CompileOutput`; the native link edge
is `Source/CmdCompile.rs::build`.

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

The native Library surface accepts one homogeneous scalar shape per export:
every parameter and the return are `Int`, `Float`, `Bool`, or `Text`.
Function-valued exports and a Library-wide `init`/`shutdown` protocol are not
part of this surface. The loader owns mapping and unmapping. The host owns
process signals, thread creation, and isolation for a failing call.

| Surface | Guarantee | Invalid use | Observable failure |
| --- | --- | --- | --- |
| Initialization | A successful `dlopen`/`dlsym` (or the platform equivalent) is the admission point. The first export call needs no `jet_init`; the artifact has no process-global init hook. | Call a missing symbol or invent a lifecycle symbol. | The loader returns its normal missing-symbol error; the generated Library does not run. |
| Shutdown | There is no `jet_shutdown`. The host waits for all calls and frees returned values before unmapping. | Unmap while a call or worker is active. | No Jet result is defined; the host violated the loader contract. |
| Thread entry | Any host thread may call a live scalar export. Calls may run concurrently; the host joins workers before unmapping. | Pass a non-C-safe value or let a worker outlive `dlclose`. | Sema rejects the former; the latter is invalid C/loader use. |
| TLS | Runtime stack and foreign-failure markers are thread-local; generated call guards restore their depth and markers on normal return. A call does not borrow another thread's state. | Share a call-local capability or use a stale thread-bound value after return. | The declaration is rejected or the host has left the defined contract. |
| Re-entry | A host function named by `#Import(c)` may call a second live export while the first export is active. The nested call uses the same scalar boundary. | Expect a function-valued `#Export(c)` callback; it is outside the homogeneous Library surface. | Library validation reports `E1341`; no callback ABI is emitted. |
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
end, not a second checker. `crates/jet-queries` is a std-only demand cache
for file inputs and derived query values. The bounded loader prepares open or
discovered sources with at most eight workers and consumes them in stable
order. The Rust checker's in-process item cache (`IncrementalSemaCache`)
serves editor sessions. The server records cancellation concurrently with
request execution and replaces a cancelled in-flight result with JSON-RPC
`-32800`. D-LSP2 requires every advertised LSP feature to have named coverage
in `tests/lsp.rs`; the server must not advertise speculative features.

### Compiler-speed evidence boundary (#666)

Compiler-speed evidence stays on the production compiler path. The differential
gate sends the same checked example to optimized AOT and the default tiered
lens, then compares exit status, stdout, and stderr. A record identifies
resident Cranelift execution or interpreter deopt; a refusal, newly broken AOT
oracle, missing record, or divergent result remains visible as a failure.
Named-job parity uses the same checked program-argument slice for `jet run`,
`jet dev`, and interpreter execution; an unknown job remains an E1294 failure.

The timing path uses `PhaseTiming` and the typed `CompilerProbe` provider. A
checked corpus pins source and expected-output digests. A measurement records
compiler/Core identities, target, profile, backend, linker, host, cache state,
fixed warmups and samples, process CPU-time variance, peak RSS, and phase totals. The
`dev` profile invokes the production Cranelift JIT lens; optimized `release`
invokes the production rustc AOT lens. Clean, no-change, and representative-
edit runs are distinct. Partial timing, changed inputs, incompatible
identities, nondeterminism, pathological inputs, and unstable samples are
unavailable/failure; they cannot be made green by changing the workload or
hiding cold-cache work. The checked-corpus report is schema version 4 and emits
six rows for each active corpus row: `jit-clean`, `jit-no-change`,
`jit-representative-edit`, `aot-release-clean`, `aot-release-no-change`, and
`aot-release-representative-edit`. The current corpus has five active rows, so
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

An application `target: sandbox` is a WASM Component Model guest in the
existing `jet-pkg-model::Prelude::Plugin` host. The native library boundary is
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
defect from typed boundary state, not by parsing backend error text. The
wire keeps the original Jet operation and exported call name and remains the
transport envelope; no public `Plugin` error type is added.

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

The boundary owner is `crates/jet-pkg-model::CompilerExtension`, which owns the
versioned snapshot, response validation, and lifecycle. Its
`Prelude/CompilerExtension.rs` substrate compiles only into the `jetpack`
binary, using the same wasmtime Component Model pin as application
`core.plugin` (`WASMTIME_CRATE_SPEC` / D-DEP-WASM1). Ordinary `jet` processes
never link or initialize Wasmtime.

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
- **E2E harness (C4 technical):** `crates/jetpack-bin/tests/compiler_extension_e2e.rs`
  loads real `compiler-extension-v1` component fixtures under
  `crates/jet-pkg-model/fixtures/compiler_extension/` through the same
  `Prelude/CompilerExtension.rs` host. Proves one custom-lint finding
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

- **R1 — Codegen is dumb.** No checks, no decisions, no "see if rustc
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
- **R2 — Sema is the gatekeeper.** Any program that passes sema must
  produce Rust that compiles. New language features land as: spec →
  parser → sema checks → codegen → tests, in that order.
- **R3 — Single surface.** User-typeable strings live in
  `crates/jet-foundation/src/Syntax.rs` only. Renaming a keyword starts there;
  parser/formatter tests, generated grammars, snapshots, and docs must move with
  it.
- **R4 — Spans everywhere.** Any AST node an error might point at carries
  its span. Adding a node without a span is a review-blocker.
- **R5 — ICE policy.** rustc failing on generated code prints the
  internal-compiler-error banner owned by `Source/CmdCompile.rs`, exits 101,
  and is treated as a P0 bug. rustc's stderr is shown only inside that banner.
  Missing rustc, linker, or C library is a tool/user diagnostic, not an ICE.
- **R6 — Name mangling.** User identifiers are emitted as `__jet_<name>`
  (`main` excepted) so user code can never collide with Rust keywords,
  macros, or std items. The sema-owned `NameLedger` is the single source for
  declaration paths, aliases, visibility, and reference origins; all Rust-name
  projections use its canonical mangle functions. This implements ratified
  D-NAME-TREE1 without adding a user-facing spelling.
- **R7 — Backend is swappable.** Rust emission stays in
  `crates/jet-codegen/src/`; native rustc invocation and ICE classification
  stay in `Source/CmdCompile.rs`. The lexer, parser, and sema crates do not
  depend on either responsibility. Another backend replaces the codegen and
  binary-build edges without changing the front end.
- **R8 — Small, self-contained binaries.** The root binary build path in
  `Source/CmdCompile.rs` calls `rustc` directly with explicit profile flags,
  `strip=symbols`, and thin LTO for optimized AOT. Its shared native-linker
  selector honors explicit `RUSTC_LINKER`/`CC`, then chooses mold or lld through
  the C driver before falling back to the system linker. The selected
  driver/backend identity is part of native cache and timing evidence.
  It links content-addressed `jet_runtime` and reachable `jet_runtime_core`
  rlibs, so rustc does not compile the fixed embedded runtime or an unused Core
  closure for each program. The object keys include the exact emitted/exported
  source, the runtime dependency key, rustc identity, target and profile flags,
  and explicit profile environment. The final native key carries the same
  relevant runtime/Core digests; it does not read or hash the compiler binary.
  A verified warm object is reused; a malformed, corrupt, or rejected object
  falls back to the complete inline program, and the shared cache remains
  bounded.
  The linker keeps only what the program uses ("only link what's needed"). The
  output is one self-contained native binary. Rust's std links a baseline
  (low-hundreds-of-KB), accepted as the cost of a beginner-friendly std-backed
  runtime. A size-minimal profile (`opt-level="z"`, possibly `panic=abort`) is
  decision S15 and is available as `jet build --small`; the default leans toward
  speed.
- **R9 — A file is a complete program.** `jet run foo.jet` compiles and
  runs a single file with no manifest, no project folder, and no config.
  Inside a package, a bare `jet run` resolves the same complete program from
  `run.jet`, `src/run.jet`, or `<package>.jet`; `jet dev`, `jet check`,
  `jet build`, and `jet test` share that resolver, while an explicit source
  path stays explicit.
  The generated `.rs` file remains a complete standalone program for audits.
  Native builds can split its marked fixed-runtime block into Jet's hidden
  cached `rlib`, then compile and link the generated user program. This process
  does not create or require a Cargo project for user code. Users may supply
  package structure when their package contract requires it; the compiler does
  not impose one.
- **R10 — Std is pay-for-what-you-call.** Core standard-library modules are
  compiler-known namespaces, but importing them is free. Sema records the
  core helpers that a checked program can call, and codegen emits only those
  helper templates. A program that imports every core module but calls none
  should stay in hello-world size territory.

  Embedded Core runtime templates under `crates/jet-codegen/src/Prelude/` (and,
  for parts a comptime-reachable seam crate must also call, `crates/jet-foundation`
  prelude modules) are the canonical source for compiler-known Core behavior;
  rebuild `jet` before smoke-testing any change because `include_str!` snapshots
  them into the binary. A source-owned Core module such as `core.archive` is
  compiled from its canonical `.jet` module through the ordinary frontend; it
  does not keep a copied fallback template or a hidden Rust semantic bridge.
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
  facts (places, types, patterns, method identities) — never pre-rendered Rust
  source text. **Core/runtime meaning lives only in the embedded Prelude parts /
  CoreLib** (`crates/jet-foundation` prelude modules and
  `crates/jet-codegen/src/Prelude/**`; a part sits in `jet-foundation` when a
  comptime-reachable seam crate must call it, per I6). Rust spelling lives
  only in the AOT emit
  layer as calls into that Prelude. Cranelift JIT hosts and interpreter ambient
  bindings are the same kind of layer: marshal args, call the identical
  `jet_*` / Prelude function AOT would call, marshal results. They must not
  fork defaults, CORS/policy checks, error meaning, or other Core behavior.
  Every engine (Rust emitter, Cranelift JIT, the canonical interpreter, and web
  when the surface applies) must consume TIR exhaustively with real lowering that
  preserves one meaning (AGENTS.md invariant I9 / `claim.tier-parity`). Every
  supported construct needs that lowering; an unsupported fall-through is a
  compiler defect, not a closed feature or an AOT-only exception. The
  interpreter is the reference semantics for `jet run`/`jet dev` parity, and
  parity evidence covers stdout, stderr, exit code, diagnostics, panics, and
  side effects across applicable tiers. If deopt runs that surface, interpreter
  ambient must call the same Prelude symbol as AOT emit. Native JIT is a
  performance tier; semantic parity across AOT, JIT, interpreter, and web is
  mandatory. (D-ONECORE1=A, ratified 2026-07-24; I9 owner-directed
  2026-07-29; dumb-adapter rule owner-directed 2026-07-29.)

  The comptime Core registry uses the following namespace classification. The
  table covers the `core_calls.rs` namespaces.
  `Kernel` means that comptime marshals `CtValue` into the exact Prelude part.
  `Intrinsic` means that the evaluator implements a language value operation,
  not a second Core policy. `Host` means that the call crosses an effect or
  authority boundary. Every listed semantic rule has one Prelude/Core home;
  engines only marshal values to it.

  | Namespace | Class | Semantic home |
  |---|---|---|
  | `core.archive` | Host | Package bridge; comptime rejects unsupported host work. |
  | `core.args` | Kernel | `Prelude/CoreLib/Top/Args.rs`; `ArgsLite` marshals values. |
  | `core.auth` | Kernel | `AuthSession.rs` and `CryptoEntropy.rs`; `AuthLite` marshals values. |
  | `core.archive.gzip` | Host | Native compression boundary; comptime rejects unavailable calls. |
  | `core.archive.zstd` | Host | Native compression boundary; comptime rejects unavailable calls. |
  | `core.compute` | Kernel | `Prelude/CoreLib/Top/Compute.rs`; `ComputeLite` marshals values. |
  | `core.crypto` | Kernel | `Prelude/CoreLib/Top/CryptoEntropy.rs` and the shared crypto Prelude own typed crypto rules; every engine marshals values. |
  | `core.crypto.expert` | Kernel | The audited raw-byte route shares the crypto Prelude symbols; the `#Unsafe` gate is policy, not a second engine implementation. |
  | `core.data` | Kernel | `DataStats.rs` and `DataPlot.rs`; comptime marshals values. |
  | `core.data line renderers need groups` | Kernel | `DataPlot.rs` owns line-renderer argument validation. |
  | `core.data line renderers need options` | Kernel | `DataPlot.rs` owns line-renderer option validation. |
  | `core.data: argument must be `[DataGroup]`` | Kernel | `DataPlot.rs` owns `DataGroup` validation. |
  | `core.data: argument must be `[Float]`` | Kernel | `DataStats.rs` owns numeric input validation. |
  | `core.email` | Kernel | `Prelude/CoreLib/Email.rs`; `EmailAdapter` marshals values. |
  | `core.encoding` | Kernel | Encoding dispatch and value validation use the shared encoding Prelude/Core kernels. |
  | `core.encoding.jsonl.to_string: expected a list` | Kernel | JSONL value validation stays with the encoding kernel. |
  | `core.encoding.base32` | Kernel | `Prelude/Core/EncodingBase.rs` owns encode; `BaseEncodingDispatch.rs` owns edition-aware decode; comptime/JIT marshal values. |
  | `core.encoding.base64` | Kernel | `Prelude/Core/EncodingBase.rs` owns encode; `BaseEncodingDispatch.rs` owns edition-aware decode; comptime/JIT marshal values. |
  | `core.encoding.cbor` | Kernel | Typed CBOR marshals through the shared DataTree/codec kernel. |
  | `core.encoding.csv` | Kernel | Typed CSV marshals through the shared codec kernel. |
  | `core.encoding.hex` | Kernel | `Prelude/Core/EncodingBase.rs` owns encode/decode; comptime/JIT marshal values. |
  | `core.encoding.json` | Kernel | Typed JSON uses the shared DataTree/codec kernel. |
  | `core.encoding.jsonl` | Kernel | JSONL uses the shared typed JSON kernel. |
  | `core.encoding.toml` | Kernel | Typed TOML uses the shared codec kernel. |
  | `core.encoding.xml` | Kernel | `jet-foundation/XmlKernel.rs`; AOT embeds it and comptime/JIT marshal `DataTree` values. |
  | `core.encoding.yaml` | Kernel | Typed YAML uses the shared codec kernel. |
  | `core.sys` | Host | Explicit comptime environment authority and policy boundary. |
  | `core.event` | Host | Callback registration crosses the runtime closure boundary; the host only marshals shared event values. |
  | `core.files` | Host | Explicit comptime filesystem authority and policy boundary. |
  | `core.text.fmt` | Kernel | `Prelude/Core/Fmt.rs` owns number, decimal, grouping, byte, duration, ordinal, plural, and padding rules; adapters marshal values. |
  | `core.term` | Host | I/O authority boundary; pure progress values use `Core/Progress.rs`. |
  | `core.math.abs: non-numeric argument` | Mixed | Numeric argument validation stays with the shared math rules. |
  | `core.math` | Mixed | `MathLibPure.rs` owns residual integer, gcd/lcm, factorial, and erf-family rules; true numeric intrinsics remain intrinsic. |
  | `core.net.mime` | Kernel | `Prelude/CoreLib/JetStd/Mime.rs`; comptime marshals values. |
  | `core.net` | Mixed | `Prelude/Core/NetPure.rs` owns IP/socket parse and field rules; sockets and DNS stay authority-bound. |
  | `Path` / `core.files` | Kernel + Host | `Prelude/Core/Path.rs` owns lexical join/parent/extension/stem/normalize rules; filesystem work stays host-bound and accepts `String | Path`. |
  | `core.perf` | Host | Runtime performance authority; comptime rejects unavailable calls. |
  | `core.process` | Host | Process authority boundary and REPL host adapter. |
  | `core.math.random` | Mixed | `Prelude/Core/SeededRandom.rs` owns deterministic seeded RNG rules; ambient RNG stays host-bound. |
  | `core.game.raylib` | Intrinsic | `CtValue` constructor only; runtime graphics work stays host-bound. |
  | `core.reactive.loadable` | Kernel | `Prelude/Core/Loadable.rs` owns tag/presence rules; typed payloads and handles remain adapters. |
  | `core.reflect` | Intrinsic | Compiler-owned type metadata construction. |
  | `core.regex` | Kernel | The generated regex engine exposes one shared matching and validation kernel. |
  | `core.units` | Kernel | `Prelude/Core/Measurement.rs`; all tiers marshal `(value, uncertainty)`. |
  | `core.service` | Kernel | Typed tree declarations and the shipped mailbox, lifecycle, authenticated directory/routing, handoff, rollback, partition, reconciliation, and typed observability paths use `ServiceAuthority.rs` and `Services.rs`; structured export uses the existing `core.log` sink, and `ServicesLite` marshals the service calls. |
  | `core.data.sketch.cms` | Kernel | `Prelude/Core/Sketch.rs`; comptime and JIT marshal state. |
  | `core.data.sketch.hll` | Kernel | `Prelude/Core/Sketch.rs`; comptime and JIT marshal state. |
  | `core.data.sketch.reservoir` | Kernel | `Prelude/Core/Sketch.rs`; comptime and JIT marshal state. |
  | `core.data.sketch.tdigest` | Kernel | `Prelude/Core/Sketch.rs`; comptime and JIT marshal state. |
  | `core.compute.solve` | Kernel | `Prelude/CoreLib/Top/Solver.rs`; comptime and JIT marshal state. |
  | `core.sync` | Kernel | `Prelude/CoreLib/Top/Sync.rs`; `SyncLite` marshals values. |
  | `core.testing` | Mixed | Shared deterministic helpers call Prelude; harness and host work stay adapters. |
  | `core.text` | Kernel | `Prelude/CoreLib/Top/Text.rs` plus Unicode tables owns text/Unicode rules, including scalar names; `TextLite` marshals values. |
  | `core.text.ends_any: non-list argument` | Kernel | Text argument validation stays with the text kernel. |
  | `core.text.starts_any: non-list argument` | Kernel | Text argument validation stays with the text kernel. |
  | `core.time` | Mixed | `Prelude/Core/Time.rs` owns civil, duration, and zone rules; clock reads remain host effects and every engine marshals the same Prelude symbols. |
  | `core.net.tls` | Host | Native TLS boundary; comptime rejects unavailable calls. |
  | `core.ui` | Intrinsic | `CtValue` constructors and field projection only. |
  | `core.ui.box() needs UiNode children` | Intrinsic | UI node-shape validation stays with the intrinsic constructor path. |
  | `core.ui.box() needs [UiNode]` | Intrinsic | UI list validation stays with the intrinsic constructor path. |
  | `core.ui.node_role(): missing role` | Intrinsic | UI role validation stays with the intrinsic constructor path. |
  | `core.net.url.data: first argument must be a Mime` | Kernel | URL data validation stays with the shared URL/MIME kernel. |
  | `core.net.url.data: mime.sub` | Kernel | URL MIME field validation stays with the shared kernel. |
  | `core.net.url.data: mime.top` | Kernel | URL MIME field validation stays with the shared kernel. |
  | `core.net.url` | Kernel | `Prelude/CoreLib/JetStd/UrlMime.rs` owns URL parse/render/percent rules; `UrlLite` marshals `JetURL`. |

- **R13 — An abort is never an outcome; no unwind may reach a JIT frame.**
  (D-JITUNWIND1, #1997.) `cranelift-jit` registers no unwind
  information for the code it emits, so a JIT frame carries no FDE. A Rust
  panic raised above one makes libgcc's phase-1 walk run off the top of the
  stack and the process rtaborts with `fatal runtime error: failed to initiate
  panic, error 5` before any outer `catch_unwind` can see it. The user gets a
  bare `SIGABRT` with no text, which is not one of the exit codes below.

  Two mechanisms could fix that, and the choice is recorded here rather than
  left implicit at each call site.

  1. **Register unwind info for JIT'd code** — `create_unwind_info(isa)` plus
     per-function FDE registration. **Rejected.** It makes unwinding a
     *supported* path through generated machine code: a far larger semantic
     commitment than it looks, platform-specific frame registration, and it
     writes Rust panic propagation into the contract of Jet-generated code.
  2. **Convert at the boundary** — every host seam turns a panic into a status
     before returning into JIT'd code, so no unwind ever begins with JIT frames
     below it. **Chosen.** It is how the tiers already talk: `#Shield` delivers
     a deferred cancel as a status
     (`jet_scheduler_shield_leave_status`), and the deopt seam already carried
     exactly this conversion.

  **The recorded cost of (2) is that the guarantee has to hold across every one
  of the ~1.7k JIT host symbols, so it needs a mechanical check and not review
  discipline.** That cost is the centre of the design. It is paid structurally,
  three ways:

  - A host seam is an ordinary Rust `fn`, never an `extern "C" fn`. rustc gives
    an `extern "C"` **body** an abort-on-unwind shim, so a panic inside one dies
    as `thread caused non-unwinding panic` at that body's own edge, before any
    wrapper above it could catch. A boundary can only be added by *replacing*
    the C frame, never by wrapping one.
  - The boundary is **generated**, from the one canonical per-symbol
    declaration `host_fns!` already owns (card #1633).
    `crates/jet-jit/src/host_seam.rs` builds an `extern "C"` shim with the
    seam's exact C signature whose body runs the seam inside `guard_seam`, and
    `builder.symbol` registers that shim. A new host symbol cannot be declared
    without one.
  - `tests/jit_no_unwind_boundary.rs` is the check, and it is a rule about
    frames rather than about names: **no `extern` fn may be defined in
    `crates/jet-jit` at all**, whatever its ABI or its name, except the shim
    `host_seam.rs` generates. No host address may escape except through
    `guarded_addr`, and the macro must still emit the guard. The first form of
    this check banned `extern "C" fn jet_*` only, and a bridge callback named
    `jit_ffi_reporter` — a C frame a foreign library enters when a foreign
    function has already failed, with a Cranelift frame below it — sat outside
    that name for exactly as long as the name was the rule.
  - The rule is also **unfilable**. Every ratcheted section of
    `tests/jit_corpus_gate.txt` is shrink-only, so a stem recorded there is a
    stem this suite has agreed not to fail on again — and an abort recorded
    there would be an abort under permanent protection. So the gate refuses to
    classify one: `corpus_gate_refuse_abort` reads the AOT oracle, the default
    `jet run` and the forced interpreter, and any abort marker in stderr fails
    the stem outright in every section. `streams/generators` is why. It raised a
    second time from drop glue, died as `panic in a destructor during cleanup`,
    and the classifier — which only ever looked at the exit code — filed it as
    the benign row `AOT exit 1`. The marker list lives once, in `tests/common`,
    so the corpus gate and `tests/jit_no_unwind_boundary.rs` cannot drift.
  - The one carve-out is a **process signal handler**, which the kernel enters
    on a borrowed stack that may have a JIT frame under it. Conversion is the
    wrong tool there: `guard_seam` reaches
    `jet_scheduler_install_panic_hook`, which takes the process panic-hook lock
    and allocates on first call, so catching inside a handler would trade an
    unreachable panic for a reachable deadlock. A handler must therefore be
    panic-free — its whole body notes one relaxed atomic — and the check pins
    the permitted statements instead of trusting the name.

  Conversion targets the tier's **existing** status channel — a cancel or a
  blown deadline lands on the pending-interrupt channel `#Shield` already uses,
  a program stop renders through the shared report boundary and exits `70`, and
  an engine defect takes the branded ICE rail and exits `101` (I2). No second
  error channel is introduced (I8), and `Prelude/Scheduler.rs` stays the only
  place a caught payload is read as a status (I9).


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
| `101` | Jet's own compiler defect (I2/R5): rustc rejected generated code or the compiler reached an impossible state. Never a user-program exit. | compiler |

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

1. **diagnostic snapshots** (tests/diagnostic_snapshots.rs): every
   diagnostic's exact text, pinned. The error messages are the product;
   treat snapshot diffs like UI diffs.
2. **golden examples** (tests/golden.rs): Examples/ must front-end-pass,
   contain no `unsafe`, and — when rustc is present — build and print
   exactly Examples/features/expected/*.out.
3. rustc-as-verifier: golden tests assert rustc accepts generated code,
   so a sema soundness hole becomes a loud test failure, not a shipped bug.

## Why transpile to Rust (recorded rationale)

The front end is hand-built either way; only the backend was a choice.
Rust gives: a soundness verifier for our ownership checker (critical when
agents write the compiler), LLVM optimization, cross-compilation, and std
— for free. Known costs, accepted: compile times stack on rustc's;
debuggers show generated Rust until M6+ tooling. Precedent: cfront, Nim,
TypeScript, Gleam.
