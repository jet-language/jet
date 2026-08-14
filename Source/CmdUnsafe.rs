//! D-UNSAFE-OBLIG1=A: the unsafe projection of the shared gate ledger.

use std::process::exit;

use jet::AST::ProgramBundle;
use jet::Diagnostics::Span;
use jet::Sema::GateLedger::{GateEntry, GateKind, GateLedger};
use jet_foundation::JSON::json_escape;

pub(crate) fn run(
    args: &[String],
    json: bool,
    color: bool,
    gates: jet::Policy::GateSet,
) {
    let mut skip_value = false;
    let file = args.iter().find(|argument| {
        if skip_value {
            skip_value = false;
            return false;
        }
        if *argument == "--gate" {
            skip_value = true;
            return false;
        }
        !argument.starts_with('-') && !argument.contains('=')
    });
    let Some(file) = file else {
        crate::cli_error!(@fix "E2104", "`jet inspect unsafe` needs an entry file", "jet inspect unsafe Source/main.jet");
        exit(jet::ExitCodes::USAGE);
    };
    let bundle = jet::Loader::load_entry_with_diagnostics(file).unwrap_or_else(|diagnostics| {
        if json {
            for entry in &diagnostics {
                let machine_file = crate::machine_report_path_for_entry(file, &entry.file);
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
    });
    let ledger = GateLedger::collect(&bundle, gates);
    if !ledger.diagnostics().is_empty() {
        render_report_diagnostics(&ledger, &bundle, json, color);
    }
    let entries = ledger
        .entries()
        .iter()
        .filter(|entry| entry.kind == GateKind::Unsafe)
        .collect::<Vec<_>>();
    if json {
        render_json(&entries, &bundle);
    } else {
        render_human(&entries, &bundle);
    }
}

fn render_report_diagnostics(
    ledger: &GateLedger,
    bundle: &ProgramBundle,
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

fn render_human(entries: &[&GateEntry], bundle: &ProgramBundle) {
    println!("unsafe gates: {}", entries.len());
    for entry in entries {
        let source = module_source(bundle, &entry.source);
        let mode = entry
            .detail
            .strip_prefix("mode=")
            .unwrap_or(entry.detail.as_str());
        println!(
            "{}  {}  reason={}",
            location_for(entry, &source),
            mode,
            entry.reason.as_deref().unwrap_or("<missing>")
        );
        for policy in &entry.provenance {
            println!("  policy {policy}");
        }
        for operation in &entry.operations {
            println!(
                "  {}  {}  {}  required=[{}] asserted=[{}]",
                location(&entry.source, &source, operation.span),
                operation.kind,
                if operation.discharged { "discharged" } else { "missing" },
                operation.required.join(","),
                operation.asserted.join(",")
            );
        }
    }
}

fn render_json(entries: &[&GateEntry], bundle: &ProgramBundle) {
    print!("{{\"schema_version\":1,\"gates\":[");
    for (gate_index, entry) in entries.iter().enumerate() {
        if gate_index > 0 {
            print!(",");
        }
        let source = module_source(bundle, &entry.source);
        let mode = entry
            .detail
            .strip_prefix("mode=")
            .unwrap_or(entry.detail.as_str());
        print!(
            "{{\"source\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}},\"location\":{},\"mode\":\"{}\",\"reason\":{},\"provenance\":[",
            json_escape(&entry.source),
            entry.span.map(|span| span.start).unwrap_or(0),
            entry.span.map(|span| span.end).unwrap_or(0),
            entry.span.map(|span| json_location(&source, span)).unwrap_or_else(|| "null".to_string()),
            json_escape(mode),
            entry
                .reason
                .as_ref()
                .map(|reason| format!("\"{}\"", json_escape(reason)))
                .unwrap_or_else(|| "null".to_string()),
        );
        strings(&entry.provenance);
        print!("],\"operations\":[");
        for (operation_index, operation) in entry.operations.iter().enumerate() {
            if operation_index > 0 {
                print!(",");
            }
            print!(
                "{{\"kind\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}},\"location\":{},\"required\":[",
                json_escape(&operation.kind),
                operation.span.start,
                operation.span.end,
                json_location(&source, operation.span),
            );
            strings(&operation.required);
            print!("],\"asserted\":[");
            strings(&operation.asserted);
            print!("],\"discharged\":{}}}", operation.discharged);
        }
        print!("]}}");
    }
    println!("]}}");
}

fn strings(values: &[String]) {
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            print!(",");
        }
        print!("\"{}\"", json_escape(value));
    }
}

fn module_source(bundle: &ProgramBundle, display: &str) -> String {
    bundle
        .modules
        .iter()
        .find(|module| module.display == display || module.path.to_string_lossy() == display)
        .map(|module| module.source.clone())
        .unwrap_or_default()
}

fn location_for(entry: &GateEntry, source: &str) -> String {
    entry
        .span
        .map(|span| location(&entry.source, source, span))
        .unwrap_or_else(|| entry.source.clone())
}

fn location(source_path: &str, source: &str, span: Span) -> String {
    let (line, column) = jet::Diagnostics::span_line_col(source, span.start);
    format!("{source_path}:{line}:{column}")
}

fn json_location(source: &str, span: Span) -> String {
    let (start_line, start_column) = jet::Diagnostics::span_line_col(source, span.start);
    let (end_line, end_column) = jet::Diagnostics::span_line_col(source, span.end);
    format!(
        "{{\"start\":{{\"line\":{},\"column\":{}}},\"end\":{{\"line\":{},\"column\":{}}}}}",
        start_line, start_column, end_line, end_column
    )
}
