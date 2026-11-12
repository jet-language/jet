# Compiler phase budget checker

`check/run.jet` reads a compiler trace (`jet build --trace=<path> …`) and
`budget.json` (per-phase ns per source line) and prints, per phase, its
seconds, ns per source line, the budget of the chosen column and the ratio,
then the subtotal of the always-on check rows. It exits 1 when any phase, the
work sum or a natural-unit budget is over, or when a full-unit trace (it has a
`lower` span) lacks an always-on span.

Columns:

- `225K`, `500K`, `1M`: single-thread-equivalent CPU budgets from
  SPEED-500K.md §1 (lines per second at 16 threads).
- `2.5s-wall`, `20ms-wall`: the owner's self-compile targets (#5479) as wall
  ns per source line. Check them with `--clock wall`.
- `30M`, `30M-wall`: the #5479 target, 30 million lines per second from cold:
  533.3 CPU ns per line at 16 threads, and 33.3 wall ns per line, of which
  30.0 go to the traced phases and 3.3 to `outside_trace`, the build steps
  around the traced compile (the package's source generators and the runtime
  pack step). Each splits as the `1M` and `20ms-wall` columns do. The
  compile-throughput cell checks them per corpus line, `outside_trace`
  included:
  `JET=PATH jet Tools/perf/compile-throughput/Throughput.jet --budget 30M-wall`.

```
jet Tools/perf/budget/check build.trace.jsonl --column 2.5s-wall --clock wall
jet Tools/perf/budget/check build.trace.jsonl --column 1M
jet test Tools/perf/budget/check/run.jet
```

Run it from the repository root: the default `--budget` is
`Tools/perf/budget/budget.json` relative to the working directory. Its
JavaScript number text and JSON reads come from the Tools/perf/kit package.

The rows follow the current pipeline: load and parse, register, comptime, sema
bodies, effects, failure inference, other finalize, `lower`, `ir.build`,
`ir.opt` with `ir.lint`, `ir.x64`, `ir.object` and link. Driver time between
phases is the residual P15 row.

Rows with `always_on` carry the checks that run once in every default build
(D-SPEED-CHECKS1, Docs/spec/reference/compiler-speed.md): ownership in
P5a+P5b, effects in P5c, failure inference in P5d, the IR consistency check
(`ir.lint`) in P9 and DWARF debug info in P11. Its `spans` are the spans a
full-unit trace must carry.

What the native `jet` writes for the checker (Compiler/JetCli/Source/Cli/Request.jet):

- every span `E` event: `cpu_ns` (the span task's CPU time, the per-fiber
  clock `core.tasks.cpu_ns`), `proc_cpu_ns` (the whole process's,
  every thread, `/proc/self/stat`, 10 ms ticks) and `majflt`; an event's
  `tid` is its task's lane (1 for the task that opened the trace, then
  numbered in first-trace order), so a task's spans nest on one lane
  whichever carrier threads ran it;
- work counters (`C` events, summed per name), such as `parse.{user_,core_,}lines`,
  `parse.{user_,core_,}tokens` and `lower.functions`;
- census spans: `load.marker_groups.relex` / `.reparse` and `comptime.build`
  (detail `program`: one IR build of a comptime batch).

The default clock is the process CPU time when the trace carries it, else wall
time. The per-line denominator is the trace's `parse.user_lines`, else
`default_lines` (448,165, the self-compile unit).
