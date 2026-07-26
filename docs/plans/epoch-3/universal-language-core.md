# Epoch 3 — universal language and Core product parity

**Status:** executable master plan. **Audit date:** 2026-07-09.

This plan covers the language, compiler, runtime, Core, interoperability, and
developer-facing product surfaces. It does not duplicate the Epoch 4 Jetpack
program in `world-class-package-manager.md`, the Epoch 6 Canvas program, the
Epoch 7 JetOS program, the Epoch 8 documentation/CI program, or Epoch 9
self-hosting. Those programs are dependencies of the final universal-product
claim.

## Exit claim

Jet may claim language or ecosystem parity only when a user can build, debug,
profile, test, package, deploy, and operate representative production systems
without a hidden mock, transcript, fixture, schema-only path, unsupported
lowering, silent omission, or unverified platform claim.

An API name is not a capability. A parser path is not execution. A generated
schema is not a package manager. A fake DOM is not browser proof. A three-frame
transcript is not a game runtime. A validated TIR followed by AST emission is
not R12. A fallback is a capability only when its observable contract is the
same, its limitations are explicit, and its acceptance lane proves them.

The milestone closes only when every attached card is done, every acceptance
lane below is green on the required platform matrix, and every dependency epoch
needed by the product proof is also complete.

## Current grade

The architecture and product intent are unusually strong. The implementation
is a broad prototype with several real vertical slices, but it is not yet an
honest replacement for a production language ecosystem.

| Area | Grade | Audited truth |
| --- | --- | --- |
| Specification and architecture | A- | Safety, one semantic path, TIR ownership, diagnostics, and beginner/expert priorities are explicit. Some docs describe staged or fallback behavior as shipped. |
| Front end and native AOT | C+ | Large syntax and sema surface, real native execution, and broad tests. Confirmed accepts-invalid, unsupported-lowering, generic-module, and rustc-leak risks keep it below production grade. |
| TIR/JIT/comptime/REPL/web parity | D | Coverage manifests and explicit divergences contradict R12. Web validates TIR and then emits from AST-shaped data; REPL/comptime/JIT support proper subsets. |
| Safety and security | D | Safe intent is strong. Cryptographic randomness now uses one fail-closed OS provider with bounded, zeroized WASI retries; cross-target live proof and silently unsupported C FFI lowering remain stop-lines. |
| Core | C- | Broad typed signatures and many executable helpers exist. Several modules are thin bridges, partial protocols, deterministic facades, or compiler-emitted templates with weak live conformance. |
| Concurrency and networking | C- | Structured task vocabulary exists. Pause/cancel controls, blocking server execution, stream-select gaps, platform fallbacks, and ignored scale tests leave the runtime incomplete. |
| REPL, CLI, dev server, IDE, debug | C | Strong active UX work and broad LSP features. REPL editing/history, dependency-aware watch, real editor DAP, query-engine reuse, testing workflow, profiling, and notebooks remain incomplete. |
| Web and full-stack applications | D | A narrow JS/DOM subset and web dev server exist. Unsupported web nodes can disappear or become `undefined`; there is no first-party React/SvelteKit/Next-class application platform. |
| Native/mobile UI | D | One real GTK/Linux slice exists. macOS, Windows, iOS, Android, real-browser conformance, and state-preserving native hot reload are absent. |
| Data, ML, and accelerators | D+ | Typed CSV/table/stat/plot slices exist. Production dataframe semantics, Arrow/Parquet, ndarray, autodiff, ML, device execution, and multi-accelerator proof do not. |
| Game/media | F | Current `core.game` proof is a headless three-frame string transcript; assets fail by path substring. No renderer, audio, asset pipeline, editor, or packaged live backend is proven. |
| Packages and environments | C- | See the independent Epoch 4 audit and #393–#434. Do not duplicate that program here. |
| Production dogfood | D | Current apps are classified as slices, not capstones. No cross-domain portfolio proves the replacement claim. |

Overall: **C- language foundation, D product-equivalence**. The first work is
truth repair and P0 correctness, not adding more names to the surface.

## Confirmed stop-lines

These are source-backed defects, not roadmap speculation:

- `crates/jet-sema/src/Sema/FFI.rs:90` accepts C ABI types that
  `crates/jet-codegen/src/Codegen/CModule.rs:83` emits as
  `/* unsupported */ ()`. Interior-NUL strings also collapse to empty at
  `CModule.rs:36`.
