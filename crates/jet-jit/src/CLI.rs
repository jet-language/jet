//! Typed CLI entry adapter (#1219) — CLISchema + canonical Args parser.
//! Zero-arg `jet_jit_cli_main` decodes argv and calls user `run(args)`.

use super::Concurrency;
use cranelift_codegen::ir::Signature;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use jet_foundation::AST::{CtValue, Item, ProgramBundle, StructDef, Type, VariantPayload};
use jet_foundation::CLISchema::{
    self, CLICommandSchema, CLIDefault, CLIInputSchema, CLIInputShape, CLIValueKind,
};
use std::cell::RefCell;
use std::sync::atomic::{AtomicPtr, Ordering};

#[allow(dead_code, unused_imports, clippy::all)]
mod runtime {
    use super::Concurrency;

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

    pub(super) fn program_name(prog: &str) -> String {
        jet_args_program_name(prog)
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
    ) -> Spec {
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
            JetArgValueKind::String,
        ))
    }

    pub(super) fn positional(spec: Spec, name: &str, help: &str) -> Spec {
        Spec(jet_args_positional(
            spec.0,
            &name.to_string(),
            &help.to_string(),
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
}

use runtime::{
    empty_spec, flag, flag_set, flag_short, help_text, option, option_val, parse, positional,
    program_name, Parsed, Spec,
};

#[derive(Clone)]
pub(crate) struct CLIPlan {
    pub schema: CLICommandSchema,
    /// Field types for the entry struct (struct CLI) or empty (enum CLI).
    pub field_types: Vec<(String, Type)>,
    /// Enum variant order: (variant_name_lower, payload_struct_fields).
    pub variants: Vec<(String, Vec<(String, Type)>)>,
    pub user_run: String,
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

pub(crate) fn prepare_cli_from_bundle(bundle: &ProgramBundle) {
    clear_cli_plan();
    let Some(module) = bundle.modules.get(bundle.entry) else {
        return;
    };
    if let Some(plan) = cli_plan_from_items(&module.items) {
        install_cli_plan(plan);
    }
}

pub(crate) fn cli_plan_from_items(items: &[Item]) -> Option<CLIPlan> {
    let schema = CLISchema::entry_schema(items)?;
    let entry = schema.entry_type.clone();
    if !schema.commands.is_empty() {
        let enumeration = items.iter().find_map(|item| match item {
            Item::Enum(e) if e.name == entry => Some(e),
            _ => None,
        })?;
        let mut variants = Vec::new();
        for v in &enumeration.variants {
            let VariantPayload::Single(Type::Named(payload), _) = &v.payload else {
                continue;
            };
            let fields = struct_fields(items, payload)?;
            variants.push((v.name.to_lowercase(), fields));
        }
        return Some(CLIPlan {
            schema,
            field_types: Vec::new(),
            variants,
            user_run: "run".to_string(),
        });
    }
    let field_types = struct_fields(items, &entry)?;
    Some(CLIPlan {
        schema,
        field_types,
        variants: Vec::new(),
        user_run: "run".to_string(),
    })
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

fn alloc_str(s: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s))
}

fn build_spec(inputs: &[CLIInputSchema], prog: &str) -> Spec {
    let mut spec = empty_spec(prog);
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
                );
                if input.positional.is_some() {
                    spec = positional(spec, &flag_name, &help);
                }
            }
        }
    }
    spec
}

fn decode_struct(
    inputs: &[CLIInputSchema],
    field_types: &[(String, Type)],
    parsed: &Parsed,
    spec: &Spec,
) -> Result<i64, String> {
    let n = field_types.len();
    let rec = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_record(n));
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
                    kind: CLIValueKind::Int,
                    optional: false,
                    default,
                },
                Type::Int,
            ) => match option_val(parsed, flag_name) {
                Some(v) => v
                    .parse::<i64>()
                    .map_err(|_| format!("invalid int for --{flag_name}"))?,
                None => match default {
                    Some(CLIDefault::Value(CtValue::Int(n))) => *n,
                    Some(CLIDefault::TypeDefault) => 0,
                    Some(CLIDefault::Value(other)) => other
                        .jet_show()
                        .parse()
                        .map_err(|_| format!("bad default for --{flag_name}"))?,
                    Some(CLIDefault::Recorded(s)) => s
                        .parse()
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
                alloc_str(text)
            }
            (
                CLIInputShape::Value {
                    kind: CLIValueKind::String | CLIValueKind::Path,
                    optional: true,
                    ..
                },
                Type::Option(_),
            ) => match option_val(parsed, flag_name) {
                Some(v) => alloc_str(v).wrapping_add(1),
                None => 0,
            },
            _ => {
                return Err(format!(
                    "jit CLI decode unsupported field `{fname}`"
                ));
            }
        };
        Concurrency::with_runtime_mut(|rt| {
            if matches!(fty, Type::Bool) {
                let _ = rt.heap.record_set_bool(rec, idx as i64, bits != 0);
            } else {
                let _ = rt.heap.record_set_int(rec, idx as i64, bits);
            }
        });
    }
    Ok(rec)
}

