# Datetime Temporal-parity + default-tier hardening — 2026-08-28

Follow-up to `docs/audits/video-mine-five-languages-2026-08-28.md` under two owner
directives: (1) support everything in Temporal's datetime surface "and more"; (2) default-tier
bugs/gaps/breakage are "ABSOLUTELY AND UNEQUIVOCALLY UNACCEPTABLE" — strengthen testing so
these defect classes become structurally impossible, without bloat.

Evidence: complete 232-member Temporal API extraction from TC39 primary sources; exhaustive
both-tier probe of Jet's datetime surface; a 926-function registry census on the default tier;
a testing-architecture seam map. Artifacts in `~/.cache/jet-luna/mine-2026-08-28/artifacts/`
(`jet-dt-temporal.md`, `jet-dt-probe.md`, `jet-tier-census.{md,tsv}`, `jet-harness-map.md`).
Every load-bearing finding below was re-run first-hand.

## Verdict

Jet's datetime **model** already beats Temporal on four axes: the exact/calendar split
(`Duration` ns + `Period` y/m/d) is cleaner than Temporal's mixed 10-field Duration and erases
their balancing/serialization criticisms; the injectable seeded `Clock` beats untestable
`Temporal.Now`; the comptime-validated `DateTime{"…"}` literal has no Temporal analog; and DST
arithmetic is already **correct and tier-identical** (fall-back, spring-forward, ambiguous-time
probes). What's missing is **surface completeness** (comparison, rounding options, transition
queries, zoned round-trip) and **tier honesty** — five `core.time` constructors are E0956 on
the tier beginners use, and core time types render differently per tier. The hardening design
turns the two escape routes those defects used into checked contracts.

## Fresh defects (this pass, all first-hand-verified)

| # | Defect | Live contrast | Card |
|---|---|---|---|
| 1 | **Nested map literals corrupt data + ICE on both tiers**; map render emits malformed `[:x: 1, y: 2]` on every map, both tiers | default: inner entries leak into outer map, then ICE `jit map len: bad handle`; release: ICE 101, generated Rust E0308 (inner map typed as outer value) | **#2280 P0** |
| 2 | **Core time types render structurally on default, and release ignores `:Debug`/`:Pretty`** | `print(zone)`: default `Zone(name: Europe/London, offset: 3600)`, release `Europe/London`; `{dt:Debug}`: default correct, release prints bare form | **#2281 P0** |
| 3 | **No equality/ordering on time types**; E0312 Why-text leaks a literal `?` placeholder | `DateTime == DateTime` → E0312 "field `?` doesn't support `==`"; `<` → E0109 | **#2282 P1** |
| 4 | **`Duration.in(unit)` is registered but uncallable** — `in` is the reserved loop keyword | `a.in(.Nanoseconds)` → E0003 | **#2283 P2** |
| 5 | **Datetime default-tier E0956 rows** (used results): `from_unix_ms`, `new`, `parse`, `datetime`, `local_time`; census adds `core.compiler.{lex,parse,check,source_map}`, `core.testing.{corpus,test_suite}` | same programs succeed on `--release` | **#2252 → P0** |

Discarded after verification: the JETDT lane's "`Clock.wait` hangs on release" claim — re-run
completes in 14s; lane-timeout artifact, not a defect.

## Anything else in the mine? (owner question)

