# Embedded and freestanding profile versus the Prelude

Date: 2026-08-28. Card: #2046.

## Result

Jet has a partial runtime-layer model today. RuntimeLayer orders core, alloc, and
hosted. Its comments define core as no heap and no OS, alloc as heap without
direct OS I/O, and hosted as OS I/O, networking, and processes
(crates/jet-foundation/src/RingLayer.rs:1-6, 10-17, 48-104). TargetMachine also
records no_os, memory, linker, allocator, panic, and audit facts, and derives a
maximum runtime layer from no_os and allocator.provides_heap()
(crates/jet-foundation/src/TargetMachine.rs:11-21, 37-58, 579-606).

This is not yet a complete freestanding Prelude. The unconditional embedded
Prelude list contains Foundation Outcome, collections, time, terminal, stream,
and binary-reader parts, while the codegen closure unconditionally appends text,
codec, entropy, DNS, deadlines, sleep, networking, seeded random, and time
parts (crates/jet-codegen/src/Codegen/mod.rs:188-272, 1521-1558). Several of
those parts directly use std I/O, std time, std threads, filesystem calls, or
heap containers. The current freestanding path therefore has a target gate and
an allocator model, but it still needs target-aware source admission and
source emission before it can be a no_std-class build
(crates/jet-codegen/src/Prelude/Term.rs:69-82, 126-155;
crates/jet-codegen/src/Prelude/CoreLib/Top/NetHTTP.rs:800-819;
crates/jet-codegen/src/Prelude/CoreLib/Top/FSIoEnvOsTesting.rs:435-447;
crates/jet-codegen/src/Prelude/Core/Collections.rs:271-380).

The I9-compatible answer is one semantic Prelude with target-selected
capability providers. A target may omit a symbol when sema proves that its
required capability is absent, or bind the symbol to a target provider. AOT,
Cranelift, the interpreter, and web code must still call the same semantic
Prelude function and only marshal values at the engine boundary
(docs/spec/architecture.md:710-736; AGENTS.md:160-177). A second semantic
implementation named “freestanding Prelude” would be an I9 violation unless it
is only a build artifact made from the same source and the same checked
operation contract (docs/spec/architecture.md:680-694, 710-736).

The smallest coherent direction is a hybrid:

1. Split the source closure into core, alloc, and hosted parts.
2. Add target capability facts for resources that do not form a total order,
   such as MMIO, a clock, entropy, a scheduler, and a byte sink.
3. Make sema admit a reachable Prelude closure only when both its runtime layer
   and its target capabilities are present.

This is a research result, not a ratified design. The owner decisions appear at
the end. No ballot was created.

## What the current code already says

The current code has three useful seams.

* D-NOSTD1 says that the std baseline follows the typed target and that
  bare-metal implies no-std. The same decision rejects a separate no_std flag
  (docs/spec/syntax-decisions.md:2939-2940).
* D-WD11 and its target decisions reserve typed target profiles for memory,
  linker, allocator, panic, volatile/MMIO, and audit facts. Freestanding
  allocator and panic facts are required, and sema reads them to reject
  unavailable Core APIs (docs/spec/syntax-decisions.md:4940-4958).
* The current driver derives freestanding from --freestanding or from a
  selected no-OS machine, then runs a freestanding sema pass before codegen
  (Source/main.rs:1507-1561; crates/jet-driver/src/Driver/mod.rs:3383-3408).

The current admission pass is narrower than the target model. It rejects a
fixed list of OS modules in freestanding mode, including files, terminal,
network, tasks, process, time, HTTP, log, and the crypto vault
(crates/jet-sema/src/Sema/CheckerCoreLib/serde_diags.rs:9-19). It records the
Core effect first and then emits E3301 for a forbidden module
(crates/jet-sema/src/Sema/CheckerCoreLib/core_call.rs:1227-1260). List and map
literals also emit E3303 whenever freestanding mode is active
(crates/jet-sema/src/Sema/CheckerInfer/expr.rs:4619-4625, 4911-4918), and the
diagnostic says that a custom allocator is required
(crates/jet-sema/src/Sema/FFI.rs:375-386).

The target model is already richer than a single no_os bit. TargetMachine
contains memory and linker facts, allocator and panic policy, MMIO validation,
and a stable audit JSON shape (crates/jet-foundation/src/TargetMachine.rs:11-21,
60-116). The web decision names wasm32-unknown-unknown with a generated JS
loader, while the networking Prelude has a separate wasm32-wasip2 socket path
(docs/spec/syntax-decisions.md:4177-4186;
crates/jet-codegen/src/Prelude/CoreLib/Top/NetHTTP.rs:800-811). “Wasm” must
therefore not become one undifferentiated freestanding profile.

## Dependency inventory

### Method and legend

This is a source-level inventory of the embedded target closure. It includes
the Foundation Prelude modules and the Rust parts under
crates/jet-codegen/src/Prelude. A row groups symbols with the same prefix when a
file exposes a large family. The cited range contains the named symbols or the
implementation that they call.

The classes are:

* A: heap allocation through String, Vec, Box, Arc, or a map/set.
* IO: stdin, stdout, stderr, terminal, or a byte stream.
* FS: filesystem or filesystem-backed path work.
* T: an ambient wall clock, monotonic clock, time-zone database, or sleep.
* OS: environment, process, exit, signals, or platform startup.
* S: threads, synchronization, or a task scheduler.
* N: sockets, DNS, HTTP, or WebSocket transport.
* E: operating-system or WASI entropy.
* FFI: a native, browser, graphics, or device boundary.

