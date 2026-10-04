# #3235 — Glaze v8.3.0 capability denominator: body and export sources per TOC family

Date: 2026-09-29. Card #3235 (CORE-F006). Research evidence only; it approves no API, default or dependency.

## Question

Does every family in the selected Glaze scope (the v8.3.0 `mkdocs.yml` navigation, CORE-C022) have an exact body source and export source, or a bounded unresolved record? Is the rendered-main identity kept apart from the tag identity?

## Method (commands actually run)

```
git ls-remote https://github.com/stephenberry/glaze refs/tags/v8.3.0 refs/heads/main
git clone --depth 1 --branch v8.3.0 https://github.com/stephenberry/glaze glaze-v8.3.0
git clone --depth 1 https://github.com/stephenberry/glaze glaze-main
git clone --depth 1 --branch v9.0.0 https://github.com/stephenberry/glaze glaze-v9.0.0
python3 glaze_rows.py glaze-v8.3.0 glaze-main glaze_rows.json
```

`glaze_rows.py` (kept at `~/.cache/jet-dev/scratch/Closer06/keep/glaze_rows.py`) reads the `nav:` block of the tag's `mkdocs.yml` (lines 66–150). For each page it records:

- the page body's SHA-256 at the tag;
- whether the same path on `main` is byte-identical;
- every `#include "glaze/…"` in the body, and whether that header exists at the tag;
- the first four `glz::` names the body uses, each resolved to a namespace-scope declaration under `include/` at the tag.

The resolver ranks type, concept, alias, enum, namespace and variable definitions ahead of function declarators. It ranks the exception wrapper (`glz::ex`), `ext/` and `api/std/` last. For 13 names the mechanical pick was a member function or a same-named wire constant, for example `glz::read` resolving to the virtual `api::read`, or `glz::object` resolving to the BEVE tag constant. Each of those was corrected by hand to the free declaration, and the table shows the corrected line.

Spot-checked against the tag: `glz::write_toml` → `toml/write.hpp:1465` reads `[[nodiscard]] error_ctx write_toml(T&& value, Buffer&& buffer)`; `glz::http_default_max_body_size` → `net/http.hpp:24` reads `inline constexpr size_t http_default_max_body_size = 100 * 1024 * 1024;`; `glz::json_t` → `json/generic_fwd.hpp:565` reads `using json_t [[deprecated("glz::json_t is deprecated, use glz::generic instead")]] = generic;`.

## Identities (criterion 4)

| identity | value | how observed |
|---|---|---|
| selected tag | `v8.3.0` = commit `dce5e0f7ec572725ec369a0bc59a00eeb2270a9a` (lightweight tag; `git cat-file -t` → `commit`), committed 2026-08-29T18:27:26-05:00 "version 8.3.0 bump" | `git ls-remote`, `git log -1` |
| tag `mkdocs.yml` | sha256 `8909a06a5ffcb60be02af7f453c0ecd6b07d41fac678f0d229fa4fdd4e447bda`, 74 nav pages | file hash |
| rendered docs source | branch `main` = `ccbbd884750931af0945cc81cdc11e31eac7936b` (2026-09-28 "Bound HTTP request headers and first-request wait (#2975)") | `git ls-remote`, `git log -1` |
| main vs tag docs | 52 of 74 pages byte-identical, 22 changed, 0 missing. The nav lists are identical: no page was added or removed on main or in v9.0.0 | `glaze_rows.py`, nav diff |
| newer releases since the capture | `v8.4.0` (2026-09-15) and `v9.0.0` = `d78832c82289c61a9315bfbc35332cec9f4e93ca` (2026-09-24). The 2026-09-11 capture's "current 8.3.0" is no longer current; the delta is routed in #3247's companion | GitHub releases API |

Rendered pages on the docs site come from `main`. A body quoted from the rendered site therefore describes `ccbbd884` and later, not `v8.3.0`, for the 22 pages marked `changed` below. The tag bodies (sha column) are the selected-scope evidence.

## Row table: one row per selected TOC family (criteria 1, 2, 5)

A **body source** is the page at the tag with its hash. An **export source** is the declaration line of each named API at the tag. `exact` means every include exists and every name resolved. `bounded-unresolved` names what is missing. Each row stands alone: JSON rows are not counted for BEVE, CBOR, TOML or any other format (criterion 2), and no linked card substitutes for a locator (criterion 5).

