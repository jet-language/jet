//! M5: list/map host shims for the Cranelift JIT (`JetArena` handles).

use super::Concurrency;
use std::collections::{BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};

mod set_semantics {
    include!("../../jet-codegen/src/Prelude/Core/SetAlgebra.rs");
}

mod range_semantics {
    use jet_foundation::StructuralDebug::jet_debug_range;
    include!("../../jet-codegen/src/Prelude/Core/RangeBounds.rs");
}

mod disjoint_semantics {
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

fn option_i64(rt: &mut crate::JitRuntime, value: Option<i64>) -> i64 {
    crate::runtime_host::alloc_jit_result(
        rt,
        value.is_some(),
        value.unwrap_or_default() as u64,
    )
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

extern "C" fn jet_jit_list_slice(list: i64, start: i64, end: i64, _line: u32) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if rt.heap.list_len(list).is_none() {
            jet_foundation::ice!(None, "jit list slice: bad handle");
        }
        match rt.heap.list_slice(list, start, end) {
            Some(id) => id,
            None => {
                rt.set_trap("slice out of bounds: the range is outside the list");
                0
            }
        }
    })
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
    Concurrency::with_runtime_mut(|rt| {
        let len = rt
            .heap
            .map_len(map)
            .expect("jit map clone: bad handle");
        let out = rt.heap.alloc_empty_map();
        for i in 0..len {
            let key = rt
                .heap
                .map_key_at(map, i)
                .expect("jit map clone: key");
            let value = rt
                .heap
                .map_value_at(map, i)
                .expect("jit map clone: value");
            rt.heap
                .map_insert(out, key, value)
                .expect("jit map clone: insert");
        }
        out
    })
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

fn string_elems_eq(a: i64, b: i64) -> bool {
    if a == b {
        return true;
    }
    Concurrency::with_runtime_mut(|rt| {
        match (rt.heap.get_string(a), rt.heap.get_string(b)) {
            (Some(sa), Some(sb)) => sa == sb,
            _ => false,
        }
    })
}

extern "C" fn jet_jit_iter_take(list: i64, n: i64) -> i64 {
    let xs = clone_list_ints(list);
    let n = n.max(0) as usize;
    alloc_from_ints(&xs[..n.min(xs.len())])
}

extern "C" fn jet_jit_iter_skip(list: i64, n: i64) -> i64 {
    let xs = clone_list_ints(list);
    let n = n.max(0) as usize;
    if n >= xs.len() {
        alloc_from_ints(&[])
    } else {
        alloc_from_ints(&xs[n..])
    }
}

extern "C" fn jet_jit_iter_step_by(list: i64, n: i64) -> i64 {
    let xs = clone_list_ints(list);
    if n <= 0 {
        return alloc_from_ints(&[]);
    }
    let stepped: Vec<i64> = xs.into_iter().step_by(n as usize).collect();
    alloc_from_ints(&stepped)
}

/// `string_elems != 0` → compare string contents (handles may differ); else i64 eq.
extern "C" fn jet_jit_iter_dedup(list: i64, string_elems: i64) -> i64 {
    let xs = clone_list_ints(list);
    let string_elems = string_elems != 0;
    let mut out = Vec::new();
    let mut prev: Option<i64> = None;
    for x in xs {
        let dup = match prev {
            Some(p) if !string_elems => p == x,
            Some(p) => string_elems_eq(p, x),
            None => false,
        };
        if dup {
            continue;
        }
        prev = Some(x);
        out.push(x);
    }
    alloc_from_ints(&out)
}

extern "C" fn jet_jit_iter_chunks(list: i64, n: i64) -> i64 {
    let xs = clone_list_ints(list);
    let size = n.max(1) as usize;
    Concurrency::with_runtime_mut(|rt| {
        let out = rt.heap.alloc_empty_list();
        let mut i = 0usize;
        while i < xs.len() {
            let end = (i + size).min(xs.len());
            let chunk = rt.heap.alloc_empty_list();
            for &v in &xs[i..end] {
                rt.heap
                    .list_push_int(chunk, v)
                    .expect("jit iter chunks: push");
            }
            rt.heap
                .list_push_int(out, chunk)
                .expect("jit iter chunks: outer push");
            i = end;
        }
        out
    })
}

extern "C" fn jet_jit_iter_windows(list: i64, n: i64) -> i64 {
    let xs = clone_list_ints(list);
    let size = n.max(1) as usize;
    if xs.len() < size {
        return alloc_from_ints(&[]);
    }
    Concurrency::with_runtime_mut(|rt| {
        let out = rt.heap.alloc_empty_list();
        for start in 0..=(xs.len() - size) {
            let win = rt.heap.alloc_empty_list();
            for &v in &xs[start..start + size] {
                rt.heap
                    .list_push_int(win, v)
                    .expect("jit iter windows: push");
            }
            rt.heap
                .list_push_int(out, win)
                .expect("jit iter windows: outer push");
        }
        out
    })
}

extern "C" fn jet_jit_list_sum_i64(list: i64) -> i64 {
    let values = clone_list_ints(list);
    Concurrency::with_runtime_mut(|rt| {
        values
            .into_iter()
            .try_fold(0i64, i64::checked_add)
            .unwrap_or_else(|| {
                rt.set_trap("integer overflow");
                0
            })
    })
}

