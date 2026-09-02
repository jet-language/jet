# Foundations for every domain — mining report and probe program (v0, 2026-09-02)

## Status

| Field | Value |
|---|---|
| State | Research in progress: mining done, probes prepared, probe wave not yet run |
| Epoch | e15 "Foundations for every domain" |
| Blocker | The Codex usage limit (Luna and Sol) is exhausted until 2026-09-06 22:29; owner guidance forbids substituting another model family for these lanes, so the 22-probe wave waits. Everything that is thinking is done below. |
| Ballots | None yet. Owner rule: one consolidated slate after all probes, 5-15 primitive ballots, recommended option A, every loss designed away. |
| Owner gates | None open |

## The one idea

Jet stays feature-rich at the foundation and gets out of the way above it. Core keeps four things: the language primitives a library cannot add for itself (syntax, the type system, memory and effects, execution tiers, the runtime); the ecosystem and developer-experience tools (build, packages, devtools, live loop, tests, receipts, diagnostics, editor support); the core library that ships today; and the shared data types two libraries must agree on (one table, one unit, one receipt, one tensor). A niche (acoustics, a ledger, an HDL flow, a media pipeline) is a library someone writes in Jet. Our job is to make sure that someone has every tool they need, and to find the places where they do not.

Aside (more precisely): "very good, not perfect" is the bar. An awkward but working, safe, fast-enough library build counts as buildable. A gap is only ever a primitive: impossible, unsafe without `#Unsafe`, slow without compiler help, ceremony at every call site, or heavy author boilerplate when it is very common or shared across domains.

## Where we were

The domain-matrix slate (epoch e15 before today) proposed 88 shared mechanisms as typed core APIs for 108 fields, with 46 ballots and 219 cards. The owner ratified 42 ballots and then withdrew the slate the same day: it would have made Jet the author of every niche library forever. The research behind it (5,698 census rows, 3,171 source-linked claims, 11 family syntheses, 46 worked ballots) is archived at `docs/research/domain-matrix-archive/` and is mined below rather than re-collected.

## Where we are: the 88 former mechanisms, re-judged

Each former mechanism gets one of five verdicts under the rule. "Evidence" names the probe that will confirm or overturn the judgment; "judgment" means the archive alone was enough and no probe is planned unless a critical-area probe trips over it.

### Primitives: core keeps or must add (15)

| Former mechanism | Fields served | Evidence | Why |
|---|---|---|---|
| GPU-KERNELS | 33 | area-ai | kernel placement, fusion, and tier parity need the compiler |
| NETWORKING | 31 | area-backend | sockets, TLS, HTTP, WebSocket are core; protocol libraries build on them |
| HARDWARE-IO | 15 | area-embedded | authority-safe device access touches the capability model and MMIO |
| EXACT-NUMERIC | 10 | prim-exact-numerics | BigInt is core; rational and decimal are library unless literals and operators cannot be made to feel native |
| AUTODIFF | 8 | area-ai | reverse-mode over arbitrary code needs the compiler for speed; a library tape is the fallback to measure |
| WCET | 8 | prim-realtime | timing evidence needs compiler and target cooperation |
| PLUGIN-CAPS | 7 | prim-capabilities | capability scoping of loaded code is a runtime property |
| OS-SERVICES | 5 | prim-capabilities | authority-bound services touch the runtime's capability model |
| GPU-GRAPHICS | 5 | area-games | portable graphics devices and shaders are runtime and target work |
| INTERRUPTS | 4 | area-embedded | interrupt vectors and bounded handoff need the runtime and target |
| REALTIME-STREAMS | 3 | prim-realtime | allocation-free deadline-checked callbacks need runtime and compiler guarantees |
| PLUGIN-ABI | 3 | prim-capabilities | a versioned ABI is a toolchain and runtime contract |
| SAMPLE-SAFETY | 2 | prim-capabilities | a capability boundary around untrusted input is runtime-enforced |
| TEXT-SHAPING | 2 | prim-text | font discovery and shaping need a platform bridge the runtime must own for GUI parity |
| DMA | 2 | area-embedded | DMA buffer ownership is a memory-model question |

### Shared data types: core owns the type (4)

