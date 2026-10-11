// ── D-ITERTOOLS1=A: expanded collection/runtime handles ─────────────────────
#[inline(always)]
fn jet_collection_native_int(value: &jet_foundation::Numeric::JetInt) -> i64 {
    value.to_native_i64().unwrap_or_else(|error| {
        jet_panic_rich_code(
            "E3010", "<core.collections>", 0, "", "", 0, 0, error, "",
        )
    })
}

impl<K: PartialEq + Clone, V: Clone> JetLru<K, V> {
    fn add_new(&mut self, key: K, value: V) -> bool {
        if self.bound == Some(0) || self.entries.iter().any(|(stored, _)| stored == &key) {
            return false;
        }
        self.put(key, value);
        true
    }

    fn remove<Q: ?Sized>(&mut self, key: &Q) -> Option<V> where K: std::borrow::Borrow<Q>, Q: PartialEq {
        let index = self.entries.iter().position(|(stored, _)| stored.borrow() == key)?;
        Some(self.entries.remove(index).1)
    }

    fn contains_key<Q: ?Sized>(&self, key: &Q) -> bool where K: std::borrow::Borrow<Q>, Q: PartialEq {
        self.entries.iter().any(|(stored, _)| stored.borrow() == key)
    }

    fn keys(&self) -> Vec<K> {
        self.entries.iter().map(|(key, _)| key.clone()).collect()
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn capacity(&self) -> i64 {
        self.bound.map(|bound| bound as i64).unwrap_or(i64::MAX)
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

#[derive(Clone)]
struct JetCache<K, V> {
    lru: JetLru<K, V>,
}

impl<K: Eq + Clone, V: Clone> JetCache<K, V> {
    fn new(capacity: jet_foundation::Numeric::JetInt) -> Self {
        let capacity = jet_collection_native_int(&capacity);
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
    fn get<Q: ?Sized>(&mut self, key: &Q) -> Option<V> where K: std::borrow::Borrow<Q>, Q: PartialEq {
        self.lru.get_matching(|stored| stored.borrow() == key)
    }
    fn remove<Q: ?Sized>(&mut self, key: &Q) -> Option<V> where K: std::borrow::Borrow<Q>, Q: PartialEq {
        self.lru.remove(key)
    }
    fn contains_key<Q: ?Sized>(&self, key: &Q) -> bool where K: std::borrow::Borrow<Q>, Q: PartialEq {
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
        jet_bits_has_kernel(&self.bits, *bit)
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

/// One `Bits` membership definition for every execution tier: AOT reaches it
/// through `JetBitSet::contains`, and the Cranelift host reaches it directly on
/// the resident bit set, so neither tier re-encodes the test (I9).
fn jet_bits_has_kernel(bits: &std::collections::BTreeSet<i64>, bit: i64) -> bool {
    bits.contains(&bit)
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

/// D-FRED1=A: Float collection reductions use the shared eight-lane Prelude
/// tree; the generic `sum` helper remains the left-to-right standard fold for
/// element types without this ratified fixed-order rule.
#[inline(always)]
fn jet_list_sum_fixed_f32<I: IntoIterator<Item = f32>>(xs: I) -> f32 {
    jet_simd_reduce_fixed_iter(xs, 0.0)
}

#[inline(always)]
fn jet_list_sum_fixed_f64<I: IntoIterator<Item = f64>>(xs: I) -> f64 {
    jet_simd_reduce_fixed_iter(xs, 0.0)
}

/// D-FRED1=A: Float `fold` with scalar addition uses the same fixed seed
/// placement and lane tree as `sum`.
#[inline(always)]
fn jet_list_fold_add_fixed_f32<I: IntoIterator<Item = f32>>(xs: I, init: f32) -> f32 {
    jet_simd_reduce_fixed_iter(xs, init)
}

#[inline(always)]
fn jet_list_fold_add_fixed_f64<I: IntoIterator<Item = f64>>(xs: I, init: f64) -> f64 {
    jet_simd_reduce_fixed_iter(xs, init)
}

/// Apply one selected D-ACCEL1 column-copy pass over bounded cache blocks.
/// Callers that already ran a serial probe can prepend that probe's typed
/// result and invoke this helper only for the remaining range.
#[inline(always)]
fn jet_list_accel_column_copy_range_map<U, R, P, C>(
    range: std::ops::Range<usize>,
    project: P,
    copied_kernel: C,
) -> Vec<R>
where
    P: Fn(usize) -> U,
    C: Fn(&[U], std::ops::Range<usize>) -> Vec<R>,
{
    let start = range.start;
    let end = range.end;
    let element_bytes = std::mem::size_of::<U>().max(1);
    let block_items = (JET_LIST_ACCEL_CACHE_BLOCK_BYTES / element_bytes).max(1);
    let mut result = Vec::new();
    let mut block_start = start;
    while block_start < end {
        let block_end = block_start.saturating_add(block_items).min(end);
        let column = (block_start..block_end).map(&project).collect::<Vec<_>>();
        result.extend(copied_kernel(&column, block_start..block_end));
        block_start = block_end;
    }
    result
}

const JET_LIST_ACCEL_CACHE_BLOCK_BYTES: usize = 64 * 1024;

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

fn jet_list_sort_by<T, K: Ord, F>(xs: &mut [T], mut f: F)
where
    F: FnMut(&T) -> K,
{
    jet_list_try_sort_by_key_kernel(
        xs,
        |item| Ok::<_, std::convert::Infallible>(f(item)),
        Ord::cmp,
    )
    .unwrap_or_else(|never| match never {});
}

fn jet_list_sort_desc<T: Ord>(xs: &mut [T]) {
    xs.sort_by(|left, right| right.cmp(left));
}

trait JetListSurface {
    fn jet_len(&self) -> i64;
    fn jet_is_empty(&self) -> bool;
}

impl<T> JetListSurface for Vec<T> {
    #[inline(always)]
    fn jet_len(&self) -> i64 {
        self.len() as i64
    }

    #[inline(always)]
    fn jet_is_empty(&self) -> bool {
        self.is_empty()
    }
}

impl<T> JetListSurface for [T] {
    #[inline(always)]
    fn jet_len(&self) -> i64 {
        self.len() as i64
    }

    #[inline(always)]
    fn jet_is_empty(&self) -> bool {
        self.is_empty()
    }
}

impl<T, const N: usize> JetListSurface for [T; N] {
    #[inline(always)]
    fn jet_len(&self) -> i64 {
        N as i64
    }

    #[inline(always)]
    fn jet_is_empty(&self) -> bool {
        N == 0
    }
}


trait JetListPushSurface {
    type Item;

    fn jet_push(&mut self, value: Self::Item);
}

impl<T> JetListPushSurface for Vec<T> {
    type Item = T;

    #[inline(always)]
    fn jet_push(&mut self, value: Self::Item) {
        self.push(value);
    }
}
impl<T: Ord> JetListPushSurface for std::collections::BinaryHeap<T> {
    type Item = T;

    #[inline(always)]
    fn jet_push(&mut self, value: Self::Item) {
        self.push(value);
    }
}


// #4566 slice 8 inline storage: `jet_mem::JetInlineList` (the vetted memory
// prelude owns its unsafe storage); the list surfaces here are safe code.
impl<T, const N: usize> JetListSurface for jet_mem::JetInlineList<T, N> {
    #[inline(always)]
    fn jet_len(&self) -> i64 {
        <[T]>::len(self) as i64
    }

    #[inline(always)]
    fn jet_is_empty(&self) -> bool {
        <[T]>::is_empty(self)
    }
}

impl<T, const N: usize> JetListPushSurface for jet_mem::JetInlineList<T, N> {
    type Item = T;

    #[inline(always)]
    fn jet_push(&mut self, value: Self::Item) {
        self.push(value);
    }
}

#[inline(always)]
fn jet_list_push<C: JetListPushSurface>(xs: &mut C, value: C::Item) {
    xs.jet_push(value);
}


#[inline(always)]
fn jet_list_extend<T>(xs: &mut Vec<T>, other: Vec<T>) {
    xs.extend(other);
}

#[inline(always)]
fn jet_list_reverse<T>(xs: &mut [T]) {
    xs.reverse();
}

// Embedded Core-only port seam; dies at Rust cutover.
#[inline(always)]
fn jet_list_swap<T>(xs: &mut [T], first: i64, second: i64) {
    // Borrow-only indexing checks reuse the ordinary indexing stop exactly.
    let _ = jet_index_vec_ref(xs, first, "<core.prelude>", 0);
    let _ = jet_index_vec_ref(xs, second, "<core.prelude>", 0);
    xs.swap(first as usize, second as usize);
}

#[inline(always)]
fn jet_index_vec_ref<'a, T>(xs: &'a [T], i: i64, file: &str, line: u32) -> &'a T {
    jet_fixed_list_index(xs.len(), i, |index| &xs[index])
        .unwrap_or_else(|error| jet_arithmetic_stop(file, line, &error.message()))
}
#[inline(always)]
fn jet_list_sort<T: Ord>(xs: &mut [T]) {
    xs.sort();
}

// D-FLOATSORT1: a Float list has no `Ord`; its sorts use the one shared
// comparator (every NaN after every number, NaNs equal). Descending reverses
// that comparator, so NaNs lead. Float32 widens exactly to the same order.
#[inline(always)]
fn jet_list_sort_float(xs: &mut [f64]) {
    xs.sort_by(|left, right| jet_float_sort_cmp(*left, *right));
}

#[inline(always)]
fn jet_list_sort_desc_float(xs: &mut [f64]) {
    xs.sort_by(|left, right| jet_float_sort_cmp(*right, *left));
}

#[inline(always)]
fn jet_list_sort_float32(xs: &mut [f32]) {
    xs.sort_by(|left, right| jet_float_sort_cmp(f64::from(*left), f64::from(*right)));
}

#[inline(always)]
fn jet_list_sort_desc_float32(xs: &mut [f32]) {
    xs.sort_by(|left, right| jet_float_sort_cmp(f64::from(*right), f64::from(*left)));
}

trait JetCollectionClear {
    fn jet_clear(&mut self);
}

impl<T> JetCollectionClear for Vec<T> {
    #[inline(always)]
    fn jet_clear(&mut self) {
        self.clear();
    }
}

impl<K: Ord + Clone, V: Clone> JetCollectionClear for JetMap<K, V> {
    #[inline(always)]
    fn jet_clear(&mut self) {
        self.clear();
    }
}

#[inline(always)]
fn jet_list_clear<C: JetCollectionClear>(collection: &mut C) {
    collection.jet_clear();
}

fn jet_list_sort_by_desc<T, K: Ord, F>(xs: &mut [T], mut f: F)
where
    F: FnMut(&T) -> K,
{
    jet_list_try_sort_by_key_kernel(
        xs,
        |item| Ok::<_, std::convert::Infallible>(f(item)),
        |left, right| right.cmp(left),
    )
    .unwrap_or_else(|never| match never {});
}

fn jet_list_try_sort_by<T, K: Ord, E, F>(xs: &mut [T], f: F) -> Result<(), E>
where
    F: FnMut(&T) -> Result<K, E>,
{
    jet_list_try_sort_by_key_kernel(xs, f, Ord::cmp)
}

fn jet_list_try_sort_by_desc<T, K: Ord, E, F>(xs: &mut [T], f: F) -> Result<(), E>
where
    F: FnMut(&T) -> Result<K, E>,
{
    jet_list_try_sort_by_key_kernel(xs, f, |left, right| right.cmp(left))
}

#[inline(always)]
fn jet_list_len<C: JetListSurface + ?Sized>(xs: &C) -> i64 {
    xs.jet_len()
}

#[inline(always)]
fn jet_list_is_empty<C: JetListSurface + ?Sized>(xs: &C) -> bool {
    xs.jet_is_empty()
}

#[inline(always)]
pub(crate) fn jet_list_contains<T: PartialEq<U>, U: ?Sized>(xs: &[T], needle: &U) -> bool {
    xs.iter().any(|item| item == needle)
}
#[inline(always)]
fn jet_list_index_of<T: PartialEq<U>, U: ?Sized>(xs: &[T], needle: &U) -> JetOutcome<i64, JetAbsent> {
    jet_outcome_of(xs.iter().position(|item| item == needle).map(|index| index as i64))
}

#[inline(always)]
fn jet_list_get_opt<T: Clone>(xs: &[T], index: i64) -> JetOutcome<T, JetAbsent> {
    usize::try_from(index)
        .ok()
        .and_then(|index| xs.get(index).cloned())
        .ok_or(JetAbsent)
}

#[inline(always)]
fn jet_list_first<T: Clone>(xs: &[T]) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(xs.first().cloned())
}

#[inline(always)]
fn jet_list_last<T: Clone>(xs: &[T]) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(xs.last().cloned())
}

#[inline(always)]
fn jet_map_len<K: Ord, V>(map: &JetMap<K, V>) -> i64 {
    map.len() as i64
}

#[inline(always)]
fn jet_map_is_empty<K: Ord, V>(map: &JetMap<K, V>) -> bool {
    map.is_empty()
}

/// The key is any borrowed form of `K` (`K: Borrow<Q>`), so a `String`-keyed
/// map is read with a literal `&str` key without building a `String`.
#[inline(always)]
fn jet_map_get_opt<K: Ord + std::borrow::Borrow<Q>, V: Clone, Q: Ord + ?Sized>(
    map: &JetMap<K, V>,
    key: &Q,
) -> JetOutcome<V, JetAbsent> {
    jet_outcome_of(map.get(key).cloned())
}
#[inline(always)]
fn jet_map_has_key<K: Ord + std::borrow::Borrow<Q>, V, Q: Ord + ?Sized>(map: &JetMap<K, V>, key: &Q) -> bool {
    map.contains_key(key)
}

#[inline(always)]
fn jet_bag_has<T: Eq + std::hash::Hash + std::borrow::Borrow<Q>, Q: Eq + std::hash::Hash + ?Sized>(
    bag: &std::collections::HashMap<T, usize>,
    value: &Q,
) -> bool {
    bag.get(value).copied().unwrap_or(0) > 0
}

fn jet_bag_count<T: Eq + std::hash::Hash + std::borrow::Borrow<Q>, Q: Eq + std::hash::Hash + ?Sized>(
    bag: &std::collections::HashMap<T, usize>,
    value: &Q,
) -> i64 {
    bag.get(value).copied().unwrap_or(0) as i64
}

#[inline(always)]
fn jet_set_len<T>(set: &std::collections::HashSet<T>) -> i64 {
    set.len() as i64
}

#[inline(always)]
fn jet_set_capacity<T>(set: &std::collections::HashSet<T>) -> i64 {
    set.capacity() as i64
}

#[inline(always)]
fn jet_set_is_empty<T>(set: &std::collections::HashSet<T>) -> bool {
    set.is_empty()
}
#[inline(always)]
fn jet_set_has<T: Eq + std::hash::Hash + std::borrow::Borrow<Q>, Q: Eq + std::hash::Hash + ?Sized>(
    set: &std::collections::HashSet<T>,
    value: &Q,
) -> bool {
    set.contains(value)
}

#[inline(always)]
fn jet_set_to_list<T: Clone>(set: &std::collections::HashSet<T>) -> Vec<T> {
    set.iter().cloned().collect()
}
#[inline(always)]
fn jet_set_values<T: 'static + Clone>(
    set: &std::collections::HashSet<T>,
) -> JetIter<T> {
    jet_iter_from_vec(jet_set_to_list(set))
}


#[inline(always)]
fn jet_set_max<T: Ord + Clone>(
    set: &std::collections::HashSet<T>,
) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(set.iter().cloned().max())
}

#[inline(always)]
fn jet_set_sort<T: Ord + Clone>(set: &std::collections::HashSet<T>) -> Vec<T> {
    let mut values = jet_set_to_list(set);
    jet_list_sort(&mut values);
    values
}

#[inline(always)]
fn jet_set_shuffle<T: 'static + Clone>(set: &std::collections::HashSet<T>) -> Vec<T> {
    jet_iter_shuffle(jet_iter_from_vec(jet_set_to_list(set))).to_list()
}

