# Owner Guidance Evidence Archive

**Status:** Immutable evidence snapshot from 2026-08-31. Not an active policy source. Agents read `docs/agents/owner-guidance.md` before work and consult this file only to audit provenance or recover detail. Owner changes current guidance through Tower's **Guidance** tab.

This snapshot preserves full corpus method, historical conflict notes, evidence index, skill inventory, and pre-compression wording. Current authority always wins.

---

# Owner Guidance

**Status:** Owner-maintained operational guidance. Agents must read this file before they use a skill or dispatch another agent. Agents may read this file, but must not edit it. The owner edits it through Tower's **Guidance** tab.

This file consolidates the owner's operational feedback for orchestrators and workers. It is not a second specification, planner, phase model, or board. Resolve conflicts in this order: the owner's current explicit instruction, ratified Tower decisions and acceptance terms, the relevant domain specification, this report for agent conduct, `AGENTS.md` and its nested rules, and then task-specific skills. `docs/agents/orchestration.md` remains the detailed law for dispatch and closeout mechanics. `docs/agents/agent-memory.md` remains the owner-auditable store for project state and technical traps.

`[Owner]` marks a direct owner rule or preference. `[Synthesis]` marks an operational conclusion drawn from repeated feedback and checked against the current repository authority. A dated transcript is evidence of what the owner said at that time. It is not automatically a current ruling when a later instruction or repository authority supersedes it.

## Non-negotiable rules

1. **Read, then act.** Read this file before any skill or dispatch. Agents must not edit this file. Use the current authority chain instead of copying a stale transcript instruction.
2. **Measure results, not activity.** A card is closed only when a fresh Tower query shows `done`, with robust criteria evidence, an integrated patch, and no contradictory blocker. Agent launches, code-complete claims, merges, green checks, and proof in progress are not closure.
3. **Do not stop with work in flight.** Continue the orchestration loop while work is running. Harvest results, integrate, prove, close, repair, and refill. Pause only the slice genuinely gated on the owner or an external dependency.
4. **Owner decisions are explicit.** Do not act because the owner asked about an option. Raise a ballot or proposal for new syntax, external dependencies, invariant exceptions, scope or epoch moves, and other owner-only choices. Continue all independent work.
5. **Give workers complete, bounded work.** One worker owns one coherent source slice. Provide the actual failure, expected behavior, exact paths, constraints, invariants, cutover requirements, and observable exit criteria.
6. **Use adaptive concurrency.** Parallelize genuinely independent work when the speed gain exceeds reintegration cost. Keep write paths disjoint, name one close owner, protect shared seams and build resources, and use the current orchestration rule rather than a historical fixed lane count.
7. **Integrate promptly and preserve work.** Never leave useful work in an unmerged worktree or branch. Never overwrite another task's paths. Salvage coherent work after a worker dies and record a handoff.
8. **Prove the behavior that matters.** Do not call a source-only claim a runtime, tier, golden, snapshot, or generated-artifact result. Prove applicable execution tiers, failure paths, diagnostics, and examples. Silent wrong answers are P0-quality failures.
9. **Keep the board honest.** Query Tower before quoting counts. Deduplicate and retarget persistent problems. Every surviving card has one home, a real plan, and a coherent work slice. Never hand-edit `.tower` data or run a second Tower server.
10. **Protect the machine.** `/tmp` is RAM-backed. Keep scratch and logs on the configured disk paths, share the one bounded build target, monitor RAM, swap, disk, and target size, and remove stale artifacts. A guardrail must not hide a root resource defect.
11. **Improve the whole language.** Do not overfit a benchmark or one example. Prefer a systemic, durable root fix that improves other programs while preserving safety, parity, determinism, and expert control.
12. **Optimize understanding.** Reasoning comes before reading, and reading comes before writing when the trade-off is close. Prefer clear, cohesive, commonality-weighted surfaces over clever short forms. Show the code and the delta instead of describing an abstract possibility.

## Planning

