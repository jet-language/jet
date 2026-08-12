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
- **Workers:** implement assigned cards and return concrete evidence for every robust
  observable criterion, plus blockers. They do not write criteria evidence or close cards.
- **Model routing:** default worker is GPT-5.6 Luna at `model_reasoning_effort=max`.
  Escalate a single task to Sol (`high`) only after Luna demonstrably failed it. Never
  launch Sol workers in a group; its burn rate is multiples of Luna's.

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

## Milestone stream

1. Select one milestone and its unblocked cards. Dispatch workers in parallel only when
   their paths and tests are disjoint.
2. Give each worker the card's full criteria, exact writable paths, applicable invariants,
   and the required evidence shape. Workers implement and return evidence; they do not
   write the board.
3. Inspect each return. Integrate a ready patch promptly. After integration, record
   concrete evidence for every robust observable criterion and close the card when the
   criteria are met and no known blocker contradicts the evidence.
4. Keep a card open when evidence is missing or a known blocker contradicts it. Route the
   fix to the owning worker and integrate the resulting patch.
5. Do not hold a card for a per-card reviewer, duplicate proof, or repeated fresh-context
   audit. Continue through the milestone as ready patches arrive.
6. At milestone end, run one composed targeted test sweep over the milestone's gates and
   one fresh-context review of the integrated milestone diff. Include every applicable I9
   execution tier.
7. Every finding reopens the owning card and affected criteria. Apply and integrate the
   fix, review the delta, verify the affected criteria, and close the card again. Close
   the milestone only after all findings are resolved and no known blocker remains.

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
