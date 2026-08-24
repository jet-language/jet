# Card #1415 — Script-to-system continuity

The source fixture is [`run.jet`](../../examples/continuity/script_to_system/run.jet). It starts as one useful file. It has no `package.jet`, lockfile, generated wrapper, or hidden project directory.

The focused receipt is [`tests/script_to_system.rs`](../../tests/script_to_system.rs). It copies only `run.jet` into a scratch directory, records command latency, and checks the observable result at each stage.

## Journey

| Stage | User action | Source change | Artifact or proof | Reversal or failure record |
|---|---|---|---|---|
| Script | `jet run run.jet -- --minutes 1` | None | `60` from the typed CLI default path | No project files exist. |
| Package | `jet init run.jet` | Adds generated `package.jet` only | Package identity exists; `run.jet` bytes are unchanged | The test keeps the source bytes as the rollback baseline. |
| Dependency | Create a local `support` package; run `jet add support --path ./support` and `jet fetch` | Adds one path edge and `.jet/lock` | The dependency graph is recorded without changing the program call path | Lock bytes are checked before and after split/fold. |
| Configuration and outputs | Add typed `settings`, `environments`, and `outputs` facts to `package.jet`; the one-file source already carries the checked `Executable`/`Service` addresses | Adds project structure; `run.jet` is unchanged | `jet explain`, executable `app`, and service `api` all point at the existing functions | `jet split env --check` is dry-run proof. |
| Installed command | Attempt `jetpack tool install ./ --as pulse --trust` | None | The current command fails closed with registered `E1272`; no installed command is claimed | The focused test checks the package file is unchanged after the failure. A native compatibility output is still required. |
| Dev lens | `jet dev run.jet` | None | The same `dev` helper calls `serve` and returns `60` | The direct script reports `L0104` for the unselected helper; it is a warning, not a different semantic path. |
| Tested CLI | `jet test run.jet`; `jet run run.jet -- --minutes 1` | None | The same `seconds` function returns `60` in test and JIT CLI execution | Test failure output includes the complete compiler report. |
| Service | `jet run --quiet --output api run.jet` | None | The typed `Service` output calls the same `serve` function and returns `60` | Explicit output selection avoids a second service entry. |
| Native executable | `jet build run.jet`; run `build/run` | None | Native executable prints `60` | The source remains the same file. |
| Library projection | Add `core: .Library{…}` to the package outputs, then run `jet build --lib run.jet` | Adds only the final package output record | The native Library emits the static/shared, header, `.jetlib`, and C binding artifacts | The source and existing executable/service outputs are unchanged. |
| Embeddable Library | `jet build --lib run.jet` | None | `target/libpulse.a`, `target/libpulse.so`/`.dylib`/`.dll`, `target/pulse.h`, `target/pulse.jetlib`, and C binding are emitted; the C host calls `seconds(1)` and prints `60` | The test uses the canonical native guest path and the generated header. |
| Structural transition | `jet split env`, run, then `jet fold package/env.jet` | Moves the closed environment fact and then restores it | The split run still prints `60`; fold restores exact `package.jet`, source bytes, lock bytes, and removes `package/env.jet` | Stale and ambiguous journal failures are covered by `package_transition_cli_covers_split_fold_init_restore_and_failures` in `tests/jetpack_engine.rs`. |

The dependency is intentionally not imported by `run.jet`. This proves that adding a graph edge does not require a semantic rewrite. The package metadata points all output kinds at the same checked functions and `seconds` type.

The manifest-free source carries the checked `Executable`/`Service` addresses and a default `app` selection in the same file. The direct command and the explicit `api` command both return `60`; no second service source or hidden script mode is introduced. The direct lens reports `L0104` for the reserved `dev` helper until `jet dev` selects it.

## Host matrix

The fixture was smoke-run on the current Linux checkout with the prebuilt binary. The focused test records `elapsed_ms` for every Jet action when run with `--nocapture`, and `expected.out` is the committed stdout golden. Clean-host Linux, macOS, and Windows runs, including their edit/build latency receipts, remain required before this card can claim the cross-host criterion.

The current repository already has the cross-host package-transition and native-library test surfaces. This card does not claim those external hosts were executed here. A disposable Linux run with the prebuilt binary did exercise the direct, package test, typed service, config explain, AOT executable, native Library, C host, split, and fold commands. The first focused test run found a stale lock baseline after those later build actions (`test result: FAILED. 9 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 79.70s`); the baseline now captures immediately before split. A Cargo rerun then reached an unrelated sibling edit in `crates/jet-foundation/src/Syntax/package_files.rs` and stopped on Rust `E0658` before executing the test.

Two fresh-context reviews compared the journey. They confirm the source-growth path and identify the remaining gaps: local-source install still fails closed with `E1272`, split identity and foreign-host proof need stronger receipts, and clean-host coverage is absent. This is review evidence, not owner acceptance.

## Canonical surfaces used

- `jet init` creates package metadata from a script.
- `jet add` and `jet fetch` own local dependency and lockfile growth.
- `jet test`, `jet run`, `jet explain`, `jet build`, `jet split`, and `jet fold` are the user-facing transitions.
- `jet build --lib` emits the native guest boundary. The generated C header is the foreign host contract.
- `jet install` is not used: the current package tool is `jetpack tool install`. Local source installation currently stops at `E1272` until a pinned compatibility output exists.
