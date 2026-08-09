// Deadline clock, budget, and JetDeadlineGuard: card #1747, one home in
// Prelude/Deadline.rs (included by this AOT emission list and by
// jet_codegen::scheduler for the JIT host).

fn jet_deadline_remaining_ms() -> Option<i64> {
    let deadline = jet_ctx_deadline_ms()?;
    Some(deadline.saturating_sub(jet_std_time_now()))
}

fn jet_deadline_exceeded(wait_kind: &str) -> ! {
    let rendered = jet_std::jet_task_deadline(wait_kind).render();
    if jet_interrupt_handler_should_unwind()
        || jet_scheduler_wait_boundary_should_unwind()
        || jet_typed_deadline_boundary_should_unwind()
    {
        std::panic::panic_any(JetDeadlineUnwind { rendered });
    }
    jet_runtime_diagnostic(rendered);
}

fn jet_deadline_check(wait_kind: &str) {
    if matches!(jet_deadline_remaining_ms(), Some(ms) if ms <= 0) {
        jet_deadline_exceeded(wait_kind);
    }
}

fn jet_std_time_sleep(millis: i64) {
    let want = millis.max(0);
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            jet_deadline_exceeded("time sleep");
        }
        if want > remaining {
            jet_scheduler_sleep_ms(remaining as u64);
            jet_deadline_exceeded("time sleep");
        }
    }
    jet_scheduler_sleep_ms(want as u64);
    jet_deadline_check("time sleep");
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
    r.state = r.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = r.state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
