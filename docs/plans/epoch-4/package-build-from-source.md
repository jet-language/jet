# Package build-from-source + ring package shipping — #99 plan

**Card:** `c1rixz5d` (#99, was Tower c50). **Priority:** P1, first in the
jetpack lane (owner 2026-07-03: `#99 → #176 → #90 → #3 → #13`).

Two facets the card names, which may stage separately:

1. **Build-from-source** — jetpack realizes a dependency (Jet source + any
   hash-pinned FFI-bridge crate) into the hangar by building it, not by fetching
   a Nix store path.
2. **Ring package shipping** — the first-party `core.*` libraries reach a build
   as realized packages, not only as compiler-embedded templates.

## Shipped baseline (what already exists)

- `CoreProvider` in `crates/jet-driver/src/Jetpack/Provider.rs` realizes
  first-party Jet packages into the hangar with **no Nix**, behind the same
  `Realized` boundary as the `nix` provider.
- `build_rlib_from_cargo` (D-BFS1) already compiles a library package's
  `Cargo.toml` to an rlib **by shelling out to host `cargo`** and caches the
  target dir under `<store>/build-cache/<pkgdir>/`.
- Hangar ids are `<name>-<version>-<fp>` (D-PM1); lock is `.jet/lock`
  (`Store::lock_path`) with `name/version/source/fingerprint/dependencies`.
- Ring modules are **compiler-embedded**: `Syntax::is_ring_module` is true for
  `log|crypto|http|regex|reactive|archive|db`; `is_ring_module_staged` is the
  hangar-realization seam and is **always `false`** today.

So both facets have a working seam and a real gap. Do not assume the shipped
code matches the target; slice T0 is reconciliation.

## Ratified law this plan obeys

- `D-DEP1` / `D-BFS1`: compiler zero-external-crate (I6); crate-backed
  capability ships as a Jet package wrapping the crate via `extern rust`, source
  **vendored + hash-pinned**.
- `D-JPK-ADAPTER1=A`: build safety contract — probe read-only; network during
  build denied except locked `fetch(url, sha256:)`; ambient commands only via
  `BuildContext` with effect provenance in `.jet/lock`; outputs install only
  under the package output root; build tools are `Pkg` deps, never host
  `/usr/bin`; first build passes the U19 trust gate.
- `D-JPK-CACHE1=A`: outputs are output-hash-addressed hangar objects; the
  envelope (output hash, platform key, signature slot, provenance) is frozen
  into the hangar/lock schema in Phase A (see `implementation.md` A4). A
  build-from-source output is a cache-substitutable object like any other.
- `D-JPK-GC1`, `D-JPK-OFFLINE1`, `D-JPK-NONIX1`, `D-JPK-NODAEMON1`,
  `D-JPK-BUILDDBG1`: disk/offline/no-Nix/no-daemon/failed-build contracts apply
  unchanged to source builds.

## Owner gates on this card (blocking)

Two owner-facing choices this card hits are **not** yet ratified. Slices that
depend on them are marked; everything else is buildable now.

- **`D-JPK-RINGSHIP1`** — how `core.*` ring libraries are delivered
  (embedded / realized-source / hybrid-prebuilt-with-toolchain). Gates T3.
- **`D-JPK-BUILDTOOL1`** — what Rust/native toolchain compiles a user's
  `extern rust` bridge dep, and whether it is host-provided or a pinned,
  realized build dependency. Gates T2's reproducibility guarantee.

The plan below is written to the **recommended** outcomes
(`RINGSHIP1=C` hybrid, `BUILDTOOL1=A` pinned realized toolchain). If the owner
picks otherwise, only the marked steps change.

---

## Slice T0 — reconcile build-from-source with the envelope (no gate)

Goal: a source build produces a proper cache-substitutable hangar object.

1. **Failing test** `core_build_writes_envelope`: realizing a fixture
   first-party library package writes a hangar object whose lock record carries
   the A4 envelope (`output_hash`, `platform`, `signature` slot, `provenance`),
   not just `fingerprint`.
2. Route `CoreProvider::realize`'s output through the A4 envelope writer
   (shared with the nix provider). `output_hash` = content hash of the realized
   `out` tree (reuse `SHA256`); `platform` = target triple; `provenance` =
   the resolved source ref + build recipe id.
3. Make the `build-cache` target dir hangar-scoped and crash-cleaned
   (D-JPK-GC1: build scratch is hangar-scoped, swept on crash), not a sibling of
   the store root.

Exit: source-built outputs are envelope-complete and GC-reachable; snapshot of
`jet hangar du` counts them honestly.

## Slice T1 — build recipe + sandbox for source realization (no gate)

Goal: turn "realize from source" from "shell out to cargo" into a recipe run
under the D-JPK-ADAPTER1 safety contract.

1. **Failing tests**:
   - `build_denies_ambient_network`: a recipe step that reaches the network
     without a locked `fetch(url, sha256:)` fails with `E1236`.
   - `build_output_confined`: a step writing outside the package output root
     fails with `E1237`.
   - `locked_fetch_roundtrips`: `fetch(url, sha256:)` records source hash in
     `.jet/lock` and is offline-satisfiable on re-build.
2. Define a `BuildRecipe` over a fetched/staged source tree in
   `Jetpack/` (new `Recipe.rs`): steps are `fetch(url, sha256:)`,
   `exec(tool, args)` where `tool` is a realized `Pkg` dep, and
   `install(path, as:)` confined to the output root. This is the substrate
   `D-JPK-ADAPTER1`'s `Recipe.*` sits on (constructor spellings are
   `D-JPK-ADAPTNAME1`, card #176 — do not hardcode user-facing names here; use
   internal `BuildRecipe` types).
