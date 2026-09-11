---
name: tower-setup
description: Set up or configure Tower — init plugins/tower/.tower, import an older tower.json, or tune config.json. Use for "set up tower", "configure tower", or first-run problems (no Tower data found).
---

# Tower — set up in a project

## Contract

- **Requested outcome:** An initialized or repaired Tower data/configuration directory, ready for the owner to start the board when needed.
- **Supplied inputs:** Project name, existing Tower state or import file, config values, and the installed Tower path.
- **Allowed child result:** Passive reads and `tower` CLI inspection may return state/config facts. No child starts the server, edits board JSON, or changes project policy.
- **Completion owner:** `tower-setup` owns initialization and configuration; the owner owns server startup and board exposure.
- **Return point:** Return from each inspection or import to setup, then report the exact resulting paths and config.
- **Stopping condition:** Stop after init/import/config is complete and the owner has the command to start the server. Do not start `tower serve` automatically.

Tower's code lives where it's installed (plugin dir or vendored `Tower/`);
its DATA lives at `plugins/tower/.tower/` beside this app. Setup = create that
dir and shape the config. The owner starts the board server when needed.

```
# Jet (vendored):
node plugins/tower/tower.mjs init --name "<Project>"

# Plugin install (Claude Code / Cursor):
node ${CLAUDE_PLUGIN_ROOT}/tower.mjs init --name "<Project>"
```

The owner decides when to run `tower serve --open`; setup does not start a
server or open a browser.

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
  7878, set another port here. The server listens on the local network —
  treat it as trusted-network-only (LAN/tailnet).
- **`retireAfterDays`** — the walk-back buffer: how long a done card, or a
  ratified decision, sits live before it retires into `plugins/tower/.tower/history.json`
  (`tower archive status|show|restore` reads it back). Nothing retires the
  instant it's ratified — the owner sees it on Now's "Recently decided"
  strip and can reopen it in one tap while it's fresh.

## LAN access, git linking

- Tower needs no credentials or setup key for LAN access. Open
  `http://<machine-hostname>:<port>` or `http://<machine-ip>:<port>` from
  another device. Every device on the LAN can read and change the board; do
  not expose Tower to the public Internet.
- Browser mutations require same-origin evidence. CLI mutations must send the
  explicit `X-Tower-Client: cli` header.
- Opening the board silently creates a short-lived HttpOnly owner interaction
  session. Acceptance uses a one-time challenge tied to that session.
- `auth` and `push` are removed fields. Tower rejects them in tracked
  `config.json`; existing ignored `secrets.json` files are not read.
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
