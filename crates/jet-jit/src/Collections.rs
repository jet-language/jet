//! M5: list/map host shims for the Cranelift JIT (`JetArena` handles).

use super::Concurrency;
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
    include!("../../jet-codegen/src/Prelude/Core/ByteBuffer.rs");
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
#[allow(dead_code, unused_imports)]
mod collection_semantics {
    pub use jet_foundation::Outcome::*;

    trait JetShow {
        fn jet_show(&self) -> String;
    }
    trait JetDisplay {
        fn jet_display(&self) -> String;
    }
    trait JetDebug {
        fn jet_debug(&self) -> String;
    }

    macro_rules! jet_scalar_show {
        ($($t:ty),+ $(,)?) => {
            $(
                impl JetShow for $t {
                    fn jet_show(&self) -> String { self.to_string() }
                }
                impl JetDisplay for $t {
                    fn jet_display(&self) -> String { self.to_string() }
                }
                impl JetDebug for $t {
                    fn jet_debug(&self) -> String { self.to_string() }
                }
            )+
        };
    }
    jet_scalar_show!(i64, i8, i16, i32, u8, u16, u32, u64, bool, char);

    impl JetShow for f32 {
        fn jet_show(&self) -> String { format!("{self:?}") }
    }
    impl JetDisplay for f32 {
        fn jet_display(&self) -> String { format!("{self:?}") }
    }
    impl JetDebug for f32 {
        fn jet_debug(&self) -> String { format!("{self:?}") }
    }
    impl JetShow for f64 {
        fn jet_show(&self) -> String { format!("{self:?}") }
    }
    impl JetDisplay for f64 {
        fn jet_display(&self) -> String { format!("{self:?}") }
    }
    impl JetDebug for f64 {
        fn jet_debug(&self) -> String { format!("{self:?}") }
    }
    impl JetShow for String {
        fn jet_show(&self) -> String { self.clone() }
    }
    impl JetDisplay for String {
        fn jet_display(&self) -> String { self.clone() }
    }
    impl JetDebug for String {
        fn jet_debug(&self) -> String { format!("{self:?}") }
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

    fn jet_panic(_file: &str, _line: u32, msg: &str) -> ! {
        super::Concurrency::with_runtime_mut(|rt| rt.set_trap(msg));
        std::process::exit(70)
    }

    include!("../../jet-codegen/src/Prelude/Core/Collections.rs");

    pub(super) fn iter_take<T: 'static>(xs: Vec<T>, n: i64) -> Vec<T> {
        jet_iter_take(jet_iter_from_vec(xs), n).to_list()
    }

    pub(super) fn iter_skip<T: 'static>(xs: Vec<T>, n: i64) -> Vec<T> {
        jet_iter_skip(jet_iter_from_vec(xs), n).to_list()
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

    pub(super) fn iter_zip_i64(left: Vec<i64>, right: Vec<i64>) -> Vec<(i64, i64)> {
        jet_iter_zip(jet_iter_from_vec(left), jet_iter_from_vec(right), |a, b| (a, b)).to_list()
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
        jet_map_remove_kernel(&mut map, &key).ok()
    }

    pub(super) fn map_pop_first_i64(
        entries: Vec<(String, i64)>,
    ) -> (Option<String>, Option<i64>, Vec<(String, i64)>) {
        let mut map = map_from_pairs(entries);
        let key = map.keys().next().cloned();
        let value = jet_map_pop_first_kernel(&mut map).ok();
        (key, value, map_pairs(&map))
    }

    pub(super) fn iter_zip_many_i64(
        columns: Vec<Vec<i64>>,
        mode: u8,
        fills: Vec<i64>,
    ) -> Option<Vec<Vec<i64>>> {
        jet_iter_zip_many(columns, mode, |column| {
            fills.get(column).copied().unwrap_or_default()
        })
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

    pub(super) fn list_replace<T: Clone + PartialEq>(xs: &[T], old: &T, new: T) -> Vec<T> {
        jet_list_replace(xs, old, new)
    }

    pub(super) fn list_starts_with<T: PartialEq>(xs: &[T], prefix: &[T]) -> bool {
        jet_list_starts_with(xs, prefix)
    }

    pub(super) fn list_ends_with<T: PartialEq>(xs: &[T], suffix: &[T]) -> bool {
        jet_list_ends_with(xs, suffix)
    }

    pub(super) fn list_unzip_i64(xs: Vec<(i64, i64)>) -> (Vec<i64>, Vec<i64>) {
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

fn option_i64(rt: &mut crate::JitRuntime, value: Option<i64>) -> i64 {
    crate::runtime_host::alloc_jit_result(
        rt,
        value.is_some(),
        value.unwrap_or_default() as u64,
    )
}

/// Packed Option ABI for `Option(Int)`: 0 = None, value+1 = Some.
fn option_packed(value: Option<i64>) -> i64 {
    match value {
        Some(v) => v.wrapping_add(1),
        None => 0,
    }
}

/// Record an out-of-bounds trap. Returns normally; JIT code branches to its
/// epilogue at the next `emit_trap_check` (I1 — no Rust panic ever unwinds
/// through a JIT frame; cranelift-jit emits no unwind tables for them).
fn trap_index() {
    Concurrency::with_runtime_mut(|rt| {
        rt.set_trap("index out of bounds: the index is outside the list");
    });
}

extern "C" fn jet_jit_list_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_list())
}

extern "C" fn jet_jit_list_uninit(len: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_uninit_list(len.max(0) as usize))
}

/// `core.io.args()` — List(String) matching AOT `jet_std_io_args`, fed by the
/// `with_program_args` argv installed for this JIT run (falls back to
/// `std::env::args` when unset, same as a bare host process).
extern "C" fn jet_jit_io_args() -> i64 {
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
                .expect("jit io.args push");
        }
        list
    })
}

extern "C" fn jet_jit_list_push(list: i64, v: i64) {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .list_push_int(list, v)
            .expect("jit list push: bad handle");
    });
}

extern "C" fn jet_jit_list_push_f64(list: i64, v: f64) {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .list_push_float(list, v)
            .expect("jit list push f64: bad handle");
    });
}

extern "C" fn jet_jit_list_push_range(
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

extern "C" fn jet_jit_list_len(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.list_len(list).expect("jit list len: bad handle"))
}

extern "C" fn jet_jit_list_contains_str(list: i64, needle: i64) -> i8 {
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
extern "C" fn jet_jit_list_eq(a: i64, b: i64) -> i8 {
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

/// Mirror AOT `jet_iter_indexes(n)` — materialize `Iter<Int>` as a list handle.
extern "C" fn jet_jit_list_indexes(n: i64) -> i64 {
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

extern "C" fn jet_jit_loop_stride_check(stride: i64) -> i64 {
    if stride <= 0 {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("E0123: a source loop stride must be positive");
        });
    }
    stride
}

extern "C" fn jet_jit_list_get(list: i64, idx: i64, _line: u32) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.list_get_int(list, idx) {
        Some(value) => value,
        None => {
            if rt.heap.list_len(list).is_none() {
                jet_foundation::ice!(None, "jit list get: bad handle");
            }
            rt.set_trap("index out of bounds: the index is outside the list");
            0
        }
    })
}

