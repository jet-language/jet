# Architecture

## Pipeline

```
 Jet source (.jet)
        │
        ▼
 jet-lexer ──► tokens (every token has a byte Span)
        │
        ▼
jet-parser ──► AST                      ┐
        │                               │  the FRONT END owns all
        ▼                               │  semantics and every
  jet-sema ──► checked AST              │  user-facing diagnostic
        │      (M2: + ownership check)  ┘
        ▼
jet-codegen ──► boring Rust source
        │
        ▼
     rustc  ──► native binary      (verifier + optimizer; never
                                    speaks to users — see R5)
```

### Typed IR (TIR) — the codegen seam

Codegen does not read the AST plus side registries; it lowers the checked AST to a
**typed IR** (`crates/jet-codegen/src/Codegen/TIR/`) that carries only sema-approved facts, then emits
Rust from the TIR with **zero inference** (every type/convention/mangle/overflow decision
is resolved at lowering — R1/I3). The TIR is the **only** codegen seam (R7) for every emitted
body: free functions, methods, trait methods, `#Test` block bodies, and error-conversion
`impl Old -> New` bodies all lower through it. A per-surface gate (`tir_covers*`) decides
coverage, and a construct **outside** the TIR subset is an **internal compiler error** (R5 ICE),
never an AST fallback or a miscompile. The legacy AST codegen path (`emit_expr`/`emit_stmt`/
`emit_stmts`/`emit_lambda`) was deleted (c109) once a whole-test-suite byte-parity check proved
every reachable body routes through the TIR; no legacy emit machinery remains. Constructs the
gate excludes are provably sema-unreachable (generic-struct methods E0311, bare `?? return`
in a value fn E0405, nested `T??`, a bare `Variant(n) ->` arm) — they never reach codegen.
(Type *definitions* — `emit_struct`/`emit_enum`/`emit_trait_def` — are structural, not bodies,
and emit directly; only executable bodies go through the TIR.)

## Compiler crate map

D-COMPILERSEAMS1/2 split the compiler into workspace seam crates. The root
`jet` crate is now a thin facade and binary host over these internal APIs.

