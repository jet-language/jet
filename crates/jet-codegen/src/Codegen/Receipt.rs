//! Checked Codable projection for interpreter receipt attachments.
//!
//! The interpreter carries structured MIR values rather than resident handles.
//! This adapter consumes only the MIR type rows and their checked wire metadata;
//! it never derives a wire key from a display spelling.

use jet_foundation::MIR::{
    MirRuntimeValue, MirSerdeAttributeKind, MirType, MirTypeDef, MirTypeDefKind, MirTypeKind,
    MirVariantPayload, MirProgram, MirField,
};
use std::collections::HashSet;
use jet_foundation::Shape::ShapeProjectionKind;

const JSON_OBJECT: &str = "JSONObject";
fn field_json_name(field: &MirField) -> &str {
    field
        .shape_names
        .name_for(ShapeProjectionKind::Json)
        .expect("checked receipt fields always have a JSON projection name")
}

fn object(fields: Vec<(String, MirRuntimeValue)>) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: JSON_OBJECT.to_string(),
        fields,
    }
}

fn type_definition<'a>(
    program: &'a MirProgram,
    ty: &MirType,
) -> Option<&'a MirTypeDef> {
    let id = ty.identity?;
    program.types.iter().find(|definition| definition.id == id)
}

fn is_u8(ty: &MirType) -> bool {
    matches!(
        ty.kind(),
        MirTypeKind::IntN {
            signed: false,
            bits: 8
        }
    )
}

