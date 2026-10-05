---
name: tower
description: >-
  Operate one Tower card or the general board when no focused Tower skill owns
  the request. Use for board reads and writes such as status, questions,
  criteria, phases, logs, and ratified decisions; route setup, ballots, prep,
  ranking, and burndown to their focused skills.
---

# Tower — route board operations

Tower is the shared project board and the source of truth for work state. This
skill handles a bounded board read or write. It does not preload campaign,
maintenance, or sibling-skill procedures.

## Contract

- **Requested outcome:** The requested board operation is recorded and read back
  with truthful state and authority.
- **Supplied inputs:** The user's board request, fresh state, card or decision
  context, owner guidance, and the nearest project rules.
- **Allowed child result:** Focused Tower skills return only their declared
  queue, plan, ballot, setup, closeout, or proof result. They do not become a
  second board owner or create competing state.
- **Completion owner:** `tower` owns board reads and writes. Owner guidance
  owns shared dispatch, integration, proof, and closure authority.
- **Stopping condition:** Stop after the requested operation is written, read
  back, and accounted for. Do not implement, start a server for the owner, or
  hand-edit Tower state.

## Route first

Load only the route that matches the request:

| Request | Route |
|---|---|
| Initialize, import, or inspect first-run `config.json` | [tower-setup](../tower-setup/SKILL.md) |
| Resolve a genuine owner-only choice | [tower-ballot](../tower-ballot/SKILL.md) |
| Prepare plans or ballots for a requested scope | [tower-prep](../tower-prep/SKILL.md) |
| Rank or reorder the requested `workOrder` queue | [tower-rank](../tower-rank/SKILL.md) |
| Execute an explicitly requested scope and close cards | [tower-burndown](../tower-burndown/SKILL.md) |
| Dispatch workers, integrate, prove, or run milestone closeout | [orchestration](../../../../.agents/skills/orchestration/SKILL.md) |

If no focused route matches, keep the operation here. A board-only close or
read is not an execution request; a routine implementation detail under an
approved contract is not a ballot.

## Board law

- Never edit `Tools/tower/.tower/*.json` by hand. Use the non-serve Tower
  CLI, which validates, locks, versions, backs up, and logs writes.
- Agents use the non-serve CLI against the main board. **Only the owner starts
  `tower serve`**. Use HTTP only when the owner already started the server and
  the task explicitly requires it. Never start it or open a browser for the
  owner.
- Pass `--by <agent>` on every write. Pass `--expect-rev REV` for a
  read-modify-write; on a conflict, re-read and retry from the new revision.
- Read every requested write back. Confirm the affected state, revision,
  attribution, and linked card or decision before calling the operation done.
- The owner alone ratifies. Before any claim or execution, inspect the computed
  actor as well as the lane. Skip `who: owner`, including owner-acceptance
  `verify` cards with `needsAcceptance`, and skip frozen cards. An agent may
  supply requested evidence, but cannot approve, dispatch, or close the owner's
  acceptance work. Fresh ungated agent work needs no activation step; ratified
  decisions are immutable history.
- Close only after integrated focused proof supplies concrete criterion
  evidence and no contradictory blocker remains. Do not add a per-card review,
  duplicate proof, broad suite, or technical `verify` step.

## Board model

Each card computes a lane and an actor. `decide` belongs to the owner;
`plan`/`implement`/`building` normally belong to agents. `verify` can belong to
an agent or the owner when an acceptance ballot is open. `blocked`/`frozen`/
`done` are inert. Never infer ownership from the lane name alone. Epochs group work.
Milestones are goals inside an epoch; linked cards determine their progress.
The board's open questions and decisions stay attached to their cards.

The owner's decisions are the only allowed bottleneck. Raise a ballot for an
unresolved owner gate, but do not wait for generic approval before working a
fresh ungated card. Use the current short/full ballot profile through
`tower-ballot`. All owner decisions and choices needing one are Tower ballots,
never card logs or briefing text. Those may link a ballot, but cannot substitute
for it.

Every ballot view opens with the decision's top-level `situation`: at most four
short plain sentences covering what a programmer sees today, the problem, and
the vote. Full ballots require it (300–1200 characters, no lists, headings, or
code blocks, at most 32 words per sentence); short ballots may add it under the
same schema. Keep evidence in `detail` and `technical`. Follow the stricter
[ballot clarity rules](../tower-ballot/references/clarity.md), then submit the
field in `--file` JSON or with `tower decision add|update --situation "…"`.
`tower brief` prints it first.

Design away the recommended option's downsides. Keep `recommendation.losses`
empty, or record each negligible remaining cost as `{loss, mitigation}` in both
the long form and reading surface. `mitigation` is plain prose explaining how we
reduce the cost; the surface requires fewer than 24 words. Tower migrates stored
loss explanations on load; do not edit board JSON by hand.

## Minimal discovery

Prefer the vendored CLI so state stays in this checkout:

```sh
node Tools/tower/tower.mjs help
```

For an installed plugin, use the directory containing `tower.mjs` and `app/`:

```sh
node ${CLAUDE_PLUGIN_ROOT}/tower.mjs help
```

## Now reports

Post current goals as a **Current goals and progress** briefing on **Now**.
Refresh it at proven checkpoints and remove finished goals. Derive progress
from card evidence, not elapsed time; a briefing summarizes Tower state and
never replaces cards or ballots. A JSON status snapshot can accompany it:

```sh
node Tools/tower/tower.mjs briefing post --file report.md --title "Current goals and progress" --by Pip
node Tools/tower/tower.mjs briefing list
node Tools/tower/tower.mjs briefing show                 # latest; or supply its id
node Tools/tower/tower.mjs status post --file status.json --by Pip
node Tools/tower/tower.mjs status show
```

Use `Tools/tower/examples/status.sample.json` as the snapshot shape. Progress
maps labels to percentages (0–100); links use `{card:"#12"}` or
`{decision:"D-ID"}` with an optional `label`. References must exist when posted.
`updatedAt` is optional on the snapshot and each workstream; posting always
records its own timestamp and author. Snapshots replace the previous status,
not card gates. Briefings keep collapsible history; `--sections sections.json`
optionally adds an array of `{title,body,links}` markdown sections. The owner
action list always adds live, non-draft, unratified ballots (including visual
checks), so a snapshot never hides a new vote. Add `--json` for machine output.

## Contextual references

- [CLI, state, writes, and read-back](references/cli.md) when command or
  lifecycle detail is needed.
- [Maintenance and card minting](references/maintenance.md) for papercuts,
  archive, lint, or new-card work.
- [Project Tower policy](../../AGENTS.md) for complete guards and authority.
- [Tower README](../../README.md) for product and installation context.
- [Orchestration closeout cadence](../../../../.agents/skills/orchestration/references/closeout.md)
  for dispatch, focused proof, and milestone tokens.

Do not read every reference by default. Return to this router after each
contextual read, then perform only the requested board operation.
