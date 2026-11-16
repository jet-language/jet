# Debugging and profiling adversarial design review (2026-11-14)

This review covers the first criterion of card #6841. It compares what Jet has for debugging and profiling, built and planned, with the strongest tools in use today:

- Jai's metaprogram instrumentation;
- Tracy;
- Superluminal;
- RemedyBG;
- the RAD Debugger;
- rr and Pernosco;
- Go's pprof;
- Linux perf and Apple Instruments.

Jet's docs and ballots claim seven strengths:

1. `jet debug` shows a native program in Jet terms: Jet frames, Jet source lines and typed Jet values (`Docs/spec/reference/cli.md:232`, "Debug a program from Jet source", and card #6730).
2. The terminal and the editor share one debugging session through a full local DAP profile (D-DBG-DAP1=A).
3. `jet perf` answers "which symbols consume wall time, CPU, task, lock, or I/O time?" (`Docs/spec/observability.md`, table row "Wall-clock session").
4. A trace exports to standard viewers: Chrome and Perfetto, pprof and OpenTelemetry (`observability.md` § Exporting a wall-clock trace).
5. Compile-time metaprogramming can instrument a program, as a Jai metaprogram can (`meta.events` with `meta.rewrite_body`, `Docs/spec/reference/metaprogramming.md:722-800`).
6. A run can be recorded once and queried later with `why` and `when` (D-RUN-RECORD1=A, `observability.md` § Recorded-run queries).
7. A release crash is symbolicated later from the symbol store (`jet crash`, with split debug files under `.jet/symbols`).

A tacit eighth claim runs under all seven: because Jet writes ordinary ELF with DWARF, the standard native tools work on Jet programs.

This review attacks each claim and checks it against the code on dev `7700a065ff`. Probes ran on Linux x86-64 with the lane's gate `jet`, perf 7.2.9 and LLDB 21.1.8. The gate `jet` predates the in-flight #6730 work: typed DWARF values, the debug link, `--break` and the DAP adapter. Where a claim depends on that work, the review says so. Every gap ends in the gap table with the card or ballot that owns it.

## Measured facts

The probe program is `fib` (recursive) plus `sum_squares` (a loop), with `n` read from the command line so that neither call folds at compile time. `run` calls both.

| # | Probe or read | Observed |
|---|---|---|
| M1 | `jet perf run prog.jet -- 36`, then `jet perf view` | `profile method=sampling status=captured coverage=partial rows=2 … overhead=not_measured`. The flame graph has one frame, `prog.jet#run`, with `samples=14` for wall and `samples=14` for CPU. `fib` is about 36% of the time (M3), but it never appears. |
| M2 | `Compiler/JetCli/Source/Cli/Perf/SourceSampling.jet:22,48-58`, `Perf/Session.jet:123,280` | The profile's only symbol is the `run` declaration (`name: "run"`). Rows are capped at 4096. A "sample" is one 25 ms poll of the runtime's live snapshot (`perf_time.sleep(25ms)`), not a program-counter or stack sample. No source in Compiler/ or Core/ calls `perf_event_open` or `setitimer`. `SIGPROF` appears only as a signal name in a crash report. `observability.md:64-66` says the same: "rows do not yet separate the functions that `run` calls." |
| M3 | `perf record -F 999 -g` on the `-O2` image, then `perf report --sort srcline` | 197 samples in 0.2 s. Frame-pointer call graphs unwind cleanly: `prog_fib` → `prog_fib` … → `prog_run` → `jet_rt_main` → `main`. By source line: `prog.jet:10` (the `sum_squares` loop, inlined into `run`) 57%, `prog.jet:5` 21%, `prog.jet:3` 9%, `prog.jet:4` 5%, unknown 7%. `perf annotate` puts Jet source lines next to the machine code. |
| M4 | `nm` and `readelf -S` on the `-O2` and `-O0` images | Symbols are `prog_fib` and `prog_run`, the module and function joined by `_`, not Jet's path `prog.fib`. Both images carry `.eh_frame` (228 FDEs), `.debug_info`, `.debug_line`, `.debug_loclists` and `.debug_jetline`. The backend writes ELF only: Compiler/JetBackend has no Mach-O and no PDB/CodeView writer, although Core/Target names `aarch64-apple-darwin`. |
| M5 | Wall time of the same input | `-O2` image 0.20 s; `-O0` image 0.30 s. The `jet perf run` window says `run_wall=308024894ns`, which matches the `-O0` time and not the `-O2` time. [INFERENCE: `jet perf run` profiles the unoptimized image, and the trace does not say which profile was built. #4632 c5 asks for that receipt.] |
| M6 | `lldb` on the `-O2` image, breakpoint at `prog.jet:10` | The stop reads `prog`run at prog.jet:10`. There is no inlined `sum_squares` frame: the backtrace is `run`, `jet_rt_main`, `main`. `n` and `total` print; `a` is "optimized out", `b` is "not available", and `args` is an `(Aggregate)` that fails to extract. This is #6730 c9 as stated. |
| M7 | `jet debug prog.jet`, stdin `b 4`, `c`, `bt`, `locals`, `q` | From the call to the first stop takes 0.55 s, with a warm build cache. The console reports `Breakpoint hit  prog.jet:4  in fib()`, `#0 fib() at prog.jet:4`, `#1 run() at prog.jet:17` and `Locals: n = 30`. At the first stop on line 15, all four locals print `<Unavailable>` before their bindings run (#6730 c8 fixes that in flight). |
| M8 | `Compiler/JetCli/Source/Cli/DebugDap.jet` (in flight for #6730 c23) | The adapter advertises one capability, `supportsConfigurationDoneRequest`. It handles launch, setBreakpoints, configurationDone, threads (one thread, `run`), stackTrace, scopes, variables (one level), next, stepIn, stepOut, continue and disconnect. Run control is synchronous: a `continue` blocks the adapter until the next stop. There is no pause, attach, restart, conditional breakpoint, hit count, logpoint, evaluate, nested variables, exception filter or `showRawFrames`, and the known-failing rows name each one. |
| M9 | `jet perf export <trace> --pprof` | `{"kind":"jet.trace.pprof-projection","loss":"pprof-json; samples are Jet wall/cpu only — no native stack frames, no gzip proto, no labels beyond domain","samples":[{"location":"wall",…},{"location":"cpu",…}]}`. `go tool pprof` cannot read it: pprof reads a gzip-compressed `profile.proto`. |
| M10 | A `meta.events(watch: .Functions{marked: Profiled{}})` handler on the gate `jet`; it calls `meta.rewrite_body(f, #code { zone := Zone{…}; defer close(zone); #{f.body} })`, and `Zone` implements `Close` | It builds. `fib(3)` prints `zone fib closed` five times, once per call, and then `2`. So the closing half of each zone runs on every exit, including the early `return n`. The mechanism for Jai-style per-function instrumentation exists today. |
| M11 | `Core/Log/Log.jet:107-148`, `Core/Perf/Perf.jet`, `Core/Debug/Debug.jet` | `core.log` has `span`, `enter`, `close`, `counter`, `sample_every` and `otlp_file`. These are log-sink records; nothing turns them into `.jettrace` zones today. `core.perf` is an 83-line fidelity knob. `core.debug` holds the stop handler and trace records. No Core API marks a timed zone, a frame boundary or a plot value for a profiler. |
| M12 | `Compiler/JetCli/Source/Cli/Crash.jet:6-39` | `jet crash` reads the build ID from a release report and finds `.jet/symbols/.build-id/<2>/<rest>.debug`, in the working directory or one above it. There is no symbol server and no remote store. |
| M13 | `tests/known-failing.tsv` | The rows `perf_run_captures_wall_and_alloc_into_jettrace` and `perf_attach_captures_wall_and_alloc_with_jet_symbol_from_observe` fail on #4632. The trace keeps an empty `allocations` array, and attach never sees a live snapshot. The DAP rows for attach, restart, pause and `showRawFrames` fail on #6730. `--record` and `--replay` report "not built" (#6418). |

## Peer practice

**Jai metaprogram instrumentation.** A Jai build is a Jai program. It runs a compiler message loop (`compiler_wait_for_message`) and sees every declaration as it is typechecked. It can add code with `#insert` and `add_build_string`, and it can change build options. The beta's `Iprof` module is a metaprogram plugin that wraps every procedure with timing calls and shows the result in an in-game overlay. Instrumenting a program is a build-script choice, not a source edit. Sources: the Jai community wiki, "Metaprogramming" (github.com/Jai-Community/Jai-Community-Library/wiki/Metaprogramming), and the beta's `modules/Iprof`. [Iprof is not verified first-hand: Jai is a closed beta.]

**Tracy.** Tracy is "a real time, nanosecond resolution, remote telemetry, hybrid frame and sampling profiler" (github.com/wolfpld/tracy). Its features:

- manual zones (`ZoneScoped`), frame marks, plots, messages and lock annotations;
- GPU zones;
- memory allocation tracking with call stacks;
- context switches, and sampling of call stacks;
- a client that connects to a live process over the network and keeps the whole capture browsable.

The manual (`tracy.pdf` in each release) gives the cost of one zone in nanoseconds. Tracy's strength is that a frame, its zones and the samples line up on one timeline.

**Superluminal.** Superluminal is "the industry leading sampling profiler" (superluminal.eu). It samples at a high rate across all threads and records thread interactions, so a blocked thread shows who unblocked it. It can add instrumentation events through its PerformanceAPI. Its UI goes from the overview to thread interactions, samples, source and disassembly. It supports Windows, Xbox, PlayStation and Linux, with C++, Rust and .NET. It needs debug information it can read: PDB on Windows, and DWARF on Linux [INFERENCE: from the platform list].

**RemedyBG.** RemedyBG is "a human made 64-bit Windows debugger which ships as a portable 7 MB executable" (remedybg.itch.io/remedybg). It reads PDB. Its strength is instant startup and stepping, watch windows with expressions and format specifiers, memory and disassembly views, and a driver protocol for editors.

**The RAD Debugger.** The RAD Debugger is "a native, user-mode, multi-process, graphical debugger" (github.com/EpicGamesExt/raddebugger). It is in alpha; Windows x64 is the main host, and its Linux support is "still very early". It converts PDB and DWARF into its own RAD Debug Info (RDI) format, which it built for very large programs. Its strength is speed at that size, plus rich visualizers and multi-process sessions.

**rr and Pernosco.** rr records a Linux process once and replays it deterministically under gdb, "as many times as you want", with "efficient reverse execution" (rr-project.org). Pernosco takes an rr recording and builds an omniscient database over it. It "explains how pixel data was computed" and answers "where did this value come from" across the whole run (pernos.co/about/overview). Pernosco targets C, C++, Rust, Ada and V8 JavaScript on x86-64 Linux. Both work on any native program that rr can record, with no compiler support beyond DWARF.

**Go pprof.** Go's runtime samples CPU with `SIGPROF` (100 Hz by default) and records heap, allocation, block, mutex and goroutine profiles. Every profile uses one format: `profile.proto`, "a gzip-compressed protocol buffer" whose samples hold "the program call stack when the sample was taken" (github.com/google/pprof/blob/main/proto/README.md). The `pprof` tool gives top lists, call graphs, flame graphs, source and disassembly, and diffs between profiles. Any language that writes `profile.proto` gets all of these views.

**perf and Instruments.** Linux perf samples hardware and software events through `perf_event_open`. It unwinds with frame pointers, DWARF or LBR, and attributes samples to symbols and to source lines through DWARF. It needs no help from the language. Apple's Instruments does the same on macOS with its Time Profiler, plus allocations, leaks, signposts (`os_signpost`) and System Trace. Instruments needs a Mach-O image with dSYM debug information.

## Claim 1: `jet debug` shows the program in Jet terms

**Verdict: it holds only within stated limits: an `-O0` image on Linux x86-64 with LLDB installed.**

M7 shows the claim working: the console names the stop in Jet terms (`fib()`, `prog.jet:4`) and hides the runtime frames below `run` (`jet_rt_main`, `main`). When a user debugs a Jet program in plain LLDB, those runtime frames show, along with C-shaped locals such as `(long) n` and `(Aggregate) args` (M6). The typed-DWARF and binding-range work in flight for #6730 (c1, c2 and c8) closes the value side.

The attack has three parts:

- **Optimized code.** RemedyBG and the RAD Debugger are used daily on optimized builds. M6 shows an `-O2` Jet image with no inlined frame (`sum_squares` disappears into `run`) and with locals missing. #6730 c9 owns this.
- **Speed.** M7's 0.55 s to the first stop is fine for a small program. It includes an `-O0` build and an LLDB start, so it grows with program size. RemedyBG's whole pitch is that its startup and steps feel instant. No Jet test measures time to first stop or time per step on a large program (gap G9).
- **Platforms.** RemedyBG and the RAD Debugger are Windows-first, and Instruments is macOS-only. Jet writes ELF only (M4), so none of those debuggers can open a Jet program. A Jet program on Windows or macOS gets no debugger at all today. #6730 c13 asks for Jet meaning "across the platform matrix", and that work begins with a PE or Mach-O image writer.

## Claim 2: one session for the terminal and the editor (D-DBG-DAP1)

**Verdict: the design holds. The build is a small subset of the ratified profile.**

The ballot is strong on paper. It asks for strict framing, a state machine, capabilities that equal behavior, and attach that verifies build ID and source hashes. No peer's adapter promises that much. The ruling also keeps raw frames off by default, and that matches what users want.

M8 shows the built subset after #6730 c23: launch, source breakpoints, step, typed variables one level deep, and disconnect. Its limits:

- **No pause.** A synchronous `continue` means VS Code's Pause button cannot interrupt a hot loop.
- **No conditional breakpoints, hit counts or logpoints.** These are what a RemedyBG or Visual Studio user reaches for first.
- **No evaluate and no nested variables.** A struct local cannot be opened.
- **No attach.** The ballot's verified attach does not exist.

A further attack: VS Code with CodeLLDB already opens a Jet `-O0` image through plain DWARF, with pause, conditions, memory and disassembly views. The Jet adapter wins only on Jet naming. It has to reach the ballot's profile before it beats the generic adapter on daily use. Each missing request has a known-failing row on #6730 (M13); conditions, hit counts, logpoints and evaluate have no row yet (gap G7).

## Claim 3: `jet perf` names the symbols that consume time

**Verdict: it fails.**

The doc's table row promises "which symbols consume wall time, CPU, task, lock, or I/O time". M1 and M2 show one symbol, `run`. Samples are 25 ms polls of a scheduler snapshot that "has no current-function or stack sample". On the probe program, `fib`'s 36% is invisible.

On the same image, with no help from Jet, Linux perf gives a correct call graph and a per-line split of the time (M3). Today a Jet user gets a better profile from perf than from `jet perf`. Tracy, Superluminal and pprof all sample the program counter and the stack at hundreds to thousands of hertz. Tracy also lines the samples up with zones and frames.

The `overhead=not_measured` status (M1) is honest, but it means no capture says what it cost. M5 suggests that `jet perf run` profiles the unoptimized image, so its numbers are not the numbers a release user sees.

The design already names the fix:

- #4727 c4: self and inclusive time per Jet source line, from a sampling profile;
- #4632 c2: inclusive and exclusive time per source location, with recursion counted once;
- #4632 c5: the receipt names the build profile.

What no card says is how to take a sample (gap G1). A real sampler is a `perf_event_open` or `SIGPROF` timer that unwinds frame pointers, which M3 shows are present and correct. It maps program counters through `.debug_line` and `.debug_jetline` to Jet source. The 25 ms snapshot poll cannot be refined into a sampler.

## Claim 4: exports to standard viewers

**Verdict: Chrome and Perfetto holds within stated limits; pprof fails; OpenTelemetry fails.**

`--chrome` writes real Trace Event `X` and `M` events, and the doc lists what stays outside: async flows, counters and screenshots. `--pprof` (M9) writes JSON that names its own loss, "no gzip proto". pprof's value is its shared format, so a projection that `go tool pprof` cannot open gives none of pprof's views. `--otel` is likewise Jet JSON, not OTLP (`observability.md:41`).

Writing `profile.proto` is small once G1 exists: samples carry stacks, locations carry Jet lines, and functions carry Jet names. Until then the export has nothing to carry but two totals (gap G2).

## Claim 5: metaprogram instrumentation, as in Jai

**Verdict: the mechanism holds. The profiler built on it does not exist.**

M10 is the strongest result in this review. A Jet build script hears each function that carries a marker. It rewrites the function's body, and `defer close(zone)` runs on every exit, including an early `return`. That is the core of Jai's `Iprof`, with two differences in Jet's favor:

- Jet's watch is typed (`.Functions{marked: M{}}`), where Jai filters raw messages.
- The rewrite composes in registration order, which the `meta_events_rewrites_compose_in_order` case pins.

The attack:

- **No sink.** A Jai metaprogram can call any timer and draw an overlay. A Jet zone has no Core API to report to, and no path into `.jettrace` (M11). `core.log` spans are log records, and #4632 c3 asks that they reach the trace.
- **No library.** Jai ships `Iprof`; Jet ships no profiling marker library.
- **Limited reach.** The watch covers marked functions and the root package's calls. Instrumenting a dependency's functions, which Jai's plugin does by default, needs that package's consent, and the meta step budget (E0952) is shared with every other handler.

None of these limits is wrong, but together they mean "instrument every function in my program" is not one line. The gap is a Core zone API that writes `.jettrace` zones, plus a shipped `#Profiled` marker library built on M10's pattern (gap G3).

## Claim 6: record once, query `why` and `when` later

**Verdict: it fails today (not built), and the ratified design is deliberately weaker than rr.**

`--record` and `--replay` report "not built" (#6418, M13). The design (`observability.md:163-177`, D-RUN-RECORD1) records "source-level act snapshots" and answers `why <place> == <value>` and `when <place>`. It rules out reverse execution and any standing per-variable history.

rr already gives deterministic replay and reverse execution for any native Linux program, with no compiler support. Pernosco answers `why` across the whole run from that same recording. Jet's design asks the compiler to record acts, which costs work in every backend, and still answers fewer questions than rr plus Pernosco.

Jet's design has real advantages:

- it works where rr cannot: wasm, and hosts without performance counters;
- the replay receipt carries Time authority and frame hashes;
- it can name Jet acts rather than machine writes.

But nothing tests whether rr records a Jet image today; rr is not on this host. If it does, then `jet debug --replay` over an rr trace is the cheapest route to reverse stepping on Linux. Whether to keep the reverse-step deferral is a ballot question, not an implementation detail (gap G8).

## Claim 7: release crashes symbolicate later

**Verdict: it holds within stated limits: a local store on one machine.**

M12 shows build-ID lookup in `.jet/symbols`, which is the same layout gdb, LLDB and perf use. With the in-flight debug-link work (#6730 c5), LLDB also finds the split file by itself. Breakpad- and Sentry-style symbol servers, or a debuginfod lookup, would cover crashes from other machines. No Jet card asks for one. This review leaves that out of scope, because #6841 names no crash-reporting peer.

## Claim 8 (tacit): standard native tools work

**Verdict: it holds on Linux, with one naming gap. It fails on Windows and macOS.**

M3 and M4 show a correct Linux toolchain story: frame pointers, `.eh_frame` and DWARF line tables. perf, LLDB, and (by the same data) Tracy's sampler, Superluminal on Linux and the RAD Debugger's DWARF importer can all work.

The weak spot is naming. Native tools print `prog_fib`, while every Jet tool says `prog.jet#fib` or `fib()`. In a large program, `module_function` joined by `_` can collide with a function whose own name has an underscore, and it never matches the names the user wrote (gap G5).

On Windows and macOS there is no image at all (M4), so RemedyBG, the RAD Debugger, Superluminal-on-Windows and Instruments are out of reach.

## Gaps

| # | Gap | Home |
|---|---|---|
| G1 | A real sampler: `perf_event_open` or a `SIGPROF` timer, frame-pointer unwinding, and mapping program counters to Jet lines through `.debug_line`/`.debug_jetline`. It replaces the 25 ms snapshot poll as the source of profile rows (M1, M2, M3). | #4727 c4 and #4632 c2 own the result. The sampling method is a new criterion on #4727 (filed by Main from this review). |
| G2 | `jet perf export --pprof` writes gzip `profile.proto` that `go tool pprof` opens, and `--otel` writes OTLP. Golden tests run both through the real tools (M9). | New criterion on #4632 (filed by Main from this review). |
| G3 | A Core zone API (enter/close, frame mark, plot value) that writes `.jettrace` zones, and a shipped `#Profiled` marker library built on `meta.rewrite_body` plus `defer close` (M10, M11). | #4632 c3 (log spans in `.jettrace`) plus a new criterion on #4632 for the marker library (filed by Main from this review). |
| G4 | Each capture measures its own overhead against a matched unprofiled run, and profiles the build profile the user ships. `overhead=not_measured` and the `-O0` window (M1, M5) are the current state. | #4632 c5 (the receipt names the profile) plus a new criterion on #4632 for measured overhead (filed by Main from this review). |
| G5 | Native symbol names match Jet names, either through `DW_AT_linkage_name` and `DW_AT_name` or through a symbol table spelling that native tools print unchanged. `prog_fib` cannot collide with another function (M4). | New criterion on #6730 (filed by Main from this review). |
| G6 | `-O2` images show inlined frames and live locals in LLDB (M6). | #6730 c9. |
| G7 | The DAP profile reaches D-DBG-DAP1: pause, attach, restart, conditions, hit counts, logpoints, evaluate, nested variables, exception filters and `showRawFrames` (M8). | #6730 c13 and c14, #6386 c1 (attach) and the known-failing DAP rows. Conditions, hit counts, logpoints and evaluate are a new criterion on #6730 (filed by Main from this review). |
| G8 | Whether reverse stepping stays deferred. Option A keeps D-RUN-RECORD1's deferral. Option B adds an rr-backed `jet debug --replay` on Linux, with forward and reverse stepping over a real recording, and keeps the act snapshots for other hosts. A probe first records a Jet `-O0` image under rr. | New ballot, D-DBG-REVERSE1, for Main to post. Built recording and replay stays with #6418 and #6730 c15-c19. |
| G9 | Time to first stop and time per step are measured on a large program and pinned by a performance gate, as RemedyBG's daily use demands (M7). | New criterion on #6730 (filed by Main from this review). |
| G10 | Allocation rows reach `.jettrace` with call sites, and attach sees the live snapshot (M13). Tracy and pprof both have allocation profiles. | #4632 (the two known-failing perf rows) and #4727 c4 (allocation counts per line). |
| G11 | A Windows (PE with PDB or CodeView) and macOS (Mach-O with dSYM) image, so RemedyBG, the RAD Debugger, Superluminal and Instruments can open Jet programs (M4). | #6730 c13 (Jet meaning across the platform matrix). The image writers belong to the backend platform cards. |
| G12 | Live, Tracy-style viewing of a running program: `jet perf attach` and `jet inspect live` stream zones, tasks and samples while the program runs. | #6386 c2 and c3 (the live view) and #4632 c1 (wait attribution on the timeline). |

Crash symbol servers and the hardware counters of #4727 c1-c3 stay out of scope: #6841 names no peer for them.
