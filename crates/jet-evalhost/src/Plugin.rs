//! Resident JIT adapter for the checked `core.plugin` Component Model surface.
//!
//! The included Prelude owns sandbox policy, wasmtime calls, and the wire
//! protocol. This module only marshals checked MIR carriers across Cranelift.

use crate::ambient_interp::InterpreterAmbientContext;
use jet_codegen::Comptime::AmbientMirHandleResult;
use jet_foundation::Diagnostics::{Diagnostic, Span};
use jet_foundation::MIR::{MirNominalRef, MirRuntimeValue, MirType, MirTypeKind};
use jet_foundation::PluginWire as plugin_wire;

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
        err: Box<Self>
    },
    Record {
        name: String,
        fields: Vec<(usize, String, Self)>
    }
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
            err: Box::new(parse_type(bytes, pos)?)
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
        _ => Err("plugin component descriptor has an unknown type tag".to_string())
    }
}

#[derive(Clone)]
struct ParsedSignature {
    params: Vec<ParsedType>,
    result: ParsedType
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
            err: Box::new(mir_type_for_descriptor(err))
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
                fields: values
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
        _ => Err("plugin parameter does not match the checked component descriptor".to_string())
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
                f32: false
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
                element: mir_type_for_descriptor(inner)
            })
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
                    fields
                })
        }
        _ => Err("plugin result does not match the checked component descriptor".to_string())
    }
}

fn plugin_interpreter_error(error: plugin_wire::PluginError) -> Result<AmbientMirHandleResult, Diagnostic> {
    use plugin_wire::{PluginError, PluginLimit};
    let (variant, limit, fault) = match error {
        PluginError::Guest(fault) => ("Guest", None, fault),
        PluginError::Denied(fault) => ("Denied", None, fault),
        PluginError::Budget { limit, fault } => ("Budget", Some(limit), fault),
        PluginError::Defect(fault) => ("Defect", None, fault)
    };
    let fault = MirRuntimeValue::Struct {
        type_name: "PluginFault".to_string(),
        fields: vec![
            ("export".to_string(), MirRuntimeValue::String(fault.export)),
            ("message".to_string(), MirRuntimeValue::String(fault.message)),
            ("frames".to_string(), MirRuntimeValue::List(fault.frames.into_iter().map(|frame| MirRuntimeValue::Struct {
                type_name: "PluginFrame".to_string(),
                fields: vec![
                    ("function".to_string(), MirRuntimeValue::String(frame.function)),
                    ("component".to_string(), MirRuntimeValue::String(frame.component)),
                    ("offset".to_string(), MirRuntimeValue::Int(frame.offset)),
                ]
            }).collect())),
        ]
    };
    let args = if let Some(limit) = limit {
        vec![
            (Some("limit".to_string()), MirRuntimeValue::Enum {
                type_name: "PluginLimit".to_string(),
                variant: match limit {
                    PluginLimit::Fuel => "Fuel", PluginLimit::Memory => "Memory",
                    PluginLimit::Table => "Table", PluginLimit::Time => "Time", PluginLimit::Wire => "Wire"
                }.to_string(),
                args: Vec::new()
            }),
            (Some("fault".to_string()), fault),
        ]
    } else { vec![(None, fault)] };
    Ok(AmbientMirHandleResult::Value(MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::Enum {
        type_name: "PluginError".to_string(), variant: variant.to_string(), args
    }))))
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
        ))
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
                Err(error) => return Some(Err(error))
            };
            let authority = match plugin_interpreter_string(&args[1], "authority", span) {
                Ok(authority) => authority,
                Err(error) => return Some(Err(error))
            };
            let needs_wire =
                match plugin_interpreter_string(&args[2], "declared authority-needs", span) {
                    Ok(needs) => needs,
                    Err(error) => return Some(Err(error))
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
                )))
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
                Err(error) => return Some(Err(error))
            };
            let signature_wire =
                match plugin_interpreter_string(&args[2], "component signature", span) {
                    Ok(signature) => signature,
                    Err(error) => return Some(Err(error))
                };
            let signature = match parse_signature(&signature_wire) {
                Ok(signature) => signature,
                Err(error) => return Some(Err(plugin_ambient_diag(error, span)))
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
                Err(error) => return Some(Err(plugin_ambient_diag(error, span)))
            };
            let params_wire = plugin_wire::plugin_encode_params(&values);
            let wire = runtime::jet_plugin_call(handle as u64, &name, &params_wire);
            let value = match plugin_wire::plugin_decode_result(&wire) {
                Ok(value) => value,
                Err(error) => return Some(plugin_interpreter_error(error))
            };
            match plugin_to_mir(&value, &signature.result) {
                Ok(value) => Some(Ok(AmbientMirHandleResult::Value(
                    MirRuntimeValue::Present(Box::new(value)),
                ))),
                Err(error) => Some(plugin_interpreter_error(plugin_wire::PluginError::defect(&name, error)))
            }
        }
        _ => None
    }
}

pub(crate) fn register_interpreter_ambient(context: &mut InterpreterAmbientContext) {
    context.register_mir_handle(ambient_mir_handle);
}

// The rows carry the exact Prelude symbol the checked MIR route names
// (`jet_plugin_load` / `jet_plugin_call`): lowering resolves a Prelude call by
// `MirSymbol::name()`, so a `jet_jit_`-prefixed spelling would be unreachable.

