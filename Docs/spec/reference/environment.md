# Environment variables

This page is the registry for environment variables that configure Jet tools,
generated Jet programs, and repository verification. It is for users writing
reproducible automation and maintainers passing explicit state to child
processes. Command-line flags remain the normal user interface. The executable
name checks are in [`tests/guards/syntax_reconciliation.jet`](../../../tests/guards/syntax_reconciliation.jet),
the effect surface is
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

| Variable | Accepted value | Effect |
| --- | --- | --- |
| `JETPACK_BIN` | executable path | Names the `jetpack` engine that `jet`'s engine verbs (`fetch`, `add`, `env`, `trust`, …) run. The lookup order is `JETPACK_BIN`, then the `jetpack` beside the running `jet`, then `PATH`; a `JETPACK_BIN` that names no executable file refuses with E1228 instead of falling through. `jet test --jetpack=<path>` (the `jetpack` test setting) sets it for every corpus row it runs, which otherwise get the engine this order finds for the jet under test. The name predates the `JET_<AREA>_<SETTING>` convention; it matches the `JETPACK_BIN` that Jetpack's process claims already use. |
| `JET_CACHE_DIR` | path | Overrides the compiler build-cache directory. |
| `JET_CC_PACK_URL` | `https://` or `file://` URL | Names where `jet cc` and `jet env fetch-toolchain` fetch this jet's C toolchain pack when it is neither in the Hangar nor beside jet, instead of the jet release. The pack must still hash to the SHA-256 jet embeds; a `file://` pack is a local copy and works offline. |
| `JET_FFI_CACHE_DIR` | path | Overrides the hidden Rust FFI bridge cache directory. |
| `JET_ENV_DISABLE` | any non-empty value | D-ENVHOOK1 escape for `jet env hook`; suppresses automatic activation in the current shell and drops environment that hook already activated. |
| `JET_FUZZ_CORPUS` | path | Overrides the fuzz corpus directory. |
| `JET_FUZZ_ITERATIONS` | positive integer | Sets the generated fuzz-case limit: the cases each property test runs (200 when unset). `jet test --grade=generated --iterations=<n>` sets it on the test harness (1000 when `--iterations` is unset). |
| `JET_FUZZ_SEED` | unsigned integer | Makes fuzz generation reproducible. |
| `JET_FUZZ_TIME_MS` | non-negative integer | Sets the fuzz time budget in milliseconds. |
| `JET_KEYS_DIR` | path | Overrides the signing-key directory used by publishing and secrets tooling. |
| `JET_PROP_SEED` | integer from 0 to 2^63-1 | Replays a property-test run with the seed its failure report printed. `jet test --grade=generated --seed=<n>` sets it on the test harness (the fixed 0 when `--seed` is unset and the caller sets none). `jet prove` runs every property from this seed, else from one derived from the program's inputs, and names it in each property row as `effectiveSeed`. |
| `JET_RAYLIB_CAPTURE` | path | Writes each frame that `core.game.raylib.end_drawing` presents to the path as a PAM (P7, RGB_ALPHA) image of the canvas. |
| `JET_RAYLIB_DISPLAY` | `1` | Enables the real raylib display bridge; without it the bridge stays headless. |
| `JET_REGISTRY_CACHE_DIR` | path | Overrides the local registry-index cache. |
| `JET_REGISTRY_NAME` | registry name | Selects the named registry used by Jetpack doctor checks. |
| `JET_REGISTRY_URL` | URL | Overrides the default registry index. |
| `JET_REPL_HISTORY` | `off` | Keeps REPL history in memory for the current session instead of persisting it. |
| `JET_REPL_HISTORY_LIMIT` | positive integer | Sets the maximum persisted REPL entries. |
| `JET_ROOT` | path | Overrides the installed Jet runtime-data root. |
| `JET_SCHEDULER_THREADS` | positive integer | Sets the generated program's scheduler worker count. |
| `JET_PACKAGE_STORE_DIR` | path | Overrides the package-store directory. |
| `JET_TEST_FILTER` | test-name substring | Selects generated `jet test` cases; prefer the corresponding CLI flag. |
| `JET_TEST_SERIAL` | `1` | Runs generated `jet test` cases serially; prefer the corresponding CLI flag. |
| `JET_TEST_SHUFFLE_SEED` | unsigned integer | Replays a shuffled generated test run. |
| `JET_TIMING` | `1` | Prints compiler phase timings. |
| `JET_TZDB_DIR` | path | Overrides the timezone database read by generated programs. |
| `JET_UI_HEADLESS` | `1` | Suppresses native UI windows for display-less execution. |

## Maintainer and CI controls

