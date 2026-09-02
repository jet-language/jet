# Tools and productivity synthesis

| Family | Domains | P0 rows | Mapped rows | Mechanisms | New ids | Reused ids | Defects |
|---|---:|---:|---:|---:|---:|---:|---:|
| tools-productivity | 9 | 238 | 222 | 20 | 12 | 8 | 5 |

| Mechanism | Title | Domain count | Owner gate / kind | e14 overlap |
|---|---|---:|---|---|
| M-OBJECT-PIPELINE | Typed object pipelines and remote session handles | 4 | yes / public-api | #2423/#2424/#2429/#2492/#2495; D-DX-PLUGIN1 |
| M-CORPUS-SEARCH | Ignore-aware corpus search and replacement | 3 | yes / public-api | #2481 measurement only; no e14 search card |
| M-STRUCTURED-FILTER | Structured filter programs over bounded streams | 3 | yes / syntax | #2421/#2471 CLI input; #2498 test selection |
| M-PARSER-COMBINATORS | Composable parser combinators with bounded recovery | 1 | yes / public-api | none |
| M-SYNTAX-TREE | Incremental lossless syntax trees and structural queries | 1 | yes / syntax | platforms-infrastructure M-SYNTAX-TREE reuse; no direct e14 card |
| M-DOCS-SITE | Documentation site scaffolds, routing, search, and versions | 1 | yes / public-api | #2426 live loop; #2439 doctest loop; #2481 measurement |
| M-LOAD-RUNNER | Scenario load runners with deterministic thresholds | 6 | yes / public-api | #2481 DX benchmark matrix |
| M-CONTRACT-MUTATION | Contract verification and mutation feedback | 2 | yes / public-api | #2439 unified test loop; #2498 browser reports |
| M-CHANNEL-ADAPTERS | Channel adapters, normalized turns, and delivery policy | 1 | yes / public-api | #2429/#2478 generic event/stream substrate only |
| M-VISUAL-STAGE | Source-backed visual stages and teaching projections | 1 | yes / ui | D-CANVAS-RAD1 ratified-in-progress; #2426/#2445/#2495 adjacent host cards |
| M-OFFICE-ARTIFACTS | Typed DOCX, PPTX, PDF, form, and mail artifacts | 1 | yes / public-api | #2481 measurement only; no office artifact card |
| M-SUPPLY-CHAIN-INTEROP | SBOM, provenance, vulnerability, and signing interoperability | 1 | yes / public-api | #2499 registry; D-BOUND-PROV1/D-PKGSIGN1; security reuses M-PACKAGING/M-VALIDATION-RECEIPTS |
| M-FOREIGN-LOCKS | Foreign lockfile graph import and conflict repair | 1 | yes / public-api | #2499 registry; D-JPK-EXTPROV1/D-JPK-OFFLINE2 |
| M-DATA-TABLES | Typed tabular, relational, and columnar data boundary | 4 | yes / public-api | #2421/#2471 typed CLI/data surfaces |
| M-NETWORKING | Typed network and service protocol boundaries | 2 | yes / public-api | #2429 generic event stream |
| M-PACKAGING | Package, dependency, and native-provider boundary | 2 | yes / stdlib-dependency | D-ECO-FILEROOT1; #2499 registry |
| M-VALIDATION-RECEIPTS | Cross-tier validation, reproducibility, and evidence receipts | 7 | yes / invariant | #2439/#2440/#2498 |
| M-WORKFLOW-DAG | Inspectable workflow graphs and execution plans | 2 | yes / command | #2441 jobs graph |
| M-STORAGE | Durable stores, caches, and content identity | 1 | yes / stdlib-dependency | #2422/#2441 job persistence |
| M-OS-SERVICES | Authority-bound operating-system services | 1 | yes / public-api | platforms-infrastructure M-OS-SERVICES reuse |

