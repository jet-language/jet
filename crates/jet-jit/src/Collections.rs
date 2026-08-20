//! M5: list/map host shims for the Cranelift JIT (`JetArena` handles).

use super::Concurrency;
use crate::runtime_host::{jit_callable_parts, JitCallableSlot};
use std::collections::{BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};

mod set_semantics {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/Core/SetAlgebra.rs");
}

mod range_semantics {
    use jet_foundation::StructuralDebug::jet_debug_range;
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/Core/RangeBounds.rs");
}

pub(crate) mod byte_buffer_semantics {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/Core/Bytes.rs");
}

mod disjoint_semantics {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/Core/Disjoint.rs");

    pub(super) fn split(
        len: usize,
        mid: i64,
    ) -> Result<((usize, usize), (usize, usize)), String> {
        jet_disjoint_split_bounds(len, mid)
    }

    pub(super) fn indexes(
        len: usize,
        indices: &[i64],
    ) -> Result<Vec<(usize, usize, usize)>, String> {
        jet_disjoint_index_bounds(len, indices)
    }
}

// The resident collection ABI owns only handle conversion.  The operation
// itself must stay in the same Prelude source embedded by AOT and evaluated by
// tier 0 (I9).  This small module supplies the Prelude's sibling types that
// are normally present in the flat emitted program, then exposes Vec-shaped
// adapters for JIT handles.
#[allow(dead_code, non_camel_case_types, unused_imports)]
mod collection_semantics {
    pub use jet_foundation::Outcome::*;
    use crate::fault_injection::jet_fault_should_fail_allocation;

    use jet_foundation::StructuralDebug::jet_debug_map;
    use jet_foundation::StructuralDebug::jet_debug_optional;
    use jet_foundation::StructuralDebug::jet_debug_range;

    // These are emitted beside the Prelude in an AOT program.  Keep the same
    // bridge here so Values.rs remains the only scalar/string/list formatter.
    trait __jet_Display {
        fn display(&self) -> String;
    }
    trait __jet_Equatable: Sized {
        fn equal(&self, rhs: &Self) -> bool;
    }

    #[derive(Clone)]
    struct JetByteBuffer {
        bytes: Vec<u8>,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct JetMap<K, V>(std::sync::Arc<std::collections::BTreeMap<K, V>>);

    impl<K, V> JetMap<K, V> {
        fn new() -> Self {
            Self(std::sync::Arc::new(std::collections::BTreeMap::new()))
        }
    }

    impl<K: Ord, V> std::iter::FromIterator<(K, V)> for JetMap<K, V> {
        fn from_iter<I: IntoIterator<Item = (K, V)>>(pairs: I) -> Self {
            Self(std::sync::Arc::new(pairs.into_iter().collect()))
        }
    }

    impl<K, V> std::ops::Deref for JetMap<K, V> {
        type Target = std::collections::BTreeMap<K, V>;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl<K: Ord + Clone, V: Clone> std::ops::DerefMut for JetMap<K, V> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            std::sync::Arc::make_mut(&mut self.0)
        }
    }

    // BTreeMap has no stable fallible reservation API. Keep this representation
    // step at the map seam; the shared Prelude owns the AllocError projection.
    fn jet_map_try_insert_storage<K: Ord + Clone, V: Clone>(
        map: &mut JetMap<K, V>,
        key: K,
        value: V,
    ) -> Result<Option<V>, ()> {
        Ok(map.insert(key, value))
    }

    fn jet_panic(_file: &str, line: u32, msg: &str) -> ! {
        crate::runtime_host::runtime_stop_unwind("E3001", line, msg)
    }

    include!("../../jet-codegen/src/Prelude/Core/Loadable.rs");
    include!("../../jet-codegen/src/Prelude/Core/Values.rs");
    include!("../../jet-codegen/src/Prelude/Core/RangeBounds.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/Iter.rs");
    include!("../../jet-codegen/src/Prelude/Memo.rs");
    include!("../../jet-codegen/src/Prelude/Core/Collections.rs");

    // Core.rs owns the public any/all/each spellings. These adapters compose
    // the same eager Prelude kernels available in this shared collection seam;
    // they do not inspect handles or define a second traversal policy.
    pub(super) fn list_closure_any<F>(xs: Vec<i64>, mut f: F) -> bool
    where
        F: FnMut(&i64) -> bool,
    {
        jet_list_position(xs, |value| f(value)).is_ok()
    }

    pub(super) fn list_closure_all<F>(xs: Vec<i64>, mut f: F) -> bool
    where
        F: FnMut(&i64) -> bool,
    {
        jet_list_position(xs, |value| !f(value)).is_err()
    }

    pub(super) fn list_closure_map<F>(xs: Vec<i64>, f: F) -> Vec<i64>
    where
        F: Fn(&i64) -> i64,
    {
        jet_list_map(xs, f)
    }

    pub(super) fn list_closure_map_mut<F>(xs: Vec<i64>, f: F) -> Vec<i64>
    where
        F: FnMut(&i64) -> i64,
    {
        jet_list_map_mut(xs, f)
    }

    pub(super) fn list_closure_map_f64<F>(xs: Vec<i64>, f: F) -> Vec<f64>
    where
        F: Fn(&i64) -> f64,
    {
        jet_list_map(xs, f)
    }

    pub(super) fn list_closure_map_f64_mut<F>(xs: Vec<i64>, f: F) -> Vec<f64>
    where
        F: FnMut(&i64) -> f64,
    {
        jet_list_map_mut(xs, f)
    }

    pub(super) fn list_closure_filter<F>(xs: Vec<i64>, f: F) -> Vec<i64>
    where
        F: FnMut(&i64) -> bool,
    {
        jet_list_filter(xs, f)
    }

    pub(super) fn list_closure_each<F>(xs: Vec<i64>, f: F)
    where
        F: Fn(&i64),
    {
        let _ = jet_list_fold(xs, (), |_, value| {
            f(value);
        });
    }

    pub(super) fn list_closure_each_mut<F>(xs: Vec<i64>, mut f: F)
    where
        F: FnMut(&i64),
    {
        let _ = jet_list_fold(xs, (), |_, value| {
            f(value);
        });
    }

    pub(super) fn zip_fill_at<T: Clone>(
        fill_mode: u8,
        common_fills: &[T],
        column_fills: &[T],
        default: T,
        column: usize,
    ) -> T {
        jet_zip_fill_at(fill_mode, common_fills, column_fills, default, column)
    }

    pub(super) fn zip_rows<T: Clone, Read, Fill>(
        lengths: &[usize],
        mode: u8,
        read: Read,
        fill: Fill,
    ) -> Option<Vec<Vec<T>>>
    where
        Read: FnMut(usize, usize) -> Option<T>,
        Fill: FnMut(usize) -> T,
    {
        jet_zip_rows(lengths, mode, read, fill)
    }

    pub(super) fn zip_length_mismatch_message() -> &'static str {
        jet_zip_length_mismatch_message()
    }

    pub(super) fn try_list_new<T>() -> JetOutcome<Vec<T>, AllocError> {
        jet_list_try_new()
    }

    pub(super) fn try_list_with_capacity<T>(
        capacity: i64,
        allocator: &jet_codegen::program_allocator::JetProgramAllocator,
    ) -> JetOutcome<Vec<T>, AllocError> {
        jet_list_try_with_capacity_defaulted(
            capacity,
            |requested| allocator.try_reserve_hosted(requested),
            |requested| allocator.cancel_hosted_reservation(requested),
        )
    }

    pub(super) fn try_list_push<T>(values: &mut Vec<T>, value: T) -> JetOutcome<(), AllocError> {
        jet_list_try_push(values, value)
    }

    pub(super) fn try_list_reserve<T>(
        values: &mut Vec<T>,
        additional: i64,
    ) -> JetOutcome<(), AllocError> {
        jet_list_try_reserve(values, additional)
    }

    pub(super) fn try_map_insert_i64(
        pairs: Vec<(String, i64)>,
        key: String,
        value: i64,
    ) -> (Vec<(String, i64)>, JetOutcome<Option<i64>, AllocError>) {
        let mut map = JetMap::from_iter(pairs);
        let result = jet_map_try_insert(&mut map, key, value);
        let pairs = map.iter().map(|(key, value)| (key.clone(), *value)).collect();
        (pairs, result)
    }

    pub(super) fn try_string_push(
        text: &mut String,
        addition: &str,
    ) -> JetOutcome<(), AllocError> {
        jet_string_try_push(text, addition)
    }

    fn show<T: JetShow>(value: &T) -> String {
        value.jet_show()
    }

    fn debug<T: JetDebug>(value: &T) -> String {
        value.jet_debug()
    }

    pub(super) fn debug_i64(value: i64) -> String { debug(&value) }
    pub(super) fn debug_f64(value: f64) -> String { debug(&value) }
    pub(super) fn debug_f32(value: f32) -> String { debug(&value) }
    pub(super) fn debug_bool(value: bool) -> String { debug(&value) }
    pub(super) fn debug_char(value: char) -> String { debug(&value) }
    pub(super) fn debug_string(value: String) -> String { debug(&value) }

    pub(super) fn show_i64_list(value: Vec<i64>) -> String { show(&value) }
    pub(super) fn debug_i64_list(value: Vec<i64>) -> String { debug(&value) }
    pub(super) fn show_u64_list(value: Vec<u64>) -> String { show(&value) }
    pub(super) fn debug_u64_list(value: Vec<u64>) -> String { debug(&value) }
    pub(super) fn show_f64_list(value: Vec<f64>) -> String { show(&value) }
    pub(super) fn debug_f64_list(value: Vec<f64>) -> String { debug(&value) }
    pub(super) fn show_f32_list(value: Vec<f32>) -> String { show(&value) }
    pub(super) fn debug_f32_list(value: Vec<f32>) -> String { debug(&value) }
    pub(super) fn show_bool_list(value: Vec<bool>) -> String { show(&value) }
    pub(super) fn debug_bool_list(value: Vec<bool>) -> String { debug(&value) }
    pub(super) fn show_char_list(value: Vec<char>) -> String { show(&value) }
    pub(super) fn debug_char_list(value: Vec<char>) -> String { debug(&value) }
    pub(super) fn show_string_list(value: Vec<String>) -> String { show(&value) }
    pub(super) fn debug_string_list(value: Vec<String>) -> String { debug(&value) }

    pub(super) fn iter_take<T: 'static>(xs: Vec<T>, n: i64) -> Vec<T> {
        jet_iter_take(jet_iter_from_vec(xs), n).to_list()
    }

    pub(super) fn iter_skip<T: 'static>(xs: Vec<T>, n: i64) -> Vec<T> {
        jet_iter_skip(jet_iter_from_vec(xs), n).to_list()
    }

    pub(super) fn iter_first<T: 'static>(xs: Vec<T>) -> Option<T> {
        jet_iter_first(jet_iter_from_vec(xs)).ok()
    }

    pub(super) fn iter_step_by<T: 'static>(xs: Vec<T>, n: i64) -> Vec<T> {
        jet_iter_step_by(jet_iter_from_vec(xs), n).to_list()
    }

    pub(super) fn iter_dedup<T: 'static + Clone + PartialEq>(xs: Vec<T>) -> Vec<T> {
        jet_iter_dedup(jet_iter_from_vec(xs)).to_list()
    }

    pub(super) fn iter_chunks<T: 'static>(xs: Vec<T>, n: i64) -> Vec<Vec<T>> {
        jet_iter_chunks(jet_iter_from_vec(xs), n).to_list()
    }

    pub(super) fn iter_windows<T: 'static + Clone>(xs: Vec<T>, n: i64) -> Vec<Vec<T>> {
        jet_iter_windows(jet_iter_from_vec(xs), n).to_list()
    }

    pub(super) fn list_sum_i64(xs: Vec<i64>) -> i64 {
        jet_list_sum(xs)
    }

    pub(super) fn list_product_i64(xs: Vec<i64>) -> i64 {
        jet_list_product(xs)
    }

    pub(super) fn list_copy_i64(xs: &[i64]) -> Vec<i64> {
        jet_list_copy(xs)
    }

    pub(super) fn list_min_i64(xs: Vec<i64>) -> JetOutcome<i64, JetAbsent> {
        jet_list_min(xs)
    }

    pub(super) fn list_max_i64(xs: Vec<i64>) -> JetOutcome<i64, JetAbsent> {
        jet_list_max(xs)
    }

    pub(super) fn list_flatten_i64(xs: Vec<Vec<i64>>) -> Vec<i64> {
        jet_list_flatten(xs)
    }

    pub(super) fn list_intersperse_i64(xs: Vec<i64>, separator: i64) -> Vec<i64> {
        jet_list_intersperse(xs, separator)
    }

    pub(super) fn iter_repeat<T: 'static + Clone>(xs: Vec<T>, n: i64) -> Vec<T> {
        jet_iter_repeat(jet_iter_from_vec(xs), n).to_list()
    }

    pub(super) fn iter_cycle<T: 'static + Clone>(xs: Vec<T>, n: i64) -> Vec<T> {
        jet_iter_cycle(jet_iter_from_vec(xs), n).to_list()
    }

    pub(super) fn iter_drop_last<T: 'static>(xs: Vec<T>, n: i64) -> Vec<T> {
        jet_iter_drop_last(jet_iter_from_vec(xs), n).to_list()
    }

    pub(super) fn iter_shuffle<T: 'static>(xs: Vec<T>) -> Vec<T> {
        jet_iter_shuffle(jet_iter_from_vec(xs)).to_list()
    }

    pub(super) fn iter_is_sorted<T: 'static + Ord>(xs: Vec<T>) -> bool {
        jet_iter_is_sorted(jet_iter_from_vec(xs))
    }

    pub(super) fn iter_last_index_of<T: 'static + PartialEq>(
        xs: Vec<T>,
        needle: T,
    ) -> JetOutcome<i64, JetAbsent> {
        jet_iter_last_index_of(jet_iter_from_vec(xs), needle)
    }

    pub(super) fn iter_average_int(xs: Vec<i64>) -> f64 {
        jet_iter_average_int(jet_iter_from_vec(xs))
    }

    pub(super) fn iter_average_float(xs: Vec<f64>) -> f64 {
        jet_iter_average_float(jet_iter_from_vec(xs))
    }

    pub(super) fn iter_compare<T: 'static + Ord>(xs: Vec<T>, other: Vec<T>) -> i64 {
        jet_iter_compare(jet_iter_from_vec(xs), other)
    }

    pub(super) fn iter_split_i64(xs: Vec<i64>, n: i64) -> (Vec<i64>, Vec<i64>) {
        jet_iter_split_at(jet_iter_from_vec(xs), n, |left, right| (left, right))
    }

    fn map_from_pairs(entries: Vec<(String, i64)>) -> JetMap<String, i64> {
        entries.into_iter().collect()
    }

    fn map_pairs(map: &JetMap<String, i64>) -> Vec<(String, i64)> {
        jet_map_entries_kernel(map)
    }

    pub(super) fn map_copy_i64(entries: Vec<(String, i64)>) -> Vec<(String, i64)> {
        map_pairs(&jet_map_copy_kernel(&map_from_pairs(entries)))
    }

    pub(super) fn map_equal_i64(
        left: Vec<(String, i64)>,
        right: Vec<(String, i64)>,
    ) -> bool {
        jet_map_equal_kernel(&map_from_pairs(left), &map_from_pairs(right))
    }

    pub(super) fn bit_set_copy(
        values: &std::collections::BTreeSet<i64>,
    ) -> std::collections::BTreeSet<i64> {
        jet_bits_copy(&JetBitSet { bits: values.clone() }).bits
    }

    /// `Bits.has` — the same membership kernel `JetBitSet::contains` calls in
    /// AOT, read straight off the resident set (no copy on a lookup).
    pub(super) fn bit_set_has(
        values: &std::collections::BTreeSet<i64>,
        bit: i64,
    ) -> bool {
        jet_bits_has_kernel(values, bit)
    }

    pub(super) fn map_first_key_i64(entries: Vec<(String, i64)>) -> Option<String> {
        jet_map_first_key_kernel(&map_from_pairs(entries)).ok()
    }

    pub(super) fn map_entries_i64(entries: Vec<(String, i64)>) -> Vec<(String, i64)> {
        map_pairs(&map_from_pairs(entries))
    }

    pub(super) fn map_min_i64(entries: Vec<(String, i64)>) -> Option<i64> {
        jet_map_min_value_kernel(&map_from_pairs(entries)).ok()
    }

    pub(super) fn map_max_i64(entries: Vec<(String, i64)>) -> Option<i64> {
        jet_map_max_value_kernel(&map_from_pairs(entries)).ok()
    }

    pub(super) fn map_intersection_i64(
        left: Vec<(String, i64)>,
        right: Vec<(String, i64)>,
    ) -> Vec<(String, i64)> {
        map_pairs(&jet_map_intersection_kernel(
            &map_from_pairs(left),
            &map_from_pairs(right),
        ))
    }

    pub(super) fn map_slice_i64(
        entries: Vec<(String, i64)>,
        keys: Vec<String>,
    ) -> Vec<(String, i64)> {
        map_pairs(&jet_map_slice_keys_kernel(&map_from_pairs(entries), keys))
    }

    pub(super) fn map_from_keys_i64(keys: Vec<String>, default: i64) -> Vec<(String, i64)> {
        map_pairs(&jet_map_from_keys_kernel(keys, default))
    }

    pub(super) fn map_contains_i64(entries: Vec<(String, i64)>, needle: i64) -> bool {
        jet_map_contains_value_kernel(&map_from_pairs(entries), &needle)
    }

    pub(super) fn map_remove_i64(
        entries: Vec<(String, i64)>,
        key: String,
    ) -> Option<i64> {
        let mut map = map_from_pairs(entries);
        jet_map_pop_kernel(&mut map, &key).ok()
    }

    pub(super) fn list_pop<T>(values: &mut Vec<T>) -> Option<T> {
        jet_list_pop_kernel(values).ok()
    }

    pub(super) fn list_remove_value<T: Clone + PartialEq>(
        values: &mut Vec<T>,
        value: T,
    ) -> Option<T> {
        jet_list_remove_value_kernel(values, value)
    }

    pub(super) fn list_remove_slot<T: Clone>(
        values: &mut Vec<T>,
        index: i64,
    ) -> Result<T, String> {
        jet_list_remove_slot_kernel(values, index)
    }

    pub(super) fn list_count<T: PartialEq>(values: &[T], value: &T) -> i64 {
        jet_list_count_kernel(values, value)
    }

    pub(super) fn set_pop_i64(
        values: &mut std::collections::HashSet<i64>,
        value: &i64,
    ) -> Option<i64> {
        jet_set_pop_kernel(values, value).ok()
    }

    pub(super) fn deque_pop_front<T>(values: &mut std::collections::VecDeque<T>) -> Option<T> {
        jet_deque_pop_front_kernel(values).ok()
    }

    pub(super) fn deque_pop_back<T>(values: &mut std::collections::VecDeque<T>) -> Option<T> {
        jet_deque_pop_back_kernel(values).ok()
    }

    pub(super) fn priority_queue_pop<T: Ord>(values: &mut std::collections::BinaryHeap<T>) -> Option<T> {
        jet_priority_queue_pop_kernel(values).ok()
    }

    pub(super) fn map_pop_first_i64(
        entries: Vec<(String, i64)>,
    ) -> (Option<String>, Option<i64>, Vec<(String, i64)>) {
        let mut map = map_from_pairs(entries);
        let key = map.keys().next().cloned();
        let value = jet_map_pop_first_kernel(&mut map).ok();
        (key, value, map_pairs(&map))
    }

    pub(super) fn list_slice<T: Clone>(xs: &[T], start: i64, end: i64) -> Vec<T> {
        jet_list_slice(xs, start, end)
    }

    pub(super) fn list_equal<T: PartialEq>(left: &[T], right: &[T]) -> bool {
        jet_list_equal(left, right)
    }

    pub(super) fn list_binary_search<T: Ord>(xs: &[T], needle: &T) -> JetOutcome<i64, JetAbsent> {
        jet_list_binary_search(xs, needle)
    }

    pub(super) fn list_union<T: Clone + Eq>(left: &[T], right: &[T]) -> Vec<T> {
        jet_list_union(left, right)
    }

    pub(super) fn list_intersection<T: Clone + Eq>(left: &[T], right: &[T]) -> Vec<T> {
        jet_list_intersection(left, right)
    }

    pub(super) fn list_difference<T: Clone + Eq>(left: &[T], right: &[T]) -> Vec<T> {
        jet_list_difference(left, right)
    }

    pub(super) fn list_random<T: Clone>(xs: &[T]) -> JetOutcome<T, JetAbsent> {
        jet_list_random(xs)
    }

    pub(super) fn list_min_max_i64(xs: &[i64]) -> JetOutcome<(i64, i64), JetAbsent> {
        jet_list_min_max(xs, |min, max| (min, max))
    }

    pub(super) fn list_replace<T: Clone>(xs: &[T], index: i64, new: T) -> Vec<T> {
        jet_list_replace(xs, index, new)
    }

    pub(super) fn list_starts_with<T: PartialEq>(xs: &[T], prefix: &[T]) -> bool {
        jet_list_starts_with(xs, prefix)
    }

    pub(super) fn list_ends_with<T: PartialEq>(xs: &[T], suffix: &[T]) -> bool {
        jet_list_ends_with(xs, suffix)
    }

    /// The split itself stays in the Prelude kernel (I9); only the per-column
    /// READ and REPUBLISH representation is the engine's business, so the
    /// wrapper is generic over the column word instead of pinned to `i64`.
    pub(super) fn list_unzip_pairs<T, U>(xs: Vec<(T, U)>) -> (Vec<T>, Vec<U>) {
        jet_list_unzip(xs)
    }

    pub(super) fn priority_queue_remove_value(
        pq: &mut std::collections::BinaryHeap<i64>,
        value: i64,
    ) -> JetOutcome<i64, JetAbsent> {
        jet_priority_queue_remove_value_kernel(pq, value)
    }

    pub(super) fn priority_queue_remove_slot(
        pq: &mut std::collections::BinaryHeap<i64>,
        index: i64,
        file: &str,
        line: u32,
    ) -> Result<JetOutcome<i64, JetAbsent>, String> {
        jet_priority_queue_remove_slot_kernel(pq, index, file, line)
    }
}

/// Result-arena Option ABI: a 1-based `rt.results` handle carrying `(ok, bits)`.
/// `LowerCtx::uses_result_option_abi` is the ONE fact that says which host
/// answers this way, and it enumerates the ops explicitly. A host that reaches
/// for this helper without an arm in that predicate hands a handle to a consumer
/// that decodes it as `option_packed`, and a 1-based handle is never zero, so the
/// miss reads as `Some(handle - 1)` — a wrong answer, never a refusal.
fn option_i64(rt: &mut crate::JitRuntime, value: Option<i64>) -> i64 {
    crate::runtime_host::alloc_jit_result(
        rt,
        value.is_some(),
        value.unwrap_or_default() as u64,
    )
}

fn alloc_error_result(
    rt: &mut crate::JitRuntime,
    error: jet_foundation::Outcome::AllocError,
) -> i64 {
    let record = rt.heap.alloc_record(2);
    let allocator = rt.heap.alloc_string(error.allocator);
    let _ = rt.heap.record_set_int(record, 0, error.requested_bytes);
    let _ = rt.heap.record_set_string(record, 1, allocator);
    crate::runtime_host::alloc_jit_result(rt, false, record as u64)
}

/// Packed Option ABI for `Option(Int)`: 0 = None, value+1 = Some. This is what
/// `uses_result_option_abi`'s `_ => false` default assumes, so it is the carrier
/// for every Option-returning host with no arm in that predicate. Sound only for
/// payloads that are never `-1` (indices, lengths, arena handles).
fn option_packed(value: Option<i64>) -> i64 {
    match value {
        Some(v) => v.wrapping_add(1),
        None => 0,
    }
}

/// Record an out-of-bounds trap. Returns normally; JIT code branches to its
/// epilogue at the next `emit_trap_check` (I1 — no Rust panic ever unwinds
/// through a JIT frame; cranelift-jit emits no unwind tables for them).
fn trap_index(len: i64, index: i64, line: u32) {
    Concurrency::with_runtime_mut(|rt| {
        rt.set_runtime_stop(
            "E3010",
            line,
            &jet_foundation::Outcome::jet_list_bounds_message(len, index),
        );
    });
}

fn jet_jit_list_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_list())
}

fn jet_jit_list_uninit(len: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_uninit_list(len.max(0) as usize))
}

/// `core.process.argv()` — List(String) matching AOT `jet_std_io_args`, fed by the
/// `with_program_args` argv installed for this JIT run (falls back to
/// `std::env::args` when unset, same as a bare host process).
fn jet_jit_io_args() -> i64 {
    let args = {
        let installed = crate::program_args();
        if installed.is_empty() {
            std::env::args().collect::<Vec<_>>()
        } else {
            installed
        }
    };
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for arg in args {
            let sid = rt.heap.alloc_string(arg);
            rt.heap
                .list_push_int(list, sid)
                .expect("jit process.argv push");
        }
        list
    })
}

