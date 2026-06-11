# CLAUDE.md — agent operating manual

You are building a beginner-first, memory-safe compiled language. The
front end (this repo) owns all semantics and every error message; rustc
is a hidden verifier/optimizer. A human **owner** has final say on all
user-facing syntax.

## Read order (before any work)

1. docs/00-philosophy.md — ranked priorities; settles all arguments
2. docs/02-syntax-decisions.md — what syntax you may use; never invent any
3. docs/03-architecture.md — pipeline + rules R1–R7
4. docs/04-diagnostics.md — error voice + format; snapshot-pinned
5. docs/05-roadmap.md — current milestone and exit criteria

## Task zero (do this first, before any feature work)

This scaffold was authored in a sandbox **without a Rust toolchain**.
1. `cargo build` — fix any compile errors (keep fixes minimal/mechanical).
2. `cargo test` — golden tests and `tests/decisions.rs` (ratification
   enforcement) must pass as-is. If a ui snapshot differs
   only because rendering drifted from the hand-computed fixtures, check
   the actual output against the format in docs/04-diagnostics.md, then
   bless with `UPDATE_EXPECT=1 cargo test` and re-run.
3. `./target/debug/jet run examples/01_hello.jet` prints `hello, world`.
Commit that as "M0 verified" before anything else.

## Invariants (violating one = stop and fix)

- **I1** No `unsafe` in the language or in generated code. Ever (v1).
- **I2** rustc never speaks to users. rustc rejecting generated code is an
  internal compiler error (exit 101, banner in src/main.rs) and a P0 bug.
- **I3** Codegen is dumb. All checking lives in sema. Never "try rustc and
  see" as a checking strategy.
- **I4** Every diagnostic has a code in docs/04, what/why/fix, and a
  tests/ui snapshot. No snapshot → the diagnostic doesn't exist.
- **I5** Examples are the executable spec. Every feature ships with an
  example + expected output that golden tests enforce.
- **I6** Zero external crates in the compiler without owner approval.
- **I7** Every user-typeable keyword/sigil lives in src/syntax.rs with a
  decision ID.
- **I8** Simplicity ratchet: prefer rejecting a program with a great
  error + workaround over adding a feature. New features need a roadmap
  slot or owner sign-off.

## Workflow loop

Pick the next roadmap item → write the failing test first (ui fixture or
example) → spec it in docs/01 → implement parser → sema → codegen →
all tests green → update docs touched → done means: tests pass, docs
match behavior, no invariant bent.

## Syntax decision protocol

Need syntax that isn't Ratified or Provisional in docs/02? Add a row to
its Open Decisions table — options, one-line tradeoffs, your
recommendation — and **stop work on that feature** until the owner
decides. Build something else meanwhile. When the owner ratifies: update
src/syntax.rs / parser, re-bless snapshots, log it in the decision table.

## Style

- Plain std-only Rust; small modules; no cleverness codegen-side.
- Error message text is product copy: write it like docs/04, get it
  snapshot-tested, never tweak casually.
- When in doubt, the ranked priorities in docs/00 decide. Effort is the
  resource you spend; safety and beginner experience are the ones you
  don't.
