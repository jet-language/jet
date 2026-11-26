# Design Review: Working Directory and Environment as Context Keys (`#Context(dir:, env:)`)

- **Date:** 2026-11-26
- **Scope:** Proposal item P18 (`#Context(dir:, env:)`), task-scoped working directory and environment resolution across Core, JetRuntime, and Compiler, parallel test isolation, and process-wide mutation cutover.
- **Verdict:** **PASS WITH CUTOVER CONDITIONS** — Jet's existing task-owned `#Context` value chain (`Context.jet`, `BuildTransact.jet`, `ContextKeys.jet`) already provides O(1) push/pop, unwind restoration, and `context_inherit` task propagation without growing `CONTEXT_RECORD_BYTES` (`48` bytes). The hard engineering work is (1) closing every relative-path and environment bypass in JetRuntime without breaking non-path callers of `fd_c_path`, (2) resolving the cutover of process-wide `chdir` / `set` / `unset` mutators so no Core API mutates shared process state outside task context, and (3) supporting the `{CC: "clang"}` record-literal shorthand alongside `[String:String]` in `#Context(env: ...)`.

---

## 1. Current Inventory: Every Site Holding `cwd`, `chdir`, `env`, `getenv`, `setenv`, `spawn-cwd`, and `spawn-env`

### 1.1 Core Libraries (`Core/`)

| Subsystem | File & Line | Symbol(s) | Current Mechanism | Gap / Hazard Today |
| :--- | :--- | :--- | :--- | :--- |
| System env (read/write) | `Core/Sys/Sys.jet:64-109, 2121` | `get`, `set`, `unset`, `vars`, `environment`, `getenv`, `__env_snapshot`, `__env_present` | Calls `sys_rt.env_value`, `sys_rt.env_set`, `sys_rt.env_remove`, `sys_rt.env_columns` (process-wide `logical_env_root` or World cell) | `set`/`unset` mutate process-wide `logical_env_root` across concurrent tasks; reads do not consult `#Context(env:)`. |
| System cwd (read/write) | `Core/Sys/Sys.jet:312, 1492` | `current_dir`, `set_current_dir` | Calls `sys_rt.current_dir` (`getcwd(3)`) and `sys_rt.set_current_dir` (`chdir(2)` syscall 80) | `set_current_dir` mutates kernel process-wide CWD, racing with every concurrent task and test. |
| System XDG / expand | `Core/Sys/Sys.jet:798, 1500+` | `expand`, `home_dir`, `config_dir`, `cache_dir`, `data_dir`, `temp_dir` | Reads env via `sys.get` / `sys_rt.env_value` | Inherits context automatically once `sys_rt.env_value` / `logical_env` consults `CONTEXT_ENV_KEY`. |
| Files cwd/chdir/env aliases | `Core/Files/Files.jet:2730, 2785, 2787, 2793, 3041, 3049` | `cwd`, `getcwd`, `chdir`, `getenv`, `pwd`, `cd` | Delegates directly to `sys.current_dir`, `sys.set_current_dir`, `sys.get` | `chdir` and `cd` are direct duplicate mutators of process-wide CWD; must follow the same cutover as `sys.set_current_dir`. |
| Files relative path ops | `Core/Files/Files.jet:619-642, 1202-1955, 2404-2549` | `open`, `create`, `append`, `read`, `read_bytes`, `write`, `write_bytes`, `exists`, `is_file`, `is_dir`, `is_symlink`, `stat`, `lstat`, `metadata`, `list_dir`, `mkdir`, `create_dir_all`, `remove`, `remove_dir`, `remove_all`, `rename`, `copy`, `symlink`, `hard_link`, `read_link`, `canonicalize`, `resolve`, `absolute`, `walk`, `glob`, `lock`, `truncate`, `fsync` | Passes raw relative strings to `jet.runtime.files` leaves; `resolve`/`absolute` join against `sys.current_dir()` | Relative paths pass directly to kernel syscalls (`open`, `stat`, `unlink`, `mkdir`, `rename`, `getdents64`) using the kernel's process-wide CWD instead of `context.dir`. |
| Path helpers | `Core/Files/Path.jet:50` | `Path.cwd`, `Path.home` | Calls `files.cwd()` and `rt.path_home` (`RoutePath.jet`) | `RoutePath.jet:328` calls `jet_sys_current_dir_raw` directly; fixed once `jet_sys_current_dir_raw` checks `context.dir`. |
| Process env & spawn | `Core/Process/Process.jet:261-262, 292, 298, 468, 580, 709, 722` | `cwd`, `env_get`, `env_get_or`, `env_keys`, `env_nonempty`, `env_truthy`, `ProcessSpec.cwd`, `ProcessSpec.env`, `ProcessSpec.spawn` | Reads env via `core.sys.get`; spawns via `rt.spec_spawn` -> `ProcessExecutionPrepare.jet` -> `HostSpawn.jet` | Spawning a spec with `cwd == None` inherits kernel process CWD instead of `context.dir`; relative `spec.cwd("sub")` resolves against kernel CWD instead of `context.dir`; child env ignores `#Context(env:)`. |
| Database VFS | `Core/DB/Engine/OS.jet:835` | `os_full_pathname` | Calls `rt.vfs_cwd()` (`jet_db_vfs_cwd`) for relative DB paths | `jet_db_vfs_cwd` invokes raw `getcwd` (syscall 79) directly, bypassing `context.dir`. |
| Service host dir | `Core/Service/Service.jet:1206` | `service_current_dir` | Calls `jet_sys_current_dir_raw` | Fixed automatically when `jet_sys_current_dir_raw` checks `context.dir`. |
| Args / App / Net / Git | `Core/Args/Args.jet:1558`, `Core/App/App.jet:3344`, `Core/Net/TLS.jet:938`, `Core/Git/Git.jet:32` | Env lookups & git subprocesses | Route through `core.process.env_get`, `core.sys.get`, `jet_rt_logical_env_get_os`, and `ProcessSpec` | Automatically honor `#Context(env:)` and `#Context(dir:)` once the runtime leaves resolve against task context. |

