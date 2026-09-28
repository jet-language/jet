# `prim-time` probe

## What I built

I built one Jet program over `core.time` with three layers: an exchange calendar trait with holiday-aware business-day arithmetic, a daily schedule over IANA zones, and package-owned astronomical UTC/TAI/TT/UT1 plus split Julian-date values. The program exercises the 2016 leap boundary, the 2024 New York DST gap and fold, nanosecond parsing, and both native and interpreter behavior at the signed nanosecond edge. The package authority grants `IO`, `Mem.Alloc`, and `Time`.

Files:

- `pkg/package.jet`
- `pkg/time_probe.jet` — working combined library and smoke program
- `pkg/api_probe.jet` — RFC3339 leap-second negative path
- `pkg/unknown_zone.jet` — IANA lookup negative path
- `pkg/highres_max.jet` — native/interpreter boundary mismatch

## What worked

- **Calendar trait and business dates:** `trait BusinessCalendar`, `ExchangeCalendar`, holidays, weekends, forward and backward counting compile and run. Proof: `jet run pkg/time_probe.jet --watch=off` -> `business_forward=2024-01-03; business_backward=2023-12-27; holiday_is_business=false; weekend_is_business=false`.
- **Recurring local schedule across DST:** `ZonedDateTime.add_period(period_days(1))` keeps 09:30 local while New York changes from `-05:00` to `-04:00` on March 10. The same proof run prints five occurrences with those offsets.
- **DST gap and fold choices:** `zoned_local` accepts `compatible`, `later`, and `earlier`; the proof run prints `gap_compatible=...03:30...-04:00`, `gap_later=...03:30...-04:00`, `overlap_earlier=...01:30...-04:00`, and `overlap_later=...01:30...-05:00`.
- **Zone database access:** `time.zone("America/New_York")`, transition lookup, `hours_in_day`, and `start_of_day` work. Unknown names fail with an actionable path: `Error: unknown IANA time zone: Mars/Base; set JET_TZDB_DIR or TZDIR to an IANA TZif database` (`E3002`, `unknown_zone.jet:3`). Core documentation names the same environment override and bundled/system TZif search (`docs/reference/core-library.md:2287-2290`).
- **Astronomical scale arithmetic as library code:** `AstroTime` carries a scale string and split seconds/nanoseconds; 28 package-owned leap rows implement UTC to TAI, TT (+32.184 s), and UT1 with a caller-supplied DUT1. Proof: the run prints `tai=1483228835s+0ns scale=TAI`, `tai_after=1483228837s+0ns`, `tt=1483228867s+184000000ns scale=TT`, `ut1=1483228799s+334000000ns scale=UT1`, and both UTC round trips return `2016-12-31T23:59:59Z`.
- **Julian-date conversion:** the package computes exact integer day plus nanosecond fraction without a floating conversion. Proof: `tai_jd=2457754+43235000000000ns`, `tt_jd=2457754+43267184000000ns`, `ut1_jd=2457754+43199334000000ns`.
- **Ordinary high-resolution instants:** nanosecond parsing and round trip work. Proof: `highres_ns=123456789 roundtrip=1711726200123456789`.
- **Static checking:** `jet check pkg/time_probe.jet` passes. It reports one `L0520` display-migration warning and eight `L2510` hidden-cost-in-loop warnings; no compile error occurs.

Research agrees on the boundary: the finance synthesis calls exchange/fiscal/banking calendars a separate mechanism (`_families/finance-business/synthesis.md:20,43-55`); quant research calls out DatetimeIndex, resampling, business offsets, and exchange-aware scheduled events (`quant-trading/report.md:4-7,25-29`; `claims.json:147-160`); astronomy research calls for UTC/TAI/UT1/TT, leap/Earth-rotation provenance, and long-epoch precision (`astronomy-astrophysics/claims.json:163-192`); space research calls mission scales and leap data first-class (`space-satellite/census.json:63-71`). The deleted calendar ballot also treats scalar time as shipped and exchange policy as separate (`ballots/D-M-CALENDARS1.json:7-11`).

## Gaps

### 1. `impossible` — no leap-second instant/label in core time

