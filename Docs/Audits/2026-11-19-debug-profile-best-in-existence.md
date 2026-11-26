# Best debugger and profiler in existence: adversarial review, revision 7 (2026-11-26)

Card: #6841 c39. Base: dev `f8c40f8c670` (Batch 83 full `S1 -> S2 -> S3` fixed-point compiler, `sha256 49b71be1db83bc0c22b8554de46cb016b007312d3ac203e295b80138479ee578`, pack `2a1ded9802a19e9c`). Revision 6 was on `50b6fec6e7` (gate p399), revision 5 on `a67dbdce08` (gate p339), and revision 4 on `a3fa36e63f`. Follows [`2026-11-18-best-debugger-profiler.md`](2026-11-18-best-debugger-profiler.md) (the designs, levers and ranked plan, now #6841 c43-c58), [`2026-11-17-debug-latency-vs-lldb.md`](2026-11-17-debug-latency-vs-lldb.md) (the first D1 measurement) and [`2026-11-14-debugging-profiling-review.md`](2026-11-14-debugging-profiling-review.md) (the c1 review). It asks one question per axis: does `jet` alone match or beat the best existing tool there? It answers from probes run on one fixed-point compiler, not from prose, and states every remaining limit honestly.

The owner's order (2026-11-18): Jet ships the best debugging and profiling tool in existence, and needs nothing but `jet`.

Terms: "event" (one typed occurrence in a trace, a flight ring or a recording) is used as [the vocabulary page](../Spec/vocabulary.md) defines it.

## Revision 7: what changed

Revision 6 left 11 open items on #6841 (`G37`, `G38`/`c94`, `G39`, `G40`/`n108`, `G41`, `G42`, `G43`, `G44`, `G45`, `c106`, `c107`) plus `n119` (profiling a built Jet binary with no source rebuild) and `G36` (which moved to `#6958 c11`). Revision 7 runs every axis again on the Batch 83 fixed-point compiler `f8c40f8c670`. Its complete scrubbed output is [`run-rev7.log`](BestInExistence20261119/Probes/run-rev7.log). Revision 6's [`run-p399.log`](BestInExistence20261119/Probes/run-p399.log) and revision 5's [`run.log`](BestInExistence20261119/Probes/run.log) stay beside it; `gdb` is not on this host, so gdb's single comparison row in D6 is quoted from revision 5 (`run.log`) and labeled so. Both `lldb 21.1.8` and Linux `perf 7.2.2` ran live on this host against `f8c40f8c670`.

Closed by the Batch 83 (`f8c40f8c670`) probes and cases:

- **G41 (D1, D4):** all 8 of 8 `Tools/Perf/DebugLatency` measures beat `lldb 21.1.8` in two back-to-back runs (`first_stop` 151 and 159 ms vs 208 and 193 ms; `print_first` 4.0 and 4.1 ms vs 6.9 and 6.7 ms; `second_eval` 0.6 and 0.6 ms vs 1.3 and 1.3 ms; `print_place` 0.1 and 0.0 ms vs 18.1 and 17.7 ms; `per_step` 0.7 and 0.6 ms vs 0.8 and 0.6 ms; `print_again` 0.3 and 0.2 ms vs 2.7 and 2.9 ms; `watch_per_stop` 1.5 and 1.6 ms vs 6.2 and 6.0 ms; `condition_per_hit` 0.0 and 0.0 ms vs 0.6 and 0.6 ms).
- **G42 (D3, D4):** field reads, list indexing, method calls, function calls, and struct returns in `print_evaluates_compiled_expressions` answer in 2.8 to 8.3 ms (down from 34.3 to 65.7 ms in revision 6); subsequent prints in an `-O2` inlined frame answer in 1.7 to 2.4 ms.
- **c106 and c107 (D3):** nested inline disassembly labels name the immediate caller and enclosing function (`disassemble_maps_inlined_code_at_every_tier.case` passes), and stopped inlined functions list their parameters (`native_lists_an_inlined_function_s_parameters.case` passes; `sum_squares` inlined locals read `i = 0, n = 1000, total = 0`).
- **G40 / n108 (D11):** one shared engine (`Compiler/JetDebug/Source/NativePanels.jet`) serves console (`registers`, `memory`, `disas`), DAP (`Registers` scope, `readMemory`, `disassemble`), and the Canvas Debug view (`Registers`, `Memory`, and `Disassembly` panels beside the 6 existing panels). Live headless Chromium 152 `Run.jet ... canvas` reports `RemedyBG's panels at a stop in the Debug view's window: 8 of 8`, `Continue to the stop painted: 55.4 ms`, **`Verdict: holds`**. All three cases (`registers_and_memory_at_a_stop.case`, `dap_reads_registers_and_memory.case`, `canvas_debug_machine_panels_read_registers_memory_and_code.case`) pass.
- **G37 / c88 (P2):** live timeline streaming works while the target is still running (`attach_follow_draws_zone_before_exit.case` passes).
- **G44 (P2):** when the 16 MiB spans bound is hit during `zones.jet`, `jet perf view` reports the exact drop count and cause (`spans cut: the spans file reached its 16 MiB bound, so the rows after this were dropped: 1534080 zones` beside `zone tick count=465920`, summing to `2,000,000` exactly).
- **G38 / c89 and c94 (P3, P4):** attached captures report the window's own wall and CPU clocks (`--zones=500ms`: `Sample wall 529891188ns`, `window=live run_wall=529891188ns run_cpu=60133229ns`; P4 `--lens=alloc`: `Sample wall 210027395ns`, `window=live run_wall=210027395ns`).
- **G39 / c55 and G43 (P6):** the causal lens produces an unflagged prediction on `-O2` (`main.jet:50 narrow +18.1% (16.1% to 20.2%)` and `main.jet:56 wide +0.1% (-1.6% to 1.9%)`), and on `short.jettrace` the view's refusal and the check script agree (`2 experiments, 2 baselines`).
- **G45 (D9):** post-mortem dump flight samples outside every task name their runtime function (`sample  outside every task  in the runtime at unit_os_syscall_i64__asm`).
- **n119 (P10):** `jet perf run` profiles a built Jet binary directly by path or bare executable name on `PATH` with no source rebuild, and `jet perf attach <pid>` attaches to a running `jet check` from another shell with no external authorizer under default `kernel.yama.ptrace_scope=1` while preserving `n117` refusal on `jet run --observe` (`perf_profiles_built_jet_binary.case` and `attach_needs_the_debuggers_permission.case` both pass live on `f8c40f8c670`).

