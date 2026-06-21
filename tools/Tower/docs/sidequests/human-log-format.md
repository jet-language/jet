# Plan: Human-readable log format for `jet.log` (D-LOGFMT1)

**Status: plan — awaiting owner decision D-LOGFMT1.**

Unblocks: **Amara** (automation — readable console/file logs without
hand-building strings).

---

## Goal

`jet.log` emits JSON lines only (verified `54_log.jet` outputs JSON). For a script
whose log a human reads in a terminal, JSON is noise — the user falls back to
manual string-building. Add a human-readable formatter (level + timestamp +
message + key=value fields) selectable at logger setup, so the same `log.info(…)`
calls render either way.

Verified: `54_log.jet` outputs JSON lines; the structured logger has no text
formatter (`grep -i "log.*format\|text format" Source/` → nothing for a human
mode).

## Pipeline touch points

- **stdlib** (`jet.log`): a formatter abstraction with at least `json` and a
  human/`text` formatter; a way to select it at logger construction (and possibly
  auto-select: text on a TTY, JSON when piped).
- **sema**: register any new constructor/option.
- **codegen**: none beyond the stdlib helper.

## Invariants in play

- **I8** one logger, two output formats — not two loggers.
- **Beginner-experience**: the *default* should be the friendly one for a beginner
  running locally; structured JSON is the opt-in for production/aggregation (or
  auto by TTY detection).
- **I5** example showing the same calls in both formats.

## Open questions (need owner decision — D-LOGFMT1)

1. **Default format** — human/text by default (beginner-friendly) with JSON opt-in,
   or keep JSON default? Or **auto**: text when stderr is a TTY, JSON when piped
   (the most magic, matches modern loggers)?
2. **Selection surface** — `log.setup(format: text)`, a `Logger` constructor
   arg, an env var (`JET_LOG_FORMAT`), or all three with a precedence order?
3. **Text layout** — the exact human line format is product copy: level coloring,
   timestamp format (or omit when interactive), how structured key=value fields
   render inline. Define it like a diagnostics format.
4. **Color** — colorize levels on a TTY? (shares the color question with D-TERM1).

## Test plan

1. `examples/features/log_human.jet` — same `log.info`/`log.error` calls, render
   in text format; golden output (I5).
2. JSON format still golden-tested (regression on `54_log.jet`).
3. (If auto-by-TTY) a test forcing each mode via the explicit selector.
4. Structured-field rendering snapshot (key=value inline) — product copy.
