# Dispatch and worker receipts

Use this reference when a bounded slice needs a brief or when a worker result is
harvested. `AGENTS.md` controls policy and roles. Give one worker one mechanism
or one small criterion set, with one named implementer and one close owner.

## Brief fields by context

Every brief names the card, the intended integration target, the actual and
expected outcome, writable paths, non-goals, and the owner of closure. Add only
the fields that the worker and artifact need:

| Worker or artifact | Add to the brief |
|---|---|
| Source implementation | The last dev commit, the worker's own export under `$JET_SCRATCH_ROOT/lanes/<Worker>/`, the paths it owns, the submission contract in [`intake.md`](intake.md), applicable authority and invariants, complete consumer/caller cutover, and the exact criterion proof command with its expected observable result for the submission's `tests`. |
| `.jet` source writer | The source file's scoped checks: `Tools/agent/JetEnv.jet jet check path/to/file.jet` and `Tools/agent/JetEnv.jet jet fmt --check path/to/file.jet`. These are source checks, not runtime, tier, snapshot, golden, or generated-artifact proof. |
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
cancellation, and liveness. Select the most specific role available; prompts do
not choose or name a model. If the host cannot run the required role, record
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