#[inline(always)]
fn jet_set_equal<T: Eq + std::hash::Hash>(
    left: &std::collections::HashSet<T>,
    right: &std::collections::HashSet<T>,
) -> bool {
    left == right
}

#[inline(always)]
fn jet_set_first<T: Clone>(set: &std::collections::HashSet<T>) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(set.iter().next().cloned())
}

#[inline(always)]
fn jet_sorted_set_len<T>(set: &std::collections::BTreeSet<T>) -> i64 {
    set.len() as i64
}

#[inline(always)]
fn jet_sorted_set_is_empty<T>(set: &std::collections::BTreeSet<T>) -> bool {
    set.is_empty()
}

#[inline(always)]
fn jet_sorted_set_first<T: Ord + Clone>(
    set: &std::collections::BTreeSet<T>,
) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(set.first().cloned())
}

#[inline(always)]
fn jet_sorted_set_last<T: Ord + Clone>(
    set: &std::collections::BTreeSet<T>,
) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(set.last().cloned())
}

#[inline(always)]
fn jet_priority_queue_from<T: Ord>(values: Vec<T>) -> std::collections::BinaryHeap<T> {
    values.into_iter().collect()
}

fn jet_priority_queue_len<T>(queue: &std::collections::BinaryHeap<T>) -> i64 {
    queue.len() as i64
}

#[inline(always)]
fn jet_priority_queue_is_empty<T>(queue: &std::collections::BinaryHeap<T>) -> bool {
    queue.is_empty()
}
#[inline(always)]
fn jet_priority_queue_peek<T: Ord + Clone>(
    queue: &std::collections::BinaryHeap<T>,
) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(queue.peek().cloned())
}
#[inline(always)]
fn jet_priority_queue_to_sorted_list<T: Ord + Clone>(
    queue: &std::collections::BinaryHeap<T>,
) -> Vec<T> {
    let mut values = queue.clone().into_sorted_vec();
    values.reverse();
    values
}

#[inline(always)]
fn jet_lru_len<K: Eq + Clone, V: Clone>(cache: &JetCache<K, V>) -> i64 {
    cache.len() as i64
}

#[inline(always)]
fn jet_lru_is_empty<K: Eq + Clone, V: Clone>(cache: &JetCache<K, V>) -> bool {
    cache.is_empty()
}
#[inline(always)]
fn jet_lru_has<K: Eq + Clone + std::borrow::Borrow<Q>, V: Clone, Q: PartialEq + ?Sized>(cache: &JetCache<K, V>, key: &Q) -> bool {
    cache.contains_key(key)
}
#[inline(always)]
fn jet_lru_keys<K: Eq + Clone, V: Clone>(cache: &JetCache<K, V>) -> Vec<K> {
    cache.keys()
}

#[inline(always)]
fn jet_lru_put<K: Eq + Clone, V: Clone>(
    cache: &mut JetCache<K, V>,
    key: K,
    value: V,
) -> JetOutcome<V, JetAbsent> {
    jet_outcome_of(cache.put(key, value))
}

#[inline(always)]
fn jet_lru_get<K: Eq + Clone, V: Clone>(
    cache: &mut JetCache<K, V>,
    key: &K,
) -> JetOutcome<V, JetAbsent> {
    jet_outcome_of(cache.get(key))
}

#[inline(always)]
fn jet_lru_add_new<K: Eq + Clone, V: Clone>(
    cache: &mut JetCache<K, V>,
    key: K,
    value: V,
) -> bool {
    cache.add_new(key, value)
}

#[inline(always)]
fn jet_bag_len<T>(bag: &std::collections::HashMap<T, usize>) -> i64 {
    bag.values().sum::<usize>() as i64
}

#[inline(always)]
fn jet_bag_is_empty<T>(bag: &std::collections::HashMap<T, usize>) -> bool {
    bag.is_empty()
}

#[inline(always)]
fn jet_bit_set_len(bits: &JetBitSet) -> i64 {
    bits.len()
}

#[inline(always)]
fn jet_bit_set_is_empty(bits: &JetBitSet) -> bool {
    bits.is_empty()
}
#[inline(always)]
fn jet_bit_set_has(bits: &JetBitSet, bit: jet_foundation::Numeric::JetInt) -> bool {
    let bit = jet_collection_native_int(&bit);
    bits.contains(&bit)
}
#[inline(always)]
fn jet_bit_set_to_list(bits: &JetBitSet) -> Vec<i64> {
    bits.to_list()
}
#[inline(always)]
fn jet_bit_set_count(bits: &JetBitSet) -> i64 {
    bits.count()
}

#[inline(always)]
fn jet_bitset_add(bits: &mut JetBitSet, bit: jet_foundation::Numeric::JetInt) -> bool {
    let bit = jet_collection_native_int(&bit);
    bits.add(bit)
}

#[inline(always)]
fn jet_bitset_remove(bits: &mut JetBitSet, bit: jet_foundation::Numeric::JetInt) {
    let bit = jet_collection_native_int(&bit);
    bits.remove(&bit);
}
#[inline(always)]
fn jet_deque_len<T>(queue: &std::collections::VecDeque<T>) -> i64 {
    queue.len() as i64
}

#[inline(always)]
fn jet_deque_capacity<T>(queue: &std::collections::VecDeque<T>) -> i64 {
    queue.capacity() as i64
}
#[inline(always)]
fn jet_deque_peek_front<T: Clone>(
    queue: &std::collections::VecDeque<T>,
) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(queue.front().cloned())
}

#[inline(always)]
fn jet_deque_peek_back<T: Clone>(
    queue: &std::collections::VecDeque<T>,
) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(queue.back().cloned())
}

#[inline(always)]
fn jet_deque_contains<T: PartialEq<U>, U: ?Sized>(
    queue: &std::collections::VecDeque<T>,
    value: &U,
) -> bool {
    queue.iter().any(|item| item == value)
}

#[inline(always)]
fn jet_string_starts_with(text: &str, prefix: &str) -> bool {
    text.starts_with(prefix)
}

#[inline(always)]
fn jet_string_copy(text: &str) -> String {
    text.to_owned()
}

#[inline(always)]
fn jet_string_ends_with(text: &str, suffix: &str) -> bool {
    text.ends_with(suffix)
}
#[inline(always)]
fn jet_string_replace(text: &str, from: &str, to: &str) -> String {
    text.replace(from, to)
}

#[inline(always)]
fn jet_deque_get<T: Clone>(
    queue: &std::collections::VecDeque<T>,
    index: i64,
) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(usize::try_from(index).ok().and_then(|index| queue.get(index).cloned()))
}

#[inline(always)]
fn jet_deque_to_list<T: Clone>(queue: &std::collections::VecDeque<T>) -> Vec<T> {
    queue.iter().cloned().collect()
}

#[inline(always)]
fn jet_deque_join<T: JetShow>(
    queue: &std::collections::VecDeque<T>,
    separator: &str,
) -> String {
    jet_deque_join_by(queue, separator, JetShow::jet_show)
}

fn jet_deque_join_by<T, F: Fn(&T) -> String>(
    queue: &std::collections::VecDeque<T>,
    separator: &str,
    show: F,
) -> String {
    queue.iter().map(show).collect::<Vec<_>>().join(separator)
}

#[inline(always)]
fn jet_deque_from<T>(values: Vec<T>) -> std::collections::VecDeque<T> {
    values.into_iter().collect()
}

#[inline(always)]
fn jet_deque_push_front<T>(queue: &mut std::collections::VecDeque<T>, value: T) {
    queue.push_front(value);
}

#[inline(always)]
fn jet_deque_push_back<T>(queue: &mut std::collections::VecDeque<T>, value: T) {
    queue.push_back(value);
}

/// Remove the first element equal to `value`; a missing value is a no-op.
#[inline(always)]
fn jet_deque_delete<T: PartialEq>(queue: &mut std::collections::VecDeque<T>, value: T) {
    if let Some(index) = queue.iter().position(|candidate| *candidate == value) {
        queue.remove(index);
    }
}

#[inline(always)]
fn jet_deque_reverse<T>(queue: &mut std::collections::VecDeque<T>) {
    queue.make_contiguous().reverse();
}

/// Keep `[0, index)` in `queue` and return the tail; the owned Int index
/// clamps to the queue bounds.
#[inline(always)]
fn jet_deque_split<T>(
    queue: &mut std::collections::VecDeque<T>,
    index: jet_foundation::Numeric::JetInt,
) -> std::collections::VecDeque<T> {
    jet_deque_split_native(queue, jet_int_saturating_word(&index))
}

/// The machine-word body of `jet_deque_split`; hosts that carry `Int` as a
/// word call it directly.
#[inline(always)]
fn jet_deque_split_native<T>(
    queue: &mut std::collections::VecDeque<T>,
    index: i64,
) -> std::collections::VecDeque<T> {
    let at = usize::try_from(index).unwrap_or(0).min(queue.len());
    queue.split_off(at)
}

#[inline(always)]
fn jet_deque_is_empty<T>(queue: &std::collections::VecDeque<T>) -> bool {
    queue.is_empty()
}

#[inline(always)]
fn jet_string_is_empty(text: &str) -> bool {
    text.is_empty()
}

#[inline(always)]
pub(crate) fn jet_string_contains(text: &str, needle: &str) -> bool {
    text.contains(needle)
}
#[inline(always)]
fn jet_string_repeat(text: &str, count: i64) -> String {
    text.repeat(count.max(0) as usize)
}


