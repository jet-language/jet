# AI/ML and data platforms family synthesis

| Mechanism | Title | Domains | Gate kind | Jet state | Reuse |
|---|---|---:|---|---|---|
| M-MODEL-STATE | Typed model/module state, parameter registries, artifacts, and checkpoints | 7 | public-api | real-gap | new |
| M-DATALOADER | Typed datasets, data loaders, batching, and feature schemas | 5 | public-api | real-gap | new |
| M-COLUMNAR | Arrow/Parquet columnar interchange and out-of-core execution | 7 | stdlib-dependency | real-gap | new |
| M-EXPERIMENT-LEDGER | Asset, lineage, run, artifact, and reproducible evidence ledger | 10 | public-api | real-gap | new |
| M-LLM-CLIENT | Provider-neutral model calls, streaming, and bounded tool loops | 2 | public-api | real-gap | new |
| M-NLP-PIPELINE | Tokenizer, document, alignment, and text annotation pipeline | 5 | public-api | real-gap | new |
| M-RETRIEVAL-INDEX | Vector and lexical retrieval indexes | 3 | public-api | real-gap | new |
| M-RL-ENV | Typed environment, rollout, and policy contract | 1 | public-api | real-gap | new |
| M-STREAM-WINDOWS | Event-time windows, keyed state, and recovery | 3 | public-api | real-gap | new |
| M-QUERY-PLANNER | Analytical SQL, typed plans, and physical execution | 5 | public-api | real-gap | new |
| M-SEMANTIC-LAYER | Dashboard semantic metrics, components, and interaction state | 3 | ui | real-gap | new |
| M-LINALG | Generic typed tensor and linear-algebra substrate | 5 | none | mixed | M-LINALG |
| M-GPU-KERNELS | GPU kernels, fusion, placement, and backend receipts | 5 | invariant | mixed | M-GPU-KERNELS |
| M-DATA-TABLES | Typed tabular, relational, and columnar data boundary | 12 | public-api | mixed | M-DATA-TABLES |
| M-WORKFLOW-DAG | Inspectable workflow graphs and execution plans | 6 | public-api | mixed | M-WORKFLOW-DAG |
| M-EVIDENCE | Inspection, diagnostics, and reproducible evidence | 12 | none | mixed | M-EVIDENCE |
| M-PLOTTING | Deterministic and interactive visualization | 5 | ui | mixed | M-PLOTTING |
| M-NOTEBOOK | Notebook and interactive REPL loop | 4 | none | mixed | M-NOTEBOOK |
| M-SCI-FORMATS | Standard file formats and interchange | 12 | stdlib-dependency | mixed | M-SCI-FORMATS |
| M-NETWORKING | Typed network and service protocol boundaries | 8 | public-api | mixed | M-NETWORKING |
| M-STORAGE | Durable stores, caches, and content identity | 8 | public-api | mixed | M-STORAGE |
| M-PACKAGING | Package, dependency, and native-provider boundary | 7 | stdlib-dependency | mixed | M-PACKAGING |
| M-VALIDATION-RECEIPTS | Cross-tier validation, reproducibility, and evidence receipts | 12 | invariant | mixed | M-VALIDATION-RECEIPTS |