extern "C" fn jet_jit_list_product_i64(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .clone_int_list(list)
            .unwrap_or_default()
            .into_iter()
            .try_fold(1i64, i64::checked_mul)
            .unwrap_or_else(|| {
                rt.set_trap("integer overflow");
                0
            })
    })
}

extern "C" fn jet_jit_list_min_i64(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let value = rt.heap.clone_int_list(list).unwrap_or_default().into_iter().min();
        option_i64(rt, value)
    })
}

extern "C" fn jet_jit_list_max_i64(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let value = rt.heap.clone_int_list(list).unwrap_or_default().into_iter().max();
        option_i64(rt, value)
    })
}

extern "C" fn jet_jit_list_flatten(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let outer = rt.heap.clone_int_list(list).unwrap_or_default();
        let out = rt.heap.alloc_empty_list();
        for inner in outer {
            for value in rt.heap.clone_int_list(inner).unwrap_or_default() {
                let _ = rt.heap.list_push_int(out, value);
            }
        }
        out
    })
}

extern "C" fn jet_jit_list_intersperse(list: i64, separator: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let values = rt.heap.clone_int_list(list).unwrap_or_default();
        let out = rt.heap.alloc_empty_list();
        for (index, value) in values.into_iter().enumerate() {
            if index != 0 {
                let _ = rt.heap.list_push_int(out, separator);
            }
            let _ = rt.heap.list_push_int(out, value);
        }
        out
    })
}

extern "C" fn jet_jit_list_zip(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let left = rt.heap.clone_int_list(left).unwrap_or_default();
        let right = rt.heap.clone_int_list(right).unwrap_or_default();
        let out = rt.heap.alloc_empty_list();
        for (a, b) in left.into_iter().zip(right) {
            let pair = rt.heap.alloc_record(2);
            let _ = rt.heap.record_set_int(pair, 0, a);
            let _ = rt.heap.record_set_int(pair, 1, b);
            let _ = rt.heap.list_push_int(out, pair);
        }
        out
    })
}

extern "C" fn jet_jit_list_unzip(pairs: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let pairs = rt.heap.clone_int_list(pairs).unwrap_or_default();
        let left = rt.heap.alloc_empty_list();
        let right = rt.heap.alloc_empty_list();
        for pair in pairs {
            if let (Some(a), Some(b)) = (
                rt.heap.record_get_int(pair, 0),
                rt.heap.record_get_int(pair, 1),
            ) {
                let _ = rt.heap.list_push_int(left, a);
                let _ = rt.heap.list_push_int(right, b);
            }
        }
        let result = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(result, 0, left);
        let _ = rt.heap.record_set_int(result, 1, right);
        result
    })
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

fn deque_handle(rt: &mut crate::JitRuntime, dq: VecDeque<i64>) -> i64 {
    rt.deques.push(dq);
    rt.deques.len() as i64
}

extern "C" fn jet_jit_set_from_list(list: i64, string_kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let xs = rt.heap.clone_int_list(list).unwrap_or_default();
        set_handle(rt, xs.into_iter().collect(), string_kind != 0)
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

extern "C" fn jet_jit_sorted_set_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| sorted_set_handle(rt, BTreeSet::new(), false))
}

extern "C" fn jet_jit_sorted_set_from(list: i64, string_kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let set = rt
            .heap
            .clone_int_list(list)
            .unwrap_or_default()
            .into_iter()
            .collect();
        sorted_set_handle(rt, set, string_kind != 0)
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
        rt.byte_buffers.push(Vec::new());
        rt.byte_buffers.len() as i64
    })
}

extern "C" fn jet_jit_byte_buffer_write(handle: i64, value: i64, method: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let bytes = if method == 7 {
            rt.heap
                .clone_int_list(value)
                .unwrap_or_default()
                .into_iter()
                .map(|byte| byte as u8)
                .collect()
        } else {
            match method {
                0 => vec![value as u8],
                1 => (value as u16).to_le_bytes().to_vec(),
                2 => (value as u16).to_be_bytes().to_vec(),
                3 => (value as u32).to_le_bytes().to_vec(),
                4 => (value as u32).to_be_bytes().to_vec(),
                5 => (value as u64).to_le_bytes().to_vec(),
                6 => (value as u64).to_be_bytes().to_vec(),
                _ => Vec::new(),
            }
        };
        if let Some(buffer) = rt
            .byte_buffers
            .get_mut((handle as usize).wrapping_sub(1))
        {
            buffer.extend(bytes);
        }
    });
}

extern "C" fn jet_jit_byte_buffer_to_bytes(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let values = rt
            .byte_buffers
            .get((handle as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(i64::from)
            .collect::<Vec<_>>();
        copy_list(rt, values)
    })
}

