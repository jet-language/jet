# c2fizfx3 — Web backend: JS DOM ops for views + WASM for logic

**Status:** READY — all architecture decisions ratified (D-WEBBACKEND1/WASM1/JSBIND1/WEBKIND1/DOMGEN1=A).
Amended by D-MARK-TARGET1=A (ratified 2026-07-11, card #498): the bare
per-function `#Wasm`/`#Js` overrides below are retired — `#Target(Wasm)` /
`#Target(Js)` is the one spelling for both the ceiling and the per-function
override. `#WasmExport` is untouched.

## Goal

Emit JS DOM operations for view/browser code and WASM for pure logic/compute, with a
JS bridge calling into the WASM module. No full Jet-to-JS transpilation. The partition
is inferred from effects, with expert override markers.

## Architecture decisions locked in

| Decision | Outcome |
|---|---|
| D-WEBBACKEND1=A | Hybrid: view/UI → JS DOM; pure/compute → WASM |
| D-WASM1=A | `Browser` effect added to D-EFF4 closed set; partition by effects; `#Target(Wasm\|Js)` module-level ceilings + per-function `#Wasm`/`#Js`/`#WasmExport` |
| D-JSBIND1=A | ABI: scalars, String, Codable structs/enums, List/Map of ABI-safe values; generated adapters |
| D-WEBKIND1=A | Browser-focused WASM + generated JS loader (no WASI) |
| D-DOMGEN1=A | First-party JS runtime shim for create/update/event wiring |

## Implementation steps

1. **Effect extension** — add `Browser` to the closed effect set in `Source/Sema/Effects.rs`
   (amending D-EFF4). `Browser` = any call into JS DOM APIs.

2. **Syntax.rs markers** — register `#Target(Wasm|Js)`, `#Wasm`, `#Js`, `#WasmExport`
   per I7. PascalCase (D-MARKER-CANON1=A).

3. **Partition sema pass** — walk the module graph; assign each function to a partition
   bucket (JS or WASM) based on:
   - Direct `Browser` effect → JS bucket.
   - Pure/no-effect → eligible for WASM.
   - `#Target(Wasm)` / `#Js` / `#Wasm` overrides explicit ceiling.
   - Cross-bucket call from wrong direction → E-WEB-CROSS-PARTITION.

4. **JS boundary type check** — at every JS/WASM call site, verify parameter/return types
   are ABI-safe (scalars, String, Codable, List/Map of ABI-safe). Non-ABI type →
   E-WEB-ABI-TYPE.

5. **WASM codegen** — add a `WasmBackend` behind the existing codegen seam. Target:
   `wasm32-unknown-unknown`. Emit `.wasm` module + generated JS loader (thin wrapper that
   instantiates the module and bridges exported functions).

6. **JS DOM codegen** — emit calls into a small first-party JS runtime shim
   (`jet_dom_runtime.js`). The shim exposes create/update/event-wiring primitives.
   Jet view code lowers to shim calls.

7. **Partition manifest** — driver emits a `web.manifest.json` listing which modules went
   to which bucket; used by the loader and for diagnostics.

8. **Examples + golden tests** — `examples/features/NN_web_hello.jet` (DOM hello),
   `NN_web_compute.jet` (WASM compute called from JS). Golden test on `--target web`.

9. **Diagnostics** — E-WEB-CROSS-PARTITION (doc + snapshot), E-WEB-ABI-TYPE (doc + snapshot).

## Verification

- Build and run `examples/features/NN_web_hello.jet` with `jet build --target web`.
- Golden test: JS + WASM artifacts match expected outputs.
- Snapshots for E-WEB-CROSS-PARTITION and E-WEB-ABI-TYPE.
- `nix develop -c cargo test` fully green.

## Decision status

No open decisions. All architectural choices ratified. I6 note: `wasm32-unknown-unknown`
target is a `rustc` backend, not an external crate — I6 holds.
