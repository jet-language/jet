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

Implementation workers type-check but never test. The orchestrator integrates each
result on its `CHECK OK` receipt and runs the smallest proof that can reject the patch, in
the main checkout, against the one warm target dir.

**One build cache, bounded.** A second build tree in a worktree is not free: one grew to
517G (438G of stale `deps` generations, 40G incremental, 39G scratch) and the machine
ended a session in an OOM kill. There is now one target dir. `scripts/agent/disk-report.sh`
prints the footprint and the exact command to give each piece back;
`proof-parallel.sh` refuses to run when `target/` is over `JET_TARGET_CAP_GB` (120 by
default).

**Nothing heavy in /tmp.** `/tmp` is RAM-backed tmpfs on this machine, so test scratch
there is RAM. Every agent script exports `TMPDIR=~/.cache/jet-test-scratch` before
`jet-env` so the nix shell inherits it, and sets `CARGO_INCREMENTAL=0`. Briefs and lane
logs live in `~/.cache/jet-luna`, not `/tmp/luna`.

**Prove in parallel, not end to end.** `scripts/agent/proof-parallel.sh [-j N] SUITE…
[--crate NAME]…` builds every test binary once, then runs the named suites concurrently and
prints one PASS/FAIL line each with a log path. A serial pass over fifteen suites left a
16-core machine idle waiting on cargo's build lock; this holds that lock once.

**Record evidence from the command, not from memory.** `.claude/bdlog/proofmap.json` maps
`cardId → criterion → { cmd, note }`, and `node .claude/bdlog/prove.mjs <cardId>…` runs each
command, marks the criterion met **only on exit 0**, and stores the command plus its real
`test result:` lines as the evidence string. `--dry` runs without writing. This makes the
lying-ledger failure mode structurally impossible: no command, no evidence row.

## The burndown loop

Use `scripts/agent/lane-dispatch.mjs`. It encodes this loop, and the loop is the
difference between a few cards a day and forty in a session.

```sh
node scripts/agent/lane-dispatch.mjs status          # running, yielded, died, headroom
node scripts/agent/lane-dispatch.mjs brief --auto 12 # briefs straight from Tower
node scripts/agent/lane-dispatch.mjs launch c1234 …  # detached, staggered, capped
node scripts/agent/lane-dispatch.mjs harvest         # each unread lane's final message
```

Poll every five minutes: harvest what finished, close what is done, refill the
freed lanes, and repair anything that broke. Never let the loop idle.

### The five rules, and what each one cost before it existed

1. **Parallelism is the whole game.** Run 25-30 lanes, not five. Cards are mostly
   independent, and every lane shares one working tree, so there is no merge
   afterwards. Serial dispatch was the single largest source of elapsed time.
2. **Launch detached.** `(sh run.sh x &)` inside a tool call loses most of the
   batch when the parent shell exits. Measured: 9 of 27 lanes survived and the
   other 18 were silently absent for twenty minutes. Use `setsid nohup … </dev/null`.
3. **Never hand-write a brief.** Generate it from the card, which already holds
   the title, body, plan and exit criteria. Hand-write only for a defect that has
   no card yet.
4. **Workers type-check; they do not test.** `scripts/agent/lane-check.sh` and
   nothing heavier. Before this rule the orchestrator became a serial repair
   queue — nine build breaks in one session from source-only patches.
5. **Close on implementation; batch the proof.** A card closes when its criteria
   have concrete implementation evidence and the patch is integrated. Deferred
   test runs, found defects and owner gates all go to one sweep ledger.

### Liveness is a process question

A lane that hit its timeout and a lane still thinking both leave a log with no
completion marker, so reading logs alone reports ghosts. `run.sh` writes a
pidfile and clears it on exit; `status` reads that. Measured once: 27 logs looked
busy while 10 processes existed, and two thirds of the capacity sat idle behind
the misreading. A lane with no report is not a failure — re-brief it smaller.

### Things that silently waste a wave

- **Stale blockers.** Check `blockedBy` against real phase before believing a
  card is gated. On one board 21 of 31 "blocked" cards had blockers already closed.
- **Build breaks are the orchestrator's.** Lanes share one tree, so one break
  blocks every lane's self-check. Fix it immediately; do not wait for the lane
  that caused it, and do not ask a worker to fix another worker's file.
- **Large cards time out.** A card with eight or more criteria gets a
  slice-scoped brief: make ONE criterion genuinely true end to end and leave the
  rest untouched. `brief` adds that instruction automatically.

### Verification cadence

Targeted suites run once at a milestone boundary. The full suite runs once at
epoch end. Everything deferred collects in one ledger — see
`.claude/bdlog/EPOCH3-SWEEP.md` for the shape: a table of deferred proof, a table
of defects found with their evidence, and a table of open owner gates. After the
cards are closed, that ledger is worked in a single comprehensive sweep.

### Session safety

`scripts/agent/lane-guardian.sh` runs for the duration of a wave: a working-tree
snapshot every three minutes into `~/.cache/jet-luna/snapshots`, and an
available-memory floor that sheds the newest worker first. Many lanes share one
tree, so the risk is two lanes writing one file; the snapshot interval bounds
what that can cost. `/tmp` is RAM-backed here — a session has already died of
OOM with scratch in tmpfs, so every script exports `TMPDIR` to disk before
`jet-env` and caps `CARGO_BUILD_JOBS`.

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
