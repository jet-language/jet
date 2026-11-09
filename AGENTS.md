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
| `Compiler/` | The Jet compiler (native backend, runtime, embedded Prelude under `Compiler/Prelude/`) and its bootstrap assembly |
| `Jetpack/` | The Jet package manager |
| `Core/` | Core library sources |
| `Examples/` | Executable examples and golden output |
| `tests/` | Native test corpora (cases, UI snapshots), fixtures, and compiler proofs |
| `Docs/` | Documentation, the website, and assets |
| `Tools/` | Contributor tooling: `agent/` (environment and gates), `ci/`, `perf/`, `gauntlet/`, `editors/`, `tower/`, and more |

`tests/` keeps its lowercase name; every other top-level
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

- **I1 — Safety.** Jet is memory-safe and type-safe by default. Memory safety
  includes leak freedom: safe Jet cannot leak memory, and only code inside an
  `#Unsafe` block or function may. The expert escape hatch is a user-written,
  audited `#Unsafe("reason") { … }` block or `#Unsafe("reason") fn`. Generated
  `unsafe` operations appear only there or in vetted standard-library and
  memory internals.
- **I2 — Hidden backend.** If the native backend rejects or cannot lower checked
  code, that is an internal compiler error with exit code 101, never a user
  diagnostic.
- **I3 — Sema owns checks.** All language checking lives in sema. Codegen lowers
  facts sema already established; it never probes the backend to find user errors.
- **I4 — Complete diagnostics.** Every diagnostic has a registered code,
  what/why/fix text, and a UI snapshot. Without the snapshot it is not done.
- **I5 — Executable examples.** Every feature has an example with golden-tested
  output, and the example proves the same meaning on every applicable tier.
- **I6 — Dependency seams.** The compiler and its seam packages use path
  dependencies only. The ratified stdlib bootstrap dependencies are temporary.
  Any new stdlib external dependency needs an owner decision and an approved
  bridge pattern.
- **I7 — Ratified syntax.** Every keyword or sigil a user can type is registered
  in the syntax registry (`Compiler/JetFoundation/Data/Syntax`) and ratified on
  Tower. Decision IDs and decision history live in Tower and the specs, never
  in source: no decision fields in registries, no decision citations in code
  or comments.
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
  later level. `jet run` and `jet dev` compile at O0 and run the linked image
  from memory without writing an executable (D-EXEC2=A); compile-time code
  also runs natively at O0, in a child of the compiler, with the same meaning.
  No interpreter or second code generator serves any tier. An owner-ratified
  exception must name the tier it excludes.

## Performance gate

Jet's performance target has two stages, and each required cell and metric
must pass both. The first stage is the peer floor: Jet/peer must be below
`1.00` for every matched peer. Rust is judged like every other peer, with no
parity band. The second and final stage is the machine ceiling: hand-written,
maximally optimized machine code for the same workload. Jet must match or beat
it in both cycles and bytes (Jet/ceiling at most `1.00`). Beating the peers is
the floor, not the goal.

Every required cell needs a ceiling reference: an asm-parity cell
(Tools/perf/asm-parity) that names its gauntlet cell and does the work with the
best known technique for the workload (SIMD, algorithmic, or syscall-level),
not a naive transliteration. A cell without a reference is uncovered and fails
the gate, and a peer win never closes a cell whose ceiling is unmet. A better
technique replaces the reference and raises the target.

The ceiling delivers the same features as the Jet program. Where a Jet feature
(diagnostics, safety checks, fault reports) physically costs bytes or cycles,
the reference includes the smallest hand-written implementation of that
feature. Jet minimizes each feature's cost and never removes a feature to meet
the ceiling.

Close every performance gap at its root, in the general mechanism: the
codegen, runtime, or Core path that every program using the feature goes
through. Never tailor a fix to one cell: no pattern that recognizes a
benchmark program, no per-cell flag or constant, and no special case that only
that workload reaches. When the ceiling shows that a feature costs almost
nothing (for example, CPU time while waiting on I/O), Jet's general code for
that feature must cost almost nothing too. Every performance fix names the
feature it improves and proves the gain on a second program that uses the
feature differently from the cell.

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

This is a standing owner order, repeated hundreds of times: no agent asks the
owner questions. No questions in chat, none as card logs, notes or "owner
question" entries, and none that hold work. Within approved scope, the agent
decides, records the decision as fact on the card, and keeps working. An
owner-only choice goes on a ballot and the work moves to independent criteria
meanwhile. Workers send their questions to the orchestrator, and it answers them
the same way.

