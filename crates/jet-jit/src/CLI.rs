//! Typed CLI entry adapter (#1219) — CLISchema + canonical Args parser.
//! The zero-argument `jet_jit_cli_main` trampoline decodes argv and calls user
//! `run(args)`.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use super::Concurrency;
use crate::Marshal::{alloc_string, result_ok};
use jet_foundation::MIR::{
    MirArtifactId, MirArtifactTarget, MirCliDefault, MirCliEntry, MirCliInput,
    MirCliInputShape, MirCliValueKind, MirConstant, MirFunctionId, MirProgram, MirType,
    MirTypeKind,
};
use std::cell::RefCell;
use std::sync::atomic::{AtomicPtr, Ordering};

#[allow(dead_code, unused_imports, clippy::all)]
mod runtime {
    use super::{Concurrency, MirCliValueKind};
    use crate::Job::jet_args_source_program_name;

    trait JetShow {
        fn jet_show(&self) -> String;
    }
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/Args.rs");

    #[derive(Clone)]
    pub(super) struct Spec(JetArgsSpec);
    #[derive(Clone)]
    pub(super) struct Parsed(JetParsedArgs);

    pub(super) fn empty_spec(prog: &str) -> Spec {
        Spec(jet_args_program(jet_args_spec(), prog))
    }

    pub(super) fn description(spec: Spec, description: &str) -> Spec {
        Spec(jet_args_description(spec.0, &description.to_string()))
    }

    pub(super) fn flag(spec: Spec, name: &str, help: &str) -> Spec {
        Spec(jet_args_flag(spec.0, &name.to_string(), &help.to_string()))
    }

    pub(super) fn flag_short(spec: Spec, name: &str, short: &str, help: &str) -> Spec {
        Spec(jet_args_flag_short(
            spec.0,
            &name.to_string(),
            &short.to_string(),
            &help.to_string(),
        ))
    }

    pub(super) fn option(
        spec: Spec,
        name: &str,
        short: Option<&str>,
        help: &str,
        meta: &str,
        default: Option<String>,
        env: Option<&str>,
        required: bool,
        repeat: bool,
        kind: MirCliValueKind,
    ) -> Spec {
        let value = match kind {
            MirCliValueKind::Int => JetArgValueKind::Int,
            MirCliValueKind::Float => JetArgValueKind::Float,
            MirCliValueKind::String | MirCliValueKind::Path => JetArgValueKind::String,
            MirCliValueKind::Bool => JetArgValueKind::String,
        };
        Spec(jet_args_option_base(
            spec.0,
            &name.to_string(),
            short.map(str::to_string),
            &help.to_string(),
            &meta.to_string(),
            default,
            env.map(str::to_string),
            required,
            repeat,
            value,
        ))
    }

    pub(super) fn option_choice(
        spec: Spec,
        name: &str,
        help: &str,
        meta: &str,
        choices: &str,
    ) -> Spec {
        Spec(jet_args_option_choice(
            spec.0,
            &name.to_string(),
            &help.to_string(),
            &meta.to_string(),
            &choices.to_string(),
        ))
    }

    pub(super) fn version(spec: Spec, version: &str) -> Spec {
        Spec(jet_args_version(spec.0, &version.to_string()))
    }

    pub(super) fn positional(spec: Spec, name: &str, help: &str) -> Spec {
        Spec(jet_args_positional(
            spec.0,
            &name.to_string(),
            &help.to_string(),
        ))
    }

    pub(super) fn subcommand_spec(spec: Spec, name: &str, help: &str, nested: Spec) -> Spec {
        Spec(jet_args_subcommand(
            spec.0,
            &name.to_string(),
            &help.to_string(),
            nested.0,
        ))
    }

    pub(super) fn parse(spec: &Spec, argv: &[String]) -> Result<Parsed, String> {
        let argv = argv.to_vec();
        jet_args_parse(&spec.0, &argv).map(Parsed)
    }

    pub(super) fn parse_guided(spec: &Spec, argv: &[String]) -> Result<Parsed, String> {
        let argv = argv.to_vec();
        jet_args_parse_guided(&spec.0, &argv).map(Parsed)
    }

    pub(super) fn explicit_names(parsed: &Parsed) -> Vec<String> {
        parsed
            .0
            .explicit_flags
            .iter()
            .chain(parsed.0.explicit_options.iter())
            .cloned()
            .collect()
    }
    pub(super) fn guided_argv(spec: &Spec, argv: &[String]) -> Result<Vec<String>, String> {
        if !crate::IO::term_prelude::jet_term_stdin_is_terminal()
            || !crate::IO::term_prelude::jet_term_stderr_is_terminal()
            || crate::IO::term_prelude::jet_term_machine_output()
            || std::env::var_os("CI").is_some()
        {
            return Ok(argv.to_vec());
        }
        let input = argv.to_vec();
        jet_args_guided_argv_with(&spec.0, &input, |field, initial, error| {
            let mut prompt = if field.help.is_empty() {
                format!("{}:", field.name)
            } else {
                format!("{} — {}:", field.name, field.help)
            };
            if !initial.is_empty() {
                prompt.push_str(&format!(" [{}]", initial));
            }
            if let Some(error) = error {
                prompt.push_str(&format!(" Correction: {error}"));
            }
            prompt.push(' ');
            crate::runtime_host::write_jit_stderr(&prompt, true)?;
            match crate::IO::term_prelude::jet_term_read_stdin_line() {
                Ok(crate::IO::term_prelude::JetTermRead::Line(value)) => Ok(value),
                Ok(crate::IO::term_prelude::JetTermRead::EndOfInput) => {
                    Err("guided input ended before the form was submitted".to_string())
                }
                Err(error) => Err(format!("guided input could not read stdin: {error}")),
            }
        })
    }

