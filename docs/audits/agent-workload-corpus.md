# Agent workload conformance corpus

The corpus compares shipped Jet with Bash, Python, and Node.

Card #769 owns the task schema, the runner, the scoring rules, and the frozen inputs. It does not own the workload domains. Cards #1165 to #1169 own the domains, cards #1170 to #1172 own the Bash, Python, and Node baselines, and card #1173 owns the native OS matrix, the report, and the CI gate. Every manifest row still records `#769` in `tower_card`, because #769 owns the corpus machinery that the row is written against.

## Frozen contract

`tests/agent_workloads/manifest.tsv` is the task authority. A row fixes the task ID, domain, case, outcome, input, authority, adapters, platforms, evidence, and Tower cards.

`tests/agent_workloads/SHA256SUMS` fixes all task inputs and declared outputs. The completeness test asserts its exact frozen 82-row count. The test compares the recursive file set below `inputs/` and `expected/` with all checksum rows. An extra, missing, or changed file fails. The checksum reader also rejects an absolute path, a path with `..` or an empty part, a duplicate path, and a row it cannot split.

`tests/agent_workloads/domain_contract.tsv` fixes allowed dependencies, machine specification, hostile or normal variant, and the #769 scoring string for each contracted task. The repository discovery, semantic inspection/edit, Git, build/test recovery, and direct subprocess tasks are contracted here, including their hostile fixtures. The completeness test compares the file with its frozen in-test expectation.

The #1169 contract covers read-only MCP environment resources and denied-resource handling, PTY dialogue and closed-session handling, and service start/health/readiness/log/stop plus bounded readiness failure. Each task freezes its inputs, expected output, allowed dependencies, machine specification, normal or hostile variant, and the same #769 scoring string. The hostile cases exercise denied MCP access, a closed terminal session, readiness timeout, descendant cleanup, secret redaction, and unsupported authority.

The completeness test also fixes each task ID, domain, case, and outcome in code, in `EXPECTED_TASKS`. A change cannot silently remove or reclassify a task. The same test fixes the manifest header, the row width of 13 fields, the corpus version, and the authority, adapters, platforms, evidence, and card fields of every row.

Each adapter gets the same declared input path as its only argument. Each adapter starts in an empty scratch working directory and inherits ambient host authority. The runner does not restrict or measure network access or writes outside the input tree or file.

Build-sandbox hostile cases are a separate shared fixture at
`tests/fixtures/build_sandbox/hostile-corpus.tsv`. The build and hermetic
executors, plus the authority-bound agent executor, run that fixture through
the native child boundary. This ambient workload runner only records its
unsupported confinement dimensions as `unmeasured:#769`; its parsing of the
fixture is not isolation proof.

The runner starts Jet through the integration-test `CARGO_BIN_EXE_jet` public CLI with `jet run --release`. The report records the CLI path, SHA-256 digest, and reported version. Corpus test evidence is not shipped product proof.

The repository and Git Jet adapters execute through the public `jet run --release` path, including semantic inspection, semantic rename, empty search, changed diff, and empty diff. Cross-platform native runs remain owned by #1173; sibling workload domains keep their own implementation owners.

## Authority policy and receipt

`tests/agent_workloads/policy.tsv` is the canonical policy contract. `tests/agent_workloads.rs::policy_digest` hashes that exact contract as the one policy identity. It covers the frozen plan, launch transaction, process-group descendant cleanup, wall and output limits, captured outputs, and receipt formats. The same digest appears in the baseline receipt, the run policy line, every task machine line, every adapter result, and the generated report; each machine and result line also records the enforced authority string.

The manifest accepts only the enforced authority string. An unsupported authority value fails before an adapter starts. Baseline artifact paths reject absolute paths, empty components, and `..` components. Timed-out children are killed and reaped with their process group; scratch residue is checked before cleanup.

## Scoring

A task gets one success point only when all required native adapters meet the declared exit status and stdout. An unavailable adapter does not pass.

The runner records cold and warm stderr byte counts and SHA-256 digests. It does not hide public CLI build or effect output.

## Interpreter baseline receipt

`tests/agent_workloads/baselines/receipt.tsv` records one frozen Linux run for every task under Bash, Python, and Node. Each row pins the run ID, declared machine, interpreter version, task input digest, expected output digest, existing `#769` scoring string, exit status, cold and warm timings, output-stability result, raw stdout/stderr paths and hashes, and the shared policy digest. A supported row must match the checked-in expected output. An unsupported row must carry an explicit finding and no output path; it is never omitted.