| Crate | Job | May emit diagnostics? |
|-------|-----|-----------------------|
| `jet-foundation` | shared leaf types and policies: `Syntax`, `Diagnostics`, `AST`, `Span`, `Generics`, `JitBackend`, stable exit codes, std-only JSON | renders diagnostics |
| `jet-lexer` | text to tokens | yes (E00xx) |
| `jet-parser` | tokens to AST, formatter | yes (E00xx) |
| `jet-comptime` | comptime values and interpreter support | no user-facing surface by itself |
| `jet-sema` | all semantic checks, collects all front-end diagnostics | yes (E01xx+) |
| `jet-codegen` | checked program to Rust text; TIR is internal here | **never** |
| `jet-pkg-model` | **L1**, shared read-only package/config data model: `pkg.jet` manifest parsing, lock, hangar store listing, ref classification, FFI bridge construction, inline script deps, §6 structural `Merge`, the `BuildRecipe` data shape, plus the pure effect-budget/lint-policy computation over that data (no network/provider/shell) | package/FFI diagnostics |
| `jet-env-model` | **L2**, the shared pure plan model (card #367 slice 4): `ModuleEval` (the computed-modules evaluator) and its typed plan outputs (`EnvPlan`/`SystemPlan`/`ImagePlan`/`FleetPlan`/…). Depends on `jet-pkg-model` (L1) + `jet-codegen`; no provider/store/network/shell | plan-evaluation diagnostics |
| `jetpack` | **L3**, package manager engine: provider/network/shell realization, JetOS, CLI — depends on `jet-pkg-model` (L1) for read-only data and `jet-env-model` (L2) for the plan model it realizes | package/JetOS diagnostics |
| `jetos` | `jetos` binary front door for OS workflows; still dispatches into `jetpack`'s `os` verb (JetOS realization hasn't physically relocated out of `jetpack::JetOS` — that's a distinct, still-open scope gate, not part of slice 4) | package/JetOS diagnostics (via `jetpack`) |
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

### Safe Jet data-race guarantee

A data race is two tasks that access the same memory at the same time when at
least one access writes and the accesses do not use a synchronization rule.
**Safe Jet programs cannot contain a data race.** An attempt to create one does
not compile.

The guarantee covers all Jet-owned concurrency paths:

- A task created by `tasks.spawn` or `g.task` owns or copies its captures.
  Sema rejects a mutable capture, a borrowed view, or another value that cannot
  cross a task boundary.
- A channel moves a sendable owned value. The sender cannot keep an alias that
  permits unsynchronized writes after the send.
- A task group changes child lifetimes and cancellation only. Its children use
  the same capture and result checks as ordinary tasks.
- `para_map`, `para_filter`, `para_partition`, and `para_fold` reject mutable
  captures and values that workers cannot safely share or transfer.
- `Shared<T>` is the explicit shared-mutation path. Its `read` and `edit`
  closures use a lock-scoped view. `#Transact` uses the same handles and commits
  their changes atomically.

This is a guarantee for safe Jet source and Jet-owned Core APIs. Code inside an
`#Unsafe("reason")` region, a foreign implementation, or a vetted runtime
internal must uphold its boundary contract. Those explicit boundaries are not
proof that arbitrary foreign code has no internal data race.

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

`tests/workspace_crates.rs` pins the current path-dependency direction. Compiler
front-end crates may not grow back-edges into driver/codegen clients. Tooling and
runtime crates stay outside the compiler seam unless their dependency row is
changed here and in the test. Jetpack/JetOS live in `crates/jetpack`; `jetos`
(its own crate/binary, card #367 slice 2) is a thin front door still
dispatching into `jetpack::JetOS`. The shared manifest/lock/store-listing/
FFI-bridge/script-dep data model, plus the pure effect-budget/lint-policy
policy computation (card #367 slice 3), lives in `crates/jet-pkg-model`
(D-PRODUCT-SPLIT1=C), which `jetpack` re-exports under the legacy module
names so its own internal call sites are unchanged. `jet-driver` depends on
`jet-pkg-model` directly — never on `jetpack` — so the compiler's module
loader never needs Jetpack's provider/network/shell engine to resolve
`use <pkg>` imports.

The root `jet` package (`Source/`) routes the same way: it no longer carries a
blanket `pub use jetpack as Jetpack` re-export (card #367 slice 3). Read-only
model needs (`PackageManifest`, `Manifest`, `ScriptDeps`, `Lock`, `CBind`,
`CFFI`, `FFI`, `EffectBudget`, `LintPolicy`, and the hangar-listing half of
`Store` as `PkgStore`) come from `jet-driver`'s `jet-pkg-model` re-export, the
same seam the compiler itself uses. Genuine `jetpack`-engine calls that
haven't split out yet (`Overlay` engine side, `Discovery`, `JetPin`,
`ScriptLock`) stay direct `jetpack::…` references in a small, explicit set of
files (`tests/workspace_crates.rs::direct_jetpack_imports_stay_behind_known_boundaries`
pins exactly which). `ModuleEval` is no longer one of them (card #367 slice 4:
sank into `jet-env-model` L2). `WorkspaceFile` and `WorkspaceLock` are no
longer one of them either: card #367 slice 5 sank the pure overlay-policy
types and parse/strip into `jet-pkg-model::Overlay` (L1), the
`WorkspacePlan`/`WorkspaceMember` types and lock read path into
`jet-pkg-model::WorkspacePlan`/`WorkspaceLock` (L1), and the `workspace.jet`
evaluator (`load`/`evaluate`) into `jet-env-model::WorkspaceFile` (L2). Both
`jetpack::WorkspaceFile` and `jetpack::WorkspaceLock` are now re-export shims.
`jet-devserver` dropped its `jetpack` dependency entirely; Canvas
WorkspaceFile/WorkspaceLock scans (`jet_env_model::WorkspaceFile::load`,
`jet_env_model::WorkspaceLock::load`) and env-plan scans
(`jet_env_model::ModuleEval::evaluate_env`) all route through `jet-env-model`,
the same L2 crate both realizers depend on. The three acyclic layers now own
their full surface: L1 `jet-pkg-model` data, L2 `jet-env-model` plan model,
L3 `jetpack` env-runtime + JetOS realization (both depending down on L2).
The root binary
still owns native build execution: `Source/CmdCompile.rs`
invokes rustc, classifies linker/tool failures, renders the I2 ICE banner, and
links any prepared FFI artifact. Do not move that responsibility into a seam
crate in documentation until the code moves with it.

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

### Adding an FFI bridge

Foreign dependencies stay behind the existing runtime boundary; they never
become dependencies of the compiler workspace crates.

1. Start from a ratified interop surface and dependency approval. Do not add a
   new user spelling, ABI policy, or external stdlib dependency from code alone.
2. Parse and type-check the Jet declaration in the normal front end. Unsupported
   declaration types, unsafe signatures, and invalid boundary conversions need
   Jet diagnostics before codegen (I2/I3/I4). Tool and library availability is
   not a source-level fact; classify it later at bridge build/native link time.
3. Extend `crates/jet-pkg-model/src/FFI.rs` (or `CFFI.rs` for C) to prepare the
   bridge (re-exported at `crates/jetpack::FFI`/`::CFFI` and `crates/jet-driver::FFI`).
   The Rust bridge is a generated, content-addressed crate under Jet's cache: its
   generated `Cargo.toml` owns foreign dependencies, `cargo build` produces an
   rlib, and `FfiLink` records the crate name, rlib, selected-target runtime
   dependency directory, and host proc-macro dependency directory. Every rustc
   consumer passes the deterministic target-then-host, deduplicated search list.
   Do not put the dependency in the root or compiler-seam `Cargo.toml` files.
   Inline `#FFI(c|cpp|asm)` bodies use the same `FFI.rs` bridge and include the
   exact raw body, checked signature, target, and bridge schema in their cache
   identity, including the selected target and native toolchain identity.
   C/C++ bodies compile behind generated C-ABI wrappers for that target; asm lowers
   only after sema has proved its named operands, return anchor, clobbers, and
   target contract.
   `jet inspect bind cpp` is owned by `CppBind.rs`: clang AST discovery produces
   a deterministic Jet module plus C-linkage shim/archive under
   `.jet/bindings/cpp/`. Clang JSON—not header text—is the declaration source of
   truth. The content-addressed provenance hashes the header, selected
   namespaces/templates, target, absolute clang/archiver identities, include and
   library search inputs, link libraries, clang version/AST, generated sources,
   and schema. The proof link uses the same inputs with undefined symbols denied;
   final native link discovery reads the generated link sidecar.
4. Thread the prepared `FfiLink` through `crates/jet-driver/src/Driver/mod.rs` and
   `CompileOutput`. `Source/CmdCompile.rs::build` is the real native link edge: it
   passes `--extern`/`-L dependency` to rustc and keeps missing-tool/library
   failures out of the ICE path by reporting build/link diagnostics there.
5. Keep generated wrappers minimal and audited. Safe Jet cannot acquire an
   ungated unsafe operation through a bridge; boundary ownership, error, and
   layout conversions must be explicit.
6. Add focused tests for front-end rejection, generated wrapper/link arguments,
   cache reuse, and a real end-to-end bridge call. Add the diagnostic snapshot
   and docs for every new error. Run
   `scripts/agent/jet-env full cargo test --test cffi` for the C bridge matrix,
   the scoped `polyglot_systems` C++ compile/link/run proof, and
   `scripts/agent/jet-env full cargo test --test golden` for real generated calls,
   then run the project verification workflow. `tests/cffi.rs` and
   `tests/golden.rs` are the executable proof.

### Incremental Compiler Service

D-LSP1 makes editor tooling a client of the front end, not a second checker.
`crates/jet-queries` is a std-only demand cache for file inputs and derived
queries. The LSP stores open-buffer text as query inputs and memoizes lexing,
diagnostics, checked bundles, and fix data through that cache. A changed root is
reloaded through the canonical parser; sema then reuses span-exact checked
function bodies while their signature/import/global environment is unchanged.
Signature and dependency changes invalidate that item cache, and disk import
checks conservatively revalidate their module closure. Warm-session timings are
reported as observations; deterministic query/item counters and retained-byte
totals are the regression gates. The server records cancellation concurrently
with request execution and replaces a cancelled in-flight result with JSON-RPC
`-32800`. D-LSP2 requires every advertised LSP capability to have named coverage
in `tests/lsp.rs`; the server must not advertise speculative features.

## Compiler-extension plugins (D-DX5-HOOK1=A)

Tower #549. After sema, the compiler may freeze a **versioned typed
read-only snapshot** and send it to an isolated WASM Component Model guest.
The guest returns structured findings and edit proposals; the host validates
every response and remains the only semantic authority (I2/I3).

- **Boundary ownership:** `crates/jet-pkg-model::CompilerExtension` owns the
  versioned snapshot, response validation, and lifecycle. Its
  `Prelude/CompilerExtension.rs` substrate compiles only into the shipped
  sibling `jetpack` binary, using the same wasmtime Component Model pin as
  application `core.plugin` (`WASMTIME_CRATE_SPEC` / D-DEP-WASM1). Ordinary
  `jet` processes never link or initialize Wasmtime.
- **WIT world:** `compiler-extension-v1` (`package jet:compiler-extension@0.1.0`,
  export `analyze`). Distinct from application plugins' fixed world
  `jetplugin` (D-PLUGIN1 / D-PLUGIN-EXPORT1).
- **Not:** PATH-discovered `jet-*` helpers (D-DX5 in `Source/main.rs`), and
  not `target: plugin` / `core.plugin` application loaders.
- **V1 stage:** `typed` only. Later parse/codegen observation extends the same
  capability-negotiated protocol (I8 — one mechanism).

### Protocol / schema (exact)

`analyze` carries opaque `list<u8>` payloads. Host-owned wire format is UTF-8
JSON with lexicographic key order and no insignificant whitespace
(`CompilerExtension::{TypedSnapshot,AnalyzeResponse}`).

**Snapshot** (`protocol=1`, `stage="typed"`, `trust="untrusted"`):

| Field | Meaning |
|-------|---------|
| `capabilities` | Negotiated subset of `read_types`, `read_symbols`, `read_effects`, `read_spans`, `read_provenance`, `emit_finding`, `propose_edit` |
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
  user-written gated regions — an `#Unsafe("reason") { … }` block (or bare
  `#Unsafe { … }`, which emits lint L3101) or an `#Unsafe fn` contract, both
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
- **R6 — Name mangling.** User identifiers are emitted as `user_<name>`
  (`main` excepted) so user code can never collide with Rust keywords,
  macros, or std items.
- **R7 — Backend is swappable.** Rust emission stays in
  `crates/jet-codegen/src/`; native rustc invocation and ICE classification
  stay in `Source/CmdCompile.rs`. The lexer, parser, and sema crates do not
  depend on either responsibility. Another backend replaces the codegen and
  binary-build edges without changing the front end.
- **R8 — Small, self-contained binaries.** The root binary build path in
  `Source/CmdCompile.rs` calls `rustc` directly with `strip=symbols` and thin LTO,
  so the linker keeps only
  what the program uses ("only link what's needed"). Output is one
  self-contained native binary. Floor: Rust's std links a baseline
  (low-hundreds-of-KB), accepted as the cost of a beginner-friendly
  std-backed runtime — we do NOT pursue `no_std` in v1 (it would remove
  the conveniences priority #2 depends on). A size-minimal profile
  (`opt-level="z"`, possibly `panic=abort`) is decision S15, exposed
  later as `jet build --small`; the default leans toward speed.
- **R9 — A file is a complete program.** `jet run foo.jet` compiles and
  runs a single file with no manifest, no project folder, no config.
  The compiler invokes `rustc` on one generated `.rs` file — it never
  creates or requires a Cargo project for user code. Agents must not add
  a mandatory project structure, lockfile, or manifest for users; any
  future multi-file/package story is opt-in and post-v1 (see roadmap).
- **R10 — Std is pay-for-what-you-call.** M10 standard-library modules are
  compiler-known namespaces, but importing them is free. Sema records the
  core helpers that a checked program can call, and codegen emits only those
  helper templates. A program that imports every core module but calls none
  should stay in hello-world size territory.

  Embedded Core runtime templates under `crates/jet-codegen/src/Prelude/` are
  the canonical source for compiler-known Core behavior; rebuild `jet` before
  smoke-testing any change because `include_str!` snapshots them into the
  binary. A first-party package with a separately buildable source tree must
  not maintain a copied fallback template. `core.archive` is the concrete
  model: `corelib/core.archive/pkgs/archive/src/lib.rs` is consumed directly by
  both CoreProvider and the hidden bridge fallback.
- **R11 — Generated code re-enters the front end.** Every build-time
  code-generation step — a derive body, a comptime splice, any future
  metaprogram — emits a **typed source fragment** that re-enters
  lexer→parser→sema exactly like hand-written code. **No generation path may
  inject pre-parsed AST past the sema gatekeeper** (R2). The guarantee that
  buys: generated code is trustworthy-by-construction (R1 codegen-dumb, R2
  sema-gatekeeper, R5/I2 rustc-never-speaks all keep holding through
  generation), and any error in generated output surfaces as a **real sema
  diagnostic pinned to the user's trigger site** — the struct, field, or
  derive marker that caused it — with the generated fragment shown only as
  optional context, never as raw rustc output. The shipped `#[Codable]` derive
  already works this way; it is the required shape for all future derives and
  build-time steps (S56 user derives, comptime). (D-CTCODEGEN1=A, ratified
  2026-06-25; pairs with D-METADERIVE1=A, which makes a user derive's output a
  source fragment for exactly this reason.)
- **R12 — Two consumers, one executable IR.** Every executable TIR variant is
  owned by two consumers: the Rust emitter for AOT binaries and the dev/JIT
  lowerer for `jet dev`. The Rust emitter and the JIT lowerer must both handle
  executable TIR exhaustively, wildcard-free, either by real lowering or by a
  named internal unsupported reason that falls through transparently to the
  next dev tier. A feature PR is incomplete unless its example/golden proves
  the AOT path and `tests/dev.rs::dev_default_matches_compiled_binary` proves
  default `jet dev` has the same stdout, stderr, exit code, diagnostics,
  panics, and side effects. Native JIT coverage is a performance tier; semantic
  parity is mandatory.

## Exit codes (stable contract)

The `jet` driver returns one of a small, stable set of exit codes so
scripts and CI can branch on the outcome without parsing output. This
table is pinned by the `tests/cli/` transcripts and is part of the public
contract — codes are never repurposed.

| Code | Meaning | Who produces it |
|------|---------|-----------------|
| `0`   | Success. (Includes the no-args greeting — orientation, not an error.) | driver |
| `1`   | User error: a reported diagnostic, a failed `check`, a missing file, a failed `test`. | driver |
| `2`   | Usage error: unknown subcommand (E2101), unknown/ambiguous flag (E2102), or a missing required argument. | driver |
| `70`  | A built program panicked at runtime (`panic`/`require`, S36). Forwarded through by `jet run`. | the user's program |
| `101` | Internal compiler error (I2/R5): rustc rejected generated code. A P0 bug, never the user's fault. | driver |

Presentation is TTY-aware (E2-M3): color and progress appear only when
the relevant stream is a terminal. `NO_COLOR` and `--color=never` force
plain output; `FORCE_COLOR` and `--color=always` force color; `--color=auto`
(the default) defers to TTY detection. Piped or CI output is always plain,
deterministic, ANSI-free bytes — scripts never parse escape sequences.

## Testing strategy

1. **diagnostic snapshots** (tests/diagnostic_snapshots.rs): every
   diagnostic's exact text, pinned. The error messages are the product;
   treat snapshot diffs like UI diffs.
2. **golden examples** (tests/golden.rs): examples/ must front-end-pass,
   contain no `unsafe`, and — when rustc is present — build and print
   exactly examples/features/expected/*.out.
3. rustc-as-verifier: golden tests assert rustc accepts generated code,
   so a sema soundness hole becomes a loud test failure, not a shipped bug.

## Why transpile to Rust (recorded rationale)

The front end is hand-built either way; only the backend was a choice.
Rust gives: a soundness verifier for our ownership checker (critical when
agents write the compiler), LLVM optimization, cross-compilation, and std
— for free. Known costs, accepted: compile times stack on rustc's;
debuggers show generated Rust until M6+ tooling. Precedent: cfront, Nim,
TypeScript, Gleam.
