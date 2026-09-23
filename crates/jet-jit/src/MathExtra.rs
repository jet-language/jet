//! Extra `core.math` hosts (#1464 / I9). Algorithms from Prelude `MathLibPure`
//! via build.rs extract — marshalling only.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_module::Module;

pub(crate) mod math_rt {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!(concat!(env!("OUT_DIR"), "/math_rt.rs"));
}

pub(crate) mod collection_rt {
    pub(crate) type JetOutcome<T, E> = Result<T, E>;

    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub(crate) struct JetAbsent;

    pub(crate) trait JetShow {
        fn jet_show(&self) -> String;
    }
    pub(crate) trait JetDisplay {
        fn jet_display(&self) -> String;
    }
    pub(crate) trait JetDebug {
        fn jet_debug(&self) -> String;
    }

    impl<T: std::fmt::Display> JetShow for Vec<T> {
        fn jet_show(&self) -> String {
            format!(
                "[{}]",
                self.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }

    include!(concat!(env!("OUT_DIR"), "/collection_rt.rs"));
}

fn jet_jit_math_pi() -> f64 {
    math_rt::jet_std_math_pi()
}
fn jet_jit_math_abs_i64(value: i64) -> i64 {
    math_rt::jet_std_math_abs_i64(value)
}
fn jet_jit_math_abs_f64(value: f64) -> f64 {
    math_rt::jet_std_math_abs_f64(value)
}
fn jet_jit_math_abs_f32(value: f64) -> f64 {
    math_rt::jet_std_math_abs_f32(value as f32) as f64
}
fn jet_jit_math_to_bits(value: f64) -> i64 {
    math_rt::jet_std_math_to_bits(value)
}
fn jet_jit_math_from_bits(value: i64) -> f64 {
    math_rt::jet_std_math_from_bits(value)
}
fn jet_jit_math_round(value: f64) -> i64 {
    math_rt::jet_std_math_round(value)
}
fn jet_jit_math_is_nan(x: f64) -> i8 {
    i8::from(math_rt::jet_std_math_is_nan(x))
}
fn jet_jit_math_is_infinite(x: f64) -> i8 {
    i8::from(math_rt::jet_std_math_is_infinite(x))
}
fn jet_jit_math_is_finite(x: f64) -> i8 {
    i8::from(math_rt::jet_std_math_is_finite(x))
}

fn opt_i64(v: Option<i64>) -> i64 {
    match v {
        Some(n) => n.wrapping_add(1),
        None => 0,
    }
}
fn list_f64s(list: i64) -> Vec<f64> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        (0..len)
            .map(|index| rt.heap.list_get_float(list, index).unwrap_or(0.0))
            .collect()
    })
}
fn list_i64s(list: i64) -> Vec<i64> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        (0..len)
            .map(|index| rt.heap.list_get_int(list, index).unwrap_or(0))
            .collect()
    })
}
fn list_bools(list: i64) -> Vec<bool> {
    list_i64s(list).into_iter().map(|value| value != 0).collect()
}
fn alloc_i64_list(values: &[i64]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let handle = rt.heap.alloc_empty_list();
        for &value in values {
            let _ = rt.heap.list_push_int(handle, value);
        }
        handle
    })
}

fn coll_string(rt: &crate::runtime_host::JitRuntime, handle: i64) -> String {
    rt.heap
        .clone_string(handle)
        .or_else(|| crate::runtime_host::view_string(rt, handle))
        .unwrap_or_default()
}

fn coll_read_string_list(
    rt: &crate::runtime_host::JitRuntime,
    list: i64,
) -> Option<Vec<String>> {
    let len = rt.heap.list_len(list)?;
    (0..len)
        .map(|index| rt.heap.list_get_string(list, index))
        .collect()
}

fn coll_read_i64_list(
    rt: &crate::runtime_host::JitRuntime,
    list: i64,
) -> Option<Vec<i64>> {
    let len = rt.heap.list_len(list)?;
    (0..len)
        .map(|index| rt.heap.list_get_int(list, index))
        .collect()
}

fn coll_alloc_string_list(rt: &mut crate::runtime_host::JitRuntime, values: &[String]) -> i64 {
    let list = rt.heap.alloc_empty_list();
    for value in values {
        let handle = rt.heap.alloc_string(value.clone());
        let _ = rt.heap.list_push_int(list, handle);
    }
    list
}

