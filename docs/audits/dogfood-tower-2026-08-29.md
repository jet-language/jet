# Tower dogfood audit

Date: 2026-08-29

## Result

Side-by-side snapshot-only source exists. Semantic, runtime, route, and UI parity is not proven because external Jet blockers stop execution.

The Jet implementation sits beside the Node implementation. This canary did not edit `plugins/tower/**`. The Jet source is not a replacement for the Node app.

## Identity and scope

Canonical Jet backend: `dogfood/tower/run.jet`.

Snapshot inputs are the exact fixtures `2026-08-26`, `2026-08-27`, and `2026-08-28`.

The `captured_at` value is `2026-08-29T02:50:10.825Z` in `capture-time.txt`.

Combined fixture hash: `a6ad831ca668a435bafdec6cc414d06c63f0467cc6e7c533fdeeb0a70c4bc355`.

The scope is snapshot-only. Node write and recovery are outside the comparison. Token counts use `simple-regex-v1` from `dogfood/tower/tests/metrics/node_baseline.mjs`.

The JS capture and metric harnesses reject symlinks and hardlinked regular files. Jet canonicalizes the fixed fixture root, every directory, and every file. It rejects canonical overlap with `plugins/tower/.tower`. It validates the exact schema, date, order, commits, fixture map, source map, file map, and capture time. It then verifies each SHA-256 before parity checks.

## Source facts

The Jet API has read-only `GET` state, card, closed, history, lint, version, and stream routes. It rejects `POST`, `PUT`, `PATCH`, and `DELETE`.

The Jet UI source covers Board, Focus, Papercuts, Status, progress, milestones, burndown, ideas, filters and sorts, residual full-record fields, and immutable snapshot identity.

These source facts are not runtime proof.

## Implementation size

| Surface | Node LOC | Node tokens | Jet LOC | Jet tokens |
| --- | ---: | ---: | ---: | ---: |
| Backend | 2,883 | 31,224 | 2,472 | 20,987 |
| UI | 2,678 | 34,593 | 4,069 | 34,180 |
| Total | 5,561 | 65,817 | 6,541 | 55,167 |

In this scope, Jet is 980 LOC larger but 10,650 tokens smaller. Counts are not parity evidence.

## Reproduction commands

Run these commands from the repository root under the same canary conditions.

```text
scripts/agent/jet-env node --check dogfood/tower/src/board/board.js
scripts/agent/jet-env node --check dogfood/tower/tests/parity/capture.mjs
scripts/agent/jet-env node --check dogfood/tower/tests/metrics/node_baseline.mjs
scripts/agent/jet-env node dogfood/tower/tests/metrics/node_baseline.mjs dogfood/tower/tests/parity/fixtures/2026-08-28
scripts/agent/jet-env jet check dogfood/tower
scripts/agent/jet-env jet fmt --check dogfood/tower/run.jet
scripts/agent/jet-env jet build dogfood/tower
scripts/agent/jet-env jet build dogfood/tower/run.jet
scripts/agent/jet-env jet test dogfood/tower
scripts/agent/jet-env jet run dogfood/tower -- dogfood/tower/tests/parity/fixtures/2026-08-28 8090
scripts/agent/jet-env jet dev dogfood/tower -- dogfood/tower/tests/parity/fixtures/2026-08-28 8090
```

Run the Node baseline five times sequentially and use the median. The Node tokenizer is `simple-regex-v1`.

The diagnostic cascade and LSP probes are existing probes. Their shell commands are not part of the supplied canary record, so this audit records their results without inventing commands.

## Node metrics

The final five-run sequential Node medians on the `2026-08-28` fixture are:

| Metric | Value |
| --- | ---: |
| Fixture load | 341.568 ms |
| Project | 172.049 ms |
| Load and project | 487.835 ms |
| Load, project, and serialize | 643.378 ms |
| RSS before | 57,438,208 bytes |
| RSS after | 225,058,816 bytes |
| RSS delta | 168,579,072 bytes |
| Serialized state | 19,094,088 bytes |