| # | TOC page (docs/ at tag) | body sha256[:16] @v8.3.0 | rendered main `ccbbd884` | headers the body #includes | first `glz::` names → declaration at tag (file:line) | record |
|---|---|---|---|---|---|---|
| 1 | `index.md` | `a8b23564bd3a2ad4` | same | — | `write_json`→`glaze/json/write.hpp:2827`; `read_json`→`glaze/json/read.hpp:5265` | exact |
| 2 | `installation.md` | `be2f470c61cc23a2` | same | `glaze/glaze.hpp` | `simd_info`→`glaze/simd/backends.hpp:134` | exact |
| 3 | `quick-start.md` | `d4dabcee7880ca9c` | same | `glaze/glaze.hpp` | `write_json`→`glaze/json/write.hpp:2827`; `read_json`→`glaze/json/read.hpp:5265`; `format_error`→`glaze/core/reflect.hpp:3333`; `meta`→`glaze/forward.hpp:71` | exact |
| 4 | `options.md` | `c6c31e968bd0c918` | changed | — | `opts`→`glaze/core/opts.hpp:69`; `opts_csv`→`glaze/core/opts.hpp:93`; `meta`→`glaze/forward.hpp:71`; `read`→`glaze/core/read.hpp:218` | exact |
| 5 | `optimization-levels.md` | `e2d9fa14e2ec418b` | same | `glaze/glaze.hpp` | `write_json`→`glaze/json/write.hpp:2827`; `write`→`glaze/core/write.hpp:17`; `opts_size`→`glaze/core/opts.hpp:1606`; `opts`→`glaze/core/opts.hpp:69` | exact |
| 6 | `optimizing-performance.md` | `968d0778b17322f5` | changed | — | `simd_info`→`glaze/simd/backends.hpp:134`; `write_json`→`glaze/json/write.hpp:2827`; `read`→`glaze/core/read.hpp:218`; `opts_size`→`glaze/core/opts.hpp:1606` | exact |
| 7 | `security.md` | `031e388a9de7740a` | same | — | `read_beve`→`glaze/beve/read.hpp:3191`; `error_code`→`glaze/core/context.hpp:17`; `opts`→`glaze/core/opts.hpp:69`; `BEVE`→`glaze/forward.hpp:31` | exact |
| 8 | `rename-keys.md` | `2419aa4dd2048cb7` | same | — | `meta`→`glaze/forward.hpp:71`; `write_json`→`glaze/json/write.hpp:2827`; `read_json`→`glaze/json/read.hpp:5265`; `camel_case`→`glaze/util/key_transformers.hpp:252` | exact |
| 9 | `skip-keys.md` | `9562a5970e98542b` | changed | — | `meta`→`glaze/forward.hpp:71`; `write_json`→`glaze/json/write.hpp:2827`; `operation`→`glaze/core/meta.hpp:26`; `read_json`→`glaze/json/read.hpp:5265` | exact |
| 10 | `generic-json.md` | `b6b62a68bb4761d6` | changed | `glaze/json/generic_fwd.hpp`; `glaze/json/generic.hpp` | `generic`→`glaze/json/generic_fwd.hpp:551`; `json_t`→`glaze/json/generic_fwd.hpp:565`; `generic_sorted`→`glaze/json/generic_fwd.hpp:560`; `generic_sorted_i64`→`glaze/json/generic_fwd.hpp:561` | exact |
| 11 | `ordered-maps.md` | `c6531c41f66abd83` | same | `glaze/containers/ordered_small_map.hpp`; `glaze/containers/ordered_map.hpp` | `ordered_small_map`→`glaze/containers/ordered_small_map.hpp:47`; `generic`→`glaze/json/generic_fwd.hpp:551`; `ordered_map`→`glaze/containers/ordered_map.hpp:54`; `has_optional_ref`→`glaze/core/feature_test.hpp:51` | exact |
| 12 | `pure-reflection.md` | `9c4fde787ee969e7` | same | — | `meta`→`glaze/forward.hpp:71`; `reflectable`→`glaze/core/common.hpp:570`; `has_reflect`→`glaze/core/reflect.hpp:3442`; `as_array`→`glaze/core/common.hpp:704` | exact |
| 13 | `modify-reflection.md` | `c56fe8d98a028a1f` | same | — | `meta`→`glaze/forward.hpp:71`; `object`→`glaze/core/common.hpp:724`; `reflectable`→`glaze/core/common.hpp:570` | exact |
| 14 | `json.md` | `da8c7673651002e3` | changed | `glaze/glaze.hpp`; `glaze/json/flatten_map.hpp` | `write_json`→`glaze/json/write.hpp:2827`; `read_json`→`glaze/json/read.hpp:5265`; `error_ctx`→`glaze/core/context.hpp:129`; `read_file_json`→`glaze/json/read.hpp:5325` | exact |
| 15 | `binary.md` | `3f6659d1f5cea563` | changed | `glaze/beve.hpp` | `write_beve`→`glaze/beve/write.hpp:1837`; `read_beve`→`glaze/beve/read.hpp:3191`; `beve_to_json`→`glaze/beve/beve_to_json.hpp:743`; `beve_size`→`glaze/beve/size.hpp:1012` | exact |
| 16 | `bson.md` | `6c948461ed02fefc` | same | `glaze/bson.hpp`; `glaze/util/uuid.hpp` | `meta`→`glaze/forward.hpp:71`; `bson`→`glaze/bson/header.hpp:30`; `uuid`→`glaze/util/uuid.hpp:24`; `write_bson`→`glaze/bson/write.hpp:882` | exact |
| 17 | `cbor.md` | `9284e4afb11e2b94` | changed | `glaze/cbor.hpp`; `glaze/cbor/cbor_exceptions.hpp` **(absent at tag)**; `glaze/ext/eigen.hpp` | `write_cbor`→`glaze/cbor/write.hpp:1204`; `read_cbor`→`glaze/cbor/read.hpp:2634`; `ex`→`glaze/exceptions/json_exceptions.hpp:12`; `cbor_to_json`→`glaze/cbor/cbor_to_json.hpp:751` | bounded-unresolved: page includes `glaze/cbor/cbor_exceptions.hpp`, absent at the tag (header ships as `glaze/exceptions/cbor_exceptions.hpp`) |
| 18 | `jsonb.md` | `4f407ffe54e13ec8` | same | `glaze/jsonb.hpp`; `glaze/exceptions/jsonb_exceptions.hpp` | `write_jsonb`→`glaze/jsonb/write.hpp:759`; `read_jsonb`→`glaze/jsonb/read.hpp:1441`; `jsonb_to_json`→`glaze/jsonb/jsonb_to_json.hpp:473`; `meta`→`glaze/forward.hpp:71` | exact |
| 19 | `csv.md` | `b543626185c5449c` | same | — | `read_csv`→`glaze/csv/read.hpp:1543`; `colwise`→`glaze/core/opts.hpp:20`; `write_csv`→`glaze/csv/write.hpp:760`; `write`→`glaze/core/write.hpp:17` | exact |
| 20 | `msgpack.md` | `c7fb145e54343f40` | changed | `glaze/msgpack.hpp` | `meta`→`glaze/forward.hpp:71`; `write_msgpack`→`glaze/msgpack/write.hpp:1156`; `format_error`→`glaze/core/reflect.hpp:3333`; `read_msgpack`→`glaze/msgpack/read.hpp:1320` | exact |
| 21 | `stencil-mustache.md` | `2747c10709f377a8` | same | `glaze/stencil/stencilcount.hpp` | `stencil`→`glaze/stencil/stencil.hpp:44`; `error_ctx`→`glaze/core/context.hpp:129`; `write_json`→`glaze/json/write.hpp:2827`; `mustache`→`glaze/stencil/stencil.hpp:394` | exact |
| 22 | `toml.md` | `0b69654bebb1d315` | changed | `glaze/toml.hpp` | `meta`→`glaze/forward.hpp:71`; `write_toml`→`glaze/toml/write.hpp:1465`; `format_error`→`glaze/core/reflect.hpp:3333`; `read_toml`→`glaze/toml/read.hpp:3339` | exact |
| 23 | `yaml.md` | `5fcef12292ecd54f` | changed | `glaze/yaml.hpp` | `meta`→`glaze/forward.hpp:71`; `write_yaml`→`glaze/yaml/write.hpp:1817`; `format_error`→`glaze/core/reflect.hpp:3333`; `read_yaml`→`glaze/yaml/read.hpp:6605` | exact |
| 24 | `EETF/erlang-external-term-format.md` | `7273e097e140cd17` | same | — | `eetf_to_json`→`glaze/eetf/eetf_to_json.hpp:324`; `eetf`→`glaze/eetf/opts.hpp:7` | exact |
| 25 | `enum-reflection.md` | `960b8c1d856b9613` | changed | `glaze/glaze.hpp` | `meta`→`glaze/forward.hpp:71`; `enumerate`→`glaze/core/common.hpp:729`; `write_json`→`glaze/json/write.hpp:2827`; `read_json`→`glaze/json/read.hpp:5265` | exact |
| 26 | `exceptions.md` | `9721dca3c6fb246c` | same | — | `ex`→`glaze/exceptions/json_exceptions.hpp:12` | exact |
| 27 | `wrappers.md` | `af0606b87589c502` | changed | `glaze/chrono.hpp`; `glaze/json/flatten_map.hpp` | `append_arrays`→`glaze/core/wrappers.hpp:71`; `bools_as_numbers`→`glaze/core/wrappers.hpp:75`; `cast`→`glaze/core/cast.hpp:153`; `quoted_num`→`glaze/core/wrappers.hpp:79` | exact |
| 28 | `custom-wrappers.md` | `b97c1058e4057f9e` | same | — | `meta`→`glaze/forward.hpp:71`; `write_json`→`glaze/json/write.hpp:2827`; `read_json`→`glaze/json/read.hpp:5265`; `to`→`glaze/forward.hpp:77` | exact |
| 29 | `custom-serialization.md` | `1adb52e560203003` | same | — | `custom`→`glaze/core/wrappers.hpp:105`; `meta`→`glaze/forward.hpp:71`; `write_json`→`glaze/json/write.hpp:2827`; `read_json`→`glaze/json/read.hpp:5265` | exact |
| 30 | `chrono.md` | `47adc6021a48a747` | changed | `glaze/glaze.hpp`; `glaze/chrono.hpp` | `write_json`→`glaze/json/write.hpp:2827`; `read_json`→`glaze/json/read.hpp:5265`; `write_beve`→`glaze/beve/write.hpp:1837`; `read_beve`→`glaze/beve/read.hpp:3191` | exact |
| 31 | `variant-handling.md` | `116ae30756e7fac0` | changed | `glaze/glaze.hpp` | `write_json`→`glaze/json/write.hpp:2827`; `read_json`→`glaze/json/read.hpp:5265`; `meta`→`glaze/forward.hpp:71`; `custom`→`glaze/core/wrappers.hpp:105` | exact |
| 32 | `unknown-keys.md` | `b888cb746488804a` | same | — | `merge`→`glaze/core/common.hpp:102`; `sv`→`glaze/util/string_literal.hpp:11`; `raw_json`→`glaze/core/common.hpp:191`; `meta`→`glaze/forward.hpp:71` | exact |
| 33 | `json-pointer-syntax.md` | `27f8d88e250cc3e9` | same | — | `get`→`glaze/core/seek.hpp:265`; `set`→`glaze/core/seek.hpp:340`; `read_as_json`→`glaze/json/ptr.hpp:11`; `get_as_json`→`glaze/json/json_ptr.hpp:208` | exact |
| 34 | `struct-json-pointer.md` | `f96d0f79a1195cd3` | same | — | `generic`→`glaze/json/generic_fwd.hpp:551`; `get`→`glaze/core/seek.hpp:265`; `set`→`glaze/core/seek.hpp:340`; `seek`→`glaze/core/seek.hpp:71` | exact |
| 35 | `json-patch.md` | `7a44601e43d7e377` | same | `glaze/json/patch.hpp` | `generic`→`glaze/json/generic_fwd.hpp:551`; `patch`→`glaze/json/patch.hpp:721`; `read_json`→`glaze/json/read.hpp:5265`; `patch_document`→`glaze/json/patch.hpp:51` | exact |
| 36 | `json-merge-patch.md` | `893100ea916f0445` | same | `glaze/json/patch.hpp` | `generic`→`glaze/json/generic_fwd.hpp:551`; `merge_patch`→`glaze/json/patch.hpp:919`; `read_json`→`glaze/json/read.hpp:5265`; `merge_diff`→`glaze/json/patch.hpp:1002` | exact |
| 37 | `JMESPath.md` | `3a13dbcc8cd2cf34` | same | `glaze/json/jmespath.hpp` | `write_json`→`glaze/json/write.hpp:2827`; `read_jmespath`→`glaze/json/jmespath.hpp:818`; `format_error`→`glaze/core/reflect.hpp:3333`; `jmespath_expression`→`glaze/json/jmespath.hpp:971` | exact |
| 38 | `json-schema.md` | `9d07334954988b51` | same | — | `write_json_schema`→`glaze/json/schema.hpp:1372`; `json_schema`→`glaze/core/meta.hpp:50`; `schema`→`glaze/json/schema.hpp:157`; `opts`→`glaze/core/opts.hpp:69` | exact |
| 39 | `field-validation.md` | `550c30877171dc67` | same | — | `read_json`→`glaze/json/read.hpp:5265`; `read`→`glaze/core/read.hpp:218`; `opts`→`glaze/core/opts.hpp:69`; `meta`→`glaze/forward.hpp:71` | exact |
| 40 | `partial-read.md` | `e723e5ab0415846f` | same | — | `opts`→`glaze/core/opts.hpp:69`; `read`→`glaze/core/read.hpp:218`; `error_code`→`glaze/core/context.hpp:17` | exact |
| 41 | `partial-write.md` | `6205670c2cce8462` | same | — | `json_ptrs`→`glaze/core/seek.hpp:535`; `object`→`glaze/core/common.hpp:724`; `write_json`→`glaze/json/write.hpp:2827`; `write_json_partial`→`glaze/json/write.hpp:2863` | exact |
| 42 | `reading.md` | `13debbb33804694d` | same | — | `error_ctx`→`glaze/core/context.hpp:129`; `read_json`→`glaze/json/read.hpp:5265`; `format_error`→`glaze/core/reflect.hpp:3333`; `error_code`→`glaze/core/context.hpp:17` | exact |
| 43 | `lazy-json.md` | `b2c2ccbe8e360383` | same | `glaze/json.hpp` | `lazy_json`→`glaze/json/lazy.hpp:549`; `read_json`→`glaze/json/read.hpp:5265`; `generic`→`glaze/json/generic_fwd.hpp:551`; `validate_json`→`glaze/json/read.hpp:5248` | exact |
| 44 | `writing.md` | `58d77a9802736f13` | changed | — | `error_ctx`→`glaze/core/context.hpp:129`; `write_json`→`glaze/json/write.hpp:2827`; `format_error`→`glaze/core/reflect.hpp:3333`; `error_code`→`glaze/core/context.hpp:17` | exact |
| 45 | `streaming.md` | `6ade58cc6c589c56` | changed | `glaze/core/ostream_buffer.hpp`; `glaze/core/istream_buffer.hpp`; `glaze/json/json_stream.hpp` | `basic_ostream_buffer`→`glaze/core/ostream_buffer.hpp:44`; `write_json`→`glaze/json/write.hpp:2827`; `ostream_buffer`→`glaze/core/ostream_buffer.hpp:192`; `basic_istream_buffer`→`glaze/core/istream_buffer.hpp:56` | exact |
| 46 | `max-float-precision.md` | `279fe9ad1f3d84e6` | same | — | `write_float32`→`glaze/json/max_write_precision.hpp:115`; `write_float64`→`glaze/json/max_write_precision.hpp:118`; `write_float_full`→`glaze/json/max_write_precision.hpp:121`; `float_format`→`glaze/json/float_format.hpp:135` | exact |
| 47 | `ranges.md` | `fa5b36d09730708d` | same | — | `write_json`→`glaze/json/write.hpp:2827`; `sv`→`glaze/util/string_literal.hpp:11`; `array`→`glaze/core/common.hpp:721` | exact |
| 48 | `networking/http-rest-support.md` | `7a67bfd15aea7586` | same | `glaze/net/http_server.hpp`; `glaze/rpc/registry.hpp`; `glaze/net/http_client.hpp` | `http_server`→`glaze/net/http_server.hpp:1416`; `request`→`glaze/net/http_router.hpp:45`; `response`→`glaze/net/http_router.hpp:69`; `meta`→`glaze/forward.hpp:71` | exact |
| 49 | `networking/http-server.md` | `cf7414b0dd003cf1` | changed | `glaze/net/http_server.hpp` | `http_server`→`glaze/net/http_server.hpp:1416`; `request`→`glaze/net/http_router.hpp:45`; `response`→`glaze/net/http_router.hpp:69`; `https_server`→`glaze/net/http_server.hpp:3383` | exact |
| 50 | `networking/http-router.md` | `3a521f5fe83a0874` | changed | — | `http_router`→`glaze/net/http_router.hpp:1190`; `http_method`→`glaze/net/http.hpp:26`; `request`→`glaze/net/http_router.hpp:45`; `response`→`glaze/net/http_router.hpp:69` | exact |
| 51 | `networking/url.md` | `2b1fe271e3d1ac48` | same | `glaze/net/url.hpp` | `url_decode`→`glaze/net/url.hpp:38`; `url_encode`→`glaze/net/url.hpp:87`; `parse_urlencoded`→`glaze/net/url.hpp:139`; `split_target`→`glaze/net/url.hpp:237` | exact |
| 52 | `networking/rest-registry.md` | `98b28a64913ecb0d` | same | `glaze/rpc/registry.hpp`; `glaze/net/http_server.hpp` | `meta`→`glaze/forward.hpp:71`; `object`→`glaze/core/common.hpp:724`; `http_server`→`glaze/net/http_server.hpp:1416`; `registry`→`glaze/rpc/registry.hpp:141` | exact |
| 53 | `networking/tls-support.md` | `32163a9a2cd5f771` | changed | `glaze/net/http_server.hpp` | `https_server`→`glaze/net/http_server.hpp:3383`; `request`→`glaze/net/http_router.hpp:45`; `response`→`glaze/net/http_router.hpp:69`; `http_server`→`glaze/net/http_server.hpp:1416` | exact |
| 54 | `networking/http-client.md` | `d159e3a104509138` | changed | `glaze/net/http_client.hpp`; `glaze/glaze.hpp` | `http_client`→`glaze/net/http_client.hpp:993`; `http_headers`→`glaze/net/http_headers.hpp:54`; `http_status_category`→`glaze/net/http.hpp:183`; `http_status_from`→`glaze/net/http.hpp:191` | exact |
| 55 | `networking/websocket-client.md` | `fbf6121f6699bfa5` | same | `glaze/net/websocket_client.hpp`; `glaze/glaze.hpp` | `websocket_client`→`glaze/net/websocket_client.hpp:26`; `ws_opcode`→`glaze/net/websocket_connection.hpp:42`; `ws_close_code`→`glaze/net/websocket_connection.hpp:45`; `meta`→`glaze/forward.hpp:71` | exact |
| 56 | `networking/body-size-limits.md` | `54d0700c236514cc` | changed | — | `http_server`→`glaze/net/http_server.hpp:1416`; `http_default_max_body_size`→`glaze/net/http.hpp:24`; `http_client_error`→`glaze/net/http_client.hpp:75`; `http_client`→`glaze/net/http_client.hpp:993` | exact |
| 57 | `networking/advanced-networking.md` | `8448718df3e3eb9a` | same | `glaze/net/http_server.hpp`; `glaze/net/websocket_connection.hpp`; `glaze/net/http_client.hpp` | `http_server`→`glaze/net/http_server.hpp:1416`; `cors_config`→`glaze/net/cors.hpp:17`; `create_cors_middleware`→`glaze/net/cors.hpp:140`; `request`→`glaze/net/http_router.hpp:45` | exact |
| 58 | `networking/http-examples.md` | `e116e8324263b8d7` | same | `glaze/net/http_server.hpp`; `glaze/glaze.hpp`; `glaze/rpc/registry.hpp`; `glaze/net/websocket_connection.hpp` | `http_server`→`glaze/net/http_server.hpp:1416`; `request`→`glaze/net/http_router.hpp:45`; `response`→`glaze/net/http_router.hpp:69`; `read_json`→`glaze/json/read.hpp:5265` | exact |
| 59 | `rpc/repe-rpc.md` | `214605e8753e311d` | same | — | `asio_server`→`glaze/ext/glaze_asio.hpp:620`; `asio_client`→`glaze/ext/glaze_asio.hpp:355`; `repe`→`glaze/rpc/repe/repe.hpp:32`; `async_string`→`glaze/thread/async_string.hpp:20` | exact |
| 60 | `rpc/repe-buffer.md` | `3186ee5cce1b837f` | same | `glaze/rpc/repe/buffer.hpp`; `glaze/rpc/repe/repe.hpp` | `repe`→`glaze/rpc/repe/repe.hpp:32`; `error_code`→`glaze/core/context.hpp:17`; `write`→`glaze/core/write.hpp:17`; `registry`→`glaze/rpc/registry.hpp:141` | exact |
| 61 | `rpc/repe-plugin.md` | `dc4c7c70f95b20bf` | same | `glaze/rpc/repe/plugin.h`; `glaze/rpc/repe/plugin_helper.hpp`; `glaze/ext/glaze_asio.hpp` | `registry`→`glaze/rpc/registry.hpp:141`; `asio_server`→`glaze/ext/glaze_asio.hpp:620`; `repe`→`glaze/rpc/repe/repe.hpp:32`; `meta`→`glaze/forward.hpp:71` | exact |
| 62 | `rpc/jsonrpc-registry.md` | `038dbbd6f4e8d267` | same | `glaze/rpc/registry.hpp` | `registry`→`glaze/rpc/registry.hpp:141`; `meta`→`glaze/forward.hpp:71`; `object`→`glaze/core/common.hpp:724`; `opts`→`glaze/core/opts.hpp:69` | exact |
| 63 | `rpc/json-rpc.md` | `29d0f56c41dada4d` | same | — | `rpc`→`glaze/ext/jsonrpc.hpp:12`; `expected`→`glaze/util/expected.hpp:14`; `unexpected`→`glaze/util/expected.hpp:22`; `write_json`→`glaze/json/write.hpp:2827` | exact |
| 64 | `rpc/repe-jsonrpc-conversion.md` | `5331ae1b9ae2b85a` | same | `glaze/rpc/repe/repe_to_jsonrpc.hpp` | `error_code`→`glaze/core/context.hpp:17` | exact |
| 65 | `json-include.md` | `8370b1e6c1d056c7` | changed | `glaze/file/hostname_include.hpp` | `file_include`→`glaze/core/common.hpp:151`; `meta`→`glaze/forward.hpp:71`; `read_json`→`glaze/json/read.hpp:5265`; `hostname_include`→`glaze/file/hostname_include.hpp:37` | exact |
| 66 | `cli-menu.md` | `ea0158df24d2f15a` | same | — | `meta`→`glaze/forward.hpp:71`; `make_reflectable`→`glaze/core/reflect.hpp:1465`; `raw_json`→`glaze/core/common.hpp:191`; `run_cli_menu`→`glaze/ext/cli_menu.hpp:61` | exact |
| 67 | `glaze-interfaces.md` | `bf70e733299c54ea` | same | `glaze/api/impl.hpp`; `glaze/api/lib.hpp` | `hash_t`→`glaze/api/trait.hpp:138`; `iface`→`glaze/api/api.hpp:65`; `api`→`glaze/api/api.hpp:26`; `JSON`→`glaze/forward.hpp:35` | exact |
| 68 | `building-shared-libraries.md` | `0e74e22aa009275f` | same | `glaze/api/impl.hpp`; `glaze/api/lib.hpp` | `meta`→`glaze/forward.hpp:71`; `object`→`glaze/core/common.hpp:724`; `version_t`→`glaze/version.hpp:16`; `iface_fn`→`glaze/api/api.hpp:142` | exact |
| 69 | `advanced-api-usage.md` | `3243886e3c1492b4` | same | `glaze/api/api.hpp` | `impl`→`glaze/api/impl.hpp:30`; `api`→`glaze/api/api.hpp:26`; `hash_t`→`glaze/api/trait.hpp:138`; `hash`→`glaze/api/api.hpp:70` | exact |
| 70 | `time-trace.md` | `b6008981009553f5` | same | `glaze/trace/trace.hpp` | `trace`→`glaze/trace/trace.hpp:59`; `write_file_json`→`glaze/json/write.hpp:2977` | exact |
| 71 | `recorder.md` | `ed07dbe9e5667036` | same | `glaze/record/recorder.hpp` | `recorder`→`glaze/record/recorder.hpp:39`; `write_file_json`→`glaze/json/write.hpp:2977` | exact |
| 72 | `thread-pool.md` | `c21a5c6cc560ef82` | same | — | `pool`→`glaze/thread/threadpool.hpp:18` | exact |
| 73 | `volatile-support.md` | `14d4a65b9f83df2f` | same | — | `volatile_array`→`glaze/hardware/volatile_array.hpp:18`; `write_json`→`glaze/json/write.hpp:2827`; `read_json`→`glaze/json/read.hpp:5265` | exact |
| 74 | `FAQ.md` | `99b6f4f1c503f618` | same | — | `read`→`glaze/core/read.hpp:218`; `opts`→`glaze/core/opts.hpp:69`; `meta`→`glaze/forward.hpp:71` | exact |