fn coll_read_counter(
    rt: &mut crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<collection_rt::JetCounter> {
    let keys = rt.heap.record_get_int(handle, 0)?;
    let counts = rt.heap.record_get_int(handle, 1)?;
    Some(collection_rt::JetCounter {
        keys: coll_read_string_list(rt, keys)?,
        counts: coll_read_i64_list(rt, counts)?,
    })
}

fn coll_alloc_counter(
    rt: &mut crate::runtime_host::JitRuntime,
    counter: &collection_rt::JetCounter,
) -> i64 {
    let keys = coll_alloc_string_list(rt, &counter.keys);
    let counts = rt.heap.alloc_int_list(counter.counts.clone());
    let record = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_int(record, 0, keys);
    let _ = rt.heap.record_set_int(record, 1, counts);
    record
}

fn coll_read_deque(
    rt: &mut crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<collection_rt::JetDeque> {
    let items = rt.heap.record_get_int(handle, 0)?;
    let head = rt.heap.record_get_int(handle, 1)?;
    Some(collection_rt::JetDeque {
        items: coll_read_string_list(rt, items)?,
        head,
    })
}

fn coll_alloc_deque(
    rt: &mut crate::runtime_host::JitRuntime,
    deque: &collection_rt::JetDeque,
) -> i64 {
    let items = coll_alloc_string_list(rt, &deque.items);
    let record = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_int(record, 0, items);
    let _ = rt.heap.record_set_int(record, 1, deque.head);
    record
}

fn coll_read_ordered_map(
    rt: &mut crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<collection_rt::JetOrderedMap> {
    let keys = rt.heap.record_get_int(handle, 0)?;
    let values = rt.heap.record_get_int(handle, 1)?;
    Some(collection_rt::JetOrderedMap {
        keys: coll_read_string_list(rt, keys)?,
        values: coll_read_string_list(rt, values)?,
    })
}

fn coll_alloc_ordered_map(
    rt: &mut crate::runtime_host::JitRuntime,
    map: &collection_rt::JetOrderedMap,
) -> i64 {
    let keys = coll_alloc_string_list(rt, &map.keys);
    let values = coll_alloc_string_list(rt, &map.values);
    let record = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_int(record, 0, keys);
    let _ = rt.heap.record_set_int(record, 1, values);
    record
}

fn coll_read_layer(
    rt: &mut crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<collection_rt::JetLayer> {
    let keys = rt.heap.record_get_int(handle, 0)?;
    let values = rt.heap.record_get_int(handle, 1)?;
    Some(collection_rt::JetLayer {
        keys: coll_read_string_list(rt, keys)?,
        values: coll_read_string_list(rt, values)?,
    })
}

fn coll_read_layer_list(
    rt: &mut crate::runtime_host::JitRuntime,
    list: i64,
) -> Option<Vec<collection_rt::JetLayer>> {
    let len = rt.heap.list_len(list)?;
    (0..len)
        .map(|index| {
            let handle = rt
                .heap
                .list_get_int(list, index)
                .or_else(|| rt.heap.list_get_raw(list, index))?;
            coll_read_layer(rt, handle)
        })
        .collect()
}

fn coll_alloc_layer(
    rt: &mut crate::runtime_host::JitRuntime,
    layer: &collection_rt::JetLayer,
) -> i64 {
    let keys = coll_alloc_string_list(rt, &layer.keys);
    let values = coll_alloc_string_list(rt, &layer.values);
    let record = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_int(record, 0, keys);
    let _ = rt.heap.record_set_int(record, 1, values);
    record
}

fn coll_read_chain(
    rt: &mut crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<collection_rt::JetChain> {
    let layers = rt.heap.record_get_int(handle, 0)?;
    Some(collection_rt::JetChain {
        layers: coll_read_layer_list(rt, layers)?,
    })
}

fn coll_alloc_chain(
    rt: &mut crate::runtime_host::JitRuntime,
    chain: &collection_rt::JetChain,
) -> i64 {
    let layers = rt.heap.alloc_empty_list();
    for layer in &chain.layers {
        let handle = coll_alloc_layer(rt, layer);
        let _ = rt.heap.list_push_int(layers, handle);
    }
    let record = rt.heap.alloc_record(1);
    let _ = rt.heap.record_set_int(record, 0, layers);
    record
}


fn coll_option_string(rt: &mut crate::runtime_host::JitRuntime, value: Option<String>) -> i64 {
    value
        .map(|value| rt.heap.alloc_string(value).wrapping_add(1))
        .unwrap_or(0)
}

fn coll_deque_pair(
    rt: &mut crate::runtime_host::JitRuntime,
    deque: &collection_rt::JetDeque,
    value: collection_rt::JetOutcome<String, collection_rt::JetAbsent>,
) -> i64 {
    let deque = coll_alloc_deque(rt, deque);
    let value = match value {
        Ok(value) => {
            let handle = rt.heap.alloc_string(value);
            crate::runtime_host::alloc_jit_result(rt, true, handle as u64)
        }
        Err(_) => crate::runtime_host::alloc_jit_result(rt, false, 0),
    };
    let record = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_int(record, 0, deque);
    let _ = rt.heap.record_set_int(record, 1, value);
    record
}

fn jet_jit_comb_chain(left: i64, right: i64) -> i64 {
    alloc_i64_list(&math_rt::jet_comb_chain(list_i64s(left), list_i64s(right)))
}
fn jet_jit_comb_compress(items: i64, mask: i64) -> i64 {
    alloc_i64_list(&math_rt::jet_comb_compress(list_i64s(items), list_bools(mask)))
}
fn jet_jit_comb_drop(items: i64, count: i64) -> i64 {
    alloc_i64_list(&math_rt::jet_comb_drop(list_i64s(items), count))
}
fn jet_jit_comb_takewhile(items: i64, pred_nonneg: i8) -> i64 {
    alloc_i64_list(&math_rt::jet_comb_takewhile(list_i64s(items), pred_nonneg != 0))
}
fn jet_jit_comb_dropwhile(items: i64, pred_nonneg: i8) -> i64 {
    alloc_i64_list(&math_rt::jet_comb_dropwhile(list_i64s(items), pred_nonneg != 0))
}
fn jet_jit_comb_filterfalse(items: i64, pred_nonneg: i8) -> i64 {
    alloc_i64_list(&math_rt::jet_comb_filterfalse(list_i64s(items), pred_nonneg != 0))
}
fn jet_jit_comb_islice(items: i64, start: i64, stop: i64, step: i64) -> i64 {
    alloc_i64_list(&math_rt::jet_comb_islice(list_i64s(items), start, stop, step))
}
fn jet_jit_comb_unique(items: i64) -> i64 {
    alloc_i64_list(&math_rt::jet_comb_unique(list_i64s(items)))
}
fn jet_jit_comb_repeat(value: i64, times: i64) -> i64 {
    alloc_i64_list(&math_rt::jet_comb_repeat(value, times))
}
fn jet_jit_comb_count_from(start: i64, step: i64, count: i64) -> i64 {
    alloc_i64_list(&math_rt::jet_comb_count_from(start, step, count))
}
fn jet_jit_comb_cycle(items: i64, times: i64) -> i64 {
    alloc_i64_list(&math_rt::jet_comb_cycle(list_i64s(items), times))
}
fn jet_jit_comb_accumulate(items: i64) -> i64 {
    alloc_i64_list(&math_rt::jet_comb_accumulate(list_i64s(items)))
}
fn jet_jit_comb_reverse(items: i64) -> i64 {
    alloc_i64_list(&math_rt::jet_comb_reverse(list_i64s(items)))
}
fn list_i64_matrix(list: i64) -> Vec<Vec<i64>> {
    Concurrency::with_runtime_mut(|rt| {
        let outer_len = rt.heap.list_len(list).unwrap_or(0);
        (0..outer_len)
            .map(|outer_index| {
                let inner = rt.heap.list_get_int(list, outer_index).unwrap_or(0);
                let inner_len = rt.heap.list_len(inner).unwrap_or(0);
                (0..inner_len)
                    .map(|inner_index| rt.heap.list_get_int(inner, inner_index).unwrap_or(0))
                    .collect()
            })
            .collect()
    })
}
fn alloc_i64_matrix(values: &[Vec<i64>]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let outer = rt.heap.alloc_empty_list();
        for row in values {
            let inner = rt.heap.alloc_empty_list();
            for &value in row {
                let _ = rt.heap.list_push_int(inner, value);
            }
            let _ = rt.heap.list_push_int(outer, inner);
        }
        outer
    })
}
fn jet_jit_comb_permutations(items: i64, k: i64) -> i64 {
    alloc_i64_matrix(&math_rt::jet_comb_permutations(list_i64s(items), k))
}
fn jet_jit_comb_combinations(items: i64, k: i64) -> i64 {
    alloc_i64_matrix(&math_rt::jet_comb_combinations(list_i64s(items), k))
}
fn jet_jit_comb_combinations_with_replacement(items: i64, k: i64) -> i64 {
    alloc_i64_matrix(&math_rt::jet_comb_combinations_with_replacement(
        list_i64s(items),
        k,
    ))
}
fn jet_jit_comb_product(items: i64, repeat: i64) -> i64 {
    alloc_i64_matrix(&math_rt::jet_comb_product(list_i64s(items), repeat))
}
fn jet_jit_comb_cartesian(left: i64, right: i64) -> i64 {
    alloc_i64_matrix(&math_rt::jet_comb_cartesian(list_i64s(left), list_i64s(right)))
}
fn jet_jit_comb_pairwise(items: i64) -> i64 {
    alloc_i64_matrix(&math_rt::jet_comb_pairwise(list_i64s(items)))
}
fn jet_jit_comb_batched(items: i64, size: i64) -> i64 {
    alloc_i64_matrix(&math_rt::jet_comb_batched(list_i64s(items), size))
}
fn jet_jit_comb_groupby(items: i64) -> i64 {
    alloc_i64_matrix(&math_rt::jet_comb_groupby(list_i64s(items)))
}
fn jet_jit_comb_tee(items: i64, copies: i64) -> i64 {
    alloc_i64_matrix(&math_rt::jet_comb_tee(list_i64s(items), copies))
}
fn jet_jit_comb_zip_longest(left: i64, right: i64, fill: i64) -> i64 {
    alloc_i64_matrix(&math_rt::jet_comb_zip_longest(
        list_i64s(left),
        list_i64s(right),
        fill,
    ))
}
fn jet_jit_comb_powerset(items: i64) -> i64 {
    alloc_i64_matrix(&math_rt::jet_comb_powerset(list_i64s(items)))
}
fn jet_jit_comb_windows(items: i64, size: i64) -> i64 {
    alloc_i64_matrix(&math_rt::jet_comb_windows(list_i64s(items), size))
}
fn jet_jit_comb_flatten(groups: i64) -> i64 {
    alloc_i64_list(&math_rt::jet_comb_flatten(list_i64_matrix(groups)))
}
fn jet_jit_comb_starmap(rows: i64) -> i64 {
    alloc_i64_list(&math_rt::jet_comb_starmap(list_i64_matrix(rows)))
}
fn jet_jit_coll_heapify(items: i64) -> i64 {
    let items = list_i64s(items);
    alloc_i64_list(&math_rt::jet_coll_heapify(&items))
}
fn jet_jit_coll_heappush(items: i64, value: i64) -> i64 {
    let items = list_i64s(items);
    alloc_i64_list(&math_rt::jet_coll_heappush(&items, value))
}
fn jet_jit_coll_bisect_left(items: i64, value: i64) -> i64 {
    let items = list_i64s(items);
    math_rt::jet_coll_bisect_left(&items, value)
}
fn jet_jit_coll_bisect_right(items: i64, value: i64) -> i64 {
    let items = list_i64s(items);
    math_rt::jet_coll_bisect_right(&items, value)
}
fn jet_jit_coll_insort_left(items: i64, value: i64) -> i64 {
    let items = list_i64s(items);
    alloc_i64_list(&math_rt::jet_coll_insort_left(&items, value))
}
fn jet_jit_coll_insort_right(items: i64, value: i64) -> i64 {
    let items = list_i64s(items);
    alloc_i64_list(&math_rt::jet_coll_insort_right(&items, value))
}
fn jet_jit_coll_merge_sorted(left: i64, right: i64) -> i64 {
    let left = list_i64s(left);
    let right = list_i64s(right);
    alloc_i64_list(&math_rt::jet_coll_merge_sorted(&left, &right))
}
fn jet_jit_coll_nsmallest(n: i64, items: i64) -> i64 {
    let items = list_i64s(items);
    alloc_i64_list(&math_rt::jet_coll_nsmallest(n, &items))
}
fn jet_jit_coll_nlargest(n: i64, items: i64) -> i64 {
    let items = list_i64s(items);
    alloc_i64_list(&math_rt::jet_coll_nlargest(n, &items))
}
fn jet_jit_coll_heappop(items: i64) -> i64 {
    let items = list_i64s(items);
    let (heap, value) = math_rt::jet_coll_heappop_pure(&items);
    pair_list_opt(&heap, value)
}
fn jet_jit_coll_heappushpop(items: i64, value: i64) -> i64 {
    let items = list_i64s(items);
    let (heap, popped) = math_rt::jet_coll_heappushpop_pure(&items, value);
    pair_list_i64(&heap, popped)
}
fn jet_jit_coll_heapreplace(items: i64, value: i64) -> i64 {
    let items = list_i64s(items);
    let (heap, popped) = math_rt::jet_coll_heapreplace_pure(&items, value);
    pair_list_opt(&heap, popped)
}

fn pair_list_opt(values: &[i64], value: Option<i64>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_int_list(values.to_vec());
        let option = crate::runtime_host::alloc_jit_result(
            rt,
            value.is_some(),
            value.unwrap_or_default() as u64,
        );
        let h = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(h, 0, list);
        let _ = rt.heap.record_set_int(h, 1, option);
        h
    })
}

fn pair_list_i64(values: &[i64], value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_int_list(values.to_vec());
        let h = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(h, 0, list);
        let _ = rt.heap.record_set_int(h, 1, value);
        h
    })
}


fn jet_jit_coll_counter() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        coll_alloc_counter(rt, &collection_rt::jet_coll_counter())
    })
}

fn jet_jit_coll_counter_from(keys: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let keys = coll_read_string_list(rt, keys).unwrap_or_default();
        coll_alloc_counter(rt, &collection_rt::jet_coll_counter_from(&keys))
    })
}

fn jet_jit_coll_counter_add(counter: i64, key: i64, amount: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let counter =
            coll_read_counter(rt, counter).unwrap_or_else(collection_rt::jet_coll_counter);
        let key = coll_string(rt, key);
        let next = collection_rt::jet_coll_counter_add(&counter, &key, amount);
        coll_alloc_counter(rt, &next)
    })
}

fn jet_jit_coll_counter_inc(counter: i64, key: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let counter =
            coll_read_counter(rt, counter).unwrap_or_else(collection_rt::jet_coll_counter);
        let key = coll_string(rt, key);
        let next = collection_rt::jet_coll_counter_inc(&counter, &key);
        coll_alloc_counter(rt, &next)
    })
}

fn jet_jit_coll_counter_dec(counter: i64, key: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let counter =
            coll_read_counter(rt, counter).unwrap_or_else(collection_rt::jet_coll_counter);
        let key = coll_string(rt, key);
        let next = collection_rt::jet_coll_counter_dec(&counter, &key);
        coll_alloc_counter(rt, &next)
    })
}

fn jet_jit_coll_counter_get(counter: i64, key: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let counter =
            coll_read_counter(rt, counter).unwrap_or_else(collection_rt::jet_coll_counter);
        let key = coll_string(rt, key);
        collection_rt::jet_coll_counter_get(&counter, &key)
    })
}

fn jet_jit_coll_counter_set_count(counter: i64, key: i64, count: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let counter =
            coll_read_counter(rt, counter).unwrap_or_else(collection_rt::jet_coll_counter);
        let key = coll_string(rt, key);
        let next = collection_rt::jet_coll_counter_set_count(&counter, &key, count);
        coll_alloc_counter(rt, &next)
    })
}

fn jet_jit_coll_counter_total(counter: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let counter =
            coll_read_counter(rt, counter).unwrap_or_else(collection_rt::jet_coll_counter);
        collection_rt::jet_coll_counter_total(&counter)
    })
}

fn jet_jit_coll_counter_names(counter: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let counter =
            coll_read_counter(rt, counter).unwrap_or_else(collection_rt::jet_coll_counter);
        coll_alloc_string_list(rt, &collection_rt::jet_coll_counter_names(&counter))
    })
}