extern "C" fn jet_jit_list_get_f64(list: i64, idx: i64, _line: u32) -> f64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.list_get_float(list, idx) {
        Some(value) => value,
        None => {
            if rt.heap.list_len(list).is_none() {
                jet_foundation::ice!(None, "jit list get f64: bad handle");
            }
            rt.set_trap("index out of bounds: the index is outside the list");
            0.0
        }
    })
}

extern "C" fn jet_jit_fixed_list_get(list: i64, idx: i64, _line: u32) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt
            .heap
            .list_len(list)
            .expect("jit fixed-list index: bad handle");
        match jet_codegen::fixed_list::jet_fixed_list_index(len, idx, |position| {
            rt.heap.list_get_int(list, position as i64).unwrap_or_default()
        }) {
            Ok(value) => value,
            Err(error) => {
                rt.set_trap(&error.message());
                0
            }
        }
    })
}

extern "C" fn jet_jit_fixed_list_get_f64(list: i64, idx: i64, _line: u32) -> f64 {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt
            .heap
            .list_len(list)
            .expect("jit fixed-list index f64: bad handle");
        match jet_codegen::fixed_list::jet_fixed_list_index(len, idx, |position| {
            rt.heap
                .list_get_float(list, position as i64)
                .unwrap_or_default()
        }) {
            Ok(value) => value,
            Err(error) => {
                rt.set_trap(&error.message());
                0.0
            }
        }
    })
}

fn jet_jit_list_get_range(list: i64, idx: i64) -> (i64, i64, bool) {
    Concurrency::with_runtime_mut(|rt| match rt.heap.list_get_range(list, idx) {
        Some(value) => value,
        None => {
            if rt.heap.list_len(list).is_none() {
                jet_foundation::ice!(None, "jit list get Range: bad handle");
            }
            rt.set_trap("index out of bounds: the index is outside the list");
            (0, 0, false)
        }
    })
}

extern "C" fn jet_jit_list_get_range_start(list: i64, idx: i64, _line: u32) -> i64 {
    jet_jit_list_get_range(list, idx).0
}

extern "C" fn jet_jit_list_get_range_end(list: i64, idx: i64, _line: u32) -> i64 {
    jet_jit_list_get_range(list, idx).1
}

extern "C" fn jet_jit_list_get_range_exclusive(list: i64, idx: i64, _line: u32) -> i8 {
    i8::from(jet_jit_list_get_range(list, idx).2)
}

/// `0` = absent; otherwise `value + 1`.
extern "C" fn jet_jit_list_get_opt(list: i64, idx: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if rt.heap.list_len(list).is_none() {
            jet_foundation::ice!(None, "jit list get_opt: bad handle");
        }
        rt.heap.list_get_int(list, idx).map(|v| v + 1).unwrap_or(0)
    })
}

extern "C" fn jet_jit_list_set(list: i64, idx: i64, v: i64, _line: u32) {
    Concurrency::with_runtime_mut(|rt| {
        if rt.heap.list_len(list).is_none() {
            jet_foundation::ice!(None, "jit list set: bad handle");
        }
        if rt.heap.list_set_int(list, idx, v).is_none() {
            trap_index();
        }
    });
}

extern "C" fn jet_jit_list_set_f64(list: i64, idx: i64, v: f64, _line: u32) {
    Concurrency::with_runtime_mut(|rt| {
        if rt.heap.list_len(list).is_none() {
            jet_foundation::ice!(None, "jit list set f64: bad handle");
        }
        if rt.heap.list_set_float(list, idx, v).is_none() {
            trap_index();
        }
    });
}

extern "C" fn jet_jit_list_sort(list: i64) {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .list_sort_int(list)
            .expect("jit list sort: bad handle")
    });
}

/// Lexicographic sort of a `[String]` list (handles are string arena ids).
extern "C" fn jet_jit_list_sort_str(list: i64) {
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

extern "C" fn jet_jit_list_clone(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_list(list).expect("jit list clone: bad handle"))
}

extern "C" fn jet_jit_list_copy(list: i64) -> i64 {
    let values = clone_list_ints(list);
    alloc_from_ints(&collection_semantics::list_copy_i64(&values))
}

extern "C" fn jet_jit_list_slice(list: i64, start: i64, end: i64, _line: u32) -> i64 {
    let xs = clone_list_ints(list);
    let out = collection_semantics::list_slice(&xs, start, end);
    alloc_from_ints(&out)
}

extern "C" fn jet_jit_list_range_end(
    list: i64,
    start: i64,
    end: i64,
    exclusive: i64,
    _line: u32,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(len) = rt.heap.list_len(list) else {
            jet_foundation::ice!(None, "jit Range window: bad list handle");
        };
        match range_semantics::jet_range_bounds(start, end, exclusive != 0, len) {
            Some((_, end_exclusive)) => end_exclusive,
            None => {
                rt.set_trap(&format!(
                    "can't view {len} items from {start} to {end} ({})",
                    if exclusive != 0 {
                        "exclusive"
                    } else {
                        "inclusive"
                    }
                ));
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

extern "C" fn jet_jit_split_write(list: i64, mid: i64) -> i64 {
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

extern "C" fn jet_jit_get_disjoint_write(list: i64, targets: i64) -> i64 {
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

extern "C" fn jet_jit_range_contains(
    start: i64,
    end: i64,
    exclusive: i64,
    value: i64,
) -> i8 {
    range_semantics::jet_range_contains(start, end, exclusive != 0, value) as i8
}

extern "C" fn jet_jit_range_show(start: i64, end: i64, exclusive: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap.alloc_string(range_semantics::jet_range_structural_text(
            start,
            end,
            exclusive != 0,
        ))
    })
}

extern "C" fn jet_jit_range_equal(
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

extern "C" fn jet_jit_list_join_str(list: i64, sep_id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(xs) = rt.heap.clone_int_list(list) else {
            rt.set_trap("list join received an invalid list");
            return 0;
        };
        let Some(sep) = rt.heap.clone_string(sep_id) else {
            rt.set_trap("list join received an invalid separator");
            return 0;
        };
        // Match AOT JoinSep: `iter().map(|x| x.jet_show()).collect::<Vec<_>>().join(sep)`.
        // String elements are heap handles; Int (and other non-string carriers) show as
        // decimal — never trap. AOT already accepts `[Int].join(",")`.
        let parts: Vec<String> = xs
            .iter()
            .map(|id| {
                rt.heap
                    .clone_string(*id)
                    .unwrap_or_else(|| id.to_string())
            })
            .collect();
        let joined = parts.join(&sep);
        rt.heap.alloc_string(joined)
    })
}

extern "C" fn jet_jit_map_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_map())
}

extern "C" fn jet_jit_map_clone(map: i64) -> i64 {
    alloc_map_pairs(&collection_semantics::map_copy_i64(clone_map_pairs(map)))
}

extern "C" fn jet_jit_map_equal(left: i64, right: i64) -> i8 {
    i8::from(collection_semantics::map_equal_i64(
        clone_map_pairs(left),
        clone_map_pairs(right),
    ))
}

extern "C" fn jet_jit_map_first(map: i64) -> i64 {
    let value = collection_semantics::map_first_key_i64(clone_map_pairs(map));
    Concurrency::with_runtime_mut(|rt| {
        let value = value.map(|key| rt.heap.alloc_string(key));
        option_i64(rt, value)
    })
}