- `crates/jet-codegen/src/Codegen/Items.rs:691` replaces non-primitive
  `#Default(expr)` values with `Default::default()`, silently changing typed
  CLI and decode semantics. The same file directly emits Rust `Codable` impls
  instead of satisfying R11's re-entry law (`Items.rs:667`).
- `tests/dev.rs:480` skips four default AOT examples; `tests/jit_gaps.txt`
  records 45 covered shapes, 245 gaps, compiler panics, verifier failures, and
  two interpreter/AOT output divergences.
- `crates/jet-codegen/src/Codegen/Web.rs:54-124,638-748,920-957` validates TIR,
  then emits from AST-shaped data; unsupported cases can disappear or become
  `undefined`.
- `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs:1857-1866` lowers
  native `core.web` effects to no-ops, empty strings, and `None`.
- `crates/jet-sema/src/Sema/Bundle.rs:194-285` implements only a signature/
  function subset of generic modules; value parameters, bounds, cycles, and
  body types remain staged.
- `Source/REPL/mod.rs:367-388` records every list as `[Int]`, every map as
  `[String:Int]`, and skips Option/Result/struct/enum/closure state. The raw
  substring gate at `Source/REPL/mod.rs:930` rejects harmless comments/strings.
- `crates/jet-codegen/src/Codegen/mod.rs:241-272,1241-1253` emits the whole Core
  prelude after the first Core call, contradicting R10 at source/compile level.
- `crates/jet-codegen/src/Prelude/CoreLib/Top/HttpServer.rs:89-164,231-249,
  378-429` uses an OS thread per connection, allocates unbounded requests,
  defaults malformed input to `GET /`, collapses repeated headers, and can
  slice UTF-8 static-file ranges on invalid boundaries.
- `crates/jet-codegen/src/Prelude/CoreLib/Top/Process.rs:75-197` drains child
  pipes serially, waits before timeout drains, checks limits after allocation,
  loses signals, and maps interrupt/terminate to the same hard kill.
- `crates/jet-codegen/src/Prelude/CoreLib/Top/Game.rs:209-358` implements asset
  failure by a `"missing"` path substring and runs exactly three synthetic text
  frames.
- `crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs:59-87,113-172` implements
  normalization, case folding, segmentation, and width with small hard-coded
  maps and heuristics rather than Unicode conformance data.
- `crates/jet-codegen/src/Prelude/CoreLib/JetStd/CommonTypes.rs:123-141` and
  related data helpers execute “lazy” work eagerly; joins return grouped counts
  rather than typed joined rows.
- `crates/jet-debug/src/Dap.rs` has no real editor acceptance; the shipped
  adapter is minimal and one-threaded. `crates/jet-queries/src/lib.rs:79-185`
  remains LSP-only and whole-document despite #209's shared incremental exit.
- `Source/CmdCompile.rs:613-726,834-894` lacks the promised testing and project
  formatting workflows; `--update-snapshots` is ignored.
- `Cargo.toml:32-51`, `crates/jet-driver/Cargo.toml:10-17`, and
  `Source/Bin/JetOS.rs:1-8` show that #367's ratified product split was replaced
  by ratchets, not performed.

## Evidence classes

Every capability row and Tower closeout uses exactly one class:

1. **Reserved** — syntax, type, command, or schema is recognized but not run.
2. **Facade** — public shape exists over a mock, deterministic transcript,
   placeholder, silent omission, or non-production backend.
3. **Partial** — real execution exists for a named subset; unsupported cases
   fail loudly with a Jet diagnostic.
4. **Implemented** — complete documented behavior works on one supported path.
5. **Proven** — implemented behavior passes cross-tier, cross-platform, live,
   hostile, scale, recovery, and dogfood lanes applicable to its claim.

Only **Proven** closes a capability card. Partial paths are useful and remain
shippable when truthfully labeled; they never close a broader claim.

## Product laws

1. **One executable meaning.** Parser, sema, TIR, AOT, JIT, comptime, REPL,
   web, debugger, editor, and analysis tools consume the same semantic facts.
2. **Exhaustive lowering.** No wildcard arm may silently omit a checked node or
   synthesize `undefined`, `()`, empty text, or a success-shaped placeholder.