fn jet_jit_coll_counter_elements(counter: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let counter =
            coll_read_counter(rt, counter).unwrap_or_else(collection_rt::jet_coll_counter);
        coll_alloc_string_list(rt, &collection_rt::jet_coll_counter_elements(&counter))
    })
}

fn jet_jit_coll_counter_most_common(counter: i64, k: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let counter =
            coll_read_counter(rt, counter).unwrap_or_else(collection_rt::jet_coll_counter);
        let next = collection_rt::jet_coll_counter_most_common(&counter, k);
        coll_alloc_counter(rt, &next)
    })
}

fn jet_jit_coll_counter_subtract(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let left = coll_read_counter(rt, left).unwrap_or_else(collection_rt::jet_coll_counter);
        let right =
            coll_read_counter(rt, right).unwrap_or_else(collection_rt::jet_coll_counter);
        let next = collection_rt::jet_coll_counter_subtract(&left, &right);
        coll_alloc_counter(rt, &next)
    })
}

fn jet_jit_coll_counter_merge_add(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let left = coll_read_counter(rt, left).unwrap_or_else(collection_rt::jet_coll_counter);
        let right =
            coll_read_counter(rt, right).unwrap_or_else(collection_rt::jet_coll_counter);
        let next = collection_rt::jet_coll_counter_merge_add(&left, &right);
        coll_alloc_counter(rt, &next)
    })
}

fn jet_jit_coll_counter_clear(counter: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let counter =
            coll_read_counter(rt, counter).unwrap_or_else(collection_rt::jet_coll_counter);
        coll_alloc_counter(rt, &collection_rt::jet_coll_counter_clear(&counter))
    })
}

fn jet_jit_coll_deque() -> i64 {
    Concurrency::with_runtime_mut(|rt| coll_alloc_deque(rt, &collection_rt::jet_coll_deque()))
}

fn jet_jit_coll_deque_from(items: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let items = coll_read_string_list(rt, items).unwrap_or_default();
        coll_alloc_deque(rt, &collection_rt::jet_coll_deque_from(&items))
    })
}

fn jet_jit_coll_deque_len(deque: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let deque = coll_read_deque(rt, deque).unwrap_or_else(collection_rt::jet_coll_deque);
        collection_rt::jet_coll_deque_len(&deque)
    })
}

fn jet_jit_coll_deque_is_empty(deque: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let deque = coll_read_deque(rt, deque).unwrap_or_else(collection_rt::jet_coll_deque);
        i8::from(collection_rt::jet_coll_deque_is_empty(&deque))
    })
}

fn jet_jit_coll_deque_append(deque: i64, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let deque = coll_read_deque(rt, deque).unwrap_or_else(collection_rt::jet_coll_deque);
        let value = coll_string(rt, value);
        let next = collection_rt::jet_coll_deque_append(&deque, &value);
        coll_alloc_deque(rt, &next)
    })
}

fn jet_jit_coll_deque_appendleft(deque: i64, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let deque = coll_read_deque(rt, deque).unwrap_or_else(collection_rt::jet_coll_deque);
        let value = coll_string(rt, value);
        let next = collection_rt::jet_coll_deque_appendleft(&deque, &value);
        coll_alloc_deque(rt, &next)
    })
}

fn jet_jit_coll_deque_pop(deque: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let deque = coll_read_deque(rt, deque).unwrap_or_else(collection_rt::jet_coll_deque);
        let (next, value) = collection_rt::jet_coll_deque_pop(&deque);
        coll_deque_pair(rt, &next, value)
    })
}

fn jet_jit_coll_deque_popleft(deque: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let deque = coll_read_deque(rt, deque).unwrap_or_else(collection_rt::jet_coll_deque);
        let (next, value) = collection_rt::jet_coll_deque_popleft(&deque);
        coll_deque_pair(rt, &next, value)
    })
}

fn jet_jit_coll_deque_peek(deque: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let deque = coll_read_deque(rt, deque).unwrap_or_else(collection_rt::jet_coll_deque);
        coll_option_string(rt, collection_rt::jet_coll_deque_peek(&deque))
    })
}

fn jet_jit_coll_deque_peekleft(deque: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let deque = coll_read_deque(rt, deque).unwrap_or_else(collection_rt::jet_coll_deque);
        coll_option_string(rt, collection_rt::jet_coll_deque_peekleft(&deque))
    })
}
fn jet_jit_coll_deque_extend(deque: i64, values: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let deque = coll_read_deque(rt, deque).unwrap_or_else(collection_rt::jet_coll_deque);
        let values = coll_read_string_list(rt, values).unwrap_or_default();
        let next = collection_rt::jet_coll_deque_extend(&deque, &values);
        coll_alloc_deque(rt, &next)
    })
}

fn jet_jit_coll_deque_extendleft(deque: i64, values: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let deque = coll_read_deque(rt, deque).unwrap_or_else(collection_rt::jet_coll_deque);
        let values = coll_read_string_list(rt, values).unwrap_or_default();
        let next = collection_rt::jet_coll_deque_extendleft(&deque, &values);
        coll_alloc_deque(rt, &next)
    })
}

fn jet_jit_coll_deque_rotate(deque: i64, amount: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let deque = coll_read_deque(rt, deque).unwrap_or_else(collection_rt::jet_coll_deque);
        let next = collection_rt::jet_coll_deque_rotate(&deque, amount);
        coll_alloc_deque(rt, &next)
    })
}

fn jet_jit_coll_deque_items(deque: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let deque = coll_read_deque(rt, deque).unwrap_or_else(collection_rt::jet_coll_deque);
        coll_alloc_string_list(rt, &collection_rt::jet_coll_deque_items(&deque))
    })
}

fn jet_jit_coll_ordered_map() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        coll_alloc_ordered_map(rt, &collection_rt::jet_coll_ordered_map())
    })
}

fn jet_jit_coll_map_get(map: i64, key: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let map =
            coll_read_ordered_map(rt, map).unwrap_or_else(collection_rt::jet_coll_ordered_map);
        let key = coll_string(rt, key);
        coll_option_string(rt, collection_rt::jet_coll_map_get(&map, &key))
    })
}

fn jet_jit_coll_map_set(map: i64, key: i64, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let map =
            coll_read_ordered_map(rt, map).unwrap_or_else(collection_rt::jet_coll_ordered_map);
        let key = coll_string(rt, key);
        let value = coll_string(rt, value);
        let next = collection_rt::jet_coll_map_set(&map, &key, &value);
        coll_alloc_ordered_map(rt, &next)
    })
}

fn jet_jit_coll_map_remove(map: i64, key: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let map =
            coll_read_ordered_map(rt, map).unwrap_or_else(collection_rt::jet_coll_ordered_map);
        let key = coll_string(rt, key);
        let next = collection_rt::jet_coll_map_remove(&map, &key);
        coll_alloc_ordered_map(rt, &next)
    })
}

fn jet_jit_coll_map_keys(map: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let map =
            coll_read_ordered_map(rt, map).unwrap_or_else(collection_rt::jet_coll_ordered_map);
        coll_alloc_string_list(rt, &collection_rt::jet_coll_map_keys(&map))
    })
}

fn jet_jit_coll_map_values(map: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let map =
            coll_read_ordered_map(rt, map).unwrap_or_else(collection_rt::jet_coll_ordered_map);
        coll_alloc_string_list(rt, &collection_rt::jet_coll_map_values(&map))
    })
}

fn jet_jit_coll_map_contains(map: i64, key: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let map =
            coll_read_ordered_map(rt, map).unwrap_or_else(collection_rt::jet_coll_ordered_map);
        let key = coll_string(rt, key);
        i8::from(collection_rt::jet_coll_map_contains(&map, &key))
    })
}

fn jet_jit_coll_map_len(map: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let map =
            coll_read_ordered_map(rt, map).unwrap_or_else(collection_rt::jet_coll_ordered_map);
        collection_rt::jet_coll_map_len(&map)
    })
}

fn jet_jit_coll_chain() -> i64 {
    Concurrency::with_runtime_mut(|rt| coll_alloc_chain(rt, &collection_rt::jet_coll_chain()))
}

fn jet_jit_coll_chain_push(chain: i64, keys: i64, values: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let chain = coll_read_chain(rt, chain).unwrap_or_else(collection_rt::jet_coll_chain);
        let keys = coll_read_string_list(rt, keys).unwrap_or_default();
        let values = coll_read_string_list(rt, values).unwrap_or_default();
        let next = collection_rt::jet_coll_chain_push(&chain, &keys, &values);
        coll_alloc_chain(rt, &next)
    })
}

fn jet_jit_coll_chain_get(chain: i64, key: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let chain = coll_read_chain(rt, chain).unwrap_or_else(collection_rt::jet_coll_chain);
        let key = coll_string(rt, key);
        coll_option_string(rt, collection_rt::jet_coll_chain_get(&chain, &key))
    })
}

fn jet_jit_coll_chain_contains(chain: i64, key: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let chain = coll_read_chain(rt, chain).unwrap_or_else(collection_rt::jet_coll_chain);
        let key = coll_string(rt, key);
        i8::from(collection_rt::jet_coll_chain_contains(&chain, &key))
    })
}


fn alloc_f64_list(values: &[f64]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let handle = rt.heap.alloc_empty_list();
        for &value in values {
            let _ = rt.heap.list_push_float(handle, value);
        }
        handle
    })
}

fn jet_jit_stats_sum(list: i64) -> f64 {
    math_rt::jet_stats_sum(list_f64s(list))
}

fn jet_jit_stats_prod(list: i64) -> f64 {
    math_rt::jet_stats_prod(list_f64s(list))
}

fn jet_jit_stats_count(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.list_len(list).unwrap_or(0))
}

fn jet_jit_stats_cumsum(list: i64) -> i64 {
    alloc_f64_list(&math_rt::jet_stats_cumsum(list_f64s(list)))
}

fn jet_jit_stats_cumprod(list: i64) -> i64 {
    alloc_f64_list(&math_rt::jet_stats_cumprod(list_f64s(list)))
}

fn jet_jit_stats_diff(list: i64) -> i64 {
    alloc_f64_list(&math_rt::jet_stats_diff(list_f64s(list)))
}

fn jet_jit_stats_moving_average(list: i64, window: i64) -> i64 {
    alloc_f64_list(&math_rt::jet_stats_moving_average(list_f64s(list), window))
}

fn jet_jit_stats_ewma(list: i64, alpha: f64) -> i64 {
    alloc_f64_list(&math_rt::jet_stats_ewma(list_f64s(list), alpha))
}

