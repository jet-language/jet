# Jet Core surface ledger

Owner ruling 2026-08-03: the bar is not Python. It is every language Jet
competes with, and a missing feature is not acceptable.

This page is the durable review index. The JSON file beside it is the
machine-readable source that card #1398 reads. Do not keep a second
hand-written workflow inventory.

Generated on: 2026-08-06

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
- `--check` rejects source drift, a competitor claim the recorded surface
  does not support, a duplicate row, a container a language silently
  skipped, an owner card that is closed or missing, and an unratified
  scope exclusion.

## Inventory

| Measure | Count |
| --- | ---: |
| Languages compared | 11 |
| Shared containers | 54 |
| Core modules | 85 |
| Module members | 1011 |
| Collection method rows | 419 |
| Jet-side rows | 1431 |
| Total rows | 10514 |

## Verdicts

| Verdict | Rows |
| --- | ---: |
| Jet wins | 332 |
| Equal | 508 |
| Jet loses (two or more languages agree) | 632 |
| Single witness (recorded, not scored) | 8451 |
| Exported type, not an operation | 168 |
| Not compared | 423 |
| Deliberately declined | 0 |

## Competitors

| Language | Surface read from | Recorded operations | Jet rows matched | Loss rows |
| --- | --- | ---: | ---: | ---: |
| Rust | standard-library source (rust-src component) | 1032 | 278 | 102 |
| Go | official frozen API files (GOROOT/api/go1*.txt) | 1878 | 253 | 229 |
| Swift | official documentation JSON (developer.apple.com) | 505 | 109 | 80 |
| Kotlin | official API reference (kotlinlang.org) | 1141 | 171 | 123 |
| C# | official API documentation source (github.com/dotnet/dotnet-api-docs) | 1267 | 227 | 133 |
| TypeScript | runtime introspection | 789 | 193 | 102 |
| Ruby | runtime introspection | 1294 | 225 | 192 |
| Elixir | runtime introspection | 1270 | 265 | 164 |
| Julia | official documentation search index (docs.julialang.org) | 1132 | 164 | 199 |
| R | official R manual package index (stat.ethz.ch R-devel) | 1768 | 42 | 0 |
| Python | runtime introspection | 2232 | 254 | 288 |

## Loss clusters

A cluster is one container's losses. Owning a gap per container is what
the existing cards already do, so the ledger folds into them rather than
opening a second owner for the same surface. `needs_card` means no card
owns that container today, and `closed` means the owning card is done
while losses remain.

| Container | Loss rows | Owner card | Card phase | State |
| --- | ---: | --- | --- | --- |
| core.math | 66 | #1464 | planning | live |
| core.files | 63 | #288 | building | live |
| String | 61 | #1476 | planning | live |
| core.os | 47 | #1465 | planning | live |
| ByteBuffer | 45 | #1467 | planning | live |
| core.time | 43 | #1466 | planning | live |
| List | 27 | #1477 | planning | live |
| Map | 25 | #1477 | planning | live |
| core.tasks | 22 | #1468 | planning | live |
| Set | 20 | #1478 | planning | live |
| core.net | 19 | #1469 | planning | live |
| Iter | 19 | #1479 | planning | live |
| core.crypto | 17 | #1473 | planning | live |
| core.archive | 15 | #1470 | planning | live |
| core.log | 13 | #1474 | planning | live |
| core.regex | 12 | #1471 | planning | live |
| core.url | 12 | #1472 | planning | live |
| core.sync | 11 | #1481 | planning | live |
| Deque | 10 | #1475 | planning | live |
| core.io | 9 | #1480 | planning | live |
| core.process | 9 | #1481 | planning | live |
| core.encoding.xml | 8 | #1481 | planning | live |
| core.path | 8 | #288 | building | live |
| core.testing | 8 | #1481 | planning | live |
| core.db | 7 | #1481 | planning | live |
| core.reflect | 6 | #1481 | planning | live |
| core.http | 5 | #1481 | planning | live |
| core.tls | 5 | #1481 | planning | live |
| core.uuid | 4 | #1481 | planning | live |
| core.binary | 3 | #1481 | planning | live |
| core.encoding.csv | 3 | #1481 | planning | live |
| core.args | 2 | #1481 | planning | live |
| core.random | 2 | #1481 | planning | live |
| PriorityQueue | 2 | #1481 | planning | live |
| core.encoding.json | 1 | #1481 | planning | live |
| core.mem | 1 | #1481 | planning | live |
| core.mime | 1 | #1481 | planning | live |
| SortedSet | 1 | #1478 | planning | live |

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

## Core domains not yet compared

No competitor surface records a container for these Core modules, so no
row scores them. They are listed so the shortfall stays countable rather
than invisible.

`app`, `core.auth`, `core.browser`, `core.compiler`, `core.compute`, `core.encoding.cbor`, `core.encoding.jsonl`, `core.event`, `core.game`, `core.lang`, `core.mem.alloc`, `core.perf`, `core.plugin`, `core.raylib`, `core.reactive`, `core.reactive.loadable`, `core.science.measurement`, `core.scope`, `core.services`, `core.sketch.cms`, `core.sketch.hll`, `core.sketch.reservoir`, `core.sketch.tdigest`, `core.solve`, `core.ui`, `core.vault`, `core.vault.expert`, `core.watcher`, `core.web.devserver`, `core.web.storage`, `core.web.storage.local`, `core.web.storage.session`, `core.ws`

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