### 1.2 Runtime (`Compiler/JetRuntime/Source/`)

| File & Lines | Symbols | Current Behavior | Required Cutover |
| :--- | :--- | :--- | :--- |
| `Context.jet:1-472` | `CONTEXT_VALUES`, `CONTEXT_ON_STOP_KEY = 1`, `jet_ctx_value_push`, `jet_ctx_value_pop`, `jet_ctx_value_find`, `context_inherit` | Task-local value chain shared across spawns via retain/unref. | Reserve `CONTEXT_DIR_KEY = 2` (resolved `String`) and `CONTEXT_ENV_KEY = 3` (flat `[String]` key/value overlay list). Add `context_dir_current() -> I64` and `context_env_lookup(bytes, len, ...)`. |
| `CompilerHost.jet:326-349` | `jet_sys_current_dir_raw` | Raw `getcwd` (syscall 79). | Check `context_dir_current()` first; return cloned `String` when set, else `getcwd(3)` fallback. |
| `RouteSys.jet:653-671` | `jet_sys_set_current_dir_raw` | Raw `chdir(2)` (syscall 80) mutating kernel process-wide CWD. | Eliminate process-wide `chdir(2)` mutation from Core APIs (see Section 3 cutover options). |
| `LogicalEnvironment.jet:1-275` | `logical_env_root`, `jet_rt_logical_env_get`, `jet_rt_logical_env_get_os`, `jet_rt_logical_env_set`, `jet_rt_logical_env_unset` | Reads/writes process-wide `logical_env_root` (or `World` cell). | Single-key reads (`get`, `get_os`) walk `CONTEXT_ENV_KEY` nodes innermost-first before `logical_env_root` / startup index. |
| `CompilerEnv.jet:15-50` | `jet_rt_logical_env_columns` | Dumps `logical_environment_current()` pairs. | Materialize base `logical_environment_current()` overlaid with `CONTEXT_ENV_KEY` chain from outermost to innermost (last/innermost key wins). |
| `ProcessExecutionPrepare.jet:63-114, 421-536` | `process_execution_environment`, `process_execution_inherited_path`, `process_execution_launch_native`, `process_execution_pack` | Builds child `envp` from `logical_environment_current()`, searches `PATH`, passes `spec.cwd` verbatim to `HostSpawn`. | Overlay `CONTEXT_ENV_KEY` onto child `envp` and inherited `PATH` (respecting `env_clear`, `spec.env`, `spec.env_remove`). When `spec.cwd == 0` and no held `as_cwd`, set child `cwd` to `context_dir_current()`; when `spec.cwd` is relative and `context_dir_current() != 0`, resolve `spec.cwd` against `context_dir_current()`. |
| `ProcessExecutionAuthority.jet:200, 213, 336, 430, 479` | Authority path & `PATH` checks | Calls `jet_rt_logical_env_get("PATH")` and `fd_c_path(path)`. | `PATH` auto-updates via `jet_rt_logical_env_get`; resolve relative executable/target paths against `context_dir_current()`. |
| `HostFiles.jet:44-672` | `fd_c_path`, `jet_fd_open_raw`, `stat`, `lstat`, `unlink`, `mkdir`, `rmdir`, `rename`, `symlink`, `hard_link`, `readlink`, `chmod`, `chown`, `truncate`, `xattr` | Calls `fd_c_path(path)` -> raw Linux syscalls (`open`, `stat`, etc.) relative to kernel CWD. | **Critical finding:** `fd_c_path` is also called on non-paths (DNS hostnames in `RouteDNS.jet`, xattr keys in `HostFiles.jet:579`, usernames in `RouteSys.jet:376`, loopback addr in `HostNetLookup.jet:542`). Must introduce a dedicated `fd_c_fs_path(path)` + `fd_c_fs_path_drop(buf)` (storing or tracking resolved length) ONLY at filesystem path sites, never inside `fd_c_path`! |
| `FilesPathOps.jet`, `FilesWalk.jet`, `RouteAFiles.jet`, `HostFileLock.jet`, `RouteALog.jet`, `HostPathFacts.jet` | Directory walk, `create_dir_all`, `remove_all`, `copy`, `lock`, log file open, metadata | Issue raw `open`/`stat`/`unlink`/`mkdir`/`getdents64` on relative paths. | Route all filesystem path inputs through `fd_c_fs_path` / `context_resolve_path`. |
| `HostDBVfs.jet:48, 283, 296, 309, 326, 337` | `jet_db_vfs_open`, `unlink`, `access`, `stat`, `readlink`, `jet_db_vfs_cwd` | Uses raw `getcwd` (syscall 79) and `fd_c_path`. | `jet_db_vfs_cwd` returns `context_dir_current()` when set; DB VFS path leaves resolve relative paths via `fd_c_fs_path`. |
| `HostStartup.jet:480-500` | `rt_env_get`, `rt_env_get_lossy` | Direct immutable startup `environ` reader used by runtime infrastructure (`JET_RECORD`, `JET_COV_OUT`, `TMPDIR`, etc.). | Intentionally stays on immutable startup env for runtime bootstrap/telemetry (not user Core API). |

