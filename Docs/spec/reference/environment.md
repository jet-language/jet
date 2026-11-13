# Environment variables

This page is the registry for environment variables that configure Jet tools,
generated Jet programs, and repository verification. It is for users writing
reproducible automation and maintainers passing explicit state to child
processes. Command-line flags remain the normal user interface. Every variable
has one row in
[`EnvironmentVariables.jet`](../../../Compiler/JetFoundation/Source/Registry/EnvironmentVariables.jet)
(`JET_ENVIRONMENT_VARIABLES`), and the User, Maintainer and Internal tables
below are generated from it by `jet Tools/agent/EnvironmentRows.jet --write`;
[`tests/guards/environment_variables.jet`](../../../tests/guards/environment_variables.jet)
fails on a `JET_*` variable the compiler names with no row. The effect surface is
[`EffectsSurface.jet`](../../../Compiler/JetFoundation/Data/Syntax/EffectsSurface.jet),
and the verification defaults are in
[`Tools/agent/VerifyFull.jet`](../../../Tools/agent/VerifyFull.jet).

## Naming convention

Public Jet controls use `JET_<AREA>_<SETTING>` in uppercase ASCII with words
separated by underscores. A boolean uses `1` unless its row names another
value. Paths are native filesystem paths. Variables labelled **internal** are
compiler-to-child transport or fault-injection hooks: users must not set them,
and they carry no compatibility promise. Tests should use a public control when
one exists instead of adding another spelling for the same job.

## User and automation controls

<!-- BEGIN GENERATED ENVIRONMENT VARIABLES: User (Tools/agent/EnvironmentRows.jet; edit Compiler/JetFoundation/Source/Registry/EnvironmentVariables.jet) -->
| Variable | Accepted value | Effect |
| --- | --- | --- |
| `JETPACK_BIN` | executable path | Names the `jetpack` engine that `jet`'s engine verbs (`fetch`, `add`, `env`, `trust`, …) run. The lookup order is `JETPACK_BIN`, then the `jetpack` beside the running `jet`, then `PATH`; a `JETPACK_BIN` that names no executable file refuses with E1228 instead of falling through. `jet test --jetpack=<path>` (the `jetpack` test setting) sets it for every corpus row it runs, which otherwise get the engine this order finds for the jet under test. The name predates the `JET_<AREA>_<SETTING>` convention; it matches the `JETPACK_BIN` that Jetpack's process claims already use. |
| `JET_BWRAP_URL` | `https://` or `file://` URL | Names where a command that runs build actions (`jet build`, `jet cc`) fetches Jet's pinned bubblewrap when neither the `bwrap` on PATH nor a pinned copy in the Hangar (`<hangar>/tools/<sha256>/bwrap`) or beside jet passes its probe, instead of the jet release. The file must still hash to the SHA-256 jet embeds (`NATIVE_SANDBOX_BWRAP_ASSETS`); a `file://` copy works offline, and `JETPACK_DENY_NETWORK` refuses an `https://` fetch. |
| `JET_CC_PACK_ROOT` | directory | Names an unpacked C toolchain pack (a `jet env fetch-toolchain --json` root) whose clang and llvm-ar build a C or C++ file with `jet run` and `jet build` on every tier, instead of the pack jet's `jetpack` fetches. A Jet program's C closure still takes the pack and its musl sysroot from the `jetpack` engine. `CC` or `AR` still win. A golden `jet test` run with a jetpack sets it for every row. |
| `JET_CC_PACK_URL` | `https://` or `file://` URL | Names where `jet cc` and `jet env fetch-toolchain` fetch this jet's C toolchain pack when it is neither in the Hangar nor beside jet, instead of the jet release. The pack must still hash to the SHA-256 jet embeds; a `file://` pack is a local copy and works offline. |
| `JET_ENV_DISABLE` | any non-empty value | The escape for `jet env hook`; suppresses automatic activation in the current shell and drops environment that hook already activated. |
| `JET_FFI_CACHE_DIR` | path | Overrides the hidden Rust FFI bridge cache directory. |
| `JET_FUZZ_ITERATIONS` | positive integer | Sets the generated fuzz-case limit: the cases each property test runs (200 when unset). `jet test --grade=generated --iterations=<n>` sets it on the test harness (1000 when `--iterations` is unset). |
| `JET_HELP_SHELL_PREFILL` | any value | Set by the `jet env hook` and branded-shell widgets (bare `jet`, Alt-?) when they run bare `jet` with stdout captured: the command picker still opens while stdin and stderr are a terminal, and prints the picked command line on stdout for the shell to put in its edit buffer, never running it. Unset, a picked line is only named on stderr. |
| `JET_KEYS_DIR` | path | Overrides the signing-key directory used by publishing and secrets tooling. |
| `JET_PACKAGE_STORE_DIR` | path | Overrides the package-store directory. |
| `JET_PROP_SEED` | integer from 0 to 2^63-1 | Replays a property-test run with the seed its failure report printed. `jet test --grade=generated --seed=<n>` sets it on the test harness (the fixed 0 when `--seed` is unset and the caller sets none). `jet prove` runs every property from this seed, else from one derived from the program's inputs, and names it in each property row as `effectiveSeed`. |
| `JET_REGISTRY_CACHE_DIR` | path | Overrides the local registry-index cache. |
| `JET_REGISTRY_NAME` | registry name | Selects the named registry used by Jetpack doctor checks. |
| `JET_REGISTRY_URL` | URL | Overrides the default registry index. |
| `JET_REPL_HISTORY` | `off` | Keeps REPL history in memory for the current session instead of persisting it. |
| `JET_REPL_HISTORY_LIMIT` | positive integer | Sets the maximum persisted REPL entries. |
| `JET_ROOT` | path | Overrides the installed Jet runtime-data root. |
| `JET_SCHEDULER_THREADS` | positive integer | Sets the generated program's scheduler worker count. |
| `JET_TEST_FILTER` | test-name substring | Selects generated `jet test` cases; prefer the corresponding CLI flag. |
| `JET_TEST_SERIAL` | `1` | Runs generated `jet test` cases serially; prefer the corresponding CLI flag. |
| `JET_TEST_SHUFFLE_SEED` | unsigned integer | Replays a shuffled generated test run. |
| `JET_TZDB_DIR` | path | Overrides the timezone database read by generated programs. |
<!-- END GENERATED ENVIRONMENT VARIABLES: User -->

