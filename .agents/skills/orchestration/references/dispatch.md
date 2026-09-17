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
| Source implementation | In-repository worktree and branch, last integrated commit, applicable authority and invariants, complete consumer/caller cutover, and the exact criterion proof command with its expected observable result. |
| `.jet` source writer | The source file's scoped checks: `scripts/agent/jet-env jet check path/to/file.jet` and `scripts/agent/jet-env jet fmt --check path/to/file.jet`. These are source checks, not runtime, tier, snapshot, golden, or generated-artifact proof. |
| Prose or static-data writer | The user-visible audience, source-of-truth paths, link/path consumers, and the expected `DOCS ONLY` receipt. Do not require compiler commands for prose-only work. |
| Review worker | The exact bounded artifact, acceptance criteria, authority, invariants, and evidence to inspect. A review worker reports findings and does not repair the artifact. |
| Recovery or long wave | The last integrated state, explicit timeout, resource/liveness boundary, salvage point, and rebrief condition. Load [`resources.md`](resources.md) or [`recovery.md`](recovery.md) rather than copying their controls. |

All source workers are told that workers do not write Tower, close cards, or
spawn workers. They do not run broad tests, release builds, generators,
blessing, full formatting, or milestone proof. A type-check is never evidence
for runtime, I9 tier, diagnostic, snapshot, golden, or generated-artifact
claims. If the owner explicitly orders implementation-before-validation, state
that it overrides the ordinary validation sequence; do not activate that order
without the owner's direct request.

## Dispatch and style reads

Use OMP `task` first and `hub` for steering, harvest, cancellation, and
liveness. Select the most specific role from `AGENTS.md`; prompts do not choose
or name a model. If OMP cannot run the required role, record the exact harness
failure in `JET_OMP_FALLBACK_REASON` before a bounded fallback. A fallback still
uses the assigned in-repository worktree, explicit timeout, complete brief, no
Tower writes, and no sibling worktree access.

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

- **Code:** the first line is the exact final `CHECK OK` line emitted by
  `scripts/agent/lane-check.sh`; then list changed paths, checks actually run,
  blockers, and the named implementer. A missing or failed receipt is rejected
  and the same card is rebriefed with the exact error; the orchestrator does not
  repair the worker's implementation.
- **`.jet` code:** include the `CHECK OK` line when the code lane ran, plus the
  exact `scripts/agent/jet-env jet check …` and `jet fmt --check …` results.
  These checks do not authorize runtime claims.
- **Prose or static data:** the first line is exactly `DOCS ONLY`; then list
  changed paths, source references read, blockers, and the named implementer.
- **Review:** identify the artifact and return findings, unknowns, and the
  evidence examined; do not imply that a finding was fixed.

A receipt describes work actually performed. It never claims a green runtime,
tier, snapshot, golden, or generated artifact whose proof was not run.
