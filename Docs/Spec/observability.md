# Jet observability, debugging, and profiling

This document states the durable contracts for Jet's first-party profiler
(`jet perf`), debugger (`jet debug`), flight recorder, devtools protocol, and
build diagnostics. Exact wire schemas and command flags live in
`Compiler/JetCLI/`, `Compiler/JetRuntime/`, and `Tools/`. Use the
[Jet vocabulary](vocabulary.md) for domain terms. (Full historical prose lives
in git history: `git show 20be9d938a5:Docs/Spec/observability.md`.)

## Wall-clock traces, instructions, and capture comparison

- **First-party profiling:** `jet perf` profiles any Jet program (including the
  compiler) without external tools, exporting deterministic wall-clock traces,
  instruction/cycle attribution, and allocation counts.
- **Zero-cost when disabled:** Instrumentation and trace hooks must cost
  effectively nothing when profiling is off; enabled profiling measures real
  execution without altering program semantics (I9).
- **Capture comparison:** `jet perf` compares two captures on the same metric
  axes (wall time, cycles, allocations, RSS) and highlights regressions by
  function/zone without averaging away losses.

## Flight recorder and crash dumps

- **Continuous low-overhead ring:** An optional in-process flight recorder
  retains recent structured spans, events, and sync-order entries in bounded
  memory so faults and hangs produce actionable post-mortem evidence.
- **Crash dumps:** Runtime faults (`exit 70`) emit a structured, deterministic
  diagnostic frame (stack, active spans, last recorder window) through the
  shared Prelude report boundary without leaking unrelated host memory.

## Canonical devtools protocol and hosts

- **One protocol:** CLI, IDE, Web dashboard, and Canvas hosts speak one
  versioned devtools protocol (`jet.canvas.profile` and companion trace/debug
  messages) with four canonical timeline lanes: `spans`, `zones`, `waits`, `io`.
- **Server, query, and mutation observations:** Server functions, queries, and
  mutations emit structured start/end/outcome/latency records through the same
  observation stream—no per-host ad-hoc trace format.

## Debugger console, recording, and replay

- **Live, attach, and replay:** `jet debug` supports live launch, `--attach`,
  and `--record`ed deterministic replay of native Jet binaries (including the
  compiler itself).
- **Native evaluation:** Expression evaluation inside `jet debug` uses Jet's
  own checked semantics and debug metadata rather than a separate interpreter
  with divergent rules.
- **Recorder boundary:** Recording captures non-deterministic boundary inputs
  (clock, OS I/O, signals, thread sync order) at the runtime boundary so
  replay reproduces the exact execution path and failure state.
- **Sync-order log:** Concurrency debugging records synchronization ordering
  at channel/lock/task boundaries so race-free ordering bugs and deadlocks
  replay deterministically.

## Debugging the build

- Compiler and build-script phases emit the same structured spans and phase
  timings (`jet build`, `jet check`, `jet perf`) so compile-time and build-time
  bottlenecks are diagnosed with the exact same tooling as user programs.
