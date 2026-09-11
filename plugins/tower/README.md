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
  /path/to/plugins/tower/ ~/.cursor/plugins/local/tower/
```

Then **Developer: Reload Window**. Skills show up as `/tower`,
`/tower-ballot`, `/tower-rank`, `/tower-prep`, `/tower-burndown`,
`/tower-setup`. In the Jet repo, project symlinks under `.cursor/skills/`
also expose those skills without a local install; always run the CLI from
the checkout (`node plugins/tower/tower.mjs`) so the board stays in
`plugins/tower/.tower/`.

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

`init` creates `plugins/tower/.tower/` beside this app: `tower.json` (all state),
`config.json` (public terminology + taxonomies), `backups/` (rolling,
automatic), and historical ignore rules for `secrets.json` plus crash-residue
`.secrets.json.tmp-*` files. Commit `plugins/tower/.tower/` to share the board with
the team. The old credential-file patterns stay ignored so local files never become trackable.

Migrating from a v3-era board: `node <tower-dir>/tower.mjs import old-tower.json --name "My Project"`.

## Model

- **Epochs** — the major groupings of work (`epoch add/update/current`).
- **Milestones** — goals within an epoch. Linked cards move a milestone to
  `review-ready` when all are done. `milestone closeout` records the frozen
  source commit that authorizes broad proof. Only `milestone verify` can make
  the tokened milestone `met`.
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
  are orchestration evidence, not mandatory rows on every card.
  Acceptance is owner-UI-only: generic ratify, batch clearance, CLI
  `--by owner`, and agent quotes cannot resolve `D-ACCEPT-*`; rejected
  attempts remain in the audit log.
- **Decisions** — ballot-ready choices attached to a card; only the owner
  ratifies. New full ballots contain a complete base draft followed by
  fresh-agent RLI5 beginner and separate fresh-agent adversarial reviews;
  model families may match. Historical review records stay unchanged.
  Short ballots contain the same complete base draft without reviews and
  are the default for one mechanism with at most three options. The
  `simple` skill applies to every visible ballot field. A card with an open
  decision surfaces as **Decide** no matter its stage.
- **Milestone review** — milestone criteria use the same `open` → `met` →
  `verified` flow. After all cards close, freeze the source commit with
  `scripts/agent/closeout-gate.mjs open <id> --by X`; broad proof is refused
  before that. `tower milestone verify <id> --evidence "…" --by X` works only
  when every linked card is done, every milestone criterion is verified, and
  the token exists. Reopening a linked card or milestone criterion clears the
  token and signoff.
- **Questions** — owner ⇄ agent threads on a card.
- **Ideas** — capture bay; promote to a card when real.
- **Events** — append-only audit trail of every mutation, with `--by` attribution.
- **History** — a done card, or a ratified decision, sits live for
  `config.retireAfterDays` (default 3) before it retires into
  `plugins/tower/.tower/history.json` — the walk-back buffer. A card's own decisions and
  questions stay live with it until the card itself retires, so no card view
  is ever half-archived. `tower archive status|show <id>|restore <id>` reads
  the archive back and, if needed, brings something back to the live board.
  `card show`/`decision show` fall through to history automatically once
  something isn't live any more (marked `archived: true`).

## CLI

```
tower status | state | next | events
tower brief [ref] [--agent me] [--json] [--no-claim]
tower card      list|show|add|update|claim|release|delete
tower decision  list|show|add|update|ratify|reopen|delete
tower question  list|ask|answer|delete
tower message   list|add|done
tower papercut  list|add|resolve
tower idea      list|add|promote|delete
tower epoch     list|add|update|current
tower milestone list|add|update|criteria|closeout|verify|delete
tower archive   status | show <id> | restore <id>
tower init | serve | import
```

Agents can leave a durable card message with
`tower message add '#N' --text "…" --by agent-name`. The Now page keeps each
message until the owner marks it done. `tower message list` shows open
messages. `tower message done <id> --by owner` closes one message. Clearing
completed cards in the Now page does not clear messages.

Agents log one-line tooling friction (dead-end commands, broken helpers,
misleading docs, stale caches) with `tower papercut add --by agent-name --text
"…"` instead of silently pushing through. The bar is recurrence: the snag hit
twice, or it is deterministic for anyone running the same command. One-offs,
self-inflicted state, and session collisions are not papercuts, and a bug in Jet
itself is a card. It never fails on a card lane, so logging never derails the
task. The **Papercuts** tab groups them by day; the owner clears a handled one
with `tower papercut resolve <id> --by owner`.

The **AGENTS.md** tab sits to the right of **Papercuts**. It reads and edits
the repository-root `AGENTS.md`, not a plugin policy file. Changing the editor
does not rewrite policy content.

The **Docs** tab groups durable files into exactly four sections: **Spec**,
**Audits**, **Research**, and **Proposals**. `docs/README.md` remains navigation
for the repository and is not a fifth Docs section. There is no Docs archive
command; use the existing owner deletion action when a document must be
removed.

Use **Save** or **Ctrl/Cmd+S** to save. Each save includes the content revision
loaded with the draft. If the file changed, Tower rejects the save and keeps
the draft. Copy your edits before using **Cancel** or **Escape** to load the
latest file, then merge your changes into it.

Drafts survive navigation between Tower tabs and failed requests. Edits typed
while a save is pending remain unsaved after its acknowledgement. Drafts are
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

`--json` everywhere for machine output; `--file x.json` / `--file -` (stdin)
for rich payloads; cards accept `#num` or id; `--by <name>` attributes every
write; `--expect-rev N` gives optimistic concurrency (exit 2 on conflict).

## Reliability

- All writes go through the CLI/HTTP API: input validation (bad enums and
  dangling references are rejected), a cross-process lock (stale-safe),
  atomic tmp+rename writes, rolling backups, a monotonic `rev`, and an event
  log. Nothing ever hand-edits the JSON.
- The HTTP API returns structured errors (`{error, message}`, 400/404/409).
- `node --test test/*.test.mjs` runs the suite from the plugin root.

## Configuration (`plugins/tower/.tower/config.json`)

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
  "retireAfterDays": 3
}
```

Everything is optional; the UI and validation follow whatever you set.
`retireAfterDays` is the walk-back buffer before a done card / ratified
decision moves to `plugins/tower/.tower/history.json`.

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
  record). Focus Mode shows the six review summaries in order: slate base,
  violet breadth, cyan hybrid, green cooperative, blue beginner, and orange
  adversarial. The recommendation is blue, while reasons against alternatives
  are muted red. Labels and icons repeat every color's meaning. Empty state =
  tower clear.
- **Board** — idea capture, sidequests, epochs → milestones → cards, frozen
  bay; card modal for editing, decisions, questions, log.
- **Radar** *(prototype, owner-acceptance pending)* — roadmap ledger ×
  ops-table hybrid: per active epoch, a 30-day burndown sparkline, milestone
  progress with stall badges, and a sortable/filterable/inline-editable
  table of that epoch's active cards (+ its sidequests). Adds to Board/Now,
  changes neither.

Durable collapse state, no localStorage, no framework, mobile bottom tabs.

## Plugin skills

Focused skills ship with the plugin: **tower** (board mechanics),
**tower-ballot** (authoring decisions the owner can decide from the ballot
alone), **tower-rank** (ordered queue / `workOrder`), **tower-prep** (plans
+ ballots until ready or decide), **tower-burndown** (orchestrated card
closeout), and **tower-setup** (init, import, config, server). Non-Claude
agents use `AGENTS.md` — same protocol, plain shell.
