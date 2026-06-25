# Package build-from-source + M9 wave-2 (c50)

**Status: ready — active. Every gating dep decision ratified
(D-DEP-ARCHIVE1, D-DEP-DB1, D-BFS1 on 2026-06-25; D-REGEX1 prior; D-BUILD1/S50
prior). No open owner decision. Implement on the owner's "go".**

Two halves, both now decided: (1) the **build-from-source mechanics** — give
`realize()` a step that compiles a dependency from source and feeds the cargo
bridge vendored, hash-pinned crate source (D-BFS1); (2) the **wave-2 packages**
— `jet.regex` is done, `jet.archive` and `jet.db` land on the approved crates.

## Goal

A dependency (pure-Jet library, or a std module backed by an `extern rust`
block) builds offline and byte-reproducibly into a cached artifact, with any
wrapped crate's source vendored in-tree and hash-pinned. On top of that, ship
`jet.archive` (zip/tar/tar.gz) and `jet.db` (sqlite) the way `jet.regex` already
ships.

## Current state (verified, file:line)

- **`realize()` has no compile step.** `Source/Jetpack/Provider.rs:149`
  `CoreProvider::realize` discovers the package's `module`, content-addresses the
  source tree (`tree_fingerprint`, line 190), copies it into the store
  (`copy_tree`, line 195), and stages a `bin/` for an executable or bare source
  for a library (lines 218-223). There is **no jet→Rust→rustc compile** and **no
  crate compile** — a library is consumed as staged source the module resolver
  walks. Fine for pure-Jet libraries; a wrapping package whose body is an
  `extern rust` block needs its crate built and linked. That is the gap.
- **The cargo/FFI bridge (D-BUILD1 / S50) exists.** `Source/FFI.rs`
  `build_bridge` (line 100) materializes a hidden cargo crate under
  `~/.cache/jet/ffi/<key>`, writes `Cargo.toml` (`emit_cargo_toml`, line 323),
  runs `cargo build` (line 135), links the rlib. `extern rust "crate@version"`
  is parsed/sema'd (`Source/Sema/FFI.rs` — exact version pin required for
  non-std, line 23).
- **The bridge fetches crates online from crates.io** (cargo build at FFI.rs:135;
  "Updating crates.io" stripped from snapshot output at FFI.rs:466). It is **not**
  offline/vendored today — D-BFS1 changes this.
- **`jet.regex` is done (D-REGEX1)** and ships as a **built-in std module, not a
  fetched package**: `use jet.regex as re` (example
  `examples/features/74_regex.jet`), runtime in `Source/Prelude/Regex.rs`,
  crate pin `REGEX_CRATE_SPEC = ("regex", "1")` at `Source/FFI.rs:94`, wired
  into the bridge by a `needs_regex` flag (FFI.rs:81-103). So the shipped
  pattern is **prelude-runtime + FFI-bridge**, *not* a `pkg.jet` wrapping package
  fetched from a registry. `jet.archive`/`jet.db` follow this precedent.
- **`jet.archive` / `jet.db` do not exist** — no prelude runtime, no example,
  no crate pins.
- **`.jet/lock`** (`Source/Lock.rs`) records `name/version/source/locked/
  fingerprint/dependencies` and has the fingerprint machinery
  (`compute_fingerprint`, line 373) but **no vendored-crate hash-pin field** —
  D-BFS1 needs one.

## Decision (ratified)

- **D-BUILD1 / S50** (prior) — the cargo bridge that compiles `extern rust`
  blocks; offline-capable, `jet doctor` surfaces it. The engine c50's compile
  step rides.
- **D-REGEX1** (2026-06-21) — `jet.regex` on `regex` (done). The native-ize
  obligation (in-house RE2-style engine before Epoch 3 ends) stands.
- **D-DEP-ARCHIVE1 (A)** (2026-06-25) — `jet.archive` wraps `zip@2.1.3` +
  `tar@0.4.40` + `flate2@1.0` (all pure-Rust, no C toolchain) covering
  zip/tar/tar.gz in one approval. Carries the native-ize obligation.
- **D-DEP-DB1 (A)** (2026-06-25) — `jet.db` wraps `rusqlite@0.31` with the
  `bundled` feature (SQLite C amalgamation compiled in → no system
  libsqlite3). Native-ize end state may be "keep bundled public-domain SQLite C"
  (flagged for a later frozen card).
- **D-BFS1 (A)** (2026-06-25) — the wrapped-crate source for an offline build is
  **vendored inside the wrapping package** (committed `vendor/`), **hash-pinned
  in `.jet/lock`**. Offline + byte-reproducible from the first build, fully
  auditable in the dep tree. This is the supply-chain default for every D-DEP1
  package.

## Implementation (staged, end-to-end per the skill standard)

### Stage 1 — build-from-source infra (`realize()` + vendored cargo)

1. **Compile step in `realize()`** — `Source/Jetpack/Provider.rs`
   `CoreProvider::realize`: when a `library` package is realized, build its Jet
   source (jet → Rust → `rustc`, the existing program path) into a hangar-store
   artifact keyed on (source tree hash + crate pins + toolchain). Stays
   deterministic and offline — jetpack pre-stages the artifact; `jet build`/`run`
   never realize on demand. Reuse the existing content-addressed store entry
   (`out_dir`, line 193) as the cache key root.