| AVOID | DO |
|---|---|
| **[Owner]** Do not open the next milestone while the current milestone has unfinished cards or unreviewed integration fallout. Do not treat a large “fix everything” prompt as a plan. | **[Owner]** Work in the current board order unless the owner changes it. Finish and close the current milestone, then run its boundary sweep, before advancing. Group work only when the group has one coherent mechanism and clear dependencies. |
| **[Owner]** Do not leave cards with vague goals, weak “run a test” criteria, hidden scope, or unrecorded decisions. | **[Owner]** Make every card implementable by a competent but context-limited worker: state actual and expected behavior, exact paths, non-goals, dependencies, applicable tiers and invariants, complete cutover, and observable exit criteria. |
| **[Owner]** Do not stream discovery card by card when a census can establish scope. Do not keep status-quo cards with no actual change. | **[Synthesis]** Probe and inventory first. Deduplicate by root mechanism, retarget a card when the failure cause changes, and delete or fold cards that contain no remaining work. Record what was absorbed. |
| **[Owner]** Do not use the owner's absence as permission to invent direction. Do not stop all work because one decision is pending. | **[Owner]** Separate gated from ungated work. Ballot the choice, pause only its slice, and continue work that does not depend on it. Freeze intake when the current handoff or owner instruction says to freeze it. |

## Delegation and model choice

| AVOID | DO |
|---|---|
| **[Owner]** Do not make the orchestrator implement card work unless it is more time or token efficient, and do not ask a worker to make a broad design judgment from a problem statement alone. | **[Owner]** The orchestrator owns reasoning, planning, dispatch, integration, evidence, and board updates. A worker implements one bounded slice and returns the patch or commit, source evidence, and blockers. |
| **[Owner]** Do not spend scarce model usage on duplicate exploration, unrequested model families, or an elaborate role taxonomy. Do not assume a label such as “Luna” proves the actual model or reasoning setting. | **[Synthesis]** Use the current `docs/agents/orchestration.md` routing. Verify the selected profile instead of trusting a name. |
| **[Owner]** Do not delegate plans and ballots when the owner assigned them to the orchestrator. Do not use a model's intelligence as a substitute for a precise brief. | **[Owner]** Write plans and ballots yourself when the current task assigns that judgment to the orchestrator. Use implementation workers for implementation, and use a reviewer or planner only for a bounded role allowed by current authority. |
| **[Current authority]** Do not let workers write Tower, set a card to `done`, claim an unrun proof, run broad tests, or spawn nested workers. | **[Current authority]** Workers run the current lane-check command and return its receipt. The orchestrator runs the rejecting proof in the main integration target, records evidence, and controls closure. |

## Concurrency

| AVOID | DO |
|---|---|
| **[Owner]** Do not sit idle while independent cards are available. Do not launch a wide wave without a reintegration plan, and do not let multiple workers write the same path. | **[Owner]** Fan out only genuinely independent slices. Compare expected time saved with merge, review, build, token, and recovery cost. Give each lane disjoint writable paths and one named close owner. |
| **[Owner]** Do not treat historical caps such as five, ten, 16, 20, 25–30, or two lanes as permanent policy. Those numbers were responses to different resource and collision conditions. | **[Current authority]** Use the adaptive rule: default to one delivery stream; expand only when paths, tests, integration target, and resources are clean. Contract or reduce concurrency around shared compiler seams, shared build caches, another agent's work, or memory pressure. |
| **[Owner]** Do not confuse a live log with a live process, or lose a detached batch when the parent shell exits. | **[Current authority]** Use the current lane tooling, detached launch recipe, pid-aware status, and five-minute harvest/refill loop. A missing completion marker is not enough to declare failure; inspect process state and rebrief a timed-out slice smaller. |
| **[Owner]** Do not remerge a large set of stale worktrees at the end of a sprint. | **[Synthesis]** Integrate each coherent result promptly, resolve shared fallout immediately, and refill only after accepting or rejecting the returned work. |

## Context and prompts

