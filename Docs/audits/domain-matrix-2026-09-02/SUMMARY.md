# Global domain-matrix synthesis (e15)

This mechanical union covers 108 registry domains and 127 unique mechanism IDs from 11 family syntheses.

## Counts

- Unique mechanisms: **127**.
- Mechanisms with at least five domains: **51**.
- Overlap candidates: **67** pair or cluster entries.
- Defect groups: **27** from 62 source observations, including the SQLite BLOB-to-NULL raw census finding.
- Domain matrix rows: **108**. P0 field total: **2957**; owner-gated P0 total: **2328**.
- Jet state: shipped 0; mixed 48; real-gap 79.

### Gate-kind memberships

A mechanism can contribute to more than one union gate kind. Membership counts: invariant 13; syntax 7; public-api 91; stdlib-dependency 12; command 4; ui 7; none 6. Most-restrictive primary counts: invariant 13; syntax 6; public-api 88; stdlib-dependency 10; command 2; ui 7; none 1.

### Source P0 row totals

- science-numerics: 270
- engineering: 458
- life-sciences-health: 173
- earth-space: 179
- finance-business: 159
- ai-ml: 287
- media-creative: 238
- embedded-hardware: 294
- platforms-infrastructure: 423
- security: 143
- tools-productivity: 222

## Twenty-five widest mechanisms

| ID | Name | Domains | P0 rows | Gate kind | Jet state | Existing cards |
| --- | --- | ---: | ---: | --- | --- | --- |
| M-VALIDATION-RECEIPTS | Cross-tier validation, reproducibility, and evidence receipts | 59 | 113 | invariant | mixed | D-DX-BENCH1, D-DX-LIVE1, #2421, #2439, #2440, #2471, #2481, #2498 |
| M-SCI-FORMATS | Standard scientific formats and interchange | 58 | 135 | public-api > stdlib-dependency | mixed | none |
| M-DATA-TABLES | Typed tabular, relational, and columnar data boundary | 50 | 80 | public-api > none | mixed | D-DX-DEVTOOLS-UX1, D-DX-LAZY1, D-DX-LOADERS1, #2421, #2471 |
| M-WORKFLOW-DAG | Inspectable workflow graphs and execution plans | 39 | 107 | public-api > command | mixed | #2441 |
| M-LINALG | Generic typed tensor and linear-algebra substrate | 37 | 41 | public-api > none | mixed | D-MATHLIB1 |
| M-PACKAGING | Package, dependency, and native-provider boundary | 36 | 79 | stdlib-dependency | mixed | D-DX-PROD1, D-ECO-FILEROOT1, #2421, #2499 |
| M-EVIDENCE | Inspection, diagnostics, and reproducible evidence | 34 | 51 | invariant > none | mixed | D-DX-DEVTOOLS-UX1, D-DX-PROD1, D-DX-SUITE1, #2429, #2453 |
| M-UNITS | Typed physical units and dimensional analysis | 34 | 49 | invariant > syntax | mixed | D-SHAPE-QUANTITY1 |
| M-GPU-KERNELS | GPU kernels, fusion, placement, and backend receipts | 33 | 57 | invariant > public-api | mixed | none |
| M-PLOTTING | Deterministic plotting, visualization, and publication export | 32 | 43 | ui | mixed | D-DX-DEVTOOLS-UX1, D-DX-PLOT1, D-DX-WEBARCH1 |
| M-NETWORKING | Typed network and service protocol boundaries | 31 | 90 | public-api > none | mixed | #2429 |
| M-NOTEBOOK | Notebook/REPL evaluation and reactive inspection | 31 | 29 | ui > none | mixed | D-DX-DEVTOOLS-UX1, D-DX-LIVE1, D-DX-SUITE1 |
| M-STORAGE | Durable scientific stores and caches | 28 | 55 | public-api > stdlib-dependency | mixed | #2422, #2441 |
| M-OPTIMIZATION | Optimization model surface and LP/MILP/NLP/CP-SAT backends | 27 | 67 | syntax > public-api | mixed | none |
| M-MPI-LAUNCH | Distributed execution, MPI collectives, and allocation-aware job launch | 26 | 29 | stdlib-dependency > command | mixed | D-DX-JOBS-UX1 |
| M-SIMULATION | Reproducible domain simulation lifecycle | 19 | 91 | public-api | mixed | none |
| M-SPARSE-SOLVERS | Sparse assembly, Krylov methods, and AMG/preconditioners | 18 | 24 | public-api | mixed | none |
| M-FFT | Planner-backed FFT family | 15 | 23 | public-api | mixed | none |
| M-HARDWARE-IO | Authority-safe hardware and instrument I/O | 15 | 53 | public-api | real-gap | none |
| M-SOLVER-SUBSTRATE | Checked nonlinear and domain solver substrate | 14 | 39 | public-api | mixed | none |
| M-TYPED-MODEL-RESULT | Typed model, fit, and result records | 12 | 27 | public-api | real-gap | none |
| M-DSP-FILTERS | Typed filter design and multirate DSP | 10 | 38 | public-api | real-gap | none |
| M-EXACT-NUMERIC | Exact and arbitrary-precision numeric domains | 10 | 13 | none | mixed | none |
| M-EXPERIMENT-LEDGER | Asset, lineage, run, artifact, and reproducible evidence ledger | 10 | 16 | public-api | real-gap | none |
| M-MESH-STENCILS | Meshes, stencils, and discretized field operators | 10 | 34 | public-api | real-gap | none |

