//! Native JIT adapters for `binary.Reader`, `text.Cursor`, and match scans.
//! Marshalling only: handles in, heap values out. The semantics come from the
//! shared kernels in `jet_foundation::StreamCursor` (spliced verbatim into the
//! AOT prelude) and `jet_foundation::MatchScan` — no second parser.

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use jet_foundation::AST::{BinMatchPart, StrMatchPart, Type};
use jet_foundation::MatchScan::{
    bin_match_consumed, bin_match_scan, str_match_consumed, str_match_scan, BinBind,
};
use jet_foundation::StreamCursor as kernel;
use std::sync::Mutex;

/// Pattern tables live outside `Runtime` so ids baked into Cranelift IR during
/// lowering survive Runtime resets between compile and execute.
static STR_PATTERNS: Mutex<Vec<Vec<StrMatchPart>>> = Mutex::new(Vec::new());
static BIN_PATTERNS: Mutex<Vec<Vec<BinMatchPart>>> = Mutex::new(Vec::new());

/// Reader/Cursor state is the shared D-SHIFT1 kernel (`jet-foundation`), the
/// same source the AOT prelude splices in — the JIT only marshals handles.
pub(crate) type ReaderSlot = kernel::JetReader;
pub(crate) type CursorSlot = kernel::JetCursor;

fn push_reader(slot: ReaderSlot) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.readers.push(slot);
        rt.readers.len() as i64
    })
}

fn push_cursor(slot: CursorSlot) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.cursors.push(slot);
        rt.cursors.len() as i64
    })
}

fn with_reader_mut<R>(handle: i64, f: impl FnOnce(&mut ReaderSlot) -> R) -> Option<R> {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        rt.readers.get_mut(idx).map(f)
    })
}

fn with_cursor_mut<R>(handle: i64, f: impl FnOnce(&mut CursorSlot) -> R) -> Option<R> {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        rt.cursors.get_mut(idx).map(f)
    })
}

fn result_ok(bits: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| crate::runtime_host::alloc_jit_result(rt, true, bits as u64))
}

fn result_err(msg: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let sid = rt.heap.alloc_string(msg);
        crate::runtime_host::alloc_jit_result(rt, false, sid as u64)
    })
}

fn result_ok_bytes(bytes: Vec<u8>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_int_list(bytes.into_iter().map(|b| b as i64).collect());
        crate::runtime_host::alloc_jit_result(rt, true, list as u64)
    })
}

fn clone_byte_list(handle: i64) -> Vec<u8> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(handle).unwrap_or(0);
        (0..len)
            .filter_map(|i| rt.heap.list_get_int(handle, i).map(|v| v as u8))
            .collect()
    })
}

extern "C" fn jet_jit_reader_over(bytes: i64) -> i64 {
    push_reader(kernel::jet_reader_over(&clone_byte_list(bytes)))
}

macro_rules! reader_read {
    ($name:ident, $kernel:path) => {
        extern "C" fn $name(handle: i64) -> i64 {
            match with_reader_mut(handle, |r| $kernel(r).map(|v| v as i64)) {
                Some(Ok(v)) => result_ok(v),
                Some(Err(e)) => result_err(e),
                None => result_err("Reader: bad handle".into()),
            }
        }
    };
}

reader_read!(jet_jit_reader_read_u8, kernel::jet_reader_read_u8);
reader_read!(jet_jit_reader_read_u16_le, kernel::jet_reader_read_u16_le);
reader_read!(jet_jit_reader_read_u16_be, kernel::jet_reader_read_u16_be);
reader_read!(jet_jit_reader_read_u32_le, kernel::jet_reader_read_u32_le);
reader_read!(jet_jit_reader_read_u32_be, kernel::jet_reader_read_u32_be);
reader_read!(jet_jit_reader_read_u64_le, kernel::jet_reader_read_u64_le);
reader_read!(jet_jit_reader_read_u64_be, kernel::jet_reader_read_u64_be);

extern "C" fn jet_jit_reader_take(handle: i64, n: i64) -> i64 {
    match with_reader_mut(handle, |r| kernel::jet_reader_take(r, n)) {
        Some(Ok(bytes)) => result_ok_bytes(bytes),
        Some(Err(e)) => result_err(e),
        None => result_err("Reader: bad handle".into()),
    }
}

extern "C" fn jet_jit_reader_remaining(handle: i64) -> i64 {
    with_reader_mut(handle, |r| kernel::jet_reader_remaining(r)).unwrap_or(0)
}

extern "C" fn jet_jit_reader_at_end(handle: i64) -> i8 {
    with_reader_mut(handle, |r| i8::from(kernel::jet_reader_at_end(r))).unwrap_or(1)
}

