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

### Step 4 — `jet bench` driver (already stubbed in `Source/CmdDevTools.rs`, line 444)

Implement the stub:

- Parse `#Bench` blocks (same grammar as `#Test` but named `Bench`; add `Item::Bench` to
  AST).
- Codegen wraps each bench body in a `for _ in 0..N { … }` loop; measure with
  `std::time::Instant`; report `ns/op`.
- `jet bench` CLI verb dispatches to `cmd_bench` in `CmdDevTools.rs`.
- Output format: `bench <name>  <ns/op> ns/op  (<N> iters)` to stdout.

**NEEDS BALLOT:** `#Bench` block syntax. Current proposal is identical to `#Test` but named
`Bench`. This is a user-visible syntax choice; the owner must ratify it before implementation.
The fallback (no new block syntax; bench is just a `#Test` with a timing API) is an
alternative. File as **D-BENCH1**.

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
| `Source/CmdDevTools.rs` | `jet bench` dispatch; `#Bench` item |
| `Source/AST.rs` | `Item::Bench(BenchDef)` (if D-BENCH1 ratified) |
| `Source/Parser/Modules.rs` | `bench_def()` parser (if D-BENCH1 ratified) |
| `Source/LSP/mod.rs` | Request handler timing |
| `tools/perf/dashboard.sh` (new) | Aggregate dashboard |
| `tools/perf/ci-perf-check.sh` (new) | CI regression check |
| `tools/perf/baseline.json` (new) | Committed baseline |

---

## Decision verdict

**NEEDS BALLOT: D-BENCH1** — what is the user-visible syntax for benchmark blocks? Options:
1. `#Bench "name" { … }` (mirrors `#Test "name" { … }`, D-TEST1 pattern).
2. No new block syntax; benchmarks are plain fns called by a `jet bench` runner that measures
   wall time.
3. `#Test(bench: true) fn name()` — reuse the `#Test` fn form with a named argument.

The owner must pick before Step 4 is implemented. Everything else in this plan is unblocked.
