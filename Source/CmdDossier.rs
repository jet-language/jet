//! D-WD2/D-DOSSIER1: `jet inspect dossier` — one explainable view over semantic facts.

use std::path::{Path, PathBuf};
use std::process::exit;

use jet::ExitCodes;
use jet_foundation::Report::{StatusEnvelope, StatusFields, StatusValue};
/// D-CONF-MODULE1=A: explain a generic-module member's specialization input
/// from the semantic index, including the profile/declaration chain for a
/// build-fact value.
pub(crate) fn run_module_explain(subject: &str, file: &str, profile: &str, json: bool) {
    let abs = absolutize(file);
    let module_name = subject.split('.').next().unwrap_or(subject);
    let checked = crate::CmdInspect::check_projection_with_options(
        &abs,
        jet::Policy::GateSet::default(),
        profile,
        &std::collections::BTreeMap::new(),
    )
    .unwrap_or_else(|diagnostics| {
        crate::CmdInspect::render_check_failure(&abs, &diagnostics, json, false);
    });
    let index = &checked.index;
    let Some(instance) = index.instances().iter().find(|instance| {
        instance.name == module_name
            || instance
                .applications
                .iter()
                .any(|application| application.name == module_name)
    }) else {
        crate::cli_error!(@fix "E2104", format!("generic module `{module_name}` is not present in `{file}`"), "pass the instantiated member as `module.member` and the source entry file");
        exit(ExitCodes::USER_ERROR);
    };
    if json {
        let arguments = StatusValue::array(
            instance
                .argument_values
                .iter()
                .zip(&instance.argument_provenance)
                .map(|(value, sources)| {
                    StatusValue::object(
                        StatusFields::new()
                            .with("value", value.as_str())
                            .with(
                                "provenance",
                                StatusValue::array(
                                    sources
                                        .iter()
                                        .map(|source| StatusValue::from(source.as_str())),
                                ),
                            ),
                    )
                }),
        );
        let generic_module = StatusValue::object(
            StatusFields::new()
                .with("subject", subject)
                .with("module", instance.name.as_str())
                .with("fingerprint", instance.fingerprint.as_str())
                .with("arguments", arguments)
                .with("check", crate::CmdInspect::check_result_value(&checked.check)),
        );
        println!(
            "{}",
            StatusEnvelope::new("inspect.generic_module", true)
                .with_field("generic_module", generic_module)
                .json()
        );
        return;
    }
    println!("generic module `{subject}`");
    println!("  instance: {}", instance.name);
    println!("  fingerprint: {}", instance.fingerprint);
    for (value, sources) in instance
        .argument_values
        .iter()
        .zip(&instance.argument_provenance)
    {
        println!("  argument: {value}");
        for source in sources {
            println!("    from: {source}");
        }
    }
    print!("{}", crate::CmdInspect::check_result_text(&checked.check));
}