extern "C" fn jet_jit_map_to_list(map: i64) -> i64 {
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

extern "C" fn jet_jit_map_min(map: i64) -> i64 {
    let value = collection_semantics::map_min_i64(clone_map_pairs(map));
    Concurrency::with_runtime_mut(|rt| option_i64(rt, value))
}

extern "C" fn jet_jit_map_max(map: i64) -> i64 {
    let value = collection_semantics::map_max_i64(clone_map_pairs(map));
    Concurrency::with_runtime_mut(|rt| option_i64(rt, value))
}

extern "C" fn jet_jit_map_intersection(left: i64, right: i64) -> i64 {
    let pairs = collection_semantics::map_intersection_i64(
        clone_map_pairs(left),
        clone_map_pairs(right),
    );
    alloc_map_pairs(&pairs)
}

extern "C" fn jet_jit_map_slice(map: i64, keys: i64) -> i64 {
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

extern "C" fn jet_jit_map_from_keys(keys: i64, default: i64) -> i64 {
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

extern "C" fn jet_jit_map_contains_value(map: i64, needle: i64) -> i8 {
    i8::from(collection_semantics::map_contains_i64(
        clone_map_pairs(map),
        needle,
    ))
}

extern "C" fn jet_jit_map_pop_first(map: i64) -> i64 {
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
extern "C" fn jet_jit_map_merge(left: i64, right: i64) -> i64 {
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

extern "C" fn jet_jit_map_insert(map: i64, key: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .map_insert(map, key, value)
            .expect("jit map insert: bad handle");
    });
}

extern "C" fn jet_jit_map_increment(map: i64, key: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let current = rt.heap.map_get(map, key).unwrap_or(0);
        let Some(next) = current.checked_add(1) else {
            rt.set_trap("integer overflow");
            return;
        };
        let _ = rt.heap.map_insert(map, key, next);
    });
}

extern "C" fn jet_jit_map_get(map: i64, key: i64, _line: u32) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.map_get(map, key) {
        Some(value) => value,
        None => {
            if rt.heap.map_len(map).is_none() {
                jet_foundation::ice!(None, "jit map get: bad handle");
            }
            rt.set_trap("the map has no entry for this key");
            0
        }
    })
}

extern "C" fn jet_jit_map_validate(map: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if rt.heap.map_len(map).is_some() {
            map
        } else {
            rt.set_trap("data object payload is not a map");
            0
        }
    })
}

/// Result-arena Option handle (`result_is_ok` / `result_get_i64`).
extern "C" fn jet_jit_map_get_opt(map: i64, key: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if rt.heap.map_len(map).is_none() {
            jet_foundation::ice!(None, "jit map get_opt: bad handle");
        }
        let value = rt.heap.map_get(map, key);
        option_i64(rt, value)
    })
}

extern "C" fn jet_jit_map_remove(map: i64, key: i64) -> i64 {
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

extern "C" fn jet_jit_map_len(map: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.map_len(map).expect("jit map len: bad handle"))
}

extern "C" fn jet_jit_map_key_at(map: i64, idx: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .map_key_at(map, idx)
            .expect("jit map key_at: bad handle")
    })
}

extern "C" fn jet_jit_map_value_at(map: i64, idx: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .map_value_at(map, idx)
            .expect("jit map value_at: bad handle")
    })
}

extern "C" fn jet_jit_map_keys(map: i64) -> i64 {
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

extern "C" fn jet_jit_map_values(map: i64) -> i64 {
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

fn clone_map_pairs(map: i64) -> Vec<(String, i64)> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.map_len(map).expect("jit map adapter: bad handle");
        (0..len)
            .map(|index| {
                let key_id = rt
                    .heap
                    .map_key_at(map, index)
                    .expect("jit map adapter: key");
                let key = rt
                    .heap
                    .clone_string(key_id)
                    .expect("jit map adapter: string key");
                let value = rt
                    .heap
                    .map_value_at(map, index)
                    .expect("jit map adapter: value");
                (key, value)
            })
            .collect()
    })
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

extern "C" fn jet_jit_iter_take(list: i64, n: i64) -> i64 {
    let xs = clone_list_ints(list);
    let out = collection_semantics::iter_take(xs, n);
    transfer_progress_take(list, alloc_from_ints(&out), n)
}

extern "C" fn jet_jit_iter_skip(list: i64, n: i64) -> i64 {
    let xs = clone_list_ints(list);
    let out = collection_semantics::iter_skip(xs, n);
    transfer_progress_skip(list, alloc_from_ints(&out), n)
}

extern "C" fn jet_jit_iter_step_by(list: i64, n: i64) -> i64 {
    let xs = clone_list_ints(list);
    let out = collection_semantics::iter_step_by(xs, n);
    transfer_progress_step(list, alloc_from_ints(&out), n)
}

/// `string_elems != 0` → compare string contents (handles may differ); else i64 eq.
extern "C" fn jet_jit_iter_dedup(list: i64, string_elems: i64) -> i64 {
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

extern "C" fn jet_jit_iter_chunks(list: i64, n: i64) -> i64 {
    let xs = clone_list_ints(list);
    let out = alloc_nested_from_ints(&collection_semantics::iter_chunks(xs, n));
    crate::IO::progress_transfer_chunks_state(list, out, n);
    out
}

extern "C" fn jet_jit_iter_windows(list: i64, n: i64) -> i64 {
    let xs = clone_list_ints(list);
    let out = alloc_nested_from_ints(&collection_semantics::iter_windows(xs, n));
    crate::IO::progress_transfer_windows_state(list, out, n);
    out
}

extern "C" fn jet_jit_list_sum_i64(list: i64) -> i64 {
    let values = clone_list_ints(list);
    collection_semantics::list_sum_i64(values)
}

extern "C" fn jet_jit_list_product_i64(list: i64) -> i64 {
    collection_semantics::list_product_i64(clone_list_ints(list))
}

extern "C" fn jet_jit_list_min_i64(list: i64) -> i64 {
    let value = collection_semantics::list_min_i64(clone_list_ints(list)).ok();
    Concurrency::with_runtime_mut(|rt| option_i64(rt, value))
}

extern "C" fn jet_jit_list_max_i64(list: i64) -> i64 {
    let value = collection_semantics::list_max_i64(clone_list_ints(list)).ok();
    Concurrency::with_runtime_mut(|rt| option_i64(rt, value))
}

extern "C" fn jet_jit_list_flatten(list: i64) -> i64 {
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

extern "C" fn jet_jit_list_intersperse(list: i64, separator: i64) -> i64 {
    let out = alloc_from_ints(&collection_semantics::list_intersperse_i64(
        clone_list_ints(list),
        separator,
    ));
    crate::IO::progress_transfer_intersperse_state(list, out);
    out
}

extern "C" fn jet_jit_list_zip(left: i64, right: i64) -> i64 {
    let (left_values, right_values) = Concurrency::with_runtime_mut(|rt| {
        (
            rt.heap.clone_int_list(left).unwrap_or_default(),
            rt.heap.clone_int_list(right).unwrap_or_default(),
        )
    });
    let pairs = collection_semantics::iter_zip_i64(left_values, right_values);
    let out = Concurrency::with_runtime_mut(|rt| {
        let out = rt.heap.alloc_empty_list();
        for (a, b) in pairs {
            let pair = rt.heap.alloc_record(2);
            let _ = rt.heap.record_set_int(pair, 0, a);
            let _ = rt.heap.record_set_int(pair, 1, b);
            let _ = rt.heap.list_push_int(out, pair);
        }
        out
    });
    crate::IO::progress_transfer_zip_state(left, right, out);
    out
}

extern "C" fn jet_jit_list_unzip(pairs: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let pairs = rt.heap.clone_int_list(pairs).unwrap_or_default();
        let pairs = pairs
            .into_iter()
            .filter_map(|pair| {
                Some((
                    rt.heap.record_get_int(pair, 0)?,
                    rt.heap.record_get_int(pair, 1)?,
                ))
            })
            .collect::<Vec<_>>();
        let (left_values, right_values) = collection_semantics::list_unzip_i64(pairs);
        let left = rt.heap.alloc_empty_list();
        for value in left_values {
            let _ = rt.heap.list_push_int(left, value);
        }
        let right = rt.heap.alloc_empty_list();
        for value in right_values {
            let _ = rt.heap.list_push_int(right, value);
        }
        let result = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(result, 0, left);
        let _ = rt.heap.record_set_int(result, 1, right);
        result
    })
}

