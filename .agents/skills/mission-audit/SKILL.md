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

Before running, read `.agents/skills/_shared/audit-dispositions.md`. It owns
shared publication, workflow-boundary, and disposition mechanics; this method
still owns the mission scorecard, third-facet pass, evidence, and stopping rule.


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

This is a report-only method. Write one markdown report under `docs/audits/`
through the project-approved non-serve CLI. Do not create Tower work or
implementation edits unless the owner explicitly asks. Read
`.agents/skills/_shared/audit-dispositions.md` before the run and use it for
publication rules and the required finding-disposition table. Keep the
aligned/drift/unknown score and corrective action separate from implementation
completion.
