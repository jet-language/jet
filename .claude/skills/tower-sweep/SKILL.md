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

`frozen → backlog → deciding → planning → ready → building → done`

- **frozen** — parked for later; considered, not pursued. **Do not touch.** The owner moves these in himself. (was `far-horizon`)
- **backlog** — wanted, no plan yet. → generate a plan (see below). (was `pre-plan`)
- **deciding** — blocked on an owner decision sitting in the ballot. (was `decisions`/`blocked`)
- **planning** — owner decided to proceed; you are building the plan. Transient.
- **ready** — has a vetted plan, no open decision. Ready to implement on his "go". (was `planned`)
- **building** — actively being built. → action via subagent. (was `implementation`)
- **done** — shipped.

Tower normalizes the old v1 names on load, so legacy cards keep working; write
new cards with the v2 names. Tower computes each card's status and a **"Ready for
Claude" worklist** (shown on the Board) from the ballot linkage — every card whose
next move is yours, split **auto** vs **gated**:

- **auto** (build a plan, draft a decision) — proceed without waiting for "go".
- **gated** (implement code / unblocked plans) — wait for the owner's "go".

The link is the ballot group heading `## <name> — board card cXX`: the `### <ID>`
decisions beneath it are that card's blockers. Keep that heading accurate or the
status/worklist go blank.

## Processing an answered ballot (the owner just decided — do ALL of this, not just the log)

When the owner drops decisions into `ballots/ballot-results.md`, ratifying into
`syntax-decisions.md` is **step one of five, not the whole job.** A decision is not
"processed" until its card and plan are moved and — if unblocked — it is **built end to
end.** For every answered decision:

1. **Honor every word the owner wrote.** A ballot line may carry a *question* or *request*
   ("can this be called X?", "ensure Y is captured", "defer Z"), not just a letter. Address
   the request explicitly — rename, capture the deferral as a tracked card/deferred-entry,
   etc. — and reflect it in the ratified text. Never silently treat a question as a clean pick.
2. **Ratify** into `syntax-decisions.md` (Ratified section + decision log).
3. **Strip** the decided card from `decision-ballots.md` (leave still-open sub-decisions).
4. **Reconcile the board card** (`board.json`, surgical edit):
   - All the card's decisions answered **and no upstream open decision gates it** →
     move to **building** and build it now (step 5).
   - Some sub-decisions still open, **or** gated on another *unratified* decision (e.g. a
     feature gated on D-EFF1 while D-EFF1 is still open) → keep in **deciding**; record in
     the plan which gate it waits on. Do **not** mark it "ready" — it isn't.
   - Ratified but gated on a decision that **just became ratified** → it is now unblocked →
     **building**.
5. **Update the plan** (`sidequests/<slug>.md`) to match the ratified choice, then **implement
   end to end** (see standard below). When green, move the card to **done** and delete the
   plan from `sidequests/` (its durable rationale already lives in `syntax-decisions.md`).

### Implementation standard — non-negotiable

"Implemented" means a **100% end-to-end, fully functional** vertical slice, never a stub or a
"ratified, milestone-pending" doc edit:

- parser → sema → codegen all wired, behavior reachable from real `.jet` source;
- every new diagnostic has a code in `diagnostics.md` **and** a `tests/ui` snapshot (I4);
- a runnable example in `examples/` with golden-tested output where the feature is user-visible (I5);
- unit/integration tests for the new paths; `nix develop -c cargo test` fully green;
- docs touched (`spec.md`, `syntax-decisions.md` status) updated to match real behavior.

"Ratified but not yet implemented / `src/` untouched" is **only** legitimate for a decision
**still gated on an unratified upstream decision**. The owner's ballot answer on an unblocked
decision **is** the "go" — do not park it.

## Sweep procedure

