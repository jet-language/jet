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
| Collection method rows | 354 |
| Jet-side rows | 1366 |
| Total rows | 10141 |

## Verdicts

| Verdict | Rows |
| --- | ---: |
| Jet wins | 369 |
| Equal | 442 |
| Jet loses (two or more languages agree) | 603 |
| Single witness (recorded, not scored) | 8172 |
| Exported type, not an operation | 168 |
| Not compared | 387 |
| Deliberately declined | 0 |

## Competitors

| Language | Surface read from | Recorded operations | Jet rows matched | Loss rows |
| --- | --- | ---: | ---: | ---: |
| Rust | standard-library source (rust-src component) | 997 | 230 | 97 |
| Go | official frozen API files (GOROOT/api/go1*.txt) | 1878 | 218 | 227 |
| Swift | official documentation JSON (developer.apple.com) | 505 | 98 | 82 |
| Kotlin | official API reference (kotlinlang.org) | 1141 | 157 | 122 |
| C# | official API documentation source (github.com/dotnet/dotnet-api-docs) | 1267 | 192 | 133 |
| TypeScript | runtime introspection | 360 | 128 | 39 |
| Ruby | runtime introspection | 1294 | 201 | 192 |
| Elixir | runtime introspection | 1270 | 231 | 157 |
| Julia | official documentation search index (docs.julialang.org) | 1132 | 137 | 200 |
| R | official R manual package index (stat.ethz.ch R-devel) | 1768 | 37 | 0 |
| Python | runtime introspection | 2232 | 216 | 272 |

## Loss clusters

A cluster is one container's losses. Owning a gap per container is what
the existing cards already do, so the ledger folds into them rather than
opening a second owner for the same surface. `needs_card` means no card
owns that container today, and `closed` means the card that used to owns
it is done while losses remain.

| Container | Loss rows | Prior card | Card phase | Owner |
| --- | ---: | --- | --- | --- |
| core.math | 71 | none | n/a | needs_card |
| String | 61 | #1409 | done | closed |
| core.files | 51 | #288 | building | live |
| ByteBuffer | 44 | none | n/a | needs_card |
| core.os | 44 | none | n/a | needs_card |
| core.time | 43 | none | n/a | needs_card |
| List | 27 | #1410 | done | closed |
| Map | 25 | #1410 | done | closed |
| core.tasks | 22 | none | n/a | needs_card |
| Set | 20 | #1404 | done | closed |
| Iter | 19 | #1400 | done | closed |
| core.net | 15 | none | n/a | needs_card |
| core.archive | 14 | none | n/a | needs_card |
| core.regex | 12 | none | n/a | needs_card |
| core.url | 12 | none | n/a | needs_card |
| core.crypto | 11 | none | n/a | needs_card |
| core.log | 11 | none | n/a | needs_card |
| core.sync | 11 | none | n/a | needs_card |
| Deque | 10 | none | n/a | needs_card |
| core.io | 9 | #1402 | done | closed |
| core.encoding.xml | 8 | none | n/a | needs_card |
| core.process | 8 | none | n/a | needs_card |
| core.db | 7 | none | n/a | needs_card |
| core.testing | 7 | none | n/a | needs_card |
| core.path | 6 | #288 | building | live |
| core.reflect | 6 | none | n/a | needs_card |
| core.http | 5 | none | n/a | needs_card |
| core.tls | 5 | none | n/a | needs_card |
| core.uuid | 4 | none | n/a | needs_card |
| core.binary | 3 | none | n/a | needs_card |
| core.encoding.csv | 3 | none | n/a | needs_card |
| core.random | 2 | none | n/a | needs_card |
| PriorityQueue | 2 | none | n/a | needs_card |
| core.args | 1 | none | n/a | needs_card |
| core.encoding.json | 1 | none | n/a | needs_card |
| core.mem | 1 | none | n/a | needs_card |
| core.mime | 1 | none | n/a | needs_card |
| SortedSet | 1 | #1404 | done | closed |

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