fn jet_jit_list_push(list: i64, v: i64) {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .list_push_int(list, v)
            .expect("jit list push: bad handle");
    });
}

fn jet_jit_list_push_f64(list: i64, v: f64) {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .list_push_float(list, v)
            .expect("jit list push f64: bad handle");
    });
}

fn jet_jit_list_try_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match collection_semantics::try_list_new::<i64>() {
            Ok(values) => {
                let list = rt.heap.alloc_int_list(values);
                crate::runtime_host::alloc_jit_result(rt, true, list as u64)
            }
            Err(error) => alloc_error_result(rt, error),
        }
    })
}

fn jet_jit_list_try_with_capacity(capacity: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match collection_semantics::try_list_with_capacity::<i64>(
            capacity,
            &rt.program_allocator,
        ) {
            Ok(values) => {
                let list = rt.heap.alloc_int_list(values);
                crate::runtime_host::alloc_jit_result(rt, true, list as u64)
            }
            Err(error) => alloc_error_result(rt, error),
        }
    })
}

fn jet_jit_list_try_push(list: i64, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let mut values = rt.heap.clone_int_list(list).unwrap_or_default();
        match collection_semantics::try_list_push(&mut values, value) {
            Ok(()) => {
                let _ = rt.heap.replace_int_list(list, values);
                crate::runtime_host::alloc_jit_result(rt, true, 0)
            }
            Err(error) => alloc_error_result(rt, error),
        }
    })
}

fn jet_jit_list_try_push_f64(list: i64, value: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let mut values = {
            let len = rt.heap.list_len(list).unwrap_or(0);
            (0..len)
                .map(|index| rt.heap.list_get_float(list, index).unwrap_or_default())
                .collect::<Vec<_>>()
        };
        match collection_semantics::try_list_push(&mut values, value) {
            Ok(()) => {
                let _ = rt.heap.replace_float_list(list, values);
                crate::runtime_host::alloc_jit_result(rt, true, 0)
            }
            Err(error) => alloc_error_result(rt, error),
        }
    })
}

fn jet_jit_list_try_reserve(list: i64, additional: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let mut values = rt.heap.clone_int_list(list).expect("jit list try_reserve: bad handle");
        match collection_semantics::try_list_reserve(&mut values, additional) {
            Ok(()) => {
                let _ = rt.heap.replace_int_list(list, values);
                crate::runtime_host::alloc_jit_result(rt, true, 0)
            }
            Err(error) => alloc_error_result(rt, error),
        }
    })
}

fn jet_jit_list_try_reserve_f64(list: i64, additional: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).expect("jit list try_reserve f64: bad handle");
        let mut values = (0..len)
            .map(|index| rt.heap.list_get_float(list, index).unwrap_or_default())
            .collect::<Vec<_>>();
        match collection_semantics::try_list_reserve(&mut values, additional) {
            Ok(()) => {
                let _ = rt.heap.replace_float_list(list, values);
                crate::runtime_host::alloc_jit_result(rt, true, 0)
            }
            Err(error) => alloc_error_result(rt, error),
        }
    })
}

fn jet_jit_string_try_push(text: i64, addition: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let mut value = rt.heap.clone_string(text).unwrap_or_default();
        let addition = rt.heap.clone_string(addition).unwrap_or_default();
        match collection_semantics::try_string_push(&mut value, &addition) {
            Ok(()) => {
                if let Some(target) = rt.heap.get_string_mut(text) {
                    *target = value;
                }
                crate::runtime_host::alloc_jit_result(rt, true, 0)
            }
            Err(error) => alloc_error_result(rt, error),
        }
    })
}

fn jet_jit_list_push_range(
    list: i64,
    start: i64,
    end: i64,
    exclusive: i8,
) {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .list_push_range(list, start, end, exclusive != 0)
            .expect("jit list push Range: bad handle");
    });
}

fn jet_jit_list_len(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.list_len(list).expect("jit list len: bad handle"))
}

fn closure_callback_slot(handle: i64) -> Option<JitCallableSlot> {
    Concurrency::with_runtime_mut(|rt| {
        let slot = jit_callable_parts(rt, handle);
        if slot.is_none() {
            jet_foundation::ice!(None, "invalid resident collection callback");
        }
        slot
    })
}

fn closure_trapped() -> bool {
    Concurrency::with_runtime_mut(|rt| rt.trapped.is_some())
}

// The binder records the callback pointer, environment word, and env flag as
// one slot. Each adapter selects the ABI declared by the callback's TIR type.
unsafe fn invoke_closure_bool(slot: JitCallableSlot, value: i64) -> bool {
    if slot.has_env {
        let callback: unsafe extern "C" fn(i64, i64) -> i8 =
            std::mem::transmute(slot.fn_ptr as usize);
        callback(slot.env, value) != 0
    } else {
        let callback: unsafe extern "C" fn(i64) -> i8 =
            std::mem::transmute(slot.fn_ptr as usize);
        callback(value) != 0
    }
}

unsafe fn invoke_closure_i64(slot: JitCallableSlot, value: i64) -> i64 {
    if slot.has_env {
        let callback: unsafe extern "C" fn(i64, i64) -> i64 =
            std::mem::transmute(slot.fn_ptr as usize);
        callback(slot.env, value)
    } else {
        let callback: unsafe extern "C" fn(i64) -> i64 =
            std::mem::transmute(slot.fn_ptr as usize);
        callback(value)
    }
}

unsafe fn invoke_closure_i8(slot: JitCallableSlot, value: i64) -> i8 {
    if slot.has_env {
        let callback: unsafe extern "C" fn(i64, i64) -> i8 =
            std::mem::transmute(slot.fn_ptr as usize);
        callback(slot.env, value)
    } else {
        let callback: unsafe extern "C" fn(i64) -> i8 =
            std::mem::transmute(slot.fn_ptr as usize);
        callback(value)
    }
}

unsafe fn invoke_closure_i32(slot: JitCallableSlot, value: i64) -> i32 {
    if slot.has_env {
        let callback: unsafe extern "C" fn(i64, i64) -> i32 =
            std::mem::transmute(slot.fn_ptr as usize);
        callback(slot.env, value)
    } else {
        let callback: unsafe extern "C" fn(i64) -> i32 =
            std::mem::transmute(slot.fn_ptr as usize);
        callback(value)
    }
}

unsafe fn invoke_closure_f64(slot: JitCallableSlot, value: i64) -> f64 {
    if slot.has_env {
        let callback: unsafe extern "C" fn(i64, i64) -> f64 =
            std::mem::transmute(slot.fn_ptr as usize);
        callback(slot.env, value)
    } else {
        let callback: unsafe extern "C" fn(i64) -> f64 =
            std::mem::transmute(slot.fn_ptr as usize);
        callback(value)
    }
}

unsafe fn invoke_closure_unit(slot: JitCallableSlot, value: i64) {
    if slot.has_env {
        let callback: unsafe extern "C" fn(i64, i64) =
            std::mem::transmute(slot.fn_ptr as usize);
        callback(slot.env, value);
    } else {
        let callback: unsafe extern "C" fn(i64) =
            std::mem::transmute(slot.fn_ptr as usize);
        callback(value);
    }
}

fn alloc_closure_ints(values: Vec<i64>) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_int_list(values))
}

fn alloc_closure_floats(values: Vec<f64>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for value in values {
            if rt.heap.list_push_float(list, value).is_none() {
                jet_foundation::ice!(None, "JIT collection closure list allocation failed");
            }
        }
        list
    })
}

fn jet_jit_list_closure_any(list: i64, callback: i64) -> i8 {
    let Some(slot) = closure_callback_slot(callback) else {
        return 0;
    };
    let values = clone_list_ints(list);
    i8::from(collection_semantics::list_closure_any(values, |value| {
        if closure_trapped() {
            return true;
        }
        let matched = unsafe { invoke_closure_bool(slot, *value) };
        closure_trapped() || matched
    }))
}

fn jet_jit_list_closure_all(list: i64, callback: i64) -> i8 {
    let Some(slot) = closure_callback_slot(callback) else {
        return 0;
    };
    let values = clone_list_ints(list);
    i8::from(collection_semantics::list_closure_all(values, |value| {
        if closure_trapped() {
            return false;
        }
        let matched = unsafe { invoke_closure_bool(slot, *value) };
        !closure_trapped() && matched
    }))
}

fn list_closure_map_i64(list: i64, callback: i64, mutable: bool) -> i64 {
    let Some(slot) = closure_callback_slot(callback) else {
        return 0;
    };
    let values = clone_list_ints(list);
    let mapped = if mutable {
        collection_semantics::list_closure_map_mut(values, |value| {
            if closure_trapped() {
                0
            } else {
                unsafe { invoke_closure_i64(slot, *value) }
            }
        })
    } else {
        collection_semantics::list_closure_map(values, |value| {
            if closure_trapped() {
                0
            } else {
                unsafe { invoke_closure_i64(slot, *value) }
            }
        })
    };
    if closure_trapped() {
        return 0;
    }
    alloc_closure_ints(mapped)
}

fn jet_jit_list_closure_map(list: i64, callback: i64) -> i64 {
    list_closure_map_i64(list, callback, false)
}

fn jet_jit_list_closure_map_mut(list: i64, callback: i64) -> i64 {
    list_closure_map_i64(list, callback, true)
}

fn list_closure_map_i8(list: i64, callback: i64, mutable: bool) -> i64 {
    let Some(slot) = closure_callback_slot(callback) else {
        return 0;
    };
    let values = clone_list_ints(list);
    let mapped = if mutable {
        collection_semantics::list_closure_map_mut(values, |value| {
            if closure_trapped() {
                0
            } else {
                i64::from(unsafe { invoke_closure_i8(slot, *value) })
            }
        })
    } else {
        collection_semantics::list_closure_map(values, |value| {
            if closure_trapped() {
                0
            } else {
                i64::from(unsafe { invoke_closure_i8(slot, *value) })
            }
        })
    };
    if closure_trapped() {
        return 0;
    }
    alloc_closure_ints(mapped)
}

fn jet_jit_list_closure_map_i8(list: i64, callback: i64) -> i64 {
    list_closure_map_i8(list, callback, false)
}

fn jet_jit_list_closure_map_i8_mut(list: i64, callback: i64) -> i64 {
    list_closure_map_i8(list, callback, true)
}

fn list_closure_map_i32(list: i64, callback: i64, mutable: bool) -> i64 {
    let Some(slot) = closure_callback_slot(callback) else {
        return 0;
    };
    let values = clone_list_ints(list);
    let mapped = if mutable {
        collection_semantics::list_closure_map_mut(values, |value| {
            if closure_trapped() {
                0
            } else {
                i64::from(unsafe { invoke_closure_i32(slot, *value) })
            }
        })
    } else {
        collection_semantics::list_closure_map(values, |value| {
            if closure_trapped() {
                0
            } else {
                i64::from(unsafe { invoke_closure_i32(slot, *value) })
            }
        })
    };
    if closure_trapped() {
        return 0;
    }
    alloc_closure_ints(mapped)
}

fn jet_jit_list_closure_map_i32(list: i64, callback: i64) -> i64 {
    list_closure_map_i32(list, callback, false)
}

fn jet_jit_list_closure_map_i32_mut(list: i64, callback: i64) -> i64 {
    list_closure_map_i32(list, callback, true)
}

fn list_closure_map_f64(list: i64, callback: i64, mutable: bool) -> i64 {
    let Some(slot) = closure_callback_slot(callback) else {
        return 0;
    };
    let values = clone_list_ints(list);
    let mapped = if mutable {
        collection_semantics::list_closure_map_f64_mut(values, |value| {
            if closure_trapped() {
                0.0
            } else {
                unsafe { invoke_closure_f64(slot, *value) }
            }
        })
    } else {
        collection_semantics::list_closure_map_f64(values, |value| {
            if closure_trapped() {
                0.0
            } else {
                unsafe { invoke_closure_f64(slot, *value) }
            }
        })
    };
    if closure_trapped() {
        return 0;
    }
    alloc_closure_floats(mapped)
}

fn jet_jit_list_closure_map_f64(list: i64, callback: i64) -> i64 {
    list_closure_map_f64(list, callback, false)
}

fn jet_jit_list_closure_map_f64_mut(list: i64, callback: i64) -> i64 {
    list_closure_map_f64(list, callback, true)
}

fn jet_jit_list_closure_filter(list: i64, callback: i64) -> i64 {
    let Some(slot) = closure_callback_slot(callback) else {
        return 0;
    };
    let values = clone_list_ints(list);
    let filtered = collection_semantics::list_closure_filter(values, |value| {
        if closure_trapped() {
            false
        } else {
            (unsafe { invoke_closure_bool(slot, *value) }) && !closure_trapped()
        }
    });
    if closure_trapped() {
        return 0;
    }
    alloc_closure_ints(filtered)
}

fn list_closure_each(list: i64, callback: i64, mutable: bool) -> i8 {
    let Some(slot) = closure_callback_slot(callback) else {
        return 0;
    };
    let values = clone_list_ints(list);
    if mutable {
        collection_semantics::list_closure_each_mut(values, |value| {
            if !closure_trapped() {
                unsafe { invoke_closure_unit(slot, *value) };
            }
        });
    } else {
        collection_semantics::list_closure_each(values, |value| {
            if !closure_trapped() {
                unsafe { invoke_closure_unit(slot, *value) };
            }
        });
    }
    0
}

fn jet_jit_list_closure_each(list: i64, callback: i64) -> i8 {
    list_closure_each(list, callback, false)
}

fn jet_jit_list_closure_each_mut(list: i64, callback: i64) -> i8 {
    list_closure_each(list, callback, true)
}

fn jet_jit_list_contains_str(list: i64, needle: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let needle = rt.heap.clone_string(needle).unwrap_or_default();
        let len = rt.heap.list_len(list).unwrap_or(0);
        for i in 0..len {
            let Some(sid) = rt.heap.list_get_int(list, i) else {
                continue;
            };
            if rt.heap.clone_string(sid).as_deref() == Some(needle.as_str()) {
                return 1;
            }
        }
        0
    })
}

/// Element-wise list equality for `[T]` / fixed lists (int/byte elements).
fn jet_jit_list_eq(a: i64, b: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        if a == b {
            return 1;
        }
        let (Some(la), Some(lb)) = (rt.heap.list_len(a), rt.heap.list_len(b)) else {
            return 0;
        };
        if la != lb {
            return 0;
        }
        for i in 0..la {
            match (rt.heap.list_get_int(a, i), rt.heap.list_get_int(b, i)) {
                (Some(x), Some(y)) if x == y => {}
                _ => return 0,
            }
        }
        1
    })
}

/// Typed list equality adapters preserve the element semantics of the shared
/// Prelude kernel. String elements are arena handles, so comparing raw IDs
/// would make equal independently-created lists unequal.
fn jet_jit_list_eq_str(a: i64, b: i64) -> i8 {
    collection_semantics::list_equal(&clone_list_strings(a), &clone_list_strings(b)) as i8
}

fn jet_jit_list_eq_f64(a: i64, b: i64) -> i8 {
    collection_semantics::list_equal(&clone_list_floats(a), &clone_list_floats(b)) as i8
}

/// Lexicographic list ordering for `[T]` / `[T#n]` under `<`, `<=`, `>`, `>=`.
///
/// AOT emits the bare Rust operator on whatever the operand lowers to —
/// `[i64; 3]` for `[Int#3]`, `Vec<i64>` for `[Int]` (the plain `rust_spell`
/// arm of `Codegen/TIR/emit/expressions.rs`) — so the ordering law for these
/// types is Rust's own slice `PartialOrd`; the Prelude has `jet_list_equal`
/// but no ordering kernel to call. Ask that one law once and report it as a
/// tag, so this host never decides which of the four operators was written and
/// float partiality survives the crossing: `0` less, `1` equal, `2` greater
/// (the same numbering `ordering_from_flags` uses), `3` incomparable — a NaN
/// element, where all four operators are false in both tiers.
fn list_order<T: PartialOrd>(left: &[T], right: &[T]) -> i8 {
    match left.partial_cmp(right) {
        Some(std::cmp::Ordering::Less) => 0,
        Some(std::cmp::Ordering::Equal) => 1,
        Some(std::cmp::Ordering::Greater) => 2,
        None => 3,
    }
}

fn jet_jit_list_order(a: i64, b: i64) -> i8 {
    list_order(&clone_list_ints(a), &clone_list_ints(b))
}

fn jet_jit_list_order_str(a: i64, b: i64) -> i8 {
    list_order(&clone_list_strings(a), &clone_list_strings(b))
}

fn jet_jit_list_order_f64(a: i64, b: i64) -> i8 {
    list_order(&clone_list_floats(a), &clone_list_floats(b))
}

/// Mirror AOT `jet_iter_indexes(n)` — materialize `Iter<Int>` as a list handle.
fn jet_jit_list_indexes(n: i64) -> i64 {
    let n = n.max(0);
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for i in 0..n {
            rt.heap
                .list_push_int(list, i)
                .expect("jit indexes push");
        }
        list
    })
}

fn jet_jit_loop_stride_check(stride: i64) -> i64 {
    if stride <= 0 {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_runtime_stop(
                "E3001",
                0,
                jet_foundation::Outcome::jet_loop_stride_message(),
            );
        });
    }
    stride
}

fn jet_jit_list_get(list: i64, idx: i64, line: u32) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.list_get_int(list, idx) {
        Some(value) => value,
        None => {
            if rt.heap.list_len(list).is_none() {
                jet_foundation::ice!(None, "jit list get: bad handle");
            }
            rt.set_runtime_stop(
                "E3010",
                line,
                &jet_foundation::Outcome::jet_list_bounds_message(
                    rt.heap.list_len(list).unwrap_or_default(),
                    idx,
                ),
            );
            0
        }
    })
}

fn jet_jit_list_get_f64(list: i64, idx: i64, line: u32) -> f64 {
    Concurrency::with_runtime_mut(|rt| {
        if let Some(value) = crate::Compute::try_get_list_f64(rt, list, idx) {
            return value;
        }
        match rt.heap.list_get_float(list, idx) {
            Some(value) => value,
            None => {
                if rt.heap.list_len(list).is_none() {
                    jet_foundation::ice!(None, "jit list get f64: bad handle");
                }
                rt.set_runtime_stop(
                    "E3010",
                    line,
                    &jet_foundation::Outcome::jet_list_bounds_message(
                        rt.heap.list_len(list).unwrap_or_default(),
                        idx,
                    ),
                );
                0.0
            }
        }
    })
}

/// D-SOA-TIER1=A / D-SOA1: THE read that pulls one record out of a
/// `#Layout(columnar)` list on the Cranelift tier.
///
/// The tier holds a columnar list as its logical rows (D-SOA-TIER1=A), so this
/// host marshals those rows into the shared Prelude column store and reads them
/// with the Prelude's own `jet_columns_gather` — the same source AOT compiles
/// and the same store the interpreter ambient builds. Nothing about the layout,
/// the row bookkeeping, the bounds selection or the wording is decided here: a
/// Cranelift host marshals, it does not re-encode policy (I9).
///
/// The gathered cells come back in column order, which is the declared
/// stored-field order the arena already lays a record of that struct out in, so
/// the row is rebuilt slot-for-slot and the fused field read can index it with
/// the very column number the store was built with (I8: one numbering).
fn jet_jit_columnar_gather(list: i64, idx: i64, line: u32) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(rows) = rt.heap.record_rows(list) else {
            jet_foundation::ice!(None, "jit columnar gather: bad handle");
        };
        // Every row has one cell per stored field, so row 0 sizes the store; an
        // empty list has no row and therefore no column, which is exactly the
        // zero-row store the shared bounds stop reports against.
        let width = rows.first().map_or(0, Vec::len);
        match jet_codegen::columns::JetColumns::from_rows(width, rows).gather(idx) {
            Ok(cells) => rt.heap.alloc_record_cells(cells),
            Err(error) => {
                rt.set_runtime_stop("E3010", line, &error.message());
                0
            }
        }
    })
}

fn jet_jit_fixed_list_get(list: i64, idx: i64, _line: u32) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.list_get_int_proven(list, idx))
}

fn jet_jit_fixed_list_get_f64(list: i64, idx: i64, _line: u32) -> f64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.list_get_float_proven(list, idx))
}

fn jet_jit_list_get_range(list: i64, idx: i64, line: u32) -> (i64, i64, bool) {
    Concurrency::with_runtime_mut(|rt| match rt.heap.list_get_range(list, idx) {
        Some(value) => value,
        None => {
            if rt.heap.list_len(list).is_none() {
                jet_foundation::ice!(None, "jit list get Range: bad handle");
            }
            rt.set_runtime_stop(
                "E3010",
                line,
                &jet_foundation::Outcome::jet_list_bounds_message(
                    rt.heap.list_len(list).unwrap_or_default(),
                    idx,
                ),
            );
            (0, 0, false)
        }
    })
}

fn jet_jit_list_get_range_start(list: i64, idx: i64, line: u32) -> i64 {
    jet_jit_list_get_range(list, idx, line).0
}

fn jet_jit_list_get_range_end(list: i64, idx: i64, line: u32) -> i64 {
    jet_jit_list_get_range(list, idx, line).1
}

fn jet_jit_list_get_range_exclusive(list: i64, idx: i64, line: u32) -> i8 {
    i8::from(jet_jit_list_get_range(list, idx, line).2)
}

/// Packed Option carrier: `0` = absent, otherwise `value + 1`.
///
/// Deliberately *not* the carrier `jet_jit_map_get_opt` returns. These two are
/// siblings in name only: this one is packed, that one is a result-arena
/// handle, and the JIT lowering discriminates with `uses_result_option_abi`
/// rather than assuming the family is uniform.
///
/// The packed carrier is retained here (it allocates nothing on a hot lookup)
/// but it is *not* free of representational limits, and this comment used to
/// claim otherwise. `Type::Int` is not `Type::IntN`, so a plain `Int` element
/// does reach this path, and `value + 1` then aliases a `-1` element onto
/// `None` and overflows at `i64::MAX`. `[-1].get(0)` decoding as absent is a
/// live defect of this encoding, independent of the map carrier above; fixing
/// it means moving this producer to the arena carrier and updating the
/// `GetList` arm of `uses_result_option_abi` with it.
///
/// #1995 sibling: the carrier is *kind*-sensitive, not int-only. Its gates are
/// float-capable — `TBuiltinOp::GetList` (`jit/safety.rs`, `jit_list_native_type`
/// minus `IntN`) and `TBuiltinOp::Last` (same gate) both admit a `[Float]`, and
/// both lower straight to this host. `list_get_int` answers `None` for a
/// `JetVal::Float`, so reading only through it made every `[Float].get(i)` and
/// `[Float].last()` decode as absent: a silent wrong answer on the native tier,
/// with no trap and no deopt to reveal it. The consumer already knows better —
/// `lower_ctx.rs::unpack_option_payload_with_abi` bitcasts `packed - 1` to
/// `f64` for a float payload, and the sibling producer `jet_jit_list_pop`
/// already packs `f64::to_bits() + 1`. This producer now agrees with both.
fn jet_jit_list_get_opt(list: i64, idx: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if rt.heap.list_len(list).is_none() {
            // An impossible handle is an engine fault, not a program stop.
            // Recording it beats panicking even now that the generated
            // `host_seam` boundary would convert a panic (#1997): the fault
            // rail names the defect, an unwind only reports "a seam panicked".
            rt.set_host_fault("jit list get_opt: bad list handle");
            return 0;
        }
        if let Some(value) = rt.heap.list_get_int(list, idx) {
            return value.wrapping_add(1);
        }
        // `list_get_float` is strict (`JetVal::Float` only), so the two reads
        // are disjoint and the int carrier keeps its exact prior encoding.
        if let Some(value) = rt.heap.list_get_float(list, idx) {
            return (value.to_bits() as i64).wrapping_add(1);
        }
        0
    })
}

fn jet_jit_list_set(list: i64, idx: i64, v: i64, line: u32) {
    Concurrency::with_runtime_mut(|rt| {
        if rt.heap.list_len(list).is_none() {
            jet_foundation::ice!(None, "jit list set: bad handle");
        }
        if rt.heap.list_set_int(list, idx, v).is_none() {
            trap_index(rt.heap.list_len(list).unwrap_or_default(), idx, line);
        }
    });
}

fn jet_jit_list_set_f64(list: i64, idx: i64, v: f64, line: u32) {
    Concurrency::with_runtime_mut(|rt| {
        if crate::Compute::try_set_list_f64(rt, list, idx, v) {
            return;
        }
        if rt.heap.list_len(list).is_none() {
            jet_foundation::ice!(None, "jit list set f64: bad handle");
        }
        if rt.heap.list_set_float(list, idx, v).is_none() {
            trap_index(rt.heap.list_len(list).unwrap_or_default(), idx, line);
        }
    });
}

