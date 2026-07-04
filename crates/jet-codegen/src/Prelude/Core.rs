trait JetShow { fn jet_show(&self) -> String; }
/// D-DISPLAYDBG1: user-facing interpolation (`{value}`).
trait JetDisplay { fn jet_display(&self) -> String; }
/// D-DISPLAYDBG1: developer interpolation (`{value@Debug}`).
trait JetDebug { fn jet_debug(&self) -> String; }

macro_rules! jet_scalar_show {
    ($($t:ty),+ $(,)?) => {$(
        impl JetShow for $t { fn jet_show(&self) -> String { self.to_string() } }
        impl JetDisplay for $t { fn jet_display(&self) -> String { self.to_string() } }
        impl JetDebug for $t { fn jet_debug(&self) -> String { self.to_string() } }
    )+};
}
jet_scalar_show!(i64, i8, i16, i32, u8, u16, u32, u64, bool, char);
impl JetShow for f32 { fn jet_show(&self) -> String { format!("{:?}", self) } }
impl JetDisplay for f32 { fn jet_display(&self) -> String { format!("{:?}", self) } }
impl JetDebug for f32 { fn jet_debug(&self) -> String { format!("{:?}", self) } }
impl JetShow for f64 { fn jet_show(&self) -> String { format!("{:?}", self) } }
impl JetDisplay for f64 { fn jet_display(&self) -> String { format!("{:?}", self) } }
impl JetDebug for f64 { fn jet_debug(&self) -> String { format!("{:?}", self) } }
impl JetShow for String { fn jet_show(&self) -> String { self.clone() } }
impl JetDisplay for String { fn jet_display(&self) -> String { self.clone() } }
impl JetDebug for String { fn jet_debug(&self) -> String { format!("{self:?}") } }
// D-MEM1 stage S5: a string view (`s.trim()`/`.after()`/`.before()` bound to a
// local, see `jet_string_*_view` below) is a genuine `&str` in generated Rust —
// `String` stays the one Jet-level type, so anything that already works on a
// `String` (print/interpolate/debug) must also work on the view directly.
impl JetShow for str { fn jet_show(&self) -> String { self.to_string() } }
impl JetDisplay for str { fn jet_display(&self) -> String { self.to_string() } }
impl JetDebug for str { fn jet_debug(&self) -> String { format!("{self:?}") } }
impl<T: JetShow> JetShow for &T { fn jet_show(&self) -> String { (**self).jet_show() } }
impl<T: JetDisplay> JetDisplay for &T { fn jet_display(&self) -> String { (**self).jet_display() } }
impl<T: JetDebug> JetDebug for &T { fn jet_debug(&self) -> String { (**self).jet_debug() } }
impl<T: JetShow> JetShow for Vec<T> { fn jet_show(&self) -> String {
    let parts: Vec<String> = self.iter().map(|x| x.jet_show()).collect();
    format!("[{}]", parts.join(", "))
} }
impl<T: JetDisplay> JetDisplay for Vec<T> { fn jet_display(&self) -> String {
    let parts: Vec<String> = self.iter().map(|x| x.jet_display()).collect();
    format!("[{}]", parts.join(", "))
} }
impl<T: JetDebug> JetDebug for Vec<T> { fn jet_debug(&self) -> String {
    let parts: Vec<String> = self.iter().map(|x| x.jet_debug()).collect();
    format!("[{}]", parts.join(", "))
} }
// D-FIXARR1: `[T#N]` lowers to a real Rust array `[T; N]`; render it like a list
// so printing/interpolating a fixed array (or a fan-out result) works.
impl<T: JetShow, const N: usize> JetShow for [T; N] { fn jet_show(&self) -> String {
    let parts: Vec<String> = self.iter().map(|x| x.jet_show()).collect();
    format!("[{}]", parts.join(", "))
} }
impl<T: JetDisplay, const N: usize> JetDisplay for [T; N] { fn jet_display(&self) -> String {
    let parts: Vec<String> = self.iter().map(|x| x.jet_display()).collect();
    format!("[{}]", parts.join(", "))
} }
impl<T: JetDebug, const N: usize> JetDebug for [T; N] { fn jet_debug(&self) -> String {
    let parts: Vec<String> = self.iter().map(|x| x.jet_debug()).collect();
    format!("[{}]", parts.join(", "))
} }
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
impl<T: JetShow> JetShow for std::collections::VecDeque<T> { fn jet_show(&self) -> String {
    let parts: Vec<String> = self.iter().map(|x| x.jet_show()).collect();
    format!("[{}]", parts.join(", "))
} }
impl<T: JetDisplay> JetDisplay for std::collections::VecDeque<T> { fn jet_display(&self) -> String {
    let parts: Vec<String> = self.iter().map(|x| x.jet_display()).collect();
    format!("[{}]", parts.join(", "))
} }
impl<T: JetDebug> JetDebug for std::collections::VecDeque<T> { fn jet_debug(&self) -> String {
    let parts: Vec<String> = self.iter().map(|x| x.jet_debug()).collect();
    format!("[{}]", parts.join(", "))
} }
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
            Ok(v) => format!("ok({})", v.jet_show()),
            Err(e) => format!("err({})", e.jet_show()),
        }
    }
}
impl<T: JetDisplay, E: JetDisplay> JetDisplay for Result<T, E> {
    fn jet_display(&self) -> String {
        match self {
            Ok(v) => format!("ok({})", v.jet_display()),
            Err(e) => format!("err({})", e.jet_display()),
        }
    }
}
impl<T: JetDebug, E: JetDebug> JetDebug for Result<T, E> {
    fn jet_debug(&self) -> String {
        match self {
            Ok(v) => format!("ok({})", v.jet_debug()),
            Err(e) => format!("err({})", e.jet_debug()),
        }
    }
}
// D-PENDING1=B: async UI state machine — Idle/Loading/Loaded(T)/Failed(E).
#[derive(Clone, Debug)]
enum JetLoadable<T: Clone, E: Clone> { Idle, Loading, Loaded(T), Failed(E) }
impl<T: Clone, E: Clone> JetLoadable<T, E> {
    fn is_idle(&self) -> bool { matches!(self, JetLoadable::Idle) }
    fn is_loading(&self) -> bool { matches!(self, JetLoadable::Loading) }
    fn is_loaded(&self) -> bool { matches!(self, JetLoadable::Loaded(_)) }
    fn is_failed(&self) -> bool { matches!(self, JetLoadable::Failed(_)) }
    fn loaded(&self) -> Option<T> {
        if let JetLoadable::Loaded(v) = self { Some(v.clone()) } else { None }
    }
    fn or_else(&self, default: T) -> T {
        if let JetLoadable::Loaded(v) = self { v.clone() } else { default }
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
    fn jet_show(&self) -> String { "Expired".to_string() }
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
    fn is_valid(&self, now_ms: i64) -> bool { now_ms <= self.deadline_ms }
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
        JetRotting { value, deadline_ms, consumed: false }
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
        } else if let Some(v) = (&mut self.value as &mut dyn std::any::Any).downcast_mut::<Vec<u8>>() {
            for b in v.iter_mut() { *b = 0; }
            v.clear();
        }
    }
}
impl<T: Clone + JetShow> JetShow for JetRotting<T> {
    fn jet_show(&self) -> String {
        format!("Rotting(deadline={})", self.deadline_ms)
    }
}
// D-TIMEDEPTH1=A: civil-time types (Date, DateTime) and calendar math.
// Pure Rust, no external crates (I6). Proleptic Gregorian calendar.
#[derive(Clone, Debug, PartialEq)]
struct JetDate { year: i64, month: i64, day: i64 }
impl JetDate {
    fn is_leap(y: i64) -> bool { (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 }
    fn days_in_month(y: i64, m: i64) -> i64 {
        match m {
            1|3|5|7|8|10|12 => 31,
            4|6|9|11 => 30,
            2 => if Self::is_leap(y) { 29 } else { 28 },
            _ => 30,
        }
    }
    fn new(y: i64, m: i64, d: i64) -> Self { JetDate { year: y, month: m, day: d } }
    // Days since 0001-01-01 (proleptic Gregorian).
    fn to_day_number(&self) -> i64 {
        let y = self.year - 1;
        365 * y + y / 4 - y / 100 + y / 400
            + [0i64,31,59,90,120,151,181,212,243,273,304,334][(self.month - 1) as usize]
            + if self.month > 2 && Self::is_leap(self.year) { 1 } else { 0 }
            + self.day - 1
    }
    fn from_day_number(mut n: i64) -> Self {
        let mut y = n / 365 + 1;
        loop {
            let start = JetDate::new(y, 1, 1).to_day_number();
            let next = JetDate::new(y + 1, 1, 1).to_day_number();
            if n >= start && n < next { break; }
            if n < start { y -= 1; } else { y += 1; }
        }
        n -= JetDate::new(y, 1, 1).to_day_number();
        let mut m = 1i64;
        while m < 12 && n >= Self::days_in_month(y, m) { n -= Self::days_in_month(y, m); m += 1; }
        JetDate::new(y, m, n + 1)
    }
    fn parse(s: &str) -> Result<JetDate, String> {
        let parts: Vec<&str> = s.splitn(3, '-').collect();
        if parts.len() != 3 { return Err(format!("invalid date: {}", s)); }
        let y = parts[0].parse::<i64>().map_err(|_| format!("bad year: {}", parts[0]))?;
        let m = parts[1].parse::<i64>().map_err(|_| format!("bad month: {}", parts[1]))?;
        let d = parts[2].parse::<i64>().map_err(|_| format!("bad day: {}", parts[2]))?;
        if m < 1 || m > 12 || d < 1 || d > Self::days_in_month(y, m) {
            return Err(format!("date out of range: {}", s));
        }
        Ok(JetDate::new(y, m, d))
    }
    fn year(&self) -> i64 { self.year }
    fn month(&self) -> i64 { self.month }
    fn day(&self) -> i64 { self.day }
    fn add_days(&self, n: i64) -> JetDate { Self::from_day_number(self.to_day_number() + n) }
    fn add_months(&self, n: i64) -> JetDate {
        let total = self.month - 1 + n;
        let y = self.year + total / 12;
        let m = total % 12 + 1;
        let d = self.day.min(Self::days_in_month(y, m));
        JetDate::new(y, m, d)
    }
    fn diff_days(&self, other: &JetDate) -> i64 { self.to_day_number() - other.to_day_number() }
    fn weekday(&self) -> i64 {
        // 0=Mon, 6=Sun (ISO).
        (self.to_day_number() + 6) % 7
    }
    fn day_of_year(&self) -> i64 { self.to_day_number() - JetDate::new(self.year, 1, 1).to_day_number() + 1 }
    fn to_string_fmt(&self) -> String { format!("{:04}-{:02}-{:02}", self.year, self.month, self.day) }
    fn today_utc() -> JetDate {
        // Seconds since Unix epoch ÷ 86400 days.
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
        let days_since_1970 = secs / 86400;
        let epoch = JetDate::new(1970, 1, 1).to_day_number();
        JetDate::from_day_number(epoch + days_since_1970)
    }
}
impl JetShow for JetDate {
    fn jet_show(&self) -> String { self.to_string_fmt() }
}

#[derive(Clone, Debug, PartialEq)]
struct JetDateTime { secs: i64 } // seconds since Unix epoch (UTC)
impl JetDateTime {
    fn from_timestamp(secs: i64) -> Self { JetDateTime { secs } }
    fn now() -> Self {
        let s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
        JetDateTime { secs: s }
    }
    fn date(&self) -> JetDate {
        let days = self.secs.div_euclid(86400);
        let epoch = JetDate::new(1970, 1, 1).to_day_number();
        JetDate::from_day_number(epoch + days)
    }
    fn hour(&self) -> i64 { self.secs.div_euclid(3600) % 24 }
    fn minute(&self) -> i64 { self.secs.div_euclid(60) % 60 }
    fn second(&self) -> i64 { self.secs.rem_euclid(60) }
    fn to_timestamp(&self) -> i64 { self.secs }
    fn to_string_fmt(&self) -> String {
        let d = self.date();
        format!("{} {:02}:{:02}:{:02} UTC", d.to_string_fmt(), self.hour(), self.minute(), self.second())
    }
}
impl JetShow for JetDateTime {
    fn jet_show(&self) -> String { self.to_string_fmt() }
}

// D-AUTOPAR1=A: explicit parallel list adapters using std::thread::scope (I6-safe).
fn jet_list_par_map<T, U, F>(xs: Vec<T>, f: F) -> Vec<U>
where T: Send + Clone, U: Send, F: Fn(T) -> U + Sync
{
    std::thread::scope(|s| {
        let handles: Vec<_> = xs.into_iter().map(|x| s.spawn(|| f(x))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}
fn jet_list_par_filter<T, F>(xs: Vec<T>, f: F) -> Vec<T>
where T: Send + Clone, F: Fn(T) -> bool + Sync
{
    // Clone each element for the predicate thread; keep original for result.
    let pairs: Vec<(T, T)> = xs.into_iter().map(|x| (x.clone(), x)).collect();
    std::thread::scope(|s| {
        let (originals, clones): (Vec<T>, Vec<T>) = pairs.into_iter().unzip();
        let handles: Vec<_> = clones.into_iter().map(|x| s.spawn(|| f(x))).collect();
        originals.into_iter().zip(handles).filter_map(|(x, h)| if h.join().unwrap() { Some(x) } else { None }).collect()
    })
}
// par_fold(init, f) — sequential execution; parallelism requires an associative combiner
// the caller cannot provide separately. Semantically correct; future parallel version
// would accept a combine: Fn(U, U) -> U argument.
fn jet_list_par_fold<T, U, F>(xs: Vec<T>, init: U, f: F) -> U
where F: Fn(U, T) -> U
{
    xs.into_iter().fold(init, f)
}

// D-ADAPTFID1=A: Perf.fidelity() / Perf.set_fidelity(v) — global atomic f32.
static JET_PERF_FIDELITY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1065353216); // 1.0f32 bits
fn jet_perf_fidelity() -> f64 {
    let bits = JET_PERF_FIDELITY.load(std::sync::atomic::Ordering::Relaxed);
    f32::from_bits(bits) as f64
}
fn jet_perf_set_fidelity(v: f64) {
    let bits = (v as f32).to_bits();
    JET_PERF_FIDELITY.store(bits, std::sync::atomic::Ordering::Relaxed);
}

// ── D-APPROX1=A: core.sketch — approximate data structures ────────────────────
// FNV-1a: deterministic, I6-safe, no external crates.
fn fnv1a(data: &[u8]) -> u64 {
    let mut hash: u64 = 14695981039346656037;
    for &b in data { hash ^= b as u64; hash = hash.wrapping_mul(1099511628211); }
    hash
}
// Second independent hash (FNV with a different offset) for multi-hash sketches.
fn fnv1a_h2(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325u64.wrapping_add(0xdeadbeef);
    for &b in data { hash ^= b as u64; hash = hash.wrapping_mul(1099511628211); }
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
        let reg = (h & 0xFF) as usize;          // bottom 8 bits → register index
        let rest = h >> 8;                       // remaining 56 bits
        let lz = if rest == 0 { 57u8 } else { rest.leading_zeros() as u8 + 1 };
        let mut regs = self.0.lock().unwrap();
        if lz > regs[reg] { regs[reg] = lz; }
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
    fn jet_show(&self) -> String { format!("HyperLogLog(count={})", self.count()) }
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
            if merged.is_empty() { merged.push((mean, weight)); cum += weight; continue; }
            let last = merged.last_mut().unwrap();
            let q = cum / total;
            let limit = 4.0 * total * q * (1.0 - q) / Self::DELTA;
            if last.1 + weight <= limit.max(1.0) {
                let new_w = last.1 + weight;
                last.0 = (last.0 * last.1 + mean * weight) / new_w;
                last.1 = new_w;
            } else {
                merged.push((mean, weight)); cum += weight;
            }
        }
        *cs = merged;
    }
    fn quantile(&self, q: f64) -> f64 {
        let cs = self.0.lock().unwrap();
        if cs.is_empty() { return 0.0; }
        let total: f64 = cs.iter().map(|(_, w)| w).sum();
        let target = q * total;
        let mut cum = 0.0f64;
        for &(mean, weight) in cs.iter() {
            cum += weight;
            if cum >= target { return mean; }
        }
        cs.last().unwrap().0
    }
}
impl JetShow for JetTDigest {
    fn jet_show(&self) -> String { "TDigest".to_string() }
}

// CountMinSketch — frequency estimator. 4 rows × 256 cols; FNV + offset.
const CMS_COLS: usize = 256;
#[derive(Clone)]
struct JetCountMinSketch(std::sync::Arc<std::sync::Mutex<[[u32; CMS_COLS]; 4]>>);
impl JetCountMinSketch {
    fn new() -> Self {
        JetCountMinSketch(std::sync::Arc::new(std::sync::Mutex::new([[0u32; CMS_COLS]; 4])))
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
        (0..4usize).map(|row| {
            let col = ((h1.wrapping_add(h2.wrapping_mul(row as u64 + 1))) & 0xFF) as usize;
            tbl[row][col]
        }).min().unwrap() as i64
    }
}
impl JetShow for JetCountMinSketch {
    fn jet_show(&self) -> String { "CountMinSketch".to_string() }
}

// ReservoirSampler — uniform random sample. Seeded xorshift64 PRNG (I6-safe).
#[derive(Clone)]
struct JetReservoirSampler(std::sync::Arc<std::sync::Mutex<JetReservoirInner>>);
struct JetReservoirInner { capacity: usize, reservoir: Vec<String>, count: u64, rng: u64 }
impl Clone for JetReservoirInner {
    fn clone(&self) -> Self {
        JetReservoirInner {
            capacity: self.capacity, reservoir: self.reservoir.clone(),
            count: self.count, rng: self.rng,
        }
    }
}
impl JetReservoirSampler {
    fn new(capacity: i64) -> Self {
        let cap = (capacity.max(1)) as usize;
        JetReservoirSampler(std::sync::Arc::new(std::sync::Mutex::new(JetReservoirInner {
            capacity: cap, reservoir: Vec::with_capacity(cap), count: 0, rng: 0xdeadbeef_cafebabe,
        })))
    }
    fn add(&self, item: String) {
        let mut inner = self.0.lock().unwrap();
        inner.count += 1;
        if inner.reservoir.len() < inner.capacity {
            inner.reservoir.push(item);
        } else {
            // xorshift64
            let mut x = inner.rng;
            x ^= x << 13; x ^= x >> 7; x ^= x << 17;
            inner.rng = x;
            let j = (x % inner.count) as usize;
            if j < inner.capacity { inner.reservoir[j] = item; }
        }
    }
    fn sample(&self) -> Vec<String> {
        self.0.lock().unwrap().reservoir.clone()
    }
}
impl JetShow for JetReservoirSampler {
    fn jet_show(&self) -> String { format!("ReservoirSampler(n={})", self.0.lock().unwrap().count) }
}

thread_local! {
    static JET_IN_SCHEDULER_TASK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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

fn jet_panic(file: &str, line: u32, msg: &str) -> ! {
    if jet_scheduler_in_task() {
        panic!("{} (at {}:{})", msg, file, line);
    }
    eprintln!("panic: {}", msg);
    eprintln!("  --> {}:{}", file, line);
    std::process::exit(70);
}
/// E3005 (D-PREPOST1): a `@Pre`/`@Post` contract clause failed at runtime.
/// `clause_kw` is `"Pre"`/`"Post"`; `msg` is the clause's own message text
/// (the second argument to `@Pre(cond, "msg")`/`@Post(cond, "msg")`).
#[allow(dead_code)] // only called from generated code that has a @Pre/@Post
fn jet_contract_fail(file: &str, line: u32, clause_kw: &str, msg: &str) -> ! {
    if jet_scheduler_in_task() {
        panic!("@{} contract failed: {} (at {}:{})", clause_kw, msg, file, line);
    }
    eprintln!("@{} contract failed: {}", clause_kw, msg);
    eprintln!("  --> {}:{}", file, line);
    std::process::exit(70);
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
    if jet_scheduler_in_task() {
        panic!("{} (at {}:{})", msg, file, line);
    }
    std::process::exit(70);
}
/// E3002 (E2-M12, D-OBS1): error-return trace frame. In debug builds, when a `?`
/// actually propagates an `Err`, print one Zig-style frame to stderr, then hand
/// the `Result` back unchanged so the caller's `?` proceeds (incl. any
/// `From`/`to_error` conversion). In release builds this is a no-op.
fn jet_trace_err<T, E>(r: Result<T, E>, file: &str, line: u32, fn_name: &str) -> Result<T, E> {
    if cfg!(debug_assertions) && r.is_err() {
        eprintln!("error propagated from: {} ({}:{}) via ?", fn_name, file, line);
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
        jet_panic(file, line, &format!("the list has {} items, so position {} doesn't exist", len, i));
    }
    xs[i as usize].clone()
}
fn jet_unpack_vec<T: Clone>(xs: &[T], want: usize, i: usize, file: &str, line: u32) -> T {
    if xs.len() != want {
        jet_panic(file, line, &format!("this pattern needs exactly {} item{}, but the list has {}", want, if want == 1 { "" } else { "s" }, xs.len()));
    }
    xs[i].clone()
}
fn jet_slice_vec<T: Clone>(xs: &[T], a: i64, b: i64, file: &str, line: u32) -> Vec<T> {
    let len = xs.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(file, line, &format!("can't slice {} items from {} to {} (inclusive)", len, a, b));
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
        jet_panic(file, line, &format!("can't view {} items from {} to {} (inclusive)", len, a, b));
    }
    &xs[a as usize..=b as usize]
}
// D-DYNARRAY1: View<T> read-only closure surface. `xs` is already a borrow
// (never `.clone()`d to an owned `Vec` first, unlike the `jet_list_*` family
// above) — folding/mapping a view touches no allocation beyond the result.
fn jet_view_fold<T: Clone, U, F>(xs: &[T], init: U, f: F) -> U where F: Fn(U, T) -> U {
    let mut acc = init;
    for x in xs {
        acc = f(acc, x.clone());
    }
    acc
}
fn jet_view_map<T: Clone, U, F>(xs: &[T], f: F) -> Vec<U> where F: Fn(T) -> U {
    xs.iter().cloned().map(f).collect()
}
fn jet_index_map<K: Ord + Clone, V: Clone>(m: &std::collections::BTreeMap<K, V>, k: &K, file: &str, line: u32) -> V {
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
        jet_panic(file, line, &format!("the list has {} items, so position {} doesn't exist", len, i));
    }
    xs.remove(i as usize)
}
fn jet_char_len(s: &String) -> i64 { s.chars().count() as i64 }
fn jet_string_split(s: &String, sep: &str) -> Vec<String> { s.split(sep).map(|x| x.to_string()).collect() }
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
fn jet_string_lines(s: &String) -> Vec<String> { s.lines().map(|x| x.to_string()).collect() }
fn jet_string_slice(s: &String, a: i64, b: i64, file: &str, line: u32) -> String {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(file, line, &format!("can't slice {} characters from {} to {} (inclusive)", len, a, b));
    }
    chars[a as usize..=b as usize].iter().collect()
}
fn jet_list_map<T, U, F>(xs: Vec<T>, f: F) -> Vec<U> where F: Fn(T) -> U {
    xs.into_iter().map(f).collect()
}
fn jet_list_map_mut<T, U, F>(xs: Vec<T>, mut f: F) -> Vec<U> where F: FnMut(T) -> U {
    xs.into_iter().map(|x| f(x)).collect()
}
fn jet_list_filter<T: Clone, F>(xs: Vec<T>, f: F) -> Vec<T> where F: Fn(T) -> bool {
    xs.into_iter().filter(|x| f(x.clone())).collect()
}
fn jet_list_each<T, F>(xs: Vec<T>, f: F) where F: Fn(T) {
    for x in xs { f(x); }
}
fn jet_list_each_ref<T, F>(xs: &Vec<T>, f: F) where F: Fn(&T) {
    for x in xs.iter() { f(x); }
}
fn jet_list_each_mut<T, F>(xs: Vec<T>, mut f: F) where F: FnMut(T) {
    for x in xs { f(x); }
}
fn jet_list_find<T: Clone, F>(xs: Vec<T>, f: F) -> Option<T> where F: Fn(T) -> bool {
    xs.into_iter().find(|x| f(x.clone()))
}
fn jet_list_any<T: Clone, F>(xs: Vec<T>, f: F) -> bool where F: Fn(T) -> bool {
    xs.iter().any(|x| f(x.clone()))
}
fn jet_list_all<T: Clone, F>(xs: Vec<T>, f: F) -> bool where F: Fn(T) -> bool {
    xs.iter().all(|x| f(x.clone()))
}
fn jet_list_sort_by<T: Clone, K: Ord, F>(xs: &mut Vec<T>, f: F) where F: Fn(T) -> K {
    xs.sort_by_key(|x| f(x.clone()));
}
fn jet_list_reduce<T, U, F>(xs: Vec<T>, init: U, f: F) -> U where F: Fn(U, T) -> U {
    xs.into_iter().fold(init, f)
}
fn jet_map_each<K: Ord, V, F>(m: std::collections::BTreeMap<K, V>, f: F) where F: Fn(K, V) {
    for (k, v) in m { f(k, v); }
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
    JetReader { buf: bytes.clone(), pos: 0 }
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
    jet_reader_take_fixed(r, 4, "read_u32_le")
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn jet_reader_read_u32_be(r: &mut JetReader) -> Result<u32, String> {
    jet_reader_take_fixed(r, 4, "read_u32_be")
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}
fn jet_reader_read_u64_le(r: &mut JetReader) -> Result<u64, String> {
    jet_reader_take_fixed(r, 8, "read_u64_le").map(|b| {
        u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    })
}
fn jet_reader_read_u64_be(r: &mut JetReader) -> Result<u64, String> {
    jet_reader_take_fixed(r, 8, "read_u64_be").map(|b| {
        u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    })
}

fn jet_reader_take(r: &mut JetReader, n: i64) -> Result<Vec<u8>, String> {
    if n < 0 {
        return Err(format!("Reader.take: length must not be negative, got {}", n));
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
    JetCursor { buf: s.clone(), pos: 0 }
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
    if n <= 0 { return Vec::new(); }
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
    if n > xs.len() { return Vec::new(); }
    xs.windows(n).map(|w| w.to_vec()).collect()
}
fn jet_list_take_while<T: Clone, F>(xs: Vec<T>, f: F) -> Vec<T> where F: Fn(T) -> bool {
    xs.into_iter().take_while(|x| f(x.clone())).collect()
}
fn jet_list_skip_while<T: Clone, F>(xs: Vec<T>, f: F) -> Vec<T> where F: Fn(T) -> bool {
    xs.into_iter().skip_while(|x| f(x.clone())).collect()
}
fn jet_list_flat_map<T, U, F>(xs: Vec<T>, f: F) -> Vec<U> where F: Fn(T) -> Vec<U> {
    xs.into_iter().flat_map(f).collect()
}
fn jet_list_filter_map<T, U, E, F>(xs: Vec<T>, f: F) -> Vec<U> where F: Fn(T) -> Result<U, E> {
    xs.into_iter().filter_map(|x| f(x).ok()).collect()
}
fn jet_list_try_collect<T: Clone, E: Clone>(xs: Vec<Result<T, E>>) -> Result<Vec<T>, E> {
    xs.into_iter().collect()
}
fn jet_list_scan<T, U: Clone, F>(xs: Vec<T>, init: U, f: F) -> Vec<U>
where F: Fn(U, T) -> U {
    let mut acc = init;
    let mut out = Vec::new();
    for x in xs {
        acc = f(acc, x);
        out.push(acc.clone());
    }
    out
}
fn jet_list_fold<T, U, F>(xs: Vec<T>, init: U, f: F) -> U where F: Fn(U, T) -> U {
    xs.into_iter().fold(init, f)
}
fn jet_list_position<T: Clone, F>(xs: Vec<T>, f: F) -> Option<i64>
where F: Fn(T) -> bool {
    xs.into_iter().position(|x| f(x)).map(|i| i as i64)
}
fn jet_list_min_by<T: Clone, K: Ord, F>(xs: Vec<T>, f: F) -> Option<T>
where F: Fn(T) -> K {
    xs.into_iter().min_by_key(|x| f(x.clone()))
}
fn jet_list_max_by<T: Clone, K: Ord, F>(xs: Vec<T>, f: F) -> Option<T>
where F: Fn(T) -> K {
    xs.into_iter().max_by_key(|x| f(x.clone()))
}
fn jet_list_group_by<T: Clone, K: Ord + Clone, F>(
    xs: Vec<T>, f: F
) -> std::collections::BTreeMap<K, Vec<T>>
where F: Fn(T) -> K {
    let mut m: std::collections::BTreeMap<K, Vec<T>> = std::collections::BTreeMap::new();
    for x in xs {
        let k = f(x.clone());
        m.entry(k).or_default().push(x);
    }
    m
}
/// `partition(f)` — splits into (true-list, false-list) as a named-tuple struct.
/// `build` receives `(true_vec, false_vec)` and wraps them into the JetTup struct.
fn jet_list_partition<T: Clone, F, S, B>(xs: Vec<T>, f: F, build: B) -> S
where
    F: Fn(T) -> bool,
    B: FnOnce(Vec<T>, Vec<T>) -> S,
{
    let mut yes: Vec<T> = Vec::new();
    let mut no: Vec<T> = Vec::new();
    for x in xs {
        if f(x.clone()) { yes.push(x); } else { no.push(x); }
    }
    build(yes, no)
}
// ── D-DEFER1 option B: core.scope.guard ──────────────────────────────────────
// A ScopeGuard stores a zero-argument closure and runs it in Drop — on every
// exit path (normal fall-through, early `return`, `?` propagation).
// LIFO ordering is guaranteed by Rust's reverse-declaration drop order.
// Generic over F: avoids boxing and allows non-'static captures. Purely safe.
struct JetScopeGuard<F: FnOnce()> {
    f: Option<F>,
}
fn jet_scope_guard<F: FnOnce()>(f: F) -> JetScopeGuard<F> {
    JetScopeGuard { f: Some(f) }
}
impl<F: FnOnce()> Drop for JetScopeGuard<F> {
    fn drop(&mut self) {
        if let Some(f) = self.f.take() { f(); }
    }
}
// ── D-TXN1–D-TXN4 + D-TXN-ROLLBACK (2026-06-24/25): #Transact transaction blocks
// A `#Transact(tx) { … }` block lowers to:
//   { let mut tx = jet_transaction(); <body>; tx.commit(); }
//
// Three Drop-backed hook stacks, each LIFO (mirroring scope-guard drop order):
//   • commit hooks   (`tx.on_commit(() => …)`, D-TXN3) — run only if `commit()` ran.
//   • rollback hooks (`tx.on_rollback(() => …)`, D-TXN-ROLLBACK layer 3) — run only
//     if `commit()` did NOT run (a `?`-failure / early return undoes the block).
//   • auto-snapshots (D-TXN-ROLLBACK layer 1) — restore closures captured at the
//     point of mutation; run only if `commit()` did NOT run, restoring the pre-state.
//
// A `?`-failure (or any early return) inside the block skips `commit()`, so on Drop:
//   committed   → commit hooks fire LIFO; rollback hooks + snapshots are dropped un-run.
//   uncommitted → snapshots restore (LIFO) then rollback hooks fire (LIFO); commit
//                 hooks are dropped un-run.
// Purely safe std Rust; no runtime effect machinery (I3).
struct JetTransaction {
    hooks: Vec<Box<dyn FnOnce()>>,
    undo: Vec<Box<dyn FnOnce()>>,
    committed: bool,
}
fn jet_transaction() -> JetTransaction {
    JetTransaction { hooks: Vec::new(), undo: Vec::new(), committed: false }
}
impl JetTransaction {
    fn on_commit(&mut self, f: Box<dyn FnOnce()>) {
        self.hooks.push(f);
    }
    fn on_rollback(&mut self, f: Box<dyn FnOnce()>) {
        self.undo.push(f);
    }
    fn commit(&mut self) {
        self.committed = true;
    }
}
impl Drop for JetTransaction {
    fn drop(&mut self) {
        if self.committed {
            // Clean commit: run commit hooks LIFO; undo stack is dropped un-run.
            while let Some(f) = self.hooks.pop() {
                f();
            }
        } else {
            // Rollback path (`?`-failure / early return): restore auto-snapshots
            // and run explicit rollback hooks, both LIFO; commit hooks drop un-run.
            // `undo` holds both kinds interleaved in registration order, so a single
            // LIFO drain mirrors the source order they were established in.
            while let Some(f) = self.undo.pop() {
                f();
            }
        }
    }
}
// D-TXN-ROLLBACK layer 1 (auto-snapshot): the snapshot/restore mechanism lives in a
// vetted prelude module, mirroring `jet_mem`. `jet_txn_snapshot` clones the
// pre-mutation state of a place and registers a Drop-backed restore on the
// transaction's undo stack; on a `?`-failure the guard's Drop writes the clone back.
// The raw-pointer writeback is sound because the transaction guard is declared after
// the place and dropped before it (LIFO scope teardown), so the place is always live
// when restore runs. The compiler picks WHICH places to snapshot (I3); this module is
// just the dumb runtime. Stripped from the golden memory-safety check like `jet_mem`.
mod jet_txn {
    use super::JetTransaction;
    /// Snapshot `*place` (a `Clone` of its pre-mutation state) and register a restore
    /// closure on `tx`'s undo stack. Restores on rollback; dropped un-run on commit.
    pub(crate) fn snapshot<T: Clone + 'static>(tx: &mut JetTransaction, place: &mut T) {
        let saved = place.clone();
        let raw: *mut T = place;
        tx.on_rollback(Box::new(move || {
            // `raw` points at a local that outlives the transaction guard; the
            // guard's Drop (the caller) runs before that local is dropped.
            let slot: &mut T = unsafe { &mut *raw };
            *slot = saved;
        }));
    }
    /// D-TXN-ROLLBACK layer 2: snapshot a value via its `Rollback` impl instead of
    /// a full `Clone`. The caller captures the snap by calling `place.snapshot()` and
    /// passes it together with the type-erased `restore` function pointer. Sound for
    /// the same reason as `snapshot`: the place outlives the transaction guard (LIFO).
    pub(crate) fn snapshot_custom<T: 'static, S: 'static>(
        tx: &mut JetTransaction,
        place: &mut T,
        snap: S,
        restore: fn(&mut T, S),
    ) {
        let raw: *mut T = place;
        tx.on_rollback(Box::new(move || {
            let slot: &mut T = unsafe { &mut *raw };
            restore(slot, snap);
        }));
    }
}
trait user_Serialize { fn to_json(&self) -> String; }

// ── D-TERM1 (ratified 2026-06-22): terminal direct-input primitives ───────────
// `live { … }` blocks in Jet source emit:
//   jet_term_enter();
//   let _live_guard = jet_scope_guard(|| { jet_term_leave(); });
//   <body>
//
// `term.read_key()` emits `jet_term_read_key()`.
//
// I6: zero external crates. Platform-specific setup uses inline `extern "C"` /
// `extern "system"` declarations — standard Rust FFI, not the `libc` crate.
// ──────────────────────────────────────────────────────────────────────────────

/// The key-event type returned by `term.read_key()` (D-TERM1).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum JetKey {
    /// A printable character.
    Char(char),
    /// Enter / Return.
    Enter,
    /// Escape.
    Escape,
    /// Backspace.
    Backspace,
    /// Tab.
    Tab,
    /// Delete (forward delete).
    Delete,
    /// Arrow up.
    Up,
    /// Arrow down.
    Down,
    /// Arrow left.
    Left,
    /// Arrow right.
    Right,
    /// Function key F1–F12.
    F(i64),
    /// Ctrl + a printable character (e.g. Ctrl-C = Char('\x03')).
    Ctrl(char),
    /// Anything else (bytes we could not parse into a known sequence).
    Unknown,
}

