# Platforms-infrastructure family synthesis

**Family verdict.** Evolve. The 17 domains share a typed substrate for network, storage, graph, package, authority, and evidence facts. The family is not yet a stock infrastructure platform: distributed delivery, consensus, edge bindings, native desktop/OS, API contracts, content models, standard telemetry, remote execution, and measured gauntlets remain open.

**Corpus.** 17 domains; 423 P0 rows; 299 P0 owner-gated rows. Every P0 row is retained in domains.json; mechanism assignments are retained in mechanisms.json.
**Mechanisms.** 25 mechanism records: 13 reused IDs and 12 new family seams.

## Mechanism map

| Mechanism | Domains | P0 rows | State | Gate |
|---|---|---:|---|---|
| M-NETWORKING — Typed network and service protocol boundaries | databases-storage-engines, networking-protocols, telecom-5g, containers-orchestration, os-kernels, serverless-edge, messaging-eventing, distributed-systems | 34 | mixed | public-api |
| M-STORAGE — Durable stores, caches, and content identity | databases-storage-engines, telecom-5g, devops-iac-cloud, os-kernels, serverless-edge, browser-extensions, cms-ecommerce, distributed-systems | 43 | mixed | stdlib-dependency |
| M-WORKFLOW-DAG — Inspectable workflow graphs and execution plans | devops-iac-cloud, containers-orchestration, build-systems-monorepo, serverless-edge | 35 | mixed | command |
| M-PACKAGING — Package, dependency, and native-provider boundary | devops-iac-cloud, containers-orchestration, os-kernels, compilers-language-tooling, build-systems-monorepo, wasm-plugins-sandboxing, desktop-apps, browser-extensions | 35 | mixed | stdlib-dependency |
| M-EVIDENCE — Inspection, diagnostics, and reproducible evidence | devops-iac-cloud, compilers-language-tooling, observability-platforms | 20 | mixed | invariant |
| M-VALIDATION-RECEIPTS — Cross-tier validation, reproducibility, and evidence receipts | databases-storage-engines, containers-orchestration, os-kernels, compilers-language-tooling, build-systems-monorepo, wasm-plugins-sandboxing, serverless-edge, desktop-apps, browser-extensions, distributed-systems, api-design-contracts | 16 | mixed | invariant |
| M-DATA-TABLES — Typed tabular, relational, and columnar data boundary | desktop-apps, cms-ecommerce, distributed-systems | 7 | mixed | public-api |
| M-QUERY-PLANNER — Analytical SQL, typed plans, and physical execution | databases-storage-engines, cms-ecommerce | 5 | mixed | public-api |
| M-REALTIME-STREAMS — Bounded real-time audio and device streams | telecom-5g | 1 | real-gap | public-api |
| M-PLUGIN-ABI — Capability-scoped media/audio plugin ABI and lifecycle | wasm-plugins-sandboxing | 7 | mixed | public-api |
| M-DURABLE-WORKFLOWS — Durable workflows, queues, replay, and recovery | serverless-edge | 1 | real-gap | public-api |
| M-DURABLE-STREAMS — Durable streams, consumer ownership, and delivery recovery | serverless-edge, messaging-eventing | 19 | real-gap | public-api |
| M-CONSENSUS-LOG — Replicated logs, quorum commit, and durable distributed state | distributed-systems | 6 | real-gap | invariant |
| M-CLUSTER-SIM — Deterministic cluster simulation, nemeses, and replay | distributed-systems | 5 | real-gap | command |
| M-API-CONTRACT — Executable API contracts, schemas, bindings, and compatibility | networking-protocols, telecom-5g, devops-iac-cloud, compilers-language-tooling, browser-extensions, cms-ecommerce, messaging-eventing, distributed-systems, api-design-contracts | 50 | mixed | public-api |
| M-OTEL-EXPORT — Telemetry signals, standard export, and query interoperability | observability-platforms | 10 | mixed | stdlib-dependency |
| M-REMOTE-CACHE — Content-addressed remote cache and capability-matched execution | devops-iac-cloud, build-systems-monorepo | 7 | mixed | stdlib-dependency |
| M-SYNTAX-TREE — Incremental lossless syntax trees and structural queries | compilers-language-tooling | 3 | mixed | syntax |
| M-PLUGIN-CAPS — Explicit capability imports, limits, and authority-scoped extensions | devops-iac-cloud, containers-orchestration, os-kernels, compilers-language-tooling, wasm-plugins-sandboxing, serverless-edge, browser-extensions | 25 | mixed | invariant |
| M-EDGE-BINDINGS — Typed edge handlers, deployment bindings, and invocation context | serverless-edge, browser-extensions | 11 | real-gap | public-api |
| M-DESKTOP-SHELL — Native desktop shells, windows, controls, dialogs, and lifecycle | desktop-apps | 19 | mixed | ui |
| M-CONTENT-MODEL — Typed content models, editorial state, commerce records, and media pipelines | cms-ecommerce | 20 | mixed | public-api |
| M-OS-SERVICES — Authority-bound operating-system services | telecom-5g, devops-iac-cloud, containers-orchestration, os-kernels | 32 | mixed | public-api |
| M-CONNECTION-POOL — Bounded connection and instance pools | databases-storage-engines, wasm-plugins-sandboxing | 2 | real-gap | public-api |
| M-DISTRIBUTED-STATE — Typed replicated state, CRDT merge, and synchronization | distributed-systems | 10 | mixed | public-api |
## Domain matrix