fn jet_jit_list_sort(list: i64) {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .list_sort_int(list)
            .expect("jit list sort: bad handle")
    });
}

fn jet_jit_list_sort_desc(list: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let mut values = rt
            .heap
            .clone_int_list(list)
            .expect("jit list sort_desc: bad handle");
        values.sort_by(|left, right| right.cmp(left));
        for (index, value) in values.into_iter().enumerate() {
            rt.heap
                .list_set_int(list, index as i64, value)
                .expect("jit list sort_desc: set");
        }
    });
}

/// Lexicographic sort of a `[String]` list (handles are string arena ids).
fn jet_jit_list_sort_str(list: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some(ids) = rt.heap.clone_int_list(list) else {
            jet_foundation::ice!(None, "jit list sort_str: bad handle");
        };
        let mut pairs: Vec<(String, i64)> = ids
            .into_iter()
            .map(|id| {
                (
                    rt.heap.clone_string(id).unwrap_or_default(),
                    id,
                )
            })
            .collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        for (i, (_, id)) in pairs.into_iter().enumerate() {
            rt.heap
                .list_set_int(list, i as i64, id)
                .expect("jit list sort_str: set");
        }
    });
}

fn jet_jit_list_sort_str_desc(list: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some(ids) = rt.heap.clone_int_list(list) else {
            jet_foundation::ice!(None, "jit list sort_str_desc: bad handle");
        };
        let mut pairs: Vec<(String, i64)> = ids
            .into_iter()
            .map(|id| (rt.heap.clone_string(id).unwrap_or_default(), id))
            .collect();
        pairs.sort_by(|a, b| b.0.cmp(&a.0));
        for (index, (_, id)) in pairs.into_iter().enumerate() {
            rt.heap
                .list_set_int(list, index as i64, id)
                .expect("jit list sort_str_desc: set");
        }
    });
}

fn jet_jit_list_clone(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_list(list).expect("jit list clone: bad handle"))
}

fn jet_jit_list_copy(list: i64) -> i64 {
    let values = clone_list_ints(list);
    alloc_from_ints(&collection_semantics::list_copy_i64(&values))
}

fn jet_jit_list_count(list: i64, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(values) = rt.heap.clone_int_list(list) else {
            jet_foundation::ice!(None, "jit list count: bad handle");
        };
        collection_semantics::list_count(&values, &value)
    })
}

fn jet_jit_list_remove_value(list: i64, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let values: &mut Vec<jet_rt::JetVal> =
            unsafe { &mut *(&mut rt.heap as *mut jet_rt::JetArena as *mut Vec<jet_rt::JetVal>) };
        let Some(jet_rt::JetVal::List(xs)) = values.get_mut(list as usize) else {
            jet_foundation::ice!(None, "jit list remove value: bad handle");
        };
        match collection_semantics::list_remove_value(xs, jet_rt::JetVal::Int(value)) {
            Some(jet_rt::JetVal::Int(removed)) => option_packed(Some(removed)),
            Some(_) | None => 0,
        }
    })
}

/// Element-kind preserving window (#1995).
///
/// The list carrier is heterogeneous at this ABI: `[Float]` stores
/// `JetVal::Float`, `[Range]` stores `JetVal::Range`, `[String]` and record
/// lists store handle ints. Materializing the window through the i64 element
/// view therefore lost every non-integer list — `clone_int_list` answers
/// `None` as soon as one element is not an `Int`, and the `.expect` behind it
/// panicked in a seam that was then an `extern "C"` frame, aborting the process
/// instead of reporting. `xs[1..2]` on a `[Float]` reaches here from a correct
/// program: `jit_list_native_type` (`jit/safety.rs`) admits float slices.
///
/// The shared Prelude kernel keeps the window policy (I9): it runs over the
/// index vector, and the arena copies whatever elements those indexes name.
fn jet_jit_list_slice(list: i64, start: i64, end: i64, _line: u32) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(len) = rt.heap.list_len(list) else {
            rt.set_host_fault("jit list slice: bad list handle");
            return 0;
        };
        let indexes = (0..len).collect::<Vec<_>>();
        let kept = collection_semantics::list_slice(&indexes, start, end);
        let (Some(&first), Some(&last)) = (kept.first(), kept.last()) else {
            return rt.heap.alloc_empty_list();
        };
        if let Some(window) = rt.heap.list_slice(list, first, last + 1) {
            return window;
        }
        // The arena's raw window covers a `JetVal::List` carrier only. A
        // fixed-size `UninitList` keeps its elements behind the integer view
        // it is built from, so materialize that one through the same view.
        let Some(values) = rt.heap.clone_int_list(list) else {
            rt.set_host_fault("jit list slice: uninitialized fixed-list carrier");
            return 0;
        };
        let out = rt.heap.alloc_empty_list();
        for &value in &values[first as usize..=last as usize] {
            let _ = rt.heap.list_push_int(out, value);
        }
        out
    })
}

fn jet_jit_list_range_end(
    list: i64,
    start: i64,
    end: i64,
    exclusive: i64,
    line: u32,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(len) = rt.heap.list_len(list) else {
            jet_foundation::ice!(None, "jit Range window: bad list handle");
        };
        match range_semantics::jet_checked_view_bounds(start, end, exclusive != 0, len) {
            Ok((_, end_exclusive)) => end_exclusive,
            Err(message) => {
                rt.set_runtime_stop("E3001", line, &message);
                0
            }
        }
    })
}

fn alloc_view_mut(
    rt: &mut crate::JitRuntime,
    list: i64,
    start: usize,
    end_exclusive: usize,
) -> i64 {
    let view = rt.heap.alloc_record(3);
    let _ = rt.heap.record_set_int(view, 0, list);
    let _ = rt.heap.record_set_int(view, 1, start as i64);
    let _ = rt
        .heap
        .record_set_int(view, 2, end_exclusive as i64 - 1);
    view
}

fn alloc_disjoint_result(
    rt: &mut crate::JitRuntime,
    result: Result<i64, String>,
) -> i64 {
    match result {
        Ok(value) => crate::runtime_host::alloc_jit_result(rt, true, value as u64),
        Err(error) => {
            let error = rt.heap.alloc_string(error);
            crate::runtime_host::alloc_jit_result(rt, false, error as u64)
        }
    }
}

fn jet_jit_split_write(list: i64, mid: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(len) = rt.heap.list_len(list) else {
            jet_foundation::ice!(None, "jit split_write: bad list handle");
        };
        let result = disjoint_semantics::split(len as usize, mid).map(
            |((left_start, left_end), (right_start, right_end))| {
                let pair = rt.heap.alloc_record(2);
                let left = alloc_view_mut(rt, list, left_start, left_end);
                let right = alloc_view_mut(rt, list, right_start, right_end);
                let _ = rt.heap.record_set_int(pair, 0, left);
                let _ = rt.heap.record_set_int(pair, 1, right);
                pair
            },
        );
        alloc_disjoint_result(rt, result)
    })
}

fn jet_jit_get_disjoint_write(list: i64, targets: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(len) = rt.heap.list_len(list) else {
            jet_foundation::ice!(None, "jit get_disjoint_write: bad list handle");
        };
        let Some(target_len) = rt.heap.list_len(targets) else {
            jet_foundation::ice!(None, "jit get_disjoint_write: bad targets handle");
        };
        let indices = (0..target_len)
            .map(|index| rt.heap.list_get_int(targets, index).unwrap_or_default())
            .collect::<Vec<_>>();
        let result = disjoint_semantics::indexes(len as usize, &indices).map(|ordered| {
            let mut views = vec![0; indices.len()];
            for (start, end, position) in ordered {
                views[position] = alloc_view_mut(rt, list, start, end);
            }
            let output = rt.heap.alloc_empty_list();
            for view in views {
                rt.heap
                    .list_push_int(output, view)
                    .expect("jit get_disjoint_write output");
            }
            output
        });
        alloc_disjoint_result(rt, result)
    })
}

fn jet_jit_range_contains(
    start: i64,
    end: i64,
    exclusive: i64,
    value: i64,
) -> i8 {
    range_semantics::jet_range_contains(start, end, exclusive != 0, value) as i8
}

fn jet_jit_range_show(start: i64, end: i64, exclusive: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap.alloc_string(range_semantics::jet_range_structural_text(
            start,
            end,
            exclusive != 0,
        ))
    })
}

fn jet_jit_range_equal(
    left_start: i64,
    left_end: i64,
    left_exclusive: i64,
    right_start: i64,
    right_end: i64,
    right_exclusive: i64,
) -> i8 {
    range_semantics::jet_range_equal(
        left_start,
        left_end,
        left_exclusive != 0,
        right_start,
        right_end,
        right_exclusive != 0,
    ) as i8
}

fn jet_jit_map_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_map())
}

fn jet_jit_map_clone(map: i64) -> i64 {
    alloc_map_pairs(&collection_semantics::map_copy_i64(clone_map_pairs(map)))
}

fn jet_jit_map_equal(left: i64, right: i64) -> i8 {
    i8::from(collection_semantics::map_equal_i64(
        clone_map_pairs(left),
        clone_map_pairs(right),
    ))
}

fn jet_jit_map_first(map: i64) -> i64 {
    let value = collection_semantics::map_first_key_i64(clone_map_pairs(map));
    Concurrency::with_runtime_mut(|rt| {
        let value = value.map(|key| rt.heap.alloc_string(key));
        option_i64(rt, value)
    })
}

fn jet_jit_map_to_list(map: i64) -> i64 {
    let entries = collection_semantics::map_entries_i64(clone_map_pairs(map));
    Concurrency::with_runtime_mut(|rt| {
        let out = rt.heap.alloc_empty_list();
        for (key, value) in entries {
            let row = rt.heap.alloc_record(2);
            let key_id = rt.heap.alloc_string(key);
            let _ = rt.heap.record_set_string(row, 0, key_id);
            let _ = rt.heap.record_set_int(row, 1, value);
            let _ = rt.heap.list_push_int(out, row);
        }
        out
    })
}

fn jet_jit_map_min(map: i64) -> i64 {
    let value = collection_semantics::map_min_i64(clone_map_pairs(map));
    Concurrency::with_runtime_mut(|rt| option_i64(rt, value))
}

fn jet_jit_map_max(map: i64) -> i64 {
    let value = collection_semantics::map_max_i64(clone_map_pairs(map));
    Concurrency::with_runtime_mut(|rt| option_i64(rt, value))
}

fn jet_jit_map_intersection(left: i64, right: i64) -> i64 {
    let pairs = collection_semantics::map_intersection_i64(
        clone_map_pairs(left),
        clone_map_pairs(right),
    );
    alloc_map_pairs(&pairs)
}

fn jet_jit_map_slice(map: i64, keys: i64) -> i64 {
    let key_ids = clone_list_ints(keys);
    let keys = Concurrency::with_runtime_mut(|rt| {
        key_ids
            .into_iter()
            .map(|key| rt.heap.clone_string(key).expect("jit map slice: string key"))
            .collect::<Vec<_>>()
    });
    let pairs = collection_semantics::map_slice_i64(clone_map_pairs(map), keys);
    alloc_map_pairs(&pairs)
}

fn jet_jit_map_from_keys(keys: i64, default: i64) -> i64 {
    let key_ids = clone_list_ints(keys);
    let keys = Concurrency::with_runtime_mut(|rt| {
        key_ids
            .into_iter()
            .map(|key| rt.heap.clone_string(key).expect("jit map from_keys: string key"))
            .collect::<Vec<_>>()
    });
    let pairs = collection_semantics::map_from_keys_i64(keys, default);
    alloc_map_pairs(&pairs)
}

fn jet_jit_map_contains_value(map: i64, needle: i64) -> i8 {
    i8::from(collection_semantics::map_contains_i64(
        clone_map_pairs(map),
        needle,
    ))
}

fn jet_jit_map_pop_first(map: i64) -> i64 {
    let (key, value, _) = collection_semantics::map_pop_first_i64(clone_map_pairs(map));
    if let Some(key) = key {
        Concurrency::with_runtime_mut(|rt| {
            let key_id = rt.heap.alloc_string(key);
            let _ = rt.heap.map_remove(map, key_id);
        });
    }
    Concurrency::with_runtime_mut(|rt| option_i64(rt, value))
}

/// D-MAP-MERGE1=E: clone `left`, then overwrite/insert every entry from `right`
/// (right wins on shared keys).
fn jet_jit_map_merge(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let out = {
            let len = rt
                .heap
                .map_len(left)
                .expect("jit map merge: bad left handle");
            let out = rt.heap.alloc_empty_map();
            for i in 0..len {
                let key = rt.heap.map_key_at(left, i).expect("jit map merge: left key");
                let value = rt
                    .heap
                    .map_value_at(left, i)
                    .expect("jit map merge: left value");
                rt.heap
                    .map_insert(out, key, value)
                    .expect("jit map merge: left insert");
            }
            out
        };
        let len = rt
            .heap
            .map_len(right)
            .expect("jit map merge: bad right handle");
        for i in 0..len {
            let key = rt
                .heap
                .map_key_at(right, i)
                .expect("jit map merge: right key");
            let value = rt
                .heap
                .map_value_at(right, i)
                .expect("jit map merge: right value");
            rt.heap
                .map_insert(out, key, value)
                .expect("jit map merge: right insert");
        }
        out
    })
}

fn jet_jit_map_insert(map: i64, key: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .map_insert(map, key, value)
            .expect("jit map insert: bad handle");
    });
}

fn jet_jit_map_try_insert(map: i64, key: i64, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let key_text = rt.heap.clone_string(key).unwrap_or_default();
        let pairs = clone_map_pairs_in_runtime(&mut rt.heap, map);
        let (_, result) = collection_semantics::try_map_insert_i64(pairs, key_text, value);
        match result {
            Ok(previous) => {
                let _ = rt.heap.map_insert(map, key, value);
                let option = option_i64(rt, previous);
                crate::runtime_host::alloc_jit_result(rt, true, option as u64)
            }
            Err(error) => alloc_error_result(rt, error),
        }
    })
}

fn jet_jit_map_increment(map: i64, key: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let current = rt.heap.map_get(map, key).unwrap_or(0);
        let one = rt.heap.int_from_i64(1);
        let next = rt.heap.int_add(current, one);
        let _ = rt.heap.map_insert(map, key, next);
    });
}

fn jet_jit_map_get(map: i64, key: i64, line: u32) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.map_get(map, key) {
        Some(value) => value,
        None => {
            if rt.heap.map_len(map).is_none() {
                jet_foundation::ice!(None, "jit map get: bad handle");
            }
            let key_text = match rt.heap.clone_string(key) {
                Some(key) => jet_foundation::Outcome::jet_missing_map_key_value(key),
                None => jet_foundation::Outcome::jet_missing_map_key_message(None),
            };
            rt.set_runtime_stop("E3001", line, &key_text);
            0
        }
    })
}

/// Result-arena Option handle (`result_is_ok` / `result_get_i64`), *not* the
/// packed `0 / value + 1` carrier its sibling `jet_jit_list_get_opt` returns.
///
/// The arena carrier is required, not incidental: a map value is an arbitrary
/// i64, so packing it as `value + 1` would alias a `-1` payload onto `None`.
/// Function values make that concrete — `bind_jit_callable` hands out
/// `-(index + 1)`, so the very first callable stored in a map has the handle
/// `-1`. Keeping presence in a separate `ok` bit is what makes any payload,
/// including `i64::MAX` and every negative handle, representable.
fn jet_jit_map_get_opt(map: i64, key: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if rt.heap.map_len(map).is_none() {
            jet_foundation::ice!(None, "jit map get_opt: bad handle");
        }
        let value = rt.heap.map_get(map, key);
        option_i64(rt, value)
    })
}

fn jet_jit_map_remove(map: i64, key: i64) -> i64 {
    let key_text = Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .clone_string(key)
            .expect("jit map remove: string key")
    });
    let value = collection_semantics::map_remove_i64(clone_map_pairs(map), key_text);
    if value.is_some() {
        Concurrency::with_runtime_mut(|rt| {
            let _ = rt.heap.map_remove(map, key);
        });
    }
    Concurrency::with_runtime_mut(|rt| option_i64(rt, value))
}

fn jet_jit_map_len(map: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.map_len(map).expect("jit map len: bad handle"))
}

fn jet_jit_map_key_at(map: i64, idx: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .map_key_at(map, idx)
            .expect("jit map key_at: bad handle")
    })
}

fn jet_jit_map_value_at(map: i64, idx: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .map_value_at(map, idx)
            .expect("jit map value_at: bad handle")
    })
}

fn jet_jit_map_keys(map: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.map_len(map).expect("jit map keys: bad handle");
        let out = rt.heap.alloc_empty_list();
        for i in 0..len {
            let key = rt.heap.map_key_at(map, i).expect("jit map keys: key");
            rt.heap
                .list_push_int(out, key)
                .expect("jit map keys: push");
        }
        out
    })
}

fn jet_jit_map_values(map: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.map_len(map).expect("jit map values: bad handle");
        let out = rt.heap.alloc_empty_list();
        for i in 0..len {
            let value = rt
                .heap
                .map_value_at(map, i)
                .expect("jit map values: value");
            rt.heap
                .list_push_int(out, value)
                .expect("jit map values: push");
        }
        out
    })
}

/// Eager materialization of AOT `jet_iter_*` adapters over list handles.
///
/// Cranelift can't host true `JetIter` values; producers already store the same
/// element sequence in a list handle. Adapters rewrite that sequence into a
/// fresh list so observable `to_list` / for-in / print values match AOT.
fn clone_list_ints(list: i64) -> Vec<i64> {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .clone_int_list(list)
            .expect("jit iter adapter: bad list handle")
    })
}

fn clone_list_strings(list: i64) -> Vec<String> {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .clone_int_list(list)
            .expect("jit string-list adapter: bad list handle")
            .into_iter()
            .map(|id| {
                rt.heap
                    .clone_string(id)
                    .expect("jit string-list adapter: bad string handle")
            })
            .collect()
    })
}

fn clone_list_floats(list: i64) -> Vec<f64> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt
            .heap
            .list_len(list)
            .expect("jit float-list adapter: bad list handle");
        (0..len)
            .map(|index| {
                rt.heap
                    .list_get_float(list, index)
                    .expect("jit float-list adapter: bad element")
            })
            .collect()
    })
}

fn clone_map_pairs_in_runtime(heap: &mut jet_rt::JetArena, map: i64) -> Vec<(String, i64)> {
    let len = heap.map_len(map).expect("jit map adapter: bad handle");
    (0..len)
        .map(|index| {
            let key_id = heap.map_key_at(map, index).expect("jit map adapter: key");
            let key = heap
                .clone_string(key_id)
                .expect("jit map adapter: string key");
            let value = heap
                .map_value_at(map, index)
                .expect("jit map adapter: value");
            (key, value)
        })
        .collect()
}

fn clone_map_pairs(map: i64) -> Vec<(String, i64)> {
    Concurrency::with_runtime_mut(|rt| clone_map_pairs_in_runtime(&mut rt.heap, map))
}

fn alloc_from_ints(xs: &[i64]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let out = rt.heap.alloc_empty_list();
        for &v in xs {
            rt.heap
                .list_push_int(out, v)
                .expect("jit iter adapter: push");
        }
        out
    })
}

fn alloc_from_strings(xs: &[String]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let out = rt.heap.alloc_empty_list();
        for value in xs {
            let id = rt.heap.alloc_string(value.clone());
            rt.heap
                .list_push_int(out, id)
                .expect("jit string-list adapter: push");
        }
        out
    })
}

fn alloc_from_floats(xs: &[f64]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let out = rt.heap.alloc_empty_list();
        for &value in xs {
            rt.heap
                .list_push_float(out, value)
                .expect("jit float-list adapter: push");
        }
        out
    })
}

fn alloc_map_pairs(pairs: &[(String, i64)]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let map = rt.heap.alloc_empty_map();
        for (key, value) in pairs {
            let key_id = rt.heap.alloc_string(key.clone());
            rt.heap
                .map_insert(map, key_id, *value)
                .expect("jit map adapter: insert");
        }
        map
    })
}

fn alloc_nested_from_ints(xs: &[Vec<i64>]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let out = rt.heap.alloc_empty_list();
        for inner in xs {
            let child = rt.heap.alloc_empty_list();
            for &value in inner {
                rt.heap
                    .list_push_int(child, value)
                    .expect("jit nested-list adapter: push");
            }
            rt.heap
                .list_push_int(out, child)
                .expect("jit nested-list adapter: outer push");
        }
        out
    })
}

fn transfer_progress(source: i64, target: i64) -> i64 {
    crate::IO::progress_transfer_state(source, target);
    target
}

fn transfer_progress_take(source: i64, target: i64, n: i64) -> i64 {
    crate::IO::progress_transfer_take_state(source, target, n);
    target
}

fn transfer_progress_skip(source: i64, target: i64, n: i64) -> i64 {
    crate::IO::progress_transfer_skip_state(source, target, n);
    target
}

fn transfer_progress_step(source: i64, target: i64, n: i64) -> i64 {
    crate::IO::progress_transfer_step_state(source, target, n);
    target
}

fn jet_jit_iter_take(list: i64, n: i64) -> i64 {
    let xs = clone_list_ints(list);
    let out = collection_semantics::iter_take(xs, n);
    transfer_progress_take(list, alloc_from_ints(&out), n)
}

fn jet_jit_iter_skip(list: i64, n: i64) -> i64 {
    let xs = clone_list_ints(list);
    let out = collection_semantics::iter_skip(xs, n);
    transfer_progress_skip(list, alloc_from_ints(&out), n)
}

fn jet_jit_iter_first(list: i64) -> i64 {
    match collection_semantics::iter_first(clone_list_ints(list)) {
        Some(value) => value.wrapping_add(1),
        None => 0,
    }
}

fn jet_jit_iter_first_string(list: i64) -> i64 {
    match collection_semantics::iter_first(clone_list_strings(list)) {
        Some(value) => Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value).wrapping_add(1)),
        None => 0,
    }
}

fn jet_jit_iter_first_float(list: i64) -> i64 {
    match collection_semantics::iter_first(clone_list_floats(list)) {
        Some(value) => value.to_bits().wrapping_add(1) as i64,
        None => 0,
    }
}

fn jet_jit_iter_skip_string(list: i64, n: i64) -> i64 {
    let xs = clone_list_strings(list);
    let out = collection_semantics::iter_skip(xs, n);
    transfer_progress_skip(list, alloc_from_strings(&out), n)
}

fn jet_jit_iter_skip_float(list: i64, n: i64) -> i64 {
    let xs = clone_list_floats(list);
    let out = collection_semantics::iter_skip(xs, n);
    transfer_progress_skip(list, alloc_from_floats(&out), n)
}

fn jet_jit_iter_step_by(list: i64, n: i64) -> i64 {
    let xs = clone_list_ints(list);
    let out = collection_semantics::iter_step_by(xs, n);
    transfer_progress_step(list, alloc_from_ints(&out), n)
}

/// `string_elems != 0` → compare string contents (handles may differ); else i64 eq.
fn jet_jit_iter_dedup(list: i64, string_elems: i64) -> i64 {
    let string_elems = string_elems != 0;
    let out = if string_elems {
        let xs = clone_list_strings(list);
        alloc_from_strings(&collection_semantics::iter_dedup(xs))
    } else {
        let xs = clone_list_ints(list);
        alloc_from_ints(&collection_semantics::iter_dedup(xs))
    };
    crate::IO::progress_transfer_dedup_state(list, out, string_elems);
    out
}

fn jet_jit_iter_chunks(list: i64, n: i64) -> i64 {
    let xs = clone_list_ints(list);
    let out = alloc_nested_from_ints(&collection_semantics::iter_chunks(xs, n));
    crate::IO::progress_transfer_chunks_state(list, out, n);
    out
}

fn jet_jit_iter_windows(list: i64, n: i64) -> i64 {
    let xs = clone_list_ints(list);
    let out = alloc_nested_from_ints(&collection_semantics::iter_windows(xs, n));
    crate::IO::progress_transfer_windows_state(list, out, n);
    out
}

fn jet_jit_list_sum_i64(list: i64) -> i64 {
    let values = clone_list_ints(list);
    collection_semantics::list_sum_i64(values)
}

fn jet_jit_list_product_i64(list: i64) -> i64 {
    collection_semantics::list_product_i64(clone_list_ints(list))
}

fn jet_jit_list_min_i64(list: i64) -> i64 {
    let value = collection_semantics::list_min_i64(clone_list_ints(list)).ok();
    Concurrency::with_runtime_mut(|rt| option_i64(rt, value))
}

fn jet_jit_list_max_i64(list: i64) -> i64 {
    let value = collection_semantics::list_max_i64(clone_list_ints(list)).ok();
    Concurrency::with_runtime_mut(|rt| option_i64(rt, value))
}

