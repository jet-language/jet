//! `core.encoding.hex` / `base64` / `base32` / `csv` / `json` and `core.crypto.uuid`
//! host shims (#729). Encode mirrors `jet_std_*` in EncodingCodecs.rs; decode
//! calls `jet_foundation::base_encoding_dispatch` (no third algorithm). CSV
//! mirrors `jet_ring_csv_parse` / `jet_ring_csv_render`. JSON parse/render
//! `include!` the canonical `jet_std` parser. UUID mirrors `jet_std_uuid_*`.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use super::{runtime_host, Concurrency};
use crate::Marshal::{alloc_byte_list, clone_bytes, clone_string, result_err_msg, result_ok};
use crate::Time::TimeValue;
use crate::{JetDebug, JetDisplay, JetShow};
use jet_foundation::AST::{CtKey, CtReport, CtValue, Type};
use jet_foundation::base_encoding_dispatch;
use jet_foundation::Diagnostics::{Diagnostic, Span};
use jet_foundation::PackageEdition;
use jet_foundation::CborKernel::{self, Value};
use jet_foundation::Shape::ShapeProjectionKind;
use jet_rt::JetVal;
// The Foundation carrier's tier-side accessors are a trait defined by the
// included Prelude `DataTree.rs`; the host adapters below call them by name.
use self::json_rt::JetDataTreeAccess;

mod encoding_base_rt {
    include!("../../jet-codegen/src/Prelude/Core/EncodingBase.rs");
}

mod encoding_error_rt {
    include!("../../jet-codegen/src/Prelude/Core/EncodingError.rs");
}


mod field_error_rt {
    include!("../../jet-codegen/src/Prelude/Core/FieldError.rs");
}

mod inline_range_rt {
    include!("../../jet-codegen/src/Prelude/Core/InlineRange.rs");
}

// D-CONFIG-ENV1 / I9: source collection is the same Prelude fragment used by
// generated AOT programs and the whole-program interpreter. This module only
// supplies the resident heap adapter below.
mod env_config_rt {
    include!("../../jet-codegen/src/Prelude/Core/EnvConfig.rs");
}

pub(crate) mod codec_rt {
    pub(crate) mod jet_std {
        pub(crate) type JetDecimal = jet_foundation::Numeric::CtDecimal;
    }
    include!("../../jet-codegen/src/Prelude/Core/Codec.rs");
}

pub(crate) const CODEC_KIND_DATE: i64 = 0;
pub(crate) const CODEC_KIND_LOCAL_DATE: i64 = 1;
pub(crate) const CODEC_KIND_LOCAL_TIME: i64 = 2;
pub(crate) const CODEC_KIND_DATETIME: i64 = 3;
pub(crate) const CODEC_KIND_DURATION: i64 = 4;
pub(crate) const CODEC_KIND_DECIMAL: i64 = 5;
pub(crate) const CSV_DECODE_INT: i64 = 0;
pub(crate) const CSV_DECODE_FLOAT: i64 = 1;
pub(crate) const CSV_DECODE_BOOL: i64 = 2;
pub(crate) const CSV_DECODE_STRING: i64 = 3;
pub(crate) const CSV_DECODE_CHAR: i64 = 4;
pub(crate) const CSV_DECODE_DATATREE: i64 = 5;
pub(crate) const CSV_DECODE_DATE: i64 = 10;
pub(crate) const CSV_DECODE_LOCAL_DATE: i64 = 11;
pub(crate) const CSV_DECODE_LOCAL_TIME: i64 = 12;
pub(crate) const CSV_DECODE_DATETIME: i64 = 13;
pub(crate) const CSV_DECODE_DURATION: i64 = 14;
pub(crate) const CSV_DECODE_DECIMAL: i64 = 15;


/// Canonical `jet_std` JSON/DataTree runtime — adapter types, shared algorithm via include!
pub(crate) mod json_rt {
    pub(crate) use jet_foundation::Shape::ShapeProjection;
    pub use jet_foundation::DataTree::DataTree;
    // AOT places the Foundation JSON kernel at the program root; the
    // DataTree/TOML renderers name its quoting entry unqualified.
    use crate::jet_encoding_json::quote_json;


    // JSON.rs starts with `io_error_at`; provide the IO surface it names.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum IOOperation {
        Read,
        Write,
        Flush,
        Connect,
        Accept,
        Close,
        Resolve,
        Codec,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct IOContext {
        pub operation: IOOperation,
        pub resource: Option<String>,
        pub os_code: Option<i64>,
        pub cause: Option<String>,
    }

    impl IOContext {
        pub fn new(
            operation: IOOperation,
            resource: Option<String>,
            os_code: Option<i64>,
            cause: Option<String>,
        ) -> Self {
            Self {
                operation,
                resource,
                os_code,
                cause,
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum IOError {
        InvalidInput(IOContext),
        NotFound(IOContext),
        PermissionDenied(IOContext),
        TimedOut(IOContext),
        Cancelled(IOContext),
        Closed(IOContext),
        Protocol(IOContext),
        Other(IOContext),
    }

    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/WireOrder.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/DataTreeKind.rs");
    // The one FieldError projection (I9), included beside its caller so
    // `DataTree.rs`'s `JetShow`/`JetDisplay` impls resolve it here too.
    include!("../../jet-codegen/src/Prelude/Core/FieldError.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/DataTree.rs");
    jet_datatree_decode_helpers!();
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/EncodingTypes.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/JSON.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/TOML.rs");

    /// JIT marshalling adapter for the Prelude exact-Int decoder.
    pub fn jet_int_from_i64(value: i64) -> i64 {
        crate::Concurrency::with_runtime_mut(|rt| rt.heap.int_from_i64(value))
    }

    pub fn jet_int_from_str(value: &str) -> Result<i64, String> {
        // `with_runtime_mut` needs a `Default` return, and a `Result` has none:
        // carry the fallible answer in an `Option` so "no resident runtime" is
        // reported instead of silently reading as a parse failure.
        crate::Concurrency::with_runtime_mut(|rt| Some(rt.heap.int_from_str(value)))
            .unwrap_or_else(|| Err("no resident runtime to hold an exact Int".to_string()))
    }

    pub fn jet_int_to_i64(value: i64) -> Option<i64> {
        crate::Concurrency::with_runtime_mut(|rt| rt.heap.int_to_i64(value))
    }

    pub fn jet_int_to_i128(value: i64) -> Option<i128> {
        crate::Concurrency::with_runtime_mut(|rt| rt.heap.int_to_i128(value))
    }

    pub fn jet_int_to_string(value: i64) -> String {
        crate::Concurrency::with_runtime_mut(|rt| rt.heap.int_to_string(value))
    }

    pub fn jet_int_to_f64(value: i64) -> f64 {
        crate::Concurrency::with_runtime_mut(|rt| rt.heap.int_to_f64(value))
    }

    pub fn parse_datatree(text: &str) -> Result<DataTree, EncodingError> {
        parse_json_datatree(text)
    }

    pub fn parse_datatree_ordered(text: &str) -> Result<DataTree, EncodingError> {
        parse_json_datatree(text)
    }

    pub fn parse_datatree_typed_ordered(text: &str) -> Result<DataTree, EncodingError> {
        parse_json_typed_datatree(text)
    }

    /// D-JSON3 lenient decode. The walk, the coercion message and the audit-line
    /// shape are ONE policy in the included `JSONDataTree.rs`; this host used to
    /// carry a byte-equivalent `coerce_walk` copy self-documented as "same as"
    /// AOT's (I8/I9). Only the sink is per-tier, and picking it is the resident
    /// output adapter's job — a raw `eprintln!` would escape `ProgramOutput`,
    /// and a raw buffer push would escape the program's own output order.
    pub fn decode_lenient(text: &str) -> Result<DataTree, EncodingError> {
        jet_std_json_decode_lenient(text, &mut |line| {
            let _ = crate::runtime_host::write_jit_stderr(
                &crate::IO::term_prelude::jet_term_print_frame(&line),
                false,
            );
        })
    }
}
/// Shared CSV/data query kernel. The resident layer only marshals heap
/// `DataTree` values; plan parsing and comparison stay in the Prelude.
pub(crate) mod data_query_rt {
    pub(crate) mod jet_std {
        pub(crate) use super::super::json_rt::{
            datatree_get, render_datatree_json, DataTree, FieldError,
        };
        // `DataSchema::infer` / `DataFormat` from CommonTypes.rs, cut by
        // build.rs (`write_data_schema_std`).
        include!(concat!(env!("OUT_DIR"), "/data_schema_std.rs"));
    }
    include!("../../jet-codegen/src/Prelude/Core/LazyTablePlan.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/DataQuery.rs");
}


/// Canonical YAML via build.rs-stripped include (trailing prelude brace removed).
mod yaml_rt {
    use super::json_rt::DataTree;
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!(concat!(env!("OUT_DIR"), "/yaml_std.rs"));
}

/// DataTree heap ABI: record `[disc:i64, payload:i64]` (Float payload = to_bits).
/// Discriminants come from the Prelude enum declaration at build time.
const DT_NULL: i64 = crate::types_meta::PRELUDE_DATATREE_NULL;
const DT_BOOL: i64 = crate::types_meta::PRELUDE_DATATREE_BOOL;
const DT_INT: i64 = crate::types_meta::PRELUDE_DATATREE_INT;
const DT_FLOAT: i64 = crate::types_meta::PRELUDE_DATATREE_FLOAT;
const DT_TEXT: i64 = crate::types_meta::PRELUDE_DATATREE_TEXT;
const DT_BYTES: i64 = crate::types_meta::PRELUDE_DATATREE_BYTES;
const DT_ARRAY: i64 = crate::types_meta::PRELUDE_DATATREE_ARRAY;
const DT_OBJECT: i64 = crate::types_meta::PRELUDE_DATATREE_OBJECT;
// Private adapter tags: these carriers never escape a typed JSON decode.
const DT_NUMBER: i64 = -1;
const DT_TYPED_TEXT: i64 = -2;

/// One enum-record ABI: slot 0 is the discriminant and slot 1 holds the
/// payload under its own `JetVal` tag, exactly as `pack_enum_record` writes a
/// user enum. A text payload is therefore a `JetVal::String`, not an
/// Int-tagged string id: `unpack_enum_heap_payload_at` reads a `String`
/// payload with `struct_get_str`, and `jet_jit_struct_get_str` answers 0 for
/// an Int slot, so an Int-tagged `.Text` bound heap id 0 — the first string
/// the lowerer allocates, the entry file path — instead of the payload.
/// Allocate one DataTree enum record with the carrier tag expected by its
/// payload type.  Handles are record references in typed enum fields; dense
/// erased lists may still carry those same handles as `JetVal::Int`, which the
/// reader accepts explicitly below.
fn alloc_dt_record(disc: i64, payload: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let h = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(h, 0, disc);
        match disc {
            DT_BOOL => {
                let _ = rt.heap.record_set_bool(h, 1, payload != 0);
            }
            DT_FLOAT => {
                let _ = rt
                    .heap
                    .record_set_float(h, 1, f64::from_bits(payload as u64));
            }
            DT_TEXT | DT_NUMBER | DT_TYPED_TEXT => {
                let _ = rt.heap.record_set_string(h, 1, payload);
            }
            DT_ARRAY | DT_BYTES | DT_OBJECT => {
                let _ = rt.heap.record_set_record(h, 1, payload);
            }
            DT_NULL | DT_INT => {
                let _ = rt.heap.record_set_int(h, 1, payload);
            }
            _ => jet_foundation::ice!(None, "invalid DataTree discriminant"),
        }
        h
    })
}

pub(crate) fn alloc_datatree(tree: &json_rt::DataTree) -> i64 {
    match tree {
        json_rt::DataTree::Null => alloc_dt_record(DT_NULL, 0),
        json_rt::DataTree::Bool(b) => alloc_dt_record(DT_BOOL, i64::from(*b)),
        json_rt::DataTree::Int(n) => alloc_dt_record(DT_INT, *n),
        json_rt::DataTree::Float(f) => alloc_dt_record(DT_FLOAT, f.to_bits() as i64),
        json_rt::DataTree::Number(text) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text.clone()));
            alloc_dt_record(DT_NUMBER, sid)
        }
        json_rt::DataTree::TypedText(text) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text.clone()));
            alloc_dt_record(DT_TYPED_TEXT, sid)
        }
        json_rt::DataTree::Text(s) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s.clone()));
            alloc_dt_record(DT_TEXT, sid)
        }
        json_rt::DataTree::Bytes(bs) => {
            let list = Concurrency::with_runtime_mut(|rt| {
                let list = rt.heap.alloc_empty_list();
                for byte in bs {
                    let _ = rt.heap.list_push_int(list, i64::from(*byte));
                }
                list
            });
            alloc_dt_record(DT_BYTES, list)
        }
        json_rt::DataTree::Array(items) => {
            let handles: Vec<i64> = items.iter().map(alloc_datatree).collect();
            let list = Concurrency::with_runtime_mut(|rt| {
                rt.heap.alloc_list_values(
                    handles
                        .into_iter()
                        .map(JetVal::RecordRef)
                        .collect(),
                )
            });
            alloc_dt_record(DT_ARRAY, list)
        }
        json_rt::DataTree::Object(entries) => {
            // Ordered pair list (AOT `Vec<(String, DataTree)>`), not a key-sorted
            // Map — Codable / source field order must survive `json.to_string`.
            let pairs: Vec<(String, i64)> = entries
                .iter()
                .map(|(k, v)| (k.clone(), alloc_datatree(v)))
                .collect();
            let list = Concurrency::with_runtime_mut(|rt| {
                let pair_values = pairs
                    .into_iter()
                    .map(|(key, value)| {
                        let key = rt.heap.alloc_string(key);
                        let pair = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_string(pair, 0, key);
                        let _ = rt.heap.record_set_record(pair, 1, value);
                        JetVal::RecordRef(pair)
                    })
                    .collect();
                rt.heap.alloc_list_values(pair_values)
            });
            alloc_dt_record(DT_OBJECT, list)
        }
    }
}

fn datatree_handle_slot(slot: &JetVal) -> Option<i64> {
    match slot {
        JetVal::Int(value) | JetVal::RecordRef(value) => Some(*value),
        _ => None,
    }
}

fn datatree_string_slot(rt: &runtime_host::JitRuntime, slot: &JetVal) -> Option<String> {
    match slot {
        JetVal::String(value) => Some(value.clone()),
        JetVal::StringView { owner, start, end } => rt
            .heap
            .get_string(*owner)?
            .get(*start..*end)
            .map(str::to_owned),
        _ => None,
    }
}

pub(crate) fn read_datatree(handle: i64) -> Option<json_rt::DataTree> {
    let (disc, payload, text, child_handles, object_pairs, float_val) =
        Concurrency::with_runtime_mut(|rt| {
            let disc = rt.heap.record_get_int(handle, 0)?;
            match disc {
                DT_NULL => Some((disc, 0i64, None, None, None, None)),
                DT_BOOL => {
                    let payload = match rt.heap.record_get(handle, 1)? {
                        JetVal::Bool(value) => i64::from(*value),
                        JetVal::Int(value) => *value,
                        _ => return None,
                    };
                    Some((disc, payload, None, None, None, None))
                }
                DT_INT => {
                    let payload = match rt.heap.record_get(handle, 1)? {
                        JetVal::Int(value) => *value,
                        _ => return None,
                    };
                    Some((disc, payload, None, None, None, None))
                }
                DT_FLOAT => {
                    let f = rt.heap.record_get_float(handle, 1)?;
                    Some((disc, 0i64, None, None, None, Some(f)))
                }
                DT_TEXT | DT_NUMBER | DT_TYPED_TEXT => {
                    let s = datatree_string_slot(rt, rt.heap.record_get(handle, 1)?)?;
                    Some((disc, 0i64, Some(s), None, None, None))
                }
                DT_ARRAY => {
                    let payload = datatree_handle_slot(rt.heap.record_get(handle, 1)?)?;
                    let items = rt
                        .heap
                        .clone_list_values(payload)?
                        .into_iter()
                        .map(|slot| datatree_handle_slot(&slot))
                        .collect::<Option<Vec<_>>>()?;
                    Some((disc, payload, None, Some(items), None, None))
                }
                DT_BYTES => {
                    let payload = datatree_handle_slot(rt.heap.record_get(handle, 1)?)?;
                    let bytes = rt
                        .heap
                        .clone_list_values(payload)?
                        .into_iter()
                        .map(|slot| match slot {
                            JetVal::Int(value) => Some(value),
                            _ => None,
                        })
                        .collect::<Option<Vec<_>>>()?;
                    Some((disc, payload, None, Some(bytes), None, None))
                }
                DT_OBJECT => {
                    let payload = datatree_handle_slot(rt.heap.record_get(handle, 1)?)?;
                    let slots = rt.heap.clone_list_values(payload)?;
                    let mut pairs = Vec::with_capacity(slots.len());
                    for slot in slots {
                        let rec = datatree_handle_slot(&slot)?;
                        let key = datatree_string_slot(rt, rt.heap.record_get(rec, 0)?)?;
                        let value = datatree_handle_slot(rt.heap.record_get(rec, 1)?)?;
                        pairs.push((key, value));
                    }
                    Some((disc, payload, None, None, Some(pairs), None))
                }
                _ => None,
            }
        })?;
    match disc {
        DT_NULL => Some(json_rt::DataTree::Null),
        DT_BOOL => Some(json_rt::DataTree::Bool(payload != 0)),
        DT_INT => Some(json_rt::DataTree::Int(payload)),
        DT_FLOAT => Some(json_rt::DataTree::Float(float_val?)),
        DT_TEXT => Some(json_rt::DataTree::Text(text?)),
        DT_NUMBER => Some(json_rt::DataTree::Number(text?)),
        DT_TYPED_TEXT => Some(json_rt::DataTree::TypedText(text?)),
        DT_ARRAY => {
            let items = child_handles?
                .into_iter()
                .map(read_datatree)
                .collect::<Option<Vec<_>>>()?;
            Some(json_rt::DataTree::Array(items))
        }
        DT_BYTES => {
            let bytes = child_handles?
                .into_iter()
                .map(|byte| u8::try_from(byte).ok())
                .collect::<Option<Vec<_>>>()?;
            Some(json_rt::DataTree::Bytes(bytes))
        }
        DT_OBJECT => {
            let pairs = object_pairs?
                .into_iter()
                .map(|(key, value)| read_datatree(value).map(|tree| (key, tree)))
                .collect::<Option<Vec<_>>>()?;
            Some(json_rt::DataTree::Object(pairs))
        }
        _ => None,
    }
}