fn print_usage(schema: &CLICommandSchema, prog: &str) {
    let mut out = format!(
        "Usage: {} <command> [options]\n\nCommands:\n",
        program_name(prog)
    );
    for cmd in &schema.commands {
        out.push_str("  ");
        out.push_str(&cmd.name);
        out.push('\n');
    }
    Concurrency::with_runtime_mut(|rt| {
        rt.stdout.push_str(&out);
    });
}

/// Zero-arg trampoline installed as `jet_jit_cli_main` for typed CLI programs.
pub(crate) extern "C" fn jet_jit_cli_main() {
    let plan = CLI_PLAN.with(|slot| slot.borrow().clone());
    let Some(plan) = plan else {
        Concurrency::with_runtime_mut(|rt| {
            rt.stderr.push_str("jit CLI: no plan installed\n");
        });
        return;
    };
    let argv = crate::program_args();
    let run_ptr = CLI_RUN_PTR.load(Ordering::SeqCst);
    if run_ptr.is_null() {
        Concurrency::with_runtime_mut(|rt| {
            rt.stderr.push_str("jit CLI: run pointer missing\n");
        });
        return;
    }
    let run: extern "C" fn(i64) = unsafe { std::mem::transmute(run_ptr) };

    if !plan.variants.is_empty() {
        // Enum subcommands — bare / --help prints usage and exits 0.
        if argv.len() < 2 || argv[1] == "--help" {
            print_usage(
                &plan.schema,
                argv.first().map(String::as_str).unwrap_or(""),
            );
            return;
        }
        let sub = argv[1].to_lowercase();
        let Some((disc, fields)) = plan
            .variants
            .iter()
            .enumerate()
            .find(|(_, (name, _))| name == &sub)
            .map(|(i, (_, f))| (i as i64, f))
        else {
            Concurrency::with_runtime_mut(|rt| {
                rt.stderr.push_str(&format!("unknown command `{sub}`\n"));
            });
            return;
        };
        let cmd_schema = plan
            .schema
            .commands
            .iter()
            .find(|c| c.name == sub)
            .expect("schema command");
        let nested_prog = format!("{} {}", argv[0], sub);
        let mut rest = vec![nested_prog.clone()];
        rest.extend_from_slice(&argv[2..]);
        let spec = build_spec(&cmd_schema.inputs, &nested_prog);
        let parsed = match parse(&spec, &rest) {
            Ok(p) => p,
            Err(e) => {
                Concurrency::with_runtime_mut(|rt| {
                    rt.stderr.push_str(&e);
                    rt.stderr.push('\n');
                });
                return;
            }
        };
        if flag_set(&parsed, "help") {
            Concurrency::with_runtime_mut(|rt| {
                rt.stdout.push_str(&help_text(&spec));
                rt.stdout.push('\n');
            });
            return;
        }
        let payload = match decode_struct(&cmd_schema.inputs, fields, &parsed, &spec) {
            Ok(h) => h,
            Err(e) => {
                Concurrency::with_runtime_mut(|rt| {
                    rt.stderr.push_str(&e);
                    rt.stderr.push('\n');
                });
                return;
            }
        };
        let packed = (payload << 8) | (disc & 0xff);
        run(packed);
        return;
    }

    // Struct typed entry.
    let prog = argv.first().map(String::as_str).unwrap_or("program");
    let spec = build_spec(&plan.schema.inputs, prog);
    let parsed = match parse(&spec, &argv) {
        Ok(p) => p,
        Err(e) => {
            Concurrency::with_runtime_mut(|rt| {
                rt.stderr.push_str(&e);
                rt.stderr.push('\n');
            });
            return;
        }
    };
    if flag_set(&parsed, "help") {
        Concurrency::with_runtime_mut(|rt| {
            rt.stdout.push_str(&help_text(&spec));
            rt.stdout.push('\n');
        });
        return;
    }
    let args = match decode_struct(&plan.schema.inputs, &plan.field_types, &parsed, &spec) {
        Ok(h) => h,
        Err(e) => {
            Concurrency::with_runtime_mut(|rt| {
                rt.stderr.push_str(&e);
                rt.stderr.push('\n');
            });
            return;
        }
    };
    run(args);
}

pub(crate) fn register_cli_symbols(builder: &mut JITBuilder) {
    builder.symbol("jet_jit_cli_main", jet_jit_cli_main as *const u8);
}

pub(crate) fn declare_cli_main_import(module: &mut JITModule) -> Result<FuncId, String> {
    let cc = module.target_config().default_call_conv;
    let sig = Signature::new(cc);
    module
        .declare_function("jet_jit_cli_main", Linkage::Import, &sig)
        .map_err(|e| e.to_string())
}
