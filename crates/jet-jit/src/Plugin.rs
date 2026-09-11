//! Resident JIT adapter for the checked `core.plugin` Component Model surface.
//!
//! The included Prelude owns sandbox policy, wasmtime calls, and the wire
//! protocol. This module only marshals checked MIR carriers across Cranelift.

use crate::ambient_interp::InterpreterAmbientContext;
use crate::runtime_host::{alloc_jit_result, jit_result_parts, JitRuntime};
use crate::Concurrency;
use jet_codegen::Comptime::AmbientMirHandleResult;
use jet_foundation::Diagnostics::{Diagnostic, Span};
use jet_foundation::MIR::{MirNominalRef, MirRuntimeValue, MirType, MirTypeKind};
use jet_foundation::PluginWire as plugin_wire;
use jet_rt::JetVal;

pub(crate) mod runtime {
    include!("../../jet-pkg-model/src/Prelude/Plugin.rs");
}

#[derive(Clone)]
enum ParsedType {
    Int,
    Float,
    Bool,
    String,
    List(Box<Self>),
    Option(Box<Self>),
    Result {
        ok: Box<Self>,
        err: Box<Self>,
    },
    Record {
        name: String,
        fields: Vec<(usize, String, Self)>,
    },
}

fn parse_decimal(bytes: &[u8], pos: &mut usize) -> Result<usize, String> {
    let start = *pos;
    while bytes.get(*pos).is_some_and(|byte| byte.is_ascii_digit()) {
        *pos += 1;
    }
    if *pos == start {
        return Err("plugin component descriptor is missing a decimal length".to_string());
    }
    std::str::from_utf8(&bytes[start..*pos])
        .ok()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| "plugin component descriptor has an invalid decimal length".to_string())
}

fn parse_text(bytes: &[u8], pos: &mut usize) -> Result<String, String> {
    let length = parse_decimal(bytes, pos)?;
    if bytes.get(*pos) != Some(&b':') {
        return Err("plugin component descriptor is missing a text separator".to_string());
    }
    *pos += 1;
    let end = (*pos)
        .checked_add(length)
        .ok_or_else(|| "plugin component descriptor text length overflowed".to_string())?;
    let value = bytes
        .get(*pos..end)
        .ok_or_else(|| "plugin component descriptor text is truncated".to_string())?;
    *pos = end;
    String::from_utf8(value.to_vec())
        .map_err(|_| "plugin component descriptor contains invalid UTF-8".to_string())
}

fn parse_type(bytes: &[u8], pos: &mut usize) -> Result<ParsedType, String> {
    let tag = *bytes
        .get(*pos)
        .ok_or_else(|| "plugin component descriptor is truncated".to_string())?;
    *pos += 1;
    match tag {
        b'i' => Ok(ParsedType::Int),
        b'f' => Ok(ParsedType::Float),
        b'b' => Ok(ParsedType::Bool),
        b's' => Ok(ParsedType::String),
        b'l' => Ok(ParsedType::List(Box::new(parse_type(bytes, pos)?))),
        b'o' => Ok(ParsedType::Option(Box::new(parse_type(bytes, pos)?))),
        b'q' => Ok(ParsedType::Result {
            ok: Box::new(parse_type(bytes, pos)?),
            err: Box::new(parse_type(bytes, pos)?),
        }),
        b'r' => {
            let name = parse_text(bytes, pos)?;
            let count = parse_decimal(bytes, pos)?;
            if bytes.get(*pos) != Some(&b':') {
                return Err("plugin component descriptor is missing a field separator".to_string());
            }
            *pos += 1;
            let mut fields = Vec::with_capacity(count);
            for _ in 0..count {
                let index = parse_decimal(bytes, pos)?;
                if bytes.get(*pos) != Some(&b':') {
                    return Err("plugin component descriptor is missing a field index separator".to_string());
                }
                *pos += 1;
                let field = parse_text(bytes, pos)?;
                let ty = parse_type(bytes, pos)?;
                fields.push((index, field, ty));
            }
            Ok(ParsedType::Record { name, fields })
        }
        _ => Err("plugin component descriptor has an unknown type tag".to_string()),
    }
}

