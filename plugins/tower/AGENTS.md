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
                              # docs/spec/** reference scan with --docs); exit 1
                              # on any finding, 0 clean
tower question list --open   # owner questions — block only affected slices
tower message list [--all]   # open card messages, or every one with --all
tower papercut list [--open] # logged tooling friction, newest first (--open filters)
tower card show '#12'        # one card, with computed lane + decisions
tower card list --tag needs-triage --json   # triage / wayfinder tag filter
tower card list --parent '#12' --json       # wayfinder map children
tower card update '#12' --add-tag ready-for-agent --by me
tower events --limit 20      # who did what, when
```

`tower brief` is the one-shot work packet. No ref picks the canonical next
card; `--agent me` takes a renewable 24-hour lease. `E_CLAIMED` means another
agent holds it. `E_CLOSE_READY` means an actively claimed card already has all
criteria met and no open gate: close, reopen, or block it before any other
brief or claim. The barrier also applies to `--no-claim` and read-only briefs.
Normal writes renew leases; expired leases do not block; done and frozen cards
clear them. Decisions in the packet are copied verbatim from the live store.

Report completions and blockers on the card itself: a `--log` entry when you
advance it, a `tower question answer` when the owner asked something. The
board (and the live SSE UI) is how the owner finds out — there is no
side channel.

Cards may carry free-form **`tags[]`** (triage roles like `needs-triage` /
`ready-for-agent`, wayfinder labels like `wayfinder:map`) and an optional
**`parentId`** (child of a wayfinder map). These are orthogonal to `phase`
— do not encode triage state in phases. See `docs/spec/contributing/issue-tracker.md`
and `docs/spec/contributing/triage-labels.md` in the host repo.

LAN note: Tower is keyless on the local network. Every LAN device can read
and change the board; do not expose the server to the public Internet.
Browser mutations require same-origin evidence; CLI mutations require the
explicit `X-Tower-Client: cli` header. Never put removed auth or push fields
in tracked `config.json`; existing ignored `secrets.json` files are not read.

Each card has a computed `lane`: `decide` (owner), `plan`/`implement`/
`building`/`verify` (displayed as Review; agent), `blocked`/`frozen`/`done` (inert).
`tower next` sorts by review > building > implement > plan, then by ascending
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
tower message add '#12' --text "..." --by me
tower message done <id> --by owner
tower papercut add --by me --text "jet-env swallowed stderr" [--card '#12']  # recurring tooling friction only (hit twice, or deterministic for everyone); never blocked by a card lane
tower papercut resolve <id> --by owner        # owner clears a handled papercut
tower decision add --file ballot.json --by me # or --file - for stdin, --draft if unfinished
tower card update '#12' --phase done --log "criteria met" --by orchestrator
tower card release '#12' --by me              # if you stop without finishing
tower card release '#12' --by me --handoff "parser done, sema left, watch X"  # required if the card is `building`
```

Phase honesty: workers return `CHECK OK` or `DOCS ONLY` and never write Tower.
After integration, the orchestrator runs the exact criterion proof, marks the
corresponding exit criteria `met`, and closes the card `done` immediately when
every row is `met` or `verified` and no blocker contradicts it. Query `done`
before briefing another card. There is no independent per-card review,
duplicate proof, broad confidence sweep, or technical verify step.

### Exit criteria gate `done`

A card requires a nonempty `criteria[]` checklist for agent closure. Each item
uses `open` → `met` → `verified`.
Add and progress it:

```
tower card criteria '#12' --add "matrix vs full spec, per feature" --by planner
tower card criteria '#12' --meet 1 --evidence "ran the matrix, 9/9" --by orchestrator
tower card criteria '#12' --verify 1 --evidence "milestone review confirmed it" --by reviewer
tower card criteria '#12' --list
```