The mechanism union keeps family IDs intact. The overlap file lists candidates only; it does not adjudicate merges, renames, or separations.

## Defect groups

| Group | Diagnostic code | Kind | Families | Observing domains |
| --- | --- | --- | --- | --- |
| DEF-GLOBAL-E0109 | E0109 | diagnostic-explanation-context-drift | tools-productivity | education-teaching |
| DEF-GLOBAL-E0150 | E0150 | typestate-negative | platforms-infrastructure | distributed-systems |
| DEF-GLOBAL-E0152 | E0152 | type-diagnostic | ai-ml | nlp-text |
| DEF-GLOBAL-E0405-EXAMPLE-DRIFT | E0405 | stale-example-diagnostic | finance-business, platforms-infrastructure, security | blockchain-web3, cryptography-engineering, distributed-systems, fintech-payments, identity-auth |
| DEF-GLOBAL-E0956-EVALUATOR-BOUNDARY | E0956 | default-tier-evaluator-gap | finance-business, ai-ml, media-creative, platforms-infrastructure | 3d-animation-dcc, data-engineering-etl, databases-storage-engines, observability-platforms, quant-trading, spreadsheets-business-logic |
| DEF-GLOBAL-E0981 | E0981 | missing-configuration-diagnostic | platforms-infrastructure | os-kernels |
| DEF-GLOBAL-E1001-MISSING-MODULES | E1001 | missing-core-module-diagnostic | science-numerics, earth-space, finance-business, ai-ml, media-creative, embedded-hardware, platforms-infrastructure, security | acoustics-audio-engineering, ar-vr-xr, audio-music-production, automotive-embedded, creative-coding, digital-forensics, econometrics, gis-geospatial, image-processing, ml-inference-serving, nlp-text, numerical-computing, plc-industrial, remote-sensing, telecom-5g, typography-publishing, video-vfx-compositing |
| DEF-GLOBAL-E1004-MISSING-ITEM | E1004 | missing-standard-library-item | finance-business, ai-ml, embedded-hardware | computer-vision, recommender-search, risk-actuarial, robotics |
| DEF-GLOBAL-E1803-AUTHORITY | E1803 | authority-boundary | finance-business, ai-ml, media-creative, platforms-infrastructure, tools-productivity | accounting-erp, creative-coding, databases-storage-engines, desktop-apps, ml-training, observability-platforms, rendering-graphics, shell-sysadmin |
| DEF-GLOBAL-E2101-INSPECTION-COMMAND | E2101 | missing-inspection-command | platforms-infrastructure, security | build-systems-monorepo, reverse-engineering |
| DEF-GLOBAL-E2102 | E2102 | stale-probe-input | embedded-hardware | embedded-iot |
| DEF-GLOBAL-E2104 | E2104 | expected-negative-policy-diagnostic | embedded-hardware | drones-uav |
| DEF-GLOBAL-E2105-LIVE-RUNTIME | E2105 | missing-live-runtime-diagnostic | platforms-infrastructure | observability-platforms |
| DEF-GLOBAL-E2105-PROVE-HASH | E2105 | proof-runtime-identity-hash | science-numerics, platforms-infrastructure, security | blockchain-web3, cryptography-engineering, digital-forensics, distributed-systems, identity-auth, proof-formal, reverse-engineering, security-tooling |
| DEF-GLOBAL-E2105-UNKNOWN-TARGET | E2105 | expected-negative-target-diagnostic | embedded-hardware, platforms-infrastructure | embedded-iot, os-kernels |
| DEF-GLOBAL-E3001 | E3001 | runtime-probe-failure | ai-ml | ml-training |
| DEF-GLOBAL-E3002 | E3002 | bridge-diagnostic | ai-ml | mlops |
| DEF-GLOBAL-E3260 | E3260 | platform-gated-binding | finance-business | spreadsheets-business-logic |
| DEF-GLOBAL-ICE-EXPRESSIONS-2737 | ICE | internal-compiler-error | engineering | chemical-process, fea-structural |
| DEF-GLOBAL-SOURCE-DEF-AIML-MLOPS-BRIDGE-EXAMPLE-001 | SOURCE-DEF-AIML-MLOPS-BRIDGE-EXAMPLE-001 | stale-example-contract | ai-ml | mlops |
| DEF-GLOBAL-SOURCE-DEF-AIML-TRAIN-AUTODIFF-001 | SOURCE-DEF-AIML-TRAIN-AUTODIFF-001 | compiler-diagnostic | ai-ml | ml-training |
| DEF-GLOBAL-SOURCE-DEF-ES-GEOSCIENCE-JIT-HANDLE-001 | SOURCE-DEF-ES-GEOSCIENCE-JIT-HANDLE-001 | internal-compiler-error | earth-space | geoscience-seismology |
| DEF-GLOBAL-SOURCE-DEF-FB-FLOAT-AUDIT-001 | SOURCE-DEF-FB-FLOAT-AUDIT-001 | precision-warning | finance-business | risk-actuarial |
| DEF-GLOBAL-SOURCE-DEF-TOOLS-EDITOR-LSP-TIMEOUT-001 | SOURCE-DEF-TOOLS-EDITOR-LSP-TIMEOUT-001 | bounded-probe-timeout | tools-productivity | editor-ide-extensions |
| DEF-GLOBAL-SOURCE-DEF-TOOLS-SHELL-POSIX-DRIFT-001 | SOURCE-DEF-TOOLS-SHELL-POSIX-DRIFT-001 | stale-example-api-contract | tools-productivity | shell-sysadmin |
| DEF-GLOBAL-SOURCE-DEF-TOOLS-TEXT-SOURCE-TIMEOUT-001 | SOURCE-DEF-TOOLS-TEXT-SOURCE-TIMEOUT-001 | external-evidence-retrieval-timeout | tools-productivity | text-processing-parsing |
| DEF-GLOBAL-SQLITE-BLOB-NULL | SQLITE-BLOB-NULL | lossy-storage-bridge | platforms-infrastructure | databases-storage-engines |

Each defect group retains source records and raw observations in defects.json. E2105 proof hashing is separate from E2105 target and live-runtime diagnostics.

## Parse and schema findings

| File | Field | Finding |
| --- | --- | --- |
| _families/science-numerics/defects.json | id, kind, status, domains, feature, diagnostic, evidence, probe_exit_code, impact, correction | All four records use a flat probe-observation shape with domain, command, observed, expected, and source_location; standard defect fields are absent. |
| _families/life-sciences-health/domains.json | avoid | All seven domain records omit the required avoid field. |
| _families/media-creative/domains.json | family | acoustics-audio-engineering is included here but registry.json assigns it to engineering; matrix uses the registry owner. |
| _families/embedded-hardware/domains.json | family | instrumentation-lab-automation is included here but registry.json assigns it to engineering; matrix uses the registry owner. |

No required JSON input failed to parse. All 108 census.json and performance.json domain files parsed. All mechanism domain references resolved to registry IDs.
