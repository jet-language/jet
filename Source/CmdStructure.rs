//! D-STRUCT-PLANE1=A: `jet inspect structure` projects the checked structure
//! facts and the structure slice of the one gate ledger. It does not run a
//! second analyzer and it never passes these compiler facts to codegen.

use std::path::PathBuf;
use std::process::exit;

use jet::Sema::GateLedger::{GateKind, GateLedger};
use jet_foundation::Names::StructureFact;
use jet_foundation::Registry;
use jet_foundation::JSON::json_escape;

pub(crate) fn run_structure(args: &[String], json: bool, color: bool, gates: jet::Policy::GateSet) {
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
    let rows = Registry::structure_rows()
        .map(|row| {
            format!(
                "{{\"name\":\"{}\",\"safe_direction\":\"{}\",\"gates\":[{}],\"decision\":\"{}\"}}",
                json_escape(row.name),
                json_escape(row.safe_direction.name()),
                row.gates
                    .iter()
                    .map(|gate| format!("\"{}\"", json_escape(gate)))
                    .collect::<Vec<_>>()
                    .join(","),
                json_escape(row.decision),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let facts = facts
        .iter()
        .map(render_fact_json)
        .collect::<Vec<_>>()
        .join(",");
    let gates = ledger
        .entries()
        .iter()
        .filter(|entry| entry.kind == GateKind::Structure)
        .map(|entry| {
            let provenance = entry
                .provenance
                .iter()
                .map(|value| json_string(value))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"kind\":\"{}\",\"scope\":\"{}\",\"source\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}},\"subject\":\"{}\",\"reason\":{},\"status\":{},\"detail\":\"{}\",\"provenance\":[{}]}}",
                json_escape(entry.kind.name()),
                json_escape(&entry.scope),
                json_escape(&entry.source),
                entry.span.map_or(0, |span| span.start),
                entry.span.map_or(0, |span| span.end),
                json_escape(&entry.subject),
                entry
                    .reason
                    .as_deref()
                    .map_or_else(|| "null".to_string(), |value| json_string(value)),
                entry
                    .status
                    .as_deref()
                    .map_or_else(|| "null".to_string(), |value| json_string(value)),
                json_escape(&entry.detail),
                provenance,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    println!(
        "{{\"schema_version\":1,\"rows\":[{}],\"facts\":[{}],\"gates\":[{}]}}",
        rows, facts, gates
    );
}

fn render_fact_json(fact: &StructureFact) -> String {
    let registry_name = Registry::structure_row(fact.kind)
        .map(|row| row.name)
        .unwrap_or(fact.kind.name());
    format!(
        "{{\"kind\":\"{}\",\"registry\":\"{}\",\"subject\":\"{}\",\"source\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}},\"status\":\"{}\",\"detail\":\"{}\",\"gate\":{}}}",
        json_escape(fact.kind.name()),
        json_escape(registry_name),
        json_escape(&fact.subject),
        json_escape(&fact.source),
        fact.span.start,
        fact.span.end,
        json_escape(&fact.status),
        json_escape(&fact.detail),
        fact.gate
            .as_deref()
            .map_or_else(|| "null".to_string(), json_string),
    )
}

fn json_string(value: &str) -> String {
    format!("\"{}\"", json_escape(value))
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
        print!(
            "{}",
            jet::render_all_json(
                &crate::machine_report_path_for_process(entry),
                &source,
                diagnostics,
            )
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
    for diagnostic in ledger.diagnostics() {
        let source = bundle
            .modules
            .iter()
            .find(|module| module.display == diagnostic.source)
            .map(|module| module.source.as_str())
            .unwrap_or("");
        if json {
            print!(
                "{}",
                jet::render_all_json(
                    &crate::machine_report_path_for_bundle(bundle, &diagnostic.source),
                    source,
                    std::slice::from_ref(&diagnostic.diagnostic),
                )
            );
        } else {
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