| Domain | Persona/incumbent | P0 | Performance incumbent | Gauntlet | Biggest beat vector |
|---|---|---:|---|---|---|
| databases-storage-engines | A database and platform engineer starts with an embedded store, then needs durable transactions, indexes, analytics, pooling, and an honest path to distributed backends. | 31 | RocksDB C++ LSM engine and TigerBeetle Zig ledger, with bundled SQLite as Jet's current embedded baseline | none | One typed store boundary can cover embedded, server, and analytical providers without changing row or error meaning. |
| networking-protocols | A network and service engineer builds APIs, proxies, RPC systems, and packet paths, then operates deadlines, identity, flow control, and high-rate transports. | 23 | DPDK poll-mode drivers for packet forwarding, io_uring for Linux asynchronous I/O, Envoy for HTTP/gRPC proxying, and Cloudflare quiche for QUIC/HTTP/3 | netserv.client + netserv.service (HTTP client and long-lived HTTP JSON service); no HTTP/2/QUIC or packet-Mpps cell | One typed network vocabulary can carry HTTP, RPC, deadlines, cancellation, and transport identity across tiers. |
| telecom-5g | A telecom engineer operates 5G core and SIP systems, where protocol state, subscriber identity, media timing, and recovery must remain observable. | 19 | Open5GS C 5G-core control plane plus Kamailio 5.7 C SIP proxy/registrar; this performance cell measures core/signaling/SIP | none | Protocol state and service topology can share typed contracts instead of a second telecom runtime. |
| devops-iac-cloud | A platform engineer owns many environments and wants a graph query, affected plan, cache summary, drift explanation, and reversible apply before touching infrastructure. | 37 | Terraform/OpenTofu plan-and-apply engine, with Pulumi as the typed-program IaC comparator | none | The resource graph is one inspectable typed plan rather than a wrapper around shell scripts. |
| containers-orchestration | A platform operator ships services as replaceable workloads, values a fast local loop, and needs health, identity, rollout, rollback, and density evidence. | 20 | Firecracker microVM VMM for isolated workloads; crun OCI runtime for container startup | none | One workload record can project OCI, microVM, and JetOS backends without hiding lifecycle differences. |
| os-kernels | An OS engineer builds kernels and first-party services, then proves ABI, scheduling, memory, filesystems, networking, isolation, and rollback on a named target. | 24 | Linux kernel on x86_64 (CFS scheduler plus VFS); the published baseline below is Linux/i686 with EXT2FS from the lmbench paper, not a modern host measurement. | none | Target facts, service effects, and artifact identity can be checked before native execution. |
| compilers-language-tooling | A compiler and tooling engineer owns a large codebase, editor loop, incremental rebuilds, diagnostics, and backend stability across interactive and batch users. | 28 | LLVM/Clang plus MLIR optimized native pipeline; rustc/Cranelift for fast compilation and JIT; clangd for editor latency | none | One front end and typed TIR can feed editor, interpreter, JIT, and AOT without semantic forks. |
| build-systems-monorepo | A build and release engineer supports many language teams and needs an explicit target graph, affected selection, hermetic actions, cache evidence, and repairable workers. | 20 | Bazel remote-cache/CAS plus remote-execution workers for a mixed-language monorepo; Buck2 with DICE, hermetic actions and remote execution is the closest graph-oriented peer. Nx, Turborepo and Gradle are task-cache comparators. | none | The graph, action identity, policy, and receipt can be one typed build fact rather than language-specific scripts. |
| wasm-plugins-sandboxing | A platform engineer packages trusted code with untrusted extensions and needs typed imports, resource limits, lifecycle, distribution, and diagnostic receipts. | 23 | Wasmtime Component Model host, with Wasmer runtime and Extism as application-facing comparators; mlua/Lua is the dynamic-script baseline. | none | Typed component boundaries can make plugin imports and exports ordinary checked package facts. |
| serverless-edge | A product or platform engineer ships a small globally reachable handler and cares about cold starts, tail latency, locality, retries, secrets, and rollback. | 28 | Cloudflare Workers isolates with Workers KV and Durable Objects; Fastly Compute Wasmtime is the portable Wasm comparator and AWS Lambda is the cold-start baseline. | none | A handler, binding, authority, limit, and invocation can be one typed record across local and hosted tiers. |
| desktop-apps | An application developer builds a document editor, data tool, utility, or agent and expects a runnable native window, controls, files, tray actions, and installable artifacts. | 28 | Qt 6 Quick/QML with Qt Quick Controls 2, TableView backed by QAbstractTableModel, QFileDialog, and QSystemTrayIcon | none — no desktop-app cell was found under .agents/skills/gauntlet or gauntlet/ | One checked UI graph can keep Null, TUI, GTK, web, and future native hosts semantically aligned. |
| browser-extensions | An extension or userscript author needs manifest and context generation, least-privilege permissions, fast reload, durable state, and cross-browser release evidence. | 22 | Chromium and Firefox WebExtensions runtimes, with WXT/Plasmo plus web-ext as the build and reload baseline | none | One typed context and permission record can make browser boundaries visible instead of implicit JavaScript globals. |
| cms-ecommerce | A full-stack developer supports editors, merchandisers, marketers, and operations staff, and needs content, admin, preview, APIs, catalog, cart, and safe checkout workflows. | 30 | Shopify Storefront Cart API behind a Hydrogen storefront; Medusa and Saleor are secondary open-commerce comparators for the same catalog and checkout workload. | none — gauntlet/measurement-manifest.json:9-37 has generic http-client, http-service, and web-app entries but no CMS, catalog, cart, checkout, or content-migration cell. | Content shape, policy, route, and publication can share one typed graph without a second schema language. |
| messaging-eventing | A platform engineer publishes events, operates consumers, repairs poison messages, evolves schemas, and needs retention, replay, lag, security, and exactly-once boundaries. | 20 | Redpanda and Apache Kafka broker pair; registry workload is publish/subscribe throughput and latency with exactly-once delivery | none | One typed event vocabulary can span local hooks and external brokers without hiding delivery state. |
| distributed-systems | A distributed-systems engineer builds replicated state machines, collaborative documents, storage layers, and failure harnesses where recovery and causality matter more than a thin API. | 26 | FoundationDB distributed ordered key-value store | none — gauntlet/matrix.json and gauntlet/measurement-manifest.json have no distributed, consensus, CRDT, FoundationDB, or TigerBeetle cell. | Typed state, protocol phases, and receipts can make causal and recovery behavior visible in ordinary source. |
| observability-platforms | An application or platform engineer needs a symptom to become searchable metrics, logs, traces, profiles, or SLO facts without copying identifiers between tools. | 18 | Prometheus TSDB and VictoriaMetrics | none | One typed observation protocol can join source, effect, metric, log, trace, and UI evidence. |
| api-design-contracts | A platform or product engineer owns public HTTP, GraphQL, RPC, or event contracts and needs generated bindings, validation, evolution checks, mocks, and consumer evidence. | 26 | Protocol Buffers C++ generated messages with protoc and libprotobuf, paired with gRPC for the service-contract path | none | A contract can be one typed fact feeding documentation, validation, clients, tests, and operator inspection. |

