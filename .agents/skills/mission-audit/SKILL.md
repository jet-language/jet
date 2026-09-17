---
name: mission-audit
description: >-
  Score Jet's alignment with philosophy and invariants across a declared mission
  slice. Use when checking whether a language area serves its intended readers.
---

# Mission audit

Score the requested Jet area against `docs/spec/philosophy.md` and `AGENTS.md` invariants. Check beginner defaults, expert control, one mechanism, hidden rustc, diagnostics as product, batteries, systems path, one package graph, and lean tools. Mark each dimension `aligned`, `drift`, or `unknown` with evidence and the smallest corrective action. Treat the philosophy as the target; do not claim current status without evidence.

Before running, read [`_shared/audit-dispositions.md`](../_shared/audit-dispositions.md) and [`_shared/standing-lens.md`](../_shared/standing-lens.md). They own shared permissions, scope depth, evidence rules, publication, and finding dispositions. This method owns the mission scorecard, coding-agent facet, evidence, and finite closeout.

## Scope and standing lens

At activation, freeze the mission slice, relevant sources, dependencies, and required workloads. Apply only the standing-lens sections relevant to that declaration. Use the four questions, five quantities, micro sweep, and runtime probes when the slice calls for them; do not force unrelated evidence. Mark unavailable evidence `unknown`.

## The third facet

The mission has three readers: beginner, expert, and coding agent. Score the coding agent as its own pass over the five quantities:

| Quantity | Aligned / drift / unknown | Evidence | Smallest correction |
|---|---|---|---|
| Verdict fidelity | | | |
| Verdict latency | | | |
| Verdict actionability | | | |
| Context economy | | | |
| Repair determinism | | | |

Use this invariant mapping without making an unsupported ownership claim: verdict fidelity maps to I3; verdict latency to per-file checkability; verdict actionability to I4 diagnostics; context economy to the measured evidence burden; repair determinism to I8's one-mechanism rule. If a quantity has no ratified invariant or owner, mark `unknown` and name the missing authority instead of inventing one. Judge each invariant for machine and human readers; I8 is drift when several plausible repairs can make an agent thrash.

## Completion and output

Stop when every named mission dimension and all five coding-agent quantities have a grade, evidence or an honest `unknown`, and smallest correction; every required source and workload is accounted for; and the report's disposition marker is complete. A report can contain zero drift findings when the declared evidence supports that result.

This is a report-only method. Write one report under `docs/audits/` through the project-approved non-serve CLI, cite existing Tower records read-only, and create no cards, decisions, ballots, or implementation edits unless the owner explicitly changes the boundary. Keep alignment grades and corrective actions separate from implementation completion. Follow the shared disposition contract before publication.
