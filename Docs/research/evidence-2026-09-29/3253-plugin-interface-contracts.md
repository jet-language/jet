# #3253 — Typed shared-library and plugin interfaces (CORE-F026)

Date: 2026-09-29. Binary: `jet-debug-snapshot14` (`~/.cache/jet-luna/safe-jet.sh`), source rev `a8d8417c2`.
Author: Closer12 (evidence closer). No compiler, runtime or Core change was made.

## Question

What are the ABI, version, type, ownership, error and unload contracts of Jet's
one plugin boundary (the intrinsic WASM Component loader, `core.plugin.load`)?
How do Glaze Interfaces / REPE plugin / shared-library loading and baseline FFI
map onto it? Is raw-pointer safety inferred anywhere?

## Method

- Read: `Core/plugin/plugin.jet`, `crates/jet-pkg-model/src/Prelude/Plugin.rs`
  (header :1-83, load :1104-1238, close :1240-1250, call :1252-1464),
  `crates/jet-codegen/src/Prelude/CoreLib/Top/HandlesRaylib.rs:241-270`,
  `crates/jet-pkg-model/src/FFI.rs:2302-2305,2423-2430`,
  `Compiler/JetSema/Source/Sema/Calls/Plugin.jet:125-127`, `Docs/spec/spec.md:3928-3930`.
- Read primary peer sources (Glaze v8.3.0): `mkdocs.yml`, `docs/rpc/repe-plugin.md`,
  `docs/building-shared-libraries.md` (raw.githubusercontent.com, tag v8.3.0).
- Ran the witness `Examples/features/packages/sandbox_mathkit/run.jet` on the current binary,
  staged like `tests/golden.rs` (`fixture-state/` → `.jet/`) under
  `~/.cache/jet-test-scratch/Closer12/plugin/`.
- Ran the prebuilt UI test binary:
  `JET_UI_FILTER=plugin_e12 target-integ/debug/deps/diagnostic_snapshots-a3bde39c91baa0bf ui_snapshots`
  (built 2026-09-29 01:58, under the jetwork slice, 6G cap).

## Evidence

### Witness run (sandbox_mathkit)

1. From the repo root, `safe-jet.sh run Examples/features/packages/sandbox_mathkit/run.jet`
   → exit 1, six `E1257` ("plugin.load needs a registered Component interface"): the
   checkout has no `.jet/cache/api` snapshot; the golden harness stages one.
2. Staged with the top-level `fixture-state/` → exit 1, `E1257 Plugin interface
   plugin__mathkit has no export is_enabled` and `... greet`. The top-level
   `fixture-state/cache/api/plugin__mathkit.api` lists only `gcd`, `hypot`, `scale`;
   `sandbox_src/fixture-state/cache/api/plugin__mathkit.api` lists all five exports.
   The top-level snapshot is stale against `run.jet`.
3. Staged with the five-export snapshot:
   - `jet run run.jet` → `E1803 Application authority is undecided for Exec` (loading
     a plugin host needs `Exec` because the bridge crate is built by cargo).
   - `jet run --allow=Exec run.jet` → exit 1, `E0704 Couldn't fetch or build jet-foundation…`;
     the cargo error inside is
     `error[E0053]: method table_growing has an incompatible type for trait … expected usize, found u32`
     at the bridge `src/lib.rs:261`.
   - `jet run --interpret --allow=Exec run.jet` → `E2102 --interpret cannot be combined with build or artifact flags`.
   - `jet build run.jet` → exit 1, the same `E0704`/`E0053`, followed by the line
     `ok: build current (receipt 8c4641806af1)`.
   Root cause (read, not run): `FFI.rs:2303-2304` `FEATURED_DEPS` pins
   `wasmtime = { version = "26", … }`, while `WASMTIME_CRATE_SPEC` (`FFI.rs:2430`) and the unit
   assertion (`CompilerExtension.rs:1824`) say `"25"`. `Plugin.rs:151-154` implements
   `ResourceLimiter::table_growing(u32, u32, Option<u32>)` (wasmtime 25). Wasmtime 26 changed it to `usize`.

### UI fixtures

`JET_UI_FILTER=plugin_e12 … diagnostic_snapshots … ui_snapshots` → FAILED (1 mismatch):

- `plugin_e1257_version_mismatch`: matches (no mismatch reported).
- `plugin_e1260_unsupported_shape`: actual `(no errors)`; expected
  `E1260 … pub fn mixed isn't one homogeneous Int, Float, Bool, or Text shape`.
  `pub fn mixed(a: Int, flag: Bool) -> Bool` is now accepted by `jet::compile_plugin`.
  The spec (`Docs/spec/spec.md:3928-3930`) and the registered E1260 row still state the homogeneous-scalar rule.
  The self-hosted sema E1260 (`Plugin.jet:125-127`) only rejects non-Component shapes. Two meanings are live.

## Contract table (current source, with exercised status)

