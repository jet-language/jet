//! D-SHIFT1 `binary.Reader` / `text.Cursor` on the canonical TIR evaluator.
//!
//! Marshalling only (I9): a `Reader`/`Cursor` value is carried as a
//! `CtValue::Struct` holding the same `buf`/`pos` pair the AOT struct holds,
//! and every operation decodes it, calls the shared
//! `jet_foundation::StreamCursor` kernel — the very source `Codegen/mod.rs`
//! splices into the emitted prelude — and re-encodes the result. Pattern takes
//! walk the shared `jet_foundation::MatchScan` engine, the same one the
//! Cranelift host uses. No reader semantics live in this file.

use super::unsupported;
use crate::Codegen::TIR::THandleOp;
use crate::Comptime::Builtins::exact_int_value;
use crate::Comptime::CtValue;
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{CtFloat, Type};
use jet_foundation::MatchScan::{
    bin_match_consumed, bin_match_scan, str_match_consumed, str_match_scan, BinBind,
};
use jet_foundation::StreamCursor as kernel;

const READER: &str = "Reader";
const CURSOR: &str = "Cursor";

pub(super) fn eval(
    op: &THandleOp,
    recv: &mut CtValue,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    match op {
        THandleOp::ReaderOver => {
            let bytes = bytes_of(recv).ok_or_else(|| unsupported("Reader.over subject", span))?;
            Ok(reader_ct(&kernel::jet_reader_over(&bytes)))
        }
        THandleOp::ReaderReadU8 => {
            read(recv, span, |r| kernel::jet_reader_read_u8(r).map(i64::from))
        }
        THandleOp::ReaderReadI8 => {
            read(recv, span, |r| kernel::jet_reader_read_i8(r).map(i64::from))
        }
        THandleOp::ReaderReadU16Le => read(recv, span, |r| {
            kernel::jet_reader_read_u16_le(r).map(i64::from)
        }),
        THandleOp::ReaderReadU16Be => read(recv, span, |r| {
            kernel::jet_reader_read_u16_be(r).map(i64::from)
        }),
        THandleOp::ReaderReadI16Le => read(recv, span, |r| {
            kernel::jet_reader_read_i16_le(r).map(i64::from)
        }),
        THandleOp::ReaderReadI16Be => read(recv, span, |r| {
            kernel::jet_reader_read_i16_be(r).map(i64::from)
        }),
        THandleOp::ReaderReadU32Le => read(recv, span, |r| {
            kernel::jet_reader_read_u32_le(r).map(i64::from)
        }),
        THandleOp::ReaderReadU32Be => read(recv, span, |r| {
            kernel::jet_reader_read_u32_be(r).map(i64::from)
        }),
        THandleOp::ReaderReadI32Le => read(recv, span, |r| {
            kernel::jet_reader_read_i32_le(r).map(i64::from)
        }),
        THandleOp::ReaderReadI32Be => read(recv, span, |r| {
            kernel::jet_reader_read_i32_be(r).map(i64::from)
        }),
        // U64 exceeds `Int`'s range; AOT's `as i64` host cast wraps the same way.
        THandleOp::ReaderReadU64Le => read(recv, span, |r| {
            kernel::jet_reader_read_u64_le(r).map(|v| v as i64)
        }),
        THandleOp::ReaderReadU64Be => read(recv, span, |r| {
            kernel::jet_reader_read_u64_be(r).map(|v| v as i64)
        }),
        THandleOp::ReaderReadI64Le => read(recv, span, kernel::jet_reader_read_i64_le),
        THandleOp::ReaderReadI64Be => read(recv, span, kernel::jet_reader_read_i64_be),
        THandleOp::ReaderReadF32Le => read_float(recv, span, |r| {
            kernel::jet_reader_read_f32_le(r).map(CtFloat::f32)
        }),
        THandleOp::ReaderReadF32Be => read_float(recv, span, |r| {
            kernel::jet_reader_read_f32_be(r).map(CtFloat::f32)
        }),
        THandleOp::ReaderReadF64Le => read_float(recv, span, |r| {
            kernel::jet_reader_read_f64_le(r).map(CtFloat::f64)
        }),
        THandleOp::ReaderReadF64Be => read_float(recv, span, |r| {
            kernel::jet_reader_read_f64_be(r).map(CtFloat::f64)
        }),
        THandleOp::ReaderPeek => {
            let r = reader_of(recv).ok_or_else(|| unsupported("Reader receiver", span))?;
            kernel::jet_reader_peek(&r)
                .map(i64::from)
                .map(CtValue::Int)
                .map_err(|message| unsupported(&message, span))
        }
        THandleOp::ReaderSeek | THandleOp::ReaderSkip => {
            let n = match arg(args, 0, span)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("Reader cursor position", span)),
            };
            with_reader(recv, span, |r| {
                let result = match op {
                    THandleOp::ReaderSeek => kernel::jet_reader_seek(r, n),
                    THandleOp::ReaderSkip => kernel::jet_reader_skip(r, n),
                    _ => unreachable!(),
                };
                result.map(|()| CtValue::Unit)
            })
        }
        THandleOp::ReaderTake => {
            let CtValue::Int(n) = arg(args, 0, span)? else {
                return Err(unsupported("Reader.take length", span));
            };
            let n = *n;
            with_reader(recv, span, |r| {
                kernel::jet_reader_take(r, n).map(CtValue::Bytes)
            })
        }
        THandleOp::ReaderRemaining => {
            let r = reader_of(recv).ok_or_else(|| unsupported("Reader receiver", span))?;
            Ok(CtValue::Int(kernel::jet_reader_remaining(&r)))
        }
        THandleOp::ReaderAtEnd => {
            let r = reader_of(recv).ok_or_else(|| unsupported("Reader receiver", span))?;
            Ok(CtValue::Bool(kernel::jet_reader_at_end(&r)))
        }
        THandleOp::CursorOver => {
            let CtValue::Str(text) = recv else {
                return Err(unsupported("Cursor.over subject", span));
            };
            Ok(cursor_ct(&kernel::jet_cursor_over(text)))
        }
        THandleOp::CursorTakeUntil => {
            let CtValue::Str(delim) = arg(args, 0, span)? else {
                return Err(unsupported("Cursor.take_until delimiter", span));
            };
            let delim = delim.clone();
            with_cursor(recv, span, |c| {
                kernel::jet_cursor_take_until(c, &delim).map(CtValue::Str)
            })
        }
        THandleOp::CursorSkipWs => {
            let mut c = cursor_of(recv).ok_or_else(|| unsupported("Cursor receiver", span))?;
            kernel::jet_cursor_skip_ws(&mut c);
            *recv = cursor_ct(&c);
            Ok(CtValue::Unit)
        }
        THandleOp::CursorTakePattern { parts, canonical } => {
            let mut c = cursor_of(recv).ok_or_else(|| unsupported("Cursor receiver", span))?;
            let hit = {
                let tail = kernel::jet_cursor_tail(&c);
                str_match_scan(tail, parts, true)
                    .map(|binds| (str_match_consumed(tail, parts).unwrap_or(0), binds))
            };
            Ok(match hit {
                Some((consumed, binds)) => {
                    kernel::jet_cursor_take_pattern(&mut c, consumed);
                    let value = str_tuple(canonical, &binds);
                    *recv = cursor_ct(&c);
                    CtValue::Present(Box::new(value))
                }
                None => {
                    CtValue::failed(Box::new(CtValue::Str(kernel::jet_cursor_pattern_miss(&c))))
                }
            })
        }
        THandleOp::ReaderTakePattern { parts, canonical } => {
            let mut r = reader_of(recv).ok_or_else(|| unsupported("Reader receiver", span))?;
            let hit = bin_match_scan(kernel::jet_reader_tail(&r), parts, true)
                .and_then(|(bit_pos, binds)| bin_match_consumed(bit_pos).map(|n| (n, binds)));
            Ok(match hit {
                Some((consumed, binds)) => {
                    kernel::jet_reader_take_pattern(&mut r, consumed);
                    let value = bin_tuple(canonical, &binds);
                    *recv = reader_ct(&r);
                    CtValue::Present(Box::new(value))
                }
                None => {
                    CtValue::failed(Box::new(CtValue::Str(kernel::jet_reader_pattern_miss(&r))))
                }
            })
        }
        _ => Err(unsupported("handle `stream`", span)),
    }
}

