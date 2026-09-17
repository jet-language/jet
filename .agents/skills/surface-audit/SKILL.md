---
name: surface-audit
description: >-
  Audit a declared Jet surface for shape, consistency, and repairable next steps.
  Use when reviewing reachable syntax or API outliers, not implementation status.
---

# Surface audit

Find outliers and gaps in the declared Jet surface. Judge them against `docs/spec/philosophy.md` and `docs/spec/syntax-decisions.md`. End with concrete next actions as ballot titles or card IDs; do not create them unless the owner authorizes that work.

Before running, read [`_shared/audit-dispositions.md`](../_shared/audit-dispositions.md) and [`_shared/standing-lens.md`](../_shared/standing-lens.md). They own shared permissions, scope depth, evidence rules, publication, and finding dispositions. This method owns the surface question, ten-category accounting, evidence, actionability, and finite closeout.

## Scope and evidence

At activation, record the target surface and its reachable corpus. Search live specs, examples, stdlib, CLI/tooling, and direct analogues that explain the target. Include dependencies needed to understand it, not unrelated Jet. Prefer `scripts/agent/jet-env` and repository search over memory. Use the standing-lens sections relevant to this scope and probe the running binary when a claim is executable.

## Ten-category sweep

Within the declared target, account for all ten categories below. Record each as a finding, clean result, or reasoned `not-applicable`; never silently skip a category:

1. syntax
2. ergonomics
3. surfaces
4. APIs, types, and methods
5. defaults
6. naming
7. error text
8. UX and DX
9. tooling and CLI shape
10. ceremony versus control

For each outlier, use exactly one kind: **inconsistent** (same idea, two spellings), **absent** (implied shape missing), **ceremonial** (required text buys nothing), or **asymmetric** (beginner and expert roads disagree). Judge human readability, verdict actionability, and repair determinism. A clean category is a result worth printing.

## Completion and output

Stop when every reachable source and direct analogue is accounted for, all ten category rows are recorded, every finding has evidence and a concrete action or an honest `unknown`, and the disposition marker is complete. A report-only run writes one report under `docs/audits/` through the project-approved non-serve CLI, cites Tower read-only, and creates no cards, decisions, ballots, or implementation edits unless the owner explicitly changes the boundary. Report completion remains separate from implementation completion.