| Variable | Accepted value | Effect |
| --- | --- | --- |
| `JET_CANVAS_PREREQUISITES` | `strict` | Makes Canvas scenario verification fail instead of skip when a prerequisite is absent. |
| `JET_PERF_MAP` | `1` | **Internal** profiling aid for `jet run` and `jet dev`: before a program runs from memory, jet writes `/tmp/perf-<pid>.map` from its image's symbol table, so `perf report` shows its function names (`Compiler/JetBackend/Source/Image/Loader.jet`, `x64_image_perf_map`). Unset, nothing is written. |
| `JET_CFFI_ABI` | ABI name | Selects the C-ABI matrix row. Companion `JET_CFFI_CC`, `JET_CFFI_AR`, `JET_CFFI_RUSTC`, `JET_CFFI_RUST_TARGET`, `JET_CFFI_RUST_LINKER`, and `JET_CFFI_RUNNER` name that row's tools and target. |
| `JET_COMPILER_IDENTITY` | `name@version#build-id` | Names the running compiler's identity, which keys its receipts and versions its target layout. Unset, `jet` reads the one line of the `IDENTITY` file beside its executable; with neither, every compile refuses (E2105) and names both. |
| `JET_GOLDEN_FILTER` | repository-relative example substring | Selects one golden example. |
| `JET_REQUIRE_RUSTC` | `1` | Fails tests when rustc is unavailable instead of skipping rustc-backed proof. |
| `JET_TEST_JOBS` | positive integer | Sets the repository test-process budget. |
| `JET_TOOLCHAIN_ROOT` | path | Names the Jet toolchain source tree, the directory holding `Core/` and `Compiler/Prelude/`, that the Core library bodies are read from. Unset, `jet` uses the nearest directory at or above the input that holds both; with none, it compiles against the Core sources built into it. Checkout tools that read the tree's other files (`jet devtools grammars`, the diagnostic coverage report) refuse (E2105) and name both places. |
| `JET_UI_FILTER` | repository-relative fixture substring | Selects UI diagnostic or lint fixtures. |
| `JET_UPDATE_GOLDEN` | `1` | Updates the existing expected channel for the selected golden example; requires `JET_GOLDEN_FILTER`. |
| `JET_UPDATE_SNAPSHOTS` | `1` | Updates compiler-owned snapshot output in explicitly supported workflows, and the files `core.testing.snap` and `core.testing.golden` compare against (`jet test -u` sets it); review the diff immediately. |
| `JET_VERIFY_TMPDIR` | path | Overrides the temporary root used by verification scenarios. |

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

Canvas verification can receive explicit paths for its browser and driver
processes:

| Variable | Accepted value | Effect |
| --- | --- | --- |
| `JET_CANVAS_CHROMIUM` | executable path | Selects the Chromium executable. |
| `JET_CANVAS_FIREFOX` | executable path | Selects the Firefox executable. |
| `JET_CANVAS_GECKODRIVER` | executable path | Selects the geckodriver executable used with Firefox. |
| `JET_CANVAS_NODE` | executable path | Selects the Node executable used by the Canvas harness. |

`JET_CANVAS_PREREQUISITES=strict` is the policy control. Resolved paths and
proof variables with longer `JET_CANVAS_*` names are harness transport rather
than a second user configuration surface.

## Internal transport and test hooks

These families are listed so a repository search does not make them look like
undocumented user controls:

- `JET_BIN`, `JET_DEV_FILE`, `JET_BUILD_OUTPUT`, `JET_TOOLCHAIN_EXEC`, and
  `JET_TEST_PROOF_REPORT` carry state between Jet-owned processes.
- `JET_COV_OUT`, `JET_PROVE_CHILD_*`, and `JET_REPL_BIN` carry state inside
  coverage, proof, and PTY harnesses.
- `JET_API_CACHE_DIR`, `JET_SCHEMA_CACHE_DIR`, and
  `JET_INLINE_DEPS_FIXTURES` isolate repository tests from user state.
- `JET_CODEMOD_CRASH_*`, `JET_CRYPTO_*_TEST_*`, and variables whose names end
  in `_OBSERVER` or `_SUBPROCESS` are fault-injection hooks.

The test kit starts every case child with `HOME`, `JET_HOME`, and the
`XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_DATA_HOME`, and `XDG_STATE_HOME`
roots under that case's scratch directory. A child whose `HOME` would be the
invoking user's home, or whose `JET_HOME` would be that user's `~/.jet`, is
not started and its row is `REFUSED`, so a suite run never reads or writes the
user's `~/.jet`, configuration, or caches.

Do not build scripts against internal variables. Their names and values may
change with the implementation; use a documented command-line flag or public
control instead.
