# Card #1415 — Script-to-system continuity

The source fixture is [`run.jet`](../../examples/continuity/script_to_system/run.jet). It starts as one useful file. It has no `package.jet`, lockfile, generated wrapper, or hidden project directory.

The focused receipt is [`tests/script_to_system.rs`](../../tests/script_to_system.rs). It copies only `run.jet` into a scratch directory, records action/edit/build latency plus failure/recovery/artifact rows, checks the observable result at each stage, runs the forced interpreter, and compares a clean second emission.

## Journey

| Stage | User action | Source change | Artifact or proof | Reversal or failure record |
|---|---|---|---|---|
| Script | `jet run run.jet -- --minutes 1` | None | `60` from the typed CLI default path | No project files exist. |
| Forced interpreter | `jet run --interpret run.jet -- --minutes 1` | None | Tier-0 interpreter exit code, stdout, and stderr match the direct run; stdout matches `expected.out` | The fixture is applicable to the forced interpreter; this is a parity check, not a second program path. |
| Package | `jet init run.jet` | Adds generated `package.jet` only | Package identity exists; `run.jet` bytes are unchanged | The test keeps the source bytes as the rollback baseline. |
| Dependency | Create a local `support` package; run `jet add support --path ./support` and `jet fetch` | Adds one path edge and `.jet/lock` | The dependency graph is recorded without changing the program call path | Lock bytes are checked before and after split/fold. |
| Configuration and outputs | Add typed `settings`, `environments`, and `outputs` facts to `package.jet`; the one-file source already carries the checked `Executable`/`Service` addresses | Adds project structure; `run.jet` is unchanged | `jet explain`, executable `app`, and service `api` all point at the existing functions | `jet split env --check` is dry-run proof. |
| Installed command | Attempt `jetpack tool install ./ --as pulse --trust` | None | The current command fails closed with registered `E1272`; no installed command is claimed | The focused test checks source, package, and lock bytes are unchanged after the failure. A native compatibility output is still required. |
| Dev lens | `jet dev run.jet` | None | The same `dev` helper calls `serve` and returns `60` | The direct script reports `L0104` for the unselected helper; it is a warning, not a different semantic path. |
| Tested CLI | `jet test run.jet`; `jet run run.jet -- --minutes 1` | None | The same `seconds` function returns `60` in test and JIT CLI execution | Test failure output includes the complete compiler report. |
| Service | `jet run --quiet --output api run.jet` | None | The typed `Service` output calls the same `serve` function and returns `60` | Explicit output selection avoids a second service entry. |
| Native executable | `jet build run.jet`; run `build/run` | None | Native executable prints `60` | The source remains the same file. |
| Library projection | Add `core: .Library{…}` to the package outputs, then run `jet build --lib run.jet` | Adds only the final package output record | The native Library emits the static/shared, header, `.jetlib`, and C binding artifacts | The source and existing executable/service outputs are unchanged. |
| Embeddable Library compatibility probe | `jet build --lib run.jet` | None | `target/libpulse.a`, `target/libpulse.so`/`.dylib`/`.dll`, `target/pulse.h`, `target/pulse.jetlib`, and C binding are emitted; the C host smoke call prints `60` | This checks the current native Library/header path only. It does not claim the open #1343 guest contract. |
| Structural transition | `jet split env --check`, `jet split env`, run, then `jet fold package/env.jet --check` and `jet fold package/env.jet` | Moves the closed environment fact and then restores it | The split and fold checks report the same canonical graph fingerprint; fold restores exact `package.jet`, source bytes, lock bytes, and removes `package/env.jet` | Stale and ambiguous journal failures are covered by `package_transition_cli_covers_split_fold_init_restore_and_failures` in `tests/jetpack_engine.rs`. |

The dependency is intentionally not imported by `run.jet`. This proves that adding a graph edge does not require a semantic rewrite. The package metadata points all output kinds at the same checked functions and `seconds` type.

The manifest-free source carries the checked `Executable`/`Service` addresses and a default `app` selection in the same file. The direct command and the explicit `api` command both return `60`; no second service source or hidden script mode is introduced. The direct lens reports `L0104` for the reserved `dev` helper until `jet dev` selects it.

