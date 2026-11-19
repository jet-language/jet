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
delivered to that thread, one sample per millisecond of its CPU), so threads
that run at once each keep their samples, and records the program counter
each `SIGPROF` interrupted with the call site of each caller, each handler in
a record buffer of its own so no sample loses its stack to another thread's.
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
writes with the samples, so an allocation in the runtime or a core module
counts at the line of the source that called into it; a run whose runtime
recorded no allocation site leaves it null. A sample in a compiler-emitted
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

`jet perf run` takes `--lens=<lenses>`, comma-separated: `cpu`, the
program's CPU time and the default, `memory`, its memory behavior, and
`alloc`, its live heap; `--lens=cpu,memory,alloc` takes all three in one
run. `jet perf test` takes only `cpu`, and a word that names no lens is
refused with the three it may name. Under `memory` the
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

Under `alloc` the session builds the program with `--checked-alloc`, whose
heap keeps each block's ten allocating call sites in its prefix, and the
runtime links every block allocated from the start of the run into a live
list until it is freed. A run without the lens tracks nothing and pays
nothing. `core.perf.heap_mark(name)` takes a heap snapshot named `name`,
and the runtime takes one named `exit` as the program exits; in any other
run `heap_mark` does nothing. A snapshot holds the live list while it
forks: the copy-on-write child walks the list as it stood, groups the live
blocks by their call sites, writes the groups to the samples file and
exits, while the program goes on once the child has the copy and waits for
it to exit. The trace keeps the snapshots in its `heap` receipt section
(type `JetPerfHeapSnapshots`): the checked source's `path`, then each
snapshot in the order the program took them with its `name`, `number`,
live `blocks` and their `bytes`, and its `sites`, the blocks by the
`function` and `line` of the checked source innermost on their allocating
stack, the most bytes first, with a null `function` and `line` for the
blocks no line of it allocated. `jet perf view` prints each snapshot after
the per-line table, `live at filled  101 blocks  57344 bytes` and then
each site, `  leak.jet:14 run  100 blocks  56000 bytes`, or
`  elsewhere  1 blocks  1344 bytes`; a name the run marked more than once
is shown with its count, `after-cleanup#3`.

`jet perf compare base.jettrace head.jettrace` sets the last heap snapshot
of each trace against each other when both kept one, and
`--heap=<base>..<head>` names the two: the base trace's snapshot `<base>`
and the head trace's `<head>`, both of one trace when it names only one
(`jet perf compare run.jettrace --heap=start..filled`). A name alone picks
the last snapshot of that name, and `name#k` its k-th; a name the trace
never kept is refused with the names it kept. Compare prints `Heap start
-> filled: blocks 1->101 · bytes 1344->57344` and then each place whose
live blocks or bytes changed, signed, the most bytes gained first,
`  leak.jet:14 run  +100 blocks  +56000 bytes`; its receipt deltas leave
the `heap` section out.
Tests/Cases/PerfRun/alloc_lens_heap_snapshots.case is the worked example.

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
attach records no transfers. Tests/Cases/Perf/io_on_timeline_without_instrumentation.case
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
`allocations` `call-sites`, and under the `cpu` lens `clock` `cpu` and
`interval_us` 1000, which are null without it. `jet perf view` prints it
as `receipt perf <canonical JSON>`, and `jet perf compare` lists each field
that differs between two traces' receipts (`perf.build.options.optimize: "O0"
-> "O2"`). `jet perf attach` records no `perf` section.

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
<file>:<line>`) and reads one command a line at the `(Jet) ` prompt.
D-DBG-VERBS1=A names the commands:

- `step` (`s`), `next` (`n`), `continue` (`c`) and `finish` (`f`) run the
  program.
- `break <line|file:line|function> [if <expr>]` (`b`) sets a breakpoint.
  `break 12` is line 12 of the file being debugged. With `if`, the breakpoint
  stops only when the Jet expression holds over the frame's locals. A
  condition that is not a Bool, or can't be evaluated, stops and says why.
- `log <file:line> "<text with {expr}>"` sets a logpoint. It prints its
  message with each `{expr}` filled in, then the program runs on.
- `watch <place>` stops at each write that changes a local of the selected
  frame, or a field path under one (`watch order.total`). The stop names
  the line that wrote it and shows `total: 0 -> 2`. A write that leaves the
  value as it was runs on.
- `breaks` lists the breakpoints and watches by number, as `  2 break
  cart.jet:88 if total < 0` and `  3 watch total`; `delete <n>` removes one.
- `frame <n>` selects a frame as `backtrace` (`bt`) numbers it. `up` moves
  toward the callers and `down` back. `locals` and `print` (`p`) read the
  selected frame until the program moves.
- `help` (`h`) lists the commands and `quit` (`q`) ends the session.

A step that passes a breakpoint which does not stop goes on to where the
step would have ended. `jet debug --dap` settles every run the same way
(`Compiler/JetDebug/Source/Native.jet`, NativeSession.settle), and both read
expressions with the one evaluator, `Compiler/JetDebug/Source/Debug/Eval.jet`.

A watch on a place in memory takes the CPU's debug registers (at most 4,
each an aligned 1, 2, 4 or 8 bytes), so the program runs at full speed
until the write. A local the compiler keeps in a register has no address
to trap: its watch single-steps that frame's own code, comparing the
register after each instruction, while the calls it makes run at full
speed (they restore the register before they return). One such watch is
active at a time. A watch covers the place's own bytes: for a list, its
length and buffer, not the items. A watch on a frame's local ends when
that frame returns; the next stop says so. Only the program's main thread
is traced, so a write from another thread does not stop it. A Float held
in a vector register can't be watched. Globals can't be watched yet,
because the debug information has no global variables.

## Recorder boundary

D-RUN-RECORD1=A ratifies `--record=<name>` on `run`, `dev`, and `test` as the
user-verb recorder. The name is not a path, and the artifact is written under
`.jet/replays/<name>.jetproof-replay`. This guide does not add a second
recording flag; live snapshots, GC evidence, wall-clock sessions, and browser
transport remain distinct until their contracts can share an envelope without
losing meaning.

## Recorded-run queries

`jet run <file> --record=<name>` attaches source-level act snapshots to the
same `.jetproof-replay` receipt. The receipt keeps its Time authority, identity,
frame hashes, and footer; it does not create a `.jettrace` file or a second
trace format.

`jet debug <file> --replay=<name>` reads the receipt without changing it. The
source-level prompt supports:

- `why <place> == <value>` — names the recorded acts that produced the value;
- `when <place>` — lists recorded changes and names the last change.

These queries use recorded local snapshots. They do not reverse execution, move
the program through time, or keep a standing per-variable history buffer.

## Implementation homes

- Live scheduler: `Compiler/JetFoundation/Source/Devtools/LiveSnapshot.jet` (codec and transport), viewed by `Compiler/JetCLI/Source/CLI/LiveReport.jet` and `Compiler/JetDevServer/Source/LiveInspect.jet`
- GC promotions: `Compiler/JetCLI/Source/CLI/GCReport.jet`
- Wall-clock session: `Compiler/JetCLI/Source/CLI/Perf/` and `Compiler/JetFoundation/Source/Trace/JetTrace.jet`
- Browser rows: `Compiler/JetDevServer/Source/BrowserTrace.jet`, merged by `Compiler/JetCLI/Source/CLI/Perf/BrowserCapture.jet`
- Devtools protocol and panel policy: `Compiler/JetFoundation/Source/Devtools/` and the panel catalog projection in `Compiler/JetDevServer/Source/Session.jet`
