# c50 — package build-from-source + M9 wave-2 (regex / archive / db)

**Status:** sidequest plan for board card **c50**. Refreshes the stale Epoch-3
plan (`plans/epoch-3/package-build-from-source.md`) which still said `pack.jet`
and predates D-REGEX1, D-DEP1's `pkg.jet` rename, and D-BUILD1 (the C-FFI/cargo
bridge). Two halves: (1) the **build-from-source mechanics** — already mostly
ratified, vetted here; (2) the **dependency-approval ballot** — under I6 + D-DEP1
each new Rust crate behind a wrapping package needs its own owner sign-off, like
regex got with D-REGEX1.

## Ratified ground this stands on (verified in syntax-decisions.md)

- **I6** — compiler `Source/` is zero external crates, forever. Stdlib
  sub-libraries may take an owner-approved crate to bootstrap until end of
  Epoch 3, then native-ize.
- **D-DEP1** (2026-06-17) — third-party deps ship as **FFI-wrapping Jet
  packages**: a normal Jet package wraps a crate via `extern rust "crate@version"`
  (S50) and exposes a clean Jet API. Consumers depend on the *package*, never the
  crate. Manifest is **`pkg.jet`** (`payload:` / `deps:` / `packages:`);
  `PAYLOAD_FILE = "pkg.jet"` in `Source/Syntax.rs`.
- **D-NET1** (2026-06-17) — `jet.tls` wraps `rustls` (the precedent: one package,
  one crate, owner-approved). `jet.http` → `jet.tls`.
- **D-REGEX1** (2026-06-21) — `jet.regex` on the `regex` crate, the **one**
  already-approved wave-2 crate. Standing obligation to native-ize before Epoch 3
  ends.
- **S50** (2026-06-12) — `extern rust "crate@version" { fn … = "rust::path"; }`.
  Version pin is **inline and authoritative**; required for non-std crates.
- **D-BUILD1** — the C-FFI / cargo bridge that compiles `extern rust` blocks;
  runs offline by default (`jet doctor` surfaces it). This is the "build a
  dependency's Rust/FFI from source" engine that c50's library half rides on.
- **D-JITDEP1** (2026-06-24) — precedent that a scoped, owner-signed crate is
  acceptable for a *runtime-side* capability (Cranelift) while the compiler stays
  zero-crate, with a frozen native-progression card. Same shape applies to each
  wave-2 crate.

## Half 1 — build-from-source mechanics (vet, don't re-decide)

`jet build` / `jet run` already compile a *program* jet → Rust → `rustc` → binary
(I2/I3). The gap is on the **jetpack** side: `Source/Jetpack/Provider.rs`
`realize()` stages a prebuilt `bin/` (executable) or raw source (library) but has
no step that **compiles a dependency from its Jet source** into a cached artifact.
Until that lands, a `library` dependency is consumed as staged source the module
resolver (U17) walks — fine for pure-Jet libraries, but a wrapping package whose
body is an `extern rust` block needs its crate compiled and linked.

What c50 builds:

1. **Compile step in `realize()`** — when a `library` package is realized, build
   its Jet source (jet → Rust → `rustc`) into a hangar-store artifact, hash-keyed
   on (source tree hash + crate pins + toolchain). Stays offline/deterministic:
   `jet build`/`run` never realize on demand — the artifact is pre-staged by
   `jetpack`.
2. **Crate compilation via D-BUILD1** — a wrapping package's `extern rust
   "crate@version"` block is compiled by the existing C-FFI/cargo bridge
   (D-BUILD1). The crate source must be available **offline** at build time; how
   it gets there (vendored vs fetched-then-locked) is the policy decision
   **D-BFS1** below.
3. **Cache invalidation** — an unchanged dependency (same source + pins +
   toolchain hash) hits the cache; a changed crate pin or source forces a rebuild.

**No language prerequisite** for the compile step — it builds on
`Provider.rs` + the hangar store + D-BUILD1. The one upstream policy choice that
*is* a real fork is D-BFS1 (where the crate sources live for an offline build),
balloted below.

### Exit criteria (unchanged from the E3 plan, re-pinned to `pkg.jet`)

- A `library` dependency with Jet source builds to a cached artifact during
  `jetpack build`; a downstream `use <pkg>` links it.
- `jet.regex` / `jet.archive` / `jet.db` exist as real packages (each its own
  `pkg.jet`) with tests and examples.
- Rebuild is incremental: an unchanged dependency hits the cache.

## Half 2 — the dependency-approval ballot

I6 + D-DEP1 require that **each** crate behind a wrapping package be approved
individually by the owner. `regex` is done (D-REGEX1). c50 needs:

| Capability | Wrapping package | Crate(s) to approve | Card |
|---|---|---|---|
| regex | `jet.regex` | `regex` | **done — D-REGEX1** |
| archive (zip/tar) | `jet.archive` | which zip + tar crate | **D-DEP-ARCHIVE1** |
| sqlite | `jet.db` | `rusqlite` vs `sqlite` (vs bundled-C) | **D-DEP-DB1** |
| build-from-source policy | (infra) | vendored vs fetched-then-locked sources | **D-BFS1** |

Each wrapping package carries the **standing native-ize obligation** D-REGEX1
established (replace the crate with an in-house implementation before the
dependency-free end state, I6). Approving the crate approves the *bootstrap*, not
a permanent dependency.

The full ballot cards (house format, with worked `pkg.jet` + `extern rust`
examples per option) are drafted in the scratch file for owner review:
`scratchpad/ballot_c50.md` → group heading **"Package build-from-source + M9
wave-2 — board card c50"**.

## Files (when this burns down — not part of this PM task)

| File | Change |
|---|---|
| `Source/Jetpack/Provider.rs` | compile step in `realize()`; hangar-store artifact, hash-keyed |
| stdlib `jet.archive/pkg.jet` + body | new wrapping package, `extern rust` over the approved crate |
| stdlib `jet.db/pkg.jet` + body | new wrapping package, `extern rust` over the approved sqlite crate |
| `examples/features/` | one example + expected output per new package (I5) |
| `docs/spec/roadmap.md` | mark `jet.archive`/`jet.db` available (E2-M9 note) |
| `docs/spec/syntax-decisions.md` | log D-DEP-ARCHIVE1 / D-DEP-DB1 / D-BFS1 on ratify |

## Native-ize tracking (I6 end state)

Each approved crate gets a frozen native-progression card (D-JITDEP1 precedent):
`regex` → in-house RE2-style engine; `archive` → native deflate/zip + tar
reader; `sqlite` → either keep sqlite-as-C (it is a stable, public-domain C
artifact, arguably the native form) or a native embedded store. These are
post-c50; logged so the obligation isn't lost.
