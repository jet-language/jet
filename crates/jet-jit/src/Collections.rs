//! M5: list/map host shims for the Cranelift JIT (`JetArena` handles).

use super::Concurrency;
use std::collections::{HashSet, VecDeque};

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

extern "C" fn jet_jit_list_len(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.list_len(list).expect("jit list len: bad handle"))
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

extern "C" fn jet_jit_list_join_str(list: i64, sep_id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let xs = rt
            .heap
            .clone_int_list(list)
            .expect("jit list join: bad handle");
        let sep = rt.heap.clone_string(sep_id).unwrap_or_default();
        let joined = xs
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(&sep);
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

extern "C" fn jet_jit_map_insert(map: i64, key: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .map_insert(map, key, value)
            .expect("jit map insert: bad handle");
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

/// `0` = absent; otherwise `value + 1` (Option Int encoding).
extern "C" fn jet_jit_map_get_opt(map: i64, key: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if rt.heap.map_len(map).is_none() {
            jet_foundation::ice!(None, "jit map get_opt: bad handle");
        }
        rt.heap.map_get(map, key).map(|v| v + 1).unwrap_or(0)
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
    clone_list_ints(list).into_iter().sum()
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

/// Print `[T]` / materialized `Iter<T>` with the same `jet_show` shape AOT uses.
/// `string_elems != 0` → elements are string handles; else raw i64.
extern "C" fn jet_jit_print_list(list: i64, string_elems: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let text = list_show_text(rt, list, string_elems);
        rt.stdout.push_str(&text);
        rt.stdout.push('\n');
    });
}

fn list_show_text(rt: &crate::JitRuntime, list: i64, string_elems: i64) -> String {
    let xs = rt
        .heap
        .clone_int_list(list)
        .unwrap_or_default();
    let mut parts = Vec::with_capacity(xs.len());
    if string_elems != 0 {
        for id in xs {
            parts.push(rt.heap.clone_string(id).unwrap_or_default());
        }
    } else {
        for v in xs {
            parts.push(v.to_string());
        }
    }
    format!("[{}]", parts.join(", "))
}

/// JetShow `[T]` as a string handle for `{list}` interpolation.
extern "C" fn jet_jit_list_show(list: i64, string_elems: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = list_show_text(rt, list, string_elems);
        rt.heap.alloc_string(text)
    })
}

/// Print `T?` using JIT packed Option encoding (`0` = None, else `value + 1`).
/// `kind`: 0 = i64 payload, 1 = string handle, 2 = f64 bits (bitcast).
extern "C" fn jet_jit_print_opt(packed: i64, kind: i64) {
    Concurrency::with_runtime_mut(|rt| {
        if packed == 0 {
            rt.stdout.push_str("null\n");
            return;
        }
        let payload = packed - 1;
        match kind {
            1 => {
                let text = rt.heap.clone_string(payload).unwrap_or_default();
                rt.stdout.push_str(&text);
            }
            2 => {
                rt.stdout
                    .push_str(&jet_rt::display_f64(f64::from_bits(payload as u64)));
            }
            _ => {
                rt.stdout.push_str(&payload.to_string());
            }
        }
        rt.stdout.push('\n');
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

fn set_handle(rt: &mut crate::JitRuntime, set: HashSet<i64>) -> i64 {
    rt.sets.push(set);
    rt.sets.len() as i64
}

fn deque_handle(rt: &mut crate::JitRuntime, dq: VecDeque<i64>) -> i64 {
    rt.deques.push(dq);
    rt.deques.len() as i64
}

extern "C" fn jet_jit_set_from_list(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let xs = rt.heap.clone_int_list(list).unwrap_or_default();
        set_handle(rt, xs.into_iter().collect())
    })
}

extern "C" fn jet_jit_set_insert(set: i64, v: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(s) = rt.sets.get_mut((set as usize).wrapping_sub(1)) else {
            return 0;
        };
        if s.insert(v) {
            1
        } else {
            0
        }
    })
}

extern "C" fn jet_jit_set_remove(set: i64, v: i64) {
    Concurrency::with_runtime_mut(|rt| {
        if let Some(s) = rt.sets.get_mut((set as usize).wrapping_sub(1)) {
            s.remove(&v);
        }
    });
}

extern "C" fn jet_jit_set_has(set: i64, v: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        match rt.sets.get((set as usize).wrapping_sub(1)) {
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
        let xs: Vec<i64> = rt
            .sets
            .get((set as usize).wrapping_sub(1))
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        rt.heap.alloc_int_list(xs)
    })
}

