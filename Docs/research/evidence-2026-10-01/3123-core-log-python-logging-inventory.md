# core.log against Python logging: inventory and retained gaps (#3123, SCRIPT-F36)

Date: 2026-10-01. Source read: `Core/log/log.jet` (481 lines) on the working tree; runtime checked with snapshot172 (`jet run` and `jet run --interpret`). Finding: `Docs/research/mine-for-jet-2026-09-12.md#finding-script-f36`.

This page maps each Python `logging` feature to the existing Jet route, or names the exact capability that is missing. It proposes no API. Each missing capability is listed once under "Retained missing capabilities" so a later ballot can pick it up; nothing here is built or ratified.

## Three channels stay apart

| Channel | Jet route | Renderer and stream |
|---|---|---|
| Application events | `core.log` (`debug` … `fatal`, `*_fields`, `handle`) | The configured log sink (stderr JSON by default, or a `text`/`jsonl` file, or the OTLP file) |
| Program failures | Fallible values (`T E!`) reported through the error path, for example `.Err(error) -> print("failure report: {error}")` | The program's own output, or the shared report boundary for an unhandled entry error |
| Compiler diagnostics | `jet check` / `jet run` front end | The diagnostic renderer (`Error [Ennnn]` with What/Why/Fix), never the log sink |

`Examples/features/io/log_privacy.jet` proves the split: a runtime `IOError` prints through the error path, and neither the text nor the JSONL sink contains it (`failure report leaked=false`). Snapshot172 output matches `Examples/features/expected/io/log_privacy.out` on `jet run` and `jet run --interpret`.

## Feature map

| Python `logging` feature | Jet route today | Status |
|---|---|---|
| Levels DEBUG, INFO, WARNING, ERROR, CRITICAL | `log.debug`, `info`, `warn`, `error`, `critical`, plus `fatal`; `log.log(level, message)`; the spelling `warning` is accepted and canonicalized to `warn` (`canon_level`, log.jet:221-230) | Route exists |
| `logger.setLevel`, `isEnabledFor` | `log.set_level(level)`, `log.enabled(level)` (log.jet:100-103, 144-148); proven by `io/log` | Route exists |
| Custom numeric levels (`addLevelName`) | None. Only the six named levels exist; an unknown level name is dropped without a report (`emit`, log.jet:151-152) | Missing: M6 |
| Named logger hierarchy (`getLogger("a.b")`, propagation, per-logger levels) | None. One process-global logger; `group(event, prefix)` prefixes field keys, not logger names | Missing: M1 |
| `Handler` with its own level | `log.handler(level)`, `handler_format(handler, mode)`, `handle(handler, record)`: the threshold is applied before formatting (log.jet:360-383); proven by `io/log_privacy` | Route exists |
| Several handlers on one logger (fan-out) | None. `set_sink` replaces the single process-global sink; `handle` writes through that same sink | Missing: M2 |
| `StreamHandler(sys.stderr)` | Default sink; `set_sink("stderr", "")` | Route exists |
| `StreamHandler(sys.stdout)` | `set_sink("stdout", "")` returns without changing anything (log.jet:84) | Missing, and silent: M7 |
| `FileHandler` | `set_sink("text", path)` or `set_sink("jsonl", path)`; `setup("file=path")` | Route exists |
| `RotatingFileHandler`, `TimedRotatingFileHandler` | None | Missing: M3 |
| OTLP / exporter handlers | `log.otlp_file(path)` writes one OTLP-shaped JSON line per record; proven by `io/log_trace_export`. Remote exporters are third-party territory (owner policy 2026-10-01) | Route exists (file only) |
| `Filter` objects and callables | Level thresholds (`set_level`, handler minimum) and `sample_every(n)` only. A caller can guard with `log.enabled(level)` or its own `if` | Missing as a first-class filter: M4 |
| `Formatter` with a format string (`%`-style, `{}`-style, `datefmt`) | `log.formatter("text" or "json")` and `log.format(formatter, record)` (log.jet:356-372). Message text uses ordinary string interpolation. The record layout has two fixed shapes; no user layout or date format | Missing: M5 |
| Lazy `%`-args (formatting deferred until enabled) | `if log.enabled("debug") { … }` guards the cost explicitly | Route exists |
| `extra=` and `LoggerAdapter` context | Typed fields `field`/`int`/`float`/`bool`/`counter` on `*_fields`; `record`, `add`, `group`, `time`, `clear` build and extend a `LogRecord` (log.jet:352-481); `set_trace_id` and `span`/`enter`/`close` carry request context (proven by `io/log_structured`, `io/log_trace_export`) | Route exists |
| `exc_info`, `logger.exception`, `stack_info` | Failures are values, so the error is logged as a field (`log.field("error", "{err}")`). No backtrace is attached; emitting-site metadata is owned by #3182 | Partial: the site half is #3182; no other gap retained |
| `basicConfig`, `dictConfig` | `log.setup("level=warn,format=text,file=app.log,sample=10,trace_id=t")` (log.jet:112-138) | Route exists for `key=value` specs; see M7 for the bare form |
| `logging.disable`, `shutdown`/`flush` | `log.disable()`, `log.flush()` | Route exists |
| Secret-safe output | `log.redact(key)` builds a field that never receives the value, so it cannot leak by construction; both renderers print `[redacted]` (log.jet:71-73, 386-409). A secret passed as an ordinary `field(key, value)` is not redacted by key name | Route exists; no implicit key-name redaction (by design: no hidden rule) |

## Retained missing capabilities

Each item is a candidate for its own short ballot. None is built here, and none is a commitment.

- **M1 Named loggers.** A per-module or per-component logger name carried on every record, with a per-name level. Python: `getLogger(name)`, propagation.
- **M2 Several sinks at once.** One record delivered to more than one destination (for example stderr text plus a JSONL file). Python: several handlers on one logger.
- **M3 Rotating file sink.** Size- or time-based rotation with a retention count. Python: `RotatingFileHandler`, `TimedRotatingFileHandler`.
- **M4 Predicate filters.** A filter beyond level and sampling, applied by a logger or handler. Python: `Filter` objects and callables.
- **M5 Record layout.** A user-chosen text layout for a record (field order, timestamp format) beyond the fixed `text` and `json` shapes. Python: `Formatter(fmt, datefmt, style)`.
- **M6 Custom levels.** A level outside the six named ones, or a report when an unknown level name is used. Today `log.log("verbose", …)` is dropped without a report.
- **M7 Silent sink and setup fallbacks (defect, not a feature gap).** `set_sink("stdout", …)`, an unknown sink kind, or a `text`/`jsonl` sink with an empty path return without changing anything and without a report (log.jet:84-86). `log.setup("text")`, the bare form that `Docs/spec/syntax-decisions.md:3925` (D-LOGFMT1) and `Examples/features/io/log_human.jet:2` describe, matches no `key=value` branch, so it does nothing: on snapshot172, `jet run` and `jet run --interpret` of `io/log_human` print JSON lines on stderr, not text. The golden checks stdout only, so it still passes. This needs a defect fix in `Core/log/log.jet` (accept the bare format or report the unknown setting), not a ballot.
