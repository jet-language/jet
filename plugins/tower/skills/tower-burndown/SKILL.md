---
name: tower-burndown
description: >-
  Execute and close an explicitly requested Tower scope: sidequests, one epoch,
  or both. Use only when the user asks to implement or burn down that scope;
  board-only reads and closes stay with tower. Tower owns scope and board state;
  orchestration owns dispatch, integration, proof, and closeout cadence.
---

# Tower — execute a requested scope

## Contract

- **Requested outcome:** The requested scope is integrated, has exact evidence,
  and ends in honest `done` state or an honestly recorded blocker.
- **Supplied inputs:** Explicit execution scope, fresh board state,
  dependencies, active claims, owner guidance, project rules, and the exact
  proof route.
- **Allowed child result:** OMP workers return `CHECK OK` or `DOCS ONLY`.
  `tower`, `tower-rank`, `tower-prep`, and `verify` return only board, order,
  plan, or proof results. No child expands scope or creates competing Tower
  state.
- **Completion owner:** `tower-burndown` owns scope parsing and board
  operations. Owner guidance and orchestration own dispatch, integration,
  focused proof, and closure law.
- **Return point:** Every worker or sibling result returns to its claimed card
  before another claim or brief.
- **Stopping condition:** Stop only when the requested scope is integrated and
  closed, or every remaining card is accounted for with a concrete blocker or
  owner gate. No worker, proof, claim, or handoff may remain unaccounted.

A plain board request such as “show cards” or “close this already-proven card”
does not authorize implementation. Require an explicit execute, implement,
or burn-down request plus a scope. This adapter sets no model, lane count,
worktree policy, concurrency limit, or proof cadence.

## Scope

```text
/tower-burndown
/tower-burndown sidequests
/tower-burndown epoch 3
/tower-burndown epoch 3+sidequests
```

- No argument: sidequests first, then the current epoch.
- `sidequests`: sidequest track only.
- `epoch N`: named epoch only.
- `epoch N+sidequests`: both in the stated order.
- Explicit owner scope overrides these defaults.

Use [tower-rank](../tower-rank/SKILL.md) for queue order and
[tower-prep](../tower-prep/SKILL.md) for plans or ballots.

## Tower responsibilities

1. Query fresh status, open gates, active claims, dependencies, and the exact
   requested scope.
2. Follow `workOrder` unless owner direction or a real dependency requires
   another order. Inspect the computed actor before claiming; skip every
   `who: owner` card, including owner-acceptance `verify` work, as well as
   frozen, done, blocked, and foreign-claimed cards. A next-card pick alone
   does not authorize an agent claim.
3. Claim one bounded card slice through Tower. Preserve active ownership and
   release or hand off unfinished claims with exact state and continuation.
4. Enforce the closure barrier: when an actively claimed card has every
   criterion met and no open gate, close it and confirm `done` before another
   claim or brief. Tower reports this as `E_CLOSE_READY`.
5. After orchestration returns integrated focused-proof evidence, mark the
   matching criteria `met`, record the real evidence and attribution, advance
   the phase honestly, close immediately, and read `done` back.
6. If an unrelated defect appears, mint one deduplicated card and continue the
   current card unless the defect blocks its criterion or violates I1/I2 on
   that exact path.
7. When **all cards linked to the milestone are `done`**, freeze the source and
   open the closeout token. Never open a token after only one linked card.
8. Reopen only the owning card for a material closeout finding; re-prove its
   affected criteria, close it, freeze the new source, and open a new token.
9. Never hand-edit `plugins/tower/.tower/`. Release or hand off every
   unfinished claim with its exact phase, blocker, and continuation path.

Use the canonical orchestration references for the mechanics this adapter does
not own:

- [dispatch and worker receipts](../../../../.agents/skills/orchestration/references/dispatch.md)
- [milestone closeout cadence](../../../../.agents/skills/orchestration/references/closeout.md)
- [proof contract](../../../../.agents/skills/verify/SKILL.md)
- [Tower policy](../../AGENTS.md)

## Boundaries

This skill owns:

- Tower scope parsing;
- queue and dependency reads;
- claims, criteria, phases, logs, and handoffs;
- immediate card closure after integrated evidence; and
- links to `tower`, `tower-rank`, `tower-prep`, and `verify`.

It does not own:

- agent roles, worker briefs, or model selection;
- concurrency, worktrees, or build-cache mechanics;
- integration strategy or proof cadence;
- milestone token implementation; or
- user-facing prose style.

Those rules come from owner guidance, `AGENTS.md`, orchestration, and
`verify`. If they conflict, stop dispatch, follow higher authority, and repair
stale routing before continuing.
