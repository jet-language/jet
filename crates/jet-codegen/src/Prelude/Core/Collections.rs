// ── D-ITERTOOLS1=A: expanded collection/runtime handles ─────────────────────
#[derive(Clone)]
struct JetCache<K, V> {
    lru: JetLru<K, V>,
}

impl<K: Eq + Clone, V: Clone> JetCache<K, V> {
    fn new(capacity: i64) -> Self {
        Self {
            lru: JetLru::new(Some(capacity.max(0) as usize)),
        }
    }
    fn put(&mut self, key: K, value: V) -> Option<V> {
        self.lru.put(key, value)
    }
    fn add_new(&mut self, key: K, value: V) -> bool {
        self.lru.add_new(key, value)
    }
    fn get(&mut self, key: &K) -> Option<V> {
        self.lru.get(key)
    }
    fn remove(&mut self, key: &K) -> Option<V> {
        self.lru.remove(key)
    }
    fn contains_key(&self, key: &K) -> bool {
        self.lru.contains_key(key)
    }
    fn keys(&self) -> Vec<K> {
        self.lru.keys()
    }
    fn len(&self) -> usize {
        self.lru.len()
    }
    fn is_empty(&self) -> bool {
        self.lru.is_empty()
    }
    fn capacity(&self) -> i64 {
        self.lru.capacity()
    }
    fn clear(&mut self) {
        self.lru.clear();
    }
}

