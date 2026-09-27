//! Generated, typed Source-MIR -> native MIR conversion for the private bootstrap.
//!
//! This module is deliberately a generator, rather than a handwritten carrier
//! projection.  The source MIR declarations and the binding manifest are the
//! inputs to the generator; every source field and variant is resolved through
//! the checked symbols before Rust glue is emitted.  A schema drift therefore
//! fails bootstrap artifact construction instead of becoming an opaque value or
//! a silently dropped field.

use crate::compiler_bootstrap_host::{BootstrapCodecSymbols, BootstrapHostCodecError};
use std::collections::BTreeMap;
use std::fmt::Write as _;

const SOURCE_MIR_SCHEMA: &str = include_str!("../../JetFoundation/Source/MIR/MIR.jet");
const SOURCE_SUPPORT_SCHEMAS: &[(&str, &[&str])] = &[
    (
        include_str!("../../JetFoundation/Source/Registry/CoreCalls.jet"),
        &[
            "Effect",
            "SinkClass",
            "CoreCallFallibility",
            "CoreCallPureRoute",
            "CoreCallInterpreterRoute",
            "CoreCallSymbol",
            "CoreMarkerApplication",
        ],
    ),
    (
        include_str!("../../JetFoundation/Source/Types/Types.jet"),
        &["ParamZone"],
    ),
    (
        include_str!("../../JetFoundation/Source/Target/Layout.jet"),
        &["ByteLayout", "FieldLayoutFacts", "LayoutFacts"],
    ),
];
const HOST_MIR_SCHEMA: &str = include_str!("../../../crates/jet-foundation/src/MIR.rs");