#[inline(always)]
fn jet_string_count_bytes(text: &str) -> i64 {
    text.len() as i64
}

/// An owned Int size or position as a machine word. A value past the word
/// saturates, so it keeps its side of every clamp and bound check.
#[inline(always)]
fn jet_int_saturating_word(value: &jet_foundation::Numeric::JetInt) -> i64 {
    value.to_i64().unwrap_or_else(|| {
        if *value < jet_foundation::Numeric::JetInt::from_i64(0) {
            i64::MIN
        } else {
            i64::MAX
        }
    })
}

// D-ALLOCFAIL1=A: collection fallibility is one Prelude path. Native
// reservations stay here; map storage insertion is a representation hook.
// AOT, JIT, and TIR-eval marshal these functions.

/// `List.try_new<T>()`: the unit operand is the static `List` owner the
/// builtin route carries in its receiver slot.
fn jet_list_try_new<T>(_static_owner: ()) -> JetOutcome<Vec<T>, AllocError> {
    if jet_fault_should_fail_allocation() {
        return Err(jet_alloc_error(0, "List"));
    }
    Ok(Vec::new())
}

fn jet_list_try_with_capacity_defaulted<T>(
    capacity: i64,
    program_allocator_reserve: impl FnOnce(usize) -> bool,
    program_allocator_cancel: impl FnOnce(usize),
) -> JetOutcome<Vec<T>, AllocError> {
    if capacity < 0 {
        return Err(jet_alloc_error(0, "List"));
    }
    let capacity = usize::try_from(capacity).map_err(|_| jet_alloc_error(0, "List"))?;
    let requested = capacity.saturating_mul(std::mem::size_of::<T>().max(1));
    if jet_fault_should_fail_allocation() || !program_allocator_reserve(requested) {
        return Err(jet_alloc_error(requested, "List"));
    }
    let mut list = Vec::new();
    if list.try_reserve_exact(capacity).is_err() {
        program_allocator_cancel(requested);
        return Err(jet_alloc_error(requested, "List"));
    }
    Ok(list)
}

/// `List.try_with_capacity<T>(capacity)`: the unit operand is the static
/// `List` owner in the builtin route's receiver slot.
fn jet_list_try_with_capacity<T>(
    _static_owner: (),
    capacity: jet_foundation::Numeric::JetInt,
) -> JetOutcome<Vec<T>, AllocError> {
    jet_list_try_with_capacity_defaulted(jet_int_saturating_word(&capacity), |_| true, |_| {})
}

fn jet_list_try_push<T>(xs: &mut Vec<T>, value: T) -> JetOutcome<(), AllocError> {
    let requested = std::mem::size_of::<T>().max(1);
    if jet_fault_should_fail_allocation() {
        return Err(jet_alloc_error(requested, "List"));
    }
    xs.try_reserve(1)
        .map_err(|_| jet_alloc_error(requested, "List"))?;
    xs.push(value);
    Ok(())
}

fn jet_list_try_reserve<T>(
    xs: &mut Vec<T>,
    additional: jet_foundation::Numeric::JetInt,
) -> JetOutcome<(), AllocError> {
    jet_list_try_reserve_native(xs, jet_int_saturating_word(&additional))
}

/// The machine-word body of `jet_list_try_reserve`; hosts that carry `Int`
/// as a word call it directly.
fn jet_list_try_reserve_native<T>(
    xs: &mut Vec<T>,
    additional: i64,
) -> JetOutcome<(), AllocError> {
    if additional < 0 {
        return Err(jet_alloc_error(0, "List"));
    }
    let additional = usize::try_from(additional).map_err(|_| jet_alloc_error(0, "List"))?;
    let requested = additional.saturating_mul(std::mem::size_of::<T>().max(1));
    if jet_fault_should_fail_allocation() {
        return Err(jet_alloc_error(requested, "List"));
    }
    xs.try_reserve(additional)
        .map_err(|_| jet_alloc_error(requested, "List"))?;
    Ok(())
}

#[inline(always)]
fn jet_map_try_insert<M, K: Ord + Clone, V: Clone>(
    map: &mut M,
    key: K,
    value: V,
) -> JetOutcome<Option<V>, AllocError>
where
    M: std::ops::DerefMut<Target = std::collections::BTreeMap<K, V>>,
{
    if map.contains_key(&key) {
        return Ok(map.insert(key, value));
    }
    let requested = std::mem::size_of::<(K, V)>().max(1);
    if jet_fault_should_fail_allocation() {
        return Err(jet_alloc_error(requested, "Map"));
    }
    Ok(map.insert(key, value))
}

fn jet_string_try_push(text: &mut String, addition: &str) -> JetOutcome<(), AllocError> {
    let requested = addition.len();
    if jet_fault_should_fail_allocation() {
        return Err(jet_alloc_error(requested, "String"));
    }
    text.try_reserve(requested)
        .map_err(|_| jet_alloc_error(requested, "String"))?;
    text.push_str(addition);
    Ok(())
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
/// D-FLOATSORT1: Float extrema follow the collection sort order, so NaN is the
/// greatest item. Ties keep the first minimum and the last maximum, the same
/// choice the interpreter's list reducer makes.
fn jet_list_min_float<T: Copy + Into<f64>, I>(xs: I) -> JetOutcome<T, JetAbsent>
where
    I: IntoIterator<Item = T>,
{
    jet_outcome_of(xs.into_iter().reduce(|best, item| {
        if jet_collection_float_sort_cmp(best.into(), item.into()) == std::cmp::Ordering::Greater {
            item
        } else {
            best
        }
    }))
}
fn jet_list_max_float<T: Copy + Into<f64>, I>(xs: I) -> JetOutcome<T, JetAbsent>
where
    I: IntoIterator<Item = T>,
{
    jet_outcome_of(xs.into_iter().reduce(|best, item| {
        if jet_collection_float_sort_cmp(best.into(), item.into()) == std::cmp::Ordering::Greater {
            best
        } else {
            item
        }
    }))
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
    let storage = std::ops::DerefMut::deref_mut(&mut m);
    for x in xs {
        let k = f(&x);
        *storage.entry(k).or_insert(0) += 1;
    }
    m
}
fn jet_bag_any<K, F>(bag: &std::collections::HashMap<K, usize>, mut f: F) -> bool
where
    F: FnMut(&K) -> bool,
{
    bag.iter().any(|(key, count)| *count != 0 && f(key))
}

fn jet_option_map_ref<T, U, F>(value: &JetOutcome<T, JetAbsent>, f: F) -> JetOutcome<U, JetAbsent>
where
    F: FnOnce(&T) -> U,
{
    value.as_ref().map(f).map_err(|_| JetAbsent)
}


/// Count each item directly. `count_by(x -> x)` is the same operation with
/// the identity projection, but this named kernel keeps the common path terse.
fn jet_list_counts<T: Ord + Clone, I: IntoIterator<Item = T>>(xs: I) -> JetMap<T, i64> {
    let mut m: JetMap<T, i64> = JetMap::new();
    let storage = std::ops::DerefMut::deref_mut(&mut m);
    for item in xs {
        *storage.entry(item).or_insert(0) += 1;
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

fn jet_map_first_key_kernel<K: Ord + Clone, V>(m: &JetMap<K, V>) -> JetOutcome<K, JetAbsent> {
    jet_outcome_of(m.keys().next().cloned())
}

fn jet_map_entries_kernel<K: Ord + Clone, V: Clone>(m: &JetMap<K, V>) -> Vec<(K, V)> {
    m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// Return at most `n` map entries by descending value, with ascending keys as
/// the deterministic tie break. Negative limits select no entries.
fn jet_map_top_n<K: Ord + Clone, V: Ord + Clone>(m: &JetMap<K, V>, n: i64) -> Vec<(K, V)> {
    let limit = usize::try_from(n.max(0)).unwrap_or(usize::MAX).min(m.len());
    if limit == 0 {
        return Vec::new();
    }

    let rank = |(left_key, left_value): &(K, V), (right_key, right_value): &(K, V)| {
        right_value
            .cmp(left_value)
            .then_with(|| left_key.cmp(right_key))
    };
    let mut best = Vec::with_capacity(limit);
    for (key, value) in m.iter() {
        if best.len() < limit {
            best.push((key.clone(), value.clone()));
            if best.len() == limit {
                best.sort_by(&rank);
            }
            continue;
        }
        let worst = best.len() - 1;
        let better = value > &best[worst].1
            || (value == &best[worst].1 && key < &best[worst].0);
        if !better {
            continue;
        }
        best[worst] = (key.clone(), value.clone());
        let mut inserted = worst;
        while inserted > 0 && rank(&best[inserted], &best[inserted - 1]).is_lt() {
            best.swap(inserted, inserted - 1);
            inserted -= 1;
        }
    }
    best
}

fn jet_map_min_value_kernel<K: Ord, V: Ord + Clone>(m: &JetMap<K, V>) -> JetOutcome<V, JetAbsent> {
    jet_outcome_of(m.values().min().cloned())
}

fn jet_map_max_value_kernel<K: Ord, V: Ord + Clone>(m: &JetMap<K, V>) -> JetOutcome<V, JetAbsent> {
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

fn jet_map_from_keys_kernel<K: Ord + Clone, V: Clone>(keys: Vec<K>, default: V) -> JetMap<K, V> {
    keys.into_iter().map(|key| (key, default.clone())).collect()
}

fn jet_map_contains_value_kernel<K: Ord, V: PartialEq<U>, U: ?Sized>(m: &JetMap<K, V>, needle: &U) -> bool {
    m.values().any(|value| value == needle)
}

fn jet_map_pop_kernel<M, K: Ord + std::borrow::Borrow<Q>, V, Q: Ord + ?Sized>(m: &mut M, key: &Q) -> JetOutcome<V, JetAbsent>
where
    M: std::ops::DerefMut<Target = std::collections::BTreeMap<K, V>>,
{
    jet_outcome_of(m.remove(key))
}

fn jet_list_pop_kernel<T>(xs: &mut Vec<T>) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(xs.pop())
}

fn jet_list_remove_value_kernel<T: Clone + PartialEq>(xs: &mut Vec<T>, value: T) -> Option<T> {
    xs.iter()
        .position(|item| *item == value)
        .map(|index| xs.remove(index))
}

fn jet_list_remove_slot_kernel<T>(xs: &mut Vec<T>, index: i64) -> Result<T, String> {
    let len = xs.len() as i64;
    if index < 0 || index >= len {
        return Err(jet_list_bounds_message(len, index));
    }
    Ok(xs.remove(index as usize))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JetListInsertError {
    IndexOutOfBounds { len: usize, index: i64 },
}

impl JetListInsertError {
    pub(crate) fn code(self) -> &'static str {
        "E3010"
    }

    pub(crate) fn message(self) -> String {
        match self {
            Self::IndexOutOfBounds { len, index } => jet_list_bounds_message(len, index),
        }
    }
}

fn jet_list_insert_kernel<T>(
    xs: &mut Vec<T>,
    index: i64,
    value: T,
) -> Result<(), JetListInsertError> {
    let position = match usize::try_from(index) {
        Ok(position) if position <= xs.len() => position,
        _ => {
            return Err(JetListInsertError::IndexOutOfBounds {
                len: xs.len(),
                index,
            })
        }
    };
    xs.insert(position, value);
    Ok(())
}

fn jet_list_count_kernel<T: PartialEq<U>, U: ?Sized>(xs: &[T], value: &U) -> i64 {
    jet_count_to_int(xs.iter().filter(|item| *item == value).count())
}

/// A host `usize` count crosses into Jet's `Int` without truncation: a count
/// the one-word carrier cannot hold stops instead of wrapping negative.
fn jet_count_to_int(count: usize) -> i64 {
    i64::try_from(count).unwrap_or_else(|_| {
        jet_panic("<core.collections>", 0, "sequence count exceeds the Int carrier")
    })
}

/// A checked nonnegative Jet count selects at most `usize::MAX` host items,
/// so saturating keeps `take`/`skip`/`step_by` exact on every pointer width.
fn jet_sequence_count(n: i64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
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
        self.iter()
            .position(|item| item == value)
            .map(|index| self.remove(index))
    }
}

fn jet_set_pop_kernel<C: JetSetPopKernel>(
    set: &mut C,
    value: &C::Item,
) -> JetOutcome<C::Item, JetAbsent> {
    jet_outcome_of(set.pop_value(value))
}

trait JetSetReplaceKernel {
    type Item;

    fn replace_value(&mut self, value: Self::Item) -> Option<Self::Item>;
}

impl<T: Eq + std::hash::Hash> JetSetReplaceKernel for std::collections::HashSet<T> {
    type Item = T;

    fn replace_value(&mut self, value: Self::Item) -> Option<Self::Item> {
        self.replace(value)
    }
}

impl<T: PartialEq> JetSetReplaceKernel for Vec<T> {
    type Item = T;

    fn replace_value(&mut self, value: Self::Item) -> Option<Self::Item> {
        if let Some(index) = self.iter().position(|item| *item == value) {
            Some(std::mem::replace(&mut self[index], value))
        } else {
            self.push(value);
            None
        }
    }
}

fn jet_set_replace_kernel<C: JetSetReplaceKernel>(
    set: &mut C,
    value: C::Item,
) -> JetOutcome<C::Item, JetAbsent> {
    jet_outcome_of(set.replace_value(value))
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
        jet_count_to_int(self.0.count())
    }
    fn is_empty(mut self) -> bool {
        self.0.next().is_none()
    }
}

// Iterator terminals are free-function spellings so every target can route
// the checked BuiltinMethod without reaching into this private carrier.
fn jet_iter_to_list<T: 'static>(it: JetIter<T>) -> Vec<T> {
    it.to_list()
}

fn jet_iter_collect<T: 'static>(it: JetIter<T>) -> Vec<T> {
    it.collect()
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

fn jet_iter_first<T: 'static>(it: JetIter<T>) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(it.into_iter().next())
}