impl<K: JetShow, V: JetShow> JetShow for JetCache<K, V> {
    fn jet_show(&self) -> String {
        let parts: Vec<String> = self
            .lru
            .entries
            .iter()
            .map(|(k, v)| format!("{}: {}", k.jet_show(), v.jet_show()))
            .collect();
        format!("[:{}]", parts.join(", "))
    }
}
impl<K: JetDisplay, V: JetDisplay> JetDisplay for JetCache<K, V> {
    fn jet_display(&self) -> String {
        let parts: Vec<String> = self
            .lru
            .entries
            .iter()
            .map(|(k, v)| format!("{}: {}", k.jet_display(), v.jet_display()))
            .collect();
        format!("[:{}]", parts.join(", "))
    }
}
impl<K: JetDebug, V: JetDebug> JetDebug for JetCache<K, V> {
    fn jet_debug(&self) -> String {
        let parts: Vec<String> = self
            .lru
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

fn jet_bits_copy(bits: &JetBitSet) -> JetBitSet {
    bits.clone()
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

fn jet_list_sum<T, I>(xs: I) -> T
where
    I: IntoIterator<Item = T>,
    T: std::iter::Sum<T>,
{
    xs.into_iter().sum()
}
fn jet_list_product<T, I>(xs: I) -> T
where
    I: IntoIterator<Item = T>,
    T: std::iter::Product<T>,
{
    xs.into_iter().product()
}
fn jet_list_copy<T: Clone>(xs: &[T]) -> Vec<T> {
    xs.to_vec()
}
fn jet_list_min<T: Ord, I>(xs: I) -> JetOutcome<T, JetAbsent>
where
    I: IntoIterator<Item = T>,
{
    jet_outcome_of(xs.into_iter().min())
}
fn jet_list_max<T: Ord, I>(xs: I) -> JetOutcome<T, JetAbsent>
where
    I: IntoIterator<Item = T>,
{
    jet_outcome_of(xs.into_iter().max())
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
fn jet_list_count_by<T, K: Ord + Clone, F, I>(xs: I, mut f: F) -> JetMap<K, i64>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> K,
{
    let mut m: JetMap<K, i64> = JetMap::new();
    for x in xs {
        let k = f(&x);
        *m.entry(k).or_insert(0) += 1;
    }
    m
}

fn jet_map_copy_kernel<K: Ord + Clone, V: Clone>(m: &JetMap<K, V>) -> JetMap<K, V> {
    m.clone()
}

fn jet_map_equal_kernel<K: Ord + PartialEq, V: PartialEq>(
    left: &JetMap<K, V>,
    right: &JetMap<K, V>,
) -> bool {
    left == right
}

fn jet_map_first_key_kernel<K: Ord + Clone, V>(
    m: &JetMap<K, V>,
) -> JetOutcome<K, JetAbsent> {
    jet_outcome_of(m.keys().next().cloned())
}

fn jet_map_entries_kernel<K: Ord + Clone, V: Clone>(m: &JetMap<K, V>) -> Vec<(K, V)> {
    m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

fn jet_map_min_value_kernel<K: Ord, V: Ord + Clone>(
    m: &JetMap<K, V>,
) -> JetOutcome<V, JetAbsent> {
    jet_outcome_of(m.values().min().cloned())
}

fn jet_map_max_value_kernel<K: Ord, V: Ord + Clone>(
    m: &JetMap<K, V>,
) -> JetOutcome<V, JetAbsent> {
    jet_outcome_of(m.values().max().cloned())
}

fn jet_map_intersection_kernel<K: Ord + Clone, V: Clone>(
    left: &JetMap<K, V>,
    right: &JetMap<K, V>,
) -> JetMap<K, V> {
    left.iter()
        .filter(|(key, _)| right.contains_key(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn jet_map_slice_keys_kernel<K: Ord + Clone, V: Clone>(
    m: &JetMap<K, V>,
    keys: Vec<K>,
) -> JetMap<K, V> {
    keys.into_iter()
        .filter_map(|key| m.get(&key).cloned().map(|value| (key, value)))
        .collect()
}

fn jet_map_from_keys_kernel<K: Ord + Clone, V: Clone>(
    keys: Vec<K>,
    default: V,
) -> JetMap<K, V> {
    keys.into_iter()
        .map(|key| (key, default.clone()))
        .collect()
}

fn jet_map_contains_value_kernel<K: Ord, V: PartialEq>(
    m: &JetMap<K, V>,
    needle: &V,
) -> bool {
    m.values().any(|value| value == needle)
}

fn jet_map_pop_kernel<K: Ord + Clone, V: Clone>(
    m: &mut JetMap<K, V>,
    key: &K,
) -> JetOutcome<V, JetAbsent> {
    jet_outcome_of(m.remove(key))
}

fn jet_list_pop_kernel<T>(xs: &mut Vec<T>) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(xs.pop())
}

trait JetSetPopKernel {
    type Item;

    fn pop_value(&mut self, value: &Self::Item) -> Option<Self::Item>;
}

impl<T: Eq + std::hash::Hash> JetSetPopKernel for std::collections::HashSet<T> {
    type Item = T;

    fn pop_value(&mut self, value: &Self::Item) -> Option<Self::Item> {
        self.take(value)
    }
}

impl<T: PartialEq> JetSetPopKernel for Vec<T> {
    type Item = T;

    fn pop_value(&mut self, value: &Self::Item) -> Option<Self::Item> {
        self.iter().position(|item| item == value).map(|index| self.remove(index))
    }
}

fn jet_set_pop_kernel<C: JetSetPopKernel>(
    set: &mut C,
    value: &C::Item,
) -> JetOutcome<C::Item, JetAbsent> {
    jet_outcome_of(set.pop_value(value))
}

trait JetDequePopFrontKernel {
    type Item;

    fn pop_front_value(&mut self) -> Option<Self::Item>;
}

impl<T> JetDequePopFrontKernel for std::collections::VecDeque<T> {
    type Item = T;

    fn pop_front_value(&mut self) -> Option<Self::Item> {
        self.pop_front()
    }
}

impl<T> JetDequePopFrontKernel for Vec<T> {
    type Item = T;

    fn pop_front_value(&mut self) -> Option<Self::Item> {
        (!self.is_empty()).then(|| self.remove(0))
    }
}

fn jet_deque_pop_front_kernel<C: JetDequePopFrontKernel>(
    deque: &mut C,
) -> JetOutcome<C::Item, JetAbsent> {
    jet_outcome_of(deque.pop_front_value())
}

trait JetDequePopBackKernel {
    type Item;

    fn pop_back_value(&mut self) -> Option<Self::Item>;
}

impl<T> JetDequePopBackKernel for std::collections::VecDeque<T> {
    type Item = T;

    fn pop_back_value(&mut self) -> Option<Self::Item> {
        self.pop_back()
    }
}

impl<T> JetDequePopBackKernel for Vec<T> {
    type Item = T;

    fn pop_back_value(&mut self) -> Option<Self::Item> {
        self.pop()
    }
}

fn jet_deque_pop_back_kernel<C: JetDequePopBackKernel>(
    deque: &mut C,
) -> JetOutcome<C::Item, JetAbsent> {
    jet_outcome_of(deque.pop_back_value())
}

trait JetPriorityQueuePopKernel {
    type Item;

    fn pop_priority_value(&mut self) -> Option<Self::Item>;
}

impl<T: Ord> JetPriorityQueuePopKernel for std::collections::BinaryHeap<T> {
    type Item = T;

    fn pop_priority_value(&mut self) -> Option<Self::Item> {
        self.pop()
    }
}

impl<T> JetPriorityQueuePopKernel for Vec<T> {
    type Item = T;

    fn pop_priority_value(&mut self) -> Option<Self::Item> {
        (!self.is_empty()).then(|| self.remove(0))
    }
}

fn jet_priority_queue_pop_kernel<C: JetPriorityQueuePopKernel>(
    queue: &mut C,
) -> JetOutcome<C::Item, JetAbsent> {
    jet_outcome_of(queue.pop_priority_value())
}

fn jet_map_pop_first_kernel<K: Ord + Clone, V: Clone>(
    m: &mut JetMap<K, V>,
) -> JetOutcome<V, JetAbsent> {
    let Some(key) = m.keys().next().cloned() else {
        return Err(JetAbsent);
    };
    jet_outcome_of(m.remove(&key))
}

// ── D-ITER1 / D-ITERTOOLS1=A: true lazy iterator fusion ──────────────────────
// Adapters return `JetIter<T>` = boxed `dyn Iterator`. No intermediate Vec until
// `to_list` / `collect` / a terminal reducer. Closures are `'static` (Jet emits
// `move` lambdas / capture prep for escaping adapter callbacks).
struct JetIter<T>(Box<dyn Iterator<Item = T>>);

impl<T: 'static> JetIter<T> {
    fn to_list(self) -> Vec<T> {
        self.0.collect()
    }
    fn collect(self) -> Vec<T> {
        self.0.collect()
    }
    fn len(self) -> i64 {
        self.0.count() as i64
    }
    fn is_empty(mut self) -> bool {
        self.0.next().is_none()
    }
    fn first(mut self) -> JetOutcome<T, JetAbsent> {
        jet_outcome_of(self.0.next())
    }
}

impl<T> IntoIterator for JetIter<T> {
    type Item = T;
    type IntoIter = Box<dyn Iterator<Item = T>>;
    fn into_iter(self) -> Self::IntoIter {
        self.0
    }
}

fn jet_iter_from_vec<T: 'static>(xs: Vec<T>) -> JetIter<T> {
    JetIter(Box::new(xs.into_iter()))
}

/// Lazy `String.split` — yields owned pieces on pull (no intermediate Vec of parts).
/// Empty `sep` matches `jet_string_split` / Rust `str::split("")`: leading empty,
/// one Char string per scalar, trailing empty.
fn jet_iter_string_split(s: &String, sep: &str) -> JetIter<String> {
    let s = s.clone();
    let sep = sep.to_string();
    if sep.is_empty() {
        // Index into owned `s` — `s.chars()` would borrow and break `'static` JetIter.
        let mut offset = 0usize;
        // 0 = leading empty; 1 = chars; 2 = done after trailing empty.
        let mut phase = 0u8;
        return JetIter(Box::new(std::iter::from_fn(move || {
            match phase {
                0 => {
                    phase = 1;
                    Some(String::new())
                }
                1 => {
                    if offset >= s.len() {
                        phase = 2;
                        return Some(String::new());
                    }
                    let ch = s[offset..].chars().next().expect("offset in bounds");
                    let len = ch.len_utf8();
                    let out = s[offset..offset + len].to_string();
                    offset += len;
                    Some(out)
                }
                _ => None,
            }
        })));
    }
    let mut start = 0usize;
    let mut done = false;
    JetIter(Box::new(std::iter::from_fn(move || {
        if done {
            return None;
        }
        match s[start..].find(&sep) {
            Some(rel) => {
                let end = start + rel;
                let part = s[start..end].to_string();
                start = end + sep.len();
                Some(part)
            }
            None => {
                done = true;
                Some(s[start..].to_string())
            }
        }
    })))
}

/// Lazy `String.rsplit` — same left-to-right part order as Python `str.rsplit`
/// without a limit (Rust's `rsplit` yields right-to-left; reverse after collect).
fn jet_iter_string_rsplit(s: &String, sep: &str) -> JetIter<String> {
    if sep.is_empty() {
        return jet_iter_string_split(s, sep);
    }
    let mut parts: Vec<String> = s.rsplit(sep).map(|p| p.to_string()).collect();
    parts.reverse();
    jet_iter_from_vec(parts)
}

fn jet_iter_take<T: 'static>(it: JetIter<T>, n: i64) -> JetIter<T> {
    JetIter(Box::new(it.0.take(n.max(0) as usize)))
}
fn jet_iter_skip<T: 'static>(it: JetIter<T>, n: i64) -> JetIter<T> {
    JetIter(Box::new(it.0.skip(n.max(0) as usize)))
}
fn jet_iter_step_by<T: 'static>(it: JetIter<T>, n: i64) -> JetIter<T> {
    if n <= 0 {
        return JetIter(Box::new(std::iter::empty()));
    }
    JetIter(Box::new(it.0.step_by(n as usize)))
}

struct JetDedupIter<T> {
    inner: Box<dyn Iterator<Item = T>>,
    prev: Option<T>,
}
impl<T: Clone + PartialEq> Iterator for JetDedupIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        while let Some(x) = self.inner.next() {
            if self.prev.as_ref() == Some(&x) {
                continue;
            }
            self.prev = Some(x.clone());
            return Some(x);
        }
        None
    }
}
fn jet_iter_dedup<T: 'static + Clone + PartialEq>(it: JetIter<T>) -> JetIter<T> {
    JetIter(Box::new(JetDedupIter {
        inner: it.0,
        prev: None,
    }))
}