## Owner gates

| Domain | Gate decisions | Suggested decision |
|---|---:|---|
| databases-storage-engines | 3 | D-PLAT-DATABASES-STORAGE-ENGINES-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |
| networking-protocols | 3 | D-PLAT-NETWORKING-PROTOCOLS-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |
| telecom-5g | 3 | D-PLAT-TELECOM-5G-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |
| devops-iac-cloud | 3 | D-PLAT-DEVOPS-IAC-CLOUD-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |
| containers-orchestration | 3 | D-PLAT-CONTAINERS-ORCHESTRATION-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |
| os-kernels | 3 | D-PLAT-OS-KERNELS-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |
| compilers-language-tooling | 3 | D-PLAT-COMPILERS-LANGUAGE-TOOLING-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |
| build-systems-monorepo | 3 | D-PLAT-BUILD-SYSTEMS-MONOREPO-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |
| wasm-plugins-sandboxing | 3 | D-PLAT-WASM-PLUGINS-SANDBOXING-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |
| serverless-edge | 3 | D-PLAT-SERVERLESS-EDGE-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |
| desktop-apps | 3 | D-PLAT-DESKTOP-APPS-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |
| browser-extensions | 3 | D-PLAT-BROWSER-EXTENSIONS-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |
| cms-ecommerce | 3 | D-PLAT-CMS-ECOMMERCE-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |
| messaging-eventing | 3 | D-PLAT-MESSAGING-EVENTING-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |
| distributed-systems | 3 | D-PLAT-DISTRIBUTED-SYSTEMS-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |
| observability-platforms | 3 | D-PLAT-OBSERVABILITY-PLATFORMS-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |
| api-design-contracts | 3 | D-PLAT-API-DESIGN-CONTRACTS-BOUNDARY1: choose the public contract, provider scope, authority, and proof required before implementation is counted. |