pub(crate) fn result_err_fields(errors: Vec<json_rt::FieldError>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let error_list = rt.heap.alloc_empty_list();
        for error in errors {
            let path = rt.heap.alloc_string(error.path);
            let reason = rt.heap.alloc_string(error.reason);
            let record = rt.heap.alloc_record(2);
            let _ = rt.heap.record_set_string(record, 0, path);
            let _ = rt.heap.record_set_string(record, 1, reason);
            let _ = rt.heap.list_push_int(error_list, record);
        }
        rt.results.push(super::JitResultValue {
            ok: false,
            bits: error_list as u64,
        });
        rt.results.len() as i64
    })
}

fn result_err_encoding(error: json_rt::EncodingError) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let format = match error.format {
            json_rt::EncodingFormat::JSON => 0,
            json_rt::EncodingFormat::JSONL => 1,
            json_rt::EncodingFormat::CSV => 2,
            json_rt::EncodingFormat::TOML => 3,
            json_rt::EncodingFormat::YAML => 4,
            json_rt::EncodingFormat::XML => 5,
            json_rt::EncodingFormat::CBOR => 6,
        };
        let kind = match error.kind {
            json_rt::EncodingErrorKind::Syntax => 0,
            json_rt::EncodingErrorKind::Truncated => 1,
            json_rt::EncodingErrorKind::Unsupported => 2,
            json_rt::EncodingErrorKind::Limit => 3,
            json_rt::EncodingErrorKind::IO => 4,
            json_rt::EncodingErrorKind::State => 5,
        };
        let cause = match error.cause {
            Ok(cause) => {
                let cause_record = rt.heap.alloc_record(3);
                let cause_kind = rt.heap.alloc_string(cause.kind);
                let cause_message = rt.heap.alloc_string(cause.message);
                let _ = rt.heap.record_set_string(cause_record, 0, cause_kind);
                let _ = rt.heap.record_set_int(
                    cause_record,
                    1,
                    cause.os_code.map(|code| code.wrapping_add(1)).unwrap_or(0),
                );
                let _ = rt.heap.record_set_string(cause_record, 2, cause_message);
                cause_record.wrapping_add(1)
            }
            Err(json_rt::JetAbsent) => 0,
        };
        let h = rt.heap.alloc_record(8);
        let _ = rt.heap.record_set_int(h, 0, format);
        let _ = rt.heap.record_set_int(h, 1, kind);
        let _ = rt.heap.record_set_int(h, 2, error.byte_offset);
        let _ = rt
            .heap
            .record_set_int(h, 3, error.line.map(|line| line).unwrap_or(0));
        let _ = rt
            .heap
            .record_set_int(h, 4, error.column.map(|column| column).unwrap_or(0));
        let path = rt.heap.alloc_string(error.path);
        let _ = rt.heap.record_set_string(h, 5, path);
        let reason = rt.heap.alloc_string(error.reason);
        let _ = rt.heap.record_set_string(h, 6, reason);
        let _ = rt.heap.record_set_int(h, 7, cause);
        rt.results.push(super::JitResultValue {
            ok: false,
            bits: h as u64,
        });
        rt.results.len() as i64
    })
}

fn result_errors(result: i64) -> Option<Vec<json_rt::FieldError>> {
    Concurrency::with_runtime_mut(|rt| {
        let value = result
            .checked_sub(1)
            .and_then(|index| rt.results.get(index as usize))
            .copied()?;
        if value.ok {
            return None;
        }
        let len = rt.heap.list_len(value.bits as i64)?;
        let mut errors = Vec::with_capacity(len as usize);
        for i in 0..len {
            let record = rt.heap.list_get_int(value.bits as i64, i).unwrap_or(0);
            errors.push(json_rt::FieldError {
                path: rt
                    .heap
                    .record_get_string(record, 0)
                    .and_then(|id| rt.heap.clone_string(id))
                    .unwrap_or_default(),
                reason: rt
                    .heap
                    .record_get_string(record, 1)
                    .and_then(|id| rt.heap.clone_string(id))
                    .unwrap_or_default(),
            });
        }
        Some(errors)
    })
}

fn result_err_decode(path: &str, reason: &str) -> i64 {
    let errors = if path.is_empty() {
        json_rt::FieldError::one(reason)
    } else {
        json_rt::FieldError::at(path, reason)
    };
    result_err_fields(errors)
}

fn clone_string_list(list: i64) -> Vec<String> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        (0..len)
            .filter_map(|index| rt.heap.list_get_int(list, index))
            .filter_map(|handle| rt.heap.clone_string(handle))
            .collect()
    })
}

fn env_config_insert_tree(tree: &mut json_rt::DataTree, segments: &[String], value: String) {
    let json_rt::DataTree::Object(entries) = tree else {
        return;
    };
    let Some(segment) = segments.first() else {
        return;
    };
    if segments.len() == 1 {
        if let Some((_, existing)) = entries
            .iter_mut()
            .find(|(name, _)| name.eq_ignore_ascii_case(segment))
        {
            *existing = json_rt::DataTree::Text(value);
        } else {
            entries.push((segment.clone(), json_rt::DataTree::Text(value)));
        }
        return;
    }
    if let Some((_, child)) = entries.iter_mut().find(|(name, child)| {
        name.eq_ignore_ascii_case(segment) && matches!(child, json_rt::DataTree::Object(_))
    }) {
        env_config_insert_tree(child, &segments[1..], value);
    } else {
        let mut child = json_rt::DataTree::Object(Vec::new());
        env_config_insert_tree(&mut child, &segments[1..], value);
        entries.push((segment.clone(), child));
    }
}

fn alloc_env_config_origins(origins: &[(String, String)]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for (name, path) in origins {
            let record = rt.heap.alloc_record(2);
            let name = rt.heap.alloc_string(name.clone());
            let path = rt.heap.alloc_string(path.clone());
            let _ = rt.heap.record_set_string(record, 0, name);
            let _ = rt.heap.record_set_string(record, 1, path);
            let _ = rt.heap.list_push_int(list, record);
        }
        list
    })
}

/// Build the resident `DataTree` source carrier for `core.sys.decode`.
///
/// The source order, dotenv parser, prefix fold, nested-key split, and
/// allowlist are all owned by `Prelude/Core/EnvConfig.rs`; this function only
/// converts its result to the Cranelift heap ABI.
fn jet_jit_env_config(prefix: i64, file: i64, allow: i64) -> i64 {
    let prefix = clone_string(prefix);
    let file = clone_string(file);
    let allow = clone_string_list(allow);
    if !env_config_rt::jet_env_config_file_is_project_relative(&file) {
        return result_err_decode("", "E2416: Dotenv.file must be project-relative");
    }
    let dotenv = if file.is_empty() {
        None
    } else {
        match std::fs::read_to_string(&file) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return result_err_decode(
                    "",
                    &format!("E2416: cannot read Dotenv.file `{file}`: {error}"),
                )
            }
        }
    };
    let process = crate::CoreHost::jit_env_snapshot_raw()
        .into_iter()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)));
    let entries =
        match env_config_rt::jet_env_config_entries(&prefix, dotenv.as_deref(), &allow, process) {
            Ok(entries) => entries,
            Err(reason) => return result_err_decode("", &format!("E2416: {reason}")),
        };
    let mut tree = json_rt::DataTree::Object(Vec::new());
    let mut origins = Vec::with_capacity(entries.len());
    for entry in entries {
        origins.push((entry.name, entry.segments.join(".")));
        env_config_insert_tree(&mut tree, &entry.segments, entry.value);
    }
    let tree = alloc_datatree(&tree);
    let origins = alloc_env_config_origins(&origins);
    let carrier = Concurrency::with_runtime_mut(|rt| {
        let carrier = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(carrier, 0, tree);
        let _ = rt.heap.record_set_int(carrier, 1, origins);
        carrier
    });
    result_ok(carrier as u64)
}

fn clone_env_config_origins(handle: i64) -> Vec<(String, String)> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(handle).unwrap_or(0);
        (0..len)
            .filter_map(|index| {
                let record = rt.heap.list_get_int(handle, index)?;
                let name = rt.heap.record_get_string(record, 0)?;
                let path = rt.heap.record_get_string(record, 1)?;
                Some((rt.heap.clone_string(name)?, rt.heap.clone_string(path)?))
            })
            .collect()
    })
}

/// Reframe the existing typed decoder's `[FieldError]` list with its source
/// environment variable. Success results pass through unchanged; all
/// validation still comes from the regular generated Decode function.
fn jet_jit_env_config_map(result: i64, origins: i64) -> i64 {
    let Some(errors) = result_errors(result) else {
        return result;
    };
    let origins = clone_env_config_origins(origins);
    let mapped = errors
        .into_iter()
        .map(|mut error| {
            let path = error.path.clone();
            let reason = std::mem::take(&mut error.reason);
            error.reason = env_config_rt::jet_env_config_error_reason(&path, &reason, &origins);
            error
        })
        .collect();
    result_err_fields(mapped)
}

fn hex_encode(bytes: &[u8]) -> String {
    encoding_base_rt::jet_std_hex_encode(&bytes.to_vec())
}

fn b64_encode(bytes: &[u8]) -> String {
    encoding_base_rt::jet_std_b64_encode(&bytes.to_vec())
}

fn b64url_encode(bytes: &[u8]) -> String {
    encoding_base_rt::jet_std_b64url_encode(&bytes.to_vec())
}

fn base32_encode(bytes: &[u8]) -> String {
    encoding_base_rt::jet_std_base32_encode(&bytes.to_vec())
}

fn hex_decode(text: &str) -> Result<Vec<u8>, String> {
    encoding_base_rt::jet_std_hex_decode(&text.to_string())
}

fn jet_jit_hex_encode(bytes: i64) -> i64 {
    let encoded = hex_encode(&clone_bytes(bytes));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(encoded))
}

fn jet_jit_hex_decode(text: i64) -> i64 {
    match hex_decode(&clone_string(text)) {
        Ok(bytes) => result_ok(alloc_byte_list(&bytes) as u64),
        Err(e) => result_err_msg(&e),
    }
}

fn jet_jit_b64_encode(bytes: i64) -> i64 {
    let encoded = b64_encode(&clone_bytes(bytes));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(encoded))
}

fn jet_jit_b64_encode_url(bytes: i64) -> i64 {
    let encoded = b64url_encode(&clone_bytes(bytes));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(encoded))
}

fn jet_jit_b64_decode(text: i64) -> i64 {
    let edition = PackageEdition::package_edition();
    match base_encoding_dispatch::decode_base64(&edition, &clone_string(text), false, false) {
        Ok(bytes) => result_ok(alloc_byte_list(&bytes) as u64),
        Err(e) => result_err_msg(&e),
    }
}

fn jet_jit_b64_decode_url(text: i64) -> i64 {
    let edition = PackageEdition::package_edition();
    match base_encoding_dispatch::decode_base64url(&edition, &clone_string(text), false, false) {
        Ok(bytes) => result_ok(alloc_byte_list(&bytes) as u64),
        Err(e) => result_err_msg(&e),
    }
}

fn jet_jit_base32_encode(bytes: i64) -> i64 {
    let encoded = base32_encode(&clone_bytes(bytes));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(encoded))
}

fn jet_jit_base32_decode(text: i64) -> i64 {
    let edition = PackageEdition::package_edition();
    match base_encoding_dispatch::decode_base32(&edition, &clone_string(text), false, false, false)
    {
        Ok(bytes) => result_ok(alloc_byte_list(&bytes) as u64),
        Err(e) => result_err_msg(&e),
    }
}

// ── CSV (shared with the AOT Prelude and comptime parser) ─────────────────────

pub(crate) fn csv_parse(
    text: &str,
    delimiter: &str,
    header: bool,
    skip_blank: bool,
) -> Result<Vec<jet_foundation::CsvKernel::CsvRecord>, String> {
    let delimiter = jet_foundation::CsvKernel::delimiter(delimiter)?;
    jet_foundation::CsvKernel::parse(
        text,
        jet_foundation::CsvKernel::CsvOptions {
            delimiter,
            header,
            skip_blank,
        },
    )
}

fn csv_render(rows: &[Vec<String>]) -> String {
    jet_foundation::CsvKernel::render(rows)
}

fn alloc_csv_records(records: Vec<jet_foundation::CsvKernel::CsvRecord>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let outer = rt.heap.alloc_empty_list();
        for record in records {
            let fields = rt.heap.alloc_empty_list();
            for cell in record.fields {
                let sid = rt.heap.alloc_string(cell);
                let _ = rt.heap.list_push_int(fields, sid);
            }
            let row = rt.heap.alloc_record(2);
            let _ = rt.heap.record_set_int(row, 0, fields);
            let _ = rt.heap.record_set_int(row, 1, record.line);
            let _ = rt.heap.list_push_int(outer, row);
        }
        outer
    })
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

fn jet_jit_csv_parse(text: i64, delimiter: i64, header: i64, skip_blank: i64) -> i64 {
    match csv_parse(
        &clone_string(text),
        &clone_string(delimiter),
        header != 0,
        skip_blank != 0,
    ) {
        Ok(rows) => result_ok(alloc_string_rows(
            rows.into_iter().map(|row| row.fields).collect(),
        ) as u64),
        Err(e) => result_err_msg(&e),
    }
}

fn jet_jit_csv_rows(text: i64, delimiter: i64, header: i64, skip_blank: i64) -> i64 {
    match csv_parse(
        &clone_string(text),
        &clone_string(delimiter),
        header != 0,
        skip_blank != 0,
    ) {
        Ok(rows) => result_ok(alloc_csv_records(rows) as u64),
        Err(e) => result_err_msg(&e),
    }
}

fn jet_jit_csv_to_string(rows: i64) -> i64 {
    let rendered = csv_render(&clone_string_rows(rows));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

fn jet_jit_enc_csv_to_string(values: i64, type_key: i64) -> i64 {
    let type_key = clone_string(type_key);
    let descriptor = typed_runtime_descriptor(&type_key);
    Concurrency::with_runtime_mut(|rt| {
        let rendered = (|| {
            let descriptor = descriptor
                .as_ref()
                .ok_or_else(|| format!("typed csv.to_string has no type `{type_key}`"))?;
            let values = rt
                .heap
                .clone_list_values(values)
                .ok_or_else(|| "typed csv.to_string received an invalid list".to_string())?;
            let mut trees = Vec::with_capacity(values.len());
            for value in values {
                trees.push(crate::Receipt::encode_jit_value_slot(rt, value, descriptor)?);
            }
            csv_render_datatree(&json_rt::DataTree::Array(trees)).map_err(str::to_owned)
        })();
        match rendered {
            Ok(rendered) => rt.heap.alloc_string(rendered),
            Err(message) => {
                rt.set_trap(&message);
                0
            }
        }
    })
}

/// Typed `csv.to_string([T])` where `T` is `#Codable`: the encoded `DataTree` is
/// an array of flat objects. Header comes from the first row's keys, then one
/// record per element. Mirrors AOT `jet_enc_csv_to_string` cell for cell.
/// parity: guard tests/encoding_parity.rs::typed_csv_encode_matches_aot_and_default_dev
fn csv_render_datatree(tree: &json_rt::DataTree) -> Result<String, &'static str> {
    let json_rt::DataTree::Array(trees) = tree else {
        return Err("csv.to_string needs rows or records");
    };
    let mut header: Vec<String> = Vec::new();
    if let Some(json_rt::DataTree::Object(entries)) = trees.first() {
        header = entries.iter().map(|(k, _)| k.clone()).collect();
    } else if !trees.is_empty() {
        return Err("csv.to_string needs rows or records");
    }
    let mut rows: Vec<Vec<String>> = vec![header.clone()];
    for tree in trees {
        let json_rt::DataTree::Object(entries) = tree else {
            return Err("csv.to_string needs rows or records");
        };
        let mut record = Vec::with_capacity(header.len());
        for key in &header {
            let cell = entries
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone());
            record.push(match cell {
                Some(json_rt::DataTree::Text(s)) => s,
                Some(json_rt::DataTree::Int(n)) => n.to_string(),
                Some(json_rt::DataTree::Float(f)) => format!("{f:?}"),
                Some(json_rt::DataTree::Bool(b)) => b.to_string(),
                Some(json_rt::DataTree::Null) | None => String::new(),
                Some(other) => json_rt::render_datatree_json(&other, false, 0),
            });
        }
        rows.push(record);
    }
    Ok(csv_render(&rows))
}

fn jet_jit_csv_tree_to_string(tree: i64) -> i64 {
    let rendered = read_datatree(tree)
        .ok_or("invalid DataTree")
        .and_then(|t| csv_render_datatree(&t));
    Concurrency::with_runtime_mut(|rt| match rendered {
        Ok(rendered) => rt.heap.alloc_string(rendered),
        Err(message) => {
            rt.set_trap(message);
            rt.heap.alloc_string(String::new())
        }
    })
}

// ── UUID (marshals through the shared Prelude entropy seam) ─────────────────

fn uuid_format(b: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-\
         {:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}

// #1481 core.crypto.uuid: parse/v5 mirror the AOT Prelude's `jet_uuid_bytes`/
// `jet_std_uuid_parse`/`jet_std_uuid_v5` (Source/Prelude/CoreLib/Top/
// EncodingCodecs.rs) — same validation and SHA-1 math, JIT-side heap ABI.
fn uuid_bytes(s: &str) -> Result<[u8; 16], String> {
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("`{s}` is not a UUID (want 8-4-4-4-12 hex digits)"));
    }
    let groups: Vec<usize> = s.match_indices('-').map(|(i, _)| i).collect();
    if groups != [8, 13, 18, 23] {
        return Err(format!("`{s}` is not a UUID (want 8-4-4-4-12 hex digits)"));
    }
    let mut bytes = [0u8; 16];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| format!("`{s}` is not a UUID (want 8-4-4-4-12 hex digits)"))?;
    }
    Ok(bytes)
}

