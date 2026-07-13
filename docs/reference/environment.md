# Environment variables

This page is the registry for environment variables that configure Jet tools,
generated Jet programs, and the repository's verification harness. Command-line
flags remain the normal user interface. Environment variables exist for
reproducible automation, process-wide defaults, and controls that a child
process must inherit.

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
| `JET_BENCH_VERBOSE` | `1` | Prints per-sample benchmark output. |
| `JET_CACHE_DIR` | path | Overrides the compiler build-cache directory. |
| `JET_ENV_DISABLE` | any non-empty value | D-ENVHOOK1 escape for the `jet env hook` auto-activation hook: set it to suppress automatic activation in the current shell and drop any env the hook already activated. |
| `JET_FUZZ_CORPUS` | path | Overrides the fuzz corpus directory. |
| `JET_FUZZ_ITERATIONS` | non-negative integer | Sets the generated fuzz-case limit. |
| `JET_FUZZ_SEED` | unsigned integer | Makes fuzz generation reproducible. |
| `JET_FUZZ_TIME_MS` | non-negative integer | Sets the fuzz time budget in milliseconds. |
| `JET_KEYS_DIR` | path | Overrides the signing-key directory used by publishing and secrets tooling. |
| `JET_PROP_SEED` | unsigned integer | Replays a property-test run with the printed seed. |
| `JET_RAYLIB_DISPLAY` | `1` | Enables the real raylib display bridge; without it the bridge stays headless. |
| `JET_REGISTRY_CACHE_DIR` | path | Overrides the local registry-index cache. |
| `JET_REGISTRY_NAME` | registry name | Selects the named registry used by Jetpack doctor checks. |
| `JET_REGISTRY_URL` | URL | Overrides the default registry index. |
| `JET_REPL_HISTORY` | `off` | Keeps REPL history in memory for the current session instead of persisting it. |
| `JET_REPL_HISTORY_LIMIT` | positive integer | Sets the maximum persisted REPL entries. |
| `JET_ROOT` | path | Overrides the installed Jet runtime-data root. |
| `JET_SCHEDULER_THREADS` | positive integer | Sets the generated program's scheduler worker count. |
| `JET_STORE_DIR` | path | Overrides the package-store directory. |
| `JET_TEST_FILTER` | test-name substring | Selects generated `jet test` cases. Prefer the corresponding CLI flag. |
| `JET_TEST_SERIAL` | `1` | Runs generated `jet test` cases serially. Prefer the corresponding CLI flag. |
| `JET_TEST_SHUFFLE_SEED` | unsigned integer | Replays a shuffled generated test run. |
| `JET_TIMING` | `1` | Prints compiler phase timings. |
| `JET_TZDB_DIR` | path | Overrides the timezone database read by generated programs. |
| `JET_UI_HEADLESS` | `1` | Suppresses native UI windows for display-less execution. |

## Maintainer and CI controls

| Variable | Accepted value | Effect |
| --- | --- | --- |
| `JET_CANVAS_PREREQUISITES` | `strict` | Makes Canvas scenario verification fail instead of skip when a prerequisite is absent. |
| `JET_CFFI_ABI` | ABI name | Selects the C-ABI matrix row. Companion `JET_CFFI_CC`, `JET_CFFI_AR`, `JET_CFFI_RUSTC`, `JET_CFFI_RUST_TARGET`, `JET_CFFI_RUST_LINKER`, and `JET_CFFI_RUNNER` name that row's tools and target. |
| `JET_GOLDEN_FILTER` | repository-relative example substring | Selects one golden example. |
| `JET_REQUIRE_RUSTC` | `1` | Fails tests when rustc is unavailable instead of skipping rustc-backed proof. |
| `JET_TEST_JOBS` | positive integer | Sets the repository test-process budget; `scripts/agent/verify-full.sh` defaults it to 16. |
| `JET_UI_FILTER` | repository-relative fixture substring | Selects UI diagnostic or lint fixtures. |
| `JET_UPDATE_GOLDEN` | `1` | Updates the existing expected channel for the selected golden example. Requires `JET_GOLDEN_FILTER`. |
| `JET_UPDATE_SNAPSHOTS` | `1` | Updates compiler-owned snapshot output in explicitly supported workflows. Review the diff immediately. |
| `JET_VERIFY_TMPDIR` | path | Overrides the temporary root used by verification scenarios. |

The Canvas harness also accepts `JET_CANVAS_CHROMIUM` and `JET_CANVAS_NODE`
as explicit tool paths. `JET_CANVAS_PREREQUISITES=strict` is the stable policy
control; resolved-path and proof variables with longer `JET_CANVAS_*` names are
harness transport, not user configuration.

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

Do not build scripts against internal variables. Their names and values may
change with the implementation.