    pub(super) fn help_text(spec: &Spec) -> String {
        spec.0.help()
    }

    pub(super) fn flag_set(parsed: &Parsed, name: &str) -> bool {
        jet_parsed_flag(&parsed.0, &name.to_string())
    }

    pub(super) fn option_val(parsed: &Parsed, name: &str) -> Option<String> {
        jet_parsed_option(&parsed.0, &name.to_string()).ok()
    }

    pub(super) fn option_values(parsed: &Parsed, name: &str) -> Vec<String> {
        jet_parsed_options(&parsed.0, &name.to_string())
    }

    pub(super) fn standard_log_level(parsed: &Parsed) -> String {
        jet_args_standard_log_level(&parsed.0)
    }

    pub(super) fn standard_color_mode(parsed: &Parsed) -> String {
        jet_args_standard_color_mode(&parsed.0)
    }

    pub(super) fn subcommand(parsed: &Parsed) -> Option<String> {
        jet_parsed_subcommand(&parsed.0).ok()
    }
}

use crate::Job::{jet_args_source_program_name, jet_cli_banner};

use runtime::{
    empty_spec, explicit_names, flag, flag_set, flag_short, help_text, option, option_val,
    option_values, parse, parse_guided, positional, standard_color_mode, standard_log_level, Parsed,
    Spec,
};

mod inline_range_semantics {
    include!("../../jet-codegen/src/Prelude/Core/InlineRange.rs");
}

fn apply_standard_cli(parsed: &Parsed, standard: bool) {
    if !standard {
        return;
    }
    super::CoreHost::set_cli_log_level(&standard_log_level(parsed));
    crate::IO::set_cli_color_mode(&standard_color_mode(parsed));
}
#[derive(Clone)]
pub(crate) struct CLIPlan {
    pub schema: MirCliEntry,
    pub version: Option<String>,
    /// Field types for the entry record, or the direct `run` parameters.
    pub field_types: Vec<(String, MirType)>,
    /// Canonical callable members. Stable MIR function IDs are resolved to
    /// native pointers only after the artifact has been compiled.
    pub commands: Vec<CLICommandPlan>,
    /// The CLI frame passed to the `run` adapter is already the entry record.
    pub run_record: bool,
    /// The typed entry's ABI carries a non-unit return value.
    pub run_returns_value: bool,
    pub user_run: MirFunctionId,
}

#[derive(Clone)]
pub(crate) struct CLICommandPlan {
    pub name: String,
    pub function: MirFunctionId,
    pub method: bool,
    pub ptr: Option<*const u8>,
}

thread_local! {
    static CLI_PLAN: RefCell<Option<CLIPlan>> = const { RefCell::new(None) };
}

static CLI_RUN_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

pub(crate) fn clear_cli_plan() {
    CLI_PLAN.with(|slot| *slot.borrow_mut() = None);
    CLI_RUN_PTR.store(std::ptr::null_mut(), Ordering::SeqCst);
}

pub(crate) fn install_cli_plan(plan: CLIPlan) {
    CLI_PLAN.with(|slot| *slot.borrow_mut() = Some(plan));
}

pub(crate) fn install_cli_run_ptr(ptr: *const u8) {
    CLI_RUN_PTR.store(ptr as *mut (), Ordering::SeqCst);
}
pub(crate) fn install_cli_command_ptr(function: MirFunctionId, ptr: *const u8) {
    CLI_PLAN.with(|slot| {
        let mut plan_slot = slot.borrow_mut();
        let Some(plan) = plan_slot.as_mut() else {
            return;
        };
        if let Some(command) = plan
            .commands
            .iter_mut()
            .find(|command| command.function == function)
        {
            command.ptr = Some(ptr);
        }
    });
}

pub(crate) fn cli_function_targets() -> Vec<MirFunctionId> {
    CLI_PLAN.with(|slot| {
        let plan_slot = slot.borrow();
        let Some(plan) = plan_slot.as_ref() else {
            return Vec::new();
        };
        let mut targets = vec![plan.user_run];
        targets.extend(plan.commands.iter().map(|command| command.function));
        targets.sort();
        targets.dedup();
        targets
    })
}

pub(crate) fn cli_run_requires_adapter() -> bool {
    CLI_PLAN.with(|slot| slot.borrow().is_some())
}

pub(crate) fn cli_run_frame_is_value() -> bool {
    CLI_PLAN.with(|slot| slot.borrow().as_ref().is_some_and(|plan| plan.run_record))
}
pub(crate) fn cli_user_run_target() -> Option<MirFunctionId> {
    CLI_PLAN.with(|slot| slot.borrow().as_ref().map(|plan| plan.user_run))
}

pub(crate) fn cli_command_targets() -> Vec<MirFunctionId> {
    CLI_PLAN.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|plan| plan.commands.iter().map(|command| command.function).collect())
            .unwrap_or_default()
    })
}


