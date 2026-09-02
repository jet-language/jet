# Finance and business family synthesis

| Mechanism | Status | Reuse | Domains | Gate |
|---|---|---|---:|---|
| `M-DATA-TABLES` | mixed | reused | 8 | public-api |
| `M-UNITS` | mixed | reused | 8 | syntax |
| `M-EXACT-NUMERIC` | mixed | reused | 6 | none |
| `M-DISTRIBUTIONS` | mixed | reused | 3 | public-api |
| `M-TYPED-MODEL-RESULT` | real-gap | reused | 3 | public-api |
| `M-OPTIMIZATION` | mixed | reused | 3 | syntax |
| `M-VALIDATION-RECEIPTS` | mixed | reused | 8 | invariant |
| `M-SIM-EVENTS` | real-gap | reused | 4 | public-api |
| `M-LINALG` | mixed | reused | 3 | public-api |
| `M-GPU-KERNELS` | mixed | reused | 2 | public-api |
| `M-DURABLE-WORKFLOWS` | ratified-in-progress | new | 7 | public-api |
| `M-DOUBLE-ENTRY-LEDGER` | real-gap | new | 4 | invariant |
| `M-WORKBOOK-FORMULAS` | real-gap | new | 4 | public-api |
| `M-CONNECTORS-WEBHOOKS` | mixed | new | 7 | stdlib-dependency |
| `M-FINANCE-CODECS` | mixed | new | 7 | stdlib-dependency |
| `M-CALENDARS-BUSINESS-DATES` | mixed | new | 8 | public-api |
| `M-RULES-DECISIONS` | real-gap | new | 7 | syntax |

| Domain | Persona | Incumbent DX | P0 | Performance incumbent | Gauntlet | Biggest beat vector |
|---|---|---|---:|---|---|---|
| `quant-trading` | Quant trader | QuantConnect and kdb+ | 37 | kdb+ and hand-tuned C++ | none | Explicit time, order, fill, and strategy replay |
| `risk-actuarial` | Actuary or risk engineer | R actuar | 25 | FIS Insurance Risk Suite / Prophet | none | Explicit money, missingness, assumptions, and seeds |
| `econometrics` | Applied econometrician | Stata | 28 | R fixest and Julia | none | One typed model ledger for sample, inference, and assumptions |
| `spreadsheets-business-logic` | Finance or operations analyst | Excel | 27 | Excel calculation engine | none | A typed dependency graph with calculation receipts |
| `accounting-erp` | Accountant or ERP implementer | Odoo | 26 | SAP HANA 2.0 SPS07 | none | Balanced, immutable, auditable business facts |
| `fintech-payments` | Payments engineer | Stripe SDK and docs | 40 | TigerBeetle | none | Typed money, authorization, provider state, and ledger invariants |
| `business-rules-workflows` | Business-process engineer | Temporal | 25 | Drools KIE and Temporal Go | none | One typed workflow ledger for source, history, and effects |
| `low-code-automation` | Automation builder or operator | n8n and Retool | 36 | n/a (integration latency) | none | A source-backed typed graph with receipts for every effect |

## Family verdict

The family has 244 P0 census rows and 17 shared mechanisms (10 reused, seven new). The eight miners mark 174 P0 rows with owner gates. All eight domains lack a gauntlet cell, so every speed claim remains a target rather than a result. The first design should therefore join semantics and evidence before adding vendor breadth: one fact/result ledger, one explicit capability boundary, and one matched fixture per workload.


## Shared mechanism reading

`M-DATA-TABLES`, `M-UNITS`, and `M-VALIDATION-RECEIPTS` span all eight domains. They are the family floor, not new finance features. `M-EXACT-NUMERIC` protects scale and precision across six domains. `M-LINALG` and `M-GPU-KERNELS` connect the tensor substrate to quant, risk, and econometrics, but finance workloads still need authority, precision, and parity receipts. `M-DISTRIBUTIONS`, `M-TYPED-MODEL-RESULT`, and `M-OPTIMIZATION` connect the scientific substrate to risk, quant, and econometrics, but the result and estimator contracts remain gaps. `M-SIM-EVENTS` supplies deterministic event vocabulary, yet it does not supply exchange replay or workflow operator semantics.

The seven new candidates cover the missing business layer. `M-DURABLE-WORKFLOWS` unifies accounting queues, payment webhooks, workflow history, and low-code recovery. `M-DOUBLE-ENTRY-LEDGER` makes balanced posting a typed invariant. `M-WORKBOOK-FORMULAS` treats a workbook as a dependency graph and calculation receipt, not a record struct. `M-CONNECTORS-WEBHOOKS` makes external effects typed and capability-scoped. `M-FINANCE-CODECS` separates generic codecs from FIX, ISO 20022, BPMN/DMN, and XLSX conformance. `M-CALENDARS-BUSINESS-DATES` adds exchange, fiscal, banking, and workflow calendars over explicit time values. `M-RULES-DECISIONS` covers tax, coverage, provider state, DMN, and automation policy without inventing an unchecked second language.

## Owner gates

The counts below are P0 census rows marked `owner_gate: true`; one proposed decision can cover several rows. The orchestrator may rename these IDs.

