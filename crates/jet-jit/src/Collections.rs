//! M5: list host shims for the Cranelift JIT (`Vec<i64>` handles).

use super::Concurrency;

fn trap_index(line: u32) -> ! {
    Concurrency::with_runtime_mut(|rt| {
        rt.stderr
            .push_str("panic: index out of bounds: the index is outside the list\n");
        rt.stderr
            .push_str(&format!("  --> {}:{line}\n", rt.source_file));
    });
    std::process::exit(70);
}

extern "C" fn jet_jit_list_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let id = rt.lists.len() as i64;
        rt.lists.push(Vec::new());
        id
    })
}

extern "C" fn jet_jit_list_push(list: i64, v: i64) {
    Concurrency::with_runtime_mut(|rt| {
        rt.lists
            .get_mut(list as usize)
            .expect("jit list push: bad handle")
            .push(v);
    });
}

extern "C" fn jet_jit_list_len(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.lists
            .get(list as usize)
            .expect("jit list len: bad handle")
            .len() as i64
    })
}

extern "C" fn jet_jit_list_get(list: i64, idx: i64, line: u32) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let xs = rt
            .lists
            .get(list as usize)
            .expect("jit list get: bad handle");
        if idx < 0 || (idx as usize) >= xs.len() {
            rt.stderr
                .push_str("panic: index out of bounds: the index is outside the list\n");
            rt.stderr
                .push_str(&format!("  --> {}:{line}\n", rt.source_file));
            std::process::exit(70);
        }
        xs[idx as usize]
    })
}

/// `0` = absent; otherwise `value + 1`.
extern "C" fn jet_jit_list_get_opt(list: i64, idx: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let xs = rt
            .lists
            .get(list as usize)
            .expect("jit list get_opt: bad handle");
        if idx < 0 || (idx as usize) >= xs.len() {
            return 0;
        }
        xs[idx as usize] + 1
    })
}

extern "C" fn jet_jit_list_set(list: i64, idx: i64, v: i64, line: u32) {
    Concurrency::with_runtime_mut(|rt| {
        let xs = rt
            .lists
            .get_mut(list as usize)
            .expect("jit list set: bad handle");
        if idx < 0 || (idx as usize) >= xs.len() {
            trap_index(line);
        }
        xs[idx as usize] = v;
    });
}

extern "C" fn jet_jit_list_sort(list: i64) {
    Concurrency::with_runtime_mut(|rt| {
        rt.lists
            .get_mut(list as usize)
            .expect("jit list sort: bad handle")
            .sort_unstable();
    });
}

extern "C" fn jet_jit_list_slice(list: i64, start: i64, end: i64, line: u32) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let xs = rt
            .lists
            .get(list as usize)
            .expect("jit list slice: bad handle");
        if start < 0 || end < start || end > xs.len() as i64 {
            rt.stderr
                .push_str("panic: slice out of bounds: the range is outside the list\n");
            rt.stderr
                .push_str(&format!("  --> {}:{line}\n", rt.source_file));
            std::process::exit(70);
        }
        let slice = xs[start as usize..end as usize].to_vec();
        let id = rt.lists.len() as i64;
        rt.lists.push(slice);
        id
    })
}

extern "C" fn jet_jit_list_join_str(list: i64, sep_id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let xs = rt
            .lists
            .get(list as usize)
            .expect("jit list join: bad handle")
            .clone();
        let sep = rt.strings.get(sep_id as usize).cloned().unwrap_or_default();
        let joined = xs
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(&sep);
        let id = rt.strings.len() as i64;
        rt.strings.push(joined);
        id
    })
}

pub(crate) struct CollectionsHostFns {
    pub list_new: cranelift_module::FuncId,
    pub list_push: cranelift_module::FuncId,
    pub list_get: cranelift_module::FuncId,
    pub list_get_opt: cranelift_module::FuncId,
    pub list_set: cranelift_module::FuncId,
    pub list_len: cranelift_module::FuncId,
    pub list_sort: cranelift_module::FuncId,
    pub list_slice: cranelift_module::FuncId,
    pub list_join_str: cranelift_module::FuncId,
}

pub(crate) fn register_collections_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_list_new", jet_jit_list_new as *const u8);
    builder.symbol("jet_jit_list_push", jet_jit_list_push as *const u8);
    builder.symbol("jet_jit_list_get", jet_jit_list_get as *const u8);
    builder.symbol("jet_jit_list_get_opt", jet_jit_list_get_opt as *const u8);
    builder.symbol("jet_jit_list_set", jet_jit_list_set as *const u8);
    builder.symbol("jet_jit_list_len", jet_jit_list_len as *const u8);
    builder.symbol("jet_jit_list_sort", jet_jit_list_sort as *const u8);
    builder.symbol("jet_jit_list_slice", jet_jit_list_slice as *const u8);
    builder.symbol("jet_jit_list_join_str", jet_jit_list_join_str as *const u8);
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
    let mut sig_len = Signature::new(cc);
    sig_len.params.push(AbiParam::new(types::I64));
    sig_len.returns.push(AbiParam::new(types::I64));
    let mut sig_get = sig_len.clone();
    sig_get.params.push(AbiParam::new(types::I64));
    sig_get.params.push(AbiParam::new(types::I32));
    let mut sig_get_opt = sig_len.clone();
    sig_get_opt.params.push(AbiParam::new(types::I64));
    let sig_set = sig_get.clone();
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
        list_get: import("jet_jit_list_get", &sig_get)?,
        list_get_opt: import("jet_jit_list_get_opt", &sig_get_opt)?,
        list_set: import("jet_jit_list_set", &sig_set)?,
        list_len: import("jet_jit_list_len", &sig_len)?,
        list_sort: import("jet_jit_list_sort", &sig_sort)?,
        list_slice: import("jet_jit_list_slice", &sig_slice)?,
        list_join_str: import("jet_jit_list_join_str", &sig_join)?,
    })
}
