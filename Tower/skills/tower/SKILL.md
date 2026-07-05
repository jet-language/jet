---
name: tower
description: Work the Tower project board — pick up agent-lane cards (plan / implement / verify), act on ratified decisions, answer the owner's questions and messages, and keep the board honest. Use when asked to "process tower", "work the board", "act on my decisions", "sweep the board", or when a task says to track work in Tower. The owner only ever does two things (decide, greenlight); this skill does everything that follows.
---

# Tower — work the board

Tower is the project's board plus a message line to the owner. All state lives
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

## Stay reachable — the message line

The owner messages you from the board's Agents view (often from a phone).
Pick a stable agent name (e.g. `claude-main`) and, at the start of a board
session, arm a listener in the background so each owner message wakes you:

- Under Claude Code: run `tower agent listen --name <me> --kind claude`
  inside the **Monitor** tool (persistent). Each incoming message arrives as
  a `[owner] …` line.
- Reply with `tower message send --to owner --text "…" --by <me>`
  (add `--card '#12'` when it concerns a card).

Treat owner messages like interrupts: answer or act, then resume. When you
finish a work item, a one-line report to the owner
(`tower message send --to owner …`) is how progress reaches their phone
(`--attach shot.png` for screenshots — images render inline).

Also: keep `tower agent status --name <me> --text "building #12 — tests
green"` fresh when you switch tasks (shows live in the owner's roster), and
treat a `[tower]` system message ("N decisions ratified … greenlit: …") as
one signal that the board changed — run `tower next` once, don't fan out
per item.

## Session loop

1. `tower status` · `tower question list --open` — answer questions first;
   `tower message list --unread --for <me>` — catch anything sent while away.
2. `tower next` — canonical picker: lowest `workOrder`, then building >
   verify > implement > plan. Respect `blockedBy`; never route around a gate.
3. Claim before working when other agents may be active:
   `tower card claim <#> --by <me>` (already claimed → pick another).
   Release with `tower card release <#> --by <me>` if you stop.
4. Do the work per the host repo's own conventions (its CLAUDE.md/AGENTS.md
   rule the *how*; Tower rules the *what/when*).
5. Advance with attribution:
   `tower card update <#> --phase building --log "started: X" --by <me>`.
   Phase honesty: `planning`→(`deciding` if decisions raised, else `ready`);
   `ready`→`building`; `building`→`verify` on claimed done; `verify`→`done`
   only after real verification. Never close what you haven't verified.
6. Report: message the owner what advanced and what's newly blocked on them.

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
