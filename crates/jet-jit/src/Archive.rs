//! `core.archive` host shims (#729).
//! Adapts the source package's internal dependency-free archive ABI calls through
//! Foundation — no public semantic or engine-specific fallback.

use super::Concurrency;
use jet_foundation::CoreArchive as runtime;

fn clone_heap_string(id: i64) -> String {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id).unwrap_or_default())
}

fn clone_heap_bytes(list: i64) -> Vec<u8> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(list, i).unwrap_or(0) as u8);
        }
        out
    })
}

fn alloc_byte_list(bytes: &[u8]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for &b in bytes {
            let _ = rt.heap.list_push_int(list, b as i64);
        }
        list
    })
}

extern "C" fn jet_jit_zip_compress(name: i64, bytes: i64) -> i64 {
    let out = runtime::jet_archive_zip_compress(
        &clone_heap_string(name),
        &clone_heap_bytes(bytes),
    );
    alloc_byte_list(&out)
}

extern "C" fn jet_jit_zip_decompress(bytes: i64) -> i64 {
    let out = runtime::jet_archive_zip_decompress(&clone_heap_bytes(bytes));
    alloc_byte_list(&out)
}

extern "C" fn jet_jit_tar_add(archive: i64, name: i64, bytes: i64) -> i64 {
    let out = runtime::jet_archive_tar_add(
        &clone_heap_bytes(archive),
        &clone_heap_string(name),
        &clone_heap_bytes(bytes),
    );
    alloc_byte_list(&out)
}

extern "C" fn jet_jit_tar_get(archive: i64, name: i64) -> i64 {
    let out =
        runtime::jet_archive_tar_get(&clone_heap_bytes(archive), &clone_heap_string(name));
    alloc_byte_list(&out)
}

extern "C" fn jet_jit_tar_names_json(archive: i64) -> i64 {
    let json = runtime::jet_archive_tar_names_json(&clone_heap_bytes(archive));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(json))
}

pub(crate) struct ArchiveHostFns {
    pub zip_compress: cranelift_module::FuncId,
    pub zip_decompress: cranelift_module::FuncId,
    pub tar_add: cranelift_module::FuncId,
    pub tar_get: cranelift_module::FuncId,
    pub tar_names_json: cranelift_module::FuncId,
}

pub(crate) fn register_archive_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_zip_compress", jet_jit_zip_compress as *const u8);
    builder.symbol("jet_jit_zip_decompress", jet_jit_zip_decompress as *const u8);
    builder.symbol("jet_jit_tar_add", jet_jit_tar_add as *const u8);
    builder.symbol("jet_jit_tar_get", jet_jit_tar_get as *const u8);
    builder.symbol("jet_jit_tar_names_json", jet_jit_tar_names_json as *const u8);
}

pub(crate) fn declare_archive_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<ArchiveHostFns, String> {
    use cranelift_codegen::ir::{types, AbiParam, Signature};
    use cranelift_module::{Linkage, Module};

    let cc = module.target_config().default_call_conv;
    let mut sig_unary = Signature::new(cc);
    sig_unary.params.push(AbiParam::new(types::I64));
    sig_unary.returns.push(AbiParam::new(types::I64));
    let mut sig_bin = Signature::new(cc);
    sig_bin.params.push(AbiParam::new(types::I64));
    sig_bin.params.push(AbiParam::new(types::I64));
    sig_bin.returns.push(AbiParam::new(types::I64));
    let mut sig_tern = Signature::new(cc);
    sig_tern.params.push(AbiParam::new(types::I64));
    sig_tern.params.push(AbiParam::new(types::I64));
    sig_tern.params.push(AbiParam::new(types::I64));
    sig_tern.returns.push(AbiParam::new(types::I64));
    let mut import = |name: &str, sig: &Signature| -> Result<cranelift_module::FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
    Ok(ArchiveHostFns {
        zip_compress: import("jet_jit_zip_compress", &sig_bin)?,
        zip_decompress: import("jet_jit_zip_decompress", &sig_unary)?,
        tar_add: import("jet_jit_tar_add", &sig_tern)?,
        tar_get: import("jet_jit_tar_get", &sig_bin)?,
        tar_names_json: import("jet_jit_tar_names_json", &sig_unary)?,
    })
}
