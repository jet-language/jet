---
name: lessons-learned
description: >-
  Peer-language lineage lessons and regrets so Jet does not repeat known
  failures.
---

# Lessons Learned

For each lesson: peer failure → Jet risk → guard (invariant, ratified decision,
open card, or ballot needed). Cover systems, managed, functional, scripting,
config/OS, and proof-oriented families. Include a do-not-ballot list where law
already covers the concern.

## Output

Write one markdown report under `docs/research/` via the Tower CLI (never hand-edit board JSON):

```
node plugins/tower/tower.mjs docs add --section research --id <skill>-YYYY-MM-DD --title "…" --file -
```

Or `docs update docs/research/<skill>-YYYY-MM-DD.md --file -` for the same day only when the owner asks to revise that run.
Never overwrite a different day's note. Do not write reports under `docs/plans/`.

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