fn arg(args: &[CtValue], index: usize, span: Span) -> Result<&CtValue, Diagnostic> {
    args.get(index)
        .ok_or_else(|| unsupported("stream handle argument", span))
}

fn bytes_of(value: &CtValue) -> Option<Vec<u8>> {
    match value {
        CtValue::Bytes(bytes) => Some(bytes.clone()),
        CtValue::List(items) => items
            .iter()
            .map(|item| match item {
                CtValue::Int(n) => Some(*n as u8),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn field<'a>(recv: &'a CtValue, want: &str, name: &str) -> Option<&'a CtValue> {
    match recv {
        CtValue::Struct { type_name, fields } if type_name == want => fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value),
        _ => None,
    }
}

fn pos_of(recv: &CtValue, want: &str) -> Option<usize> {
    match field(recv, want, "pos")? {
        CtValue::Int(pos) => Some(*pos as usize),
        _ => None,
    }
}

fn reader_of(recv: &CtValue) -> Option<kernel::JetReader> {
    Some(kernel::JetReader {
        buf: bytes_of(field(recv, READER, "buf")?)?,
        pos: pos_of(recv, READER)?,
    })
}

/// Move the Reader buffer through one evaluator operation instead of cloning
/// it out of and back into `CtValue` on every read.
fn take_reader(recv: &mut CtValue) -> Option<kernel::JetReader> {
    let CtValue::Struct { type_name, fields } = recv else {
        return None;
    };
    if type_name != READER {
        return None;
    }
    let pos = fields
        .iter()
        .find(|(name, _)| name == "pos")
        .and_then(|(_, value)| match value {
            CtValue::Int(pos) => usize::try_from(*pos).ok(),
            _ => None,
        })?;
    let buf =
        fields
            .iter_mut()
            .find(|(name, _)| name == "buf")
            .and_then(|(_, value)| match value {
                CtValue::Bytes(buf) => Some(std::mem::take(buf)),
                _ => None,
            })?;
    Some(kernel::JetReader { buf, pos })
}

fn restore_reader(recv: &mut CtValue, reader: kernel::JetReader) -> Option<()> {
    let CtValue::Struct { type_name, fields } = recv else {
        return None;
    };
    if type_name != READER {
        return None;
    }
    let buf =
        fields
            .iter_mut()
            .find(|(name, _)| name == "buf")
            .and_then(|(_, value)| match value {
                CtValue::Bytes(buf) => Some(buf),
                _ => None,
            })?;
    *buf = reader.buf;
    let pos =
        fields
            .iter_mut()
            .find(|(name, _)| name == "pos")
            .and_then(|(_, value)| match value {
                CtValue::Int(pos) => Some(pos),
                _ => None,
            })?;
    *pos = i64::try_from(reader.pos).ok()?;
    Some(())
}

fn reader_ct(r: &kernel::JetReader) -> CtValue {
    CtValue::Struct {
        type_name: READER.to_string(),
        fields: vec![
            ("buf".to_string(), CtValue::Bytes(r.buf.clone())),
            ("pos".to_string(), CtValue::Int(r.pos as i64)),
        ],
    }
}

fn cursor_of(recv: &CtValue) -> Option<kernel::JetCursor> {
    let CtValue::Str(buf) = field(recv, CURSOR, "buf")? else {
        return None;
    };
    Some(kernel::JetCursor {
        buf: buf.clone(),
        pos: pos_of(recv, CURSOR)?,
    })
}

fn cursor_ct(c: &kernel::JetCursor) -> CtValue {
    CtValue::Struct {
        type_name: CURSOR.to_string(),
        fields: vec![
            ("buf".to_string(), CtValue::Str(c.buf.clone())),
            ("pos".to_string(), CtValue::Int(c.pos as i64)),
        ],
    }
}

/// Run a fallible reader read, writing the advanced position back.
fn read(
    recv: &mut CtValue,
    span: Span,
    call: impl FnOnce(&mut kernel::JetReader) -> Result<i64, String>,
) -> Result<CtValue, Diagnostic> {
    with_reader(recv, span, |r| call(r).map(CtValue::Int))
}

fn read_float(
    recv: &mut CtValue,
    span: Span,
    call: impl FnOnce(&mut kernel::JetReader) -> Result<CtFloat, String>,
) -> Result<CtValue, Diagnostic> {
    with_reader(recv, span, |r| call(r).map(CtValue::Float))
}

fn with_reader(
    recv: &mut CtValue,
    span: Span,
    call: impl FnOnce(&mut kernel::JetReader) -> Result<CtValue, String>,
) -> Result<CtValue, Diagnostic> {
    let mut reader = take_reader(recv).ok_or_else(|| unsupported("Reader receiver", span))?;
    let out = call(&mut reader);
    restore_reader(recv, reader).ok_or_else(|| unsupported("Reader receiver storage", span))?;
    Ok(result_ct(out))
}

fn with_cursor(
    recv: &mut CtValue,
    span: Span,
    call: impl FnOnce(&mut kernel::JetCursor) -> Result<CtValue, String>,
) -> Result<CtValue, Diagnostic> {
    let mut c = cursor_of(recv).ok_or_else(|| unsupported("Cursor receiver", span))?;
    let out = call(&mut c);
    *recv = cursor_ct(&c);
    Ok(result_ct(out))
}

fn result_ct(out: Result<CtValue, String>) -> CtValue {
    match out {
        Ok(value) => CtValue::Present(Box::new(value)),
        Err(message) => CtValue::failed(Box::new(CtValue::Str(message))),
    }
}

/// The `JetTup_<hash>` record AOT builds for a successful take, or `Unit` for a
/// hole-free pattern.
fn tuple_ct(canonical: &[(String, Type)], values: Vec<CtValue>) -> CtValue {
    if canonical.is_empty() {
        return CtValue::Unit;
    }
    CtValue::Struct {
        type_name: crate::Codegen::Tuples::tuple_struct_name(canonical),
        fields: canonical
            .iter()
            .map(|(name, _)| name.clone())
            .zip(values)
            .collect(),
    }
}

fn str_tuple(canonical: &[(String, Type)], binds: &[(String, Type, String)]) -> CtValue {
    let values = canonical
        .iter()
        .zip(binds.iter())
        .map(|((_, ty), (_, _, raw))| match ty {
            Type::Int => exact_int_value(
                jet_foundation::Numeric::CtBigInt::from_str(raw)
                    .unwrap_or_else(|_| jet_foundation::Numeric::CtBigInt::from_int(0)),
            ),
            Type::IntN { .. } => CtValue::Int(raw.parse::<i64>().unwrap_or(0)),
            Type::InlineRange { .. } => CtValue::Int(raw.parse::<i64>().unwrap_or(0)),
            Type::Float | Type::Float32 => {
                CtValue::Float(CtFloat::f64(raw.parse::<f64>().unwrap_or(0.0)))
            }
            Type::Bool => CtValue::Bool(matches!(raw.as_str(), "true" | "True" | "1")),
            _ => CtValue::Str(raw.clone()),
        })
        .collect();
    tuple_ct(canonical, values)
}

fn bin_tuple(canonical: &[(String, Type)], binds: &[(String, Type, BinBind)]) -> CtValue {
    let values = binds
        .iter()
        .map(|(_, _, bind)| match bind {
            BinBind::Int(value) => CtValue::Int(*value),
            BinBind::Rest(bytes) => CtValue::Bytes(bytes.clone()),
        })
        .collect();
    tuple_ct(canonical, values)
}
