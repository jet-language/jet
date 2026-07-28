# Agent workload conformance corpus

The corpus compares shipped Jet with Bash, Python, and Node.

Card #769 owns the task schema, the runner, the scoring rules, and the frozen inputs. It does not own the workload domains. Cards #1165 to #1169 own the domains, cards #1170 to #1172 own the Bash, Python, and Node baselines, and card #1173 owns the native OS matrix, the report, and the CI gate. Every manifest row still records `#769` in `tower_card`, because #769 owns the corpus machinery that the row is written against.

## Frozen contract

`tests/agent_workloads/manifest.tsv` is the task authority. A row fixes the task ID, domain, case, outcome, input, authority, adapters, platforms, evidence, and Tower cards.

`tests/agent_workloads/SHA256SUMS` fixes all task inputs and declared outputs. It holds 23 rows, and the test asserts that exact count. The test compares the recursive file set below `inputs/` and `expected/` with all checksum rows. An extra, missing, or changed file fails. The checksum reader also rejects an absolute path, a path with `..` or an empty part, a duplicate path, and a row it cannot split.

The completeness test also fixes each task ID, domain, case, and outcome in code, in `EXPECTED_TASKS`. A change cannot silently remove or reclassify a task. The same test fixes the manifest header, the row width of 13 fields, the corpus version, and the authority, adapters, platforms, evidence, and card fields of every row.

Each adapter gets the same declared input path as its only argument. Each adapter starts in an empty scratch working directory and inherits ambient host authority. The runner does not restrict or measure network access or writes outside the input tree or file.

The runner starts Jet through the integration-test `CARGO_BIN_EXE_jet` public CLI with `jet run --release`. The report records the CLI path, SHA-256 digest, and reported version. Corpus test evidence is not shipped product proof.

Default `jet run` rejects three of the four Jet adapters with E0956. The dev lens cannot yet run `core.files.walk()` or `core.process.cmd()`, so `repository_marker_scan.jet`, `git_diff_review.jet`, and `process_batch.jet` all fail. `incident_report.jet` reads its input file only, and it now runs under the default lens. Card #688 owns the remaining red gap.

## Scoring

A task gets one success point only when all required native adapters meet the declared exit status and stdout. An unavailable adapter does not pass.

The runner records cold and warm stderr byte counts and SHA-256 digests. It does not hide public CLI build or effect output.

A safety result is green only when the runner proves the required authority and process checks. An unavailable check stays red and names its blocking Tower card.

Cold time is the first public adapter command. Warm time is the next unchanged command. Output stability requires byte-identical cold and warm output.

Every process that the runner starts has a 120-second deadline. The runner kills and reaps that direct process on timeout. This does not prove descendant cleanup or orphan containment.

`source_tokens` is the count of nonempty runs split by Unicode whitespace. This stable lexical count does not claim to match a model tokenizer.

No aggregate Jet rank exists until the corpus records all required metrics and all named domains. A missing metric or domain stays `not-recorded` or `not-run`. It never becomes zero, not applicable, or pass.

## Current executable coverage

| Domain | Task | State | Blocking card |
| --- | --- | --- | --- |
| Repository search and edit | `repository-marker-scan` | Linux proved; macOS declared native but not run; Windows cannot count Bash as native | #1165, #1173 |
| Build, test, debug, and Git | `git-diff-review` | Linux Git diff inspection proved; build, test, and debug not run; macOS declared native but not run; Windows cannot count Bash as native | #1166, #1173 |
| Data cleanup and report generation | `incident-report-success`, `incident-report-malformed`, `incident-report-partial` | Linux proved; macOS declared native but not run; Windows cannot count Bash as native | #1167, #1173 |
| API and database work | None | Not run | #1167 |
| Browser and desktop work | None | Not run | #1168 |
| Document and media work | None | Not run | #1168 |
| MCP tools and hooks | None | Not run | #1169 |
| Long-running and interactive commands | `process-batch-success`, `process-batch-large-stderr`, `process-batch-timeout-recovery` | Linux proved for direct subprocesses; macOS declared native but not run; Windows cannot count Bash as native | #1169, #1173 |

The report records task success, source tokens, cold and warm wall time, output stability, platform, architecture, adapter versions, the ambient Git version, corpus evidence, and the canonical card. The Git workload runs one bounded `git diff --no-index --no-renames` over identical before and after trees. Each adapter parses and classifies modified, added, and deleted paths, including a same-content move and a path with a space. The data workload proves exact success, malformed-input, and partial-failure outcomes over identical TSV input for all four adapters. The process workload proves captured output, concurrent drain or explicit discard of 100,000 bytes of child stderr, direct-KILL cleanup of a TERM-resistant child at 50 milliseconds, and a successful next launch. The runner checks each adapter scratch directory before RAII deletion, so removing adapter cleanup fails the task. A cached Jet AOT run leaves exactly `build/<adapter-stem>`. An uncached run can also leave the regular public artifact `build/<adapter-stem>.rs`. Any other root or nested entry fails. Descendant-tree cleanup is not claimed. Agent tool calls, repair turns, peak memory, diagnostic quality, orphan processes, sandbox escapes, and cross-platform runs remain `not-recorded:#769`.

Network access and external writes remain `unmeasured:#769`. The declared-input hash check only proves that an adapter did not change its input file or tree.

The Linux AOT run is much slower than all three peers. The report keeps this loss red on card #666; it does not count it as parity.

## Tests

`tests/agent_workloads.rs` holds five tests. No test is ignored, gated, or skipped. Only the dangling-symlink case is `cfg(unix)`, and it runs on Linux and macOS.

| Test | Kind | What it proves |
| --- | --- | --- |
| `manifest_is_complete_frozen_and_non_vacuous` | Success | The manifest schema, the frozen task set, the 23 checksum rows, and the presence of every declared input, expected output, and adapter source. |
| `checksum_closure_rejects_drift_and_hostile_sums` | Hostile | An undeclared fixture, a drifted fixture byte, and a removed declared fixture each fail with an error that names the path. A hostile `SHA256SUMS` cannot smuggle in an absolute path or a `..` path, hide a fixture behind a duplicate row, or pass an unsplittable row. |
| `process_deadline_reaps_and_scratch_drop_cleans` | Failure | The runner deadline stops and reaps a sleeping child in under two seconds, and `Scratch` removes its directory on `Drop`. |
| `scratch_output_shape_rejects_arbitrary_build_residue` | Hostile | Seven separate scratch cases. A cache-hit and a cache-miss Jet layout both pass. A `build/` leak, a root leak, a nested directory and file, a wrong entry type, and a dangling symlink each fail with the exact violation list. |
| `equivalent_adapters_complete_declared_tasks` | Integration | All eight tasks across all four adapters, cold and warm: exit status, exact stdout, output stability, unchanged input authority, no undeclared scratch residue, and adapter agreement on the declared outcome. |

The frozen-input gate is proved by mutation, not by assertion alone. Changing one input byte fails with `fixture drift: <path>`. Adding an undeclared input fails with `checksum closure mismatch`. Deleting a declared expected output fails the presence check. Changing a task domain in the manifest fails with `task removed, added, or reclassified`.

The focused integration test is the first CI tier:

```sh
JET_NIX_TMP_CLEANED=1 timeout 20m scripts/agent/jet-env cargo test --test agent_workloads -- --nocapture
```

The last recorded Linux run passed 6 of 6 tests in 316.31 seconds. The count is six because the shared `common` module adds one test to the binary.
