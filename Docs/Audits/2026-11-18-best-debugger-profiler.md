# The best debugger and profiler in existence: an adversarial review (2026-11-18)

Card: #6841 c39. Revision 2. Base: dev `3c593a7f16`; dev has no debugger,
profiler or runtime change since (`git diff 3c593a7f16 fa4be89170` over
`Compiler/JetRuntime`, `Compiler/JetDebug`, `CLI/Perf` and `CLI/Debug.jet` is
empty). The probes ran on the p294 gate compiler, that commit's build, and
are committed with their runner and exact log. Follows
`2026-11-14-debugging-profiling-review.md` (the c1 review, claims 1-8, facts
M1-M13, gaps G1-G12). This review does not repeat its findings. It raises the
bar from "complete" to "beats the best peer on each axis" and says where that
bar cannot be met.

Revision 1 of this review was written the same morning and never landed.
This revision reruns every probe through a committed runner, with nothing but
`jet` on the path. It also adds the facts those runs found (F13-F17) and the
D1 bar measured on a 120,004-line image. It aligns D4, D6 and D10 with the
evaluator proposal's revision 4, which replaced the interpreter this review
first assumed with the compiler's own code run in a fork. Each of those three
verdicts weakens accordingly.

The owner's order (2026-11-18): Jet ships the best debugging and profiling tool
in existence, and needs nothing but `jet`. No lldb, gdb, Linux perf, go pprof or
otelcol on the path.

## Method

The AGENTS.md adversarial design review rule, applied once per axis:

1. **Peer.** The best tool on the axis and its best case, stated fairly, with a
   source.
2. **Jet today.** What the code at the base does, from a file read or a probe.
3. **Design.** What Jet should build to win.
4. **Lever.** The Jet-only property the win rests on: the compiler, the one
   shared runtime, the scheduler, the capability and effect chokepoint, or the
   compile-time metaprogramming kit. A design with no lever is only parity.
5. **Attack.** Hidden costs, cheating, edge cases, platforms, what it gives up.
6. **Verdict.** `holds`, `holds only within stated limits`, or `fails`, for the
   claim "the design beats the peer". A design verdict is not a claim about
   today; every axis also states today's standing.
7. **Honest limit.** Where Jet cannot beat the peer, said plainly.

The ranked plan at the end turns each surviving design into proposed #6841
criteria with jet-only proofs. New user surfaces (commands, flags, markers, Core
API, debugger verbs) carry owner ballots, filed in the lane's ballot folder; this
review names them but ratifies nothing.

## Measured facts (this review)

Every probe is committed in
[`BestDebuggerProfiler20261118/Probes/`](BestDebuggerProfiler20261118/Probes/):
the programs, the runner `run.py`, and its exact output
[`run.log`](BestDebuggerProfiler20261118/Probes/run.log) and
[`latency.log`](BestDebuggerProfiler20261118/Probes/latency.log). `python3
run.py <jet> <repo>` reruns them, and `python3 run.py <jet> <repo> latency`
reruns the D1 bar. The runner gives every command an empty PATH, runs it at
`nice 19` on CPUs 8-15 in a temporary directory, and removes machine paths
from the output. The logs were made one job at a time, `run.log` under a 4 GB
memory cap and `latency.log` under 2 GB, on a machine shared with other
builds. Programs:

- `fib.jet` is `Tests/Cases/JetPerfTracePartsCaptureRun/Fixtures/Fib/fib.jet`;
- `fib4.jet` runs the same `fib` in four tasks at once and joins them;
- `deadlock.jet` starts two tasks that each receive from the other's channel
  before sending;
- `alloc.jet` builds a 64-element list 2000 times and keeps every 100th.

Sample counts move between runs. Where a figure is a range, it covers the
runner's three runs; the log holds the last one.

| # | Fact | Source |
|---|------|--------|
| F1 | A two-task channel wait cycle hangs forever. `jet run deadlock.jet` printed nothing and was killed by `timeout 20` (exit 124). Under `jet run --observe`, `jet inspect live --attach PID --once` after 2 s shows the root blocked on a join, both `ping` tasks blocked on `channel receive`, and both channels at depth 0/1 with one receiver and no sender waiting. It names no cycle, no stack and no line. The runtime has no wait-for check: "deadlock" appears in `Compiler/JetRuntime/Source` only as an errno string. | probe; grep |
| F2 | The process-wide `ITIMER_PROF` timer (`PerfSamples.jet:4,23`) loses samples once several threads are due at once. `jet perf run -O2` kept 78-90% of the nominal 1 kHz samples for one busy thread (`fib.jet -- 34`, 42-64 ms of CPU) and 54-68% for four (`fib4.jet -- 32`, 94-98 ms). Revision 1's longer runs (`-- 38`) kept 93% and 48%. | probe; file read |
| F3 | 24-32% (one thread) and 31-37% (four threads) of the kept samples land in `jet_ir_stop_turn`, the stop poll the compiler emits at a recursive body's entry (`JetIR/Source/IR.jet:184-194`). The view reports them "outside Jet functions" instead of charging them to `fib`'s entry line. A third of the CPU profile of the simplest recursive program is unattributed. | probe |
| F4 | One shared record buffer, claimed by `sync_cas`. A sample on a second thread while the first holds it records its program counter alone, with no stack (`PerfSamples.jet:183-186`). Every sample is one `write` syscall. | file read |
| F5 | Every receipt says `overhead_status: not_measured` ("a matched unprofiled run was not captured"). No capture states its own cost. | probe |
| F6 | `jet debug --break fib.jet:6 fib.jet` with eight `c` and a `q`: 106-121 ms wall, a warm `-O0` build included. Start and step are fast on a small program. | probe |
| F7 | The console's `print` reads locals only: `p n + 1` answers "`n + 1` is not a local here" (`Native.jet:1158`). Its verbs are step, next, continue, finish, `break N`, locals, `print NAME`, backtrace, help and quit; `watch` is "Unknown command". The DAP `evaluate` route has a separate hand-written evaluator over a subset of operators; "Nothing runs in the program" (`CLI/DebugEval.jet:1-8`). Two evaluators, neither the compiler. | probe; file read |
| F8 | Program arguments never reach the debugged program: `jet debug --break fib.jet:6 fib.jet -- 20` stopped with `n = 30`, the default (`CLI/Debug.jet:39-50`). | probe; file read |
| F9 | `jet debug` always builds the source at `-O0` (`Native.jet:1067`); `jet debug -O2` is refused with E2102. A prebuilt `-O2` image of `fib` stops at `fib.jet:6` with `n = 30` and a two-frame backtrace. | probe; file read |
| F10 | `jet debug --record=r1` and `jet run --record=r1` are refused: "is not built yet" (E2104). `--replay` and `--raw-frames` take the same path (`CLI/Debug.jet:27`). Reverse execution does not exist. | probe; file read |
| F11 | A type's `debug_view` runs inside the debuggee: the debugger injects registers and a stack, calls the method, and reads back the `[DebugItem]` list (`Native.jet:262-312`, `1592-1656`). It cannot run on a core file or a recording, and a view with a bug can corrupt the program being debugged. | file read |
| F12 | Core has no zone, frame-mark or plot API. `core.log.span` is the only scoped timer; it allocates its name and needs the `Log` effect (`Core/Log/Log.jet:141`), so a pure `-[]>` function cannot be timed. | file read |
| F13 | `jet perf run alloc.jet` writes a trace with 0 allocation rows, and `jet perf run --lens=memory` is refused (E2102): dev's perf session does not know the flag and hands it to the child `jet run`, where `--lens` is only valid with `jet prove`. Allocations per line (c17) and the memory lens (c13-c16, D-PERF-MEMORY1=A) are held SUBs, not dev. | probe |
| F14 | `jet perf compare` of a `fib -- 30` capture against a `fib -- 34` capture prints whole-run wall and CPU deltas and "regression=2200bp", and still reports "Compare OK" with exit 0. | probe |
| F15 | `jet perf view --html` writes a 1320-byte page with no external `src` or `href`: a browser alone opens it. | probe |
| F16 | A `jet perf run` that fails before the program runs, on a compile error or an unknown flag, still prints a `Trace:` path. | probe |
| F17 | Nothing but `jet`: no lldb, gdb, perf, go, pprof, otelcol or rr is on the probes' PATH or on the host's. Every probe above ran that way. | probe |
| F18 | D1's bar on a large image. `Tools/Perf/DebugLatency/Main.jet --functions 20000 --runs 3 --bar 200` built a 20,000-function, 120,004-line program at `-O0` and stopped `jet debug` at its last line. Median of 3: first stop 57 ms on the prebuilt image (the 200 ms bar: met), a `next` under 1 ms (it rounds to 0), and the `-O0` build 31,533 ms. No lldb was on the path, so the lldb column is empty; c22 needs it. | `latency.log` |

