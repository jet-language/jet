# Jet observation guide

Start here when a program is slow.

This page is the terminal guide for package authors and toolchain maintainers.
It covers live runtime facts, GC evidence, wall-clock traces, browser rows, and
recorded runs. Executable truth is in `Compiler/JetFoundation/Source/Devtools/`,
`Compiler/JetDevServer/Source/LiveInspect.jet`, `Compiler/JetCLI/Source/CLI/GCReport.jet`,
and `Compiler/JetCLI/Source/CLI/Perf/`. The observation terms follow `Docs/Spec/vocabulary.md`;
an observation event is not a private payload channel.

The table is the single vocabulary for observation. Choose a surface by the
question it answers.

| Surface | Question answered | Start here | Flag or recorder | Artifact envelope and convergence |
|---|---|---|---|---|
| Live scheduler | What runs, waits, or queues now? | `jet run app.jet --observe`, then `jet inspect live --attach <pid>` (the task tree with each task's name, open log spans and arena allocations, channel depths and waiters, deadlines and effect counts, redrawn every second; `--once` prints one view), or `jet inspect structure --live <pid>` or another named inspect plane | `--observe` publishes bounded `jet.devtools.v1` facts. Use `--json` for a machine projection. Attaching to a process run without `--observe`, or owned by another user, is refused with `E2105` before anything of it is read. | The live snapshot is `jet-observe-<pid>.json` in the host temporary directory. Despite the `.json` name, it holds the runtime's raw records, which `jet inspect` decodes. It carries freshness and process identity and exposes no unpublished values. |
| GC promotions | Which allocations enter automatic memory management, and what ownership rewrite helps? | `jet run app.jet --gc-trace`, then `jet gc report` | `--gc-trace` records promotion evidence. | `.jet/gc/trace-v1.json` uses `jet.gc.trace` v1. The separate `jet.gc.report` v1 projection requires complete promotion and identity evidence and rejects dropped rows. |
| Wall-clock session | Which symbols consume wall time, CPU, task, lock, or I/O time? | `jet perf run app.jet`, then `jet perf view <trace.jettrace>` | `jet perf run`, `jet perf test`, and `jet perf attach <pid> --source app.jet` record sessions. The ratified user-verb recorder is `--record=<name>` on `run`, `dev`, and `test`. | The default artifact is `.jet/perf/<stamp>-<short-id>.jettrace`, with schema `jet.trace` v1. |
| Browser rows | Which browser, WebAssembly, or DOM rows consume time? | `jet dev --target=web`, then `jet perf attach <pid>` | The dev server sends bounded, payload-free relay rows; `jet perf attach` requests collection. | The request transport uses `jet.browser.request.v1`; `jet perf attach` merges rows into the canonical `.jettrace` rather than creating a second trace format. |
| Log records | Which modules log, at which level, and who set it? | `jet explain log`, then `JET_LOG=warn,shop.db=debug jet run .` or `jet run --log=warn,shop.db=debug .` | `--log=<filter>` on `run` and `dev` sets the top `core.log` level filter, above `JET_LOG`, the code's `set_level` and the package's `log:` field. | Records go to the `core.log` sink; each carries its `module`. The ladder is in `Docs/Spec/Reference/core-library.md` under `core.log`. |

The scheduler's internal I/O-quiescence predicate is not a census of every
host operation. It tests `active == 0 && allocated == retired` for the IOCP
owners (`Prelude/Observe.rs`, with writes confined to Windows `run_iocp` in
`Prelude/Scheduler.rs`). On Linux these three facts remain zero: the Jet
`HostIO.jet` provider therefore returns true both at idle and while a pipe
read is blocked. Linux descriptor operations must not update those facts.
`Compiler/JetRuntime/Tests/HostIOQuiescentTests.c` witnesses that distinction
using a real blocked kernel read; the host fixture runner compares exact
output and audits unique provider ownership.


## Exporting a wall-clock trace

`jet perf export <trace.jettrace> --chrome` writes Chrome Trace Event JSON.
Open it in Perfetto UI or `chrome://tracing`. Captured wall/CPU, allocation,
browser, task-span, I/O, lock, and native rows become complete (`X`) events;
process, domain, and task lanes use metadata (`M`) events, and a transfer
outside every task has its own `I/O outside tasks` lane. Unavailable states,
async flows, counters, screenshots, and source-map events remain outside this
projection.

`jet perf view` shows every profile with jet alone. In the terminal it prints
the flame graph, the timeline and, last, the per-line table: each function's
CPU-time samples at each source line, hottest first, at most 24 lines, each
with its share of every line's samples and the line's typed cost facts
(D-COSTLAW1), as `jet explain --cost` reports them
(`  main.jet:11 spin_hot_loop samples=592 share=99% · cost: exact Int spill ×2
— semantic remainder (loop depth 1, repeats every loop turn)`). A fact
describes the code that ran: the view reads them by running
`jet explain --cost` on each source the trace names whose file still holds
the digest the trace recorded. A source it can't read, or that changed since
the capture, gets a `cost facts: none, …` line above the sections instead.
A pprof or OTLP export carries no source, so its table has no facts. `--html`
prints one self-contained page of the same three, which a browser opens with
no other program and nothing to fetch.
Tests/Cases/Perf/view_shows_every_profile.case and
Tests/Cases/Perf/view_shows_cost_facts_beside_samples.case are the worked
examples.

`jet perf export <trace.jettrace> --pprof` writes pprof's gzip-compressed
`profile.proto` beside the trace (`<stem>.pb.gz`, or `--out <path>`). Each
CPU-time profile row is one sample whose stack is one location, the row's
function at its source line, valued by the row's own samples; the period is
one millisecond of CPU. Inclusive samples, allocations, counter events and
full stacks are not carried, and the profile's comment says so. `--otel`
prints OTLP/JSON, one export request a line as a collector's file exporter
writes them: an `ExportTraceServiceRequest` (core.log `otlp_traces_json`) of
the trace's captured spans and browser rows, then an
`ExportMetricsServiceRequest` of the per-line totals, the cumulative sum
`jet.profile.samples` with a point for each row named by its
`code.file.path`, `code.line.number` and `code.function.name`. Its trace id
is the first 32 hex digits of the trace's id. A task span's parent is its
parent task's span, and each span's kind and task, and a wait's awaited task,
spawn line, channel, socket addresses, stack and waker, are `jet.*`
attributes. Span times count from the session's start, since a trace records
no wall-clock origin.
`--out <path>` writes any export to a file instead of stdout.

Both exports are interop for other tools; jet reads them back itself.
`jet perf view <profile.pb.gz>` (or `.pb`) and `jet perf view
<export.json>` print `export pprof <path>` or `export otlp <path>`, what the
export says of itself (a pprof comment, an OTLP span count), then the flame
graph, the export's spans on the timeline and the per-line table, which
matches the trace's own view line for line; `--html` works on them too, and
`--json` stays the trace's own view. Tests/Cases/Perf/export_round_trip.case
is the worked example.

The canonical trace wrapper is `jet.trace` version 1, verified by
`Compiler/JetFoundation/Source/Trace/JetTrace.jet`. A trace's identity, toolchain, and
hardware facts stay bound to that artifact; `jet perf view`, `compare`,
`attach`, and `export` use the same verification seam.

A session's `profile` record carries the sampling rows and the run window. The
run window is the runtime's own account of the run: the wall time and process
CPU since the Jet runtime started, which leaves out any in-process compile
that the session `wall`/`cpu` samples include. `window_status` is `exit` when
the runtime published the window at its shared exit seam, `live` for the last
periodic publication of a run that had not exited, and `unavailable` when no
window was published. On Linux, `jet perf run` reads the exit publication
before it reaps the `jet` child, so a completed run that took jet's place
records the same profile rows, in the same order and with the same source
ranges, on every capture. The `jet` host's own publications describe its
compile and are never attributed to the program. A program that `jet run`
starts in a child process is reaped by jet first, so its window is the last
periodic one (`live`). A program that links no task, channel, live value or
event publishes nothing: its trace keeps the source identity and map, and its
profile is `no_samples` with no window.
`jet perf run` samples the program's CPU time: the runtime arms a timer on
each thread's own CPU clock (`timer_create` on `CLOCK_THREAD_CPUTIME_ID`,
delivered to that thread, one sample per 125 microseconds of its CPU, 8 kHz),
so threads that run at once each keep their samples, and records the program
counter each `SIGPROF` interrupted with the call site of each caller, each
handler in a record buffer of its own so no sample loses its stack to another
thread's.
A frame without a frame pointer (code before a function's prologue sets one,
a compiler-emitted helper such as the stop poll, a frameless runtime
routine, a C library function) is stepped out of by the image's own call
frame information (`.eh_frame`), read by the runtime's unwinder, up to the
first frame that keeps one; the frame-pointer chain gives the rest. At exit
the runtime records the CPU time it sampled, and `jet run` writes the
program's image beside the samples. The session names each sample's Jet
function and source line from that image's `.debug_jetline` location table, as
Linux perf names the same image's samples from its line tables, and code
the optimizer inlined from the image's DWARF inlined subroutines: a sample in
`-O2` code inlined into a caller counts at the inlined function's line under
that function's name, and inclusive at each call it was inlined for, as the
debugger's inlined frames show it; a line of an inlined function from another
file counts outside the checked source. A caller's frame is looked up at its
call, the byte before its return address. The rows are `cpu` rows, one per
function and line of the checked
source, hottest first. A row's `sample_count` is its exclusive time, the
samples whose program counter is on it; its `inclusive_count` is its inclusive
time, the samples that hold it anywhere on the stack, each once however often
(a recursive call's line counts once per sample), so a line the stack only
passes through has a row with no exclusive samples. A row's
`allocation_count` is the allocations whose stack holds its line innermost
of the checked source's lines, from the allocation records the runtime
writes at exit under the `alloc` lens, so an allocation in the runtime or a
core module counts at the line of the source that called into it; a run
without that lens leaves it null. A sample in a compiler-emitted
helper (a `jet_ir_` symbol, such as the stop poll `jet_ir_stop_turn`) counts
at the Jet line that called it, and the `reason` counts those samples.
`coverage` is `complete` when every sample landed in that source and
`partial` when some ran in the runtime or another file, which the `reason`
counts and, for samples outside every Jet function, names by the image's
`.symtab` function, hottest first, as `perf report` names them. The `reason`
also gives how many samples the CPU time sampled should give beside how many
the session kept, and counts any sample in Jet code that kept no stack. The
profile's overhead is `measured`: `overhead_ns` is the time the sampler's
handlers ran plus, for each sample's signal, the kernel's delivery and
return, which the runtime times as sampling starts by sending its own thread
64 signals; `overhead_reason` gives it as a share of the CPU time sampled.
`jet perf view` sums the rows per function with its share of the
samples, then its hottest lines with their exclusive and inclusive samples and
allocations (`line 7 samples=12 inclusive=40 allocations=300`). A run whose
samples never reach the checked source (a short program, `jet perf test`, `jet
perf attach`) keeps the observation rows, which attribute to the parsed `run:
fn` declaration (`fn` through its closing brace) and do not separate the
functions that `run` calls; their `inclusive_count` and `allocation_count` are
null.

Unless the run is a causal experiment or a flight recording, each thread
samples by a perf event of its own, written to a ring its `SIGPROF` reports,
where `perf_event_paranoid` allows one. At 1 or below, on a host with a
precise sampler, the runtime charges each sample by the instruction that
spent the time: AMD's IBS op sampling (`perf_event_open` on the `ibs_op` PMU,
filtered to user mode in software), else Intel's PEBS (cycles at
`precise_ip` 2), each once per a period in cycles, the cycles the main thread
ran in 125 microseconds of its CPU time as sampling started, so their rate
follows the clock speed the cores run at. At 2, or where neither opens, the
task clock (`PERF_COUNT_SW_TASK_CLOCK`) samples each thread exactly once per
125 microseconds of its CPU time in user mode, at the program counter its
interrupt landed on, a few instructions past the one that spent the time (its
skid); time in the kernel takes no sample. Above 2, or where the kernel
refuses the task clock, each thread keeps its CPU-time timer, whose `SIGPROF`
the kernel checks once a tick, so each thread takes at most one sample a tick,
at the program counter the signal interrupted. The callers still come from
the interrupted stack, which the skid can leave in another function: the
sample keeps the callers of the innermost interrupted frame in its own
function, and when no frame is (the function returned before the signal
came), the interrupted frame is its caller. The main thread arms the first
source the kernel accepts, and each thread after it arms that source. The
receipt's `capture.sampling.pc` names the source, and the profile's `reason`
says which took the samples: the rate a precise source reached, or, for the
timer, its cap and why the run kept it.
Tests/Cases/Perf/precise_pc_names_its_source.case is the worked example,
Tests/Cases/Perf/task_clock_keeps_eight_khz.case shows four threads keep
the task clock's 8 kHz, and
Tests/Cases/Perf/timer_fallback_states_its_cap_and_why.case shows the timer a
causal experiment keeps.