/// Packed-enum JetShow table: variant mangled names + payload kind codes.
/// kind: 0 = unit, 1 = Int (>>8), 2 = nested packed enum (>>8), 3 = String handle (>>8).
#[derive(Clone)]
struct PackedEnumShow {
    variants: Vec<(String, u8, String)>, // (user_Variant, kind, nested_enum_name)
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

pub(crate) struct CollectionsHostFns {
    pub io_args: cranelift_module::FuncId,
    pub list_new: cranelift_module::FuncId,
    pub list_uninit: cranelift_module::FuncId,
    pub list_push: cranelift_module::FuncId,
    pub list_push_f64: cranelift_module::FuncId,
    pub list_push_range: cranelift_module::FuncId,
    pub list_get: cranelift_module::FuncId,
    pub list_get_f64: cranelift_module::FuncId,
    pub list_get_range_start: cranelift_module::FuncId,
    pub list_get_range_end: cranelift_module::FuncId,
    pub list_get_range_exclusive: cranelift_module::FuncId,
    pub list_get_opt: cranelift_module::FuncId,
    pub list_set: cranelift_module::FuncId,
    pub list_set_f64: cranelift_module::FuncId,
    pub list_len: cranelift_module::FuncId,
    pub list_contains_str: cranelift_module::FuncId,
    pub list_eq: cranelift_module::FuncId,
    pub list_indexes: cranelift_module::FuncId,
    pub list_sort: cranelift_module::FuncId,
    pub list_sort_str: cranelift_module::FuncId,
    pub list_clone: cranelift_module::FuncId,
    pub list_slice: cranelift_module::FuncId,
    pub list_range_end: cranelift_module::FuncId,
    pub split_write: cranelift_module::FuncId,
    pub get_disjoint_write: cranelift_module::FuncId,
    pub range_contains: cranelift_module::FuncId,
    pub range_show: cranelift_module::FuncId,
    pub range_equal: cranelift_module::FuncId,
    pub list_join_str: cranelift_module::FuncId,
    pub loop_stride_check: cranelift_module::FuncId,
    pub map_new: cranelift_module::FuncId,
    pub map_clone: cranelift_module::FuncId,
    pub map_merge: cranelift_module::FuncId,
    pub map_insert: cranelift_module::FuncId,
    pub map_increment: cranelift_module::FuncId,
    pub map_get: cranelift_module::FuncId,
    pub map_validate: cranelift_module::FuncId,
    pub map_get_opt: cranelift_module::FuncId,
    pub map_len: cranelift_module::FuncId,
    pub map_key_at: cranelift_module::FuncId,
    pub map_value_at: cranelift_module::FuncId,
    pub map_keys: cranelift_module::FuncId,
    pub map_values: cranelift_module::FuncId,
    pub iter_take: cranelift_module::FuncId,
    pub iter_skip: cranelift_module::FuncId,
    pub iter_step_by: cranelift_module::FuncId,
    pub iter_dedup: cranelift_module::FuncId,
    pub iter_chunks: cranelift_module::FuncId,
    pub iter_windows: cranelift_module::FuncId,
    pub list_sum_i64: cranelift_module::FuncId,
    pub list_product_i64: cranelift_module::FuncId,
    pub list_min_i64: cranelift_module::FuncId,
    pub list_max_i64: cranelift_module::FuncId,
    pub list_flatten: cranelift_module::FuncId,
    pub list_intersperse: cranelift_module::FuncId,
    pub list_zip: cranelift_module::FuncId,
    pub list_unzip: cranelift_module::FuncId,
    pub list_sort_by_i64_keys: cranelift_module::FuncId,
    pub list_sort_by_str_keys: cranelift_module::FuncId,
    pub print_list: cranelift_module::FuncId,
    pub print_opt: cranelift_module::FuncId,
    pub print_enum: cranelift_module::FuncId,
    pub list_show: cranelift_module::FuncId,
    pub list_remove: cranelift_module::FuncId,
    pub list_pop: cranelift_module::FuncId,
    pub list_insert: cranelift_module::FuncId,
    pub set_from_list: cranelift_module::FuncId,
    pub set_insert: cranelift_module::FuncId,
    pub set_remove: cranelift_module::FuncId,
    pub set_has: cranelift_module::FuncId,
    pub set_len: cranelift_module::FuncId,
    pub set_to_list: cranelift_module::FuncId,
    pub set_union: cranelift_module::FuncId,
    pub set_intersection: cranelift_module::FuncId,
    pub set_difference: cranelift_module::FuncId,
    pub set_symmetric_difference: cranelift_module::FuncId,
    pub set_is_subset: cranelift_module::FuncId,
    pub set_is_superset: cranelift_module::FuncId,
    pub set_is_disjoint: cranelift_module::FuncId,
    pub deque_new: cranelift_module::FuncId,
    pub deque_push_front: cranelift_module::FuncId,
    pub deque_push_back: cranelift_module::FuncId,
    pub deque_pop_front: cranelift_module::FuncId,
    pub deque_pop_back: cranelift_module::FuncId,
    pub deque_peek_front: cranelift_module::FuncId,
    pub deque_peek_back: cranelift_module::FuncId,
    pub deque_len: cranelift_module::FuncId,
    pub bag_new: cranelift_module::FuncId,
    pub bag_add: cranelift_module::FuncId,
    pub bag_remove: cranelift_module::FuncId,
    pub bag_has: cranelift_module::FuncId,
    pub bag_count: cranelift_module::FuncId,
    pub bag_len: cranelift_module::FuncId,
    pub sorted_set_new: cranelift_module::FuncId,
    pub sorted_set_from: cranelift_module::FuncId,
    pub sorted_set_insert: cranelift_module::FuncId,
    pub sorted_set_remove: cranelift_module::FuncId,
    pub sorted_set_to_list: cranelift_module::FuncId,
    pub sorted_set_first: cranelift_module::FuncId,
    pub sorted_set_last: cranelift_module::FuncId,
    pub sorted_set_union: cranelift_module::FuncId,
    pub sorted_set_intersection: cranelift_module::FuncId,
    pub sorted_set_difference: cranelift_module::FuncId,
    pub sorted_set_symmetric_difference: cranelift_module::FuncId,
    pub sorted_set_is_subset: cranelift_module::FuncId,
    pub sorted_set_is_superset: cranelift_module::FuncId,
    pub sorted_set_is_disjoint: cranelift_module::FuncId,
    pub priority_queue_new: cranelift_module::FuncId,
    pub priority_queue_from: cranelift_module::FuncId,
    pub priority_queue_push: cranelift_module::FuncId,
    pub priority_queue_peek: cranelift_module::FuncId,
    pub priority_queue_pop: cranelift_module::FuncId,
    pub priority_queue_to_sorted_list: cranelift_module::FuncId,
    pub lru_new: cranelift_module::FuncId,
    pub lru_put: cranelift_module::FuncId,
    pub lru_get: cranelift_module::FuncId,
    pub lru_has: cranelift_module::FuncId,
    pub lru_keys: cranelift_module::FuncId,
    pub bit_set_new: cranelift_module::FuncId,
    pub bit_set_add: cranelift_module::FuncId,
    pub bit_set_remove: cranelift_module::FuncId,
    pub bit_set_to_list: cranelift_module::FuncId,
    pub bit_set_len: cranelift_module::FuncId,
    pub bit_set_count: cranelift_module::FuncId,
    pub byte_buffer_new: cranelift_module::FuncId,
    pub byte_buffer_write: cranelift_module::FuncId,
    pub byte_buffer_to_bytes: cranelift_module::FuncId,
}

pub(crate) fn register_collections_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_io_args", jet_jit_io_args as *const u8);
    builder.symbol("jet_jit_list_new", jet_jit_list_new as *const u8);
    builder.symbol("jet_jit_list_uninit", jet_jit_list_uninit as *const u8);
    builder.symbol("jet_jit_list_push", jet_jit_list_push as *const u8);
    builder.symbol("jet_jit_list_push_f64", jet_jit_list_push_f64 as *const u8);
    builder.symbol(
        "jet_jit_list_push_range",
        jet_jit_list_push_range as *const u8,
    );
    builder.symbol("jet_jit_list_get", jet_jit_list_get as *const u8);
    builder.symbol("jet_jit_list_get_f64", jet_jit_list_get_f64 as *const u8);
    builder.symbol(
        "jet_jit_list_get_range_start",
        jet_jit_list_get_range_start as *const u8,
    );
    builder.symbol(
        "jet_jit_list_get_range_end",
        jet_jit_list_get_range_end as *const u8,
    );
    builder.symbol(
        "jet_jit_list_get_range_exclusive",
        jet_jit_list_get_range_exclusive as *const u8,
    );
    builder.symbol("jet_jit_list_get_opt", jet_jit_list_get_opt as *const u8);
    builder.symbol("jet_jit_list_set", jet_jit_list_set as *const u8);
    builder.symbol("jet_jit_list_set_f64", jet_jit_list_set_f64 as *const u8);
    builder.symbol("jet_jit_list_len", jet_jit_list_len as *const u8);
    builder.symbol("jet_jit_list_contains_str", jet_jit_list_contains_str as *const u8);
    builder.symbol("jet_jit_list_eq", jet_jit_list_eq as *const u8);
    builder.symbol("jet_jit_list_indexes", jet_jit_list_indexes as *const u8);
    builder.symbol("jet_jit_list_sort", jet_jit_list_sort as *const u8);
    builder.symbol("jet_jit_list_sort_str", jet_jit_list_sort_str as *const u8);
    builder.symbol("jet_jit_list_clone", jet_jit_list_clone as *const u8);
    builder.symbol("jet_jit_list_slice", jet_jit_list_slice as *const u8);
    builder.symbol("jet_jit_list_range_end", jet_jit_list_range_end as *const u8);
    builder.symbol("jet_jit_split_write", jet_jit_split_write as *const u8);
    builder.symbol(
        "jet_jit_get_disjoint_write",
        jet_jit_get_disjoint_write as *const u8,
    );
    builder.symbol("jet_jit_range_contains", jet_jit_range_contains as *const u8);
    builder.symbol("jet_jit_range_show", jet_jit_range_show as *const u8);
    builder.symbol("jet_jit_range_equal", jet_jit_range_equal as *const u8);
    builder.symbol("jet_jit_list_join_str", jet_jit_list_join_str as *const u8);
    builder.symbol("jet_jit_loop_stride_check", jet_jit_loop_stride_check as *const u8);
    builder.symbol("jet_jit_map_new", jet_jit_map_new as *const u8);
    builder.symbol("jet_jit_map_clone", jet_jit_map_clone as *const u8);
    builder.symbol("jet_jit_map_merge", jet_jit_map_merge as *const u8);
    builder.symbol("jet_jit_map_insert", jet_jit_map_insert as *const u8);
    builder.symbol("jet_jit_map_increment", jet_jit_map_increment as *const u8);
    builder.symbol("jet_jit_map_get", jet_jit_map_get as *const u8);
    builder.symbol("jet_jit_map_validate", jet_jit_map_validate as *const u8);
    builder.symbol("jet_jit_map_get_opt", jet_jit_map_get_opt as *const u8);
    builder.symbol("jet_jit_map_len", jet_jit_map_len as *const u8);
    builder.symbol("jet_jit_map_key_at", jet_jit_map_key_at as *const u8);
    builder.symbol("jet_jit_map_value_at", jet_jit_map_value_at as *const u8);
    builder.symbol("jet_jit_map_keys", jet_jit_map_keys as *const u8);
    builder.symbol("jet_jit_map_values", jet_jit_map_values as *const u8);
    builder.symbol("jet_jit_iter_take", jet_jit_iter_take as *const u8);
    builder.symbol("jet_jit_iter_skip", jet_jit_iter_skip as *const u8);
    builder.symbol("jet_jit_iter_step_by", jet_jit_iter_step_by as *const u8);
    builder.symbol("jet_jit_iter_dedup", jet_jit_iter_dedup as *const u8);
    builder.symbol("jet_jit_iter_chunks", jet_jit_iter_chunks as *const u8);
    builder.symbol("jet_jit_iter_windows", jet_jit_iter_windows as *const u8);
    builder.symbol("jet_jit_list_sum_i64", jet_jit_list_sum_i64 as *const u8);
    builder.symbol("jet_jit_list_product_i64", jet_jit_list_product_i64 as *const u8);
    builder.symbol("jet_jit_list_min_i64", jet_jit_list_min_i64 as *const u8);
    builder.symbol("jet_jit_list_max_i64", jet_jit_list_max_i64 as *const u8);
    builder.symbol("jet_jit_list_flatten", jet_jit_list_flatten as *const u8);
    builder.symbol("jet_jit_list_intersperse", jet_jit_list_intersperse as *const u8);
    builder.symbol("jet_jit_list_zip", jet_jit_list_zip as *const u8);
    builder.symbol("jet_jit_list_unzip", jet_jit_list_unzip as *const u8);
    builder.symbol(
        "jet_jit_list_sort_by_i64_keys",
        jet_jit_list_sort_by_i64_keys as *const u8,
    );
    builder.symbol(
        "jet_jit_list_sort_by_str_keys",
        jet_jit_list_sort_by_str_keys as *const u8,
    );
    builder.symbol("jet_jit_print_list", jet_jit_print_list as *const u8);
    builder.symbol("jet_jit_print_opt", jet_jit_print_opt as *const u8);
    builder.symbol("jet_jit_print_enum", jet_jit_print_enum as *const u8);
    builder.symbol("jet_jit_list_show", jet_jit_list_show as *const u8);
    builder.symbol("jet_jit_list_remove", jet_jit_list_remove as *const u8);
    builder.symbol("jet_jit_list_pop", jet_jit_list_pop as *const u8);
    builder.symbol("jet_jit_list_insert", jet_jit_list_insert as *const u8);
    builder.symbol("jet_jit_set_from_list", jet_jit_set_from_list as *const u8);
    builder.symbol("jet_jit_set_insert", jet_jit_set_insert as *const u8);
    builder.symbol("jet_jit_set_remove", jet_jit_set_remove as *const u8);
    builder.symbol("jet_jit_set_has", jet_jit_set_has as *const u8);
    builder.symbol("jet_jit_set_len", jet_jit_set_len as *const u8);
    builder.symbol("jet_jit_set_to_list", jet_jit_set_to_list as *const u8);
    builder.symbol("jet_jit_set_union", jet_jit_set_union as *const u8);
    builder.symbol(
        "jet_jit_set_intersection",
        jet_jit_set_intersection as *const u8,
    );
    builder.symbol("jet_jit_set_difference", jet_jit_set_difference as *const u8);
    builder.symbol(
        "jet_jit_set_symmetric_difference",
        jet_jit_set_symmetric_difference as *const u8,
    );
    builder.symbol("jet_jit_set_is_subset", jet_jit_set_is_subset as *const u8);
    builder.symbol("jet_jit_set_is_superset", jet_jit_set_is_superset as *const u8);
    builder.symbol("jet_jit_set_is_disjoint", jet_jit_set_is_disjoint as *const u8);
    builder.symbol("jet_jit_deque_new", jet_jit_deque_new as *const u8);
    builder.symbol("jet_jit_deque_push_front", jet_jit_deque_push_front as *const u8);
    builder.symbol("jet_jit_deque_push_back", jet_jit_deque_push_back as *const u8);
    builder.symbol("jet_jit_deque_pop_front", jet_jit_deque_pop_front as *const u8);
    builder.symbol("jet_jit_deque_pop_back", jet_jit_deque_pop_back as *const u8);
    builder.symbol("jet_jit_deque_peek_front", jet_jit_deque_peek_front as *const u8);
    builder.symbol("jet_jit_deque_peek_back", jet_jit_deque_peek_back as *const u8);
    builder.symbol("jet_jit_deque_len", jet_jit_deque_len as *const u8);
    builder.symbol("jet_jit_bag_new", jet_jit_bag_new as *const u8);
    builder.symbol("jet_jit_bag_add", jet_jit_bag_add as *const u8);
    builder.symbol("jet_jit_bag_remove", jet_jit_bag_remove as *const u8);
    builder.symbol("jet_jit_bag_has", jet_jit_bag_has as *const u8);
    builder.symbol("jet_jit_bag_count", jet_jit_bag_count as *const u8);
    builder.symbol("jet_jit_bag_len", jet_jit_bag_len as *const u8);
    builder.symbol("jet_jit_sorted_set_new", jet_jit_sorted_set_new as *const u8);
    builder.symbol("jet_jit_sorted_set_from", jet_jit_sorted_set_from as *const u8);
    builder.symbol("jet_jit_sorted_set_insert", jet_jit_sorted_set_insert as *const u8);
    builder.symbol("jet_jit_sorted_set_remove", jet_jit_sorted_set_remove as *const u8);
    builder.symbol("jet_jit_sorted_set_to_list", jet_jit_sorted_set_to_list as *const u8);
    builder.symbol("jet_jit_sorted_set_first", jet_jit_sorted_set_first as *const u8);
    builder.symbol("jet_jit_sorted_set_last", jet_jit_sorted_set_last as *const u8);
    builder.symbol(
        "jet_jit_sorted_set_union",
        jet_jit_sorted_set_union as *const u8,
    );
    builder.symbol(
        "jet_jit_sorted_set_intersection",
        jet_jit_sorted_set_intersection as *const u8,
    );
    builder.symbol(
        "jet_jit_sorted_set_difference",
        jet_jit_sorted_set_difference as *const u8,
    );
    builder.symbol(
        "jet_jit_sorted_set_symmetric_difference",
        jet_jit_sorted_set_symmetric_difference as *const u8,
    );
    builder.symbol(
        "jet_jit_sorted_set_is_subset",
        jet_jit_sorted_set_is_subset as *const u8,
    );
    builder.symbol(
        "jet_jit_sorted_set_is_superset",
        jet_jit_sorted_set_is_superset as *const u8,
    );
    builder.symbol(
        "jet_jit_sorted_set_is_disjoint",
        jet_jit_sorted_set_is_disjoint as *const u8,
    );
    builder.symbol("jet_jit_priority_queue_new", jet_jit_priority_queue_new as *const u8);
    builder.symbol("jet_jit_priority_queue_from", jet_jit_priority_queue_from as *const u8);
    builder.symbol("jet_jit_priority_queue_push", jet_jit_priority_queue_push as *const u8);
    builder.symbol("jet_jit_priority_queue_peek", jet_jit_priority_queue_peek as *const u8);
    builder.symbol("jet_jit_priority_queue_pop", jet_jit_priority_queue_pop as *const u8);
    builder.symbol(
        "jet_jit_priority_queue_to_sorted_list",
        jet_jit_priority_queue_to_sorted_list as *const u8,
    );
    builder.symbol("jet_jit_lru_new", jet_jit_lru_new as *const u8);
    builder.symbol("jet_jit_lru_put", jet_jit_lru_put as *const u8);
    builder.symbol("jet_jit_lru_get", jet_jit_lru_get as *const u8);
    builder.symbol("jet_jit_lru_has", jet_jit_lru_has as *const u8);
    builder.symbol("jet_jit_lru_keys", jet_jit_lru_keys as *const u8);
    builder.symbol("jet_jit_bit_set_new", jet_jit_bit_set_new as *const u8);
    builder.symbol("jet_jit_bit_set_add", jet_jit_bit_set_add as *const u8);
    builder.symbol("jet_jit_bit_set_remove", jet_jit_bit_set_remove as *const u8);
    builder.symbol("jet_jit_bit_set_to_list", jet_jit_bit_set_to_list as *const u8);
    builder.symbol("jet_jit_bit_set_len", jet_jit_bit_set_len as *const u8);
    builder.symbol("jet_jit_bit_set_count", jet_jit_bit_set_count as *const u8);
    builder.symbol("jet_jit_byte_buffer_new", jet_jit_byte_buffer_new as *const u8);
    builder.symbol("jet_jit_byte_buffer_write", jet_jit_byte_buffer_write as *const u8);
    builder.symbol(
        "jet_jit_byte_buffer_to_bytes",
        jet_jit_byte_buffer_to_bytes as *const u8,
    );
}

