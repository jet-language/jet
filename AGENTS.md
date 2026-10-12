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
  later level. Until native in-memory O0 lands, `jet run` and `jet dev` use the
  normal build path and execute the result (D-EXEC2=A); the in-process
  evaluator serves compile-time code (and the Canvas dev host) and must keep
  that same meaning; do not extend it. An owner-ratified exception must name
  the tier it excludes.

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
paths and one named close owner. The repository's working branches are
`master` and `dev`, with one dev worktree. At the Rust cutover, the last
commit that still contains the Rust implementation becomes the `rust` branch: a
frozen dead-end leaf kept as a reference and a re-bootstrap safety net. Nobody
commits to it or merges it into `dev` or `master`. Every agent commits directly
on `dev` in the dev worktree, in small commits that stage only its own paths.
Never create any other branch, a worktree, or any other ref (`refs/keep`,
`refs/salvage`, `refs/archive`, private lineages), for any purpose: experiments,
candidates, measurements, and proofs included. To build or measure an
uncommitted change,
apply a scratch patch to a temporary `git archive` export outside the
repository. Use checkout-owned external Cargo targets configured through
`jet-env` (see the orchestration resource reference). Never overwrite another
task's paths. Never run `git add -A`, a broad `git commit -a`, `git stash`,
`git restore .`, `git reset --hard`, or any equivalent broad operation. Never
hand-edit `Tools/tower/.tower/`.

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

## Find every issue at once

This is a standing owner order, repeated many times; violating it wastes hours.
It binds every agent on every step of every effort, whether or not the owner
repeats it, and nothing overrides it short of an explicit owner instruction to
rerun one thing. Never discover problems serially, where one blocker is found,
fixed and rerun to find the next. Before fixing anything, collect every issue
in one pass:

- **Expensive launches are gated, never bypassed.** A stage0, self-hosting
  chain, release build or any other run costing more than about 15 minutes
  starts only through tooling that refuses to launch without a collect-all
  preflight receipt for that exact commit
  (`~/.cache/jet-dev/scratch/Main/preflight.mjs`, enforced by
  `prepare-sharded-chain.mjs`). The receipt runs every later stage's cheap
  probe on the new sources (parse, generators, comptime-host count, and the
  last good factory's keep-going censuses of every unit through every phase it
  can reach). Every row is clean or carries a written disposition naming the
  commit in this launch that fixes it. Launching to "see what fails next" is
  forbidden.
- **Every compiler phase has a collect-all mode, or that is the first defect.**
  Parse, imports, sema, comptime, codegen lowering, backend lowering, machine
  emission, object, link and run must each record a failing item as a census
  row, trap or skip it, and continue. A process-stopping `panic` that guards
  an assumption about checked input is a collect-all defect: convert it to a
  keep-going report before relying on that phase to gate anything.
- **Use waiting time to run later stages early.** While an expensive build
  runs, probe every later stage with the best available artifact (the last
  good factory, an older compiler, a dry run) and fix what they find in the
  same batch.
- **Run every stage in collect-all mode.** Use the existing keep-going
  mechanisms (`JET_KEEP_GOING=1` all-phases reports,
  `JET_DEBUG_LOWER_ALL=1` lowering sweeps, per-error census scripts) so one run
  reports all errors in all items, not the first one. A tool that stops at its
  first failure needs a collect-all mode before it gates anything.
- **Preflight the whole pipeline end to end first.** Before starting a
  multi-stage effort (self-hosting, a cutover, a release candidate), dry-run or
  probe every stage up front: parse, imports, sema, lowering, emission,
  generated tables, runtime pack, link, run, tests. That maps every blocker,
  dependency and missing piece before the first fix.
- **Partition when a run is slow or hangs.** Split the input and check the parts
  in parallel to localize every failure at once.
- **Fix the whole list in parallel.** Group the findings by root, give each root
  one owner, and land them as one batch before the next expensive build or
  check. A rebuild that exists only to reveal the next error is a process
  failure.
- **Gate every commit cheaply first.** Parse and check each touched file,
  regenerate and commit generated outputs with their sources, and run the
  generator checks against the exact source a build will use, so no expensive
  run is lost to a mistake a fast check would catch.
- **Run independent stages in parallel.** A pipeline's dependency order does not
  make its probes serial. Start every stage that can run now (a host build
  beside a sweep, checks of later-stage inputs beside earlier stages), and
  make each stage keep going past its failures far enough that the next stage's
  own collect-all census runs in the same cycle (for example, write partial
  output so a downstream type check reports everything else at once).
- **A stage only stops at a failure that blocks every later item.** Everywhere
  else it records the failure, skips the item and continues, then reports the
  full list and returns the first error.
- **Never claim a job started until it is observed running.** After releasing
  queued work, confirm it in the scheduler's actual running state (the real
  queue path, its start decision, its live process) before reporting it as
  started; a misplaced job file is a silent day-long stall.

## Environment and proof

Run repository commands through `Tools/agent/jet-env`. Keep scratch and logs on
disk and build output outside the repository. Follow the orchestration resource
reference for bounded, checkout-owned targets and shared artifacts; respect
`JET_TARGET_CAP_GB=120`. Keep `CARGO_INCREMENTAL=0`, except
in the orchestrator's lock-serialized build targets, which opt in with
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
