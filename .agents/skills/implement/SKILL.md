---
name: implement
description: "Implement a piece of work based on a spec or set of tickets."
disable-model-invocation: true
---

Implement the work described by the user in the spec or tickets.

Read `docs/agents/owner-guidance.md` and the current task brief before changing
anything. Keep this skill focused on implementation intake: one bounded,
complete source slice with its required examples and tests when the brief calls
for them.

Do not select models, dispatch nested agents, write Tower state, or choose the
proof cadence here. The owner guide and `docs/agents/orchestration.md` own those
shared rules. Workers run the authorized lane check and return source evidence;
the orchestrator owns integration, broader proof, and closure.

Do not automatically invoke another skill or commit. Return the changed paths,
the evidence you actually exercised, the lane-check receipt, and any blocker.