fn uuid_sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn jet_jit_uuid_parse(text: i64) -> i64 {
    match uuid_bytes(&clone_string(text)) {
        Ok(bytes) => {
            let normalized = uuid_format(&bytes);
            Concurrency::with_runtime_mut(|rt| result_ok(rt.heap.alloc_string(normalized) as u64))
        }
        Err(e) => result_err_msg(&e),
    }
}

fn jet_jit_uuid_v5(namespace: i64, name: i64) -> i64 {
    let ns = match uuid_bytes(&clone_string(namespace)) {
        Ok(ns) => ns,
        Err(e) => return result_err_msg(&e),
    };
    let mut input = ns.to_vec();
    input.extend_from_slice(clone_string(name).as_bytes());
    let digest = uuid_sha1(&input);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let s = uuid_format(&bytes);
    Concurrency::with_runtime_mut(|rt| result_ok(rt.heap.alloc_string(s) as u64))
}
pub(crate) fn ambient_uuid_parse(text: &str) -> Result<String, String> {
    uuid_bytes(text).map(|bytes| uuid_format(&bytes))
}

pub(crate) fn ambient_uuid_v5(namespace: &str, name: &str) -> Result<String, String> {
    let ns = uuid_bytes(namespace)?;
    let mut input = ns.to_vec();
    input.extend_from_slice(name.as_bytes());
    let digest = uuid_sha1(&input);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(uuid_format(&bytes))
}


fn jet_jit_uuid_v4() -> i64 {
    let s = crate::Crypto::runtime::jet_crypto_uuid_v4();
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s))
}

/// `clock` is a 1-based handle to a canonical resident Clock.
fn jet_jit_uuid_v7(clock: i64) -> i64 {
    let timestamp = Concurrency::with_runtime_mut(|rt| {
        Some(if clock <= 0 {
            Err("invalid clock handle".to_string())
        } else {
            let idx = (clock as usize).saturating_sub(1);
            rt.clocks
                .get(idx)
                .map(|clock| clock.now())
                .ok_or_else(|| "invalid clock handle".to_string())
        })
    });
    let ts_ms = match timestamp {
        Some(Ok(timestamp)) => timestamp,
        Some(Err(message)) => {
            return Concurrency::with_runtime_mut(|rt| {
                rt.set_trap(&message);
                rt.heap.alloc_string(String::new())
            });
        }
        None => return 0,
    };
    let s = crate::Crypto::runtime::jet_crypto_uuid_v7(ts_ms);
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s))
}

// ── JSON / DataTree (core.encoding.json) ─────────────────────────────────────

fn jet_jit_json_parse(text: i64) -> i64 {
    match json_rt::parse_datatree(&clone_string(text)) {
        Ok(tree) => result_ok(alloc_datatree(&tree) as u64),
        Err(error) => result_err_encoding(error),
    }
}

/// Ordered JSON parsing is the front half of typed `json.decode`. Its failure
/// uses the same canonical `EncodingError` carrier as every format parser.
fn jet_jit_json_parse_ordered(text: i64) -> i64 {
    match json_rt::parse_datatree_typed_ordered(&clone_string(text)) {
        Ok(tree) => result_ok(alloc_datatree(&tree) as u64),
        Err(error) => result_err_encoding(error),
    }
}

fn jet_jit_json_decode(text: i64) -> i64 {
    match json_rt::decode_lenient(&clone_string(text)) {
        Ok(tree) => result_ok(alloc_datatree(&tree) as u64),
        Err(error) => result_err_encoding(error),
    }
}
fn typed_runtime_descriptor(type_key: &str) -> Option<runtime_host::RuntimeTypeDescriptor> {
    Concurrency::with_runtime_mut(|rt| {
        let id = type_key
            .strip_prefix("id:")
            .and_then(|value| value.parse::<u64>().ok());
        match id {
            Some(id) => rt.runtime_type_descriptor(id).cloned(),
            None => rt.runtime_type_descriptor_by_name(type_key).cloned(),
        }
    })
}

fn typed_child_descriptor(
    descriptor: &runtime_host::RuntimeTypeDescriptor,
    id: Option<u64>,
    role: &str,
) -> Result<runtime_host::RuntimeTypeDescriptor, Vec<json_rt::FieldError>> {
    id.and_then(|id| {
        Concurrency::with_runtime_mut(|rt| rt.runtime_type_descriptor(id).cloned())
    })
    .ok_or_else(|| {
        json_rt::FieldError::one(format!(
            "type `{}` has no checked {role} descriptor",
            descriptor.name
        ))
    })
}

fn typed_result_handle(ok: bool, bits: u64) -> i64 {
    Concurrency::with_runtime_mut(|rt| runtime_host::alloc_jit_result(rt, ok, bits))
}

fn typed_slot_raw(
    value: JetVal,
    descriptor: &runtime_host::RuntimeTypeDescriptor,
) -> Result<i64, String> {
    match descriptor.kind {
        runtime_host::RuntimeValueKind::Unit => match value {
            JetVal::Int(0) => Ok(0),
            _ => Err("typed decode produced a non-unit carrier".to_string()),
        },
        runtime_host::RuntimeValueKind::Int => match value {
            JetVal::Int(value) => Ok(value),
            JetVal::ExactInt(value) => Concurrency::with_runtime_string(|rt| {
                rt.heap
                    .int_from_str(&value.to_string_rep())
                    .map_err(|error| format!("invalid exact integer in typed decode: {error}"))
            }),
            _ => Err("typed decode produced a non-Int carrier".to_string()),
        },
        runtime_host::RuntimeValueKind::Float => match value {
            JetVal::Float(value) => Ok(value.to_bits() as i64),
            _ => Err("typed decode produced a non-Float carrier".to_string()),
        },
        runtime_host::RuntimeValueKind::Bool => match value {
            JetVal::Bool(value) => Ok(i64::from(value)),
            _ => Err("typed decode produced a non-Bool carrier".to_string()),
        },
        runtime_host::RuntimeValueKind::Char => match value {
            JetVal::Char(value) => Ok(i64::from(u32::from(value))),
            _ => Err("typed decode produced a non-Char carrier".to_string()),
        },
        runtime_host::RuntimeValueKind::String => match value {
            JetVal::String(value) => {
                Ok(Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value)))
            }
            JetVal::StringView { owner, start, end } => Concurrency::with_runtime_string(|rt| {
                let value = rt
                    .heap
                    .get_string(owner)
                    .and_then(|text| text.get(start..end))
                    .ok_or_else(|| "invalid StringView in typed decode".to_string())?
                    .to_string();
                Ok(rt.heap.alloc_string(value))
            }),
            _ => Err("typed decode produced a non-String carrier".to_string()),
        },
        runtime_host::RuntimeValueKind::Record | runtime_host::RuntimeValueKind::Enum => {
            match value {
                JetVal::RecordRef(value) => Ok(value),
                JetVal::Record(values) => Ok(Concurrency::with_runtime_mut(|rt| {
                    rt.heap.alloc_record_values(values)
                })),
                _ => Err("typed decode produced a non-record carrier".to_string()),
            }
        }
        runtime_host::RuntimeValueKind::List
        | runtime_host::RuntimeValueKind::Map
        | runtime_host::RuntimeValueKind::Shared
        | runtime_host::RuntimeValueKind::Option
        | runtime_host::RuntimeValueKind::Result
        | runtime_host::RuntimeValueKind::Closure
        | runtime_host::RuntimeValueKind::View
        | runtime_host::RuntimeValueKind::Iterator
        | runtime_host::RuntimeValueKind::Named
        | runtime_host::RuntimeValueKind::Handle => match value {
            JetVal::Int(value) | JetVal::RecordRef(value) => Ok(value),
            _ => Err("typed decode produced an incompatible handle carrier".to_string()),
        },
        _ => Err("typed decode produced an unsupported heap value".to_string()),
    }
}

fn typed_codec_kind(descriptor: &runtime_host::RuntimeTypeDescriptor) -> Option<i64> {
    match descriptor.name.as_str() {
        "Date" => Some(CODEC_KIND_DATE),
        "LocalDate" => Some(CODEC_KIND_LOCAL_DATE),
        "LocalTime" => Some(CODEC_KIND_LOCAL_TIME),
        "DateTime" => Some(CODEC_KIND_DATETIME),
        "Duration" => Some(CODEC_KIND_DURATION),
        "Decimal" => Some(CODEC_KIND_DECIMAL),
        _ => None,
    }
}

fn typed_codec_slot(
    descriptor: &runtime_host::RuntimeTypeDescriptor,
    tree: &json_rt::DataTree,
) -> Result<JetVal, Vec<json_rt::FieldError>> {
    let kind = typed_codec_kind(descriptor).ok_or_else(|| {
        json_rt::FieldError::one(format!("type `{}` has no checked decoder", descriptor.name))
    })?;
    let result = jet_jit_codec_decode(kind, alloc_datatree(tree));
    Concurrency::with_runtime_result(
        json_rt::FieldError::one("typed codec has no active resident runtime"),
        |rt| {
            let Some((ok, bits)) = runtime_host::jit_result_parts(rt, result) else {
                return Err(json_rt::FieldError::one("typed codec returned an invalid result"));
            };
            if ok {
                Ok(JetVal::Int(bits as i64))
            } else {
                Err(typed_result_errors(rt, bits as i64))
            }
        },
    )
}

fn typed_result_errors(rt: &runtime_host::JitRuntime, handle: i64) -> Vec<json_rt::FieldError> {
    let mut errors = Vec::new();
    let len = rt.heap.list_len(handle).unwrap_or(0);
    for index in 0..len {
        let record = rt.heap.list_get_int(handle, index).unwrap_or(0);
        let path = rt
            .heap
            .record_clone_string(record, 0)
            .unwrap_or_default();
        let reason = rt
            .heap
            .record_clone_string(record, 1)
            .unwrap_or_default();
        errors.push(json_rt::FieldError { path, reason });
    }
    if errors.is_empty() {
        json_rt::FieldError::one("typed codec decode failed")
    } else {
        errors
    }
}

fn typed_default_slot(
    descriptor: &runtime_host::RuntimeTypeDescriptor,
) -> Result<JetVal, Vec<json_rt::FieldError>> {
    let slot = match descriptor.kind {
        runtime_host::RuntimeValueKind::Unit
        | runtime_host::RuntimeValueKind::Int
        | runtime_host::RuntimeValueKind::Char => JetVal::Int(0),
        runtime_host::RuntimeValueKind::Float => JetVal::Float(0.0),
        runtime_host::RuntimeValueKind::Bool => JetVal::Bool(false),
        runtime_host::RuntimeValueKind::String => JetVal::String(String::new()),
        runtime_host::RuntimeValueKind::Option => {
            JetVal::Int(typed_result_handle(false, 0))
        }
        runtime_host::RuntimeValueKind::Result => {
            return Err(json_rt::FieldError::one(format!(
                "type `{}` has no checked default",
                descriptor.name
            )));
        }
        runtime_host::RuntimeValueKind::List => JetVal::Int(Concurrency::with_runtime_mut(|rt| {
            rt.heap.alloc_empty_list()
        })),
        runtime_host::RuntimeValueKind::Map => JetVal::Int(Concurrency::with_runtime_mut(|rt| {
            rt.heap.alloc_empty_map()
        })),
        runtime_host::RuntimeValueKind::Record => JetVal::RecordRef(Concurrency::with_runtime_mut(
            |rt| rt.heap.alloc_record(descriptor.fields.len()),
        )),
        _ => {
            return Err(json_rt::FieldError::one(format!(
                "type `{}` has no checked default",
                descriptor.name
            )))
        }
    };
    Ok(slot)
}

fn typed_default_field(
    parent: &runtime_host::RuntimeTypeDescriptor,
    field: &runtime_host::RuntimeFieldDescriptor,
) -> Result<JetVal, Vec<json_rt::FieldError>> {
    let descriptor = typed_child_descriptor(parent, Some(field.type_id), "field")?;
    typed_default_slot(&descriptor)
}

