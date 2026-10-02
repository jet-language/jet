# AGENTS.md — Jet agent contract

This is Jet's single policy for every agent and tool. It covers authority,
documentation, invariants, the performance gate, work state, delegation, and
proof. Strategy lives in `Docs/spec/philosophy.md`. Plans and work state live
only in Tower. Procedures live in skills. Current behavior is established by
code, executable registries, and exercised tests and examples.

## Authority

Read `Docs/spec/philosophy.md` for Jet's purpose and ranked design priorities.
Do not copy that guidance into other documents.

Code is the source of truth for what Jet does. Specs explain durable contracts
and their reasons; they do not prove that a feature works, is missing, or is
complete. When code conflicts with an approved requirement, treat it as a
defect: investigate it and record it in Tower. Do not bless the implementation
or repeat stale prose.

When rules about intended behavior or agent conduct conflict, this order
decides:

1. The owner's latest explicit instruction.
2. A ratified Tower decision and its acceptance terms.
3. The relevant domain specification or ADR.
4. This contract.
5. The task-specific skill.

A newer ruling from a higher authority wins. Never average conflicting rules.
When a conflict or owner gate blocks one slice of work, stop only that slice
and continue with independent work.

## Decision levels

| Level | Decides | Central question |
|---|---|---|
| **Strategic** | Purpose and direction: desired outcomes, priorities, boundaries, trade-offs, and what success means. It may question whether work is worth doing at all. | What are we trying to accomplish, and why? |
| **Operational** | The approach: what must exist or change, how the parts fit, sequencing, scope, dependencies, milestones, and acceptance criteria. | What approach and body of work will achieve the outcome? |
| **Tactical** | Concrete actions: implementation details, specific changes, immediate problems, and verification. | What exactly do we do, and how do we show it worked? |

Tactical findings can reveal that an operational approach or a strategic
assumption needs to be revisited. Raise them; do not silently absorb them.

## Documentation

- **One strategy document.** `Docs/spec/philosophy.md` holds the durable vision
  and design priorities. It never holds schedules, feature inventories,
  progress notes, or release scope. Do not start a separate roadmap without
  owner direction.
- **One planning system.** Tower holds goals, plans, milestones, dependencies,
  decisions, acceptance criteria, status, blockers, and handoffs. Move live work
  into Tower before deleting a document copy. Discard superseded plans instead
  of keeping a second queue.
- **Executable truth.** Spellings, APIs, diagnostics, defaults, and constraints
  live in their code or registry home. Tests, snapshots, and executable
  examples prove behavior. Generate reference output on demand; never commit
  generated status or duplicate catalogs into docs.
- **Short explanations.** Specs and guides explain durable contracts,
  non-obvious reasons, and how to use or change the system. Link to executable
  sources instead of maintaining feature lists, limitation lists, coverage
  tables, or implementation maps. Fix stale prose by cutting or correcting it,
  never by redefining reality.
- **Evidence is not work state.** Audits and research keep dated findings.
  Proposals keep alternatives and reasons tied to a Tower decision. None of
  them may own a plan, goal, or live status. Historical evidence never
  establishes current behavior.

`Docs/` has four document categories: `spec`, `audits`, `research`, and
`proposals`. `Docs/README.md` is navigation only; `Docs/site` is the generated
project website and `Docs/assets` holds images. Only the owner may delete
audits or final reports; agents may move them intact. Do not create cleanup
reports, preservation maps, archive folders, or recurring documentation
janitors. Keep unique decision rationale where it already lives, and use Tower
and source history for work tracking and recovery.

## Repository layout

| Path | Holds |
|---|---|
| `Source/`, `crates/` | The Rust reference implementation: the `jet` CLI and its crates (`crates/vendor/` holds patched upstream crates) |
| `Compiler/`, `Jetpack/` | Staged Jet-authored ports of the compiler and the package manager |
| `Core/` | Core library sources |
| `Examples/` | Executable examples and golden output |
| `tests/` | Rust integration tests, UI snapshots, fixtures, and compiler proofs |
| `Docs/` | Documentation, the website, and assets |
| `Tools/` | Contributor tooling: `agent/` (environment and gates), `ci/`, `perf/`, `gauntlet/`, `editors/`, `tower/`, and more |

`crates/` and `tests/` keep Cargo's lowercase names; every other top-level
folder is capitalized. Keep the top level small: new tooling goes under
`Tools/`, new documentation under `Docs/`. Never commit build output, scratch
files, or one-off run artifacts.

