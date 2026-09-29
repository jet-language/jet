# #3295 — Matrix and extension-type codec adapters (CORE-F075)

Date: 2026-09-29. Binary: `jet-debug-snapshot14` via `~/.cache/jet-luna/safe-jet.sh`,
source rev `e9c708fa7`. Author: Closer03 (evidence closer).

## Question

How do matrix/tensor values cross Jet's typed codecs today — shape, layout
(row/column-major), scalar type and tag — and how does that compare with Glaze's
Eigen adapter? Is any adapter or dependency choice needed, and is it codec glue or
external-library inclusion?

## Method

- Primary peer source (read, not run): Glaze v8.3.0 `include/glaze/ext/eigen.hpp`
  (downloaded from `raw.githubusercontent.com/stephenberry/glaze/v8.3.0/...`, 462 lines),
  located through `api.github.com/repos/stephenberry/glaze/git/trees/v8.3.0`.
- Jet source read: `Core/compute/compute.jet` (`Tensor` :41-46, `serialize` :647-670,
  `deserialize` :672-738, profiles :73-74, `tensor_profile` :772-775),
  `Core/encoding/cbor.jet:29-37` (profile comment).
- Ran the fixture `Examples/features/serde/tensor_codec.jet` on default run, AOT and the
  interpreter, plus scratch probes `tensor_json.jet` / `tensor_cbor.jet`
  (`~/.cache/jet-test-scratch/Closer03/`).

## Glaze v8.3.0 Eigen adapter (primary source)

| Wire | Shape | Layout | Scalar | Tag |
|---|---|---|---|---|
| JSON, fixed-size matrix (`to<JSON,T>` :327-339) | not written (compile-time) | not written; data in storage order | JSON numbers | none |
| JSON, dynamic matrix (:341-360) | `[[rows, cols], [data...]]` | not written; data in storage order (`value.data()`) | JSON numbers | none |
| BEVE (:66-170) | `[rows, cols]` extents | explicit layout byte `!IsRowMajor` | typed array | BEVE extension tag `tag::extensions | 0b00010'000` |
| CBOR (:172-311) | `[[rows, cols], typed_array]` | RFC 8746 tag 40 (row-major) / 1040 (column-major); read rejects the other tag | RFC 8746 typed array | yes |

Glaze's JSON form therefore loses layout: a column-major Eigen matrix and its
row-major transpose-order are indistinguishable on the wire. Only BEVE and CBOR keep it.
Glaze's adapter is glue compiled against the Eigen headers (`#include <Eigen/Core>`,
`static_assert` otherwise): the codec glue and the external library inclusion are
separate — the glue is useless without the external library.

## Jet today

Source facts: `Tensor` is `{shape: [Int], data: [Float], device, numeric_profile}` (compute.jet:41-46), row-major by construction; it carries no `#Codable`, so `json.decode<compute.Tensor>` is rejected with E2411 ("`compute.Tensor` can't be decoded … Add `#[Codable]`", probe `tensor_json.jet`, `--interpret`), and `json.to_string(tensor)` crashes the compiler (probe `tensor_cbor.jet`, default run and interpreter, exit 101 — defect).

Fixture `Examples/features/serde/tensor_codec.jet`, default `jet run` (stdout before the crash noted below):

```
source: shape=[2, 3] data=[1.0, 2.0, 3.0, 4.0, 5.0, 6.0] profile=F64Strict+Reproducible
element [1, 0]: 4.0
serialize: shape=2,3;data=1.0,2.0,3.0,4.0,5.0,6.0;profile=F64Strict+Reproducible;checksum=0000000000000000
deserialize: shape=[2, 3] data=[1.0, 2.0, 3.0, 4.0, 5.0, 6.0] profile=F64Strict+Reproducible
f32 deserialize: shape=[2, 3] data=[1.5, 2.0, 3.0, 4.0, 5.0, 6.25] profile=F32Strict+Reproducible
json: {"data":[1.0,2.0,3.0,4.0,5.0,6.0],"profile":"F64Strict+Reproducible","shape":[2,3]}
json back: shape=[2, 3] data=[1.0, 2.0, 3.0, 4.0, 5.0, 6.0] profile=F64Strict+Reproducible
json f32 back: shape=[2, 3] data=[1.5, 2.0, 3.0, 4.0, 5.0, 6.25] profile=F32Strict+Reproducible
cbor back: shape=[2, 3] data=[1.0, 2.0, 3.0, 4.0, 5.0, 6.0] profile=F64Strict+Reproducible
cbor tag 40: rejected: CBOR tags are unsupported
```

then `internal compiler error: JIT drop CBORErrorKind enum discriminant is invalid` (exit 101). The interpreter stops with E0956 `core.builtin.float_parse()`. Note the `compute.serialize` checksum prints as `0000000000000000`.

| Path | Shape | Layout | Scalar type | Tag |
|---|---|---|---|---|
| `compute.serialize`/`deserialize` | kept | row-major, implicit | kept via `profile=` (F64/F32 only) | n/a (private text format with checksum) |
| JSON, typed | — (Tensor not Codable: E2411 decode; encode crashes) | — | — | — |
| JSON via explicit `DataTree` | kept (app-written) | row-major, implicit (not marked) | only if the app writes the profile; element values are JSON numbers | none |
| CBOR via explicit `DataTree` | kept (app-written) | row-major, implicit | float64 elements; profile only if written | tags rejected: tag 40 → "CBOR tags are unsupported" |


## Verdict

Investigation criteria 1-4 met (evidence above; defects filed separately). Jet has no canonical codec for `Tensor`: its only self-describing wire form is the private `compute.serialize` text, a parallel serializer outside Encode/Decode (criterion 2 finding). Explicit `DataTree` glue keeps shape and row-major order but, like Glaze's JSON form, does not mark layout, and scalar profile survives only by convention. CBOR RFC 8746 tags (40/1040 multi-dimensional, typed arrays) are rejected, so Jet cannot read Glaze's CBOR Eigen output. Codec glue (a Tensor Encode/Decode derive or RFC 8746 support) is separate from external-library inclusion (Glaze's adapter requires the Eigen headers); no dependency is needed for either. Criterion 5 (passing golden) is not met: the fixture crashes at exit and has no golden.

## Follow-ups (each needs its own ballot or card; nothing ships here)

1. Ballot: derive canonical Encode/Decode for `Tensor` (shape, row-major data, profile) and retire `compute.serialize`/`deserialize`.
2. Ballot: RFC 8746 tag 40/1040 + typed-array support in `core.encoding.cbor` for matrix interchange.
3. Defects: JIT drop crash on `CBORErrorKind`; `json.to_string(Tensor)` compiler crash instead of E2411; interpreter `float_parse`; `compute.serialize` checksum prints all zeros (check whether `wire_checksum` is intended).
