---
name: spec-compliance-audit
description: >-
  Audit the codebase against ratified syntax and spec. Measure shipped vs gap.
  Do not reopen syntax.
---

# Spec Compliance Audit

Compare `docs/spec/syntax-decisions.md` (and related ratified spec) to parser,
sema, tests, and examples. Status keys: `shipped`, `partial`, `gap`, `gated`,
`declined`, `stale-doc`. Cite paths. Do not invent or reopen syntax.

## Output

Write one markdown report under `docs/audits/` via the Tower CLI (never hand-edit board JSON):

```
node plugins/tower/tower.mjs docs add --section audits --id <skill>-YYYY-MM-DD --title "…" --file -
```

Or `docs update docs/audits/<skill>-YYYY-MM-DD.md --file -` for the same day only when the owner asks to revise that run.
Never overwrite a different day's note. Do not write reports under `docs/plans/`.

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
