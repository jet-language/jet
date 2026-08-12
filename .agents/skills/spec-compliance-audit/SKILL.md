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

## The standing lens (partial)

Apply the **probe the running binary** and **honesty rules** sections of
`.agents/skills/_shared/standing-lens.md`. Skip the four questions, the five
quantities, and the micro sweep: this skill measures shipped against ratified,
and a competitive or design frame would distort that measurement.

Probing is not optional here — it is the whole method. A `shipped` status
earned from a spec paragraph, a code path that looks right, or a passing name in
a test list is not earned. Run the surface and read the real output before
writing `shipped`.

Two failures this skill exists to catch, both of which read as `shipped` from a
distance:

- A registered surface that cannot fire — a diagnostic code with no
  implementation, a documented field emitted as a constant, a flag parsed and
  ignored.
- A surface that fires for the demo case and nothing else. Record it as
  `partial` with the covered case named, never as `shipped`.

## Output

Write one markdown report under `docs/audits/` via the Tower CLI (never hand-edit board JSON):

```
node plugins/tower/tower.mjs docs add --section audits --id <skill>-YYYY-MM-DD --title "…" --file -
```

Or `docs update docs/audits/<skill>-YYYY-MM-DD.md --file -` for the same day only when the owner asks to revise that run.
Never overwrite a different day's note. Do not write reports under `docs/plans/`.

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
