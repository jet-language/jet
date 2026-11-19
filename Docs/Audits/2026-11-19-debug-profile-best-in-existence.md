# Best debugger and profiler in existence: adversarial review, revision 3 (2026-11-19)

Card: #6841 c39. Base: dev `82821a4de9`. Follows
[`2026-11-18-best-debugger-profiler.md`](2026-11-18-best-debugger-profiler.md)
(revision 2: the designs, levers and ranked plan R1-R16, now #6841 c43-c58),
[`2026-11-17-debug-latency-vs-lldb.md`](2026-11-17-debug-latency-vs-lldb.md)
(the D1 measurement) and
[`2026-11-14-debugging-profiling-review.md`](2026-11-14-debugging-profiling-review.md)
(the c1 review). This revision does not repeat their designs. It asks one
question per axis: on the code that is built, does `jet` alone match or beat
the best existing tool there? It answers from gate runs, not prose, and gives
every gap a one-line criterion.

The owner's order (2026-11-18): Jet ships the best debugging and profiling tool
in existence, and needs nothing but `jet`.

Terms: "event" (one typed occurrence in a trace, a flight ring or a recording)
is used as [the vocabulary page](../Spec/vocabulary.md) defines it.

## Method

The AGENTS.md adversarial design review rule, once per axis:

1. **Peer.** The strongest existing tools on the axis, their best case stated
   fairly with a source, and what they do that Jet does not. Revision 3 adds
   three peers the earlier revisions under-used: Chrome DevTools, Linux
   `perf`, and lldb and gdb.
2. **Jet today.** What the built code does, from a named gate's case rows.
3. **Attack.** Hidden costs, an easier workload, a narrower scope, a deferred
   hard case.
4. **Verdict.** `holds`, `holds only within stated limits`, or `fails`, for the
   claim "jet alone matches or beats the peer here today". A row that has not
   run on a gate is not a pass: its axis is `fails`, marked PENDING with the
   SUB that would change it.
5. **Gap.** Each gap is one line naming the criterion that closes it: an open
   #6841 criterion, or a proposed one (G-numbers, dispositions at the end).

### Evidence

Two gates ran the case suite (`jet test Compiler --profile cases`) with every
row reported:

- **Gate p299**: the cycle before the evaluator landed.
- **Gate p300**: cycle-p300 built base `d2c1886238` plus 15 SUBs
  (commit `cb2ec4938d`). In the suite, 2993 ui rows passed and 38 failed, and
  5074 golden rows passed and 23 failed. Over the 302 case rows in `Debug/`,
  `DebugEval/`, `Perf/`, `PerfRun/`, `Concurrency/`, `LiveInspect/`,
  `JetPerfTraceParts*/`, `Observe/` and `BuildEntry/`, 239 passed, 39
  failed and 24 are known-failing.

A row is cited as `Debug/name` (the case
`Tests/Cases/Debug/name.case`) with its gate and result. Each row reruns
with `jet test Compiler --profile cases --filter Debug/name`.

**One regression dominates p300's Debug/ failures.** The one-evaluator change
(c45 v1, `0ccf16e14e`) broke 15 Debug/ rows that passed on p299. It turned a
plain `print x` into a compiled function that drops uncalled code, returns
borrowed values and spells types that have no parameter spelling. SUB
`Lens-6841-c45v1-fix` fixes all four causes and is not yet built. Until it is,
the axes it touches (D3, D4, D5, D6, D10) read `fails (PENDING
Lens-6841-c45v1-fix)`.

## Debugger axes

### D1. Start and step

**Peer.** RemedyBG and the RAD Debugger: a stop on a large game in well under
a second, and steps with no visible latency
([remedybg.itch.io](https://remedybg.itch.io/remedybg),
[github.com/EpicGamesExt/raddebugger](https://github.com/EpicGamesExt/raddebugger)).
lldb: a `next` under 1 ms on a 120,004-line image, after loading the image's
DWARF up front (measured in `2026-11-17-debug-latency-vs-lldb.md`).

**Jet today.** The first stop beats lldb. Per step it loses badly.
`Tools/Perf/DebugLatency/Main.jet`, as
`2026-11-17-debug-latency-vs-lldb.md` measured it:

| Image | jet first stop | lldb first stop | jet per step | lldb per step |
|---|---|---|---|---|
| 12,004 lines | 7 ms | 150 ms | 76 ms | under 1 ms |
| 120,004 lines | 55 ms | 206 ms | 1,339 ms | under 1 ms |

Per step grows with the image. `a39f5a5c5e` (a step writes its traps in one
pass) landed after that measurement, and no gate has measured since. On p300,
`Debug/next_over_a_loop_is_one_stop` and
`Debug/program_arguments_reach_the_program` PASS.

**Attack.** Both first-stop figures are on a prebuilt image. `jet debug
prog.jet` builds first, and the 120,004-line `-O0` build took 31.5 s (revision
2, F18). lldb's per-step figure rounds to 0 in whole milliseconds, so "at most
lldb's + 1 ms" is the only honest bar.

**Verdict.** Fails (per step: 1,339 ms against under 1 ms). The first stop
holds against lldb on both images. PENDING: c59 needs a rerun of the
DebugLatency tool on a gate that has `a39f5a5c5e`.

**Gap.** G1: per step grows with the image. Closed by c59 (open).

### D2. Record, replay, reverse, "why"

**Peer.** rr records at low overhead and replays deterministically with
reverse execution; it runs all threads on one core while recording and needs
a hardware counter
([USENIX ATC 2017](https://www.usenix.org/system/files/conference/atc17/atc17-o_callahan.pdf)).
Pernosco answers "where did this value come from" in one click
([pernos.co](https://www.pernos.co/about/dataflow)). WinDbg TTD traces any
Windows process at instruction granularity
([learn.microsoft.com](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/time-travel-debugging-overview)).
Replay.io adds print statements to a recorded browser session
([docs.replay.io](https://docs.replay.io)). UDB records multi-threaded Linux
programs ([undo.io](https://undo.io)).

**Jet today.** Nothing built on dev. No gate has run
`Debug/replay_threads_across_tiers_*`, `Debug/replay_reverse_*` or
`Debug/replay_why_*`.

**Attack.** As revision 2 (D2): unsafe and FFI writes, relaxed atomics,
address-dependent behavior and tier-dependent stack depth all limit
determinism, and only the design's divergence check makes them visible.

**Verdict.** Fails. PENDING: `IfHole-6-1-c43-a1-record.r3`, `-a2-replay.r2`,
`-b-reverse.r2` and `-c-why.r2`.

**Gap.** G2: record, replay, reverse and `why`. Closed by c43 (open).

### D3. Jet-typed values at O2, inlined frames included

**Peer.** Visual Studio and the RAD Debugger show optimized C++ well when the
PDB is good. lldb and gdb often show `<optimized out>`. No peer shows a value
the optimizer deleted.

**Jet today.** p299: `Debug/type_facts_display_every_kind`,
`Debug/locals_render_big_ints_strings_lists_structs_and_decimals` and
`Debug/native_shows_o2_inlined_frames_and_their_locals` PASS. p300: the O2
inlined-frames row PASS, but the two `print` rows FAIL (the c45 v1
regression).

**Attack.** A value optimized out at `-O2` stays out without a recording. The
recovery from an `-O0` replay is D2's design, which is not built.

**Verdict.** Fails on p300 (PENDING `Lens-6841-c45v1-fix`). On p299 it holds
only within stated limits: values the location lists keep, like every peer.

**Gap.** G3: show a deleted `-O2` value from the recording. Closed by c43(c)
(open).

### D4. Expression evaluation

**Peer.** lldb compiles expressions with Clang and runs them in the process. It
handles templates and method calls, but it can change or crash the debuggee
([lldb.llvm.org](https://lldb.llvm.org/use/tutorial.html)). gdb's `compile
code` injects compiled C
([sourceware.org/gdb](https://sourceware.org/gdb/current/onlinedocs/gdb.html/Compiling-and-Injecting-Code.html)).
Visual Studio refuses side effects only at run time, with `, nse`. Chrome
DevTools evaluates any JavaScript in the paused frame, with "eager evaluation"
previews that throw on side effects
([developer.chrome.com](https://developer.chrome.com/docs/devtools/console/reference)).

**Jet today.** One evaluator, the compiler (c45 v1, `0ccf16e14e`). On p300:

- `Debug/print_evaluates_compiled_expressions` and
  `Debug/print_refuses_effectful_expression` FAIL;
- `Debug/dap_cli_conditions_logpoints_and_evaluate` and
  `Debug/dap_cli_selects_a_caller_frame` FAIL.

The causes:

1. an uncalled expression function is pruned;
2. a plain name is returned as a borrow;
3. the failure solve's deferred obligation is taken as a refusal;
4. a stop on a binding line has no statement at its name.

`Lens-6841-c45v1-fix` fixes all four. With the p300 compiler, `jet check`
accepted the generated functions it makes, and refused a callee that can
fail.

**Attack.**

- Every compound expression pays a compile of the program with the function
  appended, then a fork.
- Revision 2 measured a fork at 1.59-3.00 ms at 64 MiB resident and up to
  400 ms at 2 GiB, and the first check in a large package at 9.95 s for
  JetCLI.
- lldb's first expression is also slow, but its later ones are not.
- An expression that calls a function that can fail is refused rather than
  shown as `Ok` or `Err`.
- A capture whose type has no parameter spelling (`[Int String!]`) is refused
  inside a compound expression.
- At most 5 captures.
- A lock another thread held at the stop costs the 2 s budget.
- Nothing is evaluated in an inlined frame. Since the fix, a plain local's
  name is read there too.

**Verdict.** Fails (PENDING `Lens-6841-c45v1-fix`; the four Debug rows have not
run with it). After it lands: holds only within stated limits. No peer
proves an expression free of side effects before running it, but latency
loses to lldb until the compile reuses the program's check.

**Gaps.**

- G4: a fallible call evaluates to `Ok(v)` or `Err(e)` instead of a refusal.
  Proposed: "`print parse(s)` of a function that can fail shows `Ok(3)` or
  `Err(…)` as `locals` shows a Result."
- G5: the latency of a second evaluation. Proposed: "a second `print` of a new
  expression in the same stopped function answers in under 100 ms on JetCLI,
  by the DebugLatency tool."
- G6: an expression in an inlined frame. Proposed: "`print n + 1` in an O2
  inlined frame answers as at O0."
- G7: the caller-frame column. Closed by c42 Q1 (open; SUB
  `Lens-6841-c42-q1-call-site-rows.r2`).

### D5. Watch and data breakpoints

**Peer.** gdb and lldb arm the four x86 debug registers
([sourceware.org/gdb](https://sourceware.org/gdb/current/onlinedocs/gdb.html/Set-Watchpoints.html)).
rr adds reverse watchpoints. Chrome DevTools has no data breakpoint on a
JavaScript value; it breaks on DOM changes instead
([developer.chrome.com](https://developer.chrome.com/docs/devtools/javascript/breakpoints)).

**Jet today.** `watch` exists (c49, `d59dcd53a3`). On p300,
`Debug/watch_stops_at_the_changing_write` FAILS, because the watch's value
goes through the regressed evaluator ("Can't watch `total`: the compiled
program has no function for it").

**Attack.** A watch has four slots, as every live debugger does. A local held
in a register is followed by stepping, which is slow. A reallocated list moves
its place.

**Verdict.** Fails (PENDING `Lens-6841-c45v1-fix`). The design is at parity
with gdb live; it beats gdb only in a replay (D2).

**Gaps.**

- G8: a watch follows a reallocated value. Proposed: "`watch xs[3]` keeps
  stopping after `xs` grows past its capacity."
- G9: reverse watches. Closed by c43(b) (open).

### D6. Conditional breakpoints and logpoints

**Peer.** gdb and Visual Studio evaluate a condition at each hit through a
debugger round trip. Live++ and Dyninst patch code. Chrome DevTools logpoints
and conditional breakpoints run in the page's own engine
([developer.chrome.com](https://developer.chrome.com/docs/devtools/javascript/breakpoints)).
Replay.io's logpoints run against the past.

**Jet today.** The console takes `break … if` and `log` (c49, `077790620f`).
On p300, `Debug/console_breakpoints_take_conditions_and_logs` FAILS: the
condition's probe landed before `i` was bound, and the logpoint's function
was pruned (both fixed in `Lens-6841-c45v1-fix`). The in-process placed
condition (R7) is SUB `Lens-6841-c49-r7-placed-conditions`; its row
`Debug/conditional_breakpoint_runs_in_process` has run on no gate.

**Attack.** A condition that is not placed pays a fork per hit, which is more
than gdb's ptrace stop. R7 places no logpoint, and it does not save the AVX
upper halves, MXCSR or the x87 state across the trampoline.

**Verdict.** Fails (PENDING `Lens-6841-c45v1-fix` and
`Lens-6841-c49-r7-placed-conditions`).

**Gap.** G10: placed logpoints. Proposed: "a logpoint whose message R7's scan
places runs in the program, and a million hits finish in under a second."

### D7. Hot patch

**Peer.** Live++ patches running C++ in about a second, keeping state
([liveplusplus.tech](https://liveplusplus.tech)). Chrome DevTools edits a
running script's function and keeps the page's state
([developer.chrome.com](https://developer.chrome.com/docs/devtools/javascript/reference)).

**Jet today.** `fix` landed (c54, `e778edd600`). On p300,
`Debug/fix_swaps_function_in_native_process` FAILS with "Fix refused: `score`
now calls the runtime's `jet_int_add`": the edited function's `-O0` build
calls a runtime entry that the image does not export.

**Attack.** Inlined copies of an old function survive a swap. A function on a
stack keeps its old code until it returns.

**Verdict.** Fails.

**Gap.** G11: `fix` of an edit whose `-O0` build calls a runtime entry the
image lacks. Closed by c54 (open).

### D8. Tasks, channels, locks and deadlocks

**Peer.** Go's runtime reports "all goroutines are asleep - deadlock!" only
when every goroutine is blocked. Java's `ThreadMXBean.findDeadlockedThreads`
finds monitor cycles when asked
([docs.oracle.com](https://docs.oracle.com/en/java/javase/21/docs/api/java.management/java/lang/management/ThreadMXBean.html)).
Visual Studio's Parallel Stacks draws tasks.

**Jet today.** On p300, `Debug/tasks_lists_every_task` and every
`LiveInspect/` row PASS. `Concurrency/wait_cycles_stop_every_task_in_them`
FAILS on both p299 and p300 (the S1 runtime, `f51b7ecacb`, landed). The
channel-cycle half, `Tasks/wait_cycle_stops_with_the_cycle`, is SUB
`IfHole-6-1-c48-s2-channel-cycles` and has run on no gate.

**Verdict.** Fails (PENDING `IfHole-6-1-c48-s2-channel-cycles`).

**Gap.** G12: the deadlock stop. Closed by c48 (open).

### D9. Post-mortem dumps

**Peer.** WinDbg and Visual Studio open minidumps with full types; Crashpad
writes them in production. gdb opens a Linux core with C types.

**Jet today.** Typed dumps and the flight ring landed (c51, `a7d5535c3e`).
`Debug/crash_dump_opens_typed` FAILS on p299 and p300 at
`file: {cap:dump} !~ "hunter2"`: the Secret's text reaches the dump file,
though the console shows `key = <redacted>`. A dump's `print NAME` was also
refused ("a dump runs no code"); `Lens-6841-c45v1-fix` reads a plain name as
`locals` does. SUBs `IfHole-6-1-c51-reachable-budget`
(`Debug/crash_dump_keeps_its_budget`) and `IfHole-6-1-c51-task-stacks` are
pending.

**Verdict.** Fails. A dump that leaks a Secret loses to a minidump writer that
never claimed redaction.

**Gap.** G13: a dump file keeps no Secret's bytes. Closed by c51 (open).

### D10. Visualizers

**Peer.** Visual Studio natvis, gdb Python pretty-printers and lldb data
formatters: a second language kept apart from the type
([learn.microsoft.com](https://learn.microsoft.com/en-us/visualstudio/debugger/create-custom-views-of-native-objects)).

**Jet today.** `debug_view` runs in a fork, never in the program (c50,
`05f0cf3547`). `Debug/debug_view_package_ring_buffer` PASSED on p299. On p300,
it and `Debug/debug_view_renders_without_running_the_target` FAIL at `print`
(the c45 v1 regression).

**Verdict.** Fails (PENDING `Lens-6841-c45v1-fix`). After it: holds only within
stated limits. The visualizer is Jet and is checked by the compiler, a fork
costs D4's fork, and replays wait for D2.

### D11. Surfaces: terminal, DAP, graphical

**Peer.** RemedyBG and the RAD Debugger (native GUI, every panel at once).
VS Code over DAP, which attaches, pauses, restarts and steps back. Chrome
DevTools' Sources, Console and Performance panels in one window.

**Jet today.** On p300:

- PASS: the Canvas `native-debug-panels` mode (c58, `e94e79f709`), and
  `Debug/dap_cli_launches_and_projects_a_live_target_in_jet_terms` and
  `Debug/dap_cli_steps_over_and_into_with_typed_variables`.
- Known-failing, as "#6730 unbuilt": DAP `attach`, `pause`, `restart` and
  `showRawFrames` (`Debug/dap_cli_attaches_and_disconnects_through_the_production_wire`,
  `Debug/dap_cli_pauses_a_running_target_through_production_wire`,
  `Debug/dap_cli_keeps_running_state_transitions_on_the_production_wire`,
  `Debug/dap_cli_projects_raw_frames_only_when_requested_through_production_wire`).
- Console attach is SUB `Lens-6841-c36b-attach.r3`; on this host,
  `yama.ptrace_scope=1` makes its cases use the authorizer.

**Attack.** A DAP client that cannot attach or pause is not VS Code parity.
The Canvas window runs in a browser.

**Verdict.** Fails.

**Gaps.**

- G14: DAP attach, pause and restart. Proposed: "the four #6730 known-failing
  DAP rows pass on the native tier."
- G15: Canvas replay controls and dump/recording open. Closed by c58 (open).
- Console attach is c36 (open).

### Generic instances

**Peer.** Every native debugger stops in each instantiation of a template.

**Jet today.** p300: `Debug/breakpoint_in_generic_module_instance` PASS.

**Verdict.** Holds only within stated limits: one tier ran. c41 asks for O0,
O1 and O2 (open).

## Profiler axes

### P1. Sampling

**Peer.** Superluminal samples at 8 kHz with full stacks on every thread
([superluminal.eu](https://superluminal.eu/docs/documentation.html)). Linux
`perf` adds precise PEBS/IBS program counters and LBR call stacks
([man7.org perf-record](https://man7.org/linux/man-pages/man1/perf-record.1.html)).
Go 1.18 moved to per-thread timers
([felixge.de](https://felixge.de/2022/02/11/profiling-improvements-in-go-1.18)).

**Jet today.** c44 is met.

- PASS on p299 and p300: `Perf/every_thread_keeps_whole_stacks`,
  `Perf/helper_samples_charge_the_calling_line`,
  `Perf/inlined_samples_name_the_inlined_line` and
  `Perf/unwinds_without_frame_pointers`.
- `Perf/receipt_reports_measured_overhead` PASSED on p299 and FAILS on p300
  ("JSON numeric comparison failed").

**Attack.**

- Without `perf_event_open` there is no precise program counter and no LBR,
  so skid remains.
- The overhead row moves between gates, so the receipt's figure is not yet a
  stable claim.
- Jet attributes samples by the compiler's own inline records, where `perf`
  needs DWARF.

**Verdict.** Holds only within stated limits: parity with Superluminal on
threads and stacks, compiler-exact helper and inline attribution, no precise
PC. The p300 overhead row must pass again.

**Gaps.**

- G16: the overhead receipt is stable across gates. Proposed: "the overhead
  row passes on three consecutive gates."
- G17: precise PC where the kernel allows. Proposed: "under `--lens=cpu`, a
  capture on a CPU with PEBS or IBS and `perf_event_paranoid` at most 1 charges
  samples by the precise PC, and says `precise` in the receipt."

### P2. Zones, frame marks, plots; zero cost off; instrument everything

**Peer.** Tracy: nanosecond zones, frame marks, plots, locks and messages
([github.com/wolfpld/tracy](https://github.com/wolfpld/tracy)). Jai's Iprof
instruments every procedure by a metaprogram. LLVM XRay's patchable sleds
([llvm.org](https://llvm.org/docs/XRay.html)).

**Jet today.** The runtime half (`5eae9c9e17`), the strip setting
(`19ea79ea21`) and package markers (`e890a78d1f`) landed, but Core's
`perf.zone`, `perf.plot` and `perf.frame` did not. On p300:

- `Perf/zones_strip_setting_leaves_no_site` FAILS (E1004 "`core.perf` has no
  item `zone`");
- `BuildEntry/meta_events_hear_package_marked_functions` and
  `BuildEntry/meta_events_rewrites_compose_in_order` FAIL.

**Verdict.** Fails. PENDING: `Main-6841-zone-c55-core`,
`Main-6841-c47-zone-core`, `Main-6841-c47-profiled-marker`,
`Main-6841-c47-perf-attach-zones`; c3 (`Main-6841-c3-instrument-every-function`)
and c6 (`Main-6841-c6-profile-overhead`).

**Gaps.** Closed by c47, c3 and c6 (open).

### P3 and P9. One timeline: waits, wakers, off-CPU, I/O

**Peer.** Superluminal shows each thread's states and the thread that
unblocked a wait. Tracy shows zones and locks live. Off-CPU flame graphs
(eBPF) show blocked time by stack
([brendangregg.com](https://www.brendangregg.com/offcpuanalysis.html)).
Chrome DevTools' Performance panel puts the main thread, network, frames and
GC on one timeline
([developer.chrome.com](https://developer.chrome.com/docs/devtools/performance/reference)).

**Jet today.** PASS on p299 and p300: `Perf/every_wait_names_its_waker`,
`Perf/off_cpu_time_by_stack`, `Perf/io_on_timeline_without_instrumentation`,
`Perf/socket_wait_names_its_peer` and `Perf/join_wait_names_awaited_task`.

**Attack.** It sees Jet code only. A wait inside FFI is invisible. The
timeline is a terminal and HTML view after the run, not live as Tracy's and
DevTools' are.

**Verdict.** Holds only within stated limits: exact wakers from the scheduler
beat Superluminal's inferred ones, for Jet code. Not live.

**Gap.** G18: a live timeline. Closed by c58's Profile view (open).

### P4. Memory and heap

**Peer.** Instruments' Allocations and Leaks keep every allocation's stack and
generations. Heaptrack does the same on Linux. Chrome DevTools heap snapshots
show retainer paths and compare two snapshots
([developer.chrome.com](https://developer.chrome.com/docs/devtools/memory-problems/heap-snapshots)).
Go pprof samples the heap by stack
([pkg.go.dev](https://pkg.go.dev/runtime/pprof)).

**Jet today.** c52's stack landed (`433d34956e`, `315cabf85e`, `d8275d5c78`,
`18577df9f7` and more). On p300:

- FAIL: `Perf/heap_snapshot_types_retention_and_diff` ("JSON filter selected
  no values") and `PerfRun/alloc_lens_heap_snapshots`;
- FAIL: `JetPerfTracePartsCaptureRun/perf_run_captures_wall_and_alloc_into_jettrace`
  and `JetPerfTracePartsCaptureAttach/perf_attach_captures_wall_and_alloc_with_jet_symbol_from_observe`.

**Verdict.** Fails.

**Gap.** G19: typed snapshots, retention and diffs pass on a gate. Closed by
c52 (open).

### P5. Hardware counters

**Peer.** VTune's top-down analysis (TMA) splits cycles into retiring, bad
speculation, front-end and back-end bound
([intel.com](https://www.intel.com/content/www/us/en/docs/vtune-profiler/user-guide/current/top-down-microarchitecture-analysis-method.html)).
`perf stat --topdown` gives level 1, and level 2 on newer Intel cores
([man7.org perf-stat](https://man7.org/linux/man-pages/man1/perf-stat.1.html)).

**Jet today.** On p299 and p300, `PerfRun/memory_counters_per_line`,
`PerfRun/no_counters_reports_unavailable` and
`PerfRun/memory_lens_alone_takes_no_cpu_time` PASS. TMA is SUB
`IfHole-6-1-c57-topdown.r2` (`PerfRun/topdown_levels_per_function`,
`PerfRun/topdown_unavailable_names_the_refusal`), which has run on no gate.

**Attack.** AMD Zen 1 and Zen 2 publish no LLC-miss event the lens can use
(revision 2).

**Verdict.** Fails (PENDING `IfHole-6-1-c57-topdown.r2`). The memory lens alone
is at parity with `perf` per line.

**Gap.** Closed by c57 (open).

### P6. Causal profiling

**Peer.** Coz's virtual speedups predict a line's whole-program gain from
progress points written into the source
([github.com/plasma-umass/coz](https://github.com/plasma-umass/coz)).

**Jet today.** The compiler half landed (`fd33c440f0`).
`Perf/causal_profile_predicts_speedup` and `Perf/causal_counts_answered_requests`
belong to `Main-6841-c55-causal` and `Main-6841-c55-request-progress`, which
have run on no gate.

**Verdict.** Fails (PENDING).

**Gap.** Closed by c55 (open).

### P7. Static cost beside measured cost

**Peer.** None ships the compiler's cost facts per line beside measured cost.
`llvm-mca` predicts a snippet's throughput, apart from any profile.

**Jet today.** `Perf/view_shows_cost_facts_beside_samples` PASS on p299 and
p300.

**Attack.** The facts are qualitative (allocations, copies, locks, dispatch,
bounds checks). A predicted number would invite a false ranking, and none is
shown.

**Verdict.** Holds: no peer has the compiler's facts.

### P8. Diffs and regression gates

**Peer.** pprof `-diff_base` gives per-function and per-line deltas
([github.com/google/pprof](https://github.com/google/pprof)). Bencher fails CI
on a significant slowdown.

**Jet today.** `Perf/compare_per_line_with_significance` PASS on p300 (c56,
`86c0cc3201`).

**Attack.** A verdict needs repeated captures; with one capture each, compare
refuses. Compare exits 0, and the gate is `jet budget check`.

**Verdict.** Holds only within stated limits: pprof's diff plus significance,
on repeated captures.

### P10. Always-on and attach

**Peer.** Parca and Grafana Pyroscope profile every process on a host through
eBPF. Go's pprof endpoint serves profiles from any service that imports it.

**Jet today.** `LiveInspect/` rows PASS on p300. `Perf/attach_drains_flight_ring`
FAILS on p299 and p300 (a snapshot mismatch on its `ready.out`).

**Verdict.** Fails.

**Honest limit.** eBPF profilers see every process; Jet sees Jet processes.

**Gap.** G20: attach drains the flight ring. Closed by c51 (open).

### Nothing but `jet`

**Jet today.** c34 is met: every Debug and Perf case that proves a product
behavior runs with no lldb, gdb, Linux perf, go or otelcol on PATH. c37 is
met: no text names an outside tool as Jet's debugger or profiler. Revision 2
(F17) ran every probe with an empty PATH. lldb appears only as D1's comparator
in `Tools/Perf/DebugLatency`.

**Verdict.** Holds.

## Verdict table

"Today" is the verdict for "jet alone matches or beats the best peer", from
gates p299 and p300. PENDING names the SUB whose rows have not run.

| Axis | Best peers | Today | Closed by |
|---|---|---|---|
| D1 Start and step | RemedyBG, RAD Debugger, lldb | fails (1,339 ms per step against under 1 ms; first stop wins); PENDING a rerun after `a39f5a5c5e` | c59 |
| D2 Record, replay, why | rr + Pernosco, TTD, Replay.io, UDB | fails; PENDING c43 SUBs | c43 |
| D3 Typed values at O2 | Visual Studio, RAD Debugger, lldb | fails on p300; PENDING `Lens-6841-c45v1-fix` | c45, c43(c) |
| D4 Evaluation | lldb, gdb `compile`, Chrome DevTools | fails; PENDING `Lens-6841-c45v1-fix` | c45, c42, G4-G6 |
| D5 Watch | gdb, lldb, rr | fails; PENDING `Lens-6841-c45v1-fix` | c49, G8, c43(b) |
| D6 Conditions, logpoints | gdb, Live++, Chrome DevTools, Replay.io | fails; PENDING `Lens-6841-c45v1-fix`, `Lens-6841-c49-r7-placed-conditions` | c49, G10 |
| D7 Hot patch | Live++, Chrome DevTools | fails (`fix_swaps_function_in_native_process`) | c54 |
| D8 Tasks, deadlocks | Go runtime, Java ThreadMXBean | fails; PENDING `IfHole-6-1-c48-s2-channel-cycles` | c48 |
| D9 Dumps | WinDbg minidumps | fails (the Secret reaches the dump file) | c51 |
| D10 Visualizers | natvis, gdb and lldb formatters | fails on p300; PENDING `Lens-6841-c45v1-fix` | c45, c50 |
| D11 Surfaces | RemedyBG, VS Code, Chrome DevTools | fails (DAP attach, pause, restart unbuilt) | c36, c58, G14 |
| Generic instances | every native debugger | holds only within stated limits (one tier) | c41 |
| P1 Sampling | Superluminal, Linux perf | holds only within stated limits (no precise PC; overhead row failed on p300) | G16, G17 |
| P2 Zones | Tracy, Jai Iprof, LLVM XRay | fails; PENDING zone-core SUBs | c47, c3, c6 |
| P3/P9 Timeline, waits | Superluminal, Tracy, Chrome DevTools | holds only within stated limits (Jet code; not live) | c58 |
| P4 Heap | Instruments, Heaptrack, Chrome DevTools | fails | c52 |
| P5 Counters | VTune, `perf stat` | fails; PENDING `IfHole-6-1-c57-topdown.r2` | c57 |
| P6 Causal | Coz | fails; PENDING c55 SUBs | c55 |
| P7 Cost beside measured | none | holds | none |
| P8 Diffs and gates | pprof, Bencher | holds only within stated limits (repeated captures) | none |
| P10 Always-on, attach | Parca, Pyroscope | fails (`attach_drains_flight_ring`) | c51 |
| Nothing but `jet` | the owner's order | holds | none |

c39 is not met: P7 and "nothing but `jet`" hold, and every other axis has an
open criterion or a proposed one. The limits no engineering removes are the
same as in revision 2:

- Jet sees Jet processes;
- unsafe and FFI code is visible only at its boundary;
- the hardware binds Jet as it binds every peer (four watch slots, counter
  availability);
- evaluation costs a fork.

## Finding dispositions

G1-G20 are this review's gaps. Each `card` row names the open #6841
criterion that closes it; the gaps this review proposed criteria for are
criteria c60 to c67.

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| G1 per step grows with the image | card | #6841 c59 |
| G2 record, replay, reverse, why | card | #6841 c43 |
| G3 a deleted -O2 value from the recording | card | #6841 c43 |
| G4 a fallible call shows Ok or Err | card | #6841 c60 |
| G5 second-evaluation latency | card | #6841 c66 |
| G6 evaluation in an inlined frame | card | #6841 c61 |
| G7 the caller-frame column | card | #6841 c42 |
| G8 a watch follows reallocation | card | #6841 c62 |
| G9 reverse watches | card | #6841 c43 |
| G10 placed logpoints | card | #6841 c63 |
| G11 fix of an edit calling an absent runtime entry | card | #6841 c54 |
| G12 deadlock stop | card | #6841 c48 |
| G13 a dump file keeps no Secret | card | #6841 c51 |
| G14 DAP attach, pause, restart | card | #6841 c64 |
| G15 Canvas replay controls, dumps, recordings | card | #6841 c58 |
| G16 a stable overhead receipt | card | #6841 c67 |
| G17 precise PC | card | #6841 c65 |
| G18 a live timeline | card | #6841 c58 |
| G19 heap snapshots pass on a gate | card | #6841 c52 |
| G20 attach drains the flight ring | card | #6841 c51 |
<!-- /audit-dispositions -->
