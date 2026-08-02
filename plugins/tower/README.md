# Tower

A file-backed project board for one human **owner** + any number of AI
**agents**, for any project. Node ≥ 18, zero dependencies, no build step.

The owner does one thing: **decide** (ratify decision ballots in a focused,
keyboard-driven UI). There is no greenlight/activate gate — a new card lands
straight in an agent lane. Agents do everything else through a CLI/HTTP API —
plan, implement, verify, answer questions, raise new decisions — and every
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
automatic), and ignore rules for `secrets.json` plus crash-residue
`.secrets.json.tmp-*` files. Commit `plugins/tower/.tower/` to share the board with the
team; backups and both secret-file forms are gitignored.

Migrating from a v3-era board: `node <tower-dir>/tower.mjs import old-tower.json --name "My Project"`.

## Model

- **Epochs** — the major groupings of work (`epoch add/update/current`).
- **Milestones** — goals within an epoch; cards link to one and milestone
  progress is computed from their done-ratio (`milestone add/update`).
- **Cards** — the work. Stages: deciding → planning → ready → building →
  verify → done (+ frozen). A fresh card lands in `planning` — no owner
  greenlight step. Tower picks verify, building, implement, then plan cards.
  Fields include `workOrder` (pick order inside each lane), `blockedBy`, an
  internal renewable work lease, `plan`, `log`, and `refs`
  (explicit doc-path pointers, merged with auto-harvested ones in `tower brief`).
- **Exit criteria** — a card's `criteria[]` checklist (open → met → verified)
  gates `--phase done` for anyone but the owner, and the verifier must differ
  from whoever met it. Flag a card `needsAcceptance` **only** for owner
  visual/UI/UX/DX taste (or a real environment eyes-only check). That mints an
  accept/bounce ballot once the checklist is clean. Never use it for technical
  correctness — agents meet, independently verify, and `--phase done` themselves.
  Bare `verify` is agent work and must not appear in the owner's Now queue.
  Acceptance is owner-UI-only: generic ratify, batch clearance, CLI
  `--by owner`, and agent quotes cannot resolve `D-ACCEPT-*`; rejected
  attempts remain in the audit log.
- **Decisions** — ballot-ready choices attached to a card; only the owner
  ratifies. Full ballots contain a complete base draft followed by
  boil-the-ocean, hybrid, cooperative, and adversarial reviews. Short ballots
  contain the same complete base draft without reviews and require an explicit
  owner request. The `simple` skill applies to every visible ballot field. A
  card with an open decision surfaces as **Decide** no matter its stage.
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
tower idea      list|add|promote|delete
tower epoch     list|add|update|current
tower milestone list|add|update|delete
tower archive   status | show <id> | restore <id>
tower init | serve | import
```

Agents can leave a durable card message with
`tower message add '#N' --text "…" --by agent-name`. The Now page keeps each
message until the owner marks it done. `tower message list` shows open
messages. `tower message done <id> --by owner` closes one message. Clearing
completed cards in the Now page does not clear messages.

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

## Live + remote

- **SSE** — the UI updates over `/api/stream` the instant anything changes;
  passive updates never disturb reading, typing, or an open ballot.
- **Auth (opt-in)** — set `"auth": {"token": "…"}` in the untracked
  `plugins/tower/.tower/secrets.json` to
  require a key from non-localhost devices (`/?key=<token>` once per device;
  localhost always exempt). Without it the board is open to your LAN/tailnet.
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

Runtime credentials belong only in ignored `plugins/tower/.tower/secrets.json`:

```json
{
  "auth": { "token": "replace-with-a-random-access-key" }
}
```

Auth token (optional) belongs only in ignored `plugins/tower/.tower/secrets.json`. Tower
never provisions push credentials — web push was removed.
If an older tracked `config.json` contains `auth` or `push`, Tower refuses to
start: remove those fields, rotate any exposed auth token, then put only auth
in `secrets.json`. Delete any leftover `push` key from secrets. Tower never
migrates committed credentials forward.

## UI

Black & red, pure dark, phone-friendly. Red is reserved for what needs the
**owner**; agent work reads calm, resolved goes green, done disappears. The
**beacon** on the left edge carries one lit segment per owner-blocking item
and goes dark as you clear them. Two views:

- **Now** — everything blocked on you in one queue: cards needing your
  verification, and decisions (opens focus mode: ←/→ move, 1–9 pick, Enter
  record). Focus Mode shows the five review summaries in order: slate base,
  violet breadth, cyan hybrid, green cooperative, and orange adversarial. The
  recommendation is blue, while reasons against alternatives are muted red.
  Labels and icons repeat every color's meaning. Empty state = tower clear.
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
