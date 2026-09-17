---
name: orchestration
description: >-
  Coordinate bounded Jet delivery from worker brief through integration and
  honest closure. Use for disjoint implementation waves, card delivery, or
  milestone closeout; not for audits, research, or an unrequested phase override.
---

# Orchestration

Read `AGENTS.md` first. This root is a router; load only the reference that
matches the work:

| Need | Read |
|---|---|
| Brief a worker, dispatch a slice, or harvest a receipt | [`references/dispatch.md`](references/dispatch.md) |
| Card proof and linked-card milestone cadence | [`references/closeout.md`](references/closeout.md) |
| Review a new, draft, or changed full ballot | [`references/ballot-review.md`](references/ballot-review.md) |
| Recover after a crash, timeout, or tangled integration | [`references/recovery.md`](references/recovery.md) |
| Bound time, disk, memory, and process liveness | [`references/resources.md`](references/resources.md) |

## Authority and outcome

- Tower is the only work ledger. `AGENTS.md` is policy; these references
  describe dispatch and delivery mechanics.
- One named implementer owns each coherent patch. Concurrent slices have
  disjoint writable paths and one named close owner. Integrate the complete
  greenfield cutover: callers, tests, tools, docs, generated uses, environment
  variables, and CLI spellings that consume the changed contract.
- A worker returns a bounded patch, source evidence, a blocker, or a receipt. A
  worker never writes Tower, closes a card, claims unrun proof, or spawns a
  worker. The orchestrator owns integration, criteria evidence, Tower state,
  and closeout.
- `CHECK OK` and `DOCS ONLY` are receipts, not runtime or milestone proof.
  The exact focused criterion proof, linked-card token, composed sweep, and
  fresh-context review are defined only in `references/closeout.md`.

## Conditional owner order

Implementation-before-validation is not the default. Activate it only when the
owner explicitly requests that phase order; while active, continue through all
requested implementation cards before validation. Never call an unrun runtime,
tier, diagnostic, snapshot, or golden criterion green. Do not activate this
mode merely because a worker or prompt mentions it.

Successful completion requires every requested path integrated and evidenced,
with Tower reflecting the real state. If no agent-actionable work remains,
return an honest blocked result only when every unfinished path has a concrete
owner gate or external blocker recorded in Tower. Never close an unmet
criterion. In either outcome, account for every worker, proof, worktree,
branch, and handoff; leave nothing unowned or silently in flight. Follow the
terminal conditions in `references/closeout.md`.
