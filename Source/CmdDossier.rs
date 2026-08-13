//! D-WD2/D-DOSSIER1: `jet inspect dossier` — one explainable view over semantic facts.

use std::path::{Path, PathBuf};
use std::process::exit;

use jet::Diagnostics::json_str as json_string;
use jet::ExitCodes;
use jet_semindex::{open, SemIndexError};

/// D-CONF-MODULE1=A: explain a generic-module member's specialization input
/// from the semantic index, including the profile/declaration chain for a
/// build-fact value.
pub(crate) fn run_module_explain(subject: &str, file: &str, profile: &str, json: bool) {
    let abs = absolutize(file);
    let module_name = subject.split('.').next().unwrap_or(subject);
    let index = match jet_semindex::open_with_profile(&abs, profile) {
        Ok(index) => index,
        Err(SemIndexError::Load(diags)) => {
            for diagnostic in &diags {
                eprintln!(
                    "{}",
                    jet::render_diagnostics(
                        &abs.display().to_string(),
                        "",
                        std::slice::from_ref(diagnostic),
                    )
                );
            }
            exit(ExitCodes::USER_ERROR);
        }
    };
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
        let arguments = instance
            .argument_values
            .iter()
            .zip(&instance.argument_provenance)
            .map(|(value, sources)| {
                format!(
                    "{{\"value\":{},\"provenance\":[{}]}}",
                    json_string(value),
                    sources
                        .iter()
                        .map(|source| json_string(source))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{{\"schema_version\":1,\"subject\":{},\"module\":{},\"fingerprint\":{},\"arguments\":[{}]}}",
            json_string(subject),
            json_string(&instance.name),
            json_string(&instance.fingerprint),
            arguments
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
}

pub(crate) fn run_dossier(args: &[String], json: bool) {
    let mut positional: Vec<&str> = Vec::new();
    for a in args {
        if !a.starts_with('-') {
            positional.push(a.as_str());
        }
    }

    // D-TARGET-AUDIT1=A: `jet inspect dossier target <machine>`
    if positional.first().copied() == Some("target") {
        let name = positional.get(1).copied().unwrap_or("board.sensor_v1");
        match jet::Driver::target_machine_dossier_json(name) {
            Ok(audit) => {
                if json {
                    println!("{audit}");
                } else {
                    println!("target machine: {name}");
                    println!("{audit}");
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

    let (path, target) = match positional.as_slice() {
        [path] => (*path, None),
        [path, target] => (*path, Some(*target)),
        _ => {
            crate::cli_error!(@fix "E2104", "`jet inspect dossier` needs an entry file and optional symbol", "jet inspect dossier examples/features/basics/hello.jet run; use `target board.sensor_v1` or `data` for those dossiers");
            exit(ExitCodes::USER_ERROR);
        }
    };

    let abs = absolutize(path);
    match open(&abs) {
        Ok(idx) => {
            let target = target.unwrap_or_else(|| {
                idx.definitions()
                    .iter()
                    .find(|d| matches!(d.kind, jet_semindex::SymbolKind::Struct { .. }))
                    .or_else(|| idx.definitions().iter().find(|d| d.name == "run"))
                    .map(|d| d.name.as_str())
                    .unwrap_or("run")
            });
            let dossier = idx.dossier(target);
            let (budgets, command) = auxiliary_projections(&abs);
            if json {
                let mut value = dossier.to_json();
                if value.ends_with('}') {
                    value.pop();
                    value.push_str(",\"performance_budgets\":");
                    value.push_str(&budgets.to_json());
                    value.push_str(",\"command_schema\":");
                    value.push_str(&command_json(command.as_ref()));
                    value.push('}');
                }
                println!("{value}");
            } else {
                print!("{}", dossier.render_text());
                print!("{}", command_text(command.as_ref()));
                print!("{}", budgets.render_text());
            }
            if dossier.definition.is_none() {
                exit(ExitCodes::USER_ERROR);
            }
        }
        Err(SemIndexError::Load(diags)) => {
            for d in &diags {
                eprintln!(
                    "{}",
                    jet::render_diagnostics(
                        &abs.display().to_string(),
                        "",
                        std::slice::from_ref(d)
                    )
                );
            }
            exit(ExitCodes::USER_ERROR);
        }
    }
}

fn auxiliary_projections(
    entry: &Path,
) -> (
    jet::BudgetView::BudgetProjection,
    Option<jet_foundation::CLISchema::CLICommandSchema>,
) {
    let entry_text = entry.to_string_lossy();
    let (diagnostics, bundle, _) = jet::Driver::check_file_with_effect_facts(&entry_text, None, false);
    if diagnostics.iter().any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error) {
        return (jet::BudgetView::BudgetProjection::default(), None);
    }
    let Some(bundle) = bundle else {
        return (jet::BudgetView::BudgetProjection::default(), None);
    };
    let command = entry_command_schema(&bundle);
    let root = jet::Loader::find_manifest_root(entry.parent().unwrap_or(Path::new(".")))
        .unwrap_or_else(|| entry.parent().unwrap_or(Path::new(".")).to_path_buf());
    let sources = bundle.modules.iter().map(|module| {
        let path = module.path.strip_prefix(&root).unwrap_or(&module.path).to_string_lossy().replace('\\', "/");
        (path, jet::SHA256::sha256_hex(module.source.as_bytes()))
    }).collect::<Vec<_>>();
    (jet::BudgetView::read_compatible(&root, &sources), command)
}

fn entry_command_schema(
    bundle: &jet::AST::ProgramBundle,
) -> Option<jet_foundation::CLISchema::CLICommandSchema> {
    let items = &bundle.modules.get(bundle.entry)?.items;
    items.iter().find_map(|item| match item {
        jet::AST::Item::Func(function)
            if function.name == "run" && function.params.len() == 1 =>
        {
            Some(())
        }
        _ => None,
    })?;
    Some(jet_foundation::CLISchema::executable_schema(bundle))
}

fn command_json(command: Option<&jet_foundation::CLISchema::CLICommandSchema>) -> String {
    let Some(command) = command else {
        return "null".to_string();
    };
    let input_json = |input: &jet_foundation::CLISchema::CLIInputSchema| {
            let shape = match (&input.shape, input.positional) {
                (jet_foundation::CLISchema::CLIInputShape::Flag, _) => "flag",
                (jet_foundation::CLISchema::CLIInputShape::Value { .. }, Some(_)) => "positional",
                (jet_foundation::CLISchema::CLIInputShape::Value { .. }, None) => "option",
            };
            let default = input
                .default_display()
                .map(|value| json_string(&value))
                .unwrap_or_else(|| "null".to_string());
            let metavar = input
                .metavar
                .as_deref()
                .map(json_string)
                .unwrap_or_else(|| "null".to_string());
            let positional = input
                .positional
                .map(|order| order.to_string())
                .unwrap_or_else(|| "null".to_string());
            let short = input
                .short
                .as_deref()
                .map(|short| json_string(&format!("-{short}")))
                .unwrap_or_else(|| "null".to_string());
            let env = input
                .env
                .as_deref()
                .map(json_string)
                .unwrap_or_else(|| "null".to_string());
            format!(
                "{{\"field\":{},\"flag\":{},\"short\":{},\"env\":{},\"shape\":{},\"value_type\":{},\"required\":{},\"default\":{},\"metavar\":{},\"positional\":{},\"help\":{}}}",
                json_string(&input.field),
                json_string(&format!("--{}", input.flag)),
                short,
                env,
                json_string(shape),
                json_string(input.value_kind().as_str()),
                input.required(),
                default,
                metavar,
                positional,
                json_string(&input.help),
            )
    };
    let inputs = command.inputs.iter().map(&input_json)
        .collect::<Vec<_>>()
        .join(",");
    let commands = command.commands.iter().map(|subcommand| {
        let inputs = subcommand.inputs.iter().map(&input_json).collect::<Vec<_>>().join(",");
        let description = subcommand
            .description
            .as_deref()
            .map(json_string)
            .unwrap_or_else(|| "null".to_string());
        format!("{{\"name\":{},\"description\":{},\"inputs\":[{}]}}", json_string(&subcommand.name), description, inputs)
    }).collect::<Vec<_>>().join(",");
    let description = command
        .description
        .as_deref()
        .map(json_string)
        .unwrap_or_else(|| "null".to_string());
    let completion = command
        .completion_words()
        .iter()
        .map(|word| json_string(word))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"source\":{},\"entry_type\":{},\"description\":{},\"inputs\":[{}],\"commands\":[{}],\"completion_words\":[{}]}}",
        json_string(&format!("fn run(args: {})", command.entry_type)),
        json_string(&command.entry_type),
        description,
        inputs,
        commands,
        completion,
    )
}

fn command_text(command: Option<&jet_foundation::CLISchema::CLICommandSchema>) -> String {
    let Some(command) = command else {
        return "command schema\n  none (plain fn run() or non-command target)\n".to_string();
    };
    let mut out = format!("command schema\n  entry: fn run(args: {})\n", command.entry_type);
    if let Some(description) = &command.description {
        out.push_str(&format!("  description: {description}\n"));
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
    let status = if input.required() { "required" } else { "optional" };
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
        let body = rows
            .iter()
            .map(|(step, path, copy, ownership, trust, fallback, replacement)| {
                format!(
                    "{{\"step\":{},\"path\":{},\"copy\":{},\"ownership\":{},\"trust\":{},\"fallback\":{},\"replacement\":{}}}",
                    json_string(step),
                    json_string(path),
                    json_string(copy),
                    json_string(ownership),
                    json_string(trust),
                    json_string(fallback),
                    json_string(replacement),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{{\"lens\":\"data\",\"schema\":\"jet.data-status\",\"rows\":[{body}]}}"
        );
        return;
    }
    println!("data status");
    for (step, path, copy, ownership, trust, fallback, replacement) in rows {
        println!(
            "  {step}: path={path} copy={copy} ownership={ownership} trust={trust} fallback={fallback} replacement={replacement}"
        );
    }
}
