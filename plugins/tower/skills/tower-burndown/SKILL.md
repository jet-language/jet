---
name: tower-burndown
description: >-
  Close Tower cards through milestone streams. Workers implement; the orchestrator
  integrates and closes cards on robust criteria evidence. At milestone end, one
  composed targeted sweep and one fresh-context review close the milestone. Use when
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
| `model <id>` | Pin every worker and milestone reviewer to that model |
| `subagents N` | Permit up to N concurrent implementation subagents; hard max **10** |

**Worker permission is invocation-granted.** The user says at invocation
whether implementation workers are allowed and how many. No grant means no
card implementation: the orchestrator does not absorb worker work. It may
plan, inspect, integrate existing patches, record state, or report the blocker.

Also honor plain language: “burn down epoch 3”, “continue burndown”, “close
the backlog”. Assume `workOrder` is already set unless the user also asks to
rank/prep or invokes those skills.

## Role split

**You are the orchestrator.** Keep context light.

- With a worker grant: dispatch one-layer workers up to the granted count.
  **Workers must not spawn subagents.** Do not implement card work yourself,
  including a small mechanical slice.
- Without a grant: do not implement cards. Report the missing worker grant or
  continue only with planning and board reconciliation.
- With a worker grant, keep at least one worker on the critical path (lowest
  ready `workOrder` / hardest blocker in scope).
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
| Closeout proof | host **verify** skill (criteria and milestone sweep rules) |
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
- Load and pass only the context needed for the assigned card or milestone. Return
  compact evidence instead of full command output.

## Reference index

| Need | Source |
|---|---|
| Claims, brief, criteria, phases | `../tower/SKILL.md` |
| Rank / reorder | `../tower-rank/SKILL.md` |
| Plans + ballots before build | `../tower-prep/SKILL.md` |
| Ballot authoring | `../tower-ballot/SKILL.md` |
| Project invariants, jet-env, review policy | nearest `AGENTS.md` |
| Criteria and milestone proof | `.agents/skills/verify/SKILL.md` or `.claude/skills/verify/SKILL.md` |

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

## Milestone stream

Select one milestone and its unblocked cards. Do not set a card-count target.
Compose the stream around the milestone's dependency order and disjoint worker
paths. Workers implement; the orchestrator integrates and closes.

1. Name the milestone, its cards, and their order before work starts.
2. Dispatch one-layer workers. Parallelize only disjoint paths and serialize
   shared mechanisms. Give each worker the full criteria and the evidence shape.
3. Inspect each worker return. Integrate a ready patch promptly. Record concrete
   evidence for every robust observable criterion and close the card immediately
   when the criteria are met and no known blocker contradicts the evidence.
4. Keep a card open when evidence is missing or a known blocker contradicts it.
   Route the fix to the owning worker and integrate the result.
5. Do not hold a card for a per-card reviewer, duplicate proof, or repeated
   fresh-context audit.
6. At milestone end, run one composed targeted test sweep over the milestone's
   gates and one fresh-context review of the integrated milestone diff. Include
   every applicable I9 execution tier.
7. Every finding reopens its owning card and affected criteria. Apply and
   integrate the fix, review the delta, verify the affected criteria, and close
   the card again. Close the milestone only after all findings are resolved and
   no known blocker remains.

## Close discipline

Closing cards is the product. A card closes immediately when all robust
observable exit criteria have concrete implementation evidence, the patch is
integrated, and no known blocker contradicts that evidence.

- No per-card reviewer, duplicate proof, or repeated fresh-context audit is a
  closure requirement.
- A missing criterion, an invariant violation, a correctness bug, false-green
  evidence, or any other known blocker that contradicts the evidence keeps the
  card open or reopens it.
- Style, taste, refactor ideas, and "could also" do not block closure unless a
  written criterion requires them.
- Milestone closeout findings always reopen the owning card and affected
  criteria. The owning worker applies the fix; the orchestrator integrates it,
  reviews the delta, verifies the affected criteria, and closes the card again.

## Dispatch brief (every worker)

Workers get zero ambiguity. Each brief states:

- Goal + definition of done (criteria text verbatim when present)
- Card `#N` (or grouped `#A+#B`), `--by` identity, claim rules
- Exact writable paths; everything else read-only
- Model + effort; **ponytail** + **caveman** + **simple** (for user-facing text)
- **No nested subagents. No stubs, facades, placeholders, or fake-green.**
- The minimum targeted red/green command needed while implementing; do not
  duplicate already-proved commands merely for reassurance
