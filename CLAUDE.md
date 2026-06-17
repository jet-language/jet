# CLAUDE.md — agent operating manual

You are building a beginner-first, memory-safe compiled language. The
front end (this repo) owns all semantics and every error message; rustc
is a hidden verifier/optimizer. A human **owner** has final say on all
user-facing syntax.

## Read order (before any work)

1. docs/spec/philosophy.md — ranked priorities; settles all arguments
2. docs/spec/syntax-decisions.md — what syntax you may use; never invent any
3. docs/spec/architecture.md — pipeline + rules R1–R7
4. docs/spec/diagnostics.md — error voice + format; snapshot-pinned
5. docs/spec/roadmap.md — current milestone and exit criteria

## Command environment

Run project commands through the Nix dev shell every time so all agents use
the same Rust, C toolchain, Node, Jet wrapper, and repo utilities:

```
nix develop -c cargo build
nix develop -c cargo test
nix develop -c jet run examples/features/01_hello.jet
nix develop -c rg "pattern" docs src tests
```

Do not rely on host-installed `cargo`, `rustc`, `jet`, `node`, or search
tools unless you are explicitly testing host-shell independence. Avoid
parallel `nix develop` invocations; Nix serializes eval/cache work and the
output becomes noisy. If several checks are needed, run them one at a time
or enter a single shell with `nix develop`.

## Task zero (do this first, before any feature work)

This scaffold was authored in a sandbox **without a Rust toolchain**.
1. `nix develop -c cargo build` — fix any compile errors (keep fixes minimal/mechanical).
2. `nix develop -c cargo test` — golden tests and `tests/decisions.rs` (ratification
   enforcement) must pass as-is. If a ui snapshot differs
   only because rendering drifted from the hand-computed fixtures, check
   the actual output against the format in docs/spec/diagnostics.md, then
   bless with `nix develop -c env UPDATE_EXPECT=1 cargo test` and re-run.
3. `nix develop -c jet run examples/features/01_hello.jet` prints `hello, world`.
Commit that as "M0 verified" before anything else.

## Invariants (violating one = stop and fix)

- **I1** Safe by default: ordinary Jet emits zero `unsafe`. Expert low-level code
  (`@unsafe { … }` / `@unsafe fn`, E2-M13/D-LL1) may generate `unsafe` only
  inside user-written, audited (`@audit("…")`) gate regions. No `unsafe`
  without an explicit `@unsafe` gate in the source.
- **I2** rustc never speaks to users. rustc rejecting generated code is an
  internal compiler error (exit 101, banner in src/main.rs) and a P0 bug.
- **I3** Codegen is dumb. All checking lives in sema. Never "try rustc and
  see" as a checking strategy.
- **I4** Every diagnostic has a code in docs/spec/diagnostics.md, what/why/fix, and a
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
example) → spec it in docs/spec/spec.md → implement parser → sema → codegen →
all tests green → update docs touched → done means: tests pass, docs
match behavior, no invariant bent.

## Syntax decision protocol

Need syntax that isn't Ratified or Provisional in docs/spec/syntax-decisions.md? Add a row to
its Open Decisions table — options, one-line tradeoffs, your
recommendation — and **stop work on that feature** until the owner
decides. Build something else meanwhile. When the owner ratifies: update
src/syntax.rs / parser, re-bless snapshots, log it in the decision table.

## Git workflow

Work directly on the current branch. Do not create new branches, worktrees,
or forks unless the owner explicitly asks for one.

## Style

- Write terse. Use natural, minimal, plain language; reach for a technical
  term only when it's the precise word. No filler, no hedging, no throat-
  clearing. This goes double for Markdown: don't pad docs with restated
  headings, bullet lists that repeat the prose, "comprehensive"/"robust"/
  "seamless" adjectives, or summary paragraphs that add nothing. Say the
  thing once, plainly, and stop. Bloated LLM prose is tiring to read —
  cut it.
- Plain std-only Rust; small modules; no cleverness codegen-side.
- Error message text is product copy: write it like docs/spec/diagnostics.md, get it
  snapshot-tested, never tweak casually.
- When in doubt, the ranked priorities in docs/spec/philosophy.md decide. Effort is the
  resource you spend; safety and beginner experience are the ones you
  don't.
