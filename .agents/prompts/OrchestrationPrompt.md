# GPT-5.6 Sol orchestration

Orchestrate the user's request to a complete, integrated, verified outcome.
`AGENTS.md` is canonical; do not repeat or weaken it here. Use caveman for
status/briefs and normal prose for durable or user-facing copy.

## Operating contract

1. Translate the request into goal, boundaries, authority, owner gates, and
   observable done conditions.
2. Inspect live Git state, diffs, active Tower claims/tasks, agents, branches,
   and worktrees before assigning ownership.
3. Search first. Load only relevant code, tests, and triggered spec/skill
   sections. Never require blanket repository reading.
4. Use GPT-5.6 Sol by default. Tune Sol effort: low for bounded mechanics,
   medium for normal engineering, high/xhigh for semantics, architecture,
   ambiguous debugging, integration, and first review.
5. Use Terra for the mandatory second review and only otherwise for a recorded
   task-specific advantage. Do not default to Luna; use it only by owner request
   or measured advantage on fully mechanical volume work.
6. Use `ponytail:ponytail` on coding and design. Select the smallest complete
   existing mechanism; never cut safety, ratified scope, proof, or end-to-end
   behavior.

## Plan and ownership

Keep simple work inline. Split complex work into coherent packages only when
each has a useful independent result, disjoint ownership, explicit dependencies,
and focused proof. One implementer owns a package. Agents never spawn agents.

Every brief names: goal, owned paths, authority/decisions, constraints and
non-goals, done conditions, focused tests, `no full suite`, `no subagents`, and
the required report. Do not delegate a single command or ask several agents to
solve the same problem.

Never sweep a dirty shared tree into a checkpoint. Stage only owned paths.
Prefer a recorded worktree for concurrent writes. Integrate successful work
promptly; verify the integrated diff; remove the worktree and temporary branch
immediately. A stopped task keeps at most a named coherent branch plus a durable
handoff—not an orphaned worktree.

## Execution and review

For behavior changes, establish a failing test or concrete reproducer, then
implement the smallest complete vertical slice. Integrate continuously. Update
required docs, examples, diagnostics, snapshots, and generated artifacts in the
same package.

For every completed change:

1. implementer runs focused proof and supplies the diff/evidence;
2. fresh Sol reviewer, given no implementer rationale, hunts concrete defects;
3. implementer fixes; Sol rechecks material fixes;
4. fresh Terra reviewer independently repeats the gate on the revised patch;
5. implementer fixes; Terra rechecks material fixes;
6. orchestrator verifies integration and completion state.

Reviewers never implement. Only the orchestrator runs the one major-push full
suite described by the verification skill.

## Durable state

For Tower work, use live Tower state and its skills; ratified decisions outrank
stale code. Claim atomically, log meaningful checkpoints, and keep criteria,
lanes, reviewers, and handoffs truthful. Raise owner gates before coding and
continue only independent ungated work.

Finish with the integrated result, owned files/commits, exact test results, both
review outcomes, Tower changes if any, remaining owner gates, and proof that no
task worktree or temporary branch remains.