- Return criteria evidence and progress to the orchestrator; workers do not write Tower
- Worktree path/branch if used (must be under `.claude/worktrees/`); merge + remove before reporting done
- Return shape: commits, evidence for every criterion, tests run, handoff, blockers

Checkpoint the orchestrator's own dirty work before a write-capable worker
touches a shared tree. Prefer disjoint worktrees for parallel writers; merge
to the integration branch promptly; delete the worktree and temp branch after
integration. No orphaned trees.

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
  Use the single composed milestone sweep below for closeout proof.

Put these constraints in every worker brief that runs compiler/test commands.

**Per-worktree persistent build caches.** Do NOT share one
`CARGO_TARGET_DIR` across concurrent worktrees on diverged branches: streams
overwrite each other's dep artifacts and `target/debug/jet`, producing
wrong-binary smoke runs and phantom compile errors from another branch's
code (observed 2026-08-07). Each worktree keeps its own `target/`; the cache
stays warm because worktrees persist across milestones (below). Sharing the
main clone's target dir is safe only for a lone worker or branches at the
same base. Never point any target dir at `/tmp` (RAM tmpfs).

**Worktrees persist across milestones.** Keep a finished worker's worktree in
place for the next milestone instead of delete/recreate; remove worktrees only at
end of burndown scope (or when a stream is permanently done). Branches still
merge and delete promptly — only the working directory and its build cache
stay warm.

## Concurrency

Concurrency exists only under a worker grant. Without one, there is no card
implementation stream: the orchestrator does not implement. With a grant, run
multiple concurrent card streams **only when write paths and tests are disjoint**.

- Live workers never exceed the granted `subagents N` (hard max **10**).
- Record ownership before creating worktrees. Prefer in-repo worktrees
  (`.claude/worktrees/<name>`) for concurrent writers; one named close owner
  per stream. Never sibling folders beside the clone.
- Serialize (or contract) when streams share compiler seams, contend for build
  resources, or produce an integration backlog.
- Never `git add -A`. Never stage, commit, overwrite, or touch another stream's
  paths.
- Integrate one clean branch at a time. **Do not build an integration backlog.**
  A ready patch moves through integration and card closure as soon as its
  evidence is complete; it does not wait for milestone review.
- Cap retries: reject once or twice with a tighter brief; then escalate or
  re-scope.

## Milestone proof and review

Workers may run the commands named by their card criteria and return the
evidence. The orchestrator does not repeat those commands only for reassurance.
Card closure uses robust observable evidence, an integrated patch, and an honest
blocker check.

At milestone end, the orchestrator runs exactly one composed targeted test sweep
over the milestone's gates and one fresh-context review of the integrated
milestone diff. The sweep includes every applicable I9 execution tier. Include
broader targets only when the criteria or a known interaction requires them.

The fresh-context review checks acceptance criteria, diffs, invariants, false-green
risk, generated files, safety, and I9 parity. It does not implement. Every finding
reopens the owning card and affected criteria. The owning worker applies the fix;
the orchestrator integrates it, the reviewer reviews the delta, and the
orchestrator verifies the affected criteria before closing the card and milestone.

Do not restart a green command to be safe. If the composed sweep finds a concrete
failure, fix its cause and rerun only the affected target as part of the closeout.
Known blockers stay visible; never bless around them.

## Board honesty

- `tower brief '#N' --agent <worker>` (or claim) before build.
- Phase: ready → building → done after the orchestrator confirms robust criteria,
  concrete evidence, an integrated patch, and no known contradictory blocker.
- Criteria evidence and closure are orchestrator-owned. A per-card `--verify`
  step is not required. Use `verify` only for owner visual acceptance or an
  explicit closeout follow-up.
- **Owner verification ≠ technical verification.** Never put a technical card
  in the owner's Now/beacon queue. `needsAcceptance` only for visual/UI/UX/DX
  taste, design judgment, or real-world eyes the harness cannot replace.
- Release with `--handoff` if stopping mid-`building`.
- Log progress on the card; owner learns from the board, not a side channel.

## Stop / report

Keep burning until scope is empty, capacity ends, or only owner gates remain.
Final report (simple prose): cards closed/advanced, milestones shipped, tests and
closeout findings, open gates, live claims/worktrees left, suggested next
`/tower-burndown …` line.
