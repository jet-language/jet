//! Interpreter and resident-host adapters for the shared receipt section kernel.
//!
//! The kernel owns canonicalization, limits, identity, and publication. This
//! module only turns a tier value into the kernel's encoded boundary and wires
//! the interpreter ambient callback.

use jet_foundation::AST::{CtValue, Type};
use jet_foundation::Shape::ShapeProjectionKind;
use jet_foundation::Diagnostics::{Diagnostic, Span};

pub(crate) mod kernel {
    pub(crate) use crate::Encoding::json_rt as jet_std;
    use jet_codegen::development_receipt::ensure_receipt_directory;

    #[allow(non_camel_case_types)]
    pub(crate) trait __jet_Encode {
        fn jet_encode(&self) -> jet_std::DataTree;
    }

    fn jet_sha256_raw(data: &[u8]) -> [u8; 32] {
        crate::Crypto::runtime::jet_crypto_email_sha256_impl(data)
    }

    include!("../../jet-codegen/src/Prelude/Core/Receipt.rs");
}

use crate::Encoding::json_rt::DataTree;
use crate::runtime_host::{
    JitRuntime, RuntimeTypeDescriptor, RuntimeValueAbi, RuntimeValueKind,
};
use std::collections::HashSet;
use jet_rt::JetVal;

const RECEIPT_VALUE_MAX_DEPTH: usize = 128;
const RECEIPT_VALUE_MAX_NODES: usize = 1_000_000;
const RECEIPT_VALUE_MAX_BYTES: usize = 64 * 1024 * 1024;

pub(crate) struct EncodeState {
    nodes: usize,
    bytes: usize,
    active: HashSet<(u64, i64)>,
    callback: Option<HistoryCallableEncoder>,
}

pub(crate) type HistoryCallableEncoder = fn(
    &mut JitRuntime,
    i64,
    &RuntimeTypeDescriptor,
    &mut EncodeState,
    usize,
) -> Result<DataTree, String>;

impl EncodeState {
    pub(crate) fn visit(&mut self, depth: usize) -> Result<(), String> {
        if depth > RECEIPT_VALUE_MAX_DEPTH {
            return Err(format!(
                "receipt value exceeds maximum depth {}",
                RECEIPT_VALUE_MAX_DEPTH
            ));
        }
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| "receipt value node count overflowed".to_string())?;
        if self.nodes > RECEIPT_VALUE_MAX_NODES {
            return Err(format!(
                "receipt value exceeds maximum node count {}",
                RECEIPT_VALUE_MAX_NODES
            ));
        }
        Ok(())
    }
    pub(crate) fn bytes(&mut self, amount: usize) -> Result<(), String> {
        self.bytes = self
            .bytes
            .checked_add(amount)
            .ok_or_else(|| "receipt value byte count overflowed".to_string())?;
        if self.bytes > RECEIPT_VALUE_MAX_BYTES {
            return Err(format!(
                "receipt value exceeds maximum byte count {}",
                RECEIPT_VALUE_MAX_BYTES
            ));
        }
        Ok(())
    }

    pub(crate) fn enter(&mut self, descriptor: &RuntimeTypeDescriptor, raw: i64) -> Result<(), String> {
        if !self.active.insert((descriptor.id, raw)) {
            return Err(format!(
                "receipt value contains a cycle through type `{}`",
                descriptor.name
            ));
        }
        Ok(())
    }

    pub(crate) fn leave(&mut self, descriptor: &RuntimeTypeDescriptor, raw: i64) {
        self.active.remove(&(descriptor.id, raw));
    }
}

enum RuntimeWord {
    Bits(i64),
    Float(f64),
    Bool(bool),
    Char(char),
    String(String),
}

fn lookup_descriptor(
    rt: &JitRuntime,
    type_id: Option<u64>,
    owner: &str,
) -> Result<RuntimeTypeDescriptor, String> {
    let type_id =
        type_id.ok_or_else(|| format!("receipt value `{owner}` has no type descriptor"))?;
    rt.runtime_type_descriptor(type_id)
        .cloned()
        .ok_or_else(|| format!("receipt value `{owner}` has unknown type descriptor {type_id}"))
}