fn jet_jit_list_flatten(list: i64) -> i64 {
    let outer = clone_list_ints(list);
    let nested = Concurrency::with_runtime_mut(|rt| {
        outer
            .iter()
            .map(|inner| rt.heap.clone_int_list(*inner).unwrap_or_default())
            .collect::<Vec<_>>()
    });
    let out = alloc_from_ints(&collection_semantics::list_flatten_i64(nested));
    crate::IO::progress_transfer_flatten_state(list, out);
    out
}

fn jet_jit_list_intersperse(list: i64, separator: i64) -> i64 {
    let out = alloc_from_ints(&collection_semantics::list_intersperse_i64(
        clone_list_ints(list),
        separator,
    ));
    crate::IO::progress_transfer_intersperse_state(list, out);
    out
}

#[derive(Clone, Copy)]
enum JitZipValue {
    Bits(i64),
    Float(f64),
    Absent,
}

fn jit_zip_read_value(
    heap: &jet_rt::JetArena,
    list: i64,
    kind: crate::runtime_host::JitZipValueKind,
    index: usize,
) -> Option<JitZipValue> {
    let index = i64::try_from(index).ok()?;
    match kind {
        crate::runtime_host::JitZipValueKind::Float => {
            heap.list_get_float(list, index).map(JitZipValue::Float)
        }
        crate::runtime_host::JitZipValueKind::Int
        | crate::runtime_host::JitZipValueKind::Bool
        | crate::runtime_host::JitZipValueKind::Char
        | crate::runtime_host::JitZipValueKind::String
        | crate::runtime_host::JitZipValueKind::Opaque => {
            heap.list_get_int(list, index).map(JitZipValue::Bits)
        }
    }
}

/// Read record field `index` back in the kind `jit_zip_set_value` wrote it in.
///
/// Two callers need this exact per-kind accessor ladder: the per-column fill
/// record `zip(fill:)` passes, and `jet_jit_list_unzip` taking a row apart
/// again. It is the read half of `jit_zip_set_value` and must stay total over
/// the same `JitZipValueKind` set — the unzip host used to read every field
/// with `record_get_int`, so a `record_set_string` column returned `None`, the
/// row was dropped, and `["a","bb","c"].zip([1,2,3]).unzip()` answered two
/// EMPTY lists.
fn jit_zip_record_field(
    heap: &mut jet_rt::JetArena,
    kind: crate::runtime_host::JitZipValueKind,
    column_fills_handle: i64,
    index: usize,
) -> JitZipValue {
    let Some(index) = i64::try_from(index).ok() else {
        return JitZipValue::Absent;
    };
    match kind {
        crate::runtime_host::JitZipValueKind::Float => heap
            .record_get_float(column_fills_handle, index)
            .map_or(JitZipValue::Absent, JitZipValue::Float),
        crate::runtime_host::JitZipValueKind::Bool => heap
            .record_get_bool(column_fills_handle, index)
            .map_or(JitZipValue::Absent, |value| JitZipValue::Bits(i64::from(value))),
        crate::runtime_host::JitZipValueKind::Char => heap
            .record_get_char(column_fills_handle, index)
            .map_or(JitZipValue::Absent, |value| {
                JitZipValue::Bits(i64::from(u32::from(value)))
            }),
        crate::runtime_host::JitZipValueKind::Int => heap
            .record_get_int(column_fills_handle, index)
            .map_or(JitZipValue::Absent, JitZipValue::Bits),
        crate::runtime_host::JitZipValueKind::String => heap
            .record_get_string(column_fills_handle, index)
            .map_or(JitZipValue::Absent, JitZipValue::Bits),
        // `Opaque` is the ONE kind that names no payload representation, so it
        // must not assert one. It is also the only kind
        // `LowerCtx::unzip_column_kinds` can reach WITHOUT `JitMeta`: the
        // static `clif_ty` is distinct-blind, so a `distinct Name = String`
        // column looks like a plain I64 handle to it, while the row writer used
        // the distinct-AWARE erasure and stored a `String` cell. Asserting
        // `record_get_int` there read `None` — the same shape that answered two
        // empty lists, one ICE further on. So read the cell in ITS shape: the
        // column list this word is republished into is typed by the same field
        // type, so the cell's own shape is the representation that list slot
        // wants (`[Float]` slots are floats, `[String]`/`[Bool]`/`[Char]` slots
        // are the erased int word). `Int` stays first: it is the hot cell and
        // pays exactly one match, as before.
        crate::runtime_host::JitZipValueKind::Opaque => {
            if let Some(value) = heap.record_get_int(column_fills_handle, index) {
                return JitZipValue::Bits(value);
            }
            if let Some(value) = heap.record_get_float(column_fills_handle, index) {
                return JitZipValue::Float(value);
            }
            if let Some(value) = heap.record_get_bool(column_fills_handle, index) {
                return JitZipValue::Bits(i64::from(value));
            }
            if let Some(value) = heap.record_get_char(column_fills_handle, index) {
                return JitZipValue::Bits(i64::from(u32::from(value)));
            }
            heap.record_get_string(column_fills_handle, index)
                .map_or(JitZipValue::Absent, JitZipValue::Bits)
        }
    }
}

fn jit_zip_common_fill_value(
    kind: crate::runtime_host::JitZipValueKind,
    common_fill: i64,
) -> JitZipValue {
    match kind {
        crate::runtime_host::JitZipValueKind::Float => {
            JitZipValue::Float(f64::from_bits(common_fill as u64))
        }
        _ => JitZipValue::Bits(common_fill),
    }
}

fn jit_zip_pack_value(value: JitZipValue) -> i64 {
    match value {
        JitZipValue::Bits(value) => value.wrapping_add(1),
        JitZipValue::Float(value) => (value.to_bits() as i64).wrapping_add(1),
        JitZipValue::Absent => 0,
    }
}

fn jit_zip_set_value(
    heap: &mut jet_rt::JetArena,
    record: i64,
    index: usize,
    column: crate::runtime_host::JitZipColumn,
    value: JitZipValue,
) -> Option<()> {
    let index = i64::try_from(index).ok()?;
    if column.optional {
        return heap.record_set_int(record, index, jit_zip_pack_value(value));
    }
    match (column.field, value) {
        (crate::runtime_host::JitZipValueKind::Float, JitZipValue::Float(value)) => {
            heap.record_set_float(record, index, value)
        }
        (crate::runtime_host::JitZipValueKind::Int, JitZipValue::Bits(value))
        | (crate::runtime_host::JitZipValueKind::Opaque, JitZipValue::Bits(value)) => {
            heap.record_set_int(record, index, value)
        }
        (crate::runtime_host::JitZipValueKind::Bool, JitZipValue::Bits(value)) => {
            heap.record_set_bool(record, index, value != 0)
        }
        (crate::runtime_host::JitZipValueKind::Char, JitZipValue::Bits(value)) => heap
            .record_set_char(record, index, char::from_u32(value as u32).unwrap_or('\0')),
        (crate::runtime_host::JitZipValueKind::String, JitZipValue::Bits(value)) => {
            heap.record_set_string(record, index, value)
        }
        (crate::runtime_host::JitZipValueKind::Float, JitZipValue::Bits(value)) => heap
            .record_set_float(record, index, f64::from_bits(value as u64)),
        (_, JitZipValue::Absent) => None,
        (_, _) => None,
    }
}

fn jet_jit_iter_zip_family(
    plan_id: i64,
    column_handles: i64,
    common_fill: i64,
    column_fills: i64,
) -> i64 {
    let out = zip_family_rows(plan_id, column_handles, common_fill, column_fills);
    // The sidecar reads list lengths itself, so it runs outside the runtime
    // borrow the row builder holds.
    let columns = Concurrency::with_runtime_mut(|rt| {
        rt.heap.clone_int_list(column_handles).unwrap_or_default()
    });
    crate::IO::progress_transfer_zip_state(&columns, out);
    out
}

fn zip_family_rows(
    plan_id: i64,
    column_handles: i64,
    common_fill: i64,
    column_fills: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let out = rt.heap.alloc_empty_list();
        let Some(plan_id) = usize::try_from(plan_id).ok() else {
            jet_foundation::ice!(None, "zip plan missing");
        };
        let Some(plan) = rt.zip_plans.get(plan_id).cloned() else {
            jet_foundation::ice!(None, "zip plan missing");
        };
        let Some(handles) = rt.heap.clone_int_list(column_handles) else {
            jet_foundation::ice!(None, "zip columns handle invalid");
        };
        if handles.len() != plan.columns.len() {
            jet_foundation::ice!(None, "zip columns arity mismatch");
        }
        let Some(lengths) = handles
            .iter()
            .map(|handle| rt.heap.list_len(*handle))
            .collect::<Option<Vec<_>>>()
            .map(|lengths| {
                lengths
                    .into_iter()
                    .map(|length| usize::try_from(length).unwrap_or(0))
                    .collect::<Vec<_>>()
            })
        else {
            jet_foundation::ice!(None, "zip column handle invalid");
        };
        let common_fills = if plan.fill_mode == 1 {
            plan.columns
                .iter()
                .map(|column| jit_zip_common_fill_value(column.field, common_fill))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let column_fills = if plan.fill_mode == 2 {
            plan.columns
                .iter()
                .enumerate()
                .map(|(index, column)| {
                    jit_zip_record_field(&mut rt.heap, column.field, column_fills, index)
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let Some(rows) = collection_semantics::zip_rows(
            &lengths,
            plan.mode,
            |column, index| {
                jit_zip_read_value(&rt.heap, handles[column], plan.columns[column].input, index)
            },
            |column| {
                collection_semantics::zip_fill_at(
                    plan.fill_mode,
                    &common_fills,
                    &column_fills,
                    JitZipValue::Absent,
                    column,
                )
            },
        ) else {
            rt.set_runtime_stop(
                "E3001",
                0,
                collection_semantics::zip_length_mismatch_message(),
            );
            return out;
        };
        for values in rows {
            let record = rt.heap.alloc_record(plan.columns.len());
            for (column_index, (column, value)) in plan.columns.iter().zip(values).enumerate() {
                if jit_zip_set_value(&mut rt.heap, record, column_index, *column, value).is_none() {
                    jet_foundation::ice!(None, "zip value representation mismatch");
                }
            }
            let _ = rt.heap.list_push_int(out, record);
        }
        out
    })
}

/// Push one already-read column word back into a list in its own kind.
///
/// The write half of `jit_zip_read_value`: every kind except `Float` lives as
/// one `i64` word in a list slot (a `String` slot holds the string handle), and
/// `Float` needs `list_push_float` because a list slot is a `JetVal::Float`,
/// not the bit pattern.
fn jit_zip_push_column_value(
    heap: &mut jet_rt::JetArena,
    list: i64,
    value: JitZipValue,
) -> Option<()> {
    match value {
        JitZipValue::Float(value) => heap.list_push_float(list, value),
        JitZipValue::Bits(value) => heap.list_push_int(list, value),
        JitZipValue::Absent => None,
    }
}

/// `[(A, B)].unzip()` — two columns, each read and republished in its OWN kind.
///
/// The kinds arrive as immediates (`JitZipValueKind::code`) because lowering
/// already knows the row's field types and an unzip has no fill mode to plan.
/// This used to be an i64-pair host: it read both fields with `record_get_int`,
/// so a `String` column read `None`, the whole row fell out of a `filter_map`,
/// and the answer was two EMPTY lists rather than a stop. The split semantics
/// still belong to the Prelude kernel (I9); only the representation is here.
fn jet_jit_list_unzip(pairs: i64, left_kind: i64, right_kind: i64) -> i64 {
    let (Some(left_kind), Some(right_kind)) = (
        crate::runtime_host::JitZipValueKind::from_code(left_kind),
        crate::runtime_host::JitZipValueKind::from_code(right_kind),
    ) else {
        jet_foundation::ice!(None, "unzip column kind code out of range");
    };
    Concurrency::with_runtime_mut(|rt| {
        jit_list_unzip_columns(&mut rt.heap, pairs, left_kind, right_kind)
    })
}

/// The arena half of `jet_jit_list_unzip`, split from the runtime borrow so it
/// can be pinned without a resident runtime.
///
/// #2091: the gate arm and the lowering now read ONE table
/// (`LowerCtx::unzip_column_kinds`), so the last pairing under `.unzip()` that
/// can still drift is the per-kind representation pair inside this file — the
/// writer `jit_zip_set_value` against the readers `jit_zip_record_field` and
/// `jit_zip_read_value`. That pair is exactly what shipped two EMPTY lists, and
/// it is a pure set over `JitZipValueKind`, so `unzip_column_round_trip` below
/// checks it instead of leaving a stem as the only witness.
fn jit_list_unzip_columns(
    heap: &mut jet_rt::JetArena,
    pairs: i64,
    left_kind: crate::runtime_host::JitZipValueKind,
    right_kind: crate::runtime_host::JitZipValueKind,
) -> i64 {
    // A stop, not `unwrap_or_default()`: an unzip receiver that is not a
    // row-handle list used to answer two EMPTY lists here, which is the same
    // silent wrong answer the `record_get_int` column read gave.
    let Some(rows) = heap.clone_int_list(pairs) else {
        jet_foundation::ice!(None, "unzip receiver is not a row-handle list");
    };
    let mut columns = Vec::with_capacity(rows.len());
    for row in rows {
        let left = jit_zip_record_field(heap, left_kind, row, 0);
        let right = jit_zip_record_field(heap, right_kind, row, 1);
        if matches!(left, JitZipValue::Absent) || matches!(right, JitZipValue::Absent) {
            jet_foundation::ice!(None, "unzip row representation mismatch");
        }
        columns.push((left, right));
    }
    let (left_values, right_values) = collection_semantics::list_unzip_pairs(columns);
    let left = heap.alloc_empty_list();
    for value in left_values {
        if jit_zip_push_column_value(heap, left, value).is_none() {
            jet_foundation::ice!(None, "unzip column representation mismatch");
        }
    }
    let right = heap.alloc_empty_list();
    for value in right_values {
        if jit_zip_push_column_value(heap, right, value).is_none() {
            jet_foundation::ice!(None, "unzip column representation mismatch");
        }
    }
    let result = heap.alloc_record(2);
    let _ = heap.record_set_int(result, 0, left);
    let _ = heap.record_set_int(result, 1, right);
    result
}

/// The one pairing `.unzip()` still has on both sides of a representation
/// boundary: `jit_zip_set_value` writes a column word, `jit_zip_record_field`
/// reads it back out of the row, and `jit_zip_read_value` reads it back out of
/// the republished column. Total over `JitZipValueKind`, so it is checkable as
/// a set — unlike the `TBuiltinOp` lowering halves, which need a live
/// `FunctionBuilder` (#2091).
#[cfg(test)]
mod unzip_column_round_trip {
    use super::{
        jit_list_unzip_columns, jit_zip_pack_value, jit_zip_read_value, jit_zip_set_value,
        JitZipValue,
    };
    use crate::runtime_host::{JitZipColumn, JitZipValueKind};

    /// No `_` arm: a new column kind must fail to compile here rather than
    /// quietly skip the round trip.
    fn kind_name(kind: JitZipValueKind) -> &'static str {
        match kind {
            JitZipValueKind::Int => "Int",
            JitZipValueKind::Float => "Float",
            JitZipValueKind::Bool => "Bool",
            JitZipValueKind::Char => "Char",
            JitZipValueKind::String => "String",
            JitZipValueKind::Opaque => "Opaque",
        }
    }

    const EVERY_KIND: [JitZipValueKind; 6] = [
        JitZipValueKind::Int,
        JitZipValueKind::Float,
        JitZipValueKind::Bool,
        JitZipValueKind::Char,
        JitZipValueKind::String,
        JitZipValueKind::Opaque,
    ];

    /// One column word per kind, in the encoding that kind's writer expects.
    fn sample(heap: &mut jet_rt::JetArena, kind: JitZipValueKind, nth: i64) -> JitZipValue {
        match kind {
            JitZipValueKind::Float => JitZipValue::Float(0.5 + nth as f64),
            JitZipValueKind::Bool => JitZipValue::Bits(nth % 2),
            JitZipValueKind::Char => JitZipValue::Bits(i64::from(u32::from('a')) + nth),
            JitZipValueKind::String => JitZipValue::Bits(heap.alloc_string(format!("col{nth}"))),
            JitZipValueKind::Int | JitZipValueKind::Opaque => JitZipValue::Bits(11 * nth + 3),
        }
    }

    /// Compare by VALUE, never by word: `record_get_string` republishes a fresh
    /// handle, so two equal strings hold different handles.
    fn rendered(heap: &jet_rt::JetArena, kind: JitZipValueKind, value: JitZipValue) -> String {
        match (kind, value) {
            (JitZipValueKind::String, JitZipValue::Bits(handle)) => heap
                .clone_string(handle)
                .unwrap_or_else(|| format!("<handle {handle} is not a string>")),
            (_, JitZipValue::Bits(bits)) => bits.to_string(),
            (_, JitZipValue::Float(value)) => format!("f{:016x}", value.to_bits()),
            (_, JitZipValue::Absent) => "<absent>".to_string(),
        }
    }

    fn round_trip(kind: JitZipValueKind, optional: bool, rows: i64) {
        let mut heap = jet_rt::JetArena::default();
        let pairs = heap.alloc_empty_list();
        let left_column = JitZipColumn {
            input: kind,
            field: kind,
            optional,
        };
        let right_column = JitZipColumn {
            input: JitZipValueKind::Int,
            field: JitZipValueKind::Int,
            optional: false,
        };
        // `LowerCtx::unzip_column_kinds` maps an OPTIONAL field to `Opaque`
        // because the optional writer stores the packed presence word through
        // `record_set_int` whatever the payload kind is.
        let left_read = if optional {
            JitZipValueKind::Opaque
        } else {
            kind
        };
        let mut expected_left = Vec::new();
        let mut expected_right = Vec::new();
        for nth in 0..rows {
            let word = sample(&mut heap, kind, nth);
            let record = heap.alloc_record(2);
            assert!(
                jit_zip_set_value(&mut heap, record, 0, left_column, word).is_some(),
                "`{}` column word could not be written at all (optional={optional})",
                kind_name(kind)
            );
            let right_word = JitZipValue::Bits(1_000 + nth);
            assert!(
                jit_zip_set_value(&mut heap, record, 1, right_column, right_word).is_some(),
                "Int companion column word could not be written at all"
            );
            let stored = if optional {
                JitZipValue::Bits(jit_zip_pack_value(word))
            } else {
                word
            };
            expected_left.push(rendered(&heap, left_read, stored));
            expected_right.push(rendered(&heap, JitZipValueKind::Int, right_word));
            assert!(heap.list_push_int(pairs, record).is_some());
        }

        let out = jit_list_unzip_columns(&mut heap, pairs, left_read, JitZipValueKind::Int);
        let left_list = heap
            .record_get_int(out, 0)
            .expect("unzip published no left column handle");
        let right_list = heap
            .record_get_int(out, 1)
            .expect("unzip published no right column handle");
        assert_eq!(
            heap.list_len(left_list),
            Some(rows),
            "`{}` left column lost rows (optional={optional}) — the empty-list \
             answer is exactly the shipped bug",
            kind_name(kind)
        );
        assert_eq!(
            heap.list_len(right_list),
            Some(rows),
            "Int right column lost rows next to a `{}` left column \
             (optional={optional})",
            kind_name(kind)
        );
        for nth in 0..rows {
            let index = nth as usize;
            let left = jit_zip_read_value(&heap, left_list, left_read, index)
                .unwrap_or(JitZipValue::Absent);
            assert_eq!(
                rendered(&heap, left_read, left),
                expected_left[index],
                "`{}` column word {nth} did not survive the row it was written \
                 into (optional={optional})",
                kind_name(kind)
            );
            let right = jit_zip_read_value(&heap, right_list, JitZipValueKind::Int, index)
                .unwrap_or(JitZipValue::Absent);
            assert_eq!(
                rendered(&heap, JitZipValueKind::Int, right),
                expected_right[index],
                "Int column word {nth} did not survive next to a `{}` column \
                 (optional={optional})",
                kind_name(kind)
            );
        }
    }

    #[test]
    fn every_unzip_column_kind_survives_the_row_it_was_written_into() {
        for kind in EVERY_KIND {
            for rows in [0, 1, 3] {
                round_trip(kind, false, rows);
                round_trip(kind, true, rows);
            }
        }
    }

    /// The distinct case, which is the whole reason `Opaque` may not assert a
    /// representation: `unzip_column_kinds` answers for the gate too, so it
    /// reads the distinct-blind `clif_ty` and calls a `distinct Name = String`
    /// column `Opaque`, while the row writer stored the cell the AWARE erasure
    /// chose. Reading that cell must work for every shape the writer can
    /// produce, or a legal receiver the gate admitted stops on an ICE.
    #[test]
    fn an_opaque_column_reads_every_cell_shape_the_writer_can_produce() {
        for kind in EVERY_KIND {
            let mut heap = jet_rt::JetArena::default();
            let pairs = heap.alloc_empty_list();
            let written = JitZipColumn {
                input: kind,
                field: kind,
                optional: false,
            };
            let int_column = JitZipColumn {
                input: JitZipValueKind::Int,
                field: JitZipValueKind::Int,
                optional: false,
            };
            let word = sample(&mut heap, kind, 1);
            let record = heap.alloc_record(2);
            assert!(jit_zip_set_value(&mut heap, record, 0, written, word).is_some());
            assert!(
                jit_zip_set_value(&mut heap, record, 1, int_column, JitZipValue::Bits(5)).is_some()
            );
            assert!(heap.list_push_int(pairs, record).is_some());
            // The cell keeps its own shape, so compare against the shape the
            // writer chose, not against `Opaque`.
            let expected = rendered(&heap, kind, word);

            let out = jit_list_unzip_columns(
                &mut heap,
                pairs,
                JitZipValueKind::Opaque,
                JitZipValueKind::Int,
            );
            let left_list = heap
                .record_get_int(out, 0)
                .expect("unzip published no left column handle");
            assert_eq!(
                heap.list_len(left_list),
                Some(1),
                "an `Opaque` read of a `{}` cell dropped the row",
                kind_name(kind)
            );
            let left = jit_zip_read_value(&heap, left_list, kind, 0).unwrap_or(JitZipValue::Absent);
            assert_eq!(
                rendered(&heap, kind, left),
                expected,
                "an `Opaque` read of a `{}` cell did not republish it in that \
                 cell's own shape",
                kind_name(kind)
            );
        }
    }

    /// The stem shape: `["a", "bb", "c"].zip([1, 2, 3]).unzip()`. This answered
    /// two EMPTY lists, and the residency gate was the only thing standing
    /// between that and a user (#2091).
    #[test]
    fn a_string_int_pair_unzips_into_a_string_column_and_an_int_column() {
        let mut heap = jet_rt::JetArena::default();
        let pairs = heap.alloc_empty_list();
        let string_column = JitZipColumn {
            input: JitZipValueKind::String,
            field: JitZipValueKind::String,
            optional: false,
        };
        let int_column = JitZipColumn {
            input: JitZipValueKind::Int,
            field: JitZipValueKind::Int,
            optional: false,
        };
        for (text, number) in [("a", 1), ("bb", 2), ("c", 3)] {
            let text = heap.alloc_string(text);
            let record = heap.alloc_record(2);
            assert!(
                jit_zip_set_value(&mut heap, record, 0, string_column, JitZipValue::Bits(text))
                    .is_some()
            );
            assert!(
                jit_zip_set_value(&mut heap, record, 1, int_column, JitZipValue::Bits(number))
                    .is_some()
            );
            assert!(heap.list_push_int(pairs, record).is_some());
        }

        let out = jit_list_unzip_columns(
            &mut heap,
            pairs,
            JitZipValueKind::String,
            JitZipValueKind::Int,
        );
        let left = heap.record_get_int(out, 0).expect("left column handle");
        let right = heap.record_get_int(out, 1).expect("right column handle");
        let strings: Vec<String> = (0..3)
            .map(|index| {
                heap.list_get_string(left, index)
                    .unwrap_or_else(|| "<lost>".to_string())
            })
            .collect();
        let numbers: Vec<i64> = (0..3)
            .map(|index| heap.list_get_int(right, index).unwrap_or(-1))
            .collect();
        assert_eq!(
            strings,
            vec!["a".to_string(), "bb".to_string(), "c".to_string()],
            "the String column came back wrong — two empty lists is the shape \
             the `record_get_int` read produced"
        );
        assert_eq!(numbers, vec![1, 2, 3], "the Int column came back wrong");
    }

    /// An optional column carries the packed presence word, so `none` must
    /// republish as the zero word and `some` as payload + 1.
    #[test]
    fn an_optional_column_republishes_its_packed_presence_word() {
        let mut heap = jet_rt::JetArena::default();
        let pairs = heap.alloc_empty_list();
        let optional_string = JitZipColumn {
            input: JitZipValueKind::String,
            field: JitZipValueKind::String,
            optional: true,
        };
        let int_column = JitZipColumn {
            input: JitZipValueKind::Int,
            field: JitZipValueKind::Int,
            optional: false,
        };
        let present = heap.alloc_string("here");
        for word in [JitZipValue::Absent, JitZipValue::Bits(present)] {
            let record = heap.alloc_record(2);
            assert!(jit_zip_set_value(&mut heap, record, 0, optional_string, word).is_some());
            assert!(
                jit_zip_set_value(&mut heap, record, 1, int_column, JitZipValue::Bits(9)).is_some()
            );
            assert!(heap.list_push_int(pairs, record).is_some());
        }

        let out = jit_list_unzip_columns(
            &mut heap,
            pairs,
            JitZipValueKind::Opaque,
            JitZipValueKind::Int,
        );
        let left = heap.record_get_int(out, 0).expect("left column handle");
        assert_eq!(
            heap.list_get_int(left, 0),
            Some(0),
            "`none` lost its zero presence word, so the column reads as \
             `some(handle - 1)`"
        );
        assert_eq!(
            heap.list_get_int(left, 1),
            Some(present.wrapping_add(1)),
            "a present optional column word lost its +1 packing"
        );
    }
}

fn jet_jit_list_starts_with(list: i64, prefix: i64) -> i8 {
    collection_semantics::list_starts_with(&clone_list_ints(list), &clone_list_ints(prefix)) as i8
}

fn jet_jit_list_ends_with(list: i64, suffix: i64) -> i8 {
    collection_semantics::list_ends_with(&clone_list_ints(list), &clone_list_ints(suffix)) as i8
}

fn jet_jit_iter_repeat(list: i64, n: i64) -> i64 {
    alloc_from_ints(&collection_semantics::iter_repeat(clone_list_ints(list), n))
}

fn jet_jit_iter_cycle(list: i64, n: i64) -> i64 {
    alloc_from_ints(&collection_semantics::iter_cycle(clone_list_ints(list), n))
}

fn jet_jit_iter_drop_last(list: i64, n: i64) -> i64 {
    alloc_from_ints(&collection_semantics::iter_drop_last(clone_list_ints(list), n))
}

fn jet_jit_iter_shuffle(list: i64) -> i64 {
    alloc_from_ints(&collection_semantics::iter_shuffle(clone_list_ints(list)))
}

fn jet_jit_iter_is_sorted(list: i64) -> i8 {
    collection_semantics::iter_is_sorted(clone_list_ints(list)) as i8
}

fn jet_jit_iter_last_index_of(list: i64, needle: i64) -> i64 {
    let value = collection_semantics::iter_last_index_of(clone_list_ints(list), needle).ok();
    Concurrency::with_runtime_mut(|rt| option_i64(rt, value))
}

fn jet_jit_iter_average_int(list: i64) -> f64 {
    collection_semantics::iter_average_int(clone_list_ints(list))
}

fn jet_jit_iter_average_float(list: i64) -> f64 {
    let values = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        (0..len)
            .map(|index| rt.heap.list_get_float(list, index).unwrap_or_default())
            .collect::<Vec<_>>()
    });
    collection_semantics::iter_average_float(values)
}

