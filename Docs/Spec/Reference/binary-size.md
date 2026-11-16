# Binary size budgets

Jet's size profile is selected with `jet build --small`. This page is for
release engineers and package authors checking the final executable against a
budget. The size gate was a Rust integration test and is retired with the
Rust suite; `--small` is the `small` build option of
[`BuildOptions.jet`](../../../Compiler/JetFoundation/Source/Registry/BuildOptions.jet).

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
| Hello | `Examples/Basics/Hello.jet` | 512,000 |
| CLI | `Examples/Systems/CLI/CLIArgs.jet` | 524,288 |
| Small HTTP service | `Examples/Network/HTTP/HTTPServer.jet` | 3,145,728 |
| Library | `Examples/Modules/Library.jet` | 4,194,304 |
| Low-level | `Examples/LowLevel/LowLevel.jet` | 4,194,304 |
| Freestanding | `Examples/LowLevel/Freestanding.jet` | 4,194,304 |

The test also exercises `Examples/Collections/WordCount.jet` as a
small-profile comparison: its `--small` artifact must be smaller than the
default artifact. That comparison is an invariant, not a fixed byte budget.

## Reproduce a size check

Build the same source in a clean directory with the same target and inspect the
resulting executable size:

```sh
jet build --small Examples/Basics/Hello.jet
```

The gate compares the file size with the cap and requires a positive result.
A toolchain, runtime, reachable-Core, linker, or profile change can alter the
measurement; record the exact source and profile when investigating a breach.

## Hello size and start gate

`jet Tools/Perf/Budget/Check/run.jet --hello` is the local perf gate for the
size floor. It builds `Examples/Basics/Hello.jet` with `jet build --small`,
records the executable's size, and runs it 50 times to get the median start
time, measured from spawn to exit. Run it on the reference Linux x86-64
machine. It fails when either measure is over its limit in this table. The
gate reads its limits from this table, and nothing else holds a copy:

| Measure | Limit |
|---|---:|
| Hello binary size | 200 KB |
| Hello start time | 5 ms |

A KB is 1,024 bytes. Each run prints a `card row:` line with the size and the
median start time, for the binary-size card's log. `--jet PATH` picks the jet
that builds hello. `--runs N` changes how many runs the median covers.

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
