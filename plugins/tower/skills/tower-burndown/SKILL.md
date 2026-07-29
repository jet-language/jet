---
name: tower-burndown
description: >-
  Orchestrate closing Tower cards in efficient 3–5-card batches with one-layer
  implementers, one batch reviewer, one grouped verification, honest board
  updates, and prompt worktree integration. Use when asked to burn down, close
  out, work the backlog, or when invoked as /tower-burndown. Executes work;
  ranking is tower-rank and prep is tower-prep.
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
- Worktrees only under `<repo>/.claude/worktrees/<name>` (or
  `.agent-worktrees/`). Never create sibling `jet-*` / `jet-bd-*` folders
  beside the main clone. Relocate leaks immediately with `git worktree move`.

## No BS

- Ship the complete accepted behavior. No facades, stubs, placeholders,
  “follow-up will make it real,” fake-green tests, or report-only closeout.
- Do not shrink ratified scope because the real path is difficult. Do not add
  speculative machinery outside it.
- A card is not implemented when only its API shape, happy path, mock, or
  fallback exists. Close the production path end to end.
- Say exactly what is implemented, red, blocked, or deferred. Never launder a
  technical gap into owner acceptance.

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

## Token and prose discipline

Strive for token efficiency without cutting substance:

- Use **caveman** for agent-to-agent messages, status updates, handoffs, and
  review findings. Report decisions, evidence, hashes, and blockers; do not
  paste routine logs or retell context the recipient already has.
- Use **ponytail** throughout coding and review. Choose the shortest complete
  production path, reuse existing mechanisms, and avoid speculative
  abstractions. Minimal does not mean partial: no facades, stubs, or reduced
  acceptance scope.
- Use **simple** for every user-facing message and every user-visible artifact:
  documentation, Tower plans and logs, diagnostics, examples, commit and PR
  text, owner questions, progress reports, and final reports. Keep that prose
  clear and controlled; do not write user-facing text in caveman shorthand.
- Load and pass only the context needed for the assigned card or batch. Return
  compact evidence instead of full command output.

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
   named group with disjoint **paths and tests** from other live workers. Do
   not smear unrelated cards into one blob.
5. Prefer law/syntax/structure cards that later work builds on when redo risk
   is real; otherwise follow `workOrder` and critical path.

## Batch loop

Default to a declared batch of **3–5 cards in Tower work order**. Choose similar
cards when that saves setup, compilation, review, or verification work.

1. Name the 3–5 cards and their order before dispatch.
2. Implement every card in the batch before starting review.
   - Parallelize only disjoint paths.
   - Serialize shared mechanisms on stacked branches/worktrees in card order.
   - A completed card may wait in `verify` for the batch reviewer. Do not start
     work beyond the declared batch while it waits.
3. Spawn **one fresh reviewer agent for the whole batch**. That reviewer
   reviews every card separately, then the composed stack. Do not spawn one
   reviewer per card.
4. Route findings to the original implementers. The same batch reviewer
   rechecks material fixes and final conflict resolutions.
5. Integrate the reviewed cards in work order.
6. Run **one grouped verification pass** for the batch. Do not repeat the same
   targets in each worker, reviewer, and orchestrator.
7. Verify criteria, close all proved cards, remove worktrees/branches, then
   declare the next 3–5-card batch.

Use a smaller final batch only when fewer than three actionable cards remain or
genuine owner gates prevent filling it. Never pad a batch with unrelated work.

## Dispatch brief (every worker)

Workers get zero ambiguity. Each brief states:

- Goal + definition of done (criteria text verbatim when present)
- Card `#N` (or grouped `#A+#B`), `--by` identity, claim rules
- Exact writable paths; everything else read-only
- Model + effort; **ponytail** + **caveman** + **simple** (for user-facing text)
- **No nested subagents. No stubs, facades, placeholders, or fake-green.**
- The minimum targeted red/green command needed while implementing; do not
  duplicate already-proved commands merely for reassurance
- Tower update commands to run on progress (`criteria --meet`, `--log`, phase)
- Worktree path/branch if used (must be under `.claude/worktrees/`); merge + remove before reporting done
- Return shape: commits, tests run, criteria met, handoff, blockers