impl JetShow for JetKey {
    fn jet_show(&self) -> String {
        match self {
            JetKey::Char(c) => format!("Char({})", c),
            JetKey::Enter => "Enter".to_string(),
            JetKey::Escape => "Escape".to_string(),
            JetKey::Backspace => "Backspace".to_string(),
            JetKey::Tab => "Tab".to_string(),
            JetKey::Delete => "Delete".to_string(),
            JetKey::Up => "Up".to_string(),
            JetKey::Down => "Down".to_string(),
            JetKey::Left => "Left".to_string(),
            JetKey::Right => "Right".to_string(),
            JetKey::F(n) => format!("F({})", n),
            JetKey::Ctrl(c) => format!("Ctrl({})", c),
            JetKey::Unknown => "Unknown".to_string(),
        }
    }
}

#[cfg(unix)]
mod jet_term_unix {
    use std::io::Read;

    // POSIX termios constants (POSIX.1-2008). We inline these rather than
    // depending on `libc` (I6).
    const TCSANOW: i32 = 0;
    const ECHO: u32 = 0o0000010;
    const ICANON: u32 = 0o0000002;
    const VMIN: usize = 6;
    const VTIME: usize = 5;

    // Termios struct layout for Linux/macOS (glibc + Darwin agree on the fields
    // that matter here; we only touch `c_lflag` and `c_cc`).
    #[repr(C)]
    struct Termios {
        c_iflag: u32,
        c_oflag: u32,
        c_cflag: u32,
        c_lflag: u32,
        #[cfg(target_os = "linux")]
        c_line: u8,
        c_cc: [u8; 32],
        #[cfg(target_os = "linux")]
        c_ispeed: u32,
        #[cfg(target_os = "linux")]
        c_ospeed: u32,
        // macOS pads the c_cc array to 20 bytes inside a struct that's 60 bytes
        // total. We over-allocate to cover both layouts safely.
        #[cfg(not(target_os = "linux"))]
        _pad: [u8; 12],
    }