fn typed_decode_record(
    tree: &json_rt::DataTree,
    descriptor: &runtime_host::RuntimeTypeDescriptor,
) -> Result<JetVal, Vec<json_rt::FieldError>> {
    let json_rt::DataTree::Object(entries) = tree else {
        return Err(json_rt::FieldError::one(format!(
            "expected {}, found {}",
            descriptor.name,
            json_rt::datatree_kind_for(tree)
        )));
    };
    let mut slots = vec![JetVal::Int(0); descriptor.fields.len()];
    let mut errors = Vec::new();
    if descriptor.serde_deny_unknown {
        for (name, _) in entries {
            let known = descriptor.fields.iter().any(|field| {
                !field.skip && field.matches_name(name, ShapeProjectionKind::Json)
            });
            if !known {
                errors.extend(json_rt::FieldError::at(
                    name.clone(),
                    format!("E2412: unknown field `{name}`"),
                ));
            }
        }
    }
    for field in &descriptor.fields {
        if field.skip || field.computed {
            match typed_default_field(descriptor, field) {
                Ok(slot) => slots[field.index] = slot,
                Err(error) => errors.extend(json_rt::FieldError::under_errors(
                    &field.source_name,
                    error,
                )),
            }
            continue;
        }
        let child = match typed_child_descriptor(descriptor, Some(field.type_id), "field") {
            Ok(child) => child,
            Err(error) => {
                errors.extend(json_rt::FieldError::under_errors(&field.source_name, error));
                continue;
            }
        };
        let value = entries
            .iter()
            .find(|(name, _)| field.matches_name(name, ShapeProjectionKind::Json))
            .map(|(_, value)| value);
        let Some(value) = value else {
            if matches!(
                child.kind,
                runtime_host::RuntimeValueKind::Option | runtime_host::RuntimeValueKind::Result
            ) || field.has_default
            {
                match typed_default_slot(&child) {
                    Ok(slot) => slots[field.index] = slot,
                    Err(error) => errors.extend(json_rt::FieldError::under_errors(
                        field.name_for(ShapeProjectionKind::Json),
                        error,
                    )),
                }
            } else {
                errors.extend(json_rt::FieldError::at(
                    field.name_for(ShapeProjectionKind::Json),
                    format!("missing field `{}`", field.name_for(ShapeProjectionKind::Json)),
                ));
            }
            continue;
        };
        match typed_decode_value(value, &child) {
            Ok(slot) => slots[field.index] = slot,
            Err(error) => errors.extend(json_rt::FieldError::under_errors(
                field.name_for(ShapeProjectionKind::Json),
                error,
            )),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let record = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_record_values(slots));
    Ok(JetVal::RecordRef(record))
}

fn typed_decode_list(
    tree: &json_rt::DataTree,
    descriptor: &runtime_host::RuntimeTypeDescriptor,
) -> Result<JetVal, Vec<json_rt::FieldError>> {
    let child = typed_child_descriptor(descriptor, descriptor.element, "element")?;
    let values = match tree {
        json_rt::DataTree::Array(values) => values.clone(),
        json_rt::DataTree::Bytes(values) => values
            .iter()
            .map(|value| json_rt::DataTree::Int(json_rt::jet_int_from_i64(i64::from(*value))))
            .collect(),
        _ => {
            return Err(json_rt::FieldError::one(format!(
                "expected list, found {}",
                json_rt::datatree_kind_for(tree)
            )))
        }
    };
    let mut slots = Vec::with_capacity(values.len());
    let mut errors = Vec::new();
    for (index, value) in values.iter().enumerate() {
        match typed_decode_value(value, &child) {
            Ok(slot) => slots.push(slot),
            Err(error) => errors.extend(json_rt::FieldError::under_errors(
                &format!("[{index}]"),
                error,
            )),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let list = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_list_values(slots));
    Ok(JetVal::Int(list))
}

fn typed_decode_map(
    tree: &json_rt::DataTree,
    descriptor: &runtime_host::RuntimeTypeDescriptor,
) -> Result<JetVal, Vec<json_rt::FieldError>> {
    let json_rt::DataTree::Object(entries) = tree else {
        return Err(json_rt::FieldError::one(format!(
            "expected map, found {}",
            json_rt::datatree_kind_for(tree)
        )));
    };
    let key = typed_child_descriptor(descriptor, descriptor.key, "key")?;
    let value = typed_child_descriptor(descriptor, descriptor.value, "value")?;
    let map = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_map());
    let mut errors = Vec::new();
    for (name, tree) in entries {
        let key_slot = match key.kind {
            runtime_host::RuntimeValueKind::String => JetVal::String(name.clone()),
            runtime_host::RuntimeValueKind::Int => {
                match typed_decode_int(&json_rt::DataTree::Text(name.clone()), &key) {
                    Ok(value) => value,
                    Err(error) => {
                        errors.extend(json_rt::FieldError::under_errors(name, error));
                        continue;
                    }
                }
            }
            runtime_host::RuntimeValueKind::Bool => match name.as_str() {
                "true" => JetVal::Bool(true),
                "false" => JetVal::Bool(false),
                _ => {
                    errors.extend(json_rt::FieldError::under_errors(
                        name,
                        json_rt::FieldError::one("boolean map key must be true or false"),
                    ));
                    continue;
                }
            },
            _ => {
                errors.extend(json_rt::FieldError::under_errors(
                    name,
                    json_rt::FieldError::one("map key must be String, Int, or Bool"),
                ));
                continue;
            }
        };
        let slot = match typed_decode_value(tree, &value) {
            Ok(slot) => slot,
            Err(error) => {
                errors.extend(json_rt::FieldError::under_errors(name, error));
                continue;
            }
        };
        let key_raw = match typed_slot_raw(key_slot, &key) {
            Ok(value) => value,
            Err(error) => {
                errors.extend(json_rt::FieldError::under_errors(
                    name,
                    json_rt::FieldError::one(error),
                ));
                continue;
            }
        };
        let value_raw = match typed_slot_raw(slot, &value) {
            Ok(value) => value,
            Err(error) => {
                errors.extend(json_rt::FieldError::under_errors(
                    name,
                    json_rt::FieldError::one(error),
                ));
                continue;
            }
        };
        let key_int = if key.kind == runtime_host::RuntimeValueKind::Int {
            if key.integer_width.is_some() {
                Some(key_raw)
            } else {
                json_rt::jet_int_to_i64(key_raw)
            }
        } else {
            None
        };
        let inserted = Concurrency::with_runtime_mut(|rt| match key.kind {
            runtime_host::RuntimeValueKind::String => rt.heap.map_insert(map, key_raw, value_raw),
            runtime_host::RuntimeValueKind::Int => key_int
                .and_then(|value| rt.heap.map_insert_int(map, value, value_raw)),
            runtime_host::RuntimeValueKind::Bool => {
                rt.heap.map_insert_bool(map, name == "true", value_raw)
            }
            _ => None,
        });
        if inserted.is_none() {
            errors.extend(json_rt::FieldError::under_errors(
                name,
                json_rt::FieldError::one("map insertion failed"),
            ));
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(JetVal::Int(map))
}

fn typed_enum_shape_matches(
    tree: &json_rt::DataTree,
    parent: &runtime_host::RuntimeTypeDescriptor,
    variant: &runtime_host::RuntimeVariantDescriptor,
) -> bool {
    if variant.fields.is_empty() {
        return match tree {
            json_rt::DataTree::Null => true,
            json_rt::DataTree::Text(name) | json_rt::DataTree::TypedText(name) => {
                name == &variant.name || name == &variant.wire_name
            }
            _ => false,
        };
    }
    if variant.fields.len() == 1 {
        let Some(field) = variant.fields.first() else {
            return false;
        };
        let Ok(child) = typed_child_descriptor(parent, Some(field.type_id), "variant field") else {
            return false;
        };
        if typed_codec_kind(&child).is_some() {
            return matches!(
                tree,
                json_rt::DataTree::Text(_)
                    | json_rt::DataTree::TypedText(_)
                    | json_rt::DataTree::Int(_)
                    | json_rt::DataTree::Number(_)
                    | json_rt::DataTree::Float(_)
            );
        }
        return match child.kind {
            runtime_host::RuntimeValueKind::Unit => matches!(tree, json_rt::DataTree::Null),
            runtime_host::RuntimeValueKind::Int => matches!(
                tree,
                json_rt::DataTree::Int(_)
                    | json_rt::DataTree::Number(_)
                    | json_rt::DataTree::Text(_)
                    | json_rt::DataTree::TypedText(_)
            ),
            runtime_host::RuntimeValueKind::Float => matches!(
                tree,
                json_rt::DataTree::Int(_)
                    | json_rt::DataTree::Number(_)
                    | json_rt::DataTree::Float(_)
                    | json_rt::DataTree::Text(_)
            ),
            runtime_host::RuntimeValueKind::Bool => {
                matches!(tree, json_rt::DataTree::Bool(_) | json_rt::DataTree::Text(_))
            }
            runtime_host::RuntimeValueKind::Char
            | runtime_host::RuntimeValueKind::String => matches!(
                tree,
                json_rt::DataTree::Text(_)
                    | json_rt::DataTree::TypedText(_)
                    | json_rt::DataTree::Int(_)
                    | json_rt::DataTree::Float(_)
                    | json_rt::DataTree::Bool(_)
            ),
            runtime_host::RuntimeValueKind::List => {
                matches!(tree, json_rt::DataTree::Array(_) | json_rt::DataTree::Bytes(_))
            }
            runtime_host::RuntimeValueKind::Map
            | runtime_host::RuntimeValueKind::Record => {
                matches!(tree, json_rt::DataTree::Object(_))
            }
            runtime_host::RuntimeValueKind::Option | runtime_host::RuntimeValueKind::Shared => true,
            runtime_host::RuntimeValueKind::Result => false,
            runtime_host::RuntimeValueKind::Named | runtime_host::RuntimeValueKind::Handle => {
                (matches!(child.name.as_str(), "Path" | "JetPath")
                    || matches!(child.canonical.as_str(), "Path" | "JetPath"))
                    && matches!(
                        tree,
                        json_rt::DataTree::Text(_) | json_rt::DataTree::TypedText(_)
                    )
            }
            _ => false,
        };
    }
    let json_rt::DataTree::Object(entries) = tree else {
        return false;
    };
    variant.fields.iter().all(|field| {
        entries
            .iter()
            .any(|(name, _)| field.matches_name(name, ShapeProjectionKind::Json))
    })
}

fn typed_decode_enum(
    tree: &json_rt::DataTree,
    descriptor: &runtime_host::RuntimeTypeDescriptor,
) -> Result<JetVal, Vec<json_rt::FieldError>> {
    let (variant, payload) = match tree {
        json_rt::DataTree::Text(name) | json_rt::DataTree::TypedText(name) => {
            let variant = if descriptor.serde_untagged {
                descriptor
                    .variants
                    .iter()
                    .find(|variant| typed_enum_shape_matches(tree, descriptor, variant))
            } else {
                descriptor
                    .variants
                    .iter()
                    .find(|variant| variant.name == *name || variant.wire_name == *name)
            };
            (
                variant,
                if descriptor.serde_untagged {
                    tree.clone()
                } else {
                    json_rt::DataTree::Null
                },
            )
        }
        json_rt::DataTree::Null => (
            descriptor
                .serde_untagged
                .then(|| {
                    descriptor
                        .variants
                        .iter()
                        .find(|variant| typed_enum_shape_matches(tree, descriptor, variant))
                })
                .flatten(),
            tree.clone(),
        ),
        json_rt::DataTree::Object(entries) => {
            if descriptor.serde_untagged {
                (
                    descriptor
                        .variants
                        .iter()
                        .find(|variant| typed_enum_shape_matches(tree, descriptor, variant)),
                    tree.clone(),
                )
            } else {
                let tag_name = descriptor.serde_tag.as_deref().unwrap_or("tag");
                let tagged = entries
                    .iter()
                    .find(|(name, _)| name == tag_name)
                    .and_then(|(_, value)| match value {
                        json_rt::DataTree::Text(name)
                        | json_rt::DataTree::TypedText(name) => Some(name),
                        _ => None,
                    });
                if let Some(name) = tagged {
                    (
                        descriptor
                            .variants
                            .iter()
                            .find(|variant| variant.name == *name || variant.wire_name == *name),
                        tree.clone(),
                    )
                } else if let Some((variant, value)) = entries.iter().find_map(|(name, value)| {
                    descriptor
                        .variants
                        .iter()
                        .find(|variant| variant.name == *name || variant.wire_name == *name)
                        .map(|variant| (variant, value.clone()))
                }) {
                    (Some(variant), value)
                } else {
                    (None, tree.clone())
                }
            }
        }
        _ => (
            descriptor
                .serde_untagged
                .then(|| {
                    descriptor
                        .variants
                        .iter()
                        .find(|variant| typed_enum_shape_matches(tree, descriptor, variant))
                })
                .flatten(),
            tree.clone(),
        ),
    };
    let Some(variant) = variant else {
        return Err(json_rt::FieldError::one(format!(
            "unknown {} variant",
            descriptor.name
        )));
    };
    let mut slots = vec![JetVal::Int(variant.discriminant)];
    if variant.fields.len() == 1 {
        let value = match &payload {
            json_rt::DataTree::Object(entries) => entries
                .iter()
                .find(|(name, _)| name == &variant.fields[0].source_name)
                .map(|(_, value)| value)
                .unwrap_or(&payload),
            _ => &payload,
        };
        let child = typed_child_descriptor(descriptor, Some(variant.fields[0].type_id), "variant field")?;
        slots.push(typed_decode_value(value, &child)?);
    } else if !variant.fields.is_empty() {
        let json_rt::DataTree::Object(entries) = &payload else {
            return Err(json_rt::FieldError::one(format!(
                "expected object payload for {}",
                variant.name
            )));
        };
        for field in &variant.fields {
            let Some((_, value)) = entries
                .iter()
                .find(|(name, _)| field.matches_name(name, ShapeProjectionKind::Json))
            else {

                return Err(json_rt::FieldError::at(
                    field.name_for(ShapeProjectionKind::Json),
                    "missing enum payload field",
                ));
            };
            let child = typed_child_descriptor(descriptor, Some(field.type_id), "variant field")?;
            slots.push(typed_decode_value(value, &child)?);
        }
    }
    let record = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_record_values(slots));
    Ok(JetVal::RecordRef(record))
}

fn typed_decode_int(
    tree: &json_rt::DataTree,
    descriptor: &runtime_host::RuntimeTypeDescriptor,
) -> Result<JetVal, Vec<json_rt::FieldError>> {
    if let Some(width) = descriptor.integer_width {
        let value = json_rt::jet_datatree_decode_fixed_integer(
            tree,
            width.signed,
            width.bits,
            &descriptor.name,
        )?;
        if descriptor.integer_range.is_some_and(|(lo, hi)| !(lo..=hi).contains(&value)) {
            return Err(json_rt::FieldError::one(format!(
                "expected {}, found out-of-range Int",
                descriptor.name
            )));
        }
        // Fixed integers use native bits, not a tagged JetInt owner word.
        return Ok(JetVal::Int(value as i64));
    }
    let raw = json_rt::decode_int_with(
        tree,
        json_rt::jet_int_from_i64,
        json_rt::jet_int_from_str,
    )?;
    if let Some((lo, hi)) = descriptor.integer_range {
        let value = json_rt::jet_int_to_i128(raw).ok_or_else(|| {
            json_rt::FieldError::one(format!("expected {}, found out-of-range Int", descriptor.name))
        })?;
        if !(lo..=hi).contains(&value) {
            return Err(json_rt::FieldError::one(format!(
                "expected {}, found out-of-range Int",
                descriptor.name
            )));
        }
    }
    Ok(JetVal::Int(raw))
}

fn typed_decode_value(
    tree: &json_rt::DataTree,
    descriptor: &runtime_host::RuntimeTypeDescriptor,
) -> Result<JetVal, Vec<json_rt::FieldError>> {
    if typed_codec_kind(descriptor).is_some() {
        return typed_codec_slot(descriptor, tree);
    }
    match descriptor.kind {
        runtime_host::RuntimeValueKind::Unit => match tree {
            json_rt::DataTree::Null => Ok(JetVal::Int(0)),
            _ => Err(json_rt::FieldError::one(format!(
                "expected Unit, found {}",
                json_rt::datatree_kind_for(tree)
            ))),
        },
        runtime_host::RuntimeValueKind::Int => typed_decode_int(tree, descriptor),
        runtime_host::RuntimeValueKind::Float => {
            if descriptor.abi == runtime_host::RuntimeValueAbi::Float32 {
                json_rt::decode_f32(tree).map(|value| JetVal::Float(value as f64))
            } else {
                json_rt::decode_float(tree).map(JetVal::Float)
            }
        }
        runtime_host::RuntimeValueKind::Bool => json_rt::decode_bool(tree).map(JetVal::Bool),
        runtime_host::RuntimeValueKind::Char => {
            let text = json_rt::decode_string(tree)?;
            let mut chars = text.chars();
            let Some(value) = chars.next() else {
                return Err(json_rt::FieldError::one("expected Char, found empty text"));
            };
            if chars.next().is_some() {
                return Err(json_rt::FieldError::one("expected Char, found multiple characters"));
            }
            Ok(JetVal::Char(value))
        }
        runtime_host::RuntimeValueKind::String => json_rt::decode_string(tree).map(JetVal::String),
        runtime_host::RuntimeValueKind::Option => {
            if matches!(tree, json_rt::DataTree::Null) {
                return Ok(JetVal::Int(typed_result_handle(false, 0)));
            }
            let child = typed_child_descriptor(descriptor, descriptor.ok, "option")?;
            let value = typed_decode_value(tree, &child)?;
            let raw = typed_slot_raw(value, &child).map_err(json_rt::FieldError::one)?;
            Ok(JetVal::Int(typed_result_handle(true, raw as u64)))
        }
        runtime_host::RuntimeValueKind::Result => Err(json_rt::FieldError::one(format!(
            "type `{}` has no checked JSON decoder",
            descriptor.name
        ))),
        runtime_host::RuntimeValueKind::List => typed_decode_list(tree, descriptor),
        runtime_host::RuntimeValueKind::Map => typed_decode_map(tree, descriptor),
        runtime_host::RuntimeValueKind::Shared => {
            let child = typed_child_descriptor(descriptor, descriptor.element, "shared")?;
            let value = typed_slot_raw(typed_decode_value(tree, &child)?, &child)
                .map_err(json_rt::FieldError::one)?;
            let shared = Concurrency::with_runtime_mut(|rt| {
                crate::Memory::shared_alloc_for_persist(rt, value)
            });
            Ok(JetVal::Int(shared))
        }
        runtime_host::RuntimeValueKind::Record => typed_decode_record(tree, descriptor),
        runtime_host::RuntimeValueKind::Enum => typed_decode_enum(tree, descriptor),
        runtime_host::RuntimeValueKind::Named | runtime_host::RuntimeValueKind::Handle
            if matches!(descriptor.name.as_str(), "Path" | "JetPath")
                || matches!(descriptor.canonical.as_str(), "Path" | "JetPath") =>
        {
            let value = json_rt::decode_string(tree)?;
            let record = Concurrency::with_runtime_mut(|rt| {
                let record = rt.heap.alloc_record(1);
                let string = rt.heap.alloc_string(value);
                let _ = rt.heap.record_set_string(record, 0, string);
                record
            });
            Ok(JetVal::RecordRef(record))
        }
        _ => Err(json_rt::FieldError::one(format!(
            "type `{}` has no checked JSON decoder",
            descriptor.name
        ))),
    }
}

pub(crate) fn decode_datatree_for_type(
    tree: &json_rt::DataTree,
    type_key: &str,
) -> Result<i64, Vec<json_rt::FieldError>> {
    let descriptor = typed_runtime_descriptor(type_key)
        .ok_or_else(|| json_rt::FieldError::one(format!("typed JSON has no type `{type_key}`")))?;
    typed_decode_value(tree, &descriptor)
        .and_then(|value| typed_slot_raw(value, &descriptor).map_err(json_rt::FieldError::one))
}

fn jet_jit_json_decode_typed(text: i64, type_key: i64) -> i64 {
    let text = clone_string(text);
    let Some(type_key) = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(type_key)) else {
        return result_err_fields(json_rt::FieldError::one("typed JSON received an invalid type key"));
    };
    match json_rt::parse_datatree_typed_ordered(&text) {
        Ok(tree) => match decode_datatree_for_type(&tree, &type_key) {
            Ok(value) => result_ok(value as u64),
            Err(errors) => result_err_fields(errors),
        },
        Err(error) => result_err_fields(json_rt::FieldError::one(format!(
            "invalid JSON (line {}): {}",
            error.line.unwrap_or(0),
            error.reason
        ))),
    }
}
/// Typed codec ABI for TIR's borrowed DataTree route. The checked target type
/// travels as a heap string because the resident host has no monomorphized
/// Rust type to receive at this boundary.
fn jet_codec_decode_typed(tree: i64, type_key: i64) -> i64 {
    let Some(tree) = read_datatree(tree) else {
        return result_err_fields(json_rt::FieldError::one(
            "typed codec received an invalid DataTree",
        ));
    };
    let Some(type_key) = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(type_key)) else {
        return result_err_fields(json_rt::FieldError::one(
            "typed codec received an invalid type key",
        ));
    };
    match decode_datatree_for_type(&tree, &type_key) {
        Ok(value) => result_ok(value as u64),
        Err(errors) => result_err_fields(errors),
    }
}

fn typed_tree_at(tree: &json_rt::DataTree, path: &str) -> Option<json_rt::DataTree> {
    let mut value = tree.clone();
    for segment in path.split('.').filter(|segment| !segment.is_empty()) {
        value = match value {
            json_rt::DataTree::Object(entries) => entries
                .into_iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(segment))
                .map(|(_, value)| value)?,
            _ => return None,
        };
    }
    Some(value)
}

fn typed_tree_insert(tree: &mut json_rt::DataTree, path: &[String], value: json_rt::DataTree) {
    let json_rt::DataTree::Object(entries) = tree else {
        return;
    };
    let Some(segment) = path.first() else {
        return;
    };
    if path.len() == 1 {
        if let Some((_, existing)) = entries
            .iter_mut()
            .find(|(name, _)| name.eq_ignore_ascii_case(segment))
        {
            *existing = value;
        } else {
            entries.push((segment.clone(), value));
        }
        return;
    }
    if let Some((_, child)) = entries.iter_mut().find(|(name, child)| {
        name.eq_ignore_ascii_case(segment) && matches!(child, json_rt::DataTree::Object(_))
    }) {
        typed_tree_insert(child, &path[1..], value);
    } else {
        let mut child = json_rt::DataTree::Object(Vec::new());
        typed_tree_insert(&mut child, &path[1..], value);
        entries.push((segment.clone(), child));
    }
}

fn typed_env_projection_descriptor(
    descriptor: runtime_host::RuntimeTypeDescriptor,
) -> runtime_host::RuntimeTypeDescriptor {
    let child_id = match descriptor.kind {
        runtime_host::RuntimeValueKind::Option
        | runtime_host::RuntimeValueKind::Shared => descriptor.ok.or(descriptor.element),
        _ => None,
    };
    child_id
        .and_then(|id| {
            Concurrency::with_runtime_mut(|rt| rt.runtime_type_descriptor(id).cloned())
        })
        .map(typed_env_projection_descriptor)
        .unwrap_or(descriptor)
}

fn typed_env_project_tree(
    tree: &json_rt::DataTree,
    descriptor: &runtime_host::RuntimeTypeDescriptor,
    _prefix: &str,
    _origins: &[(String, String)],
) -> json_rt::DataTree {
    let json_rt::DataTree::Object(entries) = tree else {
        return tree.clone();
    };
    let projected = entries
        .iter()
        .map(|(name, value)| {
            let field = descriptor.fields.iter().find(|field| {
                !field.skip
                    && !field.computed
                    && (field.source_name.eq_ignore_ascii_case(name)
                        || field
                            .name_for(ShapeProjectionKind::Env)
                            .eq_ignore_ascii_case(name))
            });
            let Some(field) = field else {
                return (name.clone(), value.clone());
            };
            let value = Concurrency::with_runtime_mut(|rt| {
                rt.runtime_type_descriptor(field.type_id).cloned()
            })
            .map(|child| {
                let child = typed_env_projection_descriptor(child);
                typed_env_project_tree(value, &child, "", &[])
            })
            .unwrap_or_else(|| value.clone());
            (field.name_for(ShapeProjectionKind::Json).to_string(), value)
        })
        .collect();
    json_rt::DataTree::Object(projected)
}

