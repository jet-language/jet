# Web backend: JS DOM ops for views + WASM for logic (card #123)

**Status:** PARTIAL — hybrid backend ships for the checked TIR + D-JSBIND1 ABI
surface. Umbrella criteria 1–4 closed on child cards #701–#704 (2026-07-24).
Criterion 5 (this doc, examples, focused tests, scoped proof) is card #705.
Breadth beyond the lists below is **not** claimed complete.

Ratified: D-WEBBACKEND1, D-WASM1, D-JSBIND1, D-WEBKIND1, D-DOMGEN1 (all =A).
Per-function partition pins use `#Target(Wasm)` / `#Target(JS)` only
(D-MARK-TARGET1 retired bare `#Wasm` / `#JS`). `#WasmExport` marks a Wasm entry
called from generated JS.

## What ships today

| Area | Shipped | Proof |
|------|---------|-------|
| Checked TIR preflight + honest fallback | `validate_web_tir_support` before emit; miss → `E-WEB-TIR-UNSUPPORTED` | `web_tir_contract`, `web_build` |
| Effect-based partition + qualified identity | JS vs Wasm buckets, same-leaf collisions, cross-file imports | `web_partition` (15) |
| D-JSBIND1 ABI round-trips | scalars, `String`, `[Int]`/`[String]`, `Map<String,Int>`, Codable structs, callbacks/errors | `web_build` hostile harnesses |
| DOM shim + reactive UI | `ui.null_backend()` → `jet_dom_runtime.js`; signals/effects | `web_hello`, `ui_web_reactive`, browser harness |
| Wasm compute bridge | `#WasmExport` + `#Target(JS) run()` calling Wasm | `web_compute`, `web_wasm_*` examples |
| Companion HTML pages | `#HTML("page.html")`, `#Target(Web)` inference | `ui_web_click`, `ui_showcase` |
| Dev server + live reload | `fn dev()` + `core.web.devserver` | `web_dev` (7/8; canvas panel test unrelated) |
| Source maps + manifest | `web.manifest.json` partitions, JS source maps | `web_build` |
| Real browser acceptance | DOM, reactive, Wasm, bundle, maps | `web_browser` (1) |

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

## Examples (`examples/features/web/`)

| File | Teaches |
|------|---------|
| `web_hello.jet` | DOM shim round-trip via `core.ui` null backend |
| `web_compute.jet` | `#WasmExport` compute called from `#Target(JS) run` |
| `ui_web_reactive.jet` | `reactive_render` + DOM command stream |
| `ui_web_click.jet` + `ui_web_click.html` | Real clickable page, `#HTML`, exported `render` |
| `ui_showcase.jet` + `ui_showcase.html` | Flagship reactive UI + companion host page |
| `web_wasm_range.jet` | Wasm `for` range loops across the bridge |
| `web_wasm_for_in.jet` | Wasm `for-in` collection loops |
| `web_wasm_string.jet` | Wasm `String` return/export |
| `web_wasm_string_param.jet` | Wasm `String` parameter reconstruction |
| `web_wasm_list.jet` | Wasm `[Int]` ABI |
| `web_wasm_list_string.jet` | Wasm `[String]` ABI |
| `web_wasm_map.jet` | Wasm `Map<String,Int>` ABI (BigInt-safe) |
| `web_wasm_callback.jet` | Wasm callback + error hostile cases |

Golden outputs live under `examples/features/expected/web/`. Harness-only
artifacts (browser DOM) use `*.web.out` or `*.harness.out` suffixes.

## Scoped verification (card #705)

```sh
scripts/agent/jet-env cargo test --test web_build      # 48
scripts/agent/jet-env cargo test --test web_partition  # 15
scripts/agent/jet-env cargo test --test web_browser    # 1
scripts/agent/jet-env cargo test --test web_tir_contract # 2
scripts/agent/jet-env cargo test --test web_dev \
  -- --skip jet_dev_web_exposes_canvas_panel_and_graph  # 7 (canvas panel out of scope)
```

Full `cargo test` is not required for this slice. Known unrelated red:
`jet_dev_web_exposes_canvas_panel_and_graph` (Canvas project-edit protocol).

## Explicit non-goals / unsupported breadth

Do **not** assume full Jet-to-JS transpilation, WASI, or every `core.ui`
backend on web. The following remain outside the shipped web TIR boundary unless
a test proves otherwise:

- AST-side lowering or silent omission — unsupported constructs emit
  `E-WEB-TIR-UNSUPPORTED` at preflight (D-WEBTIR1).
- Cross-partition direct calls without a generated bridge →
  `E-WEB-CROSS-PARTITION`.
- Non-ABI types at JS/Wasm boundaries → `E-WEB-ABI-TYPE`.
- Wasm `#Target(Browser)` / DOM from Wasm → `E-WEB-TARGET-BROWSER`.
- Full `core.ui` backend matrix on web (GTK/TUI/native mobile rows in
  `core-library.md` stay unsupported).
- Method/map/stride/columnar forms not wired in `web_expr_supported` /
  `web_wasm_expr_supported` (see `crates/jet-codegen/src/Codegen/Web.rs`).
- Exhaustive executable-TIR coverage for every future language construct —
  tracked on umbrella #123 criterion 1 closure, not re-claimed here.

## Architecture (ratified, unchanged)

Hybrid partition: view/DOM code → generated JS calling `jet_dom_runtime.js`;
pure compute → `wasm32-unknown-unknown` module with a generated loader/bridge.
Partition inference follows `Browser` effect facts plus `#Target` ceilings.
`web.manifest.json` lists module→bucket assignments for loaders and
`--explain-partition`.

## Related docs

- Diagnostics: `docs/spec/diagnostics.md` (`E-WEB-*` family)
- Error pages: `docs/reference/errors/E-WEB-*.md`
- `core.web` browser storage/events: `docs/reference/core-library.md`
- Target markers: `docs/spec/syntax-decisions.md` (Web target section)