    extern "C" {
        fn tcgetattr(fd: i32, termios: *mut Termios) -> i32;
        fn tcsetattr(fd: i32, optional_actions: i32, termios: *const Termios) -> i32;
    }

    // Thread-local storage for the saved terminal state so `jet_term_leave`
    // can restore exactly what `jet_term_enter` captured.
    std::thread_local! {
        static SAVED: std::cell::RefCell<Option<Termios>> = std::cell::RefCell::new(None);
    }

    pub fn enter() {
        unsafe {
            let mut t = std::mem::zeroed::<Termios>();
            if tcgetattr(0, &mut t) != 0 { return; }
            SAVED.with(|s| *s.borrow_mut() = Some(std::mem::transmute_copy(&t)));
            t.c_lflag &= !(ECHO | ICANON);
            t.c_cc[VMIN] = 1;
            t.c_cc[VTIME] = 0;
            tcsetattr(0, TCSANOW, &t);
        }
    }

    pub fn leave() {
        unsafe {
            SAVED.with(|s| {
                if let Some(saved) = s.borrow().as_ref() {
                    tcsetattr(0, TCSANOW, saved as *const Termios);
                }
            });
        }
    }

    pub fn read_key() -> super::JetKey {
        use super::JetKey;
        let mut buf = [0u8; 6];
        let stdin = std::io::stdin();
        let n = stdin.lock().read(&mut buf).unwrap_or(0);
        if n == 0 { return JetKey::Unknown; }
        match &buf[..n] {
            [0x0d] | [0x0a] => JetKey::Enter,
            [0x1b] if n == 1 => JetKey::Escape,
            [0x7f] | [0x08] => JetKey::Backspace,
            [0x09] => JetKey::Tab,
            // CSI sequences: ESC [ …
            [0x1b, 0x5b, rest @ ..] => parse_csi(rest),
            // Ctrl + letter: bytes 0x01–0x1a (A–Z).
            [b] if *b >= 1 && *b <= 26 => JetKey::Ctrl((b'a' - 1 + *b) as char),
            [b] if *b < 0x80 => JetKey::Char(*b as char),
            // Multi-byte UTF-8 character.
            bytes => {
                if let Ok(s) = std::str::from_utf8(bytes) {
                    if let Some(c) = s.chars().next() {
                        return JetKey::Char(c);
                    }
                }
                JetKey::Unknown
            }
        }
    }

