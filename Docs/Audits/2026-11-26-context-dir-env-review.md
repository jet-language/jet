# Adversarial Design Review: Working Directory and Environment as Context Keys (`#Context(dir:, env:)`)

- **Date:** 2026-11-26
- **Card:** #7016 (greenfield proposal item P18)
- **Scope:** Task-scoped working directory and environment resolution (`#Context(dir:, env:) { … }`, `context.dir`, `context.env`), authority enforcement on relative paths, elimination of process-wide `chdir`/`setenv` mutations and duplicate Core wrappers, diagnostics (`E0761`/`E0762`), and true parallel isolation across tests and tasks.
- **Verdict:** **FAILS AS ORIGINALLY SHIPPED; PASSES ONLY AFTER ALL 5 BINDING CUTOVER FIXES BELOW LAND AND PASS FULL SUITE.**
  The core `CONTEXT_VALUES` chain (`CONTEXT_DIR_KEY = 2`, `CONTEXT_ENV_KEY = 3`, O(1) push/pop, unwind restoration, and `context_inherit` task propagation) is sound, but adversarial review of the original build uncovered **five real defects** — including a live filesystem authority bypass — that must be fixed at the root before #7016 can close.

---

## 1. Strongest Existing Solutions (with Primary Sources)

1. **Plan 9 `rfork(RFNAMEG | RFENVG)`** (Rob Pike et al., *The Use of Name Spaces in Plan 9*, Bell Labs; `fork(2)`):
   Each process/task can fork its own private environment group and filesystem namespace without mutating siblings. Strongest OS-level model, but Linux lacks cheap user-space per-task cwd/env groups without `clone(CLONE_FS)` + heavy thread management. Jet's refcounted `CONTEXT_VALUES` chain achieves Plan 9's per-task isolation in user space for all Jet tasks without syscall overhead.
2. **Rust Edition 2024 `std::env` & `cap-std`** (Rust RFC 3467 / Edition 2024 guide: `std::env::set_var` and `remove_var` marked `unsafe`; `cap-std` `Dir`):
   Rust eliminated data races by marking process-wide env mutation `unsafe` and pushing directory scoping into explicit `cap_std::fs::Dir` handles. Trade-off: explicit `Dir` threading forces every helper signature to take a directory handle, whereas process-wide `std::env::set_current_dir` still breaks parallel `cargo test` runs. Jet keeps ergonomic relative paths inside `#Context(dir:, env:)` while preventing process-wide races.
3. **Go 1.24 `os.Root` and `testing.T.Chdir`** (`https://pkg.go.dev/os#Root`, `https://pkg.go.dev/testing#T.Chdir`):
   `exec.Cmd.Dir` and `Cmd.Env` cleanly isolate spawned subprocesses, and `os.Root` confines relative operations to a directory tree. However, `os.Chdir` remains process-global, so Go's `T.Chdir` explicitly forbids `t.Parallel()` and panics if combined with parallel subtests. Jet's task-scoped `context.dir` lets parallel tasks and tests use different working directories concurrently.
4. **Python `contextlib.chdir`** (`https://docs.python.org/3/library/contextlib.html#contextlib.chdir`):
   Provides block-scoped syntax (`with chdir(path):`), but the Python documentation explicitly warns that it mutates process-wide cwd and is neither thread-safe nor async-task-safe. Jet's `#Context(dir:)` gives the same block ergonomics with true per-task isolation.
5. **Zig `std.fs.Dir`** (`https://ziglang.org/documentation/master/std/#std.fs.Dir`):
   Avoids ambient cwd in favor of explicit `Dir` values (including `std.fs.cwd()`). Strong against accidental cwd dependence, but still leaves `std.process.EnvMap` manual and offers no lexical block scoping for third-party libraries.

---

## 2. Adversarial Attack on Every Claim

### Attack 1 (CRITICAL — Security): Filesystem Authority Scope Bypass on Relative Paths
- **Claim:** Every relative filesystem operation inside `#Context(dir:)` obeys Jet's effect-scope authority checks (`-[FS.Read("<dir>")]>`, etc.).
- **Attack:** **FAILED in the original build.**
  `CompilerFS.jet:59-68` calls `effect_scope_admit_path(path, follow, operation, error)` (`Authority.jet:655`), which passed the raw `path` directly into `effect_scope_resolve` (`Authority.jet:497, 520-526`). `effect_scope_resolve_prefix` called `process_realpath(".")` — resolving relative paths against the **process's real OS cwd**, while the actual OS open (`fd_c_fs_path`) resolved the same relative path against **`context.dir`**!
  Consequently, a function confined to `-[FS.Read("<allowed>")]>` running with OS cwd `<allowed>` could execute `#Context(dir: "<outside>") { files.read("secret.txt") }`: `effect_scope_admit_path` checked `<allowed>/secret.txt` (admitted!), then `fd_c_fs_path` opened `<outside>/secret.txt` (bypass!). The same hole affected `FilesPathOps.jet`, `RouteAFiles.jet`, and `HostHeldDirectory.jet`.
