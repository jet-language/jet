//! Typed CLI entry marshalling for the canonical TIR interpreter.
//!
//! The parser and its error/default rules live in the embedded Args Prelude.
//! This module only turns the checked `CLISchema` projection into Prelude
//! handles and turns the resulting scalar values into `CtValue`s.

use crate::AST::{CtReport, CtValue, EnumDef, Item, StructDef, Type, VariantPayload};
use jet_foundation::CLISchema::{
    CLIDefault, CLICommandSchema, CLIInputSchema, CLIInputShape, CLIValueKind,
};
use crate::Comptime;
use crate::Diagnostics::{Diagnostic, Span};

use super::unsupported;

pub(super) enum Dispatch {
    Run(CtValue),
    Help(String),
    Error(String),
}

fn args_call(
    op: &str,
    recv: &mut CtValue,
    args: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let mut args = args;
    Comptime::eval_args_handle(op, recv, &mut args, span)
        .ok_or_else(|| unsupported("typed CLI args", span))?
}

fn optional_text(value: Option<&str>) -> CtValue {
    value.map_or(CtValue::Unit, |value| CtValue::Str(value.to_string()))
}

fn program_name(argv0: &str) -> String {
    let name = std::path::Path::new(argv0)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(argv0);
    name.strip_suffix(".jet").unwrap_or(name).to_string()
}

fn value_kind(kind: CLIValueKind) -> &'static str {
    match kind {
        CLIValueKind::Int => "Int",
        CLIValueKind::Float => "Float",
        CLIValueKind::Bool | CLIValueKind::String | CLIValueKind::Path => "String",
    }
}

fn add_inputs(
    mut spec: CtValue,
    inputs: &[CLIInputSchema],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    for input in inputs {
        match &input.shape {
            CLIInputShape::Flag => {
                let op = if input.short.is_some() {
                    "ArgsSpecFlagShort"
                } else {
                    "ArgsSpecFlag"
                };
                let args = if let Some(short) = input.short.as_deref() {
                    vec![
                        CtValue::Str(input.flag.clone()),
                        CtValue::Str(short.to_string()),
                        CtValue::Str(input.builder_help()),
                    ]
                } else {
                    vec![
                        CtValue::Str(input.flag.clone()),
                        CtValue::Str(input.builder_help()),
                    ]
                };
                spec = args_call(op, &mut spec, args, span)?;
            }
            CLIInputShape::Value { .. } => {
                spec = args_call(
                    "ArgsSpecOptionBase",
                    &mut spec,
                    vec![
                        CtValue::Str(input.flag.clone()),
                        optional_text(input.short.as_deref()),
                        CtValue::Str(input.builder_help()),
                        CtValue::Str(input.metavar.clone().unwrap_or_else(|| "VALUE".to_string())),
                        CtValue::Unit,
                        optional_text(input.env.as_deref()),
                        CtValue::Bool(false),
                        CtValue::Bool(false),
                        CtValue::Str(value_kind(input.value_kind()).to_string()),
                    ],
                    span,
                )?;
                if input.positional.is_some() {
                    spec = args_call(
                        "ArgsSpecPositional",
                        &mut spec,
                        vec![
                            CtValue::Str(input.flag.clone()),
                            CtValue::Str(input.builder_help()),
                        ],
                        span,
                    )?;
                }
            }
        }
    }
    Ok(spec)
}

fn build_spec(
    inputs: &[CLIInputSchema],
    description: Option<&str>,
    argv0: &str,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let mut spec = Comptime::core_args_spec();
    spec = args_call(
        "ArgsSpecProgram",
        &mut spec,
        vec![CtValue::Str(program_name(argv0))],
        span,
    )?;
    if let Some(description) = description {
        spec = args_call(
            "ArgsSpecDescription",
            &mut spec,
            vec![CtValue::Str(description.to_string())],
            span,
        )?;
    }
    add_inputs(spec, inputs, span)
}

fn build_command_spec(
    schema: &CLICommandSchema,
    argv0: &str,
    span: Span,
) -> Result<(CtValue, Vec<(String, CtValue)>), Diagnostic> {
    let mut root = build_spec(&schema.inputs, schema.description.as_deref(), argv0, span)?;
    let mut commands = Vec::new();
    for command in &schema.commands {
        let nested = build_spec(
            &command.inputs,
            command.description.as_deref(),
            &format!("{} {}", program_name(argv0), command.name),
            span,
        )?;
        root = args_call(
            "ArgsSpecSubcommand",
            &mut root,
            vec![
                CtValue::Str(command.name.clone()),
                CtValue::Str(command.description.clone().unwrap_or_default()),
                nested.clone(),
            ],
            span,
        )?;
        commands.push((command.name.clone(), nested));
    }
    Ok((root, commands))
}

fn parsed_option(parsed: &mut CtValue, name: &str, span: Span) -> Result<Option<String>, Diagnostic> {
    match args_call(
        "ParsedArgsOption",
        parsed,
        vec![CtValue::Str(name.to_string())],
        span,
    )? {
        CtValue::Present(value) => match *value {
            CtValue::Str(value) => Ok(Some(value)),
            _ => Err(unsupported("typed CLI option", span)),
        },
        CtValue::Failed(CtReport::Clean(_)) => Ok(None),
        _ => Err(unsupported("typed CLI option", span)),
    }
}

