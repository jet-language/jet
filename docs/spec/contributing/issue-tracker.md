# Issue tracker: Tower

Tower is the durable work ledger for this repository. Use the non-serve CLI for card operations. Never hand-edit `plugins/tower/.tower/`.

```sh
alias tower='node plugins/tower/tower.mjs'
tower help
```

The board data is in `plugins/tower/.tower/`. Only the owner starts `tower serve --open`. Agents never start a second server. Full board mechanics live in `plugins/tower/skills/tower/SKILL.md` and `plugins/tower/AGENTS.md`.

## Card operations

- **Create:** `tower card add --title "..." --body "..." --kind bug|feature --add-tag needs-triage --by <me>`. Use `--file payload.json` or `--file -` for multi-line bodies.
- **Read:** `tower card show '#N' --json`. Reads fall through to the archive after retirement.
- **List:** `tower card list --json` with `--lane`, `--phase`, `--epoch`, `--track`, `--kind`, `--tag <name>`, `--untagged`, or `--parent '#N'`.
- **Log:** `tower card update '#N' --log "..." --by <me>`.
- **Triage tags:** add or remove tags with `--add-tag` and `--remove-tag`.
- **Questions:** `tower question ask --card '#N' --text "..." --by <me>`.
- **Messages:** `tower message add '#N' --text "..." --by <me>`. Use a message for an update that needs no answer, not a question or ballot.
- **Tooling friction:** `tower papercut add --by <me> --text "..." [--card '#N']`. Log only a deterministic, repeatable in-repository tooling fault that is not already logged and is not a Jet defect.
- **Blockers:** `tower card update '#N' --blockedBy '#1,#2' --by <me>`.
- **Claim:** `tower brief '#N' --agent <me>` or `tower card claim '#N' --by <me>`.
- **Close:** the orchestrator runs `tower card update '#N' --phase done --by <me>` only after integrated focused evidence and a fresh query showing `done`.
- **Won't fix:** `tower card update '#N' --add-tag wontfix --phase frozen --by owner`, or delete an unpromoted idea.

Card numbers are stable handles, like GitHub issue numbers. An intake item that is not yet a card belongs in Ideas (`tower idea list|add|promote`).

## Card contract

Every incomplete stream has one homed card in an epoch, sidequest, or frozen state. Put the implementable plan, dependencies, owner gates, complete cutover, and observable criteria on the card. Keep work state in Tower, not in a parallel task list or a durable status document. Close each card as soon as its integrated criteria are proven; milestone review is a separate later gate.

## Messages to the owner

Write every Tower message in ELI5 language: explain it for someone who does not know the compiler or the task. Start with what changed or what is still broken. Use short sentences, explain any needed technical term, and say what happens next. Keep exact commands, error codes, and evidence paths after the explanation. Do not weaken the facts or call unchecked work finished.

Messages use light blue. Finished-card notices use blue. Owner-check pills, counts, and indicators use gold. Decision ballots use red. When decisions and owner checks are both waiting, show their counts separately so each keeps its own color.

## Pull requests

External GitHub pull requests are not triage queue items. Collaborator delivery work uses Tower cards, not GitHub Issues.

## Skill wording

When a skill says “publish to the issue tracker,” create a Tower card, normally with `--add-tag ready-for-agent` when the work is fully specified. When it says “fetch the relevant ticket,” run `tower card show '#N' --json` and read linked decisions or questions from `tower brief '#N' --no-claim --json`.

## Wayfinder map

`/wayfinder` uses one map card and child tickets:

- **Map:** tag the card `wayfinder:map`; its body holds Destination, Notes, Decisions-so-far, Fog, and Out of scope.
- **Child:** set `parentId` to the map card's ID and tag it `wayfinder:research`, `wayfinder:prototype`, `wayfinder:grilling`, or `wayfinder:task`.
- **Blocking:** use Tower's native `blockedBy`; a ticket is unblocked when every blocker is `done` or its blocking decision is ratified.
- **Frontier:** list open children, drop blocked or claimed items, and choose the first remaining item in map or `workOrder` order.
- **Claim:** `tower brief '#N' --agent <me>` is the session's first write.
- **Resolve:** log the answer, set `--phase done`, then append a one-line gist and link to the map's Decisions-so-far.
