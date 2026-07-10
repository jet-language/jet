# c81 — `plugin` target support

**Status:** UNBLOCKED on substrate; two surface gates remain. `D-PLUGIN1=B`
and `D-DEP-WASM1=A` are ratified — the architecture is decided (sandboxed WASM
via wasmtime + Component Model, safe by default, **no `#Unsafe` gate**). The
backend + loader can be built now. Two owner decisions still gate the *surface*:
the plugin export spelling (`D-PLUGIN-EXPORT1`) and the version/ABI handshake
(`D-PLUGIN-VERSION1`) — balloted below. The reserved keyword + E1210 rejection
stay until the backend lands.

Relation to the FFI program: a `plugin` builds *on* the same wasmtime runtime
the native JS-on-wasm binder reuses (`../plans/epoch-4/ffi-interop-program.md`
§4). One WASM runtime, two consumers — do not stand up a second.

## Goal

Turn the reserved manifest target `plugin` into a working build target +
loader: a package declares `target: plugin`, builds a sandboxed `wasm32`
Component Model module, and a host program (or the compiler) loads and calls it
across the typed `.wit` boundary — safe by default, no source-level unsafe gate.
Today `plugin` parses but is rejected with E1210 "has no backend yet".

## Ratified decisions (the design is settled)

- **D-PLUGIN1=B** (2026-06-25): `target: plugin` compiles to a **sandboxed WASM
  module**. Loading an untrusted plugin is safe by default with **no `#Unsafe`
  gate** (I1 holds by construction — the sandbox is the safety boundary).
  Native `cdylib` (option A) is a future expert opt-in on top of B, its own
  deferred card. RPC (option C) rejected.
- **D-DEP-WASM1=A** (2026-06-25): **wasmtime + Component Model** is the sandbox
  engine. Reuses the already-approved Cranelift (D-JITDEP1). The Component Model
  gives the typed host↔plugin interface (`.wit`, deny-by-default capabilities).
  wasmi (B) / wasmer (C) / own-now (D) rejected. **Runtime-side only** — I6
  holds, never in `Source/`; wrap wasmtime with the ffi-bridge pattern
  (`crates/jet-driver/src/Prelude/{Archive,Db}.rs` precedent), hash-pinned in
  `.jet/lock`; native-ize obligation = the frozen own-runtime end-state
  (option D).

This closes the old Owner-Q 1/2/3/5/7: it is a general application-plugin
substrate (Q1), WASM (Q2), in-process sandboxed (Q3), safe-by-default no gate
(Q5), and complements — does not replace — D-DX5-A `jet-*` PATH tools (Q7).

## Two remaining owner gates (ballots)

Both were explicitly deferred at ratification ("Owner Q1") and block only the
*surface*, not the substrate:

- **D-PLUGIN-EXPORT1** — how a plugin declares its exported surface: a new
  in-source `#Plugin { … }` marker (joins `#Test`/`#Bench`), vs manifest-level
  `entry:`/`export:` reusing D-TGT3/4 target fields, vs convention (`pub`
  surface frozen like `api: stable`, D-CAP4). Needs a decision ID + I7 entry in
  `Syntax.rs` whichever wins.
- **D-PLUGIN-VERSION1** — the load-time compatibility handshake: per-build ABI
  hash vs declared semver range vs reuse of the D-CAP4 `api: stable` capability
  freeze as the frozen `.wit` interface contract. Mismatch must be a clean Jet
  diagnostic (I2), never a loader crash.

Full ballot text for both is carried on card #5 for owner decision.

## Current state (verified)

- Keyword reserved — `crates/jet-foundation/src/Syntax.rs`
  `TARGET_RESERVED = ["benchmark","plugin"]`.
- Parse + rejection —
  `crates/jet-driver/src/Jetpack/PackageManifest/ParseBlocks.rs`: `plugin` ∈
  `TARGET_RESERVED` → `ManifestError::ReservedTarget`; diagnostic **E1210**
  "…has no backend yet" in `crates/jet-driver/src/Manifest.rs`. Reserved-target
  test uses `plugin`.
