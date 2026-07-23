---
name: tower-burndown
description: >-
  Orchestrate closing Tower cards with one-layer subagents: claim, implement,
  verify, update board, merge worktrees. Use when asked to burn down, close out,
  work the backlog, or when invoked as /tower-burndown. Executes work; ranking
  is tower-rank and prep is tower-prep.
---

# Tower — burn down cards

Orchestrate real closeout. Rank is **tower-rank**. Plans/ballots are
**tower-prep**. This skill does the work.

## Triggers and args

```
/tower-burndown
/tower-burndown epoch 3
/tower-burndown e3
/tower-burndown sidequests
/tower-burndown epoch 3+sidequests
/tower-burndown epoch 3 model gpt-5.6-sol
/tower-burndown sidequests model grok max 5
```

| Arg | Meaning |
|---|---|
| *(none)* | **Sidequests first**, then `meta.currentEpoch` epoch-track |
| `epoch N` / `eN` | That epoch's epoch-track only |
| `sidequests` | Sidequest track only |
| `epoch N+sidequests` | Named epoch plus sidequests |
| `model <id>` | Pin every worker (and reviewer) to that model |
| `max N` | Concurrent workers; default **3**, hard max **10** |

Also honor plain language: “burn down epoch 3”, “continue burndown”, “close
the backlog”. Assume `workOrder` is already set unless the user also asks to
rank/prep or invokes those skills.

## Role split

**You are the orchestrator.** Keep context light.

- Dispatch one-layer workers. **Workers must not spawn subagents.**
- Do not implement large or multi-file cards yourself.
- Exception: tiny mechanical work when spinning a worker would waste more
  tokens than doing it inline (single-file typo, snapshot bless, log-only
  board reconciliation). Still no stubs/facades.
- Always keep at least one worker on the critical path (lowest ready
  `workOrder` / hardest blocker in scope).
- Never hand-edit `plugins/tower/.tower/*.json`. Tower CLI only.
- Never clobber other agents' claims, dirty paths, or worktrees. Inspect
  `git status`, `git worktree list`, and claims before dispatch.

## Forced skills (every participant)

| Surface | Skill |
|---|---|
| Agent chatter / status | **caveman** (full unless user sets another level) |
| Implementation | **ponytail** (lazy ladder; smallest complete solution) |
| User-visible prose | **simple** — card plans/logs, ballots, docs, commit/PR bodies, owner reports |
| Board mechanics | **tower** |
| Closeout proof | host **verify** skill (targeted vs full-suite rules) |
| Owner gates mid-flight | **tower-ballot**; stop that slice, burn ungated work |

Pass these by name in every worker brief. If `model` was set, pass that model
into every Task/subagent spawn. Effort: medium default; low for mechanical;
high for hard semantics/architecture/debug.

## Reference index

| Need | Source |
|---|---|
| Claims, brief, criteria, phases | `../tower/SKILL.md` |
| Rank / reorder | `../tower-rank/SKILL.md` |
| Plans + ballots before build | `../tower-prep/SKILL.md` |
| Ballot authoring | `../tower-ballot/SKILL.md` |
| Project invariants, jet-env, review policy | nearest `AGENTS.md` |
| Scoped vs full-suite proof | `.agents/skills/verify/SKILL.md` or `.claude/skills/verify/SKILL.md` |

## Pick work

1. `tower status` + open questions.
2. Build the actionable queue from live board (not stale chat):
   - Default: sidequest agent-lane cards by `workOrder`, then current-epoch
     epoch-track agent-lane cards by `workOrder`.
   - Explicit scope overrides the default.
3. Skip `decide`, `frozen`, `done`, foreign active claims, and unfinished
   `blockedBy` predecessors.
4. **Group** when it cuts duplication: same files, same mechanism, or a thin
   umbrella whose criteria are the same vertical slice. One worker may own a
   named group with disjoint paths from other live workers. Do not smear
   unrelated cards into one blob.
5. Prefer law/syntax/structure cards that later work builds on when redo risk
   is real; otherwise follow `workOrder` and critical path.

## Dispatch brief (every worker)

Workers get zero ambiguity. Each brief states:

- Goal + definition of done (criteria text verbatim when present)
- Card `#N` (or grouped `#A+#B`), `--by` identity, claim rules
- Exact writable paths; everything else read-only
- Model + effort; **ponytail** + **caveman** + **simple** (for user-facing text)
- **No nested subagents. No stubs, facades, placeholders, or fake-green.**
- Exact targeted test commands (`scripts/agent/jet-env cargo test --test …`)
- Tower update commands to run on progress (`criteria --meet`, `--log`, phase)
- Worktree path/branch if used; merge + remove before reporting done
- Return shape: commits, tests run, criteria met, handoff, blockers

Checkpoint the orchestrator's own dirty work before a write-capable worker
touches a shared tree. Prefer disjoint worktrees for parallel writers; merge
to the integration branch promptly; delete the worktree and temp branch after
verify. No orphaned trees.

## Concurrency

- Default **3** live workers. User `max N` may raise up to **10**.
- Serialize on path / generated-artifact / test-resource collisions.
- Do not start a new stream while a finished patch is waiting to integrate.
- Cap retries: reject once or twice with a tighter brief; then escalate or
  re-scope.

## Proof and review (burndown policy)

Owner override for this skill (outranks generic “review everything” habits):

| Kind | Proof |
|---|---|
| Covered by targeted tests / golden / criteria evidence | Independent **reviewer subagent not required**. Meet criteria with evidence; when the card has criteria, `--verify` still needs a **different** agent identity than `--meet` (board `E_CRITERIA_SELF`). Orchestrator may be the verifier when they did not build. |
| High-impact / hard / architectural / safety-sensitive | Spawn a fresh reviewer (same pinned `model` if set). Reviewer does not implement. Fix findings; recheck. |
| Batch / major milestone | After **3–5** integrated closures, or at a major-push boundary, orchestrator runs `scripts/agent/jet-env full scripts/agent/verify-full.sh` once. Workers never run the full suite “to be safe.” |

Never trust a worker's green alone for closeout: re-read diff scope, confirm
named tests, spot-check evidence. Rebuild before `jet` smoke when binaries
matter. Known out-of-scope reds stay out of scope — fix or card them, do not
bless around them.

## Board honesty

- `tower brief '#N' --agent <worker>` (or claim) before build.
- Phase: ready → building → verify → done only with real proof.
- Criteria: `--meet` with evidence; `--verify` by a different `--by`.
- `needsAcceptance` only for real owner taste/hardware/visual checks — never
  for machine-verifiable technical correctness.
- Release with `--handoff` if stopping mid-`building`.
- Log progress on the card; owner learns from the board, not a side channel.

## Stop / report

Keep burning until scope is empty, capacity ends, or only owner gates remain.
Final report (simple prose): cards closed/advanced, groups shipped, tests and
any full-suite run, open gates, live claims/worktrees left, suggested next
`/tower-burndown …` line.
