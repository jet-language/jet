# Domain foundations probe method and evidence map

Date: 2026-09

## Status and boundary

This note is the maintained method and topic map for the completed domain-foundations probe campaign. It is not a new probe report and it is not runtime qualification. The campaign's final reports, gap payloads, classification, master synthesis, and rendered proposal are preserved intact under:

`docs/audits/domain-foundations-2026-09-02/`

The executable evaluation corpus is separately maintained under:

`tools/agent-eval/domain-foundations/`

The historical ballot source is kept with that corpus at:

`tools/agent-eval/domain-foundations/slate/ballots.mjs`

The ballot module is source history and is not probe evidence. It must not be imported as a docs-time tool or treated as a runtime result.

The source-only Git recovery tree is tag `cleanup-source-2026-09-05-114107` (`6875b5e07a55900a630f280c9c88edbfca2094b1`). Every retired working note named below can be recovered exactly with `git show TAG:PATH`. The campaign itself recorded 22 probes, 99 gaps, 23 root-cause defects, 11 ballots, and 55 campaign cards. All 11 ballots were ratified A on 2026-09-03. Ratification records an intended contract; it does not turn an unshipped or failing implementation into a qualification result.

This note keeps the questions, applicability, method, constraints, alternatives, observed outcomes, and remaining implementation/proof ownership that made the working notes useful. It deliberately does not reopen ratified syntax or close Tower work.

## Reproducible method

### Source order and setup

Each probe followed the same evidence order:

1. Read the probe brief and the campaign's source-of-truth material: ratified specs and decisions, shipped examples, `jet help`, compiler facts, and the relevant Prelude/core surface.
2. Read the applicable law map and prior family evidence, including retained `dx2` reports and deleted-ballot history when that changed interpretation.
3. Build the smallest useful program in `~/.cache/jet-luna/dx3/<probe>` rather than editing the repository.
4. Run it through `scripts/agent/jet-env jet run`; build or run AOT only when the question required it.
5. Record successful behavior, exact diagnostics or ICEs, performance observations, and the smallest credible workaround. A workaround is evidence of reachability, not proof that the missing native surface is acceptable.
6. Put every observed gap in the matching JSON payload. The Markdown report explains the experiment, decision, and evidence. Area probes also record a focused battery.

Probe work did not write source files, Tower, Cargo metadata, or repository test fixtures. It used no network unless an experiment explicitly called the loader or compiler with `--allow-net`. A negative result is retained with its error code when the compiler or runtime blocked the path; a positive result includes the runnable path, not only a claim that a feature exists.

The campaign's final reports and machine-readable evidence preserve the exact transcripts and numbers. This note is an index and method summary, not a replacement for those files.

### Evidence layout

The audit directory preserves these groups without distillation:

- `domain-foundations-2026-09.md`: master synthesis and campaign status.
- `domain-foundations.html`: rendered proposal and its visual summary.
- `results/*.md`: 22 final probe reports.
- `results/*.gaps.json`: 14 primitive and 8 area gap payloads.
- `results/*.batteries.json`: the 8 area battery payloads.
- `all-gaps.json`: campaign-wide gap ledger.
- `defects.json`: root-cause defect ledger.
- `classification.json`: classification evidence.
- `probes/probes.json`: the 22-probe manifest.
- `slate/milestones.json`: the historical milestone input.

The master report is the campaign-level index. The JSON files are the numerical and machine-readable evidence. The Markdown files explain the experiments and preserve alternatives and workarounds.

## Cross-cutting law guardrails

These are the durable boundaries extracted from the 13 law maps. “Open” means an implementation or specification question remains. It does not mean the old syntax was silently accepted.

### Realtime and scheduling

**Applies to:** fixed-rate loops, audio, game frames, embedded callbacks, allocation and blocking guarantees.

**Ratified/current:** effects include `!Mem.Alloc` and `!Mem.Rc`; bounded denials, views, STM/channel/shared state, cancellation at wait points, `#Job`/`#Every`, headless game/replay, and `FrameTime` as a performance budget.

**Shipped:** `Duration` and sleep, deadline error `E3003`, channels and cancellation cleanup, game `on_frame`, replay/headless three-frame execution, and `core.perf`.

**Open and constrained:** fixed callbacks, ISR/audio callback contracts, nanosecond or absolute waits, lock-free `Shared`, and CPU/WCET evidence. `#Every` is not an ISR or callback; cancellation is not arbitrary preemption; current shared behavior can lock; a three-frame headless run is not a realtime guarantee; `FrameTime` is not WCET. Sleep currently truncates nanoseconds to milliseconds. The probe's caller-buffer/control-loop workaround demonstrates a path without changing these laws.

### FFI, ownership, and native bridges

**Applies to:** Rust/C/C++ bindings, callbacks, links, opaque resources, and host services.

**Ratified/current:** Rust externs with version pins and by-value data, C generated bindings and overlays, `link c@system/vendor`, safe callback constraints, `#FFI` raw bodies, full-depth C++, unified `<lang>.<lib>` naming, an FFI authority root, and exact-once `#Close`. `&`/`^` and `#Close` are the ownership boundary.

