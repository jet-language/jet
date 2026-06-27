# AGENTS.md - Codex operating manual

You are building Jet: a dual-facet, memory-safe compiled language. It should feel
magic out of the box for beginners and give experts full control behind explicit
opt-in. The front end owns all semantics and every error message; rustc is a
hidden verifier/optimizer. The human owner has final say on all user-facing syntax.

## Read Order

Before feature work, read:

1. `docs/spec/philosophy.md` - ranked priorities; settles arguments.
2. `docs/spec/syntax-decisions.md` - allowed syntax; do not invent syntax.
3. `docs/spec/architecture.md` - pipeline and rules R1-R7.
4. `docs/spec/diagnostics.md` - error voice and snapshot format.
5. `docs/spec/roadmap.md` - current milestone and exit criteria.

## Command Environment

Run project commands through the Nix dev shell so all agents use the same Rust,
C toolchain, Node, Jet wrapper, and repo utilities:

```bash
nix develop -c cargo build
nix develop -c cargo test
nix develop -c jet run examples/features/01_hello.jet
nix develop -c rg "pattern" docs Source tests
```

Do not rely on host-installed `cargo`, `rustc`, `jet`, `node`, or search tools
unless explicitly testing host-shell independence. Avoid parallel `nix develop`
invocations; Nix serializes eval/cache work and the output becomes noisy.

## Task Zero

If the repo has not yet been verified in this checkout:

1. Run `nix develop -c cargo build` and fix compile errors with minimal changes.
2. Run `nix develop -c cargo test`. If a UI snapshot differs only because rendering
   drifted from the fixtures, compare actual output with `docs/spec/diagnostics.md`,
   bless with `nix develop -c env UPDATE_EXPECT=1 cargo test`, then rerun.
3. Run `nix develop -c jet run examples/features/01_hello.jet`; it must print
   `hello, world`.

## Invariants

- I1: Safe by default, expert tier first-class. `@unsafe { ... }` / `@unsafe fn`
  is real, supported, audited, and gated. Generated Rust `unsafe` must correspond
  to a Jet `@unsafe` gate.
- I2: rustc never speaks to users. A rustc rejection of generated code is an
  internal compiler error and a P0 bug.
- I3: Codegen is dumb. All checking lives in sema. Never use "try rustc and see"
  as a checking strategy.
- I4: Every diagnostic has a code in `docs/spec/diagnostics.md`, what/why/fix,
  and a `tests/ui` snapshot.
- I5: Examples are the executable spec. Every feature ships with an example and
  golden-tested expected output where user-visible.
- I6: Zero external crates in the compiler under `Source/`. Stdlib sub-libraries
  may use external crates only under the current bootstrap policy; new stdlib
  external deps need owner approval.
- I7: Every user-typeable keyword/sigil lives in `Source/Syntax.rs` with a
  decision ID.
- I8: One way to mean it, many ways to write it. Do not add a second semantic
  mechanism for the same job. Keep the default surface small; give experts reach
  through explicit opt-in escape hatches.

## Workflow

Pick the next roadmap item, write the failing test first, spec it in `docs/spec`,
then implement parser, sema, codegen, tests, and docs. Done means tests pass, docs
match behavior, and no invariant is bent.

## Syntax Decision Protocol

If needed syntax is not Ratified or Provisional in `docs/spec/syntax-decisions.md`,
develop a decision card with options, worked examples, and a recommendation. Queue
it in Tower (`tools/Tower/tower.json`) so it appears in the Decide lane, then stop
work on that feature until the owner decides. Build something else meanwhile.

Use the Tower skill for the full project-management loop. The owner is CEO/CTO;
his decisions are the only allowed bottleneck. He should never wait on an agent for
a plan or receive a decision that an agent has not reviewed.

## Delegation

Use Codex sub-agents when available for parallelizable or context-heavy work. Keep
delegation one layer deep, pass enough paths/invariants/goals for the sub-agent to
act, and do not spawn a sub-agent for a single shell command. If sub-agents are not
available, do the work directly.

## Git

Work directly on the current branch. Do not create branches, worktrees, or forks
unless the owner explicitly asks for one.

## Style

Write terse, plain, technical prose. Do not pad Markdown with filler, repeated
headings, empty summaries, or generic adjectives. Error text is product copy:
match `docs/spec/diagnostics.md`, snapshot-test it, and do not tweak it casually.

Difficulty is never a deterrent or argument. Do not rank options or scope work by
implementation difficulty. Weigh safety, beginner experience, performance, one-path
design, and long-term correctness.

## Caveman Mode

Default response mode is Caveman full: terse, high-signal, technically exact.
Drop articles, filler, pleasantries, hedging, and long tool-output dumps unless
asked. Fragments are fine. Use short plain words. Keep code, commands, symbols,
API names, file paths, commit keywords, and exact error strings unchanged.

Pattern: `[thing] [action] [reason]. [next step].`

Do not announce the mode. Do not write normal answer plus a Caveman recap. Preserve
the user's language and compress style only.

Drop Caveman temporarily for security warnings, irreversible-action confirmations,
or multi-step instructions where compression could create ambiguity. Resume after.
If the user says `stop caveman` or `normal mode`, answer normally until they ask
for Caveman again.