## Greenfield cutover

Jet owes no compatibility to its own history until the owner declares
otherwise. Ship one canonical current form. When syntax, semantics, an API, an
ABI, a format, a name, a path, or a command changes, migrate every in-repository
caller, example, test, generated artifact, schema, and affected explanation in
one cutover. Remove old spellings, aliases, shims, fallback parsers, legacy
readers, version branches, and parallel implementations. Ratification history
belongs in Tower and necessary design rationale in specs, never as retired
behavior in the compiler or tools. A compatibility exception needs an
owner-ratified decision naming its exact scope and removal condition.

## Invariants

A violation stops the affected work until it is fixed at the root.

- **I1 — Safety.** Jet is memory-safe and type-safe by default. The expert
  escape hatch is a user-written, audited `#Unsafe("reason") { … }` block or
  `#Unsafe("reason") fn`. Generated Rust `unsafe` appears only there or in
  vetted standard-library and memory internals.
- **I2 — Hidden backend.** If rustc rejects generated code, that is an internal
  compiler error with exit code 101, never a user diagnostic.
- **I3 — Sema owns checks.** All language checking lives in sema. Codegen lowers
  facts sema already established; it never probes rustc to find user errors.
- **I4 — Complete diagnostics.** Every diagnostic has a registered code,
  what/why/fix text, and a UI snapshot. Without the snapshot it is not done.
- **I5 — Executable examples.** Every feature has an example with golden-tested
  output, and the example proves the same meaning on every applicable tier.
- **I6 — Dependency seams.** The compiler and its seam crates use path
  dependencies only. The ratified stdlib bootstrap dependencies are temporary.
  Any new stdlib external dependency needs an owner decision and an approved
  bridge pattern.
- **I7 — Ratified syntax.** Every keyword or sigil a user can type is registered
  in `crates/jet-foundation/src/Syntax.rs` and tied to a decision ID.
- **I8 — One mechanism.** Each meaning has one canonical semantic mechanism.
  Spelling or organization may flex only where that helps users. Keep the
  beginner surface small and safe, and expose expert control only by explicit
  opt-in. A new mechanism needs approved Tower scope or owner approval.
- **I9 — One meaning across tiers.** Every language feature and Core API has
  one executable meaning on every tier: the Jet code generator's levels (O0
  for `jet run`, `jet dev`, and compile-time code; O1 for `jet build`; O2 for
  `--release`) and web where applicable. That meaning lives in the embedded
  Prelude and the ratified CoreLib, compiled once into the one shared runtime
  (D-EXEC1). Code generators and hosts only marshal values and call the same
  runtime symbols; they never re-encode validation, defaults, policy, or error
  meaning. Never mark a feature as working on one level only, or defer it to a
  later level. Until D-EXEC1's retirement step lands, the Cranelift JIT and the
  interpreter still serve `jet run` and must keep that same meaning; do not
  extend them. An owner-ratified exception must name the tier it excludes.

## Performance gate

Jet must strictly beat every matched peer on every required cell and metric.
Against Rust, a same-run Jet/Rust ratio of `1.05` or lower counts as parity:
that band is measurement noise, not a target and not a win. Against every other
peer, Jet/peer must be below `1.00`.

Required cells cover the foundations (numerics, text, files, concurrency,
networking, build time, and run time) plus one real workload in each critical
area: web, games, CLI and scripts, data analysis, backend services, GUI
applications, and embedded. A niche becomes required only when
Jet ships a first-party battery for it; never claim a niche win without one.

Apply the comparator separately to each cell and metric. Never average away a
loss, substitute an easier workload or tier, drop a peer, or accept wrong,
unavailable, uncovered, mismatched, or inconclusive evidence. A performance
card, milestone, dashboard, or release gate stays open while any required cell
fails. Never trade semantics, diagnostics, determinism, safety, or I9 parity
for a score. Manifests and gates must encode this policy; prose alone is not
evidence. Historical receipts stay immutable under the policy they were
recorded with and never weaken the current gate.

A surface added for performance must arrive with a paired two-program cell
comparing it with the plain spelling it replaces. Ratification waits until the
surface strictly beats the plain form. Record the pair in the canonical
manifest and keep any loss on a card.

## Owner gates and work state

Before coding, identify the choices only the owner can make: new syntax, public
API, commands, dependencies, invariant or I9 exceptions, epoch or scope moves,
product behavior, and visual direction. Put each one on a Tower ballot with
same-program alternatives, exact syntax and behavior, trade-offs, edge cases,
and the beginner and expert paths. Do not ballot implementation choices an
approved contract already covers. A question is not approval, and a ratified
outcome stays in force until the owner changes it.