- **Root fix (binding):** Resolve `path` via `context_dir_resolve_owned(path)` at the entry of `effect_scope_admit_path` (`Authority.jet:655`) before calling `effect_scope_resolve`, and pin an escape-attempt test (`Tests/Cases/Context/context_dir_cannot_escape_fs_effect_scope.case`) proving `#Context(dir: outside) { files.read("secret.txt") }` inside a restricted `FS.Read(allowed)` scope fails with `Jet.EffectBoundExceeded` (`PermissionDenied`).

### Attack 2: Core Environment Bypass in `core.time`
- **Claim:** No Core API reads process-wide environment outside task context.
- **Attack:** **FAILED in the original build.**
  `Core/Time/Time.jet:2093-2118` calls `rt.zone_env("JET_TZDB_DIR" | "TZDIR" | "JET_ROOT" | "TZ")` -> `HostTime.jet:203` (`jet_time_raw_zone_env`), which read `rt_env_get_lossy(name)` directly from process startup `environ`, ignoring `#Context(env:)` overrides and unsets completely.
- **Root fix (binding):** Make `jet_time_raw_zone_env` (`HostTime.jet:203`) consult `context_env_lookup(rt_text_data(name), rt_text_len(name))` first (honoring both overrides and tombstones) before falling back to `rt_env_get_lossy(name)`.

### Attack 3: Duplicate Wrapper Copies Left Behind
- **Claim:** Duplicate `cwd`/`chdir`/`env`/remove wrappers over `core.sys` and `core.files` are deleted (I8, greenfield cutover).
- **Attack:** **FAILED in the original build.**
  The original submission left all duplicate wrappers alive in `Core/Files/Files.jet` (`rmdir`, `unlink`, `getcwd`, `chdir`, `getenv`, `delete`, `rm`, `pwd`, `cd`) and `Core/Sys/Sys.jet` (`getenv`), along with their `Compiler/Prelude/Core.rows` entries, conformance stubs, and callers.
- **Root fix (binding):** Delete all 10 duplicate wrappers from `Core/Files/Files.jet`, `Core/Sys/Sys.jet`, and `Compiler/Prelude/Core.rows`; migrate every in-repo caller (`IntegLand4.jet`, `CensusSnap2.jet`, `LibcCensus.jet`, `EnumsetBench/check.jet`, `TraceViewer/view.jet`, `TargetSurfaces.jet`, `WebHostDiagnosticFixtures.jet`, `ContextDirEnvScopingAndParallelIsolation`, `NeverNeverCallsFillFallbackSlotsOnEveryTier`, `spec.md`, `core-library.md`); delete the 10 retired conformance stubs under `Tests/Conformance/Corpus/Core/{Files,Sys}/`.

### Attack 4: Diagnostics `E0761` / `E0762` Omitted `dir` and `env`
- **Claim:** Diagnostics accurately describe `#Context` fields (I4).
- **Attack:** **FAILED in the original build.**
  `Compiler/Prelude/Diagnostics.jet:11018-11059`, `Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet:9369-9403`, and `Compiler/JetSema/Source/Sema/Statements/ContextBlocks.jet:68` still listed only `allocator`, `logger` (and `on_stop`), omitting `dir` and `env`.
- **Root fix (binding):** Update `E0761` and `E0762` in `Diagnostics.jet`, `DiagnosticRows.jet`, and `ContextBlocks.jet` to name `dir` and `env`, and update `Tests/UI/context_unknown_field.stderr`.

### Attack 5: Claimed Parallel Test Proof Ran Sequentially
- **Claim:** Two tests setting different directories run in parallel without interference.
- **Attack:** **FAILED in the original build.**
  `Compiler/JetRuntime/Source/TestHarness.jet:149-158` (`jet_rt_test_spawn`) calls `coll_call1(invoke, env)` synchronously before returning, so `#Test` blocks in a single batch ran strictly one after another. Moreover, `ContextDirEnvScopingAndParallelIsolation.fixture/main.jet` had no synchronization barrier proving that concurrent tasks overlapped in time while holding distinct `context.dir` / `context.env` values.