| Contract | Current law (source) | Exercised tonight |
|---|---|---|
| ABI | WASM Component Model via wasmtime (D-DEP-WASM1=A). Values cross as canonical Component `Val`s, checked against the component's actual `Type` before every call and after every return. The wire (`I/F/B/T/L/R/Q/N/P/K/X/V/E/G`) is only a transport envelope (`Plugin.rs:22-32`). No C ABI and no dlopen. | Not reached: bridge fails to compile (E0053 above). |
| Version | Load-time contract is the frozen interface snapshot `.jet/cache/api/plugin__<name>.api` (`api_version = 3`, D-PLUGIN-VERSION1). A removed or changed export is `E1257`. Sema resolves members only from a literal artifact with a snapshot. | `plugin_e1257_version_mismatch` passes. Sandbox witness shows E1257 fires for exports missing from the snapshot. |
| Type shape | Two live meanings. The spec and E1260 row say one homogeneous `Int`/`Float`/`Bool`/`Text` shape per export. Self-hosted sema says "recursively closed Component shapes" (Int, Float, Bool, String, #Codable record/list/Option/Result). | `plugin_e1260_unsupported_shape` FAILS: the mixed export is accepted. |
| Ownership | `JetPlugin { handle: u64 }` is a `Copy` handle (`HandlesRaylib.rs:245-248`). Stores are owner-thread, thread-local. Handle 0 is the error sentinel. Results are copied out before `post_return` (`Plugin.rs:1418-1421`), so no guest memory escapes. Source `Close` is pending #3072 (ready). | Not reached. |
| Error | Host returns `O:<value>` or `E:<kind>:<message>` with kinds `user-error`, `denied-authority`, `budget-exhausted`, `internal-defect` (typed boundary state, never backend strings). Budgets: 10M fuel, 16 MiB memory, 10k table elements, 2 s epoch, 16 MiB wire, depth 64. A load failure becomes a program stop `E3001` through `jet_runtime_stop` (`HandlesRaylib.rs:263-265`), not a returned error. | Not reached. |
| Unload | `jet_plugin_close(handle) -> Bool` removes the store. It returns `false` for unknown or closed handles and for a nested close during an active call (`Plugin.rs:1242-1250`). Load re-entry during a call is refused (`:1115-1120`). No source-level `Close` exists yet; the status is "host close exists, source Close pending (#3072)". | Not reached. |
| Authority | The load needs `FS.Read` (denied-authority otherwise). Host imports are preflighted against typed HostImportFacts and the lent Authority before linking. A zero-grant scope is used when nothing is declared. | Partially: the host program itself needs `Exec` authority to build the bridge (E1803). |

Criterion 2 (no raw-pointer safety inference): `Plugin.rs` contains no `unsafe`, `*const`,
`*mut` or `as_ptr` (grep, 0 matches). The plugin path moves only typed Component values
through wasmtime. The `.call` surface never exposes an address. The one native boundary
(C FFI, `jet-ffi-descriptor-v1`) is separate and audited under I1, and it is not part of this path.

## Peer accounting (Glaze v8.3.0, read from primary docs)

| Glaze capability | Glaze mechanism | Jet status |
|---|---|---|
| Typed interface discovery (`glz::iface`, `lib["my_api"]()`, `io->get<T>("/x")`) | Reflection metadata exported by the `glz_iface()` C symbol. Typed access is checked at runtime by type-name/hash | Supported, different mechanism. The frozen Component interface is checked at compile time (sema) and at load time (E1257). |
| Shared-library load (`glz::lib_loader`, dlopen of `.so/.dylib/.dll`, unload on destructor) | Native in-process code with a C ABI and no sandbox | Unsupported by design. No native dylib plugin API exists. The ask is routed to #3598 (ready); any addition needs its own ballot (new ABI and dependency). |
| REPE plugin (`repe_plugin_interface_version`, `repe_plugin_info`, `repe_plugin_call(bytes)`, optional init/shutdown) | Versioned C ABI with a byte-buffer RPC; the host owns version checks | Partial. The version check maps to the frozen snapshot plus E1257, the typed call maps to `jet_plugin_call`, and shutdown maps to `jet_plugin_close` (source Close pending #3072). There is no REPE wire protocol and no registry RPC; none is proposed here. |
| Mutable data members through the interface (`*x = 42`) | Raw pointer into the library's memory | Unsupported by design (I1). Values are copied across the boundary. |
| Thread-safety of plugin calls | Plugin's responsibility (thread_local buffers) | Owner-thread only; handles are thread-local. |

No genuine gap needs a new ballot from this card. The native dylib ask already lives on #3598.

## Verdict

PARTIAL. Criteria 1-3 are met as investigation: the contract table, the no-raw-pointer finding and the
peer accounting are above, with E1257 exercised. Criterion 4 is not met: interoperability itself is
broken tonight, because the plugin host cannot build on any tier. That breakage and the E1260
divergence are filed below as defects.

## Defects (for dispatch)

1. The plugin host bridge fails rustc. `FEATURED_DEPS` pins `wasmtime 26`, but `Plugin.rs`
   implements the wasmtime 25 `ResourceLimiter::table_growing(u32,…)`. Repro: stage
   `sandbox_mathkit` with the five-export snapshot, then run
   `jet run --allow=Exec run.jet` or `jet build run.jet` → E0704/E0053. Tiers: run and AOT.
   The interpreter can't take `--allow`.
2. `jet build` prints `ok: build current (receipt …)` after an E0704 failure (exit 1).
3. `plugin_e1260_unsupported_shape` is now `(no errors)`. The spec and E1260 row say the
   homogeneous-scalar rule; the binary accepts mixed scalars. One rule must win; this may be an owner question.
4. `Examples/features/packages/sandbox_mathkit/fixture-state/cache/api/plugin__mathkit.api` is stale.
   It lists 3 exports, while `run.jet` calls 5. Run through the golden staging, the example
   hits E1257 for `is_enabled` and `greet` before it reaches defect 1.
5. `jet run --interpret` refuses `--allow=…` (E2102 "build or artifact flags"). A host that needs
   `Exec` authority can then only be interpreted through a package block.
