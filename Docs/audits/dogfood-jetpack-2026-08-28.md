# Jetpack canary comparison — 2026-08-28

This report gives the owner a side-by-side view of the Jet jetpack canary and the shipped Rust jetpack. The baseline is integration commit `8b027acc1`. The current integration point is `eb576ea03`, Phase 1, blocked and not green. Jet implementation LOC and whitespace-token counts are measured at the current point; the Rust matched slice and all blocked runtime metrics remain `not measured`.

The running ledger is [`dogfood/jetpack/METRICS.md`](../../dogfood/jetpack/METRICS.md). Add measurements there at each integration point, then copy the same integration-point label into the tables below. Preserve earlier rows.

## Owner summary

| Item | Current fact | State |
| --- | --- | --- |
| Integration base | `8b027acc1` | `baseline fact` |
| Current integration point | `eb576ea03` | `Phase 1; blocked; not green` |
| Comparison | The Jet port and the shipped Rust jetpack run side by side. The Jet port has its own tree and distinct entry. | `baseline fact` |
| Non-replacement law | The shipped Rust jetpack is not replaced or edited. | `baseline fact` |
| Own-store law | The Jet port writes only below `~/.cache/jet-dogfood/jetpack-store/` by default. A port-defined override may select another owned root. | `baseline fact` |
| No-network default | Provider fetches do not run by default. Explicit network-marked test phases and recorded transcripts provide the provider boundary. | `baseline fact` |
| Standing canary | D-MEGAPROJ1, card `#2327`. | `baseline fact` |
| Jet implementation size | 1,961 physical LOC and 6,379 whitespace tokens across nine implementation files. | `measured` |
| Rust matched implementation size | No honest same-functionality Rust slice has been established. | `not measured` |
| Semantic check | The integration-worktree check exited 0 with `runner.jet has no problems` before the current harness-only correction. | `measured before correction` |
| Phase-1 parity | The default Phase-1 run is blocked by the findings below. | `blocked; not green` |
| Runtime and build metrics | Binary size, startup, verb latency, cold build, warm build, first-result latency, largest-file LSP timing, and error-cascade data are not recorded because Phase 1 is blocked. | `not measured` |

`not measured` means that no result exists. It is not zero, pass, parity, or not applicable.

## Phase parity bars

| Phase | Scope | Parity bar | Baseline state | Current integration state |
| --- | --- | --- | --- | --- |
| Phase 1 | Read-only verbs parse real `package.jet` manifests and `.jet/lock` files and produce `plan`, `list`, and `inspect` output. | Output matches the shipped Rust jetpack on the same inputs by documented byte equality or semantic equality. | `baseline fact` | `blocked; not green` |
| Phase 2 | Store behavior covers reserved names, case-fold collisions, no implicit normalization, own-root ingest, journal behavior, and compaction on synthetic fixtures. | Accept or reject verdicts and store layout match the shipped Rust jetpack against a throwaway store. | `baseline fact` | `not reached; not measured` |
| Phase 3 | Realization covers local-path and pre-fetched packages end to end in the own store. Recorded transcripts stand in for network providers. | End-to-end realization uses the own store and recorded provider results. | `baseline fact` | `not reached; not measured` |

## Metric command definitions

Run the commands from the repository root. Keep the Jet and Rust inputs and argv matched. The existing timer emits `wall_seconds` and `time_to_first_stdout_seconds`. A blocked metric stays `not measured`.

