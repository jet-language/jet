---
name: lessons-learned
description: >-
  Compare named peer-language lineage for Jet's risks and transferable strengths.
  Use for a focused lesson request or a broad family review.
---

# Lessons Learned

For each named lesson, record the peer failure or strength, the Jet risk or
opportunity, and the guard or state: invariant, ratified decision, open card,
ballot needed, shipped, ratified-but-unbuilt, or absent.

## Scope and route

Name the peer, language family, workload, and question before collecting
evidence. A focused request covers only that declared lineage scope. A broad
request loads [`references/full-review.md`](references/full-review.md) and
covers systems, managed, functional, scripting, config/OS, and proof-oriented
families. Mark an unavailable family or source with its exact reason. Do not
widen scope because another lineage looks interesting.

Read `.agents/skills/_shared/audit-dispositions.md` before running. It owns
shared publication and workflow boundaries. This method remains report-only:
write one Markdown report under `docs/research/` through the
project-approved non-serve CLI. Do not create Tower work or implementation
edits unless the owner explicitly asks.

Include the required `audit-dispositions:v1` finding-disposition table in the
retained report. It records report dispositions; it does not authorize Tower
writes.

## Evidence

Use `.agents/skills/_shared/standing-lens.md` in context. A full lineage review
uses its four questions, five agent-optimality quantities, relevant micro sweep,
running-binary probes, and honesty rules. A focused review uses only relevant
questions and categories, and records non-applicable checks instead of doing
unrelated work.

For every peer and lesson, record the exact source URL or repository, version or
commit, publication or retrieval date, and locator: section, file:line,
timestamp, issue, or decision ID. Separate primary, audience, local Jet, and
inference evidence. Preserve enough provenance to reproduce each claim.

## Avoid and Beat

Report both sides when evidence supports them. Do not force equal counts or
equal depth. Weight space by evidence strength and consequence; state when one
side has weak or no support.

- **Avoid** — peer failure → Jet risk → guard. Include structural immunity once
  when it is a design asset.
- **Beat** — peer strength → mechanism → Jet state (`shipped`,
  `ratified-but-unbuilt`, or `absent`). Rank by how categorical and supported
  the win is. State what the peer must change to match Jet, including when it
  can copy the idea without breaking its model.

Include a do-not-ballot list where existing Jet law already covers the concern.
Say what Jet does instead. Do not turn the report into a ballot, card, or
implementation change.

## Stop

Stop when every named peer and family has its requested evidence, Jet state,
failure mode, guard or opportunity, and uncertainty recorded. A broad review
stops only after every named family is covered or marked unavailable. Report
completion does not implement a guard or change Jet law.