#[derive(Clone)]
struct ParsedSignature {
    params: Vec<ParsedType>,
    result: ParsedType,
}

fn parse_signature(wire: &str) -> Result<ParsedSignature, String> {
    let bytes = wire.as_bytes();
    let mut pos = 0;
    if bytes.get(pos) != Some(&b'C') {
        return Err("plugin component signature is missing its `C` prefix".to_string());
    }
    pos += 1;
    let count = parse_decimal(bytes, &mut pos)?;
    if bytes.get(pos) != Some(&b':') {
        return Err("plugin component signature is missing its parameter separator".to_string());
    }
    pos += 1;
    let mut params = Vec::with_capacity(count);
    for _ in 0..count {
        params.push(parse_type(bytes, &mut pos)?);
    }
    if bytes.get(pos) != Some(&b'r') {
        return Err("plugin component signature is missing its result separator".to_string());
    }
    pos += 1;
    let result = parse_type(bytes, &mut pos)?;
    if pos != bytes.len() {
        return Err("plugin component signature has trailing bytes".to_string());
    }
    Ok(ParsedSignature { params, result })
}
fn plugin_ambient_diag(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        message.into(),
        "the interpreter plugin adapter rejected the checked operation".to_string(),
        "report this as a compiler bug".to_string(),
        Some(span),
    )
}

fn mir_type_for_descriptor(ty: &ParsedType) -> MirType {
    let kind = match ty {
        ParsedType::Int => MirTypeKind::Int,
        ParsedType::Float => MirTypeKind::Float,
        ParsedType::Bool => MirTypeKind::Bool,
        ParsedType::String => MirTypeKind::String,
        ParsedType::List(inner) => MirTypeKind::List(Box::new(mir_type_for_descriptor(inner))),
        ParsedType::Option(inner) => MirTypeKind::Option(Box::new(mir_type_for_descriptor(inner))),
        ParsedType::Result { ok, err } => MirTypeKind::Result {
            ok: Box::new(mir_type_for_descriptor(ok)),
            err: Box::new(mir_type_for_descriptor(err)),
        },
        ParsedType::Record { name, .. } => {
            MirTypeKind::Apply { name: MirNominalRef::from_name(name.clone()), args: Vec::new() }
        }
    };
    MirType::from_kind(kind)
}

fn mir_to_plugin(
    value: &MirRuntimeValue,
    ty: &ParsedType,
) -> Result<plugin_wire::PluginValue, String> {
    match (ty, value) {
        (ParsedType::Int, MirRuntimeValue::Int(value)) => {
            Ok(plugin_wire::PluginValue::Int(*value))
        }
        (ParsedType::Int, MirRuntimeValue::BigInt(value)) => value
            .parse::<i64>()
            .map(plugin_wire::PluginValue::Int)
            .map_err(|_| "plugin integer value exceeds the Component Model i64 range".to_string()),
        (ParsedType::Float, MirRuntimeValue::Float { value, .. }) => {
            Ok(plugin_wire::PluginValue::Float(*value))
        }
        (ParsedType::Bool, MirRuntimeValue::Bool(value)) => {
            Ok(plugin_wire::PluginValue::Bool(*value))
        }
        (ParsedType::String, MirRuntimeValue::String(value)) => {
            Ok(plugin_wire::PluginValue::Text(value.clone()))
        }
        (ParsedType::List(inner), MirRuntimeValue::List(values)) => values
            .iter()
            .map(|value| mir_to_plugin(value, inner))
            .collect::<Result<Vec<_>, _>>()
            .map(plugin_wire::PluginValue::List),
        (ParsedType::Option(_), MirRuntimeValue::Absent { .. }) => {
            Ok(plugin_wire::PluginValue::Option(None))
        }
        (ParsedType::Option(inner), MirRuntimeValue::Present(value)) => Ok(
            plugin_wire::PluginValue::Option(Some(Box::new(mir_to_plugin(value, inner)?))),
        ),
        (ParsedType::Result { ok, .. }, MirRuntimeValue::Present(value)) => Ok(
            plugin_wire::PluginValue::ResultOk(Some(Box::new(mir_to_plugin(value, ok)?))),
        ),
        (ParsedType::Result { err, .. }, MirRuntimeValue::FailedTold(value)) => Ok(
            plugin_wire::PluginValue::ResultErr(Some(Box::new(mir_to_plugin(value, err)?))),
        ),
        (
            ParsedType::Record { name, fields, .. },
            MirRuntimeValue::Struct {
                type_name,
                fields: values,
            },
        ) => {
            if type_name != name {
                return Err(format!(
                    "plugin record carrier has type `{type_name}`, expected `{name}`"
                ));
            }
            if values.len() != fields.len()
                || values
                    .iter()
                    .any(|(field, _)| !fields.iter().any(|(_, name, _)| name == field))
            {
                return Err("plugin record carrier has the wrong checked field set".to_string());
            }
            fields
                .iter()
                .map(|(_, field, field_ty)| {
                    let value = values
                        .iter()
                        .find(|(name, _)| name == field)
                        .map(|(_, value)| value)
                        .ok_or_else(|| format!("plugin record is missing field `{field}`"))?;
                    Ok((field.clone(), mir_to_plugin(value, field_ty)?))
                })
                .collect::<Result<Vec<_>, String>>()
                .map(plugin_wire::PluginValue::Record)
        }
        _ => Err("plugin parameter does not match the checked component descriptor".to_string()),
    }
}