#[cfg(test)]
mod failure_tests {
    use super::*;
    use plugin_wire::{PluginError, PluginLimit, PluginValue};

    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    #[test]
    fn plugin_failure_transport_retains_typed_cause_export_and_frames() {
        let home = std::env::var_os("HOME").expect("HOME is set for the bounded fixture");
        let root = std::path::PathBuf::from(home).join(".cache/jet-dev/scratch").join(format!(
            "plugin-failure-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
        ));
        std::fs::create_dir_all(&root).unwrap();
        let fixture = Fixture(root);
        let path = fixture.0.join("failure_guest.wat");
        std::fs::write(&path, include_str!("../../../Examples/features/packages/plugin_failure/failure_guest.wat")).unwrap();
        let authority = format!("FS.Read:{}", fixture.0.display());
        let load = runtime::jet_plugin_load_declared(path.to_str().unwrap(), &authority, &["FS.Read".to_string()]);
        let handle: u64 = load.strip_prefix("O:").expect("load returns a handle").parse().unwrap();
        let trap = plugin_wire::plugin_decode_result(&runtime::jet_plugin_call(handle, "trap", "0:")).unwrap_err();
        let PluginError::Guest(fault) = trap else { panic!("expected Guest, got {trap:?}"); };
        assert_eq!(fault.export, "trap", "{}", fault.message);
        assert!(!fault.frames.is_empty());
        assert!(fault.frames.len() <= plugin_wire::PLUGIN_MAX_FRAMES);
        assert_eq!(fault.frames[0].function, "trap");
        assert_eq!(fault.frames[0].component, "failure_guest");
        assert!(fault.frames[0].offset > 0);

        assert!(runtime::jet_plugin_close(handle));
        let load = runtime::jet_plugin_load_declared(path.to_str().unwrap(), &authority, &["FS.Read".to_string()]);
        let handle = load.strip_prefix("O:").unwrap().parse().unwrap();
        let denied = plugin_wire::plugin_decode_result(&runtime::jet_plugin_call(handle, "read", "0:")).unwrap_err();
        assert!(matches!(denied, PluginError::Denied(ref fault) if fault.export == "read" && fault.frames.is_empty()));
        assert!(runtime::jet_plugin_close(handle));
        let load = runtime::jet_plugin_load_declared(path.to_str().unwrap(), &authority, &["FS.Read".to_string()]);
        let handle = load.strip_prefix("O:").unwrap().parse().unwrap();
        let budget = plugin_wire::plugin_decode_result(&runtime::jet_plugin_call(handle, "spin", "0:")).unwrap_err();
        assert!(matches!(budget, PluginError::Budget { limit: PluginLimit::Fuel, ref fault } if fault.export == "spin" && !fault.frames.is_empty()));
        assert!(runtime::jet_plugin_close(handle));
        let load = runtime::jet_plugin_load_declared(path.to_str().unwrap(), &authority, &["FS.Read".to_string()]);
        let handle = load.strip_prefix("O:").unwrap().parse().unwrap();
        assert!(matches!(
            plugin_wire::plugin_decode_result(&runtime::jet_plugin_call(handle, "normal", "0:")),
            Ok(PluginValue::Int(42)),
        ));

        // All admitted rights have real adapters today. This test-only host
        // state removal reaches the exact absent-adapter Defect branch.
        runtime::plugin_test_remove_adapter(handle, "read");
        let defect = plugin_wire::plugin_decode_result(&runtime::jet_plugin_call(handle, "read", "0:")).unwrap_err();
        assert!(matches!(defect, PluginError::Defect(ref fault) if fault.export == "read" && fault.frames.is_empty()));
        let interpreted = plugin_interpreter_error(defect.clone()).unwrap();
        assert!(matches!(interpreted, AmbientMirHandleResult::Value(MirRuntimeValue::FailedTold(error))
            if matches!(*error, MirRuntimeValue::Enum { ref type_name, ref variant, .. } if type_name == "PluginError" && variant == "Defect")));
        assert_eq!(plugin_wire::plugin_decode_result(&plugin_wire::plugin_encode_error(&defect)).unwrap_err(), defect);
        assert!(runtime::jet_plugin_close(handle));

        let malformed = fixture.0.join("malformed.wasm");
        std::fs::write(&malformed, b"not a Component").unwrap();
        let load = runtime::jet_plugin_load_declared(malformed.to_str().unwrap(), &authority, &["FS.Read".to_string()]);
        let handle = load.strip_prefix("O:").expect("bad load still returns a handle").parse().unwrap();
        let error = plugin_wire::plugin_decode_result(&runtime::jet_plugin_call(handle, "normal", "0:")).unwrap_err();
        assert!(matches!(error, PluginError::Guest(ref fault) if fault.export == "load" && fault.frames.is_empty()));
        let load = runtime::jet_plugin_load_declared(path.to_str().unwrap(), &authority, &["FS.Read".to_string()]);
        let handle = load.strip_prefix("O:").unwrap().parse().unwrap();
        assert!(matches!(plugin_wire::plugin_decode_result(&runtime::jet_plugin_call(handle, "normal", "0:")), Ok(PluginValue::Int(42))));
        assert!(runtime::jet_plugin_close(handle));

        let failure = PluginError::defect("utf8:export", "message:\n雪");
        assert_eq!(plugin_wire::plugin_decode_result(&plugin_wire::plugin_encode_error(&failure)).unwrap_err(), failure);
    }
}