extern "C" fn jet_jit_list_starts_with(list: i64, prefix: i64) -> i8 {
    collection_semantics::list_starts_with(&clone_list_ints(list), &clone_list_ints(prefix)) as i8
}

extern "C" fn jet_jit_list_ends_with(list: i64, suffix: i64) -> i8 {
    collection_semantics::list_ends_with(&clone_list_ints(list), &clone_list_ints(suffix)) as i8
}

extern "C" fn jet_jit_iter_repeat(list: i64, n: i64) -> i64 {
    alloc_from_ints(&collection_semantics::iter_repeat(clone_list_ints(list), n))
}

extern "C" fn jet_jit_iter_cycle(list: i64, n: i64) -> i64 {
    alloc_from_ints(&collection_semantics::iter_cycle(clone_list_ints(list), n))
}

extern "C" fn jet_jit_iter_drop_last(list: i64, n: i64) -> i64 {
    alloc_from_ints(&collection_semantics::iter_drop_last(clone_list_ints(list), n))
}

extern "C" fn jet_jit_iter_shuffle(list: i64) -> i64 {
    alloc_from_ints(&collection_semantics::iter_shuffle(clone_list_ints(list)))
}

extern "C" fn jet_jit_iter_is_sorted(list: i64) -> i8 {
    collection_semantics::iter_is_sorted(clone_list_ints(list)) as i8
}

extern "C" fn jet_jit_iter_last_index_of(list: i64, needle: i64) -> i64 {
    let value = collection_semantics::iter_last_index_of(clone_list_ints(list), needle).ok();
    Concurrency::with_runtime_mut(|rt| option_i64(rt, value))
}

extern "C" fn jet_jit_iter_average_int(list: i64) -> f64 {
    collection_semantics::iter_average_int(clone_list_ints(list))
}

extern "C" fn jet_jit_iter_average_float(list: i64) -> f64 {
    let values = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        (0..len)
            .map(|index| rt.heap.list_get_float(list, index).unwrap_or_default())
            .collect::<Vec<_>>()
    });
    collection_semantics::iter_average_float(values)
}

extern "C" fn jet_jit_iter_compare(list: i64, other: i64) -> i64 {
    collection_semantics::iter_compare(clone_list_ints(list), clone_list_ints(other))
}

extern "C" fn jet_jit_iter_split(list: i64, n: i64) -> i64 {
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

extern "C" fn jet_jit_list_equal(left: i64, right: i64) -> i8 {
    collection_semantics::list_equal(&clone_list_ints(left), &clone_list_ints(right)) as i8
}

extern "C" fn jet_jit_list_binary_search(list: i64, needle: i64) -> i64 {
    let value = collection_semantics::list_binary_search(&clone_list_ints(list), &needle).ok();
    Concurrency::with_runtime_mut(|rt| option_i64(rt, value))
}

extern "C" fn jet_jit_list_union(left: i64, right: i64) -> i64 {
    alloc_from_ints(&collection_semantics::list_union(
        &clone_list_ints(left),
        &clone_list_ints(right),
    ))
}

extern "C" fn jet_jit_list_intersection(left: i64, right: i64) -> i64 {
    alloc_from_ints(&collection_semantics::list_intersection(
        &clone_list_ints(left),
        &clone_list_ints(right),
    ))
}

extern "C" fn jet_jit_list_difference(left: i64, right: i64) -> i64 {
    alloc_from_ints(&collection_semantics::list_difference(
        &clone_list_ints(left),
        &clone_list_ints(right),
    ))
}

extern "C" fn jet_jit_list_random(list: i64) -> i64 {
    let value = collection_semantics::list_random(&clone_list_ints(list)).ok();
    Concurrency::with_runtime_mut(|rt| option_i64(rt, value))
}

extern "C" fn jet_jit_list_min_max(list: i64) -> i64 {
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

extern "C" fn jet_jit_list_replace(list: i64, old: i64, new: i64) -> i64 {
    alloc_from_ints(&collection_semantics::list_replace(
        &clone_list_ints(list),
        &old,
        new,
    ))
}

/// Stable sort `list` in place by parallel i64 `keys` (same length).
extern "C" fn jet_jit_list_sort_by_i64_keys(list: i64, keys: i64) {
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
        order.sort_by_key(|&i| keys[i]);
        for (dst, src) in order.into_iter().enumerate() {
            rt.heap
                .list_set_int(list, dst as i64, xs[src])
                .expect("jit sort_by: set");
        }
    });
}

/// Stable sort `list` by parallel string-handle keys (Jet `String` heap ids).
extern "C" fn jet_jit_list_sort_by_str_keys(list: i64, keys: i64) {
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
        order.sort_by(|&a, &b| key_strs[a].cmp(&key_strs[b]));
        for (dst, src) in order.into_iter().enumerate() {
            rt.heap
                .list_set_int(list, dst as i64, xs[src])
                .expect("jit sort_by_str: set");
        }
    });
}

/// Print `[T]` / materialized `Iter<T>` with the same `jet_show` shape AOT uses.
/// `kind`: 0 = raw i64, 1 = string, 2 = signed IntN, 3 = unsigned IntN.
extern "C" fn jet_jit_print_list(list: i64, kind: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let text = list_show_text(rt, list, kind);
        rt.stdout.push_str(&text);
        rt.stdout.push('\n');
    });
}

fn list_show_text(rt: &crate::JitRuntime, list: i64, kind: i64) -> String {
    if kind == 4 {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut parts = Vec::with_capacity(len as usize);
        for i in 0..len {
            let v = rt.heap.list_get_float(list, i).unwrap_or(0.0);
            parts.push(jet_rt::display_f64(v));
        }
        return format!("[{}]", parts.join(", "));
    }
    let xs = rt
        .heap
        .clone_int_list(list)
        .unwrap_or_default();
    let mut parts = Vec::with_capacity(xs.len());
    if kind == 1 {
        for id in xs {
            parts.push(rt.heap.clone_string(id).unwrap_or_default());
        }
    } else {
        for v in xs {
            parts.push(match kind {
                2 | 3 => jet_codegen::Comptime::MathLayout::integer_show(v, kind == 2),
                _ => v.to_string(),
            });
        }
    }
    format!("[{}]", parts.join(", "))
}