fn typed_env_error_result(
    errors: Vec<json_rt::FieldError>,
    origins: &[(String, String)],
) -> i64 {
    let mapped = errors
        .into_iter()
        .map(|mut error| {
            let path = error.path.clone();
            let reason = std::mem::take(&mut error.reason);
            error.reason = env_config_rt::jet_env_config_error_reason(&path, &reason, origins);
            error
        })
        .collect();
    result_err_fields(mapped)
}

fn jet_jit_env_decode(prefix: i64, file: i64, allow: i64, type_key: i64) -> i64 {
    let prefix_text = clone_string(prefix);
    let Some(type_key) = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(type_key)) else {
        return result_err_fields(json_rt::FieldError::one("typed env received an invalid type key"));
    };
    let Some(descriptor) = typed_runtime_descriptor(&type_key) else {
        return result_err_fields(json_rt::FieldError::one(format!(
            "typed env has no type `{type_key}`"
        )));
    };
    let result = jet_jit_env_config(prefix, file, allow);
    let Some((tree_handle, origins_handle)) = Concurrency::with_runtime_mut(|rt| {
        let Some((ok, bits)) = runtime_host::jit_result_parts(rt, result) else {
            return None;
        };
        if !ok {
            return None;
        }
        let carrier = bits as i64;
        Some((
            rt.heap.record_get_int(carrier, 0)?,
            rt.heap.record_get_int(carrier, 1)?,
        ))
    }) else {
        return result;
    };
    let Some(tree) = read_datatree(tree_handle) else {
        return result_err_fields(json_rt::FieldError::one("typed env returned an invalid DataTree"));
    };
    let origins = clone_env_config_origins(origins_handle);
    let tree = typed_env_project_tree(&tree, &descriptor, &prefix_text, &origins);
    match typed_decode_value(&tree, &descriptor)
        .and_then(|value| typed_slot_raw(value, &descriptor).map_err(json_rt::FieldError::one))
    {
        Ok(value) => result_ok(value as u64),
        Err(errors) => typed_env_error_result(errors, &origins),
    }
}

fn jet_jit_db_decode(row: i64, type_key: i64) -> i64 {
    let Some(type_key) = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(type_key)) else {
        return result_err_fields(json_rt::FieldError::one("typed DB decode received an invalid type key"));
    };
    let Some(descriptor) = typed_runtime_descriptor(&type_key) else {
        return result_err_fields(json_rt::FieldError::one(format!(
            "typed DB decode has no type `{type_key}`"
        )));
    };
    let names = descriptor
        .fields
        .iter()
        .filter(|field| !field.skip && !field.computed)
        .map(|field| {
            (
                field.name_for(ShapeProjectionKind::Db),
                field.name_for(ShapeProjectionKind::Json),
            )
        })
        .collect::<Vec<_>>();
    let tree = match crate::DB::db_row_to_datatree(row, &names) {
        Ok(tree) => tree,
        Err(error) => return result_err_fields(json_rt::FieldError::one(error)),
    };
    match typed_decode_value(&tree, &descriptor)
        .and_then(|value| typed_slot_raw(value, &descriptor).map_err(json_rt::FieldError::one))
    {
        Ok(value) => result_ok(value as u64),
        Err(errors) => result_err_fields(errors),
    }
}




fn jet_jit_json_to_string(tree: i64) -> i64 {
    let rendered = read_datatree(tree)
        .map(|t| json_rt::render_datatree_json(&t, false, 0))
        .unwrap_or_else(|| "null".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

fn jet_jit_json_to_string_pretty(tree: i64) -> i64 {
    let rendered = read_datatree(tree)
        .map(|t| json_rt::render_datatree_json(&t, true, 0))
        .unwrap_or_else(|| "null".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

/// `core.encoding.json.canonical` — same sort+render as `jet_std_json_render_canonical`.
fn render_canonical(t: &json_rt::DataTree) -> String {
    fn sort_tree(t: &json_rt::DataTree) -> json_rt::DataTree {
        match t {
            json_rt::DataTree::Object(entries) => {
                let mut sorted = entries.clone();
                sorted.sort_by(|a, b| a.0.cmp(&b.0));
                json_rt::DataTree::Object(
                    sorted
                        .into_iter()
                        .map(|(k, v)| (k, sort_tree(&v)))
                        .collect(),
                )
            }
            json_rt::DataTree::Array(items) => {
                json_rt::DataTree::Array(items.iter().map(sort_tree).collect())
            }
            other => other.clone(),
        }
    }
    json_rt::render_datatree_json(&sort_tree(t), false, 0)
}

fn jet_jit_json_canonical(tree: i64) -> i64 {
    let rendered = read_datatree(tree)
        .map(|t| render_canonical(&t))
        .unwrap_or_else(|| "null".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

/// D-JSONCANON1=A edition 2027 — marshall to Prelude `jet_enc_json_canonical`.
fn jet_jit_json_canonical_checked(tree: i64, limits: i64) -> i64 {
    crate::enc_stream::json_canonical_checked(tree, limits)
}

/// `core.encoding.json.events` — same walk as `jet_std_json_events`.
fn json_events(t: &json_rt::DataTree) -> String {
    fn walk(path: String, t: &json_rt::DataTree, out: &mut Vec<String>) {
        let here = if path.is_empty() {
            "$".to_string()
        } else {
            path
        };
        match t {
            json_rt::DataTree::Object(entries) => {
                out.push(format!("object_start {here}"));
                for (k, v) in entries {
                    walk(format!("{here}.{k}"), v, out);
                }
                out.push(format!("object_end {here}"));
            }
            json_rt::DataTree::Array(items) => {
                out.push(format!("array_start {here}"));
                for (i, v) in items.iter().enumerate() {
                    walk(format!("{here}[{i}]"), v, out);
                }
                out.push(format!("array_end {here}"));
            }
            _ => out.push(format!("value {here} {}", render_canonical(t))),
        }
    }
    let mut out = Vec::new();
    walk(String::new(), t, &mut out);
    out.join("\n")
}

fn jet_jit_json_events(tree: i64) -> i64 {
    let rendered = read_datatree(tree)
        .map(|t| json_events(&t))
        .unwrap_or_else(|| String::new());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

/// `core.encoding.jsonl.parse` — same as `jet_std_jsonl_parse`.
fn jet_jit_jsonl_parse(text: i64) -> i64 {
    let src = clone_string(text);
    let mut handles = Vec::new();
    for (idx, line) in src.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match json_rt::parse_datatree(trimmed) {
            Ok(tree) => handles.push(alloc_datatree(&tree)),
            Err(mut error) => {
                error.format = json_rt::EncodingFormat::JSONL;
                error.line = error.line.map(|line| idx as i64 + line);
                return result_err_encoding(error);
            }
        }
    }
    let list = Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for h in handles {
            let _ = rt.heap.list_push_int(list, h);
        }
        list
    });
    result_ok(list as u64)
}

/// `core.encoding.jsonl.to_string` — same as `jet_std_jsonl_render`.
fn jet_jit_jsonl_to_string(rows: i64) -> i64 {
    let trees: Vec<json_rt::DataTree> = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(rows).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(rows, i).unwrap_or(0));
        }
        out
    })
    .into_iter()
    .filter_map(read_datatree)
    .collect();
    let mut rendered = trees
        .iter()
        .map(render_canonical)
        .collect::<Vec<_>>()
        .join("\n");
    if !rendered.is_empty() {
        rendered.push('\n');
    }
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

// ── XML via the shared foundation kernel ────────────────────────────────────

fn xml_value_to_datatree(value: jet_foundation::XmlPull::Value) -> json_rt::DataTree {
    use jet_foundation::XmlPull::Value;
    match value {
        Value::Null => json_rt::DataTree::Null,
        Value::Bool(b) => json_rt::DataTree::Bool(b),
        Value::Int(n) => json_rt::DataTree::Int(n),
        Value::Text(s) => json_rt::DataTree::Text(s),
        Value::Array(xs) => {
            json_rt::DataTree::Array(xs.into_iter().map(xml_value_to_datatree).collect())
        }
        Value::Object(es) => json_rt::DataTree::Object(
            es.into_iter()
                .map(|(k, v)| (k, xml_value_to_datatree(v)))
                .collect(),
        ),
    }
}

fn datatree_to_xml_value(
    tree: &json_rt::DataTree,
) -> Result<jet_foundation::XmlPull::Value, String> {
    use jet_foundation::XmlPull::Value;
    match tree {
        json_rt::DataTree::Null => Ok(Value::Null),
        json_rt::DataTree::Bool(b) => Ok(Value::Bool(*b)),
        json_rt::DataTree::Int(n) => Ok(Value::Int(*n)),
        json_rt::DataTree::Number(_) | json_rt::DataTree::TypedText(_) => {
            Err("internal JSON carrier escaped typed decode".to_string())
        }
        json_rt::DataTree::Text(s) => Ok(Value::Text(s.clone())),
        json_rt::DataTree::Array(xs) => Ok(Value::Array(
            xs.iter()
                .map(datatree_to_xml_value)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        json_rt::DataTree::Object(es) => Ok(Value::Object(
            es.iter()
                .map(|(k, v)| Ok((k.clone(), datatree_to_xml_value(v)?)))
                .collect::<Result<Vec<_>, String>>()?,
        )),
        json_rt::DataTree::Float(_) | json_rt::DataTree::Bytes(_) => {
            Err("XML tree cannot contain Float or Bytes values".to_string())
        }
    }
}

fn xml_shape_error(reason: impl Into<String>) -> json_rt::EncodingError {
    json_rt::EncodingError::new(
        json_rt::EncodingFormat::XML,
        json_rt::EncodingErrorKind::Syntax,
        0,
        Err(json_rt::JetAbsent),
        Err(json_rt::JetAbsent),
        "",
        reason,
    )
}

fn xml_error_encoding(error: jet_foundation::XmlPull::Error) -> json_rt::EncodingError {
    use jet_foundation::XmlPull::Reason;
    let kind = match error.kind {
        Reason::EntityCycle | Reason::Limit => json_rt::EncodingErrorKind::Limit,
        Reason::Canonicalization | Reason::Unsupported => {
            json_rt::EncodingErrorKind::Unsupported
        }
        _ => json_rt::EncodingErrorKind::Syntax,
    };
    json_rt::EncodingError::new(
        json_rt::EncodingFormat::XML,
        kind,
        error.offset as i64,
        error
            .line
            .map(|line| Ok(line as i64))
            .unwrap_or(Err(json_rt::JetAbsent)),
        error
            .column
            .map(|column| Ok(column as i64))
            .unwrap_or(Err(json_rt::JetAbsent)),
        error.path,
        error.reason,
    )
}

fn jet_jit_xml_parse(text: i64) -> i64 {
    match jet_foundation::XmlKernel::parse_document(&clone_string(text)) {
        Ok(value) => result_ok(alloc_datatree(&xml_value_to_datatree(value)) as u64),
        Err(error) => result_err_encoding(xml_error_encoding(error)),
    }
}

fn jet_jit_xml_to_string(tree: i64) -> i64 {
    let rendered = read_datatree(tree)
        .and_then(|t| datatree_to_xml_value(&t).ok())
        .and_then(|v| jet_foundation::XmlKernel::render_document(&v).ok())
        .unwrap_or_default();
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

fn xml_tree_value(
    tree: i64,
) -> Result<jet_foundation::XmlPull::Value, json_rt::EncodingError> {
    let dt = read_datatree(tree).ok_or_else(|| xml_shape_error("invalid DataTree"))?;
    datatree_to_xml_value(&dt).map_err(xml_shape_error)
}

fn pack_opt_string(opt: Option<String>) -> i64 {
    Concurrency::with_runtime_mut(|rt| match opt {
        None => 0,
        Some(s) => {
            let sid = rt.heap.alloc_string(s);
            sid + 1
        }
    })
}

/// D-ENCXML-PROJECTION1: `xml.root` via the shared XML kernel.
fn jet_jit_xml_root(tree: i64) -> i64 {
    match xml_tree_value(tree)
        .and_then(|v| jet_foundation::XmlKernel::document_root(&v).map_err(xml_error_encoding))
    {
        Ok(root) => result_ok(alloc_datatree(&xml_value_to_datatree(root)) as u64),
        Err(error) => result_err_encoding(error),
    }
}

/// `xml.expanded_name` → Result[(raw, prefix?, local, namespace_uri?), EncodingError].
fn jet_jit_xml_expanded_name(tree: i64) -> i64 {
    match xml_tree_value(tree).and_then(|v| {
        jet_foundation::XmlKernel::expanded_name_parts(&v).map_err(xml_error_encoding)
    }) {
        Ok((raw, prefix, local, uri)) => {
            let handle = Concurrency::with_runtime_mut(|rt| {
                let rec = rt.heap.alloc_record(4);
                let raw_id = rt.heap.alloc_string(raw);
                let _ = rt.heap.record_set_string(rec, 0, raw_id);
                let prefix_bits = match prefix {
                    None => 0,
                    Some(s) => rt.heap.alloc_string(s) + 1,
                };
                let _ = rt.heap.record_set_int(rec, 1, prefix_bits);
                let local_id = rt.heap.alloc_string(local);
                let _ = rt.heap.record_set_string(rec, 2, local_id);
                let uri_bits = match uri {
                    None => 0,
                    Some(s) => rt.heap.alloc_string(s) + 1,
                };
                let _ = rt.heap.record_set_int(rec, 3, uri_bits);
                rec
            });
            result_ok(handle as u64)
        }
        Err(error) => result_err_encoding(error),
    }
}

/// `xml.attribute` → Result[String?, EncodingError] (Option packed as 0 / sid+1).
fn jet_jit_xml_attribute(tree: i64, name: i64) -> i64 {
    let key = clone_string(name);
    match xml_tree_value(tree).and_then(|v| {
        jet_foundation::XmlKernel::lookup_attribute(&v, &key).map_err(xml_error_encoding)
    }) {
        Ok(opt) => result_ok(pack_opt_string(opt) as u64),
        Err(error) => result_err_encoding(error),
    }
}

/// `xml.content` → Result[[DataTree], EncodingError].
fn jet_jit_xml_content(tree: i64) -> i64 {
    match xml_tree_value(tree)
        .and_then(|v| jet_foundation::XmlKernel::element_content(&v).map_err(xml_error_encoding))
    {
        Ok(nodes) => {
            let handles: Vec<i64> = nodes
                .into_iter()
                .map(|n| alloc_datatree(&xml_value_to_datatree(n)))
                .collect();
            let list = Concurrency::with_runtime_mut(|rt| {
                let list = rt.heap.alloc_empty_list();
                for h in handles {
                    let _ = rt.heap.list_push_int(list, h);
                }
                list
            });
            result_ok(list as u64)
        }
        Err(error) => result_err_encoding(error),
    }
}

/// `xml.to_bytes` with XMLRenderOptions::safe (UTF-8 + PreserveValid).
fn jet_jit_xml_to_bytes(tree: i64) -> i64 {
    match xml_tree_value(tree).and_then(|v| {
        jet_foundation::XmlKernel::render_document_bytes(
            &v,
            jet_foundation::XmlPull::RenderEncoding::UTF8,
            jet_foundation::XmlPull::LexicalPolicy::PreserveValid,
        )
        .map_err(xml_error_encoding)
    }) {
        Ok(bytes) => result_ok(alloc_byte_list(&bytes) as u64),
        Err(error) => result_err_encoding(error),
    }
}

/// Parse + `project_document_for_decode` — front half of typed `xml.decode`.
fn jet_jit_xml_project(text: i64) -> i64 {
    match jet_foundation::XmlKernel::parse_document(&clone_string(text)) {
        Ok(value) => match jet_foundation::XmlKernel::project_document_for_decode(&value) {
            Ok(projected) => result_ok(alloc_datatree(&xml_value_to_datatree(projected)) as u64),
            Err(e) => result_err_fields(xml_decode_fields(e)),
        },
        Err(e) => result_err_fields(xml_decode_fields(e)),
    }
}

/// Parse bytes + project — front half of typed `xml.decode_bytes`.
fn jet_jit_xml_project_bytes(bytes: i64) -> i64 {
    let input = clone_bytes(bytes);
    match jet_foundation::XmlKernel::parse_document_bytes(&input) {
        Ok(value) => match jet_foundation::XmlKernel::project_document_for_decode(&value) {
            Ok(projected) => result_ok(alloc_datatree(&xml_value_to_datatree(projected)) as u64),
            Err(e) => result_err_fields(xml_decode_fields(e)),
        },
        Err(e) => result_err_fields(xml_decode_fields(e)),
    }
}

// ── CBOR (shared foundation kernel) ─────────────────────────────────────────

fn jet_jit_cbor_to_bytes(tree: i64) -> i64 {
    jet_jit_cbor_to_bytes_impl(tree, false)
}

fn jet_jit_cbor_to_bytes_canonical(tree: i64) -> i64 {
    jet_jit_cbor_to_bytes_impl(tree, true)
}

fn jet_jit_cbor_to_bytes_impl(tree: i64, canonical: bool) -> i64 {
    match read_datatree(tree) {
        Some(tree) => {
            let value = cbor_datatree_to_value(&tree);
            match CborKernel::encode(&value, canonical) {
                Ok(out) => result_ok(alloc_byte_list(&out) as u64),
                Err(error) => result_err_cbor(error),
            }
        }
        None => result_err_msg("invalid DataTree"),
    }
}

fn cbor_datatree_to_value(value: &json_rt::DataTree) -> Value {
    match value {
        json_rt::DataTree::Null => Value::Null,
        json_rt::DataTree::Bool(value) => Value::Bool(*value),
        json_rt::DataTree::Int(value) => Value::Int(*value),
        json_rt::DataTree::Float(value) => Value::Float(*value),
        json_rt::DataTree::Number(_) | json_rt::DataTree::TypedText(_) => {
            unreachable!("internal JSON carrier escaped typed decode")
        }
        json_rt::DataTree::Text(value) => Value::Text(value.clone()),
        json_rt::DataTree::Bytes(value) => Value::Bytes(value.clone()),
        json_rt::DataTree::Array(values) => {
            Value::Array(values.iter().map(cbor_datatree_to_value).collect())
        }
        json_rt::DataTree::Object(entries) => Value::Object(
            entries
                .iter()
                .map(|(key, value)| (key.clone(), cbor_datatree_to_value(value)))
                .collect(),
        ),
    }
}

fn cbor_value_to_datatree(value: Value) -> json_rt::DataTree {
    match value {
        Value::Null => json_rt::DataTree::Null,
        Value::Bool(value) => json_rt::DataTree::Bool(value),
        Value::Int(value) => json_rt::DataTree::Int(value),
        Value::Float(value) => json_rt::DataTree::Float(value),
        Value::Text(value) => json_rt::DataTree::Text(value),
        Value::Bytes(value) => json_rt::DataTree::Bytes(value),
        Value::Array(values) => json_rt::DataTree::Array(
            values.into_iter().map(cbor_value_to_datatree).collect(),
        ),
        Value::Object(entries) => json_rt::DataTree::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key, cbor_value_to_datatree(value)))
                .collect(),
        ),
    }
}

fn cbor_error_kind(kind: CborKernel::ErrorKind) -> i64 {
    match kind {
        CborKernel::ErrorKind::Syntax => 0,
        CborKernel::ErrorKind::Truncated => 1,
        CborKernel::ErrorKind::Unsupported => 2,
        CborKernel::ErrorKind::Limit => 3,
        CborKernel::ErrorKind::TypeMismatch => 4,
        CborKernel::ErrorKind::TrailingData => 5,
        CborKernel::ErrorKind::NonCanonical => 6,
    }
}

fn result_err_cbor_parts(kind: i64, byte_offset: i64, path: String, reason: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let path = rt.heap.alloc_string(path);
        let reason = rt.heap.alloc_string(reason);
        let error = rt.heap.alloc_record(4);
        let _ = rt.heap.record_set_int(error, 0, kind);
        let _ = rt.heap.record_set_int(error, 1, byte_offset);
        let _ = rt.heap.record_set_string(error, 2, path);
        let _ = rt.heap.record_set_string(error, 3, reason);
        rt.results.push(super::JitResultValue {
            ok: false,
            bits: error as u64,
        });
        rt.results.len() as i64
    })
}

