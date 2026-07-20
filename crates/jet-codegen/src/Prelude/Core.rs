trait JetShow {
    fn jet_show(&self) -> String;
}
/// D-DISPLAYDBG1: user-facing interpolation (`{value}`).
trait JetDisplay {
    fn jet_display(&self) -> String;
}
/// D-DISPLAYDBG1: developer interpolation (`{value@Debug}`).
trait JetDebug {
    fn jet_debug(&self) -> String;
}

/// D-QUANTITY-TYPE1=A: internal compile-time bridge for physical-unit generic
/// bounds. Concrete unit identity stays in the
/// monomorphized type; this trait adds no runtime metadata.
trait JetQuantity: Sized {
    fn raw(&self) -> f64;
    fn from_float(value: f64) -> Self;
}

// D-SHAPE-RESOURCE2=A: scope-owned deferred close. `FnOnce` lives in Option so
// Drop consumes it exactly once; declaration order gives reverse cleanup order.
struct JetDeferredClose<F: FnOnce()> {
    close: Option<F>,
}
impl<F: FnOnce()> JetDeferredClose<F> {
    fn new(close: F) -> Self {
        Self { close: Some(close) }
    }
    fn run(&mut self) {
        if let Some(close) = self.close.take() {
            close();
        }
    }
}
impl<F: FnOnce()> Drop for JetDeferredClose<F> {
    fn drop(&mut self) {
        self.run();
    }
}

// D-PROVENANCE1=B: `@Track x :: <Float>` records local Float provenance by
// address. Plain copies remain plain values; a copied Float is untracked unless
// rebound under `@Track`.
static JET_FLOAT_ORIGINS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<usize, String>>,
> = std::sync::OnceLock::new();

fn jet_float_origins() -> &'static std::sync::Mutex<std::collections::HashMap<usize, String>> {
    JET_FLOAT_ORIGINS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn jet_track_float_origin(value: &f64, origin: &str) {
    if let Ok(mut origins) = jet_float_origins().lock() {
        origins.insert(value as *const f64 as usize, origin.to_string());
    }
}

fn jet_float_origin(value: &f64) -> String {
    jet_float_origins()
        .lock()
        .ok()
        .and_then(|origins| origins.get(&(value as *const f64 as usize)).cloned())
        .unwrap_or_else(|| "untracked".to_string())
}

