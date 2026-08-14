//! Typed CLI entry marshalling for the canonical TIR interpreter.
//!
//! The parser and its error/default rules live in the embedded Args Prelude.
//! This module only turns the checked `CLISchema` projection into Prelude
//! handles and turns the resulting scalar values into `CtValue`s.

use crate::AST::{CtReport, CtValue, Func, Item, Param, StructDef, Type};
use crate::Comptime::Builtins::exact_int_value;
use jet_foundation::CLISchema::{
    CLIDefault, CLICommandSchema, CLIInputSchema, CLIInputShape, CLIValueKind,
};
use jet_foundation::Numeric::CtBigInt;
use crate::Comptime;
use crate::Diagnostics::{Diagnostic, Span};

use super::unsupported;

pub(super) enum Dispatch {
    Run(CtValue),
    Direct { function: String, args: Vec<CtValue> },
    Invoke { function: String, receiver: Option<CtValue>, args: Vec<CtValue> },
    Help(String),
    Version(String),
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
    standard: bool,
    version: Option<&str>,
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
    if standard {
        spec = args_call(
            "ArgsSpecFlagShort",
            &mut spec,
            vec![
                CtValue::Str("verbose".to_string()),
                CtValue::Str("v".to_string()),
                CtValue::Str("print extra detail".to_string()),
            ],
            span,
        )?;
        spec = args_call(
            "ArgsSpecFlagShort",
            &mut spec,
            vec![
                CtValue::Str("quiet".to_string()),
                CtValue::Str("q".to_string()),
                CtValue::Str("suppress normal output".to_string()),
            ],
            span,
        )?;
        spec = args_call(
            "ArgsSpecOptionChoice",
            &mut spec,
            vec![
                CtValue::Str("color".to_string()),
                CtValue::Str("control terminal color".to_string()),
                CtValue::Str("MODE".to_string()),
                CtValue::Str("auto,always,never".to_string()),
            ],
            span,
        )?;
        if let Some(version) = version {
            spec = args_call(
                "ArgsSpecVersion",
                &mut spec,
                vec![CtValue::Str(version.to_string())],
                span,
            )?;
        }
    }
    add_inputs(spec, inputs, span)
}

fn build_command_spec(
    schema: &CLICommandSchema,
    argv0: &str,
    span: Span,
) -> Result<(CtValue, Vec<(String, CtValue)>), Diagnostic> {
    let mut root = build_spec(
        &schema.inputs,
        schema.description.as_deref(),
        schema.standard,
        schema.version.as_deref(),
        argv0,
        span,
    )?;
    let mut commands = Vec::new();
    for command in &schema.commands {
        let nested = build_spec(
            &command.inputs,
            command.description.as_deref(),
            false,
            None,
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
        Type::Bool => match text.to_ascii_lowercase().as_str() {
            "true" => Ok(CtValue::Bool(true)),
            "false" => Ok(CtValue::Bool(false)),
            _ => Err(format!(
                "invalid value for --{flag}: `{text}` is not true or false"
            )),
        },
        Type::Int => CtBigInt::from_str(text)
            .map(exact_int_value)
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
        Type::Bool => CtValue::Bool(false),
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
    type_name: &str,
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
        let value = decode_input(input, &field.ty, parsed, help, span)?;
        fields.push((field.name.clone(), value));
    }
    Ok(CtValue::Struct {
        type_name: type_name.to_string(),
        fields,
    })
}

fn decode_input(
    input: &CLIInputSchema,
    ty: &Type,
    parsed: &mut CtValue,
    help: &str,
    span: Span,
) -> Result<CtValue, String> {
    match &input.shape {
        CLIInputShape::Flag => {
            if !matches!(ty, Type::Bool) {
                return Err(format!("CLI flag `{}` has a non-Bool field", input.flag));
            }
            Ok(CtValue::Bool(
                parsed_flag(parsed, &input.flag, span).map_err(|d| d.what)?,
            ))
        }
        CLIInputShape::Value { optional: true, .. } => {
            let Type::Option(inner) = ty else {
                return Err(format!("CLI option `{}` has a non-Option field", input.flag));
            };
            match parsed_option(parsed, &input.flag, span).map_err(|d| d.what)? {
                Some(value) => Ok(CtValue::Present(Box::new(scalar_from_text(
                    inner,
                    &value,
                    &input.flag,
                )?))),
                None => Ok(CtValue::absent((**inner).clone())),
            }
        }
        CLIInputShape::Value { optional: false, .. } => {
            let raw = parsed_option(parsed, &input.flag, span).map_err(|d| d.what)?;
            match raw {
                Some(value) => scalar_from_text(ty, &value, &input.flag),
                None => missing_value(input, ty, help),
            }
        }
    }
}

fn decode_params(
    params: &[Param],
    inputs: &[CLIInputSchema],
    parsed: &mut CtValue,
    help: &str,
    span: Span,
) -> Result<Vec<CtValue>, String> {
    params
        .iter()
        .filter(|param| param.name != crate::Syntax::KW_SELF)
        .map(|param| {
            let input = inputs
                .iter()
                .find(|input| input.field == param.name)
                .ok_or_else(|| format!("missing CLI input for `{}`", param.name))?;
            decode_input(input, &param.ty, parsed, help, span)
        })
        .collect()
}

fn find_struct<'a>(items: &'a [Item], name: &str) -> Option<&'a StructDef> {
    items.iter().find_map(|item| match item {
        Item::Struct(structure) if structure.name == name => Some(structure),
        _ => None,
    })
}

