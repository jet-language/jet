//! D-STRUCT-PLANE1=A: `jet inspect structure` projects the checked structure
//! facts and the structure slice of the one gate ledger. It does not run a
//! second analyzer and it never passes these compiler facts to codegen.

use std::path::PathBuf;
use std::process::exit;

use jet::Sema::GateLedger::{GateKind, GateLedger};
use jet_foundation::CoreModuleExports::{
    core_bootstrap_order, core_modules, CoreLeafKind, CORE_ROOT_TYPES,
};
use jet_foundation::Names::StructureFact;
use jet_foundation::Registry;
use jet_foundation::Report::{StatusEnvelope, StatusFields, StatusValue};

pub(crate) fn run_structure(args: &[String], json: bool, color: bool, gates: jet::Policy::GateSet) {
    if args.iter().any(|argument| argument == "--core") {
        if json {
            render_core_json();
        } else {
            render_core_text();
        }
        return;
    }
    let Some(path) = entry_file(args) else {
        crate::cli_error!(
            @full "E2104",
            "`jet inspect structure` needs an entry file",
            "structure facts come from one checked Jet entry file",
            "run `jet inspect structure examples/features/basics/hello.jet`"
        );
        exit(jet::ExitCodes::USAGE);
    };

    let abs = absolutize(&path);
    // Preserve the command's relative spelling in provenance. Loading still
    // resolves from the same working directory, while committed reports stay
    // deterministic across checkout paths.
    let entry = path;
    let (diagnostics, bundle, facts) =
        jet::Driver::check_file_with_effect_facts(&entry, None, false);
    let has_errors = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error);
    let Some(bundle) = (if has_errors { None } else { bundle }) else {
        if !facts.name_ledger.structure_facts().is_empty() {
            let mut ledger = GateLedger::default();
            ledger.append_structure_facts(&facts.name_ledger);
            if json {
                render_json(facts.name_ledger.structure_facts(), &ledger);
            } else {
                render_text(facts.name_ledger.structure_facts(), &ledger);
            }
        }
        render_frontend_diagnostics(&entry, &abs, &diagnostics, json, color);
    };

    let ledger = GateLedger::collect(&bundle, gates);
    if !ledger.diagnostics().is_empty() {
        render_gate_diagnostics(&ledger, &bundle, json, color);
    }

    if json {
        render_json(facts.name_ledger.structure_facts(), &ledger);
    } else {
        render_text(facts.name_ledger.structure_facts(), &ledger);
    }
}

fn entry_file(args: &[String]) -> Option<String> {
    args.iter()
        .find(|argument| !argument.starts_with('-'))
        .cloned()
}

fn absolutize(path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}
fn render_core_text() {
    println!("core");
    println!("bootstrap:");
    for (index, step) in core_bootstrap_order().iter().enumerate() {
        let dependencies = if step.dependencies.is_empty() {
            "none".to_string()
        } else {
            step.dependencies.join(", ")
        };
        println!(
            "  {}. {}  depends_on [{}]",
            index + 1,
            step.name,
            dependencies
        );
    }

    println!("root types: {}", CORE_ROOT_TYPES.join(", "));
    println!("modules: {}", core_modules().len());
    for module in core_modules() {
        let dependencies = if module.dependencies.is_empty() {
            "none".to_string()
        } else {
            module.dependencies.join(", ")
        };
        println!("  module {}  depends_on [{}]", module.module, dependencies);
        println!("    members: {}", module.members.join(", "));
        if module.type_exports.is_empty() {
            println!("    types: (none)");
        } else {
            println!("    types:");
            for &(name, kind) in module.type_exports {
                println!("      {name} ({})", core_leaf_kind_text(kind));
            }
        }
    }
}

fn core_leaf_kind_text(kind: CoreLeafKind) -> String {
    match kind {
        CoreLeafKind::Plain => "plain".to_string(),
        CoreLeafKind::CryptoNominal => "crypto_nominal".to_string(),
        CoreLeafKind::Generic(arity) => format!("generic({arity})"),
        CoreLeafKind::Enum(variants) => {
            format!("enum [{}]", variants.iter().copied().collect::<Vec<_>>().join(", "))
        }
    }
}