The 456-claim ledger was re-swept: 79 high-confidence topics had no round-one Jet cross-check.
Most are release minutiae (ports, GODEBUG, linker flags) or micro-details already absorbed.
Three substantive ones were closed this pass: **map merge** exists (`a.merge(b)`, right-biased —
and its probe found #2280), **`core.db`** exists (open/open_memory/policy…), **post-quantum
signatures** are a real crypto delta (Jet has sha2/3, blake3, ed25519, hkdf, argon2id — no
ML-DSA); logged on #2278's territory as an assessment item. Nothing else in the ledgers
warrants new probing.

## Temporal ↔ Jet gap matrix (family level; full 232-member inventory in artifacts)

| Temporal family | Jet today (probe-proven) | Gap → owner |
|---|---|---|
| Instant / epoch | `DateTime`, `now`, `from_unix_ms`, `to_timestamp`, `to_unix_ms`, ns getters | `from_unix_s/us/ns` + `to_unix_s/us/ns` (Temporal's own v2 gap — beat) → G7 #2284 |
| PlainDate | `LocalDate`: weekday, iso_weekday, day_of_year, iso_week, quarter, days_in_month, is_leap_year, add_days/months, diff_days, replace (clamps), format | ordering (#2282); ctor E0956 (#2252); week-date parse (beat) → G8 |
| PlainTime | `LocalTime` components + parse_time | `local_time` E0956 (#2252); ordering (#2282) |
| PlainDateTime | `DateTime` civil reads + `datetime(y,m,d,h,mi,s)` | ctor E0956 (#2252) |
| ZonedDateTime | `zoned`, `zoned_local`, `in_zone`, `offset_seconds`, `is_dst`, `add_duration` (absolute) vs `add_period` (calendar) — hybrid law matches; DST verified | disambiguation options (compatible/earlier/later/reject), offset policy, RFC 9557 zoned round-trip parse, `with_time`, `start_of_day`, `hours_in_day` → G5/G10 |
| getTimeZoneTransition | absent publicly; TZif transition data already internal (`Prelude Time.rs`) | `next_transition`/`previous_transition` → G6 |
| Duration | exact-ns `Duration` + ctors, `total_seconds`, `difference`; unit literals `2s`/`5min` | `total_in` (fractional), `round`, `abs`, `negated`, `sign`; `in` rename (#2283) → G4 |
| since/until options | `difference()` fixed-form only | largest/smallest unit, 9 rounding modes, increment → G4 |
| compare/equals | **nothing** (E0312/E0109) | #2282 |
| toString/from round-trip | `format_rfc3339` ↔ `parse_rfc3339` proven | zoned-annotation round-trip → G5 |
| Now | `time.now/now_utc/today` + **injectable `Clock`** | none — Jet ahead |
| Calendars/eras/monthCode | ISO-only by D-TIME-CALENDAR1 | non-goal (written verdict on #2284) |
| PlainYearMonth/MonthDay | absent | do/defer verdict on #2284 (Temporal's own MVC defers them too) |
| valueOf-throws defense | unnecessary — Jet has no implicit coercion | structural immunity, stated |
| Formatting | `format(pattern)` with real day names ("Thu"), no Intl dependency | unknown tokens (`VV XXX`) print literally — implement or reject; deterministic name data is the beat vs Temporal's Intl split → G9 |

Plan card: **#2284** (blockedBy #2281, #2282, #2283, #2252), gap families G1–G11 + non-goals,
authored with beginner/expert passes and per-family acceptance (example + golden on default AND
release, zero E0956 in family).

## Why nothing caught these — and the structural fix

Three escape routes, each now owned by a card:

1. **The registry lies by construction.** `CoreCallRecord::new` hardcodes
   `coverage: CoreCallCoverage::ALL` (`core_calls.rs:523`); validation checks declared bits,
   never consumers; the ambient dispatcher is hand-written arms ending `_ => None`. A
   reconciliation helper (`core_call_mismatch`) exists and is fed nothing. → **#2285 P0**:
   consumers export real arm sets, the existing helper reconciles claim↔arm both directions in
   the existing `core_call_table` suite, the unconditional `ALL` dies. A claimed-but-missing
   tier arm becomes unrepresentable in a green tree. Endgame assessment: one table generating
   row + arm.
2. **The tier gate is sound but blind.** `dev_corpus_gate` already byte-diffs AOT/default/
   interpreter for every example — but only examples that exist; its `tier_divergent` ledger is
   empty while `uuid.v4()` diverges live. → **#2286 P1**: one conformance program per public
   Core function (module_items = denominator, 926 rows), discovered by the same gate, with a
   suite_membership-style denominator law (stem or explicit counted exclusion, else red). Two
   census lessons baked in: probes must **consume** results (a bind-and-discard probe reported
   `uuid.v4` ok while the used call is E0956 today), and mechanical arg synthesis is
   insufficient (239× E0112, 129× E1803 artifacts) — generate once, hand-finish, check in.
3. **Admission and lowering never meet.** Sema admits Display/Debug impls via the trait
   registry; AOT bridges, the TIR evaluator, and the JIT each do independent lookups; no golden
   covers the two-impl shape. → **#2287 P1**: the closed shape×context matrix (~30 goldens)
   through the same gate, plus a ratchet asserting the matrix covers exactly what sema admits.

No new runners, no per-function hand tests: one existing suite extended, one existing gate fed
a generated corpus, one bounded golden matrix. That is the no-bloat shape of "structurally
impossible": the *registry* is the test's denominator, so surface and enforcement cannot drift
apart silently.

## Board state after this pass

| Card | P | What |
|---|---|---|
| #2280 | P0 | nested map literal corruption + ICE, map render defect |
| #2281 | P0 | core-type render/selector tier parity |
| #2285 | P0 | core-call registry totality |
| #2252 | P0 (raised) | E0956 arms; + datetime/census rows, owner directive logged |
| #2282 | P1 | time-type equality/ordering + E0312 placeholder leak |
| #2284 | P1 | Temporal-parity datetime plan (G1–G11), blockedBy 2281/2282/2283/2252 |
| #2286 | P1 | registry-driven Core conformance corpus |
| #2287 | P1 | impl-shape × interpolation matrix, blockedBy 2273/2281 |
| #2283 | P2 | `Duration.in` uncallable |

## Strongest unverified assumption

That `module_items.rs` is the complete public-Core denominator. If any user-reachable surface
is registered elsewhere (builtin receiver methods route through a separate table, as
`Duration`'s do), #2286's denominator law must union those tables — the card's first
implementation step should verify the union before trusting the count of 926.
