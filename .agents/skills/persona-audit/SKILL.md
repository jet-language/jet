---
name: persona-audit
description: >-
  Persona-based status checks for Jet: practical use, push and pull factors,
  feel for development state.
---

# Persona Audit

Generate fresh personas (beginner through expert, distinct domains). For each,
define a concrete project and its core loop, run representative examples with
`scripts/agent/jet-env`, and report push/pull factors plus a clear verdict
(`ship-ready` / `usable-with-friction` / `blocked`).

## Output

Write one markdown report under `docs/audits/` via the Tower CLI (never hand-edit board JSON):

```
node plugins/tower/tower.mjs docs add --section audits --id <skill>-YYYY-MM-DD --title "…" --file -
```

Or `docs update docs/audits/<skill>-YYYY-MM-DD.md --file -` for the same day only when the owner asks to revise that run.
Never overwrite a different day's note. Do not write reports under `docs/plans/`.

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