pub(crate) fn declare_collections_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<CollectionsHostFns, String> {
    use cranelift_codegen::ir::{types, AbiParam, Signature};
    use cranelift_module::{Linkage, Module};

    let cc = module.target_config().default_call_conv;
    let mut sig_new = Signature::new(cc);
    sig_new.returns.push(AbiParam::new(types::I64));
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

    let mut import = |name: &str, sig: &Signature| -> Result<cranelift_module::FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };

    Ok(CollectionsHostFns {
        io_args: import("jet_jit_io_args", &sig_new)?,
        list_new: import("jet_jit_list_new", &sig_new)?,
        list_uninit: import("jet_jit_list_uninit", &sig_uninit)?,
        list_push: import("jet_jit_list_push", &sig_push)?,
        list_push_f64: import("jet_jit_list_push_f64", &sig_push_f64)?,
        list_push_range: import("jet_jit_list_push_range", &sig_push_range)?,
        list_get: import("jet_jit_list_get", &sig_get)?,
        list_get_f64: import("jet_jit_list_get_f64", &sig_get_f64)?,
        list_get_range_start: import(
            "jet_jit_list_get_range_start",
            &sig_get_range_scalar,
        )?,
        list_get_range_end: import("jet_jit_list_get_range_end", &sig_get_range_scalar)?,
        list_get_range_exclusive: import(
            "jet_jit_list_get_range_exclusive",
            &sig_get_range_exclusive,
        )?,
        list_get_opt: import("jet_jit_list_get_opt", &sig_get_opt)?,
        list_set: import("jet_jit_list_set", &sig_set)?,
        list_set_f64: import("jet_jit_list_set_f64", &sig_set_f64)?,
        list_len: import("jet_jit_list_len", &sig_len)?,
        list_contains_str: import("jet_jit_list_contains_str", &sig_list_eq)?,
        list_eq: import("jet_jit_list_eq", &sig_list_eq)?,
        list_indexes: import("jet_jit_list_indexes", &sig_len)?,
        list_sort: import("jet_jit_list_sort", &sig_sort)?,
        list_sort_str: import("jet_jit_list_sort_str", &sig_sort)?,
        list_clone: import("jet_jit_list_clone", &sig_len)?,
        list_slice: import("jet_jit_list_slice", &sig_slice)?,
        list_range_end: import("jet_jit_list_range_end", &sig_range_end)?,
        split_write: import("jet_jit_split_write", &sig_disjoint)?,
        get_disjoint_write: import("jet_jit_get_disjoint_write", &sig_disjoint)?,
        range_contains: import("jet_jit_range_contains", &sig_range_contains)?,
        range_show: import("jet_jit_range_show", &sig_range_show)?,
        range_equal: import("jet_jit_range_equal", &sig_range_equal)?,
        list_join_str: import("jet_jit_list_join_str", &sig_join)?,
        loop_stride_check: import("jet_jit_loop_stride_check", &sig_len)?,
        map_new: import("jet_jit_map_new", &sig_new)?,
        map_clone: import("jet_jit_map_clone", &sig_len)?,
        map_merge: import("jet_jit_map_merge", &sig_get_opt)?,
        map_insert: import("jet_jit_map_insert", &sig_map_insert)?,
        map_increment: import("jet_jit_map_increment", &sig_push)?,
        map_get: import("jet_jit_map_get", &sig_map_get)?,
        map_validate: import("jet_jit_map_validate", &sig_len)?,
        map_get_opt: import("jet_jit_map_get_opt", &sig_map_get_opt)?,
        map_len: import("jet_jit_map_len", &sig_len)?,
        map_key_at: import("jet_jit_map_key_at", &sig_map_at)?,
        map_value_at: import("jet_jit_map_value_at", &sig_map_at)?,
        map_keys: import("jet_jit_map_keys", &sig_len)?,
        map_values: import("jet_jit_map_values", &sig_len)?,
        iter_take: import("jet_jit_iter_take", &sig_get_opt)?,
        iter_skip: import("jet_jit_iter_skip", &sig_get_opt)?,
        iter_step_by: import("jet_jit_iter_step_by", &sig_get_opt)?,
        iter_dedup: import("jet_jit_iter_dedup", &sig_get_opt)?,
        iter_chunks: import("jet_jit_iter_chunks", &sig_get_opt)?,
        iter_windows: import("jet_jit_iter_windows", &sig_get_opt)?,
        list_sum_i64: import("jet_jit_list_sum_i64", &sig_len)?,
        list_product_i64: import("jet_jit_list_product_i64", &sig_len)?,
        list_min_i64: import("jet_jit_list_min_i64", &sig_len)?,
        list_max_i64: import("jet_jit_list_max_i64", &sig_len)?,
        list_flatten: import("jet_jit_list_flatten", &sig_len)?,
        list_intersperse: import("jet_jit_list_intersperse", &sig_get_opt)?,
        list_zip: import("jet_jit_list_zip", &sig_get_opt)?,
        list_unzip: import("jet_jit_list_unzip", &sig_len)?,
        list_sort_by_i64_keys: import("jet_jit_list_sort_by_i64_keys", &sig_sort_by_keys)?,
        list_sort_by_str_keys: import("jet_jit_list_sort_by_str_keys", &sig_sort_by_keys)?,
        print_list: import("jet_jit_print_list", &sig_print_list)?,
        print_opt: import("jet_jit_print_opt", &sig_print_list)?,
        print_enum: import("jet_jit_print_enum", &sig_print_enum)?,
        list_show: import("jet_jit_list_show", &sig_get_opt)?,
        list_remove: import("jet_jit_list_remove", &sig_get_opt)?,
        list_pop: import("jet_jit_list_pop", &sig_len)?,
        list_insert: import("jet_jit_list_insert", &sig_map_insert)?,
        set_from_list: import("jet_jit_set_from_list", &sig_set_from)?,
        set_insert: import("jet_jit_set_insert", &sig_list_eq)?,
        set_remove: import("jet_jit_set_remove", &sig_push)?,
        set_has: import("jet_jit_set_has", &sig_list_eq)?,
        set_len: import("jet_jit_set_len", &sig_len)?,
        set_to_list: import("jet_jit_set_to_list", &sig_len)?,
        set_union: import("jet_jit_set_union", &sig_get_opt)?,
        set_intersection: import("jet_jit_set_intersection", &sig_get_opt)?,
        set_difference: import("jet_jit_set_difference", &sig_get_opt)?,
        set_symmetric_difference: import("jet_jit_set_symmetric_difference", &sig_get_opt)?,
        set_is_subset: import("jet_jit_set_is_subset", &sig_list_eq)?,
        set_is_superset: import("jet_jit_set_is_superset", &sig_list_eq)?,
        set_is_disjoint: import("jet_jit_set_is_disjoint", &sig_list_eq)?,
        deque_new: import("jet_jit_deque_new", &sig_new)?,
        deque_push_front: import("jet_jit_deque_push_front", &sig_push)?,
        deque_push_back: import("jet_jit_deque_push_back", &sig_push)?,
        deque_pop_front: import("jet_jit_deque_pop_front", &sig_len)?,
        deque_pop_back: import("jet_jit_deque_pop_back", &sig_len)?,
        deque_peek_front: import("jet_jit_deque_peek_front", &sig_len)?,
        deque_peek_back: import("jet_jit_deque_peek_back", &sig_len)?,
        deque_len: import("jet_jit_deque_len", &sig_len)?,
        bag_new: import("jet_jit_bag_new", &sig_new)?,
        bag_add: import("jet_jit_bag_add", &sig_list_eq)?,
        bag_remove: import("jet_jit_bag_remove", &sig_push)?,
        bag_has: import("jet_jit_bag_has", &sig_list_eq)?,
        bag_count: import("jet_jit_bag_count", &sig_get_opt)?,
        bag_len: import("jet_jit_bag_len", &sig_len)?,
        sorted_set_new: import("jet_jit_sorted_set_new", &sig_new)?,
        sorted_set_from: import("jet_jit_sorted_set_from", &sig_set_from)?,
        sorted_set_insert: import("jet_jit_sorted_set_insert", &sig_list_eq)?,
        sorted_set_remove: import("jet_jit_sorted_set_remove", &sig_push)?,
        sorted_set_to_list: import("jet_jit_sorted_set_to_list", &sig_len)?,
        sorted_set_first: import("jet_jit_sorted_set_first", &sig_len)?,
        sorted_set_last: import("jet_jit_sorted_set_last", &sig_len)?,
        sorted_set_union: import("jet_jit_sorted_set_union", &sig_get_opt)?,
        sorted_set_intersection: import("jet_jit_sorted_set_intersection", &sig_get_opt)?,
        sorted_set_difference: import("jet_jit_sorted_set_difference", &sig_get_opt)?,
        sorted_set_symmetric_difference: import(
            "jet_jit_sorted_set_symmetric_difference",
            &sig_get_opt,
        )?,
        sorted_set_is_subset: import("jet_jit_sorted_set_is_subset", &sig_list_eq)?,
        sorted_set_is_superset: import("jet_jit_sorted_set_is_superset", &sig_list_eq)?,
        sorted_set_is_disjoint: import("jet_jit_sorted_set_is_disjoint", &sig_list_eq)?,
        priority_queue_new: import("jet_jit_priority_queue_new", &sig_new)?,
        priority_queue_from: import("jet_jit_priority_queue_from", &sig_len)?,
        priority_queue_push: import("jet_jit_priority_queue_push", &sig_push)?,
        priority_queue_peek: import("jet_jit_priority_queue_peek", &sig_len)?,
        priority_queue_pop: import("jet_jit_priority_queue_pop", &sig_len)?,
        priority_queue_to_sorted_list: import(
            "jet_jit_priority_queue_to_sorted_list",
            &sig_len,
        )?,
        lru_new: import("jet_jit_lru_new", &sig_len)?,
        lru_put: import("jet_jit_lru_put", &sig_three_ret)?,
        lru_get: import("jet_jit_lru_get", &sig_get_opt)?,
        lru_has: import("jet_jit_lru_has", &sig_list_eq)?,
        lru_keys: import("jet_jit_lru_keys", &sig_len)?,
        bit_set_new: import("jet_jit_bit_set_new", &sig_new)?,
        bit_set_add: import("jet_jit_bit_set_add", &sig_list_eq)?,
        bit_set_remove: import("jet_jit_bit_set_remove", &sig_push)?,
        bit_set_to_list: import("jet_jit_bit_set_to_list", &sig_len)?,
        bit_set_len: import("jet_jit_bit_set_len", &sig_len)?,
        bit_set_count: import("jet_jit_bit_set_count", &sig_len)?,
        byte_buffer_new: import("jet_jit_byte_buffer_new", &sig_new)?,
        byte_buffer_write: import("jet_jit_byte_buffer_write", &sig_map_insert)?,
        byte_buffer_to_bytes: import("jet_jit_byte_buffer_to_bytes", &sig_len)?,
    })
}