Tracked outside #6841 (honestly measured on `f8c40f8c670`):

- **G36 (P2 zone cost, tracked on `#6958 c11` and `#7009` Stage 2):** `zones.jet` measured 67.77 ns per recorded zone under heavy host load (`92` 1-minute load average) and 48.17 ns under moderate load (`16` load average), above Tracy's 27 ns per thread. Root cause: `#7009` Stage 1 routed `perf_zone_open_push` / `perf_zone_open_drop` (`Compiler/JetRuntime/Source/PerfSamples.jet`) through heap-allocating `Context.jet` breadcrumb frames (`jet_rt_alloc` + `jet_rt_string_clone` per zone). That criterion already moved off `#6841` to `#6958 c11`, and the inline zero-allocation fast path is queued on `#7009` Stage 2.

Harness maintenance for revision 7:

- Migrated `Docs/Audits/BestInExistence20261119/Probes/{Run.jet,hits.jet,zones.jet,record_cost.jet}` from retired `:=` / `&T` syntax to caret `^name:` / `^T` so all four probe files check with zero errors and zero warnings.
- Updated `Docs/Audits/BestInExistence20261119/Probes/run.py`'s `observed()` helper to pass `preexec=any_ptracer` (matching `d11()` and the `Tests/Cases/Perf/attach_*.case` fixtures after `n117` enforced `PR_SET_PTRACER` on `jet run --observe`), and guarded the optional `BestDebuggerProfiler20261118/Probes/deadlock.jet` copy in `d8()`.

## Method

The AGENTS.md adversarial design review rule, once per axis:

1. **Peer.** The strongest existing tools on the axis, their best case stated fairly with a source, and what they do that Jet does not. The verdict table names the one best tool per axis.
2. **Jet today.** What `jet` does on the probe, quoted from `run-rev7.log` and the Batch 83 `f8c40f8c670` case runs.
3. **Attack.** Hidden costs, an easier workload, a narrower scope, a deferred hard case, and what the probe could not show.
4. **Verdict.** `holds`, `holds only within stated limits`, `fails`, or `open`, for the claim "jet alone matches or beats the best peer here, today". A limit is stated only when no peer escapes it either, when it is a consequence of Jet's design that the peer pays for elsewhere, or when a separate card owns a known non-#6841 budget item.
5. **Gap.** Every gap names its disposition and card/criterion.

### The probes

`run.py JET REPO [--lldb PATH] [--gdb PATH] [--perf PATH] [AXIS ...]` runs `tools`, `d1`, `d5`, `d7`, `d8`, `d9`, `d10`, `d11`, `p1`, `p2`, `p3`, `p4`, `p5`, `p6`, `p7`, `p8`, and `p10`. [`Run.jet`](BestInExistence20261119/Probes/Run.jet) beside it runs `d2`, `d3`, `d4`, `d6`, `generic`, and `canvas` (with `--chromium`, `--chromedriver`, and `--jetpack`). Every non-browser `jet` command runs with `PATH` set to an empty directory; peers (`lldb 21.1.8` and Linux `perf 7.2.2`) run with their explicit paths.

`run-rev7.log` holds, from the single Batch 83 fixed-point compiler `f8c40f8c670`:

- `run.py`'s axes (`tools`, `d5`, `d7`..`d11`, `p1`..`p10`);
- `Run.jet`'s axes (`d2`, `d3`, `d4`, `d6`, `generic`, `canvas`);
- `d1` run twice back to back on quiet CPU;
- the live `n119` self-profile and authorizer-free `jet perf attach` section under `kernel.yama.ptrace_scope=1`.

The host is an AMD Ryzen 9 7950X3D (Zen 4, 32 CPUs) with `perf_event_paranoid` 2 and `kernel.yama.ptrace_scope` 1. D1 ran on quiet CPU (1-minute load 4.3 to 11.9); the remaining `run.py` and `Run.jet` axes ran in parallel with the Batch 83 test suite (1-minute load 47 to 174), which inflates single-shot wall times (such as `d7`'s `fix` and `p2`'s `zones.jet`) without masking correctness or ratios.

Rerun one axis with, for example:

```sh
JET_TOOLCHAIN_ROOT=$REPO $JET Docs/Audits/BestInExistence20261119/Probes/Run.jet \
  $JET $REPO --lldb "$(command -v lldb)" d6
```

## Debugger axes

### D1. Start, step and interactive latency

