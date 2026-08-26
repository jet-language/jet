# Jet Core surface ledger

Owner ruling 2026-08-03: the bar is not Python. It is every language Jet
competes with, and a missing feature is not acceptable.

This page is the durable review index. The JSON file beside it is the
machine-readable source that card #1398 reads. Do not keep a second
hand-written workflow inventory.

Generated on: 2026-08-26

## What decides a row

- What Jet ships comes from the compiler tables: module_items.rs,
  fixed_sigs.rs, and Collections.rs.
- What a competitor ships comes from that language's own recorded surface,
  read from a runtime, from standard-library source, or from official
  machine-readable documentation.
- A row carries one verdict. `equal` means at least one recorded competitor
  answers the same workflow. `jet_wins` means none does. `jet_loses` is an
  operation two or more compared languages ship and Jet has no spelling for.
  `single_witness` is an operation exactly one language ships.
  `not_compared` means no surface records that container yet.
- A gap is one workflow, not one row per language. Ten languages shipping
  `sqrt` is one missing operation with ten witnesses.
- One language is not evidence. A single-witness row is almost always that
  language's own internals, such as Rust's `align_to` and `as_mut_ptr`,
  which a memory-safe language must not expose. Those rows stay in the
  ledger and stay counted, but they are recorded rather than scored.
- A gap merges by domain, so one name can still recur across domains, and
  that has two different answers. `clone` on a List and on a Map is one
  operation asked twice, so its witnesses pool across domains before the
  two-witness threshold; scoring each domain alone can hold a real gap at
  one witness forever. `close` on a byte buffer and on a database handle
  are different operations sharing a spelling, so they keep the per-domain
  count. There is no mechanical separator — the difference is what the
  operation means. Every recurring name is classified by hand in
  `scripts/agent/check-core-surface-ledger.mjs`, in `CROSS_DOMAIN_POOLED`
  or `CROSS_DOMAIN_DISTINCT`, and `--check` rejects a recurring name that
  is in neither. A row scored on pooled evidence records the pooled count
  in `pooledWitnessCount`, so it is never mistaken for its own.
- `--check` rejects source drift, a competitor claim the recorded surface
  does not support, a duplicate row, a container a language silently
  skipped, an owner card that is closed or missing, and an unratified
  scope exclusion.

## Inventory

| Measure | Count |
| --- | ---: |
| Languages compared | 11 |
| Shared containers | 51 |
| Core modules | 79 |
| Module members | 1098 |
| Collection method rows | 743 |
| Jet-side rows | 1860 |
| Total rows | 9389 |

## Verdicts

| Verdict | Rows |
| --- | ---: |
| Jet wins | 484 |
| Equal | 828 |
| Jet loses (two or more languages agree) | 119 |
| Single witness (recorded, not scored) | 7305 |
| Exported type, not an operation | 161 |
| Not compared | 387 |
| Deliberately declined | 105 |

## Competitors

| Language | Surface read from | Recorded operations | Jet rows matched | Loss rows |
| --- | --- | ---: | ---: | ---: |
| Rust | standard-library source (rust-src component) | 1032 | 342 | 11 |
| Go | official frozen API files (GOROOT/api/go1*.txt) | 1878 | 368 | 27 |
| Swift | official documentation JSON (developer.apple.com) | 505 | 183 | 4 |
| Kotlin | official API reference (kotlinlang.org) | 1141 | 245 | 9 |
| C# | official API documentation source (github.com/dotnet/dotnet-api-docs) | 1267 | 313 | 22 |
| TypeScript | runtime introspection | 724 | 210 | 30 |
| Ruby | runtime introspection | 1294 | 319 | 23 |
| Elixir | runtime introspection | 1270 | 391 | 41 |
| Julia | official documentation search index (docs.julialang.org) | 1132 | 271 | 46 |
| R | official R manual package index (stat.ethz.ch R-devel) | 1768 | 52 | 0 |
| Python | runtime introspection | 1896 | 399 | 65 |

## Loss clusters

A cluster is one container's losses. Owning a gap per container is what
the existing cards already do, so the ledger folds into them rather than
opening a second owner for the same surface. `needs_card` means no card
owns that container today, and `closed` means the owning card is done
while losses remain.

