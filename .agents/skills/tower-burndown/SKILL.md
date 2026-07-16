---
name: tower-burndown
description: Orchestrate a lean, quality-gated burndown of Tower cards, prioritizing the lowest-hanging non-frozen work while preserving dependencies, ownership, progress, and resumability. Use when asked to burn down, sweep, clear, reorder, thin, or parallelize a Tower backlog; finish easy cards first; delegate Tower work to model-scoped subagents; or continue a multi-card Tower campaign without stubs, facades, false closes, runaway tests, nested agents, or orphaned worktrees.
---

# Tower Burndown

Act only as orchestrator. Inventory, prioritize, delegate, review, integrate, verify, and update Tower. Do not implement card code yourself.

## Load sibling Tower protocols

Read `../tower/SKILL.md` completely before acting. Its live-data, scope, lane, CLI, criteria, owner-gate, and done rules are mandatory.

Read `../tower-ballot/SKILL.md` before raising or reviewing an owner decision. Read `../tower-setup/SKILL.md` only when Tower is missing or misconfigured. Also read the repository `AGENTS.md` and any user-named prompt. Load domain specs only when the selected card triggers them.

Inspect live Tower state, Git state, claims, worktrees, and running agents. Treat ratified decisions and acceptance criteria as design authority; source is implementation state. Old plans and completion claims are evidence to verify, not facts to trust. Use `tower brief` for each selected card.

Use caveman-compressed status, briefs, and agent reports unless the user requests normal prose.

## Establish the queue

Account for every live non-frozen card in the user-requested scope. Default to the repository Tower skill's burndown scope; use the entire non-frozen board when the user explicitly requests it.

Rank cards by:

1. Already complete but stale state: reconcile only after proof.
2. Tiny, ungated, mechanically verifiable repairs.
3. Narrow tests, docs, tooling, and durability work.
4. Existing builds with small, concrete remaining gaps.
5. Bounded implementation with ratified behavior.
6. Broad, cross-layer, architectural, owner-gated, or externally blocked work.

Respect dependencies before ease. Preserve active ownership. Never touch frozen or owner lanes unless the owner explicitly acts. Use one atomic Tower mutation when reordering many cards; verify complete coverage, unique ranks, and zero dependency inversions. Record collision warnings.

## Build a collision graph

Before delegation, map each candidate's likely files, shared generated artifacts, Tower writes, test resources, and external services.

Parallelize only file-disjoint or safely isolated cards. Serialize cards sharing compiler phases, terminal plumbing, Canvas runtime, Jetpack output, generated snapshots, Tower config/store, or scarce browser/VM resources.

Prefer direct current-branch work only when the checkout is clean and collision-free. Use a worktree when:

- the main tree contains another agent's changes;
- concurrent cards may touch nearby files;
- a risky card needs independent review before integration.

Name and record worktrees/branches by card and owner. Integrate every successful worktree promptly, verify the integrated diff, then immediately remove the worktree and temporary branch. If work stops, commit only a coherent owned checkpoint, log exact resume state and owner, remove the worktree, and retain only the named branch. Before ending, prove no skill-created worktree remains. Never discard another agent's dirty work.

## Scope models

Use GPT-5.6 Sol by default and state its effort in every brief. Use low effort
for bounded mechanics, medium for normal implementation, high/xhigh for
architecture, compiler semantics, hard debugging, and first-pass review, and
max for the hardest cases.
Prefer changing Sol effort over changing model families. Terra performs the
mandatory second review; use it elsewhere only for a recorded task-specific
advantage. Luna requires an owner request or measured stable advantage on
high-volume fully mechanical work.

## Enforce one agent layer

The orchestrator may spawn subagents. Subagents must never spawn subagents. State `no subagents` in every brief.

Use at most the harness concurrency ceiling. One implementer owns each coherent change; parallel builders must have disjoint cards and paths. Reserve capacity for sequential reviews. If capacity fails, log a handoff and retry without duplicating ownership or leaving an untracked worktree.

