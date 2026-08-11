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

- **Orchestrator:** plans, writes briefs, dispatches, isolates, runs every proof, reviews,
  merges, writes the board. Never implements card work — not a one-liner, not to save time,
  not when a worker launch flakes. Resolving your own merge/integration fallout is allowed;
  that is not card implementation.
- **Workers:** implement, and `--meet` criteria with evidence. `--verify` and `--phase done`
  belong to the orchestrator or an independent reviewer; Tower enforces verifier ≠ builder.
  Audit `verifiedBy` on any worker-touched card — a worker that self-verified is an
  integrity violation; reverse it.
- **Model routing:** default worker is GPT-5.6 Luna at `model_reasoning_effort=max`.
  Escalate a single task to Sol (`high`) only after Luna demonstrably failed it. Never
  batch-launch Sol; its burn rate is multiples of Luna's.

## Never stop while work is in flight

An orchestrator's turn ends when it sends a message, and nothing resumes it until a worker
notification or the owner types. Block *inside* the turn on worker logs and keep merging,
proving, and closing. Reporting status and stopping is the most expensive failure mode
there is.

## Worker briefs

One card per brief, and in it: the worktree path and branch; "read the card JSON in full
plus every cited decision verbatim"; meet EVERY criterion; the invariants that apply by
number; greenfield migration deletes the replaced form; unratified user-facing spelling →
STOP that slice and return `ballot-needed` with the exact question; the exact writable
paths; the hard prohibitions (`plugins/tower`, `jet-adjacent`, `AGENTS.md`, other cards'
in-flight files); **no board writes, no cargo/jet, no git**; skills ponytail + caveman +
simple; the return shape (files+lines, per-criterion evidence, proof commands, blockers).
Codex workers cannot run cargo/jet (the sandbox blocks the Nix daemon) and cannot write the
board — the orchestrator owns every proof and every board write.

## Batch rhythm

1. Dispatch many workers in parallel, each in its own git worktree on its own branch.
2. `cargo check --workspace --tests` before anything enters the merge queue, and again
   after the batch merges. Integration fallout — visibility, unused imports, signature
   drift, duplicate table rows — is constant and cheap to catch here, ruinous to catch in
   a test run.
3. Merge the ready queue sequentially, resolving conflicts as you go.
4. ONE combined `cargo test -p jet --test <targets…>` over the union of the batch's gates.
   Every test command is `timeout`-wrapped. No test binary may exceed the 15-minute suite
   budget; the harness guard in `tests/common` aborts one that does, and a binary that
   trips it is a defect to split or speed, never a limit to raise. Filter within the big
   suites (`--test cli`, `--test golden`, `--test corelib`) while they remain over budget.
   Confirm `pgrep -fc "rustc|cargo"` is 0 first — a second cargo blocks on the build lock.
5. Batch-close on green. Per-card proof still gates each close.
6. A regression keeps its own card open and gets a fixer. Never mint a new card to fix an
   existing one; never revert-and-defer to keep master clean.

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
3. **Group by work slice, not by symptom.** Defects one worker fixes in one pass with one
   proof run are ONE card with a discrete exit criterion per defect. Splitting them costs a
   dispatch, a build, a review, and a merge each. Log absorbed content on the surviving
   card so nothing is hidden, then delete the folded cards.
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