3. Effects/provenance: every `exec`/`fetch` records an effect entry in the lock
   (Adapter contract). Reuse the `Secret`-effect machinery pattern for the build
   effect vocabulary.
4. First build crosses the U19 trust gate (reuse the trust store from
   `implementation.md` U19).

Diagnostics: `E1236` `build-network-ungranted`, `E1237` `build-output-escape`,
`E1238` `build-tool-not-dep` (a recipe referenced a tool that is not a realized
`Pkg` dep — never falls through to host `/usr/bin`).

Exit: a fixture source package builds through the recipe under the sandbox;
network/output/tool violations each diagnose; the build is offline-reproducible
from the lock.

## Slice T2 — build toolchain for `extern rust` bridge deps  ⚠ gated on `D-JPK-BUILDTOOL1`

Goal: compile a user's vendored, hash-pinned bridge crate reproducibly.

Written to **`BUILDTOOL1=A`** (pinned, realized build toolchain):

1. **Failing test** `bridge_build_uses_pinned_toolchain`: building a fixture
   `extern rust` package resolves the Rust toolchain as a **realized hangar
   object** (fixture stands in for the prebuilt/substituted toolchain), not host
   `cargo`; the output hash is stable across two runs with different host
   `cargo` on PATH.
2. Replace `build_rlib_from_cargo`'s bare `Command::new("cargo")` with a call
   through the `BuildRecipe` `exec` step whose `tool` is the realized toolchain
   `Pkg` (a build dep). Native C toolchain (for crates with `build.rs` linking C)
   enters via the standing native-deps stopgap (nixpkgs on Nix machines;
   `D-UIDEVSHELL1` pattern), surfaced as a build dep, with the `D-JPK-NONIX1`
   honest-error path when neither a realized toolchain nor Nix is present.
3. The bridge crate source is vendored + hash-pinned per `D-BFS1`; the toolchain
   object id and the source hash both enter the output's `provenance`.

If **`BUILDTOOL1=B`** (host toolchain): keep host `cargo`, drop the stable-hash
guarantee, add `L02xx` `build-toolchain-unpinned` lint + an `E12xx`
`build-no-rust-toolchain` when host `cargo` is absent.
If **`BUILDTOOL1=C`** (prebuilt-only): no local Rust build; a missing per-platform
object is `E12xx` `prebuilt-only-miss` naming the publisher path.

Diagnostics: `E1239` `build-toolchain-unavailable` (recommended-path form:
no realized Rust toolchain and no Nix; names both fixes).

Exit: bridge builds are reproducible under the chosen model; host-toolchain
leakage removed on the recommended path.

## Slice T3 — ring package shipping  ⚠ gated on `D-JPK-RINGSHIP1`

