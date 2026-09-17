---
name: surface-frequency-audit
description: >-
  Measure how programming surfaces appear in declared public code and rank Jet
  friction. Use for an explicit full corpus audit or a bounded frequency
  question that needs resumable evidence.
---

# Surface Frequency Audit

Produce one evidence-backed Markdown report for the owner. Measure first.
Recommend only after the evidence is stable.

## Non-negotiable bounds

- Retain only the final Markdown report. Keep unfinished checkpoints under
  `.tmp/surface-frequency-audit/<run-id>/` and remove them after installation.
- Never create or change Tower cards, decisions, ballots, Tower docs, or board
  state. This method is report-only; it may read Tower late for mapping.
- Treat public source as evidence of written use, not runtime frequency or
  private production behavior.
- State the declared scope, denominator, provenance, and every coverage gap.
  Missing or unavailable evidence is not zero.
- Keep full requested corpus coverage, measurement catalogs, denominator rules,
  and resume discipline. A bounded route must name what it did not attempt.

## Stage router

Before collection, read `.agents/skills/_shared/audit-dispositions.md` and
`.agents/skills/_shared/standing-lens.md`, then read
[`references/operations.md`](references/operations.md). The shared lens scopes
evidence to the declared subject. Then choose one route:

Read `AGENTS.md` for repository conduct. Search the current tree before broad
reading and preserve unrelated worktree changes.

- **Full audit:** the owner requests exhaustive or full corpus coverage. Load
  [`references/method.md`](references/method.md) and use its complete baseline,
  catalogs, metrics, and required category coverage.
- **Focused audit:** the owner names a bounded language, domain, workload,
  source set, or surface question. Load `references/method.md` for metric
  definitions, declare the boundary, and use `operations.md` to record every
  omitted cell or metric. Do not call the result exhaustive.

At the collection stage, do not require a report-template or prose-style read.
At the report stage, load
[`references/report-template.md`](references/report-template.md) and
`.agents/skills/simple/SKILL.md`. The template remains the final report
contract; the stage order prevents prose ceremony from blocking evidence.

## Start or resume

Use the commands in [`references/operations.md`](references/operations.md) from
the repository root. Its `init` command pins `AGENTS.md`, this root,
`references/method.md`, `references/report-template.md`,
`references/operations.md`, the ontology, `simple`, the standing lens, and
audit dispositions. The checkpoint tool also pins its checkpoint and
aggregation scripts.

Use one stable run directory and one report target. If the run directory exists,
resume it; do not reinitialize it. Inspect status before claiming work.

The run records a digest for each listed config. Stop if a saved digest changed.
Restore the pinned input or start a new run. Owner authority remains live for
scope and policy decisions; reconcile an approved change in a new run or in the
retained report. Never mix a changed policy silently.

## Operational stages

Use [`references/operations.md`](references/operations.md) for the commands
and close conditions for:

1. freezing pinned source identities and stratified corpus scope;
2. building and independently reviewing official catalogs;
3. planning units and resuming leases;
4. collecting normalized measurements and closing blocked or unavailable units;
5. aggregating, checking denominators, and running sensitivity views;
6. late read-only Jet/Tower cross-checks;
7. fresh-reader review, atomic report installation, and checkpoint cleanup.

If a unit cannot be collected, use the documented `checkpoint.py block` command
with an exact reason; add `--unavailable` only when no sound public sample
exists. Never call a blocked unit complete.

## Close

Before installation, validate with `--require-complete`, reconcile raw totals
with project/language/domain/stratum aggregates, and obtain a fresh-context
review of source classification, parser gaps, denominators, arithmetic, rank
stability, Jet claims, Tower mappings, and clarity. Fix findings and validate
again. Install only through the method-owned checkpoint command. Remove only
the completed run directory.

The retained artifact is one report. Include the required
`audit-dispositions:v1` marker from `.agents/skills/_shared/audit-dispositions.md`.
It records recommendation dispositions; it does not authorize Tower writes.
Report the path, declared scope, coverage, strongest limit, and review result.
Do not create follow-up work.