**Shipped:** FFI examples, binding generation, and markers.

**Open and constrained:** opaque C handle contract (`E3208`), native link closure (`E3210`), FFI authority (`E1803`), integer-pointer forms, and ABI versioning. There is no second ownership/close model, parallel extern-C grammar, or safe raw-pointer escape. The scalar zlib adapter and host bridge are valid alternatives for reachable functionality; they do not make opaque handles or transitive links native.

### Capabilities and loaded code

**Applies to:** effects, authority attenuation, plugins, sandboxed guests, and resource grants.

**Ratified/current:** closed roots are `Net`, `FS`, `IO`, `DB`, `Time`, `Rand`, `Env`, `Exec`, `Log`, `GPU`, `FFI`, `Browser`, and `Secret`; `Mem` and `Panic` are deny-only. Dotted rights form one subsuming tree. Effect declarations, one rights tree, `Authority` names/scopes/manifest/gate, and target sandbox replace a separate plugin authority model.

**Shipped:** package authority and filesystem scoping, plus scalar-only WIT/wasm sandbox behavior with zero imports (`E1257`–`E1260`).

**Open and constrained:** sandbox rules, canonical/no-follow/symlink policy, import-set and grant syntax/precedence, WIT ABI versioning, and resource grants. `D-PLUGIN1` and `D-DEP-WASM1` remain specification-only. There is no second rights namespace, new root, flat-root shortcut, `Mem`/`Panic` grant, arbitrary effectful guest, or rich unsafe guest export. Scalar sandbox plus host-side attenuation is the supported workaround.

### Lifecycle and cancellation

**Applies to:** tasks, streams, process pipelines, request shutdown, deadlines, and Ctrl-C.

**Ratified/current:** cancellation at wait points, shield and deadline, stream-drop cancellation, channels, structured tasks, scheduled jobs, typed `process.run`, explicit-stop cleanup, and `on_interrupt` for Ctrl-C/SIGINT.

**Shipped:** process specifications, pipelines, cleanup examples, and HTTP graceful drain.

**Open and constrained:** SIGTERM handling, `ProcessStdin.close`/EOF, request-timeout middleware, and typed interprocess channels. CPU work is not preempted; `on_interrupt` currently covers SIGINT only; child timeout is not request timeout; a channel is not typed IPC; SIGKILL skips cleanup; `#Every` is not a signal or timeout. A supervisor, a fixed consumer, or a native bridge is an evidence-backed alternative where the typed surface is absent.

### Type-level values, operators, and extensibility

**Applies to:** generics, arrays, dimensions, numeric operators, literals, modules, bounds, macros, and coherence.

**Ratified/current:** `Type<Args>`, `call<T>` without Rust turbofish, bounds without `where`, no HKT, `[T#N]` fixed stack arrays, generic module type/value parameters, closed Tier-0 type-level values with only layout `[T#capacity]`, same-type `#Numeric`, same-operand/fixed-symbol `D-OPDEF1`, `#UnitFamily`, dimensions and suffixes, checked literal carrier/head, one `@` compile marker/effect model, and one declaration registration table.

**Shipped:** fixed arrays, generic examples, units, comptime, and operators.

**Open and constrained:** general type-level/const values, heterogeneous operators, phantom types, coherence/orphan details, library literals/suffixes, and macro-like facilities. `D-EXT1` rejects proc macros and grammar extensions; unrestricted const generics, new Numeric widening, prefixes, and a second declaration table are not alternatives. The accepted orphan-rule workaround is a newtype. Dimension arithmetic is not a gap because `#UnitFamily` already covers it. A recursive data structure or explicit symbolic representation is the supported alternative to a macro or unnameable lowering.

### Views, memory, and ownership

**Applies to:** slices, text/bytes views, tensors, pinning, sharing, return values, and zero-copy APIs.

**Ratified/current:** an unmarked read, `&T` exclusive write, `^T` take, and `~` owned copy; named `View<T>` and `ViewMut<T>`; bare checked read windows; no resize/move while a view lives; provenance across returns/fields; one mutable view; read-view to a non-view is an owned copy; `ViewMut` to a non-view is an error; `Pin` tracks address stability; `D-CONC-SHARE1`; and `View` is the sole borrowed strided type.

**Shipped:** ranges/views, local text zero-copy and named-view provenance, arenas, pinning, and tensor `Arc<Vec<f64>>`; crossing boundaries rejects views and `core.files` has no mmap surface.

**Open and constrained:** explicit text/bytes zero-copy APIs, `core.files.mmap`, a complete strided host/device contract, and dtype/owner forms beyond `f64`. There is no raw-borrow/lifetime syntax, zero-copy assignment into a non-view, second view mechanism, arena escape, sending a view, or second tensor borrow model. Copying, pinning, an owned buffer, or a host FFI is the valid alternative when the native view is unavailable.

### Tensor and compute