fn find_func<'a>(items: &'a [Item], name: &str) -> Option<&'a Func> {
    items.iter().find_map(|item| match item {
        Item::Func(function) if function.name == name => Some(function),
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
    let nominal_name = |name: &str| {
        if module == bundle.entry {
            name.to_string()
        } else {
            bundle
                .name_ledger
                .nominal_identity(module, name)
                .unwrap_or_else(|| name.to_string())
        }
    };
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
    if schema.standard {
        let color_mode = parsed_option(&mut parsed, "color", span)?
            .unwrap_or_else(|| "auto".to_string());
        super::term_semantics::jet_term_set_color_mode(&color_mode);
    }
    if parsed_flag(&mut parsed, "help", span)? {
        let mut help_spec = if let Some(command) = parsed_subcommand(&mut parsed, span)? {
            command_specs
                .iter()
                .find(|(name, _)| name.as_str() == command.as_str())
                .map(|(_, spec)| spec.clone())
                .unwrap_or_else(|| spec.clone())
        } else {
            spec.clone()
        };
        let help = spec_help(&mut help_spec, span)?;
        return Ok(Dispatch::Help(help));
    }

    if schema.standard && parsed_flag(&mut parsed, "version", span)? {
        if let Some(version) = schema.version.clone() {
            return Ok(Dispatch::Version(version));
        }
    }

    if schema.commands.is_empty() {
        if type_name == "run"
            && jet_foundation::CLISchema::is_direct_run_entry(
                &bundle.modules[bundle.entry].items,
            )
        {
            let function = find_func(items, "run")
                .ok_or_else(|| unsupported("typed CLI direct entry", span))?;
            let help = spec_help(&mut spec, span)?;
            return decode_params(&function.params, &schema.inputs, &mut parsed, &help, span)
                .map(|args| Dispatch::Direct {
                    function: function.name.clone(),
                    args,
                })
                .map_err(|error| {
                    crate::Sema::Diagnostics::render_registered(
                        "E2201",
                        error,
                        "typed CLI decoding failed".to_string(),
                        "fix the command arguments".to_string(),
                        None,
                    )
                });
        }
        let structure = find_struct(items, type_name)
            .ok_or_else(|| unsupported("typed CLI entry struct", span))?;
        let help = spec_help(&mut spec, span)?;
        let value_type_name = nominal_name(type_name);
        return decode_struct(
            structure,
            &value_type_name,
            &schema.inputs,
            &mut parsed,
            &help,
            span,
        )
            .map(Dispatch::Run)
            .map_err(|error| {
                crate::Sema::Diagnostics::render_registered(
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
        .find(|candidate| candidate.name.as_str() == command.as_str())
        .ok_or_else(|| unsupported("typed CLI subcommand", span))?;
    let mut help_spec = command_specs
        .iter()
        .find(|(name, _)| name.as_str() == command.as_str())
        .map(|(_, spec)| spec.clone())
        .unwrap_or_else(|| spec.clone());
    let help = spec_help(&mut help_spec, span)?;
    let structure = find_struct(items, type_name)
        .ok_or_else(|| unsupported("typed CLI entry struct", span))?;
    let method_member = structure
        .methods
        .iter()
        .any(|method| method.name.to_lowercase() == command.as_str());
    let method = structure
        .methods
        .iter()
        .find(|method| method.name.to_lowercase() == command.as_str());
    let binding = structure
        .cli_bindings
        .iter()
        .find(|binding| binding.name.to_lowercase() == command.as_str());
    let function = if let Some(method) = method {
        method
    } else {
        let crate::AST::Expr::Ident(target, _) = binding
            .ok_or_else(|| unsupported("typed CLI command binding", span))?
            .target
            .without_parens()
        else {
            return Err(unsupported("typed CLI command binding target", span));
        };
        find_func(items, target)
            .ok_or_else(|| unsupported("typed CLI bound function", span))?
    };
    let is_method = function
        .params
        .iter()
        .any(|param| param.name == crate::Syntax::KW_SELF);
    let bound_shared = binding.is_some()
        && function.params.first().is_some_and(|param| {
            matches!(&param.ty, Type::Named(name) if name.rsplit('.').next().unwrap_or(name) == type_name)
        });
    let mut receiver = if is_method || bound_shared {
        let value_type_name = nominal_name(type_name);
        Some(
            decode_struct(
                structure,
                &value_type_name,
                &schema.inputs,
                &mut parsed,
                &help,
                span,
            )
            .map_err(|error| {
                crate::Sema::Diagnostics::render_registered(
                    "E2201",
                    error,
                    "typed CLI decoding failed".to_string(),
                    "fix the command arguments".to_string(),
                    None,
                )
            }
            )?,
        )
    } else {
        None
    };
    let command_params = function
        .params
        .iter()
        .filter(|param| {
            param.name != crate::Syntax::KW_SELF
                && !(bound_shared
                    && matches!(&param.ty, Type::Named(name) if name.rsplit('.').next().unwrap_or(name) == type_name))
        })
        .cloned()
        .collect::<Vec<_>>();
    let args = decode_params(&command_params, &command_schema.inputs, &mut parsed, &help, span)
        .map_err(|error| {
            crate::Sema::Diagnostics::render_registered(
                "E2201",
                error,
                "typed CLI decoding failed".to_string(),
                "fix the command arguments".to_string(),
                None,
            )
        })?;
    // Inherent methods bind their receiver through the evaluator's `self`
    // scope. A bound free function has an ordinary first parameter, so pass
    // the same decoded program value through its argument tuple instead.
    let (receiver, args) = if bound_shared {
        let mut args = args;
        args.insert(
            0,
            receiver
                .take()
                .ok_or_else(|| unsupported("typed CLI bound receiver", span))?,
        );
        (None, args)
    } else {
        (receiver, args)
    };
    let function_owner = nominal_name(&structure.name);
    let function_name = if method_member {
        format!("{}::{}", function_owner, function.name)
    } else if let Some((module_name, _)) = function_owner.rsplit_once("::") {
        format!("{module_name}::{}", function.name)
    } else {
        function.name.clone()
    };
    Ok(Dispatch::Invoke {
        function: function_name,
        receiver,
        args,
    })
}
