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
find yourself reading `.tower/tower.json`, stop — it is frozen legacy.

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
reaches his phone (`--attach shot.png` for screenshots). Catch up after
gaps: `tower message list --unread --for claude-main`.

Keep `tower agent status --name claude-main --text "building #187 — tests
green"` fresh when switching tasks (shows live in his roster). A `[tower]`
system message ("N decisions ratified … greenlit: …") is ONE signal that the
board changed — run `tower next` once; do not fan out an agent per item.

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
3. **BALLOT FIRST — before any code on the card.** Enumerate every owner-gate
   it contains: new user-facing syntax, a new stdlib external dep (I6), an
   invariant carve-out, any owner-only approval. Queue EVERY gate as a
   ballot-ready decision NOW (tower-ballot skill / `jet-ballot` agent); gated
   card → `deciding`. Only then implement the ungated remainder — or move to
   the next card. A gate left as prose in a plan or log never reaches the
   owner; it MUST be a ballot.
4. Do the work per AGENTS.md: failing test first → spec → parser → sema →
   codegen → targeted tests while iterating → full suite green at the end →
   docs. Invariants I1–I8 hold. Delegate with the project agents (`jet-impl`
   implement, `jet-verify` verify, `jet-ballot` ballots) — caveman + rules are
   baked into them.
5. Advance honestly, with attribution and a log entry:
   `tower card update '#N' --phase building --log "…" --by claude-main`.
   `verify`→`done` only after real verification (verify skill; never trust a
   sub-agent's green — re-run the suite yourself). Finish EVERY exit
   criterion before touching the next card; "step N done" ≠ card done.
6. Release or leave a `[handoff]` log entry if you stop mid-card — cards are
   the handoff source of truth; harness task lists don't survive resets.
7. Report: message the owner what advanced + anything newly blocked on him.

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
- Once ratified, a decision leaves every open-ballot doc — decided clutter
  causes decision fatigue.

## When the owner ratifies

1. **Honor every word** — a comment or question inside a ratification is not
   a clean pick; address it explicitly.
2. Ratify into `docs/spec/syntax-decisions.md` (Ratified section + log); the
   board decision is already `ratified` with its `outcome`.
3. Reconcile the card: nothing else gates it → `building`, build now.
4. Implement end to end (standard above). When green, `done`.

## Rules

- Parallelise independent in-scope cards with sub-agents (`jet-impl` /
  `jet-verify` / `jet-ballot`; opus for design); one layer deep; no worktrees
  unless the owner asks. Checkpoint-commit before delegating.
- Always `--by <agent-name>` on writes; claims prevent double-work.
- `--expect-rev N` for read-modify-write races (exit 2 → re-read, retry).
- Don't close anything you haven't verified. If board and reality disagree,
  fix the board.
