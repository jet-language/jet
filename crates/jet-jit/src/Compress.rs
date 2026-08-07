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

pub(crate) struct CompressHostFns {
    pub gzip_compress: cranelift_module::FuncId,
    pub gzip_decompress: cranelift_module::FuncId,
    pub zstd_compress: cranelift_module::FuncId,
    pub zstd_decompress: cranelift_module::FuncId,
}

pub(crate) fn register_compress_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_gzip_compress", jet_jit_gzip_compress as *const u8);
    builder.symbol("jet_jit_gzip_decompress", jet_jit_gzip_decompress as *const u8);
    builder.symbol("jet_jit_zstd_compress", jet_jit_zstd_compress as *const u8);
    builder.symbol("jet_jit_zstd_decompress", jet_jit_zstd_decompress as *const u8);
}

pub(crate) fn declare_compress_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<CompressHostFns, String> {
    use cranelift_codegen::ir::{types, AbiParam, Signature};
    use cranelift_module::{Linkage, Module};

    let cc = module.target_config().default_call_conv;
    let mut sig_unary = Signature::new(cc);
    sig_unary.params.push(AbiParam::new(types::I64));
    sig_unary.returns.push(AbiParam::new(types::I64));
    let mut import = |name: &str, sig: &Signature| -> Result<cranelift_module::FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
    Ok(CompressHostFns {
        gzip_compress: import("jet_jit_gzip_compress", &sig_unary)?,
        gzip_decompress: import("jet_jit_gzip_decompress", &sig_unary)?,
        zstd_compress: import("jet_jit_zstd_compress", &sig_unary)?,
        zstd_decompress: import("jet_jit_zstd_decompress", &sig_unary)?,
    })
}
