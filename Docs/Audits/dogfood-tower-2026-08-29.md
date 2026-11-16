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

Jet exact-canonicalizes and validates fixed paths and the exact manifest. It rejects overlap with `plugins/tower/.tower`. The selected six files are each canonicalized, read once, checked against lowercase SHA-256, retained, then parsed from the verified bytes. This removes post-hash rereads. JS capture/metrics reject symlinks and hardlinks. Capture manifest validation enforces exact top-level/entry/source/file/hash-record keys and valid `captured_at`/`capture_day`.

## Source facts

The Jet API has read-only `GET` state, card, closed, history, lint, version, and stream routes. Every `POST`, `PUT`, `PATCH`, and `DELETE` request is refused by a final `/*path` guard, including paths outside `/api`.

The Jet UI source covers Board, Focus, Papercuts, Status, progress, milestones, burndown, ideas, filters and sorts, residual full-record fields, and immutable snapshot identity.

These source facts are not runtime proof.

## Evidence contract

The comparison unit is one immutable fixture directory. Each fixture has the same six files and a manifest; the three dated directories are the only accepted inputs. The combined fixture hash above identifies the recorded input set.

The loader reads each selected file once, verifies its manifest hash before parsing, and keeps all parsed values in local state. It never opens `plugins/tower/.tower`, writes a snapshot, or consults a live Tower store.

Node timing, RSS, serialized-size, source-size, diagnostic, and LSP values below are recorded canary measurements. This lane did not rerun validation or timing commands after the read-only route guard edit; unavailable values remain unavailable, not zero.

Source-size counts are the captured baseline for the listed source maps. They are not a post-edit claim and are not parity evidence.

## Implementation size

| Surface | Node LOC | Node tokens | Jet LOC | Jet tokens |
| --- | ---: | ---: | ---: | ---: |
| Backend | 3,083 | 33,473 | 2,497 | 21,332 |
| UI | 2,678 | 34,593 | 4,068 | 34,176 |
| Total | 5,761 | 68,066 | 6,565 | 55,508 |

In this scope, Jet is 804 LOC larger but 12,558 tokens smaller. Counts are not parity evidence.

## Focused proof commands

Run these commands from the repository root after the external blockers clear. This lane did not rerun them after the read-only route guard edit.

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
| Fixture load | 106.954 ms |
| Project | 69.708 ms |
| Load and project | 176.663 ms |
| Load, project, and serialize | 221.450 ms |
| RSS before | 58,949,632 bytes |
| RSS after | 199,352,320 bytes |
| RSS delta | 140,087,296 bytes |
| Serialized state | 19,094,088 bytes |

These are successful Node baseline values.

## Canary ledger

| Required measure | Recorded evidence | Status |
| --- | --- | --- |
| Cold and warm builds | Historical source builds: 18,768 ms cold and 18,935 ms warm; both exit 101. | Blocked by #2350/#2356 |
| Default run startup | 21.00 s; E0956 at `core.files.canonicalize`; no bind. | Blocked by #2252 |
| Dev startup | 120 s wait; no banner or port; same E0956. | Blocked by #2252 |
| LSP | 2,583.733 ms; exit 0; 3 frames including `publishDiagnostics`. | Recorded |
| Diagnostic cascade | 1,201.678 ms; exit 1; exactly one E0003. | Recorded |
| LOC and tokens | Captured source maps: Node 5,761 LOC/68,066 tokens; Jet 6,565 LOC/55,508 tokens. | Recorded baseline |
| RSS, request latency, API, SSE, and UI smoke | No Jet runtime values. | Unavailable until runtime binds |

This ledger records the canary evidence without converting a compiler or evaluator failure into a zero or pass.


## Recorded Jet validation

The captured pre-edit canary passed these checks:

`node --check` passed `board.js`, `capture.mjs`, and `node_baseline.mjs`.

`scripts/agent/jet-env jet fmt --check dogfood/tower/run.jet` passed.

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

It exits 101 after 24.88 s in generated `build/run.rs`. The failure is tracked by #2350 and #2356.

Earlier pre-hardening historical measurements for the same command were:

| Condition | Time | Exit |
| --- | ---: | ---: |
| Cold source build | 18,768 ms | 101 |
| Warm source build | 18,935 ms | 101 |
| One-line comment rebuild | 20,343 ms | 101 |

`scripts/agent/jet-env jet test dogfood/tower` exits 101 in generated `build/run.rs` after 27.78 s. This is tracked by #2350 and #2356. The parity tests are present, but this command does not pass.

`scripts/agent/jet-env jet build dogfood/tower` exits 1 after 1,508 ms with E1334. This directory build result is tracked by #2352.

## Runtime results

The final default command is:

```text
scripts/agent/jet-env jet run dogfood/tower -- dogfood/tower/tests/parity/fixtures/2026-08-28 8090
```

It exits 1 after 21.00 s with E0956 at `core.files.canonicalize`, tracked by #2252. It does not bind to port 8090.

Final dev readiness uses:

```text
scripts/agent/jet-env jet dev dogfood/tower -- dogfood/tower/tests/parity/fixtures/2026-08-28 8090
```

`jet dev` waits 120 s. The exact banner and port 8090 never become ready. It logs the same E0956, then the watcher stops. The exact banner exists in source but was not observed at runtime.

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

| Card | External blocker or delivery state |
| --- | --- |
| #2252 | External evaluator bug at `core.files.canonicalize`; default and dev commands stop before bind. |
| #2350/#2356 | External AOT/test compiler bugs; generated Rust exits 101. |
| #2352 | External compiler/package-build bug; directory build reports E1334. |
| #2357 | External formatter/linter contract bug; canonical L0508 contradiction remains. |
| #2331/#2332/#2333 | Dogfood delivery cards remain open; no compiler workaround is claimed. |
| #2327 | Standing parent remains open. |

The first four rows are external blockers, not defects in the dogfood source. No shim, exclusion, or fallback is used to hide them.

The exact banner exists in source but was not observed at runtime. These blockers prevent a passing semantic test and a bound service, so no runtime, route, RSS, UI, mutation, or SSE claim is proven.
