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
  answers the same workflow. `jet_wins` means none does. `jet_loses` is a
  competitor operation with no Jet spelling. `not_compared` means no
  surface records that container yet.
- `--check` rejects source drift, a competitor claim the recorded surface
  does not support, a duplicate row, a container a language silently
  skipped, an owner card that is closed or missing, and an unratified
  scope exclusion.

## Inventory

| Measure | Count |
| --- | ---: |
| Languages compared | 11 |
| Shared containers | 44 |
| Core modules | 85 |
| Module members | 1011 |
| Collection method rows | 334 |
| Jet-side rows | 1346 |
| Total rows | 10577 |

## Verdicts

| Verdict | Rows |
| --- | ---: |
| Jet wins | 410 |
| Equal | 351 |
| Jet loses (two or more languages agree) | 687 |
| Single witness (recorded, not scored) | 8544 |
| Not compared | 585 |
| Deliberately declined | 0 |

## Competitors

| Language | Surface read from | Recorded operations | Jet rows matched | Loss rows |
| --- | --- | ---: | ---: | ---: |
| Rust | standard-library source (rust-src component) | 980 | 146 | 134 |
| Go | official frozen API files (GOROOT/api/go1*.txt) | 1681 | 133 | 235 |
| Swift | official documentation JSON (developer.apple.com) | 505 | 65 | 88 |
| Kotlin | official API reference (kotlinlang.org) | 1141 | 120 | 136 |
| C# | official API documentation source (github.com/dotnet/dotnet-api-docs) | 1102 | 117 | 134 |
| TypeScript | runtime introspection | 347 | 96 | 57 |
| Ruby | runtime introspection | 1209 | 130 | 201 |
| Elixir | runtime introspection | 1450 | 147 | 203 |
| Julia | official documentation search index (docs.julialang.org) | 1132 | 98 | 197 |
| R | official R manual package index (stat.ethz.ch R-devel) | 3536 | 34 | 0 |
| Python | runtime introspection | 2227 | 128 | 285 |

## Loss clusters

A cluster is one container's losses. Owning a gap per container is what
the existing cards already do, so the ledger folds into them rather than
opening a second owner for the same surface. `needs_card` means no card
owns that container today, and `closed` means the card that used to owns
it is done while losses remain.

| Container | Loss rows | Prior card | Card phase | Owner |
| --- | ---: | --- | --- | --- |
| String | 81 | #1409 | done | closed |
| core.math | 75 | none | n/a | needs_card |
| core.os | 56 | none | n/a | needs_card |
| List | 54 | #1410 | done | closed |
| core.time | 42 | none | n/a | needs_card |
| core.tasks | 33 | none | n/a | needs_card |
| Map | 30 | #1410 | done | closed |
| Set | 30 | #1404 | done | closed |
| core.io | 27 | #1402 | done | closed |
| core.files | 26 | #288 | building | live |
| core.net | 23 | none | n/a | needs_card |
| core.path | 22 | #288 | building | live |
| Iter | 21 | #1400 | done | closed |
| ByteBuffer | 20 | none | n/a | needs_card |
| core.archive | 15 | none | n/a | needs_card |
| Deque | 15 | none | n/a | needs_card |
| core.url | 14 | none | n/a | needs_card |
| core.log | 12 | none | n/a | needs_card |
| core.process | 12 | none | n/a | needs_card |
| core.crypto | 11 | none | n/a | needs_card |
| core.regex | 11 | none | n/a | needs_card |
| core.tls | 10 | none | n/a | needs_card |
| core.text | 8 | none | n/a | needs_card |
| core.testing | 7 | none | n/a | needs_card |
| core.db | 6 | none | n/a | needs_card |
| core.http | 6 | none | n/a | needs_card |
| SortedSet | 5 | #1404 | done | closed |
| core.binary | 3 | none | n/a | needs_card |
| core.encoding.csv | 3 | none | n/a | needs_card |
| core.uuid | 3 | none | n/a | needs_card |
| core.encoding.base64 | 2 | none | n/a | needs_card |
| core.random | 2 | none | n/a | needs_card |
| PriorityQueue | 2 | none | n/a | needs_card |

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
| R | core.random | 1768 |

## Core domains not yet compared

No competitor surface records a container for these Core modules, so no
row scores them. They are listed so the shortfall stays countable rather
than invisible.

`app`, `core.args`, `core.auth`, `core.browser`, `core.compiler`, `core.compute`, `core.email`, `core.encoding.cbor`, `core.encoding.jsonl`, `core.encoding.xml`, `core.encoding.yaml`, `core.event`, `core.game`, `core.lang`, `core.mem`, `core.mem.alloc`, `core.mime`, `core.perf`, `core.plugin`, `core.raylib`, `core.reactive`, `core.reactive.loadable`, `core.reflect`, `core.science.measurement`, `core.scope`, `core.services`, `core.sketch.cms`, `core.sketch.hll`, `core.sketch.reservoir`, `core.sketch.tdigest`, `core.solve`, `core.sync`, `core.term`, `core.ui`, `core.vault`, `core.vault.expert`, `core.watcher`, `core.web`, `core.web.devserver`, `core.web.storage`, `core.web.storage.local`, `core.web.storage.session`, `core.ws`

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