fn plugin_to_mir(
    value: &plugin_wire::PluginValue,
    ty: &ParsedType,
) -> Result<MirRuntimeValue, String> {
    match (ty, value) {
        (ParsedType::Int, plugin_wire::PluginValue::Int(value)) => Ok(MirRuntimeValue::Int(*value)),
        (ParsedType::Float, plugin_wire::PluginValue::Float(value)) => {
            Ok(MirRuntimeValue::Float {
                value: *value,
                f32: false,
            })
        }
        (ParsedType::Bool, plugin_wire::PluginValue::Bool(value)) => {
            Ok(MirRuntimeValue::Bool(*value))
        }
        (ParsedType::String, plugin_wire::PluginValue::Text(value)) => {
            Ok(MirRuntimeValue::String(value.clone()))
        }
        (ParsedType::List(inner), plugin_wire::PluginValue::List(values)) => values
            .iter()
            .map(|value| plugin_to_mir(value, inner))
            .collect::<Result<Vec<_>, _>>()
            .map(MirRuntimeValue::List),
        (ParsedType::Option(inner), plugin_wire::PluginValue::Option(value)) => match value {
            Some(value) => Ok(MirRuntimeValue::Present(Box::new(plugin_to_mir(
                value, inner,
            )?))),
            None => Ok(MirRuntimeValue::Absent {
                element: mir_type_for_descriptor(inner),
            }),
        },
        (ParsedType::Result { ok, .. }, plugin_wire::PluginValue::ResultOk(Some(value))) => {
            Ok(MirRuntimeValue::Present(Box::new(plugin_to_mir(value, ok)?)))
        }
        (ParsedType::Result { err, .. }, plugin_wire::PluginValue::ResultErr(Some(value))) => {
            Ok(MirRuntimeValue::FailedTold(Box::new(plugin_to_mir(value, err)?)))
        }
        (ParsedType::Result { .. }, plugin_wire::PluginValue::ResultOk(None))
        | (ParsedType::Result { .. }, plugin_wire::PluginValue::ResultErr(None)) => Err(
            "plugin returned a result arm without the checked payload".to_string(),
        ),
        (
            ParsedType::Record { name, fields, .. },
            plugin_wire::PluginValue::Record(values),
        ) => {
            if values.len() != fields.len()
                || values
                    .iter()
                    .any(|(field, _)| !fields.iter().any(|(_, name, _)| name == field))
            {
                return Err("plugin result has the wrong checked record field set".to_string());
            }
            fields
                .iter()
                .map(|(_, field, field_ty)| {
                    let value = values
                        .iter()
                        .find(|(name, _)| name == field)
                        .map(|(_, value)| value)
                        .ok_or_else(|| format!("plugin result is missing record field `{field}`"))?;
                    Ok((field.clone(), plugin_to_mir(value, field_ty)?))
                })
                .collect::<Result<Vec<_>, String>>()
                .map(|fields| MirRuntimeValue::Struct {
                    type_name: name.clone(),
                    fields,
                })
        }
        _ => Err("plugin result does not match the checked component descriptor".to_string()),
    }
}


