# Domain matrix

## Status

This document is the owner-facing proposal for Tower epoch e15.

| Field | Value |
|---|---|
| State | Proposal |
| Epoch | e15 |
| Date | 2026-09-02 |
| Owner gates | Open: D-DOMAIN-RINGS1, D-DOMAIN-GAUNTLET1, and D-DOMAIN-ORDER1 |
| Ratification unlocks | Ring placement, workload registration, closure rules, and the e15 build order |
| Card map | 219 cards: 88 mechanism, 108 domain, 9 defect, 11 gauntlet, and 3 global |
| Ballot map | 45 ballots: 42 mechanism ballots and 3 global ballots |

The global ballots are `D-DOMAIN-RINGS1` (#2539), `D-DOMAIN-GAUNTLET1` (#2537), and `D-DOMAIN-ORDER1` (#2538). Each is open on its card in Tower; `node plugins/tower/tower.mjs decision show <id>` prints the full ballot.

Ratification permits implementation against one placement policy, one performance gate, and one ranked order. It does not claim that any domain is shipped.

## Owner brief

The owner intent in DESIGN2.md is:

> "the goal is not just to beat languages on performance, we need to exceed
> the performance of other languages WHILE providing better out of the box experiences with better, more modern dx/ux.
> ... beating everyone on performance AND providing a substantially better experience than the best experience with
> other language's best ecosystem tools/libraries in STOCK OUT OF THE BOX jet."

Every domain a person writes software for is in scope. The owner named acoustics/MATLAB as the bar for niche depth.

Every design must pass two facets. Beginner magic means a first file follows the stock open -> do -> see loop without setup ceremony. Expert control means explicit backends, receipts, inspection, locks, authority, and performance evidence remain available.

## What the evidence says

The corpus gives four facts.

1. Jet's substrate is graded strong across domains. It includes typed bounded data (`core.data`, `DataLimits`, `DataError`), exact numerics (`Int`/`Decimal`, nominal units), and typed codecs, HTTP, SQL, and HTML.

   It also includes fail-closed authority and effects (E1803, `allow:` in `package.jet`), provider and placement receipts (`core.compute`), deterministic replay, tier parity (I9), and What/Why/Fix diagnostics.

2. The domain mechanism layer is missing. The missing layer includes value models, engines, format codecs, and the incumbent-shaped day-one loop, open -> do -> see.

3. The performance gate has no matched cells. 96 of 108 domains report gauntlet coverage as `none`.

   The other 12 name only adjacent generic cells: `numerics.float-kernel`, `netserv`, `embedded.kernel`, and `regex-logscan`. No domain has a matched cell.

4. A short list of substrate defects blocks even shipped claims.

The count views have different scopes, so this table labels each source.

| Measure | Count | Source |
|---|---:|---|
| Registry domains | 108 | SUMMARY.md |
| Census rows | 5,698 | DESIGN2.md evidence note and direct census recount |
| Raw P0 rows | 2,981 | DESIGN2.md evidence note and direct census recount |
| Raw owner-gated rows | 4,232 | DESIGN2.md evidence note and direct census recount |
| Claim-ledger rows | 3,171 | DESIGN2.md evidence note and direct claims recount |
| Sources | 3,595 | DESIGN2.md evidence note and direct manifest recount |
| Unique mechanism IDs before merges | 127 | SUMMARY.md |
| Merged mechanisms | 120 | DESIGN2.md §B |
| SUMMARY.md union P0 field total | 2,957 | SUMMARY.md |
| SUMMARY.md union owner-gated P0 total | 2,328 | SUMMARY.md |

## The unifying idea

The matrix is not 108 products. It is one substrate ledger plus ~120 shared mechanisms, projected into each domain's vocabulary.

51 mechanisms span five or more domains. The 12 widest touch 26–59 domains each.

Build order by mechanism breadth unblocks the most domains per unit of work. Each domain then needs its domain-only rows, example program, and gauntlet cell.

The beat vector shared by all families is one typed evidence ledger: source spans, effects, authority, provider, precision, placement, and replay.

Incumbent instrumentation is ambient, so it cannot offer this shared ledger. Jet wins a domain when the incumbent loop runs in stock Jet with the ledger attached and the gauntlet cell shows a win.

## Mechanism merges

The orchestrator adjudicated these merges.

| Canonical | Absorbed | Reason |
|---|---|---|
| `M-VALIDATION-RECEIPTS` | `M-EVIDENCE` | One evidence ledger; security `D-SEC-LEDGER1` argued the same. |
| `M-WORKFLOW-DAG` | `M-DURABLE-WORKFLOWS` | Durability is an execution property of the same graph; queue is #2457. |
| `M-DATA-TABLES` | `M-COLUMNAR` | Arrow, Parquet, and out-of-core work share one table boundary. |
| `M-RASTER` | `M-RASTER-GRIDS` | A georeferenced grid is a raster with CRS and nodata facets. |
| `M-SIMULATION` | `M-SIM-EVENTS` | Model, events, queues, and experiments form one simulation lifecycle. |
| `M-MESH-STENCILS` | `M-MESH-FEM` | Finite-element spaces are one facet of mesh and field discretization. |
| `M-LINALG` | `M-DENSE-FACTORS` | Factorizations are linear algebra. |

Folds have no card of their own. `M-FINANCE-CODECS` and `M-CATALOG-ARCHIVES` move to `M-SCI-FORMATS`.

`M-PACKAGING` moves to the global RINGS card (D1). Everything else stays separate.

After the merges, 120 mechanisms remain. The 88 mechanisms with two or more domains have mechanism cards.

A mechanism with exactly one domain has no mechanism card. Its rows live in that domain's design card plan.

The existing-card law is strict. When `_global/mechanisms.json` lists `existing_cards`, the new mechanism card extends that card or decision. It adds `blockedBy` and never re-decides the existing law.

These ratified e14 decisions bind every mechanism:

| Decision | Binding law |
|---|---|
| `D-DX-DEVTOOLS-UX1 = D` | `Pill -> Lens -> Workbench` devtools |
| `D-DX-LIVE1 = A` | One live-loop law |
| `D-DX-ENTRY1 = C` | Bare commands resolve a package entry by override-safe facts, then shallow inference |
| `D-DX-SUITE1 = C` | `core.web` suite on `core.reactive` |
| `D-DX-JOBS-UX1 = E` | `jet jobs <name>` |

Open e14 ballots stay pending. They are `D-DX-PLUGIN1`, `D-DX-WEBARCH1`, and `D-DX-PROD1`.

## Three global decisions

### D-DOMAIN-RINGS1 (#2539)

Question: Where do domain features live, who may depend on native libraries under I6, and what does `use` look like in one file?

- A. Fat Core: Put every domain API in the std-only Prelude under `core.<x>` and use only Jet implementations.
- B. Bridge packs: Ship each capability as a separately versioned first-party bridge pack with a package pin beside `use`.
- C. Two rings and a ratchet: Keep mechanism meaning in Ring 0, then place packs and backends in Ring 1 behind one index and receipt.
- D. Per-domain choice: Let each domain choose its own package, backend, lock, and import policy.
- E. Graduating rings: Start packs in Ring 1, then graduate them into Ring 0 after adoption and measured proof.

Recommendation: C.

Why: C keeps shared table and format meanings in Ring 0, so combined domain values use one type and operation contract. Ring 1 can accelerate those mechanisms behind one receipt. One toolchain index keeps `use jet.image` short while experts retain pins, trust policy, and offline locks.

Ring 0 owns a mechanism's types, operations, CPU-oracle reference implementation, diagnostics, and receipts in the std-only Prelude. Ring 1 holds first-party domain packs and backends behind that meaning.

A Ring 1 pack uses a toolchain-pinned index. A single file can resolve `use jet.image` without a manifest. Ring 1 backends can accelerate Ring 0 linalg, FFT, and compute behind the same receipt.
Ring 1 domain packs use the proposed `jet.<domain>` namespace and are distributed through jetpack.

The `backends` package key in `package.jet` is PROPOSED. The `authority.providers` field is ratified trust data, not backend selection.

The RINGS ratchet permits a bridge-only pack only when its gauntlet cell exists and is measured against the incumbent. Its native-backend card must be open, and proposed `jet inspect backends` must show the bridge.

`jet build --release` and publish refuse a bridge-only pack that fails the ratchet. They report a registered diagnostic.

### D-DOMAIN-GAUNTLET1 (#2537)

Question: When does Jet create each workload row, measure it, and let a domain card close?

- A. Measure every cell before any domain closes: Register all 108 rows, measure them first, and block every card on one uncovered or losing row.
- B. Measure one flagship cell per family: Register 11 family rows, add domain rows on request, and close each card after its registered rows pass.
- C. Register every cell and measure at domain build: Register all 108 rows as `uncovered`, then measure when each domain card enters `building`.
- D. Register every cell and measure on independent CI: Register all 108 rows, measure on a schedule, and let cards close without waiting for their rows.

Recommendation: C.

Why: C shows all 108 rows before work starts and still lets unrelated domains advance. It starts a row when its domain card enters `building`, then blocks that domain on missing or losing proof. Each row exposes its workload, output check, ratio, backend, and status.

C accepts a temporary owner-ratified matrix deferral with an explicit `remove_when` condition. A losing row never receives that exception.

### D-DOMAIN-ORDER1 (#2538)

Question: Which e15 work moves first for shared mechanisms and domain packs?

- A. Family by family: Build one owner-chosen family block with one flagship, shared dependencies, and its gauntlet before the next family.
- B. Mechanism breadth first: Build Ring 0 mechanisms in descending domain reach before opening flagships or domain packs.
- C. Controlled breadth with ranked flagships: Advance Ring 0 by reach, then open owner-swappable flagships under a bounded path gate.
- D. Owner-ranked backlog only: Give eligible cards to `tower-rank` without a family order, Ring 0 rule, or flagship quota.

Recommendation: C.

Why: C keeps Ring 0 as one breadth-first delivery stream and gives users an early, bounded flagship result. Its seed list uses the registry practitioner-base signal, Ring 0 coverage, and cheap gauntlet registration. `tower-rank` refreshes eligible order after closure.

The adaptive gate allows at most two flagship paths. It requires disjoint paths, a clean integration target, enough capacity, and one close owner.

## Mechanism ballots

The 42 D-M-* entries in BALLOTS-ADDED.json are grouped by DESIGN2.md's wave.

All 42 JSON ballots include `reviewPasses.beginner` and `reviewPasses.adversarial`. The beginner pass uses rli5. Each ballot passed the Tower validator and remains open on its mapped card.

### D2 wave 1

| Ballot | Card | Mechanism title | One-line question | Rec |
|---|---:|---|---|:---:|
| `D-M-RECEIPTS1` | #2568 | `M-VALIDATION-RECEIPTS`: Cross-tier validation, reproducibility, and evidence receipts | Which domain-specific evidence fields belong on Jet's existing receipt? | C |
| `D-M-FORMATS1` | #2625 | `M-SCI-FORMATS`: Standard scientific formats and interchange | Which format data does Jet own, and which approved bridges may read it? | C |
| `D-M-TABLES1` | #2561 | `M-DATA-TABLES`: Typed tabular, relational, and columnar data boundary | What stock path handles typed Parquet tables beyond memory? | C |
| `D-M-WORKFLOW1` | #2638 | `M-WORKFLOW-DAG`: Inspectable workflow graphs and execution plans | How does Jet record workflow parts and resume failed runs? | C |
| `D-M-LINALG1` | #2600 | `M-LINALG`: Generic typed tensor and linear-algebra substrate | What public module holds shaped numbers, matrix breakdowns, and their library? | C |
| `D-M-UNITS1` | #2575 | `M-UNITS`: Typed physical units and dimensional analysis | Where does Jet check units and record meaning for wire data and integration? | D |
| `D-M-GPU1` | #2595 | `M-GPU-KERNELS`: GPU kernels, fusion, placement, and backend receipts | Should Jet hide GPU work, expose checked launches, or offer both? | C |
| `D-M-STORAGE1` | #2630 | `M-STORAGE`: Durable scientific stores and caches | Should one typed path serve local files and approved database engines? | C |
| `D-M-OPTIMIZATION1` | #2572 | `M-OPTIMIZATION`: Optimization model surface and LP/MILP/NLP/CP-SAT backends | How should Jet write a checked optimization model? | C |
| `D-M-DISTRIBUTED-EXEC1` | #2610 | `M-MPI-LAUNCH`: Distributed execution, MPI collectives, and allocation-aware job launch | How should Jet start work on several machines and record its result? | C |
| `D-M-SIMULATION1` | #2627 | `M-SIMULATION`: Reproducible domain simulation lifecycle | How should Jet advance steps, people, and equations through one reproducible run? | C |
| `D-M-SOLVERS1` | #2556 | `M-SOLVER-SUBSTRATE`: Checked nonlinear and domain solver substrate | How should ODE, sparse, and root calls share typed results and receipts? | C |

### D3 wave 2

| Ballot | Card | Mechanism title | One-line question | Rec |
|---|---:|---|---|:---:|
| `D-M-SIGNAL1` | #2558 | `M-FFT`: Planner-backed FFT family | What public calls and result values cover frequencies, filters, and sample-rate changes? | C |
| `D-M-HARDWARE-IO1` | #2596 | `M-HARDWARE-IO`: Authority-safe hardware and instrument I/O | Should hardware use manual calls, typed bus drivers, or one shared session? | C |
| `D-M-STATS1` | #2635 | `M-TYPED-MODEL-RESULT`: Typed model, fit, and result records | What checked path covers formulas, model results, and random draws? | C |
| `D-M-ML-LIFECYCLE1` | #2590 | `M-EXPERIMENT-LEDGER`: Asset, lineage, run, artifact, and reproducible evidence ledger | How should Jet check and join data, model state, and run history? | C |
| `D-M-GEOMETRY-MESH1` | #2609 | `M-MESH-STENCILS`: Meshes, stencils, and discretized field operators | What native or bridged path produces a typed STEP-to-field result? | C |
| `D-M-API-CONTRACT1` | #2552 | `M-API-CONTRACT`: Executable API contracts, schemas, bindings, and compatibility | Where does the typed promise for `POST /orders` live? | C |
| `D-M-EMBEDDED-PLATFORM1` | #2579 | `M-BOARD-BSP`: Typed board, target, BSP, and HAL facts | What owns the board lifecycle for board, flash, debug, and OTA work? | C |
| `D-M-CALENDARS1` | #2582 | `M-CALENDARS-BUSINESS-DATES`: Civil calendars, exchange sessions, and business-date arithmetic | Where should calendar rules and business-date calculations live? | C |
| `D-M-SAFETY-TIMING1` | #2636 | `M-WCET`: Target-bound timing, jitter, WCET, and deadline evidence | How should Jet prove a control task meets its deadline and safety duties? | C |
| `D-M-CONNECTORS1` | #2560 | `M-CONNECTORS-WEBHOOKS`: Typed external connectors, webhooks, and capability injection | How should Jet type service connectors, credentials, and verified webhooks? | C |
| `D-M-PLUGIN-CAPS1` | #2616 | `M-PLUGIN-CAPS`: Explicit capability imports, limits, and authority-scoped extensions | How should image data, filesystem permission, and failure limits cross a plugin boundary? | C |
| `D-M-QUERY1` | #2554 | `M-QUERY-PLANNER`: Analytical SQL, typed plans, and physical execution | Should a data report use query text, typed operations, one plan, or wait? | C |
| `D-M-RULES1` | #2621 | `M-RULES-DECISIONS`: Declarative rules, decisions, and policy execution | How should Jet represent and run typed rules? | C |
| `D-M-SYMBOLIC1` | #2632 | `M-SYMBOLIC`: Typed symbolic AST, assumptions, equations, and rewrite calculus | Should symbolic formulas use calls, a `#Symbolic` block, or typed values? | C |
| `D-M-LOAD1` | #2601 | `M-LOAD-RUNNER`: Scenario load runners with deterministic thresholds | How should Jet schedule repeated work and record its measurements? | C |
| `D-M-GRAPHICS1` | #2594 | `M-GPU-GRAPHICS`: Portable graphics devices, shaders, pipelines, and frame resources | How should Jet draw a triangle, sketch in 2D, and pick a ray? | C |
| `D-M-SCENE1` | #2624 | `M-SCENE-GRAPH`: Typed scene graphs, transforms, assets, and graph identity | Should scene access use component queries, parent paths, or one graph? | C |
| `D-M-NLP1` | #2611 | `M-NLP-PIPELINE`: Tokenizer, document, alignment, and text annotation pipeline | How should Jet turn text into aligned tokens and annotations? | C |
| `D-M-OS-SERVICES1` | #2569 | `M-OS-SERVICES`: Authority-bound operating-system services | Should one backup receipt use manager commands, a supervisor, host records, or dev supervision? | C |
| `D-M-LEDGER1` | #2606 | `M-DOUBLE-ENTRY-LEDGER`: Balanced double-entry ledger and immutable posting | Where should Jet own balanced money movement rules and failure results? | C |
| `D-M-FINDINGS1` | #2607 | `M-FINDINGS-RULES`: Portable security rules, dataflow findings, and SARIF publication | Should findings use YAML, Jet queries, or typed rules? | C |
| `D-M-LABELED-ARRAYS1` | #2599 | `M-LABELED-ARRAYS`: Labeled N-D arrays, coordinates, and CF metadata | Where should labels live when a `Tensor` is read and aligned? | C |
| `D-M-OBJECT-PIPELINE1` | #2613 | `M-OBJECT-PIPELINE`: Typed object pipelines and remote session handles | Should one process task use text pipes, records, or typed Jet values? | C |
| `D-M-RASTER1` | #2617 | `M-RASTER`: First-class raster/image values and bounded pixel storage | What one value model should represent image and map pixels? | C |
| `D-M-WORKBOOK1` | #2637 | `M-WORKBOOK-FORMULAS`: Workbook dependency graphs, formulas, and calculation receipts | How should Jet edit Excel files and compute changed cells? | C |
| `D-M-STREAMS1` | #2555 | `M-REALTIME-STREAMS`: Bounded real-time audio and device streams | Should streams use separate types, library checks, one carrier, or a plan value? | C |
| `D-M-STRUCTURED-FILTER1` | #2631 | `M-STRUCTURED-FILTER`: Structured filter programs over bounded streams | Should bounded JSONL use jq text, a filter file, or typed Jet code? | C |
| `D-M-SYNTAX-TREE1` | #2633 | `M-SYNTAX-TREE`: Incremental lossless syntax trees and structural queries | What one source tree should serve editor, formatter, codemod, and query work? | C |
| `D-M-HDL1` | #2719 | `M-HDL-SEMANTICS`: Typed HDL signals, clocks, elaboration, and RTL | Should circuits use library netlists, an HDL block grammar, or typed signal values? | C |
| `D-M-MEDIA1` | #2608 | `M-MEDIA-PIPELINE`: Media codec, demux/decode/filter/encode/mux pipeline | How should Jet move media, keep time, process audio, and load plugins? | C |

`D-M-HDL1` uses #2719, the FPGA-HDL domain card. One-domain mechanisms have no separate mechanism card, so its title comes from `_global/mechanisms.json`.

## Card structure

The minted card map has this shape.

| Card kind | Count |
|---|---:|
| Mechanism | 88 |
| Domain design | 108 |
| Defect | 9 |
| Gauntlet | 11 |
| Global policy | 3 |
| Total | 219 |

Domain design cards group by family as follows.

| Family | Domain cards |
|---|---:|
| science-numerics | 12 |
| engineering | 14 |
| life-sciences-health | 7 |
| earth-space | 7 |
| finance-business | 8 |
| ai-ml | 12 |
| media-creative | 8 |
| embedded-hardware | 8 |
| platforms-infrastructure | 17 |
| security | 6 |
| tools-productivity | 9 |

Every mechanism and domain card is blocked by the RINGS policy card. A mechanism card is also blocked by each existing e14 card or decision that it extends.

A domain card is blocked by its shared mechanism cards and its family gauntlet card. A gauntlet card is blocked by the GAUNTLET policy card.

`needsAcceptance` is true only when the incumbent loop is a GUI product. The listed GUI loop families are dashboards, desktop, creative, education, DCC, and notebooks. Other domain cards set it false.

## Substrate defects

The nine defect cards in `_cards/_defects/*.json` are:

| Card | Defect id | Diagnostic code | Families affected | Effect |
|---|---|---|---|---|
| #2639 | `autodiff-higher-order` | E0112, E0102 | ml-training | The higher-order derivative fixture stops before curvature and lacks source-spanned derivative evidence. |
| #2640 | `core-sys-posix-example-drift` | E0305, E0307, E0107, L0520 | shell-sysadmin | The POSIX example uses obsolete `Ok`/`Err` patterns and fails the current `Int`, `Unit`, and `[Int]` contracts. |
| #2641 | `e0109-explanation-context-drift` | E0109 | education-teaching | The live `Int`/`String` error disagrees with `jet explain`'s U8/I8 explanation. |
| #2642 | `e0405-crypto-auth-examples` | E0405 | blockchain-web3, cryptography-engineering, distributed-systems, fintech-payments, identity-auth | Current crypto and auth examples use value-carrying returns in result-less functions and fail checks. |
| #2643 | `e0956-evaluator-boundary` | E0956 | 3d-animation-dcc, data-engineering-etl, databases-storage-engines, observability-platforms, quant-trading, spreadsheets-business-logic | The default evaluator rejects lazy, query, and `GameScene` paths. |
| #2644 | `e2105-prove-hash` | E2105 | blockchain-web3, cryptography-engineering, digital-forensics, distributed-systems, identity-auth, proof-formal, reverse-engineering, security-tooling | `jet prove` hashes the running compiler, so proof and replay producers stay unreachable. |
| #2645 | `ice-unit-formatting-2737` | ICE at `expressions.rs:2737:14` | chemical-process, fea-structural | A successful quantity program emits ICE warnings for unit-format expressions. |
| #2646 | `jit-bad-handle-geoscience` | ICE `jit list len: bad handle` | geoscience-seismology | The typed waveform table prints rows, then the JIT aborts with a bad handle. |
| #2647 | `sqlite-blob-null` | SQLITE-BLOB-NULL | databases-storage-engines | The SQLite bridge maps queried BLOB bytes to NULL instead of preserving them. |

## Performance gate

The AGENTS.md comparator is per cell and metric.

- Jet must strictly beat every matched non-Rust peer, with Jet/peer below `1.00`.
- Rust permits a Jet/Rust ratio of `1.05` or lower. This band is measurement noise, not a target or win.
- No gate may average away a loss, substitute a workload or tier, omit a peer, or accept unavailable, uncovered, mismatched, or inconclusive evidence.

The 11 gauntlet cards use each domain's `performance.json` workload, incumbent, and metric. They register all 108 cells immediately as `uncovered`.

Each cell records a locked workload, incumbent, metric, fixed input, output check, required tiers, and backend receipt. A valid same-machine run changes the status from `uncovered` to `win` or `loss`.

Measurement starts when the domain card enters `building`. No domain card closes while its cell is `uncovered` or `loss`.

The only exception is an owner-ratified matrix deferral with an explicit `remove_when` condition. A losing cell never receives that exception. The public scoreboard shows one status row for every domain.

## Build order

D-DOMAIN-ORDER1 recommends Ring 0 breadth first, with controlled flagship breadth.

The default flagship list is:

| Family | Flagship |
|---|---|
| science-numerics | numerical-computing |
| engineering | signal-processing |
| life-sciences-health | bioinformatics-genomics |
| earth-space | gis-geospatial |
| finance-business | quant-trading |
| ai-ml | ml-training |
| media-creative | image-processing |
| embedded-hardware | embedded-iot |
| platforms-infrastructure | databases-storage-engines |
| security | cryptography-engineering |
| tools-productivity | testing-qa-automation |

The seed criterion has three parts. Use the registry incumbent as the practitioner-base signal. Use Ring 0 coverage. Use a cheap gauntlet registration seed.

The owner may swap any flagship before it opens. `tower-rank` refreshes eligible order after closure.

Ring 0 remains one breadth-first delivery stream. A second flagship path needs disjoint paths, a clean integration target, enough capacity, and one close owner.

## Beginner and expert passes

A beginner sees a stock Jet file open a domain input, do one typed operation, and show a result, error, or receipt. The loop is open -> do -> see, with no backend selection or manifest setup.

An expert controls `backends`, backend pins, authority, locks, and receipts. The expert can run `jet inspect backends` and `jet explain --cost` to inspect placement and cost.

## What is not decided here

No I9 carve-outs are proposed.

New syntax enters only through the I7 decision id named in its ballot. This proposal does not ratify syntax by itself.

The open e14 ballots remain pending: `D-DX-PLUGIN1`, `D-DX-WEBARCH1`, and `D-DX-PROD1`.

The pending RINGS decision blocks every Ring 1 bridge and pack.

## Evidence index

The corpus is under `~/.cache/jet-luna/dx2/`. It is a machine-local working set, not a repository artifact; Tower and this document carry the durable record.

| Path | Contents |
|---|---|
| `<domain>/report.md` | Domain report and evidence summary |
| `<domain>/census.json` | Domain census rows |
| `<domain>/performance.json` | Domain performance workload and incumbent |
| `<domain>/claims.json` | Domain claim ledger rows |
| `<domain>/manifest.json` | Domain source manifest |
| `_families/<family>/` | Family synthesis files |
| `_global/` | Global merge, registry, summary, and defect data |
| `_cards/` | Minted global, mechanism, domain, defect, and gauntlet payloads |
| `ballots/` | Full global and mechanism ballot JSON files |
| `reviews/` | Beginner and adversarial review files |

The card map is `_cards/MINTED.json`. The ballot map is `_cards/BALLOTS-ADDED.json`.
