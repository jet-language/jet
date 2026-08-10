trait JetShow {
    fn jet_show(&self) -> String;
    /// D-FAIL-CARRIER1=A: is this the carrier's clean report? The optional view
    /// answers yes, so `T?` prints its payload bare and its absence as `null`;
    /// every failure report answers no, so `T ? E` prints its verdict.
    fn jet_report_is_clean() -> bool
    where
        Self: Sized,
    {
        false
    }
}
/// D-DISPLAYDBG1: user-facing interpolation (`{value}`).
trait JetDisplay {
    fn jet_display(&self) -> String;
    /// D-FAIL-CARRIER1=A: see `JetShow::jet_report_is_clean`.
    fn jet_report_is_clean() -> bool
    where
        Self: Sized,
    {
        false
    }
}
/// D-ATTR4=A: developer interpolation (`{value:Debug}`).
trait JetDebug {
    fn jet_debug(&self) -> String;
    /// D-FAIL-CARRIER1=A: see `JetShow::jet_report_is_clean`.
    fn jet_report_is_clean() -> bool
    where
        Self: Sized,
    {
        false
    }
}

/// D-QUANTITY-TYPE1=A: internal compile-time bridge for physical-unit generic
/// bounds. Concrete unit identity stays in the
/// monomorphized type; this trait adds no runtime metadata.
trait JetQuantity: Sized {
    fn raw(&self) -> f64;
    fn from_float(value: f64) -> Self;
}
impl JetQuantity for f64 {
    fn raw(&self) -> f64 { *self }
    fn from_float(value: f64) -> Self { value }
}

// D-RANGE-VALUE1=A: one allocation-free integer range value. Both source
// spellings use this type; `exclusive` selects the half-open end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct JetRange {
    start: i64,
    end: i64,
    exclusive: bool,
}
impl JetRange {
    fn contains(&self, value: &i64) -> bool {
        jet_range_contains(self.start, self.end, self.exclusive, *value)
    }