fn slot_handle(slot: &JetVal) -> Option<i64> {
    match slot {
        JetVal::Int(value) | JetVal::RecordRef(value) => Some(*value),
        _ => None,
    }
}

fn list_slots(rt: &JitRuntime, slot: &JetVal) -> Result<Vec<JetVal>, String> {
    match slot {
        JetVal::List(values) => Ok(values.clone()),
        JetVal::IntList(values) => Ok(values.iter().copied().map(JetVal::Int).collect()),
        JetVal::UninitList { .. } => Err("plugin list carrier is uninitialized".to_string()),
        _ => slot_handle(slot)
            .and_then(|handle| rt.heap.clone_list_values(handle))
            .ok_or_else(|| "plugin list handle is invalid".to_string()),
    }
}

fn record_slots(rt: &JitRuntime, slot: &JetVal) -> Result<Vec<JetVal>, String> {
    match slot {
        JetVal::Record(values) => Ok(values.clone()),
        JetVal::RecordRef(handle) | JetVal::Int(handle) => rt
            .heap
            .clone_record_values(*handle)
            .ok_or_else(|| "plugin record handle is invalid".to_string()),
        _ => Err("plugin record carrier is invalid".to_string()),
    }
}

fn string_slot(rt: &JitRuntime, slot: &JetVal) -> Result<String, String> {
    match slot {
        JetVal::String(value) => Ok(value.clone()),
        JetVal::StringView { owner, start, end } => rt
            .heap
            .get_string(*owner)
            .and_then(|value| value.get(*start..*end))
            .map(str::to_string)
            .ok_or_else(|| "plugin string view is invalid".to_string()),
        JetVal::Int(handle) => rt
            .heap
            .clone_string(*handle)
            .ok_or_else(|| "plugin string handle is invalid".to_string()),
        _ => Err("plugin string carrier is invalid".to_string()),
    }
}

fn raw_slot(bits: u64, ty: &ParsedType) -> JetVal {
    match ty {
        ParsedType::Int => JetVal::Int(bits as i64),
        ParsedType::Float => JetVal::Float(f64::from_bits(bits)),
        ParsedType::Bool => JetVal::Bool(bits != 0),
        ParsedType::String
        | ParsedType::List(_)
        | ParsedType::Option(_)
        | ParsedType::Result { .. }
        | ParsedType::Record { .. } => JetVal::Int(bits as i64),
    }
}