    fn parse_csi(rest: &[u8]) -> super::JetKey {
        use super::JetKey;
        match rest {
            [0x41] => JetKey::Up,
            [0x42] => JetKey::Down,
            [0x43] => JetKey::Right,
            [0x44] => JetKey::Left,
            [0x33, 0x7e] => JetKey::Delete,
            // F1–F4: ESC O P/Q/R/S (VT100) — handled as CSI variant here.
            // F1–F12 numeric: ESC [ 1 1 ~ through ESC [ 2 4 ~
            bytes => {
                // Try numeric Pn ~ form: digits followed by ~.
                if let Some((&0x7e, digits)) = bytes.split_last() {
                    if let Ok(s) = std::str::from_utf8(digits) {
                        if let Ok(n) = s.parse::<i64>() {
                            let fkey = match n {
                                11 => 1, 12 => 2, 13 => 3, 14 => 4,
                                15 => 5, 17 => 6, 18 => 7, 19 => 8,
                                20 => 9, 21 => 10, 23 => 11, 24 => 12,
                                _ => return JetKey::Unknown,
                            };
                            return JetKey::F(fkey);
                        }
                    }
                }
                JetKey::Unknown
            }
        }
    }
}

#[cfg(windows)]
mod jet_term_windows {
    use std::io::Read;

