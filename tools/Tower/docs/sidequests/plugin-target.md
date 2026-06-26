# c81 — `plugin` target support

**Status:** BLOCKED on owner decisions. This card cannot start implementation
until the owner settles the questions in `## Open Owner-Q`. The reserved keyword
+ E1210 rejection ship today and stay until the design lands. This plan folds in
the Epoch-3 `plugin-api.md` milestone doc (D-DX5-B deferral) and reconciles it to
current reality.

## Goal

Turn the reserved manifest target keyword `plugin` into a working build target +
loader: a package declares `plugin`, builds a loadable artifact, and a host
program (or the compiler) loads and calls it across a defined, versioned,
I1-safe ABI boundary. Today `plugin` parses but is rejected with E1210 "has no
backend yet".

This is a **major design surface** — a plugin ABI + dynamic loader is a language
feature, not wiring. Most of the user-facing shape is unsettled (see Open
Owner-Q). The implementation section below is conditional on those answers.

## Current state (verified)

- Keyword reserved — `Source/Syntax.rs:1075` `TARGET_RESERVED = ["benchmark","plugin"]`.
- Parse + rejection — `Source/Jetpack/PackageManifest/ParseBlocks.rs:176-181`:
  `plugin` ∈ `TARGET_RESERVED` → `ManifestError::ReservedTarget`
  (`mod.rs:175-177`).
- Diagnostic — `Source/Manifest.rs:351-356`, code **E1210** "…has no backend yet
  (reserved for a future increment)". Reserved-target test `mod.rs:374-384` uses
  `plugin`.
- `Target` enum — `Source/Jetpack/PackageManifest/mod.rs:74-80` — no `Plugin`
  variant.
- **No dynamic-loading machinery exists anywhere.** Verified: no `dlopen`, no
  `libloading`, no `cdylib`/`dylib`, no `#[no_mangle]`, no plugin registry. The
  only `extern "C"` is C-FFI scaffolding (`Source/Prelude/Core.rs:517`). The
  rustc invocation (`Source/CmdCompile.rs:671-723`) always emits an executable
  (`-o bin`); it never passes `--crate-type` and has no shared-object path. C-FFI
  (`Source/CFFI.rs`) links *native C libs into* a Jet binary; it is not a plugin
  loader and is a link dep, not a load-at-runtime path.
- Compiler-extension story today (the only shipped "plugin"): **D-DX5-A** PATH
  discovery — unknown `jet <cmd>` execs `jet-<cmd>` on PATH (`git`-style), zero
  ABI. This is unrelated to a manifest `plugin` *target*.

So c81 starts from zero on the load/ABI side. Nothing can be mirrored from an
existing target backend except the trivial parse/realize plumbing.

## Decision context (ratified)

- **D-TGT2** (2026-06-21): `plugin` is a reserved target keyword, rejected with a
  "no backend yet" message (`syntax-decisions.md:1781`).
- **D-DX5 / D-DX5-A / D-DX5-B** (2026-06-16): PATH `jet-*` discovery ships now; a
  *formal plugin API* is owner-ratified **deferred to Epoch 3**
  (`syntax-decisions.md:2812`; milestone doc `tools/Tower/docs/plans/epoch-3/plugin-api.md`).

**What `plugin-api.md` decided and left open (folded in here):**
- Decided: PATH `jet-*` discovery is the Epoch-2 mechanism and *remains* even
  after a formal API lands; it is not replaced. A formal plugin API is "for deep
  integration only" (shared typed AST / sema hooks, custom lints, codegen passes,
  stable registration + versioning, a curated reviewed registry).
- Explicitly left open by that doc: in-process vs out-of-process (security vs
  latency); stable C ABI vs WASM sandbox vs LSP-style JSON-RPC; which compiler
  pipelines expose hooks (parse-only / sema / after-codegen).
- Reconciliation gap: `plugin-api.md` is scoped entirely to **compiler-extension
  plugins**. The manifest `plugin` *target* most naturally reads as a **general
  loadable-plugin artifact for any Jet host application**. These are different
  surfaces (see Owner-Q 1). The Epoch-3 doc does not settle which one the target
  builds — that is the first question to answer.

