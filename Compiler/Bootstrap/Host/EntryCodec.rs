//! Checked request/result transport for the private generated compiler entry.
//!
//! The retained compiler MIR and the emitted binding descriptor are the sole
//! shape authorities. Ordinary aggregates are copied only as required to cross
//! the entry ABI. Shared, trait-object, and callable leaves go through the
//! invocation-local NativeAdapter hook; they are never serialized as IDs or
//! reconstructed from display text.

use crate::compiler_bootstrap_host::{
    BootstrapBindingDescriptor, BootstrapCodecSymbols, BootstrapHostCodecError,
};
use jet_foundation::MIR::{
    MirConstKey, MirFieldId, MirFunctionId, MirFunctionKind, MirProgram, MirRuntimeValue, MirType,
    MirTypeDef, MirTypeDefKind, MirTypeId, MirTypeKind, MirVariantPayload,
};
use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as _;

const MAX_VALUE_DEPTH: usize = 256;

/// NativeAdapter-owned conversion for physical compiler-entry values.
///
/// The hook is generic so generated glue can pass the actual typed Source
/// field to its owner. Implementations validate that the field belongs to the
/// active registered binding set before returning its exact root carrier.
pub(crate) trait BootstrapEntryPhysicalBindings {
    fn encode<T: 'static>(
        &self,
        field_path: &[String],
        checked_type: &MirType,
        source_value: &T,
    ) -> Result<Option<MirRuntimeValue>, String>;

    fn decode<T: 'static>(
        &self,
        field_path: &[String],
        checked_type: &MirType,
        runtime_value: MirRuntimeValue,
    ) -> Result<Option<T>, String>;
}

/// A complete entry result, retained as its checked runtime aggregate until a
/// tier-specific generated Source projection consumes it.
#[derive(Debug)]
pub(crate) struct BootstrapEntryValue {
    pub(crate) ty: MirType,
    value: MirRuntimeValue,
}

impl BootstrapEntryValue {
    pub(crate) fn into_value(self) -> MirRuntimeValue {
        self.value
    }

    pub(crate) fn value(&self) -> &MirRuntimeValue {
        &self.value
    }

    pub(crate) fn field(
        &self,
        program: &MirProgram,
        field: MirFieldId,
    ) -> Option<&MirRuntimeValue> {
        checked_record_field(program, &self.ty, &self.value, field)
            .ok()
            .flatten()
    }
}

/// The exact checked `jet_bootstrap_compile` parameter and result carriers.
pub(crate) struct BootstrapEntryCodec<'a> {
    program: &'a MirProgram,
    entry: MirFunctionId,
    request_type: MirType,
    result_type: MirType,
}

impl<'a> BootstrapEntryCodec<'a> {
    pub(crate) fn new(
        program: &'a MirProgram,
        bindings: &BootstrapBindingDescriptor,
        entry: MirFunctionId,
    ) -> Result<Self, BootstrapHostCodecError> {
        let symbols = BootstrapCodecSymbols::new(bindings)?;
        let callable = bindings
            .callables
            .iter()
            .find(|row| row.source_name == "jet_bootstrap_compile" && row.metadata.function == entry)
            .ok_or_else(|| BootstrapHostCodecError::MissingEntry("jet_bootstrap_compile".to_string()))?;
        let function = program
            .functions
            .iter()
            .find(|function| function.id == entry)
            .ok_or_else(|| {
                BootstrapHostCodecError::InvalidMetadata(
                    "jet_bootstrap_compile binding is absent from checked compiler MIR".to_string(),
                )
            })?;
        if function.kind != MirFunctionKind::Jet
            || function.name != callable.source_name
            || function.params.len() != 1
            || callable.metadata.parameter_types.len() != 1
            || callable.metadata.parameter_access.as_slice() != [function.params[0].access]
        {
            return Err(BootstrapHostCodecError::InvalidMetadata(
                "jet_bootstrap_compile must retain its exact one-parameter checked ABI".to_string(),
            ));
        }
        let request_type = function.params[0].ty.clone();
        let result_type = function.return_type.clone();
        let request = checked_nominal_definition(program, &request_type)?;
        let result = checked_nominal_definition(program, &result_type)?;
        if request.name != "JetDriverCompileRequest" || result.name != "JetDriverCompileResult" {
            return Err(BootstrapHostCodecError::InvalidMetadata(
                "jet_bootstrap_compile ABI is not JetDriverCompileRequest -> JetDriverCompileResult"
                    .to_string(),
            ));
        }
        if callable.metadata.return_type != result_type.canonical_key() {
            return Err(BootstrapHostCodecError::InvalidMetadata(
                "jet_bootstrap_compile return binding disagrees with its checked MIR return type"
                    .to_string(),
            ));
        }
        validate_binding_graph(program, bindings, &symbols, &request_type)?;
        validate_binding_graph(program, bindings, &symbols, &result_type)?;
        Ok(Self {
            program,
            entry,
            request_type,
            result_type,
        })
    }

    pub(crate) fn entry(&self) -> MirFunctionId {
        self.entry
    }

    pub(crate) fn request_type(&self) -> &MirType {
        &self.request_type
    }

    pub(crate) fn result_type(&self) -> &MirType {
        &self.result_type
    }

    /// Validate and transfer one complete aggregate into the checked factory
    /// parameter vector. The factory entry has exactly one source argument.
    pub(crate) fn encode_compile_arguments(
        &self,
        request: MirRuntimeValue,
    ) -> Result<Vec<MirRuntimeValue>, BootstrapHostCodecError> {
        validate_runtime_value(self.program, &self.request_type, &request, 0)
            .map_err(BootstrapHostCodecError::InvalidMetadata)?;
        Ok(vec![request])
    }

    /// Validate and transfer the entire result aggregate without dropping
    /// diagnostics, source MIR, artifact/output metadata, or control results.
    pub(crate) fn decode_compile_result(
        &self,
        value: MirRuntimeValue,
    ) -> Result<BootstrapEntryValue, BootstrapHostCodecError> {
        validate_runtime_value(self.program, &self.result_type, &value, 0)
            .map_err(BootstrapHostCodecError::InvalidMetadata)?;
        Ok(BootstrapEntryValue {
            ty: self.result_type.clone(),
            value,
        })
    }

    /// Build a record from checked field IDs, emitted in checked MIR order.
    pub(crate) fn record(
        &self,
        owner: MirTypeId,
        fields: Vec<(MirFieldId, MirRuntimeValue)>,
    ) -> Result<MirRuntimeValue, BootstrapHostCodecError> {
        let definition = checked_definition_by_id(self.program, owner)?;
        let declared = match &definition.kind {
            MirTypeDefKind::Struct { fields, .. } => fields,
            _ => {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "checked type `{}` is not a struct carrier",
                    definition.name
                )))
            }
        };
        let mut supplied = std::collections::BTreeMap::new();
        for (field, value) in fields {
            if supplied.insert(field, value).is_some() {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "record `{}` repeats checked field ID {}",
                    definition.name, field.0
                )));
            }
        }
        if supplied.len() != declared.len() {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "record `{}` has {} values for {} checked fields",
                definition.name,
                supplied.len(),
                declared.len()
            )));
        }
        let mut output = Vec::with_capacity(declared.len());
        for field in declared {
            let value = supplied.remove(&field.id).ok_or_else(|| {
                BootstrapHostCodecError::InvalidMetadata(format!(
                    "record `{}` is missing checked field `{}`",
                    definition.name, field.name
                ))
            })?;
            validate_runtime_value(self.program, &field.ty, &value, 0)
                .map_err(BootstrapHostCodecError::InvalidMetadata)?;
            output.push((field.name.clone(), value));
        }
        if !supplied.is_empty() {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "record `{}` contains an undeclared checked field ID",
                definition.name
            )));
        }
        Ok(MirRuntimeValue::Struct {
            type_name: definition.name.clone(),
            fields: output,
        })
    }

    /// Build an enum from its checked owner, variant, and payload field types.
    pub(crate) fn enumeration(
        &self,
        owner: MirTypeId,
        variant_name: &str,
        values: Vec<MirRuntimeValue>,
    ) -> Result<MirRuntimeValue, BootstrapHostCodecError> {
        let definition = checked_definition_by_id(self.program, owner)?;
        let MirTypeDefKind::Enum { variants, .. } = &definition.kind else {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "checked type `{}` is not an enum carrier",
                definition.name
            )));
        };
        let variant = variants
            .iter()
            .find(|variant| variant.name == variant_name)
            .ok_or_else(|| BootstrapHostCodecError::MissingVariant {
                owner: definition.name.clone(),
                variant: variant_name.to_string(),
            })?;
        let expected = match &variant.payload {
            MirVariantPayload::Unit => Vec::new(),
            MirVariantPayload::Single(ty) => vec![(None, ty)],
            MirVariantPayload::Named(fields) => fields
                .iter()
                .map(|field| (Some(field.name.as_str()), &field.ty))
                .collect(),
        };
        if values.len() != expected.len() {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "enum `{}` variant `{}` has {} values for {} checked fields",
                definition.name,
                variant_name,
                values.len(),
                expected.len()
            )));
        }
        let mut args = Vec::with_capacity(values.len());
        for ((name, ty), value) in expected.into_iter().zip(values) {
            validate_runtime_value(self.program, ty, &value, 0)
                .map_err(BootstrapHostCodecError::InvalidMetadata)?;
            args.push((name.map(str::to_string), value));
        }
        Ok(MirRuntimeValue::Enum {
            type_name: definition.name.clone(),
            variant: variant.name.clone(),
            args,
        })
    }

    /// Build Present/Absent while retaining the checked optional element type.
    pub(crate) fn option(
        &self,
        ty: &MirType,
        value: Option<MirRuntimeValue>,
    ) -> Result<MirRuntimeValue, BootstrapHostCodecError> {
        let MirTypeKind::Option(inner) = ty.kind() else {
            return Err(BootstrapHostCodecError::InvalidMetadata(
                "optional carrier requested for a non-optional checked type".to_string(),
            ));
        };
        match value {
            Some(value) => {
                validate_runtime_value(self.program, inner, &value, 0)
                    .map_err(BootstrapHostCodecError::InvalidMetadata)?;
                Ok(MirRuntimeValue::Present(Box::new(value)))
            }
            None => Ok(MirRuntimeValue::Absent {
                element: inner.as_ref().clone(),
            }),
        }
    }
}

