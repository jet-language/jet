---
name: tower-v2-act
description: Act on what the owner just recorded in Tower v2 — implement ratified decisions, answer open card questions, and advance agent-lane cards (plan / implement / verify). Use after the owner records decisions or leaves notes in Tower v2, or when asked to "process tower v2", "act on my decisions", "do the tower work", "work the board". The owner only ever does two things (decide, greenlight); this skill does everything that follows, so he never has to retype the context.
---

# Tower v2 — act on recorded decisions

Tower v2 (`tools/Tower-v2/`) holds the whole board in `tools/Tower-v2/tower.json`.
The owner just recorded decisions and/or left notes. Your job is to do the work
that those unblock — and nothing that is still his.

## The model (read this first)

Every card has a computed **lane** (see `app/store.mjs` → `laneOf`). Only two
lanes are the owner's: `decide` and `activate` (greenlight). **Never touch those,
and never touch `frozen`.** Your lanes are:

- **plan** — write a thorough plan for the card + raise any decisions it needs
  (queue them as `decisions` so they show up for the owner).
- **implement** — the plan is vetted and every linked decision is ratified; build it.
- **building** — in progress; continue to completion.
- **verify** — a card was claimed done; verify it 100%, then close it.

Plus: any **open question** (`questions[]` with `status:"open"`) is the owner
asking you something on a card — answer it.

## Steps

1. **Read** `tools/Tower-v2/tower.json`. Build three work-lists:
   - Ratified decisions (`status:"ratified"`) whose card is in `ready`/`building`
     and whose work isn't done yet — implement to the chosen `outcome`.
   - Cards in `plan` / `implement` / `building` / `verify` (use the lane logic in
     `laneOf`, or just run `node Tower.mjs status` and read the AGENT sections).
   - Open questions (`questions[]`, `status:"open"`).

2. **Answer questions first** — they often change what to build. Answer concretely
   on the merits; if the owner asked to change a ballot (add a comparison, reword
   an option, etc.), edit that decision in `tower.json`. Then mark the question
   answered (`status:"answered"`, set `answer`).

3. **Do each card's work** following the repo workflow in `CLAUDE.md`: failing
   test first → spec in `docs/spec` → parser → sema → codegen → all tests green →
   docs updated. Respect invariants **I1–I8**. For a `plan` card, write the plan
   (and raise decisions, don't guess owner-facing syntax). For `verify`, prove it
   end-to-end before closing.

4. **Advance the card** as you go — append a dated entry to the card's `log`, and
   move `phase` forward: `planning`→(needs decisions? `deciding` : `ready`);
   `ready`→`building`; `building`→`verify` when you claim done; `verify`→`done`
   only after real verification. Renumber/ids: leave `num` alone.

5. **Write back.** Prefer the running server's API (start it if needed:
   `node Tower.mjs serve`), e.g. `POST /api/card/update` `{id, phase, logEntry}`,
   `POST /api/question/answer` `{id, answer}`, `POST /api/clearance` only if the
   owner explicitly delegated a decision (normally you do NOT decide). If no
   server, edit `tower.json` directly using the shapes in `app/store.mjs` and keep
   it valid JSON.

6. **Report** a short summary: what you implemented, what you answered, which
   cards advanced, and anything newly blocked on the owner (new decisions raised,
   or a card that needs his greenlight). Surface those — they're the only thing he
   should have to look at next.

## Rules

- Parallelise independent cards with sub-agents (sonnet for impl, opus for hard
  type-system/design calls); give each enough context to act without re-reading
  the whole board. One layer deep, no nested spawns.
- Don't invent owner-facing syntax — if a card needs a surface decision, raise it
  as a `decision` on that card and leave the card in `deciding`.
- Don't close anything you haven't actually verified. "Done" means done.
- Touch only `tools/Tower-v2/`; never edit the v1 Tower (`tools/Tower/`).
