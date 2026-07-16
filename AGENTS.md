# AGENTS.md — Jet agent operating manual

Canonical policy for every coding agent. `CLAUDE.md` is a symlink here; do not
fork per-tool copies. Keep this file short, current, and limited to rules that
apply across tasks. Put procedures in skills, design in specs, work state in
Tower, and deterministic enforcement in tests or hooks.

## Mission and authority

Jet is a dual-facet, memory-safe compiled language: magic out of the box for
beginners, full expert control behind explicit opt-in. The front end owns all
semantics and user-facing errors; rustc is a hidden verifier and optimizer. The
human owner has final say on user-facing syntax.

Resolve guidance in this order:

1. the owner's current explicit instruction;
2. ratified Tower verdicts and their acceptance terms;
3. the relevant domain spec;
4. this file and the nearest nested `AGENTS.md`;
5. task-specific skills and prompts.

Code shows implementation state, not design authority. A newer ratified ruling
beats stale code or prose. If sources conflict, follow the higher authority,
record the conflict, and stop only the affected slice. Never silently average
contradictory rules.

## Load context by trigger

Read this file, then inspect the relevant code, tests, and current diff. Load
only the references the task requires:

- language semantics or syntax: relevant sections of
  `docs/spec/philosophy.md`, `docs/spec/syntax-decisions.md`, and
  `docs/spec/architecture.md`;
- diagnostics: `docs/spec/diagnostics.md` and the matching UI snapshots;
- Tower work or owner decisions: `.agents/skills/tower/SKILL.md`, plus
  `.agents/skills/tower-ballot/SKILL.md` when a choice is owner-gated;
- completion claims: `.claude/skills/verify/SKILL.md`;
- a specialized task: the matching skill named in the request or skill catalog.

Do not front-load every spec, plan, prompt, or board record. Search first and
read the smallest authoritative slice that can settle the question.

## Command environment

Run project commands through `scripts/agent/jet-env`:

```sh
scripts/agent/jet-env cargo build
scripts/agent/jet-env cargo test --test NAME
scripts/agent/jet-env jet run examples/features/basics/hello.jet
scripts/agent/jet-env rg "pattern" docs Source crates tests
```

Use `scripts/agent/jet-env full <command>` only for FFI, Canvas/browser,
graphics, VM/image, or full verification. Do not rely on host tools unless
testing host-shell independence. Group dependent checks in one shell launch
when practical.

Rebuild before smoke-testing compiler changes: the dev-shell `jet` wrapper runs
`target/debug/jet`. A full `/tmp` can produce false ENOSPC failures; check it
before trusting one. The verification skill owns deeper snapshot, golden,
formatter, grammar, and full-suite traps.

## Invariants

Violating an invariant means stop and fix it.

- **I1 — Safety.** Jet is memory-safe and type-safe by default. Expert escape
  uses user-written audited `#Unsafe("reason") { … }` or
  `#Unsafe("reason") fn` regions. Generated Rust `unsafe` may appear only
  there or in vetted std/mem internals.
- **I2 — rustc is hidden.** rustc rejection of generated code is an internal
  compiler error (exit 101), never a user diagnostic.
- **I3 — Sema checks.** All checking lives in sema. Codegen is dumb; never use
  “try rustc and see” as validation.
- **I4 — Diagnostics are products.** Every diagnostic has a registered code,
  what/why/fix text, and a UI snapshot. No snapshot means no diagnostic.
- **I5 — Examples are executable specs.** Every feature ships with an example
  and golden-tested output.
- **I6 — Compiler seams are dependency-free.** The root compiler and compiler
  seam crates accept only path dependencies. Existing ratified stdlib bootstrap
  dependencies remain temporary; any new stdlib external dependency requires
  owner approval.
- **I7 — Syntax is ratified.** Every user-typeable keyword or sigil lives in
  `crates/jet-foundation/src/Syntax.rs` with a decision ID.
- **I8 — One mechanism.** One canonical semantic mechanism may have flexible
  spelling and organization. Keep the beginner surface small and safe; expose
  expert control through explicit opt-in. New mechanisms require a roadmap slot
  or owner approval.

## GPT-5.6 model policy

Use GPT-5.6 Sol by default. Adjust Sol reasoning effort to task uncertainty and
risk: low for bounded mechanical work, medium for normal implementation, high
or xhigh for architecture, compiler semantics, hard debugging, and first-pass
review. Prefer raising or lowering Sol effort over switching model families.

Use Terra for the mandatory second review and only otherwise when a concrete,
task-specific reason makes it better than Sol. Record that reason. Do not route
work to Terra from habit. Use Luna only when the owner asks or measurements show
a stable advantage on high-volume, fully mechanical work.

Give agents a clear goal, relevant context, hard constraints, owned paths, and
observable done conditions. Avoid step-by-step micromanagement when tests and
acceptance criteria can express the result.