| Domain | Persona | P0 gaps | Performance incumbent | Gauntlet coverage | Biggest beat vector |
|---|---|---:|---|---|---|
| ml-training | A model engineer or research engineer who turns typed tensors, datasets, model modules, optimizers, checkpoints, and distributed runs into reproducible training jobs. | 14/26 | JAX/XLA jax.jit for array programs, plus PyTorch torch.compile/Inductor in Lightning or Hugging Face training loops | none — gauntlet/matrix.json has numerics and generic program cells but no ML, transformer, ResNet, mixed-precision, or steps-per-second cell. | Keep one typed Tensor and shape law across model, data, autodiff, and accelerator paths. |
| ml-inference-serving | An ML platform engineer who packages model artifacts, selects an execution provider, serves online and offline requests, and inspects latency, memory, batching, and failure state. | 29/29 | TensorRT-LLM on NVIDIA Blackwell for accelerator serving, with llama.cpp as the CPU and quantized edge comparator | none; probes/gauntlet_coverage.txt records no ML-serving, LLM, ONNX, TensorRT, GGUF, vLLM, image-model, or tokens/s entry in gauntlet/measurement-manifest.json | Keep model artifact, tokenizer, tensor shape, provider, device, scheduler, and output agreement in one typed receipt. |
| llm-apps-agents | An application or platform engineer who builds chat, extraction, retrieval, workflow, coding, and tool-using agents with provider, authority, persistence, and spend controls. | 28/31 | Python LangGraph/LangChain plus provider SDKs, TypeScript Vercel AI SDK, DSPy, Instructor, and MCP clients/servers; the practical incumbent is provider latency and spend, not CPU throughput. | none: no llm agent latency, token, tool-overhead, or cost cell exists in the current gauntlet | Attach provider calls, tool inputs, approvals, limits, and redacted evidence to one typed value path. |
| nlp-text | An application engineer, data scientist, or computational linguist who turns raw text into normalized documents, tokens, offsets, annotations, model inputs, and searchable features. | 15/27 | Hugging Face tokenizers Rust implementation for tokenization, paired with spaCy en_core_web_lg for production CPU NER/pipeline throughput | none | Make Unicode units, offsets, normalization, token boundaries, and annotations one explicit typed document model. |
| computer-vision | A computer-vision engineer who decodes images or video, applies geometric and differentiable transforms, runs detection or classical operators, inspects results, and exports a reproducible artifact. | 20/23 | Ultralytics YOLO26n exported ONNX on CPU and TensorRT10 on an NVIDIA T4 | none | Make image layout, channel order, range, coordinates, device, and precision explicit in the Tensor contract. |
| recommender-search | A search or recommendation engineer who ingests documents and vectors, builds mutable indexes, combines lexical and semantic retrieval, filters results, and measures recall, latency, and update behavior. | 25/31 | Vespa HNSW on SPACEV-1B (primary incumbent; FAISS is a secondary implementation comparator) | none — gauntlet/matrix.json:1-71 has no recommender, vector-search, ANN or BM25 cell. Jet must add a dedicated cell and measure the same dataset or a declared equivalent before claiming parity. | Keep vector shape, metric, precision, filter semantics, index identity, and exact-reference evidence typed. |
| reinforcement-learning | An applied ML or robotics engineer who defines a reproducible environment, rolls out seeded vector episodes, trains and evaluates a policy, and resumes from a portable checkpoint. | 18/19 | MuJoCo MJX-Warp on GPU for batched physics, with Isaac Lab/Isaac Gym as the end-to-end PPO vectorized-environment incumbent | none — gauntlet/measurement-manifest.json has nbody batch coverage but no PPO or vectorized-environment steps/s cell; gauntlet/matrix.json is absent | Make environment state, spaces, reset/step outcomes, seeds, and episode termination explicit and typed. |
| mlops | An ML platform engineer who records runs, artifacts, models, labels, features, lineage, and deployments so a training result can be reproduced and served. | 32/35 | Weights & Biases tracking/artifacts plus MLflow Tracking and Model Registry, with Feast Feature Server for online features | none (no MLOps, W&B, MLflow, DVC, Kubeflow, Label Studio, or Feast cell in the current gauntlet manifest) | Make source, dataset, model, feature, run, artifact, and deployment identity one lineage ledger. |
| data-engineering-etl | A data engineer who builds schema-aware ingestion and transformations, schedules and retries materializations, selects affected partitions, and proves lineage and data quality. | 27/33 | Apache Spark 4.2.0 SQL/DataFrame for distributed transforms, with Polars lazy/streaming as the fast local baseline | none | Extend typed, bounded data meaning into lazy, columnar, incremental, and distributed paths without an untyped escape hatch. |
| stream-processing | A stream engineer who connects unbounded sources, assigns event time, maintains keyed state and windows, recovers from failure, and delivers correct results with visible progress. | 26/30 | Apache Flink DataStream/Table runtime and Arroyo Rust streaming SQL engine | none | Keep bounded streams, typed errors, cancellation, and limits as the same contract for unbounded connectors and windows. |
| big-data-analytics | An analyst or data engineer who queries files and lake tables locally, inspects plans, scales the same work across partitions, and proves a large result during an incident. | 30/35 | ClickHouse and DuckDB are the performance exemplars; Spark and Trino are distributed SQL baselines, with Dask as the Python partitioned-data baseline. | none | Carry static schema, nullability, limits, and DataError facts into columnar batches and out-of-core operators. |
| bi-dashboards | A product analyst, operations lead, or data engineer who defines a metric, filters a segment, inspects a chart/table, drills into records, and publishes a trusted view to a team. | 23/35 | ClickHouse-class analytical query execution serving Evidence or Metabase dashboard cards; Evidence, Metabase, Tableau, Looker, and Streamlit are the user-facing peers, not one interchangeable engine. | none (gauntlet/matrix.json has no bi-dashboards cell; its webfront and netserv cells are not dashboard substitutes) | Make source, schema, metric, component, filter, selection, permission, and refresh state one typed dashboard ledger. |

