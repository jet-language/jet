# Domain-matrix history and rescope

## Status

This is the maintained history of the domain-matrix slate from 2026-09-02. It is not a proposal, a ballot, a ratification, or runtime evidence. The current replacement record is [Foundations for every domain](../audits/domain-foundations-2026-09-02/domain-foundations-2026-09.md). The complete historical reports and payloads are retained intact under [`docs/audits/domain-matrix-2026-09-02/`](../audits/domain-matrix-2026-09-02/).

The old proposal and its research wrapper are retired. The old archive directory is not an active source. The audit copies are immutable historical evidence.

## Why the slate was withdrawn

The slate started from a valid owner goal: Jet must beat other languages on performance and give people a better stock development experience. It treated every domain a person writes software for as in scope, with acoustics and MATLAB named as the depth bar. It also had the right two-facet test:

- A beginner should open an input, do one typed operation, and see a result, error, or receipt without backend or manifest setup.
- An expert should retain explicit backends, provider pins, authority, locks, inspection, and evidence.

The mistake was the ownership boundary. The plan turned the domain census into a program for Jet to own the mechanisms and eventually the libraries for every niche. The owner withdrew it on 2026-09-02 because Jet would become the author of every niche library forever. The owner’s replacement rule is simpler: Jet must supply the primitives and tools that a library author cannot supply safely or efficiently, then that author builds the niche library in Jet.

This was a scope correction, not a finding that the corpus had no value. The family reports showed repeated needs for typed data, units, tensors, formats, networking, workflows, evidence, authority, and complete first-use loops. They did not prove that Jet should ship acoustics, ledgers, HDL, media, geospatial, medical, or other domain products in Core. Generic substrate success is not domain support, and an incumbent benchmark number is not a Jet result.

## What the withdrawn slate contained

The count views have different scopes. They remain separate here rather than being averaged.

| Measure | Count | Scope and source |
|---|---:|---|
| Registry domains | 108 | Global `SUMMARY.md` union |
| Census rows | 5,698 | Raw `census.jsonl.gz` |
| Raw P0 rows | 2,981 | Raw census priority rows |
| Raw owner-gated rows | 4,232 | Raw census rows with `owner_gate: true` |
| Source-linked claims | 3,171 | Raw `claims.jsonl.gz` |
| Sources | 3,595 | Original manifest recount recorded by the proposal |
| Mechanism IDs before adjudication | 127 | Global mechanism union |
| Mechanism IDs after the proposed merges | 120 | Proposal merge section |
| Mechanisms with two or more domains | 88 | Mechanism-card rule after merges |
| Mechanisms touching at least five domains | 51 | Global summary |
| Overlap candidates | 67 | Raw `overlapCandidates`; candidates were not decisions |
| Global-union P0 field total | 2,957 | `SUMMARY.md`; not the raw 2,981 total |
| Global-union owner-gated P0 total | 2,328 | `SUMMARY.md`; not the raw 4,232 total |
| Defect groups | 27 from 62 observations | Global summary; source observations remain in the payload |
| Jet state rows | 0 shipped, 48 mixed, 79 real-gap | Global summary's mechanism-state view |

The 127-to-120 change came from these proposed folds:

| Canonical mechanism | Absorbed or redirected mechanism | Reason recorded at the time |
|---|---|---|
| `M-VALIDATION-RECEIPTS` | `M-EVIDENCE` | One evidence ledger; the security synthesis made the same case. |
| `M-WORKFLOW-DAG` | `M-DURABLE-WORKFLOWS` | Durability was treated as an execution property of the same graph. |
| `M-DATA-TABLES` | `M-COLUMNAR` | Arrow, Parquet, and out-of-core work shared one table boundary. |
| `M-RASTER` | `M-RASTER-GRIDS` | A georeferenced grid was treated as a raster with CRS and nodata facets. |
| `M-SIMULATION` | `M-SIM-EVENTS` | Models, events, queues, and experiments formed one lifecycle. |
| `M-MESH-STENCILS` | `M-MESH-FEM` | Finite-element spaces were treated as one mesh and field facet. |
| `M-LINALG` | `M-DENSE-FACTORS` | Factorizations were treated as linear algebra. |
| `M-SCI-FORMATS` | `M-FINANCE-CODECS`, `M-CATALOG-ARCHIVES` | Those rows were redirected to the shared formats mechanism. |
| Global RINGS policy | `M-PACKAGING` | Packaging was moved to the global policy card rather than a mechanism card. |

