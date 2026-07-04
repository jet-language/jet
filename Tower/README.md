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
tower init | serve | import
```

`--json` everywhere for machine output; `--file x.json` / `--file -` (stdin)
for rich payloads; cards accept `#num` or id; `--by <name>` attributes every
write; `--expect-rev N` gives optimistic concurrency (exit 2 on conflict).

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

Black & red, pure dark. Red is reserved for what needs the **owner** — open
decisions, activations, P0s — so the eye goes there first; agent work reads
calm, done work disappears. Four views: **Decisions** (queue + focus mode:
←/→ move, 1–9 pick, Enter record), **Agent** (dispatch board), **Board**
(epochs → milestones → cards), **Ideas**. Durable collapse state, no
localStorage, no framework.