#[derive(Clone, Debug, PartialEq, Eq)]
struct Field {
    name: String,
    ty: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct VariantField {
    name: Option<String>,
    ty: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Variant {
    name: String,
    fields: Vec<VariantField>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Body {
    Struct(Vec<Field>),
    Enum(Vec<Variant>),
    Tuple,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Definition {
    name: String,
    body: Body,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TypeExpr {
    Named(String),
    Optional(Box<TypeExpr>),
    List(Box<TypeExpr>),
    Map(Box<TypeExpr>, Box<TypeExpr>),
}

/// Append the complete typed Source-MIR decoder to generated host glue.
pub(crate) fn append_runtime_mir_codec(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let mut source = parse_definitions(SOURCE_MIR_SCHEMA)
        .map_err(BootstrapHostCodecError::InvalidMetadata)?;
    for (support_schema, names) in SOURCE_SUPPORT_SCHEMAS {
        source.extend(
            parse_definitions(support_schema)
                .map_err(BootstrapHostCodecError::InvalidMetadata)?
                .into_iter()
                .filter(|definition| names.contains(&definition.name.as_str())),
        );
    }
    let host = parse_definitions(HOST_MIR_SCHEMA).map_err(BootstrapHostCodecError::InvalidMetadata)?;
    validate_unique_definitions(&source).map_err(BootstrapHostCodecError::InvalidMetadata)?;
    validate_unique_definitions(&host).map_err(BootstrapHostCodecError::InvalidMetadata)?;
    let mut host_by_name = host
        .into_iter()
        .map(|definition| (definition.name.clone(), definition))
        .collect::<BTreeMap<_, _>>();
    insert_external_host_definitions(&mut host_by_name);

    // Resolve the complete source schema up front.  This is intentionally
    // stricter than resolving only fields reached from MirProgram: a generated
    // Source MIR artifact must not gain an unverified carrier type merely by
    // moving a field behind an option or collection.
    for definition in &source {
        symbols.type_symbol(&definition.name)?;
        match &definition.body {
            Body::Struct(fields) => {
                for field in fields {
                    symbols.field_symbol(&definition.name, &field.name)?;
                }
            }
            Body::Enum(variants) => {
                for variant in variants {
                    symbols.variant_path(&definition.name, &variant.name)?;
                }
            }
            Body::Tuple => {}
        }
    }


    emit_special_structural_converters(out, symbols)?;
    writeln!(
        out,
        "\n// Source-MIR codec: generated from Compiler/JetFoundation/Source/MIR/MIR.jet.\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    for definition in &source {
        if definition.name.ends_with("Id") && definition.name != "MirTypeId" {
            emit_id_converter(out, symbols, &definition.name)?;
            continue;
        }
        if custom_source_converter(&definition.name) {
            continue;
        }
        match &definition.body {
            Body::Struct(fields) => {
                emit_struct_converter(out, symbols, &host_by_name, definition, fields)?
            }
            Body::Enum(variants) => {
                emit_enum_converter(out, symbols, &host_by_name, definition, variants)?
            }
            Body::Tuple => emit_tuple_converter(out, symbols, definition)?,
        }
    }
    emit_image_payload_codec(out, symbols, &source)?;


    // The top-level entry is named and stable so Runner/Native wiring never
    // needs to know how many schema rows the source MIR grows by.
    let program = source
        .iter()
        .find(|definition| definition.name == "MirProgram")
        .ok_or_else(|| BootstrapHostCodecError::MissingType("MirProgram".to_string()))?;
    let program_source = symbols.type_symbol("MirProgram")?;
    writeln!(
        out,
        "#[doc(hidden)]\npub(crate) fn __jet_bootstrap_mir_program_to_host(\n    value: &{program_source},\n) -> Result<::jet_foundation::MIR::MirProgram, String> {{\n    __jet_bootstrap_mir_MirProgram_to_host(value)\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    if !matches!(program.body, Body::Struct(_)) {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "Source MirProgram must remain a struct".to_string(),
        ));
    }
    Ok(())
}

fn insert_external_host_definitions(host_by_name: &mut BTreeMap<String, Definition>) {
    host_by_name.insert(
        "MirNameDeclaration".to_string(),
        Definition {
            name: "MirNameDeclaration".to_string(),
            body: Body::Struct(vec![
                Field {
                    name: "module".to_string(),
                    ty: "usize".to_string(),
                },
                Field {
                    name: "name".to_string(),
                    ty: "String".to_string(),
                },
                Field {
                    name: "path".to_string(),
                    ty: "String".to_string(),
                },
                Field {
                    name: "kind".to_string(),
                    ty: "String".to_string(),
                },
                Field {
                    name: "span".to_string(),
                    ty: "Span".to_string(),
                },
                Field {
                    name: "visibility".to_string(),
                    ty: "MirNameVisibility".to_string(),
                },
            ]),
        },
    );
    host_by_name.insert(
        "MirNameAlias".to_string(),
        Definition {
            name: "MirNameAlias".to_string(),
            body: Body::Struct(vec![
                Field {
                    name: "module".to_string(),
                    ty: "usize".to_string(),
                },
                Field {
                    name: "name".to_string(),
                    ty: "String".to_string(),
                },
                Field {
                    name: "target".to_string(),
                    ty: "String".to_string(),
                },
                Field {
                    name: "target_module".to_string(),
                    ty: "Option<usize>".to_string(),
                },
                Field {
                    name: "span".to_string(),
                    ty: "Span".to_string(),
                },
                Field {
                    name: "visibility".to_string(),
                    ty: "MirNameVisibility".to_string(),
                },
            ]),
        },
    );
    host_by_name.insert(
        "MirNameModule".to_string(),
        Definition {
            name: "MirNameModule".to_string(),
            body: Body::Struct(vec![
                Field {
                    name: "alias".to_string(),
                    ty: "String".to_string(),
                },
                Field {
                    name: "path".to_string(),
                    ty: "String".to_string(),
                },
                Field {
                    name: "package".to_string(),
                    ty: "String".to_string(),
                },
            ]),
        },
    );
    host_by_name.insert(
        "MirNameReference".to_string(),
        Definition {
            name: "MirNameReference".to_string(),
            body: Body::Struct(vec![
                Field {
                    name: "module_path".to_string(),
                    ty: "String".to_string(),
                },
                Field {
                    name: "kind".to_string(),
                    ty: "String".to_string(),
                },
                Field {
                    name: "def_span".to_string(),
                    ty: "Span".to_string(),
                },
                Field {
                    name: "semantic_identity".to_string(),
                    ty: "Option<String>".to_string(),
                },
            ]),
        },
    );
    host_by_name.insert(
        "MirNameVisibility".to_string(),
        Definition {
            name: "MirNameVisibility".to_string(),
            body: Body::Enum(vec![
                Variant {
                    name: "Private".to_string(),
                    fields: Vec::new(),
                },
                Variant {
                    name: "Package".to_string(),
                    fields: Vec::new(),
                },
                Variant {
                    name: "Public".to_string(),
                    fields: Vec::new(),
                },
            ]),
        },
    );
    host_by_name.insert(
        "MirStructureFactKind".to_string(),
        Definition {
            name: "MirStructureFactKind".to_string(),
            body: Body::Enum(vec![
                Variant {
                    name: "Liveness".to_string(),
                    fields: Vec::new(),
                },
                Variant {
                    name: "Lifecycle".to_string(),
                    fields: Vec::new(),
                },
                Variant {
                    name: "ImportEdge".to_string(),
                    fields: Vec::new(),
                },
            ]),
        },
    );
    host_by_name.insert(
        "MirStructureFact".to_string(),
        Definition {
            name: "MirStructureFact".to_string(),
            body: Body::Struct(vec![
                Field {
                    name: "kind".to_string(),
                    ty: "MirStructureFactKind".to_string(),
                },
                Field {
                    name: "subject".to_string(),
                    ty: "String".to_string(),
                },
                Field {
                    name: "source".to_string(),
                    ty: "String".to_string(),
                },
                Field {
                    name: "span".to_string(),
                    ty: "Span".to_string(),
                },
                Field {
                    name: "status".to_string(),
                    ty: "String".to_string(),
                },
                Field {
                    name: "detail".to_string(),
                    ty: "String".to_string(),
                },
                Field {
                    name: "gate".to_string(),
                    ty: "Option<String>".to_string(),
                },
            ]),
        },
    );
    host_by_name.insert(
        "MirHostImportFact".to_string(),
        Definition {
            name: "MirHostImportFact".to_string(),
            body: Body::Struct(vec![
                Field {
                    name: "id".to_string(),
                    ty: "String".to_string(),
                },
                Field {
                    name: "operation".to_string(),
                    ty: "String".to_string(),
                },
                Field {
                    name: "required_right".to_string(),
                    ty: "String".to_string(),
                },
                Field {
                    name: "parameter_type_ids".to_string(),
                    ty: "Vec<String>".to_string(),
                },
                Field {
                    name: "result_type_id".to_string(),
                    ty: "Option<String>".to_string(),
                },
            ]),
        },
    );
}

fn emit_special_structural_converters(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let value_fact_source = symbols.type_symbol("MirValueFact")?;
    let value_fact_id = symbols.field_symbol("MirValueFact", "id")?;
    let value_fact_ty = symbols.field_symbol("MirValueFact", "ty")?;
    let value_fact_span = symbols.field_symbol("MirValueFact", "span")?;
    let value_fact_ownership = symbols.field_symbol("MirValueFact", "ownership")?;
    writeln!(
        out,
        "fn __jet_bootstrap_mir_value_fact_to_host(value: &{value_fact_source}) -> Result<(::jet_foundation::MIR::MirValueId, ::jet_foundation::MIR::MirType, ::jet_foundation::Diagnostics::Span, ::jet_foundation::MIR::MirOwnership), String> {{\n    Ok((\n        __jet_bootstrap_mir_MirValueId_to_host(&value.{value_fact_id})?,\n        __jet_bootstrap_type_to_host(&value.{value_fact_ty})?,\n        __jet_bootstrap_span_to_host(&value.{value_fact_span})?,\n        __jet_bootstrap_mir_MirOwnership_to_host(&value.{value_fact_ownership})?,\n    ))\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    let provenance_source = symbols.type_symbol("MirViewProvenanceEntry")?;
    let provenance_slot = symbols.field_symbol("MirViewProvenanceEntry", "slot")?;
    let provenance_value = symbols.field_symbol("MirViewProvenanceEntry", "provenance")?;
    writeln!(
        out,
        "fn __jet_bootstrap_mir_view_provenance_to_host(values: &[{provenance_source}]) -> Result<::std::collections::BTreeMap<Vec<String>, ::jet_foundation::MIR::MirViewProvenance>, String> {{\n    let mut output = ::std::collections::BTreeMap::new();\n    for value in values {{\n        let key = value.{provenance_slot}.iter().cloned().collect::<Vec<_>>();\n        let converted = __jet_bootstrap_mir_MirViewProvenance_to_host(&value.{provenance_value})?;\n        if output.insert(key, converted).is_some() {{\n            return Err(\"duplicate Source MIR view provenance slot\".to_string());\n        }}\n    }}\n    Ok(output)\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    let job_limit_source = symbols.type_symbol("MirJobLimit")?;
    let job_limit_name = symbols.field_symbol("MirJobLimit", "name")?;
    let job_limit_value = symbols.field_symbol("MirJobLimit", "value")?;
    writeln!(
        out,
        "fn __jet_bootstrap_mir_job_limits_to_host(values: &[{job_limit_source}]) -> Result<::std::collections::BTreeMap<String, String>, String> {{\n    let mut output = ::std::collections::BTreeMap::new();\n    for value in values {{\n        if output.insert(value.{job_limit_name}.clone(), value.{job_limit_value}.clone()).is_some() {{\n            return Err(\"duplicate Source MIR job limit name\".to_string());\n        }}\n    }}\n    Ok(output)\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn emit_id_converter(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
    name: &str,
) -> Result<(), BootstrapHostCodecError> {
    let source = symbols.type_symbol(name)?;
    let value_field = symbols.field_symbol(name, "value")?;
    let host_path = host_type_path(name);
    writeln!(
        out,
        "fn __jet_bootstrap_mir_{name}_to_host(value: &{source}) -> Result<{host_path}, String> {{\n    let raw = value.{value_field}.to_string_rep().parse::<u64>().map_err(|_| \"MIR identity is not an unsigned integer\".to_string())?;\n    if raw == 0 {{ return Err(\"MIR identity must be non-zero\".to_string()); }}\n    Ok({host_path}(raw))\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn custom_source_converter(name: &str) -> bool {
    matches!(
        name,
        "Span"
            | "MirTypeId"
            | "MirMeasure"
            | "MirDimension"
            | "MirTagMarker"
            | "MirAbi"
            | "MirSize"
            | "MirType"
            | "MirValueFact"
            | "MirViewProvenanceEntry"
            | "MirJobLimit"
    ) || name.ends_with("Id")
}




fn emit_tuple_converter(
    _out: &mut String,
    _symbols: &BootstrapCodecSymbols<'_>,
    definition: &Definition,
) -> Result<(), BootstrapHostCodecError> {
    Err(BootstrapHostCodecError::InvalidMetadata(format!(
        "Source MIR tuple carrier `{}` has no checked native representation",
        definition.name
    )))
}

fn emit_struct_converter(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
    host_by_name: &BTreeMap<String, Definition>,
    definition: &Definition,
    source_fields: &[Field],
) -> Result<(), BootstrapHostCodecError> {
    let source = symbols.type_symbol(&definition.name)?;
    let host_path = host_type_path(&definition.name);
    let host_fields = match host_by_name.get(&definition.name) {
        Some(Definition {
            body: Body::Struct(fields),
            ..
        }) => fields.clone(),
        Some(Definition {
            body: Body::Tuple, ..
        }) => {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "native MIR type `{}` is a tuple carrier but Source MIR declares a struct",
                definition.name
            )))
        }
        Some(Definition {
            body: Body::Enum(_), ..
        }) => {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "native MIR type `{}` is an enum but Source MIR declares a struct",
                definition.name
            )))
        }
        None => source_fields.to_vec(),
    };
    let source_by_name = source_fields
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect::<BTreeMap<_, _>>();
    let mut used = BTreeMap::<String, String>::new();
    let mut initializers = Vec::new();

    for host_field in &host_fields {
        let source_name = source_field_name(&definition.name, &host_field.name);
        let source_field = source_by_name.get(source_name.as_str()).copied();
        let expression = if let Some(source_field) = source_field {
            used.insert(source_field.name.clone(), host_field.name.clone());
            convert_expression(
                &source_field.ty,
                &host_field.ty,
                &format!("value.{}", symbols.field_symbol(&definition.name, &source_field.name)?),
            )?
        } else {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "native MIR field `{}.{}` has no lossless Source-MIR field",
                definition.name, host_field.name
            )));
        };
        initializers.push(format!("        {}: {},", host_field.name, expression));
    }

    for source_field in source_fields {
        if !used.contains_key(&source_field.name)
            && !source_field_is_host_derived(&definition.name, &source_field.name)
        {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "Source MIR field `{}.{}` has no native MIR destination",
                definition.name, source_field.name
            )));
        }
    }

    writeln!(
        out,
        "fn __jet_bootstrap_mir_{}_to_host(value: &{}) -> Result<{}, String> {{\n    Ok({} {{\n{}\n    }})\n}}\n",
        definition.name,
        source,
        host_path,
        host_path,
        initializers.join("\n"),
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}


