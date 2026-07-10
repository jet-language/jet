---
name: tower
description: Work the Tower project board — pick up agent-lane cards (plan / implement / verify), act on ratified decisions, answer the owner's questions, and keep the board honest. Use when asked to "process tower", "work the board", "act on my decisions", "sweep the board", or when a task says to track work in Tower. The owner only ever does two things (decide, greenlight); this skill does everything that follows.
---

# Tower — work the board

Tower is the project's board. All state lives
in `.tower/tower.json` in the project root, but you **never edit that file by
hand** — every operation goes through the Tower CLI (or the HTTP API of a
running `tower serve`).

```
node ${CLAUDE_PLUGIN_ROOT}/tower.mjs help
```

If Tower is vendored in the repo instead of installed as a plugin, find the
directory containing `tower.mjs` with `app/` beside it (commonly `Tower/` at
the repo root). Alias once per session: `alias tower='node <path>/tower.mjs'`.
No `.tower/` in the project yet → use the **tower-setup** skill.

## The one rule that governs everything

**The owner's decisions are the only allowed bottleneck.** The owner must
never wait on you for a plan or a decision writeup, and must never receive a
plan or ballot no agent has reviewed. Do plans and decision development
eagerly; the owner only picks.

## The model

Every card computes to exactly one **lane** — who owns the next move. Owner
lanes, **never touch**: `decide`, `activate`, plus `frozen` cards. Your lanes:

- `plan` — write a thorough plan + raise the decisions it needs (use the
  **tower-ballot** skill for the ballot standard)
- `implement` — plan vetted, decisions ratified: build it
- `building` — in progress; continue to completion
- `verify` — claimed done; verify 100%, then close

**Epochs** are the major groupings; **milestones** are goals within an epoch
(cards link via `milestoneId`; progress is computed). `tower state` returns
everything as JSON; `tower status` is the human summary.

## Session loop

1. `tower status` for the overview, then answer questions first
   (`tower question list --open`).
2. `tower brief --agent <me>` — one call replaces reading
   `status`/`next`/`card show`/`decision show`/`question list` separately:
   picks the top card by the canonical order (lowest `workOrder`, then
   building > verify > implement > plan; respects `blockedBy` — never route
   around a gate) and claims it in the same step (already claimed by someone
   else → `E_CLAIMED`, pick another with `tower brief '#N' --agent <me>`).
   The packet returned is everything needed to start: card, live blockers,
   full criteria checklist, every linked decision copied verbatim, open
   questions, refs, recent log, and the rules footer — no other reads
   needed. Omit `--agent`, or add `--no-claim`, to read without claiming.
   Release with `tower card release <#> --by <me>` if you stop — releasing a
   card that's `building` needs `--handoff "what's done, what's left,
   gotchas"` (`E_HANDOFF` otherwise) so the next agent isn't starting cold.
3. Do the work per the host repo's own conventions (its CLAUDE.md/AGENTS.md
   rule the *how*; Tower rules the *what/when*).
4. Advance with attribution:
   `tower card update <#> --phase building --log "started: X" --by <me>`.
   Phase honesty: `planning`→(`deciding` if decisions raised, else `ready`);
   `ready`→`building`; `building`→`verify` on claimed done; `verify`→`done`
   only after real verification. Never close what you haven't verified.
   If the card has a `criteria[]` checklist, meet each item as you finish it
   (`tower card criteria <#> --meet n --evidence "…" --by <me>`) and get a
   *different* agent to verify (`--verify n`) — the board refuses `--phase
   done` (`E_CRITERIA`) while any item is unverified, and refuses a verifier
   who is also the builder (`E_CRITERIA_SELF`). Cards flagged
   `needsAcceptance` mint an owner accept/bounce ballot once the checklist is
   clean; the card waits in `verify` for that ratification, not `done`.
5. Report through the board itself: a `--log` entry on each card you advanced
   and a question/ballot for anything newly blocked on the owner — those are
   what the owner sees (and gets push notifications for).

## Burndown scope + durability sweep (#457)

When the goal is burndown (work the board's current epoch to empty), scope
picks with `tower next --burndown` instead of hand-filtering by epoch: it
narrows the pool to `track:"epoch"` cards in `meta.currentEpoch` plus every
`track:"sidequest"` card, agent lanes only, same `workOrder` order as plain
`tower next`. Exit when that pool is empty.

Run `tower lint` before or after a sweep to catch durability rot the guards
don't: cards marked `done` with no verification evidence in the log, cards
claimed and idle 3+ days, events missing `by`, decisions that would fail the
ballot-ready gate, stale drafts, and dangling `blockedBy` refs. `--docs` also
flags a ratified decision id still sitting in `docs/ballots/*.md`. Exit code
1 means findings exist — fix them or raise a ballot, don't just clear the
board and move on.

## Guards (agent-hard, owner-soft)

Writes with `--by` other than `owner` are gated; `--by owner` bypasses
everything (bypass event-logged). Full table in Tower/AGENTS.md; headlines:

- `decision add` needs a full ballot (gist/story/inWild/options[].code/rec)
  or `E_BALLOT` — save unfinished work with `--draft`, finish later with
  `decision update <id> --ready`.
- `decision ratify` / `card activate` are owner-only (`E_OWNER_ONLY`) unless
  you pass `--quote "owner's words"` for an on-behalf-of action.
- Frozen and triage-phase-change writes are owner-only (`E_OWNER_LANE`);
  body/plan/log edits on a triage card are still fine.
- `card delete` refuses when a ratified decision is attached (`E_HAS_RATIFIED`)
  — it's a live decision, not a stub; let it retire (below) or restore+detach.
- `decision ratify --outcome` must match one of the decision's option keys.
- Own an owner ruling with `tower verdict '#N' --outcome "..." --by owner` —
  it mints a durable ratified decision instead of a log note that gets lost.

## Archive — history is separate from live

A done card, or a ratified decision, sits live for a walk-back buffer
(`config.retireAfterDays`, default 3 days) before it retires into
`.tower/history.json` — the owner sees it on Now's collapsed **Recently
decided** strip in the meantime and can reopen it in one tap. A card's own
decisions/questions stay live with it until the card retires, so no card
view is ever half-archived. `tower archive status|show <id>|restore <id>`
reads it back; `card show`/`decision show` fall through to history
automatically once something isn't live any more.

## Non-negotiables

- **Never edit `.tower/tower.json` directly** — the CLI/HTTP validate, lock,
  version, back up, and log; hand edits do none of that.
- Always pass `--by <me>` on writes.
- "Implemented" = fully functional end-to-end slice, never a stub.
- Owner lanes (`decide`, `activate`) and `frozen` are read-only to you.
- Concurrency: writes are lock-safe; for read-modify-write races pass
  `--expect-rev N` (exit 2 = conflict → re-read, retry).
- If board and reality disagree, fix the board — it's the handoff source of
  truth across sessions.
