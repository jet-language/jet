# Build server protocol: Zig's design, Jai's build-as-code, and Jet

Date: 2026-10-30. Card #6322, criteria 1 and 2. Zig sources are Zig 0.16.0's
`lib/zig` tree (`/nix/store/kbb1kfzlvqiwbcpw4sd2j3v63vqab3vg-zig-0.16.0`).
Jet probes ran with `$JET_SCRATCH_ROOT/discovery/base/latest/jet`
(bd83fe6361) against a dev export of the same commit. Probe files are in
`$JET_SCRATCH_ROOT/lanes/MemDesign/probes/`.

## Zig

### The compiler server

`zig build` compiles `build.zig` into an ordinary program, the build runner
(`compiler/build_runner.zig`), and runs it. The build runner then starts the
compiler once per compile step as `zig build-exe ... --listen=-` and talks to
it over stdin and stdout (`std/Build/Step.zig:404-600`, `evalZigProcess`).

Each message is a header `{tag: u32, bytes_len: u32}`, little-endian, then
the body (`std/zig/Server.zig`, `std/zig/Client.zig`).

| Direction | Messages |
|---|---|
| Client to compiler | `exit`, `update`, `run`, `hot_update`, `query_test_metadata`, `run_test`, `start_fuzzing`, `new_fuzz_input` |
| Compiler to client | `zig_version`, `error_bundle`, `emit_digest`, `test_metadata`, `test_results`, `test_started`, `file_system_inputs`, `coverage_id`, `fuzz_start_addr`, `fuzz_test_change`, `broadcast_fuzz_input`, `time_report` |

What they do:

- `update` asks for one rebuild. Without `--watch` the runner sends `exit`
  right after (`Step.zig:550-551`).
- `error_bundle` carries every diagnostic of the update as one binary bundle.
- `emit_digest` names the output by hash and says whether it was a cache hit.
- `file_system_inputs` lists every file the compiler read, each with a prefix
  for its root (cwd, Zig lib, local cache, global cache). The runner watches
  exactly those files.
- `hot_update` swaps code into a running process.
- `time_report` sends per-phase timing.

### Watch and rebuild

`zig build --watch` keeps compiler processes alive between rebuilds
(`build_runner.zig:571-640`). After a change it waits a debounce interval,
50 ms by default (line 128) and set by `--debounce <ms>`, and then reruns every step
whose inputs changed, plus their dependents. On Linux it uses fanotify,
keyed by mount (`std/Build/Watch.zig:43-60`). `--watch` stops with "not yet
implemented" where no watcher exists (line 548). `-fincremental` lets a warm compiler
redo only changed declarations.

### Step graph, web UI and cache

`--webui` serves a page over WebSocket (`std/Build/abi.zig`). The page sees
the build status (idle, watching, running, fuzz setup) and each step's state
(pending, running, success, failure). It can ask for a rebuild. It also shows
fuzzing and time reports. `--time-report` prints the same timing.

The cache (`std/Build/Cache.zig`, 1599 lines) hashes each step's inputs into
a manifest. Its header says it "is not designed to withstand attacks". There
is no remote cache.

## Jai

Jai's build program is ordinary Jai run at compile time by `#run`. It creates
a workspace with `compiler_create_workspace`, sets options, and calls
`compiler_begin_intercept`. It then loops on `compiler_wait_for_message`.
Messages report files, imports, typechecked code and phases. The loop can add
code with `add_build_string` while the program is still compiling. Sources:
the Jai Community wiki, "Metaprogramming"
(`https://github-wiki-see.page/m/Jai-Community/Jai-Community-Library/wiki/Metaprogramming`),
and "The Way to Jai", chapter 30B
(`https://github.com/Ivo-Balbaert/The_Way_to_Jai/blob/main/book/30B_Manipulating_the_build_process.md`).

## Related protocols

LSP 3.17 sends diagnostics with `textDocument/publishDiagnostics`, file
changes with `workspace/didChangeWatchedFiles`, and progress with `$/progress`
(`https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/`).
The Build Server Protocol adds build targets, `buildTarget/compile`,
`build/taskStart`, `build/taskProgress` and `build/taskFinish` over JSON-RPC
(`https://build-server-protocol.github.io/docs/specification`).

## Jet: ratified against built

Ratified decisions already cover most of what Zig's server does:

| Topic | Ratified | Built on bd83fe6361 |
|---|---|---|
| Wire format: one JSON message per line, `jet.session.v1` | D-SESSION-WIRE1 | No |
| `--listen=-` on check, build, test, run and dev; `update` redoes the command's work | D-SESSION-ENTRY1 C, D-SESSION-UPDATE1 A | No: `jet build --listen=-` is E2102, "`--listen` isn't a flag jet understands" |
| Joining a workspace session, `--session=auto\|off\|required` | D-SESSION-UPDATE1 A, D-BUILD-SESSION1 A | No: `--session=auto` is E2102 |
| Session owners `jet dev --serve` and `jet self lsp --serve` | D-SESSION-OWNER1 | `jet dev --serve` starts and watches; `jet self lsp` is E2104, "is not built yet" |
| One public event enum used by hooks, `--json` and the wire | D-COMPILER-EVENT1 | No |
| Hooks on compiler moments, Jai-style (imported, settled, link, before and after write, finished) | D-HOOK-STAGES1, D-JAI-EVENTS1 | No: no `#On(` anywhere in the tree; `jet inspect hooks` is E2101 |
| Unchanged builds reuse checks and print a receipt | D-BUILD-NOCHANGE1 | No: three builds of one file took 1420, 669 and 988 ms and printed no receipt |
| `--no-cache`, `--cache=off`, `b.cache(false)` | D-BUILD-NOCHANGE1, D-BUILD-REUSESET1 | No: `--no-cache` and `--cache=off` are E2102 |
| `jet build --verify` | D-BUILD-NOCHANGE1 | No: E2104 |
| Remote cache through Jet's own protocol | D-REAPI1 | Not probed |
| Hot swap in `jet dev` | D-NATIVEUI-DEV1 C | Not probed |
| Phase timing | D-HOOK-STAGES1 (`--timings`) | `--trace` prints each phase with time and memory |

What is built:

- `fn build(b: Build)` runs in the compile-time evaluator. The `Build` value
  carries a session id (`Compiler/JetEval/Source/BuildServices.jet:17-28`).
  Its methods are served by `Compiler/JetBuildSession/Source/Session.jet:343-405`:
  `add_*`, `plan`, `generate`, `contribute`, `embed`, `fetch`, `find`, `pin`,
  `probe`, `plugin`, `signing`, `toolchain` and `action`.
- The build writes facts the program reads while compiling.
  `Examples/features/packages/build_contribution/run.jet` contributes
  `cache_slots = 4` in `fn build`, and `run` prints
  `$build.settings.cache_slots`. The golden output says `4`
  (`Examples/features/expected/packages/build_contribution.out`), but
  `jet run` on the file or its directory prints `1`, the package default, on
  o0 and o2.
- `core.build` projects a checked plan into a `BuildGraph` and diffs two
  graphs, with no file reads (`Core/Build/Build.jet:17-21, 98-160`).
- `core.compiler` exposes `lex`, `parse` and `check` to compile-time code.
  `Examples/features/foundations/tooling/run.jet` uses them, but `jet run`
  and `jet build` on it stop with an internal compiler error on o0 and o2:
  "the comptime evaluator can't build `LEXED`".
- Watching exists for `jet run --watch`, `jet test --watch` and `jet dev`.
  `Compiler/JetDevserver/Source/WatchService.jet:130` uses inotify.
- `jet build --watch` is accepted. It builds once, exits 0 and does not
  watch, and nothing says the flag was ignored (probe `ub/shl_63.jet`).
  D-RUN-WATCH1 gives `--watch` to run, test and check, not build.
- `jet build --json` prints nothing on a successful build.

## Keep and drop

Keep, because Jet needs the same thing:

- The compiler as a long-lived server over stdin and stdout. Already ratified.
- One structured report per diagnostic. Jet's `jet.report.v1` lines play the
  part of Zig's error bundle.
- A watch set equal to the files the compiler read. D-BUILD-NOCHANGE1 already
  requires recording every read for the reuse key, so the watch set is free.
- Reporting whether each output was reused. The D-BUILD-NOCHANGE1 receipt
  counts reuse.
- `jet build --watch`, which Zig has and Jet accepts but ignores. Ballot
  D-BUILD-WATCH1.
- Per-step progress for tools. Zig shows it only in its web UI. Ballot
  D-BUILD-PROGRESS1.

Drop, with reasons:

- A binary wire. D-SESSION-WIRE1 chose JSON lines; the session carries
  control messages, not bulk data.
- A separate build web UI. Canvas (`jet dev --canvas`) already shows a
  session in the browser.
- Fuzz control over the build protocol. Fuzzing belongs to `jet test`.
- A build runner compiled as a separate program. Jet's `fn build` runs in the
  compile-time evaluator, as Jai's does, so build code and program code share
  one evaluator and one set of facts.

## How the build loops into compile-time code

Zig's `build.zig` is a separate program, and only passes options into the
compile as a generated module. Jai's build is compile-time code that watches
and changes the compile while it runs. Jet chose Jai's shape:

1. `fn build` runs in the compile-time evaluator and writes facts
   (`b.contribute`) and generated modules (`b.generate`).
2. Program code reads those facts while compiling through `$build`.
3. Hooks on compiler moments (D-HOOK-STAGES1, D-JAI-EVENTS1) let build code
   see each settled round and add code until no new items appear, like Jai's
   message loop.
4. The same moments stream to tools as `compiler.Event` values over `--json`
   and the session wire (D-COMPILER-EVENT1).

Steps 1 and 2 are built, though the contribution example prints the default
instead of the contributed value. Steps 3 and 4 are ratified and not built.