## Host matrix

The focused test writes version 2 `script-to-system.tsv`.
It records host identity, operation, and elapsed time for each action, edit, build, failure, recovery, and artifact.
Artifact rows carry SHA-256 content hashes.
Split and fold rows carry source, package, lock, and canonical graph identities.
The test reloads checked `PackageFacts` after install, split, and fold, and compares the dependency map separately from the transition-reported fingerprint.
The install row carries `E1272` and the stderr hash.
Export rows carry hashes for the executable and native Library outputs, including the clean rebuild.
The rollback row proves that source, package, lock, graph, and output identities remain unchanged.
`expected.out` is the committed stdout golden.
[`tests/suites.txt`](../../tests/suites.txt) registers `tests/script_to_system`.
The `native-session-matrix` CI job runs this binary on clean Linux, macOS, and Windows runners.
Each runner writes and checks a native receipt. The proof does not rely on a cross-compiled claim.

The current checkout can prove only its Linux row locally.
The `native-session-matrix` job runs the macOS and Windows rows.
A disposable Linux run with the prebuilt binary exercised the direct, package test, typed service, config explain, AOT executable, native Library, C host, split, and fold commands.
The first focused test run found a stale lock baseline after those later build actions (`test result: FAILED; 9 passed; 1 failed`).
The baseline now captures immediately before split.
A Cargo rerun reached an unrelated edit in `crates/jet-foundation/src/Syntax/package_files.rs` and stopped on Rust `E0658` before the test ran.

A fresh review found missing artifact hashes, a discarded split graph fingerprint, no fold dry-run receipt, and no link between install diagnostics and export outputs.
The focused test now records these identities and runs `--interpret` against the fixture.
It removes the first executable and Library outputs, uses a fresh cache, and compares the second emission by bytes and SHA-256.
Local-source install still fails closed with `E1272`.
The journey records that prerequisite failure and does not claim an installed command.
The native Library C-host check is only an artifact and header probe.
It does not claim #1343's guest contract. `LibraryExport` and `CmdCompile` remain outside this card.
This is review evidence, not owner acceptance.

## Criteria 4 and 6 review

Criterion 4 has receipt evidence for each named transition; the focused test still needs its batched run.
`split-preservation` records source, package, generated file, lock, and graph hashes.
`fold-preservation` records the same graph and checks that `jet fold --check` changes nothing.
`tool-install` records the complete `E1272` diagnostic, its stderr hash, independently reloaded graph/dependency identity, and unchanged source, package, and lock hashes.
`export-reproducibility` records native Library and executable hashes and carries the install diagnostic hash as its failure-to-output link.
`export-clean-rebuild` compares a fresh output tree and cache by bytes and hash, with both build latencies in the main receipt.
`fold-rollback` records restored source, package, lock, independently reloaded graph, and output hashes.

Criterion 6 is closed by a fresh source-growth review.
The reviewer compared each Journey row with its corresponding action in the focused test.
Script, interpreter, dev, test, service, executable, and native-host stages keep using `run.jet` and the `seconds` function.
`init` adds `package.jet`; dependency setup adds `support/` and `.jet/lock`; configuration adds typed package facts; Library export adds native artifacts.
The install attempt adds nothing because `E1272` stops it before a write.
Split and fold change only the package layout, and the test records a `*-source-ownership` proof at every structure boundary.
The review confirms that growth adds structure when a stage needs it and does not add a second program path.

## Canonical surfaces used

- `jet init` creates package metadata from a script.
- `jet add` and `jet fetch` own local dependency and lockfile growth.
- `jet test`, `jet run`, `jet explain`, `jet build`, `jet split`, and `jet fold` are the user-facing transitions.
- `jet build --lib` emits the current native Library compatibility surface. The generated C header supports the bounded C-host smoke check; #1343 owns the separate guest contract.
- `jet install` is not used: the current package tool is `jetpack tool install`. Local source installation currently stops at `E1272` until a pinned compatibility output exists.
