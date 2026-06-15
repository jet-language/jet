# Forge salvage notes for Jetpack

**Source:** `examples/capstone/forge/`, reviewed 2026-06-15 before removal.
Forge was a Nix-backed dev-environment/task-runner capstone. It is superseded
by Jetpack, but several implementation ideas should be carried forward.

## Keep

- **Nix JSON parsing pattern.** Forge shells out to `nix build --no-link --json`,
  parses the JSON, and extracts `outputs.out` or `outputs.bin`. Jetpack should
  keep this shape behind a provider/translation interface.
- **Offline fixtures.** Forge used canned `nix build --json` fixtures so tests
  could run without network or Nix. Jetpack should keep fixture-backed tests for
  `nixpkgs:fastfetch`, `nixpkgs:ripgrep`, and at least one GitHub ref.
- **Friendly provider errors.** Preserve errors equivalent to:
  "Nix missing", "build failed", and "could not understand provider output".
- **PATH assembly.** Forge prepended realized `<store-path>/bin` directories to
  the existing PATH. Jetpack should generalize this to shell env composition.
- **Parallel realization.** Forge resolved tools concurrently using tasks and a
  channel. Jetpack should keep provider operations parallel where possible.
- **Task graph logic.** Forge's taskrunner topologically sorted task deps and
  reported unknown tasks/cycles clearly. Jetpack can reuse the concept later
  for `pack.jet`/`config.jet` tasks, but it is not required for JPK-0.
- **`-- cmd` passthrough.** Forge's `use <pkg> -- <cmd>` maps directly to
  `jetpack run <source>:<pkg> -- <cmd>`.
- **Generated shell wrappers.** Forge generated `forge-env.sh` / shell wrappers.
  Jetpack should generate temporary rc/env files rather than mutating the parent
  shell.
- **TTY-aware color.** Forge carried explicit `--no-color` support. Jetpack
  should support `--no-color` and `NO_COLOR`.

## Do Not Carry Forward

- **`nixpkgs#attr` user syntax.** Jetpack's public syntax is
  `<source>:<package/path-to-package>`, e.g. `nixpkgs:fastfetch`.
- **JSON as the primary manifest.** Forge used `forge.json` because the language
  lacked better support. Jetpack's source of truth is the Jet pack file
  (`pack.jet`; the earlier prototype used `config.jet`).
- **Task runner as Phase 1 core.** Useful later, but the immediate target is
  package/environment realization and shell entry.
- **Multiple capstone names.** Forge must not remain as a parallel package-manager
  story once these notes are saved.

## Migration Mapping

| Forge concept | Jetpack concept |
|---|---|
| `forge use jq -- jq --version` | `jetpack run nixpkgs:jq -- jq --version` |
| `forge shell` | `jetpack run` / project shell from pack file |
| `forge env` | generated Jetpack shell rc/env file |
| `demo/fixtures/*.json` | `examples/jetpack/fixtures/*.json` |
| `forge.env.jet` | Jet pack file (`pack.jet`; would be renamed from `forge.env.jet`) |
| `demo/forge.json` | generated internal plan, not user-owned config |
| `nixbridge` | Jetpack Nix provider/translator |
| `taskrunner` | future pack task graph support |

## Test Ideas To Port

- Missing provider binary diagnostic.
- Bad provider JSON diagnostic.
- Empty provider output diagnostic.
- Multiple packages resolved to PATH in deterministic sorted order.
- `-- cmd` exits with the child command status.
- Parent environment remains unchanged after shell exit.
- Offline fixtures produce the same plan as online provider output shape.