### 1.3 Compiler & Editor Tooling (`Compiler/`, `Tools/Editors/`)

| File & Lines | Role Today | Required Change |
| :--- | :--- | :--- |
| `Compiler/JetFoundation/Source/Facts/Body.jet:314-320` | Defines `CONTEXT_ON_STOP_KEY`, `CONTEXT_PRINT_KEY`. | Add `CONTEXT_DIR_KEY: "context:dir"` and `CONTEXT_ENV_KEY: "context:env"`. |
| `Compiler/JetSema/Source/Sema/ContextKeys.jet:40, 56, 67-128` | `SEMA_CONTEXT_RESERVED_NAMES`, `SEMA_CONTEXT_CORE_FIELDS`, `sema_context_field_module`, `sema_context_field_key`. | Add `"dir"` and `"env"` to reserved names and core fields, owned by `"core.sys"` via role functions `ContextDir` (`String`) and `ContextEnv` (`[String:String]`). |
| `Compiler/JetSema/Source/Sema/Statements/ContextBlocks.jet:37-90` | Checks `#Context(...)` labels and values; emits E0761/E0762. | Accept `dir: String` (and `files.Path`, converting via `.to_string()`) and `env: [String:String]` OR inferred record literal `{CC: "clang", ...}` (desugared in Sema to `[String:String]` `MapLit` with string-typed values). Reject non-string record fields or wrong types with clear E0762. Update E0761 diagnostic text to mention `dir` and `env`. |
| `Compiler/JetSema/Source/Sema/Expressions/Places.jet:928-956` | Checks `context.<field>` reads. | `context.dir` (`String`) and `context.env` (`[String:String]`) resolve through `sema_context_field_key`; update E0302 diagnostic list to include `dir` and `env`. |
| `Compiler/JetDriver/Source/Driver/Loader.jet:4580-4761` | Loads owning Core module for every field in `SEMA_CONTEXT_CORE_FIELDS`. | Works automatically once `"dir"` and `"env"` are in `SEMA_CONTEXT_CORE_FIELDS` and mapped to `"core.sys"`! |
| `Compiler/JetIR/Source/BuildTransact.jet:328-534` | `ir_context_key_word`, `ir_context_push_value`, `ir_context_get`, `ir_context_lookup`. | Map `CONTEXT_DIR_KEY -> 2`, `CONTEXT_ENV_KEY -> 3` (shift general hash floor from `<= 1` to `<= 3`). For `CONTEXT_DIR_KEY` push: resolve relative dir against enclosing context dir before pushing. For `CONTEXT_ENV_KEY` push: pack `[String:String]` into flat `[String]` pair list `[k0, v0, ...]` so runtime reads it with zero hash-map layout coupling. For `context.dir` / `context.env` read: invoke `core.sys` role helpers returning effective `String` / `[String:String]`. |
| `Compiler/Prelude/Diagnostics.jet:10993-11029` | E0761 / E0762 descriptions. | Update text to list `dir` and `env` among `#Context` fields. |
| `Compiler/JetWeb/Source/Web/Host.jet:2369` | JS host syscall 80 (`nodeProcess.chdir`). | Unused once Core no longer invokes OS `chdir(2)` outside context; context dir stays portable across native and web tiers. |
| `Tools/Editors/{jet.tmGrammar, TreeSitter/grammar.js, Zed/languages/jet/highlights.scm}` | Syntax highlighting and TreeSitter rules for `#Context(...)`. | Verify `#Context(dir: ..., env: {CC: "clang"}) { ... }` parses and highlights cleanly in all three grammars and add a TreeSitter corpus test for `#Context(dir:, env:)`. |