| AVOID | DO |
|---|---|
| **[Owner]** Do not give a worker only “fix this issue,” a broad audit, or a stale symbol. Do not hide the expected output, known constraints, or the reason the failure matters. | **[Owner]** Give a compact brief with the concrete repro, actual and expected behavior, exact writable paths, last integrated commit, invariants, deletion and migration requirements, non-goals, and a compact return shape. Ask for one mechanism or one criterion set. |
| **[Owner]** Do not bury a simple goal in an exhaustive prompt for a capable planner. Do not omit detail where the worker must act without further judgment. | **[Synthesis]** Match prompt size to the role: short goal plus relevant constraints for a capable planner; complete explicit context for a bounded implementer. Every brief must still be self-contained enough to run without hidden chat state. |
| **[Owner]** Do not use different programs when asking the owner to compare alternatives. Do not describe a surface theoretically when code can show it. | **[Owner]** Use the same realistic call site for before/after examples. Show the exact Jet syntax, the user-visible delta, the common path, edge cases, and expert escape. Preserve exact identifiers and diagnostics. |
| **[Owner]** Do not assume a cold agent can inspect a repository, discover hidden helpers, download packages, or infer unstated input. | **[Owner]** For cold-agent tasks, provide one complete context capsule, one explicit task contract, exact observable output, and the allowed APIs. Require the requested output shape only; no hidden dependencies, network, or extra output. |

## Ownership and worktrees

| AVOID | DO |
|---|---|
| **[Owner]** Do not interfere with another agent's paths, overwrite a dirty shared tree, broad-restore files, or assume a branch is safe because it has a familiar name. | **[Current authority]** Record ownership before work. Inspect the current diff. Stage, commit, and clean only explicitly owned paths. If ownership or collision is unclear, stop the write and resolve it. |
| **[Owner]** Do not accumulate sibling clones, orphaned worktrees, stale branches, duplicate build targets, or uncommitted completed work. | **[Current authority]** Use only an in-repository worktree path when isolation is needed. Integrate into the intended branch promptly, verify there, then remove the worktree and temporary branch. A paused task keeps a named handoff branch and resume note. |
| **[Current authority]** Never use `git add -A`, broad `git commit -a`, `git restore .`, or an equivalent command. Never hand-edit `plugins/tower/.tower/`. | **[Current authority]** Use explicit paths and the non-serve Tower CLI. Keep the canonical board in the main checkout. Preserve unrelated dirty work. |
| **[Owner]** Do not create a fresh cold build tree for every lane. | **[Current authority]** Share the one bounded target as the orchestration rules require. Workers type-check; the orchestrator owns heavier proof. |

## Integration

| AVOID | DO |
|---|---|
| **[Owner]** Do not call work complete when it exists only on a worker branch, or defer every merge until the end. Do not cherry-pick a branch's unrelated WIP just because one file is useful. | **[Owner]** Integrate each coherent patch promptly. Inspect the diff, preserve only the owned change, and prove it in the intended integration target. If one worker breaks a shared seam, repair the integration fallout immediately without taking ownership of unrelated card implementation. |
| **[Owner]** Do not discard a dead worker's uncommitted work or restart from zero without checking it. | **[Current authority]** Salvage a coherent diff or commit into a named continuation branch, record its last integrated commit, and rebrief only the missing slice. |
| **[Owner]** Do not preserve old syntax, aliases, shims, or parallel mechanisms after a greenfield change. | **[Current authority]** Migrate every in-repo caller, example, test, generated artifact, schema, and document in one coherent cutover. Delete the replaced form unless an owner-ratified exception names its scope and removal condition. |
| **[Owner]** Do not hold a proven card open for a later mega-sweep. | **[Current authority]** Once robust criteria have concrete evidence, the patch is integrated, and no blocker contradicts it, record the evidence and close the card. The milestone sweep is a separate gate. |

## Verification and closure