## Compiler controls

Each compiler control is one flag, one setting and one variable, spelled once
in [`CLIControls.jet`](../../../Compiler/JetFoundation/Source/Registry/CLIControls.jet).
Its value is its default, then the command settings
(`<command>.<name>`, such as `build.keep_going`, in the user settings file or
the package's `commands:` block, as the CLI reference's "Remembered flags"
section says), then its variable, then its
flag typed on the command line: each replaces the one before. A relative path
in a settings file is under that file's directory. A repeated flag
adds its values within the one place that wins. A boolean variable is on with
`1` or `true` and off with any other value; a boolean flag is on bare or with
`=true`, off with `=off` or `=false`. A list variable separates its values with
commas. `jet env vars` lists every control with its value and the source that
set it, and `jet env vars --command=build -- --trace` shows them as that
command line would see them; `--json` writes them as the `env.vars` status
object. A URL's user and password never appear in that list.

| Flag | Variable | Commands | Accepted value | Effect |
| --- | --- | --- | --- | --- |
| `--keep-going` | `JET_KEEP_GOING` | build, run, check, test, emit | boolean | Goes on past the first failure through every phase, and reports every failure. |
| `--keep-going-report=PATH` | `JET_KEEP_GOING_REPORT` | build, run, check, test, emit | path | Writes the `--keep-going` failure census, one TSV row per failure, to PATH. |
| `--keep-going-skip=ITEM` | `JET_KEEP_GOING_SKIP` | build, run, check, test, emit | list | Leaves ITEM out of the `--keep-going` collector. |
| `--trace[=FILE]` | `JET_TRACE_FILE` | build, run, check, test, emit | bare, `off`, or a path | Streams the compiler's phases and progress to stderr, or to FILE; `off` streams nothing. |
| `--trace-detail=DETAIL` | `JET_TRACE_DETAIL` | build, run, check, test, emit | `phases` or `functions` | Says what the trace shows: the phases, or each function as well. |
| `--phase-counters` | `JET_PHASE_COUNTERS` | build, run, check, test, emit | boolean | Also prints each trace counter on stderr. |
| `--opt-dump=FUNCTION` | `JET_OPT_DUMP` | build, run, test, inspect decisions | list of function names | Prints the slot IR of each named function before and after the optimizer's passes, at O0 (the inline Int facts only), O1 and O2. |
| `--opt-disable=PASS` | `JET_OPT_DISABLE` | build, run, test, inspect decisions | list of optimizer pass names | Skips the named optimizer passes; any other name is refused with the list of passes. |
| `--opt-verify` | `JET_OPT_VERIFY` | build, run, test, inspect decisions | boolean | Checks each function with IR Lint after every optimizer pass. |
| `--opt-ssa-stress` | `JET_OPT_SSA_STRESS` | build, run, test, inspect decisions | boolean | Moves every SSA value out of SSA through a fresh slot. |
| `--gc-trace[=PATH]` | `JET_GC_TRACE` | run and dev (the variable; their flag form is not ported yet), gc report | bare, `off`, or a path | Records bounded automatic GC-promotion evidence; `jet gc report` reads the trace at PATH. |
| `--memory-ledger=PATH` | `JET_MEMORY_LEDGER` | audit memory, fix memory | path | Reads the memory ledger at PATH. |

