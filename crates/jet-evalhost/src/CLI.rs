//! Typed CLI entry adapter (#1219) — CLISchema + canonical Args parser.
//! The zero-argument `jet_jit_cli_main` trampoline decodes argv and calls user
//! `run(args)`.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.

use super::Concurrency;
use jet_foundation::MIR::{
    MirArtifactId, MirCliDefault, MirCliEntry, MirCliInput,
    MirCliInputShape, MirCliValueKind, MirConstant, MirProgram, MirType,
    MirTypeKind
};

#[allow(unused_imports, clippy::all)]
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
            MirCliValueKind::Bool => JetArgValueKind::String
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

    pub(super) fn subcommand(parsed: &Parsed) -> Option<String> {
        jet_parsed_subcommand(&parsed.0).ok()
    }
}

use crate::Job::jet_args_source_program_name;

use runtime::{
    empty_spec, flag, flag_short, option, parse, positional,
    Spec
};

mod inline_range_semantics {
    include!("../../jet-codegen/src/Prelude/Core/InlineRange.rs");
}

thread_local! {
}

/// D-DX-JOBGRAPH1=A: parse-only argv check for one checked MIR artifact
/// entry, built from the same spec `jet_jit_cli_main` decodes with. A job
/// graph validates its root arguments with this before any predecessor runs,
/// matching the generated AOT job validator (plain parse, never guided input).
/// `argv[0]` is the program name.
pub fn validate_entry_argv(
    program: &MirProgram,
    artifact_id: MirArtifactId,
    argv: &[String],
) -> Result<(), String> {
    let entry = program
        .artifacts
        .iter()
        .find(|artifact| artifact.id == artifact_id)
        .and_then(|artifact| artifact.entry.as_ref());
    let Some(cli) = entry.and_then(|entry| entry.cli.as_ref()) else {
        return if argv.len() <= 1 {
            Ok(())
        } else {
            Err("job takes no arguments".to_string())
        };
    };
    let version = entry
        .map(|entry| entry.package_version.clone())
        .filter(|version| !version.is_empty());
    let prog = argv.first().map(String::as_str).unwrap_or("program");
    let spec = if cli.commands.is_empty() {
        build_spec(
            &cli.inputs,
            cli.description.as_deref(),
            cli.standard,
            version.as_deref(),
            prog,
        )?
    } else {
        build_command_spec(cli, version.as_deref(), prog)?.0
    };
    parse(&spec, argv).map(|_| ())
}

fn cli_value_kind(input: &MirCliInput) -> MirCliValueKind {
    match &input.shape {
        MirCliInputShape::Flag => MirCliValueKind::Bool,
        MirCliInputShape::Value { kind, .. } => *kind
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
                    None => flag(spec, &flag_name, &help)
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
        | MirTypeKind::Measure(_) => None
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
        MirConstant::SharedSite(_) => panic!("publication site nested in CLI default"),
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
            jet_foundation::MIR::MirConstReport::Told(value) => constant_text(value)
        }
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
        None => Ok(None)
    }
}

// `jet_jit_cli_main`'s registration + import lives in the top-level
// `HostFns` table (jit/runtime_host.rs) — the CLI trampoline is present
// on every JIT module (like every other host symbol), not only for
// `cli_entry` programs, so `host_fns_audit` sees a matching pair on every
// `new_jit_module()` call instead of only when compiling a CLI program.