// D-ITER-RESUME1=A: the one partial pull. It advances the same boxed source
// through an exclusive receiver, so the caller keeps the exact remainder.
fn jet_iter_next<T: 'static>(it: &mut JetIter<T>) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(it.0.next())
}
fn jet_iter_len<T: 'static>(it: JetIter<T>) -> i64 {
    it.len()
}

fn jet_iter_is_empty<T: 'static>(it: JetIter<T>) -> bool {
    it.is_empty()
}


// D-TIER-ONEIR1=A: one target-neutral loop cursor protocol. The MIR adapter
// only marshals values into these calls; source-kind identity and iteration
// semantics stay here, beside the existing lazy iterator carrier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JetLoopSourceKind {
    Plain,
    Chars,
    LinesFile,
    LinesStdin,
    LinesProcessStream,
    ChannelReceiver,
    EncodingReader { reader_type: &'static str },
    Iterable {
        coll_type: &'static str,
        iter_type: &'static str,
        iter_symbol: &'static str,
        next_symbol: &'static str,
    },
}

impl JetLoopSourceKind {
    fn from_wire(wire: &'static str) -> Option<Self> {
        fn take(input: &'static str) -> Option<(&'static str, &'static str)> {
            let separator = input.find(':')?;
            let length = input.get(..separator)?.parse::<usize>().ok()?;
            let payload_start = separator.checked_add(1)?;
            let payload_end = payload_start.checked_add(length)?;
            let payload = input.get(payload_start..payload_end)?;
            let rest = input.get(payload_end..)?;
            Some((payload, rest))
        }

        match wire {
            "plain" => Some(Self::Plain),
            "chars" => Some(Self::Chars),
            "lines:file" => Some(Self::LinesFile),
            "lines:stdin" => Some(Self::LinesStdin),
            "lines:process" => Some(Self::LinesProcessStream),
            "channel" => Some(Self::ChannelReceiver),
            _ => {
                if let Some(payload) = wire.strip_prefix("encoding:") {
                    let (reader_type, rest) = take(payload)?;
                    if reader_type.is_empty() || !rest.is_empty() {
                        return None;
                    }
                    return Some(Self::EncodingReader { reader_type });
                }
                let payload = wire.strip_prefix("iterable:")?;
                let (coll_type, rest) = take(payload)?;
                let rest = rest.strip_prefix(':')?;
                let (iter_type, rest) = take(rest)?;
                let rest = rest.strip_prefix(':')?;
                let (iter_symbol, rest) = take(rest)?;
                let rest = rest.strip_prefix(':')?;
                let (next_symbol, rest) = take(rest)?;
                if coll_type.is_empty()
                    || iter_type.is_empty()
                    || iter_symbol.is_empty()
                    || next_symbol.is_empty()
                    || !rest.is_empty()
                {
                    return None;
                }
                Some(Self::Iterable {
                    coll_type,
                    iter_type,
                    iter_symbol,
                    next_symbol,
                })
            }
        }
    }
}

// Flattened into the generated crate root: `pub(super)` has no parent there.
#[derive(Clone, Debug)]
pub(crate) struct JetLoopRangeCursor {
    current: i64,
    end: i64,
    step: i64,
    exclusive: bool,
    done: bool,
}

#[inline]
pub(crate) fn jet_loop_range_init(
    start: i64,
    end: i64,
    step_value: i64,
    has_step: bool,
    exclusive: bool,
) -> JetLoopRangeCursor {
    jet_loop_range_init_checked(start, end, step_value, has_step, exclusive)
        .unwrap_or_else(|message| jet_panic("<core.prelude>", 0, message))
}

pub(crate) fn jet_loop_range_init_checked(
    start: i64,
    end: i64,
    step_value: i64,
    has_step: bool,
    exclusive: bool,
) -> Result<JetLoopRangeCursor, &'static str> {
    let step = if has_step { step_value } else { 1 };
    if step == 0 {
        return Err("range loop stride must not be zero");
    }
    Ok(JetLoopRangeCursor {
        current: start,
        end,
        step,
        exclusive,
        done: false,
    })
}

#[inline(always)]
pub(crate) fn jet_loop_range_has_next(cursor: &JetLoopRangeCursor) -> bool {
    !cursor.done && if cursor.step > 0 {
        if cursor.exclusive { cursor.current < cursor.end } else { cursor.current <= cursor.end }
    } else {
        if cursor.exclusive { cursor.current > cursor.end } else { cursor.current >= cursor.end }
    }
}

#[inline(always)]
pub(crate) fn jet_loop_range_value(cursor: &JetLoopRangeCursor) -> i64 {
    if !jet_loop_range_has_next(cursor) {
        jet_panic("<core.prelude>", 0, "range loop value requested after exhaustion");
    }
    cursor.current
}

#[inline(always)]
pub(crate) fn jet_loop_range_advance(cursor: &mut JetLoopRangeCursor) {
    if cursor.done {
        return;
    }
    cursor.current = match cursor.current.checked_add(cursor.step) {
        Some(value) => value,
        None => {
            cursor.done = true;
            return;
        }
    };
    cursor.done = !jet_loop_range_has_next(cursor);
}

/// #4566: the elements `jet_int_sum_walk` adds in one branch-free pass.
const JET_INT_SUM_CHUNK: usize = 256;
/// An element joins a chunk sum only as an inline word in
/// `[-2^52, 2^52)`, so a chunk sum stays inside `±2^60`.
const JET_INT_SUM_ELEMENT_BITS: u32 = 52;
/// A chunk is added only to a running total inside `[-2^61, 2^61)`, so the
/// new total stays inside the inline range `[-2^62, 2^62)`.
const JET_INT_SUM_TOTAL_BOUND: i64 = 1 << 61;
/// Fewer elements than this are left to the exact loop: on so short a walk
/// the chunk set-up costs more than the per-element checks it saves.
const JET_INT_SUM_MIN_ELEMENTS: usize = 32;

/// #4566: the native form of an exact `Int` range walk `loop i in a..<b ->
/// acc += xs[i]` (MIRRust `int_sum_walk`), run just before that loop. It
/// sums the elements left under `cursor` in chunks of `JET_INT_SUM_CHUNK`,
/// one pass per chunk that vectorises, and commits a chunk only when every
/// element and the running total are inline words inside the bounds above,
/// so each committed total is the exact sum. The cursor is left at the
/// first element not committed: the exact loop finishes the walk from there
/// (no iteration once all are committed) and reports an out-of-range index
/// itself.
#[inline(always)]
pub(crate) fn jet_int_sum_walk(
    cursor: &mut JetLoopRangeCursor,
    acc: &mut jet_foundation::Numeric::JetInt,
    xs: &[jet_foundation::Numeric::JetInt],
) {
    if cursor.done || cursor.step != 1 || !cursor.exclusive || cursor.current < 0 {
        return;
    }
    let start = cursor.current as usize;
    let end = usize::try_from(cursor.end).map_or(0, |end| end.min(xs.len()));
    if end <= start || end - start < JET_INT_SUM_MIN_ELEMENTS {
        return;
    }
    let mut total = acc.to_raw();
    let mut at = start;
    for chunk in xs[start..end].chunks(JET_INT_SUM_CHUNK) {
        if !(-JET_INT_SUM_TOTAL_BOUND..JET_INT_SUM_TOTAL_BOUND).contains(&total) {
            break;
        }
        let mut sum = 0_i64;
        let mut wide = 0_u64;
        for element in chunk {
            let raw = element.to_raw();
            wide |= (raw.wrapping_add(1 << JET_INT_SUM_ELEMENT_BITS) as u64) >> (JET_INT_SUM_ELEMENT_BITS + 1);
            sum = sum.wrapping_add(raw);
        }
        if wide != 0 {
            break;
        }
        total += sum;
        at += chunk.len();
    }
    if at == start {
        return;
    }
    // The replaced total was an inline word (checked above): no release.
    std::mem::forget(std::mem::replace(acc, jet_foundation::Numeric::JetInt::from_i64(total)));
    cursor.current = at as i64;
    cursor.done = !jet_loop_range_has_next(cursor);
}

/// #4566: the most a loop capacity hint reserves up front. Past it the list
/// grows by doubling as before, so a loop that exits early never holds more
/// than this much unused room.
const JET_LOOP_RESERVE_MAX_BYTES: usize = 1 << 20;

/// A capacity hint before a loop that pushes onto `xs` once per trip: room
/// for `trips` more elements, capped at `JET_LOOP_RESERVE_MAX_BYTES`. The
/// list's contents never change.
#[inline(always)]
fn jet_list_reserve_for_loop<T>(xs: &mut Vec<T>, trips: u64) {
    let cap = JET_LOOP_RESERVE_MAX_BYTES / std::mem::size_of::<T>().max(1);
    xs.reserve(usize::try_from(trips).unwrap_or(usize::MAX).min(cap));
}

/// The capacity hint for a range loop: its cursor's remaining trips.
#[inline(always)]
fn jet_list_reserve_for_range<T>(xs: &mut Vec<T>, cursor: &JetLoopRangeCursor) {
    jet_list_reserve_for_loop(xs, jet_loop_range_remaining(cursor));
}

/// How many values a fresh range cursor still yields.
fn jet_loop_range_remaining(cursor: &JetLoopRangeCursor) -> u64 {
    if !jet_loop_range_has_next(cursor) {
        return 0;
    }
    let span = (i128::from(cursor.end) - i128::from(cursor.current)).unsigned_abs();
    let step = i128::from(cursor.step).unsigned_abs();
    let trips = if cursor.exclusive { span.div_ceil(step) } else { span / step + 1 };
    u64::try_from(trips).unwrap_or(u64::MAX)
}

/// #4566 slice 7 capacity reuse (function-boundary research 4.3): a buffer
/// the emitter reuses only when dropping it releases memory and runs nothing
/// else (a String, or a list of scalars or Strings), so an emptied buffer
/// that keeps its capacity is indistinguishable from a fresh empty one.
trait JetReusableBuffer: Default {
    fn jet_reuse_clear(&mut self);
    fn jet_reuse_bytes(&self) -> usize;
}

impl<T> JetReusableBuffer for Vec<T> {
    #[inline(always)]
    fn jet_reuse_clear(&mut self) {
        self.clear();
    }

    #[inline(always)]
    fn jet_reuse_bytes(&self) -> usize {
        self.capacity().saturating_mul(std::mem::size_of::<T>())
    }
}

impl JetReusableBuffer for String {
    #[inline(always)]
    fn jet_reuse_clear(&mut self) {
        self.clear();
    }

    #[inline(always)]
    fn jet_reuse_bytes(&self) -> usize {
        self.capacity()
    }
}

/// A return-slot function's `out := [T]{}` / `out := ""`: the buffer its
/// caller left in the slot, emptied, else a fresh one. The value is an empty
/// list or String either way; only the capacity differs.
#[inline(always)]
fn jet_reuse_slot<B: JetReusableBuffer>(slot: &mut Option<B>) -> &mut B {
    let buffer = slot.get_or_insert_with(B::default);
    buffer.jet_reuse_clear();
    buffer
}

/// The drop of a call destination on a path back to the call that refills
/// it: the contents drop here, as `drop` would drop them, and the emptied
/// buffer stays in the slot for that call's `jet_reuse_slot`. A buffer past
/// `JET_LOOP_RESERVE_MAX_BYTES` is released, so a loop never keeps more
/// than that much idle capacity alive.
#[inline(always)]
fn jet_retire_slot<B: JetReusableBuffer>(slot: &mut Option<B>) {
    match slot {
        Some(buffer) if buffer.jet_reuse_bytes() <= JET_LOOP_RESERVE_MAX_BYTES => buffer.jet_reuse_clear(),
        _ => *slot = None,
    }
}