/// Append typed request/result conversion code to the private emitted artifact.
/// The generated routines use only types/fields/variants reached from the exact
/// factory signature and resolved through the same binding descriptor.
pub(crate) fn append_bootstrap_entry_codec(
    out: &mut String,
    bindings: &BootstrapBindingDescriptor,
    symbols: &BootstrapCodecSymbols<'_>,
    program: &MirProgram,
    entry: MirFunctionId,
) -> Result<(), BootstrapHostCodecError> {
    let function = program
        .functions
        .iter()
        .find(|function| function.id == entry)
        .ok_or_else(|| {
            BootstrapHostCodecError::InvalidMetadata(
                "entry codec cannot find the checked compiler factory function".to_string(),
            )
        })?;
    if function.name != "jet_bootstrap_compile" || function.params.len() != 1 {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "entry codec requires the checked one-request factory function".to_string(),
        ));
    }
    let request = checked_nominal_definition(program, &function.params[0].ty)?;
    let result = checked_nominal_definition(program, &function.return_type)?;
    if request.name != "JetDriverCompileRequest" || result.name != "JetDriverCompileResult" {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "factory signature is not the checked compile request/result contract".to_string(),
        ));
    }
    validate_binding_graph(program, bindings, symbols, &function.params[0].ty)?;
    validate_binding_graph(program, bindings, symbols, &function.return_type)?;

    let mut request_types = Vec::new();
    let mut result_types = Vec::new();
    collect_nominal_types(program, &function.params[0].ty, &mut HashSet::new(), &mut request_types)?;
    collect_nominal_types(program, &function.return_type, &mut HashSet::new(), &mut result_types)?;
    request_types.sort_by_key(|definition| definition.id);
    result_types.sort_by_key(|definition| definition.id);
    for definition in &request_types {
        emit_to_runtime_converter(out, symbols, definition)?;
    }
    for definition in &result_types {
        emit_from_runtime_converter(out, symbols, definition)?;
    }
    emit_entry_conversion_helpers(out)?;

    let request_source = symbols.type_symbol("JetDriverCompileRequest")?;
    let result_source = symbols.type_symbol("JetDriverCompileResult")?;
    let request_expression = to_runtime_expression(
        program,
        symbols,
        &function.params[0].ty,
        "request",
        "&__jet_request_type",
        "program",
        "path",
        "physical",
    )?;
    let result_expression = from_runtime_expression(
        program,
        symbols,
        &function.return_type,
        "value",
        "&__jet_result_type",
        "program",
        "path",
    )?;
    writeln!(
        out,
        "\n#[doc(hidden)]\npub(crate) fn __jet_bootstrap_entry_request_to_runtime<P: crate::BootstrapEntryPhysicalBindings>(request: {request_source}, program: &::jet_foundation::MIR::MirProgram, physical: &P) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {{\n    let __jet_request_type = __jet_bootstrap_entry_type_for(program, ::jet_foundation::MIR::MirTypeId({request_id}))?;\n    let path = vec![\"JetDriverCompileRequest\".to_string()];\n    let value = {request_expression};\n    crate::compiler_bootstrap_entry_codec::validate_runtime_value(program, &__jet_request_type, &value, 0)?;\n    Ok(value)\n}}\n\n#[doc(hidden)]\npub(crate) fn __jet_bootstrap_entry_result_from_runtime<P: crate::BootstrapEntryPhysicalBindings>(value: ::jet_foundation::MIR::MirRuntimeValue, program: &::jet_foundation::MIR::MirProgram, physical: &P) -> Result<{result_source}, String> {{\n    let __jet_result_type = __jet_bootstrap_entry_type_for(program, ::jet_foundation::MIR::MirTypeId({result_id}))?;\n    crate::compiler_bootstrap_entry_codec::validate_runtime_value(program, &__jet_result_type, &value, 0)?;\n    let path = vec![\"JetDriverCompileResult\".to_string()];\n    {result_expression}\n}}\n",
        request_id = request.id.0,
        result_id = result.id.0,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}