fn parsed_flag(parsed: &mut CtValue, name: &str, span: Span) -> Result<bool, Diagnostic> {
    match args_call(
        "ParsedArgsFlag",
        parsed,
        vec![CtValue::Str(name.to_string())],
        span,
    )? {
        CtValue::Bool(value) => Ok(value),
        _ => Err(unsupported("typed CLI flag", span)),
    }
}

fn parsed_subcommand(parsed: &mut CtValue, span: Span) -> Result<Option<String>, Diagnostic> {
    match args_call("ParsedArgsSubcommand", parsed, Vec::new(), span)? {
        CtValue::Present(value) => match *value {
            CtValue::Str(value) => Ok(Some(value)),
            _ => Err(unsupported("typed CLI subcommand", span)),
        },
        CtValue::Failed(CtReport::Clean(_)) => Ok(None),
        _ => Err(unsupported("typed CLI subcommand", span)),
    }
}

fn spec_help(spec: &mut CtValue, span: Span) -> Result<String, Diagnostic> {
    match args_call("ArgsSpecHelp", spec, Vec::new(), span)? {
        CtValue::Str(value) => Ok(value),
        _ => Err(unsupported("typed CLI help", span)),
    }
}

fn parse_args(
    spec: &mut CtValue,
    argv: &[String],
    span: Span,
) -> Result<Result<CtValue, String>, Diagnostic> {
    let parsed = args_call(
        "ArgsSpecParse",
        spec,
        vec![CtValue::List(argv.iter().cloned().map(CtValue::Str).collect())],
        span,
    )?;
    match parsed {
        CtValue::Present(value) => Ok(Ok(*value)),
        CtValue::Failed(CtReport::Told(value)) => Ok(Err(value.jet_show())),
        _ => Err(unsupported("typed CLI parse result", span)),
    }
}

fn path_value(value: String) -> CtValue {
    CtValue::Struct {
        type_name: crate::Syntax::TYPE_PATH.to_string(),
        fields: vec![("inner".to_string(), CtValue::Str(value))],
    }
}

fn scalar_from_text(ty: &Type, text: &str, flag: &str) -> Result<CtValue, String> {
    match ty {
        Type::Int => text
            .parse::<i64>()
            .map(CtValue::Int)
            .map_err(|_| format!("invalid value for --{flag}: `{text}` is not a whole number")),
        Type::Float => text
            .parse::<f64>()
            .map(|value| CtValue::Float(crate::AST::CtFloat::f64(value)))
            .map_err(|_| format!("invalid value for --{flag}: `{text}` is not a number")),
        Type::String => Ok(CtValue::Str(text.to_string())),
        Type::Named(name) if name == crate::Syntax::TYPE_PATH => Ok(path_value(text.to_string())),
        _ => Err(format!("typed CLI field --{flag} has no scalar decoder")),
    }
}

fn type_default(ty: &Type) -> CtValue {
    match ty {
        Type::Int => CtValue::Int(0),
        Type::Float => CtValue::Float(crate::AST::CtFloat::f64(0.0)),
        Type::String => CtValue::Str(String::new()),
        Type::Named(name) if name == crate::Syntax::TYPE_PATH => path_value(String::new()),
        _ => CtValue::Unit,
    }
}

fn default_value(
    ty: &Type,
    default: &CLIDefault,
    flag: &str,
) -> Result<CtValue, String> {
    match default {
        CLIDefault::Value(value) => Ok(value.clone()),
        CLIDefault::TypeDefault => Ok(type_default(ty)),
        CLIDefault::Recorded(value) => scalar_from_text(ty, value, flag),
    }
}

fn missing_value(input: &CLIInputSchema, ty: &Type, help: &str) -> Result<CtValue, String> {
    if let CLIInputShape::Value {
        default: Some(default),
        ..
    } = &input.shape
    {
        return default_value(ty, default, &input.flag);
    }
    Err(format!(
        "missing required {} --{}\n\n{}",
        if input.positional.is_some() { "argument" } else { "flag" },
        input.flag,
        help
    ))
}

fn decode_struct(
    structure: &StructDef,
    inputs: &[CLIInputSchema],
    parsed: &mut CtValue,
    help: &str,
    span: Span,
) -> Result<CtValue, String> {
    let mut fields = Vec::new();
    for field in structure.fields.iter().filter(|field| field.computed.is_none()) {
        let input = inputs
            .iter()
            .find(|input| input.field == field.name)
            .ok_or_else(|| format!("missing CLI input for `{}`", field.name))?;
        let value = match &input.shape {
            CLIInputShape::Flag => {
                if !matches!(&field.ty, Type::Bool) {
                    return Err(format!("CLI flag `{}` has a non-Bool field", input.flag));
                }
                CtValue::Bool(parsed_flag(parsed, &input.flag, span).map_err(|d| d.what)?)
            }
            CLIInputShape::Value { optional: true, .. } => {
                let Type::Option(inner) = &field.ty else {
                    return Err(format!("CLI option `{}` has a non-Option field", input.flag));
                };
                match parsed_option(parsed, &input.flag, span).map_err(|d| d.what)? {
                    Some(value) => CtValue::Present(Box::new(
                        scalar_from_text(inner, &value, &input.flag)?,
                    )),
                    None => CtValue::absent((**inner).clone()),
                }
            }
            CLIInputShape::Value { optional: false, .. } => {
                let raw = parsed_option(parsed, &input.flag, span).map_err(|d| d.what)?;
                match raw {
                    Some(value) => scalar_from_text(&field.ty, &value, &input.flag)?,
                    None => missing_value(input, &field.ty, help)?,
                }
            }
        };
        fields.push((field.name.clone(), value));
    }
    Ok(CtValue::Struct {
        type_name: structure.name.clone(),
        fields,
    })
}

