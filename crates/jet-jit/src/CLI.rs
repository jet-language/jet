//! Typed CLI entry adapter (#1219) — CLISchema + canonical Args parser.
//! The zero-argument `jet_jit_cli_main` trampoline decodes argv and calls user
//! `run(args)`.

use super::Concurrency;
use jet_foundation::AST::{CtValue, Item, ProgramBundle, StructDef, Type};
use jet_foundation::CLISchema::{
    self, CLICommandSchema, CLIDefault, CLIInputSchema, CLIInputShape, CLIValueKind,
};
use std::cell::RefCell;
use std::sync::atomic::{AtomicPtr, Ordering};
use crate::Marshal::alloc_string;

#[allow(dead_code, unused_imports, clippy::all)]
mod runtime {
    use super::{CLIValueKind, Concurrency};
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
        env: Option<&str>,
        kind: CLIValueKind,
    ) -> Spec {
        let value = match kind {
            CLIValueKind::Int => JetArgValueKind::Int,
            CLIValueKind::Float => JetArgValueKind::Float,
            CLIValueKind::String | CLIValueKind::Path => JetArgValueKind::String,
            CLIValueKind::Bool => JetArgValueKind::String,
        };
        Spec(jet_args_option_base(
            spec.0,
            &name.to_string(),
            short.map(str::to_string),
            &help.to_string(),
            &meta.to_string(),
            None,
            env.map(str::to_string),
            false,
            false,
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

    pub(super) fn help_text(spec: &Spec) -> String {
        spec.0.help()
    }

    pub(super) fn flag_set(parsed: &Parsed, name: &str) -> bool {
        jet_parsed_flag(&parsed.0, &name.to_string())
    }

    pub(super) fn option_val(parsed: &Parsed, name: &str) -> Option<String> {
        jet_parsed_option(&parsed.0, &name.to_string()).ok()
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
    empty_spec, flag, flag_set, flag_short, help_text, option, option_val, parse, positional,
    standard_color_mode, standard_log_level, Parsed, Spec,
};

mod inline_range_semantics {
    #![allow(dead_code, unused_imports)]
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
    pub schema: CLICommandSchema,
    /// Field types for the entry struct, or the direct `run` parameters.
    pub field_types: Vec<(String, Type)>,
    /// Canonical callable members. Function names are TIR keys; method
    /// commands carry the root record as their first ABI argument.
    pub commands: Vec<CLICommandPlan>,
    /// The CLI frame passed to the `run` adapter is already the entry record.
    pub run_record: bool,
    /// The typed entry's ABI carries a non-unit return value.
    pub run_returns_value: bool,
    pub user_run: String,
}

#[derive(Clone)]
pub(crate) struct CLICommandPlan {
    pub name: String,
    pub function: String,
    pub method: bool,
    pub arg_types: Vec<Type>,
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

pub(crate) fn install_cli_command_ptr(function: &str, ptr: *const u8) {
    CLI_PLAN.with(|slot| {
        let mut plan_slot = slot.borrow_mut();
        let Some(plan) = plan_slot.as_mut() else {
            return;
        };
        if let Some(command) = plan
            .commands
            .iter_mut()
            .find(|command| command.function.as_str() == function)
        {
            command.ptr = Some(ptr);
        }
    });
}

pub(crate) fn cli_function_targets() -> Vec<String> {
    CLI_PLAN.with(|slot| {
        let plan_slot = slot.borrow();
        let Some(plan) = plan_slot.as_ref() else {
            return Vec::new();
        };
        let mut targets = vec![plan.user_run.clone()];
        targets.extend(plan.commands.iter().map(|command| command.function.clone()));
        targets.sort();
        targets.dedup();
        targets
    })
}

pub(crate) fn cli_run_requires_adapter() -> bool {
    CLI_PLAN.with(|slot| slot.borrow().is_some())
}

pub(crate) fn cli_run_frame_is_value() -> bool {
    CLI_PLAN.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|plan| plan.run_record)
    })
}

pub(crate) fn prepare_cli_from_bundle(bundle: &ProgramBundle) {
    clear_cli_plan();
    let Some(module) = bundle.modules.get(bundle.entry) else {
        return;
    };
    let Some(cli_module) = CLISchema::entry_type_module(bundle) else {
        return;
    };
    let Some(schema) = CLISchema::entry_schema_for_bundle(bundle) else {
        return;
    };
    let Some(cli_items) = bundle
        .modules
        .get(cli_module)
        .map(|module| module.items.as_slice())
    else {
        return;
    };
    let entry_leaf = schema.entry_type.rsplit('.').next().unwrap_or(&schema.entry_type);
    let type_identity = (!CLISchema::is_direct_run_entry(
        &bundle.modules[bundle.entry].items,
    ))
        .then(|| {
            if cli_module == bundle.entry {
                Some(entry_leaf.to_string())
            } else {
                bundle
                    .name_ledger
                    .module_identity(cli_module)
                    .map(|owner| format!("{owner}::{entry_leaf}"))
            }
        })
        .flatten();
    if let Some(plan) = cli_plan_from_schema(
        schema,
        &module.items,
        cli_items,
        type_identity.as_deref(),
    ) {
        install_cli_plan(plan);
    }
}

pub(crate) fn cli_plan_from_items(items: &[Item]) -> Option<CLIPlan> {
    let schema = CLISchema::entry_schema(items)?;
    cli_plan_from_schema(schema, items, items, None)
}

fn cli_plan_from_schema(
    schema: CLICommandSchema,
    entry_items: &[Item],
    cli_items: &[Item],
    type_identity: Option<&str>,
) -> Option<CLIPlan> {
    let entry = schema.entry_type.clone();
    let run_returns_value = cli_run_returns_value(entry_items);
    let entry_leaf = entry.rsplit('.').next().unwrap_or(&entry);
    if !schema.commands.is_empty() {
        return cli_plan_from_struct_schema(
            schema,
            entry_items,
            cli_items,
            entry_leaf,
            type_identity,
            run_returns_value,
        );
    }
    if entry_leaf == "run" && CLISchema::is_direct_run_entry(entry_items) {
        let function = entry_items.iter().find_map(|item| match item {
            Item::Func(function) if function.name == "run" => Some(function),
            _ => None,
        })?;
        let field_types = function
            .params
            .iter()
            .filter(|param| param.name != jet_foundation::Syntax::KW_SELF)
            .map(|param| (param.name.clone(), param.ty.clone()))
            .collect();
        return Some(CLIPlan {
            schema,
            field_types,
            commands: Vec::new(),
            run_record: false,
            run_returns_value,
            user_run: "run".to_string(),
        });
    }
    cli_plan_from_struct_schema(
        schema,
        entry_items,
        cli_items,
        entry_leaf,
        type_identity,
        run_returns_value,
    )
}

fn cli_plan_from_struct_schema(
    schema: CLICommandSchema,
    _entry_items: &[Item],
    cli_items: &[Item],
    entry: &str,
    type_identity: Option<&str>,
    run_returns_value: bool,
) -> Option<CLIPlan> {
    let field_types = struct_fields(cli_items, entry)?;
    let structure = cli_items.iter().find_map(|item| match item {
        Item::Struct(structure) if structure.name == entry => Some(structure),
        _ => None,
    })?;
    let function_owner = type_identity.unwrap_or(entry);
    let commands = schema
        .commands
        .iter()
        .map(|command| {
            let target = jet_foundation::CLISchema::command_target(
                structure,
                command,
                cli_items,
                function_owner,
            )?;
            let method = target.is_method || target.bound_shared;
            let mut arg_types = if method {
                vec![Type::Named(function_owner.to_string())]
            } else {
                Vec::new()
            };
            arg_types.extend(
                target
                    .payload_params(&structure.name)
                    .into_iter()
                    .map(|param| param.ty),
            );
            Some(CLICommandPlan {
                name: command.name.clone(),
                function: target.function_name,
                method,
                arg_types,
                ptr: None,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(CLIPlan {
        schema,
        field_types,
        commands,
        run_record: true,
        run_returns_value,
        user_run: "run".to_string(),
    })
}

fn cli_run_returns_value(items: &[Item]) -> bool {
    match items
        .iter()
        .find_map(|item| match item {
            Item::Func(function) if function.name == "run" => {
                function.return_type.as_ref()
            }
            _ => None,
        })
    {
        Some(Type::Named(name)) if name == "Unit" => false,
        Some(_) => true,
        None => false,
    }
}

fn struct_fields(items: &[Item], name: &str) -> Option<Vec<(String, Type)>> {
    let s: &StructDef = items.iter().find_map(|item| match item {
        Item::Struct(s) if s.name == name => Some(s),
        _ => None,
    })?;
    Some(
        s.fields
            .iter()
            .filter(|f| f.computed.is_none())
            .map(|f| (f.name.clone(), f.ty.clone()))
            .collect(),
    )
}

fn alloc_path_record(path: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(1);
        let string = rt.heap.alloc_string(path);
        let _ = rt.heap.record_set_string(record, 0, string);
        record
    })
}

fn build_spec(
    inputs: &[CLIInputSchema],
    description: Option<&str>,
    standard: bool,
    version: Option<&str>,
    prog: &str,
) -> Spec {
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
        let flag_name = input.flag.clone();
        let help = input.builder_help();
        match &input.shape {
            CLIInputShape::Flag => {
                spec = match &input.short {
                    Some(short) => flag_short(spec, &flag_name, short, &help),
                    None => flag(spec, &flag_name, &help),
                };
            }
            CLIInputShape::Value { .. } => {
                let meta = input
                    .metavar
                    .clone()
                    .unwrap_or_else(|| "VALUE".to_string());
                spec = option(
                    spec,
                    &flag_name,
                    input.short.as_deref(),
                    &help,
                    &meta,
                    input.env.as_deref(),
                    input.value_kind(),
                );
                if input.positional.is_some() {
                    spec = positional(spec, &flag_name, &help);
                }
            }
        }
    }
    spec
}

fn build_command_spec(
    schema: &CLICommandSchema,
    prog: &str,
) -> (Spec, Vec<(String, Spec)>) {
    let mut root = build_spec(
        &schema.inputs,
        schema.description.as_deref(),
        schema.standard,
        schema.version.as_deref(),
        prog,
    );
    let mut commands = Vec::new();
    for command in &schema.commands {
        let nested_prog = format!("{} {}", jet_args_source_program_name(prog), command.name);
        let nested = build_spec(
            &command.inputs,
            command.description.as_deref(),
            false,
            None,
            &nested_prog,
        );
        root = runtime::subcommand_spec(
            root,
            &command.name,
            &command.description.clone().unwrap_or_default(),
            nested.clone(),
        );
        commands.push((command.name.clone(), nested));
    }
    (root, commands)
}

fn decode_struct(
    inputs: &[CLIInputSchema],
    field_types: &[(String, Type)],
    parsed: &Parsed,
    spec: &Spec,
) -> Result<i64, String> {
    decode_frame(inputs, field_types, parsed, spec, None)
}

fn decode_frame(
    inputs: &[CLIInputSchema],
    field_types: &[(String, Type)],
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
            .find(|i| i.field == *fname)
            .ok_or_else(|| format!("missing CLI input for `{fname}`"))?;
        let flag_name = &input.flag;
        let bits = match (&input.shape, fty) {
            (CLIInputShape::Flag, Type::Bool) => i64::from(flag_set(parsed, flag_name)),
            (
                CLIInputShape::Value {
                    kind: CLIValueKind::Bool,
                    optional: true,
                    ..
                },
                Type::Option(inner),
            ) if matches!(inner.as_ref(), Type::Bool) => match option_val(parsed, flag_name) {
                Some(value) => match value.to_ascii_lowercase().as_str() {
                    "true" => 2,
                    "false" => 1,
                    _ => return Err(format!("invalid bool for --{flag_name}")),
                },
                None => 0,
            },
            (
                CLIInputShape::Value {
                    kind: CLIValueKind::Int,
                    optional: true,
                    ..
                },
                Type::Option(inner),
            ) if matches!(inner.as_ref(), Type::Int) => match option_val(parsed, flag_name) {
                Some(value) => value
                    .parse::<i64>()
                    .map(|value| value.wrapping_add(1))
                    .map_err(|_| format!("invalid int for --{flag_name}"))?,
                None => 0,
            },
            (
                CLIInputShape::Value {
                    kind: CLIValueKind::Float,
                    optional: true,
                    ..
                },
                Type::Option(inner),
            ) if matches!(inner.as_ref(), Type::Float) => match option_val(parsed, flag_name) {
                Some(value) => value
                    .parse::<f64>()
                    .map(|value| (value.to_bits() as i64).wrapping_add(1))
                    .map_err(|_| format!("invalid float for --{flag_name}"))?,
                None => 0,
            },
            (
                CLIInputShape::Value {
                    kind: CLIValueKind::Float,
                    optional: false,
                    default,
                },
                Type::Float,
            ) => match option_val(parsed, flag_name) {
                Some(v) => v
                    .parse::<f64>()
                    .map(f64::to_bits)
                    .map(|bits| bits as i64)
                    .map_err(|_| format!("invalid float for --{flag_name}"))?,
                None => match default {
                    Some(CLIDefault::Value(CtValue::Float(value))) => value.as_f64().to_bits() as i64,
                    Some(CLIDefault::TypeDefault) => 0.0f64.to_bits() as i64,
                    Some(CLIDefault::Value(other)) => other
                        .jet_show()
                        .parse::<f64>()
                        .map(f64::to_bits)
                        .map(|bits| bits as i64)
                        .map_err(|_| format!("bad default for --{flag_name}"))?,
                    Some(CLIDefault::Recorded(value)) => value
                        .parse::<f64>()
                        .map(f64::to_bits)
                        .map(|bits| bits as i64)
                        .map_err(|_| format!("bad default for --{flag_name}"))?,
                    None if input.positional.is_some() => {
                        return Err(format!(
                            "missing required argument {flag_name}\n\n{}",
                            help_text(spec)
                        ));
                    }
                    None => {
                        return Err(format!(
                            "missing required flag --{flag_name}\n\n{}",
                            help_text(spec)
                        ));
                    }
                },
            },
            (
                CLIInputShape::Value {
                    kind: CLIValueKind::Int,
                    optional: false,
                    default,
                },
                Type::Int,
            ) => match option_val(parsed, flag_name) {
                Some(v) => Concurrency::with_runtime_mut(|rt| rt.heap.int_from_str(v.trim()).ok())
                    .ok_or_else(|| format!("invalid int for --{flag_name}"))?,
                None => match default {
                    Some(CLIDefault::Value(CtValue::Int(n))) => *n,
                    Some(CLIDefault::TypeDefault) => 0,
                    Some(CLIDefault::Value(other)) => {
                        let text = other.jet_show();
                        Concurrency::with_runtime_mut(|rt| rt.heap.int_from_str(text.trim()).ok())
                            .ok_or_else(|| format!("bad default for --{flag_name}"))?
                    }
                    Some(CLIDefault::Recorded(s)) => {
                        Concurrency::with_runtime_mut(|rt| rt.heap.int_from_str(s.trim()).ok())
                            .ok_or_else(|| format!("bad default for --{flag_name}"))?
                    }
                    None if input.positional.is_some() => {
                        return Err(format!(
                            "missing required argument {flag_name}\n\n{}",
                            help_text(spec)
                        ));
                    }
                    None => {
                        return Err(format!(
                            "missing required flag --{flag_name}\n\n{}",
                            help_text(spec)
                        ));
                    }
                },
            },
            (
                CLIInputShape::Value {
                    kind: CLIValueKind::Int,
                    optional: false,
                    default,
                },
                Type::InlineRange { lo, hi, .. },
            ) => {
                let value = match option_val(parsed, flag_name) {
                    Some(v) => v
                        .trim()
                        .parse::<i64>()
                        .map_err(|_| format!("invalid int for --{flag_name}"))?,
                    None => match default {
                        Some(CLIDefault::Value(CtValue::Int(n))) => *n,
                        Some(CLIDefault::TypeDefault) => *lo,
                        Some(CLIDefault::Value(other)) => other
                            .jet_show()
                            .trim()
                            .parse::<i64>()
                            .map_err(|_| format!("bad default for --{flag_name}"))?,
                        Some(CLIDefault::Recorded(s)) => s
                            .trim()
                            .parse::<i64>()
                            .map_err(|_| format!("bad default for --{flag_name}"))?,
                        None if input.positional.is_some() => {
                            return Err(format!(
                                "missing required argument {flag_name}\n\n{}",
                                help_text(spec)
                            ));
                        }
                        None => {
                            return Err(format!(
                                "missing required flag --{flag_name}\n\n{}",
                                help_text(spec)
                            ));
                        }
                    },
                };
                inline_range_semantics::jet_inline_range_from_int(value, *lo, *hi)
                    .map_err(|reason| format!("invalid value for --{flag_name}: {reason}"))?
            },
            (
                CLIInputShape::Value {
                    kind: CLIValueKind::String | CLIValueKind::Path,
                    optional: false,
                    default,
                },
                Type::String | Type::Named(_),
            ) => {
                let text = match option_val(parsed, flag_name) {
                    Some(v) => v,
                    None => match default {
                        Some(CLIDefault::Value(CtValue::Str(s))) => s.clone(),
                        Some(CLIDefault::TypeDefault) => String::new(),
                        Some(CLIDefault::Value(other)) => other.jet_show(),
                        Some(CLIDefault::Recorded(s)) => s.clone(),
                        None if input.positional.is_some() => {
                            return Err(format!(
                                "missing required argument {flag_name}\n\n{}",
                                help_text(spec)
                            ));
                        }
                        None => {
                            return Err(format!(
                                "missing required flag --{flag_name}\n\n{}",
                                help_text(spec)
                            ));
                        }
                    },
                };
                if matches!(fty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_PATH) {
                    alloc_path_record(text)
                } else {
                    alloc_string(text)
                }
            }
            (
                CLIInputShape::Value {
                    kind: CLIValueKind::String | CLIValueKind::Path,
                    optional: true,
                    ..
                },
                Type::Option(_),
            ) => match option_val(parsed, flag_name) {
                Some(v) => {
                    let value = if matches!(fty, Type::Option(inner) if matches!(inner.as_ref(), Type::Named(name) if name == jet_foundation::Syntax::TYPE_PATH)) {
                        alloc_path_record(v)
                    } else {
                        alloc_string(v)
                    };
                    value.wrapping_add(1)
                }
                None => 0,
            },
            _ => {
                return Err(format!(
                    "jit CLI decode unsupported field `{fname}`"
                ));
            }
        };
        Concurrency::with_runtime_mut(|rt| {
            let index = (idx + offset) as i64;
            if matches!(fty, Type::Float) {
                let _ = rt
                    .heap
                    .record_set_float(rec, index, f64::from_bits(bits as u64));
            } else if matches!(fty, Type::Bool) {
                let _ = rt.heap.record_set_bool(rec, index, bits != 0);
            } else if matches!(fty, Type::String) {
                let _ = rt.heap.record_set_string(rec, index, bits);
            } else {
                let _ = rt.heap.record_set_int(rec, index, bits);
            }
        });
    }
    Ok(rec)
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
    let Some(version) = plan.schema.version.as_deref() else {
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
    let argv = crate::program_args();
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
        let (spec, command_specs) = build_command_spec(&plan.schema, prog);
        let parsed = match parse(&spec, &argv) {
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
        let type_offset = usize::from(command.method);
        let command_types: Vec<(String, Type)> = command_schema
            .inputs
            .iter()
            .zip(command.arg_types.iter().skip(type_offset))
            .map(|(input, ty)| (input.field.clone(), ty.clone()))
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
            report_cli_error(&format!("jit CLI: command `{command_name}` pointer missing"));
            return 0;
        };
        let call: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(ptr) };
        return call(frame);
    }

    // Struct or parameter-direct typed entry.
    let spec = build_spec(
        &plan.schema.inputs,
        plan.schema.description.as_deref(),
        plan.schema.standard,
        plan.schema.version.as_deref(),
        prog,
    );
    let parsed = match parse(&spec, &argv) {
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