fn jet_jit_stats_rank(list: i64) -> i64 {
    alloc_f64_list(&math_rt::jet_stats_rank(list_f64s(list)))
}

fn jet_jit_stats_zscore(list: i64) -> i64 {
    alloc_f64_list(&math_rt::jet_stats_zscore(list_f64s(list)))
}

fn jet_jit_stats_clip(list: i64, low: f64, high: f64) -> i64 {
    alloc_f64_list(&math_rt::jet_stats_clip(list_f64s(list), low, high))
}
fn opt_f64(value: Option<f64>) -> i64 {
    match value {
        Some(value) => (value.to_bits() as i64).wrapping_add(1),
        None => 0,
    }
}

macro_rules! stats_opt_unary {
    ($name:ident, $function:ident) => {
        fn $name(list: i64) -> i64 {
            opt_f64(math_rt::$function(list_f64s(list)))
        }
    };
}
macro_rules! stats_opt_binary {
    ($name:ident, $function:ident) => {
        fn $name(left: i64, right: i64) -> i64 {
            opt_f64(math_rt::$function(list_f64s(left), list_f64s(right)))
        }
    };
}
macro_rules! stats_opt_float {
    ($name:ident, $function:ident) => {
        fn $name(list: i64, value: f64) -> i64 {
            opt_f64(math_rt::$function(list_f64s(list), value))
        }
    };
}
macro_rules! stats_list_unary {
    ($name:ident, $function:ident) => {
        fn $name(list: i64) -> i64 {
            alloc_f64_list(&math_rt::$function(list_f64s(list)))
        }
    };
}
macro_rules! stats_list_float {
    ($name:ident, $function:ident) => {
        fn $name(list: i64, value: f64) -> i64 {
            alloc_f64_list(&math_rt::$function(list_f64s(list), value))
        }
    };
}
macro_rules! stats_list_float_int {
    ($name:ident, $function:ident) => {
        fn $name(list: i64, value: f64, count: i64) -> i64 {
            alloc_f64_list(&math_rt::$function(list_f64s(list), value, count))
        }
    };
}
macro_rules! stats_list_int {
    ($name:ident, $function:ident) => {
        fn $name(list: i64, count: i64) -> i64 {
            alloc_f64_list(&math_rt::$function(list_f64s(list), count))
        }
    };
}
macro_rules! stats_list_binary {
    ($name:ident, $function:ident) => {
        fn $name(left: i64, right: i64) -> i64 {
            alloc_f64_list(&math_rt::$function(list_f64s(left), list_f64s(right)))
        }
    };
}
macro_rules! stats_list_histogram {
    ($name:ident, $function:ident) => {
        fn $name(list: i64, bins: i64) -> i64 {
            alloc_i64_list(&math_rt::$function(list_f64s(list), bins))
        }
    };
}

stats_opt_unary!(jet_jit_stats_mean, jet_stats_mean);
stats_opt_unary!(jet_jit_stats_geometric_mean, jet_stats_geometric_mean);
stats_opt_unary!(jet_jit_stats_harmonic_mean, jet_stats_harmonic_mean);
stats_opt_unary!(jet_jit_stats_median, jet_stats_median);
stats_opt_unary!(jet_jit_stats_median_low, jet_stats_median_low);
stats_opt_unary!(jet_jit_stats_median_high, jet_stats_median_high);
stats_opt_unary!(jet_jit_stats_mode, jet_stats_mode);
stats_opt_unary!(jet_jit_stats_pvariance, jet_stats_pvariance);
stats_opt_unary!(jet_jit_stats_pstdev, jet_stats_pstdev);
stats_opt_unary!(jet_jit_stats_variance, jet_stats_variance);
stats_opt_unary!(jet_jit_stats_stdev, jet_stats_stdev);
stats_opt_unary!(jet_jit_stats_iqr, jet_stats_iqr);
stats_opt_unary!(jet_jit_stats_mad, jet_stats_mad);
stats_opt_unary!(jet_jit_stats_mean_abs_deviation, jet_stats_mean_abs_deviation);
stats_opt_unary!(jet_jit_stats_skew, jet_stats_skew);
stats_opt_unary!(jet_jit_stats_kurtosis, jet_stats_kurtosis);
stats_opt_unary!(jet_jit_stats_min, jet_stats_min);
stats_opt_unary!(jet_jit_stats_max, jet_stats_max);
stats_opt_unary!(jet_jit_stats_range, jet_stats_range);
stats_opt_binary!(jet_jit_stats_covariance, jet_stats_covariance);
stats_opt_binary!(jet_jit_stats_correlation, jet_stats_correlation);
stats_opt_binary!(jet_jit_stats_sumprod, jet_stats_sumprod);
stats_opt_binary!(jet_jit_stats_weighted_mean, jet_stats_weighted_mean);
stats_opt_binary!(jet_jit_stats_spearman, jet_stats_spearman);
stats_opt_binary!(jet_jit_stats_pearson, jet_stats_correlation);
stats_opt_binary!(jet_jit_stats_covariance_population, jet_stats_covariance_population);
stats_opt_float!(jet_jit_stats_median_grouped, jet_stats_median_grouped);
stats_opt_float!(jet_jit_stats_quantile, jet_stats_quantile);
stats_opt_float!(jet_jit_stats_percentile, jet_stats_percentile);
stats_list_unary!(jet_jit_stats_multimode, jet_stats_multimode);
stats_list_float!(jet_jit_stats_kde, jet_stats_kde);
stats_list_float_int!(jet_jit_stats_kde_random, jet_stats_kde_random);
stats_list_int!(jet_jit_stats_quantiles, jet_stats_quantiles);
stats_list_binary!(jet_jit_stats_residuals, jet_stats_residuals);
stats_list_histogram!(jet_jit_stats_histogram, jet_stats_histogram);
stats_list_float!(jet_jit_stats_winsorize, jet_stats_winsorize);

fn jet_jit_math_min_i64(left: i64, right: i64) -> i64 {
    math_rt::jet_std_math_min_i64(left, right)
}
fn jet_jit_math_max_i64(left: i64, right: i64) -> i64 {
    math_rt::jet_std_math_max_i64(left, right)
}
fn jet_jit_math_clamp_i64(value: i64, low: i64, high: i64) -> i64 {
    math_rt::jet_std_math_clamp_i64(value, low, high)
}
fn jet_jit_math_min_f64(left: f64, right: f64) -> f64 {
    math_rt::jet_std_math_min_f64(left, right)
}
fn jet_jit_math_max_f64(left: f64, right: f64) -> f64 {
    math_rt::jet_std_math_max_f64(left, right)
}
fn jet_jit_math_clamp_f64(value: f64, low: f64, high: f64) -> f64 {
    math_rt::jet_std_math_clamp_f64(value, low, high)
}
fn jet_jit_math_checked_sub(left: i64, right: i64) -> i64 {
    opt_i64(math_rt::jet_std_math_checked_sub(left, right))
}
fn jet_jit_math_checked_mul(left: i64, right: i64) -> i64 {
    opt_i64(math_rt::jet_std_math_checked_mul(left, right))
}
fn jet_jit_math_saturating_sub(left: i64, right: i64) -> i64 {
    math_rt::jet_std_math_saturating_sub(left, right)
}
fn jet_jit_math_saturating_mul(left: i64, right: i64) -> i64 {
    math_rt::jet_std_math_saturating_mul(left, right)
}
fn jet_jit_math_tau() -> f64 {
    math_rt::jet_std_math_tau()
}
fn jet_jit_math_hypot3(a: f64, b: f64, c: f64) -> f64 {
    math_rt::jet_std_math_hypot3(a, b, c)
}
fn jet_jit_math_midpoint(a: f64, b: f64) -> f64 {
    math_rt::jet_std_math_midpoint(a, b)
}
fn jet_jit_math_gcd_many(values: i64) -> i64 {
    let values = list_i64s(values);
    math_rt::jet_std_math_gcd_many(&values)
}
fn jet_jit_math_lcm_many(values: i64) -> i64 {
    let values = list_i64s(values);
    math_rt::jet_std_math_lcm_many(&values)
}
fn jet_jit_math_powmod(base: i64, exp: i64, modulus: i64) -> i64 {
    opt_i64(math_rt::jet_std_math_powmod(base, exp, modulus))
}
fn jet_jit_math_abs_diff(left: i64, right: i64) -> i64 {
    math_rt::jet_std_math_abs_diff(left, right)
}
fn jet_jit_math_in_range(value: i64, low: i64, high: i64) -> i8 {
    i8::from(math_rt::jet_std_math_in_range(value, low, high))
}
fn jet_jit_math_xor(left: i64, right: i64) -> i64 {
    math_rt::jet_std_math_xor(left, right)
}
fn jet_jit_math_min_f32(left: f64, right: f64) -> f64 {
    math_rt::jet_std_math_min_f32(left as f32, right as f32) as f64
}
fn jet_jit_math_max_f32(left: f64, right: f64) -> f64 {
    math_rt::jet_std_math_max_f32(left as f32, right as f32) as f64
}
fn jet_jit_math_clamp_f32(value: f64, low: f64, high: f64) -> f64 {
    math_rt::jet_std_math_clamp_f32(value as f32, low as f32, high as f32) as f64
}

fn pair_ff(a: f64, b: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let h = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_float(h, 0, a);
        let _ = rt.heap.record_set_float(h, 1, b);
        h
    })
}

fn pair_fi(a: f64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let h = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_float(h, 0, a);
        let _ = rt.heap.record_set_int(h, 1, b);
        h
    })
}

fn pair_ii(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let h = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(h, 0, a);
        let _ = rt.heap.record_set_int(h, 1, b);
        h
    })
}

// ── Prelude-backed (#1464) ───────────────────────────────────────────────────