A pure label means that the semantic operation has no ambient resource read. It
does not mean that the current Rust implementation is heap-free. This matters
for the core/alloc boundary.

| Prelude symbol or family | Dependency class | What the source does | Source witness |
|---|---|---|---|
| jet_alloc_error, jet_try_alloc_value | A | Builds the common allocation error and accounts for a requested heap charge. | crates/jet-foundation/src/Outcome.rs:24-58 |
| jet_stream_*, jet_journey_*, jet_notes, jet_present, jet_absent | A, thread-local state | Stores reports, journey hops, notes, and rendered error data in String and Vec values. | crates/jet-foundation/src/Outcome.rs:276-332, 1566-1623 |
| JetReportStyle::for_stderr, jet_terminal_auto_color | IO, Env, A | Reads stderr terminal state and NO_COLOR, FORCE_COLOR, and COLUMNS. | crates/jet-foundation/src/Outcome.rs:915-972 |
| jet_runtime_register_atexit, jet_runtime_drain_atexit, runtime stop helpers | A, OS, IO | Stores process-boundary handlers and reports or exits through the host-facing stop path. | crates/jet-foundation/src/Outcome.rs:1625-1635, 1989-2116; crates/jet-codegen/src/Prelude/Core.rs:630-730, 859-900 |
| jet_panic, jet_arithmetic_stop, jet_todo_stop, jet_runtime_diagnostic, jet_contract_fail | A, IO, OS | Builds or receives a rendered failure report, writes it to the host sink, and exits or unwinds through the runtime boundary. | crates/jet-codegen/src/Prelude/Core.rs:859-997, 1175-1208 |
| jet_reader_*, jet_cursor_*, jet_bin_match_scan | A, pure | Owns or returns copied buffers, Strings, and Rest vectors; it performs no OS read. | crates/jet-foundation/src/StreamCursor.rs:14-35, 301-325; crates/jet-foundation/src/Prelude/MatchScan.rs:13-30, 68-89 |
| jet_list_*, jet_iter_*, jet_map_*, jet_set_*, jet_sorted_set_* | A; S for parallel chunks | Copies, materializes, boxes, or returns collection values. Parallel chunks also use std threads. | crates/jet-codegen/src/Prelude/Core/Collections.rs:6-56, 271-380, 692-750, 1055-1125; crates/jet-codegen/src/Prelude/Core/SetAlgebra.rs:11-45, 83-158 |
| jet_columns_rows, jet_columns_gather, jet_columns_gather_cell | A | Returns row cells and owns Vec<Vec<C>> column storage. | crates/jet-codegen/src/Prelude/Core/Columns.rs:23-73, 96-115 |
| jet_string_concat, jet_std_hex_*, jet_std_b64_*, jet_std_base32_* | A, pure | Produces owned String or Vec results without an ambient resource read. | crates/jet-codegen/src/Prelude/Core/StringConcat.rs:1-6; crates/jet-codegen/src/Prelude/Core/EncodingBase.rs:1-78 |
| jet_text_* | A, pure | Unicode transforms, segmentation, padding, and splitting return owned String and Vec values. | crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs:1-29, 144-226, 387-506, 739-832 |
| jet_std_fs_* in Text.rs, FSIoEnvOsTesting.rs, and FSRuntimeOps.rs | FS, IO, A | Reads, writes, removes, lists, walks, syncs, and mutates files. | crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs:835-970; crates/jet-codegen/src/Prelude/CoreLib/Top/FSIoEnvOsTesting.rs:1-35, 82-170, 172-300, 994-1009; crates/jet-codegen/src/Prelude/CoreLib/Top/FSRuntimeOps.rs:1-33 |
| jet_fs_rename, jet_fs_open, jet_fs_glob | FS, IO, A | Opens files, renames paths, walks directories, and returns glob matches. | crates/jet-codegen/src/Prelude/Core/FSOps.rs:1-45 |
| jet_std_files_open/create/append and jet_std_file_* | FS, IO, A | Wraps File, BufReader, and BufWriter for line reads, writes, and flushes. | crates/jet-codegen/src/Prelude/CoreLib/Top/FileStream.rs:1-125 |
| jet_path_from, jet_path_join, jet_path_parent, jet_path_extension, jet_path_stem, jet_path_normalize | A, pure lexical path work | Uses PathBuf and owned strings for path values, but does not itself touch the filesystem. | crates/jet-codegen/src/Prelude/CoreLib/Top/PathFiles.rs:1-59 |
| jet_path_write_atomic, jet_path_walk | FS, IO, A | Creates temporary files, writes and syncs them, renames them, or walks the filesystem. | crates/jet-codegen/src/Prelude/CoreLib/Top/PathFiles.rs:61-68, 199-275 |
| jet_term_*, jet_std_io_* | IO, Env, OS, A | Detects TTYs, reads and writes standard streams, invokes stty for dimensions, and handles prompts and styles. | crates/jet-codegen/src/Prelude/Term.rs:69-180, 188-257, 260-315; crates/jet-codegen/src/Prelude/CoreLib/Top/FSIoEnvOsTesting.rs:321-413, 435-613 |
| jet_std_env_*, jet_std_io_args, jet_args_*, jet_parsed_* | Env, OS, A | Reads process arguments and environment, builds parsed argument structures, and changes the logical environment. | crates/jet-codegen/src/Prelude/CoreLib/Top/FSIoEnvOsTesting.rs:803-1009; crates/jet-codegen/src/Prelude/CoreLib/Top/Args.rs:78-1000 |
| jet_std_os_* | OS, Env, FS, T, A, FFI | Reads identity, temp paths, process IDs, host data, uptime, and POSIX controls; several calls use direct C functions. | crates/jet-codegen/src/Prelude/CoreLib/Top/OsExtra.rs:1-41, 341-529, 614-701 |
| jet_process_*, jet_process_spec_*, jet_std_process_* | OS, IO, T, S, A, FFI | Builds commands, environment snapshots, pipes, child processes, resource limits, waits, and signal operations. | crates/jet-codegen/src/Prelude/CoreLib/Top/Process.rs:135-225, 392-469, 540-570, 1397-1443; crates/jet-codegen/src/Prelude/CoreLib/ProcessPty.rs:1-9, 51-120 |
| jet_time_monotonic_now_ns, jet_std_time_now, jet_time_now_utc, jet_time_today, jet_time_instant_now | T; Env for replay overrides | Reads Instant or SystemTime. The deadline path also reads replay environment variables. | crates/jet-codegen/src/Prelude/Core/TimeMonotonic.rs:1-10; crates/jet-codegen/src/Prelude/Deadline.rs:13-28; crates/jet-codegen/src/Prelude/CoreLib/Top/MathRandomTime.rs:291-306 |
| jet_time_datetime, jet_time_days_in_month, jet_time_is_leap_year, jet_time_period_*, jet_time_zone_utc | A, pure | Performs calendar and duration work, but current formatting and zone values use owned strings and vectors. | crates/jet-codegen/src/Prelude/Core/Time.rs:28-90, 145-166, 577-597; crates/jet-codegen/src/Prelude/CoreLib/Top/MathRandomTime.rs:308-337 |
| jet_time_zone_named | T, FS, Env, A | Searches environment-selected and standard zoneinfo paths, then reads a TZif file. | crates/jet-codegen/src/Prelude/Core/Time.rs:577-633 |
| jet_std_time_sleep, jet_std_time_sleep_duration_ns, jet_task_timeout_duration_ns, workflow sleep | T, S, OS | Converts Duration to scheduler milliseconds, reads the ambient clock, and waits through the scheduler. | crates/jet-codegen/src/Prelude/CoreLib/Top/TimeSleep.rs:1-69; crates/jet-codegen/src/Prelude/CoreLib/Top/WorkflowSleep.rs:1-10 |
| jet_std_clock_new, jet_std_rng_new, jet_clock_*, jet_rng_* | A for byte/collection results; otherwise pure | Manual Clock and seeded Rng use caller data. Byte and collection results allocate. The system clock constructor is ambient. | crates/jet-codegen/src/Prelude/CoreLib/Top/MathRandomTime.rs:19-105 |
| jet_crypto_entropy_*, jet_std_crypto_random_bytes, jet_crypto_uuid_v4/v7 | E, A, FFI | Allocates output and calls native getrandom or WASI entropy; UUID v4 and v7 require entropy. | crates/jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs:1-3, 214-233, 413-455, 515-580 |
| jet_net_pure_* | A, pure value work | Parses address values and returns strings. It does not open a socket, but current signatures use std::net types. | crates/jet-codegen/src/Prelude/Core/NetPure.rs:1-29 |
| jet_net_*, jet_tcp_*, jet_udp_*, jet_tls_* | N, IO, T, S, A, FFI | Uses std sockets, nonblocking registration, scheduler waits, deadlines, and TLS callback state. | crates/jet-codegen/src/Prelude/CoreLib/Top/NetHTTP.rs:1-6, 800-851, 1978-2035 |
| jet_http_*, jet_app_http_*, jet_ws_* | N, IO, T, S, FS, A | Binds listeners, reads and writes streams, serves static files, uses timers, and owns connection state. | crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs:1-27, 758-835, 2793-2868, 4925-5030; crates/jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs:1-8, 57-94, 294-367, 426-584, 665-691 |
| jet_scheduler_*, jet_task_*, jet_shared_* | S, T, OS, N, A | Uses threads, mutexes, condition variables, channels, timers, panic boundaries, and raw I/O polling. | crates/jet-codegen/src/Prelude/Scheduler.rs:1-10, 128-148, 727-847, 1729-2037, 3019-3114; crates/jet-codegen/src/Prelude/TaskGroup.rs:1-16, 86-145; crates/jet-codegen/src/Prelude/SharedProtocol.rs:20-99 |
| jet_sentry_*, jet_ctx_*, jet_memory_ledger_record | A, S, FS, Env, IO, volatile | Tracks allocations and scopes; memory ledger persistence uses environment, paths, filesystem, and output. | crates/jet-codegen/src/Prelude/Mem.rs:1-45, 127-164, 885-925; crates/jet-foundation/src/MemSentry.rs:1-15, 33-41, 56-120 |
| jet_with_host_program_allocator, jet_host_program_allocator_*, JetProgramAllocator | A, S, hosted OS | Installs a global allocator, uses atomics and mutexes, and delegates to std::alloc::System. | crates/jet-codegen/src/Prelude/ProgramAllocator.rs:1-9, 28-47, 61-85, 260-347, 423-500 |
| jet_interrupt_* | OS, IO, S, T, A, FFI | Installs process signal handlers, stores pending interrupts, and dispatches them through a thread/runtime boundary. | crates/jet-codegen/src/Prelude/CoreLib/Top/Interrupt.rs:1-23, 41-80, 130-180 |
| jet_ui_*, jet_gtk_*, jet_game_*, jet_compute_* | A, FFI, GPU, Browser, OS | Builds UI trees and compute/game state, and exposes GTK, graphics, accelerator, or device boundaries. | crates/jet-codegen/src/Prelude/Ui.rs:1-4, 54-85, 143-220; crates/jet-codegen/src/Prelude/UiGtk.rs:30-50, 94-106, 199-220; crates/jet-codegen/src/Prelude/CoreLib/Top/Game.rs:1-27; crates/jet-codegen/src/Prelude/CoreLib/Top/Compute.rs:1-7, 9-17 |
| jet_browser_* | A, Browser, IO, FFI, OS | Implements native WebDriver BiDi protocol state, JSON values, timeouts, environment/filesystem-backed lock state, and waits. | crates/jet-codegen/src/Prelude/CoreLib/Top/Browser.rs:1-9, 57-90, 182-240, 279-318, 525-652, 1603-1630 |
| jet_ring_log_*, jet_log_*, jet_ring_crypto_* | IO, FS, T, OS, A, E | Logs to configured sinks, formats records, timestamps them, and exposes crypto helpers. | crates/jet-codegen/src/Prelude/CoreLib/Top/RingCsvLogTimeCrypto.rs:166-227, 372-433, 458-555 |
| jet_debug_*, jet_show_io_error | A; IO/OS for error fields | Renders structural values and IO errors into owned strings; the representation itself is pure. | crates/jet-foundation/src/StructuralDebug.rs:46-73, 145-185, 281-290 |

