# Binary size budgets

Jet's size profile is selected with `jet build --small`. This page is for
release engineers and package authors checking the final executable against a
budget. The executable gate is
[`tests/release_gates.rs`](../../../tests/release_gates.rs); profile arguments
are defined in [`Source/main.rs`](../../../Source/main.rs).

## What the size profile measures

The gate measures the final non-empty executable produced by the
`jet build --small` command, not a Rust debug artifact.
`BuildProfile::Small` uses `opt-level=z`, `panic=abort`, and symbol
stripping; native non-FFI builds also use fat LTO. These are profile inputs,
not permission for a workload to omit required runtime behavior.

Each cap is a limit, not a target. The release-gate rows use the default Linux
x86-64 target and compare the produced file size in bytes:

| Workload | Example | CI cap |
|---|---|---:|
| Hello | `Examples/features/basics/hello.jet` | 512,000 |
| CLI | `Examples/features/io/cli.jet` | 524,288 |
| Small HTTP service | `Examples/features/net/http_server_tasks.jet` | 3,145,728 |
| Library | `Examples/features/modules/library.jet` | 4,194,304 |
| Low-level | `Examples/features/lowlevel/lowlevel.jet` | 4,194,304 |
| Freestanding | `Examples/features/lowlevel/freestanding.jet` | 4,194,304 |

The test also exercises `Examples/features/collections/wordcount.jet` as a
small-profile comparison: its `--small` artifact must be smaller than the
default artifact. That comparison is an invariant, not a fixed byte budget.

## Reproduce a size check

Build the same source in a clean directory with the same target and inspect the
resulting executable size:

```sh
jet build --small Examples/features/basics/hello.jet
```

The gate compares the file size with the cap and requires a positive result.
A toolchain, runtime, reachable-Core, linker, or profile change can alter the
measurement; record the exact source and profile when investigating a breach.

## Size levers and boundaries

- Use `--small` rather than hand-editing generated compiler arguments.
- Keep dead-code elimination effective by emitting and linking only reachable
  Core code.
- Keep package and runtime inputs reproducible so a size difference is
  attributable to a declared change.
- Do not trade away diagnostics, determinism, memory safety, or I9 tier meaning
  to satisfy a byte cap.

The release-gate source is authoritative for the rows and numeric limits. This
page explains how to invoke the profile without turning an observed size into a
status or performance claim.
