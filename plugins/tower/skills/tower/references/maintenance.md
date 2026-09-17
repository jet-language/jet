# Tower maintenance operations

Read this reference only when the task concerns recurring tooling friction,
retirement, lint, or card creation. These operations do not replace the board
policy in `plugins/tower/AGENTS.md`.

## Papercuts

A papercut is recurring agent-tooling friction. Log one only when the same snag
happened at least twice or the dead end is deterministic for anyone using the
same command and flags:

```sh
tower papercut add --by <agent> --text "<reproducible tooling snag>" [--card '#N']
```

Do not log one-off failures, self-inflicted dirty state or stale caches,
session collisions, friction already fixed in passing, or a Jet compiler,
stdlib, example, test, or golden defect. Those defects are cards. Logging a
papercut must not block on a frozen or owner `decide` lane. The owner resolves
one with `tower papercut resolve <id> --by owner`.

## Archive

A `done` card or ratified decision remains live for `config.retireAfterDays`
(default three days), then retires to
`plugins/tower/.tower/history.json`. A card's questions and decisions retire
with the card, so a card view is never half-archived. Use these commands when
history matters:

```sh
tower archive status
tower archive show <id>
tower archive restore <id>
```

`card show` and `decision show` fall through to history after retirement. A
live ratified decision still prevents `card delete`; restore and detach it, or
let it retire naturally.

## Lint

`tower lint` is a read-only durability sweep. It reports, among other things,
done cards without criteria or integration evidence, claims idle for three or
more days, events without `by`, decisions that fail the ballot-ready gate,
stale drafts, and dangling `blockedBy` references. `tower lint --docs` also
checks `docs/spec/**` for card and decision IDs with no live or historical
record. Exit code one means findings exist; repair them or raise the applicable
ballot. Do not clear the board to hide a finding.

Run lint only when the request needs a durability finding. It is not a default
step for a one-card operation or for preparation.

## Minting cards

The open-card count is a promise. Before adding a card:

1. Probe the named symptom once.
2. Close or retarget an already-fixed cause; never duplicate it.
3. Add one deduplicated card for an unrelated defect.
4. Retarget only the same persistent problem with a new cause.
5. Group one worker-sized mechanism behind one proof boundary.
6. State plainly when the new card is separate, uncovered work.

Do not quote an unmeasured scope. A full-corpus census belongs before a
campaign or under a milestone closeout token, not inside an active
close-and-refill loop.
