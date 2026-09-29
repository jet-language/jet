# #3106: bounded streaming data paths (SCRIPT-F15)

Closer10, 2026-09-29. Binary: `~/.cache/jet-luna/safe-jet.sh` →
`jet-debug-snapshot14`. Peer: CPython 3.14.7 `csv`. Verdict: **PARTIAL**: criterion 3 met (ballot draft below); criteria 1 and 2 unmet because the stream runs on no tier.

## Question

For one large bounded CSV task, can a Jet consumer start and stop before the
whole source is materialized, with explicit buffers and limits, and a
predictable stop on malformed or oversize input? How do retained memory and
time compare with a streaming peer?

## Workload

`~/.cache/jet-luna/stream-probe/gen.py` (deterministic) generates:

| File | Size | sha256 prefix | Content |
|---|---|---|---|
| `big.csv` | 204,002,029 B | `85664c9e2a2ef5b7` | header + 2.6 M rows; every 1000th row (i%1000==7) has kind `match` |
| `big.jsonl` | 222,668,208 B | `d2ef240c570d12de` | 2 M records (generated; not exercised, see below) |
| `malformed.csv` | 15,541,630 B | `c42650db41f488bc` | 200,000 good rows, then `200000,"broken,1,pad` (unterminated quote), then 1,000 rows |
| `oversize.csv` | 20,971,582 B | `381b9f2a25809fe5` | a row with one 20 MiB field, above `EncodingLimits.safe().max_item_bytes` = 16 MiB |

The task: take the first 10 `match` rows of `big.csv` and stop. The hostile
files are scanned until an error or EOF.

Jet programs, in `~/.cache/jet-test-scratch/Closer10/stream/`:

- `csv_stream.jet`: `files.open(path)` → `csv.reader(^input, encoding.EncodingLimits.safe())`,
  a `reader.next()` loop, stopping at 10 matches.
- `csv_string.jet`: `files.read(path)` → `csv.rows(text)`, the same loop.

Measurement is `measure.py` (`os.wait4`: wall, maxrss, utime, stime).

## Evidence

### Source/provider split (confirmed by execution)

- `Core/encoding/csv.jet:96-99` declares `pub fn reader(text: String) -> CSVReader`.
- The checker binds `csv.reader` to the FileReader provider instead:
  `csv_string.jet` calling `fmt.reader(text)` fails with
  `E0112: reader wants FileReader for argument 1, but this is String` and
  `E0201 … move marker ^`.
- So the String reader declared in Core is unreachable by its own name.
- The same split breaks the plan's proof tests. With
  `target-integ/debug/deps/encoding_parity-a5cbcf2eb785ef59`, both named
  tests fail at the AOT compile:
  - `jsonl_csv_xml_cbor_streams_match_aot_and_default_dev` →
    `E1220 root function run::run uses the Time.Wait effect, which this package's budget doesn't allow`.
    File-stream reads now reach `Time.Wait`, and the fixture budget lacks it.
  - `malformed_limits_and_terminal_errors_match_across_applicable_tiers` →
    `E0104 writer expects 0 arguments, got 3` on `json.writer(^fs_write, limits, false)`.
    Here the Core `json.writer()` shadows the provider writer.
  - Log: `~/.cache/jet-test-scratch/Closer10/encoding_parity.log`.

### File-backed streaming reader (the path the card needs)

| Tier | Command | Result |
|---|---|---|
| AOT | `safe-jet.sh build --allow=FS,FS.Read,Time.Wait csv_stream.jet` | ICE: "the generated Rust did not compile". rustc errors on `jet_enc_csv_reader(...)` (argument mismatch), `jet_enc_csv_reader_next(&…)` ("types differ in mutability"), and `error.line` (`Result<JetInt,_>` vs `Result<i64,_>`). |
| default `jet run` | `safe-jet.sh run --allow=… csv_stream.jet` | the same rustc rejection, exit 101 (56.9 s, including the compile) |
| interpreter | `safe-jet.sh run --interpret csv_stream_pkg.jet` (an inline `package {}` authority block, because `--interpret` rejects `--allow` with E2102) | `E0956 core.encoding.limits_safe() isn't supported by the current evaluator yet`, exit 1 (218.8 s, peak RSS 175,000 KiB) |

The first version of the probe printed `{error.kind:Debug}`. The checker
accepted it, and AOT then failed with
`the trait bound EncodingErrorKind: JetDebug is not satisfied`, a second
emission defect.

### Whole-text baseline and peer (same task)

