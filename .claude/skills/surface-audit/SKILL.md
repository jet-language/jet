---
name: surface-audit
description: >-
  Audit Jet language shape, uniformity, and consistency. Find outliers and gaps
  in syntax and structure; say what to do next.
---

# Surface Audit

Find outliers and gaps in language shape, uniformity, and consistency. Compose
output quality from `docs/proposals/language-shape-constitution.md` and
`docs/proposals/uniformity-paradigm.md`. End with concrete next actions (ballot
titles or card ids only — do not create cards unless asked).

Search live specs, examples, stdlib, and CLI surfaces. Prefer
`scripts/agent/jet-env` and `rg` over memory.

## Output

Write one Tower scratch note via the Tower CLI (never hand-edit board JSON):

```
node plugins/tower/tower.mjs scratch add --id <skill>-YYYY-MM-DD --title "…" --file -
```

Or `scratch update` for the same id only when the owner asks to revise that run.
Never overwrite a different day's note. Do not write reports under `docs/plans/`.

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
