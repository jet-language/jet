---
name: surface-audit
description: >-
  Audit Jet language shape, uniformity, and consistency. Find outliers and gaps
  in syntax and structure; say what to do next.
---

# Surface Audit

Find outliers and gaps in language shape, uniformity, and consistency. Compose
output quality from `docs/spec/philosophy.md` and
`docs/spec/syntax-decisions.md`. End with concrete next actions (ballot
titles or card ids only — do not create cards unless asked).

Search live specs, examples, stdlib, and CLI surfaces. Prefer
`scripts/agent/jet-env` and `rg` over memory.

## The standing lens

Apply `.agents/skills/_shared/standing-lens.md` in full: the four questions, the
five agent-optimality quantities, the micro sweep, probe the running binary, and
the honesty rules. The owner never has to ask for any of it.

## Method: the micro sweep is this skill

"Shape, uniformity, and consistency" is measured category by category, not by
impression. Walk every category in the shared lens's micro sweep — syntax,
ergonomics, surfaces, APIs and types and methods, defaults, naming, error text,
UX and DX, tooling and CLI shape, ceremony versus control — and report each one
even when it is clean. A category with no finding is a result worth printing; a
category you skipped is a hole in the audit.

For each outlier, say which of the four it is:

- **Inconsistent** — the same idea spelled two ways. Name both and pick one.
- **Absent** — a shape the language implies but does not offer.
- **Ceremonial** — required text that buys the reader nothing.
- **Asymmetric** — the beginner road and the expert road disagree about the
  same concept.

Judge each finding against verdict actionability and repair determinism as well
as human readability. A surface with one obvious spelling is cheaper for an
agent to drive, which is the machine-facing half of I8.

## Output

Write one markdown report under `docs/audits/` via the Tower CLI (never hand-edit board JSON):

```
node plugins/tower/tower.mjs docs add --section audits --id <skill>-YYYY-MM-DD --title "…" --file -
```

Or `docs update docs/audits/<skill>-YYYY-MM-DD.md --file -` for the same day only when the owner asks to revise that run.
Never overwrite a different day's note. Do not write reports under `docs/plans/`.

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