## Implementation (staged — conditional on Open Owner-Q)

This is the shape *once decisions land*; do not start before then.

1. **Define what a plugin target compiles to.** Depends on Owner-Q 2 (ABI
   substrate). E.g. WASM → compile entry to a `wasm32` module; native dylib →
   teach `CmdCompile.rs` a `--crate-type cdylib`/`dylib` path (the first non-bin
   crate type in the codebase) with `#[no_mangle]` export shims. Add
   `Target::Plugin` to `mod.rs:74-80`; like test/example it maps to no
   `PackageKind` (it is neither imported nor PATH-installed — it is loaded).

2. **ABI boundary / export surface.** Depends on Owner-Q 3-4. Define how a plugin
   declares its exported entry points and the host-visible contract (a `#Plugin`
   block? a designated `pub` signature set? a manifest `api:`-style freeze?).
   This is new front-end surface and needs its own decision IDs + I7 entries in
   `Syntax.rs`.

3. **Loader.** Depends on Owner-Q 2. New module (e.g. `Source/PluginLoader.rs`)
   that loads the artifact and calls across the boundary. Under I1 (Owner-Q 5):
   if native dylib, the load+call is inherently unsafe and must sit behind an
   `@unsafe`/`@audit` gate — generated `unsafe` requires a source gate (and note
   the golden.rs "unsafe" substring grep). If WASM, the sandbox is I1-clean by
   construction and preferred for the safe default.

4. **Versioning.** Depends on Owner-Q 6. ABI/version handshake at load; reject
   incompatible plugins with a diagnostic, not a crash (I2 — rustc/loader never
   speaks raw to users).

5. **Diagnostics + snapshots (I4).** Drop `plugin` from `TARGET_RESERVED`; new
   codes for load failure, ABI/version mismatch, missing export, unsafe-gate-
   required — each with `docs/spec/diagnostics.md` text + `tests/ui` snapshot.
   Keep E1210's `plugin` test only if some sub-form stays reserved.

6. **Example + golden (I5).** A host package + a plugin package; expected output
   shows the host loading and calling the plugin. Golden-enforced.

7. **Tests + docs.** Manifest-parse (`Target::Plugin`), loader integration,
   `tests/decisions.rs` for the new decision IDs; spec sections; retire the
   relevant part of `plugin-api.md` into the durable spec once shipped.

## Sequencing / gates

- **Hard gate: owner decisions below.** Nothing in steps 1-7 is safe to start
  until Owner-Q 1-2 (at minimum) are ratified — they determine whether the
  codebase grows a WASM toolchain dependency, a `cdylib` rustc path, or an
  out-of-process RPC layer. These are mutually exclusive architectures.
- **I6 watch.** A WASM substrate likely pulls a runtime; per I6 no external
  crates in `Source/`, and any stdlib external dep needs owner approval. Flag the
  dependency cost in the ABI decision.
- **Epoch placement.** `plugin-api.md` puts the *formal plugin API* in Epoch 3.
  If the manifest `plugin` target is the compiler-extension surface (Owner-Q 1
  → A), c81 inherits that Epoch-3 timing. If it is a general app-plugin feature
  (→ B), it is a new language feature whose epoch the owner must place.
- Do c80 (`benchmark` target) first — it is unblocked and sets the
  non-kind-target pattern; c81 reuses the parse/enum plumbing.

## Open Owner-Q

This card very likely needs **all** of these answered before implementation.
Candidate menus + recommendations follow; the owner decides — none picked here.

### Owner-Q 1 — What does a `plugin` target actually build? (foundational)

The reserved keyword does not say. Two distinct products share the word "plugin":

- **A. Compiler-extension plugin** — extends `jet` itself: custom lints, sema
  hooks, codegen passes (the `plugin-api.md` scope). Host = the compiler.
