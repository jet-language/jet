# Tower CLI and board operations

Read this reference only when the requested board operation needs command detail.
The root skill is the router; `plugins/tower/AGENTS.md` remains the policy authority.

## Find the CLI

Prefer the vendored executable so the board stays in this checkout:

```sh
node plugins/tower/tower.mjs help
```

For an installed plugin, use the directory that contains `tower.mjs` and `app/`:

```sh
node ${CLAUDE_PLUGIN_ROOT}/tower.mjs help
```

An alias is optional. Do not assume that an installed copy owns this project's
`.tower` directory.

## Read the board

Use the smallest read that answers the question:

```sh
tower status
tower state --json
tower card show '#12' --json
tower question list --open
tower decision list --json
tower brief --no-claim
```

Read an unclaimed packet first and inspect its computed `who`; a next-card
picker can return owner-acceptance `verify` work. Skip owner work before taking
a lease. Only after that check, `tower brief '#N' --agent <me>` takes a renewable 24-hour lease.
`E_CLAIMED` means another agent owns the card. `E_CLOSE_READY` means an
actively claimed card already has every criterion met and no open gate. Close,
reopen, or block that card before any other brief or claim; the barrier also
applies to `--no-claim` reads.

## Write and read back

Every mutation goes through the non-serve CLI. Pass `--by <agent>` and, for a
read-modify-write, `--expect-rev REV`. A revision conflict means re-read the
card and retry from the new revision. Never edit
`plugins/tower/.tower/*.json` by hand.

After a requested write, read the affected card, decision, question, or board
state back. Confirm the phase, criteria, log attribution, revision, and linked
state that the write was meant to change. A successful command without a
read-back is not completion.

Workers do not write Tower. In an orchestrated run, they return `CHECK OK` for
code or `DOCS ONLY` for prose/static data. The orchestrator attributes phase,
criteria, proof, and handoff changes on the board.

## State and closure

Cards compute to one lane: `decide` is owner-only; `plan`, `implement`,
`building`, and `verify` are agent work; `blocked`, `frozen`, and `done` are
inert. Epochs group work. Milestones group goals inside an epoch and derive
progress from linked cards.

A card needs a non-empty criteria checklist. Each row moves from `open` to
`met` to `verified`. Integrated focused proof supplies `met`; a different
reviewer may supply `verified`. Close a card as soon as every row has concrete
evidence and no contradictory blocker remains. Do not add a per-card review,
duplicate proof, broad suite, or technical `verify` step. Use `verify` only for
an explicit owner acceptance or closeout follow-up.

Only the owner starts `tower serve`. Agents use the non-serve CLI. Use HTTP
only when the owner has already started the server and the task explicitly
requires it; never start a server or open a browser for the owner.

For dispatch, worker receipts, integration, focused proof, and milestone
cadence, read the canonical orchestration references:

- [dispatch and receipts](../../../../../.agents/skills/orchestration/references/dispatch.md)
- [milestone closeout](../../../../../.agents/skills/orchestration/references/closeout.md)
- [owner policy](../../../AGENTS.md)
