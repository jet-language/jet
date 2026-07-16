---
name: tower
description: Act on what the owner just recorded in Tower — implement ratified decisions, answer open card questions, advance agent-lane cards (plan / implement / verify), and raise new decisions in ballot-ready form. When burndown is the goal, work only the board's current epoch + sidequest cards in workOrder until both sections are empty. Use after the owner records decisions or leaves notes in Tower, or when asked to "process tower", "act on my decisions", "do the tower work", "work the board", "sweep the board". The owner only ever does one thing (decide); this skill does everything that follows.
---

# Tower — act on the board

Tower moved (2026-07-04): the app lives at repo-root `Tower/`, the DATA lives
in `.tower/tower.json`. **Never edit the JSON by hand** — every operation
goes through the CLI (or the HTTP API of the server on port 7878):

```sh
alias tower='scripts/agent/jet-env node Tower/tower.mjs'
tower help
```

## The one rule that governs everything

**The owner's decisions are the only allowed bottleneck.** He must never wait
on you for a plan or a decision, and nothing reaches him that an agent hasn't
already reviewed. Do plans and decision-development eagerly; he only picks.
There is no greenlight/activate gate — a fresh card lands straight in an
agent lane; a ballot is the only way the owner confirms anything.

## The model

Every card computes to one **lane**. Owner lanes — never touch: `decide`,
plus `frozen` cards. Your lanes: `plan` (write plan + raise
decisions), `implement`, `building`, `verify` (verify 100%, then close).
Epochs group the work; **milestones** are goals within an epoch (link cards
with `--milestone`). `tower state` = full JSON; `tower status` = summary.

## Scope & work order — current-epoch burndown

When the owner asks to work the board or burn down the current epoch, read
`meta.currentEpoch` from Tower and stay inside:

1. **Current epoch** — `track:"epoch"` + `epoch:meta.currentEpoch` + agent lane.
2. **Sidequests** — `track:"sidequest"` + agent lane.

