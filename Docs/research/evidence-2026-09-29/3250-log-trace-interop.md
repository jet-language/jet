# #3250 — Recorder and trace interoperability (CORE-F023)

Date: 2026-09-29. Binary: `jet-debug-snapshot14`, source rev `e9c708fa7`.
Author: Closer03 (evidence closer). No compiler, runtime or Core change was made.

## Question

Where do Glaze's time-trace (Chrome `traceEvents`) and data-recorder jobs land in Jet,
what exact formats do the owners emit, how are buffers and observers bounded, and do
exported records keep the caller's source identity?

## Method (source read; runtime witness pending)

- `Core/log/log.jet:13-21,71-107` (fields, `redact`, spans, `otlp_file`, `sample_every`, `set_trace_id`).
- `crates/jet-codegen/src/Prelude/CoreLib/Top/Log.rs` (host sink and telemetry).
- `Source/CmdPerf.rs:2884-3053` (`jet perf export --chrome`), `tests/jet_perf_trace_parts/views.rs:103-114`.
- Witness `Examples/features/io/log_trace_export.jet` (nested spans, redacted field,
  `otlp_file` into a `files.temp_dir`, read-back). Its first version, which parsed the
  lines with `json.parse`, failed default `jet run` because the generated Rust did not
  compile (defect below). The rewrite masks `ts` by string splitting; its run result is
  recorded in the closer report.

## Formats (criterion 1)

- **Application log file** (`log.otlp_file(path)` = `set_sink("jsonl", path)`, Log.rs:121-123):
  one JSON object per record, `{"level","body"[,"trace_id"],"ts":<unix-ms>, <each field
  as a top-level key>, "spans":[<active span names, outer→inner>]}` (Log.rs:703-727).
  Redacted fields carry `"[redacted]"`; typed int/float/bool/counter values are bare JSON.
  This is Jet's JSON-lines shape, **not** OTLP (`resourceLogs`/`resourceSpans` never appear),
  despite the `log.jet:18` comment. Field keys share the record's top-level namespace, so a
  field named `body`/`level`/`ts` produces a duplicate key that strict `json.parse` rejects.
- **Span telemetry** (devtools events, Log.rs:553-593): `Trace` events with `name`,
  `parent_span_id`, `trace_id`, `span_id`, `start_ns`/`end_ns`/`duration_ns`,
  `linked_log_indexes`; `TraceLink` events link logs to spans. Parent links exist only here,
  not in the file sink.
- **Profiling**: `jet perf export <trace> --chrome` emits `{"kind":"jet.trace.chrome-projection",
  "loss":…, "schema", "traceEvents":[X events + M lane metadata], "trace_id", "version"}`
  (CmdPerf.rs:3036-3053) — the Glaze time-trace format, owned by tooling, not by `core.log`.
- **Recorded-value/replay**: absent from Core. That is a missing capability needing its
  own ballot, not an inferred gap.

## Bounds and observer effects (criterion 3)

Telemetry caps (Log.rs:10-13): 512-byte text, 16 fields, 64 metrics, 32 open span
observations / linked logs. `sample_every(n)` keeps every n-th record (Log.rs:773-782).
The file sink has **no in-memory buffer and no size cap**: every record reopens the file
in append mode (Log.rs:689-700), so buffering is bounded (zero) but file growth is not.
The 10k-event wall-time measurement (`~/.cache/jet-dev/scratch/Closer03/log_cost.jet`) was
not taken: BLOCKED by the request budget.

## Source identity (criterion 4)

The file sink keeps the caller's span names and trace id verbatim. The devtools span/log
events do not keep the caller's location: they hard-code `source_id "core.log"`,
`source_file` = the log implementation, `source_line` 201/724 and `source_function`
`jet_ring_log_enter`/`jet_log_emit` (Log.rs:477, 573, 601). That replaces the user's
source identity with the implementation site (defect). No new Core export was added.

## Verdict

Criteria 1, 3 (bounds) and 4 are answered here, with defects. Criterion 2 (a finite
exported-trace job) and 5 (golden on all tiers) depend on the witness run; the observer
cost measurement is BLOCKED.

## Defects

1. `core.log` + `json.parse` in one program: the generated Rust fails to compile
   (`LogField: JetDebug` missing; `DataEvent::Int` given `JetInt`), default run exit 101.
2. `log.otlp_file` is not OTLP; the name and `log.jet:18` comment mislead.
3. Devtools telemetry replaces the caller's source identity with the `core.log` site.
4. Interpreter: `String.lines()` unsupported (E0956).
