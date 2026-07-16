---
name: codeflow
description: Compile and run dynamic, resumable, evidence-gated coding workflows with native Codex subagents. Use for complex or ambiguous implementation, refactoring, migration, debugging, repository-wide analysis, multi-part review, or any request naming Codeflow, Ultracode, dynamic workflows, agent teams, parallel agents, or fresh adversarial verification. Do not auto-trigger for a trivial isolated edit unless the user explicitly invokes it.
---

# Codeflow

Turn the current task into a purpose-built execution graph. Keep the main thread as coordinator: preserve intent, own state, integrate changes, and decide completion. Delegate bounded packets; never delegate the whole prompt unchanged.

Codeflow is a Codex-native translation of dynamic workflow harnesses. The generated workflow and ledger are the controller. Native agent tools perform the work; this skill does not pretend an external agent SDK exists.

## Required behavior

1. Read applicable repository instructions before planning.
2. Inspect working-tree state. Preserve unrelated and pre-existing changes.
3. Extract goal, acceptance criteria, constraints, authority boundaries, verification commands, and owner gates.
4. Choose the smallest workflow shape that materially improves reliability. Read [patterns.md](references/patterns.md).
5. For nontrivial work, create a run under `<workspace>/.codeflow/runs/<run-id>/` using `scripts/run_state.py`. Never stage `.codeflow/` or edit `.gitignore` automatically. Use `/tmp` only when the user accepts ephemeral state.
6. Generate task-specific packets and a DAG. Validate it before execution. Use the contract in [protocol.md](references/protocol.md).
7. Execute ready nodes in dependency order. Parallelize safe independent work; serialize integration and overlapping writes.
8. Validate every worker report against its acceptance criteria and evidence before marking it passed.
9. Require one fresh-context adversarial reviewer named Sol after every meaningful change. Sol assumes the change is wrong and independently reruns targeted proof.
10. Repair concrete findings, then send material fixes back to the same Sol reviewer. Do not spawn a second reviewer. Stop only on verified convergence, a real gate, exhausted declared budget, or user cancellation.
11. Reconcile final repository state against every acceptance criterion. Report outcome, proof, residual risks, and resumable run path.

Never mark work complete from an agent's claim alone. Never use agent agreement as proof. Tests, diffs, source evidence, or reproducible checks decide.

## Classify before compiling

Use `direct` when the task is one coherent low-risk edit. The coordinator may work inline, but still runs proportionate verification. If the user explicitly requested agents, add Sol as the one fresh reviewer.

Use `loop` for one implementation stream with repeated build-check-repair cycles.

Use `team` for heterogeneous independent roles such as exploration, architecture, tests, security, and adversarial review.

Use `batch` for many homogeneous read-heavy rows. Use a tournament only when alternatives are genuinely ambiguous or high-stakes; candidates propose, one coordinator selects, one writer implements.

Do not fan out merely to increase agent count. One well-scoped writer beats conflicting writers. Scope by coherent ownership and proof, not by an artificial one-card rule: an implementation or Sol review packet may cover one card, a batch, a related group, or multiple dependent cards when that produces the clearest integration boundary.

## Compile the workflow

Create a JSON workflow with schema version `1`. Every node packet must include:

- stable `id`, `kind`, objective, and concrete acceptance checks;
- dependencies and allowed paths;
- `mode`: `read`, `write`, or `verify`;
- write scope for each writer;
- forbidden actions and authority limits;
- evidence expected in the report;
- bounded stop condition.

Start from facts known now. After discovery, adapt by editing the workflow and running `sync`; never mutate completed or running node contracts. Add nodes only when evidence changes the problem. Sync may adapt nodes, but it cannot silently expand the user-approved goal, acceptance criteria, or budgets; surface that as a gate requiring a new approved run.

For meaningful writes, the graph must contain a downstream `verify` or `review` node with `fresh_context: true`. Review nodes are read-only. Keep integration owned by the coordinator or one designated writer.

Initialize and inspect runs:

```bash
python3 <skill-dir>/scripts/run_state.py validate workflow.json
python3 <skill-dir>/scripts/run_state.py init workflow.json --root .codeflow/runs
python3 <skill-dir>/scripts/run_state.py ready .codeflow/runs/<run-id>
python3 <skill-dir>/scripts/run_state.py status .codeflow/runs/<run-id>
```

If project instructions require a command wrapper, use it around Python.

## Dispatch agents

Feature-detect the tools in the current session; do not promise unavailable controls.

