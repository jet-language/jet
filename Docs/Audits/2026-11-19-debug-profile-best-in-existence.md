# Best debugger and profiler in existence: adversarial review, revision 5 (2026-11-19)

Card: #6841 c39. Base: dev `a67dbdce08` (gate p339; revision 4 was on
`a3fa36e63f`). Follows
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

## Revision 5: what changed

Revision 4 read its verdicts from the case rows of gates p306 to p309. A case
row proves that a behavior exists; it does not measure the behavior against a
peer, and revision 4 called eleven axes "holds" on that footing. Revision 5
sets those verdicts aside and proves every axis again with one harness,
[`BestInExistence20261119/Probes/run.py`](BestInExistence20261119/Probes/run.py),
run against the gate p339 compiler. Its output is
[`run.log`](BestInExistence20261119/Probes/run.log). Where a peer runs on
Linux, the harness runs it on the same image and host: lldb 21.1.8, gdb 17.2
and Linux `perf` 7.2.9. Where it does not (WinDbg TTD, RemedyBG, the RAD
Debugger, Live++, Tracy, Superluminal, VTune, Instruments, Coz, Replay.io), the
peer's own published figure is used, with its source.

What the probes found that revision 4 did not say:

- In an `-O2` image a loop's locals keep their first values at every hit, and
  jet, lldb and gdb all show the stale values (D3).
- A `Shared` value shows as a bare pointer (D3).
- A generic instance's frame is named `__jet_small_dbump`, and at `-O1` and
  `-O2` its inlined locals are listed under its caller (generic instances).
- Three DebugLatency measures are above lldb's: `first_stop`, `print_first` and
  `watch_per_stop` (D1).
- A recorded zone costs 59 ns, where Tracy's costs about 27 ns per thread (P2).
- The causal prediction for the line sped up by hand is now too high: 16.8% to
  26.6% predicted, 10.9% to 16.2% measured (P6).
- The view charges a runtime helper that takes a third of `fib`'s time to
  `fib`'s lines without naming it there; Linux `perf` names it (P1).

And what revision 4 could not claim:

- A recording costs 1.02 times the plain run with four threads running in
  parallel (D2).
- A condition that never holds costs 9.5 ns a hit, against gdb's 69 µs and
  lldb's 916 µs on the same image (D6).
- Per-line top-down analysis works on this AMD host, where `perf stat
  --topdown` refuses (P5).

## Method

The AGENTS.md adversarial design review rule, once per axis:

1. **Peer.** The strongest existing tools on the axis, their best case stated
   fairly with a source, and what they do that Jet does not.
2. **Jet today.** What `jet` does on the probe, quoted from `run.log`.
3. **Attack.** Hidden costs, an easier workload, a narrower scope, a deferred
   hard case, and what the probe could not show.
4. **Verdict.** `holds`, `holds only within stated limits`, or `fails`, for the
   claim "jet alone matches or beats the best peer here, today". A limit is
   stated only when no peer escapes it either, or when it is a consequence of
   Jet's design that the peer pays for elsewhere. Anything else that loses is
   `fails`, and so is anything the probes did not show.
5. **Gap.** Every gap names its fix. A gap with an open criterion names it;
   the others are proposed criteria for #6841 (G21 onward, in [Gaps](#gaps)
   and the dispositions table).

### The probes

`run.py JET REPO [--lldb PATH] [--gdb PATH] [--perf PATH] [AXIS ...]` runs the
axes in order. It uses the `Tests/Cases` fixtures and stdin files of `REPO` as
they are, the three probe programs beside it
([`record_cost.jet`](BestInExistence20261119/Probes/record_cost.jet),
[`hits.jet`](BestInExistence20261119/Probes/hits.jet) and
[`zones.jet`](BestInExistence20261119/Probes/zones.jet)), the 2026-11-18
review's `deadlock.jet`, and `Tools/Perf/DebugLatency`. The axes D2, D3, D4, D6
and generic instances have since moved to
[`Run.jet`](BestInExistence20261119/Probes/Run.jet) beside it, which takes the
same arguments but `--perf` and runs the same commands with the same report;
`run.py` keeps the others. Every `jet` command runs
with `PATH` set to an empty directory. A console session is driven one command
at a time, and each command is timed from sending it to the next `(Jet) `
prompt. A DAP session is timed from each request to its response, or to the
event it waits for.

Every command's line gives its wall time, its CPU time and the host's load
average when it started. The host is an AMD Ryzen 9 7950X3D (Zen 4, 32 CPUs)
with `perf_event_paranoid` 2 and Yama `ptrace_scope` 1. Other agents' builds
shared it. The 1-minute load average was 6 to 30 for most axes, 53 to 141
during D1, and about 230 during D11 and P1. Each axis ran alone, in a
memory-capped systemd scope at `nice 10`. Timings under 1 ms are noise at that
load. The verdicts rest on ratios of 10 or more, or on a jet and a peer run
back to back under the same load.

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