fn result_err_cbor(error: CborKernel::Error) -> i64 {
    result_err_cbor_parts(
        cbor_error_kind(error.kind),
        error.byte_offset as i64,
        error.path,
        error.reason,
    )
}

/// Kernel `$`-rooted paths become the Prelude's field-error spelling: the
/// document root is the empty path and `$.` prefixes are dropped.
fn decode_path(path: &str) -> String {
    if path == "$" {
        String::new()
    } else if let Some(path) = path.strip_prefix("$.") {
        path.to_string()
    } else {
        path.strip_prefix('$').unwrap_or(path).to_string()
    }
}

fn xml_decode_fields(error: jet_foundation::XmlPull::Error) -> Vec<json_rt::FieldError> {
    json_rt::FieldError::at(
        decode_path(&error.path),
        format!("XML {:?}: {}", error.kind, error.reason),
    )
}

fn cbor_decode_fields(error: CborKernel::Error) -> Vec<json_rt::FieldError> {
    json_rt::FieldError::at(
        decode_path(&error.path),
        format!("CBOR at byte {}: {}", error.byte_offset, error.reason),
    )
}

fn jet_jit_cbor_parse_impl(bytes: i64, options: Option<i64>, allow_bytes: bool) -> i64 {
    let input = clone_bytes(bytes);
    let options = match options {
        Some(handle) => {
            let (max_depth, max_items, max_bytes, require_canonical) =
                Concurrency::with_runtime_mut(|rt| {
                    (
                        rt.heap.record_get_int(handle, 0),
                        rt.heap.record_get_int(handle, 1),
                        rt.heap.record_get_int(handle, 2),
                        rt.heap.record_get_bool(handle, 3),
                    )
                });
            match CborKernel::Options::from_fields(
                max_depth,
                max_items,
                max_bytes,
                require_canonical,
            ) {
                Ok(options) => options,
                Err(error) => {
                    return if allow_bytes {
                        result_err_fields(cbor_decode_fields(error))
                    } else {
                        result_err_cbor(error)
                    }
                }
            }
        }
        None => CborKernel::Options::safe(),
    };
    match CborKernel::decode(&input, &options, allow_bytes) {
        Ok(value) => result_ok(alloc_datatree(&cbor_value_to_datatree(value)) as u64),
        Err(error) if allow_bytes => result_err_fields(cbor_decode_fields(error)),
        Err(error) => result_err_cbor(error),
    }
}

fn jet_jit_cbor_parse(bytes: i64) -> i64 {
    jet_jit_cbor_parse_impl(bytes, None, false)
}

fn jet_jit_cbor_parse_options(bytes: i64, options: i64) -> i64 {
    jet_jit_cbor_parse_impl(bytes, Some(options), false)
}

fn jet_jit_cbor_decode_tree(bytes: i64) -> i64 {
    jet_jit_cbor_parse_impl(bytes, None, true)
}

fn jet_jit_cbor_decode_tree_options(bytes: i64, options: i64) -> i64 {
    jet_jit_cbor_parse_impl(bytes, Some(options), true)
}

/// Typed CSV decode: parse header+rows, then invoke the resident Decode
/// callback through the shared Prelude row kernel.
fn jet_jit_enc_csv_decode(text: i64, callback: i64, element_kind: i64) -> i64 {
    let callback = Concurrency::with_runtime_mut(|rt| {
        let Some(slot) = runtime_host::jit_callable_parts(rt, callback) else {
            rt.set_host_fault("CSV Decode callback is invalid");
            return None;
        };
        if slot.raw_unary.is_none() || slot.raw_pair.is_some() || slot.raw_many.is_some() {
            rt.set_host_fault("CSV Decode callback has no unary universal thunk");
            return None;
        }
        Some(slot)
    });
    let Some(callback) = callback else {
        return 0;
    };
    let rows = match csv_parse(&clone_string(text), ",", false, false) {
        Ok(rows) => rows,
        Err(error) => return result_err_fields(json_rt::FieldError::one(error)),
    };
    let mut callback_fault = false;
    let decoded = json_rt::jet_enc_csv_decode_rows(
        rows.into_iter().map(|row| row.fields),
        |tree| {
            if callback_fault {
                return Err(Vec::new());
            }
            let tree_handle = alloc_datatree(&tree);
            let Some(decoded_result) = runtime_host::invoke_universal_unary(callback, tree_handle)
            else {
                callback_fault = true;
                Concurrency::with_runtime_mut(|rt| {
                    rt.set_host_fault("CSV Decode callback invocation failed")
                });
                return Err(Vec::new());
            };
            let Some((ok, bits)) =
                Concurrency::with_runtime_mut(|rt| runtime_host::jit_result_parts(rt, decoded_result))
            else {
                callback_fault = true;
                Concurrency::with_runtime_mut(|rt| {
                    rt.set_host_fault("CSV Decode callback returned an invalid Result")
                });
                return Err(Vec::new());
            };
            if !ok {
                let Some(errors) = result_errors(decoded_result) else {
                    callback_fault = true;
                    Concurrency::with_runtime_mut(|rt| {
                        rt.set_host_fault("CSV Decode callback returned an invalid error")
                    });
                    return Err(Vec::new());
                };
                return Err(errors);
            }
            let value = if element_kind == CSV_DECODE_FLOAT {
                JetVal::Float(f64::from_bits(bits))
            } else {
                JetVal::Int(bits as i64)
            };
            Ok(value)
        },
        |row, error| json_rt::FieldError::under_errors(row, error),
        json_rt::DataTree::Text,
        json_rt::DataTree::Object,
    );
    if callback_fault {
        return 0;
    }
    let decoded = match decoded {
        Ok(values) => values,
        Err(errors) => return result_err_fields(errors),
    };
    let list = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_list_values(decoded));
    result_ok(list as u64)
}
fn jet_jit_csv_query_read(path: i64) -> i64 {
    let path = clone_string(path);
    match data_query_rt::jet_data_query_read(&path) {
        Ok(text) => {
            let handle = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text));
            result_ok(handle as u64)
        }
        Err(errors) => result_err_fields(errors),
    }
}

fn jet_jit_csv_decode_scalar(kind: i64, tree: i64) -> i64 {
    match kind {
        CSV_DECODE_INT => jet_jit_datatree_decode_int(tree),
        CSV_DECODE_FLOAT => jet_jit_datatree_float(tree),
        CSV_DECODE_BOOL => jet_jit_datatree_bool(tree),
        CSV_DECODE_STRING => jet_jit_datatree_decode_string(tree),
        CSV_DECODE_CHAR => jet_jit_datatree_decode_char(tree),
        CSV_DECODE_DATATREE => {
            if read_datatree(tree).is_some() {
                result_ok(tree as u64)
            } else {
                result_err_decode("", "invalid DataTree")
            }
        }
        CSV_DECODE_DATE => jet_jit_codec_decode(CODEC_KIND_DATE, tree),
        CSV_DECODE_LOCAL_DATE => jet_jit_codec_decode(CODEC_KIND_LOCAL_DATE, tree),
        CSV_DECODE_LOCAL_TIME => jet_jit_codec_decode(CODEC_KIND_LOCAL_TIME, tree),
        CSV_DECODE_DATETIME => jet_jit_codec_decode(CODEC_KIND_DATETIME, tree),
        CSV_DECODE_DURATION => jet_jit_codec_decode(CODEC_KIND_DURATION, tree),
        CSV_DECODE_DECIMAL => jet_jit_codec_decode(CODEC_KIND_DECIMAL, tree),
        _ => result_err_decode("", "CSV query row type has no scalar DataTree decoder"),
    }
}

fn jet_jit_enc_csv_query(path: i64, sql: i64, callback: i64, element_kind: i64) -> i64 {
    if !matches!(element_kind, CSV_DECODE_INT | CSV_DECODE_FLOAT) {
        return result_err_fields(json_rt::FieldError::one(
            "CSV query row type has an unsupported list carrier",
        ));
    }
    let path = clone_string(path);
    let sql = clone_string(sql);
    let text = match data_query_rt::jet_data_query_read(&path) {
        Ok(text) => text,
        Err(errors) => return result_err_fields(errors),
    };
    let rows = match csv_parse(&text, ",", false, false) {
        Ok(rows) => rows,
        Err(error) => return result_err_fields(json_rt::FieldError::one(error)),
    };
    let fields = rows
        .first()
        .map(|row| row.fields.clone())
        .unwrap_or_default();
    if let Err(errors) = data_query_rt::jet_data_query_validate_fields(&fields, &sql) {
        return result_err_fields(errors);
    }
    let trees = rows
        .into_iter()
        .skip(1)
        .map(|row| {
            let object = row
                .fields
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    (
                        fields.get(index).cloned().unwrap_or_default(),
                        json_rt::DataTree::Text(value),
                    )
                })
                .collect();
            json_rt::DataTree::Object(object)
        })
        .collect::<Vec<_>>();

    // Decode every input row before applying SQL selection. This matches the
    // canonical Prelude query path, which reports malformed rows even when a
    // query would otherwise discard them.
    let mut decoded = Vec::with_capacity(trees.len());
    let mut decode_errors = Vec::new();
    for (index, tree) in trees.iter().enumerate() {
        let row_path = format!("row {}", index + 1);
        let tree_handle = alloc_datatree(tree);
        let decoded_result = Concurrency::with_runtime_mut(|rt| {
            runtime_host::jit_callable_parts(rt, callback)
                .and_then(|slot| runtime_host::invoke_universal_unary(slot, tree_handle))
        });
        let Some(decoded_result) = decoded_result else {
            decode_errors.extend(json_rt::FieldError::under_errors(
                &row_path,
                json_rt::FieldError::one("CSV query Decode callback is invalid"),
            ));
            continue;
        };
        let Some((ok, bits)) =
            Concurrency::with_runtime_mut(|rt| runtime_host::jit_result_parts(rt, decoded_result))
        else {
            decode_errors.extend(json_rt::FieldError::under_errors(
                &row_path,
                json_rt::FieldError::one("CSV query Decode callback returned an invalid Result"),
            ));
            continue;
        };
        if !ok {
            let errors = result_errors(decoded_result).unwrap_or_else(|| {
                json_rt::FieldError::one("CSV query Decode callback returned an invalid error")
            });
            decode_errors.extend(json_rt::FieldError::under_errors(&row_path, errors));
            continue;
        }
        decoded.push(bits);
    }
    if !decode_errors.is_empty() {
        return result_err_fields(decode_errors);
    }

    let selected = match data_query_rt::jet_data_query_indices(&trees, &sql) {
        Ok(indices) => indices,
        Err(errors) => return result_err_fields(errors),
    };
    let list = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_list_values(Vec::new()));
    let built = Concurrency::with_runtime_mut(|rt| {
        selected.iter().all(|index| {
            let Some(bits) = decoded.get(*index) else {
                return false;
            };
            if element_kind == CSV_DECODE_FLOAT {
                rt.heap
                    .list_push_float(list, f64::from_bits(*bits))
                    .is_some()
            } else {
                rt.heap.list_push_int(list, *bits as i64).is_some()
            }
        })
    });
    if !built {
        return result_err_fields(json_rt::FieldError::one(
            "CSV query selected row has no decoded value",
        ));
    }
    result_ok(list as u64)
}


fn jet_jit_data_query_rows(rows: i64, sql: i64) -> i64 {
    let Some(json_rt::DataTree::Array(rows)) = read_datatree(rows) else {
        return result_err_fields(json_rt::FieldError::one(
            "analytics query needs an array of rows",
        ));
    };
    let sql = clone_string(sql);
    match data_query_rt::jet_data_query_trees(&rows, &sql) {
        Ok(selected) => result_ok(alloc_datatree(&json_rt::DataTree::Array(selected)) as u64),
        Err(errors) => result_err_fields(errors),
    }
}


fn jet_jit_datatree_field(tree: i64, name: i64) -> i64 {
    let key = clone_string(name);
    let Some(tree) = read_datatree(tree) else {
        return result_err_decode(&key, "invalid DataTree");
    };
    match tree.field(&key) {
        Ok(value) => result_ok(alloc_datatree(&value) as u64),
        Err(errors) => result_err_fields(errors),
    }
}

fn jet_jit_datatree_at(tree: i64, index: i64) -> i64 {
    let Some(tree) = read_datatree(tree) else {
        return result_err_decode(&format!("[{index}]"), "invalid DataTree");
    };
    match tree.at(index) {
        Ok(value) => result_ok(alloc_datatree(&value) as u64),
        Err(errors) => result_err_fields(errors),
    }
}

fn jet_jit_datatree_int(tree: i64) -> i64 {
    let Some(tree) = read_datatree(tree) else {
        return result_err_decode("", "invalid DataTree");
    };
    match tree.int() {
        Ok(value) => result_ok(value as u64),
        Err(errors) => result_err_fields(errors),
    }
}

/// `__jet_Decode for Int`, distinct from strict `DataTree.int()`.
fn jet_jit_datatree_decode_int(tree: i64) -> i64 {
    let Some(tree) = read_datatree(tree) else {
        return result_err_decode("", "invalid DataTree");
    };
    match json_rt::decode_int_with(
        &tree,
        json_rt::jet_int_from_i64,
        json_rt::jet_int_from_str,
    ) {
        Ok(value) => result_ok(value as u64),
        Err(errors) => result_err_fields(errors),
    }
}

/// `__jet_Decode for Char`, with String's shared coercion policy and the
/// Prelude's exact single-scalar error.
fn jet_jit_datatree_decode_char(tree: i64) -> i64 {
    let Some(tree) = read_datatree(tree) else {
        return result_err_decode("", "invalid DataTree");
    };
    let text = match json_rt::decode_string(&tree) {
        Ok(text) => text,
        Err(errors) => return result_err_fields(errors),
    };
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(value), None) => result_ok(value as u64),
        _ => result_err_fields(json_rt::FieldError::one(format!(
            "expected a single Char, found {:?}",
            text
        ))),
    }
}

fn jet_jit_decode_int_range(result: i64, lo: i64, hi: i64, type_name: i64) -> i64 {
    let state = Concurrency::with_runtime_mut(|rt| {
        result
            .checked_sub(1)
            .and_then(|index| rt.results.get(index as usize))
            .copied()
    });
    let Some(value) = state else { return result };
    if !value.ok {
        return result;
    }
    let value = value.bits as i64;
    let type_name = clone_string(type_name);
    let Some(value) = Concurrency::with_runtime_mut(|rt| rt.heap.int_to_i64(value)) else {
        return result_err_decode("", &format!("expected {type_name}, found out-of-range Int"));
    };
    if (lo..=hi).contains(&value) {
        result
    } else {
        result_err_decode("", &format!("expected {type_name}, found out-of-range Int"))
    }
}

/// Apply the shared inline-range kernel to a successful typed DataTree decode.
/// The result handle remains the carrier for both success and failure, so this
/// host only adapts the result arena to the Prelude function.
fn jet_jit_decode_inline_range(result: i64, lo: i64, hi: i64) -> i64 {
    let state = Concurrency::with_runtime_mut(|rt| {
        result
            .checked_sub(1)
            .and_then(|index| rt.results.get(index as usize))
            .copied()
    });
    let Some(value) = state else { return result };
    if !value.ok {
        return result;
    }
    let value = value.bits as i64;
    match inline_range_rt::jet_inline_range_from_int(value, lo, hi) {
        Ok(_) => result,
        Err(reason) => result_err_decode("", &reason),
    }
}

fn jet_jit_decode_f32_range(result: i64) -> i64 {
    let state = Concurrency::with_runtime_mut(|rt| {
        result
            .checked_sub(1)
            .and_then(|index| rt.results.get(index as usize))
            .copied()
    });
    let Some(value) = state else { return result };
    if !value.ok {
        return result;
    }
    let value = f64::from_bits(value.bits);
    match json_rt::check_f32_range(Ok(value)) {
        Ok(_) => result,
        Err(errors) => result_err_fields(errors),
    }
}

