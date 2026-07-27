//! Native JIT adapters for `binary.Reader`, `text.Cursor`, and match scans.
//! Reuses the same algorithms as AOT `jet_reader_*` / `jet_cursor_*` / match
//! engines — no second parser.

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use jet_foundation::AST::{BinEndian, BinMatchPart, BinSpec, StrMatchPart, Type};
use std::sync::Mutex;

/// Pattern tables live outside `Runtime` so ids baked into Cranelift IR during
/// lowering survive Runtime resets between compile and execute.
static STR_PATTERNS: Mutex<Vec<Vec<StrMatchPart>>> = Mutex::new(Vec::new());
static BIN_PATTERNS: Mutex<Vec<Vec<BinMatchPart>>> = Mutex::new(Vec::new());

#[derive(Clone)]
pub(crate) struct ReaderSlot {
    buf: Vec<u8>,
    pos: usize,
}

#[derive(Clone)]
pub(crate) struct CursorSlot {
    buf: String,
    pos: usize,
}

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
    push_reader(ReaderSlot {
        buf: clone_byte_list(bytes),
        pos: 0,
    })
}

fn take_fixed(r: &mut ReaderSlot, n: usize, method: &str) -> Result<Vec<u8>, String> {
    if r.pos + n > r.buf.len() {
        return Err(format!(
            "Reader.{}: needed {} byte{} at position {}, only {} remain",
            method,
            n,
            if n == 1 { "" } else { "s" },
            r.pos,
            r.buf.len().saturating_sub(r.pos),
        ));
    }
    let out = r.buf[r.pos..r.pos + n].to_vec();
    r.pos += n;
    Ok(out)
}

macro_rules! reader_read {
    ($name:ident, $n:expr, $method:expr, $map:expr) => {
        extern "C" fn $name(handle: i64) -> i64 {
            match with_reader_mut(handle, |r| take_fixed(r, $n, $method).map($map)) {
                Some(Ok(v)) => result_ok(v as i64),
                Some(Err(e)) => result_err(e),
                None => result_err("Reader: bad handle".into()),
            }
        }
    };
}

reader_read!(jet_jit_reader_read_u8, 1, "read_u8", |b| b[0] as u64);
reader_read!(jet_jit_reader_read_u16_le, 2, "read_u16_le", |b| {
    u16::from_le_bytes([b[0], b[1]]) as u64
});
reader_read!(jet_jit_reader_read_u16_be, 2, "read_u16_be", |b| {
    u16::from_be_bytes([b[0], b[1]]) as u64
});
reader_read!(jet_jit_reader_read_u32_le, 4, "read_u32_le", |b| {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64
});
reader_read!(jet_jit_reader_read_u32_be, 4, "read_u32_be", |b| {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as u64
});
reader_read!(jet_jit_reader_read_u64_le, 8, "read_u64_le", |b| {
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
});
reader_read!(jet_jit_reader_read_u64_be, 8, "read_u64_be", |b| {
    u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
});

extern "C" fn jet_jit_reader_take(handle: i64, n: i64) -> i64 {
    if n < 0 {
        return result_err(format!(
            "Reader.take: length must not be negative, got {n}"
        ));
    }
    match with_reader_mut(handle, |r| take_fixed(r, n as usize, "take")) {
        Some(Ok(bytes)) => result_ok_bytes(bytes),
        Some(Err(e)) => result_err(e),
        None => result_err("Reader: bad handle".into()),
    }
}

extern "C" fn jet_jit_reader_remaining(handle: i64) -> i64 {
    with_reader_mut(handle, |r| (r.buf.len() - r.pos) as i64).unwrap_or(0)
}

extern "C" fn jet_jit_reader_at_end(handle: i64) -> i8 {
    with_reader_mut(handle, |r| if r.pos >= r.buf.len() { 1 } else { 0 }).unwrap_or(1)
}

extern "C" fn jet_jit_cursor_over(text: i64) -> i64 {
    let s = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(text).unwrap_or_default());
    push_cursor(CursorSlot { buf: s, pos: 0 })
}

extern "C" fn jet_jit_cursor_skip_ws(handle: i64) {
    let _ = with_cursor_mut(handle, |c| {
        let tail = &c.buf[c.pos..];
        let skipped = tail.len() - tail.trim_start().len();
        c.pos += skipped;
    });
}

