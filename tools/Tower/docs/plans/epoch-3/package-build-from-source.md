# Epoch 3 — package build-from-source + M9 wave-2 libraries

**Status:** tracked Epoch-3 milestone (owner, 2026-06-18). Moved out of the
Epoch-2 GA bar. **Not** an E2 exit criterion.

## Why this is its own milestone

`jet build` / `jet run` already compile a program jet → Rust → `rustc` → binary
(I2/I3) — the *core* compiler is fine. What's missing is on the **jetpack** side:
`provider.rs::realize()` stages a prebuilt `bin/` (executable) or raw source
(library), but **no step compiles a dependency from its Jet source**. Until that
exists, a `library` dependency is consumed as staged source searched by the
module resolver (U17), not as a built artifact.

This blocks **M9 wave-2** (`jet.regex`, `jet.archive`, `jet.db`/sqlite): the
owner requires those to be *real packages*, not compiler-known modules like the
wave-1 rings.

## Scope

1. **Compile step in `realize()`** — when a `library` package is realized, build
   its Jet source into a cached artifact (jet → Rust → `rustc`), staged in the
   hangar store, hash-keyed for invalidation. Stays offline/deterministic like
   the existing pre-fetched flow (`jet build`/`run` never realize on demand).
2. **M9 wave-2 as packages** — `jet.regex`, `jet.archive`, `jet.db`/sqlite ship
   as ordinary packages consumed via `use` (U17), each with its own `pkg.jet`.
   `jet.db`/sqlite also depends on the `jet bind` C-FFI backend (E2-M14).
3. **Cache invalidation** — couples to the C-FFI Phase-3 header/cflags-hash
   regen, which is likewise deferred until the build step lands.

## Exit criteria

- A `library` dependency with Jet source builds to a cached artifact during
  `jetpack build`; a downstream `use <pkg>` links it.
- `jet.regex` / `jet.archive` / `jet.db` exist as real packages with tests.
- Rebuild is incremental: an unchanged dependency hits the cache.

## Prerequisite

`jet bind` real backend (E2-M14) for `jet.db`/sqlite. The compile-step design
itself has no language prerequisite — it builds on the existing
`Source/Jetpack/Provider.rs` + hangar store.
