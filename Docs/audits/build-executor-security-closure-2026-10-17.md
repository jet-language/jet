# Jet build executor security closure note

Report date: 2026-10-17
Scope: the Jet port of the `jet cc` / `jet c++` build model and sandboxed
executor (`Jetpack/CLI/Source/Build/*.jet`, the timeout-bounded runner in
`Jetpack/Foundation/Source/NativeSandbox.jet`, and the CcDriver caller in
`Jetpack/CLI/Source/CcDriver/DriverInvocation.jet`).
Reference: crates/jet-comptime `Comptime/Build/*.rs` and crates/jet-driver
`BuildArtifactStoreHandle` at e0d994d63.
Report status: boundary inventory and fixtures recorded; fresh external scan
pending.

## Closure state

This note is the boundary inventory a security-closure request starts from
(see [`security-closure.md`](../spec/contributing/security-closure.md)). It is
not closure. Closure needs the fresh Standard Codex Security scan of this exact
scope at the integration revision, its bundle stored with this inventory, and
the acceptance evidence recorded on the owning Tower card. None of that has
run yet.

The fixtures named below are written and parse clean with the r29 stage3
parser. They have not been run: the native Jetpack test build is the
integration owner's step. Every fixture that crosses the native boundary
requires the backend (Linux `bwrap` on PATH, macOS `sandbox-exec`) and fails
when it is missing; none returns early without running.

This revision also records the fixes for the independent review
`BuildExecSecReview2` (findings F1–F12, 2026-10-17); see "Review findings"
below. Rows 4, 5, 6, 9, 13 and 14 are corrected accordingly, and rows 22 and
23 are new.

## Enforced boundaries

Each row names the check, where it is enforced, and the fixture in
`Jetpack/CLI/Tests/BuildExecutor.jet` that exercises the allowed case and the
refused neighbour.

