// D-TIME-INSTANT-SPLIT1=A: JetInstant trait impls stay beside the Core-owned
// carrier. The Date-family carriers and their traits live in the fixed runtime
// (`Prelude/Core.rs`) so the optional Core crate does not violate orphan rules.
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
    jet_scheduler_world_reject_uncontrolled("clock");
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
    let now = c.now().saturating_add(jet_std_time_duration_to_millis(d.ns));
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
    n: jet_foundation::Numeric::JetInt,
    unit: jet_std::DurationUnit,
) -> Result<jet_std::Duration, jet_std::RangeError> {
    let n = jet_std::jet_int_owned_to_i64(&n).map_err(|_| jet_std::RangeError {
        reason: jet_duration_kernel_int_error_reason().to_string(),
    })?;
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
/// AOT's checked Core route carries Duration by reference; keep conversion at
/// this boundary while the shared sleep policy remains nanosecond-based.
fn jet_std_time_sleep_duration(duration: &jet_std::Duration) {
    jet_std_time_sleep_duration_ns(duration.ns);
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
fn jet_duration_abs(d: &jet_std::Duration) -> jet_std::Duration {
    jet_std::Duration {
        ns: jet_duration_kernel_abs(d.ns),
    }
}
fn jet_duration_negated(d: &jet_std::Duration) -> jet_std::Duration {
    jet_std::Duration {
        ns: jet_duration_kernel_negated(d.ns),
    }
}
fn jet_duration_sign(d: &jet_std::Duration) -> i64 {
    jet_duration_kernel_sign(d.ns)
}
fn jet_duration_total_in(d: &jet_std::Duration, unit: &String) -> f64 {
    jet_duration_kernel_total_in(d.ns, unit)
}
fn jet_duration_round(
    d: &jet_std::Duration,
    unit: &String,
    increment: &i64,
    mode: &String,
) -> jet_std::Duration {
    let ns = jet_duration_kernel_round(d.ns, unit, *increment, mode).unwrap_or(d.ns);
    jet_std::Duration { ns }
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
// D-TIME-INT-ADAPTER1: the native temporal kernel uses i64 while AOT's
// default-Int ABI carries an exact packed word. Decode once at the surface
// boundary; constructors below keep all temporal policy in Core/Time.rs.
fn jet_time_unix_input(value: i64) -> i64 {
    jet_std::jet_int_to_i64(value).unwrap_or_else(|| {
        jet_runtime_stop("E1003", file!(), line!(), jet_c_int_range_message())
    })
}
fn jet_time_owned_input(value: &jet_foundation::Numeric::JetInt) -> i64 {
    jet_std::jet_int_owned_to_i64(value).unwrap_or_else(|_| {
        jet_runtime_stop("E1003", file!(), line!(), jet_c_int_range_message())
    })
}
fn jet_local_date_owned(
    year: &jet_foundation::Numeric::JetInt,
    month: &jet_foundation::Numeric::JetInt,
    day: &jet_foundation::Numeric::JetInt,
) -> JetDate {
    JetDate::new(
        jet_time_owned_input(year),
        jet_time_owned_input(month),
        jet_time_owned_input(day),
    )
}
fn jet_local_time_owned(
    hour: &jet_foundation::Numeric::JetInt,
    minute: &jet_foundation::Numeric::JetInt,
    second: &jet_foundation::Numeric::JetInt,
) -> JetLocalTime {
    JetLocalTime::new(
        jet_time_owned_input(hour),
        jet_time_owned_input(minute),
        jet_time_owned_input(second),
    )
}
fn jet_period_owned(
    years: &jet_foundation::Numeric::JetInt,
    months: &jet_foundation::Numeric::JetInt,
    days: &jet_foundation::Numeric::JetInt,
) -> JetPeriod {
    JetPeriod::new(
        jet_time_owned_input(years),
        jet_time_owned_input(months),
        jet_time_owned_input(days),
    )
}
fn jet_time_datetime(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) -> JetDateTime {
    JetDateTime::from_parts(
        jet_time_unix_input(year),
        jet_time_unix_input(month),
        jet_time_unix_input(day),
        jet_time_unix_input(hour),
        jet_time_unix_input(minute),
        jet_time_unix_input(second),
        0,
    )
}
fn jet_time_time(hour: i64, minute: i64, second: i64) -> JetLocalTime {
    JetLocalTime::new(
        jet_time_unix_input(hour),
        jet_time_unix_input(minute),
        jet_time_unix_input(second),
    )
}
fn jet_time_days_in_month(year: i64, month: i64) -> i64 {
    JetDate::days_in_month_of(jet_time_unix_input(year), jet_time_unix_input(month))
}
fn jet_time_is_leap_year(year: i64) -> bool {
    JetDate::is_leap(jet_time_unix_input(year))
}
fn jet_calendar_isleap(year: i64) -> bool {
    jet_time_is_leap_year(year)
}
fn jet_calendar_leapdays(
    first: i64,
    second: i64,
) -> jet_foundation::Numeric::JetInt {
    let (start, end) = if first <= second {
        (first, second)
    } else {
        (second, first)
    };
    jet_std::jet_int_owned_from_i64(
        (start..end)
            .filter(|year| JetDate::is_leap(*year))
            .count() as i64,
    )
}
fn jet_calendar_weekday_i64(year: i64, month: i64, day: i64) -> i64 {
    let adjusted_year = if month < 3 { year - 1 } else { year };
    let offsets = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let month_index = month.saturating_sub(1).clamp(0, 11) as usize;
    let sunday_zero = (adjusted_year
        + adjusted_year.div_euclid(4)
        - adjusted_year.div_euclid(100)
        + adjusted_year.div_euclid(400)
        + offsets[month_index]
        + day)
        .rem_euclid(7);
    (sunday_zero + 6).rem_euclid(7)
}
fn jet_coll_owned_values(
    values: Vec<i64>,
) -> Vec<jet_foundation::Numeric::JetInt> {
    values
        .into_iter()
        .map(jet_std::jet_int_owned_from_i64)
        .collect()
}

fn jet_coll_sift_down_aot(values: &mut [i64], mut index: usize) {
    loop {
        let left = index.saturating_mul(2).saturating_add(1);
        if left >= values.len() {
            break;
        }
        let right = left + 1;
        let child = if right < values.len() && values[right] < values[left] {
            right
        } else {
            left
        };
        if values[index] <= values[child] {
            break;
        }
        values.swap(index, child);
        index = child;
    }
}

fn jet_coll_heappop_aot(values: &[i64]) -> (Vec<i64>, Option<i64>) {
    if values.is_empty() {
        return (Vec::new(), None);
    }
    if values.len() == 1 {
        return (Vec::new(), Some(values[0]));
    }
    let mut heap = Vec::with_capacity(values.len() - 1);
    heap.push(values[values.len() - 1]);
    heap.extend_from_slice(&values[1..values.len() - 1]);
    let value = values[0];
    jet_coll_sift_down_aot(&mut heap, 0);
    (heap, Some(value))
}

fn jet_coll_heappushpop_aot(values: &[i64], value: i64) -> (Vec<i64>, i64) {
    if values.is_empty() || value <= values[0] {
        return (values.to_vec(), value);
    }
    let popped = values[0];
    let mut heap = Vec::with_capacity(values.len());
    heap.push(value);
    heap.extend_from_slice(&values[1..]);
    jet_coll_sift_down_aot(&mut heap, 0);
    (heap, popped)
}

fn jet_coll_heapreplace_aot(values: &[i64], value: i64) -> (Vec<i64>, Option<i64>) {
    if values.is_empty() {
        return (vec![value], None);
    }
    let popped = values[0];
    let mut heap = Vec::with_capacity(values.len());
    heap.push(value);
    heap.extend_from_slice(&values[1..]);
    jet_coll_sift_down_aot(&mut heap, 0);
    (heap, Some(popped))
}

fn jet_coll_heappop(
    values: &[i64],
) -> (
    Vec<jet_foundation::Numeric::JetInt>,
    JetOutcome<jet_foundation::Numeric::JetInt, JetAbsent>,
) {
    let (heap, value) = jet_coll_heappop_aot(values);
    (
        jet_coll_owned_values(heap),
        value
            .map(jet_std::jet_int_owned_from_i64)
            .ok_or(JetAbsent),
    )
}

fn jet_coll_heappushpop(
    values: &[i64],
    value: i64,
) -> (
    Vec<jet_foundation::Numeric::JetInt>,
    jet_foundation::Numeric::JetInt,
) {
    let (heap, popped) = jet_coll_heappushpop_aot(values, value);
    (
        jet_coll_owned_values(heap),
        jet_std::jet_int_owned_from_i64(popped),
    )
}

fn jet_coll_heapreplace(
    values: &[i64],
    value: i64,
) -> (
    Vec<jet_foundation::Numeric::JetInt>,
    JetOutcome<jet_foundation::Numeric::JetInt, JetAbsent>,
) {
    let (heap, popped) = jet_coll_heapreplace_aot(values, value);
    (
        jet_coll_owned_values(heap),
        popped
            .map(jet_std::jet_int_owned_from_i64)
            .ok_or(JetAbsent),
    )
}
impl JetShow for (
    Vec<jet_foundation::Numeric::JetInt>,
    JetOutcome<jet_foundation::Numeric::JetInt, JetAbsent>,
) {
    fn jet_show(&self) -> String {
        format!(
            "(heap,value) {{ heap: {}, value: {} }}",
            self.0.jet_show(),
            self.1.jet_show(),
        )
    }
}

impl JetDisplay for (
    Vec<jet_foundation::Numeric::JetInt>,
    JetOutcome<jet_foundation::Numeric::JetInt, JetAbsent>,
) {
    fn jet_display(&self) -> String {
        self.jet_show()
    }
}

impl JetDebug for (
    Vec<jet_foundation::Numeric::JetInt>,
    JetOutcome<jet_foundation::Numeric::JetInt, JetAbsent>,
) {
    fn jet_debug(&self) -> String {
        self.jet_show()
    }
}

impl JetShow for (
    Vec<jet_foundation::Numeric::JetInt>,
    jet_foundation::Numeric::JetInt,
) {
    fn jet_show(&self) -> String {
        format!(
            "(heap,value) {{ heap: {}, value: {} }}",
            self.0.jet_show(),
            self.1.jet_show(),
        )
    }
}

impl JetDisplay for (
    Vec<jet_foundation::Numeric::JetInt>,
    jet_foundation::Numeric::JetInt,
) {
    fn jet_display(&self) -> String {
        self.jet_show()
    }
}

impl JetDebug for (
    Vec<jet_foundation::Numeric::JetInt>,
    jet_foundation::Numeric::JetInt,
) {
    fn jet_debug(&self) -> String {
        self.jet_show()
    }
}
impl JetShow for (String, String) {
    fn jet_show(&self) -> String {
        format!("({}, {})", self.0.jet_show(), self.1.jet_show())
    }
}

impl JetDisplay for (String, String) {
    fn jet_display(&self) -> String {
        self.jet_show()
    }
}

impl JetDebug for (String, String) {
    fn jet_debug(&self) -> String {
        self.jet_show()
    }
}

fn jet_calendar_weekday(
    year: i64,
    month: i64,
    day: i64,
) -> jet_foundation::Numeric::JetInt {
    jet_std::jet_int_owned_from_i64(jet_calendar_weekday_i64(year, month, day))
}
fn jet_calendar_monthrange(
    year: i64,
    month: i64,
) -> (
    jet_foundation::Numeric::JetInt,
    jet_foundation::Numeric::JetInt,
) {
    (
        jet_std::jet_int_owned_from_i64(jet_time_days_in_month(year, month)),
        jet_std::jet_int_owned_from_i64(jet_calendar_weekday_i64(year, month, 1)),
    )
}
fn jet_calendar_monthcalendar(
    year: i64,
    month: i64,
) -> Vec<Vec<jet_foundation::Numeric::JetInt>> {
    jet_calendar_monthcalendar_start(year, month, 0)
}
fn jet_calendar_monthcalendar_start(
    year: i64,
    month: i64,
    firstweekday: i64,
) -> Vec<Vec<jet_foundation::Numeric::JetInt>> {
    let days = jet_time_days_in_month(year, month);
    let start = (jet_calendar_weekday_i64(year, month, 1) - firstweekday).rem_euclid(7);
    let mut weeks = Vec::new();
    let mut week = Vec::new();
    for _ in 0..start {
        week.push(jet_std::jet_int_owned_from_i64(0));
    }
    for day in 1..=days {
        week.push(jet_std::jet_int_owned_from_i64(day));
        if week.len() == 7 {
            weeks.push(week);
            week = Vec::new();
        }
    }
    if !week.is_empty() {
        while week.len() < 7 {
            week.push(jet_std::jet_int_owned_from_i64(0));
        }
        weeks.push(week);
    }
    weeks
}
fn jet_calendar_yearcalendar(
    year: i64,
) -> Vec<Vec<Vec<jet_foundation::Numeric::JetInt>>> {
    (1..=12)
        .map(|month| jet_calendar_monthcalendar(year, month))
        .collect()
}
fn jet_calendar_weekheader(width: i64, firstweekday: i64) -> Vec<String> {
    let width = width.max(1) as usize;
    (0..7)
        .map(|index| {
            let text = jet_calendar_day_abbr((index + firstweekday).rem_euclid(7));
            let bytes = text.as_bytes();
            if bytes.len() >= width {
                String::from_utf8_lossy(&bytes[..width]).into_owned()
            } else {
                format!("{text}{}", " ".repeat(width - bytes.len()))
            }
        })
        .collect()
}
fn jet_calendar_formatmonth(year: i64, month: i64, width: i64) -> String {
    let width = width.max(2) as usize;
    let title = format!("{} {year}", jet_calendar_month_name(month));
    let title_width = width.saturating_mul(7).saturating_add(6);
    let title_bytes = title.as_bytes();
    let title = if title_bytes.len() >= title_width {
        String::from_utf8_lossy(&title_bytes[..title_width]).into_owned()
    } else {
        let left = (title_width - title_bytes.len()) / 2;
        let right = title_width - title_bytes.len() - left;
        format!("{}{}{}", " ".repeat(left), title, " ".repeat(right))
    };
    let cells = jet_calendar_weekheader(width as i64, 0);
    let mut out = format!("{title}\n{}\n", cells.join(" "));
    for week in jet_calendar_monthcalendar(year, month) {
        let row = week
            .into_iter()
            .map(|day| {
                let day = day.to_raw().to_string();
                let cell = if day == "0" { String::new() } else { day };
                if cell.len() >= width {
                    cell[..width].to_string()
                } else {
                    format!("{}{}", " ".repeat(width - cell.len()), cell)
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&row);
        out.push('\n');
    }
    out
}
fn jet_calendar_formatyear(year: i64) -> String {
    let mut out = String::new();
    for month in 1..=12 {
        out.push_str(&jet_calendar_formatmonth(year, month, 3));
        out.push('\n');
    }
    out
}
fn jet_calendar_day_name(weekday: i64) -> String {
    [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ]
    .get(weekday.clamp(0, 6) as usize)
    .unwrap_or(&"Sunday")
    .to_string()
}
fn jet_calendar_day_abbr(weekday: i64) -> String {
    ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
        .get(weekday.clamp(0, 6) as usize)
        .unwrap_or(&"Sun")
        .to_string()
}
fn jet_calendar_month_name(month: i64) -> String {
    [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ]
    .get(month.saturating_sub(1).clamp(0, 11) as usize)
    .unwrap_or(&"December")
    .to_string()
}
fn jet_calendar_month_abbr(month: i64) -> String {
    [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov",
        "Dec",
    ]
    .get(month.saturating_sub(1).clamp(0, 11) as usize)
    .unwrap_or(&"Dec")
    .to_string()
}
fn jet_calendar_timegm(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) -> jet_foundation::Numeric::JetInt {
    jet_std::jet_int_owned_from_i64(
        JetDateTime::from_parts(year, month, day, hour, minute, second, 0).to_unix_seconds(),
    )
}
fn jet_time_period(years: i64, months: i64, days: i64) -> JetPeriod {
    JetPeriod::new(
        jet_time_unix_input(years),
        jet_time_unix_input(months),
        jet_time_unix_input(days),
    )
}
fn jet_time_period_days(days: i64) -> JetPeriod {
    JetPeriod::days(jet_time_unix_input(days))
}
fn jet_time_period_months(months: i64) -> JetPeriod {
    JetPeriod::months(jet_time_unix_input(months))
}
fn jet_time_period_years(years: i64) -> JetPeriod {
    JetPeriod::years(jet_time_unix_input(years))
}
fn jet_time_from_unix_ms(value: i64) -> JetDateTime {
    JetDateTime::from_unix_ms(jet_time_unix_input(value))
}
fn jet_time_from_unix_seconds(value: i64) -> JetDateTime {
    JetDateTime::from_unix_seconds(jet_time_unix_input(value))
}
fn jet_time_from_unix_microseconds(value: i64) -> JetDateTime {
    JetDateTime::from_unix_microseconds(jet_time_unix_input(value))
}
fn jet_time_from_unix_nanoseconds(value: i64) -> JetDateTime {
    JetDateTime::from_unix_nanoseconds(jet_time_unix_input(value))
}

fn jet_time_now_utc() -> JetDateTime {
    JetDateTime::now()
}
fn jet_time_today() -> JetDate {
    JetDate::today_utc()
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
fn jet_time_zoned_local(
    date: &JetDate,
    time: &JetLocalTime,
    zone: &JetZone,
    disambiguation: &String,
) -> Result<JetZonedDateTime, String> {
    JetZonedDateTime::from_local_with_disambiguation(date, time, zone, disambiguation)
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
fn jet_time_text_error(message: String) -> jet_std::TextError {
    jet_std::TextError { message }
}

fn jet_time_range_error(reason: String) -> jet_std::RangeError {
    jet_std::RangeError { reason }
}

impl JetDate {
    fn diff_days_value(&self, other: JetDate) -> i64 {
        self.diff_days(&other)
    }

    fn add_period_value(&self, period: JetPeriod) -> JetDate {
        self.add_period(&period)
    }

    fn subtract_period_value(&self, period: JetPeriod) -> JetDate {
        self.subtract_period(&period)
    }

    fn until_duration(
        &self,
        other: JetDate,
        largest_unit: &String,
        smallest_unit: &String,
        rounding_mode: &String,
        increment: i64,
    ) -> jet_std::Duration {
        jet_std::Duration {
            ns: self.until_ns(
                &other,
                largest_unit,
                smallest_unit,
                rounding_mode,
                increment,
            ),
        }
    }

    fn since_duration(
        &self,
        other: JetDate,
        largest_unit: &String,
        smallest_unit: &String,
        rounding_mode: &String,
        increment: i64,
    ) -> jet_std::Duration {
        jet_std::Duration {
            ns: self.since_ns(
                &other,
                largest_unit,
                smallest_unit,
                rounding_mode,
                increment,
            ),
        }
    }

    fn format_checked_text(&self, pattern: &String) -> Result<String, jet_std::TextError> {
        self.format_checked(pattern).map_err(jet_time_text_error)
    }

    fn equal_value(&self, other: JetDate) -> bool {
        self == &other
    }

    fn compare_value(&self, other: JetDate) -> __jet_Ordering {
        jet_time_ordering(self.cmp(&other))
    }
}

impl JetLocalTime {
    fn add_duration_value(&self, duration: jet_std::Duration) -> JetLocalTime {
        self.add_duration_ns(duration.ns)
    }

    fn subtract_duration_value(&self, duration: jet_std::Duration) -> JetLocalTime {
        self.subtract_duration_ns(duration.ns)
    }

    fn until_duration(
        &self,
        other: JetLocalTime,
        largest_unit: &String,
        smallest_unit: &String,
        rounding_mode: &String,
        increment: i64,
    ) -> jet_std::Duration {
        jet_std::Duration {
            ns: self.until_ns(
                &other,
                largest_unit,
                smallest_unit,
                rounding_mode,
                increment,
            ),
        }
    }

    fn since_duration(
        &self,
        other: JetLocalTime,
        largest_unit: &String,
        smallest_unit: &String,
        rounding_mode: &String,
        increment: i64,
    ) -> jet_std::Duration {
        jet_std::Duration {
            ns: self.since_ns(
                &other,
                largest_unit,
                smallest_unit,
                rounding_mode,
                increment,
            ),
        }
    }

    fn format_checked_text(&self, pattern: &String) -> Result<String, jet_std::TextError> {
        self.format_checked(pattern).map_err(jet_time_text_error)
    }

    fn equal_value(&self, other: JetLocalTime) -> bool {
        self == &other
    }

    fn compare_value(&self, other: JetLocalTime) -> __jet_Ordering {
        jet_time_ordering(self.cmp(&other))
    }
}

impl JetDateTime {
    fn to_unix_ms_value(&self) -> jet_foundation::Numeric::JetInt {
        jet_std::jet_int_owned_from_i64(self.to_unix_ms())
    }

    fn to_unix_seconds_value(&self) -> jet_foundation::Numeric::JetInt {
        jet_std::jet_int_owned_from_i64(self.to_unix_seconds())
    }

    fn to_unix_microseconds_value(
        &self,
    ) -> Result<jet_foundation::Numeric::JetInt, jet_std::RangeError> {
        self.to_unix_microseconds()
            .map(jet_std::jet_int_owned_from_i64)
            .map_err(jet_time_range_error)
    }

    fn to_unix_nanoseconds_value(
        &self,
    ) -> Result<jet_foundation::Numeric::JetInt, jet_std::RangeError> {
        self.to_unix_nanoseconds()
            .map(jet_std::jet_int_owned_from_i64)
            .map_err(jet_time_range_error)
    }

    fn plus_duration_value(&self, duration: jet_std::Duration) -> JetDateTime {
        self.plus_duration_ns(duration.ns)
    }

    fn subtract_duration_value(&self, duration: jet_std::Duration) -> JetDateTime {
        self.subtract_duration_ns(duration.ns)
    }

    fn difference_duration(&self, other: JetDateTime) -> jet_std::Duration {
        jet_std::Duration {
            ns: self.difference_ns(&other),
        }
    }

    fn add_period_value(&self, period: JetPeriod) -> JetDateTime {
        self.add_period(&period)
    }

    fn subtract_period_value(&self, period: JetPeriod) -> JetDateTime {
        self.subtract_period(&period)
    }

    fn until_duration(
        &self,
        other: JetDateTime,
        largest_unit: &String,
        smallest_unit: &String,
        rounding_mode: &String,
        increment: i64,
    ) -> jet_std::Duration {
        jet_std::Duration {
            ns: self.until_ns(
                &other,
                largest_unit,
                smallest_unit,
                rounding_mode,
                increment,
            ),
        }
    }

    fn since_duration(
        &self,
        other: JetDateTime,
        largest_unit: &String,
        smallest_unit: &String,
        rounding_mode: &String,
        increment: i64,
    ) -> jet_std::Duration {
        jet_std::Duration {
            ns: self.since_ns(
                &other,
                largest_unit,
                smallest_unit,
                rounding_mode,
                increment,
            ),
        }
    }

    fn in_zone_value(&self, zone: JetZone) -> JetZonedDateTime {
        self.in_zone(&zone)
    }

    fn format_checked_text(&self, pattern: &String) -> Result<String, jet_std::TextError> {
        self.format_checked(pattern).map_err(jet_time_text_error)
    }

    fn equal_value(&self, other: JetDateTime) -> bool {
        self == &other
    }

    fn compare_value(&self, other: JetDateTime) -> __jet_Ordering {
        jet_time_ordering(self.cmp(&other))
    }
}

impl JetInstant {
    fn elapsed_duration(&self) -> jet_std::Duration {
        jet_std::Duration {
            ns: self.elapsed_nanos(),
        }
    }

    fn equal_value(&self, other: JetInstant) -> bool {
        self == &other
    }

    fn compare_value(&self, other: JetInstant) -> __jet_Ordering {
        jet_time_ordering(self.cmp(&other))
    }
}

impl JetPeriod {
    fn add_value(&self, other: JetPeriod) -> JetPeriod {
        self.add(&other)
    }

    fn sub_value(&self, other: JetPeriod) -> JetPeriod {
        self.sub(&other)
    }
}

pub(crate) trait JetPeriodAnchor {
    fn period_total_in(&self, period: &JetPeriod, unit: &String) -> f64;
}

impl JetPeriodAnchor for JetDate {
    fn period_total_in(&self, period: &JetPeriod, unit: &String) -> f64 {
        period.total_in_date(unit, self)
    }
}

impl JetPeriodAnchor for JetDateTime {
    fn period_total_in(&self, period: &JetPeriod, unit: &String) -> f64 {
        period.total_in_datetime(unit, self)
    }
}

impl JetPeriod {
    fn total_in_value<A: JetPeriodAnchor>(&self, unit: &String, anchor: &A) -> f64 {
        anchor.period_total_in(self, unit)
    }
}

impl JetZone {
    fn next_transition_value(&self, utc_seconds: i64) -> JetOutcome<i64, JetAbsent> {
        jet_outcome_of(self.next_transition(utc_seconds))
    }

    fn previous_transition_value(&self, utc_seconds: i64) -> JetOutcome<i64, JetAbsent> {
        jet_outcome_of(self.previous_transition(utc_seconds))
    }

    fn start_of_day_value(&self, date: JetDate) -> JetZonedDateTime {
        self.start_of_day_zoned(&date)
    }

    fn hours_in_day_value(&self, date: JetDate) -> i64 {
        self.hours_in_day(&date)
    }
}

impl JetZonedDateTime {
    fn add_duration_value(&self, duration: jet_std::Duration) -> JetZonedDateTime {
        self.add_duration_ns(duration.ns)
    }

    fn subtract_duration_value(&self, duration: jet_std::Duration) -> JetZonedDateTime {
        self.subtract_duration_ns(duration.ns)
    }

    fn add_period_value(&self, period: JetPeriod) -> JetZonedDateTime {
        self.add_period(&period)
    }

    fn subtract_period_value(&self, period: JetPeriod) -> JetZonedDateTime {
        self.subtract_period(&period)
    }

    fn until_duration(
        &self,
        other: JetZonedDateTime,
        largest_unit: &String,
        smallest_unit: &String,
        rounding_mode: &String,
        increment: i64,
    ) -> jet_std::Duration {
        jet_std::Duration {
            ns: self.until_ns(
                &other,
                largest_unit,
                smallest_unit,
                rounding_mode,
                increment,
            ),
        }
    }

    fn since_duration(
        &self,
        other: JetZonedDateTime,
        largest_unit: &String,
        smallest_unit: &String,
        rounding_mode: &String,
        increment: i64,
    ) -> jet_std::Duration {
        jet_std::Duration {
            ns: self.since_ns(
                &other,
                largest_unit,
                smallest_unit,
                rounding_mode,
                increment,
            ),
        }
    }

    fn next_transition_value(&self) -> JetOutcome<i64, JetAbsent> {
        jet_outcome_of(self.next_transition())
    }

    fn previous_transition_value(&self) -> JetOutcome<i64, JetAbsent> {
        jet_outcome_of(self.previous_transition())
    }

    fn format_checked_text(&self, pattern: &String) -> Result<String, jet_std::TextError> {
        self.format_checked(pattern).map_err(jet_time_text_error)
    }

    fn equal_value(&self, other: JetZonedDateTime) -> bool {
        self == &other
    }

    fn compare_value(&self, other: JetZonedDateTime) -> __jet_Ordering {
        jet_time_ordering(self.cmp(&other))
    }
}


fn jet_url_parse(s: &String) -> Result<crate::jet_std::JetURL, String> {
    crate::jet_std::JetURLParts::parse(s).map(crate::jet_std::JetURL::from_url_parts)
}

/// D-BOUND-HEAD1=A: DateTime heads are complete RFC3339 values. Sema has
/// already rejected holes that could make the literal invalid.
fn jet_typed_datetime_literal(literals: &[&str], holes: Vec<String>) -> JetDateTime {
    let text = jet_typed_datetime_interpolate(literals, &holes);
    match jet_time_parse_rfc3339(&text) {
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
    crate::jet_std::JetURLParts::from_parts(scheme, host, path, query, fragment)
        .map(crate::jet_std::JetURL::from_url_parts)
}
fn jet_url_file(path: &String) -> crate::jet_std::JetURL {
    crate::jet_std::JetURL::from_url_parts(crate::jet_std::JetURLParts::file(path))
}
fn jet_url_data(mime: &crate::jet_std::JetMIME, text: &String) -> crate::jet_std::JetURL {
    crate::jet_std::JetURL::from_url_parts(crate::jet_std::JetURLParts::data(mime, text))
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
fn jet_url_join(base: &String, rel: &String) -> String {
    crate::jet_std::jet_url_join_text(base, rel)
}
fn jet_url_parse_qsl(query: &String) -> Vec<Vec<String>> {
    crate::jet_std::jet_url_parse_qsl_rows(query)
}
fn jet_url_urlencode(pairs: &Vec<Vec<String>>) -> String {
    crate::jet_std::jet_url_query_rows(pairs)
}
fn jet_url_split_fragment(text: &String) -> (String, String) {
    let (url, fragment) = crate::jet_std::jet_url_split_fragment(text);
    (fragment, url)
}
fn jet_url_quote(text: &String) -> String {
    crate::jet_std::jet_url_quote(text)
}
fn jet_url_quote_from_bytes(data: &Vec<u8>) -> String {
    crate::jet_std::jet_url_quote_from_bytes(data)
}
fn jet_url_quote_plus(text: &String) -> String {
    crate::jet_std::jet_url_quote_plus(text)
}
fn jet_url_unquote(text: &String) -> Result<String, String> {
    crate::jet_std::jet_url_unquote(text)
}
fn jet_url_unquote_to_bytes(text: &String) -> Result<Vec<u8>, String> {
    crate::jet_std::jet_url_unquote_to_bytes(text)
}
fn jet_url_unquote_plus(text: &String) -> Result<String, String> {
    crate::jet_std::jet_url_unquote_plus(text)
}
fn jet_url_to_string(url: &crate::jet_std::JetURL) -> String {
    url.to_string_value()
}
fn jet_url_urldefrag(text: &String) -> (String, String) {
    crate::jet_std::jet_url_split_fragment(text)
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
#[inline(always)]
fn jet_decimal_from_str(s: &String) -> jet_std::JetDecimal {
    crate::jet_precise_numeric::decimal_from_str(s)
        .map(crate::jet_std::jet_std_decimal_from_exact)
        .unwrap_or_else(|_| {
            jet_panic(
                "",
                0,
                crate::jet_precise_numeric::failure_message("Decimal", "from_str"),
            )
        })
}

impl __jet_Comparable for jet_std::JetDecimal {
    #[inline(always)]
    fn compare(&self, rhs: &Self) -> __jet_Ordering {
        jet_time_ordering(crate::jet_precise_numeric::decimal_compare(
            &crate::jet_std::jet_std_decimal_to_exact(self),
            &crate::jet_std::jet_std_decimal_to_exact(rhs),
        ))
    }
}

// D-NUMTYPE1=A: exact ratios. Every answer is optional, because a zero bottom
// has no value and a product can leave the range.
fn jet_fraction_new(numerator: i64, denominator: i64) -> Option<jet_std::JetFraction> {
    crate::jet_precise_numeric::fraction_new(
        crate::jet_std::jet_std_raw_to_exact(numerator),
        crate::jet_std::jet_std_raw_to_exact(denominator),
    )
    .map(crate::jet_std::jet_std_fraction_from_exact)
}
fn jet_fraction_from_parts(numerator: i64, denominator: i64) -> jet_std::JetFraction {
    jet_fraction_new(numerator, denominator).unwrap_or_else(|| {
        panic!(
            "{}",
            crate::jet_precise_numeric::failure_message("Fraction", "from_parts")
        )
    })
}
fn jet_fraction_from_owned_parts(
    numerator: &jet_foundation::Numeric::JetInt,
    denominator: &jet_foundation::Numeric::JetInt,
) -> jet_std::JetFraction {
    jet_fraction_from_parts(numerator.to_raw(), denominator.to_raw())
}
fn jet_fraction_add(a: &jet_std::JetFraction, b: &jet_std::JetFraction) -> jet_std::JetFraction {
    let value = crate::jet_precise_numeric::fraction_add(
        &crate::jet_std::jet_std_fraction_to_exact(a),
        &crate::jet_std::jet_std_fraction_to_exact(b),
    )
    .unwrap_or_else(|| {
        panic!(
            "{}",
            crate::jet_precise_numeric::failure_message("Fraction", "add")
        )
    });
    crate::jet_std::jet_std_fraction_from_exact(value)
}
fn jet_fraction_sub(a: &jet_std::JetFraction, b: &jet_std::JetFraction) -> jet_std::JetFraction {
    let value = crate::jet_precise_numeric::fraction_sub(
        &crate::jet_std::jet_std_fraction_to_exact(a),
        &crate::jet_std::jet_std_fraction_to_exact(b),
    )
    .unwrap_or_else(|| {
        panic!(
            "{}",
            crate::jet_precise_numeric::failure_message("Fraction", "sub")
        )
    });
    crate::jet_std::jet_std_fraction_from_exact(value)
}
fn jet_fraction_mul(a: &jet_std::JetFraction, b: &jet_std::JetFraction) -> jet_std::JetFraction {
    let value = crate::jet_precise_numeric::fraction_mul(
        &crate::jet_std::jet_std_fraction_to_exact(a),
        &crate::jet_std::jet_std_fraction_to_exact(b),
    )
    .unwrap_or_else(|| {
        panic!(
            "{}",
            crate::jet_precise_numeric::failure_message("Fraction", "mul")
        )
    });
    crate::jet_std::jet_std_fraction_from_exact(value)
}
fn jet_fraction_div(a: &jet_std::JetFraction, b: &jet_std::JetFraction) -> jet_std::JetFraction {
    let value = crate::jet_precise_numeric::fraction_div(
        &crate::jet_std::jet_std_fraction_to_exact(a),
        &crate::jet_std::jet_std_fraction_to_exact(b),
    )
    .unwrap_or_else(|| {
        panic!(
            "{}",
            crate::jet_precise_numeric::failure_message("Fraction", "div")
        )
    });
    crate::jet_std::jet_std_fraction_from_exact(value)
}
fn jet_fraction_from_int(value: i64) -> jet_std::JetFraction {
    crate::jet_precise_numeric::fraction_from_int(crate::jet_std::jet_std_raw_to_exact(value))
        .map(crate::jet_std::jet_std_fraction_from_exact)
        .unwrap_or_else(|| {
            panic!(
                "{}",
                crate::jet_precise_numeric::failure_message("Fraction", "from_int")
            )
        })
}
fn jet_fraction_from_float(value: f64) -> jet_std::JetFraction {
    crate::jet_precise_numeric::fraction_from_float(value)
        .map(crate::jet_std::jet_std_fraction_from_exact)
        .unwrap_or_else(|| {
            panic!(
                "{}",
                crate::jet_precise_numeric::failure_message("Fraction", "from_float")
            )
        })
}
fn jet_fraction_from_decimal(value: jet_std::JetDecimal) -> jet_std::JetFraction {
    crate::jet_precise_numeric::fraction_from_decimal(
        &crate::jet_std::jet_std_decimal_to_exact(&value),
    )
    .map(crate::jet_std::jet_std_fraction_from_exact)
    .unwrap_or_else(|| {
        panic!(
            "{}",
            crate::jet_precise_numeric::failure_message("Fraction", "from_decimal")
        )
    })
}
fn jet_fraction_equal(a: &jet_std::JetFraction, b: &jet_std::JetFraction) -> bool {
    crate::jet_precise_numeric::fraction_equal(
        &crate::jet_std::jet_std_fraction_to_exact(a),
        &crate::jet_std::jet_std_fraction_to_exact(b),
    )
}
fn jet_fraction_numerator(a: &jet_std::JetFraction) -> i64 {
    crate::jet_std::jet_std_exact_to_raw(crate::jet_precise_numeric::fraction_numerator(
        &crate::jet_std::jet_std_fraction_to_exact(a),
    ))
}
fn jet_fraction_denominator(a: &jet_std::JetFraction) -> i64 {
    crate::jet_std::jet_std_exact_to_raw(crate::jet_precise_numeric::fraction_denominator(
        &crate::jet_std::jet_std_fraction_to_exact(a),
    ))
}
fn jet_fraction_to_string(a: &jet_std::JetFraction) -> String {
    crate::jet_precise_numeric::fraction_to_string(
        &crate::jet_std::jet_std_fraction_to_exact(a),
    )
}
fn jet_fraction_to_float(a: &jet_std::JetFraction) -> f64 {
    crate::jet_precise_numeric::fraction_to_float(
        &crate::jet_std::jet_std_fraction_to_exact(a),
    )
}
fn jet_fraction_is_zero(a: &jet_std::JetFraction) -> bool {
    crate::jet_precise_numeric::fraction_is_zero(
        &crate::jet_std::jet_std_fraction_to_exact(a),
    )
}
fn jet_fraction_to_int(a: &jet_std::JetFraction) -> i64 {
    let value = crate::jet_precise_numeric::fraction_to_int(
        &crate::jet_std::jet_std_fraction_to_exact(a),
    )
    .unwrap_or_else(|| {
        panic!(
            "{}",
            crate::jet_precise_numeric::failure_message("Fraction", "to_int")
        )
    });
    crate::jet_std::jet_std_exact_to_raw(value)
}
fn jet_fraction_to_decimal(a: &jet_std::JetFraction) -> jet_std::JetDecimal {
    crate::jet_precise_numeric::fraction_to_decimal(
        &crate::jet_std::jet_std_fraction_to_exact(a),
    )
    .map(crate::jet_std::jet_std_decimal_from_exact)
    .unwrap_or_else(|| {
        panic!(
            "{}",
            crate::jet_precise_numeric::failure_message("Fraction", "to_decimal")
        )
    })
}
fn jet_decimal_from_int(value: i64) -> jet_std::JetDecimal {
    crate::jet_std::jet_std_decimal_from_exact(crate::jet_precise_numeric::decimal_from_int(
        crate::jet_std::jet_std_raw_to_exact(value),
    ))
}
fn jet_decimal_from_float(value: f64) -> jet_std::JetDecimal {
    crate::jet_precise_numeric::decimal_from_float(value)
        .map(crate::jet_std::jet_std_decimal_from_exact)
        .unwrap_or_else(|| {
            panic!(
                "{}",
                crate::jet_precise_numeric::failure_message("Decimal", "from_float")
            )
        })
}
fn jet_decimal_from_fraction(value: jet_std::JetFraction) -> jet_std::JetDecimal {
    crate::jet_precise_numeric::decimal_from_fraction(
        &crate::jet_std::jet_std_fraction_to_exact(&value),
    )
    .map(crate::jet_std::jet_std_decimal_from_exact)
    .unwrap_or_else(|| {
        panic!(
            "{}",
            crate::jet_precise_numeric::failure_message("Decimal", "from_fraction")
        )
    })
}
fn jet_decimal_div(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetFraction {
    crate::jet_precise_numeric::decimal_div(
        &crate::jet_std::jet_std_decimal_to_exact(a),
        &crate::jet_std::jet_std_decimal_to_exact(b),
    )
    .map(crate::jet_std::jet_std_fraction_from_exact)
    .unwrap_or_else(|| {
        panic!(
            "{}",
            crate::jet_precise_numeric::failure_message("Decimal", "div")
        )
    })
}
fn jet_decimal_round(a: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    crate::jet_std::jet_std_decimal_from_exact(crate::jet_precise_numeric::decimal_round(
        &crate::jet_std::jet_std_decimal_to_exact(a),
    ))
}
fn jet_decimal_floor(a: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    crate::jet_std::jet_std_decimal_from_exact(crate::jet_precise_numeric::decimal_floor(
        &crate::jet_std::jet_std_decimal_to_exact(a),
    ))
}
fn jet_decimal_ceil(a: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    crate::jet_std::jet_std_decimal_from_exact(crate::jet_precise_numeric::decimal_ceil(
        &crate::jet_std::jet_std_decimal_to_exact(a),
    ))
}
fn jet_decimal_to_int(a: &jet_std::JetDecimal) -> i64 {
    let value = crate::jet_precise_numeric::decimal_to_int(
        &crate::jet_std::jet_std_decimal_to_exact(a),
    )
    .unwrap_or_else(|| {
        panic!(
            "{}",
            crate::jet_precise_numeric::failure_message("Decimal", "to_int")
        )
    });
    crate::jet_std::jet_std_exact_to_raw(value)
}
fn jet_decimal_to_fraction(a: &jet_std::JetDecimal) -> jet_std::JetFraction {
    crate::jet_precise_numeric::decimal_to_fraction(
        &crate::jet_std::jet_std_decimal_to_exact(a),
    )
    .map(crate::jet_std::jet_std_fraction_from_exact)
    .unwrap_or_else(|| {
        panic!(
            "{}",
            crate::jet_precise_numeric::failure_message("Decimal", "to_fraction")
        )
    })
}
#[inline(always)]
fn jet_decimal_add(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    crate::jet_std::jet_std_decimal_from_exact(crate::jet_precise_numeric::decimal_add(
        &crate::jet_std::jet_std_decimal_to_exact(a),
        &crate::jet_std::jet_std_decimal_to_exact(b),
    ))
}
#[inline(always)]
fn jet_decimal_sub(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    crate::jet_std::jet_std_decimal_from_exact(crate::jet_precise_numeric::decimal_sub(
        &crate::jet_std::jet_std_decimal_to_exact(a),
        &crate::jet_std::jet_std_decimal_to_exact(b),
    ))
}
fn jet_decimal_mul(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> jet_std::JetDecimal {
    crate::jet_std::jet_std_decimal_from_exact(crate::jet_precise_numeric::decimal_mul(
        &crate::jet_std::jet_std_decimal_to_exact(a),
        &crate::jet_std::jet_std_decimal_to_exact(b),
    ))
}
fn jet_decimal_equal(a: &jet_std::JetDecimal, b: &jet_std::JetDecimal) -> bool {
    crate::jet_precise_numeric::decimal_equal(
        &crate::jet_std::jet_std_decimal_to_exact(a),
        &crate::jet_std::jet_std_decimal_to_exact(b),
    )
}
#[inline(always)]
fn jet_decimal_compare(
    a: &jet_std::JetDecimal,
    b: &jet_std::JetDecimal,
) -> __jet_Ordering {
    jet_time_ordering(crate::jet_precise_numeric::decimal_compare(
        &crate::jet_std::jet_std_decimal_to_exact(a),
        &crate::jet_std::jet_std_decimal_to_exact(b),
    ))
}
#[inline(always)]
fn jet_decimal_to_string(a: &jet_std::JetDecimal) -> String {
    crate::jet_precise_numeric::decimal_to_string(
        &crate::jet_std::jet_std_decimal_to_exact(a),
    )
}
// D-TYPE2-DEFAULT1=A: the AOT half of the exact-to-approximate crossing. The
// checker admits an exact Decimal at the irrational-result math functions
// exactly as it admits a Fraction, so every tier needs this conversion, not
// only the evaluator.
#[inline(always)]
fn jet_decimal_to_float(a: &jet_std::JetDecimal) -> f64 {
    crate::jet_precise_numeric::decimal_to_float(
        &crate::jet_std::jet_std_decimal_to_exact(a),
    )
}

// D-ENC-DYN1=A+: the dynamic `parse` returns the one rich `Data` value (the
// user-facing face of `DataTree`). JSON text parses directly to the canonical
// ordered `DataTree` carrier.
fn jet_std_json_parse(text: &String) -> Result<jet_std::DataTree, jet_std::EncodingError> {
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
fn jet_std_jsonl_parse(text: &String) -> Result<Vec<jet_std::DataTree>, jet_std::EncodingError> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match jet_std_json_parse(&trimmed.to_string()) {
            Ok(v) => out.push(v),
            Err(mut error) => {
                error.format = jet_std::EncodingFormat::JSONL;
                error.line = error.line.map(|line| idx as i64 + line);
                return Err(error);
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
fn jet_std_jsonl_count_rows(text: &String) -> jet_foundation::Numeric::JetInt {
    jet_std::jet_int_owned_from_i64(
        text.lines().filter(|line| !line.trim().is_empty()).count() as i64,
    )
}
fn jet_std_jsonl_append_line(text: &String, value: &jet_std::DataTree) -> String {
    let rendered = jet_std_json_render_canonical(value);
    if text.is_empty() {
        return format!("{rendered}\n");
    }
    if text.ends_with('\n') {
        format!("{text}{rendered}\n")
    } else {
        format!("{text}\n{rendered}\n")
    }
}
fn jet_std_jsonl_first(
    text: &String,
) -> Result<Option<jet_std::DataTree>, jet_std::EncodingError> {
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match jet_std_json_parse(&trimmed.to_string()) {
            Ok(value) => return Ok(Some(value)),
            Err(mut error) => {
                error.format = jet_std::EncodingFormat::JSONL;
                error.line = error.line.map(|line| idx as i64 + line);
                return Err(error);
            }
        }
    }
    Ok(None)
}

// D-JSON1-decode + D-JSON3: lenient JSON decode with coercion surfacing. The
// walk, the coercion message and the audit-line shape are ONE policy in
// `CoreLib/JetStd/JSONDataTree.rs`, shared with the resident JIT host that
// used to carry a byte-equivalent copy (I8/I9). AOT supplies only the sink it
// owns: the process's own stderr.
fn jet_std_json_decode_lenient(text: &String) -> Result<jet_std::DataTree, jet_std::EncodingError> {
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