extern "C" fn jet_jit_set_union(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let mut out = rt
            .sets
            .get((a as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_default();
        if let Some(other) = rt.sets.get((b as usize).wrapping_sub(1)) {
            out.extend(other.iter().copied());
        }
        set_handle(rt, out)
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
    pub list_push: cranelift_module::FuncId,
    pub list_push_f64: cranelift_module::FuncId,
    pub list_get: cranelift_module::FuncId,
    pub list_get_f64: cranelift_module::FuncId,
    pub list_get_opt: cranelift_module::FuncId,
    pub list_set: cranelift_module::FuncId,
    pub list_set_f64: cranelift_module::FuncId,
    pub list_len: cranelift_module::FuncId,
    pub list_eq: cranelift_module::FuncId,
    pub list_indexes: cranelift_module::FuncId,
    pub list_sort: cranelift_module::FuncId,
    pub list_clone: cranelift_module::FuncId,
    pub list_slice: cranelift_module::FuncId,
    pub list_join_str: cranelift_module::FuncId,
    pub loop_stride_check: cranelift_module::FuncId,
    pub map_new: cranelift_module::FuncId,
    pub map_clone: cranelift_module::FuncId,
    pub map_insert: cranelift_module::FuncId,
    pub map_get: cranelift_module::FuncId,
    pub map_get_opt: cranelift_module::FuncId,
    pub map_len: cranelift_module::FuncId,
    pub map_key_at: cranelift_module::FuncId,
    pub map_value_at: cranelift_module::FuncId,
    pub iter_take: cranelift_module::FuncId,
    pub iter_skip: cranelift_module::FuncId,
    pub iter_step_by: cranelift_module::FuncId,
    pub iter_dedup: cranelift_module::FuncId,
    pub iter_chunks: cranelift_module::FuncId,
    pub iter_windows: cranelift_module::FuncId,
    pub list_sum_i64: cranelift_module::FuncId,
    pub list_sort_by_i64_keys: cranelift_module::FuncId,
    pub print_list: cranelift_module::FuncId,
    pub print_opt: cranelift_module::FuncId,
    pub print_enum: cranelift_module::FuncId,
    pub list_show: cranelift_module::FuncId,
    pub list_remove: cranelift_module::FuncId,
    pub set_from_list: cranelift_module::FuncId,
    pub set_insert: cranelift_module::FuncId,
    pub set_remove: cranelift_module::FuncId,
    pub set_has: cranelift_module::FuncId,
    pub set_len: cranelift_module::FuncId,
    pub set_to_list: cranelift_module::FuncId,
    pub set_union: cranelift_module::FuncId,
    pub deque_new: cranelift_module::FuncId,
    pub deque_push_front: cranelift_module::FuncId,
    pub deque_push_back: cranelift_module::FuncId,
    pub deque_pop_front: cranelift_module::FuncId,
    pub deque_pop_back: cranelift_module::FuncId,
    pub deque_peek_front: cranelift_module::FuncId,
    pub deque_peek_back: cranelift_module::FuncId,
    pub deque_len: cranelift_module::FuncId,
}

pub(crate) fn register_collections_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_io_args", jet_jit_io_args as *const u8);
    builder.symbol("jet_jit_list_new", jet_jit_list_new as *const u8);
    builder.symbol("jet_jit_list_push", jet_jit_list_push as *const u8);
    builder.symbol("jet_jit_list_push_f64", jet_jit_list_push_f64 as *const u8);
    builder.symbol("jet_jit_list_get", jet_jit_list_get as *const u8);
    builder.symbol("jet_jit_list_get_f64", jet_jit_list_get_f64 as *const u8);
    builder.symbol("jet_jit_list_get_opt", jet_jit_list_get_opt as *const u8);
    builder.symbol("jet_jit_list_set", jet_jit_list_set as *const u8);
    builder.symbol("jet_jit_list_set_f64", jet_jit_list_set_f64 as *const u8);
    builder.symbol("jet_jit_list_len", jet_jit_list_len as *const u8);
    builder.symbol("jet_jit_list_eq", jet_jit_list_eq as *const u8);
    builder.symbol("jet_jit_list_indexes", jet_jit_list_indexes as *const u8);
    builder.symbol("jet_jit_list_sort", jet_jit_list_sort as *const u8);
    builder.symbol("jet_jit_list_clone", jet_jit_list_clone as *const u8);
    builder.symbol("jet_jit_list_slice", jet_jit_list_slice as *const u8);
    builder.symbol("jet_jit_list_join_str", jet_jit_list_join_str as *const u8);
    builder.symbol("jet_jit_loop_stride_check", jet_jit_loop_stride_check as *const u8);
    builder.symbol("jet_jit_map_new", jet_jit_map_new as *const u8);
    builder.symbol("jet_jit_map_clone", jet_jit_map_clone as *const u8);
    builder.symbol("jet_jit_map_insert", jet_jit_map_insert as *const u8);
    builder.symbol("jet_jit_map_get", jet_jit_map_get as *const u8);
    builder.symbol("jet_jit_map_get_opt", jet_jit_map_get_opt as *const u8);
    builder.symbol("jet_jit_map_len", jet_jit_map_len as *const u8);
    builder.symbol("jet_jit_map_key_at", jet_jit_map_key_at as *const u8);
    builder.symbol("jet_jit_map_value_at", jet_jit_map_value_at as *const u8);
    builder.symbol("jet_jit_iter_take", jet_jit_iter_take as *const u8);
    builder.symbol("jet_jit_iter_skip", jet_jit_iter_skip as *const u8);
    builder.symbol("jet_jit_iter_step_by", jet_jit_iter_step_by as *const u8);
    builder.symbol("jet_jit_iter_dedup", jet_jit_iter_dedup as *const u8);
    builder.symbol("jet_jit_iter_chunks", jet_jit_iter_chunks as *const u8);
    builder.symbol("jet_jit_iter_windows", jet_jit_iter_windows as *const u8);
    builder.symbol("jet_jit_list_sum_i64", jet_jit_list_sum_i64 as *const u8);
    builder.symbol(
        "jet_jit_list_sort_by_i64_keys",
        jet_jit_list_sort_by_i64_keys as *const u8,
    );
    builder.symbol("jet_jit_print_list", jet_jit_print_list as *const u8);
    builder.symbol("jet_jit_print_opt", jet_jit_print_opt as *const u8);
    builder.symbol("jet_jit_print_enum", jet_jit_print_enum as *const u8);
    builder.symbol("jet_jit_list_show", jet_jit_list_show as *const u8);
    builder.symbol("jet_jit_list_remove", jet_jit_list_remove as *const u8);
    builder.symbol("jet_jit_set_from_list", jet_jit_set_from_list as *const u8);
    builder.symbol("jet_jit_set_insert", jet_jit_set_insert as *const u8);
    builder.symbol("jet_jit_set_remove", jet_jit_set_remove as *const u8);
    builder.symbol("jet_jit_set_has", jet_jit_set_has as *const u8);
    builder.symbol("jet_jit_set_len", jet_jit_set_len as *const u8);
    builder.symbol("jet_jit_set_to_list", jet_jit_set_to_list as *const u8);
    builder.symbol("jet_jit_set_union", jet_jit_set_union as *const u8);
    builder.symbol("jet_jit_deque_new", jet_jit_deque_new as *const u8);
    builder.symbol("jet_jit_deque_push_front", jet_jit_deque_push_front as *const u8);
    builder.symbol("jet_jit_deque_push_back", jet_jit_deque_push_back as *const u8);
    builder.symbol("jet_jit_deque_pop_front", jet_jit_deque_pop_front as *const u8);
    builder.symbol("jet_jit_deque_pop_back", jet_jit_deque_pop_back as *const u8);
    builder.symbol("jet_jit_deque_peek_front", jet_jit_deque_peek_front as *const u8);
    builder.symbol("jet_jit_deque_peek_back", jet_jit_deque_peek_back as *const u8);
    builder.symbol("jet_jit_deque_len", jet_jit_deque_len as *const u8);
}

pub(crate) fn declare_collections_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<CollectionsHostFns, String> {
    use cranelift_codegen::ir::{types, AbiParam, Signature};
    use cranelift_module::{Linkage, Module};

    let cc = module.target_config().default_call_conv;
    let mut sig_new = Signature::new(cc);
    sig_new.returns.push(AbiParam::new(types::I64));
    let mut sig_push = Signature::new(cc);
    sig_push.params.push(AbiParam::new(types::I64));
    sig_push.params.push(AbiParam::new(types::I64));
    let mut sig_push_f64 = Signature::new(cc);
    sig_push_f64.params.push(AbiParam::new(types::I64));
    sig_push_f64.params.push(AbiParam::new(types::F64));
    let mut sig_len = Signature::new(cc);
    sig_len.params.push(AbiParam::new(types::I64));
    sig_len.returns.push(AbiParam::new(types::I64));
    let mut sig_get = sig_len.clone();
    sig_get.params.push(AbiParam::new(types::I64));
    sig_get.params.push(AbiParam::new(types::I32));
    let mut sig_get_f64 = sig_get.clone();
    sig_get_f64.returns.clear();
    sig_get_f64.returns.push(AbiParam::new(types::F64));
    let mut sig_get_opt = sig_len.clone();
    sig_get_opt.params.push(AbiParam::new(types::I64));
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
    let mut sig_join = sig_len.clone();
    sig_join.params.push(AbiParam::new(types::I64));
    let mut sig_map_insert = Signature::new(cc);
    sig_map_insert.params.push(AbiParam::new(types::I64));
    sig_map_insert.params.push(AbiParam::new(types::I64));
    sig_map_insert.params.push(AbiParam::new(types::I64));
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
        list_push: import("jet_jit_list_push", &sig_push)?,
        list_push_f64: import("jet_jit_list_push_f64", &sig_push_f64)?,
        list_get: import("jet_jit_list_get", &sig_get)?,
        list_get_f64: import("jet_jit_list_get_f64", &sig_get_f64)?,
        list_get_opt: import("jet_jit_list_get_opt", &sig_get_opt)?,
        list_set: import("jet_jit_list_set", &sig_set)?,
        list_set_f64: import("jet_jit_list_set_f64", &sig_set_f64)?,
        list_len: import("jet_jit_list_len", &sig_len)?,
        list_eq: import("jet_jit_list_eq", &sig_list_eq)?,
        list_indexes: import("jet_jit_list_indexes", &sig_len)?,
        list_sort: import("jet_jit_list_sort", &sig_sort)?,
        list_clone: import("jet_jit_list_clone", &sig_len)?,
        list_slice: import("jet_jit_list_slice", &sig_slice)?,
        list_join_str: import("jet_jit_list_join_str", &sig_join)?,
        loop_stride_check: import("jet_jit_loop_stride_check", &sig_len)?,
        map_new: import("jet_jit_map_new", &sig_new)?,
        map_clone: import("jet_jit_map_clone", &sig_len)?,
        map_insert: import("jet_jit_map_insert", &sig_map_insert)?,
        map_get: import("jet_jit_map_get", &sig_map_get)?,
        map_get_opt: import("jet_jit_map_get_opt", &sig_map_get_opt)?,
        map_len: import("jet_jit_map_len", &sig_len)?,
        map_key_at: import("jet_jit_map_key_at", &sig_map_at)?,
        map_value_at: import("jet_jit_map_value_at", &sig_map_at)?,
        iter_take: import("jet_jit_iter_take", &sig_get_opt)?,
        iter_skip: import("jet_jit_iter_skip", &sig_get_opt)?,
        iter_step_by: import("jet_jit_iter_step_by", &sig_get_opt)?,
        iter_dedup: import("jet_jit_iter_dedup", &sig_get_opt)?,
        iter_chunks: import("jet_jit_iter_chunks", &sig_get_opt)?,
        iter_windows: import("jet_jit_iter_windows", &sig_get_opt)?,
        list_sum_i64: import("jet_jit_list_sum_i64", &sig_len)?,
        list_sort_by_i64_keys: import("jet_jit_list_sort_by_i64_keys", &sig_sort_by_keys)?,
        print_list: import("jet_jit_print_list", &sig_print_list)?,
        print_opt: import("jet_jit_print_opt", &sig_print_list)?,
        print_enum: import("jet_jit_print_enum", &sig_print_enum)?,
        list_show: import("jet_jit_list_show", &sig_get_opt)?,
        list_remove: import("jet_jit_list_remove", &sig_get_opt)?,
        set_from_list: import("jet_jit_set_from_list", &sig_len)?,
        set_insert: import("jet_jit_set_insert", &sig_list_eq)?,
        set_remove: import("jet_jit_set_remove", &sig_push)?,
        set_has: import("jet_jit_set_has", &sig_list_eq)?,
        set_len: import("jet_jit_set_len", &sig_len)?,
        set_to_list: import("jet_jit_set_to_list", &sig_len)?,
        set_union: import("jet_jit_set_union", &sig_get_opt)?,
        deque_new: import("jet_jit_deque_new", &sig_new)?,
        deque_push_front: import("jet_jit_deque_push_front", &sig_push)?,
        deque_push_back: import("jet_jit_deque_push_back", &sig_push)?,
        deque_pop_front: import("jet_jit_deque_pop_front", &sig_len)?,
        deque_pop_back: import("jet_jit_deque_pop_back", &sig_len)?,
        deque_peek_front: import("jet_jit_deque_peek_front", &sig_len)?,
        deque_peek_back: import("jet_jit_deque_peek_back", &sig_len)?,
        deque_len: import("jet_jit_deque_len", &sig_len)?,
    })
}