pub(crate) fn run_dossier(args: &[String], json: bool, profile: &str) {
    let mut positional: Vec<&str> = Vec::new();
    for a in args {
        if !a.starts_with('-') {
            positional.push(a.as_str());
        }
    }

    // D-TARGET-AUDIT1=A: `jet inspect dossier target <machine>`
    if positional.first().copied() == Some("target") {
        let name = positional.get(1).copied().unwrap_or("board.sensor_v1");
        match jet::Driver::target_machine_dossier_value(name) {
            Ok(target) => {
                if json {
                    println!(
                        "{}",
                        StatusEnvelope::new("inspect.target", true)
                            .with_field("target", target)
                            .json()
                    );
                } else {
                    println!("target machine: {name}");
                    println!("{}", jet::Driver::target_machine_dossier_json(name).unwrap_or_default());
                }
            }
            Err(msg) => {
                crate::cli_error!(@fix "E2105", msg, "jet inspect dossier target board.sensor_v1 (or board.virt_aarch64)");
                exit(ExitCodes::USER_ERROR);
            }
        }
        return;
    }

    // D-DATA-STATUS1=A: `jet inspect dossier data` — human/JSON lens over
    // the same bridge/native status rows as `data.status()`.
    if positional.first().copied() == Some("data") {
        render_data_status_dossier(json);
        return;
    }

    // D-FFI-UNIFY1: the capability report is derived from the same descriptor
    // table used by import routing and cache/link planning.
    if positional.first().copied() == Some("ffi") {
        if json {
            println!("{}", jet::Foreign::capability_report_json());
        } else {
            print!("{}", jet::Foreign::capability_report_text());
        }
        return;
    }

    let (path, target) = match positional.as_slice() {
        [path] => (*path, None),
        [path, target] => (*path, Some(*target)),
        _ => {
            crate::cli_error!(@fix "E2104", "`jet inspect dossier` needs an entry file and optional symbol", "jet inspect dossier examples/features/basics/hello.jet run; use `target board.sensor_v1`, `data`, or `ffi` for those dossiers");
            exit(ExitCodes::USER_ERROR);
        }
    };

    let abs = absolutize(path);
    let checked = crate::CmdInspect::check_projection_with_options(
        &abs,
        jet::Policy::GateSet::default(),
        profile,
        &std::collections::BTreeMap::new(),
    )
    .unwrap_or_else(|diagnostics| {
        crate::CmdInspect::render_check_failure(&abs, &diagnostics, json, false);
    });
    let target = target.unwrap_or_else(|| {
        checked
            .index
            .definitions()
            .iter()
            .find(|d| matches!(d.kind, jet_semindex::SymbolKind::Struct { .. }))
            .or_else(|| checked.index.definitions().iter().find(|d| d.name == "run"))
            .map(|d| d.name.as_str())
            .unwrap_or("run")
    });
    let dossier = checked.index.dossier(target);
    let (budgets, command, allocator) = auxiliary_projections(&abs, &checked.bundle);
    let target_dossier = jet::TargetMachine::target_dossier_value(
        &checked.bundle.build_facts.target_dossier,
        &checked.bundle.build_facts.target_triple,
    );
    if json {
        let envelope = dossier
            .to_status_envelope()
            .with_field("target_dossier", target_dossier)
            .with_field("program_allocator", allocator.audit_value())
            .with_field("performance_budgets", budgets.to_status_value())
            .with_field("command_schema", command_value(command.as_ref()))
            .with_field("check", crate::CmdInspect::check_result_value(&checked.check));
        println!("{}", envelope.json());
    } else {
        print!("{}", dossier.render_text());
        println!(
            "target dossier: {}",
            jet::TargetMachine::target_dossier_json(
                &checked.bundle.build_facts.target_dossier,
                &checked.bundle.build_facts.target_triple,
            )
        );
        print!("{}", allocator_text(&allocator));
        print!("{}", command_text(command.as_ref()));
        print!("{}", budgets.render_text());
        print!("{}", crate::CmdInspect::check_result_text(&checked.check));
    }
    if dossier.definition.is_none() {
        exit(ExitCodes::USER_ERROR);
    }
}

fn auxiliary_projections(
    entry: &Path,
    bundle: &jet::AST::ProgramBundle,
) -> (
    jet::BudgetView::BudgetProjection,
    Option<jet_foundation::CLISchema::CLICommandSchema>,
    jet::TargetMachine::AllocatorPolicy,
) {
    let command = entry_command_schema(bundle);
    let root = jet::Loader::find_manifest_root(entry.parent().unwrap_or(Path::new(".")))
        .unwrap_or_else(|| entry.parent().unwrap_or(Path::new(".")).to_path_buf());
    let sources = bundle
        .modules
        .iter()
        .map(|module| {
            let path = module
                .path
                .strip_prefix(&root)
                .unwrap_or(&module.path)
                .to_string_lossy()
                .replace('\\', "/");
            (path, jet::SHA256::sha256_hex(module.source.as_bytes()))
        })
        .collect::<Vec<_>>();
    (
        jet::BudgetView::read_compatible(&root, &sources),
        command,
        bundle.program_allocator.clone(),
    )
}