The orchestrator stays on the open goals in rank order, G1 and G2 first, until
every criterion is met. Implementers write the code, land it on LaneCheck and
report the commit; the orchestrator tests it in batches and sends every failure
back to its implementer to fix.

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
`master` and `dev`. At the Rust cutover, the last
commit that still contains the Rust implementation becomes the `rust` branch: a
frozen dead-end leaf kept as a reference and a re-bootstrap safety net. Nobody
commits to it or merges it into `dev` or `master`. Only the orchestrator
commits, directly on `dev` in the dev worktree, one commit per landed
submission that stages only that submission's paths.
Besides the master checkout, at most three worktrees may exist: the dev worktree
and up to two more that the orchestrator creates as detached checkouts of
`dev`. Each extra worktree has one named owner, is kept at a current `dev`
commit, holds no build output, and is removed as soon as it is idle; its work
reaches `dev` before removal. Never create any other branch, worktree, or other
ref (`refs/keep`, `refs/salvage`, `refs/archive`, private lineages), for any
purpose: experiments, candidates, measurements, and proofs included. Work
happens in these worktrees; no agent keeps its own copy of the repository. A
`git archive` export
outside the repository is allowed only for one build or measurement that can
use neither the worktree nor a published compiler (a release gate, or an A/B
base whose compiler is not published), and the job that made it deletes it
when it ends. Use checkout-owned external targets (see the orchestration
resource reference). Never overwrite another
task's paths. Never run `git add -A`, a broad `git commit -a`, `git stash`,
`git restore .`, `git reset --hard`, or any equivalent broad operation. Never
hand-edit `Tools/tower/.tower/`.

Scratch feeds work into `dev`; it is not an archive. Before removing anything,
save the work it holds: commit it on `dev`, or record on its Tower card exactly
what remains unlanded and why. Once the work is on `dev`, delete everything
that fed it: exports, build trees, staged patches, test scratch and logs. Each
agent deletes its own scratch when its commit lands. The orchestrator removes
leftovers at every milestone and whenever the scratch disk passes 75% full.
Keep only the compilers in use: the admitted seed, a candidate under test, and
the compilers a running job names. Never retire files by moving them elsewhere
on disk; git history and Tower are the record. Audits and final reports under
`Docs/` stay owner-only deletions (see Documentation).

Only the owner starts `tower serve` unless the owner directs otherwise. Agents
use the non-serve Tower CLI (`node Tools/tower/tower.mjs`) against the main
board and report any stale or duplicate server. Never touch the owner's
personal notes or scratch files.

