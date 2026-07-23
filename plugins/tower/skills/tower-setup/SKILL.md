---
name: tower-setup
description: Set up or configure Tower — init plugins/tower/.tower, import an older tower.json, tune config.json, and start the board server. Use for "set up tower", "configure tower", or first-run problems (no Tower data found).
---

# Tower — set up in a project

Tower's code lives where it's installed (plugin dir or vendored `Tower/`);
its DATA lives at `plugins/tower/.tower/` beside this app. Setup = create that dir,
shape the config, start the server.

```
node ${CLAUDE_PLUGIN_ROOT}/tower.mjs init --name "<Project>"
node ${CLAUDE_PLUGIN_ROOT}/tower.mjs serve --open      # board at :7878
```

`init` creates `plugins/tower/.tower/tower.json` (all state), public `plugins/tower/.tower/config.json`,
and a `.gitignore` for `backups/`, `secrets.json`, and crash-residue
`.secrets.json.tmp-*` files. Commit `plugins/tower/.tower/` so the
team shares the board —
including `plugins/tower/.tower/history.json` once it appears (retired cards/decisions,
see below); it's board history, not a cache, and is NOT gitignored.
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
  "retireAfterDays": 3
}
```

- **`port`** — CLI and UI both use it; if a different tool already owns
  7878, set another port here. The server binds on the configured port —
  treat it as trusted-network-only (LAN/tailnet).
- **`retireAfterDays`** — the walk-back buffer: how long a done card, or a
  ratified decision, sits live before it retires into `plugins/tower/.tower/history.json`
  (`tower archive status|show|restore` reads it back). Nothing retires the
  instant it's ratified — the owner sees it on Now's "Recently decided"
  strip and can reopen it in one tap while it's fresh.

## Remote access, git linking

- Auth is OPT-IN: set `"auth": {"token": "…"}` in ignored `plugins/tower/.tower/secrets.json`
  to require a key from non-localhost devices (unlock screen asks once per
  device; localhost always exempt). Web push/VAPID is removed — live updates
  use SSE only. Tower never invents secrets.
- Never put `auth` or `push` in tracked `config.json`. Tower rejects that
  legacy layout with migration guidance. Remove those fields, rotate any
  committed auth token, delete leftover `push` from secrets.
- `tower githook` installs a post-commit hook so commits mentioning `#12`
  append to that card's log — install it once per repo.

## First work session

1. Create the structure: `tower epoch add e1 --name "…" --goal "…"`,
   `tower epoch current e1`, `tower milestone add --epoch e1 --title "…"`.
2. Seed cards: `tower card add --title "…" --priority P1 …` — they land
   straight in `planning`, agent-ready; no owner greenlight step.
3. Add a line to the host repo's CLAUDE.md / AGENTS.md pointing agents at
   the **tower** skill (or the plugin's `AGENTS.md` for other agents) so every
   session knows the board is the source of truth.