The merge file retained 67 overlap candidates without deciding every rename or separation. A one-domain mechanism had no mechanism card; its rows were to live on a domain design card. This distinction matters because the withdrawn design’s 120 mechanism IDs were not 120 products.

### Ballot and card lineage

The slate had 46 archived ballot records and 219 cards:

| Card kind | Count | Disposition after withdrawal |
|---|---:|---|
| Mechanism | 88 | Deleted with the slate |
| Domain design | 108 | Deleted with the slate |
| Gauntlet | 11 | Deleted with the slate |
| Global policy | 3 | Deleted with the slate |
| Defect | 9 | Retained as substrate defects |
| **Total** | **219** | **210 deleted; 9 retained** |

The archive payload contains 42 ratified records and four records that were still open at the snapshot. Its recommendation counts are one `A`, 44 `C`, and one `D`. The proposal’s prose lists the 42 `D-M-*` mechanism ballots; the payload also contains `D-DOMAIN-LAYOUT1`, which is retained in the index below instead of being inferred into the three global-policy count.

## Historical decision index

These entries are for lookup and migration. They do not reopen decisions. The full options, comparisons, worked code, review passes, and status fields remain in the retained ballot payload at [`ballots.json.gz`](../audits/domain-matrix-2026-09-02/ballots.json.gz). A human `#` card is shown where the proposal named one; the opaque archived card key remains in the payload.

