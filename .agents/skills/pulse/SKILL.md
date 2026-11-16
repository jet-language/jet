---
name: pulse
description: "On-request ELI5 status: approximate completion per task and overall, what changed, and what remains."
argument-hint: "[scope] [bars|table|bullets]"
---

# Pulse

Use `/skill:pulse` or ask for status. Scope is the current run's full goal set unless the user names another.

- Read context, recent proof, Tower criteria/closures, and available worker updates. Flag stale/conflicting evidence. Be quick: no new tests, audits, worker waits, or file/board writes.
- Estimate completed acceptance work, including integration and checks. Round to tens with `~`; use ranges or `unknown` when uncertain. `100%` needs delivery, proof, and Tower closure where applicable, not merely finished code.
- Include every done, active, blocked, and unstarted goal task. Weight overall by task size, not card count or time spent. State the basis; flag added scope. Never double-count parents and children.
- ELI5: what works, what is happening, what remains. Explain stalls and the next closure. Name blockers and who must act. ETA only if asked and supported.

Default to progress bars, one block per goal task:

```text
<scope>: [<10 cells>] ~X% overall (work-weighted estimate); N/M tasks closed.
<task>: [<10 cells>] ~X%
  <Full ELI5 description: what works, what remains, and any blocker.>
Since last update: <completed outcome; added scope or no change>.
Next: <next finishable step>. Blocker / need from you: <specific or none>.
```

`table` and `bullets` are opt-in alternatives with the same facts. Unknown completion has no bar.
Keep descriptions concise but complete, with full sentences wrapping beneath each bar.
Never truncate or squeeze descriptions to fit one line or screen. No report file unless asked.

Only report on request. Reply, then resume authorized work; keep workers running.
Explicit stop, wrap-up, or handoff requests override resumption.