**Applies to:** rank/shape storage, dtypes, placement, kernels, gradients, FFT/matmul, and device providers.

**Ratified/current:** one `core.compute` family; `Tensor<T>` ranked storage with static/runtime shape checks; `View` as the sole borrow; `Vec`/`Matrix` substrate; Auto for beginners; explicit placement receipts with no silent fallback; a conservative safe `#Kernel` subset; gradient/VJP contracts; default `F32Strict` plus `Reproducible` and a CPU oracle; raw-device providers under `#Unsafe`; and `Complex` as a type/suffix.

**Shipped:** core compute operations, `compute.set`, autodiff examples, naive `O(n^2)` `compute.fft`, matmul tile/kernel examples, runtime shape/stride checks, `Arc<Vec<f64>>`, and scalar `JetComplex`. There is no `core.image` implementation.

**Open and constrained:** public dtype storage policy, complex tensors, FFT complexity/provider guarantees, matmul guarantees, labeled dimensions, image API, and autodiff tier/provider support. There is one Tensor and one View; no silent fallback, old parallel gradient name, or raw-device token outside a provider. The CPU oracle, explicit receipt, manual chain rule, or external index are evidence-backed alternatives, not replacements for the open contracts. Tensor dtype status is an implementation/reconciliation issue, not a new ballot.

### UI, GUI, and platform services

**Applies to:** reactive UI, event routing, layout, accessibility, native/mobile services, and packaging.

**Ratified/current:** UI tree dot variants (ratified but not built), `ui.mount`, path identity with O(1) slot dispatch, a typed event family, reactive boxes, layout facts/reflection, case law, native/mobile target protocols, `Signal`/`Computed`/`Effect`/`Style`/`Layout`/`Clock`/native FFI, and an accessibility lint gate.

**Shipped:** callable `core.ui` (`ui.text`, `ui.button`, `ui.box`, `ui.mount`), null/TUI backends, reactive/event/mount/role behavior, the headless/TUI note app, layout C, and web/native entry points.

**Open and constrained:** dialogs; clipboard/IME/drag; fonts/shaping; shortcuts; accessible names; desktop bundling; reactive ICEs; native targets; and complete component/layout contracts. Callable UI is the current surface; dot construction is not falsely reported as shipped. Mobile direction is ratified but implementation is `E3302`; portable core click is intentionally small; there are no CSS selectors; open reactive/layout/component records are not probe outcomes. Host FFI or an external release tool is the supported alternative. Accessibility lint is not the same as typed accessible names.

### Embedded targets and low-level audit

**Applies to:** freestanding profiles, target memory/linker facts, C layout, raw memory, MMIO, pinning, interrupts, DMA, WCET, flash/OTA, device transport, and audit receipts.

**Ratified/current:** typed embedded/freestanding profile modules are selected through `targets:`; hosted single-file programs need no target ceremony. A selected profile may expose memory, linker, allocator, panic, volatile/MMIO, and audit controls. Named memory regions carry origin, size, access, and kind with typed sizes; validation catches overflow, overlap, and MMIO mistakes before code generation. Linker input comes from typed profile facts; freestanding allocator/panic are required facts; `jet inspect dossier target` is the canonical human/machine audit view and builds write a stable JSON artifact. Packages use `targets:` rather than `kind:`; target kinds are library/executable/test/example/benchmark, and omitted targets infer from `fn run()` with duplicate-entry diagnostics.

`#Layout(c)` stamps C representation, preserves field order, and rejects growable fields. `use core.mem` is the discovery gate. `#Unsafe(\"reason\")` is the audit gate for raw-pointer construction/dereference, volatile/MMIO, pointer arithmetic, and foreign-pointer crossings; reasonless forms are `E3112`. Optional `valid_ptr`, `aligned`, and `no_alias` obligations become required when policy selects them, attach to the immediately preceding operation, and appear in `jet inspect unsafe`. MMIO uses `mem.volatile_write(ptr, value)` paired with `mem.volatile_read(ptr)`; pointer-assignment lvalue syntax is not added. `mem.pin(&place) -> Pin<T>` is a tracked write window: safe code may read/edit but cannot move, replace, or resize the place while live, and field projections retain the pin/view distinction. Dev-tier raw accesses carry allocation/liveness/alignment sentries; `contain`/`harden` fence dependencies; hardened builds retain sentries and foreign-dependency containment. Source closure is semantic authority; a target artifact is only an optional cache projection carrying the full identity key, with same-source recompilation on a miss. Pure calendar/Duration operations need no provider, while wall, monotonic, zone-data, and sleep services are separate declared facts. FFI `&` is exclusive for one call, `^` transfers ownership, and `#Close(fn)` joins consuming close; a parallel `extern c` block is not current grammar.