    extern "system" {
        fn GetStdHandle(nStdHandle: u32) -> *mut std::ffi::c_void;
        fn GetConsoleMode(hConsoleHandle: *mut std::ffi::c_void, lpMode: *mut u32) -> i32;
        fn SetConsoleMode(hConsoleHandle: *mut std::ffi::c_void, dwMode: u32) -> i32;
    }

    const STD_INPUT_HANDLE: u32 = 0xFFFFFFF6u32;
    const ENABLE_ECHO_INPUT: u32 = 0x0004;
    const ENABLE_LINE_INPUT: u32 = 0x0002;

    std::thread_local! {
        static SAVED: std::cell::RefCell<Option<u32>> = std::cell::RefCell::new(None);
    }

    pub fn enter() {
        unsafe {
            let h = GetStdHandle(STD_INPUT_HANDLE);
            let mut mode: u32 = 0;
            if GetConsoleMode(h, &mut mode) == 0 { return; }
            SAVED.with(|s| *s.borrow_mut() = Some(mode));
            let new_mode = mode & !(ENABLE_ECHO_INPUT | ENABLE_LINE_INPUT);
            SetConsoleMode(h, new_mode);
        }
    }

    pub fn leave() {
        unsafe {
            let h = GetStdHandle(STD_INPUT_HANDLE);
            SAVED.with(|s| {
                if let Some(saved) = *s.borrow() {
                    SetConsoleMode(h, saved);
                }
            });
        }
    }

