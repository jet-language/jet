---
name: tower
description: Act on what the owner just recorded in Tower — implement ratified decisions, answer open card questions, advance agent-lane cards (plan / implement / verify), and raise new decisions in ballot-ready form. When burndown is the goal, work only Epoch 3 + sidequest cards in workOrder until both sections are empty. Use after the owner records decisions or leaves notes in Tower, or when asked to "process tower", "act on my decisions", "do the tower work", "work the board", "sweep the board". The owner only ever does two things (decide, greenlight); this skill does everything that follows, so he never has to retype the context.
---

# Tower — act on the board

Tower (`tools/Tower/`) holds the whole board in `tools/Tower/tower.json`. The
owner just recorded decisions and/or left notes. Your job is to do the work those
unblock — and nothing that is still his.

## The one rule that governs everything

**The owner's decisions are the only allowed bottleneck.** He must never wait on
you to write a plan or identify a decision, and he must never receive a plan or
decision that no agent has reviewed. Do the plans and decision-development eagerly
and in parallel; he only picks.

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

## Scope & work order — Epoch 3 burndown

When the owner asks to work the board, burn down Epoch 3, or continue the
milestone, **stay inside two sections only** until both are empty:

1. **Epoch 3** — `track:"epoch"` + `epoch:"e3"` + agent lane (`planning` /
   `ready` / `building` / `verify`). This is the on-plan Epoch 3 section in Tower.
2. **Sidequests** — `track:"sidequest"` + agent lane. Off-plan work the board
   surfaces above the epoch list.

**Do not pick up work outside this scope** unless the owner explicitly redirects:

- Other epochs (`e4`, `e5`, `e6`, …) — even if `ready` or `building`.
- Epoch cards assigned to a non-`e3` epoch.
- `frozen` cards (owner-only until activated).
- `decide` / `activate` lanes (owner-only).
- New cards, binder hygiene, or doc-only sweeps that expand scope.

**Exit criterion for the burndown:** both sections show no active agent-lane
cards — every in-scope card is `done` or `frozen` (frozen only when genuinely
parked; do not freeze to avoid finishing work).

### How to pick the next card

Sort the in-scope agent queue by **`workOrder` ascending** (lowest number first).
Tower UI uses the same sort (`app/ui/tower.js` → `orderOf` / `byOrderThenPhaseThenPrio`).
Within the same `workOrder`, prefer **`building`** (continue in-flight) over
**`verify`** (close claimed work) over **`implement`** / **`ready`** (start new)
over **`planning`** (write/vet plan). Respect `blockedBy` — skip blocked cards
and surface the blocker; do not invent spellings to bypass a gate.

If a card has no `workOrder`, treat it as last — after all numbered cards in
that section. Prefer setting `workOrder` when you activate or advance a card so
the board stays honest.

### Session loop

1. `node tools/Tower/Tower.mjs status` (or read `tower.json`) — count active
   **e3** + **sidequest** agent cards.
2. Pick the lowest `workOrder` unblocked card; finish a vertical slice; log it;
   move phase forward only on real verification.
3. Repeat until both sections are empty, then report burndown complete and list
   anything still in owner lanes (`decide`, `activate`) or blocked on ratification.


1. **Read** `tools/Tower/tower.json` (or run `node Tower.mjs status` and read the
   AGENT sections). Build work-lists **scoped to Epoch 3 + sidequests** (see
   above). Within that scope:
   - Ratified decisions (`status:"ratified"`) whose card is in `ready`/`building`
     and whose work isn't done — implement to the chosen `outcome`.
   - Cards in `plan` / `implement` / `building` / `verify` (lane logic in `laneOf`).
   - Open questions (`questions[]`, `status:"open"`).

2. **Answer questions first** — they often change what to build. Answer concretely
   on the merits; if the owner asked to change a ballot (add a comparison, reword
   an option), edit that decision in `tower.json`. Then mark the question answered
   (`status:"answered"`, set `answer`).

