# Tower

A file-backed project board for one human **owner** + any number of AI
**agents**, for any project. Node ≥ 18, zero dependencies, no build step.

The owner does one thing: **decide** (ratify decision ballots in a focused,
keyboard-driven UI). There is no greenlight/activate gate — a new card lands
straight in an agent lane. Agents do everything else through a CLI/HTTP API —
plan, implement, review, answer questions, raise new decisions — and every
card always computes to exactly one **lane** that says who owns the next
move. Decision state is derived on every read, so a card and its ballots can
never desync.

## Install into a project

**As a Cursor plugin** — this directory has `.cursor-plugin/plugin.json` and
skills under `skills/`. Cursor does not auto-load a vendored plugin from the
repo, so install it locally (real copy — external symlinks are rejected):

```
mkdir -p ~/.cursor/plugins/local
rsync -a --exclude '.tower/' --exclude 'node_modules/' \
  /path/to/Tools/Tower/ ~/.cursor/plugins/local/tower/
```

Then **Developer: Reload Window**. Skills show up as `/tower`,
`/tower-ballot`, `/tower-rank`, `/tower-prep`, `/tower-burndown`,
`/tower-setup`. In the Jet repo, project symlinks under `.cursor/skills/`
also expose those skills without a local install; always run the CLI from
the checkout (`node Tools/Tower/tower.mjs`) so the board stays in
`Tools/Tower/.tower/`.

**As a Codex plugin** — install `tower` from the repository marketplace; the
Tower skills are discovered from `.codex-plugin/plugin.json`.

**As a Claude Code plugin** — add this directory (or its repo) as a plugin;
the `tower` skill teaches Claude the board mechanics automatically.

**Vendored** — copy or submodule this directory into the repo (any location;
`Tower/` at the root is conventional). Non-Claude agents follow `AGENTS.md`.

Then, in the host project root (`<tower-dir>` is this plugin directory):

```
node <tower-dir>/tower.mjs init --name "My Project"
node <tower-dir>/tower.mjs serve --open        # board at http://localhost:7878
```

`init` creates `Tools/Tower/.tower/` beside this app: `tower.json` (all state),
`config.json` (public terminology + taxonomies), `backups/` (rolling,
automatic), and historical ignore rules for `secrets.json` plus crash-residue
`.secrets.json.tmp-*` files. Commit `Tools/Tower/.tower/` to share the board with
the team. The old credential-file patterns stay ignored so local files never become trackable.

Migrating from a v3-era board: `node <tower-dir>/tower.mjs import old-tower.json --name "My Project"`.

## Model

- **Epochs** — the board's one planning axis (`epoch list|add|update|remove|current`).
  Each epoch has a `name`, a `goal` description, a `status`, an `order`, and
  `exitCriteria` that say what finishing it means. Every listing — `epoch list`,
  the radar, the Board view, briefings, and `next` — shows epochs by `order`,
  then by id number. An epoch is labeled `E18` or `E18 <name>`; the stored id
  stays `e18`. `epoch remove` is refused while any card (live in any phase, or
  archived) or milestone still points at the epoch, and the refusal names the
  counts.
- **Milestones** — targets within an epoch, worked in `order`, each `open`,
  `met` or `retired`, with card-shaped `exitCriteria`. All linked cards done
  makes a milestone ready for review (`progress.reviewReady`).
  `milestone closeout` records the frozen source commit that authorizes broad
  proof. Only `milestone verify` can make the tokened milestone `met`.
  `milestone update --epoch E` moves it with its live cards; `milestone retire
  --reason "…"` keeps it for the record but out of lists, briefs and new links.
- **Cards** — the work. Stages: deciding → planning → ready → building →
  review → done (+ frozen). A fresh card lands in `planning` — no owner
  greenlight step. Tower picks review, building, implement, then plan cards.
  Fields include `workOrder` (pick order inside each lane), `blockedBy`, an
  internal renewable work lease, `plan`, `log`, and `refs`
  (explicit doc-path pointers, merged with auto-harvested ones in `tower brief`).