| Domain | Persona | P0 rows | Performance incumbent | Gauntlet coverage | Single biggest beat vector |
|---|---|---:|---|---|---|
| editor-ide-extensions | Jet language users who move between terminal, Neovim, VS Code, and Zed and need one semantic source with diagnostics, actions, navigation, and trustworthy extension publication. | 18 | Zed | none | One semantic registry drives diagnostics, navigation, formatting, and effect-aware tokens across hosts. |
| shell-sysadmin | Operators who inspect logs, compose data, invoke bounded local or remote processes, configure fleets, and supervise services while preserving authority, failure, and output receipts. | 25 | bash 5.x with GNU coreutils for local log pipelines; Ansible 2.x for fleet configuration; systemd 261.2 for process supervision | none; closest cells are text.report-cli, text.script, files.script, files.orchestration, concurrency.app, concurrency.service, and cli.app | Make process output a typed stream with explicit success/error and limits instead of stderr text conventions. |
| text-processing-parsing | Search, data, and language-tool users who scan repositories or multi-gigabyte streams, transform records, and recover useful structure from malformed text without runaway execution. | 23 | ripgrep (Rust regex engine plus ignore crate) | regex-logscan | Make repository search one typed, ignore-aware corpus operation with actionable spans and replacement receipts. |
| documentation-static-sites | Documentation authors who want a first page quickly, edit Markdown with live feedback, publish stable searchable versions, and keep examples and API facts tied to source. | 27 | Hugo 0.165.0 CLI with Go templates and static output | none | One package scaffold should produce content, nav, search, version, and publication artifacts with no hidden service. |
| testing-qa-automation | Test engineers who create a project, select a browser or workload, watch the first failure, replay a minimal case, and preserve contract, load, mutation, and privacy evidence. | 30 | k6 OSS local runner (`k6 run`), with Locust as the Python user-flow comparator; the registry names k6 as the performance exemplar. | none — gauntlet/measurement-manifest.json lists 22 entries and 25 cells, with no 10,000-VU, property-throughput, contract, browser-suite, or mutation workload. | One test loop should select scope, run the first failure, preserve trace, and replay the smallest counterexample. |
| chatbots-conversational | Bot builders who receive signed events, acknowledge within provider deadlines, normalize turns, maintain state, send rich responses, and survive rate limits, retries, and reconnects. | 32 | Slack Bolt JS and discord.js for platform bots; Rasa for intent/state bots; Telegram Bot API and Twilio Conversations for channel/state breadth | none (gauntlet/matrix.json covers netserv.client and netserv.service, not chatbot providers or conversation state) | One channel-adapter contract should normalize provider envelopes while retaining provider-specific fields and signatures. |
| education-teaching | Learners and teachers who need a no-install first run, immediate visual or textual feedback, clear repair guidance, a small lesson arc, and a shareable reproducible result. | 40 | Scratch 3 Web Editor with scratch-vm (registry performance exemplar is n/a) | none | One source-backed visual stage should make blocks, sprites, draw loops, events, and output inspectable rather than a separate toy runtime. |
| office-document-automation | Operations users who merge records into DOCX/PPTX/PDF, preserve layout and fields, convert or mail artifacts, and inspect deterministic output at batch scale. | 19 | PDFium (Chromium's PDF renderer) for PDF display/rendering; ReportLab and Apache PDFBox are the practical PDF generation/composition comparators. Apache POI is the registry's spreadsheet comparator but XLSX/workbook performance is delegated to spreadsheets-business-logic and is not duplicated here. | none — gauntlet/measurement-manifest.json:9-36 lists 25 cells and none is office-document/PDF/DOCX/PPTX/mail-merge. | Treat DOCX/PPTX/PDF as typed, loss-aware artifacts with explicit package, layout, field, and metadata semantics. |
| package-registries-supply-chain | Package authors and operators who resolve mixed graphs, reuse caches, approve build effects, publish signed artifacts, inspect provenance/SBOMs, and repair lock conflicts. | 24 | uv (Rust universal resolver plus cache) and pnpm 12 (content-addressed store; pnpr server-resolution benchmark) | none (gauntlet/matrix.json is absent; no package-registry entry was found) | One lock ledger should import/export foreign graphs without losing source, feature, platform, or conflict identity. |

## Family verdict
Jet is unusually strong at typed execution, diagnostics, authority, bounded processes, data codecs, HTML, notebooks, and evidence receipts, but tools-productivity users buy complete loops rather than substrates. Across nine domains, the missing value is a typed projection boundary: object streams and sessions for operators, corpus/filter/tree operations for text users, scaffold/search/version artifacts for docs, scenario/contract evidence for testing, channel adapters for bots, source-backed stages for learners, native office artifacts, and foreign package graph interoperability.
The strict family verdict is EVOLVE. Keep the existing ledger, effects, diagnostics, and receipt laws. Add the smallest domain mechanisms named above, and do not claim a performance win until each performance.json workload has a gauntlet cell and a fresh measurement.

## Owner-gate ballot slate
The 45 domain owner-gate rows collapse to the following 13 ballot-ready decisions. The orchestrator may rename these ids.
| Suggested decision id | Mechanism or e14 owner | Gate to decide |
|---|---|---|
| D-TOOLS-PIPELINE1 | M-OBJECT-PIPELINE | Typed object pipeline, remote/disconnected handles, and host session identity |
| D-TOOLS-SEARCH1 | M-CORPUS-SEARCH | Ignore-aware traversal, encoding, replacement, and offline corpus search |
| D-TOOLS-FILTER1 | M-STRUCTURED-FILTER | Typed jq-like stream/slurp/raw filters, recovery, and module boundaries |
| D-TOOLS-PARSER1 | M-PARSER-COMBINATORS | Composable parser API, zero-copy/remainder behavior, and bounded backtracking |
| D-TOOLS-DOCS1 | M-DOCS-SITE | Site scaffold, routing, markup, sidebar, search/index, versions, URLs, and publication |
| D-TOOLS-LOAD1 | M-LOAD-RUNNER | Deterministic scenarios, executors, thresholds, metrics, and missing-measurement policy |
| D-TOOLS-CONTRACT1 | M-CONTRACT-MUTATION | Pact-compatible artifacts, provider states/broker policy, mutation operators, and score gates |
| D-TOOLS-CHANNEL1 | M-CHANNEL-ADAPTERS | First providers, normalized envelope, ACK/deferred work, Retry-After, reconnect, signatures, and state |
| D-TOOLS-VISUAL1 | M-VISUAL-STAGE | Source-backed Canvas/stage/sprite/event projection and local/hosted authority |
| D-TOOLS-OFFICE1 | M-OFFICE-ARTIFACTS | DOCX/PPTX/PDF packages, layout/forms/conversion/provider boundaries, and loss reports |
| D-TOOLS-SUPPLY1 | M-SUPPLY-CHAIN-INTEROP | OSV/SBOM/SLSA/Sigstore identity, roots, provenance, and signing interoperability |
| D-TOOLS-LOCKS1 | M-FOREIGN-LOCKS | uv/Cargo/npm/pnpm/Nix/Go lock import/export and conflict repair |
| D-TOOLS-EDITORHOST1 | e14 #2494/#2495 | Neovim host/LSP setup and extension publication; map existing e14 ownership rather than minting a duplicate |

## Contradictions and boundaries
- Education has a live E0109 probe naming Int/String while the diagnostic reference explains U8/I8; both receipts are real and must converge before beginner claims.
- The shell POSIX example models Ok/Err, but the current checker exposes Int/Unit/[Int]; the example drift is not evidence that host controls are absent.
- shell-sysadmin/os_facts fails E1803 because Env is undecided by default; fail-closed authority is intentional, but the beginner scaffold must show the exact grant.
- The editor report calls #2423/#2429/#2492/#2495 ratified or in progress while the five-second LSP probe stops after hover; design ownership is not runtime proof.
- Platforms now owns canonical M-SYNTAX-TREE and M-OS-SERVICES; tools reuses those ids for text syntax and shell service/timer rows rather than minting duplicates.
- Security intentionally reuses M-PACKAGING and M-VALIDATION-RECEIPTS for package/provenance evidence; tools adds M-SUPPLY-CHAIN-INTEROP only for ecosystem standards interoperability, not a competing receipt law.
- The text report records a GNU sed retrieval timeout, but that is an evidence boundary, not a Jet runtime defect; it is separated in defects.json from the editor LSP timeout.

## Traceability
domains.json copies every P0 census row with its source feature and needed behavior. mechanisms.json records the shared mechanism assignment, local evidence, prior-art API shape, owner gate kind, e14 overlap, and existing mechanism id. Performance objects are copied from each domain performance.json. defects.json contains only observed probe failures or explicitly bounded evidence limits.
