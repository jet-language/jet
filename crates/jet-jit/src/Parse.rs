//! Native JIT adapters for `binary.Reader`, `text.Cursor`, and match scans.
//! Marshalling only: handles in, heap values out. The semantics come from the
//! shared kernels in `jet_foundation::StreamCursor` (spliced verbatim into the
//! AOT prelude) and `jet_foundation::MatchScan` — no second parser.

use super::Concurrency;
use crate::Marshal::result_err_msg;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_module::Module;
use jet_foundation::StreamCursor as kernel;

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
    crate::Marshal::result_ok(bits as u64)
}

fn result_err(msg: String) -> i64 {
    result_err_msg(&msg)
}

fn result_ok_bytes(bytes: Vec<u8>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt
            .heap
            .alloc_int_list(bytes.into_iter().map(|b| b as i64).collect());
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

fn jet_jit_reader_over(bytes: i64) -> i64 {
    push_reader(kernel::jet_reader_over(&clone_byte_list(bytes)))
}

macro_rules! reader_read {
    ($name:ident, $kernel:path) => {
        fn $name(handle: i64) -> i64 {
            match with_reader_mut(handle, |r| $kernel(r).map(|v| v as i64)) {
                Some(Ok(v)) => result_ok(v),
                Some(Err(e)) => result_err(e),
                None => result_err("Reader: bad handle".into()),
            }
        }
    };
}

reader_read!(jet_jit_reader_read_u8, kernel::jet_reader_read_u8);
reader_read!(jet_jit_reader_read_i8, kernel::jet_reader_read_i8);
reader_read!(jet_jit_reader_read_u16_le, kernel::jet_reader_read_u16_le);
reader_read!(jet_jit_reader_read_u16_be, kernel::jet_reader_read_u16_be);
reader_read!(jet_jit_reader_read_i16_le, kernel::jet_reader_read_i16_le);
reader_read!(jet_jit_reader_read_i16_be, kernel::jet_reader_read_i16_be);
reader_read!(jet_jit_reader_read_u32_le, kernel::jet_reader_read_u32_le);
reader_read!(jet_jit_reader_read_u32_be, kernel::jet_reader_read_u32_be);
reader_read!(jet_jit_reader_read_i32_le, kernel::jet_reader_read_i32_le);
reader_read!(jet_jit_reader_read_i32_be, kernel::jet_reader_read_i32_be);
reader_read!(jet_jit_reader_read_u64_le, kernel::jet_reader_read_u64_le);
reader_read!(jet_jit_reader_read_u64_be, kernel::jet_reader_read_u64_be);
reader_read!(jet_jit_reader_read_i64_le, kernel::jet_reader_read_i64_le);
reader_read!(jet_jit_reader_read_i64_be, kernel::jet_reader_read_i64_be);

macro_rules! reader_read_float {
    ($name:ident, $kernel:path, $convert:expr) => {
        fn $name(handle: i64) -> i64 {
            match with_reader_mut(handle, |r| $kernel(r).map(|v| $convert(v) as i64)) {
                Some(Ok(v)) => result_ok(v),
                Some(Err(e)) => result_err(e),
                None => result_err("Reader: bad handle".into()),
            }
        }
    };
}

reader_read_float!(
    jet_jit_reader_read_f32_le,
    kernel::jet_reader_read_f32_le,
    |v: f32| (v as f64).to_bits()
);
reader_read_float!(
    jet_jit_reader_read_f32_be,
    kernel::jet_reader_read_f32_be,
    |v: f32| (v as f64).to_bits()
);
reader_read_float!(
    jet_jit_reader_read_f64_le,
    kernel::jet_reader_read_f64_le,
    |v: f64| v.to_bits()
);
reader_read_float!(
    jet_jit_reader_read_f64_be,
    kernel::jet_reader_read_f64_be,
    |v: f64| v.to_bits()
);

fn jet_jit_reader_peek(handle: i64) -> i64 {
    match with_reader_mut(handle, |r| kernel::jet_reader_peek(r)) {
        Some(Ok(value)) => result_ok(i64::from(value)),
        Some(Err(e)) => result_err(e),
        None => result_err("Reader: bad handle".into()),
    }
}

fn jet_jit_reader_seek(handle: i64, position: i64) -> i64 {
    match with_reader_mut(handle, |r| kernel::jet_reader_seek(r, position)) {
        Some(Ok(())) => result_ok(0),
        Some(Err(e)) => result_err(e),
        None => result_err("Reader: bad handle".into()),
    }
}

fn jet_jit_reader_skip(handle: i64, count: i64) -> i64 {
    match with_reader_mut(handle, |r| kernel::jet_reader_skip(r, count)) {
        Some(Ok(())) => result_ok(0),
        Some(Err(e)) => result_err(e),
        None => result_err("Reader: bad handle".into()),
    }
}

fn jet_jit_reader_take(handle: i64, n: i64) -> i64 {
    match with_reader_mut(handle, |r| kernel::jet_reader_take(r, n)) {
        Some(Ok(bytes)) => result_ok_bytes(bytes),
        Some(Err(e)) => result_err(e),
        None => result_err("Reader: bad handle".into()),
    }
}

fn jet_jit_reader_remaining(handle: i64) -> i64 {
    with_reader_mut(handle, |r| kernel::jet_reader_remaining(r)).unwrap_or(0)
}

fn jet_jit_reader_at_end(handle: i64) -> i8 {
    with_reader_mut(handle, |r| i8::from(kernel::jet_reader_at_end(r))).unwrap_or(1)
}

fn jet_jit_cursor_over(text: i64) -> i64 {
    let s = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(text).unwrap_or_default());
    push_cursor(kernel::jet_cursor_over(&s))
}

