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
| Total rows | 16744 |

## Verdicts

| Verdict | Rows |
| --- | ---: |
| Jet wins | 395 |
| Equal | 366 |
| Jet loses | 15398 |
| Not compared | 585 |
| Deliberately declined | 0 |

## Competitors

| Language | Surface read from | Recorded operations | Jet rows matched | Loss rows |
| --- | --- | ---: | ---: | ---: |
| Rust | standard-library source (rust-src component) | 966 | 144 | 828 |
| Go | official frozen API files (GOROOT/api/go1*.txt) | 2000 | 147 | 1865 |
| Swift | official documentation JSON (developer.apple.com) | 588 | 67 | 527 |
| Kotlin | official API reference (kotlinlang.org) | 1729 | 125 | 1608 |
| C# | official API documentation source (github.com/dotnet/dotnet-api-docs) | 1241 | 127 | 1122 |
| TypeScript | runtime introspection | 347 | 96 | 257 |
| Ruby | runtime introspection | 1641 | 140 | 1508 |
| Elixir | runtime introspection | 1450 | 147 | 1312 |
| Julia | official documentation search index (docs.julialang.org) | 2182 | 136 | 2056 |
| R | official R manual package index (stat.ethz.ch R-devel) | 3536 | 34 | 0 |
| Python | runtime introspection | 4482 | 176 | 4315 |

## Loss clusters

A cluster is one container's losses. Owning a gap per container is what
the existing cards already do, so the ledger folds into them rather than
opening a second owner for the same surface. `needs_card` means no card
owns that container today, and `closed` means the card that used to owns
it is done while losses remain.

| Container | Loss rows | Prior card | Card phase | Owner |
| --- | ---: | --- | --- | --- |
| List | 1120 | #1410 | done | closed |
| String | 928 | #1409 | done | closed |
| core.text | 814 | none | n/a | needs_card |
| core.files | 790 | #288 | building | live |
| core.os | 785 | none | n/a | needs_card |
| core.io | 711 | #1402 | done | closed |
| core.env | 697 | none | n/a | needs_card |
| Iter | 651 | #1400 | done | closed |
| core.fmt | 617 | none | n/a | needs_card |
| Map | 577 | #1410 | done | closed |
| core.time | 572 | none | n/a | needs_card |
| Set | 539 | #1404 | done | closed |
| core.net | 536 | none | n/a | needs_card |
| core.math | 497 | none | n/a | needs_card |
| core.tasks | 477 | none | n/a | needs_card |
| core.process | 452 | none | n/a | needs_card |
| core.tls | 414 | none | n/a | needs_card |
| core.http | 395 | none | n/a | needs_card |
| ByteBuffer | 324 | none | n/a | needs_card |
| core.path | 315 | #288 | building | live |
| core.db | 310 | none | n/a | needs_card |
| core.binary | 301 | none | n/a | needs_card |
| core.regex | 267 | none | n/a | needs_card |
| core.archive | 249 | none | n/a | needs_card |
| core.url | 236 | none | n/a | needs_card |
| core.testing | 231 | none | n/a | needs_card |
| core.log | 212 | none | n/a | needs_card |
| Deque | 206 | none | n/a | needs_card |
| core.crypto | 157 | none | n/a | needs_card |
| core.crypto.random | 123 | none | n/a | needs_card |
| core.random | 114 | none | n/a | needs_card |
| core.encoding.csv | 112 | none | n/a | needs_card |
| core.uuid | 111 | none | n/a | needs_card |
| core.encoding.base64 | 87 | none | n/a | needs_card |
| core.encoding.json | 84 | none | n/a | needs_card |
| SortedSet | 70 | #1404 | done | closed |
| BitSet | 67 | none | n/a | needs_card |
| core.text.unicode | 55 | none | n/a | needs_card |
| core.encoding.base32 | 46 | none | n/a | needs_card |
| PriorityQueue | 46 | none | n/a | needs_card |
| core.encoding.hex | 43 | none | n/a | needs_card |
| core.data | 39 | none | n/a | needs_card |
| Cache | 16 | none | n/a | needs_card |
| core.encoding.toml | 5 | none | n/a | needs_card |

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
