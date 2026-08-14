//! Read-only projections owned by `jet inspect`.

use std::process::exit;

use jet::Diagnostics::Diagnostic;
use jet::Sema::GateLedger::{GateKind, GateLedger};
use jet_foundation::JSON::json_escape;

pub(crate) fn run_guarantees(
    args: &[String],
    json: bool,
    color: bool,
    gates: jet::Policy::GateSet,
    profile: &str,
    freestanding: bool,
) {
    let Some(file) = entry_file(args) else {
        crate::cli_error!(@fix "E2104", "`jet inspect guarantees` needs an entry file", "jet inspect guarantees Source/main.jet");
        exit(jet::ExitCodes::USAGE);
    };
    let bundle = jet::Loader::load_entry_with_diagnostics(&file).unwrap_or_else(|diagnostics| {
        render_loader_diagnostics(&diagnostics, json, color);
    });
    let package = match jet::Package::PackageFacts::load(&bundle.project_root) {
        None => None,
        Some(Ok(facts)) => Some(facts),
        Some(Err(error)) => {
            let file = bundle.project_root.join(jet::Syntax::PACKAGE_FILE);
            let diagnostic = Diagnostic::error(
                "E1206",
                "invalid package manifest".to_string(),
                error.to_string(),
                "fix the fields in package.jet before inspecting guarantees".to_string(),
                None,
            );
            if json {
                print!(
                    "{}",
                    jet::render_all_json(
                        &file.display().to_string(),
                        "",
                        std::slice::from_ref(&diagnostic),
                    )
                );
            } else {
                eprint!(
                    "{}",
                    jet::render_all_colored(
                        &file.display().to_string(),
                        "",
                        std::slice::from_ref(&diagnostic),
                        color,
                    )
                );
            }
            exit(jet::ExitCodes::USER_ERROR);
        }
    };

    let ledger = GateLedger::collect(&bundle, gates);
    if !ledger.diagnostics().is_empty() {
        render_ledger_diagnostics(&ledger, &bundle, json, color);
    }
    let unsafe_gates = ledger
        .entries()
        .iter()
        .filter(|entry| entry.kind == GateKind::Unsafe)
        .count();
    let dependencies = bundle.dep_roots.keys().cloned().collect::<Vec<_>>();
    let report = jet::Driver::guarantee_report(
        package.as_ref(),
        dependencies,
        unsafe_gates,
        profile,
        freestanding,
    );
    if json {
        render_json(&report);
    } else {
        render_human(&report);
    }
}

fn entry_file(args: &[String]) -> Option<String> {
    let mut skip_value = false;
    for argument in args {
        if skip_value {
            skip_value = false;
            continue;
        }
        if matches!(argument.as_str(), "--profile" | "--target" | "--scope" | "--kind") {
            skip_value = true;
            continue;
        }
        if argument.starts_with('-') {
            continue;
        }
        return Some(argument.clone());
    }
    None
}

fn render_loader_diagnostics(
    diagnostics: &[jet::Loader::LoaderDiagnostic],
    json: bool,
    color: bool,
) -> ! {
    if json {
        for entry in diagnostics {
            print!(
                "{}",
                jet::render_all_json(
                    &entry.file,
                    &entry.source,
                    std::slice::from_ref(&entry.diagnostic),
                )
            );
        }
    } else {
        for (index, entry) in diagnostics.iter().enumerate() {
            if index > 0 {
                eprint!("\n");
            }
            eprint!(
                "{}",
                jet::render_all_colored(
                    &entry.file,
                    &entry.source,
                    std::slice::from_ref(&entry.diagnostic),
                    color,
                )
            );
        }
    }
    exit(jet::ExitCodes::USER_ERROR);
}

fn render_ledger_diagnostics(
    ledger: &GateLedger,
    bundle: &jet::AST::ProgramBundle,
    json: bool,
    color: bool,
) -> ! {
    if json {
        for entry in ledger.diagnostics() {
            let source = module_source(bundle, &entry.source);
            print!(
                "{}",
                jet::render_all_json(
                    &entry.source,
                    &source,
                    std::slice::from_ref(&entry.diagnostic),
                )
            );
        }
    } else {
        for (index, entry) in ledger.diagnostics().iter().enumerate() {
            if index > 0 {
                eprint!("\n");
            }
            let source = module_source(bundle, &entry.source);
            eprint!(
                "{}",
                jet::render_all_colored(
                    &entry.source,
                    &source,
                    std::slice::from_ref(&entry.diagnostic),
                    color,
                )
            );
        }
    }
    exit(jet::ExitCodes::USER_ERROR);
}

fn module_source(bundle: &jet::AST::ProgramBundle, display: &str) -> String {
    bundle
        .modules
        .iter()
        .find(|module| module.display == display || module.path.to_string_lossy() == display)
        .map(|module| module.source.clone())
        .unwrap_or_default()
}

fn render_human(report: &jet::Driver::GuaranteeReport) {
    println!("guarantees");
    println!("profile: {}", report.profile);
    println!(
        "scope: {}",
        if report.package { "package" } else { "single-file" }
    );
    if report.freestanding {
        println!("target: freestanding");
    }
    println!("component              guarantee  evidence");
    for component in &report.components {
        println!(
            "{:<22} {:<10} {}",
            component.name,
            component.status.label(),
            component.evidence
        );
    }
    for note in &report.notes {
        println!("note: {note}");
    }
}

fn render_json(report: &jet::Driver::GuaranteeReport) {
    print!(
        "{{\"schema_version\":1,\"profile\":\"{}\",\"package\":{},\"freestanding\":{},\"components\":[",
        json_escape(&report.profile),
        report.package,
        report.freestanding,
    );
    for (index, component) in report.components.iter().enumerate() {
        if index > 0 {
            print!(",");
        }
        print!(
            "{{\"component\":\"{}\",\"guarantee\":\"{}\",\"evidence\":\"{}\"}}",
            json_escape(&component.name),
            component.status.label(),
            json_escape(&component.evidence),
        );
    }
    print!("],\"notes\":[");
    for (index, note) in report.notes.iter().enumerate() {
        if index > 0 {
            print!(",");
        }
        print!("\"{}\"", json_escape(note));
    }
    println!("]}}");
}