| Decision | Historical card | Recommendation | Snapshot status | Disposition |
|---|---:|:---:|---|---|
| `D-DOMAIN-LAYOUT1` | archived key `c0nctuf1` | A | ratified | Superseded with the slate; preserve record only. |
| `D-DOMAIN-RINGS1` | #2539 | C | ratified | Superseded; broad Core ownership is rejected by the replacement boundary. |
| `D-DOMAIN-GAUNTLET1` | #2537 | C | ratified | Superseded; use the replacement area's probes and matched foundation cells. |
| `D-DOMAIN-ORDER1` | #2538 | C | ratified | Superseded; use the replacement area's bounded probe order. |
| `D-M-RECEIPTS1` | #2568 | C | open | Superseded; receipt extensibility is judged under the foundation policy. |
| `D-M-FORMATS1` | #2625 | C | ratified | Superseded; domain formats are library work unless a primitive blocks them. |
| `D-M-TABLES1` | #2561 | C | ratified | Superseded; retain one shared table type as a foundation. |
| `D-M-WORKFLOW1` | #2638 | C | ratified | Superseded; workflow batteries are not a niche Core product. |
| `D-M-LINALG1` | #2600 | C | ratified | Superseded; retain the shared tensor and linear-algebra boundary. |
| `D-M-UNITS1` | #2575 | D | ratified | Superseded; retain one shared unit type and re-judge missing primitives. |
| `D-M-GPU1` | #2595 | C | ratified | Superseded; retain compiler/runtime placement questions, not domain GPU libraries. |
| `D-M-STORAGE1` | #2630 | C | ratified | Superseded; storage packages build above the available durability primitives. |
| `D-M-OPTIMIZATION1` | #2572 | C | ratified | Superseded; solver and optimization libraries build above the primitive seam. |
| `D-M-DISTRIBUTED-EXEC1` | #2610 | C | ratified | Superseded; keep only primitive execution and evidence gaps in scope. |
| `D-M-SIMULATION1` | #2627 | C | ratified | Superseded; simulation lifecycles are library work unless a primitive is required. |
| `D-M-SOLVERS1` | #2556 | C | ratified | Superseded; solver APIs are not automatically Core-owned. |
| `D-M-SIGNAL1` | #2558 | C | ratified | Superseded; signal libraries remain above the numeric primitive boundary. |
| `D-M-HARDWARE-IO1` | #2596 | C | ratified | Superseded; retain authority, MMIO, and ownership primitives. |
| `D-M-STATS1` | #2635 | C | ratified | Superseded; model and result libraries are not Core by default. |
| `D-M-ML-LIFECYCLE1` | #2590 | C | ratified | Superseded; experiment ledgers are library/tooling work above receipts. |
| `D-M-GEOMETRY-MESH1` | #2609 | C | ratified | Superseded; geometry and mesh products are not automatic Core APIs. |
| `D-M-API-CONTRACT1` | #2552 | C | ratified | Superseded; retain shared contract/build primitives and let libraries publish APIs. |
| `D-M-EMBEDDED-PLATFORM1` | #2579 | C | open | Superseded; board primitives are re-judged by the embedded probe. |
| `D-M-CALENDARS1` | #2582 | C | ratified | Superseded; calendar libraries build over core time. |
| `D-M-SAFETY-TIMING1` | #2636 | C | ratified | Superseded; target-bound timing evidence remains a primitive question. |
| `D-M-CONNECTORS1` | #2560 | C | ratified | Superseded; typed connectors are library code over network and authority seams. |
| `D-M-PLUGIN-CAPS1` | #2616 | C | open | Superseded; capability scoping remains a runtime/toolchain primitive question. |
| `D-M-QUERY1` | #2554 | C | ratified | Superseded; analytical planners are not a second Core data product. |
| `D-M-RULES1` | #2621 | C | ratified | Superseded; rules and decision tables are library DSLs unless a primitive blocks them. |
| `D-M-SYMBOLIC1` | #2632 | C | ratified | Superseded; symbolic ASTs are library DSLs unless a primitive blocks them. |
| `D-M-LOAD1` | #2601 | C | ratified | Superseded; load measurement is an ecosystem tool, not a domain product. |
| `D-M-GRAPHICS1` | #2594 | C | ratified | Superseded; graphics runtime seams are re-judged by games and GUI probes. |
| `D-M-SCENE1` | #2624 | C | open | Superseded; scene graphs are library work over the graphics seam. |
| `D-M-NLP1` | #2611 | C | ratified | Superseded; token and annotation pipelines are library work over text primitives. |
| `D-M-OS-SERVICES1` | #2569 | C | ratified | Superseded; retain authority-bound service primitives. |
| `D-M-LEDGER1` | #2606 | C | ratified | Superseded; ledgers are library records over exact money and evidence. |
| `D-M-FINDINGS1` | #2607 | C | ratified | Superseded; findings and SARIF are library/tooling projections over compiler seams. |
| `D-M-LABELED-ARRAYS1` | #2599 | C | ratified | Superseded; labels are library data over the shared tensor boundary. |
| `D-M-OBJECT-PIPELINE1` | #2613 | C | ratified | Superseded; object pipelines are library code over iteration and processes. |
| `D-M-RASTER1` | #2617 | C | ratified | Superseded; raster values are library data over the shared tensor boundary. |
| `D-M-WORKBOOK1` | #2637 | C | ratified | Superseded; workbook graphs are library DSLs over records and formulas. |
| `D-M-STREAMS1` | #2555 | C | ratified | Superseded; retain bounded stream and deadline primitives only. |
| `D-M-STRUCTURED-FILTER1` | #2631 | C | ratified | Superseded; filters are library code over typed streams. |
| `D-M-SYNTAX-TREE1` | #2633 | C | ratified | Superseded; retain compiler-owned tree/tooling seams, not domain products. |
| `D-M-HDL1` | #2719 | C | ratified | Superseded; HDL is library/tooling work unless a primitive is proven necessary. |
| `D-M-MEDIA1` | #2608 | C | ratified | Superseded; media pipelines are library code over graphics, time, and bridge seams. |

The three named global ballots proposed these choices:

- **RINGS (`D-DOMAIN-RINGS1`, recommendation C):** Ring 0 would own mechanism types, operations, CPU-oracle implementations, diagnostics, and receipts. Ring 1 would hold first-party domain packs and backends behind one index and receipt. The rejected alternatives were a fat Core, package-only bridges with per-use pins, per-domain policy, and graduating packs into Core.
- **GAUNTLET (`D-DOMAIN-GAUNTLET1`, recommendation C):** Register all 108 cells as uncovered, then measure a domain when its card entered `building`. The rejected alternatives measured all cells before any domain, measured only one flagship per family, or let independent CI run without blocking closure.
- **ORDER (`D-DOMAIN-ORDER1`, recommendation C):** Advance Ring 0 by reach, then open bounded, owner-swappable flagships. The rejected alternatives were family-by-family blocks, pure mechanism breadth before flagships, and an owner-ranked backlog without a Ring 0 or path gate.