    fn structural_text(&self) -> String {
        jet_range_structural_text(self.start, self.end, self.exclusive)
    }
}
impl JetShow for JetRange {
    fn jet_show(&self) -> String {
        self.structural_text()
    }
}
impl __jet_Display for JetRange {
    fn display(&self) -> String {
        self.structural_text()
    }
}
impl JetDisplay for JetRange {
    fn jet_display(&self) -> String {
        <Self as __jet_Display>::display(self)
    }
}
impl JetDebug for JetRange {
    fn jet_debug(&self) -> String {
        self.structural_text()
    }
}
impl __jet_Equatable for JetRange {
    fn equal(&self, rhs: &Self) -> bool {
        jet_range_equal(
            self.start,
            self.end,
            self.exclusive,
            rhs.start,
            rhs.end,
            rhs.exclusive,
        )
    }
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
        jet_debug_map(
            self.iter()
                .map(|(key, value)| (key.jet_show(), value.jet_show())),
        )
    }
}
impl<K: Ord + JetDisplay, V: JetDisplay> JetDisplay for std::collections::BTreeMap<K, V> {
    fn jet_display(&self) -> String {
        jet_debug_map(
            self.iter()
                .map(|(key, value)| (key.jet_display(), value.jet_display())),
        )
    }
}
impl<K: Ord + JetDebug, V: JetDebug> JetDebug for std::collections::BTreeMap<K, V> {
    fn jet_debug(&self) -> String {
        jet_debug_map(
            self.iter()
                .map(|(key, value)| (key.jet_debug(), value.jet_debug())),
        )
    }
}
impl<K: Ord + JetShow, V: JetShow> JetShow for JetMap<K, V> {
    fn jet_show(&self) -> String {
        jet_debug_map(self.iter().map(|(key, value)| (key.jet_show(), value.jet_show())))
    }
}
impl<K: Ord + JetDisplay, V: JetDisplay> JetDisplay for JetMap<K, V> {
    fn jet_display(&self) -> String {
        jet_debug_map(self.iter().map(|(key, value)| (key.jet_display(), value.jet_display())))
    }
}
impl<K: Ord + JetDebug, V: JetDebug> JetDebug for JetMap<K, V> {
    fn jet_debug(&self) -> String {
        jet_debug_map(self.iter().map(|(key, value)| (key.jet_debug(), value.jet_debug())))
    }
}
// D-FAIL-CARRIER1=A: one carrier, so one printer. The report type says which
// view is being read: the clean report prints the payload bare and `null` for
// an absence; a failure report prints the verdict around them.
impl JetShow for JetAbsent {
    fn jet_show(&self) -> String {
        "null".to_string()
    }
    fn jet_report_is_clean() -> bool {
        true
    }
}
impl JetDisplay for JetAbsent {
    fn jet_display(&self) -> String {
        "null".to_string()
    }
    fn jet_report_is_clean() -> bool {
        true
    }
}
impl JetDebug for JetAbsent {
    fn jet_debug(&self) -> String {
        "null".to_string()
    }
    fn jet_report_is_clean() -> bool {
        true
    }
}
impl JetShow for JetTaskFailure {
    fn jet_show(&self) -> String {
        match self {
            JetTaskFailure::Cancelled => "Cancelled".to_string(),
            JetTaskFailure::DeadlineBlown => "DeadlineBlown".to_string(),
            JetTaskFailure::Panicked(reason) => format!("Panicked({reason})"),
        }
    }
}
impl JetDisplay for JetTaskFailure {
    fn jet_display(&self) -> String {
        self.jet_show()
    }
}
impl JetDebug for JetTaskFailure {
    fn jet_debug(&self) -> String {
        match self {
            JetTaskFailure::Cancelled => "Cancelled".to_string(),
            JetTaskFailure::DeadlineBlown => "DeadlineBlown".to_string(),
            JetTaskFailure::Panicked(reason) => format!("Panicked({reason:?})"),
        }
    }
}
impl<T: JetShow, E: JetShow> JetShow for JetOutcome<T, E> {
    fn jet_show(&self) -> String {
        let clean = <E as JetShow>::jet_report_is_clean();
        match self {
            Ok(v) if clean => v.jet_show(),
            Ok(v) => format!("Ok({})", v.jet_show()),
            Err(e) if clean => e.jet_show(),
            Err(e) => format!("Err({})", e.jet_show()),
        }
    }
}
impl<T: JetDisplay, E: JetDisplay> JetDisplay for JetOutcome<T, E> {
    fn jet_display(&self) -> String {
        let clean = <E as JetDisplay>::jet_report_is_clean();
        match self {
            Ok(v) if clean => v.jet_display(),
            Ok(v) => format!("Ok({})", v.jet_display()),
            Err(e) if clean => e.jet_display(),
            Err(e) => format!("Err({})", e.jet_display()),
        }
    }
}
impl<T: JetDebug, E: JetDebug> JetDebug for JetOutcome<T, E> {
    fn jet_debug(&self) -> String {
        let clean = <E as JetDebug>::jet_report_is_clean();
        match self {
            Ok(v) if clean => v.jet_debug(),
            Ok(v) => format!("Ok({})", v.jet_debug()),
            Err(e) if clean => e.jet_debug(),
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
    fn loadable_tag(&self) -> u8 {
        match self {
            JetLoadable::Idle => JET_LOADABLE_IDLE,
            JetLoadable::Loading => JET_LOADABLE_LOADING,
            JetLoadable::Loaded(_) => JET_LOADABLE_LOADED,
            JetLoadable::Failed(_) => JET_LOADABLE_FAILED,
        }
    }
    fn is_idle(&self) -> bool {
        jet_loadable_is_tag(self.loadable_tag(), JET_LOADABLE_IDLE)
    }
    fn is_loading(&self) -> bool {
        jet_loadable_is_tag(self.loadable_tag(), JET_LOADABLE_LOADING)
    }
    fn is_loaded(&self) -> bool {
        jet_loadable_has_value(self.loadable_tag())
    }
    fn is_failed(&self) -> bool {
        jet_loadable_is_tag(self.loadable_tag(), JET_LOADABLE_FAILED)
    }
    fn loaded(&self) -> JetOutcome<T, JetAbsent> {
        if jet_loadable_has_value(self.loadable_tag()) {
            let JetLoadable::Loaded(v) = self else { unreachable!() };
            Ok(v.clone())
        } else {
            Err(JetAbsent)
        }
    }
    fn or_else(&self, default: T) -> T {
        if jet_loadable_has_value(self.loadable_tag()) {
            let JetLoadable::Loaded(v) = self else { unreachable!() };
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
// D-CORE-SECRETS1=A: generic TTL remains separate from secret lifecycle.
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
