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
| Total rows | 2321 |
| Jet-loses rows | 37 |

Jet-loses rows are currently owned by: #288. A loss stays
visible until its owner closes; it is never converted into an omission.

## Closure state

Walking only Jet's own tables cannot surface a feature Jet is missing, so
the ledger also walks the Python surface. Each unmatched comparison point
is a visible row, not an omission.

| Measure | Count |
| --- | ---: |
| Python comparison points | 1036 |
| Matched by a Jet row | 56 |
| Unadjudicated | 980 |

Unadjudicated points are owned by #1426. Per-container counts:

| Container | Unadjudicated |
| --- | ---: |
| os | 205 |
| asyncio | 86 |
| math | 57 |
| bytes | 42 |
| str | 41 |
| socket | 40 |
| logging | 34 |
| ssl | 34 |
| sqlite3 | 28 |
| time | 27 |
| random | 26 |
| statistics | 22 |
| unittest | 21 |
| urllib.parse | 21 |
| base64 | 20 |
| tarfile | 20 |
| io | 19 |
| itertools | 19 |
| re | 16 |
| csv | 14 |
| unicodedata | 14 |
| binascii | 12 |
| functools | 12 |
| uuid | 12 |
| bool | 11 |
| heapq | 11 |
| int | 11 |
| set | 10 |
| collections | 9 |
| subprocess | 9 |
| tempfile | 9 |
| float | 8 |
| struct | 8 |
| zipfile | 8 |
| pathlib | 7 |
| secrets | 7 |
| datetime | 6 |
| dict | 6 |
| json | 5 |
| range | 5 |
| http | 2 |
| list | 2 |
| tomllib | 2 |
| tuple | 2 |

Only the Python surface has been read. Operations for the other competitor
languages are recorded as unverified and are owned by #1426.

## Python claim boundary

The claim covers the built-in types and standard-library modules listed in
the JSON pythonScope, at Python 3.14.6. 1013 module-level constants are
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