## Top mechanisms and beats

- **M-WORKFLOW-DAG:** one graph/plan/apply/evidence path covers IaC, workloads, deployment, placement, and service topology; do not mint a resource-graph duplicate.
- **M-API-CONTRACT:** executable identity, schema, bindings, validation, and compatibility are the missing public seam across APIs, RPC, messaging, telecom, browser, and compiler tooling.
- **M-DURABLE-STREAMS / M-CONSENSUS-LOG:** delivery recovery and quorum state are separate mechanisms; neither follows from local events, CRDTs, or typed HTTP.
- **M-VALIDATION-RECEIPTS:** every performance file is copied without a Jet number, and no platform domain currently has a registered gauntlet cell.

## Contradictions and defect boundaries

- **Existing mechanism titles differ.** M-SYNTAX-TREE and M-OS-SERVICES now reuse IDs found in tools-productivity. M-DATA-TABLES is “typed tables, records, and result schemas” in one prior family and “typed tabular, relational, and columnar data boundary” in another. M-STORAGE also has two prior titles. This file uses the neutral titles above and reuses IDs.
- **Graph naming.** DevOps and containers use “resource graph,” “application graph,” and “workload graph.” They map to M-WORKFLOW-DAG; a new M-RESOURCE-GRAPH would fragment one plan/evidence law.
- **Protocol versus domain package.** Telecom rows need 5G/SIP state and codecs; generic core.net/core.http only supply transport substrate. The E1001 core.sip probe is a real missing package boundary.
- **Evaluator parity.** Shared data rows report E0956 for lazy/query operations. This is retained as a cross-family evaluator defect, not relabeled as a platform probe or solved by a source declaration.
- **Authority negatives.** DB, Browser, and Log E1803 probes are expected fail-closed behavior. They show correct refusal and do not count as positive integration evidence.
- **Explicit-file diagnostics.** E2389–E2392 not-applicable rows in source-check artifacts are proof-scope markers, not implementation defects.

## Defects

See defects.json for 10 evidence-backed rows, including E1001, E0981, E1803, E2105, E0150, E0405, E2101, and shared E0956 evaluator boundaries.

## Source ledger

Census and performance sources are the 17 directories under ~/.cache/jet-luna/dx2/. Probe paths are retained verbatim in defects.json. No builds, Tower writes, or repository changes were performed.
