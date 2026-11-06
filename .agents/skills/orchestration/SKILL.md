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
| Diagnose and batch failures, integrate, or gate `master` | [`references/closeout.md`](references/closeout.md) |
| Review a new, draft, or changed full ballot | [`references/ballot-review.md`](references/ballot-review.md) |
| Recover after a crash, timeout, or tangled integration | [`references/recovery.md`](references/recovery.md) |
| Shared artifacts and bounded build output | [`references/resources.md`](references/resources.md) |

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

## Standing work order: implement, then batch-verify

This is the owner's durable order and the default for every wave:

- **Workers implement.** A worker takes the card's unmet criteria directly,
  writes the code to the spec, runs only the cheap source checks for the files
  it touched (`jet check`, `jet fmt --check`, and a generator's `--check` when
  it changed that generator's source), commits on `dev`, reports the commit and
  the criteria it addresses, and moves on to the next criterion. Workers never
  run suites, A/B comparisons, chains, release builds, or censuses.
- **The orchestrator verifies, in batches.** It collects the reported commits,
  builds once, and runs one collect-all verification covering every reported
  criterion's proof and the affected suites. It groups every failure by root
  and sends each owner its complete fix list in one message. Each criterion
  proven there is recorded as evidence, and its card is closed.
- Never call an unrun runtime, tier, diagnostic, snapshot, or golden criterion
  green.

Successful completion requires every requested path integrated and evidenced,
with Tower reflecting the real state. If no agent-actionable work remains,
return an honest blocked result only when every unfinished path has a concrete
owner gate or external blocker recorded in Tower. Never close an unmet
criterion. In either outcome, account for every worker, proof, worktree,
branch, and handoff; leave nothing unowned or silently in flight. Follow the
terminal conditions in `references/closeout.md`.