struct JetChunksIter<T> {
    inner: Box<dyn Iterator<Item = T>>,
    size: usize,
}
impl<T> Iterator for JetChunksIter<T> {
    type Item = Vec<T>;
    fn next(&mut self) -> Option<Vec<T>> {
        let mut chunk = Vec::with_capacity(self.size);
        for _ in 0..self.size {
            match self.inner.next() {
                Some(x) => chunk.push(x),
                None => break,
            }
        }
        if chunk.is_empty() {
            None
        } else {
            Some(chunk)
        }
    }
}
fn jet_iter_chunks<T: 'static>(it: JetIter<T>, n: i64) -> JetIter<Vec<T>> {
    JetIter(Box::new(JetChunksIter {
        inner: it.0,
        size: n.max(1) as usize,
    }))
}

struct JetWindowsIter<T> {
    inner: Box<dyn Iterator<Item = T>>,
    size: usize,
    buf: std::collections::VecDeque<T>,
}
impl<T: Clone> Iterator for JetWindowsIter<T> {
    type Item = Vec<T>;
    fn next(&mut self) -> Option<Vec<T>> {
        while self.buf.len() < self.size {
            self.buf.push_back(self.inner.next()?);
        }
        let out: Vec<T> = self.buf.iter().cloned().collect();
        self.buf.pop_front();
        Some(out)
    }
}
fn jet_iter_windows<T: 'static + Clone>(it: JetIter<T>, n: i64) -> JetIter<Vec<T>> {
    JetIter(Box::new(JetWindowsIter {
        inner: it.0,
        size: n.max(1) as usize,
        buf: std::collections::VecDeque::new(),
    }))
}

