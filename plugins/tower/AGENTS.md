# Tower — agent protocol

This file is the model-agnostic version of the Tower workflow. Any coding
agent (Claude, GPT, Gemini, local models, scripts) that can run shell
commands can drive the board with it. Plugin users get the same board
mechanics from the `tower` skill. Sibling skills: `tower-rank` (order the
queue), `tower-prep` (plans + ballots), `tower-burndown` (orchestrated
closeout). Everyone else uses the CLI below.

## What Tower is

A file-backed project board shared between one human **owner** and any number
of **agents**. State lives in `plugins/tower/.tower/tower.json` (beside the Tower app).
**Never edit that file directly** — use the CLI (or the HTTP API when
`tower serve` is running). The CLI validates input, takes a cross-process
lock, writes atomically, keeps rolling backups, bumps a revision counter, and
records an event log; hand edits do none of that.

```
node <tower-dir>/tower.mjs help        # full command surface
```

`<tower-dir>` is `plugins/tower` in this repo (or wherever the Tower plugin is
installed).

## The contract

- The owner does exactly one thing: **decide** (ratify decision ballots).
  There is no greenlight/activate gate — a fresh card lands straight in an
  agent lane. Everything else is agent work.
- The owner's decisions are the only allowed bottleneck: never make the owner
  write a plan, and never send the owner a plan or ballot no agent reviewed.
- Owner-only surfaces — read-only for agents: cards in the `decide` lane, and
  `frozen` cards.

## Reading the board

```
tower status                 # human summary
tower brief --agent me       # ONE call: card, blockers, criteria, decisions
                              # (verbatim), open questions, refs, log, rules —
                              # everything needed to start, no other reads;
                              # claims the card unless --no-claim (--json for
                              # machine output; a #ref picks a specific card)
tower state                  # full projected state as JSON
tower next [--agent me]      # what to pick up, in canonical order
tower next --burndown        # burndown: active epoch's epoch-track cards
                              # + every sidequest, agent lanes only (#457,
                              # D-TWR-OPS1)
tower next --ready-across-epochs  # every unblocked card board-wide —
                              # the parallel-safe set (D-TWR-OPS2)
tower docs list|show|add|update|archive|delete   # durable docs/*.md + scratchpad
                              # (archive → docs/archive/, hidden from Docs UI)
tower lint [--json] [--docs] # durability sweeper over the live board (+
                              # docs/ballots/*.md scan with --docs); exit 1
                              # on any finding, 0 clean
tower question list --open   # owner questions — answer these before building
tower card show '#12'        # one card, with computed lane + decisions
tower events --limit 20      # who did what, when
```