| Suggested decision | Scope | Source rows covered | Decision needed |
|---|---|---:|---|
| `D-FB-FOUNDATION1` | Finance module and authority boundary | 17 | Choose which shared typed facts become finance Core, package, or provider surfaces. |
| `D-FB-MONEY1` | Decimal, currency, rates, and rounding | 19 | Set exact-money defaults, nominal currency rules, FX provenance, and domain rounding law. |
| `D-FB-DATA1` | Tables, plans, schemas, missingness, and bounds | 22 | Set the common table/plan contract and default-tier parity requirement. |
| `D-FB-TIME1` | Exchange, fiscal, banking, and workflow calendars | 14 | Choose versioned calendar adapters and no-future/time-zone behavior. |
| `D-FB-MODELS1` | Quant, actuarial, and econometric models | 20 | Choose model namespaces, result records, estimator scope, and diagnostics obligations. |
| `D-FB-LEDGER1` | Double-entry posting and reconciliation | 23 | Ratify journal, transfer, invoice, payment, lock, and immutable-correction invariants. |
| `D-FB-WORKBOOK1` | Workbook graph and formula engine | 18 | Choose workbook artifact, formula scope, recalc law, and XLSX/ODS boundary. |
| `D-FB-INTEROP1` | Connectors and finance codecs | 26 | Choose provider, FIX, ISO 20022, BPMN/DMN, Power Query, and COM package boundaries. |
| `D-FB-WORKFLOW1` | Durable workflow and recovery | 21 | Set typed tree, task queue, activity effects, retries, replay, and human approval law. |
| `D-FB-RULES1` | Rules, policy, and decision tables | 14 | Choose native policy scope versus DMN/FEEL/DRL adapters and provenance rules. |
| `D-FB-AUTOMATION1` | Low-code graph and operator surface | 18 | Require one source-backed graph and define trigger, mapping, redaction, and recovery scope. |
| `D-FB-AUDIT1` | Audit, authority, and evidence | 16 | Require immutable before/after facts, secret redaction, explicit effects, and replay receipts. |
| `D-FB-GAUNTLET1` | Matched performance and parity corpus | 6 | Add finance fixtures and reject a win claim when correctness, memory, or tail metrics are missing. |
The table counts overlap because one P0 row can need more than one decision. The disjoint source total is 174 owner-gated P0 rows.

## Contradictions between miners

- **Deferred data plans:** `quant-trading` census row `Deferred table plan` says the plan is ratified-in-progress and its probe fails with E0956. `risk-actuarial` row `Lazy plans, joins, missingness and rolling summaries` calls the same `core.data` plan and collect path shipped; `spreadsheets-business-logic` and `low-code-automation` still report default-tier reader and collect gaps. Resolution: the source-level Prelude exists, but default-tier execution is mixed and must not be counted as parity.
- **Money and currency:** `quant-trading`, `risk-actuarial`, `spreadsheets-business-logic`, `accounting-erp`, and `fintech-payments` rows report Decimal and nominal currency types as shipped. `accounting-erp` row `Multi-currency posting and gain/loss treatment` and `fintech-payments` rows `IBAN/BIC/cross-field validation` report missing rates, revaluation, and standards rules. Resolution: exact values and unit labels are the foundation; accounting and payment semantics remain gaps.
- **Workflow durability:** `business-rules-workflows` row `Durable event history/replay` has a passing replay probe, while its `Workflow-as-code definition`, activities, and task queues remain incomplete. `low-code-automation` rows `Replay errored/completed runs` and `Incomplete executions`, plus `accounting-erp` row `Durable delayed/bulk job queue`, remain gaps or plans. Resolution: the history primitive ships; typed workflow trees, queues, and operator recovery do not.
- **Database boundary:** `fintech-payments` row `Database transaction and migration receipts` calls the policy-bound transaction and migration primitive shipped. `accounting-erp` rows `Typed SQL sinks DB.Read/DB.Write` and `Migration generation/preview/status/apply/rollback` remain ratified or incomplete, with an E1803 refusal probe. Resolution: a generic database primitive is not a typed accounting or payment ledger sink.
- **Statistics and models:** `risk-actuarial` rows describe generic distributions and compute as present but report no actuarial model. `econometrics` row `Lazy filtering and sorting before estimation` is shipped, but `Econometrics model namespace` fails E1001 and no result contract exists. Resolution: numeric and data substrates do not establish fitters, model results, or domain diagnostics.
- **Exactly-once wording:** `fintech-payments` row `Atomic shared transfer block` preserves an exactly-once transaction-body guarantee and client-generated transfer IDs. `business-rules-workflows` and `low-code-automation` rows `Standard and Express execution profiles` and `Exactly-once delivery promise` reject an end-to-end distributed exactly-once claim. Resolution: an atomic local body and an idempotent client ID do not prove exactly-once delivery across provider, queue, and ledger.
- **Calendars and time:** `quant-trading` row `Exchange calendars and sessions` is a P0 real gap, while `accounting-erp` row `Date/period/timezone/RFC3339` and risk time rows call scalar time shipped. `business-rules-workflows` and `low-code-automation` still need timer and service-tier calendar behavior. Resolution: explicit scalar time is shipped; exchange, fiscal, banking, and operator calendars are separate mechanisms.
- **Generic codecs and standards:** `accounting-erp` row `Bounded JSON, JSONL, CSV, XML, and CBOR codecs` reports a ratified generic boundary. `fintech-payments` rows `ISO pain.001/002`, `ISO pacs.008/002`, `ISO camt.052/053/054`, and `canonical XML/namespaces`, plus quant FIX rows and spreadsheet XLSX rows, remain gaps. Resolution: generic codec support is not ISO, FIX, BPMN/DMN, or workbook conformance.

## Evidence and next measurement

The source records are under `~/.cache/jet-luna/dx2/{quant-trading,risk-actuarial,econometrics,spreadsheets-business-logic,accounting-erp,fintech-payments,business-rules-workflows,low-code-automation}/`. Probe-level defects are in `defects.json`; they include E0956 default-tier lazy execution, missing `core.math.random.poisson`, missing `core.stats`, the expected Linux COM refusal, an explicit DB authority refusal, stale fintech examples, and a float-comparison warning. A valid family gauntlet must freeze fixtures, versions, seeds, authority, output hashes, and failure schedules before measuring latency, memory, or authoring cost.