**Shipped:** target and freestanding examples show hosted code without target ceremony and Core-only freestanding intent. `#Layout(c)`, `mem.address_of`, `mem.volatile_read`, `mem.volatile_write`, `mem.pin`, `Pin<T>`, and `jet inspect unsafe` are registered. `core.sys.on_interrupt` is a separate process-lifetime hook with no direct AOT/JIT route. The embedded-shaped host package checked and natively built C-layout GPIO/timer/UART/DMA records, bounded queues/rings, pinning, signing, image-slot policy, and replay; no physical board was used. The existing hardening/sentry laws provide an auditable raw-memory path, not target admission, interrupt vectors, DMA mapping, WCET, flash, or device transport.

**Open questions:** a usable reproducible `thumbv7em-none-eabihf` profile with startup/linker/provider facts and a build receipt; typed Cortex-M vector tables and ISR-to-task handoff with priority/nesting/overflow/cancellation rules; target-aware WCET or hard-deadline contracts with static or measured evidence; typed DMA mapping and ownership proving lifetime, addressability, cache/coherency, and completion; target-backed typed MMIO register blocks with widths/access modes/generated obligations; target-backed flash and authenticated A/B OTA state with confirmation, rollback, power-loss, and key authority; and a typed `jet flash`/hardware debug receipt containing target, image, programmer, observation, and failure identity.

**Conflicts and limits:** the existing target-profile decisions and the `E3302` missing `thumbv7em` component must not produce a second profile model. `D-REPRC1` already fixes C layout; S58/`D-UNSAFE2`/`D-FLAGSHIP-MMIO1` already fix the import gate, reasoned unsafe, volatile helpers, and no pointer-assignment spelling. `mem.pin` proves address stability only; the probe's `owner`, `submitted`, and `completed` fields are user protocol, not DMA, cache, device-ownership, or completion proof. `core.sys.on_interrupt` is not a vector/ISR API. Sentries and hardened containment are not WCET or target certification, and a manual `wcet_estimate_us` comparison is not compiler WCET evidence. Flash/OTA artifacts cannot become a second semantic source or artifact identity system. Release crypto and in-memory slot policy are library evidence, not persistent flash, secure boot, rollback, or device-programming evidence. See `results/area-embedded.md`, `.gaps.json`, and `.batteries.json`.

### Tool seams and compiler facts

**Applies to:** compiler inspection, build graphs, receipts, generated modules, plugins, and in-process tooling.

**Ratified/current:** compile-time compiler API with a JSON CLI mirror; graph inspect/explain-build; build units; no-change/session/store/prebuilt/bench/UI/context facts; target/action/toolchain/probe/cache/remote/schedule/legacy/plugin facts; DSL/gen restrictions; and TIR as the code-generation seam.

**Shipped:** compiler inspect, graph/explain, build-hook probe, replay/rename, and store operations.

**Open and constrained:** runtime compiler bridge (`E0956`), stable in-process graph/receipt diff, idempotent generated modules (`E3510`), graph diff, output identity, and plugin observation hooks. There is one graph/store/receipt model, no daemon, no undeclared fact, and no TIR inference. The JSON CLI and compile-time invocation are valid alternatives; they do not close runtime API work.

### HTTP and web architecture

**Applies to:** routes, JSON/messages, forms/query, static assets, CORS, web graph, DB drivers, live query, effects, reload, release, and devtools.

**Ratified/current:** URL/MIME and HTTP depth, route syntax, JSON/messages/static/CORS/web serving/error wire, DB driver/live query/effects, and the e14 query/forms/table/virtual/store suite with web graph, entry/reload/release/devtools/jobs.

**Shipped:** `core.http`, typed JSON/raw paths, static/CORS/router/shutdown/pooling/HTTP2, and the backend/web probe behavior.

**Open and constrained:** typed query and form decoding, bounded DB pool, route/OpenAPI metadata, request-timeout cancellation, signal handling, compression/access logs, and default evaluator gaps. `D-DX-SUITE1` is product-level evidence for the web gaps; `D-DX-WEBARCH1` records route facts but not OpenAPI. `D-HTTPDEPTH1` has a timeout concept but the server wrapper is open; `D-HTTPLIB1/3/4` are spec-only. Do not reopen route syntax, JSON/message choices, or the DB driver. Manual query/form decoding, one connection/app pooling, hand-written OpenAPI, and client/supervisor timeouts are the observed alternatives.

### Receipts and replay

**Applies to:** local evidence, status, execution records, replay, performance traces, and source closure.

**Ratified/current:** one local receipt store; read-only status; one evidence report and rendered ledger; proof/replay command contracts; fixed `.jet<kind>` artifacts; record/replay; perf `.jettrace`; and source-closure cache projection.

**Shipped:** ReceiptStore v2 fixed claim/status/stdout/stderr/digest, status, run record/replay, JSON prove, and perf compare.

**Open and constrained:** extensible typed facets, receipt query/diff, replay of ambient time, source names, a domain-evidence facet, and reverse stepping. Fixed codec/artifacts and one schema/store/lens remain the law; G2/G3 are defects; reverse replay is not shipped. A typed record plus CLI evidence is a valid workaround while native facets/query/diff remain open.

### Time and calendars

**Applies to:** durations, instants, clocks, zones, leap/calendar behavior, scheduling, and replay.

