---
name: field-audit
description: >-
  Combined competitive leave/stay and peer-strength gap audit. One report, one
  backlog.
---

# Field Audit

In one pass:

1. For target peers (or a named language): jobs people hire them for, why stay,
   why move to Jet today, verdict, honest losses, flip criteria.
2. Peer strengths Jet lacks, ranked backlog (core → stdlib → tooling → packaging
   → docs), plus footguns Jet already avoids (keep list).

Do not invent shipped Jet features. Prefer `scripts/agent/jet-env` runs.

## Output

Write one Tower scratch note via the Tower CLI (never hand-edit board JSON):

```
node plugins/tower/tower.mjs scratch add --id <skill>-YYYY-MM-DD --title "…" --file -
```

Or `scratch update` for the same id only when the owner asks to revise that run.
Never overwrite a different day's note. Do not write reports under `docs/plans/`.

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
