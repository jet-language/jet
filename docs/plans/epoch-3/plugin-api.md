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

## Ratified + shipped substrate (c81)

- D-PLUGIN1=B: `target: plugin` means a sandboxed WASM module, safe by default, no
  `#Unsafe` gate for loading untrusted plugins. **Shipped** — see
  docs/spec/spec.md's "`target: plugin`" section.
- D-DEP-WASM1=A: wasmtime + Component Model is the approved runtime-side engine.
  **Shipped** — `crates/jet-driver/src/Prelude/Plugin.rs` (FFI-bridge pattern).
- D-PLUGIN-EXPORT1=A / D-PLUGIN-VERSION1=A: export surface (`pub` items +
  `export:` field) and version handshake (`Sema::ApiFreeze` snapshot diff).
  **Shipped** — this application-plugin substrate's two former "remaining
  design questions" are resolved; see docs/spec/spec.md.
- PATH discovery remains for simple `jet-*` tools; WASM plugins are for deep typed
  integration.

This application-plugin target (c81) is a *different, orthogonal* mechanism
from D-DX5-B below — a general host-loads-untrusted-plugin substrate, not
compiler-extension hooks. Don't conflate them (I8).

## Remaining design questions (D-DX5-B specifically — compiler-extension hooks)

- Which compiler pipelines expose hooks first (parse-only, sema, after-codegen).
- Whether D-DX5-B reuses the shipped c81 WASM Component Model runtime
  (`crates/jet-driver/src/Prelude/Plugin.rs`) as its own sandbox, or needs a
  different host-side surface for typed AST/sema access — an open question
  for whoever picks up D-DX5-B, not resolved by c81.

## Non-goals

- No required plugin framework to ship a `jet-*` helper on PATH.
- No breaking PATH discovery when the API lands.
