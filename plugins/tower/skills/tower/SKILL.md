---
name: tower
description: Use Tower's board mechanics — inspect and claim agent-lane cards, act on ratified decisions, answer owner questions, update criteria and phases, and keep board state honest. Use when a task reads from or writes to Tower. For ranking use tower-rank; for plans/ballots use tower-prep; for closing cards use tower-burndown.
---

# Tower — work the board

Tower is the project's board. All state lives
in `plugins/tower/.tower/tower.json`, but you **never edit that file by
hand** — every operation goes through the Tower CLI (or the HTTP API of a
running `tower serve`).

```
# Jet (vendored) — always prefer this so the board stays in-repo:
node plugins/tower/tower.mjs help

# Plugin install (Claude Code / Cursor): ${CLAUDE_PLUGIN_ROOT} or the
# installed plugin directory containing tower.mjs + app/.
node ${CLAUDE_PLUGIN_ROOT}/tower.mjs help
```

Alias once per session: `alias tower='node <path>/tower.mjs'`.
No `plugins/tower/.tower/` yet → use the **tower-setup** skill.

## Contract

- **Requested outcome:** One requested Tower board operation, with state and authority kept truthful.
- **Supplied inputs:** The user's board request, fresh Tower state, the card/decision context, and the owner guide plus nearest project rules.
- **Allowed child result:** `tower-rank`, `tower-prep`, `tower-ballot`, `tower-burndown`, `tower-setup`, and `verify` may return only their declared queue, plan/ballot, closeout, setup, or proof result. They do not become a second board owner or create competing state.
- **Completion owner:** `tower` owns board reads and writes; owner guidance owns shared dispatch, proof, and closure authority.
- **Return point:** Each sibling handoff returns to the current board operation before the next phase or write.
- **Stopping condition:** Stop after the requested board operation is recorded and read back. Do not jump from planning to implementation, start a server for the owner, or hand-edit Tower state.

## The one rule that governs everything

**The owner's decisions are the only allowed bottleneck.** The owner must
never wait on you for a plan or a decision writeup. Full ballots receive the
current two-reader review; short ballots are the default for one mechanism with
at most three options. Use full for new syntax, invariant carve-outs, or cards
the owner marks full.
Do plans and decision development eagerly; the owner only picks. There is no
greenlight/activate gate — a fresh card lands straight in an agent lane; a
ballot is the only way the owner confirms anything.

## The model

Every card computes to exactly one **lane** — who owns the next move. Owner
lanes, **never touch**: `decide`, plus `frozen` cards. Your lanes:

- `plan` — write a thorough plan + raise the decisions it needs (use the
  **tower-ballot** skill for the ballot standard)
- `implement` — plan vetted, decisions ratified: build it
- `building` — in progress; continue to completion
- `verify` — owner visual acceptance or an explicit closeout follow-up; technical
  cards do not wait there for a per-card review

**Epochs** are the major groupings; **milestones** are goals within an epoch
(cards link via `milestoneId`; progress is computed). `tower state` returns
everything as JSON; `tower status` is the human summary.

## Session loop

1. `tower status` for the overview, then inspect open questions
   (`tower question list --open`). Block only affected slices; continue independent work.
2. `tower brief --agent <me>` picks the top card and takes a renewable
   24-hour work lease. `E_CLAIMED` means another agent owns it. `E_CLOSE_READY`
   means an actively claimed card already has every criterion met and no open
   gate: close, reopen, or block that card before briefing anything else.
   This barrier also applies to read-only briefs so work cannot silently move
   past closure.
   The packet contains card, live blockers, criteria, decisions, questions,
   refs, recent log, and rules. Release unfinished `building` work with
   `tower card release <#> --by <me> --handoff "done; left; gotchas"`.
3. Do the work per the host repo's own conventions (its CLAUDE.md/AGENTS.md
   rule the *how*; Tower rules the *what/when*). In orchestrated campaigns,
   workers return `CHECK OK` or `DOCS ONLY` and never write Tower.
4. The orchestrator advances with attribution:
   `tower card update <#> --phase building --log "started: X" --by <me>`.
   Phase honesty: `planning`→(`deciding` if decisions raised, else `ready`);
   `ready`→`building`; after integration and the exact focused proof, mark
   criteria `met`; then `building`→`done` as soon as every observable criterion
   has concrete evidence and no contradictory blocker remains. Record the final
   evidence, close immediately, then query `done` before another brief. No
   per-card reviewer, duplicate proof, broad suite, or separate technical
   verify step.
   **Owner verification is not technical review.** Do not leave technical cards
   sitting in `verify` for the owner. Use `verify` only when a card needs the owner's
   visual acceptance or an explicit closeout follow-up.
   Cards flagged `needsAcceptance` mint an owner accept/bounce ballot once the
   checklist is clean; the card waits in `verify` for that ratification, not
   `done`. Set `needsAcceptance` **only** for: visual/UI/UX/DX taste and design
   judgment; surfaces the harness cannot screenshot-judge; unavailable
   hardware/platforms/real environments. Never for tests, criteria, diffs,
   builds, or other machine-verifiable correctness. Give the owner only a brief
   observable look-and-feel checklist; omit machine-verification details.
   After all cards in a milestone close, commit the frozen source and run
   `scripts/agent/closeout-gate.mjs open <milestone> --by <me>`. Broad proof
   and `tower milestone verify` refuse without that commit-bound token.
5. The orchestrator reports through the board itself: a `--log` entry on each
   card advanced and a question/ballot for anything newly blocked on the owner.
   Workers report only through their receipt or handoff.

## Multi-card campaigns + durability sweep (#457)

Board semantics live here. Campaign roles split across sibling skills:

- **tower-rank** — ordered `workOrder` queue
- **tower-prep** — plans + ballots until ready or decide
- **tower-burndown** — orchestrated closeout with one-layer workers

`tower next --burndown` narrows the pool to `track:"epoch"` cards in
`meta.currentEpoch` plus every `track:"sidequest"` card, agent lanes only, in
the same lane-first order as plain `tower next`. Default **tower-burndown**
execution order is sidequests first, then the current epoch, unless the owner
names a different grouping.

Run `tower lint` before or after a sweep to catch durability rot the guards
don't: cards marked `done` with no criteria and integration evidence in the log, cards
claimed and idle 3+ days, events missing `by`, decisions that would fail the
ballot-ready gate, stale drafts, and dangling `blockedBy` refs. `--docs` also
walks `docs/spec/**` and reports card or decision IDs with no Tower record. Exit
code 1 means findings exist — fix them or raise a ballot, don't just clear the
board and move on.

## Papercuts — log *recurring* tooling friction, don't push through it

A papercut is friction in the agent toolchain that will hit the next agent too.
Log one only when one of these holds:

- the same snag hit you (or another agent) **at least twice**, or
- the cause is plainly deterministic — same command, same flags, same dead end
  for anyone who runs it.

```
tower papercut add --by me --text "jet-env swallowed stderr on failure" [--card '#N']
```

Not a papercut — do not log these:

- a one-off you cannot reproduce, or a single flaky failure;
- a state you caused: dirty tree, wrong cwd, stale binary you forgot to rebuild,
  a cache you then cleared;
- a collision with another session;
- friction you fixed in passing.

Not a papercut either: a bug in **Jet** — compiler, stdlib, examples, tests,
goldens. That is a card (`tower card add`). Papercuts cover the tools agents
drive Jet with: tower, `scripts/agent/*`, hooks, skills, agent docs.

Logging is deliberately low-friction: only `--by` (non-owner) and non-empty text
are required, and it is never blocked by a frozen/decide card lane — logging must
never fail. Do **not** derail the task to fix the friction; the papercut is the
record. The owner reviews them on the **Papercuts** tab and clears handled ones
with `tower papercut resolve <id> --by owner`.

## Guards (agent-hard, owner-soft)

Writes with `--by` other than `owner` are gated; `--by owner` bypasses
everything (bypass event-logged). Full table in the plugin's `AGENTS.md`; headlines:

- `decision add` needs a plain-language ballot with
  gist/lesson/story/inWild/options[].code/rec plus structured recommendation
  reasons for the winner and every loser. New full ballots need the base draft,
  a fresh-agent RLI5 beginner pass, and a separate fresh-agent adversarial pass.
  The adversarial reviewer may share the author's model family.
  Stored process 2/3 ballots retain their historical six-pass records.
  Short ballots are the default for one mechanism with at most three options,
  omit reviews, and need no `shortAuthorizedBy`. Use the `simple` skill for
  every user-visible ballot field or `E_BALLOT` — save unfinished work with
  `--draft`, finish later with `decision update <id> --ready`.
- `decision ratify` is owner-only (`E_OWNER_ONLY`) unless
  you pass `--quote "owner's words"` for an on-behalf-of action.
- Any write to a frozen card is owner-only (`E_OWNER_LANE`); the owner moves
  it out with a plain phase update.
- `card delete` refuses when a ratified decision is attached (`E_HAS_RATIFIED`)
  — it's a live decision, not a stub; let it retire (below) or restore+detach.
- `decision ratify --outcome` must match one of the decision's option keys.
- Own an owner ruling with `tower verdict '#N' --outcome "..." --by owner` —
  it mints a durable ratified decision instead of a log note that gets lost.

## Archive — history is separate from live

A done card, or a ratified decision, sits live for a walk-back buffer
(`config.retireAfterDays`, default 3 days) before it retires into
`plugins/tower/.tower/history.json` — the owner sees it on Now's collapsed **Recently
decided** strip in the meantime and can reopen it in one tap. A card's own
decisions/questions stay live with it until the card retires, so no card
view is ever half-archived. `tower archive status|show <id>|restore <id>`
reads it back; `card show`/`decision show` fall through to history
automatically once something isn't live any more.

## Minting law — the count must be honest

The board's open count is a promise. A stated remaining count that turns out to be far
higher is the same failure as one that turns out to be far lower. Before `tower card add`:

1. **Probe the named symptom once.** A cause already fixed closes or retargets the
   current card; it never creates a duplicate.
2. **Separate unrelated defects.** Add one deduplicated card and continue the current
   card. Fix the new defect immediately only when it blocks the current criterion or
   violates I1/I2 on that exact path.
3. **Retarget only the same persistent problem.** A named symptom still red for a new
   cause keeps its card. Do not absorb unrelated failures.
4. **One card per worker-sized mechanism.** Group only work one bounded worker can fix
   coherently with the same proof boundary.
5. **Mint only separate, uncovered work** — and say plainly that you did.

Never quote an unmeasured scope. A full-corpus census runs before a campaign or under
a milestone closeout token, never inside the active close-and-refill loop.

## Non-negotiables

- **Never edit `plugins/tower/.tower/tower.json` directly** — the CLI/HTTP validate, lock,
  version, back up, and log; hand edits do none of that.
- Always pass `--by <me>` on writes.
- "Implemented" = fully functional end-to-end slice, never a stub.
- Owner lane (`decide`) and `frozen` are read-only to you.
- Concurrency: writes are lock-safe; for read-modify-write races pass
  `--expect-rev N` (exit 2 = conflict → re-read, retry).
- If board and reality disagree, fix the board — it's the handoff source of
  truth across sessions.