Summary: 74 of 74 rows carry an exact tag body source. 73 are `exact` for their named export locators. One row (`cbor.md`) is bounded-unresolved: the page includes `glaze/cbor/cbor_exceptions.hpp`, which does not exist at the tag. The exception wrapper ships as `include/glaze/exceptions/cbor_exceptions.hpp` (it is included from `glaze_exceptions.hpp:11`). This is an upstream documentation defect, not a Jet gap.

Proof limit: a row names the page's first four `glz::` APIs and the headers the page includes. It is not a complete symbol-level export listing of every header in the family, which remains **unknown** for every row. Nothing was compiled or executed.

## Reconciliation with the retained companion

The CORE-C022 companion `peer-evidence/west/captured/glaze-v8.3-current-index.md` is retained in `Docs/research/mine-for-jet-2026-09-12.md` as 74 capability rows (lines 35787–35860, `capability-corelib-glaze-*`). Each of those rows reads "Tag-pinned navigation; body is tag-pinned or current-main only where its capture says so. Unread …". The 74 page names above match those 74 rows one for one. This table replaces "Unread" with a tag body hash and export locators, and it records which bodies differ on main.

## Routing of useful uncovered capabilities (criterion 3)

Existing Jet owners are cited by file only; that establishes an owner, not parity.