fn emit_entry_conversion_helpers(out: &mut String) -> Result<(), BootstrapHostCodecError> {
    writeln!(
        out,
        "\nfn __jet_bootstrap_entry_type_for(program: &::jet_foundation::MIR::MirProgram, id: ::jet_foundation::MIR::MirTypeId) -> Result<::jet_foundation::MIR::MirType, String> {{\n    let definition = program.types.iter().find(|definition| definition.id == id).ok_or_else(|| format!(\"checked entry type {{:?}} is absent\", id))?;\n    Ok(::jet_foundation::MIR::MirType::from_kind(::jet_foundation::MIR::MirTypeKind::Apply {{ name: ::jet_foundation::MIR::MirNominalRef {{ id, name: definition.name.clone() }}, args: Vec::new() }}).with_identity(id))\n}}\n\nfn __jet_bootstrap_entry_field_type(program: &::jet_foundation::MIR::MirProgram, owner: ::jet_foundation::MIR::MirTypeId, field: ::jet_foundation::MIR::MirFieldId) -> Result<::jet_foundation::MIR::MirType, String> {{\n    let definition = program.types.iter().find(|definition| definition.id == owner).ok_or_else(|| format!(\"checked entry type {{:?}} is absent\", owner))?;\n    let ::jet_foundation::MIR::MirTypeDefKind::Struct {{ fields, .. }} = &definition.kind else {{ return Err(format!(\"checked entry type `{{}}` is not a struct\", definition.name)); }};\n    fields.iter().find(|row| row.id == field).map(|row| row.ty.clone()).ok_or_else(|| format!(\"checked entry field {{:?}} is absent from `{{}}`\", field, definition.name))\n}}\n\nfn __jet_bootstrap_entry_variant_field_type(program: &::jet_foundation::MIR::MirProgram, owner: ::jet_foundation::MIR::MirTypeId, variant: &str, field: usize) -> Result<::jet_foundation::MIR::MirType, String> {{\n    let definition = program.types.iter().find(|definition| definition.id == owner).ok_or_else(|| format!(\"checked entry type {{:?}} is absent\", owner))?;\n    let ::jet_foundation::MIR::MirTypeDefKind::Enum {{ variants, .. }} = &definition.kind else {{ return Err(format!(\"checked entry type `{{}}` is not an enum\", definition.name)); }};\n    let row = variants.iter().find(|row| row.name == variant).ok_or_else(|| format!(\"checked entry variant `{{}}::{{}}` is absent\", definition.name, variant))?;\n    match &row.payload {{\n        ::jet_foundation::MIR::MirVariantPayload::Single(ty) if field == 0 => Ok(ty.clone()),\n        ::jet_foundation::MIR::MirVariantPayload::Named(fields) => fields.get(field).map(|row| row.ty.clone()).ok_or_else(|| format!(\"checked entry enum field index {{field}} is absent\")),\n        _ => Err(format!(\"checked entry enum variant `{{}}::{{}}` has no field at index {{field}}\", definition.name, variant)),\n    }}\n}}\n\nfn __jet_bootstrap_entry_child_type(ty: &::jet_foundation::MIR::MirType, kind: &str, index: usize) -> Result<&::jet_foundation::MIR::MirType, String> {{\n    match (kind, ty.kind()) {{\n        (\"list\", ::jet_foundation::MIR::MirTypeKind::List(inner)) | (\"fixed-list\", ::jet_foundation::MIR::MirTypeKind::FixedList {{ elem: inner, .. }}) | (\"option\", ::jet_foundation::MIR::MirTypeKind::Option(inner)) | (\"shared\", ::jet_foundation::MIR::MirTypeKind::Shared(inner)) | (\"range\", ::jet_foundation::MIR::MirTypeKind::InlineRange {{ base: inner, .. }}) | (\"tagged\", ::jet_foundation::MIR::MirTypeKind::Tagged {{ inner, .. }}) | (\"quantity\", ::jet_foundation::MIR::MirTypeKind::Quantity {{ base: inner, .. }}) => Ok(inner),\n        (\"map\", ::jet_foundation::MIR::MirTypeKind::Map {{ key, .. }}) if index == 0 => Ok(key),\n        (\"map\", ::jet_foundation::MIR::MirTypeKind::Map {{ value, .. }}) if index == 1 => Ok(value),\n        (\"result\", ::jet_foundation::MIR::MirTypeKind::Result {{ ok, .. }}) if index == 0 => Ok(ok),\n        (\"result\", ::jet_foundation::MIR::MirTypeKind::Result {{ err, .. }}) if index == 1 => Ok(err),\n        (\"tuple\", ::jet_foundation::MIR::MirTypeKind::Tuple(fields)) => fields.get(index).map(|(_, ty)| ty).ok_or_else(|| \"checked tuple field is absent\".to_string()),\n        _ => Err(format!(\"checked MIR type `{{}}` has no {{kind}} child {{index}}\", ty.canonical_key())),\n    }\n}}\n\nfn __jet_bootstrap_entry_integer_to_runtime(value: &jet_foundation::Numeric::JetInt) -> ::jet_foundation::MIR::MirRuntimeValue {{\n    value.to_i64().map(::jet_foundation::MIR::MirRuntimeValue::Int).unwrap_or_else(|| ::jet_foundation::MIR::MirRuntimeValue::BigInt(value.to_string_rep()))\n}}\n\nfn __jet_bootstrap_entry_integer_from_runtime(value: ::jet_foundation::MIR::MirRuntimeValue) -> Result<jet_foundation::Numeric::JetInt, String> {{\n    match value {{\n        ::jet_foundation::MIR::MirRuntimeValue::Int(value) => Ok(jet_foundation::Numeric::JetInt::from_i64(value)),\n        ::jet_foundation::MIR::MirRuntimeValue::BigInt(value) => jet_foundation::Numeric::JetInt::from_str(&value),\n        _ => Err(\"entry integer does not have an exact integer carrier\".to_string()),\n    }}\n}}\n\nfn __jet_bootstrap_entry_integer_text(value: ::jet_foundation::MIR::MirRuntimeValue) -> Result<String, String> {{\n    match value {{\n        ::jet_foundation::MIR::MirRuntimeValue::Int(value) => Ok(value.to_string()),\n        ::jet_foundation::MIR::MirRuntimeValue::BigInt(value) => Ok(value),\n        _ => Err(\"entry fixed-width integer does not have an exact integer carrier\".to_string()),\n    }}\n}}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn emit_to_runtime_converter(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
    definition: &MirTypeDef,
) -> Result<(), BootstrapHostCodecError> {
    let source = symbols.type_symbol(&definition.name)?;
    let id = definition.id.0;
    let body = match &definition.kind {
        MirTypeDefKind::Struct { fields, .. } => {
            let mut rows = Vec::with_capacity(fields.len());
            for field in fields {
                let symbol = symbols.field_symbol(&definition.name, &field.name)?;
                let expr = to_runtime_expression(
                    field_program_placeholder(),
                    symbols,
                    &field.ty,
                    &format!("&value.{symbol}"),
                    &format!("&__jet_field_type_{}", field.id.0),
                    "program",
                    &format!("&__jet_field_path_{}", field.id.0),
                    "physical",
                )?;
                rows.push(format!(
                    "let __jet_field_type_{field_id} = __jet_bootstrap_entry_field_type(program, ::jet_foundation::MIR::MirTypeId({owner}), ::jet_foundation::MIR::MirFieldId({field_id}))?; let mut __jet_field_path_{field_id} = path.to_vec(); __jet_field_path_{field_id}.push({field_name:?}.to_string()); fields.push(({field_name:?}.to_string(), {expr}));",
                    owner = id,
                    field_id = field.id.0,
                    field_name = field.name,
                ));
            }
            format!(
                "let definition = program.types.iter().find(|definition| definition.id == ::jet_foundation::MIR::MirTypeId({id})).ok_or_else(|| \"checked entry record type is absent\".to_string())?; if ty.nominal_id() != Some(definition.id) {{ return Err(format!(\"entry type mismatch for `{{}}`\", definition.name)); }} let mut fields = Vec::with_capacity({field_count}); {rows} Ok(::jet_foundation::MIR::MirRuntimeValue::Struct {{ type_name: {name:?}.to_string(), fields }})",
                id = id,
                field_count = fields.len(),
                rows = rows.join(" "),
                name = definition.name,
            )
        }
        MirTypeDefKind::Enum { variants, .. } => {
            let mut arms = Vec::with_capacity(variants.len());
            for variant in variants {
                let path = symbols.variant_path(&definition.name, &variant.name)?;
                let fields = match &variant.payload {
                    MirVariantPayload::Unit => Vec::new(),
                    MirVariantPayload::Single(ty) => vec![(None, None, ty)],
                    MirVariantPayload::Named(fields) => fields
                        .iter()
                        .map(|field| {
                            Ok((
                                Some(field.name.as_str()),
                                Some(symbols.field_symbol(&definition.name, &field.name)?),
                                &field.ty,
                            ))
                        })
                        .collect::<Result<Vec<_>, BootstrapHostCodecError>>()?,
                };
                let bindings = fields
                    .iter()
                    .enumerate()
                    .map(|(index, (_, symbol, _))| {
                        symbol.map_or_else(|| format!("__payload_{index}"), |symbol| symbol.to_string())
                    })
                    .collect::<Vec<_>>();
                let pattern = if fields.is_empty() {
                    path.clone()
                } else if fields.iter().all(|(name, _, _)| name.is_some()) {
                    format!(
                        "{path} {{ {} }}",
                        fields
                            .iter()
                            .zip(&bindings)
                            .map(|((name, _, _), binding)| format!("{}: {binding}", name.unwrap()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                } else {
                    format!("{path}({})", bindings.join(", "))
                };
                let mut encode = Vec::with_capacity(fields.len());
                for (index, ((name, _, field_ty), binding)) in fields.iter().zip(&bindings).enumerate() {
                    let ty_var = format!("__jet_variant_type_{index}");
                    let path_var = format!("__jet_variant_path_{index}");
                    let value = to_runtime_expression(
                        field_program_placeholder(),
                        symbols,
                        field_ty,
                        &format!("{binding}"),
                        &format!("&{ty_var}"),
                        "program",
                        &format!("&{path_var}"),
                        "physical",
                    )?;
                    let name_expr = name.map_or_else(|| "None".to_string(), |name| format!("Some({name:?}.to_string())"));
                    encode.push(format!(
                        "let {ty_var} = __jet_bootstrap_entry_variant_field_type(program, ::jet_foundation::MIR::MirTypeId({owner}), {variant_name:?}, {index})?; let mut {path_var} = path.to_vec(); {path_var}.push(format!(\"{{}}::{{}}[{index}]\", {owner_name:?}, {variant_name:?})); args.push(({name_expr}, {value}));",
                        owner = id,
                        variant_name = variant.name,
                        owner_name = definition.name,
                    ));
                }
                arms.push(format!(
                    "{pattern} => {{ let mut args = Vec::with_capacity({payload_len}); {encode} Ok(::jet_foundation::MIR::MirRuntimeValue::Enum {{ type_name: {owner_name:?}.to_string(), variant: {variant_name:?}.to_string(), args }}) }},",
                    payload_len = fields.len(),
                    encode = encode.join(" "),
                    owner_name = definition.name,
                    variant_name = variant.name,
                ));
            }
            format!(
                "let definition = program.types.iter().find(|definition| definition.id == ::jet_foundation::MIR::MirTypeId({id})).ok_or_else(|| \"checked entry enum type is absent\".to_string())?; if ty.nominal_id() != Some(definition.id) {{ return Err(format!(\"entry type mismatch for `{{}}`\", definition.name)); }} match value {{ {} }}",
                arms.join(" "),
                id = id,
            )
        }
        MirTypeDefKind::Distinct { base, .. } => {
            let expr = to_runtime_expression(
                field_program_placeholder(),
                symbols,
                base,
                "&value.0",
                "&__jet_distinct_base",
                "program",
                "path",
                "physical",
            )?;
            format!(
                "let __jet_distinct_base = __jet_bootstrap_entry_distinct_base(program, ::jet_foundation::MIR::MirTypeId({id}))?; {expr}",
                id = id,
            )
        }
        MirTypeDefKind::Alias { target } => to_runtime_expression(
            field_program_placeholder(),
            symbols,
            target,
            "value",
            "ty",
            "program",
            "path",
            "physical",
        )?,
        MirTypeDefKind::UnitFamily { members } => {
            let mut arms = Vec::with_capacity(members.len());
            for member in members {
                let path = symbols.variant_path(&definition.name, member)?;
                arms.push(format!(
                    "{path} => Ok(::jet_foundation::MIR::MirRuntimeValue::Enum {{ type_name: {name:?}.to_string(), variant: {member:?}.to_string(), args: Vec::new() }}),",
                    name = definition.name,
                ));
            }
            format!("match value {{ {} }}", arms.join(" "))
        }
    };
    writeln!(
        out,
        "fn __jet_bootstrap_entry_type_{id}_to_runtime<P: crate::BootstrapEntryPhysicalBindings>(value: &{source}, ty: &::jet_foundation::MIR::MirType, program: &::jet_foundation::MIR::MirProgram, path: &[String], physical: &P) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {{ {body} }}\n",
        id = id,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn emit_from_runtime_converter(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
    definition: &MirTypeDef,
) -> Result<(), BootstrapHostCodecError> {
    let source = symbols.type_symbol(&definition.name)?;
    let id = definition.id.0;
    let body = match &definition.kind {
        MirTypeDefKind::Struct { fields, .. } => {
            let mut binds = Vec::with_capacity(fields.len());
            let mut initializers = Vec::with_capacity(fields.len());
            for field in fields {
                let symbol = symbols.field_symbol(&definition.name, &field.name)?;
                let value_name = format!("__jet_field_value_{}", field.id.0);
                let type_name = format!("__jet_field_type_{}", field.id.0);
                let expr = from_runtime_expression(
                    field_program_placeholder(),
                    symbols,
                    &field.ty,
                    &value_name,
                    &format!("&{type_name}"),
                    "program",
                    &format!("&{{ let mut p = path.to_vec(); p.push({:?}.to_string()); p }}", field.name),
                )?;
                binds.push(format!(
                    "let {type_name} = __jet_bootstrap_entry_field_type(program, ::jet_foundation::MIR::MirTypeId({owner}), ::jet_foundation::MIR::MirFieldId({field_id}))?; let ({field_name}, {value_name}) = __jet_bootstrap_entry_next_record_field(&mut fields, {field_name:?})?;",
                    owner = id,
                    field_id = field.id.0,
                    field_name = field.name,
                ));
                initializers.push(format!("{symbol}: {expr}"));
            }
            format!(
                "let definition = program.types.iter().find(|definition| definition.id == ::jet_foundation::MIR::MirTypeId({id})).ok_or_else(|| \"checked entry record type is absent\".to_string())?; if ty.nominal_id() != Some(definition.id) {{ return Err(format!(\"entry type mismatch for `{{}}`\", definition.name)); }} let ::jet_foundation::MIR::MirRuntimeValue::Struct {{ type_name, fields: mut fields }} = value else {{ return Err(format!(\"expected record carrier for `{{}}`\", {name:?})); }}; if type_name != {name:?} {{ return Err(format!(\"record carrier type mismatch for `{{}}`\", {name:?})); }} {} if fields.next().is_some() {{ return Err(format!(\"record `{{}}` contains undeclared fields\", {name:?})); }} Ok({source} {{ {} }})",
                binds.join(" "),
                initializers.join(", "),
                id = id,
                name = definition.name,
                source = source,
            )
        }
        MirTypeDefKind::Enum { variants, .. } => {
            let mut arms = Vec::with_capacity(variants.len());
            for variant in variants {
                let path = symbols.variant_path(&definition.name, &variant.name)?;
                let fields = match &variant.payload {
                    MirVariantPayload::Unit => Vec::new(),
                    MirVariantPayload::Single(ty) => vec![(None, None, ty)],
                    MirVariantPayload::Named(fields) => fields
                        .iter()
                        .map(|field| {
                            Ok((
                                Some(field.name.as_str()),
                                Some(symbols.field_symbol(&definition.name, &field.name)?),
                                &field.ty,
                            ))
                        })
                        .collect::<Result<Vec<_>, BootstrapHostCodecError>>()?,
                };
                let mut decode = Vec::with_capacity(fields.len());
                let mut payload_values = Vec::with_capacity(fields.len());
                for (index, (name, symbol, field_ty)) in fields.iter().enumerate() {
                    let arg = format!("__jet_arg_{index}");
                    let ty_var = format!("__jet_arg_type_{index}");
                    let expr = from_runtime_expression(
                        field_program_placeholder(),
                        symbols,
                        field_ty,
                        &format!("{arg}.1"),
                        &format!("&{ty_var}"),
                        "program",
                        &format!("&{{ let mut p = path.to_vec(); p.push(format!(\"{{}}::{{}}[{index}]\", {owner_name:?}, {variant_name:?})); p }}"),
                    )?;
                    let expected_name = name.map_or_else(|| "None".to_string(), |name| format!("Some({name:?})"));
                    decode.push(format!(
                        "let {ty_var} = __jet_bootstrap_entry_variant_field_type(program, ::jet_foundation::MIR::MirTypeId({owner}), {variant_name:?}, {index})?; let {arg} = __jet_bootstrap_entry_next_enum_arg(&mut args, {expected_name})?; let {arg}_value = {expr};",
                        owner = id,
                        variant_name = variant.name,
                        owner_name = definition.name,
                    ));
                    payload_values.push(match symbol {
                        Some(symbol) if matches!(variant.payload, MirVariantPayload::Named(_)) => {
                            format!("{symbol}: {arg}_value")
                        }
                        _ => format!("{arg}_value"),
                    });
                }
                let construction = if payload_values.is_empty() {
                    path
                } else if matches!(variant.payload, MirVariantPayload::Named(_)) {
                    format!("{path} {{ {} }}", payload_values.join(", "))
                } else {
                    format!("{path}({})", payload_values.join(", "))
                };
                arms.push(format!(
                    "{variant_name:?} => {{ let mut args = args.into_iter(); {} if args.next().is_some() {{ return Err(\"enum contains excess payload values\".to_string()); }} Ok({construction}) }},",
                    decode.join(" "),
                    variant_name = variant.name,
                ));
            }
            format!(
                "let definition = program.types.iter().find(|definition| definition.id == ::jet_foundation::MIR::MirTypeId({id})).ok_or_else(|| \"checked entry enum type is absent\".to_string())?; if ty.nominal_id() != Some(definition.id) {{ return Err(format!(\"entry type mismatch for `{{}}`\", definition.name)); }} let ::jet_foundation::MIR::MirRuntimeValue::Enum {{ type_name, variant, args }} = value else {{ return Err(format!(\"expected enum carrier for `{{}}`\", {name:?})); }}; if type_name != {name:?} {{ return Err(format!(\"enum carrier type mismatch for `{{}}`\", {name:?})); }} match variant.as_str() {{ {} _ => Err(format!(\"unknown checked enum variant `{{}}::{{}}`\", {name:?}, variant)), }}",
                arms.join(" "),
                id = id,
                name = definition.name,
            )
        }
        MirTypeDefKind::Distinct { base, .. } => {
            let expr = from_runtime_expression(
                field_program_placeholder(),
                symbols,
                base,
                "value",
                "&__jet_distinct_base",
                "program",
                "path",
            )?;
            format!(
                "let __jet_distinct_base = __jet_bootstrap_entry_distinct_base(program, ::jet_foundation::MIR::MirTypeId({id}))?; Ok({source}({expr}))",
                id = id,
                source = source,
            )
        }
        MirTypeDefKind::Alias { target } => from_runtime_expression(
            field_program_placeholder(),
            symbols,
            target,
            "value",
            "ty",
            "program",
            "path",
        )?,
        MirTypeDefKind::UnitFamily { members } => {
            let mut arms = Vec::with_capacity(members.len());
            for member in members {
                let path = symbols.variant_path(&definition.name, member)?;
                arms.push(format!("{member:?} => Ok({path}),"));
            }
            format!(
                "let ::jet_foundation::MIR::MirRuntimeValue::Enum {{ type_name, variant, args }} = value else {{ return Err(\"expected unit-family enum carrier\".to_string()); }}; if type_name != {name:?} || !args.is_empty() {{ return Err(\"unit-family carrier shape mismatch\".to_string()); }} match variant.as_str() {{ {} _ => Err(format!(\"unknown checked unit-family member `{{}}`\", variant)), }}",
                arms.join(" "),
                name = definition.name,
            )
        }
    };
    writeln!(
        out,
        "fn __jet_bootstrap_entry_type_{id}_from_runtime<P: crate::BootstrapEntryPhysicalBindings>(value: ::jet_foundation::MIR::MirRuntimeValue, ty: &::jet_foundation::MIR::MirType, program: &::jet_foundation::MIR::MirProgram, path: &[String], physical: &P) -> Result<{source}, String> {{ {body} }}\n",
        id = id,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn to_runtime_expression(
    program: &MirProgram,
    symbols: &BootstrapCodecSymbols<'_>,
    ty: &MirType,
    source: &str,
    checked: &str,
    program_expr: &str,
    path: &str,
    physical: &str,
) -> Result<String, BootstrapHostCodecError> {
    use MirTypeKind as Kind;
    let mir = "::jet_foundation::MIR::MirRuntimeValue";
    Ok(match ty.kind() {
        Kind::Int => format!("__jet_bootstrap_entry_integer_to_runtime({source})"),
        Kind::IntN { .. } => format!("{mir}::BigInt(({source}).to_string())"),
        Kind::Float => format!("{mir}::Float {{ value: *({source}), f32: false }}"),
        Kind::Float32 => format!("{mir}::Float {{ value: f64::from(*({source})), f32: true }}"),
        Kind::Measure(_) => format!("{mir}::Float {{ value: *({source}), f32: false }}"),
        Kind::Bool => format!("{mir}::Bool(*({source}))"),
        Kind::Char => format!("{mir}::Char(*({source}))"),
        Kind::String => format!("{mir}::String(({source}).clone())"),
        Kind::List(inner) | Kind::FixedList { elem: inner, .. } => {
            let inner_ty = format!("__jet_bootstrap_entry_child_type({checked}, {}, 0)?", if matches!(ty.kind(), Kind::List(_)) { "\"list\"" } else { "\"fixed-list\"" });
            if is_u8_type(inner) {
                format!("{mir}::Bytes(({source}).iter().copied().collect())")
            } else {
                let child = to_runtime_expression(
                    program,
                    symbols,
                    inner,
                    "item",
                    "&__jet_item_type",
                    program_expr,
                    path,
                    physical,
                )?;
                format!("{{ let __jet_item_type = {inner_ty}; {mir}::List(({source}).iter().map(|item| Ok({child})).collect::<Result<Vec<_>, String>>()?) }}")
            }
        }
        Kind::Map { key, value } => {
            let key_ty = "__jet_bootstrap_entry_child_type(checked, \"map\", 0)?";
            let value_ty = "__jet_bootstrap_entry_child_type(checked, \"map\", 1)?";
            let encoded_key = to_const_key_expression(program, symbols, key, "key", "__jet_key_type", program_expr, path)?;
            let encoded_value = to_runtime_expression(program, symbols, value, "item", "__jet_value_type", program_expr, path, physical)?;
            format!("{{ let __jet_key_type = {key_ty}; let __jet_value_type = {value_ty}; {mir}::Map(({source}).iter().map(|(key, item)| Ok(({encoded_key}, {encoded_value}))).collect::<Result<Vec<_>, String>>()?) }}")
        }
        Kind::Shared(_) | Kind::TraitObject(_) | Kind::Fn(_) | Kind::SendFn { .. } => {
            format!("{physical}.encode({path}, checked, {source})?.ok_or_else(|| format!(\"NativeAdapter has no carrier for checked physical field `{{}}`\", {path}.join(\".\")))?")
        }
        Kind::Option(inner) => {
            let inner_ty = "__jet_bootstrap_entry_child_type(checked, \"option\", 0)?";
            let child = to_runtime_expression(program, symbols, inner, "inner", "&__jet_inner_type", program_expr, path, physical)?;
            format!("{{ let __jet_inner_type = {inner_ty}; match {source} {{ Ok(inner) => {mir}::Present(Box::new({child})), Err(_) => {mir}::Absent {{ element: __jet_inner_type.clone() }} }} }}")
        }
        Kind::Result { ok, err } => {
            let ok_ty = "__jet_bootstrap_entry_child_type(checked, \"result\", 0)?";
            let err_ty = "__jet_bootstrap_entry_child_type(checked, \"result\", 1)?";
            let ok_value = to_runtime_expression(program, symbols, ok, "inner", "&__jet_ok_type", program_expr, path, physical)?;
            let err_value = to_runtime_expression(program, symbols, err, "inner", "&__jet_err_type", program_expr, path, physical)?;
            format!("{{ let __jet_ok_type = {ok_ty}; let __jet_err_type = {err_ty}; match {source} {{ Ok(inner) => {mir}::Present(Box::new({ok_value})), Err(inner) => {mir}::FailedTold(Box::new({err_value})) }} }}")
        }
        Kind::Apply { name, .. } => {
            checked_definition_by_id(program, name.id)?;
            format!("__jet_bootstrap_entry_type_{}_to_runtime({source}, checked, {program_expr}, {path}, {physical})?", name.id.0)
        }
        Kind::Tuple(fields) => {
            let mut items = Vec::with_capacity(fields.len());
            for (index, (name, field_ty)) in fields.iter().enumerate() {
                let ty_expr = format!("__jet_bootstrap_entry_child_type(checked, \"tuple\", {index})?");
                let expr = to_runtime_expression(program, symbols, field_ty, &format!("&({source}).{index}"), &format!("__jet_tuple_type_{index}"), program_expr, path, physical)?;
                items.push(format!("{{ let __jet_tuple_type_{index} = {ty_expr}; ({name:?}.to_string(), {expr}) }}"));
            }
            if fields.is_empty() {
                format!("{mir}::Unit")
            } else {
                format!("{mir}::Struct {{ type_name: \"Tuple\".to_string(), fields: vec![{}] }}", items.join(", "))
            }
        }
        Kind::InlineRange { base, .. } | Kind::Tagged { inner: base, .. } | Kind::Quantity { base, .. } => {
            let kind = match ty.kind() {
                Kind::InlineRange { .. } => "range",
                Kind::Tagged { .. } => "tagged",
                _ => "quantity",
            };
            let child_ty = format!("__jet_bootstrap_entry_child_type(checked, \"{kind}\", 0)?");
            let child = to_runtime_expression(program, symbols, base, source, "__jet_inner_type", program_expr, path, physical)?;
            format!("{{ let __jet_inner_type = {child_ty}; {child} }}")
        }
        Kind::Union(members) => {
            let id = ty.identity.ok_or_else(|| BootstrapHostCodecError::InvalidMetadata("checked entry union has no nominal identity".to_string()))?;
            if members.is_empty() {
                return Err(BootstrapHostCodecError::InvalidMetadata("checked entry union has no members".to_string()));
            }
            format!("__jet_bootstrap_entry_type_{}_to_runtime({source}, checked, {program_expr}, {path}, {physical})?", id.0)
        }
    })
}

fn from_runtime_expression(
    program: &MirProgram,
    symbols: &BootstrapCodecSymbols<'_>,
    ty: &MirType,
    runtime: &str,
    checked: &str,
    program_expr: &str,
    path: &str,
) -> Result<String, BootstrapHostCodecError> {
    use MirTypeKind as Kind;
    let value = runtime;
    Ok(match ty.kind() {
        Kind::Int => format!("__jet_bootstrap_entry_integer_from_runtime({value})?"),
        Kind::IntN { signed, bits } => {
            let target = format!("{}{bits}", if *signed { 'i' } else { 'u' });
            format!("__jet_bootstrap_entry_integer_text({value})?.parse::<{target}>().map_err(|_| format!(\"checked fixed-width integer does not fit {target}\"))?")
        }
        Kind::Float => format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Float {{ value, f32: false }} => value, _ => return Err(\"checked Float result has invalid carrier\".to_string()) }}"),
        Kind::Float32 => format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Float {{ value, f32: true }} => value as f32, _ => return Err(\"checked Float32 result has invalid carrier\".to_string()) }}"),
        Kind::Measure(_) => format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Float {{ value, f32: false }} => value, _ => return Err(\"checked Measure result has invalid carrier\".to_string()) }}"),
        Kind::Bool => format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Bool(value) => value, _ => return Err(\"checked Bool result has invalid carrier\".to_string()) }}"),
        Kind::Char => format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Char(value) => value, _ => return Err(\"checked Char result has invalid carrier\".to_string()) }}"),
        Kind::String => format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::String(value) => value, _ => return Err(\"checked String result has invalid carrier\".to_string()) }}"),
        Kind::List(inner) | Kind::FixedList { elem: inner, .. } => {
            if is_u8_type(inner) {
                format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Bytes(bytes) => bytes, ::jet_foundation::MIR::MirRuntimeValue::List(items) => items.into_iter().map(|item| match item {{ ::jet_foundation::MIR::MirRuntimeValue::Int(value) => u8::try_from(value).map_err(|_| \"byte value is out of range\".to_string()), _ => Err(\"byte list contains a non-integer carrier\".to_string()) }}).collect::<Result<Vec<_>, String>>()?, _ => return Err(\"checked byte list result has invalid carrier\".to_string()) }}")
            } else {
                let kind = if matches!(ty.kind(), Kind::List(_)) { "list" } else { "fixed-list" };
                let child_type = format!("__jet_bootstrap_entry_child_type({checked}, \"{kind}\", 0)?");
                let child = from_runtime_expression(program, symbols, inner, "item", "&__jet_item_type", program_expr, path)?;
                if matches!(ty.kind(), Kind::List(_)) {
                    format!("{{ let __jet_item_type = {child_type}; match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::List(items) => items.into_iter().map(|item| Ok({child})).collect::<Result<_, String>>()?, _ => return Err(\"checked List result has invalid carrier\".to_string()) }} }}")
                } else {
                    format!("{{ let __jet_item_type = {child_type}; let items = match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::List(items) => items.into_iter().map(|item| Ok({child})).collect::<Result<Vec<_>, String>>()?, _ => return Err(\"checked FixedList result has invalid carrier\".to_string()) }}; items.try_into().map_err(|_| \"checked FixedList result has the wrong length\".to_string())? }}")
                }
            }
        }
        Kind::Map { key, value: item_ty } => {
            let key_type = "__jet_bootstrap_entry_child_type(checked, \"map\", 0)?";
            let item_type = "__jet_bootstrap_entry_child_type(checked, \"map\", 1)?";
            let decoded_key = from_const_key_expression(program, symbols, key, "key", "&__jet_key_type", program_expr, path)?;
            let decoded_item = from_runtime_expression(program, symbols, item_ty, "item", "&__jet_item_type", program_expr, path)?;
            format!("{{ let __jet_key_type = {key_type}; let __jet_item_type = {item_type}; match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Map(entries) => entries.into_iter().map(|(key, item)| Ok(({decoded_key}, {decoded_item}))).collect::<Result<_, String>>()?, _ => return Err(\"checked Map result has invalid carrier\".to_string()) }} }}")
        }
        Kind::Shared(_) | Kind::TraitObject(_) | Kind::Fn(_) | Kind::SendFn { .. } => {
            format!("physical.decode(path, checked, {value})?.ok_or_else(|| format!(\"NativeAdapter has no typed result projection for physical field `{{}}`\", path.join(\".\")))?")
        }
        Kind::Option(inner) => {
            let inner_type = "__jet_bootstrap_entry_child_type(checked, \"option\", 0)?";
            let child = from_runtime_expression(program, symbols, inner, "*inner", "&__jet_inner_type", program_expr, path)?;
            format!("{{ let __jet_inner_type = {inner_type}; match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Present(inner) => Ok({child}), ::jet_foundation::MIR::MirRuntimeValue::Absent {{ element }} if element.same_checked_type(__jet_inner_type) => Err(Default::default()), _ => return Err(\"checked Option result has invalid carrier\".to_string()) }} }}")
        }
        Kind::Result { ok, err } => {
            let ok_type = "__jet_bootstrap_entry_child_type(checked, \"result\", 0)?";
            let err_type = "__jet_bootstrap_entry_child_type(checked, \"result\", 1)?";
            let ok_value = from_runtime_expression(program, symbols, ok, "*inner", "&__jet_ok_type", program_expr, path)?;
            let err_value = from_runtime_expression(program, symbols, err, "*inner", "&__jet_err_type", program_expr, path)?;
            format!("{{ let __jet_ok_type = {ok_type}; let __jet_err_type = {err_type}; match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Present(inner) => Ok({ok_value}), ::jet_foundation::MIR::MirRuntimeValue::FailedTold(inner) => Err({err_value}), _ => return Err(\"checked Result result has invalid carrier\".to_string()) }} }}")
        }
        Kind::Apply { name, .. } => {
            checked_definition_by_id(program, name.id)?;
            format!("__jet_bootstrap_entry_type_{}_from_runtime({value}, checked, {program_expr}, {path}, physical)?", name.id.0)
        }
        Kind::Tuple(fields) => {
            if fields.is_empty() {
                format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Unit => (), _ => return Err(\"checked unit result has invalid carrier\".to_string()) }}")
            } else {
                let mut decoded = Vec::with_capacity(fields.len());
                for (index, (_, field_ty)) in fields.iter().enumerate() {
                    let field_type = format!("__jet_bootstrap_entry_child_type(checked, \"tuple\", {index})?");
                    let child = from_runtime_expression(program, symbols, field_ty, "field_value", &format!("&__jet_tuple_type_{index}"), program_expr, path)?;
                    decoded.push(format!("{{ let __jet_tuple_type_{index} = {field_type}; let field_value = __jet_bootstrap_entry_take_tuple_field(&mut fields, {index})?; {child} }}"));
                }
                format!("{{ let ::jet_foundation::MIR::MirRuntimeValue::Struct {{ type_name, fields: mut fields }} = {value} else {{ return Err(\"checked tuple result has invalid carrier\".to_string()) }}; if type_name != \"Tuple\" {{ return Err(\"checked tuple result has wrong nominal name\".to_string()) }} ({}) }}", decoded.join(", "))
            }
        }
        Kind::InlineRange { base, .. } | Kind::Tagged { inner: base, .. } | Kind::Quantity { base, .. } => {
            let kind = match ty.kind() { Kind::InlineRange { .. } => "range", Kind::Tagged { .. } => "tagged", _ => "quantity" };
            let inner_type = format!("__jet_bootstrap_entry_child_type(checked, \"{kind}\", 0)?");
            let child = from_runtime_expression(program, symbols, base, value, "__jet_inner_type", program_expr, path)?;
            format!("{{ let __jet_inner_type = {inner_type}; {child} }}")
        }
        Kind::Union(members) => {
            let id = ty.identity.ok_or_else(|| BootstrapHostCodecError::InvalidMetadata("checked entry union has no nominal identity".to_string()))?;
            if members.is_empty() {
                return Err(BootstrapHostCodecError::InvalidMetadata("checked entry union has no members".to_string()));
            }
            format!("__jet_bootstrap_entry_type_{}_from_runtime({value}, checked, {program_expr}, {path}, physical)?", id.0)
        }
    })
}

fn to_const_key_expression(
    program: &MirProgram,
    _symbols: &BootstrapCodecSymbols<'_>,
    ty: &MirType,
    source: &str,
    _checked: &str,
    _program_expr: &str,
    _path: &str,
) -> Result<String, BootstrapHostCodecError> {
    use MirTypeKind as Kind;
    let key = "::jet_foundation::MIR::MirConstKey";
    Ok(match ty.kind() {
        Kind::Int => format!("{key}::Int(({source}).to_i64().ok_or_else(|| \"integer map key exceeds i64\".to_string())?)"),
        Kind::IntN { .. } => format!("{key}::Int(({source}).to_string().parse::<i64>().map_err(|_| \"fixed-width integer map key exceeds i64\".to_string())?)"),
        Kind::String => format!("{key}::String(({source}).clone())"),
        Kind::Bool => format!("{key}::Bool(*({source}))"),
        Kind::Char => format!("{key}::Char(*({source}))"),
        Kind::Tagged { inner, .. } | Kind::Quantity { base: inner, .. } => {
            return to_const_key_expression(program, _symbols, inner, source, _checked, _program_expr, _path)
        }
        Kind::Apply { name, .. } => {
            let definition = checked_definition_by_id(program, name.id)?;
            match &definition.kind {
                MirTypeDefKind::Enum { variants, .. } if variants.iter().all(|variant| matches!(variant.payload, MirVariantPayload::Unit)) => {
                    let mut arms = Vec::new();
                    for variant in variants {
                        let path = _symbols.variant_path(&definition.name, &variant.name)?;
                        arms.push(format!("{path} => Ok({key}::Enum {{ type_name: {name:?}.to_string(), variant: {variant:?}.to_string() }}),", name=definition.name, variant=variant.name));
                    }
                    return Ok(format!("match {source} {{ {} }}", arms.join(" ")))
                }
                MirTypeDefKind::UnitFamily { members } => {
                    let mut arms = Vec::new();
                    for member in members {
                        let path = _symbols.variant_path(&definition.name, member)?;
                        arms.push(format!("{path} => Ok({key}::Enum {{ type_name: {name:?}.to_string(), variant: {member:?}.to_string() }}),", name=definition.name, member=member));
                    }
                    return Ok(format!("match {source} {{ {} }}", arms.join(" ")))
                }
                _ => return Err(BootstrapHostCodecError::InvalidMetadata(format!("entry map key type `{}` has no checked constant-key ABI", definition.name))),
            }
        }
        _ => return Err(BootstrapHostCodecError::InvalidMetadata(format!("entry map key type `{}` has no checked constant-key ABI", ty.canonical_key()))),
    })
}

fn from_const_key_expression(
    _program: &MirProgram,
    _symbols: &BootstrapCodecSymbols<'_>,
    ty: &MirType,
    key: &str,
    _checked: &str,
    _program_expr: &str,
    _path: &str,
) -> Result<String, BootstrapHostCodecError> {
    use MirTypeKind as Kind;
    Ok(match ty.kind() {
        Kind::Int => format!("match {key} {{ ::jet_foundation::MIR::MirConstKey::Int(value) => jet_foundation::Numeric::JetInt::from_i64(value), _ => return Err(\"map key does not match checked Int\".to_string()) }}"),
        Kind::IntN { signed, bits } => {
            let target=format!("{}{bits}",if *signed {'i'} else {'u'});
            format!("match {key} {{ ::jet_foundation::MIR::MirConstKey::Int(value) => {target}::try_from(value).map_err(|_| \"map key does not fit checked fixed width\".to_string())?, _ => return Err(\"map key does not match checked fixed width\".to_string()) }}")
        }
        Kind::String => format!("match {key} {{ ::jet_foundation::MIR::MirConstKey::String(value) => value, _ => return Err(\"map key does not match checked String\".to_string()) }}"),
        Kind::Bool => format!("match {key} {{ ::jet_foundation::MIR::MirConstKey::Bool(value) => value, _ => return Err(\"map key does not match checked Bool\".to_string()) }}"),
        Kind::Char => format!("match {key} {{ ::jet_foundation::MIR::MirConstKey::Char(value) => value, _ => return Err(\"map key does not match checked Char\".to_string()) }}"),
        Kind::Tagged { inner, .. } | Kind::Quantity { base: inner, .. } => return from_const_key_expression(_program, _symbols, inner, key, _checked, _program_expr, _path),
        _ => return Err(BootstrapHostCodecError::InvalidMetadata(format!("entry map key type `{}` has no checked source reconstruction",ty.canonical_key()))),
    })
}

fn is_u8_type(ty: &MirType) -> bool {
    matches!(ty.kind(), MirTypeKind::IntN { signed: false, bits: 8 })
}

fn field_program_placeholder() -> &'static MirProgram {
    // Kept only to make expression generation signatures uniform. The checked
    // MIR graph is supplied to the expression builders by their caller.
    static EMPTY: std::sync::OnceLock<MirProgram> = std::sync::OnceLock::new();
    EMPTY.get_or_init(|| MirProgram {
        cffi: Default::default(), schema_version: jet_foundation::MIR::MIR_SCHEMA_VERSION,
        package_identity: String::new(), facts: Default::default(), names: Default::default(),
        modules: Vec::new(), imports: Vec::new(), types: Vec::new(), traits: Vec::new(),
        core_owners: Vec::new(), impls: Vec::new(), constants: Vec::new(), fields: Vec::new(),
        source_files: Vec::new(), functions: Vec::new(), foreign: Vec::new(), links: Vec::new(),
        callbacks: Vec::new(), handles: Vec::new(), jobs: Vec::new(), tests: Vec::new(),
        harnesses: Vec::new(), artifacts: Vec::new(), core_calls: Vec::new(), prelude_calls: Vec::new(),
        type_instances: Vec::new(), codec_migrations: std::collections::HashMap::new(), unreachable: Vec::new(),
    })
}

fn validate_binding_graph(
    program: &MirProgram,
    bindings: &BootstrapBindingDescriptor,
    symbols: &BootstrapCodecSymbols<'_>,
    root: &MirType,
) -> Result<(), BootstrapHostCodecError> {
    let mut visited = HashSet::new();
    let mut stack = vec![root];
    while let Some(ty) = stack.pop() {
        match ty.kind() {
            MirTypeKind::List(inner) | MirTypeKind::Shared(inner) | MirTypeKind::Option(inner)
            | MirTypeKind::FixedList { elem: inner, .. } | MirTypeKind::InlineRange { base: inner, .. }
            | MirTypeKind::Tagged { inner, .. } | MirTypeKind::Quantity { base: inner, .. } => stack.push(inner),
            MirTypeKind::Map { key, value } | MirTypeKind::Result { ok: key, err: value } => { stack.push(key); stack.push(value); }
            MirTypeKind::Fn(signature) => { stack.extend(signature.params.iter()); if let Some(ret)=signature.ret.as_deref(){stack.push(ret);} }
            MirTypeKind::SendFn { params, ret, .. } => { stack.extend(params.iter()); if let Some(ret)=ret.as_deref(){stack.push(ret);} }
            MirTypeKind::Tuple(fields) => stack.extend(fields.iter().map(|(_,ty)|ty)),
            MirTypeKind::Union(members) => stack.extend(members.iter()),
            MirTypeKind::Apply { name, args } => {
                stack.extend(args.iter());
                if !visited.insert(name.id) { continue; }
                let definition=checked_definition_by_id(program,name.id)?;
                let _=symbols.type_symbol(&definition.name)?;
                match &definition.kind {
                    MirTypeDefKind::Struct { fields, .. } => for field in fields {
                        let row=symbols.field_binding(&definition.name,&field.name)?;
                        if row.field!=field.id || row.owner!=definition.id || !row.ty.same_checked_type(&field.ty) {
                            return Err(BootstrapHostCodecError::InvalidMetadata(format!("checked field binding drift for `{}.{}`",definition.name,field.name)));
                        }
                        stack.push(&field.ty);
                    },
                    MirTypeDefKind::Enum { variants, .. } => for variant in variants {
                        let _=symbols.variant_path(&definition.name,&variant.name)?;
                        match &variant.payload {
                            MirVariantPayload::Unit=>{},
                            MirVariantPayload::Single(ty)=>stack.push(ty),
                            MirVariantPayload::Named(fields)=>for field in fields {
                                let row=symbols.field_binding(&definition.name,&field.name)?;
                                if row.field!=field.id || row.owner!=definition.id || !row.ty.same_checked_type(&field.ty) {
                                    return Err(BootstrapHostCodecError::InvalidMetadata(format!("checked enum payload binding drift for `{}::{}.{}`",definition.name,variant.name,field.name)));
                                }
                                stack.push(&field.ty);
                            },
                        }
                    },
                    MirTypeDefKind::Distinct { base, .. } | MirTypeDefKind::Alias { target: base }=>stack.push(base),
                    MirTypeDefKind::UnitFamily { .. }=>{},
                }
            }
            _=>{}
        }
    }
    let _=bindings;
    Ok(())
}

fn collect_nominal_types<'a>(
    program:&'a MirProgram,
    root:&MirType,
    visited:&mut HashSet<MirTypeId>,
    output:&mut Vec<&'a MirTypeDef>,
)->Result<(),BootstrapHostCodecError>{
    let mut stack=vec![root];
    while let Some(ty)=stack.pop(){
        match ty.kind(){
            MirTypeKind::List(inner)|MirTypeKind::Shared(inner)|MirTypeKind::Option(inner)|MirTypeKind::FixedList{elem:inner,..}|MirTypeKind::InlineRange{base:inner,..}|MirTypeKind::Tagged{inner,..}|MirTypeKind::Quantity{base:inner,..}=>stack.push(inner),
            MirTypeKind::Map{key,value}|MirTypeKind::Result{ok:key,err:value}=>{stack.push(key);stack.push(value);},
            MirTypeKind::Fn(signature)=>{stack.extend(signature.params.iter());if let Some(ret)=signature.ret.as_deref(){stack.push(ret);}},
            MirTypeKind::SendFn{params,ret,..}=>{stack.extend(params.iter());if let Some(ret)=ret.as_deref(){stack.push(ret);}},
            MirTypeKind::Tuple(fields)=>stack.extend(fields.iter().map(|(_,ty)|ty)),
            MirTypeKind::Union(members)=>stack.extend(members.iter()),
            MirTypeKind::Apply{name,args}=>{
                stack.extend(args.iter());
                if visited.insert(name.id){
                    let def=checked_definition_by_id(program,name.id)?;output.push(def);
                    match &def.kind{
                        MirTypeDefKind::Struct{fields,..}=>stack.extend(fields.iter().map(|field|&field.ty)),
                        MirTypeDefKind::Enum{variants,..}=>for variant in variants{match &variant.payload{MirVariantPayload::Unit=>{},MirVariantPayload::Single(ty)=>stack.push(ty),MirVariantPayload::Named(fields)=>stack.extend(fields.iter().map(|field|&field.ty)),}},
                        MirTypeDefKind::Distinct{base,..}|MirTypeDefKind::Alias{target:base}=>stack.push(base),
                        MirTypeDefKind::UnitFamily{..}=>{},
                    }
                }
            },
            _=>{},
        }
    }
    Ok(())
}

fn checked_nominal_definition<'a>(program:&'a MirProgram,ty:&MirType)->Result<&'a MirTypeDef,BootstrapHostCodecError>{
    let id=ty.nominal_id().ok_or_else(||BootstrapHostCodecError::InvalidMetadata(format!("entry type `{}` is not checked nominal",ty.canonical_key())))?;
    checked_definition_by_id(program,id)
}
fn checked_definition_by_id<'a>(program:&'a MirProgram,id:MirTypeId)->Result<&'a MirTypeDef,BootstrapHostCodecError>{
    program.types.iter().find(|definition|definition.id==id).ok_or_else(||BootstrapHostCodecError::MissingType(format!("MIR type ID {}",id.0)))
}

fn checked_record_field<'a>(program:&MirProgram,ty:&MirType,value:&'a MirRuntimeValue,id:MirFieldId)->Result<Option<&'a MirRuntimeValue>,String>{
    let definition=ty.nominal_id().and_then(|id|program.types.iter().find(|d|d.id==id)).ok_or_else(||"checked record type is absent".to_string())?;
    let MirTypeDefKind::Struct{fields,..}=&definition.kind else{return Err(format!("checked type `{}` is not a record",definition.name));};
    let field=fields.iter().find(|field|field.id==id).ok_or_else(||format!("checked field ID {} is absent",id.0))?;
    let MirRuntimeValue::Struct{type_name,fields:values}=value else{return Err(format!("result `{}` is not a record",definition.name));};
    if type_name!=&definition.name || values.len()!=fields.len(){return Err(format!("result record `{}` is incomplete or mistyped",definition.name));}
    for (actual,expected) in values.iter().zip(fields){if actual.0!=expected.name{return Err(format!("result record `{}` has field order mismatch",definition.name));}}
    Ok(values.iter().find(|(name,_)|name==&field.name).map(|(_,value)|value))
}

pub(crate) fn validate_runtime_value(program:&MirProgram,ty:&MirType,value:&MirRuntimeValue,depth:usize)->Result<(),String>{
    use MirRuntimeValue as V;use MirTypeKind as K;
    if depth>=MAX_VALUE_DEPTH{return Err("compiler-entry value exceeds checked nesting bound".to_string());}
    match ty.kind(){
        K::Int|K::IntN{..}|K::Measure(_)=>validate_integer(ty,value),
        K::Float=>if matches!(value,V::Float{f32:false,..}){Ok(())}else{Err(mismatch(ty,"f64"))},
        K::Float32=>if matches!(value,V::Float{f32:true,..}){Ok(())}else{Err(mismatch(ty,"f32"))},
        K::Bool=>if matches!(value,V::Bool(_)){Ok(())}else{Err(mismatch(ty,"Bool"))},
        K::Char=>if matches!(value,V::Char(_)){Ok(())}else{Err(mismatch(ty,"Char"))},
        K::String=>if matches!(value,V::String(_)){Ok(())}else{Err(mismatch(ty,"String"))},
        K::List(inner)=>validate_sequence(program,ty,inner,value,None,depth),
        K::FixedList{elem,len}=>validate_sequence(program,ty,elem,value,len.literal_value(),depth),
        K::Map{key,value:item_ty}=>{
            let V::Map(entries)=value else{return Err(mismatch(ty,"Map"));};let mut seen=BTreeSet::new();
            for (key_value,item) in entries{validate_const_key(program,key,key_value,depth+1)?;if !seen.insert(key_value){return Err("compiler-entry map has duplicate keys".to_string());}validate_runtime_value(program,item_ty,item,depth+1)?;}Ok(())
        },
        K::Shared(_)|K::TraitObject(_)|K::Fn(_)|K::SendFn{..}=>if matches!(value,V::NativeOwned(_)){Ok(())}else{Err(mismatch(ty,"registered native-owned physical carrier"))},
        K::Option(inner)=>match value{V::Present(value)=>validate_runtime_value(program,inner,value,depth+1),V::Absent{element} if element.same_checked_type(inner)=>Ok(()),V::Absent{..}=>Err("absent carrier changed its exact checked element type".to_string()),_=>Err(mismatch(ty,"Option"))},
        K::Result{ok,err}=>match value{V::Present(value)=>validate_runtime_value(program,ok,value,depth+1),V::FailedTold(value)=>validate_runtime_value(program,err,value,depth+1),_=>Err(mismatch(ty,"Result"))},
        K::Apply{name,..}=>{let def=program.types.iter().find(|d|d.id==name.id).ok_or_else(||format!("checked type `{}` is absent",name.name))?;validate_nominal(program,def,ty,value,depth+1)},
        K::Tuple(fields)=>{
            if fields.is_empty(){return if matches!(value,V::Unit){Ok(())}else{Err(mismatch(ty,"Unit"))};}
            let V::Struct{type_name,fields:values}=value else{return Err(mismatch(ty,"Tuple"));};
            if type_name!="Tuple"{return Err(mismatch(ty,"Tuple"));}
            validate_named_values(program,ty,&fields.iter().map(|(name,ty)|(name.as_str(),ty)).collect::<Vec<_>>(),values,depth+1)
        },
        K::InlineRange{base,lo,hi}=>{validate_runtime_value(program,base,value,depth+1)?;let number=integer_as_i128(value).ok_or_else(||mismatch(ty,"integer range"))?;if number<i128::from(*lo)||number>i128::from(*hi){return Err(format!("value is outside checked range `{}`",ty.canonical_key()));}Ok(())},
        K::Tagged{inner,..}|K::Quantity{base:inner,..}=>validate_runtime_value(program,inner,value,depth+1),
        K::Union(members)=>{let mut last=None;for member in members{match validate_runtime_value(program,member,value,depth+1){Ok(())=>return Ok(()),Err(err)=>last=Some(err)}}Err(last.unwrap_or_else(||mismatch(ty,"union member")))},
    }
}
fn validate_sequence(program:&MirProgram,ty:&MirType,elem:&MirType,value:&MirRuntimeValue,expected:Option<u64>,depth:usize)->Result<(),String>{
    let count=match value{MirRuntimeValue::Bytes(bytes) if is_u8_type(elem)=>bytes.len(),MirRuntimeValue::List(values)=>{for item in values{validate_runtime_value(program,elem,item,depth+1)?;}values.len()},_=>return Err(mismatch(ty,"sequence"))};
    if expected.is_some_and(|expected|usize::try_from(expected).ok()!=Some(count)){return Err(format!("sequence `{}` has incorrect fixed length",ty.canonical_key()));}Ok(())
}
fn validate_nominal(program:&MirProgram,def:&MirTypeDef,ty:&MirType,value:&MirRuntimeValue,depth:usize)->Result<(),String>{
    use MirRuntimeValue as V;
    match &def.kind{
        MirTypeDefKind::Struct{fields,..}=>{let V::Struct{type_name,fields:values}=value else{return Err(mismatch(ty,"record"));};if type_name!=&def.name{return Err(mismatch(ty,"exact record name"));}validate_named_values(program,ty,&fields.iter().map(|field|(field.name.as_str(),&field.ty)).collect::<Vec<_>>(),values,depth+1)},
        MirTypeDefKind::Enum{variants,..}=>{let V::Enum{type_name,variant,args}=value else{return Err(mismatch(ty,"enum"));};if type_name!=&def.name{return Err(mismatch(ty,"exact enum name"));}let variant=variants.iter().find(|v|v.name==*variant).ok_or_else(||format!("unknown checked variant `{}`",variant))?;let expected=match &variant.payload{MirVariantPayload::Unit=>Vec::new(),MirVariantPayload::Single(ty)=>vec![(None,ty)],MirVariantPayload::Named(fields)=>fields.iter().map(|f|(Some(f.name.as_str()),&f.ty)).collect()};if args.len()!=expected.len(){return Err("checked enum payload arity mismatch".to_string());}for ((name,ty),(actual,value)) in expected.into_iter().zip(args){if name!=actual.as_deref(){return Err("checked enum payload name/order mismatch".to_string());}validate_runtime_value(program,ty,value,depth+1)?;}Ok(())},
        MirTypeDefKind::Distinct{base,..}|MirTypeDefKind::Alias{target:base}=>validate_runtime_value(program,base,value,depth+1),
        MirTypeDefKind::UnitFamily{members}=>match value{V::Enum{type_name,variant,args} if type_name==&def.name&&members.iter().any(|m|m==variant)&&args.is_empty()=>Ok(()),_=>Err(mismatch(ty,"unit-family variant"))},
    }
}
fn validate_named_values(program:&MirProgram,ty:&MirType,declared:&[(&str,&MirType)],values:&[(String,MirRuntimeValue)],depth:usize)->Result<(),String>{
    if declared.len()!=values.len(){return Err(format!("aggregate `{}` has missing/extra fields",ty.canonical_key()));}let mut seen=HashSet::with_capacity(values.len());
    for ((expected,expected_ty),(actual,value)) in declared.iter().zip(values){if !seen.insert(actual.as_str())||actual!=expected{return Err(format!("aggregate `{}` field names/order are invalid",ty.canonical_key()));}validate_runtime_value(program,expected_ty,value,depth+1)?;}Ok(())
}
fn validate_const_key(program:&MirProgram,ty:&MirType,key_ty:&MirConstKey,depth:usize)->Result<(),String>{
    if depth>=MAX_VALUE_DEPTH{return Err("map key exceeds checked nesting bound".to_string());}
    match (ty.kind(),key_ty){
        (MirTypeKind::Int|MirTypeKind::IntN{..}|MirTypeKind::Measure(_),MirConstKey::Int(_))|(MirTypeKind::String,MirConstKey::String(_))|(MirTypeKind::Bool,MirConstKey::Bool(_))|(MirTypeKind::Char,MirConstKey::Char(_))=>Ok(()),
        (MirTypeKind::Tuple(types),MirConstKey::Tuple(values)) if types.len()==values.len()=>{for ((name,ty),(actual,key)) in types.iter().zip(values){if name!=actual{return Err("tuple map key names do not match checked type".to_string());}validate_const_key(program,ty,key,depth+1)?;}Ok(())},
        (MirTypeKind::Apply{name,..},MirConstKey::Struct{type_name,fields})=>{let def=program.types.iter().find(|d|d.id==name.id).ok_or_else(||"checked map key type is missing".to_string())?;let MirTypeDefKind::Struct{fields:declared,..}=&def.kind else{return Err("map key is not a checked struct".to_string());};if type_name!=&def.name||declared.len()!=fields.len(){return Err("struct key differs from checked nominal type".to_string());}for (field,(name,key)) in declared.iter().zip(fields){if field.name!=*name{return Err("struct key field order/name mismatch".to_string());}validate_const_key(program,&field.ty,key,depth+1)?;}Ok(())},
        (MirTypeKind::Apply{name,..},MirConstKey::Enum{type_name,variant})=>{let def=program.types.iter().find(|d|d.id==name.id).ok_or_else(||"checked map key type is missing".to_string())?;if type_name!=&def.name{return Err("enum key name mismatch".to_string());}match &def.kind{MirTypeDefKind::Enum{variants,..} if variants.iter().any(|row|row.name==*variant&&matches!(row.payload,MirVariantPayload::Unit))=>Ok(()),MirTypeDefKind::UnitFamily{members} if members.iter().any(|member|member==variant)=>Ok(()),_=>Err("map enum key is not a checked unit variant".to_string())}},
        (MirTypeKind::Tagged{inner,..}|MirTypeKind::Quantity{base:inner,..},_)=>validate_const_key(program,inner,key_ty,depth+1),
        _=>Err(format!("map key does not match checked type `{}`",ty.canonical_key())),
    }
}
fn validate_integer(ty:&MirType,value:&MirRuntimeValue)->Result<(),String>{
    let text=match value{MirRuntimeValue::Int(v)=>v.to_string(),MirRuntimeValue::BigInt(v)=>v.clone(),_=>return Err(mismatch(ty,"exact integer"))};
    let exact=jet_foundation::Numeric::CtBigInt::from_str(&text).map_err(|_|"invalid exact integer carrier".to_string())?;
    if exact.to_string_rep()!=text{return Err("integer carrier is not canonical".to_string());}
    if let MirTypeKind::IntN{signed,bits}=ty.kind(){if *bits==0{return Err("fixed-width integer has zero bits".to_string());}let binary=exact.abs().to_radix(2).map_err(|e|e.to_string())?;let width=binary.len();let fits=if *signed{if exact.is_zero(){true}else if text.starts_with('-'){width<usize::from(bits.saturating_sub(1))||(width==usize::from(*bits)&&binary==format!("1{}","0".repeat(usize::from(bits.saturating_sub(1)))))}else{width<=usize::from(bits.saturating_sub(1))}}else{!text.starts_with('-')&&width<=usize::from(*bits)};if !fits{return Err(format!("integer is outside checked {}{} range",if *signed{'I'}else{'U'},bits));}}Ok(())
}
fn integer_as_i128(value:&MirRuntimeValue)->Option<i128>{match value{MirRuntimeValue::Int(v)=>Some(i128::from(*v)),MirRuntimeValue::BigInt(v)=>jet_foundation::Numeric::CtBigInt::from_str(v).ok()?.try_i128(),_=>None}}
fn is_u8_type(ty:&MirType)->bool{matches!(ty.kind(),MirTypeKind::IntN{signed:false,bits:8})}
fn mismatch(ty:&MirType,expected:&str)->String{format!("runtime value does not match checked `{}` ({expected})",ty.canonical_key())}
