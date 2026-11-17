# Tower — agent protocol

This file is the model-agnostic version of the Tower workflow. Any coding
agent (Claude, GPT, Gemini, local models, scripts) that can run shell
commands can drive the board with it. Plugin users get the same board
mechanics from the `tower` skill. Sibling skills: `tower-rank` (order the
queue), `tower-prep` (plans + ballots), `tower-burndown` (orchestrated
closeout). Everyone else uses the CLI below.

## What Tower is

A file-backed project board shared between one human **owner** and any number
of **agents**. State lives in `Tools/tower/.tower/tower.json` (beside the Tower app).
**Never edit that file directly** — use the CLI (or the HTTP API when
`tower serve` is running). The CLI validates input, takes a cross-process
lock, writes atomically, keeps rolling backups, bumps a revision counter, and
records an event log; hand edits do none of that.

```
node <tower-dir>/tower.mjs help        # full command surface
```

`<tower-dir>` is `Tools/tower` in this repo (or wherever the Tower plugin is
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
tower next [--agent me]      # what to pick up: the epoch chain first (active
                              # epoch, then the next), then lane and workOrder
tower next --burndown        # burndown: active epoch's epoch-track cards
                              # + every sidequest, agent lanes only (#457,
                              # D-TWR-OPS1)
tower next --ready-across-epochs  # every unblocked card board-wide —
                              # the parallel-safe set (D-TWR-OPS2)
tower chain [--json]         # epochs in order: active (3 agents), next (1
                              # agent), open cards and open criteria each, with
                              # today's change
tower closest [--epoch E] [--limit N] [--sort KEY[:asc|desc]] [--json]
                              # open cards nearest to done: fewest open
                              # criteria first, blocked counted apart
tower docs list|show|add|update|archive|delete   # durable Docs/*.md + scratchpad
                              # (archive → Docs/archive/, hidden from Docs UI)
tower lint [--json]          # durability sweeper over the live board; exit 1
                              # on any finding, 0 clean
tower lint --proofs --tree DIR  # proofs whose filter matches no Tests/Examples file
tower lint --evidence [--json]  # settled rows whose evidence cites a scratch
                              # path, a wave tree or a Rust-era command and no
                              # commit on dev/master; warnings, exit 0
tower lint --limits [--json]  # unfinished cards over criteriaSoftLimit, largest
                              # first, with their logged reason; warnings, exit 0
tower prove --card '#N'|--epoch E [--yes --jet PATH --tree DIR --commit SHA --by X] [--dry-run]
                              # run row proofs; meet the passing rows (see Proofs)
jet Tools/tower/Board.jet lint --docs [--docs-root DIR] [--json]
                              # Docs/Spec/** reference scan, read-only; exit 1
                              # on any finding, 0 clean
tower question list --open   # owner questions — block only affected slices
tower message list [--all]   # open card messages, or every one with --all
tower epoch list [--json]    # epochs in `order`, labelled E18 <name>, with exit criteria
tower card show '#12'        # one card, with computed lane + decisions
tower card list --tag needs-triage --json   # triage / wayfinder tag filter
tower card list --parent '#12' --json       # wayfinder map children
tower card list --epoch e18 --sort updated  # newest edits first (any board sort)
tower card update '#12' --add-tag ready-for-agent --by me
tower card split '#12' --criteria 5,6,7 --title "…" --by me  # move open rows
                              # onto a new card in the same epoch
tower card history '#12' [--limit N] [--json]  # everything that happened to the
                              # card, its ballots and questions, and the plans
                              # (board.apply) that touched it, newest first
tower events --limit 20      # who did what, when
```

`tower brief` is the one-shot work packet. No ref picks the canonical next
card; `--agent me` takes a renewable 24-hour lease. `E_CLAIMED` means another
agent holds it. `E_CLOSE_READY` means an actively claimed card already has all
criteria met and no open gate: close, reopen, or block it before any other
brief or claim. The barrier also applies to `--no-claim` and read-only briefs.
Normal writes renew leases; expired leases do not block; done and frozen cards
clear them. Decisions in the packet are copied verbatim from the live store.

Gates read the board in Jet, never through node: `jet Tools/tower/Board.jet`
answers `card show|list`, `decision show|list` and `milestone list` with the
JSON the CLI prints for them (`--json`, `--data`, `TOWER_DATA`, the archive
and the worktree redirect included). It only reads; writes stay on the CLI.
It decodes the store one record at a time with typed `json.decode`, keeping
the fields its answers read; a record's JSON text is kept and parsed back
only when the verb prints that record. No tree of the whole board is built:
on a 15 MB board with 65 MB of history, `card show` peaks near 56 MB and
the list verbs near 120 MB, where the whole tree took 785 MB.

Report completions and blockers on the card itself: a `--log` entry when you
advance it, a `tower question answer` when the owner asked something. The
board (and the live SSE UI) is how the owner finds out — there is no
side channel.

Post Now-page context with
`tower publish --title "…" --file report.md --by me` and
`tower status post --file snapshot.json --by me`. `publish` posts the
briefing, commits the board, pushes the board branch and exits non-zero
(`E_PUBLISH`) unless `origin` then holds that very commit;
`tower briefing post` posts without publishing. Status snapshots
contain milestone progress and workstreams, not an owner task queue.
The removed `ownerActions` field is rejected with `E_INVALID`; omit it.
Use ballots, owner verification, messages, and card closures for owner work.

Cards may carry free-form **`tags[]`** (triage roles like `needs-triage` /
`ready-for-agent`, wayfinder labels like `wayfinder:map`) and an optional
**`parentId`** (child of a wayfinder map). These are orthogonal to `phase`
— do not encode triage state in phases. See `Docs/Spec/Contributing/issue-tracker.md`
and `Docs/Spec/Contributing/triage-labels.md` in the host repo.

LAN note: Tower is keyless on the local network. Every LAN device can read
and change the board; do not expose the server to the public Internet.
Browser mutations require same-origin evidence; CLI mutations require the
explicit `X-Tower-Client: cli` header. Never put removed auth or push fields
in tracked `config.json`; existing ignored `secrets.json` files are not read.

Each card has a computed `lane`: `decide` (owner), `plan`/`implement`/
`building`/`verify` (displayed as Review; agent), `blocked`/`frozen`/`done` (inert).
`tower next` sorts by review > building > implement > plan, then by ascending
`workOrder` inside each lane. **Epochs** are the board's one planning axis,
worked in `order`; each carries `exitCriteria` that say what finishing it
means. Show an epoch as `E18` or `E18 <name>` (the stored id stays `e18`).
**Milestones** are targets inside an epoch — link cards with `--milestone <id>`
and progress computes itself.
Every card lives in an epoch or is a sidequest — frozen cards included
(frozen is a phase, not a home; the store refuses an epoch-track card with no
epoch). The board shows frozen cards in their epoch or sidequest section with a
Frozen badge; its Frozen filter shows only them.

## Writing

Always pass `--by <your-agent-name>`.

```
  tower card claim '#12' --by me                # renewable lease vs double work
tower card update '#12' --phase building --log "started X" --by me
tower card update '#12' --plan "1. ... 2. ..." --by me
tower card update '#12' --refs "Docs/Spec/foo.md,Examples/bar.jet"  # explicit doc pointers (also auto-harvested from body/plan into `tower brief`)
tower question answer <qid> --text "..." --by me
tower message add '#12' --text "..." --by me
tower message done <id> --by owner
tower epoch add e19 --name "…" --order 2 --exit-criteria "…" --exit-criteria "…" --by me
tower epoch update e19 --order 3 --by me     # or --file plan.json with {"order":3,"exitCriteria":[…]}
tower epoch remove e19 --by me                # refused while any card or milestone still points at it
tower decision add --file ballot.json --draft --by me  # or --file - for stdin; a new ballot starts drafted
tower decision review D-ID --by reviewer      # a fresh agent passes it, or --reject --reason "…"
tower decision post D-ID --by me             # a reviewed ballot enters the owner's queue
tower ballots                                # every ballot by stage, with the work waiting on it
tower decision list --open --drafts          # agent review: include drafts, hidden from owner views
tower card update '#12' --phase done --log "criteria met" --by orchestrator
tower apply --file plan.json --by me         # many edits as one all-or-nothing write; --dry-run first
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
tower card criteria '#12' --add "matrix vs full spec, per feature" --proof "jet test Compiler --profile cases --filter matrix/" --by planner
tower card criteria '#12' --meet 1 --evidence "landed 3f2a9c1e: matrix 9/9" --by orchestrator
tower card criteria '#12' --verify 1 --evidence "milestone review of 3f2a9c1e confirmed it" --by reviewer
tower card criteria '#12' --meet 2 --kind decision --evidence "D-CACHE1 ratified A: no code change" --by orchestrator
tower card criteria '#12' --move 3 --to '#40' --reason "belongs with the parser work" --by planner
tower card criteria '#12' --list
```

`--phase done` by a non-owner is refused when the card has no criteria, when
any row is not `met` or `verified` (`E_CRITERIA`), or when a settled row's
evidence no longer passes the rule below (`E_EVIDENCE`). One refusal lists
every failing row, one per line, as `cN: reason`. The orchestrator marks rows
`met` only from integrated focused proof. When the last row settles on an
actively claimed card, `E_CLOSE_READY` blocks every further claim and brief
until that card closes, reopens, or gains a real gate. A milestone reviewer may
mark a row `verified`; that reviewer must differ from the agent that marked it
met (`E_CRITERIA_SELF`). Owner writes keep the legacy closure bypass and record
`card.criteria-bypass`. Integration and no-known-blocker are orchestration
evidence, not checklist rows.

A met or verified row promises that its change is durable. Its evidence must
name a commit reachable from `dev` or `master` (`durableRefs` in config), or
the row declares a non-code closure with `--kind`: `decision` (names the
ratified `D-…` that settles it), `duplicate` (names the `#N` it duplicates or
merged into) or `obsolete` (the evidence is the reason). Evidence that cites
only scratch output (`$JET_SCRATCH_ROOT`, `$JET_STATE_ROOT`, `$JET_RUN_ROOT`,
`~/.cache`, `/tmp`) is refused. `--meet` and `--verify` apply this rule when the
row settles, so a refused meet changes nothing, and `--phase done` re-checks
every row (`E_EVIDENCE`). The guard applies when the project root is a git work
tree holding one of those branches.