**Ratified/current:** signed `i64` nanosecond `Duration`, `DurationUnit`, checked type-owned constructors, one canonical Time family, `.in(.Unit)`, distinct `Instant`/`DateTime`/`LocalDate`/`LocalTime`/`Duration`/`Zone`, pure calendars without an ambient provider, injected `Clock`, checked DateTime literals (`E0155`), week suffix only, months/years as `Period`, and `D-TIMETRAVEL2` B-jump with a recorder and byte-identical reverse snapshots.

**Shipped:** constructors, conversions and truncating/fractional totals, core conversions/zone/monotonic/clock, normalized seconds+nanos DateTime, leap-second rejection, ambient-now denial (`E3403`), and broad time-probe behavior. `time.sleep` truncates nanos to milliseconds.

**Open and constrained:** `D-TIMEDEPTH1` (leap/calendar/zone evolution), richer TAI/TT/UT1 scales, TZDB provenance/version, Instant precision, scheduler missed-deadline facts, an implemented replay clock adapter, and `total_in` versus `in` reconciliation. Duration is not Period; there is no second duration carrier; leap `:60` rejection is intentional; `Clock.system` must be explicit; travel/replay is ratified direction, not shipped behavior. A business calendar, DST schedule, or own astronomy table is a valid library experiment while the provider contracts remain open.

## Probe matrix

Every row points to its complete report and machine-readable payload in the preserved audit directory. “Alternative” means the path used to answer the question without pretending that a missing native surface shipped.

