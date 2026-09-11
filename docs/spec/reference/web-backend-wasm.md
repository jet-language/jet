# Web backend: JS DOM ops for views + WASM for logic

Vocabulary: [Jet vocabulary](../vocabulary.md).

The web backend is a hybrid partition for checked TIR and the D-JSBIND1 ABI.
Partition and boundary rules are ratified by D-WEBBACKEND1, D-WASM1, D-JSBIND1,
D-WEBKIND1, and D-DOMGEN1. Per-function partition pins use `#Target(Wasm)` and
`#Target(JS)`; `#WasmExport` marks a Wasm entry called from generated JS.


## Commands

```sh
# Build JS + Wasm artifacts into build/
jet build --target web examples/features/web/web_compute.jet

# Audit partition assignments
jet build --target web --explain-partition examples/features/web/web_compute.jet

# Native demo output (null backend, no browser)
jet run examples/features/web/web_hello.jet

# Programmable dev server (when the file defines dev())
jet dev examples/features/web/ui_web_click.jet
```

`#Target(Web)` on an entry file lets `jet build` / `jet dev` infer the web
backend without `--target=web`. `jet run` never infers web.

## Examples

Source-backed web examples live under `examples/features/web/`. Their expected
outputs live under `examples/features/expected/web/`; browser harness artifacts
use `*.web.out` or `*.harness.out` suffixes.


## Boundary and diagnostics

Do not assume full Jet-to-JS transpilation, WASI, or every `core.ui` backend on
web. Unsupported constructs are rejected before emission with typed diagnostics:

- unsupported TIR lowering emits `E-WEB-TIR-UNSUPPORTED`;
- a direct call across JS/Wasm partitions without a generated bridge emits
  `E-WEB-CROSS-PARTITION`;
- non-ABI values at a JS/Wasm boundary emit `E-WEB-ABI-TYPE`;
- Wasm `#Target(Browser)` or DOM access emits `E-WEB-TARGET-BROWSER`.

The complete `core.ui` backend matrix is not a web contract. Browser WebGPU
calls use the browser-owned async Prelude; unsupported providers and operations
return typed unsupported results and never fall back to CPU. Control-flow and
ABI behavior follows the checked TIR boundary rather than a separate web
language.

## Architecture (ratified, unchanged)

Hybrid partition: view/DOM code → generated JS calling `jet_dom_runtime.js`;
pure native compute → `wasm32-unknown-unknown` module with a generated
loader/bridge. Explicit `#Target(JS)` compute calls use the browser-owned
WebGPU Prelude adapter, which awaits queue work and readback; native hosts keep
the typed fail-closed WebGPU provider. Partition inference follows `Browser`
effect facts plus `#Target` ceilings.
`web.manifest.json` lists module→bucket assignments for loaders and
`--explain-partition`.

## Related docs

- Diagnostics: `docs/spec/diagnostics.md` (`E-WEB-*` family)
- `core.web` browser storage/events: `docs/spec/reference/core-library.md`
- Target markers: `docs/spec/syntax-decisions.md` (Web target section)