---

## 2. Design Attack & Edge-Case Analysis

### Attack 1: Nested Relative `#Context(dir: ...)`
- **Scenario:** `#Context(dir: "/tmp/work") { #Context(dir: "sub") { files.read("a.txt") } }`.
- **Hazard:** If the inner `#Context(dir: "sub")` pushes the raw string `"sub"`, then `context_dir_current()` returns `"sub"` (losing `"/tmp/work"`!).
- **Fix:** At `#Context(dir: value)` entry, resolve `value` against the enclosing `context_dir_current()` (or OS `getcwd` when none is active) and normalize (`.`, `..`, duplicate `/`), pushing an **absolute** path onto `CONTEXT_DIR_KEY`. Then inner `#Context(dir: "sub")` pushes `"/tmp/work/sub"`, and `context.dir` always returns a canonical absolute directory string.

### Attack 2: Nested `#Context(env: ...)` Inheritance & Shadowing
- **Scenario:** `#Context(env: {CC: "clang", A: "1"}) { #Context(env: {A: "2", B: "3"}) { ... } }`.
- **Hazard:** If inner `env` replaced the entire environment, `PATH`, `HOME`, and outer `CC` would vanish and spawning `sh` or `clang` would fail!
- **Fix:** Each `#Context(env: ...)` node stores an **overlay** (delta pairs).
  - Single-key lookup (`sys.get`, `process.env_get`) walks `CONTEXT_VALUES` nodes matching `CONTEXT_ENV_KEY` **innermost-first**; first hit wins; miss falls through to base `logical_environment_current()`.
  - Whole-environment snapshot (`sys.environment()`, `sys.vars()`, `context.env`, and `process_execution_environment`) copies base `logical_environment_current()` then applies `CONTEXT_ENV_KEY` overlays **outermost-to-innermost** so inner shadows outer while inheriting `PATH` and outer keys.
  - Validation: reject empty key, `=` in key, or NUL byte in key/value at `#Context(env: ...)` entry (consistent with `EnvError.InvalidName` / `InvalidValue`).

### Attack 3: `fd_c_path` Aliasing Hazard (Non-Path Callers)
- **Finding:** Auditing all 48 callers of `fd_c_path` in `Compiler/JetRuntime/Source/` revealed that `RouteDNS.jet` (hostnames), `HostFiles.jet:579` (xattr names), `RouteAFiles.jet:654` (xattr names), `RouteSys.jet:376` (usernames), and `HostNetLookup.jet:542` (loopback address) all call `fd_c_path`.
- **Hazard:** Mutating `fd_c_path` to prepend `context.dir` would corrupt DNS lookups and xattr keys inside `#Context(dir:)`.
- **Fix:** Keep `fd_c_path` strictly as a NUL-terminator helper; introduce `fd_c_fs_path(path)` / `fd_c_fs_path_drop(buf)` (which returns a resolved NUL-terminated buffer with its exact allocation size stored or returned) and use it exclusively at filesystem path sites.