extern "C" fn jet_jit_cursor_over(text: i64) -> i64 {
    let s = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(text).unwrap_or_default());
    push_cursor(kernel::jet_cursor_over(&s))
}

extern "C" fn jet_jit_cursor_skip_ws(handle: i64) {
    let _ = with_cursor_mut(handle, kernel::jet_cursor_skip_ws);
}

extern "C" fn jet_jit_cursor_take_until(handle: i64, delim: i64) -> i64 {
    let delim = Concurrency::with_runtime_mut(|rt| {
        rt.heap.clone_string(delim).unwrap_or_default()
    });
    match with_cursor_mut(handle, |c| kernel::jet_cursor_take_until(c, &delim)) {
        Some(Ok(s)) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
            result_ok(sid)
        }
        Some(Err(e)) => result_err(e),
        None => result_err("Cursor: bad handle".into()),
    }
}

extern "C" fn jet_jit_cursor_advance(handle: i64, nbytes: i64) {
    let _ = with_cursor_mut(handle, |c| {
        c.pos = (c.pos + nbytes as usize).min(c.buf.len());
    });
}

extern "C" fn jet_jit_reader_advance_bits(handle: i64, bits: i64) {
    let _ = with_reader_mut(handle, |r| {
        let bytes = (bits as usize + 7) / 8;
        // bit-accurate: store bit pos in pos*8... keep byte pos when aligned
        r.pos = (r.pos * 8 + bits as usize) / 8;
        let _ = bytes;
    });
}


pub(crate) fn install_str_pattern(parts: Vec<StrMatchPart>) -> i64 {
    let mut table = STR_PATTERNS.lock().expect("str pattern table");
    table.push(parts);
    table.len() as i64
}

pub(crate) fn install_bin_pattern(parts: Vec<BinMatchPart>) -> i64 {
    let mut table = BIN_PATTERNS.lock().expect("bin pattern table");
    table.push(parts);
    table.len() as i64
}

fn pack_str_binds(binds: &[(String, Type, String)]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let handle = rt.heap.alloc_record(binds.len());
        for (i, (_, ty, raw)) in binds.iter().enumerate() {
            let idx = i as i64;
            match ty {
                Type::Int | Type::IntN { .. } => {
                    let _ = rt.heap.record_set_int(handle, idx, raw.parse::<i64>().unwrap_or(0));
                }
                Type::Float | Type::Float32 => {
                    let _ = rt
                        .heap
                        .record_set_float(handle, idx, raw.parse::<f64>().unwrap_or(0.0));
                }
                Type::Bool => {
                    let _ = rt.heap.record_set_bool(
                        handle,
                        idx,
                        matches!(raw.as_str(), "true" | "True" | "1"),
                    );
                }
                _ => {
                    let sid = rt.heap.alloc_string(raw.clone());
                    let _ = rt.heap.record_set_string(handle, idx, sid);
                }
            }
        }
        handle
    })
}

fn pack_bin_binds(binds: &[(String, Type, BinBind)]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let handle = rt.heap.alloc_record(binds.len());
        for (i, (_, _, bind)) in binds.iter().enumerate() {
            let v = match bind {
                BinBind::Int(v) => *v,
                BinBind::Rest(bytes) => {
                    rt.heap.alloc_int_list(bytes.iter().map(|b| *b as i64).collect()) as i64
                }
            };
            let _ = rt.heap.record_set_int(handle, i as i64, v);
        }
        handle
    })
}

fn str_pattern(pattern: i64) -> Option<Vec<StrMatchPart>> {
    let table = STR_PATTERNS.lock().expect("str pattern table");
    let idx = (pattern as usize).wrapping_sub(1);
    table.get(idx).cloned()
}

fn bin_pattern(pattern: i64) -> Option<Vec<BinMatchPart>> {
    let table = BIN_PATTERNS.lock().expect("bin pattern table");
    let idx = (pattern as usize).wrapping_sub(1);
    table.get(idx).cloned()
}

/// Full-match probe: 1 = Some, 0 = None.
extern "C" fn jet_jit_str_match_is_some(subject: i64, pattern: i64) -> i8 {
    let text = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(subject).unwrap_or_default());
    let Some(parts) = str_pattern(pattern) else {
        return 0;
    };
    i8::from(str_match_scan(&text, &parts, false).is_some())
}

/// Full-match unwrap → tuple struct handle (caller proved Some).
extern "C" fn jet_jit_str_match_unwrap(subject: i64, pattern: i64) -> i64 {
    let text = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(subject).unwrap_or_default());
    let Some(parts) = str_pattern(pattern) else {
        return 0;
    };
    match str_match_scan(&text, &parts, false) {
        Some(binds) => pack_str_binds(&binds),
        None => 0,
    }
}

