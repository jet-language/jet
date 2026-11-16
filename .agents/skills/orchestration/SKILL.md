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
| Take worker submissions, run a batch, or land on `dev` | [`references/intake.md`](references/intake.md) |
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
- Submissions and `DOCS ONLY` are receipts, not runtime or milestone proof.
  The exact focused criterion proof, linked-card token, composed sweep, and
  fresh-context review are defined only in `references/closeout.md`.
- The orchestrator may name one Lander, which runs the rolling train of
  `references/intake.md` and commits on `dev` in its place, and one Closer,
  which works the cards with the fewest open criteria and hands back proof.
  Tower writes, owner gates and dispatch stay with the orchestrator.

## Standing work order: implement, then batch-verify

This is the owner's durable order and the default for every wave. It is not
suspended near a deadline, after a crash, or for a release:

- **Code first.** Where a spec or criterion exists, the worker writes the code
  before anything else. It takes the card's unmet criteria directly, writes
  the code to the spec, runs only the cheap source checks for the files it
  touched (`jet check` and a single-file test), submits the patch to the
  orchestrator's inbox ([`references/intake.md`](references/intake.md)) with
  the criteria it addresses, and moves on to the next criterion at once. It
  never stops to prove rows, profile, or wait for the orchestrator. Workers
  never commit, build the compiler, or run suites, A/B comparisons, chains,
  release builds, or censuses.
- **Failures come back; the worker fixes them and moves on.** A batch failure
  goes to its owner as one fix list; the owner submits the fixes and returns to
  its next criterion without waiting for a re-run.
- **The orchestrator never gates a landing on proof.** It does not hunt
  evidence criterion by criterion mid-wave or hold workers for it; it keeps
  every worker writing code and does its proof only in the batches below.
- **The orchestrator verifies, in batches.** It collects the reported commits,
  builds once, and runs one collect-all verification covering every reported
  criterion's proof and the affected suites. It groups every failure by root
  and sends each owner its complete fix list in one message. Each criterion
  proven there is recorded as evidence, and its card is closed.
- Never call an unrun runtime, tier, diagnostic, snapshot, or golden criterion
  green.

## Standing work order: close a goal in one sweep

This is the owner's required process for closing a goal or milestone. Follow
it every time; never drift into one-blocker-at-a-time work:

1. **Collect every closure blocker at once.** List every open criterion on
   every open card of the goal, and run each one's census in collect-all mode
   in the same pass: `JET_KEEP_GOING=1` checks of every package it names (and
   of every sibling the same rule covers), an inventory of every file it
   names, and the suite or fresh-clone run it names. The complete list exists
   before the first fix.
2. **Fix the whole list in one sweep.** Group the rows by root. The
   orchestrator writes the code to spec for the roots it owns. Each root in a
   subagent's area goes to that subagent as one focused prompt naming the
   exact rows, files and criterion text.
3. **Land at once and move on.** A ready submission lands on `dev` as soon
   as its checks pass; a compiler, runtime or Core change lands on its
   touched-file checks. Nothing ready waits in a pending list, fix pass or
   stack for a later build. Every landed commit goes on the batch queue with
   the proof its criterion names, and the orchestrator moves to the next
   goal's work without waiting.
4. **Close from the batch.** When the batch reports, record each proven
   criterion as evidence and close its card. Every failure goes back to its
   owner as one focused prompt, and the orchestrator moves on again.

Only code work and closure count toward the sweep. Merging, carving or
reshuffling cards is not progress.

Successful completion requires every requested path integrated and evidenced,
with Tower reflecting the real state. If no agent-actionable work remains,
return an honest blocked result only when every unfinished path has a concrete
owner gate or external blocker recorded in Tower. Never close an unmet
criterion. In either outcome, account for every worker, proof, worktree,
branch, and handoff; leave nothing unowned or silently in flight. Follow the
terminal conditions in `references/closeout.md`.
