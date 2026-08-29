# Tower dogfood metrics

Date: 2026-08-29

## Identity

- Canonical Jet backend: `dogfood/tower/run.jet`
- Snapshot fixtures: `2026-08-26`, `2026-08-27`, and `2026-08-28`
- `captured_at`: `2026-08-29T02:50:10.825Z` in `capture-time.txt`
- Combined fixture hash: `a6ad831ca668a435bafdec6cc414d06c63f0467cc6e7c533fdeeb0a70c4bc355`
- Tokenizer: `simple-regex-v1` from `dogfood/tower/tests/metrics/node_baseline.mjs`
- Scope: snapshot-only. Node write and recovery are outside the comparison.

The Jet source implementation sits beside the Node app. This canary did not edit `plugins/tower/**`.

Jet exact-canonicalizes and validates fixed paths and the exact manifest. It rejects overlap with `plugins/tower/.tower`. The selected six files are each canonicalized, read once, checked against lowercase SHA-256, retained, then parsed from the verified bytes. This removes post-hash rereads. JS capture/metrics reject symlinks and hardlinks. Capture manifest validation enforces exact top-level/entry/source/file/hash-record keys and valid `captured_at`/`capture_day`.

## Source facts

The Jet API has read-only `GET` state, card, closed, history, lint, version, and stream routes. It rejects `POST`, `PUT`, `PATCH`, and `DELETE`.

The Jet UI source covers Board, Focus, Papercuts, Status, progress, milestones, burndown, ideas, filters and sorts, residual full-record fields, and immutable snapshot identity.

These source facts are not runtime proof.

## Source size

| Surface | Node LOC | Node tokens | Jet LOC | Jet tokens |
| --- | ---: | ---: | ---: | ---: |
| Backend | 3,083 | 33,473 | 2,497 | 21,332 |
| UI | 2,678 | 34,593 | 4,068 | 34,176 |
| Total | 5,761 | 68,066 | 6,565 | 55,508 |

In this scope, Jet is 804 LOC larger but 12,558 tokens smaller. Counts are not parity evidence.

## Node baseline

Command: `scripts/agent/jet-env node dogfood/tower/tests/metrics/node_baseline.mjs dogfood/tower/tests/parity/fixtures/2026-08-28`

Condition: run five times sequentially and use the median on the `2026-08-28` fixture.

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

These Node values are successful baseline measurements.

## Validation

These commands pass:

```text
scripts/agent/jet-env node --check dogfood/tower/src/board/board.js
scripts/agent/jet-env node --check dogfood/tower/tests/parity/capture.mjs
scripts/agent/jet-env node --check dogfood/tower/tests/metrics/node_baseline.mjs
scripts/agent/jet-env jet fmt --check dogfood/tower/run.jet
```

`scripts/agent/jet-env jet check dogfood/tower` exits 0 with 14 warnings:

- 4 L0507 branch-style warnings;
- 3 L2901 false empty-test warnings; assertions are in a helper;
- 5 L0502 warnings for intentional exact JavaScript/DataTree float and infinity semantics;
- 1 L0508 formatter/linter contradiction, tracked by #2357;
- 1 L0104 false unused-helper warning; the helper is called by `#Test` blocks.

## Jet build results

| Command or probe | Condition | Time | Exit | Result |
| --- | --- | ---: | ---: | --- |
| `scripts/agent/jet-env jet test dogfood/tower` | generated `build/run.rs` | 27.78 s | 101 | #2350/#2356 |
| `scripts/agent/jet-env jet build dogfood/tower/run.jet` | generated `build/run.rs` | 24.88 s | 101 | #2350/#2356 |
| `scripts/agent/jet-env jet build dogfood/tower` | directory package build | 1,508 ms | 1 | E1334, #2352 |

Earlier pre-hardening historical measurements for `scripts/agent/jet-env jet build dogfood/tower/run.jet`:

| Condition | Time | Exit |
| --- | ---: | ---: |
| Cold source build | 18,768 ms | 101 |
| Warm source build | 18,935 ms | 101 |
| One-line comment rebuild | 20,343 ms | 101 |

## Runtime results

Final default command:

```text
scripts/agent/jet-env jet run dogfood/tower -- dogfood/tower/tests/parity/fixtures/2026-08-28 8090
```

It exits 1 after 21.00 s with E0956 at `core.files.canonicalize`, tracked by #2252. It does not bind to port 8090.

Final dev readiness command:

```text
scripts/agent/jet-env jet dev dogfood/tower -- dogfood/tower/tests/parity/fixtures/2026-08-28 8090
```

`jet dev` waits 120 s. The exact banner and port 8090 never become ready. It logs the same E0956, then the watcher stops. The exact banner exists in source but was not observed at runtime.

The following Jet runtime values are unavailable. They are not zero and they are not passing:

- steady Jet RSS;
- request latency;
- API routes;
- SSE;
- mutation refusal;
- browser and UI smoke.

## Existing probes

The diagnostic cascade probe takes 1,201.678 ms, exits 1, and reports exactly one E0003.

The LSP probe takes 2,583.733 ms, exits 0, and produces 3 frames, including `publishDiagnostics`.

## Parity status

Side-by-side snapshot-only source exists. Semantic, runtime, route, and UI parity is not proven because external Jet blockers stop execution. Static checks and source inspection do not replace a passing semantic test, a bound service, route checks, or browser/UI smoke.

## Open blockers

- #2252 — default/dev evaluator `core.files.canonicalize`; final `jet run` and `jet dev` stop before bind.
- #2350/#2356 — AOT/test generated Rust exits 101.
- #2352 — directory package build reports E1334.
- #2357 — canonical formatter/L0508 contradiction.
- #2331/#2332/#2333 — delivery cards remain open.
- #2327 — standing parent remains open.