extern "C" fn jet_jit_cursor_take_until(handle: i64, delim: i64) -> i64 {
    let delim = Concurrency::with_runtime_mut(|rt| {
        rt.heap.clone_string(delim).unwrap_or_default()
    });
    match with_cursor_mut(handle, |c| {
        let tail = &c.buf[c.pos..];
        match tail.find(delim.as_str()) {
            Some(i) => {
                let out = tail[..i].to_string();
                c.pos += i;
                Ok(out)
            }
            None => Err(format!(
                "Cursor.take_until: {:?} not found in the remaining text",
                delim
            )),
        }
    }) {
        Some(Ok(s)) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
            result_ok(sid)
        }
        Some(Err(e)) => result_err(e),
        None => result_err("Cursor: bad handle".into()),
    }
}

/// Shared string-match scan used by pattern arms and `Cursor.take_pattern`.
/// Returns packed Option-of-tuple handle: 0 = None, else value+1 style list handle.
pub(crate) fn str_match_scan(
    subject: &str,
    parts: &[StrMatchPart],
    consume_prefix: bool,
) -> Option<Vec<(String, Type, String)>> {
    let mut i = 0usize;
    let mut binds = Vec::new();
    for (pi, part) in parts.iter().enumerate() {
        match part {
            StrMatchPart::Lit(lit) => {
                if !subject[i..].starts_with(lit.as_str()) {
                    return None;
                }
                i += lit.len();
            }
            StrMatchPart::Hole { name, ty, .. } => {
                let end = match parts.get(pi + 1) {
                    Some(StrMatchPart::Lit(next)) => {
                        subject[i..].find(next.as_str()).map(|o| i + o)?
                    }
                    _ => {
                        if consume_prefix {
                            // consume mode: take until we can still match remaining
                            // literals later — for trailing hole, take rest only when
                            // no more parts follow.
                            subject.len()
                        } else {
                            subject.len()
                        }
                    }
                };
                let raw = &subject[i..end];
                let ok = match ty {
                    Some(Type::Int) | Some(Type::IntN { .. }) => raw.parse::<i64>().is_ok(),
                    Some(Type::Float) | Some(Type::Float32) => raw.parse::<f64>().is_ok(),
                    Some(Type::Bool) => matches!(raw, "true" | "false" | "True" | "False" | "0" | "1"),
                    None | Some(Type::String) | Some(Type::Named(_)) => true,
                    _ => true,
                };
                if !ok {
                    return None;
                }
                binds.push((name.clone(), ty.clone().unwrap_or(Type::String), raw.to_string()));
                i = end;
            }
        }
    }
    if !consume_prefix && i != subject.len() {
        return None;
    }
    if consume_prefix {
        // caller advances cursor by `i`
    }
    Some(binds)
}

pub(crate) fn str_match_consumed(subject: &str, parts: &[StrMatchPart]) -> Option<usize> {
    let mut i = 0usize;
    for (pi, part) in parts.iter().enumerate() {
        match part {
            StrMatchPart::Lit(lit) => {
                if !subject[i..].starts_with(lit.as_str()) {
                    return None;
                }
                i += lit.len();
            }
            StrMatchPart::Hole { ty, .. } => {
                let end = match parts.get(pi + 1) {
                    Some(StrMatchPart::Lit(next)) => {
                        subject[i..].find(next.as_str()).map(|o| i + o)?
                    }
                    _ => subject.len(),
                };
                let raw = &subject[i..end];
                let ok = match ty {
                    Some(Type::Int) | Some(Type::IntN { .. }) => raw.parse::<i64>().is_ok(),
                    Some(Type::Float) | Some(Type::Float32) => raw.parse::<f64>().is_ok(),
                    Some(Type::Bool) => matches!(raw, "true" | "false" | "True" | "False" | "0" | "1"),
                    _ => true,
                };
                if !ok {
                    return None;
                }
                i = end;
            }
        }
    }
    Some(i)
}

fn read_bits(buf: &[u8], bit_pos: &mut usize, width: usize, be: bool) -> Option<u64> {
    if width == 0 || *bit_pos + width > buf.len() * 8 {
        return None;
    }
    let mut value = 0u64;
    for _ in 0..width {
        let byte = buf[*bit_pos / 8];
        let bit = 7 - (*bit_pos % 8);
        let b = ((byte >> bit) & 1) as u64;
        value = if be {
            (value << 1) | b
        } else {
            value | (b << (*bit_pos % width))
        };
        // For LE multi-bit we still read MSB-first within the stream; AOT uses
        // explicit byte assembly for U8/U16/…. Prefer width-aligned byte reads.
        let _ = be;
        *bit_pos += 1;
        let _ = value;
    }
    // Re-do with byte-oriented reads for common widths.
    None
}