| Suggested decision id | Owner gate | Gate kind | Domains and census rows |
|---|---|---|---|
| D-AIML-MODELSTATE1 | Typed model/module state, parameter registries, artifacts, and checkpoints | public-api | ml-training: Neural-network activation set; ml-training: AdamW and parameter groups; ml-training: Model module and parameter registry |
| D-AIML-DATALOADER1 | Typed datasets, data loaders, batching, and feature schemas | public-api | ml-training: DataLoader batching and worker pipeline; ml-training: Typed dataset storage and feature schema; ml-training: Automatic vectorization over batches |
| D-AIML-COLUMNAR1 | Arrow/Parquet columnar interchange and out-of-core execution | stdlib-dependency | mlops: JSON, JSONL, NDJSON, and Parquet task inputs; data-engineering-etl: Streaming collection for larger-than-memory data; data-engineering-etl: Arrow and Parquet columnar interchange |
| D-AIML-EXPERIMENTLEDGER1 | Asset, lineage, run, artifact, and reproducible evidence ledger | public-api | mlops: Run context and project; mlops: Run configuration and hyperparameters; mlops: Metric logging with steps |
| D-AIML-LLMCLIENT1 | Provider-neutral model calls, streaming, and bounded tool loops | public-api | ml-inference-serving: Offline batched LLM generation; ml-inference-serving: OpenAI-compatible chat and completion API; ml-inference-serving: Ollama generate and chat API |
| D-AIML-NLPPIPELINE1 | Tokenizer, document, alignment, and text annotation pipeline | public-api | ml-inference-serving: Tokenizer and chat-template metadata; nlp-text: Out-of-box NLP namespace and pipeline entry point; nlp-text: Document object with token and annotation ownership |
| D-AIML-RETRIEVALINDEX1 | Vector and lexical retrieval indexes | public-api | recommender-search: Dense vector field declaration; recommender-search: Approximate nearest-neighbor query; recommender-search: Exact nearest-neighbor baseline |
| D-AIML-RLENV1 | Typed environment, rollout, and policy contract | public-api | reinforcement-learning: Environment factory and registry; reinforcement-learning: Reset and step lifecycle; reinforcement-learning: Terminated versus truncated episode ends |
| D-AIML-STREAMWINDOWS1 | Event-time windows, keyed state, and recovery | public-api | stream-processing: DataStream transformation graph; stream-processing: Keyed partitioning and keyed state; stream-processing: Event-time timestamps and watermarks |
| D-AIML-QUERYPLANNER1 | Analytical SQL, typed plans, and physical execution | public-api | recommender-search: Filter-aware ANN; recommender-search: YQL query API and query controls; recommender-search: One Query API for search modes |
| D-AIML-SEMANTICLAYER1 | Dashboard semantic metrics, components, and interaction state | ui | mlops: Interactive run dashboard |
| D-AIML-GPUKERNELS1 | GPU kernels, fusion, placement, and backend receipts | invariant | ml-training: Autocast and gradient scaling; ml-training: Multi-device sharding and collectives; ml-inference-serving: TensorRT-LLM prebuilt serving |
| D-AIML-DATATABLES1 | Typed tabular, relational, and columnar data boundary | public-api | mlops: Logged model and dataset links; data-engineering-etl: Incremental model predicate; data-engineering-etl: Unique keys and merge strategy |
| D-AIML-WORKFLOWDAG1 | Inspectable workflow graphs and execution plans | public-api | llm-apps-agents: Stateful graph orchestration; llm-apps-agents: Compiled graph invocation and state updates; llm-apps-agents: Checkpoint persistence and long-term store |
| D-AIML-PLOTTING1 | Deterministic and interactive visualization | ui | bi-dashboards: Declarative dashboard pages; bi-dashboards: Typed dashboard component catalog; bi-dashboards: Cross-filtering and drill-through |
| D-AIML-SCIFORMATS1 | Standard file formats and interchange | stdlib-dependency | ml-inference-serving: ONNX export, validation, and session run; computer-vision: Image file read; computer-vision: Image file write |
| D-AIML-NETWORKING1 | Typed network and service protocol boundaries | public-api | ml-inference-serving: API key, TLS, and service authentication; llm-apps-agents: MCP protocol revision negotiation; llm-apps-agents: MCP server tool discovery and calls |
| D-AIML-STORAGE1 | Durable stores, caches, and content identity | public-api | bi-dashboards: Background refresh and freshness state; bi-dashboards: Dashboard freshness and card-level observability |
| D-AIML-PACKAGING1 | Package, dependency, and native-provider boundary | stdlib-dependency | ml-inference-serving: vLLM install and first run; ml-inference-serving: Ollama one-command local model loop; computer-vision: Python binding installation and import |