/// JetShow `[T]` as a string handle for `{list}` interpolation.
extern "C" fn jet_jit_list_show(list: i64, kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = list_show_text(rt, list, kind);
        rt.heap.alloc_string(text)
    })
}

/// Print `T?` using its JIT Option carrier.
/// `kind`: 0 = packed i64, 1 = packed string, 2 = packed f64 bits,
/// 3 = result-arena signed IntN, 4 = result-arena unsigned IntN.
extern "C" fn jet_jit_print_opt(packed: i64, kind: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let result_abi = kind >= 10;
        if (result_abi
            && !crate::runtime_host::jit_result_is_ok(rt, packed).unwrap_or(false))
            || (!result_abi && packed == 0)
        {
            rt.stdout.push_str("null\n");
            return;
        }
        let kind = if result_abi { kind - 10 } else { kind };
        let payload = if result_abi || kind >= 3 {
            crate::runtime_host::jit_result_i64(rt, packed).unwrap_or_default()
        } else {
            packed - 1
        };
        match kind {
            1 => {
                let text = rt.heap.clone_string(payload).unwrap_or_default();
                rt.stdout.push_str(&text);
            }
            2 => {
                rt.stdout
                    .push_str(&jet_rt::display_f64(f64::from_bits(payload as u64)));
            }
            3 | 4 => {
                rt.stdout.push_str(
                    &jet_codegen::Comptime::MathLayout::integer_show(payload, kind == 3),
                );
            }
            _ => {
                rt.stdout.push_str(&payload.to_string());
            }
        }
        rt.stdout.push('\n');
    });
}

/// `list.pop()` — Option ABI: `0` = None, `value + 1` = Some (i64/handle elems).
extern "C" fn jet_jit_list_pop(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let values: &mut Vec<jet_rt::JetVal> =
            unsafe { &mut *(&mut rt.heap as *mut jet_rt::JetArena as *mut Vec<jet_rt::JetVal>) };
        let Some(jet_rt::JetVal::List(xs)) = values.get_mut(list as usize) else {
            jet_foundation::ice!(None, "jit list pop: bad handle");
        };
        match xs.pop() {
            Some(jet_rt::JetVal::Int(v)) => v.wrapping_add(1),
            Some(jet_rt::JetVal::Float(v)) => (v.to_bits() as i64).wrapping_add(1),
            Some(_) | None => 0,
        }
    })
}

/// `list.insert(i, v)` — AOT `Vec::insert`; OOB traps like remove.
///
/// # ponytail: same JetArena layout poke as `list_remove`.
extern "C" fn jet_jit_list_insert(list: i64, idx: i64, v: i64) {
    Concurrency::with_runtime_mut(|rt| {
        // SAFETY: JetArena is `{ values: Vec<JetVal> }` — one field, identical address.
        let values: &mut Vec<jet_rt::JetVal> =
            unsafe { &mut *(&mut rt.heap as *mut jet_rt::JetArena as *mut Vec<jet_rt::JetVal>) };
        let Some(jet_rt::JetVal::List(xs)) = values.get_mut(list as usize) else {
            jet_foundation::ice!(None, "jit list insert: bad handle");
        };
        let len = xs.len() as i64;
        if idx < 0 || idx > len {
            rt.set_trap(&format!(
                "the list has {len} items, so position {idx} doesn't exist"
            ));
            return;
        }
        xs.insert(idx as usize, jet_rt::JetVal::Int(v));
    });
}

/// `list.remove(i)` — AOT `jet_list_remove` panic text on OOB; in-bounds mutates
/// the `JetArena` list in place (same `Vec::remove` as AOT).
///
/// # ponytail: single-field `JetArena` layout = `Vec<JetVal>`; no public remove API yet.
extern "C" fn jet_jit_list_remove(list: i64, idx: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        // SAFETY: JetArena is `{ values: Vec<JetVal> }` — one field, identical address.
        let values: &mut Vec<jet_rt::JetVal> =
            unsafe { &mut *(&mut rt.heap as *mut jet_rt::JetArena as *mut Vec<jet_rt::JetVal>) };
        let Some(jet_rt::JetVal::List(xs)) = values.get_mut(list as usize) else {
            jet_foundation::ice!(None, "jit list remove: bad handle");
        };
        let len = xs.len() as i64;
        if idx < 0 || idx >= len {
            rt.set_trap(&format!(
                "the list has {len} items, so position {idx} doesn't exist"
            ));
            return 0;
        }
        match xs.remove(idx as usize) {
            jet_rt::JetVal::Int(v) => v,
            jet_rt::JetVal::Float(v) => v.to_bits() as i64,
            _ => 0,
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

extern "C" fn jet_jit_set_from_list(list: i64, string_kind: i64) -> i64 {
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
extern "C" fn jet_jit_set_new(string_kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| set_handle(rt, HashSet::new(), string_kind != 0))
}

extern "C" fn jet_jit_set_copy(set: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (set as usize).wrapping_sub(1);
        let existing = rt.sets.get(idx).cloned().unwrap_or_default();
        let string_kind = set_is_string(rt, set);
        set_handle(rt, existing, string_kind)
    })
}