include!("LoopCursor.rs");
fn jet_loop_iter_init<C: JetLoopSource>(
    collection: C,
    step_value: i64,
    has_step: bool,
    by_value: bool,
    source_kind: JetLoopSourceKind,
) -> JetLoopIterCursor {
    jet_loop_iter_init_checked(collection, step_value, has_step, by_value, source_kind)
        .unwrap_or_else(|message| jet_panic("<core.prelude>", 0, message))
}

fn jet_loop_iter_init_checked<C: JetLoopSource>(
    collection: C,
    step_value: i64,
    has_step: bool,
    by_value: bool,
    source_kind: JetLoopSourceKind,
) -> Result<JetLoopIterCursor, &'static str> {
    let iter = collection.jet_loop_source(source_kind, by_value);
    jet_loop_iter_init_typed(iter, step_value, has_step)
}

/// Byte-slice fast path for `loop b in bytes`. MIR emits it for the
/// `ByteIterCursor` nominal, so a byte loop steps a slice iterator without
/// boxing each element.
struct JetByteLoopIterCursor<'a> {
    iter: std::slice::Iter<'a, u8>,
    current: Option<u8>,
    step: usize,
    exhausted: bool,
}

#[inline(always)]
fn jet_loop_iter_bytes_init<'a>(
    bytes: &'a [u8],
    step_value: i64,
    has_step: bool,
) -> JetByteLoopIterCursor<'a> {
    let step = jet_loop_iter_step(step_value, has_step)
        .unwrap_or_else(|message| jet_panic("<core.prelude>", 0, message));
    let mut iter = bytes.iter();
    let current = iter.next().copied();
    let exhausted = current.is_none();
    JetByteLoopIterCursor {
        iter,
        current,
        step,
        exhausted,
    }
}

#[inline(always)]
fn jet_loop_iter_bytes_has_next(cursor: &JetByteLoopIterCursor<'_>) -> bool {
    cursor.current.is_some()
}

#[inline(always)]
fn jet_loop_iter_bytes_value(cursor: &mut JetByteLoopIterCursor<'_>) -> u8 {
    cursor.current.take().unwrap_or_else(|| {
        jet_panic("<core.prelude>", 0, "iterator loop value requested after exhaustion")
    })
}

#[inline(always)]
fn jet_loop_iter_bytes_advance(cursor: &mut JetByteLoopIterCursor<'_>) {
    if cursor.exhausted {
        return;
    }
    for _ in 1..cursor.step {
        if cursor.iter.next().is_none() {
            cursor.current = None;
            cursor.exhausted = true;
            return;
        }
    }
    cursor.current = cursor.iter.next().copied();
    cursor.exhausted = cursor.current.is_none();
}

fn jet_loop_source_kind_name(kind: JetLoopSourceKind) -> String {
    match kind {
        JetLoopSourceKind::Plain => "Plain".to_string(),
        JetLoopSourceKind::Chars => "Chars".to_string(),
        JetLoopSourceKind::LinesFile => "LinesFile".to_string(),
        JetLoopSourceKind::LinesStdin => "LinesStdin".to_string(),
        JetLoopSourceKind::LinesProcessStream => "LinesProcessStream".to_string(),
        JetLoopSourceKind::ChannelReceiver => "ChannelReceiver".to_string(),
        JetLoopSourceKind::EncodingReader { reader_type } => {
            format!("EncodingReader({reader_type})")
        }
        JetLoopSourceKind::Iterable { coll_type, iter_type, .. } => {
            format!("Iterable({coll_type}::{iter_type})")
        }
    }
}

fn jet_loop_source_error(kind: JetLoopSourceKind) -> ! {
    jet_panic(
        "<core.prelude>",
        0,
        &format!(
            "loop source kind {} is not supported by this collection carrier",
            jet_loop_source_kind_name(kind)
        ),
    )
}

trait JetLoopSource {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>>;
}

impl<T: 'static> JetLoopSource for Vec<T> {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::Plain => {
                if !by_value {
                    jet_loop_source_error(source_kind);
                }
                Box::new(
                    self.into_iter()
                        .map(|value| Box::new(value) as JetLoopAny),
                )
            }
            JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}




impl<T: Clone + 'static> JetLoopSource for &mut Vec<T> {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::Plain => {
                if by_value {
                    jet_loop_source_error(source_kind);
                }
                let values = self.clone();
                Box::new(
                    values
                        .into_iter()
                        .map(|value| Box::new(value) as JetLoopAny),
                )
            }
            JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl<T: 'static, const N: usize> JetLoopSource for [T; N] {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::Plain => {
                if !by_value {
                    jet_loop_source_error(source_kind);
                }
                Box::new(
                    self.into_iter()
                        .map(|value| Box::new(value) as JetLoopAny),
                )
            }
            JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl<T: Clone + 'static, const N: usize> JetLoopSource for &mut [T; N] {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::Plain => {
                if by_value {
                    jet_loop_source_error(source_kind);
                }
                let values = self.to_vec();
                Box::new(
                    values
                        .into_iter()
                        .map(|value| Box::new(value) as JetLoopAny),
                )
            }
            JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}
impl<T: Clone + 'static> JetLoopSource for &mut &[T] {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::Plain => {
                if by_value {
                    jet_loop_source_error(source_kind);
                }
                let values = self.to_vec();
                Box::new(
                    values
                        .into_iter()
                        .map(|value| Box::new(value) as JetLoopAny),
                )
            }
            JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}


impl<K: Ord + Clone + 'static, V: Clone + 'static> JetLoopSource for JetMap<K, V> {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::Plain => {
                if !by_value {
                    jet_loop_source_error(source_kind);
                }
                // Each host owns the map representation; `jet_map_into_entries`
                // is its by-value take (AOT/JIT unwrap the shared handle).
                let values = jet_map_into_entries(self).into_iter();
                Box::new(values.map(|value| Box::new(value) as JetLoopAny))
            }
            JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl<K: Ord + Clone + 'static, V: Clone + 'static> JetLoopSource for &mut JetMap<K, V> {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::Plain => {
                if by_value {
                    jet_loop_source_error(source_kind);
                }
                let values = self
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect::<Vec<_>>()
                    .into_iter();
                Box::new(values.map(|value| Box::new(value) as JetLoopAny))
            }
            JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl<T: 'static> JetLoopSource for JetIter<T> {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::Plain
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => {
                if !by_value {
                    jet_loop_source_error(source_kind);
                }
                Box::new(
                    self.into_iter()
                        .map(|value| Box::new(value) as JetLoopAny),
                )
            }
            JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver => jet_loop_source_error(source_kind),
        }
    }
}


impl JetLoopSource for String {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::Plain | JetLoopSourceKind::Chars => {
                if !by_value {
                    jet_loop_source_error(source_kind);
                }
                Box::new(
                    JetStringChars::new(self)
                        .map(|value| Box::new(value) as JetLoopAny),
                )
            }
            JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl JetLoopSource for &mut String {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::Plain | JetLoopSourceKind::Chars => {
                if by_value {
                    jet_loop_source_error(source_kind);
                }
                let values = JetStringChars::new(self.clone());
                Box::new(
                    values
                        .map(|value| Box::new(value) as JetLoopAny),
                )
            }
            JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl JetLoopSource for JetRange {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::Plain => {
                if !by_value {
                    jet_loop_source_error(source_kind);
                }
                let end = if self.exclusive {
                    self.end
                } else {
                    self.end.saturating_add(1)
                };
                // A `Range` item is a Jet `Int`, so the cursor yields the
                // `JetInt` carrier that `jet_loop_iter_value::<JetInt>` reads.
                Box::new((self.start..end).map(|value| {
                    Box::new(jet_foundation::Numeric::JetInt::from_i64(value)) as JetLoopAny
                }))
            }
            JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl JetLoopSource for &mut JetRange {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::Plain => {
                if by_value {
                    jet_loop_source_error(source_kind);
                }
                let range = *self;
                range.jet_loop_source(source_kind, true)
            }
            JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}



/// Lazy `String.split` — yields owned pieces on pull (no intermediate Vec of parts).
/// Empty `sep` matches `jet_string_split` / Rust `str::split("")`: leading empty,
/// one Char string per scalar, trailing empty.
///
/// Keep one-scalar separators in their scalar form.  The old path converted
/// every separator to an owned `String` before building the iterator; that was
/// an avoidable allocation for the line-oriented `" "`/`"\n"` shapes that
/// dominate text pipelines.  A multi-scalar separator still owns its pattern
/// because `JetIter` is intentionally `'static`.
enum JetStringSplitSeparator {
    Empty,
    Scalar(char),
    Text(String),
}

/// Where a `split` walk stands in its source: the one piece sequence of
/// `text.split(sep)`, read as views of the source. `JetStringSplitIter` owns
/// its source; a loop over a stable source borrows it
/// (`JetStringSplitCursor`).
struct JetStringSplitState {
    separator: JetStringSplitSeparator,
    offset: usize,
    phase: u8,
    done: bool,
}

impl JetStringSplitState {
    fn new(sep: &str) -> Self {
        let mut chars = sep.chars();
        let separator = match (chars.next(), chars.next()) {
            (None, _) => JetStringSplitSeparator::Empty,
            (Some(ch), None) => JetStringSplitSeparator::Scalar(ch),
            (Some(_), Some(_)) => JetStringSplitSeparator::Text(sep.to_owned()),
        };
        JetStringSplitState {
            separator,
            offset: 0,
            phase: 0,
            done: false,
        }
    }

    /// The next piece of `source`: an empty separator yields a leading empty
    /// piece, one piece per scalar and a trailing empty piece.
    fn next_piece<'a>(&mut self, source: &'a str) -> Option<&'a str> {
        match &self.separator {
            JetStringSplitSeparator::Empty => match self.phase {
                0 => {
                    self.phase = 1;
                    Some(&source[..0])
                }
                1 => {
                    if self.offset >= source.len() {
                        self.phase = 2;
                        return Some(&source[..0]);
                    }
                    let ch = source[self.offset..]
                        .chars()
                        .next()
                        .expect("offset in bounds");
                    let start = self.offset;
                    self.offset += ch.len_utf8();
                    Some(&source[start..self.offset])
                }
                _ => None,
            },
            separator => {
                if self.done {
                    return None;
                }
                let start = self.offset;
                let found = match separator {
                    JetStringSplitSeparator::Scalar(ch) => source[start..]
                        .find(*ch)
                        .map(|relative| (relative, ch.len_utf8())),
                    JetStringSplitSeparator::Text(separator) => source[start..]
                        .find(separator.as_str())
                        .map(|relative| (relative, separator.len())),
                    JetStringSplitSeparator::Empty => unreachable!("empty separator handled above"),
                };
                match found {
                    Some((relative, separator_len)) => {
                        let end = start + relative;
                        self.offset = end + separator_len;
                        Some(&source[start..end])
                    }
                    None => {
                        self.done = true;
                        Some(&source[start..])
                    }
                }
            }
        }
    }
}

struct JetStringSplitIter {
    source: String,
    state: JetStringSplitState,
}

impl Iterator for JetStringSplitIter {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        self.state.next_piece(&self.source).map(str::to_owned)
    }
}

fn jet_iter_string_split(s: &str, sep: &str) -> JetIter<String> {
    JetIter(Box::new(JetStringSplitIter {
        source: s.to_owned(),
        state: JetStringSplitState::new(sep),
    }))
}

/// `loop piece in text.split(sep)` over a source that stays unchanged while
/// the loop runs (#4566 slice 6; MIRRust `split_loop_cursor`): the loop walks
/// the pieces of `jet_iter_string_split` as views of the borrowed source, so
/// the source is never copied and each piece the loop binds is allocated
/// once.
struct JetStringSplitCursor<'a> {
    source: &'a str,
    state: JetStringSplitState,
    current: Option<&'a str>,
}

fn jet_loop_split_init<'a>(source: &'a str, sep: &str) -> JetStringSplitCursor<'a> {
    let mut state = JetStringSplitState::new(sep);
    let current = state.next_piece(source);
    JetStringSplitCursor {
        source,
        state,
        current,
    }
}

#[inline(always)]
fn jet_loop_split_has_next(cursor: &JetStringSplitCursor<'_>) -> bool {
    cursor.current.is_some()
}

#[inline(always)]
fn jet_loop_split_value(cursor: &mut JetStringSplitCursor<'_>) -> String {
    cursor
        .current
        .take()
        .unwrap_or_else(|| {
            jet_panic("<core.prelude>", 0, "iterator loop value requested after exhaustion")
        })
        .to_owned()
}

#[inline(always)]
fn jet_loop_split_advance(cursor: &mut JetStringSplitCursor<'_>) {
    cursor.current = cursor.state.next_piece(cursor.source);
}

