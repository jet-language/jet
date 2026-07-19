//! M5: list host shims for the Cranelift JIT (`JetArena` list handles).

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
    })
}
