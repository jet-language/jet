# c121 — Performance and compile-time dashboards
**Decision:** none required — tooling only, no new language surface.
**Gate:** none.

---

## Goal

Track compiler phase latency, generated Rust size, final binary size, runtime benchmarks,
LSP latency, package resolution time, and `jet dev` hot-loop latency. All measurement is
std-only (I6 — no external crates in `Source/`; measurement tooling lives outside `Source/`
in `tools/`).

---

## Metrics to track

| Category | Metric | How measured |
|----------|--------|--------------|
| Compile phases | Lex / parse / sema / TIR lower / emit (ms) | wall time around each phase in `Source/lib.rs` |
| Codegen output | Generated Rust size (bytes, lines) | `len()` on the emitted string |
| Final binary | Binary size after `rustc` link (bytes) | `fs::metadata` on the output binary |
| Runtime | Benchmark case throughput (ops/s) | `jet bench` driver (`CmdDevTools.rs`) |
| LSP | Request latency (ms) | measured inside `Source/LSP/mod.rs` |
| Package resolution | `jet fetch` wall time (ms) | timer in `Source/Fetch.rs` |
| Dev loop | `jet dev` hot-loop iteration time (ms) | timer in `Source/Interpreter.rs` / `CmdDevTools.rs` |

---

## Plan

### Step 1 — Phase timer infrastructure (`Source/PhaseTiming.rs`, new)

A zero-dependency struct that wraps `std::time::Instant`:

```rust
pub struct PhaseTimer {
    phases: Vec<(String, u128)>,   // (name, duration_ms)
    start: std::time::Instant,
}

impl PhaseTimer {
    pub fn new() -> Self { … }
    pub fn lap(&mut self, phase: &str) { … }
    pub fn report(&self) -> Vec<(String, u128)> { self.phases.clone() }
    pub fn to_json(&self) -> String { … }   // hand-rolled, no serde dep (I6)
}
```

`to_json` emits `{"phases":[{"name":"lex","ms":12},…]}` — hand-rolled string building, no
external crate.

### Step 2 — Instrument `Source/lib.rs`

Wrap each pipeline phase in `compile_file` / `compile_bundle`:

```rust
let mut t = PhaseTimer::new();
// lex
let tokens = Lexer::lex(src)?;
t.lap("lex");
// parse
let ast = Parser::parse(tokens)?;
t.lap("parse");
// sema
Sema::check_bundle(&mut bundle, mode)?;
t.lap("sema");
// TIR + emit
let rust_src = Codegen::emit(&bundle)?;
t.lap("codegen");
t.lap("emit_size_bytes"); // record rust_src.len() as a "phase"
```

Emit timing only when `JET_TIMING=1` env var is set (off by default; zero overhead in
normal builds). Write JSON to `jet-timing.json` in the project root.

### Step 3 — Binary size measurement (`Source/CmdCompile.rs`)

After `rustc` completes (existing subprocess call), stat the output binary:

```rust
if let Ok(meta) = std::fs::metadata(&output_path) {
    if std::env::var("JET_TIMING").is_ok() {
        eprintln!("binary size: {} bytes", meta.len());
        // append to jet-timing.json
    }
}
```

### Step 4 — in-source benchmark regions (`jet bench` already exists)

**Correction:** `jet bench` is **not a stub** — `run_bench` (`Source/CmdDevTools.rs:447`,
labeled "D-TEST1 / D-TOOL5") already builds the program (`build/bench_<stem>`), runs 5 warmups
+ 20 timed trials of the **whole program**, and reports `mean ms ±stddev (N runs, M warmup)`
(text and `--json`). What is *missing* is a way to benchmark a **named code region in source**
(e.g. one hot loop) the way `#Test` names a unit, instead of timing the whole binary. That
region surface is the open question:

- If D-BENCH1 lands a `#Bench "name" { … }` block: add `Item::Bench(BenchDef)` to `AST.rs`, a
  `bench_def()` parser in `Parser/Items.rs` (sibling of `test_def`), codegen that wraps each
  bench body in a warmup + timed `for _ in 0..N` loop measured with `std::time::Instant`, and
  extend `run_bench` to discover/run them and report ns/iter per region.
- Output for regions: `<name>  <ns/iter> ns/iter  (<N> iters)` to stdout.

**NEEDS BALLOT: D-BENCH1** — the in-source benchmark-region surface. Options below. (The
whole-program `jet bench` already works regardless of the outcome.)

### Step 5 — LSP latency measurement (`Source/LSP/mod.rs`)

Wrap each LSP request handler (hover, completion, goto-definition) with a timer. Emit
`{"method":"hover","ms":4}` to a `jet-lsp-timing.json` in the project root when
`JET_TIMING=1`.

### Step 6 — Dashboard report tool (`tools/perf/dashboard.sh`)

A shell script (no Rust, no external tools beyond standard POSIX):

```sh
#!/usr/bin/env sh
# Usage: tools/perf/dashboard.sh [--baseline] [--compare baseline.json]
# Runs jet build on all examples, collects jet-timing.json files,
# prints a text table, and optionally compares against a baseline.
```

Runs `nix develop -c jet build examples/features/01_hello.jet` (and a set of larger
representative programs) and aggregates `jet-timing.json` output. Prints a fixed-width table:

```
phase        current  baseline   delta
lex              12ms      11ms   +1ms
parse            34ms      32ms   +2ms
sema            180ms     175ms   +5ms
codegen          22ms      21ms   +1ms
binary size    420KB     418KB   +2KB
```

Regression threshold: any phase > 10% over baseline prints `REGRESSION` in red (using ANSI
codes from a simple shell helper — no external tool).

### Step 7 — CI integration (`tools/perf/ci-perf-check.sh`)

Called from CI after tests pass. Compares current timings against a stored baseline
(`tools/perf/baseline.json`, committed). If any regression is detected, exits nonzero and
prints the table. Owner manually updates the baseline after intentional performance changes
with `tools/perf/update-baseline.sh`.

---

## Files touched

| File | Change |
|------|--------|
| `Source/PhaseTiming.rs` (new) | `PhaseTimer` struct |
| `Source/lib.rs` | Phase instrumentation; `JET_TIMING` env gate |
| `Source/CmdCompile.rs` | Binary size stat |
| `Source/CmdDevTools.rs` | extend existing `run_bench` (`:447`) to discover/run in-source bench regions (if D-BENCH1 ratified) |
| `Source/AST.rs` | `Item::Bench(BenchDef)` (if D-BENCH1 ratified) |
| `Source/Parser/Items.rs` | `bench_def()` parser, sibling of `test_def` (if D-BENCH1 ratified) |
| `Source/LSP/mod.rs` | Request handler timing |
| `tools/perf/dashboard.sh` (new) | Aggregate dashboard |
| `tools/perf/ci-perf-check.sh` (new) | CI regression check |
| `tools/perf/baseline.json` (new) | Committed baseline |

---

## Decision verdict

**NEEDS BALLOT: D-BENCH1** — the user-visible surface for benchmarking a **named code region
in source** (the whole-program `jet bench`/`run_bench` already exists). Options:
1. `#Bench "name" { … }` (sibling of `#Test "name" { … }`).
2. No new block syntax; a `core.time` stopwatch / `bench(name, fn)` timing API.
3. `#Test(bench: true) fn name()` — reuse the `#Test` fn form with a named argument.

The owner must pick before in-source bench regions are implemented. Everything else in this
plan — including the existing whole-program `jet bench` — is unblocked.
