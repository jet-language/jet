---
name: surface-research
description: >-
  Research Jet surface gaps by mining other languages for ideas.
---

# Surface Research

Mine peer languages for ideas that address Jet surface gaps, issues, and
weaknesses. Prefer primary sources. Record idea, Jet use, and failure to avoid.
This is research, not ratification.

## Output

Write one markdown report under `docs/research/` via the Tower CLI (never hand-edit board JSON):

```
node plugins/tower/tower.mjs docs add --section research --id <skill>-YYYY-MM-DD --title "…" --file -
```

Or `docs update docs/research/<skill>-YYYY-MM-DD.md --file -` for the same day only when the owner asks to revise that run.
Never overwrite a different day's note. Do not write reports under `docs/plans/`.

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
