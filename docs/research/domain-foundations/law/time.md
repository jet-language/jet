# time

## Ratified

- **D-TIMERES1=A** — `Duration` is a signed whole-nanosecond `i64` carrier, about 292 years; `DurationUnit` includes Nano/Micro and constructors include nanoseconds/microseconds, with no stored-format rewrite. — `tower` (`D-TIMERES1` outcome A)
- **D-SHAPE-DURATION1=A / D-SHAPE-DURATIONCONVERT1=A** — runtime durations are type-owned checked constructors; nonfinite/out-of-range values fail with `RangeError`, and the former free `core.time` constructor/readers do not return as aliases. — `docs/spec/syntax-decisions.md:4013-4023`
- **D-TYPE2-TIME1=A** — `Duration` is the delta quantity of the canonical Time unit family and `Instant` is the matching point quantity; timeout/schedule values use that rail. — `docs/spec/syntax-decisions.md:6852-6868`; `tower` (`D-TYPE2-TIME1` outcome A)
- **D-TIME-IN1=C** — the reserved word `in` is accepted as a postfix member after `.`, so `duration.in(.Seconds)` is the current spelling; bare `in` remains the source-loop keyword. — `docs/spec/syntax-decisions.md:8312-8322`; `tower` (`D-TIME-IN1` outcome C)
- **D-TIME-CALENDAR1=A** — time has distinct `Instant`, `DateTime`, `LocalDate`, `LocalTime`, `Duration`, and `Zone` types, with beginner constructors and expert timezone, monotonic-clock, fake-clock, and scheduler control. — `docs/spec/syntax-decisions.md:3581-3584`
- **D-FREESTAND-TIME1=A** — pure calendar/Duration code has no provider; deterministic code receives an injected Clock; wall, monotonic, zone-data, and sleep services stay distinct declared facts. — `tower` (`D-FREESTAND-TIME1` outcome A)
- **D-BOUND-HEAD1=A** — `DateTime{"…"}` is a checked typed head; sema validates its complete literal skeleton and emits E0155 for an invalid head, while runtime strings retain parse/from constructors. — `docs/spec/syntax-decisions.md:1311-1315,7953-7955`; `tower` (`D-BOUND-HEAD1` outcome A)
- **D-VERDICT-2039-1** — week is the only new Time literal suffix: a fixed seven-day scale on the nanosecond carrier; month/year/decade/century/millennium stay in Period/date APIs. — `tower` (`D-VERDICT-2039-1`)
- **D-TIMETRAVEL2=B** — jump ships with the recorder; reverse step is `back N` over deterministic re-execution and unblocks on byte-identical locals snapshots for every fixture on both adapters. — `tower` (`D-TIMETRAVEL2` outcome B)

## Shipped

- The spec defines duration constructors, checked conversion, truncating whole-unit `in`, fractional `total_in`, nine rounding modes, exact carrier operations, and `ns/us/ms/s/min/h/d` literals. — `docs/spec/spec.md:61-71`
- The core reference exposes Unix millisecond/second/microsecond/nanosecond conversion, RFC3339/ISO-week/zoned parsing, civil and zoned types, monotonic `instant()`, IANA `zone()`, sleep, Stopwatch, seeded `clock(seed)`, `Clock.system()`, and Duration constructors. — `docs/reference/core-library.md:2255-2278`
- `DateTime` stores `secs: i64` plus normalized `nanos: u32` relative to the UTC Unix epoch; `from_unix_nanoseconds` uses Euclidean division/remainder so pre-epoch values normalize correctly. — `crates/jet-codegen/src/Prelude/Core/Time.rs:651-681`
- `DateTime`/`ZonedDateTime` and `LocalDate`/`LocalTime` keep elapsed and calendar planes distinct; DST uses explicit disambiguation and `time.zone` reads IANA TZif data from configured/bundled/system paths. — `docs/reference/core-library.md:2280-2290`
- Date/time arithmetic, rounding, formatting, RFC3339/RFC9557 output, ISO week operations, zone transitions, and checked Unix precision conversions are documented; `to_unix_us/ns` return E2704 rather than silently changing an out-of-range value. — `docs/reference/core-library.md:2292-2306`
- Leap seconds are not represented as a distinct instant; RFC3339 parsing rejects `:60`. A pure `fn … -[]>` cannot read ambient `time.now()` or construct `Clock.system()` (E3403); a seeded Clock is the deterministic path. — `docs/reference/core-library.md:2308-2329`
- `LEX_TEST_EPOCH` pins `time.now()` for lexer/test output; normal programs use the real wall clock. — `docs/reference/core-library.md:2318-2321`
- Duration constructors and unit conversions are registered in the syntax surface, with nanosecond/microsecond through hour constructors and `in` as the member method. — `crates/jet-foundation/src/Syntax/core_surface.rs:859-888`
- The time probe verified nanosecond duration arithmetic, negative normalization, date/zone parsing, DST gap/overlap behavior, deterministic clock/timer behavior, monotonic elapsed values, and leap-second rejection. — `~/.cache/jet-luna/dx3/prim-time/probe.md:8-31`

