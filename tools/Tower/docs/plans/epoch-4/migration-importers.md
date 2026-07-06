# Migration importers from foreign ecosystems

**Card:** Tower #233. **Epoch 4.** **Scope:** planning slice for `D-WD5`.

## Goal

Importers turn foreign project metadata into editable canonical Jet source:
`pkg.jet`, role modules, deps, adapters, FFI stubs, lock rationale, and TODO
diagnostics. They help users migrate without hiding foreign assumptions in an
opaque bridge.

## Current Ratified Law

- `D-WD5`: migration importers generate editable canonical Jet source, role
  modules, deps, adapters, FFI stubs, and TODO diagnostics; native migration
  progress is tracked.
- `D-WD6`: npm, PyPI, Cargo, SwiftPM, Nix, GitHub, and binary sources are
  federated providers under Jetpack authority.
- `D-JPK-ADAPTER1=A` and `D-JPK-ADAPTNAME1=A`: no-metadata refs become
  `Pkg.adapt(name:, source:, recipe:)` with `Recipe.*`.
- `D-BUILDLEGACY1=A`: legacy builds are Tier-2 wrappers with declared inputs,
  outputs, and caps.
- `D-WD15`: native replacement overlays require compatibility proof before
  replacing a foreign surface without call-site rewrites.
- `D-WD4`: lock rationale records importer provenance and migration status.

## Vertical Slices

### T1. Import Plan IR

Create an internal import-plan model shared by all ecosystems:

- discovered packages;
- source/provider refs;
- direct deps and dev deps;
- scripts/build commands;
- services/env vars/secrets hints;
- generated Jet files;
- adapter recipes;
- FFI stub candidates;
- TODO diagnostics with source pointers;
- native replacement status.

Exit: each importer produces the same IR shape before file emission.

### T2. Nix Flake Importer

Convert `flake.nix` / `devenv.nix` facts already used by U16 into canonical
Jet role modules when possible. Unmappable fields become TODO diagnostics with
the original source path and a suggested Jetpack surface if one exists.

Exit: fixture flake imports packages, dev shell tools, services where known,
and leaves an unmapped-field TODO without executing Nix build code.

### T3. Cargo And npm Importers

Read `Cargo.toml`/`Cargo.lock` and `package.json`/lockfiles into package deps,
build wrappers, and FFI stub candidates. Existing lock hashes and exact versions
become lock rationale. Scripts that run commands become Tier-2 legacy build
actions, not ambient shell magic.

Exit: fixtures produce `pkg.jet`, direct deps, build action TODOs, and strict
graph diagnostics for undeclared transitive use.

### T4. PyPI And SwiftPM Importers

Read Python and Swift package metadata into federated provider refs, adapter
recipes, and bridge stubs. Native Jet replacements are tracked as status, not
claimed, until compatibility proof passes.

Exit: fixtures emit editable source plus explicit TODO diagnostics for dynamic
metadata or unsupported script hooks.

### T5. Editable Emission And Idempotence

Generated files are normal Jet source. Re-running an importer updates only the
owned generated sections or emits a conflict when the user edited a generated
line. Importers never keep hidden state as the canonical source.

Exit: import twice is byte-stable; import after user edit preserves the edit or
produces a conflict with the source span.

### T6. Progress Tracking

Track migration status in lock/explain facts: foreign dependency retained,
adapter wrapped, FFI stub generated, native replacement candidate found,
compatibility proved, or native replacement active.

Exit: explain facts list migration state for every imported dependency.

## Acceptance Tests

- `nix_import_emits_role_modules_and_todos`.
- `cargo_import_preserves_locked_versions`.
- `npm_import_turns_scripts_into_legacy_build_actions`.
- `python_import_marks_dynamic_metadata_todo`.
- `swiftpm_import_emits_provider_refs`.
- `import_idempotent_without_user_edits`.
- `import_conflict_preserves_user_edit`.
- `migration_status_feeds_lock_explain`.

## Dependencies

- Phase A module declaration role form.
- U16 Nix bridge facts.
- Federated providers for npm/PyPI/Cargo/SwiftPM/GitHub/binary metadata.
- Strict package graph/catalogs, because emitted deps must be strict.
- Explainable lockfiles, because imported provenance and TODOs need durable
  rationale.
- Native replacement overlays, because importers should surface replacement
  candidates and proof status.

## Ballots Needed

- `D-JPK-IMPORTCMD1` — Canonical importer command spelling and overwrite/update
  behavior. `D-WD5` ratifies importer output shape, not the user command.
- `D-JPK-IMPORTTODO1` — Canonical TODO diagnostic family for importer gaps if
  new E12xx codes or warning codes are introduced.

