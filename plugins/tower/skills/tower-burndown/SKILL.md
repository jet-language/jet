---
name: tower-burndown
description: >-
  Execute Tower card closeout for sidequests, one epoch, or both. Owns Tower
  scope and board operations only; shared dispatch, integration, proof, and
  closure mechanics come from project authority.
---

# Tower burndown adapter

## Contract

- **Requested outcome:** Close the requested Tower scope with integrated evidence and honest `done` state.
- **Supplied inputs:** The requested scope, fresh board state, dependencies, active claims, owner guidance, project rules, and exact proof route.
- **Allowed child result:** OMP workers return `CHECK OK` or `DOCS ONLY`; `tower`, `tower-rank`, `tower-prep`, and `verify` return only board, order, plan, or proof results. No child expands scope or writes competing Tower state.
- **Completion owner:** `tower-burndown` owns Tower scope and board operations; owner guidance owns dispatch, integration, proof, and closure law.
- **Return point:** Every worker or sibling result returns to the claimed card before the next claim.
- **Stopping condition:** Stop after each completed criterion is integrated, evidenced, and closed, or after every remaining blocker is recorded. Do not refill past a closure barrier.

Thin Tower adapter. Read `AGENTS.md` and `.agents/skills/orchestration/SKILL.md` before dispatch. Use `tower` for board commands and `verify` for closeout proof.

Plain “burn down,” “close cards,” or `/tower-burndown` authorizes implementation workers. Concurrency remains adaptive under owner guidance and orchestration mechanics; this adapter sets no model, fixed lane count, worktree policy, or proof cadence.

## Scope

```text
/tower-burndown
/tower-burndown sidequests
/tower-burndown epoch 3
/tower-burndown epoch 3+sidequests
```

- No argument: sidequests first, then current epoch.
- `sidequests`: sidequest track only.
- `epoch N`: named epoch only.
- `epoch N+sidequests`: both, in stated order.
- Explicit owner scope overrides defaults.

Ranking and preparation remain separate: use `tower-rank` for order and `tower-prep` for plans or ballots.

## Tower loop

1. Query fresh Tower status, open gates, active claims, dependencies, and requested scope.
2. **Closure barrier:** when an actively claimed card has all criteria met and no open gate, close it and confirm `done` before any new claim or brief. Tower enforces this with `E_CLOSE_READY`.
3. Follow `workOrder` unless owner instruction or a real dependency requires another order. Skip owner-gated, frozen, done, blocked, and foreign-claimed cards.
4. Claim one bounded slice through Tower. Dispatch only via OMP `task` and `hub`. A code worker must return `CHECK OK`; reject any other code receipt. `DOCS ONLY` is valid only when every changed file is prose or static data.
5. Integrate, run the exact criterion proof once, record its real evidence, close the card immediately when complete, and confirm `done`. Only then refill.
6. A newly discovered unrelated defect gets a separate deduplicated card. Continue the current card unless that defect directly blocks its criterion or violates I1/I2 on the exact path.
7. After every linked card closes, commit the frozen source and run `scripts/agent/closeout-gate.mjs open MILESTONE --by AGENT`. Only then may broad proof, an unfiltered census, milestone review, or `verify-full.sh` run.
8. Reopen only the owning card for a material closeout finding. Re-prove its affected criteria, close it, freeze a new commit, and open a new token.
9. Never hand-edit `plugins/tower/.tower/`. Release or hand off every unfinished claim with exact state, blocker, and continuation path.

## Adapter boundaries

This skill owns:

- Tower scope parsing;
- queue and dependency reads;
- claims, criteria, logs, phases, and handoffs;
- links to `tower-rank`, `tower-prep`, `tower`, and `verify`.

This skill does not own:

- agent roles or worker briefs;
- model or reasoning selection;
- concurrency limits;
- worktree or build-cache mechanics;
- integration strategy;
- proof cadence or closure meaning;
- user-facing prose style.

Those rules come from owner guidance, `AGENTS.md`, orchestration mechanics, and `verify`. Conflict means stop dispatch, follow higher authority, and repair stale routing before continuing.
