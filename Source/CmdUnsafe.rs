//! D-UNSAFE-OBLIG1=A: deterministic `jet inspect unsafe` audit report.

use std::fs;
use std::process::exit;

use jet::Diagnostics::Span;

pub(crate) fn run(args: &[String], json: bool) {
    let Some(file) = args.iter().find(|argument| !argument.starts_with('-')) else {
        eprintln!("error: `jet inspect unsafe` needs an entry file");
        eprintln!(" fix: jet inspect unsafe Source/main.jet");
        exit(jet::ExitCodes::USAGE);
    };
    let bundle = jet::Loader::load_entry(file).unwrap_or_else(|diagnostics| {
        // Loader failures are ordinary Jet diagnostics. Keep the same what,
        // why, fix, source frame, and color policy as `jet check` instead of
        // collapsing them to a two-field tool error.
        let source = fs::read_to_string(file).unwrap_or_default();
        eprint!("{}", jet::render_diagnostics(file, &source, &diagnostics));
        exit(jet::ExitCodes::USER_ERROR);
    });
    let report = jet::Sema::UnsafeObligations::inspect(&bundle);
    if json { render_json(&report); } else { render_human(&report); }
}

fn render_human(report: &jet::Sema::UnsafeObligations::UnsafeInspection) {
    println!("unsafe gates: {}", report.gates.len());
    for gate in &report.gates {
        println!("{}  {}  reason={}", location(&gate.source, gate.span), gate.mode, gate.reason.as_deref().unwrap_or("<missing>"));
        for source in &gate.provenance { println!("  policy {source}"); }
        for operation in &gate.operations {
            println!("  {}  {}  {}  required=[{}] asserted=[{}]", location(&gate.source, operation.span), operation.kind, if operation.discharged { "discharged" } else { "missing" }, operation.required.join(","), operation.asserted.join(","));
        }
    }
}

fn render_json(report: &jet::Sema::UnsafeObligations::UnsafeInspection) {
    print!("{{\"schema_version\":1,\"gates\":[");
    for (gate_index, gate) in report.gates.iter().enumerate() {
        if gate_index > 0 { print!(","); }
        print!("{{\"source\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}},\"location\":{},\"mode\":\"{}\",\"reason\":{},\"provenance\":[", escape(&gate.source), gate.span.start, gate.span.end, json_location(&gate.source, gate.span), escape(&gate.mode), gate.reason.as_ref().map(|reason| format!("\"{}\"", escape(reason))).unwrap_or_else(|| "null".to_string()));
        strings(&gate.provenance);
        print!("],\"operations\":[");
        for (operation_index, operation) in gate.operations.iter().enumerate() {
            if operation_index > 0 { print!(","); }
            print!("{{\"kind\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}},\"location\":{},\"required\":[", escape(&operation.kind), operation.span.start, operation.span.end, json_location(&gate.source, operation.span));
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
        print!("\"{}\"", escape(value));
    }
}

fn location(source_path: &str, span: Span) -> String {
    let source = fs::read_to_string(source_path).unwrap_or_default();
    let (line, column) = jet::Diagnostics::span_line_col(&source, span.start);
    format!("{source_path}:{line}:{column}")
}

fn json_location(source_path: &str, span: Span) -> String {
    let source = fs::read_to_string(source_path).unwrap_or_default();
    let (start_line, start_column) = jet::Diagnostics::span_line_col(&source, span.start);
    let (end_line, end_column) = jet::Diagnostics::span_line_col(&source, span.end);
    format!(
        "{{\"start\":{{\"line\":{start_line},\"column\":{start_column}}},\"end\":{{\"line\":{end_line},\"column\":{end_column}}}}}"
    )
}

fn escape(value: &str) -> String { value.chars().flat_map(|character| match character { '"' => "\\\"".chars().collect::<Vec<_>>(), '\\' => "\\\\".chars().collect(), '\n' => "\\n".chars().collect(), '\r' => "\\r".chars().collect(), '\t' => "\\t".chars().collect(), other => vec![other] }).collect() }
