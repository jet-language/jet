# #3265 — Time, timezone and calendar semantics (evidence, 2026-09-29)

## Question

Do the shipped time owners prove these behaviours on the current binary: wall versus
monotonic time, ambiguous and nonexistent local times, parse round-trip, and the approved
`23:59:60` leap label? Are scheduler timers kept separate?

## Method

- Binary: snapshot14 through `~/.cache/jet-dev/safe-jet.sh`. Tiers: `run`,
  `run --interpret`, and `build` plus the binary (`~/.cache/jet-dev/scratch/Closer05/tiers.sh`).
- Goldens: `Examples/features/time/{datetime_accuracy_zones,datetime_accuracy_epoch_parse,time_core_witnesses}.jet`.
- Probes: `~/.cache/jet-dev/scratch/Closer05/t3265/probe.jet`, which adds `parse_rfc3339`
  and `isoformat` round trips, and `probe2.jet`, which covers `zone`, `parse_time`, `parse`
  and `instant` only.
- Source read: `Core/time/time.jet:14-22,124-129,249-270,348-358,371-382,458,522-548,691-692`,
  `Core/tasks/tasks.jet:107-113,154-162`, `crates/jet-codegen/src/Prelude/Core/Time.rs:1203-1226`.

## Evidence

| Run | Observed |
|---|---|
| zones golden, `run` | rc=101. `Stop [E3001] panic: zone` at line 24: `time.zone("Europe/London")` fails |
| zones golden, `--interpret` | rc=101. `E0956 core.builtin.string_lines() isn't supported by the current evaluator yet` (line 403) |
| zones golden, `build` | rc=101 ICE. rustc `E0119 conflicting implementations of trait __jet_Comparable for type JetDateTime` |
| epoch_parse golden, `run`/`--interpret`/`build` | rc=1. `E0102 DateTime has no method format_rfc3339` (×6), `to_unix_ms` (×2), `to_timestamp` |
| time_core_witnesses golden, all tiers | rc=1. E2402 TimeError→Err (×5); E0605 private `compare`/`equal`; E0102 missing `DateTime.to_unix_s`, `Zone.name/next_transition/previous_transition/start_of_day/hours_in_day`, `Period.years/total_in`; E0112 |
| probe.jet, `run` and `--interpret` | ICE: `checked TIR cannot lower to MIR … __jet_core_time::__jet_add_duration: missing checked MIR owner type` |
| probe2.jet, `run` | Prints `zone UTC: UTC offset=0`, then `Stop [E3010] Value doesn't fit in destination type` at `Core/time/time.jet:539` (`parse_int`, reached from `zone("+05:30")`) |
| probe2.jet, `--interpret` | Same line, then `Stop [E3001] panic: Value doesn't fit in destination type` at time.jet:539 |
| `time.elapsed(w)` / `use core.time.[elapsed]` | E1004 `core.time exposes elapsed, but not in this position`. The suggested fix produces the same error |

Source facts (read, not executed):

- `zone` (time.jet:124-129) accepts only UTC/GMT/Z and numeric offsets. A Jet owner with
  only fixed offsets can't produce an ambiguous or nonexistent local time. The IANA TZif
  reader lives only in the Rust Prelude (Time.rs:1203-1226). The time.jet:18-19 header
  ("zone names … fail closed") is therefore accurate for the Jet owner, not stale as the
  plan assumes.
- `parse_time` (:267-268) accepts second 60 only as `23:59:60`. It is unexercised because
  `parse_int` stops first.
- `Instant`/`Stopwatch` (:371-373, :691-692) read `host_unix_ns` (wall clock), and elapsed
  saturates on a backward step. There is no monotonic source.
- [INFERENCE] The :539 stop comes from `time_int_min()` (:458), which is written as the
  literal `-9223372036854775808`. The magnitude doesn't fit in `Int` before the negation.
  I tried the one-line alternative `Int.from_u8(d)` at :536 and it did not change the stop,
  so I reverted it. The root cause remains unconfirmed.

## Verdict

PARTIAL.

- Criterion 2 is met: scheduler timers are `core.tasks.after/interval/timeout` in
  `Core/tasks/tasks.jet:107-113,154-156`. Core/time has no timer rows.
- Criterion 3 is met: no new time model is proposed. A monotonic clock would be a new
  model and needs its own ballot.
- Criterion 1 is unmet: every exercise attempt stopped at a defect. The `zone`/`parse_int`
  stop, the `add_duration` ICE, and the missing IANA zones in the Jet owner mean there are
  no ambiguous or nonexistent local times to test.
- Criterion 4 is unmet: none of the three goldens compiles or runs on any tier.

## Follow-ups

- Port the IANA zone provider and the missing DateTime/Zone/Period methods to `Core/time/time.jet`, or route them to the host provider.
- Fix `parse_int` at time.jet:539 and the `add_duration` MIR ICE. Add `string_lines` support to the evaluator. Fix the duplicate AOT `__jet_Comparable` impl for `JetDateTime`.
- Make `core.time.elapsed` reachable.
- Ballot, if wanted: a monotonic clock source for `Instant`/`Stopwatch`.