extern "C" fn jet_jit_set_equal(a: i64, b: i64) -> i8 {
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

extern "C" fn jet_jit_set_capacity(set: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.sets
            .get((set as usize).wrapping_sub(1))
            .map(|s| s.capacity() as i64)
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_set_first(set: i64) -> i64 {
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

extern "C" fn jet_jit_set_insert(set: i64, v: i64) -> i8 {
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

extern "C" fn jet_jit_set_remove(set: i64, v: i64) {
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

// #1478: native swap-in — always leaves `v` (or its string-canonical id) in
// the set; returns the displaced equal element as a packed Option (Rust's
// `HashSet::replace`), same ABI convention as `jet_jit_set_first`.
extern "C" fn jet_jit_set_replace(set: i64, v: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (set as usize).wrapping_sub(1);
        let Some(existing) = rt.sets.get(idx).cloned() else {
            return option_packed(None);
        };
        let string_kind = set_is_string(rt, set);
        let old = if string_kind {
            let needle = rt.heap.clone_string(v).unwrap_or_default();
            existing
                .iter()
                .find(|id| rt.heap.clone_string(**id).as_deref() == Some(needle.as_str()))
                .copied()
        } else {
            existing.contains(&v).then_some(v)
        };
        if let Some(old) = old {
            rt.sets[idx].remove(&old);
        }
        rt.sets[idx].insert(v);
        option_packed(old)
    })
}

// #1478: native remove-and-return-if-present (Rust's `HashSet::take`); does
// NOT insert on a miss, unlike `replace`.
extern "C" fn jet_jit_set_take(set: i64, v: i64) -> i64 {
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
        if let Some(found) = found {
            rt.sets[idx].remove(&found);
        }
        option_packed(found)
    })
}

extern "C" fn jet_jit_set_has(set: i64, v: i64) -> i8 {
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

extern "C" fn jet_jit_set_len(set: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.sets
            .get((set as usize).wrapping_sub(1))
            .map(|s| s.len() as i64)
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_set_to_list(set: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let xs: Vec<i64> = rt.sets.get((set as usize).wrapping_sub(1))
            .map(|s| s.iter().copied().collect()).unwrap_or_default();
        rt.heap.alloc_int_list(xs)
    })
}

extern "C" fn jet_jit_set_union(a: i64, b: i64) -> i64 {
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

extern "C" fn jet_jit_set_intersection(a: i64, b: i64) -> i64 {
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

extern "C" fn jet_jit_set_difference(a: i64, b: i64) -> i64 {
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

extern "C" fn jet_jit_set_symmetric_difference(a: i64, b: i64) -> i64 {
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

extern "C" fn jet_jit_set_is_subset(a: i64, b: i64) -> i8 {
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

extern "C" fn jet_jit_set_is_superset(a: i64, b: i64) -> i8 {
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

extern "C" fn jet_jit_set_is_disjoint(a: i64, b: i64) -> i8 {
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

extern "C" fn jet_jit_deque_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| deque_handle(rt, VecDeque::new()))
}

extern "C" fn jet_jit_deque_push_front(dq: i64, v: i64) {
    Concurrency::with_runtime_mut(|rt| {
        if let Some(d) = rt.deques.get_mut((dq as usize).wrapping_sub(1)) {
            d.push_front(v);
        }
    });
}

extern "C" fn jet_jit_deque_push_back(dq: i64, v: i64) {
    Concurrency::with_runtime_mut(|rt| {
        if let Some(d) = rt.deques.get_mut((dq as usize).wrapping_sub(1)) {
            d.push_back(v);
        }
    });
}

/// Packed Option: 0 = None, else value+1 (Int elems).
extern "C" fn jet_jit_deque_pop_front(dq: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match rt.deques.get_mut((dq as usize).wrapping_sub(1)).and_then(|d| d.pop_front()) {
            Some(v) => v + 1,
            None => 0,
        }
    })
}

extern "C" fn jet_jit_deque_pop_back(dq: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match rt.deques.get_mut((dq as usize).wrapping_sub(1)).and_then(|d| d.pop_back()) {
            Some(v) => v + 1,
            None => 0,
        }
    })
}

extern "C" fn jet_jit_deque_peek_front(dq: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match rt.deques.get((dq as usize).wrapping_sub(1)).and_then(|d| d.front().copied()) {
            Some(v) => v + 1,
            None => 0,
        }
    })
}

extern "C" fn jet_jit_deque_peek_back(dq: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match rt.deques.get((dq as usize).wrapping_sub(1)).and_then(|d| d.back().copied()) {
            Some(v) => v + 1,
            None => 0,
        }
    })
}

extern "C" fn jet_jit_deque_len(dq: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.deques
            .get((dq as usize).wrapping_sub(1))
            .map(|d| d.len() as i64)
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_deque_capacity(dq: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.deques
            .get((dq as usize).wrapping_sub(1))
            .map(|d| d.capacity() as i64)
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_deque_contains(dq: i64, v: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        i8::from(
            rt.deques
                .get((dq as usize).wrapping_sub(1))
                .is_some_and(|d| d.contains(&v)),
        )
    })
}

extern "C" fn jet_jit_deque_get(dq: i64, idx: i64) -> i64 {
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

extern "C" fn jet_jit_deque_delete(dq: i64, v: i64) {
    Concurrency::with_runtime_mut(|rt| {
        if let Some(d) = rt.deques.get_mut((dq as usize).wrapping_sub(1)) {
            if let Some(i) = d.iter().position(|x| *x == v) {
                d.remove(i);
            }
        }
    });
}

extern "C" fn jet_jit_deque_to_list(dq: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let xs: Vec<i64> = rt
            .deques
            .get((dq as usize).wrapping_sub(1))
            .map(|d| d.iter().copied().collect())
            .unwrap_or_default();
        rt.heap.alloc_int_list(xs)
    })
}

extern "C" fn jet_jit_deque_join(dq: i64, sep_id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(xs) = rt
            .deques
            .get((dq as usize).wrapping_sub(1))
            .map(|d| d.iter().copied().collect::<Vec<_>>())
        else {
            rt.set_trap("deque join received an invalid deque");
            return 0;
        };
        let Some(sep) = rt.heap.clone_string(sep_id) else {
            rt.set_trap("deque join received an invalid separator");
            return 0;
        };
        let parts: Vec<String> = xs.iter().map(|id| id.to_string()).collect();
        let joined = parts.join(&sep);
        rt.heap.alloc_string(joined)
    })
}

extern "C" fn jet_jit_deque_reverse(dq: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if let Some(d) = rt.deques.get_mut((dq as usize).wrapping_sub(1)) {
            d.make_contiguous().reverse();
        }
        0
    })
}

extern "C" fn jet_jit_deque_split(dq: i64, idx: i64) -> i64 {
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

extern "C" fn jet_jit_deque_from(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let xs = rt.heap.clone_int_list(list).unwrap_or_default();
        deque_handle(rt, xs.into_iter().collect())
    })
}

fn bag_handle(rt: &mut crate::JitRuntime, bag: HashMap<i64, usize>) -> i64 {
    rt.bags.push(bag);
    rt.bags.len() as i64
}

extern "C" fn jet_jit_bag_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| bag_handle(rt, HashMap::new()))
}

extern "C" fn jet_jit_bag_add(bag: i64, value: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(bag) = rt.bags.get_mut((bag as usize).wrapping_sub(1)) else {
            return 0;
        };
        *bag.entry(value).or_insert(0) += 1;
        1
    })
}

extern "C" fn jet_jit_bag_remove(bag: i64, value: i64) {
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

extern "C" fn jet_jit_bag_has(bag: i64, value: i64) -> i8 {
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

extern "C" fn jet_jit_bag_count(bag: i64, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.bags
            .get((bag as usize).wrapping_sub(1))
            .and_then(|bag| bag.get(&value))
            .copied()
            .unwrap_or(0) as i64
    })
}

extern "C" fn jet_jit_bag_len(bag: i64) -> i64 {
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

extern "C" fn jet_jit_sorted_set_new(string_kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| sorted_set_handle(rt, BTreeSet::new(), string_kind != 0))
}

