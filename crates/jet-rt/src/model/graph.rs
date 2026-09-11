//! ONNX wire-format trust checks shared by native and browser execution.
//! This is not an operator engine. ORT still owns ONNX semantics. External
//! tensors are bound to checked package bytes so neither host can open a file.
//! Schema: https://github.com/onnx/onnx/blob/v1.19.0/onnx/onnx.proto

use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
enum Message { Model, Graph, Node, Attribute, Tensor, Sparse, Function, Training }

struct Field<'a> {
    number: u64,
    wire: u64,
    raw: &'a [u8],
    value: &'a [u8],
}

fn varint(bytes: &mut &[u8]) -> Result<u64, String> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let (&byte, rest) = bytes.split_first().ok_or("truncated protobuf integer")?;
        *bytes = rest;
        if shift == 63 && byte > 1 { return Err("protobuf integer overflow".into()); }
        value |= u64::from(byte & 127) << shift;
        if byte & 128 == 0 { return Ok(value); }
    }
    Err("protobuf integer overflow".into())
}

fn fields(mut bytes: &[u8]) -> Result<Vec<Field<'_>>, String> {
    let mut result = Vec::new();
    while !bytes.is_empty() {
        let start = bytes;
        let key = varint(&mut bytes)?;
        if key >> 3 == 0 || key >> 3 > 0x1fff_ffff { return Err("invalid protobuf field".into()); }
        let wire = key & 7;
        let value = match wire {
            0 => {
                let start = bytes;
                varint(&mut bytes)?;
                &start[..start.len() - bytes.len()]
            }
            1 | 2 | 5 => {
                let len = match wire {
                    1 => 8,
                    5 => 4,
                    _ => usize::try_from(varint(&mut bytes)?).map_err(|_| "protobuf length overflow")?,
                };
                if len > bytes.len() { return Err("truncated protobuf field".into()); }
                let (value, rest) = bytes.split_at(len);
                bytes = rest;
                value
            }
            _ => return Err("unsupported protobuf wire type".into()),
        };
        result.push(Field { number: key >> 3, wire, raw: &start[..start.len() - bytes.len()], value });
    }
    Ok(result)
}

fn text<'a>(fields: &[Field<'a>], number: u64) -> Result<Option<&'a str>, String> {
    let mut values = fields.iter().filter(|f| f.number == number);
    let Some(field) = values.next() else { return Ok(None); };
    if values.next().is_some() || field.wire != 2 { return Err("ambiguous protobuf string field".into()); }
    std::str::from_utf8(field.value).map(Some).map_err(|_| "invalid ONNX UTF-8".into())
}

fn emit_varint(out: &mut Vec<u8>, mut value: u64) {
    while value > 127 {
        out.push(value as u8 | 128);
        value >>= 7;
    }
    out.push(value as u8);
}

fn emit_bytes(out: &mut Vec<u8>, number: u64, bytes: &[u8]) {
    emit_varint(out, number << 3 | 2);
    emit_varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

pub(super) fn prepare(
    package: &ModelPackage,
    policy: &OnnxRuntimePolicy,
    artifacts: &[PreparedArtifact],
) -> Result<Option<Vec<u8>>, ModelError> {
    transform(Message::Model, &artifacts[0].bytes, package, policy, artifacts, 0)
        .map_err(|reason| provenance_error(&package.package, reason))
}

/// Extract the graph's operator closure before provider initialization. The
/// caller must have already authenticated the graph bytes against the package
/// descriptor; this helper only projects names for the explicit CPU policy.
pub(super) fn operator_names(bytes: &[u8]) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    collect_operator_names(Message::Model, bytes, &mut names, 0)?;
    if names.is_empty() {
        return Err("ONNX graph declares no operators".into());
    }
    Ok(names)
}

fn collect_operator_names(
    kind: Message,
    bytes: &[u8],
    names: &mut BTreeSet<String>,
    depth: usize,
) -> Result<(), String> {
    if depth > 128 {
        return Err("ONNX graph nesting exceeds the host stack bound".into());
    }
    let rows = fields(bytes)?;
    if matches!(kind, Message::Node) {
        let op = text(&rows, 4)?.ok_or("ONNX node has no operator")?;
        let domain = text(&rows, 7)?.unwrap_or("");
        let name = if domain.is_empty() || domain == "ai.onnx" {
            op.to_string()
        } else {
            format!("{domain}::{op}")
        };
        names.insert(name);
    }
    for row in rows {
        let nested = match (kind, row.number) {
            (Message::Model, 7) | (Message::Attribute, 6 | 11) | (Message::Training, 1 | 2) => {
                Some(Message::Graph)
            }
            (Message::Model, 20) => Some(Message::Training),
            (Message::Model, 25) => Some(Message::Function),
            (Message::Graph, 1) | (Message::Function, 7) => Some(Message::Node),
            (Message::Node, 5) | (Message::Function, 11) => Some(Message::Attribute),
            (Message::Graph, 5)
            | (Message::Attribute, 5 | 10)
            | (Message::Sparse, 1 | 2) => Some(Message::Tensor),
            (Message::Graph, 15) | (Message::Attribute, 22 | 23) => Some(Message::Sparse),
            _ => None,
        };
        if let Some(nested) = nested {
            if row.wire != 2 {
                return Err("invalid ONNX nested message".into());
            }
            collect_operator_names(nested, row.value, names, depth + 1)?;
        }
    }
    Ok(())
}