3. **Do each card's work** following the repo workflow in `AGENTS.md`: failing
   test first → spec in `docs/spec` → parser → sema → codegen → all tests green →
   docs updated. Respect invariants **I1–I8**. For a `plan` card, write the plan
   (and raise decisions, don't guess owner-facing syntax). For `verify`, prove it
   end-to-end before closing.

4. **Advance the card** as you go — append a dated entry to the card's `log`, and
   move `phase` forward: `planning`→(needs decisions? `deciding` #= `ready`);
   `ready`→`building`; `building`→`verify` when you claim done; `verify`→`done`
   only after real verification. Leave `num`/`id` alone.

5. **Write back.** Prefer the running server's API (start it if needed:
   `node Tower.mjs serve`), e.g. `POST /api/card/update` `{id, phase, logEntry}`,
   `POST /api/question/answer` `{id, answer}`, `POST /api/clearance` only if the
   owner explicitly delegated a decision (normally you do NOT decide). If no
   server, edit `tower.json` directly using the shapes in `app/store.mjs` and keep
   it valid JSON.

6. **Report** a short summary: what you implemented, what you answered, which cards
   advanced, and anything newly blocked on the owner (new decisions raised, or a
   card needing his greenlight). Surface those — they're the only thing he should
   have to look at next.

## Implementation standard — non-negotiable

"Implemented" means a **100% end-to-end, fully functional** vertical slice, never a
stub or a "ratified, milestone-pending" doc edit:

- parser → sema → codegen all wired, behavior reachable from real `.jet` source;
- every new diagnostic has a code in `diagnostics.md` **and** a `tests/ui` snapshot (I4);
- a runnable example in `examples/` with golden-tested output where user-visible (I5);
- unit/integration tests for the new paths; `nix develop -c cargo test` fully green;
- docs touched (`spec.md`, `syntax-decisions.md` status) updated to match real behavior.

A decision may sit "ratified but not yet built" **only** if it is still gated on an
unratified upstream decision. The owner's answer on an unblocked decision **is** the
"go" — do not park it.

## Raising a decision — ballot-ready or it doesn't count

Any owner-facing choice becomes a `decision` on its card. The owner decides from
the card alone in **Focus Mode**, which renders these `tower.json` fields — fill
them all or the card isn't ballot-ready:

- **`gist`** — one VERY short plain-language sentence: what is being chosen. No jargon.
- **`story`** — one short paragraph naming a real person (American-traditional first
  name — Betty, Hank, Walter, Doris, Earl…) and what they're doing, so the owner
  knows *why this decision exists* before he sees syntax.
- **`inWild`** — realistic `jet` code from a plausible real project (not a toy)
  where this choice actually bites.
- **`comparisons`** — how Rust/TS/Swift/etc. spell the same thing, when a
  cross-language comparison is relevant (skip if not).
- **`options[]`** — `{key, name, detail, code}` for **every** option; each carries
  its own worked `code` block showing what the person types and sees (including the
  error they hit). No option described abstractly. Mark the recommended `name`.
- **`rec`** — the recommended option key + a one-line *why*.

Rules:
- Give a rich menu of original syntax candidates, never 2–3 derivative ones. Never
  invent syntax that contradicts a ratified decision — read `syntax-decisions.md`.
- **ID must be Tower-parseable** (`D-…` or `S<digits>-…`) and must not collide with
  a ratified id: `rg "\bD-XXX\b" docs/spec/syntax-decisions.md`.
- **Implementation difficulty must never appear** in a tradeoff, an option ranking,
  or the recommendation (philosophy.md → "Effort is never a deterrent"). Rank only
  on safety, beginner experience, performance, one-path, long-term correctness.
- A plan-writer/vetter **proposes** syntax; never picks or ratifies it. Leave the
  card in `deciding` until the owner decides.

## When the owner ratifies a decision

Ratifying into `syntax-decisions.md` is step one of several, not the whole job:

1. **Honor every word the owner wrote.** A decision may carry a *question* or
   *request* ("can this be called X?", "defer Z"), not just a letter. Address it
   explicitly and reflect it in the ratified text. Never treat a question as a clean pick.
2. **Ratify** into `syntax-decisions.md` (Ratified section + decision log); set the
   decision's `status:"ratified"` + `outcome` in `tower.json`.
3. **Reconcile the card:** all its decisions answered and nothing upstream gates it
   → `building`, build now. Sub-decisions still open, or gated on an unratified
   decision → keep `deciding`, note the gate. Once-blocking decision just ratified →
   now unblocked → `building`.
4. **Implement end to end** (standard above). When green, `done`.

## Rules

- **Burndown scope:** Epoch 3 epoch-track + sidequest track only; sort by
  `workOrder`; do not wander into e4+ or frozen/decide/activate unless the owner
  says otherwise. Keep going until both sections have no active agent cards.
- Parallelise independent in-scope cards with sub-agents (sonnet for impl, opus
  type-system/design calls); give each enough context to act without re-reading the
  whole board. One layer deep, no nested spawns. No git worktrees unless the owner asks.
- Don't invent owner-facing syntax — raise it as a `decision` and leave the card `deciding`.
- Don't close anything you haven't actually verified. "Done" means done.