/// Install a CLI plan from the exact canonical MIR artifact.
pub(crate) fn prepare_cli_from_mir(program: &MirProgram, artifact_id: MirArtifactId) {
    clear_cli_plan();
    let Some(artifact) = program
        .artifacts
        .iter()
        .find(|artifact| artifact.id == artifact_id)
    else {
        return;
    };
    if artifact.target != MirArtifactTarget::Cranelift {
        return;
    }
    let Some(entry) = artifact.entry.as_ref() else {
        return;
    };
    let Some(cli) = entry.cli.clone() else {
        return;
    };
    let Some(user_run) = entry.function else {
        return;
    };
    let Some(run_function) = program.functions.iter().find(|function| function.id == user_run)
    else {
        return;
    };
    if !cli.record_inputs || run_function.params.len() != 1 {
        return;
    }
    let field_types = cli
        .inputs
        .iter()
        .map(|input| (input.name.clone(), input.ty.clone()))
        .collect();
    let commands = cli
        .commands
        .iter()
        .map(|command| CLICommandPlan {
            name: command.name.clone(),
            function: command.function,
            method: command.receiver.is_some(),
            ptr: None,
        })
        .collect();
    let version = (!entry.package_version.is_empty()).then(|| entry.package_version.clone());
    install_cli_plan(CLIPlan {
        schema: cli,
        version,
        field_types,
        commands,
        run_record: true,
        run_returns_value: !run_function.return_type.is_unit(),
        user_run,
    });
}

fn alloc_path_record(path: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(1);
        let string = rt.heap.alloc_string(path);
        let _ = rt.heap.record_set_string(record, 0, string);
        record
    })
}

fn cli_value_kind(input: &MirCliInput) -> MirCliValueKind {
    match &input.shape {
        MirCliInputShape::Flag => MirCliValueKind::Bool,
        MirCliInputShape::Value { kind, .. } => *kind,
    }
}

fn build_spec(
    inputs: &[MirCliInput],
    description: Option<&str>,
    standard: bool,
    version: Option<&str>,
    prog: &str,
) -> Result<Spec, String> {
    let mut spec = empty_spec(&jet_args_source_program_name(prog));
    if let Some(description) = description {
        spec = runtime::description(spec, description);
    }
    if standard {
        spec = flag_short(spec, "verbose", "v", "print extra detail");
        spec = flag_short(spec, "quiet", "q", "suppress normal output");
        spec = runtime::option_choice(
            spec,
            "color",
            "control terminal color",
            "MODE",
            "auto,always,never",
        );
        if let Some(version) = version {
            spec = runtime::version(spec, version);
        }
    }
    for input in inputs {
        let flag_name = input.name.clone();
        let help = input.help.clone();
        match &input.shape {
            MirCliInputShape::Flag => {
                spec = match &input.short {
                    Some(short) => flag_short(spec, &flag_name, short, &help),
                    None => flag(spec, &flag_name, &help),
                };
            }
            MirCliInputShape::Value {
                default,
                optional,
                ..
            } => {
                let meta = input
                    .metavar
                    .clone()
                    .unwrap_or_else(|| "VALUE".to_string());
                let has_declared_default = default.is_some();
                let default = input_default_text(input)?;
                let required =
                    !optional && !has_declared_default && input.positional.is_none();
                spec = option(
                    spec,
                    &flag_name,
                    input.short.as_deref(),
                    &help,
                    &meta,
                    default,
                    input.env.as_deref(),
                    required,
                    input.variadic,
                    cli_value_kind(input),
                );
                if input.positional.is_some() {
                    spec = positional(spec, &flag_name, &help);
                }
            }
        }
    }
    Ok(spec)
}

fn build_command_spec(
    schema: &MirCliEntry,
    version: Option<&str>,
    prog: &str,
) -> Result<(Spec, Vec<(String, Spec)>), String> {
    let mut root = build_spec(
        &schema.inputs,
        schema.description.as_deref(),
        schema.standard,
        version,
        prog,
    )?;
    let mut commands = Vec::new();
    for command in &schema.commands {
        let nested_prog = format!("{} {}", jet_args_source_program_name(prog), command.name);
        let nested = build_spec(
            &command.inputs,
            command.description.as_deref(),
            false,
            None,
            &nested_prog,
        )?;
        root = runtime::subcommand_spec(
            root,
            &command.name,
            &command.description.clone().unwrap_or_default(),
            nested.clone(),
        );
        commands.push((command.name.clone(), nested));
    }
    Ok((root, commands))
}

fn is_path_type(ty: &MirType) -> bool {
    match &ty.kind {
        MirTypeKind::Int
        | MirTypeKind::Float
        | MirTypeKind::Bool
        | MirTypeKind::String
        | MirTypeKind::Char
        | MirTypeKind::TraitObject(_)
        | MirTypeKind::Measure(_) => false,
        MirTypeKind::List(inner)
        | MirTypeKind::Shared(inner)
        | MirTypeKind::Option(inner)
        | MirTypeKind::InlineRange { base: inner, .. }
        | MirTypeKind::Tagged { inner, .. }
        | MirTypeKind::Quantity { base: inner, .. } => is_path_type(inner),
        MirTypeKind::Map { .. }
        | MirTypeKind::Result { .. }
        | MirTypeKind::Fn(_)
        | MirTypeKind::SendFn { .. }
        | MirTypeKind::Tuple(_)
        | MirTypeKind::FixedList { .. }
        | MirTypeKind::IntN { .. }
        | MirTypeKind::Float32
        | MirTypeKind::Union(_) => false,
        MirTypeKind::Apply { name, .. } => {
            name.name == jet_foundation::Syntax::TYPE_PATH
        }
    }
}

