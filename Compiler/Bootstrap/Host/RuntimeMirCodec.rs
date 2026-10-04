//! Generated, typed Source-MIR <-> native MIR conversion for the private bootstrap.
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
/// Native carriers outside the parsed `MIR.rs` rows, keyed by the
/// `jet_foundation` module path `host_type_path` names them under; a module
/// declared across several files lists each. Only the definitions Source MIR
/// maps onto are read, so their real shapes (tuple variants, `&'static str`,
/// set fields) drive the generated glue rather than the Source declaration.
const HOST_SUPPORT_SCHEMAS: &[(&str, &str)] = &[
    ("MIR", HOST_MIR_SCHEMA),
    ("App", include_str!("../../../crates/jet-foundation/src/App.rs")),
    ("AST", include_str!("../../../crates/jet-foundation/src/AST/data_plan.rs")),
    ("AST", include_str!("../../../crates/jet-foundation/src/AST/ffi.rs")),
    ("AST", include_str!("../../../crates/jet-foundation/src/AST/items.rs")),
    ("Authority", include_str!("../../../crates/jet-foundation/src/Authority.rs")),
    ("Effects", include_str!("../../../crates/jet-foundation/src/Effects.rs")),
    ("Facts", include_str!("../../../crates/jet-foundation/src/Facts.rs")),
    ("Facts", include_str!("../../../crates/jet-foundation/src/FactsDerivation.rs")),
    ("Layout", include_str!("../../../crates/jet-foundation/src/Layout.rs")),
    (
        "MIROptimization::Acceleration",
        include_str!("../../../crates/jet-foundation/src/MIROptimization/Acceleration.rs"),
    ),
    ("Names", include_str!("../../../crates/jet-foundation/src/Names.rs")),
    ("OSTarget", include_str!("../../../crates/jet-foundation/src/OSTarget.rs")),
    ("ResourceSchedule", include_str!("../../../crates/jet-foundation/src/ResourceSchedule.rs")),
    ("RingLayer", include_str!("../../../crates/jet-foundation/src/RingLayer.rs")),
    ("SchemaMigration", include_str!("../../../crates/jet-foundation/src/SchemaMigration.rs")),
    ("Shape", include_str!("../../../crates/jet-foundation/src/Shape.rs")),
    ("Syntax", include_str!("../../../crates/jet-foundation/src/Syntax/core_calls.rs")),
    ("Syntax", include_str!("../../../crates/jet-foundation/src/Syntax/core_surface.rs")),
    ("Syntax", include_str!("../../../crates/jet-foundation/src/Syntax/sinks.rs")),
    ("TargetMachine", include_str!("../../../crates/jet-foundation/src/TargetMachine.rs")),
    ("WebPartition", include_str!("../../../crates/jet-foundation/src/WebPartition.rs")),
];

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

/// A Source MIR record that stands for one native tuple: Jet spells a native
/// `(A, B, ..)` as a record over the same fields in order, and a native map as
/// a list of two-field key/value records.
#[derive(Clone, Debug)]
struct PairRecord {
    symbol: String,
    /// Each field's emitted symbol and Source type, in declaration order.
    fields: Vec<(String, String)>,
}

impl PairRecord {
    /// The key and value fields of a record standing for one map entry.
    fn entry(&self) -> Result<(&(String, String), &(String, String)), BootstrapHostCodecError> {
        match self.fields.as_slice() {
            [key, value] => Ok((key, value)),
            _ => Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "Source MIR record `{}` stands for a native map entry but has {} fields",
                self.symbol,
                self.fields.len()
            ))),
        }
    }
}

type PairRecords = BTreeMap<String, PairRecord>;

