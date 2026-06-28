# Epoch 3 pillar — formal plugin API

**Status:** owner-ratified deferral (2026-06-16, D-DX5-B). **Epoch 3 only.**

## Epoch 2 (ships now)

**D-DX5-A — PATH discovery.** Unknown `jet <cmd>` → exec `jet-<cmd>` on PATH
(same model as `git-lfs`). Zero ABI, zero registry.

```
$ jet flamegraph app.jet   →   jet-flamegraph app.jet
```

## Epoch 3 (this doc)

**D-DX5-B — optional formal plugin API** for tools that need:

- Shared typed AST / sema hooks (custom lints, codegen passes).
- Stable registration and versioning.
- Curated registry with security review.

PATH discovery **remains** for simple tools; the plugin API is for deep
integration only — not a replacement for `jet-*` binaries.

## Ratified substrate

- D-PLUGIN1=B: `target: plugin` means a sandboxed WASM module, safe by default, no
  `#Unsafe` gate for loading untrusted plugins.
- D-DEP-WASM1=A: wasmtime + Component Model is the approved runtime-side engine.
- PATH discovery remains for simple `jet-*` tools; WASM plugins are for deep typed
  integration.

Implementation plan lives in [`../sidequests/plugin-target.md`](../sidequests/plugin-target.md).

## Remaining design questions

- Version / ABI handshake over the Component Model.
- Export surface spelling (`#Plugin` marker vs manifest `entry:` + `pub` contract).
- Which compiler pipelines expose hooks first (parse-only, sema, after-codegen).

## Non-goals

- No required plugin framework to ship a `jet-*` helper on PATH.
- No breaking PATH discovery when the API lands.