| Metric | Command definition | Capture |
| --- | --- | --- |
| Jet/Rust LOC | `wc -l -- <matched Jet files>` and `wc -l -- <matched Rust files>` | Physical source lines for matched functionality. |
| Jet/Rust tokens | `python3 -c 'from pathlib import Path; import sys; print(sum(len(Path(p).read_text(encoding="utf-8").split()) for p in sys.argv[1:]))' -- <matched files>` for each arm | Stable lexical source-token count from nonempty Unicode-whitespace runs; this is not a model-token count. |
| Jet/Rust binary size | `wc -c -- <Jet binary>` and `wc -c -- <Rust binary>` | Final executable bytes for the matched profile. |
| Jet/Rust startup | `python3 gauntlet/harness/timer.py -- <built binary> <same startup argv>` for each arm | `wall_seconds` for a prebuilt binary and the same startup argv. |
| Jet/Rust verb latency | `python3 gauntlet/harness/timer.py -- jet run dogfood/jetpack -- <verb argv>` and `python3 gauntlet/harness/timer.py -- <Rust binary> <verb argv>` | `wall_seconds` for the same fixture, verb, and argv. |
| Jet/Rust cold build | `python3 gauntlet/harness/timer.py -- jet build dogfood/jetpack` and the matched `<Rust build command>` with no prior artifact for the integration point | Cold build `wall_seconds`. |
| Jet/Rust warm build | The same build commands after the defined one-line source edit and without clearing the build state. | Warm rebuild `wall_seconds`. |
| Jet/Rust first-result latency | Use the verb commands above through `python3 gauntlet/harness/timer.py`. | `time_to_first_stdout_seconds` from process start to the first stdout byte. |
| Largest-file LSP timing | `JET_TIMING=1 jet self lsp` with the same request sequence for the largest Jet source file. | Per-request timing and the largest-file identity. |
| Error cascade | `jet check dogfood/jetpack` after one deliberate seeded breaking change, with the matched `<Rust check command>` for the Rust arm. | Diagnostic count and whether the first diagnostic names the seeded cause. |

## Current Phase-1 implementation scope

The current Jet implementation comparison uses these nine files and no test harness files. The Rust side has no honest same-functionality file list, so its list, LOC, and token count are `not measured`.

```text
wc -l -- dogfood/jetpack/package.jet dogfood/jetpack/entry.jet dogfood/jetpack/runner.jet dogfood/jetpack/src/cli/main.jet dogfood/jetpack/src/plan/plan.jet dogfood/jetpack/src/plan/read_only.jet dogfood/jetpack/src/lock/lock.jet dogfood/jetpack/src/model/ref.jet dogfood/jetpack/src/model/manifest.jet
python3 -c 'from pathlib import Path; import sys; print(sum(len(Path(p).read_text(encoding="utf-8").split()) for p in sys.argv[1:]))' -- dogfood/jetpack/package.jet dogfood/jetpack/entry.jet dogfood/jetpack/runner.jet dogfood/jetpack/src/cli/main.jet dogfood/jetpack/src/plan/plan.jet dogfood/jetpack/src/plan/read_only.jet dogfood/jetpack/src/lock/lock.jet dogfood/jetpack/src/model/ref.jet dogfood/jetpack/src/model/manifest.jet
```

| Jet implementation file | Physical LOC |
| --- | ---: |
| `dogfood/jetpack/package.jet` | 5 |
| `dogfood/jetpack/entry.jet` | 1 |
| `dogfood/jetpack/runner.jet` | 5 |
| `dogfood/jetpack/src/cli/main.jet` | 118 |
| `dogfood/jetpack/src/plan/plan.jet` | 970 |
| `dogfood/jetpack/src/plan/read_only.jet` | 221 |
| `dogfood/jetpack/src/lock/lock.jet` | 269 |
| `dogfood/jetpack/src/model/ref.jet` | 56 |
| `dogfood/jetpack/src/model/manifest.jet` | 316 |
| **Total** | **1,961** |

| Rust matched implementation file list | Physical LOC | Whitespace tokens | State |
| --- | ---: | ---: | --- |
| `not measured` | `not measured` | `not measured` | `not measured` |

The shipped Rust behavior spans broader modules, and no honest same-functionality Rust slice has been established. The Rust comparison is therefore `not measured`, not zero.

| Harness path | Physical LOC | Whitespace tokens | Comparison use | State |
| --- | ---: | ---: | --- | --- |
| `dogfood/jetpack/tests/transcript.jet` | 415 | `not measured` | Excluded from the nine-file implementation comparison. | `measured LOC; tokens not measured` |

## Semantic-check evidence

| Integration point | Command | Result | State |
| --- | --- | --- | --- |
| `eb576ea03` | `/home/nate/Projects/Github/jet/scripts/agent/jet-env /home/nate/Projects/Github/jet/target/debug/jet check dogfood/jetpack` | Exit 0 with `runner.jet has no problems`. This evidence predates the current harness-only correction. | `measured before correction` |

The semantic check does not make Phase 1 parity green. The default Phase-1 run remains blocked by the findings below.

## Jet/Rust LOC

