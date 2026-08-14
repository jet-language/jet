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
| `subagents N` | Permit up to N concurrent implementation workers; hard max **5** |

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
into every worker spawn. Use medium reasoning for mechanical changes, high for
normal semantic fixes, and max only for one narrow root-cause problem after a
concrete failed proof.

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

Finish one milestone before opening another. Keep about five lanes only when
their writable paths are disjoint. Shared compiler seams get one implementation
lane.

1. Name the milestone, its cards, and their dependency order before work starts.
2. Dispatch bounded slices, not broad card audits. A slice names one mechanism,
   one concrete failure or criterion set, exact writable paths, and the expected
   observable result.
3. Integrate each clean patch as soon as it arrives. Advance the persistent
   builder once, then run the smallest proof that can reject that patch.
4. Give an exact failing command and decisive output to a correction worker.
   Do not resend the whole card or ask the worker to rediscover the failure.
5. Record evidence and close a card only after the integrated builder proves
   every changed observable criterion. Source inspection supports structural
   criteria; it never substitutes for runtime, tier, snapshot, or golden proof.
6. Refill a lane only after its prior patch is integrated, rejected, or reduced
   to one explicit blocker. Never create an integration backlog.
7. After every milestone card is done, run one composed targeted sweep and one
   fresh-context review. Include every applicable I9 execution tier.
8. Each finding reopens its owning card and criteria. Fix only the finding,
   verify the affected targets, review the correction delta, and close again.

## Close discipline

Closing cards is the product. A card closes immediately when all robust
observable exit criteria have concrete evidence from the integrated tree and no
known blocker contradicts that evidence.

- Worker claims are leads, not proof. A source-only worker cannot claim a
  runtime, tier, snapshot, golden, or generated-artifact criterion passed.
- The persistent builder runs each required changed-contract proof once. Do not
  duplicate it for reassurance.
- A missing criterion, invariant violation, correctness bug, false-green
  result, or stale expected output keeps the card open or reopens it.
- Style, taste, refactor ideas, and "could also" do not block closure unless a
  written criterion requires them.
- Milestone findings route back as narrow corrections. Do not launch another
  whole-card implementation or a second milestone review.

## Dispatch brief (every worker)

Workers get one bounded job with zero ambiguity. Each brief states:

- Card and exact criterion or failed proof owned by this slice
- The concrete failure, actual output, expected behavior, and last integrated
  commit
- Exact writable paths; all other paths are read-only
- One mechanism only; no broad audit, milestone review, or unrelated cleanup
- Model and effort: medium mechanical, high normal, max only after a failed
  high-reasoning root-cause attempt
- **ponytail** + **caveman** + **simple**; no nested subagents, stubs, facades,
  placeholders, compatibility paths, or fake-green
- Source-only workers run no cargo, Jet, formatter, linter, or generator command
- Return only commit hash, changed paths and lines, source evidence, blockers,
  and clean status; never report an unrun proof as PASS

Launch bounded workers with a hard wall-clock limit. Default: 12 minutes for a
normal correction and 5 minutes for a mechanical fixture change. A genuinely
hard semantic slice may get 20 minutes after a concrete failing repro. At the
limit, cancel, salvage a coherent commit or diff, and rebrief a smaller slice.

Checkpoint owned dirty work before dispatch. Integrate or reject each result
before refilling its lane. Remove disposable worktrees and their targets after
integration; never park completed work.

## Command and builder hygiene (mandatory)

One persistent builder owns every compiler build and test:
`.claude/worktrees/builder`. Implementation workers are source-only. They never
create per-worktree `target/` caches.

- Claim and refresh the builder with `scripts/agent/builder-sync.sh`.
- Set `JET_NIX_TMP_CLEANED=1` after the first cleanup in a session.
- Wrap long commands with hard `timeout` limits and stream their output.
- After each integration, run the smallest rejecting proof first.
- Batch related green targets only after the first proof passes.
- Never rerun a green target for reassurance.
- Never run a project-wide suite before epoch closeout.
- Remove test-created scratch such as `.jet/perf/` before releasing the builder.
- Never use `/tmp` for cargo targets or large logs.

Disposable implementation worktrees contain source only. Remove them and their
temporary branches after integration or explicit rejection. Reuse the fixed
builder cache, not disposable worker caches.

## Concurrency

Concurrency exists only under a worker grant. Standard cap: about five bounded
source-only lanes.

- Parallelize only disjoint writable paths. One lane owns any shared compiler
  seam at a time.
- Never dispatch two broad cards that both need parser, sema, TIR, codegen, or
  the same tests.
- A lane remains occupied until its result is integrated, rejected, or reduced
  to a precise blocker.
- Integrate one clean branch at a time. Do not queue completed branches.
- Never `git add -A`; never touch another lane's paths.
- Cancel a worker that exceeds its brief or wall-clock limit. Salvage only a
  coherent owned diff.
- After one failed correction, shrink the next brief to the exact error and
  path. After two failures, diagnose inline before spending another worker.

## Milestone proof and review

Workers do not run compiler or test commands. The orchestrator's persistent
builder supplies the sole behavioral evidence.

For each integrated patch, run the smallest changed-contract proof that can
reject it. A green result may close the card when all other criteria have
concrete integrated evidence. Do not wait for milestone review.

At milestone end, run exactly one composed targeted sweep and one fresh-context
review of the integrated milestone diff. The sweep covers every applicable I9
tier. Each finding reopens its owning card and affected criteria. Apply one
narrow correction, review its delta, and rerun only affected targets.

Do not restart a green command to be safe. Known blockers stay visible; never
bless around them.

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
