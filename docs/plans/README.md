# Implementation plans

How work is organized, and the protocol an implementing agent follows. The plans
are the *how*; `docs/spec/` remains the *what* and *why* and always wins on conflict.

## Where work lives

- **Active epochs** — [`epoch-2/`](epoch-2/README.md) (status of record:
  [`epoch-2/EPOCH2-STATUS.md`](epoch-2/EPOCH2-STATUS.md)) and
  [`epoch-3/`](epoch-3/README.md) for future pillars.
- **Sidequests** — [`sidequests/`](sidequests/): one agent-reviewed plan per
  in-flight task. A sidequest is deleted the moment its feature ships (behavior
  then lives in `docs/spec/spec.md` + golden examples). Plans are scaffolding,
  not trophies, and cite code by **symbol**, never by `file.rs:NNNN`.
- **Product tracks** — [`jetpack-jetos/`](jetpack-jetos/README.md): the package
  manager + OS. Design-of-record is
  [`unified-ecosystem.md`](jetpack-jetos/unified-ecosystem.md); live status is
  [`IMPLEMENTATION-STATUS.md`](jetpack-jetos/IMPLEMENTATION-STATUS.md).

Tasks, their live pipeline stage, every open decision, and bugs are managed in the
dashboard (`node tools/pipeline/pipeline.mjs serve`), not in a checked-in to-do file.

## Protocol for the implementing agent (read this first, every time)

1. Read, in order: docs/spec/philosophy.md, docs/spec/syntax-decisions.md,
   docs/spec/architecture.md, docs/spec/diagnostics.md, then your plan file.
2. **Syntax gate.** Your plan lists the decision IDs it depends on. Check
   docs/spec/syntax-decisions.md: every listed ID must be **Ratified** (or
   Provisional and explicitly allowed by the plan). If one is still open, STOP and
   report to the owner — do not invent syntax, do not pick an option yourself
   (invariant I7, CLAUDE.md protocol). Plans show example code using the
   *recommended* option from docs/spec/decision-ballots.md; if the owner ratified a
   different option, substitute it everywhere mechanically.
3. Work test-first: for each feature, write the failing ui fixture or example
   before the code. Snapshot text must follow docs/spec/diagnostics.md voice rules
   exactly.
4. Build in pipeline order: syntax.rs → lexer → parser → sema → codegen, never
   skipping sema into codegen (rules R1/R2).
5. Error codes: claim them in docs/spec/diagnostics.md's registry as you go; lints
   take L-prefixed codes.
6. Definition of done: all exit criteria pass as tests; `cargo test` fully green;
   every new diagnostic has a snapshot; every new feature has an example with
   expected output; docs/spec/spec.md + diagnostics registry + roadmap updated; no
   invariant bent; zero new external crates in the compiler (I6 — tooling-binary
   exceptions must be pre-approved by the owner).
7. Commit at the end; do not start the next milestone in the same run.

## Example numbering (reserved)

Sequential `examples/NN_*.jet` slots — do not reuse or skip when adding an
example. Multi-file demos use a directory (`examples/features/21_imports/`).

| # | Milestone | File(s) |
|---|-----------|---------|
| 01–09 | M0–M2 | hello, functions, values, branches, fizzbuzz, compound, switch, ownership, ref_field |
| 10–13 | M3–M4 | structs, enums, option, errors |
| 14 | M4 | panic |
| 15–18 | M5 | lists, wordcount, strings, list_bounds |
| 19 | M5 | map_key (error demo) |
| 20 | M6 | tests |
| 21 | M6 | imports/ |
| 22 | M7 | ffi |
| 23–24 | M8 | closures, callbacks |
| 25–26 | M9 | traits, generic_types |
| 27–28 | M9.5 | comptime_table, embed |
| 29–31 | M10 | files, json, cli |
| 32 | M12 | packages/ |
| 33–34 | M11 | tasks, pipeline |