| Integration point | Jet LOC | Rust LOC | State |
| --- | ---: | ---: | --- |
| `8b027acc1` | `not measured` | `not measured` | `not measured` |
| `eb576ea03` | 1,961 | `not measured` | `Jet measured; Rust not measured` |

## Jet/Rust tokens

| Integration point | Jet tokens | Rust tokens | State |
| --- | ---: | ---: | --- |
| `8b027acc1` | `not measured` | `not measured` | `not measured` |
| `eb576ea03` | 6,379 | `not measured` | `Jet measured; Rust not measured` |

## Binary size

| Integration point | Jet binary bytes | Rust binary bytes | State |
| --- | ---: | ---: | --- |
| `8b027acc1` | `not measured` | `not measured` | `not measured` |
| `eb576ea03` | `not measured` | `not measured` | `not measured` |

## Startup

| Integration point | Jet startup | Rust startup | State |
| --- | --- | --- | --- |
| `8b027acc1` | `not measured` | `not measured` | `not measured` |
| `eb576ea03` | `not measured` | `not measured` | `not measured` |

## Verb latency

| Integration point | Fixture or verb | Jet latency | Rust latency | State |
| --- | --- | --- | --- | --- |
| `8b027acc1` | `not measured` | `not measured` | `not measured` | `not measured` |
| `eb576ea03` | `not measured` | `not measured` | `not measured` | `not measured` |

## Cold and warm build

| Integration point | Jet cold | Jet warm | Rust cold | Rust warm | State |
| --- | --- | --- | --- | --- | --- |
| `8b027acc1` | `not measured` | `not measured` | `not measured` | `not measured` | `not measured` |
| `eb576ea03` | `not measured` | `not measured` | `not measured` | `not measured` | `not measured` |

## First-result latency

| Integration point | Fixture or verb | Jet first result | Rust first result | State |
| --- | --- | --- | --- | --- |
| `8b027acc1` | `not measured` | `not measured` | `not measured` | `not measured` |
| `eb576ea03` | `not measured` | `not measured` | `not measured` | `not measured` |

## Largest-file LSP timing

| Integration point | Largest Jet file | Jet timing | Rust timing | State |
| --- | --- | --- | --- | --- |
| `8b027acc1` | `not measured` | `not measured` | `not measured` | `not measured` |
| `eb576ea03` | `not measured` | `not measured` | `not measured` | `not measured` |

## Error cascade

| Integration point | Seeded change | Jet diagnostics | Rust diagnostics | First diagnostic names the cause | State |
| --- | --- | --- | --- | --- | --- |
| `8b027acc1` | `not measured` | `not measured` | `not measured` | `not measured` | `not measured` |
| `eb576ea03` | `not measured` | `not measured` | `not measured` | `not measured` | `not measured` |

## Findings and card numbers

This table records observed findings only. The baseline row remains unchanged, and the current rows use the integration evidence supplied for `eb576ea03`.

| Integration point | Finding | Evidence | Card | State |
| --- | --- | --- | --- | --- |
| `8b027acc1` | Canary scope | Ratified side-by-side jetpack canary. | `#2327` | `baseline fact` |
| `8b027acc1` | No finding recorded | No finding observation exists at the baseline. | `not measured` | `not measured` |
| `eb576ea03` | Generated Codable decoding can make ordinary error structs fail default-tier evaluation. | Finding reopened during the Phase-1 integration. | `#1310` | `reopened` |
| `eb576ea03` | Default evaluator coverage gap. Generated `CLIError.decode` deopts the whole package, then reaches E0956 `field recv`, blocking the default Phase-1 run. | Observed default-tier blocker. | `#2252` | `blocked` |
| `eb576ea03` | Generated AOT for imported modules uses unqualified root helpers and inconsistent JetErr/String errors, causing exit 101. | Observed AOT blocker. | `#2350` | `blocked` |
| `eb576ea03` | Package output cannot directly resolve an entry imported two directories deep; a runner adapter is required. | Observed package-output blocker. | `#2352` | `blocked` |
| `eb576ea03` | A direct match on struct-typed errors propagates instead of binding, forcing explicit unwrap branching. | Observed language-semantics blocker. | `#2354` | `blocked` |
| `eb576ea03` | Shipped `hangar list --json --offline` creates an otherwise empty `.locks/hangar.lock`; the parity harness normalizes only that exact artifact. | Observed parity artifact. | `#2355` | `observed` |
