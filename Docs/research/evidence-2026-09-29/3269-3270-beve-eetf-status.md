# #3269 BEVE and #3270 EETF — implementation status

Date: 2026-09-29. Binary: `jet-debug-snapshot14`.

## Question

Can the BEVE (#3269) and ETF (#3270) criteria be proven on the current tree?

## Method

- `grep -i "beve|eetf"` over `Core/`, `Compiler/`, `crates/jet-codegen/src/Prelude/` and `Examples/` found no codec, registry row, dispatcher row or example. Every match is an unrelated identifier (`WebEvent`, `FleetField`, `JetJobEventSink`).
- `ls Core/encoding`: base32, base64, binary, cbor, csv, encoding, hex, ini, json, jsonl, toml, xml, yaml.
- Tower decisions: `node Tools/tower/tower.mjs card show 3269|3270`. D-CORE-BEVE1 is ratified A (2026-09-12). D-CORE-EETF1 and D-CORE-ETF-CODEC1 are both ratified A (2026-09-12).

## Contracts captured from the ratified ballots (#3270 criterion 1)

- **D-CORE-EETF1=A**: `core.encoding.eetf.to_json(bytes: [U8], limits: EncodingLimits{EncodingLimits.safe()}) -> String EncodingError!`. Standalone ETF version131 (OTP 29.0.6 / ERTS 17.0.6), not distribution or LOCAL_EXT. It fully validates one term and rejects trailing bytes. Integers and bignums stay exact; finite floats use the JSON writer. Atoms `true`/`false` become Bool; other atoms become text. Proper lists, NIL and tuples become arrays. STRING_EXT has the same value as a byte list. Binaries become Base64 text; non-byte-aligned bitstrings are Unsupported. Map keys may be atoms, UTF-8 binaries or Unicode-scalar lists; any other key, or a collision after conversion, is rejected. Pids, ports, refs and funs are Unsupported. No Erlang runtime is used.
- **D-CORE-ETF-CODEC1=A**: `decode<T: Decode>(bytes, layout: Layout{.Map}, strings: StringEncoding{.Utf8Binary}, byte_arrays: ByteEncoding{.Binary}, limits)` on the FieldError rail, and `encode<T: Encode>` returning `[U8] EncodingError!`. Layout is Map or Proplist. Reader, writer and caller-buffer operations use the same adapter.
- Host workload named in both ballots: the COUNT-ETF-131 fixtures `[131,116,0,0,0,1,109,0,0,0,5,99,111,117,110,116,97,2]` (binary key) and `[131,116,0,0,0,1,119,5,99,111,117,110,116,97,2]` (atom key), which must yield `{"count":2}` / `CountRecord{count: 2}`.
- **D-CORE-BEVE1=A** (#3269): Version2 wire only: little-endian, low-3-bit tags, compressed counts, headerless typed elements, zero reserved bits. Version1 is read only through an explicitly named typed operation.

## Verdict

Neither module exists, so every criterion that needs execution is unmet: #3269
criteria 1-3 and 5-9, and #3270 criteria 2-3, 5-12 and 14. What is met:

- #3269 criterion 4, by the ratified D-CORE-BEVE1=A.
- #3270 criterion 4, already met in Tower.
- #3270 criterion 13 (separate recorded outcomes, both A).
- #3270 criterion 1 (contract and workload captured above).

Both cards are implementation work: a new Core module, registry rows in
`Compiler/JetFoundation/Source/Registry/CoreCallRows.jet`, and a
`gen-core-tables` regeneration. That is outside a closer's scope (no compiler
changes).
