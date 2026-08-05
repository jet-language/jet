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
- --check rejects source drift, unmapped members, duplicate rows, hidden
  exclusions, stale loss owners, and unratified deliberate declines.

## Inventory

| Measure | Count |
| --- | ---: |
| Core modules | 85 |
| Module members | 1006 |
| Fixed-signature-only rows | 1 |
| Collection method-return functions | 42 |
| Collection method rows | 334 |
| Total rows | 1341 |
| Jet-loses rows | 37 |

Jet-loses rows are currently owned by: #288. A loss stays
visible until its owner closes; it is never converted into an omission.

## Python claim boundary

The claim covers every row mapped to the built-in types and standard-library
modules listed in the JSON pythonScope. Rows without a single Python
member still carry the Python workflow comparator and an explicit reason.
The ledger does not pretend that a Python package or a Jet-only domain is a
stdlib member.

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
workflow inventory. The ledger contains stable row IDs, Python member or
explicit no-single-member reason, Jet spelling, workflow, verdict, all
competitor operations, source provenance, and evidence links.

Run the focused guard from the repository root:

~~~sh
node scripts/agent/check-core-surface-ledger.mjs --check --tower plugins/tower/.tower/tower.json
~~~

Full rows are intentionally kept in the JSON artifact so the release
rubric can consume structured data without duplicating this inventory.
