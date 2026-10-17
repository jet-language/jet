# SPEED-500K budget checker

`check.mjs` reads a compiler trace (`JET_TRACE_FILE=<path> jet build …`) and
`budget.json` (the per-phase ns/line columns 225K / 500K / 1M of
SPEED-500K.md §1) and prints, per phase, its seconds, ns per source line, the
budget of the chosen milestone column and the ratio. It exits 1 when any phase,
the work sum or a natural-unit budget is over.

```
node Tools/perf/budget/check.mjs stage3.trace.jsonl --column 225K
node Tools/perf/budget/check.mjs stage3.trace.jsonl --column 1M --clock wall --lines 448165
node --test Tools/perf/budget/check.test.mjs
```

What the native `jet` writes for the checker (Compiler/JetCli/Source/Cli/Request.jet):

- every span `E` event: `cpu_ns` (the span thread's CPU time,
  `/proc/thread-self/schedstat`), `proc_cpu_ns` (the whole process's, every
  thread, `/proc/self/stat`, 10 ms ticks) and `majflt`;
- work counters (`C` events, summed per name): `parse.{user_,core_,}lines`,
  `parse.{user_,core_,}tokens`, `lower.functions`, `{lower,assemble,optimize}.mir_insts`,
  `native.lower.functions`, `native.lower.lir_insts`,
  `native.object.bytes`, `native.link.bytes`, `comptime.functions_lowered`,
  `lower.comptime_overlap_functions`;
- the spans the plan had no measurement for: `load.marker_groups.relex` /
  `.reparse` (D10b), `comptime.lower` (detail `program` or `grow`, D12) with
  the overlap counter, `mir.digest` (detail: the consumer, `web.semantic`; D5;
  traces written before the D5 cutover also carry `optimize.seal_check` and
  `optimize.seal`), and `optimize.check`, the one MIR checker (P9; traces
  written before the D3 cutover carry `optimize.legality` and `optimize.lint`
  instead). The one LIR check (P11, D6) runs per function inside
  `native.object` and has no span of its own: its allowance is in the
  P11+P12+P13 row (traces written before the D6 cutover carry
  `native.lir_lint` spans, which that row and P10 then include).

The default clock is the process CPU time when the trace carries it, else wall
time (traces written before these spans and clocks; their P9 and D5 time is
still inside P8, which then shows `absent` rows). The per-line
denominator is the trace's `parse.user_lines`, else `default_lines` (448,165,
the self-compile unit).