**Jet today.** `run.log`, D1: `Tools/Perf/DebugLatency` on a 120,015-line image
(20,000 functions, 5 runs each), jet and lldb on the same image and host,
interleaved, at a load average of 53 to 141. Milliseconds, the median:

| Measure | jet | lldb | At most lldb's |
|---|---|---|---|
| `first_stop` | 895 | 793 | missed |
| `print_place` | 0.3 | 160.8 | met |
| `print_first` | 137.7 | 79.4 | missed |
| `second_eval` | 6.2 | 9.4 | met |
| `per_step` | 1.6 | 9.9 | met |
| `print_again` | 3.5 | 25.7 | met |
| `watch_per_stop` | 30.8 | 22.8 | missed |
| `condition_per_hit` | 0.0 | 5.8 | met |

**Attack.** The load inflated both columns: lldb's `per_step` was under 1 ms on
a quiet host (the 2026-11-17 audit). Ratios at this load are what the probe
shows, and three of them lose. Building is not in the table: `jet debug
prog.jet` builds first, as `lldb` never does, and its first prompt on the small
fixtures came 51 to 288 ms after start, against 2 to 5 ms on a prebuilt image
(D4, D6).

**Verdict.** Fails: `first_stop`, `print_first` and `watch_per_stop` are above
lldb's.

**Gap.** G21: every DebugLatency measure at most lldb's. This is c70 (open),
with c71 (open, one in-process evaluation mechanism).

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

**Jet today.** `run.log`, D2:

- Reverse-next, reverse-step, reverse-finish and reverse-continue land on the
  right lines with the right values, in 7.6 to 17.6 ms each. Reverse-continue
  goes back to the previous watch hit (`total: 0 -> 2`).
- `why total` names the write, its task and the value before, in 21 ms.
- A logpoint added during a replay prints all three of its hits from the
  recording (`k=0 d=0 total=0`, `k=1 d=2 total=0`, `k=2 d=4 total=2`), as
  Replay.io's print statements do.
- One `-O2` recording of three workers replays on `-O0`, `-O1` and `-O2` images
  with the same output. `why d` in an `-O2` image recovers the optimized-away
  local from the `-O0` replay (`d = 2 (recovered from the -O0 replay)`), in
  126 ms.
- Recording cost, `record_cost.jet` at `-O2`, the faster of two runs each:

  | Workload | Plain | Recorded | Ratio | Replay file |
  |---|---|---|---|---|
  | `cpu 36`: four fib tasks in parallel | 96.97 ms | 99.14 ms | 1.02 | 2,823 bytes |
  | `sync 20000`: channel ping-pong and a shared counter | 146.35 ms | 134.94 ms | 0.92 (noise) | 5,123,528 bytes |

  The recorded `cpu` runs used 2.0 and 2.3 CPU seconds per wall second: the
  threads ran in parallel while recording. rr would have run them on one core.

**Attack.**

- Jet records the nondeterminism its runtime sees (scheduling, channels,
  clocks, inputs), not instructions. The effects of an FFI call, relaxed
  atomics and address-dependent behavior are seen only through the replay's
  divergence check.
- A name that is already recorded makes `--record=NAME` fail after the run
  (`Jet.ReplayFinalizeFailed`), so the run's work is lost. It should refuse
  before the program starts.
- At `-O1`, the first stop of a replay lists the function's not-yet-bound
  locals as `<Unavailable>` (`done = <Unavailable>   lines = <Unavailable> …`),
  where `-O0` shows `(none)`.

**Verdict.** Holds only within stated limits: Jet processes, with FFI code seen
at its boundary. Inside that limit it beats rr on parallel programs (1.02,
where rr gives up the cores) and TTD on cost (1.02 against 10x-20x). It also
adds what no peer has: one recording replayed at three tiers, and `-O2` values
recovered from the `-O0` replay.

**Gaps.**

- G22: `--record=NAME` refuses a taken name before the program runs.
- G23: at `-O1`, a stop lists no local whose binding has not run, as `-O0`
  does.

### D3. Jet-typed values at O2, inlined frames included

**Peer.** Visual Studio and the RAD Debugger show optimized C++ well when the
PDB is good. lldb and gdb show `<optimized out>` where a location list has no
entry. No peer shows a value the optimizer deleted.

**Jet today.** `run.log`, D3:

- On `LldbShowsO2InlinedFramesAndTheirLocals`' `-O2` image, jet, lldb and gdb
  all show `sum_squares` inlined into `run` with `i` and `total`. Jet adds the
  types (`i: Int = 0`) and names a task `task 2` and a channel `channel 2, 0 of
  1 held`, where lldb shows `(Task<Int>) helper = 0x00007ffff7f44e18`.
- `print n + 1` and `print total + n * 10` evaluate in an inlined frame, and
  `print count + 1` in its caller.

**Attack.**

- **Stale values at `-O2`.** On `ReplayReverse`'s `-O2` image, a breakpoint on
  line 21 inside `loop k in 0..<3` stops three times. Jet shows
  `k = 0   total = 0` at all three stops, and so do lldb (`(Int) k = 0`) and gdb
  (`k = 0`). The image's location lists name a place that keeps the first
  iteration's values. The true values are `k` 0, 1, 2 and `total` 0, 0, 2, as
  the `-O0` image shows. A debugger that shows a wrong value is worse than one
  that says `<optimized out>`.