| Former mechanism | Fields served | Evidence | Why |
|---|---|---|---|
| VALIDATION-RECEIPTS | 59 | prim-receipts | jet-receipt-v2 is shipped; the question is library extensibility |
| DATA-TABLES | 50 | area-data | core.data is shipped; libraries need one table type to hand around |
| LINALG | 37 | prim-numerics-perf | core.compute tensor is the shared substrate; operations beyond it are library or perf-gated |
| UNITS | 34 | prim-units | dimensional analysis lives in types; two libraries must share one unit type |

### Ecosystem tools: core owns the tool (10)

| Former mechanism | Fields served | Evidence | Why |
|---|---|---|---|
| WORKFLOW-DAG | 39 | area-cli | #Job and the build graph exist; inspectable DAGs are tooling |
| NOTEBOOK | 31 | area-data | jet notebook exists; the loop's gaps are tooling gaps |
| BOARD-BSP | 8 | area-embedded | board and target facts belong to the build |
| LOAD-RUNNER | 6 | prim-tooling-hooks | load testing is a test tool; .measure and receipts exist |
| SAFETY-EVIDENCE | 6 | prim-receipts | jet prove and receipts exist; traceability is tooling |
| FLASH-DEBUG | 4 | area-embedded | flash and debug receipts are toolchain work |
| HEADLESS-REPLAY | 3 | prim-tooling-hooks | deterministic replay is a devtool |
| CONTRACT-MUTATION | 2 | judgment | test tooling over #Test; library-buildable on top of the test runner |
| SYNTAX-TREE | 2 | prim-tooling-hooks | the compiler owns the lossless tree; tools need an exposed seam |
| REMOTE-CACHE | 2 | prim-tooling-hooks | build cache is toolchain work |

### Batteries: library code a critical area needs (10)

| Former mechanism | Fields served | Evidence | Why |
|---|---|---|---|
| PLOTTING | 32 | area-data | drawing to SVG/PNG is library code; data battery |
| API-CONTRACT | 9 | area-backend | OpenAPI export from typed routes; backend battery |
| DATALOADER | 5 | area-ai | batching and shuffling are library code; AI/ML battery |
| OBJECT-PIPELINE | 4 | area-cli | typed record pipelines are library code over iteration and processes; CLI battery |
| CORPUS-SEARCH | 3 | area-cli | ignore-aware search is library code; CLI battery |
| AUDIO-GRAPH | 2 | area-games | audio graphs are library code once the real-time callback primitive exists; games need one |
| CONNECTION-POOL | 2 | area-backend | library code; every service needs one |
| EDGE-BINDINGS | 2 | area-web | deployment bindings for web targets |
| LLM-CLIENT | 2 | area-ai | HTTP client with streaming and a tool loop; AI/ML battery |
| CANVAS-2D | 2 | area-games | window plus 2-D drawing; games and creative battery over the bridge |

### Libraries: example only (49)