The rejected choices all shared one failure: they optimized the mechanics of Jet owning the niche matrix instead of asking what a library author cannot build.

## What the family reports established

All eleven family syntheses are retained intact. They differ in domain ownership and count scope, but converge on these findings:

- A typed evidence and receipt ledger, shared tables, units, numeric/tensor foundations, authority, packaging, networking, formats, workflows, and inspection recur across families.
- Incumbents win complete first loops: a domain object carries its representation, execution, diagnostics, and result from first input to publication or operation.
- Generic Jet substrate is not a domain implementation. A tensor is not a raster, sockets are not packet tooling, generic codecs are not FHIR/FIX/NetCDF/FFmpeg, a target dossier is not a board witness, and a headless scene is not a renderer.
- Most family performance records had no matched domain gauntlet. Adjacent generic cells, illustrative timings, and published incumbent values remain leads, not Jet wins.
- Expected-negative policy diagnostics, stale examples, evaluator boundaries, and actual compiler defects must stay distinct. The family reports do not average contradictory evidence into a green result.

| Family report | Family view retained in the report | Mechanism records in that report | Global `SUMMARY.md` P0 allocation |
|---|---|---:|---:|
| `science-numerics.md` | 12 scientific and logic domains; algebra, solvers, transforms, models, proof, and evidence | 21, all emitted as new because the prior file was absent | 270 |
| `engineering.md` | 14 physical and numerical domains; units, DSP, solvers, geometry, I/O, formats, and evidence | 24, emitted as new because prior mechanism files were absent | 458 |
| `life-sciences-health.md` | 7 record, geometry, model, and clinical domains; six new mechanisms plus reused substrate | 24 candidates (6 new, 18 reused) | 173 |
| `earth-space.md` | 7 spatial, physical, orbit, and catalog domains; labeled arrays, CRS, grids, frames, and formats | 31 (9 new, 22 reused) | 179 |
| `finance-business.md` | 8 business domains; exact money, ledgers, calendars, workflows, codecs, connectors, and rules | 17 (7 new, 10 reused) | 159 |
| `ai-ml.md` | 12 model, data, retrieval, streaming, analytics, and dashboard domains | 23 (11 new, 12 reused) | 287 |
| `media-creative.md` | 9 raster, graphics, scene, audio, media, timeline, document, and publishing domains | 26 (13 new, 13 reused) | 238 |
| `embedded-hardware.md` | 9 target-bound firmware, board, bus, HDL, safety, driver, and instrument domains | 30 (11 new, 19 reused) | 294 |
| `platforms-infrastructure.md` | 17 platform, network, storage, build, service, desktop, and distributed domains | 25 (12 new, 13 reused) | 423 |
| `security.md` | 6 security domains; packet, findings, binary, forensic, identity, authorization, chain, and crypto seams | 15 (10 new, 5 reused) | 143 |
| `tools-productivity.md` | 9 operator, text, docs, testing, bot, education, office, and package domains | 20 (12 new, 8 reused) | 222 |

The family domain views intentionally overlap in places. For example, the family files include acoustics and instrumentation in more than one view while the registry assigns them to engineering. Their P0 totals also use different scopes: finance reports 244 family rows, life sciences reports 227 P0 rows and 203 post-implementation deficits, embedded reports 306 P0 rows, and security reports 184 P0 rows, while the global union table above assigns 159, 173, 294, and 143 respectively. These are not errors to reconcile by averaging; the retained reports and raw payloads preserve the source scope and cross-family ownership notes.

## Replacement boundary

The owner replaced the niche matrix with a builder-first program. Core keeps four categories:

1. Language primitives a library cannot add: syntax, the type system, memory and effects, execution tiers, and the runtime.
2. Ecosystem and developer-experience tools: build, packages, devtools, the live loop, tests, receipts, diagnostics, and editor support.
3. The core library that already ships.
4. Shared data types that independent libraries must agree on: one table, one unit, one receipt, and one tensor.

A niche such as acoustics, a ledger, an HDL flow, or a media pipeline is a library someone writes in Jet. Jet must give that author the tools needed to build it, but does not promise to ship the niche product in Core.

The replacement tests a mechanism in this order:

- If a niche can be built with today's Jet, keep it as an executable example or battery at most.
- If it cannot be built, propose the missing primitive, not the niche library.
- Call something a primitive gap only when it is impossible, unsafe without `#Unsafe`, slow without compiler help, burdened by repeated call-site ceremony, or burdened by heavy author boilerplate that is very common or shared across domains.
- Use full “everything a builder needs” probes for eight critical areas: web, games, CLI and scripts, data analysis, backend services, AI/ML applications, GUI apps, and embedded.
- Use targeted build probes for doubtful mechanisms and judgment for plain cases.
- Measure foundations plus one real workload per critical area. A niche gets a performance cell only when Jet ships a battery for it.

This boundary keeps the beginner `open -> do -> see` loop and expert control, but moves domain breadth above the foundation. It also keeps e14 developer-experience rulings in force, limits this rescope to e15, and does not create an I9 carve-out. The replacement note and its evidence tree are the current record; this history note does not add a ballot or claim that any implementation or benchmark is complete.

## Evidence and raw payload preservation

The retained global report is [`SUMMARY.md`](../audits/domain-matrix-2026-09-02/SUMMARY.md). It records the 25 widest mechanisms, 27 defect groups, gate memberships, parse/schema findings, and the rule that overlap candidates do not adjudicate merges. The 11 family reports are under [`families/`](../audits/domain-matrix-2026-09-02/families/).

The compressed payloads remain byte-for-byte historical inputs:

| Payload | Records or shape | Schema fields preserved |
|---|---|---|
| [`ballots.json.gz`](../audits/domain-matrix-2026-09-02/ballots.json.gz) | Top-level array of 46 | `id`, `cardId`, `status`, `outcome`, `rec`, `options`, `comparisons`, `recommendation`, `reviewPasses`, `story`, `lesson`, `gist`, and the reading-surface fields |
| [`mechanisms.json.gz`](../audits/domain-matrix-2026-09-02/mechanisms.json.gz) | Object: 127 `mechanisms`, 108 `domainMatrix` rows, 27 `defects`, 67 `overlapCandidates` | Mechanism IDs, families, domains, counts, existing cards, definitions, state notes; domain personas/incumbents/P0 and gate counts; defect diagnostics, evidence, impact, and status |
| [`census.jsonl.gz`](../audits/domain-matrix-2026-09-02/census.jsonl.gz) | 5,698 JSONL feature rows | `domain`, `feature`, `tool`, `why_great`, `needed`, `mechanism`, `mechanism_family`, `priority`, `owner_gate`, `jet_state`, `jet_evidence`, `evidence`, and `beat_vector` |
| [`claims.jsonl.gz`](../audits/domain-matrix-2026-09-02/claims.jsonl.gz) | 3,171 JSONL source-linked claims | `claim`, `topic`, `domain`, `source_id`, `source_ref`, `source_identity`, `source_kind`, `locator`, `jet_evidence`, `correction`, `stance`, `confidence`, `classification`, `owner`, and `action` |
| [`performance.jsonl.gz`](../audits/domain-matrix-2026-09-02/performance.jsonl.gz) | 108 JSONL domain workloads | `domain`, `workload`, `dataset`, `incumbent`, `implementation`, `published_numbers`, `why_fast`, `gauntlet_cell`, and `jet_win_must_show` |

The raw payload recounts also show 3,069 high-confidence, 99 medium-confidence, and three low-confidence claims. Their stances are 3,108 supports, 51 neutral, and 12 disputes. These counts describe the archived evidence ledger; they are not current implementation or performance claims.

