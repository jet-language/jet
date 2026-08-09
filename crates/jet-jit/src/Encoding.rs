//! `core.encoding.hex` / `base64` / `base32` / `csv` / `json` and `core.uuid`
//! host shims (#729). Encode mirrors `jet_std_*` in EncodingCodecs.rs; decode
//! calls `jet_foundation::base_encoding_dispatch` (no third algorithm). CSV
//! mirrors `jet_ring_csv_parse` / `jet_ring_csv_render`. JSON parse/render
//! `include!` the canonical `jet_std` parser. UUID mirrors `jet_std_uuid_*`.

use super::Concurrency;
use crate::JetShow;
use jet_foundation::base_encoding_dispatch;
use jet_foundation::PackageEdition;
use jet_foundation::AST::{CtKey, CtValue, Expr, Item, MigrationOp, ProgramBundle, StrPart};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use crate::Marshal::{clone_string, clone_bytes, alloc_byte_list, result_ok, result_err_msg};

mod encoding_base_rt {
    include!("../../jet-codegen/src/Prelude/Core/EncodingBase.rs");
}

/// Canonical `jet_std` JSON/DataTree runtime — adapter types, shared algorithm via include!
pub(crate) mod json_rt {
    #[derive(Clone, Debug, PartialEq)]
    pub struct JSONError {
        pub line: i64,
        pub message: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum JSON {
        Null,
        Boolean(bool),
        Number(f64),
        Text(String),
        Array(Vec<JSON>),
        Object(std::collections::BTreeMap<String, JSON>),
    }

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
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/DataTree.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/JSON.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/TOML.rs");

    /// D-JSON3 coerce walk — same as `jet_std_json_coerce_walk` (MathRandomTime.rs).
    pub fn coerce_walk(value: &JSON, path: &str) -> JSON {
        match value {
            JSON::Text(s) => {
                if s == "true" {
                    emit_coerce(path, "string", "boolean");
                    return JSON::Boolean(true);
                }
                if s == "false" {
                    emit_coerce(path, "string", "boolean");
                    return JSON::Boolean(false);
                }
                if let Ok(n) = s.parse::<f64>() {
                    if n.is_finite() {
                        emit_coerce(path, "string", "number");
                        return JSON::Number(n);
                    }
                }
                value.clone()
            }
            JSON::Object(entries) => {
                let mut out = std::collections::BTreeMap::new();
                for (k, v) in entries {
                    let child = if path.is_empty() {
                        k.clone()
                    } else {
                        format!("{path}.{k}")
                    };
                    out.insert(k.clone(), coerce_walk(v, &child));
                }
                JSON::Object(out)
            }
            JSON::Array(items) => JSON::Array(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let child = if path.is_empty() {
                            format!("[{i}]")
                        } else {
                            format!("{path}[{i}]")
                        };
                        coerce_walk(v, &child)
                    })
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    fn emit_coerce(path: &str, from: &str, to: &str) {
        let field_label = if path.is_empty() { "<root>" } else { path };
        let msg = format!("json coerce: field \"{field_label}\" {from} \u{2192} {to}");
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        // Capture into JitRuntime.stderr (like jit_log_emit / core.io). Raw
        // eprintln! escapes the ProgramOutput buffer and breaks JIT/AOT parity.
        let line = format!("{{\"level\":\"info\",\"body\":\"{msg}\",\"ts\":{ts}}}");
        crate::Concurrency::with_runtime_mut(|rt| {
            rt.stderr.push_str(&line);
            rt.stderr.push('\n');
        });
    }

    pub fn parse_datatree(text: &str) -> Result<DataTree, JSONError> {
        parse_json(text).map(|j| datatree_from_json(&j))
    }

    pub fn decode_lenient(text: &str) -> Result<DataTree, JSONError> {
        let parsed = parse_json(text)?;
        Ok(datatree_from_json(&coerce_walk(&parsed, "")))
    }
}

/// Canonical YAML via build.rs-stripped include (trailing prelude brace removed).
mod yaml_rt {
    use super::json_rt::DataTree;
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!(concat!(env!("OUT_DIR"), "/yaml_std.rs"));
}

/// DataTree heap ABI: record `[disc:i64, payload:i64]` (Float payload = to_bits).
/// Variant order matches sema core_literals: Null,Bool,Int,Float,Text,Array,Object.
const DT_NULL: i64 = 0;
const DT_BOOL: i64 = 1;
const DT_INT: i64 = 2;
const DT_FLOAT: i64 = 3;
const DT_TEXT: i64 = 4;
const DT_ARRAY: i64 = 5;
const DT_OBJECT: i64 = 6;
const DT_BYTES: i64 = 7;

fn alloc_dt_record(disc: i64, payload: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let h = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(h, 0, disc);
        if disc == DT_FLOAT {
            let _ = rt
                .heap
                .record_set_float(h, 1, f64::from_bits(payload as u64));
        } else {
            let _ = rt.heap.record_set_int(h, 1, payload);
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
                let list = rt.heap.alloc_empty_list();
                for h in handles {
                    let _ = rt.heap.list_push_int(list, h);
                }
                list
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
                let list = rt.heap.alloc_empty_list();
                for (k, v) in pairs {
                    let kid = rt.heap.alloc_string(k);
                    let rec = rt.heap.alloc_record(2);
                    let _ = rt.heap.record_set_int(rec, 0, kid);
                    let _ = rt.heap.record_set_int(rec, 1, v);
                    let _ = rt.heap.list_push_int(list, rec);
                }
                list
            });
            alloc_dt_record(DT_OBJECT, list)
        }
    }
}

pub(crate) fn read_datatree(handle: i64) -> Option<json_rt::DataTree> {
    let (disc, payload, text, child_handles, object_pairs, float_val) =
        Concurrency::with_runtime_mut(|rt| {
            let disc = rt.heap.record_get_int(handle, 0)?;
            match disc {
                DT_FLOAT => {
                    let f = rt.heap.record_get_float(handle, 1).unwrap_or(0.0);
                    Some((disc, 0i64, None, None, None, Some(f)))
                }
                DT_TEXT => {
                    let payload = rt.heap.record_get_int(handle, 1)?;
                    let s = rt.heap.clone_string(payload).unwrap_or_default();
                    Some((disc, payload, Some(s), None, None, None))
                }
                DT_ARRAY => {
                    let payload = rt.heap.record_get_int(handle, 1)?;
                    let len = rt.heap.list_len(payload).unwrap_or(0);
                    let mut items = Vec::with_capacity(len as usize);
                    for i in 0..len {
                        items.push(rt.heap.list_get_int(payload, i).unwrap_or(0));
                    }
                    Some((disc, payload, None, Some(items), None, None))
                }
                DT_BYTES => {
                    let payload = rt.heap.record_get_int(handle, 1)?;
                    let len = rt.heap.list_len(payload).unwrap_or(0);
                    let mut bytes = Vec::with_capacity(len as usize);
                    for i in 0..len {
                        bytes.push(rt.heap.list_get_int(payload, i).unwrap_or(0));
                    }
                    Some((disc, payload, None, Some(bytes), None, None))
                }
                DT_OBJECT => {
                    let payload = rt.heap.record_get_int(handle, 1)?;
                    let len = rt.heap.list_len(payload).unwrap_or(0);
                    let mut pairs = Vec::with_capacity(len as usize);
                    for i in 0..len {
                        let rec = rt.heap.list_get_int(payload, i).unwrap_or(0);
                        let k = rt.heap.record_get_int(rec, 0).unwrap_or(0);
                        let v = rt.heap.record_get_int(rec, 1).unwrap_or(0);
                        let ks = rt.heap.clone_string(k).unwrap_or_default();
                        pairs.push((ks, v));
                    }
                    Some((disc, payload, None, None, Some(pairs), None))
                }
                _ => {
                    let payload = rt.heap.record_get_int(handle, 1).unwrap_or(0);
                    Some((disc, payload, None, None, None, None))
                }
            }
        })?;
    match disc {
        DT_NULL => Some(json_rt::DataTree::Null),
        DT_BOOL => Some(json_rt::DataTree::Bool(payload != 0)),
        DT_INT => Some(json_rt::DataTree::Int(payload)),
        DT_FLOAT => Some(json_rt::DataTree::Float(float_val.unwrap_or(0.0))),
        DT_TEXT => Some(json_rt::DataTree::Text(text.unwrap_or_default())),
        DT_ARRAY => {
            let items = child_handles.unwrap_or_default();
            Some(json_rt::DataTree::Array(
                items.into_iter().filter_map(read_datatree).collect(),
            ))
        }
        DT_BYTES => Some(json_rt::DataTree::Bytes(
            child_handles
                .unwrap_or_default()
                .into_iter()
                .map(|byte| byte as u8)
                .collect(),
        )),
        DT_OBJECT => {
            let pairs = object_pairs.unwrap_or_default();
            Some(json_rt::DataTree::Object(
                pairs
                    .into_iter()
                    .filter_map(|(k, v)| read_datatree(v).map(|t| (k, t)))
                    .collect(),
            ))
        }
        _ => None,
    }
}

