# Agent workload conformance corpus

Card #769 owns this corpus. The corpus compares shipped Jet with Bash, Python, and Node.

## Frozen contract

`tests/agent_workloads/manifest.tsv` is the task authority. A row fixes the task ID, domain, case, outcome, input, authority, adapters, platforms, evidence, and Tower card.

`tests/agent_workloads/SHA256SUMS` fixes all task inputs and declared outputs. The test compares the recursive file set with all checksum rows. An extra, missing, or changed file fails.

The completeness test also fixes each task ID, domain, case, and outcome in code. A change cannot silently remove or reclassify a task.

Each adapter gets the same input root as its only argument. Each adapter starts in an empty scratch working directory and inherits ambient host authority. The runner does not restrict or measure network access or writes outside the input tree.

The runner starts Jet through the integration-test `CARGO_BIN_EXE_jet` public CLI with `jet run --release`. The report records the CLI path, SHA-256 digest, and reported version. Corpus test evidence is not shipped product proof.

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
| Repository search and edit | `repository-marker-scan` | Native Linux and macOS task; Windows cannot count Bash as native | #769 |
| Build, test, debug, and Git | None | Not run | #769 |
| Data cleanup and report generation | None | Not run | #769 |
| API and database work | None | Not run | #769 |
| Browser and desktop work | None | Not run | #769 |
| Document and media work | None | Not run | #769 |
| MCP tools and hooks | None | Not run | #769 |
| Long-running and interactive commands | None | Not run | #769 |

The first report records task success, source tokens, cold and warm wall time, output stability, platform, architecture, adapter versions, corpus evidence, and the canonical card. Agent tool calls, repair turns, peak memory, diagnostic quality, orphan processes, sandbox escapes, and cross-platform runs remain `not-recorded:#769`.

Network access and external writes remain `unmeasured:#769`. The input-tree hash check only proves that an adapter did not change its declared input.

The focused integration test is the first CI tier:

```sh
JET_NIX_TMP_CLEANED=1 timeout 20m scripts/agent/jet-env cargo test --test agent_workloads -- --nocapture
```