fn slot_to_plugin(rt: &JitRuntime, slot: &JetVal, ty: &ParsedType) -> Result<plugin_wire::PluginValue, String> {
    match ty {
        ParsedType::Int => match slot {
            JetVal::Int(value) => Ok(plugin_wire::PluginValue::Int(*value)),
            _ => Err("plugin integer carrier is invalid".to_string()),
        },
        ParsedType::Float => match slot {
            JetVal::Float(value) => Ok(plugin_wire::PluginValue::Float(*value)),
            JetVal::Int(value) => Ok(plugin_wire::PluginValue::Float(f64::from_bits(*value as u64))),
            _ => Err("plugin float carrier is invalid".to_string()),
        },
        ParsedType::Bool => match slot {
            JetVal::Bool(value) => Ok(plugin_wire::PluginValue::Bool(*value)),
            JetVal::Int(0) => Ok(plugin_wire::PluginValue::Bool(false)),
            JetVal::Int(1) => Ok(plugin_wire::PluginValue::Bool(true)),
            _ => Err("plugin bool carrier is invalid".to_string()),
        },
        ParsedType::String => Ok(plugin_wire::PluginValue::Text(string_slot(rt, slot)?)),
        ParsedType::List(inner) => list_slots(rt, slot)?
            .iter()
            .map(|value| slot_to_plugin(rt, value, inner))
            .collect::<Result<Vec<_>, _>>()
            .map(plugin_wire::PluginValue::List),
        ParsedType::Option(inner) => {
            let handle = slot_handle(slot).ok_or_else(|| "plugin option carrier is invalid".to_string())?;
            let (present, bits) = jit_result_parts(rt, handle)
                .ok_or_else(|| "plugin option result handle is invalid".to_string())?;
            if present {
                let value = raw_slot(bits, inner);
                Ok(plugin_wire::PluginValue::Option(Some(Box::new(slot_to_plugin(
                    rt, &value, inner,
                )?))))
            } else {
                Ok(plugin_wire::PluginValue::Option(None))
            }
        }
        ParsedType::Result { ok, err } => {
            let handle = slot_handle(slot).ok_or_else(|| "plugin result carrier is invalid".to_string())?;
            let (success, bits) = jit_result_parts(rt, handle)
                .ok_or_else(|| "plugin result handle is invalid".to_string())?;
            if success {
                let value = raw_slot(bits, ok);
                Ok(plugin_wire::PluginValue::ResultOk(Some(Box::new(slot_to_plugin(
                    rt, &value, ok,
                )?))))
            } else {
                let value = raw_slot(bits, err);
                Ok(plugin_wire::PluginValue::ResultErr(Some(Box::new(slot_to_plugin(
                    rt, &value, err,
                )?))))
            }
        }
        ParsedType::Record { fields, .. } => {
            let slots = record_slots(rt, slot)?;
            fields
                .iter()
                .map(|(index, name, field_ty)| {
                    let value = slots
                        .get(*index)
                        .ok_or_else(|| "plugin record field index is invalid".to_string())?;
                    Ok((name.clone(), slot_to_plugin(rt, value, field_ty)?))
                })
                .collect::<Result<Vec<_>, String>>()
                .map(plugin_wire::PluginValue::Record)
        }
    }
}

fn plugin_value_to_slot(
    rt: &mut JitRuntime,
    value: &plugin_wire::PluginValue,
    ty: &ParsedType,
) -> Result<JetVal, String> {
    match (ty, value) {
        (ParsedType::Int, plugin_wire::PluginValue::Int(value)) => Ok(JetVal::Int(*value)),
        (ParsedType::Float, plugin_wire::PluginValue::Float(value)) => Ok(JetVal::Float(*value)),
        (ParsedType::Bool, plugin_wire::PluginValue::Bool(value)) => Ok(JetVal::Bool(*value)),
        (ParsedType::String, plugin_wire::PluginValue::Text(value)) => Ok(JetVal::String(value.clone())),
        (ParsedType::List(inner), plugin_wire::PluginValue::List(values)) => {
            let slots = values
                .iter()
                .map(|value| plugin_value_to_slot(rt, value, inner))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(JetVal::Int(rt.heap.alloc_list_values(slots)))
        }
        (ParsedType::Record { fields, .. }, plugin_wire::PluginValue::Record(values)) => {
            let mut slots = vec![JetVal::Int(0); fields.iter().map(|(index, _, _)| *index + 1).max().unwrap_or(0)];
            for (index, name, field_ty) in fields {
                let value = values
                    .iter()
                    .find(|(field, _)| field == name)
                    .map(|(_, value)| value)
                    .ok_or_else(|| format!("plugin result is missing record field `{name}`"))?;
                slots[*index] = plugin_value_to_slot(rt, value, field_ty)?;
            }
            Ok(JetVal::RecordRef(rt.heap.alloc_record_values(slots)))
        }
        (ParsedType::Option(inner), plugin_wire::PluginValue::Option(value)) => {
            let bits = value
                .as_deref()
                .map(|value| plugin_value_to_raw(rt, value, inner))
                .transpose()?
                .unwrap_or(0);
            Ok(JetVal::Int(alloc_jit_result(rt, value.is_some(), bits as u64)))
        }
        (ParsedType::Result { ok, .. }, plugin_wire::PluginValue::ResultOk(value)) => {
            let bits = value
                .as_deref()
                .map(|value| plugin_value_to_raw(rt, value, ok))
                .transpose()?
                .unwrap_or(0);
            Ok(JetVal::Int(alloc_jit_result(rt, true, bits as u64)))
        }
        (ParsedType::Result { err, .. }, plugin_wire::PluginValue::ResultErr(value)) => {
            let bits = value
                .as_deref()
                .map(|value| plugin_value_to_raw(rt, value, err))
                .transpose()?
                .unwrap_or(0);
            Ok(JetVal::Int(alloc_jit_result(rt, false, bits as u64)))
        }
        _ => Err("plugin result does not match the checked component descriptor".to_string()),
    }
}