fn jet_iter_map<T: 'static, U: 'static, F: 'static>(it: JetIter<T>, mut f: F) -> JetIter<U>
where
    F: FnMut(&T) -> U,
{
    JetIter(Box::new(it.0.map(move |x| f(&x))))
}
fn jet_iter_map_mut<T: 'static, U: 'static, F: 'static>(it: JetIter<T>, mut f: F) -> JetIter<U>
where
    F: FnMut(&T) -> U,
{
    JetIter(Box::new(it.0.map(move |x| f(&x))))
}
fn jet_iter_filter<T: 'static, F: 'static>(it: JetIter<T>, mut f: F) -> JetIter<T>
where
    F: FnMut(&T) -> bool,
{
    JetIter(Box::new(it.0.filter(move |x| f(x))))
}
fn jet_iter_take_while<T: 'static, F: 'static>(it: JetIter<T>, mut f: F) -> JetIter<T>
where
    F: FnMut(&T) -> bool,
{
    JetIter(Box::new(it.0.take_while(move |x| f(x))))
}
fn jet_iter_skip_while<T: 'static, F: 'static>(it: JetIter<T>, mut f: F) -> JetIter<T>
where
    F: FnMut(&T) -> bool,
{
    JetIter(Box::new(it.0.skip_while(move |x| f(x))))
}
fn jet_iter_flat_map<T: 'static, U: 'static, F: 'static>(it: JetIter<T>, mut f: F) -> JetIter<U>
where
    F: FnMut(&T) -> Vec<U>,
{
    JetIter(Box::new(it.0.flat_map(move |x| f(&x))))
}
fn jet_iter_filter_map<T: 'static, U: 'static, E: 'static, F: 'static>(
    it: JetIter<T>,
    mut f: F,
) -> JetIter<U>
where
    F: FnMut(&T) -> Result<U, E>,
{
    JetIter(Box::new(it.0.filter_map(move |x| f(&x).ok())))
}
fn jet_iter_scan<T: 'static, U: 'static + Clone, F: 'static>(
    it: JetIter<T>,
    init: U,
    mut f: F,
) -> JetIter<U>
where
    F: FnMut(&U, &T) -> U,
{
    let mut acc = init;
    JetIter(Box::new(it.0.map(move |x| {
        acc = f(&acc, &x);
        acc.clone()
    })))
}
fn jet_iter_flatten<T: 'static>(it: JetIter<Vec<T>>) -> JetIter<T> {
    JetIter(Box::new(it.0.flatten()))
}