`tower brief` is the one-shot work packet (#462): it replaces reading
`status`/`next`/`card show`/`decision show`/`question list` separately to
start a card. No `[ref]` → picks the top card via the same picker as
`next`. `--agent me` takes a renewable 24-hour work lease (E_CLAIMED if
someone else holds an active lease; a no-op if you already do); omit
`--agent`, or pass `--no-claim`, to read without leasing. Normal card writes
by the holder renew it. Expired leases never block selection or takeover.
Done and frozen cards clear them. Decisions in the packet are copied verbatim
off the live store — never paraphrased.

Report completions and blockers on the card itself: a `--log` entry when you
advance it, a `tower question answer` when the owner asked something. The
board (and the live SSE UI) is how the owner finds out — there is no
side channel.

Auth note: localhost is exempt. Remote access reads `auth.token` from the
untracked `plugins/tower/.tower/secrets.json`; never put credentials in `config.json`.

Each card has a computed `lane`: `decide` (owner), `plan`/`implement`/
`building`/`verify` (agent), `blocked`/`frozen`/`done` (inert).
`tower next` sorts by verify > building > implement > plan, then by ascending
`workOrder` inside each lane. **Epochs** group the work; **milestones** are
goals inside an epoch — link cards with `--milestone <id>` and progress computes itself.

## Writing

Always pass `--by <your-agent-name>`.

```
  tower card claim '#12' --by me                # renewable lease vs double work
tower card update '#12' --phase building --log "started X" --by me
tower card update '#12' --plan "1. ... 2. ..." --by me
tower card update '#12' --refs "docs/spec/foo.md,examples/features/bar.jet"  # explicit doc pointers (also auto-harvested from body/plan into `tower brief`)
tower question answer <qid> --text "..." --by me
tower decision add --file ballot.json --by me # or --file - for stdin, --draft if unfinished
tower card update '#12' --phase verify --log "claiming done: tests green" --by me
tower card release '#12' --by me              # if you stop without finishing
tower card release '#12' --by me --handoff "parser done, sema left, watch X"  # required if the card is `building`
```

Phase honesty: `verify → done` only after real **agent** verification, by a
different session/agent than the one that claimed done when possible. Bare
`verify` is never an owner duty. If the board and reality disagree, fix the board.

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

Flag a card `needsAcceptance` **only** when the owner must judge look-and-feel
with their eyes: UI/UX/DX taste, visual presentation, copy polish, or a real
environment the harness cannot replace. **Never** for technical correctness,
tests, criteria, builds, diffs, or agent review — agents own all of that and
close with `--phase done` themselves after criteria are verified.

A card that needs owner acceptance must name the exact surface the owner should
look at (what to open, what “good” looks like). Do not dump machine evidence
into the owner checklist.

```
tower card update '#12' --needs-acceptance true --by owner
```

Once its criteria are all verified, an agent's `--phase done` attempt mints a
`D-ACCEPT-<num>` decision (accept / bounce) instead of closing — the card
sits in `verify` until the owner ratifies. Accept closes the card; bounce
reopens it to `building` with the owner's comment logged. A second done
attempt while one acceptance ballot is still open is a no-op, not a duplicate
mint.

Acceptance is not generic ratification. Only the dedicated owner-verification
buttons may accept or bounce, from a loopback device or a remote device that
presents the configured `auth.token` (the same trust boundary every other
remote write already uses — see Auth note above; no token configured means no
remote device can prove it's the owner, so acceptance stays loopback-only).
The server binds each click to an HttpOnly in-memory UI session and a
short-lived, single-use challenge for that exact ballot and outcome. CLI
ratify, `clearance`, batch clearance, `--quote`, and caller-supplied
`by: owner` are rejected and audited.

Ballot-ready decisions carry: `gist` (one plain sentence), `lesson` (a
zero-context mini lesson defining the concept, mechanics, vocabulary, stakes,
and one tiny example), `story` (a named person, why this exists), `inWild`
(realistic code where the choice bites),
`options[]` each with plain `{key,name,detail,code}` worked examples and optional
hidden `technical` law, `comparisons[]` when relevant, `rec`, and structured
`recommendation:{why,whyNot,tradeoff}`. `whyNot` covers every losing option.
Plain prose uses one idea per sentence, defines jargon, expands acronyms, and
leads with user impact. Write-time density limits are 32 words per sentence and
90 per paragraph. The owner decides from the ballot
alone — if they'd need to ask you something to decide, it isn't ready.

### Archive (#461) — history is separate from live

A done card, or a ratified decision, sits live for `config.retireAfterDays`
(default 3) — a walk-back buffer — before it retires into
`plugins/tower/.tower/history.json`. A card's own decisions and questions stay live with
it until the card itself retires, so no card view is ever half-archived; a
still-active (non-`done`) card keeps its ratified decisions live no matter
how old. Nothing about this needs an explicit command — it happens inside
every write (`store.mutate`'s retire pass).

```
tower archive status                # counts + sizes of history.json
tower archive show <id>             # an archived card or decision
tower archive restore <id> --by owner   # bring one back to the live board
```

`card show '#N'` / `decision show <id>` fall through to history
automatically once something isn't live any more (the result carries
`archived: true`). `card delete` still refuses while a ratified decision is
LIVE on the card (`E_HAS_RATIFIED`) — let it retire on its own, or
`tower archive restore` then re-detach, then delete.

### Lint (#457) — durability sweeper

`tower lint` is a read-only, rule-based sweep over the live board (each rule
its own function, returning `{rule, ref, msg}` findings):

| Rule | Flags |
|---|---|
| `done-without-evidence` | a `done` card whose log never mentions verif/green/tests/evidence AND whose criteria are empty or not all `verified` |
| `claimed-idle` | internal lease metadata remains on `building`/`ready` work untouched for more than 3 days |
| `missing-attribution` | an event (newest 500, live) with an empty/missing `by` |
| `ballot-gaps` | an OPEN, non-draft, non-`acceptance` decision that would fail `addDecision`'s own ballot-ready gate today |
| `stale-draft` | a draft decision more than 7 days old |
| `orphan-blockers` | a `blockedBy` ref that resolves to no live card, history card, or live decision |
| `blocker-unpopulated` | epoch-track `planning` card with a plan but empty `blockedBy` (and no `blockedBy: none` marker) — D-TWR-OPS2 |

`--docs` adds `ratified-in-open-ballot-doc`: a decision id ratified in the
live store (or history) but still listed in a `docs/ballots/*.md` file
(deliberately scoped to `docs/ballots/` only — `docs/plans/` may legitimately
reference a ratified id long after the fact).

```
tower lint                 # human output: one line per finding, exit 1/0
tower lint --json          # machine output
tower lint --docs          # also scan docs/ballots/*.md
tower lint --docs-root DIR # override the docs root (default: <project>/docs)
```

## Guards (agent-hard, owner-soft)

Every guard below binds writes where `--by` is not `owner`; `--by owner`
always bypasses (bypass event-logged). D-TWRGUARD1=C.

| Guard | Trigger | Error | Escape |
|---|---|---|---|
| Ballot-ready | missing required fields, complete recommendation rationale, or plain-language density limits | `E_BALLOT` | `--draft`, rewrite, then `decision update <id> --ready` |
| Owner-only ratify | `decision ratify` by a non-owner, for a non-acceptance ballot | `E_OWNER_ONLY` | `--quote "owner's words"` |
| Owner acceptance provenance | Any generic ratify, clearance, quote, or batch attempt on `D-ACCEPT-*` | `E_ACCEPTANCE_OWNER_UI` | Owner uses the verification UI (loopback or an `auth.token`-authenticated device) |
| Frozen lane | any write to a `frozen` card | `E_OWNER_LANE` | none — owner moves it out with `tower card update --phase ... --by owner` |
| Ratified-decision delete | `card delete` on a card with a ratified decision | `E_HAS_RATIFIED` | let it retire (`tower archive status`) or `tower archive restore` then re-detach — applies to owner too |
| Outcome/option match | `decision ratify --outcome K` not one of the decision's option keys | `E_INVALID` | pass a real option key |
| Building-release handoff | `card release` on a `building` card | `E_HANDOFF` | `--handoff "what's done, what's left, gotchas"` |

`tower verdict '#N' --outcome "..." [--title "…"] --by owner` records an
owner ruling as an already-ratified decision (never a mere log note) and is
owner-only with no `--quote` escape — it IS the owner speaking.

Ratifying a `group: "syntax"` decision auto-appends the standard
post-ratification chores to the card's `criteria[]`: Syntax.rs entry
updated, syntax-decisions.md log entry, `jet devtools grammars`
regenerated, snapshots re-blessed.

`blockedBy` accepts a card ref or a decision id; an unratified decision id
blocks the same as an unfinished card.

## Concurrency

- Writes are serialized by a lock and applied atomically; concurrent agents
  are safe.
- For read-modify-write races: pass `--expect-rev N` (from `tower state`'s
  `meta.rev`). Exit code 2 = conflict → re-read, retry.
- `tower card claim` prevents two agents double-working a card while its
  24-hour renewable lease is active. An expired lease is never a blocker.

## HTTP API (when `tower serve` is up, default :7878)

```
GET  /api/state                     full projected state
GET  /api/next?agent=me&limit=5     canonical work picker
GET  /api/lint?docs=0|1             durability sweeper findings (#457)
GET  /api/brief?card=&agent=&claim=0|1   one-shot work packet (#462); no
                                     card= → picks the top card via next's
                                     picker; claims only when agent= AND
                                     claim=1 are both given
GET  /api/events?limit=50           audit trail
POST /api/card/add|update|claim|release|delete   (release: {handoff})
POST /api/card/criteria-add {id,text}  criteria-meet {id,n,evidence}  criteria-verify {id,n,evidence}
POST /api/decision/add|update|delete   (add: {draft}; update: {ready})
POST /api/clearance {decisionId,outcome,comment,quote}  (generic ballots only; quote = on-behalf-of)
POST /api/acceptance/challenge {decisionId,outcome}     (owner UI, loopback or auth.token; session-bound, short TTL)
POST /api/acceptance/resolve {challenge,decisionId,outcome,comment}  (single-use)
POST /api/verdict {id,outcome,title}                    (owner-only; mints a ratified decision)
POST /api/question/add|answer|delete
POST /api/idea/add|update|delete|promote
POST /api/epoch/add|update|current
POST /api/milestone/add|update|delete
```

POST bodies are JSON; include `by`, optionally `expectRev`. Errors are
`{error: CODE, message}` with 400/404/409 status.