The raw baseline files live under `tests/agent_workloads/baselines/outputs/`. The receipt validator is `tests/agent_workloads.rs::recorded_baselines_cover_frozen_tasks`. Capture uses the existing corpus runner and scorer:

```sh
JET_CORPUS_BASELINE_DIR=tests/agent_workloads/baselines \
JET_CORPUS_BASELINE_RUN_ID=2026-08-23-linux-x86_64-nix-core \
JET_CORPUS_BASELINE_MACHINE=linux-x86_64:nix-core \
scripts/agent/jet-env cargo test --test agent_workloads equivalent_adapters_complete_declared_tasks -- --nocapture
```

The capture command is one run on one machine. It does not add a benchmark runner or a second scoring model. The existing integration test still performs the cold/warm execution, exact-output comparison, input-authority check, and scratch-residue check.

## Native OS matrix and gate

`tests/agent_workloads/native_os_matrix.tsv` freezes the native matrix: Linux x86_64 and macOS require Jet, Bash, Python, and Node; Windows is excluded because Bash has no native adapter in this corpus. The gate rejects a host that is absent from this file or uses a disallowed architecture.

`tests/agent_workloads/jet_baseline.tsv` freezes every task that previously passed through Jet and keeps the existing loss-owner links. A Jet loss must keep a `#card` link or a `non-goal:` link.

`docs/audits/agent-workload-corpus-report.tsv` is the generated report. Each row scores Jet, Bash, Python, and Node for one task. `jet_vs_baselines=pass` requires exact output, exit status, cold and warm stability, unchanged input authority, and clean scratch state for all four adapters. The row also records source-token counts, cold and warm times, and the shared policy digest for each adapter run.

Run the gate on each required native host:

```sh
TMPDIR="$HOME/.cache/jet-test-scratch" bash tools/ci/agent-workload-gate.sh
```

The gate writes the report atomically, checks the frozen task set, and fails when a task that passed in `jet_baseline.tsv` no longer passes. Set `JET_AGENT_WORKLOAD_REPORT` for a per-host CI artifact. After Linux and macOS jobs finish, run `bash tools/ci/agent-workload-matrix.sh <report-dir>`; it requires one passing report for each required OS. Keep the committed report from the selected baseline host at the path above.

A safety result is green only when the runner proves the required authority and process checks. An unavailable check stays red and names its blocking Tower card.

Cold time is the first public adapter command. Warm time is the next unchanged command. Output stability requires byte-identical cold and warm output.

Every process that the runner starts has a 120-second deadline. On Unix, the runner gives each child a private process group and kills and reaps the group on normal completion, timeout, or output-limit failure. The service fixture also checks descendant cleanup; Windows remains outside the native Bash matrix.

`source_tokens` is the count of nonempty runs split by Unicode whitespace. This stable lexical count does not claim to match a model tokenizer.

No aggregate Jet rank exists until the corpus records all required metrics and all named domains. A missing metric or domain stays `not-recorded` or `not-run`. It never becomes zero, not applicable, or pass.

## Current executable coverage

| Domain | Task | State | Blocking card |
| --- | --- | --- | --- |
| Repository discovery, search, and edit | `repository-marker-scan`, `repository-marker-scan-empty`, `repository-semantic-inspection`, `repository-semantic-edit` | Linux proved; macOS declared native but not run; Windows cannot count Bash as native | #1165, #1173 |
| Git | `git-diff-review`, `git-diff-empty` | Linux Git diff inspection proved; macOS declared native but not run; Windows cannot count Bash as native | #1165, #1173 |
| Build, test, and debug | `build-test-failure-recovery` | Linux failed-check → successful-check → successful-test → run recovery proved; debug not run; macOS declared native but not run; Windows cannot count Bash as native | #1166, #1173 |
| Data cleanup and report generation | `incident-report-success`, `incident-report-malformed`, `incident-report-partial` | Linux proved; macOS declared native but not run; Windows cannot count Bash as native | #1167, #1173 |
| Structured data | `structured-data-transform`, `structured-data-hostile` | Linux production-path outputs proved; macOS declared native but not run; Windows cannot count Bash as native | #1167, #1173 |
| Database access | `database-access`, `database-hostile` | Linux production-path outputs proved; macOS declared native but not run; Windows cannot count Bash as native | #1167, #1173 |
| HTTP APIs | `http-api`, `http-hostile` | Linux production-path outputs proved; macOS declared native but not run; Windows cannot count Bash as native | #1167, #1173 |
| Browser and desktop work | `browser-automation-preflight`, `desktop-interaction-focus` | Linux production-path outputs proved; macOS declared native but not run; Windows cannot count Bash as native | #1168, #1173 |
| Document and media work | `document-markdown-inspection`, `media-asset-inventory` | Linux production-path outputs proved; macOS declared native but not run; Windows cannot count Bash as native | #1168, #1173 |
| MCP tools and hooks | `mcp-environment-readonly`, `mcp-environment-denied` | Linux production-path outputs proved; macOS declared native but not run; Windows unavailable | #1169, #1173 |
| Interactive terminals | `interactive-terminal-dialogue`, `interactive-terminal-closed` | Linux PTY dialogue, resize, and closed-session outputs proved; macOS declared native but util-linux `script` is unavailable; Windows unavailable | #1169, #1173 |
| Service lifecycle | `service-lifecycle-roundtrip`, `service-lifecycle-readiness-timeout` | Linux systemd-shim authority, health/readiness/log/stop, bounded timeout, and descendant cleanup outputs proved; macOS authority unavailable; Windows unavailable | #1169, #1173 |
| Subprocess control | `process-batch-success`, `process-batch-large-stderr`, `process-batch-timeout-recovery` | Linux proved for direct subprocesses; macOS declared native but not run; Windows cannot count Bash as native | #1166, #1173 |

