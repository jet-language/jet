//! D-UNSAFE-OBLIG1=A: deterministic `jet inspect unsafe` audit report.

use std::process::exit;

use jet::AST::ProgramBundle;
use jet::Diagnostics::Span;
use jet_foundation::JSON::json_escape;

pub(crate) fn run(args: &[String], json: bool, color: bool) {
    let Some(file) = args.iter().find(|argument| !argument.starts_with('-')) else {
        crate::cli_error!(@fix "E2104", "`jet inspect unsafe` needs an entry file", "jet inspect unsafe Source/main.jet");
        exit(jet::ExitCodes::USAGE);
    };
    let bundle = jet::Loader::load_entry_with_diagnostics(file).unwrap_or_else(|diagnostics| {
        if json {
            for entry in &diagnostics {
                print!("{}", jet::render_all_json(&entry.file, &entry.source, std::slice::from_ref(&entry.diagnostic)));
            }
        } else {
            for (index, entry) in diagnostics.iter().enumerate() {
                if index > 0 { eprint!("\n"); }
                eprint!("{}", jet::render_all_colored(&entry.file, &entry.source, std::slice::from_ref(&entry.diagnostic), color));
            }
        }
        exit(jet::ExitCodes::USER_ERROR);
    });
    let report = jet::Sema::UnsafeObligations::inspect(&bundle);
    if !report.diagnostics.is_empty() {
        render_report_diagnostics(&report, &bundle, json, color);
    }
    if json { render_json(&report, &bundle); } else { render_human(&report, &bundle); }
}

fn render_report_diagnostics(
    report: &jet::Sema::UnsafeObligations::UnsafeInspection,
    bundle: &ProgramBundle,
    json: bool,
    color: bool,
) -> ! {
    if json {
        for entry in &report.diagnostics {
            let source = module_source(bundle, &entry.source);
            print!("{}", jet::render_all_json(&entry.source, &source, std::slice::from_ref(&entry.diagnostic)));
        }
    } else {
        for (index, entry) in report.diagnostics.iter().enumerate() {
            if index > 0 { eprint!("\n"); }
            let source = module_source(bundle, &entry.source);
            eprint!("{}", jet::render_all_colored(&entry.source, &source, std::slice::from_ref(&entry.diagnostic), color));
        }
    }
    exit(jet::ExitCodes::USER_ERROR);
}

fn render_human(report: &jet::Sema::UnsafeObligations::UnsafeInspection, bundle: &ProgramBundle) {
    println!("unsafe gates: {}", report.gates.len());
    for gate in &report.gates {
        let source = module_source(bundle, &gate.source);
        println!("{}  {}  reason={}", location(&gate.source, &source, gate.span), gate.mode, gate.reason.as_deref().unwrap_or("<missing>"));
        for policy in &gate.provenance { println!("  policy {policy}"); }
        for operation in &gate.operations {
            println!("  {}  {}  {}  required=[{}] asserted=[{}]", location(&gate.source, &source, operation.span), operation.kind, if operation.discharged { "discharged" } else { "missing" }, operation.required.join(","), operation.asserted.join(","));
        }
    }
}

fn render_json(report: &jet::Sema::UnsafeObligations::UnsafeInspection, bundle: &ProgramBundle) {
    print!("{{\"schema_version\":1,\"gates\":[");
    for (gate_index, gate) in report.gates.iter().enumerate() {
        if gate_index > 0 { print!(","); }
        let source = module_source(bundle, &gate.source);
        print!("{{\"source\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}},\"location\":{},\"mode\":\"{}\",\"reason\":{},\"provenance\":[", json_escape(&gate.source), gate.span.start, gate.span.end, json_location(&source, gate.span), json_escape(&gate.mode), gate.reason.as_ref().map(|reason| format!("\"{}\"", json_escape(reason))).unwrap_or_else(|| "null".to_string()));
        strings(&gate.provenance);
        print!("],\"operations\":[");
        for (operation_index, operation) in gate.operations.iter().enumerate() {
            if operation_index > 0 { print!(","); }
            print!("{{\"kind\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}},\"location\":{},\"required\":[", json_escape(&operation.kind), operation.span.start, operation.span.end, json_location(&source, operation.span));
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
        if index > 0 { print!(","); }
        print!("\"{}\"", json_escape(value));
    }
}

fn module_source(bundle: &ProgramBundle, display: &str) -> String {
    bundle.modules.iter()
        .find(|module| module.display == display || module.path.to_string_lossy() == display)
        .map(|module| module.source.clone())
        .unwrap_or_default()
}

fn location(source_path: &str, source: &str, span: Span) -> String {
    let (line, column) = jet::Diagnostics::span_line_col(source, span.start);
    format!("{source_path}:{line}:{column}")
}

fn json_location(source: &str, span: Span) -> String {
    let (start_line, start_column) = jet::Diagnostics::span_line_col(source, span.start);
    let (end_line, end_column) = jet::Diagnostics::span_line_col(source, span.end);
    format!(
        "{{\"start\":{{\"line\":{start_line},\"column\":{start_column}}},\"end\":{{\"line\":{end_line},\"column\":{end_column}}}}}"
    )
}