The Foundation compiler registry is a separate case. Prelude.rs uses
LazyLock<Vec<Entry>> and jet_as_bytes allocates, but this is compiler-seam
state, not code emitted into a target program
(crates/jet-foundation/src/Prelude.rs:1-12, 35-76). The target profile must not
confuse compiler memory needs with the generated program's runtime needs.

The inventory exposes three gaps in a simple module table.

1. RingLayer labels core.encoding.hex and core.crypto as core, but
   EncodingBase returns Vec and String. RingLayer labels core.text as alloc,
   while NetPure is semantically pure but names std::net and returns String
   (crates/jet-foundation/src/RingLayer.rs:48-91;
   crates/jet-codegen/src/Prelude/Core/EncodingBase.rs:6-78;
   crates/jet-codegen/src/Prelude/Core/NetPure.rs:3-29).
2. Outcome is a shared semantic carrier, but its current implementation mixes
   allocation, thread-local state, terminal detection, environment reads, and
   process-boundary handlers. A freestanding profile needs to split its
   carrier, allocation, and report-sink parts without changing Outcome meaning
   (crates/jet-foundation/src/Outcome.rs:10-22, 276-332, 922-972,
   1625-1635).
3. The current physical closure is larger than the module gate. Codegen emits
   Term in the base list and emits NetHTTP, time, entropy, and sleep in the
   kernel closure. Target selection must happen before those sources enter the
   generated crate (crates/jet-codegen/src/Codegen/mod.rs:188-272,
   1521-1558).

