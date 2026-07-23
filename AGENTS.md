# AGENTS.md — Jet agent operating manual

Canonical policy for every coding agent. `CLAUDE.md` is a symlink here; do not fork per-tool copies.
Put procedures in skills, design in specs, work state in Tower, and deterministic enforcement in tests or hooks.

## Mission and authority

Jet is a dual-facet, memory-safe compiled language: magic for beginners, full expert control behind explicit opt-in.
The front end owns semantics and user-facing errors; rustc is hidden. The human owner decides user-facing syntax.

Resolve guidance in this order:

1. the owner's current explicit instruction;
2. ratified Tower verdicts and their acceptance terms;
3. the relevant domain spec;
4. this file and the nearest nested `AGENTS.md`;
5. task-specific skills.

Code shows implementation state, not design authority. A newer ratified ruling beats stale code or prose. On conflict,
follow the higher authority, record it, and stop only the affected slice. Never average contradictory rules.

## Load context by trigger

Read this file, then relevant code, tests, and the current diff. Load only task-triggered references:

- language semantics or syntax: relevant sections of `docs/spec/philosophy.md`,
  `docs/spec/syntax-decisions.md`, and `docs/spec/architecture.md`; adding syntax
  uses `.agents/skills/verify/SKILL.md`;
- diagnostics: `docs/spec/diagnostics.md` (including "Adding a diagnostic") and
  the matching UI snapshots;
- FFI bridges: `docs/spec/architecture.md` ("Adding an FFI bridge");
- Tower board mechanics or owner decisions: `plugins/tower/skills/tower/SKILL.md`, plus
  `plugins/tower/skills/tower-ballot/SKILL.md` when a choice is owner-gated;
- Tower backlog ranking: `plugins/tower/skills/tower-burndown/SKILL.md`;
- Jet audits / research / cleanup routing: `.agents/skills/JetSkillsRouter.md`;
- completion claims: `.agents/skills/verify/SKILL.md` (code closeout only);
- a specialized task: the matching skill named in the request or skill catalog.

Do not front-load every spec, plan, skill, or board record. Search first; read the smallest authoritative slice.

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

Start the local Tower board with the one canonical command:
`node plugins/tower/tower.mjs serve --open`. Board data lives in
`plugins/tower/.tower/`. Read and write board state through that CLI.

## Invariants

Violating an invariant means stop and fix it.

- **I1 — Safety.** Jet is memory-safe and type-safe by default. Expert escape
  uses user-written audited `@Unsafe("reason") { … }` or
  `@Unsafe("reason") fn` regions. Generated Rust `unsafe` may appear only
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

Keep bounded work inline. For larger work, use the active harness plus Tower.
Do not invent a durable competing planner, phase model, or orchestration product
in this repo.

Before writing, inspect relevant Git/Tower ownership and the authoritative decision. Search
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
independent ungated slices meanwhile. Never hand-edit `plugins/tower/.tower/` data.

Kill a design slice before ballot or code when it breaks an invariant,
duplicates a mechanism, burdens the beginner default without necessity, or
hides expert control or auditability. Otherwise, unresolved owner choices go
through the Tower ballot workflow; a ratified verdict and its acceptance terms
remain law until a later owner verdict amends them.

After ratification, implement the complete ruling and acceptance terms; the verification skill owns syntax chores.

When the owner explicitly says a task is outside the Jet decision system, raise
choices directly in chat rather than creating Tower ballots.

## Ownership and worktrees

One implementer owns each coherent patch. Concurrent writers need disjoint paths. The active specialized
skill decides delegation mechanics when needed. Start agent chatter with `caveman:caveman` where available; product copy, specs,
diagnostics, ballots, and commits use normal prose.

Default to one active delivery stream. Expand concurrency only when each stream
has disjoint write paths and tests, a clean integration target, and one named
close owner. Contract when streams share compiler seams, contend for build
resources, or produce an integration backlog. Never start new work while a
reviewed or completed patch is waiting to integrate. This is an adaptive rule,
not a fixed worker cap.

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

Technical verification is agent-owned: the implementer runs every machine-verifiable
requirement, however many there are, and the independent reviewer validates the
evidence. Do not create owner-acceptance cards or `needsAcceptance` gates for
technical correctness. Reserve owner acceptance for unavailable hardware,
platforms, or real environments; visual confirmation the harness cannot perform;
or genuine UI, UX, or DX taste and design judgment. Tell the owner only what to
observe and why human inspection is needed, as a brief checklist; omit the machine
verification details agents already own.

Use targeted tests during implementation and review. Close each bounded card
from scoped proof and independent review. Run
`scripts/agent/jet-env full scripts/agent/verify-full.sh` once after a batch of
3–5 integrated card closures, at a major-push boundary, or when targeted
evidence identifies a repository-wide interaction. CI runs it again. An
unrelated full-suite failure becomes its own card and does not reopen a scoped,
proved closure. Keep normal parallelism unless reproducing a race.

Done means: integrated code matches current authority; targeted tests pass;
docs/examples/snapshots match behavior; the Sol review closes; Tower/task state is
accurate; no owned worktree or temporary branch remains; and the final report
names tests, commits, open gates, and any retained handoff branch.

## Style

Be terse and precise. Say each rule once. Plain std-only Rust; small modules; no
cleverness in codegen. Treat error text as snapshot-tested product copy. When in
doubt, `docs/spec/philosophy.md` decides: effort is expendable; safety and the
beginner experience are not.
