# Orchestration

Law for any agent that dispatches other agents (burndowns, sweeps, multi-card waves).
`AGENTS.md` still governs everything else; this file adds only what it does not cover.

## Results, not activity

`closed` means a fresh Tower query shows the card in `done`. Nothing else counts: not
code-complete, not green, not merged, not "landed", not proof-running. Never predict a
closure as a result. The board is the ledger — do not repeat closure lists back to the
owner in chat; chat carries blockers, owner-gated decisions, regressions, and resource
problems only.

## Roles

- **Orchestrator:** plans, writes briefs, dispatches, isolates, integrates, records criteria
  evidence, closes cards, runs milestone proof and review, and writes the board. Never
  implements card work — not a one-liner, not to save time, not when a worker launch flakes.
  Resolving your own merge or integration fallout is allowed; that is not card
  implementation.
- **Workers:** implement one bounded source slice and return commit, source evidence, and
  blockers. They do not write the board or claim an unrun proof passed.
- **Model routing:** use GPT-5.6 Luna with medium reasoning for mechanical changes and high
  for normal semantic fixes. Use max only for one narrow root-cause problem after a concrete
  proof failed under high reasoning. Never launch broad max-reasoning card waves.

## Never stop while work is in flight

An orchestrator's turn ends when it sends a message, and nothing resumes it until a worker
notification or the owner types. Block *inside* the turn on worker logs and keep merging,
proving, and closing. Reporting status and stopping is the most expensive failure mode
there is.

## Worker briefs

One bounded slice per brief. A slice owns one mechanism, one concrete failed proof or small
criterion set, and exact writable paths. Do not ask a worker to audit or complete a large
card from scratch when the builder already exposed a specific failure.

Every brief includes: worktree and branch; last integrated commit; exact actual and expected
behavior; writable paths; applicable invariants; greenfield deletion requirements; no board
writes; no nested workers; ponytail + caveman + simple; and a compact return shape.