    pub fn read_key() -> super::JetKey {
        use super::JetKey;
        let mut buf = [0u8; 6];
        let n = std::io::stdin().lock().read(&mut buf).unwrap_or(0);
        if n == 0 { return JetKey::Unknown; }
        match &buf[..n] {
            [0x0d] | [0x0a] => JetKey::Enter,
            [0x1b] => JetKey::Escape,
            [0x7f] | [0x08] => JetKey::Backspace,
            [0x09] => JetKey::Tab,
            [0x1b, 0x5b, rest @ ..] => {
                match rest {
                    [0x41] => JetKey::Up,
                    [0x42] => JetKey::Down,
                    [0x43] => JetKey::Right,
                    [0x44] => JetKey::Left,
                    _ => JetKey::Unknown,
                }
            }
            [b] if *b >= 1 && *b <= 26 => JetKey::Ctrl((b'a' - 1 + *b) as char),
            [b] if *b < 0x80 => JetKey::Char(*b as char),
            bytes => {
                if let Ok(s) = std::str::from_utf8(bytes) {
                    if let Some(c) = s.chars().next() { return JetKey::Char(c); }
                }
                JetKey::Unknown
            }
        }
    }
}

// ── Platform-dispatched entry points ────────────────────────────────────────

/// Enter un-buffered, no-echo terminal input mode.
/// Called at the top of every `live { … }` block.
fn jet_term_enter() {
    #[cfg(unix)]
    jet_term_unix::enter();
    #[cfg(windows)]
    jet_term_windows::enter();
    #[cfg(not(any(unix, windows)))]
    {} // no-op on unsupported targets (freestanding blocks sema-rejected)
}

/// Restore the terminal to the state captured by the most recent `jet_term_enter`.
/// Called by the scope guard that `live { … }` installs.
fn jet_term_leave() {
    #[cfg(unix)]
    jet_term_unix::leave();
    #[cfg(windows)]
    jet_term_windows::leave();
    #[cfg(not(any(unix, windows)))]
    {}
}

/// Read one key event from stdin (blocking).
/// Used by `term.read_key()`.
fn jet_term_read_key() -> JetKey {
    #[cfg(unix)]
    return jet_term_unix::read_key();
    #[cfg(windows)]
    return jet_term_windows::read_key();
    #[cfg(not(any(unix, windows)))]
    return JetKey::Unknown;
}