extern "C" fn jet_jit_bin_match_is_some(subject: i64, pattern: i64) -> i8 {
    let bytes = clone_byte_list(subject);
    let Some(parts) = bin_pattern(pattern) else {
        return 0;
    };
    i8::from(bin_match_scan(&bytes, &parts, false).is_some())
}

extern "C" fn jet_jit_bin_match_unwrap(subject: i64, pattern: i64) -> i64 {
    let bytes = clone_byte_list(subject);
    let Some(parts) = bin_pattern(pattern) else {
        return 0;
    };
    match bin_match_scan(&bytes, &parts, false) {
        Some((_, binds)) => pack_bin_binds(&binds),
        None => 0,
    }
}

/// Cursor.take_pattern — Result<tuple, String>. Advances cursor on Ok.
extern "C" fn jet_jit_cursor_take_pattern(handle: i64, pattern: i64) -> i64 {
    let Some(parts) = str_pattern(pattern) else {
        return result_err("Cursor.take_pattern: bad pattern".into());
    };
    match with_cursor_mut(handle, |c| {
        let hit = {
            let tail = kernel::jet_cursor_tail(c);
            str_match_scan(tail, &parts, true)
                .map(|binds| (str_match_consumed(tail, &parts).unwrap_or(0), binds))
        };
        match hit {
            Some((consumed, binds)) => {
                kernel::jet_cursor_take_pattern(c, consumed);
                Ok(binds)
            }
            None => Err(kernel::jet_cursor_pattern_miss(c)),
        }
    }) {
        Some(Ok(binds)) => result_ok(pack_str_binds(&binds)),
        Some(Err(e)) => result_err(e),
        None => result_err("Cursor: bad handle".into()),
    }
}

/// Reader.take_pattern — Result<tuple, String>. Advances reader on Ok.
extern "C" fn jet_jit_reader_take_pattern(handle: i64, pattern: i64) -> i64 {
    let Some(parts) = bin_pattern(pattern) else {
        return result_err("Reader.take_pattern: bad pattern".into());
    };
    match with_reader_mut(handle, |r| {
        let hit = bin_match_scan(kernel::jet_reader_tail(r), &parts, true)
            .and_then(|(bit_pos, binds)| bin_match_consumed(bit_pos).map(|n| (n, binds)));
        match hit {
            Some((consumed, binds)) => {
                kernel::jet_reader_take_pattern(r, consumed);
                Ok(binds)
            }
            None => Err(kernel::jet_reader_pattern_miss(r)),
        }
    }) {
        Some(Ok(binds)) => result_ok(pack_bin_binds(&binds)),
        Some(Err(e)) => result_err(e),
        None => result_err("Reader: bad handle".into()),
    }
}

pub(crate) struct HostFns {
    pub reader_over: FuncId,
    pub reader_read_u8: FuncId,
    pub reader_read_u16_le: FuncId,
    pub reader_read_u16_be: FuncId,
    pub reader_read_u32_le: FuncId,
    pub reader_read_u32_be: FuncId,
    pub reader_read_u64_le: FuncId,
    pub reader_read_u64_be: FuncId,
    pub reader_take: FuncId,
    pub reader_remaining: FuncId,
    pub reader_at_end: FuncId,
    pub cursor_over: FuncId,
    pub cursor_skip_ws: FuncId,
    pub cursor_take_until: FuncId,
    pub cursor_advance: FuncId,
    pub str_match_is_some: FuncId,
    pub str_match_unwrap: FuncId,
    pub bin_match_is_some: FuncId,
    pub bin_match_unwrap: FuncId,
    pub cursor_take_pattern: FuncId,
    pub reader_take_pattern: FuncId,
}