extern "C" fn jet_jit_sorted_set_len(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.sorted_sets
            .get((handle as usize).wrapping_sub(1))
            .map(|s| s.len() as i64)
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_sorted_set_has(handle: i64, v: i64) -> i8 {
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

extern "C" fn jet_jit_sorted_set_from(list: i64, string_kind: i64) -> i64 {
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

extern "C" fn jet_jit_sorted_set_insert(handle: i64, value: i64) -> i8 {
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

extern "C" fn jet_jit_sorted_set_remove(handle: i64, value: i64) {
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

extern "C" fn jet_jit_sorted_set_to_list(handle: i64) -> i64 {
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

extern "C" fn jet_jit_sorted_set_first(handle: i64) -> i64 {
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

extern "C" fn jet_jit_sorted_set_last(handle: i64) -> i64 {
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

extern "C" fn jet_jit_sorted_set_union(a: i64, b: i64) -> i64 {
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

extern "C" fn jet_jit_sorted_set_intersection(a: i64, b: i64) -> i64 {
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

extern "C" fn jet_jit_sorted_set_difference(a: i64, b: i64) -> i64 {
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

extern "C" fn jet_jit_sorted_set_symmetric_difference(a: i64, b: i64) -> i64 {
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

extern "C" fn jet_jit_sorted_set_is_subset(a: i64, b: i64) -> i8 {
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

extern "C" fn jet_jit_sorted_set_is_superset(a: i64, b: i64) -> i8 {
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

extern "C" fn jet_jit_sorted_set_is_disjoint(a: i64, b: i64) -> i8 {
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

extern "C" fn jet_jit_priority_queue_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.priority_queues.push(BinaryHeap::new());
        rt.priority_queues.len() as i64
    })
}

extern "C" fn jet_jit_priority_queue_len(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.priority_queues
            .get((handle as usize).wrapping_sub(1))
            .map(|heap| heap.len() as i64)
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_priority_queue_from(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let heap = BinaryHeap::from(rt.heap.clone_int_list(list).unwrap_or_default());
        rt.priority_queues.push(heap);
        rt.priority_queues.len() as i64
    })
}

extern "C" fn jet_jit_priority_queue_push(handle: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        if let Some(heap) = rt
            .priority_queues
            .get_mut((handle as usize).wrapping_sub(1))
        {
            heap.push(value);
        }
    });
}

extern "C" fn jet_jit_priority_queue_peek(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let value = rt.priority_queues
            .get((handle as usize).wrapping_sub(1))
            .and_then(|heap| heap.peek().copied());
        option_i64(rt, value)
    })
}

extern "C" fn jet_jit_priority_queue_pop(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let value = rt.priority_queues
            .get_mut((handle as usize).wrapping_sub(1))
            .and_then(BinaryHeap::pop);
        option_i64(rt, value)
    })
}

extern "C" fn jet_jit_priority_queue_to_sorted_list(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let values = rt
            .priority_queues
            .get((handle as usize).wrapping_sub(1))
            .map(|heap| heap.clone().into_sorted_vec().into_iter().rev().collect::<Vec<_>>())
            .unwrap_or_default();
        copy_list(rt, values)
    })
}

extern "C" fn jet_jit_priority_queue_remove_value(handle: i64, value: i64) -> i64 {
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

extern "C" fn jet_jit_priority_queue_remove_slot(handle: i64, index: i64, line: u32) -> i64 {
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
                    rt.set_trap(&message);
                    None
                }
            },
            None => None,
        };
        option_i64(rt, removed)
    })
}

extern "C" fn jet_jit_lru_new(capacity: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.lrus.push(LruState {
            capacity: capacity.max(0) as usize,
            entries: VecDeque::new(),
        });
        rt.lrus.len() as i64
    })
}

extern "C" fn jet_jit_lru_put(handle: i64, key: i64, value: i64) -> i64 {
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

extern "C" fn jet_jit_lru_get(handle: i64, key: i64) -> i64 {
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

extern "C" fn jet_jit_lru_has(handle: i64, key: i64) -> i8 {
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

extern "C" fn jet_jit_lru_keys(handle: i64) -> i64 {
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

extern "C" fn jet_jit_bit_set_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.bit_sets.push(BTreeSet::new());
        rt.bit_sets.len() as i64
    })
}

extern "C" fn jet_jit_bit_set_add(handle: i64, value: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        i8::from(
            rt.bit_sets
                .get_mut((handle as usize).wrapping_sub(1))
                .is_some_and(|set| set.insert(value)),
        )
    })
}

extern "C" fn jet_jit_bit_set_remove(handle: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        if let Some(set) = rt.bit_sets.get_mut((handle as usize).wrapping_sub(1)) {
            set.remove(&value);
        }
    });
}

extern "C" fn jet_jit_bit_set_to_list(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let values = rt
            .bit_sets
            .get((handle as usize).wrapping_sub(1))
            .map(|set| set.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        copy_list(rt, values)
    })
}

extern "C" fn jet_jit_bit_set_len(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.bit_sets
            .get((handle as usize).wrapping_sub(1))
            .and_then(|set| set.iter().next_back().copied())
            .map(|last| last.saturating_add(1))
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_bit_set_count(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.bit_sets
            .get((handle as usize).wrapping_sub(1))
            .map(|set| set.len() as i64)
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_byte_buffer_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.byte_buffers
            .push(byte_buffer_semantics::JetByteBuffer::new());
        rt.byte_buffers.len() as i64
    })
}

extern "C" fn jet_jit_byte_buffer_with_capacity(n: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.byte_buffers
            .push(byte_buffer_semantics::JetByteBuffer::with_capacity(n));
        rt.byte_buffers.len() as i64
    })
}

