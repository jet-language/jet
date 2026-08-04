# Issue tracker: Tower

Issues and PRDs for this repo live as **Tower cards** on the in-repo board. Use the Tower CLI for all operations — never hand-edit `plugins/tower/.tower/`.

```sh
alias tower='node plugins/tower/tower.mjs'
tower help
```

Board data: `plugins/tower/.tower/`. UI: `tower serve --open`. Full board mechanics: `plugins/tower/skills/tower/SKILL.md` and `plugins/tower/AGENTS.md`.

## Conventions

- **Create a card**: `tower card add --title "..." --body "..." --kind bug|feature --add-tag needs-triage --by <me>`. Multi-line bodies: `--file payload.json` or `--file -`.
- **Read a card**: `tower card show '#N' --json` (falls through to archive once retired).
- **List cards**: `tower card list --json` with filters `--lane`, `--phase`, `--epoch`, `--track`, `--kind`, `--tag <name>`, `--untagged`, `--parent '#N'`.
- **Comment / triage notes**: `tower card update '#N' --log "..." --by <me>`. Log entries are the comment stream.
- **Apply / remove triage tags**: `tower card update '#N' --add-tag needs-info --by <me>` / `--remove-tag needs-info`.
- **Ask the reporter / owner**: `tower question ask --card '#N' --text "..." --by <me>`.
- **Log tooling friction**: `tower papercut add --by <me> --text "..." [--card '#N']` — one line for a dead-end command, broken helper, misleading doc, or stale cache; never derail the task to fix it.
- **Blockers**: `tower card update '#N' --blockedBy '#1,#2' --by <me>` (card refs or decision ids).
- **Claim**: `tower brief '#N' --agent <me>` or `tower card claim '#N' --by <me>`.
- **Close**: `tower card update '#N' --phase done --by <me>` after real verification (criteria gate may apply).
- **Wontfix**: `tower card update '#N' --add-tag wontfix --phase frozen --by owner` (or delete an unpromoted idea).

Intake that is not yet a card lives in **Ideas** (`tower idea list|add|promote`). Untagged open ideas count as unlabeled triage surface.

Card numbers (`#N`) are the stable handle — same role as a GitHub issue number.

## Pull requests as a triage surface

**PRs as a request surface: no.** _(Set to `yes` if this repo treats external PRs as feature requests; `/triage` reads this flag.)_

External GitHub PRs are not Triage queue items. Collaborator delivery work uses Tower cards, not GitHub Issues.

## When a skill says "publish to the issue tracker"

Create a Tower card (`tower card add ...`), normally with `--add-tag ready-for-agent` when the ticket is already agent-grabbable (e.g. `/to-tickets`).

## When a skill says "fetch the relevant ticket"

Run `tower card show '#N' --json` (and read linked decisions / questions from the projected card or `tower brief '#N' --no-claim --json`).

## Wayfinding operations

Used by `/wayfinder`. The **map** is a single card with **child** cards as tickets.

- **Map**: a card tagged `wayfinder:map`, body holds Destination / Notes / Decisions-so-far / Fog / Out of scope. `tower card add --title "..." --add-tag wayfinder:map --by <me>`.
- **Child ticket**: a card with `parentId` set to the map card's id (`tower card add ... --parent '#MAP' --add-tag wayfinder:research|prototype|grilling|task --by <me>`). List children with `tower card list --parent '#MAP' --json`.
- **Blocking**: Tower's native `blockedBy` — the canonical, UI-visible gate. A ticket is unblocked when every blocker is `done` (or a blocking decision is ratified).
- **Frontier query**: list the map's open children (`tower card list --parent '#MAP' --json`), drop any with an open blocker or an active claim/assignee; first in map / `workOrder` order wins.
- **Claim**: `tower brief '#N' --agent <me>` — the session's first write.
- **Resolve**: `tower card update '#N' --log "<answer>" --phase done --by <me>`, then append a one-line gist + link to the map's Decisions-so-far (`tower card update '#MAP' --body "..."` or `--log` pointer).
