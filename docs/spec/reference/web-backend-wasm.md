# Web backend: JavaScript DOM operations and WebAssembly logic

The web backend partitions checked TIR between JavaScript for browser/DOM work
and WebAssembly for eligible logic. This page is for web application authors
and compiler contributors choosing a target, reading partition diagnostics, and
checking the source-backed examples. The executable boundary is implemented by
[`Source/CmdCompile.rs`](../../../Source/CmdCompile.rs) and
[`crates/jet-foundation/src/WebPartition.rs`](../../../crates/jet-foundation/src/WebPartition.rs);
web diagnostics are registered in
[`crates/jet-codegen/src/Prelude/Diagnostics.jet`](../../../crates/jet-codegen/src/Prelude/Diagnostics.jet).

The ratified decisions are D-WEBBACKEND1, D-WASM1, D-JSBIND1, D-WEBKIND1, and
D-DOMGEN1. Per-function partition pins use `#Target(JS)` and `#Target(Wasm)`;
`#WasmExport` marks a Wasm entry called from generated JavaScript.

## Build and inspect

Build JavaScript and WebAssembly artifacts explicitly:

```sh
jet build --target=web examples/features/web/web_compute.jet
jet build --target=web --explain-partition examples/features/web/web_compute.jet
jet dev --target=web examples/features/web/ui_web_click.jet
```

`--explain-partition` reports the module-to-bucket assignment used by the
loader. A web-targeted `jet run` is not a browser runner: attempting to execute
one produces `E-WEB-RUN`. Use `jet build` for artifacts and `jet dev` for the
browser development surface rather than relying on a native run fallback.

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

This is a checked TIR boundary, not a second web language. It does not imply
full Jet-to-JavaScript transpilation, WASI imports, or that every `core.ui`
provider is available in a browser.

## Diagnostics and failure behavior

Unsupported constructs fail before emission with registered diagnostics:

- `E-WEB-TIR-UNSUPPORTED` — the construct cannot be lowered for the selected
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

The top-level source set under `examples/features/web/` is the contract checked
by [`tests/web_examples_doc.rs`](../../../tests/web_examples_doc.rs). Each file
is named here so documentation review cannot silently omit a new example:

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
`examples/features/expected/web/`; browser outputs use the `*.web.out` or
`*.harness.out` suffixes. The example source and harness are the executable
truth for API spellings and output behavior.

## Related boundaries

- Target markers and ratified web decisions: [syntax decisions](../syntax-decisions.md).
- `core.web` browser storage and events: [Core library](core-library.md).
- General diagnostic conventions: [diagnostics](../diagnostics.md).