fn plugin_value_to_raw(
    rt: &mut JitRuntime,
    value: &plugin_wire::PluginValue,
    ty: &ParsedType,
) -> Result<i64, String> {
    match (ty, value) {
        (ParsedType::Int, plugin_wire::PluginValue::Int(value)) => Ok(*value),
        (ParsedType::Float, plugin_wire::PluginValue::Float(value)) => Ok(value.to_bits() as i64),
        (ParsedType::Bool, plugin_wire::PluginValue::Bool(value)) => Ok(i64::from(*value)),
        (ParsedType::String, plugin_wire::PluginValue::Text(value)) => Ok(rt.heap.alloc_string(value.clone())),
        (ParsedType::List(inner), plugin_wire::PluginValue::List(values)) => {
            let slots = values
                .iter()
                .map(|value| plugin_value_to_slot(rt, value, inner))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rt.heap.alloc_list_values(slots))
        }
        (ParsedType::Record { fields, .. }, plugin_wire::PluginValue::Record(values)) => {
            let mut slots = vec![JetVal::Int(0); fields.iter().map(|(index, _, _)| *index + 1).max().unwrap_or(0)];
            for (index, name, field_ty) in fields {
                let value = values
                    .iter()
                    .find(|(field, _)| field == name)
                    .map(|(_, value)| value)
                    .ok_or_else(|| format!("plugin result is missing record field `{name}`"))?;
                slots[*index] = plugin_value_to_slot(rt, value, field_ty)?;
            }
            Ok(rt.heap.alloc_record_values(slots))
        }
        (ParsedType::Option(inner), plugin_wire::PluginValue::Option(value)) => {
            let bits = value
                .as_deref()
                .map(|value| plugin_value_to_raw(rt, value, inner))
                .transpose()?
                .unwrap_or(0);
            Ok(alloc_jit_result(rt, value.is_some(), bits as u64))
        }
        (ParsedType::Result { ok, .. }, plugin_wire::PluginValue::ResultOk(value)) => {
            let bits = value
                .as_deref()
                .map(|value| plugin_value_to_raw(rt, value, ok))
                .transpose()?
                .unwrap_or(0);
            Ok(alloc_jit_result(rt, true, bits as u64))
        }
        (ParsedType::Result { err, .. }, plugin_wire::PluginValue::ResultErr(value)) => {
            let bits = value
                .as_deref()
                .map(|value| plugin_value_to_raw(rt, value, err))
                .transpose()?
                .unwrap_or(0);
            Ok(alloc_jit_result(rt, false, bits as u64))
        }
        _ => Err("plugin result does not match the checked component descriptor".to_string()),
    }
}

fn plugin_error(rt: &mut JitRuntime, message: impl Into<String>) -> i64 {
    let error = rt.heap.alloc_string(message.into());
    alloc_jit_result(rt, false, error as u64)
}

pub(crate) fn jet_jit_plugin_load(path: i64, authority: i64, declared_needs: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(path) = rt.heap.clone_string(path) else {
            rt.set_host_fault("MIR plugin path handle is invalid");
            return 0;
        };
        let authority = rt
            .heap
            .clone_string(authority)
            .unwrap_or_else(|| crate::Collections::authority_wire(rt, authority));
        let Some(needs_wire) = rt.heap.clone_string(declared_needs) else {
            rt.set_host_fault("MIR plugin authority-needs handle is invalid");
            return 0;
        };
        let declared_needs = needs_wire
            .split('\n')
            .filter(|need| !need.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        let wire = runtime::jet_plugin_load_declared(&path, &authority, &declared_needs);
        match wire.strip_prefix("O:").and_then(|value| value.parse::<u64>().ok()) {
            Some(handle) if handle > 0 => handle as i64,
            _ => {
                rt.set_host_fault(wire.strip_prefix("E:").unwrap_or(&wire));
                0
            }
        }
    })
}