- **B. General application plugin** — a loadable module any Jet *program* loads
  at runtime to extend itself (think: an app's plugin folder). Host = the user's
  app.
- **C. Both**, sharing one ABI/loader.

*Recommendation:* clarify first; everything downstream forks here. B is the
reading the manifest surface most invites (a package "is a plugin"), and it is a
language feature with broad reach; A is already partly served by D-DX5-A PATH
discovery for shallow tools and is Epoch-3-deferred for deep hooks. Lean B (or C
with B as the general substrate and A as a specialization), but this is the
owner's call.

### Owner-Q 2 — ABI substrate

- **A. Native dynamic library** (`cdylib`/`dylib` via rustc) + a C ABI boundary.
  Fast, zero new runtime; but crossing it is `unsafe` (I1 cost), brittle across
  compiler versions, and adds the first non-bin rustc path.
- **B. WASM sandbox** (`wasm32` component/module + a host runtime). I1-clean by
  construction (sandboxed), portable, versionable; but pulls a WASM runtime
  dependency (I6 owner-approval gate) and has call-boundary latency + a
  marshaling story.
- **C. Out-of-process + JSON-RPC** (LSP-style). Strong isolation, language-
  agnostic; highest latency, process-management complexity, no shared typed AST.

*Recommendation:* if compiler hooks (Q1-A) need the typed AST, that pushes toward
A or B in-process; for a safe *default* under I1, B (WASM) is the cleanest — the
sandbox is the safe-by-default story and native dylib can be the expert
`@unsafe` opt-in later. But the runtime dependency (I6) is a real cost the owner
must weigh. Decide A vs B vs C; they are mutually exclusive foundations.

### Owner-Q 3 — In-process vs out-of-process (if not implied by Q2)

Security/isolation vs latency/shared-state. (Q2-C forces out-of-process; A/B can
be either.) *Recommendation:* in-process for compiler hooks (need shared typed
AST), with the sandbox (Q2-B) providing isolation without a separate process.

### Owner-Q 4 — How a plugin declares its exported surface

- **A. New in-source marker** (e.g. `#Plugin { … }` joining the `#Test`/`#Bench`
  family) listing exported entry points.
- **B. Manifest-level** — the `plugin` target's block names the export module /
  entry, reusing `entry:`/`name:` target fields (D-TGT4/D-TGT3).
- **C. Convention** — all `pub` items in the entry module are the ABI, frozen
  like `api: stable` (D-CAP4).

*Recommendation:* B + C (manifest declares the target + entry; `pub` surface is
the contract, frozen via the existing `api:` freeze machinery) avoids a new
in-source keyword. A is cleaner if compiler-hook plugins need to tag *which*
pipeline phase each export hooks (parse/sema/codegen). Needs a decision ID + I7
entry whichever way.

### Owner-Q 5 — I1 safety model for loading

A plugin crossing a native ABI is inherently unsafe. Options:
- **A. WASM sandbox** — safe by default, no gate needed (ties to Q2-B).
- **B. Native + mandatory `@unsafe`/`@audit` gate** — the load site is expert-
  tier, audited; matches I1's "expert opt-in, never the default footgun".
- **C. Native but trusted-registry-only** — curated reviewed registry
  (`plugin-api.md` mentions this) substitutes for source-level gating.

*Recommendation:* A for the default; B available as the expert escape hatch. Do
not make an unsafe native load the default (I1). C is a distribution policy, not
a substitute for the safety model.

### Owner-Q 6 — Versioning / compatibility handshake

How does the host detect an incompatible plugin? Per-build ABI hash vs a declared
semver compatibility range vs a stable frozen interface (D-CAP4 `api: stable`
reuse). *Recommendation:* reuse the c129 capability-signature freeze
(`ApiMode::Stable`, `mod.rs:101-103`) as the interface contract and reject
mismatches with a clean diagnostic (I2). Confirm direction with the owner.

### Owner-Q 7 — Relationship to D-DX5-A PATH discovery

Does the `plugin` target *replace*, *complement*, or stay *orthogonal* to
`jet-*` PATH tools? *Recommendation:* complement — `plugin-api.md` already states
PATH discovery remains for shallow tools and the formal API is "deep integration
only". Confirm so we do not build a second mechanism for shallow tools (I8).
