# Host reads under another effect

This page lists every host read that the runtime or Core performs under an
effect other than the read's own: a file, clock, entropy, environment or
process-state read that a public operation makes while its row names a
different effect, or no effect. A denial is truthful for a read only as far as
this page classifies it. `-[!FS.Read]>`, for example, does not stop the
resolver from reading `/etc/hosts` while `Net` is allowed.

A read that can change what the program does or prints is declared under its
own effect.

Each row is classified in one of four ways:

- **declared**: the public operation's row names the read's own effect, or
  the root that the effect table in spec.md "Effects" assigns to the
  operation.
- **ruled**: a ratified decision places the read under the operation's
  effect.
- **fixed**: a change removed the read, or moved it under its own effect.
- **open**: none of the above applies yet, and the owner decides it.

Until every open row is ruled, no page or diagnostic may say that a denial
blocks every host read of the denied kind.

A read that only the running program can answer is run-time only, whatever
its row: the running task's identity and cancel state, the process id (H7),
whether a standard stream is a terminal (H15), the UI host provider's state
(capabilities, clipboard, event queues, key claims and dialogs), and the
platform family, module loader, OS name, path syntax and f32 SIMD width of the
machine the program runs on.
Being run-time only adds no effect, so the callers' rows and package ceilings
stay as they were.
`Compiler/JetFoundation/Source/Registry/RuntimeOnlyReads.jet` lists their
runtime exports. The compiler never computes a value that reaches one while
building, a module value included, so the image never holds the compiler's
own answer, or for a cross build another machine's.

Run this census again whenever a runtime `#Import(c)`, an IR route effect or a
Core host leaf changes. The sources are:

- every `#Import(c)` in `Compiler/JetRuntime/Source/Public/*.jet`, grouped by
  runtime symbol;
- the IR route effects in `Compiler/JetIR/Source/RouteHandle.jet`;
- the Core host-leaf effects in `Tools/agent/CoreSourceRows.jet` `call_effect`;
- every `rt_static_text("/…")` host path in `Compiler/JetRuntime/Source`, and
  every path the runtime spells out in stored words (`os_store_i64`,
  `rt_text_store`) there;
- every environment name the runtime reads through `rt_env_get`,
  `rt_env_get_lossy`, `env_get` or `jet_rt_logical_env_get`.

## Inventory