| AVOID | DO |
|---|---|
| **[Owner]** Do not close on a worker's “met” claim, a source diff, a single output-only check, or a test that never exercised the actual failure. Do not hide a compiler or runtime defect with a panic, fallback, suppression, or test weakening. | **[Owner]** Use the smallest proof that can reject the integrated patch. Test the real boundary, failure behavior, and data correctness. For silent-data risks, use differential or cross-tier assertions and stress cases, not only expected-looking output. |
| **[Owner]** Do not label a default-tier or AOT-only result complete when another applicable tier is unproved. Do not park unfinished parity in `jit_gaps`. | **[Current authority]** Apply I9 end to end: parser, sema, TIR, AOT, default `jet run`, interpreter/deopt, and web when applicable. Apply I4 diagnostic requirements and I5 executable examples. Semantics belong in the shared Prelude; engines marshal them. |
| **[Owner]** Do not run the full suite for every card or use a long suite as evidence that a weak criterion is acceptable. | **[Owner]** Run targeted suites once at the milestone boundary and the full suite once at epoch end, then resolve the collected findings in one comprehensive sweep. Use current verification rules when a known interaction requires an earlier rejecting proof. |
| **[Current authority]** Do not require a per-card reviewer or duplicate reassurance proof before card closure. Do not ask the owner to verify machine claims. | **[Current authority]** The orchestrator records machine evidence. At milestone end, run one composed targeted sweep and one fresh-context integrated-diff review. Owner acceptance is only for look-and-feel or a real environment the harness cannot replace. |

## Tower hygiene

| AVOID | DO |
|---|---|
| **[Owner]** Do not measure progress by agent count, code volume, a branch, or a green command. Do not quote an unmeasured open-card count. | **[Current authority]** Treat fresh Tower state as the ledger. Quote only measured counts, and make the count fall through real closure rather than status manipulation. |
| **[Owner]** Do not create duplicate cards for one persistent problem, close and remint to change its cause, or keep a card whose only purpose is to preserve the status quo. | **[Current authority]** Probe first. Retarget the existing card when the failure changes, group defects by work slice, fold valid goals and criteria into the surviving card, and delete duplicates. Mint only genuinely separate uncovered work. |
| **[Owner]** Do not leave cards loose in an epoch, unplanned, or with criteria that a worker must interpret. Do not leave owner questions invisible in chat. | **[Owner]** Home every card to the correct epoch, sidequest, frozen state, or milestone. Write the plan and criteria before implementation. Raise each genuine owner choice as a visible ballot or proposal and pause only that slice. |
| **[Current authority]** Do not run `node plugins/tower/tower.mjs serve --open`, another Tower server, or a worktree copy of the board. | **[Current authority]** Only the owner starts the Tower server. Agents use the non-serve CLI against the main board, and report a stale or wrong server instead of restarting it. |
| **[Owner]** Do not let a status update replace work or hide a stalled loop. | **[Synthesis]** Keep board state current while work proceeds. Use the current phase vocabulary and fresh queries. Put closure details in Tower; use chat for blockers, owner-gated decisions, regressions, and resource problems. |

## Communication

| AVOID | DO |
|---|---|
| **[Owner]** Do not stop because the owner is away, wait for an answer that is not needed, or ask the same question repeatedly. Do not send a long status story when a measured answer is enough. | **[Owner]** Work autonomously on every unblocked slice. Ask one grouped set of questions only when needed, use the harness question tool when available, and ballot genuine decisions. A requested status contains measured state, completed work recorded on the board, blockers, and the next action. |
| **[Owner]** Do not claim a card, milestone, parity level, or release state without fresh evidence. Do not tell the owner a feature is ready when only an internal implementation exists. | **[Synthesis]** State what was actually exercised, what remains, and what the owner must visually inspect. Use “unknown” or “blocked” when that is the measured state. |
| **[Owner]** Do not expose irrelevant private chat content, reproduce hostile wording, or use AI filler. Do not make public-facing work about agents when the owner asked for a human-first product. | **[Owner]** Use plain, concise, professional language. Keep internal shorthand out of owner-facing prose. For public language, position Jet for people and broad workloads; agent usability is a consequence, not the pitch. Use requested ELI5 or caveman compression for informal updates, while formal reports remain clear and complete. |
| **[Owner]** Do not let a new report or skill become another competing source of truth. | **[Owner]** Persist decisions in the proper authority: design in specs or ballots, work state in Tower, procedures in skills, and owner behavior guidance in this file. |

## Resource use