| Probe | Question and applicability | Observed result | Constraints and alternative |
| --- | --- | --- | --- |
| `prim-numerics-perf` | Can shipped numeric primitives meet a useful speed band, and are slow or undiscoverable primitives the language or library boundary? Applies to FFT, matmul, CSR, stencil, and exact checksums. | Plain Jet covered FFT/matmul/CSR/stencil. Release FFT and matmul measured about 105 µs versus 117 µs and 341 ms versus 344 ms for the comparison implementation. Shipped `compute.fft` is `O(n^2)`; `pi` hit `E0956`/ICE; a 4096 map did not finish (`#2778`). | Preserve both parity evidence and slow-path evidence. Native release is an available alternative, not proof that the evaluator path is acceptable. See `results/prim-numerics-perf.md` and `.gaps.json`.
| `prim-exact-numerics` | Do Rational, Decimal, Money, operators, JSON, comptime, generic helpers, and a million-value sum provide library-quality exact arithmetic? | Rational/Decimal/Money paths and the large sum were exercised. Comptime division produced the wrong `10/3 -> 0/1` result in AOT; a helper in a trait hit `E0403`; literal/generic edges remain in the payload. | Keep exact diagnostics and numeric outputs. A runtime/library implementation can work around comptime or trait edges but does not repair them. See `results/prim-exact-numerics.md` and `.gaps.json`.
| `prim-units` | Does dimensional analysis feel like a usable language/library boundary, and where does operator ceremony exceed peer languages? | `#UnitFamily` dimension arithmetic works. Heterogeneous operators hit `E0907`/`E0360` and are covered by `OPMIX`; closed const-generic policy and the orphan rule were confirmed. | Use same-type operators or explicit conversions; use a newtype for foreign implementations. Dimension arithmetic is not reopened as a gap. See `results/prim-units.md` and `.gaps.json`.
| `prim-receipts` | Can a library extend the receipt model, and can users perform typed replay and diff without making the CLI the only path? Applies to regression evidence, JSON, static time, debugger names, and perf traces. | Typed regression record/JSON and pure replay work. Typed facets/query/diff are absent; replaying ambient time hit `E0956`; generated debugger names are not stable; perf evidence requires `.jettrace`. | Use typed records and the CLI receipt commands. This preserves evidence while the single-store/facet/query contracts remain open. See `results/prim-receipts.md` and `.gaps.json`.
| `prim-capabilities` | Can authority be attenuated for loaded code, and what plugin/sandbox ABI exists today? | Pure scalar sandbox builds; host filesystem attenuation works. Effectful guests hit `E1258`, rich exports `E1260`, resource-scoped FS grants `E1803`, and Net authority `E3304`. `D-PLUGIN1`/`D-DEP-WASM1` remain open. | Keep guests scalar-only with host-side attenuation, or use a host FFI boundary. Do not infer a rich plugin ABI from a scalar probe. See `results/prim-capabilities.md` and `.gaps.json`.
| `prim-distributed` | Can the runtime express parallel iteration, bounded typed channels, TCP/process exchange, finite streams, checkpoints, keyed/event-time work, and restart/replay? | Parallel map, bounded channels, TCP process exchange, finite `Stream`, and checkpoint replay work. Keyed/event-time/watermark/restart contracts are absent. | Library code can provide keyed/event-time logic and typed IPC-shaped protocols; this is a workaround, not a runtime guarantee. See `results/prim-distributed.md` and `.gaps.json`.
| `prim-bridges` | How much FFI boilerplate is needed, how are safety and ownership expressed, and which formats are reachable? | A scalar zlib adapter binds and runs. Opaque handles hit `E3208`, transitive links `E3210`, `process.run` FFI `E0956`, C++ generated structs `E0320`, and authority `E1803`. The adapter measured 34 lines including links. | Keep adapters small and explicit; use by-value/generated bindings or host process boundaries. Opaque handles and link closure remain open. See `results/prim-bridges.md` and `.gaps.json`.
| `prim-storage` | Are fsync, atomic rename, locking, crash recovery, local logs, KV, pools, queues, caches, and mmap available with durable semantics? | Fsync/atomic write/locks/crash recovery/local log/KV/pool/queue/cache work. Mmap is absent; imported fallible storage exposed a native defect. | Use explicit local storage and a native/library adapter for mmap or fallible imported code. Do not call a cache or queue a durability proof without the recorded sequence. See `results/prim-storage.md` and `.gaps.json`.
| `prim-dsl` | Which of pattern matching, comptime, reflection, literals, symbolic rules, formulas, and macros can be expressed, and what is the honest replacement? | Symbolic/rules/filter/formula experiments work with explicit structures. Recursive enums hit `E0112`/`E0401`; named-payload lowering ICEs; map assignment has a defect. Macros are rejected by design (`D-EXT1`). | Use explicit tagged records/structures and comptime where shipped. Do not add grammar/proc-macro machinery to solve a source-shape problem. See `results/prim-dsl.md` and `.gaps.json`.
| `prim-realtime` | Can allocation, blocking, deadlines, fixed callbacks, audio, shared state, and control loops be expressed with evidence of realtime behavior? | Fixed caller buffers, a control loop, a fake callback, and a package-native build work. There is no `core.audio`/callback API; sleep truncates milliseconds; shared paths can lock/block; CPU deadline is not WCET evidence. | Use caller-owned buffers, a native callback bridge, and external WCET measurement. Preserve the distinction between `FrameTime`, deadline, and WCET. See `results/prim-realtime.md` and `.gaps.json`.
| `prim-tooling-hooks` | Can a program inspect compiler facts/build graphs and use replay/rename/load hooks, and is there a stable runtime API for tooling? | Compiler lex/parse/check, graph/explain, replay/rename, and the load runner work (load needs `--allow-net`). Runtime compiler facts hit `E0956`; in-process graph/receipt diff is absent; repeated generated modules hit `E3510`; there is no daemon. | Use the compile-time API or JSON CLI mirror. Generated-module idempotence and runtime bridge remain owning work. See `results/prim-tooling-hooks.md` and `.gaps.json`.
| `prim-arrays` | Do views, strides, dtype, labels, lazy plans, image-shaped data, broadcasting, transpose/map, and AOT paths compose without unsafe or compiler gaps? | Core Tensor shape/broadcast/transpose/map/default paths, labeled-array and image prototypes, and the storage experiments are recorded. Tensor-bearing records hit TIR ICE; `compute.set` has an AOT defect; labels/lazy/image/dtype edges remain. | Keep Tensor shape checks and explicit bindings; use external image/columnar adapters while dtype/image/lazy contracts are open. See `results/prim-arrays.md` and `.gaps.json`.
| `prim-text` | Are Unicode normalization/segmentation, regex, streaming, byte/text views, BPE, and shaping reachable with usable ownership? | Unicode normalization/segmentation/regex/streaming and a HarfBuzz process path work. BPE requires boilerplate and is slow; text views/copy/span and direct shaping handles remain limited; regex lookaround is a design refusal. | Use named views/owned copies, a process or FFI shaping bridge, and explicit tokenization. Do not reintroduce lookaround or raw-borrow syntax. See `results/prim-text.md` and `.gaps.json`.
| `prim-time` | Can leap tables, zones, calendars, high-resolution boundaries, and astronomy scales be implemented with explicit clocks and reproducible results? | BusinessCalendar/holiday, DST schedule, owned astronomy scales, and Julian calculations work. Leap-second instant support is absent; a nanosecond boundary is wrong in the native path; astronomy tables require boilerplate; a generic native build ICEs. | Use explicit `Clock`, `Period`, business-calendar tables, and a native/library adapter for high-resolution or astronomy data. See `results/prim-time.md` and `.gaps.json`.
| `area-web` | Can a realistic package express routes, HTML, SQLite, sessions, static assets, browser/WASM, watchers, typed query, typed forms, table/virtual/store, and live state? | Routes/HTML/SQLite/session/static/browser/WASM/watcher paths work. Native server is an ICE (`#2762`); typed query is missing (`E0102`); form decoder is missing (`E0405`) and needs a manual parser. | Manual query/form decoding is the current workaround. G1 remains a defect; e14 cards `#2472`/`#2474` own typed query/forms. See `results/area-web.md`, `.gaps.json`, and `.batteries.json`.
| `area-games` | Can a small game express scenes, input, fixed-step update, AABB collision, save/load, audio, assets, frame budget, and headless replay? | Package/scene/keyboard/fixed-step/AABB/save, headless three-frame transcript, and AOT work. Frame budget is `E0764`; gamepad and atlas/draw are `E1004`; core audio is missing (`E1001`); custom bridge is `E0956`. | Use a pure loop, keyboard input, rectangles, one-shot sound, or a native bridge. Do not claim audio, assets, or gamepad completion from the headless battery. See `results/area-games.md`, `.gaps.json`, and `.batteries.json`.
| `area-cli` | Can a typed CLI/files/env/regex/records/`#Job`/script/release flow replace a ten-flag object pipeline, including closable stdin and ignore-aware walking? | Typed CLI, files, environment, regex, records, jobs, scripts, release, and a native pipeline work. `ProcessStdin.close` is missing (`E0102`); there is no ignore-walk API; typed object adapters are a call-site gap. | Use a fixed consumer/native pipeline, a manual ignore subset, and explicit decoding. See `results/area-cli.md`, `.gaps.json`, and `.batteries.json`.
| `area-data` | Can a data workflow cover 100k CSV, table filter/group/pivot/window/stats/OLS, joins, labeled arrays, plotting PNG/SVG, Parquet, Tensor records, and notebook invalidation? | CSV/table/filter/group/pivot/window/stats/OLS/manual join/labeled arrays/SVG/notebook invalidation work. Parquet and PNG are `E1004`; default `inner_join` is `E0956`; Tensor-bearing records hit a TIR ICE. | Use CSV/JSON or a library codec, SVG, a nested-loop join, and separate Tensor bindings. See `results/area-data.md`, `.gaps.json`, and `.batteries.json`.
| `area-backend` | Can a small backend cover typed JSON/SQLite, queues, auth/sessions/logs, load p99, bounded pool, OpenAPI, timeout, signal, graceful drain, and native release? | Typed JSON/SQLite/queue/auth/logs/load and graceful drain/native release work; measured load p99 was about 3 ms. `core.db.pool` is missing (`E1004`), `core.openapi` is missing (`E1001`), server timeout and signal are missing (`E1004`), and default `set_trace_id` is `E0956`. | Use one connection/app pooling, hand-written OpenAPI, client/manual timeout, a supervisor or unsafe signal bridge, and release-tier code. See `results/area-backend.md`, `.gaps.json`, and `.batteries.json`.
| `area-ai` | Can a small AI workload cover Tensor, broadcast, manual/autodiff training, save/restore, SSE, GPU/Vulkan placement, Dataset/Loader, and a 10k embedding build? | Tensor/manual chain rule/loss training (about 0.5 to .0037), save/restore, SSE, and explicit Vulkan receipt work. `compute.gradient` hits `E0956`/native ICE; Dataset/Loader needs author boilerplate; a 10k embedding build exceeded 360 s. Python/NumPy/PyTorch comparison was unavailable because `python3` was absent. | Use manual chain rules, small shuffle/batch helpers, and an external index or smaller corpus. No unavailable peer benchmark is inferred. See `results/area-ai.md`, `.gaps.json`, and `.batteries.json`.
| `area-gui` | Can a useful GUI/TUI app cover Unicode, reactive state, roles/focus/themes/undo, dialogs, clipboard/IME/drag, fonts/shaping, shortcuts, accessibility, packaging, and native/mobile targets? | Headless/TUI reactive note app, roles/focus/themes/undo, and release flow work. Android/iOS are `E3302`; dialogs/clipboard/IME/drag/fonts/shaping/shortcuts are `E1004`; accessible names are incomplete; `jet package` is `E2101`; default reactive lowering has `E0956` and ICEs `E0382`/`E0277`/`E0308`; GTK bridge is `E3201`. | Use host FFI or external services/tools for missing platform surfaces. Keep callable UI and TUI evidence separate from ratified-but-unbuilt dot construction. See `results/area-gui.md`, `.gaps.json`, and `.batteries.json`.
| `area-embedded` | Can host-shaped board code express C layout, rings, pinning, signing, slots, replay, interrupts, DMA, WCET, flash/OTA, and MMIO on a Cortex-M target? | Host board records, C layout, rings, pinning, signing, slots, replay, and native build work. The target is `E3302`; interrupts, DMA, WCET, flash/OTA are `E1004`; `jet flash` is `E2101`; MMIO remains manual unsafe work. | Install/use the target, external startup/FFI, manual owner flags/protocol, external WCET, bootloader/updater, probe-rs, and manual records. Host-shaped success is not Cortex-M qualification. See `results/area-embedded.md`, `.gaps.json`, and `.batteries.json`.