`jet perf run` takes `--lens=<lenses>`, comma-separated: `cpu`, the
program's CPU time and the default, `memory`, its memory behavior,
`alloc`, its live heap, and `topdown`, why its hot lines are slow on the
CPU; `--lens=cpu,memory,alloc,topdown` takes all four in one run. `jet perf
test` takes only `cpu`, and a word that names no lens is refused with the
four it may name. Under `memory` the
runtime opens four counters of the program's main thread
with `perf_event_open`, in user mode only: L1D read misses (one sample per
10007 events), LLC and dTLB read misses (one per 1009) and page faults (one
per fault). The kernel has no generic LLC read-miss event for AMD's cores; on
AMD family 19h and later (Zen 3, 4 and 5) the LLC counter opens the core's
event 0x43 with unit mask 0xFC instead, the demand data-cache fills from
another CCX's cache or from DRAM, each a demand read that missed the L3.
Lines the hardware prefetcher fetched ahead of the reads count in neither
event. Each counter's overflow signals that thread, and the handler
records the program counter and the Jet call sites as it does for a CPU-time
sample, marked with the counter. The profile's `method` stays `sampling`,
and its `counters` lists the four in that order, each `captured` with its
`period` or `unavailable` with the `errno` the kernel refused it with (0
when the program opened none); without the memory lens it lists none. Under
`memory` alone the runtime arms no profiling timer, so no row holds a
CPU-time sample, only its counters' events, and the profile's `reason`
says so. A counter's sample counts its period of events at the
innermost line of the checked source on its stack, into the row's
`l1d_miss_count`, `llc_miss_count`, `dtlb_miss_count` or `page_fault_count`;
every per-line row has the count of each captured counter, and no row has a
count of an unavailable one, not even zero, so a host without the counters
(a VM, a CPU without the event, a kernel that forbids `perf_event_open`)
reports them unavailable and the `reason` says why. Events of other threads
are not counted. `jet perf view` prints `counter l1d_misses captured
period=10007` or `counter l1d_misses unavailable errno=2` per counter and
adds each count to a line (`line 30 samples=40 inclusive=40 allocations=0
l1d_misses=1600000`).
It then prints the memory table: every function and source line that counted
events, as `path:line function` and the four counts, with the lines most
served from RAM first, then the most L1D misses, at most 16 lines
(`  walk.jet:25 strided l1d_misses=1600000 llc_misses=9081 dtlb_misses=0
page_faults=0`). Its first line says what each count means: `l1d_misses`
left L1D, `llc_misses` were served from RAM, and `page_faults` fell to the
page cache or disk. A counter the profile did not capture shows
`unavailable`. Tests/Run/Perf/CacheVisualization is the worked example.

