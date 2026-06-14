# jsonfmt — M14 showcase notes

## What it does

Reads JSON from a file or stdin, pretty-prints on success, prints parse error
on stderr and exits 1 on invalid JSON.

## Line count vs Rust reference

| Implementation | Lines |
|----------------|-------|
| Jet (`showcase/jsonfmt.jet`) | ~60 |
| Rust (`showcase/ref/jsonfmt.rs`) | ~45 |

## Where Jet fought the author

1. **`e.line` / `e.message` on `JsonError`** — codegen currently prefixes struct
   fields incorrectly; use `"{e}"` (JetShow) until fixed.
2. **No `if` expressions** — `val raw = if ...` is invalid; use `var` + branches.
3. **`process.exit` in helpers** — sema doesn't treat it as diverging; keep IO in
   `main` or use `switch` on fallible results.
4. **`switch` arms use `->` not `=>`** (Rust habit).

## Benchmark

Dominated by JSON parse/render; Jet within ~1.2× reference (see BENCHMARKS.md).
