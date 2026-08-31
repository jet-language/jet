---
name: tower-burndown
description: >-
  Execute Tower card closeout for sidequests, one epoch, or both. Owns Tower
  scope and board operations only; shared dispatch, integration, proof, and
  closure mechanics come from project authority.
---

# Tower burndown adapter

Thin Tower adapter. Read `docs/agents/owner-guidance.md`, nearest `AGENTS.md`, and `docs/agents/orchestration.md` before dispatch. Use `tower` for board commands and `verify` for closeout proof.

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

1. Query fresh Tower status, open questions, claims, dependencies, and requested scope.
2. Follow `workOrder` unless owner instruction or a real dependency requires another order.
3. Skip owner-gated, frozen, done, blocked, and foreign-claimed cards.
4. Group only cards sharing one mechanism, writable path set, or proof boundary. Preserve each card's criteria and evidence.
5. Claim through Tower, then dispatch **only** via the OMP `task` tool and `hub` (`docs/agents/orchestration.md`). Never `codex exec`, `setsid`, or `lane-dispatch.mjs launch` unless that OMP spawn already failed and the failure is recorded.
6. Record criterion evidence only from the integrated tree. Use `verify` for applicable runtime, tier, snapshot, golden, diagnostic, and milestone proof.
7. Close cards under project closure law. Never hand-edit `plugins/tower/.tower/`.
8. At milestone boundary, run the required composed sweep and fresh-context review; reopen owning cards for material findings.
9. Release or hand off every unfinished claim with exact state, blocker, and continuation path.

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