## Maintainer and CI controls

<!-- BEGIN GENERATED ENVIRONMENT VARIABLES: Maintainer (Tools/agent/EnvironmentRows.jet; edit Compiler/JetFoundation/Source/Registry/EnvironmentVariables.jet) -->
| Variable | Accepted value | Effect |
| --- | --- | --- |
| `JET_CFFI_ABI` | ABI name | Selects the C-ABI matrix row. Companion `JET_CFFI_CC`, `JET_CFFI_AR`, `JET_CFFI_RUST_TARGET`, `JET_CFFI_RUST_LINKER`, and `JET_CFFI_RUNNER` name that row's tools and target. |
| `JET_COMPILER_IDENTITY` | `name@version#build-id` | Names the running compiler's identity, which keys its receipts and versions its target layout. Unset, `jet` reads the one line of the `IDENTITY` file beside its executable; with neither, every compile refuses (E2105) and names both. A code suite of `jet test` (the `packages`, `selfhost` and `perf` profiles of `Compiler`) gets the identity of the jet under test here, so it runs in a shell that sets none. |
| `JET_PERF_MAP` | `1` | **Internal** profiling aid for `jet run` and `jet dev`: before a program runs from memory, jet writes `/tmp/perf-<pid>.map` from its image's symbol table, so `perf report` shows its function names (`Compiler/JetBackend/Source/Image/Loader.jet`, `x64_image_perf_map`). Unset, nothing is written. |
| `JET_READ_SCOPE` | comma-separated paths | **Set by jet** for a program that `jet run --allow='FS.Read("<path>")'`, or the same right in its package's `authority.allow` row, confines to reading those paths: a read the confinement refuses fails with `PermissionDenied`, and its cause names E1803 and these paths (`Compiler/JetRuntime/Source/CompilerFS.jet`). Setting it yourself confines nothing; it only changes that wording. |
| `JET_RECHECK_VERIFY` | `1` | **Internal** compiler test switch: each body comptime pass that lands its local values in the bodies holding them, instead of checking the whole program again (`Compiler/JetSema/Source/Sema/Comptime/LocalSplice.jet`), also checks the whole program again, and any difference between the two results stops the compile as an internal compiler problem that names the first differing rows. |
| `JET_TEST_TOOLCHAIN_ROOT` | path | Names the toolchain tree the rows of a corpus suite (`golden`, `run`, `ui`, `ui_lint`, `cases`) read their Core from; the testkit hands it to each row as its `JET_TOOLCHAIN_ROOT` and passes on no `JET_TEST_TOOLCHAIN_ROOT`. Unset, the rows get the kit's `JET_TOOLCHAIN_ROOT`, else the nearest tree at or above the kit's directory. The `jet` running the kit still reads `JET_TOOLCHAIN_ROOT`, so a newer runner can test an older `jet` against that jet's own tree, as the release gate does for the current release (`Tools/release/Admit.jet`). |
| `JET_TIMING_QUIET` | `1` | Says the machine is quiet enough to time: the run is pinned to idle CPUs, isolated ones when the kernel has them. Cases that measure wall time against a fixed limit (`requires: env:JET_TIMING_QUIET=1`), such as the Wayland 1 ms frame-time case, run only under it and skip elsewhere; their correctness twins run in every batch. `jet Tools/perf/frame-time/Main.jet` sets it and pins the run to the kernel's isolated CPUs (`/sys/devices/system/cpu/isolated`) or, when there are none, to the CPUs its `--cpu LIST` names; with neither it exits 2. |
| `JET_TOOLCHAIN_ROOT` | path | Names the Jet toolchain source tree, the directory holding `Core/` and `Compiler/Prelude/`, that the Core library bodies are read from. Unset, `jet` uses the nearest directory at or above the input that holds both; with none, it compiles against the Core sources built into it. Checkout tools that read the tree's other files (`jet devtools grammars`, the diagnostic coverage report) refuse (E2105) and name both places. |
| `JET_UPDATE_SNAPSHOTS` | `1` | Updates compiler-owned snapshot output in explicitly supported workflows, and the files `core.testing.snap` and `core.testing.golden` compare against (`jet test -u` sets it); review the diff immediately. |
| `JET_VERIFY_TMPDIR` | path | Overrides the temporary root used by verification scenarios. |
| `JET_X64_EMIT_LOG` | comma-separated function names or source path parts (an entry holding `/`) | **Internal** check aid of the x86-64 selector: with `JET_X64_EMIT_LOG_REPORT` also set, the native build (`jet build`, `jet run`, `jet test`) keeps an emission log of each named function. The log holds the slot IR the selector consumed, each slot's checked type, and one row per jump, call, memory operand (its base and index registers), division, string copy and inline-asm template the encoder wrote. Each row is attributed to the IR instruction it lowers (`Compiler/JetBackend/Source/X64/Encoder.jet`, `X64Code`). A conditional jump whose taken side ends the program (a numeric or shift stop) is marked `stop`, with the one slot its condition reads when there is one. Each body the optimizer inlined is an `inline <site> <outer site> <function>` line, and a `sites` line names the site of every IR instruction (0: the function's own code). Before the functions, one `field <struct> <offset> <size> <member> <type>` line per stored member of each struct the module laid out names what an offset reaches and the member's checked type. Unset, nothing is logged. |
| `JET_X64_EMIT_LOG_REPORT` | path | The file the `JET_X64_EMIT_LOG` log is written to, replaced on every native build. A failed write fails the build. |
<!-- END GENERATED ENVIRONMENT VARIABLES: Maintainer -->