- **Root fix (binding):** Add a true two-party channel rendezvous barrier inside the concurrent `task.all` proof (so sequential execution would deadlock, proving real concurrent overlap while both tasks hold different `context.dir` and `context.env` values and perform relative file I/O and subprocess spawns), AND prove parallel `#Test` execution across separate processes / threads with distinct directories.

### Attack 6: Nested Relative `#Context(dir:)`, Env Shadowing, Non-Path `fd_c_path`, Pure Path Ops, `#Memo`/Comptime
- **Nested relative `#Context(dir:)`:** Resolved against enclosing `context_dir_current()` (or OS `getcwd`) and normalized at entry (`Context.jet:920`). **Holds.**
- **Nested `#Context(env:)` inheritance & tombstones:** Single-key lookup walks `CONTEXT_ENV_KEY` innermost-first with `CONTEXT_ENV_UNSET` tombstones; whole-env snapshots overlay outermost-to-innermost onto `logical_environment_current()`. **Holds.**
- **Non-path `fd_c_path` callers (DNS, xattr, usernames):** Separate `fd_c_fs_path` used only at FS path boundaries; non-path callers keep `fd_c_path`. **Holds.**
- **Pure lexical path ops (`files.join`, `dirname`, `basename`, `normalize`):** Remain pure; only OS-touching leaves and `cwd`/`resolve`/`absolute` consult `context.dir`. **Holds.**
- **`#Memo` & comptime:** `context.dir` / `context.env` route via `THostCall.ContextGet`, refused at comptime (`E0643`) and in C callbacks (`E3218`), and tracked in `#Memo` keys. **Holds.**

---

## 3. Binding Cutover List

Every item below MUST land and pass before #7016 closes:

1. **Authority hole fix + escape test:**
   - `Compiler/JetRuntime/Source/Authority.jet` (`effect_scope_admit_path` resolves via `context_dir_resolve_owned`)
   - `Tests/Cases/Context/context_dir_cannot_escape_fs_effect_scope.case` + fixture
2. **`core.time` context-env fix:**
   - `Compiler/JetRuntime/Source/HostTime.jet` (`jet_time_raw_zone_env` checks `context_env_lookup` first)
3. **Duplicate wrapper deletion & caller cutover (10 wrappers deleted):**
   - `Core/Files/Files.jet`: delete `rmdir`, `unlink`, `getcwd`, `chdir`, `getenv`, `delete`, `rm`, `pwd`, `cd`
   - `Core/Sys/Sys.jet`: delete `getenv`
   - `Compiler/Prelude/Core.rows`: remove all 10 removed names from `core.files` and `core.sys` exports/owns
   - Migrate callers: `Tools/Agent/Orchestrator/{IntegLand4,CensusSnap2}.jet`, `Tools/Agent/LibcCensus.jet`, `Tools/Gauntlet/Entries/EnumsetBench/check.jet`, `Tests/Cases/WebBrowser/Fixtures/TraceViewer/view.jet`, `Tests/Cases/Context/ContextDirEnvScopingAndParallelIsolation.fixture/main.jet`, `Tests/Cases/NeverReturn/NeverNeverCallsFillFallbackSlotsOnEveryTier.fixture/main.jet`, `Compiler/JetSema/Source/Sema/TargetSurfaces.jet`, `Compiler/JetSema/Tests/WebHostDiagnosticFixtures.fixture/WebHostDiagnosticFixtures.jet`, `Docs/Spec/spec.md`, `Docs/Spec/Reference/core-library.md`
   - Delete retired stubs: `Tests/Conformance/Corpus/Core/Files/{rm,getcwd,chdir,pwd,cd,getenv,rmdir,unlink,delete}.jet`, `Tests/Conformance/Corpus/Core/Sys/getenv.jet`
4. **Diagnostics `E0761` / `E0762` alignment:**
   - `Compiler/Prelude/Diagnostics.jet`
   - `Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet`
   - `Compiler/JetSema/Source/Sema/Statements/ContextBlocks.jet`
   - `Tests/UI/context_unknown_field.stderr`
5. **True concurrent overlap proof:**
   - `Tests/Cases/Context/ContextDirEnvScopingAndParallelIsolation.fixture/main.jet` (rendezvous barrier proving simultaneous overlap of distinct `context.dir` / `context.env` tasks)
6. **Full suite verification:**
   - Batch compiler rebuild (`S1 -> S2 -> S3`), targeted cases (`Context/`, `EnvOverlay/`, `CoreDeclarations`, `ContextDirEnv` golden O0/O1/O2), and full suite (`golden`, `ui`, `cases`, `Tests/Guards`).