struct JetIntersperseIter<T> {
    inner: Box<dyn Iterator<Item = T>>,
    sep: T,
    turn_sep: bool,
    next_item: Option<T>,
    started: bool,
}
impl<T: Clone> Iterator for JetIntersperseIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        if !self.started {
            self.started = true;
            return self.inner.next();
        }
        if self.turn_sep {
            self.turn_sep = false;
            self.next_item = self.inner.next();
            if self.next_item.is_some() {
                return Some(self.sep.clone());
            }
            return None;
        }
        self.turn_sep = true;
        self.next_item.take()
    }
}
fn jet_iter_intersperse<T: 'static + Clone>(it: JetIter<T>, sep: T) -> JetIter<T> {
    JetIter(Box::new(JetIntersperseIter {
        inner: it.0,
        sep,
        turn_sep: true,
        next_item: None,
        started: false,
    }))
}
fn jet_iter_enumerate<T: 'static, U: 'static, F: 'static>(
    it: JetIter<T>,
    mut f: F,
) -> JetIter<U>
where
    F: FnMut(i64, T) -> U,
{
    JetIter(Box::new(
        it.0.enumerate()
            .map(move |(i, x)| f(i as i64, x)),
    ))
}
/// D-RANGE-EXCL1=C: every valid Int index for a sequence of length `n`.
fn jet_iter_indexes(n: i64) -> JetIter<i64> {
    let n = n.max(0);
    JetIter(Box::new((0..n).map(|i| i)))
}
fn jet_iter_zip<A: 'static, B: 'static, O: 'static, F: 'static>(
    a: JetIter<A>,
    b: JetIter<B>,
    mut f: F,
) -> JetIter<O>
where
    F: FnMut(A, B) -> O,
{
    JetIter(Box::new(a.0.zip(b.0).map(move |(x, y)| f(x, y))))
}
fn jet_iter_empty<T: 'static>() -> JetIter<T> {
    JetIter(Box::new(std::iter::empty()))
}
// D-FAIL-CARRIER1=A: padding a short side yields carrier values, so a zip
// column reads as `T?` and nothing else.
fn jet_iter_some<T: 'static>(it: JetIter<T>) -> JetIter<JetOutcome<T, JetAbsent>> {
    JetIter(Box::new(it.0.map(Ok)))
}
fn jet_iter_zip_strict<A: 'static, B: 'static, O: 'static, F: 'static>(
    mut a: JetIter<A>,
    mut b: JetIter<B>,
    mut f: F,
) -> JetIter<O>
where
    F: FnMut(A, B) -> O,
{
    JetIter(Box::new(std::iter::from_fn(move || {
        match jet_zip_strict_step(a.0.next(), b.0.next()) {
            Ok(Some((x, y))) => Some(f(x, y)),
            Ok(None) => None,
            Err(()) => jet_panic("<core.collections>", 0, "zip length mismatch"),
        }
    })))
}
fn jet_iter_zip_pad<A: 'static + Clone, B: 'static + Clone, O: 'static, F: 'static>(
    mut a: JetIter<A>,
    mut b: JetIter<B>,
    fill_a: A,
    fill_b: B,
    mut f: F,
) -> JetIter<O>
where
    F: FnMut(A, B) -> O,
{
    JetIter(Box::new(std::iter::from_fn(move || {
        match jet_zip_pad_step(a.0.next(), b.0.next(), fill_a.clone(), fill_b.clone()) {
            Some((x, y)) => Some(f(x, y)),
            None => None,
        }
    })))
}