fn find_struct<'a>(items: &'a [Item], name: &str) -> Option<&'a StructDef> {
    items.iter().find_map(|item| match item {
        Item::Struct(structure) if structure.name == name => Some(structure),
        _ => None,
    })
}

fn find_enum<'a>(items: &'a [Item], name: &str) -> Option<&'a EnumDef> {
    items.iter().find_map(|item| match item {
        Item::Enum(enumeration) if enumeration.name == name => Some(enumeration),
        _ => None,
    })
}

pub(super) fn prepare(
    bundle: &crate::AST::ProgramBundle,
    argv: &[String],
) -> Result<Dispatch, Diagnostic> {
    let schema = jet_foundation::CLISchema::entry_schema_for_bundle(bundle)
        .ok_or_else(|| unsupported("typed CLI entry", Span::new(0, 0)))?;
    let module = jet_foundation::CLISchema::entry_type_module(bundle)
        .ok_or_else(|| unsupported("typed CLI entry type", Span::new(0, 0)))?;
    let items = &bundle.modules[module].items;
    let type_name = schema
        .entry_type
        .rsplit('.')
        .next()
        .unwrap_or(&schema.entry_type);
    let span = Span::new(0, 0);
    let (mut spec, command_specs) = build_command_spec(
        &schema,
        argv.first().map(String::as_str).unwrap_or("program"),
        span,
    )?;
    let parsed = match parse_args(&mut spec, argv, span)? {
        Ok(parsed) => parsed,
        Err(error) => return Ok(Dispatch::Error(error)),
    };
    let mut parsed = parsed;
    if parsed_flag(&mut parsed, "help", span)? {
        let mut help_spec = if let Some(command) = parsed_subcommand(&mut parsed, span)? {
            command_specs
                .iter()
                .find(|(name, _)| name == &command)
                .map(|(_, spec)| spec.clone())
                .unwrap_or_else(|| spec.clone())
        } else {
            spec.clone()
        };
        let help = spec_help(&mut help_spec, span)?;
        return Ok(Dispatch::Help(help));
    }

    if schema.commands.is_empty() {
        let structure = find_struct(items, type_name)
            .ok_or_else(|| unsupported("typed CLI entry struct", span))?;
        let help = spec_help(&mut spec, span)?;
        return decode_struct(structure, &schema.inputs, &mut parsed, &help, span)
            .map(Dispatch::Run)
            .map_err(|error| {
                Diagnostic::error(
                    "E2201",
                    error,
                    "typed CLI decoding failed".to_string(),
                    "fix the command arguments".to_string(),
                    None,
                )
            });
    }

    let Some(command) = parsed_subcommand(&mut parsed, span)? else {
        return Ok(Dispatch::Help(spec_help(&mut spec, span)?));
    };
    let command_schema = schema
        .commands
        .iter()
        .find(|candidate| candidate.name == command)
        .ok_or_else(|| unsupported("typed CLI subcommand", span))?;
    let enumeration = find_enum(items, type_name)
        .ok_or_else(|| unsupported("typed CLI entry enum", span))?;
    let variant = enumeration
        .variants
        .iter()
        .find(|variant| variant.name.to_lowercase() == command)
        .ok_or_else(|| unsupported("typed CLI subcommand variant", span))?;
    let VariantPayload::Single(Type::Named(payload_name), _) = &variant.payload else {
        return Err(unsupported("typed CLI subcommand payload", span));
    };
    let payload = find_struct(items, payload_name)
        .ok_or_else(|| unsupported("typed CLI subcommand payload struct", span))?;
    let mut help_spec = command_specs
        .iter()
        .find(|(name, _)| name == &command)
        .map(|(_, spec)| spec.clone())
        .unwrap_or_else(|| spec.clone());
    let help = spec_help(&mut help_spec, span)?;
    let payload = decode_struct(payload, &command_schema.inputs, &mut parsed, &help, span)
        .map_err(|error| {
            Diagnostic::error(
                "E2201",
                error,
                "typed CLI decoding failed".to_string(),
                "fix the command arguments".to_string(),
                None,
            )
        })?;
    Ok(Dispatch::Run(CtValue::Enum {
        type_name: enumeration.name.clone(),
        variant: variant.name.clone(),
        args: vec![(None, payload)],
    }))
}