fn jet_jit_decode_fixed_len(result: i64, expected: i64) -> i64 {
    let state = Concurrency::with_runtime_mut(|rt| {
        let value = result
            .checked_sub(1)
            .and_then(|index| rt.results.get(index as usize))
            .copied()?;
        let found = value
            .ok
            .then(|| rt.heap.list_len(value.bits as i64).unwrap_or(0));
        Some((value.ok, found))
    });
    let Some((true, Some(found))) = state else {
        return result;
    };
    if found == expected {
        return result;
    }
    result_err_fields(json_rt::fixed_list_length_error(
        found as usize,
        expected as usize,
    ))
}

fn jet_jit_datatree_decode_list_error(tree: i64) -> i64 {
    let reason = read_datatree(tree)
        .map(|tree| {
            format!(
                "expected a list, found {}",
                json_rt::datatree_kind_for(&tree)
            )
        })
        .unwrap_or_else(|| "expected a list, found value".to_string());
    result_err_decode("", &reason)
}

fn jet_jit_datatree_decode_map_error(tree: i64) -> i64 {
    let reason = read_datatree(tree)
        .map(|tree| {
            format!(
                "expected an object, found {}",
                json_rt::datatree_kind_for(&tree)
            )
        })
        .unwrap_or_else(|| "expected an object, found value".to_string());
    result_err_decode("", &reason)
}

fn jet_jit_decode_error_under(result: i64, index: i64) -> i64 {
    let Some(errors) = result_errors(result) else {
        return result;
    };
    result_err_fields(json_rt::FieldError::under_errors(
        &format!("[{index}]"),
        errors,
    ))
}

/// Prefix every member of a resident `Result<T, [FieldError]>` error list
/// with an arbitrary field segment. Success values pass through unchanged.
fn jet_jit_decode_error_under_segment(result: i64, segment: i64) -> i64 {
    let Some(errors) = result_errors(result) else {
        return result;
    };
    let segment = clone_string(segment);
    result_err_fields(json_rt::FieldError::under_errors(&segment, errors))
}

/// Adapter for the canonical `jet_std::FieldError::under` route. MIR passes
/// the borrowed segment before the consumed result; the resident helper above
/// keeps the historical result-first ABI used by its other callers.
fn jet_jit_decode_error_under_prelude(segment: i64, result: i64) -> i64 {
    jet_jit_decode_error_under_segment(result, segment)
}

/// Add one failed list element to the resident `[FieldError]` accumulator.
/// Zero is the SSA-level empty accumulator; every non-zero value is a normal
/// Result error handle. Keeping this operation here makes JIT list traversal
/// an adapter over the same error-list shape as AOT/TIR.
fn jet_jit_decode_error_accumulate(accum: i64, result: i64, index: i64) -> i64 {
    let Some(errors) = result_errors(result) else {
        return accum;
    };
    let mut combined = result_errors(accum).unwrap_or_default();
    combined.extend(json_rt::FieldError::under_errors(
        &format!("[{index}]"),
        errors,
    ));
    result_err_fields(combined)
}

/// Add one failed map value to the resident `[FieldError]` accumulator.
/// Object keys are already strings, so the path is the wire key itself.
fn jet_jit_decode_error_accumulate_segment(accum: i64, result: i64, segment: i64) -> i64 {
    let Some(errors) = result_errors(result) else {
        return accum;
    };
    let mut combined = result_errors(accum).unwrap_or_default();
    let segment = clone_string(segment);
    combined.extend(json_rt::FieldError::under_errors(&segment, errors));
    result_err_fields(combined)
}

fn jet_jit_datatree_decode_union_error() -> i64 {
    result_err_decode("", "value does not match any union member")
}

fn jet_jit_datatree_text(tree: i64) -> i64 {
    let Some(tree) = read_datatree(tree) else {
        return result_err_decode("", "invalid DataTree");
    };
    match tree.text() {
        Ok(value) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value));
            result_ok(sid as u64)
        }
        Err(errors) => result_err_fields(errors),
    }
}

/// `__jet_Decode for String`, distinct from strict `DataTree.text()`.
fn jet_jit_datatree_decode_string(tree: i64) -> i64 {
    let Some(tree) = read_datatree(tree) else {
        return result_err_decode("", "invalid DataTree");
    };
    match json_rt::decode_string(&tree) {
        Ok(value) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value));
            result_ok(sid as u64)
        }
        Err(errors) => result_err_fields(errors),
    }
}

fn jet_jit_datatree_bool(tree: i64) -> i64 {
    let Some(tree) = read_datatree(tree) else {
        return result_err_decode("", "invalid DataTree");
    };
    match json_rt::decode_bool(&tree) {
        Ok(value) => result_ok(u64::from(value)),
        Err(errors) => result_err_fields(errors),
    }
}

fn jet_jit_datatree_float(tree: i64) -> i64 {
    let Some(tree) = read_datatree(tree) else {
        return result_err_decode("", "invalid DataTree");
    };
    match json_rt::decode_float(&tree) {
        Ok(value) => result_ok(value.to_bits()),
        Err(errors) => result_err_fields(errors),
    }
}

/// D-DATATREE-ERGO1=A: the JIT only marshals the resident heap tree into the
/// shared Prelude operation; it does not reimplement scalar policy here.
fn jet_jit_datatree_to_text(tree: i64) -> i64 {
    pack_opt_string(read_datatree(tree).and_then(|tree| tree.to_text()))
}

/// D-DATATREE-ERGO1=A: compare through the shared Prelude tree operation.
fn jet_jit_datatree_equal_unordered(left: i64, right: i64) -> i64 {
    match (read_datatree(left), read_datatree(right)) {
        (Some(left), Some(right)) => i64::from(left.equal_unordered(&right)),
        _ => 0,
    }
}

fn jet_jit_toml_parse(text: i64) -> i64 {
    match json_rt::toml::parse_to_tree(&clone_string(text)) {
        Ok(tree) => result_ok(alloc_datatree(&tree) as u64),
        Err(error) => result_err_encoding(json_rt::EncodingError::new(
            json_rt::EncodingFormat::TOML,
            json_rt::EncodingErrorKind::Syntax,
            0,
            Ok(error.line as i64),
            Err(json_rt::JetAbsent),
            "",
            error.message,
        )),
    }
}

fn jet_jit_toml_to_string(tree: i64) -> i64 {
    let rendered = read_datatree(tree)
        .map(|t| json_rt::toml::render(&t))
        .unwrap_or_else(|| String::new());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

fn jet_jit_yaml_parse(text: i64) -> i64 {
    match yaml_rt::yaml::parse_to_tree(&clone_string(text)) {
        Ok(tree) => result_ok(alloc_datatree(&tree) as u64),
        Err(error) => result_err_encoding(json_rt::EncodingError::new(
            json_rt::EncodingFormat::YAML,
            json_rt::EncodingErrorKind::Syntax,
            0,
            Ok(error.line as i64),
            Err(json_rt::JetAbsent),
            "",
            error.message,
        )),
    }
}

fn jet_jit_yaml_to_string(tree: i64) -> i64 {
    let rendered = read_datatree(tree)
        .map(|t| yaml_rt::yaml::render(&t))
        .unwrap_or_else(|| String::new());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

/// Marshal the resident `EncodingError` record into the one Prelude rendering
/// AOT's `impl JetShow for EncodingError` embeds. The index tables and the
/// Option ABI decode below are this host's ABI adapter; the text ladder is not
/// re-encoded here (I9).
fn jet_jit_encoding_error_show(handle: i64) -> i64 {
    const FORMAT: &[&str] = &["JSON", "JSONL", "CSV", "TOML", "YAML", "XML", "CBOR"];
    const KIND: &[&str] = &["Syntax", "Truncated", "Unsupported", "Limit", "IO", "State"];
    Concurrency::with_runtime_mut(|rt| {
        let format = rt.heap.record_get_int(handle, 0).unwrap_or(0) as usize;
        let kind = rt.heap.record_get_int(handle, 1).unwrap_or(0) as usize;
        let byte_offset = rt.heap.record_get_int(handle, 2).unwrap_or(0);
        let line = rt.heap.record_get_int(handle, 3).unwrap_or(0);
        let column = rt.heap.record_get_int(handle, 4).unwrap_or(0);
        let path_id = rt.heap.record_get_string(handle, 5).unwrap_or(0);
        let reason_id = rt.heap.record_get_string(handle, 6).unwrap_or(0);
        let path = rt.heap.clone_string(path_id).unwrap_or_default();
        let reason = rt.heap.clone_string(reason_id).unwrap_or_default();
        // Option ABI: 0 = None, else bits+1.
        let out = encoding_error_rt::jet_encoding_error_kernel_show(
            FORMAT.get(format).copied().unwrap_or("?"),
            KIND.get(kind).copied().unwrap_or("?"),
            byte_offset,
            (line != 0).then(|| line - 1),
            (column != 0).then(|| column - 1),
            &path,
            &reason,
        );
        rt.heap.alloc_string(out)
    })
}



/// Resident `[FieldError]` rendering for `print(errors)` and `"{errors}"`.
/// Marshals each record out of the heap and calls the one Prelude projection
/// (`jet_field_error_kernel_show`); the `[a, b]` wrapper matches AOT's
/// `impl<T: JetDisplay> JetDisplay for Vec<T>` (Prelude/Core/Values.rs).
fn jet_jit_decode_error_show(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let mut shown = Vec::new();
        let len = rt.heap.list_len(handle).unwrap_or(0);
        for i in 0..len {
            let error = rt.heap.list_get_int(handle, i).unwrap_or(0);
            let path_id = rt.heap.record_get_string(error, 0).unwrap_or(0);
            let reason_id = rt.heap.record_get_string(error, 1).unwrap_or(0);
            let path = rt.heap.clone_string(path_id).unwrap_or_default();
            let reason = rt.heap.clone_string(reason_id).unwrap_or_default();
            shown.push(field_error_rt::jet_field_error_kernel_show(&path, &reason));
        }
        let shown = format!("[{}]", shown.join(", "));
        rt.heap.alloc_string(shown)
    })
}
/// Pack a lowered DataTree literal into the heap record ABI.
pub(crate) fn pack_datatree_host(disc: i64, payload: i64) -> i64 {
    alloc_dt_record(disc, payload)
}

fn jet_jit_bytes_datatree(bytes: i64) -> i64 {
    alloc_dt_record(DT_BYTES, bytes)
}

fn jit_codec_encode_tree(kind: i64, value: i64) -> Result<json_rt::DataTree, String> {
    match kind {
        CODEC_KIND_DATE | CODEC_KIND_LOCAL_DATE => {
            let text = crate::Time::with_time(value, |value| match value {
                TimeValue::Date(date) => Some(codec_rt::jet_codec_date_encode(
                    date.year(),
                    date.month(),
                    date.day(),
                )),
                _ => None,
            });
            text.map(json_rt::DataTree::Text)
                .ok_or_else(|| "invalid Date codec value".to_string())
        }
        CODEC_KIND_LOCAL_TIME => {
            let text = crate::Time::with_time(value, |value| match value {
                TimeValue::LocalTime(time) => Some(codec_rt::jet_codec_local_time_encode(
                    time.hour(),
                    time.minute(),
                    time.second(),
                )),
                _ => None,
            });
            text.map(json_rt::DataTree::Text)
                .ok_or_else(|| "invalid LocalTime codec value".to_string())
        }
        CODEC_KIND_DATETIME => {
            let text = crate::Time::with_time(value, |value| match value {
                TimeValue::DateTime(datetime) => Some(codec_rt::jet_codec_datetime_encode(
                    datetime.unix_seconds_anchor(),
                    datetime.nanosecond() as u32,
                    datetime.is_leap_second(),
                )),
                _ => None,
            });
            text.map(json_rt::DataTree::Text)
                .ok_or_else(|| "invalid DateTime codec value".to_string())
        }
        CODEC_KIND_DURATION => Ok(json_rt::DataTree::Int(codec_rt::jet_codec_duration_encode(
            value,
        ))),
        CODEC_KIND_DECIMAL => {
            let decimal = Concurrency::with_runtime_mut(|rt| {
                let index = value.saturating_sub(1) as usize;
                rt.decimal_values
                    .get(index)
                    .and_then(|decimal| decimal.as_ref())
                    .cloned()
            })
            .ok_or_else(|| "invalid Decimal codec value".to_string())?;
            Ok(json_rt::DataTree::Text(codec_rt::jet_codec_decimal_encode(
                &decimal,
            )))
        }
        _ => Err(format!("unknown codec kind {kind}")),
    }
}

fn jet_jit_codec_encode(kind: i64, value: i64) -> i64 {
    let tree = match jit_codec_encode_tree(kind, value) {
        Ok(tree) => tree,
        Err(error) => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap(&error));
            json_rt::DataTree::Null
        }
    };
    alloc_datatree(&tree)
}

fn jet_codec_encode_typed(value: i64, type_key: i64) -> i64 {
    let type_key = clone_string(type_key);
    let tree = match typed_runtime_descriptor(&type_key) {
        Some(descriptor) => {
            if let Some(kind) = typed_codec_kind(&descriptor) {
                jit_codec_encode_tree(kind, value)
            } else {
                Concurrency::with_runtime_mut(|rt| {
                    Some(crate::Receipt::encode_jit_value(rt, value, &descriptor))
                })
                .unwrap_or_else(|| Err("typed codec encode has no active runtime".to_string()))
            }
        }
        None => Err(format!("typed codec encode has no type `{type_key}`")),
    };
    match tree {
        Ok(tree) => alloc_datatree(&tree),
        Err(message) => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap(&message));
            0
        }
    }
}

fn jet_jit_codec_decode(kind: i64, tree: i64) -> i64 {
    let Some(tree) = read_datatree(tree) else {
        return result_err_decode("", "invalid DataTree");
    };
    match (kind, tree) {
        (CODEC_KIND_DATE | CODEC_KIND_LOCAL_DATE, json_rt::DataTree::Text(text))
        | (CODEC_KIND_DATE | CODEC_KIND_LOCAL_DATE, json_rt::DataTree::TypedText(text)) => {
            match codec_rt::jet_codec_date_decode(&text) {
                Ok((year, month, day)) => result_ok(crate::Time::push(TimeValue::Date(
                    crate::Time::time_rt::JetDate::new(year, month, day),
                )) as u64),
                Err(error) => result_err_decode("", &format!("expected Date: {error}")),
            }
        }
        (CODEC_KIND_LOCAL_TIME, json_rt::DataTree::Text(text))
        | (CODEC_KIND_LOCAL_TIME, json_rt::DataTree::TypedText(text)) => {
            match codec_rt::jet_codec_local_time_decode(&text) {
                Ok((hour, minute, second)) => result_ok(crate::Time::push(TimeValue::LocalTime(
                    crate::Time::time_rt::JetLocalTime::new(hour, minute, second),
                )) as u64),
                Err(error) => result_err_decode("", &format!("expected LocalTime: {error}")),
            }
        }
        (CODEC_KIND_DATETIME, json_rt::DataTree::Text(text))
        | (CODEC_KIND_DATETIME, json_rt::DataTree::TypedText(text)) => {
            match codec_rt::jet_codec_datetime_decode(&text) {
                Ok((secs, nanos, leap_second)) => result_ok(crate::Time::push(TimeValue::DateTime(
                    crate::Time::time_rt::JetDateTime::from_timestamp_ns_with_leap(
                        secs, nanos, leap_second,
                    ),
                )) as u64),
                Err(error) => result_err_decode("", &format!("expected DateTime: {error}")),
            }
        }
        (CODEC_KIND_DURATION, json_rt::DataTree::Int(ns)) => {
            result_ok(codec_rt::jet_codec_duration_decode(ns) as u64)
        }
        (CODEC_KIND_DURATION, json_rt::DataTree::Number(text))
        | (CODEC_KIND_DURATION, json_rt::DataTree::Text(text)) => {
            let decoded =
                jet_foundation::JSONNumber::json_exact_integer_text(&text).and_then(|value| {
                    value
                        .parse::<i64>()
                        .map_err(|_| "Duration is outside the I64 range".to_string())
                });
            match decoded {
                Ok(ns) => result_ok(codec_rt::jet_codec_duration_decode(ns) as u64),
                Err(error) => result_err_decode("", &error),
            }
        }
        (CODEC_KIND_DURATION, json_rt::DataTree::TypedText(text)) => {
            result_err_decode("", &format!("expected Duration, found text {:?}", text))
        }
        (CODEC_KIND_DECIMAL, json_rt::DataTree::Number(text)) => {
            match jet_foundation::Numeric::CtDecimal::from_json_number(&text) {
                Ok(decimal) => result_ok(crate::Numeric::push_decimal(decimal) as u64),
                Err(error) => result_err_decode("", &format!("expected Decimal: {error}")),
            }
        }
        (CODEC_KIND_DECIMAL, json_rt::DataTree::TypedText(text)) => {
            result_err_decode("", &format!("expected Decimal, found text {:?}", text))
        }
        (CODEC_KIND_DECIMAL, json_rt::DataTree::Text(text)) => {
            match codec_rt::jet_codec_decimal_decode_text(&text) {
                Ok(decimal) => result_ok(crate::Numeric::push_decimal(decimal) as u64),
                Err(error) => result_err_decode("", &format!("expected Decimal: {error}")),
            }
        }
        (CODEC_KIND_DECIMAL, json_rt::DataTree::Int(value)) => {
            match codec_rt::jet_codec_decimal_decode_int(value) {
                Ok(decimal) => result_ok(crate::Numeric::push_decimal(decimal) as u64),
                Err(error) => result_err_decode("", &error),
            }
        }
        (CODEC_KIND_DATE | CODEC_KIND_LOCAL_DATE, other) => result_err_decode(
            "",
            &format!(
                "expected Date, found {}",
                json_rt::datatree_kind_for(&other)
            ),
        ),
        (CODEC_KIND_LOCAL_TIME, other) => result_err_decode(
            "",
            &format!(
                "expected LocalTime, found {}",
                json_rt::datatree_kind_for(&other)
            ),
        ),
        (CODEC_KIND_DATETIME, other) => result_err_decode(
            "",
            &format!(
                "expected DateTime, found {}",
                json_rt::datatree_kind_for(&other)
            ),
        ),
        (CODEC_KIND_DURATION, other) => result_err_decode(
            "",
            &format!(
                "expected Duration, found {}",
                json_rt::datatree_kind_for(&other)
            ),
        ),
        (CODEC_KIND_DECIMAL, other) => result_err_decode(
            "",
            &format!(
                "expected Decimal, found {}",
                json_rt::datatree_kind_for(&other)
            ),
        ),
        (_, _) => result_err_decode("", "unknown codec kind"),
    }
}