fn transform(
    kind: Message,
    bytes: &[u8],
    package: &ModelPackage,
    policy: &OnnxRuntimePolicy,
    artifacts: &[PreparedArtifact],
    depth: usize,
) -> Result<Option<Vec<u8>>, String> {
    // Bound recursive protobuf nesting independently of model tensor sizes.
    if depth > 128 { return Err("ONNX graph nesting exceeds the host stack bound".into()); }
    let rows = fields(bytes)?;
    if matches!(kind, Message::Model) && rows.iter().filter(|f| f.number == 7).count() != 1 {
        return Err("ONNX ModelProto must contain one graph".into());
    }
    if matches!(kind, Message::Node) {
        let op = text(&rows, 4)?.ok_or("ONNX node has no operator")?;
        let domain = text(&rows, 7)?.unwrap_or("");
        let name = if domain.is_empty() || domain == "ai.onnx" {
            op.to_string()
        } else { format!("{domain}::{op}") };
        if !policy.enabled_operators.contains(&name) {
            return Err(format!("ONNX operator `{name}` is outside the pinned operator closure"));
        }
    }
    if matches!(kind, Message::Tensor) {
        let external = rows.iter().any(|f| f.number == 13);
        let locations: Vec<_> = rows.iter().filter(|f| f.number == 14).collect();
        if locations.len() > 1 { return Err("ambiguous ONNX tensor data location".into()); }
        let location = match locations.first() {
            Some(f) if f.wire == 0 => { let mut value = f.value; varint(&mut value)? },
            Some(_) => return Err("invalid ONNX tensor data location".into()),
            None => 0,
        };
        if external || location != 0 {
            if !external || location != 1 { return Err("invalid ONNX external tensor declaration".into()); }
            if rows.iter().any(|f| matches!(f.number, 4 | 5 | 6 | 7 | 9 | 10 | 11)) {
                return Err("external ONNX tensor also contains inline data".into());
            }
            let mut entries = BTreeMap::new();
            for row in rows.iter().filter(|f| f.number == 13) {
                if row.wire != 2 { return Err("invalid ONNX external data field".into()); }
                let pair = fields(row.value)?;
                let key = text(&pair, 1)?.ok_or("external tensor entry has no key")?;
                let value = text(&pair, 2)?.ok_or("external tensor entry has no value")?;
                if entries.insert(key, value).is_some() { return Err("duplicate external tensor entry".into()); }
            }
            let location = *entries.get("location").ok_or("external tensor has no location")?;
            let path = Path::new(location);
            if location.is_empty() || location.contains(':') || location.contains('\\')
                || location.bytes().any(|b| b.is_ascii_control()) || path.is_absolute()
                || path.components().any(|c| !matches!(c, Component::Normal(_))) {
                return Err("external tensor location is not a package-relative file".into());
            }
            let graph_dir = Path::new(&package.artifacts[0].path).parent().unwrap_or(Path::new(""));
            let resolved = graph_dir.join(path);
            let index = package.artifacts.iter().position(|a| Path::new(&a.path) == resolved)
                .ok_or_else(|| format!("external tensor `{location}` is not a declared package artifact"))?;
            if index == 0 { return Err("graph cannot be its own external tensor data".into()); }
            let data = &artifacts[index].bytes;
            let integer = |key| -> Result<Option<usize>, String> {
                entries.get(key).map(|v| {
                    if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) { return Err(format!("invalid external tensor {key}")); }
                    v.parse::<usize>().map_err(|_| format!("external tensor {key} overflows"))
                }).transpose()
            };
            let offset = integer("offset")?.unwrap_or(0);
            let length = integer("length")?.unwrap_or_else(|| data.len().saturating_sub(offset));
            let end = offset.checked_add(length).ok_or("external tensor byte range overflows")?;
            let data = data.get(offset..end).ok_or("external tensor byte range exceeds checked artifact")?;
            let mut out = Vec::with_capacity(bytes.len().saturating_add(data.len()));
            for row in &rows {
                if !matches!(row.number, 13 | 14) { out.extend_from_slice(row.raw); }
            }
            emit_bytes(&mut out, 9, data);
            return Ok(Some(out));
        }
    }
    let mut out: Option<Vec<u8>> = None;
    let mut consumed = 0;
    for row in rows {
        let nested = match (kind, row.number) {
            (Message::Model, 7) | (Message::Attribute, 6 | 11) | (Message::Training, 1 | 2) => Some(Message::Graph),
            (Message::Model, 20) => Some(Message::Training),
            (Message::Model, 25) => Some(Message::Function),
            (Message::Graph, 1) | (Message::Function, 7) => Some(Message::Node),
            (Message::Node, 5) | (Message::Function, 11) => Some(Message::Attribute),
            (Message::Graph, 5) | (Message::Attribute, 5 | 10) | (Message::Sparse, 1 | 2) => Some(Message::Tensor),
            (Message::Graph, 15) | (Message::Attribute, 22 | 23) => Some(Message::Sparse),
            _ => None,
        };
        let replacement = if let Some(nested) = nested {
            if row.wire != 2 { return Err("invalid ONNX nested message".into()); }
            transform(nested, row.value, package, policy, artifacts, depth + 1)?
        } else { None };
        if let Some(replacement) = replacement {
            let output = out.get_or_insert_with(|| bytes[..consumed].to_vec());
            emit_bytes(output, row.number, &replacement);
        } else if let Some(output) = &mut out {
            output.extend_from_slice(row.raw);
        }
        consumed += row.raw.len();
    }
    Ok(out)
}