fn pair_records(
    source: &[Definition],
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<PairRecords, BootstrapHostCodecError> {
    let mut pairs = PairRecords::new();
    for definition in source {
        let Body::Struct(fields) = &definition.body else {
            continue;
        };
        if fields.len() < 2 {
            continue;
        }
        let fields = fields
            .iter()
            .map(|field| {
                Ok((
                    symbols.field_symbol(&definition.name, &field.name)?.to_string(),
                    field.ty.clone(),
                ))
            })
            .collect::<Result<Vec<_>, BootstrapHostCodecError>>()?;
        pairs.insert(
            definition.name.clone(),
            PairRecord {
                symbol: symbols.type_symbol(&definition.name)?.to_string(),
                fields,
            },
        );
    }
    Ok(pairs)
}

/// The host element types of a parsed native tuple type `(A, B, ..)` that a
/// Source record of the same arity stands for, if it is one.
fn host_tuple_types(
    host_type: &TypeExpr,
    record: &PairRecord,
) -> Result<Option<Vec<TypeExpr>>, BootstrapHostCodecError> {
    let TypeExpr::Named(name) = host_type else {
        return Ok(None);
    };
    let Some(inner) = name.strip_prefix('(').and_then(|rest| rest.strip_suffix(')')) else {
        return Ok(None);
    };
    let parts = split_top_level(inner, ',')
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>();
    if parts.len() != record.fields.len() {
        return Ok(None);
    }
    parts
        .iter()
        .map(|part| parse_type(part).map_err(BootstrapHostCodecError::InvalidMetadata))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

/// Append the typed Source-MIR and native-MIR converters to generated host glue.
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
    // Source declarations spell acronyms in capitals (D-ACRO-LEX1); key each
    // native definition by that spelling and keep its Rust name on the row.
    let mut host_by_name = host
        .into_iter()
        .map(|definition| {
            (
                ::jet_foundation::Syntax::respell_acronym_name(&definition.name),
                definition,
            )
        })
        .collect::<BTreeMap<_, _>>();
    insert_external_host_definitions(&mut host_by_name);
    for error in insert_support_host_definitions(&source, &mut host_by_name) {
        symbols.record(error);
    }

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
    let pairs = pair_records(&source, symbols)?;


    emit_special_structural_converters(out, symbols)?;
    writeln!(
        out,
        "\n// Typed Source-MIR/native-MIR projection generated from Compiler/JetFoundation/Source/MIR/MIR.jet.\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    // Every converter is attempted; failures are recorded on `symbols` so the
    // packaging run reports each drifted definition, not just the first.
    let keep = |result: Result<(), BootstrapHostCodecError>| {
        if let Err(error) = result {
            symbols.record(error);
        }
    };
    for definition in &source {
        if id_newtype(definition) {
            keep(emit_id_converter(out, symbols, &host_by_name, &definition.name));
            continue;
        }
        if custom_source_converter(&definition.name) || source_only_type(&definition.name) || native_type_absent(&definition.name, &host_by_name) {
            continue;
        }
        keep(match &definition.body {
            Body::Struct(fields) => {
                emit_struct_converter(out, symbols, &host_by_name, &pairs, definition, fields)
            }
            Body::Enum(variants) => {
                emit_enum_converter(out, symbols, &host_by_name, &pairs, definition, variants)
            }
            Body::Tuple => emit_tuple_converter(out, symbols, definition),
        });
    }
    emit_special_host_to_source_converters(out, symbols)?;
    for definition in &source {
        if id_newtype(definition) {
            keep(emit_id_from_host_converter(out, symbols, &host_by_name, &definition.name));
            continue;
        }
        if custom_source_converter(&definition.name) || source_only_type(&definition.name) || native_type_absent(&definition.name, &host_by_name) {
            continue;
        }
        keep(match &definition.body {
            Body::Struct(fields) => {
                emit_struct_from_host_converter(out, symbols, &host_by_name, &pairs, definition, fields)
            }
            Body::Enum(variants) => {
                emit_enum_from_host_converter(out, symbols, &host_by_name, &pairs, definition, variants)
            }
            Body::Tuple => emit_tuple_from_host_converter(definition),
        });
    }


    // The top-level entry is named and stable so Runner/Native wiring never
    // needs to know how many schema rows the source MIR grows by.
    let program = source
        .iter()
        .find(|definition| definition.name == "MIRProgram")
        .ok_or_else(|| BootstrapHostCodecError::MissingType("MIRProgram".to_string()))?;
    let program_source = symbols.type_symbol("MIRProgram")?;
    emit_binary_overflow_derivation(out, symbols)?;
    writeln!(
        out,
        "#[doc(hidden)]\npub(crate) fn __jet_bootstrap_mir_program_to_host(\n    value: &{program_source},\n) -> Result<::jet_foundation::MIR::MirProgram, String> {{\n    __jet_bootstrap_with_trap_routes(__jet_bootstrap_source_trap_routes(value), || __jet_bootstrap_mir_MIRProgram_to_host(value))\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    if !matches!(program.body, Body::Struct(_)) {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "Source MirProgram must remain a struct".to_string(),
        ));
    }
    writeln!(
        out,
        "#[doc(hidden)]\npub(crate) fn __jet_bootstrap_mir_program_from_host(\n    value: &::jet_foundation::MIR::MirProgram,\n) -> Result<{program_source}, String> {{\n    __jet_bootstrap_with_trap_routes(__jet_bootstrap_host_trap_routes(value), || __jet_bootstrap_mir_MIRProgram_from_host(value))\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    // Runner's packaging (compiler image, run-from-host wrapper) names the
    // Source program carrier through this one alias.
    writeln!(
        out,
        "#[doc(hidden)]\npub(crate) type __JetBootstrapSourceProgram = {program_source};\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}

fn insert_external_host_definitions(host_by_name: &mut BTreeMap<String, Definition>) {
    // Native MIR declares its identity newtypes with `mir_id!(MirBlockId);`,
    // which the struct/enum schema parse does not see; key each by its
    // Source spelling (`MIRBlockID`) so identity converters name the Rust type.
    for line in HOST_MIR_SCHEMA.lines() {
        let Some(name) = line
            .trim()
            .strip_prefix("mir_id!(")
            .and_then(|rest| rest.strip_suffix(");"))
        else {
            continue;
        };
        host_by_name.insert(
            ::jet_foundation::Syntax::respell_acronym_name(name),
            Definition {
                name: name.to_string(),
                body: Body::Tuple,
            },
        );
    }
}

/// Key each native carrier outside the parsed `MIR.rs` rows by its Source
/// name: the definition `host_type_path` names, read from that module's source.
/// A Source definition `host_type_path` leaves on the default `MIR::` path has
/// no native carrier (see `native_type_absent`); one it maps elsewhere must be
/// declared there, and each that is not is reported.
fn insert_support_host_definitions(
    source: &[Definition],
    host_by_name: &mut BTreeMap<String, Definition>,
) -> Vec<BootstrapHostCodecError> {
    let mut errors = Vec::new();
    for definition in source {
        let name = definition.name.as_str();
        if host_by_name.contains_key(name)
            || id_newtype(definition)
            || custom_source_converter(name)
            || source_only_type(name)
        {
            continue;
        }
        let path = host_type_path(name, host_by_name);
        if path == format!("::jet_foundation::MIR::{name}") {
            continue;
        }
        match host_support_definition(&path) {
            Ok(Some(host)) => {
                host_by_name.insert(name.to_string(), host);
            }
            Ok(None) => errors.push(BootstrapHostCodecError::InvalidMetadata(format!(
                "Source MIR `{name}` maps to native `{path}`, which its module source does not declare"
            ))),
            Err(error) => errors.push(BootstrapHostCodecError::InvalidMetadata(format!(
                "Source MIR `{name}` maps to native `{path}`, whose declaration does not parse: {error}"
            ))),
        }
    }
    errors
}

/// The declaration of `::jet_foundation::{module}::{name}` in the module
/// sources of [`HOST_SUPPORT_SCHEMAS`], if one declares it.
fn host_support_definition(path: &str) -> Result<Option<Definition>, String> {
    let Some((module, name)) = path
        .strip_prefix("::jet_foundation::")
        .and_then(|rest| rest.rsplit_once("::"))
    else {
        return Ok(None);
    };
    for (schema_module, schema) in HOST_SUPPORT_SCHEMAS {
        if *schema_module != module {
            continue;
        }
        let input = strip_comments(schema);
        let mut cursor = 0;
        while let Some((keyword, start)) = find_next_definition(&input, cursor) {
            let mut position = start + keyword.len();
            skip_space(&input, &mut position);
            cursor = position;
            if read_identifier(&input, &mut position).ok().as_deref() == Some(name) {
                return parse_definition_body(&input, keyword, name.to_string(), position)
                    .map(|(definition, _)| Some(definition));
            }
        }
    }
    Ok(None)
}

fn emit_special_structural_converters(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    // Map/set fields collect into whichever keyed carrier the field declares
    // (BTreeMap, HashMap, BTreeSet); a shorter result means a duplicate key.
    out.push_str(
        "fn __jet_bootstrap_mir_collect_unique<C, T>(items: Vec<T>, duplicate: &str) -> Result<C, String>\nwhere\n    C: ::std::iter::FromIterator<T>,\n    for<'a> &'a C: ::std::iter::IntoIterator,\n{\n    let count = items.len();\n    let output: C = items.into_iter().collect();\n    if (&output).into_iter().count() != count {\n        return Err(duplicate.to_string());\n    }\n    Ok(output)\n}\n",
    );
    // See `host_nominal_ref`: a trait-object bound is the trait ref's id and
    // name under the native nominal-ref carrier.
    let trait_ref = symbols.type_symbol("MIRTraitRef")?;
    writeln!(
        out,
        "fn __jet_bootstrap_mir_trait_bound_to_host(value: &{trait_ref}) -> Result<::jet_foundation::MIR::MirNominalRef, String> {{\n    let bound = __jet_bootstrap_mir_MIRTraitRef_to_host(value)?;\n    Ok(::jet_foundation::MIR::MirNominalRef {{ id: ::jet_foundation::MIR::MirTypeId(bound.id.0), name: bound.name }})\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    let provenance_source = symbols.type_symbol("MIRViewProvenanceEntry")?;
    let provenance_slot = symbols.field_symbol("MIRViewProvenanceEntry", "slot")?;
    let provenance_value = symbols.field_symbol("MIRViewProvenanceEntry", "provenance")?;
    writeln!(
        out,
        "fn __jet_bootstrap_mir_view_provenance_to_host(values: &[{provenance_source}]) -> Result<::std::collections::BTreeMap<Vec<String>, ::jet_foundation::MIR::MirViewProvenance>, String> {{\n    let mut output = ::std::collections::BTreeMap::new();\n    for value in values {{\n        let key = value.{provenance_slot}.iter().cloned().collect::<Vec<_>>();\n        let converted = __jet_bootstrap_mir_MIRViewProvenance_to_host(&value.{provenance_value})?;\n        if output.insert(key, converted).is_some() {{\n            return Err(\"duplicate Source MIR view provenance slot\".to_string());\n        }}\n    }}\n    Ok(output)\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    let job_limit_source = symbols.type_symbol("MIRJobLimit")?;
    let job_limit_name = symbols.field_symbol("MIRJobLimit", "name")?;
    let job_limit_value = symbols.field_symbol("MIRJobLimit", "value")?;
    writeln!(
        out,
        "fn __jet_bootstrap_mir_job_limits_to_host(values: &[{job_limit_source}]) -> Result<::std::collections::BTreeMap<String, String>, String> {{\n    let mut output = ::std::collections::BTreeMap::new();\n    for value in values {{\n        if output.insert(value.{job_limit_name}.clone(), value.{job_limit_value}.clone()).is_some() {{\n            return Err(\"duplicate Source MIR job limit name\".to_string());\n        }}\n    }}\n    Ok(output)\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn emit_special_host_to_source_converters(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let trait_ref = symbols.type_symbol("MIRTraitRef")?;
    writeln!(
        out,
        "fn __jet_bootstrap_mir_trait_bound_from_host(value: &::jet_foundation::MIR::MirNominalRef) -> Result<{trait_ref}, String> {{\n    __jet_bootstrap_mir_MIRTraitRef_from_host(&::jet_foundation::MIR::MirTraitRef {{ id: ::jet_foundation::MIR::MirTraitId(value.id.0), name: value.name.clone() }})\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    let provenance_source = symbols.type_symbol("MIRViewProvenanceEntry")?;
    let provenance_slot = symbols.field_symbol("MIRViewProvenanceEntry", "slot")?;
    let provenance_value = symbols.field_symbol("MIRViewProvenanceEntry", "provenance")?;
    writeln!(
        out,
        "fn __jet_bootstrap_mir_view_provenance_from_host(values: &::std::collections::BTreeMap<Vec<String>, ::jet_foundation::MIR::MirViewProvenance>) -> Result<Vec<{provenance_source}>, String> {{\n    let mut output = Vec::with_capacity(values.len());\n    for (slot, provenance) in values {{\n        output.push({provenance_source} {{\n            {provenance_slot}: slot.iter().cloned().collect(),\n            {provenance_value}: __jet_bootstrap_mir_MIRViewProvenance_from_host(provenance)?,\n        }});\n    }}\n    Ok(output)\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    let job_limit_source = symbols.type_symbol("MIRJobLimit")?;
    let job_limit_name = symbols.field_symbol("MIRJobLimit", "name")?;
    let job_limit_value = symbols.field_symbol("MIRJobLimit", "value")?;
    writeln!(
        out,
        "fn __jet_bootstrap_mir_job_limits_from_host(values: &::std::collections::BTreeMap<String, String>) -> Result<Vec<{job_limit_source}>, String> {{\n    Ok(values.iter().map(|(name, value)| {job_limit_source} {{ {job_limit_name}: name.clone(), {job_limit_value}: value.clone() }}).collect())\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn emit_id_from_host_converter(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
    host_by_name: &BTreeMap<String, Definition>,
    name: &str,
) -> Result<(), BootstrapHostCodecError> {
    let source = symbols.type_symbol(name)?;
    let value_field = symbols.field_symbol(name, "value")?;
    let host_path = host_type_path(name, host_by_name);
    writeln!(
        out,
        "fn __jet_bootstrap_mir_{name}_from_host(value: &{host_path}) -> Result<{source}, String> {{\n    let raw = value.0;\n    if raw == 0 {{ return Err(\"MIR identity must be non-zero\".to_string()); }}\n    Ok({source} {{ {value_field}: raw }})\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn emit_tuple_from_host_converter(
    definition: &Definition,
) -> Result<(), BootstrapHostCodecError> {
    Err(BootstrapHostCodecError::InvalidMetadata(format!(
        "Source MIR tuple carrier `{}` has no checked native representation",
        definition.name
    )))
}

fn emit_struct_from_host_converter(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
    host_by_name: &BTreeMap<String, Definition>,
    pairs: &PairRecords,
    definition: &Definition,
    source_fields: &[Field],
) -> Result<(), BootstrapHostCodecError> {
    let source = symbols.type_symbol(&definition.name)?;
    let host_path = host_type_path(&definition.name, host_by_name);
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
        None => return Err(missing_host_definition(&definition.name)),
    };
    let mut initializers = Vec::new();
    for source_field in source_fields {
        let source_field_symbol = symbols.field_symbol(&definition.name, &source_field.name)?;
        if let Some(expression) = custom_struct_field_from_host(&definition.name, &source_field.name) {
            initializers.push(format!("        {source_field_symbol}: {expression},"));
            continue;
        }
        let host_field = host_fields
            .iter()
            .find(|field| source_field_name(&definition.name, &field.name) == source_field.name)
            .ok_or_else(|| {
                BootstrapHostCodecError::InvalidMetadata(format!(
                    "Source MIR field `{}.{}` has no native MIR source",
                    definition.name, source_field.name
                ))
            })?;
        let expression = convert_from_host_expression(
            &source_field.ty,
            &host_field.ty,
            &format!("&value.{}", host_field.name),
            pairs,
        )?;
        initializers.push(format!(
            "        {source_field_symbol}: {},",
            into_source_field(&source_field.ty, &expression)
        ));
    }

    // A native field Source MIR does not carry crosses to native MIR as one
    // fixed value (`custom_struct_field_to_host`); any other native value would
    // be dropped by the round trip, so it is refused instead.
    let mut checks = String::new();
    for host_field in &host_fields {
        let source_name = source_field_name(&definition.name, &host_field.name);
        if source_fields.iter().any(|field| field.name == source_name) {
            continue;
        }
        if let Some(fixed) = custom_struct_field_to_host(symbols, &definition.name, &host_field.name)? {
            writeln!(
                checks,
                "    if !matches!(&value.{0}, {fixed}) {{ return Err(\"native MIR field `{1}.{0}` holds a value Source MIR does not carry\".to_string()); }}",
                host_field.name, definition.name
            )
            .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
        }
    }
    writeln!(
        out,
        "fn __jet_bootstrap_mir_{}_from_host(value: &{}) -> Result<{}, String> {{\n{checks}    Ok({} {{\n{}\n    }})\n}}\n",
        definition.name,
        host_path,
        source,
        source,
        initializers.join("\n"),
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn emit_enum_from_host_converter(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
    host_by_name: &BTreeMap<String, Definition>,
    pairs: &PairRecords,
    definition: &Definition,
    source_variants: &[Variant],
) -> Result<(), BootstrapHostCodecError> {
    let source = symbols.type_symbol(&definition.name)?;
    let host_path = host_type_path(&definition.name, host_by_name);
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
        None => return Err(missing_host_definition(&definition.name)),
    };
    let mut arms = Vec::new();
    for source_variant in source_variants {
        let source_path = symbols.variant_path(&definition.name, &source_variant.name)?;
        let host_name = host_variant_name(&definition.name, &source_variant.name);
        let host_variant = host_variants
            .iter()
            .find(|variant| ::jet_foundation::Syntax::respell_acronym_name(&variant.name) == host_name)
            .ok_or_else(|| {
                BootstrapHostCodecError::InvalidMetadata(format!(
                    "native MIR enum `{}` has no variant `{}`",
                    definition.name, host_name
                ))
            })?;
        let aligned = align_variant_fields(&definition.name, source_variant, host_variant)?;
        let bindings = (0..host_variant.fields.len())
            .map(|index| format!("__field_{index}"))
            .collect::<Vec<_>>();
        let pattern = if host_variant.fields.is_empty() {
            format!("{host_path}::{}", host_variant.name)
        } else if host_variant.fields.iter().all(|field| field.name.is_some()) {
            let fields = host_variant
                .fields
                .iter()
                .zip(bindings.iter())
                .map(|(field, binding)| {
                    format!(
                        "{}: {binding}",
                        field.name.as_deref().unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>();
            format!(
                "{host_path}::{} {{ {} }}",
                host_variant.name,
                fields.join(", ")
            )
        } else if host_variant.fields.iter().all(|field| field.name.is_none()) {
            format!(
                "{host_path}::{}({})",
                host_variant.name,
                bindings.join(", ")
            )
        } else {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "native MIR enum variant `{}::{}` mixes named and positional fields",
                definition.name, host_variant.name
            )));
        };
        let mut values = Vec::with_capacity(source_variant.fields.len());
        for (source_index, source_field) in source_variant.fields.iter().enumerate() {
            let value = match aligned.iter().position(|aligned| *aligned == Some(source_index)) {
                Some(host_index) => convert_from_host_expression(
                    &source_field.ty,
                    &host_variant.fields[host_index].ty,
                    &bindings[host_index],
                    pairs,
                )?,
                None => derived_field_from_host(symbols, &definition.name, source_variant, host_variant, &bindings)?,
            };
            values.push(into_source_field(&source_field.ty, &value));
        }
        let constructor = source_variant_shape(symbols, &definition.name, source_variant, source_path, &values)?;
        arms.push(format!("        {pattern} => Ok({constructor}),"));
    }
    writeln!(
        out,
        "fn __jet_bootstrap_mir_{}_from_host(value: &{}) -> Result<{}, String> {{\n    match value {{\n{}\n        _ => Err(\"native MIR enum variant is not representable in Source MIR\".to_string()),\n    }}\n}}\n",
        definition.name,
        host_path,
        source,
        arms.join("\n"),
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

/// A Source enum variant as emitted Rust spells it: a unit variant is its
/// path, a payload variant a struct variant over the emitted payload field
/// symbols (`Path { field: part, .. }`), used both as pattern and constructor.
fn source_variant_shape(
    symbols: &BootstrapCodecSymbols<'_>,
    owner: &str,
    variant: &Variant,
    path: String,
    parts: &[String],
) -> Result<String, BootstrapHostCodecError> {
    if variant.fields.is_empty() {
        return Ok(path);
    }
    let fields = variant
        .fields
        .iter()
        .zip(parts)
        .map(|(field, part)| {
            let name = field.name.as_deref().ok_or_else(|| {
                BootstrapHostCodecError::InvalidMetadata(format!(
                    "Source MIR enum variant `{owner}::{}` has a positional payload field",
                    variant.name
                ))
            })?;
            Ok(format!("{}: {part}", symbols.field_symbol(owner, name)?))
        })
        .collect::<Result<Vec<_>, BootstrapHostCodecError>>()?;
    Ok(format!("{path} {{ {} }}", fields.join(", ")))
}

fn emit_id_converter(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
    host_by_name: &BTreeMap<String, Definition>,
    name: &str,
) -> Result<(), BootstrapHostCodecError> {
    let source = symbols.type_symbol(name)?;
    let value_field = symbols.field_symbol(name, "value")?;
    let host_path = host_type_path(name, host_by_name);
    writeln!(
        out,
        "fn __jet_bootstrap_mir_{name}_to_host(value: &{source}) -> Result<{host_path}, String> {{\n    let raw = value.{value_field};\n    if raw == 0 {{ return Err(\"MIR identity must be non-zero\".to_string()); }}\n    Ok({host_path}(raw))\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

/// Source MIR identities are single-`value` records named `…ID`; native MIR
/// carries them as `u64` newtypes. Enums that happen to end in `ID`
/// (`MIROptimizationPassID`, `MIRRuntimePartID`) convert structurally.
fn id_newtype(definition: &Definition) -> bool {
    definition.name.ends_with("ID")
        && definition.name != "MIRTypeID"
        && matches!(definition.body, Body::Struct(_))
}

fn custom_source_converter(name: &str) -> bool {
    matches!(
        name,
        "Span"
            | "MIRTypeID"
            | "MIRMeasure"
            | "MIRDimension"
            | "MIRTagMarker"
            | "MIRABI"
            | "MIRSize"
            | "MIRType"
            | "MIRViewProvenanceEntry"
            | "MIRJobLimit"
    )
}

/// Source binary overflow is derived from dispatch and has no standalone
/// native carrier. A converter may not name a nonexistent native type.
fn source_only_type(name: &str) -> bool {
    name == "MIRBinaryOverflow"
}

/// A Source definition with no native type of its own: absent from the parsed
/// `MIR.rs` schema and from the support carriers `host_type_path` maps into
/// other `jet_foundation` modules (a mapped carrier that is not declared is
/// reported by `insert_support_host_definitions`). Such records cross only
/// inline (pair records, hand converters), so a standalone converter would
/// name a type that does not exist; a use that does need one fails at its call
/// site instead.
fn native_type_absent(name: &str, host_by_name: &BTreeMap<String, Definition>) -> bool {
    !host_by_name.contains_key(name)
}

fn missing_host_definition(name: &str) -> BootstrapHostCodecError {
    BootstrapHostCodecError::InvalidMetadata(format!(
        "Source MIR `{name}` has no parsed native carrier"
    ))
}

/// A converted value as the Source field it initializes: `.into()` covers the
/// emitter's boxed recursive fields (`Box<MIRType>`, `Box<Vec<..>>`) through
/// `From<T>`. A map is never boxed, and `.into()` would leave the collected
/// map type uninferred, so a map value is used as is.
fn into_source_field(source_type: &str, value: &str) -> String {
    if matches!(parse_type(source_type), Ok(TypeExpr::Map(..))) {
        value.to_string()
    } else {
        format!("({value}).into()")
    }
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
    pairs: &PairRecords,
    definition: &Definition,
    source_fields: &[Field],
) -> Result<(), BootstrapHostCodecError> {
    let source = symbols.type_symbol(&definition.name)?;
    let host_path = host_type_path(&definition.name, host_by_name);
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
        None => return Err(missing_host_definition(&definition.name)),
    };
    let source_by_name = source_fields
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect::<BTreeMap<_, _>>();
    let mut used = BTreeMap::<String, String>::new();
    let mut initializers = Vec::<(String, String)>::new();

    // A struct with a crate-private native field cannot be spelled as a
    // literal (not even with a `..Default::default()` base): it starts from
    // the native default, gets each public field assigned and each private
    // field set through its accessor (`HOST_PRIVATE_FIELDS`).
    let mut private_default = false;
    let mut private_setters = String::new();
    for host_field in &host_fields {
        if let Some((_, _, _, setter)) = host_private_row(&definition.name, &host_field.name) {
            let source_name = source_field_name(&definition.name, &host_field.name);
            let source_symbol = symbols.field_symbol(&definition.name, &source_name)?;
            used.insert(source_name, host_field.name.clone());
            private_setters.push_str(&format!(
                "    __host.{setter}(value.{source_symbol}.as_ref().ok().map(|__value| __value.as_str()))?;\n"
            ));
            private_default = true;
            continue;
        }
        if let Some(expression) =
            custom_struct_field_to_host(symbols, &definition.name, &host_field.name)?
        {
            used.insert(source_field_name(&definition.name, &host_field.name), host_field.name.clone());
            initializers.push((host_field.name.clone(), expression));
            continue;
        }
        let source_name = source_field_name(&definition.name, &host_field.name);
        let source_field = source_by_name.get(source_name.as_str()).copied();
        let expression = if let Some(source_field) = source_field {
            used.insert(source_field.name.clone(), host_field.name.clone());
            convert_expression(
                &source_field.ty,
                &host_field.ty,
                &format!("value.{}", symbols.field_symbol(&definition.name, &source_field.name)?),
                pairs,
            )?
        } else {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "native MIR field `{}.{}` has no lossless Source-MIR field",
                definition.name, host_field.name
            )));
        };
        initializers.push((host_field.name.clone(), expression));
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

    let body = if private_default {
        let assignments = initializers
            .iter()
            .map(|(field, expression)| format!("    __host.{field} = {expression};\n"))
            .collect::<String>();
        format!("    let mut __host: {host_path} = ::std::default::Default::default();\n{assignments}{private_setters}    Ok(__host)")
    } else {
        let fields = initializers
            .iter()
            .map(|(field, expression)| format!("        {field}: {expression},"))
            .collect::<Vec<_>>();
        format!("    Ok({host_path} {{\n{}\n    }})", fields.join("\n"))
    };
    writeln!(
        out,
        "fn __jet_bootstrap_mir_{}_to_host(value: &{}) -> Result<{}, String> {{\n{body}\n}}\n",
        definition.name, source, host_path,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}


fn emit_enum_converter(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
    host_by_name: &BTreeMap<String, Definition>,
    pairs: &PairRecords,
    definition: &Definition,
    source_variants: &[Variant],
) -> Result<(), BootstrapHostCodecError> {
    let source = symbols.type_symbol(&definition.name)?;
    let host_path = host_type_path(&definition.name, host_by_name);
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
        None => return Err(missing_host_definition(&definition.name)),
    };
    let host_by_variant = host_variants
        .iter()
        .map(|variant| (::jet_foundation::Syntax::respell_acronym_name(&variant.name), variant))
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
        let aligned = align_variant_fields(&definition.name, source_variant, host_variant)?;
        let bindings = (0..source_variant.fields.len())
            .map(|index| format!("__field_{index}"))
            .collect::<Vec<_>>();
        let pattern = source_variant_shape(symbols, &definition.name, source_variant, source_path.clone(), &bindings)?;
        let values = host_variant
            .fields
            .iter()
            .zip(aligned.iter())
            .filter_map(|(host_field, source_index)| source_index.map(|index| (host_field, index)))
            .map(|(host_field, index)| {
                convert_expression(&source_variant.fields[index].ty, &host_field.ty, &bindings[index], pairs)
            })
            .collect::<Result<Vec<_>, _>>()?;
        // A derived Source field is checked against the converted native
        // fields, so those are bound once and the check reads the bindings.
        let derived = (0..source_variant.fields.len())
            .filter(|index| !aligned.contains(&Some(*index)))
            .collect::<Vec<_>>();
        let (lets, host_values) = if derived.is_empty() {
            (Vec::new(), values)
        } else {
            let lets = values
                .iter()
                .enumerate()
                .map(|(index, value)| format!("let __host_{index} = {value};"))
                .collect::<Vec<_>>();
            (lets, (0..values.len()).map(|index| format!("__host_{index}")).collect())
        };
        let constructor = if host_variant.fields.is_empty() {
            format!("{}::{}", host_path, host_variant.name)
        } else if host_variant.fields.iter().all(|field| field.name.is_some()) {
            let fields = host_variant
                .fields
                .iter()
                .zip(host_values.iter())
                .map(|(field, expression)| {
                    format!("{}: {}", field.name.as_deref().unwrap_or_default(), expression)
                })
                .collect::<Vec<_>>();
            format!("{}::{} {{ {} }}", host_path, host_variant.name, fields.join(", "))
        } else {
            format!("{}::{}({})", host_path, host_variant.name, host_values.join(", "))
        };
        if derived.is_empty() {
            arms.push(format!("        {} => Ok({}),", pattern, constructor));
        } else {
            let checks = derived
                .iter()
                .map(|index| derived_field_to_host_check(symbols, &definition.name, source_variant, host_variant, *index, &bindings))
                .collect::<Result<Vec<_>, _>>()?;
            arms.push(format!("        {} => {{ {} {} Ok({}) }}", pattern, lets.join(" "), checks.join(" "), constructor));
        }
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
    pairs: &PairRecords,
) -> Result<String, BootstrapHostCodecError> {
    let source_type = parse_type(source_type).map_err(BootstrapHostCodecError::InvalidMetadata)?;
    let host_type =
        parse_type(&expand_host_aliases(host_type)).map_err(BootstrapHostCodecError::InvalidMetadata)?;
    convert_type_expression(&source_type, &host_type, expression, pairs)
}

fn convert_from_host_expression(
    source_type: &str,
    host_type: &str,
    expression: &str,
    pairs: &PairRecords,
) -> Result<String, BootstrapHostCodecError> {
    let source_type = parse_type(source_type).map_err(BootstrapHostCodecError::InvalidMetadata)?;
    let host_type =
        parse_type(&expand_host_aliases(host_type)).map_err(BootstrapHostCodecError::InvalidMetadata)?;
    convert_from_host_type_expression(&source_type, &host_type, expression, pairs)
}

/// `ty` with each native type alias (`EffectSet` = `Holds` =
/// `BTreeSet<String>`) spelled as the type it names, so the carrier shape
/// (set, map, list) drives the conversion. Aliases are the single-line,
/// non-generic `pub type` items of [`HOST_SUPPORT_SCHEMAS`]; a name aliased
/// differently in two modules is left as written.
fn expand_host_aliases(ty: &str) -> String {
    static ALIASES: ::std::sync::OnceLock<BTreeMap<String, Option<String>>> =
        ::std::sync::OnceLock::new();
    let aliases = ALIASES.get_or_init(|| {
        let mut aliases = BTreeMap::<String, Option<String>>::new();
        for (_, schema) in HOST_SUPPORT_SCHEMAS {
            for line in schema.lines() {
                let Some((name, target)) = line
                    .trim()
                    .strip_prefix("pub type ")
                    .and_then(|rest| rest.strip_suffix(';'))
                    .and_then(|rest| rest.split_once('='))
                else {
                    continue;
                };
                let (name, target) = (name.trim(), target.trim().to_string());
                if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    continue;
                }
                aliases
                    .entry(name.to_string())
                    .and_modify(|known| {
                        if known.as_deref() != Some(target.as_str()) {
                            *known = None;
                        }
                    })
                    .or_insert(Some(target));
            }
        }
        aliases
    });
    let mut expanded = ty.to_string();
    // Aliases name aliases (`EffectSet` -> `Holds`); a cycle stops at the bound.
    for _ in 0..8 {
        let mut out = String::with_capacity(expanded.len());
        let mut changed = false;
        let mut rest = expanded.as_str();
        while let Some(start) = rest.find(|c: char| c.is_ascii_alphabetic() || c == '_') {
            out.push_str(&rest[..start]);
            let path = &rest[start..];
            let end = path
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
                .unwrap_or(path.len());
            let segment = path[..end].rsplit("::").next().unwrap_or_default();
            match aliases.get(segment) {
                Some(Some(target)) => {
                    out.push_str(target);
                    changed = true;
                }
                _ => out.push_str(&path[..end]),
            }
            rest = &path[end..];
        }
        out.push_str(rest);
        expanded = out;
        if !changed {
            break;
        }
    }
    expanded
}

/// The Source pair record an element type names, if it is one.
fn pair_record<'p>(element: &TypeExpr, pairs: &'p PairRecords) -> Option<&'p PairRecord> {
    match element {
        TypeExpr::Named(name) => pairs.get(name),
        _ => None,
    }
}

fn convert_from_host_type_expression(
    source_type: &TypeExpr,
    host_type: &TypeExpr,
    expression: &str,
    pairs: &PairRecords,
) -> Result<String, BootstrapHostCodecError> {
    match source_type {
        TypeExpr::Optional(inner) => {
            let host_inner = match host_type {
                TypeExpr::Optional(host_inner) => host_inner.as_ref(),
                _ => host_type,
            };
            let inner = convert_from_host_type_expression(inner, host_inner, "__inner", pairs)?;
            Ok(format!(
                "match ({expression}).as_ref() {{ Some(__inner) => Ok({inner}), None => Err(jet_foundation::Outcome::JetAbsent), }}"
            ))
        }
        TypeExpr::List(inner) => {
            if let TypeExpr::Map(host_key, host_value) = host_type {
                if let TypeExpr::Named(name) = inner.as_ref() {
                    let helper = match name.as_str() {
                        "MIRViewProvenanceEntry" => {
                            "__jet_bootstrap_mir_view_provenance_from_host"
                        }
                        "MIRJobLimit" => "__jet_bootstrap_mir_job_limits_from_host",
                        _ => "",
                    };
                    if !helper.is_empty() {
                        return Ok(format!("{helper}({expression})?"));
                    }
                }
                if let Some(pair) = pair_record(inner, pairs) {
                    pair.entry()?;
                    let entry = source_record_from_host(
                        pair,
                        &[host_key.as_ref().clone(), host_value.as_ref().clone()],
                        &["__key", "__value"],
                        pairs,
                    )?;
                    return Ok(format!(
                        "({expression}).iter().map(|(__key, __value)| Ok({entry})).collect::<Result<Vec<_>, String>>()?"
                    ));
                }
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "native MIR map has no checked Source-MIR list converter".to_string(),
                ));
            }
            let host_inner = match host_type {
                TypeExpr::List(host_inner) => host_inner.as_ref(),
                TypeExpr::Named(name) => {
                    if let Some(inner) =
                        generic_inner(name, "BTreeSet").or_else(|| generic_inner(name, "HashSet"))
                    {
                        return convert_host_set_to_source(inner, source_type, expression, pairs);
                    }
                    source_type
                }
                _ => source_type,
            };
            let inner = convert_from_host_type_expression(inner, host_inner, "__item", pairs)?;
            Ok(format!(
                "({expression}).iter().map(|__item| Ok({inner})).collect::<Result<Vec<_>, String>>()?"
            ))
        }
        TypeExpr::Map(key, value) => {
            let (host_key, host_value) = match host_type {
                TypeExpr::Map(host_key, host_value) => (host_key.as_ref(), host_value.as_ref()),
                _ => (key.as_ref(), value.as_ref()),
            };
            let key = convert_from_host_type_expression(key, host_key, "__key", pairs)?;
            let value = convert_from_host_type_expression(value, host_value, "__value", pairs)?;
            Ok(format!(
                "__jet_bootstrap_mir_collect_unique(({expression}).iter().map(|(__key, __value)| Ok(({key}, {value}))).collect::<Result<Vec<_>, String>>()?, \"duplicate native MIR map key\")?"
            ))
        }
        TypeExpr::Named(name) => {
            if let Some(pair) = pair_record(source_type, pairs) {
                if let Some(host_types) = host_tuple_types(host_type, pair)? {
                    let parts = (0..host_types.len())
                        .map(|index| format!("&__pair.{index}"))
                        .collect::<Vec<_>>();
                    let parts = parts.iter().map(String::as_str).collect::<Vec<_>>();
                    let record = source_record_from_host(pair, &host_types, &parts, pairs)?;
                    return Ok(format!("{{ let __pair = {expression}; {record} }}"));
                }
            }
            convert_named_from_host(name, host_type, expression, pairs)
        }
    }
}

/// The Source record built from a native tuple or map entry whose elements
/// `parts` name, each converted against its host element type.
fn source_record_from_host(
    record: &PairRecord,
    host_types: &[TypeExpr],
    parts: &[&str],
    pairs: &PairRecords,
) -> Result<String, BootstrapHostCodecError> {
    let fields = record
        .fields
        .iter()
        .zip(host_types)
        .zip(parts)
        .map(|(((symbol, ty), host_type), part)| {
            let source_type = parse_type(ty).map_err(BootstrapHostCodecError::InvalidMetadata)?;
            let value = convert_from_host_type_expression(&source_type, host_type, part, pairs)?;
            Ok(format!("{symbol}: {}", into_source_field(ty, &value)))
        })
        .collect::<Result<Vec<_>, BootstrapHostCodecError>>()?;
    Ok(format!("{} {{ {} }}", record.symbol, fields.join(", ")))
}

/// The native tuple built from the fields of the Source record `base`, each
/// converted against its host element type.
fn host_tuple_from_source(
    record: &PairRecord,
    host_types: &[TypeExpr],
    base: &str,
    pairs: &PairRecords,
) -> Result<String, BootstrapHostCodecError> {
    let elements = record
        .fields
        .iter()
        .zip(host_types)
        .map(|((symbol, ty), host_type)| {
            let source_type = parse_type(ty).map_err(BootstrapHostCodecError::InvalidMetadata)?;
            convert_type_expression(&source_type, host_type, &format!("({base}).{symbol}"), pairs)
        })
        .collect::<Result<Vec<_>, BootstrapHostCodecError>>()?;
    Ok(format!("({})", elements.join(", ")))
}

fn convert_host_set_to_source(
    host_element: &str,
    source_type: &TypeExpr,
    expression: &str,
    pairs: &PairRecords,
) -> Result<String, BootstrapHostCodecError> {
    let source_element = match source_type {
        TypeExpr::List(inner) => inner.as_ref(),
        _ => {
            return Err(BootstrapHostCodecError::InvalidMetadata(
                "native MIR set has no Source-MIR list element type".to_string(),
            ))
        }
    };
    let host_element =
        parse_type(host_element).map_err(BootstrapHostCodecError::InvalidMetadata)?;
    let converted =
        convert_from_host_type_expression(source_element, &host_element, "__item", pairs)?;
    Ok(format!(
        "({expression}).iter().map(|__item| Ok({converted})).collect::<Result<Vec<_>, String>>()?"
    ))
}

fn convert_named_from_host(
    source_name: &str,
    host_type: &TypeExpr,
    expression: &str,
    pairs: &PairRecords,
) -> Result<String, BootstrapHostCodecError> {
    if let TypeExpr::Named(name) = host_type {
        if let Some(inner) = generic_inner(name, "Box") {
            let host_inner =
                parse_type(inner).map_err(BootstrapHostCodecError::InvalidMetadata)?;
            let source = TypeExpr::Named(source_name.to_string());
            return convert_from_host_type_expression(
                &source,
                &host_inner,
                &format!("&**({expression})"),
                pairs,
            );
        }
    }
    if source_name == "MIRTraitRef" && host_nominal_ref(host_type) {
        return Ok(format!("__jet_bootstrap_mir_trait_bound_from_host({expression})?"));
    }
    if source_name == "String"
        && matches!(
            host_type,
            TypeExpr::Named(name) if name == "std::path::PathBuf" || name.ends_with("::PathBuf")
        )
    {
        return Ok(format!(
            "({expression}).to_str().ok_or_else(|| \"native MIR path is not valid UTF-8\".to_string())?.to_string()"
        ));
    }
    if source_name == "Int" {
        return Ok(format!(
            "jet_foundation::Numeric::JetInt::from_big(jet_foundation::Numeric::CtBigInt::from_str(&({expression}).to_string())?)"
        ));
    }
    if source_name == "String" && host_static_str(host_type) {
        return Ok(format!("({expression}).to_string()"));
    }
    if source_name == "String" {
        return Ok(format!("({expression}).clone()"));
    }
    if source_name == "Bool"
        || source_name == "Float"
        || source_name == "Float32"
        || source_name == "Char"
        || source_name.starts_with('U')
        || source_name.starts_with('I')
    {
        return Ok(format!("*({expression})"));
    }
    if source_name == "Span" {
        return Ok(format!("__jet_bootstrap_span_from_host({expression})?"));
    }
    match source_name {
        "MIRTypeID" => {
            return Ok(format!(
                "__jet_bootstrap_type_id_from_host(*({expression}))?"
            ))
        }
        "MIRMeasure" => {
            return Ok(format!(
                "__jet_bootstrap_measure_from_host({expression})?"
            ))
        }
        "MIRDimension" => {
            return Ok(format!(
                "__jet_bootstrap_dimension_from_host({expression})?"
            ))
        }
        "MIRTagMarker" => {
            return Ok(format!("__jet_bootstrap_tag_from_host({expression})?"))
        }
        "MIRABI" => {
            return Ok(format!("__jet_bootstrap_abi_from_host({expression})"))
        }
        "MIRSize" => {
            return Ok(format!("__jet_bootstrap_size_from_host({expression})?"))
        }
        "MIRType" => {
            return Ok(format!("__jet_bootstrap_type_from_host({expression})?"))
        }
        _ => {}
    }
    Ok(format!(
        "__jet_bootstrap_mir_{source_name}_from_host({expression})?"
    ))
}


fn convert_type_expression(
    source_type: &TypeExpr,
    host_type: &TypeExpr,
    expression: &str,
    pairs: &PairRecords,
) -> Result<String, BootstrapHostCodecError> {
    match source_type {
        TypeExpr::Optional(inner) => {
            let host_inner = match host_type {
                TypeExpr::Optional(host_inner) => host_inner.as_ref(),
                _ => host_type,
            };
            let inner = convert_type_expression(inner, host_inner, "__inner", pairs)?;
            Ok(format!(
                "::std::result::Result::as_ref(&({expression})).ok().map(|__inner| -> Result<_, String> {{ Ok({inner}) }}).transpose()?"
            ))
        }
        TypeExpr::List(inner) => {
            if let TypeExpr::Map(host_key, host_value) = host_type {
                if let TypeExpr::Named(name) = inner.as_ref() {
                    let helper = match name.as_str() {
                        "MIRViewProvenanceEntry" => {
                            "__jet_bootstrap_mir_view_provenance_to_host"
                        }
                        "MIRJobLimit" => "__jet_bootstrap_mir_job_limits_to_host",
                        _ => "",
                    };
                    if !helper.is_empty() {
                        return Ok(format!("{helper}(({expression}).as_ref())?"));
                    }
                }
                if let Some(pair) = pair_record(inner, pairs) {
                    pair.entry()?;
                    let entry = host_tuple_from_source(
                        pair,
                        &[host_key.as_ref().clone(), host_value.as_ref().clone()],
                        "__pair",
                        pairs,
                    )?;
                    return Ok(format!(
                        "__jet_bootstrap_mir_collect_unique(({expression}).iter().map(|__pair| Ok({entry})).collect::<Result<Vec<_>, String>>()?, \"duplicate Source MIR map key\")?"
                    ));
                }
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "Source MIR list has no checked native map converter".to_string(),
                ));
            }
            // A native set collects the converted elements and refuses a
            // duplicate, which the Source list could carry.
            let set = match host_type {
                TypeExpr::Named(name) => generic_inner(name, "BTreeSet")
                    .map(|element| ("BTreeSet", element))
                    .or_else(|| generic_inner(name, "HashSet").map(|element| ("HashSet", element))),
                _ => None,
            };
            let set_element = set
                .map(|(_, element)| parse_type(element))
                .transpose()
                .map_err(BootstrapHostCodecError::InvalidMetadata)?;
            let host_inner = match (host_type, &set_element) {
                (_, Some(element)) => element,
                (TypeExpr::List(host_inner), None) => host_inner.as_ref(),
                _ => source_type,
            };
            let inner = convert_type_expression(inner, host_inner, "__item", pairs)?;
            Ok(match set {
                Some((carrier, _)) => format!(
                    "{{ let __items = ({expression}).iter().map(|__item| Ok({inner})).collect::<Result<Vec<_>, String>>()?; let __count = __items.len(); let __output = __items.into_iter().collect::<::std::collections::{carrier}<_>>(); if __output.len() != __count {{ return Err(\"duplicate MIR set element\".to_string()); }} __output }}"
                ),
                None => format!(
                    "({expression}).iter().map(|__item| Ok({inner})).collect::<Result<Vec<_>, String>>()?"
                ),
            })
        }
        TypeExpr::Map(key, value) => {
            let (host_key, host_value) = match host_type {
                TypeExpr::Map(host_key, host_value) => (host_key.as_ref(), host_value.as_ref()),
                _ => (key.as_ref(), value.as_ref()),
            };
            let key = convert_type_expression(key, host_key, "__key", pairs)?;
            let value = convert_type_expression(value, host_value, "__value", pairs)?;
            Ok(format!(
                "__jet_bootstrap_mir_collect_unique(({expression}).iter().map(|(__key, __value)| Ok(({key}, {value}))).collect::<Result<Vec<_>, String>>()?, \"duplicate Source MIR map key\")?"
            ))
        }
        TypeExpr::Named(name) => {
            if let Some(pair) = pair_record(source_type, pairs) {
                if let Some(host_types) = host_tuple_types(host_type, pair)? {
                    return host_tuple_from_source(pair, &host_types, expression, pairs);
                }
            }
            convert_named_expression(name, host_type, expression)
        }
    }
}

fn convert_named_expression(
    source_name: &str,
    host_type: &TypeExpr,
    expression: &str,
) -> Result<String, BootstrapHostCodecError> {
    if source_name == "MIRTraitRef" && host_nominal_ref(host_type) {
        return Ok(format!("__jet_bootstrap_mir_trait_bound_to_host(&({expression}))?"));
    }
    if source_name == "String" && host_static_str(host_type) {
        return Ok(format!("(::std::boxed::Box::leak(({expression}).clone().into_boxed_str()) as &'static str)"));
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
    let mut converted = match existing_helper(source_name) {
        Some((helper, true)) => format!("{helper}(&({expression}))?"),
        Some((helper, false)) => format!("{helper}(&({expression}))"),
        None => format!("__jet_bootstrap_mir_{}_to_host(&({expression}))?", source_name),
    };
    let target = match host_type {
        TypeExpr::Optional(inner) => inner.as_ref(),
        _ => host_type,
    };
    if let TypeExpr::Named(name) = target {
        if name.starts_with("Box<") {
            converted = format!("Box::new({converted})");
        }
    }
    Ok(converted)
}

/// The Native.rs helper converting a Source carrier to native MIR, and
/// whether it is fallible (`tag`/`abi` map every Source value).
fn existing_helper(name: &str) -> Option<(&'static str, bool)> {
    match name {
        "MIRTypeID" => Some(("__jet_bootstrap_type_id_to_host", true)),
        "MIRMeasure" => Some(("__jet_bootstrap_measure_to_host", true)),
        "MIRDimension" => Some(("__jet_bootstrap_dimension_to_host", true)),
        "MIRTagMarker" => Some(("__jet_bootstrap_tag_to_host", false)),
        "MIRABI" => Some(("__jet_bootstrap_abi_to_host", false)),
        "MIRSize" => Some(("__jet_bootstrap_size_to_host", true)),
        "MIRType" => Some(("__jet_bootstrap_type_to_host", true)),
        _ => None,
    }
}

/// A native `&'static str` (registry rows such as `CoreCallSymbol`). The
/// Source string crosses by leaking one copy: a program converts once per
/// bootstrap process, and the native rows hold registry-lifetime text.
fn host_static_str(host_type: &TypeExpr) -> bool {
    matches!(host_type, TypeExpr::Named(name) if name.starts_with('&') && name.ends_with("str"))
}

/// Native trait-object bounds are `MirNominalRef`s whose type id carries the
/// trait's stable id (`stable_id("mir-trait", name)`, as Source MIR's
/// `MIRTraitRef` does), so a Source trait ref crosses with its raw id intact.
fn host_nominal_ref(host_type: &TypeExpr) -> bool {
    matches!(host_type, TypeExpr::Named(name) if name == "MirNominalRef")
}

/// Rust path of the native carrier for a Source MIR declaration. Jet spells
/// acronyms in capitals (D-ACRO-LEX1, `MIRTargetDMAOwner`) while the native
/// carriers keep their Rust names (`TargetDmaOwner`), so a carrier outside the
/// parsed `MIR.rs` schema names its Rust type explicitly here.
fn host_type_path(name: &str, host_by_name: &BTreeMap<String, Definition>) -> String {
    match name {
        "MIRSchemaMigrationPlan" | "MIRSchemaMigrationStep" | "MIRSchemaMigrationOp" => {
            format!(
                "::jet_foundation::SchemaMigration::{}",
                name.trim_start_matches("MIR")
            )
        }
        "Span" => "::jet_foundation::Diagnostics::Span".to_string(),
        "ParamZone" => "::jet_foundation::MIR::MirParamZone".to_string(),
        "MIRShapeFieldNames" => "::jet_foundation::Shape::ShapeFieldNames".to_string(),
        "MIROperatorMarker" => "::jet_foundation::AST::OperatorMarker".to_string(),
        "CoreMarkerApplication" => "::jet_foundation::Syntax::CoreMarkerApplication".to_string(),
        "Effect" => "::jet_foundation::Effects::Effect".to_string(),
        "SinkClass" => "::jet_foundation::Syntax::SinkClass".to_string(),
        "LayoutFacts" | "ByteLayout" | "FieldLayoutFacts" => {
            format!("::jet_foundation::Layout::{name}")
        }
        "MIRDerivationRef" | "MIRDerivationMethod" | "MIRDerivationDisposition" => {
            format!(
                "::jet_foundation::Facts::{}",
                name.trim_start_matches("MIR")
            )
        }
        "MIRFFIEvidenceBasis" => "::jet_foundation::AST::FfiEvidenceBasis".to_string(),
        "MIRFFIBoundaryFacts" => "::jet_foundation::AST::FfiBoundaryFacts".to_string(),
        "MIRFFIBoundaryObligation" => "::jet_foundation::AST::FfiBoundaryObligation".to_string(),
        "MIRTypedHeadKind" => "::jet_foundation::Syntax::TypedHeadKind".to_string(),
        "MIRWebBucket" => "::jet_foundation::WebPartition::WebBucket".to_string(),
        "MIRWebPartitionMarker" => {
            "::jet_foundation::WebPartition::WebPartitionMarker".to_string()
        }
        "MIRFFICloseSource" => "::jet_foundation::AST::FfiCloseSource".to_string(),
        "MIRFFIThreadSafety" => "::jet_foundation::AST::FfiThreadSafety".to_string(),
        "MIRResourceAccessMode" => {
            "::jet_foundation::ResourceSchedule::JetResourceAccessMode".to_string()
        }
        "MIRResourceIdentity" => {
            "::jet_foundation::ResourceSchedule::JetResourceIdentity".to_string()
        }
        "MIRResourceRegion" => {
            "::jet_foundation::ResourceSchedule::JetResourceRegion".to_string()
        }
        "MIRResourceLayout" => {
            "::jet_foundation::ResourceSchedule::JetResourceLayout".to_string()
        }
        "MIRResourceAccess" => {
            "::jet_foundation::ResourceSchedule::JetResourceAccess".to_string()
        }
        "MIRFrameCompletion" => {
            "::jet_foundation::ResourceSchedule::JetFrameCompletion".to_string()
        }
        "MIRFrameOperation" => {
            "::jet_foundation::ResourceSchedule::JetFrameOperation".to_string()
        }
        "MIRFrameDependency" => {
            "::jet_foundation::ResourceSchedule::JetFrameDependency".to_string()
        }
        "MIRFrameTransfer" => {
            "::jet_foundation::ResourceSchedule::JetFrameTransfer".to_string()
        }
        "MIRFrameReuse" => {
            "::jet_foundation::ResourceSchedule::JetFrameReuse".to_string()
        }
        "MIRFrameRetention" => {
            "::jet_foundation::ResourceSchedule::JetFrameRetention".to_string()
        }
        "MIRFrameSchedule" => {
            "::jet_foundation::ResourceSchedule::JetFrameSchedule".to_string()
        }
        "MIRHostImportFact" => "::jet_foundation::Authority::HostImportFact".to_string(),
        "MIRNameFacts" => "::jet_foundation::MIR::MirNameFacts".to_string(),
        "MIRStructureFact" | "MIRStructureFactKind" => {
            format!(
                "::jet_foundation::Names::{}",
                name.trim_start_matches("MIR")
            )
        }
        "MIRFixedReductionTree"
        | "MIRFixedReductionOrder"
        | "MIRAccelerationTransform"
        | "MIRAccelerationProof"
        | "MIRAccelerationWorkloadFacts" => {
            format!(
                "::jet_foundation::MIROptimization::Acceleration::{}",
                name.trim_start_matches("MIR")
            )
        }
        "CoreCallFallibility"
        | "CoreCallPureRoute"
        | "CoreCallInterpreterRoute"
        | "CoreCallSymbol" => {
            format!("::jet_foundation::Syntax::{name}")
        }
        "MIRLayoutSupportFacts" | "MIRLayoutCapabilityFacts" | "MIRLayoutAlignmentFact" => {
            format!(
                "::jet_foundation::Layout::{}",
                name.trim_start_matches("MIR")
            )
        }
        "MIROSTarget" => "::jet_foundation::OSTarget::OSTarget".to_string(),
        "MIRRuntimeLayer" => "::jet_foundation::RingLayer::RuntimeLayer".to_string(),
        "MIRByteSize"
        | "MIRMemoryAccess"
        | "MIRMemoryRegion"
        | "MIRLinkerInput"
        | "MIRProviderLimits"
        | "MIRProviderContract"
        | "MIRAllocatorPolicy"
        | "MIRPanicPolicy"
        | "MIRClockPolicy"
        | "MIREntropyPolicy"
        | "MIRSchedulerPolicy"
        | "MIRByteSinkPolicy"
        | "MIRStartupPolicy"
        | "MIRAuditPolicy"
        | "MIRRegisterWidth"
        | "MIRTargetRegisterAccessMode"
        | "MIRTargetRegisterFact"
        | "MIRTargetRegisterBlockFact"
        | "MIRTargetRegisterOperation"
        | "MIRTargetRegisterAccessFact"
        | "MIRTargetInterruptFact"
        | "MIRTargetInterruptHandlerFact"
        | "MIRTargetHardwareFacts"
        | "MIRTargetHardwareUse"
        | "MIRTargetHardwareUnresolvedReference"
        | "MIRTargetProgrammerAdapter"
        | "MIRTargetProgrammerFacts"
        | "MIRTargetMachine"
        | "MIRMemoryKind" => {
            format!("::jet_foundation::TargetMachine::{}", name.trim_start_matches("MIR"))
        }
        "MIRMMIOPolicy" => "::jet_foundation::TargetMachine::MmioPolicy".to_string(),
        "MIRSVDProvenance" => "::jet_foundation::TargetMachine::SvdProvenance".to_string(),
        "MIRTargetDossier" => "::jet_foundation::Facts::TargetDossier".to_string(),
        "MIRComponentSignatureDescriptor" | "MIRComponentTypeDescriptor" => {
            format!("::jet_foundation::MIR::{}", name.trim_start_matches("MIR"))
        }
        "MIRProviderABI" => "::jet_foundation::TargetMachine::ProviderAbi".to_string(),
        "MIRTargetDMAOwnership" => "::jet_foundation::TargetMachine::TargetDmaOwnership".to_string(),
        "MIRTargetDMAChannelFact" => {
            "::jet_foundation::TargetMachine::TargetDmaChannelFact".to_string()
        }
        "MIRTargetDMAOperation" => "::jet_foundation::TargetMachine::TargetDmaOperation".to_string(),
        "MIRTargetDMAOwner" => "::jet_foundation::TargetMachine::TargetDmaOwner".to_string(),
        "MIRTargetDMAOperationFact" => {
            "::jet_foundation::TargetMachine::TargetDmaOperationFact".to_string()
        }
        "MIRTargetApplicability" => "::jet_foundation::MIR::MirTargetApplicability".to_string(),
        "MIRAppPendingBoundaryID" => "::jet_foundation::App::AppPendingBoundaryId".to_string(),
        // `MIRNamedSpan` is the Source spelling of one native span-map entry,
        // not a `Names` carrier.
        name if name.starts_with("MIRName") && name != "MIRNamedSpan" => format!(
            "::jet_foundation::Names::{}",
            name.trim_start_matches("MIR")
        ),
        name if name.starts_with("MIRApp") => format!(
            "::jet_foundation::App::{}",
            name.trim_start_matches("MIR")
        ),
        "MIRDataPlanOperation" | "MIRDataPlanPhysicalOperator" => {
            format!(
                "::jet_foundation::AST::DataPlan{}Kind",
                name.trim_start_matches("MIRDataPlan")
            )
        }
        "MIRDataPlanSourceKind" | "MIRDataPlanStreamMode" => {
            format!("::jet_foundation::AST::{}", name.trim_start_matches("MIR"))
        }
        _ => format!(
            "::jet_foundation::MIR::{}",
            host_by_name.get(name).map_or(name, |definition| definition.name.as_str())
        ),
    }
}

fn source_field_name(owner: &str, host_field: &str) -> String {
    match (owner, host_field) {
        ("MIRImport", "module") => "module_id".to_string(),
        ("MIRTypeDef", "module") => "module_id".to_string(),
        ("MIRFunction", "module") => "module_name".to_string(),
        ("MIRPreludeCall", "module") => "module_name".to_string(),
        ("MIRCoreCall", "module") => "module_name".to_string(),
        ("MIRCoreCall", "effect") => "effect_kind".to_string(),
        ("MIRPreludeCall", "effect") => "effect_kind".to_string(),
        ("MIRForeign", "module") => "module_name".to_string(),
        ("MIRCLib", "module") => "module_name".to_string(),
        ("MIRTraitDef", "module")
        | ("MIRImplDef", "module")
        | ("MIRConstantDef", "module") => "module_id".to_string(),
        ("MIRNameDeclaration", "module") | ("MIRNameAlias", "module") => {
            "module_index".to_string()
        }
        // `comptime` is a Jet keyword, so the Source field carries the prefix.
        ("MIRLocal", "comptime") => "is_comptime".to_string(),
        _ => host_field.to_string(),
    }
}

/// A Source field whose native value another field's conversion carries.
fn source_field_is_host_derived(owner: &str, field: &str) -> bool {
    matches!((owner, field), ("MIRFunction", "memo_unbounded"))
}

/// Native memo bound is represented losslessly by Source bound + unbounded.
fn custom_struct_field_to_host(
    symbols: &BootstrapCodecSymbols<'_>,
    owner: &str,
    host_field: &str,
) -> Result<Option<String>, BootstrapHostCodecError> {
    Ok(match (owner, host_field) {
        ("MIRFunction", "memo_bound") => {
            let bound = symbols.field_symbol(owner, "memo_bound")?;
            let unbounded = symbols.field_symbol(owner, "memo_unbounded")?;
            Some(format!(
                "if value.{unbounded} {{ Some(None) }} else {{ value.{bound}.as_ref().ok().map(|__bound| -> Result<_, String> {{ Ok(Some(__bound.to_string_rep().parse::<usize>().map_err(|_| \"MIR memo bound is outside native range\".to_string())?)) }}).transpose()? }}"
            ))
        }
        _ => None,
    })
}

/// The Source side of [`custom_struct_field_to_host`]'s memo split and of
/// [`HOST_PRIVATE_FIELDS`]' crate-private native fields.
fn custom_struct_field_from_host(owner: &str, source_field: &str) -> Option<String> {
    if let Some((_, _, getter, _)) = host_private_row(owner, source_field) {
        return Some(format!(
            "value.{getter}().ok_or(jet_foundation::Outcome::JetAbsent)"
        ));
    }
    match (owner, source_field) {
        ("MIRFunction", "memo_bound") => Some(
            "match &value.memo_bound { Some(Some(__bound)) => Ok(jet_foundation::Numeric::JetInt::from_big(jet_foundation::Numeric::CtBigInt::from_str(&__bound.to_string())?)), _ => Err(jet_foundation::Outcome::JetAbsent) }".to_string(),
        ),
        ("MIRFunction", "memo_unbounded") => Some("matches!(value.memo_bound, Some(None))".to_string()),
        _ => None,
    }
}

/// Crate-private native fields glue outside `jet_foundation` can neither read
/// nor write directly, each crossing through a public accessor pair:
/// `(owner, field, getter -> Option<String>, setter(Option<&str>) -> Result)`.
/// The native image codec persists these fields, so a restored compiler image
/// only survives the Source MIR round trip when they cross both ways.
/// `MirOptimizationFacts.derived_from_digest` is the optimizer pipeline seal;
/// Source MIR carries it as the lowercase hex `mir_program_digest` spells.
/// Carrying it (rather than re-digesting the program) keeps image restore
/// linear in the program and free of whole-program digests.
const HOST_PRIVATE_FIELDS: &[(&str, &str, &str, &str)] = &[(
    "MIROptimizationFacts",
    "derived_from_digest",
    "pipeline_seal_hex",
    "set_pipeline_seal_hex",
)];

fn host_private_row(owner: &str, host_field: &str) -> Option<&'static (&'static str, &'static str, &'static str, &'static str)> {
    HOST_PRIVATE_FIELDS
        .iter()
        .find(|(row_owner, row_field, _, _)| *row_owner == owner && *row_field == host_field)
}

fn host_private_field(owner: &str, host_field: &str) -> bool {
    host_private_row(owner, host_field).is_some()
}

/// For each native payload field of a variant, the Source payload field it
/// converts from. Same-arity variants pair by position; otherwise both sides
/// pair by field name and every unpaired field must be a known one-sided field.
fn align_variant_fields(
    owner: &str,
    source: &Variant,
    host: &Variant,
) -> Result<Vec<Option<usize>>, BootstrapHostCodecError> {
    if source.fields.len() == host.fields.len() {
        return Ok((0..host.fields.len()).map(Some).collect());
    }
    let arity = || {
        BootstrapHostCodecError::InvalidMetadata(format!(
            "MIR enum variant `{owner}::{}` changes payload arity",
            source.name
        ))
    };
    let mut aligned = Vec::with_capacity(host.fields.len());
    for host_field in &host.fields {
        let name = host_field.name.as_deref().ok_or_else(arity)?;
        let index = source.fields.iter().position(|field| field.name.as_deref() == Some(name));
        if index.is_none() {
            return Err(arity());
        }
        aligned.push(index);
    }
    for (index, field) in source.fields.iter().enumerate() {
        let derived = field
            .name
            .as_deref()
            .is_some_and(|name| derived_source_variant_field(owner, &source.name, name));
        if !aligned.contains(&Some(index)) && !derived {
            return Err(arity());
        }
    }
    Ok(aligned)
}


/// Source-only payload fields native MIR derives from the variant's other
/// fields. `Binary.overflow` is `Trap` exactly when the dispatch is a
/// fixed-width trap route (`{width}.trap.{op}`) for an overflowing operator,
/// the route `jet_codegen_binary_dispatch` picks for the same condition; native
/// MIR carries the trap in that route. Going to native MIR the field must equal
/// that derivation (else the conversion fails); coming back it is re-derived.
fn derived_source_variant_field(owner: &str, variant: &str, field: &str) -> bool {
    matches!((owner, variant, field), ("MIROperation", "Binary", "overflow"))
}

/// The variant field a derived Source field is derived from.
fn derived_field_basis(owner: &str, variant: &str) -> Result<&'static str, BootstrapHostCodecError> {
    match (owner, variant) {
        ("MIROperation", "Binary") => Ok("dispatch"),
        _ => Err(BootstrapHostCodecError::InvalidMetadata(format!(
            "MIR enum variant `{owner}::{variant}` has no derivation for its Source-only field"
        ))),
    }
}

fn variant_field_index(fields: &[VariantField], name: &str, owner: &str, variant: &str) -> Result<usize, BootstrapHostCodecError> {
    fields.iter().position(|field| field.name.as_deref() == Some(name)).ok_or_else(|| {
        BootstrapHostCodecError::InvalidMetadata(format!(
            "MIR enum variant `{owner}::{variant}` has no `{name}` field"
        ))
    })
}

/// The `to_host` statement that refuses a derived Source field whose value
/// disagrees with its derivation from the converted native fields
/// (`__host_{index}`, bound in native field order).
fn derived_field_to_host_check(
    symbols: &BootstrapCodecSymbols<'_>,
    owner: &str,
    source: &Variant,
    host: &Variant,
    field_index: usize,
    bindings: &[String],
) -> Result<String, BootstrapHostCodecError> {
    let basis = derived_field_basis(owner, &source.name)?;
    let host_basis = variant_field_index(&host.fields, basis, owner, &source.name)?;
    let trap = symbols.variant_path("MIRBinaryOverflow", "Trap")?;
    Ok(format!(
        "if __jet_bootstrap_binary_traps(&__host_{host_basis})? != matches!({}, {trap}) {{ return Err(\"Source MIR Binary overflow disagrees with its dispatch: Trap is exactly a fixed-width trap route for an overflowing operator\".to_string()); }}",
        bindings[field_index]
    ))
}

/// The `from_host` expression that re-derives a derived Source field.
fn derived_field_from_host(
    symbols: &BootstrapCodecSymbols<'_>,
    owner: &str,
    source: &Variant,
    host: &Variant,
    bindings: &[String],
) -> Result<String, BootstrapHostCodecError> {
    let basis = derived_field_basis(owner, &source.name)?;
    let host_basis = variant_field_index(&host.fields, basis, owner, &source.name)?;
    let trap = symbols.variant_path("MIRBinaryOverflow", "Trap")?;
    let unchecked = symbols.variant_path("MIRBinaryOverflow", "Unchecked")?;
    Ok(format!(
        "if __jet_bootstrap_binary_traps({})? {{ {trap} }} else {{ {unchecked} }}",
        bindings[host_basis]
    ))
}

/// The program-level context `Binary.overflow` derives from: the ids of the
/// fixed-width trap routes, collected from the program being converted and in
/// scope only while that one program converts.
fn emit_binary_overflow_derivation(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let program = symbols.type_symbol("MIRProgram")?;
    let prelude_calls = symbols.field_symbol("MIRProgram", "prelude_calls")?;
    let family = symbols.field_symbol("MIRPreludeCall", "family")?;
    let member = symbols.field_symbol("MIRPreludeCall", "member")?;
    let id = symbols.field_symbol("MIRPreludeCall", "id")?;
    let id_value = symbols.field_symbol("MIRPreludeCallID", "value")?;
    let overflow = symbols.variant_path("MIRPreludeFamily", "Overflow")?;
    writeln!(
        out,
        "thread_local! {{\n    static __JET_BOOTSTRAP_TRAP_ROUTES: ::std::cell::RefCell<Option<::std::collections::BTreeSet<u64>>> = ::std::cell::RefCell::new(None);\n}}\n\
         fn __jet_bootstrap_fixed_trap_member(member: &str) -> bool {{\n    member.split_once(\".trap.\").is_some_and(|(width, op)| matches!(width, \"i8\" | \"i16\" | \"i32\" | \"i64\" | \"u8\" | \"u16\" | \"u32\" | \"u64\") && matches!(op, \"add\" | \"sub\" | \"mul\" | \"div\" | \"pow\" | \"shl\" | \"shr\"))\n}}\n\
         fn __jet_bootstrap_with_trap_routes<T>(routes: ::std::collections::BTreeSet<u64>, convert: impl FnOnce() -> Result<T, String>) -> Result<T, String> {{\n    let previous = __JET_BOOTSTRAP_TRAP_ROUTES.with(|cell| cell.replace(Some(routes)));\n    let result = convert();\n    __JET_BOOTSTRAP_TRAP_ROUTES.with(|cell| *cell.borrow_mut() = previous);\n    result\n}}\n\
         fn __jet_bootstrap_binary_traps(dispatch: &::jet_foundation::MIR::MirBinaryDispatch) -> Result<bool, String> {{\n    match dispatch {{\n        ::jet_foundation::MIR::MirBinaryDispatch::Primitive => Ok(false),\n        ::jet_foundation::MIR::MirBinaryDispatch::Prelude {{ call, .. }} => __JET_BOOTSTRAP_TRAP_ROUTES.with(|cell| cell.borrow().as_ref().map(|routes| routes.contains(&call.0))).ok_or_else(|| \"MIR Binary overflow derives from the program's Prelude routes; convert the whole program\".to_string()),\n    }}\n}}\n\
         fn __jet_bootstrap_source_trap_routes(value: &{program}) -> ::std::collections::BTreeSet<u64> {{\n    value.{prelude_calls}.iter().filter(|row| matches!(row.{family}, {overflow}) && __jet_bootstrap_fixed_trap_member(&row.{member})).map(|row| row.{id}.{id_value}).collect()\n}}\n\
         fn __jet_bootstrap_host_trap_routes(value: &::jet_foundation::MIR::MirProgram) -> ::std::collections::BTreeSet<u64> {{\n    value.prelude_calls.iter().filter(|row| matches!(row.family, ::jet_foundation::MIR::MirPreludeFamily::Overflow) && __jet_bootstrap_fixed_trap_member(&row.member)).map(|row| row.id.0).collect()\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

/// Jet cannot spell a variant `None` (it is the absent-value keyword), so the
/// Source enums name the empty case after their subject.
fn host_variant_name(owner: &str, variant: &str) -> String {
    match (owner, variant) {
        ("MIRDropKind", "NoDrop")
        | ("MIRCopyCost", "NoCopy")
        | ("MIREntryOutput", "NoOutput")
        | ("MIRAllocatorPolicy", "NoAllocator")
        | ("MIRClockPolicy", "NoClock")
        | ("MIREntropyPolicy", "NoEntropy")
        | ("MIRSchedulerPolicy", "NoScheduler")
        | ("MIRByteSinkPolicy", "NoByteSink")
        | ("MIRMMIOPolicy", "NoMMIO")
        | ("CoreCallPureRoute", "NoPureRoute")
        | ("CoreCallInterpreterRoute", "NoRoute") => {
            "None".to_string()
        }
        _ => variant.to_string(),
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









fn parse_type(input: &str) -> Result<TypeExpr, String> {
    let input = input.trim().trim_end_matches(',').trim();
    if input.is_empty() {
        return Err("empty MIR field type".to_string());
    }
    if let Some(rest) = input.strip_prefix('?').or_else(|| input.strip_suffix('?')) {
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
        // A `macro_rules!` template (`pub struct $name(...)`) declares no
        // schema row; its expansions are not visible to this source scan.
        if input.as_bytes().get(position) == Some(&b'$') {
            cursor = position;
            continue;
        }
        let name = read_identifier(&input, &mut position)?;
        let (definition, end) = parse_definition_body(&input, keyword, name, position)?;
        definitions.push(definition);
        cursor = end;
    }
    Ok(definitions)
}

/// Parse the body of the `struct`/`enum` declaration `name` whose identifier
/// ends at `position`; returns it and the offset just past the body. `Self`
/// in a field type is spelled as the declaring type.
fn parse_definition_body(
    input: &str,
    keyword: &str,
    name: String,
    mut position: usize,
) -> Result<(Definition, usize), String> {
    skip_space(input, &mut position);
    let body = match input.as_bytes().get(position).copied() {
        Some(b'{') => {
            let end = matching_delimiter(input, position, b'{', b'}')?;
            let body = &input[position + 1..end];
            position = end + 1;
            if keyword == "struct" {
                Body::Struct(
                    parse_fields(body)?
                        .into_iter()
                        .map(|field| Field {
                            ty: spell_self(&field.ty, &name),
                            ..field
                        })
                        .collect(),
                )
            } else {
                Body::Enum(
                    parse_variants(body)?
                        .into_iter()
                        .map(|variant| Variant {
                            fields: variant
                                .fields
                                .into_iter()
                                .map(|field| VariantField {
                                    ty: spell_self(&field.ty, &name),
                                    ..field
                                })
                                .collect(),
                            ..variant
                        })
                        .collect(),
                )
            }
        }
        Some(b'(') => {
            let end = matching_delimiter(input, position, b'(', b')')?;
            position = end + 1;
            Body::Tuple
        }
        _ => return Err(format!("MIR declaration `{name}` has no body")),
    };
    Ok((Definition { name, body }, position))
}

/// `ty` with each `Self` path segment spelled as `owner`.
fn spell_self(ty: &str, owner: &str) -> String {
    let bytes = ty.as_bytes();
    let identifier = |index: usize| bytes.get(index).is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
    let mut out = String::with_capacity(ty.len());
    let mut cursor = 0;
    while let Some(relative) = ty[cursor..].find("Self") {
        let start = cursor + relative;
        let end = start + "Self".len();
        out.push_str(&ty[cursor..start]);
        if identifier(start.wrapping_sub(1)) || identifier(end) {
            out.push_str("Self");
        } else {
            out.push_str(owner);
        }
        cursor = end;
    }
    out.push_str(&ty[cursor..]);
    out
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
        (Some(_), Some(right)) => Some(("enum", right + 4)),
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
        // The type ends at the field's comma; field attributes that follow
        // (`#[serde(..)]`) belong to the next field.
        let ty = split_top_level(&field[cursor..], ',')
            .into_iter()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
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
        // Variant attributes (`#[default]`) carry no schema shape.
        while input[cursor..].starts_with("#[") {
            cursor = matching_delimiter(input, cursor + 1, b'[', b']')? + 1;
            skip_space_and_commas(input, &mut cursor);
        }
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
        // An explicit discriminant (`Core = 0`) carries no schema shape.
        skip_space(input, &mut cursor);
        if input[cursor..].starts_with('=') {
            cursor += split_top_level(&input[cursor..], ',')[0].len();
        }
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
            // `crate::Type` path separators are not a field's colon.
            ':' if depth == 0
                && !input[..index].ends_with(':')
                && !input[index + 1..].starts_with(':') =>
            {
                return Some(index)
            }
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
                    "MIRType".to_string(),
                )))),
            )))
        );
    }
}