fn core_type_json(name: &str, kind: CoreLeafKind) -> StatusValue {
    let mut fields = StatusFields::new()
        .with("name", name)
        .with(
            "kind",
            match kind {
                CoreLeafKind::Plain => "plain",
                CoreLeafKind::CryptoNominal => "crypto_nominal",
                CoreLeafKind::Generic(_) => "generic",
                CoreLeafKind::Enum(_) => "enum",
            },
        );
    if let CoreLeafKind::Enum(variants) = kind {
        fields = fields.with(
            "variants",
            StatusValue::array(variants.iter().copied().map(StatusValue::from)),
        );
    }
    StatusValue::object(fields)
}

fn render_core_json() {
    let bootstrap = StatusValue::array(core_bootstrap_order().iter().map(|step| {
        StatusValue::object(
            StatusFields::new()
                .with("name", step.name)
                .with(
                    "dependencies",
                    StatusValue::array(step.dependencies.iter().copied().map(StatusValue::from)),
                ),
        )
    }));
    let roots = StatusValue::array(CORE_ROOT_TYPES.iter().copied().map(StatusValue::from));
    let modules = StatusValue::array(core_modules().iter().map(|module| {
        StatusValue::object(
            StatusFields::new()
                .with("module", module.module)
                .with(
                    "members",
                    StatusValue::array(module.members.iter().copied().map(StatusValue::from)),
                )
                .with(
                    "type_exports",
                    StatusValue::array(
                        module
                            .type_exports
                            .iter()
                            .map(|(name, kind)| core_type_json(name, *kind)),
                    ),
                )
                .with(
                    "dependencies",
                    StatusValue::array(module.dependencies.iter().copied().map(StatusValue::from)),
                ),
        )
    }));
    let core = StatusValue::object(
        StatusFields::new()
            .with("bootstrap", bootstrap)
            .with("root_types", roots)
            .with("modules", modules),
    );
    println!(
        "{}",
        StatusEnvelope::new("inspect.structure", true)
            .with_field("core", core)
            .json()
    );
}


fn render_text(facts: &[StructureFact], ledger: &GateLedger) {
    println!("structure");
    for row in Registry::structure_rows() {
        println!(
            "  {}  safe={}  gates={}",
            row.name,
            row.safe_direction.name(),
            row.gates.join(", ")
        );
    }

    println!("facts: {}", facts.len());
    if facts.is_empty() {
        println!("  (none in this program)");
    } else {
        for fact in facts {
            println!(
                "  {}  {}:{}..{}  {}  {} — {}",
                fact.kind.name(),
                fact.source,
                fact.span.start,
                fact.span.end,
                fact.subject,
                fact.status,
                fact.detail
            );
        }
    }

    let gates: Vec<_> = ledger
        .entries()
        .iter()
        .filter(|entry| entry.kind == GateKind::Structure)
        .collect();
    println!("gates: {}", gates.len());
    if gates.is_empty() {
        println!("  (none in this program)");
    } else {
        for entry in gates {
            println!(
                "  {}  {}:{}..{}  {}  {} — {}",
                entry.scope,
                entry.source,
                entry.span.map_or(0, |span| span.start),
                entry.span.map_or(0, |span| span.end),
                entry.subject,
                entry.reason.as_deref().unwrap_or("unknown"),
                entry.detail
            );
            for provenance in &entry.provenance {
                println!("    provenance {provenance}");
            }
        }
    }
}