pub(crate) fn jet_jit_plugin_call(
    handle: i64,
    name: i64,
    params: i64,
    descriptor: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(name) = rt.heap.clone_string(name) else {
            rt.set_host_fault("MIR plugin export name handle is invalid");
            return 0;
        };
        let Some(signature_wire) = rt.heap.clone_string(descriptor) else {
            rt.set_host_fault("MIR plugin component signature handle is invalid");
            return 0;
        };
        let signature = match parse_signature(&signature_wire) {
            Ok(signature) => signature,
            Err(error) => {
                rt.set_host_fault(&error);
                return 0;
            }
        };
        let slots = match rt.heap.clone_list_values(params) {
            Some(slots) => slots,
            None => {
                rt.set_host_fault("MIR plugin parameter list handle is invalid");
                return 0;
            }
        };
        if slots.len() != signature.params.len() {
            rt.set_host_fault(&format!(
                "MIR plugin export `{name}` expects {} parameter(s), got {}",
                signature.params.len(),
                slots.len()
            ));
            return 0;
        }
        let values = match slots
            .iter()
            .zip(&signature.params)
            .map(|(slot, ty)| slot_to_plugin(rt, slot, ty))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(values) => values,
            Err(error) => {
                rt.set_host_fault(&error);
                return 0;
            }
        };
        let params_wire = plugin_wire::plugin_encode_params(&values);
        let wire = runtime::jet_plugin_call(handle as u64, &name, &params_wire);
        let value = match plugin_wire::plugin_decode_result(&wire) {
            Ok(value) => value,
            Err(error) => return plugin_error(rt, error),
        };
        match plugin_value_to_raw(rt, &value, &signature.result) {
            Ok(bits) => alloc_jit_result(rt, true, bits as u64),
            Err(error) => plugin_error(rt, error),
        }
    })
}
fn plugin_interpreter_error(
    message: impl Into<String>,
) -> Result<AmbientMirHandleResult, Diagnostic> {
    Ok(AmbientMirHandleResult::Value(MirRuntimeValue::FailedTold(
        Box::new(MirRuntimeValue::String(message.into())),
    )))
}

fn plugin_interpreter_string(
    value: &MirRuntimeValue,
    label: &str,
    span: Span,
) -> Result<String, Diagnostic> {
    match value {
        MirRuntimeValue::String(value) => Ok(value.clone()),
        _ => Err(plugin_ambient_diag(
            format!("MIR plugin {label} carrier is not a checked String"),
            span,
        )),
    }
}