| Former mechanism | Fields served | Evidence | Why |
|---|---|---|---|
| SCI-FORMATS | 58 | prim-bridges | formats are readers; bridge ergonomics decide whether HDF5/NetCDF/Parquet readers are library code |
| STORAGE | 28 | prim-storage | durable stores are library code if fsync, locks, and mmap are available |
| OPTIMIZATION | 27 | prim-dsl | a model DSL plus bridged solvers (HiGHS); the DSL-authoring primitives are the question |
| MPI-LAUNCH | 26 | prim-distributed | process launch and collectives over sockets; runtime support is the question |
| SIMULATION | 19 | judgment | simulation lifecycles are library code over time, receipts, and arrays |
| SPARSE-SOLVERS | 18 | prim-numerics-perf | CSR and Krylov are library code; speed band is the question |
| FFT | 15 | prim-numerics-perf | buildable; the primitive question is SIMD and layout for the speed band |
| SOLVER-SUBSTRATE | 14 | prim-numerics-perf | nonlinear solvers are library code |
| TYPED-MODEL-RESULT | 12 | area-data | typed fit results are records |
| DSP-FILTERS | 10 | prim-numerics-perf | filter design over arrays |
| ODE-DAE | 10 | judgment | adaptive integrators are library code over arrays |
| MESH-STENCILS | 10 | judgment | meshes and stencils are library code; speed band covered by the stencil kernel probe |
| EXPERIMENT-LEDGER | 10 | judgment | run and artifact ledgers over receipts and storage |
| DISTRIBUTIONS | 9 | judgment | samplers and densities over the numeric core |
| CALENDARS-BUSINESS-DATES | 8 | prim-time | calendars over core time |
| SYMBOLIC | 7 | prim-dsl | symbolic AST and rewriting is a library DSL |
| RULES-DECISIONS | 7 | prim-dsl | rules engines are library DSLs |
| QUERY-PLANNER | 7 | area-data | core.data query exists; analytical planning beyond it is library |
| CONNECTORS-WEBHOOKS | 7 | area-backend | HTTP clients with typed payloads |
| MODEL-STATE | 7 | area-ai | parameter registries and checkpoints are records plus storage |
| SCENE-GRAPH | 6 | area-games | scene trees are library code |
| NLP-PIPELINE | 5 | prim-text | tokenizers and annotations over strings |
| BUS-TXN | 5 | area-embedded | typed transactions over MMIO once ownership is settled |
| LABELED-ARRAYS | 4 | prim-arrays | names and coordinates over the shared tensor |
| WORKBOOK-FORMULAS | 4 | prim-dsl | formula graphs are a library DSL |
| BREP-GEOMETRY | 4 | prim-bridges | a kernel bridge (OpenCascade) plus library types |
| DOUBLE-ENTRY-LEDGER | 4 | prim-exact-numerics | ledgers are records over exact money |
| FINDINGS-RULES | 4 | judgment | rule packs and SARIF over the syntax tree seam |
| RASTER | 3 | prim-arrays | image values over the shared tensor |
| STRUCTURED-FILTER | 3 | prim-dsl | jq-style filters are a library DSL |
| STREAM-WINDOWS | 3 | prim-distributed | event-time windows over streams |
| RETRIEVAL-INDEX | 3 | area-ai | vector and lexical indexes are library code |
| SEMANTIC-LAYER | 3 | judgment | dashboard metrics are library code over tables and UI |
| MAP-RENDERING | 3 | judgment | tiles and rasters over canvas |
| GEOMETRY-CRS | 3 | judgment | geodesy is library math |
| CONTROL-MODELS | 3 | judgment | state-space models over linear algebra |
| TIMELINE | 3 | judgment | keyframes and time bases are records |
| MEDIA-PIPELINE | 3 | prim-bridges | FFmpeg bridge plus library pipeline |
| CHUNKED-LAZY-ARRAYS | 2 | prim-arrays | lazy plans over the shared tensor; views and strides are the primitive question |
| DURABLE-STREAMS | 2 | prim-storage | queues with consumer groups over the log |
| SPATIAL-INDEX | 2 | judgment | R-trees and cells are library code |
| BVH-ACCEL | 2 | judgment | acceleration structures are library code; speed band is the numerics question |
| COLOR | 2 | judgment | color math is library code |
| OTA | 2 | area-embedded | slots and rollback are library code over flash tooling |
| FIELDBUS | 2 | judgment | protocol models over bus transactions |
| ROBOT-BUS | 2 | judgment | message buses over networking |
| ORBIT-DYNAMICS | 2 | judgment | propagators over arrays and time |
| REFERENCE-FRAMES | 2 | prim-time | time scales over core time; leap-second data is the question |
| FHIR-HL7 | 2 | judgment | record models over serde and HTTP |

Counts: 15 primitives, 4 shared types, 10 ecosystem tools, 10 batteries, 49 libraries. Of the 88, only 29 can stay anywhere near core, and most of those already exist in some form (networking, receipts, tables, the tensor, notebook, jobs, prove). The question the probes answer is not "what to build" but "what stops a library author today".

## What the census already says (hypotheses, not findings)

Counting the 3,348 census rows the field surveys marked as real gaps by how many of the 108 fields raise them, the same needs recur far outside any one niche. These are the "two birds" candidates: one primitive would unblock many fields at once. Each is a hypothesis until a probe reproduces it in code.