fn jet_jit_iter_compare(list: i64, other: i64) -> i64 {
    collection_semantics::iter_compare(clone_list_ints(list), clone_list_ints(other))
}

fn jet_jit_iter_split(list: i64, n: i64) -> i64 {
    let (left, right) = collection_semantics::iter_split_i64(clone_list_ints(list), n);
    Concurrency::with_runtime_mut(|rt| {
        let pair = rt.heap.alloc_record(2);
        let left = {
            let handle = rt.heap.alloc_empty_list();
            for value in left {
                let _ = rt.heap.list_push_int(handle, value);
            }
            handle
        };
        let right = {
            let handle = rt.heap.alloc_empty_list();
            for value in right {
                let _ = rt.heap.list_push_int(handle, value);
            }
            handle
        };
        let _ = rt.heap.record_set_int(pair, 0, left);
        let _ = rt.heap.record_set_int(pair, 1, right);
        pair
    })
}

fn jet_jit_list_equal(left: i64, right: i64) -> i8 {
    collection_semantics::list_equal(&clone_list_ints(left), &clone_list_ints(right)) as i8
}

fn jet_jit_list_binary_search(list: i64, needle: i64) -> i64 {
    let value = collection_semantics::list_binary_search(&clone_list_ints(list), &needle).ok();
    Concurrency::with_runtime_mut(|rt| option_i64(rt, value))
}

fn jet_jit_list_union(left: i64, right: i64) -> i64 {
    alloc_from_ints(&collection_semantics::list_union(
        &clone_list_ints(left),
        &clone_list_ints(right),
    ))
}

fn jet_jit_list_intersection(left: i64, right: i64) -> i64 {
    alloc_from_ints(&collection_semantics::list_intersection(
        &clone_list_ints(left),
        &clone_list_ints(right),
    ))
}

fn jet_jit_list_difference(left: i64, right: i64) -> i64 {
    alloc_from_ints(&collection_semantics::list_difference(
        &clone_list_ints(left),
        &clone_list_ints(right),
    ))
}

fn jet_jit_list_random(list: i64) -> i64 {
    let value = collection_semantics::list_random(&clone_list_ints(list)).ok();
    Concurrency::with_runtime_mut(|rt| option_i64(rt, value))
}

fn jet_jit_list_min_max(list: i64) -> i64 {
    let values = clone_list_ints(list);
    let Some((min, max)) = collection_semantics::list_min_max_i64(&values).ok() else {
        return 0;
    };
    Concurrency::with_runtime_mut(|rt| {
        let pair = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(pair, 0, min);
        let _ = rt.heap.record_set_int(pair, 1, max);
        pair + 1
    })
}

fn jet_jit_list_replace(list: i64, index: i64, new: i64) -> i64 {
    alloc_from_ints(&collection_semantics::list_replace(
        &clone_list_ints(list),
        index,
        new,
    ))
}

/// Stable sort `list` in place by parallel i64 `keys` (same length).
fn jet_jit_list_sort_by_i64_keys(list: i64, keys: i64) {
    jet_jit_list_sort_by_i64_keys_impl(list, keys, false);
}

fn jet_jit_list_sort_by_i64_keys_desc(list: i64, keys: i64) {
    jet_jit_list_sort_by_i64_keys_impl(list, keys, true);
}

fn jet_jit_list_sort_by_i64_keys_impl(list: i64, keys: i64, descending: bool) {
    Concurrency::with_runtime_mut(|rt| {
        let xs = rt
            .heap
            .clone_int_list(list)
            .expect("jit sort_by: bad list handle");
        let keys = rt
            .heap
            .clone_int_list(keys)
            .expect("jit sort_by: bad keys handle");
        debug_assert_eq!(xs.len(), keys.len());
        let mut order: Vec<usize> = (0..xs.len()).collect();
        order.sort_by(|&a, &b| {
            if descending {
                keys[b].cmp(&keys[a])
            } else {
                keys[a].cmp(&keys[b])
            }
        });
        for (dst, src) in order.into_iter().enumerate() {
            rt.heap
                .list_set_int(list, dst as i64, xs[src])
                .expect("jit sort_by: set");
        }
    });
}

/// Stable sort `list` by parallel string-handle keys (Jet `String` heap ids).
fn jet_jit_list_sort_by_str_keys(list: i64, keys: i64) {
    jet_jit_list_sort_by_str_keys_impl(list, keys, false);
}

fn jet_jit_list_sort_by_str_keys_desc(list: i64, keys: i64) {
    jet_jit_list_sort_by_str_keys_impl(list, keys, true);
}

fn jet_jit_list_sort_by_str_keys_impl(list: i64, keys: i64, descending: bool) {
    Concurrency::with_runtime_mut(|rt| {
        let xs = rt
            .heap
            .clone_int_list(list)
            .expect("jit sort_by_str: bad list handle");
        let key_ids = rt
            .heap
            .clone_int_list(keys)
            .expect("jit sort_by_str: bad keys handle");
        debug_assert_eq!(xs.len(), key_ids.len());
        let key_strs: Vec<String> = key_ids
            .iter()
            .map(|id| rt.heap.clone_string(*id).unwrap_or_default())
            .collect();
        let mut order: Vec<usize> = (0..xs.len()).collect();
        order.sort_by(|&a, &b| {
            if descending {
                key_strs[b].cmp(&key_strs[a])
            } else {
                key_strs[a].cmp(&key_strs[b])
            }
        });
        for (dst, src) in order.into_iter().enumerate() {
            rt.heap
                .list_set_int(list, dst as i64, xs[src])
                .expect("jit sort_by_str: set");
        }
    });
}

fn list_text(rt: &crate::JitRuntime, list: i64, kind: i64, debug: bool) -> String {
    if !debug {
        let values = || rt.heap.clone_int_list(list).unwrap_or_default();
        match kind {
            5 => {
                return format!(
                    "[{}]",
                    values()
                        .into_iter()
                        .map(|handle| crate::Net::show_value(rt, handle))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            6 => {
                return collection_semantics::show_bool_list(
                    values().into_iter().map(|value| value != 0).collect(),
                );
            }
            7 => {
                return collection_semantics::show_char_list(
                    values()
                        .into_iter()
                        .map(|value| char::from_u32(value as u32).unwrap_or('?'))
                        .collect(),
                );
            }
            8 => {
                return format!(
                    "[{}]",
                    values()
                        .into_iter()
                        .map(|handle| crate::CoreHost::show_path(rt, handle))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            9 => {
                return format!(
                    "[{}]",
                    values()
                        .into_iter()
                        .map(|handle| crate::Time::show_value(rt, handle))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            _ => {}
        }
    }
    match kind {
        1 => {
            let values = rt
                .heap
                .clone_int_list(list)
                .unwrap_or_default()
                .into_iter()
                .map(|id| rt.heap.clone_string(id).unwrap_or_default())
                .collect::<Vec<_>>();
            if debug {
                collection_semantics::debug_string_list(values)
            } else {
                collection_semantics::show_string_list(values)
            }
        }
        2 => {
            let values = rt.heap.clone_int_list(list).unwrap_or_default();
            if debug {
                collection_semantics::debug_i64_list(values)
            } else {
                collection_semantics::show_i64_list(values)
            }
        }
        3 => {
            let values = rt
                .heap
                .clone_int_list(list)
                .unwrap_or_default()
                .into_iter()
                .map(|value| value as u64)
                .collect::<Vec<_>>();
            if debug {
                collection_semantics::debug_u64_list(values)
            } else {
                collection_semantics::show_u64_list(values)
            }
        }
        4 => {
            let len = rt.heap.list_len(list).unwrap_or(0);
            let values = (0..len)
                .map(|index| rt.heap.list_get_float(list, index).unwrap_or(0.0))
                .collect::<Vec<_>>();
            if debug {
                collection_semantics::debug_f64_list(values)
            } else {
                collection_semantics::show_f64_list(values)
            }
        }
        7 => {
            let len = rt.heap.list_len(list).unwrap_or(0);
            let values = (0..len)
                .map(|index| rt.heap.list_get_float(list, index).unwrap_or(0.0) as f32)
                .collect::<Vec<_>>();
            if debug {
                collection_semantics::debug_f32_list(values)
            } else {
                collection_semantics::show_f32_list(values)
            }
        }
        5 => {
            let values = rt
                .heap
                .clone_int_list(list)
                .unwrap_or_default()
                .into_iter()
                .map(|value| value != 0)
                .collect::<Vec<_>>();
            if debug {
                collection_semantics::debug_bool_list(values)
            } else {
                collection_semantics::show_bool_list(values)
            }
        }
        6 => {
            let values = rt
                .heap
                .clone_int_list(list)
                .unwrap_or_default()
                .into_iter()
                .map(|value| char::from_u32(value as u32).unwrap_or('?'))
                .collect::<Vec<_>>();
            if debug {
                collection_semantics::debug_char_list(values)
            } else {
                collection_semantics::show_char_list(values)
            }
        }
        _ => {
            let values = rt.heap.clone_int_list(list).unwrap_or_default();
            if debug {
                collection_semantics::debug_i64_list(values)
            } else {
                collection_semantics::show_i64_list(values)
            }
        }
    }
}

fn list_debug_text(rt: &crate::JitRuntime, list: i64, kind: i64) -> String {
    list_text(rt, list, kind, true)
}

/// Jet Debug list text as a string handle for structural-value marshalling.
fn jet_jit_list_debug(list: i64, kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = list_debug_text(rt, list, kind);
        rt.heap.alloc_string(text)
    })
}

/// String Debug text as a handle for structural-value marshalling.
fn jet_jit_string_debug(value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(value).unwrap_or_default();
        rt.heap.alloc_string(collection_semantics::debug_string(text))
    })
}

/// Marshal a resident scalar into the shared Prelude Debug implementation.
/// `kind`: 0 = Int, 1 = Float, 2 = Bool, 3 = Char, 4 = F32. Floats arrive as bits.
fn jet_jit_scalar_debug(value: i64, kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = match kind {
            0 => collection_semantics::debug_i64(value),
            1 => collection_semantics::debug_f64(f64::from_bits(value as u64)),
            2 => collection_semantics::debug_bool(value != 0),
            3 => collection_semantics::debug_char(char::from_u32(value as u32).unwrap_or('?')),
            4 => collection_semantics::debug_f32(f32::from_bits(value as u32)),
            _ => String::new(),
        };
        rt.heap.alloc_string(text)
    })
}

/// Append `T?` Debug text through the shared StructuralDebug formatter.
fn jet_jit_str_push_debug_optional(
    buf_id: i64,
    payload_id: i64,
    present: i64,
) {
    Concurrency::with_runtime_mut(|rt| {
        let payload = (present != 0)
            .then(|| rt.heap.clone_string(payload_id).unwrap_or_default());
        let text = jet_foundation::StructuralDebug::jet_debug_optional(payload);
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            buf.push_str(&text);
        }
    });
}

/// Append a record after the JIT has marshalled rendered values and metadata.
fn jet_jit_str_push_debug_record(
    buf_id: i64,
    type_name_id: i64,
    fields_id: i64,
) {
    Concurrency::with_runtime_mut(|rt| {
        let Some(type_name) = rt.heap.clone_string(type_name_id) else {
            return;
        };
        let fields = rt.heap.clone_int_list(fields_id).unwrap_or_default();
        if fields.len() % 4 != 0 {
            return;
        }
        let mut marshalled = Vec::with_capacity(fields.len() / 4);
        for field in fields.chunks_exact(4) {
            let Some(name) = rt.heap.clone_string(field[0]) else {
                return;
            };
            let Some(value) = rt.heap.clone_string(field[1]) else {
                return;
            };
            if field[2] < 0 {
                return;
            }
            marshalled.push(jet_foundation::StructuralDebug::JetDebugField {
                name,
                value,
                storage_index: field[2] as usize,
                redacted: field[3] != 0,
            });
        }
        let text = jet_foundation::StructuralDebug::jet_debug_record_fields(
            &type_name,
            marshalled,
        );
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            buf.push_str(&text);
        }
    });
}

/// Append a positional variant through the shared StructuralDebug formatter.
fn jet_jit_str_push_debug_variant(
    buf_id: i64,
    variant_id: i64,
    payload_id: i64,
    has_payload: i64,
) {
    Concurrency::with_runtime_mut(|rt| {
        let Some(variant) = rt.heap.clone_string(variant_id) else {
            return;
        };
        let payload = (has_payload != 0)
            .then(|| rt.heap.clone_string(payload_id).unwrap_or_default());
        let text = jet_foundation::StructuralDebug::jet_debug_variant(&variant, payload);
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            buf.push_str(&text);
        }
    });
}

/// `list.pop()` — Option ABI: `0` = None, `value + 1` = Some (i64/handle elems).
fn jet_jit_list_pop(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let values: &mut Vec<jet_rt::JetVal> =
            unsafe { &mut *(&mut rt.heap as *mut jet_rt::JetArena as *mut Vec<jet_rt::JetVal>) };
        let Some(jet_rt::JetVal::List(xs)) = values.get_mut(list as usize) else {
            jet_foundation::ice!(None, "jit list pop: bad handle");
        };
        match collection_semantics::list_pop(xs) {
            Some(jet_rt::JetVal::Int(v)) => v.wrapping_add(1),
            Some(jet_rt::JetVal::Float(v)) => (v.to_bits() as i64).wrapping_add(1),
            Some(_) | None => 0,
        }
    })
}

/// `list.insert(i, v)` — AOT `Vec::insert`; OOB traps like remove.
///
/// # ponytail: same JetArena layout poke as `list_remove`.
fn jet_jit_list_insert(list: i64, idx: i64, v: i64) {
    Concurrency::with_runtime_mut(|rt| {
        // SAFETY: JetArena is `{ values: Vec<JetVal> }` — one field, identical address.
        let values: &mut Vec<jet_rt::JetVal> =
            unsafe { &mut *(&mut rt.heap as *mut jet_rt::JetArena as *mut Vec<jet_rt::JetVal>) };
        let Some(jet_rt::JetVal::List(xs)) = values.get_mut(list as usize) else {
            jet_foundation::ice!(None, "jit list insert: bad handle");
        };
        let len = xs.len() as i64;
        if idx < 0 || idx > len {
            rt.set_runtime_stop(
                "E3010",
                0,
                &jet_foundation::Outcome::jet_list_bounds_message(len, idx),
            );
            return;
        }
        xs.insert(idx as usize, jet_rt::JetVal::Int(v));
    });
}

/// `list.remove(i, RemoveBy.Slot)` — shared Prelude bounds semantics, with the
/// JIT only marshalling the list handle and runtime-stop boundary.
fn jet_jit_list_remove_slot(list: i64, idx: i64, line: u32) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        // SAFETY: JetArena is `{ values: Vec<JetVal> }` — one field, identical address.
        let values: &mut Vec<jet_rt::JetVal> =
            unsafe { &mut *(&mut rt.heap as *mut jet_rt::JetArena as *mut Vec<jet_rt::JetVal>) };
        let Some(jet_rt::JetVal::List(xs)) = values.get_mut(list as usize) else {
            jet_foundation::ice!(None, "jit list remove slot: bad handle");
        };
        match collection_semantics::list_remove_slot(xs, idx) {
            Ok(jet_rt::JetVal::Int(removed)) => option_packed(Some(removed)),
            Ok(_) => 0,
            Err(message) => {
                rt.set_runtime_stop("E3010", line, &message);
                0
            }
        }
    })
}

fn set_handle(rt: &mut crate::JitRuntime, set: HashSet<i64>, string_kind: bool) -> i64 {
    rt.sets.push(set);
    rt.set_string_kinds.push(string_kind);
    rt.sets.len() as i64
}

fn sorted_set_handle(rt: &mut crate::JitRuntime, set: BTreeSet<i64>, string_kind: bool) -> i64 {
    rt.sorted_sets.push(set);
    rt.sorted_set_string_kinds.push(string_kind);
    rt.sorted_sets.len() as i64
}

fn set_is_string(rt: &crate::JitRuntime, handle: i64) -> bool {
    rt.set_string_kinds
        .get((handle as usize).wrapping_sub(1))
        .copied()
        .unwrap_or(false)
}

fn sorted_set_is_string(rt: &crate::JitRuntime, handle: i64) -> bool {
    rt.sorted_set_string_kinds
        .get((handle as usize).wrapping_sub(1))
        .copied()
        .unwrap_or(false)
}

fn string_ids(rt: &crate::JitRuntime, values: &HashSet<i64>) -> HashMap<String, i64> {
    values
        .iter()
        .filter_map(|id| rt.heap.clone_string(*id).map(|value| (value, *id)))
        .collect()
}

fn sorted_string_ids(rt: &crate::JitRuntime, values: &BTreeSet<i64>) -> HashMap<String, i64> {
    values
        .iter()
        .filter_map(|id| rt.heap.clone_string(*id).map(|value| (value, *id)))
        .collect()
}

fn set_string_values(rt: &crate::JitRuntime, values: &HashSet<i64>) -> HashSet<String> {
    string_ids(rt, values).into_keys().collect()
}

fn sorted_string_values(rt: &crate::JitRuntime, values: &BTreeSet<i64>) -> BTreeSet<String> {
    sorted_string_ids(rt, values).into_keys().collect()
}

fn canonical_string_set(
    rt: &crate::JitRuntime,
    values: impl IntoIterator<Item = i64>,
) -> HashSet<i64> {
    let mut ids = HashMap::new();
    for id in values {
        if let Some(value) = rt.heap.clone_string(id) {
            ids.entry(value).or_insert(id);
        }
    }
    ids.into_values().collect()
}

fn canonical_sorted_string_set(
    rt: &crate::JitRuntime,
    values: impl IntoIterator<Item = i64>,
) -> BTreeSet<i64> {
    canonical_string_set(rt, values).into_iter().collect()
}

fn deque_handle(rt: &mut crate::JitRuntime, dq: VecDeque<i64>) -> i64 {
    rt.deques.push(dq);
    rt.deques.len() as i64
}

fn jet_jit_set_from_list(list: i64, string_kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let xs = rt.heap.clone_int_list(list).unwrap_or_default();
        let string_kind = string_kind != 0;
        let set = if string_kind {
            canonical_string_set(rt, xs)
        } else {
            xs.into_iter().collect()
        };
        set_handle(rt, set, string_kind)
    })
}

// #1478: empty Set constructor + remaining non-closure surface.
fn jet_jit_set_new(string_kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| set_handle(rt, HashSet::new(), string_kind != 0))
}

fn jet_jit_set_copy(set: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (set as usize).wrapping_sub(1);
        let existing = rt.sets.get(idx).cloned().unwrap_or_default();
        let string_kind = set_is_string(rt, set);
        set_handle(rt, existing, string_kind)
    })
}

fn jet_jit_set_equal(a: i64, b: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let left = rt
            .sets
            .get((a as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let right = rt
            .sets
            .get((b as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let string_kind = set_is_string(rt, a) || set_is_string(rt, b);
        let eq = if string_kind {
            let left_values = set_string_values(rt, &left);
            let right_values = set_string_values(rt, &right);
            left_values == right_values
        } else {
            left == right
        };
        i8::from(eq)
    })
}

fn jet_jit_set_capacity(set: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.sets
            .get((set as usize).wrapping_sub(1))
            .map(|s| s.capacity() as i64)
            .unwrap_or(0)
    })
}

fn jet_jit_set_first(set: i64) -> i64 {
    // Packed Option ABI (0 = None, value+1 = Some) so `let f := set.first()`
    // then `f ?? …` works — result-arena Options break once bound to a local.
    Concurrency::with_runtime_mut(|rt| {
        let value = rt
            .sets
            .get((set as usize).wrapping_sub(1))
            .and_then(|existing| existing.iter().next().copied());
        option_packed(value)
    })
}

fn jet_jit_set_insert(set: i64, v: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (set as usize).wrapping_sub(1);
        let Some(existing) = rt.sets.get(idx).cloned() else {
            return 0;
        };
        let string_kind = set_is_string(rt, set);
        if string_kind {
            let needle = rt.heap.clone_string(v).unwrap_or_default();
            if existing.iter().any(|id| rt.heap.clone_string(*id).as_deref() == Some(needle.as_str())) {
                return 0;
            }
        }
        if rt.sets[idx].insert(v) {
            1
        } else {
            0
        }
    })
}

fn jet_jit_set_remove(set: i64, v: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (set as usize).wrapping_sub(1);
        let Some(existing) = rt.sets.get(idx).cloned() else { return; };
        let id = if set_is_string(rt, set) {
            let needle = rt.heap.clone_string(v).unwrap_or_default();
            existing.iter().find(|id| rt.heap.clone_string(**id).as_deref() == Some(needle.as_str())).copied()
        } else {
            Some(v)
        };
        if let Some(id) = id {
            rt.sets[idx].remove(&id);
        }
    });
}

fn jet_jit_set_pop(set: i64, v: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (set as usize).wrapping_sub(1);
        let Some(existing) = rt.sets.get(idx).cloned() else {
            return option_packed(None);
        };
        let string_kind = set_is_string(rt, set);
        let found = if string_kind {
            let needle = rt.heap.clone_string(v).unwrap_or_default();
            existing
                .iter()
                .find(|id| rt.heap.clone_string(**id).as_deref() == Some(needle.as_str()))
                .copied()
        } else {
            existing.contains(&v).then_some(v)
        };
        option_packed(found.and_then(|id| collection_semantics::set_pop_i64(&mut rt.sets[idx], &id)))
    })
}

fn jet_jit_set_has(set: i64, v: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        match rt.sets.get((set as usize).wrapping_sub(1)) {
            Some(s) if set_is_string(rt, set) && {
                let needle = rt.heap.clone_string(v).unwrap_or_default();
                s.iter().any(|id| rt.heap.clone_string(*id).as_deref() == Some(needle.as_str()))
            } => 1,
            Some(s) if s.contains(&v) => 1,
            _ => 0,
        }
    })
}

