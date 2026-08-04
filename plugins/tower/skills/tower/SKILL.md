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

## The one rule that governs everything

**The owner's decisions are the only allowed bottleneck.** The owner must
never wait on you for a plan or a decision writeup. Full ballots receive every
required review pass; short ballots exist only on the owner's explicit request.
Do plans and decision development
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
   picks the top card by the canonical order (verify > building > implement >
   plan, then lowest `workOrder`; respects `blockedBy` — never route
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
   only after real **agent** verification. Never close what you haven't verified.
   If the card has a `criteria[]` checklist, meet each item as you finish it
   (`tower card criteria <#> --meet n --evidence "…" --by <me>`) and get a
   *different* agent to verify (`--verify n`) — the board refuses `--phase
   done` (`E_CRITERIA`) while any item is unverified, and refuses a verifier
   who is also the builder (`E_CRITERIA_SELF`).
   **Owner verification is not technical review.** Do not leave technical cards
   sitting in `verify` for the owner. Agents own machine proof and independent
   technical criteria verify, then `--phase done` themselves.
   Cards flagged `needsAcceptance` mint an owner accept/bounce ballot once the
   checklist is clean; the card waits in `verify` for that ratification, not
   `done`. Set `needsAcceptance` **only** for: visual/UI/UX/DX taste and design
   judgment; surfaces the harness cannot screenshot-judge; unavailable
   hardware/platforms/real environments. Never for tests, criteria, diffs,
   builds, or other machine-verifiable correctness. Give the owner only a brief
   observable look-and-feel checklist; omit machine-verification details.
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
the same lane-first order as plain `tower next`. Default **tower-burndown**
execution order is sidequests first, then the current epoch, unless the owner
names a different grouping.

Run `tower lint` before or after a sweep to catch durability rot the guards
don't: cards marked `done` with no verification evidence in the log, cards
claimed and idle 3+ days, events missing `by`, decisions that would fail the
ballot-ready gate, stale drafts, and dangling `blockedBy` refs. `--docs` also
flags a ratified decision id still sitting in `docs/ballots/*.md`. Exit code
1 means findings exist — fix them or raise a ballot, don't just clear the
board and move on.

## Papercuts — log tooling friction, don't push through it

When a tool wastes your time mid-task — a dead-end command, a broken helper, a
misleading doc or error, a stale cache — log it in one line and keep going:

```
tower papercut add --by me --text "jet-env swallowed stderr on failure" [--card '#N']
```

It is deliberately low-friction: only `--by` (non-owner) and non-empty text are
required, and it is never blocked by a frozen/decide card lane — logging must
never fail. Do **not** derail the task to fix the friction; the papercut is the
record. The owner reviews them on the **Papercuts** tab and clears handled ones
with `tower papercut resolve <id> --by owner`.

## Guards (agent-hard, owner-soft)

Writes with `--by` other than `owner` are gated; `--by owner` bypasses
everything (bypass event-logged). Full table in the plugin's `AGENTS.md`; headlines:

- `decision add` needs a plain-language ballot with
  gist/lesson/story/inWild/options[].code/rec plus structured recommendation
  reasons for the winner and every loser. Full ballots also need ordered base,
  boil-the-ocean, hybrid, cooperative, and adversarial summaries. Short ballots
  need the owner's quoted request and must omit reviews. Use the `simple` skill
  for every user-visible ballot field
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
