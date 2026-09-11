---
name: orchestration
description: >-
  Dispatch and close bounded Jet work through OMP, Tower, disjoint ownership,
  focused proof, and milestone closeout. Use for worker waves, sweeps, and
  multi-card delivery.
---

# Orchestration

## Contract

- **Requested outcome:** integrated, proven, and accurately closed work for the selected Tower card or bounded sweep.
- **Supplied inputs:** the current card, owner contract in `AGENTS.md`, relevant authority, ownership state, and the exact observable criteria.
- **Allowed child result:** a bounded worker patch, source evidence, blocker, or receipt. A child does not open a new agenda or write Tower.
- **Completion owner:** the orchestrator owns integration, evidence, Tower state, and closeout.
- **Return point:** every worker result returns to the orchestrator before proof, closure, or refill.
- **Stopping condition:** stop only when the requested scope is integrated, its criteria have focused evidence, Tower shows `done` where closure is requested, and no owned work remains in flight.

`AGENTS.md` is the policy. This skill adds dispatch, integration, proof cadence, recovery, and closure mechanics. Read the owner guidance migration source before dispatch while it remains on disk; never edit it.

## Results, not activity

`closed` means a fresh Tower query shows `done`. Code-complete, compiled, merged, committed, green, proof-running, or “landed” are not closure. The board is the work ledger. Use live messages for blockers, owner gates, regressions, and resource faults; report closure IDs only after the query confirms them.

## Roles

- **Orchestrator:** plans the slice, writes the brief, dispatches, integrates, records criteria evidence, closes cards, and owns milestone proof and review. It does not implement card work. Resolving merge or integration fallout it created is allowed.
- **Worker:** implements one bounded source slice and returns a patch or commit, source evidence, blockers, and the required receipt. It does not write Tower, close cards, claim unrun proof, or spawn workers.
- **Review worker:** reviews only the bounded artifact named in its brief and returns findings. It does not repair the artifact.

## Never stop while work is in flight

When a worker reports, harvest it, inspect the owned diff, integrate it, run the exact proof, record evidence, close the card, and query `done` before claiming or briefing more work. Do not end a turn with a status report while a worker, proof, repair, or closure is ready. Pause only the slice blocked by an owner choice or external dependency; continue independent slices.

## Worker briefs

Give one bounded mechanism or one small criterion set per brief. Include:

1. the in-repository worktree and branch, plus the last integrated commit;
2. the exact actual and expected behavior;
3. every writable path and explicit non-goals;
4. applicable invariants and the full greenfield cutover;
5. the criterion proof command and expected observable result;
6. no Tower writes, no nested workers, and the required `ponytail`, `caveman`, and `simple` usage;
7. the compact return shape: changed paths, evidence, blockers, and receipt.

Code workers run only `scripts/agent/lane-check.sh` for the code patch. The report starts with the command's exact final `CHECK OK` line. A prose or static-data-only patch starts with `DOCS ONLY`. A worker writing a `.jet` file also runs `./target/debug/jet check <file>` and `jet fmt --check <file>` for that file. Workers do not run broad tests, release builds, generators, blessing, full formatting, or milestone proof. Type-checking is not runtime, tier, golden, snapshot, or generated-artifact proof; name the command that would prove those criteria without claiming it ran.

Reject a code receipt without `CHECK OK` and return the same card to the worker with the exact error. Do not repair worker implementation errors in the orchestrator.

## Dispatch and routing

Use OMP `task` first and `hub` for steering, harvest, cancellation, and liveness. Submit one `tasks[]` batch only for genuinely independent slices with disjoint writable paths and one named close owner. Choose the most specific available agent. OMP role aliases in `AGENTS.md` choose model and reasoning; prompts do not override them.

Direct Codex or rescue CLI is a fallback only after OMP cannot run the required role. Record the exact harness failure in `JET_OMP_FALLBACK_REASON` before launching a fallback. Do not make raw process launch, `run.sh`, or `lane-dispatch.mjs launch` the first path. A fallback still uses the assigned in-repository worktree, explicit timeout, complete brief, no Tower writes, and no sibling worktree access.

Use 300 seconds for mechanical fixture work, 720 seconds for normal work, and 1,200 seconds only for one narrow semantic root cause. At the limit, cancel the lane, salvage only a coherent owned patch, and rebrief a smaller slice. Use a per-item loop for a 20-minute worker slice when the task has several independent items; the continuation reads the worker's status table instead of re-deriving the work.

## Ballot review passes

A new, draft, or updated full ballot gets a fresh true-beginner pass from an OMP agent that did not join an earlier pass. Invoke `/rli5`, provide the complete ballot, and require explain, predict, modify, derive, and a friction table. Record this exact prefix in `reviewPasses.beginner`:

```text
Fresh agent: agent-id. Skill: rli5. The beginner pass tested the complete ballot.
```

The same ballot gets an adversarial review from a separate fresh agent who did not author the ballot or perform its beginner pass. The model family may match the author's (owner direction, 2026-09-05). Record the model provenance and actual reviewer ID in `reviewPasses.adversarial`:

```text
Author model family: family-a. Adversarial model family: family-a. Fresh agent: reviewer-id. The adversarial review attacked the recommendation.
```