Do not wander into another epoch, frozen, or owner lanes unless he redirects. Pick with
`tower next --burndown` (workOrder ascending, then building > verify >
implement > plan) — the canonical burndown loop: scopes to `meta.currentEpoch`
epoch-track cards plus every sidequest, agent lanes only, in one filter
(#457). Respect `blockedBy`; never invent a spelling to bypass a
ratification gate. Exit criterion: both sections empty.

Sweep for durability rot with `tower lint` before/after a burndown pass
(#457): done cards with no verification evidence, claimed-idle cards,
unattributed events, ballot-gap decisions, stale drafts, orphan `blockedBy`
refs. `--docs` also flags a ratified decision id still listed in
`docs/ballots/*.md`. Exit 1 on any finding, 0 clean — fix or ballot what it
surfaces, don't silence it.

## Session loop

1. `tower status` for the overview, then answer any open questions first
   (`tower question list --open`).
2. `tower brief --agent <agent-name>` — one call replaces reading
   `status`/`next`/`card show`/`decision show`/`question list` separately:
   picks the top card (or `tower brief '#N' --agent <agent-name>` for a
   specific one) and claims it in the same step (claimed by someone else →
   `E_CLAIMED`, pick another). The packet is everything needed to start:
   card, live blockers, criteria, every linked decision verbatim, open
   questions, refs, recent log, and board rules. Inspect the relevant source,
   tests, and only the authoritative spec sections the card triggers.
3. **BALLOT FIRST — before any code on the card.** Enumerate every owner-gate
   it contains: new user-facing syntax, a new stdlib external dep (I6), an
   invariant carve-out, any owner-only approval. Queue EVERY gate as a
   ballot-ready decision NOW (tower-ballot skill / `jet-ballot` agent); gated
   card → `deciding`. Only then implement the ungated remainder — or move to
   the next card. A gate left as prose in a plan or log never reaches the
   owner; it MUST be a ballot. `decision add` refuses an incomplete ballot
   (`E_BALLOT` — missing gist/lesson/story/inWild/options[].code/rec or the
   structured recommendation); dense plain-language fields also fail. Save an
   unfinished one with `--draft`, finish later with `decision update <id>
   --ready`.
4. Do the work per AGENTS.md with `ponytail:ponytail`: failing test first →
   smallest complete vertical implementation → scoped targeted tests → docs →
   sequential Sol and Terra reviews. Only the
   orchestrator runs the full suite, once after a major push on its closeout or
   blocking card; CI runs it again. Invariants I1–I8 hold. Delegate with the
   project agents (`jet-impl` implement, `jet-verify` review, `jet-ballot`
   ballots) when available. One implementer owns each coherent patch.
5. Advance honestly, with attribution and a log entry:
   `tower card update '#N' --phase building --log "…" --by claude-main`.
   `verify`→`done` only after sequential fresh Sol and Terra verification (verify skill;
   never trust a builder's green). Finish EVERY exit
   criterion before touching the next card; "step N done" ≠ card done.
   Cards with a machine-checked `criteria[]` list: meet each item as you land
   it (`tower card criteria '#N' --meet n --evidence "…" --by claude-main`),
   log the Sol review, then get the Terra reviewer to record the final
   independent criterion verification (`--verify n`) —
   `--phase done` is refused (`E_CRITERIA`) while any item is unverified, and
   refused (`E_CRITERIA_SELF`) if the verifier is also the builder. A card
   flagged `needsAcceptance` mints an owner accept/bounce ballot once its
   checklist is clean; it sits in `verify` until the owner ratifies — that's
   not a bug, don't force it to `done`.
6. Release or leave a `[handoff]` log entry if you stop mid-card — cards are
   the handoff source of truth; harness task lists don't survive resets.
   `card release` on a `building` card requires `--handoff "…"` (`E_HANDOFF`
   otherwise) so the next session doesn't restart from zero.
7. Report on the board itself: log entries on advanced cards, ballots/
   questions for anything newly blocked on him — that's what he sees (and
   gets push notifications for).

## Implementation standard — non-negotiable

"Implemented" = 100% end-to-end vertical slice, never a stub:
parser→sema→codegen wired and reachable from real `.jet` source; every new
diagnostic has a code in `docs/spec/diagnostics.md` **and** a `tests/ui`
snapshot (I4); runnable example with golden output where user-visible (I5);
  scoped targeted proof passes and fresh Sol then Terra reviewers clear it;
docs match behavior. Major-push closeout additionally requires the orchestrator's one full
suite run. A ratified
decision may sit unbuilt **only** while gated on an unratified upstream
decision — the owner's answer on an unblocked decision IS the "go".

## Raising a decision — ballot-ready or it doesn't count

Follow the **tower-ballot** skill for the standard (plain-language gist / lesson /
story / worked options / comparisons / rec + why / why-not / accepted tradeoff)
and add via
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

Ratifying a `group: "syntax"` decision auto-appends the standard
post-ratification chores to the card's `criteria[]` (Syntax.rs entry,
syntax-decisions.md log,
`scripts/agent/jet-env jet self devtools grammars`, re-bless) — meet/verify
them like any other exit criterion.

## Guards — agent-hard, owner-soft

`decision ratify` is owner-only (`E_OWNER_ONLY`) for any
`--by` other than `owner` — pass `--quote "owner's words"` only for a genuine
on-behalf-of action (recorded in the event log). Any write to a frozen card
is owner-only (`E_OWNER_LANE`; the owner moves it out with a plain phase
update). `card delete` refuses when a ratified decision
is attached (`E_HAS_RATIFIED`). `decision ratify --outcome` must match one of
the decision's option keys. `--by owner` bypasses every guard here (bypass
event-logged) — full table in Tower/AGENTS.md.

Record an owner ruling with `tower verdict '#N' --outcome "..." [--title
"…"] --by owner` — it mints an already-ratified decision on the card instead
of a log note, so a verdict is never lost across a context reset.

## Archive — history is separate from live (#461)

A done card, or a ratified decision, sits live for a walk-back buffer
(`config.retireAfterDays`, default 3 days) before it retires into
`.tower/history.json` — he sees it on Now's collapsed **Recently decided**
strip meanwhile and can reopen it in one tap. A card's own decisions/
questions stay live with it until the card retires, so no card view is ever
half-archived. `tower archive status|show <id>|restore <id>` reads it back;
`card show`/`decision show` fall through to history once something isn't
live any more. `card delete`'s `E_HAS_RATIFIED` refusal doesn't need a
manual detach any more — let the decision retire on its own, or
`tower archive restore` it first if you need it back sooner.

## Rules

- Use GPT-5.6 Sol by default and tune its effort. Terra is the required second
  reviewer and is otherwise used only for a recorded task-specific advantage.
- One implementer owns a coherent change. Review in order: fresh Sol, fixes and
  recheck, then fresh Terra, fixes and recheck. Reviewers never implement.
- Parallelise only disjoint in-scope cards; one agent layer. Worktrees are
  allowed for write isolation, but successful work must be integrated promptly
  and every task-created worktree and temporary branch removed before close.
- Never checkpoint with `git add -A`. Stage and commit only paths owned by the
  task; never sweep another agent's work into a commit.
- Always `--by <agent-name>` on writes; claims prevent double-work.
- `--expect-rev N` for read-modify-write races (exit 2 → re-read, retry).
- Don't close anything you haven't verified. If board and reality disagree,
  fix the board.