The generated report records task status, per-adapter status, source-token counts, cold and warm wall time, platform, architecture, exit code, loss owner, canonical card, and the shared policy digest. The raw runner output also records adapter versions, the ambient Git version, corpus evidence, stderr digests, and the fields that remain `not-recorded:#769` or `unmeasured:#769`. The repository search workload freezes nested paths, a no-match case with a path containing spaces, and exact marker counts. The semantic-inspection workload reads the checked semantic index and compares definition, reference, and call-edge counts; the semantic-edit workload copies an input project, renames a resolved function through the existing Jet codemod engine, and proves that matching text in a string and comment stays unchanged. The Git workload runs one bounded `git diff --no-index --no-renames` over changed before and after trees, plus an identical-tree empty diff that accepts Git's zero exit. The build/test workload runs each adapter's native syntax check against an invalid source, records that failed check, then checks and runs the valid source. Each adapter parses and classifies modified, added, and deleted paths, including a same-content move and a path with a space. The data workload proves exact success, malformed-input, and partial-failure outcomes over identical TSV input for all four adapters. The browser workload validates the shipped BiDi profile and timeout contracts and attempts a closed-port `connect_profile`, with an unpinned profile, zero timeout, and refused connection as hostile rows. The desktop workload drives the shipped TUI focus group through two Tab events and an empty-key hostile row. The document workload walks Markdown files, counts headings and bullets, and rejects an empty heading. The media workload inventories shipped SVG and MP3 MIME mappings and rejects an unknown extension. The subprocess workload proves captured output, concurrent drain or explicit discard of 100,000 bytes of child stderr, direct-KILL cleanup of a TERM-resistant child at 50 milliseconds, and a successful next launch. The runner checks each adapter scratch directory before RAII deletion, so removing adapter cleanup fails the task. A cached Jet AOT run leaves exactly `build/<adapter-stem>`. An uncached run can also leave the regular public artifact `build/<adapter-stem>.rs`. Any other root or nested entry fails. The runner kills timed-out Unix process groups, and the #1169 service fixture checks delegated descendant cleanup. Cross-platform proof remains `not-run:#1173` until the native matrix runs.

The six structured-data, database, and HTTP #1167 tasks normalize JSON, use parameterized database queries, and post JSON to a loopback HTTP server. Each has a normal and hostile fixture plus frozen dependencies, machine specification, variant, and #769 scoring in `domain_contract.tsv`. `structured_data_database_http_production_paths_handle_success_and_hostile_inputs` executes the six Jet adapters through the public `jet run --release` path, checks exact success and hostile output, preserves input hashes, and rejects scratch residue.

The build/test recovery fixture now exercises the production `jet test --release` path between the successful check and the final run. The process-policy fixture exercises both default and AOT `jet run` paths with an output limit, unsupported authority refusal, timeout cancellation, and descendant cleanup.

The #1169 MCP workload speaks the shipped `jet self lsp` JSON-RPC resource path and proves that the environment projection is read-only; its hostile row rejects a denied resource and redacts the inherited secret. The terminal workload uses the shipped PTY process surface, resizes the session, completes a dialogue, and rejects a closed session. The service workload uses the shipped Jetpack service authority and lifecycle path to run health, readiness, logs, and stop, then exercises bounded readiness timeout and descendant cleanup. These rows do not claim cross-platform proof until #1173 runs the declared native matrix.

