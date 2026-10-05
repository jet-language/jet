# 3275 — Binary-to-text encodings and fixed-buffer contracts

Date: 2026-09-29. Closer02. Binary: jet-debug-snapshot14 via `~/.cache/jet-dev/safe-jet.sh`.

## Question

Do the Core base encodings (base64, base64url, base32, base32hex, hex) and `core.encoding.binary` reject malformed, noncanonical, overflowing and truncated input instead of repairing it? Do encoded lengths match the documented required-size formulas? And which fixed-buffer controls (required-size query, caller-owned output, borrowed view) are missing and need separate owner ballots?

## Method

- Read `Core/encoding/base64.jet:1-60`, `base32.jet`, `hex.jet:1-100`, `binary.jet:1-160` and `crates/jet-codegen/src/Prelude/Core.jet:80-91`. All of these are Jet source modules.
- New witness `Examples/features/serde/encoding_base_hostile.jet` and scratch probes `~/.cache/jet-dev/scratch/Closer02/base_probe.jet`, `pack_probe.jet` and `pack_probe2.jet`, run on JIT, interpreter and AOT with `tiers.sh` (AOT binary `.jet/build/<stem>`).
- Grep of `Core/` for `encode_into|decode_into|encoded_len|decoded_len` finds no matches (confirmed by the triage log on the card and by the export lists at `Core.jet:80-91`).

## Evidence

Observed output of the witness is recorded verbatim in `Examples/features/expected/serde/encoding_base_hostile.out` (identical on JIT, interpreter and AOT; see the closer report). Summary:

- Padding: base64 `Zg` (missing), `Zg===` (extra), `Zg==Zg==` (interior) are all rejected with distinct reasons. base64url `Zg==` is rejected (no `=` in the canonical URL alphabet). base32 `MY` (missing), `MY=====` (short) and `MY======MY======` (interior) are rejected.
- Noncanonical bits: base64 `Zh==`, base64url `Zh` and base32 `MZ======` are rejected with `non-zero unused bits`. Only `Zg==`, `Zg` and `MY======` decode to `[102]`.
- Alphabet and whitespace: mixed alphabets (`-_8=` in standard, `+/8` in URL, `MY======` in base32hex), lowercase base32, spaces, newlines and a non-ASCII byte are all rejected. There is no silent whitespace skipping.
- Hex: odd length and bad digits are rejected, and a `0x` prefix is rejected. For `"0a ff"` (5 bytes) the reported reason is the odd length, not the space.
- Empty input decodes to `[]` for every codec.
- Required size, for n ∈ {0,1,2,3,4,5,6,31,32,33}: encoded length equals `4*ceil(n/3)` (base64), `ceil(4n/3)` (unpadded base64url), `8*ceil(n/5)` (base32) and `2n` (hex) in every row.
- `binary`: `pack_u8(256)`, `pack_u8(-1)`, `pack_i8(128)`, `pack_u16le(65536)` and `pack_u32be(4294967296)` are rejected (`BinaryError`). `unpack_u32le` on 3 bytes, `unpack_u16be` at offset -1 and `unpack_u64le` on 7 bytes return absence (`null`).

## Defects found (not blessed into the witness)

- **Packers stop on in-range values ≥ 256.** `binary.pack_u32be(256)` → `Stop [E3010]: Value doesn't fit in destination type --> Core/encoding/binary.jet:59` on JIT. `pack_u16le(256)` stops at `binary.jet:44`. `pack_u32be(1)` and `pack_u32be(255)` succeed (`pack_probe.jet`, `pack_probe2.jet`). The masked expression `U8{(n >> k) & 255}` stops instead of producing the byte. The earlier full run showed the interpreter reporting the same stop as `E3001 panic: Value doesn't fit in destination type` while JIT/AOT say `E3010` (different stop codes across tiers).
- **`sign_extend` width outside its range stops the program.** `binary.sign_extend(1, 0)` → `Stop [E3010]: Invalid shift count --> Core/encoding/binary.jet:133` on JIT/AOT. The function is declared `-> Int Never!` (total), so an out-of-range width has no typed error. On the interpreter the same program fails at *compile time* with `Error [E3010] Invalid shift count` pointing at an unrelated line (`base_probe.jet:130`).
- **`BinaryError` is not observable by callers.** `use core.encoding.binary.[BinaryError]` gives `E0119 There's no type called BinaryError`. Matching `.Range` on `err` gives `E0305 … BinaryError is a struct, not an enum`. `{err:Debug}` gives `E0112 … can't be shown with :Debug yet`. The source declares `pub enum BinaryError { Range Format Values Buffer }` (binary.jet:22-28), so callers cannot tell Range from Format.