## Working method

Before editing:

1. state the goal and done condition;
2. inspect `git status`, relevant diffs, active Tower claims/tasks, and
   worktrees;
3. identify authoritative decisions and owner gates;
4. claim one coherent work package and name owned paths;
5. choose targeted proof before implementation.

Use the `ponytail:ponytail` skill for coding, refactoring, fixes, review, and
technical design. Choose the smallest complete solution: standard library and
existing project mechanisms before new dependencies or abstractions. Ponytail
never permits cutting ratified scope, safety, necessary tests, or end-to-end
behavior. No stubs, facades, speculative extension points, or parallel
mechanisms.

Write a failing behavioral test or executable example first when feasible, then
implement the smallest complete vertical slice. For language features, preserve
parser → sema → TIR/codegen → JIT/dev parity and update touched docs. Difficulty
and duration do not lower the required outcome.

Before plans, ballots, or public frontend acceptance, run both passes:

- **Beginner:** safe useful defaults, no unnecessary ceremony or policy jargon.
- **Expert:** explicit control over targets, effects, generated code, toolchains,
  scheduling, caching, and audit output.

A frontend is accepted only from the real terminal/browser state matrix:
archetypes, viewports, states, keyboard/focus paths, and ANSI/`NO_COLOR` where
relevant—not prose or a selected screenshot.

## Owner gates

Before coding, enumerate new syntax, a new stdlib external dependency, an
invariant carve-out, and any other owner-only call. For Jet project work, make
each choice ballot-ready in Tower, then pause only the gated slice. Work on
independent ungated slices meanwhile. Never hand-edit `.tower/` data.

After ratification, implement the complete ruling and acceptance terms. Syntax
changes update `Syntax.rs`, parser behavior, snapshots, and
`docs/spec/syntax-decisions.md`; additions or removals also run
`scripts/agent/jet-env jet self devtools grammars`.

When the owner explicitly says a task is outside the Jet decision system, raise
choices directly in chat rather than creating Tower ballots.

## Concurrency, delegation, and worktrees

One implementer owns each coherent change. Never have multiple agents edit the
same patch. Delegate only bounded, independently useful work; one layer deep;
never delegate a single shell command.

Before delegation, give the agent: goal, owned paths, relevant authority,
invariants, done conditions, and targeted tests. Start sub-agent chatter with
the `caveman:caveman` skill where available. Product copy, specs, diagnostics,
ballots, and commit messages remain normal prose.

Shared-tree safety is absolute:

- never run `git add -A`, broad `git commit -a`, `git restore .`, or equivalent;
- never stage, commit, overwrite, clean, or revert another task's paths;
- stage and commit only explicitly owned paths after inspecting the diff;
- if ownership or collision is unclear, stop and resolve it before writing.

Worktrees are allowed and preferred for concurrent write-capable work. Their
lifecycle is part of done:

1. record task/card, owner, branch, and path before work starts;
2. keep one coherent change per worktree and commit only owned paths;
3. test there, then integrate successful work into the intended branch promptly;
4. verify the integrated diff and tests;
5. remove the worktree and its temporary branch immediately;
6. confirm `git worktree list` contains no orphan from the task.

Do not leave “finished” work parked on an unmerged branch or worktree. If work
must stop, make a coherent checkpoint, record exact resume state and owner in
Tower/task handoff, remove the worktree, and retain only the named branch until
resumed or explicitly abandoned.

## Review and verification

Every completed change has one implementer and two fresh-context adversarial
reviewers in this order:

1. **Sol reviewer:** inspect the diff, acceptance criteria, invariants, and test
   evidence; assume the patch is wrong; report only concrete findings.
2. The implementer fixes findings; the Sol reviewer rechecks material fixes.
3. **Terra reviewer:** independently inspect the resulting patch with the same
   evidence and no access to the implementer's rationale.
4. The implementer fixes findings; the Terra reviewer rechecks material fixes.

Reviewers do not implement. They check missing paths, semantic and safety bugs,
false-green tests, stale decisions, accidental scope, duplicate mechanisms, and
orphaned work. A green build never waives review.

Use targeted tests during implementation and both reviews. Only the orchestrator
runs `scripts/agent/jet-env full scripts/agent/verify-full.sh`, once after a
major integrated push or on a blocking closeout card; CI also runs the full
suite. Keep normal test parallelism unless reproducing a specific race.

Done means: integrated code matches current authority; targeted tests pass;
docs/examples/snapshots match behavior; both reviews close; Tower/task state is
accurate; no owned worktree or temporary branch remains; and the final report
names tests, commits, open gates, and any retained handoff branch.

## Style

Be terse and precise. Say each rule once. Plain std-only Rust; small modules; no
cleverness in codegen. Treat error text as snapshot-tested product copy. When in
doubt, `docs/spec/philosophy.md` decides: effort is expendable; safety and the
beginner experience are not.