## Family verdict
AI/ML and data platforms are one typed systems problem: model state, data movement, execution, external providers, and evidence must agree. Incumbent stacks win separate loops through mature model runtimes, loaders, indexes, stream engines, query planners, and dashboards. Jet already contributes a valuable typed Tensor/table/stream/effect/evidence floor, but the census shows domain gaps and no matched AI/ML gauntlet cells.
The family should reuse existing tensor, GPU, table, workflow, evidence, plotting, notebook, format, network, storage, packaging, and validation mechanisms. The new seams are model state, data loaders, Arrow/Parquet columnar execution, experiment lineage, provider-neutral model calls, NLP pipelines, retrieval indexes, RL environments, stream windows, analytical query planning, and semantic dashboard projections.

## Cross-domain contradictions and tensions
| Mechanism | Source A | Source B | Tension to resolve |
|---|---|---|---|
| M-COLUMNAR / M-QUERY-PLANNER | `data-engineering-etl/probes/lazy-refusal.receipt.txt:5-9` reports default-tier E0956, while its cache probe says `data.lazy` and `data.collect` succeed outside the repo. | `bi-dashboards` and `big-data-analytics` reports mark lazy plans as ratified-in-progress and large-query execution as unbuilt. | Treat evaluator/tier sensitivity as a defect or publish the boundary; do not count one successful cache probe as general lazy or out-of-core parity. |
| M-GPU-KERNELS | `ml-training/probes/compute_ml.authority-failure.txt:9-14` refuses GPU with E1803 when package authority is undecided. | `computer-vision/probes/compute-gpu-contrast.txt:5-7` reports CUDA unavailable and selects the CPU oracle with an explicit placement receipt. | Both are fail-closed foundations, but neither proves a positive AI model accelerator path; the future contract must distinguish authority refusal from host unavailability. |
| M-RETRIEVAL-INDEX | `recommender-search/report.md:198-200` says the typed Tensor/data/placement substrate can support a production ANN path without a second authority model. | `recommender-search/census.json` row “First-class vector retrieval item” is a real gap and `probes/gap.out:3-12` returns E1004 for `compute.search`. | Foundation reuse is plausible, but it must not be reported as an implemented index, query, persistence, or recall path. |

## Defect index
| ID | Domains | Kind | Diagnostic | Status |
|---|---|---|---|---|
| DEF-AIML-TRAIN-AUTODIFF-001 | ml-training | compiler-diagnostic | Error [E0112]: `compute.gradient` needs named Tensor parameters; Error [E0102]: nothing named `curvature` exists here. | open |
| DEF-AIML-TRAIN-GPU-AUTH-001 | ml-training | authority-diagnostic | Error [E1803]: application authority is undecided for `GPU`. | open |
| DEF-AIML-TRAIN-SHAPE-001 | ml-training | runtime-probe-failure | Stop [E3001]: `panic: unexpected success` from a matmul shape probe. | open |
| DEF-AIML-INFER-MODULE-001 | ml-inference-serving | missing-standard-library-module | Error [E1001]: there is no core module `core.inference`. | open |
| DEF-AIML-NLP-MODULE-001 | nlp-text | missing-standard-library-module | Error [E1001]: there is no core module `core.nlp`. | open |
| DEF-AIML-NLP-REGEX-001 | nlp-text | type-diagnostic | Error [E0152]: a `String` is not a `Regex` pattern. | known-boundary |
| DEF-AIML-CV-CONV-001 | computer-vision | missing-standard-library-item | Error [E1004]: `core.compute` has no item `conv2d`. | open |
| DEF-AIML-REC-SEARCH-001 | recommender-search | missing-standard-library-item | Error [E1004]: `core.compute` has no item `search`. | open |
| DEF-AIML-MLOPS-BRIDGE-001 | mlops | bridge-diagnostic | Error [E3002]: bridge `require_bridge: py.* unavailable` returned a typed `DataError`. | known-boundary |
| DEF-AIML-MLOPS-BRIDGE-EXAMPLE-001 | mlops | stale-example-contract | Errors [E0305] and [E0107]: the example pattern-matches `Ok`/`Err` against `Unit` and references undeclared `error`. | open |
| DEF-AIML-ETL-LAZY-001 | data-engineering-etl | default-tier-evaluator-gap | Error [E0956]: ``core.data.lazy()`` is not supported by the current evaluator yet. | open |

## Traceability and limits
The full P0 census rows, mechanism assignment, state, gate, evidence, and needed text are in `domains.json`; mechanism-level P0 rows are in `mechanisms.json`. Performance objects are copied from each domain `performance.json` without inventing Jet numbers. Retrieval statuses come from each domain manifest. No benchmark, build, formatter, linter, Tower write, or repository mutation was performed.