| Program | Tier | wall s | peak RSS | Result line |
|---|---|---|---|---|
| Jet `files.read` + `csv.rows` | AOT (`.jet/build/csv_string`) ×3 | 2.20 / 2.14 / 2.18 | 2,638,360 / 2,638,980 / 2,637,816 KiB (2.52 GiB, 12.9× the file) | stopped after 9009 rows with 10 matches (counts the header row) |
| Jet `files.read` + `csv.rows` | default `jet run` ×1 | 36.06 (including the debug compile) | 2,637,620 KiB | same |
| CPython `csv.reader(open(...))`, streaming | ×3 | 0.177 / 0.019 / 0.018 | 12,276 / 12,340 / 12,340 KiB | stopped after 9008 rows with 10 matches |
| CPython whole-text `read()` + `csv.reader(splitlines)` | ×3 | 2.88 / 3.00 / 2.53 | ~1,477,000 KiB | same |
| CPython streaming, hostile files, `field_size_limit(16 MiB)` | ×3 | ~0.23 | ~101,900 KiB | malformed: **no error**, it swallows to EOF ("stopped after 200001 rows"); oversize: `field larger than field limit (16777216)` at line 3 |

## Criteria

1. **Fail.**
   - The only Jet route that runs (whole text) materializes the full 204 MB
     source into a 2.5 GiB peak before the first row, against 12 MB for the
     streaming peer.
   - The file-backed `csv.reader(^input, limits)` stream does not compile on
     AOT or default `jet run`.
2. **Not demonstrated.**
   - `EncodingLimits.safe()` makes the buffers and limits explicit in source
     (`buffer_bytes: 65536`, `max_item_bytes: 16777216`, `json.jet:72-81`).
   - The malformed/oversize stop could not be observed through the streaming
     reader on AOT or default run.
   - The interpreter rejects `EncodingLimits.safe()` with E0956, so no tier
     runs the stream.
3. **Balloted, no zero-copy claim.**
   - No new dependency is proposed.
   - The missing piece is not a new API. The ratified file-backed reader
     (D-ENCSTREAM-SURFACE1=A, cited at `Core/encoding/jsonl.jet:14-16`) is
     not declared in the Core `.jet` sources, and it clashes with the String
     readers under the same name.
   - Ballot draft below.

## Ballot draft: D-ENCSTREAM-NAMES1 "One spelling per CSV/JSON/JSONL/XML reader source"

**Question.** `core.encoding.<fmt>.reader` names two things. One is the
String reader declared in Core (`reader(text: String)`, which materializes).
The other is the ratified FileReader stream (`reader(^input, limits)`).
Only the provider one resolves. Which spelling does each source get?

**Same program in each option:** first 10 matching rows from a large file,
then stop.

- **A: stream owns `reader`, text gets `parse`/`rows` only (recommended).**
  `reader :: csv.reader(^files.open(path) ?? …, EncodingLimits.safe())`.
  - Declare the provider signature in `Core/encoding/{csv,json,jsonl,xml}.jet`.
  - Delete the String `reader(text)` declarations. Whole-text users already
    have `csv.rows(text)` and `json.parse(text)`.
  - Beginner path: `csv.rows(files.read(p))` for small files.
  - Expert path: `csv.reader(^file, limits)` with explicit limits.
- **B: separate names.** `csv.reader(text)` stays the String reader, and
  the stream becomes `csv.stream(^input, limits)`. This keeps today's Core
  declarations. It moves the ratified surface, so it needs an owner
  re-ruling of D-ENCSTREAM-SURFACE1.
- **C: overload on argument type.** Both stay `reader`. That needs
  overload resolution Jet does not have (one meaning per name).

**Edge cases.**

- The stream reader must surface `Syntax` with `byte_offset` and `line` for
  an unterminated quote. It must not swallow to EOF, which is the CPython
  default.
- It must surface `Limit` at `max_item_bytes` for an oversize field.
- The file handle must close when the reader is dropped after an early
  break.

**Recommendation.** A. The implementation card must also fix the three AOT
emission errors above, and add `Time.Wait` to the encoding_parity fixture
budgets or remove it from file-stream reads.

## Not exercised

- JSONL, XML and the client/network composition. `big.jsonl` was generated,
  but the CSV reader already fails at emission, and the JSONL provider
  shares the stream codegen path. That shared path is an [INFERENCE] from
  `encoding_parity.rs` using one fixture shape for jsonl/csv/xml/cbor; it was
  not run.
- `Tools/perf/stream_bench.py` was not run.