fn result_err_fields(errors: Vec<json_rt::FieldError>) -> i64 {
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

fn result_errors(result: i64) -> Option<Vec<json_rt::FieldError>> {
    Concurrency::with_runtime_mut(|rt| {
        let value = result
            .checked_sub(1)
            .and_then(|index| rt.results.get(index as usize))
            .copied()?;
        if value.ok {
            return None;
        }
        let len = rt.heap.list_len(value.bits as i64).unwrap_or(0);
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

extern "C" fn jet_jit_hex_encode(bytes: i64) -> i64 {
    let encoded = hex_encode(&clone_bytes(bytes));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(encoded))
}

extern "C" fn jet_jit_hex_decode(text: i64) -> i64 {
    match hex_decode(&clone_string(text)) {
        Ok(bytes) => result_ok(alloc_byte_list(&bytes) as u64),
        Err(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_b64_encode(bytes: i64) -> i64 {
    let encoded = b64_encode(&clone_bytes(bytes));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(encoded))
}

extern "C" fn jet_jit_b64_encode_url(bytes: i64) -> i64 {
    let encoded = b64url_encode(&clone_bytes(bytes));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(encoded))
}

extern "C" fn jet_jit_b64_decode(text: i64) -> i64 {
    let edition = PackageEdition::package_edition();
    match base_encoding_dispatch::decode_base64(&edition, &clone_string(text), false, false) {
        Ok(bytes) => result_ok(alloc_byte_list(&bytes) as u64),
        Err(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_b64_decode_url(text: i64) -> i64 {
    let edition = PackageEdition::package_edition();
    match base_encoding_dispatch::decode_base64url(&edition, &clone_string(text), false, false)
    {
        Ok(bytes) => result_ok(alloc_byte_list(&bytes) as u64),
        Err(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_base32_encode(bytes: i64) -> i64 {
    let encoded = base32_encode(&clone_bytes(bytes));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(encoded))
}

extern "C" fn jet_jit_base32_decode(text: i64) -> i64 {
    let edition = PackageEdition::package_edition();
    match base_encoding_dispatch::decode_base32(
        &edition,
        &clone_string(text),
        false,
        false,
        false,
    ) {
        Ok(bytes) => result_ok(alloc_byte_list(&bytes) as u64),
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
    match csv_parse(&clone_string(text)) {
        Ok(rows) => result_ok(alloc_string_rows(rows) as u64),
        Err(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_csv_to_string(rows: i64) -> i64 {
    let rendered = csv_render(&clone_string_rows(rows));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

/// Typed `csv.to_string([T])` where `T` is `#Codable`: the encoded `DataTree` is
/// an array of flat objects. Header comes from the first row's keys, then one
/// record per element. Mirrors AOT `jet_enc_csv_to_string` cell for cell.
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

extern "C" fn jet_jit_csv_tree_to_string(tree: i64) -> i64 {
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

// #1481 core.uuid: parse/v5 mirror the AOT Prelude's `jet_uuid_bytes`/
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

extern "C" fn jet_jit_uuid_parse(text: i64) -> i64 {
    match uuid_bytes(&clone_string(text)) {
        Ok(bytes) => {
            let normalized = uuid_format(&bytes);
            Concurrency::with_runtime_mut(|rt| result_ok(rt.heap.alloc_string(normalized) as u64))
        }
        Err(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_uuid_v5(namespace: i64, name: i64) -> i64 {
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

// ── JSON / DataTree (core.encoding.json) ─────────────────────────────────────

extern "C" fn jet_jit_json_parse(text: i64) -> i64 {
    match json_rt::parse_datatree(&clone_string(text)) {
        Ok(tree) => result_ok(alloc_datatree(&tree) as u64),
        Err(e) => result_err_msg(&format!("invalid JSON (line {}): {}", e.line, e.message)),
    }
}

extern "C" fn jet_jit_json_decode(text: i64) -> i64 {
    match json_rt::decode_lenient(&clone_string(text)) {
        Ok(tree) => result_ok(alloc_datatree(&tree) as u64),
        Err(e) => result_err_msg(&format!("invalid JSON (line {}): {}", e.line, e.message)),
    }
}

extern "C" fn jet_jit_json_to_string(tree: i64) -> i64 {
    let rendered = read_datatree(tree)
        .map(|t| json_rt::render_datatree_json(&t, false, 0))
        .unwrap_or_else(|| "null".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

extern "C" fn jet_jit_json_to_string_pretty(tree: i64) -> i64 {
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

extern "C" fn jet_jit_json_canonical(tree: i64) -> i64 {
    let rendered = read_datatree(tree)
        .map(|t| render_canonical(&t))
        .unwrap_or_else(|| "null".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

/// D-JSONCANON1=A edition 2027 — marshall to Prelude `jet_enc_json_canonical`.
extern "C" fn jet_jit_json_canonical_checked(tree: i64, limits: i64) -> i64 {
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

extern "C" fn jet_jit_json_events(tree: i64) -> i64 {
    let rendered = read_datatree(tree)
        .map(|t| json_events(&t))
        .unwrap_or_else(|| String::new());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

/// `core.encoding.jsonl.parse` — same as `jet_std_jsonl_parse`.
extern "C" fn jet_jit_jsonl_parse(text: i64) -> i64 {
    let src = clone_string(text);
    let mut handles = Vec::new();
    for (idx, line) in src.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match json_rt::parse_datatree(trimmed) {
            Ok(tree) => handles.push(alloc_datatree(&tree)),
            Err(e) => {
                return result_err_msg(&format!(
                    "invalid JSON (line {}): {}",
                    idx as i64 + e.line,
                    e.message
                ));
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
extern "C" fn jet_jit_jsonl_to_string(rows: i64) -> i64 {
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
        Value::Array(xs) => json_rt::DataTree::Array(xs.into_iter().map(xml_value_to_datatree).collect()),
        Value::Object(es) => json_rt::DataTree::Object(
            es.into_iter()
                .map(|(k, v)| (k, xml_value_to_datatree(v)))
                .collect(),
        ),
    }
}

fn datatree_to_xml_value(tree: &json_rt::DataTree) -> Result<jet_foundation::XmlPull::Value, String> {
    use jet_foundation::XmlPull::Value;
    match tree {
        json_rt::DataTree::Null => Ok(Value::Null),
        json_rt::DataTree::Bool(b) => Ok(Value::Bool(*b)),
        json_rt::DataTree::Int(n) => Ok(Value::Int(*n)),
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

fn xml_err_msg(e: jet_foundation::XmlPull::Error) -> String {
    let path = if e.path.is_empty() { "$" } else { e.path.as_str() };
    format!("XML error at {path}: {}", e.reason)
}

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

extern "C" fn jet_jit_xml_parse(text: i64) -> i64 {
    match jet_foundation::XmlKernel::parse_document(&clone_string(text)) {
        Ok(value) => {
            result_ok(alloc_datatree(&xml_value_to_datatree(value)) as u64)
        }
        Err(e) => result_err_msg(&xml_err_msg(e)),
    }
}

extern "C" fn jet_jit_xml_to_string(tree: i64) -> i64 {
    let rendered = read_datatree(tree)
        .and_then(|t| datatree_to_xml_value(&t).ok())
        .and_then(|v| jet_foundation::XmlKernel::render_document(&v).ok())
        .unwrap_or_default();
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

fn xml_tree_value(tree: i64) -> Result<jet_foundation::XmlPull::Value, String> {
    let dt = read_datatree(tree).ok_or_else(|| "invalid DataTree".to_string())?;
    datatree_to_xml_value(&dt)
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
extern "C" fn jet_jit_xml_root(tree: i64) -> i64 {
    match xml_tree_value(tree).and_then(|v| {
        jet_foundation::XmlKernel::document_root(&v).map_err(xml_err_msg)
    }) {
        Ok(root) => result_ok(alloc_datatree(&xml_value_to_datatree(root)) as u64),
        Err(e) => result_err_msg(&e),
    }
}

/// `xml.expanded_name` → Result[(raw, prefix?, local, namespace_uri?), XMLError].
extern "C" fn jet_jit_xml_expanded_name(tree: i64) -> i64 {
    match xml_tree_value(tree).and_then(|v| {
        jet_foundation::XmlKernel::expanded_name_parts(&v).map_err(xml_err_msg)
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
        Err(e) => result_err_msg(&e),
    }
}

/// `xml.attribute` → Result[String?, XMLError] (Option packed as 0 / sid+1).
extern "C" fn jet_jit_xml_attribute(tree: i64, name: i64) -> i64 {
    let key = clone_string(name);
    match xml_tree_value(tree).and_then(|v| {
        jet_foundation::XmlKernel::lookup_attribute(&v, &key).map_err(xml_err_msg)
    }) {
        Ok(opt) => result_ok(pack_opt_string(opt) as u64),
        Err(e) => result_err_msg(&e),
    }
}

/// `xml.content` → Result[[DataTree], XMLError].
extern "C" fn jet_jit_xml_content(tree: i64) -> i64 {
    match xml_tree_value(tree).and_then(|v| {
        jet_foundation::XmlKernel::element_content(&v).map_err(xml_err_msg)
    }) {
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
        Err(e) => result_err_msg(&e),
    }
}

/// `xml.to_bytes` with XMLRenderOptions::safe (UTF-8 + PreserveValid).
extern "C" fn jet_jit_xml_to_bytes(tree: i64) -> i64 {
    match xml_tree_value(tree).and_then(|v| {
        jet_foundation::XmlKernel::render_document_bytes(
            &v,
            jet_foundation::XmlPull::RenderEncoding::UTF8,
            jet_foundation::XmlPull::LexicalPolicy::PreserveValid,
        )
        .map_err(xml_err_msg)
    }) {
        Ok(bytes) => result_ok(alloc_byte_list(&bytes) as u64),
        Err(e) => result_err_msg(&e),
    }
}

/// Parse + `project_document_for_decode` — front half of typed `xml.decode`.
extern "C" fn jet_jit_xml_project(text: i64) -> i64 {
    match jet_foundation::XmlKernel::parse_document(&clone_string(text)) {
        Ok(value) => {
            match jet_foundation::XmlKernel::project_document_for_decode(&value) {
                Ok(projected) => {
                    result_ok(alloc_datatree(&xml_value_to_datatree(projected)) as u64)
                }
                Err(e) => result_err_fields(xml_decode_fields(e)),
            }
        }
        Err(e) => result_err_fields(xml_decode_fields(e)),
    }
}

/// Parse bytes + project — front half of typed `xml.decode_bytes`.
extern "C" fn jet_jit_xml_project_bytes(bytes: i64) -> i64 {
    let input = clone_bytes(bytes);
    match jet_foundation::XmlKernel::parse_document_bytes(&input) {
        Ok(value) => {
            match jet_foundation::XmlKernel::project_document_for_decode(&value) {
                Ok(projected) => {
                    result_ok(alloc_datatree(&xml_value_to_datatree(projected)) as u64)
                }
                Err(e) => result_err_fields(xml_decode_fields(e)),
            }
        }
        Err(e) => result_err_fields(xml_decode_fields(e)),
    }
}

// ── CBOR (mirrors EncodingCodecs jet_cbor_* for DataTree; safe options) ─────

fn cbor_push_len(out: &mut Vec<u8>, major: u8, n: u64) {
    if n < 24 {
        out.push((major << 5) | n as u8);
    } else if n <= u8::MAX as u64 {
        out.extend_from_slice(&[(major << 5) | 24, n as u8]);
    } else if n <= u16::MAX as u64 {
        out.push((major << 5) | 25);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else if n <= u32::MAX as u64 {
        out.push((major << 5) | 26);
        out.extend_from_slice(&(n as u32).to_be_bytes());
    } else {
        out.push((major << 5) | 27);
        out.extend_from_slice(&n.to_be_bytes());
    }
}

fn cbor_f32_to_half_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 255) as i32;
    let frac = bits & 0x7fffff;
    if exp == 255 {
        return sign | 0x7c00 | if frac == 0 { 0 } else { 0x0200 };
    }
    let half_exp = exp - 127 + 15;
    if half_exp >= 31 {
        return sign | 0x7c00;
    }
    if half_exp <= 0 {
        if half_exp < -10 {
            return sign;
        }
        let mant = frac | 0x800000;
        let shift = (14 - half_exp) as u32;
        let mut rounded = mant >> shift;
        let rem = mant & ((1u32 << shift) - 1);
        let halfway = 1u32 << (shift - 1);
        if rem > halfway || (rem == halfway && rounded & 1 != 0) {
            rounded += 1;
        }
        return sign | rounded as u16;
    }
    let mut rounded = frac >> 13;
    let rem = frac & 0x1fff;
    if rem > 0x1000 || (rem == 0x1000 && rounded & 1 != 0) {
        rounded += 1;
    }
    if rounded == 0x0400 {
        return sign | (((half_exp + 1) as u16) << 10);
    }
    sign | ((half_exp as u16) << 10) | rounded as u16
}

fn cbor_half_exact(value: f64) -> Option<u16> {
    if value.is_nan() {
        return Some(0x7e00);
    }
    let narrowed = value as f32;
    if (narrowed as f64).to_bits() != value.to_bits() {
        return None;
    }
    let bits = cbor_f32_to_half_bits(narrowed);
    (cbor_half_to_f64(bits).to_bits() == value.to_bits()).then_some(bits)
}

fn cbor_push_preferred_float(out: &mut Vec<u8>, value: f64) {
    if let Some(bits) = cbor_half_exact(value) {
        out.push(0xf9);
        out.extend_from_slice(&bits.to_be_bytes());
    } else if ((value as f32) as f64).to_bits() == value.to_bits() {
        out.push(0xfa);
        out.extend_from_slice(&(value as f32).to_bits().to_be_bytes());
    } else {
        out.push(0xfb);
        out.extend_from_slice(&value.to_bits().to_be_bytes());
    }
}

fn cbor_encode_val(
    v: &json_rt::DataTree,
    out: &mut Vec<u8>,
    canonical: bool,
) -> Result<(), String> {
    match v {
        json_rt::DataTree::Null => out.push(0xf6),
        json_rt::DataTree::Bool(false) => out.push(0xf4),
        json_rt::DataTree::Bool(true) => out.push(0xf5),
        json_rt::DataTree::Int(n) if *n >= 0 => cbor_push_len(out, 0, *n as u64),
        json_rt::DataTree::Int(n) => cbor_push_len(out, 1, (-1 - *n) as u64),
        json_rt::DataTree::Float(f) => cbor_push_preferred_float(out, *f),
        json_rt::DataTree::Text(s) => {
            cbor_push_len(out, 3, s.len() as u64);
            out.extend_from_slice(s.as_bytes());
        }
        json_rt::DataTree::Bytes(bs) => {
            cbor_push_len(out, 2, bs.len() as u64);
            out.extend_from_slice(bs);
        }
        json_rt::DataTree::Array(xs) => {
            cbor_push_len(out, 4, xs.len() as u64);
            for x in xs {
                cbor_encode_val(x, out, canonical)?;
            }
        }
        json_rt::DataTree::Object(es) => {
            let mut encoded = Vec::with_capacity(es.len());
            for (k, v) in es {
                let mut key = Vec::new();
                cbor_encode_val(
                    &json_rt::DataTree::Text(k.clone()),
                    &mut key,
                    canonical,
                )?;
                let mut value = Vec::new();
                cbor_encode_val(v, &mut value, canonical)?;
                encoded.push((key, value));
            }
            if canonical {
                encoded.sort_by(|a, b| a.0.cmp(&b.0));
            }
            if encoded.windows(2).any(|pair| pair[0].0 == pair[1].0) {
                return Err("duplicate encoded CBOR map key".to_string());
            }
            cbor_push_len(out, 5, encoded.len() as u64);
            for (key, value) in encoded {
                out.extend_from_slice(&key);
                out.extend_from_slice(&value);
            }
        }
    }
    Ok(())
}

fn cbor_read_len(input: &[u8], i: &mut usize, add: u8) -> Result<u64, String> {
    let need = match add {
        n @ 0..=23 => return Ok(n as u64),
        24 => 1,
        25 => 2,
        26 => 4,
        27 => 8,
        _ => return Err("indefinite/reserved CBOR length unsupported".to_string()),
    };
    if *i + need > input.len() {
        return Err("CBOR length argument is truncated".to_string());
    }
    let mut n = 0u64;
    for _ in 0..need {
        n = (n << 8) | input[*i] as u64;
        *i += 1;
    }
    Ok(n)
}

fn cbor_decode_val(input: &[u8], i: &mut usize, depth: i64) -> Result<json_rt::DataTree, String> {
    const MAX_DEPTH: i64 = 256;
    if depth > MAX_DEPTH {
        return Err(format!("max_depth {MAX_DEPTH} exceeded"));
    }
    if *i >= input.len() {
        return Err("CBOR value is missing".to_string());
    }
    let b = input[*i];
    *i += 1;
    let major = b >> 5;
    let add = b & 31;
    match major {
        0 => i64::try_from(cbor_read_len(input, i, add)?)
            .map(json_rt::DataTree::Int)
            .map_err(|_| "CBOR integer outside Jet Int".to_string()),
        1 => i64::try_from(cbor_read_len(input, i, add)?)
            .ok()
            .and_then(|n| n.checked_neg()?.checked_sub(1))
            .map(json_rt::DataTree::Int)
            .ok_or_else(|| "CBOR integer outside Jet Int".to_string()),
        2 => {
            let n = usize::try_from(cbor_read_len(input, i, add)?)
                .map_err(|_| "CBOR byte string too large".to_string())?;
            if n > input.len() - *i {
                return Err("CBOR byte string truncated".to_string());
            }
            let bytes = input[*i..*i + n].to_vec();
            *i += n;
            Ok(json_rt::DataTree::Bytes(bytes))
        }
        3 => {
            let n = usize::try_from(cbor_read_len(input, i, add)?)
                .map_err(|_| "CBOR text too large".to_string())?;
            if n > input.len() - *i {
                return Err("CBOR text truncated".to_string());
            }
            let s = std::str::from_utf8(&input[*i..*i + n])
                .map_err(|_| "CBOR text is not UTF-8".to_string())?
                .to_string();
            *i += n;
            Ok(json_rt::DataTree::Text(s))
        }
        4 => {
            let n = usize::try_from(cbor_read_len(input, i, add)?)
                .map_err(|_| "CBOR array too large".to_string())?;
            let mut xs = Vec::with_capacity(n);
            for _ in 0..n {
                xs.push(cbor_decode_val(input, i, depth + 1)?);
            }
            Ok(json_rt::DataTree::Array(xs))
        }
        5 => {
            let n = usize::try_from(cbor_read_len(input, i, add)?)
                .map_err(|_| "CBOR map too large".to_string())?;
            let mut es = Vec::with_capacity(n);
            for _ in 0..n {
                let k = match cbor_decode_val(input, i, depth + 1)? {
                    json_rt::DataTree::Text(s) => s,
                    _ => return Err("CBOR map key must be text".to_string()),
                };
                let v = cbor_decode_val(input, i, depth + 1)?;
                es.push((k, v));
            }
            Ok(json_rt::DataTree::Object(es))
        }
        7 => match add {
            20 => Ok(json_rt::DataTree::Bool(false)),
            21 => Ok(json_rt::DataTree::Bool(true)),
            22 => Ok(json_rt::DataTree::Null),
            25 => {
                if *i + 2 > input.len() {
                    return Err("CBOR Float16 truncated".to_string());
                }
                let bits = u16::from_be_bytes([input[*i], input[*i + 1]]);
                *i += 2;
                Ok(json_rt::DataTree::Float(cbor_half_to_f64(bits)))
            }
            26 => {
                if *i + 4 > input.len() {
                    return Err("CBOR Float32 truncated".to_string());
                }
                let mut buf = [0u8; 4];
                buf.copy_from_slice(&input[*i..*i + 4]);
                *i += 4;
                Ok(json_rt::DataTree::Float(f32::from_be_bytes(buf) as f64))
            }
            27 => {
                if *i + 8 > input.len() {
                    return Err("CBOR Float64 truncated".to_string());
                }
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&input[*i..*i + 8]);
                *i += 8;
                Ok(json_rt::DataTree::Float(f64::from_bits(u64::from_be_bytes(
                    buf,
                ))))
            }
            _ => Err(format!("unsupported CBOR simple value {add}")),
        },
        6 => Err("CBOR tags are unsupported".to_string()),
        _ => Err(format!("unsupported CBOR major type {major}")),
    }
}

fn cbor_half_to_f64(bits: u16) -> f64 {
    let sign = ((bits >> 15) as u64) << 63;
    let exp = (bits >> 10) & 31;
    let frac = bits & 1023;
    if exp == 0 {
        if frac == 0 {
            return f64::from_bits(sign);
        }
        let mut mant = frac as u64;
        let mut exponent = -14i32;
        while mant & 1024 == 0 {
            mant <<= 1;
            exponent -= 1;
        }
        mant &= 1023;
        f64::from_bits(sign | (((exponent + 1023) as u64) << 52) | (mant << 42))
    } else if exp == 31 {
        f64::from_bits(sign | (0x7ffu64 << 52) | ((frac as u64) << 42))
    } else {
        f64::from_bits(sign | (((exp as i32 - 15 + 1023) as u64) << 52) | ((frac as u64) << 42))
    }
}

extern "C" fn jet_jit_cbor_to_bytes(tree: i64) -> i64 {
    jet_jit_cbor_to_bytes_impl(tree, false)
}

extern "C" fn jet_jit_cbor_to_bytes_canonical(tree: i64) -> i64 {
    jet_jit_cbor_to_bytes_impl(tree, true)
}

fn jet_jit_cbor_to_bytes_impl(tree: i64, canonical: bool) -> i64 {
    match read_datatree(tree) {
        Some(t) => {
            let mut out = Vec::new();
            match cbor_encode_val(&t, &mut out, canonical) {
                Ok(()) => result_ok(alloc_byte_list(&out) as u64),
                Err(e) => result_err_msg(&e),
            }
        }
        None => result_err_msg("invalid DataTree"),
    }
}

fn cbor_ct_datatree(value: &CtValue) -> Option<json_rt::DataTree> {
    match value {
        CtValue::Bytes(bytes) => Some(json_rt::DataTree::Bytes(bytes.clone())),
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if matches!(
            type_name.as_str(),
            "DataTree" | "JSON" | "TOML" | "YAML" | "CSV"
        ) =>
        {
            let payload = args.first().map(|(_, value)| value);
            match (variant.as_str(), payload) {
                ("Null", _) => Some(json_rt::DataTree::Null),
                ("Bool", Some(CtValue::Bool(value))) => {
                    Some(json_rt::DataTree::Bool(*value))
                }
                ("Int", Some(CtValue::Int(value))) => Some(json_rt::DataTree::Int(*value)),
                ("Float", Some(CtValue::Float(value))) => {
                    Some(json_rt::DataTree::Float(value.as_f64()))
                }
                ("Text", Some(CtValue::Str(value))) => {
                    Some(json_rt::DataTree::Text(value.clone()))
                }
                ("Array", Some(CtValue::List(values))) => values
                    .iter()
                    .map(cbor_ct_datatree)
                    .collect::<Option<Vec<_>>>()
                    .map(json_rt::DataTree::Array),
                ("Object", Some(CtValue::Map(entries))) => entries
                    .iter()
                    .map(|(key, value)| {
                        let CtKey::Str(key) = key else {
                            return None;
                        };
                        Some((key.clone(), cbor_ct_datatree(value)?))
                    })
                    .collect::<Option<Vec<_>>>()
                    .map(json_rt::DataTree::Object),
                (
                    "Object",
                    Some(CtValue::Struct {
                        type_name,
                        fields,
                    }),
                ) if type_name == "JSONObject" => fields
                    .iter()
                    .map(|(key, value)| Some((key.clone(), cbor_ct_datatree(value)?)))
                    .collect::<Option<Vec<_>>>()
                    .map(json_rt::DataTree::Object),
                _ => None,
            }
        }
        _ => None,
    }
}

fn cbor_error_parts(value: CtValue) -> (i64, i64, String, String) {
    let CtValue::Struct { fields, .. } = value else {
        return (
            2,
            0,
            "$".to_string(),
            "CBOR parse failed".to_string(),
        );
    };
    let mut kind = 2;
    let mut byte_offset = 0;
    let mut path = "$".to_string();
    let mut reason = "CBOR parse failed".to_string();
    for (name, value) in fields {
        match (name.as_str(), value) {
            (
                "kind",
                CtValue::Enum {
                    variant,
                    args,
                    ..
                },
            ) if args.is_empty() => {
                kind = match variant.as_str() {
                    "Syntax" => 0,
                    "Truncated" => 1,
                    "Unsupported" => 2,
                    "Limit" => 3,
                    "TypeMismatch" => 4,
                    "TrailingData" => 5,
                    "NonCanonical" => 6,
                    _ => 2,
                };
            }
            ("byte_offset", CtValue::Int(offset)) => byte_offset = offset,
            ("path", CtValue::Str(value)) => path = value,
            ("reason", CtValue::Str(value)) => reason = value,
            _ => {}
        }
    }
    (kind, byte_offset, path, reason)
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

fn result_err_cbor(value: CtValue) -> i64 {
    let (kind, byte_offset, path, reason) = cbor_error_parts(value);
    result_err_cbor_parts(kind, byte_offset, path, reason)
}

fn cbor_decode_fields(value: CtValue) -> Vec<json_rt::FieldError> {
    let (_, byte_offset, path, reason) = cbor_error_parts(value);
    json_rt::FieldError::at(
        decode_path(&path),
        format!("CBOR at byte {byte_offset}: {reason}"),
    )
}

fn jet_jit_cbor_parse_impl(bytes: i64, options: Option<i64>, allow_bytes: bool) -> i64 {
    let input = clone_bytes(bytes);
    let options = options.map(|handle| {
        let (max_depth, max_items, max_bytes, require_canonical) =
            Concurrency::with_runtime_mut(|rt| {
                (
                    rt.heap.record_get_int(handle, 0).unwrap_or(256),
                    rt.heap.record_get_int(handle, 1).unwrap_or(1_000_000),
                    rt.heap
                        .record_get_int(handle, 2)
                        .unwrap_or(1_073_741_824),
                    rt.heap.record_get_bool(handle, 3).unwrap_or(false),
                )
            });
        CtValue::Struct {
            type_name: "CBOROptions".to_string(),
            fields: vec![
                ("max_depth".to_string(), CtValue::Int(max_depth)),
                ("max_items".to_string(), CtValue::Int(max_items)),
                ("max_bytes".to_string(), CtValue::Int(max_bytes)),
                (
                    "require_canonical".to_string(),
                    CtValue::Bool(require_canonical),
                ),
            ],
        }
    });
    match jet_codegen::Comptime::cbor_parse_for_tir(&input, options.as_ref(), allow_bytes) {
        Ok(tree) => match cbor_ct_datatree(&tree) {
            Some(tree) => result_ok(alloc_datatree(&tree) as u64),
            None if allow_bytes => result_err_fields(json_rt::FieldError::one(
                "invalid DataTree from CBOR parser",
            )),
            None => result_err_cbor_parts(
                2,
                0,
                "$".to_string(),
                "invalid DataTree from CBOR parser".to_string(),
            ),
        },
        Err(error) if allow_bytes => result_err_fields(cbor_decode_fields(error)),
        Err(error) => result_err_cbor(error),
    }
}

extern "C" fn jet_jit_cbor_parse(bytes: i64) -> i64 {
    jet_jit_cbor_parse_impl(bytes, None, false)
}

extern "C" fn jet_jit_cbor_parse_options(bytes: i64, options: i64) -> i64 {
    jet_jit_cbor_parse_impl(bytes, Some(options), false)
}

extern "C" fn jet_jit_cbor_decode_tree(bytes: i64) -> i64 {
    jet_jit_cbor_parse_impl(bytes, None, true)
}

extern "C" fn jet_jit_cbor_decode_tree_options(bytes: i64, options: i64) -> i64 {
    jet_jit_cbor_parse_impl(bytes, Some(options), true)
}

/// CSV typed decode front half: header+rows → `[DataTree]` of Text-cell objects
/// (mirrors `jet_enc_csv_decode` before `T::jet_decode`).
extern "C" fn jet_jit_csv_decode_trees(text: i64) -> i64 {
    match csv_parse(&clone_string(text)) {
        Ok(rows) => {
            let mut it = rows.into_iter();
            let Some(header) = it.next() else {
                let empty = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_list());
                return result_ok(empty as u64);
            };
            let mut handles = Vec::new();
            for row in it {
                let obj: Vec<(String, json_rt::DataTree)> = header
                    .iter()
                    .enumerate()
                    .map(|(c, name)| {
                        let cell = row.get(c).cloned().unwrap_or_default();
                        (name.clone(), json_rt::DataTree::Text(cell))
                    })
                    .collect();
                handles.push(alloc_datatree(&json_rt::DataTree::Object(obj)));
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
        Err(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_datatree_field(tree: i64, name: i64) -> i64 {
    let key = clone_string(name);
    let Some(tree) = read_datatree(tree) else {
        return result_err_decode(&key, "invalid DataTree");
    };
    match tree.field(&key) {
        Ok(value) => result_ok(alloc_datatree(&value) as u64),
        Err(errors) => result_err_fields(errors),
    }
}

extern "C" fn jet_jit_datatree_at(tree: i64, index: i64) -> i64 {
    let Some(tree) = read_datatree(tree) else {
        return result_err_decode(&format!("[{index}]"), "invalid DataTree");
    };
    match tree.at(index) {
        Ok(value) => result_ok(alloc_datatree(&value) as u64),
        Err(errors) => result_err_fields(errors),
    }
}

extern "C" fn jet_jit_datatree_int(tree: i64) -> i64 {
    let Some(tree) = read_datatree(tree) else {
        return result_err_decode("", "invalid DataTree");
    };
    match tree.int() {
        Ok(value) => result_ok(value as u64),
        Err(errors) => result_err_fields(errors),
    }
}

/// `user_Decode for Int`, distinct from strict `DataTree.int()`.
extern "C" fn jet_jit_datatree_decode_int(tree: i64) -> i64 {
    let Some(tree) = read_datatree(tree) else {
        return result_err_decode("", "invalid DataTree");
    };
    match json_rt::decode_int(&tree) {
        Ok(value) => result_ok(value as u64),
        Err(errors) => result_err_fields(errors),
    }
}

extern "C" fn jet_jit_decode_int_range(
    result: i64,
    lo: i64,
    hi: i64,
    type_name: i64,
) -> i64 {
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
    match json_rt::check_int_range(Ok(value), lo, hi, &type_name) {
        Ok(_) => result,
        Err(errors) => result_err_fields(errors),
    }
}

extern "C" fn jet_jit_decode_f32_range(result: i64) -> i64 {
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

extern "C" fn jet_jit_decode_fixed_len(result: i64, expected: i64) -> i64 {
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

extern "C" fn jet_jit_datatree_decode_list_error(tree: i64) -> i64 {
    let reason = read_datatree(tree)
        .map(|tree| format!("expected a list, found {}", json_rt::datatree_kind(&tree)))
        .unwrap_or_else(|| "expected a list, found value".to_string());
    result_err_decode("", &reason)
}

extern "C" fn jet_jit_decode_error_under(result: i64, index: i64) -> i64 {
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
extern "C" fn jet_jit_decode_error_under_segment(result: i64, segment: i64) -> i64 {
    let Some(errors) = result_errors(result) else {
        return result;
    };
    let segment = clone_string(segment);
    result_err_fields(json_rt::FieldError::under_errors(&segment, errors))
}

/// Add one failed list element to the resident `[FieldError]` accumulator.
/// Zero is the SSA-level empty accumulator; every non-zero value is a normal
/// Result error handle. Keeping this operation here makes JIT list traversal
/// an adapter over the same error-list shape as AOT/TIR.
extern "C" fn jet_jit_decode_error_accumulate(accum: i64, result: i64, index: i64) -> i64 {
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

extern "C" fn jet_jit_datatree_decode_union_error() -> i64 {
    result_err_decode("", "value does not match any union member")
}

extern "C" fn jet_jit_datatree_text(tree: i64) -> i64 {
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

extern "C" fn jet_jit_datatree_bool(tree: i64) -> i64 {
    let Some(tree) = read_datatree(tree) else {
        return result_err_decode("", "invalid DataTree");
    };
    match json_rt::decode_bool(&tree) {
        Ok(value) => result_ok(u64::from(value)),
        Err(errors) => result_err_fields(errors),
    }
}

extern "C" fn jet_jit_datatree_float(tree: i64) -> i64 {
    let Some(tree) = read_datatree(tree) else {
        return result_err_decode("", "invalid DataTree");
    };
    match json_rt::decode_float(&tree) {
        Ok(value) => result_ok(value.to_bits()),
        Err(errors) => result_err_fields(errors),
    }
}

extern "C" fn jet_jit_toml_parse(text: i64) -> i64 {
    match json_rt::toml::parse_to_tree(&clone_string(text)) {
        Ok(tree) => result_ok(alloc_datatree(&tree) as u64),
        Err(e) => result_err_msg(&format!("invalid TOML (line {}): {}", e.line, e.message)),
    }
}

extern "C" fn jet_jit_toml_to_string(tree: i64) -> i64 {
    let rendered = read_datatree(tree)
        .map(|t| json_rt::toml::render(&t))
        .unwrap_or_else(|| String::new());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

extern "C" fn jet_jit_yaml_parse(text: i64) -> i64 {
    match yaml_rt::yaml::parse_to_tree(&clone_string(text)) {
        Ok(tree) => result_ok(alloc_datatree(&tree) as u64),
        Err(e) => result_err_msg(&format!("invalid YAML (line {}): {}", e.line, e.message)),
    }
}

extern "C" fn jet_jit_yaml_to_string(tree: i64) -> i64 {
    let rendered = read_datatree(tree)
        .map(|t| yaml_rt::yaml::render(&t))
        .unwrap_or_else(|| String::new());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered))
}

/// Mirror `jet_std::EncodingError::display_text` for `{err}` interp.
extern "C" fn jet_jit_encoding_error_show(handle: i64) -> i64 {
    const FORMAT: &[&str] = &["JSON", "JSONL", "CSV", "XML", "CBOR"];
    const KIND: &[&str] = &["Syntax", "Truncated", "Unsupported", "Limit", "IO", "State"];
    let text = Concurrency::with_runtime_mut(|rt| {
        let format = rt.heap.record_get_int(handle, 0).unwrap_or(0) as usize;
        let kind = rt.heap.record_get_int(handle, 1).unwrap_or(0) as usize;
        let byte_offset = rt.heap.record_get_int(handle, 2).unwrap_or(0);
        let line = rt.heap.record_get_int(handle, 3).unwrap_or(0);
        let column = rt.heap.record_get_int(handle, 4).unwrap_or(0);
        let path_id = rt.heap.record_get_string(handle, 5).unwrap_or(0);
        let reason_id = rt.heap.record_get_string(handle, 6).unwrap_or(0);
        let path = rt.heap.clone_string(path_id).unwrap_or_default();
        let reason = rt.heap.clone_string(reason_id).unwrap_or_default();
        let format_s = FORMAT.get(format).copied().unwrap_or("?");
        let kind_s = KIND.get(kind).copied().unwrap_or("?");
        let mut out = format!("{format_s} {kind_s} at byte {byte_offset}");
        // Option ABI: 0 = None, else bits+1.
        if line != 0 {
            out.push_str(&format!(", line {}", line - 1));
        }
        if column != 0 {
            out.push_str(&format!(", column {}", column - 1));
        }
        if !path.is_empty() {
            out.push_str(&format!(", path {path}"));
        }
        out.push_str(&format!(": {reason}"));
        rt.heap.alloc_string(out)
    });
    text
}

/// Mirror `jet_std::FieldError` list rendering for resident `print(errors)`.
extern "C" fn jet_jit_decode_error_show(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let mut shown = Vec::new();
        let len = rt.heap.list_len(handle).unwrap_or(0);
        for i in 0..len {
            let error = rt.heap.list_get_int(handle, i).unwrap_or(0);
            let path_id = rt.heap.record_get_string(error, 0).unwrap_or(0);
            let reason_id = rt.heap.record_get_string(error, 1).unwrap_or(0);
            let path = rt.heap.clone_string(path_id).unwrap_or_default();
            let reason = rt.heap.clone_string(reason_id).unwrap_or_default();
            shown.push(if path.is_empty() {
                reason
            } else {
                format!("at `{path}`: {reason}")
            });
        }
        let shown = format!("[{}]", shown.join(", "));
        rt.heap.alloc_string(shown)
    })
}

/// Pack a lowered DataTree/JSON enum lit into the heap record ABI.
pub(crate) fn pack_datatree_host(disc: i64, payload: i64) -> i64 {
    alloc_dt_record(disc, payload)
}

extern "C" fn jet_jit_bytes_datatree(bytes: i64) -> i64 {
    alloc_dt_record(DT_BYTES, bytes)
}

host_fns! {
    struct EncodingHostFns;
    register: register_encoding_symbols;
    declare: declare_encoding_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::{Linkage, Module};
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


    }
    hex_encode: "jet_jit_hex_encode" => jet_jit_hex_encode: sig_unary;
    hex_decode: "jet_jit_hex_decode" => jet_jit_hex_decode: sig_unary;
    b64_encode: "jet_jit_b64_encode" => jet_jit_b64_encode: sig_unary;
    b64_encode_url: "jet_jit_b64_encode_url" => jet_jit_b64_encode_url: sig_unary;
    b64_decode: "jet_jit_b64_decode" => jet_jit_b64_decode: sig_unary;
    b64_decode_url: "jet_jit_b64_decode_url" => jet_jit_b64_decode_url: sig_unary;
    base32_encode: "jet_jit_base32_encode" => jet_jit_base32_encode: sig_unary;
    base32_decode: "jet_jit_base32_decode" => jet_jit_base32_decode: sig_unary;
    csv_parse: "jet_jit_csv_parse" => jet_jit_csv_parse: sig_unary;
    csv_to_string: "jet_jit_csv_to_string" => jet_jit_csv_to_string: sig_unary;
    csv_tree_to_string: "jet_jit_csv_tree_to_string" => jet_jit_csv_tree_to_string: sig_unary;
    uuid_v4: "jet_jit_uuid_v4" => jet_jit_uuid_v4: sig_nullary;
    uuid_v7: "jet_jit_uuid_v7" => jet_jit_uuid_v7: sig_unary;
    uuid_v5: "jet_jit_uuid_v5" => jet_jit_uuid_v5: sig_binary;
    uuid_parse: "jet_jit_uuid_parse" => jet_jit_uuid_parse: sig_unary;
    json_parse: "jet_jit_json_parse" => jet_jit_json_parse: sig_unary;
    json_decode: "jet_jit_json_decode" => jet_jit_json_decode: sig_unary;
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
    csv_decode_trees: "jet_jit_csv_decode_trees" => jet_jit_csv_decode_trees: sig_unary;
    datatree_field: "jet_jit_datatree_field" => jet_jit_datatree_field: sig_binary;
    datatree_at: "jet_jit_datatree_at" => jet_jit_datatree_at: sig_binary;
    datatree_int: "jet_jit_datatree_int" => jet_jit_datatree_int: sig_unary;
    datatree_decode_int: "jet_jit_datatree_decode_int" => jet_jit_datatree_decode_int: sig_unary;
    decode_int_range: "jet_jit_decode_int_range" => jet_jit_decode_int_range: sig_quaternary;
    decode_f32_range: "jet_jit_decode_f32_range" => jet_jit_decode_f32_range: sig_unary;
    decode_fixed_len: "jet_jit_decode_fixed_len" => jet_jit_decode_fixed_len: sig_binary;
    datatree_decode_list_error: "jet_jit_datatree_decode_list_error" => jet_jit_datatree_decode_list_error: sig_unary;
    decode_error_under: "jet_jit_decode_error_under" => jet_jit_decode_error_under: sig_binary;
    decode_error_under_segment: "jet_jit_decode_error_under_segment" => jet_jit_decode_error_under_segment: sig_binary;
    decode_error_accumulate: "jet_jit_decode_error_accumulate" => jet_jit_decode_error_accumulate: sig_ternary;
    datatree_decode_union_error: "jet_jit_datatree_decode_union_error" => jet_jit_datatree_decode_union_error: sig_nullary;
    datatree_text: "jet_jit_datatree_text" => jet_jit_datatree_text: sig_unary;
    datatree_bool: "jet_jit_datatree_bool" => jet_jit_datatree_bool: sig_unary;
    datatree_float: "jet_jit_datatree_float" => jet_jit_datatree_float: sig_unary;
    datatree_pack: "jet_jit_datatree_pack" => jet_jit_datatree_pack: sig_binary;
    object_from_map: "jet_jit_object_from_map" => jet_jit_object_from_map: sig_unary;
    object_entries_to_map: "jet_jit_object_entries_to_map" => jet_jit_object_entries_to_map: sig_unary;
    datatree_migrate: "jet_jit_datatree_migrate" => jet_jit_datatree_migrate: sig_binary;
    toml_parse: "jet_jit_toml_parse" => jet_jit_toml_parse: sig_unary;
    toml_to_string: "jet_jit_toml_to_string" => jet_jit_toml_to_string: sig_unary;
    yaml_parse: "jet_jit_yaml_parse" => jet_jit_yaml_parse: sig_unary;
    yaml_to_string: "jet_jit_yaml_to_string" => jet_jit_yaml_to_string: sig_unary;
    decode_error_show: "jet_jit_decode_error_show" => jet_jit_decode_error_show: sig_unary;
    encoding_error_show: "jet_jit_encoding_error_show" => jet_jit_encoding_error_show: sig_unary;
}




extern "C" fn jet_jit_datatree_pack(disc: i64, payload: i64) -> i64 {
    alloc_dt_record(disc, payload)
}

/// `DataTree.Object(map)` when the payload is a computed Map: snapshot entries in
/// map iteration order (BTree key order) into the ordered pair-list Object ABI.
extern "C" fn jet_jit_object_from_map(map: i64) -> i64 {
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
extern "C" fn jet_jit_object_entries_to_map(list: i64) -> i64 {
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

// ── D-MIGRATE3/4: decode_traced + silent migrate for plain decode ────────────
// Mirrors codegen `emit_migration_chain_walker` / step fns using registered
// MigrationDecl metadata (no per-type Rust emit). Literal `add` defaults only.

#[derive(Clone)]
enum MigrateStepOp {
    Rename { from: String, to: String },
    Remove { field: String },
    Add { field: String, value: json_rt::DataTree },
}

#[derive(Clone)]
struct TypeMigration {
    /// Historical shapes oldest-first (v1..vK); current is not included.
    shapes: Vec<BTreeSet<String>>,
    blocks: Vec<Vec<MigrateStepOp>>,
}

thread_local! {
    static TYPE_MIGRATIONS: RefCell<HashMap<String, TypeMigration>> =
        RefCell::new(HashMap::new());
}

fn literal_datatree(expr: &Expr) -> Option<json_rt::DataTree> {
    match expr {
        Expr::Bool(b, _) => Some(json_rt::DataTree::Bool(*b)),
        Expr::Int(n, _, _, _) => Some(json_rt::DataTree::Int(*n)),
        Expr::Float(f, _, _) => Some(json_rt::DataTree::Float(*f)),
        Expr::Char(c, _) => Some(json_rt::DataTree::Text(c.to_string())),
        Expr::Str(parts, _) => {
            let mut s = String::new();
            for p in parts {
                match p {
                    StrPart::Lit(t) => s.push_str(t),
                    StrPart::Interp(_, _) => return None,
                }
            }
            Some(json_rt::DataTree::Text(s))
        }
        _ => None,
    }
}

fn invert_shape(mut shape: BTreeSet<String>, ops: &[MigrateStepOp]) -> BTreeSet<String> {
    // Undo one forward step (codegen migration_shapes order).
    for op in ops {
        match op {
            MigrateStepOp::Rename { from, to } => {
                shape.remove(to);
                shape.insert(from.clone());
            }
            MigrateStepOp::Remove { field } => {
                shape.insert(field.clone());
            }
            MigrateStepOp::Add { field, .. } => {
                shape.remove(field);
            }
        }
    }
    shape
}

/// Collect `#PublishedSchema` migration chains from a checked bundle.
pub fn register_migrations(bundle: &ProgramBundle) {
    let mut by_type: HashMap<String, TypeMigration> = HashMap::new();
    let mut current_fields: HashMap<String, BTreeSet<String>> = HashMap::new();
    for module in &bundle.modules {
        for item in &module.items {
            if let Item::Struct(s) = item {
                let fields: BTreeSet<String> = s.fields.iter().map(|f| f.name.clone()).collect();
                current_fields.insert(s.name.clone(), fields);
            }
        }
    }
    for module in &bundle.modules {
        for item in &module.items {
            let Item::Migration(m) = item else { continue };
            let mut ops = Vec::new();
            let mut ok = true;
            for op in &m.ops {
                match op {
                    MigrationOp::Rename { from, to, .. } => {
                        ops.push(MigrateStepOp::Rename {
                            from: from.clone(),
                            to: to.clone(),
                        });
                    }
                    MigrationOp::Remove { field, .. } => {
                        ops.push(MigrateStepOp::Remove {
                            field: field.clone(),
                        });
                    }
                    MigrationOp::Add { field, default, .. } => {
                        let Some(value) = literal_datatree(default) else {
                            ok = false;
                            break;
                        };
                        ops.push(MigrateStepOp::Add {
                            field: field.clone(),
                            value,
                        });
                    }
                    MigrationOp::Change { .. } => {
                        // Needs converter call — leave type unregistered.
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                by_type.remove(&m.type_name);
                continue;
            }
            by_type
                .entry(m.type_name.clone())
                .or_insert_with(|| TypeMigration {
                    shapes: Vec::new(),
                    blocks: Vec::new(),
                })
                .blocks
                .push(ops);
        }
    }
    // Derive historical shapes (same invert walk as codegen migration_shapes).
    for (name, mig) in by_type.iter_mut() {
        let Some(mut shape) = current_fields.get(name).cloned() else {
            mig.shapes.clear();
            continue;
        };
        let mut shapes = Vec::with_capacity(mig.blocks.len());
        for block in mig.blocks.iter().rev() {
            shape = invert_shape(shape, block);
            shapes.push(shape.clone());
        }
        shapes.reverse(); // oldest first
        mig.shapes = shapes;
    }
    TYPE_MIGRATIONS.with(|slot| {
        *slot.borrow_mut() = by_type;
    });
}

pub fn clear_migrations() {
    TYPE_MIGRATIONS.with(|slot| slot.borrow_mut().clear());
}

fn key_set(tree: &json_rt::DataTree) -> BTreeSet<String> {
    match tree {
        json_rt::DataTree::Object(pairs) => pairs.iter().map(|(k, _)| k.clone()).collect(),
        _ => BTreeSet::new(),
    }
}

fn apply_step(pairs: &mut Vec<(String, json_rt::DataTree)>, ops: &[MigrateStepOp]) {
    for op in ops {
        match op {
            MigrateStepOp::Rename { from, to } => {
                for p in pairs.iter_mut() {
                    if p.0 == *from {
                        p.0 = to.clone();
                    }
                }
            }
            MigrateStepOp::Remove { field } => {
                pairs.retain(|p| p.0 != *field);
            }
            MigrateStepOp::Add { field, value } => {
                pairs.push((field.clone(), value.clone()));
            }
        }
    }
}

/// Try walking an older shape forward. Ok payload = record `[tree, from, steps]`.
extern "C" fn jet_jit_datatree_migrate(type_name: i64, tree: i64) -> i64 {
    let name = clone_string(type_name);
    let Some(src) = read_datatree(tree) else {
        return result_err_msg("invalid DataTree");
    };
    let outcome = TYPE_MIGRATIONS.with(|slot| {
        let guard = slot.borrow();
        let Some(mig) = guard.get(&name) else {
            return Err("no migration");
        };
        if mig.blocks.is_empty() || mig.shapes.len() != mig.blocks.len() {
            return Err("no migration");
        }
        let keys = key_set(&src);
        let k = mig.shapes.len();
        // Newest historical shape first (prefer newest matching version).
        for j in (0..k).rev() {
            if mig.shapes[j] != keys {
                continue;
            }
            let json_rt::DataTree::Object(mut pairs) = src.clone() else {
                return Err("migration needs object");
            };
            let mut steps: Vec<String> = Vec::new();
            for i in j..k {
                apply_step(&mut pairs, &mig.blocks[i]);
                steps.push(format!("v{}->v{}", i + 1, i + 2));
            }
            let migrated = json_rt::DataTree::Object(pairs);
            return Ok((migrated, format!("v{}", j + 1), steps));
        }
        Err("no matching shape")
    });
    match outcome {
        Ok((migrated, from, steps)) => {
            let tree_h = alloc_datatree(&migrated);
            let from_h = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(from));
            let steps_h = Concurrency::with_runtime_mut(|rt| {
                let list = rt.heap.alloc_empty_list();
                for s in steps {
                    let sid = rt.heap.alloc_string(s);
                    let _ = rt.heap.list_push_int(list, sid);
                }
                list
            });
            let rec = Concurrency::with_runtime_mut(|rt| {
                let h = rt.heap.alloc_record(3);
                let _ = rt.heap.record_set_int(h, 0, tree_h);
                let _ = rt.heap.record_set_int(h, 1, from_h);
                let _ = rt.heap.record_set_int(h, 2, steps_h);
                h
            });
            result_ok(rec as u64)
        }
        Err(msg) => result_err_msg(msg),
    }
}
