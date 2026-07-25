---
name: tower-prep
description: >-
  Prepare Tower cards for a burndown: write plans, raise owner ballots, and
  leave every in-scope non-frozen card either ready to implement or waiting in
  decide. Use when asked to prep the board, prepare for burndown, plan every
  card, expose decisions, or when invoked as /tower-prep. Does not implement
  or close cards.
---

# Tower — prep for burndown

Make the board burndown-ready. When this sprint ends, every in-scope card
except `frozen` is either:

- **ready / implement / building / verify** with a real plan and no open
  owner gate, or
- **decide** with a ballot-ready decision the owner can pick from alone.

Do **not** implement features, run product tests as proof of delivery, or mark
cards `done`. Ranking is **tower-rank**. Closing cards is **tower-burndown**.

## Triggers

`/tower-prep`, `/tower-prep epoch 3`, `/tower-prep sidequests`,
`/tower-prep e3`, or “prep the board / prepare for burndown / plan every
card / expose decisions.”

Default scope when unspecified: **sidequests first**, then
`meta.currentEpoch` epoch-track cards. Honor an explicit grouping when given.

## Reference index

| Need | Source |
|---|---|
| Board mechanics, claims, phases, CLI | `../tower/SKILL.md` |
| Ballot fields and density rules | `../tower-ballot/SKILL.md` |
| Ordered queue / workOrder | `../tower-rank/SKILL.md` |
| Project invariants and gates | nearest `AGENTS.md` |
| Domain law for one card | that card's `refs` + triggered specs |

Load the smallest slice. Never hand-edit `plugins/tower/.tower/*.json`.

## Session loop

1. `tower status` + `tower question list --open` — answer open questions first.
2. Enumerate in-scope agent-lane and `decide` cards (skip `frozen` / `done`).
3. For each card, in dependency-safe order:
   - Read `tower brief '#N' --no-claim` (or claim only if you must write).
   - Write or refresh a thorough `--plan` that names acceptance, proof, and
     owned paths.
   - Enumerate owner gates (new syntax, new stdlib external dep, invariant
     carve-out, genuine UX/taste). Kill a design slice before ballot when it
     breaks an invariant, duplicates a mechanism, burdens beginners without
     need, or hides expert control.
   - For each surviving gate: author a ballot-ready decision via
     **tower-ballot** (`--draft` only while unfinished; `--ready` when the
     owner can decide from the ballot alone).
   - Advance honestly: planning → ready when ungated; leave in decide while
     ballots are open. Log what changed with `--by <agent>`.
4. Optionally run **tower-rank** if the user asked to reorder as part of prep.
5. `tower lint` and report remaining decide-queue + ready counts.
6. Stop. Do not start implementation.

## Writing rules

- **simple** for every user-visible string: plans, ballot prose, card bodies,
  log lines the owner reads, docs notes meant for humans.
- **caveman** for agent-to-agent status chatter only.
- **ponytail** when editing skill/docs structure for clarity — still no
  product implementation.

## Done means

- In-scope non-frozen cards are ready-to-build or owner-decide.
- Every open decision is ballot-ready (or explicitly `--draft` with reason).
- No implementation diffs landed under the guise of prep.
- Brief owner report: ready count, decide count, blocked/external, next
  suggested `/tower-burndown …` invocation.
