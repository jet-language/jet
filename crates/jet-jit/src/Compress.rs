//! `core.compress.gzip` / `zstd` host shims (#729).
//! Calls the canonical FFI runtime via `include!` — no third algorithm.

use crate::Marshal::{clone_bytes, alloc_byte_list, result_ok, result_err_msg};

mod runtime {
    include!("../../jet-pkg-model/src/Prelude/Compress.rs");
}

extern "C" fn jet_jit_gzip_compress(bytes: i64) -> i64 {
    let out = runtime::jet_compress_gzip_compress(&clone_bytes(bytes));
    alloc_byte_list(&out)
}

extern "C" fn jet_jit_gzip_decompress(bytes: i64) -> i64 {
    match runtime::jet_compress_gzip_decompress(&clone_bytes(bytes)) {
        Ok(out) => result_ok(alloc_byte_list(&out) as u64),
        Err(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_zstd_compress(bytes: i64) -> i64 {
    let out = runtime::jet_compress_zstd_compress(&clone_bytes(bytes));
    alloc_byte_list(&out)
}

extern "C" fn jet_jit_zstd_decompress(bytes: i64) -> i64 {
    match runtime::jet_compress_zstd_decompress(&clone_bytes(bytes)) {
        Ok(out) => result_ok(alloc_byte_list(&out) as u64),
        Err(e) => result_err_msg(&e),
    }
}

host_fns! {
    struct CompressHostFns;
    register: register_compress_symbols;
    declare: declare_compress_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::{Linkage, Module};
        let cc = module.target_config().default_call_conv;
        let mut sig_unary = Signature::new(cc);
        sig_unary.params.push(AbiParam::new(types::I64));
        sig_unary.returns.push(AbiParam::new(types::I64));


    }
    gzip_compress: "jet_jit_gzip_compress" => jet_jit_gzip_compress: sig_unary;
    gzip_decompress: "jet_jit_gzip_decompress" => jet_jit_gzip_decompress: sig_unary;
    zstd_compress: "jet_jit_zstd_compress" => jet_jit_zstd_compress: sig_unary;
    zstd_decompress: "jet_jit_zstd_decompress" => jet_jit_zstd_decompress: sig_unary;
}





