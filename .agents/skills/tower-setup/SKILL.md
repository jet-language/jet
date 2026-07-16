---
name: tower-setup
description: Set up or configure Tower in a project — init the .tower/ data dir, import an older tower.json, tune config.json (terminology, priorities, decision groups), and start the board server. Use for "set up tower", "add tower to this project", "import my old board", "configure tower", or first-run problems (no Tower data found).
---

# Tower — set up in a project

Tower's code lives at repo-root `Tower/`; its data lives in `.tower/`. Setup =
create that directory, shape the config, start the server.

```sh
alias tower='scripts/agent/jet-env node Tower/tower.mjs'
tower init --name "<Project>"
tower serve --open
```

`init` creates `.tower/tower.json` (all state), public `.tower/config.json`,
and a `.gitignore` for `backups/`, `secrets.json`, and crash-residue
`.secrets.json.tmp-*` files. Commit `.tower/` so the
team shares the board —
including `.tower/history.json` once it appears (retired cards/decisions,
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
  ratified decision, sits live before it retires into `.tower/history.json`
  (`tower archive status|show|restore` reads it back). Nothing retires the
  instant it's ratified — the owner sees it on Now's "Recently decided"
  strip and can reopen it in one tap while it's fresh.

## Remote access, push, git linking

- First `tower serve` generates VAPID push keys in ignored
  `.tower/secrets.json`; the owner enables push per-device with **◍ notify**.
  Auth is OPT-IN: set `"auth": {"token": "…"}` in `secrets.json` to require
  a key from non-localhost devices (unlock screen
  asks once per device; localhost always exempt).
- Never put `auth` or `push` in tracked `config.json`. Tower rejects that
  legacy layout with migration guidance. Remove those fields, rotate any
  committed credentials, then write only replacements to `secrets.json`.
- `tower githook` installs a post-commit hook so commits mentioning `#12`
  append to that card's log — install it once per repo.

## First work session

1. Create the structure: `tower epoch add e1 --name "…" --goal "…"`,
   `tower epoch current e1`, `tower milestone add --epoch e1 --title "…"`.
2. Seed cards: `tower card add --title "…" --priority P1 …` — they land
   straight in `planning`, agent-ready; no owner greenlight step.
3. Add a line to the host repo's CLAUDE.md / AGENTS.md pointing agents at
   the **tower** skill (or `Tower/AGENTS.md` for non-Claude agents) so every
   session knows the board is the source of truth.