### Attack 4: Parallel Tasks & Spawned Tasks
- **Scenario:** Two tasks run concurrently via `task.all` or parallel `#Test` execution: Task A runs `#Context(dir: dir_a, env: {TAG: "A"})` while Task B runs `#Context(dir: dir_b, env: {TAG: "B"})`; each reads/writes relative `"note.txt"`, reads `sys.get("TAG")`, and spawns `sh -c 'pwd; printf %s "$TAG"'`.
- **Verification:**
  - `CONTEXT_VALUES` lives in each task's own `Context` record (`TCB@48`), never in a process global.
  - `context_inherit` retains the head of `CONTEXT_VALUES` on `task { ... }` spawn, so child tasks spawned inside `#Context(dir:, env:)` see their parent's `dir` and `env` even after the parent exits the block (no dangling pointer because nodes are refcounted).
  - Neither task ever calls OS `chdir(2)` or mutates `logical_env_root`; each spawned subprocess receives its own `chdir` in the child post-fork (`HostSpawn.jet:316`) and its own `envp` array (`ProcessExecutionPrepare.jet:482`).

### Attack 5: `#Context(env: {CC: "clang"})` vs `[String:String]` vs Empty `{}`
- **Parser fact:** `{CC: "clang"}` already parses as `Expr.StructLit{type_name: "", fields: [FieldInit{name: "CC", value: "clang"}], inferred: true}` (`JetParser/Source/Parser/Expressions.jet:725`).
- **Sema desugaring:** In `ContextBlocks.jet`, when `field.name == "env"` and `field.value` is an inferred `Expr.StructLit` with empty `type_name`, desugar each `FieldInit{name, value}` into a `MapLit` entry `(Expr.Str(name), value)` checked against `[String:String]`. This allows `{CC: "clang"}`, `{CC: "clang", CFLAGS: "-O2"}`, `["CC": "clang"]`, `[String:String]{}`, and map variables to all work uniformly with zero parser changes.

### Attack 6: `core.files` Pure Path Ops vs FS Ops
- **Hazard:** `files.join("a", "b")`, `files.basename("a/b")`, `files.dirname("a/b")`, `files.normalize("a/../b")` are pure lexical operations and must NEVER prepend `context.dir`.
- **Fix:** Resolving relative paths at the runtime OS boundary (plus `sys.current_dir()` / `files.cwd()` / `files.resolve()` / `files.absolute()`) guarantees pure lexical functions stay pure while every OS-touching call resolves against `context.dir`.

### Attack 7: `#Memo` & Comptime Safety
- **Fact:** `context.dir` and `context.env` are routed through `sema_context_field_key` -> `THostCall.ContextGet`, so:
  - Module-level comptime evaluation rejects reading them (`E0643`, `sema_comptime_body_unfoldable`).
  - `#Memo` functions reaching them include `CONTEXT_DIR_KEY` / `CONTEXT_ENV_KEY` in their memo cache key (`Effects/Checks.jet`).
  - C callbacks reject reading them (`E3218`).

### Attack 8: Process-Wide Mutation Cutover (`n3`)
- **Current process-wide mutators in Core:**
  1. `core.sys.set_current_dir(path)` (`Core/Sys/Sys.jet:1492`) -> OS `chdir(2)` (`RouteSys.jet:664`)
  2. `core.files.chdir(path)` (`Core/Files/Files.jet:2787`) -> forwards to `sys.set_current_dir`
  3. `core.files.cd(path)` (`Core/Files/Files.jet:3049`) -> forwards to `sys.set_current_dir`
  4. `core.sys.set(name, value)` (`Core/Sys/Sys.jet:66`) -> mutates process-wide `logical_env_root`
  5. `core.sys.unset(name)` (`Core/Sys/Sys.jet:78`) -> mutates process-wide `logical_env_root`
- **Copies deleted / cutover required:**
  - `files.chdir` and `files.cd` are duplicate wrappers over `sys.set_current_dir`; `files.getcwd` and `files.pwd` duplicate `files.cwd` / `sys.current_dir`; `files.getenv` and `sys.getenv` duplicate `sys.get`.
  - No Core API may call OS `chdir(2)` or mutate process-wide `logical_env_root` outside task context. Existing process-wide callers in `Tests/`, `Tools/`, `Examples/`, and `Compiler/` must either migrate to `#Context(dir:, env:)` (full removal of the 5 mutators) or be isolated strictly inside task-local context overlays with zero OS `chdir(2)` / zero cross-task leakage.

---

## 3. Exact Cutover File List