fn jet_jit_set_len(set: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.sets
            .get((set as usize).wrapping_sub(1))
            .map(|s| s.len() as i64)
            .unwrap_or(0)
    })
}

fn jet_jit_set_to_list(set: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let xs: Vec<i64> = rt.sets.get((set as usize).wrapping_sub(1))
            .map(|s| s.iter().copied().collect()).unwrap_or_default();
        rt.heap.alloc_int_list(xs)
    })
}

fn jet_jit_set_union(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let left = rt
            .sets
            .get((a as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let right = rt
            .sets
            .get((b as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let string_kind = set_is_string(rt, a) || set_is_string(rt, b);
        if string_kind {
            let ids = string_ids(rt, &left).into_iter().chain(string_ids(rt, &right)).collect::<HashMap<_, _>>();
            let left_values = set_string_values(rt, &left);
            let right_values = set_string_values(rt, &right);
            let out_values = set_semantics::jet_set_union(&left_values, &right_values);
            return set_handle(rt, out_values.into_iter().filter_map(|value| ids.get(&value).copied()).collect(), true);
        }
        set_handle(rt, set_semantics::jet_set_union(&left, &right), false)
    })
}

fn jet_jit_set_intersection(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let left = rt
            .sets
            .get((a as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let right = rt
            .sets
            .get((b as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let string_kind = set_is_string(rt, a) || set_is_string(rt, b);
        if string_kind {
            let ids = string_ids(rt, &left).into_iter().chain(string_ids(rt, &right)).collect::<HashMap<_, _>>();
            let out = set_semantics::jet_set_intersection(&set_string_values(rt, &left), &set_string_values(rt, &right));
            return set_handle(rt, out.into_iter().filter_map(|value| ids.get(&value).copied()).collect(), true);
        }
        set_handle(rt, set_semantics::jet_set_intersection(&left, &right), false)
    })
}

fn jet_jit_set_difference(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let left = rt
            .sets
            .get((a as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let right = rt
            .sets
            .get((b as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let string_kind = set_is_string(rt, a) || set_is_string(rt, b);
        if string_kind {
            let ids = string_ids(rt, &left);
            let out = set_semantics::jet_set_difference(&set_string_values(rt, &left), &set_string_values(rt, &right));
            return set_handle(rt, out.into_iter().filter_map(|value| ids.get(&value).copied()).collect(), true);
        }
        set_handle(rt, set_semantics::jet_set_difference(&left, &right), false)
    })
}

fn jet_jit_set_symmetric_difference(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let left = rt
            .sets
            .get((a as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let right = rt
            .sets
            .get((b as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let string_kind = set_is_string(rt, a) || set_is_string(rt, b);
        if string_kind {
            let ids = string_ids(rt, &left).into_iter().chain(string_ids(rt, &right)).collect::<HashMap<_, _>>();
            let out = set_semantics::jet_set_symmetric_difference(&set_string_values(rt, &left), &set_string_values(rt, &right));
            return set_handle(rt, out.into_iter().filter_map(|value| ids.get(&value).copied()).collect(), true);
        }
        set_handle(rt, set_semantics::jet_set_symmetric_difference(&left, &right), false)
    })
}

fn jet_jit_set_is_subset(a: i64, b: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(left) = rt.sets.get((a as usize).wrapping_sub(1)).cloned() else {
            return 1;
        };
        let Some(right) = rt.sets.get((b as usize).wrapping_sub(1)).cloned() else {
            return i8::from(left.is_empty());
        };
        if set_is_string(rt, a) || set_is_string(rt, b) {
            return i8::from(set_semantics::jet_set_is_subset(&set_string_values(rt, &left), &set_string_values(rt, &right)));
        }
        i8::from(set_semantics::jet_set_is_subset(&left, &right))
    })
}

fn jet_jit_set_is_superset(a: i64, b: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(left) = rt.sets.get((a as usize).wrapping_sub(1)).cloned() else {
            return i8::from(rt.sets.get((b as usize).wrapping_sub(1)).is_some_and(HashSet::is_empty));
        };
        let Some(right) = rt.sets.get((b as usize).wrapping_sub(1)).cloned() else {
            return 1;
        };
        if set_is_string(rt, a) || set_is_string(rt, b) {
            return i8::from(set_semantics::jet_set_is_superset(&set_string_values(rt, &left), &set_string_values(rt, &right)));
        }
        i8::from(set_semantics::jet_set_is_superset(&left, &right))
    })
}

fn jet_jit_set_is_disjoint(a: i64, b: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(left) = rt.sets.get((a as usize).wrapping_sub(1)).cloned() else {
            return 1;
        };
        let Some(right) = rt.sets.get((b as usize).wrapping_sub(1)).cloned() else {
            return 1;
        };
        if set_is_string(rt, a) || set_is_string(rt, b) {
            return i8::from(set_semantics::jet_set_is_disjoint(&set_string_values(rt, &left), &set_string_values(rt, &right)));
        }
        i8::from(set_semantics::jet_set_is_disjoint(&left, &right))
    })
}

fn jet_jit_deque_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| deque_handle(rt, VecDeque::new()))
}

fn jet_jit_deque_push_front(dq: i64, v: i64) {
    Concurrency::with_runtime_mut(|rt| {
        if let Some(d) = rt.deques.get_mut((dq as usize).wrapping_sub(1)) {
            d.push_front(v);
        }
    });
}

fn jet_jit_deque_push_back(dq: i64, v: i64) {
    Concurrency::with_runtime_mut(|rt| {
        if let Some(d) = rt.deques.get_mut((dq as usize).wrapping_sub(1)) {
            d.push_back(v);
        }
    });
}

/// Packed Option: 0 = None, else value+1 (Int elems).
fn jet_jit_deque_pop_front(dq: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match rt.deques.get_mut((dq as usize).wrapping_sub(1)).and_then(collection_semantics::deque_pop_front) {
            Some(v) => v + 1,
            None => 0,
        }
    })
}

fn jet_jit_deque_pop_back(dq: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match rt.deques.get_mut((dq as usize).wrapping_sub(1)).and_then(collection_semantics::deque_pop_back) {
            Some(v) => v + 1,
            None => 0,
        }
    })
}

fn jet_jit_deque_peek_front(dq: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match rt.deques.get((dq as usize).wrapping_sub(1)).and_then(|d| d.front().copied()) {
            Some(v) => v + 1,
            None => 0,
        }
    })
}

fn jet_jit_deque_peek_back(dq: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match rt.deques.get((dq as usize).wrapping_sub(1)).and_then(|d| d.back().copied()) {
            Some(v) => v + 1,
            None => 0,
        }
    })
}

fn jet_jit_deque_len(dq: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.deques
            .get((dq as usize).wrapping_sub(1))
            .map(|d| d.len() as i64)
            .unwrap_or(0)
    })
}

fn jet_jit_deque_capacity(dq: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.deques
            .get((dq as usize).wrapping_sub(1))
            .map(|d| d.capacity() as i64)
            .unwrap_or(0)
    })
}

fn jet_jit_deque_contains(dq: i64, v: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        i8::from(
            rt.deques
                .get((dq as usize).wrapping_sub(1))
                .is_some_and(|d| d.contains(&v)),
        )
    })
}

fn jet_jit_deque_get(dq: i64, idx: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if idx < 0 {
            return 0;
        }
        match rt
            .deques
            .get((dq as usize).wrapping_sub(1))
            .and_then(|d| d.get(idx as usize).copied())
        {
            Some(v) => v + 1,
            None => 0,
        }
    })
}

fn jet_jit_deque_delete(dq: i64, v: i64) {
    Concurrency::with_runtime_mut(|rt| {
        if let Some(d) = rt.deques.get_mut((dq as usize).wrapping_sub(1)) {
            if let Some(i) = d.iter().position(|x| *x == v) {
                d.remove(i);
            }
        }
    });
}

fn jet_jit_deque_to_list(dq: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let xs: Vec<i64> = rt
            .deques
            .get((dq as usize).wrapping_sub(1))
            .map(|d| d.iter().copied().collect())
            .unwrap_or_default();
        rt.heap.alloc_int_list(xs)
    })
}

fn jet_jit_deque_join(dq: i64, sep_id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(xs) = rt
            .deques
            .get((dq as usize).wrapping_sub(1))
            .map(|d| d.iter().copied().collect::<Vec<_>>())
        else {
            jet_foundation::ice!(None, "deque join received an invalid deque");
        };
        let Some(sep) = rt.heap.clone_string(sep_id) else {
            jet_foundation::ice!(None, "deque join received an invalid separator");
        };
        let parts: Vec<String> = xs.iter().map(|id| id.to_string()).collect();
        let joined = parts.join(&sep);
        rt.heap.alloc_string(joined)
    })
}

fn jet_jit_deque_reverse(dq: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if let Some(d) = rt.deques.get_mut((dq as usize).wrapping_sub(1)) {
            d.make_contiguous().reverse();
        }
        0
    })
}

fn jet_jit_deque_split(dq: i64, idx: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(d) = rt.deques.get_mut((dq as usize).wrapping_sub(1)) else {
            return 0;
        };
        let at = if idx < 0 {
            0
        } else {
            (idx as usize).min(d.len())
        };
        let rest = d.split_off(at);
        deque_handle(rt, rest)
    })
}

fn jet_jit_deque_from(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let xs = rt.heap.clone_int_list(list).unwrap_or_default();
        deque_handle(rt, xs.into_iter().collect())
    })
}

fn bag_handle(rt: &mut crate::JitRuntime, bag: HashMap<i64, usize>) -> i64 {
    rt.bags.push(bag);
    rt.bags.len() as i64
}

fn jet_jit_bag_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| bag_handle(rt, HashMap::new()))
}

fn jet_jit_bag_add(bag: i64, value: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(bag) = rt.bags.get_mut((bag as usize).wrapping_sub(1)) else {
            return 0;
        };
        *bag.entry(value).or_insert(0) += 1;
        1
    })
}

fn jet_jit_bag_remove(bag: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some(bag) = rt.bags.get_mut((bag as usize).wrapping_sub(1)) else {
            return;
        };
        let remove = match bag.get_mut(&value) {
            Some(count) if *count > 1 => {
                *count -= 1;
                false
            }
            Some(_) => true,
            None => false,
        };
        if remove {
            bag.remove(&value);
        }
    });
}

fn jet_jit_bag_has(bag: i64, value: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        i8::from(
            rt.bags
                .get((bag as usize).wrapping_sub(1))
                .and_then(|bag| bag.get(&value))
                .copied()
                .unwrap_or(0)
                > 0,
        )
    })
}

fn jet_jit_bag_count(bag: i64, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.bags
            .get((bag as usize).wrapping_sub(1))
            .and_then(|bag| bag.get(&value))
            .copied()
            .unwrap_or(0) as i64
    })
}

fn jet_jit_bag_len(bag: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.bags
            .get((bag as usize).wrapping_sub(1))
            .map(|bag| bag.values().sum::<usize>() as i64)
            .unwrap_or(0)
    })
}

pub(crate) struct LruState {
    capacity: usize,
    entries: VecDeque<(String, i64)>,
}

fn copy_list(rt: &mut crate::JitRuntime, values: impl IntoIterator<Item = i64>) -> i64 {
    let list = rt.heap.alloc_empty_list();
    for value in values {
        let _ = rt.heap.list_push_int(list, value);
    }
    list
}

fn jet_jit_sorted_set_new(string_kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| sorted_set_handle(rt, BTreeSet::new(), string_kind != 0))
}

fn jet_jit_sorted_set_len(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.sorted_sets
            .get((handle as usize).wrapping_sub(1))
            .map(|s| s.len() as i64)
            .unwrap_or(0)
    })
}

fn jet_jit_sorted_set_has(handle: i64, v: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        let Some(existing) = rt.sorted_sets.get(idx) else {
            return 0;
        };
        if sorted_set_is_string(rt, handle) {
            let needle = rt.heap.clone_string(v).unwrap_or_default();
            return i8::from(
                existing
                    .iter()
                    .any(|id| rt.heap.clone_string(*id).as_deref() == Some(needle.as_str())),
            );
        }
        i8::from(existing.contains(&v))
    })
}

fn jet_jit_sorted_set_from(list: i64, string_kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let xs = rt.heap.clone_int_list(list).unwrap_or_default();
        let string_kind = string_kind != 0;
        let set = if string_kind {
            canonical_sorted_string_set(rt, xs)
        } else {
            xs.into_iter().collect()
        };
        sorted_set_handle(rt, set, string_kind)
    })
}

fn jet_jit_sorted_set_insert(handle: i64, value: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        let string_kind = sorted_set_is_string(rt, handle);
        let Some(existing) = rt.sorted_sets.get(idx).cloned() else {
            return 0;
        };
        if string_kind {
            let needle = rt.heap.clone_string(value).unwrap_or_default();
            if existing.iter().any(|id| rt.heap.clone_string(*id).as_deref() == Some(needle.as_str())) {
                return 0;
            }
        }
        i8::from(rt.sorted_sets[idx].insert(value))
    })
}

fn jet_jit_sorted_set_remove(handle: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        let Some(existing) = rt.sorted_sets.get(idx).cloned() else { return; };
        let id = if sorted_set_is_string(rt, handle) {
            let needle = rt.heap.clone_string(value).unwrap_or_default();
            existing.iter().find(|id| rt.heap.clone_string(**id).as_deref() == Some(needle.as_str())).copied()
        } else {
            Some(value)
        };
        if let Some(id) = id {
            rt.sorted_sets[idx].remove(&id);
        }
    });
}

fn jet_jit_sorted_set_to_list(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let values = rt
            .sorted_sets
            .get((handle as usize).wrapping_sub(1))
            .map(|set| {
                if sorted_set_is_string(rt, handle) {
                    let mut values = sorted_string_ids(rt, set).into_iter().collect::<Vec<_>>();
                    values.sort_by(|(left, _), (right, _)| left.cmp(right));
                    values.into_iter().map(|(_, id)| id).collect()
                } else {
                    set.iter().copied().collect::<Vec<_>>()
                }
            })
            .unwrap_or_default();
        copy_list(rt, values)
    })
}

fn jet_jit_sorted_set_first(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let value = rt.sorted_sets
            .get((handle as usize).wrapping_sub(1))
            .and_then(|set| {
                if sorted_set_is_string(rt, handle) {
                    sorted_string_ids(rt, set)
                        .into_iter()
                        .min_by(|left, right| left.0.cmp(&right.0))
                        .map(|(_, id)| id)
                } else {
                    set.first().copied()
                }
            });
        option_i64(rt, value)
    })
}

fn jet_jit_sorted_set_last(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let value = rt.sorted_sets
            .get((handle as usize).wrapping_sub(1))
            .and_then(|set| {
                if sorted_set_is_string(rt, handle) {
                    sorted_string_ids(rt, set)
                        .into_iter()
                        .max_by(|left, right| left.0.cmp(&right.0))
                        .map(|(_, id)| id)
                } else {
                    set.last().copied()
                }
            });
        option_i64(rt, value)
    })
}

fn jet_jit_sorted_set_union(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let left = rt
            .sorted_sets
            .get((a as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let right = rt
            .sorted_sets
            .get((b as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let string_kind = sorted_set_is_string(rt, a) || sorted_set_is_string(rt, b);
        if string_kind {
            let ids = sorted_string_ids(rt, &left).into_iter().chain(sorted_string_ids(rt, &right)).collect::<HashMap<_, _>>();
            let out = set_semantics::jet_sorted_set_union(&sorted_string_values(rt, &left), &sorted_string_values(rt, &right));
            return sorted_set_handle(rt, out.into_iter().filter_map(|value| ids.get(&value).copied()).collect(), true);
        }
        sorted_set_handle(rt, set_semantics::jet_sorted_set_union(&left, &right), false)
    })
}

fn jet_jit_sorted_set_intersection(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let left = rt
            .sorted_sets
            .get((a as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let right = rt
            .sorted_sets
            .get((b as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let string_kind = sorted_set_is_string(rt, a) || sorted_set_is_string(rt, b);
        if string_kind {
            let ids = sorted_string_ids(rt, &left).into_iter().chain(sorted_string_ids(rt, &right)).collect::<HashMap<_, _>>();
            let out = set_semantics::jet_sorted_set_intersection(&sorted_string_values(rt, &left), &sorted_string_values(rt, &right));
            return sorted_set_handle(rt, out.into_iter().filter_map(|value| ids.get(&value).copied()).collect(), true);
        }
        sorted_set_handle(rt, set_semantics::jet_sorted_set_intersection(&left, &right), false)
    })
}

fn jet_jit_sorted_set_difference(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let left = rt
            .sorted_sets
            .get((a as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let right = rt
            .sorted_sets
            .get((b as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let string_kind = sorted_set_is_string(rt, a) || sorted_set_is_string(rt, b);
        if string_kind {
            let ids = sorted_string_ids(rt, &left);
            let out = set_semantics::jet_sorted_set_difference(&sorted_string_values(rt, &left), &sorted_string_values(rt, &right));
            return sorted_set_handle(rt, out.into_iter().filter_map(|value| ids.get(&value).copied()).collect(), true);
        }
        sorted_set_handle(rt, set_semantics::jet_sorted_set_difference(&left, &right), false)
    })
}

fn jet_jit_sorted_set_symmetric_difference(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let left = rt
            .sorted_sets
            .get((a as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let right = rt
            .sorted_sets
            .get((b as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        let string_kind = sorted_set_is_string(rt, a) || sorted_set_is_string(rt, b);
        if string_kind {
            let ids = sorted_string_ids(rt, &left).into_iter().chain(sorted_string_ids(rt, &right)).collect::<HashMap<_, _>>();
            let out = set_semantics::jet_sorted_set_symmetric_difference(&sorted_string_values(rt, &left), &sorted_string_values(rt, &right));
            return sorted_set_handle(rt, out.into_iter().filter_map(|value| ids.get(&value).copied()).collect(), true);
        }
        sorted_set_handle(rt, set_semantics::jet_sorted_set_symmetric_difference(&left, &right), false)
    })
}

fn jet_jit_sorted_set_is_subset(a: i64, b: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(left) = rt.sorted_sets.get((a as usize).wrapping_sub(1)).cloned() else {
            return 1;
        };
        let Some(right) = rt.sorted_sets.get((b as usize).wrapping_sub(1)).cloned() else {
            return i8::from(left.is_empty());
        };
        if sorted_set_is_string(rt, a) || sorted_set_is_string(rt, b) {
            return i8::from(set_semantics::jet_sorted_set_is_subset(&sorted_string_values(rt, &left), &sorted_string_values(rt, &right)));
        }
        i8::from(set_semantics::jet_sorted_set_is_subset(&left, &right))
    })
}

fn jet_jit_sorted_set_is_superset(a: i64, b: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(left) = rt.sorted_sets.get((a as usize).wrapping_sub(1)).cloned() else {
            return i8::from(
                rt.sorted_sets
                    .get((b as usize).wrapping_sub(1))
                    .is_some_and(std::collections::BTreeSet::is_empty),
            );
        };
        let Some(right) = rt.sorted_sets.get((b as usize).wrapping_sub(1)).cloned() else {
            return 1;
        };
        if sorted_set_is_string(rt, a) || sorted_set_is_string(rt, b) {
            return i8::from(set_semantics::jet_sorted_set_is_superset(&sorted_string_values(rt, &left), &sorted_string_values(rt, &right)));
        }
        i8::from(set_semantics::jet_sorted_set_is_superset(&left, &right))
    })
}

fn jet_jit_sorted_set_is_disjoint(a: i64, b: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(left) = rt.sorted_sets.get((a as usize).wrapping_sub(1)).cloned() else {
            return 1;
        };
        let Some(right) = rt.sorted_sets.get((b as usize).wrapping_sub(1)).cloned() else {
            return 1;
        };
        if sorted_set_is_string(rt, a) || sorted_set_is_string(rt, b) {
            return i8::from(set_semantics::jet_sorted_set_is_disjoint(&sorted_string_values(rt, &left), &sorted_string_values(rt, &right)));
        }
        i8::from(set_semantics::jet_sorted_set_is_disjoint(&left, &right))
    })
}

fn jet_jit_priority_queue_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.priority_queues.push(BinaryHeap::new());
        rt.priority_queues.len() as i64
    })
}

fn jet_jit_priority_queue_len(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.priority_queues
            .get((handle as usize).wrapping_sub(1))
            .map(|heap| heap.len() as i64)
            .unwrap_or(0)
    })
}

fn jet_jit_priority_queue_from(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let heap = BinaryHeap::from(rt.heap.clone_int_list(list).unwrap_or_default());
        rt.priority_queues.push(heap);
        rt.priority_queues.len() as i64
    })
}

fn jet_jit_priority_queue_push(handle: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        if let Some(heap) = rt
            .priority_queues
            .get_mut((handle as usize).wrapping_sub(1))
        {
            heap.push(value);
        }
    });
}

fn jet_jit_priority_queue_peek(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let value = rt.priority_queues
            .get((handle as usize).wrapping_sub(1))
            .and_then(|heap| heap.peek().copied());
        option_i64(rt, value)
    })
}

fn jet_jit_priority_queue_pop(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let value = rt.priority_queues
            .get_mut((handle as usize).wrapping_sub(1))
            .and_then(collection_semantics::priority_queue_pop);
        option_i64(rt, value)
    })
}

fn jet_jit_priority_queue_to_sorted_list(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let values = rt
            .priority_queues
            .get((handle as usize).wrapping_sub(1))
            .map(|heap| heap.clone().into_sorted_vec().into_iter().rev().collect::<Vec<_>>())
            .unwrap_or_default();
        copy_list(rt, values)
    })
}

fn jet_jit_priority_queue_remove_value(handle: i64, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let removed = match rt
            .priority_queues
            .get_mut((handle as usize).wrapping_sub(1))
        {
            Some(heap) => collection_semantics::priority_queue_remove_value(heap, value).ok(),
            None => None,
        };
        option_i64(rt, removed)
    })
}

fn jet_jit_priority_queue_remove_slot(handle: i64, index: i64, line: u32) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let removed = match rt
            .priority_queues
            .get_mut((handle as usize).wrapping_sub(1))
        {
            Some(heap) => match collection_semantics::priority_queue_remove_slot(
                heap, index, "<jit>", line,
            ) {
                Ok(value) => value.ok(),
                Err(message) => {
                    rt.set_runtime_stop("E3010", line, &message);
                    None
                }
            },
            None => None,
        };
        option_i64(rt, removed)
    })
}

fn jet_jit_lru_new(capacity: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.lrus.push(LruState {
            capacity: capacity.max(0) as usize,
            entries: VecDeque::new(),
        });
        rt.lrus.len() as i64
    })
}

fn jet_jit_lru_put(handle: i64, key: i64, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(key) = rt.heap.clone_string(key) else {
            return option_i64(rt, None);
        };
        let Some(lru) = rt.lrus.get_mut((handle as usize).wrapping_sub(1)) else {
            return option_i64(rt, None);
        };
        if let Some(index) = lru.entries.iter().position(|(existing, _)| existing == &key) {
            let (_, old) = lru.entries.remove(index).expect("lru position");
            lru.entries.push_front((key, value));
            return option_i64(rt, Some(old));
        }
        lru.entries.push_front((key, value));
        if lru.entries.len() > lru.capacity {
            let evicted = lru.entries.pop_back().map(|(_, old)| old);
            return option_i64(rt, evicted);
        }
        option_i64(rt, None)
    })
}

fn jet_jit_lru_get(handle: i64, key: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(key) = rt.heap.clone_string(key) else {
            return option_i64(rt, None);
        };
        let Some(lru) = rt.lrus.get_mut((handle as usize).wrapping_sub(1)) else {
            return option_i64(rt, None);
        };
        let Some(index) = lru.entries.iter().position(|(existing, _)| existing == &key) else {
            return option_i64(rt, None);
        };
        let entry = lru.entries.remove(index).expect("lru position");
        let value = entry.1;
        lru.entries.push_front(entry);
        option_i64(rt, Some(value))
    })
}

fn jet_jit_lru_has(handle: i64, key: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(key) = rt.heap.clone_string(key) else {
            return 0;
        };
        i8::from(
            rt.lrus
                .get((handle as usize).wrapping_sub(1))
                .is_some_and(|lru| lru.entries.iter().any(|(existing, _)| existing == &key)),
        )
    })
}

fn jet_jit_lru_keys(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let keys = rt
            .lrus
            .get((handle as usize).wrapping_sub(1))
            .map(|lru| {
                lru.entries
                    .iter()
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let ids = keys.into_iter().map(|key| rt.heap.alloc_string(key)).collect::<Vec<_>>();
        copy_list(rt, ids)
    })
}

fn jet_jit_bit_set_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.bit_sets.push(BTreeSet::new());
        rt.bit_sets.len() as i64
    })
}

fn jet_jit_bit_set_add(handle: i64, value: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        i8::from(
            rt.bit_sets
                .get_mut((handle as usize).wrapping_sub(1))
                .is_some_and(|set| set.insert(value)),
        )
    })
}

