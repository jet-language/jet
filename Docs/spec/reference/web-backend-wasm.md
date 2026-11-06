# Web backend: JavaScript DOM operations and WebAssembly logic

The web backend partitions the checked program between JavaScript for browser/DOM work
and WebAssembly for eligible logic. This page is for web application authors
and compiler contributors choosing a target, reading partition diagnostics, and
checking the source-backed examples. The Jet-hosted CLI builds the web target
from the IR (see [architecture](../architecture.md), the IR consumers): today
it selects the whole program to WebAssembly, and the JavaScript/WebAssembly
partition below is not built yet. Web diagnostics are registered in
[`Compiler/Prelude/Diagnostics.jet`](../../../Compiler/Prelude/Diagnostics.jet).

The ratified decisions are D-WEBBACKEND1, D-WASM1, D-JSBIND1, D-WEBKIND1, and
D-DOMGEN1. Per-function partition pins use `#Target(JS)` and `#Target(Wasm)`;
`#WasmExport` marks a Wasm entry called from generated JavaScript.

## Build and inspect

`jet build --target web FILE` (also `--os=web`, or `os: web` in the selected
profile) writes five files beside the entry in `.jet/build/`, whatever the
entry is named: `app.wasm` (the program partition), `rt.wasm` (the runtime
partition), `web.manifest.json`, the entry `app.js` and its host
`jet_dom_runtime.js`. `node .jet/build/app.js` runs the program with the
process's arguments and environment and exits with its status. A page or a
harness imports `jet_main` from `app.js`: it resolves when the program exits 0
and rejects with `JetHostExitError` (its `status`) otherwise, or
`JetHostWasmError` on a trap. `instantiateWasm` and `takeWasmError` from
`jet_dom_runtime.js` drive the modules directly.

```sh
jet build --target web Examples/features/basics/hello.jet
node Examples/features/basics/.jet/build/app.js
```

`jet run`, `jet dev` and `jet test` run Linux x86-64 images only and refuse
`os: web`; a package's `build: fn` targets refuse it too. Under node the host
file system is the program's; a page has none, so file calls fail with
ENOSYS. The host runs one thread: a program that starts a `task` stops with
E3001 (`tasks are not available on the web target yet`).

When the page has a `document`, the host shows what the program's `core.ui`
backend paints. Each paint (a `ui.mount`, or a `paint` after `layout`)
replaces the children of `#jet-app`, which the host creates in `document.body`
if the page has none: one element per leaf, placed absolutely at its frame (a
`button` for a button, an `input` for a text input, a `span` for text, a `div`
for any other node; a box stacks its children and adds no element). A click on
a button runs its Jet handler once the program's `run` has returned 0, and the
paint that handler causes (for example a `ui.reactive_render` effect re-running
on a signal it read) replaces the elements again. Under `node app.js` there is
no document and nothing is recorded.

## Partition and ABI

The hybrid boundary is:

- view and DOM operations become generated JavaScript calls through
  `jet_dom_runtime.js`;
- pure native computation becomes a `wasm32-unknown-unknown` module with a
  generated loader and bridge;
- explicit `#Target(JS)` compute calls use the browser-owned WebGPU Prelude
  adapter, which awaits queue work and readback;
- browser partition inference follows Browser effect facts and `#Target`
  ceilings.

The bridge accepts ABI-safe scalars, `String`, lists, maps, and explicitly
codable values. A value whose representation is not part of that boundary must
be represented or rejected; it is not copied through a guessed JavaScript or
Wasm layout. `#WasmExport` makes a selected Wasm entry callable from generated
JavaScript. `web.manifest.json` records module-to-bucket assignments for the
loader and `--explain-partition`.

This is a checked-program boundary, not a second web language. It does not imply
full Jet-to-JavaScript transpilation, WASI imports, or that every `core.ui`
provider is available in a browser.

## Diagnostics and failure behavior

Unsupported constructs fail before emission with registered diagnostics:

- `E-WEB-UNSUPPORTED` — the construct cannot be lowered for the selected
  web partition;
- `E-WEB-CROSS-PARTITION` — a direct JS/Wasm call lacks a generated bridge;
- `E-WEB-ABI-TYPE` — a value is not valid at the JS/Wasm boundary;
- `E-WEB-TARGET-BROWSER` — a Wasm function requests browser-only targeting or
  DOM access;
- `E-WEB-RUN` — a web-targeted program was sent to the native execution path.

Browser WebGPU calls use the browser-owned asynchronous Prelude. Unsupported
providers and operations return typed unsupported results; they do not silently
fall back to CPU. Native hosts retain the typed fail-closed WebGPU provider.

## Source-backed examples

The top-level source set under `Examples/features/web/` is the contract checked
by [`tests/cases/web_examples_doc/web_examples_are_documented_and_have_expected_outputs.case`](../../../tests/cases/web_examples_doc/web_examples_are_documented_and_have_expected_outputs.case).
Each file is named here so documentation review cannot silently omit a new example:

- `package.jet`
- `app_hello.jet`
- `app_typed_args.jet`
- `browser_surface.jet`
- `forms_table.jet`
- `query_lifecycle.jet`
- `state_surface.jet`
- `store_history.jet`
- `ui_showcase.jet`
- `ui_web_click.jet`
- `ui_web_mount.jet`
- `ui_web_reactive.jet`
- `web_app.jet`
- `web_compute.jet`
- `web_compute_webgpu.jet`
- `web_hello.jet`
- `web_wasm_callback.jet`
- `web_wasm_for_in.jet`
- `web_wasm_int_export.jet`
- `web_wasm_list.jet`
- `web_wasm_list_string.jet`
- `web_wasm_map.jet`
- `web_wasm_range.jet`
- `web_wasm_string.jet`
- `web_wasm_string_param.jet`

Expected output and browser harness artifacts live under
`Examples/features/expected/web/`; browser outputs use the `*.web.out` or
`*.harness.out` suffixes. The example source and harness are the executable
truth for API spellings and output behavior.

## Related boundaries

- Target markers and ratified web decisions: [syntax decisions](../syntax-decisions.md).
- `core.web` browser storage and events: [Core library](core-library.md).
- General diagnostic conventions: [diagnostics](../diagnostics.md).
