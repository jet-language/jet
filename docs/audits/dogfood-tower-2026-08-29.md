# Tower dogfood audit

Date: 2026-08-29

## Result

The Jet Tower implementation exists beside the Node implementation. The source comparison is complete for the snapshot-only scope. Jet static checks and editor probes provide partial evidence. Jet semantic parity and service runtime behavior remain unproven because the build, test, and startup paths stop on known blockers.

This canary patch does not edit `plugins/tower/**`. The Jet source is not a replacement for the Node app.

## Scope and inputs

Canonical Jet backend: `dogfood/tower/run.jet`.

Snapshot inputs: `2026-08-26`, `2026-08-27`, and `2026-08-28`.

Combined fixture hash: `83341cfafa7fd1946d6d093eaa9682220f05db9e183e19b58b46e973bd7cc8b5`.

The source scope excludes Node write and recovery because Jet is snapshot-only. Token counts use `simple-regex-v1` from `dogfood/tower/tests/metrics/node_baseline.mjs`.

## Architecture

Node remains the existing Tower application. Its full design includes write and recovery behavior. This audit does not compare those paths with Jet.

Jet adds a source-level backend and UI for the snapshot surface. The canonical backend is `dogfood/tower/run.jet`. The Jet surface is read-only for this canary. It does not establish mutation or recovery parity.

The two implementations are side by side. The Jet implementation does not replace, route around, or change the Node app.

## Implementation size

| Surface | Node LOC | Node tokens | Jet LOC | Jet tokens |
| --- | ---: | ---: | ---: | ---: |
| Backend | 2,883 | 31,224 | 2,361 | 19,950 |
| UI | 2,678 | 34,593 | 2,668 | 21,621 |
| Total | 5,561 | 65,817 | 5,029 | 41,571 |

The counts describe the scoped source surface. They do not show that Jet has replaced Node behavior.

## Reproduction commands

Run these commands from the repository root under the same canary conditions.

```text
scripts/agent/jet-env node dogfood/tower/tests/metrics/node_baseline.mjs dogfood/tower/tests/parity/fixtures/2026-08-28
scripts/agent/jet-env jet check dogfood/tower
scripts/agent/jet-env jet fmt --check dogfood/tower/run.jet
scripts/agent/jet-env jet build dogfood/tower
scripts/agent/jet-env jet build dogfood/tower/run.jet
scripts/agent/jet-env jet test dogfood/tower
scripts/agent/jet-env jet run dogfood/tower -- dogfood/tower/tests/parity/fixtures/2026-08-28 8090
scripts/agent/jet-env jet dev dogfood/tower -- dogfood/tower/tests/parity/fixtures/2026-08-28 8090
```

Run the Node command five times and use the median. The Node tokenizer is `simple-regex-v1`.

The supplied canary record names the existing diagnostic cascade and LSP probes but does not include their shell command. Their results are recorded below without inventing a reproduction command.

## Successful Node metrics

`scripts/agent/jet-env node dogfood/tower/tests/metrics/node_baseline.mjs dogfood/tower/tests/parity/fixtures/2026-08-28` produced these five-run medians on the `2026-08-28` fixture:

| Metric | Value |
| --- | ---: |
| Load | 93.989 ms |
| Project | 86.293 ms |
| Combined | 180.815 ms |
| Load, project, and serialize | 224.239 ms |
| RSS delta | 134,463,488 bytes |
| Serialized state | 19,094,088 bytes |

These are successful Node baseline values.

## Jet language and tooling evidence

`scripts/agent/jet-env jet check dogfood/tower` succeeds with exit 0 and 12 warnings:

- 4 intentional exact-float parity warnings;
- 4 branch-style warnings;
- 3 test-macro false empty-test warnings;
- 1 test-helper false unused warning.

`scripts/agent/jet-env jet fmt --check dogfood/tower/run.jet` passes.

The existing diagnostic cascade probe exits 1 in 1,201.678 ms with one E0003 and no cascade.

The LSP probe takes 2,583.733 ms, exits 0, and produces 3 frames, including `publishDiagnostics`.

These results show static, diagnostic, and editor behavior. They do not prove semantic parity or service behavior.

## Jet build and runtime blockers

The recorded Jet source-build probes all exit 101:

Source-build command: `scripts/agent/jet-env jet build dogfood/tower/run.jet`

| Condition | Time | Exit |
| --- | ---: | ---: |
| Cold source build | 18,768 ms | 101 |
| Warm source build | 18,935 ms | 101 |
| Final formatted source build | 19,785 ms | 101 |
| One-line comment rebuild | 20,343 ms | 101 |

`scripts/agent/jet-env jet build dogfood/tower` takes 1,508 ms and exits 1 with E1334, directory authority. This is tracked by #2352.

`scripts/agent/jet-env jet test dogfood/tower` takes 22,673 ms and exits 101 in generated `build/run.rs`. The related blockers are #2356 and #2350. The parity tests are present, but this command does not pass. No semantic parity test result is passing.

The first runtime command is:

```text
scripts/agent/jet-env jet run dogfood/tower -- dogfood/tower/tests/parity/fixtures/2026-08-28 8090
```

It reaches E3011 `PatternTest` after 74,959 ms. Wall time is 74,981 ms. Exit is 70. Failed-startup peak RSS is 4,684,036 KiB. The service never binds to port 8090. The default evaluator gap is tracked by #2252.

`scripts/agent/jet-env jet dev dogfood/tower -- dogfood/tower/tests/parity/fixtures/2026-08-28 8090` reaches the same E3011 in about 71 seconds. The watcher remains alive after the error, but the service does not bind.

## Runtime evidence limits

Because startup fails before bind, the following metrics are unavailable:

- steady-state Jet RSS;
- HTTP request latency;
- browser and UI smoke;
- mutation-route smoke;
- SSE smoke.

Unavailable does not mean zero. Unavailable does not mean passing. No service route was runtime-proven.

## Safety and read-only design

The Jet canary is snapshot-only. It does not add Node write or recovery behavior to the comparison. This keeps state changes outside the Jet scope and limits the canary to read-only snapshot behavior.

This boundary is a scope condition, not parity evidence for mutation or recovery. Mutation-route smoke is unavailable because the service never binds.

## Parity assessment

The shared snapshot set and combined fixture hash define the comparison input. Both source implementations exist for the scoped surface. Node has successful baseline measurements.

Jet has successful `check`, format, diagnostic, and LSP probes. Jet does not have a passing semantic parity test run. `scripts/agent/jet-env jet test dogfood/tower` exits 101, and `scripts/agent/jet-env jet run dogfood/tower -- dogfood/tower/tests/parity/fixtures/2026-08-28 8090` plus `scripts/agent/jet-env jet dev dogfood/tower -- dogfood/tower/tests/parity/fixtures/2026-08-28 8090` stop at E3011 before bind. The evidence supports side-by-side source implementation and tooling progress only. It does not support a claim of semantic or route parity.

## Blockers and ownership

- #2252 — default evaluator `PatternTest` gap. This blocks startup at E3011.
- #2350 — package/AOT closure. This is part of the `jet test` failure path.
- #2352 — directory package build. This blocks `jet build dogfood/tower` with E1334.
- #2356 — generated Rust/AOT payload ICE. This blocks `jet test dogfood/tower` with exit 101.
- #2331, #2332, and #2333 — child delivery cards remain part of the delivery work.
- #2327 — standing parent remains open.

No runtime claim should close these blockers. The canary needs a passing test path and a service that binds before runtime, route, RSS, UI, mutation, or SSE results can be recorded.