/// `text.split(sep).join(joiner)` when the split iterator feeds only the join
/// (#4566 slice 6; MIRRust `fused_split_join`). Same pieces as
/// `jet_iter_string_split` read as views of `s`: a non-empty separator yields
/// `s` with every match replaced by `joiner`; an empty one yields a leading
/// empty piece, one piece per scalar and a trailing empty piece. The result
/// is allocated once at its exact length.
fn jet_string_split_join(s: &str, sep: &str, joiner: &str) -> String {
    if sep.is_empty() {
        let scalars = s.chars().count();
        let mut out = String::with_capacity(s.len() + joiner.len() * (scalars + 1));
        out.push_str(joiner);
        for (start, ch) in s.char_indices() {
            out.push_str(&s[start..start + ch.len_utf8()]);
            out.push_str(joiner);
        }
        return out;
    }
    if let ([sep_byte], [join_byte]) = (sep.as_bytes(), joiner.as_bytes()) {
        // One-byte separator and joiner (both ASCII in UTF-8): every piece
        // keeps its place, so the result is the source with each separator
        // byte swapped, in one pass that vectorises. An ASCII byte never
        // occurs inside a multi-byte scalar, so the swap keeps UTF-8.
        let bytes: Vec<u8> = s.bytes().map(|byte| if byte == *sep_byte { *join_byte } else { byte }).collect();
        return String::from_utf8(bytes).expect("an ASCII byte swap keeps UTF-8");
    }
    if let [sep_byte] = sep.as_bytes() {
        // One-byte separator: the byte scan instead of the substring searcher.
        let sep_char = char::from(*sep_byte);
        let matches = s.bytes().filter(|byte| byte == sep_byte).count();
        let mut out = String::with_capacity(s.len() - matches + matches * joiner.len());
        for (index, piece) in s.split(sep_char).enumerate() {
            if index > 0 {
                out.push_str(joiner);
            }
            out.push_str(piece);
        }
        return out;
    }
    let matches = s.matches(sep).count();
    let mut out = String::with_capacity(s.len() - matches * sep.len() + matches * joiner.len());
    for (index, piece) in s.split(sep).enumerate() {
        if index > 0 {
            out.push_str(joiner);
        }
        out.push_str(piece);
    }
    out
}

/// Scan ASCII whitespace-delimited byte tokens without materialising each
/// token. The callback sees the source slice directly; the caller's
/// accumulator is proven dead after the scan.
#[inline(always)]
fn jet_bytes_ascii_whitespace_for_each<F>(
    bytes: &[u8],
    space: u8,
    ws_start: u8,
    ws_end: u8,
    mut f: F,
) where
    F: FnMut(&[u8], bool),
{
    let mut start = 0usize;
    for (index, &byte) in bytes.iter().enumerate() {
        if byte == space || (byte >= ws_start && byte <= ws_end) {
            if start < index {
                f(&bytes[start..index], false);
            }
            start = index + 1;
        }
    }
    if start < bytes.len() {
        f(&bytes[start..], true);
    }
}

/// Lazy `String.rsplit` — same left-to-right part order as Python `str.rsplit`
/// without a limit (Rust's `rsplit` yields right-to-left; reverse after collect).
fn jet_iter_string_rsplit(s: &str, sep: &str) -> JetIter<String> {
    if sep.is_empty() {
        return jet_iter_string_split(s, sep);
    }
    let mut parts: Vec<String> = s.rsplit(sep).map(|p| p.to_string()).collect();
    parts.reverse();
    jet_iter_from_vec(parts)
}