fn render_json(facts: &[StructureFact], ledger: &GateLedger) {
    let rows = StatusValue::array(Registry::structure_rows().map(|row| {
        StatusValue::object(
            StatusFields::new()
                .with("name", row.name)
                .with("safe_direction", row.safe_direction.name())
                .with(
                    "gates",
                    StatusValue::array(row.gates.iter().copied().map(StatusValue::from)),
                )
                .with("decision", row.decision),
        )
    }));
    let facts = StatusValue::array(facts.iter().map(|fact| {
        let span = StatusValue::object(
            StatusFields::new()
                .with("start", fact.span.start)
                .with("end", fact.span.end),
        );
        let gate = fact
            .gate
            .as_deref()
            .map(StatusValue::from)
            .unwrap_or(StatusValue::Null);
        StatusValue::object(
            StatusFields::new()
                .with("kind", fact.kind.name())
                .with(
                    "registry",
                    Registry::structure_row(fact.kind)
                        .map(|row| row.name)
                        .unwrap_or(fact.kind.name()),
                )
                .with("subject", fact.subject.as_str())
                .with("source", fact.source.as_str())
                .with("span", span)
                .with("status", fact.status.as_str())
                .with("detail", fact.detail.as_str())
                .with("gate", gate),
        )
    }));
    let gates = StatusValue::array(
        ledger
            .entries()
            .iter()
            .filter(|entry| entry.kind == GateKind::Structure)
            .map(|entry| {
                let span = StatusValue::object(
                    StatusFields::new()
                        .with("start", entry.span.map_or(0, |span| span.start))
                        .with("end", entry.span.map_or(0, |span| span.end)),
                );
                let reason = entry
                    .reason
                    .as_deref()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null);
                let status = entry
                    .status
                    .as_deref()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null);
                let provenance = StatusValue::array(
                    entry
                        .provenance
                        .iter()
                        .map(|value| StatusValue::from(value.as_str())),
                );
                StatusValue::object(
                    StatusFields::new()
                        .with("kind", entry.kind.name())
                        .with("scope", entry.scope.as_str())
                        .with("source", entry.source.as_str())
                        .with("span", span)
                        .with("subject", entry.subject.as_str())
                        .with("reason", reason)
                        .with("status", status)
                        .with("detail", entry.detail.as_str())
                        .with("provenance", provenance),
                )
            }),
    );
    let structure = StatusValue::object(
        StatusFields::new()
            .with("rows", rows)
            .with("facts", facts)
            .with("gates", gates),
    );
    println!(
        "{}",
        StatusEnvelope::new("inspect.structure", true)
            .with_field("structure", structure)
            .json()
    );
}


fn render_frontend_diagnostics(
    entry: &str,
    path: &PathBuf,
    diagnostics: &[jet::Diagnostics::Diagnostic],
    json: bool,
    color: bool,
) -> ! {
    let source = std::fs::read_to_string(path).unwrap_or_default();
    if json {
        let reports = diagnostics.iter().map(|diagnostic| {
            diagnostic.to_report(
                &crate::machine_report_path_for_process(entry),
                &source,
            )
        });
        print!(
            "{}",
            StatusEnvelope::new("inspect.structure", false)
                .with_reports(reports)
                .json()
        );
    } else {
        for diagnostic in diagnostics {
            eprint!(
                "{}",
                jet::render_all_colored(entry, &source, std::slice::from_ref(diagnostic), color,)
            );
        }
    }
    exit(jet::ExitCodes::USER_ERROR);
}

fn render_gate_diagnostics(
    ledger: &GateLedger,
    bundle: &jet::AST::ProgramBundle,
    json: bool,
    color: bool,
) -> ! {
    if json {
        let reports = ledger.diagnostics().iter().map(|diagnostic| {
            let source = bundle
                .modules
                .iter()
                .find(|module| module.display == diagnostic.source)
                .map(|module| module.source.as_str())
                .unwrap_or("");
            let file = crate::machine_report_path_for_bundle(bundle, &diagnostic.source);
            diagnostic.diagnostic.to_report(&file, source)
        });
        print!(
            "{}",
            StatusEnvelope::new("inspect.structure", false)
                .with_reports(reports)
                .json()
        );
    } else {
        for diagnostic in ledger.diagnostics() {
            let source = bundle
                .modules
                .iter()
                .find(|module| module.display == diagnostic.source)
                .map(|module| module.source.as_str())
                .unwrap_or("");
            eprint!(
                "{}",
                jet::render_all_colored(
                    &diagnostic.source,
                    source,
                    std::slice::from_ref(&diagnostic.diagnostic),
                    color,
                )
            );
        }
    }
    exit(jet::ExitCodes::USER_ERROR);
}
