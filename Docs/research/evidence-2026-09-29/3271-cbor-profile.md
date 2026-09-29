# 3271 — CBOR semantic profile: accepted/rejected table on the current binary

Date: 2026-09-29. Closer02. Binary: `~/.cache/jet-test-scratch/jet-current` → `jet-debug-snapshot14`, run through `~/.cache/jet-luna/safe-jet.sh`.

## Question

What does `core.encoding.cbor` actually accept and reject today (map keys, tags, simple values, typed arrays, numeric bounds, floats, indefinite lengths), does it honour the ratified `CBOROptions` contract (Docs/spec/encoding-decisions.md:887-903, D-ENC-CBOR-SURFACE1=A), and does the result hold on every tier?

## Method

- Read `crates/jet-codegen/src/Prelude/Core.jet:86-87`: `core.encoding.cbor` is a host module; the Jet source module owns only `reader` and `writer`. `parse`, `decode`, `to_bytes` and `to_bytes_canonical` dispatch to the host kernel (`crates/jet-codegen/src/Prelude/CoreLib/Top/EncodingCodecs.rs:469-600`, `jet_cbor_kernel`). `Core/encoding/cbor.jet`'s own `parse`/`decode_cbor` are dead: the build lints `decode_cbor` as an unused private function (L0104), so its header comment (lines 29-37) and its hard-wired `safe_limits()` no longer describe shipped behaviour.
- Scratch witnesses in `~/.cache/jet-test-scratch/Closer02/`: `cbor_probe2.jet` (cells), `cbor_opts.jet` (options), `cbor_err_min.jet`, `cbor_nan.jet`, `cbor_typed.jet`, `cbor_probe.jet` (tier/AOT attempt).
- Each witness was run with `safe-jet.sh run --interpret`, `safe-jet.sh run` (JIT) and `safe-jet.sh build` (AOT).

## Evidence: cell table (`safe-jet.sh run --interpret cbor_probe2.jet`, observed)

