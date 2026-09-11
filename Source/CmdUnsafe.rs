//! D-UNSAFE-OBLIG1=A: the unsafe projection of the shared gate ledger.

use std::process::exit;

use jet::Diagnostics::Span;
use jet::Sema::GateLedger::{GateEntry, GateKind, GateLedger};
use jet::AST::ProgramBundle;
use jet_foundation::Report::{StatusEnvelope, StatusFields, StatusValue};


pub(crate) fn run(args: &[String], json: bool, color: bool, gates: jet::Policy::GateSet) {
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
        crate::cli_error!(@fix "E2104", "`jet inspect unsafe` needs an entry file", "jet inspect unsafe run.jet");
        exit(jet::ExitCodes::USAGE);
    };
    let bundle = jet::Loader::load_entry_with_diagnostics(file).unwrap_or_else(|diagnostics| {
        if json {
            let reports = diagnostics
                .iter()
                .map(|entry| {
                    let machine_file = crate::machine_report_path_for_entry(file, &entry.file);
                    entry.diagnostic.to_report(&machine_file, &entry.source)
                })
                .collect::<Vec<_>>();
            println!(
                "{}",
                StatusEnvelope::new("inspect.gates", false)
                    .with_reports(reports)
                    .json()
            );
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
        let reports = ledger
            .diagnostics()
            .iter()
            .map(|entry| {
                let source = module_source(bundle, &entry.source);
                let machine_file = crate::machine_report_path_for_bundle(bundle, &entry.source);
                entry.diagnostic.to_report(&machine_file, &source)
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            StatusEnvelope::new("inspect.gates", false)
                .with_reports(reports)
                .json()
        );
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
                if operation.discharged {
                    "discharged"
                } else {
                    "missing"
                },
                operation.required.join(","),
                operation.asserted.join(",")
            );
        }
    }
}

fn render_json(entries: &[&GateEntry], bundle: &ProgramBundle) {
    let gates = StatusValue::array(entries.iter().map(|entry| {
        let source = module_source(bundle, &entry.source);
        let span = entry.span.unwrap_or(Span::new(0, 0));
        let location = entry
            .span
            .map(|span| status_location_value(&source, span))
            .unwrap_or(StatusValue::Null);
        let reason = entry
            .reason
            .as_deref()
            .map(StatusValue::from)
            .unwrap_or(StatusValue::Null);
        let provenance = StatusValue::array(
            entry
                .provenance
                .iter()
                .map(|value| StatusValue::from(value.as_str())),
        );
        let operations = StatusValue::array(entry.operations.iter().map(|operation| {
            StatusValue::object(
                StatusFields::new()
                    .with("kind", operation.kind.as_str())
                    .with(
                        "span",
                        StatusValue::object(
                            StatusFields::new()
                                .with("start", operation.span.start)
                                .with("end", operation.span.end),
                        ),
                    )
                    .with("location", status_location_value(&source, operation.span))
                    .with(
                        "required",
                        StatusValue::array(
                            operation
                                .required
                                .iter()
                                .map(|value| StatusValue::from(value.as_str())),
                        ),
                    )
                    .with(
                        "asserted",
                        StatusValue::array(
                            operation
                                .asserted
                                .iter()
                                .map(|value| StatusValue::from(value.as_str())),
                        ),
                    )
                    .with("discharged", operation.discharged),
            )
        }));
        StatusValue::object(
            StatusFields::new()
                .with("source", entry.source.as_str())
                .with(
                    "span",
                    StatusValue::object(
                        StatusFields::new()
                            .with("start", span.start)
                            .with("end", span.end),
                    ),
                )
                .with("location", location)
                .with("mode", entry.detail.strip_prefix("mode=").unwrap_or(&entry.detail))
                .with("reason", reason)
                .with("provenance", provenance)
                .with("operations", operations),
        )
    }));
    println!(
        "{}",
        StatusEnvelope::new("inspect.unsafe", true)
            .with_field("gates", gates)
            .json()
    );
}

fn status_location_value(source: &str, span: Span) -> StatusValue {
    let (start_line, start_column) = jet::Diagnostics::span_line_col(source, span.start);
    let (end_line, end_column) = jet::Diagnostics::span_line_col(source, span.end);
    StatusValue::object(
        StatusFields::new()
            .with(
                "start",
                StatusValue::object(
                    StatusFields::new()
                        .with("line", start_line)
                        .with("column", start_column),
                ),
            )
            .with(
                "end",
                StatusValue::object(
                    StatusFields::new()
                        .with("line", end_line)
                        .with("column", end_column),
                ),
            ),
    )
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

