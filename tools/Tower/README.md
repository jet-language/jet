# `Tower` — task pipeline dashboard (v2)

A dependency-free tool over the owner workflow:

```
frozen → backlog → deciding → planning → ready → building → done
```

It reads the canonical docs (board.json, the ballot markdown, the plan/proposal/
idea files) and records owner input back; it never invents state, so it can't
drift from the docs that are the source of truth.

## Use

```sh
# Dashboard + decision Focus Mode in the browser (the main way to work):
nix develop -c node tools/Tower/Tower.mjs serve --open

# Console snapshot (board + worklist + decisions):
nix develop -c node tools/Tower/Tower.mjs status

# Scaffold a new sidequest plan (with the v2 decision-card template):
nix develop -c node tools/Tower/Tower.mjs new <slug> "Title"
```

## Layout

The tool is split into modules; `Tower.mjs` is a slim entry point.

```
Tower.mjs            entry: dispatch status / serve / new
app/
  paths.mjs          paths + small shared utilities
  markdown.mjs       markdown→HTML + multi-language highlighter
  board.mjs          board store + the 7-stage model (normalizes legacy names on load)
  ballot.mjs         ballot parser (rich card schema) + card↔decision linkage + results merge
  state.mjs          per-card computed status, the "Ready for Claude" worklist, /api/state
  writes.mjs         record-only write-backs (results, regen, questions, ingest)
  server.mjs         http server + JSON API + static UI
  cli.mjs            console status + scaffold
  ui/                index.html · tower.css · tower.js  (served static; no build step)
```

## The dashboard

A dark mission-control surface. The hero is the **pipeline ribbon** — seven stages
with live counts; click one to jump. Sections start collapsed but stay informative
(count + preview).

- **Board.** Cards (task / idea / bug) move down the pipeline with inline **◀ ▶**
  buttons and a stage dropdown. Each card shows a **computed status** — `Frozen`,
  `Needs plan`, `Blocked on N decisions` (with links into Focus Mode), `Plan ready`,
  `Building`, `Done` — and whether the next move is **auto** (I proceed) or **gated**
  (say "go"). `frozen` is the parking lane for ideas you want to consider without
  going down the rabbit hole. Click any title / description / note to edit inline.
  - **Ready for Claude** panel — the worklist: every card whose next move is mine,
    split **auto** (build a plan, draft a decision — I proceed) vs **gated**
    (implement code — wait for "go").
  - **Ingest** panel — hand me a file path or pasted text; I read it, extract
    candidate ideas / features / syntax, and file them as **frozen** cards for you to
    triage. Queues to `ingest-queue.md`.
- **Decisions.** A clean overview (progress meter + grouped one-line rows) and
  **Focus Mode** — a full-screen, one-decision-at-a-time deck. Keyboard-driven
  (←/→ move, 1–9 pick, Enter next, Esc close). Each decision shows a plain-language
  **gist**, a **story** (a real person, American-traditional names), and facet tabs
  for **In the wild** (real project code), **Other languages**, **Trade-offs**
  (subagent-reviewed), and **Q&A** — kept out of the recommendation so nothing
  clutters the choice. Tick an option, comment, **Sign & file**.
- **Proposals.** Proposals and parked ideas; click to read/edit inline.
- **Scratch.** A free pad; autosaves to board.json.

## The handoff (records, never acts)

The server **records and queues**; it never edits code or ratifies. On **Sign &
file** it merges your picks into `ballots/ballot-results.md` (adds/replaces by id,
never wipes earlier decisions). Then:

- **auto** moves (build a plan, draft a decision) — I proceed without waiting.
- **gated** moves (implement code / unblocked plans) — I wait for your "go".

So a stray submit can't change the language. Ratifying and implementing stay Claude
steps. See the **tower-sweep** skill for the full loop.