| Recurring need | Fields raising it | What library authors keep asking for | Probe that tests it |
|---|---|---|---|
| File formats and interop | 99 | checked readers and writers with schema, metadata, provenance, and diagnostics; bridges to the incumbent C libraries (HDF5, NetCDF, Parquet, Arrow, FFmpeg, OpenCascade) that are safe by construction | prim-bridges |
| Parallel and distributed execution | 74 | bounded, explicit ownership of work: typed fan-out/fan-in, failure policy, cancellation, checkpoint and restart, per-task evidence | prim-distributed, area-backend |
| Build and package tooling | 64 | reproducible paths from a description (an API schema, a board, a plugin ABI) to checked bindings; versioned artifacts; capability-scoped loading | prim-tooling-hooks, prim-capabilities, area-embedded |
| Durable storage | 57 | explicit durability (fsync, locks, atomic rename, mmap), record identity, retention, ordered replay | prim-storage |
| Networking and protocols | 56 | typed endpoints with stable operation identity, request and response contracts, timeouts, retries, cancellation, lifecycle | area-backend, area-web |
| Devtools and inspection | 55 | exposed plans and facts: logical and physical plans, build graphs, syntax trees with spans, replay, load thresholds | prim-tooling-hooks |
| Tables and records | 48 | one table type with identity, missing values, schemas, deterministic queries; typed record collections with constraints | area-data |
| Plotting and visualization | 46 | deterministic drawing to SVG and PNG with publication export | area-data (battery) |
| Testing and validation | 45 | schema validation with paths and deterministic failures; contract replay; deadline and plausibility monitors | prim-tooling-hooks, area-embedded |
| Solvers and optimization | 41 | a model DSL plus bridged solvers; speed in the band from library code | prim-dsl, prim-numerics-perf |
| Simulation and reproducibility | 39 | seeds, deterministic stepping, receipts for a run, replay | prim-receipts, prim-realtime |
| Notebook and REPL | 38 | reactive cells, inspection of values and plans, restart-safe state | area-data |
| Tensors and ML | 38 | named dimensions, differentiable ops, layout conversion, GPU placement with receipts | area-ai, prim-arrays |
| Security and authority | 33 | explicit authority bound to records, fields, and operations; secrets handling; denial reasons | prim-capabilities |
| Hardware I/O | 30 | authority-safe device access, bounded transactions, interrupts, DMA ownership | area-embedded |
| Graphics and rendering | 26 | portable devices, shaders, frame resources, headless replay | area-games, area-gui |
| Geometry and meshes | 25 | exact kernels through bridges; library math above them | prim-bridges |
| Arrays and linear algebra | 24 | views, strides, dtype generics, lazy plans over one tensor | prim-arrays, prim-numerics-perf |
| Units and quantities | 23 | one unit type checked in the type system | prim-units |
| GPU | 17 | kernels, fusion, placement receipts | area-ai |
| DSP and signals | 10 | FFT and filters in the speed band from library code | prim-numerics-perf |
| Audio and real time | 8 | allocation-free deadline-checked callbacks | prim-realtime |

Reading across the rows, the words that recur in what authors ask for are the same everywhere: explicit, bounded, checked, identity, provenance, versioned, diagnostics, deterministic. That is the shape of the primitives we expect the probes to name: not "a signal library" but "a way to declare a bounded resource and get a diagnostic when it is exceeded", not "a format reader" but "a way to write a checked reader once and get the same diagnostics as core".

## Where we could be: the probe program

Twenty-two probes, each a Luna max worker with a 60-minute box, all read `docs/research/domain-foundations/probes/COMMON.md` and one brief in the same directory. Eight build a small real program end to end in one critical area; fourteen test one cross-cutting primitive from library code. Every probe returns `probe.md`, `gaps.json` (tagged by the rubric, each gap named as a capability with exact evidence), and, for areas, `batteries.json`.