`--phase done` by a non-owner is refused (`E_CRITERIA`) when the card has no
criteria or any row is not `met` or `verified`. The orchestrator marks rows
`met` only from integrated focused proof. When the last row settles on an
actively claimed card, `E_CLOSE_READY` blocks every further claim and brief
until that card closes, reopens, or gains a real gate. A milestone reviewer may
mark a row `verified`; that reviewer must differ from the agent that marked it
met (`E_CRITERIA_SELF`). Owner writes keep the legacy closure bypass and record
`card.criteria-bypass`. Integration and no-known-blocker are orchestration
evidence, not checklist rows.

Flag a card `needsAcceptance` **only** when the owner must judge look-and-feel
with their eyes: UI/UX/DX taste, visual presentation, copy polish, or a real
environment the harness cannot replace. **Never** for technical correctness,
tests, criteria, builds, diffs, or agent review — agents own all of that and
close with `--phase done` themselves after the criteria guard passes.

A card that needs owner acceptance must name the exact surface the owner should
look at (what to open, what “good” looks like). Do not dump machine evidence
into the owner checklist.

```
tower card update '#12' --needs-acceptance true --by owner
```

Once its criteria are all met or verified, an agent's `--phase done` attempt mints a
`D-ACCEPT-<num>` decision (accept / bounce) instead of closing — the card
sits in `verify` until the owner ratifies. Accept closes the card; bounce
reopens it to `building` with the owner's comment logged. A second done
attempt while one acceptance ballot is still open is a no-op, not a duplicate
mint.

Acceptance is not generic ratification. Only the dedicated owner-verification
buttons may accept or bounce, from a browser that has the silent HttpOnly owner
interaction session and same-origin provenance. The server binds each click to
that session and a short-lived, single-use challenge for that exact ballot and
outcome. CLI ratify, `clearance`, batch clearance, `--quote`, and caller-supplied
`by: owner` are rejected and audited.

### Milestone review

All linked cards done makes a milestone `review-ready`. Commit the frozen
source, then open the closeout token before broad proof:

```
scripts/agent/closeout-gate.mjs open <id> --by orchestrator
tower milestone criteria <id> --meet 1 --evidence "built" --by orchestrator
tower milestone criteria <id> --verify 1 --evidence "reviewed" --by reviewer
tower milestone verify <id> --evidence "milestone review complete" --by reviewer
```

`milestone closeout` records the frozen Git object ID. Broad proof scripts
check that the source tree still matches it. Milestone verify requires every
linked card `done`, every milestone criterion `verified`, and the token.
Reopening a linked card or milestone criterion clears the token and signoff.

Ballot-ready decisions carry: `gist` (one plain sentence), `lesson` (a few
plain sentences in one short paragraph that explain only the situation and
stakes), `story` (a named person, why this exists), `inWild`
(realistic code where the choice bites),
`options[]` each with plain `{key,name,detail,code}` worked examples and optional
hidden `technical` law, `comparisons[]` when relevant, `rec`, and structured
`recommendation:{why,whyNot,tradeoff}`. `whyNot` covers every losing option.
The `simple` skill applies to every user-visible field. A new full ballot
records one- or two-sentence summaries from two fresh readers: first RLI5
beginner, then adversarial. Neither reader helped author the ballot, and the
adversarial reader differs from the beginner reader. Model families may match
(owner direction, 2026-09-05). Preserve historical review records unchanged.
A short ballot is the complete base draft with no reviews and is the default
for one mechanism with at most three options. New syntax, invariant changes,
and owner-tagged full cards require the full profile. Plain prose uses one idea per sentence, defines
jargon, expands acronyms, and leads with user impact. Write-time density limits
are 32 words per sentence and 90 per paragraph. The owner decides from the ballot
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
| `done-without-evidence` | a `done` card whose log never mentions verif/green/tests/evidence AND whose criteria are empty or not all `met` or `verified` |
| `claimed-idle` | internal lease metadata remains on `building`/`ready` work untouched for more than 3 days |
| `missing-attribution` | an event (newest 500, live) with an empty/missing `by` |
| `ballot-gaps` | an OPEN, non-draft, non-`acceptance` decision that would fail `addDecision`'s own ballot-ready gate today |
| `stale-draft` | a draft decision more than 7 days old |
| `orphan-blockers` | a `blockedBy` ref that resolves to no live card, history card, or live decision |
| `blocker-unpopulated` | epoch-track `planning` card with a plan but empty `blockedBy` (and no `blockedBy: none` marker) — D-TWR-OPS2 |

