# Compiler phase budget checker

`check.mjs` reads a compiler trace (`JET_TRACE_FILE=<path> jet build …`) and
`budget.json` (per-phase ns per source line) and prints, per phase, its
seconds, ns per source line, the budget of the chosen column and the ratio. It
exits 1 when any phase, the work sum or a natural-unit budget is over.

Columns:

- `225K`, `500K`, `1M`: single-thread-equivalent CPU budgets from
  SPEED-500K.md §1 (lines per second at 16 threads).
- `2.5s-wall`, `20ms-wall`: the owner's self-compile targets (#5479) as wall
  ns per source line. Check them with `--clock wall`.

```
node Tools/perf/budget/check.mjs build.trace.jsonl --column 2.5s-wall --clock wall
node Tools/perf/budget/check.mjs build.trace.jsonl --column 1M
node --test Tools/perf/budget/check.test.mjs
```

The rows follow the current pipeline: load and parse, register, comptime, sema
bodies, effects, failure inference, other finalize, `lower`, `ir.build`,
`ir.opt`, `ir.x64`, `ir.object` and link. Driver time between phases is the
residual P15 row.

What the native `jet` writes for the checker (Compiler/JetCli/Source/Cli/Request.jet):

- every span `E` event: `cpu_ns` (the span thread's CPU time,
  `/proc/thread-self/schedstat`), `proc_cpu_ns` (the whole process's, every
  thread, `/proc/self/stat`, 10 ms ticks) and `majflt`;
- work counters (`C` events, summed per name), such as `parse.{user_,core_,}lines`,
  `parse.{user_,core_,}tokens` and `lower.functions`;
- census spans: `load.marker_groups.relex` / `.reparse` and `comptime.build`
  (detail `program`: one IR build of a comptime batch).

The default clock is the process CPU time when the trace carries it, else wall
time. The per-line denominator is the trace's `parse.user_lines`, else
`default_lines` (448,165, the self-compile unit).