fn emit_enum_converter(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
    host_by_name: &BTreeMap<String, Definition>,
    definition: &Definition,
    source_variants: &[Variant],
) -> Result<(), BootstrapHostCodecError> {
    let source = symbols.type_symbol(&definition.name)?;
    let host_path = host_type_path(&definition.name);
    let host_variants = match host_by_name.get(&definition.name) {
        Some(Definition {
            body: Body::Enum(variants),
            ..
        }) => variants.clone(),
        Some(_) => {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "native MIR type `{}` is not an enum",
                definition.name
            )))
        }
        None => source_variants.to_vec(),
    };
    let host_by_variant = host_variants
        .iter()
        .map(|variant| (variant.name.as_str(), variant))
        .collect::<BTreeMap<_, _>>();
    let mut arms = Vec::new();
    for source_variant in source_variants {
        let source_path = symbols.variant_path(&definition.name, &source_variant.name)?;
        let host_name = host_variant_name(&definition.name, &source_variant.name);
        let host_variant = host_by_variant.get(host_name.as_str()).ok_or_else(|| {
            BootstrapHostCodecError::InvalidMetadata(format!(
                "native MIR enum `{}` has no variant `{}`",
                definition.name, source_variant.name
            ))
        })?;
        if source_variant.fields.len() != host_variant.fields.len() {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "MIR enum variant `{}::{}` changes payload arity",
                definition.name, source_variant.name
            )));
        }
        let bindings = (0..source_variant.fields.len())
            .map(|index| format!("__field_{index}"))
            .collect::<Vec<_>>();
        let pattern = if bindings.is_empty() {
            source_path.clone()
        } else {
            format!("{}({})", source_path, bindings.join(", "))
        };
        let values = source_variant
            .fields
            .iter()
            .zip(host_variant.fields.iter())
            .zip(bindings.iter())
            .map(|((source_field, host_field), binding)| {
                convert_expression(&source_field.ty, &host_field.ty, binding)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let constructor = if host_variant.fields.is_empty() {
            format!("{}::{}", host_path, host_variant.name)
        } else if host_variant.fields.iter().all(|field| field.name.is_some()) {
            let fields = host_variant
                .fields
                .iter()
                .zip(values.iter())
                .map(|(field, expression)| {
                    format!("{}: {}", field.name.as_deref().unwrap_or_default(), expression)
                })
                .collect::<Vec<_>>();
            format!("{}::{} {{ {} }}", host_path, host_variant.name, fields.join(", "))
        } else {
            format!("{}::{}({})", host_path, host_variant.name, values.join(", "))
        };
        arms.push(format!("        {} => Ok({}),", pattern, constructor));
    }
    writeln!(
        out,
        "fn __jet_bootstrap_mir_{}_to_host(value: &{}) -> Result<{}, String> {{\n    match value {{\n{}\n    }}\n}}\n",
        definition.name,
        source,
        host_path,
        arms.join("\n"),
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn convert_expression(
    source_type: &str,
    host_type: &str,
    expression: &str,
) -> Result<String, BootstrapHostCodecError> {
    let source_type = parse_type(source_type).map_err(BootstrapHostCodecError::InvalidMetadata)?;
    let host_type = parse_type(host_type).map_err(BootstrapHostCodecError::InvalidMetadata)?;
    convert_type_expression(&source_type, &host_type, expression)
}


fn convert_type_expression(
    source_type: &TypeExpr,
    host_type: &TypeExpr,
    expression: &str,
) -> Result<String, BootstrapHostCodecError> {
    match source_type {
        TypeExpr::Optional(inner) => {
            let host_inner = match host_type {
                TypeExpr::Optional(host_inner) => host_inner.as_ref(),
                _ => host_type,
            };
            let inner = convert_type_expression(inner, host_inner, "__inner")?;
            Ok(format!(
                "({expression}).as_ref().ok().map(|__inner| -> Result<_, String> {{ Ok({inner}) }}).transpose()?"
            ))
        }
        TypeExpr::List(inner) => {
            if let TypeExpr::Map(_, _) = host_type {
                if let TypeExpr::Named(name) = inner.as_ref() {
                    let helper = match name.as_str() {
                        "MirViewProvenanceEntry" => {
                            "__jet_bootstrap_mir_view_provenance_to_host"
                        }
                        "MirJobLimit" => "__jet_bootstrap_mir_job_limits_to_host",
                        _ => "",
                    };
                    if !helper.is_empty() {
                        return Ok(format!("{helper}(({expression}).as_ref())?"));
                    }
                }
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "Source MIR list has no checked native map converter".to_string(),
                ));
            }
            let host_inner = match host_type {
                TypeExpr::List(host_inner) => host_inner.as_ref(),
                _ => source_type,
            };
            let inner = convert_type_expression(inner, host_inner, "__item")?;
            let collection = match host_type {
                TypeExpr::Named(name) if name.contains("BTreeSet") || name.contains("HashSet") => {
                    "{ let __items = ({expression}).iter().map(|__item| Ok({inner})).collect::<Result<Vec<_>, String>>()?; let __count = __items.len(); let __output = __items.into_iter().collect::<::std::collections::BTreeSet<_>>(); if __output.len() != __count { return Err(\"duplicate MIR set element\".to_string()); } __output }?"
                }
                _ => "({expression}).iter().map(|__item| Ok({inner})).collect::<Result<Vec<_>, String>>()?",
            };
            Ok(collection
                .replace("{expression}", expression)
                .replace("{inner}", &inner))
        }
        TypeExpr::Map(key, value) => {
            let (host_key, host_value) = match host_type {
                TypeExpr::Map(host_key, host_value) => (host_key.as_ref(), host_value.as_ref()),
                _ => (key.as_ref(), value.as_ref()),
            };
            let key = convert_type_expression(key, host_key, "__key")?;
            let value = convert_type_expression(value, host_value, "__value")?;
            Ok(format!(
                "{{ let __items = ({expression}).iter().map(|(__key, __value)| Ok(({key}, {value}))).collect::<Result<Vec<_>, String>>()?; let __count = __items.len(); let __output = __items.into_iter().collect::<_>(); if __output.len() != __count {{ return Err(\"duplicate Source MIR map key\".to_string()); }} __output }}"
            ))
        }
        TypeExpr::Named(name) => convert_named_expression(name, host_type, expression),
    }
}

fn convert_named_expression(
    source_name: &str,
    host_type: &TypeExpr,
    expression: &str,
) -> Result<String, BootstrapHostCodecError> {
    if source_name == "MirValueFact"
        && matches!(host_type, TypeExpr::Named(name) if name.starts_with('('))
    {
        return Ok(format!(
            "__jet_bootstrap_mir_value_fact_to_host(&({expression}))?"
        ));
    }
    if source_name == "String"
        && matches!(
            host_type,
            TypeExpr::Named(name) if name == "std::path::PathBuf" || name.ends_with("::PathBuf")
        )
    {
        return Ok(format!(
            "::std::path::PathBuf::from(({expression}).clone())"
        ));
    }
    if source_name == "String"
        || source_name == "Bool"
        || source_name == "Float"
        || source_name == "Float32"
        || source_name == "Char"
        || source_name.starts_with('U')
        || source_name.starts_with('I')
    {
        if source_name == "Int" {
            return Ok(format!(
                "({expression}).to_string_rep().parse::<_>().map_err(|_| \"MIR integer is outside native range\".to_string())?"
            ));
        }
        return Ok(format!("({expression}).clone()"));
    }
    if source_name == "Span" {
        return Ok(format!("__jet_bootstrap_span_to_host(&({expression}))?"));
    }
    if let Some(helper) = existing_helper(source_name) {
        return Ok(format!("{helper}(&({expression}))?"));
    }
    let target = match host_type {
        TypeExpr::Optional(inner) => inner.as_ref(),
        _ => host_type,
    };
    let mut converted = format!("__jet_bootstrap_mir_{}_to_host(&({expression}))?", source_name);
    if let TypeExpr::Named(name) = target {
        if name.starts_with("Box<") {
            converted = format!("Box::new({converted})");
        }
    }
    Ok(converted)
}

