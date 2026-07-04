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
  pick order), `blockedBy`, `assignee` (claims), `plan`, `log`.
- **Decisions** — ballot-ready choices attached to a card; only the owner
  ratifies. A card with an open decision surfaces as **Decide** no matter its
  stage.
- **Questions** — owner ⇄ agent threads on a card.
- **Messages** — direct owner ⇄ agent chat. Agents stay reachable with
  `tower agent listen --name <me>` (long-poll when the server is up, file
  polling otherwise); the owner writes from the board's Agents view. With
  `config.commands` set (opt-in), an offline agent gets a **Send + run**
  button that starts a headless turn (`claude -p` / `codex exec`) and posts
  its output back into the thread.
- **Ideas** — capture bay; promote to a card when real.
- **Events** — append-only audit trail of every mutation, with `--by` attribution.

## CLI

```
tower status | state | next | events
tower card      list|show|add|update|activate|claim|release|delete
tower decision  list|show|add|update|ratify|reopen|delete
tower question  list|ask|answer|delete
tower idea      list|add|promote|delete
tower epoch     list|add|update|current
tower milestone list|add|update|delete
tower message   send|list|read
tower agents                     # roster + live presence
tower agent listen --name <me>   # long-lived message feed for an agent
tower init | serve | import
```

`--json` everywhere for machine output; `--file x.json` / `--file -` (stdin)
for rich payloads; cards accept `#num` or id; `--by <name>` attributes every
write; `--expect-rev N` gives optimistic concurrency (exit 2 on conflict).

## Live + remote

- **SSE** — the UI updates over `/api/stream` the instant anything changes;
  passive updates never disturb reading, typing, or an open ballot.
- **Auth** — non-localhost requests need the token auto-generated into
  `.tower/config.json` (`auth.token`); open `http://host:7878/?key=<token>`
  once per device (cookie persists). Localhost (agents, CLIs) is exempt.
- **PWA + push** — installable app; the ◍ notify button subscribes the
  device to payload-less web push (new ballot / agent message / verify).
- **Batched agent wake** — ratifications and greenlights within
  `notifyBatchSeconds` (default 90) collapse into ONE `[tower]` message per
  listening agent, so a ballot session doesn't spin up an agent per decision.
- **Undo** — every owner action shows an Undo toast (`tower undo` in the
  CLI); rev-guarded so it can never revert another agent's interleaved write.
- **Attachments** — 📎 in the composer (or `tower message send --attach x.png`);
  images render inline.
- **Git linking** — `tower githook` installs a post-commit hook: commits
  mentioning `#12` append themselves to that card's log.
- **⌘K** — jump to any card, ballot, or agent; `j/k` walk the Now queue.
- **Digest** — "since you were away" summary at the top of Now with a
  Caught-up button.

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
  "backups": 20
}
```

Everything is optional; the UI and validation follow whatever you set.

## UI

Black & red, pure dark, phone-friendly. Red is reserved for what needs the
**owner**; agent work reads calm, resolved goes green, done disappears. The
**beacon** on the left edge carries one lit segment per owner-blocking item
and goes dark as you clear them. Three views:

- **Now** — everything blocked on you in one queue: agent messages (inline
  reply), decisions (opens focus mode: ←/→ move, 1–9 pick, Enter record),
  greenlights. Empty state = tower clear.
- **Agents** — roster with live presence (listening / running / offline) and
  a chat thread per agent; offline agents queue messages, launchable ones
  get **Send + run**.
- **Board** — idea capture, sidequests, epochs → milestones → cards, frozen
  bay; card modal for editing, decisions, questions, log.

Durable collapse state, no localStorage, no framework, mobile bottom tabs.

## Plugin skills

Three focused skills ship with the plugin: **tower** (the work loop +
staying reachable), **tower-ballot** (authoring decisions the owner can
decide from the ballot alone), **tower-setup** (init, import, config,
server). Non-Claude agents use `AGENTS.md` — same protocol, plain shell.
