# Card #1414: compiled workload gate

This gate tests complete work. Core API rows alone do not close this gate.

The gate reuses the agent corpus TSV shape and the Core competitor ledger as
the source of comparison language names. It adds peer and adapter ledgers, a
metric contract, a measurement policy, a target matrix, and one report
validator. `tools/ci/compiled-workload-runner.mjs` produces the report from
those frozen inputs; it does not add a second scoring framework.

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
target list. Each task has one `best-applicable` peer for the Jet loss
comparison. Candidate rows keep every other named peer visible and require
the same source execution when the host target applies.

`tests/compiled_workloads/adapter_ledger.tsv` binds each task to its Jet and
selected-peer source and hostile fixture. The
`tests/compiled_workloads/peer_adapter_ledger.tsv` binds every selected and
candidate peer row to its own source, hostile fixture, and immutable revision.
The measurement policy freezes sample count, variance limits, and a strict
`1.00` peer ratio.
Rust is the only noise-parity exception, with a `1.05` ratio.

Public task references and revision notes live in
`docs/research/card-1414-compiled-peer-task-definitions.md`.

## Comparison law

Each outcome row must use the same task input, expected outcome, run identity,
dependency rule, and source boundary for Jet and its selected peer. A row must
record both tool versions. A result that does not build or run is not a pass.

Each task must record these metrics for Jet and its selected peer. Every
applicable candidate row must also record the same metric set; a missing
candidate adapter, source execution, hostile execution, or metric row fails
the gate.

`source_effort`, `build_time`, `edit_time`, `runtime`, `memory`,
`artifact_size`, `diagnostics`, `debugging`, `deployment`, and
`unsafe_burden`.

Missing data fails the gate. The gate does not turn missing data into zero or
`not applicable`.

## Report contract

The measurement producer writes one report directory with ten TSV evidence
files. An independent review workflow adds `review.tsv`; the release checker
requires all eleven files.

| File | Purpose |
| --- | --- |
| `identity.tsv` | Candidate commit, host, tool identity, contract hash, source-closure hash, and sample policy. |
| `samples.tsv` | Every raw sample for every metric, task, and comparison side. |
| `statistics.tsv` | Recomputed median, range, variance, outlier count, threshold, and loss disposition. |
| `outcomes.tsv` | One row per manifest task. It binds the selected peer, task fixtures, tool versions, result, and review declaration. |
| `measurements.tsv` | One row for each metric for Jet and the selected peer. |
| `tiers.tsv` | One row for each declared execution tier for Jet and the selected peer. |
| `receipts.tsv` | Hashes for source, input, expected output, hostile input/output, tool identity, command, and exit status. |
| `tier_receipts.tsv` | Artifact/output hashes and execution receipt for every declared tier and comparison side. |
| `peer_coverage.tsv` | Source, normal/hostile output, command, target, and execution receipt for every selected and candidate peer row. |
| `peer_measurements.tsv` | The ten metrics for every applicable peer row, with explicit `not-applicable` rows for unsupported hosts. |

An outcome row must record `platform=linux|macos|windows` in its comparison
identity, plus a current `candidate=<40-hex-commit>`, `jet_tool_version`, and
`peer_tool_version`. Producer outcomes retain `review_status=pending` and
`review_evidence=-`; the separate `review.tsv` records the authenticated
workflow, run, actor, fairness scopes, and report digest. A pending producer
declaration is expected before review, but a missing or stale external review
fails the release gate.

Reports are host-scoped. A required row outside the report platform is
`not-applicable`; the release matrix must check each required host report.

Each Jet loss uses `loss_owner`. The value names one numeric live Tower card or
one ratified `non-goal:D-*` ruling. The validator reads the decision and
requires `status=ratified` with an owner outcome. A syntactically valid but
stale card does not close the owner obligation.

The report validator checks the task identity, selected peer, all-peer coverage
and measurement rows, metric set, execution tiers, review declaration, and
loss-owner field. It does not create measurements or turn unavailable data
into a pass. It recomputes each lower-is-better Jet metric against the
selected peer using strict `<1.00` superiority; Rust uses `<=1.05` noise
parity. A report cannot hide a measured loss behind a `measured` label.
Measurement toolchain IDs must also match the versions recorded by the
outcome row.

## Open gate findings

The selected TypeScript/ECMAScript browser peer for `cross-platform-notes`
declares Linux, macOS, Windows, and web, so the frozen target declaration now
matches the required web row. The web gate remains open until a report proves
an actual peer web artifact and execution; a source hash or native replay is
not web evidence. Do not mark the web row excluded without an owner ruling.

The validator checks the loss-owner shape. Closeout still needs a live Tower
lookup for each numeric card and a ratified ruling for each `non-goal:D-*` value.
The gate also needs a real clean-machine report and fresh review evidence.

## Target law

`tests/compiled_workloads/tier_matrix.tsv` freezes Linux, macOS, Windows,
native AOT, cross-target AOT, and default JIT rows where the task supports
them. Freestanding firmware marks JIT as excluded because the target has no
JIT. The four embedded AOT rows are also retained as explicit exclusions:
the freestanding profile is not shipped. Those rows name research #2046 and
implementation follow-up #2300. The embedded task stays in the manifest and
is not counted as a hosted win.

The six hosted tasks keep five required rows each: Linux, macOS, and Windows
AOT; the declared cross-target AOT row; and Linux default JIT. The web row for
`cross-platform-notes` stays required.

Hostile inputs are part of every task definition. The report must include
forced failures, variance samples, and removal-canary checks before a release
claim can pass.

`tests/compiled_workloads/canaries.tsv` names the removal mutations. The
self-check exercises every row. It covers missing outcomes, unowned losses,
missing metrics, missing tier proof, changed inputs, missing fresh review,
stale candidate identity, and missing all-peer measurement coverage.

## Owner law

The manifest records the current gate owner as `#1414`. A measured Jet loss
must carry one numeric live card or a ratified `non-goal:D-*` owner in the
outcome, metric, and tier report. The release gate rejects an unowned loss.

External trust, community size, and ecosystem age are not metrics. They are
the only standing exclusions.

## Commands

Check the frozen contract:

```sh
TMPDIR="$HOME/.cache/jet-test-scratch" bash tools/ci/compiled-workload-gate.sh --contract
```

Review and check a producer report in the independent workflow:

```sh
REVIEW_RUN=1 REVIEW_ACTOR=reviewer \
  node tools/ci/compiled-workload-review.mjs <report-dir>
```

Check a complete reviewed report directory:

```sh
TMPDIR="$HOME/.cache/jet-test-scratch" bash tools/ci/compiled-workload-gate.sh --check <report-dir>
```

The gate has no checked-in measurement report yet. This is deliberate. Until
the clean native and cross-target runs, peer builds, hostile runs, and
independent review exist, the release claim stays red. The excluded embedded
rows remain visible until #2300 supplies the ratified freestanding profile and
its production proof.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| `COMPILED-WORKLOAD-GATE` | card | #1414 |
<!-- /audit-dispositions -->
