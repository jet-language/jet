// ── D-ITERTOOLS1=A: expanded collection/runtime handles ─────────────────────
#[derive(Clone)]
struct JetCache<K, V> {
    cap: usize,
    entries: Vec<(K, V)>,
}

impl<K: Eq + Clone, V: Clone> JetCache<K, V> {
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

impl<K: JetShow, V: JetShow> JetShow for JetCache<K, V> {
    fn jet_show(&self) -> String {
        let parts: Vec<String> = self
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
fn jet_iter_some<T: 'static>(it: JetIter<T>) -> JetIter<Option<T>> {
    JetIter(Box::new(it.0.map(Some)))
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
        match (a.0.next(), b.0.next()) {
            (Some(x), Some(y)) => Some(f(x, y)),
            (None, None) => None,
            (None, Some(_)) | (Some(_), None) => {
                jet_panic("<core.collections>", 0, "zip length mismatch")
            }
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
        match (a.0.next(), b.0.next()) {
            (Some(x), Some(y)) => Some(f(x, y)),
            (Some(x), None) => Some(f(x, fill_b.clone())),
            (None, Some(y)) => Some(f(fill_a.clone(), y)),
            (None, None) => None,
        }
    })))
}

// List-shaped helpers kept for non-Iter call sites / terminals that still
// materialize; adapters above are the lazy path.
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
fn jet_list_position<T, F, I>(xs: I, mut f: F) -> Option<i64>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> bool,
{
    xs.into_iter().position(|x| f(&x)).map(|i| i as i64)
}
fn jet_list_min_by<T, K: Ord, F, I>(xs: I, f: F) -> Option<T>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> K,
{
    xs.into_iter().min_by_key(f)
}
fn jet_list_max_by<T, K: Ord, F, I>(xs: I, f: F) -> Option<T>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> K,
{
    xs.into_iter().max_by_key(f)
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

fn jet_iter_cycle<T: 'static + Clone>(it: JetIter<T>) -> JetIter<T> {
    let xs = it.to_list();
    JetIter(Box::new(xs.into_iter().cycle()))
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

fn jet_iter_last_index_of<T: 'static + PartialEq>(it: JetIter<T>, needle: T) -> Option<i64> {
    let xs = it.to_list();
    xs.iter().rposition(|x| x == &needle).map(|i| i as i64)
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
fn jet_list_binary_search<T: Ord>(xs: &[T], needle: &T) -> Option<i64> {
    xs.binary_search(needle).ok().map(|i| i as i64)
}
fn jet_list_binary_search_by<T, F>(xs: &[T], mut f: F) -> Option<i64>
where F: FnMut(&T) -> std::cmp::Ordering {
    xs.binary_search_by(|x| f(x)).ok().map(|i| i as i64)
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
fn jet_list_random<T: Clone>(xs: &[T]) -> Option<T> {
    if xs.is_empty() { return None; }
    let mut state: u64 = 0xC0FF_EE42;
    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
    Some(xs[((state >> 33) as usize) % xs.len()].clone())
}
fn jet_list_replace<T: Clone + PartialEq>(xs: &[T], old: &T, new: T) -> Vec<T> {
    xs.iter().map(|x| if x == old { new.clone() } else { x.clone() }).collect()
}
fn jet_list_min_max<T: Ord + Clone, R>(xs: &[T], build: impl FnOnce(T, T) -> R) -> Option<R> {
    Some(build(xs.iter().min()?.clone(), xs.iter().max()?.clone()))
}
fn jet_list_min_max_by<T: Clone, K: Ord, F, R>(xs: &[T], mut f: F, build: impl FnOnce(T, T) -> R) -> Option<R>
where F: FnMut(&T) -> K {
    Some(build(xs.iter().min_by_key(|x| f(x))?.clone(), xs.iter().max_by_key(|x| f(x))?.clone()))
}

