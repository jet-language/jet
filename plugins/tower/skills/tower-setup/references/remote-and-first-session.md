# Remote access and first session

Read this reference only when the owner asks for remote access, a git hook, or
initial project seeding. Setup itself remains bounded to init, import, and
configuration inspection.

## LAN access

Tower has no credentials or setup key on the local network. The owner may open
`http://<machine-hostname>:<port>` or `http://<machine-ip>:<port>` from another
trusted device. Every LAN device can read and change the board, so never expose
Tower to the public Internet.

Browser mutations require same-origin evidence. CLI mutations send the explicit
`X-Tower-Client: cli` header. Opening the board creates a short-lived HttpOnly
owner interaction session, and acceptance uses a one-time challenge tied to
that session. `auth` and `push` are removed config fields; tracked config
rejects them, and ignored `secrets.json` files are not read.

Only the owner starts `tower serve`. Setup does not start it or open a browser.

## Git linking

If the project wants commit references logged on cards, the owner or an
explicitly authorized operator may install the hook once per repository:

```sh
tower githook
```

The hook appends commits mentioning `#12` to that card's log. It does not
replace the CLI write boundary.

## First work session

After initialization, seed the board only when the owner requests it:

```sh
tower epoch add e1 --name "…" --goal "…"
tower epoch current e1
tower milestone add --epoch e1 --title "…"
tower card add --title "…" --priority P1 …
```

Cards land in `planning` and are agent-ready. There is no owner greenlight
step. Add a line to the host repo's `CLAUDE.md` or `AGENTS.md` pointing agents
to the `tower` skill (or this plugin's `AGENTS.md`) so every session treats the
board as the source of truth.