2. **Vendored, offline crate build (D-BFS1)** — point the FFI bridge
   (`Source/FFI.rs build_bridge`) at a vendored crate source dir instead of
   crates.io: emit `[source.crates-io] replace-with = "vendored"` /
   `[source.vendored] directory = "<pkg>/vendor"` into the generated
   `Cargo.toml` (`emit_cargo_toml`, line 323) and run `cargo build --offline`.
   The wrapping package commits its crate source under `vendor/`.
3. **Hash-pin in `.jet/lock` (D-BFS1)** — add a vendored-crate hash field to
   `LockedPackage` in `Source/Lock.rs` (fold the vendored tree hash into the
   package fingerprint via `compute_fingerprint`, line 373, or a dedicated
   field). A changed vendored crate source shifts the pin → forces a rebuild;
   an unchanged one hits the cache.
4. **Cache invalidation** — unchanged (source + crate pins + vendored hash +
   toolchain) hits the store; any change forces a rebuild.

Diagnostics: any new build-from-source failure (e.g. vendored source missing,
offline build can't resolve a crate) gets a code + `tests/ui` snapshot (I4).
Exit criteria: a `library` dependency with Jet source builds to a cached
artifact during `jetpack build`; a downstream `use <pkg>` links it; an
`extern rust` wrapping package builds **offline** from its vendored crate;
rebuild is incremental.

### Stage 2 — wave-2 packages on top (regex done; archive, db)

Follow the **`jet.regex` precedent**: a built-in std module (`use jet.archive`,
`use jet.db`), a runtime file emitted into the bridge crate, an approved crate
pin in `Source/FFI.rs`, a `needs_*` flag wiring it in. Each ships with its crate
source vendored + hash-pinned (Stage 1 / D-BFS1).

1. **`jet.archive`** — runtime `Source/Prelude/Archive.rs` (the only code
   touching `zip`/`tar`/`flate2`), crate pins added next to `REGEX_CRATE_SPEC`
   (FFI.rs:94), a `needs_archive` flag in `prepare`/`build_bridge`. Surface:
   zip/tar/tar.gz read+write over plain value types (no refs/callbacks across
   the FFI boundary — `Source/Sema/FFI.rs:67`).
2. **`jet.db`** — runtime `Source/Prelude/Db.rs` over `rusqlite@0.31`
   (`bundled` feature in the emitted `Cargo.toml`), `needs_db` flag. Surface: a
   small typed query/exec API returning plain values.
3. **Examples + golden (I5)** — `examples/features/<n>_archive.jet` and
   `<n>_db.jet` with expected output, golden-tested. Mind the "unsafe substring"
   golden grep — keep runtime/comment text clear of the bare word.

### Stage 3 — tests

Unit/integration: vendored offline build reproducibility (same inputs → same
artifact hash), cache hit/miss on a changed crate pin, archive round-trip
(write→read), db query round-trip. `nix develop -c cargo test` fully green.

### Stage 4 — docs

`docs/spec/roadmap.md` (mark `jet.archive`/`jet.db` available, E2-M9),
`docs/spec/spec.md` (the two modules' surface + build-from-source/vendoring
model), `docs/spec/syntax-decisions.md` status → **Implemented** for
D-DEP-ARCHIVE1 / D-DEP-DB1 / D-BFS1.

## Sequencing / gates

- **No open owner decision** — all dep approvals ratified. Ballot answer is the
  "go" for the unblocked work; only the owner's explicit start gates it.
- **Stage order is hard:** Stage 1 (build-from-source + vendored/hash-pinned
  cargo) is a prerequisite for Stages 2's offline, reproducible package builds —
  archive/db must build from vendored source, not crates.io.
- **Shared with c96:** D-BFS1's hash-pin and c96's registry pin both live in
  `.jet/lock` (`Source/Lock.rs`). Coordinate the schema edits (this card adds a
  vendored-crate hash; c96 adds `LockSource::Registry`) so the two don't collide.
  Neither hard-blocks the other in code (c50 = `Provider.rs`/`FFI.rs`/prelude
  runtimes; c96 = `CmdSupply.rs`/`Fetch.rs`/resolver).

## Native-ize tracking (I6 end state)

Each approved crate keeps the D-REGEX1 native-ize obligation (D-JITDEP1
precedent — approving the crate approves the *bootstrap*, not a permanent dep):
`regex` → in-house RE2-style engine; `archive` → native deflate/zip + tar reader;
`sqlite` → either keep bundled public-domain SQLite C (arguably already a native
form) or a native embedded store. Post-c50; logged as frozen cards so the
obligation isn't lost.

## Files (when this burns down)

| File | Change |
|---|---|
| `Source/Jetpack/Provider.rs` | compile step in `CoreProvider::realize`; hangar-store artifact, hash-keyed |
| `Source/FFI.rs` | vendored offline cargo (`[source] replace-with`, `--offline`); archive/db crate pins + `needs_*` flags |
| `Source/Lock.rs` | vendored-crate hash-pin field/fold into `compute_fingerprint` (D-BFS1) |
| `Source/Prelude/Archive.rs` | new — zip/tar/flate2 runtime (only code touching those crates) |
| `Source/Prelude/Db.rs` | new — rusqlite (bundled) runtime |
| `examples/features/` | one example + expected output per new module (I5) |
| `docs/spec/roadmap.md` | mark `jet.archive`/`jet.db` available (E2-M9) |
| `docs/spec/spec.md`, `syntax-decisions.md` | module surfaces; flip D-DEP-ARCHIVE1/DB1/BFS1 to Implemented |