fn existing_helper(name: &str) -> Option<&'static str> {
    match name {
        "MirTypeId" => Some("__jet_bootstrap_type_id_to_host"),
        "MirMeasure" => Some("__jet_bootstrap_measure_to_host"),
        "MirDimension" => Some("__jet_bootstrap_dimension_to_host"),
        "MirTagMarker" => Some("__jet_bootstrap_tag_to_host"),
        "MirAbi" => Some("__jet_bootstrap_abi_to_host"),
        "MirSize" => Some("__jet_bootstrap_size_to_host"),
        "MirType" => Some("__jet_bootstrap_type_to_host"),
        _ => None,
    }
}

fn host_type_path(name: &str) -> String {
    match name {
        "MirSchemaMigrationPlan" | "MirSchemaMigrationStep" | "MirSchemaMigrationOp" => {
            format!(
                "::jet_foundation::SchemaMigration::{}",
                name.trim_start_matches("Mir")
            )
        }
        "Span" => "::jet_foundation::Diagnostics::Span".to_string(),
        "ParamZone" => "::jet_foundation::MIR::MirParamZone".to_string(),
        "MirShapeFieldNames" => "::jet_foundation::Shape::ShapeFieldNames".to_string(),
        "MirOperatorMarker" => "::jet_foundation::AST::OperatorMarker".to_string(),
        "CoreMarkerApplication" => "::jet_foundation::Syntax::CoreMarkerApplication".to_string(),
        "Effect" => "::jet_foundation::Effects::Effect".to_string(),
        "SinkClass" => "::jet_foundation::Sinks::SinkClass".to_string(),
        "LayoutFacts" | "ByteLayout" | "FieldLayoutFacts" => {
            format!("::jet_foundation::Layout::{name}")
        }
        "MirDerivationRef" | "MirDerivationMethod" | "MirDerivationDisposition" => {
            format!(
                "::jet_foundation::Facts::{}",
                name.trim_start_matches("Mir")
            )
        }
        "MirFfiEvidenceBasis" => "::jet_foundation::AST::FfiEvidenceBasis".to_string(),
        "MirFfiBoundaryFacts" | "MirFfiBoundaryObligation" => {
            format!(
                "::jet_foundation::AST::{}",
                name.trim_start_matches("Mir")
            )
        }
        "MirModelOutputFact" => "::jet_foundation::AST::ModelOutputFact".to_string(),
        "MirTypedHeadKind" => "::jet_foundation::Syntax::TypedHeadKind".to_string(),
        "MirWebBucket" => "::jet_foundation::WebPartition::WebBucket".to_string(),
        "MirWebPartitionMarker" => {
            "::jet_foundation::WebPartition::WebPartitionMarker".to_string()
        }
        "MirFfiCloseSource" => "::jet_foundation::AST::FfiCloseSource".to_string(),
        "MirFfiThreadSafety" => "::jet_foundation::AST::FfiThreadSafety".to_string(),
        "MirResourceAccessMode" => {
            "::jet_foundation::ResourceSchedule::JetResourceAccessMode".to_string()
        }
        "MirResourceIdentity" => {
            "::jet_foundation::ResourceSchedule::JetResourceIdentity".to_string()
        }
        "MirResourceRegion" => {
            "::jet_foundation::ResourceSchedule::JetResourceRegion".to_string()
        }
        "MirResourceLayout" => {
            "::jet_foundation::ResourceSchedule::JetResourceLayout".to_string()
        }
        "MirResourceAccess" => {
            "::jet_foundation::ResourceSchedule::JetResourceAccess".to_string()
        }
        "MirFrameCompletion" => {
            "::jet_foundation::ResourceSchedule::JetFrameCompletion".to_string()
        }
        "MirFrameOperation" => {
            "::jet_foundation::ResourceSchedule::JetFrameOperation".to_string()
        }
        "MirFrameDependency" => {
            "::jet_foundation::ResourceSchedule::JetFrameDependency".to_string()
        }
        "MirFrameTransfer" => {
            "::jet_foundation::ResourceSchedule::JetFrameTransfer".to_string()
        }
        "MirFrameReuse" => {
            "::jet_foundation::ResourceSchedule::JetFrameReuse".to_string()
        }
        "MirFrameRetention" => {
            "::jet_foundation::ResourceSchedule::JetFrameRetention".to_string()
        }
        "MirFrameSchedule" => {
            "::jet_foundation::ResourceSchedule::JetFrameSchedule".to_string()
        }
        "MirHostImportFact" => "::jet_foundation::Authority::HostImportFact".to_string(),
        "MirNameFacts" => "::jet_foundation::MIR::MirNameFacts".to_string(),
        "MirStructureFact" | "MirStructureFactKind" => {
            format!(
                "::jet_foundation::Names::{}",
                name.trim_start_matches("Mir")
            )
        }
        "MirFixedReductionTree"
        | "MirFixedReductionOrder"
        | "MirAccelerationTransform"
        | "MirAccelerationProof"
        | "MirAccelerationWorkloadFacts" => {
            format!(
                "::jet_foundation::MIROptimization::Acceleration::{}",
                name.trim_start_matches("Mir")
            )
        }
        "CoreCallFallibility"
        | "CoreCallPureRoute"
        | "CoreCallInterpreterRoute"
        | "CoreCallSymbol" => {
            format!("::jet_foundation::Syntax::{name}")
        }
        "MirLayoutSupportFacts" | "MirLayoutCapabilityFacts" | "MirLayoutAlignmentFact" => {
            format!(
                "::jet_foundation::Layout::{}",
                name.trim_start_matches("Mir")
            )
        }
        "MirOSTarget" => "::jet_foundation::OSTarget::OSTarget".to_string(),
        "MirRuntimeLayer" => "::jet_foundation::RingLayer::RuntimeLayer".to_string(),
        "MirByteSize"
        | "MirMemoryAccess"
        | "MirMemoryRegion"
        | "MirLinkerInput"
        | "MirProviderAbi"
        | "MirProviderLimits"
        | "MirProviderContract"
        | "MirAllocatorPolicy"
        | "MirPanicPolicy"
        | "MirClockPolicy"
        | "MirEntropyPolicy"
        | "MirSchedulerPolicy"
        | "MirByteSinkPolicy"
        | "MirStartupPolicy"
        | "MirAuditPolicy"
        | "MirRegisterWidth"
        | "MirTargetRegisterAccessMode"
        | "MirTargetRegisterFact"
        | "MirTargetRegisterBlockFact"
        | "MirTargetRegisterOperation"
        | "MirTargetRegisterAccessFact"
        | "MirTargetInterruptFact"
        | "MirTargetInterruptHandlerFact"
        | "MirTargetDmaOwnership"
        | "MirTargetDmaChannelFact"
        | "MirTargetDmaOperation"
        | "MirTargetDmaOwner"
        | "MirTargetDmaOperationFact"
        | "MirTargetHardwareFacts"
        | "MirTargetHardwareUse"
        | "MirTargetHardwareUnresolvedReference"
        | "MirTargetProgrammerAdapter"
        | "MirTargetProgrammerFacts" => {
            format!("::jet_foundation::TargetMachine::{}", name.trim_start_matches("Mir"))
        }
        "MirTargetApplicability" => "::jet_foundation::MIR::MirTargetApplicability".to_string(),
        name if name.starts_with("MirName") => format!(
            "::jet_foundation::Names::{}",
            name.trim_start_matches("Mir")
        ),
        name if name.starts_with("MirApp") => format!(
            "::jet_foundation::App::{}",
            name.trim_start_matches("Mir")
        ),
        "MirDataPlanOperation"
        | "MirDataPlanSourceKind"
        | "MirDataPlanStreamMode"
        | "MirDataPlanPhysicalOperator" => {
            format!(
                "::jet_foundation::AST::DataPlan{}Kind",
                name.trim_start_matches("MirDataPlan")
            )
        }
        _ => format!("::jet_foundation::MIR::{name}"),
    }
}

