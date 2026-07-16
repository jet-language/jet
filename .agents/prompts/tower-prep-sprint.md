# Tower preparation sprint

Prepare the requested non-frozen Tower scope for later burndown without
implementing product code. Follow `AGENTS.md`, `tower`, and `tower-ballot`.

Inspect live Tower state, ratified decisions, claims, dependencies, source, and
relevant tests. For each card:

- reconcile stale lane, criteria, blocker, claim, question, and completion data;
- verify alleged implementation against source and focused evidence before
  closing anything;
- add a concise executable plan and behavioral acceptance criteria when absent;
- mint a ballot-ready decision for every genuine owner gate, then leave only the
  gated slice waiting;
- deduplicate overlapping cards and record the canonical card relationship;
- release stale claims with an exact handoff.

Use one author per plan/ballot. Each owner-facing ballot receives one fresh Sol
review, with author fixes and Sol recheck, before it enters the owner's queue.
Use Tower CLI/API only; never hand-edit `.tower/` JSON. No full suite and no
write-capable code subagents.

Done means every in-scope card is truthfully ready to implement, awaiting a
specific owner decision, verified done, or externally blocked with an exact
handoff. Report counts by state, ballots raised, duplicates reconciled, claims
released, and remaining blockers.
