# jetgrep — M14 showcase notes

## What it does

Grep-lite: recursive file walk (`-r`), case-insensitive search (`-i`),
line numbers (`-n`), match counts (`-c`), exit 0 on match / 1 on none / 2 on usage error.

## Line count vs Rust reference

| Implementation | Lines (.jet / .rs) |
|----------------|-------------------|
| Jet (`examples/showcase/jetgrep.jet`) | ~250 |
| Rust (`examples/showcase/ref/jetgrep.rs`) | ~120 |

Jet is longer mainly because of explicit ownership workarounds (clone path before
reuse, `if (field == true)` instead of `if field`, no `or return` with struct literals).

## Where Jet fought the author

1. **`if cfg.field {`** — parser treats `{` after field access as a struct literal;
   use `if (cfg.field == true)` or bind to a local `Bool` first.
2. **`for x in result.lines`** — same brace ambiguity; bind `result.lines` to a local.
3. **`or return out`** inside `collect_files` — early `return out` in one branch
   marks `out` moved; use `if (recursive == true) { ... }` without early return of `out`.
4. **`join_path(path, name)` in a loop** — `path` moves on first call; `path.clone()`
   at function entry or clone inside `join_path`.
5. **`switch` inside functions that mutate outer `var`** — sema thinks outer `var`
   may be moved; prefer `if x == ok(v)` pattern tests for fallible std calls.
6. **All showcase `main.jet` share stem `main`** — binaries collide in `build/`; use
   unique filenames (`jetgrep.jet`, etc.).

## Benchmark (see BENCHMARKS.md)

Target ≤1.5× Rust reference on `examples/showcase/fixtures/`.
