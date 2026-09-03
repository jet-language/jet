# Foundations for every domain — findings, slate, and evidence (2026-09-02)

## Status

| Field | Value |
|---|---|
| State | Probe wave complete: 22 probes, 99 gaps, 23 root-cause defects, 11 ballots on the board, 55 cards homed in e15 |
| Epoch | e15 "Foundations for every domain" |
| Ballots | 11, all ratified A by the owner on 2026-09-03: D-FOUND-REALTIME1, HANDLE1, SANDBOX1, LIFECYCLE1, OPMIX1, LITERAL1, VIEW1, RECEIPT1, PLATFORM1, BOARD1, COREAPI1 (cards #2784-#2794) |
| Cards | 22 defect cards (#2762-#2783, e15-m13), 8 battery cards (#2795-#2801, #2815), 13 example cards (#2802-#2814), 11 primitive cards (#2784-#2794) |
| Owner gates | None open: all eleven ballots ratified A (2026-09-03); the eleven COREAPI1 items are carded (#2847-#2857). |
| Reading path | `docs/proposals/domain-foundations.html` (the story), then the ballots in Tower Focus Mode |

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

## What the census said before the probes (hypotheses; the findings above are the test)

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

## What the probes found

Twenty-two Luna max probes ran for 23 to 46 minutes each against the real `jet` binary (22 of 22 returned; every gap carries a command, its output, and a rubric tag). They wrote 99 gaps: 47 block, 49 hurt, 3 annoy; by tag, 52 impossible, 31 boilerplate, 24 defect, 19 call-site, 6 slow, 4 unsafe. The eight critical areas all reached a working first program on today's Jet; none reached its complete target.

| Area | Verdict, in the probe's words | Working today | Blocked by |
|---|---|---|---|
| Web | development flow buildable; native server build ends in an ICE | routes, HTML, SQLite, session auth, static assets, browser WASM build, watcher restart | #2762 (handler closure ICE); query and form typing under e14 #2472/#2474 |
| Games | buildable with listed gaps fixed | package, scene, input, fixed-step loop, AABB, save file, 3-frame headless transcript, AOT build | frame budget, gamepad, atlas (D-FOUND-COREAPI1); audio callback (D-FOUND-REALTIME1) |
| CLI and scripts | buildable today | typed #CLI with ten options, files, env overlays, regex search, records, #Job, script mode, release install | closable stdin and ignore-aware walk (D-FOUND-COREAPI1) |
| Data analysis | credible typed in-memory floor; not yet columnar or raster | 100k CSV in 10 s, filter/group/pivot/window, typed OLS, SVG, notebook invalidation | Parquet (D-FOUND-HANDLE1), PNG (battery), Tensor in a struct (#2769) |
| Backend services | buildable for a small single-process service; not production-complete | typed JSON routes, SQLite, durable queue, sessions, logs, 1000-task p99 3 ms, graceful drain, native release build | pool and OpenAPI (D-FOUND-COREAPI1), deadlines and SIGTERM (D-FOUND-LIFECYCLE1), #2782 |
| AI and ML | useful small app buildable; cross-tier autodiff missing | Tensor algebra, manual chain-rule training (loss 0.5 to 0.0037), serialize/restore, SSE tool loop, Vulkan F32 receipt | autodiff (#2772), Tensor in a struct (#2769), checker slowdown at 10k entries (#2779) |
| GUI | headless and TUI app buildable; native desktop and mobile not | Unicode files, reactive tree, roles and focus, themes, undo, release build in 8.5 s | platform services (D-FOUND-PLATFORM1), reactive ICEs (#2771), packaging (COREAPI1), android/ios target admission |
| Embedded | host-shaped firmware buildable; a Cortex-M deliverable not | board records, C layout, rings, pinning, signing, slot policy, host replay, native build | interrupts, DMA, registers (D-FOUND-BOARD1), deadline (REALTIME1), flashing (COREAPI1), thumbv7em toolchain |

The fourteen primitive probes confirmed most of the archive's judgments and overturned three:

- **Dimension arithmetic is not a gap.** `#UnitFamily` already derives, cancels, and formats dimensions (D-SHAPE-QUANTITY1, D-DIMENSION-OPEN1, D-DERIVED-DIMENSION-CLAIM1); the units probe rebuilt it with plain structs and hit the generic grammar. The supported path is the shipped mechanism (I8). What remains is a real gap for everyone else: operators between different types (D-FOUND-OPMIX1).
- **Numerics reach Rust.** A hand-written radix-2 FFT and a plain 512x512 matmul in release Jet matched native Rust (105 us vs 117 us; 341 ms vs 344 ms); the shipped `compute.fft` is a naive O(n^2) DFT, 4,500 times slower than the library kernel at N=4096 (#2780), and a 4096x4096 elementwise map never finishes (#2778). The compiler is not the problem; two Prelude functions are.
- **Tensor dtypes are ratified, not shipped.** D-COMPUTE-TYPE1 already names `Tensor<T>`; the runtime stores f64 only and the docs disagree with D-COMPUTE-BACKEND1 on the default profile. That is implementation and reconciliation, not a ballot.

### The gaps, deduplicated

Ninety-nine gaps fold into eleven primitives, twenty-three defects, and two rulings that stand:

| Primitive (ballot) | Gaps it closes | Areas |
|---|---|---|
| Real-time boundary (D-FOUND-REALTIME1) | prim-realtime G1-G4, area-games G4, area-embedded G4 | games, embedded, gui, audio |
| Opaque C handles and link closure (D-FOUND-HANDLE1) | prim-bridges G1-G3, prim-text G4, area-games G5, area-gui G4 (fonts), area-data G01 (Parquet) | games, ai-ml, data, gui, science, text |
| Sandbox capabilities (D-FOUND-SANDBOX1) | prim-capabilities G1-G3 | backend, cli, gui, games |
| Signals and deadlines as cancellation (D-FOUND-LIFECYCLE1) | area-backend G3-G4, area-cli G1 | backend, cli, web, games, gui, embedded |
| Operators between types (D-FOUND-OPMIX1, amends D-OPDEF1) | prim-units G3 (and every vector, matrix, money library) | science, games, data, embedded |
| Literals for library types (D-FOUND-LITERAL1) | prim-exact-numerics G4 | numerics, data, backend |
| Zero-copy views (D-FOUND-VIEW1) | prim-text G2, prim-storage G1, prim-numerics-perf G3 | cli, web, ai-ml, data, backend |
| Typed receipt sections (D-FOUND-RECEIPT1) | prim-receipts G1, area-embedded G6, prim-tooling-hooks G2 | data, backend, embedded, ai-ml |
| Platform services in core.ui (D-FOUND-PLATFORM1) | area-gui G2-G6 | gui |
| Interrupts, DMA, registers (D-FOUND-BOARD1) | area-embedded G2, G3, G7 | embedded |
| Eleven core API names (D-FOUND-COREAPI1) | area-cli G1-G2, area-games G1-G3, area-backend G1-G2, prim-distributed G1, prim-time G1, area-gui G7, area-embedded G6, prim-tooling-hooks G2 | all |

Rulings that stand, with the gap that tested them: user-defined macros (prim-dsl G5) are rejected by D-EXT1 and the philosophy's non-goals; regex lookaround (prim-text G3) is refused by design for linear-time matching (E0152); runtime access to `core.compiler` (prim-tooling-hooks G1) is compile-time only by D-FRONTENDAPI1 and the JSON CLI mirror served the rename tool. Const generics beyond `[T#capacity]` (prim-units G4) are closed by D-GENMOD-VALUE1 and were not re-opened. The orphan rule (prim-units G5) matches Rust and Swift; a newtype is the standard answer. Library-buildable and left as examples: event-time windows and durable checkpoints (prim-distributed G1-G2 ran in user code), typed inter-process channels (G3), jq-style filters and formula graphs (prim-dsl G3-G4), labeled and chunked arrays (prim-arrays G3-G4), the BPE tokenizer (prim-text G1), calendars and time scales (prim-time G2-G3), CSR storage and complex buffers (prim-numerics-perf G4-G5).

### The defects, by root cause

Twenty-three root causes explain thirty-one probe symptoms (`docs/research/domain-foundations/defects.json`, cards #2762-#2783). Two patterns matter more than the rest. Ten are "the generated Rust did not compile": ordinary programs that check and run in the default tier and then fail native build (a Send-less cell in an HTTP handler, a Result carrier on imported functions, `Float.is_finite`, `compute.set`, reactive callbacks, autodiff, a mangled `LocalDate`). The CLI discards rustc's error before printing the banner (#2783), so none could be diagnosed from the tool; a worker reconstructed the rustc call to find each cause. Four are evaluator gaps (E0956) that make the default tier refuse what native code runs, the I9 drift the invariant names: `core.math.pi`, map index-field assignment, `set_trace_id`, a replayed `time.now`. Two are wrong answers with no error at all: comptime `10/3` folds to `0/1` in the dev native tier, and `from_unix_nanoseconds` narrows at the i64 boundary. A shipped example fails on check (recursive enums, #2766) and another in release (`data_pipeline.jet`, #2781). Two were reported and could not be reproduced on the current binary (an `inner_join` E0956 and a second-build E3510); they are recorded as stale, not carded.

### The batteries

Each critical area's probe named the parts a first-party battery needs to get a builder to a working first program, marking which are pure library code and which wait for core (`docs/research/domain-foundations/results/<area>.batteries.json`). Cards #2795-#2801 and #2815 carry the lists; library parts can start now, core parts wait on the ballots and defects the card names.

## What has to be decided

Eleven ballots, all short and now all ratified A (owner, 2026-09-03), each with its reading surface (one question, one lesson, the current and in-the-wild code, one proposed block per option, real gains and losses, why the others lose, and what the recommendation still costs). Every recommendation is A. Two ballots amend a ratified ruling and say so: D-FOUND-OPMIX1 amends D-OPDEF1 (same-type operators); D-FOUND-SANDBOX1 fills the open D-PLUGIN1/D-DEP-WASM1 records. Suggested reading order: COREAPI1 (one batch, eleven names), then the four that unblock the most areas (VIEW1, HANDLE1, LIFECYCLE1, REALTIME1), then OPMIX1 and LITERAL1 (the language), then SANDBOX1, RECEIPT1, PLATFORM1, BOARD1.

Nothing waits on the owner. Defect cards are ready for the implementing orchestrator; battery and example cards are planned with their gates named; the probe wave card (#2761) is closed with its evidence.

## Evidence index

| Path | Contents |
|---|---|
| `docs/research/domain-matrix-archive/` | the mined research: census, claims, performance rows, family syntheses, the 46 withdrawn ballots |
| `docs/research/domain-foundations/classification.json` | the 88 verdicts above with their probe assignments |
| `docs/research/domain-foundations/probes/` | COMMON.md, 22 briefs, probes.json |
| `docs/research/domain-foundations/results/` | every probe's report, gaps.json, and batteries.json (22 probes) |
| `docs/research/domain-foundations/code/` | the probe programs and the minimal defect repros, as written by the workers |
| `docs/research/domain-foundations/all-gaps.json` | the 99 gaps in one file |
| `docs/research/domain-foundations/defects.json` | the 23 root causes with commands, errors, tiers, and locations |
| `docs/research/domain-foundations/law/` | the ratified-law map per ballot area (what is ratified, shipped, undecided, in conflict) |
| `docs/research/domain-foundations/slate/` | the ballot source (ballots.mjs) and the e15 milestones |
| `docs/proposals/domain-foundations.html` | the owner-facing story |
| Tower e15 | milestones e15-m01..m10 and m13; cards #2761-#2815 and #2847-#2857; ballots D-FOUND-* (all ratified A) on #2784-#2794 |