- **A Shared value is a pointer.** `tally`, a `Shared` `Score`, shows as
  `tally = 0x7ffff7ef1f18` at every stop and as
  `tally: Pointer = 0x7ffff7ef1f18` in `locals`. `print tally.count` reads 0.
- The first two prints in an inlined frame took 134 and 118 ms.

**Verdict.** Fails: at `-O2` the debugger shows wrong values for live locals.

**Gaps.**

- G24: at `-O2`, a local's location list never names a stale place. A stop
  shows the current value or `<optimized out>`, and the `-O2` loop probe shows
  `k` 0, 1, 2 in jet and in lldb.
- G25: a `Shared` value shows its type and its contents (as the source line
  prints it, `Shared{Score{count: 0}}`) at a stop, in `locals`, in `print` and
  in DAP variables.

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

**Jet today.** `run.log`, D4: the compiler evaluates fields, indexing,
arithmetic, method calls and function calls (`largest(xs[0], xs[2])`). A call
that can fail shows `Ok(3)` or `Err(ParseError.Empty)`. A call with an effect is
refused before it runs ("uses the effect `IO`"). An unknown name gets the
compiler's message. No peer proves an expression free of side effects before
running it.

**Attack.**

- **Latency.** In a `jet debug prog.jet` session a print takes 31 to 46 ms. In a
  session on a prebuilt image it takes 1.6 to 2.1 ms. lldb's first print on the
  large image is faster than jet's (D1, `print_first`).
- **The image limits the language.** `print x * 1000000000000000000` is
  refused: "it needs the runtime's `jet_int_mul`, which the running image does
  not hold". lldb evaluates any expression its process can run.
- Evaluation still has a fork path beside the in-process one (c71).

**Verdict.** Fails: latency (G21), and expressions that need a runtime entry
the image lacks.

**Gaps.**

- G21, with c71 (open).
- G26: an expression that needs a runtime entry the image lacks still
  evaluates, by loading the entry beside the expression's own code.

### D5. Watch and data breakpoints