## Prepare each card

Before implementation:

1. Check owner gates, blockers, decisions, claims, and current source truth.
2. Add measurable criteria if absent. Criteria must test behavior, not file existence or assertions.
3. Claim the card with the builder identity.
4. Create an isolated worktree for concurrent writes, or confirm direct-tree path ownership. Never use `git add -A`; stage only owned paths.
5. Give one self-contained brief: goal, exact paths, ratified decisions, criteria, invariants, collision limits, focused tests, and `no full suite; orchestrator owns major-push closeout`.

Use one compressed investigator for a tranche when it avoids repeated discovery. Do not spawn an agent for a single command.

## Quality gate

Reject stubs, facades, placeholders, inert wiring, fake data, tautological tests, skipped fixtures, permissive normalization, static-only UI proof, and claims based only on file presence.

Require:

- complete end-to-end behavior for the card scope;
- failing-test-first or concrete reproduction where applicable;
- focused positive and negative proof;
- no silent skip or vacuous green path;
- exact rollback/restoration after temporary mutation tests;
- docs/spec/examples/diagnostics required by repository invariants;
- clean final diff and no unrelated edits.

Never trust a builder's `done`. Assign a fresh Sol reviewer whose identity differs from the builder. It inspects the diff and evidence, targets false-green paths, and returns exact findings without implementing. Send findings to the same builder and recheck material fixes. Then assign a fresh Terra reviewer to independently repeat the gate on the resulting patch. The builder fixes its findings and Terra rechecks material fixes. Merge only after both reviews are clear.

## Test budget

Subagents run focused tests only by default: the narrowest unit, integration, snapshot, scenario, browser, or package target proving their card. Avoid repeated Nix startup and redundant reruns; group related focused checks in one shell when the repository permits it.

The orchestrator may authorize broader tests when risk demands them, including:

- shared parser/sema/codegen changes;
- security or persistence changes;
- generated artifacts or snapshot-wide effects;
- cross-card batch integration;
- repository-required closeout gates.

Only the orchestrator runs a full suite, once after a major push on its closeout
or blocking card—not once per card or subagent. CI runs it again. Distinguish
unrelated pre-existing failures by exact reproduction; never relabel a caused
failure as unrelated.

## Durable progress

Tower is the recovery log. Update it at major checkpoints:

- criteria added and card claimed;
- reproduction/root cause established;
- substantial slice completed;
- focused tests and temporary mutation evidence;
- handoff before release/failure/capacity stop;
- builder criteria met;
- Sol review cleared, then Terra criteria verification cleared;
- merge commit and final lane.

Use fresh revisions or optimistic concurrency where supported. Never hand-edit Tower data. If an agent fails, inspect its diff as untrusted inherited work, commit only a coherent owned checkpoint worth retaining, log what exists and what remains, remove its worktree, then hand the named branch to one replacement agent. Do not preserve an orphaned worktree indefinitely.

Do not mark owner-gated criteria met. Merge safe local containment if useful, then leave the card building with an explicit owner handoff.

## Lean operation

- Read each large source once; pass compressed packets to builders/reviewers.
- Scout several adjacent cards in one read-only tranche.
- Reuse completed agents for follow-up tasks when the harness limits threads.
- Print only concise state deltas, not whole cards or logs.
- Use exactly the required Sol review followed by Terra review; findings receive targeted rechecks before the next gate.
- Prefer evidence references, commit IDs, counts, and commands over narrative.
- Keep the next safe lane prepared while active builders run.

## Completion

A card is complete only when implementation is integrated, focused proof passes, all builder criteria are met, the Sol and Terra reviews clear in order, Tower is updated, and its worktree/temporary branch is deleted. A campaign is complete only when the requested scope is empty except frozen/owner-blocked cards, the orchestrator's major-push closeout suite has passed, and no claims, dirty worktrees, temporary branches, or artifacts are orphaned.

End with a terse report: cards closed, cards advanced, owner gates, focused/broader test results, current active claims, and worktree cleanup proof.
