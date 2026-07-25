//! `core.encoding.hex` / `base64` / `base32` / `csv` and `core.uuid` host
//! shims (#729). Encode mirrors `jet_std_*` in EncodingCodecs.rs; decode calls
//! `jet_foundation::base_encoding_dispatch` (no third algorithm). CSV mirrors
//! `jet_ring_csv_parse` / `jet_ring_csv_render`. UUID mirrors `jet_std_uuid_*`.

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

// ── CSV (mirrors jet_ring_csv_parse / jet_ring_csv_render) ────────────────────

fn csv_parse(text: &str) -> Result<Vec<Vec<String>>, String> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    let mut closed_quote = false;
    let mut record = 1usize;
    let mut line = 1usize;
    let mut column = 0usize;
    let mut ended_record = false;

    while let Some(ch) = chars.next() {
        column += 1;
        ended_record = false;
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    column += 1;
                    field.push('"');
                } else {
                    quoted = false;
                    closed_quote = true;
                }
            } else {
                field.push(ch);
                if ch == '\n' {
                    line += 1;
                    column = 0;
                }
            }
            continue;
        }

        if closed_quote {
            match ch {
                ',' => {
                    row.push(std::mem::take(&mut field));
                    closed_quote = false;
                }
                '\r' if chars.peek() == Some(&'\n') => {
                    chars.next();
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                    closed_quote = false;
                    ended_record = true;
                    record += 1;
                    line += 1;
                    column = 0;
                }
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                    closed_quote = false;
                    ended_record = true;
                    record += 1;
                    line += 1;
                    column = 0;
                }
                _ => {
                    return Err(format!(
                        "E2701: CSV row {record}, line {line}, column {column} — only quote, comma, CRLF, LF, or EOF may follow a closing quote"
                    ))
                }
            }
            continue;
        }

        match ch {
            '"' if field.is_empty() => quoted = true,
            '"' => {
                return Err(format!(
                    "E2701: CSV row {record}, line {line}, column {column} — quote inside an unquoted field"
                ))
            }
            ',' => row.push(std::mem::take(&mut field)),
            '\r' if chars.peek() == Some(&'\n') => {
                chars.next();
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                ended_record = true;
                record += 1;
                line += 1;
                column = 0;
            }
            '\r' => {
                return Err(format!(
                    "E2701: CSV row {record}, line {line}, column {column} — bare CR is not a record ending"
                ))
            }
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                ended_record = true;
                record += 1;
                line += 1;
                column = 0;
            }
            _ => field.push(ch),
        }
    }
    if quoted {
        return Err(format!(
            "E2701: CSV row {record}, line {line}, column {} — quoted field ended before its closing quote",
            column + 1
        ));
    }
    if !ended_record {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

fn csv_render(rows: &[Vec<String>]) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|field| {
                    if field.contains(',')
                        || field.contains('"')
                        || field.contains('\n')
                        || field.contains('\r')
                    {
                        format!("\"{}\"", field.replace('"', "\"\""))
                    } else {
                        field.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn alloc_string_rows(rows: Vec<Vec<String>>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let outer = rt.heap.alloc_empty_list();
        for row in rows {
            let inner = rt.heap.alloc_empty_list();
            for cell in row {
                let sid = rt.heap.alloc_string(cell);
                let _ = rt.heap.list_push_int(inner, sid);
            }
            let _ = rt.heap.list_push_int(outer, inner);
        }
        outer
    })
}

fn clone_string_rows(list: i64) -> Vec<Vec<String>> {
    Concurrency::with_runtime_mut(|rt| {
        let outer_len = rt.heap.list_len(list).unwrap_or(0);
        let mut rows = Vec::with_capacity(outer_len as usize);
        for i in 0..outer_len {
            let inner = rt.heap.list_get_int(list, i).unwrap_or(0);
            let inner_len = rt.heap.list_len(inner).unwrap_or(0);
            let mut row = Vec::with_capacity(inner_len as usize);
            for j in 0..inner_len {
                let sid = rt.heap.list_get_int(inner, j).unwrap_or(0);
                row.push(rt.heap.clone_string(sid).unwrap_or_default());
            }
            rows.push(row);
        }
        rows
    })
}

extern "C" fn jet_jit_csv_parse(text: i64) -> i64 {
    match csv_parse(&clone_heap_string(text)) {
        Ok(rows) => result_ok_bits(alloc_string_rows(rows) as u64),
        Err(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_csv_to_string(rows: i64) -> i64 {
    let rendered = csv_render(&clone_string_rows(rows));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

// ── UUID (mirrors jet_std_uuid_v4 / jet_std_uuid_v7) ─────────────────────────

fn uuid_fill_random(out: &mut [u8]) {
    use std::io::Read;
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        if f.read_exact(out).is_ok() {
            return;
        }
    }
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let mut state = seed.wrapping_add(0x9E3779B97F4A7C15);
    for b in out.iter_mut() {
        state = state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = (state ^ (state >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        *b = (z ^ (z >> 31)) as u8;
    }
}

fn uuid_format(b: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-\
         {:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13],
        b[14], b[15]
    )
}

extern "C" fn jet_jit_uuid_v4() -> i64 {
    let mut bytes = [0u8; 16];
    uuid_fill_random(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let s = uuid_format(&bytes);
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s))
}

/// `clock` is a 1-based index into `JitRuntime::clocks` (manual ms).
extern "C" fn jet_jit_uuid_v7(clock: i64) -> i64 {
    let ts_ms = Concurrency::with_runtime_mut(|rt| {
        if clock <= 0 {
            return 0i64;
        }
        let idx = (clock as usize).saturating_sub(1);
        rt.clocks.get(idx).copied().unwrap_or(0)
    }) as u64;
    let mut bytes = [0u8; 16];
    bytes[0] = (ts_ms >> 40) as u8;
    bytes[1] = (ts_ms >> 32) as u8;
    bytes[2] = (ts_ms >> 24) as u8;
    bytes[3] = (ts_ms >> 16) as u8;
    bytes[4] = (ts_ms >> 8) as u8;
    bytes[5] = ts_ms as u8;
    uuid_fill_random(&mut bytes[6..]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let s = uuid_format(&bytes);
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s))
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
    pub csv_parse: cranelift_module::FuncId,
    pub csv_to_string: cranelift_module::FuncId,
    pub uuid_v4: cranelift_module::FuncId,
    pub uuid_v7: cranelift_module::FuncId,
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
    builder.symbol("jet_jit_csv_parse", jet_jit_csv_parse as *const u8);
    builder.symbol("jet_jit_csv_to_string", jet_jit_csv_to_string as *const u8);
    builder.symbol("jet_jit_uuid_v4", jet_jit_uuid_v4 as *const u8);
    builder.symbol("jet_jit_uuid_v7", jet_jit_uuid_v7 as *const u8);
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
    let mut sig_nullary = Signature::new(cc);
    sig_nullary.returns.push(AbiParam::new(types::I64));
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
        csv_parse: import("jet_jit_csv_parse", &sig_unary)?,
        csv_to_string: import("jet_jit_csv_to_string", &sig_unary)?,
        uuid_v4: import("jet_jit_uuid_v4", &sig_nullary)?,
        uuid_v7: import("jet_jit_uuid_v7", &sig_unary)?,
    })
}