fn inline_range_bounds(ty: &MirType) -> Option<(i64, i64)> {
    match &ty.kind {
        MirTypeKind::InlineRange { lo, hi, .. } => Some((*lo, *hi)),
        MirTypeKind::Tagged { inner, .. } | MirTypeKind::Quantity { base: inner, .. } => {
            inline_range_bounds(inner)
        }
        MirTypeKind::Int
        | MirTypeKind::Float
        | MirTypeKind::Bool
        | MirTypeKind::String
        | MirTypeKind::Char
        | MirTypeKind::List(_)
        | MirTypeKind::Map { .. }
        | MirTypeKind::Shared(_)
        | MirTypeKind::Option(_)
        | MirTypeKind::Result { .. }
        | MirTypeKind::Fn(_)
        | MirTypeKind::SendFn { .. }
        | MirTypeKind::Apply { .. }
        | MirTypeKind::TraitObject(_)
        | MirTypeKind::Tuple(_)
        | MirTypeKind::FixedList { .. }
        | MirTypeKind::IntN { .. }
        | MirTypeKind::Float32
        | MirTypeKind::Union(_)
        | MirTypeKind::Measure(_) => None,
    }
}

fn constant_text(value: &MirConstant) -> Result<String, String> {
    match value {
        MirConstant::Int { value, .. } => Ok(value.to_string()),
        MirConstant::Float { value, .. } => Ok(value.to_string()),
        MirConstant::Bool(value) => Ok(value.to_string()),
        MirConstant::Char(value) => Ok(value.to_string()),
        MirConstant::String(value) => Ok(value.clone()),
        MirConstant::Bytes(_) => Err("byte defaults are not valid CLI scalars".to_string()),
        MirConstant::Unit => Err("unit defaults are not valid CLI scalars".to_string()),
        MirConstant::BigInt(value) => Ok(value.clone()),
        MirConstant::List(_) => Err("list defaults are not valid CLI scalars".to_string()),
        MirConstant::Map(_) => Err("map defaults are not valid CLI scalars".to_string()),
        MirConstant::Struct { .. } => {
            Err("struct defaults are not valid CLI scalars".to_string())
        }
        MirConstant::Enum { .. } => Err("enum defaults are not valid CLI scalars".to_string()),
        MirConstant::Present(value) => constant_text(value),
        MirConstant::Failed(report) => match report {
            jet_foundation::MIR::MirConstReport::Clean(_) => {
                Err("failed defaults are not valid CLI scalars".to_string())
            }
            jet_foundation::MIR::MirConstReport::Told(value) => constant_text(value),
        },
    }
}

fn default_text(default: Option<&MirCliDefault>) -> Result<Option<String>, String> {
    match default {
        None => Ok(None),
        Some(MirCliDefault::TypeDefault) => Ok(None),
        Some(MirCliDefault::Value(value)) => constant_text(value).map(Some),
    }
}

fn type_default_text(ty: &MirType) -> Option<String> {
    if let Some((lo, _)) = inline_range_bounds(ty) {
        return Some(lo.to_string());
    }
    if ty.is_bool() {
        return Some("false".to_string());
    }
    if ty.is_integer() || ty.is_float() {
        return Some("0".to_string());
    }
    if ty.is_string() || is_path_type(ty) {
        return Some(String::new());
    }
    None
}

fn input_default_text(input: &MirCliInput) -> Result<Option<String>, String> {
    let MirCliInputShape::Value { default, .. } = &input.shape else {
        return Ok(None);
    };
    match default {
        Some(MirCliDefault::TypeDefault) => Ok(type_default_text(&input.ty)),
        Some(MirCliDefault::Value(value)) => constant_text(value).map(Some),
        None => Ok(None),
    }
}

fn required_input_value(
    input: &MirCliInput,
    fty: &MirType,
    parsed: &Parsed,
    spec: &Spec,
) -> Result<String, String> {
    if let Some(value) = option_val(parsed, &input.name) {
        return Ok(value);
    }
    if let MirCliInputShape::Value { default, .. } = &input.shape {
        if let Some(value) = default_text(default.as_ref())? {
            return Ok(value);
        }
        if matches!(default, Some(MirCliDefault::TypeDefault)) {
            if let Some(value) = type_default_text(fty) {
                return Ok(value);
            }
        }
    }
    let kind = if input.positional.is_some() {
        "argument"
    } else {
        "flag"
    };
    Err(format!(
        "missing required {kind} {}\n\n{}",
        input.name,
        help_text(spec)
    ))
}

fn parse_bool(text: &str, name: &str) -> Result<bool, String> {
    match text.to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("invalid bool for --{name}")),
    }
}

fn decode_struct(
    inputs: &[MirCliInput],
    field_types: &[(String, MirType)],
    parsed: &Parsed,
    spec: &Spec,
) -> Result<i64, String> {
    decode_frame(inputs, field_types, parsed, spec, None)
}

