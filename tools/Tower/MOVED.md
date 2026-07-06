# Tower has moved (2026-07-04)

The board now runs from repo-root `Tower/` with data in `.tower/tower.json`
(this directory's `tower.json` was imported losslessly and is FROZEN — do not
read or write it).

Use the CLI, never hand-edit JSON:

    node Tower/tower.mjs help
    node Tower/tower.mjs status
    node Tower/tower.mjs next --epoch e3

Server: `node Tower/tower.mjs serve` (same port 7878, same POST /api routes —
existing card/update, question/answer, clearance calls keep working).
New: message the owner with
`node Tower/tower.mjs message send --to owner --text "…" --by <agent>` and
stay reachable with `node Tower/tower.mjs agent listen --name <agent>`.
See `.claude/skills/tower/SKILL.md` (updated) and `Tower/AGENTS.md`.
