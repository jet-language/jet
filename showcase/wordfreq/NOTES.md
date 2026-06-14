# wordfreq — M14 showcase notes

## What it does

Walks directories recursively, counts words in `.txt` files, prints
`word: count` sorted by frequency (descending) then alphabetically for ties.

## Line count vs Rust reference

| Implementation | Lines |
|----------------|-------|
| Jet (`showcase/wordfreq.jet`) | ~95 |
| Rust (`showcase/ref/wordfreq.rs`) | ~70 |

## Where Jet fought the author

1. **Mutating `Map` through function parameters** — use `var counts` in `main` and
   inline counting, or pass by mut ref (not available in v1).
2. **`sort_by` closure capturing `counts`** — rustc codegen moves the map into the
   closure; sort via prefixed rank strings (`"{inv}\t{word}"`) instead.
3. **`sort_by` key must be `Int`** — not `String`; use rank+sort or double sort.
4. **Filter extensions manually** — no glob API; `path.ends_with(".txt")`.

## Benchmark

Sequential file I/O; see BENCHMARKS.md.
