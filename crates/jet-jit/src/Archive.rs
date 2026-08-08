//! `core.archive` host shims (#729).
//! Adapts the source package's internal dependency-free archive ABI calls through
//! Foundation — no public semantic or engine-specific fallback.

use super::Concurrency;
use jet_foundation::CoreArchive as runtime;
use crate::Marshal::{clone_string, clone_bytes, alloc_byte_list};

extern "C" fn jet_jit_zip_compress(name: i64, bytes: i64) -> i64 {
    let out = runtime::jet_archive_zip_compress(
        &clone_string(name),
        &clone_bytes(bytes),
    );
    alloc_byte_list(&out)
}

extern "C" fn jet_jit_zip_decompress(bytes: i64) -> i64 {
    let out = runtime::jet_archive_zip_decompress(&clone_bytes(bytes));
    alloc_byte_list(&out)
}

extern "C" fn jet_jit_tar_add(archive: i64, name: i64, bytes: i64) -> i64 {
    let out = runtime::jet_archive_tar_add(
        &clone_bytes(archive),
        &clone_string(name),
        &clone_bytes(bytes),
    );
    alloc_byte_list(&out)
}

extern "C" fn jet_jit_tar_get(archive: i64, name: i64) -> i64 {
    let out =
        runtime::jet_archive_tar_get(&clone_bytes(archive), &clone_string(name));
    alloc_byte_list(&out)
}

extern "C" fn jet_jit_tar_names_json(archive: i64) -> i64 {
    let json = runtime::jet_archive_tar_names_json(&clone_bytes(archive));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(json))
}

host_fns! {
    struct ArchiveHostFns;
    register: register_archive_symbols;
    declare: declare_archive_host_fns(module) {
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


    }
    zip_compress: "jet_jit_zip_compress" => jet_jit_zip_compress: sig_bin;
    zip_decompress: "jet_jit_zip_decompress" => jet_jit_zip_decompress: sig_unary;
    tar_add: "jet_jit_tar_add" => jet_jit_tar_add: sig_tern;
    tar_get: "jet_jit_tar_get" => jet_jit_tar_get: sig_bin;
    tar_names_json: "jet_jit_tar_names_json" => jet_jit_tar_names_json: sig_unary;
}





