---
name: mission-audit
description: >-
  Audit Jet language, surface, coverage, and experience against philosophy and
  mission.
---

# Mission Audit

Score alignment with `docs/spec/philosophy.md` and `AGENTS.md` invariants:
beginner defaults, expert control, one mechanism, hidden rustc, diagnostics as
product, batteries, systems path, one package graph, lean tools. Mark
`aligned` / `drift` / `unknown` with evidence and the smallest corrective action.

## Output

Write one markdown report under `docs/audits/` via the Tower CLI (never hand-edit board JSON):

```
node plugins/tower/tower.mjs docs add --section audits --id <skill>-YYYY-MM-DD --title "…" --file -
```

Or `docs update docs/audits/<skill>-YYYY-MM-DD.md --file -` for the same day only when the owner asks to revise that run.
Never overwrite a different day's note. Do not write reports under `docs/plans/`.

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