1. **Compiler (`seed: s1` required):**
   - `Compiler/JetFoundation/Source/Facts/Body.jet` (`CONTEXT_DIR_KEY`, `CONTEXT_ENV_KEY`)
   - `Compiler/JetSema/Source/Sema/ContextKeys.jet` (`SEMA_CONTEXT_RESERVED_NAMES`, `SEMA_CONTEXT_CORE_FIELDS`, `sema_context_field_module`, `sema_context_label_key`)
   - `Compiler/JetSema/Source/Sema/Statements/ContextBlocks.jet` (`dir:` `String`/`Path` handling, `env:` `{K: V}` record-to-map desugaring + `[String:String]`)
   - `Compiler/JetSema/Source/Sema/Expressions/Places.jet` (`context.dir`, `context.env` routed through `core.sys.__context_dir` / `core.sys.__context_env`)
   - `Compiler/JetIR/Source/BuildTransact.jet` (`ir_context_key_word` for `CONTEXT_DIR_KEY = 2` / `CONTEXT_ENV_KEY = 3`)
   - `Compiler/JetRuntime/Source/Context.jet` (`CONTEXT_DIR_KEY = 2`, `CONTEXT_ENV_KEY = 3`, `CONTEXT_IMPERATIVE_FLAG`, `context_dir_current`, `context_dir_resolve_owned`, `context_dir_set_imperative`, `context_env_lookup`, `context_env_set_imperative`, `context_env_nodes_outer_to_inner`, `jet_ctx_value_push`/`jet_ctx_value_pop` handling)
   - `Compiler/JetRuntime/Source/CompilerHost.jet` (`jet_sys_current_dir_raw` -> consult `context_dir_current()`)
   - `Compiler/JetRuntime/Source/LogicalEnvironment.jet` (`jet_rt_logical_env_get`, `jet_rt_logical_env_get_os` -> consult `context_env_lookup`; `jet_rt_logical_env_set`, `jet_rt_logical_env_unset` -> mutate task-local `CONTEXT_ENV_KEY` only, never `logical_env_root`)
   - `Compiler/JetRuntime/Source/CompilerEnv.jet` (`jet_rt_logical_env_columns` -> overlay `context_env_nodes_outer_to_inner`)
   - `Compiler/JetRuntime/Source/ProcessExecutionPrepare.jet` (inherit/resolve `context_dir_current()` for child `cwd`, overlay `CONTEXT_ENV_KEY` onto child `envp` and `PATH`)
   - `Compiler/JetRuntime/Source/ProcessExecutionAuthority.jet` (resolve relative target paths against `context_dir_current()`)
   - `Compiler/JetRuntime/Source/HostFiles.jet`, `CompilerFS.jet`, `FilesPathOps.jet`, `FilesWalk.jet`, `RouteAFiles.jet`, `HostHeldDirectory.jet`, `CompilerMapped.jet`, `RouteWatcher.jet`, `HostFileLock.jet`, `HostDBVfs.jet`, `RouteALog.jet`, `HostPathFacts.jet`, `RouteSys.jet` (dedicated `fd_c_fs_path` / `fd_c_fs_path_bytes` resolution against `context_dir_current()`, keeping symlink targets, xattr names, usernames, and `*at` dirfd-relative names verbatim; `jet_sys_set_current_dir_raw` removes OS `chdir(2)` syscall 80 in favor of validated `context_dir_set_imperative`; `jet_sys_host_env_raw` consults `context_env_lookup`; `jet_db_vfs_cwd` consults `context_dir_current()`)
2. **Core (`Core/`):**
   - `Core/Sys/Sys.jet` (`priv __context_dir`, `priv __context_env` helpers; all mutators delegate to task-context-only runtime leaves)
3. **Examples, Tests, Docs, and Editor Grammars:**
   - `Examples/Systems/Process/ContextDirEnv.jet` + `.out` (golden across `O0`/`O1`/`O2`)
   - `Tests/Cases/Context/context_dir_env_scoping_and_parallel_isolation.case` + fixture (proves `n2` & `n3`: relative files, DB, env reads, spawned child processes, nested shadow/restore, early return, spawned task inheritance, and two parallel `#Test` / `task.all` branches setting different directories without interference)
   - `Docs/Spec/spec.md` & `Docs/Spec/Reference/core-library.md`
   - `Tools/Editors/TreeSitter/test/corpus/context_dir_env.txt` (verified with `tree-sitter test` and `tree-sitter parse` on `Examples/Systems/Process/ContextDirEnv.jet`)