fn jet_rng_int(r: &mut jet_std::Rng, lo: i64, hi: i64) -> i64 {
    if hi <= lo {
        return lo;
    }
    let span = (hi - lo + 1) as u64;
    lo + (jet_det_rng_next(r) % span) as i64
}
fn jet_rng_float(r: &mut jet_std::Rng) -> f64 {
    // 53-bit mantissa → [0, 1).
    (jet_det_rng_next(r) >> 11) as f64 / (1u64 << 53) as f64
}
fn jet_rng_float_open(r: &mut jet_std::Rng) -> f64 {
    let x = jet_rng_float(r);
    if x <= 0.0 { f64::MIN_POSITIVE } else { x }
}
fn jet_rng_float_range(r: &mut jet_std::Rng, low: f64, high: f64) -> f64 {
    if !(high > low) {
        return low;
    }
    low + (high - low) * jet_rng_float(r)
}
// D-DET-CAPAPI: the widened deterministic draws — coin, uniform choice, in-place
// Fisher–Yates shuffle. Each advances the SplitMix64 stream, so they are
// reproducible from the seed and mirror the ambient `random.*` set.
fn jet_rng_bool(r: &mut jet_std::Rng) -> bool {
    (jet_det_rng_next(r) & 1) == 1
}
fn jet_rng_bool_p(r: &mut jet_std::Rng, p: f64) -> bool {
    if p <= 0.0 || p.is_nan() {
        false
    } else if p >= 1.0 {
        true
    } else {
        jet_rng_float(r) < p
    }
}
fn jet_rng_normal(r: &mut jet_std::Rng, mean: f64, stddev: f64) -> f64 {
    let u1 = jet_rng_float_open(r);
    let u2 = jet_rng_float(r);
    let z0 = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
    mean + z0 * stddev.max(0.0)
}
fn jet_rng_exponential(r: &mut jet_std::Rng, lambda: f64) -> f64 {
    if lambda <= 0.0 || lambda.is_nan() {
        return 0.0;
    }
    -jet_rng_float_open(r).ln() / lambda
}
fn jet_rng_bytes(r: &mut jet_std::Rng, n: i64) -> Vec<u8> {
    let n = n.max(0) as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(jet_det_rng_next(r) as u8);
    }
    out
}
fn jet_rng_split(r: &mut jet_std::Rng) -> jet_std::Rng {
    jet_std::Rng { state: jet_det_rng_next(r) }
}
fn jet_rng_pick<T: Clone>(r: &mut jet_std::Rng, xs: &Vec<T>) -> JetOutcome<T, JetAbsent> {
    if xs.is_empty() {
        Err(JetAbsent)
    } else {
        Ok(xs[jet_rng_int(r, 0, xs.len() as i64 - 1) as usize].clone())
    }
}
fn jet_rng_weighted_pick<T: Clone>(
    r: &mut jet_std::Rng,
    xs: &Vec<T>,
    weights: &Vec<f64>,
) -> JetOutcome<T, JetAbsent> {
    if xs.is_empty() || xs.len() != weights.len() {
        return Err(JetAbsent);
    }
    let mut total = 0.0;
    for &w in weights {
        if w.is_finite() && w > 0.0 {
            total += w;
        }
    }
    if total <= 0.0 {
        return Err(JetAbsent);
    }
    let mut needle = jet_rng_float_range(r, 0.0, total);
    for (item, &weight) in xs.iter().zip(weights.iter()) {
        let w = if weight.is_finite() && weight > 0.0 { weight } else { 0.0 };
        if needle < w {
            return Ok(item.clone());
        }
        needle -= w;
    }
    jet_outcome_of(xs.last().cloned())
}
fn jet_rng_sample<T: Clone>(r: &mut jet_std::Rng, xs: &Vec<T>, k: i64) -> Vec<T> {
    let want = (k.max(0) as usize).min(xs.len());
    let mut pool = xs.clone();
    for i in 0..want {
        let j = jet_rng_int(r, i as i64, pool.len() as i64 - 1) as usize;
        pool.swap(i, j);
    }
    pool.truncate(want);
    pool
}
fn jet_rng_shuffle<T>(r: &mut jet_std::Rng, xs: &mut Vec<T>) {
    let len = xs.len();
    for i in (1..len).rev() {
        let j = jet_rng_int(r, 0, i as i64) as usize;
        xs.swap(i, j);
    }
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
fn jet_duration_is_zero(d: &jet_std::Duration) -> bool {
    jet_duration_kernel_is_zero(d.ns)
}
fn jet_duration_total_seconds(d: &jet_std::Duration) -> i64 {
    jet_duration_kernel_total_seconds(d.ns)
}
fn jet_duration_difference(a: &jet_std::Duration, b: &jet_std::Duration) -> jet_std::Duration {
    jet_std::Duration {
        ns: jet_duration_kernel_difference(a.ns, b.ns),
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
fn jet_time_parse_rfc3339(s: &String) -> Result<JetDateTime, String> {
    JetDateTime::parse_rfc3339(s)
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

// D-BIGINT1 / D-DECIMAL1: precise numeric constructors and methods.
fn jet_bigint_from_int(n: i64) -> jet_std::JetBigInt {
    jet_std::JetBigInt::from_int(n)
}
fn jet_bigint_from_str(s: &String) -> jet_std::JetBigInt {
    jet_std::JetBigInt::from_str(s).expect("invalid BigInt string")
}
fn jet_bigint_add(a: &jet_std::JetBigInt, b: &jet_std::JetBigInt) -> jet_std::JetBigInt {
    a.add(b)
}
fn jet_bigint_sub(a: &jet_std::JetBigInt, b: &jet_std::JetBigInt) -> jet_std::JetBigInt {
    a.sub(b)
}
fn jet_bigint_mul(a: &jet_std::JetBigInt, b: &jet_std::JetBigInt) -> jet_std::JetBigInt {
    a.mul(b)
}
fn jet_bigint_neg(a: &jet_std::JetBigInt) -> jet_std::JetBigInt {
    a.neg()
}
fn jet_bigint_to_string(a: &jet_std::JetBigInt) -> String {
    a.to_string_rep()
}
fn jet_decimal_from_str(s: &String) -> jet_std::JetDecimal {
    jet_std::JetDecimal::from_str(s).expect("invalid Decimal string")
}
// D-NUMTYPE1=A: exact ratios. Every answer is optional, because a zero bottom
// has no value and a product can leave the range.
fn jet_fraction_new(numerator: i64, denominator: i64) -> Option<jet_std::JetFraction> {
    jet_std::JetFraction::new(numerator, denominator)
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
fn jet_fraction_equal(a: &jet_std::JetFraction, b: &jet_std::JetFraction) -> bool {
    a == b
}
fn jet_fraction_numerator(a: &jet_std::JetFraction) -> i64 {
    a.numerator
}
fn jet_fraction_denominator(a: &jet_std::JetFraction) -> i64 {
    a.denominator
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
fn jet_decimal_add(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    a.add(b)
}
fn jet_decimal_sub(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    a.sub(b)
}
fn jet_decimal_mul(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    a.mul(b)
}
fn jet_decimal_to_string(a: &jet_std::JetDecimal) -> String {
    a.to_string_rep()
}

// D-ENC-DYN1=A+: the dynamic `parse` returns the one rich `Data` value (the
// user-facing face of `DataTree`). JSON text parses through the internal `JSON`
// enum, then collapses onto `DataTree` (integral numbers become `Int`, fractional
// `Float`). Object keys arrive in sorted order (the internal `JSON` enum is
// `BTreeMap`-keyed), matching the pre-`Data` dynamic JSON behavior.
fn jet_std_json_parse(text: &String) -> Result<jet_std::DataTree, jet_std::JSONError> {
    jet_std::parse_json(text).map(|j| jet_std::datatree_from_json(&j))
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
            jet_std::DataTree::Int(n) => n.to_string(),
            jet_std::DataTree::Float(f) => format!("{:?}", f),
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

// D-JSON1-decode + D-JSON3: lenient JSON decode with coercion surfacing.
// Parses `text`, then walks the result. Any JSON string that looks like a
// number or boolean is coerced to that type; one log line is emitted per
// coercion naming the field and the from→to types. The coerced value collapses
// onto `Data` (D-ENC-DYN1=A+).
fn jet_std_json_decode_lenient(text: &String) -> Result<jet_std::DataTree, jet_std::JSONError> {
    let parsed = jet_std::parse_json(text)?;
    Ok(jet_std::datatree_from_json(&jet_std_json_coerce_walk(
        &parsed, "",
    )))
}

fn jet_std_json_coerce_walk(value: &jet_std::JSON, path: &str) -> jet_std::JSON {
    match value {
        jet_std::JSON::Text(s) => {
            // try bool first (exact match only)
            if s == "true" {
                jet_std_json_emit_coerce(path, "string", "boolean");
                return jet_std::JSON::Boolean(true);
            }
            if s == "false" {
                jet_std_json_emit_coerce(path, "string", "boolean");
                return jet_std::JSON::Boolean(false);
            }
            // try number (must parse as valid f64 and round-trip cleanly)
            if let Ok(n) = s.parse::<f64>() {
                if n.is_finite() {
                    jet_std_json_emit_coerce(path, "string", "number");
                    return jet_std::JSON::Number(n);
                }
            }
            value.clone()
        }
        jet_std::JSON::Object(entries) => {
            let mut out = std::collections::BTreeMap::new();
            for (k, v) in entries {
                let child_path = if path.is_empty() {
                    format!("{}", k)
                } else {
                    format!("{}.{}", path, k)
                };
                out.insert(k.clone(), jet_std_json_coerce_walk(v, &child_path));
            }
            jet_std::JSON::Object(out)
        }
        jet_std::JSON::Array(items) => {
            let coerced: Vec<jet_std::JSON> = items
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let child_path = if path.is_empty() {
                        format!("[{}]", i)
                    } else {
                        format!("{}[{}]", path, i)
                    };
                    jet_std_json_coerce_walk(v, &child_path)
                })
                .collect();
            jet_std::JSON::Array(coerced)
        }
        // Null, Boolean, Number — already the right type, no coercion.
        other => other.clone(),
    }
}

fn jet_std_json_emit_coerce(path: &str, from: &str, to: &str) {
    let field_label = if path.is_empty() { "<root>" } else { path };
    let msg = format!(
        "json coerce: field \"{}\" {} \u{2192} {}",
        field_label, from, to
    );
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    eprintln!("{{\"level\":\"info\",\"body\":\"{}\",\"ts\":{}}}", msg, ts);
}

fn jet_string_bytes(s: &String) -> Vec<u8> {
    s.as_bytes().to_vec()
}
fn jet_string_from_bytes(bs: &Vec<u8>) -> Result<String, jet_std::UTF8Error> {
    String::from_utf8(bs.clone()).map_err(|e| jet_std::UTF8Error {
        message: e.to_string(),
    })
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
