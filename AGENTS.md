# AGENTS.md — Jet agent operating manual

Canonical policy for every coding agent. `CLAUDE.md` is a symlink here; do not fork per-tool copies.
Put procedures in skills, design in specs, work state in Tower, and deterministic enforcement in tests or hooks.
Codeflow is the preferred owner of systematic and larger-scope orchestration when available; this file does not
duplicate its workflow.

## Mission and authority

Jet is a dual-facet, memory-safe compiled language: magic for beginners, full expert control behind explicit opt-in.
The front end owns semantics and user-facing errors; rustc is hidden. The human owner decides user-facing syntax.

Resolve guidance in this order:

1. the owner's current explicit instruction;
2. ratified Tower verdicts and their acceptance terms;
3. the relevant domain spec;
4. this file and the nearest nested `AGENTS.md`;
5. task-specific skills and prompts.

Code shows implementation state, not design authority. A newer ratified ruling beats stale code or prose. On conflict,
follow the higher authority, record it, and stop only the affected slice. Never average contradictory rules.

## Load context by trigger

Read this file, then relevant code, tests, and the current diff. Load only task-triggered references:

- language semantics or syntax: relevant sections of `docs/spec/philosophy.md`,
  `docs/spec/syntax-decisions.md`, and `docs/spec/architecture.md`;
- diagnostics: `docs/spec/diagnostics.md` and the matching UI snapshots;
- Tower board mechanics or owner decisions: `plugins/tower/skills/tower/SKILL.md`, plus
  `plugins/tower/skills/tower-ballot/SKILL.md` when a choice is owner-gated;
- Tower backlog ranking: `plugins/tower/skills/tower-burndown/SKILL.md`; Codeflow owns
  campaign execution, delegation, checkpoints, and review;
- completion claims: `.claude/skills/verify/SKILL.md`;
- systematic, multi-part, ambiguous, or larger-scope work: `codeflow` when available;
- a specialized task: the matching skill named in the request or skill catalog.

Do not front-load every spec, plan, prompt, or board record. Search first; read the smallest authoritative slice.

## Command environment

Run project commands through `scripts/agent/jet-env`:

```sh
scripts/agent/jet-env cargo build
scripts/agent/jet-env cargo test --test NAME
scripts/agent/jet-env jet run examples/features/basics/hello.jet
scripts/agent/jet-env rg "pattern" docs Source crates tests
```

Use `scripts/agent/jet-env full <command>` only for FFI, browser/graphics, VM/image, or full verification. Do not rely
on host tools unless testing host-shell independence. Group dependent checks when practical.

Rebuild before compiler smoke tests: the `jet` wrapper runs `target/debug/jet`. Check `/tmp` before trusting ENOSPC.
The verification skill owns snapshot, golden, formatter, grammar, and full-suite traps.

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

Use GPT-5.6 Sol exclusively; the `gpt-5.6` alias is Sol. Use only low, medium, or high effort: low for bounded
mechanics, medium for normal implementation, and high for semantics, architecture, hard debugging, and review. Do not
route implementation, orchestration, or review to another model family.

Give agents a clear goal, relevant context, hard constraints, owned paths, and observable done conditions. Prefer
tests and acceptance criteria over step-by-step micromanagement.

## Workflow ownership

When Codeflow is installed and available, use it for systematic, multi-part, ambiguous, long-running, or larger-scope
work. Codeflow owns planning, delegation, checkpoints, resumability, and phase mechanics. This manual supplies Jet's
authority, invariants, environment, ownership guards, review requirement, and done conditions. Do not restate or
extend Codeflow's orchestration algorithm in this file or repo prompts. Domain skills still own their mechanics;
Codeflow coordinates them rather than replacing them. If Codeflow is unavailable, do not create a durable competing
workflow while completing the task with the active harness.

Keep bounded work inline. Before writing, inspect relevant Git/Tower ownership and the authoritative decision. Search
before broad reading; choose targeted proof before implementation.

Use `ponytail:ponytail` for coding, refactoring, fixes, review, and technical design. Choose the smallest complete
solution: standard library and existing mechanisms before dependencies or abstractions. Never cut ratified scope,
safety, necessary tests, or end-to-end behavior. No stubs, facades, speculative extension points, or parallel mechanisms.

Write a failing behavioral test or executable example first when feasible, then the smallest complete vertical slice.
Language features preserve parser → sema → TIR/codegen → JIT/dev parity and update touched docs. Difficulty and
duration do not lower the outcome.

Before plans, ballots, or public frontend acceptance, run both passes:

- **Beginner:** safe useful defaults, no unnecessary ceremony or policy jargon.
- **Expert:** explicit control over targets, effects, generated code, toolchains,
  scheduling, caching, and audit output.

A frontend requires the real terminal/browser state matrix: archetypes, viewports, states, keyboard/focus paths, and
ANSI/`NO_COLOR` where relevant—not prose or a selected screenshot.

## Owner gates

Before coding, enumerate new syntax, a new stdlib external dependency, an
invariant carve-out, and any other owner-only call. For Jet project work, make
each choice ballot-ready in Tower, then pause only the gated slice. Work on
independent ungated slices meanwhile. Never hand-edit `.tower/` data.

After ratification, implement the complete ruling and acceptance terms; the verification skill owns syntax chores.

When the owner explicitly says a task is outside the Jet decision system, raise
choices directly in chat rather than creating Tower ballots.

## Ownership and worktrees

One implementer owns each coherent patch. Concurrent writers need disjoint paths. Codeflow or the active specialized
skill decides delegation mechanics. Start agent chatter with `caveman:caveman` where available; product copy, specs,
diagnostics, ballots, and commits use normal prose.

Shared-tree safety is absolute:

- never run `git add -A`, broad `git commit -a`, `git restore .`, or equivalent;
- never stage, commit, overwrite, clean, or revert another task's paths;
- stage and commit only explicitly owned paths after inspecting the diff;
- if ownership or collision is unclear, stop and resolve it before writing.

Worktrees are allowed for isolated or concurrent writes. Record ownership before use. Integrate successful work into
the intended branch promptly, verify it there, then remove the worktree and temporary branch immediately. Never park
finished work unmerged. Paused work keeps a named coherent handoff branch and resume note, not an orphaned worktree.

## Review and verification constraints

Every completed change has one implementer and one fresh-context Sol reviewer:

1. **Sol reviewer:** inspect the diff, acceptance criteria, invariants, and test
   evidence; assume the patch is wrong; report only concrete findings.
2. The implementer fixes findings; the Sol reviewer rechecks material fixes.

The reviewer does not implement. They check missing paths, semantic and safety bugs,
false-green tests, stale decisions, accidental scope, duplicate mechanisms, and
orphaned work. A green build never waives review.

Use targeted tests during implementation and review. The verification skill owns when to run
`scripts/agent/jet-env full scripts/agent/verify-full.sh`; CI runs it again. Keep normal parallelism unless reproducing
a race.

Done means: integrated code matches current authority; targeted tests pass;
docs/examples/snapshots match behavior; the Sol review closes; Tower/task state is
accurate; no owned worktree or temporary branch remains; and the final report
names tests, commits, open gates, and any retained handoff branch.

## Style

Be terse and precise. Say each rule once. Plain std-only Rust; small modules; no
cleverness in codegen. Treat error text as snapshot-tested product copy. When in
doubt, `docs/spec/philosophy.md` decides: effort is expendable; safety and the
beginner experience are not.