## Debugger axes

### D1. Instant start and step

**Peer.** RemedyBG and the RAD Debugger: a debugger window up and stopped at the
first breakpoint in well under a second on large games, single steps with no
visible latency, because the debug info is loaded lazily and the UI never blocks
([remedybg.itch.io](https://remedybg.itch.io/remedybg),
[github.com/EpicGamesExt/raddebugger](https://github.com/EpicGamesExt/raddebugger)).
The RAD Debugger converts PDB and DWARF into its own indexed format once.

**Jet today.** F6: about 110 ms for a whole small session, build included.
F18: on a prebuilt 120,004-line image the first stop takes 57 ms and a `next`
under 1 ms, well inside R11's 200 ms bar. The source path does not meet it:
`jet debug prog.jet` builds first, and that image's `-O0` build took 31.5 s,
about 3,800 lines a second at `nice 19` on eight CPUs of a shared machine.
Line steps single-step
instructions through ptrace; a `next` over a loop-heavy line costs one ptrace
round trip per instruction.

**Design.** The compiler already emits Jet's own line and type tables; keep
them indexed by address so the first stop reads only the pages it needs. Line
steps run to a temporary breakpoint at every successor line start (from the
line table) instead of single-stepping, so `next` costs one stop, not one per
instruction.

**Lever.** The compiler writes the debug tables the debugger reads; no
conversion step, no DWARF parser on the hot path.

**Attack.** `jet debug prog.jet` includes a build; a cold build of a large
program is seconds, not milliseconds. Breakpoint-at-successor stepping is wrong
across `longjmp`-like control (Jet has none in safe code, but FFI callbacks
can). Signals during a step land on the wrong stop unless handled.

**Verdict.** Holds only within stated limits: start-to-stop on a prebuilt image
and step latency can match RemedyBG. A source path includes the compile, which
the 30M lines/s target bounds but does not remove.

**Honest limit.** A native GUI debugger's attach to a running game in under a
second is not matched until D11's grown Canvas Debug view exists. F8 and F9 are small gaps on the
same axis: program arguments must reach the debugged program, and the source
path must be able to build at the level the user ships.

**Ballot.** D-DBG-VERBS1 (it also lets `jet debug` take `jet build`'s `-O`
level).

### D2. Record, replay, reverse and "where did this value come from"

**Peer.** rr records a Linux process at low overhead and replays it
deterministically, with reverse-continue and reverse watchpoints
([USENIX ATC 2017](https://www.usenix.org/system/files/conference/atc17/atc17-o_callahan.pdf)).
Its costs: all threads run on one core while recording, and it needs a CPU
performance counter that many VMs and some CPUs lack. Pernosco builds an
omniscient database from an rr recording and answers "where did this value
come from" with one click
([pernos.co/about/dataflow](https://www.pernos.co/about/dataflow)). WinDbg TTD
records every instruction's effects into a trace, at a large slowdown and a
large trace
([learn.microsoft.com](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/time-travel-debugging-overview)).
Replay.io records a browser runtime's non-determinism and adds print
statements to the past ([docs.replay.io](https://docs.replay.io)). Undo's UDB
records multi-threaded Linux programs ([undo.io](https://undo.io)).

**Jet today.** F10: none of it. D-RUN-RECORD1=A ratified `--record=` and
`--replay=`; programs the recorder refuses stay refused until Epoch 6, and
reverse step stays deferred under D-TIMETRAVEL1=C. D-TIMETRAVEL2=B ratified
jump with the recorder, re-executing from the start with the recorded values,
and a thin `back N` over recorded events (`back 3` from event 11 is `jump 8`).
D-JREPLAY1=A's safe capture records `Time` only. #6919 plans reverse navigation
over retained write transitions and says replay "never re-executes the
program" (#6919 c3, c4), which contradicts D-TIMETRAVEL2=B's re-execution;
this review sides with re-execution.

**Design.** Record at the two places where a safe Jet program can be
non-deterministic, and nowhere else:

1. **The capability chokepoint.** Every result of `Time`, `Rand`, `IO`, `Net`,
   `FS`, `Env` and an FFI return goes into the log.
2. **The scheduler's synchronization order.** Channel operations, lock
   acquisitions, atomic read-modify-writes, spawn and join, in the order they
   happened. Safe Jet is data-race free, so this order fixes every shared read.

Replay re-executes the program against the log, with real threads; it does not
serialize them. Because the log names no instruction counts, a recording made
at `-O2` replays at `-O0` (I9 parity), where every
local is visible and every write can be tracked. Reverse execution is
checkpoint plus re-execution, as rr does it: the replayer forks a checkpoint
every N sync events, and `reverse-next` re-runs from the nearest checkpoint to
the previous line. `why x` re-runs the window with write tracking on `x`'s
place and answers with the last write's file, line, stack and the value it
replaced. One mechanism serves record, replay, reverse, `why`, the test
runner's flaky-test capture and the D-JREPLAY1 `.jreplay` artifact.

**Lever.** The capability chokepoint (every source of input is known to the
compiler), the scheduler (it owns every synchronization), the race-freedom of
safe code, and I9 tier parity (a recording is tier-independent). No peer has
all four; rr must record at the instruction level because C can race.

**Attack.**

- *Unsafe and FFI.* `#Unsafe` blocks and foreign code can write Jet memory
  without a sync event; a racy unsafe write breaks determinism. Mitigation:
  replay checks a cheap digest at each sync event and stops at the first
  divergence with its location; FFI calls that write Jet memory log the bytes
  they wrote.
- *Relaxed atomics.* A relaxed load can see either value; log its result.
- *Address-dependent behavior.* Hash iteration order keyed on addresses, or
  printing a pointer, differs between runs and between tiers. Jet's maps must
  seed from the logged `Rand`, and address printing must be logged.
- *Tier-dependent behavior.* Stack depth differs between `-O0` and `-O2`; a
  stack overflow recorded at `-O2` may not recur at `-O0`. Replay then reports
  "diverged: stack overflow at depth N in the recording" rather than lying.
- *Asynchronous stops.* A deadline or cancellation is delivered at a stop
  poll (`JetIR/Source/IR.jet:184-194`), so the poll it lands on is a
  non-deterministic input. The log must key it by a source-level position
  (loop turn and call count), not a machine counter. If `-O2` elides polls
  that `-O0` keeps, cross-tier replay of a cancellation must map positions or
  report the divergence.
- *Sync order costs.* Replay makes each thread wait for its recorded turn at
  every sync operation. Lock-heavy code replays slower than it recorded; code
  between sync operations runs at full speed on real cores.
- *Cost.* The design keeps D-TIMETRAVEL2=B's re-execution and drops #6919 c4's
  "never re-executes": the log holds inputs and order, orders of magnitude
  smaller than a write trace. A `why` over a long window costs a re-run of
  that window; checkpoints bound it.
- *What it gives up.* It cannot replay a non-Jet process, and a program that
  mostly runs in C behind FFI records only at the FFI boundary.

**Verdict.** Holds only within stated limits: for safe Jet code, the design
beats rr on parallel programs (real threads at record and replay, no counter
requirement, works in VMs) and matches Pernosco's dataflow without a trace
database. Unsafe-heavy and FFI-heavy programs fall back to divergence
detection.

**Honest limit.** Pernosco's hosted omniscient UI and TTD's instruction-exact
trace of any Windows process are broader in scope. Jet does not record
non-Jet processes.

**Ballot.** D-TIMETRAVEL3. It reopens D-TIMETRAVEL1=C (defer per-variable
history to Epoch 6), D-RUN-RECORD1=A's two deferral clauses (refused programs
wait for Epoch 6; reverse step stays deferred) and D-TIMETRAVEL2=B's event
granularity (`back N` counts events). `#Track` (D-TRACK-ORIGIN1=A) is a
compile-time origin fact and stays; `why` is its run-time counterpart.

### D3. Jet-typed values at O2, inlined frames included

**Peer.** Visual Studio and the RAD Debugger show optimized C++ well when the
PDB is good; lldb and gdb show `<optimized out>` often. Nobody shows a value the
optimizer deleted.

**Jet today.** F9: the source path is always `-O0`, and `jet debug -O2` is
refused; a prebuilt `-O2` image of `fib` stops and shows `n`. c35 owns
inlined frames and their locals at `-O2` in Jet's own engine. Typed rendering
of every Jet kind exists (`type_facts_display_every_kind`).

**Design.** c35's location lists for live values, plus D2: a value optimized
out at `-O2` is answered by replaying the recording at `-O0` to the same sync
point, labeled as recovered.

**Lever.** Tier parity: the same program at another tier is a faithful oracle.

**Attack.** Without a recording, an optimized-out value stays out. Recovery is
exact only at sync-event granularity plus re-execution inside the window.

**Verdict.** Holds only within stated limits: Jet is the only debugger that can
show a deleted value, and only when the run was recorded.

**Honest limit.** Live debugging of an unrecorded `-O2` process shows what
the location lists keep, like every peer.

### D4. Expression evaluation compiled by the real compiler

**Peer.** lldb compiles expressions with Clang and runs them in the process;
it handles templates and method calls but can crash or change the debuggee,
and it is slow to start. Visual Studio's evaluator is a separate interpreter
with its own gaps; its `, nse` suffix refuses side effects dynamically.

**Jet today.** F7: two evaluators, neither the compiler; the console has none.

**Design.** `Docs/Proposals/2026-11-18-debug-eval-native.md` (revision 4,
answering its third adversarial review; c40 keeps it unbuilt until a review
passes). The compiler checks the expression as a lambda at a position probe
inside the stopped function's own check, under the empty row `-[]>` composed
with `!Unsafe`, so an expression with an effect is refused before anything
runs. It builds the lambda at O0 with the image's other build options and
links it against an eval runtime whose allocator, locks, journey and stop
report are stubs. It then runs the result in a fork of the stopped program,
behind a seccomp filter, and reads the value back. The same path serves
`print`, `watch`, conditions, logpoint templates, DAP `evaluate`,
`debug_view`, replays (a replay stop is a live process) and dumps. `DebugEval.jet` is
deleted (I8). An interpreter of the compiler's IR was rejected: it would be a
second back end, a fourth "level" against I9, and Jet already removed
JetEval's interpreter.

**Lever.** The effect system proves the expression is side-effect free; the
check and the code are the compiler's own; the one runtime's stateful entry
points are known, so the eval link can refuse every other one by name.

**Attack.** The proposal's three reviews found real defects in each revision.
The third (P1) showed today's engine printing `xs = [3, 4]` for a list that
held `[1, 2]` and that a chained call had already taken. Its costs are
measured, not assumed:

- A fork costs page tables in proportion to the target's resident size:
  1.59-3.00 ms median at 64 MiB, and 13.74-400.11 ms at 2 GiB, depending on
  the huge-page share (the proposal's `fork_cost` probe).
- The first evaluation in a function of a large package costs a check of that
  package, 9.95 s for JetCLI, until the compiler's item reuse lands.
- O1 and O2 images evaluate no captures until a location parity test passes.

**Verdict.** Holds only within stated limits: no peer proves side-effect
freedom before running, and none shares one evaluator across live processes,
dumps and replays. The first evaluation in a large package and evaluation in
a large target lose to lldb on latency until item reuse lands and the
proposal's §8.2 bars are met.

**Honest limit.** An effectful call (`p file.read()`) is refused, where lldb
would run it. That is the price of the guarantee.

### D5. Watch and data breakpoints

**Peer.** gdb and lldb set hardware watchpoints (four on x86, via DR0-DR3);
rr adds reverse watchpoints; Visual Studio's data breakpoints are limited to
the same four slots.

**Jet today.** None in the console. Canvas's debug `watches` are local names
shown at each stop, not stops on a write. #6919 c2 plans "bounded Jet-path
watches".

**Design.** `watch player.hp` resolves the path through type facts to a place,
arms a debug register, and on hit names old value, new value and the writing
line. Beyond four, page protection on the place's page. In replay, a watch is
a write-tracking re-run (D2), so it is unbounded and reverse-capable.

**Lever.** Type facts turn a Jet path into an address and width; the replay
tier has no hardware limit.

**Attack.** A moved or reallocated value (list growth) changes address; the
watch must follow the owner path, re-arming on reallocation. Page protection is
slow on hot pages.

**Verdict.** Holds only within stated limits: live parity with gdb plus
following reallocation; replay beats every live debugger.

**Honest limit.** Live watches are four fast slots, as for everyone.

**Ballot.** D-DBG-VERBS1 (`watch`).

### D6. Conditional breakpoints and logpoints

**Peer.** Visual Studio and gdb evaluate the condition at each hit through a
debugger round trip, costing microseconds to milliseconds per hit; Live++ and
Dyninst patch code instead. Replay.io's logpoints run against the past.

**Jet today.** DAP supports conditions and logpoints through the mini
evaluator; the console has no conditional breakpoint (c36 owns it).

**Design.** The condition is compiled by D4's path. Where the compiled
condition provably writes no memory the program reads, it is placed in the
program: a patched jump at the breakpoint calls it, and only a true condition
traps. Proof is an IR scan of the placed code, which may call only runtime
entry points that the eval link's test marks "writes no memory". Its result
goes to a per-thread arena the debugger maps. Every other condition runs in a
fork per hit, and the debugger says so when the breakpoint is set (proposal
§8.1, R7). Logpoints are the same, with a formatted write and no trap. The
patcher is the one D7's hot patch uses. In replay, logpoints run over the
recording (Replay.io's feature).

**Lever.** The compiler can emit the condition for the exact frame layout it
chose, and its own IR proves what the condition writes.

**Attack.**

- Patching needs a 5-byte site; a breakpoint on a short instruction needs a
  displaced-instruction trampoline, as Dyninst builds. Optimized frames with
  values in registers need the trampoline to save and restore exactly.
- The placement rule is narrow. The proposal's `r7_scan` probe places 3 of 6
  ordinary conditions. `stats.name.len() == 5` is not placed, because
  `jet_char_len` caches the count in the string's own word.
- A condition that is not placed pays a fork per hit: a median 1.59-3.00 ms at
  64 MiB resident. That is far more than gdb's ptrace stop per hit, which
  forks nothing. The proposal's §8.2 `condition_per_hit` row measures both.

**Verdict.** Holds only within stated limits: a placed condition costs a call
per hit, where a ptrace round trip costs two context switches and several
syscalls, the order of gain Live++ and Dyninst rely on. A condition that
cannot be placed loses to gdb per hit.

**Honest limit.** A condition that allocates or calls a runtime writer is
slower per hit than gdb's, though it can never change the program.

**Ballot.** D-DBG-VERBS1 (`break … if`, `log`).

### D7. Hot patch and edit-and-continue

**Peer.** Live++ patches running C++ processes in about a second, keeping
state ([liveplusplus.tech](https://liveplusplus.tech)). Visual Studio's
edit-and-continue is narrower. Erlang hot code loading is the language-level
gold standard.

**Jet today.** `jet dev` hot swap migrates live values (D-HOTSTATE1=A).
#6919 c5 plans a checked transactional fix in `jet debug`.

**Design.** One patch engine: D-HOTSTATE1's migration rules, applied to a
native image by redirecting old function entries to new code; layout changes
migrate or refuse with the field named.

**Lever.** The compiler knows every layout and every call site; the effect
system knows whether a swapped function changed its effects.

**Attack.** Inlined copies of the old function survive the swap; the compiler
must record inlining sites and recompile callers. Functions on some task's
stack keep old code until they return.

**Verdict.** Holds only within stated limits: parity with Live++ on speed is
plausible; beating it rests on refusing unsafe layout changes rather than
corrupting state.

**Honest limit.** A change to a function inlined into many callers is not
"one function" fast.

**Ballot.** D-DBG-VERBS1 names the console spelling of #6919 c5's fix
command; the transactional rules stay #6919's.

### D8. Tasks, channels and locks, with deadlock detection

**Peer.** Go's runtime reports "all goroutines are asleep - deadlock!" only
when every goroutine blocks. Java's `ThreadMXBean.findDeadlockedThreads` finds
monitor cycles on request. Visual Studio's Parallel Stacks shows threads and
tasks.

**Jet today.** F1: a two-task cycle hangs forever with no message. `jet
inspect live --attach` on the hung process shows both tasks blocked on
`channel receive` and both channels empty with a receiver waiting, but not
that each channel's only sender is the other blocked task. The cycle is in
the data and nothing reads it.

**Design.** The scheduler records each blocked task's wait (channel, lock,
join) and the owner of what it waits on. When a task blocks, the scheduler
walks the wait-for edges from it; a cycle is a stop that names every task in
it, its wait, its line and its stack. Partial deadlocks are found, not only
global ones. The debugger's `tasks` view shows the same graph live.

**Lever.** One runtime owns every wait; no peer's runtime knows all edges
(channels have no single owner in Go).

**Attack.** A channel's "owner" is the set of tasks holding a sender; the
cycle check must treat "no live sender can ever send" as the edge. A cycle
check on every block costs a pointer walk; bound it by the cycle length. A task
blocked on I/O is never in a cycle.

**Verdict.** Holds: partial-cycle detection beats Go's global check and
Java's on-request scan.

**Honest limit.** Deadlocks through FFI-held locks are invisible.

**Ballot.** D-DBG-DEADLOCK1 (the stop is product behavior).

### D9. Post-mortem crash dumps

**Peer.** WinDbg and Visual Studio open minidumps with full types; Crashpad
writes them in production. A Linux core opened in gdb shows C types only.

**Jet today.** A stop prints `context.trace`; there is no dump file and no
way to open one.

**Design.** On a stop, the runtime writes a typed dump: every task's stack
(the faulting task first), the values reachable from those frames to a byte
budget, and the flight-recorder ring of the last events (samples, sync events,
chokepoint results). `jet debug` opens it with the same verbs, D4's evaluator
and D10's visualizers.

**Lever.** The runtime owns the stop path and knows every task; type facts
make the dump typed without the source.

**Attack.** A dump can contain secrets; it must honor the redaction facts the
log uses. Writing a dump after memory corruption may fail; the writer must use
preallocated memory.

**Verdict.** Holds only within stated limits: typed, task-aware and
flight-recorded beats a minidump; it covers Jet processes only.

**Ballot.** D-OBSERVE-FLIGHT1.

### D10. User visualizers through metaprogramming

**Peer.** Visual Studio natvis files, gdb Python pretty-printers and lldb data
formatters: a second language, maintained apart from the type.

**Jet today.** F11: `debug_view` exists in Jet and runs inside the debuggee.

**Design.** Keep the `debug_view` method. Build and run it by D4's path, in a
fork of the stopped program and never in the program itself, so it cannot
perturb the program and works on dumps and replays. The effect system
checks its purity (R8).

**Lever.** The visualizer is Jet, next to the type, checked by the same
compiler.

**Attack.** A view that walks a large structure spends the fork's CPU time;
D4's budget bounds it, and the budget's refusal names the view. It inherits
D4's limits: a view in a generic instance waits for template parameters in
DWARF, and O1 and O2 images wait for the parity test.

**Verdict.** Holds only within stated limits: same-language visualizers that
also work post-mortem, with D4's limits.

### D11. Terminal, DAP and graphical surfaces

**Peer.** RemedyBG and the RAD Debugger (native GUI), Visual Studio, VS Code
over DAP, gdb's TUI.

**Jet today.** Console and DAP, plus Canvas: `jet dev --canvas` serves a
browser view whose debug session steps, prints locals and shows a backtrace
under Jet's own native debugger, one panel at a time, with 32 watches and 32
frames as limits (`Docs/Spec/Reference/canvas-protocol.md`, "Debug session").
It has no task list, no replay controls and no profile timeline. Core also has
a native UI stack (`Core/UI`, Wayland first) and a TUI library (`Core/TUI`).

**Design.** Grow Canvas rather than add a second graphical tool: its Debug
view shows source, stack, locals, watches and tasks together, with replay
controls; a Profile view draws the trace timeline (samples, zones, waits,
I/O). Dumps, recordings and traces open there. Console and DAP stay.

**Lever.** Canvas already shares one live session between editor, preview
and debugger (D-PROGRAM-FORM1=A), so a fix made while stopped goes through
the same checked edit path as any edit.

**Attack.** A native window on `Core/UI` would be a second graphical tool
showing the same session (I8). Canvas runs in a browser, so "nothing but jet"
holds for the console and DAP, not for the window; a native Canvas client can
come later over the same documents. A half-built panel set is worse than DAP.

**Verdict.** Holds only within stated limits: RemedyBG's panel layout is
achievable in Canvas; the win is one window that edits, debugs and profiles.

**Ballot.** D-DBG-UI1.

## Profiler axes

### P1. High-rate sampling, per line, inline-aware, at O2, with measured overhead

**Peer.** Superluminal "runs at 8 kHz" out of the box, with full stacks on
every thread ([superluminal.eu docs](https://superluminal.eu/docs/documentation.html)).
Linux perf with CFI or LBR unwinding. Go 1.18 moved to per-thread
`timer_create` because process-wide `setitimer` lost samples on busy
multi-core programs
([felixge.de](https://felixge.de/2022/02/11/profiling-improvements-in-go-1.18)).

**Jet today.** F2-F5: a process-wide timer that keeps 54-68% of the nominal
samples with four busy threads, a quarter to a third of the kept samples
unattributed, stacks dropped under contention, one syscall per sample, and
no measured overhead.

**Design.**

1. Per-thread `timer_create(CLOCK_THREAD_CPUTIME_ID)` delivered with
   `SIGEV_THREAD_ID`, one timer per runtime worker thread.
2. Per-thread sample rings; a background flush writes batches.
3. Charge a sample in a compiler-emitted helper (`jet_ir_stop_turn`, stop
   polls, write barriers) to its caller's line, since the compiler knows the
   helper has no source line of its own.
4. Inline-aware attribution from the compiler's inline records.
5. Rate up to 10 kHz; the receipt states overhead measured against a matched
   unprofiled run.

CFI unwinding is c38.

**Lever.** The compiler knows its own helpers and inlining; the runtime owns
every thread, so per-thread timers need no kernel help beyond POSIX timers.

**Attack.** Thread-CPU timers miss off-CPU time (P3 covers it). Signals
interrupt syscalls; the runtime's syscalls must restart on `EINTR`. Charging
a stop poll's samples to its caller could hide the polls' own cost, so the
receipt counts them: the cost stays visible, only its line changes. 10 kHz
costs measurable time; the overhead figure must be per program, not per
benchmark. `perf_event_open` would give precise program counters and branch
records, but `perf_event_paranoid` often forbids it, so POSIX timers stay the
default and counters are an extra.

**Verdict.** Holds only within stated limits: parity with Superluminal on
rate and threads; the win is the compiler-exact attribution.

**Honest limit.** Without `perf_event_open`, no LBR or precise PC; skid
remains.

### P2. Zones, frame marks, plots and locks; zero cost when off; instrument everything

**Peer.** Tracy: nanosecond zones, frame marks, plots, lock contention and
messages, streamed live ([github.com/wolfpld/tracy](https://github.com/wolfpld/tracy)).
Jai's Iprof instruments every procedure through a metaprogram. LLVM XRay
emits patchable nop sleds and enables them at run time
([llvm.org/docs/XRay.html](https://llvm.org/docs/XRay.html)).

**Jet today.** F12: only `core.log.span`, which needs `Log` and allocates.

**Design.** A Core zone API that needs no effect (observation, like `dbg`), a
`#Profiled` marker applied through `meta.rewrite_body`, a build switch that
instruments every function, and an XRay-style patchable entry so a running
program can turn zones on.

**Lever.** The metaprogramming kit makes "instrument everything" one line; the
effect system lets zones in pure code without lying about effects.

**Attack.** An effect-free zone is a hidden channel: it must write only to
the profiler ring, and nothing the program can read may change. Two scoped
timers would break I8 (one mechanism), so `core.log.span` must write the same
ring; a log span is a zone plus a log record. "Zero cost when off" holds for
a build that compiles zones out; the attachable form costs a nop sled per
instrumented function, a few bytes and well under a nanosecond, which the
profile-overhead gauntlet (c6) must measure, not assume.

**Verdict.** Holds: Tracy's zones plus Iprof's one-line instrumentation plus
XRay's attach, in one mechanism.

**Ballot.** D-PERF-ZONES1 (serves c2, c3 and c6).

### P3. One live timeline: samples, zones, tasks, allocations, I/O and waits

**Peer.** Superluminal shows each thread's states and who unblocked a waiting
thread; Tracy shows zones, locks and messages live.

**Jet today.** Wall and CPU windows, task spans and samples in one
`.jettrace` (probe). Join waits naming the awaited task are c7, which is
open; no other wait is on the timeline.

**Design.** Every wait (channel, lock, join, sleep, I/O readiness) records its
object, its blocking stack and the task whose action ended it (the waker).
I/O comes from the capability chokepoint with bytes and target, with no
instrumentation. Off-CPU time per stack.

**Lever.** Scheduler and chokepoint: exact wake edges, not inferred from
context switches.

**Attack.** Recording every wait costs on hot channels; record into the
per-thread ring and drop with a count past budget.

**Verdict.** Holds: exact wakers beat Superluminal's inferred ones, for Jet
code.

### P4. Memory and allocation profiles, heap snapshots, diffs

**Peer.** Instruments' Allocations and Leaks track every allocation with its
stack and show generations (heap diffs between marks). Heaptrack does the same
on Linux. Chrome DevTools heap snapshots show retainer paths to a root. Go's
pprof heap profile samples allocations by stack at a fixed byte interval
([pkg.go.dev/runtime/pprof](https://pkg.go.dev/runtime/pprof)).

**Jet today.** F13: a plain `jet perf run` of an allocating loop records no
allocation rows. `jet perf compare` shows an allocation total when a trace
has one. c17's allocation counts per line are held SUBs (the codegen gate
found 9 regressions). There is no heap snapshot, no retention path and no
diff.

**Design.** Three views over one allocator: allocations per line and stack
(c17); a typed heap snapshot, where each live block carries its type from the
allocator's type facts; and a diff of two snapshots by type and allocation
site, with the shortest retention path from a root for any block.

**Lever.** One allocator in one runtime, and type facts for every block, so
the snapshot is typed without symbols or heuristics. Instruments and
Heaptrack see `malloc` sizes, not types.

**Attack.** Walking a large heap stops the program; take the snapshot in a
forked child (copy-on-write) so the parent runs on. Arena and region
allocations are one block to the allocator; the snapshot must expand them
through type facts or say it did not. Retention through FFI-held pointers is
invisible.

**Verdict.** Holds only within stated limits: typed snapshots and diffs beat
Instruments and DevTools on Jet heaps; foreign heaps are out of reach.

**Honest limit.** Instruments sees every allocation in the process, including
system frameworks. Jet sees its own allocator.

**Ballot.** D-PERF-HEAP1.

### P5. Hardware counters (VTune class)

**Peer.** Intel VTune's top-down microarchitecture analysis (TMA) splits each
hot function's cycles into retiring, bad speculation, front-end bound and
back-end bound, then drills down several levels
([VTune user guide](https://www.intel.com/content/www/us/en/docs/vtune-profiler/user-guide/current/top-down-microarchitecture-analysis-method.html)).

**Jet today.** F13: no `perf_event_open` and no `--lens=memory` on dev.
c13-c16 (held SUBs) add cache, TLB and page-fault counters per line under the
memory lens, `jet perf run --lens=memory` alone or `--lens=cpu,memory` with
CPU time (D-PERF-MEMORY1=A). Reviewing that stack found that AMD Zen 1 and
Zen 2 cores publish no LLC-miss event the lens can use, so on them LLC is
"unavailable" by c16's rule.

**Design.** On the same `perf_event_open` path as c14, TMA level 1 and level 2
per function and per line, using the CPU's published top-down events.

**Lever.** Per-line attribution through the compiler's own line table, at
`-O2`, with no symbol step.

**Attack.** Event names and formulas differ per microarchitecture; a wrong
table gives confident nonsense, so unknown models must report "unavailable".
`perf_event_paranoid` above 2, or a VM without a virtual PMU, leaves nothing to
read; c16 already requires "unavailable with a reason", never zeros.

**Verdict.** Holds only within stated limits: TMA levels 1-2 on mainstream x86
cores where the kernel allows counters.

**Honest limit.** VTune's deeper TMA levels, Intel-only events and its GPU
views are not matched.

**Ballot.** D-PERF-TOPDOWN1.

### P6. Causal profiling

**Peer.** Coz ([SOSP 2015](https://dl.acm.org/doi/10.1145/2815400.2815409),
[github.com/plasma-umass/coz](https://github.com/plasma-umass/coz)) runs
"virtual speedup" experiments: it pauses the other threads whenever one line
runs, and so predicts the whole-program gain of making that line faster. It
needs a progress point written into the source.

**Jet today.** None.

**Design.** The scheduler inserts the pauses at its own pause points;
progress points are any zone (P2), any request the capability chokepoint
answers (an HTTP response, a file write), or the end of each task.

**Lever.** The scheduler owns every thread's pauses; the chokepoint gives
progress points with no source change.

**Attack.** Coz's predictions are noisy on short runs and need many
experiments; the view must show each prediction's confidence interval or
refuse to rank. Pauses inserted only at stop polls are coarser than Coz's
signal-driven ones for a tight loop with no poll in it.

**Verdict.** Holds only within stated limits: Coz-quality predictions with
automatic progress points, on runs long enough for the experiments.

**Ballot.** D-PERF-CAUSAL1.

### P7. Static cost next to measured cost

**Peer.** None ships the compiler's predicted cost per line beside the
measured cost. `llvm-mca` predicts throughput for a snippet, apart from any
profile.

**Jet today.** #6897 (D-COSTLAW1=A) plans hidden-cost facts and a per-line
cost view (#6897 c3, c4); the profiler does not read them.

**Design.** `jet perf view`'s per-line table shows each line's cost facts
(allocations, copies, locks, dynamic dispatch, bounds checks) beside its
measured share, so "predicted cheap, measured hot" stands out.

**Lever.** The compiler's own cost facts, keyed by the same line table.

**Attack.** Predicted cost is qualitative; a predicted number beside a
percentage invites a false ranking. Show the facts, not a predicted number.

**Verdict.** Holds: no peer has the compiler's facts.

### P8. Profile diffs and regression gates

**Peer.** pprof's `-diff_base` subtracts two profiles per function and line
([github.com/google/pprof](https://github.com/google/pprof)); continuous
benchmarking gates such as Bencher fail CI on a significant slowdown.

**Jet today.** F14: `jet perf compare` prints whole-run wall, CPU and
allocation deltas and a budget line, and exits 0 even on a 22% regression
(`CLI/Perf/Command.jet:243-322`). It has no per-function or per-line diff and
no significance test. `jet budget check` is the existing gate, with a
"regressed" or "inconclusive" verdict (`CLI/BudgetMeasure.jet:520-521`).

**Design.** `jet perf compare` gains per-function and per-line deltas with a
significance verdict over repeated captures; the gate stays `jet budget
check`, which then names the function that regressed. No new command or
flag.

**Lever.** The hardware and toolchain identity check compare already does,
and stable per-line identity from the compiler.

**Attack.** One capture per side cannot support a significance test; compare
must refuse a verdict on one capture each and say so.

**Verdict.** Holds only within stated limits: pprof's diff plus a gate, on
repeated captures.

### P9. Off-CPU and wake-up analysis

**Peer.** Superluminal shows, for each wait, the thread that unblocked it.
Brendan Gregg's off-CPU flame graphs (eBPF) show blocked time by stack.

**Jet today and design.** As P3: the scheduler records each wait's blocking
stack and waker; off-CPU time per stack is the same data summed.

**Verdict.** Holds, for Jet code (P3's limits).

### P10. Always-on production profiling and attach

**Peer.** Parca and Grafana Pyroscope profile every process on a host through
eBPF at low overhead, with no restart. Go's pprof endpoint serves profiles
from any Go service that imports it.

**Jet today.** `jet perf attach` and `jet inspect live --attach PID` work on a
process started with `--observe` (F1's live view is one); c18-c20 stay open
on the card. A process started without it cannot be profiled or inspected live.

**Design.** A flight recorder: a low-rate sampling ring plus the last sync and
chokepoint events, always on in builds that keep it, drained by attach and
written into crash dumps (D9). One ring, three readers.

**Lever.** The runtime owns the ring and the stop path; attach needs no
restart and no eBPF.

**Attack.** "Always on" must show its cost: the default rate must fit c6's
budget, and the ring must be bounded memory.

**Verdict.** Holds only within stated limits: Jet processes only.

**Honest limit.** eBPF profilers see every process on the host; Jet sees Jet
processes.

**Ballot.** D-OBSERVE-FLIGHT1.

## Verdict table

"Today" is the verdict for "jet alone matches or beats the best peer", from
this review's probes and file reads. "Design" is the verdict for the design
above, after the attack. c39 requires that every axis that does not hold is an
open criterion on #6841; the last column names it (existing criteria by
number, proposed ones by plan rank).

| Axis | Best peer | Today | Design | Closed by |
|------|-----------|-------|--------|-----------|
| D1 Start and step | RemedyBG, RAD Debugger | holds only within stated limits (F18 prebuilt first stop 57 ms; the source path's build 31.5 s; F8, F9) | holds only within stated limits | c22, R11 |
| D2 Record, replay, reverse, why | rr + Pernosco, TTD, Replay.io, UDB | fails (F10) | holds only within stated limits | R1, #6919 |
| D3 Typed values at O2 | Visual Studio, RAD Debugger | fails (F9; c35 open) | holds only within stated limits | c35, R1 |
| D4 Compiled evaluation | lldb | fails (F7) | holds only within stated limits (fork cost, first check in a large package) | c40, R3 |
| D5 Watch, data breakpoints | gdb, rr | fails (F7: `watch` is unknown) | holds only within stated limits | c36, R7 |
| D6 Conditions, logpoints | Visual Studio; Live++ and Dyninst patching | fails (DAP only, mini evaluator) | holds only within stated limits (an unplaced condition forks per hit) | c36, c40, R7 |
| D7 Hot patch | Live++ | fails on native images (`jet dev` only) | holds only within stated limits | #6919 c5, R12 |
| D8 Tasks, deadlocks | Go runtime, Java ThreadMXBean | fails (F1: hangs; the live view names no cycle) | holds | R6 |
| D9 Post-mortem | WinDbg with minidumps | fails (none) | holds only within stated limits | R9 |
| D10 Visualizers | natvis, gdb and lldb formatters | holds only within stated limits (F11) | holds only within stated limits (D4's) | c40, R8 |
| D11 Surfaces | RemedyBG, VS Code | fails (Canvas debug view is one panel at a time, no timeline) | holds only within stated limits | R16 |
| Generic instances | every peer | fails (a breakpoint on a generic-module instance's member never hits: the evaluator proposal's `genmod` probe) | holds | c41 |
| P1 Sampling | Superluminal, Linux perf | fails (F2-F5) | holds only within stated limits | R2, c38 |
| P2 Zones, instrument all | Tracy, Jai Iprof, LLVM XRay | fails (F12) | holds | c3, c6, R5 |
| P3 and P9 Timeline, waits, off-CPU | Superluminal, Tracy | fails (no wait on the timeline; c7 open) | holds | c7, R4 |
| P4 Heap | Instruments, Heaptrack, DevTools | fails (F13: no allocation rows) | holds only within stated limits | c17, R10 |
| P5 Counters | VTune | fails (F13: none on dev) | holds only within stated limits (no LLC event on Zen 1 and Zen 2) | c13-c16, R15 |
| P6 Causal | Coz | fails (none) | holds only within stated limits | R13 |
| P7 Cost beside measured | none | fails (not joined) | holds | R14 |
| P8 Diffs and gates | pprof, Bencher | fails (F14: whole-run deltas, exit 0 on a regression) | holds only within stated limits | R14 |
| P10 Always-on, attach | Parca, Pyroscope | holds only within stated limits (attach needs `--observe`) | holds only within stated limits | c18-c20, R9 |
| Nothing but `jet` | (the owner's order) | holds for these probes (F17); c34 and c37 are open for the case suites and the docs | holds | c34, c37 |

No design verdict is `fails`. Each `holds only within stated limits` names its
limit in its axis section. The limits that cannot be engineered away are the
same three everywhere: Jet sees Jet processes only; unsafe and FFI code is
visible only at its boundary; and hardware limits (four watch slots, counter
availability) bind Jet as they bind every peer. Revision 2 adds a fourth, the
cost of evaluating in a fork (D4, D6, D10), which the evaluator proposal
bounds and measures but does not remove.

No axis's Today column reads a plain `holds`, so every axis needs an open
criterion, and the last column names one or more. The existing ones are on
#6841 now: c3, c6, c7, c13-c20, c22, c34-c38, c40 and c41. The R-ranks are
proposed criteria. None of R1-R16 is on the card yet (its criteria run
c1-c41). Until Main adds them, D8, D9, D11, P6 and P7 have no open criterion
on #6841 at all, and D2 and D7 have only #6919's.

## Ranked plan

Largest expected gain first: how far the step moves an axis from `fails` to
`holds`, times how many users meet that axis. R-numbers are this review's;
Main assigns #6841 criterion numbers. Every proof runs with jet alone (c34).
"Ballot" is the decision a step needs before it is built (c2); "no" means the
step adds no user surface. Existing criteria are referenced, not repeated:
c3, c6, c7, c13-c20, c22 and c34-c41 stay as written.

| Rank | Step | Proposed criterion | Proof | Ballot |
|------|------|--------------------|-------|--------|
| R1 | Time travel over the chokepoint and sync order | (a) `jet run --record=r1` of a program whose tasks share channels and a lock, made at -O2, replays under `jet debug --replay=r1` with the same output and the same sync order at -O0, -O1 and -O2, on real threads; an unsafe write that changes the run is reported at the first sync event where it diverges. (b) In a replay, `reverse-step`, `reverse-next`, `reverse-finish` and `reverse-continue` reach the previous line, call, caller and breakpoint or watch hit, in the console and over DAP `stepBack` and `reverseContinue`. (c) `why x` in a replay names the write that gave `x` its value: file:line, task, stack and the value it replaced; a local optimized out at -O2 is shown from the -O0 replay and labeled recovered. | `jet test Compiler --profile cases --filter Debug/replay_threads_across_tiers_`; `--filter Debug/replay_reverse_`; `--filter Debug/replay_why_` | D-TIMETRAVEL3 |
| R2 | A sampler that is right on threads | (a) `jet perf run` samples each thread on its own CPU clock: four tasks running fib at once at -O2 keep at least 90% of the nominal samples, and every sample keeps its whole stack. (b) A sample in a compiler-emitted helper (a stop poll, a barrier) is charged to the Jet line that called it, and the receipt counts such samples; no fib sample is reported outside Jet functions. (c) A sample in an inlined function names the inlined function's line and its caller's line, at -O2. (d) Every receipt states the capture's overhead, measured against a matched unprofiled run, and the default rate costs under 2% on fib. | `jet test Compiler --profile cases --filter Perf/every_thread_keeps_whole_stacks`; `--filter Perf/helper_samples_charge_the_calling_line`; `--filter Perf/inlined_samples_name_the_inlined_line`; `--filter Perf/receipt_reports_measured_overhead` | no |
| R3 | One evaluator: the compiler's | Every debugger expression (console `print` and `watch`, breakpoint conditions, logpoint templates, DAP `evaluate`) is checked by the compiler at a position probe in the stopped function's own check, built at O0 against the eval runtime, and run in a fork of the stopped program, as `Docs/Proposals/2026-11-18-debug-eval-native.md` specifies. Field paths, indexing, method calls and generic functions work; an expression with an effect is refused with the effect named; `DebugEval.jet`'s second evaluator is gone. Built only after c40 is met. | `jet test Compiler --profile cases --filter Debug/print_evaluates_compiled_expressions`; `--filter Debug/print_refuses_effectful_expression`; the proposal's §8.2 bars | no |
| R4 | Waits with wakers, off-CPU time, I/O | Every wait in the trace (channel send and receive, lock, join, sleep, I/O readiness) names its object, its blocking stack and the task whose action ended it; `jet perf view` lists off-CPU time per stack; file and network I/O through the capability chokepoint appear on the timeline with bytes and target and no instrumentation. Extends c7 from joins to every wait. | `jet test Compiler --profile cases --filter Perf/every_wait_names_its_waker`; `--filter Perf/off_cpu_time_by_stack`; `--filter Perf/io_on_timeline_without_instrumentation` | no |
| R5 | Zones, frame marks and plots | An effect-free zone, frame-mark and plot API and a `#Profiled` marker write one per-thread ring that `core.log.span` also writes, and line up with samples on the `jet perf view` timeline. Each zone site is a patchable no-op until `jet perf run` or `jet perf attach` turns zones on, at a cost c6 measures; a build setting strips the sites. Serves c3 and c6. | `jet test Compiler --profile cases --filter Perf/zones_frames_plots_share_the_timeline`; c3; c6 | D-PERF-ZONES1 |
| R6 | Deadlock stop | When the last task in a wait cycle (channels, locks, joins) blocks, the program stops with a diagnostic naming every task in the cycle, its wait and its line; a partial cycle is found while other tasks still run. | `jet test Compiler --profile cases --filter Tasks/wait_cycle_stops_with_the_cycle` | D-DBG-DEADLOCK1 |
| R7 | Console verbs, fast conditions, hardware watches | c36's verbs ship under the ratified spellings; `watch` uses the CPU's debug registers and stops at the changing write with old and new values; a condition that R3's scan places in the program (it writes no memory the program reads) and that is false a million times finishes in under a second, and a condition it cannot place says so when the breakpoint is set; `tasks` lists every task with state, wait and stack. | `jet test Compiler --profile cases --filter Debug/watch_stops_at_the_changing_write`; `--filter Debug/conditional_breakpoint_runs_in_process`; `--filter Debug/tasks_lists_every_task` | D-DBG-VERBS1 (serves c36) |
| R8 | Visualizers that never run in the target | A type's `debug_view` renders in `jet debug`, over DAP, in a crash dump and in a replay without running code in the target: R3's path builds it and runs it in a fork of the stopped program, or of the dump's or the replay's process. Built only after c40 is met. | `jet test Compiler --profile cases --filter Debug/debug_view_renders_without_running_the_target` | no |
| R9 | Typed crash dumps and the flight recorder | A stop writes a typed dump (every task's stack, values reachable from them to a budget, the flight-recorder ring) and `jet debug` opens it with the live verbs; `jet perf attach` drains the same ring. | `jet test Compiler --profile cases --filter Debug/crash_dump_opens_typed`; `--filter Perf/attach_drains_flight_ring` | D-OBSERVE-FLIGHT1 |
| R10 | Typed heap snapshots | Under `--lens=alloc`, a heap snapshot taken at exit, at a `perf.heap_mark` or on attach names each live block's type and allocation line; `jet perf compare` diffs two snapshots by type and line; `jet perf view --retained-by` shows a block's shortest retention path from a root. | `jet test Compiler --profile cases --filter Perf/heap_snapshot_types_retention_and_diff` | D-PERF-HEAP1 |
| R11 | Debugger start and step on large programs | Program arguments after `--` reach the debugged program (F8); `next` over a loop-heavy line costs one stop, not one per instruction; on a prebuilt 100,000-line image the first stop comes in under 200 ms (57 ms today on a 120,004-line image, F18, so this bar guards against regression). Adds the bar to c22's measurement. | `jet test Compiler --profile cases --filter Debug/program_arguments_reach_the_program`; `--filter Debug/next_over_a_loop_is_one_stop`; `Tools/Perf/DebugLatency/Main.jet --bar 200`, as `latency.log` | no (the `-O` level on `jet debug` is in D-DBG-VERBS1) |
| R12 | Native hot patch | Under `jet debug`, the fix command swaps an edited function into the running native process at its next call; a layout change migrates by D-HOTSTATE1=A's rules or is refused naming the field. Extends #6919 c5 to the native tier. | `jet test Compiler --profile cases --filter Debug/fix_swaps_function_in_native_process` | D-DBG-VERBS1 (spelling) |
| R13 | Causal profiling | `--lens=causal` predicts the whole-program gain of speeding each hot line, with automatic progress points (closed zones, answered requests, finished tasks); on the fixture, the prediction for a line later sped up by hand is within the stated interval, and a run too short to rank is refused with the reason. | `jet test Compiler --profile cases --filter Perf/causal_profile_predicts_speedup` | D-PERF-CAUSAL1 |
| R14 | Cost facts and diffs in the views | `jet perf view` shows each hot line's #6897 cost facts beside its measured share; `jet perf compare` reports per-function and per-line deltas with a significance verdict over repeated captures and refuses a verdict on one capture each. | `jet test Compiler --profile cases --filter Perf/view_shows_cost_facts_beside_samples`; `--filter Perf/compare_per_line_with_significance` | no |
| R15 | Top-down counters | `--lens=topdown` reports TMA levels 1 and 2 per function and line on supported x86 cores, on the memory lens's counter path, and "unavailable" with a reason elsewhere (as c16). | `jet test Compiler --profile cases --filter PerfRun/topdown_levels_per_function` | D-PERF-TOPDOWN1 |
| R16 | One window: Canvas grown | Canvas's Debug view shows source, stack, locals, watches and tasks together, with replay controls, and opens dumps and recordings; a Profile view draws the trace timeline (samples, zones, waits, I/O). No second graphical tool. | `jet Tools/CanvasTest/Run.jet native-debug-panels` (a new mode beside `native-lifecycle`) | D-DBG-UI1 |

### Ballots filed

Each new user surface above has an owner ballot (short mode) in the lane's
ballot folder: D-TIMETRAVEL3 (a reopen naming clauses of D-TIMETRAVEL1=C,
D-RUN-RECORD1=A and D-TIMETRAVEL2=B), D-DBG-VERBS1, D-PERF-ZONES1,
D-DBG-DEADLOCK1, D-OBSERVE-FLIGHT1, D-PERF-HEAP1, D-PERF-CAUSAL1,
D-PERF-TOPDOWN1 and D-DBG-UI1. The steps marked "no" change behavior behind
existing surfaces only.

## Finding dispositions

Main has since added R1-R16 to #6841 as c43-c58, in rank order.

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| R1 | card | #6841 c43 |
| R2 | card | #6841 c44 |
| R3 | card | #6841 c45 |
| R4 | card | #6841 c46 |
| R5 | card | #6841 c47 |
| R6 | card | #6841 c48 |
| R7 | card | #6841 c49 |
| R8 | card | #6841 c50 |
| R9 | card | #6841 c51 |
| R10 | card | #6841 c52 |
| R11 | card | #6841 c53 |
| R12 | card | #6841 c54 |
| R13 | card | #6841 c55 |
| R14 | card | #6841 c56 |
| R15 | card | #6841 c57 |
| R16 | card | #6841 c58 |
<!-- /audit-dispositions -->
