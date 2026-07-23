---
name: tower-rank
description: >-
  Rank and prioritize Tower cards into a dependency-safe ordered queue while
  preserving scope, gates, and active ownership. Use when asked to rank,
  reorder, triage, thin, choose next Tower work, or when invoked as
  /tower-rank. Produces or applies workOrder values; does not implement,
  review, verify, plan, or ballot cards.
---

# Tower — rank the queue

One job: turn live Tower state into a dependency-safe ordered queue. Stop after
reporting the queue or applying requested `workOrder` values. Planning is
**tower-prep**. Closing cards is **tower-burndown**.

## Reference index

Load only what the ranking question needs:

| Need | Source |
|---|---|
| Lanes, claims, blockers, criteria, archive, CLI writes | `../tower/SKILL.md` |
| Ballot readiness (gate detection only) | `../tower-ballot/SKILL.md` |
| Missing board / setup | `../tower-setup/SKILL.md` |
| Project rules | nearest `AGENTS.md` |
| Domain semantics for one card | that card's `refs` |

Do not preload every sibling skill or spec.

## Select scope

Default rank scope comes from `tower next --burndown --json`: current
epoch-track cards plus all sidequests, agent lanes only. Use another epoch,
track, or the whole non-frozen board only when the user requests it.

Exclude `decide`, `frozen`, `done`, and externally blocked cards from the
actionable queue. Keep them in a separate gates list when they explain why
downstream work cannot start. Preserve active claims; ranking must not
reassign a claimed card.

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

Prefer order that avoids redo: do not put low-hanging fruit ahead of work that
will force rework of that fruit. Prefer law/syntax/structure cards that later
work builds on.

Likely file, generated-artifact, test-resource, or service collisions are
queue metadata. Record them so an implementer can serialize or isolate work.

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

Re-read after revision conflicts. Never edit `plugins/tower/.tower/*.json`
directly. Verify final coverage, unique ranks, dependency order, and unchanged
claims. Then hand the ordered queue and collision metadata to the implementer
or **tower-burndown**.

User-facing table copy and card log notes use the **simple** skill. Agent
status chatter uses **caveman**.
