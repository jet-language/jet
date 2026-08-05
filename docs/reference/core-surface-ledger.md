# Jet Core surface ledger

Generated from the compiler's Core source tables. The JSON file is the
machine-readable source consumed by #1398; this page is the durable
review index. Do not maintain a second hand-written workflow inventory.

Generated on: 2026-08-05

## Source contract

- Module members come from module_items.rs, including its dynamic
  core.lang policy registry and resolved Syntax constants.
- Fixed call signatures come from fixed_sigs.rs.
- Built-in type method returns come from Collections.rs.
- The Python side comes from docs/reference/python-surface.json, read from
  a real interpreter. A constructed member name is never evidence.
- --check rejects source drift, an unverified Python member, an equal
  verdict without a member, duplicate rows, hidden exclusions, stale gap
  owners, and unratified deliberate declines.

## Inventory

| Measure | Count |
| --- | ---: |
| Core modules | 85 |
| Module members | 1006 |
| Fixed-signature-only rows | 1 |
| Collection method-return functions | 42 |
| Collection method rows | 334 |
| Jet-side rows | 1341 |
| Total rows | 2492 |
| Jet-loses rows | 37 |

Jet-loses rows are currently owned by: #288. A loss stays
visible until its owner closes; it is never converted into an omission.

## Closure state

Walking only Jet's own tables cannot surface a feature Jet is missing, so
the ledger also walks the Python surface. Each unmatched comparison point
is a visible row, not an omission.

| Measure | Count |
| --- | ---: |
| Python comparison points | 1254 |
| Matched by a Jet row | 131 |
| Unadjudicated | 2327 |

Unadjudicated points are owned by #1426. Per-container counts:

| Container | Unadjudicated |
| --- | ---: |
| os | 203 |
| builtins | 141 |
| asyncio | 86 |
| core.net | 75 |
| core.compute | 72 |
| core.services | 53 |
| List | 52 |
| bytes | 42 |
| Iter | 41 |
| str | 41 |
| core.data | 40 |
| socket | 40 |
| sys | 39 |
| core.crypto | 37 |
| math | 34 |
| ssl | 33 |
| types | 33 |
| core.vault | 32 |
| core.text | 31 |
| logging | 30 |
| sqlite3 | 28 |
| core.math | 27 |
| time | 27 |
| core.encoding.xml | 26 |
| core.web | 26 |
| core.sync | 25 |
| core.time | 23 |
| random | 23 |
| core.browser | 21 |
| core.email | 21 |
| core.ui | 21 |
| unittest | 21 |
| urllib.parse | 21 |
| tarfile | 20 |
| core.log | 19 |
| io | 19 |
| itertools | 19 |
| statistics | 19 |
| base64 | 18 |
| String | 18 |
| app | 17 |
| core.auth | 17 |
| core.http.server | 16 |
| core.lang | 16 |
| core.crypto.expert | 15 |
| core.io | 14 |
| core.raylib | 14 |
| unicodedata | 14 |
| csv | 13 |
| re | 13 |
| SortedSet | 13 |
| binascii | 12 |
| ByteBuffer | 12 |
| functools | 12 |
| Rng | 12 |
| uuid | 12 |
| bool | 11 |
| core.db | 11 |
| core.encoding.cbor | 11 |
| core.mem | 11 |
| core.random | 11 |
| heapq | 11 |
| int | 11 |
| Cache | 10 |
| set | 10 |
| AsyncEvent | 9 |
| collections | 9 |
| core.fmt | 9 |
| core.regex | 9 |
| core.tls | 9 |
| subprocess | 9 |
| tempfile | 9 |
| View | 9 |
| BitSet | 8 |
| Cell | 8 |
| core.encoding.json | 8 |
| float | 8 |
| struct | 8 |
| zipfile | 8 |
| Bag | 7 |
| core.encoding | 7 |
| core.event | 7 |
| core.url | 7 |
| pathlib | 7 |
| secrets | 7 |
| WatchHandle | 7 |
| CellGuard | 6 |
| core.encoding.jsonl | 6 |
| core.http.client | 6 |
| core.tasks | 6 |
| core.testing | 6 |
| core.text.unicode | 6 |
| datetime | 6 |
| dict | 6 |
| Event | 6 |
| Hook | 6 |
| Map | 6 |
| Shared | 6 |
| core.encoding.csv | 5 |
| core.os | 5 |
| core.perf | 5 |
| DecisionHook | 5 |
| DispatchReport | 5 |
| json | 5 |
| PriorityQueue | 5 |
| range | 5 |
| Clock | 4 |
| core.compiler | 4 |
| core.encoding.yaml | 4 |
| core.game | 4 |
| core.mem.alloc | 4 |
| core.reactive | 4 |
| core.reactive.loadable | 4 |
| core.vault.expert | 4 |
| core.watcher | 4 |
| core.web.storage.local | 4 |
| core.web.storage.session | 4 |
| Deque | 4 |
| EventTrace | 4 |
| WatchSet | 4 |
| core.encoding.toml | 3 |
| core.http | 3 |
| core.mime | 3 |
| core.time.date | 3 |
| Pool | 3 |
| Set | 3 |
| SharedGuard | 3 |
| Solver | 3 |
| Condition | 2 |
| core.archive | 2 |
| core.compress.gzip | 2 |
| core.compress.zstd | 2 |
| core.encoding.base64 | 2 |
| core.time.datetime | 2 |
| core.web.devserver | 2 |
| core.web.storage | 2 |
| core.ws | 2 |
| EventScope | 2 |
| http | 2 |
| list | 2 |
| Signal | 2 |
| Subscription | 2 |
| tomllib | 2 |
| tuple | 2 |
| core.args | 1 |
| core.plugin | 1 |
| core.reflect | 1 |
| core.science.measurement | 1 |
| core.scope | 1 |
| core.sketch.cms | 1 |
| core.sketch.hll | 1 |
| core.sketch.reservoir | 1 |
| core.sketch.tdigest | 1 |
| core.solve | 1 |
| core.term | 1 |
| Derived | 1 |
| Option | 1 |
| Receiver | 1 |
| Sender | 1 |
| SharedWeak | 1 |
| Stopwatch | 1 |
| Task | 1 |