// D-CORE-EAGER1=A / D-LOOPMAP1=B: concrete collection map/filter are eager.
// `.lazy()` enters the JetIter plane, where the same names remain deferred.
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
/// Adjacent eager adapters may be fused when the intermediate list is not
/// observable. The callbacks still run in source order, once per element.
fn jet_list_map_filter<T, U, F, P>(xs: Vec<T>, mut map: F, mut keep: P) -> Vec<U>
where
    F: FnMut(&T) -> U,
    P: FnMut(&U) -> bool,
{
    xs.iter().map(|x| map(x)).filter(|value| keep(value)).collect()
}

// List-shaped helpers kept for non-Iter call sites / terminals that still
// materialize; non-map/filter adapters above remain the lazy path.
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
fn jet_list_try_collect<T, E, I>(xs: I) -> Result<Vec<T>, E>
where
    I: IntoIterator<Item = Result<T, E>>,
{
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
fn jet_list_fold<T, U, F, I>(xs: I, init: U, mut f: F) -> U
where
    I: IntoIterator<Item = T>,
    F: FnMut(&U, &T) -> U,
{
    xs.into_iter().fold(init, |acc, x| f(&acc, &x))
}
fn jet_list_position<T, F, I>(xs: I, mut f: F) -> JetOutcome<i64, JetAbsent>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> bool,
{
    jet_outcome_of(xs.into_iter().position(|x| f(&x)).map(|i| i as i64))
}
fn jet_list_min_by<T, K: Ord, F, I>(xs: I, f: F) -> JetOutcome<T, JetAbsent>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> K,
{
    jet_outcome_of(xs.into_iter().min_by_key(f))
}
fn jet_list_max_by<T, K: Ord, F, I>(xs: I, f: F) -> JetOutcome<T, JetAbsent>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> K,
{
    jet_outcome_of(xs.into_iter().max_by_key(f))
}
fn jet_list_group_by<T: Clone, K: Ord + Clone, F, I>(xs: I, mut f: F) -> JetMap<K, Vec<T>>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> K,
{
    let mut m: JetMap<K, Vec<T>> = JetMap::new();
    for x in xs {
        let k = f(&x);
        m.entry(k).or_default().push(x);
    }
    m
}
/// `partition(f)` — splits into (true-list, false-list) as a named-tuple struct.
/// `build` receives `(true_vec, false_vec)` and wraps them into the JetTup struct.
fn jet_list_partition<T, F, S, B, I>(xs: I, mut f: F, build: B) -> S
where
    I: IntoIterator<Item = T>,
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

// ── #1479: remaining Iter ledger surface ─────────────────────────────────────
fn jet_iter_repeat<T: 'static + Clone>(it: JetIter<T>, n: i64) -> JetIter<T> {
    let xs = it.to_list();
    let n = n.max(0) as usize;
    JetIter(Box::new((0..n).flat_map(move |_| xs.clone().into_iter())))
}

/// D-ITER1: bounded cycle — produces exactly `n` items by looping the
/// source (not `n` loops; `jet_iter_repeat` covers "loop n times"). A
/// 0-arg infinite cycle has no safe representation across every execution
/// tier (I9), so `.cycle(n)` is the only shipped form.
fn jet_iter_cycle<T: 'static + Clone>(it: JetIter<T>, n: i64) -> JetIter<T> {
    let xs = it.to_list();
    let n = n.max(0) as usize;
    if xs.is_empty() {
        return jet_iter_from_vec(Vec::new());
    }
    JetIter(Box::new(xs.into_iter().cycle().take(n)))
}

fn jet_iter_drop_last<T: 'static>(it: JetIter<T>, n: i64) -> JetIter<T> {
    let mut xs = it.to_list();
    let n = n.max(0) as usize;
    if n >= xs.len() {
        xs.clear();
    } else {
        xs.truncate(xs.len() - n);
    }
    jet_iter_from_vec(xs)
}