fn bits(word: RuntimeWord, owner: &str) -> Result<i64, String> {
    match word {
        RuntimeWord::Bits(value) => Ok(value),
        RuntimeWord::Float(_)
        | RuntimeWord::Bool(_)
        | RuntimeWord::Char(_)
        | RuntimeWord::String(_) => {
            Err(format!("receipt value `{owner}` has an incompatible runtime carrier"))
        }
    }
}
fn word_from_raw(
    rt: &JitRuntime,
    raw: i64,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<RuntimeWord, String> {
    if descriptor.kind == RuntimeValueKind::String {
        return rt
            .heap
            .clone_string(raw)
            .map(RuntimeWord::String)
            .ok_or_else(|| {
                format!(
                    "receipt value `{}` has an invalid String handle {}",
                    descriptor.name, raw
                )
            });
    }
    Ok(RuntimeWord::Bits(raw))
}

fn word_from_slot(
    rt: &mut JitRuntime,
    value: JetVal,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<RuntimeWord, String> {
    match value {
        JetVal::Int(raw) | JetVal::RecordRef(raw) => word_from_raw(rt, raw, descriptor),
        JetVal::Float(value) if descriptor.kind == RuntimeValueKind::Float => {
            Ok(RuntimeWord::Float(value))
        }
        JetVal::Bool(value) if descriptor.kind == RuntimeValueKind::Bool => {
            Ok(RuntimeWord::Bool(value))
        }
        JetVal::Char(value) if descriptor.kind == RuntimeValueKind::Char => {
            Ok(RuntimeWord::Char(value))
        }
        JetVal::String(value) if descriptor.kind == RuntimeValueKind::String => {
            Ok(RuntimeWord::String(value))
        }
        JetVal::StringView { owner, start, end }
            if descriptor.kind == RuntimeValueKind::String =>
        {
            let value = rt
                .heap
                .get_string(owner)
                .and_then(|text| text.get(start..end))
                .ok_or_else(|| {
                    format!(
                        "receipt value `{}` has an invalid StringView",
                        descriptor.name
                    )
                })?
                .to_string();
            Ok(RuntimeWord::String(value))
        }
        JetVal::ExactInt(value) if descriptor.kind == RuntimeValueKind::Int => {
            let raw = rt
                .heap
                .int_from_str(&value.to_string_rep())
                .map_err(|error| {
                    format!("receipt value `{}` has an invalid Int: {error}", descriptor.name)
                })?;
            Ok(RuntimeWord::Bits(raw))
        }
        JetVal::Record(values)
            if matches!(
                descriptor.kind,
                RuntimeValueKind::Record | RuntimeValueKind::Enum
            ) =>
        {
            Ok(RuntimeWord::Bits(rt.heap.alloc_record_values(values)))
        }
        value => Err(format!(
            "receipt value `{}` has an incompatible runtime slot {:?}",
            descriptor.name, value
        )),
    }
}

pub(crate) fn encode_int(
    rt: &JitRuntime,
    raw: i64,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<DataTree, String> {
    if let Some(width) = descriptor.integer_width {
        return Ok(if width.signed {
            crate::Encoding::json_rt::jet_datatree_encode_i64(raw)
        } else {
            crate::Encoding::json_rt::jet_datatree_encode_u64(raw as u64)
        });
    }
    match rt.heap.int_to_i64(raw) {
        Some(value) => Ok(crate::Encoding::json_rt::jet_datatree_encode_i64(value)),
        None => Ok(DataTree::Number(rt.heap.int_to_string(raw))),
    }
}

fn encode_float(word: RuntimeWord, descriptor: &RuntimeTypeDescriptor) -> Result<DataTree, String> {
    let value = match word {
        RuntimeWord::Float(value) => value,
        RuntimeWord::Bits(value) => match descriptor.abi {
            RuntimeValueAbi::Float => f64::from_bits(value as u64),
            RuntimeValueAbi::Float32 => f32::from_bits(value as u32) as f64,
            _ => {
                return Err(format!(
                    "receipt value `{}` has an incompatible Float carrier",
                    descriptor.name
                ))
            }
        },
        RuntimeWord::Bool(_) | RuntimeWord::Char(_) | RuntimeWord::String(_) => {
            return Err(format!(
                "receipt value `{}` has an incompatible Float carrier",
                descriptor.name
            ))
        }
    };
    if !value.is_finite() {
        return Err(format!(
            "receipt value `{}` contains a non-finite Float",
            descriptor.name
        ));
    }
    Ok(DataTree::Float(value))
}

fn encode_string(
    state: &mut EncodeState,
    word: RuntimeWord,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<DataTree, String> {
    let value = match word {
        RuntimeWord::String(value) => value,
        RuntimeWord::Bits(value) => {
            return Err(format!(
                "receipt value `{}` has an invalid String handle {}",
                descriptor.name, value
            ))
        }
        RuntimeWord::Float(_)
        | RuntimeWord::Bool(_)
        | RuntimeWord::Char(_) => {
            return Err(format!(
                "receipt value `{}` has an incompatible String carrier",
                descriptor.name
            ))
        }
    };
    state.bytes(value.len())?;
    Ok(DataTree::Text(value))
}

fn encode_value(
    rt: &mut JitRuntime,
    word: RuntimeWord,
    descriptor: &RuntimeTypeDescriptor,
    state: &mut EncodeState,
    depth: usize,
) -> Result<DataTree, String> {
    state.visit(depth)?;
    match descriptor.kind {
        RuntimeValueKind::Unit => Ok(DataTree::Null),
        RuntimeValueKind::Int => encode_int(rt, bits(word, &descriptor.name)?, descriptor),
        RuntimeValueKind::Float => encode_float(word, descriptor),
        RuntimeValueKind::Bool => match word {
            RuntimeWord::Bool(value) => Ok(DataTree::Bool(value)),
            RuntimeWord::Bits(value) => Ok(DataTree::Bool(value != 0)),
            RuntimeWord::Float(_)
            | RuntimeWord::Char(_)
            | RuntimeWord::String(_) => Err(format!(
                "receipt value `{}` has an incompatible Bool carrier",
                descriptor.name
            )),
        },
        RuntimeValueKind::Char => {
            let value = match word {
                RuntimeWord::Char(value) => value,
                RuntimeWord::Bits(value) => char::from_u32(value as u32).ok_or_else(|| {
                    format!("receipt value `{}` contains an invalid Char", descriptor.name)
                })?,
                RuntimeWord::Float(_)
                | RuntimeWord::Bool(_)
                | RuntimeWord::String(_) => {
                    return Err(format!(
                        "receipt value `{}` has an incompatible Char carrier",
                        descriptor.name
                    ))
                }
            };
            let value = value.to_string();
            state.bytes(value.len())?;
            Ok(DataTree::Text(value))
        }
        RuntimeValueKind::String => encode_string(state, word, descriptor),
        RuntimeValueKind::List => {
            let raw = bits(word, &descriptor.name)?;
            encode_list(rt, raw, descriptor, state, depth)
        }
        RuntimeValueKind::Map => {
            let raw = bits(word, &descriptor.name)?;
            encode_map(rt, raw, descriptor, state, depth)
        }
        RuntimeValueKind::Shared => {
            let raw = bits(word, &descriptor.name)?;
            state.enter(descriptor, raw)?;
            let result = (|| {
                let value = crate::Memory::shared_value(rt, raw).ok_or_else(|| {
                    format!(
                        "receipt value `{}` has an invalid Shared handle",
                        descriptor.name
                    )
                })?;
                let child = lookup_descriptor(rt, descriptor.element, "Shared value")?;
                encode_value(rt, RuntimeWord::Bits(value), &child, state, depth + 1)
            })();
            state.leave(descriptor, raw);
            result
        }
        RuntimeValueKind::Option => {
            let raw = bits(word, &descriptor.name)?;
            state.enter(descriptor, raw)?;
            let result = encode_result_like(rt, raw, descriptor, descriptor.ok, state, depth);
            state.leave(descriptor, raw);
            result
        }
        RuntimeValueKind::Result => {
            let raw = bits(word, &descriptor.name)?;
            state.enter(descriptor, raw)?;
            let result = encode_result_like(rt, raw, descriptor, descriptor.ok, state, depth);
            state.leave(descriptor, raw);
            result
        }
        RuntimeValueKind::Record => {
            let raw = bits(word, &descriptor.name)?;
            encode_record(rt, raw, descriptor, state, depth)
        }
        RuntimeValueKind::Enum => {
            let raw = bits(word, &descriptor.name)?;
            encode_enum(rt, raw, descriptor, state, depth)
        }
        RuntimeValueKind::Closure => {
            if let Some(callback) = state.callback {
                callback(rt, bits(word, &descriptor.name)?, descriptor, state, depth)
            } else {
                Err(format!(
                    "receipt value `{}` has unsupported runtime kind {:?}",
                    descriptor.name, descriptor.kind
                ))
            }
        }
        RuntimeValueKind::Named
        | RuntimeValueKind::View
        | RuntimeValueKind::Iterator
        | RuntimeValueKind::Handle => Err(format!(
            "receipt value `{}` has unsupported runtime kind {:?}",
            descriptor.name, descriptor.kind
        )),
    }
}

fn encode_result_like(
    rt: &mut JitRuntime,
    raw: i64,
    owner: &RuntimeTypeDescriptor,
    ok_id: Option<u64>,
    state: &mut EncodeState,
    depth: usize,
) -> Result<DataTree, String> {
    let (is_ok, payload) = crate::runtime_host::jit_result_parts(rt, raw)
        .ok_or_else(|| format!("receipt value `{}` has an invalid result handle", owner.name))?;
    if !is_ok {
        return Ok(DataTree::Null);
    }
    let child = lookup_descriptor(rt, ok_id, "result payload")?;
    let word = word_from_raw(rt, payload as i64, &child)?;
    encode_value(rt, word, &child, state, depth + 1)
}

fn encode_byte_list(
    rt: &mut JitRuntime,
    raw: i64,
    owner: &RuntimeTypeDescriptor,
    state: &mut EncodeState,
    depth: usize,
) -> Result<DataTree, String> {
    let length = rt
        .heap
        .list_len(raw)
        .ok_or_else(|| format!("receipt value `{}` has an invalid byte list", owner.name))?;
    let length = usize::try_from(length)
        .map_err(|_| format!("receipt value `{}` has a negative list length", owner.name))?;
    if length > RECEIPT_VALUE_MAX_NODES.saturating_sub(state.nodes) {
        return Err(format!(
            "receipt value exceeds maximum node count {}",
            RECEIPT_VALUE_MAX_NODES
        ));
    }
    let mut bytes = Vec::with_capacity(length);
    for index in 0..length {
        state.visit(depth + 1)?;
        let index = i64::try_from(index).map_err(|_| "receipt byte index overflowed".to_string())?;
        let value = rt
            .heap
            .list_get_int(raw, index)
            .ok_or_else(|| format!("receipt value `{}` has an invalid byte slot", owner.name))?;
        let value = rt
            .heap
            .int_to_i64(value)
            .ok_or_else(|| format!("receipt value `{}` contains an out-of-range byte", owner.name))?;
        bytes.push(
            u8::try_from(value)
                .map_err(|_| format!("receipt value `{}` contains an out-of-range byte", owner.name))?,
        );
    }
    state.bytes(bytes.len())?;
    Ok(DataTree::Bytes(bytes))
}

fn is_byte_descriptor(descriptor: &RuntimeTypeDescriptor) -> bool {
    descriptor.kind == RuntimeValueKind::Int
        && (descriptor.name == "U8" || descriptor.canonical == "U8")
}

fn encode_list(
    rt: &mut JitRuntime,
    raw: i64,
    owner: &RuntimeTypeDescriptor,
    state: &mut EncodeState,
    depth: usize,
) -> Result<DataTree, String> {
    let length = rt
        .heap
        .list_len(raw)
        .ok_or_else(|| format!("receipt value `{}` has an invalid list handle", owner.name))?;
    let length = usize::try_from(length)
        .map_err(|_| format!("receipt value `{}` has a negative list length", owner.name))?;
    let element = lookup_descriptor(rt, owner.element, "list element")?;
    state.enter(owner, raw)?;
    let result = if is_byte_descriptor(&element) {
        encode_byte_list(rt, raw, owner, state, depth)
    } else {
        if length > RECEIPT_VALUE_MAX_NODES.saturating_sub(state.nodes) {
            Err(format!(
                "receipt value exceeds maximum node count {}",
                RECEIPT_VALUE_MAX_NODES
            ))
        } else {
            let slots = rt
                .heap
                .clone_list_values(raw)
                .ok_or_else(|| format!("receipt value `{}` has an invalid list handle", owner.name))?;
            let mut values = Vec::with_capacity(length);
            for slot in slots {
                let word = word_from_slot(rt, slot, &element)?;
                values.push(encode_value(rt, word, &element, state, depth + 1)?);
            }
            Ok(DataTree::Array(values))
        }
    };
    state.leave(owner, raw);
    result
}

fn encode_map(
    rt: &mut JitRuntime,
    raw: i64,
    owner: &RuntimeTypeDescriptor,
    state: &mut EncodeState,
    depth: usize,
) -> Result<DataTree, String> {
    let length = rt
        .heap
        .map_len(raw)
        .ok_or_else(|| format!("receipt value `{}` has an invalid map handle", owner.name))?;
    let length = usize::try_from(length)
        .map_err(|_| format!("receipt value `{}` has a negative map length", owner.name))?;
    let key = lookup_descriptor(rt, owner.key, "map key")?;
    if key.kind != RuntimeValueKind::String {
        return Err(format!(
            "receipt value `{}` has a non-String map key",
            owner.name
        ));
    }
    let value = lookup_descriptor(rt, owner.value, "map value")?;
    state.enter(owner, raw)?;
    let result = if length > RECEIPT_VALUE_MAX_NODES.saturating_sub(state.nodes) {
        Err(format!(
            "receipt value exceeds maximum node count {}",
            RECEIPT_VALUE_MAX_NODES
        ))
    } else {
        let mut entries = Vec::with_capacity(length);
        for index in 0..length {
            let index =
                i64::try_from(index).map_err(|_| "receipt map index overflowed".to_string())?;
            let key_raw = rt
                .heap
                .map_key_at(raw, index)
                .ok_or_else(|| format!("receipt value `{}` has an invalid map key", owner.name))?;
            let key_text = rt
                .heap
                .clone_string(key_raw)
                .ok_or_else(|| format!("receipt value `{}` has an invalid map key string", owner.name))?;
            state.bytes(key_text.len())?;
            let value_raw = rt
                .heap
                .map_value_at(raw, index)
                .ok_or_else(|| format!("receipt value `{}` has an invalid map value", owner.name))?;
            let word = word_from_raw(rt, value_raw, &value)?;
            let value = encode_value(rt, word, &value, state, depth + 1)?;
            entries.push((key_text, value));
        }
        Ok(DataTree::Object(entries))
    };
    state.leave(owner, raw);
    result
}

fn record_word(
    rt: &JitRuntime,
    record: i64,
    index: usize,
    field: &RuntimeTypeDescriptor,
) -> Result<RuntimeWord, String> {
    let index =
        i64::try_from(index).map_err(|_| "receipt record field index overflowed".to_string())?;
    match field.kind {
        RuntimeValueKind::Float => rt
            .heap
            .record_get_float(record, index)
            .map(RuntimeWord::Float)
            .ok_or_else(|| format!("receipt value `{}` has an invalid Float field", field.name)),
        RuntimeValueKind::Bool => rt
            .heap
            .record_get_bool(record, index)
            .map(RuntimeWord::Bool)
            .ok_or_else(|| format!("receipt value `{}` has an invalid Bool field", field.name)),
        RuntimeValueKind::Char => rt
            .heap
            .record_get_char(record, index)
            .map(RuntimeWord::Char)
            .ok_or_else(|| format!("receipt value `{}` has an invalid Char field", field.name)),
        RuntimeValueKind::String => rt
            .heap
            .record_clone_string(record, index)
            .map(RuntimeWord::String)
            .ok_or_else(|| format!("receipt value `{}` has an invalid String field", field.name)),
        _ => rt
            .heap
            .record_get_int(record, index)
            .map(RuntimeWord::Bits)
            .ok_or_else(|| format!("receipt value `{}` has an invalid field carrier", field.name)),
    }
}

fn encode_record(
    rt: &mut JitRuntime,
    raw: i64,
    owner: &RuntimeTypeDescriptor,
    state: &mut EncodeState,
    depth: usize,
) -> Result<DataTree, String> {
    state.enter(owner, raw)?;
    let result = (|| {
        let mut fields = Vec::with_capacity(owner.fields.len());
        for field in &owner.fields {
            if field.skip {
                continue;
            }
            let child = lookup_descriptor(rt, Some(field.type_id), &field.source_name)?;
            let word = record_word(rt, raw, field.index, &child)?;
            if child.kind == RuntimeValueKind::Option {
                let option_raw = match &word {
                    RuntimeWord::Bits(value) => *value,
                    _ => {
                        return Err(format!(
                            "receipt value `{}` has an invalid Option field carrier",
                            field.source_name
                        ))
                    }
                };
                let (present, _) = crate::runtime_host::jit_result_parts(rt, option_raw)
                    .ok_or_else(|| {
                        format!(
                            "receipt value `{}` has an invalid Option field",
                            field.source_name
                        )
                    })?;
                if !present {
                    continue;
                }
            }
            let field_json_name = field
                .shape_names
                .name_for(ShapeProjectionKind::Json)
                .expect("runtime receipt fields always have a JSON projection name");
            state.bytes(field_json_name.len())?;
            let value = encode_value(rt, word, &child, state, depth + 1)?;
            fields.push((field_json_name.to_string(), value));
        }
        Ok(DataTree::Object(fields))
    })();
    state.leave(owner, raw);
    result
}

fn encode_enum(
    rt: &mut JitRuntime,
    raw: i64,
    owner: &RuntimeTypeDescriptor,
    state: &mut EncodeState,
    depth: usize,
) -> Result<DataTree, String> {
    state.enter(owner, raw)?;
    let result = (|| {
        let discriminant = rt
            .heap
            .record_get_int(raw, 0)
            .ok_or_else(|| format!("receipt value `{}` has an invalid enum record", owner.name))?;
        let variant = owner
            .variants
            .iter()
            .find(|variant| variant.discriminant == discriminant)
            .cloned()
            .ok_or_else(|| format!("receipt value `{}` has an unknown enum discriminant", owner.name))?;
        let variant_wire = variant.wire_name.clone();
        let mut named_payload = Vec::with_capacity(variant.fields.len());
        let mut single_payload = None;
        for field in &variant.fields {
            if field.skip {
                continue;
            }
            let child = lookup_descriptor(rt, Some(field.type_id), &field.source_name)?;
            let word = record_word(rt, raw, field.index.saturating_add(1), &child)?;
            let value = encode_value(rt, word, &child, state, depth + 1)?;
            if variant.fields.len() == 1 {
                single_payload = Some(value);
            } else {
                let field_json_name = field
                    .shape_names
                    .name_for(ShapeProjectionKind::Json)
                    .expect("runtime receipt fields always have a JSON projection name");
                state.bytes(field_json_name.len())?;
                named_payload.push((field_json_name.to_string(), value));
            }
        }
        let payload = if variant.fields.is_empty() {
            None
        } else if variant.fields.len() == 1 {
            Some(single_payload.ok_or_else(|| {
                format!(
                    "receipt enum variant `{}` has no encoded payload",
                    variant.name
                )
            })?)
        } else {
            Some(DataTree::Object(named_payload))
        };
        if owner.serde_untagged {
            return Ok(payload.unwrap_or(DataTree::Null));
        }
        if let Some(tag) = owner.serde_tag.as_deref() {
            state.bytes(tag.len())?;
            state.bytes(variant_wire.len())?;
            let mut entries = vec![(tag.to_string(), DataTree::Text(variant_wire))];
            match payload {
                None => {}
                Some(value) if variant.fields.len() == 1 => {
                    state.bytes("value".len())?;
                    entries.push(("value".to_string(), value));
                }
                Some(DataTree::Object(fields)) => entries.extend(fields),
                Some(_) => {
                    return Err(format!(
                        "receipt enum variant `{}` has an invalid named payload",
                        variant.name
                    ))
                }
            }
            return Ok(DataTree::Object(entries));
        }
        match payload {
            None => {
                state.bytes(variant_wire.len())?;
                Ok(DataTree::Text(variant_wire))
            }
            Some(payload) => {
                state.bytes(variant_wire.len())?;
                Ok(DataTree::Object(vec![(variant_wire, payload)]))
            }
        }
    })();
    state.leave(owner, raw);
    result
}

/// Encode one erased value using an existing recursive state. History's
/// resident callable hook uses this to preserve cycle detection while it
/// descends into a callback's live captures.
pub(crate) fn encode_jit_value_nested(
    rt: &mut JitRuntime,
    raw: i64,
    descriptor: &RuntimeTypeDescriptor,
    state: &mut EncodeState,
    depth: usize,
) -> Result<DataTree, String> {
    let word = word_from_raw(rt, raw, descriptor)?;
    encode_value(rt, word, descriptor, state, depth)
}

/// Encode one erased resident slot while preserving its concrete carrier.
pub(crate) fn encode_jit_value_slot(
    rt: &mut JitRuntime,
    value: JetVal,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<DataTree, String> {
    let mut state = EncodeState {
        nodes: 0,
        bytes: 0,
        active: HashSet::new(),
        callback: None,
    };
    let word = word_from_slot(rt, value, descriptor)?;
    encode_value(rt, word, descriptor, &mut state, 0)
}

/// Encode one erased resident value with an optional checked callable hook.
/// Ordinary receipt callers use `encode_jit_value`, which leaves callable
/// values unsupported; history alone supplies the hook for known JIT slots.
pub(crate) fn encode_jit_value_with_history_callback(
    rt: &mut JitRuntime,
    raw: i64,
    descriptor: &RuntimeTypeDescriptor,
    callback: HistoryCallableEncoder,
) -> Result<DataTree, String> {
    let mut state = EncodeState {
        nodes: 0,
        bytes: 0,
        active: HashSet::new(),
        callback: Some(callback),
    };
    encode_jit_value_nested(rt, raw, descriptor, &mut state, 0)
}

/// Encode one erased resident value through the checked descriptor registry.
///
/// The returned tree is rendered only after the runtime lock is released. This
/// keeps the shared JSON renderer's own exact-Int adapter from re-entering the
/// resident lock while it formats the value.
pub(crate) fn encode_jit_value(
    rt: &mut JitRuntime,
    raw: i64,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<DataTree, String> {
    let mut state = EncodeState {
        nodes: 0,
        bytes: 0,
        active: HashSet::new(),
        callback: None,
    };
    encode_jit_value_nested(rt, raw, descriptor, &mut state, 0)
}

fn receipt_diag(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        message.into(),
        "the interpreter ambient receipt adapter rejected the attachment".to_string(),
        "report this as a compiler bug".to_string(),
        Some(span),
    )
}

fn ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    _resolved_ret: Option<Type>,
    _sink: Option<&mut jet_codegen::Comptime::DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    // Receiver rows are represented by an empty Core module in the canonical
    // registry. The route identity remains `core.handle.attach`; only this
    // ambient callback sees the receiver projection's empty module.
    if !module.is_empty() || method != "attach" {
        return None;
    }
    let [value, CtValue::Str(section_name), CtValue::Str(type_name), CtValue::Str(schema_digest)] =
        args.as_slice()
    else {
        return Some(Err(receipt_diag(
            "receipt.attach received malformed payload or metadata",
            span,
        )));
    };
    let payload = jet_codegen::Comptime::render_datatree_for_tir(value);
    kernel::jet_receipt_attach_encoded(
        &payload,
        section_name,
        type_name,
        schema_digest,
    );
    Some(Ok(CtValue::Unit))
}

pub(crate) fn register_interpreter_ambient(context: &mut crate::InterpreterAmbientContext) {
    context.register_core_call(ambient_core_call);
}

