---
name: tower-burndown
description: Orchestrate a lean, quality-gated burndown of Tower cards, prioritizing the lowest-hanging non-frozen work while preserving dependencies, ownership, progress, and resumability. Use when asked to burn down, sweep, clear, reorder, thin, or parallelize a Tower backlog; finish easy cards first; delegate Tower work to model-scoped subagents; or continue a multi-card Tower campaign without stubs, facades, false closes, runaway tests, nested agents, or orphaned worktrees.
---

# Tower Burndown

Act only as orchestrator. Inventory, prioritize, delegate, review, integrate, verify, and update Tower. Do not implement card code yourself.

## Load sibling Tower protocols

Read `../tower/SKILL.md` completely before acting. Its live-data, scope, lane, CLI, criteria, owner-gate, and done rules are mandatory.

Read `../tower-ballot/SKILL.md` before raising or reviewing an owner decision. Read `../tower-setup/SKILL.md` only when Tower is missing or misconfigured. Also read the repository `AGENTS.md`, its required files, and any user-named orchestration prompt.

Inspect live Tower state, Git state, claims, worktrees, and running agents. Treat card state, decisions, criteria, logs, and current source as truth. Old plans and completion claims are evidence to verify, not facts to trust. Use `tower brief` for each selected card.

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

Name worktrees/branches by card. Merge every successful worktree, then immediately remove the worktree and delete its branch. Before ending, prove no skill-created worktree or branch remains. Never discard another agent's dirty work.

## Scope models

Use only available 5.6 model tiers and state the tier in every brief:

- **Luna:** read-only inventory, grep, stale-state reconciliation, simple docs/mechanical changes with obvious proof.
- **Terra:** default implementation and review tier; ordinary Rust, tests, tooling, focused refactors.
- **Sol:** architecture, security, type-system/sema, subtle correctness or false-green review, complex planning, cross-layer changes.

Do not spend Sol on routine work. Escalate Terra work to Sol review when scope expands across layers, security boundaries, semantic phases, or complex test oracles.

## Enforce one agent layer

The orchestrator may spawn subagents. Subagents must never spawn subagents. State `no subagents` in every brief.

Use at most the harness concurrency ceiling. Prefer multiple disjoint builders when the user prioritizes throughput. Reserve or rotate one slot for review as builders finish. If model capacity fails, preserve the claim/worktree and retry with another correctly scoped agent; do not duplicate ownership.

## Prepare each card

Before implementation:

1. Check owner gates, blockers, decisions, claims, and current source truth.
2. Add measurable criteria if absent. Criteria must test behavior, not file existence or assertions.
3. Claim the card with the builder identity.
4. Create a clean checkpoint or isolated worktree.
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

Never trust a builder's `done`. Assign an independent reviewer whose identity differs from the builder. The reviewer must inspect the diff and evidence, target false-green paths, and return exact findings. Send findings back to the same builder; re-review only changed findings plus regression risk. Merge only after no material findings remain.

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
- independent criteria verified;
- merge commit and final lane.

Use fresh revisions or optimistic concurrency where supported. Never hand-edit Tower data. If an agent fails, preserve its worktree, inspect its diff as untrusted inherited work, log what exists/what remains, then hand the same card to one replacement agent.

Do not mark owner-gated criteria met. Merge safe local containment if useful, then leave the card building with an explicit owner handoff.

## Lean operation

- Read each large source once; pass compressed packets to builders/reviewers.
- Scout several adjacent cards in one read-only tranche.
- Reuse completed agents for follow-up tasks when the harness limits threads.
- Print only concise state deltas, not whole cards or logs.
- Use one reviewer pass per card unless findings require a targeted re-review.
- Prefer evidence references, commit IDs, counts, and commands over narrative.
- Keep the next safe lane prepared while active builders run.

## Completion

A card is complete only when implementation is merged, focused proof passes, all builder criteria are met, a different agent verifies them, Tower is updated, and its worktree/branch is deleted. A campaign is complete only when the requested scope is empty except frozen/owner-blocked cards, the orchestrator's major-push closeout suite has passed, and no claims, dirty worktrees, or temporary artifacts are orphaned.

End with a terse report: cards closed, cards advanced, owner gates, focused/broader test results, current active claims, and worktree cleanup proof.
