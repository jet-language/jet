---
name: tower-burndown
description: >-
  Close Tower cards in 3–5-card batches of like items, with one batch reviewer,
  blocking-only findings capped at two recheck rounds, targeted proof per
  batch, and one full suite at end of scope. Implementation subagents only
  when the invocation grants them (subagents N); otherwise inline. Use when
  asked to burn down, close out, work the backlog, or when invoked as
  /tower-burndown. Executes work; ranking is tower-rank and prep is tower-prep.
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
/tower-burndown sidequests model grok subagents 5
```

| Arg | Meaning |
|---|---|
| *(none)* | **Sidequests first**, then `meta.currentEpoch` epoch-track |
| `epoch N` / `eN` | That epoch's epoch-track only |
| `sidequests` | Sidequest track only |
| `epoch N+sidequests` | Named epoch plus sidequests |
| `model <id>` | Pin every worker (and reviewer) to that model |
| `subagents N` | Permit up to N concurrent implementation subagents; hard max **10** |

**Subagent permission is invocation-granted.** The user says at invocation
whether implementation subagents are allowed and how many. No grant means
**zero implementation subagents**: the orchestrator implements every card
inline. The **batch reviewer subagent is exempt** — one fresh-context reviewer
per batch is always allowed and always used, granted or not.

Also honor plain language: “burn down epoch 3”, “continue burndown”, “close
the backlog”. Assume `workOrder` is already set unless the user also asks to
rank/prep or invokes those skills.

## Role split

**You are the orchestrator.** Keep context light.

- With a subagent grant: dispatch one-layer workers up to the granted count.
  **Workers must not spawn subagents.** Do not implement large or multi-file
  cards yourself; tiny mechanical work stays inline (single-file typo,
  snapshot bless, log-only board reconciliation).
- Without a grant: implement every card yourself, inline, batch by batch.
  Same quality bar; no stubs/facades either way.
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

Default to a declared batch of **3–5 cards**. Compose the batch for shared
cost, not just work order: like items, cards touching the same files or
mechanism, cards proved by the same test target. One build, one review, one
verification pass amortized across the batch. Never smear unrelated cards
into one blob just to fill five slots.

1. Name the 3–5 cards and their order before any work.
2. Implement every card in the batch before starting review.
   - With subagents: parallelize only disjoint paths; serialize shared
     mechanisms in card order.
   - Without subagents: implement sequentially inline, one shared tree.
   - A completed card may wait in `verify` for the batch reviewer. Do not start
     work beyond the declared batch while it waits.
3. Spawn **one fresh reviewer agent for the whole batch** (always permitted).
   That reviewer reviews every card separately, then the composed stack. Do
   not spawn one reviewer per card.
4. Route blocking findings to the original implementer (or fix inline). The
   same batch reviewer rechecks material fixes — see **Close discipline**.
5. Integrate the reviewed cards in work order.
6. Run **one grouped targeted verification pass** for the batch. Do not repeat
   the same targets in each worker, reviewer, and orchestrator.
7. Verify criteria, close all proved cards, remove worktrees/branches, then
   declare the next 3–5-card batch.

Use a smaller final batch only when fewer than three actionable cards remain or
genuine owner gates prevent filling it.

## Close discipline

Closing cards is the product; an endless review cycle is a failure mode equal
to shipping a stub.

- **Blocking findings only hold a card open**: broken acceptance criteria,
  invariant violations, correctness bugs, false-green tests. Style, taste,
  refactor ideas, and "could also" never block close — apply them only if
  trivial, otherwise drop them.
- **Two recheck rounds maximum** per batch. Round 1: fix all blockers,
  reviewer rechecks. Round 2: fix any remainder, reviewer rechecks the fixes
  only. After round 2, the orchestrator settles remaining items itself: fix a
  real blocker directly and close on targeted proof, or — only if it is
  genuinely new scope beyond the card's criteria — file it as its own card and
  close the original.
- A card that meets its written exit criteria with passing targeted proof
  **closes now**. Do not hold it for perfection, adjacent cleanups, or the
  full suite.
- If the same card bounces twice for the same root cause, stop cycling:
  re-read the criteria, fix it yourself end to end, close it.

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

**Shared build cache (mandatory for worktree workers).** Every worktree
worker exports `CARGO_TARGET_DIR=<main-clone>/target` so all streams share
one warm cache instead of cold-building a private `target/` (5–10 min each).
Cargo's own lock serializes concurrent builds safely; brief queueing beats
cold rebuilds. Never point the target dir at `/tmp` (RAM tmpfs).

**Worktrees persist across batches.** Keep a finished worker's worktree in
place for the next batch instead of delete/recreate; remove worktrees only at
end of burndown scope (or when a stream is permanently done). Branches still
merge and delete promptly — only the working directory and its build cache
stay warm.

## Concurrency

Concurrency exists only under a subagent grant. Without one, there is exactly
one stream: the orchestrator, inline. With a grant, run multiple concurrent
card streams **only when write paths and tests are disjoint**.

- Live workers never exceed the granted `subagents N` (hard max **10**).
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
and the composed stack. Findings follow **Close discipline**: blocking-only,
two recheck rounds.

After review and integration, run the union of required proof **once**:

- Prefer one grouped targeted command when it proves the batch. Cards close
  on that targeted proof.
- **The full suite runs once at the end of the burndown scope**, after the
  scoped cards are closed — not per batch, not per card. Run
  `scripts/agent/jet-env full scripts/agent/verify-full.sh` once, then fix
  what it surfaces: in-scope regressions get fixed immediately; unrelated
  failures become their own cards and do not reopen proved closures.
- Do not run targeted groups and then rerun the same groups through several
  agents. Do not restart a broad suite “to be safe.” If a run finds a concrete
  failure, fix the cause and rerun only the failing group.

**Work first, prove once (mandatory).** Compile time dominates burndown
wall-clock, so proof runs are batch-level, never card-level:

- A worker implements **every card in its brief before its first proof run**,
  then runs **one** combined command covering all of them (one cargo
  invocation, many `--test` targets), fixes what is red, and reruns **only
  the failing targets**. No per-card proof loops, no "quick check" runs
  between cards, no rerunning a suite that is already green on the same code.
- While implementing, a worker may run one red test **once** to confirm a
  failure exists before fixing it — never repeatedly.
- The orchestrator's integration proof is **one** run whose target union
  covers every branch merged since the last proof, not one run per branch.
- The reviewer reruns nothing that is green in evidence; spot-runs only on a
  concrete false-green smell, and at most once.
- Chain every proof as one shell command (`cargo test --test a --test b …`
  or one `bash -c` list) so one cargo lock, one build, one report serves the
  whole batch.

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
