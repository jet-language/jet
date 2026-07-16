# Codeflow protocol

The coordinator owns workflow compilation, ledger mutation, integration, and final truth. Workers own bounded packets only.

## Workflow document

```json
{
  "schema_version": 1,
  "goal": "Observable end state",
  "workspace": "/absolute/project/path",
  "limits": {
    "max_parallel": 4,
    "max_attempts": 2,
    "max_cycles": 2
  },
  "acceptance": ["Externally checkable condition"],
  "nodes": [
    {
      "id": "inspect-api",
      "kind": "investigate",
      "mode": "read",
      "objective": "Locate the owning API and invariants",
      "depends_on": [],
      "paths": ["src", "tests"],
      "acceptance": ["Return file and line evidence"],
      "forbidden": ["edit files", "spawn subagents"],
      "fresh_context": false
    },
    {
      "id": "implement",
      "kind": "implement",
      "mode": "write",
      "objective": "Implement the accepted behavior",
      "depends_on": ["inspect-api"],
      "paths": ["src/feature.py", "tests/test_feature.py"],
      "write_scope": ["src/feature.py", "tests/test_feature.py"],
      "acceptance": ["Targeted test passes"],
      "forbidden": ["reset unrelated changes", "spawn subagents"],
      "fresh_context": false
    },
    {
      "id": "adversarial-review",
      "kind": "review",
      "mode": "verify",
      "objective": "Assume the implementation is wrong and find concrete defects",
      "depends_on": ["implement"],
      "paths": ["src/feature.py", "tests/test_feature.py"],
      "acceptance": ["Rerun targeted proof", "Report actionable findings or clean verdict"],
      "forbidden": ["edit files", "use implementer reasoning", "spawn subagents"],
      "fresh_context": true
    }
  ]
}
```

Required node fields: `id`, `kind`, `mode`, `objective`, `depends_on`, `paths`, `acceptance`, `forbidden`, and `fresh_context`. Write nodes also require nonempty `write_scope`. Node IDs use lowercase letters, digits, `_`, and `-`.

Kinds are `investigate`, `design`, `implement`, `test`, `review`, `verify`, `synthesize`, or `gate`. Modes are `read`, `write`, or `verify`.

Every dependency must exist and the graph must be acyclic. Every meaningful write needs a downstream fresh `review` or `verify` node. A review packet must not contain the implementer's reasoning or desired verdict.

## Worker brief

Send the packet plus:

- applicable repository rules and exact command wrapper;
- current dirty-tree facts relevant to owned paths;
- read/write permission and write scope;
- required evidence and tests;
- attempt and stop limits;
- report schema below;
- explicit prohibition on nested delegation.

Workers must surface blockers immediately. They never stage, commit, reset, restore, integrate, edit the ledger, or expand their authority unless the packet explicitly grants that action.

## Worker report

```json
{
  "status": "passed",
  "summary": "What was established or changed",
  "evidence": ["path:line and observed fact", "test command and result"],
  "artifacts": ["src/feature.py", "tests/test_feature.py"],
  "checks": [
    {"command": "python -m unittest tests.test_feature", "status": "passed", "detail": "2 tests"}
  ],
  "changed_paths": ["src/feature.py", "tests/test_feature.py"],
  "findings": [],
  "risks": [],
  "next": []
}
```

`status` is `passed`, `failed`, or `blocked`. Passed reports require nonempty evidence. Artifact and changed paths are workspace-relative, cannot escape the workspace, and must fit the packet's declared paths. The ledger computes hashes; workers do not.

Reviewer findings should include severity, location, failure mode, evidence, and required fix. “Looks good” without rerun proof is not a passed review.

## State model

Nodes move through:

`pending -> ready -> running -> passed | failed | blocked`

`failed -> ready` is allowed while attempts remain. `blocked` requires changed authority or external state before retry. Resume may move stale `running` to `ready`, and may invalidate `passed` nodes whose artifacts changed. Invalidation cascades to downstream nodes.

Only the coordinator mutates `run.json`. Writes are atomic. Events retain transition evidence. `cancelled` is a run state, never a successful node state.

## Acceptance ledger

Before final completion, map every top-level acceptance item to one or more passed node evidence entries. A passed graph without full acceptance coverage remains incomplete.