Workers **type-check their own patch** and nothing else. The one command is
`scripts/agent/lane-check.sh` (whole workspace, all targets, warm shared target dir;
concurrent callers queue on cargo's own build lock). A worker's report carries that
command's last line verbatim. A worker writing a `.jet` file also runs
`./target/debug/jet check <file>` and `jet fmt --check <file>` on the file it wrote.

Nothing else: no `cargo test`, no release build, no formatter over the tree, no generator,
no bless or update-expect, no git or Tower write. Those stay with the orchestrator.

This replaced a strict source-only rule that was costing more than it saved. In one
session, source-only workers produced nine separate integration breaks — an unclosed
`impl`, stale self-qualified paths after a module split, `super::` paths one level wrong,
wrong visibility on split items, a borrow-of-moved-value, a renamed enum variant, a
mismatched delimiter — and two `.jet` examples written in syntax that does not exist,
which broke the golden corpus for every other lane. Every one of those is what
`cargo check` or `jet check` prints in seconds. The orchestrator spent hours as a serial
repair queue instead of closing cards.

Workers still never label a runtime, tier, golden, snapshot, or generated-artifact
criterion met: type-checking is not testing. They name the command that would prove it,
runnable as written.

## Dispatching codex workers

Launch from the assigned worktree with an explicit effort override and wall-clock limit:

```sh
timeout 720 codex exec -p luna -c model_reasoning_effort=\"high\" \
  --skip-git-repo-check - < /tmp/luna/<brief>.md
```

Use 300 seconds and medium reasoning for mechanical fixture changes. Use 1,200 seconds and
max reasoning only for a narrow semantic root cause after high reasoning failed on a concrete
repro. At the limit, cancel, salvage only a coherent owned commit or diff, and rebrief a
smaller slice.

The `luna` profile pins the model, no approvals, and full access. Full access is required:
codex `workspace-write` protects worktree `.git` pointers and blocks the Nix daemon socket.
Every brief therefore restricts the worker to its assigned worktree and forbids the main
checkout, sibling worktrees, and `plugins/tower`.

Implementation workers type-check but never test. The persistent
`.claude/worktrees/builder` is the sole test lane. The orchestrator integrates each result
on its `CHECK OK` receipt, advances the builder, and runs the smallest proof that can
reject the patch.

**Prove in parallel, not end to end.** `scripts/agent/proof-parallel.sh [-j N] SUITE…
[--crate NAME]…` builds every test binary once, then runs the named suites concurrently and
prints one PASS/FAIL line each with a log path. A serial pass over fifteen suites left a
16-core machine idle waiting on cargo's build lock; this holds that lock once.

**Record evidence from the command, not from memory.** `.claude/bdlog/proofmap.json` maps
`cardId → criterion → { cmd, note }`, and `node .claude/bdlog/prove.mjs <cardId>…` runs each
command, marks the criterion met **only on exit 0**, and stores the command plus its real
`test result:` lines as the evidence string. `--dry` runs without writing. This makes the
lying-ledger failure mode structurally impossible: no command, no evidence row.

## Milestone stream

Finish one milestone before opening another. Keep about five source-only lanes only when
their writable paths are disjoint. Shared parser, sema, TIR, codegen, Prelude, or test seams
get one implementation lane.

1. Select one milestone and order its unblocked cards.
2. Dispatch bounded slices with exact paths and evidence shape. Do not dispatch mega-card
   prompts or broad fresh reviews during implementation.
3. Inspect each return and integrate or reject it before refilling that lane. Never build an
   integration backlog.
4. Advance the persistent builder once and run the smallest changed-contract proof first.
   Source inspection never substitutes for runtime, I9 tier, snapshot, golden, or generated
   artifact proof.
5. If proof fails, send the exact command and decisive failure to one correction worker.
   Do not resend the whole card or ask it to rediscover context.
6. Record concrete integrated evidence and close the card as soon as every criterion is met
   and no blocker contradicts it.
7. After every milestone card is done, run one composed targeted sweep and one fresh-context
   review of the integrated milestone diff.
8. Reopen findings on their owning cards. Apply narrow corrections, review only the delta,
   and rerun only affected proof targets.

Disposable implementation worktrees never build and never retain `target/`. Remove each
worktree and temporary branch after integration or explicit rejection. The fixed builder
cache is the only persistent cargo cache.

## Board hygiene

The count must be **honest** and it must **fall**. Honest beats flattering in both
directions: a stated remaining count that turns out to be 56 is the same failure as one
that turns out to be 10. Never quote a number you have not measured, and never let a
quoted number grow.

Before minting anything:

1. **Probe first.** Re-run the failure. A cause that is already fixed gets the card
   retargeted or closed, never duplicated.
2. **Retarget, don't close-and-remint.** When a test stays red for a new reason, update
   that card's title, body, and log. One card per persistent problem, retargeted as causes
   peel off. Close-and-remint churns the count while hiding that nothing was fixed.
3. **Group by work slice, not by symptom.** Defects one worker fixes in one pass are ONE
   card with a discrete exit criterion per defect. Splitting them costs a dispatch, a
   build, and a merge each. Log absorbed content on the surviving card so nothing is
   hidden, then delete the folded cards.
4. **Mint only genuinely separate, uncovered work.** Then say so plainly.

Front-load discovery instead of streaming it: one full-corpus census (every test binary,
every tier) converts unknown into known once, so scope is measured rather than discovered
card by card. Quote scope numbers only after a census, and say which cards are counted.

Every card is homed (epoch, sidequest, or frozen). Deleting legitimate work to improve a
metric requires owner approval; folding duplicates into one work-slice card does not.

## Owner gates

New syntax, a new stdlib external dependency, an invariant carve-out, an epoch or scope
move, and any other owner-only call become a Tower ballot or a proposal — never a guess and
never an action taken because the owner asked *about* it. Pause only the gated slice.
Ballot quality bar: cross-language prior art naming the most-lauded shapes in the domains
that use the feature most, a worked Jet example per option at a realistic call site, no
visually-ambiguous syntax, beginner magic plus explicit expert control, and a synthesis
option that beats the parents as the recommendation. Only the owner ratifies; a
`status: ratified` decision with an outcome is his answer and is never reopened without him.

## Recovery

Before dispatching new work after a crash or tangle, account for Tower cards, worktrees,
branches, stashes, uncommitted files, and pending proofs. Checkpoint everything to a
recovery branch first, committing owned paths explicitly. A dead worker's uncommitted work
is salvage: commit it to its branch and hand it to a continuation worker rather than
discarding it.
