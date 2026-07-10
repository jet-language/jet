# Tower — agent protocol

This file is the model-agnostic version of the Tower workflow. Any coding
agent (Claude, GPT, Gemini, local models, scripts) that can run shell
commands can drive the board with it. Claude Code users get the same content
as the `tower` skill; everyone else: read this, then use the CLI below.

## What Tower is

A file-backed project board shared between one human **owner** and any number
of **agents**. State lives in `.tower/tower.json` at the host project root.
**Never edit that file directly** — use the CLI (or the HTTP API when
`tower serve` is running). The CLI validates input, takes a cross-process
lock, writes atomically, keeps rolling backups, bumps a revision counter, and
records an event log; hand edits do none of that.

```
node <tower-dir>/tower.mjs help        # full command surface
```

`<tower-dir>` is wherever Tower lives: an installed Claude Code plugin, or a
`Tower/` directory vendored in the repo.

## The contract

- The owner does exactly two things: **decide** (ratify decision ballots) and
  **greenlight** (activate triaged cards). Everything else is agent work.
- The owner's decisions are the only allowed bottleneck: never make the owner
  write a plan, and never send the owner a plan or ballot no agent reviewed.
- Owner-only surfaces — read-only for agents: cards in lanes `decide` /
  `activate`, and `frozen` cards.

## Reading the board

```
tower status                 # human summary
tower state                  # full projected state as JSON
tower next [--agent me]      # what to pick up, in canonical order
tower question list --open   # owner questions — answer these before building
tower card show '#12'        # one card, with computed lane + decisions
tower events --limit 20      # who did what, when
```

Report completions and blockers on the card itself: a `--log` entry when you
advance it, a `tower question answer` when the owner asked something. The
board (and its push notifications) is how the owner finds out — there is no
side channel.

Auth note: localhost is exempt; remote CLIs read `auth.token` from
`.tower/config.json` automatically.

Each card has a computed `lane`: `decide`/`activate` (owner), `plan`/
`implement`/`building`/`verify` (agent), `blocked`/`frozen`/`done` (inert).
`tower next` sorts by `workOrder` ascending, then building > verify >
implement > plan. **Epochs** group the work; **milestones** are goals inside
an epoch — link cards with `--milestone <id>` and progress computes itself.

## Writing

Always pass `--by <your-agent-name>`.

```
tower card claim '#12' --by me                # soft lock vs other agents
tower card update '#12' --phase building --log "started X" --by me
tower card update '#12' --plan "1. ... 2. ..." --by me
tower question answer <qid> --text "..." --by me
tower decision add --file ballot.json --by me # or --file - for stdin
tower card update '#12' --phase verify --log "claiming done: tests green" --by me
tower card release '#12' --by me              # if you stop without finishing
```

Phase honesty: `verify → done` only after real verification, by a different
session/agent than the one that claimed done when possible. If the board and
reality disagree, fix the board.

### Exit criteria gate `done`

A card can carry a `criteria[]` checklist (each item `open` → `met` → `verified`).
Add and progress it:

```
tower card criteria '#12' --add "matrix vs full spec, per feature" --by planner
tower card criteria '#12' --meet 1 --evidence "ran the matrix, 9/9" --by builder
tower card criteria '#12' --verify 1 --evidence "re-ran independently" --by verifier
tower card criteria '#12' --list
```

`--phase done` on any `--by` other than `owner` is refused (`E_CRITERIA`) while
a card has criteria and any item isn't `verified`. The verifier must differ
from whoever met it (`E_CRITERIA_SELF`) — one agent cannot sign off its own
work. Cards with no criteria are unaffected (legacy behavior). `--by owner`
always bypasses both the checklist gate and the acceptance step below.

Flag a card `needsAcceptance` when you want the owner's own verdict, not just
a green checklist:

```
tower card update '#12' --needs-acceptance true --by owner
```

Once its criteria are all verified, an agent's `--phase done` attempt mints a
`D-ACCEPT-<num>` decision (accept / bounce) instead of closing — the card
sits in `verify` until the owner ratifies. Accept closes the card; bounce
reopens it to `building` with the owner's comment logged. A second done
attempt while one acceptance ballot is still open is a no-op, not a duplicate
mint.

Ballot-ready decisions carry: `gist` (one plain sentence), `story` (a named
person, why this exists), `inWild` (realistic code where the choice bites),
`options[]` each with `{key,name,detail,code}` worked examples,
`comparisons[]` when relevant, `rec` + why. The owner decides from the ballot
alone — if they'd need to ask you something to decide, it isn't ready.

## Concurrency

- Writes are serialized by a lock and applied atomically; concurrent agents
  are safe.
- For read-modify-write races: pass `--expect-rev N` (from `tower state`'s
  `meta.rev`). Exit code 2 = conflict → re-read, retry.
- `tower card claim` prevents two agents double-working a card; a claim held
  by someone else is a hard stop, pick another card.

## HTTP API (when `tower serve` is up, default :7878)

```
GET  /api/state                     full projected state
GET  /api/next?agent=me&limit=5     canonical work picker
GET  /api/events?limit=50           audit trail
POST /api/card/add|update|activate|claim|release|delete
POST /api/card/criteria-add {id,text}  criteria-meet {id,n,evidence}  criteria-verify {id,n,evidence}
POST /api/decision/add|update|delete
POST /api/clearance {decisionId,outcome,comment}       (owner ratify)
POST /api/question/add|answer|delete
POST /api/idea/add|update|delete|promote
POST /api/epoch/add|update|current
POST /api/milestone/add|update|delete
```

POST bodies are JSON; include `by`, optionally `expectRev`. Errors are
`{error: CODE, message}` with 400/404/409 status.
