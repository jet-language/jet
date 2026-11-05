# Ballot validation

[ELI5-STANDARD.md](ELI5-STANDARD.md) defines the owner's plain-language ballot
standard. `Validate.jet` checks ballot payload files against the rules Tower
applies when a ballot is posted (`ballotGaps` and `addDecision` in
`Tools/tower/app/store.mjs`, ballot process 5). It reads only the payloads: no
Tower store, board or state root, and it writes nothing, so it runs from a
fresh clone with only `jet`.

```sh
jet Tools/agent/ballots/Validate.jet "$JET_STATE_ROOT/ballots/READY/D-EXAMPLE.json"
jet Tools/agent/ballots/Validate.jet --update edited-ballot.json
```

Each payload prints `ok <path> (<id>)`, or its gaps one per line. Exit status
is 1 if any payload has a gap and 2 with no payloads. `--update` reads the
payloads as edits of ballots already on the board, so the new-ballot rules
(title, recommended option first) are skipped. Rules that need the board use
only what the payload carries: a short ballot's card tags come from `cardTags`
or `card.tags`. Whether `cardId` names a live card, whether the id is taken and
whether a verdict's `supersededBy` exists are checked by Tower when the ballot
is posted. Review and owner approval remain separate from validation. Do not
move mutable payloads or review receipts into this directory.