fn encode_byte_values(values: &[MirRuntimeValue]) -> Result<MirRuntimeValue, String> {
    values
        .iter()
        .map(|value| match value {
            MirRuntimeValue::Int(value) => u8::try_from(*value)
                .map_err(|_| "receipt byte list contains an out-of-range value".to_string()),
            MirRuntimeValue::BigInt(value) => value
                .parse::<u8>()
                .map_err(|_| "receipt byte list contains an out-of-range value".to_string()),
            _ => Err("receipt byte list contains a non-integer value".to_string()),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(MirRuntimeValue::Bytes)
}

fn encode_fields(
    program: &MirProgram,
    source: &[(String, MirRuntimeValue)],
    fields: &[MirField],
) -> Result<Vec<(String, MirRuntimeValue)>, String> {
    let mut seen = HashSet::with_capacity(source.len());
    for (name, _) in source {
        if !seen.insert(name) {
            return Err(format!("receipt record repeats checked field `{name}`"));
        }
        if !fields.iter().any(|field| &field.name == name) {
            return Err(format!("receipt record has unknown checked field `{name}`"));
        }
    }
    let mut encoded = Vec::with_capacity(fields.len());
    for field in fields {
        if field.skip {
            continue;
        }
        let Some((_, value)) = source.iter().find(|(name, _)| name == &field.name) else {
            return Err(format!(
                "receipt value is missing checked field `{}`",
                field.name
            ));
        };
        if matches!(field.ty.kind(), MirTypeKind::Option(_))
            && matches!(value, MirRuntimeValue::Absent { .. })
        {
            continue;
        }
        encoded.push((
            field_json_name(field).to_string(),
            encode_value(program, value, &field.ty)?,
        ));
    }
    Ok(encoded)
}

fn serde_tag(definition: &MirTypeDef) -> Option<String> {
    definition.serde.iter().find_map(|attribute| {
        (attribute.kind == MirSerdeAttributeKind::Tag)
            .then(|| attribute.value.clone())
            .flatten()
    })
}

fn serde_untagged(definition: &MirTypeDef) -> bool {
    definition
        .serde
        .iter()
        .any(|attribute| attribute.kind == MirSerdeAttributeKind::Untagged)
}

fn encode_enum(
    program: &MirProgram,
    value: &MirRuntimeValue,
    variants: &[jet_foundation::MIR::MirVariant],
    definition: &MirTypeDef,
) -> Result<MirRuntimeValue, String> {
    let MirRuntimeValue::Enum {
        variant: source_variant,
        args,
        ..
    } = value
    else {
        return Err(format!(
            "receipt value `{}` is not an enum carrier",
            definition.name
        ));
    };
    let variant = variants
        .iter()
        .find(|candidate| candidate.name == *source_variant)
        .ok_or_else(|| {
            format!(
                "receipt value `{}` has unknown enum variant `{}`",
                definition.name, source_variant
            )
        })?;
    let payload = match &variant.payload {
        MirVariantPayload::Unit => {
            if !args.is_empty() {
                return Err(format!(
                    "receipt enum variant `{}` has an unexpected payload",
                    variant.name
                ));
            }
            None
        }
        MirVariantPayload::Single(ty) => {
            let Some((_, payload)) = args.first() else {
                return Err(format!(
                    "receipt enum variant `{}` is missing its payload",
                    variant.name
                ));
            };
            if args.len() != 1 {
                return Err(format!(
                    "receipt enum variant `{}` has too many payload values",
                    variant.name
                ));
            }
            Some(encode_value(program, payload, ty)?)
        }
        MirVariantPayload::Named(fields) => {
            if args.len() != fields.len() {
                return Err(format!(
                    "receipt enum variant `{}` has {} payload values; checked shape needs {}",
                    variant.name,
                    args.len(),
                    fields.len()
                ));
            }
            let mut encoded = Vec::with_capacity(fields.len());
            for (index, field) in fields.iter().enumerate() {
                if field.skip {
                    continue;
                }
                let labeled = args.iter().find_map(|(label, value)| {
                    (label.as_deref() == Some(field.name.as_str())).then_some(value)
                });
                let positional = args
                    .iter()
                    .all(|(label, _)| label.is_none())
                    .then(|| args.get(index).map(|(_, value)| value))
                    .flatten();
                let Some(payload) = labeled.or(positional) else {
                    return Err(format!(
                        "receipt enum variant `{}` is missing checked field `{}`",
                        variant.name, field.name
                    ));
                };
                encoded.push((
                    field_json_name(field).to_string(),
                    encode_value(program, payload, &field.ty)?,
                ));
            }
            Some(object(encoded))
        }
    };
    if serde_untagged(definition) {
        return Ok(payload.unwrap_or(MirRuntimeValue::Unit));
    }
    let wire = variant.wire_name.clone();
    if let Some(tag) = serde_tag(definition) {
        let mut fields = vec![(tag, MirRuntimeValue::String(wire))];
        match payload {
            None => {}
            Some(value) if matches!(&variant.payload, MirVariantPayload::Single(_)) => {
                fields.push(("value".to_string(), value));
            }
            Some(MirRuntimeValue::Struct { fields: payload, .. }) => fields.extend(payload),
            Some(_) => {
                return Err(format!(
                    "receipt enum variant `{}` has an invalid named payload",
                    variant.name
                ));
            }
        }
        return Ok(object(fields));
    }
    match payload {
        None => Ok(MirRuntimeValue::String(wire)),
        Some(payload) => Ok(object(vec![(wire, payload)])),
    }
}

fn encode_definition(
    program: &MirProgram,
    value: &MirRuntimeValue,
    definition: &MirTypeDef,
) -> Result<MirRuntimeValue, String> {
    match &definition.kind {
        MirTypeDefKind::Struct { fields, .. } => {
            let MirRuntimeValue::Struct { fields: source, .. } = value else {
                return Err(format!(
                    "receipt value `{}` is not a record carrier",
                    definition.name
                ));
            };
            Ok(object(encode_fields(program, source, fields)?))
        }
        MirTypeDefKind::Enum { variants, .. } => {
            encode_enum(program, value, variants, definition)
        }
        MirTypeDefKind::Distinct { base, .. } => encode_value(program, value, base),
        MirTypeDefKind::Alias { target } => encode_value(program, value, target),
        MirTypeDefKind::UnitFamily { .. } => Err(format!(
            "receipt type `{}` has no checked Codable variant metadata",
            definition.name
        )),
    }
}

fn encode_option(
    program: &MirProgram,
    value: &MirRuntimeValue,
    inner: &MirType,
) -> Result<MirRuntimeValue, String> {
    match value {
        MirRuntimeValue::Present(value) => encode_value(program, value, inner),
        MirRuntimeValue::Absent { .. } => Ok(MirRuntimeValue::Unit),
        MirRuntimeValue::FailedTold(_) => Err("receipt Option carries a told failure".to_string()),
        _ => Err("receipt Option has an invalid result carrier".to_string()),
    }
}

fn encode_result(
    program: &MirProgram,
    value: &MirRuntimeValue,
    ok: &MirType,
) -> Result<MirRuntimeValue, String> {
    match value {
        MirRuntimeValue::Present(value) => encode_value(program, value, ok),
        MirRuntimeValue::Absent { .. } => Ok(MirRuntimeValue::Unit),
        MirRuntimeValue::FailedTold(_) => Err("receipt Result has a non-Codable error carrier".to_string()),
        _ => Err("receipt Result has an invalid result carrier".to_string()),
    }
}

fn encode_value(
    program: &MirProgram,
    value: &MirRuntimeValue,
    ty: &MirType,
) -> Result<MirRuntimeValue, String> {
    if let MirTypeKind::Apply { name, args } = ty.kind() {
        match (name.name.as_str(), args.as_slice()) {
            ("List", [inner]) => {
                return encode_value(program, value, &MirType::from_kind(
                    MirTypeKind::List(Box::new(inner.clone())),
                ));
            }
            ("Shared", [inner]) => {
                return encode_value(program, value, &MirType::from_kind(
                    MirTypeKind::Shared(Box::new(inner.clone())),
                ));
            }
            ("Option", [inner]) => return encode_option(program, value, inner),
            ("Result", [ok, _err]) => return encode_result(program, value, ok),
            _ => {}
        }
    }
    if let Some(definition) = type_definition(program, ty) {
        return encode_definition(program, value, definition);
    }
    match ty.kind() {
        MirTypeKind::Int | MirTypeKind::IntN { .. } | MirTypeKind::Measure(_) => match value {
            MirRuntimeValue::Int(_) | MirRuntimeValue::BigInt(_) => Ok(value.clone()),
            _ => Err("receipt integer has an invalid scalar carrier".to_string()),
        },
        MirTypeKind::Float | MirTypeKind::Float32 => match value {
            MirRuntimeValue::Float { value: number, .. } if number.is_finite() => Ok(value.clone()),
            MirRuntimeValue::Float { .. } => {
                Err("receipt Float contains a non-finite value".to_string())
            }
            _ => Err("receipt Float has an invalid scalar carrier".to_string()),
        },
        MirTypeKind::Bool => match value {
            MirRuntimeValue::Bool(_) => Ok(value.clone()),
            _ => Err("receipt Bool has an invalid scalar carrier".to_string()),
        },
        MirTypeKind::Char => match value {
            MirRuntimeValue::Char(_) => Ok(value.clone()),
            _ => Err("receipt Char has an invalid scalar carrier".to_string()),
        },
        MirTypeKind::String => match value {
            MirRuntimeValue::String(_) => Ok(value.clone()),
            _ => Err("receipt String has an invalid scalar carrier".to_string()),
        },
        MirTypeKind::List(inner) => match value {
            MirRuntimeValue::Bytes(_) if is_u8(inner) => Ok(value.clone()),
            MirRuntimeValue::List(values) if is_u8(inner) => encode_byte_values(values),
            MirRuntimeValue::List(values) => Ok(MirRuntimeValue::List(
                values
                    .iter()
                    .map(|value| encode_value(program, value, inner))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            _ => Err("receipt List has an invalid list carrier".to_string()),
        },
        MirTypeKind::FixedList { elem, .. } => match value {
            MirRuntimeValue::Bytes(_) if is_u8(elem) => Ok(value.clone()),
            MirRuntimeValue::List(values) if is_u8(elem) => encode_byte_values(values),
            MirRuntimeValue::List(values) => Ok(MirRuntimeValue::List(
                values
                    .iter()
                    .map(|value| encode_value(program, value, elem))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            _ => Err("receipt fixed list has an invalid list carrier".to_string()),
        },
        MirTypeKind::Map { value: value_ty, .. } => match value {
            MirRuntimeValue::Map(entries) => Ok(MirRuntimeValue::Map(
                entries
                    .iter()
                    .map(|(key, value)| {
                        Ok((key.clone(), encode_value(program, value, value_ty)?))
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            )),
            _ => Err("receipt Map has an invalid map carrier".to_string()),
        },
        MirTypeKind::Shared(inner) => encode_value(program, value, inner),
        MirTypeKind::Option(inner) => encode_option(program, value, inner),
        MirTypeKind::Result { ok, .. } => encode_result(program, value, ok),
        MirTypeKind::Tuple(_) => Err(format!(
            "receipt type `{}` has no checked Codable field metadata",
            ty.display_name()
        )),
        MirTypeKind::Tagged { inner, .. }
        | MirTypeKind::InlineRange { base: inner, .. }
        | MirTypeKind::Quantity { base: inner, .. } => encode_value(program, value, inner),
        MirTypeKind::Apply { name, args } => match name.name.as_str() {
            "List" if args.len() == 1 => encode_value(
                program,
                value,
                &MirType::from_kind(MirTypeKind::List(Box::new(args[0].clone()))),
            ),
            "Shared" if args.len() == 1 => encode_value(
                program,
                value,
                &MirType::from_kind(MirTypeKind::Shared(Box::new(args[0].clone()))),
            ),
            "Option" if args.len() == 1 => encode_option(program, value, &args[0]),
            "Result" if args.len() == 2 => encode_result(program, value, &args[0]),
            _ => Err(format!(
                "receipt type `{}` has no checked Codable definition",
                ty.display_name()
            )),
        },
        MirTypeKind::Union(_) => Err(format!(
            "receipt type `{}` has no checked Codable definition",
            ty.display_name()
        )),
        MirTypeKind::Fn(_) | MirTypeKind::SendFn { .. } | MirTypeKind::TraitObject(_) => Err(format!(
            "receipt type `{}` is not Codable",
            ty.display_name()
        )),
    }
}

/// Canonicalize one interpreter value using the checked MIR type and wire rows.
pub(crate) fn encode_interpreter_value(
    program: &MirProgram,
    value: &MirRuntimeValue,
    ty: &MirType,
) -> Result<MirRuntimeValue, String> {
    encode_value(program, value, ty)
}