fn jet_iter_shuffle<T: 'static>(it: JetIter<T>) -> JetIter<T> {
    let mut xs = it.to_list();
    // Stable demo shuffle (not crypto). Seeded LCG so goldens are deterministic.
    let mut state: u64 = 0xC0FF_EE42;
    for i in (1..xs.len()).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = ((state >> 33) as usize) % (i + 1);
        xs.swap(i, j);
    }
    jet_iter_from_vec(xs)
}

fn jet_iter_is_sorted<T: 'static + Ord>(it: JetIter<T>) -> bool {
    let xs = it.to_list();
    xs.windows(2).all(|w| w[0] <= w[1])
}

fn jet_iter_is_sorted_by<T: 'static, K: Ord, F>(it: JetIter<T>, mut f: F) -> bool
where
    F: FnMut(&T) -> K,
{
    let xs = it.to_list();
    xs.windows(2).all(|w| f(&w[0]) <= f(&w[1]))
}

fn jet_iter_dedup_by<T: 'static + Clone, K: PartialEq, F>(it: JetIter<T>, mut f: F) -> JetIter<T>
where
    F: 'static + FnMut(&T) -> K,
{
    let xs = it.to_list();
    let mut out: Vec<T> = Vec::new();
    let mut prev_key: Option<K> = None;
    for x in xs {
        let key = f(&x);
        if prev_key.as_ref() == Some(&key) {
            continue;
        }
        prev_key = Some(key);
        out.push(x);
    }
    jet_iter_from_vec(out)
}

fn jet_iter_last_index_of<T: 'static + PartialEq>(it: JetIter<T>, needle: T) -> JetOutcome<i64, JetAbsent> {
    let xs = it.to_list();
    jet_outcome_of(xs.iter().rposition(|x| x == &needle).map(|i| i as i64))
}

fn jet_iter_average_int(it: JetIter<i64>) -> f64 {
    let xs = it.to_list();
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().sum::<i64>() as f64 / xs.len() as f64
    }
}

fn jet_iter_average_float(it: JetIter<f64>) -> f64 {
    let xs = it.to_list();
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().sum::<f64>() / xs.len() as f64
    }
}

fn jet_iter_compare<T: 'static + Ord>(it: JetIter<T>, other: Vec<T>) -> i64 {
    match it.to_list().as_slice().cmp(other.as_slice()) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn jet_iter_split_at<T: 'static, R>(
    it: JetIter<T>,
    n: i64,
    build: impl FnOnce(Vec<T>, Vec<T>) -> R,
) -> R {
    let mut xs = it.to_list();
    let n = n.max(0) as usize;
    if n >= xs.len() {
        build(xs, Vec::new())
    } else {
        let right = xs.split_off(n);
        build(xs, right)
    }
}

fn jet_iter_chunk_while<T: 'static + Clone, F>(it: JetIter<T>, mut f: F) -> JetIter<Vec<T>>
where
    F: 'static + FnMut(&T, &T) -> bool,
{
    let xs = it.to_list();
    let mut chunks: Vec<Vec<T>> = Vec::new();
    for x in xs {
        if let Some(last) = chunks.last_mut() {
            if f(last.last().unwrap(), &x) {
                last.push(x);
                continue;
            }
        }
        chunks.push(vec![x]);
    }
    jet_iter_from_vec(chunks)
}

fn jet_iter_to_set<T: Eq + std::hash::Hash>(it: JetIter<T>) -> std::collections::HashSet<T> {
    it.into_iter().collect()
}