1. **Reconcile with reality first (cheap, durable — do before fanning out).**
   - `rg "Implemented" docs/spec/syntax-decisions.md` and check each card's decision
     IDs. Decision ratified + implemented → move card to **done**; document the impl
     (it's already in the syntax-decisions log) and delete the card's plan from
     `sidequests/`. Decision ratified but not built → move out of **deciding** to
     **ready** (note "ready — say go").
   - `rg "not yet implemented" docs/spec/syntax-decisions.md` — **every hit needs a
     board card.** Before deleting any shipped plan, create cards for its ratified
     follow-ups or they orphan.
   - Verify the durable rationale of any plan is captured in `syntax-decisions.md`
     **before** deleting the plan file. Sidequests is for *active* plans, not an
     archive — keep it clean.

2. **Building-stage items** → action with a subagent (no worktrees — the owner
   dislikes uncleaned worktrees; serialize anything touching `Source/`).

3. **Ready items** → if no plan, generate one. If it needs an owner decision,
   develop the decision, get it agent-reviewed, add it to the ballot, and move the
   card to **deciding**.

4. **Backlog items** → for each, run the plan pipeline:
   - **Write** (subagent, parallel — each writes its own file in
     `tools/Tower/docs/sidequests/<slug>.md`, so they never collide).
   - **Vet/refine** (a *different* subagent per plan): verify every claim against the
     codebase, enforce invariants I1–I8 and owner standards, refine in place.
   - **Develop decisions**: any user-facing choice becomes a ballot card (below).
     Plan-writers and vetters **propose** syntax; they never pick or ratify it.
   - Set the card's `plan` slug and move it to **deciding** (has a decision) or
     **ready** (none).

5. **Frozen** → leave it.

6. **Merge decisions into the ballot** yourself in one controlled write (single
   writer = no corruption) once vetters return ballot-ready cards.

## Decision-card house rules (or Tower can't show it / the owner can't trust it)

The owner decides from the card alone — he should never have to open the plan to
understand the choice. Tower v2's **Focus Mode** renders these as labeled facets,
so use the exact bold labels below. **Every card MUST carry all of these, or it is
not ballot-ready:**

1. **`**Gist:**`** — one VERY short plain-language sentence: what is being chosen.
   This is the big headline in Focus Mode; no jargon.
2. **`**Story.**`** — one short paragraph naming a real person (use an
   **American-traditional first name** — Betty, Hank, Walter, Doris, Earl…) and what
   they're doing, so the owner knows *why this decision exists* before he sees syntax.
3. **`**In the wild:**`** — a fenced ```jet block of realistic code from a plausible
   real project (not a toy) where this choice actually bites.
4. **`**Other languages:**`** — short fenced blocks showing how Rust/TS/Swift/etc.
   spell the same thing, **when a cross-language comparison is relevant** (skip if not).
5. **`**Tradeoffs:**`** — a compact table, one row per option, columns that actually
   differ (ceremony, failure mode, familiarity). Mark it **subagent-reviewed**: a
   second agent confirms each pro/con against the codebase before it reaches the owner.
6. **A worked example of *every* option** — each `- **Option X — <name>.**` bullet
   carries its own fenced ```jet (or ```shell) block showing what the real person
   types and sees (including the error they hit). No option may be described
   abstractly; if it's an option, it has a code block. Mark the recommended one
   `(recommended)` in its header so Focus Mode pills it.

Then close with `**Recommendation:**` and a one-line *why*. Any Owner Q&A goes in
`**Owner Q …**` blocks — Tower routes those to a separate Q&A facet, out of the
recommendation, so keep them there (not jammed into the rec).

- Format exactly: `### <ID> — <title> (rec X)`, then the labeled sections above, then
  the `- **Option A — <name>.**` bullets each with their worked example, then
  `**Recommendation:**`. No abstract option tables standing in for examples.
- **ID must be Tower-parseable**: `D-…` or `S<digits>-…` (the parser regex rejects
  `S-DBG1`-style ids). Check the new id does **not** collide with a ratified id:
  `rg "\bD-XXX\b" docs/spec/syntax-decisions.md`.
- Give a rich menu of original syntax candidates, never 2–3 derivative ones. Never
  invent syntax that contradicts a ratified decision — read `syntax-decisions.md`.
- **Implementation difficulty must never appear in a tradeoff column, an option
  ranking, or the recommendation** (philosophy.md → "Effort is never a deterrent").
  Rank options only on safety, beginner experience, performance, one-path, and
  long-term correctness. "Easier/faster to build" is not an advantage; "harder to
  build" is not a drawback. If a column like "ratification cost" or "effort" sneaks
  in, drop it.

## Hard constraints

- **board.json is owner-owned and live-edited.** Never regenerate/clobber it; make
  surgical edits or a targeted load-mutate-save of specific cards/fields only.
- **No git worktrees**, ever, unless the owner explicitly asks.
- **Lanes:** the owner may run a separate agent on `tools/Tower/docs/proposals/`
  (blue-sky proposals from `board.json` scratch). Plans (carded items) go to
  `sidequests/`; never write to `proposals/` when sweeping.
- Drain `tools/Tower/questions-queue.md`, `regen-queue.md`, and
  `ingest-queue.md` each sweep. **Ingest:** for each open item the owner handed
  Tower (a file path or pasted text, mirrored in `board.json` `ingest`), read the
  source, extract candidate ideas / features / syntax, file them as **frozen** cards
  (draft a ballot decision where a real choice exists), then check the item off and
  mark it `done` in `board.json`.

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