## Campaign decisions and proof ownership

The historical ballot source remains executable only as source history at `tools/agent-eval/domain-foundations/slate/ballots.mjs`. Its 11 ballots are `D-FOUND-REALTIME1`, `D-FOUND-HANDLE1`, `D-FOUND-SANDBOX1`, `D-FOUND-LIFECYCLE1`, `D-FOUND-OPMIX1`, `D-FOUND-LITERAL1`, `D-FOUND-VIEW1`, `D-FOUND-RECEIPT1`, `D-FOUND-PLATFORM1`, `D-FOUND-BOARD1`, and `D-FOUND-COREAPI1`; their campaign cards are `#2784`–`#2794`. The campaign also recorded defect cards `#2762`–`#2783`, battery cards `#2795`–`#2801` and `#2815`, example cards `#2802`–`#2814`, and core-API cards `#2847`–`#2857`. The campaign wave card was `#2761`.

The master report and the gap payloads remain the evidence for card ownership. In particular:

- `REALTIME` closes the realtime primitive gaps and game/embedded fixed-callback gaps only at the ratified contract level; implementation and proof cards remain where the reports show missing callbacks, audio, WCET, or target support.
- `HANDLE` closes the bridge/text/game/GUI/data handle boundary questions only where the shipped view and ownership rules pass; mmap, device views, and richer dtype ownership remain open.
- `SANDBOX` owns the capability boundary; scalar sandbox evidence does not close rich guest/plugin ABI work.
- `LIFECYCLE` owns cancellation/deadline/cleanup; SIGTERM, EOF, request middleware, and typed IPC remain proof/implementation work.
- `OPMIX` owns the same-operand operator rule and its unit consequence; heterogeneous operator support is an amendment, not an accidental compiler gap.
- `LITERAL` owns exact numeric literal/comptime semantics; the observed wrong comptime division remains a defect.
- `VIEW` owns view spelling/provenance/copy behavior; explicit text/bytes and mmap contracts remain open.
- `RECEIPT` owns the one-store evidence and replay boundary; facets, query/diff, ambient-time replay, and reverse stepping remain open.
- `PLATFORM` owns native/mobile service direction; the current GUI service errors and packaging failure remain proof work.
- `BOARD` owns target/interrupt/DMA/flash direction; host-shaped board evidence is not target proof.
- `COREAPI` owns the named standard-library surfaces; each area report retains the exact missing API and diagnostic.