Goal: `core.*` libraries reach a build as realized packages, version-coherent
with the pinned toolchain.

Written to **`RINGSHIP1=C`** (hybrid: prebuilt ring objects ride the toolchain
object; embedded templates remain the zero-config fallback):

1. **Failing test** `ring_module_realizes_from_hangar`: with a fixture toolchain
   object that carries prebuilt `core.http`/`core.regex` artifacts,
   `is_ring_module_staged("http")` becomes true and the loader resolves the ring
   module from the hangar object; the emitted program links the staged artifact,
   not the embedded template.
2. Flip `Syntax::is_ring_module_staged` from a constant `false` to a real query:
   "is this ring lib present as a realized hangar object for the active
   toolchain?" The toolchain object (`toolchain-as-dependency.md`) carries the
   per-platform prebuilt ring artifacts; realization stages them beside the
   toolchain.
3. Loader (`Loader.rs`) prefers the staged ring artifact when present; falls back
   to the compiler-embedded bridge template (`FFI.rs` / `CoreLib.rs`) when not —
   e.g. a freshly-built dev compiler with no realized toolchain. One resolution
   path, two sources, no user-visible difference.
4. Ring version = toolchain version by construction (no independent stdlib
   version matrix); `deps: { core.http }` in `pkg.jet` resolves to the staged
   object; unlisted ring imports still resolve via the fallback (rung-0 magic
   preserved).

If **`RINGSHIP1=A`** (embedded only): T3 collapses to "keep embedded; delete the
`_staged` seam"; the card's second facet is declared out of scope with an owner
note.
If **`RINGSHIP1=B`** (unbundled source packages): ring libs realize from a
first-party channel via T1's recipe path (a Rust build on first use unless
cached), independently versioned — add a stdlib-version-skew diagnostic.

Diagnostics: `E1240` `ring-artifact-platform-miss` (staged ring object absent
for the active platform; falls back to embedded, or errors under
`RINGSHIP1=B`).

Exit: a ring import resolves from the hangar when staged and from the embedded
template otherwise, with identical program behavior; version coherence asserted.

## Slice T4 — `jet build` / `jet registry vendor` / `jet inspect audit` surface (no gate)

1. `jet build` realizes all deps from source (or cache) and reports per-package
   `built | substituted | cached` (mirror the `D-JPK-CACHE1` example output).
2. `jet registry vendor` writes vendored + hash-pinned sources for every crate-backed dep
   (D-BFS1 / the `jet registry vendor` mention in the manifest law).
3. `jet inspect audit` reads the build recipes, effects, and locked source hashes
   **without executing** anything (D-BUILDSCOPE1 audit contract).

Tests: `jet_build_reports_source_states`, `jet_vendor_writes_pinned_sources`,
`jet_audit_reads_without_exec`.

---

## Exit criteria checklist

- [ ] T0: source-built outputs are envelope-complete, GC-reachable,
      cache-substitutable; `jet hangar du` counts them.
- [ ] T1: builds run under the D-JPK-ADAPTER1 sandbox; network/output/tool
      violations diagnose (`E1236`–`E1238`); offline-reproducible from lock.
- [ ] T2: bridge builds reproducible under the ratified `D-JPK-BUILDTOOL1`
      model; host-toolchain leakage handled per outcome.
- [ ] T3: ring libraries resolve from the hangar when staged, embedded fallback
      otherwise, per the ratified `D-JPK-RINGSHIP1` model; version coherence
      asserted.
- [ ] T4: `jet build` reports source states; `jet registry vendor` / `jet inspect audit`
      implemented and tested.
- [ ] Diagnostics `E1236`–`E1240` in `docs/spec/diagnostics.md` with snapshots.
- [ ] `D-JPK-OFFLINE1` golden sweep covers the source-build verbs.
- [ ] Full `cargo test` green; `examples/features/` example builds a dep from
      source (fixture) and shows the `built/substituted/cached` report.

## Sequencing

T0 → T1 land now (no gate). T2 lands when `D-JPK-BUILDTOOL1` ratifies; T3 lands
when `D-JPK-RINGSHIP1` ratifies (and depends on `toolchain-as-dependency.md`
T3 for the toolchain object that carries ring artifacts). T4 rides T0–T1 and
gains T2/T3 reporting as those land.
