# Binary size budgets

Jet's size profile is selected with `jet build --small`. This page is for
release engineers and package authors checking the final executable against a
budget. `--small` is the `small` build option of
[`BuildOptions.jet`](../../../Compiler/JetFoundation/Source/Registry/BuildOptions.jet):
it keeps the optimizer's inlining to the O1 limits at any level, so inlining
grows the code no more than `optimize: O1` does. It is a profile input, not
permission for a workload to omit required runtime behavior.

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

This page explains how to invoke the profile without turning an observed size
into a status or performance claim.
