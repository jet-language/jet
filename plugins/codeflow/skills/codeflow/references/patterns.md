# Workflow patterns

Select by dependency shape, not task size labels.

## Direct

One coherent edit, one owner, cheap proof. Coordinator inspects, changes, and verifies. Add a fresh read-only reviewer when requested or when the change is meaningful under repository policy.

## Loop

Use for a single implementation stream where each check determines the next change.

`investigate -> implement -> verify -> repair? -> verify`

Keep one writer. Each repair cycle must cite a new failing check or reviewer finding. Stop at the declared cycle limit with incomplete status.

## Team

Use when independent specialties improve coverage.

`parallel scouts -> coordinator synthesis -> writer(s) -> fresh reviewer -> repairs -> reviewer recheck`

Examples: repository map + test strategy + compatibility research; correctness + security + performance review. Scouts return evidence, not prose volume.

## Batch

Use for at least eight homogeneous independent rows: files, packages, findings, migrations, or test cases. Prefer native CSV fan-out when available. Each row has the same prompt template and strict result schema. Batch workers should usually be read-only; centralize edits after aggregation.

Run batch work in dependency waves when rows depend on earlier outputs. Persist row IDs so retry and resume do not duplicate completed work.

## Tournament

Use only when several plausible approaches have materially different correctness or architecture consequences.

1. Candidate agents independently propose evidence-backed designs.
2. A separate judge scores them against predeclared criteria.
3. Coordinator chooses or combines compatible parts and records why.
4. One writer implements the selected design.
5. A fresh reviewer validates the implementation, not the popularity of the design.

Never let candidate agents edit the shared tree in parallel.

## Adaptive expansion

Discovery may reveal missing work. Add a node only when it addresses a concrete new dependency, risk, or acceptance gap. Do not regenerate the graph wholesale. Preserve completed packet contracts and evidence so resume remains trustworthy.

## Selection table

| Signal | Pattern |
|---|---|
| One low-risk coherent edit | Direct |
| One writer, repeated feedback | Loop |
| Different independent roles | Team |
| Many same-shaped rows | Batch |
| Competing high-stakes designs | Tournament |

Combine patterns sparingly: a team can contain a batch discovery node and a loop implementation node. Keep one clear integration owner.