fn decode_frame(
    inputs: &[MirCliInput],
    field_types: &[(String, MirType)],
    parsed: &Parsed,
    spec: &Spec,
    receiver: Option<i64>,
) -> Result<i64, String> {
    let offset = usize::from(receiver.is_some());
    let n = field_types.len() + offset;
    let rec = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_record(n));
    if let Some(receiver) = receiver {
        Concurrency::with_runtime_mut(|rt| {
            let _ = rt.heap.record_set_int(rec, 0, receiver);
        });
    }
    for (idx, (fname, fty)) in field_types.iter().enumerate() {
        let input = inputs
            .iter()
            .find(|input| input.name == *fname)
            .ok_or_else(|| format!("missing CLI input for `{fname}`"))?;
        let bits = match &input.shape {
            MirCliInputShape::Flag => {
                if !fty.is_bool() {
                    return Err(format!("CLI flag `{}` is not Bool", input.name));
                }
                i64::from(flag_set(parsed, &input.name))
            }
            MirCliInputShape::Value {
                kind,
                optional,
                ..
            } => {
                if *optional {
                    match fty.option_inner() {
                        Some(inner) if inner.is_bool() && *kind == MirCliValueKind::Bool => {
                            match option_val(parsed, &input.name) {
                                Some(value) => i64::from(parse_bool(&value, &input.name)?) + 1,
                                None => 0,
                            }
                        }
                        Some(inner) if inner.is_integer() && *kind == MirCliValueKind::Int => {
                            match option_val(parsed, &input.name) {
                                Some(value) => {
                                    if let Some((lo, hi)) = inline_range_bounds(inner) {
                                        let value = value.parse::<i64>().map_err(|_| {
                                            format!("invalid int for --{}", input.name)
                                        })?;
                                        inline_range_semantics::jet_inline_range_from_int(
                                            value, lo, hi,
                                        )
                                        .map(|value| value.wrapping_add(1))
                                        .map_err(|reason| {
                                            format!("invalid value for --{}: {reason}", input.name)
                                        })?
                                    } else {
                                        Concurrency::with_runtime_mut(|rt| {
                                            rt.heap.int_from_str(value.trim()).ok()
                                        })
                                        .ok_or_else(|| {
                                            format!("invalid int for --{}", input.name)
                                        })?
                                        .wrapping_add(1)
                                    }
                                }
                                None => 0,
                            }
                        }
                        Some(inner) if inner.is_float() && *kind == MirCliValueKind::Float => {
                            match option_val(parsed, &input.name) {
                                Some(value) => value
                                    .parse::<f64>()
                                    .map(|value| value.to_bits() as i64 + 1)
                                    .map_err(|_| {
                                        format!("invalid float for --{}", input.name)
                                    })?,
                                None => 0,
                            }
                        }
                        Some(inner)
                            if (inner.is_string() || is_path_type(inner))
                                && matches!(
                                    kind,
                                    MirCliValueKind::String | MirCliValueKind::Path
                                ) =>
                        {
                            match option_val(parsed, &input.name) {
                                Some(value) => {
                                    let value = if is_path_type(inner) {
                                        alloc_path_record(value)
                                    } else {
                                        alloc_string(value)
                                    };
                                    value.wrapping_add(1)
                                }
                                None => 0,
                            }
                        }
                        Some(_) => {
                            return Err(format!("jit CLI decode unsupported field `{fname}`"));
                        }
                        None => {
                            return Err(format!(
                                "optional CLI input `{}` has a non-option MIR type",
                                input.name
                            ));
                        }
                    }
                } else {
                    let value = required_input_value(input, fty, parsed, spec)?;
                    match kind {
                        MirCliValueKind::Bool => {
                            if !fty.is_bool() {
                                return Err(format!("CLI input `{}` is not Bool", input.name));
                            }
                            i64::from(parse_bool(&value, &input.name)?)
                        }
                        MirCliValueKind::Int => {
                            if !fty.is_integer() {
                                return Err(format!("CLI input `{}` is not Int", input.name));
                            }
                            if let Some((lo, hi)) = inline_range_bounds(fty) {
                                let value = value.parse::<i64>().map_err(|_| {
                                    format!("invalid int for --{}", input.name)
                                })?;
                                inline_range_semantics::jet_inline_range_from_int(value, lo, hi)
                                    .map_err(|reason| {
                                        format!("invalid value for --{}: {reason}", input.name)
                                    })?
                            } else {
                                Concurrency::with_runtime_mut(|rt| {
                                    rt.heap.int_from_str(value.trim()).ok()
                                })
                                .ok_or_else(|| format!("invalid int for --{}", input.name))?
                            }
                        }
                        MirCliValueKind::Float => {
                            if !fty.is_float() {
                                return Err(format!("CLI input `{}` is not Float", input.name));
                            }
                            value
                                .parse::<f64>()
                                .map(|value| value.to_bits() as i64)
                                .map_err(|_| format!("invalid float for --{}", input.name))?
                        }
                        MirCliValueKind::String | MirCliValueKind::Path => {
                            if !(fty.is_string() || is_path_type(fty)) {
                                return Err(format!("CLI input `{}` is not String", input.name));
                            }
                            if is_path_type(fty) {
                                alloc_path_record(value)
                            } else {
                                alloc_string(value)
                            }
                        }
                    }
                }
            }
        };
        Concurrency::with_runtime_mut(|rt| {
            let index = (idx + offset) as i64;
            if fty.is_float() {
                let _ = rt
                    .heap
                    .record_set_float(rec, index, f64::from_bits(bits as u64));
            } else if fty.is_bool() && fty.option_inner().is_none() {
                let _ = rt.heap.record_set_bool(rec, index, bits != 0);
            } else if fty.is_string() && fty.option_inner().is_none() {
                let _ = rt.heap.record_set_string(rec, index, bits);
            } else {
                let _ = rt.heap.record_set_int(rec, index, bits);
            }
        });
    }
    Ok(rec)
}