Only the Python surface has been read. Operations for the other competitor
languages are recorded as unverified and are owned by #1426.

## Python claim boundary

The claim covers the built-in types and standard-library modules listed in
the JSON pythonScope, at Python 3.14.6. 1057 module-level constants are
excluded by the recorded scope rule and stay counted so the exclusion
cannot hide a gap.

Primary Python references:

- Python library index: https://docs.python.org/3/library/index.html
- Python built-in functions and types: https://docs.python.org/3/library/functions.html

## Competitor references

- Python: https://docs.python.org/3/library/functions.html, https://docs.python.org/3/library/stdtypes.html, https://docs.python.org/3/library/index.html
- Rust: https://doc.rust-lang.org/std/collections/, https://doc.rust-lang.org/std/iter/
- Go: https://pkg.go.dev/std
- Swift: https://developer.apple.com/documentation/swift/sequence-and-collection-protocols
- Kotlin: https://kotlinlang.org/api/core/kotlin-stdlib/
- C#: https://learn.microsoft.com/en-us/dotnet/standard/linq/
- TypeScript: https://www.typescriptlang.org/tsconfig/lib.html
- Ruby: https://ruby-doc.org/3.4.1/
- Elixir: https://hexdocs.pm/elixir/Enum.html
- Julia: https://docs.julialang.org/en/v1/base/collections/
- R: https://stat.ethz.ch/R-manual/R-devel/library/base/html/00Index.html

## Consumer

Card #1398 reads docs/reference/core-surface-ledger.json as its only
workflow inventory. The ledger contains stable row IDs, a verified Python
member or an explicit reason, Jet spelling, workflow, verdict, gap owner,
source provenance, and evidence links.

Run the focused guard from the repository root:

~~~sh
node scripts/agent/check-core-surface-ledger.mjs --check --tower plugins/tower/.tower/tower.json
~~~

Full rows are intentionally kept in the JSON artifact so the release
rubric can consume structured data without duplicating this inventory.