3. **Core is real software.** Typed signatures and emitted templates do not
   count without a real implementation, failure semantics, standards tests,
   and target conformance.
4. **No fake closure.** Fake DOMs, fake clocks, fake registries, fake QEMU,
   in-memory transports, and deterministic transcripts support tests; live
   acceptance closes cards.
5. **Beginner magic and expert control share one mechanism.** Defaults select
   safe policy; expert flags expose target, scheduler, authority, cache,
   device, generated code, and proof facts without changing meaning.
6. **Silent degradation is a defect.** Unsupported behavior fails before
   execution with a Jet diagnostic and the smallest valid fix.
7. **Platform claims are literal.** Linux proof says Linux. Desktop means
   Linux, macOS, and Windows. Mobile means iOS and Android. Web means supported
   browser engines. Each tier has a published matrix.
8. **Performance claims include correctness.** Benchmarks run equivalent work,
   publish source and environment, enforce memory/startup/tail-latency budgets,
   and never excuse semantic gaps.
9. **Foreign bridges are accelerators, not facades.** Every bridge exposes
   provenance, safety, ownership, replacement status, and live conformance.
10. **Done is reproducible.** A clean machine can rerun the exact proof from
    source and obtain the recorded artifact, transcript, metrics, and report.

## Competitive contract

Jet does not copy surface syntax. It adopts the strongest semantic and product
properties through one coherent Jet mechanism.

| Domain | Reference bar | Jet closure proof |
| --- | --- | --- |
| Safety and systems | Rust ownership, Zig comptime/build control, Swift ergonomics | memory/ownership adversary corpus; no safe-source `unsafe`; verified FFI; freestanding and hosted target portfolio; compile/startup/runtime budgets |
| Concurrency and services | Go task/network ergonomics; Erlang/Elixir supervision and upgrade discipline | one task/taskgroup scheduler; nonblocking I/O; cancellation/deadline proof; supervised service tree; cluster/chaos/rolling-generation tests |
| Interactive work | Python/Julia/Jupyter REPL and notebook loop | multiline structural REPL; persistent search; rich display; Jupyter protocol; interrupt/debug/profile; AOT-equivalent Core semantics |
| Web applications | React compiler and server components; Svelte runes and compiled reactivity; SvelteKit/Next routing, data, forms, SSR; Vite HMR | one typed application graph; fine-grained dependency compilation; SSR/SSG/streaming; hydration/islands; server actions/forms; accessibility; real-browser HMR and deployment adapters |
| Native/mobile UI | SwiftUI and Jetpack Compose state, previews, accessibility, adaptive layout | one renderer/component model; desktop/mobile backends; previews; hot reload; navigation/lifecycle/restoration; accessibility and store packaging |
| Data/ML/compute | NumPy broadcasting/ufunc/linalg; dataframe/lazy-query systems; accelerator graphs and explicit kernels | one typed public compute model beneath executable TIR; ndarray/table schemas; Arrow/Parquet; lazy fusion; autodiff; CPU/GPU parity; device ownership; multi-device/distributed proof |
| Tooling | rust-analyzer incremental semantics; Go tool coherence; .NET diagnostics | shared incremental query service; LSP/DAP; formatter/test/doc/profile/trace; stable JSON; one diagnostic model; project-scale latency gates |
| Packages/builds | Nix hermetic store/build model plus Cargo/uv/pnpm ergonomics | Epoch 4 #393–#434 and its live hostile acceptance lanes |
| Games/media | mature engine asset/render/audio/input/editor/replay workflows | real renderer/audio/input; asset import/cook/hot reload; ECS; replay/networking; editor; packaged game on tier platforms |
| Ecosystem reach | C/C++/Rust/Swift/JS/Python/JVM/.NET/R/Julia integration | generated typed bindings; ownership/error/async mapping; real upstream suites; in-situ native replacement proof |

Primary references used to set this bar:

