# Card #1414: compiled workload gate

This gate tests complete work. Core API rows alone do not close this gate.

The gate reuses the agent corpus TSV shape and the Core competitor ledger as
the source of comparison language names. It adds one peer ledger, one metric
contract, one target matrix, and one report validator. It does not add a
second benchmark runner.

## Frozen task set

`tests/compiled_workloads/manifest.tsv` has one row for each required domain.
The task files freeze the input shape and the output contract.

| Domain | Task |
| --- | --- |
| Systems | `systems-file-index` |
| Service | `service-json-http` |
| CLI | `cli-archive-filter` |
| Library | `library-json-roundtrip` |
| Compute | `compute-mandelbrot` |
| Embedded | `embedded-sensor-ring` |
| Cross-platform application | `cross-platform-notes` |

`tests/compiled_workloads/peer_ledger.tsv` freezes source URL, source
revision, build command, run command, dependency rule, source boundary, and
target list. Each task has one `best-applicable` peer. Candidate rows keep the
other named languages visible.

Public task references and revision notes live in
`docs/research/card-1414-compiled-peer-task-definitions.md`.

## Comparison law

Each outcome row must use the same task input, expected outcome, run identity,
dependency rule, and source boundary for Jet and its selected peer. A row must
record both tool versions. A result that does not build or run is not a pass.

Each task must record these metrics for Jet and the selected peer:

`source_effort`, `build_time`, `edit_time`, `runtime`, `memory`,
`artifact_size`, `diagnostics`, `debugging`, `deployment`, and
`unsafe_burden`.

Missing data fails the gate. The gate does not turn missing data into zero or
`not applicable`.

## Target law

`tests/compiled_workloads/tier_matrix.tsv` freezes Linux, macOS, Windows,
native AOT, cross-target AOT, and default JIT rows where the task supports
them. Freestanding firmware marks JIT as excluded because the target has no
JIT. Exclusions need a reason and evidence.

Hostile inputs are part of every task definition. The report must include
forced failures, variance samples, and removal-canary checks before a release
claim can pass.

## Owner law

The manifest records the current gate owner as `#1414`. A measured Jet loss
must carry one numeric live card or a ratified `non-goal:` owner in the outcome,
metric, and tier report. The release gate rejects an unowned loss.

External trust, community size, and ecosystem age are not metrics. They are
the only standing exclusions.

## Commands

Check the frozen contract:

```sh
TMPDIR="$HOME/.cache/jet-test-scratch" bash tools/ci/compiled-workload-gate.sh --contract
```

Check a complete report directory:

```sh
TMPDIR="$HOME/.cache/jet-test-scratch" bash tools/ci/compiled-workload-gate.sh --check <report-dir>
```

The gate has no checked-in measurement report yet. This is deliberate. Until
the clean native and cross-target runs, peer builds, hostile runs, and
independent review exist, the release claim stays red.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| `COMPILED-WORKLOAD-GATE` | card | #1414 |
<!-- /audit-dispositions -->
