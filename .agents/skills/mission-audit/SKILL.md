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

## The standing lens

Apply `.agents/skills/_shared/standing-lens.md` in full: the four questions, the
five agent-optimality quantities, the micro sweep, probe the running binary, and
the honesty rules. The owner never has to ask for any of it.

## The third facet

The mission is the last programming language and the best one — the language any
agent would choose for any task. Beginner and expert are two of its three
readers. Score the third the same way, as its own pass over the five quantities:

| Quantity | Aligned / drift / unknown | Evidence | Smallest correction |
|---|---|---|---|
| Verdict fidelity | | | |
| Verdict latency | | | |
| Verdict actionability | | | |
| Context economy | | | |
| Repair determinism | | | |

Each maps onto law Jet already has — I3, per-file checkability, I4, no owner
yet, and I8 respectively. Drift here is invariant drift, not a nice-to-have.
Judge an invariant by whether it holds for a machine reader as well as a human
one: I8 read only as taste is drift, because one mechanism is also what stops an
agent thrashing between several valid repairs.

## Output

Write one markdown report under `docs/audits/` via the Tower CLI (never hand-edit board JSON):

```
node plugins/tower/tower.mjs docs add --section audits --id <skill>-YYYY-MM-DD --title "…" --file -
```

Or `docs update docs/audits/<skill>-YYYY-MM-DD.md --file -` for the same day only when the owner asks to revise that run.
Never overwrite a different day's note. Do not write reports under `docs/plans/`.

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