A library cannot represent a UTC `23:59:60` label or preserve it through `DateTime`. Proof: `jet run pkg/api_probe.jet --watch=off` -> `Error: time out of range: 23:59:60`; `Trail [E3002]`; origin `api_probe.jet:13`. This blocks faithful astronomy and mission timestamps in backend, data, and science packages. A package can keep a separate `AstroTime`/leap flag, but that value cannot interoperate as a core `DateTime`.

### 2. `defect` — native high-resolution boundary is wrong

The native tier mishandles the valid signed `Int` boundary for `from_unix_nanoseconds`. `jet run pkg/highres_max.jet --watch=off` prints `1677-09-21T00:12:43.145224196Z`; the same command with `run --interpret` prints `2262-04-11T23:47:16.854775807Z`. The fixture passes `9223372036854775807` at `highres_max.jet:3`. Core source stores seconds plus nanoseconds (`crates/jet-codegen/src/Prelude/Core/Time.rs:652-655`) and routes the constructor through an `i64` (`:677-681`). This blocks trustworthy boundary use in backend, data, and science code. Staying far from the boundary or using the interpreter/custom carrier is not a production fix.

### 3. `boilerplate` — no maintained astronomical time-data primitive

Core exposes UTC `DateTime`, IANA zones, `Duration`, `Instant`, and `Clock`, but no leap-second/Earth-rotation data source or checked UTC/TAI/TT/UT1 conversion (`docs/reference/core-library.md:2255-2303`). The working package repeats 28 dated offset rows (`time_probe.jet:97-128`), a lookup and inverse iteration (`:130-153`), TT constants (`:155-160`), and a DUT1 argument (`:163-169`). This is heavy common author work for science, backend event replay, and data pipelines. A package can ship a versioned table and IERS/DUT1 data itself, but must own freshness, provenance, and stale-data refusal.

### 4. `call-site` — no long-baseline high-resolution core carrier

`JetDateTime` is `i64` seconds plus nanoseconds, and the Unix-nanosecond constructor accepts an `i64`; this does not provide a two-part long-epoch representation with declared precision bounds. The ordinary 2024 nanosecond case works, but astronomy's two-part long-epoch requirement is documented in `astronomy-astrophysics/claims.json:179-192`. The package therefore exposes `AstroTime { seconds, nanoseconds, scale }` and `JulianDate { day, nanoseconds }` (`time_probe.jet:68-94,172-181`) and converts to core values only for ordinary-range instants. This adds a conversion boundary for data and science callers.

## Friction

- The exchange example needs roughly 19 lines for one trait, holiday record, and counting loop (`time_probe.jet:3-41`); this is ordinary package code, not a missing primitive.
- The astronomical workaround needs 28 leap rows plus four conversion functions and a three-pass inverse offset search (`time_probe.jet:97-169`). Users do not see this when calling a well-designed package, so it is recorded as author boilerplate in G3 rather than call-site ceremony.
- `jet check` emits eight `L2510 hidden_cost_in_loop` warnings for list traversal, exact counter updates, nanosecond normalization, and inverse conversion. The run still succeeds. The run/check path also emits one `L0520` warning that `RangeError` has no `Display`, mapped to a leap-table literal (`time_probe.jet:114`); this is diagnostic friction, not a time semantic gap.

## Defects

- **Native/AOT nanosecond mismatch:** exact repro and cross-tier outputs are recorded in G2 and `pkg/highres_max.jet`.
- **Generic native build ICE:** `jet build --verbose pkg/time_probe.jet` reports `internal compiler error: the generated Rust did not compile` and points to generated `build/time_probe.rs`; the same failure occurs for the minimal `pkg/business.jet` fixture before it was removed. This was not counted as a time-specific gap because the requested `jet run` path succeeds and the failure reproduces outside the astronomical code.

## Battery notes

Not applicable. This is a primitive probe; the brief explicitly excludes `batteries.json`.

## Verdict

Buildable with listed gaps fixed.

Business calendars, IANA zones, DST policy, recurring periods, and nanosecond values are expressible today.

Astronomical scales and Julian pairs are expressible as package code, but trusted data and scale identity are not core values.

Leap-second labels cannot cross the core `DateTime` boundary, and native nanosecond edges have a wrong-answer defect.

Fix leap-aware scale data, a long-range carrier, and the native boundary lowering before claiming mission-grade time.