| AVOID | DO |
|---|---|
| **[Owner]** Do not allow OOM kills, swap exhaustion, `/tmp` growth, target-directory explosions, disk exhaustion, or silent hangs. Do not accept a memory cap as the fix for a process that still has a root leak or unbounded work. | **[Owner]** Monitor RAM, swap, disk, scratch, target size, and process liveness during a wave. Diagnose the cause, keep scratch on disk, and clean stale bloat as soon as it is safe. Make long operations show active progress and use bounded commands. |
| **[Current authority]** Never put cargo targets, alternate targets, multi-GB logs, or test scratch in `/tmp`; never create a second build tree for convenience. | **[Current authority]** Use the configured disk scratch path `~/.cache/jet-test-scratch`, the shared target, and `CARGO_INCREMENTAL=0`. `scripts/agent/disk-report.sh` reports reclaimable footprint; `scripts/agent/proof-parallel.sh` enforces the `JET_TARGET_CAP_GB` limit, 120 GiB by default. |
| **[Owner]** Do not pay repeated cold-compilation costs or run serial heavy suites when the work can share a build and execute bounded suites concurrently. | **[Current authority]** Keep approved worktrees warm, use the one target, and use the current proof-parallel runner. Do not trade correctness for speed, but remove avoidable compilation, merge, and administrative delay. |
| **[Owner]** Do not start or duplicate a server that the owner is already running, and do not leave an agent-hosted server running after the owner asks to take control. | **[Current authority]** For Tower, the owner alone starts the server. For other development servers, inspect existing processes and follow the latest explicit owner instruction; never create a duplicate by assumption. |

## Scope and design authority

| AVOID | DO |
|---|---|
| **[Owner]** Do not treat an exploratory question as approval. Do not invent a frontend command, public API, syntax, or product behavior. Do not reopen a ratified decision because a transcript is older or a worker prefers another shape. | **[Owner]** Give the owner direct control over user-facing syntax, APIs, commands, and visual surfaces. Show the real same-program alternatives, explain the delta and trade-offs, and ballot every genuine owner choice. A ratified outcome remains law until the owner amends it. |
| **[Owner]** Do not optimize one benchmark, one language comparison, or one example at the expense of the rest of Jet. Do not use “easy to implement” as a product argument. | **[Owner]** Improve the general mechanism and test a matched, fair comparison across representative workloads. Preserve safety, performance, determinism, tier parity, and expert control. Judge the best final design, not implementation effort. |
| **[Owner]** Do not add a method, keyword, abstraction, or dependency for every isolated case. Do not let brevity make code harder to read or reason about. | **[Owner]** Use one coherent mechanism, a small safe beginner default, and explicit expert control. Minimize friction in proportion to commonality. Prefer code that is easy to reason about, then easy to read, then easy to write when the trade-off is close. |
| **[Owner]** Do not accept an attractive surface that is ambiguous, inconsistent, or unsupported on another tier. Do not describe a theoretical feature without showing its use at a realistic call site. | **[Current authority]** Apply beginner and expert passes. Use consistent shapes across functions, lambdas, control flow, bindings, and common library operations. Show before/after code, real error text, and the complete applicable execution path. |
| **[Owner]** Do not use a narrow style guide as the only defense against recurring defects. Do not settle for a warning that identifies a problem but leaves every future author to repeat it. | **[Owner]** Prefer light structural enforcement: compiler diagnostics, lint rules, safe fixes, formatter or language-server actions, canonical executable examples, and one source of truth. Allow explicit expert opt-out only where the current design and safety policy permit it. |

## Positive reinforcement to preserve

The owner explicitly approved these patterns when they produced observable outcomes:

- Fanout was praised when it produced tangible, recorded progress and was followed by disciplined reintegration, rather than when it merely increased agent count.
- A cadence of checking work, closing proven cards, and batching the later sweep was described as substantially better than stalled, serial work.
- A clean integration of common surfaces, with a simple default and an expert path, was repeatedly preferred over a larger but fragmented API.
- Commonality-weighted friction, real examples, and clear visual or terminal feedback were treated as quality improvements, not cosmetic extras.
- Durable fixes to the process, not another temporary workaround, were repeatedly requested after a failure was found.