## Peer survey

### Rust: core, alloc, and std

Rust gives the clearest three-layer vocabulary. core is dependency-free and
does not assume an operating system, heap, threads, or I/O
([core](https://doc.rust-lang.org/stable/core/)). alloc supplies heap-backed
collections and smart pointers, but needs an allocator
([alloc](https://doc.rust-lang.org/stable/alloc/)). std adds the platform
library, including filesystem, networking, process, and thread facilities
([std](https://doc.rust-lang.org/stable/std/)). The no_std attribute removes
std's prelude and uses core's prelude; it does not promise that a program has
no heap ([Rust Reference: no_std](https://doc.rust-lang.org/reference/names/preludes.html#the-no_std-attribute)).

The Embedded Rust Book makes the ownership boundary explicit: firmware must
choose its entry point, panic behavior, and allocator when it uses a heap
([The Embedded Rust Book: no_std](https://doc.rust-lang.org/stable/embedded-book/intro/no-std.html)).

What Jet can steal is the vocabulary, not Rust's crate names. Keep a heap-free
core, a heap-using alloc layer, and a platform layer. Make allocator, startup,
panic, and I/O providers target facts. That maps to Jet's existing
RuntimeLayer and TargetMachine fields
(crates/jet-foundation/src/RingLayer.rs:10-39;
crates/jet-foundation/src/TargetMachine.rs:11-21, 37-58).

### Zig: target facts and explicit allocators

Zig documents freestanding as a target environment, not as a required
language mode. Its target model includes architecture, operating system, ABI,
and feature choices ([Zig: freestanding](https://ziglang.org/documentation/master/#freestanding);
[Zig: targets](https://ziglang.org/documentation/master/#targets)). Zig's
memory-management guidance passes an allocator as a value instead of hiding
one inside a collection API ([Zig: memory management](https://ziglang.org/documentation/master/#memory-management)).

The standard library has target-specific startup and heap branches. Its source
contains an explicit wasm32 freestanding startup path and target-conditioned
heap choices ([std/start.zig](https://github.com/ziglang/zig/blob/master/lib/std/start.zig#L1979-L1985);
[std/heap.zig](https://github.com/ziglang/zig/blob/master/lib/std/heap.zig#L2301-L2318)).

What Jet can steal is explicit target knowledge and allocator ownership. A
typed target profile should say which allocator, linker, startup, and device
boundaries exist. Jet should keep policy and error meaning in the Prelude while
letting a target provider supply the raw allocator or MMIO operation. The
existing typed target fields already point in this direction
(docs/spec/syntax-decisions.md:4940-4958;
crates/jet-foundation/src/TargetMachine.rs:60-116).

### Go: target-specific runtime files

Go does not expose a core/alloc/std triad. The go command selects files through
GOOS, GOARCH, and build constraints
([Go build constraints](https://pkg.go.dev/cmd/go#hdr-Build_constraints)).
The runtime itself contains operating-system-specific files and a distinct
JavaScript/Wasm runtime path
([Go runtime source](https://go.dev/src/runtime/);
[runtime/os_js.go](https://go.dev/src/runtime/os_js.go);
[runtime/rt0_js_wasm.s](https://go.dev/src/runtime/rt0_js_wasm.s)).

The useful lesson is source selection by target fact. A target-specific
implementation can preserve one API contract while replacing startup or an OS
boundary. The risk is also clear: if Jet copies a whole runtime file set into
every generated program, its “only what you call” rule loses force
(docs/spec/architecture.md:680-694). Jet should select a checked Prelude
closure before codegen and keep target providers behind the same symbols.

### TinyGo: target manifests and removable runtime services

TinyGo makes more runtime axes explicit than ordinary Go. Its target option
selects a target description and tool integration. The documented choices
include garbage collection, scheduler, panic handling, serial output, and
target-specific startup ([TinyGo important options](https://tinygo.org/docs/reference/usage/important-options/)).
The target manifest stores values such as GOOS, GOARCH, libc, GC, scheduler,
runtime, linker, and emulator
([TinyGo wasm target](https://github.com/tinygo-org/tinygo/blob/release/targets/wasm.json)).
The bare-metal runtime and the non-OS source split show how device builds
replace host services ([TinyGo bare-metal runtime](https://github.com/tinygo-org/tinygo/blob/release/src/runtime/baremetal.go);
[TinyGo non-OS runtime](https://github.com/tinygo-org/tinygo/blob/release/src/runtime/os_other.go)).

TinyGo's explicit failure modes are useful. gc=none makes allocations fail at
link time, and scheduler=none removes goroutines and channels
(TinyGo important options). Its documentation also treats the machine package
as the hardware-facing layer for microcontrollers
([TinyGo microcontrollers](https://tinygo.org/docs/reference/microcontrollers/)).

What Jet can steal is a target manifest with independent axes. Runtime layer,
allocator, scheduler, startup, panic sink, clock, entropy, and I/O must not be
hidden inside one triple. Jet's TargetMachine already has memory, linker,
allocator, panic, MMIO, and audit axes; it needs the remaining provider facts
if it adopts this shape (crates/jet-foundation/src/TargetMachine.rs:11-21,
60-116).

## Candidate shapes

The candidates differ in where target knowledge lives. They can be combined,
but each has a different primary seam.

### A. Layered Prelude with target-gated sema admission

Shape:

* Put heap-free operations in a core source closure.
* Put String, Vec, maps, and other heap operations in an alloc closure.
* Put terminal, filesystem, process, network, ambient time, entropy, and
  platform services in a hosted closure.
* Have sema compute the transitive helper closure and reject any helper above
  TargetMachine.max_runtime_layer before codegen.

The current RuntimeLayer and max_runtime_layer are nearly the needed control
plane. The missing work is the source partition and helper-level accounting:
the current layer table is module-based, while actual helpers can allocate or
touch the host inside a module
(crates/jet-foundation/src/RingLayer.rs:43-120;
crates/jet-foundation/src/TargetMachine.rs:50-75;
crates/jet-codegen/src/Prelude/Core/EncodingBase.rs:6-78;
crates/jet-codegen/src/Prelude/Core/Time.rs:598-625).

I9 impact: strong if a part is only a compilation unit. Each admitted operation
still has one Prelude implementation, and all engines marshal to that
operation. It fails I9 if the alloc or hosted closure grows an independent
implementation of an existing operation. The codegen comments already define
the required shared-source rule for AOT, JIT, and the interpreter
(crates/jet-codegen/src/Codegen/mod.rs:245-272;
crates/jet-codegen/src/lib.rs:168-193).

Beginner surface: unchanged for hosted files. A freestanding target gives a
clear sema error only when the program reaches an unavailable layer. The
single-file default stays hosted, as required by the no-manifest rule
(docs/spec/philosophy.md:245-254, 256-272).

Expert opt-in: select a typed target profile; set allocator and panic facts;
use core.mem and the existing #Unsafe("reason") gate for raw memory, volatile,
and MMIO. Those target controls already have ratified homes
(docs/spec/philosophy.md:282-289;
docs/spec/syntax-decisions.md:4940-4958).

Migration cost: medium-high. It requires moving unconditional Prelude parts,
splitting Outcome and report sinks, replacing std spellings with core/alloc
providers, auditing every helper closure, and adding all-tier target tests.
The current unconditional source list and kernel closure are the main cost
(crates/jet-codegen/src/Codegen/mod.rs:188-272, 1521-1558).

Main risk: a total-order layer cannot express a board with a heap but no
filesystem, or a WASI target with sockets but no browser. It must be paired
with capability facts.

### B. Effect and authority driven admission

Shape:

* Give each reachable Prelude operation a required resource fact, such as
  Mem.Alloc, IO.Write, Time.Monotonic, Time.Wall, Rand.Entropy, Net.Socket,
  Scheduler.Cooperative, or MMIO.Read.
* Let sema intersect those requirements with target capabilities.
* Continue to use effect rows for semantic effects and ApplicationAuthority for
  grants and denials. Do not use an effect root as a substitute for a heap,
  linker, or ABI fact.

Jet already has one effect table, dotted effect paths, scoped abilities, and
inferred effects
(crates/jet-foundation/src/Effects.rs:1-18, 44-68;
docs/spec/syntax-decisions.md:2458-2504, 2537-2543). It also passes one checked
ApplicationAuthority carrier to every execution tier
(crates/jet-foundation/src/Authority.rs:239-252). This is a good policy seam.
It is not enough by itself: core.text is alloc in the runtime layer, while
effect rows classify semantic side effects rather than every heap operation
(crates/jet-foundation/src/RingLayer.rs:65-91;
docs/spec/syntax-decisions.md:2475-2495).

I9 impact: good for admission and policy, provided that target providers are
called through the same Prelude function. It fails if a JIT host or interpreter
turns a missing capability into its own default, error, or fallback. I9
requires those meanings to stay in the shared Prelude
(docs/spec/architecture.md:710-736).

Beginner surface: potentially the best diagnostics. A program can use the
ordinary API and get “this operation needs Net.Socket, but target board.sensor
provides no Net.Socket” at the call site. The hidden complexity is the
capability table, so the error must name the missing fact and its target.

Expert opt-in: use existing -[Effects]> rows and #Abilities(...) for semantic
authority, plus typed target profile fields for target resources. Do not add a
new user marker until the owner decides whether target capability providers are
source-visible.

Migration cost: high. The current freestanding check is a module-name list,
Core calls record a bare root in the ordinary call path, and allocation
admission is currently a broad freestanding check
(crates/jet-sema/src/Sema/CheckerCoreLib/serde_diags.rs:9-19;
crates/jet-sema/src/Sema/CheckerCoreLib/core_call.rs:1227-1260;
crates/jet-sema/src/Sema/CheckerInfer/expr.rs:4619-4625). Each helper needs a
resource ledger and each provider needs a parity test.

Main risk: overloading effects with implementation requirements. A pure text
transform can require a heap, and a pure calendar operation can be valid on a
board with no clock. Keep semantic effect rows and target resource facts as
different relations.

### C. Separate freestanding Prelude build artifact

Shape:

* Keep one canonical Prelude source registry.
* Build a target-keyed artifact containing only the selected core, alloc, and
  provider parts.
* Let AOT, JIT, interpreter, and web adapters consume the same target-keyed
  source closure or artifact identity.
* Treat a “freestanding Prelude” as packaging, not as a second library with
  alternate semantics.

This fits the current include_str and cached-runtime model, but only if the
selected artifact is derived from one source and its target facts enter the
cache key. The architecture already forbids copied fallback templates and
requires the embedded parts to be canonical
(docs/spec/architecture.md:680-694). The current AOT/JIT/interpreter sharing
pattern shows the acceptable shape: one source, separate marshalling
(crates/jet-codegen/src/lib.rs:168-193).

I9 impact: highest risk. Exact source selection is compatible. A second source
tree, a second error renderer, or a JIT-only freestanding substitute is not.
If a target artifact cannot be used by a relevant tier, the owner must ratify
that tier as inapplicable; an ordinary “JIT later” gap is not acceptable
(AGENTS.md:160-177, 299-305).

Beginner surface: invisible for hosted programs. Experts get clear target
artifacts and smaller binaries. Debugging is harder because the selected
closure, provider set, and artifact digest must appear in the target dossier.
The dossier and stable JSON shape already exist for target facts
(docs/spec/syntax-decisions.md:4953-4958;
crates/jet-foundation/src/TargetMachine.rs:78-116).

Expert opt-in: select a named typed target profile and inspect the exact
source/provider digest. Use existing memory, linker, allocator, panic, and
MMIO controls rather than a new freestanding syntax
(docs/spec/syntax-decisions.md:4940-4958).

Migration cost: high. It changes codegen source selection, runtime caching,
cross-target linking, tier setup, artifact identity, and the test matrix. It
also makes accidental duplicate semantics easier unless the registry is the
only source.

Main risk: packaging can conceal a semantic fork. This shape should follow A
and B, not replace them.

### Tradeoff summary

| Shape | I9 impact | Beginner surface | Expert opt-in | Migration cost |
|---|---|---|---|---|
| A. Layered source closure plus layer ceiling | Strong when parts are source-only; needs capability facts for non-total resources. | Hosted default stays unchanged; missing layer is a sema error. | Typed target profile, allocator/panic facts, core.mem, #Unsafe. | Medium-high source and helper audit. |
| B. Resource facts tied to effects and authority | Strong policy seam; requires strict shared-provider calls. | Best call-site explanation, but hidden capability vocabulary must stay simple. | Existing effect rows and #Abilities plus target facts. | High helper ledger and sema migration. |
| C. Target-keyed freestanding artifact | Compatible only as one-source packaging; duplicate implementations violate I9. | Invisible by default; target artifacts help expert size work. | Named profile, provider/source digest, target dossier. | High build, cache, linker, and parity work. |

The evidence favors A plus B as the semantic design. C can be a later
packaging optimization. A alone misses orthogonal target facts. B alone cannot
make the generated Rust compile without std or a heap. C alone risks the
semantic fork that I9 forbids
(crates/jet-foundation/src/RingLayer.rs:10-17;
crates/jet-foundation/src/TargetMachine.rs:11-21;
docs/spec/architecture.md:710-736).

## Requirements for a real implementation

These are implementation requirements, not new decisions.

* The compiler must classify the transitive emitted Prelude closure, not only
  the imported module. R10 says that codegen emits called helper templates, and
  the current closure contains unconditional parts that cross the proposed
  core boundary (docs/spec/architecture.md:680-694;
  crates/jet-codegen/src/Codegen/mod.rs:1521-1558).
* The allocator contract must distinguish no heap, a supplied heap, and a
  hosted system heap. TargetMachine already models HostedDefault, None,
  Unspecified, Fixed, and Counting policies
  (crates/jet-foundation/src/TargetMachine.rs:579-606). The generated AOT
  allocator must not be emitted for core-only builds
  (crates/jet-codegen/src/Codegen/mod.rs:1776-1800).
* Output and failure need explicit sinks. print, input, eprint, terminal
  reports, and panic reports currently assume standard streams or environment
  facts (crates/jet-codegen/src/Prelude/core/prelude.jet:6-28;
  crates/jet-codegen/src/Prelude/Term.rs:126-155, 260-306;
  crates/jet-foundation/src/Outcome.rs:922-972).
* Time must split pure calendar math, injected Clock, ambient clock,
  monotonic time, time-zone data, and sleep. The code already distinguishes
  deterministic Clock/Rng constructors from ambient time and random calls
  (crates/jet-foundation/src/Effects.rs:13-41, 60-66;
  crates/jet-codegen/src/Prelude/CoreLib/Top/MathRandomTime.rs:19-105).
* Entropy must distinguish deterministic seeded Rng from cryptographic entropy.
  The current crypto provider calls native or WASI operations and fails closed
  when entropy is unavailable (crates/jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs:214-233, 413-455, 515-580).
* Concurrency must name the scheduler model. The current scheduler uses OS
  threads, synchronization, timers, and raw I/O polling
  (crates/jet-codegen/src/Prelude/Scheduler.rs:1-10, 727-847, 1729-2037).
  A cooperative or interrupt-driven board provider must preserve the same
  cancellation, deadline, and failure meanings in the shared Prelude.
* Startup, linker, memory regions, panic, volatile, MMIO, and audit facts must
  remain typed target facts. The target profile decisions already require
  validation before codegen and stable dossier output
  (docs/spec/syntax-decisions.md:4940-4958;
  crates/jet-foundation/src/TargetMachine.rs:60-116).
* Web targets need separate provider sets. Browser DOM, wasm32-unknown-unknown,
  wasm32-wasi sockets, and no-OS firmware do not share the same host boundary
  (docs/spec/syntax-decisions.md:4177-4186;
  crates/jet-codegen/src/Prelude/CoreLib/Top/NetHTTP.rs:800-811).

## Ballot-ready owner decisions

Each item below names the choice an implementation would need. The options are
shaped for a future ballot. No ballot was created by this research.

1. **Runtime vocabulary.** Adopt the existing core/alloc/hosted order as the
   minimum runtime contract, or use only independent capability facts. The
   recommended ballot wording is “hybrid: runtime layer for heap/OS plus
   orthogonal target capabilities.” Acceptance must define core as heap-free,
   alloc as heap-allowed with a supplied allocator, and hosted as OS-backed.
   Current anchors: crates/jet-foundation/src/RingLayer.rs:10-17, 43-120;
   crates/jet-foundation/src/TargetMachine.rs:50-58.

2. **Target selection spelling.** Decide whether embedded profiles are selected
   only through typed target: or targets: profiles and #Target, and whether the
   current --freestanding flag becomes an internal alias or is retired. Do not
   add a second no_std flag while D-NOSTD1 says no no_std flag. Acceptance must
   state precedence, single-file behavior, and the migration path for current
   freestanding callers. Current anchors:
   docs/spec/syntax-decisions.md:2939-2940, 4177-4186, 4568-4579;
   Source/main.rs:1507-1561.

3. **Prelude partition boundary.** Decide the exact membership of core, alloc,
   and hosted. In particular, decide whether Outcome, text, encoding, path
   values, structural debug, and error rendering are split into multiple parts.
   Acceptance must require one semantic source and list every part that a
   core-only generated crate may compile. Current anchors:
   crates/jet-codegen/src/Codegen/mod.rs:188-272, 1521-1558;
   crates/jet-foundation/src/Outcome.rs:10-22, 276-332, 922-972.

4. **Admission relation.** Decide whether sema checks only runtime layer,
   checks a target capability relation, or checks both. If both, define whether
   allocation is represented as Mem.Alloc, a runtime-layer requirement, or
   both. Acceptance must use transitive helper closure and reject before
   codegen. Current anchors:
   crates/jet-sema/src/Sema/CheckerCoreLib/serde_diags.rs:9-19;
   crates/jet-sema/src/Sema/CheckerCoreLib/core_call.rs:1227-1260;
   crates/jet-foundation/src/Effects.rs:44-68.

5. **Target capability vocabulary.** Choose the closed facts for allocator,
   startup, panic sink, IO write/read, clock, monotonic time, sleep, entropy,
   scheduler, socket, filesystem, browser, GPU, FFI, and MMIO. Decide which
   facts are typed target fields and which are effect or authority roots.
   Acceptance must prevent a target triple from silently implying a provider.
   Current anchors:
   crates/jet-foundation/src/TargetMachine.rs:11-21, 60-116;
   docs/spec/syntax-decisions.md:2475-2504.

6. **Allocator and collection behavior.** Decide whether alloc-layer
   collections use a target global allocator, explicit allocator handles,
   arena-only APIs, or a combination. Decide the result for an allocation
   failure and whether core APIs may return owned String or Vec values.
   Acceptance must preserve the one AllocError meaning across AOT, JIT,
   interpreter, and web where applicable. Current anchors:
   crates/jet-foundation/src/TargetMachine.rs:579-606;
   crates/jet-foundation/src/Outcome.rs:24-58;
   crates/jet-codegen/src/Prelude/Core/Collections.rs:293-380.

7. **Output and panic sinks.** Decide what print, eprint, input, terminal
   reports, and panic reports do on a target with no standard streams. Options
   include compile-time rejection, a typed byte-sink provider, a target panic
   hook, or a report buffer. Acceptance must define ordering, failure, and
   whether a missing sink is a sema error. Current anchors:
   crates/jet-codegen/src/Prelude/core/prelude.jet:6-28;
   crates/jet-codegen/src/Prelude/Term.rs:126-155, 260-306;
   crates/jet-foundation/src/Outcome.rs:922-972.

8. **Time model.** Decide which time APIs require a target provider. Separate
   pure calendar and duration operations, injected Clock, ambient wall clock,
   monotonic Instant, zone database, and sleep. Acceptance must define the
   diagnostic and behavior for each missing provider. Current anchors:
   crates/jet-codegen/src/Prelude/Core/Time.rs:1-3, 157-166, 598-633;
   crates/jet-codegen/src/Prelude/CoreLib/Top/TimeSleep.rs:33-69;
   docs/spec/syntax-decisions.md:2557-2564.

9. **Randomness and entropy.** Decide whether deterministic Rng is core or
   alloc, how crypto entropy is supplied on firmware and wasm, and whether
   UUID v4/v7 is admitted without an entropy provider. Acceptance must keep
   deterministic seeded behavior separate from ambient or cryptographic
   randomness. Current anchors:
   crates/jet-codegen/src/Prelude/CoreLib/Top/MathRandomTime.rs:19-105;
   crates/jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs:413-455, 544-580.

10. **Concurrency and scheduler.** Decide whether a freestanding target may
    expose tasks through cooperative scheduling, interrupt-driven scheduling,
    a board runtime, or no scheduler. Decide the semantics of cancellation,
    deadlines, blocking waits, and raw I/O when OS threads do not exist.
    Acceptance must name every applicable execution tier. Current anchors:
    crates/jet-codegen/src/Prelude/Scheduler.rs:727-847, 1729-2037;
    crates/jet-codegen/src/Prelude/TaskGroup.rs:86-145.

11. **Startup and linker contract.** Decide the profile keys and provider
    interfaces for entry symbol, vector table, linker input, memory regions,
    ABI, panic path, allocator placement, and MMIO/volatile. Decide which
    expert controls require #Unsafe and which remain typed profile facts.
    Acceptance must validate all facts before codegen and include provenance in
    the target dossier. Current anchors:
    docs/spec/syntax-decisions.md:4940-4958;
    crates/jet-foundation/src/TargetMachine.rs:60-125.

12. **Wasm target split.** Decide whether wasm32-unknown-unknown browser builds,
    wasm32-wasi builds, and no-OS wasm are separate target profiles or one
    profile with provider capabilities. Acceptance must define browser DOM,
    WASI sockets/files, startup, output, and allocator behavior. Current
    anchors: docs/spec/syntax-decisions.md:4177-4186;
    crates/jet-codegen/src/Prelude/CoreLib/Top/NetHTTP.rs:800-811;
    crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs:784-810.

13. **Execution-tier applicability.** Decide which tiers must run a selected
    embedded target: AOT firmware, host-side interpreter, Cranelift
    simulation, web, or none beyond AOT. Any exception must name the tier and
    target class in an I9 carve-out. Acceptance must not permit an AOT-only
    feature or a durable jit_gaps entry. Current anchors:
    docs/spec/architecture.md:710-736;
    AGENTS.md:160-177, 299-305.

14. **Artifact and cache identity.** Decide whether a target-specific Prelude
    is only a selected source closure or a separately linked artifact. If an
    artifact is allowed, require the canonical source digest, target facts,
    provider identities, linker inputs, and tier parity facts in its cache key
    and audit output. Current anchors:
    docs/spec/architecture.md:650-660, 680-694;
    docs/spec/syntax-decisions.md:4953-4958.

15. **Diagnostics and audit contract.** Decide whether missing layer,
    allocator, sink, clock, scheduler, entropy, or provider gets new
    registered diagnostics or a common target-capability diagnostic. Acceptance
    must keep rustc rejection internal, name the missing capability and target,
    and write the same human and JSON facts. Current anchors:
    crates/jet-sema/src/Sema/FFI.rs:375-386;
    docs/spec/diagnostic-rows.md:460-465;
    docs/spec/syntax-decisions.md:4953-4958.

16. **Compiler-seam boundary.** Decide which Foundation Prelude parts must become
    no_std-compatible because comptime or a compiler seam calls them, and which
    remain compiler-only or hosted runtime parts. Acceptance must preserve I6
    path-only compiler seams and must not make generated target code depend on
    the compiler binary. Current anchors:
    crates/jet-foundation/src/Prelude.rs:1-12, 35-89;
    docs/spec/architecture.md:686-694;
    AGENTS.md:145-158.