Network access and external writes remain `unmeasured:#769`. The declared-input hash check only proves that an adapter did not change its input file or tree.

The Linux AOT run is much slower than all three peers. The report keeps this loss red on card #666; it does not count it as parity.

## Tests

`tests/agent_workloads.rs` holds the corpus tests. No test is ignored, gated, or skipped. Only the dangling-symlink case is `cfg(unix)`, and it runs on Linux and macOS.

| Test | Kind | What it proves |
| --- | --- | --- |
| `manifest_is_complete_frozen_and_non_vacuous` | Success | The manifest schema, frozen task and domain-contract sets, checksum closure, and the presence of every declared input, expected output, and adapter source. |
| `checksum_closure_rejects_drift_and_hostile_sums` | Hostile | An undeclared fixture, a drifted fixture byte, and a removed declared fixture each fail with an error that names the path. A hostile `SHA256SUMS` cannot smuggle in an absolute path or a `..` path, hide a fixture behind a duplicate row, or pass an unsplittable row. |
| `process_deadline_reaps_and_scratch_drop_cleans` | Failure | The runner deadline stops and reaps a sleeping child in under two seconds, and `Scratch` removes its directory on `Drop`. |
| `scratch_output_shape_rejects_arbitrary_build_residue` | Hostile | Seven separate scratch cases. A cache-hit and a cache-miss Jet layout both pass. A `build/` leak, a root leak, a nested directory and file, a wrong entry type, and a dangling symlink each fail with the exact violation list. |
| `recorded_baselines_cover_frozen_tasks` | Receipt | Every frozen task has Bash, Python, and Node rows with one machine, pinned interpreter versions, frozen input/output hashes, existing scoring, and checked raw stdout/stderr. Unsupported rows require an explicit finding. |
| `policy_digest_covers_authority_and_receipt_contract` | Policy | Plan, launch transaction, descendant handling, limits, outputs, authority, and receipt formats use one digest; unsupported authority fails closed. |
| `receipt_artifact_paths_reject_escape_attempts` | Hostile | Absolute, empty-component, and parent-traversal receipt paths fail closed. |
| `native_os_matrix_is_frozen_and_names_current_host` | Matrix | The committed Linux/macOS/Windows matrix and current host declaration stay exact. |
| `jet_baseline_is_frozen_and_each_loss_has_an_owner` | Regression input | The Jet pass set covers every task and every loss owner names a card or ratified non-goal. |
| `repository_and_git_jet_adapters_use_production_paths` | Non-vacuity | The Jet repository and Git adapters retain their public semantic-inspection, codemod, filesystem, and bounded Git-diff calls. |
| `structured_data_database_http_jet_adapters_use_production_paths` | Non-vacuity | The Jet structured-data, database, and HTTP adapters retain their public JSON, SQLite, and loopback HTTP calls. |
| `mcp_terminal_service_jet_adapters_use_production_paths` | Non-vacuity | The #1169 Jet adapters retain the public MCP JSON-RPC, PTY resize/session, and Jetpack service lifecycle calls, including hostile failure markers. |
| `structured_data_database_http_production_paths_handle_success_and_hostile_inputs` | Production path | The six structured-data, database, and HTTP Jet cases execute through the public CLI with exact normal/hostile output, unchanged inputs, bounded execution, and clean scratch state. |
| `equivalent_adapters_complete_declared_tasks` | Integration | All declared tasks across all four adapters, cold and warm: exit status, exact stdout, output stability, unchanged input authority, no undeclared scratch residue, and adapter agreement on the declared outcome. |

The frozen-input gate is proved by mutation, not by assertion alone. Changing one input byte fails with `fixture drift: <path>`. Adding an undeclared input fails with `checksum closure mismatch`. Deleting a declared expected output fails the presence check. Changing a task domain in the manifest fails with `task removed, added, or reclassified`.

The focused integration test is the first CI tier:

```sh
JET_NIX_TMP_CLEANED=1 timeout 20m scripts/agent/jet-env cargo test --test agent_workloads -- --nocapture
```

The report-validator self-check is:

```sh
TMPDIR="$HOME/.cache/jet-test-scratch" bash tools/ci/test-agent-workload-gate.sh
```

The old six-test run note is obsolete. The current source also checks the interpreter receipt, native OS matrix, Jet baseline ownership, production-path adapter reachability, and the first-program digest; a fresh full corpus run remains a closeout proof owned by #1173.
