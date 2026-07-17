//! D-UNSAFE-OBLIG1=A: deterministic `jet inspect unsafe` audit report.

use std::process::exit;

pub(crate) fn run(args: &[String], json: bool) {
    let Some(file) = args.iter().find(|argument| !argument.starts_with('-')) else {
        eprintln!("error: `jet inspect unsafe` needs an entry file");
        eprintln!(" fix: jet inspect unsafe Source/main.jet");
        exit(jet::ExitCodes::USAGE);
    };
    let bundle = jet::Loader::load_entry(file).unwrap_or_else(|diagnostics| {
        for diagnostic in diagnostics { eprintln!("Error [{}]: {}", diagnostic.code, diagnostic.what); }
        exit(jet::ExitCodes::USER_ERROR);
    });
    let report = jet::Sema::UnsafeObligations::inspect(&bundle);
    if json { render_json(&report); } else { render_human(&report); }
}

fn render_human(report: &jet::Sema::UnsafeObligations::UnsafeInspection) {
    println!("unsafe gates: {}", report.gates.len());
    for gate in &report.gates {
        println!("{}:{}..{}  {}  reason={}", gate.source, gate.span.start, gate.span.end, gate.mode, gate.reason.as_deref().unwrap_or("<missing>"));
        for source in &gate.provenance { println!("  policy {source}"); }
        for operation in &gate.operations {
            println!("  {} {}..{}  {}  required=[{}] asserted=[{}]", operation.kind, operation.span.start, operation.span.end, if operation.discharged { "discharged" } else { "missing" }, operation.required.join(","), operation.asserted.join(","));
        }
    }
}

fn render_json(report: &jet::Sema::UnsafeObligations::UnsafeInspection) {
    print!("{{\"schema_version\":1,\"gates\":[");
    for (gate_index, gate) in report.gates.iter().enumerate() {
        if gate_index > 0 { print!(","); }
        print!("{{\"source\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}},\"mode\":\"{}\",\"reason\":{},\"provenance\":[", escape(&gate.source), gate.span.start, gate.span.end, escape(&gate.mode), gate.reason.as_ref().map(|reason| format!("\"{}\"", escape(reason))).unwrap_or_else(|| "null".to_string()));
        strings(&gate.provenance);
        print!("],\"operations\":[");
        for (operation_index, operation) in gate.operations.iter().enumerate() {
            if operation_index > 0 { print!(","); }
            print!("{{\"kind\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}},\"required\":[", escape(&operation.kind), operation.span.start, operation.span.end);
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

fn escape(value: &str) -> String { value.chars().flat_map(|character| match character { '"' => "\\\"".chars().collect::<Vec<_>>(), '\\' => "\\\\".chars().collect(), '\n' => "\\n".chars().collect(), '\r' => "\\r".chars().collect(), '\t' => "\\t".chars().collect(), other => vec![other] }).collect() }