- [React Compiler](https://react.dev/learn/react-compiler), [React Server Components](https://react.dev/reference/rsc/server-components), and the [Next.js App Router](https://nextjs.org/docs/app)
- [Svelte runes](https://svelte.dev/docs/svelte/what-are-runes), [SvelteKit](https://svelte.dev/docs/kit/introduction), and [SvelteKit form actions](https://svelte.dev/docs/kit/form-actions)
- [Vite feature and HMR model](https://vite.dev/guide/features.html) and [Vite SSR](https://vite.dev/guide/ssr.html)
- [Rust ownership](https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html), [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html), and [Cargo features](https://doc.rust-lang.org/cargo/reference/features.html)
- [Go modules](https://go.dev/ref/mod), [race detector](https://go.dev/doc/articles/race_detector), [fuzzing](https://go.dev/doc/security/fuzz/), and [diagnostics](https://go.dev/doc/diagnostics)
- [Python asyncio](https://docs.python.org/3/library/asyncio.html), [Jupyter architecture](https://docs.jupyter.org/en/stable/), and [Jupyter kernels](https://docs.jupyter.org/en/stable/projects/kernels.html)
- [Elixir supervision](https://elixir-lang.org/getting-started/mix-otp/supervisor-and-application.html) and [Erlang release handling](https://www.erlang.org/doc/system/release_handling.html)
- [Zig language and build system](https://ziglang.org/documentation/master/) and [.NET diagnostics](https://learn.microsoft.com/en-us/dotnet/core/diagnostics/)
- [NumPy broadcasting](https://numpy.org/doc/stable/user/basics.broadcasting.html), [NumPy ufuncs](https://numpy.org/doc/stable/reference/ufuncs.html), and the [CUDA programming guide](https://docs.nvidia.com/cuda/cuda-c-programming-guide/)

## Work program

Work order is binding. Existing cards are reused and reopened where their
original acceptance is unmet. New cards cover only untracked outcomes.

| Tower | Program role |
| --- | --- |
| #435 | UL0 truth ledger and claim stop-line |
| #436 | focused P0 C ABI correctness vertical under #180 |
| #437 | Core source architecture, reachable emission, and conformance |
| #438 | first-party full-stack web application platform |
| #439 | dependency-aware watch/dev/hot-replacement service |
| #440 | project formatter workflow |
| #441 | first-party performance session and trace product |
| #442 | notebook kernel and rich interactive display |
| #443 | native data/ML/autodiff/accelerator computing |
| #444 | fault-tolerant distributed service runtime |
| #445 | competitive laboratory and production capstone portfolio |

Reopened canonical cards retain their original ownership: #12, #17, #64,
#84, #91, #95, #117, #123, #125, #126, #129, #131, #134, #180, #209, #224,
#237, #238, #239, #244, #286, #288, #291, #292, #296, #298, #300–#302,
#306–#308, #343, #353, and #367. Active #358/#360/#362 are attached without
resetting their building state. #392 remains in the owner activation lane. #86
remains frozen in Epoch 8. E4 #393–#434 and #359/#361 remain the one Jetpack
package/environment/CLI program.

### UL0 — truth ledger and claim stop-line

- Generate a checked capability ledger from source, tests, target matrix, and
  Tower evidence using the five evidence classes above.
- Reclassify every E3 `done` capability; reopen any card closed by a facade,
  subset, fixture, ignored test, fallback, static schema, or plan-only proof.
- CI rejects broad claims unsupported by live evidence and rejects code/docs/
  Tower drift.
- Exit: every advertised language/Core/tool capability points to an executable
  proof; deletion of that proof fails CI.

### UL1 — P0 semantic and security stop-line

Card #436 is governed by `D-CABI-CALLBACK1=A`, `D-CABI-RESULT1=C`, and
`D-CABI-PLATFORM1=A`. C callbacks are non-null, monomorphic, and C-safe.
They must be capture-free or have an explicit `--[]->` bound. They remain
restricted to the foreign-thread-safe subset. Generic Result stays outside C
declarations; raw status/out-pointer
functions get ordinary Jet wrappers. Alternate native conventions use a local
per-function `#Abi` marker, with C as the default, the exact target matrix in
the syntax ledger, no module inheritance, and no invented symbol decoration.

- Reopen #180 for C FFI types that sema accepts but codegen cannot emit.
- #302 now removes predictable randomness through D-CRYPTO-RNG1's shared
  fail-closed OS provider. D-CRYPTO-WASI-ALLOC2 gives every interrupted WASI
  call a new exact-count zeroed ownership generation, zeroizes and drops it
  before retry, allows numeric address reuse, caps calls at seventeen, and
  exposes no failed bytes. Keep #302/#64 open for the remaining cross-target
  live backend and full reference-vector proof.
- Reopen #353 for accepts-invalid, miscompile, generated-unsafe, generic,
  ownership, sendability, and rustc-leak adversary campaigns.
- Reopen #91 for complete generic modules; reopen #129/#131 for R11-compliant
  generated Jet and exact `#Default(expr)` behavior.
- Reopen #343 so every active diagnostic is reachable and snapshot-covered;
  staged and unimplemented codes cannot sit in informational allowlists.
- Unsupported target/backend behavior becomes a Jet diagnostic before codegen.
- Exit: all known silent-success and predictable-security paths removed; fuzz,
  sanitizer, race, differential, and hostile fixtures run in CI.

### UL2 — one executable TIR and tier parity

- Reopen #125. Remove the `jit_covers` whitelist; every AOT-runnable program
  has a behavior-identical dev path, with an internal fallback only when it is
  transparent and tested.
- Reopen #244. Web emission consumes executable TIR only; no AST re-emission,
  silent wildcard omission, or `undefined` synthesis.
- Activate #392 for comptime/REPL builtin parity, beginning with BigInt.
- Reopen #84 where the REPL advertises full Core but uses native-only stubs or
  collapsed types.
- Exit: generated parity matrix covers stdout, stderr, exit, panic, effects,
  Core, ownership, and diagnostics across AOT/JIT/comptime/REPL/web.

### UL3 — Core implementation boundary and conformance

- Preserve D-JPK-RINGSHIP1=C delivery: Core rides the pinned toolchain and works
  offline. Replace hand-maintained compiler-template breadth with a minimal,
  audited intrinsic/ABI kernel plus ordinary Jet Core packages compiled through
  the same front end. The developer fallback is generated from the same source.
- Make R10 mechanical: emit/link only reachable Core packages and intrinsic
  fragments; a Core call never drags all fragments into generated source.
- Publish per-function ownership, effects, failure, blocking, platform, and
  backend facts through the semantic index and dossier.
- Differentially test any native/foreign acceleration against the canonical
  Jet semantics.
- Exit: Core can be built, tested, documented, profiled, and audited as source;
  compiler templates contain only the ratified intrinsic kernel.

### UL4 — runtime, networking, and production services

- Reopen #126/#306: real pause/cancel semantics, one-winner select,
  task-owned I/O, blocking-pool isolation, epoll/kqueue/IOCP, fake-clock proof,
  and non-ignored scale lanes.
- Reopen #300/#301/#17: typed socket addresses/DNS/deadlines; bounded streaming
  HTTP; pooling, cookies, redirects, proxies, multipart, SSE, WebSockets,
  HTTP/2 and HTTP/3; graceful shutdown; abuse limits; real TLS.
- Reopen #291/#292: concurrent bounded process capture, process-tree control,
  real signals, distinct interrupt/terminate/kill, and Windows console control.
- Reopen #288/#296/#298: durable atomic file replacement and typed paths;
  standards-complete XML; versioned Unicode conformance data.
- DNS never invents a public resolver, fixed transaction ID, or unvalidated
  response; system configuration, TCP fallback, retry, IPv6, and hostile-packet
  tests are mandatory under #300.
- New service-runtime card adds the owner-selected supervision, typed mailbox,
  backpressure, placement, cluster, recovery, and rolling-generation model.
- Exit: one production service survives cancellation races, connection floods,
  partial reads, worker failure, network partitions, deployment, and rollback.

### UL5 — REPL, notebook, help, and interactive diagnostics

- Finish #358 against the full owner matrix: structural multiline editing,
  persistent searchable history, completion, highlighting, bracketed paste,
  terminal-width/unicode handling, interrupt, safe recovery, and no-terminal
  deterministic mode.
- Finish #360 without duplicate registries or nonexistent commands.
- Add a Jupyter-compatible kernel over the same REPL session engine with rich
  MIME values, plots/tables/UI previews, stdin, completion, inspect, interrupt,
  debug, and reproducible notebook export.
- Core/AOT parity is inherited from UL2; notebooks do not get a second evaluator.

### UL6 — unified watch, dev server, and hot replacement

- Finish #362 and its owner-environment matrix.
- Replace entry-file polling with the shared incremental dependency graph:
  imports, assets, HTML/styles, manifests, lockfiles, build inputs, generated
  source, and target facts invalidate exactly their dependents.
- Make the already-advertised `jet run --watch` real. `jet dev` adds the richer
  server/overlay/session surface over the same engine.
- Preserve compatible `#Persist` state through typed migration; incompatible
  state explains the reset. Client and server updates are one transaction.
- Exit: edit-to-visible budgets, crash/reconnect, rename/delete, dependency
  update, and state-migration lanes pass in real browsers and native apps.

**Shipped surface (#439):** `jet-devserver::WatchService` owns the typed
`WatchGraph` / `WatchSession` / `PersistStore` / `HotReplaceTxn`. Both
`jet run --watch` and `jet dev` (native + web) poll that engine. Receipts are
deterministic JSON; unsupported hot replacement keeps the prior session and
reports the failure.

### UL7 — formatter, test, docs, debug, IDE, and profiling product

- New project-formatter card extends `jet fmt` to workspace recursion,
  stdin/stdout, check/diff/changed modes, ignore/generated/vendor policy, stable
  ordering, and editor parity after owner ratifies command/default semantics.
- Reopen #308: real snapshot update, filters/tags/list/shuffle/parallel
  isolation, fuzz corpus/minimization, property shrinking, coverage, statistical
  benchmarks, budgets, and stable machine output.
- Reopen #209 narrowly: one item-granular incremental compiler service shared by
  check/dev/REPL/LSP; retain the shipped LSP breadth and add recomputation/latency
  proof.
- Reopen #12: real VS Code/Zed DAP sessions, conditional/data breakpoints,
  evaluate, exceptions, threads/tasks, native and interpreted parity, Linux/
  macOS/Windows backends, Jet-only default frames.
- New profiler card: CPU/wall/allocation/task/lock/I/O/browser tracing, Jet
  symbols, flamegraph/timeline/JSON, before/after comparison, and budget links.
- #86 remains frozen in E8 per owner decision; its semantic `jet doc` plan is a
  required dependency, not reactivated here.

### UL8 — first-party full-stack web platform

- Build the owner-selected application model over one typed application graph.
- Remove native `core.web` inert stubs. Existing Browser-effect and target law
  requires a target-checking diagnostic by default; this is an ungated
  correctness fix. Any future native embed/test host needs its own ballot.
- Components use the existing reactive/view/style/a11y foundations; compiler
  analysis performs fine-grained updates and safe automatic optimization.
- Router supports nested layouts, typed params, data dependencies, loading/
  error states, redirects, middleware, and code splitting.
- Rendering supports CSR, SSR, SSG, streaming, hydration/islands/resume policy,
  server components, server actions/RPC, progressive forms, and optimistic UI.
- Production surface includes assets/CSS, images/fonts, CSP/CSRF/sessions/auth,
  caching/revalidation, source maps, accessibility/SEO, observability, adapters,
  and real-browser devtools/HMR.
- Exit: TodoMVC, content site, authenticated SaaS, streaming dashboard, and
  offline/PWA app beat equivalent reference implementations on the published
  correctness/UX/performance rubric.

### UL9 — universal reactive native/mobile UI

- Reopen #134/#122. Keep one view/style/component/motion/a11y model.
- Complete real browser behavior, keyed reconciliation, input/forms, focus,
  selection, IME, clipboard, drag/drop, localization, theming, adaptive layout,
  navigation, lifecycle, restoration, and deterministic UI testing.
- Ship Linux, macOS, Windows, iOS, Android, web, and TUI backends with previews,
  hot reload, accessibility-tree proof, packaging, signing, and store artifacts.
- Native platform APIs remain reachable through typed expert handles without
  forking component semantics.

### UL10 — data, ML, and accelerator computing

- Reopen #237/#307 against their ratified full table/dataframe surface: typed
  schema/columns, missing values, joins, pivots, windows, lazy plans, Arrow,
  Parquet, SQL, streaming/out-of-core execution, plotting, and notebook display.
- Add ndarray broadcasting, ufuncs, linalg/decompositions, FFT, sparse arrays,
  automatic differentiation, optimizers, model serialization, training,
  inference, distributed datasets, and deterministic numerics policy.
- Add the owner-selected public compute model: CPU SIMD plus CUDA, Metal,
  Vulkan/WebGPU; device ownership, streams/events, kernels, fusion, transfer
  planning, memory limits, multi-device, profiling, and bridge/native
  equivalence. Any graph/kernel form stays an internal stage beneath TIR.
- Exit: production analysis, model training/inference, and custom-kernel apps
  run on CPU and tier accelerators with numerical/performance proof.

### UL11 — systems, embedded, interop, and replacement

- Reopen #239: turn typed target-profile data into real firmware/kernel builds,
  linker images, startup, allocator/panic policy, volatile/atomic/interrupt/DMA
  APIs, QEMU plus real-board proof, debug/flash, and reproducible artifacts.
- Reopen #180: make the one FFI structure real for C/C++/Rust/Swift/JS/Python/
  JVM/.NET/R/Julia, including callbacks, exceptions/results, ownership, async,
  generics, ABI/version checks, bindgen, debugger/source-map integration, and
  live upstream conformance.
- Replacement overlays are not caller-supplied proof metadata; they compile and
  run both implementations against generated, golden, fuzz, performance, and
  side-effect contracts before resolver policy may substitute them.

### UL12 — real game/media product

- Reopen #238. Preserve the ratified stable `core.game` substrate boundary.
- Replace path-substring assets and fixed transcript execution with real asset
  import/cook/cache/hot reload, ECS scheduling, window/input, renderer, audio,
  animation, physics integration, networking/rollback, replay, budgets, and
  editor hooks.
- Backends remain packages, but at least one first-party production renderer,
  audio backend, and editor integration must ship and pass live platform proof.
- Exit: packaged 2D and 3D games, replay determinism, multiplayer rollback,
  asset iteration, frame pacing, memory, and crash-recovery lanes.

### UL13 — product boundaries and build graph

- Reopen #367. Perform the ratified jet/jetpack/jetos crate and binary split;
  ratchets alone do not satisfy the card (`claim.product-boundaries`).
- #95/#224 public build product shipped: root `fn build(b: BuildContext)`,
  typed graph, `jet inspect graph` / `jet inspect query build` / `jet inspect explain-build` provenance,
  local action cache, sandboxed action execution, and jet→jetpack engine
  dispatch (`claim.package-build` / `public-build-product`).
- Root `jet` owns the language/dev loop, Jetpack owns packages/build/store/env,
  JetOS owns OS realization. Teaching shims preserve the ratified public UX.
- One versioned protocol connects processes without importing product engines
  into the driver.

### UL14 — competitive laboratory and production capstones

- Extend #344 from a checklist to enforced proof. Every domain ships one
  non-toy application and a reference implementation with equivalent work.
- Portfolio: compiler/tool, CLI, full-stack SaaS, desktop/mobile app, service
  cluster, data/ML notebook and batch job, GPU kernel, game, embedded firmware,
  plugin/FFI package, public package, and JetOS deployment.
- Record correctness corpus, UX task timing, source size, build/edit latency,
  startup, throughput, tail latency, memory, artifact size, energy, deployment,
  failure recovery, accessibility, and security.
- Independent reproduction on clean machines is required. The milestone never
  closes from microbenchmarks or project-authored assertions alone.

## Dependency order

```text
UL0 truth
├─ UL1 semantic/security stop-line
├─ UL2 executable TIR parity
├─ UL13 product boundaries
└─ UL3 Core architecture       after UL1 + UL2 + UL13

UL4 runtime/network/services   after UL1 + UL2 + UL3
UL5 REPL/notebook              after UL2 + UL3
UL6 watch/dev/HMR              after UL2 + shared query engine
UL7 toolchain product          after UL0 + UL2; profiler after UL3 + UL4
UL8 full-stack web             after UL2–UL4 + UL6 + owner ballot
UL9 native/mobile UI           after UL2 + UL3 + UL6
UL10 data/ML/compute           after UL1–UL4 + owner ballot
UL11 systems/interop           after UL1–UL3 + UL13
UL12 game/media                after UL1–UL4 + UL6 + UL9
UL14 capstones                 after all above plus E4, E6, E7, E8
```

## Binding acceptance lanes

1. **Truth:** delete or corrupt the implementation/proof; claimed capability
   becomes impossible to report.
2. **Tier parity:** AOT/JIT/comptime/REPL/web agree on output, failures, effects,
   ownership, panic, and diagnostics for every supported construct.
3. **Accepts-invalid:** grammar-guided mutation, fuzz, and adversarial programs
   never reach codegen after violating type, ownership, effect, or authority law.
4. **No rustc voice:** all accepted safe Jet is rustc-accepted; failures are Jet
   diagnostics or exit-101 internal compiler errors with captured reproducers.
5. **Safety:** Miri/sanitizer/race/static unsafe audit plus hostile FFI, alias,
   cancellation, secret, crypto, and malformed-input corpora.
6. **Platform:** Linux/macOS/Windows, supported browser engines, iOS/Android, and
   declared embedded targets run real artifacts; compile-only is labeled so.
7. **Live protocol:** HTTP/TLS/DNS/WebSocket/DAP/LSP/Jupyter/registry/cache/
   remote-build tests talk to independent implementations over real transports.
8. **Browser/UI:** real browser and native accessibility/input/render tests;
   fake DOM/headless fixtures remain supporting tests only.
9. **Scale:** million-line/workspace facts where applicable; 100k tasks/actions/
   objects; tail latency and memory budgets; no ignored closure test.
10. **Recovery:** process kill, power loss, cancellation race, reconnect,
    deployment rollback, cache corruption, state migration, and partial I/O.
11. **Performance:** equivalent work and output against published reference
    versions on pinned machines; startup, throughput, tail, memory, size, energy.
12. **UX:** novice task tests, expert audit/control tests, TTY/non-TTY, NO_COLOR,
    Unicode/narrow terminals, stable JSON, shell/editor/browser parity.
13. **Accessibility:** keyboard, screen reader, contrast, motion, focus, semantics,
    localization, and adaptive layout across UI targets.
14. **Interop:** real representative upstream libraries per provider; ownership,
    callback, exception, async, ABI, version, unload, and debugger cases.
15. **Dogfood:** Jet builds and operates its own representative products with
    the public surface; internal shortcuts fail the gate.

## Verification and ownership

- **Sol:** architecture, owner gates, language/runtime/Core integration,
  dependency order, final review, final verification, Tower closeout.
- **Terra:** bounded vertical implementations and independent reviews after the
  public contract is stable.
- **Luna:** inventories, corpus generation, fixture discovery, matrix evidence,
  mechanical migrations, and targeted command runs.

Each implementation card follows test/reproducer first, parser/sema/TIR/runtime
as applicable, diagnostics, example, docs, targeted tests, independent review,
then close. Only the Sol orchestrator runs
`scripts/agent/jet-env full scripts/agent/verify-full.sh`, once after a major
push on its closeout or blocking card; CI runs it again.

## Ratified owner decisions (2026-07-10)

All gates below are ratified as their hybrid option D and their implementation
cards are ready:

- `D-WEBAPP1` and `D-WEBAUTHOR1`: application-model ownership and explicit
  builder/optional convention authoring.
- `D-COMPUTE1`, `D-COMPUTE-TYPE1`, `D-COMPUTE-PLACE1`,
  `D-COMPUTE-KERNEL1`, `D-COMPUTE-AUTODIFF1`, and
  `D-COMPUTE-BACKEND1`: compute ownership, types, placement, safe kernels,
  differentiation, and backend contract.
- `D-SERVICE1`, `D-SERVICE-DELIVERY1`, `D-SERVICE-STATE1`,
  `D-SERVICE-WORKFLOW1`, `D-SERVICE-IDENTITY1`, and `D-SERVICE-UPGRADE1`:
  topology, delivery, state, workflows, authority, and generation handoff.
- `D-FMTPROJECT1`: project formatter command, defaults, and policy surface.
- `D-PERFSESSION1`: performance collection, trace, privacy, and report surface.
- `D-NOTEBOOK-SURFACE1`, `D-NOTEBOOK-DOC1`, and
  `D-NOTEBOOK-TRUST1`: clients/protocol, document truth, and active-output
  authority.

Full current law lives in `docs/spec/syntax-decisions.md`. New syntax, Core
external dependencies, diagnostic codes, manifest fields, provider roots, and
command spellings found during implementation require a reviewed follow-up
ballot before that slice.