Checkpoint the orchestrator's own dirty work before a write-capable worker
touches a shared tree. Prefer disjoint worktrees for parallel writers; merge
to the integration branch promptly; delete the worktree and temp branch after
verify. No orphaned trees.

## Command hygiene (mandatory)

`scripts/agent/jet-env` can stall 10+ minutes in flake `shellHook` →
`clean-nix-tmp.sh` scanning `/proc/*/environ`. Piping only to `tail` hides
progress and looks hung. Every burndown command path must:

- Set `JET_NIX_TMP_CLEANED=1` after the first clean in the session (or always
  when re-entering nix shells repeatedly) so later `jet-env` invocations skip
  the scan.
- Prefer `nix develop` / `nix develop .#full` when the card needs host libs
  (e.g. `-lbz2`) instead of thrashing rustup-without-cc paths.
- Wrap long builds/tests with hard `timeout` budgets.
- Stream output or log to a file and poll — never wait on a long command piped
  solely to `tail`.
- Never re-run the same full-suite or other long command in a silent loop.
  Scoped proof first; full suite only at the batch boundary below.

Put these constraints in every worker brief that runs compiler/test commands.

## Concurrency

Safe parallelization is first-class. After the active sidequest is handled (or
when scope is already an epoch/sidequest slice), run multiple concurrent card
streams **only when write paths and tests are disjoint**.

- Default **3** live workers. User `max N` may raise up to **10**.
- Record ownership before creating worktrees. Prefer in-repo worktrees
  (`.claude/worktrees/<name>`) for concurrent writers; one named close owner
  per stream. Never sibling folders beside the clone.
- Serialize (or contract) when streams share compiler seams, contend for build
  resources, or produce an integration backlog.
- Never `git add -A`. Never stage, commit, overwrite, or touch another stream's
  paths.
- Integrate one clean branch at a time. **Do not start a new stream while a
  finished patch is waiting outside the declared batch.** Within a declared
  batch, completed patches wait in `verify` for the one batch reviewer.
- Cap retries: reject once or twice with a tighter brief; then escalate or
  re-scope.

## Proof and review (burndown policy)

One fresh reviewer agent reviews **all cards in the 3–5-card batch**. The
reviewer does not implement and does not rerun every implementer's tests.
Review acceptance terms, diffs, invariants, false-green risk, generated files,
and the composed stack. Fix findings; use the same reviewer for recheck.

After review and integration, run the union of required proof **once**:

- Prefer one grouped targeted command when it proves the batch.
- At a 3–5-card or major-push boundary, run
  `scripts/agent/jet-env full scripts/agent/verify-full.sh` once when required
  by the host `verify` skill or project `AGENTS.md`.
- Do not run targeted groups and then rerun the same groups through several
  agents. Do not restart a broad suite “to be safe.” If it finds a concrete
  failure, fix the cause and rerun only the failing group unless the host
  verification policy explicitly requires another full pass.

Never trust worker greens alone. The batch reviewer checks evidence; the
orchestrator checks final integration and grouped proof. Rebuild before `jet`
smoke when binaries matter. Known out-of-scope reds stay out of scope — fix or
card them, never bless around them.

## Board honesty

- `tower brief '#N' --agent <worker>` (or claim) before build.
- Phase: ready → building → verify → done only with real **agent** proof.
- Criteria: `--meet` with evidence; `--verify` by a different `--by`; then
  `--phase done` for technical cards — **do not wait on the owner**.
- **Owner verification ≠ technical verification.** Never put a technical card
  in the owner's Now/beacon queue. `needsAcceptance` only for visual/UI/UX/DX
  taste, design judgment, or real-world eyes the harness cannot replace.
- Release with `--handoff` if stopping mid-`building`.
- Log progress on the card; owner learns from the board, not a side channel.

## Stop / report

Keep burning until scope is empty, capacity ends, or only owner gates remain.
Final report (simple prose): cards closed/advanced, groups shipped, tests and
any full-suite run, open gates, live claims/worktrees left, suggested next
`/tower-burndown …` line.