| Container | Loss rows | Owner card | Card phase | State |
| --- | ---: | --- | --- | --- |
| core.files | 38 | #288 | done | closed |
| core.tasks | 18 | #1468 | done | closed |
| core.time | 14 | #1466 | done | closed |
| core.math | 10 | #1464 | done | closed |
| core.net | 9 | #1469 | done | closed |
| core.crypto | 8 | #1725 | done | closed |
| core.log | 8 | #1474 | done | closed |
| core.archive | 4 | #1470 | done | closed |
| core.regex | 3 | #1471 | done | closed |
| Set | 2 | #1584 | done | closed |
| Bytes | 1 | #1467 | done | closed |
| core.crypto.uuid | 1 | #1590 | done | closed |
| core.process | 1 | #1590 | done | closed |
| Map | 1 | #1477 | done | closed |
| String | 1 | #1581 | done | closed |

## Containers indexed per package

These surfaces are indexed a whole package at a time, so the index can
confirm that the language documents a name but cannot place that name in
one container. They still confirm a Jet match; they do not mint a gap row,
because that would score Jet against operations the index never attributed
here. The skip is listed so it stays countable.

| Language | Container | Recorded operations |
| --- | --- | ---: |
| R | core.data | 592 |
| R | core.math | 1176 |

## Competitive Core API gate

The release gate is owned by card #1398 and attached to docs/spec/stdlib-api-laws.md at `## Competitive Core API gate`. Python is the calibration arm;
the claim covers all 11 recorded competitor languages.

| Gate measure | Value |
| --- | ---: |
| Workflow manifest entries | 9389 |
| Beginner cases | 9389 |
| Expert-policy cases | 451 |
| Failure cases | 1366 |
| Lifecycle cases | 675 |
| Pending evidence records | 9389 |
| Measured evidence records | 0 |
| Accepted Jet wins | 9389 |
| Release status | `blocked-until-evidence-complete` |

Every ledger row has one frozen task record with the same input and outcome
across language arms, allowed dependencies, tool versions, source boundary,
and competing workflow. Design declines remain scored; only ratified scope
decisions may exclude a workflow. The fixture contract reuses tests/agent_workloads/manifest.tsv, tests/agent_workloads/baselines/receipt.tsv,
the existing tests/agent_workloads.rs::equivalent_adapters_complete_declared_tasks, and the recorded #769
scoring contract. Raw source counts are evidence, not a universal ratio.
Incidental ceremony fails; accepted extra constructs need a clarity, local
reasoning, named guarantee, or expert-control benefit and an independent
fixture review. Every workflow also has one source-derived machine Jet win:
operation coverage, a typed contract, or an explicit fail-closed diagnosis.
Fixture selection is one accepted review row per frozen task, bound to the
adapter-source, input, and expected-output digests. The existing runner's
pinned tools, cold and warm runs, exact stdout, unchanged-input, and clean
scratch checks are revalidated from the receipt and artifact files.
A fresh-context release check reviews workflow closure, construct
classifications, reasoning evidence, syntax coverage, and fixture selection.

Run the structural check and the fail-closed release check:

~~~sh
node scripts/agent/check-core-surface-ledger.mjs --check
node scripts/agent/check-core-surface-ledger.mjs --core-api-release-check
~~~

## Core domains not yet compared

No competitor surface records a container for these Core modules, so no
row scores them. They are listed so the shortfall stays countable rather
than invisible.

`app`, `core.auth`, `core.compiler`, `core.compiler.lang`, `core.compute`, `core.compute.solve`, `core.crypto.vault`, `core.data.plot`, `core.data.sketch.cms`, `core.data.sketch.hll`, `core.data.sketch.reservoir`, `core.data.sketch.tdigest`, `core.encoding.cbor`, `core.encoding.jsonl`, `core.event`, `core.game`, `core.game.raylib`, `core.mem.scope`, `core.mod`, `core.net.ws`, `core.perf`, `core.plugin`, `core.prelude`, `core.reactive`, `core.reactive.loadable`, `core.service`, `core.ui`, `core.units`, `core.watcher`, `core.web.browser`, `core.web.devserver`, `core.web.storage`, `core.web.storage.local`, `core.web.storage.session`

## Consumer

Card #1398 reads docs/reference/core-surface-ledger.json as its only
workflow inventory.

Regenerate and check from the repository root:

~~~sh
node scripts/agent/check-core-surface-ledger.mjs --refresh
node scripts/agent/check-core-surface-ledger.mjs --check
~~~

Full rows stay in the JSON artifact so the release rubric can read
structured data without duplicating this inventory.