- Use native direct subagents for heterogeneous packets. Prefer built-in explorer behavior for read-only discovery and worker behavior for implementation when the spawn surface exposes roles.
- Use `spawn_agents_on_csv` only when it is actually available and there are at least eight homogeneous, independent, normally read-only rows. Supply a strict result schema, concurrency, and timeout. Otherwise use bounded direct-agent waves.
- Respect visible thread capacity. Reserve one slot for the coordinator. Default to at most four concurrent workers when capacity is unknown.
- If delegation is unavailable, execute read-only or trivial packets as separate local passes. A meaningful write cannot truthfully satisfy independent Sol review without a fresh agent; preserve the run as blocked instead of simulating independence.
- Do not claim a particular worker model unless the spawn tool actually accepts or reports it. Inherited current-model execution is valid.

Create exactly one review agent per run, with stable task name `sol-review` and display name Sol when the surface permits naming. Reuse that agent for every material recheck. Pass the native agent thread ID, not the display label, to `start --worker`; the ledger rejects a second ID and an implementation ID reused as Sol. When model controls are exposed, Sol uses GPT-5.6, never a Terra variant. Sol reasoning may be `low`, `medium`, or `high` only; default to `high` for adversarial review. Never set Sol to `minimal`, `none`, `xhigh`, `max`, or `ultra`. The ledger cannot attest model or freshness: verify them from native spawn evidence or visible current configuration. If that evidence is unavailable, keep the Sol gate blocked rather than claim compliance.

Every brief must be self-contained: role, objective, relevant paths, repository rules, permissions, acceptance, required proof, report shape, and “do not spawn subagents.” Explicitly forbid touching `.codeflow/`. Do not leak candidate answers into independent reviews. Keep agents that inspect untrusted web pages, issues, logs, or generated instructions read-only; the coordinator curates facts before an acting agent receives them.

Parallel reads are safe. Serialize writes in a shared workspace; whole-workspace snapshots deliberately reject peer changes. Put one card, a coherent batch, or multiple related cards in a single writer packet when that is the better scope. Use isolated worktrees only when the user and repository explicitly authorize them. Workers must not reset, restore, stage, commit, or rewrite changes they do not own. The coordinator alone updates the ledger and integrates.

Use the native wait/steer/interrupt controls. Send follow-up information to the existing agent instead of spawning duplicates. Close or release completed agents when the surface supports it.

## Record evidence

Before dispatch, mark a node running. After return, create a report JSON following [protocol.md](references/protocol.md), then finish the node:

```bash
python3 <skill-dir>/scripts/run_state.py start <run-dir> <node-id> --worker <native-agent-thread-id>
python3 <skill-dir>/scripts/run_state.py finish <run-dir> <node-id> <run-dir>/incoming/<node-id>.json
```

A passed report needs nonempty evidence. For write nodes, the state tool snapshots the workspace at start and rejects actual changes outside the declared scope or omitted from the report. Include expected generated byproducts in scope or suppress/clean them before finish. It fingerprints declared artifacts and stored reports. Missing or changed proof invalidates that node and its downstream dependents on resume.

On a recoverable failure, record it and use `retry` only while the node's attempt budget remains. On an owner decision, missing authority, or unavailable external state, record `blocked`; do not disguise it as failure or invent permission.

## Resume and converge

For “resume Codeflow,” locate the newest relevant run, inspect `status`, then run:

```bash
python3 <skill-dir>/scripts/run_state.py resume <run-dir> --stale-after 1800
python3 <skill-dir>/scripts/run_state.py ready <run-dir>
```

Resume rechecks passed artifacts and returns stale running nodes to the queue. Re-read current repository policy and working-tree state before continuing because both may have changed.

Bound repair loops in the workflow limits. When the same blocker persists, stop with the precise gate and preserved run path. Budget exhaustion is incomplete, not success.

Completion requires:

- all required nodes passed;
- every acceptance criterion mapped to evidence;
- Sol's independent review passed after the last material fix;
- targeted verification green in the current tree;
- no unintended path changes;
- generated runtime state left unstaged.

## User controls

Interpret these forms:

- `$codeflow <goal>`: compile and run.
- `$codeflow status`: summarize the newest relevant run.
- `$codeflow resume [run-id]`: revalidate and continue.
- `$codeflow cancel [run-id]`: interrupt active agents if possible, mark the run cancelled, preserve evidence.
- `$codeflow plan <goal>`: compile and validate only; make no implementation changes.

The user's explicit scope, approval mode, agent limit, time/token budget, or “no subagents” instruction overrides defaults. A terminal instruction such as “finish” increases persistence, not authority.
