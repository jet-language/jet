//! `core.encoding.hex` / `base64` / `base32` host shims (#729).
//! Encode mirrors `jet_std_*` in EncodingCodecs.rs; decode calls
//! `jet_foundation::base_encoding_dispatch` (no third algorithm).

use super::Concurrency;
use jet_foundation::base_encoding_dispatch;
use jet_foundation::PackageEdition;

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

fn result_ok_bits(bits: u64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(super::JitResultValue { ok: true, bits });
        rt.results.len() as i64
    })
}

fn result_err_msg(msg: &str) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let sid = rt.heap.alloc_string(msg.to_string());
        rt.results.push(super::JitResultValue {
            ok: false,
            bits: sid as u64,
        });
        rt.results.len() as i64
    })
}

// ── encode (mirrors EncodingCodecs.rs jet_std_hex/b64/base32_encode) ─────────

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64_CHARS[(n >> 18) as usize] as char);
        out.push(B64_CHARS[((n >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64_CHARS[((n >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64_CHARS[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn b64url_encode(bytes: &[u8]) -> String {
    b64_encode(bytes)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}

const BASE32_CHARS: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0u8;
    for &b in bytes {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            let idx = ((buffer >> (bits - 5)) & 31) as usize;
            out.push(BASE32_CHARS[idx] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        let idx = ((buffer << (5 - bits)) & 31) as usize;
        out.push(BASE32_CHARS[idx] as char);
    }
    while out.len() % 8 != 0 {
        out.push('=');
    }
    out
}

fn hex_decode(text: &str) -> Result<Vec<u8>, String> {
    let s = text.trim();
    if s.len() % 2 != 0 {
        return Err(format!("hex string has odd length ({})", s.len()));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        match u8::from_str_radix(&s[i..i + 2], 16) {
            Ok(b) => out.push(b),
            Err(_) => return Err(format!("invalid hex at offset {}: {:?}", i, &s[i..i + 2])),
        }
    }
    Ok(out)
}

extern "C" fn jet_jit_hex_encode(bytes: i64) -> i64 {
    let encoded = hex_encode(&clone_heap_bytes(bytes));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(encoded))
}

extern "C" fn jet_jit_hex_decode(text: i64) -> i64 {
    match hex_decode(&clone_heap_string(text)) {
        Ok(bytes) => result_ok_bits(alloc_byte_list(&bytes) as u64),
        Err(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_b64_encode(bytes: i64) -> i64 {
    let encoded = b64_encode(&clone_heap_bytes(bytes));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(encoded))
}

extern "C" fn jet_jit_b64_encode_url(bytes: i64) -> i64 {
    let encoded = b64url_encode(&clone_heap_bytes(bytes));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(encoded))
}

extern "C" fn jet_jit_b64_decode(text: i64) -> i64 {
    let edition = PackageEdition::package_edition();
    match base_encoding_dispatch::decode_base64(&edition, &clone_heap_string(text), false, false) {
        Ok(bytes) => result_ok_bits(alloc_byte_list(&bytes) as u64),
        Err(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_b64_decode_url(text: i64) -> i64 {
    let edition = PackageEdition::package_edition();
    match base_encoding_dispatch::decode_base64url(&edition, &clone_heap_string(text), false, false)
    {
        Ok(bytes) => result_ok_bits(alloc_byte_list(&bytes) as u64),
        Err(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_base32_encode(bytes: i64) -> i64 {
    let encoded = base32_encode(&clone_heap_bytes(bytes));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(encoded))
}

extern "C" fn jet_jit_base32_decode(text: i64) -> i64 {
    let edition = PackageEdition::package_edition();
    match base_encoding_dispatch::decode_base32(
        &edition,
        &clone_heap_string(text),
        false,
        false,
        false,
    ) {
        Ok(bytes) => result_ok_bits(alloc_byte_list(&bytes) as u64),
        Err(e) => result_err_msg(&e),
    }
}

pub(crate) struct EncodingHostFns {
    pub hex_encode: cranelift_module::FuncId,
    pub hex_decode: cranelift_module::FuncId,
    pub b64_encode: cranelift_module::FuncId,
    pub b64_encode_url: cranelift_module::FuncId,
    pub b64_decode: cranelift_module::FuncId,
    pub b64_decode_url: cranelift_module::FuncId,
    pub base32_encode: cranelift_module::FuncId,
    pub base32_decode: cranelift_module::FuncId,
}

pub(crate) fn register_encoding_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_hex_encode", jet_jit_hex_encode as *const u8);
    builder.symbol("jet_jit_hex_decode", jet_jit_hex_decode as *const u8);
    builder.symbol("jet_jit_b64_encode", jet_jit_b64_encode as *const u8);
    builder.symbol("jet_jit_b64_encode_url", jet_jit_b64_encode_url as *const u8);
    builder.symbol("jet_jit_b64_decode", jet_jit_b64_decode as *const u8);
    builder.symbol("jet_jit_b64_decode_url", jet_jit_b64_decode_url as *const u8);
    builder.symbol("jet_jit_base32_encode", jet_jit_base32_encode as *const u8);
    builder.symbol("jet_jit_base32_decode", jet_jit_base32_decode as *const u8);
}

pub(crate) fn declare_encoding_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<EncodingHostFns, String> {
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
    Ok(EncodingHostFns {
        hex_encode: import("jet_jit_hex_encode", &sig_unary)?,
        hex_decode: import("jet_jit_hex_decode", &sig_unary)?,
        b64_encode: import("jet_jit_b64_encode", &sig_unary)?,
        b64_encode_url: import("jet_jit_b64_encode_url", &sig_unary)?,
        b64_decode: import("jet_jit_b64_decode", &sig_unary)?,
        b64_decode_url: import("jet_jit_b64_decode_url", &sig_unary)?,
        base32_encode: import("jet_jit_base32_encode", &sig_unary)?,
        base32_decode: import("jet_jit_base32_decode", &sig_unary)?,
    })
}
