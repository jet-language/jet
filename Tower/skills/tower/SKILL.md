---
name: tower
description: Work the Tower project board — implement ratified decisions, answer open card questions, advance agent-lane cards (plan / implement / verify), raise new decisions in ballot-ready form, and keep the board honest. Use when asked to "process tower", "work the board", "act on my decisions", "sweep the board", or when a task says to track work in Tower. The owner only ever does two things (decide, greenlight); this skill does everything that follows.
---

# Tower — act on the board

Tower is the project's board. All state lives in `.tower/tower.json` in the
project root, but you **never edit that file by hand** — every operation goes
through the Tower CLI (or the HTTP API of a running `tower serve`).

```
node ${CLAUDE_PLUGIN_ROOT}/tower.mjs help
```

If Tower is vendored into the repo instead of installed as a plugin, find it:
the directory containing `tower.mjs` with an `app/` beside it (commonly
`Tower/` at the repo root). Alias it once per session:
`alias tower='node <path>/tower.mjs'`. If no `.tower/` exists and the user
asked you to set Tower up, run `tower init --name "<Project>"`.

## The one rule that governs everything

**The owner's decisions are the only allowed bottleneck.** The owner must
never wait on you for a plan or a decision writeup, and must never receive a
plan or decision no agent has reviewed. Do plans and decision-development
eagerly; the owner only picks.

## The model

Every card computes to exactly one **lane** that says who owns the next move.
Owner lanes — **never touch**: `decide` (open decisions), `activate`
(greenlight), plus `frozen` cards. Your lanes:

- `plan` — write a thorough plan for the card + raise the decisions it needs
- `implement` — plan vetted, decisions ratified: build it
- `building` — in progress; continue to completion
- `verify` — claimed done; verify 100%, then close

Plus: any open question (`tower question list --open`) is the owner asking you
something — answer it first, it often changes what to build.

Structure: **epochs** are the major groupings; **milestones** are goals within
an epoch (cards link to one via `milestoneId`; progress is computed from their
done-ratio). `tower state` returns everything as JSON.

## Session loop

1. `tower status` — see counts; `tower question list --open` — answer these first.
2. `tower next` — the canonical picker: lowest `workOrder` first, then
   building > verify > implement > plan. Respect `blockedBy`; never invent a
   way around a blocker.
3. Claim before working when other agents may be active:
   `tower card claim <#> --by <your-name>` (fails if already claimed — pick
   another). Release with `tower card release <#> --by <your-name>` if you stop.
4. Do the card's work following the host project's own conventions
   (CLAUDE.md / AGENTS.md of the host repo rule the *how*; Tower only rules
   the *what/when*).
5. Advance as you go — always with `--by <your-name>` and a log entry:
   `tower card update <#> --phase building --log "started: <what>" --by <name>`
   Phase honesty: `planning`→(`deciding` if you raised decisions, else
   `ready`); `ready`→`building`; `building`→`verify` when you claim done;
   `verify`→`done` only after real verification. Never close what you haven't
   verified.
6. Report: what you implemented, what you answered, which cards advanced, and
   anything newly blocked on the owner — that list is all they should need to
   look at next.

## Raising a decision — ballot-ready or it doesn't count

Any owner-facing choice becomes a decision on its card, made in the UI's
focus mode from these fields — fill them all:

- `gist` — one very short plain-language sentence: what is being chosen.
- `story` — a short paragraph naming a real person and what they're doing, so
  the owner knows why this decision exists before seeing details.
- `inWild` — a realistic code/usage example from a plausible real project
  where the choice actually bites (rendered syntax-highlighted).
- `comparisons` — `[{lang, note, code}]`: how other tools/languages/products
  spell the same thing, when relevant.
- `options[]` — `{key, name, detail, code}` for **every** option, each with
  its own worked example showing what the person types and sees. No option
  described only abstractly. Give a rich menu, not 2–3 derivative picks.
- `rec` — the recommended option key + a one-line why.

Add via JSON payload (stdin or file):

```
echo '{"cardId":"#12","id":"D-CACHE1","title":"Cache invalidation strategy",
  "gist":"How cached results expire","story":"...","inWild":"...",
  "options":[{"key":"A","name":"TTL","detail":"...","code":"..."}],
  "rec":"A"}' | tower decision add --file - --by <name>
```

Then leave the card in `deciding` — the ratify is the owner's. When the owner
ratifies (`status:"ratified"` + `outcome`), honor every word of any comment
they attached, then implement end-to-end.

## Non-negotiables

- **Never edit `.tower/tower.json` directly.** CLI/HTTP only — they validate,
  lock, version (`rev`), back up, and log events; hand edits do none of that.
- Always pass `--by <your-agent-name>` on writes so the event log stays honest.
- "Implemented" means a fully functional end-to-end slice per the host
  project's definition of done — never a stub. Difficulty is not an argument.
- Owner lanes (`decide`, `activate`) and `frozen` cards are read-only to you.
- Concurrent writes are safe (lock + atomic writes). For read-modify-write
  races use `--expect-rev N` (exit code 2 on conflict → re-read and retry).
- If the board and reality disagree, fix the board (log entry, correct phase)
  — the board is the handoff source of truth across sessions.
