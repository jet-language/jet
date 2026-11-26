# Dispatch and worker receipts

Use this reference when a bounded slice needs a brief or when a worker result is
harvested. `AGENTS.md` controls policy and roles. Give one worker one mechanism
or one small criterion set, with one named implementer and one close owner.

## Briefs come from the board

A brief quotes each criterion's exact text, read from the card at the moment
of dispatch (`node Tools/Tower/tower.mjs card criteria '#N' --list`). Never
brief from memory or by criterion number alone: numbers shift when rows move,
and a row may already be met. Give each worker a queue of five to ten open
criteria, cheapest first, each with the files it touches and the exact proof
command. A worker with a queue never waits for its next task.

## The worker's standing rules

Every brief, and every message that revives a worker, carries these rules:

1. The unit of work is one open criterion, or one bundle of criteria in the
   same files. The worker reads its text from the card and runs its proof
   before writing code. A proof that already passes is reported with its
   command and output, and the worker takes the next criterion.
2. A criterion with no passing proof after 45 minutes is written to
   `BLOCKED.md` in the worker's lane (what blocks it, where, what was tried,
   what to try next). The worker tells the orchestrator in one line and moves
   on. The orchestrator routes every blocker to one expert pass.
3. The worker never waits and never parks. Inside approved scope it makes the
   implementation choices the approved contract covers and continues. A choice
   the contract does not cover is a decision: the worker sends it to the
   orchestrator as ballot facts and goes on with other criteria. No decision is
   ever written on a card. It never ends its turn to wait on its own job.
4. Proof before submitting is narrow: the cases the criterion names and the
   checks of the touched files, with the batch compiler the orchestrator last
   announced. The batch runs the guards and suites once for everyone.
5. Finished work is submitted at once, in as few submissions as its files
   allow, and never held for a later train.
6. A submission's `msg` ends with one line per criterion it completes:
   `#CARD cN | exact proof command | expected result`. The orchestrator or the
   Closer runs that line and meets the criterion from it.
7. Messages to the orchestrator are one line each: a submission staged, a
   blocker, an already-met proof, or ballot facts.
8. Every message to a worker, and every message that revives one, carries its
   next queued items, so its run never ends with nothing to do. A worker
   whose run ends empty is an orchestrator failure (AGENTS.md, Delegation).

## Already-met claims

When a worker reports that a criterion already holds, the orchestrator or the
Closer reruns the named proof on the admitted compiler from the dev worktree,
reads the criterion's full text for any clause a test cannot prove (a search
of the repository, spec text, a named file), checks those too, and only then
meets it with that evidence. Rerun the rows in parallel; dozens of single-case
runs take under a minute.

## Fresh agents and measured output

A long-lived agent slows as its context grows. When a lead's context passes
about 150K tokens, have it write a handoff note in its lane (its queue, what
is staged, what is half-written and where) and replace it with a fresh agent
on the same queue. Measure each lane by criteria met per day, read from the
board, and re-brief or replace the slowest. Activity is not output.

## Brief fields by context

Every brief names the card, the intended integration target, the actual and
expected outcome, writable paths, non-goals, and the owner of closure. Add only
the fields that the worker and artifact need:

| Worker or artifact | Add to the brief |
|---|---|
| Source implementation | The last dev commit, the worker's own export under `$JET_SCRATCH_ROOT/lanes/<Worker>/`, the paths it owns, the submission contract in [`intake.md`](intake.md), applicable authority and invariants, complete consumer/caller cutover, and the exact criterion proof command with its expected observable result for the submission's `tests`. |
| `.jet` source writer | The source file's scoped checks: `Tools/Agent/JetEnv.jet jet check path/to/file.jet` and `Tools/Agent/JetEnv.jet jet fmt --check path/to/file.jet`. These are source checks, not runtime, tier, snapshot, golden, or generated-artifact proof. |
| Case or CLI-surface writer | Run each case the submission adds or changes once through the runner on the seed (`jet test Compiler --profile cases --filter NAME`): a refused or failing case is a mistake one run catches. A change to a command, flag or help text regenerates the CLI snapshots it reaches (`Tests/CLI/`, the settings census) with `JET_UPDATE_SNAPSHOTS=1`, and the worker reviews each diff. |
| Prose or static-data writer | The user-visible audience, source-of-truth paths, link/path consumers, and the expected `DOCS ONLY` receipt. Do not require compiler commands for prose-only work. |
| Review worker | The exact bounded artifact, acceptance criteria, authority, invariants, and evidence to inspect. A review worker reports findings and does not repair the artifact. |
| Recovery or long wave | The last integrated state, explicit timeout, resource/liveness boundary, salvage point, and rebrief condition. Load [`resources.md`](resources.md) or [`recovery.md`](recovery.md) rather than copying their controls. |

All source workers are told that workers do not write Tower, close cards, or
spawn workers. Under the standing work order in [`../SKILL.md`](../SKILL.md),
they implement to the spec, run only the touched files' source checks, commit,
report, and move on. They do not run suites, A/B comparisons, chains, release
builds, censuses, blessing, or full formatting: the orchestrator verifies in
batches and returns every failure at once. A type-check is never evidence for
runtime, I9 tier, diagnostic, snapshot, golden, or generated-artifact claims.

## Dispatch and style reads

Dispatch through the host's task and steering tools, which handle harvest,
cancellation, and liveness. Select the most specific role available; prompts do not choose or name a model.
If the host cannot run the required role, record
the exact harness failure in the handoff before a bounded fallback. A fallback
still uses the dev worktree and its owned paths, explicit timeout, complete
brief, no Tower writes, and no other worker's paths.

Workers do not write Tower, close cards, or start `tower serve`. The completion
owner uses the non-serve CLI against the main board; no agent hand-edits Tower
data.

Read style guidance only when the artifact needs it. Use the applicable writing
skill and local source conventions for durable user-visible prose or ballots;
do not load prose-style skills for a code or static-data brief. Use coding
simplification guidance for an implementation/design task when policy calls for
it. Never make `ponytail`, `caveman`, `simple`, or another style procedure a
blanket worker requirement.

## Receipt shapes

- **Code:** a submission folder in the orchestrator's inbox
  ([`intake.md`](intake.md)) plus a one-line notice naming it. The folder lists
  the changed paths in its patch, the checks actually run in `msg`, blockers,
  and the criteria claimed in `cards`. A submission that fails a batch is
  returned with every failure; the orchestrator does not repair the worker's
  implementation.
- **`.jet` code:** `msg` names the exact `jet check …` and single-file test
  results. These checks do not authorize runtime claims.
- **Prose or static data:** the first line is exactly `DOCS ONLY`; then list
  changed paths, source references read, blockers, and the named implementer.
- **Review:** identify the artifact and return findings, unknowns, and the
  evidence examined; do not imply that a finding was fixed.

A receipt describes work actually performed. It never claims a green runtime,
tier, snapshot, golden, or generated artifact whose proof was not run.
