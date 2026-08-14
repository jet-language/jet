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
        render_loader_diagnostics(&diagnostics, &file, json, color);
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

/// `jet inspect provenance [<dependency>]` — read the one lock-backed
/// dependency provenance record. Verification stays on existing resolver and
/// E1204 paths; this command only projects their recorded facts.
pub(crate) fn run_provenance(args: &[String], json: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let root = crate::require_manifest_root(
        &cwd,
        "error: no package.jet found — run `jet inspect provenance` inside a project",
    );
    let lock = match jet::Lock::load(&root) {
        Some(lock) => lock,
        None => {
            crate::cli_error!("E1202", "no lockfile found — run `jet fetch` first");
            exit(jet::ExitCodes::USER_ERROR);
        }
    };
    let manifest_path = jet::Loader::manifest_path(&root).expect("manifest root has a Package file");
    let manifest = match jet::Manifest::load(&root) {
        Some(Ok(manifest)) => manifest,
        Some(Err(diagnostic)) => {
            eprint!(
                "{}",
                jet::render_diagnostics(
                    &manifest_path.display().to_string(),
                    "",
                    &[diagnostic],
                )
            );
            exit(jet::ExitCodes::USER_ERROR);
        }
        None => unreachable!("manifest root was found above"),
    };
    let requirement = manifest
        .authority
        .trust
        .as_ref()
        .and_then(|trust| trust.require)
        .unwrap_or(jet::Package::ProvenanceRequirement::None);
    let target = provenance_target(args);
    let mut reports = lock
        .packages
        .iter()
        .filter(|package| !matches!(&package.source, jet::Lock::LockSource::Root))
        .filter(|package| target.is_none() || target.as_deref() == Some(package.name.as_str()))
        .map(jet::Lock::LockedPackage::provenance_report)
        .collect::<Vec<_>>();
    reports.sort_by(|left, right| left.name.cmp(&right.name).then(left.version.cmp(&right.version)));
    if let Some(target) = target {
        if reports.is_empty() {
            crate::cli_error!(
                @fix "E2104",
                format!("dependency `{target}` is not present in the lockfile"),
                "use `jet inspect provenance` to list locked dependencies"
            );
            exit(jet::ExitCodes::USER_ERROR);
        }
    }
    if json {
        render_provenance_json(requirement, &reports);
    } else {
        render_provenance_text(requirement, &reports);
    }
}

fn provenance_target(args: &[String]) -> Option<String> {
    args.iter()
        .find(|argument| !argument.starts_with('-'))
        .cloned()
}

fn render_provenance_text(
    requirement: jet::Package::ProvenanceRequirement,
    reports: &[jet::Lock::DependencyProvenanceReport],
) {
    println!("provenance");
    println!("require: {}", requirement.label());
    for (index, report) in reports.iter().enumerate() {
        if index > 0 {
            println!();
        }
        println!("{} {}", report.name, report.version);
        let evidence = matches!(
            report.integrity.status,
            jet::Lock::ProvenanceStatus::Enforced
        )
        .then_some(", E1204")
        .unwrap_or("");
        println!(
            "  {:<12} {} — matches .jet/lock ({}{evidence})",
            "integrity",
            report.integrity.value,
            report.integrity.status.label()
        );
        render_provenance_field("transparency", &report.transparency);
        render_provenance_field("publisher", &report.publisher);
        render_provenance_field("build", &report.build);
    }
}

fn render_provenance_field(label: &str, field: &jet::Lock::ProvenanceField) {
    println!(
        "  {label:<12} {} ({})",
        field.value,
        field.status.label()
    );
}

fn render_provenance_json(
    requirement: jet::Package::ProvenanceRequirement,
    reports: &[jet::Lock::DependencyProvenanceReport],
) {
    print!(
        "{{\"schema_version\":1,\"require\":\"{}\",\"packages\":[",
        json_escape(requirement.label())
    );
    for (index, report) in reports.iter().enumerate() {
        if index > 0 {
            print!(",");
        }
        print!(
            "{{\"name\":\"{}\",\"version\":\"{}\",",
            json_escape(&report.name),
            json_escape(&report.version)
        );
        let integrity_evidence = matches!(
            report.integrity.status,
            jet::Lock::ProvenanceStatus::Enforced
        )
        .then_some("E1204");
        render_provenance_json_field(
            "integrity",
            &report.integrity,
            integrity_evidence,
            true,
        );
        render_provenance_json_field("transparency", &report.transparency, None, true);
        render_provenance_json_field("publisher", &report.publisher, None, true);
        render_provenance_json_field("build", &report.build, None, false);
        print!("}}");
    }
    println!("]}}");
}

fn render_provenance_json_field(
    key: &str,
    field: &jet::Lock::ProvenanceField,
    evidence: Option<&str>,
    trailing_comma: bool,
) {
    print!(
        "\"{key}\":{{\"value\":\"{}\",\"status\":\"{}\"",
        json_escape(&field.value),
        field.status.label()
    );
    if let Some(evidence) = evidence {
        print!(",\"evidence\":\"{}\"", json_escape(evidence));
    }
    if trailing_comma {
        print!(",");
    }
    print!("}}");
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
    entry_file: &str,
    json: bool,
    color: bool,
) -> ! {
    if json {
        for entry in diagnostics {
            let machine_file = crate::machine_report_path_for_entry(entry_file, &entry.file);
            print!(
                "{}",
                jet::render_all_json(
                    &machine_file,
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
            let machine_file = crate::machine_report_path_for_bundle(bundle, &entry.source);
            print!(
                "{}",
                jet::render_all_json(
                    &machine_file,
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
