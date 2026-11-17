# Issue tracker: Tower

Tower is Jet's durable work ledger. This page records the repository's card
vocabulary and the commands that address it. The executable command reference
is [`Tools/Tower/skills/tower/SKILL.md`](../../../Tools/Tower/skills/tower/SKILL.md);
the board policy is [`Tools/Tower/AGENTS.md`](../../../Tools/Tower/AGENTS.md).
Use the non-serve CLI for card operations. Never edit
`Tools/Tower/.tower/*.json` by hand. Card text uses the
[Jet vocabulary](../vocabulary.md) for language terms.

## Start with the CLI

From the repository root, use the vendored command:

```sh
alias tower='node Tools/Tower/tower.mjs'
tower help
```

Only the owner starts `tower serve`. Agents use the non-serve CLI against the
main board and do not start a second server. Tower validates writes, takes the
board lock, keeps backups, and records revisions.

## Work with cards

Use one homed card for each incomplete stream. Put the implementable plan,
dependencies, owner gates, complete cutover, and observable criteria on that
card. Keep work state in Tower rather than in a second task list or a durable
status document.

Before adding a card, look for the card that already owns the area. A
regression, follow-up, or gap found while proving work becomes a criterion on
the card whose change caused it, or on the card that owns the area; the
findings of one sweep share one card. Only a new stream of work (a ballot, a
feature, an epoch's plan) earns a new card (AGENTS.md, Owner gates and work
state).

| Operation | Command |
|---|---|
| Create a bug card | `tower card add --title "..." --body "..." --kind bug --add-tag needs-triage --by <agent>` |
| Create a feature card | `tower card add --title "..." --body "..." --kind feature --add-tag needs-triage --by <agent>` |
| Read a card | `tower card show '#N' --json` |
| List cards | `tower card list --json` |
| Filter cards | `tower card list --json --lane <lane> --phase <phase> --tag <tag>` |
| Log an update | `tower card update '#N' --log "..." --by <agent>` |
| Add or remove a triage tag | `tower card update '#N' --add-tag <tag> --remove-tag <tag> --by <agent>` |
| Ask a question | `tower question ask '#N' --text "..." --by <agent>` |
| Add a message | `tower message add '#N' --text "..." --by <agent>` |
| Move a card to an epoch | `tower card update '#N' --epoch e18 --by <agent>` |
| Record blockers | `tower card update '#N' --blocked-by '#1,#2' --by <agent>` |
| Claim a card | `tower brief '#N' --agent <agent>` |

Use `--file payload.json` or `--file -` when a card body or update needs
multiple lines. A card number is a stable handle, like a GitHub issue number.
An intake item that is not yet a card belongs in Ideas:
`tower idea list`, `tower idea add`, and `tower idea promote`.

The orchestrator closes a card only after integrated focused evidence proves its
criteria. The close operation is:

```sh
tower card update '#N' --phase done --by <orchestrator>
```

Read the card back and confirm `done`. To decline work, the owner uses:

```sh
tower card update '#N' --add-tag wontfix --phase frozen --by owner
```

## Epochs

Epochs are the one planning axis. Each epoch is one stretch of work, such as
"declarations as types" or "remove Rust", worked in the board's epoch order.
Its exit criteria say what finishing it means. Write an epoch as `E18` or
`E18 <name>`; the stored id stays `e18`.

| Operation | Command |
|---|---|
| List epochs in order | `tower epoch list --json` |
| Add an epoch | `tower epoch add e26 --name "..." --order 9 --exit-criteria "..." --by <agent>` |
| Reorder or edit an epoch | `tower epoch update e26 --order 3 --exit-criteria "..." --by <agent>` |
| Remove an unused epoch | `tower epoch remove e26 --by <agent>` |

`tower epoch list` shows epochs by `order`, then by id number, with their exit
criteria. `--exit-criteria` repeats for each row; `--file` takes
`{"order": N, "exitCriteria": ["..."]}`. Tower refuses `epoch remove` while any
live or archived card, or any milestone, still points at the epoch, and names
the counts.

## Messages and questions

Write Tower messages in ELI5 language. Start with what changed or what is
broken, use short sentences, define technical terms, and put exact commands,
error codes, and evidence paths after the explanation. A message reports an
update; a question asks for an answer; an owner decision uses the ballot path.
Do not call unchecked work finished.

## Pull requests

External GitHub pull requests are not the triage queue. Collaborator delivery
work uses Tower cards, not GitHub Issues. When a skill says to publish to the
issue tracker, create a Tower card. When it says to fetch a ticket, read the
card and its linked decisions with the Tower CLI.

## Wayfinder maps

A Wayfinder map is one parent card with child cards:

- Tag the map `wayfinder:map`. Its body holds Destination, Notes,
  Decisions-so-far, Fog, and Out of scope.
- Set a child's parent to the map card and tag it
  `wayfinder:research`, `wayfinder:prototype`, `wayfinder:grilling`, or
  `wayfinder:task`.
- Use Tower's native `blockedBy` relationship. A child is unblocked only when
  every blocker is done or its blocking decision is ratified.
- Claim work with `tower brief '#N' --agent <agent>` as the session's first
  board write.
- Resolve a child by recording its answer, setting its phase to `done`, and
  adding a one-line gist and link to the map's Decisions-so-far.

Triage tags and delivery phases are separate concepts; use
[`triage-labels.md`](triage-labels.md) for their exact mapping.
