# Best debugger and profiler in existence: adversarial review, revision 6 (2026-11-21)

Card: #6841 c39. Base: dev `50b6fec6e7` (gate p399). Revision 5 was on
`a67dbdce08` (gate p339), and revision 4 on `a3fa36e63f`. Follows
[`2026-11-18-best-debugger-profiler.md`](2026-11-18-best-debugger-profiler.md)
(the designs, levers and ranked plan, now #6841 c43-c58),
[`2026-11-17-debug-latency-vs-lldb.md`](2026-11-17-debug-latency-vs-lldb.md)
(the first D1 measurement) and
[`2026-11-14-debugging-profiling-review.md`](2026-11-14-debugging-profiling-review.md)
(the c1 review). It asks one question per axis: does `jet` alone match or beat
the best existing tool there? It answers from probes run on one compiler, not
from case rows or prose, and gives every gap a fix.

The owner's order (2026-11-18): Jet ships the best debugging and profiling tool
in existence, and needs nothing but `jet`.

Terms: "event" (one typed occurrence in a trace, a flight ring or a recording)
is used as [the vocabulary page](../Spec/vocabulary.md) defines it.

## Revision 6: what changed

Revision 5 found 20 gaps (G21 to G40), and #6841 took them as c70 to c89.
Revision 6 runs every axis again, with the same harness, on the gate p399
compiler. Its output is
[`run-p399.log`](BestInExistence20261119/Probes/run-p399.log). Revision 5's
[`run.log`](BestInExistence20261119/Probes/run.log) stays beside it, because it
holds the only gdb and Linux `perf` runs: neither is on this host any longer, so
their figures below come from revision 5, on p339, and say so.

Closed by the p399 probes:

- G21: seven of DebugLatency's eight measures are at most lldb's, two runs
  back to back. `print_first` is not (D1, now G41).
- G22: a taken recording name is refused before the program runs, under
  `jet debug --record` and `jet run --record` (D2).
- G23: an `-O1` replay's first stop lists `(none)` (D2).
- G24: an `-O2` loop's locals read 0, 1, 2 at successive hits, in jet and in
  lldb (D3).
- G25: a `Shared` local shows as `Shared{Score{count: 0}}` (D3).
- G26: `x * 1000000000000000000` evaluates to `11000000000000000000` (D4).
- G27: `[i, 2][0] == 3` and `"{i}" == "3"` run in the program, at 49.7 ns a hit
  for the String condition (D6).
- G28: a deadlock stops as `Jet.Deadlock` at the root task's waiting line (D8).
- G29 and G30: a dump's caller frame shows its moved tasks as `<consumed>`, and
  a small program's dump is 2.2 MB, its image kept beside it by build hash (D9).
- G31: a DAP `pause` at 0 ms stops in `run` with its frame (D11).
- G32: the Canvas axis ran on p399 itself (D11).
- G33 and G34: an instance frame reads `small.bump()`, and at `-O1` and `-O2`
  its inlined locals show in its own frame (generic instances).
- G35: `fib`'s line 7 names its `stop poll ×2` in the cost facts (P1, P7).

Still open, or new:

- G41 (new): `print_first` is 11.1 and 13.2 ms against lldb's 7.7 and 7.8
  (D1, D4).
- G42 (new): a print that reads a field, indexes a list, calls a function or
  runs in an inlined frame takes 34 to 66 ms. Int arithmetic on a plain frame's
  locals takes 1.2 to 6.7 ms (D3, D4).
- G36: a recorded zone costs 29.56 ns, against Tracy's 27 (P2). Revision 5
  measured 59.25 ns. The criterion moved to #6958 c11.
- G37 (c88): the timeline is drawn only from a finished capture (P2).
- G38 (c89) and c94: an attached capture's header gives `run_wall=861299318ns`
  for a 500 ms window, and `Sample wall 2214ns` (P3, P4).
- G40: the Canvas Debug view shows no memory, registers or disassembly (D11).
- G43 (new): the causal lens withheld its gains on a CPU clock drop. Its view
  then gives a refusal reason that its own record contradicts (P6).
- G44 (new): a capture of two million zones counts 465,920 of them, and says
  nothing of the rest (P2).
- G45 (new): a dump's flight sample outside every task says only `in the
  runtime`, where `jet perf view` names the runtime function (D9).
- c106 and c107 (open, found on p399): inlined code at `-O1` and `-O2` is
  labeled with the wrong call line, and an inlined function's parameter shows
  `(none)` (D3).

The probe harness had one defect: `run.py`'s d7 broke on `prog.jet:17`, a line
the fixture no longer has code on. The session ran through without its `fix`.
`run.py` now breaks on line 15, and the log's D7 section is that run.

## Method

The AGENTS.md adversarial design review rule, once per axis:

1. **Peer.** The strongest existing tools on the axis, their best case stated
   fairly with a source, and what they do that Jet does not. The verdict table
   names the one best tool per axis.
2. **Jet today.** What `jet` does on the probe, quoted from `run-p399.log`.
3. **Attack.** Hidden costs, an easier workload, a narrower scope, a deferred
   hard case, and what the probe could not show.
4. **Verdict.** `holds`, `holds only within stated limits`, `fails`, or `open`,
   for the claim "jet alone matches or beats the best peer here, today". A limit
   is stated only when no peer escapes it either, or when it is a consequence of
   Jet's design that the peer pays for elsewhere. Anything else that loses is
   `fails`. An axis whose p399 probe did not show the claim either way is
   `open`.
5. **Gap.** Every gap names its fix. A gap with a criterion names it; the others
   are proposed criteria for #6841 (G41 onward, in [Gaps](#gaps) and the
   dispositions table).

### The probes

`run.py JET REPO [--lldb PATH] [--gdb PATH] [--perf PATH] [AXIS ...]` runs the
axes in order. It uses the `Tests/Cases` fixtures and stdin files of `REPO` as
they are, the three probe programs beside it
([`record_cost.jet`](BestInExistence20261119/Probes/record_cost.jet),
[`hits.jet`](BestInExistence20261119/Probes/hits.jet) and
[`zones.jet`](BestInExistence20261119/Probes/zones.jet)), the 2026-11-18
review's `deadlock.jet`, and `Tools/Perf/DebugLatency`. The axes D2, D3, D4, D6,
generic instances and canvas are in
[`Run.jet`](BestInExistence20261119/Probes/Run.jet) beside it, which takes the
same arguments but `--perf` and runs the same commands with the same report.
`Run.jet`'s `canvas` axis (D11) takes `--chromium`, `--chromedriver` and
`--jetpack`: the jetpack pins the browser in the probe's work directory with
`jetpack browser lock`, as the Canvas cases do, and Tools/CanvasTest drives the
page. That scenario runs with the host's `PATH`, which the browser needs. Every
other `jet` command runs with `PATH` set to an empty directory. A console
session is driven one command at a time, and each command is timed from sending
it to the next `(Jet) ` prompt. A DAP session is timed from each request to its
response, or to the event it waits for.

`run-p399.log` holds, in axis order:

- `run.py`'s axes and `Run.jet`'s;
- D1 run twice back to back;
- D7 rerun with the corrected line;
- the canvas axis;
- one section by hand: the taken recording name of D2.

Every command's line gives its wall time, its CPU time and the host's load
average when it started. The host is an AMD Ryzen 9 7950X3D (Zen 4, 32 CPUs)
with `perf_event_paranoid` 2. Other agents' builds
shared it. The 1-minute load average was:

| Axes | 1-minute load |
|---|---|
| `run.py`'s axes | 5.4 to 6.7 |
| `Run.jet`'s axes | 7 to 11 |
| D1 and D7 | 18 to 27 |
| canvas | 26 |

Each axis ran alone, in a memory-capped systemd scope at `nice 10`. Timings
under 1 ms are noise at that load. The verdicts rest on ratios of 10 or more, or
on a jet and a peer run back to back under the same load. lldb 21.1.8 is the
one peer run on this host.

Rerun one axis with, for example:

```sh
JET_TOOLCHAIN_ROOT=$REPO $REPO/Compiler/JetCLI/.jet/build/jet Docs/Audits/BestInExistence20261119/Probes/Run.jet \
  $REPO/Compiler/JetCLI/.jet/build/jet $REPO --lldb "$(command -v lldb)" d6
```

## Debugger axes

### D1. Start, step and interactive latency

**Peer.** RemedyBG and the RAD Debugger stop a large game in well under a
second, and step with no visible latency
([remedybg.itch.io](https://remedybg.itch.io/remedybg),
[github.com/EpicGamesExt/raddebugger](https://github.com/EpicGamesExt/raddebugger)).
Neither runs on Linux. lldb is the Linux peer this probe can measure: it loads
the image's DWARF up front, then answers a step or the print of a plain local
at once.

**Jet today.** The `run-p399.log` D1 section. The probe runs
`Tools/Perf/DebugLatency` on a 120,015-line image (20,000 functions, 5 runs
each). jet and lldb run on the same image and host, interleaved. It ran twice,
back to back, at a load average of 18 to 27. Milliseconds, the median:

| Measure | jet, run 1 | lldb, run 1 | jet, run 2 | lldb, run 2 | At most lldb's |
|---|---|---|---|---|---|
| `first_stop` | 123 | 239 | 158 | 261 | met |
| `print_place` | 0.1 | 19.0 | 0.1 | 19.5 | met |
| `print_first` | 11.1 | 7.7 | 13.2 | 7.8 | missed |
| `second_eval` | 1.4 | 1.7 | 1.3 | 1.6 | met |
| `per_step` | 0.4 | 1.5 | 0.4 | 1.7 | met |
| `print_again` | 0.5 | 3.1 | 0.5 | 3.6 | met |
| `watch_per_stop` | 2.2 | 9.0 | 2.4 | 10.0 | met |
| `condition_per_hit` | 0.0 | 1.0 | 0.0 | 1.0 | met |

Revision 5 measured `first_stop` at 895 ms and `watch_per_stop` at 30.8, both
above lldb's.

**Attack.** `print_first` is above lldb's in both runs, by 3.4 and 5.4 ms; c70
says every measure. Building is not in the table: `jet debug prog.jet` builds
first, as `lldb` never does. The image's build took 3.9 and 4.7 s. On the small
fixtures, the first prompt of a source session came 55.6 to 199.4 ms after
start, against 25.5 to 32.9 ms on a prebuilt image and 3.9 ms on attach (D2 to
D11, generic instances).

**Verdict.** Fails narrowly: one of eight measures, `print_first`, is above
lldb's.

**Gap.** G41: `print_first` at most lldb's again. c70 was met on an earlier
gate and does not hold on p399.

### D2. Record, replay, reverse, "why"

**Peer.**

- rr records a Linux process at a slowdown "under 2x" on most workloads, but
  runs every thread on one core, so a parallel program loses its parallelism
  ([USENIX ATC 2017](https://www.usenix.org/system/files/conference/atc17/atc17-o_callahan.pdf)).
- Pernosco answers "where did this value come from" in one click over an rr
  recording ([pernos.co](https://pernos.co/about/dataflow/)).
- WinDbg TTD traces any Windows process at instruction granularity, at "about a
  10x-20x performance hit in typical recording scenarios"
  ([learn.microsoft.com](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/time-travel-debugging-overview)).
- Replay.io adds print statements to a recorded browser session and shows every
  hit at once ([docs.replay.io](https://docs.replay.io)).

What they do that Jet does not: rr, TTD and Pernosco record any native process,
in any language, and see every instruction, FFI code included.

**Jet today.** The `run-p399.log` D2 section:

- Reverse-next, reverse-step, reverse-finish and reverse-continue land on the
  right lines with the right values, in 8.4 to 12.2 ms each. Reverse-continue
  goes back to the previous watch hit (`total: 0 -> 2`) in 11.6 ms.
- `why total` names the write, its task and the value before, in 39.3 ms.
- A logpoint added during a replay prints all three of its hits from the
  recording (`k=0 d=0 total=0`, `k=1 d=2 total=0`, `k=2 d=4 total=2`), as
  Replay.io's print statements do.
- One `-O2` recording of three workers replays on `-O0`, `-O1` and `-O2` images
  with the same output. The `-O1` replay's first stop lists `(none)`.
- In the `-O2` image, `why d` answers in 16.5 ms. It names line 20 in the
  inlined `run` frame ("before: none: this line binds it").
- A second `--record=NAME` with a taken NAME is refused before the program
  runs, under `jet debug` and `jet run` alike: `Jet.ReplayNameTaken: The
  recording name `dup` is taken, and nothing ran` (the section by hand).
- Recording cost, `record_cost.jet` at `-O2`, the faster of two runs each:

  | Workload | Plain | Recorded | Ratio | Replay file |
  |---|---|---|---|---|
  | `cpu 36`: four fib tasks in parallel | 78.30 ms | 75.47 ms | 0.96 (noise) | 2,809 bytes |
  | `sync 20000`: channel ping-pong and a shared counter | 156.01 ms | 142.41 ms | 0.91 (noise) | 5,123,515 bytes |

  The recorded `cpu` runs used 2.42 and 2.41 CPU seconds per wall second: the
  threads ran in parallel while recording. rr would have run them on one core.

**Attack.**

- Jet records the nondeterminism its runtime sees (scheduling, channels,
  clocks, inputs), not instructions. The effects of an FFI call, relaxed
  atomics and address-dependent behavior are seen only through the replay's
  divergence check.
- Both ratios are under 1: at this load the probe cannot tell a recorded run
  from a plain one, so it bounds the cost, not measures it.

**Verdict.** Holds only within stated limits: Jet processes, with FFI code seen
at its boundary. Inside that limit it beats rr on parallel programs (no
measurable cost, where rr gives up the cores) and TTD on cost (against 10x-20x).
It also adds what no peer has: one recording replayed at three tiers.

### D3. Jet-typed values at O2, inlined frames included

**Peer.** Visual Studio and the RAD Debugger show optimized C++ well when the
PDB is good. lldb and gdb show `<optimized out>` where a location list has no
entry. No peer shows a value the optimizer deleted.

**Jet today.** The `run-p399.log` D3 section:

- On `LldbShowsO2InlinedFramesAndTheirLocals`' `-O2` image, jet and lldb both
  show `sum_squares` inlined into `run` with `i`, `n` and `total`. Jet adds the
  types (`i: Int = 0`).
- On `ReplayReverse`'s `-O2` image, a breakpoint inside `loop k in 0..<3`
  stops three times. Jet shows `k` 0, 1, 2, `d` 0, 2, 4 and `total` 0, 0, 2, and
  so does lldb.
- Jet shows `tally = Shared{Score{count: 0}}` and `helper = task 2`. lldb shows
  `(Shared<module:29:package:12:case_fixture:0.1.0:prog.jet.Score>) tally =
  0x00007ffff7ef1f18` and `(Task<Int>) helper = 0x00007ffff7f44e18`.
- `print n + 1` and `print total + n * 10` evaluate in an inlined frame, and
  `print count + 1` and `print sum_squares(3)` in its caller.

**Attack.**

- Two open criteria were found on this compiler, proving c95. c106: `disas
  square` labels a nested inline with the outermost function and the innermost
  call line (`inlined into run on line 8`, where line 8 is in `sum_squares`).
  c107: stopped in the inlined `square`, `Locals:` shows `(none)` for its
  parameter `i`, which the stop instruction reads from `r14`.
- The first two prints in an inlined frame took 66.3 and 57.2 ms; the prints in
  its caller took 1.8 and 2.1 ms.

**Verdict.** Fails narrowly: jet shows the right values where lldb shows
pointers, but an inlined function's parameter can show `(none)` (c107), and an
inline's label can name the wrong call line (c106).

**Gaps.** c106 and c107 (open).

### D4. Expression evaluation

**Peer.** lldb compiles expressions with Clang and runs them in the process. It
handles templates and method calls, but it can change or crash the debuggee
([lldb.llvm.org](https://lldb.llvm.org/use/tutorial.html)). gdb's `compile
code` injects compiled C
([sourceware.org](https://sourceware.org/gdb/current/onlinedocs/gdb.html/Compiling-and-Injecting-Code.html)).
Visual Studio refuses side effects only when asked, with `, nse`. Chrome
DevTools evaluates any JavaScript in the paused frame, and its eager-evaluation
previews refuse side effects
([developer.chrome.com](https://developer.chrome.com/docs/devtools/console/reference)).

**Jet today.** The `run-p399.log` D4 section:

- The compiler evaluates fields, indexing, arithmetic, method calls and
  function calls (`largest(xs[0], xs[2])`).
- A call that can fail shows `Ok(3)` or `Err(ParseError.Empty)`.
- A call with an effect is refused before it runs ("uses the effect `IO`").
- An unknown name gets the compiler's message.
- `print x * 1000000000000000000` gives `11000000000000000000`: the runtime
  entry the image lacked is loaded with the expression's code.
- A condition of the wrong type is refused when set ("is a Bool ParseError!,
  not a Bool").

No peer proves an expression free of side effects before running it.

**Attack.**

- **Latency by expression.** Each print's time depends on what it reads:
  - A print that reads a field, indexes a list, calls a function or reads a
    String took 34.3 to 65.7 ms: seven on one stop of
    `print_evaluates_compiled_expressions` and three on
    `print_a_fallible_call_shows_ok_or_err`. D2's `print tally.count` took
    46.1 ms, and D3's `print n + 1` in an inlined frame 66.3 ms.
  - Int arithmetic on a plain frame's locals took 1.2 to 6.7 ms: `print n * 2`
    in a source session (3.2 ms), the three prints of
    `new_operations_at_a_stop_evaluate` on a prebuilt image, and D3's
    `print count + 1` (1.8 ms).
  - DebugLatency's `print_first` is the one of these lldb is measured on, and
    it is above lldb's (D1).
- Evaluation runs in the stopped process (c71), so an expression that loops
  forever would hold the stop. lldb bounds its expressions with a timeout; the
  probe did not try one that does not end.

**Verdict.** Fails narrowly, on latency: DebugLatency's `print_first`, and
every print beyond Int arithmetic on a plain frame's locals.

**Gaps.** G41, and G42: a print that reads fields, indexes, calls or runs in an
inlined frame answers within lldb's `print_first`.

### D5. Watch and data breakpoints

**Peer.** gdb and lldb arm the four x86 debug registers
([sourceware.org](https://sourceware.org/gdb/current/onlinedocs/gdb.html/Set-Watchpoints.html)).
rr adds reverse watchpoints. Chrome DevTools has no data breakpoint on a
JavaScript value.

**Jet today.** The `run-p399.log` D5 section:

- `watch total` stops at each write that changes it (`total: 0 -> 2`,
  `2 -> 6`) and ends when its frame returns.
- `watch xs[3]` keeps stopping after `xs` grows from 4 items to 104 and then
  204, where a debug register on the old address would go silent.
- In a replay, `rc` goes back to the previous watch hit (D2).
- DebugLatency's `watch_per_stop` is 2.2 and 2.4 ms, against lldb's 9.0 and
  10.0 (D1).

**Attack.** There are four hardware slots, as in every live debugger. Arming
the list watch took 48.3 ms, where `watch total` took 0.3 ms.

**Verdict.** Holds: a watch stop costs a quarter of lldb's, and a watch follows
a moved list, which gdb's and lldb's cannot.

### D6. Conditional breakpoints and logpoints

**Peer.** gdb and lldb evaluate a condition at each hit through a debugger round
trip. Chrome DevTools logpoints and conditional breakpoints run in the page's
own engine
([developer.chrome.com](https://developer.chrome.com/docs/devtools/javascript/breakpoints)).
Replay.io's logpoints run against the past (D2).

**Jet today.** The `run-p399.log` D6 section: `hits.jet` on one `-O0` image,
with a breakpoint on line 15 whose condition never holds.

| Debugger | Hits | Time over the plain run, per hit |
|---|---|---|
| jet, `i == -1` placed in the program | 1,000,000 | 16.2 ns |
| jet, `"{i}" == "-1"` placed in the program | 1,000,000 | 49.7 ns |
| jet, a placed logpoint `"s{step}"` that writes every hit | 1,000,000 | 119.3 ns |
| lldb 21.1.8, `--condition 'i == -1'` | 100,000 | 636,831 ns |
| gdb 17.2, `break hits.jet:15 if i == -1` (revision 5, p339) | 100,000 | 68,858 ns |

`conditional_breakpoint_runs_in_process` places all four of its conditions,
`[i, 2][0] == 3` and `"{i}" == "3"` among them, and each stops on the right hit.
DebugLatency's `condition_per_hit` agrees: 0.0 ms against lldb's 1.0 (D1).

**Attack.** Setting a placed condition costs a compile, 116 to 180 ms, and a
logpoint 239 to 336 ms. lldb sets one at once, then pays 0.64 ms a hit. At that
rate jet's condition is ahead from about the 280th hit.

**Verdict.** Holds only within stated limits: a placed condition beats lldb by
39,000 times a hit and gdb by 4,000 times, and a String condition is placed
too. The limit is the compile when it is set, which a condition that will be
hit a few hundred times repays.

### D7. Hot patch

**Peer.** Live++ patches running C++ in about a second and keeps its state
([liveplusplus.tech](https://liveplusplus.tech/docs/documentation.html)). Chrome
DevTools edits a running script's function and keeps the page's state.

**Jet today.** The `run-p399.log` D7 section:

- `fix` swaps `score` in the running native process in 165.8 ms, and the next
  call runs the edit (`round 1 score 1010`).
- A change of parameters is refused before anything is installed, with the
  reason ("restart the program to run the edit"), in 170.9 ms.

**Attack.** A function on a stack keeps its old code until it returns. Inlined
copies of an old function survive a swap.

**Verdict.** Holds: six times faster than Live++'s second, and transactional.

### D8. Tasks, channels, locks and deadlocks

**Peer.** Go's runtime reports "all goroutines are asleep - deadlock!" only when
every goroutine is blocked. Java's `ThreadMXBean.findDeadlockedThreads` finds
monitor cycles when asked
([docs.oracle.com](https://docs.oracle.com/en/java/javase/21/docs/api/java.management/java/lang/management/ThreadMXBean.html)).
Visual Studio's Parallel Stacks window draws tasks.

**Jet today.** The `run-p399.log` D8 section:

- `cycle.jet` stops with exit 70 as `Jet.Deadlock: Tasks wait on each other`,
  at the main task's waiting line (`--> cycle.jet:23`). It names every task in
  the cycle, what it waits for and where.
- The 2026-11-18 review's `deadlock.jet`, which hung then, stops the same way.
- Under `--observe` the stop writes a dump whose `tasks` lists all three tasks,
  each with typed locals (`from_a: Receiver<Int> = channel 2, 0 of 1 held`).
- `tasks` in a live session lists each task's state, wait and stack in 0.4 ms.

**Verdict.** Holds: Jet stops a cycle that Go leaves running whenever another
goroutine still runs, and it names the waits.

### D9. Post-mortem dumps

**Peer.** WinDbg and Visual Studio open minidumps with full types; Crashpad
writes them in production. gdb opens a Linux core with C types.

**Jet today.** The `run-p399.log` D9 section:

- The dump opens typed, with its first prompt in 81.5 ms. `bt`, `locals`,
  `print`, `tasks` and the last flight events answer in 0.1 to 0.4 ms each, and
  `disas` lists `check`'s machine code by source line in 14.5 ms.
- `frame 1` shows `run()` with `first = <consumed>`, `second = <consumed>` and
  `third = <consumed>`: the three tasks were joined before the stop.
- The Secret shows `<redacted>`, its text is not in the file, and the file is
  mode 600.
- The dump is 2,202,568 bytes. It names its image by build hash
  (`crash-cd5ba1722bc10c09.image beside the dump`).
- A program that holds 48 MB of lists keeps its dump within the 32 MiB budget,
  says what it cut (31,096,832 bytes), and shows `<cut off>` where it cut.

**Attack.**

- The image beside the dump is 58.6 MB for a 21-line program. A minidump names
  its modules and fetches them from a symbol server.
- Two of the 15 flight events read `sample  outside every task  in the
  runtime`. P1's view names the runtime function each such sample landed in
  (`unit_os_syscall_i64__asm`); the dump does not.

**Verdict.** Holds: typed, budgeted and redacted, where a minidump is none of
the three by default.

### D10. Visualizers

**Peer.** Visual Studio natvis, gdb Python pretty-printers and lldb data
formatters: a second language, kept apart from the type
([learn.microsoft.com](https://learn.microsoft.com/en-us/visualstudio/debugger/create-custom-views-of-native-objects)).

**Jet today.** The `run-p399.log` D10 section: a `Ring`'s `debug_view` shows
its three live items in order (`Ring{0: 3, 1: 4, 2: 5}`) in `locals` and
`print`, in 2.2 to 6.7 ms, without running the target (`viewed.txt exists:
false`). A view from a package works the same way. The view runs in the
stopped process, by the one evaluation mechanism (c71).

**Verdict.** Holds: the visualizer is Jet, is checked by the compiler, and has
no effect on the program.

### D11. Surfaces: terminal, DAP, graphical

**Peer.** RemedyBG and the RAD Debugger show every panel at once in a native
window. VS Code over DAP attaches, pauses, restarts and steps back. Chrome
DevTools puts sources, console and performance in one window.

**Jet today.** The `run-p399.log` D11 and Canvas sections:

- Over DAP:
  - `initialize` answers in 55.5 to 57.0 ms.
  - A `pause` sent with `configurationDone` stops in `run` at line 2, with its
    frame; sent 100 ms later, at line 3.
  - A breakpoint stop, `restart` (3.6 ms to the next stop), `variables` and
    `evaluate` all answer.
- In a replay over DAP, `reverseContinue` (7.2 ms) and `stepBack` (8.6 ms) land
  on the right line, and `evaluate k` reads the earlier value.
- The console attaches to a running `-O0` image (first prompt 3.9 ms), breaks
  in `tick`, and leaves the program running when it detaches.
- The Canvas Debug view, at a load average of 26:
  - The page loads and draws in 295.9 ms.
  - Start to the entry shown paused takes 159.3 ms.
  - Continue to a breakpoint paints the stop in 56.8 ms, with the source line
    active, both frames and `x = 2` in Locals.
  - A watch typed and entered paints its value in 195.1 ms.
  - At the stop, the window shows six panels at once: Source, Call Stack,
    Locals, Watches, Tasks and the Debug Console.
- The Profile view paints its timeline 262.3 ms after Profile entry, with Hot
  Lines and Selection beside it.

**Attack.**

- RemedyBG also shows memory, registers and disassembly at a stop; the Debug
  view has none of the three. `jet debug` disassembles (D9's `disas`), but the
  Canvas does not show it. The probe prints "Verdict: does not hold".
- RemedyBG runs on Windows alone, so its time is not measured here. It shows a
  stop in well under a second; jet's took 56.8 ms.
- The probe's program is a few lines long, so its profile is one sample: the
  axis times the Profile view's paint, not what it shows.

**Verdict.** Fails: the Debug view lacks three of RemedyBG's panels at a stop.

**Gap.** G40: the Canvas Debug view shows memory, registers and disassembly at
a stop, as RemedyBG does.

### Generic instances

**Peer.** Every native debugger stops in each instantiation of a template and
names it as the source spells it (`Stack<int>::push`).

**Jet today.** The `run-p399.log` generic instances section:

- A breakpoint on the generic module's line 7 stops in both instances,
  `small.bump()` and `large.bump()`, at `-O0`, `-O1` and `-O2`.
- At `-O1` and `-O2` the frame reads `small.bump()  at prog.jet:7  [inlined]`,
  and its local `by = 1` shows in it.
- A step from the call site enters the instance at every tier.

**Verdict.** Holds: named as written and scoped right at every tier.

## Profiler axes

### P1. Sampling

**Peer.** Superluminal samples at 8 kHz with full stacks on every thread
([superluminal.eu](https://superluminal.eu/docs/documentation.html)). Linux
`perf` names every symbol a sample lands in, the runtime's included, and adds
precise PEBS and IBS program counters where the host allows them
([man7.org](https://man7.org/linux/man-pages/man1/perf-record.1.html)).

**Jet today.** The `run-p399.log` P1 section:

- Four threads of `fib` run at `-O2`, sampled once per 125 µs of each thread's
  CPU time (8 kHz). 16,238 samples land in `fib`, and the capture kept 16,267
  of the 16,309 that 2,039 ms of CPU time gives.
- The receipt measures the sampler's own cost: 1.75%, of which the kernel's
  ring writes are 28.6 ms.
- Line 7's row carries `cost: stop poll ×2` beside its 62%.
- 28 samples fell outside `burn.jet`, each named (`unit_os_syscall_i64__asm
  11`, `unit.alloc_frames_key 4`, ...).
- Neither jet nor `perf` gets a precise program counter at
  `perf_event_paranoid` 2. Jet's receipt says `precise: false` and names the
  skid source, `task_clock_interrupt`. In revision 5, `perf` refused
  `cycles:upp` and `ibs_op//u` on this host.

**Attack.** `perf` is not on the host any longer, so the probe could not repeat
revision 5's split of `fib`'s samples. On p339, `perf` gave the runtime's stop
poll 33.7% self. c86 then inlined the poll's quiet test and named the poll per
line. The p399 view shows that naming, but this run has no hardware reference
to check the 5-point bar against.

**Verdict.** Holds: the rate, whole stacks on every thread and a measured
overhead under 2%, with Superluminal's rate and stacks. The precise-PC limit
binds `perf` on this host too.

### P2. Zones, frame marks and plots; zero cost when off

**Peer.** Tracy: nanosecond zones, frame marks, plots, locks and messages,
streamed live. Its manual measures "only 2.25 ns per zone (this includes two
events: start and end of a zone)": 37.7 ms for 16,777,216 zones on 12 threads
in parallel
([Tracy manual, "Performance impact"](https://github.com/wolfpld/tracy/blob/master/manual/tracy.tex)).
Per thread that is about 27 ns a zone.

**Jet today.** The `run-p399.log` P2 section: zones, frame marks and plots
share the timeline (`zone physics count=5`, `frames count=5`,
`plot moved count=5`). `zones.jet` makes two million calls at `-O2`; the table
gives the faster of two runs:

| Run | Per call |
|---|---|
| plain, `jet run` | 6.21 ns |
| a zone in each call, profiling off (`jet run`) | 8.40 ns |
| plain, `jet perf run` | 6.66 ns |
| a zone in each call, `jet perf run` | 36.22 ns |

A zone costs 2.19 ns when nothing is profiling and 29.56 ns when it records.
Revision 5 measured 59.25 ns.

**Attack.**

- 29.56 ns is still above Tracy's 27 ns per thread, and 13 times its headline
  figure.
- The view of the recorded run counts `zone tick count=465920` for two million
  calls, with no word of the other 1,534,080. Whether the zones were dropped,
  or the count is wrong, the probe cannot tell.
- Tracy's zones compile to nothing without `TRACY_ENABLE`. Jet's strip setting
  does the same, but the default build keeps the 2.19 ns.
- Jet's timeline, in `jet perf view` and in Canvas's Profile view, is drawn
  from a finished capture. Tracy's is live.

**Verdict.** Fails: a recorded zone costs 29.6 ns against Tracy's 27, and the
timeline is not live.

**Gaps.**

- G36: a recorded zone costs under 27 ns on `zones.jet`. The criterion moved to
  #6958 c11, which was met there; this probe does not meet it on p399.
- G37 (c88): the timeline is drawn while the program runs, from its flight
  ring.
- G44: a capture counts every zone the program entered, or says how many it
  dropped and why.

### P3 and P9. One timeline: waits, wakers, off-CPU time, I/O

**Peer.** Superluminal shows each thread's states and the thread that unblocked
a wait. Tracy shows zones and locks live, with context switches only when it
runs elevated. Off-CPU flame graphs (eBPF) show blocked time by stack
([brendangregg.com](https://www.brendangregg.com/offcpuanalysis.html)).

**Jet today.** The `run-p399.log` P3, P9 section:

- Every wait names its waker (`task 1 in shared lock at main.jet:12, woken by
  task 2 at main.jet:31`; `tcp connect … with peer 127.0.0.1:43039, woken by
  its peer`).
- Off-CPU time is shown by stack.
- Reads and writes appear with their byte counts.
- Every runtime thread shows its CPU and off-CPU time.
- Each syscall shows its count, its time and its calling Jet frame (c72).
- None of this needs privilege.
- `jet perf attach --zones=500ms` turns a running program's zones on for half a
  second: 50 frames of 10.4 ms.

**Attack.**

- Jet sees Jet code only; a wait inside FFI code is invisible.
- The attached capture's header says `run_wall=861299318ns` for a 500 ms
  window, `Sample wall 2214ns`, and `native process_cpu=0ns observed_at=0ns`.
- Liveness is P2's gap.

**Verdict.** Holds only within stated limits: Jet processes. There, exact
wakers from the scheduler beat Superluminal's inferred ones, with no privilege
needed.

**Gap.** G38 (c89, with c94 for the other lenses): an attached capture's header
gives the window's own wall and CPU time.

### P4. Memory and heap

**Peer.** Instruments' Allocations and Leaks instruments keep every
allocation's stack and generations. Heaptrack does the same on Linux. Chrome
DevTools heap snapshots show retainer paths and compare two snapshots
([developer.chrome.com](https://developer.chrome.com/docs/devtools/memory-problems/heap-snapshots)).
Go's pprof samples the heap by stack ([pkg.go.dev](https://pkg.go.dev/runtime/pprof)).

**Jet today.** The `run-p399.log` P4 section, at `-O2`:

- allocations by Jet type and line (`[Message]  chat.jet:22`);
- live snapshots at named marks, and `compare --heap` between two of them
  (`[Int]  chat.jet:22 run  +200 blocks  +3200 bytes`);
- retainer paths (`rooms[3].history[49].body  200 blocks  3200 bytes`);
- a snapshot of a running program that held its allocations for 148 µs.

The alloc lens states its own cost: 206 ns an allocation, 6.49% of CPU time.

**Attack.**

- The attach snapshot reads the checked heap, so the program must start with
  `--checked-alloc`, as the probe's `held.jet` does.
- At exit, the retainer of all 200 blocks is "no root reached".
- The attach capture's header has P3's fault (`Sample wall 4338ns`, c94).

**Verdict.** Holds: Jet types and retainer paths, as Chrome DevTools gives
them, on a native program, with the cost stated.

### P5. Hardware counters

**Peer.** VTune's top-down microarchitecture analysis splits cycles into
retiring, bad speculation, front-end bound and back-end bound, on Intel CPUs
([intel.com](https://www.intel.com/content/www/us/en/docs/vtune-profiler/cookbook/current/top-down-microarchitecture-analysis-method.html)).
`perf stat --topdown` gives level 1 where the kernel has the metric groups
([man7.org](https://man7.org/linux/man-pages/man1/perf-stat.1.html)).

**Jet today.** The `run-p399.log` P5 section, on this Zen 4 host at
`perf_event_paranoid` 2:

- Levels 1 and 2 per function and per line. `td.jet:35` is 100% back-end bound,
  and at level 2, 100% memory bound.
- Low-confidence lines are marked.
- L1D, LLC and dTLB misses and page faults are given per line.

In revision 5, `perf stat --topdown` on the same program refused: "Topdown
requested but the topdown metric groups aren't present".

**Verdict.** Holds: per-line top-down analysis without privilege, where Linux
`perf` gives none. VTune does not cover AMD CPUs.

### P6. Causal profiling

**Peer.** Coz's virtual speedups predict a line's whole-program gain. On
ferret it predicted 21.4% and the change gave 21.2%, and on dedup 9% and
8.95%, at 17.6% average overhead
([Curtsinger and Berger, SOSP 2015](https://arxiv.org/abs/1608.03676)).

**Jet today.** The `run-p399.log` P6 section, the `-O2` arm of
`causal_profile_predicts_speedup`:

- The lens's rows put the narrow line's virtual round at 1,026.4 µs against a
  baseline of 1,259.8: a 18.5% gain.
- Sped up by hand, the narrow line gained 18.7% (1,275.9 to 1,036.8 µs).
- For the wide line, the lens gives 0.0% and the hand speedup 0.4%.

**Attack.**

- The lens withheld its gains: "the profiled run's CPUs clocked 5153 to 4540
  MHz and it could not fit how that slowed its tasks". The check reports
  "the lens flagged its gains", not a prediction it can hold to an interval.
- `jet perf view` of the same capture gives another reason: "the run kept 2
  experiments of 25 ms … and no line had 6". The lens's own record counts 155
  experiments, 568 rounds on the narrow line and 784 on the wide one. One of
  the two reports is wrong.

**Verdict.** Open: the rows land within 0.4 points of the hand measurement, as
Coz's do, but the profiler withheld the prediction, so the probe has none to
judge.

**Gaps.** G39 (c55, met on p391, not shown on p399), and G43: the view's
refusal reason agrees with the lens's record.

### P7. Static cost beside measured cost

**Peer.** No tool ships the compiler's cost facts per line beside measured
cost. `llvm-mca` predicts a snippet's throughput, apart from any profile.

**Jet today.** The `run-p399.log` P7 section: each hot line carries the
compiler's facts (`cost: exact Int spill ×2 — semantic remainder (loop depth 1,
repeats every loop turn) · cost: stop poll — semantic remainder`). Revision 5's
gap, that the facts left out the stop poll, is closed.

**Verdict.** Holds: no peer has the facts at all.

### P8. Diffs and regression gates

**Peer.** `pprof -diff_base` gives per-function and per-line deltas
([github.com/google/pprof](https://github.com/google/pprof)), with no test of
significance.

**Jet today.** The `run-p399.log` P8 section:

- Five captures against five get an exact Mann-Whitney U per function and per
  line, adjusted by Benjamini-Hochberg (`spin_hot_loop 1215 -> 2427 samples
  (+99%) slower (p=0.008)`).
- One capture each gets no verdict, with the reason.
- Captures export to pprof and to OTLP JSON, and both read back.

**Attack.** The pprof export carries one location per line and drops
inclusive samples, allocations and counters; its comment says so.

**Verdict.** Holds: pprof's diff, plus a significance test.

### P10. Always-on and attach

**Peer.** Parca and Grafana Pyroscope profile every process on a host through
eBPF. Go's pprof endpoint serves profiles from any service that imports it.

**Jet today.** The `run-p399.log` P10 section: `jet perf attach` drains a
running program's flight ring in 128 ms (its waits, wakers and off-CPU stacks),
and `jet inspect live --attach` shows its tasks, channels and effects in 1 ms.

**Attack.** The program must have started with `--observe`, as D-OBSERVE-LIVE1
sets; the probe's does. eBPF profilers see every process, Jet's or not.

**Verdict.** Holds only within stated limits: Jet processes started with
`--observe`, as a Go service must import `net/http/pprof`.

### Nothing but `jet`

**Jet today.** The `run-p399.log` tools section: every probe but the canvas
axis's scenario ran with `PATH` empty. lldb, gdb, `perf`, rr, go, pprof and
otelcol were absent from it and from the host's `PATH`. lldb ran only as the
comparator, from a path given to the harness.

**Verdict.** Holds for the terminal, DAP, every profiler view and the Canvas,
which `jet dev --canvas` serves to the user's own browser. The canvas axis
needs `jetpack` and chromedriver only to pin and drive that browser.

## Verdict table

"Today" is the verdict for "jet alone matches or beats the best tool", from
the gate p399 probes in `run-p399.log`.

| Axis | Best tool | Other peers | Today | Gaps |
|---|---|---|---|---|
| D1 start, step, latency | RemedyBG (lldb measured) | the RAD Debugger | fails | G41 |
| D2 record, replay, reverse, why | rr with Pernosco | WinDbg TTD, Replay.io | holds only within stated limits | none |
| D3 typed values at O2 | the RAD Debugger | Visual Studio, lldb | fails | c106, c107 |
| D4 evaluation | lldb | Chrome DevTools, gdb | fails | G41, G42 |
| D5 watch | rr (reverse watches) | gdb, lldb | holds | none |
| D6 conditions, logpoints | Chrome DevTools | Replay.io, gdb, lldb | holds only within stated limits | none |
| D7 hot patch | Live++ | Chrome DevTools | holds | none |
| D8 tasks, deadlocks | Go's runtime | Java, Visual Studio | holds | none |
| D9 dumps | WinDbg | Visual Studio, gdb | holds | none |
| D10 visualizers | Visual Studio natvis | gdb and lldb formatters | holds | none |
| D11 surfaces | RemedyBG | the RAD Debugger, VS Code | fails | G40 |
| Generic instances | Visual Studio | every native debugger | holds | none |
| P1 sampling | Superluminal | Linux `perf` | holds | none |
| P2 zones | Tracy | none | fails | G36, G37, G44 |
| P3/P9 one timeline | Superluminal | Tracy, eBPF off-CPU | holds only within stated limits | G38 |
| P4 heap | Chrome DevTools heap snapshots | Instruments, Heaptrack | holds | none |
| P5 counters | VTune | Linux `perf` | holds | none |
| P6 causal | Coz | none | open | G39, G43 |
| P7 static cost | none | `llvm-mca` | holds | none |
| P8 diffs | pprof | none | holds | none |
| P10 always-on, attach | Parca | Pyroscope, Go pprof | holds only within stated limits | none |
| Nothing but `jet` | none | none | holds | none |

c39 is not met: five axes fail and one is open, and each has its gaps below.
The limits that no engineering removes:

- Jet sees Jet processes.
- Unsafe and FFI code is seen only at its boundary.
- The hardware binds Jet as it binds every peer: four watch slots, the counters
  the CPU has, and no precise program counter at `perf_event_paranoid` 2.

## Gaps

Every axis that does not hold, with its gap and fix, and one finding on an axis
that holds (G45). G37, G38 (with c94), G39, c106 and c107 have criteria. G36's
criterion moved to #6958 c11. G40 and G41 to G45 are proposed criteria for
#6841, and each names its proof.

- **G36** (P2). A recorded zone costs 29.56 ns, above 27. Fix: find the
  remaining 2.6 ns in the zone's record, which the probe does not split. Proof:
  `run.py … p2` shows a zone on under 27 ns. Criterion: #6958 c11, met there
  and not on p399.
- **G37** (P2, c88). The timeline is drawn only from a finished capture. Fix:
  Canvas's Profile view and an attach follow mode draw from the flight ring
  while the program runs, after a ballot for the new surface. Proof: a case
  that sees a zone drawn before the program exits.
- **G38** (P3, c89; c94 for every lens). An attached capture's header says
  `run_wall=861299318ns` for a 500 ms window, and `Sample wall 2214ns`. Fix:
  set the header from the window's clocks. Proof: `run.py … p3` shows a wall
  time of about 500 ms.
- **G39** (P6, c55). The lens withheld its prediction on this run. Proof:
  `run.py … p6` passes its check on a run whose clocks hold.
- **G40** (D11). The Canvas Debug view shows no memory, registers or
  disassembly at a stop, which RemedyBG shows. Fix: those three panels in the
  Debug view, the disassembly from the listing `jet debug` gives. Proof:
  `Run.jet … canvas` prints `Verdict: holds`. A new user surface, so a ballot
  first.
- **G41** (D1, D4). `print_first` is 11.1 and 13.2 ms against lldb's 7.7 and
  7.8 on the 120,015-line image. Fix: find what the first print in a stopped
  function pays that `second_eval` (1.3 ms) does not, and move it to the stop.
  Proof: `run.py … d1` shows every bar met in two runs back to back.
- **G42** (D3, D4). A print that reads a field, indexes a list, calls a
  function or runs in an inlined frame takes 34 to 66 ms, against 1.2 to 6.7
  ms for Int arithmetic on a plain frame's locals. Fix: find what those prints
  compile that Int arithmetic does not, and keep it from the first such print
  at a stop. Proof: `Run.jet … d4`'s `print_evaluates_compiled_expressions` and
  `Run.jet … d3`'s inlined prints answer within lldb's `print_first` on the
  same host.
- **G43** (P6). `jet perf view`'s causal refusal says the run kept 2
  experiments, where the lens's record has 155. Fix: the view reads the
  experiments the lens kept. Proof: `run.py … p6` shows the view's reason and
  the check's counts agreeing.
- **G44** (P2). `jet perf view` of a recorded `zones.jet` run counts 465,920
  zones for two million calls, and its receipt says nothing of the rest. Fix:
  the capture counts the zones it dropped, and the view prints that count with
  its cause. Proof: `run.py … p2` shows `count=2000000`, or the count plus a
  dropped count that sums to it.
- **G45** (D9). A dump's flight sample outside every task reads `in the
  runtime`. Fix: it names the runtime function its program counter is in, by
  the symbol table `jet perf view` uses. Proof: `run.py … d9` shows each
  `events` row with a Jet place or a runtime function.
- **c106** (D3, open). A nested inline's label names the outermost function
  with the innermost call line, and a call line outside the listed file does
  not name its file.
- **c107** (D3, open). Stopped in an inlined function at `-O1` and `-O2`,
  `locals` shows `(none)` for its parameter.

## Finding dispositions

G1 to G20 are revision 4's gaps, each with the criterion that closed it. G21
to G40 are revision 5's, taken as c70 to c89; revision 6's probes close G21 to
G35 but G32's c83 (whose proof is this revision's canvas log). G41 onward are
this revision's, proposed for #6841.

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
| G18 a live timeline | card | #6841 c58, reopened as G37 |
| G19 heap snapshots pass on a gate | card | #6841 c52 |
| G20 attach drains the flight ring | card | #6841 c51 |
| G21 interactive latency above lldb's | card | #6841 c70, c71; print_first reopened as G41 |
| G22 a taken record name fails after the run | card | #6841 c73 |
| G23 O1 lists unbound locals | card | #6841 c74 |
| G24 stale O2 location lists | card | #6841 c75 |
| G25 a Shared value shows as a pointer | card | #6841 c76 |
| G26 evaluation refused for a missing runtime entry | card | #6841 c77 |
| G27 allocating conditions are not placed | card | #6841 c78 |
| G28 deadlock reported as a prelude panic | card | #6841 c79 |
| G29 dump caller-frame locals unavailable | card | #6841 c80 |
| G30 dump copies the whole image | card | #6841 c81 |
| G31 DAP pause at start has no frames | card | #6841 c82 |
| G32 Canvas not probed against RemedyBG | card | #6841 c83 |
| G33 generic instance frame names | card | #6841 c84 |
| G34 inlined locals listed in the caller | card | #6841 c85 |
| G35 runtime helper time hidden in its line | card | #6841 c86 |
| G36 a recorded zone costs more than Tracy's | card | #6841 c87, moved to card 6958 c11 |
| G37 no live timeline | card | #6841 c88 |
| G38 attach header wall and CPU time | card | #6841 c89, c94 |
| G39 causal prediction not shown | card | #6841 c55 |
| G40 Canvas Debug view lacks memory, registers, disassembly | card | #6841 (proposed criterion) |
| G41 print_first above lldb's | card | #6841 (proposed criterion) |
| G42 a print beyond Int arithmetic costs a compile | card | #6841 (proposed criterion) |
| G43 the causal view's refusal contradicts the lens | card | #6841 (proposed criterion) |
| G44 a zone capture's count misses zones silently | card | #6841 (proposed criterion) |
| G45 a dump's runtime samples name no function | card | #6841 (proposed criterion) |
<!-- /audit-dispositions -->