fn typed_cli_tree(
    inputs: &[MirCliInput],
    parsed: &Parsed,
    descriptor: &crate::runtime_host::RuntimeTypeDescriptor,
) -> Result<crate::Encoding::json_rt::DataTree, Vec<crate::Encoding::json_rt::FieldError>> {
    let mut fields = Vec::new();
    for input in inputs {
        let Some(field) = descriptor.fields.iter().find(|field| {
            !field.skip
                && !field.computed
                && (field.name_for(jet_foundation::Shape::ShapeProjectionKind::Args)
                    == input.name
                    || field.source_name == input.label)
        }) else {
            return Err(crate::Encoding::json_rt::FieldError::at(
                input.name.clone(),
                "checked CLI input has no target record field",
            ));
        };
        let output_name = field
            .name_for(jet_foundation::Shape::ShapeProjectionKind::Json)
            .to_string();
        if fields.iter().any(|(name, _)| name == &output_name) {
            return Err(crate::Encoding::json_rt::FieldError::at(
                output_name,
                "ambiguous duplicate argument shape field",
            ));
        }
        let value = match &input.shape {
            MirCliInputShape::Flag => {
                crate::Encoding::json_rt::DataTree::Bool(flag_set(parsed, &input.name))
            }
            MirCliInputShape::Value { .. } => {
                let values = option_values(parsed, &input.name);
                if values.is_empty() {
                    continue;
                }
                let mut decoded = Vec::with_capacity(values.len());
                for value in values {
                    let value = match cli_value_kind(input) {
                        MirCliValueKind::Bool => match parse_bool(&value, &input.name) {
                            Ok(value) => crate::Encoding::json_rt::DataTree::Bool(value),
                            Err(error) => {
                                return Err(crate::Encoding::json_rt::FieldError::at(
                                    input.name.clone(),
                                    error,
                                ));
                            }
                        },
                        MirCliValueKind::String | MirCliValueKind::Path => {
                            crate::Encoding::json_rt::DataTree::Text(value)
                        }
                        MirCliValueKind::Int => {
                            if let Some((lo, hi)) = inline_range_bounds(&input.ty) {
                                let parsed = match value.parse::<i64>() {
                                    Ok(value) => value,
                                    Err(_) => {
                                        return Err(crate::Encoding::json_rt::FieldError::at(
                                            input.name.clone(),
                                            format!("expected Int, found {value:?}"),
                                        ));
                                    }
                                };
                                if let Err(reason) =
                                    inline_range_semantics::jet_inline_range_from_int(parsed, lo, hi)
                                {
                                    return Err(crate::Encoding::json_rt::FieldError::at(
                                        input.name.clone(),
                                        reason,
                                    ));
                                }
                            }
                            match crate::Encoding::json_rt::jet_int_from_str(&value) {
                                Ok(value) => crate::Encoding::json_rt::DataTree::Int(value),
                                Err(_) => {
                                    return Err(crate::Encoding::json_rt::FieldError::at(
                                        input.name.clone(),
                                        format!("expected Int, found {value:?}"),
                                    ));
                                }
                            }
                        }
                        MirCliValueKind::Float => match value.parse::<f64>() {
                            Ok(value) => crate::Encoding::json_rt::DataTree::Float(value),
                            Err(_) => {
                                return Err(crate::Encoding::json_rt::FieldError::at(
                                    input.name.clone(),
                                    format!("expected Float, found {value:?}"),
                                ));
                            }
                        },
                    };
                    decoded.push(value);
                }
                if input.variadic {
                    crate::Encoding::json_rt::DataTree::Array(decoded)
                } else {
                    decoded
                        .pop()
                        .expect("checked CLI option values are non-empty")
                }
            }
        };
        fields.push((output_name, value));
    }
    Ok(crate::Encoding::json_rt::DataTree::Object(fields))
}

fn typed_cli_descriptor(type_key: &str) -> Option<crate::runtime_host::RuntimeTypeDescriptor> {
    Concurrency::with_runtime_mut(|rt| {
        let id = type_key
            .strip_prefix("id:")
            .and_then(|value| value.parse::<u64>().ok());
        match id {
            Some(id) => rt.runtime_type_descriptor(id).cloned(),
            None => rt.runtime_type_descriptor_by_name(type_key).cloned(),
        }
    })
}

fn typed_cli_error(error: impl Into<String>) -> i64 {
    crate::Encoding::result_err_fields(crate::Encoding::json_rt::FieldError::one(error))
}