| Glaze family (rows) | Jet owner or work |
|---|---|
| JSON read/write, options, optimization, keys, generic, ordered maps, reading, writing, ranges (4–6, 8–11, 14, 42, 44, 47) | Core/encoding/json.jet (existing). CORE-F029 #3255 (data-model losslessness) |
| pure/modify reflection, enum strings, exceptions, wrappers, custom wrappers/serialization (12–13, 25–29) | #3236 (custom codec and reflection interoperability) |
| BEVE (15) | #3269 |
| BSON (16) | #3267 |
| CBOR (17) | #3271 |
| JSONB (18) | #3268 |
| CSV (19) | Core/encoding/csv.jet (existing; no card claims Glaze CSV parity) |
| MessagePack (20) | #3312 |
| Stencil/Mustache (21) | #3252 |
| TOML (22) | #3272 |
| YAML (23) | #3273 |
| EETF (24) | #3270 |
| Chrono (30) | #3265 |
| Variant handling, max float precision (31, 46) | #3255 |
| Unknown keys (32) | #3077 criterion 3 (unknown/duplicate member policy) and #3259 |
| JSON Pointer, struct pointer, partial read/write (33–34, 40–41) | #3237 |
| JSON Patch / Merge Patch (35–36) | **Main ballot list**: no card found for RFC 6902/7386 document patching |
| JMESPath (37) | **Main ballot list**: no card found for a query language over the data model |
| JSON Schema (38) | #3238 |
| Field validation (39) | #3259 |
| Lazy JSON, streaming I/O (43, 45) | #3274 |
| Networking rows (48–58) | Core/http + Core/net (existing); body limits: Examples/features/net/http_server_limits.jet |
| RPC rows (59–64) | #3254 |
| JSON include (65) | #3251. Note that v9.0.0 removed `glz::hostname_include` (#2924) |
| CLI menu (66) | **Main ballot list**: no card found |
| Glaze interfaces, shared libraries, advanced API (67–69) | #3253 |
| Time trace, recorder (70–71) | #3250 |
| Thread pool (72) | Core/tasks (existing) |
| Volatile support (73) | #3244 |
| index, installation, quick-start, security, FAQ (1–3, 7, 74) | documentation only; no capability to route |

## Verdict

- Criteria 1, 2, 4 and 5: met by the table and the identity rows above, within the stated proof limit (full per-header export listings are recorded as unknown, not claimed).
- Criterion 3: met. Every family routes to an existing owner or card, or appears on Main's ballot list (JSON Patch/Merge Patch, JMESPath, CLI menu).

This file is the durable record. It was not appended into the 12 MB report under the CORE-F006 anchor, to avoid a concurrent edit collision. Pip can link this file from that anchor.
