---
name: tower-burndown
description: Rank and prioritize Tower cards for a burndown while preserving scope, dependencies, gates, and active ownership. Use when asked to rank, reorder, triage, thin, or choose the next Tower work. Produces an ordered queue for Codeflow; it does not orchestrate, delegate, implement, review, integrate, or verify cards.
---

# Tower — rank a burndown

This skill has one job: turn live Tower state into a dependency-safe ordered
queue. **Codeflow owns execution**: planning, delegation, write isolation,
implementation, checkpoints, review, verification, integration, and resume.
Stop this skill after reporting the queue or applying requested `workOrder`
values.

## Reference index

Load only the section needed for the current ranking question:

| Need | Source |
|---|---|
| Lanes, claims, blockers, criteria, archive, CLI writes | `../tower/SKILL.md` |
| Ballot readiness or an owner-gated choice | `../tower-ballot/SKILL.md` |
| Missing board, import, config, or server startup | `../tower-setup/SKILL.md` |
| Project-specific scope, authority, model, review, or command rules | nearest `AGENTS.md` |
| Campaign execution after ranking | `codeflow` skill |
| Domain semantics for one card | that card's `refs` and triggered project index |

Do not preload every sibling skill, repository manual, plan, or spec. Start
from live board state; follow a pointer only when a card or requested action
needs it.

## Select scope

Default burndown scope comes from `tower next --burndown --json`: current
epoch-track cards plus all sidequests, agent lanes only. Use another epoch,
track, or the whole non-frozen board only when the user requests it.

Exclude `decide`, `frozen`, `done`, and externally blocked cards from the
actionable queue. Keep them in a separate gates list when they explain why
downstream work cannot start. Preserve active claims; a claimed card remains
owned and must not be reassigned by ranking.

## Rank

Dependencies outrank ease: no card may appear before an unfinished
`blockedBy` predecessor. Within each ready dependency layer, use these bands:

1. Proven complete work whose board state only needs reconciliation.
2. Tiny ungated repair with direct mechanical proof.
3. Narrow test, documentation, tooling, or durability work.
4. Existing implementation with a small concrete gap.
5. Bounded implementation governed by ratified behavior.
6. Broad cross-layer or architectural work.
7. Owner-gated or externally blocked work; report separately, do not schedule.

Tie-break in this order: already in progress, smaller verified remainder,
unblocks more ready work, lower current `workOrder`, then card number. Never
rank by implementation cost when project policy forbids effort as a design
criterion; here, size only optimizes safe delivery order after behavior is
settled.

Likely file, generated-artifact, test-resource, or service collisions are
queue metadata, not orchestration instructions. Record them so Codeflow can
choose serialization or isolation.

## Output and optional write

Return an ordered table with rank, card, band, dependency reason, active
claim, likely collision domain, and evidence/confidence. Also return owner
gates, external blockers, and cards excluded from scope. Do not claim cards.

Ranking is read-only by default. When the user asks to reorder Tower, assign
unique ascending `workOrder` values with no dependency inversion. Apply each
change through:

```
tower card update '#N' --work-order N --expect-rev REV --by <agent>
```

Re-read after revision conflicts. Never edit `.tower/*.json` directly.
Verify final coverage, unique ranks, dependency order, and unchanged claims.
Then hand the ordered queue and collision metadata to Codeflow.
