# Jet quality audit and improvement pass

Evaluate Jet's requested scope as a language designer, compiler engineer, and
maintainer, then implement high-confidence ungated improvements. Follow
`AGENTS.md`, the orchestration prompt, `ponytail:ponytail`, the verification
skill, and Tower skills when the board is in scope. Load domain specs by trigger,
not as a blanket reading ritual.

## Baseline

- inspect Git diffs, active claims/tasks, worktrees, live Tower state, and
  ratified decisions;
- check `/tmp`, build health, and the narrow tests/examples relevant to observed
  risks;
- treat completion claims and old plans as hypotheses; verify against source and
  executable proof;
- distinguish design authority from implementation state.

## Audit

Assess:

- language coherence for beginners and experts;
- parser, sema, TIR/codegen, compiled/JIT parity, runtime, stdlib, packaging,
  editor/tooling, and deployment seams relevant to scope;
- safety, ownership, FFI, diagnostics, examples, snapshots, and generated files;
- spec/decision/code drift, stale syntax, dead paths, duplicate mechanisms,
  dependency violations, stubs, and false-green tests;
- current Tower blockers, stale claims, incomplete criteria, and orphaned work.

Rank findings by Jet's philosophy and observable user risk, never implementation
difficulty. Cite concrete files, tests, decisions, and card IDs.

## Act

Raise owner gates before implementation. Directly fix ungated correctness,
cleanup, docs alignment, test gaps, and stale-state problems. Use one implementer
per coherent change, path-scoped commits, recorded worktrees for concurrent
writes, and immediate integration/cleanup. Do not introduce abstractions or
dependencies merely to make the audit look substantial.

Each change needs focused behavioral proof, then one fresh Sol review with
implementer fixes and Sol recheck. Only the orchestrator runs a
major-push full suite.

Write or update one dated audit under `docs/reviews/` only if a durable report is
part of the request. Include verified current state, ranked findings, changes
landed, owner gates, exact proof, review evidence, and remaining risks. Update
Tower to reality. Do not leave duplicate cards, claims, branches, or worktrees.