fn allocator_text(allocator: &jet::TargetMachine::AllocatorPolicy) -> String {
    use jet::TargetMachine::AllocatorPolicy;
    match allocator {
        AllocatorPolicy::Counting { cap: Some(cap) } => {
            format!(
                "program allocator: counting(system), cap={} bytes\n",
                cap.bytes
            )
        }
        AllocatorPolicy::Counting { cap: None } => {
            "program allocator: counting(system), uncapped\n".to_string()
        }
        _ => "program allocator: hidden system heap\n".to_string(),
    }
}

fn entry_command_schema(
    bundle: &jet::AST::ProgramBundle,
) -> Option<jet_foundation::CLISchema::CLICommandSchema> {
    let items = &bundle.modules.get(bundle.entry)?.items;
    items.iter().find_map(|item| match item {
        jet::AST::Item::Func(function) if function.name == "run" && !function.params.is_empty() => {
            Some(())
        }
        _ => None,
    })?;
    Some(jet_foundation::CLISchema::executable_schema(bundle))
}

fn command_input_value(input: &jet_foundation::CLISchema::CLIInputSchema) -> StatusValue {
    let shape = match (&input.shape, input.positional) {
        (jet_foundation::CLISchema::CLIInputShape::Flag, _) => "flag",
        (jet_foundation::CLISchema::CLIInputShape::Value { .. }, Some(_)) => "positional",
        (jet_foundation::CLISchema::CLIInputShape::Value { .. }, None) => "option",
    };
    StatusValue::object(
        StatusFields::new()
            .with("field", input.field.as_str())
            .with("flag", format!("--{}", input.flag))
            .with(
                "short",
                input
                    .short
                    .as_deref()
                    .map(|short| StatusValue::from(format!("-{short}")))
                    .unwrap_or(StatusValue::Null),
            )
            .with(
                "env",
                input
                    .env
                    .as_deref()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            )
            .with("shape", shape)
            .with("value_type", input.value_kind().as_str())
            .with("required", input.required())
            .with(
                "default",
                input
                    .default_display()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            )
            .with(
                "metavar",
                input
                    .metavar
                    .as_deref()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            )
            .with(
                "positional",
                input
                    .positional
                    .map(|order| StatusValue::from(u64::from(order)))
                    .unwrap_or(StatusValue::Null),
            )
            .with("help", input.help.as_str()),
    )
}

fn command_value(
    command: Option<&jet_foundation::CLISchema::CLICommandSchema>,
) -> StatusValue {
    let Some(command) = command else {
        return StatusValue::Null;
    };
    let inputs = StatusValue::array(command.inputs.iter().map(command_input_value));
    let commands = StatusValue::array(command.commands.iter().map(|subcommand| {
        StatusValue::object(
            StatusFields::new()
                .with("name", subcommand.name.as_str())
                .with(
                    "description",
                    subcommand
                        .description
                        .as_deref()
                        .map(StatusValue::from)
                        .unwrap_or(StatusValue::Null),
                )
                .with(
                    "inputs",
                    StatusValue::array(subcommand.inputs.iter().map(command_input_value)),
                ),
        )
    }));
    StatusValue::object(
        StatusFields::new()
            .with(
                "source",
                format!("fn run(args: {})", command.entry_type),
            )
            .with("entry_type", command.entry_type.as_str())
            .with(
                "description",
                command
                    .description
                    .as_deref()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            )
            .with("inputs", inputs)
            .with("commands", commands)
            .with("standard", command.standard)
            .with(
                "version",
                command
                    .version
                    .as_deref()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            )
            .with(
                "completion_words",
                StatusValue::array(
                    command
                        .completion_words()
                        .iter()
                        .map(|word| StatusValue::from(word.as_str())),
                ),
            ),
    )
}