fn jet_jit_bit_set_remove(handle: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        if let Some(set) = rt.bit_sets.get_mut((handle as usize).wrapping_sub(1)) {
            set.remove(&value);
        }
    });
}

fn jet_jit_bit_set_has(handle: i64, value: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        i8::from(
            rt.bit_sets
                .get((handle as usize).wrapping_sub(1))
                .is_some_and(|set| collection_semantics::bit_set_has(set, value)),
        )
    })
}

fn jet_jit_bit_set_to_list(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let values = rt
            .bit_sets
            .get((handle as usize).wrapping_sub(1))
            .map(|set| set.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        copy_list(rt, values)
    })
}

fn jet_jit_bit_set_copy(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let values = rt
            .bit_sets
            .get((handle as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        rt.bit_sets.push(collection_semantics::bit_set_copy(&values));
        rt.bit_sets.len() as i64
    })
}

fn jet_jit_bit_set_len(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.bit_sets
            .get((handle as usize).wrapping_sub(1))
            .and_then(|set| set.iter().next_back().copied())
            .map(|last| last.saturating_add(1))
            .unwrap_or(0)
    })
}

fn jet_jit_bit_set_count(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.bit_sets
            .get((handle as usize).wrapping_sub(1))
            .map(|set| set.len() as i64)
            .unwrap_or(0)
    })
}

fn jet_jit_byte_buffer_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.byte_buffers
            .push(byte_buffer_semantics::JetByteBuffer::new());
        rt.byte_buffers.len() as i64
    })
}

fn jet_jit_byte_buffer_with_capacity(n: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.byte_buffers
            .push(byte_buffer_semantics::JetByteBuffer::with_capacity(n));
        rt.byte_buffers.len() as i64
    })
}

fn jet_jit_byte_buffer_from(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let bytes = rt
            .heap
            .clone_int_list(list)
            .unwrap_or_default()
            .into_iter()
            .map(|byte| byte as u8)
            .collect::<Vec<_>>();
        rt.byte_buffers
            .push(byte_buffer_semantics::JetByteBuffer::from(&bytes));
        rt.byte_buffers.len() as i64
    })
}

fn jet_jit_byte_buffer_write(handle: i64, value: i64, method: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let list_bytes = if matches!(method, 7 | 9) {
            Some(
                rt.heap
                    .clone_int_list(value)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|byte| byte as u8)
                    .collect::<Vec<_>>(),
            )
        } else {
            None
        };
        let Some(buffer) = rt
            .byte_buffers
            .get_mut((handle as usize).wrapping_sub(1))
        else {
            return;
        };
        match method {
            0 => buffer.write_u8(value as u8),
            8 => buffer.write_byte(value as u8),
            1 => buffer.write_u16_le(value as u16),
            2 => buffer.write_u16_be(value as u16),
            3 => buffer.write_u32_le(value as u32),
            4 => buffer.write_u32_be(value as u32),
            5 => buffer.write_u64_le(value as u64),
            6 => buffer.write_u64_be(value as u64),
            7 => {
                if let Some(bytes) = list_bytes {
                    buffer.write_bytes(&bytes);
                }
            }
            9 => {
                if let Some(bytes) = list_bytes {
                    buffer.write(&bytes);
                }
            }
            _ => {}
        }
    });
}

fn jet_jit_byte_buffer_to_bytes(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let values = rt
            .byte_buffers
            .get((handle as usize).wrapping_sub(1))
            .map(|b| b.to_bytes())
            .unwrap_or_default()
            .into_iter()
            .map(i64::from)
            .collect::<Vec<_>>();
        copy_list(rt, values)
    })
}

/// method codes for ByteBufferMethod / cursor+string-like surface.
/// Returns packed values: bool as 0/1 in low byte when ret_kind=0;
/// i64 when ret_kind=1; option i64 (0=None else v+1) when ret_kind=2;
/// string handle when ret_kind=3; new Bytes handle when ret_kind=4;
/// list-of-string handle when ret_kind=5; unit 0 when ret_kind=6.
fn jet_jit_byte_buffer_method(
    handle: i64,
    method: i64,
    arg0: i64,
    arg1: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        match method {
            // 0-arg reads / transforms
            0 => rt.byte_buffers.get(idx).map(|b| b.len()).unwrap_or(0),
            1 => i64::from(rt.byte_buffers.get(idx).is_some_and(|b| b.is_empty())),
            2 => {
                if let Some(b) = rt.byte_buffers.get_mut(idx) {
                    b.clear();
                }
                0
            }
            3 => rt.byte_buffers.get(idx).map(|b| b.capacity()).unwrap_or(0),
            4 => rt.byte_buffers.get(idx).map(|b| b.position()).unwrap_or(0),
            5 => i64::from(rt.byte_buffers.get(idx).is_some_and(|b| b.eof())),
            6 => {
                if let Some(b) = rt.byte_buffers.get_mut(idx) {
                    b.rewind();
                }
                0
            }
            7 => {
                if let Some(b) = rt.byte_buffers.get_mut(idx) {
                    b.flush();
                }
                0
            }
            8 => {
                if let Some(b) = rt.byte_buffers.get_mut(idx) {
                    b.close();
                }
                0
            }
            9 => {
                if let Some(b) = rt.byte_buffers.get_mut(idx) {
                    b.shutdown();
                }
                0
            }
            10 | 11 | 12 => {
                let bytes = rt
                    .byte_buffers
                    .get(idx)
                    .map(|b| match method {
                        10 => b.to_bytes(),
                        11 => b.get_buffer(),
                        _ => b.buffer(),
                    })
                    .unwrap_or_default()
                    .into_iter()
                    .map(i64::from)
                    .collect::<Vec<_>>();
                copy_list(rt, bytes)
            }
            13 | 14 => {
                let s = rt
                    .byte_buffers
                    .get(idx)
                    .map(|b| {
                        if method == 13 {
                            b.to_string()
                        } else {
                            b.string()
                        }
                    })
                    .unwrap_or_default();
                rt.heap.alloc_string(s)
            }
            15..=21 => {
                let out = rt.byte_buffers.get(idx).map(|b| match method {
                    15 => b.trim(),
                    16 => b.trim_start(),
                    17 => b.trim_end(),
                    18 => b.to_lower(),
                    19 => b.to_upper(),
                    20 => b.to_title(),
                    _ => b.title(),
                });
                match out {
                    Some(buf) => {
                        rt.byte_buffers.push(buf);
                        rt.byte_buffers.len() as i64
                    }
                    None => 0,
                }
            }
            22 | 23 => {
                let out = rt.byte_buffers.get(idx).map(|b| b.copy());
                match out {
                    Some(buf) => {
                        rt.byte_buffers.push(buf);
                        rt.byte_buffers.len() as i64
                    }
                    None => 0,
                }
            }
            24 => {
                let lines = rt
                    .byte_buffers
                    .get(idx)
                    .map(|b| b.lines())
                    .unwrap_or_default();
                let ids = lines
                    .into_iter()
                    .map(|s| rt.heap.alloc_string(s))
                    .collect::<Vec<_>>();
                copy_list(rt, ids)
            }
            25 => option_i64(
                rt,
                rt.byte_buffers
                    .get(idx)
                    .and_then(|b| b.first().ok())
                    .map(|b| b as i64),
            ),
            26 | 27 => {
                let byte = rt.byte_buffers.get_mut(idx).and_then(|b| {
                    if method == 26 {
                        b.next().ok()
                    } else {
                        b.read_byte().ok()
                    }
                });
                option_i64(rt, byte.map(|b| b as i64))
            }
            28 => {
                let out = rt.byte_buffers.get_mut(idx).and_then(|b| b.read().ok());
                match out {
                    Some(bytes) => {
                        let values = bytes.into_iter().map(i64::from).collect::<Vec<_>>();
                        let list = copy_list(rt, values);
                        list + 1
                    }
                    None => 0,
                }
            }
            29 => i64::from(rt.byte_buffers.get(idx).is_some_and(|b| b.is_ascii())),
            30 => {
                let text = rt
                    .byte_buffers
                    .get(idx)
                    .map(|b| b.to_string())
                    .unwrap_or_default();
                match rt.heap.int_from_str(text.trim()) {
                    Ok(n) => {
                    // Result packed: positive = Ok(n+1) won't work for negatives.
                    // Store ok as string/int via existing result helpers if any —
                    // for the example, return n directly when ok and use trap on err.
                    n
                    }
                    Err(_) => {
                        rt.trapped = Some(format!(
                            "cannot parse `{text}` as an integer"
                        ));
                        0
                    }
                }
            }
            // 1-arg
            40 => option_i64(
                rt,
                rt.byte_buffers
                    .get(idx)
                    .and_then(|b| b.get(arg0).ok())
                    .map(|b| b as i64),
            ),
            41 => {
                if let Some(b) = rt.byte_buffers.get_mut(idx) {
                    b.seek(arg0);
                }
                0
            }
            42 => match rt.byte_buffers.get_mut(idx).and_then(|b| b.read_bytes(arg0).ok()) {
                Some(bytes) => {
                    let values = bytes.into_iter().map(i64::from).collect::<Vec<_>>();
                    copy_list(rt, values) + 1
                }
                None => 0,
            },
            43 => match rt
                .byte_buffers
                .get_mut(idx)
                .and_then(|b| b.read_string(arg0).ok())
            {
                Some(s) => rt.heap.alloc_string(s) + 1,
                None => 0,
            },
            44 | 45 | 46 => {
                let Some(needle) = rt.heap.clone_string(arg0) else {
                    return 0;
                };
                i64::from(rt.byte_buffers.get(idx).is_some_and(|b| match method {
                    44 => b.contains(&needle),
                    45 => b.starts_with(&needle),
                    _ => b.ends_with(&needle),
                }))
            }
            47 | 48 => {
                let Some(needle) = rt.heap.clone_string(arg0) else {
                    return 0;
                };
                option_packed(rt.byte_buffers.get(idx).and_then(|b| {
                    if method == 47 {
                        b.index_of(&needle).ok()
                    } else {
                        b.last_index_of(&needle).ok()
                    }
                }))
            }
            49 => {
                let Some(sep) = rt.heap.clone_string(arg0) else {
                    return 0;
                };
                let parts = rt
                    .byte_buffers
                    .get(idx)
                    .map(|b| b.split(&sep))
                    .unwrap_or_default();
                let ids = parts
                    .into_iter()
                    .map(|s| rt.heap.alloc_string(s))
                    .collect::<Vec<_>>();
                copy_list(rt, ids)
            }
            50 => {
                let parts = rt
                    .heap
                    .clone_int_list(arg0)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|id| rt.heap.clone_string(id))
                    .collect::<Vec<_>>();
                let out = rt
                    .byte_buffers
                    .get(idx)
                    .map(|b| b.join(&parts));
                match out {
                    Some(buf) => {
                        rt.byte_buffers.push(buf);
                        rt.byte_buffers.len() as i64
                    }
                    None => 0,
                }
            }
            51 | 52 => {
                let other_idx = (arg0 as usize).wrapping_sub(1);
                let other = rt.byte_buffers.get(other_idx).cloned();
                match (rt.byte_buffers.get(idx), other) {
                    (Some(a), Some(b)) => {
                        if method == 51 {
                            i64::from(a.equal(&b))
                        } else {
                            a.compare(&b)
                        }
                    }
                    _ => 0,
                }
            }
            53 | 54 => {
                let other_idx = (arg0 as usize).wrapping_sub(1);
                if idx == other_idx {
                    return 0;
                }
                // split borrows: clone source first
                let src = rt.byte_buffers.get(idx).cloned();
                if let (Some(src), Some(dst)) =
                    (src, rt.byte_buffers.get_mut(other_idx))
                {
                    if method == 53 {
                        src.copy_to(dst);
                    } else {
                        let mut src = src;
                        src.write_to(dst);
                    }
                }
                0
            }
            // replace (2-arg)
            60 => {
                let Some(from) = rt.heap.clone_string(arg0) else {
                    return 0;
                };
                let Some(to) = rt.heap.clone_string(arg1) else {
                    return 0;
                };
                let out = rt.byte_buffers.get(idx).map(|b| b.replace(&from, &to));
                match out {
                    Some(buf) => {
                        rt.byte_buffers.push(buf);
                        rt.byte_buffers.len() as i64
                    }
                    None => 0,
                }
            }
            _ => 0,
        }
    })
}

/// Packed-enum JetShow table: variant mangled names + payload kind codes.
/// kind: 0 = unit, 1 = Int (>>8), 2 = nested packed enum (>>8), 3 = String handle (>>8).
#[derive(Clone)]
struct PackedEnumShow {
    variants: Vec<(String, u8, String)>, // (__jet_Variant, kind, nested_enum_name)
}

thread_local! {
    static PACKED_ENUM_SHOW: std::cell::RefCell<std::collections::HashMap<String, PackedEnumShow>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

pub(crate) fn clear_packed_enum_show() {
    PACKED_ENUM_SHOW.with(|t| t.borrow_mut().clear());
}

pub(crate) fn register_packed_enum_show(
    enum_name: &str,
    variants: Vec<(String, u8, String)>,
) {
    PACKED_ENUM_SHOW.with(|t| {
        t.borrow_mut().insert(
            enum_name.to_string(),
            PackedEnumShow { variants },
        );
    });
}

fn show_packed_enum(packed: i64, enum_name: &str, heap: &jet_rt::JetArena) -> String {
    if enum_name == "IOError" {
        let context = packed >> 8;
        let resource = heap
            .record_get_int(context, 1)
            .and_then(|encoded| encoded.checked_sub(1))
            .and_then(|handle| heap.clone_string(handle));
        let cause = heap
            .record_get_int(context, 3)
            .and_then(|encoded| encoded.checked_sub(1))
            .and_then(|handle| heap.clone_string(handle));
        return jet_foundation::StructuralDebug::jet_show_io_error(
            packed & 0xff,
            heap.record_get_int(context, 0).unwrap_or(0),
            resource.as_deref(),
            cause.as_deref(),
        );
    }
    let def = PACKED_ENUM_SHOW.with(|t| t.borrow().get(enum_name).cloned());
    let Some(def) = def else {
        return format!("<enum {enum_name}>");
    };
    let disc = (packed & 0xff) as usize;
    let Some((vname, kind, nested)) = def.variants.get(disc).cloned() else {
        return format!("<bad disc {disc}>");
    };
    let payload = match kind {
        0 => None,
        1 => Some((packed >> 8).to_string()),
        2 => Some(show_packed_enum(packed >> 8, &nested, heap)),
        // String handle in high bits — AOT JetShow uses Debug quotes.
        3 => {
            let text = heap.clone_string(packed >> 8).unwrap_or_default();
            Some(format!("{text:?}"))
        }
        _ => return format!("<{vname}?>"),
    };
    jet_foundation::StructuralDebug::jet_debug_variant(&vname, payload)
}

pub(crate) fn render_packed_enum(
    packed: i64,
    enum_name: &str,
    heap: &jet_rt::JetArena,
) -> String {
    show_packed_enum(packed, enum_name, heap)
}

fn packed_enum_name(name_ptr: i64, name_len: i64) -> String {
    if name_ptr == 0 || name_len <= 0 {
        return String::new();
    }
    let slice = unsafe {
        std::slice::from_raw_parts(name_ptr as *const u8, name_len as usize)
    };
    String::from_utf8_lossy(slice).into_owned()
}

/// Print a packed i64 enum. `name_ptr`/`name_len` are a UTF-8 view of the Jet
/// enum name (stable for the process — not a heap string handle).
fn jet_jit_print_enum(packed: i64, name_ptr: i64, name_len: i64) {
    // One routing adapter for every resident print. Pushing straight into the
    // capture buffer put an enum's line wherever that buffer was flushed, which
    // is not where the program printed it.
    let text = Concurrency::with_runtime_mut(|rt| {
        let name = packed_enum_name(name_ptr, name_len);
        Some(show_packed_enum(packed, &name, &rt.heap))
    });
    if let Some(text) = text {
        let frame = crate::IO::term_prelude::jet_term_print_frame(&text);
        let _ = crate::runtime_host::write_jit_stdout(&frame, false);
    }
}

/// Return a packed enum's JetShow text as a string handle for interpolation.
fn jet_jit_enum_show(packed: i64, name_ptr: i64, name_len: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let name = packed_enum_name(name_ptr, name_len);
        let text = show_packed_enum(packed, &name, &rt.heap);
        rt.heap.alloc_string(text)
    })
}