fn source_field_name(owner: &str, host_field: &str) -> String {
    match (owner, host_field) {
        ("MirImport", "module") => "module_id".to_string(),
        ("MirTypeDef", "module") => "module_id".to_string(),
        ("MirFunction", "module") => "module_name".to_string(),
        ("MirPreludeCall", "module") => "module_name".to_string(),
        ("MirCoreCall", "module") => "module_name".to_string(),
        ("MirCoreCall", "effect") => "effect_kind".to_string(),
        ("MirForeign", "module") => "module_name".to_string(),
        ("MirCLib", "module") => "module_name".to_string(),
        ("MirTraitDef", "module")
        | ("MirImplDef", "module")
        | ("MirConstantDef", "module") => "module_id".to_string(),
        ("MirNameDeclaration", "module") | ("MirNameAlias", "module") => {
            "module_index".to_string()
        }
        _ => host_field.to_string(),
    }
}

fn source_field_is_host_derived(_owner: &str, _field: &str) -> bool {
    false
}

fn host_variant_name(owner: &str, variant: &str) -> String {
    if owner == "MirDropKind" && variant == "NoDrop" {
        "None".to_string()
    } else {
        variant.to_string()
    }
}
fn validate_unique_definitions(definitions: &[Definition]) -> Result<(), String> {
    let mut names = BTreeMap::new();
    for definition in definitions {
        if names.insert(definition.name.as_str(), ()).is_some() {
            return Err(format!("duplicate MIR schema type `{}`", definition.name));
        }
        match &definition.body {
            Body::Struct(fields) => {
                let mut field_names = BTreeMap::new();
                for field in fields {
                    if field_names.insert(field.name.as_str(), ()).is_some() {
                        return Err(format!(
                            "duplicate MIR schema field `{}.{}`",
                            definition.name, field.name
                        ));
                    }
                }
            }
            Body::Enum(variants) => {
                let mut variant_names = BTreeMap::new();
                for variant in variants {
                    if variant_names.insert(variant.name.as_str(), ()).is_some() {
                        return Err(format!(
                            "duplicate MIR schema variant `{}::{}`",
                            definition.name, variant.name
                        ));
                    }
                }
            }
            Body::Tuple => {}
        }
    }
    Ok(())
}