| # | Read | Where | Reached through (row) | Own effect | Class |
| --- | --- | --- | --- | --- | --- |
| H1 | `/etc/hosts` and `/etc/resolv.conf` | Runtime HostNetLookup.jet `lk_hosts` and `lk_resolv_conf`; RouteDns.jet `dns_system_servers` | `core.net` address resolution `resolve_socket_addrs` (`-[Net, Time.Wait]>`, Core/Net/Net.jet) and the DNS lookups in Public/Net.jet | `FS.Read` | ruled: D-NETDNS2=A. Ordinary IP resolution delegates to the host resolver and keeps hosts files, search domains, VPNs and enterprise policy, so these reads are part of `Net`. |
| H2 | TLS: OS entropy, CA bundle files and directories, the wall clock, `SSL_CERT_FILE`/`SSL_CERT_DIR` | Public/Net.jet `tls_entropy`, `tls_read_bytes`, `tls_open_directory`, `tls_close_directory`, `tls_wall_sample`, `tls_env_value` (all `-[Net]>`) | `core.net.tls`, `core.http` HTTPS (`Net`) | `Rand`, `FS.Read`, `Time`, `Env` | ruled: D-NET-HOSTREADS1=A. These reads are declared under ordinary `Time`, `Rand.Draw`, `Env` and `FS.Read`. Until that cutover lands, the rows above still name `Net` alone. |
| H3 | Wall clock, OS entropy and working directory | Public/Sys.jet `service_entropy` (`-[Rand]>`) and `service_current_dir` (`-[Env]>`); the `core.service` host leaves `__host_wall_sample` (`Time`), `__host_entropy` (`Rand`) and `__host_current_dir` (`Env`), reached from Core/Service/Service.jet `service_now`, `service_issue_authority` and `service_current_dir` | `core.service` trees, authority tokens and store paths | `Time`, `Rand`, `Env` | declared: each leaf carries its own effect, so every `core.service` entry point that reaches one names it and `--deny` refuses it. The process id that `service_pid` reads for a temporary file name is ruled with H7. |
| H4 | Wall clock for the JSON coercion audit line | Core/Encoding/Json.jet `coerce_line`, which once read the clock through a runtime leaf inside an audited `#Unsafe` | `core.encoding.json` decode with coercion | `Time` | fixed: the audit line carries no timestamp, so the read is gone. |
| H5 | `COLUMNS`, `LINES`, `TERM`, `COLORTERM`, `JET_TUI_ASCII`, `FORCE_COLOR`, `NO_COLOR` | Public/Tui.jet `cap_env_bytes` (`-[Env]>`), the `core.tui` host leaf `__cap_env_bytes` (`Env`), reached from Core/Tui/Tui.jet `capability_inputs` | `core.tui` capability detection and the terminal UI backend | `Env` | declared: the variables decide what the terminal UI draws, so capability detection names `Env`. |
| H6 | Clock sample and `TMPDIR` for a temporary name | Public/Files.jet `temp_clock_raw` and `temp_directory_raw` (`-[FS]>`); runtime HostPathFacts.jet `jet_fs_temp_directory_raw` | `core.files` `mkdtemp`, `files_temp_path` and the `temp_*` constructors (`-[FS]>`) | `Time`, `Env` | ruled: D-HOST-NONCE1=A. The reads only make a temporary name, so they are part of `FS.Write`; the name is unspecified. `mktemp` declares `-[FS, Env, Time]>` for the same reads (declared). |
| H7 | Process id for an atomic write's or a service store's temporary name | Public/Files.jet `atomic_pid` (no row); Core/Files/Files.jet and Core/Crypto/Crypto.jet atomic writes; the `core.service` host leaf `__host_pid` (Core/Service/Service.jet `service_pid`) | `core.files` and `core.crypto` atomic writes (`FS.Write`), `core.service` store publishing | `Env` (host fact) | ruled: D-HOST-NONCE1=A. The process id only makes a collision-avoiding name, so it is part of `FS.Write`; the name is unspecified. |
| H8 | `HOME`, then the working directory | Runtime RoutePath.jet `jet_path_home`; IR RouteHandle.jet `.PathHome` route (`Effect.Env`) and StreamHandles.jet `path.home` (`Env`) | `path.home()` (`Env`) | `Env` | declared: `path.home()` names `Env` and touches no file. |
| H9 | `PATH` from the logical environment, to resolve a command | Runtime ProcessExecutionAuthority.jet `PATH` lookups | Process start (`Exec`) | `Env` | ruled: D-HOST-NONCE1=A. The lookup finds the program the `Exec` call starts, so it is part of `Exec`. |
| H10 | The process's own kernel state: `/proc/self/task/<tid>/*`, `/proc/self/cgroup`, `/sys/fs/cgroup`, `/proc/self/fd/` | Runtime Scheduler.jet `scheduler_task_file`, ProcessScope.jet, HostSockets.jet, TestReads.jet | The scheduler monitor, process scopes, Unix-socket paths, test reads (no row) | `FS.Read` | ruled: D-HOST-NONCE1=A. Each read inspects the running process, so it is part of whatever call makes it. |
| H11 | Runtime controls: `JET_LOG`, `JET_SCHEDULER_THREADS`, `JET_TEST_*`, `JET_PROVE_*`, `JET_RECEIPT_*`, `JET_DEVELOPMENT_RECEIPT_*`, `JET_COV_OUT`, `JET_MEMORY_LEDGER`, `JET_ENC_HOSTILE_*`, `JET_DEVTOOLS_RELAY_*`, `JET_READ_SCOPE`, `JET_STORE_DIR`, `JET_MACHINE_OUTPUT`, `JET_BUILD_OUTPUT`, `JET_PROP_SEED`, `JET_FUZZ_ITERATIONS`, `JETPACK_FAKE_SANDBOX`; the scheduler pool's CPU-count probe | Runtime RouteALog.jet, SchedulerMessages.jet, Coverage.jet, ReceiptSections.jet, CompilerFS.jet, HostParallelism.jet and the other readers of these names | Program start and the runtime services, not a public operation (no row) | `Env`, `FS.Read` | ruled: D-ENV-SURFACE1=A (owner, 2026-11-11). Every hidden control becomes a flag or setting, and Jet-owned transport goes through one handoff (#6606 c4, c5, c8). |
| H12 | `/proc/self/cgroup`, `/sys/fs/cgroup/…` and `/sys/devices/system/cpu/online` | Runtime HostParallelism.jet | `core.sys.cpu_count` (`-[Env]>`) | `FS.Read` | declared: a `core.sys` host fact under `Env` (spec.md effect table). |
| H13 | `/etc/group` and the nscd socket | Runtime RouteSys.jet `sys_grouplist` and `sys_nscd_initgroups` | `core.sys` `initgroups` (`-[Env]>`) | `FS.Read`, `Net` (local socket) | declared: a `core.sys` environment operation under `Env`. |
| H14 | Auxiliary vector, then `/proc/self/exe` | Runtime CompilerHost.jet `jet_sys_executable_raw` | `core.sys` `executable` (`-[Env]>`) | `FS.Read` | declared: a `core.sys` host fact under `Env`. |
| H15 | Whether stdout and stderr are terminals | Public/Term.jet `stdout_terminal` and `stderr_terminal` (no row) | Terminal output paths | `IO` | declared: `IO` is in the floor, and a terminal test is a terminal operation. |
| H16 | The monotonic clock for deadlines and intervals | Public/Time.jet `clock_mono_ns` (`-[Time.Wait]>`) | Every waiting operation | `Time` | ruled: D-AUTH-AMBIENT2=A. Waiting is the deny-only `Time.Wait` leaf. |
| H17 | Task CPU clock | Public/Tasks.jet `task_cpu_ns`; Core/Tasks/Tasks.jet `cpu_ns` (`-[Time]>`) | `core.tasks.cpu_ns` | `Time` | declared |
| H18 | Clipboard, dialogs, events and key claims | Public/Ui.jet primitives (`-[IO]>`) | `core.ui.host`, which carries `UI.<capability>` (E2937) | `UI.<capability>` | declared |
| H19 | Whatever foreign code reads | `#Import(c)` bindings without a descriptor, unannotated foreign calls | Foreign calls (`FFI`, the maximal set) | any | declared: spec.md, "calls native code, which can do anything". |
| H20 | Dynamic loader | Runtime RouteMod.jet: the static musl runtime has no dynamic loader, so mapping a payload is Unsupported | `core.mod` `load` (`FS`, `Exec`) | none at run time | declared |
| H21 | `/proc/self/exe`, after the auxiliary vector, for symbol names in a fault or checked-allocator report | Runtime Alloc.jet `alloc_check_image`, reached from Fault.jet and the checked allocator's report | A fault or allocator defect report (no row) | `FS.Read` | ruled: D-HOST-NONCE1=A. The read inspects the running process's own image. |
| H22 | `/proc/<pid>/status` and `/proc/<pid>/fd` of a started child | Runtime ProcessExecutionLimits.jet `process_execution_memory_used` and `process_execution_files_used` | Process start with memory or open-file limits (`Exec`) | `FS.Read` | ruled: D-HOST-NONCE2=A. The read inspects the child the `Exec` call started, to enforce that call's memory or open-file limit, so it is part of `Exec`, and the typed memory and open-file limit errors stay. |
| H23 | `/proc/<pid>/stat` start time for a vault lock's owner | Runtime VaultLinux.jet, which once recorded the writer's pid and start time in the lock file and read the recorded owner's entry to decide whether a held lock was stale | Vault operations (`Secret`) | `FS.Read` | fixed: D-HOST-NONCE2=A. The vault lock is a kernel flock that the kernel releases when its writer exits, so no vault call reads a process entry. |
