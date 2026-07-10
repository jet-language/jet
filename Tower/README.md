# Tower

A file-backed project board for one human **owner** + any number of AI
**agents**, for any project. Node ≥ 18, zero dependencies, no build step.

The owner does two things: **decide** (ratify decision ballots in a focused,
keyboard-driven UI) and **greenlight** (activate new cards). Agents do
everything else through a CLI/HTTP API — plan, implement, verify, answer
questions, raise new decisions — and every card always computes to exactly
one **lane** that says who owns the next move. Decision state is derived on
every read, so a card and its ballots can never desync.

## Install into a project

**As a Claude Code plugin** — add this directory (or its repo) as a plugin;
the `tower` skill teaches Claude the whole workflow automatically.

**Vendored** — copy or submodule this directory into the repo (any location;
`Tower/` at the root is conventional). Non-Claude agents follow `AGENTS.md`.

Then, in the host project root:

```
node Tower/tower.mjs init --name "My Project"
node Tower/tower.mjs serve --open        # board at http://localhost:7878
```

`init` creates `.tower/` in the host project: `tower.json` (all state),
`config.json` (terminology + taxonomies), `backups/` (rolling, automatic).
Commit `.tower/` to share the board with the team; `backups/` is gitignored.

Migrating from a v3-era board: `node Tower/tower.mjs import old-tower.json --name "My Project"`.

## Model

- **Epochs** — the major groupings of work (`epoch add/update/current`).
- **Milestones** — goals within an epoch; cards link to one and milestone
  progress is computed from their done-ratio (`milestone add/update`).
- **Cards** — the work. Stages: triage → deciding → planning → ready →
  building → verify → done (+ frozen). Fields include `workOrder` (canonical
  pick order), `blockedBy`, `assignee` (claims), `plan`, `log`, `refs`
  (explicit doc-path pointers, merged with auto-harvested ones in `tower brief`).
- **Exit criteria** — a card's `criteria[]` checklist (open → met → verified)
  gates `--phase done` for anyone but the owner, and the verifier must differ
  from whoever met it. Flag a card `needsAcceptance` to also require an owner
  accept/bounce ballot (auto-minted) once its checklist is clean.
- **Decisions** — ballot-ready choices attached to a card; only the owner
  ratifies. A card with an open decision surfaces as **Decide** no matter its
  stage.
- **Questions** — owner ⇄ agent threads on a card.
- **Ideas** — capture bay; promote to a card when real.
- **Events** — append-only audit trail of every mutation, with `--by` attribution.
- **History** — a done card, or a ratified decision, sits live for
  `config.retireAfterDays` (default 3) before it retires into
  `.tower/history.json` — the walk-back buffer. A card's own decisions and
  questions stay live with it until the card itself retires, so no card view
  is ever half-archived. `tower archive status|show <id>|restore <id>` reads
  the archive back and, if needed, brings something back to the live board.
  `card show`/`decision show` fall through to history automatically once
  something isn't live any more (marked `archived: true`).

## CLI

```
tower status | state | next | events
tower brief [ref] [--agent me] [--json] [--no-claim]
tower card      list|show|add|update|activate|claim|release|delete
tower decision  list|show|add|update|ratify|reopen|delete
tower question  list|ask|answer|delete
tower idea      list|add|promote|delete
tower epoch     list|add|update|current
tower milestone list|add|update|delete
tower archive   status | show <id> | restore <id>
tower init | serve | import
```

`tower brief` is the one-shot agent work packet (#462): card, live blocker
state, exit criteria, every linked decision copied verbatim, open questions,
`refs` (explicit + harvested from body/plan), recent log, and the standing
rules footer — everything needed to start a card with no other reads. No
`ref` → picks the top card the same way `next` would. `--agent` claims the
card (unless `--no-claim`); without `--agent` it's read-only.

`--json` everywhere for machine output; `--file x.json` / `--file -` (stdin)
for rich payloads; cards accept `#num` or id; `--by <name>` attributes every
write; `--expect-rev N` gives optimistic concurrency (exit 2 on conflict).

## Live + remote

- **SSE** — the UI updates over `/api/stream` the instant anything changes;
  passive updates never disturb reading, typing, or an open ballot.
- **Auth (opt-in)** — set `"auth": {"token": "…"}` in `.tower/config.json` to
  require a key from non-localhost devices (`/?key=<token>` once per device;
  localhost always exempt). Without it the board is open to your LAN/tailnet.
- **PWA + push** — installable app; the ◍ notify button subscribes the
  device to payload-less web push (new ballot / new question).
- **Undo** — every owner action shows an Undo toast (`tower undo` in the
  CLI); rev-guarded so it can never revert another agent's interleaved write.
- **Git linking** — `tower githook` installs a post-commit hook: commits
  mentioning `#12` append themselves to that card's log.
- **⌘K** — jump to any card, ballot, or view; `j/k` walk the Now queue.
- **Digest** — "since you were away" summary at the top of Now with a
  Caught-up button.
- **Recently decided** — a quiet, collapsed strip on Now lists every ratified
  decision still on the live board ("reversible for N days") with a one-tap
  Reopen — the walk-back buffer, surfaced.

## Reliability

- All writes go through the CLI/HTTP API: input validation (bad enums and
  dangling references are rejected), a cross-process lock (stale-safe),
  atomic tmp+rename writes, rolling backups, a monotonic `rev`, and an event
  log. Nothing ever hand-edits the JSON.
- The HTTP API returns structured errors (`{error, message}`, 400/404/409).
- `node --test Tower/test/` runs the suite.

## Configuration (`.tower/config.json`)

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
decision moves to `.tower/history.json`.

## UI

Black & red, pure dark, phone-friendly. Red is reserved for what needs the
**owner**; agent work reads calm, resolved goes green, done disappears. The
**beacon** on the left edge carries one lit segment per owner-blocking item
and goes dark as you clear them. Two views:

- **Now** — everything blocked on you in one queue: decisions (opens focus
  mode: ←/→ move, 1–9 pick, Enter record) and greenlights. Empty state =
  tower clear.
- **Board** — idea capture, sidequests, epochs → milestones → cards, frozen
  bay; card modal for editing, decisions, questions, log.

Durable collapse state, no localStorage, no framework, mobile bottom tabs.

## Plugin skills

Three focused skills ship with the plugin: **tower** (the work loop),
**tower-ballot** (authoring decisions the owner can decide from the ballot
alone), **tower-setup** (init, import, config, server). Non-Claude agents
use `AGENTS.md` — same protocol, plain shell.
