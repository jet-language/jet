# Compiler scaling gate (Tower #4319, #4601)

Dependency-free Node tools. The gate compiles every corpus shape at a size
ladder with every compiler and fails when any phase or work counter grows
faster than k * N log N, naming the phase and the counter that dominates it,
or when a phase is slower than its lines/s floor.

```sh
node Tools/perf/scaling/generate.mjs --list                 # every shape
node Tools/perf/scaling/generate.mjs web 125000 NEW_DIR     # one corpus
node --test Tools/perf/scaling/scaling.test.mjs
Tools/agent/jet-env node Tools/perf/scaling/run-gate.mjs --compiler rust=jet:target/release/jet \
  --compiler p15=bootstrap:$HOME/.cache/jet-dev/candP15/jetc0 --memory-max 24G \
  --timeout 120 --stop-after 60 --profile --work-dir $HOME/.cache/jet-dev/scratch/ScalingGate2
node Tools/perf/scaling/report.mjs --runs-dir RUNS_DIR > report.md # merge all shape receipts
```

Every corpus is measured in two layouts: `files` (as generated, many files)
and `single` (one main.jet). A compiler that fails the multi-file layout fails
the gate. Run `jet` compilers through `Tools/agent/jet-env` (rustc on PATH);
the proofq jobs are `~/.cache/jet-dev/proofq/queue/PerfScalingGate-<kind>-<shape>.sh`.
The single `run-gate.mjs` command above is the landing proof: it covers every
stress and domain shape, both layouts, sequentially, and writes per-shape and
merged reports. For proofq use one `--shape NAME` per job with the same work
directory; at 120 s per compile, at most 24 compiles including profiles fit
within the runner's one-hour deadline. A timeout fails with its open phase.
Every baseline, rung and profile compile is atomically checkpointed. Repeat
the identical command to resume without retrying completed rungs (including
failures); changed options or compilers require a new work directory.
To queue bounded jobs, run `enqueue.mjs` with the same options and
`JET_SCALING_QUEUE_STAMP` naming the replaced job whose mtime must be retained.
It writes one `PerfScalingGate-<kind>-<shape>.sh` per shape; all hold the same
`scaling-gate.lock`, so only one scaling compile runs at a time.

Shapes (`corpora.mjs`, `domains.mjs`, helpers in `package.mjs`): stress shapes
grow one dimension with N (files, small functions, statements per function,
nesting depth, enum width and arm tables, impls/traits, generic
instantiations, comptime items, table elements, Core imports, call depth,
fallible functions, diagnostics, interpolation holes, call sites) at N, 2N, 4N,
8N; domain shapes are realistic programs (web, game, cli, data, gui,
embedded, compiler, scientific, generic-lib, generated) at 125k/250k/500k/1M
code lines. Adding a shape is one builder function plus a registry row; see
the header comments of `corpora.mjs` and `domains.mjs`. `mixed` (`mixed.mjs`)
remains the compile-throughput cell's pinned program.

Per rung the gate records wall/user/RSS (GNU time), per-phase wall from the
`JET_TRACE_FILE` phase spans, the trace's span and item counts
(`trace.<kind>.*`), and `JET_PHASE_COUNTERS` lines (`counter.<phase>.<name>`):
the Jet half emits `sink.progress("counters.<phase>.<name>", n, -1)`, the
Rust half counts through `jet_foundation::WorkCounters` (tokens, items,
scope lookups, item/impl scans, whole-program copies, functions lowered and
emitted, allocations), drained at every phase end by `jet_driver::Trace`.
The Jet half counts module item scans, ordinary and return-view-summary body
checks, method jobs and impl-row scans in sema; function rows in lowering;
global-instruction scans, impls, top-level functions and output lines in emit.
Counters are deltas per loop invocation, so repeated module passes sum rather
than overwrite one another. They add no traversal or per-item sink calls.
Series are baseline-subtracted (an N=1 rung), judged between the first and
last measurable rung against N log N (bounds: counters 1.25, time/RSS 1.5),
and projected to 1M lines. Floors come from
`Tools/perf/throughput/bench.json` phase budgets plus 1M lines in 10 s end
to end. `--profile` re-runs the top rung of a failing series under perf and
attributes samples per phase to source functions (`profile.mjs`). Exit 0
pass, 1 failure, 2 invalid invocation; the receipt is JSON in `--work-dir`.
