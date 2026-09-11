// ── D-ITERTOOLS1=A: expanded collection/runtime handles ─────────────────────
impl<K: PartialEq + Clone, V: Clone> JetLru<K, V> {
    fn add_new(&mut self, key: K, value: V) -> bool {
        if self.bound == Some(0) || self.entries.iter().any(|(stored, _)| stored == &key) {
            return false;
        }
        self.put(key, value);
        true
    }

    fn remove(&mut self, key: &K) -> Option<V> {
        let index = self.entries.iter().position(|(stored, _)| stored == key)?;
        Some(self.entries.remove(index).1)
    }

    fn contains_key(&self, key: &K) -> bool {
        self.entries.iter().any(|(stored, _)| stored == key)
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

fn jet_bits_copy(bits: &JetBitSet) -> JetBitSet {
    bits.clone()
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
/// D-FRED1=A masked collection reductions share the same lane assignment and
/// adjacent tree as ordinary Float sums.  A malformed resident mask is
/// rejected instead of truncating the input.
#[inline(always)]
fn jet_list_masked_sum_fixed_f32(
    values: &[f32],
    mask: &[bool],
    seed: f32,
) -> Option<f32> {
    jet_simd_masked_reduce_slice(values, mask, seed)
}

#[inline(always)]
fn jet_list_masked_sum_fixed_f64(
    values: &[f64],
    mask: &[bool],
    seed: f64,
) -> Option<f64> {
    jet_simd_masked_reduce_slice(values, mask, seed)
}

#[inline(always)]
fn jet_list_first_match<T: JetSimdComparable>(
    values: &[T],
    needle: &T,
    op: JetSimdCompareOp,
) -> Option<usize> {
    jet_simd_first_match_slice(values, needle, op)
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

/// D-ACCEL1 column copy keeps one private cache-block projection at a time.
/// Rejected or single-pass paths call the original range operation unchanged.
#[inline(always)]
fn jet_list_accel_column_range_map<U, R, P, C, F>(
    range: std::ops::Range<usize>,
    nested_reuse: bool,
    single_pass: bool,
    proof_proven: bool,
    pin_active: bool,
    gate_selected: bool,
    project: P,
    copied_kernel: C,
    original_kernel: F,
) -> Vec<R>
where
    P: Fn(usize) -> U,
    C: Fn(&[U], std::ops::Range<usize>) -> Vec<R>,
    F: FnOnce(std::ops::Range<usize>) -> Vec<R>,
{
    let start = range.start;
    let end = range.end;
    if !gate_selected || pin_active || !proof_proven || !nested_reuse || single_pass {
        return original_kernel(start..end);
    }
    jet_list_accel_column_copy_range_map(start..end, project, copied_kernel)
}

const JET_LIST_ACCEL_CACHE_BLOCK_BYTES: usize = 64 * 1024;

/// Slice convenience wrapper over the range kernel; it does not materialize
/// source indices while constructing transient columns.
#[inline(always)]
fn jet_list_accel_column_map<T, U, R, P, C, F>(
    source: &[T],
    nested_reuse: bool,
    single_pass: bool,
    proof_proven: bool,
    pin_active: bool,
    gate_selected: bool,
    project: P,
    copied_kernel: C,
    original_kernel: F,
) -> Vec<R>
where
    P: Fn(&T) -> U,
    C: Fn(&[U], std::ops::Range<usize>) -> Vec<R>,
    F: FnOnce(&[T]) -> Vec<R>,
{
    jet_list_accel_column_range_map(
        0..source.len(),
        nested_reuse,
        single_pass,
        proof_proven,
        pin_active,
        gate_selected,
        |index| project(&source[index]),
        copied_kernel,
        |range| original_kernel(&source[range]),
    )
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

fn jet_list_sort_by<T, K: Ord, F>(xs: &mut Vec<T>, mut f: F)
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

fn jet_list_sort_desc<T: Ord>(xs: &mut Vec<T>) {
    xs.sort_by(|left, right| right.cmp(left));
}

#[inline(always)]
fn jet_list_push<T>(xs: &mut Vec<T>, value: T) {
    xs.push(value);
}

#[inline(always)]
fn jet_list_extend<T>(xs: &mut Vec<T>, other: Vec<T>) {
    xs.extend(other);
}

#[inline(always)]
fn jet_list_reverse<T>(xs: &mut Vec<T>) {
    xs.reverse();
}

#[inline(always)]
fn jet_list_sort<T: Ord>(xs: &mut Vec<T>) {
    xs.sort();
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

fn jet_list_sort_by_desc<T, K: Ord, F>(xs: &mut Vec<T>, mut f: F)
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

fn jet_list_try_sort_by<T, K: Ord, E, F>(xs: &mut Vec<T>, f: F) -> Result<(), E>
where
    F: FnMut(&T) -> Result<K, E>,
{
    jet_list_try_sort_by_key_kernel(xs, f, Ord::cmp)
}

fn jet_list_try_sort_by_desc<T, K: Ord, E, F>(xs: &mut Vec<T>, f: F) -> Result<(), E>
where
    F: FnMut(&T) -> Result<K, E>,
{
    jet_list_try_sort_by_key_kernel(xs, f, |left, right| right.cmp(left))
}

#[inline(always)]
fn jet_list_len<T>(xs: &[T]) -> i64 {
    xs.len() as i64
}

#[inline(always)]
fn jet_list_is_empty<T>(xs: &[T]) -> bool {
    xs.is_empty()
}

#[inline(always)]
pub(crate) fn jet_list_contains<T: PartialEq>(xs: &[T], needle: &T) -> bool {
    xs.contains(needle)
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

#[inline(always)]
fn jet_map_get_opt<K: Ord, V: Clone>(
    map: &JetMap<K, V>,
    key: &K,
) -> JetOutcome<V, JetAbsent> {
    jet_outcome_of(map.get(key).cloned())
}

#[inline(always)]
fn jet_set_len<T>(set: &std::collections::HashSet<T>) -> i64 {
    set.len() as i64
}

#[inline(always)]
fn jet_set_is_empty<T>(set: &std::collections::HashSet<T>) -> bool {
    set.is_empty()
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
fn jet_priority_queue_len<T>(queue: &std::collections::BinaryHeap<T>) -> i64 {
    queue.len() as i64
}

#[inline(always)]
fn jet_priority_queue_is_empty<T>(queue: &std::collections::BinaryHeap<T>) -> bool {
    queue.is_empty()
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
fn jet_deque_len<T>(queue: &std::collections::VecDeque<T>) -> i64 {
    queue.len() as i64
}

#[inline(always)]
fn jet_deque_is_empty<T>(queue: &std::collections::VecDeque<T>) -> bool {
    queue.is_empty()
}

#[inline(always)]
fn jet_string_is_empty(text: &String) -> bool {
    text.is_empty()
}

#[inline(always)]
pub(crate) fn jet_string_contains(text: &str, needle: &str) -> bool {
    text.contains(needle)
}

#[inline(always)]
fn jet_string_count_bytes(text: &String) -> i64 {
    text.len() as i64
}

// D-ALLOCFAIL1=A: collection fallibility is one Prelude path. Native
// reservations stay here; map storage insertion is a representation hook.
// AOT, JIT, and TIR-eval marshal these functions.
fn jet_list_try_new<T>() -> JetOutcome<Vec<T>, AllocError> {
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

fn jet_list_try_with_capacity<T>(capacity: i64) -> JetOutcome<Vec<T>, AllocError> {
    jet_list_try_with_capacity_defaulted(capacity, |_| true, |_| {})
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

fn jet_list_try_reserve<T>(xs: &mut Vec<T>, additional: i64) -> JetOutcome<(), AllocError> {
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
fn jet_bag_any<K, F>(bag: &JetMap<K, usize>, mut f: F) -> bool
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

fn jet_map_contains_value_kernel<K: Ord, V: PartialEq>(m: &JetMap<K, V>, needle: &V) -> bool {
    m.values().any(|value| value == needle)
}

fn jet_map_pop_kernel<M, K: Ord + Clone, V: Clone>(m: &mut M, key: &K) -> JetOutcome<V, JetAbsent>
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

fn jet_list_remove_slot_kernel<T: Clone>(xs: &mut Vec<T>, index: i64) -> Result<T, String> {
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

fn jet_list_count_kernel<T: PartialEq>(xs: &[T], value: &T) -> i64 {
    xs.iter().filter(|item| *item == value).count() as i64
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
fn jet_iter_len<T: 'static>(it: JetIter<T>) -> i64 {
    it.len()
}

fn jet_iter_is_empty<T: 'static>(it: JetIter<T>) -> bool {
    it.is_empty()
}

fn jet_iter_last<T: 'static>(it: JetIter<T>) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(it.into_iter().last())
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

pub(crate) fn jet_loop_range_has_next(cursor: &JetLoopRangeCursor) -> bool {
    !cursor.done && if cursor.step > 0 {
        if cursor.exclusive { cursor.current < cursor.end } else { cursor.current <= cursor.end }
    } else {
        if cursor.exclusive { cursor.current > cursor.end } else { cursor.current >= cursor.end }
    }
}

pub(crate) fn jet_loop_range_value(cursor: &JetLoopRangeCursor) -> i64 {
    if !jet_loop_range_has_next(cursor) {
        jet_panic("<core.prelude>", 0, "range loop value requested after exhaustion");
    }
    cursor.current
}

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

type JetLoopAny = Box<dyn std::any::Any>;

struct JetLoopIterCursor {
    iter: Box<dyn Iterator<Item = JetLoopAny>>,
    current: Option<JetLoopAny>,
    step: usize,
    exhausted: bool,
}
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
    let step = if has_step { step_value } else { 1 };
    if step <= 0 {
        return Err("iterator loop stride must be positive");
    }
    let mut iter = collection.jet_loop_source(source_kind, by_value);
    let current = iter.next();
    let exhausted = current.is_none();
    Ok(JetLoopIterCursor {
        iter,
        current,
        step: step as usize,
        exhausted,
    })
}

fn jet_loop_iter_has_next(cursor: &JetLoopIterCursor) -> bool {
    cursor.current.is_some()
}

fn jet_loop_iter_value<T: 'static>(cursor: &mut JetLoopIterCursor) -> T {
    let Some(value) = cursor.current.take() else {
        jet_panic("<core.prelude>", 0, "iterator loop value requested after exhaustion");
    };
    match value.downcast::<T>() {
        Ok(value) => *value,
        Err(_) => jet_panic("<core.prelude>", 0, "iterator loop item type does not match MIR"),
    }
}

fn jet_loop_iter_advance(cursor: &mut JetLoopIterCursor) {
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
    cursor.current = cursor.iter.next();
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

struct JetStringChars {
    bytes: Vec<u8>,
    offset: usize,
}

impl JetStringChars {
    fn new(value: String) -> Self {
        Self {
            bytes: value.into_bytes(),
            offset: 0,
        }
    }
}

impl Iterator for JetStringChars {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        let end = self.offset.saturating_add(4).min(self.bytes.len());
        let bytes = self.bytes.get(self.offset..end)?;
        // The window can end inside the following scalar.
        let tail = match std::str::from_utf8(bytes) {
            Ok(tail) => tail,
            Err(error) => std::str::from_utf8(&bytes[..error.valid_up_to()]).ok()?,
        };
        let value = tail.chars().next()?;
        self.offset += value.len_utf8();
        Some(value)
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
                Box::new(
                    (self.start..end)
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
fn jet_iter_string_split(s: &String, sep: &str) -> JetIter<String> {
    let s = s.clone();
    let sep = sep.to_string();
    if sep.is_empty() {
        // Index into owned `s` — `s.chars()` would borrow and break `'static` JetIter.
        let mut offset = 0usize;
        // 0 = leading empty; 1 = chars; 2 = done after trailing empty.
        let mut phase = 0u8;
        return JetIter(Box::new(std::iter::from_fn(move || match phase {
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

/// Run a synchronous consumer over `String.split` pieces. This is the direct
/// Prelude seam for AOT loops whose body does not escape the loop callback:
/// it keeps the source string borrowed, avoids a boxed `JetIter`, and preserves
/// the exact `str::split` piece order and empty-separator behavior.
#[inline(always)]
fn jet_string_split_for_each<F>(s: &String, sep: &str, mut f: F)
where
    F: FnMut(String),
{
    for part in s.split(sep) {
        f(part.to_string());
    }
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
fn jet_iter_string_rsplit(s: &String, sep: &str) -> JetIter<String> {
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
    JetIter(Box::new(it.0.take(n as usize)))
}
fn jet_iter_skip<T: 'static>(it: JetIter<T>, n: i64) -> JetIter<T> {
    if let Some(message) = jet_sequence_argument_message("skip", n) {
        jet_panic("<core.collections>", 0, message);
    }
    JetIter(Box::new(it.0.skip(n as usize)))
}
fn jet_iter_step_by<T: 'static>(it: JetIter<T>, n: i64) -> JetIter<T> {
    if let Some(message) = jet_sequence_argument_message("step_by", n) {
        jet_panic("<core.collections>", 0, message);
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
    F: FnMut(i64, T) -> U,
{
    JetIter(Box::new(it.0.enumerate().map(move |(i, x)| f(i as i64, x))))
}
/// D-RANGE-EXCL1=C: every valid Int index for a sequence of length `n`.
fn jet_iter_indexes(n: i64) -> JetIter<i64> {
    let n = n.max(0);
    JetIter(Box::new((0..n).map(|i| i)))
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
fn jet_iter_zip_strict<A: 'static, B: 'static, O: 'static, F: 'static>(
    mut a: JetIter<A>,
    mut b: JetIter<B>,
    mut f: F,
) -> JetIter<O>
where
    F: FnMut(A, B) -> O,
{
    JetIter(Box::new(std::iter::from_fn(
        move || match jet_zip_strict_step(a.0.next(), b.0.next()) {
            Ok(Some((x, y))) => Some(f(x, y)),
            Ok(None) => None,
            Err(()) => jet_panic("<core.collections>", 0, jet_zip_length_mismatch_message()),
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
/// Adjacent eager adapters may be fused when the intermediate list is not
/// observable. The callbacks still run in source order, once per element.
fn jet_list_map_filter<T, U, F, P>(xs: Vec<T>, mut map: F, mut keep: P) -> Vec<U>
where
    F: FnMut(&T) -> U,
    P: FnMut(&U) -> bool,
{
    xs.iter()
        .map(|x| map(x))
        .filter(|value| keep(value))
        .collect()
}

// List-shaped helpers serve eager concrete-List adapters and materializing
// terminals; `.lazy()` and Iter receivers stay on the JetIter helpers above.
fn jet_list_take<T: Clone>(xs: Vec<T>, n: i64) -> Vec<T> {
    if let Some(message) = jet_sequence_argument_message("take", n) {
        jet_panic("<core.collections>", 0, message);
    }
    xs.into_iter().take(n as usize).collect()
}
fn jet_list_skip<T: Clone>(xs: Vec<T>, n: i64) -> Vec<T> {
    if let Some(message) = jet_sequence_argument_message("skip", n) {
        jet_panic("<core.collections>", 0, message);
    }
    xs.into_iter().skip(n as usize).collect()
}
fn jet_list_step_by<T: Clone>(xs: Vec<T>, n: i64) -> Vec<T> {
    if let Some(message) = jet_sequence_argument_message("step_by", n) {
        jet_panic("<core.collections>", 0, message);
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
    if let Some(message) = jet_sequence_argument_message("chunks", n) {
        jet_panic("<core.collections>", 0, message);
    }
    xs.chunks(n as usize).map(|c| c.to_vec()).collect()
}
fn jet_list_windows<T: Clone>(xs: Vec<T>, n: i64) -> Vec<Vec<T>> {
    if let Some(message) = jet_sequence_argument_message("windows", n) {
        jet_panic("<core.collections>", 0, message);
    }
    let n = n as usize;
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

fn jet_iter_last_index_of<T: 'static + PartialEq>(
    it: JetIter<T>,
    needle: T,
) -> JetOutcome<i64, JetAbsent> {
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
    if e <= s {
        Vec::new()
    } else {
        xs[s..e].to_vec()
    }
}
/// Sorted membership search. Returns the matching position as `?Int`; with
/// duplicates, the matching position is unspecified and is not a lower bound.
fn jet_list_binary_search<T: Ord>(xs: &[T], needle: &T) -> JetOutcome<i64, JetAbsent> {
    jet_outcome_of(xs.binary_search(needle).ok().map(|i| i as i64))
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