host_fns! {
    struct CollectionsHostFns;
    register: register_collections_symbols;
    declare: declare_collections_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::{Linkage, Module};
        let cc = module.target_config().default_call_conv;
        let mut sig_new = Signature::new(cc);
        sig_new.returns.push(AbiParam::new(types::I64));
        let mut sig_zip_family = Signature::new(cc);
        sig_zip_family
            .params
            .extend([AbiParam::new(types::I64); 4]);
        sig_zip_family.returns.push(AbiParam::new(types::I64));
        let mut sig_sorted_set_new = sig_new.clone();
        sig_sorted_set_new.params.push(AbiParam::new(types::I64));
        let mut sig_uninit = Signature::new(cc);
        sig_uninit.params.push(AbiParam::new(types::I64));
        sig_uninit.returns.push(AbiParam::new(types::I64));
        let mut sig_push = Signature::new(cc);
        sig_push.params.push(AbiParam::new(types::I64));
        sig_push.params.push(AbiParam::new(types::I64));
        let mut sig_push_f64 = Signature::new(cc);
        sig_push_f64.params.push(AbiParam::new(types::I64));
        sig_push_f64.params.push(AbiParam::new(types::F64));
        let sig_try_new = sig_new.clone();
        let mut sig_try_with_capacity = Signature::new(cc);
        sig_try_with_capacity.params.push(AbiParam::new(types::I64));
        sig_try_with_capacity.returns.push(AbiParam::new(types::I64));
        let mut sig_try_push = sig_push.clone();
        sig_try_push.returns.push(AbiParam::new(types::I64));
        let mut sig_try_push_f64 = sig_push_f64.clone();
        sig_try_push_f64.returns.push(AbiParam::new(types::I64));
        let mut sig_try_reserve = sig_push.clone();
        sig_try_reserve.returns.push(AbiParam::new(types::I64));
        let sig_try_reserve_f64 = sig_try_reserve.clone();
        let sig_try_string_push = sig_try_push.clone();
        let mut sig_push_range = Signature::new(cc);
        sig_push_range.params.push(AbiParam::new(types::I64));
        sig_push_range.params.push(AbiParam::new(types::I64));
        sig_push_range.params.push(AbiParam::new(types::I64));
        sig_push_range.params.push(AbiParam::new(types::I8));
        let mut sig_len = Signature::new(cc);
        sig_len.params.push(AbiParam::new(types::I64));
        sig_len.returns.push(AbiParam::new(types::I64));
        let mut sig_get = sig_len.clone();
        sig_get.params.push(AbiParam::new(types::I64));
        sig_get.params.push(AbiParam::new(types::I32));
        let mut sig_get_f64 = sig_get.clone();
        sig_get_f64.returns.clear();
        sig_get_f64.returns.push(AbiParam::new(types::F64));
        let sig_get_range_scalar = sig_get.clone();
        let mut sig_get_range_exclusive = sig_get.clone();
        sig_get_range_exclusive.returns.clear();
        sig_get_range_exclusive
            .returns
            .push(AbiParam::new(types::I8));
        let mut sig_get_opt = sig_len.clone();
        sig_get_opt.params.push(AbiParam::new(types::I64));
        let mut sig_list_remove_slot = sig_get_opt.clone();
        sig_list_remove_slot.params.push(AbiParam::new(types::I32));
        let mut sig_debug_optional = Signature::new(cc);
        sig_debug_optional
            .params
            .extend([AbiParam::new(types::I64); 3]);
        let sig_debug_record = sig_debug_optional.clone();
        let mut sig_debug_variant = Signature::new(cc);
        sig_debug_variant
            .params
            .extend([AbiParam::new(types::I64); 4]);
        let mut sig_scalar_debug = Signature::new(cc);
        sig_scalar_debug
            .params
            .extend([AbiParam::new(types::I64); 2]);
        sig_scalar_debug.returns.push(AbiParam::new(types::I64));
        let mut sig_set_from = sig_len.clone();
        sig_set_from.params.push(AbiParam::new(types::I64));
        let mut sig_list_eq = Signature::new(cc);
        sig_list_eq.params.push(AbiParam::new(types::I64));
        sig_list_eq.params.push(AbiParam::new(types::I64));
        sig_list_eq.returns.push(AbiParam::new(types::I8));
        // list_set(list, idx, val, line)
        let mut sig_set = Signature::new(cc);
        sig_set.params.push(AbiParam::new(types::I64));
        sig_set.params.push(AbiParam::new(types::I64));
        sig_set.params.push(AbiParam::new(types::I64));
        sig_set.params.push(AbiParam::new(types::I32));
        // list_set_f64(list, idx, val, line)
        let mut sig_set_f64 = Signature::new(cc);
        sig_set_f64.params.push(AbiParam::new(types::I64));
        sig_set_f64.params.push(AbiParam::new(types::I64));
        sig_set_f64.params.push(AbiParam::new(types::F64));
        sig_set_f64.params.push(AbiParam::new(types::I32));
        let mut sig_sort = sig_len.clone();
        sig_sort.returns.clear();
        // list_slice(list, start, end, line) -> id
        let mut sig_slice = Signature::new(cc);
        sig_slice.params.push(AbiParam::new(types::I64));
        sig_slice.params.push(AbiParam::new(types::I64));
        sig_slice.params.push(AbiParam::new(types::I64));
        sig_slice.params.push(AbiParam::new(types::I32));
        sig_slice.returns.push(AbiParam::new(types::I64));
        let mut sig_range_end = Signature::new(cc);
        sig_range_end.params.push(AbiParam::new(types::I64));
        sig_range_end.params.push(AbiParam::new(types::I64));
        sig_range_end.params.push(AbiParam::new(types::I64));
        sig_range_end.params.push(AbiParam::new(types::I64));
        sig_range_end.params.push(AbiParam::new(types::I32));
        sig_range_end.returns.push(AbiParam::new(types::I64));
        let mut sig_range_contains = Signature::new(cc);
        for _ in 0..4 {
            sig_range_contains.params.push(AbiParam::new(types::I64));
        }
        sig_range_contains.returns.push(AbiParam::new(types::I8));
        let mut sig_disjoint = sig_list_eq.clone();
        sig_disjoint.returns.clear();
        sig_disjoint.returns.push(AbiParam::new(types::I64));
        let mut sig_range_show = Signature::new(cc);
        for _ in 0..3 {
            sig_range_show.params.push(AbiParam::new(types::I64));
        }
        sig_range_show.returns.push(AbiParam::new(types::I64));
        let mut sig_range_equal = Signature::new(cc);
        for _ in 0..6 {
            sig_range_equal.params.push(AbiParam::new(types::I64));
        }
        sig_range_equal.returns.push(AbiParam::new(types::I8));
        let mut sig_join = sig_len.clone();
        sig_join.params.push(AbiParam::new(types::I64));
        let mut sig_map_insert = Signature::new(cc);
        sig_map_insert.params.push(AbiParam::new(types::I64));
        sig_map_insert.params.push(AbiParam::new(types::I64));
        sig_map_insert.params.push(AbiParam::new(types::I64));
        let mut sig_three_ret = sig_map_insert.clone();
        sig_three_ret.returns.push(AbiParam::new(types::I64));
        let sig_try_map_insert = sig_three_ret.clone();
        let mut sig_four_ret = sig_three_ret.clone();
        sig_four_ret.params.push(AbiParam::new(types::I64));
        let sig_map_get = sig_get.clone();
        let sig_map_get_opt = sig_get_opt.clone();
        let sig_map_at = sig_get_opt.clone();
        let mut sig_print_enum = Signature::new(cc);
        sig_print_enum.params.push(AbiParam::new(types::I64));
        sig_print_enum.params.push(AbiParam::new(types::I64));
        sig_print_enum.params.push(AbiParam::new(types::I64));
        let mut sig_enum_show = sig_print_enum.clone();
        sig_enum_show.returns.push(AbiParam::new(types::I64));
        let mut sig_sort_by_keys = sig_get_opt.clone();
        sig_sort_by_keys.returns.clear();
        let mut sig_bool = Signature::new(cc);
        sig_bool.params.push(AbiParam::new(types::I64));
        sig_bool.returns.push(AbiParam::new(types::I8));
        let mut sig_f64 = Signature::new(cc);
        sig_f64.params.push(AbiParam::new(types::I64));
        sig_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_closure_predicate = Signature::new(cc);
        sig_closure_predicate
            .params
            .extend([AbiParam::new(types::I64); 2]);
        sig_closure_predicate.returns.push(AbiParam::new(types::I8));
        let mut sig_closure_value = sig_closure_predicate.clone();
        sig_closure_value.returns.clear();
        sig_closure_value.returns.push(AbiParam::new(types::I64));
        let sig_closure_each = sig_closure_predicate.clone();
        let mut sig_priority_queue_slot = Signature::new(cc);
        sig_priority_queue_slot.params.push(AbiParam::new(types::I64));
        sig_priority_queue_slot.params.push(AbiParam::new(types::I64));
        sig_priority_queue_slot.params.push(AbiParam::new(types::I32));
        sig_priority_queue_slot.returns.push(AbiParam::new(types::I64));


    }
    io_args: "jet_jit_io_args" => jet_jit_io_args: sig_new;
    list_new: "jet_jit_list_new" => jet_jit_list_new: sig_new;
    list_try_new: "jet_jit_list_try_new" => jet_jit_list_try_new: sig_try_new;
    list_try_with_capacity: "jet_jit_list_try_with_capacity" => jet_jit_list_try_with_capacity: sig_try_with_capacity;
    list_uninit: "jet_jit_list_uninit" => jet_jit_list_uninit: sig_uninit;
    list_push: "jet_jit_list_push" => jet_jit_list_push: sig_push;
    list_push_f64: "jet_jit_list_push_f64" => jet_jit_list_push_f64: sig_push_f64;
    list_try_push: "jet_jit_list_try_push" => jet_jit_list_try_push: sig_try_push;
    list_try_push_f64: "jet_jit_list_try_push_f64" => jet_jit_list_try_push_f64: sig_try_push_f64;
    list_try_reserve: "jet_jit_list_try_reserve" => jet_jit_list_try_reserve: sig_try_reserve;
    list_try_reserve_f64: "jet_jit_list_try_reserve_f64" => jet_jit_list_try_reserve_f64: sig_try_reserve_f64;
    string_try_push: "jet_jit_string_try_push" => jet_jit_string_try_push: sig_try_string_push;
    list_push_range: "jet_jit_list_push_range" => jet_jit_list_push_range: sig_push_range;
    list_get: "jet_jit_list_get" => jet_jit_list_get: sig_get;
    list_get_f64: "jet_jit_list_get_f64" => jet_jit_list_get_f64: sig_get_f64;
    columnar_gather: "jet_jit_columnar_gather" => jet_jit_columnar_gather: sig_get;
    fixed_list_get: "jet_jit_fixed_list_get" => jet_jit_fixed_list_get: sig_get;
    fixed_list_get_f64: "jet_jit_fixed_list_get_f64" => jet_jit_fixed_list_get_f64: sig_get_f64;
    list_get_range_start: "jet_jit_list_get_range_start" => jet_jit_list_get_range_start: sig_get_range_scalar;
    list_get_range_end: "jet_jit_list_get_range_end" => jet_jit_list_get_range_end: sig_get_range_scalar;
    list_get_range_exclusive: "jet_jit_list_get_range_exclusive" => jet_jit_list_get_range_exclusive: sig_get_range_exclusive;
    list_get_opt: "jet_jit_list_get_opt" => jet_jit_list_get_opt: sig_get_opt;
    list_set: "jet_jit_list_set" => jet_jit_list_set: sig_set;
    list_set_f64: "jet_jit_list_set_f64" => jet_jit_list_set_f64: sig_set_f64;
    list_len: "jet_jit_list_len" => jet_jit_list_len: sig_len;
    list_closure_any: "jet_jit_list_closure_any" => jet_jit_list_closure_any: sig_closure_predicate;
    list_closure_all: "jet_jit_list_closure_all" => jet_jit_list_closure_all: sig_closure_predicate;
    list_closure_map: "jet_jit_list_closure_map" => jet_jit_list_closure_map: sig_closure_value;
    list_closure_map_mut: "jet_jit_list_closure_map_mut" => jet_jit_list_closure_map_mut: sig_closure_value;
    list_closure_map_i8: "jet_jit_list_closure_map_i8" => jet_jit_list_closure_map_i8: sig_closure_value;
    list_closure_map_i8_mut: "jet_jit_list_closure_map_i8_mut" => jet_jit_list_closure_map_i8_mut: sig_closure_value;
    list_closure_map_i32: "jet_jit_list_closure_map_i32" => jet_jit_list_closure_map_i32: sig_closure_value;
    list_closure_map_i32_mut: "jet_jit_list_closure_map_i32_mut" => jet_jit_list_closure_map_i32_mut: sig_closure_value;
    list_closure_map_f64: "jet_jit_list_closure_map_f64" => jet_jit_list_closure_map_f64: sig_closure_value;
    list_closure_map_f64_mut: "jet_jit_list_closure_map_f64_mut" => jet_jit_list_closure_map_f64_mut: sig_closure_value;
    list_closure_filter: "jet_jit_list_closure_filter" => jet_jit_list_closure_filter: sig_closure_value;
    list_closure_each: "jet_jit_list_closure_each" => jet_jit_list_closure_each: sig_closure_each;
    list_closure_each_mut: "jet_jit_list_closure_each_mut" => jet_jit_list_closure_each_mut: sig_closure_each;
    list_contains_str: "jet_jit_list_contains_str" => jet_jit_list_contains_str: sig_list_eq;
    list_eq: "jet_jit_list_eq" => jet_jit_list_eq: sig_list_eq;
    list_eq_str: "jet_jit_list_eq_str" => jet_jit_list_eq_str: sig_list_eq;
    list_eq_f64: "jet_jit_list_eq_f64" => jet_jit_list_eq_f64: sig_list_eq;
    list_order: "jet_jit_list_order" => jet_jit_list_order: sig_list_eq;
    list_order_str: "jet_jit_list_order_str" => jet_jit_list_order_str: sig_list_eq;
    list_order_f64: "jet_jit_list_order_f64" => jet_jit_list_order_f64: sig_list_eq;
    list_indexes: "jet_jit_list_indexes" => jet_jit_list_indexes: sig_len;
    list_sort: "jet_jit_list_sort" => jet_jit_list_sort: sig_sort;
    list_sort_desc: "jet_jit_list_sort_desc" => jet_jit_list_sort_desc: sig_sort;
    list_sort_str: "jet_jit_list_sort_str" => jet_jit_list_sort_str: sig_sort;
    list_sort_str_desc: "jet_jit_list_sort_str_desc" => jet_jit_list_sort_str_desc: sig_sort;
    list_clone: "jet_jit_list_clone" => jet_jit_list_clone: sig_len;
    list_copy: "jet_jit_list_copy" => jet_jit_list_copy: sig_len;
    list_count: "jet_jit_list_count" => jet_jit_list_count: sig_get_opt;
    list_remove_value: "jet_jit_list_remove_value" => jet_jit_list_remove_value: sig_get_opt;
    list_remove_slot: "jet_jit_list_remove_slot" => jet_jit_list_remove_slot: sig_list_remove_slot;
    list_slice: "jet_jit_list_slice" => jet_jit_list_slice: sig_slice;
    list_starts_with: "jet_jit_list_starts_with" => jet_jit_list_starts_with: sig_list_eq;
    list_ends_with: "jet_jit_list_ends_with" => jet_jit_list_ends_with: sig_list_eq;
    list_equal: "jet_jit_list_equal" => jet_jit_list_equal: sig_list_eq;
    list_binary_search: "jet_jit_list_binary_search" => jet_jit_list_binary_search: sig_get_opt;
    list_union: "jet_jit_list_union" => jet_jit_list_union: sig_get_opt;
    list_intersection: "jet_jit_list_intersection" => jet_jit_list_intersection: sig_get_opt;
    list_difference: "jet_jit_list_difference" => jet_jit_list_difference: sig_get_opt;
    list_random: "jet_jit_list_random" => jet_jit_list_random: sig_len;
    list_min_max: "jet_jit_list_min_max" => jet_jit_list_min_max: sig_len;
    list_replace: "jet_jit_list_replace" => jet_jit_list_replace: sig_three_ret;
    list_range_end: "jet_jit_list_range_end" => jet_jit_list_range_end: sig_range_end;
    split_write: "jet_jit_split_write" => jet_jit_split_write: sig_disjoint;
    get_disjoint_write: "jet_jit_get_disjoint_write" => jet_jit_get_disjoint_write: sig_disjoint;
    range_contains: "jet_jit_range_contains" => jet_jit_range_contains: sig_range_contains;
    range_show: "jet_jit_range_show" => jet_jit_range_show: sig_range_show;
    range_equal: "jet_jit_range_equal" => jet_jit_range_equal: sig_range_equal;
    loop_stride_check: "jet_jit_loop_stride_check" => jet_jit_loop_stride_check: sig_len;
    map_new: "jet_jit_map_new" => jet_jit_map_new: sig_new;
    map_clone: "jet_jit_map_clone" => jet_jit_map_clone: sig_len;
    map_merge: "jet_jit_map_merge" => jet_jit_map_merge: sig_get_opt;
    map_insert: "jet_jit_map_insert" => jet_jit_map_insert: sig_map_insert;
    map_try_insert: "jet_jit_map_try_insert" => jet_jit_map_try_insert: sig_try_map_insert;
    map_increment: "jet_jit_map_increment" => jet_jit_map_increment: sig_push;
    map_get: "jet_jit_map_get" => jet_jit_map_get: sig_map_get;
    map_get_opt: "jet_jit_map_get_opt" => jet_jit_map_get_opt: sig_map_get_opt;
    map_remove: "jet_jit_map_remove" => jet_jit_map_remove: sig_map_get_opt;
    map_len: "jet_jit_map_len" => jet_jit_map_len: sig_len;
    map_key_at: "jet_jit_map_key_at" => jet_jit_map_key_at: sig_map_at;
    map_value_at: "jet_jit_map_value_at" => jet_jit_map_value_at: sig_map_at;
    map_keys: "jet_jit_map_keys" => jet_jit_map_keys: sig_len;
    map_values: "jet_jit_map_values" => jet_jit_map_values: sig_len;
    map_equal: "jet_jit_map_equal" => jet_jit_map_equal: sig_list_eq;
    map_first: "jet_jit_map_first" => jet_jit_map_first: sig_len;
    map_to_list: "jet_jit_map_to_list" => jet_jit_map_to_list: sig_len;
    map_min: "jet_jit_map_min" => jet_jit_map_min: sig_len;
    map_max: "jet_jit_map_max" => jet_jit_map_max: sig_len;
    map_intersection: "jet_jit_map_intersection" => jet_jit_map_intersection: sig_get_opt;
    map_slice: "jet_jit_map_slice" => jet_jit_map_slice: sig_get_opt;
    map_from_keys: "jet_jit_map_from_keys" => jet_jit_map_from_keys: sig_get_opt;
    map_contains_value: "jet_jit_map_contains_value" => jet_jit_map_contains_value: sig_list_eq;
    map_pop_first: "jet_jit_map_pop_first" => jet_jit_map_pop_first: sig_len;
    iter_first: "jet_jit_iter_first" => jet_jit_iter_first: sig_len;
    iter_first_string: "jet_jit_iter_first_string" => jet_jit_iter_first_string: sig_len;
    iter_first_float: "jet_jit_iter_first_float" => jet_jit_iter_first_float: sig_len;
    iter_take: "jet_jit_iter_take" => jet_jit_iter_take: sig_get_opt;
    iter_skip: "jet_jit_iter_skip" => jet_jit_iter_skip: sig_get_opt;
    iter_skip_string: "jet_jit_iter_skip_string" => jet_jit_iter_skip_string: sig_get_opt;
    iter_skip_float: "jet_jit_iter_skip_float" => jet_jit_iter_skip_float: sig_get_opt;
    iter_step_by: "jet_jit_iter_step_by" => jet_jit_iter_step_by: sig_get_opt;
    iter_dedup: "jet_jit_iter_dedup" => jet_jit_iter_dedup: sig_get_opt;
    iter_chunks: "jet_jit_iter_chunks" => jet_jit_iter_chunks: sig_get_opt;
    iter_windows: "jet_jit_iter_windows" => jet_jit_iter_windows: sig_get_opt;
    iter_repeat: "jet_jit_iter_repeat" => jet_jit_iter_repeat: sig_get_opt;
    iter_cycle: "jet_jit_iter_cycle" => jet_jit_iter_cycle: sig_get_opt;
    iter_drop_last: "jet_jit_iter_drop_last" => jet_jit_iter_drop_last: sig_get_opt;
    iter_shuffle: "jet_jit_iter_shuffle" => jet_jit_iter_shuffle: sig_len;
    iter_is_sorted: "jet_jit_iter_is_sorted" => jet_jit_iter_is_sorted: sig_bool;
    iter_last_index_of: "jet_jit_iter_last_index_of" => jet_jit_iter_last_index_of: sig_get_opt;
    iter_average_int: "jet_jit_iter_average_int" => jet_jit_iter_average_int: sig_f64;
    iter_average_float: "jet_jit_iter_average_float" => jet_jit_iter_average_float: sig_f64;
    iter_compare: "jet_jit_iter_compare" => jet_jit_iter_compare: sig_get_opt;
    iter_split: "jet_jit_iter_split" => jet_jit_iter_split: sig_get_opt;
    list_sum_i64: "jet_jit_list_sum_i64" => jet_jit_list_sum_i64: sig_len;
    list_product_i64: "jet_jit_list_product_i64" => jet_jit_list_product_i64: sig_len;
    list_min_i64: "jet_jit_list_min_i64" => jet_jit_list_min_i64: sig_len;
    list_max_i64: "jet_jit_list_max_i64" => jet_jit_list_max_i64: sig_len;
    list_flatten: "jet_jit_list_flatten" => jet_jit_list_flatten: sig_len;
    list_intersperse: "jet_jit_list_intersperse" => jet_jit_list_intersperse: sig_get_opt;
    iter_zip_family: "jet_jit_iter_zip_family" => jet_jit_iter_zip_family: sig_zip_family;
    list_unzip: "jet_jit_list_unzip" => jet_jit_list_unzip: sig_three_ret;
    list_sort_by_i64_keys: "jet_jit_list_sort_by_i64_keys" => jet_jit_list_sort_by_i64_keys: sig_sort_by_keys;
    list_sort_by_i64_keys_desc: "jet_jit_list_sort_by_i64_keys_desc" => jet_jit_list_sort_by_i64_keys_desc: sig_sort_by_keys;
    list_sort_by_str_keys: "jet_jit_list_sort_by_str_keys" => jet_jit_list_sort_by_str_keys: sig_sort_by_keys;
    list_sort_by_str_keys_desc: "jet_jit_list_sort_by_str_keys_desc" => jet_jit_list_sort_by_str_keys_desc: sig_sort_by_keys;
    print_enum: "jet_jit_print_enum" => jet_jit_print_enum: sig_print_enum;
    list_debug: "jet_jit_list_debug" => jet_jit_list_debug: sig_get_opt;
    string_debug: "jet_jit_string_debug" => jet_jit_string_debug: sig_len;
    scalar_debug: "jet_jit_scalar_debug" => jet_jit_scalar_debug: sig_scalar_debug;
    enum_show: "jet_jit_enum_show" => jet_jit_enum_show: sig_enum_show;
    str_push_debug_optional: "jet_jit_str_push_debug_optional" => jet_jit_str_push_debug_optional: sig_debug_optional;
    str_push_debug_record: "jet_jit_str_push_debug_record" => jet_jit_str_push_debug_record: sig_debug_record;
    str_push_debug_variant: "jet_jit_str_push_debug_variant" => jet_jit_str_push_debug_variant: sig_debug_variant;
    list_pop: "jet_jit_list_pop" => jet_jit_list_pop: sig_len;
    list_insert: "jet_jit_list_insert" => jet_jit_list_insert: sig_map_insert;
    set_from_list: "jet_jit_set_from_list" => jet_jit_set_from_list: sig_set_from;
    set_new: "jet_jit_set_new" => jet_jit_set_new: sig_sorted_set_new;
    set_insert: "jet_jit_set_insert" => jet_jit_set_insert: sig_list_eq;
    set_remove: "jet_jit_set_remove" => jet_jit_set_remove: sig_push;
    set_has: "jet_jit_set_has" => jet_jit_set_has: sig_list_eq;
    set_len: "jet_jit_set_len" => jet_jit_set_len: sig_len;
    set_to_list: "jet_jit_set_to_list" => jet_jit_set_to_list: sig_len;
    set_copy: "jet_jit_set_copy" => jet_jit_set_copy: sig_len;
    set_equal: "jet_jit_set_equal" => jet_jit_set_equal: sig_list_eq;
    set_capacity: "jet_jit_set_capacity" => jet_jit_set_capacity: sig_len;
    set_first: "jet_jit_set_first" => jet_jit_set_first: sig_len;
    set_pop: "jet_jit_set_pop" => jet_jit_set_pop: sig_get_opt;
    set_union: "jet_jit_set_union" => jet_jit_set_union: sig_get_opt;
    set_intersection: "jet_jit_set_intersection" => jet_jit_set_intersection: sig_get_opt;
    set_difference: "jet_jit_set_difference" => jet_jit_set_difference: sig_get_opt;
    set_symmetric_difference: "jet_jit_set_symmetric_difference" => jet_jit_set_symmetric_difference: sig_get_opt;
    set_is_subset: "jet_jit_set_is_subset" => jet_jit_set_is_subset: sig_list_eq;
    set_is_superset: "jet_jit_set_is_superset" => jet_jit_set_is_superset: sig_list_eq;
    set_is_disjoint: "jet_jit_set_is_disjoint" => jet_jit_set_is_disjoint: sig_list_eq;
    deque_new: "jet_jit_deque_new" => jet_jit_deque_new: sig_new;
    deque_push_front: "jet_jit_deque_push_front" => jet_jit_deque_push_front: sig_push;
    deque_push_back: "jet_jit_deque_push_back" => jet_jit_deque_push_back: sig_push;
    deque_pop_front: "jet_jit_deque_pop_front" => jet_jit_deque_pop_front: sig_len;
    deque_pop_back: "jet_jit_deque_pop_back" => jet_jit_deque_pop_back: sig_len;
    deque_peek_front: "jet_jit_deque_peek_front" => jet_jit_deque_peek_front: sig_len;
    deque_peek_back: "jet_jit_deque_peek_back" => jet_jit_deque_peek_back: sig_len;
    deque_len: "jet_jit_deque_len" => jet_jit_deque_len: sig_len;
    deque_capacity: "jet_jit_deque_capacity" => jet_jit_deque_capacity: sig_len;
    deque_contains: "jet_jit_deque_contains" => jet_jit_deque_contains: sig_list_eq;
    deque_get: "jet_jit_deque_get" => jet_jit_deque_get: sig_get_opt;
    deque_delete: "jet_jit_deque_delete" => jet_jit_deque_delete: sig_push;
    deque_to_list: "jet_jit_deque_to_list" => jet_jit_deque_to_list: sig_len;
    deque_join: "jet_jit_deque_join" => jet_jit_deque_join: sig_join;
    deque_reverse: "jet_jit_deque_reverse" => jet_jit_deque_reverse: sig_len;
    deque_split: "jet_jit_deque_split" => jet_jit_deque_split: sig_get_opt;
    deque_from: "jet_jit_deque_from" => jet_jit_deque_from: sig_len;
    bag_new: "jet_jit_bag_new" => jet_jit_bag_new: sig_new;
    bag_add: "jet_jit_bag_add" => jet_jit_bag_add: sig_list_eq;
    bag_remove: "jet_jit_bag_remove" => jet_jit_bag_remove: sig_push;
    bag_has: "jet_jit_bag_has" => jet_jit_bag_has: sig_list_eq;
    bag_count: "jet_jit_bag_count" => jet_jit_bag_count: sig_get_opt;
    bag_len: "jet_jit_bag_len" => jet_jit_bag_len: sig_len;
    sorted_set_new: "jet_jit_sorted_set_new" => jet_jit_sorted_set_new: sig_sorted_set_new;
    sorted_set_len: "jet_jit_sorted_set_len" => jet_jit_sorted_set_len: sig_len;
    sorted_set_has: "jet_jit_sorted_set_has" => jet_jit_sorted_set_has: sig_list_eq;
    sorted_set_from: "jet_jit_sorted_set_from" => jet_jit_sorted_set_from: sig_set_from;
    sorted_set_insert: "jet_jit_sorted_set_insert" => jet_jit_sorted_set_insert: sig_list_eq;
    sorted_set_remove: "jet_jit_sorted_set_remove" => jet_jit_sorted_set_remove: sig_push;
    sorted_set_to_list: "jet_jit_sorted_set_to_list" => jet_jit_sorted_set_to_list: sig_len;
    sorted_set_first: "jet_jit_sorted_set_first" => jet_jit_sorted_set_first: sig_len;
    sorted_set_last: "jet_jit_sorted_set_last" => jet_jit_sorted_set_last: sig_len;
    sorted_set_union: "jet_jit_sorted_set_union" => jet_jit_sorted_set_union: sig_get_opt;
    sorted_set_intersection: "jet_jit_sorted_set_intersection" => jet_jit_sorted_set_intersection: sig_get_opt;
    sorted_set_difference: "jet_jit_sorted_set_difference" => jet_jit_sorted_set_difference: sig_get_opt;
    sorted_set_symmetric_difference: "jet_jit_sorted_set_symmetric_difference" => jet_jit_sorted_set_symmetric_difference: sig_get_opt;
    sorted_set_is_subset: "jet_jit_sorted_set_is_subset" => jet_jit_sorted_set_is_subset: sig_list_eq;
    sorted_set_is_superset: "jet_jit_sorted_set_is_superset" => jet_jit_sorted_set_is_superset: sig_list_eq;
    sorted_set_is_disjoint: "jet_jit_sorted_set_is_disjoint" => jet_jit_sorted_set_is_disjoint: sig_list_eq;
    priority_queue_new: "jet_jit_priority_queue_new" => jet_jit_priority_queue_new: sig_new;
    priority_queue_len: "jet_jit_priority_queue_len" => jet_jit_priority_queue_len: sig_len;
    priority_queue_from: "jet_jit_priority_queue_from" => jet_jit_priority_queue_from: sig_len;
    priority_queue_push: "jet_jit_priority_queue_push" => jet_jit_priority_queue_push: sig_push;
    priority_queue_peek: "jet_jit_priority_queue_peek" => jet_jit_priority_queue_peek: sig_len;
    priority_queue_pop: "jet_jit_priority_queue_pop" => jet_jit_priority_queue_pop: sig_len;
    priority_queue_to_sorted_list: "jet_jit_priority_queue_to_sorted_list" => jet_jit_priority_queue_to_sorted_list: sig_len;
    priority_queue_remove_value: "jet_jit_priority_queue_remove_value" => jet_jit_priority_queue_remove_value: sig_get_opt;
    priority_queue_remove_slot: "jet_jit_priority_queue_remove_slot" => jet_jit_priority_queue_remove_slot: sig_priority_queue_slot;
    lru_new: "jet_jit_lru_new" => jet_jit_lru_new: sig_len;
    lru_put: "jet_jit_lru_put" => jet_jit_lru_put: sig_three_ret;
    lru_get: "jet_jit_lru_get" => jet_jit_lru_get: sig_get_opt;
    lru_has: "jet_jit_lru_has" => jet_jit_lru_has: sig_list_eq;
    lru_keys: "jet_jit_lru_keys" => jet_jit_lru_keys: sig_len;
    bit_set_new: "jet_jit_bit_set_new" => jet_jit_bit_set_new: sig_new;
    bit_set_add: "jet_jit_bit_set_add" => jet_jit_bit_set_add: sig_list_eq;
    bit_set_has: "jet_jit_bit_set_has" => jet_jit_bit_set_has: sig_list_eq;
    bit_set_remove: "jet_jit_bit_set_remove" => jet_jit_bit_set_remove: sig_push;
    bit_set_to_list: "jet_jit_bit_set_to_list" => jet_jit_bit_set_to_list: sig_len;
    bit_set_copy: "jet_jit_bit_set_copy" => jet_jit_bit_set_copy: sig_len;
    bit_set_len: "jet_jit_bit_set_len" => jet_jit_bit_set_len: sig_len;
    bit_set_count: "jet_jit_bit_set_count" => jet_jit_bit_set_count: sig_len;
    byte_buffer_new: "jet_jit_byte_buffer_new" => jet_jit_byte_buffer_new: sig_new;
    byte_buffer_with_capacity: "jet_jit_byte_buffer_with_capacity" => jet_jit_byte_buffer_with_capacity: sig_len;
    byte_buffer_from: "jet_jit_byte_buffer_from" => jet_jit_byte_buffer_from: sig_len;
    byte_buffer_write: "jet_jit_byte_buffer_write" => jet_jit_byte_buffer_write: sig_map_insert;
    byte_buffer_to_bytes: "jet_jit_byte_buffer_to_bytes" => jet_jit_byte_buffer_to_bytes: sig_len;
    byte_buffer_method: "jet_jit_byte_buffer_method" => jet_jit_byte_buffer_method: sig_four_ret;
}