| Probe | Builds | Informs |
|---|---|---|
| area-web | an Orders full-stack app: typed routes, island, form, query cache, SQLite, session auth, live reload, web and native builds, one e2e test | web |
| area-games | a 2-D game: fixed-timestep loop, input, sprite atlas, AABB physics, audio callback, scene graph, save file, headless 600-frame replay | games |
| area-cli | a notes tool: typed subcommands, ignore-aware search, process pipelines, typed object pipeline, #Job, script mode, release install | cli |
| area-data | a data session: CSV and Parquet into the core table, group/join/pivot/window, regression, labeled array, SVG/PNG plot, notebook, receipt; timing against pandas/polars | data |
| area-backend | a JSON API: typed bodies with OpenAPI export, pool, queue, auth, logs/metrics/traces, graceful shutdown, 1,000-VU load test, contract test | backend |
| area-ai | an AI app: tensors, a two-layer net with autodiff, data loader, checkpoints, embeddings index, streaming LLM call with a tool loop, GPU matmul receipt; timing against numpy/PyTorch | ai-ml |
| area-gui | a desktop notes app: window, menu, editor with real text shaping, shortcuts, dialogs, undo, themes, reactive settings, accessibility, packaging; mobile needs | gui |
| area-embedded | Cortex-M firmware: board, MMIO, interrupt handoff, DMA ownership, UART ring buffer, deadline loop with WCET, signed flash slot with rollback, flash/debug receipt | embedded |
| prim-numerics-perf | FFT, dense matmul, sparse SpMV, stencil in library Jet, timed against numpy/scipy at both tiers | data, ai-ml, games, science |
| prim-exact-numerics | Rational, Decimal, money over BigInt with literals and operators | data, backend |
| prim-units | a units layer as a library against the type system | embedded, science, games |
| prim-receipts | extending the shared receipt with a domain field group; replay and diff from library code | data, backend, embedded, ai-ml |
| prim-capabilities | loading a plugin with granted capabilities; a sandboxed untrusted file; an OS service under authority | backend, cli, gui, games |
| prim-distributed | parallel map, two-process typed messaging, backpressure pipeline, checkpointed windowed stream | backend, ai-ml, science |
| prim-bridges | a C handle API, a header via `jet inspect bind cpp`, a Rust crate via the extern bridge; boilerplate per foreign function | games, ai-ml, data, gui, science |
| prim-storage | append-only log with fsync and crash recovery, KV store, pool, durable queue, content cache; kill mid-write | backend, data, cli |
| prim-dsl | symbolic math, rules engine, jq-style filter, spreadsheet formulas as library DSLs | data, backend, science |
| prim-realtime | 48 kHz audio callback and a 1 kHz control loop with deadline and jitter reporting, allocation-free | games, embedded, gui |
| prim-tooling-hooks | syntax-tree rename, "what changed" over the build graph, headless replay, deterministic load runner | cli, web, backend |
| prim-arrays | labeled N-D, chunked lazy, and image arrays over the core tensor; timing against numpy/xarray | data, ai-ml, science |
| prim-text | BPE tokenizer, segmentation, 100 MB log scan, HarfBuzz shaping through a bridge | cli, gui, ai-ml, web |
| prim-time | business dates, DST schedules, astronomical time scales over core time | backend, data, science |

To run the wave once the quota resets: copy the briefs to the machine-local scratch and dispatch one OMP `task` per probe (bundled `task` maps to GPT-5.6 Luna max), with the context and acceptance text in `docs/research/domain-foundations/probes/probes.json` and the briefs; up to 22 concurrently under the owner's 30-lane allowance. The orchestrator then reads every `gaps.json`, deduplicates by primitive across probes (one primitive that unblocks several areas is one proposal), writes this report's findings section, and only then drafts the ballot slate.

## What has to be decided

Nothing yet. The owner ruled the frame, the rubric, the critical areas, the undo, the gate, the ballot law, and the output shape on 2026-09-02 (recorded in `docs/agents/agent-memory.md` under `domain-rescope-2026-09-02`). The next owner decisions are the primitive ballots themselves, which exist only after the probes produce evidence.

## Evidence index

| Path | Contents |
|---|---|
| `docs/research/domain-matrix-archive/` | the mined research: census, claims, performance rows, family syntheses, the 46 withdrawn ballots |
| `docs/research/domain-foundations/classification.json` | the 88 verdicts above with their probe assignments |
| `docs/research/domain-foundations/probes/` | COMMON.md, 22 briefs, probes.json |
| Tower e15 | milestone e15-m13 (substrate defects, 9 cards), card #2759 (ballot surface), card #2760 (ballot gate), card #2761 (the probe wave, blocked on the Codex quota until 2026-09-06 22:29) |