- **Exit criteria** — a card needs a nonempty `criteria[]` checklist for agent
  closure. Every row must be `met` or `verified`. Workers return `CHECK OK` or
  `DOCS ONLY` and never write Tower. After integration, the orchestrator runs
  the exact proof, marks rows `met`, and closes the card immediately. An
  actively claimed card with every row settled and no open gate blocks every
  further claim or brief until it closes, reopens, or gains a real gate.
  `verified` is milestone-review signoff, and its reviewer must differ from the
  agent that marked the row met. There is no separate card verify step. Flag a
  card `needsAcceptance` **only** for owner visual/UI/UX/DX
  taste or a real environment eyes-only check. That mints an accept/bounce
  ballot once the checklist is clean. Bare `verify` is legacy agent state and
  does not appear in the owner's Now queue. Integration and no-known-blocker
  are orchestration evidence, not mandatory rows on every card. A row meets
  only on durable evidence: a commit on `dev` or `master`, or a declared
  `--kind decision|duplicate|obsolete` with its evidence. A refused close lists
  every failing row with its reason. `card criteria --move n --to '#M'` moves
  an open row to another card and links both rows (`movedTo`/`movedFrom`).
  A card past `criteriaSoftLimit` rows (default 6) grows only with
  `--over-limit "reason"`, logged on the card; `tower card split '#N'
  --criteria 5,6 --title "…"` moves rows onto a new card instead, and
  `tower lint --limits` lists the cards past the limit.
  A row's `proof` (`--proof "jet test …"` on `--add`/`--edit`) names the test
  run that settles it; `tower prove --card|--epoch` runs the proofs at a landed
  commit and meets the passing rows, and `tower lint --proofs --tree DIR`
  lists proofs whose filter matches no test file. `--blocked-by '#M'|'#M
  cK'|D-ID` makes a row wait on other work until it resolves.
  Acceptance is owner-UI-only: generic ratify, batch clearance, CLI
  `--by owner`, and agent quotes cannot resolve `D-ACCEPT-*`; rejected
  attempts remain in the audit log.
- **Epoch chain** — epoch `order` is the plan: the first unfinished epoch is
  active (three agents), the next gets one. `tower next` follows the chain,
  `tower chain` lists it with each epoch's open cards and open criteria and
  today's change, and `tower closest` (and the UI's Closest view) ranks open
  cards by open criteria, fewest first; `--sort KEY[:asc|desc]` on `tower
  closest` and `tower card list` orders cards by any key the board sorts by
  (workflow, workOrder, priority, open, updated, created, num, title,
  milestone, lane). `tower status` and the UI header show
  the board's open cards and open criteria with today's change.
- **Decisions** — ballot-ready choices attached to a card; only the owner
  ratifies. New full ballots contain a complete base draft followed by one
  fresh-agent adversarial review; model families may match. Historical
  review records stay unchanged.
  Short ballots contain the same complete base draft without reviews and
  are the default for one mechanism with at most three options. The
  `simple` skill applies to every visible ballot field. A ballot moves through
  stages: `drafted` → `reviewed` (`tower decision review D-ID --by <reviewer>`,
  a fresh agent, or `--reject --reason "…"` to send it back as `rejected`) →
  `posted` (`tower decision post D-ID --by me`, which runs the ballot gate) →
  `ratified`. An edit to a reviewed or rejected draft returns it to `drafted`.
  `tower ballots [--json]` lists every stage with the cards and criteria
  waiting on each ballot. Unposted ballots are hidden from owner lists, card
  panels, activity, counts, and Now report links. Agents can inspect them with
  `tower decision list --open --drafts` or `tower decision show D-ID`.
  Ratification and `tower verdict --supersedes D-ID` refuse a draft with
  `E_DRAFT`. A card with an open posted decision surfaces as **Decide** no
  matter its stage.
- **Milestone review** — milestone exit criteria use the same `open` → `met` →
  `verified` flow. After all cards close, freeze the source commit and record
  its token with `tower milestone closeout <id> --commit SHA --by X`, then
  `jet Tools/Agent/CloseoutGate.jet check <id>`; broad proof is refused
  before that. `tower milestone verify <id> --evidence "…" --by X` works only
  when every linked card is done, every exit criterion is verified, and
  the token exists. Reopening a linked card or milestone criterion clears the
  token and signoff.
- **Questions** — owner ⇄ agent threads on a card.
- **Ideas** — capture bay; promote to a card when real.
- **Events** — append-only audit trail of every mutation, with `--by` attribution.
- **Now reports** — `tower publish --title "…" --file report.md --by me` posts
  a briefing, commits the board, pushes the board branch and exits non-zero
  unless `origin` then holds that commit. `tower status post --file
  snapshot.json --by me` posts the
  latest status snapshot with milestone progress, workstreams, workers,
  blockers, and optional card/decision links. Briefings and the status board
  provide context; ballots, owner verification, messages, and card closures
  remain their own authoritative surfaces. Snapshots containing the removed
  `ownerActions` field are rejected with `E_INVALID`; omit that field.
- **Plans** — `tower apply --file plan.json --by me` runs many card, criterion,
  epoch and milestone edits as one all-or-nothing write (one revision, one
  undo); `--dry-run` checks the plan and writes nothing.