`--docs` scans every text spec file under `docs/spec/**`. It reports card and
decision IDs that have no live or historical Tower record. This keeps Tower as
the decision home while the spec remains its rendered reading surface.

```
tower lint                 # human output: one line per finding, exit 1/0
tower lint --json          # machine output
tower lint --docs          # also scan docs/spec/** references
tower lint --docs-root DIR # override the docs root (default: <project>/docs)
```

## Guards (agent-hard, owner-soft)

Card closure and progression guards are agent-hard; `--by owner` may bypass
card closure and records that bypass. Milestone review requires its explicit
closeout token, criteria, command, and evidence. D-TWRGUARD1=C.

| Guard | Trigger | Error | Escape |
|---|---|---|---|
| Ballot-ready | missing required fields, profile authority, ordered full-ballot reviews, complete recommendation rationale, or plain-language density limits | `E_BALLOT` | `--draft`, rewrite, then `decision update <id> --ready` |
| Owner-only ratify | `decision ratify` by a non-owner, for a non-acceptance ballot | `E_OWNER_ONLY` | `--quote "owner's words"` |
| Owner acceptance provenance | Any generic ratify, clearance, quote, or batch attempt on `D-ACCEPT-*` | `E_ACCEPTANCE_OWNER_UI` | Owner uses the dedicated verification UI with its same-origin session and one-time challenge |
| Frozen lane | any write to a `frozen` card | `E_OWNER_LANE` | none — owner moves it out with `tower card update --phase ... --by owner` |
| Ratified-decision delete | `card delete` on a card with a ratified decision | `E_HAS_RATIFIED` | let it retire (`tower archive status`) or `tower archive restore` then re-detach — applies to owner too |
| Outcome/option match | `decision ratify --outcome K` not one of the decision's option keys | `E_INVALID` | pass a real option key |
| Building-release handoff | `card release` on a `building` card | `E_HANDOFF` | `--handoff "what's done, what's left, gotchas"` |
| Card closure | non-owner closes a card with no criteria or an unsettled criterion | `E_CRITERIA` | add criteria and mark every row `met` or `verified` |
| Closure progression | any claim or brief while an active card has all criteria settled and no open gate | `E_CLOSE_READY` | close, reopen, or block the closure-ready card |
| Milestone closeout | closeout begins before linked cards are done, or milestone verify lacks its token or verified criteria | `E_MILESTONE` | finish and close cards, freeze source, open token, run review |

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
GET  /api/messages?card=&open=0|1   durable card messages; open=1 filters
POST /api/card/add|update|claim|release|delete   (release: {handoff})
POST /api/card/criteria-add|criteria-meet|criteria-verify|criteria-reopen
POST /api/decision/add|update|delete   (add: {draft}; update: {ready})
POST /api/clearance {decisionId,outcome,comment,quote}  (generic ballots only; quote = on-behalf-of)
POST /api/acceptance/challenge {decisionId,outcome}     (owner UI, same-origin session-bound, short TTL)
POST /api/acceptance/resolve {challenge,decisionId,outcome,comment}  (single-use)
POST /api/verdict {id,outcome,title}                    (owner-only; mints a ratified decision)
POST /api/question/add|answer|delete
POST /api/message/add {cardId,text}
POST /api/message/done {id}                    (owner-only)
POST /api/papercut/add {text,cardId?}          (one-line tooling friction; no lane guard)
POST /api/papercut/resolve {id}                (owner-only)
POST /api/done/clear                           (owner-only; clears completed cards, not messages)
POST /api/idea/add|update|delete|promote
POST /api/epoch/add|update|current
POST /api/milestone/add|update|criteria-add|criteria-meet|criteria-verify|criteria-reopen|verify|delete
```

POST bodies are JSON; include `by`, optionally `expectRev`. Errors are
`{error: CODE, message}` with 400/404/409 status.
