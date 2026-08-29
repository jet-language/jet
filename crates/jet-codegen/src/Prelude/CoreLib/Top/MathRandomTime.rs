// D-TIME-INSTANT-SPLIT1=A: these trait impls stay beside the Core-owned
// carrier. The shared carrier itself lives in Core/TimeInstant.rs so JIT and
// AOT use the same implementation.
impl JetShow for JetInstant {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDisplay for JetInstant {
    fn jet_display(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDebug for JetInstant {
    fn jet_debug(&self) -> String {
        self.to_string_fmt()
    }
}

fn jet_deadline_exceeded(wait_kind: &str) -> ! {
    let rendered = jet_std::jet_task_deadline(wait_kind).render();
    jet_std::jet_task_deadline_mark_pending();
    if jet_interrupt_handler_should_unwind()
        || jet_scheduler_wait_boundary_should_unwind()
        || jet_typed_deadline_boundary_should_unwind()
    {
        std::panic::panic_any(JetDeadlineUnwind { rendered });
    }
    jet_runtime_diagnostic(rendered);
}

fn jet_std_time_start() -> jet_std::Stopwatch {
    jet_std::Stopwatch {
        start: std::time::Instant::now(),
    }
}

// ── D-DET1: deterministic injected Clock / Rng capabilities ───────────────────
// Built from a caller-supplied seed (a pure value), so a `#Pure fn` may read
// time/randomness THROUGH the handle and stay reproducible. No wall-clock or
// OS-RNG read; std-only (no external crate, I6).
fn jet_std_clock_new(seed: i64) -> jet_std::Clock {
    jet_std::Clock::manual(seed)
}
fn jet_std_clock_system() -> jet_std::Clock {
    jet_std::Clock::system()
}
fn jet_clock_now(c: &jet_std::Clock) -> i64 {
    c.now()
}
fn jet_clock_tick(c: &mut jet_std::Clock, ms: i64) -> i64 {
    let now = c.now().saturating_add(ms);
    c.set(now);
    c.now()
}
// D-DET-CAPAPI: `clock.advance(to_ms)` sets the clock to an ABSOLUTE instant;
// `clock.wait(d)` advances by a `Duration` (relative). Both return the new value.
fn jet_clock_advance(c: &mut jet_std::Clock, to_ms: i64) -> i64 {
    c.set(to_ms);
    c.now()
}
fn jet_clock_wait(c: &mut jet_std::Clock, d: &jet_std::Duration) -> i64 {
    let now = c.now().saturating_add(d.as_millis());
    c.set(now);
    c.now()
}
fn jet_std_rng_new(seed: i64) -> jet_std::Rng {
    jet_std::Rng { state: seed as u64 }
}
// SplitMix64 step — a small, well-distributed deterministic PRNG (public domain).
fn jet_det_rng_next(r: &mut jet_std::Rng) -> u64 {
    jet_seeded_rng_next(&mut r.state)
}
fn jet_rng_int(r: &mut jet_std::Rng, lo: i64, hi: i64) -> i64 {
    let lo = jet_std::jet_int_to_i64(lo).unwrap_or_else(|| {
        jet_runtime_stop("E1003", file!(), line!(), jet_c_int_range_message())
    });
    let hi = jet_std::jet_int_to_i64(hi).unwrap_or_else(|| {
        jet_runtime_stop("E1003", file!(), line!(), jet_c_int_range_message())
    });
    match jet_seeded_rng_int_checked(&mut r.state, lo, hi) {
        Ok(value) => jet_std::jet_int_from_i64(value),
        Err(message) => jet_runtime_stop("E3010", "", 0, message),
    }
}
fn jet_rng_float(r: &mut jet_std::Rng) -> f64 {
    jet_seeded_rng_float(&mut r.state)
}
fn jet_rng_float_open(r: &mut jet_std::Rng) -> f64 {
    jet_seeded_rng_float_open(&mut r.state)
}
fn jet_rng_float_range(r: &mut jet_std::Rng, low: f64, high: f64) -> f64 {
    jet_seeded_rng_float_range(&mut r.state, low, high)
}
// D-DET-CAPAPI: the widened deterministic draws — coin, uniform choice, in-place
// Fisher–Yates shuffle. Each advances the SplitMix64 stream, so they are
// reproducible from the seed and mirror the ambient `random.*` set.
fn jet_rng_bool(r: &mut jet_std::Rng) -> bool {
    jet_seeded_rng_bool(&mut r.state)
}
fn jet_rng_bool_p(r: &mut jet_std::Rng, p: f64) -> bool {
    jet_seeded_rng_bool_p(&mut r.state, p)
}
fn jet_rng_normal(r: &mut jet_std::Rng, mean: f64, stddev: f64) -> f64 {
    jet_seeded_rng_normal(&mut r.state, mean, stddev)
}
fn jet_rng_exponential(r: &mut jet_std::Rng, lambda: f64) -> f64 {
    jet_seeded_rng_exponential(&mut r.state, lambda)
}
fn jet_rng_bytes(r: &mut jet_std::Rng, n: i64) -> Vec<u8> {
    jet_seeded_rng_bytes(&mut r.state, n)
}
fn jet_rng_split(r: &mut jet_std::Rng) -> jet_std::Rng {
    jet_std::Rng {
        state: jet_seeded_rng_split(&mut r.state),
    }
}
fn jet_rng_pick<T: Clone>(r: &mut jet_std::Rng, xs: &Vec<T>) -> JetOutcome<T, JetAbsent> {
    jet_seeded_rng_pick(&mut r.state, xs).ok_or(JetAbsent)
}
fn jet_rng_weighted_pick<T: Clone>(
    r: &mut jet_std::Rng,
    xs: &Vec<T>,
    weights: &Vec<f64>,
) -> JetOutcome<T, JetAbsent> {
    jet_seeded_rng_weighted_pick(&mut r.state, xs, weights).ok_or(JetAbsent)
}
fn jet_rng_sample<T: Clone>(r: &mut jet_std::Rng, xs: &Vec<T>, k: i64) -> Vec<T> {
    jet_seeded_rng_sample(&mut r.state, xs, k)
}
fn jet_rng_shuffle<T>(r: &mut jet_std::Rng, xs: &mut Vec<T>) {
    jet_seeded_rng_shuffle(&mut r.state, xs);
}
// D-TIMERES1=A / D-SHAPE-DURATIONCONVERT1=A: one checked nanosecond unit
// model for every runtime constructor and whole-unit read.
fn jet_duration_from_int(
    n: i64,
    unit: jet_std::DurationUnit,
) -> Result<jet_std::Duration, jet_std::RangeError> {
    jet_duration_kernel_from_int(n, unit.nanoseconds())
        .map(|ns| jet_std::Duration { ns })
        .ok_or_else(|| jet_std::RangeError {
            reason: jet_duration_kernel_int_error_reason().to_string(),
        })
}
fn jet_duration_from_float(
    n: f64,
    unit: jet_std::DurationUnit,
) -> Result<jet_std::Duration, jet_std::RangeError> {
    jet_duration_kernel_from_float(n, unit.nanoseconds())
        .map(|ns| jet_std::Duration { ns })
        .ok_or_else(|| jet_std::RangeError {
            reason: jet_duration_kernel_float_error_reason().to_string(),
        })
}
fn jet_duration_in(
    d: &jet_std::Duration,
    unit: &jet_std::DurationUnit,
) -> Result<i64, jet_std::RangeError> {
    Ok(jet_duration_kernel_in(d.ns, unit.nanoseconds()))
}
fn jet_duration_ms_value(d: &jet_std::Duration) -> i64 {
    d.as_millis()
}
fn jet_duration_ns_value(d: &jet_std::Duration) -> i64 {
    d.ns
}
fn jet_duration_is_zero(d: &jet_std::Duration) -> bool {
    jet_duration_kernel_is_zero(d.ns)
}
fn jet_duration_total_seconds(d: &jet_std::Duration) -> i64 {
    jet_duration_kernel_total_seconds(d.ns)
}
fn jet_duration_seconds_value(d: &jet_std::Duration) -> f64 {
    jet_duration_kernel_seconds_value(d.ns)
}
fn jet_duration_difference(a: &jet_std::Duration, b: &jet_std::Duration) -> jet_std::Duration {
    jet_std::Duration {
        ns: jet_duration_kernel_difference(a.ns, b.ns),
    }
}
fn jet_duration_scale(d: &jet_std::Duration, factor: &i64) -> jet_std::Duration {
    jet_std::Duration {
        ns: jet_duration_kernel_scale(d.ns, *factor)
            .unwrap_or_else(|| panic!("{}", jet_duration_kernel_scale_error_reason())),
    }
}
fn jet_duration_divide(d: &jet_std::Duration, factor: &i64) -> jet_std::Duration {
    jet_std::Duration {
        ns: jet_duration_kernel_divide(d.ns, *factor)
            .unwrap_or_else(|| panic!("{}", jet_duration_kernel_scale_error_reason())),
    }
}

impl std::ops::Add for jet_std::Duration {
    type Output = jet_std::Duration;

    fn add(self, rhs: Self) -> Self::Output {
        jet_std::Duration {
            ns: jet_duration_kernel_add(self.ns, rhs.ns),
        }
    }
}

impl std::ops::Sub for jet_std::Duration {
    type Output = jet_std::Duration;

    fn sub(self, rhs: Self) -> Self::Output {
        jet_std::Duration {
            ns: jet_duration_kernel_sub(self.ns, rhs.ns),
        }
    }
}

impl std::ops::Add<jet_std::Duration> for JetInstant {
    type Output = JetInstant;

    fn add(self, rhs: jet_std::Duration) -> Self::Output {
        self.plus_duration_ns(rhs.ns)
    }
}

impl std::ops::Sub<jet_std::Duration> for JetInstant {
    type Output = JetInstant;

    fn sub(self, rhs: jet_std::Duration) -> Self::Output {
        self.minus_duration_ns(rhs.ns)
    }
}

impl std::ops::Sub for JetInstant {
    type Output = jet_std::Duration;

    fn sub(self, rhs: Self) -> Self::Output {
        jet_std::Duration {
            ns: self.difference_ns(&rhs),
        }
    }
}

impl std::ops::Add<JetInstant> for jet_std::Duration {
    type Output = JetInstant;

    fn add(self, rhs: JetInstant) -> Self::Output {
        rhs.plus_duration_ns(self.ns)
    }
}

fn jet_time_ordering(ordering: std::cmp::Ordering) -> __jet_Ordering {
    match ordering {
        std::cmp::Ordering::Less => __jet_Ordering::__jet_Less,
        std::cmp::Ordering::Equal => __jet_Ordering::__jet_Equal,
        std::cmp::Ordering::Greater => __jet_Ordering::__jet_Greater,
    }
}

impl __jet_Equatable for JetDate {
    fn equal(&self, rhs: &Self) -> bool {
        self == rhs
    }
}

impl __jet_Comparable for JetDate {
    fn compare(&self, rhs: &Self) -> __jet_Ordering {
        jet_time_ordering(self.cmp(rhs))
    }
}

impl __jet_Equatable for JetLocalTime {
    fn equal(&self, rhs: &Self) -> bool {
        self == rhs
    }
}

impl __jet_Comparable for JetLocalTime {
    fn compare(&self, rhs: &Self) -> __jet_Ordering {
        jet_time_ordering(self.cmp(rhs))
    }
}

impl __jet_Equatable for JetDateTime {
    fn equal(&self, rhs: &Self) -> bool {
        self == rhs
    }
}

impl __jet_Comparable for JetDateTime {
    fn compare(&self, rhs: &Self) -> __jet_Ordering {
        jet_time_ordering(self.cmp(rhs))
    }
}

impl __jet_Equatable for jet_std::Duration {
    fn equal(&self, rhs: &Self) -> bool {
        self.ns == rhs.ns
    }
}

impl __jet_Comparable for jet_std::Duration {
    fn compare(&self, rhs: &Self) -> __jet_Ordering {
        jet_time_ordering(self.ns.cmp(&rhs.ns))
    }
}

impl __jet_Equatable for JetInstant {
    fn equal(&self, rhs: &Self) -> bool {
        self == rhs
    }
}

impl __jet_Comparable for JetInstant {
    fn compare(&self, rhs: &Self) -> __jet_Ordering {
        jet_time_ordering(self.cmp(rhs))
    }
}

// ZonedDateTime `==` is instant plus zone identity. Temporal keeps that
// value equality distinct from its separate `equals` distinction.
impl __jet_Equatable for JetZonedDateTime {
    fn equal(&self, rhs: &Self) -> bool {
        self == rhs
    }
}

impl __jet_Comparable for JetZonedDateTime {
    fn compare(&self, rhs: &Self) -> __jet_Ordering {
        jet_time_ordering(self.instant.cmp(&rhs.instant))
    }
}

fn jet_time_instant_now() -> JetInstant {
    JetInstant::now()
}
fn jet_instant_elapsed_millis(i: &JetInstant) -> i64 {
    i.elapsed_millis()
}
fn jet_instant_elapsed(i: &JetInstant) -> jet_std::Duration {
    jet_std::Duration {
        ns: i.elapsed_nanos(),
    }
}
fn jet_time_now_utc() -> JetDateTime {
    JetDateTime::now()
}
fn jet_time_today() -> JetDate {
    JetDate::today_utc()
}
fn jet_time_datetime(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) -> JetDateTime {
    JetDateTime::from_parts(year, month, day, hour, minute, second, 0)
}
fn jet_time_time(hour: i64, minute: i64, second: i64) -> JetLocalTime {
    JetLocalTime::new(hour, minute, second)
}
fn jet_time_days_in_month(year: i64, month: i64) -> i64 {
    JetDate::days_in_month_of(year, month.clamp(1, 12))
}
fn jet_time_is_leap_year(year: i64) -> bool {
    JetDate::is_leap(year)
}
fn jet_time_period(years: i64, months: i64, days: i64) -> JetPeriod {
    JetPeriod::new(years, months, days)
}
fn jet_time_period_days(days: i64) -> JetPeriod {
    JetPeriod::days(days)
}
fn jet_time_period_months(months: i64) -> JetPeriod {
    JetPeriod::months(months)
}
fn jet_time_period_years(years: i64) -> JetPeriod {
    JetPeriod::years(years)
}
fn jet_time_zone_named(name: &String) -> Result<JetZone, String> {
    JetZone::named(name)
}
fn jet_time_zone_utc() -> JetZone {
    JetZone::utc()
}
fn jet_time_zoned(dt: &JetDateTime, zone: &JetZone) -> JetZonedDateTime {
    dt.in_zone(zone)
}
fn jet_time_zoned_local(date: &JetDate, time: &JetLocalTime, zone: &JetZone) -> JetZonedDateTime {
    JetZonedDateTime::from_local(date, time, zone)
}
fn jet_datetime_plus_duration(dt: &JetDateTime, d: &crate::jet_std::Duration) -> JetDateTime {
    dt.plus_duration_ns(d.ns)
}
fn jet_datetime_difference(a: &JetDateTime, b: &JetDateTime) -> crate::jet_std::Duration {
    crate::jet_std::Duration {
        ns: a.difference_ns(b),
    }
}
fn jet_zoned_add_duration(z: &JetZonedDateTime, d: &crate::jet_std::Duration) -> JetZonedDateTime {
    z.add_duration_ns(d.ns)
}

fn jet_url_parse(s: &String) -> Result<crate::jet_std::JetURL, String> {
    crate::jet_std::JetURL::parse(s)
}

/// D-BOUND-HEAD1=A: DateTime heads are complete RFC3339 values. Sema has
/// already rejected holes that could make the literal invalid.
fn jet_typed_datetime_literal(literals: &[&str], holes: Vec<String>) -> JetDateTime {
    let text = jet_typed_datetime_interpolate(literals, &holes);
    match JetDateTime::parse_rfc3339(&text) {
        Ok(value) => value,
        Err(error) => unreachable!("sema accepted an invalid DateTime typed head: {error}"),
    }
}

fn jet_url_from_parts(
    scheme: &String,
    host: &String,
    path: &String,
    query: &Vec<Vec<String>>,
    fragment: &String,
) -> Result<crate::jet_std::JetURL, String> {
    crate::jet_std::JetURL::from_parts(scheme, host, path, query, fragment)
}
fn jet_url_file(path: &String) -> crate::jet_std::JetURL {
    crate::jet_std::JetURL::file(path)
}
fn jet_url_data(mime: &crate::jet_std::JetMIME, text: &String) -> crate::jet_std::JetURL {
    crate::jet_std::JetURL::data(mime, text)
}
fn jet_url_query(pairs: &Vec<Vec<String>>) -> String {
    let rows: Vec<(String, String)> = pairs
        .iter()
        .filter(|r| !r.is_empty())
        .map(|r| {
            (
                r.get(0).cloned().unwrap_or_default(),
                r.get(1).cloned().unwrap_or_default(),
            )
        })
        .collect();
    rows.iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                crate::jet_std::jet_url_percent_encode(k, false),
                crate::jet_std::jet_url_percent_encode(v, false)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}
fn jet_url_percent_encode_component(s: &String) -> String {
    crate::jet_std::jet_url_percent_encode(s, false)
}
fn jet_url_percent_decode_component(s: &String) -> Result<String, String> {
    crate::jet_std::jet_url_percent_decode_str(s)
}
fn jet_mime_parse(s: &String) -> Result<crate::jet_std::JetMIME, String> {
    crate::jet_std::JetMIME::parse(s)
}
fn jet_mime_from_extension(ext: &String) -> Option<String> {
    crate::jet_std::jet_mime_from_extension(ext).map(|s| s.to_string())
}
fn jet_mime_extension(mime: &String) -> Option<String> {
    crate::jet_std::jet_extension_from_mime(mime).map(|s| s.to_string())
}

// D-DECIMAL1 / D-NUMTYPE1: precise numeric constructors and methods.
fn jet_decimal_from_str(s: &String) -> jet_std::JetDecimal {
    jet_std::JetDecimal::from_str(s)
        .unwrap_or_else(|_| jet_panic("", 0, "invalid Decimal string"))
}
// D-NUMTYPE1=A: exact ratios. Every answer is optional, because a zero bottom
// has no value and a product can leave the range.
fn jet_fraction_new(numerator: i64, denominator: i64) -> Option<jet_std::JetFraction> {
    jet_std::JetFraction::new(numerator, denominator)
}
fn jet_fraction_from_parts(numerator: i64, denominator: i64) -> jet_std::JetFraction {
    jet_std::JetFraction::new(numerator, denominator)
        .unwrap_or_else(|| jet_panic("", 0, "invalid exact quotient"))
}
fn jet_fraction_add(a: &jet_std::JetFraction, b: &jet_std::JetFraction) -> jet_std::JetFraction {
    a.add(b).expect("this sum of ratios overflows the value type")
}
fn jet_fraction_sub(a: &jet_std::JetFraction, b: &jet_std::JetFraction) -> jet_std::JetFraction {
    a.sub(b).expect("this difference of ratios overflows the value type")
}
fn jet_fraction_mul(a: &jet_std::JetFraction, b: &jet_std::JetFraction) -> jet_std::JetFraction {
    a.mul(b).expect("this product of ratios overflows the value type")
}
fn jet_fraction_div(a: &jet_std::JetFraction, b: &jet_std::JetFraction) -> jet_std::JetFraction {
    a.div(b).expect("divided by zero")
}
fn jet_fraction_from_int(value: i64) -> jet_std::JetFraction {
    jet_std::JetFraction::from_int(value).expect("exact Int does not fit Fraction")
}
fn jet_fraction_from_float(value: f64) -> jet_std::JetFraction {
    jet_std::JetFraction::from_float(value).expect("Float has no word-sized exact Fraction")
}
fn jet_fraction_from_decimal(value: jet_std::JetDecimal) -> jet_std::JetFraction {
    jet_std::JetFraction::from_decimal(&value).expect("Decimal does not fit Fraction")
}
fn jet_fraction_equal(a: &jet_std::JetFraction, b: &jet_std::JetFraction) -> bool {
    a == b
}
fn jet_fraction_numerator(a: &jet_std::JetFraction) -> i64 {
    a.numerator_value()
}
fn jet_fraction_denominator(a: &jet_std::JetFraction) -> i64 {
    a.denominator_value()
}
fn jet_fraction_to_string(a: &jet_std::JetFraction) -> String {
    a.to_string_rep()
}
fn jet_fraction_to_float(a: &jet_std::JetFraction) -> f64 {
    a.to_float()
}
fn jet_fraction_is_zero(a: &jet_std::JetFraction) -> bool {
    a.is_zero()
}
fn jet_fraction_to_int(a: &jet_std::JetFraction) -> i64 {
    a.to_int_exact().expect("Fraction is not an integer")
}
fn jet_fraction_to_decimal(a: &jet_std::JetFraction) -> jet_std::JetDecimal {
    a.to_decimal().expect("Fraction has a repeating expansion")
}
fn jet_decimal_from_int(value: i64) -> jet_std::JetDecimal {
    jet_std::JetDecimal::from_int(value).expect("invalid exact Int")
}
fn jet_decimal_from_float(value: f64) -> jet_std::JetDecimal {
    jet_std::JetDecimal::from_float(value).expect("Float is not finite")
}
fn jet_decimal_from_fraction(value: jet_std::JetFraction) -> jet_std::JetDecimal {
    jet_std::JetDecimal::from_fraction(&value).expect("Fraction has a repeating expansion")
}
fn jet_decimal_div(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetFraction {
    a.div(b).expect("Decimal quotient does not fit Fraction, or divided by zero")
}
fn jet_decimal_round(a: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    a.round()
}
fn jet_decimal_floor(a: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    a.floor()
}
fn jet_decimal_ceil(a: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    a.ceil()
}
fn jet_decimal_to_int(a: &jet_std::JetDecimal) -> i64 {
    a.to_int_exact().expect("Decimal is not an integer")
}
fn jet_decimal_to_fraction(a: &jet_std::JetDecimal) -> jet_std::JetFraction {
    a.to_fraction().expect("Decimal does not fit Fraction")
}
fn jet_decimal_add(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    a.add(b)
}
fn jet_decimal_sub(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    a.sub(b)
}
fn jet_decimal_mul(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    a.mul(b)
}
fn jet_decimal_equal(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> bool {
    a == b
}
fn jet_decimal_to_string(a: &jet_std::JetDecimal) -> String {
    a.to_string_rep()
}
// D-TYPE2-DEFAULT1: the AOT half of the exact-to-approximate crossing. The
// checker admits an exact Decimal at the irrational-result math functions
// exactly as it admits a Fraction, so every tier needs this conversion, not
// only the evaluator.
fn jet_decimal_to_float(a: &jet_std::JetDecimal) -> f64 {
    a.to_string_rep().parse::<f64>().unwrap_or(f64::NAN)
}

// D-ENC-DYN1=A+: the dynamic `parse` returns the one rich `Data` value (the
// user-facing face of `DataTree`). JSON text parses through the internal `JSON`
// enum, then collapses onto `DataTree` (integral numbers become `Int`, fractional
// `Float`). Object keys arrive in sorted order (the internal `JSON` enum is
// `BTreeMap`-keyed), matching the pre-`Data` dynamic JSON behavior.
fn jet_std_json_parse(text: &String) -> Result<jet_std::DataTree, jet_std::JSONError> {
    jet_std::parse_json_datatree(text)
}
fn jet_std_json_render(d: &jet_std::DataTree) -> String {
    jet_std::render_datatree_json(d, false, 0)
}
fn jet_std_json_render_pretty(d: &jet_std::DataTree) -> String {
    jet_std::render_datatree_json(d, true, 0)
}
fn jet_quote_json_local(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn jet_std_json_render_canonical(d: &jet_std::DataTree) -> String {
    fn render(t: &jet_std::DataTree) -> String {
        match t {
            jet_std::DataTree::Null => "null".to_string(),
            jet_std::DataTree::Bool(b) => b.to_string(),
            jet_std::DataTree::Int(n) => jet_std::jet_int_to_string(*n),
            jet_std::DataTree::Float(f) => format!("{:?}", f),
            jet_std::DataTree::Number(_) | jet_std::DataTree::TypedText(_) => {
                unreachable!("internal JSON carrier escaped typed decode")
            }
            jet_std::DataTree::Text(s) => jet_quote_json_local(s),
            jet_std::DataTree::Bytes(bs) => {
                format!("[{}]", bs.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(","))
            }
            jet_std::DataTree::Array(xs) => {
                format!("[{}]", xs.iter().map(render).collect::<Vec<_>>().join(","))
            }
            jet_std::DataTree::Object(entries) => {
                let mut sorted = entries.clone();
                sorted.sort_by(|a, b| a.0.cmp(&b.0));
                let parts: Vec<String> = sorted
                    .iter()
                    .map(|(k, v)| format!("{}:{}", jet_quote_json_local(k), render(v)))
                    .collect();
                format!("{{{}}}", parts.join(","))
            }
        }
    }
    render(d)
}
fn jet_std_json_events(d: &jet_std::DataTree) -> String {
    fn walk(path: String, t: &jet_std::DataTree, out: &mut Vec<String>) {
        let here = if path.is_empty() { "$".to_string() } else { path };
        match t {
            jet_std::DataTree::Object(entries) => {
                out.push(format!("object_start {here}"));
                for (k, v) in entries {
                    walk(format!("{}.{}", here, k), v, out);
                }
                out.push(format!("object_end {here}"));
            }
            jet_std::DataTree::Array(items) => {
                out.push(format!("array_start {here}"));
                for (i, v) in items.iter().enumerate() {
                    walk(format!("{}[{}]", here, i), v, out);
                }
                out.push(format!("array_end {here}"));
            }
            _ => out.push(format!("value {here} {}", jet_std_json_render_canonical(t))),
        }
    }
    let mut out = Vec::new();
    walk(String::new(), d, &mut out);
    out.join("\n")
}
fn jet_std_jsonl_parse(text: &String) -> Result<Vec<jet_std::DataTree>, jet_std::JSONError> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match jet_std_json_parse(&trimmed.to_string()) {
            Ok(v) => out.push(v),
            Err(e) => {
                return Err(jet_std::JSONError {
                    line: idx as i64 + e.line,
                    message: e.message,
                })
            }
        }
    }
    Ok(out)
}
fn jet_std_jsonl_render(rows: &Vec<jet_std::DataTree>) -> String {
    let mut out = rows
        .iter()
        .map(jet_std_json_render_canonical)
        .collect::<Vec<_>>()
        .join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

// D-JSON1-decode + D-JSON3: lenient JSON decode with coercion surfacing. The
// walk, the coercion message and the audit-line shape are ONE policy in
// `CoreLib/JetStd/JSONDataTree.rs`, shared with the resident JIT host that
// used to carry a byte-equivalent copy (I8/I9). AOT supplies only the sink it
// owns: the process's own stderr.
fn jet_std_json_decode_lenient(text: &String) -> Result<jet_std::DataTree, jet_std::JSONError> {
    jet_std::jet_std_json_decode_lenient(text, &mut |line| eprintln!("{}", line))
}

fn jet_string_bytes(s: &String) -> Vec<u8> {
    s.as_bytes().to_vec()
}
fn jet_string_from_bytes(bs: &Vec<u8>) -> Result<String, jet_std::UTF8Error> {
    jet_string_decode_utf8(bs).map_err(|message| jet_std::UTF8Error {
        message,
    })
}
fn jet_string_from_bytes_lossy(bs: &Vec<u8>) -> String {
    jet_string_decode_utf8_lossy(bs)
}
fn jet_int_to_u8(n: i64) -> Result<u8, String> {
    if (0..=255).contains(&n) {
        Ok(n as u8)
    } else {
        Err("a U8 holds 0..255".to_string())
    }
}
fn jet_stopwatch_elapsed_millis(sw: &jet_std::Stopwatch) -> i64 {
    sw.start.elapsed().as_millis() as i64
}
