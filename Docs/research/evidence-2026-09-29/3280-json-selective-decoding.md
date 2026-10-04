# 3280 — Selective and lazy JSON decoding without hidden validation loss

Date: 2026-09-29. Closer02. Binary: jet-debug-snapshot14 via `~/.cache/jet-dev/safe-jet.sh`.

## Question

Does projecting one field out of a JSON document (typed `json.decode<Small>`) validate as much as a full `json.parse`? What does the event reader do? Is there any demanded-prefix or lazy extraction, and what does each path cost?

## Method

- Read `Core/encoding/json.jet:93-201` and `crates/jet-codegen/src/Prelude/Core.jet:94-95,749`. `json.parse` and `json.reader(text)` are Jet source. `json.decode<T>` is the host lenient decoder. The dispatcher row `core.encoding.json reader → jet_enc_json_reader(FileReader, limits)` also exists.
- Witnesses in `~/.cache/jet-dev/scratch/Closer02/`: `json_sel3.jet` (a 20-field document projected to `struct Small { a: Int }`, 8 inputs), `json_reader_min.jet`, `json_sel2/json_sel.jet` (file reader), `json_sel2/golden_stream.jet` (a copy of the committed `Examples/features/serde/encoding_json_stream.jet`), and `perf/json_sel_perf.jet`.

## Evidence: validation boundary (JIT, `json_sel3.jet`, observed)

```
valid | full parse: ok
valid | typed projection: ok a=1
malformed skipped subtree | full parse: rejected @14: expected a JSON value (object, array, string, number, true, false, or null)
malformed skipped subtree | typed projection: rejected [invalid JSON (line 1): expected a JSON value]
trailing garbage | full parse: rejected @159: unexpected trailing data after the JSON value
trailing garbage | typed projection: rejected [invalid JSON (line 1): extra text after JSON value]
second root | full parse: rejected @8: unexpected trailing data after the JSON value
second root | typed projection: rejected [invalid JSON (line 1): extra text after JSON value]
duplicate key in skipped subtree | full parse: rejected @22: duplicate object key `k`
duplicate key in skipped subtree | typed projection: rejected [invalid JSON (line 1): duplicate object key `k`]
lone surrogate escape in skipped string | full parse: rejected @19: high surrogate must be followed by a low surrogate
lone surrogate escape in skipped string | typed projection: rejected [invalid JSON (line 1): unpaired surrogate in string]
bad escape in skipped string | full parse: rejected @14: unknown string escape; JSON allows \", \\, \/, \b, \f, \n, \r, \t, \uXXXX
bad escape in skipped string | typed projection: rejected [invalid JSON (line 1): invalid escape in string]
truncated | full parse: rejected @11: input ended before the value was complete
truncated | typed projection: rejected [invalid JSON (line 1): expected a JSON value]
```

**Finding:** the typed projection validates the whole document. It rejects exactly the same eight inputs as the full parse, including malformed data inside skipped fields, duplicates in skipped objects, bad escapes in skipped strings, and trailing data. Ordinary decode does not weaken validation. The two paths differ in location precision: full parse reports a byte offset, while projection reports only `line 1` with differently worded reasons.

Tiers for this witness: the interpreter fails with `E0956 core.builtin.map_has_key() isn't supported by the current evaluator yet`. The AOT build fails in rustc with `jet_std::DataEvent::Int(…) expected i64, found JetInt` (json.jet). **Only the JIT runs `json.parse`.**

## Evidence: event reader (failure timing cannot be observed)

- Text pull reader, `json_reader_min.jet` (`json.reader("[1]")`, then `&reader.next()`):
  - JIT prints `rejected @0: Core/encoding/json.jet` (the reason is a file path, not an error) and then `internal compiler error: typed drop 'EncodingFormat' field 0 is unavailable`.
  - Interpreter: `E0956 core.handle.json_reader.next() isn't supported`.
  - AOT: rustc `error[E0609]: no field 'parser' on type 'JSONReader'`. The host `jet_std::JSONReader` has fields `input, limits, total, offset, line…`. The Core source `JSONReader{parser}` and the host stream reader collide on one type name.
- File reader: `json.reader(^input)` with a `FileReader` is rejected by the checker (`E0112 'reader' wants String (text) here, but this is 'FileReader'`, plus `E0109 loop … needs a list or map, not JSONReader`). The **committed golden `Examples/features/serde/encoding_json_stream.jet` does not compile on this binary** (30 errors: `writer has no parameter labelled canonical`, `write wants DataTree … but this is DataEvent`, `reader wants String`, …). The source surface in json.jet has displaced the host bounded stream API (D-ENCSTREAM-SURFACE1).
- By source (json.jet:119-120): the text reader's "first call validates and materializes the value, then subsequent calls walk its event sequence". So even when it works, failure timing is "all at the first `next()`", and there is no incremental failure.

## Evidence: cost (JIT, single samples)

`perf/json_sel_perf.jet`, a 250-field document (17 287 bytes):
`full_parse_us=2636033 typed_projection_us=123356`. The Jet-source full parse takes 2.6 s for 17 KB and the host projection takes 0.12 s. The program then ICEs (`typed drop 'Int' field 0 is unavailable`) before the next sample, and at 6 000 fields the run hit the 300 s safe-jet timeout (exit 137). Repeated runs, allocation counts and an event-path figure could not be taken. AOT, which would be the honest perf tier, does not compile `json.parse`.

## Cells

| Cell | Status |
|---|---|
| Full-document validation (`parse`) | supported (JIT) |
| Typed projection validates the whole document | supported: same accept/reject set as full parse |
| Demanded-prefix or lazy extraction | **unsupported** (no API; the text reader materializes on the first call) |
| Borrowed or lazy views into the input | **unsupported** (all results are owned) |
| Bounded file event reader (D-ENCSTREAM-SURFACE1) | **unreachable** on this binary (checker binds the Core text reader) |

## Criteria

1. Full-document validation versus demanded-prefix extraction: **met (investigation).** Only full validation exists. Projection validates everything. Demanded-prefix extraction is unsupported.
2. Malformed skipped or trailing data, borrowed lifetimes, failure timing: **met for skipped and trailing data** (above). Borrowed lifetimes: none exist, since everything is owned. Failure timing: by source, all at the first `next()`. It was not observable at runtime because of the reader defects (recorded as defects).
3. Measure repeated scans, materialization and allocations: **BLOCKED.** One JIT sample per path (above). Repeated samples ICE, AOT does not compile, and there is no allocation counter.
4. Gate expert APIs rather than weakening ordinary decode: **met.** Ordinary decode is not weakened, and no lazy API is proposed. Any future lazy or borrowed reader needs a ballot.
5. Golden `serde/json_selective` passes, full and projected fail identically, perf numbers recorded: **not met.** "Fail identically" holds for the accept/reject set on the JIT, but the golden cannot pass (interpreter unsupported, AOT does not compile, event path broken), and the perf numbers are single samples only.

## Verdict

**PARTIAL.** Criteria 1, 2 and 4 are met with evidence. Criterion 3 is BLOCKED. Criterion 5 is unmet because of defects.