Under `causal`, which runs alone (with any other lens it is refused, as its
delays would skew their readings), the run answers a different question:
which line, if it were faster, would make the program finish its work
sooner. The runtime's CPU-time sampler runs a series of 25 ms experiments.
Half of them, at random, are baselines. Each other experiment takes the
program's own source line of the sample that starts it and speeds it up
virtually by 30%: each sample that lands in the line adds 30% of the time
the sample stands for to its thread's count of delays. The global delay
is the most any thread has counted, as in Coz, and every thread that has
counted less pauses at its next sample until it has paid up to it. The
line therefore runs as if it were 30% faster relative to the rest of the
program, and threads in the line at once never pay each other's delays.
A pause spins and keeps its CPU, where Coz's sleeps: with more threads
ready than CPUs, a sleeping thread's CPU would go to the line's own
threads, which would then really run faster and inflate the prediction.
The samples a pause's CPU time sets off are dropped, and their handling,
like the pause's own clock reads, counts as time the pause paid: left out,
every pause would slow the program by more than its delay, and every
line's prediction would read lower, the more so the more often threads
pause. A sample
stands for its period of CPU time, or, when its thread was ready to run
since its previous sample and that took longer, the wall time between
them: under load a thread waits for a CPU between samples, and a faster
line would save that wait too. A thread that was blocked (its last
sample is more than 5 ms old and it has since given up its CPU of its own
accord, not merely been preempted) owes nothing for the time it was blocked.
A task's start forgives its worker the delays before it, and a task's end
pays its own before its joiner sees it. A thread another thread wakes from
a park owes nothing for the delays inserted while it was parked, as in
Coz: its waker paid them. Charged them, a thread that joins short tasks
would pause for them again on the program's critical path. Other delays
are paid at sampler ticks, so a thread pays its debt up to one
sample period late. A progress point counts passes: each closed zone by
its name, each finished task as `task`, each answered HTTP request as
`request`, and each `core.perf.progress(name)`. Outside the lens these record nothing. At the
end of each experiment the runtime writes its line, its wall time, the
delay it inserted, the run time and the runqueue wait its threads took
(from each one's `/proc/self/task/<tid>/schedstat`), the highest and the
lowest clock of the CPUs that the threads which ran last ran on (field 39
of each one's `stat`, then that CPU's `cpufreq/scaling_cur_freq`) and, for
each point, its passes after its first one, the wall time from that first
pass to its last and the delay inserted between them to the samples file. An
experiment whose line the checked source does not name counts neither as
a baseline nor toward any line. The trace
keeps the predictions in its `causal` receipt section (type
`JetPerfCausal`). For each point, a line's prediction is how much sooner
each pass comes, 1 - (baseline rate / line rate), where a rate counts the
passes after each experiment's first over the time from its first pass to
its last, less the delay inserted meanwhile. Only that time counts because
an experiment's line is the one the sample that starts it landed in, and
which line runs depends on how far into a pass the program is: counted
over the whole experiment, a line that tends to start just before a pass
would get that pass nearly free. Its interval is the
90% range of 200 deterministic bootstrap resamples. A line is ranked only
with 6 experiments on it, 6 baselines and an interval no wider than 30
points; the others are listed with the reason. When no line can be ranked,
the section says why the run was refused and how long to run. The section
keeps the threads' run time and wait (`cpu_run_ms`, `cpu_wait_ms`). When
they waited for a CPU more than 20% of the time they ran, the gains are
withheld: under that contention a line made virtually faster frees no CPU
for the rest, as a really faster one would, so every gain would read low.
The run is refused with the wait, each point keeps its name with no lines,
and the reason asks for a run where each thread has a CPU. The section
also keeps the means over the experiments of the highest and the lowest
clock (`clock_high_mhz`, `clock_low_mhz`; 0 where the kernel has no
cpufreq). A CPU that idled clocks down, and a task begun on it runs slower
than its siblings until it clocks up, whichever line the experiment speeds
up; a line really faster would leave the other threads idle less between
tasks, and their tasks would run faster, which a virtual speedup cannot
show. So the runtime also keeps an idle table: for up to 8 task bodies, and
for each octave of the time the thread had idled since its last task ended
(under 1 µs to over 16 ms), the tasks begun after such a gap, their run time
and their gaps. It counts only tasks during which no delay was inserted, reusing the clock read
the task's start and end already take, so a task pays a few stores, a row
lookup and three atomic adds, and nothing while the lens is off
(`perf_causal_state` is 0). The exit writes it to the samples file. The
section fits it (`clock_fit`): each body's run times in thousandths of its
own mean, so long and short tasks share the octaves, and each octave of 30
tasks or more a point weighted by its tasks, a line of slowness against
the log of the gap, with its slope, R² and each octave's residual. With at
least 3 such octaves and an R² of 50% or more, each gain and each resample
is warmed by it: a line that makes a pass sooner by the gain shortens each
octave's gap by that share of its gap and run, the fit gives how much
faster its tasks then run, and the pass's time shrinks by their run-weighted
mean. A fit that does not hold leaves the gains as they are, and its
reason is kept. When the lowest clock falls more than 10% below the highest
and the fit does not hold, the gains are kept but flagged (`clock_flag`,
`gains may under-read: idle CPUs clocked down …`, naming why the fit
failed). `jet perf view` prints the flag as `flagged: …`
before the points, and prints, under `causal`, each point as `` `round`: if this line were
30% faster, each pass would come sooner by``, then its lines best first
(`  main.jet:23 narrow  +14.2% (9.8% to 18.0%)`). Experiments are
statistical: a short run, or a line the sampler rarely lands in, gets a
wide interval and is not ranked.
Tests/Cases/Perf/causal_profile_predicts_speedup.case is the worked
example.

Under `alloc` the session builds the program with `--checked-alloc`, whose
heap keeps each block's ten allocating call sites in its prefix, and the
runtime links every block allocated from the program's entry into a live
list until it is freed. The blocks the runtime and the observation take
before the entry, those of the observation's writer thread, and the rows the
observation keeps for the program's tasks and channels (a task's label and
its wait's name among them), are their own, not the program's: they stay off
the list, and the lens counts none of them. A run without the lens
tracks nothing and pays nothing. `core.perf.heap_mark(name)` takes a heap
snapshot named `name`, and the runtime takes one named `exit` as the
program exits; in any other run `heap_mark` does nothing. A snapshot holds
the live list while it forks: the copy-on-write child walks the list as it
stood, groups the live blocks by their call sites, writes the groups to the
samples file and exits, while the program goes on once the child has the
copy and waits for it to exit. The trace keeps the snapshots in its `heap`
receipt section (type `JetPerfHeapSnapshots`): the checked source's
`path`, then each
snapshot in the order the program took them with its `name`, `number`,
live `blocks` and their `bytes`, and its `sites`, the blocks by the
`function` and `line` of the checked source innermost on their allocating
stack and the `type` they were allocated for, the most bytes first, with a
null `function` and `line` for the blocks no line of it allocated. The
`--checked-alloc` build records, as a DWARF `DW_TAG_call_site` of its
function, the type each allocating call allocates for: a list's buffer
as the list type, a struct's or a box's storage as its type. A block
takes the type of the innermost such call on its allocating stack, so a
block the runtime allocates for a typed call is that call's; a block no
typed call allocated has a null `type`. `jet perf view` prints each
snapshot after the per-line table, `live at filled  101 blocks  57344
bytes` and then each site, `  [Int]  leak.jet:14 run  100 blocks  56000
bytes`, or `  elsewhere  1 blocks  1344 bytes`; a name the run marked
more than once is shown with its count, `after-cleanup#3`.

Under `alloc` the same take that links a block onto the live list also
counts its allocation and its bytes by its ten allocating call sites, under
the same lock; there is no second allocation hook, and a run without the
lens neither links nor counts. At exit the runtime writes one record per
distinct call chain, so each row's `allocation_count` counts the line's
allocations even with no CPU-time sample (the lens alone takes none, and
the profile's `reason` says so). The trace's `allocations` receipt section
(type `JetPerfAllocations`) holds every allocation the program made from
its entry: the checked source's `path`, the `allocations` and their
`bytes`, and its `sites`, the allocations by `function`, `line` and `type`
as a snapshot's sites are, the most bytes first. Counting costs: the
runtime times each counted allocation from before it waits for the lock
until its count is done, and `overhead_ns` is the sum, `overhead_status`
`measured` and `overhead_reason` the time per allocation and its share of
the CPU time. `jet perf view` prints them before the snapshots, `allocated
<n> allocations  <n> bytes: <the cost>` and then each site, `  [Int]
alloc.jet:8 build  <n> allocations  <n> bytes`.

`core.perf.zone(name)` opens a zone that closes when the block holding it
ends (`_z: perf.zone("broadphase")`), `core.perf.frame()` marks the end of a
frame and `core.perf.plot(name, value)` records a number. Under `jet perf
run` each writes one row of the timeline that closed `core.log` spans write,
on the same monotonic clock as the samples: the trace's `spans` keep them as
`zone`, `frame_mark` and `plot` rows beside the `log_span` rows, a plot's
`symbol.name` reading `moved = 499`. Each CPU-time sample's record ends in
its time on that clock and its task, so a zone holds the samples its task
took between its open and its close, nested zones too. `jet perf view` lists
each zone name heaviest first with those samples and the source lines most
of them were on (`zone solve count=5 samples=41 total=81234ns
longest=20311ns · main.jet:12 38 · main.jet:11 3`), the frames' mean and
longest interval and each plot's last value; its timeline shows each zone
row that holds samples with how many and its hottest line
(`[2100..9800] main.jet#solve · 9 samples · main.jet:12`). The `zones`
receipt section keeps both: per zone name `samples` and `lines`, and per
sampled zone row of the trace `sampled`. Canvas's profile view draws the
zones on its zones lane. Outside a perf run a zone costs a call and a
compare at its open and a call at its close; frame marks and plots cost a
call each.

`jet perf compare base.jettrace head.jettrace` sets the last heap snapshot
of each trace against each other when both kept one, and
`--heap=<base>..<head>` names the two: the base trace's snapshot `<base>`
and the head trace's `<head>`, both of one trace when it names only one
(`jet perf compare run.jettrace --heap=start..filled`). A name alone picks
the last snapshot of that name, and `name#k` its k-th; a name the trace
never kept is refused with the names it kept. Compare prints `Heap start
-> filled: blocks 1->101 · bytes 1344->57344` and then each type and place
whose live blocks or bytes changed, signed, the most bytes gained first,
`  [Int]  leak.jet:14 run  +100 blocks  +56000 bytes`; its receipt deltas leave
the `heap` section out.

Each snapshot also records what retains its blocks. The child sorts the
live blocks by address and writes, for each, every word of it that points
into another live block, then the roots: the frames of the thread that
took the snapshot, each as its call site, frame pointer and lowest
address, the words of those frames that point into a live block, and the
words of the image's writable data that do (with the live list's own head
cleared). `jet perf` walks from the roots, breadth first, the variables
first, then the data objects of the image's symbol table, then any other
word, so each block takes a shortest path from a root. The path is named
by the image's DWARF: a frame's word by the variable whose location holds
it at the frame's call site and the field of it, else `<function> frame`;
a data word by its object; each block on the way by its type, a list's
buffer as the element's index and field (`kept[17]`, `rooms[3].history`),
any other block as ` -> <type>.<field>`. A path names at most 16 holders,
nearest the block, and starts with `…` past them. The `heap` section keeps
the snapshot's `retained` blocks, typed blocks only, by `type` and path
shape (the path with its list indices left out): one example `path`,
`blocks` and `bytes`, at most four paths a type with the most bytes, the
rest under the path `…`, and a null `path` for the blocks no root
reached, which only another thread's frames or a register held.
`jet perf view run.jettrace --retained-by <type>` prints, for each snapshot
that keeps live blocks of the type (its whole name, or the part after its
last `.`), `retained [Int] at filled  100 blocks  56000 bytes` and then each
path, `  kept[99]  100 blocks  56000 bytes`; a type no snapshot keeps is
refused with the types they keep.
Tests/Cases/PerfRun/alloc_lens_heap_snapshots.case and
Tests/Cases/Perf/heap_snapshot_types_retention_and_diff.case are the
worked examples.

Under `topdown` the runtime splits the program's issue slots by the
top-down method: each cycle the core can issue a fixed number of
operations (its slots), and each slot is retiring (useful work), bad
speculation (work issued and thrown away), front-end bound (no operation
to issue) or back-end bound (an operation that waits on data or an
execution unit), the level 1; level 2 splits retiring into light and heavy
(microcoded) operations, bad speculation into branch mispredicts and
machine clears, front-end bound into latency and bandwidth and back-end
bound into memory and core bound. The runtime reads the CPU's model with
`cpuid` and takes the formulas it has for it: AMD Zen 4 (six slots a
cycle) and Zen 5 (eight), from the `de_no_dispatch_per_slot`,
`de_src_op_disp`, `ex_ret_ops` and `ex_no_retire` events; Intel's Golden
Cove cores alone (Sapphire and Emerald Rapids), from the `topdown.*_slots`
events; Gracemont alone (Alder Lake-N), from `topdown_*_bound`; and the
hybrid Alder and Raptor Lake, Golden Cove's events on the `cpu_core` PMU
and Gracemont's on `cpu_atom`, each counting while the thread runs on that
core type. It opens each event on the program's main thread in user mode,
with its count's enabled and running times and a sampling period, and the
counter handler records each sample's program counter as a single word
marked with the event. It writes the formulas' set, the vendor, CPUID.1
EAX, whether a hypervisor runs it and the events' count to
`<samples>.topdown`, then each event's PMU type, config, period (0 for one
the kernel refused) and errno, and at exit each event's count, enabled
and running nanoseconds. The trace keeps the result as its `topdown`
receipt section (type `JetPerfTopdown`): the checked source's `path`, the
`cpu` (`vendor`, `family`, `model`, `name`, `hypervisor`), the `unit`
(hundredths of a percent of the slots), the `sample_floor` and, per core
type in `cores`, its `label`, `core`, `slot_width`, the `formulas` it
applied, its `events` (`name`, `pmu_type`, `config`, `period`, `errno`,
`count`, `enabled_ns`, `running_ns`, `samples`), the `run`'s split from
the exact counts, each scaled by its enabled time over its running time,
and its `functions` and `lines`, at most 64 each, the most slots first.
A row's split comes from its samples: each event's samples at the row's
lines times the event's period, scaled the same way. Each row holds the
level 1 and level 2 shares (`retiring`, `bad_speculation`,
`frontend_bound`, `backend_bound`, `light_operations`, `heavy_operations`,
`branch_mispredicts`, `machine_clears`, `frontend_latency`,
`frontend_bandwidth`, `memory_bound`, `core_bound`, and on AMD
`smt_contention`, the slots a sibling thread took), its `time` (its share
of the run's slots), its `samples` of the slots event and
`low_confidence` when those are fewer than 20; a line's row has its
`line` too. A CPU the lens has no formulas for, a CPU under a hypervisor,
an event the kernel refused and a run that never counted make the section
(or the core type) `unavailable`, with the `reason`. `jet perf view`
prints, per core type, `Why hot functions are slow (AMD Zen 4, level 1)`
and `Why hot lines are slow (AMD Zen 4, level 1)` with each row's time,
retiring, bad spec, front end and back end, then `Level 2 of those lines
(AMD Zen 4)`, ten rows each, a low-confidence row's name ending in `~`;
or `topdown unavailable: <reason>`. Only the program's starting thread is
counted. Tests/Cases/PerfRun/topdown_levels_per_function.case and
Tests/Cases/PerfRun/topdown_unavailable_names_the_refusal.case are the
worked examples.

`jet perf run` records every wait of a task too, and what ended it. When a
task spawns another, the runtime writes the new task's id with the Jet call
sites of the spawning stack. When a task parks (a join, a channel's receive
or send, a sleep, an I/O wait) or blocks on a `Shared` value's lock, the
runtime writes the waiting task, the wait's name as the task's state shows
it, what it waits for (the awaited task or the channel), its start and end
on the monotonic clock and the Jet call sites of its stack. Whatever wakes a
parked task writes a wake: the waking task and the call sites of its stack,
or the runtime outside every task. A lock's release writes a wake only while
a recorded wait is blocked on some lock, so an uncontended lock costs
nothing more. Each wait becomes a `task_wait` row of the trace's `spans`.
Its `task_id` is the waiting task, by the id its `tasks` row carries, its
symbol's name the wait's name (`task join`, `channel receive`, `time sleep`,
`shared lock`, ...) and its `wait_stack` the functions and lines of the
checked source on its stack, innermost first, so the first is the line it
waited at. A join's `awaited_task_id` is the awaited task and its
`spawn_line` the line that spawned it; a channel wait's `channel_id` is the
channel's. A socket's wait for readiness (an accept, a connect, a read or a
write) names the socket by its own address, `local_address`, and its peer's,
`peer_address`, as the kernel's `getsockname` and `getpeername` give them
(`127.0.0.1:4000`, `[::1]:4000`, a Unix socket's path), either null when the
socket has none. Its `waker` is `task` when a task's wake ended it, with that
task's `waker_task_id` and the line it woke it from as `waker_line`,
`runtime` when the runtime's did, and `timeout` when no wake did: a sleep,
or a wait that gave up. The runtime's I/O poller wakes a socket's wait for
what happened at the connection's other end, so `jet perf run` names the
task of the program that acted there: for a connected socket's wait, the
last transfer of another task on the socket at the other end (whose
addresses are the wait's, swapped) begun by the wait's end; for a listener's
accept, the last connect of another task to the listener's address. That
task is the `task` waker, at the transfer's or the connect's line. With no
such task the peer is outside the program and `waker` is `peer`, which names
no task or line. The wake that ended a wait is the last one on the
same park slot at or before its end, and after the task's previous wait on
that slot. Its start and end are measured from the session's start, on the
same clock as the task and log spans, so the wait lines up with them with no
manual alignment. A wait of a task no observation saw has no task id to name
and records no row. Each record is one write; a run that waits a million
times writes a million records. `jet perf view` prints `waits count=3 ·
longest 400212345ns: task 1 in task join at main.jet:5 for task 2 spawned at
main.jet:4, woken by task 2`. Its timeline shows up to 8 waits as
`[start..end] task 1 in channel receive at main.jet:11 for channel 1, woken
by task 2 at main.jet:28`, beside the task spans; a socket's as `task 1 in
tcp read at main.jet:15 on socket 127.0.0.1:4000 with peer 127.0.0.1:51000,
woken by task 2 at main.jet:25`, or `, woken by its peer`.
Tests/Cases/Perf/every_wait_names_its_waker.case and
Tests/Cases/Perf/socket_wait_names_its_peer.case are the worked examples.

The same waits give off-CPU time by stack, as an off-CPU flame graph does.
`jet perf view` sums the waits' time by stack, outermost frame first, ending
with the wait the stack blocked in, and lists the 16 longest under `off-cpu
by stack`: `150212345ns waits=1 · run main.jet:5 > pause main.jet:12 > nap
main.jet:10 > time sleep`. A stack holds the checked source's frames only,
as the CPU-time profile's lines do; a wait whose stack holds none counts
under its name alone. Tests/Cases/Perf/off_cpu_time_by_stack.case is the
worked example.

`jet perf run` puts the program's file and network I/O on the timeline too,
with no instrumentation in the program. Each file and socket transfer passes
through the runtime's I/O leaves after the effect chokepoint admitted its
path or host, and while sampling the leaf that makes the kernel's read or
write records it: the task, read or write, its start and end on the
monotonic clock, the bytes it moved or the errno that refused it, its target
and the Jet call sites of its stack. A file's target is the path the
program opened it by (`fd 3` for a descriptor the runtime holds no path
for); a socket's is its peer's address, as the kernel's `getpeername` gives
it (`127.0.0.1:4000`, `[::1]:4000`, a Unix socket's path, `@name` for an
abstract one). A try the kernel turned away as not ready or interrupted
moved nothing and records nothing; the task's wait for readiness is its own
`task_wait` row. Each transfer becomes a row of the trace's `io`: `kind`
`file` or `network`, `operation` `read` or `write`, `bytes`, `error` (null,
or the errno, with 0 bytes), `target`, `task_id` (null outside every
observed task), its `stack` the functions and lines of the checked source,
innermost first, and its symbol's name `read notes.txt`. At most 4096 rows
are kept (`io_rows_truncated`). `jet perf view` prints `io count=4
read_bytes=15 write_bytes=15 errors=0 · main.jet#write notes.txt`, and its
timeline shows up to 8 transfers as `[start..end] task 1 wrote 11 bytes to
file notes.txt at main.jet:11` or `task 1 read 4 bytes from peer
127.0.0.1:4000 at main.jet:18`. The leaves recorded are a whole file's read
or write (`core.files.read`, `write`, `append`), a file or pipe stream's
reads and writes, and a stream or datagram socket's sends and receives (TCP,
Unix sockets, UDP, DNS). A buffered file writer's writes are recorded when
its buffer reaches the kernel, with the flushing stack; a read its buffer
serves records nothing. SQLite's page reads and writes, file copies and
directory walks, raw descriptors and the terminal are not recorded. An
attach records the transfers its flight ring holds (below). Tests/Cases/Perf/io_on_timeline_without_instrumentation.case
is the worked example.

A `core.game` run under `jet run --observe` relays its frames through its live
snapshots: each frame callback records the frame's index, its start and wall
time on the monotonic clock and its code's function, as the program's location
table names it (`name`, file and the line its code starts at), and each frame
then records its scene, its index, its CPU time and how many callbacks it ran.
The runtime keeps the latest 512 records, each with a sequence number.
`jet perf run` merges every observation's records and `jet perf attach` reads
the one it takes; a record two observations hold must be the same record. Each
frame becomes a `game_frames` row and each of its callbacks a
`game_draw_events` row (`phase` `frame_callback`, `domain` `cpu`), keyed by the
frame's sequence and carrying its scene, index, build, revision and trace id;
the frame's `responsible_function` is its slowest callback, and a callback in
the checked source has that source's identity. A callback whose frame no
observation holds, or whose code no function names, becomes no row. The trace
then records the `game` receipt section (type `JetPerfGameRelay`): the first
and last sequence read, the records the relay retired before any observation
read them (`records_retired`), the frames and draw events kept, the frames that
ran more callbacks than were read (`frames_missing_callbacks`), the callbacks
kept nowhere (`callbacks_without_frame`, `callbacks_without_function`) and
whether rows past the trace's 4096-row limits were dropped (`rows_truncated`).
`jet perf view` prints `game frames=40 draw_events=80 scene=arena cpu=812345ns
slowest_frame=40123ns · <function> main.jet:13`, then each callback function's
draw events, the most time first (`game draw <function> main.jet:13 count=40
ns=82000000`). Tests/Cases/Perf/game_frame_events.case is the worked example.

A `jet perf run` or `jet perf test` trace records the receipt section `perf`
(type `JetPerfReceipt`). Its `build` names the build the program compiled
under: `profile`, the profile the run's words select (null for none), and
`options`, every build option's value as `jet run` resolves the words over the
source's declared profiles (`optimize`, `debug_info`, …). A `jet test` run
resolves its build through its settings layers as well, which the session does
not, so its `build` is null, as it is for words the run refuses. Its `capture`
names the capture's settings: `allowlist`, the privacy fields the trace keeps
(comma-separated, null for none), and `sampling`, null unless the program
sampled, else its `lenses` (`["cpu"]`, `["memory"]` or `["cpu","memory"]`),
`allocations` `call-sites`, and under the `cpu` lens `clock` `cpu`,
`interval_us` 125 and `pc`, which are null without it. `pc` names what each
sample was charged by: `source` `ibs_op` or `pebs` with `precise` true and
`skid` null for a precise sampler, `task_clock` with `precise` false and
`skid` `task_clock_interrupt`, else `timer` with `precise` false and `skid`
`timer_signal`; `refused` lists each source the kernel refused,
`{"errno":2,"source":"ibs_op"}`; `paranoid` is `perf_event_paranoid` (null
where the kernel has none); and `period` is the source's period, in
`period_unit` `cycles` for a precise source, else `ns` of CPU time. `jet perf
view` prints it as `receipt perf <canonical JSON>`, and `jet perf compare`
lists each field that differs between two traces' receipts
(`perf.build.options.optimize: "O0" -> "O2"`). `jet perf attach` records no
`perf` section.

## Comparing captures

`jet perf compare <base> <head>` takes each side as one `.jettrace` or a
folder of them: every `.jettrace` directly in the folder, in name order,
is one capture of that side (`jet perf run main.jet --out base/1.jettrace`
creates the folder). An empty folder is refused. Every capture must share
the first base capture's toolchain digest and hardware fingerprint unless
`--override-identity`, and each side's captures must have run one source
(the same path and digest for every source file); either refusal is E2102
naming the capture. The `Captures:` line lists each side's captures. The
wall, CPU and allocation deltas and the budget line set the sides' means
against each other; the receipt and heap deltas set the first capture of
each side against each other.

Compare then prints a function table and a line table of CPU-time
samples, the most samples first, each row the base side's mean, the head
side's and the change: `main.jet:11 spin_hot_loop 59 -> 118 samples
(+100%) slower (p=0.029)`. A row a capture never sampled counts 0 for it.
With at least 4 captures a side and at most 62 in all, each row holding
at least 1% of either side's samples gets a verdict from an exact
two-sided Mann-Whitney U test over its per-capture samples, its p
adjusted over the rows judged in its table (Benjamini-Hochberg) and
rounded up: `slower` or `faster` below 0.05, else `no significant
change`; a smaller row is `too few samples to judge`. With fewer captures
the verdict is none, naming why (`verdict: none (one capture each;
capture each side at least 4 times)`), and the rows show the means
alone. Compare exits 0 either way: `jet budget check` is the gate.
`--json` prints the same under `perf`, kind `jet.perf.compare`: each row's
per-capture `base` and `head` samples, `p_ppm` (the adjusted p in parts
per million, null when unjudged) and `verdict`.

## Flight recorder and crash dumps

D-OBSERVE-FLIGHT1=B ratifies an always-on flight recorder under `jet run
--observe` (when `JET_PERF_SAMPLES` names no `jet perf run` capture). The
runtime samples its CPU time every 10ms and records its tasks' waits, wakes,
spawns and file and network transfers, as `jet perf run` does, into a
256 KiB ring. Each record is one datagram on a nonblocking socket pair, sent
whole or, while the socket is full, lost, so no record ever waits. The observe
writer drains the socket into the ring every 100ms, the oldest records going
once it fills, and publishes `<temp>/jet-flight-<pid>.samples` beside the live snapshot, and the
program's image as `.image` once. `jet perf attach <pid> --source app.jet`
copies both and reads the copies, so its trace holds the waits and transfers
the program made before the attach: each wait's task, line, object and
waker, and each transfer's `io` row. A wait of a task the live snapshot no
longer holds becomes no row. Tests/Cases/Perf/attach_drains_flight_ring.case
is the worked example.

`core.perf` zones, frame marks and plots stay off in such a run: each zone
site tests one runtime word and calls nothing while it is clear.
`jet perf attach <pid> --source app.jet --zones=500ms` turns them on for the
window. It leaves `<temp>/jet-flight-<pid>.samples.zones` beside the ring,
which the writer's next publication answers by setting the word and writing
the zone, frame, plot and log span rows to `.samples.spans`. It then removes
the request, so the next publication clears the word, and reads the rows into
the trace beside the ring's, each zone holding the ring's samples inside it.
Tests/Cases/Perf/attach_turns_zones_on.case is the worked example.

`jet run --observe --checked-alloc app.jet` keeps the live list as a `jet
perf run --lens=alloc` does, which `jet perf attach <pid> --source app.jet
--lens=alloc` snapshots while the program runs. It leaves
`<temp>/jet-flight-<pid>.samples.heap-request` beside the ring, which the
writer's next publication notices as it looks for the zones request: it
takes the snapshot on the writer's thread, named `attach`, with the live
list held as a `heap_mark` holds it, so no block joins or leaves it while
the child forks, and answers in `.samples.heap`: the time the list was held
and the snapshot's records. Its roots are the image's writable segments;
the writer's frames are not the program's. The trace keeps the snapshot in
its `heap` section, and stderr states how long the program's allocations
and frees waited, `Heap snapshot: held the program's allocations and frees
for 412 µs`. A program not started with `--checked-alloc` keeps no live
list: the attach is refused with that reason and the command that starts
one, and writes no trace. An observed run that is never attached pays one
`faccessat` a publication, every 100ms, for the request.
Tests/Cases/Perf/heap_snapshot_types_retention_and_diff.case is the worked
example.

A stop that ends an observed process (a panic, a trap, a contract failure)
writes a typed dump while the stopping stack is whole:
`.jet/dumps/<program>-<UTC time>.jetdump` under the working directory, mode
0600, `~<n>` after the time when a dump of the same second exists, of which
the newest three stay. Stderr names it (`dump written: <path>`). The dump
holds every task's registers, the memory the stop reaches, the flight ring,
and the program's image, so it opens without the binary. The memory starts
from every thread's and task's stack and the program's globals and follows
each word that points into the program's writable memory: to its page, or
to a whole large block (a mapping of its own) at that block's start. The
walk is conservative, so it holds at least what the typed pointers reach,
without reading the image's types in the stop. It holds at most 32 MiB of
that memory, in the order it reaches it; past that budget the rest is cut
off, and the dump lists each range it cut off. The stopping thread's
registers are its own, every general register as it sends itself the
real-time signal 62 from the dump; each other thread is interrupted by
that signal, records its task and every general register, and waits until
the dump is written, so its stack is the one its registers name; a parked
or queued task's registers are the callee-saved ones its last switch
saved. A thread that does not answer within 200ms is left out. Every
`core.crypto.Secret`'s bytes are zeroed before the write.

`jet debug <file>.jetdump` opens a dump behind the `(Jet)` console: `Dump of
process <pid>, <n> flight events`, then `Memory: <n> bytes held, within
the 33554432-byte budget; <n> bytes cut off past it`, then the innermost Jet
frame of the stop (`Stopped  main.jet:9  in check()`), its source line and
its locals. `bt`, `frame N`, `locals` and `print NAME` read the stop as a
live session reads one, with typed locals; a `Secret` shows as
`<redacted>`, and a value whose bytes the dump cut off as `<cut off>`.
The runtime's frames between the stop and the program are stepped out of
by the image's call frame information (`.eh_frame`), so a register they
saved holds its value in the program's frames.
A parameter the code no longer keeps where a frame stopped reads the value
its caller passed, as the call recorded it (a DWARF call site parameter,
read through `DW_OP_entry_value`); when the call recorded none, or the body
may rebind the parameter, it shows `unavailable (not kept at this point)`,
never a register's later value. Live sessions read parameters the same way.
`tasks` lists every task the dump holds, the selected one marked `*`, as
`task <id>  stopped`, `running` or `parked`, then its innermost Jet frame
and, when that frame is outside the program's source, the innermost one
inside it (`task 3  parked in sleep() at time.jet:41, from run() at
main.jet:14`); `task N` selects task N for `bt`, `frame`, `locals` and
`print`. `events [--last N]` (20 by default) lists the ring's records,
oldest first, each at the first Jet frame of its stack: `sample`, `spawn`,
`wake`, `clock`, `wait` and `io`. A run-control command answers that a
dump doesn't run. Tests/Cases/Debug/crash_dump_opens_typed.case is the
worked example; Tests/Cases/Debug/crash_dump_keeps_its_budget.case shows
the budget and a value cut off past it.

## Canonical devtools protocol

`jet.devtools.v1` is the single observation envelope for development hosts.
Every execution tier publishes the same typed event record; a host projects it
instead of reconstructing meaning from transport text. The first-party event
families are `build`, `route`, `query`, `mutation`, `form`, `table`, `store`,
`trace`, `cost`, `gate`, `structure`, `test`, `job`, `frame`,
`game-frame-sample`, `game-draw-event`, `game-launch-profile`, `game-swap`,
`request`, and `response`. Package-specific data may use the typed payload seam,
but a host must not invent a family or reinterpret a payload.

The protocol version is 1. Its development policy is loopback-only and
payload-free by default, with a maximum of 256 retained events and 256 history
entries, 16 KiB text fields, and 1 MiB envelopes. Values are private unless a
producer publishes them through the typed boundary. Live-value decisions carry
a slot identity, type identity, disposition, reason, and optional rendered
text; sensitive or invalid text is rejected or redacted by Foundation policy.
Production hosts do not expose the development stream.

A development session owns a bounded event ring. `devtools_events_since` and
`devtools_panels` return typed projections; hosts do not decode event text to
recover panel meaning. A cursor older than retained history requests a reset
and marks truncation, rather than pretending that missing history exists.

## Server-function observations

`action`, `form`, and `data` registrations publish typed server-function facts
in the same `jet.devtools.v1` stream. A fact can identify the endpoint and
method, input/output/error wire types, CSRF policy, capability and declared
effects, retry and idempotency policy, middleware, call state, attempt count,
and dependent-data keys for revalidation.

The fact is metadata only. Request bodies, authentication credentials,
capability tokens, handler results, and handler errors are never devtools
payloads. A development App may render the projection; release HTML omits the
panel and server-function state.

## Query and mutation observations

`query` and `mutation` events carry identity and lifecycle facts, not private
values. A query fact includes its explicit key, dependency footprint,
generation, freshness age, observer count, invalidation cause, and state. A
mutation fact includes its key, lifecycle, attempt count, queue length, and
invalidation targets. Query and mutation payloads and handler results remain in
typed runtime state.

Query keys are cache identities. Reusing a key with a different footprint is
a runtime contract error (card #3709), not a second cache row. A mutation without
explicit targets revalidates the declaring query's footprint; explicit
`key:<key>` and footprint targets remain in the fact.

Offline-first mutations use the durable FIFO queue identified by
`jet-web-query-queue-v1`. The queue is loaded before replay and persisted by an
atomic replacement. Missing state configuration, malformed rows, and I/O
failures are durability errors; the runtime never reports a successful enqueue
after losing the payload. Development panels may show queue count and lifecycle
metadata, but never payload bytes or error text.

## Devtools hosts

The canonical panel catalog has these IDs: `Build`, `Routes`, `Queries`,
`Mutations`, `Forms`, `Table`, `Store`, `Traces`, `Cost`, `Gates`, `Structure`,
`Tests`, `Jobs`, `UiTree`, `Database`, `Topology`, `Game`, `World`, and
`Profiler`. A checked capability can make a panel available, unavailable with
a reason, or omitted from a host projection; unavailable is never a silent
no-op.

| Host | Entry point | Shared behavior |
|---|---|---|
| Browser in-app | `/__jet_devtools` | Shows status, selected fact, and available panels. |
| Browser workbench | `/__jet_devtools/workbench` | Shows the panel rail, event timeline, inspector, and cursor. |
| Terminal | `jet dev` | Renders the same panel facts through the terminal host; `NO_COLOR` uses plain text. |
| Editor | `jet/devtools/open` | Returns a bounded workbench projection for editor clients. |
| Native overlay | Foundation native-host callbacks | Receives the same typed events and bounded frame commands. |

The browser workbench uses three columns at wide widths and stacks the rail,
facts, and inspector at narrow widths. Browser selection posts `panel_id` and
`item_key` to `/__jet_devtools/selection`. Reconnection requests use the
bounded cursor at `/__jet_devtools/reconnect?cursor=<sequence>`; a stale cursor
returns an explicit reset/truncation projection. The next projection carries
the shared selection and cursor to every host.

The shared lifecycle states are `starting`, `building`, `ready`, `error`,
`unavailable`, and `stopped`. A host shows the state and its diagnostic facts;
it does not turn an unavailable panel or command into a silent no-op.

## The debugger console

`jet debug <file.jet>` stops at the start of `run` (or at `--break
<file>:<line>`) and reads one command a line at the `(Jet) ` prompt. When
its stdin is a terminal, the program shares it, as a shell's job does: the
program reads the terminal while it runs and the console while it is
stopped. Otherwise the program's stdin is empty, and its reads end at
once; `jet debug --dap` keeps stdin for its protocol, so its program's
stdin is empty too. D-DBG-VERBS1=A names the commands:

- `step` (`s`), `next` (`n`), `continue` (`c`) and `finish` (`f`) run the
  program.
- `reverse-step` (`rs`), `reverse-next` (`rn`), `reverse-finish` (`rf`)
  and `reverse-continue` (`rc`) go back in a replay (see Replaying a
  recording); `events`, `jump <n>` and `back [<k>]` move through its
  history of stops.
- `why <place>` names, in a replay, the write that gave a local or field
  path its value (see Replaying a recording).
- `break <line|file:line|function> [if <expr>]` (`b`) sets a breakpoint.
  `break 12` is line 12 of the file being debugged. With `if`, the breakpoint
  stops only when the Jet expression holds in the stopped frame. A condition
  that is not a Bool is refused when it is set; one that fails at a hit
  stops and says why. Setting it says where the condition runs: the
  debugger checks it at each hit, which stops the program each time.
- `log <file:line> "<text with {expr}>"` sets a logpoint. It prints its
  message with each `{expr}` filled in, then the program runs on. Setting it
  says where it runs. A logpoint with no condition, and whose parts are
  each a number, Bool or Char, runs in the program: each part is compiled
  on its own and placed as a condition is, and a hit writes its parts'
  values and a sequence number to a ring of 65,536 slots in the program's
  memory, then runs on without a stop. The debugger reads the ring and
  fills in the messages, in the order of their hits, when the program
  stops, when the ring is full, when a thread exits and before it changes
  the breakpoints. Its messages then come after the program output that
  preceded the read, not between its lines. Any other part (a String, a
  struct) is read from memory that may change after the hit, so such a
  logpoint says why it can't run in the program, and the debugger fills in
  its message at each hit, which stops the program each time.
- `watch <place>` stops at each write that changes a local of the selected
  frame, a field path under one (`watch order.total`), or an item of a List
  (`watch xs[3]`, `watch order.lines[i + 1]`). The compiler's parser reads
  the place, as `print` reads its expression, so a malformed one is refused
  with the parser's diagnostic. An index that is not a literal is evaluated
  as `print` evaluates it, once, when the watch is set, and the watch stays
  on that item. The stop names the line that wrote it and shows
  `total: 0 -> 2`. A write that leaves the value as it was runs on.
- `breaks` lists the breakpoints and watches by number, as `  2 break
  cart.jet:88 if total < 0` and `  3 watch total`; `delete <n>` removes one.
  A condition or logpoint that runs in the program adds how many of its
  hits have trapped to the debugger, as `  (1 trap)`, once any has. Only a
  hit where the condition held, needed the debugger or filled the ring
  traps. The placed code keeps no state per thread or task, so the first hit
  on a thread or task that starts later runs on without a trap like any
  other hit.
- `frame <n>` selects a frame as `backtrace` (`bt`) numbers it. `up` moves
  toward the callers and `down` back. `locals` and `print <expr>` (`p`)
  read the selected frame until the program moves.
- `tasks` lists every task by id: its label, its state (queued, running,
  blocked or paused), what it waits on (`channel receive`) and its Jet
  frames; `*` marks the task the program stopped in. Task 1 is the main
  thread.
- `help` (`h`) lists the commands and `quit` (`q`) ends the session.

Every thread of the program is traced. A stop stops them all, so `tasks`
and `print` see the whole program still. `continue` runs them all again,
and a step moves only the stopped thread. The task list is the runtime's
own registry (`Compiler/JetRuntime/Source/Observe.jet`, the one `jet
inspect live` publishes), read from the stopped program's memory. A parked
task's frames come from the stack its fiber saved when it parked.
`jet debug --dap` still shows the program as one thread.

A step that passes a breakpoint which does not stop goes on to where the
step would have ended. `jet debug --dap` settles every run the same way
(`Compiler/JetDebug/Source/Native.jet`, NativeSession.settle).

While `jet debug --dap` runs the program on (`continue`, and the start from
`configurationDone` or `restart`), the next request stops every thread where
it is: `pause` answers and reports the stop; `disconnect` ends the session;
`restart` starts the program again (an attached one does not restart); a
request about a stop (`stackTrace`, `scopes`, `variables`, `evaluate`) or a
run command waits for the next stop, the program running on to it, and is
answered there in the order it came; any other request is answered with the
program stopped, and it runs on. The end of stdin ends the session once the
requests waiting for a stop are answered. A replay's runs are made as
recorded, without stopping for requests.
`stackTrace` with `showRawFrames: true` adds the machine frames, each named
`[raw] <function>` with its `instructionPointerReference`, whose one scope,
`[raw] Locals`, holds its pc and canonical frame address; a `showRawFrames`
that is not true or false is E2236 (`jet.invalidArguments`). A frame id or
variable reference names the stop it was made at: once the program runs
again it is E2238 (`jet.staleReference`). The error's `jetCode` names the
diagnostic. `dataBreakpointInfo` and `setDataBreakpoints` are `watch`: a
local or field path of a frame of the stopped task, named by its scope's
`variablesReference` (or a `frameId`), gets a data id that
`setDataBreakpoints` places as the watch, and its stop is a `data
breakpoint` one whose description is the change (`total: 0 -> 2`). A
place no watch can take has no data id, its description E2237's What and
its `jetCode` E2237; a data id of an earlier stop is not verified, with
E2238's. A frame in a Core function names its source by a
`sourceReference`, which the `source` request answers with that source's
text: Core's sources are no files of the program, and the debugger reads
them, as it compiles them, from the Core the program was compiled with,
never from the working directory. The console shows a Core frame's line
the same way.

Every expression the debugger is given (`print`, `watch`, a condition, a
logpoint's message, the adapter's `evaluate`) has one evaluator, the
compiler (`Compiler/JetDebug/Source/NativeEval.jet`,
`Compiler/JetCLI/Source/CLI/DebugCompile.jet`). The expression is checked
where the selected frame stopped, as a function of the locals it reads; one
with an effect is refused, and the refusal names the effect. A caller's frame
stopped at a call: the debug information gives each call the line and column
of the source node whose lowering made it (a `DW_TAG_call_site` with its
return address), a call the compiler made for a node (a lambda that a
lowered `map` calls) included, so the expression is checked just before that
node; `bt` shows the frame at that line, and the adapter's `stackTrace` at
that line and column. A row names where the node starts, so two calls of one
chain that start together share a row. The program is compiled again at
the optimization level of its build, which the image's debug information
names (`Jet native backend (O2)`), so the code around the expression is the
code the program runs, with that function, with the locals' bytes read from
the frame. It runs in the stopped program itself, on the selected thread,
when the image's runtime exports its allocator words
(`jet_rt_alloc_class_limit`, `jet_rt_alloc_check_take_code` and
`jet_rt_alloc_check_give_code`, data symbols every image that allocates holds;
nothing in the program writes them for the debugger, so a program no
debugger touches pays nothing for them): the debugger maps the
expression's code and a room beside the program's code once per run of the
program, sets the thread to call it from that room with the other threads
stopped, and puts the thread's registers, its floating-point state and the
code the debugger planted back after it. For the call the debugger writes
the allocator words, as checked mode routes the heap, so the call's
allocations come from a bump arena in the room, reset each evaluation, and
writes them back after: the program's heap is unchanged; a block of the
program's own that the call gives back stays
allocated and is counted, and the next report says so. The call runs under
the debugger: a system call is refused (a write to stdout or stderr is
caught and shown as the failure's text), a fault or the 2 s budget stops it
where it got to, the thread's state is put back, and the refusal says the
program may keep values alive that the evaluation took references to and
could not give back; a breakpoint it passes is stepped over, a watched
place it writes stops nothing, and a signal sent to the program meanwhile
is the program's once the call is done. An image without the allocator
words, and a run that records or replays (`jet debug --record`, `--replay`),
evaluates in a fork of the stopped program instead; the program itself
neither runs nor changes. The mapped code holds a fresh copy of the
function and of every function it calls, which reach the program's own
data and runtime where its unchanged code does, so the compile's choices
for the whole program (its `main`, its register use) need not be the
image's; a function the expression calls whose code differs from the
program's is refused, the source no longer the program's. `fix` compiles
at that level too. An inlined frame is evaluated in as `bt` shows it: the
expression is checked in the inlined function at its line, the frame it was
inlined into at the call, and the locals are the frame's own; code inlined
from another file is refused, since its lines are not that file's. A value whose type has a
`debug_view` shows as `locals` shows it, the view run in a fork of the
program as the value stands. A
compiled expression is kept per line, column and text, so a condition compiles once
and runs at each hit. A `print` at a later stop in the same frame
of the same function runs the code compiled before when its names read the
same variables there (the debug information's variables of each name
declared by the stop's line, and whether each one's location covers the
stop) and each local it reads is a number, Bool or Char, which no move
empties; any other expression compiles again. The call runs the program's own runtime, so
a lock another thread held at the stop is never released while it runs (code that
takes it fails at the 2 s budget), and a function value runs the image's
code. An expression that calls a function that can fail shows its result as
`locals` shows a fallible result, `parse(s) = Ok(3)` or `Err(ParseError.Empty)`,
so a condition that can fail is refused, not a Bool. A call, of an evaluation
or of a condition run in the program, passes up to 5 locals in registers
and more by a table of their addresses on the stack, so a condition that
reads more than 5 runs in the program too. A local's own name is no
expression: `print total` shows the local as `locals` does, in a dump too,
and a dump runs no code.

`fix` swaps the edited program's changed functions into the stopped
program as one transaction: it writes nothing until each check passes, and
a refusal says why and leaves the program running as it was. The candidate
is checked: the entry compiles again with no problems, at the image's
optimization level, built and lowered whole. Its inputs are unchanged:
each source file the compile read still reads the same just before the
install, so a file saved meanwhile refuses the fix. The checkpoint it
installs at accepts it: the program's file is still the image the process
runs (its code reads the same, but for the debugger's own trap bytes and
jumps); no struct changes its layout, which the refusal names by field; no
function the program runs changes its parameters while a caller's frame on
a stack, which finishes in its old code, would pass it the old ones; and
the edit changes neither the program's start nor needs a runtime routine
the image does not hold.

A watch on a place in memory takes the CPU's debug registers (at most 4,
each an aligned 1, 2, 4 or 8 bytes), so the program runs at full speed
until the write. A local the compiler keeps in a register has no address
to trap: its watch single-steps that frame's own code, comparing the
register after each instruction, while the calls it makes run at full
speed (they restore the register before they return). One such watch is
active at a time. A watch covers the place's own bytes: for a list, its
length and buffer, not the items; `watch xs[3]` covers one item. An item's
watch also takes a debug register on the List's buffer pointer, so when
the List grows past its capacity (or a write copies a shared buffer) and
moves to a new buffer, the watch moves to the same item there; the copy
leaves the item's value as it was, so the move runs on. If the List lets go
of its buffer, the watch ends and the next stop says so. A watch on a
frame's local ends when that frame returns (a frame of another function a
later call puts at the same stack address is not it); the next stop says
so. Every
thread gets the watch's debug registers, so a write from any thread stops;
a watch on a register local follows the thread whose frame holds it. A
Float held in a vector register can't be watched. Globals can't be watched
yet, because the debug information has no global variables. A watch the
debugger can't place (a place the parser doesn't read as one, no such
local or field, no bytes, no free debug register) is refused with E2237
(`Jet.DebugRequestRefused`), which names why.

## Recorder boundary

D-RUN-RECORD1=A ratifies `--record=<name>` on `run`, `dev`, and `test` as the
user-verb recorder. The name is not a path, and the artifact is written under
`.jet/replays/<name>.jetproof-replay`. This guide does not add a second
recording flag; live snapshots, GC evidence, wall-clock sessions, and browser
transport remain distinct until their contracts can share an envelope without
losing meaning.

## Replaying a recording

`jet debug --replay=<name> <file.jet|image>` runs the program behind the
debugger console (or `--dap`) under the recording `jet run --record=<name>`
made, read from `.jet/replays/<name>.jetproof-replay` in the project of a
source entry, or of the working directory for an image; a project-relative
`.jetproof-replay` path names one directly. The recording is not changed.
jet writes its `RunSync` log to a scratch file and starts the program with
`JET_REPLAY` naming it (below), so the run takes the recorded sync order and
reads the recorded inputs at whatever tier the image was built: a source
entry is debugged as the `-O0` build `jet debug` makes, and an `-O1` or `-O2`
image `jet build` made is debugged as it is. Its output is the recorded
output, and the first sync event where the run does otherwise stops the
program with the divergence (below), which the console shows with the
program's output. Time the debugger holds the program stopped does not count
toward the 10-second stall.

A NAME that isn't one, a recording that isn't there, a recording with no
sync-order log (a `jet test --record` one) and an image that links no
observation start are refused before anything runs. So is a source file
that is not the one recorded: the recording binds its source's digest, and
one of other source is E3621 (`replay semantic identity does not match`).
An image binds no source, so it replays unchecked. Under `--dap` each of
these answers every request but `initialize` with the refusal, its
`jetCode` naming the code, then ends the session.

`jet debug --record=<name> <file.jet>` makes the same recording under the
debugger: the program runs behind the console (or `--dap`) with
`JET_RECORD` naming a scratch file, as `jet run --record=<name>` runs it,
and once it has ended jet publishes `.jet/replays/<name>.jetproof-replay`
with the arguments it ran with and says so on stderr. The recording holds
the run as it went under the debugger (a stop may change which task gets a
lock first, and the log keeps the order that happened; `print` runs in a
fork of the program, which records nothing), so `jet debug
--replay=<name>` replays it as it replays a `jet run` one. The NAME is
checked before the build, as `jet run` checks it; a built image is refused,
since the recording binds the source it was built from; and a session that
ends the program early (`quit`, an editor's disconnect) publishes nothing.
Tests/Cases/Debug/record_under_debug_then_replay_why.case is the worked
example.

A replay can go back. `reverse-step` (`rs`) goes to the line that ran
before this stop, in any frame; `reverse-next` (`rn`) to the line before
in this frame or a caller, over the calls it made; `reverse-finish` (`rf`)
to the caller's line that made this call; and `reverse-continue` (`rc`) to
the breakpoint hit or watch stop before this one (a breakpoint whose
condition does not hold there is passed). Under `--dap` the adapter
answers `supportsStepBack`, and `stepBack` and `reverseContinue` are
`reverse-next` and `reverse-continue`. The session keeps the commands that
brought the program to its stop. Going back runs them again from the start
under the same recording, so the program does the same, with one-shot
trap bytes counting how often each task passes each line's start (or each
breakpoint); it then runs them once more to the pass the command wants,
which becomes the stop. The program's output is not shown again.
Breakpoints stay as they are; a watch set after the point it goes back to
ends, with a note. With nothing earlier the program comes back to the
same stop, refused with E2237 (`Jet.DebugRequestRefused`), which says
so; under `--dap` the request fails with it, and the stop's frame ids still
hold. Outside a replay, and after `fix` (a new run would run the code
before the fix), they are refused with E2237.
Tests/Cases/Debug/replay_reverse_*.case are the worked examples.
Tests/Cases/Debug/replay_refusals_*.case are the worked examples of the
refusals, at the console and over `--dap`.

A replay's stops are its history: the launch's stop is event 0, and each
run command's stop the next; a reverse command's stop takes the place of
the events from the one it goes back into. `events` lists them, `*`
marking the current one:

```
  0  prog.jet:16  in run()  breakpoint 1.1
  1  prog.jet:21  in run()  breakpoint 2.1
* 2  prog.jet:21  in run()  breakpoint 2.1
```

`jump <n>` goes to event n, back or forward, by running the commands that
reached it again as a reverse command does, and says `Event <n>` before
the stop; a stop other than the one the event recorded gets a note. The
events after it stay listed for a jump forward until a new run command or
a reverse command replaces them. `back <k>` goes k events back; `back`
alone goes to the latest earlier event that stopped at one of the
session's watches, or one event back with no watches. A watch set after
the event ends, with a note, and comes back with a jump forward past where
it was set. Under `--dap` they are requests of Jet's own: `jump`
(`{"event": n}`) and `back` (`{"count": k}`) answer with the event and
report its stop, and `events` answers with the history. They are refused
with E2237 where the reverse commands are.
Tests/Cases/Debug/replay_jump_*.case and replay_watch_back_*.case are the
worked examples.

`why <place>` in a replay names the write that gave a local of the
selected frame, or a field path under one, the value it has here:

```
total = 2, set at prog.jet:21 in run() (task 1)
  before: 0
  #0  run()  at prog.jet:21
```

The line is the write's own, then the task that made it, the value it
replaced and the stack it was made from. A write before the local's first
line binds it, and says so as `before: none: this line binds it`. With no
write since the frame began, it says that. `why` runs the commands again
three times under the same recording: once to find the call that opened the
frame and to count this stop's passes after it; once from that call, with
a debug register on the place's bytes keeping every write that changes them
(the program runs on) until the same stop; and once more to come back here,
breakpoints and watches as they were. The traced program runs without
address randomization, so the place has the same address in every run. A
local an `-O2` image keeps no place for (optimized away, or held in a
register) is answered from the `-O0` build of the image's source, which
jet makes once into the session's scratch directory: a run counts how
often the stop's task reached the stop's line, the `-O0` image runs under
the same recording to that pass of the line, and `why` answers there, the
value labeled `(recovered from the -O0 replay)`. Outside a replay, and
after `fix`, it is refused with E2237.
Tests/Cases/Debug/replay_why_*.case are the worked examples.
Tests/Cases/Debug/replay_threads_across_tiers_*.case are the worked examples.

## Sync-order log

A program's run records its synchronization order when `JET_RECORD` names a
file, and replays it when `JET_REPLAY` names one
(`Compiler/JetRuntime/Source/RecordReplay.jet`). The log holds sync events in
one global turn order:

- spawn and join;
- a Shared read or edit permit, taken or given up;
- a channel's send, receive, try-receive, close, and endpoint drops;
- a select's chosen arm;
- each print and eprint, with its length and the FNV-1a hash of its text.

It also holds the bytes of each input: a wall or monotonic clock read, an
entropy fill, an environment value, the program's arguments, and each file,
pipe, socket, and stdin read. A UDP sender's address, a positioned file read,
and a connect are not logged yet.

Tasks, Shared values, and channels are named by ids that do not depend on
timing. The main task is 1. Any other task, Shared value, or channel mixes
the id of the task that made it with how many things that task made before
it. The log holds no instruction counts, so a recording made at one tier
replays at any other tier.

A replaying task waits at each sync event until the log's next turn names it.
It then checks that its event is the recorded one, and does it. A select takes
its recorded arm, an input reads its recorded bytes, and a print must write
the recorded text before it writes anything. The first mismatch, or a turn
that no task takes for 10 seconds, stops the program with exit 70:

```text
replay diverged at sync event 7: recorded edit of Shared value 1 by task 2, got send on channel 1 by task 3  main.jet:14
```

Only a program that links the runtime's observation start records or
replays. `jet run --record=<name>` links it, runs the program in a child with
`JET_RECORD` naming a scratch file, and writes the log into
`.jet/replays/<name>.jetproof-replay` as `RunSync` frames
(`Docs/Spec/proof-replay-decisions.md`). The artifact is written with mode
0600 and stays local: its log holds every input's bytes.

## Implementation homes

- Live scheduler: `Compiler/JetFoundation/Source/Devtools/LiveSnapshot.jet` (codec and transport), viewed by `Compiler/JetCLI/Source/CLI/LiveReport.jet` and `Compiler/JetDevServer/Source/LiveInspect.jet`
- GC promotions: `Compiler/JetCLI/Source/CLI/GCReport.jet`
- Wall-clock session: `Compiler/JetCLI/Source/CLI/Perf/` and `Compiler/JetFoundation/Source/Trace/JetTrace.jet`
- Browser rows: `Compiler/JetDevServer/Source/BrowserTrace.jet`, merged by `Compiler/JetCLI/Source/CLI/Perf/BrowserCapture.jet`
- Devtools protocol and panel policy: `Compiler/JetFoundation/Source/Devtools/` and the panel catalog projection in `Compiler/JetDevServer/Source/Session.jet`
- Flight recorder and dumps: `Compiler/JetRuntime/Source/PerfSamples.jet` (`perf_flight_*`), read by `Compiler/JetDebug/Source/NativeDump.jet` behind `Compiler/JetCLI/Source/CLI/DebugDump.jet`