// #1477 List ledger surface
fn jet_list_slice<T: Clone>(xs: &[T], start: i64, end: i64) -> Vec<T> {
    let len = xs.len() as i64;
    let s = start.clamp(0, len) as usize;
    let e = end.clamp(0, len) as usize;
    if e <= s { Vec::new() } else { xs[s..e].to_vec() }
}
fn jet_list_binary_search<T: Ord>(xs: &[T], needle: &T) -> JetOutcome<i64, JetAbsent> {
    jet_outcome_of(xs.binary_search(needle).ok().map(|i| i as i64))
}
fn jet_list_binary_search_by<T, F>(xs: &[T], mut f: F) -> JetOutcome<i64, JetAbsent>
where F: FnMut(&T) -> std::cmp::Ordering {
    jet_outcome_of(xs.binary_search_by(|x| f(x)).ok().map(|i| i as i64))
}
fn jet_list_union<T: Clone + Eq>(left: &[T], right: &[T]) -> Vec<T> {
    let mut out = left.to_vec();
    for x in right { if !out.contains(x) { out.push(x.clone()); } }
    out
}
fn jet_list_intersection<T: Clone + Eq>(left: &[T], right: &[T]) -> Vec<T> {
    let mut out = Vec::new();
    for x in left {
        if right.contains(x) && !out.contains(x) { out.push(x.clone()); }
    }
    out
}
fn jet_list_difference<T: Clone + Eq>(left: &[T], right: &[T]) -> Vec<T> {
    left.iter().filter(|x| !right.contains(x)).cloned().collect()
}
fn jet_list_random<T: Clone>(xs: &[T]) -> JetOutcome<T, JetAbsent> {
    if xs.is_empty() { return Err(JetAbsent); }
    let mut state: u64 = 0xC0FF_EE42;
    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
    Ok(xs[((state >> 33) as usize) % xs.len()].clone())
}
fn jet_list_replace<T: Clone>(xs: &[T], index: i64, new: T) -> Vec<T> {
    let mut out = xs.to_vec();
    if let Some(slot) = index.try_into().ok().and_then(|i: usize| out.get_mut(i)) {
        *slot = new;
    }
    out
}
fn jet_list_min_max<T: Ord + Clone, R>(xs: &[T], build: impl FnOnce(T, T) -> R) -> JetOutcome<R, JetAbsent> {
    match (xs.iter().min(), xs.iter().max()) {
        (Some(lo), Some(hi)) => Ok(build(lo.clone(), hi.clone())),
        _ => Err(JetAbsent),
    }
}
fn jet_list_min_max_by<T: Clone, K: Ord, F, R>(xs: &[T], mut f: F, build: impl FnOnce(T, T) -> R) -> JetOutcome<R, JetAbsent>
where F: FnMut(&T) -> K {
    match (xs.iter().min_by_key(|x| f(x)).cloned(), xs.iter().max_by_key(|x| f(x)).cloned()) {
        (Some(lo), Some(hi)) => Ok(build(lo, hi)),
        _ => Err(JetAbsent),
    }
}

fn jet_list_starts_with<T: PartialEq>(xs: &[T], prefix: &[T]) -> bool {
    xs.starts_with(prefix)
}

fn jet_list_ends_with<T: PartialEq>(xs: &[T], suffix: &[T]) -> bool {
    xs.ends_with(suffix)
}

fn jet_list_equal<T: PartialEq>(left: &[T], right: &[T]) -> bool {
    left == right
}

fn jet_list_unzip<T, U, I>(xs: I) -> (Vec<T>, Vec<U>)
where
    I: IntoIterator<Item = (T, U)>,
{
    xs.into_iter().unzip()
}

// D-LISTREMOVE1/F: PriorityQueue removal uses the same canonical
// highest-first order as `peek` and `to_sorted_list`.  Keep the mutation and
// selector semantics in this shared Prelude kernel so AOT and resident JIT
// adapters cannot drift.
fn jet_priority_queue_remove_value_kernel<T: Ord>(
    pq: &mut std::collections::BinaryHeap<T>,
    value: T,
) -> JetOutcome<T, JetAbsent> {
    let mut items: Vec<T> = std::mem::take(pq).into_sorted_vec();
    items.reverse();
    let found = items
        .iter()
        .position(|item| *item == value)
        .map(|index| items.remove(index));
    *pq = items.into_iter().collect();
    jet_outcome_of(found)
}

fn jet_priority_queue_remove_slot_kernel<T: Ord>(
    pq: &mut std::collections::BinaryHeap<T>,
    i: i64,
    _file: &str,
    _line: u32,
) -> Result<JetOutcome<T, JetAbsent>, String> {
    let mut items: Vec<T> = std::mem::take(pq).into_sorted_vec();
    items.reverse();
    let len = items.len() as i64;
    if i < 0 || i >= len {
        *pq = items.into_iter().collect();
        return Err(format!(
            "the priority queue has {} items, so position {} doesn't exist",
            len, i
        ));
    }
    let removed = items.remove(i as usize);
    *pq = items.into_iter().collect();
    Ok(jet_present(removed))
}
