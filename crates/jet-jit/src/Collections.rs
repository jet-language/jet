//! M5: list/map host shims for the Cranelift JIT (`JetArena` handles).

use super::Concurrency;

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
        let xs = rt
            .heap
            .clone_int_list(list)
            .expect("jit print list: bad handle");
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
        rt.stdout.push('[');
        rt.stdout.push_str(&parts.join(", "));
        rt.stdout.push(']');
        rt.stdout.push('\n');
    });
}

/// Print `T?` using JIT packed Option encoding (`0` = None, else `value + 1`).
/// `string_elems != 0` → payload is a string handle; else i64.
extern "C" fn jet_jit_print_opt(packed: i64, string_elems: i64) {
    Concurrency::with_runtime_mut(|rt| {
        if packed == 0 {
            rt.stdout.push_str("null\n");
            return;
        }
        let payload = packed - 1;
        if string_elems != 0 {
            let text = rt.heap.clone_string(payload).unwrap_or_default();
            rt.stdout.push_str(&text);
        } else {
            rt.stdout.push_str(&payload.to_string());
        }
        rt.stdout.push('\n');
    });
}

pub(crate) struct CollectionsHostFns {
    pub list_new: cranelift_module::FuncId,
    pub list_push: cranelift_module::FuncId,
    pub list_push_f64: cranelift_module::FuncId,
    pub list_get: cranelift_module::FuncId,
    pub list_get_f64: cranelift_module::FuncId,
    pub list_get_opt: cranelift_module::FuncId,
    pub list_set: cranelift_module::FuncId,
    pub list_set_f64: cranelift_module::FuncId,
    pub list_len: cranelift_module::FuncId,
    pub list_sort: cranelift_module::FuncId,
    pub list_clone: cranelift_module::FuncId,
    pub list_slice: cranelift_module::FuncId,
    pub list_join_str: cranelift_module::FuncId,
    pub loop_stride_check: cranelift_module::FuncId,
    pub map_new: cranelift_module::FuncId,
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
}

pub(crate) fn register_collections_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_list_new", jet_jit_list_new as *const u8);
    builder.symbol("jet_jit_list_push", jet_jit_list_push as *const u8);
    builder.symbol("jet_jit_list_push_f64", jet_jit_list_push_f64 as *const u8);
    builder.symbol("jet_jit_list_get", jet_jit_list_get as *const u8);
    builder.symbol("jet_jit_list_get_f64", jet_jit_list_get_f64 as *const u8);
    builder.symbol("jet_jit_list_get_opt", jet_jit_list_get_opt as *const u8);
    builder.symbol("jet_jit_list_set", jet_jit_list_set as *const u8);
    builder.symbol("jet_jit_list_set_f64", jet_jit_list_set_f64 as *const u8);
    builder.symbol("jet_jit_list_len", jet_jit_list_len as *const u8);
    builder.symbol("jet_jit_list_sort", jet_jit_list_sort as *const u8);
    builder.symbol("jet_jit_list_clone", jet_jit_list_clone as *const u8);
    builder.symbol("jet_jit_list_slice", jet_jit_list_slice as *const u8);
    builder.symbol("jet_jit_list_join_str", jet_jit_list_join_str as *const u8);
    builder.symbol("jet_jit_loop_stride_check", jet_jit_loop_stride_check as *const u8);
    builder.symbol("jet_jit_map_new", jet_jit_map_new as *const u8);
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
    let sig_set = sig_get.clone();
    let mut sig_set_f64 = sig_get_f64.clone();
    sig_set_f64.returns.clear();
    let mut sig_sort = sig_len.clone();
    sig_sort.returns.clear();
    let mut sig_slice = sig_get.clone();
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
    let mut sig_sort_by_keys = sig_get_opt.clone();
    sig_sort_by_keys.returns.clear();

    let mut import = |name: &str, sig: &Signature| -> Result<cranelift_module::FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };

    Ok(CollectionsHostFns {
        list_new: import("jet_jit_list_new", &sig_new)?,
        list_push: import("jet_jit_list_push", &sig_push)?,
        list_push_f64: import("jet_jit_list_push_f64", &sig_push_f64)?,
        list_get: import("jet_jit_list_get", &sig_get)?,
        list_get_f64: import("jet_jit_list_get_f64", &sig_get_f64)?,
        list_get_opt: import("jet_jit_list_get_opt", &sig_get_opt)?,
        list_set: import("jet_jit_list_set", &sig_set)?,
        list_set_f64: import("jet_jit_list_set_f64", &sig_set_f64)?,
        list_len: import("jet_jit_list_len", &sig_len)?,
        list_sort: import("jet_jit_list_sort", &sig_sort)?,
        list_clone: import("jet_jit_list_clone", &sig_len)?,
        list_slice: import("jet_jit_list_slice", &sig_slice)?,
        list_join_str: import("jet_jit_list_join_str", &sig_join)?,
        loop_stride_check: import("jet_jit_loop_stride_check", &sig_len)?,
        map_new: import("jet_jit_map_new", &sig_new)?,
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
    })
}
