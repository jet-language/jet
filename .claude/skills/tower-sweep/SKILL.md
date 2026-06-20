---
name: tower-sweep
description: Run the Tower project-management sweep — reconcile the board with reality, generate and vet plans, develop and agent-review decisions, and queue them in the ballot for the owner. Use when asked to "evaluate the tower", "sweep the board", "process tower items", "update the cards", or otherwise advance the Jet PM pipeline. The owner is CEO/CTO; the sweep exists so the ONLY thing he waits on is his own decisions.
---

# Tower sweep

Tower (`tools/Tower/Tower.mjs`) is the owner's mission control. State lives in
`tools/Tower/board.json` (cards) and `tools/Tower/docs/ballots/decision-ballots.md`
(open decisions). The sweep moves every item to its correct state and queues
exactly what the owner must decide — nothing more.

## The one rule that governs everything

**The owner's decisions are the only allowed bottleneck.** He must never wait on
you to write a plan or identify a decision, and he must never receive a plan or
decision that no agent has reviewed. Concretely:

- Every plan reaching him is written by one agent and **vetted/refined by a second,
  different agent** against the real codebase.
- Every decision reaching him is **developed, then reviewed by an agent**, then added
  to the ballot in the exact house format below.
- You do the plans and decision-development eagerly and in parallel; he only picks.

## Pipeline stages (board.json `stage`)

`far-horizon → pre-plan → planned → decisions → implementation → done`

- **far-horizon** — reference only. **Do not touch.** The owner moves these in himself.
- **pre-plan** — wanted, no plan yet. → generate a plan (see below).
- **planned** — has a vetted plan, no open decision. Ready to implement on his "go".
- **decisions** — blocked on an owner decision that is sitting in the ballot.
- **implementation** — actively being built. → action via subagent.
- **done** — shipped.

## Sweep procedure

1. **Reconcile with reality first (cheap, durable — do before fanning out).**
   - `rg "Implemented" docs/spec/syntax-decisions.md` and check each card's decision
     IDs. Decision ratified + implemented → move card to **done**; document the impl
     (it's already in the syntax-decisions log) and delete the card's plan from
     `sidequests/`. Decision ratified but not built → move out of **decisions** to
     **planned** (note "ready — say go").
   - `rg "not yet implemented" docs/spec/syntax-decisions.md` — **every hit needs a
     board card.** Before deleting any shipped plan, create cards for its ratified
     follow-ups or they orphan.
   - Verify the durable rationale of any plan is captured in `syntax-decisions.md`
     **before** deleting the plan file. Sidequests is for *active* plans, not an
     archive — keep it clean.

2. **Implementation-stage items** → action with a subagent (no worktrees — the owner
   dislikes uncleaned worktrees; serialize anything touching `Source/`).

3. **Planned items** → if no plan, generate one. If it needs an owner decision,
   develop the decision, get it agent-reviewed, add it to the ballot, and move the
   card to **decisions**.

4. **Pre-plan items** → for each, run the plan pipeline:
   - **Write** (subagent, parallel — each writes its own file in
     `tools/Tower/docs/sidequests/<slug>.md`, so they never collide).
   - **Vet/refine** (a *different* subagent per plan): verify every claim against the
     codebase, enforce invariants I1–I8 and owner standards, refine in place.
   - **Develop decisions**: any user-facing choice becomes a ballot card (below).
     Plan-writers and vetters **propose** syntax; they never pick or ratify it.
   - Set the card's `plan` slug and move it to **decisions** (has a decision) or
     **planned** (none).

5. **Far-horizon** → leave it.

6. **Merge decisions into the ballot** yourself in one controlled write (single
   writer = no corruption) once vetters return ballot-ready cards.

## Decision-card house rules (or Tower can't show it / the owner can't trust it)

The owner decides from the card alone — he should never have to open the plan to
understand the choice. **Every card MUST carry all three of these, or it is not
ballot-ready:**

1. **A user story** — one short paragraph naming a real person and what they are
   trying to do, so the owner knows *why this decision exists* before he sees syntax.
2. **A short tradeoff comparison** — a compact table (or tight bullet list) with one
   row per option and 3–4 columns that actually differ (e.g. ceremony, failure mode,
   ratification cost, familiarity). The owner must be able to weigh options at a
   glance without reverse-engineering them from prose.
3. **A worked example of *every* option** — each `- **Option X — <name>.**` bullet
   carries its own fenced ```jet (or ```shell) block showing what the real person
   from the user story types and sees (including the error they hit). No option may
   be described abstractly; if it's an option, it has a code block.

Then close with `**Recommendation:**` and a one-line *why*.

- Format exactly: `### <ID> — <title> (rec X)`, the user story, the tradeoff table,
  then the `- **Option A — <name>.**` bullets each with their worked example, then
  `**Recommendation:**`. No abstract option tables standing in for examples.
- **ID must be Tower-parseable**: `D-…` or `S<digits>-…` (the parser regex rejects
  `S-DBG1`-style ids). Check the new id does **not** collide with a ratified id:
  `rg "\bD-XXX\b" docs/spec/syntax-decisions.md`.
- Give a rich menu of original syntax candidates, never 2–3 derivative ones. Never
  invent syntax that contradicts a ratified decision — read `syntax-decisions.md`.

## Hard constraints

- **board.json is owner-owned and live-edited.** Never regenerate/clobber it; make
  surgical edits or a targeted load-mutate-save of specific cards/fields only.
- **No git worktrees**, ever, unless the owner explicitly asks.
- **Lanes:** the owner may run a separate agent on `tools/Tower/docs/proposals/`
  (blue-sky proposals from `board.json` scratch). Plans (carded items) go to
  `sidequests/`; never write to `proposals/` when sweeping.
- Drain `tools/Tower/questions-queue.md` (and `regen-queue.md` if present) each sweep.

## Doc layout (durable vs PM)

- **Durable spec — stays in `docs/spec/`:** `syntax-decisions.md` (ratified log =
  single source of truth), `philosophy.md`, `architecture.md`, `diagnostics.md`,
  `roadmap.md`, `spec.md`, `release-policy.md`.
- **In-flight PM — lives under `tools/Tower/docs/`:** `proposals/`, `sidequests/`,
  `plans/` (epoch milestones, jetpack-jetos), `ballots/` (decision-ballots.md,
  ballot-results.md). Keep `docs/` clean; keep `sidequests/` to active plans only.

## The webapp

`node tools/Tower/Tower.mjs serve --open` is the owner's surface. Plan links on
cards and proposal cards open an in-app markdown **viewer/editor** (modal). After a
sweep, the owner should be able to do everything from the webapp: read/edit any
plan or proposal, see every queued decision, pick, and sign & file.