**Peer.** gdb and lldb arm the four x86 debug registers
([sourceware.org](https://sourceware.org/gdb/current/onlinedocs/gdb.html/Set-Watchpoints.html)).
rr adds reverse watchpoints. Chrome DevTools has no data breakpoint on a
JavaScript value.

**Jet today.** `run.log`, D5: `watch total` stops at each write that changes it
(`total: 0 -> 2`, `2 -> 6`) and ends when its frame returns. `watch xs[3]` keeps
stopping after `xs` grows from 4 items to 104 and then 204, where a debug
register on the old address would go silent. In a replay, `rc` goes back to
the previous watch hit (D2).

**Attack.** There are four hardware slots, as in every live debugger. Arming
the list watch took 49.6 ms. On the large image, a watch stop takes 30.8 ms
against lldb's 22.8 (D1).

**Verdict.** Fails on cost: `watch_per_stop` is above lldb's (G21). What a
watch can follow beats gdb and lldb.

### D6. Conditional breakpoints and logpoints

**Peer.** gdb and lldb evaluate a condition at each hit through a debugger round
trip. Chrome DevTools logpoints and conditional breakpoints run in the page's
own engine
([developer.chrome.com](https://developer.chrome.com/docs/devtools/javascript/breakpoints)).
Replay.io's logpoints run against the past (D2).

**Jet today.** `run.log`, D6: `hits.jet` on one `-O0` image, with a breakpoint
on line 15 whose condition `i == -1` never holds.

| Debugger | Hits | Time over the plain run, per hit |
|---|---|---|
| jet, the condition placed in the program | 1,000,000 | 9.5 ns |
| jet, a placed logpoint `"s{step}"` that writes every hit | 1,000,000 | 91.9 ns |
| gdb 17.2, `break hits.jet:15 if i == -1` | 100,000 | 68,858 ns |
| lldb 21.1.8, `--condition 'i == -1'` | 100,000 | 916,166 ns |

DebugLatency's `condition_per_hit` agrees: 0.0 ms against lldb's 5.8 (D1).

**Attack.**

- A condition that allocates (`[i, 2][0] == 3`) or a logpoint that reads a
  `String` is not placed. The console says so, and each hit is then a stop.
- Setting a placed condition costs a compile, 105 to 121 ms, and a logpoint 198
  to 217 ms. gdb and lldb set one at once.

**Verdict.** Holds only within stated limits: a placed condition beats gdb by
7,000 times and lldb by 96,000 times a hit. One that cannot be placed pays for
a stop, as every gdb condition does. The time to set one is D1's gap.

**Gap.** G27: a condition or logpoint that allocates or reads a `String` is
placed, and allocates in the scratch arena the evaluator already uses.

### D7. Hot patch

**Peer.** Live++ patches running C++ in about a second and keeps its state
([liveplusplus.tech](https://liveplusplus.tech/docs/documentation.html)). Chrome
DevTools edits a running script's function and keeps the page's state.

**Jet today.** `run.log`, D7: `fix` swaps `score` in the running native process
in 198 ms, and the next call runs the edit. A change of parameters is refused
before anything is installed, with the reason ("restart the program"), in
250 ms.

**Attack.** A function on a stack keeps its old code until it returns. Inlined
copies of an old function survive a swap.

**Verdict.** Holds: five times faster than Live++'s second, and transactional.

### D8. Tasks, channels, locks and deadlocks

**Peer.** Go's runtime reports "all goroutines are asleep - deadlock!" only when
every goroutine is blocked. Java's `ThreadMXBean.findDeadlockedThreads` finds
monitor cycles when asked
([docs.oracle.com](https://docs.oracle.com/en/java/javase/21/docs/api/java.management/java/lang/management/ThreadMXBean.html)).
Visual Studio's Parallel Stacks window draws tasks.

**Jet today.** `run.log`, D8: `cycle.jet` stops with exit 70 and names every
task in the cycle, what it waits for and where. The 2026-11-18 review's
`deadlock.jet`, which hung then, now stops the same way. Under `--observe` the
stop writes a dump whose `tasks` lists all three tasks, each with typed locals
(`from_a: Receiver<Int> = channel 2, 0 of 1 held`). `tasks` in a live session
lists each task's state, wait and stack.

**Attack.** The stop is reported as a `Jet.Panic` at `<core.prelude>:0`, with the
panic's generic "Why" text, not at the waiting line.

**Verdict.** Holds: Jet stops a cycle that Go leaves running whenever another
goroutine still runs, and it names the waits.

**Gap.** G28: a deadlock stop has its own report, at the root task's waiting
line, not a panic in `<core.prelude>`.

### D9. Post-mortem dumps

**Peer.** WinDbg and Visual Studio open minidumps with full types; Crashpad
writes them in production. gdb opens a Linux core with C types.

**Jet today.** `run.log`, D9: the dump opens typed in 33 ms, and `bt`, `locals`,
`print`, `tasks` and the last flight events answer in 0.1 to 0.3 ms each. The
Secret shows `<redacted>`, its text is not in the file, and the file is mode
600. A program that holds 64 MB of lists keeps its dump's memory within the
32 MiB budget and says what it cut.

**Attack.**

- **A caller frame loses its locals.** `frame 1` in `crash.jet`'s dump shows
  `first`, `second` and `third`, three live tasks, as `<Unavailable>`.
- **Size.** The `crash.jet` dump is 55,130,032 bytes, of which 53,022,480 are
  the image. A minidump names its modules and fetches them from a symbol
  server.
- Three flight events read `sample`, with no place.

**Verdict.** Fails narrowly: a dump loses the live locals of a caller frame.

**Gaps.**

- G29: every live local of every frame in a dump shows its value, and every
  flight event in it names its place.
- G30: a dump names its image by build hash, and copies the image's bytes only
  when no copy of that build is on disk.

### D10. Visualizers

**Peer.** Visual Studio natvis, gdb Python pretty-printers and lldb data
formatters: a second language, kept apart from the type
([learn.microsoft.com](https://learn.microsoft.com/en-us/visualstudio/debugger/create-custom-views-of-native-objects)).

**Jet today.** `run.log`, D10: a `Ring`'s `debug_view` shows its three live
items in order (`Ring{0: 3, 1: 4, 2: 5}`) in `locals` and `print`, in 1.3 to
1.9 ms, without running the target (`viewed.txt exists: false`). A view from a
package works the same way.

**Attack.** The view runs in a fork, the mechanism c71 removes.

**Verdict.** Holds: the visualizer is Jet, is checked by the compiler, and never
runs in the program.

### D11. Surfaces: terminal, DAP, graphical

**Peer.** RemedyBG and the RAD Debugger show every panel at once in a native
window. VS Code over DAP attaches, pauses, restarts and steps back. Chrome
DevTools puts sources, console and performance in one window.

**Jet today.** `run.log`, D11, at a load average of about 230:

- Over DAP: `initialize` in 140 to 474 ms; a breakpoint stop, `restart` (41.8
  ms to the next stop), `variables` and `evaluate`.
- In a replay over DAP: `reverseContinue` (53 ms) and `stepBack` (41.8 ms) land
  on the right line, and `evaluate k` reads the earlier value.
- The console attaches to a running `-O0` image (first prompt 22.2 ms), breaks
  in `tick`, and leaves the program running when it detaches.

**Attack.**

- A `pause` sent as the program starts answers with a thread named `root (task
  1, blocked)` and no frames (`"stackFrames": []`). Sent 100 ms later, it
  shows `run` at line 3.
- This revision did not probe the graphical surface, the Canvas Debug and
  Profile views. Their only evidence is c58's gate run, which shows that they
  work, not that they match RemedyBG.

**Verdict.** Fails: an early pause shows no stack, and the graphical surface is
not measured against its peer.

**Gaps.**

- G31: a `pause` before the program's first Jet frame stops at that frame, or
  says that the program has not started yet, never with an empty stack.
- G32: `run.py` drives the Canvas Debug view and measures what RemedyBG
  shows at a stop: the time to the first painted stop, and the source, stack,
  locals, watches, tasks and memory panels.

### Generic instances

**Peer.** Every native debugger stops in each instantiation of a template and
names it as the source spells it (`Stack<int>::push`).

**Jet today.** `run.log`, generic instances: a breakpoint on the generic
module's line 7 stops in both instances at `-O0`, `-O1` and `-O2`, and a step
from the call site enters the instance.

**Attack.**

- Frames are named `__jet_small_dbump()` and `__jet_large_dbump()`, not as the
  source spells the instance (`small.bump`).
- At `-O1` and `-O2` the inlined instance's locals move to its caller. The stop
  on line 20 shows `by = <Unavailable>   total = <Unavailable>   total =
  <Unavailable>`, and the stop inside the instance shows `(none)`. At `-O0` it
  shows `by = 1`.

**Verdict.** Fails: the names are not Jet's, and inlined instance locals are
listed in the wrong frame.

**Gaps.**

- G33: a generic instance's frame is named as the source spells it.
- G34: at `-O1` and `-O2`, an inlined frame's locals are listed in that frame,
  never in its caller.

## Profiler axes

### P1. Sampling

**Peer.** Superluminal samples at 8 kHz with full stacks on every thread
([superluminal.eu](https://superluminal.eu/docs/documentation.html)). Linux
`perf` names every symbol a sample lands in, the runtime's included, and adds
precise PEBS and IBS program counters where the host allows them
([man7.org](https://man7.org/linux/man-pages/man1/perf-record.1.html)).

**Jet today.** `run.log`, P1: four threads of `fib` at `-O2`, one sample per
125 µs of each thread's CPU time (8 kHz), 16,227 samples in `fib` over 2.0 CPU
seconds. The receipt measures the sampler's own cost: 2.75%, of which the
kernel's signal delivery is 2.48 µs a sample. Linux `perf record -e
task-clock:u -F 8000 -g` on the same image took 16K samples. Neither gets a
precise program counter on this host: Jet's receipt says `precise: false`, and
`perf` refuses `cycles:upp` ("Invalid argument") and `ibs_op//u` ("enable
system wide with '-a'").

**Attack.** `perf` splits the samples as `burn.fib` 66.0% self and
`jet_ir_stop_turn` 33.7% self: a third of the time is in the runtime's stop
poll. Jet charges those samples to `fib`'s lines (line 7 71%, line 5 24%), as
its design says a helper's samples should be charged. Only the capture's
receipt says so, as one total for the whole run: "5242 samples in
compiler-emitted helpers, such as stop polls, count at the line that called
them". The view does not show which helper or which line, and line 7's cost
facts name only `exact Int spill ×3`. `perf` shows the split at once.

**Verdict.** Fails narrowly: the rate, the stacks and the stated overhead match
Superluminal, but the view hides runtime time inside the calling line.

**Gap.** G35: a line's row splits its samples into its own code and each runtime
helper it calls, named in Jet terms (`stop poll 33%`), and the compiler's cost
facts name the stop poll where it is emitted.

### P2. Zones, frame marks and plots; zero cost when off

**Peer.** Tracy: nanosecond zones, frame marks, plots, locks and messages,
streamed live. Its manual measures "only 2.25 ns per zone (this includes two
events: start and end of a zone)": 37.7 ms for 16,777,216 zones on 12 threads
in parallel
([Tracy manual, "Performance impact"](https://github.com/wolfpld/tracy/blob/master/manual/tracy.tex)).
Per thread that is about 27 ns a zone.

**Jet today.** `run.log`, P2: zones, frame marks and plots share the timeline.
`zones.jet`, two million calls at `-O2`, the faster of two runs:

| Run | Per call |
|---|---|
| plain, `jet run` | 6.58 ns |
| a zone in each call, profiling off (`jet run`) | 8.53 ns |
| plain, `jet perf run` | 6.65 ns |
| a zone in each call, `jet perf run` | 65.90 ns |

A zone costs 1.94 ns when nothing is profiling and 59.25 ns when it records.

**Attack.** That is twice Tracy's per-thread cost and 26 times its headline
figure. Tracy's zones compile to nothing without `TRACY_ENABLE`; Jet's strip
setting does the same, but the default build keeps the 1.94 ns. Jet's
timeline, in `jet perf view` and in Canvas's Profile view (c58), is drawn from
a finished capture. Tracy's is live.

**Verdict.** Fails: a recorded zone costs 59 ns against Tracy's 27, and the
timeline is not live.

**Gaps.**

- G36: a recorded zone costs at most Tracy's per thread: under 27 ns a zone on
  `zones.jet`.
- G37: the timeline is drawn while the program runs, from its flight ring.
  Revision 4 disposed of this as G18 under c58, but c58's Profile view draws a
  finished capture.

### P3 and P9. One timeline: waits, wakers, off-CPU time, I/O

**Peer.** Superluminal shows each thread's states and the thread that unblocked
a wait. Tracy shows zones and locks live, with context switches only when it
runs elevated. Off-CPU flame graphs (eBPF) show blocked time by stack
([brendangregg.com](https://www.brendangregg.com/offcpuanalysis.html)).

**Jet today.** `run.log`, P3: every wait names its waker (`task 2 in time sleep
at main.jet:29, woken by its timeout`; `tcp connect … with peer
127.0.0.1:45633, woken by its peer`). Off-CPU time is shown by stack, and reads
and writes with their byte counts, with no privilege needed. `jet perf attach
--zones=500ms` turns a running program's zones on for half a second (43 frames
of 10.4 ms).

**Attack.** Jet sees Jet code only; a wait inside FFI code is invisible. The
attached capture's header says `run_wall=1443ns run_cpu=0ns` for a 500 ms
window. Liveness is P2's gap.

**Verdict.** Holds only within stated limits: Jet processes. There, exact
wakers from the scheduler beat Superluminal's inferred ones, with no privilege
needed.

**Gap.** G38: an attached capture's header gives the window's wall and CPU
time.

### P4. Memory and heap

**Peer.** Instruments' Allocations and Leaks instruments keep every
allocation's stack and generations. Heaptrack does the same on Linux. Chrome
DevTools heap snapshots show retainer paths and compare two snapshots
([developer.chrome.com](https://developer.chrome.com/docs/devtools/memory-problems/heap-snapshots)).
Go's pprof samples the heap by stack ([pkg.go.dev](https://pkg.go.dev/runtime/pprof)).

**Jet today.** `run.log`, P4, at `-O2`:

- allocations by Jet type and line (`[Message]  chat.jet:22`);
- live snapshots at named marks, and `compare --heap` between two of them
  (`[Int]  chat.jet:22 run  +200 blocks  +3200 bytes`);
- retainer paths (`entry frame[2].history[49].body  150 blocks`);
- a snapshot of a running program that held its allocations for 102 µs.

The alloc lens states its own cost: 145 ns an allocation, 4.23% of CPU time.

**Attack.** The attach snapshot reads the checked heap, so the program must
start with `--checked-alloc`, as the probe's `held.jet` does. At exit, the
retainer of all 200 blocks is "no root reached".

**Verdict.** Holds: Jet types and retainer paths, as Chrome DevTools gives
them, on a native program, with the cost stated.

### P5. Hardware counters

**Peer.** VTune's top-down microarchitecture analysis splits cycles into
retiring, bad speculation, front-end bound and back-end bound, on Intel CPUs
([intel.com](https://www.intel.com/content/www/us/en/docs/vtune-profiler/cookbook/current/top-down-microarchitecture-analysis-method.html)).
`perf stat --topdown` gives level 1 where the kernel has the metric groups
([man7.org](https://man7.org/linux/man-pages/man1/perf-stat.1.html)).

**Jet today.** `run.log`, P5, on this Zen 4 host at `perf_event_paranoid` 2:
levels 1 and 2 per function and per line (`td.jet:35`: 90% back end bound, and
at level 2, 90% memory bound), with
low-confidence lines marked, and L1D, LLC and dTLB misses and page faults per
line. `perf stat --topdown` on the same program refuses: "Topdown requested
but the topdown metric groups aren't present".

**Verdict.** Holds: per-line top-down analysis without privilege, where Linux
`perf` gives none. VTune does not cover AMD CPUs.

### P6. Causal profiling

**Peer.** Coz's virtual speedups predict a line's whole-program gain. On
ferret it predicted 21.4% and the change gave 21.2%, and on dedup 9% and
8.95%, at 17.6% average overhead
([Curtsinger and Berger, SOSP 2015](https://arxiv.org/abs/1608.03676)).

**Jet today.** `run.log`, P6, the `-O2` arm of
`causal_profile_predicts_speedup`: the ranking is right (`narrow` first), and a
run too short to rank is refused with its reason. The prediction misses: the
fixture's check stops with "the narrow line sped up by hand gained 109 to 162,
outside its predicted 168 to 266".

**Verdict.** Fails: Coz's prediction lands within a point; Jet's is high by a
third.

**Gap.** G39: the prediction for a line sped up by hand is within its stated
interval at `-O0`, `-O1` and `-O2`. This is c55 (open).

### P7. Static cost beside measured cost

**Peer.** No tool ships the compiler's cost facts per line beside measured
cost. `llvm-mca` predicts a snippet's throughput, apart from any profile.

**Jet today.** `run.log`, P7: each hot line carries the compiler's facts
(`cost: exact Int spill ×2 — semantic remainder (loop depth 1, repeats every
loop turn)`).

**Attack.** The facts leave out the stop poll, which takes a third of `fib`'s
time (P1, G35).

**Verdict.** Holds: no peer has the facts at all. G35 completes them.

### P8. Diffs and regression gates

**Peer.** `pprof -diff_base` gives per-function and per-line deltas
([github.com/google/pprof](https://github.com/google/pprof)), with no test of
significance.

**Jet today.** `run.log`, P8: five captures against five, an exact Mann-Whitney
U per function and per line, adjusted by Benjamini-Hochberg (`spin_hot_loop
1224 -> 2385 samples (+94%) slower (p=0.008)`). One capture each gets no
verdict, with the reason. Captures export to pprof and to OTLP JSON, and both
read back.

**Attack.** The pprof export carries one location per line and drops
inclusive samples, allocations and counters; its comment says so.

**Verdict.** Holds: pprof's diff, plus a significance test.

### P10. Always-on and attach

**Peer.** Parca and Grafana Pyroscope profile every process on a host through
eBPF. Go's pprof endpoint serves profiles from any service that imports it.

**Jet today.** `run.log`, P10: `jet perf attach` drains a running program's
flight ring in 38 ms (its waits, wakers and off-CPU stacks), and `jet inspect
live --attach` shows its tasks and channels.

**Attack.** The program must have started with `--observe`, as D-OBSERVE-LIVE1
sets; the probe's does. eBPF profilers see every process, Jet's or not.

**Verdict.** Holds only within stated limits: Jet processes started with
`--observe`, as a Go service must import `net/http/pprof`.

### Nothing but `jet`

**Jet today.** `run.log`, tools: every probe ran with `PATH` empty. lldb, gdb,
`perf`, rr, go, pprof and otelcol were absent from it and from the host's
`PATH`. The peers ran only as comparators, from paths given to the harness.

**Verdict.** Holds for the terminal, DAP and every profiler view. The graphical
surface is not probed (G32).

## Verdict table

"Today" is the verdict for "jet alone matches or beats the best peer", from the
gate p339 probes in `run.log`.

| Axis | Best peers | Today | Gaps |
|---|---|---|---|
| D1 start, step, latency | RemedyBG, the RAD Debugger, lldb | fails | G21 |
| D2 record, replay, reverse, why | rr and Pernosco, WinDbg TTD, Replay.io | holds only within stated limits | G22, G23 |
| D3 typed values at O2 | Visual Studio, the RAD Debugger, lldb | fails | G24, G25 |
| D4 evaluation | lldb, Chrome DevTools | fails | G21, G26 |
| D5 watch | gdb, lldb, rr | fails | G21 |
| D6 conditions, logpoints | gdb, lldb, Chrome DevTools, Replay.io | holds only within stated limits | G27 |
| D7 hot patch | Live++, Chrome DevTools | holds | none |
| D8 tasks, deadlocks | Go, Java, Visual Studio | holds | G28 |
| D9 dumps | WinDbg, Visual Studio, gdb | fails | G29, G30 |
| D10 visualizers | natvis, gdb and lldb formatters | holds | none |
| D11 surfaces | RemedyBG, the RAD Debugger, VS Code | fails | G31, G32 |
| Generic instances | every native debugger | fails | G33, G34 |
| P1 sampling | Superluminal, Linux `perf` | fails | G35 |
| P2 zones | Tracy | fails | G36, G37 |
| P3/P9 one timeline | Superluminal, Tracy, eBPF off-CPU | holds only within stated limits | G38 |
| P4 heap | Instruments, Heaptrack, Chrome DevTools | holds | none |
| P5 counters | VTune, Linux `perf` | holds | none |
| P6 causal | Coz | fails | G39 |
| P7 static cost | none | holds | G35 |
| P8 diffs | pprof | holds | none |
| P10 always-on, attach | Parca, Pyroscope, Go pprof | holds only within stated limits | none |
| Nothing but `jet` | none | holds | G32 |

c39 is not met: ten axes fail, and each has its gaps below. The limits that no
engineering removes:

- Jet sees Jet processes.
- Unsafe and FFI code is seen only at its boundary.
- The hardware binds Jet as it binds every peer: four watch slots, the counters
  the CPU has, and no precise program counter at `perf_event_paranoid` 2.

## Gaps

Every axis that does not hold, with its gap and fix. G21 and G39 have open
criteria (c70 with c71, and c55). The rest are proposed criteria for #6841, and
each names its proof.

- **G21** (D1, D4, D5). `first_stop`, `print_first` and `watch_per_stop` are
  above lldb's on the 120,015-line image. Fix: c70 and c71. Proof:
  `run.py … d1` shows every bar met.
- **G22** (D2). `--record=NAME` fails after the run when NAME is taken. Fix:
  check the name before the program starts. Proof: a case that records the
  same name twice and sees the refusal before any program output.
- **G23** (D2). At `-O1` a stop lists not-yet-bound locals as `<Unavailable>`.
  Fix: list a local only from its binding on, at every tier. Proof: `Run.jet …
  d2`'s `-O1` replay shows `(none)` at line 18.
- **G24** (D3). `-O2` location lists name stale places: `k = 0` at every loop
  hit, in jet, lldb and gdb. Fix: end each location range where the value's
  register or slot is redefined, and say `<optimized out>` past it. Proof:
  `Run.jet … d3` shows `k` 0, 1, 2 at `-O2` in jet and lldb.
- **G25** (D3). A `Shared` value shows as `Pointer = 0x…`. Fix: type a `Shared`
  local by its contents. Proof: `Run.jet … d3` shows `Shared{Score{count: 0}}`.
- **G26** (D4). An expression that needs a runtime entry the image lacks is
  refused. Fix: load the entry with the expression's code. Proof: `Run.jet …
  d4` prints `x * 1000000000000000000`'s overflow stop or value.
- **G27** (D6). A condition that allocates, or a logpoint that reads a
  `String`, is not placed. Fix: place it, with its allocations in the scratch
  arena. Proof: `Run.jet … d6`'s `[i, 2][0] == 3` condition placed.
- **G28** (D8). A deadlock is reported as a panic at `<core.prelude>:0`. Fix: a
  deadlock report at the root task's waiting line. Proof: `run.py … d8`.
- **G29** (D9). A dump shows a caller frame's live tasks as `<Unavailable>`, and
  three flight events have no place. Fix: keep every frame's live locals, and
  the place of every event. Proof: `run.py … d9`'s `frame 1`.
- **G30** (D9). A dump is 55 MB for a small program because it copies the 53
  MB image. Fix: name the image by build hash and copy it only when no copy of
  that build is on disk. Proof: `run.py … d9` prints a dump under 4 MB.
- **G31** (D11). A DAP `pause` at start answers with an empty stack. Fix: stop
  at the first Jet frame, or say that the program has not started. Proof:
  `run.py … d11`'s 0 ms pause.
- **G32** (D11). The Canvas Debug and Profile views are not probed against
  RemedyBG. Fix: a `run.py` axis that drives Canvas and measures the time to
  the first painted stop and the panels shown. Proof: that axis's log.
- **G33** (generic instances). Frames are named `__jet_small_dbump`. Fix: name
  an instance frame as the source spells it (`small.bump`). Proof: `Run.jet …
  generic`.
- **G34** (generic instances). At `-O1` and `-O2`, inlined locals are listed
  under the caller, three times as `<Unavailable>`. Fix: scope each inlined
  local to its inlined frame. Proof: `Run.jet … generic` shows `by = 1` in the
  inlined frame at every tier.
- **G35** (P1, P7). The runtime's stop poll takes 33.7% of `fib`'s samples, and
  the view shows it neither by helper nor by line; only the receipt gives one
  run-wide total. Fix: split a line's samples by the runtime helper they land
  in, and add the poll to the cost facts. Proof: `run.py … p1` shows the poll's
  share on line 7, within 5 points of `perf`'s.
- **G36** (P2). A recorded zone costs 59.25 ns. Fix: find where the 59 ns
  goes (the probe does not split it) and cut the zone's record to a few stores
  into the thread's ring. Proof: `run.py … p2` shows a zone on under 27 ns.
- **G37** (P2). The timeline is drawn only from a finished capture. Fix: Canvas's
  Profile view and `jet perf attach --follow` draw from the flight ring while
  the program runs. Proof: a case that sees a zone drawn before the program
  exits. A new user surface, so a ballot first.
- **G38** (P3). An attached capture's header says `run_wall=1443ns
  run_cpu=0ns` for a 500 ms window. Fix: set the header from the window's
  clocks. Proof: `run.py … p3` shows a wall time of about 500 ms.
- **G39** (P6). The causal prediction is high by a third. Fix: c55. Proof:
  `run.py … p6` passes its check.

## Finding dispositions

G1 to G20 are revision 4's gaps, each with the criterion that closed it. G21
onward are this revision's: G21 and G39 have open criteria, and the rest are
proposed for #6841.

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
| G21 interactive latency above lldb's | card | #6841 c70, c71 |
| G22 a taken record name fails after the run | card | #6841 (proposed criterion) |
| G23 O1 lists unbound locals | card | #6841 (proposed criterion) |
| G24 stale O2 location lists | card | #6841 (proposed criterion) |
| G25 a Shared value shows as a pointer | card | #6841 (proposed criterion) |
| G26 evaluation refused for a missing runtime entry | card | #6841 (proposed criterion) |
| G27 allocating conditions are not placed | card | #6841 (proposed criterion) |
| G28 deadlock reported as a prelude panic | card | #6841 (proposed criterion) |
| G29 dump caller-frame locals unavailable | card | #6841 (proposed criterion) |
| G30 dump copies the whole image | card | #6841 (proposed criterion) |
| G31 DAP pause at start has no frames | card | #6841 (proposed criterion) |
| G32 Canvas not probed against RemedyBG | card | #6841 (proposed criterion) |
| G33 generic instance frame names | card | #6841 (proposed criterion) |
| G34 inlined locals listed in the caller | card | #6841 (proposed criterion) |
| G35 runtime helper time hidden in its line | card | #6841 (proposed criterion) |
| G36 a recorded zone costs 59 ns | card | #6841 (proposed criterion) |
| G37 no live timeline | card | #6841 (proposed criterion) |
| G38 attach header wall and CPU time | card | #6841 (proposed criterion) |
| G39 causal prediction high | card | #6841 c55 |
<!-- /audit-dispositions -->
