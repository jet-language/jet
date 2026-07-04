---
name: tower
description: Act on what the owner just recorded in Tower — implement ratified decisions, answer open card questions and messages, advance agent-lane cards (plan / implement / verify), and raise new decisions in ballot-ready form. When burndown is the goal, work only Epoch 3 + sidequest cards in workOrder until both sections are empty. Use after the owner records decisions or leaves notes in Tower, or when asked to "process tower", "act on my decisions", "do the tower work", "work the board", "sweep the board". The owner only ever does two things (decide, greenlight); this skill does everything that follows.
---

# Tower — act on the board

Tower moved (2026-07-04): the app lives at repo-root `Tower/`, the DATA lives
in `.tower/tower.json`. **Never edit the JSON by hand** — every operation
goes through the CLI (or the HTTP API of the server on port 7878):

```
alias tower='node /home/nate/Projects/Github/jet/Tower/tower.mjs'
tower help
```

The old `tools/Tower/` is retired; its board was imported losslessly. If you
find yourself reading `tools/Tower/tower.json`, stop — it is frozen legacy.

## The one rule that governs everything

**The owner's decisions are the only allowed bottleneck.** He must never wait
on you for a plan or a decision, and nothing reaches him that an agent hasn't
already reviewed. Do plans and decision-development eagerly; he only picks.

## The model

Every card computes to one **lane**. Owner lanes — never touch: `decide`,
`activate`, plus `frozen` cards. Your lanes: `plan` (write plan + raise
decisions), `implement`, `building`, `verify` (verify 100%, then close).
Epochs group the work; **milestones** are goals within an epoch (link cards
with `--milestone`). `tower state` = full JSON; `tower status` = summary.

## Stay reachable — the message line

The owner messages agents from the board (often from his phone). At session
start, arm a listener in the background via the Monitor tool:

```
tower agent listen --name claude-main --kind claude
```

Each owner message arrives as a `[owner] …` line and should be treated as an
interrupt: answer or act (`tower message send --to owner --text "…" --by
claude-main`), then resume. Report completions the same way — that's what
reaches his phone. Catch up after gaps: `tower message list --unread --for claude-main`.

## Scope & work order — Epoch 3 burndown

When the owner asks to work the board / burn down Epoch 3, stay inside:

1. **Epoch 3** — `track:"epoch"` + `epoch:"e3"` + agent lane.
2. **Sidequests** — `track:"sidequest"` + agent lane.

Do not wander into e4+, frozen, or owner lanes unless he redirects. Pick with
`tower next --epoch e3` / `--track sidequest` (workOrder ascending, then
building > verify > implement > plan). Respect `blockedBy`; never invent a
spelling to bypass a ratification gate. Exit criterion: both sections empty.

## Session loop

1. `tower status` · answer `tower question list --open` first · check unread messages.
2. `tower next` → claim it: `tower card claim '#N' --by claude-main` (claimed
   by someone else → pick another).
3. Do the work per CLAUDE.md: failing test first → spec → parser → sema →
   codegen → all tests green → docs. Invariants I1–I8 hold.
4. Advance honestly, with attribution and a log entry:
   `tower card update '#N' --phase building --log "…" --by claude-main`.
   `verify`→`done` only after real verification. Release if you stop.
5. Report: message the owner what advanced + anything newly blocked on him.

## Implementation standard — non-negotiable

"Implemented" = 100% end-to-end vertical slice, never a stub:
parser→sema→codegen wired and reachable from real `.jet` source; every new
diagnostic has a code in `docs/spec/diagnostics.md` **and** a `tests/ui`
snapshot (I4); runnable example with golden output where user-visible (I5);
`nix develop -c cargo test` fully green; docs match behavior. A ratified
decision may sit unbuilt **only** while gated on an unratified upstream
decision — the owner's answer on an unblocked decision IS the "go".

## Raising a decision — ballot-ready or it doesn't count

Follow the **tower-ballot** skill for the standard (gist / story / inWild /
worked options / comparisons / rec) and add via
`tower decision add --file ballot.json --by claude-main`. Jet-specific rules:

- ID must be Tower-parseable (`D-…` or `S<digits>-…`) and must not collide
  with a ratified id: `rg "\bD-XXX\b" docs/spec/syntax-decisions.md`.
- Never propose syntax that contradicts a ratified decision — read
  `docs/spec/syntax-decisions.md` first. Don't invent owner-facing syntax in
  code; raise the ballot and leave the card `deciding`.
- Implementation difficulty must never appear in a tradeoff or ranking
  (philosophy.md → "Effort is never a deterrent").

## When the owner ratifies

1. **Honor every word** — a comment or question inside a ratification is not
   a clean pick; address it explicitly.
2. Ratify into `docs/spec/syntax-decisions.md` (Ratified section + log); the
   board decision is already `ratified` with its `outcome`.
3. Reconcile the card: nothing else gates it → `building`, build now.
4. Implement end to end (standard above). When green, `done`.

## Rules

- Parallelise independent in-scope cards with sub-agents (sonnet impl, opus
  design); one layer deep; no worktrees unless the owner asks.
- Always `--by <agent-name>` on writes; claims prevent double-work.
- `--expect-rev N` for read-modify-write races (exit 2 → re-read, retry).
- Don't close anything you haven't verified. If board and reality disagree,
  fix the board.