## Criteria

1. Malformed padding, noncanonical text, overflow and truncation, exact required size: **met** by the witness, apart from the defects above, which are recorded but not blessed.
2. Prove caller buffer ownership: **not provable. No caller-buffer API exists.** Every encoder returns a newly owned `String`/`[U8]` (`encode(data: [U8]) -> String`, `decode(text) -> [U8] Base64Error!`). This is gated as ballot (b) below.
3. Byte view versus owning conversion: **only owning conversions exist.** `String.bytes()`, and every decode, produce an owned `[U8]`. There is no borrowed view type in these modules. This is gated as ballot (c) below.
4. Gate missing controls individually: **three ballot drafts below**, for Pip to file (closers make no Tower writes).
5. Golden `serde/encoding_base_hostile` on every tier: see the closer report for the three-tier diff.

## Ballot drafts

### D-ENC-REQSIZE1 — required-size queries

- Question: should the base codecs expose the exact output size before encoding or decoding?
- Option A (recommended): `base64.encoded_len(n: Int) -> Int`, `base64.encoded_len_url(n)`, `base32.encoded_len(n)`, `hex.encoded_len(n)`, and `decoded_len(text: String) -> Int Base64Error!` (validates the length and padding only, without decoding). Beginner path: ignore them (`encode` still allocates exactly). Expert path: size a buffer or a protocol header up front. Formulas: `4*ceil(n/3)`, `ceil(4n/3)`, `8*ceil(n/5)`, `2n`, as pinned by the witness.
- Option B: no API. Callers use the documented formulas (the witness proves them). This costs every caller re-deriving the arithmetic, including the unpadded URL case.

### D-ENC-INTO1 — caller-owned output buffer

- Question: should encoders write into a caller-owned buffer?
- Option A: `base64.encode_into(data: [U8], out: &[U8]) -> Int Base64Error!` appends to `out` and returns the written count. When capacity policy is exceeded it reports the shortage **before** writing anything (no partial write). Beginner: `encode(data)`. Expert: reuse one buffer across calls.
- Option B (recommended until a measured need exists): no API. Owned returns stay the one path, and AGENTS.md's performance policy asks for a paired two-program measurement before adding a performance surface.

### D-ENC-VIEW1 — borrowed byte view versus owning conversion

- Question: should decode offer a borrowed view (for example, validating base64 in place and returning a view over the input) alongside the owning `[U8]` result?
- Option A: `base64.validate(text) -> Int Base64Error!` returns the decoded length without allocating, and `decode` stays owning. This is the smallest non-allocating surface.
- Option B: a general `ByteView` borrowed from a `String`/`[U8]`. This is a language and memory-model decision, larger than this card.
- Option C (recommended): owning only, recorded as the explicit profile. Revisit with a measurement.

## Verdict

**PARTIAL.**

- Criterion 1 is met: `Examples/features/serde/encoding_base_hostile.jet` and its `.out` are byte-identical on JIT, interpreter and AOT.
- Criterion 3 is met: only owning conversions exist.
- Criterion 4 is met by the three drafts above.
- Criterion 2 is unmet: there is no caller-buffer API to prove, and that API is gated by D-ENC-INTO1.
- Criterion 5 is unmet: the golden harness `target-integ/debug/deps/golden-e0c70ed3bfd023e2` with `JET_GOLDEN_FILTER=serde/encoding_base` fails both the new example and the pre-existing `serde/encoding_base`, because the L0507 gate (tests/golden.rs:655) counts lints whose spans lie in `Core/encoding/base64.jet`, `base32.jet` and `binary.jet`. That harness binary also reports `rustc not found; checking codegen only`.

The three binary defects are open.
