# #3262 — Unicode, regex and text-encoding contracts (evidence, 2026-09-29)

## Question

How does the shipped `core.text` / `core.regex` / `String.from_bytes` behave on case
mapping and folding, normalization, grapheme versus scalar versus byte counts, regex
complexity and invalid UTF-8? Is the plan's one real gap (full case folding) still open?

## Method

- Binary: snapshot14 through `~/.cache/jet-luna/safe-jet.sh`.
- Witnesses (scratch; package grants `[IO, Mem.Alloc, Panic]`):
  - `~/.cache/jet-test-scratch/Closer05/t3262/unicode_contracts.jet`
  - `~/.cache/jet-test-scratch/Closer05/t3262/regex_probe.jet`
- Tiers: `run`, `run --interpret`, and `build` plus the binary (`tiers.sh`).
- Source read: `Core/text/text.jet:12-19,73-88` and `Core/regex/regex.jet:15-20,488-529,597-600`.

## Evidence

Tier status for `unicode_contracts.jet`:

- `run --interpret`: rc=0.
- `run`: rc=101, `internal compiler error: JIT drop 'Core/regex/regex.jet::CaptureSpan<>?' value is not a result`.
- `build`: rc=101, generated Rust `error[E0599]: no method named 'jet_debug' found for struct 'RegexFlags'`.

Observed `--interpret` output, with the expected result per Unicode 17 default (locale-neutral) mappings:

| Row | Observed | Expected | Verdict |
|---|---|---|---|
| `casefold("Straße")` | `straße` | `strasse` (C+F fold) | **GAP**: `casefold` is `lower` (text.jet:78) |
| `upper("Straße")` | `STRASSE` | `STRASSE` | OK: full upper mapping for ß is present |
| `lower("ΟΔΟΣ")` | `οδοσ` | `οδος` (final sigma) | **GAP**: no Final_Sigma context. Greek does map, so the text.jet:17-19 "Latin-1 only" header is stale |
| `lower("İ")` / `casefold("İ")` | `İ` | `i̇` (U+0069 U+0307) / `i̇` | **GAP**: identity |
| `lower("ǅ")` / `upper("ǅ")` | `ǅ` / `ǅ` | `ǆ` / `Ǆ` | **GAP**: identity |
| `lower("ÄÖÜ")` | `äöü` | `äöü` | OK |
| `caseless_eq("Straße","STRASSE")` | `false` | `true` | GAP (follows from casefold) |
| `nfc(e + U+0301) == "é"` | `true` | `true` | OK |
| `inspect(nfd("é"))` | `[U+0065, U+0301]` | same | OK |
| `nfkc("ﬁ")` | `fi` | `fi` | OK |
| `nfkd("①")` | `①` | `1` | **DEFECT**: missing compatibility decomposition <circle> |
| Hangul jamo NFC | `true` | `true` | OK |
| 👨‍👩‍👧 graphemes/scalars/bytes | `1/5/18` | `1/5/18` | OK |
| 🇯🇵 | `1/2/8` | `1/2/8` | OK |
| e + U+0301 | `1/2/3` | `1/2/3` | OK |
| `String.from_bytes([104,105,255,33])` | rejected with typed `UTF8Error` | strict | OK: strict policy. With no `??`, the error propagates as `Error: <invalid> (type: UTF8Error)`. No lossy decode exists in core.text |
| regex `ignore_case` `straße` vs `STRAße` | `false` | `true` (ASCII fold on s,t,r,a; ß identical) | **DEFECT**: see the next row |
| regex `straße` vs `straße`, no flags (regex_probe, `run` and `--interpret`) | `false` | `true` | **DEFECT**: a non-ASCII literal never matches identical text |
| regex `abc` / ignore_case `abc` vs `XABCX` | `true` / `true` | `true` | OK: ASCII ignore_case works |
| `(a+)+$` on `"a"*40 + "b"` | `false`, terminates | `false` | OK: bounded. Budget exhaustion (`exec` returns -1 at regex.jet:599) cannot be told apart from no-match; there is no typed exhaustion result |
| `regex.compile("(")` | `RegexError` | error | OK |

## Accounting (criterion 1)

- Byte jobs: `byte_count`, `byte_views`. Scalar jobs: `scalar_count`, `scalars`,
  `char_indices`. Grapheme jobs: `graphemes` and `display_width` (UAX #29). Word and
  sentence segmentation are also present. These are distinct functions, and the counts above
  confirm the distinction.
- Normalization: NFC, NFD and NFKC are correct on the tested rows. NFKD misses the <circle>
  compatibility mapping.
- Case: upper has full mapping (ß→SS). Lower lacks Final_Sigma, İ and titlecase digraphs.
  Casefold equals lower (no C+F table).
- Regex: a bounded backtracking engine over UTF-8 bytes with a per-start budget
  (regex.jet:488-529). Exhaustion reports as no-match rather than a typed failure. Non-ASCII
  literals don't match (defect).
- Invalid encoding: strict `UTF8Error` from `String.from_bytes`. There is no lossy variant.

## Verdict

PARTIAL. This document meets criteria 1 to 3:

- Criterion 1: the accounting above, exercised on `--interpret`.
- Criterion 2: no owned/view measurement was taken and no cost claim is made, because no
  perf cell for these rows was exercised tonight.
- Criterion 3: no new API. A typed budget-exhaustion result or a lossy decode would each
  need a ballot.

Criterion 4 is unmet. It names the golden witnesses `Straße`→`strasse`, Greek final sigma
and the Turkish dotted I default, and none of them holds on the current binary. Full case
folding (the plan's change 1) is a Core table-generation job. Defects found: the regex
non-ASCII literal match, NFKD <circle>, the JIT ICE on regex use, and the AOT ICE
(`RegexFlags.jet_debug`).

## Follow-ups

- Implement full Unicode case mapping and C+F folding tables in `Core/text/text.jet`
  (generated like the normalization tables), then fix the stale header at :14-19.
- Fix non-ASCII literal matching in `Core/regex/regex.jet`. Repro: `regex.is_match(regex.compile("straße") ?? panic(""), "straße")` is `false` on `run` and `--interpret`.
- Add the NFKD/NFKC compatibility entries for enclosed alphanumerics (U+2460…).
- Owner gates, if wanted: typed regex budget exhaustion (today it reports no-match), and a lossy UTF-8 decode.