- `Target` enum in `crates/jet-driver/src/Jetpack/PackageManifest/mod.rs` — no
  `Plugin` variant yet.
- **No dynamic-loading machinery exists.** No `dlopen`/`libloading`/`cdylib`,
  no wasmtime embed yet. The rustc invocation (`crates/jet-driver/src/Compile.rs`)
  always emits an executable and never passes `--crate-type`. C-FFI links
  native C libs *into* a binary; it is not a loader.
- `benchmark` target (c80) is the precedent for turning a reserved
  non-kind target real — mirror its parse/realize plumbing.

## Implementation (unblocked; conditional only on the two surface ballots)

1. **`Target::Plugin` + wasm compile path.** Add the variant to
   `PackageManifest/mod.rs` (maps to no `PackageKind` — a plugin is loaded, not
   imported or PATH-installed). Teach `Compile.rs` a `wasm32` Component Model
   compile path for this target (first non-bin output in the codebase). Drop
   `plugin` from `TARGET_RESERVED`.
2. **Embed wasmtime, runtime-side (I6).** New bridge package wrapping the
   wasmtime crate via `extern rust` — never added to `jet`'s own `Cargo.toml`;
   emit the bridge template like the regex/sqlite/flate2 bridges. Hash-pin in
   `.jet/lock`.
3. **`PluginHost` loader.** New module: `discover` / `load` / typed `call`
   across the Component Model `.wit` interface, deny-by-default capabilities.
   Safe by construction — **no `#Unsafe` gate** (D-PLUGIN1=B). This host is the
   shared substrate the native JS-on-wasm binder also targets.
4. **Export surface** — implement once `D-PLUGIN-EXPORT1` ratifies. Generate
   the `.wit` from the chosen declaration form (marker / manifest / `pub`
   convention).
5. **Version handshake** — implement once `D-PLUGIN-VERSION1` ratifies. Check
   at load; reject incompatible plugins with a Jet diagnostic (I2), not a
   crash.
6. **Diagnostics + snapshots (I4).** New E-codes: load failure, missing
   export, version/ABI mismatch, capability-denied. Each with
   `docs/spec/diagnostics.md` text + `tests/ui` snapshot. Keep E1210's `plugin`
   test only if a sub-form stays reserved.
7. **Example + golden (I5).** A host package + a plugin package; expected
   output shows the host loading and calling the sandboxed plugin. Golden-
   enforced. Reuse `examples/features/` topic-dir layout.
8. **Tests + docs.** Manifest-parse (`Target::Plugin`), loader integration,
   `tests/decisions.rs` entries for `D-PLUGIN-EXPORT1` / `D-PLUGIN-VERSION1`;
   spec section; retire the shipped part of `../plans/epoch-3/plugin-api.md`
   into durable spec.

## Sequencing / gates

- **Substrate: none left.** D-PLUGIN1=B + D-DEP-WASM1=A cover it. Steps 1–3 and
  6–8's substrate half can start now.
- **Surface: `D-PLUGIN-EXPORT1` + `D-PLUGIN-VERSION1`** gate steps 4–5 (and the
  decision-test half of 8). Build the loader against a placeholder `.wit` while
  they decide; wire the real export/version once ratified.
- **Do c80 (`benchmark`) first** — it sets the non-kind-target parse/enum
  pattern c81 reuses. (May already be done; verify before starting.)
- **Shared runtime.** Coordinate the wasmtime embed with the FFI program's
  native JS-on-wasm host (`../plans/epoch-4/ffi-interop-program.md` §4) — one
  `PluginHost`/runtime wrapper, two callers (I8).
- **Compiler-extension plugins** (custom lints / sema hooks) remain the
  Epoch-3 `plugin-api.md` scope and D-DX5-A PATH tools — orthogonal to this
  application-plugin target; do not conflate (I8).