- **History** — a done card, or a ratified decision, sits live for
  `config.retireAfterDays` (default 3) before it retires into
  `Tools/Tower/.tower/history.json` — the walk-back buffer. A card's own decisions and
  questions stay live with it until the card itself retires, so no card view
  is ever half-archived. `tower archive status|show <id>|restore <id>` reads
  the archive back and, if needed, brings something back to the live board.
  `card show`/`decision show` fall through to history automatically once
  something isn't live any more (marked `archived: true`). `tower archive
  shard --apply --by X` moves a large archive into `history/<kind>-NNN.json`
  shards of at most 8 MiB each, with `history.json` as their manifest, so no
  board file nears GitHub's 100 MB limit; without `--apply` it is a dry run.

## CLI

```
tower status | state | next | events | ballots
tower brief [ref] [--agent me] [--json] [--no-claim]
tower card      list|show|add|update|claim|release|delete|criteria|log|history
tower decision  list|show|add|update|review|post|ratify|reopen|delete
tower question  list|ask|answer|delete
tower message   list|add|done
tower idea      list|add|promote|delete
tower epoch     list|add|update|remove|current
tower milestone list|add|update|retire|criteria|closeout|verify|delete
tower archive   status | show <id> | restore <id>
tower apply --file plan.json [--dry-run] | publish --title "…" --file report.md
tower init | serve | import
```

Agents can leave a durable card message with
`tower message add '#N' --text "…" --by agent-name`. The Now page keeps each
message until the owner marks it done. `tower message list` shows open
messages. `tower message done <id> --by owner` closes one message. Clearing
completed cards in the Now page does not clear messages.

The **AGENTS.md** tab sits to the right of **Docs**. It reads and edits
the repository-root `AGENTS.md`, not a plugin policy file. Changing the editor
does not rewrite policy content.

The **Docs** tab groups durable files into exactly four sections: **Spec**,
**Audits**, **Research**, and **Proposals**. `Docs/README.md` remains navigation
for the repository and is not a fifth Docs section. There is no Docs archive
command; use the existing owner deletion action when a document must be
removed.

Use **Save** or **Ctrl/Cmd+S** to save. Each save includes the content revision
loaded with the draft. If the file changed, Tower rejects the save and keeps
the draft. Copy your edits before using **Cancel** or **Escape** to load the
latest file, then merge your changes into it.

Drafts survive navigation between Tower tabs and failed requests. Edits typed
while a save is pending remain unsaved after its acknowledgment. Drafts are
held only in the current page; canceling, reloading, or closing it discards them.

This is one fixed-path editor, not a general source editor. Generic Docs
routes cannot write root files. The editor rejects symlinks and hard links
and preserves read/write/execute permissions. After a server update, the owner
must restart an outdated Tower process and reload the page before using the new
editor.

## Live + remote

- **SSE** — the UI updates over `/api/stream` the instant anything changes;
  passive updates never disturb reading, typing, or an open ballot.
- **LAN access** — Tower listens on the local network. Open
  `http://<machine-hostname>:7878` or `http://<machine-ip>:7878` from another
  device. No credentials or setup key are needed. Every device on the LAN can
  read and change the board, so do not expose Tower to the public Internet.
  Browser mutations require same-origin evidence. CLI mutations must send the
  explicit `X-Tower-Client: cli` header.
- **Acceptance** — opening the board silently creates a short-lived,
  HttpOnly owner interaction session. Accept and Bounce use a one-time
  challenge tied to that session. This is not a login or an access key.
- **PWA** — installable app (offline shell). Live updates use SSE; web push
  was removed (owner D-VERDICT-460-1).
- **Undo** — every owner action shows an Undo toast (`tower undo` in the
  CLI); rev-guarded so it can never revert another agent's interleaved write.
- **Git linking** — `tower githook` installs a post-commit hook: commits
  mentioning `#12` append themselves to that card's log.
- **⌘K** — jump to any card, ballot, or view; `j/k` walk the Now queue.
- **Done and messages** — Now shows completed cards since the last clear and
  durable card-linked agent messages. Clearing completed cards does not clear
  messages. The owner closes each message with its own Done button.
- **Recently decided** — a quiet, collapsed strip on Now lists every ratified
  decision still on the live board ("reversible for N days") with a one-tap
  Reopen — the walk-back buffer, surfaced.