macro_rules! jet_scalar_show {
    ($($t:ty),+ $(,)?) => {$(
        impl JetShow for $t { fn jet_show(&self) -> String { self.to_string() } }
        impl JetDisplay for $t { fn jet_display(&self) -> String { self.to_string() } }
        impl JetDebug for $t { fn jet_debug(&self) -> String { self.to_string() } }
    )+};
}
jet_scalar_show!(i64, i8, i16, i32, u8, u16, u32, u64, bool, char);
impl JetShow for f32 {
    fn jet_show(&self) -> String {
        format!("{:?}", self)
    }
}
impl JetDisplay for f32 {
    fn jet_display(&self) -> String {
        format!("{:?}", self)
    }
}
impl JetDebug for f32 {
    fn jet_debug(&self) -> String {
        format!("{:?}", self)
    }
}
impl JetShow for f64 {
    fn jet_show(&self) -> String {
        format!("{:?}", self)
    }
}
impl JetDisplay for f64 {
    fn jet_display(&self) -> String {
        format!("{:?}", self)
    }
}
impl JetDebug for f64 {
    fn jet_debug(&self) -> String {
        format!("{:?}", self)
    }
}
impl JetShow for String {
    fn jet_show(&self) -> String {
        self.clone()
    }
}
impl JetDisplay for String {
    fn jet_display(&self) -> String {
        self.clone()
    }
}
impl JetDebug for String {
    fn jet_debug(&self) -> String {
        format!("{self:?}")
    }
}
// D-MEM1 stage S5: a string view (`s.trim()`/`.after()`/`.before()` bound to a
// local, see `jet_string_*_view` below) is a genuine `&str` in generated Rust —
// `String` stays the one Jet-level type, so anything that already works on a
// `String` (print/interpolate/debug) must also work on the view directly.
impl JetShow for str {
    fn jet_show(&self) -> String {
        self.to_string()
    }
}
impl JetDisplay for str {
    fn jet_display(&self) -> String {
        self.to_string()
    }
}
impl JetDebug for str {
    fn jet_debug(&self) -> String {
        format!("{self:?}")
    }
}
impl<T: JetShow> JetShow for &T {
    fn jet_show(&self) -> String {
        (**self).jet_show()
    }
}
impl<T: JetDisplay> JetDisplay for &T {
    fn jet_display(&self) -> String {
        (**self).jet_display()
    }
}
impl<T: JetDebug> JetDebug for &T {
    fn jet_debug(&self) -> String {
        (**self).jet_debug()
    }
}
impl<T: JetShow> JetShow for [T] {
    fn jet_show(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.jet_show()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: JetDisplay> JetDisplay for [T] {
    fn jet_display(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.jet_display()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: JetDebug> JetDebug for [T] {
    fn jet_debug(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.jet_debug()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: JetShow> JetShow for Vec<T> {
    fn jet_show(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.jet_show()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: JetDisplay> JetDisplay for Vec<T> {
    fn jet_display(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.jet_display()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: JetDebug> JetDebug for Vec<T> {
    fn jet_debug(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.jet_debug()).collect();
        format!("[{}]", parts.join(", "))
    }
}
// D-FIXARR1: `[T#N]` lowers to a real Rust array `[T; N]`; render it like a list
// so printing/interpolating a fixed array (or a fan-out result) works.
impl<T: JetShow, const N: usize> JetShow for [T; N] {
    fn jet_show(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.jet_show()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: JetDisplay, const N: usize> JetDisplay for [T; N] {
    fn jet_display(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.jet_display()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: JetDebug, const N: usize> JetDebug for [T; N] {
    fn jet_debug(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.jet_debug()).collect();
        format!("[{}]", parts.join(", "))
    }
}
// D-COLLBREADTH1=A: Set<T> (HashSet) shows lexicographically sorted like a list;
// Deque<T> shows in order like a list. Sort by string rep for determinism.
impl<T: JetShow> JetShow for std::collections::HashSet<T> {
    fn jet_show(&self) -> String {
        let mut parts: Vec<String> = self.iter().map(|x| x.jet_show()).collect();
        parts.sort();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: JetDisplay> JetDisplay for std::collections::HashSet<T> {
    fn jet_display(&self) -> String {
        let mut parts: Vec<String> = self.iter().map(|x| x.jet_display()).collect();
        parts.sort();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: JetDebug> JetDebug for std::collections::HashSet<T> {
    fn jet_debug(&self) -> String {
        let mut parts: Vec<String> = self.iter().map(|x| x.jet_debug()).collect();
        parts.sort();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: Ord + JetShow> JetShow for std::collections::BTreeSet<T> {
    fn jet_show(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.jet_show()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: Ord + JetDisplay> JetDisplay for std::collections::BTreeSet<T> {
    fn jet_display(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.jet_display()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: Ord + JetDebug> JetDebug for std::collections::BTreeSet<T> {
    fn jet_debug(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.jet_debug()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: Ord + Clone + JetShow> JetShow for std::collections::BinaryHeap<T> {
    fn jet_show(&self) -> String {
        let parts: Vec<String> = self.clone().into_sorted_vec().into_iter().rev().map(|x| x.jet_show()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: Ord + Clone + JetDisplay> JetDisplay for std::collections::BinaryHeap<T> {
    fn jet_display(&self) -> String {
        let parts: Vec<String> = self.clone().into_sorted_vec().into_iter().rev().map(|x| x.jet_display()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: Ord + Clone + JetDebug> JetDebug for std::collections::BinaryHeap<T> {
    fn jet_debug(&self) -> String {
        let parts: Vec<String> = self.clone().into_sorted_vec().into_iter().rev().map(|x| x.jet_debug()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: JetShow> JetShow for std::collections::VecDeque<T> {
    fn jet_show(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.jet_show()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: JetDisplay> JetDisplay for std::collections::VecDeque<T> {
    fn jet_display(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.jet_display()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: JetDebug> JetDebug for std::collections::VecDeque<T> {
    fn jet_debug(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.jet_debug()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<K: Ord + JetShow, V: JetShow> JetShow for std::collections::BTreeMap<K, V> {
    fn jet_show(&self) -> String {
        let parts: Vec<String> = self
            .iter()
            .map(|(k, v)| format!("{}: {}", k.jet_show(), v.jet_show()))
            .collect();
        format!("[:{}]", parts.join(", "))
    }
}
impl<K: Ord + JetDisplay, V: JetDisplay> JetDisplay for std::collections::BTreeMap<K, V> {
    fn jet_display(&self) -> String {
        let parts: Vec<String> = self
            .iter()
            .map(|(k, v)| format!("{}: {}", k.jet_display(), v.jet_display()))
            .collect();
        format!("[:{}]", parts.join(", "))
    }
}
impl<K: Ord + JetDebug, V: JetDebug> JetDebug for std::collections::BTreeMap<K, V> {
    fn jet_debug(&self) -> String {
        let parts: Vec<String> = self
            .iter()
            .map(|(k, v)| format!("{}: {}", k.jet_debug(), v.jet_debug()))
            .collect();
        format!("[:{}]", parts.join(", "))
    }
}
impl<T: JetShow> JetShow for Option<T> {
    fn jet_show(&self) -> String {
        match self {
            Some(v) => v.jet_show(),
            None => "null".to_string(),
        }
    }
}
impl<T: JetDisplay> JetDisplay for Option<T> {
    fn jet_display(&self) -> String {
        match self {
            Some(v) => v.jet_display(),
            None => "null".to_string(),
        }
    }
}
impl<T: JetDebug> JetDebug for Option<T> {
    fn jet_debug(&self) -> String {
        match self {
            Some(v) => v.jet_debug(),
            None => "null".to_string(),
        }
    }
}
impl<T: JetShow, E: JetShow> JetShow for Result<T, E> {
    fn jet_show(&self) -> String {
        match self {
            Ok(v) => format!("Ok({})", v.jet_show()),
            Err(e) => format!("Err({})", e.jet_show()),
        }
    }
}
impl<T: JetDisplay, E: JetDisplay> JetDisplay for Result<T, E> {
    fn jet_display(&self) -> String {
        match self {
            Ok(v) => format!("Ok({})", v.jet_display()),
            Err(e) => format!("Err({})", e.jet_display()),
        }
    }
}
impl<T: JetDebug, E: JetDebug> JetDebug for Result<T, E> {
    fn jet_debug(&self) -> String {
        match self {
            Ok(v) => format!("Ok({})", v.jet_debug()),
            Err(e) => format!("Err({})", e.jet_debug()),
        }
    }
}
// D-PENDING1=B: async UI state machine — Idle/Loading/Loaded(T)/Failed(E).
#[derive(Clone, Debug)]
enum JetLoadable<T: Clone, E: Clone> {
    Idle,
    Loading,
    Loaded(T),
    Failed(E),
}
impl<T: Clone, E: Clone> JetLoadable<T, E> {
    fn is_idle(&self) -> bool {
        matches!(self, JetLoadable::Idle)
    }
    fn is_loading(&self) -> bool {
        matches!(self, JetLoadable::Loading)
    }
    fn is_loaded(&self) -> bool {
        matches!(self, JetLoadable::Loaded(_))
    }
    fn is_failed(&self) -> bool {
        matches!(self, JetLoadable::Failed(_))
    }
    fn loaded(&self) -> Option<T> {
        if let JetLoadable::Loaded(v) = self {
            Some(v.clone())
        } else {
            None
        }
    }
    fn or_else(&self, default: T) -> T {
        if let JetLoadable::Loaded(v) = self {
            v.clone()
        } else {
            default
        }
    }
}
impl<T: Clone + JetShow, E: Clone + JetShow> JetShow for JetLoadable<T, E> {
    fn jet_show(&self) -> String {
        match self {
            JetLoadable::Idle => "Idle".to_string(),
            JetLoadable::Loading => "Loading".to_string(),
            JetLoadable::Loaded(v) => format!("Loaded({})", v.jet_show()),
            JetLoadable::Failed(e) => format!("Failed({})", e.jet_show()),
        }
    }
}
// D-TTLVAL1=A / D-CRYPTOENV1 c64: TTL-wrapped values and rotting secrets.
#[derive(Clone, Debug)]
struct JetExpired;
impl JetShow for JetExpired {
    fn jet_show(&self) -> String {
        "Expired".to_string()
    }
}
#[derive(Clone, Debug)]
struct JetExpiring<T: Clone> {
    value: T,
    deadline_ms: i64,
}
impl<T: Clone> JetExpiring<T> {
    fn new(value: T, deadline_ms: i64) -> Self {
        JetExpiring { value, deadline_ms }
    }
    fn get(&self, now_ms: i64) -> Result<T, JetExpired> {
        if now_ms > self.deadline_ms {
            Err(JetExpired)
        } else {
            Ok(self.value.clone())
        }
    }
    fn is_valid(&self, now_ms: i64) -> bool {
        now_ms <= self.deadline_ms
    }
}
impl<T: Clone + JetShow> JetShow for JetExpiring<T> {
    fn jet_show(&self) -> String {
        format!("Expiring(deadline={})", self.deadline_ms)
    }
}
#[derive(Clone, Debug)]
struct JetRotting<T: Clone> {
    value: T,
    deadline_ms: i64,
    consumed: bool,
}
impl<T: Clone + 'static> JetRotting<T> {
    fn new(value: T, deadline_ms: i64) -> Self {
        JetRotting {
            value,
            deadline_ms,
            consumed: false,
        }
    }
    fn get(&mut self, now_ms: i64) -> Result<T, JetExpired> {
        if self.consumed || now_ms > self.deadline_ms {
            self.zeroize();
            Err(JetExpired)
        } else {
            let v = self.value.clone();
            self.zeroize();
            Ok(v)
        }
    }
    fn zeroize(&mut self) {
        self.consumed = true;
        if let Some(s) = (&mut self.value as &mut dyn std::any::Any).downcast_mut::<String>() {
            s.clear();
        } else if let Some(v) =
            (&mut self.value as &mut dyn std::any::Any).downcast_mut::<Vec<u8>>()
        {
            for b in v.iter_mut() {
                *b = 0;
            }
            v.clear();
        }
    }
}
impl<T: Clone + JetShow> JetShow for JetRotting<T> {
    fn jet_show(&self) -> String {
        format!("Rotting(deadline={})", self.deadline_ms)
    }
}
// D-TIMEDEPTH1/D-TIME-CALENDAR1: civil-time types and calendar math.
// Pure Rust, no external crates (I6). Proleptic Gregorian calendar, Unix time
// as UTC seconds, and a small TZif reader for IANA zoneinfo files.
#[derive(Clone, Debug, PartialEq)]
struct JetDate {
    year: i64,
    month: i64,
    day: i64,
}
impl JetDate {
    fn is_leap(y: i64) -> bool {
        (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
    }
    fn days_in_month(y: i64, m: i64) -> i64 {
        match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if Self::is_leap(y) {
                    29
                } else {
                    28
                }
            }
            _ => 30,
        }
    }
    fn new(y: i64, m: i64, d: i64) -> Self {
        let month = m.clamp(1, 12);
        let day = d.clamp(1, Self::days_in_month(y, month));
        JetDate {
            year: y,
            month,
            day,
        }
    }
    // Days since 0001-01-01 (proleptic Gregorian).
    fn to_day_number(&self) -> i64 {
        let y = self.year - 1;
        365 * y + y / 4 - y / 100
            + y / 400
            + [0i64, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334][(self.month - 1) as usize]
            + if self.month > 2 && Self::is_leap(self.year) {
                1
            } else {
                0
            }
            + self.day
            - 1
    }
    fn from_day_number(mut n: i64) -> Self {
        let mut y = n / 365 + 1;
        loop {
            let start = JetDate::new(y, 1, 1).to_day_number();
            let next = JetDate::new(y + 1, 1, 1).to_day_number();
            if n >= start && n < next {
                break;
            }
            if n < start {
                y -= 1;
            } else {
                y += 1;
            }
        }
        n -= JetDate::new(y, 1, 1).to_day_number();
        let mut m = 1i64;
        while m < 12 && n >= Self::days_in_month(y, m) {
            n -= Self::days_in_month(y, m);
            m += 1;
        }
        JetDate::new(y, m, n + 1)
    }
    fn parse(s: &str) -> Result<JetDate, String> {
        let parts: Vec<&str> = s.splitn(3, '-').collect();
        if parts.len() != 3 {
            return Err(format!("invalid date: {}", s));
        }
        let y = parts[0]
            .parse::<i64>()
            .map_err(|_| format!("bad year: {}", parts[0]))?;
        let m = parts[1]
            .parse::<i64>()
            .map_err(|_| format!("bad month: {}", parts[1]))?;
        let d = parts[2]
            .parse::<i64>()
            .map_err(|_| format!("bad day: {}", parts[2]))?;
        if m < 1 || m > 12 || d < 1 || d > Self::days_in_month(y, m) {
            return Err(format!("date out of range: {}", s));
        }
        Ok(JetDate::new(y, m, d))
    }
    fn year(&self) -> i64 {
        self.year
    }
    fn month(&self) -> i64 {
        self.month
    }
    fn day(&self) -> i64 {
        self.day
    }
    fn add_days(&self, n: i64) -> JetDate {
        Self::from_day_number(self.to_day_number() + n)
    }
    fn add_months(&self, n: i64) -> JetDate {
        let total = self.month - 1 + n;
        let y = self.year + total / 12;
        let m = total % 12 + 1;
        let d = self.day.min(Self::days_in_month(y, m));
        JetDate::new(y, m, d)
    }
    fn diff_days(&self, other: &JetDate) -> i64 {
        self.to_day_number() - other.to_day_number()
    }
    fn weekday(&self) -> i64 {
        // Legacy D-TIMEDEPTH1 shape: 0=Sunday, 6=Saturday.
        (self.to_day_number() + 6) % 7
    }
    fn iso_weekday(&self) -> i64 {
        (self.to_day_number() % 7) + 1
    }
    fn day_of_year(&self) -> i64 {
        self.to_day_number() - JetDate::new(self.year, 1, 1).to_day_number() + 1
    }
    fn iso_week(&self) -> i64 {
        let thursday = self.add_days(4 - self.iso_weekday());
        ((thursday.to_day_number() - JetDate::new(thursday.year, 1, 1).to_day_number()) / 7) + 1
    }
    fn truncate(&self, unit: &String) -> JetDate {
        match unit.as_str() {
            "year" => JetDate::new(self.year, 1, 1),
            "month" => JetDate::new(self.year, self.month, 1),
            _ => self.clone(),
        }
    }
    fn add_period(&self, p: &JetPeriod) -> JetDate {
        self.add_months(p.years.saturating_mul(12).saturating_add(p.months))
            .add_days(p.days)
    }
    fn format_pattern(&self, pattern: &String) -> String {
        jet_time_format_pattern(pattern, self, &JetLocalTime::new(0, 0, 0), None)
    }
    fn to_string_fmt(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
    fn today_utc() -> JetDate {
        // Seconds since Unix epoch ÷ 86400 days.
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;
        let days_since_1970 = secs / 86400;
        let epoch = JetDate::new(1970, 1, 1).to_day_number();
        JetDate::from_day_number(epoch + days_since_1970)
    }
}
impl JetShow for JetDate {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct JetLocalTime {
    hour: i64,
    minute: i64,
    second: i64,
}
impl JetLocalTime {
    fn new(hour: i64, minute: i64, second: i64) -> Self {
        JetLocalTime {
            hour: hour.clamp(0, 23),
            minute: minute.clamp(0, 59),
            second: second.clamp(0, 59),
        }
    }
    fn parse(s: &str) -> Result<JetLocalTime, String> {
        let parts: Vec<&str> = s.splitn(3, ':').collect();
        if parts.len() != 3 {
            return Err(format!("invalid time: {}", s));
        }
        let h = parts[0]
            .parse::<i64>()
            .map_err(|_| format!("bad hour: {}", parts[0]))?;
        let m = parts[1]
            .parse::<i64>()
            .map_err(|_| format!("bad minute: {}", parts[1]))?;
        let sec = parts[2]
            .parse::<i64>()
            .map_err(|_| format!("bad second: {}", parts[2]))?;
        if h < 0 || h > 23 || m < 0 || m > 59 || sec < 0 || sec > 59 {
            return Err(format!("time out of range: {}", s));
        }
        Ok(Self::new(h, m, sec))
    }
    fn hour(&self) -> i64 {
        self.hour
    }
    fn minute(&self) -> i64 {
        self.minute
    }
    fn second(&self) -> i64 {
        self.second
    }
    fn to_seconds(&self) -> i64 {
        self.hour * 3600 + self.minute * 60 + self.second
    }
    fn to_string_fmt(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}
impl JetShow for JetLocalTime {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct JetPeriod {
    years: i64,
    months: i64,
    days: i64,
}
impl JetPeriod {
    fn new(years: i64, months: i64, days: i64) -> Self {
        JetPeriod {
            years,
            months,
            days,
        }
    }
    fn days(n: i64) -> Self {
        Self::new(0, 0, n)
    }
    fn months(n: i64) -> Self {
        Self::new(0, n, 0)
    }
    fn years(n: i64) -> Self {
        Self::new(n, 0, 0)
    }
    fn to_string_fmt(&self) -> String {
        format!("P{}Y{}M{}D", self.years, self.months, self.days)
    }
}
impl JetShow for JetPeriod {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

#[derive(Clone, Debug)]
struct JetInstant {
    start: std::time::Instant,
}
impl JetInstant {
    fn now() -> Self {
        JetInstant {
            start: std::time::Instant::now(),
        }
    }
    fn elapsed_millis(&self) -> i64 {
        self.start.elapsed().as_millis() as i64
    }
}
impl PartialEq for JetInstant {
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start
    }
}
impl JetShow for JetInstant {
    fn jet_show(&self) -> String {
        "Instant".to_string()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct JetDateTime {
    secs: i64,
} // seconds since Unix epoch (UTC)
impl JetDateTime {
    fn from_timestamp(secs: i64) -> Self {
        JetDateTime { secs }
    }
    fn now() -> Self {
        let s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;
        JetDateTime { secs: s }
    }
    fn date(&self) -> JetDate {
        let days = self.secs.div_euclid(86400);
        let epoch = JetDate::new(1970, 1, 1).to_day_number();
        JetDate::from_day_number(epoch + days)
    }
    fn time(&self) -> JetLocalTime {
        let sec = self.secs.rem_euclid(86400);
        JetLocalTime::new(sec / 3600, (sec / 60) % 60, sec % 60)
    }
    fn hour(&self) -> i64 {
        self.time().hour
    }
    fn minute(&self) -> i64 {
        self.time().minute
    }
    fn second(&self) -> i64 {
        self.secs.rem_euclid(60)
    }
    fn to_timestamp(&self) -> i64 {
        self.secs
    }
    fn to_unix_ms(&self) -> i64 {
        self.secs.saturating_mul(1000)
    }
    fn from_unix_ms(ms: i64) -> Self {
        JetDateTime {
            secs: ms.div_euclid(1000),
        }
    }
    fn parse_rfc3339(s: &str) -> Result<Self, String> {
        let (date_part, rest) = s
            .split_once('T')
            .ok_or_else(|| format!("invalid RFC3339 datetime: {}", s))?;
        let date = JetDate::parse(date_part)?;
        let zone_pos = rest
            .find('Z')
            .or_else(|| rest.rfind('+'))
            .or_else(|| {
                rest.get(1..)
                    .and_then(|tail| tail.rfind('-').map(|i| i + 1))
            })
            .ok_or_else(|| format!("RFC3339 datetime needs Z or an offset: {}", s))?;
        let (time_part, zone_part) = rest.split_at(zone_pos);
        let clean_time = time_part.split('.').next().unwrap_or(time_part);
        let time = JetLocalTime::parse(clean_time)?;
        let offset = if zone_part == "Z" {
            0
        } else {
            let sign = if zone_part.starts_with('-') { -1 } else { 1 };
            let z = &zone_part[1..];
            let (hh, mm) = z
                .split_once(':')
                .ok_or_else(|| format!("bad RFC3339 offset: {}", zone_part))?;
            let h = hh
                .parse::<i64>()
                .map_err(|_| format!("bad RFC3339 offset hour: {}", hh))?;
            let m = mm
                .parse::<i64>()
                .map_err(|_| format!("bad RFC3339 offset minute: {}", mm))?;
            sign * (h * 3600 + m * 60)
        };
        Ok(JetDateTime {
            secs: jet_time_utc_from_parts(&date, &time) - offset,
        })
    }
    fn format_rfc3339(&self) -> String {
        let d = self.date();
        let t = self.time();
        format!("{}T{}Z", d.to_string_fmt(), t.to_string_fmt())
    }
    fn format_pattern(&self, pattern: &String) -> String {
        jet_time_format_pattern(pattern, &self.date(), &self.time(), None)
    }
    fn plus_duration_ms(&self, ms: i64) -> JetDateTime {
        JetDateTime {
            secs: self.secs.saturating_add(ms.div_euclid(1000)),
        }
    }
    fn truncate(&self, unit: &String) -> JetDateTime {
        let size = match unit.as_str() {
            "day" => 86400,
            "hour" => 3600,
            "minute" => 60,
            _ => 1,
        };
        JetDateTime {
            secs: self.secs.div_euclid(size) * size,
        }
    }
    fn round(&self, unit: &String) -> JetDateTime {
        let size = match unit.as_str() {
            "day" => 86400,
            "hour" => 3600,
            "minute" => 60,
            _ => 1,
        };
        JetDateTime {
            secs: self.secs.saturating_add(size / 2).div_euclid(size) * size,
        }
    }
    fn in_zone(&self, zone: &JetZone) -> JetZonedDateTime {
        JetZonedDateTime {
            instant: self.clone(),
            zone: zone.clone(),
        }
    }
    fn to_string_fmt(&self) -> String {
        let d = self.date();
        format!(
            "{} {:02}:{:02}:{:02} UTC",
            d.to_string_fmt(),
            self.hour(),
            self.minute(),
            self.second()
        )
    }
}
impl JetShow for JetDateTime {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct JetTtInfo {
    offset: i64,
    is_dst: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct JetZone {
    name: String,
    transitions: Vec<(i64, usize)>,
    infos: Vec<JetTtInfo>,
}
impl JetZone {
    fn utc() -> Self {
        JetZone {
            name: "UTC".to_string(),
            transitions: Vec::new(),
            infos: vec![JetTtInfo {
                offset: 0,
                is_dst: false,
            }],
        }
    }
    fn named(name: &String) -> Result<Self, String> {
        if name == "UTC" || name == "Etc/UTC" || name == "Z" {
            return Ok(Self::utc());
        }
        if name.contains("..") || name.starts_with('/') || name.starts_with('\\') {
            return Err(format!("invalid time zone name: {}", name));
        }
        let rel = name
            .trim_start_matches("posix/")
            .trim_start_matches("right/");
        let mut roots = Vec::new();
        if let Some(dir) = std::env::var_os("JET_TZDB_DIR") {
            roots.push(std::path::PathBuf::from(dir));
        }
        if let Some(dir) = std::env::var_os("TZDIR") {
            roots.push(std::path::PathBuf::from(dir));
        }
        if let Some(root) = std::env::var_os("JET_ROOT") {
            roots.push(std::path::PathBuf::from(root).join("corelib/tzdb"));
        }
        roots.push(std::path::PathBuf::from("corelib/tzdb"));
        roots.push(std::path::PathBuf::from("/usr/share/zoneinfo"));
        roots.push(std::path::PathBuf::from("/usr/share/lib/zoneinfo"));
        roots.push(std::path::PathBuf::from("/etc/zoneinfo"));
        for base in roots {
            let path = base.join(rel);
            if let Ok(bytes) = std::fs::read(&path) {
                return Self::parse_tzif(name.clone(), &bytes);
            }
        }
        Err(format!(
            "unknown IANA time zone: {}; set JET_TZDB_DIR or TZDIR to an IANA TZif database",
            name
        ))
    }
    fn parse_tzif(name: String, bytes: &[u8]) -> Result<Self, String> {
        fn be_u32(bytes: &[u8], i: usize) -> Result<u32, String> {
            let chunk = bytes
                .get(i..i + 4)
                .ok_or_else(|| "truncated tzif".to_string())?;
            Ok(u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        }
        fn be_i32(bytes: &[u8], i: usize) -> Result<i32, String> {
            Ok(be_u32(bytes, i)? as i32)
        }
        fn be_i64(bytes: &[u8], i: usize) -> Result<i64, String> {
            let c = bytes
                .get(i..i + 8)
                .ok_or_else(|| "truncated tzif".to_string())?;
            Ok(i64::from_be_bytes([
                c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7],
            ]))
        }
        fn header(bytes: &[u8], base: usize) -> Result<(u8, [usize; 6]), String> {
            if bytes.get(base..base + 4) != Some(b"TZif") {
                return Err("invalid tzif header".to_string());
            }
            let version = *bytes.get(base + 4).unwrap_or(&0);
            let mut counts = [0usize; 6];
            for i in 0..6 {
                counts[i] = be_u32(bytes, base + 20 + i * 4)? as usize;
            }
            Ok((version, counts))
        }
        let (version, c1) = header(bytes, 0)?;
        let block32 = c1[3] * 4 + c1[3] + c1[4] * 6 + c1[5] + c1[2] * 8 + c1[0] + c1[1];
        let mut base = 44;
        let mut wide = false;
        if version == b'2' || version == b'3' || version == b'4' {
            base = 44 + block32;
            let _ = header(bytes, base)?;
            base += 44;
            wide = true;
        }
        let counts = if wide {
            header(bytes, base - 44)?.1
        } else {
            c1
        };
        let timecnt = counts[3];
        let typecnt = counts[4].max(1);
        let time_size = if wide { 8 } else { 4 };
        let mut pos = base;
        let mut times = Vec::new();
        for _ in 0..timecnt {
            let t = if wide {
                be_i64(bytes, pos)?
            } else {
                be_i32(bytes, pos)? as i64
            };
            times.push(t);
            pos += time_size;
        }
        let idxs = bytes
            .get(pos..pos + timecnt)
            .ok_or_else(|| "truncated tzif index".to_string())?;
        pos += timecnt;
        let mut infos = Vec::new();
        for _ in 0..typecnt {
            let offset = be_i32(bytes, pos)? as i64;
            let is_dst = *bytes.get(pos + 4).unwrap_or(&0) != 0;
            infos.push(JetTtInfo { offset, is_dst });
            pos += 6;
        }
        if infos.is_empty() {
            infos.push(JetTtInfo {
                offset: 0,
                is_dst: false,
            });
        }
        let mut transitions = Vec::new();
        for (t, idx) in times.into_iter().zip(idxs.iter().copied()) {
            transitions.push((t, (idx as usize).min(infos.len() - 1)));
        }
        Ok(JetZone {
            name,
            transitions,
            infos,
        })
    }
    fn name(&self) -> String {
        self.name.clone()
    }
    fn offset_at_utc(&self, secs: i64) -> i64 {
        if self.transitions.is_empty() {
            return self.infos[0].offset;
        }
        let mut lo = 0usize;
        let mut hi = self.transitions.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.transitions[mid].0 <= secs {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let idx = if lo == 0 {
            self.transitions.first().map(|(_, i)| *i).unwrap_or(0)
        } else {
            self.transitions[lo - 1].1
        };
        self.infos[idx].offset
    }
    fn local_parts(&self, secs: i64) -> (JetDate, JetLocalTime, i64) {
        let offset = self.offset_at_utc(secs);
        let local = JetDateTime::from_timestamp(secs.saturating_add(offset));
        (local.date(), local.time(), offset)
    }
    fn local_to_utc(&self, date: &JetDate, time: &JetLocalTime) -> i64 {
        let mut guess = jet_time_utc_from_parts(date, time);
        for _ in 0..4 {
            let next =
                jet_time_utc_from_parts(date, time).saturating_sub(self.offset_at_utc(guess));
            if next == guess {
                break;
            }
            guess = next;
        }
        guess
    }
}
impl JetShow for JetZone {
    fn jet_show(&self) -> String {
        self.name.clone()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct JetZonedDateTime {
    instant: JetDateTime,
    zone: JetZone,
}
impl JetZonedDateTime {
    fn now(zone: &JetZone) -> Self {
        JetDateTime::now().in_zone(zone)
    }
    fn from_local(date: &JetDate, time: &JetLocalTime, zone: &JetZone) -> Self {
        JetZonedDateTime {
            instant: JetDateTime::from_timestamp(zone.local_to_utc(date, time)),
            zone: zone.clone(),
        }
    }
    fn date(&self) -> JetDate {
        self.zone.local_parts(self.instant.secs).0
    }
    fn time(&self) -> JetLocalTime {
        self.zone.local_parts(self.instant.secs).1
    }
    fn offset_seconds(&self) -> i64 {
        self.zone.local_parts(self.instant.secs).2
    }
    fn to_datetime(&self) -> JetDateTime {
        self.instant.clone()
    }
    fn zone(&self) -> JetZone {
        self.zone.clone()
    }
    fn add_duration_ms(&self, ms: i64) -> JetZonedDateTime {
        JetZonedDateTime {
            instant: self.instant.plus_duration_ms(ms),
            zone: self.zone.clone(),
        }
    }
    fn add_period(&self, p: &JetPeriod) -> JetZonedDateTime {
        let date = self.date().add_period(p);
        let time = self.time();
        JetZonedDateTime::from_local(&date, &time, &self.zone)
    }
    fn format_pattern(&self, pattern: &String) -> String {
        let date = self.date();
        let time = self.time();
        jet_time_format_pattern(
            pattern,
            &date,
            &time,
            Some((&self.zone, self.offset_seconds())),
        )
    }
    fn to_string_fmt(&self) -> String {
        let off = self.offset_seconds();
        format!(
            "{} {} {} ({})",
            self.date().to_string_fmt(),
            self.time().to_string_fmt(),
            self.zone.name,
            jet_time_offset_string(off)
        )
    }
}
impl JetShow for JetZonedDateTime {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

fn jet_time_utc_from_parts(date: &JetDate, time: &JetLocalTime) -> i64 {
    let epoch = JetDate::new(1970, 1, 1).to_day_number();
    (date.to_day_number() - epoch)
        .saturating_mul(86400)
        .saturating_add(time.to_seconds())
}

fn jet_time_offset_string(offset: i64) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let abs = offset.abs();
    format!("{}{:02}:{:02}", sign, abs / 3600, (abs / 60) % 60)
}

fn jet_time_format_pattern(
    pattern: &String,
    date: &JetDate,
    time: &JetLocalTime,
    zone: Option<(&JetZone, i64)>,
) -> String {
    let mut out = pattern.clone();
    let weekday =
        ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][(date.iso_weekday() - 1) as usize];
    out = out.replace("yyyy", &format!("{:04}", date.year));
    out = out.replace("DDD", &format!("{:03}", date.day_of_year()));
    out = out.replace("EEE", weekday);
    out = out.replace("MM", &format!("{:02}", date.month));
    out = out.replace("dd", &format!("{:02}", date.day));
    out = out.replace("HH", &format!("{:02}", time.hour));
    out = out.replace("mm", &format!("{:02}", time.minute));
    out = out.replace("ss", &format!("{:02}", time.second));
    if let Some((z, off)) = zone {
        out = out.replace("VV", &z.name);
        out = out.replace("XXX", &jet_time_offset_string(off));
    }
    out
}

// D-PARCAPTURE1=D: one bounded indexed engine for every explicit parallel
// collection adapter. Chunk boundaries are fixed so scheduling cannot affect
// result order or `para_fold`'s merge tree; the number of worker threads is
// bounded by the host's available parallelism.
const JET_PARA_CHUNK_ITEMS: usize = 64;

struct JetParaFailure {
    index: usize,
    payload: Box<dyn std::any::Any + Send + 'static>,
}

enum JetParaRuntimeFailure {
    Simple {
        file: String,
        line: u32,
        msg: String,
    },
    Rich {
        file: String,
        line: u32,
        fn_name: String,
        src_line: String,
        col: u32,
        caret_len: u32,
        msg: String,
        locals: String,
    },
    Diagnostic {
        rendered: String,
    },
    Contract {
        file: String,
        line: u32,
        clause_kw: String,
        msg: String,
    },
    SchedulerFatal {
        msg: String,
    },
}

impl JetParaRuntimeFailure {
    fn raise(self) -> ! {
        match self {
            Self::Simple { file, line, msg } => jet_panic(&file, line, &msg),
            Self::Rich {
                file,
                line,
                fn_name,
                src_line,
                col,
                caret_len,
                msg,
                locals,
            } => jet_panic_rich(
                &file, line, &fn_name, &src_line, col, caret_len, &msg, &locals,
            ),
            Self::Diagnostic { rendered } => jet_runtime_diagnostic(rendered),
            Self::Contract {
                file,
                line,
                clause_kw,
                msg,
            } => jet_contract_fail(&file, line, &clause_kw, &msg),
            // The scheduler prelude is emitted only when task support is used,
            // while the parallel carrier is part of the always-present core
            // prelude.  Reproduce the scheduler's ordinary fatal boundary here
            // without creating a generated-code dependency on an optional item.
            Self::SchedulerFatal { msg } => {
                eprintln!("panic: {}", msg);
                std::process::exit(70);
            }
        }
    }
}

thread_local! {
    static JET_PARA_DEFER_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn jet_para_call<R, F>(index: usize, f: F) -> Result<R, JetParaFailure>
where
    F: FnOnce() -> R,
{
    let result = JET_PARA_DEFER_FAILURE.with(|defer| {
        let previous = defer.replace(true);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        defer.set(previous);
        result
    });
    result.map_err(|payload| JetParaFailure { index, payload })
}

fn jet_para_raise_failure(failure: JetParaFailure) -> ! {
    match failure.payload.downcast::<JetParaRuntimeFailure>() {
        Ok(failure) => (*failure).raise(),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

fn jet_list_para_chunks<R, F>(len: usize, f: F) -> Vec<R>
where
    R: Send,
    F: Fn(std::ops::Range<usize>) -> Result<R, JetParaFailure> + Sync,
{
    let chunk_count = len.div_ceil(JET_PARA_CHUNK_ITEMS);
    if chunk_count == 0 {
        return Vec::new();
    }
    #[cfg(jet_para_test_workers)]
    let worker_count = 3.min(chunk_count);
    #[cfg(not(jet_para_test_workers))]
    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(chunk_count);
    let mut indexed = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(worker_count);
        let f = &f;
        for worker in 0..worker_count {
            handles.push(scope.spawn(move || {
                let mut out = Vec::new();
                for chunk in (worker..chunk_count).step_by(worker_count) {
                    let start = chunk * JET_PARA_CHUNK_ITEMS;
                    let end = (start + JET_PARA_CHUNK_ITEMS).min(len);
                    out.push((chunk, f(start..end)));
                }
                out
            }));
        }
        let mut indexed = Vec::with_capacity(chunk_count);
        for handle in handles.into_iter().rev() {
            match handle.join() {
                Ok(results) => indexed.extend(results),
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
        indexed
    });
    indexed.sort_unstable_by_key(|(chunk, _)| *chunk);
    let mut results = Vec::with_capacity(chunk_count);
    let mut first_failure: Option<JetParaFailure> = None;
    for (_, outcome) in indexed {
        match outcome {
            Ok(result) => results.push(result),
            Err(failure)
                if first_failure
                    .as_ref()
                    .is_none_or(|first| failure.index < first.index) =>
            {
                first_failure = Some(failure);
            }
            Err(_) => {}
        }
    }
    if let Some(failure) = first_failure {
        jet_para_raise_failure(failure);
    }
    results
}

fn jet_list_para_map<T, U, F>(xs: Vec<T>, f: F) -> Vec<U>
where
    T: Sync,
    U: Send,
    F: Fn(&T) -> U + Sync,
{
    jet_list_para_chunks(xs.len(), |range| {
        let mut out = Vec::with_capacity(range.len());
        for index in range {
            out.push(jet_para_call(index, || f(&xs[index]))?);
        }
        Ok(out)
    })
    .into_iter()
    .flatten()
    .collect()
}

fn jet_list_para_flags<T, F>(xs: &[T], f: F) -> Vec<bool>
where
    T: Sync,
    F: Fn(&T) -> bool + Sync,
{
    jet_list_para_chunks(xs.len(), |range| {
        let mut out = Vec::with_capacity(range.len());
        for index in range {
            out.push(jet_para_call(index, || f(&xs[index]))?);
        }
        Ok(out)
    })
    .into_iter()
    .flatten()
    .collect()
}

fn jet_list_para_filter<T, F>(xs: Vec<T>, f: F) -> Vec<T>
where
    T: Sync,
    F: Fn(&T) -> bool + Sync,
{
    let keep = jet_list_para_flags(&xs, f);
    xs.into_iter()
        .zip(keep)
        .filter_map(|(x, keep)| keep.then_some(x))
        .collect()
}

fn jet_list_para_partition<T, F, R, O>(xs: Vec<T>, f: F, out: O) -> R
where
    T: Sync,
    F: Fn(&T) -> bool + Sync,
    O: FnOnce(Vec<T>, Vec<T>) -> R,
{
    let matches = jet_list_para_flags(&xs, f);
    let mut false_items = Vec::new();
    let mut true_items = Vec::new();
    for (item, matched) in xs.into_iter().zip(matches) {
        if matched {
            true_items.push(item);
        } else {
            false_items.push(item);
        }
    }
    out(false_items, true_items)
}

fn jet_list_para_fold<T, U, S, F, M>(xs: Vec<T>, seed: S, step: F, merge: M) -> U
where
    T: Sync,
    U: Send,
    S: Fn() -> U + Sync,
    F: Fn(&U, &T) -> U + Sync,
    M: Fn(&U, &U) -> U + Sync,
{
    let mut partials = jet_list_para_chunks(xs.len(), |range| {
        let start = range.start;
        let mut acc = jet_para_call(start, &seed)?;
        for index in range {
            acc = jet_para_call(index, || step(&acc, &xs[index]))?;
        }
        Ok((start, acc))
    });
    if partials.is_empty() {
        return seed();
    }
    while partials.len() > 1 {
        let mut next = Vec::with_capacity(partials.len().div_ceil(2));
        let mut iter = partials.into_iter();
        while let Some((left_index, left)) = iter.next() {
            match iter.next() {
                Some((_, right)) => match jet_para_call(left_index, || merge(&left, &right)) {
                    Ok(merged) => next.push((left_index, merged)),
                    Err(failure) => jet_para_raise_failure(failure),
                },
                None => next.push((left_index, left)),
            }
        }
        partials = next;
    }
    partials.pop().expect("non-empty parallel fold lost its result").1
}

// D-FIDELITY-API1=A: runtime-global fidelity signal. App code decides policy.
const JET_PERF_DEFAULT_FIDELITY_BITS: u32 = 1065353216; // 1.0f32 bits
static JET_PERF_FIDELITY: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(JET_PERF_DEFAULT_FIDELITY_BITS);
fn jet_perf_fidelity() -> f64 {
    let bits = JET_PERF_FIDELITY.load(std::sync::atomic::Ordering::SeqCst);
    f32::from_bits(bits) as f64
}
fn jet_perf_default_fidelity() -> f64 {
    f32::from_bits(JET_PERF_DEFAULT_FIDELITY_BITS) as f64
}
fn jet_perf_store_fidelity(v: f64) {
    JET_PERF_FIDELITY.store((v as f32).to_bits(), std::sync::atomic::Ordering::SeqCst);
}
fn jet_perf_override_fidelity(v: f64) -> Result<(), String> {
    if !v.is_finite() || v < 0.0 || v > 1.0 {
        return Err(format!(
            "core.perf.Perf.override_fidelity needs 0.0 through 1.0, got {}",
            v
        ));
    }
    jet_perf_store_fidelity(v);
    Ok(())
}
fn jet_perf_reset_fidelity() {
    JET_PERF_FIDELITY.store(
        JET_PERF_DEFAULT_FIDELITY_BITS,
        std::sync::atomic::Ordering::SeqCst,
    );
}

// ── D-APPROX1=A: core.sketch — approximate data structures ────────────────────
// FNV-1a: deterministic, I6-safe, no external crates.
fn fnv1a(data: &[u8]) -> u64 {
    let mut hash: u64 = 14695981039346656037;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}
// Second independent hash (FNV with a different offset) for multi-hash sketches.
fn fnv1a_h2(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325u64.wrapping_add(0xdeadbeef);
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

// HyperLogLog — cardinality estimator (±2% error at 256 registers).
#[derive(Clone)]
struct JetHyperLogLog(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
impl JetHyperLogLog {
    fn new() -> Self {
        JetHyperLogLog(std::sync::Arc::new(std::sync::Mutex::new(vec![0u8; 256])))
    }
    fn add(&self, item: &str) {
        let h = fnv1a(item.as_bytes());
        let reg = (h & 0xFF) as usize; // bottom 8 bits → register index
        let rest = h >> 8; // remaining 56 bits
        let lz = if rest == 0 {
            57u8
        } else {
            rest.leading_zeros() as u8 + 1
        };
        let mut regs = self.0.lock().unwrap();
        if lz > regs[reg] {
            regs[reg] = lz;
        }
    }
    fn count(&self) -> i64 {
        let regs = self.0.lock().unwrap();
        let m = regs.len() as f64;
        // LinearCounting for small cardinalities.
        let zeros = regs.iter().filter(|&&v| v == 0).count();
        if zeros > 0 {
            let estimate = m * (m / zeros as f64).ln();
            return estimate.round() as i64;
        }
        // Normal HLL estimate with bias correction constant α_256.
        let sum: f64 = regs.iter().map(|&v| 2f64.powi(-(v as i32))).sum();
        let alpha = 0.7213 / (1.0 + 1.079 / m);
        (alpha * m * m / sum).round() as i64
    }
}
impl JetShow for JetHyperLogLog {
    fn jet_show(&self) -> String {
        format!("HyperLogLog(count={})", self.count())
    }
}

// TDigest — quantile estimator (~±0.5% error). Centroid merging sketch.
#[derive(Clone)]
struct JetTDigest(std::sync::Arc<std::sync::Mutex<Vec<(f64, f64)>>>); // (mean, weight)
impl JetTDigest {
    const DELTA: f64 = 100.0; // compression factor (higher = more accurate, more memory)
    fn new() -> Self {
        JetTDigest(std::sync::Arc::new(std::sync::Mutex::new(Vec::new())))
    }
    fn add(&self, v: f64) {
        let mut cs = self.0.lock().unwrap();
        // Insert as singleton then merge nearby centroids.
        let idx = cs.partition_point(|&(m, _)| m < v);
        cs.insert(idx, (v, 1.0));
        let total: f64 = cs.iter().map(|(_, w)| w).sum();
        let mut merged: Vec<(f64, f64)> = Vec::with_capacity(cs.len());
        let mut cum = 0.0f64;
        for &(mean, weight) in cs.iter() {
            if merged.is_empty() {
                merged.push((mean, weight));
                cum += weight;
                continue;
            }
            let last = merged.last_mut().unwrap();
            let q = cum / total;
            let limit = 4.0 * total * q * (1.0 - q) / Self::DELTA;
            if last.1 + weight <= limit.max(1.0) {
                let new_w = last.1 + weight;
                last.0 = (last.0 * last.1 + mean * weight) / new_w;
                last.1 = new_w;
            } else {
                merged.push((mean, weight));
                cum += weight;
            }
        }
        *cs = merged;
    }
    fn quantile(&self, q: f64) -> f64 {
        let cs = self.0.lock().unwrap();
        if cs.is_empty() {
            return 0.0;
        }
        let total: f64 = cs.iter().map(|(_, w)| w).sum();
        let target = q * total;
        let mut cum = 0.0f64;
        for &(mean, weight) in cs.iter() {
            cum += weight;
            if cum >= target {
                return mean;
            }
        }
        cs.last().unwrap().0
    }
}
impl JetShow for JetTDigest {
    fn jet_show(&self) -> String {
        "TDigest".to_string()
    }
}

// CountMinSketch — frequency estimator. 4 rows × 256 cols; FNV + offset.
const CMS_COLS: usize = 256;
#[derive(Clone)]
struct JetCountMinSketch(std::sync::Arc<std::sync::Mutex<[[u32; CMS_COLS]; 4]>>);
impl JetCountMinSketch {
    fn new() -> Self {
        JetCountMinSketch(std::sync::Arc::new(std::sync::Mutex::new(
            [[0u32; CMS_COLS]; 4],
        )))
    }
    fn add(&self, key: &str) {
        let bytes = key.as_bytes();
        let h1 = fnv1a(bytes);
        let h2 = fnv1a_h2(bytes);
        let mut tbl = self.0.lock().unwrap();
        for row in 0..4usize {
            let col = ((h1.wrapping_add(h2.wrapping_mul(row as u64 + 1))) & 0xFF) as usize;
            tbl[row][col] = tbl[row][col].saturating_add(1);
        }
    }
    fn count(&self, key: &str) -> i64 {
        let bytes = key.as_bytes();
        let h1 = fnv1a(bytes);
        let h2 = fnv1a_h2(bytes);
        let tbl = self.0.lock().unwrap();
        (0..4usize)
            .map(|row| {
                let col = ((h1.wrapping_add(h2.wrapping_mul(row as u64 + 1))) & 0xFF) as usize;
                tbl[row][col]
            })
            .min()
            .unwrap() as i64
    }
}
impl JetShow for JetCountMinSketch {
    fn jet_show(&self) -> String {
        "CountMinSketch".to_string()
    }
}

// ReservoirSampler — uniform random sample. Seeded xorshift64 PRNG (I6-safe).
#[derive(Clone)]
struct JetReservoirSampler(std::sync::Arc<std::sync::Mutex<JetReservoirInner>>);
struct JetReservoirInner {
    capacity: usize,
    reservoir: Vec<String>,
    count: u64,
    rng: u64,
}
impl Clone for JetReservoirInner {
    fn clone(&self) -> Self {
        JetReservoirInner {
            capacity: self.capacity,
            reservoir: self.reservoir.clone(),
            count: self.count,
            rng: self.rng,
        }
    }
}
impl JetReservoirSampler {
    fn new(capacity: i64) -> Self {
        let cap = (capacity.max(1)) as usize;
        JetReservoirSampler(std::sync::Arc::new(std::sync::Mutex::new(
            JetReservoirInner {
                capacity: cap,
                reservoir: Vec::with_capacity(cap),
                count: 0,
                rng: 0xdeadbeef_cafebabe,
            },
        )))
    }
    fn add(&self, item: String) {
        let mut inner = self.0.lock().unwrap();
        inner.count += 1;
        if inner.reservoir.len() < inner.capacity {
            inner.reservoir.push(item);
        } else {
            // xorshift64
            let mut x = inner.rng;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            inner.rng = x;
            let j = (x % inner.count) as usize;
            if j < inner.capacity {
                inner.reservoir[j] = item;
            }
        }
    }
    fn sample(&self) -> Vec<String> {
        self.0.lock().unwrap().reservoir.clone()
    }
}
impl JetShow for JetReservoirSampler {
    fn jet_show(&self) -> String {
        format!("ReservoirSampler(n={})", self.0.lock().unwrap().count)
    }
}

thread_local! {
    static JET_IN_SCHEDULER_TASK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static JET_INTERRUPT_HANDLER_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

pub fn jet_scheduler_task_panic_enter() {
    JET_IN_SCHEDULER_TASK.with(|c| c.set(true));
}

pub fn jet_scheduler_task_panic_leave() {
    JET_IN_SCHEDULER_TASK.with(|c| c.set(false));
}

fn jet_scheduler_in_task() -> bool {
    JET_IN_SCHEDULER_TASK.with(|c| c.get())
}

pub fn jet_interrupt_handler_panic_enter() {
    JET_INTERRUPT_HANDLER_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
}

pub fn jet_interrupt_handler_panic_leave() {
    JET_INTERRUPT_HANDLER_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
}

fn jet_runtime_should_unwind() -> bool {
    jet_scheduler_in_task() || jet_interrupt_handler_should_unwind()
}

fn jet_interrupt_handler_should_unwind() -> bool {
    JET_INTERRUPT_HANDLER_DEPTH.with(|depth| depth.get() != 0)
}

fn jet_scheduler_panic_should_unwind() -> bool {
    jet_runtime_should_unwind()
}

struct JetRuntimeExit;

fn jet_runtime_boundary<F, T>(run: F) -> T
where
    F: FnOnce() -> T,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(value) => value,
        Err(payload) if payload.is::<JetRuntimeExit>() => std::process::exit(70),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

fn jet_runtime_exit() -> ! {
    std::panic::resume_unwind(Box::new(JetRuntimeExit))
}

fn jet_panic(file: &str, line: u32, msg: &str) -> ! {
    if JET_PARA_DEFER_FAILURE.with(|defer| defer.get()) {
        std::panic::resume_unwind(Box::new(JetParaRuntimeFailure::Simple {
            file: file.to_string(),
            line,
            msg: msg.to_string(),
        }));
    }
    jet_proof_record(2, 1, "panic", msg, file, line);
    if jet_runtime_should_unwind() {
        panic!("{} (at {}:{})", msg, file, line);
    }
    eprintln!("panic: {}", msg);
    eprintln!("  --> {}:{}", file, line);
    jet_runtime_exit();
}

fn jet_runtime_diagnostic(rendered: String) -> ! {
    if JET_PARA_DEFER_FAILURE.with(|defer| defer.get()) {
        std::panic::resume_unwind(Box::new(JetParaRuntimeFailure::Diagnostic { rendered }));
    }
    if jet_interrupt_handler_should_unwind() {
        panic!("{}", rendered);
    }
    eprintln!("{}", rendered);
    jet_runtime_exit();
}
/// E3005 (D-PREPOST1): a `@Pre`/`@Post` contract clause failed at runtime.
/// `clause_kw` is `"Pre"`/`"Post"`; `msg` is the clause's own message text
/// (the second argument to `@Pre(cond, "msg")`/`@Post(cond, "msg")`).
#[allow(dead_code)] // only called from generated code that has a @Pre/@Post
fn jet_contract_fail(file: &str, line: u32, clause_kw: &str, msg: &str) -> ! {
    if JET_PARA_DEFER_FAILURE.with(|defer| defer.get()) {
        std::panic::resume_unwind(Box::new(JetParaRuntimeFailure::Contract {
            file: file.to_string(),
            line,
            clause_kw: clause_kw.to_string(),
            msg: msg.to_string(),
        }));
    }
    if jet_runtime_should_unwind() {
        panic!(
            "@{} contract failed: {} (at {}:{})",
            clause_kw, msg, file, line
        );
    }
    eprintln!("@{} contract failed: {}", clause_kw, msg);
    eprintln!("  --> {}:{}", file, line);
    jet_runtime_exit();
}

/// Private structured producer channel used only when `jet prove` launches a
/// test harness. Length framing keeps user strings opaque; terminal text is
/// never parsed as evidence.
fn jet_proof_record(kind: u8, state: u8, name: &str, message: &str, file: &str, line: u32) {
    let Ok(path) = std::env::var("JET_TEST_PROOF_REPORT") else { return };
    let Ok(mut report) = std::fs::OpenOptions::new().create(true).append(true).open(path) else { return };
    use std::io::Write as _;
    if report.metadata().map(|m| m.len() == 0).unwrap_or(false) {
        let _ = report.write_all(b"JETTEST2");
    }
    let _ = report.write_all(&[kind, state]);
    let _ = report.write_all(&(line as u64).to_be_bytes());
    for bytes in [name.as_bytes(), message.as_bytes(), file.as_bytes()] {
        let _ = report.write_all(&(bytes.len() as u64).to_be_bytes());
        let _ = report.write_all(bytes);
    }
    let _ = report.flush();
}
// D-NUMOPS1: plain integer arithmetic traps on overflow (safe by default) — a
// silent corruption becomes a caught bug. Each `+`/`-`/`*`/`/` on a fixed-width
// integer lowers to one of these, which panic with the source location instead
// of wrapping. `wrapping(…)`/`saturating(…)`/`checked(…)` opt out at the use
// site. Floats and `@Numeric` distinct types keep the plain Rust operators.
trait JetArith: Copy {
    fn jet_add(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_sub(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_mul(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_div(self, rhs: Self, file: &str, line: u32) -> Self;
    // D-NUMOPS1: a shift by a bit-count `>=` the value's width is undefined in C
    // and a panic in Rust — Jet traps it cleanly instead. The count comes in as
    // an `i128` so any integer width (signed or unsigned) reaches here losslessly.
    fn jet_shl(self, bits: i128, file: &str, line: u32) -> Self;
    fn jet_shr(self, bits: i128, file: &str, line: u32) -> Self;
}
macro_rules! jet_arith_impl {
    ($($t:ty),*) => { $(
        impl JetArith for $t {
            fn jet_add(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_add(rhs).unwrap_or_else(|| jet_panic(file, line,
                    &format!("this addition overflows the value's type (the result is outside its range)")))
            }
            fn jet_sub(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_sub(rhs).unwrap_or_else(|| jet_panic(file, line,
                    &format!("this subtraction overflows the value's type (the result is outside its range)")))
            }
            fn jet_mul(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_mul(rhs).unwrap_or_else(|| jet_panic(file, line,
                    &format!("this multiplication overflows the value's type (the result is outside its range)")))
            }
            fn jet_div(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_div(rhs).unwrap_or_else(|| jet_panic(file, line,
                    &format!("this division can't be done (dividing by zero, or overflow)")))
            }
            fn jet_shl(self, bits: i128, file: &str, line: u32) -> Self {
                let w = (Self::BITS) as i128;
                if bits < 0 || bits >= w {
                    jet_panic(file, line, &format!(
                        "shifting left by {} bits is out of range (this type is {} bits wide)", bits, w));
                }
                self << (bits as u32)
            }
            fn jet_shr(self, bits: i128, file: &str, line: u32) -> Self {
                let w = (Self::BITS) as i128;
                if bits < 0 || bits >= w {
                    jet_panic(file, line, &format!(
                        "shifting right by {} bits is out of range (this type is {} bits wide)", bits, w));
                }
                self >> (bits as u32)
            }
        }
    )* };
}
jet_arith_impl!(i8, i16, i32, i64, u8, u16, u32, u64);
/// E3001 (E2-M12, D-OBS1/D-OBS2): rich panic report — includes the function name,
/// a source-line context box, and (in debug builds only) safe local variable values.
/// `col` is 1-based; `caret_len` covers the highlighted span in the source line.
/// `locals` is an empty string in release builds; "x = 1, y = false" in debug builds.
fn jet_panic_rich(
    file: &str,
    line: u32,
    fn_name: &str,
    src_line: &str,
    col: u32,
    caret_len: u32,
    msg: &str,
    locals: &str,
) -> ! {
    if JET_PARA_DEFER_FAILURE.with(|defer| defer.get()) {
        std::panic::resume_unwind(Box::new(JetParaRuntimeFailure::Rich {
            file: file.to_string(),
            line,
            fn_name: fn_name.to_string(),
            src_line: src_line.to_string(),
            col,
            caret_len,
            msg: msg.to_string(),
            locals: locals.to_string(),
        }));
    }
    jet_proof_record(2, 1, "panic", msg, file, line);
    let line_s = line.to_string();
    let margin = line_s.len();
    let pad = " ".repeat(margin);
    eprintln!("panic: {}", msg);
    eprintln!("  --> {}:{} in {}", file, line, fn_name);
    eprintln!("   {}|", pad);
    eprintln!("{} | {}", line_s, src_line);
    let col_offset = col.saturating_sub(1) as usize;
    let caret = "^".repeat(caret_len.max(1) as usize);
    eprintln!("   {}| {}{}", pad, " ".repeat(col_offset), caret);
    if !locals.is_empty() {
        eprintln!("locals: {}", locals);
    }
    if jet_runtime_should_unwind() {
        panic!("{} (at {}:{})", msg, file, line);
    }
    jet_runtime_exit();
}
/// E3002 (E2-M12, D-OBS1): error-return trace frame. In debug builds, when a `?`
/// actually propagates an `Err`, print one Zig-style frame to stderr, then hand
/// the `Result` back unchanged so the caller's `?` proceeds (incl. any
/// `From`/`to_error` conversion). In release builds this is a no-op.
fn jet_trace_err<T, E>(r: Result<T, E>, file: &str, line: u32, fn_name: &str) -> Result<T, E> {
    if cfg!(debug_assertions) && r.is_err() {
        eprintln!(
            "error propagated from: {} ({}:{}) via ?",
            fn_name, file, line
        );
    }
    r
}
// D-ERRCTX1=D: `.context(msg)` — a lazily-evaluated human boundary message
// prepended to the error chain (errors are plain `String`s in Jet, so the
// chain is just accumulated text: origin, then each `.context()` crossed on
// the way out). `msg` runs only on the `Err` branch.
fn jet_context<T, F: FnOnce() -> String>(r: Result<T, String>, msg: F) -> Result<T, String> {
    match r {
        Ok(v) => Ok(v),
        Err(e) => Err(format!("{}: {}", msg(), e)),
    }
}
// D-FIXARR1: index/unpack/slice helpers accept `&[T]` so that both growable
// `Vec<T>` and fixed-size `[T; N]` stack arrays coerce in without `.to_vec()`.
fn jet_index_vec<T: Clone>(xs: &[T], i: i64, file: &str, line: u32) -> T {
    let len = xs.len() as i64;
    if i < 0 || i >= len {
        jet_panic(
            file,
            line,
            &format!(
                "the list has {} items, so position {} doesn't exist",
                len, i
            ),
        );
    }
    xs[i as usize].clone()
}
fn jet_unpack_vec<T: Clone>(xs: &[T], want: usize, i: usize, file: &str, line: u32) -> T {
    if xs.len() != want {
        jet_panic(
            file,
            line,
            &format!(
                "this pattern needs exactly {} item{}, but the list has {}",
                want,
                if want == 1 { "" } else { "s" },
                xs.len()
            ),
        );
    }
    xs[i].clone()
}
fn jet_slice_vec<T: Clone>(xs: &[T], a: i64, b: i64, file: &str, line: u32) -> Vec<T> {
    let len = xs.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(
            file,
            line,
            &format!("can't slice {} items from {} to {} (inclusive)", len, a, b),
        );
    }
    xs[a as usize..=b as usize].to_vec()
}
// D-DYNARRAY1: `View<T>` — `list.view(a..b)` is the zero-copy sibling of
// `list[a..b]` (`jet_slice_vec` above): same inclusive bounds, same panic
// wording, but a borrowed Rust slice instead of a fresh `Vec` — no element
// data is copied. The returned `&[T]`'s lifetime is elided from `xs`'s; sema
// (E2305) proves the window never outlives the list before this ever runs.
fn jet_view_new<'a, T>(xs: &'a [T], a: i64, b: i64, file: &str, line: u32) -> &'a [T] {
    let len = xs.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(
            file,
            line,
            &format!("can't view {} items from {} to {} (inclusive)", len, a, b),
        );
    }
    &xs[a as usize..=b as usize]
}

fn jet_view_mut_new<'a, T>(
    xs: &'a mut [T],
    a: i64,
    b: i64,
    file: &str,
    line: u32,
) -> &'a mut [T] {
    let len = xs.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(
            file,
            line,
            &format!("can't view {} items from {} to {} (inclusive)", len, a, b),
        );
    }
    &mut xs[a as usize..=b as usize]
}

fn jet_check_view_bounds(len: i64, a: i64, b: i64, file: &str, line: u32) {
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(
            file,
            line,
            &format!("can't view {} items from {} to {} (inclusive)", len, a, b),
        );
    }
}
// D-DYNARRAY1: View<T> read-only closure surface. `xs` is already a borrow
// (never `.clone()`d to an owned `Vec` first, unlike the `jet_list_*` family
// above) — folding/mapping a view touches no allocation beyond the result.
fn jet_view_fold<T, U, F>(xs: &[T], init: U, mut f: F) -> U
where
    F: FnMut(&U, &T) -> U,
{
    let mut acc = init;
    for x in xs {
        acc = f(&acc, x);
    }
    acc
}
fn jet_view_map<T, U, F>(xs: &[T], f: F) -> Vec<U>
where
    F: FnMut(&T) -> U,
{
    xs.iter().map(f).collect()
}
fn jet_index_map<K: Ord + Clone, V: Clone>(
    m: &std::collections::BTreeMap<K, V>,
    k: &K,
    file: &str,
    line: u32,
) -> V {
    match m.get(k) {
        Some(v) => v.clone(),
        None => jet_panic(file, line, &format!("the map has no entry for this key")),
    }
}
fn jet_map_insert<K: Ord, V>(m: &mut std::collections::BTreeMap<K, V>, k: K, v: V) {
    m.insert(k, v);
}
fn jet_list_remove<T: Clone>(xs: &mut Vec<T>, i: i64, file: &str, line: u32) -> T {
    let len = xs.len() as i64;
    if i < 0 || i >= len {
        jet_panic(
            file,
            line,
            &format!(
                "the list has {} items, so position {} doesn't exist",
                len, i
            ),
        );
    }
    xs.remove(i as usize)
}
fn jet_char_len(s: &String) -> i64 {
    s.chars().count() as i64
}
fn jet_string_split(s: &String, sep: &str) -> Vec<String> {
    s.split(sep).map(|x| x.to_string()).collect()
}
// D-STR-AFTER1: first-occurrence substring split. `sep` absent -> the whole
// original string (both sides agree, mirroring `.replace`'s no-match-is-identity
// convention — no `Option`/empty-string special case to unwrap).
fn jet_string_after(s: &String, sep: &str) -> String {
    match s.find(sep) {
        Some(i) => s[i + sep.len()..].to_string(),
        None => s.clone(),
    }
}
fn jet_string_before(s: &String, sep: &str) -> String {
    match s.find(sep) {
        Some(i) => s[..i].to_string(),
        None => s.clone(),
    }
}
// D-MEM1 stage S5 (2026-07-04): zero-copy siblings of `jet_string_after`/
// `_before`/(inline `.trim()`) — a genuine borrow into `s`'s own buffer, no
// allocation, instead of a fresh owned `String`. Used ONLY when sema proves
// (E2307, `Binding::string_view`) the resulting binding can't outlive `s`'s
// scope — the same D-DYNARRAY1 soundness proof `View<T>`/`jet_view_new`
// already uses, applied to strings. `s: &str` (not `&String`) so a call
// chain of these composes without a materialize step in between.
fn jet_string_after_view<'a>(s: &'a str, sep: &str) -> &'a str {
    match s.find(sep) {
        Some(i) => &s[i + sep.len()..],
        None => s,
    }
}
fn jet_string_before_view<'a>(s: &'a str, sep: &str) -> &'a str {
    match s.find(sep) {
        Some(i) => &s[..i],
        None => s,
    }
}
fn jet_string_trim_view(s: &str) -> &str {
    s.trim()
}
// D-TYPEDTEXT1=D: escape a hole's text before it joins an `Html` template —
// the audited insertion point for every non-`.raw()` interpolation.
fn jet_html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}
fn jet_string_lines(s: &String) -> Vec<String> {
    s.lines().map(|x| x.to_string()).collect()
}
fn jet_string_slice(s: &String, a: i64, b: i64, file: &str, line: u32) -> String {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(
            file,
            line,
            &format!(
                "can't slice {} characters from {} to {} (inclusive)",
                len, a, b
            ),
        );
    }
    chars[a as usize..=b as usize].iter().collect()
}
fn jet_list_map<T, U, F>(xs: Vec<T>, f: F) -> Vec<U>
where
    F: Fn(&T) -> U,
{
    xs.iter().map(f).collect()
}
fn jet_list_map_mut<T, U, F>(xs: Vec<T>, mut f: F) -> Vec<U>
where
    F: FnMut(&T) -> U,
{
    xs.iter().map(|x| f(x)).collect()
}
fn jet_list_filter<T, F>(xs: Vec<T>, mut f: F) -> Vec<T>
where
    F: FnMut(&T) -> bool,
{
    xs.into_iter().filter(|x| f(x)).collect()
}
fn jet_list_each<T, F>(xs: Vec<T>, f: F)
where
    F: Fn(&T),
{
    for x in &xs {
        f(x);
    }
}
fn jet_list_each_ref<T, F>(xs: &Vec<T>, mut f: F)
where
    F: FnMut(&T),
{
    for x in xs.iter() {
        f(x);
    }
}
fn jet_list_each_mut<T, F>(xs: Vec<T>, mut f: F)
where
    F: FnMut(&T),
{
    for x in &xs {
        f(x);
    }
}
fn jet_list_find<T, F>(xs: Vec<T>, mut f: F) -> Option<T>
where
    F: FnMut(&T) -> bool,
{
    xs.into_iter().find(|x| f(x))
}
fn jet_list_any<T, F>(xs: Vec<T>, f: F) -> bool
where
    F: FnMut(&T) -> bool,
{
    xs.iter().any(f)
}
fn jet_list_all<T, F>(xs: Vec<T>, f: F) -> bool
where
    F: FnMut(&T) -> bool,
{
    xs.iter().all(f)
}
fn jet_list_sort_by<T, K: Ord, F>(xs: &mut Vec<T>, f: F)
where
    F: FnMut(&T) -> K,
{
    xs.sort_by_key(f);
}
fn jet_list_reduce<T, U, F>(xs: Vec<T>, init: U, mut f: F) -> U
where
    F: FnMut(&U, &T) -> U,
{
    xs.iter().fold(init, |acc, x| f(&acc, x))
}
fn jet_map_each<K: Ord, V, F>(m: std::collections::BTreeMap<K, V>, mut f: F)
where
    F: FnMut(&K, &V),
{
    for (k, v) in &m {
        f(k, v);
    }
}
// ── D-ITERTOOLS1=A: expanded collection/runtime handles ─────────────────────
#[derive(Clone)]
struct JetLru<K, V> {
    cap: usize,
    entries: Vec<(K, V)>,
}

impl<K: Eq + Clone, V: Clone> JetLru<K, V> {
    fn new(capacity: i64) -> Self {
        Self {
            cap: capacity.max(0) as usize,
            entries: Vec::new(),
        }
    }
    fn put(&mut self, key: K, value: V) -> Option<V> {
        if self.cap == 0 {
            return None;
        }
        let displaced = self.entries.iter().position(|(k, _)| *k == key)
            .map(|i| self.entries.remove(i).1);
        self.entries.insert(0, (key, value));
        if self.entries.len() > self.cap {
            self.entries.pop();
        }
        displaced
    }
    fn add_new(&mut self, key: K, value: V) -> bool {
        if self.cap == 0 || self.entries.iter().any(|(k, _)| *k == key) {
            return false;
        }
        self.entries.insert(0, (key, value));
        if self.entries.len() > self.cap {
            self.entries.pop();
        }
        true
    }
    fn get(&mut self, key: &K) -> Option<V> {
        let i = self.entries.iter().position(|(k, _)| k == key)?;
        let (k, v) = self.entries.remove(i);
        let out = v.clone();
        self.entries.insert(0, (k, v));
        Some(out)
    }
    fn remove(&mut self, key: &K) -> Option<V> {
        let i = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(i).1)
    }
    fn contains_key(&self, key: &K) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }
    fn keys(&self) -> Vec<K> {
        self.entries.iter().map(|(k, _)| k.clone()).collect()
    }
    fn len(&self) -> usize {
        self.entries.len()
    }
    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    fn capacity(&self) -> i64 {
        self.cap as i64
    }
    fn clear(&mut self) {
        self.entries.clear();
    }
}

impl<K: JetShow, V: JetShow> JetShow for JetLru<K, V> {
    fn jet_show(&self) -> String {
        let parts: Vec<String> = self
            .entries
            .iter()
            .map(|(k, v)| format!("{}: {}", k.jet_show(), v.jet_show()))
            .collect();
        format!("[:{}]", parts.join(", "))
    }
}
impl<K: JetDisplay, V: JetDisplay> JetDisplay for JetLru<K, V> {
    fn jet_display(&self) -> String {
        let parts: Vec<String> = self
            .entries
            .iter()
            .map(|(k, v)| format!("{}: {}", k.jet_display(), v.jet_display()))
            .collect();
        format!("[:{}]", parts.join(", "))
    }
}
impl<K: JetDebug, V: JetDebug> JetDebug for JetLru<K, V> {
    fn jet_debug(&self) -> String {
        let parts: Vec<String> = self
            .entries
            .iter()
            .map(|(k, v)| format!("{}: {}", k.jet_debug(), v.jet_debug()))
            .collect();
        format!("[:{}]", parts.join(", "))
    }
}

#[derive(Clone)]
struct JetBitSet {
    bits: std::collections::BTreeSet<i64>,
}
impl JetBitSet {
    fn new() -> Self {
        Self {
            bits: std::collections::BTreeSet::new(),
        }
    }
    fn add(&mut self, bit: i64) -> bool {
        if bit >= 0 {
            self.bits.insert(bit)
        } else {
            false
        }
    }
    fn remove(&mut self, bit: &i64) {
        self.bits.remove(bit);
    }
    fn contains(&self, bit: &i64) -> bool {
        self.bits.contains(bit)
    }
    fn count(&self) -> i64 {
        self.bits.len() as i64
    }
    fn len(&self) -> i64 {
        self.bits.iter().next_back().map(|v| v + 1).unwrap_or(0)
    }
    fn is_empty(&self) -> bool {
        self.bits.is_empty()
    }
    fn clear(&mut self) {
        self.bits.clear();
    }
    fn to_list(&self) -> Vec<i64> {
        self.bits.iter().copied().collect()
    }
}
impl JetShow for JetBitSet {
    fn jet_show(&self) -> String {
        self.to_list().jet_show()
    }
}
impl JetDisplay for JetBitSet {
    fn jet_display(&self) -> String {
        self.to_list().jet_display()
    }
}
impl JetDebug for JetBitSet {
    fn jet_debug(&self) -> String {
        self.to_list().jet_debug()
    }
}

#[derive(Clone)]
struct JetByteBuffer {
    bytes: Vec<u8>,
}
impl JetByteBuffer {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }
    fn from(bytes: &Vec<u8>) -> Self {
        Self {
            bytes: bytes.clone(),
        }
    }
    fn write_u8(&mut self, v: u8) {
        self.bytes.push(v);
    }
    fn write_u16_le(&mut self, v: u16) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    fn write_u16_be(&mut self, v: u16) {
        self.bytes.extend_from_slice(&v.to_be_bytes());
    }
    fn write_u32_le(&mut self, v: u32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    fn write_u32_be(&mut self, v: u32) {
        self.bytes.extend_from_slice(&v.to_be_bytes());
    }
    fn write_u64_le(&mut self, v: u64) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    fn write_u64_be(&mut self, v: u64) {
        self.bytes.extend_from_slice(&v.to_be_bytes());
    }
    fn write_bytes(&mut self, bytes: &Vec<u8>) {
        self.bytes.extend_from_slice(bytes);
    }
    fn to_bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }
    fn len(&self) -> i64 {
        self.bytes.len() as i64
    }
    fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
    fn clear(&mut self) {
        self.bytes.clear();
    }
}
impl JetShow for JetByteBuffer {
    fn jet_show(&self) -> String {
        self.bytes.jet_show()
    }
}
impl JetDisplay for JetByteBuffer {
    fn jet_display(&self) -> String {
        self.bytes.jet_display()
    }
}
impl JetDebug for JetByteBuffer {
    fn jet_debug(&self) -> String {
        self.bytes.jet_debug()
    }
}

fn jet_list_sum<T>(xs: Vec<T>) -> T
where
    T: std::iter::Sum<T>,
{
    xs.into_iter().sum()
}
fn jet_list_product<T>(xs: Vec<T>) -> T
where
    T: std::iter::Product<T>,
{
    xs.into_iter().product()
}
fn jet_list_flatten<T>(xs: Vec<Vec<T>>) -> Vec<T> {
    xs.into_iter().flatten().collect()
}
fn jet_list_intersperse<T: Clone>(xs: Vec<T>, sep: T) -> Vec<T> {
    let mut out = Vec::new();
    for (i, x) in xs.into_iter().enumerate() {
        if i > 0 {
            out.push(sep.clone());
        }
        out.push(x);
    }
    out
}
fn jet_list_count_by<T, K: Ord, F>(
    xs: Vec<T>,
    mut f: F,
) -> std::collections::BTreeMap<K, i64>
where
    F: FnMut(&T) -> K,
{
    let mut m: std::collections::BTreeMap<K, i64> = std::collections::BTreeMap::new();
    for x in xs {
        let k = f(&x);
        *m.entry(k).or_insert(0) += 1;
    }
    m
}
// ── binary.Reader / text.Cursor (D-SHIFT1, c7shift) ──────────────────────────
// The "shift" kernel from linear stream parsing (Jai's `shift` primitive),
// without a dedicated operator (D-SHIFT1=A rejected that). `Reader` owns a
// copy of its byte buffer plus a read position; every read is fallible —
// a bounds miss is an ordinary `Err` string, never a panic or silent
// truncation. Lives HERE (not CoreLib.rs): every emit path pushes Core.rs's
// PRELUDE, while CORELIB_PRELUDE is additionally appended when core modules
// are used — a copy in both files is an E0428 duplicate definition.
struct JetReader {
    buf: Vec<u8>,
    pos: usize,
}
struct JetCursor {
    buf: String,
    pos: usize,
}

fn jet_reader_over(bytes: &Vec<u8>) -> JetReader {
    JetReader {
        buf: bytes.clone(),
        pos: 0,
    }
}

fn jet_reader_bounds_error(method: &str, need: usize, r: &JetReader) -> String {
    format!(
        "Reader.{}: needed {} byte{} at position {}, only {} remain",
        method,
        need,
        if need == 1 { "" } else { "s" },
        r.pos,
        r.buf.len().saturating_sub(r.pos),
    )
}

fn jet_reader_take_fixed(r: &mut JetReader, n: usize, method: &str) -> Result<Vec<u8>, String> {
    if r.pos + n > r.buf.len() {
        return Err(jet_reader_bounds_error(method, n, r));
    }
    let out = r.buf[r.pos..r.pos + n].to_vec();
    r.pos += n;
    Ok(out)
}

fn jet_reader_read_u8(r: &mut JetReader) -> Result<u8, String> {
    jet_reader_take_fixed(r, 1, "read_u8").map(|b| b[0])
}
fn jet_reader_read_u16_le(r: &mut JetReader) -> Result<u16, String> {
    jet_reader_take_fixed(r, 2, "read_u16_le").map(|b| u16::from_le_bytes([b[0], b[1]]))
}
fn jet_reader_read_u16_be(r: &mut JetReader) -> Result<u16, String> {
    jet_reader_take_fixed(r, 2, "read_u16_be").map(|b| u16::from_be_bytes([b[0], b[1]]))
}
fn jet_reader_read_u32_le(r: &mut JetReader) -> Result<u32, String> {
    jet_reader_take_fixed(r, 4, "read_u32_le").map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn jet_reader_read_u32_be(r: &mut JetReader) -> Result<u32, String> {
    jet_reader_take_fixed(r, 4, "read_u32_be").map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}
fn jet_reader_read_u64_le(r: &mut JetReader) -> Result<u64, String> {
    jet_reader_take_fixed(r, 8, "read_u64_le")
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}
fn jet_reader_read_u64_be(r: &mut JetReader) -> Result<u64, String> {
    jet_reader_take_fixed(r, 8, "read_u64_be")
        .map(|b| u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

fn jet_reader_take(r: &mut JetReader, n: i64) -> Result<Vec<u8>, String> {
    if n < 0 {
        return Err(format!(
            "Reader.take: length must not be negative, got {}",
            n
        ));
    }
    jet_reader_take_fixed(r, n as usize, "take")
}

fn jet_reader_remaining(r: &JetReader) -> i64 {
    (r.buf.len() - r.pos) as i64
}

fn jet_reader_at_end(r: &JetReader) -> bool {
    r.pos >= r.buf.len()
}

fn jet_cursor_over(s: &String) -> JetCursor {
    JetCursor {
        buf: s.clone(),
        pos: 0,
    }
}

fn jet_cursor_take_until(c: &mut JetCursor, delim: &String) -> Result<String, String> {
    let tail = &c.buf[c.pos..];
    match tail.find(delim.as_str()) {
        Some(i) => {
            let out = tail[..i].to_string();
            c.pos += i;
            Ok(out)
        }
        None => Err(format!(
            "Cursor.take_until: {:?} not found in the remaining text",
            delim
        )),
    }
}

fn jet_cursor_skip_ws(c: &mut JetCursor) {
    let tail = &c.buf[c.pos..];
    let skipped = tail.len() - tail.trim_start().len();
    c.pos += skipped;
}
// ── D-ITER1: lazy iterator adapter set ───────────────────────────────────────
// All adapters are allocation-free until terminal — they materialise to Vec<T>
// only when a terminal method (collect) is needed. For the Jet surface these
// are the terminal forms (the language is not lazy at the surface); the Rust
// functions are still lazy internally where the size is bounded.
fn jet_list_take<T: Clone>(xs: Vec<T>, n: i64) -> Vec<T> {
    xs.into_iter().take(n.max(0) as usize).collect()
}
fn jet_list_skip<T: Clone>(xs: Vec<T>, n: i64) -> Vec<T> {
    xs.into_iter().skip(n.max(0) as usize).collect()
}
fn jet_list_step_by<T: Clone>(xs: Vec<T>, n: i64) -> Vec<T> {
    if n <= 0 {
        return Vec::new();
    }
    xs.into_iter().step_by(n as usize).collect()
}
fn jet_list_dedup<T: Clone + PartialEq>(xs: Vec<T>) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();
    for x in xs {
        if out.last().map(|last| last != &x).unwrap_or(true) {
            out.push(x);
        }
    }
    out
}
fn jet_list_chunks<T: Clone>(xs: Vec<T>, n: i64) -> Vec<Vec<T>> {
    let n = n.max(1) as usize;
    xs.chunks(n).map(|c| c.to_vec()).collect()
}
fn jet_list_windows<T: Clone>(xs: Vec<T>, n: i64) -> Vec<Vec<T>> {
    let n = n.max(1) as usize;
    if n > xs.len() {
        return Vec::new();
    }
    xs.windows(n).map(|w| w.to_vec()).collect()
}
fn jet_list_take_while<T, F>(xs: Vec<T>, mut f: F) -> Vec<T>
where
    F: FnMut(&T) -> bool,
{
    xs.into_iter().take_while(|x| f(x)).collect()
}
fn jet_list_skip_while<T, F>(xs: Vec<T>, mut f: F) -> Vec<T>
where
    F: FnMut(&T) -> bool,
{
    xs.into_iter().skip_while(|x| f(x)).collect()
}
fn jet_list_flat_map<T, U, F>(xs: Vec<T>, f: F) -> Vec<U>
where
    F: FnMut(&T) -> Vec<U>,
{
    xs.iter().flat_map(f).collect()
}
fn jet_list_filter_map<T, U, E, F>(xs: Vec<T>, mut f: F) -> Vec<U>
where
    F: FnMut(&T) -> Result<U, E>,
{
    xs.iter().filter_map(|x| f(x).ok()).collect()
}
fn jet_list_try_collect<T: Clone, E: Clone>(xs: Vec<Result<T, E>>) -> Result<Vec<T>, E> {
    xs.into_iter().collect()
}
fn jet_list_scan<T, U: Clone, F>(xs: Vec<T>, init: U, mut f: F) -> Vec<U>
where
    F: FnMut(&U, &T) -> U,
{
    let mut acc = init;
    let mut out = Vec::new();
    for x in &xs {
        acc = f(&acc, x);
        out.push(acc.clone());
    }
    out
}
fn jet_list_fold<T, U, F>(xs: Vec<T>, init: U, mut f: F) -> U
where
    F: FnMut(&U, &T) -> U,
{
    xs.iter().fold(init, |acc, x| f(&acc, x))
}
fn jet_list_position<T, F>(xs: Vec<T>, f: F) -> Option<i64>
where
    F: FnMut(&T) -> bool,
{
    xs.iter().position(f).map(|i| i as i64)
}
fn jet_list_min_by<T, K: Ord, F>(xs: Vec<T>, f: F) -> Option<T>
where
    F: FnMut(&T) -> K,
{
    xs.into_iter().min_by_key(f)
}
fn jet_list_max_by<T, K: Ord, F>(xs: Vec<T>, f: F) -> Option<T>
where
    F: FnMut(&T) -> K,
{
    xs.into_iter().max_by_key(f)
}
fn jet_list_group_by<T, K: Ord, F>(
    xs: Vec<T>,
    mut f: F,
) -> std::collections::BTreeMap<K, Vec<T>>
where
    F: FnMut(&T) -> K,
{
    let mut m: std::collections::BTreeMap<K, Vec<T>> = std::collections::BTreeMap::new();
    for x in xs {
        let k = f(&x);
        m.entry(k).or_default().push(x);
    }
    m
}
/// `partition(f)` — splits into (true-list, false-list) as a named-tuple struct.
/// `build` receives `(true_vec, false_vec)` and wraps them into the JetTup struct.
fn jet_list_partition<T, F, S, B>(xs: Vec<T>, mut f: F, build: B) -> S
where
    F: FnMut(&T) -> bool,
    B: FnOnce(Vec<T>, Vec<T>) -> S,
{
    let mut yes: Vec<T> = Vec::new();
    let mut no: Vec<T> = Vec::new();
    for x in xs {
        if f(&x) {
            yes.push(x);
        } else {
            no.push(x);
        }
    }
    build(yes, no)
}