fn jet_jit_math_erf(x: f64) -> f64 {
    math_rt::jet_std_math_erf(x)
}
fn jet_jit_math_erfc(x: f64) -> f64 {
    math_rt::jet_std_math_erfc(x)
}
fn jet_jit_math_gamma(x: f64) -> f64 {
    math_rt::jet_std_math_gamma(x)
}
fn jet_jit_math_lgamma(x: f64) -> f64 {
    math_rt::jet_std_math_lgamma(x)
}
fn jet_jit_math_ulp(x: f64) -> f64 {
    math_rt::jet_std_math_ulp(x)
}
fn jet_jit_math_significand(x: f64) -> f64 {
    math_rt::jet_std_math_significand(x)
}
fn jet_jit_math_logb(x: f64) -> f64 {
    math_rt::jet_std_math_logb(x)
}
fn jet_jit_math_ldexp(x: f64, exp: i64) -> f64 {
    math_rt::jet_std_math_ldexp(x, exp)
}
fn jet_jit_math_next_after(x: f64, toward: f64) -> f64 {
    math_rt::jet_std_math_next_after(x, toward)
}
fn jet_jit_math_cmp(a: f64, b: f64) -> i64 {
    math_rt::jet_std_math_cmp(a, b)
}
fn jet_jit_math_ilogb(x: f64) -> i64 {
    opt_i64(math_rt::jet_std_math_ilogb(x))
}
fn jet_jit_math_isqrt(v: i64) -> i64 {
    opt_i64(math_rt::jet_std_math_isqrt(v))
}
fn jet_jit_math_factorial(v: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.int_factorial(v) {
        Some(value) => value.wrapping_add(1),
        None => 0,
    })
}
fn jet_jit_math_binomial(n: i64, k: i64) -> i64 {
    opt_i64(math_rt::jet_std_math_binomial(n, k))
}
fn jet_jit_math_perm(n: i64, k: i64) -> i64 {
    opt_i64(math_rt::jet_std_math_perm(n, k))
}
fn jet_jit_math_rising_factorial(n: i64, k: i64) -> i64 {
    opt_i64(math_rt::jet_std_math_rising_factorial(n, k))
}
fn jet_jit_math_multinomial(parts: i64) -> i64 {
    opt_i64(math_rt::jet_std_math_multinomial(list_i64s(parts)))
}
fn jet_jit_math_fmod(a: f64, b: f64) -> f64 {
    math_rt::jet_std_math_fmod(a, b)
}
fn jet_jit_math_remainder(a: f64, b: f64) -> f64 {
    math_rt::jet_std_math_remainder(a, b)
}
fn jet_jit_math_isclose(a: f64, b: f64, rel: f64, abs_tol: f64) -> i8 {
    i8::from(math_rt::jet_std_math_isclose(a, b, rel, abs_tol))
}
fn jet_jit_math_dist(left: i64, right: i64) -> f64 {
    math_rt::jet_std_math_dist(list_f64s(left), list_f64s(right))
}
fn jet_jit_math_digits(v: i64) -> i64 {
    math_rt::jet_std_math_digits(v)
}
fn jet_jit_math_leading_ones(v: i64) -> i64 {
    math_rt::jet_std_math_leading_ones(v)
}
fn jet_jit_math_trailing_ones(v: i64) -> i64 {
    math_rt::jet_std_math_trailing_ones(v)
}

// ── AOT-inlined f64/i64 methods (same semantics, marshall only) ──────────────

fn jet_jit_math_asinh(x: f64) -> f64 {
    x.asinh()
}
fn jet_jit_math_acosh(x: f64) -> f64 {
    x.acosh()
}
fn jet_jit_math_atanh(x: f64) -> f64 {
    x.atanh()
}
fn jet_jit_math_atan(x: f64) -> f64 {
    x.atan()
}
fn jet_jit_math_asin(x: f64) -> f64 {
    x.asin()
}
fn jet_jit_math_acos(x: f64) -> f64 {
    x.acos()
}
fn jet_jit_math_tan(x: f64) -> f64 {
    x.tan()
}
fn jet_jit_math_sinh(x: f64) -> f64 {
    x.sinh()
}
fn jet_jit_math_cosh(x: f64) -> f64 {
    x.cosh()
}
fn jet_jit_math_tanh(x: f64) -> f64 {
    x.tanh()
}
fn jet_jit_math_ln(x: f64) -> f64 {
    x.ln()
}
fn jet_jit_math_trunc(x: f64) -> f64 {
    x.trunc()
}
fn jet_jit_math_fract(x: f64) -> f64 {
    x.fract()
}
fn jet_jit_math_cbrt(x: f64) -> f64 {
    x.cbrt()
}
fn jet_jit_math_exp2(x: f64) -> f64 {
    x.exp2()
}
fn jet_jit_math_exp_m1(x: f64) -> f64 {
    x.exp_m1()
}
fn jet_jit_math_ln_1p(x: f64) -> f64 {
    x.ln_1p()
}
fn jet_jit_math_log(x: f64, base: f64) -> f64 {
    x.log(base)
}
fn jet_jit_math_log10(x: f64) -> f64 {
    x.log10()
}
fn jet_jit_math_log2(x: f64) -> f64 {
    x.log2()
}
fn jet_jit_math_copysign(x: f64, y: f64) -> f64 {
    x.copysign(y)
}
fn jet_jit_math_signum(x: f64) -> f64 {
    x.signum()
}
fn jet_jit_math_fma(a: f64, b: f64, c: f64) -> f64 {
    a.mul_add(b, c)
}
fn jet_jit_math_identity_f64(x: f64) -> f64 {
    x
}
fn jet_jit_math_zero_f64(_: f64) -> f64 {
    0.0
}
fn jet_jit_math_is_even(n: i64) -> i8 {
    i8::from(n % 2 == 0)
}
fn jet_jit_math_is_odd(n: i64) -> i8 {
    i8::from(n % 2 != 0)
}
fn jet_jit_math_checked_abs(n: i64) -> i64 {
    opt_i64(n.checked_abs())
}
fn jet_jit_math_checked_neg(n: i64) -> i64 {
    opt_i64(n.checked_neg())
}
fn jet_jit_math_checked_div(a: i64, b: i64) -> i64 {
    opt_i64(a.checked_div(b))
}
fn jet_jit_math_checked_rem(a: i64, b: i64) -> i64 {
    opt_i64(a.checked_rem(b))
}
fn jet_jit_math_is_normal(x: f64) -> i8 {
    i8::from(x.is_normal())
}
fn jet_jit_math_is_subnormal(x: f64) -> i8 {
    i8::from(x.is_subnormal())
}
fn jet_jit_math_is_canonical(x: f64) -> i8 {
    i8::from(x.is_finite() || x.is_nan())
}
fn jet_jit_math_is_signed(x: f64) -> i8 {
    i8::from(x.is_sign_negative())
}
fn jet_jit_math_is_zero_f(x: f64) -> i8 {
    i8::from(x == 0.0)
}
fn jet_jit_math_is_integer(x: f64) -> i8 {
    i8::from(x.is_finite() && x.fract() == 0.0)
}
fn jet_jit_math_next_up(x: f64) -> f64 {
    x.next_up()
}
fn jet_jit_math_next_down(x: f64) -> f64 {
    x.next_down()
}
fn jet_jit_math_radix(_x: f64) -> i64 {
    2
}
fn jet_jit_math_zero() -> f64 {
    0.0
}
fn jet_jit_math_copy(x: f64) -> f64 {
    x
}
fn jet_jit_math_cot(x: f64) -> f64 {
    1.0 / x.tan()
}
fn jet_jit_math_inv(x: f64) -> f64 {
    1.0 / x
}
fn jet_jit_math_sin_cos(x: f64) -> i64 {
    let (s, c) = x.sin_cos();
    pair_ff(s, c)
}
fn jet_jit_math_modf(x: f64) -> i64 {
    pair_ff(x.fract(), x.trunc())
}
fn jet_jit_math_frexp(x: f64) -> i64 {
    let exp = math_rt::jet_std_math_ilogb(x).unwrap_or(0);
    let frac = if x == 0.0 || !x.is_finite() {
        x
    } else {
        math_rt::jet_std_math_ldexp(x, -exp)
    };
    pair_fi(frac, exp)
}
fn jet_jit_math_div_mod(a: i64, b: i64) -> i64 {
    if b == 0 {
        return pair_ii(0, 0);
    }
    pair_ii(a.div_euclid(b), a.rem_euclid(b))
}
fn jet_jit_math_div_rem(a: i64, b: i64) -> i64 {
    if b == 0 {
        return pair_ii(0, 0);
    }
    pair_ii(a / b, a % b)
}