/// Dispatch the checked interpreter/deopt plugin operations through the same
/// Prelude and descriptor kernel used by the resident JIT ABI.
pub(crate) fn ambient_mir_handle(
    operation: &str,
    handle: Option<i64>,
    args: Vec<MirRuntimeValue>,
    span: Span,
) -> Option<Result<AmbientMirHandleResult, Diagnostic>> {
    match operation {
        "jet_plugin_load" => {
            if handle.is_some() || args.len() != 3 {
                return Some(Err(plugin_ambient_diag(
                    "MIR plugin load received the wrong checked handle or argument shape",
                    span,
                )));
            }
            let path = match plugin_interpreter_string(&args[0], "path", span) {
                Ok(path) => path,
                Err(error) => return Some(Err(error)),
            };
            let authority = match plugin_interpreter_string(&args[1], "authority", span) {
                Ok(authority) => authority,
                Err(error) => return Some(Err(error)),
            };
            let needs_wire =
                match plugin_interpreter_string(&args[2], "declared authority-needs", span) {
                    Ok(needs) => needs,
                    Err(error) => return Some(Err(error)),
                };
            let declared_needs = needs_wire
                .split('\n')
                .filter(|need| !need.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            let wire = runtime::jet_plugin_load_declared(&path, &authority, &declared_needs);
            match wire.strip_prefix("O:").and_then(|value| value.parse::<u64>().ok()) {
                Some(plugin) if plugin > 0 => {
                    Some(Ok(AmbientMirHandleResult::Handle(plugin as i64)))
                }
                _ => Some(Err(plugin_ambient_diag(
                    wire.strip_prefix("E:").unwrap_or(&wire),
                    span,
                ))),
            }
        }
        "jet_plugin_call" => {
            let Some(handle) = handle else {
                return Some(Err(plugin_ambient_diag(
                    "MIR plugin call has no checked receiver handle",
                    span,
                )));
            };
            if args.len() != 3 {
                return Some(Err(plugin_ambient_diag(
                    "MIR plugin call received the wrong checked argument shape",
                    span,
                )));
            }
            let name = match plugin_interpreter_string(&args[0], "export name", span) {
                Ok(name) => name,
                Err(error) => return Some(Err(error)),
            };
            let signature_wire =
                match plugin_interpreter_string(&args[2], "component signature", span) {
                    Ok(signature) => signature,
                    Err(error) => return Some(Err(error)),
                };
            let signature = match parse_signature(&signature_wire) {
                Ok(signature) => signature,
                Err(error) => return Some(Err(plugin_ambient_diag(error, span))),
            };
            let MirRuntimeValue::List(params) = &args[1] else {
                return Some(Err(plugin_ambient_diag(
                    "MIR plugin call parameters are not a checked value list",
                    span,
                )));
            };
            if params.len() != signature.params.len() {
                return Some(Err(plugin_ambient_diag(
                    format!(
                        "MIR plugin export `{name}` expects {} parameter(s), got {}",
                        signature.params.len(),
                        params.len()
                    ),
                    span,
                )));
            }
            let values = match params
                .iter()
                .zip(&signature.params)
                .map(|(value, ty)| mir_to_plugin(value, ty))
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(values) => values,
                Err(error) => return Some(Err(plugin_ambient_diag(error, span))),
            };
            let params_wire = plugin_wire::plugin_encode_params(&values);
            let wire = runtime::jet_plugin_call(handle as u64, &name, &params_wire);
            let value = match plugin_wire::plugin_decode_result(&wire) {
                Ok(value) => value,
                Err(error) => return Some(plugin_interpreter_error(error)),
            };
            match plugin_to_mir(&value, &signature.result) {
                Ok(value) => Some(Ok(AmbientMirHandleResult::Value(
                    MirRuntimeValue::Present(Box::new(value)),
                ))),
                Err(error) => Some(plugin_interpreter_error(error)),
            }
        }
        _ => None,
    }
}

pub(crate) fn register_interpreter_ambient(context: &mut InterpreterAmbientContext) {
    context.register_mir_handle(ambient_mir_handle);
}

// The rows carry the exact Prelude symbol the checked MIR route names
// (`jet_plugin_load` / `jet_plugin_call`): lowering resolves a Prelude call by
// `MirSymbol::name()`, so a `jet_jit_`-prefixed spelling would be unreachable.
host_fns! {
    struct PluginHostFns;
    register: register_plugin_symbols;
    declare: declare_plugin_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::Module;
        let cc = module.target_config().default_call_conv;
        // (path, authority wire, compiler-supplied declared needs) -> handle | Result
        let mut sig_load = Signature::new(cc);
        sig_load.params.extend([AbiParam::new(types::I64); 3]);
        sig_load.returns.push(AbiParam::new(types::I64));
        // (handle, export name, checked value list, component descriptor) -> Result
        let mut sig_call = Signature::new(cc);
        sig_call.params.extend([AbiParam::new(types::I64); 4]);
        sig_call.returns.push(AbiParam::new(types::I64));
    }
    load: "jet_plugin_load" => jet_jit_plugin_load: sig_load;
    call: "jet_plugin_call" => jet_jit_plugin_call: sig_call;
}
