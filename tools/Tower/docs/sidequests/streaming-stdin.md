# Plan: Streaming / line-by-line stdin (D-STDIN1)

**Status: plan — awaiting owner decision D-STDIN1.**

Unblocks: **Priya** (CLI text filtering — grep-like tools), **Elena** (large-file
ETL — stream rows without loading the whole file).

---

## Goal

`core.io.read_all_input()` slurps all of stdin at once, so any line-filter tool
reads the whole input into memory then splits by hand. Files already have
streaming (`FileReader.lines()` / `.read_line()` exist in
`CheckerStdlib.rs:1003-1029`), but **stdin has no streaming path**. Give stdin the
same line iterator so a `cat | jet run filter.jet` pipeline processes lines one at
a time, constant-memory.

Verified: `core.io` exposes `args`, `input`, `read_all_input`, `eprint`
(`CheckerStdlib.rs:1507`); no `stdin().lines()` form. The file-side `FileLines`
streaming type already exists and is the model to mirror.

## Pipeline touch points

- **stdlib + sema** (`CheckerStdlib.rs`): add a stdin handle whose `.lines()`
  returns the existing `FileLines`-style streaming source usable in `loop … in`,
  and/or `.read_line()` returning `String? ? io`. Reuse the file machinery.
- **codegen** (`Source/Prelude/Std.rs`): a stdin-lines helper backed by a buffered
  reader over `std::io::stdin().lock()`.
- **diagnostics**: reading stdin twice (after it's drained) — optional teaching
  diagnostic; v1 may leave it as EOF.

## Invariants in play

- **I1/purity**: stdin reads are non-deterministic/impure (already true of
  `input`/`read_all_input`, see `Interpreter.rs:113`) — the streaming form must
  carry the same impurity tag so `pure fn` can't read it.
- **I5** ships an example (a line filter) with golden output.
- **One-path (philosophy):** prefer the *same* `.lines()` spelling files use, so
  there is one streaming idiom, not a file one and a stdin one.

## Open questions (need owner decision — D-STDIN1)

1. **Surface** — `io.stdin().lines()` (a stdin handle mirroring `files.open`),
   a bare `io.lines()` convenience, or `io.read_lines()` returning the iterator.
   Should it reuse the exact `FileReader`/`FileLines` type so files and stdin are
   interchangeable in a `loop … in`?
2. **`read_all_input` future** — keep it (convenience for small inputs), deprecate
   it toward the streaming form, or leave both as peers?
3. **Element type** — lines as `String` (newline-stripped) only, or also a bytes
   form for binary stdin?
4. **EOF / re-read semantics** — what happens on a second `.lines()` after stdin
   is drained? (proposed: empty iterator, no error.)

## Test plan

1. `examples/features/stdin_filter.jet` — read stdin line-by-line, print lines
   matching a substring; golden test pipes fixture input → expected output (I5).
2. Streaming proof: a large synthetic input processed without buffering all of it
   (integration test asserts constant-ish memory or just correctness on N lines).
3. Purity: a `pure fn` calling the stdin iterator → impurity diagnostic snapshot.
4. EOF: empty stdin → empty loop, exit 0.
