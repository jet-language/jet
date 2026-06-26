# Tower — Jet project board

The project-management board, rebuilt. Black & red, pure dark, plain names,
file-backed. Every card resolves to exactly one **lane** that says who owns the
next move and what it is — so it's always clear what's blocked on you vs. ready
for an agent.

## Lanes — who owns the next move

Only two lanes ever block **you**:
- **Decide** — the card has open decisions. Make them in Decisions / focus mode.
- **Activate** — a triaged card waiting for you to pull it into a track.

The rest are an agent's, inert, or hidden:
- **Plan** (build a plan + raise the decisions it needs) · **Implement** ·
  **Building** · **Verify** (a claimed-done card, verified 100% before it closes).
- **Blocked** (by another card) · **Frozen** (never touched until you activate it)
  · **Done** (verified — hidden behind collapsed groups).

Stages: Triage → Deciding → Planning → Ready → Building → Verify → Done, + Frozen.
A card with an open decision shows as **Decide** no matter its stage — so a build
that hits a late decision still surfaces as blocked on you.

## Views (four)

- **Decisions** (default) — everything blocked on you: the decision queue plus the
  activation strip. Each decision opens **focus mode** — the v1 layout: a glowing
  dot-navigator, facet tabs (Story / Why / In the wild / Other languages), options
  with syntax-highlighted code, the recommendation, and a bottom action bar. ←/→
  to move, 1–9 to pick, Enter to record, Esc to close. Clicking a chosen option
  again (or **✕ Clear choice**) cancels it. **✎ Ask a question** saves a note to
  the card for an agent *without recording a decision*.
- **Agent** — the dispatch board: open questions to answer, then Plan / Implement /
  Building / Verify groups. Filter by text, epoch, or topic.
- **Board** — cards grouped by epoch (epochs are the groupings; cards are the
  work), then a Sidequests bay, then a collapsible **Frozen** group. Done is
  collapsed per epoch. Manage every card here.
- **Ideas** — capture ideas; `Add as card` promotes one to a tracked card.

Leave a **note or question** on any card (its modal, or from focus mode) — open
ones surface to agents in the Agent view. Code everywhere is syntax-highlighted.

## Run

```
cd tools/Tower-v2
node Tower.mjs serve --open      # board at http://localhost:7878
node Tower.mjs status            # text snapshot
node Tower.mjs migrate           # dry-run: lossless port from v1 (tools/Tower)
node Tower.mjs migrate --write   # write the port into tower.json
node Tower.mjs audit             # prove every v1 item survived into v2
```

Node only, zero dependencies, no build step.

## Data — one file, fully durable

Everything (cards, decisions, questions, collapse state, numbering) lives in
`tower.json`. No localStorage, no ephemeral state.

- **cards** — `{ id, num, title, body, kind, track, epoch, phase, priority, plan,
  blockedBy, log }`. `num` is the stable tracking number shown as `#N`.
- **decisions** — `{ id, cardId, title, gist, story, explainer, inWild,
  options[{key,name,detail,code}], comparisons[{lang,note,code}], rec, status,
  outcome, comment }`. Each names exactly one card; its state is **computed** on
  every read (`store.mjs` → `clearanceOf`/`laneOf`), never stored twice — the v1
  desync, fixed structurally.
- **questions** — `{ id, cardId, by, kind, text, status, answer }`. Owner notes /
  questions; agents answer.
- **meta.ui.open** — which collapsible groups are expanded (Done stays collapsed
  by default, so finished work is hidden until asked for).
- **meta.nextNum** — next tracking number.

## Migrating from v1 (done — lossless)

The v1 board has been ported in full. `migrate` reads `tools/Tower/board.json`
**read-only** and maps everything into v2's shape — every card (stage → phase,
notes → log, `workOrder` preserved), the live decisions (with their ratified
outcomes), and **all 36 owner Q&A** (kept verbatim; the historical ones stay
attached by `decisionId` even when their card is long gone). Cards renumber to
**#1..N** but keep their original `id`, so any external reference still resolves.
It never writes a v1 file; `--write` replaces `cards`/`decisions`/`binder`/
`questions` and keeps the hand-authored `epochs`. The v1 doc tree
(`sidequests/`, `plans/`, `proposals/`, `ballots/`) is copied to
`tools/Tower-v2/docs/`.

**`audit`** compares v1 → v2 and proves nothing was lost (cards, decisions,
questions, scratch, plan docs) — run it after `--write`; it must say `LOSSLESS ✓`
before you retire v1. Track/epoch are a best-effort guess (active → epoch, else
sidequest); re-seat cards onto their real epoch when you thaw them.

## Design

Black & red, pure dark. Red is reserved for what needs **you** — open decisions,
activations, P0s, blockers — so the eye goes there first; agent work reads in calm
neutral, done work disappears. Saira headings, IBM Plex Sans body, IBM Plex Mono
for ids/data/code. No horizontal page scroll; card detail and decision focus open
as centered overlays.
