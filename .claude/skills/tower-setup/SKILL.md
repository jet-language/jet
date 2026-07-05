---
name: tower-setup
description: Set up or configure Tower in a project — init the .tower/ data dir, import an older tower.json, tune config.json (terminology, priorities, decision groups, agent roster, launch commands), and start the board server. Use for "set up tower", "add tower to this project", "import my old board", "configure tower", or first-run problems (no Tower data found).
---

# Tower — set up in a project

Tower's code lives where it's installed (plugin dir or vendored `Tower/`);
its DATA lives in the host project at `.tower/`. Setup = create that dir,
shape the config, start the server.

```
node ${CLAUDE_PLUGIN_ROOT}/tower.mjs init --name "<Project>"
node ${CLAUDE_PLUGIN_ROOT}/tower.mjs serve --open      # board at :7878
```

`init` creates `.tower/tower.json` (all state), `.tower/config.json`, and a
`.gitignore` for `backups/`. Commit `.tower/` so the team shares the board.
Migrating an older board: `tower import <old-tower.json> --name "<Project>"`
(v3-era files: `binder` → ideas, epochs/cards carried losslessly).

## config.json — everything optional

```json
{
  "project": "My Project",
  "terms": { "epoch": "Season", "milestone": "Target" },
  "tracks": ["epoch", "sidequest"],
  "kinds": ["task", "feature", "idea", "bug"],
  "priorities": ["P0", "P1", "P2", "P3"],
  "decisionGroups": ["design", "architecture", "api", "ui", "tooling"],
  "port": 7878,
  "backups": 20,
  "agents": [
    { "name": "claude-main", "kind": "claude" },
    { "name": "codex-1", "kind": "codex" }
  ],
  "commands": {
    "claude": "claude -p",
    "codex": "codex exec"
  }
}
```

- **`agents`** — the roster shown in the board's Agents view. Listeners also
  self-announce, so this is just the stable, always-visible set.
- **`commands`** — the launch bridge, **opt-in**. When an agent of that kind
  is offline, the owner's "Send + run" button starts a headless turn:
  Tower runs the command from the project root with the message in
  `$TOWER_PROMPT` (appended as one quoted argument); stdout becomes the
  agent's reply in the thread. Only add commands the owner is happy to have
  the local board server execute. The server binds localhost-style on the
  configured port — treat the port as trusted-network-only (LAN/tailnet).
- **`port`** — CLI and UI both use it; if a different tool already owns
  7878, set another port here so `tower message`/`tower agents` reach the
  right server.

## Remote access, push, git linking

- First `tower serve` generates `auth.token` (non-localhost requests need it:
  open `http://<host>:<port>/?key=<token>` once per device) and VAPID push
  keys. The owner enables push per-device with the **◍ notify** button.
- `tower githook` installs a post-commit hook so commits mentioning `#12`
  append to that card's log — install it once per repo.
- `notifyBatchSeconds` (default 90) controls how ratification/greenlight
  notifications batch before waking listening agents.

## First work session

1. Create the structure: `tower epoch add e1 --name "…" --goal "…"`,
   `tower epoch current e1`, `tower milestone add --epoch e1 --title "…"`.
2. Seed cards: `tower card add --title "…" --priority P1 …` (they start in
   `triage`; the owner greenlights from the board's Now view).
3. Add a line to the host repo's CLAUDE.md / AGENTS.md pointing agents at
   the **tower** skill (or `Tower/AGENTS.md` for non-Claude agents) so every
   session knows the board is the source of truth.
4. Each working agent starts a listener: `tower agent listen --name <me>`
   (under Claude Code, inside the Monitor tool) so the owner can reach it
   from the board.