The proposal reported that 96 of 108 domains had no gauntlet coverage. The other 12 named only adjacent generic cells such as `numerics.float-kernel`, `netserv`, `embedded.kernel`, or `regex-logscan`; no domain had a matched cell. The performance records retain each incumbent, workload, published number, and required Jet proof so later work can build the right fixture instead of reusing an easier cell.

The source corpus was mined on 2026-09-01 and 2026-09-02 by Luna max workers. Per-domain reports and probes lived under `~/.cache/jet-luna/dx2/`; that machine-local working set is not a repository artifact. The archive records the sources and evidence cited by each row, but no additional verification is implied here.

## Exact source recovery

The preservation source is the source-only Git tag below. It is not a build or release proof:

```text
cleanup-source-2026-09-05-114107
6875b5e07a55900a630f280c9c88edbfca2094b1
```

Recover the retired proposal or any protected report with the exact command:

```sh
git show cleanup-source-2026-09-05-114107:docs/proposals/domain-matrix.md
git show cleanup-source-2026-09-05-114107:docs/research/domain-matrix-archive/SUMMARY.md
git show cleanup-source-2026-09-05-114107:docs/research/domain-matrix-archive/families/<family>.md
git show cleanup-source-2026-09-05-114107:docs/research/domain-matrix-archive/ballots.json.gz > ballots.json.gz
git show cleanup-source-2026-09-05-114107:docs/research/domain-matrix-archive/census.jsonl.gz > census.jsonl.gz
git show cleanup-source-2026-09-05-114107:docs/research/domain-matrix-archive/claims.jsonl.gz > claims.jsonl.gz
git show cleanup-source-2026-09-05-114107:docs/research/domain-matrix-archive/mechanisms.json.gz > mechanisms.json.gz
git show cleanup-source-2026-09-05-114107:docs/research/domain-matrix-archive/performance.jsonl.gz > performance.jsonl.gz
```

The local identity manifest is `local://cleanup-source-identities.json`. It records the source blob IDs, modes, and byte lengths before retirement. The cleanup did not overwrite a changed source: the owned paths matched this tag before retirement.

## Retained defect obligations

The nine retained substrate defects are not domain feature approvals and are not closed by this note:

| Card | Defect | Diagnostic | Retained obligation |
|---:|---|---|---|
| #2639 | `autodiff-higher-order` | E0112, E0102 | Repair the higher-order derivative fixture and preserve source-spanned derivative evidence. |
| #2640 | `core-sys-posix-example-drift` | E0305, E0307, E0107, L0520 | Repair the POSIX example against current `Int`, `Unit`, and `[Int]` contracts. |
| #2641 | `e0109-explanation-context-drift` | E0109 | Reconcile the live `Int`/`String` error with the `U8`/`I8` explanation. |
| #2642 | `e0405-crypto-auth-examples` | E0405 | Repair crypto and auth examples that use obsolete value-carrying returns. |
| #2643 | `e0956-evaluator-boundary` | E0956 | Reconcile default-tier lazy, query, and `GameScene` evaluator boundaries. |
| #2644 | `e2105-prove-hash` | E2105 | Repair running-compiler identity hashing so proof and replay producers can be reached. |
| #2645 | `ice-unit-formatting-2737` | ICE at `expressions.rs:2737:14` | Repair the quantity formatting ICE in chemical and FEA paths. |
| #2646 | `jit-bad-handle-geoscience` | ICE `jit list len: bad handle` | Repair the JIT waveform-table handle failure. |
| #2647 | `sqlite-blob-null` | SQLITE-BLOB-NULL | Preserve SQLite BLOB bytes instead of mapping them to NULL. |

These are the only cards retained from the 219-card slate. The Tower board remains the authority for their live status.

## Retirement map

- Retire `docs/proposals/domain-matrix.md`; it was superseded and its historical meaning is captured here and recoverable from Git.
- Retire `docs/research/domain-matrix-archive/README.md`; its owner ruling, contents, provenance, counts, and replacement boundary are captured here.
- Move the final global report, all 11 family reports, and all five compressed payloads intact to `docs/audits/domain-matrix-2026-09-02/`.
- Do not treat the audit copies as a plan or as current runtime evidence.
- Update external readers from the old paths to this note and the audit copies. The current foundations note remains untouched by this slice; its old-path migration is an integration responsibility.
