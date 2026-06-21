# CLAUDE.md — agent operating manual

You are building a dual-facet, memory-safe compiled language: magic
out-of-the-box for beginners, full expert control behind explicit opt-in.
The long-run goal is jack-of-all-trades, master-of-ALL — no reason to
reach for another language. The front end owns all semantics and every
error message; rustc is a hidden verifier/optimizer. A human **owner**
has final say on all user-facing syntax.

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
nix develop -c rg "pattern" docs Source tests
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

- **I1** Safe by default, expert tier first-class: all Jet code is memory-safe
  and type-safe unless the user explicitly opts in. `@unsafe { … }` / `@unsafe fn`
  (E2-M13/D-LL1) is a real, supported expert tier — not a loophole — gated by
  user-written, audited (`@audit("…")`) regions. Generated code may contain Rust
  `unsafe` only inside those gate regions. No `unsafe` in generated code without
  a corresponding `@unsafe` gate in the source.
- **I2** rustc never speaks to users. rustc rejecting generated code is an
  internal compiler error (exit 101, banner in Source/main.rs) and a P0 bug.
- **I3** Codegen is dumb. All checking lives in sema. Never "try rustc and
  see" as a checking strategy.
- **I4** Every diagnostic has a code in docs/spec/diagnostics.md, what/why/fix, and a
  tests/ui snapshot. No snapshot → the diagnostic doesn't exist.
- **I5** Examples are the executable spec. Every feature ships with an
  example + expected output that golden tests enforce.
- **I6** Zero external crates in the compiler (`Source/`), ever. Stdlib sub-libraries
  and modules may use external crates to bootstrap until end of Epoch 3; after
  that, all external deps must be replaced with native Jet/Rust implementations.
  Any new stdlib external dep requires owner approval.
- **I7** Every user-typeable keyword/sigil lives in Source/Syntax.rs with a
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

Need syntax that isn't Ratified or Provisional in docs/spec/syntax-decisions.md? Develop it
into a decision card — options, a worked per-option example, your recommendation — get it
reviewed by another agent, and queue it in Tower's ballot
(tools/Tower/docs/ballots/decision-ballots.md); **stop work on that feature** until the
owner decides. Build something else meanwhile. When the owner ratifies: update
Source/Syntax.rs / parser, re-bless snapshots, log it in syntax-decisions.md.

See the **tower-sweep** skill for the full project-management loop. The owner is
CEO/CTO; his decisions are the only allowed bottleneck — he never waits on you for a
plan or a decision, and nothing reaches him that an agent hasn't already reviewed.

## Sub-agent delegation

Spawn sub-agents for parallelisable or context-heavy work rather than doing
everything in one context window. Match the model to the task:

| Model | When to use |
|-------|-------------|
| `haiku` | Mechanical, read-only tasks: grep, file lookup, snapshot diffing, doc summarisation |
| `sonnet` | Default for most implementation sub-tasks: writing Rust, sema passes, codegen, tests |
| `opus` | Hard reasoning: type-system design, architecture decisions, tricky sema edge cases, design reviews |

Rules:
- Prefer `pipeline()` over `parallel()` in workflows unless you genuinely need all results before proceeding.
- Never spawn a sub-agent just to run a single shell command — use Bash directly.
- Sub-agents must still follow all invariants (I1–I8) and the Nix command environment.
- Pass the sub-agent enough context (relevant file paths, invariants, goal) so it can act without re-reading this whole file.

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
- **Difficulty is never a deterrent, and never an argument** (philosophy.md →
  "Effort is never a deterrent"). Never let "this is hard / a lot of work / would
  take a long time" influence a recommendation, an option ranking, or how much you
  scope. Hard work up front is the chosen currency; a hard path is often the right
  one. **Do it right the first time** — full, end-to-end, the first time. Never
  ship a stub or "milestone-pending" placeholder meaning to revisit, unless
  genuinely blocked on an unratified upstream decision (name the gate). Don't even
  *mention* implementation difficulty as a factor; weigh only the ranked priorities.