host_fns! {
    struct MathExtraHostFns;
    register: register_math_extra_symbols;
    declare: declare_math_extra_host_fns(module) {
        let cc = module.target_config().default_call_conv;
        let mut zero_f64 = Signature::new(cc);
        zero_f64.returns.push(AbiParam::new(types::F64));
        let mut zero_i64 = Signature::new(cc);
        zero_i64.returns.push(AbiParam::new(types::I64));
        let mut f64_f64 = Signature::new(cc);
        f64_f64.params.push(AbiParam::new(types::F64));
        f64_f64.returns.push(AbiParam::new(types::F64));
        let mut f64_f64_f64 = Signature::new(cc);
        f64_f64_f64.params.push(AbiParam::new(types::F64));
        f64_f64_f64.params.push(AbiParam::new(types::F64));
        f64_f64_f64.returns.push(AbiParam::new(types::F64));
        let mut f64_f64_f64_f64 = Signature::new(cc);
        f64_f64_f64_f64.params.push(AbiParam::new(types::F64));
        f64_f64_f64_f64.params.push(AbiParam::new(types::F64));
        f64_f64_f64_f64.params.push(AbiParam::new(types::F64));
        f64_f64_f64_f64.returns.push(AbiParam::new(types::F64));
        let mut f64_f64_f64_f64_i8 = Signature::new(cc);
        f64_f64_f64_f64_i8.params.push(AbiParam::new(types::F64));
        f64_f64_f64_f64_i8.params.push(AbiParam::new(types::F64));
        f64_f64_f64_f64_i8.params.push(AbiParam::new(types::F64));
        f64_f64_f64_f64_i8.params.push(AbiParam::new(types::F64));
        f64_f64_f64_f64_i8.returns.push(AbiParam::new(types::I8));
        let mut i64_i64_f64 = Signature::new(cc);
        i64_i64_f64.params.push(AbiParam::new(types::I64));
        i64_i64_f64.params.push(AbiParam::new(types::I64));
        i64_i64_f64.returns.push(AbiParam::new(types::F64));
        let mut f64_i64_f64 = Signature::new(cc);
        f64_i64_f64.params.push(AbiParam::new(types::F64));
        f64_i64_f64.params.push(AbiParam::new(types::I64));
        f64_i64_f64.returns.push(AbiParam::new(types::F64));
        let mut f64_i64 = Signature::new(cc);
        f64_i64.params.push(AbiParam::new(types::F64));
        f64_i64.returns.push(AbiParam::new(types::I64));
        let mut i64_f64 = Signature::new(cc);
        i64_f64.params.push(AbiParam::new(types::I64));
        i64_f64.returns.push(AbiParam::new(types::F64));
        let mut f64_i8 = Signature::new(cc);
        f64_i8.params.push(AbiParam::new(types::F64));
        f64_i8.returns.push(AbiParam::new(types::I8));
        let mut f64_f64_i64 = Signature::new(cc);
        f64_f64_i64.params.push(AbiParam::new(types::F64));
        f64_f64_i64.params.push(AbiParam::new(types::F64));
        f64_f64_i64.returns.push(AbiParam::new(types::I64));
        let mut fma = Signature::new(cc);
        fma.params.push(AbiParam::new(types::F64));
        fma.params.push(AbiParam::new(types::F64));
        fma.params.push(AbiParam::new(types::F64));
        fma.returns.push(AbiParam::new(types::F64));
        let mut i64_i64 = Signature::new(cc);
        i64_i64.params.push(AbiParam::new(types::I64));
        i64_i64.returns.push(AbiParam::new(types::I64));
        let mut i64_i8 = Signature::new(cc);
        i64_i8.params.push(AbiParam::new(types::I64));
        i64_i8.returns.push(AbiParam::new(types::I8));
        let mut i64_i64_i8 = Signature::new(cc);
        i64_i64_i8.params.push(AbiParam::new(types::I64));
        i64_i64_i8.params.push(AbiParam::new(types::I64));
        i64_i64_i8.returns.push(AbiParam::new(types::I8));
        let mut i64_i64_i64 = Signature::new(cc);
        i64_i64_i64.params.push(AbiParam::new(types::I64));
        i64_i64_i64.params.push(AbiParam::new(types::I64));
        i64_i64_i64.returns.push(AbiParam::new(types::I64));
        let mut i64_i64_i64_i8 = Signature::new(cc);
        i64_i64_i64_i8.params.push(AbiParam::new(types::I64));
        i64_i64_i64_i8.params.push(AbiParam::new(types::I64));
        i64_i64_i64_i8.params.push(AbiParam::new(types::I64));
        i64_i64_i64_i8.returns.push(AbiParam::new(types::I8));
        let mut i64_i64_i64_i64 = Signature::new(cc);
        i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        i64_i64_i64_i64.returns.push(AbiParam::new(types::I64));
        let mut f64_handle = Signature::new(cc);
        f64_handle.params.push(AbiParam::new(types::F64));
        f64_handle.returns.push(AbiParam::new(types::I64));
        let mut list_f64 = Signature::new(cc);
        list_f64.params.push(AbiParam::new(types::I64));
        list_f64.returns.push(AbiParam::new(types::F64));
        let mut list_i64 = Signature::new(cc);
        list_i64.params.push(AbiParam::new(types::I64));
        list_i64.returns.push(AbiParam::new(types::I64));
        let mut list_handle = Signature::new(cc);
        list_handle.params.push(AbiParam::new(types::I64));
        list_handle.returns.push(AbiParam::new(types::I64));
        let mut list_i64_handle = Signature::new(cc);
        list_i64_handle.params.push(AbiParam::new(types::I64));
        list_i64_handle.params.push(AbiParam::new(types::I64));
        list_i64_handle.returns.push(AbiParam::new(types::I64));
        let mut list_f64_handle = Signature::new(cc);
        list_f64_handle.params.push(AbiParam::new(types::I64));
        list_f64_handle.params.push(AbiParam::new(types::F64));
        list_f64_handle.returns.push(AbiParam::new(types::I64));
        let mut list_f64_f64_handle = Signature::new(cc);
        list_f64_f64_handle.params.push(AbiParam::new(types::I64));
        list_f64_f64_handle.params.push(AbiParam::new(types::F64));
        list_f64_f64_handle.params.push(AbiParam::new(types::F64));
        list_f64_f64_handle.returns.push(AbiParam::new(types::I64));
        let mut list_i64_i8_handle = Signature::new(cc);
        list_i64_i8_handle.params.push(AbiParam::new(types::I64));
        list_i64_i8_handle.params.push(AbiParam::new(types::I8));
        list_i64_i8_handle.returns.push(AbiParam::new(types::I64));
        let mut list_list_handle = Signature::new(cc);
        list_list_handle.params.push(AbiParam::new(types::I64));
        list_list_handle.params.push(AbiParam::new(types::I64));
        list_list_handle.returns.push(AbiParam::new(types::I64));
        let mut i64_i64_handle = Signature::new(cc);
        i64_i64_handle.params.push(AbiParam::new(types::I64));
        i64_i64_handle.params.push(AbiParam::new(types::I64));
        i64_i64_handle.returns.push(AbiParam::new(types::I64));
        let mut i64_i64_i64_handle = Signature::new(cc);
        i64_i64_i64_handle.params.push(AbiParam::new(types::I64));
        i64_i64_i64_handle.params.push(AbiParam::new(types::I64));
        i64_i64_i64_handle.params.push(AbiParam::new(types::I64));
        i64_i64_i64_handle.returns.push(AbiParam::new(types::I64));
        let mut list_i64_i64_i64_i64_handle = Signature::new(cc);
        list_i64_i64_i64_i64_handle.params.push(AbiParam::new(types::I64));
        list_i64_i64_i64_i64_handle.params.push(AbiParam::new(types::I64));
        list_i64_i64_i64_i64_handle.params.push(AbiParam::new(types::I64));
        list_i64_i64_i64_i64_handle.params.push(AbiParam::new(types::I64));
        list_i64_i64_i64_i64_handle.returns.push(AbiParam::new(types::I64));
        let mut list_list_i64_handle = Signature::new(cc);
        list_list_i64_handle.params.push(AbiParam::new(types::I64));
        list_list_i64_handle.params.push(AbiParam::new(types::I64));
        list_list_i64_handle.params.push(AbiParam::new(types::I64));
        list_list_i64_handle.returns.push(AbiParam::new(types::I64));
        let mut list_f64_f64_i64_handle = Signature::new(cc);
        list_f64_f64_i64_handle.params.push(AbiParam::new(types::I64));
        list_f64_f64_i64_handle.params.push(AbiParam::new(types::F64));
        list_f64_f64_i64_handle.params.push(AbiParam::new(types::I64));
        list_f64_f64_i64_handle.returns.push(AbiParam::new(types::I64));
    }
    abs_i64: "jet_jit_math_abs_i64" => jet_jit_math_abs_i64: i64_i64;
    checked_sub: "jet_jit_math_checked_sub" => jet_jit_math_checked_sub: i64_i64_i64;
    checked_mul: "jet_jit_math_checked_mul" => jet_jit_math_checked_mul: i64_i64_i64;
    saturating_sub: "jet_jit_math_saturating_sub" => jet_jit_math_saturating_sub: i64_i64_i64;
    saturating_mul: "jet_jit_math_saturating_mul" => jet_jit_math_saturating_mul: i64_i64_i64;
    abs_diff: "jet_jit_math_abs_diff" => jet_jit_math_abs_diff: i64_i64_i64;
    in_range: "jet_jit_math_in_range" => jet_jit_math_in_range: i64_i64_i64_i8;
    xor: "jet_jit_math_xor" => jet_jit_math_xor: i64_i64_i64;
    gcd_many: "jet_jit_math_gcd_many" => jet_jit_math_gcd_many: list_i64;
    lcm_many: "jet_jit_math_lcm_many" => jet_jit_math_lcm_many: list_i64;
    powmod: "jet_jit_math_powmod" => jet_jit_math_powmod: i64_i64_i64_i64;
    tau: "jet_jit_math_tau" => jet_jit_math_tau: zero_f64;
    hypot3: "jet_jit_math_hypot3" => jet_jit_math_hypot3: f64_f64_f64_f64;
    midpoint: "jet_jit_math_midpoint" => jet_jit_math_midpoint: f64_f64_f64;
    pi: "jet_jit_math_pi" => jet_jit_math_pi: zero_f64;
    abs_f64: "jet_jit_math_abs_f64" => jet_jit_math_abs_f64: f64_f64;
    abs_f32: "jet_jit_math_abs_f32" => jet_jit_math_abs_f32: f64_f64;
    abs_f64_core: "jet_std_math_abs_f64" => jet_jit_math_abs_f64: f64_f64;
    abs_f32_core: "jet_std_math_abs_f32" => jet_jit_math_abs_f32: f64_f64;
    to_bits: "jet_jit_math_to_bits" => jet_jit_math_to_bits: f64_i64;
    from_bits: "jet_jit_math_from_bits" => jet_jit_math_from_bits: i64_f64;
    round: "jet_jit_math_round" => jet_jit_math_round: f64_i64;
    erf: "jet_jit_math_erf" => jet_jit_math_erf: f64_f64;
    erfc: "jet_jit_math_erfc" => jet_jit_math_erfc: f64_f64;
    gamma: "jet_jit_math_gamma" => jet_jit_math_gamma: f64_f64;
    lgamma: "jet_jit_math_lgamma" => jet_jit_math_lgamma: f64_f64;
    ulp: "jet_jit_math_ulp" => jet_jit_math_ulp: f64_f64;
    significand: "jet_jit_math_significand" => jet_jit_math_significand: f64_f64;
    logb: "jet_jit_math_logb" => jet_jit_math_logb: f64_f64;
    ldexp: "jet_jit_math_ldexp" => jet_jit_math_ldexp: f64_i64_f64;
    next_after: "jet_jit_math_next_after" => jet_jit_math_next_after: f64_f64_f64;
    cmp: "jet_jit_math_cmp" => jet_jit_math_cmp: f64_f64_i64;
    min_i64: "jet_jit_math_min_i64" => jet_jit_math_min_i64: i64_i64_i64;
    max_i64: "jet_jit_math_max_i64" => jet_jit_math_max_i64: i64_i64_i64;
    clamp_i64: "jet_jit_math_clamp_i64" => jet_jit_math_clamp_i64: i64_i64_i64_i64;
    min_f64: "jet_jit_math_min_f64" => jet_jit_math_min_f64: f64_f64_f64;
    max_f64: "jet_jit_math_max_f64" => jet_jit_math_max_f64: f64_f64_f64;
    clamp_f64: "jet_jit_math_clamp_f64" => jet_jit_math_clamp_f64: f64_f64_f64_f64;
    min_f32: "jet_jit_math_min_f32" => jet_jit_math_min_f32: f64_f64_f64;
    max_f32: "jet_jit_math_max_f32" => jet_jit_math_max_f32: f64_f64_f64;
    clamp_f32: "jet_jit_math_clamp_f32" => jet_jit_math_clamp_f32: f64_f64_f64_f64;
    ilogb: "jet_jit_math_ilogb" => jet_jit_math_ilogb: f64_i64;
    isqrt: "jet_jit_math_isqrt" => jet_jit_math_isqrt: i64_i64;
    factorial: "jet_jit_math_factorial" => jet_jit_math_factorial: i64_i64;
    binomial: "jet_jit_math_binomial" => jet_jit_math_binomial: i64_i64_i64;
    perm: "jet_jit_math_perm" => jet_jit_math_perm: i64_i64_i64;
    rising_factorial: "jet_jit_math_rising_factorial" => jet_jit_math_rising_factorial: i64_i64_i64;
    multinomial: "jet_jit_math_multinomial" => jet_jit_math_multinomial: list_i64;
    fmod: "jet_jit_math_fmod" => jet_jit_math_fmod: f64_f64_f64;
    remainder: "jet_jit_math_remainder" => jet_jit_math_remainder: f64_f64_f64;
    isclose: "jet_jit_math_isclose" => jet_jit_math_isclose: f64_f64_f64_f64_i8;
    dist: "jet_jit_math_dist" => jet_jit_math_dist: i64_i64_f64;
    digits: "jet_jit_math_digits" => jet_jit_math_digits: i64_i64;
    leading_ones: "jet_jit_math_leading_ones" => jet_jit_math_leading_ones: i64_i64;
    trailing_ones: "jet_jit_math_trailing_ones" => jet_jit_math_trailing_ones: i64_i64;
    asinh: "jet_jit_math_asinh" => jet_jit_math_asinh: f64_f64;
    ln: "jet_jit_math_ln" => jet_jit_math_ln: f64_f64;
    trunc: "jet_jit_math_trunc" => jet_jit_math_trunc: f64_f64;
    fract: "jet_jit_math_fract" => jet_jit_math_fract: f64_f64;
    acosh: "jet_jit_math_acosh" => jet_jit_math_acosh: f64_f64;
    atanh: "jet_jit_math_atanh" => jet_jit_math_atanh: f64_f64;
    atan: "jet_jit_math_atan" => jet_jit_math_atan: f64_f64;
    asin: "jet_jit_math_asin" => jet_jit_math_asin: f64_f64;
    acos: "jet_jit_math_acos" => jet_jit_math_acos: f64_f64;
    tan: "jet_jit_math_tan" => jet_jit_math_tan: f64_f64;
    sinh: "jet_jit_math_sinh" => jet_jit_math_sinh: f64_f64;
    cosh: "jet_jit_math_cosh" => jet_jit_math_cosh: f64_f64;
    tanh: "jet_jit_math_tanh" => jet_jit_math_tanh: f64_f64;
    cbrt: "jet_jit_math_cbrt" => jet_jit_math_cbrt: f64_f64;
    exp2: "jet_jit_math_exp2" => jet_jit_math_exp2: f64_f64;
    exp_m1: "jet_jit_math_exp_m1" => jet_jit_math_exp_m1: f64_f64;
    ln_1p: "jet_jit_math_ln_1p" => jet_jit_math_ln_1p: f64_f64;
    log: "jet_jit_math_log" => jet_jit_math_log: f64_f64_f64;
    log10: "jet_jit_math_log10" => jet_jit_math_log10: f64_f64;
    log2: "jet_jit_math_log2" => jet_jit_math_log2: f64_f64;
    copysign: "jet_jit_math_copysign" => jet_jit_math_copysign: f64_f64_f64;
    signum: "jet_jit_math_signum" => jet_jit_math_signum: f64_f64;
    fma: "jet_jit_math_fma" => jet_jit_math_fma: fma;
    identity_f64: "jet_jit_math_identity_f64" => jet_jit_math_identity_f64: f64_f64;
    coll_heapify: "jet_jit_coll_heapify" => jet_jit_coll_heapify: list_handle;
    coll_heappush: "jet_jit_coll_heappush" => jet_jit_coll_heappush: list_i64_handle;
    coll_heappop: "jet_jit_coll_heappop" => jet_jit_coll_heappop: list_handle;
    coll_heappushpop: "jet_jit_coll_heappushpop" => jet_jit_coll_heappushpop: list_i64_handle;
    coll_heapreplace: "jet_jit_coll_heapreplace" => jet_jit_coll_heapreplace: list_i64_handle;
    coll_bisect_left: "jet_jit_coll_bisect_left" => jet_jit_coll_bisect_left: i64_i64_i64;
    coll_bisect_right: "jet_jit_coll_bisect_right" => jet_jit_coll_bisect_right: i64_i64_i64;
    coll_insort_left: "jet_jit_coll_insort_left" => jet_jit_coll_insort_left: list_i64_handle;
    coll_insort_right: "jet_jit_coll_insort_right" => jet_jit_coll_insort_right: list_i64_handle;
    coll_merge_sorted: "jet_jit_coll_merge_sorted" => jet_jit_coll_merge_sorted: list_i64_handle;
    coll_nsmallest: "jet_jit_coll_nsmallest" => jet_jit_coll_nsmallest: i64_i64_handle;
    coll_nlargest: "jet_jit_coll_nlargest" => jet_jit_coll_nlargest: i64_i64_handle;
    coll_counter: "jet_jit_coll_counter" => jet_jit_coll_counter: zero_i64;
    coll_counter_from: "jet_jit_coll_counter_from" => jet_jit_coll_counter_from: list_handle;
    coll_counter_add: "jet_jit_coll_counter_add" => jet_jit_coll_counter_add: i64_i64_i64_i64;
    coll_counter_inc: "jet_jit_coll_counter_inc" => jet_jit_coll_counter_inc: i64_i64_handle;
    coll_counter_dec: "jet_jit_coll_counter_dec" => jet_jit_coll_counter_dec: i64_i64_handle;
    coll_counter_get: "jet_jit_coll_counter_get" => jet_jit_coll_counter_get: i64_i64_i64;
    coll_counter_set_count: "jet_jit_coll_counter_set_count" => jet_jit_coll_counter_set_count: i64_i64_i64_i64;
    coll_counter_total: "jet_jit_coll_counter_total" => jet_jit_coll_counter_total: i64_i64;
    coll_counter_names: "jet_jit_coll_counter_names" => jet_jit_coll_counter_names: i64_i64;
    coll_counter_elements: "jet_jit_coll_counter_elements" => jet_jit_coll_counter_elements: i64_i64;
    coll_counter_most_common: "jet_jit_coll_counter_most_common" => jet_jit_coll_counter_most_common: i64_i64_handle;
    coll_counter_subtract: "jet_jit_coll_counter_subtract" => jet_jit_coll_counter_subtract: i64_i64_handle;
    coll_counter_merge_add: "jet_jit_coll_counter_merge_add" => jet_jit_coll_counter_merge_add: i64_i64_handle;
    coll_counter_clear: "jet_jit_coll_counter_clear" => jet_jit_coll_counter_clear: i64_i64;
    coll_deque: "jet_jit_coll_deque" => jet_jit_coll_deque: zero_i64;
    coll_deque_from: "jet_jit_coll_deque_from" => jet_jit_coll_deque_from: list_handle;
    coll_deque_len: "jet_jit_coll_deque_len" => jet_jit_coll_deque_len: i64_i64;
    coll_deque_is_empty: "jet_jit_coll_deque_is_empty" => jet_jit_coll_deque_is_empty: i64_i8;
    coll_deque_append: "jet_jit_coll_deque_append" => jet_jit_coll_deque_append: i64_i64_handle;
    coll_deque_appendleft: "jet_jit_coll_deque_appendleft" => jet_jit_coll_deque_appendleft: i64_i64_handle;
    coll_deque_pop: "jet_jit_coll_deque_pop" => jet_jit_coll_deque_pop: i64_i64;
    coll_deque_popleft: "jet_jit_coll_deque_popleft" => jet_jit_coll_deque_popleft: i64_i64;
    coll_deque_peek: "jet_jit_coll_deque_peek" => jet_jit_coll_deque_peek: i64_i64;
    coll_deque_peekleft: "jet_jit_coll_deque_peekleft" => jet_jit_coll_deque_peekleft: i64_i64;
    coll_deque_extend: "jet_jit_coll_deque_extend" => jet_jit_coll_deque_extend: i64_i64_handle;
    coll_deque_extendleft: "jet_jit_coll_deque_extendleft" => jet_jit_coll_deque_extendleft: i64_i64_handle;
    coll_deque_rotate: "jet_jit_coll_deque_rotate" => jet_jit_coll_deque_rotate: i64_i64_handle;
    coll_deque_items: "jet_jit_coll_deque_items" => jet_jit_coll_deque_items: i64_i64;
    coll_ordered_map: "jet_jit_coll_ordered_map" => jet_jit_coll_ordered_map: zero_i64;
    coll_map_get: "jet_jit_coll_map_get" => jet_jit_coll_map_get: i64_i64_handle;
    coll_map_set: "jet_jit_coll_map_set" => jet_jit_coll_map_set: i64_i64_i64_handle;
    coll_map_remove: "jet_jit_coll_map_remove" => jet_jit_coll_map_remove: i64_i64_handle;
    coll_map_keys: "jet_jit_coll_map_keys" => jet_jit_coll_map_keys: i64_i64;
    coll_map_values: "jet_jit_coll_map_values" => jet_jit_coll_map_values: i64_i64;
    coll_map_contains: "jet_jit_coll_map_contains" => jet_jit_coll_map_contains: i64_i64_i8;
    coll_map_len: "jet_jit_coll_map_len" => jet_jit_coll_map_len: i64_i64;
    coll_chain: "jet_jit_coll_chain" => jet_jit_coll_chain: zero_i64;
    coll_chain_push: "jet_jit_coll_chain_push" => jet_jit_coll_chain_push: i64_i64_i64_handle;
    coll_chain_get: "jet_jit_coll_chain_get" => jet_jit_coll_chain_get: i64_i64_handle;
    coll_chain_contains: "jet_jit_coll_chain_contains" => jet_jit_coll_chain_contains: i64_i64_i8;
    is_even: "jet_jit_math_is_even" => jet_jit_math_is_even: i64_i8;
    is_odd: "jet_jit_math_is_odd" => jet_jit_math_is_odd: i64_i8;
    checked_abs: "jet_jit_math_checked_abs" => jet_jit_math_checked_abs: i64_i64;
    checked_neg: "jet_jit_math_checked_neg" => jet_jit_math_checked_neg: i64_i64;
    checked_div: "jet_jit_math_checked_div" => jet_jit_math_checked_div: i64_i64_i64;
    checked_rem: "jet_jit_math_checked_rem" => jet_jit_math_checked_rem: i64_i64_i64;
    is_nan: "jet_std_math_is_nan" => jet_jit_math_is_nan: f64_i8;
    is_infinite: "jet_std_math_is_infinite" => jet_jit_math_is_infinite: f64_i8;
    radix: "jet_jit_math_radix" => jet_jit_math_radix: f64_i64;
    zero: "jet_jit_math_zero" => jet_jit_math_zero: zero_f64;
    copy: "jet_jit_math_copy" => jet_jit_math_copy: f64_f64;
    is_finite: "jet_std_math_is_finite" => jet_jit_math_is_finite: f64_i8;
    is_normal: "jet_jit_math_is_normal" => jet_jit_math_is_normal: f64_i8;
    is_subnormal: "jet_jit_math_is_subnormal" => jet_jit_math_is_subnormal: f64_i8;
    is_canonical: "jet_jit_math_is_canonical" => jet_jit_math_is_canonical: f64_i8;
    is_signed: "jet_jit_math_is_signed" => jet_jit_math_is_signed: f64_i8;
    is_zero_f: "jet_jit_math_is_zero_f" => jet_jit_math_is_zero_f: f64_i8;
    is_integer: "jet_jit_math_is_integer" => jet_jit_math_is_integer: f64_i8;
    next_up: "jet_jit_math_next_up" => jet_jit_math_next_up: f64_f64;
    next_down: "jet_jit_math_next_down" => jet_jit_math_next_down: f64_f64;
    cot: "jet_jit_math_cot" => jet_jit_math_cot: f64_f64;
    inv: "jet_jit_math_inv" => jet_jit_math_inv: f64_f64;
    sin_cos: "jet_jit_math_sin_cos" => jet_jit_math_sin_cos: f64_handle;
    modf: "jet_jit_math_modf" => jet_jit_math_modf: f64_handle;
    frexp: "jet_jit_math_frexp" => jet_jit_math_frexp: f64_handle;
    div_mod: "jet_jit_math_div_mod" => jet_jit_math_div_mod: i64_i64_i64;
    div_rem: "jet_jit_math_div_rem" => jet_jit_math_div_rem: i64_i64_i64;
    stats_sum: "jet_jit_stats_sum" => jet_jit_stats_sum: list_f64;
    stats_prod: "jet_jit_stats_prod" => jet_jit_stats_prod: list_f64;
    stats_count: "jet_jit_stats_count" => jet_jit_stats_count: list_i64;
    stats_cumsum: "jet_jit_stats_cumsum" => jet_jit_stats_cumsum: list_handle;
    stats_cumprod: "jet_jit_stats_cumprod" => jet_jit_stats_cumprod: list_handle;
    stats_diff: "jet_jit_stats_diff" => jet_jit_stats_diff: list_handle;
    stats_mean: "jet_jit_stats_mean" => jet_jit_stats_mean: list_handle;
    stats_geometric_mean: "jet_jit_stats_geometric_mean" => jet_jit_stats_geometric_mean: list_handle;
    stats_harmonic_mean: "jet_jit_stats_harmonic_mean" => jet_jit_stats_harmonic_mean: list_handle;
    stats_median: "jet_jit_stats_median" => jet_jit_stats_median: list_handle;
    stats_median_grouped: "jet_jit_stats_median_grouped" => jet_jit_stats_median_grouped: list_f64_handle;
    stats_median_low: "jet_jit_stats_median_low" => jet_jit_stats_median_low: list_handle;
    stats_median_high: "jet_jit_stats_median_high" => jet_jit_stats_median_high: list_handle;
    stats_quantile: "jet_jit_stats_quantile" => jet_jit_stats_quantile: list_f64_handle;
    stats_percentile: "jet_jit_stats_percentile" => jet_jit_stats_percentile: list_f64_handle;
    stats_mode: "jet_jit_stats_mode" => jet_jit_stats_mode: list_handle;
    stats_multimode: "jet_jit_stats_multimode" => jet_jit_stats_multimode: list_handle;
    stats_pvariance: "jet_jit_stats_pvariance" => jet_jit_stats_pvariance: list_handle;
    stats_pstdev: "jet_jit_stats_pstdev" => jet_jit_stats_pstdev: list_handle;
    stats_variance: "jet_jit_stats_variance" => jet_jit_stats_variance: list_handle;
    stats_stdev: "jet_jit_stats_stdev" => jet_jit_stats_stdev: list_handle;
    stats_covariance: "jet_jit_stats_covariance" => jet_jit_stats_covariance: list_list_handle;
    stats_correlation: "jet_jit_stats_correlation" => jet_jit_stats_correlation: list_list_handle;
    stats_kde: "jet_jit_stats_kde" => jet_jit_stats_kde: list_f64_handle;
    stats_kde_random: "jet_jit_stats_kde_random" => jet_jit_stats_kde_random: list_f64_f64_i64_handle;
    stats_min: "jet_jit_stats_min" => jet_jit_stats_min: list_handle;
    stats_max: "jet_jit_stats_max" => jet_jit_stats_max: list_handle;
    stats_range: "jet_jit_stats_range" => jet_jit_stats_range: list_handle;
    stats_sumprod: "jet_jit_stats_sumprod" => jet_jit_stats_sumprod: list_list_handle;
    stats_weighted_mean: "jet_jit_stats_weighted_mean" => jet_jit_stats_weighted_mean: list_list_handle;
    stats_quantiles: "jet_jit_stats_quantiles" => jet_jit_stats_quantiles: list_i64_handle;
    stats_iqr: "jet_jit_stats_iqr" => jet_jit_stats_iqr: list_handle;
    stats_mad: "jet_jit_stats_mad" => jet_jit_stats_mad: list_handle;
    stats_mean_abs_deviation: "jet_jit_stats_mean_abs_deviation" => jet_jit_stats_mean_abs_deviation: list_handle;
    stats_skew: "jet_jit_stats_skew" => jet_jit_stats_skew: list_handle;
    stats_kurtosis: "jet_jit_stats_kurtosis" => jet_jit_stats_kurtosis: list_handle;
    stats_spearman: "jet_jit_stats_spearman" => jet_jit_stats_spearman: list_list_handle;
    stats_residuals: "jet_jit_stats_residuals" => jet_jit_stats_residuals: list_list_handle;
    stats_histogram: "jet_jit_stats_histogram" => jet_jit_stats_histogram: list_i64_handle;
    stats_winsorize: "jet_jit_stats_winsorize" => jet_jit_stats_winsorize: list_f64_handle;
    stats_pearson: "jet_jit_stats_pearson" => jet_jit_stats_pearson: list_list_handle;
    stats_covariance_population: "jet_jit_stats_covariance_population" => jet_jit_stats_covariance_population: list_list_handle;
    stats_moving_average: "jet_jit_stats_moving_average" => jet_jit_stats_moving_average: list_i64_handle;
    stats_ewma: "jet_jit_stats_ewma" => jet_jit_stats_ewma: list_f64_handle;
    stats_rank: "jet_jit_stats_rank" => jet_jit_stats_rank: list_handle;
    stats_zscore: "jet_jit_stats_zscore" => jet_jit_stats_zscore: list_handle;
    stats_clip: "jet_jit_stats_clip" => jet_jit_stats_clip: list_f64_f64_handle;
    comb_chain: "jet_jit_comb_chain" => jet_jit_comb_chain: list_list_handle;
    comb_compress: "jet_jit_comb_compress" => jet_jit_comb_compress: list_list_handle;
    comb_drop: "jet_jit_comb_drop" => jet_jit_comb_drop: list_i64_handle;
    comb_takewhile: "jet_jit_comb_takewhile" => jet_jit_comb_takewhile: list_i64_i8_handle;
    comb_dropwhile: "jet_jit_comb_dropwhile" => jet_jit_comb_dropwhile: list_i64_i8_handle;
    comb_filterfalse: "jet_jit_comb_filterfalse" => jet_jit_comb_filterfalse: list_i64_i8_handle;
    comb_islice: "jet_jit_comb_islice" => jet_jit_comb_islice: list_i64_i64_i64_i64_handle;
    comb_unique: "jet_jit_comb_unique" => jet_jit_comb_unique: list_handle;
    comb_repeat: "jet_jit_comb_repeat" => jet_jit_comb_repeat: i64_i64_handle;
    comb_count_from: "jet_jit_comb_count_from" => jet_jit_comb_count_from: i64_i64_i64_handle;
    comb_cycle: "jet_jit_comb_cycle" => jet_jit_comb_cycle: list_i64_handle;
    comb_accumulate: "jet_jit_comb_accumulate" => jet_jit_comb_accumulate: list_handle;
    comb_reverse: "jet_jit_comb_reverse" => jet_jit_comb_reverse: list_handle;
    comb_permutations: "jet_jit_comb_permutations" => jet_jit_comb_permutations: list_i64_handle;
    comb_combinations: "jet_jit_comb_combinations" => jet_jit_comb_combinations: list_i64_handle;
    comb_combinations_with_replacement: "jet_jit_comb_combinations_with_replacement" => jet_jit_comb_combinations_with_replacement: list_i64_handle;
    comb_product: "jet_jit_comb_product" => jet_jit_comb_product: list_i64_handle;
    comb_cartesian: "jet_jit_comb_cartesian" => jet_jit_comb_cartesian: list_list_handle;
    comb_pairwise: "jet_jit_comb_pairwise" => jet_jit_comb_pairwise: list_handle;
    comb_batched: "jet_jit_comb_batched" => jet_jit_comb_batched: list_i64_handle;
    comb_groupby: "jet_jit_comb_groupby" => jet_jit_comb_groupby: list_handle;
    comb_tee: "jet_jit_comb_tee" => jet_jit_comb_tee: list_i64_handle;
    comb_zip_longest: "jet_jit_comb_zip_longest" => jet_jit_comb_zip_longest: list_list_i64_handle;
    comb_powerset: "jet_jit_comb_powerset" => jet_jit_comb_powerset: list_handle;
    comb_windows: "jet_jit_comb_windows" => jet_jit_comb_windows: list_i64_handle;
    comb_flatten: "jet_jit_comb_flatten" => jet_jit_comb_flatten: list_handle;
    comb_starmap: "jet_jit_comb_starmap" => jet_jit_comb_starmap: list_handle;
}