Tower is the only work ledger. Every incomplete stream has exactly one card
recording its phase, plan, dependencies, criteria, and handoff state. Workers
never write to Tower, close cards, or keep a competing task list. The
orchestrator integrates a valid result, runs the exact focused proof the
criteria name, records the evidence, closes the card, and confirms it is `done`
before claiming or briefing more work. A milestone then gets one commit-bound
composed sweep and one fresh-context review; individual cards do not wait for
that gate.

One implementer owns each coherent patch. Concurrent writers need disjoint
paths and one named close owner. Default to a single delivery stream, and add
streams only when paths, integration, tests, and resources stay clean. Use only
in-repository worktrees under `.claude/worktrees/<name>` or
`.agent-worktrees/<name>`, share the bounded main `target/`, integrate
promptly, and remove finished worktrees and temporary branches. Never overwrite
another task's paths. Never run `git add -A`, a broad `git commit -a`,
`git restore .`, or any equivalent broad operation. Never hand-edit
`Tools/tower/.tower/`.

Only the owner starts `tower serve` unless the owner directs otherwise. Agents
use the non-serve Tower CLI (`node Tools/tower/tower.mjs`) against the main
board and report any stale or duplicate server. Never touch the owner's
personal notes or scratch files.

## Delegation

Use the most specific agent available for each role. Workers do not spawn
workers. Every code worker returns the exact `CHECK OK` receipt from
`Tools/agent/lane-check.sh`; prose-only or static-data-only work returns
`DOCS ONLY`. The orchestration skill defines briefs, liveness, integration,
proof cadence, recovery, and closure.

This repository carries no harness-specific configuration. Agent tools use
their own global settings; Jet contributes only this contract and the skills
under `.agents/skills` and `Tools/tower/skills`.

## Environment and proof

Run repository commands through `Tools/agent/jet-env`. Keep scratch and logs on
disk under `~/.cache/jet-test-scratch` and `~/.cache/jet-luna`, never in
`/tmp`, which is RAM-backed. Share one bounded Cargo target and respect the default
`JET_TARGET_CAP_GB=120`. Keep `CARGO_INCREMENTAL=0`, except in the
orchestrator's lock-serialized build targets, which opt in with
`JET_CARGO_INCREMENTAL=1` and are pruned between builds.
Rebuild before compiler smoke tests. After integration, run the exact narrow
proof the criteria name. Broad suites, unfiltered censuses, and
`Tools/agent/verify-full.sh` are milestone or release operations and need the
required token. A worker's type-check is not proof of runtime behavior, tier
parity, goldens, snapshots, or generated artifacts.

Hosted CI currently runs only a workspace `cargo check`
(`.github/workflows/ci.yml`); the full gate runs locally through
`Tools/agent/verify-full.sh`.

## References

Read this file first, then load only what the task needs:

| When the task involves | Read |
|---|---|
| Dispatch, waves, worktrees, receipts, recovery, or card closure | `.agents/skills/orchestration/SKILL.md` |
| Compiler or language semantics | The relevant code, examples, and tests; `Docs/spec/syntax-decisions.md` for decision rationale |
| Diagnostics or snapshots | `crates/jet-codegen/src/Prelude/Diagnostics.jet`, the matching UI snapshots, and `Docs/spec/diagnostics.md` |
| Examples or golden paths | `Docs/spec/contributing/examples.md` and `Examples/README.md` |
| Tower cards, questions, tags, or board operations | `Docs/spec/contributing/issue-tracker.md`, `Docs/spec/contributing/triage-labels.md`, and `Tools/tower/skills/tower/SKILL.md` |
| Domain vocabulary or decisions | `Docs/spec/vocabulary.md`, `Docs/spec/syntax-decisions.md`, and `Docs/spec/contributing/domain.md` |
| Security closure | `Docs/spec/contributing/security-closure.md` |
| Technical traps | `Docs/spec/contributing/agent-engineering.md` |
| Audits, research, cleanup, or skill routing | `.agents/skills/JetSkillsRouter.md` |
| Completion claims or milestone closeout | `.agents/skills/verify/SKILL.md` |

For coding and technical design, understand the whole path first, then choose
the smallest complete change. Write durable artifacts in clear, plain prose.
When a task asks for implementation, do not stop at a report, and never claim
work that the board or an exercised command does not prove.