host_fns! {
    struct EncodingHostFns;
    register: register_encoding_symbols;
    declare: declare_encoding_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::Module;
        let cc = module.target_config().default_call_conv;
        let mut sig_unary = Signature::new(cc);
        sig_unary.params.push(AbiParam::new(types::I64));
        sig_unary.returns.push(AbiParam::new(types::I64));
        let mut sig_nullary = Signature::new(cc);
        sig_nullary.returns.push(AbiParam::new(types::I64));
        let mut sig_binary = Signature::new(cc);
        sig_binary.params.push(AbiParam::new(types::I64));
        sig_binary.params.push(AbiParam::new(types::I64));
        sig_binary.returns.push(AbiParam::new(types::I64));
        let mut sig_ternary = Signature::new(cc);
        for _ in 0..3 {
            sig_ternary.params.push(AbiParam::new(types::I64));
        }
        sig_ternary.returns.push(AbiParam::new(types::I64));
        let mut sig_quaternary = Signature::new(cc);
        for _ in 0..4 {
            sig_quaternary.params.push(AbiParam::new(types::I64));
        }
        sig_quaternary.returns.push(AbiParam::new(types::I64));
        let mut sig_quinary = Signature::new(cc);
        for _ in 0..5 {
            sig_quinary.params.push(AbiParam::new(types::I64));
        }
        sig_quinary.returns.push(AbiParam::new(types::I64));


    }
    hex_encode: "jet_jit_hex_encode" => jet_jit_hex_encode: sig_unary;
    hex_decode: "jet_jit_hex_decode" => jet_jit_hex_decode: sig_unary;
    b64_encode: "jet_jit_b64_encode" => jet_jit_b64_encode: sig_unary;
    b64_encode_url: "jet_jit_b64_encode_url" => jet_jit_b64_encode_url: sig_unary;
    b64_decode: "jet_jit_b64_decode" => jet_jit_b64_decode: sig_unary;
    b64_decode_url: "jet_jit_b64_decode_url" => jet_jit_b64_decode_url: sig_unary;
    base32_encode: "jet_jit_base32_encode" => jet_jit_base32_encode: sig_unary;
    base32_decode: "jet_jit_base32_decode" => jet_jit_base32_decode: sig_unary;
    csv_parse: "jet_jit_csv_parse" => jet_jit_csv_parse: sig_quaternary;
    csv_rows: "jet_jit_csv_rows" => jet_jit_csv_rows: sig_quaternary;
    csv_to_string: "jet_jit_csv_to_string" => jet_jit_csv_to_string: sig_unary;
    csv_tree_to_string: "jet_jit_csv_tree_to_string" => jet_jit_csv_tree_to_string: sig_unary;
    uuid_v4: "jet_jit_uuid_v4" => jet_jit_uuid_v4: sig_nullary;
    uuid_v7: "jet_jit_uuid_v7" => jet_jit_uuid_v7: sig_unary;
    uuid_v5: "jet_jit_uuid_v5" => jet_jit_uuid_v5: sig_binary;
    uuid_parse: "jet_jit_uuid_parse" => jet_jit_uuid_parse: sig_unary;
    json_parse: "jet_jit_json_parse" => jet_jit_json_parse: sig_unary;
    json_parse_ordered: "jet_jit_json_parse_ordered" => jet_jit_json_parse_ordered: sig_unary;
    json_decode: "jet_jit_json_decode" => jet_jit_json_decode: sig_unary;
    json_decode_typed: "jet_jit_json_decode_typed" => jet_jit_json_decode_typed: sig_binary;
    data_json_decode: "jet_data_json_decode" => jet_jit_json_decode_typed: sig_binary;
    db_decode: "jet_jit_db_decode" => jet_jit_db_decode: sig_binary;
    json_to_string: "jet_jit_json_to_string" => jet_jit_json_to_string: sig_unary;
    json_to_string_pretty: "jet_jit_json_to_string_pretty" => jet_jit_json_to_string_pretty: sig_unary;
    json_canonical: "jet_jit_json_canonical" => jet_jit_json_canonical: sig_unary;
    json_canonical_checked: "jet_jit_json_canonical_checked" => jet_jit_json_canonical_checked: sig_binary;
    json_events: "jet_jit_json_events" => jet_jit_json_events: sig_unary;
    jsonl_parse: "jet_jit_jsonl_parse" => jet_jit_jsonl_parse: sig_unary;
    jsonl_to_string: "jet_jit_jsonl_to_string" => jet_jit_jsonl_to_string: sig_unary;
    xml_parse: "jet_jit_xml_parse" => jet_jit_xml_parse: sig_unary;
    xml_to_string: "jet_jit_xml_to_string" => jet_jit_xml_to_string: sig_unary;
    xml_root: "jet_jit_xml_root" => jet_jit_xml_root: sig_unary;
    xml_expanded_name: "jet_jit_xml_expanded_name" => jet_jit_xml_expanded_name: sig_unary;
    xml_attribute: "jet_jit_xml_attribute" => jet_jit_xml_attribute: sig_binary;
    xml_content: "jet_jit_xml_content" => jet_jit_xml_content: sig_unary;
    xml_to_bytes: "jet_jit_xml_to_bytes" => jet_jit_xml_to_bytes: sig_unary;
    xml_project: "jet_jit_xml_project" => jet_jit_xml_project: sig_unary;
    xml_project_bytes: "jet_jit_xml_project_bytes" => jet_jit_xml_project_bytes: sig_unary;
    cbor_to_bytes: "jet_jit_cbor_to_bytes" => jet_jit_cbor_to_bytes: sig_unary;
    cbor_to_bytes_canonical: "jet_jit_cbor_to_bytes_canonical" => jet_jit_cbor_to_bytes_canonical: sig_unary;
    cbor_parse: "jet_jit_cbor_parse" => jet_jit_cbor_parse: sig_unary;
    cbor_parse_options: "jet_jit_cbor_parse_options" => jet_jit_cbor_parse_options: sig_binary;
    cbor_decode_tree: "jet_jit_cbor_decode_tree" => jet_jit_cbor_decode_tree: sig_unary;
    cbor_decode_tree_options: "jet_jit_cbor_decode_tree_options" => jet_jit_cbor_decode_tree_options: sig_binary;
    bytes_datatree: "jet_jit_bytes_datatree" => jet_jit_bytes_datatree: sig_unary;
    codec_encode: "jet_jit_codec_encode" => jet_jit_codec_encode: sig_binary;
    codec_decode: "jet_jit_codec_decode" => jet_jit_codec_decode: sig_binary;
    codec_encode_prelude: "jet_codec_encode" => jet_jit_codec_encode: sig_binary;
    codec_decode_prelude: "jet_codec_decode" => jet_jit_codec_decode: sig_binary;
    codec_decode_typed: "jet_codec_decode_typed" => jet_codec_decode_typed: sig_binary;
    codec_encode_typed: "jet_codec_encode_typed" => jet_codec_encode_typed: sig_binary;
    enc_csv_decode: "jet_jit_enc_csv_decode" => jet_jit_enc_csv_decode: sig_ternary;
    enc_csv_to_string: "jet_jit_enc_csv_to_string" => jet_jit_enc_csv_to_string: sig_binary;
    csv_query_read: "jet_jit_csv_query_read" => jet_jit_csv_query_read: sig_unary;
    enc_csv_query: "jet_jit_enc_csv_query" => jet_jit_enc_csv_query: sig_quaternary;
    csv_decode_scalar: "jet_jit_csv_decode_scalar" => jet_jit_csv_decode_scalar: sig_binary;
    data_query_rows: "jet_jit_data_query_rows" => jet_jit_data_query_rows: sig_binary;
    datatree_field: "jet_jit_datatree_field" => jet_jit_datatree_field: sig_binary;
    datatree_field_prelude: "jet_datatree_field" => jet_jit_datatree_field: sig_binary;
    datatree_at_prelude: "jet_datatree_at" => jet_jit_datatree_at: sig_binary;
    datatree_int_prelude: "jet_datatree_int" => jet_jit_datatree_int: sig_unary;
    datatree_text_prelude: "jet_datatree_text" => jet_jit_datatree_text: sig_unary;
    datatree_bool_prelude: "jet_datatree_bool" => jet_jit_datatree_bool: sig_unary;
    datatree_float_prelude: "jet_datatree_float" => jet_jit_datatree_float: sig_unary;
    datatree_to_text_prelude: "jet_datatree_to_text" => jet_jit_datatree_to_text: sig_unary;
    datatree_equal_unordered_prelude: "jet_datatree_equal_unordered" => jet_jit_datatree_equal_unordered: sig_binary;
    datatree_at: "jet_jit_datatree_at" => jet_jit_datatree_at: sig_binary;
    datatree_int: "jet_jit_datatree_int" => jet_jit_datatree_int: sig_unary;
    datatree_decode_int: "jet_jit_datatree_decode_int" => jet_jit_datatree_decode_int: sig_unary;
    datatree_decode_string: "jet_jit_datatree_decode_string" => jet_jit_datatree_decode_string: sig_unary;
    datatree_decode_char: "jet_jit_datatree_decode_char" => jet_jit_datatree_decode_char: sig_unary;
    decode_int_range: "jet_jit_decode_int_range" => jet_jit_decode_int_range: sig_quaternary;
    decode_inline_range: "jet_jit_decode_inline_range" => jet_jit_decode_inline_range: sig_ternary;
    decode_f32_range: "jet_jit_decode_f32_range" => jet_jit_decode_f32_range: sig_unary;
    decode_fixed_len: "jet_jit_decode_fixed_len" => jet_jit_decode_fixed_len: sig_binary;
    datatree_decode_list_error: "jet_jit_datatree_decode_list_error" => jet_jit_datatree_decode_list_error: sig_unary;
    datatree_decode_map_error: "jet_jit_datatree_decode_map_error" => jet_jit_datatree_decode_map_error: sig_unary;
    decode_error_under: "jet_jit_decode_error_under" => jet_jit_decode_error_under: sig_binary;
    decode_error_under_segment: "jet_jit_decode_error_under_segment" => jet_jit_decode_error_under_segment: sig_binary;
    decode_error_under_prelude: "jet_std::FieldError::under" => jet_jit_decode_error_under_prelude: sig_binary;
    decode_error_accumulate: "jet_jit_decode_error_accumulate" => jet_jit_decode_error_accumulate: sig_ternary;
    decode_error_accumulate_segment: "jet_jit_decode_error_accumulate_segment" => jet_jit_decode_error_accumulate_segment: sig_ternary;
    datatree_decode_union_error: "jet_jit_datatree_decode_union_error" => jet_jit_datatree_decode_union_error: sig_nullary;
    datatree_text: "jet_jit_datatree_text" => jet_jit_datatree_text: sig_unary;
    datatree_bool: "jet_jit_datatree_bool" => jet_jit_datatree_bool: sig_unary;
    datatree_float: "jet_jit_datatree_float" => jet_jit_datatree_float: sig_unary;
    datatree_to_text: "jet_jit_datatree_to_text" => jet_jit_datatree_to_text: sig_unary;
    datatree_equal_unordered: "jet_jit_datatree_equal_unordered" => jet_jit_datatree_equal_unordered: sig_binary;
    datatree_pack: "jet_jit_datatree_pack" => jet_jit_datatree_pack: sig_binary;
    published_schema_empty: "jet_jit_published_schema_empty" => jet_jit_published_schema_empty: sig_nullary;
    published_schema_merge: "jet_jit_published_schema_merge" => jet_jit_published_schema_merge: sig_binary;
    object_from_map: "jet_jit_object_from_map" => jet_jit_object_from_map: sig_unary;
    object_entries_to_map: "jet_jit_object_entries_to_map" => jet_jit_object_entries_to_map: sig_unary;
    data_entries_to_map: "jet_data_entries_to_map" => jet_jit_object_entries_to_map: sig_unary;
    toml_parse: "jet_jit_toml_parse" => jet_jit_toml_parse: sig_unary;
    toml_to_string: "jet_jit_toml_to_string" => jet_jit_toml_to_string: sig_unary;
    yaml_parse: "jet_jit_yaml_parse" => jet_jit_yaml_parse: sig_unary;
    yaml_to_string: "jet_jit_yaml_to_string" => jet_jit_yaml_to_string: sig_unary;
    decode_error_show: "jet_jit_decode_error_show" => jet_jit_decode_error_show: sig_unary;
    encoding_error_show: "jet_jit_encoding_error_show" => jet_jit_encoding_error_show: sig_unary;
    env_decode: "jet_jit_env_decode" => jet_jit_env_decode: sig_quaternary;
    env_config: "jet_jit_env_config" => jet_jit_env_config: sig_ternary;
    env_config_map: "jet_jit_env_config_map" => jet_jit_env_config_map: sig_binary;
}

fn jet_jit_datatree_pack(disc: i64, payload: i64) -> i64 {
    alloc_dt_record(disc, payload)
}

fn jet_jit_published_schema_empty() -> i64 {
    alloc_datatree(&json_rt::DataTree::Object(Vec::new()))
}

fn jet_jit_published_schema_merge(known: i64, original: i64) -> i64 {
    let Some(known) = read_datatree(known) else {
        return 0;
    };
    let Some(original) = read_datatree(original) else {
        return 0;
    };
    alloc_datatree(&json_rt::jet_datatree_merge_wire_order(&known, &original))
}

/// `DataTree.Object(map)` when the payload is a computed Map: snapshot entries in
/// map iteration order (BTree key order) into the ordered pair-list Object ABI.
fn jet_jit_object_from_map(map: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(len) = rt.heap.map_len(map) else {
            rt.set_trap("data object payload is not a map");
            return 0;
        };
        let list = rt.heap.alloc_empty_list();
        for i in 0..len {
            let k = rt.heap.map_key_at(map, i).unwrap_or(0);
            let v = rt.heap.map_value_at(map, i).unwrap_or(0);
            let rec = rt.heap.alloc_record(2);
            let _ = rt.heap.record_set_int(rec, 0, k);
            let _ = rt.heap.record_set_int(rec, 1, v);
            let _ = rt.heap.list_push_int(list, rec);
        }
        list
    })
}

/// Pattern `if tree == .Object(entries)`: ordered pair list → user-facing Map.
fn jet_jit_object_entries_to_map(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(len) = rt.heap.list_len(list) else {
            rt.set_trap("data object payload is not an entry list");
            return 0;
        };
        let map = rt.heap.alloc_empty_map();
        for i in 0..len {
            let rec = rt.heap.list_get_int(list, i).unwrap_or(0);
            let k = rt.heap.record_get_int(rec, 0).unwrap_or(0);
            let v = rt.heap.record_get_int(rec, 1).unwrap_or(0);
            let _ = rt.heap.map_insert(map, k, v);
        }
        map
    })
}

fn decode_under_diag(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        message.into(),
        "the interpreter decode error adapter rejected the checked operation".to_string(),
        "report this as a compiler bug".to_string(),
        Some(span),
    )
}

fn decode_under_ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    _resolved_ret: Option<Type>,
    _sink: Option<&mut jet_codegen::Comptime::DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    if module != "core.encoding" || method != "decode_under" {
        return None;
    }
    let mut args = args.into_iter();
    let segment = match args.next() {
        Some(CtValue::Str(segment)) => segment,
        Some(_) => {
            return Some(Err(decode_under_diag(
                "core.encoding.decode_under expects a String segment",
                span,
            )));
        }
        None => {
            return Some(Err(decode_under_diag(
                "core.encoding.decode_under received no segment",
                span,
            )));
        }
    };
    let result = match args.next() {
        Some(result) => result,
        None => {
            return Some(Err(decode_under_diag(
                "core.encoding.decode_under received no Result",
                span,
            )));
        }
    };
    if args.next().is_some() {
        return Some(Err(decode_under_diag(
            "core.encoding.decode_under received extra arguments",
            span,
        )));
    }
    let result = match result {
        CtValue::Present(value) => CtValue::Present(value),
        CtValue::Failed(CtReport::Told(error)) => CtValue::failed(Box::new(
            jet_codegen::Comptime::decode_error_under(&segment, *error),
        )),
        _ => {
            return Some(Err(decode_under_diag(
                "core.encoding.decode_under received a non-Result value",
                span,
            )));
        }
    };
    Some(Ok(result))
}

fn data_entries_to_map_diag(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        message.into(),
        "the interpreter DataTree map adapter rejected the checked operation".to_string(),
        "report this as a compiler bug".to_string(),
        Some(span),
    )
}

fn data_entries_to_map_ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    _resolved_ret: Option<Type>,
    _sink: Option<&mut jet_codegen::Comptime::DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    if module != "core.collections" || method != "entries_to_map" {
        return None;
    }
    let [payload] = args.as_slice() else {
        return Some(Err(data_entries_to_map_diag(
            "core.collections.entries_to_map received the wrong number of arguments",
            span,
        )));
    };
    let entries = match payload {
        CtValue::Map(entries) => entries.clone(),
        CtValue::Struct { type_name, fields } if type_name == "JSONObject" => {
            let mut entries = std::collections::BTreeMap::new();
            for (key, value) in fields {
                entries.insert(CtKey::Str(key.clone()), value.clone());
            }
            entries
        }
        _ => {
            return Some(Err(data_entries_to_map_diag(
                "core.collections.entries_to_map received a non-object payload",
                span,
            )));
        }
    };
    Some(Ok(CtValue::Map(entries)))
}

pub(crate) fn register_interpreter_ambient(
    context: &mut crate::InterpreterAmbientContext,
) {
    context.register_core_call(decode_under_ambient_core_call);
}

