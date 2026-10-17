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
integration owner's step.

## Enforced boundaries

Each row names the check, where it is enforced, and the fixture in
`Jetpack/CLI/Tests/BuildExecutor.jet` that exercises the allowed case and the
refused neighbour.

| # | Boundary | Enforced in | Allowed / refused fixture |
| --- | --- | --- | --- |
| 1 | Every selected action's declared ability must be in the root grants before any action, probe or sandbox setup runs (`MissingGrant`). | `build_execute_build_plan` (pre-check and per-batch re-check) | "ability grants: …": `Exec`+`Net` granted passes the gate; `Net` ungranted is refused with no marker and no work tree created. |
| 2 | Probes run only with the `Exec` grant. | `build_execute_build_plan` | "probe grants: …": no grants refused as `probe has-sh`; `Exec` granted runs the probe. |
| 3 | The action child shares the network only when the root grants `Net` and the action declares it. | `build_action_shares_network` | "network sharing: …": granted+declared shares; granted-only and declared-only stay isolated. |
| 4 | No unsandboxed fallback: an unavailable native backend (or `JETPACK_FAKE_SANDBOX`) fails as `SandboxUnavailable`; nothing is published. | `build_native_sandbox_error`, Foundation `foundation_run_native_sandboxed_with_timeout_and_mounts` | "no unsandboxed fallback: …": refused with no output; mapping of `Unsupported`/`IO`; with the backend available the same plan runs and publishes only its declared output. |
| 5 | Every action has a finite wall-clock budget (Rust `DEFAULT_ACTION_TIMEOUT`, 30 s); a child past it is killed and the run fails. | `NATIVE_SANDBOX_DEFAULT_ACTION_TIMEOUT_MS`, `foundation_run_native_sandboxed_with_timeout_and_mounts` | "action timeout: …": a child inside its budget succeeds; one past it fails with `sandbox process timed out after 500ms`. |
| 6 | Captured child output is bounded at 64 MiB. | Foundation `NATIVE_SANDBOX_MAX_OUTPUT_BYTES` (existing) | Existing Foundation/Recipe sandbox fixtures; unchanged by this port. |
| 7 | Declared inputs, outputs and sources are relative paths with no `..`, root or prefix. | `build_path_new`, `build_validate_paths`, `build_validate_action` | "declared build paths: …": relative accepted; absolute, `..` and blank refused, also at action declaration. |
| 8 | Paths resolve only under their base; `..` or a root component is refused. | `build_resolve_under` | "resolve under: …". |
| 9 | Inputs, cached outputs and sandbox outputs are read through held directory handles that never follow a link, and only regular files are read. | `build_secure_read_file` | "secure read: …": regular file read; final link, linked parent, directory and out-of-base path refused. |
| 10 | Restored outputs are written to an exclusive `.jet-restore-<nonce>` (0600) under held, link-refusing parents, then renamed in place. | `build_atomic_restore_file` | "atomic restore: …": written under fresh parents with no temp left; a linked parent is refused and nothing lands outside. |
| 11 | Output parents below the project root are real directories; a final link at the output path is removed, never followed. | `build_prepare_output_destination` | "output destination: …". |
| 12 | The action work tree `.jet/build-sandbox/<pid>-<id>-<key>` and its parents are created without following any existing link. | `build_ensure_real_directory`, `build_execute_one_action` | "action work tree: …": a symlinked sandbox root is refused before anything is written through it; a real one is created. |
| 13 | Executables resolve through the toolchain: declared tools first; a declared-only toolchain never falls back; only the host toolchain or a target equal to the running host may use PATH. | `build_resolve_program_path` | "tool resolution: …". |
| 14 | A declared tool under a read-only mount maps to the mount source; a `..` component is refused. | `build_canonical_tool_path` | "mounted tools: …". |
| 15 | Read-only mounts are absolute, `..`-free, NUL-free directories; mount sources are real directories (existing Foundation check at run time). | `build_validate_toolchain`, Foundation `foundation_validate_read_only_mounts` | "toolchain mounts: …": an absolute mount is accepted; relative and `..` destinations are refused. |
| 16 | Handles from another build context are refused. | `HermeticBuildContext.check_*_ref`, `HermeticBuildPlan` accessors | "context handles: …": same-context accepted; foreign toolchain, action and target refused. |
| 17 | The child environment is cleared; only declared variables, filtered by the allowlist, enter. An allowlist name without a declared value is refused. | `build_action_environment`, `build_validate_action`, Foundation `--clearenv`/`env_clear` | "action environment: …". |
| 18 | One declared output has one owning action. | `build_validate_action_output_owners` | "output ownership: …". |
| 19 | A cache record must name its own key, carry exactly one outcome, carry a canonical failure report exactly when it failed, and list exactly the declared outputs before restore. | `build_read_action_record`, `build_restore_action_outputs`, `build_report_from_json` | "cache records: …" and "failure reports: …". |
| 20 | A recorded failure replays its structured report instead of rerunning the action. | `build_execute_one_action` | "cached failures: …". |
| 21 | Action identity covers argv, allowlisted env, input digests, outputs, grants, the resolved executable's digest and probe facts. | `build_canonical_action_key`, `build_canonical_effective_action_key` | "action keys: …": identical declarations share a key; a changed grant or input digest splits it. |

## Known differences from the Rust reference

- Actions of one batch run sequentially instead of on scoped threads. Every
  batch member still runs and the first error in action order is reported.
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
