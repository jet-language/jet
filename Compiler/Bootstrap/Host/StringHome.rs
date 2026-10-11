//! Original Source-machine String getters. Seed/verification only; dies at cutover.
//! A READ String decoded by the generic value codec is NOT a loan: it is freed
//! before the subsequent foreign byte-span call. Keep the borrowed machine root.
use super::super::{BootstrapCodecSymbols, BootstrapHostCodecError};
use crate::Codegen::MIRRust::MirRustTraitMethodMetadata;
use std::fmt::Write as _;

pub(super) fn emit_string_home_operations(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
    methods: &[&MirRustTraitMethodMetadata],
) -> Result<(), BootstrapHostCodecError> {
    out.push_str(&symbols.render(r#"
fn __jet_bootstrap_string_home_data<'a>(value: &'a @t.TComptimeValue@, selectors: &[@t.JetEvalStringHomeSelector@]) -> Option<&'a String> {
    if selectors.is_empty() {
        return match value { @p.TComptimeValue.String@{value} => Some(value), _ => None };
    }
    let (selector, rest) = selectors.split_first()?;
    match (value, selector) {
        (@p.TComptimeValue.Struct@{type_name: _, fields}, @p.JetEvalStringHomeSelector.Field@{index}) =>
            __jet_bootstrap_string_home_data(&fields.get(index.to_usize()?)?.@f.TComptimeField.value@, rest),
        (@p.TComptimeValue.List@{values}, @p.JetEvalStringHomeSelector.List@{index}) =>
            __jet_bootstrap_string_home_data(values.get(index.to_usize()?)?, rest),
        (@p.TComptimeValue.Map@{entries}, @p.JetEvalStringHomeSelector.Map@{index}) =>
            __jet_bootstrap_string_home_data(&entries.get(index.to_usize()?)?.@f.TComptimeMapEntry.value@, rest),
        (@p.TComptimeValue.Enum@{type_name: _, variant: _, args}, @p.JetEvalStringHomeSelector.Enum@{index}) =>
            __jet_bootstrap_string_home_data(&args.get(index.to_usize()?)?.@f.TComptimeEnumArg.value@, rest),
        (@p.TComptimeValue.Present@{value}, @v.JetEvalStringHomeSelector.Present@) => __jet_bootstrap_string_home_data(value, rest),
        (@p.TComptimeValue.Failed@{report}, @v.JetEvalStringHomeSelector.Told@) => {
            let report: &@t.TComptimeReport@ = report;
            match report {
                @p.TComptimeReport.Told@{value} => __jet_bootstrap_string_home_data(value, rest),
                _ => None,
            }
        },
        _ => None,
    }
}
fn __jet_bootstrap_string_home_runtime<'a>(value: &'a @t.JetEvalRuntimeValue@, selectors: &[@t.JetEvalStringHomeSelector@]) -> Option<&'a String> {
    if let @p.JetEvalRuntimeValue.Data@{value} = value {
        return __jet_bootstrap_string_home_data(value, selectors);
    }
    let (selector, rest) = selectors.split_first()?;
    match (value, selector) {
        (@p.JetEvalRuntimeValue.Result@{ok, value}, @p.JetEvalStringHomeSelector.Result@{ok: expected}) if ok == expected =>
            __jet_bootstrap_string_home_runtime(value, rest),
        (@p.JetEvalRuntimeValue.Aggregate@{value}, selector) => {
            let value: &@t.JetEvalRuntimeAggregate@ = value;
            match (value, selector) {
            (@p.JetEvalRuntimeAggregate.Struct@{type_name: _, fields}, @p.JetEvalStringHomeSelector.Field@{index}) =>
                __jet_bootstrap_string_home_runtime(&fields.get(index.to_usize()?)?.@f.JetEvalRuntimeField.value@, rest),
            (@p.JetEvalRuntimeAggregate.List@{values}, @p.JetEvalStringHomeSelector.List@{index}) =>
                __jet_bootstrap_string_home_runtime(values.get(index.to_usize()?)?, rest),
            (@p.JetEvalRuntimeAggregate.Map@{entries}, @p.JetEvalStringHomeSelector.Map@{index}) =>
                __jet_bootstrap_string_home_runtime(&entries.get(index.to_usize()?)?.@f.JetEvalRuntimeMapEntry.value@, rest),
            (@p.JetEvalRuntimeAggregate.Enum@{type_name: _, variant: _, args}, @p.JetEvalStringHomeSelector.Enum@{index}) =>
                __jet_bootstrap_string_home_runtime(&args.get(index.to_usize()?)?.@f.JetEvalRuntimeEnumArg.value@, rest),
            _ => None,
            }
        },
        _ => None,
    }
}
fn __jet_bootstrap_string_home_typed<'a>(machine: &'a @t.JetEvalMachine@, request: &@t.JetEvalStringHomeRequest@) -> Option<&'a String> {
    let selectors = &request.@f.JetEvalStringHomeRequest.selectors@;
    match &request.@f.JetEvalStringHomeRequest.root@ {
        @p.JetEvalStringHomeRoot.Global@{name} => __jet_bootstrap_string_home_data(machine.@f.JetEvalMachine.config@.@f.JetEvalConfig.globals@.get(name)?, selectors),
        @p.JetEvalStringHomeRoot.Local@{frame, local} => __jet_bootstrap_string_home_runtime(machine.@f.JetEvalMachine.frames@.get(frame.to_usize()?)?.@f.JetEvalFrame.locals@.get(local)?, selectors),
        @p.JetEvalStringHomeRoot.Value@{frame, value} => __jet_bootstrap_string_home_runtime(machine.@f.JetEvalMachine.frames@.get(frame.to_usize()?)?.@f.JetEvalFrame.values@.get(value)?, selectors),
        @p.JetEvalStringHomeRoot.Parameter@{frame, index} => __jet_bootstrap_string_home_runtime(machine.@f.JetEvalMachine.frames@.get(frame.to_usize()?)?.@f.JetEvalFrame.params@.get(index.to_usize()?)?, selectors),
        @p.JetEvalStringHomeRoot.Capture@{frame, index} => __jet_bootstrap_string_home_runtime(machine.@f.JetEvalMachine.frames@.get(frame.to_usize()?)?.@f.JetEvalFrame.captures@.get(index.to_usize()?)?, selectors),
    }
}
// These views are projections of the already shape-checked borrowed Source
// machine, not generic decoding. Every returned reference retains that root.
fn __jet_bootstrap_string_home_field<'a>(value: &'a ::jet_foundation::MIR::MirRuntimeValue, name: &str) -> Option<&'a ::jet_foundation::MIR::MirRuntimeValue> {
    let ::jet_foundation::MIR::MirRuntimeValue::Struct { fields, .. } = value else { return None; };
    fields.iter().find(|(field, _)| field == name).map(|(_, value)| value)
}
fn __jet_bootstrap_string_home_payload<'a>(value: &'a ::jet_foundation::MIR::MirRuntimeValue, expected: &str, name: &str) -> Option<&'a ::jet_foundation::MIR::MirRuntimeValue> {
    let ::jet_foundation::MIR::MirRuntimeValue::Enum { variant, args, .. } = value else { return None; };
    if variant.rsplit([':', '.']).next()? != expected { return None; }
    args.iter().find(|(field, _)| field.as_deref() == Some(name)).map(|(_, value)| value)
}
fn __jet_bootstrap_string_home_list(value: &::jet_foundation::MIR::MirRuntimeValue, index: usize) -> Option<&::jet_foundation::MIR::MirRuntimeValue> {
    let ::jet_foundation::MIR::MirRuntimeValue::List(values) = value else { return None; };
    values.get(index)
}
fn __jet_bootstrap_string_home_word_map(value: &::jet_foundation::MIR::MirRuntimeValue, key: u64) -> Option<&::jet_foundation::MIR::MirRuntimeValue> {
    let ::jet_foundation::MIR::MirRuntimeValue::Map(values) = value else { return None; };
    values.iter().find(|(actual, _)| matches!(actual, ::jet_foundation::MIR::MirConstKey::Int(word) if *word == key as i64)).map(|(_, value)| value)
}
fn __jet_bootstrap_string_home_mir_walk<'a>(mut value: &'a ::jet_foundation::MIR::MirRuntimeValue, selectors: &[@t.JetEvalStringHomeSelector@]) -> Option<&'a String> {
    if let Some(data) = __jet_bootstrap_string_home_payload(value, "Data", "value") { value = data; }
    if let Some(aggregate) = __jet_bootstrap_string_home_payload(value, "Aggregate", "value") { value = aggregate; }
    if selectors.is_empty() {
        let ::jet_foundation::MIR::MirRuntimeValue::String(text) = __jet_bootstrap_string_home_payload(value, "String", "value")? else { return None; };
        return Some(text);
    }
    let (selector, rest) = selectors.split_first()?;
    let child = match selector {
        @p.JetEvalStringHomeSelector.Field@{index} => __jet_bootstrap_string_home_field(__jet_bootstrap_string_home_list(__jet_bootstrap_string_home_payload(value, "Struct", "fields")?, index.to_usize()?)?, "value")?,
        @p.JetEvalStringHomeSelector.List@{index} => __jet_bootstrap_string_home_list(__jet_bootstrap_string_home_payload(value, "List", "values")?, index.to_usize()?)?,
        @p.JetEvalStringHomeSelector.Map@{index} => __jet_bootstrap_string_home_field(__jet_bootstrap_string_home_list(__jet_bootstrap_string_home_payload(value, "Map", "entries")?, index.to_usize()?)?, "value")?,
        @p.JetEvalStringHomeSelector.Enum@{index} => __jet_bootstrap_string_home_field(__jet_bootstrap_string_home_list(__jet_bootstrap_string_home_payload(value, "Enum", "args")?, index.to_usize()?)?, "value")?,
        @v.JetEvalStringHomeSelector.Present@ => __jet_bootstrap_string_home_payload(value, "Present", "value")?,
        @v.JetEvalStringHomeSelector.Told@ => __jet_bootstrap_string_home_payload(__jet_bootstrap_string_home_payload(value, "Failed", "report")?, "Told", "value")?,
        @p.JetEvalStringHomeSelector.Result@{ok: expected} => {
            let ::jet_foundation::MIR::MirRuntimeValue::Bool(actual) = __jet_bootstrap_string_home_payload(value, "Result", "ok")? else { return None; };
            if actual != expected { return None; }
            __jet_bootstrap_string_home_payload(value, "Result", "value")?
        },
    };
    __jet_bootstrap_string_home_mir_walk(child, rest)
}
fn __jet_bootstrap_string_home_mir<'a>(machine: &'a ::jet_foundation::MIR::MirRuntimeValue, request: &@t.JetEvalStringHomeRequest@) -> Option<&'a String> {
    let selectors = &request.@f.JetEvalStringHomeRequest.selectors@;
    let value = match &request.@f.JetEvalStringHomeRequest.root@ {
        @p.JetEvalStringHomeRoot.Global@{name} => {
            let globals = __jet_bootstrap_string_home_field(__jet_bootstrap_string_home_field(machine, "config")?, "globals")?;
            let ::jet_foundation::MIR::MirRuntimeValue::Map(values) = globals else { return None; };
            values.iter().find(|(key, _)| matches!(key, ::jet_foundation::MIR::MirConstKey::String(actual) if actual == name)).map(|(_, value)| value)?
        },
        @p.JetEvalStringHomeRoot.Local@{frame, local} => __jet_bootstrap_string_home_word_map(__jet_bootstrap_string_home_field(__jet_bootstrap_string_home_list(__jet_bootstrap_string_home_field(machine, "frames")?, frame.to_usize()?)?, "locals")?, *local)?,
        @p.JetEvalStringHomeRoot.Value@{frame, value} => __jet_bootstrap_string_home_word_map(__jet_bootstrap_string_home_field(__jet_bootstrap_string_home_list(__jet_bootstrap_string_home_field(machine, "frames")?, frame.to_usize()?)?, "values")?, *value)?,
        @p.JetEvalStringHomeRoot.Parameter@{frame, index} => __jet_bootstrap_string_home_list(__jet_bootstrap_string_home_field(__jet_bootstrap_string_home_list(__jet_bootstrap_string_home_field(machine, "frames")?, frame.to_usize()?)?, "params")?, index.to_usize()?)?,
        @p.JetEvalStringHomeRoot.Capture@{frame, index} => __jet_bootstrap_string_home_list(__jet_bootstrap_string_home_field(__jet_bootstrap_string_home_list(__jet_bootstrap_string_home_field(machine, "frames")?, frame.to_usize()?)?, "captures")?, index.to_usize()?)?,
    };
    __jet_bootstrap_string_home_mir_walk(value, selectors)
}
"#)?);
    for metadata in methods {
        let body = match metadata.name.as_str() {
            "string_home" => symbols.render(r#"let text = match machine {
                crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineAccess::Typed(machine) => __jet_bootstrap_string_home_typed(machine, __jet_arg_1),
                crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineAccess::Mir(machine) => __jet_bootstrap_string_home_mir(machine.value(), __jet_arg_1),
            };
            match text {
                Some(text) => Ok(@s.JetEvalStringHome@{pointer: crate::jet_mem::jet_string_data(text), length: jet_foundation::Numeric::JetInt::from_i64(crate::jet_mem::jet_string_byte_len(text))}),
                None => Err(Default::default()),
            }"#)?,
            "bytes_pack" => "Ok(crate::jet_mem::jet_bytes_pack(__jet_arg_1))".into(),
            "bytes_free" => "crate::jet_mem::jet_bytes_free(__jet_arg_1, &__jet_arg_2.to_raw()); true".into(),
            "bytes_unpack" => "Ok(crate::jet_mem::jet_bytes_unpack(__jet_arg_1, &__jet_arg_2.to_raw()))".into(),
            _ => continue,
        };
        let parameters = metadata.parameter_types.iter().enumerate().skip(1)
            .map(|(index, ty)| format!(", __jet_arg_{index}: {ty}")).collect::<String>();
        let return_type = if metadata.name == "bytes_free" { "bool" } else { &metadata.return_type };
        writeln!(out, "fn __jet_bootstrap_native_{}(machine: &mut crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineAccess<'_, @t.JetEvalMachine@>{parameters}) -> {return_type} {{ {body} }}", metadata.name)
            .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    }
    Ok(())
}