No Tower transfer or card closure was performed by this document cleanup. Main owns any Tower transfer, implementation-card update, and final proof. The remaining proof must use a fresh binary and the exact runnable paths in the preserved reports; ratified decisions alone are not green evidence.

## Retirement and recovery map

The following working notes are retired from the active research tree because their unique content is now in this note, while their final evidence is preserved separately:

- `docs/research/domain-foundations/law/BRIEF.md` → this note's **Reproducible method**, **Cross-cutting law guardrails**, and **Evidence layout** sections.
- `docs/research/domain-foundations/law/realtime.md` → **Realtime and scheduling**.
- `docs/research/domain-foundations/law/ffi.md` → **FFI, ownership, and native bridges**.
- `docs/research/domain-foundations/law/capabilities.md` → **Capabilities and loaded code**.
- `docs/research/domain-foundations/law/lifecycle.md` → **Lifecycle and cancellation**.
- `docs/research/domain-foundations/law/typelevel.md` → **Type-level values, operators, and extensibility**.
- `docs/research/domain-foundations/law/views.md` → **Views, memory, and ownership**.
- `docs/research/domain-foundations/law/tensor.md` → **Tensor and compute**.
- `docs/research/domain-foundations/law/receipts.md` → **Receipts and replay**.
- `docs/research/domain-foundations/law/toolseams.md` → **Tool seams and compiler facts**.
- `docs/research/domain-foundations/law/gui.md` → **UI, GUI, and platform services**.
- `docs/research/domain-foundations/law/embedded.md` → the embedded-specific constraints in **Cross-cutting law guardrails** and the `area-embedded` matrix row.
- `docs/research/domain-foundations/law/time.md` → **Time and calendars**.
- `docs/research/domain-foundations/law/http.md` → **HTTP and web architecture**.

The 22 probe briefs and `COMMON.md` are retired because their questions and execution rules are retained above and their final outputs are preserved in `results/`:

- Primitive briefs: `prim-numerics-perf`, `prim-exact-numerics`, `prim-units`, `prim-receipts`, `prim-capabilities`, `prim-distributed`, `prim-bridges`, `prim-storage`, `prim-dsl`, `prim-realtime`, `prim-tooling-hooks`, `prim-arrays`, `prim-text`, and `prim-time`.
- Area briefs: `area-web`, `area-games`, `area-cli`, `area-data`, `area-backend`, `area-ai`, `area-gui`, and `area-embedded`.
- Shared brief: `COMMON.md`.

For exact recovery of any retired file, use the source tag and its original path, for example:

```sh
git show cleanup-source-2026-09-05-114107:docs/research/domain-foundations/law/realtime.md
git show cleanup-source-2026-09-05-114107:docs/research/domain-foundations/probes/area-web.md
git show cleanup-source-2026-09-05-114107:docs/research/domain-foundations/probes/COMMON.md
```

The old `docs/research/domain-foundations/` planner bucket is therefore not kept as an empty active archive. The maintained history note `docs/research/domain-foundations-history.md` is a separate current record and is intentionally not retired by this slice. Main owns updates to references outside this note.