`tower brief` is the one-shot agent work packet (#462): card, live blocker
state, exit criteria, every linked decision copied verbatim, open questions,
`refs` (explicit + harvested from body/plan), recent log, and the standing
rules footer — everything needed to start a card with no other reads. No
`ref` → picks the top card the same way `next` would. `--agent` takes a
renewable 24-hour work lease (unless `--no-claim`); without `--agent` it is
read-only. Expired leases never block work, and owner-facing card views do
not show durable ownership markings.

`--json` on every verb prints exactly one JSON document; a failure prints
`{"error":{"code":"E_…","message":"…"}}` on stdout (and `tower: message` on
stderr) and exits non-zero. A mistyped command, verb or flag names the nearest
real one, and a missing argument or required flag is named
(`tower card log needs REF and --text "…"`). `tower help --json` lists every
command with its argument, required flags and flags. `--file x.json` /
`--file -` (stdin) carry rich payloads; cards accept `#num` or id; `--by
<name>` attributes every write; `--expect-rev N` gives optimistic concurrency
(exit 2 on conflict). `tower card history '#12'` lists everything that happened
to a card, its ballots and questions, and the plans that touched it, newest
first.

## Reliability

- All writes go through the CLI/HTTP API: input validation (bad enums and
  dangling references are rejected), a cross-process lock (stale-safe),
  atomic tmp+rename writes, rolling backups, a monotonic `rev`, and an event
  log. Nothing ever hand-edits the JSON.
- The HTTP API returns structured errors (`{error, message}`, 400/404/409).
- `node --test Test/*.test.mjs` runs the suite from the plugin root.

## Configuration (`Tools/Tower/.tower/config.json`)

```json
{
  "project": "My Project",
  "terms": { "epoch": "Season", "milestone": "Target" },
  "tracks": ["epoch", "sidequest"],
  "kinds": ["task", "feature", "idea", "bug"],
  "priorities": ["P0", "P1", "P2", "P3"],
  "decisionGroups": ["design", "api", "ui", "tooling"],
  "port": 7878,
  "backups": 20,
  "retireAfterDays": 3,
  "commitUrl": "https://github.com/OWNER/REPO/commit/{sha}",
  "criteriaSoftLimit": 6
}
```

Everything is optional; the UI and validation follow whatever you set.
`retireAfterDays` is the walk-back buffer before a done card / ratified
decision moves to the archive, `Tools/Tower/.tower/history.json` (with its
shards under `history/` once sharded). `commitUrl` makes commit SHAs in
criterion evidence, logs and questions link to that URL (`{sha}` is replaced);
without it they stay text. `#N` and `#N cM` in any text open that card (and
criterion) in the UI; the CLI prints them as written. `criteriaSoftLimit` is
the most criteria a card holds before a new row needs `--over-limit "reason"`.

There is no runtime credential file. `auth` and `push` are removed fields.
Tower rejects them in tracked `config.json` with `ConfigError`; remove those
fields from old config before starting Tower. Existing ignored `secrets.json`
files are not read and do not affect startup or access.

## UI

Black & red, pure dark, phone-friendly. Red is reserved for what needs the
**owner**; agent work reads calm, resolved goes green, done disappears. The
**beacon** on the left edge carries one lit segment per owner-blocking item
and goes dark as you clear them. Two views:

- **Now** — everything blocked on you in one queue: cards needing your visual
  review, and decisions (opens focus mode: ←/→ move, 1–9 pick, Enter
  record). Focus Mode shows the ballot's review summaries in order: the
  orange adversarial pass for current ballots, and the stored passes of
  historical ballots. The recommendation is blue, while reasons against alternatives
  are muted red. Labels and icons repeat every color's meaning. Empty state =
  tower clear.
- **Board** — idea capture, sidequests, epochs → milestones → cards (frozen
  cards stay in their epoch or sidequest section with a Frozen badge; the
  Frozen filter shows only them; done cards leave the board). Each epoch has
  one header: its E label, chain role, milestone progress bar, tallies and
  today's change in open work. The
  board's cards, the Closest view and a card's criteria each sort by a chosen
  key. Card modal for editing, decisions,
  questions, log, and delete (a card's ballots are listed and deleted with it).
- **Radar** *(prototype, owner-acceptance pending)* — roadmap ledger ×
  ops-table hybrid: per active epoch, a 30-day burndown sparkline, milestone
  progress with stall badges, and a sortable/filterable/inline-editable
  table of that epoch's active cards (+ its sidequests). Adds to Board/Now,
  changes neither.

Durable collapse state on the board; sort choices and Now report folds are
kept per browser in localStorage. No framework, mobile bottom tabs.

## Plugin skills

Focused skills ship with the plugin: **tower** (board mechanics),
**tower-ballot** (authoring decisions the owner can decide from the ballot
alone), **tower-rank** (ordered queue / `workOrder`), **tower-prep** (plans
+ ballots until ready or decide), **tower-burndown** (orchestrated card
closeout), and **tower-setup** (init, import, config, server). Non-Claude
agents use `AGENTS.md` — same protocol, plain shell.