These are successful Node baseline values.

## Jet validation

`node --check` passes `board.js`, `capture.mjs`, and `node_baseline.mjs`.

`scripts/agent/jet-env jet fmt --check dogfood/tower/run.jet` passes.

`scripts/agent/jet-env jet check dogfood/tower` exits 0 with 14 warnings:

- 4 L0507 branch-style warnings;
- 3 L2901 false empty-test warnings; assertions are in a helper;
- 5 L0502 warnings for intentional exact JavaScript/DataTree float and infinity semantics;
- 1 L0508 formatter/linter contradiction, tracked by #2357;
- 1 L0104 false unused-helper warning; the helper is called by `#Test` blocks.

The diagnostic cascade probe takes 1,201.678 ms, exits 1, and reports exactly one E0003.

The LSP probe takes 2,583.733 ms, exits 0, and produces 3 frames, including `publishDiagnostics`.

These results show static, diagnostic, and editor behavior. They do not prove semantic parity or service behavior.

## Jet build results

The final source-build command is:

```text
scripts/agent/jet-env jet build dogfood/tower/run.jet
```

It exits 101 after 25.66 s. The generated Rust failure is tracked by #2350 and #2356.

Earlier pre-hardening historical measurements for the same command were:

| Condition | Time | Exit |
| --- | ---: | ---: |
| Cold source build | 18,768 ms | 101 |
| Warm source build | 18,935 ms | 101 |
| One-line comment rebuild | 20,343 ms | 101 |

`scripts/agent/jet-env jet test dogfood/tower` exits 101 in generated `build/run.rs` after 29.67 s. This is tracked by #2350 and #2356. The parity tests are present, but this command does not pass.

`scripts/agent/jet-env jet build dogfood/tower` exits 1 after 1,508 ms with E1334. This directory build result is tracked by #2352.

## Runtime results

The final default command is:

```text
scripts/agent/jet-env jet run dogfood/tower -- dogfood/tower/tests/parity/fixtures/2026-08-28 8090
```

It exits 1 after 19.95 s with E0956 at `core.files.canonicalize`, tracked by #2252. The service never binds to port 8090.

Final dev readiness uses:

```text
scripts/agent/jet-env jet dev dogfood/tower -- dogfood/tower/tests/parity/fixtures/2026-08-28 8090
```

Readiness waits 120 s. It never sees the exact banner or port 8090. It logs the same E0956, then the watcher stops. The exact banner exists in source but was not observed at runtime.

## Runtime evidence limits

The following Jet values remain unavailable:

- steady Jet RSS;
- request latency;
- API routes;
- SSE;
- mutation refusal;
- browser and UI smoke.

Unavailable does not mean zero. Unavailable does not mean passing.

No service route was runtime-proven.

## Parity assessment

The exact fixture set, capture time, combined hash, and source maps define the comparison input. Both source implementations exist for the scoped snapshot surface. Node has successful baseline measurements.

Jet has successful syntax, format, static-check, diagnostic, and LSP results. The source also shows read-only routes and the listed UI surfaces. These facts do not prove runtime behavior.

Semantic parity is not proven because `scripts/agent/jet-env jet test dogfood/tower` exits 101. Runtime, route, and UI parity are not proven because the final default and dev commands stop at E0956 before the service binds. RSS, request latency, API, SSE, mutation refusal, and browser/UI smoke remain unavailable.

## Blockers and ownership

- #2252 — default/dev evaluator `core.files.canonicalize`; final commands stop before bind.
- #2350/#2356 — AOT/test generated Rust exits 101.
- #2352 — directory package build reports E1334.
- #2357 — canonical formatter/L0508 contradiction.
- #2331/#2332/#2333 — delivery cards remain open.
- #2327 — standing parent remains open.

The exact banner exists in source but was not observed at runtime. These blockers prevent a passing semantic test and a bound service, so no runtime, route, RSS, UI, mutation, or SSE claim is proven.