Follow the owner's requested reviewer; otherwise use the full-review role from `AGENTS.md`. Do not require an external provider merely to change model families. Revise the ballot from material findings. A ratified decision is immutable history.

## Burndown loop

Repeat this order:

1. Query Tower. If an actively claimed card has all criteria met and no open gate, close it and confirm `done` before any new claim or brief.
2. Brief and dispatch one bounded slice through OMP.
3. Harvest the worker. Reject missing `CHECK OK`; rebrief the same card.
4. Inspect and integrate the valid patch into the intended target.
5. Run the one exact rejecting proof named by the card criterion.
6. Record command output as criteria evidence. Close immediately when criteria are met, then query `done`.
7. Only after `done`, refill genuine disjoint capacity.

Use `.claude/bdlog/proofmap.json` and `node .claude/bdlog/prove.mjs <cardId>…` when the card uses that evidence recorder. `--dry` writes no evidence. No command means no evidence row.

## Concurrency and cutovers

Start with one delivery stream. Add streams only when paths, tests, integration target, machine capacity, and the close owner are clean. Reduce concurrency around dirty ownership, shared seams, build contention, memory pressure, or high reintegration cost. Main-tree integration is sequential even when research or isolated implementation runs in parallel.

At most one cutover of a shared compiler crate is in flight. This includes `jet-comptime`, `jet-driver`, `jet-store`, `Source/CmdCompile.rs`, `crates/jet-jit/src/jit/*`, and `Codegen/Context.rs`. A shared-crate worker edits whole functions, rereads before each edit, runs its crate lane-check before moving to another file, and never yields with the crate red. The brief names the whole consumer set: callers, tests, tools, documents, environment variables, and CLI spellings.

If the workspace is already red, broadcast the known cause and owner once. Broadcast once more when the lane is green. Do not make every worker re-report the same known failure. A fixture-only drift gets a separate mechanical lane; do not mix it into a feature fix. Examples include retired syntax, reserved keywords, moved CLI verbs, missing authority grants, and retired environment variables.

Before a compiler-defect claim, rebuild `target/debug/jet` and use a fresh scratch root. A stale compiler or hostile fixture can create a false diagnostic. In a backgrounded shell, use a literal absolute redirect path; do not rely on `$TMPDIR` expanding inside a detached subshell.

## Proof cadence

Run one focused rejecting proof immediately after each integration. Use the command named by the criterion, not a nearby suite for reassurance. Close the card as soon as its integrated criteria are proven; an unrelated red test does not hold it open unless it contradicts the current criterion or violates I1 or I2 on that path.

After every linked milestone card is `done`, freeze the source in a commit and open `scripts/agent/closeout-gate.mjs open MILESTONE --by AGENT`. Only that commit-bound token authorizes one composed targeted sweep and one fresh-context integrated-diff review. Broad proof, an unfiltered conformance census, `proof-parallel.sh`, and `verify-full.sh` do not substitute for card proof. A finding reopens only its owning card and affected criteria; fix, integrate, review the delta, re-prove, close, freeze, and tokenize again.

## Machine and session safety

Use the one shared bounded `target/`. Keep `TMPDIR` on disk at `~/.cache/jet-test-scratch`, logs and briefs at `~/.cache/jet-luna`, and `CARGO_INCREMENTAL=0`. `/tmp` is RAM-backed. Monitor RAM, swap, disk, target size, and process liveness. `scripts/agent/disk-report.sh` reports reclaimable footprint. `proof-parallel.sh` enforces `JET_TARGET_CAP_GB` (120 GiB by default) after the closeout token.

A log is not a process. Use pid-aware status and `hub jobs` or `hub wait`; a missing completion marker alone is not failure. Rebrief a timed-out slice smaller. Use `scripts/agent/lane-guardian.sh` for a long wave when the owner requests it; it snapshots the working tree and sheds the newest lane under memory pressure.

## Board hygiene

Probe the named symptom once before minting. Retarget a card only when the same persistent problem changed cause. Group defects by one worker-sized root mechanism. Mint one deduplicated card for unrelated uncovered work, and continue the current card unless the new defect blocks its proof or violates I1 or I2 on the exact path. Home every card to an epoch, sidequest, or frozen state. Do not delete legitimate work to improve a count.

Use the non-serve Tower CLI against the main board. Only the owner starts `tower serve`. Never hand-edit `.tower` data. Keep implementation status, blockers, criteria evidence, and handoffs in Tower rather than in a competing task ledger.

## Recovery

Before new dispatch after a crash or tangle, account for Tower cards, worktrees, branches, uncommitted paths, and pending proofs. Checkpoint owned paths on a recovery branch using explicit paths. Salvage a dead worker's coherent diff or commit, record its last integrated state, and rebrief only the missing slice. Do not broad-restore, discard, or overwrite another task's work. Remove finished in-repository worktrees and temporary branches after integration.

## Completion

The orchestration run is complete only when every requested path is integrated, every applicable criterion has the named evidence, Tower reflects the real state, no known blocker contradicts closure, and no worker, proof, worktree, or handoff remains unaccounted for. Return changed paths, integrated state, evidence commands and results, blockers, owner gates, and newly closed card IDs. Never return a prediction as a result.