## Undecided

- Should the open D-TIMEDEPTH1 specification contract be completed for leap-second policy, calendar/elapsed conversions, zone-data evolution, and all clock/provider effects without changing the distinct type planes?
- Should Jet expose a richer time-scale model for TAI, TT, UT1, UTC/leap handling, and explicit conversion uncertainty, or is UTC plus monotonic time sufficient?
- Should the standard library expose timezone database version/provenance and a stable update/compatibility policy for IANA rule changes?
- Should `Instant` gain precision-preserving arithmetic/formatting beyond the existing elapsed Duration path, and what observable clock resolution should be promised?
- Should scheduler deadlines, timeout cancellation, and sleep report missed-deadline/clock-jump facts through a typed result rather than only effects/errors?
- Should recording/replay provide an implemented clock-input adapter so captured ambient time can replay through `jet debug --replay`, rather than failing static time access E0956?
- Should `total_in` and `in` be reconciled in the documented/member API so fractional totals and truncating whole-unit reads cannot be confused?

## Conflicts

- D-TIMERES1 and D-TYPE2-TIME1 require one signed nanosecond `i64` carrier and the canonical Time family. A ballot must not propose a different carrier (milliseconds, microseconds, or a timespec-shaped replacement) or a second duration quantity; the millisecond/microsecond constructors remain ratified conversions. — `tower` (`D-TIMERES1`, `D-TYPE2-TIME1`)
- D-TIME-IN1=C is the selected outcome, not its earlier recommendation: `.in(.Unit)` is valid. Renaming it to `total` or `in_unit` would reopen a ratified ruling. — `docs/spec/syntax-decisions.md:8312-8322`; `tower` (`D-TIME-IN1` outcome C)
- `Duration` and `Period` are intentionally different: elapsed 24 hours and one calendar day can differ across DST. A generic month/year Duration or silent calendar conversion conflicts with the current law. — `docs/spec/spec.md:72-89`; `docs/reference/core-library.md:2280-2285`
- D-TIMEDEPTH1 is still an open spec-only record in Tower. Its appearance beside D-TIME-CALENDAR1 in the spec does not make every deeper time-scale promise a ratified ballot outcome. — `tower` (`D-TIMEDEPTH1`, open/spec-only)
- Leap seconds are an explicit current behavior (`:60` rejected), not an unimplemented parser omission; a proposal to silently accept them must decide the instant/scale semantics first. — `docs/reference/core-library.md:2315-2316`
- `Clock.system()` is explicit and carries the Time effect; pure code cannot smuggle ambient time through a global provider. The probe's E0956 captured-time replay gap is a tool/replay bridge question, not permission to weaken E3403. — `docs/reference/core-library.md:2322-2329`; `~/.cache/jet-luna/dx3/prim-time/probe.md:37-40`
- D-TIMETRAVEL2's reverse-step fixture gate and D-RUN-RECORD1's record surface do not mean reverse stepping or ambient clock replay is already shipped. — `tower` (`D-TIMETRAVEL2` outcome B); `docs/spec/syntax-decisions.md:7920-7925`
- DateTime is represented as UTC seconds plus nanoseconds in the current implementation; the documented conversions and checked errors must remain consistent with that normalized representation. — `crates/jet-codegen/src/Prelude/Core/Time.rs:651-681`
