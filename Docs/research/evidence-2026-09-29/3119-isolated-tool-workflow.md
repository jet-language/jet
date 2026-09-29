# #3119: isolated executable-tool workflow (SCRIPT-F29)

Closer10, 2026-09-29. Jetpack binary: `target/debug/jetpack` (2026-09-25
07:25, sha256 prefix `cc37ec87ef72093a`). It is the only built jetpack; the
test harness resolves it through `tests/common/mod.rs:1150-1168`, and no
rebuild was allowed. Verdict: **PARTIAL**.

## Question

Does the shipped `jetpack tool` / `jetpack use` surface give a reproducible
isolated-tool workflow? That means install, PATH exposure, project isolation,
update, offline behaviour, and pinned/`#latest`/`#auto` refs under
D-CHANNEL-AUTO1.

## Method

1. Named proof test, prebuilt binary run under the memory cap:
   `systemd-run --user --slice=jetwork.slice --scope -q -p MemoryMax=6G env TMPDIR=… Tools/agent/jet-env target-integ/debug/deps/jetpack_tool-0f59c33b64132fde <four named tests> --test-threads=1`.
   The binary was built 2026-09-29 02:06.
2. Scratch-root run: `~/.cache/jet-test-scratch/Closer10/tool/run.sh`.
   - `JETPACK_ROOT` and `HOME` point at scratch.
   - A native release fixture (`jetpackage-omp.json` plus artifact), built in
     the same shape as `write_native_omp_fixture` in `tests/jetpack_tool.rs`.
   - A project dir whose `env.jet` names `missing@releases#1.0.0`.
   - Log: `~/.cache/jet-test-scratch/Closer10/tool/run.log`.

## Evidence

### Proof test (criterion 5): 3 of 4 pass

```
test declarative_user_tools_realize_outside_repo_and_rollback_runs_previous_generation ... ok
test tool_install_publishes_real_projection_and_generation ... FAILED
test tool_profile_reports_drift_without_prompt_and_yes_moves_pin ... ok
test use_ephemeral_execs_inside_and_outside_project_without_path_projection ... ok
stderr: Error [E1317]: `greet@nixpkgs` uses the retired `@nixpkgs` source spelling
 Why: D-JPK-SNIXREUSE1 makes Jetpack the public package source; …  Fix: Write `greet@jetpack`.
```

The failing test still installs `greet@nixpkgs` (`tests/jetpack_tool.rs:409`,
and `:81`, `:467`, `:475`, `:479`, `:1287`, `:1434`). The retirement lives in
`crates/jetpack/src/Output.rs:1468-1473`. This is test drift after
D-JPK-SNIXREUSE1. The test's fixture is keyed as `nixpkgs-<pkg>.json`, and a
`greet@jetpack` ref with that fixture answers E1272 ("lacks a supported Nix
compatibility output"). So renaming the ref alone does not repair the test:
the fixture route has to move too.

### Scratch-root cell table (observed)

| Cell | Command / observation | Result |
|---|---|---|
| Install, pinned | `jetpack tool install 'omp@releases#1.0.0' --offline --fixtures fx` from inside the project | `Installed omp 1.0.0 -> $HOME/.jet/bin/omp (generation "tools", generation 1)`; exit 0 |
| Executable path | `$HOME/.jet/bin/omp`, a regular file `-r-xr-xr-x` with 2 links (hard-linked projection, not a symlink/shim) | supported |
| Generation state | `$HOME/.jet/tools/current` = `jet-profile-current-v1 / generation 1 / witness sha256-8708… / checksum sha256-1a97…`; `generations/1` | supported |
| Manifest | `$HOME/.jet/tools/manifest.json`: `"reference":"omp@releases#1.0.0","resolved":"omp@releases#1.0.0","tier":"pinned"`; source `releases` = `github:can1357/oh-my-pi#v1.0.0` | supported |
| Outside a project | `$HOME/.jet/bin/omp` → `omp 1.0.0 JETPACK_REF=unset cwd=…/outside` | supported |
| Inside a project | Same bytes and output, `cwd=…/proj`; the project tree is unchanged after install and invoke (`env.jet` only) | no project capture (criterion 1) |
| Update (pinned → new pin) | `tool install 'omp@releases#1.1.0'` → generation 2; `omp` prints `omp 1.1.0`; generations `1 2` retained | supported; the update boundary is an explicit reinstall with a new pin |
| Bare ref (no marker) | `tool install omp@releases` → resolves 1.1.0 from the fixture, generation 3 | supported (pinned on first resolve) |
| `#latest` (manual tier) | `tool install 'omp@releases#latest'` → `E1340 Couldn't understand the provider's output … mutable version selector latest is not an exact provider identity … This is likely a Jetpack bug` | **defect** |
| `#auto` (automatic tier) | `tool install 'omp@releases#auto'` → `E1276 … Release metadata for vauto is not cached` (the marker is read as version tag `vauto`) | **defect** |
| Offline, warm | Reinstalling the already-installed `omp@releases#1.1.0 --offline` with no fixtures → `E1276 Release metadata for v1.1.0 is not cached` | unsupported: offline needs cached release metadata, which a completed install does not retain |
| Offline, cold | Fresh root → `E1276 --offline forbids network access` | explicit, fail-closed |
| Ephemeral | `jetpack use 'omp@releases#1.0.0' -y --offline --fixtures fx -- omp` outside a project → `omp 1.0.0 JETPACK_REF=omp@releases#1.0.0` | supported; no PATH projection (per the passing test) |
| Uninstall | `tool uninstall omp` → `removed omp`, but `$HOME/.jet/bin/.projection-omp-3449388.partial` is left behind | **defect** (staging leftover) |

## Criteria

1. **Met.** The tool runs identically inside and outside a project, and the
   project tree is untouched. `use_ephemeral_…` also passes.
2. **Partially met.**
   - Explicit and observed: version selection by pin, PATH exposure
     (`~/.jet/bin/<name>` as a hard-linked copy), the update boundary
     (reinstall with a new pin makes a new generation), and cold offline.
   - Warm offline reinstall fails for lack of cached release metadata.
3. **No missing verb found.** `tool install|list|uninstall` and `use` exist,
   and `jetpack tool --help` lists them through E1340. `update --tools` was
   not found, and pin movement is a reinstall. There is no ballot, because
   the defects below are implementation bugs against ratified law, not
   missing surface.
4. **Failed.** Pinned behaves per D-CHANNEL-AUTO1. `#latest` errors with a
   self-described Jetpack bug, and `#auto` is mis-parsed as a version tag. The
   installed path and update boundary are recorded above.
5. **Failed.** 3 of 4 named tests pass; `tool_install_publishes_real_projection_and_generation`
   fails on the retired `@nixpkgs` spelling.

Caveat: the jetpack binary is from 2026-09-25. Rows marked defect should be
re-run on a rebuilt jetpack before a fixer starts.