A row that belongs on another card moves with `--move n --to '#M'`. Its text
becomes a new open row on #M, and the old row is met as kind `duplicate` with
evidence `Moved to #M cK`. Both rows keep the link (`movedTo` on the old row,
`movedFrom` on the new one), shown by `--list`, the briefing and the card view.
Only an open row moves, and never onto a done card.

### Card size

A card holds at most `criteriaSoftLimit` live criteria (config, default 6; a
row moved off the card does not count). A write that grows a card past it
(`--add`, `--move` onto it, `card add` or `card update` with a longer list) is
refused with `E_OVER_LIMIT` unless it carries `--over-limit "reason"`
(`overLimit` in a plan op or API payload). The reason is logged on the card as
a `card.over-limit` event. A card already past the limit is not blocked until
it grows again. The owner UI asks for the reason before adding the row.

`tower card split '#12' --criteria 5,6,7 --title "…" --by me` moves those open
rows onto a new card with the source's epoch, track, kind, priority and
milestone, body `Split from #12.`, each row moved as `--move` would move it.
The card's history logs `card.split` with both cards. `tower lint --limits`
lists the unfinished cards past the limit, each with its latest reason.

No board text names this machine's home, user, host, email, mounts or dev tool
roots (#6564 c5). Every write is checked against the rule table in
`app/personal-info.mjs`, which Tower builds from the machine it runs on. A write
that adds a match is refused (`E_PERSONAL_INFO`), and the error shows the text
to write instead, such as `$JET_SCRATCH_ROOT/lanes/X/run.log`,
`$JET_STATE_ROOT/release` or `$JET_RUN_ROOT/lander.log`.
Text already on the board does not block other writes. `tower redact` is a dry
run that counts old matches per file, per section and per rule. `tower redact
--apply --by X` rewrites them all in one write, ratified decisions included,
and logs one `board.redact` event naming the rule table. On a board with
nothing left to redact it writes nothing. This is the one change ratified
text may take, because the owner ordered it and it keeps the meaning.
`jet Tools/Agent/LaneCheck.jet --personal-info` applies the same rules to every
checked-in file, and every lane check runs it first.

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

### Proofs

A row's `proof` names the `jet test` run that settles it. `--proof "jet test
Compiler --profile P --filter F"` on `--add` or `--edit n` stores
`{profile, filter}` for the Compiler profiles `cases`, `ui`, `golden`, `core`
and `packages`; any other `jet test …` command is stored as `{profile: 'cmd',
args}`. `--proof ""` clears it, and a proof change keeps the row's status. A row
whose text ends in ``Proof: `jet test …`.`` has that sentence moved into the
field when the board loads.

```
tower prove --card '#12'                     # the plan: runs and the rows each covers
tower prove --epoch e18 --yes --jet PATH --tree DIR --commit SHA --by orchestrator
tower prove --card '#12' --yes --dry-run --jet PATH --tree DIR --commit SHA
tower lint --proofs --tree DIR               # proofs whose filter matches no test file
```

`tower prove` takes the open, unblocked rows with a proof on the card, or on every unfinished
card of the epoch. Without `--yes` it prints the plan and runs nothing. With
`--yes`, it runs `jet test` once per corpus profile (`cases`, `ui`, `golden`):
it lists the profile's cases, keeps the ids that contain any row's filter, and runs
them all in one `--cases FILE` run. `core`, `packages` and `cmd` rows run once per
distinct command. A row passes when every result row for its filter passes; a
filter that selects no case fails. Each passing row is met with evidence
`<commit>: <proof command> PASS n/n`, all in one board write; `--dry-run` runs and
reports but meets nothing. `--commit` must be on `dev` or `master`, and `--tree`,
when it is a git work tree, must be a clean checkout of that commit. The exit
code is 1 when any row failed.

`tower lint --proofs --tree DIR` lists every open row on an unfinished card
whose proof filter is not a case-insensitive substring of any file path under
`DIR/Tests` or `DIR/Examples` (rule `proof-unmatched`); exit 1 on a finding.

`tower lint --evidence` lists every met or verified row, on a card or a
milestone, whose evidence cites a scratch path (`/tmp/…`, `/mnt/…`,
`~/.cache/…`, `$JET_SCRATCH_ROOT/…`), a wave tree (`wave 12 tree`) or a Rust-era command (`cargo
test`, `rustc`), unless the same evidence also names a commit on a durable
branch (rule `evidence-provenance`). The findings are warnings: old evidence
stays readable, so the exit code is 0.

### Blocked criteria

A row can wait on another card (`#M`), another card's criterion (`#M cK`) or
a decision (`D-ID`). `--blocked-by REF` on `--add` or `--edit n` records the
wait; it is refused when the card is done or archived, the criterion or
decision is already settled, the row is on that card itself, or the wait would
close a cycle of criteria. The wait ends on its own: when the card is done, the
criterion is met or verified, or the decision leaves `open`, the next board
load moves the ref to `unblockedFrom` and the row reads `unblocked: REF
resolved`. `--unblock n` ends a wait by hand (`--reason` is logged).

```
tower card criteria '#12' --add "wire the cache" --blocked-by '#40 c2' --by planner
tower card criteria '#12' --edit 3 --blocked-by D-CACHE1 --by planner
tower card criteria '#12' --unblock 3 --reason "cache went another way" --by planner
```

A blocked row stays open but is not workable: `tower prove` skips it, and
`tower closest` counts it apart from the open rows.

### The epoch chain

Epoch `order` is the plan. The first epoch in order that is not `done` or
`arrived` is the chain's active epoch and gets three agents; the one after it
is next and gets one. `tower next` picks cards of the active epoch first, then
the next epoch's, and so on, each by lane then `workOrder`; `--burndown` scopes
to the chain's active epoch. `tower chain` lists every epoch in order with its
role, staffing, open cards and open criteria, headed by the active epoch's
open-criteria count and the next epoch. `tower brief` names the card's epoch
role, and the board's epoch headers show it.

Each row also carries today's change in open cards and open criteria
(`openCardsDelta`, `openCriteriaDelta`). The first write of each UTC day
records the board's open tally in `meta.dayStart`; today's change is the board
now against it, and a board not written today has changed by 0. `tower status`
prints the board-wide line (`OPEN TODAY`), its JSON carries it as
`counts.today`, and the UI header shows open cards and open criteria with the
same change.

`tower closest [--epoch E] [--limit N]` ranks the open (not done or frozen)
cards that have criteria by open rows, fewest first, then blocked rows, then
chain order; a card with no open or blocked rows left reads `ready`. The UI's
Closest view shows the same list with the chain above it.

`--sort KEY[:asc|desc]` on `tower closest` and `tower card list` orders cards
by one of the board's sort keys: `workflow`, `workOrder`, `priority`, `open`
(workable open criteria), `updated`, `created`, `num`, `title`, `milestone`,
`lane`. Each key has its usual direction (`updated` and `created` newest
first); `:asc` or `:desc` overrides it. The UI offers the same keys on the
board and the Closest view, and number, status or text for a card's criteria;
each choice is kept per browser.

### Milestones

A milestone is `open`, `met` or `retired`. Milestones are worked in `order`
within their epoch (`milestone add|update --order N`; unordered ones come
last), and `milestone list` shows them in epoch order, then milestone order.
Their checklist is `exitCriteria` (`--exit-criteria "…"`, repeatable), shaped
like a card's criteria: `tower milestone criteria <id>` takes the card flags
(`--add`, `--edit`, `--remove`, `--meet`, `--verify` with `--kind`,
`--reopen`), and a met or verified row passes the same durable-evidence gate
as a card row.

`tower milestone update <id> --epoch E` moves the milestone and its live
linked cards to epoch E. `tower milestone retire <id> --reason "…" --by X`
keeps the milestone for the record: its cards keep their link, but it leaves
`milestone list` (`--retired` shows it), the radar and briefs, takes no new
card links or criterion writes, and the board folds it under its epoch as
**Retired (N)**.

### Milestone review

All linked cards done makes a milestone ready for review
(`progress.reviewReady`); its status stays `open`. Commit the frozen
source, then record the closeout token and check it before broad proof:

```
tower milestone closeout <id> --commit "$(git rev-parse HEAD)" --by orchestrator
jet Tools/Agent/CloseoutGate.jet check <id>
tower milestone criteria <id> --meet 1 --evidence "built" --by orchestrator
tower milestone criteria <id> --verify 1 --evidence "reviewed" --by reviewer
tower milestone verify <id> --evidence "milestone review complete" --by reviewer
```

`milestone closeout` records the frozen Git object ID. Broad proof scripts
check that the source tree still matches it. Milestone verify requires every
linked card `done`, every exit criterion `verified`, and the token.
Reopening a linked card or milestone criterion clears the token and signoff.

Ballot-ready decisions carry: `situation` (the headline summary every ballot
view shows first: 4–7 plain sentences on what the thing is, what goes wrong
today with one concrete example, who it matters to, what is being decided, and
the recommendation; 300–1200 characters of prose, no lists, headings, or code;
required for full ballots), `gist` (one plain sentence), `lesson` (a few
plain sentences in one short paragraph that explain only the situation and
stakes), `story` (a named person, why this exists), `inWild`
(realistic code where the choice bites),
`options[]` each with plain `{key,name,detail,code}` worked examples and optional
hidden `technical` law, `comparisons[]` when relevant, `rec`, and structured
`recommendation:{why,gains,losses[{loss,mitigation}],whyNot,tradeoff}`.
Remove recommended downsides or reduce them to negligible costs with a concrete
plain-prose `mitigation` for each (under 24 words on the surface).
`whyNot` covers every losing option.
The `simple` skill applies to every user-visible field. A new full ballot
records a one- or two-sentence summary from one fresh adversarial reader who
did not help author the ballot (owner direction, 2026-10-28). Model families
may match (owner direction, 2026-09-05). Preserve historical review records
unchanged.
A short ballot is the complete base draft with no reviews and is the default
for one mechanism with at most three options. New syntax, invariant changes,
and owner-tagged full cards require the full profile. Plain prose uses one idea per sentence, defines
jargon, expands acronyms, and leads with user impact. Write-time density limits
are 32 words per sentence and 90 per paragraph (`situation` takes only the
sentence limit). The owner decides from the ballot
alone — if they'd need to ask you something to decide, it isn't ready.

### Archive (#461) — history is separate from live

A done card, or a ratified decision, sits live for `config.retireAfterDays`
(default 3) — a walk-back buffer — before it retires into
`Tools/tower/.tower/history.json`. A card's own decisions and questions stay live with
it until the card itself retires, so no card view is ever half-archived; a
still-active (non-`done`) card keeps its ratified decisions live no matter
how old. Nothing about this needs an explicit command — it happens inside
every write (`store.mutate`'s retire pass).

```
tower archive status                # counts + sizes of the archive
tower archive show <id>             # an archived card or decision
tower archive restore <id> --by owner   # bring one back to the live board
tower archive shard [--apply --by X]    # move the archive into shards (dry run without --apply)
```

GitHub warns about a file over 50 MB and refuses one over 100 MB, so a large
archive is sharded (#6564 c7). `tower archive shard --apply` turns
`history.json` into a small manifest and moves the records into
`history/<kind>-NNN.json` files, each at most 8 MiB. Tower reads the shards
back as one history; every reader and writer goes through
`app/history-shards.mjs`, never the files directly. The manifest records each
shard's item count, size and sha256, and a shard that does not match is
refused (`E_INVALID`), never read as part of a history. A shard can be
emptied but is never removed, and only the shards whose records changed are
rewritten. Backups and the repair journal hold the archive's single-file
text. A Tower older than the cutover refuses the sharded `history.json`
(`history store.decisions must be an array`) rather than reading it as empty.

`card show '#N'` / `decision show <id>` fall through to history
automatically once something isn't live any more (the result carries
`archived: true`). `card delete` still refuses while a ratified decision is
LIVE on the card (`E_HAS_RATIFIED`) — let it retire on its own, or
`tower archive restore` then re-detach, then delete. The one override is the
owner's own Tower UI: the card's Delete button lists every ballot that goes
with the card and needs a separate tick per ratified decision; the event log
records each deleted ballot. Open ballots are always deleted with their card.

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

`jet Tools/tower/Board.jet lint --docs` scans every text spec file under
`Docs/Spec/**`. It reports card and decision IDs that have no live or
historical Tower record (`spec-card-ref-missing`, `spec-decision-ref-missing`).
This keeps Tower as the decision home while the spec remains its rendered
reading surface. It reads the board read-only and needs no node.

```
tower lint                 # human output: one line per finding, exit 1/0
tower lint --json          # machine output
jet Tools/tower/Board.jet lint --docs          # scan Docs/Spec/** references
jet Tools/tower/Board.jet lint --docs --json   # the same findings as JSON
jet Tools/tower/Board.jet lint --docs --docs-root DIR  # override the docs root (default: <project>/Docs)
```

### Ballot stages

A ballot's stage is derived from its stored `draft`, `reviewedBy` and
`status` fields:

| Stage | Meaning | Next step |
|---|---|---|
| `drafted` | added with `--draft`, or edited since its last review | a fresh agent runs `decision review D-ID --by <reviewer>` |
| `reviewed` | `reviewedBy` names an agent other than its author (`draftedBy`) | `decision post D-ID --by me` runs the ballot gate and posts it |
| `posted` | in the owner's queue; blocks its card | the owner ratifies it |
| `ratified` | decided | — |
| `rejected` | the reviewer sent it back with `--reject --reason "…"` | the author revises it with `decision update`, which returns it to drafted |

`decision post` refuses any stage but `reviewed`. Any content edit to a
reviewed or rejected draft clears its review. `decision add` without
`--draft` posts at once only with `--reviewed-by <reviewer>` and a complete
ballot. A criterion waiting on a ballot (`--blocked-by D-ID`) waits until it
is ratified; rejection does not end the wait. `tower ballots [--json]` lists
each stage with the cards (`blockedBy`) and open criteria waiting on each
ballot; ratified ballots are counted and listed only inside the retire
window.

## Guards (agent-hard, owner-soft)

Card closure and progression guards are agent-hard; `--by owner` may bypass
card closure and records that bypass. Milestone review requires its explicit
closeout token, criteria, command, and evidence. D-TWRGUARD1=C.

| Guard | Trigger | Error | Escape |
|---|---|---|---|
| Ballot-ready | missing required fields, profile authority, ordered full-ballot reviews, complete recommendation rationale, or plain-language density limits | `E_BALLOT` | `--draft`, rewrite, review, then `decision post <id>` |
| Ballot stage | `decision post` of a ballot not `reviewed`, `decision add` without `--draft` or `--reviewed-by`, or a review by the ballot's own author | `E_STAGE` | `decision review <id> --by <another agent>`, then `decision post <id>` |
| Draft verdict | Ratification (even owner/quoted) or a verdict superseding a draft | `E_DRAFT` | Review and post the ballot first; no owner bypass |
| Owner-only ratify | `decision ratify` by a non-owner, for a non-acceptance ballot | `E_OWNER_ONLY` | `--quote "owner's words"` |
| Owner acceptance provenance | Any generic ratify, clearance, quote, or batch attempt on `D-ACCEPT-*` | `E_ACCEPTANCE_OWNER_UI` | Owner uses the dedicated verification UI with its same-origin session and one-time challenge |
| Frozen lane | any write to a `frozen` card | `E_OWNER_LANE` | none — owner moves it out with `tower card update --phase ... --by owner` |
| Ratified-decision delete | `card delete` on a card with a ratified decision | `E_HAS_RATIFIED` | let it retire (`tower archive status`) or `tower archive restore` then re-detach — CLI `--by owner` too; only the owner's Tower UI Delete dialog (same-origin owner session, one tick per ratified decision) deletes them with the card |
| Outcome/option match | `decision ratify --outcome K` not one of the decision's option keys | `E_INVALID` | pass a real option key |
| Building-release handoff | `card release` on a `building` card | `E_HANDOFF` | `--handoff "what's done, what's left, gotchas"` |
| Card closure | non-owner closes a card with no criteria or an unsettled criterion | `E_CRITERIA` | add criteria and mark every row `met` or `verified` |
| Durable evidence | non-owner meets, verifies or closes with a row whose evidence names no commit on `dev`/`master` or cites only scratch output | `E_EVIDENCE` | land the change and cite its commit, or `--kind` `decision`, `duplicate` or `obsolete` with the decision, card or reason |
| Personal information | any write that adds this machine's home path, user or host name, git email or name, a mount such as `$JET_SCRATCH_ROOT`, or a tool root such as `$JET_STATE_ROOT` or `$JET_RUN_ROOT` | `E_PERSONAL_INFO` | write the neutral text the error shows (`~`, `$JET_SCRATCH_ROOT`, `$JET_STATE_ROOT`, `$JET_RUN_ROOT`, `user`, `devhost`); clean old text with `tower redact --apply --by X` |
| Closure progression | any claim or brief while an active card has all criteria settled and no open gate | `E_CLOSE_READY` | close, reopen, or block the closure-ready card |
| Milestone closeout | closeout begins before linked cards are done, or milestone verify lacks its token or verified criteria | `E_MILESTONE` | finish and close cards, freeze source, open token, run review |

`tower verdict '#N' --outcome "..." --supersedes D-ID [--title "…"] --by owner`
records an owner ruling as an already-ratified decision (never a mere log note)
and is owner-only with no `--quote` escape — it IS the owner speaking. The
superseded ballot must not be a draft. Drafts remain hidden from every owner
ballot list, card panel, count, and Now view until they are posted.

Ratifying a `group: "syntax"` decision auto-appends the standard
post-ratification chores to the card's `criteria[]`: Data/Syntax registry
entry updated, syntax-decisions.md log entry, editor grammars regenerated
(`jet self devtools grammars`), snapshots re-blessed.

`blockedBy` accepts a card ref or a decision id; an unratified decision id
blocks the same as an unfinished card.

## Concurrency

- Writes are serialized by a lock and applied atomically; concurrent agents
  are safe. Every write is field-level: it re-reads the board under the lock
  and changes only the fields it names, so an unrelated agent write never
  rejects or overwrites it. `tower serve` waits for the lock without
  blocking its readers.
- For read-modify-write races on the same field: pass `--expect-rev N` (from
  `tower state`'s `meta.rev`). Exit code 2 = conflict → re-read, retry.
- `tower card claim` prevents two agents double-working a card while its
  24-hour renewable lease is active. An expired lease is never a blocker.

## Committing the board (#6563 c4)

The board is durable only once git has it. Every `config.boardBatch`-th write
(default 25) commits `tower.json` and `history.json`, with the shards under
`history/` once the archive is sharded, and nothing else, to
`config.boardBranch` (default `master`) while the write lock still holds the
pair. A shard git does not track yet is added by the commit that first holds
it. The owner's `scratch/owner-scratch.md` is never committed. A board git
does not track, or a checkout on another branch, is left alone. A failed
commit never fails the write; the next batch retries.

```
tower board commit --by <agent>   # end of your card-operation batch: commit now
tower board check                 # exit 1 (E_BOARD_STALE) when the committed
                                  # board trails the working copy by more than
                                  # one batch
tower publish --title "…" --file report.md --by <agent>
                                  # post a briefing, commit, push the board
                                  # branch and prove origin holds the commit
```

## Plans: many edits in one write

`tower apply --file plan.json --by me` (or `--file -` / `--stdin`) runs a plan
as one write: one revision and one backup, so one `tower undo` reverts the
whole plan, and one `board.apply` event summarizes it beside each op's own
event. Every op's shape is checked first and all problems are listed at once;
then the ops run in order, and the first refused op (`op N (…): reason`)
leaves the board untouched. `--dry-run` runs the plan on a copy and writes
nothing. A plan is `{ "ops": [ … ] }`; each op runs the same checks as its
CLI verb:

| `op` | Needs | May carry |
|---|---|---|
| `meet` | `card`, `n`, `evidence` | `kind` |
| `edit` | `card`, `n`, and one of `text`, `proof`, `blockedBy` | `reason` (needed with `text`) |
| `add` | `card`, `text` | `proof`, `blockedBy`, `overLimit` |
| `remove` | `card`, `n`, `reason` | — |
| `move` | `card`, `n`, `to` | `reason`, `overLimit` |
| `card.split` | `card`, `criteria` (`[5, 6]` or `"5,6"`), `title` | `overLimit` |
| `card.update` | `card` and a field | stored card fields (`phase`, `epoch`, `milestoneId`, `tags`, `blockedBy`, …), `addTags`, `removeTags`, `logEntry`, `overLimit` |
| `epoch.update` | `id` and a field | `name`, `goal`, `status`, `order`, `exitCriteria` |
| `milestone.add` | `epochId`, `title` | `id`, `goal`, `order`, `exitCriteria` |
| `milestone.update` | `id` and a field | `title`, `goal`, `order`, `exitCriteria`, `epochId` (moves its live cards too) |
| `milestone.retire` | `id`, `reason` | — |

## Gauntlet standings (#6834)

The Gauntlet page shows one run and nothing older: the newest full-matrix
results file of `Tools/Gauntlet/Harness/run.mjs` (a whole run, or a
`--languages … --merge` run). Partial runs are never shown. The page names the
run: its id, date and commit (the harness records `provenance.commit`; an older
file says "commit not recorded"), and for a merged run the run it merged into.

The page opens with a grid: one line per comparison entry, in two column
groups. Compiled peers (Rust, C, Zig, Go, Vite) and the asm ceiling ship a
built program, so they are judged against the binary `jet build` writes; the
first group is that Jet's column and theirs. Interpreted peers (Python, Node,
Bun, Lua, LuaJIT) start from source, so they are judged against `jet run`
(compile and run in one step, no binary written); the second group is that
Jet's column and theirs. Every peer cell is coloured by how its group's Jet
does against it. The standings section names each peer's Jet in `jet_modes`
(the interpreter set is `STANDINGS_INTERPRETERS` in `run.mjs`). The harness
records the `jet run` tier as the table row `jet-run`, beside the built
binary's `jet` row; `jet run` has no size.
A cell is measured (with its verdict tally once Jet is measured), not measured
yet, uncovered (a peer with no program, with the missing path; the asm ceiling
with no reference reads "uncovered: no ceiling reference"), not applicable, or
failed, mismatched or unavailable with its reason. A results file written before
a peer joined `rails.standings` keeps that column and says so.

Until a full run exists the page shows the planned grid instead: every entry
and column from this checkout's entry manifests, the standings policy and the
asm-parity references, each cell not measured yet or the manifest's reason it
cannot be.

Below the grid of a run, each entry gets one bar chart per metric (startup,
run wall, run cycles, peak RSS, size). Every measured peer value is a bar Jet
must beat, the hand-written asm ceiling is the marked bar, and Jet is a bar
once a run measured it. Until then each chart says "Jet pending" and no ratio or
verdict is shown. Once Jet is measured, each peer bar carries Jet/peer with the
peer-floor verdict, and the ceiling bar carries the gap. Rows that are
uncovered, not applicable, unavailable, mismatched or failed are listed under
the charts with their reason. A measured peer with no value for one metric is
listed in that chart with its reason. The page and the text table state the
size basis from the section's `size_basis` (static stripped, else stripped plus
every shared library including libc; web artifact; bundle; script+runtime),
and each size bar names its basis.

The page shows the gauntlet table, `$JET_STATE_ROOT/gauntlet/table.json`
(`Tools/Gauntlet/Harness/table.mjs`): the one durable record of every
measurement, one row per entry and language. The harness writes each entry's
rows into it as soon as that entry is measured, from a run in any worktree,
and reuses a row whose program and toolchain are unchanged instead of
measuring it again (`--remeasure` forces it). Cells the table has no row for
read "not measured yet". A measured cell shows its values at a glance and is
grey until Jet is measured, then green or red by whether Jet beats it; an asm
ceiling a peer beats is flagged red. Clicking a cell opens its detail against
the Jet that judges it, on every metric, and each chart shows a bar for each
Jet mode. The summary pills and the text table give the one judgement. The
page refetches the table every 20 s.
`node Tools/Gauntlet/Harness/table.mjs import
RESULTS.json` adds an earlier run report's rows. Reload the page to see new rows.

```
tower gauntlet standings [--table PATH] [--json]   # same table as text
```

## CLI contract

- `--json` on every verb prints exactly one JSON document on stdout (lists are
  one array). A failure prints `{"error":{"code":"E_…","message":"…"}}` on
  stdout, `tower: message` on stderr, and exits 1 (2 on `E_CONFLICT`); an
  unexpected failure is `E_INTERNAL`. `serve --json` prints one
  `{url, port, project, data}` line once it listens.
- A mistyped command, verb or flag names the nearest real one (`unknown flag
  for \`tower card add\`: --titel (did you mean --title?)`). A missing argument
  or required flag is named (`tower card log needs REF and --text "…"`); a
  flag given with no value or blank text counts as missing.
- `--by` attributes every write, including `epoch add|update|current` and
  `undo` (logged as `epoch.update`, `epoch.current` and `board.undo`).
- `tower help` shows each command with its argument; `tower help --json` lists
  every command path with `arg`, `required` and `flags`; `tower help --check`
  holds the help text to the parser.

## HTTP API (when `tower serve` is up, default :7878)

```
GET  /api/state                     board index (card summaries; ETag/304)
GET  /api/card?id=&live=0|1         one full card; live=1 answers from memory
GET  /api/card/history?id=          the card's events, newest first (card history)
GET  /api/stream?rev=&boot=         SSE: `state` (full index), then `delta`
                                     patches per change; a client already at
                                     rev gets `hello` and only later deltas
GET  /api/next?agent=me&limit=5     canonical work picker
GET  /api/lint                     durability sweeper findings (#457)
GET  /api/brief?card=&agent=&claim=0|1   one-shot work packet (#462); no
                                     card= → picks the top card via next's
                                     picker; claims only when agent= AND
                                     claim=1 are both given
GET  /api/events?limit=50           audit trail
GET  /api/gauntlet/standings        the one run shown: newest full or merged
                                     results file, else the planned grid
GET  /api/messages?card=&open=0|1   durable card messages; open=1 filters
POST /api/card/add|update|claim|release|delete   (release: {handoff}; add/update: {overLimit})
POST /api/card/split {id, criteria, title, overLimit}
POST /api/card/criteria-add|criteria-edit|criteria-unblock|criteria-remove|criteria-move|criteria-meet|criteria-verify|criteria-reopen   (add/edit: {proof, blockedBy}; add/move: {overLimit}; unblock: {n, reason}; move: {n, to, reason})
POST /api/decision/add|update|delete   (add: {draft, reviewedBy})
POST /api/decision/review {id, reject, reason}       (by must not be the ballot's author)
POST /api/decision/post {id}                         (stage reviewed only; runs the ballot gate)
POST /api/clearance {decisionId,outcome,comment,quote}  (generic ballots only; quote = on-behalf-of)
POST /api/acceptance/challenge {decisionId,outcome}     (owner UI, same-origin session-bound, short TTL)
POST /api/acceptance/resolve {challenge,decisionId,outcome,comment}  (single-use)
POST /api/verdict {id,outcome,title,supersedes}         (owner-only; superseded ballot must not be draft)
POST /api/question/add|answer|delete
POST /api/message/add {cardId,text}
POST /api/message/done {id}                    (owner-only)
POST /api/done/clear                           (owner-only; clears completed cards, not messages)
POST /api/idea/add|update|delete|promote
POST /api/epoch/add|update|current|remove      (add/update: {order, exitCriteria})
POST /api/milestone/add|update|retire|criteria-add|criteria-edit|criteria-remove|criteria-meet|criteria-verify|criteria-reopen|verify|delete
```

POST bodies are JSON; include `by`, optionally `expectRev`. HTTP errors are
`{error: CODE, message}` with 400/404/409 status.