pub(crate) fn register_symbols(builder: &mut JITBuilder) {
    builder.symbol("jet_jit_reader_over", jet_jit_reader_over as *const u8);
    builder.symbol("jet_jit_reader_read_u8", jet_jit_reader_read_u8 as *const u8);
    builder.symbol(
        "jet_jit_reader_read_u16_le",
        jet_jit_reader_read_u16_le as *const u8,
    );
    builder.symbol(
        "jet_jit_reader_read_u16_be",
        jet_jit_reader_read_u16_be as *const u8,
    );
    builder.symbol(
        "jet_jit_reader_read_u32_le",
        jet_jit_reader_read_u32_le as *const u8,
    );
    builder.symbol(
        "jet_jit_reader_read_u32_be",
        jet_jit_reader_read_u32_be as *const u8,
    );
    builder.symbol(
        "jet_jit_reader_read_u64_le",
        jet_jit_reader_read_u64_le as *const u8,
    );
    builder.symbol(
        "jet_jit_reader_read_u64_be",
        jet_jit_reader_read_u64_be as *const u8,
    );
    builder.symbol("jet_jit_reader_take", jet_jit_reader_take as *const u8);
    builder.symbol(
        "jet_jit_reader_remaining",
        jet_jit_reader_remaining as *const u8,
    );
    builder.symbol("jet_jit_reader_at_end", jet_jit_reader_at_end as *const u8);
    builder.symbol("jet_jit_cursor_over", jet_jit_cursor_over as *const u8);
    builder.symbol("jet_jit_cursor_skip_ws", jet_jit_cursor_skip_ws as *const u8);
    builder.symbol(
        "jet_jit_cursor_take_until",
        jet_jit_cursor_take_until as *const u8,
    );
    builder.symbol("jet_jit_cursor_advance", jet_jit_cursor_advance as *const u8);
    builder.symbol("jet_jit_str_match_is_some", jet_jit_str_match_is_some as *const u8);
    builder.symbol("jet_jit_str_match_unwrap", jet_jit_str_match_unwrap as *const u8);
    builder.symbol("jet_jit_bin_match_is_some", jet_jit_bin_match_is_some as *const u8);
    builder.symbol("jet_jit_bin_match_unwrap", jet_jit_bin_match_unwrap as *const u8);
    builder.symbol("jet_jit_cursor_take_pattern", jet_jit_cursor_take_pattern as *const u8);
    builder.symbol("jet_jit_reader_take_pattern", jet_jit_reader_take_pattern as *const u8);
    let _ = jet_jit_reader_advance_bits;
}

pub(crate) fn declare(module: &mut JITModule) -> Result<HostFns, String> {
    let cc = module.target_config().default_call_conv;
    let mut sig_unary = Signature::new(cc);
    sig_unary.params.push(AbiParam::new(types::I64));
    sig_unary.returns.push(AbiParam::new(types::I64));
    let mut sig_binary = sig_unary.clone();
    sig_binary.params.push(AbiParam::new(types::I64));
    let mut sig_void_unary = Signature::new(cc);
    sig_void_unary.params.push(AbiParam::new(types::I64));
    let mut sig_i8 = Signature::new(cc);
    sig_i8.params.push(AbiParam::new(types::I64));
    sig_i8.returns.push(AbiParam::new(types::I8));
    let mut sig_binary_i8 = Signature::new(cc);
    sig_binary_i8.params.push(AbiParam::new(types::I64));
    sig_binary_i8.params.push(AbiParam::new(types::I64));
    sig_binary_i8.returns.push(AbiParam::new(types::I8));
    let mut import = |name: &str, sig: &Signature| -> Result<FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
    Ok(HostFns {
        reader_over: import("jet_jit_reader_over", &sig_unary)?,
        reader_read_u8: import("jet_jit_reader_read_u8", &sig_unary)?,
        reader_read_u16_le: import("jet_jit_reader_read_u16_le", &sig_unary)?,
        reader_read_u16_be: import("jet_jit_reader_read_u16_be", &sig_unary)?,
        reader_read_u32_le: import("jet_jit_reader_read_u32_le", &sig_unary)?,
        reader_read_u32_be: import("jet_jit_reader_read_u32_be", &sig_unary)?,
        reader_read_u64_le: import("jet_jit_reader_read_u64_le", &sig_unary)?,
        reader_read_u64_be: import("jet_jit_reader_read_u64_be", &sig_unary)?,
        reader_take: import("jet_jit_reader_take", &sig_binary)?,
        reader_remaining: import("jet_jit_reader_remaining", &sig_unary)?,
        reader_at_end: import("jet_jit_reader_at_end", &sig_i8)?,
        cursor_over: import("jet_jit_cursor_over", &sig_unary)?,
        cursor_skip_ws: import("jet_jit_cursor_skip_ws", &sig_void_unary)?,
        cursor_take_until: import("jet_jit_cursor_take_until", &sig_binary)?,
        cursor_advance: import("jet_jit_cursor_advance", &sig_binary)?,
        str_match_is_some: import("jet_jit_str_match_is_some", &sig_binary_i8)?,
        str_match_unwrap: import("jet_jit_str_match_unwrap", &sig_binary)?,
        bin_match_is_some: import("jet_jit_bin_match_is_some", &sig_binary_i8)?,
        bin_match_unwrap: import("jet_jit_bin_match_unwrap", &sig_binary)?,
        cursor_take_pattern: import("jet_jit_cursor_take_pattern", &sig_binary)?,
        reader_take_pattern: import("jet_jit_reader_take_pattern", &sig_binary)?,
    })
}