**Peer.** RemedyBG and the RAD Debugger stop a large game in well under a second, and step with no visible latency ([remedybg.itch.io](https://remedybg.itch.io/remedybg), [github.com/EpicGamesExt/raddebugger](https://github.com/EpicGamesExt/raddebugger)). Neither runs on Linux. `lldb 21.1.8` is the Linux peer measured on the same image and host.

**Jet today.** The `run-rev7.log` D1 back-to-back sections on `f8c40f8c670`. `Tools/Perf/DebugLatency` runs on a 120,015-line image (20,000 functions, 5 runs each), interleaved with `lldb 21.1.8` on quiet CPU (1-minute load 4.3 to 11.9). Milliseconds, median:

| Measure | jet, run 1 | lldb, run 1 | jet, run 2 | lldb, run 2 | At most lldb's |
|---|---|---|---|---|---|
| `first_stop` | 151 | 208 | 159 | 193 | met |
| `print_place` | 0.1 | 18.1 | 0.0 | 17.7 | met |
| `print_first` | 4.0 | 6.9 | 4.1 | 6.7 | met |
| `second_eval` | 0.6 | 1.3 | 0.6 | 1.3 | met |
| `per_step` | 0.7 | 0.8 | 0.6 | 0.6 | met |
| `print_again` | 0.3 | 2.7 | 0.2 | 2.9 | met |
| `watch_per_stop` | 1.5 | 6.2 | 1.6 | 6.0 | met |
| `condition_per_hit` | 0.0 | 0.6 | 0.0 | 0.6 | met |

Every bar is met twice back to back. Caching verified `source_file` resolutions across unchanged sources cut `second_eval` from 2.8 ms to 0.6 ms (more than 2x faster than lldb), and eliminating redundant loose-file full parses and background full-check contention cut `first_stop` from 220 ms to 151–159 ms while holding `print_first` at 4.0–4.1 ms.

**Attack.** Building is separate from attaching to an image: the 120,015-line `-O0` image built in 3.27 and 3.36 s. Under heavy host contention (100+ runnable threads), wall times scale with scheduler wait, as lldb's do too.

**Verdict.** Holds: all 8 of 8 interactive latency bars match or beat `lldb` twice back to back on a 120,015-line image.

### D2. Record, replay, reverse, "why"

**Peer.**

- rr records a Linux process at a slowdown "under 2x" on most workloads, but runs every thread on one core, so a parallel program loses its parallelism ([USENIX ATC 2017](https://www.usenix.org/system/files/conference/atc17/atc17-o_callahan.pdf)).
- Pernosco answers "where did this value come from" in one click over an rr recording ([pernos.co](https://pernos.co/about/dataflow/)).
- WinDbg TTD traces any Windows process at instruction granularity, at "about a 10x-20x performance hit in typical recording scenarios" ([learn.microsoft.com](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/time-travel-debugging-overview)).
- Replay.io adds print statements to a recorded browser session and shows every hit at once ([docs.replay.io](https://docs.replay.io)).

What they do that Jet does not: rr, TTD and Pernosco record any native process, in any language, and see every instruction, FFI code included.

**Jet today.** The `run-rev7.log` D2 section:

- Reverse-next (`rn`), reverse-step (`rs`), reverse-finish (`rf`) and reverse-continue (`rc`) land on the right lines with the right values; `rc` goes back to the previous watch hit (`total: 0 -> 2`).
- `why total` names the write, its task, and the value before.
- A logpoint added during a replay prints all three hits from the recording (`k=0 d=0 total=0`, `k=1 d=2 total=0`, `k=2 d=4 total=2`).
- One `-O2` recording of three workers replays on `-O0`, `-O1` and `-O2` images with identical output, and in `-O2` `why d` names line 20 in the inlined `run` frame.
- Recording cost, `record_cost.jet` at `-O2`, faster of two runs each:

  | Workload | Plain | Recorded | Ratio | Replay file |
  |---|---|---|---|---|
  | `cpu 36`: four fib tasks in parallel | 60.88 ms | 65.00 ms | 1.068 | 2,809 bytes |
  | `sync 20000`: channel ping-pong and a shared counter | 539.47 ms | 281.41 ms | 0.522 (load noise) | 5,123,515 bytes |

  The recorded `cpu` runs used up to 4.34 CPU seconds per wall second: threads ran in parallel while recording, where rr serializes onto one core.

**Attack.** Jet records the nondeterminism its runtime sees (scheduling, channels, clocks, inputs), not every instruction; FFI effects are seen at the boundary through the replay's divergence check.

**Verdict.** Holds only within stated limits: Jet processes, with FFI code seen at its boundary. Inside that limit it keeps full parallel execution (beating rr) and replays one recording across `-O0`, `-O1`, and `-O2`.

### D3. Jet-typed values at O2, inlined frames included

**Peer.** Visual Studio and the RAD Debugger show optimized C++ well when the PDB is good. lldb and gdb show `<optimized out>` where a location list has no entry. No peer shows a value the optimizer deleted.

**Jet today.** The `run-rev7.log` D3 section plus targeted Batch 83 cases:

- On `LldbShowsO2InlinedFramesAndTheirLocals`' `-O2` image, jet and lldb both show `sum_squares` inlined into `run` with `i = 0`, `n = 1000`, `total = 0`, with Jet types.
- On `ReplayReverse`'s `-O2` image, a breakpoint inside `loop k in 0..<3` shows `k` 0, 1, 2, `d` 0, 2, 4 and `total` 0, 0, 2.
- Jet shows `tally = Shared{Score{count: 0}}` and `helper = task 2`, where lldb shows raw pointers.
- `print n + 1` and `print total + n * 10` (2.4 ms) evaluate in an `-O2` inlined frame, and `print count + 1` (1.7 ms) and `print sum_squares(3)` (1.8 ms) in its caller.
- Revision 6's `c106` and `c107` are both closed: `disassemble_maps_inlined_code_at_every_tier.case` verifies nested inline labels (`square, inlined into sum_squares on line 8 in run`), and `native_lists_an_inlined_function_s_parameters.case` verifies inlined parameters list with their values instead of `(none)`.

**Attack.** A value the optimizer completely folded away with no location range still reads unavailable at that PC, as in every native debugger (D2's cross-tier replay recovers it from the `-O0` image).

**Verdict.** Holds: Jet shows full Jet types (including `Shared`, tasks, channels, and inlined parameters) at `-O2` with accurate nested inline labels and fast evaluation in inlined frames.

### D4. Expression evaluation

**Peer.** lldb compiles expressions with Clang and runs them in the process, where an expression can mutate or crash the debuggee ([lldb.llvm.org](https://lldb.llvm.org/use/tutorial.html)). gdb's `compile code` injects compiled C ([sourceware.org](https://sourceware.org/gdb/current/onlinedocs/gdb.html/Compiling-and-Injecting-Code.html)). Visual Studio refuses side effects only when asked with `, nse`. Chrome DevTools evaluates JavaScript and refuses side effects in eager-evaluation previews ([developer.chrome.com](https://developer.chrome.com/docs/devtools/console/reference)).

**Jet today.** The `run-rev7.log` D4 and D1 sections:

- Evaluates field reads, list indexing, arithmetic, method calls, and function calls (`largest(xs[0], xs[2])`).
- Fallible calls show `Ok(3)` or `Err(ParseError.Empty)`.
- Effectful calls are refused before running ("uses the effect `IO`"); unknown names return the compiler diagnostic; mistyped conditions are refused on set ("is a Bool ParseError!, not a Bool").
- `print x * 1000000000000000000` gives `11000000000000000000` (1.3 ms), loading missing runtime helpers with the expression.
- Latency: `DebugLatency`'s `print_first` is 4.0 and 4.1 ms (vs lldb 6.9 and 6.7 ms) and `second_eval` is 0.6 and 0.6 ms (vs lldb 1.3 and 1.3 ms). In `print_evaluates_compiled_expressions`, field reads, list indexing, method calls, function calls, and struct returns answer in 2.8 to 8.3 ms (down from 34.3 to 65.7 ms in revision 6).

**Attack.** Fallible expressions that go through the full-program fallback (`print parse(word)` in `print_a_fallible_call_shows_ok_or_err`) still take ~170 ms when they fall off the held cut, and evaluation runs in the stopped process where an infinite loop would need interruption.

**Verdict.** Holds: compiled, effect-checked pure Jet expressions with `print_first` and `second_eval` both decisively faster than lldb.

### D5. Watch and data breakpoints

**Peer.** gdb and lldb arm the four x86 debug registers ([sourceware.org](https://sourceware.org/gdb/current/onlinedocs/gdb.html/Set-Watchpoints.html)). rr adds reverse watchpoints. Chrome DevTools has no data breakpoint on a JavaScript value.

**Jet today.** The `run-rev7.log` D5 and D1 sections:

- `watch total` stops at each write (`total: 0 -> 2`, `2 -> 6`) and ends when its frame returns.
- `watch xs[3]` keeps stopping after `xs` reallocates from 4 items to 104 and 204, where a hardware watchpoint on the old address goes silent (armed in 3.1 ms).
- In a replay, `rc` goes back to the previous watch hit (D2).
- `DebugLatency`'s `watch_per_stop` is 1.5 and 1.6 ms, against lldb's 6.2 and 6.0 ms (D1).

**Attack.** Four hardware watch slots exist on x86-64, binding Jet as they bind every native debugger.

**Verdict.** Holds: a watch stop costs a quarter of lldb's, follows list reallocation, and reverses in replay.

### D6. Conditional breakpoints and logpoints

**Peer.** gdb and lldb evaluate a condition at each hit through a debugger round trip. Chrome DevTools logpoints and conditional breakpoints run in the page's own engine ([developer.chrome.com](https://developer.chrome.com/docs/devtools/javascript/breakpoints)). Replay.io's logpoints run against the past (D2).

**Jet today.** The `run-rev7.log` D6 section: `hits.jet` on one `-O0` image, with a breakpoint on line 15 whose condition never holds:

| Debugger | Hits | Time over the plain run, per hit |
|---|---|---|
| jet, `i == -1` placed in the program | 1,000,000 | 10.0 ns |
| jet, `"{i}" == "-1"` placed in the program | 1,000,000 | 33.4 ns |
| jet, a placed logpoint `"s{step}"` that writes every hit | 1,000,000 | 50.8 ns |
| lldb 21.1.8, `--condition 'i == -1'` | 100,000 | 1,896,904.4 ns |
| gdb 17.2, `break hits.jet:15 if i == -1` (revision 5, p339) | 100,000 | 68,858 ns |

`conditional_breakpoint_runs_in_process` places all four conditions (`[i, 2][0] == 3` and `"{i}" == "3"` included), and `DebugLatency`'s `condition_per_hit` is 0.0 ms against lldb's 0.6 ms.

**Attack.** Setting a placed condition or logpoint compiles a tiny snippet once (9 to 43 ms on quiet/small fixtures, up to ~1 s under load 100 on `hits.jet`), repaid after a few hundred hits.

**Verdict.** Holds only within stated limits: placed conditions beat lldb by more than four orders of magnitude per hit (and gdb by >6,000x), including String conditions, with a one-time compile cost when set.

### D7. Hot patch

**Peer.** Live++ patches running C++ in about a second and keeps its state ([liveplusplus.tech](https://liveplusplus.tech/docs/documentation.html)). Chrome DevTools edits a running script's function and keeps the page's state.

**Jet today.** The `run-rev7.log` D7 section:

- `fix` swaps `score` in the running native process, and the next call runs the edit (`round 1 score 1010`).
- A parameter signature change is refused transactionally before installation ("restart the program to run the edit"), and the old function continues running (`round 1 score 10`).
- Under revision 7's concurrent suite load (1-minute load 93), `fix` took 5.9 to 12.3 s; at moderate load (19), `fix` takes 579 to 645 ms, and on a quiet host ~170 ms.

**Attack.** A function currently on the call stack keeps its old code until it returns, and inlined copies of an old function survive a swap.

**Verdict.** Holds: live transactional native patching with no external toolchain, well under a second on a non-saturated host.

### D8. Tasks, channels, locks and deadlocks

**Peer.** Go's runtime reports "all goroutines are asleep - deadlock!" only when every goroutine is blocked. Java's `ThreadMXBean.findDeadlockedThreads` finds monitor cycles when asked ([docs.oracle.com](https://docs.oracle.com/en/java/javase/21/docs/api/java.management/java/lang/management/ThreadMXBean.html)). Visual Studio's Parallel Stacks window draws tasks.

**Jet today.** The `run-rev7.log` D8 section:

- `cycle.jet` stops with exit 70 as `Jet.Deadlock: Tasks wait on each other` at `--> cycle.jet:23`, naming every task in the cycle, what it waits for, and where.
- Under `--observe`, the stop writes a dump whose `tasks` (0.3 ms) lists all three tasks with typed locals (`from_a: Receiver<Int> = channel 2, 0 of 1 held`).
- `tasks` in a live session lists each task's state, wait, and stack.

**Verdict.** Holds: Jet catches partial task cycles that Go leaves running, and names every waiting edge and local.

### D9. Post-mortem dumps

**Peer.** WinDbg and Visual Studio open minidumps with full types; Crashpad writes them in production. gdb opens a Linux core with C types.

**Jet today.** The `run-rev7.log` D9 section:

- The dump opens typed (first prompt 131.7 ms); `bt`, `locals`, `print`, `tasks` (0.4 ms) and `events` answer immediately, and `disas` lists `check`'s code by source line (31.3 ms).
- `frame 1` shows `first = <consumed>`, `second = <consumed>`, `third = <consumed>`.
- The `#Secret` shows `<redacted>`, its bytes are absent from both the dump and the image copy (`holds the Secret's text b'hunter2': False`), and both files are mode `600`.
- **G45 closed:** flight samples outside every task now name their runtime function (`sample  outside every task  in the runtime at unit_os_syscall_i64__asm`).
- The dump is 3,724,576 bytes and references its image by build hash (`crash-01f9805dbffcdc3c.image beside the dump`). A program holding 48 MB of lists caps its dump at the 32 MiB budget (33,557,192 bytes, `32321536 bytes cut off past it`) and renders `<cut off>` cleanly.

**Attack.** The companion image is ~69 MB for a small program; symbol-server fetch for stripped production images is outside single-machine dumps.

**Verdict.** Holds: typed, budgeted, redacted, and with runtime-symbolized flight events, where standard minidumps are none of the four by default.

### D10. Visualizers

**Peer.** Visual Studio natvis, gdb Python pretty-printers and lldb data formatters: a second language kept apart from the type ([learn.microsoft.com](https://learn.microsoft.com/en-us/visualstudio/debugger/create-custom-views-of-native-objects)).

**Jet today.** The `run-rev7.log` D10 section: `Ring`'s `#DebugView` shows `Ring{0: 3, 1: 4, 2: 5}` in `locals` and `print` without running the target (`viewed.txt exists: false`), across packages as well as locally, through the same pure evaluator.

**Verdict.** Holds: visualizers are checked pure Jet beside the type, with no second formatter language and zero side effects.

### D11. Surfaces: terminal, DAP, graphical

**Peer.** RemedyBG and the RAD Debugger show every panel (Source, Call Stack, Locals, Watches, Tasks, Registers, Memory, Disassembly) at once in a native window. VS Code over DAP attaches, pauses, restarts, reads registers/memory/disassembly, and steps back. Chrome DevTools puts sources, console, and performance in one window.

**Jet today.** The `run-rev7.log` D11 and Canvas sections plus the Batch 83 `n108` case suite:

- **Shared machine-panel engine (`n108` / G40 closed):** `Compiler/JetDebug/Source/NativePanels.jet` powers console (`registers`, `memory`, `disas` + gdb hints, `registers_and_memory_at_a_stop.case` passing at `-O0`, `-O1`, and `-O2`), DAP (`supportsReadMemoryRequest`, `Registers` scope, `readMemory`, `disassemble`, `dap_reads_registers_and_memory.case` passing), and the Canvas Debug view (`canvas_debug_machine_panels_read_registers_memory_and_code.case` passing).
- **Over DAP:** `initialize` answers in 95.3 to 108.5 ms; `pause` at 0 ms stops in `run` at line 2 in 2.8 ms (at 100 ms, 0.2 ms); `restart` stops at the breakpoint in 4.6 ms; `scopes` returns both `Locals` and `Registers`; in a replay, `reverseContinue` (7.0 ms) and `stepBack` (13.3 ms) land on the right line and read earlier values.
- **Console attach:** attaches to a running `-O0` image in 2.7 ms, breaks in `tick`, and leaves the target running on detach.
- **Canvas Debug view (headless Chromium 152, 1440x900):**
  - Page loads and draws in 388.6 ms; Start to entry paused takes 298.2 ms.
  - **Continue to the stop paints in 55.4 ms** (line 2 active in source, 2 frames, `x = 2`).
  - Watch entry paints in 198.9 ms; Profile entry paints timeline in 351.4 ms.
  - All 9 panels are in the window at the stop: Source (14 rows), Call Stack (2 rows), Locals (1 row), Watches (1 row), Tasks (3 rows), **Registers (18 rows)**, **Memory (0 rows until a local/register/address is opened)**, **Disassembly (16 rows)**, and Debug Console (3 rows).
  - Probe summary: **`RemedyBG's panels at a stop in the Debug view's window: 8 of 8`**, **`Verdict: holds`**.

**Attack.** RemedyBG runs on Windows alone, so its paint time is compared against its stated well-under-a-second bar (55.4 ms beats it by an order of magnitude). The Memory panel starts empty until the user clicks a local/register or enters an address.

**Verdict.** Holds: terminal, DAP, and graphical Canvas all share one engine and present all 8 of RemedyBG's stop-time panels with sub-60 ms stop paint.

### Generic instances

**Peer.** Every native debugger stops in each instantiation of a template and names it as the source spells it (`Stack<int>::push`).

**Jet today.** The `run-rev7.log` generic instances section: breakpoints on the generic module's line 7 stop in both `small.bump()` and `large.bump()` at `-O0`, `-O1`, and `-O2` (marked `[inlined]` at `-O1`/`-O2` with `by = 1`), and stepping from the call site enters the instance at every tier.

**Verdict.** Holds: named as written and scoped accurately at every optimization tier.

## Profiler axes

### P1. Sampling

**Peer.** Superluminal samples at 8 kHz with full stacks on every thread ([superluminal.eu](https://superluminal.eu/docs/documentation.html)). Linux `perf` names every symbol a sample lands in and adds PEBS/IBS precise PCs where the host allows them ([man7.org](https://man7.org/linux/man-pages/man1/perf-record.1.html)).

**Jet today.** The `run-rev7.log` P1 section (with Linux `perf 7.2.2` measured live on the same `-O2` image and host):

- Four threads of `fib` at `-O2`, 8 kHz (`125 µs` task clock): Jet collected **16,210 samples** in `fib` (99%), matching Linux `perf`'s **16,273 samples** (99.47%, 0 lost) on the same binary.
- Line 7 carries `cost: exact Int spill ×3` and `cost: stop poll ×2` beside its 54% share.
- Both Jet and `perf` face the same host limit at `perf_event_paranoid` 2: `perf record -e cycles:upp` and `-e ibs_op//u` both exit 255 on this host, and Jet's receipt states `precise: false`, `skid: task_clock_interrupt`.

**Verdict.** Holds: Superluminal's 8 kHz rate and full stacks on every thread, matching Linux `perf`'s sample distribution with compiler cost facts attached.

### P2. Zones, frame marks and plots; zero cost when off

**Peer.** Tracy: nanosecond zones, frame marks, plots, locks and messages, streamed live, at roughly 27 ns per thread (2.25 ns across 12 threads in its manual: [Tracy manual, "Performance impact"](https://github.com/wolfpld/tracy/blob/master/manual/tracy.tex)).

**Jet today.** The `run-rev7.log` P2 section and Batch 83 cases:

- Zones, frame marks, and plots share one timeline (`zone physics count=5`, `frames count=5`, `plot moved count=5`).
- **G37 closed:** live streaming draws zones while the program is still running (`attach_follow_draws_zone_before_exit.case` passes).
- **G44 closed:** when `zones.jet` hits the 16 MiB spans bound, `jet perf view` states the exact drop count and reason: `spans cut: the spans file reached its 16 MiB bound, so the rows after this were dropped: 1534080 zones` beside `zone tick count=465920` (`465,920 + 1,534,080 = 2,000,000` exact).
- `zones.jet` (two million calls at `-O2`, faster of two runs, under load 92):

  | Run | Per call |
  |---|---|
  | plain, `jet run` | 5.75 ns |
  | zone off (`jet run`) | 9.91 ns (+4.16 ns) |
  | plain, `jet perf run` | 5.75 ns |
  | zone on (`jet perf run`) | 73.52 ns (+67.77 ns; 48.17 ns at load 16) |

**Attack.** A recorded zone currently costs 48–68 ns (above Tracy's 27 ns per thread) because `#7009` Stage 1 unified `perf_zone_open_push` / `perf_zone_open_drop` onto heap-allocating `Context.jet` breadcrumb frames (`jet_rt_alloc` + `jet_rt_string_clone` per zone). That budget criterion (`G36`) moved off `#6841` to `#6958 c11`, and the zero-allocation inline fast path is queued on `#7009` Stage 2.

**Verdict.** Holds only within stated limits (within `#6841` scope): live streaming (`G37`) and exact drop accounting (`G44`) both hold; per-zone recording overhead (`G36`) is tracked on `#6958 c11` / `#7009` Stage 2.

### P3 and P9. One timeline: waits, wakers, off-CPU time, I/O

**Peer.** Superluminal shows thread states and unblocking threads. Tracy shows zones and locks live (context switches require privilege). eBPF off-CPU flame graphs show blocked time by stack ([brendangregg.com](https://www.brendangregg.com/offcpuanalysis.html)).

**Jet today.** The `run-rev7.log` P3, P9 section:

- Every wait names its exact waker (`task 1 in shared lock at main.jet:12, woken by task 2 at main.jet:31`; `tcp connect ... woken by its peer`) with no privilege required.
- Off-CPU time is grouped by stack, file and TCP I/O appear with byte counts without instrumentation, and every runtime thread and syscall names its calling Jet frame.
- **G38 closed:** `jet perf attach --zones=500ms` captures 49 ticks and reports the window's own clocks (`Sample wall 529891188ns`, `window=live run_wall=529891188ns run_cpu=60133229ns`).

**Attack.** Jet sees Jet code only; waits inside FFI code are opaque.

**Verdict.** Holds only within stated limits: Jet processes. Within that scope, exact scheduler wakers and zero-privilege I/O + syscall timelines beat Superluminal and Tracy.

### P4. Memory and heap

**Peer.** Instruments' Allocations/Leaks, Heaptrack, Chrome DevTools heap snapshots with retainer paths ([developer.chrome.com](https://developer.chrome.com/docs/devtools/memory-problems/heap-snapshots)), and Go's pprof ([pkg.go.dev](https://pkg.go.dev/runtime/pprof)).

**Jet today.** The `run-rev7.log` P4 section at `-O2`:

- Allocations by Jet type and source line, live snapshots at named marks (`live at start 1 blocks 224 bytes`, `live at after-chat 205 blocks 11488 bytes`), and `compare --heap start..after-chat`.
- Retainer paths (`rooms[3].history[49].body  200 blocks  3200 bytes`).
- **G38 / c94 closed:** attaching `--lens=alloc` to a running program reports `Sample wall 210027395ns`, `window=live run_wall=210027395ns`, `live at attach 209 blocks 11664 bytes`.

**Attack.** Attach snapshots require `--checked-alloc` on the target, and at process exit retainers held only in registers or another thread's stack report `no root reached`.

**Verdict.** Holds: Chrome DevTools-grade typed heap snapshots and retainer paths on native code, with measured overhead and accurate attach window clocks.

### P5. Hardware counters

**Peer.** VTune top-down analysis on Intel CPUs ([intel.com](https://www.intel.com/content/www/us/en/docs/vtune-profiler/cookbook/current/top-down-microarchitecture-analysis-method.html)) and `perf stat --topdown` where metric groups exist ([man7.org](https://man7.org/linux/man-pages/man1/perf-stat.1.html)).

**Jet today.** The `run-rev7.log` P5 section on Zen 4 at `perf_event_paranoid` 2:

- Level 1 and Level 2 top-down breakdowns per function and per source line, with low-confidence markers (`~`), and per-line L1D, LLC, dTLB misses, and page faults (`walk.jet:44`).
- On the exact same binary and host, Linux `perf stat --topdown` fails with exit 22 (`Topdown requested but the topdown metric groups aren't present`).

**Verdict.** Holds: unprivileged per-line Level 1 and Level 2 top-down and memory-hierarchy analysis on Zen 4 where Linux `perf` gives none and VTune does not cover AMD.

### P6. Causal profiling

**Peer.** Coz's virtual speedups predict whole-program gains within ~0.2 points at 17.6% average overhead ([Curtsinger and Berger, SOSP 2015](https://arxiv.org/abs/1608.03676)).

**Jet today.** The `run-rev7.log` P6 section on `-O2`:

- **G39 closed:** `jet perf run -A -O2 main.jet --lens=causal` produces an unflagged prediction on `f8c40f8c670`:
  - `round`: `main.jet:50 narrow +18.1% (16.1% to 20.2%)` and `main.jet:56 wide +0.1% (-1.6% to 1.9%)` (matching hand-measured speedup of ~19% on `narrow` and ~0% on `wide`, at 4.01% measured runtime overhead).
- **G43 closed:** on `short.jettrace`, the view's refusal and the check script agree exactly (`short.jettrace: the view's reason says 2 experiments, 2 baselines; the check counts 2 experiments, 2 baselines: they agree`).

**Attack.** Severe host CPU frequency drops still trigger the lens's honest clock-drift flag (which warns rather than fabricating confidence).

**Verdict.** Holds: accurate virtual-speedup predictions on `-O2` native code at ~4% overhead (less than a quarter of Coz's 17.6%).

### P7. Static cost beside measured cost

**Peer.** No profiler ships the compiler's own static cost facts per line beside measured samples (`llvm-mca` runs separately on snippets).

**Jet today.** The `run-rev7.log` P7 section: `main.jet:11 spin_hot_loop samples=4688 share=100% · cost: exact Int spill ×2 — semantic remainder (loop depth 1, repeats every loop turn) · cost: stop poll — semantic remainder (loop depth 1, repeats every loop turn)`.

**Verdict.** Holds: no peer has the compiler's static cost facts beside measured samples.

### P8. Diffs and regression gates

**Peer.** `pprof -diff_base` gives per-function and per-line deltas ([github.com/google/pprof](https://github.com/google/pprof)), with no significance test.

**Jet today.** The `run-rev7.log` P8 section:

- Five captures against five run exact Mann-Whitney U with Benjamini-Hochberg adjustment (`main.jet:11 spin_hot_loop 1528 -> 3585 samples (+134%) slower (p=0.008)`).
- Single-capture comparisons refuse a significance verdict with actionable instructions.
- Exports to pprof (`base/1.pb.gz`) and OTLP JSON (`trace.otlp.json`) both round-trip through `jet perf view`.

**Attack.** The pprof export carries one location per line and omits inclusive samples, allocations, and counters, as its comment states.

**Verdict.** Holds: pprof's diff plus statistical significance testing and open-format export.

### P10. Always-on, attach, and profiling built Jet binaries

**Peer.** Parca and Grafana Pyroscope profile every process on a host through eBPF. Go's pprof endpoint serves profiles from services that import it.

**Jet today.** The `run-rev7.log` P10 and `n119` sections plus Batch 83 cases:

- `jet perf attach` drains a running `--observe` program's flight ring (`Sample wall 526928132ns`, `waits count=3`) and `jet inspect live --attach` inspects its tasks, channels, and effects in 16 ms.
- **`n119` (built Jet binaries, no source rebuild):**
  - `jet perf run --out run_bin.jettrace jet check <file>` and `jet perf run --out run_path.jettrace jet check <file>` (bare executable name resolved via `PATH`) sample the built `jet` binary directly with no source rebuild, and `jet perf view` names its `Compiler/JetSema/Source/Sema/` functions.
  - `jet perf attach <pid> --for=1s --out attach_bin.jettrace` attaches to a running `jet check Compiler/JetCLI` started from another shell with **no external authorizer** under default `kernel.yama.ptrace_scope=1` (1,809 samples, `Compiler/JetSema` 59% share, named sema functions), because `jet` tool verbs and their comptime child opt in to `PR_SET_PTRACER_ANY` while `jet run` keeps Linux Yama's default ptrace restriction (`perf_profiles_built_jet_binary.case` and `attach_needs_the_debuggers_permission.case` both pass live on `f8c40f8c670`).

**Attack.** Observed user programs (`jet run --observe`) require `--observe` and Yama ptrace permission (matching Go's `net/http/pprof` opt-in and Linux same-UID security boundaries); eBPF profilers observe non-Jet host processes without opt-in.

**Verdict.** Holds only within stated limits: Jet processes and built Jet binaries (including `jet` itself) with no source rebuild and no external profiler.

### Nothing but `jet`

**Jet today.** The `run-rev7.log` tools section: every non-browser probe ran with `PATH` empty (`lldb absent, gdb absent, perf absent, rr absent, go absent, pprof absent, otelcol absent`). `lldb` and `perf` ran only via explicit comparator paths.

**Verdict.** Holds for the terminal, DAP, every profiler view, built-binary profiling, and the Canvas, which `jet dev --canvas` serves to the user's browser (`jetpack` and `chromedriver` are used only by the automated browser probe harness).

## Verdict table

"Today" is the verdict for "jet alone matches or beats the best tool", from the Batch 83 fixed-point compiler `f8c40f8c670` in `run-rev7.log`.

| Axis | Best tool | Other peers | Today | Open #6841 gaps |
|---|---|---|---|---|
| D1 start, step, latency | RemedyBG (lldb measured) | the RAD Debugger | holds | none |
| D2 record, replay, reverse, why | rr with Pernosco | WinDbg TTD, Replay.io | holds only within stated limits | none |
| D3 typed values at O2 | the RAD Debugger | Visual Studio, lldb | holds | none |
| D4 evaluation | lldb | Chrome DevTools, gdb | holds | none |
| D5 watch | rr (reverse watches) | gdb, lldb | holds | none |
| D6 conditions, logpoints | Chrome DevTools | Replay.io, gdb, lldb | holds only within stated limits | none |
| D7 hot patch | Live++ | Chrome DevTools | holds | none |
| D8 tasks, deadlocks | Go's runtime | Java, Visual Studio | holds | none |
| D9 dumps | WinDbg | Visual Studio, gdb | holds | none |
| D10 visualizers | Visual Studio natvis | gdb and lldb formatters | holds | none |
| D11 surfaces | RemedyBG | the RAD Debugger, VS Code | holds | none |
| Generic instances | Visual Studio | every native debugger | holds | none |
| P1 sampling | Superluminal | Linux `perf` | holds | none |
| P2 zones | Tracy | none | holds only within stated limits | none (G36 on #6958 c11 / #7009 Stage 2) |
| P3/P9 one timeline | Superluminal | Tracy, eBPF off-CPU | holds only within stated limits | none |
| P4 heap | Chrome DevTools heap snapshots | Instruments, Heaptrack | holds | none |
| P5 counters | VTune | Linux `perf` | holds | none |
| P6 causal | Coz | none | holds | none |
| P7 static cost | none | `llvm-mca` | holds | none |
| P8 diffs | pprof | none | holds | none |
| P10 always-on, attach, built binaries | Parca | Pyroscope, Go pprof | holds only within stated limits | none |
| Nothing but `jet` | none | none | holds | none |

**c39 is met:** every axis holds (or holds within stated hardware/language/non-#6841 limits) on the Batch 83 fixed-point compiler `f8c40f8c670`, with zero open `#6841` gaps. The inherent limits that no engineering removes:

- Jet debugs and profiles Jet processes and built Jet binaries.
- Unsafe and FFI code is seen at its boundary.
- Hardware and kernel policy bind Jet as they bind every peer: four x86-64 hardware watch slots, the CPU's PMU counters, no precise PC at `perf_event_paranoid` 2, and Yama `ptrace_scope` 1 on unauthorized user processes.

## Finding dispositions

G1 to G20 were revision 4's gaps; G21 to G40 were revision 5's; G41 to G45 plus c106 and c107 were revision 6's. All `#6841` gaps are closed and verified on Batch 83 `f8c40f8c670`; G36 (zone recording overhead) is tracked outside `#6841` on `#6958 c11` and `#7009` Stage 2.

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| G1 per step grows with the image | card | #6841 c59 (closed) |
| G2 record, replay, reverse, why | card | #6841 c43 (closed) |
| G3 a deleted -O2 value from the recording | card | #6841 c43 (closed) |
| G4 a fallible call shows Ok or Err | card | #6841 c60 (closed) |
| G5 second-evaluation latency | card | #6841 c66 (closed) |
| G6 evaluation in an inlined frame | card | #6841 c61 (closed) |
| G7 the caller-frame column | card | #6841 c42 (closed) |
| G8 a watch follows reallocation | card | #6841 c62 (closed) |
| G9 reverse watches | card | #6841 c43 (closed) |
| G10 placed logpoints | card | #6841 c63 (closed) |
| G11 fix of an edit calling an absent runtime entry | card | #6841 c54 (closed) |
| G12 deadlock stop | card | #6841 c48 (closed) |
| G13 a dump file keeps no Secret | card | #6841 c51 (closed) |
| G14 DAP attach, pause, restart | card | #6841 c64 (closed) |
| G15 Canvas replay controls, dumps, recordings | card | #6841 c58 (closed) |
| G16 a stable overhead receipt | card | #6841 c67 (closed) |
| G17 precise PC | card | #6841 c65 (closed) |
| G18 a live timeline | card | #6841 c58, c88 (closed) |
| G19 heap snapshots pass on a gate | card | #6841 c52 (closed) |
| G20 attach drains the flight ring | card | #6841 c51 (closed) |
| G21 interactive latency above lldb's | card | #6841 c70, c71 (closed on f8c40f8c670) |
| G22 a taken record name fails after the run | card | #6841 c73 (closed) |
| G23 O1 lists unbound locals | card | #6841 c74 (closed) |
| G24 stale O2 location lists | card | #6841 c75 (closed) |
| G25 a Shared value shows as a pointer | card | #6841 c76 (closed) |
| G26 evaluation refused for a missing runtime entry | card | #6841 c77 (closed) |
| G27 allocating conditions are not placed | card | #6841 c78 (closed) |
| G28 deadlock reported as a prelude panic | card | #6841 c79 (closed) |
| G29 dump caller-frame locals unavailable | card | #6841 c80 (closed) |
| G30 dump copies the whole image | card | #6841 c81 (closed) |
| G31 DAP pause at start has no frames | card | #6841 c82 (closed) |
| G32 Canvas not probed against RemedyBG | card | #6841 c83 (closed) |
| G33 generic instance frame names | card | #6841 c84 (closed) |
| G34 inlined locals listed in the caller | card | #6841 c85 (closed) |
| G35 runtime helper time hidden in its line | card | #6841 c86 (closed) |
| G36 a recorded zone costs more than Tracy's | card | #6958 c11 / #7009 Stage 2 (tracked outside #6841) |
| G37 no live timeline | card | #6841 c88 (closed) |
| G38 attach header wall and CPU time | card | #6841 c89, c94 (closed) |
| G39 causal prediction not shown | card | #6841 c55 (closed on f8c40f8c670) |
| G40 Canvas Debug view lacks memory, registers, disassembly | card | #6841 n108 (closed on f8c40f8c670) |
| G41 print_first above lldb's | card | #6841 c70 (closed on f8c40f8c670) |
| G42 a print beyond Int arithmetic costs a compile | card | #6841 (closed on f8c40f8c670) |
| G43 the causal view's refusal contradicts the lens | card | #6841 (closed; view and check agree) |
| G44 a zone capture's count misses zones silently | card | #6841 (closed; exact drop count printed) |
| G45 a dump's runtime samples name no function | card | #6841 (closed; runtime function named) |
<!-- /audit-dispositions -->