| # | Boundary | Enforced in | Allowed / refused fixture |
| --- | --- | --- | --- |
| 1 | Every selected action's declared ability must be in the root grants before any action, probe or sandbox setup runs (`MissingGrant`). | `build_execute_build_plan` (pre-check and per-batch re-check) | "ability grants: …": `Exec`+`Net` granted passes the gate; `Net` ungranted is refused with no marker and no work tree created. |
| 2 | Probes run only with the `Exec` grant. | `build_execute_build_plan` | "probe grants: …": no grants refused as `probe has-sh`; `Exec` granted runs the probe. |
| 3 | The action child shares the network only when the root grants `Net` and the action declares it. | `build_action_shares_network` | "network sharing: …": granted+declared shares; granted-only and declared-only stay isolated. |
| 4 | No unsandboxed fallback: an unavailable native backend (or `JETPACK_FAKE_SANDBOX`) fails as `SandboxUnavailable`; nothing is published. | `build_native_sandbox_error`, Foundation `foundation_run_native_sandboxed_with_timeout_and_mounts` | "no unsandboxed fallback: …": refused with no output; mapping of `Unsupported`/`IO`; then, with the backend required, the same plan runs a shell-builtin copy (the sandbox PATH has no `cp`) and publishes only its declared output. |
| 5 | Every action and every probe has a finite wall-clock budget (Rust `DEFAULT_ACTION_TIMEOUT`, 30 s; Rust probes have none); a child past it is killed and the run fails. On Linux a pipe-holding descendant is reaped on timeout and on exit by the pid-namespace teardown (see F6). The backend status probe has a fixed 5 s budget. | `NATIVE_SANDBOX_DEFAULT_ACTION_TIMEOUT_MS`, `foundation_run_native_sandboxed_with_timeout_and_mounts`, `build_run_probe_sandboxed`, `NATIVE_SANDBOX_STATUS_PROBE_TIMEOUT_MS` | "action timeout: …": a Bash child that exits leaves a builtin-clock descendant that is reaped (no marker, under 3 s); a busy-looping child past 500 ms fails with `sandbox process timed out after 500ms` in under 3 s and its descendant is reaped. "probe budget: …": a probe inside its budget succeeds; one past 500 ms fails as `IO{build probe sandbox, sandbox process timed out after 500ms}`. |
| 6 | Captured child stdout and stderr are bounded at 64 MiB. Files the child writes are read on the host with a 512 MiB per-file and 1 GiB per-action limit, checked against the opened file's size before any byte is read and again while reading; the same limits bound declared inputs and published outputs. | Foundation `NATIVE_SANDBOX_MAX_OUTPUT_BYTES` (existing); `BUILD_MAX_ACTION_FILE_BYTES`, `BUILD_MAX_ACTION_TOTAL_BYTES`, `build_read_budget`, `build_secure_read_file`, `build_publish_sandbox_outputs` | Pipes: existing Foundation/Recipe sandbox fixtures. Files: "secure read: …" (a file at its exact limit is read, one byte over is refused); "sandbox outputs: … oversized sparse output …": a sparse work-tree output one byte past the per-file limit is refused in under 3 s and nothing is published. |
| 7 | Declared inputs, outputs and sources are relative paths with no `..`, root or prefix. | `build_path_new`, `build_validate_paths`, `build_validate_action` | "declared build paths: …": relative accepted; absolute, `..` and blank refused, also at action declaration. |
| 8 | Paths resolve only under their base; `..` or a root component is refused. | `build_resolve_under` | "resolve under: …". |
| 9 | Inputs, cached outputs and sandbox outputs are read through held directory handles that never follow a link, and only regular files are read. The final name is checked through its held parent before any open, the open never waits on a FIFO or device peer (Core's non-blocking reader), and the opened file must be the inode the held parent named. | `build_secure_read_file` | "secure read: …": regular file read; final link, linked parent, directory and out-of-base path refused. "sandbox outputs: … FIFO output …": regular outputs publish; a FIFO output is refused in under 3 s with `cache entry is not a regular file` and no output is published. |
| 10 | Restored outputs are written to an exclusive `.jet-restore-<nonce>` (0600) under held, link-refusing parents, then renamed in place. | `build_atomic_restore_file` | "atomic restore: …": written under fresh parents with no temp left; a linked parent is refused and nothing lands outside. |
| 11 | Output parents below the project root are real directories; a final link at the output path is removed, never followed. | `build_prepare_output_destination` | "output destination: …". |
| 12 | The action work tree `.jet/build-sandbox/<pid>-<id>-<key>` and its parents are created without following any existing link. | `build_ensure_real_directory`, `build_execute_one_action` | "action work tree: …": a symlinked sandbox root is refused before anything is written through it; a real one is created. |
| 13 | Executables resolve through the toolchain: declared tools first; a declared-only toolchain never falls back; only the host toolchain or a target equal to the running host may use PATH, and only its absolute entries: an empty or relative PATH entry never resolves to the working directory. | `build_resolve_program_path`, `build_find_program_path` | "tool resolution: …"; "PATH resolution: …": with the tool in the working directory, `PATH=:.:relative/bin:` finds nothing, an absolute entry finds it. |
| 14 | A declared tool under a read-only mount maps to the mount source. Destination and tool path are compared as path components, so `//` or `.` after a destination stays under the source and a destination never matches a longer name; a `..` component is refused. | `build_canonical_tool_path`, `build_strip_path_prefix`, `build_path_parts` | "mounted tools: …": `/toolchain/bin/cc`, `/toolchain//bin/cc` and `/toolchain/./bin//cc` map to the source; `/toolchain/../etc/passwd` and `/toolchain//etc/passwd` (which a string join resolved to the host file) are refused; `/toolchainx/bin/cc` does not match; a non-normalized destination `/tc//nested/` matches `/tc/nested/bin/cc`. |
| 15 | Read-only mounts are absolute, `..`-free, NUL-free directories; mount sources are real directories (existing Foundation check at run time). | `build_validate_toolchain`, Foundation `foundation_validate_read_only_mounts` | "toolchain mounts: …": an absolute mount is accepted; relative and `..` destinations are refused. |
| 16 | Handles from another build context are refused. | `HermeticBuildContext.check_*_ref`, `HermeticBuildPlan` accessors | "context handles: …": same-context accepted; foreign toolchain, action and target refused. |
| 17 | The child environment is cleared; only declared variables, filtered by the allowlist, enter. An allowlist name without a declared value is refused. | `build_action_environment`, `build_validate_action`, Foundation `--clearenv`/`env_clear` | "action environment: …". |
| 18 | One declared output has one owning action. | `build_validate_action_output_owners` | "output ownership: …". |
| 19 | A cache record must name its own key, carry exactly one outcome, carry a canonical failure report exactly when it failed, and list exactly the declared outputs before restore. | `build_read_action_record`, `build_restore_action_outputs`, `build_report_from_json` | "cache records: …" and "failure reports: …". |
| 20 | A recorded failure replays its structured report instead of rerunning the action. | `build_execute_one_action` | "cached failures: …". |
| 21 | Action identity covers argv, allowlisted env, input digests, outputs, grants, the resolved executable's digest and probe facts. | `build_canonical_action_key`, `build_canonical_effective_action_key` | "action keys: …": identical declarations share a key; a changed grant or input digest splits it. |
| 22 | The work tree receives exactly the input bytes the key hashed; inputs are read once. | `build_snapshot_declared_inputs` (`ActionInputCapture`), `build_execute_one_action` | "cached failures: …": the capture carries the hashed bytes and their digest; "no unsandboxed fallback: …": the sandboxed copy publishes them. |
| 23 | The cache-observation path is appended only when it is absolute, inside `$HOME/.cache`, free of `.` and `..` components, and not a final link. | `build_record_cache_event` | "cache observation: …": `<cache>/sub/../escaped.jsonl` writes nothing; `<cache>/events.jsonl` receives the event. |

## Review findings

Findings of the independent review `BuildExecSecReview2` of commit
0ffc818a4 (2026-10-17).

| Finding | Status | Fix | Fixture |
| --- | --- | --- | --- |
| F1 (Medium): blocking, unbounded host reads of work-tree files | Fixed | `build_secure_read_file` checks the held parent's no-follow entry, opens non-blocking, requires the same regular inode, and reads by chunk up to its limit; `build_read_budget` applies the per-file and per-action limits; `build_publish_sandbox_outputs` reads every output before publishing any. | Rows 6 and 9. |
| F2 (Medium): timeout and allowed-neighbour fixtures could not pass, and skipped silently | Fixed | Fixtures use Bash builtins only (`while :; do :; done`, the `SECONDS` descendant marker, `read`/`printf` copy), as the Rust tests do; a missing backend fails the fixture. | Rows 4 and 5. |
| F3 (Medium): probes had no wall-clock budget | Fixed | `build_run_probe_sandboxed` takes a budget; probes pass the 30 s action budget. The status probe has a 5 s budget. | Row 5, "probe budget: …". |
| F4 (Low–Medium): key and execution read the inputs separately | Fixed for inputs | The snapshot keeps the hashed bytes and the work tree is written from them. The executable is still hashed from one read and executed by path; `jet cc` tools sit under verified read-only Hangar mounts (`CcToolchainDescriptor.jet`), so this remains a known difference for other callers. | Row 22. |
| F5 (Low): string mount-prefix matching let `//` escape the mount | Fixed | Component comparison and component joins in `build_canonical_tool_path`. The macOS-only argument translation `build_runtime_mount_path` keeps Rust's string join: it only rewrites argument text and Seatbelt still confines reads. | Row 14. |
| F6 (Low): no `/proc` descendant sweep | Documented; Linux checked | Linux: the bwrap pid namespace dies with the wrapper (`--die-with-parent`) and with the action, so the kernel kills every remaining process; `--new-session`/`setsid` cannot leave it (comment on `foundation_run_native_sandboxed_with_timeout_and_mounts`). One direct run on 2026-10-17 (NixOS, bwrap 0.12.0, the Foundation argv, the fixture's Bash script): the pipe-holding descendant left no marker after a normal exit (11 ms) or after a wrapper-group SIGKILL at 500 ms. macOS: Seatbelt has no pid namespace, so a child that leaves the process group survives the group kill, as in Rust (its walk is Linux-only). Open on macOS. | Row 5, "action timeout: …". |
| F7 (Low): no CPU, memory, pid or disk limits | Open | Not in this change. Row 6 now bounds host reads of files; the work tree still sits on the host disk. | — |
| F8 (Low): empty PATH entry resolved to the working directory | Fixed | `build_find_program_path` skips empty and relative entries and requires an absolute result. | Row 13. |
| F9 (Low): probe temp root via `create_dir_all` | Open | Not in this change. | — |
| F10 (Info): cache-observation path confinement | Fixed | `.`/`..` components refused; the append refuses a final link. | Row 23. |
| F11 (Info): work tree left behind on error | Open | Not in this change. | — |
| F12 (Info): output-cap message text | Open | Not in this change. | — |

## Known differences from the Rust reference

- Actions of one batch run sequentially instead of on scoped threads. Every
  batch member still runs and the first error in action order is reported.
- Jet is stricter than Rust in four places: probes have the action budget,
  host file reads are bounded (Rust's local executor reads unbounded),
  empty and relative PATH entries are skipped, and all of an action's
  outputs are read before any is published.
- On timeout or exit Jet signals only the wrapper's process group; Rust also
  kills the descendants its `/proc` walk found. On Linux the pid-namespace
  teardown covers them (F6).
- A build context's id is a 48-bit operating-system random value instead of a
  process-wide counter. Cross-context handles are still refused.
- I/O error detail text for operating-system errors is Jet's `IOError`
  rendering, not Rust's `(os error N)` text. Kinds that drive cache decisions
  (`NotFound`, `InvalidData`) are kept.
- Failure reports are `DiagnosticBuildError`, which has no origin frame or
  journey column. The executor never produces either. A cached report that
  carries one is refused as non-canonical.
- Remote builders, compiler-owned actions, WebAssembly build plugins,
  generated modules, legacy wrappers and fact contributions are not part of
  this port. The action key encodes them in their empty form.
- The 100,000-action budget is enforced as in Rust. It has no fixture here,
  because declaring 100,001 actions is too slow for a unit fixture.