pub(crate) fn jet_jit_args_decode(type_key: i64) -> i64 {
    let Some(type_key) = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(type_key)) else {
        return typed_cli_error("typed CLI decode received an invalid type key");
    };
    let Some(descriptor) = typed_cli_descriptor(&type_key) else {
        return typed_cli_error(format!("typed CLI decode has no type `{type_key}`"));
    };
    let Some(cli) = descriptor.cli.clone() else {
        return typed_cli_error(format!("type `{type_key}` has no checked CLI schema"));
    };
    let argv = jet_codegen::Comptime::runtime_argv().unwrap_or_default();
    let prog = argv.first().map(String::as_str).unwrap_or("program");
    let spec = match build_spec(
        &cli.inputs,
        cli.description.as_deref(),
        cli.standard,
        cli.version.as_deref(),
        prog,
    ) {
        Ok(spec) => spec,
        Err(error) => return typed_cli_error(error),
    };
    let parsed = match parse(&spec, &argv) {
        Ok(parsed) => parsed,
        Err(error) => return typed_cli_error(error),
    };
    let tree = match typed_cli_tree(&cli.inputs, &parsed, &descriptor) {
        Ok(tree) => tree,
        Err(errors) => return crate::Encoding::result_err_fields(errors),
    };
    match crate::Encoding::decode_datatree_for_type(&tree, &type_key) {
        Ok(value) => result_ok(value as u64),
        Err(errors) => crate::Encoding::result_err_fields(errors),
    }
}

pub(crate) fn jet_jit_args_merge(flags: i64, settings: i64, type_key: i64) -> i64 {
    let Some(type_key) = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(type_key)) else {
        return typed_cli_error("typed CLI merge received an invalid type key");
    };
    let Some(descriptor) = typed_cli_descriptor(&type_key) else {
        return typed_cli_error(format!("typed CLI merge has no type `{type_key}`"));
    };
    let Some(cli) = descriptor.cli.clone() else {
        return typed_cli_error(format!("type `{type_key}` has no checked CLI schema"));
    };
    let argv = jet_codegen::Comptime::runtime_argv().unwrap_or_default();
    let prog = argv.first().map(String::as_str).unwrap_or("program");
    let spec = match build_spec(
        &cli.inputs,
        cli.description.as_deref(),
        cli.standard,
        cli.version.as_deref(),
        prog,
    ) {
        Ok(spec) => spec,
        Err(error) => return typed_cli_error(error),
    };
    let parsed = match parse_guided(&spec, &argv) {
        Ok(parsed) => parsed,
        Err(error) => return typed_cli_error(error),
    };
    let explicit = explicit_names(&parsed);
    let Some((mut merged, flag_slots)) = Concurrency::with_runtime_mut(|rt| {
        Some((
            rt.heap.clone_record_values(settings)?,
            rt.heap.clone_record_values(flags)?,
        ))
    }) else {
        return typed_cli_error("typed CLI merge requires record flags and settings");
    };
    if merged.len() != flag_slots.len() {
        return typed_cli_error("typed CLI merge flags/settings record shape mismatch");
    }
    for input in &cli.inputs {
        if !explicit.iter().any(|name| name == &input.name) {
            continue;
        }
        let Some(field) = descriptor.fields.iter().find(|field| {
            !field.skip
                && !field.computed
                && (field.name_for(jet_foundation::Shape::ShapeProjectionKind::Args)
                    == input.name
                    || field.source_name == input.label)
        }) else {
            return typed_cli_error(format!(
                "typed CLI merge has no record field `{}`",
                input.name
            ));
        };
        if field.index >= merged.len() {
            return typed_cli_error(format!(
                "typed CLI merge field `{}` is outside its record",
                input.name
            ));
        }
        merged[field.index] = flag_slots[field.index].clone();
    }
    let record = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_record_values(merged));
    result_ok(record as u64)
}

fn report_cli_error(error: &str) {
    Concurrency::with_runtime_mut(|rt| {
        rt.stderr.push_str(&jet_cli_banner(error));
        rt.exit_code = Some(2);
    });
}

fn finish_cli_success() {
    Concurrency::with_runtime_mut(|rt| rt.exit_code = Some(0));
}

fn finish_cli_version(plan: &CLIPlan) {
    let Some(version) = plan.version.as_deref() else {
        report_cli_error("jit CLI: standard version metadata missing");
        return;
    };
    Concurrency::with_runtime_mut(|rt| {
        rt.stdout.push_str(&jet_cli_banner(version));
    });
    finish_cli_success();
}

