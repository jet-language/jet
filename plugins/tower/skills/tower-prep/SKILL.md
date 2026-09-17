---
name: tower-prep
description: >-
  Prepare an explicitly requested Tower scope by writing plans and exposing
  genuine owner decisions, without implementation or closure. Use for prep,
  burndown readiness, planning every requested card, or exposing decisions;
  route queue order to tower-rank and execution to tower-burndown.
---

# Tower — prepare a requested scope

## Contract

- **Requested outcome:** The requested board scope has honest plans and
  ballot-ready owner gates, with no implementation or closure performed.
- **Supplied inputs:** Fresh Tower state, requested scope, card references,
  dependencies, project rules, and current ballot requirements.
- **Allowed child result:** `tower`, `tower-rank`, and `tower-ballot` return only
  board reads, queue order, or a ready ballot. They do not implement, close, or
  create competing state.
- **Completion owner:** `tower-prep` owns plan and ballot preparation. The
  owner owns choices; Tower owns board state.
- **Return point:** Each read, plan, or ballot result returns to the current
  preparation pass before the next card.
- **Stopping condition:** Stop after every card in the requested scope is
  accounted for. Each unblocked actionable card is prepared or is already
  progressing with a valid plan; done/frozen and blocked/external cards have
  explicit counts or reasons. Do not start implementation or close cards.

## Scope and triggers

```text
/tower-prep
/tower-prep epoch 3
/tower-prep sidequests
/tower-prep e3
```

Use `/tower-prep`, “prepare this scope,” “plan these cards,” or “expose the
owner decisions” when the user requests preparation. If no grouping is given,
use sidequests first, then `meta.currentEpoch` epoch-track cards. Honor an
explicit grouping.

An **actionable** card is in the requested scope, is not stored as `done` or
`frozen`, and is not blocked by an internal or external dependency. A card in
the computed owner `decide` lane is actionable for ballot preparation, but it
stays in its stored `deciding` phase until the owner ratifies. A card already in
computed `implement`, `building`, or `verify` (`Review` in the UI) may retain
that phase when its plan and gates are valid. A blocked or external card is
still part of scope accounting, but must keep its reason and must not be forced
to `ready`. Done and frozen cards are counted but not touched.

Tower stores these card phases: `deciding`, `planning`, `ready`, `building`,
`verify` (shown as `Review`), `done`, and `frozen`. `triage` is a legacy stored
spelling. Computed lanes are different: `decide`, `plan`, `implement`,
`building`, `verify`, `blocked`, `frozen`, and `done`. Report both the stored
phase and computed lane; never write a computed lane as a phase.

## Reference index

| Need | Source |
|---|---|
| Board mechanics, claims, phases, CLI | [tower](../tower/SKILL.md) |
| Ballot fields and profile gates | [tower-ballot](../tower-ballot/SKILL.md) |
| Ordered queue / `workOrder` | [tower-rank](../tower-rank/SKILL.md) |
| Project invariants and owner gates | [Tower policy](../../AGENTS.md) |
| Domain law for one card | that card's `refs` and triggered specs |

Load the smallest slice. Never hand-edit `plugins/tower/.tower/*.json`.

## Preparation pass

1. Read fresh status and open questions. Answer only factual questions within
   existing authority. Never decide an owner's choice; leave it open and route
   it to [tower-ballot](../tower-ballot/SKILL.md).
2. Snapshot every card in the requested scope, including `done`, `frozen`,
   blocked, and external cards, so final accounting is complete.
3. For each unblocked actionable card, read `tower brief '#N' --no-claim`
   (claim only when a write requires it), its dependencies, refs, and recent
   decisions. Write or refresh a plan naming observable acceptance, exact
   proof, and owned paths.
4. Identify only genuine owner gates: new syntax, a new external dependency,
   an I1–I9 carve-out, product or scope direction, or real UX/taste. Discard a
   design slice that breaks an invariant, duplicates a mechanism, burdens
   beginners without need, or hides expert control.
5. For each surviving gate, use `tower-ballot`: keep incomplete fields and
   reader evidence in its scratch file. Submit a valid draft, then explicitly
   use `--ready` and read back the owner gate when the ballot is complete. Use
   `short` for one mechanism with at most three options; use `full` for new
   syntax, invariant carve-outs, or owner-tagged full cards. Full ballots use
   two fresh readers; see the [ballot review mechanics](../../../../.agents/skills/orchestration/references/ballot-review.md).
6. Advance only when preparation requires it: an ungated `planning` card may
   become stored phase `ready`; an open owner gate keeps the card in stored
   `deciding` and computed lane `decide`; blocked or external cards keep their
   reason. A card already in `ready`, `building`, or `verify` (`Review`) may
   retain its phase when its plan is valid. Never churn an in-progress card to
   make counts look ready.
7. Run [tower-rank](../tower-rank/SKILL.md) only when the user also requested
   queue order. Run `tower lint` only when requested or when a board finding
   must be reported; lint is not a preparation gate.
8. Stop and report finite accounting. Each scoped card appears once with its
   stored phase (`deciding`, `planning`, `ready`, `building`, `verify`, `done`,
   or `frozen`) and computed lane. Include `Review` for displayed `verify`,
   plus the reason for every blocked or external card. Label counts clearly:
   `deciding`/`decide` and `ready`/`implement` are different stored and
   computed values. Report ready, decide, in-progress, blocked, external, done,
   and frozen counts and the next suggested execution scope.

## Writing rules

Use `simple` for plans, ballot prose, card bodies, log lines, and owner-facing
notes. Use `caveman` only for agent status chatter. Use `ponytail` when making
skill structure clearer. These style rules do not authorize product work.

## Done means

- Every unblocked actionable card has a real plan and is either owner-decide
  with a ballot-ready decision, stored `ready`, or already progressing in
  stored `building`/`verify` (`Review`) phase.
- Every blocked or external card is listed with its actual reason.
- Done and frozen cards are counted but not touched.
- Every open decision is ready, or explicitly a draft with a reason.
- No implementation diff, product proof, or card closure landed under the
  guise of preparation.
- The owner receives complete phase-and-lane accounting and a suggested
  `/tower-burndown …` invocation.
