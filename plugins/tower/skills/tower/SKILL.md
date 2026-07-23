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
node ${CLAUDE_PLUGIN_ROOT}/tower.mjs help
```

If Tower is vendored in the repo instead of installed as a plugin, find the
  directory containing `tower.mjs` with `app/` beside it (commonly `Tower/` at
the repo root). Alias once per session: `alias tower='node <path>/tower.mjs'`.
No `plugins/tower/.tower/` yet → use the **tower-setup** skill.

## The one rule that governs everything

**The owner's decisions are the only allowed bottleneck.** The owner must
never wait on you for a plan or a decision writeup, and must never receive a
plan or ballot no agent has reviewed. Do plans and decision development
eagerly; the owner only picks. There is no greenlight/activate gate — a
fresh card lands straight in an agent lane; a ballot is the only way the
owner confirms anything.

## The model

Every card computes to exactly one **lane** — who owns the next move. Owner
lanes, **never touch**: `decide`, plus `frozen` cards. Your lanes:

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
   around a gate) and takes a renewable 24-hour work lease in the same step
   (someone else holds an active lease → `E_CLAIMED`, pick another with
   `tower brief '#N' --agent <me>`). Normal card writes by the holder renew
   it. Expired leases never block selection or takeover; done and frozen
   cards clear them.
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
   clean; the card waits in `verify` for that ratification, not `done`. Do not
   set it for technical correctness: agents own every machine-verifiable
   requirement, however many there are, plus the independent review. Use it only
   for unavailable hardware, platforms, or real environments; visual confirmation
   the harness cannot perform; or genuine UI, UX, or DX taste and design judgment.
   Give the owner only a brief observable checklist and why human inspection is
   needed; omit machine-verification details.
5. Report through the board itself: a `--log` entry on each card you advanced
   and a question/ballot for anything newly blocked on the owner — those are
   what the owner sees (live SSE UI — web push removed).

## Multi-card campaigns + durability sweep (#457)

Board semantics live here. Campaign roles split across sibling skills:

- **tower-rank** — ordered `workOrder` queue
- **tower-prep** — plans + ballots until ready or decide
- **tower-burndown** — orchestrated closeout with one-layer workers

`tower next --burndown` narrows the pool to `track:"epoch"` cards in
`meta.currentEpoch` plus every `track:"sidequest"` card, agent lanes only, in
the same `workOrder` order as plain `tower next`. Default **tower-burndown**
execution order is sidequests first, then the current epoch, unless the owner
names a different grouping.

Run `tower lint` before or after a sweep to catch durability rot the guards
don't: cards marked `done` with no verification evidence in the log, cards
claimed and idle 3+ days, events missing `by`, decisions that would fail the
ballot-ready gate, stale drafts, and dangling `blockedBy` refs. `--docs` also
flags a ratified decision id still sitting in `docs/ballots/*.md`. Exit code
1 means findings exist — fix them or raise a ballot, don't just clear the
board and move on.

## Guards (agent-hard, owner-soft)

Writes with `--by` other than `owner` are gated; `--by owner` bypasses
everything (bypass event-logged). Full table in the plugin's `AGENTS.md`; headlines:

- `decision add` needs a plain-language ballot with
  gist/lesson/story/inWild/options[].code/rec plus structured recommendation
  reasons for the winner and every loser
  or `E_BALLOT` — save unfinished work with `--draft`, finish later with
  `decision update <id> --ready`.
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