Committed files carry no personal or machine information: no home paths, user
or host names, personal emails or machine mounts. Write a placeholder instead,
such as `$HOME`, `$USER`, `$JET_SCRATCH_ROOT` or `$JET_STATE_ROOT`; fixtures
use the example account `user` on `devhost`. This holds for recorded evidence
too. `Tools/agent/LaneCheck.jet` refuses a checkout that breaks it, through
its personal-information guard (`--personal-info`, #6564 c5).

## Adversarial design review

This is a standing owner order. No design may be built, shipped, or described
as better than existing solutions until an adversarial review has tried to
break each of its claimed strengths. The review is a dated audit in
`Docs/audits`. For every claim it:

- names the strongest existing solutions and states their best case fairly,
  with sources, including what they do that the design does not;
- attacks the claim: hidden costs, cheating (an easier workload, a narrower
  scope, a dropped feature, a deferred hard case), unhandled edge cases,
  failure modes, tooling, migration and ecosystem gaps, and what the design
  gives up;
- checks the claim against code, a runnable probe, or a measurement wherever
  one is possible, never prose alone;
- ends with a verdict: holds, holds only within stated limits, or fails, and
  puts every gap on a card or ballot.

Missing context is a failed claim, not a pass. A design with a failed claim or
an open gap stays unbuilt until it is redesigned and reviewed again. As with
ballots, every downside the review finds is engineered away, and the redesign
is reviewed again, before the design is presented to the owner as the
solution; a downside that cannot be removed is named with its cost and its
best mitigation. A ratified ballot authorizes the direction, not the claim;
its build card names the passing review as its first criterion. Public text
may call a design better only where a passing review backs that exact claim.

## Delegation

Use the most specific agent available for each role. Workers do not spawn
workers.

Workers are never idle or stalled. Each worker runs one continuous loop: write
the code to the card's spec, run only the touched files' source checks,
submit the patch to the orchestrator's inbox, and start its next task at once.
Workers never commit, build the compiler, or run suites, A/B comparisons,
chains, censuses, or releases. When a spec or criterion exists, the code comes
first: no worker stops to prove rows or waits for review or a test result.
While the orchestrator's batches run, every worker keeps coding to spec.

This is a hard owner requirement. The orchestrator never lets a worker idle.
No worker waits on a landing, a batch, a seed, a review, or a proof: it stacks
its next submission on its own unlanded ones and keeps writing code to spec.
Workers deliver large batches of work at once, never one small slice per test
cycle. When failures come back, the worker fixes the whole list in parallel
with its new work and resubmits it in one pass. A worker that is waiting is an
orchestrator failure. Fix it at once by queueing that worker's next task or
reassigning it.

The orchestrator does every build and test. It keeps every worker's next task
queued, integrates submissions in batches with one collect-all pass, and lands
each passing submission on `dev` as its own commit. A failing submission is
set aside without holding back the rest, and every failure goes back at once
to the worker that owns it, as one list and that worker's top priority. The
orchestration skill's intake reference defines the submission format, batch
integration, and landing; the skill also covers briefs, liveness, proof
cadence, recovery, and closure.

The orchestrator evaluates its own plan continuously, not only when something
fails. Before each step and whenever a result arrives, it asks whether this is
the fastest effective route to the owner's targets, whether a better approach
exists, and whether work, order, or agents should be rearranged to meet every
deadline. Then it acts on the answer: it re-routes work to whoever can finish
it soonest, puts the highest-value and quickest wins first so progress shows
early, and overlaps every independent stage. The orchestrator orchestrates; it
takes on implementation only when that is the fastest route and never at the
cost of monitoring, unblocking, and verifying.

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
  starts only after a clean collect-all preflight of that exact commit:
  `jet Tools/agent/Land.jet census` (the seed's keep-going `jet check` and
  `jet build` of the compiler, the runtime pack and the self-checks of the
  CLI and of every Core body, every row of every phase in one report; the
  lander runs it through `Land.jet
  precommit` before every compiler commit) and `jet Tools/agent/Chain.jet
  TREE LEVEL --dry-run`, which names every stage the launch would build. Every
  row is clean or carries a written disposition naming the commit in this
  launch that fixes it. Launching to "see what fails next" is forbidden. The
  launchers enforce it: a clean census writes the snapshot's receipt
  (`Land.jet receipt`, keyed on the compiler source identity and the seed),
  and `Tools/agent/Chain.jet TREE full` and `Tools/release/Main.jet` `admit`
  and `build` refuse to start without the receipt for their tree and seed;
  `--check-gate` gives the answer without launching.
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
- **A tightened rule sweeps everything first.** A change that makes sema,
  a guard or a gate stricter is checked, before it lands, against every Jet
  source the rule can reach: Core, Compiler, Jetpack, Tools, Examples and every
  tests/ tree, case fixture and template. Every hit is fixed in the same batch.
  Checking only what the compiler build or one suite touches is a serial
  discovery.
- **A failed gate is read whole.** When a release, chain or suite fails, read
  every lost row of every suite, sweep for the same root across the whole
  repository, and fix the full list as one batch before the next run.
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

Run repository commands with `jet` directly; `JET` names the admitted
native compiler (the stage-2 output of `jet build Compiler`) for
every tool and gate. Keep scratch and logs on disk and build output outside the
repository. Follow the orchestration resource reference for bounded,
checkout-owned targets and shared artifacts; respect `JET_TARGET_CAP_GB=120`.
Rebuild with `jet build --profile dev Compiler` (the fast build) before compiler
smoke tests. After integration, run the exact narrow proof the criteria name.
Broad suites, unfiltered censuses, and
`Tools/agent/VerifyFull.jet` are milestone or release operations and need the
required token. A worker's type-check is not proof of runtime behavior, tier
parity, goldens, snapshots, or generated artifacts.

Agent tooling lives in the repository: `Tools/agent/Chain.jet` (the self-host
chain and the machine build slots), `Tools/agent/Land.jet` (`serve` is the
lander, `batch` its batch landing, `census` the preflight, `publish` keeps
landed compilers for lanes; `queue` adds a land job for the lander; `land`
lands a patch stack directly, with `--scratch-copy` on a fresh clone),
`Tools/agent/Suite.jet` (one `jet test
Compiler --profile` run on a compiler snapshot) and `Tools/agent/ballots/Validate.jet` (the ballot validator).
Tools find state, scratch, queues, published compilers and seeds only through
the root variables of `Tools/agent/roots.sh` (shell) and `Tools/agent/roots.mjs`
(node). Their defaults derive from `$HOME` and the XDG directories; a machine
sets its own values in the environment or in
`${XDG_CONFIG_HOME:-~/.config}/jet/env`. No
committed file spells one machine's home, user, mounts or tool roots:
`Tools/agent/LaneCheck.jet`'s personal-information guard, which every lane
check runs first, refuses them, and
`tower redact` cleans the board.

There is no hosted CI until a pinned native seed release exists
(D-SEED-DIST1). `Tools/agent/LaneCheck.jet`, the Core surface ledger check
(`jet Tools/agent/CoreSurfaceLedger.jet --check`) and the full gate
(`JET=<jet> jet Tools/agent/VerifyFull.jet`, which includes the Core
source-coverage ratchet `Tools/agent/CoreCoverage.jet`) run locally with the
admitted `JET`.

## References

Read this file first, then load only what the task needs:

| When the task involves | Read |
|---|---|
| Dispatch, waves, worktrees, receipts, recovery, or card closure | `.agents/skills/orchestration/SKILL.md` |
| Compiler or language semantics | The relevant code, examples, and tests; `Docs/spec/syntax-decisions.md` for decision rationale |
| Diagnostics or snapshots | `Compiler/Prelude/Diagnostics.jet`, the matching UI snapshots, and `Docs/spec/diagnostics.md` |
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