extern "C" fn jet_jit_byte_buffer_from(list: i64) -> i64 {
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

extern "C" fn jet_jit_byte_buffer_write(handle: i64, value: i64, method: i64) {
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

extern "C" fn jet_jit_byte_buffer_to_bytes(handle: i64) -> i64 {
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
/// string handle when ret_kind=3; new ByteBuffer handle when ret_kind=4;
/// list-of-string handle when ret_kind=5; unit 0 when ret_kind=6.
extern "C" fn jet_jit_byte_buffer_method(
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
            30 => match rt.byte_buffers.get(idx).map(|b| b.parse()) {
                Some(Ok(n)) => {
                    // Result packed: positive = Ok(n+1) won't work for negatives.
                    // Store ok as string/int via existing result helpers if any —
                    // for the example, return n directly when ok and use trap on err.
                    n
                }
                Some(Err(msg)) => {
                    rt.trapped = Some(msg);
                    0
                }
                None => 0,
            },
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
    PACKED_ENUM_SHOW.with(|t| {
        let table = t.borrow();
        let Some(def) = table.get(enum_name) else {
            return format!("<enum {enum_name}>");
        };
        let disc = (packed & 0xff) as usize;
        let Some((vname, kind, nested)) = def.variants.get(disc) else {
            return format!("<bad disc {disc}>");
        };
        match kind {
            0 => vname.clone(),
            1 => format!("{vname}({})", packed >> 8),
            2 => {
                let inner = show_packed_enum(packed >> 8, nested, heap);
                format!("{vname}({inner})")
            }
            // String handle in high bits — AOT JetShow uses Debug quotes.
            3 => {
                let text = heap.clone_string(packed >> 8).unwrap_or_default();
                format!("{vname}({text:?})")
            }
            _ => format!("<{vname}?>"),
        }
    })
}

/// Print a packed i64 enum. `name_ptr`/`name_len` are a UTF-8 view of the Jet
/// enum name (stable for the process — not a heap string handle).
extern "C" fn jet_jit_print_enum(packed: i64, name_ptr: i64, name_len: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let name = if name_ptr == 0 || name_len <= 0 {
            String::new()
        } else {
            let slice = unsafe {
                std::slice::from_raw_parts(name_ptr as *const u8, name_len as usize)
            };
            String::from_utf8_lossy(slice).into_owned()
        };
        let text = show_packed_enum(packed, &name, &rt.heap);
        rt.stdout.push_str(&text);
        rt.stdout.push('\n');
    });
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
        let mut sig_four_ret = sig_three_ret.clone();
        sig_four_ret.params.push(AbiParam::new(types::I64));
        let sig_map_get = sig_get.clone();
        let sig_map_get_opt = sig_get_opt.clone();
        let sig_map_at = sig_get_opt.clone();
        let mut sig_print_list = sig_get_opt.clone();
        sig_print_list.returns.clear();
        let mut sig_print_enum = Signature::new(cc);
        sig_print_enum.params.push(AbiParam::new(types::I64));
        sig_print_enum.params.push(AbiParam::new(types::I64));
        sig_print_enum.params.push(AbiParam::new(types::I64));
        let mut sig_sort_by_keys = sig_get_opt.clone();
        sig_sort_by_keys.returns.clear();
        let mut sig_bool = Signature::new(cc);
        sig_bool.params.push(AbiParam::new(types::I64));
        sig_bool.returns.push(AbiParam::new(types::I8));
        let mut sig_f64 = Signature::new(cc);
        sig_f64.params.push(AbiParam::new(types::I64));
        sig_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_priority_queue_slot = Signature::new(cc);
        sig_priority_queue_slot.params.push(AbiParam::new(types::I64));
        sig_priority_queue_slot.params.push(AbiParam::new(types::I64));
        sig_priority_queue_slot.params.push(AbiParam::new(types::I32));
        sig_priority_queue_slot.returns.push(AbiParam::new(types::I64));


    }
    io_args: "jet_jit_io_args" => jet_jit_io_args: sig_new;
    list_new: "jet_jit_list_new" => jet_jit_list_new: sig_new;
    list_uninit: "jet_jit_list_uninit" => jet_jit_list_uninit: sig_uninit;
    list_push: "jet_jit_list_push" => jet_jit_list_push: sig_push;
    list_push_f64: "jet_jit_list_push_f64" => jet_jit_list_push_f64: sig_push_f64;
    list_push_range: "jet_jit_list_push_range" => jet_jit_list_push_range: sig_push_range;
    list_get: "jet_jit_list_get" => jet_jit_list_get: sig_get;
    list_get_f64: "jet_jit_list_get_f64" => jet_jit_list_get_f64: sig_get_f64;
    fixed_list_get: "jet_jit_fixed_list_get" => jet_jit_fixed_list_get: sig_get;
    fixed_list_get_f64: "jet_jit_fixed_list_get_f64" => jet_jit_fixed_list_get_f64: sig_get_f64;
    list_get_range_start: "jet_jit_list_get_range_start" => jet_jit_list_get_range_start: sig_get_range_scalar;
    list_get_range_end: "jet_jit_list_get_range_end" => jet_jit_list_get_range_end: sig_get_range_scalar;
    list_get_range_exclusive: "jet_jit_list_get_range_exclusive" => jet_jit_list_get_range_exclusive: sig_get_range_exclusive;
    list_get_opt: "jet_jit_list_get_opt" => jet_jit_list_get_opt: sig_get_opt;
    list_set: "jet_jit_list_set" => jet_jit_list_set: sig_set;
    list_set_f64: "jet_jit_list_set_f64" => jet_jit_list_set_f64: sig_set_f64;
    list_len: "jet_jit_list_len" => jet_jit_list_len: sig_len;
    list_contains_str: "jet_jit_list_contains_str" => jet_jit_list_contains_str: sig_list_eq;
    list_eq: "jet_jit_list_eq" => jet_jit_list_eq: sig_list_eq;
    list_indexes: "jet_jit_list_indexes" => jet_jit_list_indexes: sig_len;
    list_sort: "jet_jit_list_sort" => jet_jit_list_sort: sig_sort;
    list_sort_str: "jet_jit_list_sort_str" => jet_jit_list_sort_str: sig_sort;
    list_clone: "jet_jit_list_clone" => jet_jit_list_clone: sig_len;
    list_copy: "jet_jit_list_copy" => jet_jit_list_copy: sig_len;
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
    list_join_str: "jet_jit_list_join_str" => jet_jit_list_join_str: sig_join;
    loop_stride_check: "jet_jit_loop_stride_check" => jet_jit_loop_stride_check: sig_len;
    map_new: "jet_jit_map_new" => jet_jit_map_new: sig_new;
    map_clone: "jet_jit_map_clone" => jet_jit_map_clone: sig_len;
    map_merge: "jet_jit_map_merge" => jet_jit_map_merge: sig_get_opt;
    map_insert: "jet_jit_map_insert" => jet_jit_map_insert: sig_map_insert;
    map_increment: "jet_jit_map_increment" => jet_jit_map_increment: sig_push;
    map_get: "jet_jit_map_get" => jet_jit_map_get: sig_map_get;
    map_validate: "jet_jit_map_validate" => jet_jit_map_validate: sig_len;
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
    iter_take: "jet_jit_iter_take" => jet_jit_iter_take: sig_get_opt;
    iter_skip: "jet_jit_iter_skip" => jet_jit_iter_skip: sig_get_opt;
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
    list_zip: "jet_jit_list_zip" => jet_jit_list_zip: sig_get_opt;
    list_unzip: "jet_jit_list_unzip" => jet_jit_list_unzip: sig_len;
    list_sort_by_i64_keys: "jet_jit_list_sort_by_i64_keys" => jet_jit_list_sort_by_i64_keys: sig_sort_by_keys;
    list_sort_by_str_keys: "jet_jit_list_sort_by_str_keys" => jet_jit_list_sort_by_str_keys: sig_sort_by_keys;
    print_list: "jet_jit_print_list" => jet_jit_print_list: sig_print_list;
    print_opt: "jet_jit_print_opt" => jet_jit_print_opt: sig_print_list;
    print_enum: "jet_jit_print_enum" => jet_jit_print_enum: sig_print_enum;
    list_show: "jet_jit_list_show" => jet_jit_list_show: sig_get_opt;
    list_remove: "jet_jit_list_remove" => jet_jit_list_remove: sig_get_opt;
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
    set_replace: "jet_jit_set_replace" => jet_jit_set_replace: sig_get_opt;
    set_take: "jet_jit_set_take" => jet_jit_set_take: sig_get_opt;
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
    bit_set_remove: "jet_jit_bit_set_remove" => jet_jit_bit_set_remove: sig_push;
    bit_set_to_list: "jet_jit_bit_set_to_list" => jet_jit_bit_set_to_list: sig_len;
    bit_set_len: "jet_jit_bit_set_len" => jet_jit_bit_set_len: sig_len;
    bit_set_count: "jet_jit_bit_set_count" => jet_jit_bit_set_count: sig_len;
    byte_buffer_new: "jet_jit_byte_buffer_new" => jet_jit_byte_buffer_new: sig_new;
    byte_buffer_with_capacity: "jet_jit_byte_buffer_with_capacity" => jet_jit_byte_buffer_with_capacity: sig_len;
    byte_buffer_from: "jet_jit_byte_buffer_from" => jet_jit_byte_buffer_from: sig_len;
    byte_buffer_write: "jet_jit_byte_buffer_write" => jet_jit_byte_buffer_write: sig_map_insert;
    byte_buffer_to_bytes: "jet_jit_byte_buffer_to_bytes" => jet_jit_byte_buffer_to_bytes: sig_len;
    byte_buffer_method: "jet_jit_byte_buffer_method" => jet_jit_byte_buffer_method: sig_four_ret;
}