fn jet_jit_cursor_skip_ws(handle: i64) {
    let _ = with_cursor_mut(handle, kernel::jet_cursor_skip_ws);
}

fn jet_jit_cursor_take_until(handle: i64, delim: i64) -> i64 {
    let delim = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(delim).unwrap_or_default());
    match with_cursor_mut(handle, |c| kernel::jet_cursor_take_until(c, &delim)) {
        Some(Ok(s)) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
            result_ok(sid)
        }
        Some(Err(e)) => result_err(e),
        None => result_err("Cursor: bad handle".into()),
    }
}

fn jet_jit_cursor_advance(handle: i64, nbytes: i64) {
    let _ = with_cursor_mut(handle, |c| {
        c.pos = (c.pos + nbytes as usize).min(c.buf.len());
    });
}

host_fns! {
    struct HostFns;
    register: register_symbols;
    declare: declare(module) {
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


    }
    reader_over: "jet_jit_reader_over" => jet_jit_reader_over: sig_unary;
    reader_over_owned: "jet_reader_over_owned" => jet_jit_reader_over: sig_unary;
    reader_read_u8: "jet_jit_reader_read_u8" => jet_jit_reader_read_u8: sig_unary;
    reader_read_i8: "jet_jit_reader_read_i8" => jet_jit_reader_read_i8: sig_unary;
    reader_read_u16_le: "jet_jit_reader_read_u16_le" => jet_jit_reader_read_u16_le: sig_unary;
    reader_read_u16_be: "jet_jit_reader_read_u16_be" => jet_jit_reader_read_u16_be: sig_unary;
    reader_read_i16_le: "jet_jit_reader_read_i16_le" => jet_jit_reader_read_i16_le: sig_unary;
    reader_read_i16_be: "jet_jit_reader_read_i16_be" => jet_jit_reader_read_i16_be: sig_unary;
    reader_read_u32_le: "jet_jit_reader_read_u32_le" => jet_jit_reader_read_u32_le: sig_unary;
    reader_read_u32_be: "jet_jit_reader_read_u32_be" => jet_jit_reader_read_u32_be: sig_unary;
    reader_read_i32_le: "jet_jit_reader_read_i32_le" => jet_jit_reader_read_i32_le: sig_unary;
    reader_read_i32_be: "jet_jit_reader_read_i32_be" => jet_jit_reader_read_i32_be: sig_unary;
    reader_read_u64_le: "jet_jit_reader_read_u64_le" => jet_jit_reader_read_u64_le: sig_unary;
    reader_read_u64_be: "jet_jit_reader_read_u64_be" => jet_jit_reader_read_u64_be: sig_unary;
    reader_read_i64_le: "jet_jit_reader_read_i64_le" => jet_jit_reader_read_i64_le: sig_unary;
    reader_read_i64_be: "jet_jit_reader_read_i64_be" => jet_jit_reader_read_i64_be: sig_unary;
    reader_read_f32_le: "jet_jit_reader_read_f32_le" => jet_jit_reader_read_f32_le: sig_unary;
    reader_read_f32_be: "jet_jit_reader_read_f32_be" => jet_jit_reader_read_f32_be: sig_unary;
    reader_read_f64_le: "jet_jit_reader_read_f64_le" => jet_jit_reader_read_f64_le: sig_unary;
    reader_read_f64_be: "jet_jit_reader_read_f64_be" => jet_jit_reader_read_f64_be: sig_unary;
    reader_peek: "jet_jit_reader_peek" => jet_jit_reader_peek: sig_unary;
    reader_seek: "jet_jit_reader_seek" => jet_jit_reader_seek: sig_binary;
    reader_skip: "jet_jit_reader_skip" => jet_jit_reader_skip: sig_binary;
    reader_take: "jet_jit_reader_take" => jet_jit_reader_take: sig_binary;
    reader_remaining: "jet_jit_reader_remaining" => jet_jit_reader_remaining: sig_unary;
    reader_at_end: "jet_jit_reader_at_end" => jet_jit_reader_at_end: sig_i8;
    cursor_over: "jet_jit_cursor_over" => jet_jit_cursor_over: sig_unary;
    cursor_skip_ws: "jet_jit_cursor_skip_ws" => jet_jit_cursor_skip_ws: sig_void_unary;
    cursor_take_until: "jet_jit_cursor_take_until" => jet_jit_cursor_take_until: sig_binary;
    cursor_advance: "jet_jit_cursor_advance" => jet_jit_cursor_advance: sig_binary;
}
