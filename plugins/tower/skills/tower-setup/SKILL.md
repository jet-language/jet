---
name: tower-setup
description: >-
  Initialize Tower data, import an older board, or inspect first-run
  `config.json`. Use when `plugins/tower/.tower/` is missing, a board must be
  migrated, or configuration needs a bounded inspection. Do not use for board
  operations, ballots, ranking, preparation, or execution.
---

# Tower — initialize or inspect setup

## Contract

- **Requested outcome:** Tower data is initialized or imported, or the existing
  configuration has been inspected and reported.
- **Supplied inputs:** Project name, existing Tower state or import file,
  installed Tower path, and any owner-requested configuration facts.
- **Allowed child result:** Passive reads and non-serve Tower CLI inspection may
  return state or configuration facts. No child starts the server, edits board
  JSON, or changes project policy.
- **Completion owner:** `tower-setup` owns initialization and import. The
  owner owns server startup and board exposure.
- **Return point:** Return from each inspection or import to setup, then report
  exact resulting paths and configuration facts.
- **Stopping condition:** Stop after the requested init, import, or inspection
  is complete and the owner has the command to start the server. Never start
  `tower serve` automatically.

## Data path and commands

Tower code lives where it is installed (`plugins/tower/` here, or a vendored
`Tower/`). Its data lives beside that app in `plugins/tower/.tower/`.
Initialization and import are the only state-creating operations in this
route:

```sh
# Jet (vendored)
node plugins/tower/tower.mjs init --name "<Project>"
node plugins/tower/tower.mjs import <old-tower.json> --name "<Project>"

# Plugin install
node ${CLAUDE_PLUGIN_ROOT}/tower.mjs init --name "<Project>"
node ${CLAUDE_PLUGIN_ROOT}/tower.mjs import <old-tower.json> --name "<Project>"
```

For an old v3 board, `binder` becomes `ideas`; epochs and cards carry over
losslessly. Inspect the installed command surface with `tower help` rather
than guessing a subcommand.

`init` creates `plugins/tower/.tower/tower.json`, public `config.json`, and a
`.gitignore` for `backups/`, `secrets.json`, and crash-residue
`.secrets.json.tmp-*`. Commit the data directory so the team shares the board,
including `history.json` after it appears; history is not a cache and is not
gitignored.

## Configuration inspection

There is no invented `tower config` command. For a configuration inspection,
read the existing `plugins/tower/.tower/config.json` as a normal file and
report only facts relevant to the request. All fields are optional:

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

`port` is shared by CLI and UI; if another local service owns 7878, the owner
may choose another value. `retireAfterDays` is the walk-back buffer before a
done card or ratified decision moves to `history.json`. Do not add removed
authentication or push fields. Do not treat configuration inspection as
permission to edit board state or project policy.

## Server boundary

**Only the owner starts `tower serve`.** Setup does not start a server, pass
owner credentials, or open a browser. Use the owner-facing command only after
setup stops:

```sh
tower serve --open
```

Read [remote access and first session](references/remote-and-first-session.md)
only when the owner requests LAN access, a git hook, or initial epoch/card
seeding. Read [tower](../tower/SKILL.md) for ordinary board operations. Do not
load remote or campaign detail during a bounded init or inspection.
