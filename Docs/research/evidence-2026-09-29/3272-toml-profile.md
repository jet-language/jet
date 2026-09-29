# 3272 — TOML version, typed round-trip and write order

Date: 2026-09-29. Closer02. Binary: jet-debug-snapshot14 via `~/.cache/jet-luna/safe-jet.sh`.

## Question

Which TOML grammar and version does Core ship, and which constructs are unsupported? Do typed dates and numbers survive decoding, do parse errors carry useful locations, and what write order does `to_string` actually produce?

## Method

- Read `Core/encoding/toml.jet:1-60` and `crates/jet-codegen/src/Prelude/Core.jet:98-99`. There are two parsers. The Jet source module owns `parse`, `load` and `loads`. `decode<T>` and `to_string` are host (`crates/jet-codegen/src/Prelude/CoreLib/Top/DataFmt.rs:153-222`, `jet_std::toml::parse_to_tree` and `render`).
- Scratch witnesses in `~/.cache/jet-test-scratch/Closer02/`: `toml_min.jet` and `toml_probe.jet` (Jet-source `parse`), `toml_probe2.jet` (`decode<DataTree>`), and `toml_typed.jet` (typed structs), run on JIT, interpreter and AOT with `tiers.sh` (AOT binary at `.jet/build/<stem>`).

## Evidence

### Jet-source `toml.parse`: ICE on every tier

`toml_min.jet` (`toml.parse("a = 1\n")`), JIT, interpreter and AOT build all fail with:

```
internal compiler error: checked TIR cannot lower to MIR at 1458..1459: __jet_core_core_encoding_toml::__jet_parse_toml: missing checked function target `<corelib>/Core/encoding::Core/encoding/toml.jet::TomlParser::fail`
```

So the untyped `DataTree` path, and with it the `EncodingError{line, column, byte_offset}` locations advertised at toml.jet:13-24, cannot run. `toml.decode<DataTree>` is not a substitute: every document rejects with `[unknown DataTree variant]` (`toml_probe2.jet`, JIT).

### Host typed `toml.decode<T>` and `to_string` (JIT, observed; AOT identical except where noted)

```
ints: 99 -17 1000 3735928559 493 13                         (+, _, 0x, 0o, 0b accepted)
n = 9223372036854775807: 9223372036854775807
n = -9223372036854775808: -9223372036854775808
n = 9223372036854775808: rejected [invalid TOML (line 1): invalid number `9223372036854775808`]
n = 0x7FFF_FFFF_FFFF_FFFF: 9223372036854775807
n = 0x8000_0000_0000_0000: rejected [invalid TOML (line 1): invalid base-16 integer `8000000000000000`]
n = 012: 12                                                  (TOML 1.0 forbids leading zeros)
n = 1__0: 10                                                 (TOML 1.0 forbids doubled underscore)
n = 10_: 10                                                  (TOML 1.0 forbids trailing underscore)
n = _10: rejected [invalid TOML (line 1): `_` does not start a valid value]
n = 1.5: rejected [at `n`: expected Int, found Float]
n = "7": 7                                                   (a string silently coerced to Int)
floats: 3.14 -0.0 5e22 inf NaN
strs: <tab\tq"ué> <C:\path> <line1\nline2>
dates as text: 1979-05-27T07:32:00-08:00 | 1979-05-27T07:32:00.999999 | 1979-05-27 | 07:32:00
"n = 1\nn = 2\n": accepted                                   (duplicate key accepted; TOML 1.0 requires rejection)
"[t]\nn = 1\n[s]\n[t]\nm = 2\n": rejected [at `n`: missing field `n`]   (table redefinition not detected; AOT prints `expected Int, found null` instead)
"n = \"abc\n": rejected [invalid TOML (line 2): unterminated string]
"n = {{ x = 1,\n y = 2 }}\n": rejected [invalid TOML (line 1): `\n` is not a valid key character]   (TOML 1.1 newline in an inline table: rejected)
"n = \"\\e\"\n": rejected [invalid TOML (line 1): invalid escape `\e`]  (TOML 1.1 \e escape: rejected)
"\n\n  bad key = 1\n": rejected [invalid TOML (line 3): expected `=` after key `bad`]
---- to_string(struct)
z = 1
(blank)
[b]
x = 2
(blank)
[a]
x = 3
---- to_string(array of tables)
(blank)
[[arr]]
k = 1
(blank)
[[arr]]
k = 2
```

Typed dates: declaring `struct TypedDate { ld: Date }` (and the same with `LocalTime`, `DateTime` or `Instant` from `core.time`) and calling `toml.decode<TypedDate>` is a compile error: `E2411 'TypedDate' can't be decoded — Only types that opt in (and their fields) have a wire form`. The `core.time` types have no Codable form, so typed dates cannot round-trip. Dates decode only into `String`.

Interpreter: `toml.decode<T>` gives `E0956 core.encoding.toml.decode() isn't supported by the current evaluator yet`.

## Criteria

1. Exact grammar and version: **recorded.** It is TOML v1.0 (toml.jet:1). The two 1.1-only constructs tried (newline in an inline table, `\e` escape) are rejected with a line number. Divergences from 1.0 in the shipped host parser: it accepts leading zeros, `__` and trailing `_`, and duplicate keys, it does not detect table redefinition, and it coerces strings to Int in typed decode.
2. Typed dates and numbers, useful locations: **not met.** Dates stay text (no Codable `core.time` types). Int bounds are exact and correct, but malformed forms are accepted. Errors carry a line only: no column, no byte offset and no path for syntax errors. The richer location path (Jet `parse`) ICEs.
3. Write order: **observed, not promised.** `to_string` writes root scalars first, then tables in struct field order (insertion order), with a blank line before each table and each `[[array]]` element. toml.jet:9 promises only "a minimal table document", so the order is an observation to document, not a contract to test.
4. Ballot for a version expansion: **none needed.** No 1.1 expansion is proposed. Staying on 1.0 is covered by existing text (toml.jet:1).
5. Golden `serde/toml_profile` on every tier: **not met.** The Jet `parse` ICEs on all tiers, the interpreter cannot run `decode<T>`, JIT and AOT disagree on one error message, and typed dates do not compile. No `.out` was blessed.

## Verdict

**PARTIAL.** Criteria 1, 3 and 4 are met with executed evidence. Criteria 2 and 5 are unmet because of defects, listed in the closer report: D-TOML1 (parse ICE), D-TOML2 (1.0 violations in the host parser), D-TOML3 (interpreter unsupported), D-TOML4 (JIT/AOT error text divergence) and D-TOML5 (`core.time` not Codable).