fn emit_image_payload_codec(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
    definitions: &[Definition],
) -> Result<(), BootstrapHostCodecError> {
    let by_name = definitions
        .iter()
        .map(|definition| (definition.name.as_str(), definition))
        .collect::<BTreeMap<_, _>>();
    for definition in definitions {
        match &definition.body {
            Body::Struct(fields) => {
                emit_image_struct_codec(out, symbols, &by_name, definition, fields)?;
            }
            Body::Enum(variants) => {
                emit_image_enum_codec(out, symbols, &by_name, definition, variants)?;
            }
            Body::Tuple => {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "Source MIR image type `{}` is an unsupported tuple",
                    definition.name
                )));
            }
        }
    }
    emit_image_span_codec(out, symbols)?;
    let program = symbols.type_symbol("MirProgram")?;
    writeln!(
        out,
        "#[doc(hidden)]\npub(crate) fn __jet_bootstrap_mir_program_to_image_payload(value: &{program}) -> Result<Vec<u8>, String> {{\n    let mut writer = crate::compiler_bootstrap_compiler_image::CompilerImagePayloadWriter::new();\n    __jet_bootstrap_image_encode_MirProgram(value, &mut writer)?;\n    Ok(writer.finish())\n}}\n#[doc(hidden)]\npub(crate) fn __jet_bootstrap_mir_program_from_image_payload(bytes: &[u8]) -> Result<{program}, String> {{\n    let mut reader = crate::compiler_bootstrap_compiler_image::CompilerImagePayloadReader::new(bytes);\n    let value = __jet_bootstrap_image_decode_MirProgram(&mut reader)?;\n    reader.finish()?;\n    Ok(value)\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn emit_image_struct_codec(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
    definitions: &BTreeMap<&str, &Definition>,
    definition: &Definition,
    fields: &[Field],
) -> Result<(), BootstrapHostCodecError> {
    let source = symbols.type_symbol(&definition.name)?;
    let mut encoder = String::new();
    let mut decoder_fields = Vec::new();
    for field in fields {
        let field_symbol = symbols.field_symbol(&definition.name, &field.name)?;
        encoder.push_str(&image_encode_expression(
            &field.ty,
            &format!("&value.{field_symbol}"),
            "writer",
            definitions,
            symbols,
        )?);
        let decoded = image_decode_expression(&field.ty, "reader", definitions, symbols)?;
        decoder_fields.push(format!("        {field_symbol}: {decoded},"));
    }
    writeln!(
        out,
        "fn __jet_bootstrap_image_encode_{}(value: &{}, writer: &mut crate::compiler_bootstrap_compiler_image::CompilerImagePayloadWriter) -> Result<(), String> {{\n{}    Ok(())\n}}\nfn __jet_bootstrap_image_decode_{}(reader: &mut crate::compiler_bootstrap_compiler_image::CompilerImagePayloadReader<'_>) -> Result<{}, String> {{\n    Ok({} {{\n{}\n    }})\n}}\n",
        definition.name,
        source,
        encoder,
        definition.name,
        source,
        source,
        decoder_fields.join("\n"),
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn emit_image_enum_codec(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
    definitions: &BTreeMap<&str, &Definition>,
    definition: &Definition,
    variants: &[Variant],
) -> Result<(), BootstrapHostCodecError> {
    let source = symbols.type_symbol(&definition.name)?;
    let mut encode_arms = Vec::new();
    let mut decode_arms = Vec::new();
    for (index, variant) in variants.iter().enumerate() {
        let discriminant = u32::try_from(index).map_err(|_| {
            BootstrapHostCodecError::InvalidMetadata(format!(
                "MIR enum `{}` exceeds the compiler-image tag range",
                definition.name
            ))
        })?;
        let path = symbols.variant_path(&definition.name, &variant.name)?;
        let bindings = (0..variant.fields.len())
            .map(|field| format!("__field_{field}"))
            .collect::<Vec<_>>();
        let pattern = if bindings.is_empty() {
            path.clone()
        } else {
            format!("{}({})", path, bindings.join(", "))
        };
        let mut encode_body = format!("            writer.write_u32({discriminant});\n");
        for (field, binding) in variant.fields.iter().zip(bindings.iter()) {
            encode_body.push_str(&image_encode_expression(
                &field.ty,
                binding,
                "writer",
                definitions,
                symbols,
            )?);
        }
        encode_arms.push(format!("        {pattern} => {{\n{encode_body}            Ok(())\n        }},"));
        let decoded = variant
            .fields
            .iter()
            .map(|field| image_decode_expression(&field.ty, "reader", definitions, symbols))
            .collect::<Result<Vec<_>, _>>()?;
        let constructor = if decoded.is_empty() {
            path
        } else {
            format!("{}({})", path, decoded.join(", "))
        };
        decode_arms.push(format!(
            "        {discriminant} => Ok({constructor}),"
        ));
    }
    writeln!(
        out,
        "fn __jet_bootstrap_image_encode_{}(value: &{}, writer: &mut crate::compiler_bootstrap_compiler_image::CompilerImagePayloadWriter) -> Result<(), String> {{\n    match value {{\n{}\n    }}\n}}\nfn __jet_bootstrap_image_decode_{}(reader: &mut crate::compiler_bootstrap_compiler_image::CompilerImagePayloadReader<'_>) -> Result<{}, String> {{\n    match reader.read_u32()? {{\n{}\n        _ => Err(\"unknown compiler-image MIR enum tag\".to_string()),\n    }}\n}}\n",
        definition.name,
        source,
        encode_arms.join("\n"),
        definition.name,
        source,
        decode_arms.join("\n"),
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn emit_image_span_codec(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let source = symbols.type_symbol("Span")?;
    let start = symbols.field_symbol("Span", "start")?;
    let end = symbols.field_symbol("Span", "end")?;
    let start_encode = image_encode_expression(
        "Int",
        &format!("&value.{start}"),
        "writer",
        &BTreeMap::new(),
        symbols,
    )?;
    let end_encode = image_encode_expression(
        "Int",
        &format!("&value.{end}"),
        "writer",
        &BTreeMap::new(),
        symbols,
    )?;
    let start_decode = image_decode_expression("Int", "reader", &BTreeMap::new(), symbols)?;
    let end_decode = image_decode_expression("Int", "reader", &BTreeMap::new(), symbols)?;
    writeln!(
        out,
        "fn __jet_bootstrap_image_encode_Span(value: &{source}, writer: &mut crate::compiler_bootstrap_compiler_image::CompilerImagePayloadWriter) -> Result<(), String> {{\n{start_encode}{end_encode}    Ok(())\n}}\nfn __jet_bootstrap_image_decode_Span(reader: &mut crate::compiler_bootstrap_compiler_image::CompilerImagePayloadReader<'_>) -> Result<{source}, String> {{\n    Ok({source} {{ {start}: {start_decode}, {end}: {end_decode} }})\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn image_encode_expression(
    source_type: &str,
    expression: &str,
    writer: &str,
    definitions: &BTreeMap<&str, &Definition>,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<String, BootstrapHostCodecError> {
    let ty = parse_type(source_type).map_err(BootstrapHostCodecError::InvalidMetadata)?;
    image_encode_type_expression(&ty, expression, writer, definitions, symbols)
}

fn image_encode_type_expression(
    ty: &TypeExpr,
    expression: &str,
    writer: &str,
    definitions: &BTreeMap<&str, &Definition>,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<String, BootstrapHostCodecError> {
    match ty {
        TypeExpr::Optional(inner) => {
            let inner = image_encode_type_expression(inner, "__value", writer, definitions, symbols)?;
            Ok(format!(
                "    match ({expression}).as_ref().ok() {{ Some(__value) => {{ {writer}.write_u8(1);\n{inner}    }}, None => {writer}.write_u8(0), }}\n"
            ))
        }
        TypeExpr::List(inner) => {
            let inner = image_encode_type_expression(inner, "__item", writer, definitions, symbols)?;
            Ok(format!(
                "    {writer}.write_len(({expression}).len())?;\n    for __item in ({expression}).iter() {{\n{inner}    }}\n"
            ))
        }
        TypeExpr::Map(key, value) => {
            let key_encode = image_encode_type_expression(
                key,
                "__key",
                "__key_writer",
                definitions,
                symbols,
            )?;
            let value_encode = image_encode_type_expression(
                value,
                "__value",
                "__value_writer",
                definitions,
                symbols,
            )?;
            Ok(format!(
                "    {{\n        let mut __entries = Vec::with_capacity(({expression}).len());\n        for (__key, __value) in ({expression}).iter() {{\n            let mut __key_writer = crate::compiler_bootstrap_compiler_image::CompilerImagePayloadWriter::new();\n{key_encode}            let mut __value_writer = crate::compiler_bootstrap_compiler_image::CompilerImagePayloadWriter::new();\n{value_encode}            __entries.push((__key_writer.finish(), __value_writer.finish()));\n        }}\n        __entries.sort_by(|left, right| left.0.cmp(&right.0));\n        if __entries.windows(2).any(|pair| pair[0].0 == pair[1].0) {{ return Err(\"duplicate Source MIR map key\".to_string()); }}\n        {writer}.write_len(__entries.len())?;\n        for (__key_bytes, __value_bytes) in __entries {{ {writer}.write_raw(&__key_bytes); {writer}.write_raw(&__value_bytes); }}\n    }}\n"
            ))
        }
        TypeExpr::Named(name) if name == "Span" => Ok(format!(
            "    __jet_bootstrap_image_encode_Span({expression}, {writer})?;\n"
        )),
        TypeExpr::Named(name) if name == "String" => Ok(format!(
            "    {writer}.write_string(({expression}).as_str())?;\n"
        )),
        TypeExpr::Named(name) if name == "Int" => Ok(format!(
            "    {{ let __integer = ({expression}).to_string_rep(); {writer}.write_string(&__integer)?; }}\n"
        )),
        TypeExpr::Named(name) if name == "Bool" => Ok(format!(
            "    {writer}.write_u8(u8::from(*({expression})));\n"
        )),
        TypeExpr::Named(name) if name == "Float" || name == "Float64" => Ok(format!(
            "    {writer}.write_u64(({expression}).to_bits());\n"
        )),
        TypeExpr::Named(name) if name == "Float32" => Ok(format!(
            "    {writer}.write_u32(({expression}).to_bits());\n"
        )),
        TypeExpr::Named(name) if name == "Char" => Ok(format!(
            "    {writer}.write_u32(*({expression}) as u32);\n"
        )),
        TypeExpr::Named(name) if name == "U8" => Ok(format!(
            "    {writer}.write_u8(*({expression}));\n"
        )),
        TypeExpr::Named(name) if name == "U16" => Ok(format!(
            "    {writer}.write_u16(*({expression}));\n"
        )),
        TypeExpr::Named(name) if name == "U32" => Ok(format!(
            "    {writer}.write_u32(*({expression}));\n"
        )),
        TypeExpr::Named(name) if name == "U64" => Ok(format!(
            "    {writer}.write_u64(*({expression}));\n"
        )),
        TypeExpr::Named(name) if name == "I8" => Ok(format!(
            "    {writer}.write_u8(*({expression}) as u8);\n"
        )),
        TypeExpr::Named(name) if name == "I16" => Ok(format!(
            "    {writer}.write_u16(*({expression}) as u16);\n"
        )),
        TypeExpr::Named(name) if name == "I32" => Ok(format!(
            "    {writer}.write_u32(*({expression}) as u32);\n"
        )),
        TypeExpr::Named(name) if name == "I64" => Ok(format!(
            "    {writer}.write_u64(*({expression}) as u64);\n"
        )),
        TypeExpr::Named(name) if name == "Usize" || name == "usize" => Ok(format!(
            "    {writer}.write_u64(u64::try_from(*({expression})).map_err(|_| \"MIR usize exceeds u64\".to_string())?);\n"
        )),
        TypeExpr::Named(name) if name == "Isize" || name == "isize" => Ok(format!(
            "    {writer}.write_u64((*({expression}) as i64) as u64);\n"
        )),
        TypeExpr::Named(name) if name.starts_with("Box<") => {
            let inner = generic_inner(name, "Box").ok_or_else(|| {
                BootstrapHostCodecError::InvalidMetadata(format!(
                    "invalid boxed MIR type `{name}`"
                ))
            })?;
            let inner_ty = parse_type(inner).map_err(BootstrapHostCodecError::InvalidMetadata)?;
            image_encode_type_expression(
                &inner_ty,
                &format!("&**({expression})"),
                writer,
                definitions,
                symbols,
            )
        }
        TypeExpr::Named(name) => {
            if !definitions.contains_key(name.as_str()) {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "Source MIR image has no schema for type `{name}`"
                )));
            }
            let _ = symbols.type_symbol(name)?;
            Ok(format!(
                "    __jet_bootstrap_image_encode_{name}({expression}, {writer})?;\n"
            ))
        }
    }
}

fn image_decode_expression(
    source_type: &str,
    reader: &str,
    definitions: &BTreeMap<&str, &Definition>,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<String, BootstrapHostCodecError> {
    let ty = parse_type(source_type).map_err(BootstrapHostCodecError::InvalidMetadata)?;
    image_decode_type_expression(&ty, reader, definitions, symbols)
}

fn image_decode_type_expression(
    ty: &TypeExpr,
    reader: &str,
    definitions: &BTreeMap<&str, &Definition>,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<String, BootstrapHostCodecError> {
    let decode = match ty {
        TypeExpr::Optional(inner) => {
            let inner = image_decode_type_expression(inner, reader, definitions, symbols)?;
            return Ok(format!(
                "{{ match {reader}.read_u8()? {{ 0 => Err(::jet_foundation::Outcome::JetAbsent), 1 => Ok({inner}), _ => return Err(\"invalid compiler-image MIR option tag\".to_string()), }} }}"
            ));
        }
        TypeExpr::List(inner) => {
            let inner = image_decode_type_expression(inner, reader, definitions, symbols)?;
            return Ok(format!(
                "{{ let __count = {reader}.read_len()?; let mut __values = Vec::new(); __values.try_reserve_exact(__count).map_err(|_| \"compiler-image MIR list is too large\".to_string())?; for _ in 0..__count {{ __values.push({inner}); }} __values }}"
            ));
        }
        TypeExpr::Map(key, value) => {
            let key_decode = image_decode_type_expression(key, reader, definitions, symbols)?;
            let value_decode = image_decode_type_expression(value, reader, definitions, symbols)?;
            let key_encode = image_encode_type_expression(
                key,
                "&__key",
                "__key_writer",
                definitions,
                symbols,
            )?;
            return Ok(format!(
                "{{ let __count = {reader}.read_len()?; let mut __seen = ::std::collections::BTreeSet::new(); let mut __entries = Vec::new(); __entries.try_reserve_exact(__count).map_err(|_| \"compiler-image MIR map is too large\".to_string())?; for _ in 0..__count {{ let __key = {key_decode}; let mut __key_writer = crate::compiler_bootstrap_compiler_image::CompilerImagePayloadWriter::new(); {key_encode} let __key_bytes = __key_writer.finish(); if !__seen.insert(__key_bytes) {{ return Err(\"duplicate compiler-image MIR map key\".to_string()); }} let __value = {value_decode}; __entries.push((__key, __value)); }} let __output = __entries.into_iter().collect::<_>(); if __output.len() != __count {{ return Err(\"duplicate compiler-image MIR map key\".to_string()); }} __output }}"
            ));
        }
        TypeExpr::Named(name) if name == "Span" => {
            return Ok(format!("__jet_bootstrap_image_decode_Span({reader})?"));
        }
        TypeExpr::Named(name) if name == "String" => {
            return Ok(format!("{reader}.read_string()?"));
        }
        TypeExpr::Named(name) if name == "Int" => {
            return Ok(format!(
                "{{ let __text = {reader}.read_string()?; let __value = jet_foundation::Numeric::JetInt::from_str(&__text)?; if __value.to_string_rep() != __text {{ return Err(\"non-canonical compiler-image MIR integer\".to_string()); }} __value }}"
            ));
        }
        TypeExpr::Named(name) if name == "Bool" => {
            return Ok(format!(
                "match {reader}.read_u8()? {{ 0 => false, 1 => true, _ => return Err(\"invalid compiler-image MIR boolean\".to_string()), }}"
            ));
        }
        TypeExpr::Named(name) if name == "Float" || name == "Float64" => {
            return Ok(format!("f64::from_bits({reader}.read_u64()?)"));
        }
        TypeExpr::Named(name) if name == "Float32" => {
            return Ok(format!("f32::from_bits({reader}.read_u32()?)"));
        }
        TypeExpr::Named(name) if name == "Char" => {
            return Ok(format!(
                "char::from_u32({reader}.read_u32()?).ok_or_else(|| \"invalid compiler-image MIR character\".to_string())?"
            ));
        }
        TypeExpr::Named(name) if name == "U8" => {
            return Ok(format!("{reader}.read_u8()?"));
        }
        TypeExpr::Named(name) if name == "U16" => {
            return Ok(format!("{reader}.read_u16()?"));
        }
        TypeExpr::Named(name) if name == "U32" => {
            return Ok(format!("{reader}.read_u32()?"));
        }
        TypeExpr::Named(name) if name == "U64" => {
            return Ok(format!("{reader}.read_u64()?"));
        }
        TypeExpr::Named(name) if name == "I8" => {
            return Ok(format!("{reader}.read_u8()? as i8"));
        }
        TypeExpr::Named(name) if name == "I16" => {
            return Ok(format!("{reader}.read_u16()? as i16"));
        }
        TypeExpr::Named(name) if name == "I32" => {
            return Ok(format!("{reader}.read_u32()? as i32"));
        }
        TypeExpr::Named(name) if name == "I64" => {
            return Ok(format!("{reader}.read_u64()? as i64"));
        }
        TypeExpr::Named(name) if name == "Usize" || name == "usize" => {
            return Ok(format!(
                "usize::try_from({reader}.read_u64()?).map_err(|_| \"compiler-image MIR usize exceeds host range\".to_string())?"
            ));
        }
        TypeExpr::Named(name) if name == "Isize" || name == "isize" => {
            return Ok(format!(
                "isize::try_from({reader}.read_u64()? as i64).map_err(|_| \"compiler-image MIR isize exceeds host range\".to_string())?"
            ));
        }
        TypeExpr::Named(name) if name.starts_with("Box<") => {
            let inner = generic_inner(name, "Box").ok_or_else(|| {
                BootstrapHostCodecError::InvalidMetadata(format!(
                    "invalid boxed MIR type `{name}`"
                ))
            })?;
            let inner_ty = parse_type(inner).map_err(BootstrapHostCodecError::InvalidMetadata)?;
            return Ok(format!(
                "Box::new({})",
                image_decode_type_expression(&inner_ty, reader, definitions, symbols)?
            ));
        }
        TypeExpr::Named(name) => {
            if !definitions.contains_key(name.as_str()) {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "Source MIR image has no schema for type `{name}`"
                )));
            }
            let _ = symbols.type_symbol(name)?;
            return Ok(format!("__jet_bootstrap_image_decode_{name}({reader})?"));
        }
    };
    Ok(decode)
}

fn parse_type(input: &str) -> Result<TypeExpr, String> {
    let input = input.trim().trim_end_matches(',').trim();
    if input.is_empty() {
        return Err("empty MIR field type".to_string());
    }
    if let Some(rest) = input.strip_prefix('?') {
        return Ok(TypeExpr::Optional(Box::new(parse_type(rest)?)));
    }
    if input.starts_with('[') && input.ends_with(']') {
        let body = &input[1..input.len() - 1];
        if let Some(index) = top_level_colon(body) {
            return Ok(TypeExpr::Map(
                Box::new(parse_type(&body[..index])?),
                Box::new(parse_type(&body[index + 1..])?),
            ));
        }
        return Ok(TypeExpr::List(Box::new(parse_type(body)?)));
    }
    if let Some(inner) = generic_inner(input, "Option") {
        return Ok(TypeExpr::Optional(Box::new(parse_type(inner)?)));
    }
    if let Some(inner) = generic_inner(input, "Vec") {
        return Ok(TypeExpr::List(Box::new(parse_type(inner)?)));
    }
    if let Some(inner) = generic_inner(input, "BTreeSet").or_else(|| generic_inner(input, "HashSet")) {
        return Ok(TypeExpr::Named(format!("BTreeSet<{inner}>")));
    }
    if let Some(inner) = generic_inner(input, "Box") {
        return Ok(TypeExpr::Named(format!("Box<{}>", inner.trim())));
    }
    if let Some(inner) = generic_inner(input, "BTreeMap").or_else(|| generic_inner(input, "HashMap")) {
        let parts = split_top_level(inner, ',');
        if parts.len() != 2 {
            return Err(format!("map type `{input}` does not have two parameters"));
        }
        return Ok(TypeExpr::Map(
            Box::new(parse_type(&parts[0])?),
            Box::new(parse_type(&parts[1])?),
        ));
    }
    Ok(TypeExpr::Named(input.to_string()))
}

fn generic_inner<'a>(input: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("{name}<");
    input
        .strip_prefix(&prefix)
        .and_then(|rest| rest.strip_suffix('>'))
}

fn parse_definitions(input: &str) -> Result<Vec<Definition>, String> {
    let input = strip_comments(input);
    let mut definitions = Vec::new();
    let mut cursor = 0;
    while let Some((keyword, start)) = find_next_definition(&input, cursor) {
        cursor = start + keyword.len();
        let mut position = cursor;
        skip_space(&input, &mut position);
        let name = read_identifier(&input, &mut position)?;
        skip_space(&input, &mut position);
        let body = match input.as_bytes().get(position).copied() {
            Some(b'{') => {
                let end = matching_delimiter(&input, position, b'{', b'}')?;
                let body = &input[position + 1..end];
                position = end + 1;
                if keyword == "struct" {
                    Body::Struct(parse_fields(body)?)
                } else {
                    Body::Enum(parse_variants(body)?)
                }
            }
            Some(b'(') => {
                let end = matching_delimiter(&input, position, b'(', b')')?;
                position = end + 1;
                Body::Tuple
            }
            _ => return Err(format!("MIR declaration `{name}` has no body")),
        };
        definitions.push(Definition { name, body });
        cursor = position;
    }
    Ok(definitions)
}

fn strip_comments(input: &str) -> String {
    input
        .lines()
        .map(|line| line.split_once("//").map_or(line, |(line, _)| line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn find_next_definition(input: &str, from: usize) -> Option<(&'static str, usize)> {
    let struct_position = find_keyword(input, "pub struct", from);
    let enum_position = find_keyword(input, "pub enum", from);
    match (struct_position, enum_position) {
        (Some(left), Some(right)) if left < right => Some(("struct", left + 4)),
        (Some(left), Some(right)) => Some(("enum", right + 4)),
        (Some(position), None) => Some(("struct", position + 4)),
        (None, Some(position)) => Some(("enum", position + 4)),
        (None, None) => None,
    }
}

fn find_keyword(input: &str, keyword: &str, from: usize) -> Option<usize> {
    let mut cursor = from;
    while let Some(relative) = input[cursor..].find(keyword) {
        let position = cursor + relative;
        let before = input.as_bytes().get(position.wrapping_sub(1)).copied();
        let after = input.as_bytes().get(position + keyword.len()).copied();
        if !before.is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            && !after.is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Some(position);
        }
        cursor = position + keyword.len();
    }
    None
}

fn parse_fields(input: &str) -> Result<Vec<Field>, String> {
    let starts = top_level_keyword_starts(input, "pub");
    let mut fields = Vec::new();
    for (index, start) in starts.iter().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(input.len());
        let field = input[*start..end].trim().trim_end_matches(',').trim();
        let mut cursor = 3;
        skip_space(field, &mut cursor);
        if field[cursor..].starts_with("(") {
            let end = matching_delimiter(field, cursor, b'(', b')')?;
            cursor = end + 1;
            skip_space(field, &mut cursor);
        }
        let name = read_identifier(field, &mut cursor)?;
        skip_space(field, &mut cursor);
        if field.as_bytes().get(cursor) != Some(&b':') {
            return Err(format!("field `{name}` has no type"));
        }
        cursor += 1;
        let ty = field[cursor..].trim().trim_end_matches(',').trim().to_string();
        fields.push(Field { name, ty });
    }
    Ok(fields)
}

fn parse_named_fields(input: &str) -> Result<Vec<Field>, String> {
    split_top_level(input, ',')
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .map(|part| {
            let index = top_level_colon(&part)
                .ok_or_else(|| format!("named MIR field `{part}` has no type"))?;
            Ok(Field {
                name: part[..index].trim().to_string(),
                ty: part[index + 1..].trim().to_string(),
            })
        })
        .collect()
}

fn parse_variants(input: &str) -> Result<Vec<Variant>, String> {
    let mut variants = Vec::new();
    let mut cursor = 0;
    while cursor < input.len() {
        skip_space_and_commas(input, &mut cursor);
        if cursor >= input.len() {
            break;
        }
        let name = read_identifier(input, &mut cursor)?;
        skip_space(input, &mut cursor);
        let fields = match input.as_bytes().get(cursor).copied() {
            Some(b'(') => {
                let end = matching_delimiter(input, cursor, b'(', b')')?;
                let fields = split_top_level(&input[cursor + 1..end], ',')
                    .into_iter()
                    .filter(|part| !part.trim().is_empty())
                    .map(parse_variant_field)
                    .collect::<Result<Vec<_>, _>>()?;
                cursor = end + 1;
                fields
            }
            Some(b'{') => {
                let end = matching_delimiter(input, cursor, b'{', b'}')?;
                let fields = parse_named_fields(&input[cursor + 1..end])?
                    .into_iter()
                    .map(|field| VariantField {
                        name: Some(field.name),
                        ty: field.ty,
                    })
                    .collect();
                cursor = end + 1;
                fields
            }
            _ => Vec::new(),
        };
        variants.push(Variant { name, fields });
    }
    Ok(variants)
}

fn parse_variant_field(input: String) -> Result<VariantField, String> {
    let input = input.trim().to_string();
    if let Some(index) = top_level_colon(&input) {
        Ok(VariantField {
            name: Some(input[..index].trim().to_string()),
            ty: input[index + 1..].trim().to_string(),
        })
    } else {
        Ok(VariantField {
            name: None,
            ty: input,
        })
    }
}

fn top_level_keyword_starts(input: &str, keyword: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut cursor = 0;
    let mut depth = 0usize;
    while cursor < input.len() {
        match input.as_bytes()[cursor] {
            b'[' | b'(' | b'{' | b'<' => depth += 1,
            b']' | b')' | b'}' | b'>' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0
            && input[cursor..].starts_with(keyword)
            && (cursor == 0 || !input.as_bytes()[cursor - 1].is_ascii_alphanumeric())
            && input
                .as_bytes()
                .get(cursor + keyword.len())
                .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'(')
        {
            starts.push(cursor);
        }
        cursor += 1;
    }
    starts
}

fn split_top_level(input: &str, delimiter: char) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    for (index, character) in input.char_indices() {
        match character {
            '[' | '(' | '{' | '<' => depth += 1,
            ']' | ')' | '}' | '>' => depth = depth.saturating_sub(1),
            character if character == delimiter && depth == 0 => {
                pieces.push(input[start..index].to_string());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    pieces.push(input[start..].to_string());
    pieces
}

fn top_level_colon(input: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, character) in input.char_indices() {
        match character {
            '[' | '(' | '{' | '<' => depth += 1,
            ']' | ')' | '}' | '>' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn matching_delimiter(input: &str, start: usize, open: u8, close: u8) -> Result<usize, String> {
    let mut depth = 0usize;
    for index in start..input.len() {
        match input.as_bytes()[index] {
            byte if byte == open => depth += 1,
            byte if byte == close => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Ok(index);
                }
            }
            _ => {}
        }
    }
    Err(format!("unclosed MIR delimiter at byte {start}"))
}

fn skip_space(input: &str, cursor: &mut usize) {
    while input
        .as_bytes()
        .get(*cursor)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        *cursor += 1;
    }
}

fn skip_space_and_commas(input: &str, cursor: &mut usize) {
    while input
        .as_bytes()
        .get(*cursor)
        .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b',')
    {
        *cursor += 1;
    }
}

fn read_identifier(input: &str, cursor: &mut usize) -> Result<String, String> {
    let start = *cursor;
    while input.as_bytes().get(*cursor).is_some_and(|byte| {
        byte.is_ascii_alphanumeric() || *byte == b'_'
    }) {
        *cursor += 1;
    }
    if *cursor == start {
        Err(format!("expected MIR identifier at byte {start}"))
    } else {
        Ok(input[start..*cursor].to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_definitions, parse_type, Body, TypeExpr};

    #[test]
    fn source_schema_parser_retains_nested_typed_rows() {
        let definitions = parse_definitions(
            "pub struct Row { pub id: Int pub values: [String] pub attrs: [String: ?MirType] }\n\
             pub enum Choice { Empty Full(name: String, row: Row) }",
        )
        .expect("schema parser");
        assert_eq!(definitions.len(), 2);
        let Body::Struct(fields) = &definitions[0].body else {
            panic!("expected struct");
        };
        assert_eq!(fields[2].ty, "[String: ?MirType]");
        let Body::Enum(variants) = &definitions[1].body else {
            panic!("expected enum");
        };
        assert_eq!(variants[1].fields.len(), 2);
        assert_eq!(variants[1].fields[1].ty, "Row");
    }

    #[test]
    fn type_parser_distinguishes_optional_list_and_map() {
        assert_eq!(
            parse_type("?[String: ?MirType]").expect("type"),
            TypeExpr::Optional(Box::new(TypeExpr::Map(
                Box::new(TypeExpr::Named("String".to_string())),
                Box::new(TypeExpr::Optional(Box::new(TypeExpr::Named(
                    "MirType".to_string(),
                )))),
            )))
        );
    }
}
