# Federated Jetpack providers under one authority graph

**Card:** Tower #234. **Epoch 4.** **Scope:** planning slice for `D-WD6`.

## Goal

npm, PyPI, Cargo, SwiftPM, Nix, GitHub, and binary sources are metadata
providers behind one Jetpack resolver, fetcher, lock, sandbox, audit, signing,
cache, and replacement-overlay authority graph. Provider differences are facts,
not separate package managers.

## Current Ratified Law

- `D-WD6`: listed ecosystems are federated providers under Jetpack authority.
- `D-JPK-EXTPROV1` is still open in the E4 README for external provider surface;
  this plan avoids implementing provider prefixes until that is decided.
- `D-JPK-ADAPTER1=A`: refs with no metadata can be adapted through recipes.
- `D-JPK-CACHE1=A`, `D-PKGSIGN1`, and `D-CASTORE1=A`: fetched and built objects
  carry cache/signature/hash identity.
- `D-JPK-OFFLINE1=A`: satisfied locks do not fetch.
- `D-JPK-NONIX1=A`: no-Nix machines realize everything except bridge-needing
  packages, which fail with an honest diagnostic.
- `D-WD1`: provider fetch authority is part of the universal grant graph.

## Vertical Slices

### T1. Provider Trait Contract

Define the internal provider contract:

- parse ref;
- metadata probe;
- resolve channel/range to exact identity;
- fetch source or binary bytes;
- verify hash/signature;
- expose license, scripts, native deps, and replacement candidates;
- report offline satisfiability.

Exit: existing `core`, `nix`, `path`, and `github` behavior uses the same
contract shape.

### T2. Metadata Fact Normalization

Normalize provider metadata into one fact model: package name, version, source
identity, integrity hash, dependencies, peer/dev/build deps, scripts, supported
platforms, license, bin names, and trust roots.

Exit: resolver and discovery index read normalized facts, not ecosystem-specific
structures.

### T3. npm And Cargo Providers

Implement read-only metadata and locked fetch for npm and Cargo fixtures. Build
execution remains through legacy wrappers, adapters, or FFI cards; this slice
does not pretend JS or Rust packages are native Jet packages.

Exit: `jet info`/resolver fixtures can lock exact npm/Cargo identities and fetch
offline from the lock.

### T4. PyPI And SwiftPM Providers

Add metadata/fetch for PyPI and SwiftPM fixtures with explicit dynamic metadata
TODOs where needed. Native extensions or build scripts require build policy
facts before execution.

Exit: fixture packages lock exact source/binary identity and explain unsupported
build hooks without executing them.

### T5. Binary Provider

Support hash-pinned binary source records through the same provider contract.
Binary objects must have platform identity, signature/hash verification, and
replacement-overlay eligibility facts.

Exit: binary fixture realizes only when hash and platform match; mismatch is a
package diagnostic, not a raw download error.

### T6. Authority And Offline Integration

Provider fetches are network authority facts; fulfilled lock records are offline
facts. The same grant/revocation engine governs all providers.

Exit: network is denied under `--offline` unless the exact provider object is
already locked and present.

## Acceptance Tests

- `provider_contract_covers_core_nix_path_github`.
- `npm_metadata_normalizes_deps_scripts_bins`.
- `cargo_metadata_normalizes_features_and_build_script_fact`.
- `pypi_dynamic_metadata_becomes_todo_fact`.
- `swiftpm_metadata_locks_exact_revision`.
- `binary_provider_requires_hash_and_platform`.
- `provider_fetch_denied_under_offline_without_lock`.
- `provider_fetch_allowed_offline_with_satisfied_lock`.
- `provider_facts_feed_search_info_and_lock_explain`.

## Dependencies

- Phase A dispatch and lock envelope.
- Signed package cache and package signing.
- Universal trust grants.
- Explainable lockfiles.
- Strict package graph/catalogs.
- Migration importers, which consume provider metadata.
- Native replacement overlays, which consume compatibility/replacement facts.

## Ballots Needed

- `D-JPK-EXTPROV1` — External provider prefixes, source-ref spelling, and CLI
  input forms for npm/PyPI/SwiftPM/binary providers. Already listed open in
  README; implementation of user-typed refs waits on it.
- `D-JPK-PROVIDERAUTH1` — Provider trust-root policy if user-visible config is
  added for registries, mirrors, key pinning, or allowed providers beyond
  existing signing/cache law.

