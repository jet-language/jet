# Jet Core surface ledger

Owner ruling 2026-08-03: the bar is not Python. It is every language Jet
competes with, and a missing feature is not acceptable.

This page is the durable review index. The JSON file beside it is the
machine-readable source that card #1398 reads. Do not keep a second
hand-written workflow inventory.

Generated on: 2026-08-07

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
  capability asked twice, so its witnesses pool across domains before the
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
| Shared containers | 54 |
| Core modules | 85 |
| Module members | 1139 |
| Collection method rows | 722 |
| Jet-side rows | 1862 |
| Total rows | 10434 |

## Verdicts

| Verdict | Rows |
| --- | ---: |
| Jet wins | 404 |
| Equal | 869 |
| Jet loses (two or more languages agree) | 192 |
| Single witness (recorded, not scored) | 8272 |
| Exported type, not an operation | 169 |
| Not compared | 420 |
| Deliberately declined | 108 |

## Competitors

| Language | Surface read from | Recorded operations | Jet rows matched | Loss rows |
| --- | --- | ---: | ---: | ---: |
| Rust | standard-library source (rust-src component) | 1032 | 360 | 12 |
| Go | official frozen API files (GOROOT/api/go1*.txt) | 1878 | 407 | 59 |
| Swift | official documentation JSON (developer.apple.com) | 505 | 185 | 4 |
| Kotlin | official API reference (kotlinlang.org) | 1141 | 245 | 18 |
| C# | official API documentation source (github.com/dotnet/dotnet-api-docs) | 1267 | 329 | 24 |
| TypeScript | runtime introspection | 724 | 222 | 53 |
| Ruby | runtime introspection | 1294 | 343 | 45 |
| Elixir | runtime introspection | 1270 | 372 | 60 |
| Julia | official documentation search index (docs.julialang.org) | 1132 | 283 | 63 |
| R | official R manual package index (stat.ethz.ch R-devel) | 1768 | 51 | 0 |
| Python | runtime introspection | 2232 | 420 | 116 |

## Loss clusters

A cluster is one container's losses. Owning a gap per container is what
the existing cards already do, so the ledger folds into them rather than
opening a second owner for the same surface. `needs_card` means no card
owns that container today, and `closed` means the owning card is done
while losses remain.

| Container | Loss rows | Owner card | Card phase | State |
| --- | ---: | --- | --- | --- |
| core.files | 66 | #288 | building | live |
| core.crypto | 17 | #1473 | ready | live |
| core.archive | 16 | #1470 | ready | live |
| core.time | 14 | #1466 | done | closed |
| core.tasks | 13 | #1468 | done | closed |
| core.math | 10 | #1464 | done | closed |
| core.os | 10 | #1465 | done | closed |
| core.log | 9 | #1474 | done | closed |
| core.net | 9 | #1469 | done | closed |
| core.path | 9 | #288 | building | live |
| ByteBuffer | 8 | #1467 | done | closed |
| core.regex | 4 | #1471 | done | closed |
| core.tls | 2 | #1593 | planning | live |
| BitSet | 1 | #1493 | ready | live |
| core.process | 1 | #1590 | verify | live |
| core.uuid | 1 | #1590 | verify | live |
| Deque | 1 | #1475 | done | closed |
| String | 1 | #1581 | verify | live |

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