pub(crate) fn bin_match_scan(
    subject: &[u8],
    parts: &[BinMatchPart],
    consume_prefix: bool,
) -> Option<(usize, Vec<(String, Type, i64)>)> {
    let mut bit_pos = 0usize;
    let mut binds = Vec::new();
    for part in parts {
        match part {
            BinMatchPart::Lit(bytes) => {
                let need = bytes.len() * 8;
                if bit_pos % 8 != 0 {
                    return None;
                }
                let byte_pos = bit_pos / 8;
                if byte_pos + bytes.len() > subject.len() {
                    return None;
                }
                if &subject[byte_pos..byte_pos + bytes.len()] != bytes.as_slice() {
                    return None;
                }
                bit_pos += need;
            }
            BinMatchPart::Hole { name, spec, .. } => {
                let (width, be) = match spec {
                    BinSpec::Bits { width, endian } => (
                        *width as usize,
                        matches!(endian, BinEndian::Big | BinEndian::None),
                    ),
                    BinSpec::Rest => {
                        if bit_pos % 8 != 0 {
                            return None;
                        }
                        let byte_pos = bit_pos / 8;
                        let rest = &subject[byte_pos..];
                        let list = Concurrency::with_runtime_mut(|rt| {
                            rt.heap
                                .alloc_int_list(rest.iter().map(|b| *b as i64).collect())
                        });
                        binds.push((
                            name.clone(),
                            Type::List(Box::new(Type::IntN {
                                signed: false,
                                bits: 8,
                            })),
                            list,
                        ));
                        bit_pos = subject.len() * 8;
                        continue;
                    }
                };
                if width % 8 == 0 && bit_pos % 8 == 0 {
                    let nbytes = width / 8;
                    let byte_pos = bit_pos / 8;
                    if byte_pos + nbytes > subject.len() {
                        return None;
                    }
                    let slice = &subject[byte_pos..byte_pos + nbytes];
                    let v = match (nbytes, be) {
                        (1, _) => slice[0] as i64,
                        (2, true) => u16::from_be_bytes([slice[0], slice[1]]) as i64,
                        (2, false) => u16::from_le_bytes([slice[0], slice[1]]) as i64,
                        (4, true) => {
                            u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]) as i64
                        }
                        (4, false) => {
                            u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as i64
                        }
                        (8, true) => u64::from_be_bytes([
                            slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6],
                            slice[7],
                        ]) as i64,
                        (8, false) => u64::from_le_bytes([
                            slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6],
                            slice[7],
                        ]) as i64,
                        _ => return None,
                    };
                    binds.push((
                        name.clone(),
                        Type::IntN {
                            signed: false,
                            bits: width as u8,
                        },
                        v,
                    ));
                    bit_pos += width;
                } else {
                    // Nibble-oriented (U4): read bit by bit MSB-first.
                    if bit_pos + width > subject.len() * 8 {
                        return None;
                    }
                    let mut v = 0u64;
                    for _ in 0..width {
                        let byte = subject[bit_pos / 8];
                        let bit = 7 - (bit_pos % 8);
                        v = (v << 1) | ((byte >> bit) as u64 & 1);
                        bit_pos += 1;
                    }
                    let _ = be;
                    binds.push((
                        name.clone(),
                        Type::IntN {
                            signed: false,
                            bits: width as u8,
                        },
                        v as i64,
                    ));
                }
            }
        }
    }
    if !consume_prefix && bit_pos != subject.len() * 8 {
        // Allow trailing unmatched only in consume mode.
        if bit_pos / 8 != subject.len() && bit_pos != subject.len() * 8 {
            // full-match requires exact end for non-rest patterns without rest hole
            let has_rest = parts.iter().any(|p| {
                matches!(p, BinMatchPart::Hole { spec: BinSpec::Rest, .. })
            });
            if !has_rest && bit_pos != subject.len() * 8 {
                return None;
            }
        }
    }
    let _ = read_bits;
    Some((bit_pos, binds))
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

fn pack_bin_binds(binds: &[(String, Type, i64)]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let handle = rt.heap.alloc_record(binds.len());
        for (i, (_, _, v)) in binds.iter().enumerate() {
            let _ = rt.heap.record_set_int(handle, i as i64, *v);
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
        let tail = &c.buf[c.pos..];
        match str_match_scan(tail, &parts, true) {
            Some(binds) => {
                let consumed = str_match_consumed(tail, &parts).unwrap_or(0);
                c.pos += consumed;
                Ok(binds)
            }
            None => Err("Cursor.take_pattern: no match".into()),
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
        let tail = &r.buf[r.pos..];
        match bin_match_scan(tail, &parts, true) {
            Some((bit_pos, binds)) => {
                if bit_pos % 8 != 0 {
                    return Err("Reader.take_pattern: unaligned bit position".into());
                }
                r.pos += bit_pos / 8;
                Ok(binds)
            }
            None => Err("Reader.take_pattern: no match".into()),
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