fn jet_iter_take<T: 'static>(it: JetIter<T>, n: i64) -> JetIter<T> {
    if let Some(message) = jet_sequence_argument_message("take", n) {
        jet_panic("<core.collections>", 0, message);
    }
    JetIter(Box::new(it.0.take(jet_sequence_count(n))))
}
fn jet_iter_skip<T: 'static>(it: JetIter<T>, n: i64) -> JetIter<T> {
    if let Some(message) = jet_sequence_argument_message("skip", n) {
        jet_panic("<core.collections>", 0, message);
    }
    JetIter(Box::new(it.0.skip(jet_sequence_count(n))))
}
fn jet_iter_step_by<T: 'static>(it: JetIter<T>, n: i64) -> JetIter<T> {
    if let Some(message) = jet_sequence_argument_message("step_by", n) {
        jet_panic("<core.collections>", 0, message);
    }
    JetIter(Box::new(it.0.step_by(jet_sequence_count(n))))
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
    if let Some(message) = jet_sequence_argument_message("chunks", n) {
        jet_panic("<core.collections>", 0, message);
    }
    JetIter(Box::new(JetChunksIter {
        inner: it.0,
        size: n as usize,
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
    if let Some(message) = jet_sequence_argument_message("windows", n) {
        jet_panic("<core.collections>", 0, message);
    }
    JetIter(Box::new(JetWindowsIter {
        inner: it.0,
        size: n as usize,
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
fn jet_iter_try_map<T: 'static, U: 'static, E, F>(it: JetIter<T>, mut f: F) -> Result<JetIter<U>, E>
where
    F: FnMut(&T) -> Result<U, E>,
{
    let values = jet_collection_try_map(it.0, |x| f(&x))?;
    Ok(JetIter(Box::new(values.into_iter())))
}
fn jet_iter_filter<T: 'static, F: 'static>(it: JetIter<T>, mut f: F) -> JetIter<T>
where
    F: FnMut(&T) -> bool,
{
    JetIter(Box::new(it.0.filter(move |x| f(x))))
}
fn jet_iter_try_filter<T: 'static, E, F>(it: JetIter<T>, f: F) -> Result<JetIter<T>, E>
where
    F: FnMut(&T) -> Result<bool, E>,
{
    let values = jet_collection_try_filter(it.0, f)?;
    Ok(JetIter(Box::new(values.into_iter())))
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
fn jet_iter_enumerate<T: 'static, U: 'static, F: 'static>(it: JetIter<T>, mut f: F) -> JetIter<U>
where
    F: FnMut(i64, &T) -> U,
{
    JetIter(Box::new(it.0.enumerate().map(move |(i, x)| f(i as i64, &x))))
}
/// D-RANGE-EXCL1=C: every valid Int index for a sequence of length `n`.
/// The length is the native word the lowering measured; each index is the
/// owned `Int` an `Iter<Int>` yields.
fn jet_iter_indexes(n: i64) -> JetIter<jet_foundation::Numeric::JetInt> {
    let n = n.max(0);
    JetIter(Box::new((0..n).map(jet_foundation::Numeric::JetInt::from_i64)))
}
fn jet_iter_zip<A: 'static, B: 'static, O: 'static, F: 'static>(
    mut a: JetIter<A>,
    mut b: JetIter<B>,
    mut f: F,
) -> JetIter<O>
where
    F: FnMut(A, B) -> O,
{
    JetIter(Box::new(std::iter::from_fn(move || {
        jet_zip_short_step(a.0.next(), || b.0.next()).map(|(x, y)| f(x, y))
    })))
}
fn jet_iter_empty<T: 'static>() -> JetIter<T> {
    JetIter(Box::new(std::iter::empty()))
}
// D-FAIL-CARRIER1=A: padding a short side yields carrier values, so a zip
// column reads as `?T` and nothing else.
fn jet_iter_some<T: 'static>(it: JetIter<T>) -> JetIter<JetOutcome<T, JetAbsent>> {
    JetIter(Box::new(it.0.map(Ok)))
}

// Strict zip carries its policy through the registered E0128 runtime row;
// preserve the user source context at the shared boundary.
fn jet_iter_zip_strict<
    A: 'static,
    B: 'static,
    O: 'static,
    F: 'static,
>(
    mut a: JetIter<A>,
    mut b: JetIter<B>,
    mut f: F,
    policy: String,
    file: String,
    line: u32,
    fn_name: String,
    source_line: String,
    col: u32,
    caret_len: u32,
) -> JetIter<O>
where
    F: FnMut(A, B) -> O,
{
    JetIter(Box::new(std::iter::from_fn(
        move || match jet_zip_strict_step(a.0.next(), b.0.next()) {
            Ok(Some((x, y))) => Some(f(x, y)),
            Ok(None) => None,
            Err(()) => {
                jet_panic_rich_code(
                    "E0128",
                    &file,
                    line,
                    &fn_name,
                    &source_line,
                    col,
                    caret_len,
                    &policy,
                    "",
                )
            }
        },
    )))
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
    JetIter(Box::new(std::iter::from_fn(
        move || match jet_zip_pad_step(a.0.next(), b.0.next(), fill_a.clone(), fill_b.clone()) {
            Some((x, y)) => Some(f(x, y)),
            None => None,
        },
    )))
}

// D-CORE-EAGER1=A / D-LOOPMAP1=B: concrete collection map/filter are eager.
// `.lazy()` enters the JetIter plane, where the same names remain deferred.
// The map kernels borrow their source (#4566 slice 5): each element is lent
// to the callback, so neither the list nor its elements are copied.
fn jet_list_map<T, U, F>(xs: &[T], f: F) -> Vec<U>
where
    F: Fn(&T) -> U,
{
    xs.iter().map(f).collect()
}
fn jet_list_map_mut<T, U, F>(xs: &[T], mut f: F) -> Vec<U>
where
    F: FnMut(&T) -> U,
{
    xs.iter().map(|x| f(x)).collect()
}
fn jet_list_try_map<T, U, E, F>(xs: Vec<T>, mut f: F) -> Result<Vec<U>, E>
where
    F: FnMut(&T) -> Result<U, E>,
{
    jet_collection_try_map(xs.iter(), |x| f(x))
}
fn jet_list_filter<T, F>(xs: Vec<T>, mut f: F) -> Vec<T>
where
    F: FnMut(&T) -> bool,
{
    xs.into_iter().filter(|x| f(x)).collect()
}
fn jet_list_try_filter<T, E, F>(xs: Vec<T>, f: F) -> Result<Vec<T>, E>
where
    F: FnMut(&T) -> Result<bool, E>,
{
    jet_collection_try_filter(xs, f)
}

fn jet_view_try_map<T, U, E, F>(xs: &[T], f: F) -> Result<Vec<U>, E>
where
    F: FnMut(&T) -> Result<U, E>,
{
    jet_collection_try_map(xs.iter(), f)
}

fn jet_view_try_filter<T: Clone, E, F>(xs: &[T], f: F) -> Result<Vec<T>, E>
where
    F: FnMut(&T) -> Result<bool, E>,
{
    jet_collection_try_filter(xs.iter().cloned(), f)
}
/// A fused eager adapter chain (#4566 slice 5; MIRRust
/// `fused_adapter_chain`): one pass over the borrowed source in which `step`
/// runs every stage on an element and yields its mapped value, or `None`
/// when a filter stage drops it. No intermediate list is built.
fn jet_list_map_chain<T, U, F>(xs: &[T], step: F) -> Vec<U>
where
    F: FnMut(&T) -> Option<U>,
{
    xs.iter().filter_map(step).collect()
}

// List-shaped helpers serve eager concrete-List adapters and materializing
// terminals; `.lazy()` and Iter receivers stay on the JetIter helpers above.
fn jet_list_take<T: Clone>(xs: Vec<T>, n: i64) -> Vec<T> {
    if let Some(message) = jet_sequence_argument_message("take", n) {
        jet_panic("<core.collections>", 0, message);
    }
    xs.into_iter().take(jet_sequence_count(n)).collect()
}
fn jet_list_skip<T: Clone>(xs: Vec<T>, n: i64) -> Vec<T> {
    if let Some(message) = jet_sequence_argument_message("skip", n) {
        jet_panic("<core.collections>", 0, message);
    }
    xs.into_iter().skip(jet_sequence_count(n)).collect()
}
fn jet_list_step_by<T: Clone>(xs: Vec<T>, n: i64) -> Vec<T> {
    if let Some(message) = jet_sequence_argument_message("step_by", n) {
        jet_panic("<core.collections>", 0, message);
    }
    xs.into_iter().step_by(jet_sequence_count(n)).collect()
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
fn jet_list_take_while_iter<T: 'static, F: 'static>(xs: Vec<T>, f: F) -> JetIter<T>
where
    F: FnMut(&T) -> bool,
{
    jet_iter_take_while(jet_iter_from_vec(xs), f)
}
fn jet_list_skip_while_iter<T: 'static, F: 'static>(xs: Vec<T>, f: F) -> JetIter<T>
where
    F: FnMut(&T) -> bool,
{
    jet_iter_skip_while(jet_iter_from_vec(xs), f)
}

fn jet_list_flat_map<T, U, F>(xs: Vec<T>, f: F) -> Vec<U>
where
    F: FnMut(&T) -> Vec<U>,
{
    xs.iter().flat_map(f).collect()
}
fn jet_list_filter_map_iter<T: 'static, U: 'static, E: 'static, F: 'static>(
    xs: Vec<T>,
    f: F,
) -> JetIter<U>
where
    F: FnMut(&T) -> Result<U, E>,
{
    jet_iter_filter_map(jet_iter_from_vec(xs), f)
}
fn jet_list_try_collect<T, E, I>(xs: I) -> Result<Vec<T>, E>
where
    I: IntoIterator<Item = Result<T, E>>,
{
    xs.into_iter().collect()
}
fn jet_list_each_ref<T, F, E>(xs: &Vec<T>, mut f: F) -> JetOutcome<(), E>
where
    F: FnMut(&T) -> JetOutcome<(), E>,
{
    for x in xs.iter() {
        f(x)?;
    }
    Ok(())
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
fn jet_list_scan_iter<T: 'static, U: Clone + 'static, F: 'static>(
    xs: Vec<T>,
    init: U,
    f: F,
) -> JetIter<U>
where
    F: FnMut(&U, &T) -> U,
{
    jet_iter_scan(jet_iter_from_vec(xs), init, f)
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
fn jet_list_extreme_by<T, K: Ord, F, I>(xs: I, mut f: F, maximum: bool) -> JetOutcome<T, JetAbsent>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> K,
{
    let mut best: Option<(K, T)> = None;
    for item in xs {
        let key = f(&item);
        let replace = match best.as_ref() {
            None => true,
            Some((best_key, _)) => {
                let order = key.cmp(best_key);
                if maximum {
                    order != std::cmp::Ordering::Less
                } else {
                    order != std::cmp::Ordering::Greater
                }
            }
        };
        if replace {
            best = Some((key, item));
        }
    }
    jet_outcome_of(best.map(|(_, item)| item))
}

fn jet_list_min_by<T, K: Ord, F, I>(xs: I, f: F) -> JetOutcome<T, JetAbsent>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> K,
{
    jet_list_extreme_by(xs, f, false)
}
fn jet_list_max_by<T, K: Ord, F, I>(xs: I, f: F) -> JetOutcome<T, JetAbsent>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> K,
{
    jet_list_extreme_by(xs, f, true)
}
fn jet_list_group_by<T: Clone, K: Ord + Clone, F, I>(xs: I, mut f: F) -> JetMap<K, Vec<T>>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> K,
{
    let mut m: JetMap<K, Vec<T>> = JetMap::new();
    let storage = std::ops::DerefMut::deref_mut(&mut m);
    for x in xs {
        let k = f(&x);
        storage.entry(k).or_default().push(x);
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
    let (yes, no) = match jet_list_try_partition_kernel(xs, |item| {
        Ok::<_, std::convert::Infallible>(f(item))
    }) {
        Ok(parts) => parts,
        Err(never) => match never {},
    };
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

/// `is_sorted_by(f)` reads any sequence: an eager List call passes its `Vec`
/// and a lazy pipeline its `JetIter`, with one meaning for both.
fn jet_iter_is_sorted_by<T, K: Ord, F>(items: impl IntoIterator<Item = T>, mut f: F) -> bool
where
    F: FnMut(&T) -> K,
{
    let xs = items.into_iter().collect::<Vec<T>>();
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

fn jet_iter_last_index_of<T: 'static + PartialEq>(
    it: JetIter<T>,
    needle: T,
) -> JetOutcome<i64, JetAbsent> {
    let xs = it.to_list();
    jet_outcome_of(xs.iter().rposition(|x| x == &needle).map(jet_count_to_int))
}

/// Integer `average()`: the element's own exact `+` builds the sum, which
/// converts to Float once before the division, so no host accumulator can
/// wrap. Every tier divides through `jet_average_of_sum`.
trait JetIntAverage: Sized {
    fn jet_sum_as_float(items: Vec<Self>) -> f64;
}

impl JetIntAverage for jet_foundation::Numeric::JetInt {
    fn jet_sum_as_float(items: Vec<Self>) -> f64 {
        jet_int_decimal_to_float(&items.into_iter().sum::<Self>().to_string_rep())
    }
}

macro_rules! jet_fixed_int_average {
    ($($ty:ty),*) => {$(
        impl JetIntAverage for $ty {
            fn jet_sum_as_float(items: Vec<Self>) -> f64 {
                items.into_iter().map(i128::from).sum::<i128>() as f64
            }
        }
    )*};
}
jet_fixed_int_average!(i8, i16, i32, i64, u8, u16, u32, u64);

/// Correctly rounded Float for an exact decimal Int; out-of-range sums
/// saturate to the signed infinity.
fn jet_int_decimal_to_float(text: &str) -> f64 {
    text.parse::<f64>().unwrap_or(if text.starts_with('-') {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    })
}

/// An empty source averages to `0.0`; otherwise the sum divides by the count.
fn jet_average_of_sum(sum: f64, count: usize) -> f64 {
    if count == 0 {
        0.0
    } else {
        sum / count as f64
    }
}

fn jet_iter_average_int<T: 'static + JetIntAverage>(it: JetIter<T>) -> f64 {
    let xs = it.to_list();
    let count = xs.len();
    jet_average_of_sum(T::jet_sum_as_float(xs), count)
}

/// `Iter.join(sep)` streams each shown item into one String without first
/// collecting the source into a list.
fn jet_iter_join<T: 'static + JetShow>(it: JetIter<T>, separator: &str) -> String {
    let mut out = String::new();
    for (index, item) in it.0.enumerate() {
        if index > 0 {
            out.push_str(separator);
        }
        item.jet_show_into(&mut out);
    }
    out
}

fn jet_iter_average_float(it: JetIter<f64>) -> f64 {
    let xs = it.to_list();
    let count = xs.len();
    jet_average_of_sum(xs.iter().sum::<f64>(), count)
}

/// Three-way lexicographic order of two sequences. Either side may be a
/// `Vec` or a `JetIter`; the lowering passes whichever carrier it holds.
fn jet_iter_compare<T: Ord>(
    items: impl IntoIterator<Item = T>,
    other: impl IntoIterator<Item = T>,
) -> i64 {
    match items.into_iter().cmp(other) {
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

fn jet_set_from<T: Eq + std::hash::Hash>(values: Vec<T>) -> std::collections::HashSet<T> {
    values.into_iter().collect()
}
fn jet_sorted_set_from<T: Ord>(values: Vec<T>) -> std::collections::BTreeSet<T> {
    values.into_iter().collect()
}
fn jet_sorted_set_to_list<T: Clone>(set: &std::collections::BTreeSet<T>) -> Vec<T> {
    set.iter().cloned().collect()
}



// #1477 List ledger surface
fn jet_list_slice<T: Clone>(xs: &[T], start: i64, end: i64) -> Vec<T> {
    let len = xs.len() as i64;
    let s = start.clamp(0, len) as usize;
    let e = end.clamp(0, len) as usize;
    if e <= s {
        Vec::new()
    } else {
        xs[s..e].to_vec()
    }
}
/// Sorted membership search. Returns the matching position as `?Int`; with
/// duplicates, the matching position is unspecified and is not a lower bound.
fn jet_list_binary_search<T: Ord + std::borrow::Borrow<Q>, Q: Ord + ?Sized>(xs: &[T], needle: &Q) -> JetOutcome<i64, JetAbsent> {
    jet_outcome_of(xs.binary_search_by(|item| item.borrow().cmp(needle)).ok().map(|i| i as i64))
}
/// Comparator form of sorted membership search. Returns `?Int`, not an
/// insertion point or a promise to select the first duplicate.
fn jet_list_binary_search_by<T, F>(xs: &[T], mut f: F) -> JetOutcome<i64, JetAbsent>
where
    F: FnMut(&T) -> std::cmp::Ordering,
{
    jet_outcome_of(xs.binary_search_by(|x| f(x)).ok().map(|i| i as i64))
}
fn jet_list_union<T: Clone + Eq>(left: &[T], right: &[T]) -> Vec<T> {
    let mut out = left.to_vec();
    for x in right {
        if !out.contains(x) {
            out.push(x.clone());
        }
    }
    out
}
fn jet_list_intersection<T: Clone + Eq>(left: &[T], right: &[T]) -> Vec<T> {
    let mut out = Vec::new();
    for x in left {
        if right.contains(x) && !out.contains(x) {
            out.push(x.clone());
        }
    }
    out
}
fn jet_list_difference<T: Clone + Eq>(left: &[T], right: &[T]) -> Vec<T> {
    left.iter()
        .filter(|x| !right.contains(x))
        .cloned()
        .collect()
}
fn jet_list_random<T: Clone>(xs: &[T]) -> JetOutcome<T, JetAbsent> {
    if xs.is_empty() {
        return Err(JetAbsent);
    }
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
pub(crate) fn jet_list_min_max<T: Ord + Clone, R>(
    xs: &[T],
    build: impl FnOnce(T, T) -> R,
) -> JetOutcome<R, JetAbsent> {
    match (xs.iter().min(), xs.iter().max()) {
        (Some(lo), Some(hi)) => Ok(build(lo.clone(), hi.clone())),
        _ => Err(JetAbsent),
    }
}
fn jet_collection_float_sort_cmp(left: f64, right: f64) -> std::cmp::Ordering {
    match left.partial_cmp(&right) {
        Some(ordering) => ordering,
        None if left.is_nan() && right.is_nan() => std::cmp::Ordering::Equal,
        None if left.is_nan() => std::cmp::Ordering::Greater,
        None => std::cmp::Ordering::Less,
    }
}

pub(crate) fn jet_list_min_max_float<R>(
    xs: &[f64],
    build: impl FnOnce(f64, f64) -> R,
) -> JetOutcome<R, JetAbsent> {
    let Some(&first) = xs.first() else {
        return Err(JetAbsent);
    };
    let (mut min, mut max) = (first, first);
    for &value in &xs[1..] {
        if jet_collection_float_sort_cmp(value, min) == std::cmp::Ordering::Less {
            min = value;
        }
        if jet_collection_float_sort_cmp(value, max) != std::cmp::Ordering::Less {
            max = value;
        }
    }
    Ok(build(min, max))
}

pub(crate) fn jet_list_min_max_by<T: Clone, K: Ord, F, R>(
    xs: &[T],
    mut f: F,
    build: impl FnOnce(T, T) -> R,
) -> JetOutcome<R, JetAbsent>
where
    F: FnMut(&T) -> K,
{
    let min = jet_list_min_by(xs.iter().cloned(), |x| f(x));
    let max = jet_list_max_by(xs.iter().cloned(), |x| f(x));
    match (min, max) {
        (Ok(lo), Ok(hi)) => Ok(build(lo, hi)),
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

/// Lexicographic list ordering. `None` preserves Rust's `PartialOrd` result
/// for a list containing an unordered element such as `Float::NaN`.
///
/// The numeric tags are the resident ABI used to rebuild `<`, `<=`, `>` and
/// `>=` without making either execution engine own this ordering law.
fn jet_list_order<T: PartialOrd>(left: &[T], right: &[T]) -> i8 {
    match left.partial_cmp(right) {
        Some(std::cmp::Ordering::Less) => 0,
        Some(std::cmp::Ordering::Equal) => 1,
        Some(std::cmp::Ordering::Greater) => 2,
        None => 3,
    }
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
// D-CORE-COLLECTIONS1=A: nominal string containers share one target-neutral
// kernel.  AOT calls these symbols directly; the comptime adapter below
// marshals the same values through this module rather than reimplementing
// Counter, OrderedMap, or Chain semantics.
#[derive(Clone, Debug)]
pub(crate) struct JetCounter {
    pub keys: Vec<String>,
    pub counts: Vec<i64>,
}

#[derive(Clone, Debug)]
pub(crate) struct JetOrderedMap {
    pub keys: Vec<String>,
    pub values: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct JetLayer {
    pub keys: Vec<String>,
    pub values: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct JetChain {
    pub layers: Vec<JetLayer>,
}
#[derive(Clone, Debug)]
pub(crate) struct JetStringSet {
    pub items: Vec<String>,
}

fn jet_coll_counter_text(counter: &JetCounter) -> String {
    format!(
        "Counter{{keys: {}, counts: {}}}",
        counter.keys.jet_show(),
        counter.counts.jet_show()
    )
}

fn jet_coll_ordered_map_text(map: &JetOrderedMap) -> String {
    format!(
        "OrderedMap{{keys: {}, values: {}}}",
        map.keys.jet_show(),
        map.values.jet_show()
    )
}

fn jet_coll_layer_text(layer: &JetLayer) -> String {
    format!(
        "Layer{{keys: {}, values: {}}}",
        layer.keys.jet_show(),
        layer.values.jet_show()
    )
}

fn jet_coll_chain_text(chain: &JetChain) -> String {
    let layers = chain
        .layers
        .iter()
        .map(jet_coll_layer_text)
        .collect::<Vec<_>>()
        .join(", ");
    format!("Chain{{layers: [{}]}}", layers)
}

impl JetShow for JetCounter {
    fn jet_show(&self) -> String {
        jet_coll_counter_text(self)
    }
}
impl JetDisplay for JetCounter {
    fn jet_display(&self) -> String {
        jet_coll_counter_text(self)
    }
}
impl JetDebug for JetCounter {
    fn jet_debug(&self) -> String {
        jet_coll_counter_text(self)
    }
}

impl JetShow for JetOrderedMap {
    fn jet_show(&self) -> String {
        jet_coll_ordered_map_text(self)
    }
}
impl JetDisplay for JetOrderedMap {
    fn jet_display(&self) -> String {
        jet_coll_ordered_map_text(self)
    }
}
impl JetDebug for JetOrderedMap {
    fn jet_debug(&self) -> String {
        jet_coll_ordered_map_text(self)
    }
}

impl JetShow for JetLayer {
    fn jet_show(&self) -> String {
        jet_coll_layer_text(self)
    }
}
impl JetDisplay for JetLayer {
    fn jet_display(&self) -> String {
        jet_coll_layer_text(self)
    }
}
impl JetDebug for JetLayer {
    fn jet_debug(&self) -> String {
        jet_coll_layer_text(self)
    }
}

impl JetShow for JetChain {
    fn jet_show(&self) -> String {
        jet_coll_chain_text(self)
    }
}
impl JetDisplay for JetChain {
    fn jet_display(&self) -> String {
        jet_coll_chain_text(self)
    }
}
impl JetShow for JetStringSet {
    fn jet_show(&self) -> String {
        self.items.jet_show()
    }
}
impl JetDisplay for JetStringSet {
    fn jet_display(&self) -> String {
        self.items.jet_show()
    }
}
impl JetDebug for JetStringSet {
    fn jet_debug(&self) -> String {
        self.items.jet_show()
    }
}
impl JetDebug for JetChain {
    fn jet_debug(&self) -> String {
        jet_coll_chain_text(self)
    }
}

pub(crate) fn jet_coll_counter() -> JetCounter {
    JetCounter {
        keys: Vec::new(),
        counts: Vec::new(),
    }
}

pub(crate) fn jet_coll_counter_from(keys: &[String]) -> JetCounter {
    let mut counter = jet_coll_counter();
    for key in keys {
        counter = jet_coll_counter_add(&counter, key, 1);
    }
    counter
}

pub(crate) fn jet_coll_counter_add(
    counter: &JetCounter,
    key: &str,
    amount: i64,
) -> JetCounter {
    let mut keys = Vec::with_capacity(counter.keys.len() + usize::from(amount != 0));
    let mut counts = Vec::with_capacity(counter.counts.len() + usize::from(amount != 0));
    let mut found = false;
    for (stored_key, stored_count) in counter.keys.iter().zip(&counter.counts) {
        if stored_key == key {
            let next = *stored_count + amount;
            if next != 0 {
                keys.push(stored_key.clone());
                counts.push(next);
            }
            found = true;
        } else {
            keys.push(stored_key.clone());
            counts.push(*stored_count);
        }
    }
    if !found && amount != 0 {
        keys.push(key.to_owned());
        counts.push(amount);
    }
    JetCounter { keys, counts }
}

pub(crate) fn jet_coll_counter_inc(counter: &JetCounter, key: &str) -> JetCounter {
    jet_coll_counter_add(counter, key, 1)
}

pub(crate) fn jet_coll_counter_dec(counter: &JetCounter, key: &str) -> JetCounter {
    jet_coll_counter_add(counter, key, -1)
}

pub(crate) fn jet_coll_counter_get(counter: &JetCounter, key: &str) -> i64 {
    counter
        .keys
        .iter()
        .zip(&counter.counts)
        .find_map(|(stored_key, count)| (stored_key == key).then_some(*count))
        .unwrap_or(0)
}

pub(crate) fn jet_coll_counter_set_count(
    counter: &JetCounter,
    key: &str,
    count: i64,
) -> JetCounter {
    let current = jet_coll_counter_get(counter, key);
    jet_coll_counter_add(counter, key, count - current)
}

pub(crate) fn jet_coll_counter_total(counter: &JetCounter) -> i64 {
    counter.counts.iter().sum()
}

pub(crate) fn jet_coll_counter_names(counter: &JetCounter) -> Vec<String> {
    counter.keys.clone()
}

pub(crate) fn jet_coll_counter_elements(counter: &JetCounter) -> Vec<String> {
    let mut out = Vec::new();
    for (key, count) in counter.keys.iter().zip(&counter.counts) {
        for _ in 0..(*count).max(0) {
            out.push(key.clone());
        }
    }
    out
}

pub(crate) fn jet_coll_counter_most_common(counter: &JetCounter, k: i64) -> JetCounter {
    let limit = if k < 0 || k as usize > counter.keys.len() {
        counter.keys.len()
    } else {
        k as usize
    };
    let mut used = vec![false; counter.keys.len()];
    let mut keys = Vec::with_capacity(limit);
    let mut counts = Vec::with_capacity(limit);
    for _ in 0..limit {
        let mut best = None;
        for (index, count) in counter.counts.iter().enumerate() {
            if !used[index]
                && best.is_none_or(|best_index| *count > counter.counts[best_index])
            {
                best = Some(index);
            }
        }
        let Some(index) = best else {
            break;
        };
        used[index] = true;
        keys.push(counter.keys[index].clone());
        counts.push(counter.counts[index]);
    }
    JetCounter { keys, counts }
}

pub(crate) fn jet_coll_counter_subtract(
    left: &JetCounter,
    right: &JetCounter,
) -> JetCounter {
    let mut out = left.clone();
    for (key, count) in right.keys.iter().zip(&right.counts) {
        out = jet_coll_counter_add(&out, key, -*count);
    }
    out
}

pub(crate) fn jet_coll_counter_merge_add(left: &JetCounter, right: &JetCounter) -> JetCounter {
    let mut out = left.clone();
    for (key, count) in right.keys.iter().zip(&right.counts) {
        out = jet_coll_counter_add(&out, key, *count);
    }
    out
}

pub(crate) fn jet_coll_counter_clear(_counter: &JetCounter) -> JetCounter {
    jet_coll_counter()
}

pub(crate) fn jet_coll_ordered_map() -> JetOrderedMap {
    JetOrderedMap {
        keys: Vec::new(),
        values: Vec::new(),
    }
}

pub(crate) fn jet_coll_map_get(
    map: &JetOrderedMap,
    key: &str,
) -> Option<String> {
    map.keys
        .iter()
        .position(|stored_key| stored_key == key)
        .map(|index| map.values[index].clone())
}

pub(crate) fn jet_coll_map_set(
    map: &JetOrderedMap,
    key: &str,
    value: &str,
) -> JetOrderedMap {
    let mut keys = Vec::with_capacity(map.keys.len() + 1);
    let mut values = Vec::with_capacity(map.values.len() + 1);
    let mut found = false;
    for (stored_key, stored_value) in map.keys.iter().zip(&map.values) {
        if stored_key == key {
            keys.push(key.to_owned());
            values.push(value.to_owned());
            found = true;
        } else {
            keys.push(stored_key.clone());
            values.push(stored_value.clone());
        }
    }
    if !found {
        keys.push(key.to_owned());
        values.push(value.to_owned());
    }
    JetOrderedMap { keys, values }
}

pub(crate) fn jet_coll_map_remove(map: &JetOrderedMap, key: &str) -> JetOrderedMap {
    let mut keys = Vec::with_capacity(map.keys.len());
    let mut values = Vec::with_capacity(map.values.len());
    for (stored_key, stored_value) in map.keys.iter().zip(&map.values) {
        if stored_key != key {
            keys.push(stored_key.clone());
            values.push(stored_value.clone());
        }
    }
    JetOrderedMap { keys, values }
}

pub(crate) fn jet_coll_map_keys(map: &JetOrderedMap) -> Vec<String> {
    map.keys.clone()
}

pub(crate) fn jet_coll_map_values(map: &JetOrderedMap) -> Vec<String> {
    map.values.clone()
}

pub(crate) fn jet_coll_map_contains(map: &JetOrderedMap, key: &str) -> bool {
    jet_coll_map_get(map, key).is_some()
}

pub(crate) fn jet_coll_map_len(map: &JetOrderedMap) -> i64 {
    map.keys.len() as i64
}

pub(crate) fn jet_coll_chain() -> JetChain {
    JetChain { layers: Vec::new() }
}

pub(crate) fn jet_coll_chain_push(
    chain: &JetChain,
    keys: &[String],
    values: &[String],
) -> JetChain {
    let mut layers = Vec::with_capacity(chain.layers.len() + 1);
    layers.push(JetLayer {
        keys: keys.to_vec(),
        values: values.to_vec(),
    });
    layers.extend(chain.layers.iter().cloned());
    JetChain { layers }
}

pub(crate) fn jet_coll_chain_get(
    chain: &JetChain,
    key: &str,
) -> Option<String> {
    for layer in &chain.layers {
        if let Some(index) = layer.keys.iter().position(|stored_key| stored_key == key) {
            return Some(layer.values[index].clone());
        }
    }
    None
}

pub(crate) fn jet_coll_chain_contains(chain: &JetChain, key: &str) -> bool {
    jet_coll_chain_get(chain, key).is_some()
}
pub(crate) fn jet_coll_set() -> JetStringSet {
    JetStringSet { items: Vec::new() }
}

pub(crate) fn jet_coll_set_from_list(items: &[String]) -> JetStringSet {
    let mut set = jet_coll_set();
    for item in items {
        set = jet_coll_set_add(&set, item);
    }
    set
}

pub(crate) fn jet_coll_set_add(set: &JetStringSet, item: &str) -> JetStringSet {
    if jet_coll_set_contains(set, item) {
        return set.clone();
    }
    let mut items = set.items.clone();
    items.push(item.to_owned());
    JetStringSet { items }
}

pub(crate) fn jet_coll_set_discard(set: &JetStringSet, item: &str) -> JetStringSet {
    JetStringSet {
        items: set.items.iter().filter(|value| *value != item).cloned().collect(),
    }
}

pub(crate) fn jet_coll_set_remove(
    set: &JetStringSet,
    item: &str,
) -> Option<JetStringSet> {
    if !jet_coll_set_contains(set, item) {
        return None;
    }
    Some(jet_coll_set_discard(set, item))
}

pub(crate) fn jet_coll_set_contains(set: &JetStringSet, item: &str) -> bool {
    set.items.iter().any(|value| value == item)
}
pub(crate) fn jet_coll_set_len(set: &JetStringSet) -> i64 {
    set.items.len() as i64
}

pub(crate) fn jet_coll_set_is_empty(set: &JetStringSet) -> bool {
    set.items.is_empty()
}


pub(crate) fn jet_coll_set_items(set: &JetStringSet) -> Vec<String> {
    set.items.clone()
}

pub(crate) fn jet_coll_set_clear(_set: &JetStringSet) -> JetStringSet {
    jet_coll_set()
}

pub(crate) fn jet_coll_set_union(left: &JetStringSet, right: &JetStringSet) -> JetStringSet {
    let mut out = left.clone();
    for item in &right.items {
        out = jet_coll_set_add(&out, item);
    }
    out
}

pub(crate) fn jet_coll_set_intersection(
    left: &JetStringSet,
    right: &JetStringSet,
) -> JetStringSet {
    JetStringSet {
        items: left
            .items
            .iter()
            .filter(|item| jet_coll_set_contains(right, item))
            .cloned()
            .collect(),
    }
}

pub(crate) fn jet_coll_set_difference(left: &JetStringSet, right: &JetStringSet) -> JetStringSet {
    JetStringSet {
        items: left
            .items
            .iter()
            .filter(|item| !jet_coll_set_contains(right, item))
            .cloned()
            .collect(),
    }
}

pub(crate) fn jet_coll_set_symmetric_difference(
    left: &JetStringSet,
    right: &JetStringSet,
) -> JetStringSet {
    jet_coll_set_union(
        &jet_coll_set_difference(left, right),
        &jet_coll_set_difference(right, left),
    )
}

pub(crate) fn jet_coll_set_issubset(left: &JetStringSet, right: &JetStringSet) -> bool {
    left.items.iter().all(|item| jet_coll_set_contains(right, item))
}

pub(crate) fn jet_coll_set_issuperset(left: &JetStringSet, right: &JetStringSet) -> bool {
    jet_coll_set_issubset(right, left)
}

pub(crate) fn jet_coll_set_isdisjoint(left: &JetStringSet, right: &JetStringSet) -> bool {
    left.items.iter().all(|item| !jet_coll_set_contains(right, item))
}

pub(crate) fn jet_coll_set_clone(set: &JetStringSet) -> JetStringSet {
    set.clone()
}