fn command_text(command: Option<&jet_foundation::CLISchema::CLICommandSchema>) -> String {
    let Some(command) = command else {
        return "command schema\n  none (plain fn run() or non-command target)\n".to_string();
    };
    let mut out = format!(
        "command schema\n  entry: fn run(args: {})\n",
        command.entry_type
    );
    if let Some(description) = &command.description {
        out.push_str(&format!("  description: {description}\n"));
    }
    if command.standard {
        out.push_str("  standard: true\n");
        if let Some(version) = &command.version {
            out.push_str(&format!("  version: {version}\n"));
        }
    }
    for input in &command.inputs {
        write_command_input(&mut out, input, "  ");
    }
    for subcommand in &command.commands {
        out.push_str(&format!("  command {}\n", subcommand.name));
        if let Some(description) = &subcommand.description {
            out.push_str(&format!("    description: {description}\n"));
        }
        for input in &subcommand.inputs {
            write_command_input(&mut out, input, "    ");
        }
    }
    out.push_str(&format!(
        "  completion words: {}\n",
        command.completion_words().join(" ")
    ));
    out
}

fn write_command_input(
    out: &mut String,
    input: &jet_foundation::CLISchema::CLIInputSchema,
    indent: &str,
) {
    let status = if input.required() {
        "required"
    } else {
        "optional"
    };
    let default = input
        .default_display()
        .map(|value| format!(", default {value}"))
        .unwrap_or_default();
    let long = match &input.short {
        Some(short) => format!("-{short} / --{}", input.flag),
        None => format!("--{}", input.flag),
    };
    let form = match input.positional {
        Some(order) => format!("positional#{order} / {long}"),
        None => long,
    };
    let env = input
        .env
        .as_deref()
        .map(|name| format!(", env {name}"))
        .unwrap_or_default();
    out.push_str(&format!(
        "{indent}{form}: {} ({status}{default}{env}) — {}\n",
        input.value_kind().as_str(),
        input.help,
    ));
}

fn absolutize(path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    }
}

/// D-DATA-STATUS1=A: project `data.status()` rows through the dossier lens.
fn render_data_status_dossier(json: bool) {
    let rows = jet::Comptime::data_status_rows();
    if json {
        let rows = StatusValue::array(rows.iter().map(
            |(step, path, copy, ownership, trust, fallback, replacement)| {
                StatusValue::object(
                    StatusFields::new()
                        .with("step", step.as_str())
                        .with("path", path.as_str())
                        .with("copy", copy.as_str())
                        .with("ownership", ownership.as_str())
                        .with("trust", trust.as_str())
                        .with("fallback", fallback.as_str())
                        .with("replacement", replacement.as_str()),
                )
            },
        ));
        let sql_console = StatusValue::object(
            StatusFields::new()
                .with("kind", "facts")
                .with("live_session", false)
                .with("authority", "same local typed query session")
                .with("source", StatusValue::Null)
                .with("result", StatusValue::Null)
                .with(
                    "default_limits",
                    crate::CmdDb::dossier_data_limits_value(),
                )
                .with("rows", crate::CmdDb::dossier_data_value()),
        );
        let data = StatusValue::object(
            StatusFields::new()
                .with("rows", rows)
                .with("sql_console", sql_console),
        );
        println!(
            "{}",
            StatusEnvelope::new("inspect.data", true)
                .with_field("data", data)
                .json()
        );
        return;
    }
    println!("data status");
    for (step, path, copy, ownership, trust, fallback, replacement) in rows {
        println!(
            "  {step}: path={path} copy={copy} ownership={ownership} trust={trust} fallback={fallback} replacement={replacement}"
        );
    }
    print!("{}", crate::CmdDb::dossier_data_text());
}
