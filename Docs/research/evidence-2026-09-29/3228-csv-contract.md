# #3228 — One typed and streaming CSV contract

Date: 2026-09-29. Binary: `jet-debug-snapshot14`.
Findings: `Docs/research/mine-for-jet-2026-09-12.md#finding-core-f049`, `#finding-script-f16`.

## Method

- Shapes: `Examples/features/serde/csv_shapes.jet` (in the tree, no `.out`). On every tier it ICEs:
  `show_rows: PatternTest` at the `if err.kind == { .Syntax -> ... }` match
  (bytes 431..438). Changing that to `{err.kind:Debug}` gives
  `Cranelift cannot execute ... EncodingErrorKind<> has no checked debug carrier`.
  My variant `~/.cache/jet-dev/scratch/Closer01/csv2.jet` prints only line,
  column and reason. It ran on `jet run` and `--interpret` with identical
  output. On AOT, `jet build` (I placed the file briefly as `Examples/features/serde/closer01_csv_probe.jet`) fails with generated-Rust
  `E0308 expected Result<JetInt, JetAbsent>, found Result<i64, JetAbsent>` on `err.line`.
- Streaming: `~/.cache/jet-dev/scratch/Closer01/csv_stream.jet` (a 40-row file larger than `buffer_bytes: 64`, an early break, and a 100-byte field over `max_item_bytes: 32`).
- Source read: `Core/encoding/csv.jet:40-101, 293-314`; `crates/jet-codegen/src/Prelude/CoreLib/Top/EncodingStream.rs:2422-2453`; `crates/jet-codegen/src/Prelude/Core.jet:753`.

## Evidence

`csv2.jet` (`jet run` == `--interpret`):
```
quoted delimiter: line 1 fields 3 [a, b,c, d]
embedded newline: line 1 fields 2 [id, text]
embedded newline: line 2 fields 2 [1, two
lines]
embedded newline: line 4 fields 2 [2, plain]
doubled quote: line 1 fields 2 [say "hi", x]
ragged: line 1 fields 3 [a, b, c]
ragged: line 2 fields 2 [1, 2]
ragged: line 3 fields 4 [3, 4, 5, 6]
unterminated quote: error line 2 column 4: unterminated quoted CSV field
typed ok: pen 3 blue
typed ok: ink 5
typed column order: cap 7 (none)
typed missing column: [at `row 1.qty`: expected Int, found null]
typed bad int: [at `row 1.qty`: expected Int, found text "three"]
typed ragged: pen 3 (none)
typed quoted newline: fountain
pen 2 (none)
```

Streaming: `csv.reader(^input, limits)` does not type-check. The error is
`E0104 reader expects 1 argument, got 2` and `reader wants String`, and a loop
over the result gives `E0109 for x in needs a list or map, not CSVReader`.
The Jet source `reader(text: String)` at csv.jet:96 now shadows the host
streaming route `jet_enc_csv_reader(FileReader, limits, delimiter, header, skip_blank)`.
The existing golden `serde/encoding_csv_stream.jet` fails the same way
(`writer expects 0 arguments, got 2`, `CSVWriter has no method write`).

## Declared results (criteria 1, 5)

| shape | result today |
|---|---|
| quoted delimiter `"b,c"` | one field `b,c` |
| quoted newline | one field containing `\n`; the next record's `line` is the physical line (4) |
| doubled quote `""` | literal `"` |
| unterminated quote at EOF | error `line 2 column 4: unterminated quoted CSV field` (the kind cannot be printed: ICE) |
| ragged rows via `rows` | kept as-is (3/2/4 fields); no rectangularity check |
| ragged row via typed `decode<T>` | **extra field silently dropped** (`pen 3 (none)`); defect |
| ragged row via `dict_rows` | short rows padded with `""` and long rows truncated (source csv.jet:303-309); defect, not run |
| missing column (typed) | `[at row 1.qty: expected Int, found null]`; missing and null are not distinguished |
| empty cell into `String?` | `Some("")`, not None (`ink 5` then an empty note) |
| wrong type | `[at row 1.qty: expected Int, found text "three"]` |
| header mapping | by name, independent of column order (`qty,name` → `cap 7`) |

## Dialect / encoding / streaming map (criterion 6)

| option | current API |
|---|---|
| delimiter | host reader parameter only (`jet_enc_csv_reader(..., delimiter, ...)`), **unreachable** while shadowed; Jet `rows`/`decode` are comma-only |
| header on/off | host reader `header: bool` (unreachable); Jet `rows(header:)` ignores the flag (`_ :: header`, csv.jet:73) |
| skip blank lines | `rows(skip_blank:)` (Jet) and the host reader |
| quote char, escape char, `skipinitialspace`, comment lines, BOM stripping, non-UTF-8 input | **unsupported** (no parameter anywhere) |
| bounded streaming | host `CSVReader` over `^FileReader` with `EncodingLimits` (spec encoding-decisions.md:116-147); **currently unreachable** |
| writer | Jet `csv.writer()` builds an in-memory `CSVWriter`; the host streaming writer is shadowed |

## Column transposition (criterion 3)

Core ships no column API. A column view is built by collecting each row's field
into per-column lists. That allocates a new `[String]` per column and copies
every cell, so it is not allocation-free. The typed `decode<T>` path allocates
one `T` per row (source: csv.jet `parse` builds a `DataTree` array before the
typed projection).

## Ballot drafts (criteria 4, 7)

- **D-CSV-RECT (rectangularity).** A: `dict_rows` and `decode<T>` fail with `EncodingError{kind: Syntax}` and the reason `row N has M fields; the header has K`. B: keep padding/truncation and document it. Recommendation: A, because silent truncation loses data.
- **D-CSV-HEADER-PARAM.** A: delete the ignored `header` parameter of `rows` and migrate callers. B: implement it (return data rows only). Recommendation: A, because `dict_rows`/`decode` already own header semantics.
- **D-CSV-DIALECT.** A: add `quote: Char{'"'}` and `trim_leading_space: Bool{false}` to the reader. B: RFC 4180 only, with other dialects unsupported. Recommendation: B until a user workload needs more.
- **Streaming route (not a ballot, a defect):** restore `csv.reader(^FileReader, limits)` / `csv.writer(^FileWriter, limits)` so the Jet source functions no longer shadow them (spec encoding-decisions.md:116-147 is ratified).

## Verdict

- Criteria 1 and 5 are met as observed and declared results. The typed ragged truncation and missing-versus-null are defects.
- Criteria 3, 4, 6 and 7 are met by this document.
- Criterion 2 (bounded streaming and close) is **not met**: the streaming reader is unreachable on snapshot14.