| Input (hex) | Cell | Observed |
|---|---|---|
| `00`, `18 18`, `20` | uint 0, uint 24, nint -1 | accepted `0`, `24`, `-1` |
| `18 01` | non-shortest uint 1 | accepted `1` (noncanonical accepted when `require_canonical: false`) |
| `1b 7f ff ff ff ff ff ff ff` | uint 2^63-1 | accepted `9223372036854775807` |
| `1b 80 00 00 00 00 00 00 00` | uint 2^63 | rejected @0 `CBOR integer is outside Jet Int` |
| `1b ff…ff` | uint 2^64-1 | rejected @0 `CBOR integer is outside Jet Int` |
| `3b 7f ff…ff` | nint -2^63 | accepted `-9223372036854775808` |
| `3b ff…ff` | nint -2^64 | rejected @0 `CBOR integer is outside Jet Int` |
| `63 6a 65 74` | text | accepted `"jet"` |
| `62 ff fe` | text, invalid UTF-8 | rejected @0 `CBOR text is not UTF-8` |
| `82 01 02`, `a1 61 61 01`, `83 f4 f5 f6` | array, text-key map, false/true/null | accepted `[1,2]`, `{"a":1}`, `[false,true,null]` |
| `f7` | undefined | rejected @0 `unsupported CBOR simple value 23` |
| `f0`, `f8 ff` | simple(16), simple(255) | rejected @0 `unsupported CBOR simple value 16` / `… 24` (reason names the additional-info 24, not the simple value 255) |
| `fb 3f f8 00…` | float64 1.5 | accepted `1.5` |
| `f9 3c 00`, `fa 3f 80 00 00` | float16 1.0, float32 1.0 | **accepted** `1.0` (cbor.jet header claims these are Unsupported; the spec's "every otherwise-supported RFC 8949 encoding" admits them) |
| `fb 7f f8 00…` | float64 NaN | **ICE on every tier**: `internal compiler error: JSON cannot encode a non-finite Float` (defect D4) |
| `c1 1a …`, `d8 1c 01`, `d8 1d 00`, `c2 41 01` | tag 1 datetime, tag 28, tag 29, tag 2 bignum | rejected @0 `CBOR tags are unsupported` |
| `d8 40 42 01 02`, `d8 46 44 …` | RFC 8746 typed arrays (tag 64 u8, tag 70 u32le) | rejected @0 `CBOR tags are unsupported` |
| `9f 01 02 ff`, `7f 61 61 ff` | indefinite array / text | **accepted** `[1,2]`, `"a"` (cbor.jet header claims Unsupported; spec :897-901 defines depth/item rules for indefinite items, so acceptance is the ratified reading) |
| `42 01 02` | bstr into `DataTree` | rejected @0 `CBOR byte strings are outside core.encoding.Data; use decode<[U8]>` |
| `a1 01 02`, `a1 80 02` | int key, array key | rejected @1 `CBOR map key must be text` |
| `a2 61 61 01 61 61 02` | duplicate key | rejected @4 `duplicate CBOR text map key` |
| `a2 61 62 01 61 61 02` | unsorted keys | accepted `{"b":1,"a":2}` (order preserved) |
| `1c` | reserved additional info 28 | rejected @0 `indefinite/reserved CBOR length is unsupported by whole-value decoding` |
| `ff` | lone break | rejected @0 `CBOR break outside an indefinite container` |
| `01 01` | trailing byte | rejected @1 `trailing CBOR data after root value` |
| `63 6a` | truncated text | rejected @2 `CBOR byte/text string is truncated` |
| (empty) | empty input | rejected @0 `CBOR value is missing` |
| `9b 00 00 00 01 00 00 00 00` | 2^32-element array header | rejected @0 `CBOR array allocation exceeds max_bytes 1073741824` |
| 256 nested `81` + `00` | depth 256 | accepted |

Encode side: `to_bytes` of the parsed `{"b":24,"a":1}` → `[162, 97, 98, 24, 24, 97, 97, 1]` (insertion order); `to_bytes_canonical` → `[162, 97, 97, 1, 97, 98, 24, 24]` (bytewise-sorted keys). `to_bytes` of float64 `1.5` emits the preferred float16 `[249, 62, 0]`.

## Evidence: `CBOROptions` (`safe-jet.sh run --interpret cbor_opts.jet`, observed)

`cbor.parse(bytes, options)` compiles at arity 2 and the host enforces every field:

```
depth 257 arrays: rejected @256: max_depth 256 exceeded
depth 300 arrays: rejected @256: max_depth 256 exceeded
depth 3 max_depth 2: rejected @2: max_depth 2 exceeded
5 items max_items 3: rejected @3: max_items 3 exceeded
text 10 bytes max_bytes 4: rejected @0: input exceeds max_bytes 4
canonical non-shortest 0x1801: rejected @0: CBOR argument does not use its shortest form
canonical unsorted keys: rejected @4: CBOR map keys are not in Core deterministic bytewise order
canonical indefinite: rejected @0: indefinite-length CBOR is not Core deterministic
canonical float64 1.5: rejected @0: CBOR Float does not use its preferred shortest encoding
max_depth 0: rejected @0: max_depth must be in 1..4096
max_depth 5000: rejected @0: max_depth must be in 1..4096
max_items 0: rejected @0: max_items must be in 1..1000000000
max_bytes -1: rejected @0: max_bytes must be in 0..1073741824
max_bytes 2^31: rejected @0: max_bytes must be in 0..1073741824
```

These match the ratified ranges (1..4096, 1..1000000000, 0..1073741824) and the depth rule (root array = depth 1). On the interpreter, criterion 4's behaviour exists.

## Evidence: tiers (the blocking part)

- **JIT (`safe-jet.sh run`)**: any rejected parse prints the error and then aborts while dropping it. `cbor_err_min.jet` (`cbor.parse([U8]{247}) ?? { print(err.reason); return }`) prints `rejected: unsupported CBOR simple value 23`, then `internal compiler error: JIT drop 'CBORErrorKind' enum discriminant is invalid`, exit 101. The interpreter runs the same file cleanly (exit 0).
- **Checker vs runtime error type**: the checker types `err` as `cbor.CBORError`/`CBORErrorKind` (3 variants in `Core.jet:86`; the spec :920 lists 7), but the host returns `jet_std::EncodingError` (`EncodingCodecs.rs:478-505`). Matching `err.kind` against `EncodingErrorKind` is E0112; matching `CBORErrorKind` makes the interpreter fail with `E0956 MIR reached an unreachable terminator`.
- **AOT (`safe-jet.sh build cbor_probe.jet`)**: the generated Rust does not compile: `error[E0425]: cannot find type 'CBORError' in module 'jet_std'` (and `CBORErrorKind` for the match variant).
- **Typed decode**: `cbor.decode<Blob>(wire)` hits an ICE on JIT, interpreter and AOT: `checked Core call 'core.encoding.cbor.decode' has no canonical TIR record (Unknown)` (`cbor_typed.jet`). Typed fidelity (criterion 2) cannot be exercised.

## Verdict

- Criterion 1 (enumerate cells): **met** by the table above, on the interpreter.
- Criterion 2 (dynamic/typed fidelity): **not met**. Dynamic Int extremes round-trip, but typed `decode<T>` ICEs on all tiers and NaN crashes every tier.
- Criterion 3 (expansion needs a ballot): **met as a record**. No expansion is proposed. Tags (including RFC 8746 typed arrays and tag 1/28/29), bignums, bstr-in-DataTree, non-text keys and undefined/simple values stay rejected, which matches spec :879-885. `Core/encoding/cbor.jet:29-37` is a stale description (it says half/single floats and indefinite items are rejected, but the shipped host accepts them per spec :893-901). Rewriting that header, or deleting the dead Jet decoder, is Core cleanup for the fixer.
- Criterion 4 (golden `serde/encoding_cbor_profile` on every tier): **not met**. The option enforcement is correct on the interpreter, but JIT crashes on any error and AOT does not compile, so no golden can pass. No `.out` was blessed.

Overall: **PARTIAL**. Defects D1-D4 in the closer report block the golden.