Golden stdout remains exact except for probes whose subject is a real host
observation. `foundations/realtime` checks the callback request, cancellation,
completed-frame accounting, and missed-deadline/jitter aggregate invariants.
`foundations/text` keeps every non-timing byte exact and parses only the two
non-negative `elapsed_ms` observations. Their expected files retain stdout
from successful real executions; they do not substitute invented timings or
normalize arbitrary numbers.

The capabilities foundation golden builds its guest Component from canonical
Jet source with the test's compiler before running the host. Its repository-
relative authority root and guest paths are mirrored under disk-backed scratch;
no checkout build artifact or precompiled canned guest can satisfy this probe.
Missing or failed sandbox tools are reported as fixture-build failures, never
as a successful skipped capability boundary.

## Internal transport and test hooks

These are compiler-to-child transport, test-kit isolation and fault-injection
hooks, listed so a repository search does not make them look like undocumented
user controls. A name ending in `*` is a family: every variable starting with
the text before it.

<!-- BEGIN GENERATED ENVIRONMENT VARIABLES: Internal (Tools/agent/EnvironmentRows.jet; edit Compiler/JetFoundation/Source/Registry/EnvironmentVariables.jet) -->
| Variable | Read by | Purpose |
| --- | --- | --- |
| `JETPACK_BOOTSTRAP_*` | `Compiler/test.jet` | The scratch directory, source root and engine areas the Jetpack bootstrap suite hands `Jetpack/Bootstrap/Assemble.jet`. |
| `JETPACK_ENV` | `Compiler/JetFoundation/Data/Syntax/JetpackConfig.jet` | Marks a shell that `jet env` activated. |
| `JETPACK_ENV_DIR` | `Compiler/JetFoundation/Data/Syntax/EffectsSurface.jet` | The `env.jet` directory `jet env hook` last activated, empty for none; a change from it loads or unloads. |
| `JETPACK_ENV_HASH` | `Compiler/JetFoundation/Data/Syntax/JetpackConfig.jet` | The identity of the environment `jet env hook` activated. |
| `JETPACK_ENV_OLD_PATH` | `Compiler/JetFoundation/Data/Syntax/EffectsSurface.jet` | The `PATH` from before activation, restored verbatim on unload. |
| `JETPACK_FAKE_SANDBOX` | `Compiler/JetCli/Source/Cli/RustForeign.jet` | Test hook: refuses the native Linux sandbox probe of the Rust foreign helper. |
| `JETPACK_REF` | `Compiler/JetFoundation/Data/Syntax/JetpackConfig.jet` | The realized package refs inside a Jetpack shell. |
| `JETPACK_ROOT` | `Compiler/JetTestKit/Source/Exec.jet` | Set by the test kit to each case's own Jetpack root under its scratch directory. |
| `JETPACK_TEST_REPO_ROOT` | `Compiler/test.jet` | The checkout a code suite's child runs from. |
| `JET_ADAPTER_MODE` | `Compiler/JetCli/Source/Cli/InspectDecisions.jet` | The adapter mode `jet inspect decisions` hands the canonical pass. |
| `JET_ADAPTER_OPERATION_IDS` | `Compiler/JetCli/Source/Cli/InspectDecisions.jet` | Comma-separated operation ids `jet inspect decisions` hands the canonical pass. |
| `JET_API_CACHE_DIR` | `Jetpack/PackageModel/Source/Store/CapabilityDigest.jet` | Isolates the capability-digest cache of repository tests from user state. |
| `JET_BIN` | `Compiler/test.jet` | The jet under test, handed to the Jetpack bootstrap and to suite children. |
| `JET_BUILD_OUTPUT` | `Compiler/JetCli/Source/Cli/RustForeign.jet` | The output directory inside the Rust foreign build sandbox. |
| `JET_CI_EVIDENCE_DIR` | `Compiler/JetCli/Source/Cli/TestProfile.jet` | Receives each corpus suite's case verdicts as `<suite>.json`. |
| `JET_CODEMOD_CRASH_*` | `Compiler/JetCli/Source/Cli/SourceTransaction.jet` | Fault injection: a source transaction exits after writing its journal or after its renames. |
| `JET_COMPILER_EXTENSION` | `Compiler/JetTestKit/Source/Exec.jet` | Kept out of every test-kit child. |
| `JET_CONFIG_VALUE` | `Compiler/JetFoundation/Source/Encoding/CompilerService.jet` | A configuration value the compiler service hands a child. |
| `JET_COV_OUT` | `Compiler/JetCli/Source/Cli/TestCoverage.jet` | The file a covered test program writes its coverage counts to. |
| `JET_DEVELOPMENT_RECEIPT_CLOSURE_DIGEST` | `Compiler/JetRuntime/Source/ProductionReceipt.jet` | The digest of the locked closure a development build's production receipt records. |
| `JET_DEVELOPMENT_RECEIPT_DIRECTORY` | `Compiler/JetRuntime/Source/ProductionReceipt.jet` | The directory a development build's program writes its production receipt to. |
| `JET_DEVELOPMENT_RECEIPT_ENTRY` | `Compiler/JetRuntime/Source/ProductionReceipt.jet` | The project-relative entry a development build's production receipt names. |
| `JET_DEVELOPMENT_RECEIPT_INPUT_COUNT` | `Compiler/JetRuntime/Source/ProductionReceipt.jet` | The argument count a development build's production receipt records. |
| `JET_DEVELOPMENT_RECEIPT_INPUT_COUNT_DIGEST` | `Compiler/JetRuntime/Source/ProductionReceipt.jet` | The digest of the argument count a development build's production receipt records. |
| `JET_DEVELOPMENT_RECEIPT_INPUT_DIGEST` | `Compiler/JetRuntime/Source/ProductionReceipt.jet` | The digest of the arguments a development build's production receipt records. |
| `JET_DEVELOPMENT_RECEIPT_SOURCE_DIGEST` | `Compiler/JetRuntime/Source/ProductionReceipt.jet` | The digest of the entry source a development build's production receipt records. |
| `JET_DEVELOPMENT_RECEIPT_TARGET` | `Compiler/JetRuntime/Source/ProductionReceipt.jet` | The target a development build's production receipt names. |
| `JET_DEVELOPMENT_RECEIPT_TARGET_DIGEST` | `Compiler/JetRuntime/Source/ProductionReceipt.jet` | The digest of the target a development build's production receipt records. |
| `JET_DEVTOOLS_RELAY_BUILD_ID` | `Compiler/JetCli/Source/Cli/Host.jet` | The build of the devtools relay an observed `jet run` hands its program. |
| `JET_DEVTOOLS_RELAY_REVISION` | `Compiler/JetCli/Source/Cli/Host.jet` | The revision of the devtools relay an observed `jet run` hands its program. |
| `JET_DEVTOOLS_RELAY_SESSION_ID` | `Compiler/JetCli/Source/Cli/Host.jet` | The session of the devtools relay an observed `jet run` hands its program. |
| `JET_DEVTOOLS_RELAY_SOURCE_ID` | `Compiler/JetCli/Source/Cli/Host.jet` | The source of the devtools relay an observed `jet run` hands its program. |
| `JET_DEVTOOLS_RELAY_WORLD_ID` | `Compiler/JetCli/Source/Cli/Host.jet` | The world of the devtools relay an observed `jet run` hands its program. |
| `JET_DEV_PORT` | `Compiler/JetCli/Source/Cli/Host.jet` | The port `jet dev` hands the program it launches. |
| `JET_ENC_HOSTILE_FAIL_READ_AFTER` | `Compiler/JetRuntime/Source/RouteAEncoding.jet` | Fault injection: where the encoding route's reads start to fail. |
| `JET_ENC_HOSTILE_FAIL_WRITE_AFTER` | `Compiler/JetRuntime/Source/RouteAEncoding.jet` | Fault injection: where the encoding route's writes start to fail. |
| `JET_ENC_HOSTILE_INTERRUPT_READS` | `Compiler/JetRuntime/Source/RouteAEncoding.jet` | Fault injection: how many of the encoding route's reads are interrupted (at most 255). |
| `JET_ENC_HOSTILE_INTERRUPT_WRITES` | `Compiler/JetRuntime/Source/RouteAEncoding.jet` | Fault injection: how many of the encoding route's writes are interrupted (at most 255). |
| `JET_ENC_HOSTILE_IO` | `Compiler/JetRuntime/Source/RouteAEncoding.jet` | Turns on fault injection for the encoding route's I/O; the other `JET_ENC_HOSTILE_` variables shape it. |
| `JET_ENC_HOSTILE_READ_ONE` | `Compiler/JetRuntime/Source/RouteAEncoding.jet` | Fault injection: the encoding route reads one byte at a time. |
| `JET_ENC_HOSTILE_READ_PLAN` | `Compiler/JetRuntime/Source/RouteAEncoding.jet` | Fault injection: the comma-separated sizes of the encoding route's reads. |
| `JET_ENC_HOSTILE_WRITE_MAX` | `Compiler/JetRuntime/Source/RouteAEncoding.jet` | Fault injection: the largest write the encoding route makes. |
| `JET_ENC_HOSTILE_WRITE_PLAN` | `Compiler/JetRuntime/Source/RouteAEncoding.jet` | Fault injection: the comma-separated sizes of the encoding route's writes; the last repeats. |
| `JET_EVIDENCE_*` | `Compiler/JetFoundation/Source/Evidence/EvidenceIO.jet` | The report path, report id, toolchain, target, profile and revisions an evidence report records. |
| `JET_FACTS_ANSWER` | `Compiler/JetCli/Source/Cli/InspectFacts.jet` | Where the facts program `jet inspect facts` runs writes its answer. |
| `JET_FFI_TRACE_WORKSPACE` | `Compiler/JetCli/Source/Cli/RustForeign.jet` | With `1`, keeps the Rust foreign workspace for inspection. |
| `JET_FRONT_*` | `Compiler/JetCli/Source/Cli/Request.jet` | The runtime-library object, lifecycle, pack, image and web outputs the toolchain's runtime-pack step asks of a child jet. |
| `JET_HOME` | `Compiler/JetTestKit/Source/Exec.jet` | Set by the test kit to each case's own `.jet` under its scratch directory. |
| `JET_INLINE_DEPS_FIXTURES` | `Jetpack/PackageModel/Source/Scripts/InlineDeps.jet` | Isolates the inline-dependency fixtures of repository tests from user state. |
| `JET_INTERNAL_ROOT_DISPATCH_PID` | `Compiler/JetCli/Source/Cli/InspectB.jet` | The PID of the root `jet` that dispatched an engine process. |
| `JET_LOG` | `Compiler/JetCli/Source/Cli/LogExplain.jet` | The log filter pairs below the command-line filter in `jet log`'s ladder. |
| `JET_MACHINE_OUTPUT` | `Compiler/JetRuntime/Source/ProgressState.jet` | With `1`, a program writes no progress display and keeps its terminal output machine-readable. |
| `JET_NATIVE_TEST_*` | `Compiler/JetCli/Source/Cli/NativeTests.jet` | The test id, report path and records path `jet test` hands each native test child. |
| `JET_OBSERVE` | `Compiler/JetCli/Source/Cli/Host.jet` | Marks a program that an observed run started. |
| `JET_ORG_UNSAFE_POLICY` | `Compiler/JetFoundation/Data/Syntax/CoreSurface.jet` | CI and admin input naming an organization's unsafe-policy file; a configured file that cannot be read fails closed. |
| `JET_PROJECT_ROOT` | `Compiler/JetCli/Source/Cli/Host.jet` | The project root `jet test` hands its test child. |
| `JET_PROVE_CAPTURE_JOURNAL` | `Compiler/JetCli/Source/Cli/ProveReplay.jet` | The journal a program `jet prove` captures appends each raw read to. |
| `JET_PROVE_REPLAY_JOURNAL` | `Compiler/JetCli/Source/Cli/ProveReplay.jet` | The journal of records a program `jet prove` replays reads from. |
| `JET_PROVE_REPLAY_TIME_MS` | `Compiler/JetCli/Source/Cli/ProveReplay.jet` | The wall clock, in Unix milliseconds, a program `jet prove` replays reads. |
| `JET_PROVE_REPLAY_TRACE` | `Compiler/JetCli/Source/Cli/ProveReplay.jet` | The served trace a program `jet prove` replays reads from. |
| `JET_RECEIPT_BYPASS` | `Compiler/JetCli/Source/Cli/CheckReceipt.jet` | When set, `jet check` neither replays nor records a receipt. |
| `JET_RECEIPT_CLAIM` | `Compiler/JetRuntime/Source/ReceiptSections.jet` | The claim a program's receipt sections attach to. |
| `JET_RECEIPT_DIGEST` | `Compiler/JetRuntime/Source/ReceiptSections.jet` | The digest of the receipt a program's receipt sections extend. |
| `JET_RECEIPT_DIR` | `Compiler/JetRuntime/Source/ReceiptSections.jet` | The directory a program's receipt sections are written to. |
| `JET_RECEIPT_WITNESS` | `Jetpack/TrustRoot/Source/CacheReceipts.jet` | The witness new cache receipts record; without it, `ci` under CI, else the local account. |
| `JET_RUST_MUSL_*` | `Compiler/JetCli/Source/Cli/RustForeign.jet` | The musl Rust toolchain, linker and archiver of Rust foreign builds. |
| `JET_SCHEMA_CACHE_DIR` | `Compiler/JetDriver/Source/Driver/Schema.jet` | Isolates the schema cache of repository tests from user state. |
| `JET_SCRATCH_ROOT` | `Compiler/JetTestKit/Source/Kit.jet` | The disk-backed root under which the test kit keeps its scratch and state. |
| `JET_STORE_CAP_BYTES` | `Compiler/JetCli/Source/Cli/Cache.jet` | The byte cap of the build store, reported by `jet cache status` as its source. |
| `JET_STORE_DIR` | `Compiler/JetRuntime/Source/VaultGet.jet` | The store directory a vault audit is rooted in when no test scratch is set. |
| `JET_TEST_CAPTURE` | `Compiler/JetCli/Source/Cli/Host.jet` | The output capture mode `jet test` hands its test child. |
| `JET_TEST_JSON` | `Tools/agent/RuntimeGenerators.jet` | Asks a test program for its results as JSON. |
| `JET_TEST_MEASURE` | `Compiler/JetCli/Source/Cli/Host.jet` | With `1`, a test program measures each test. |
| `JET_TEST_PLAN` | `Compiler/JetFoundation/Source/Registry/TestSettings.jet` | The file each `t.<setting>(…)` call of a test's plan step appends its setting to. |
| `JET_TEST_PROFILE` | `Compiler/JetCli/Source/Cli/TestProfile.jet` | The test profile `jet test` hands each child of a profile run. |
| `JET_TEST_READS_PATH` | `Compiler/JetCli/Source/Cli/TestReuse.jet` | The log a test program appends each file it reads to, for test reuse. |
| `JET_TEST_SCRATCH_DIR` | `Compiler/JetRuntime/Source/VaultGet.jet` | The scratch directory of a test run. |
| `JET_TEST_SETTINGS` | `Compiler/JetFoundation/Source/Registry/TestSettings.jet` | The decided settings of a run, one `name<TAB>value<TAB>source` line each, handed to each child. |
| `JET_TEST_STATE_DIR` | `Compiler/JetTestKit/Source/Kit.jet` | Where the test kit keeps what outlives a run: suite measurements and the FFI cache. |
| `JET_TEST_SUITE*` | `Compiler/JetCli/Source/Cli/TestProfile.jet` | The suite, its rows, package root, filter and memory budget `jet test` hands a code suite. |
| `JET_TEST_TRACE_TIERS` | `Compiler/JetCli/Source/Cli/Host.jet` | With `1`, a test program traces its tiers. |
| `JET_TOOLCHAIN_EXEC` | `Compiler/JetFoundation/Data/Syntax/Predicates.jet` | Marks a child that a pinned toolchain re-executed, so it runs natively instead of re-executing again. |
| `JET_TOOLCHAIN_FIXTURE` | `Compiler/JetFoundation/Data/Syntax/Predicates.jet` | Points toolchain realization at a Hangar object holding prebuilt ring artifacts. |
<!-- END GENERATED ENVIRONMENT VARIABLES: Internal -->

The test kit starts every case child with `HOME`, `JET_HOME`, and the
`XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_DATA_HOME`, and `XDG_STATE_HOME`
roots under that case's scratch directory. A child whose `HOME` would be the
invoking user's home, or whose `JET_HOME` would be that user's `~/.jet`, is
not started and its row is `REFUSED`, so a suite run never reads or writes the
user's `~/.jet`, configuration, or caches.

Do not build scripts against internal variables. Their names and values may
change with the implementation; use a documented command-line flag or public
control instead.