/// Zero-arg trampoline installed as `jet_jit_cli_main` for typed CLI programs.
pub(crate) fn jet_jit_cli_main() -> i64 {
    let plan = CLI_PLAN.with(|slot| slot.borrow().clone());
    let Some(plan) = plan else {
        Concurrency::with_runtime_mut(|rt| {
            rt.stderr.push_str("jit CLI: no plan installed\n");
        });
        return 0;
    };
    let argv = jet_codegen::Comptime::runtime_argv().unwrap_or_default();
    let run_ptr = CLI_RUN_PTR.load(Ordering::SeqCst);
    if run_ptr.is_null() {
        Concurrency::with_runtime_mut(|rt| {
            rt.stderr.push_str("jit CLI: run pointer missing\n");
        });
        return 0;
    }
    let call_run = |args: i64| {
        if plan.run_returns_value {
            let run: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(run_ptr) };
            run(args)
        } else {
            let run: extern "C" fn(i64) = unsafe { std::mem::transmute(run_ptr) };
            run(args);
            0
        }
    };

    let prog = argv.first().map(String::as_str).unwrap_or("program");
    if !plan.commands.is_empty() {
        let (spec, command_specs) =
            match build_command_spec(&plan.schema, plan.version.as_deref(), prog) {
                Ok(value) => value,
                Err(error) => {
                    report_cli_error(&error);
                    return 0;
                }
            };
        let guided_argv = match runtime::guided_argv(&spec, &argv) {
            Ok(value) => value,
            Err(error) => {
                report_cli_error(&error);
                return 0;
            }
        };
        let parsed = match parse(&spec, &guided_argv) {
            Ok(p) => p,
            Err(e) => {
                report_cli_error(&e);
                return 0;
            }
        };
        apply_standard_cli(&parsed, plan.schema.standard);
        let command_name = runtime::subcommand(&parsed);
        if flag_set(&parsed, "help") {
            let help_spec = command_name
                .as_deref()
                .and_then(|name| {
                    command_specs
                        .iter()
                        .find(|(candidate, _)| candidate.as_str() == name)
                        .map(|(_, spec)| spec.clone())
                })
                .unwrap_or_else(|| spec.clone());
            Concurrency::with_runtime_mut(|rt| {
                rt.stdout.push_str(&jet_cli_banner(&help_text(&help_spec)));
            });
            finish_cli_success();
            return 0;
        }
        if plan.schema.standard && flag_set(&parsed, "version") {
            finish_cli_version(&plan);
            return 0;
        }
        let Some(command_name) = command_name else {
            Concurrency::with_runtime_mut(|rt| {
                rt.stdout.push_str(&jet_cli_banner(&help_text(&spec)));
            });
            finish_cli_success();
            return 0;
        };
        let Some(command) = plan
            .commands
            .iter()
            .find(|command| command.name.as_str() == command_name.as_str())
        else {
            report_cli_error(&format!("unknown command `{command_name}`"));
            return 0;
        };
        let Some(command_schema) = plan
            .schema
            .commands
            .iter()
            .find(|candidate| candidate.name.as_str() == command_name.as_str())
        else {
            report_cli_error("jit CLI: command schema missing");
            return 0;
        };
        let Some((_, command_spec)) = command_specs
            .iter()
            .find(|(candidate, _)| candidate.as_str() == command_name.as_str())
        else {
            report_cli_error("jit CLI: command parser missing");
            return 0;
        };
        let receiver = if command.method {
            match decode_struct(&plan.schema.inputs, &plan.field_types, &parsed, &spec) {
                Ok(receiver) => Some(receiver),
                Err(error) => {
                    report_cli_error(&error);
                    return 0;
                }
            }
        } else {
            None
        };
        let command_types: Vec<(String, MirType)> = command_schema
            .inputs
            .iter()
            .map(|input| (input.name.clone(), input.ty.clone()))
            .collect();
        if command_types.len() != command_schema.inputs.len() {
            report_cli_error("jit CLI: command signature/schema mismatch");
            return 0;
        }
        let frame = match decode_frame(
            &command_schema.inputs,
            &command_types,
            &parsed,
            command_spec,
            receiver,
        ) {
            Ok(frame) => frame,
            Err(error) => {
                report_cli_error(&error);
                return 0;
            }
        };
        let Some(ptr) = command.ptr else {
            report_cli_error(&format!(
                "jit CLI: command `{command_name}` pointer missing"
            ));
            return 0;
        };
        let call: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(ptr) };
        return call(frame);
    }

    let spec = match build_spec(
        &plan.schema.inputs,
        plan.schema.description.as_deref(),
        plan.schema.standard,
        plan.version.as_deref(),
        prog,
    ) {
        Ok(spec) => spec,
        Err(error) => {
            report_cli_error(&error);
            return 0;
        }
    };
    let guided_argv = match runtime::guided_argv(&spec, &argv) {
        Ok(value) => value,
        Err(error) => {
            report_cli_error(&error);
            return 0;
        }
    };
    let parsed = match parse(&spec, &guided_argv) {
        Ok(p) => p,
        Err(e) => {
            report_cli_error(&e);
            return 0;
        }
    };
    apply_standard_cli(&parsed, plan.schema.standard);
    if flag_set(&parsed, "help") {
        Concurrency::with_runtime_mut(|rt| {
            rt.stdout.push_str(&jet_cli_banner(&help_text(&spec)));
        });
        finish_cli_success();
        return 0;
    }
    if plan.schema.standard && flag_set(&parsed, "version") {
        finish_cli_version(&plan);
        return 0;
    }
    let args = match decode_struct(&plan.schema.inputs, &plan.field_types, &parsed, &spec) {
        Ok(h) => h,
        Err(e) => {
            report_cli_error(&e);
            return 0;
        }
    };
    call_run(args)
}

// `jet_jit_cli_main`'s registration + import lives in the top-level
// `HostFns` table (jit/runtime_host.rs) — the CLI trampoline is present
// on every JIT module (like every other host symbol), not only for
// `cli_entry` programs, so `host_fns_audit` sees a matching pair on every
// `new_jit_module()` call instead of only when compiling a CLI program.
