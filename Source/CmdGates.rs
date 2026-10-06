//! D-FACT-GATE1=A: the full compile-time gate ledger and its projections.

use std::collections::BTreeMap;
use std::process::exit;

use jet::Diagnostics::Span;
use jet::Sema::GateLedger::{GateEntry, GateKind, GateLedger};
use jet::GateWriters::{append_invocation_flags, external_entry};
use jet_foundation::Report::{StatusEnvelope, StatusFields, StatusValue};

const LARGE_KIND_THRESHOLD: usize = 16;

pub(crate) fn run(args: &[String], json: bool, color: bool, gates: jet::Policy::GateSet) {
    let Some(file) = entry_file(args) else {
        crate::cli_error!(@fix "E2104", "`jet inspect gates` needs an entry file", "jet inspect gates run.jet");
        exit(jet::ExitCodes::USAGE);
    };
    let bundle = jet::Loader::load_entry_with_diagnostics(&file).unwrap_or_else(|diagnostics| {
        if json {
            let reports = diagnostics
                .iter()
                .map(|entry| {
                    let machine_file = crate::machine_report_path_for_entry(&file, &entry.file);
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

    let kind = option_value(args, "--kind");
    let kind = kind.as_deref().map(parse_kind).transpose().unwrap_or_else(|error| {
        crate::cli_error!(@fix "E2104", error, "use one of unsafe, impure, dependency_grant, build_flag, session_flag, trust_grant, force_pin, taint_scrub, duty_drop, state_transition, precision_demotion, nondeterministic, structure, or lint_allow");
        exit(jet::ExitCodes::USAGE);
    });
    let scope = option_value(args, "--scope").map(|value| value.to_ascii_lowercase());

    let mut ledger = GateLedger::collect(&bundle, gates);
    append_external_writers(&mut ledger, &bundle, args);
    let mut diagnostics = ledger.diagnostics().to_vec();
    diagnostics.extend(ledger.policy_diagnostics(&bundle).into_iter().map(|diagnostic| {
        jet::Sema::GateLedger::GateDiagnostic {
            source: diagnostic.origin().map_or_else(|| file.clone(), |origin| origin.display.clone()),
            diagnostic,
        }
    }));
    ledger.set_diagnostics(diagnostics);
    if !ledger.diagnostics().is_empty() {
        render_diagnostics(&ledger, &bundle, json, color);
    }
    ledger.sort();
    let entries = ledger
        .entries()
        .iter()
        .filter(|entry| kind.is_none_or(|wanted| entry.kind == wanted))
        .filter(|entry| scope_matches(entry, scope.as_deref()))
        .collect::<Vec<_>>();

    if json {
        render_json(&entries, &bundle);
    } else {
        render_human(&entries, &bundle);
    }
}

fn entry_file(args: &[String]) -> Option<String> {
    let mut skip_value = false;
    for argument in args {
        if skip_value {
            skip_value = false;
            continue;
        }
        if matches!(argument.as_str(), "--scope" | "--kind" | "--gate") {
            skip_value = true;
            continue;
        }
        if argument.starts_with("--scope=")
            || argument.starts_with("--kind=")
            || argument.starts_with("--gate=")
        {
            continue;
        }
        if !argument.starts_with('-') {
            return Some(argument.clone());
        }
    }
    None
}

fn option_value(args: &[String], name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    for (index, argument) in args.iter().enumerate() {
        if let Some(value) = argument.strip_prefix(&prefix) {
            return Some(value.to_string());
        }
        if argument == name {
            return args.get(index + 1).cloned();
        }
    }
    None
}

fn parse_kind(value: &str) -> Result<GateKind, String> {
    GateKind::parse(value).ok_or_else(|| format!("unknown gate kind `{value}`"))
}

fn scope_matches(entry: &GateEntry, scope: Option<&str>) -> bool {
    let Some(scope) = scope else { return true };
    scope == "all"
        || entry.scope.eq_ignore_ascii_case(scope)
        || entry.domain.eq_ignore_ascii_case(scope)
        || entry.source.eq_ignore_ascii_case(scope)
}


pub(crate) fn append_external_writers(
    ledger: &mut GateLedger,
    bundle: &jet::AST::ProgramBundle,
    args: &[String],
) {
    jet::GateWriters::append_package_writers(ledger, bundle);

    let trust_path = jetpack::Trust::store_path();
    for record in jetpack::Trust::list_records(&trust_path) {
        let (subject, scope, detail) = match record {
            jetpack::Trust::TrustRecord::Hash { hash } => (
                hash,
                "trust".to_string(),
                "trusted project hash".to_string(),
            ),
            jetpack::Trust::TrustRecord::Pattern { pattern } => (
                pattern,
                "trust".to_string(),
                "trusted project pattern".to_string(),
            ),
            jetpack::Trust::TrustRecord::Grant(grant) => (
                format!("{}:{}", grant.authority, grant.subject),
                grant.scope,
                "trusted authority grant".to_string(),
            ),
            jetpack::Trust::TrustRecord::Raw { line } => {
                (line, "trust".to_string(), "raw trust record".to_string())
            }
        };
        ledger.push(external_entry(
            GateKind::TrustGrant,
            "security",
            &scope,
            &trust_path.display().to_string(),
            &subject,
            &detail,
            vec![trust_path.display().to_string()],
        ));
    }

    append_invocation_flags(ledger, args);
}


fn render_diagnostics(
    ledger: &GateLedger,
    bundle: &jet::AST::ProgramBundle,
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

fn render_human(entries: &[&GateEntry], bundle: &jet::AST::ProgramBundle) {
    println!("gates: {}", entries.len());
    let mut index = 0;
    while index < entries.len() {
        let kind = entries[index].kind;
        let end = entries[index..]
            .iter()
            .position(|entry| entry.kind != kind)
            .map(|offset| index + offset)
            .unwrap_or(entries.len());
        let group = &entries[index..end];
        if group.len() >= LARGE_KIND_THRESHOLD && !kind.is_security() {
            println!("  {}: {} entries", kind.name(), group.len());
        } else {
            println!("  {}: {}", kind.name(), group.len());
            for entry in group {
                print_entry_human(entry, bundle);
            }
        }
        index = end;
    }
}

fn print_entry_human(entry: &GateEntry, bundle: &jet::AST::ProgramBundle) {
    let source = module_source(bundle, &entry.source);
    let entry_location = entry
        .span
        .map(|span| location(&entry.source, &source, span))
        .unwrap_or_else(|| entry.source.clone());
    let status = entry
        .status
        .as_deref()
        .map(|status| format!(" status={status}"))
        .unwrap_or_default();
    let reason = entry
        .reason
        .as_deref()
        .map(|reason| format!(" reason={reason}"))
        .unwrap_or_default();
    println!(
        "    {}  {}  {}{}{}",
        entry_location, entry.subject, entry.detail, status, reason
    );
    for provenance in &entry.provenance {
        println!("      provenance {provenance}");
    }
    for operation in &entry.operations {
        println!(
            "      {}  {}  {}  required=[{}] asserted=[{}]",
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

fn render_json(entries: &[&GateEntry], bundle: &jet::AST::ProgramBundle) {
    let mut counts = BTreeMap::<&str, usize>::new();
    for entry in entries {
        *counts.entry(entry.kind.name()).or_default() += 1;
    }
    let entries = StatusValue::array(
        entries
            .iter()
            .map(|entry| status_entry_value(entry, bundle)),
    );
    let mut count_fields = StatusFields::new();
    for (kind, count) in counts {
        count_fields = count_fields.with(kind, count);
    }
    let gates = StatusValue::object(
        StatusFields::new()
            .with("entries", entries)
            .with("counts", StatusValue::object(count_fields)),
    );
    println!(
        "{}",
        StatusEnvelope::new("inspect.gates", true)
            .with_field("gates", gates)
            .json()
    );
}

fn status_entry_value(entry: &GateEntry, bundle: &jet::AST::ProgramBundle) -> StatusValue {
    let source = module_source(bundle, &entry.source);
    let span = entry
        .span
        .map(|span| {
            StatusValue::object(
                StatusFields::new()
                    .with("start", span.start)
                    .with("end", span.end),
            )
        })
        .unwrap_or(StatusValue::Null);
    let location = entry
        .span
        .map(|span| status_location_value(&source, span))
        .unwrap_or(StatusValue::Null);
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
            .with("kind", entry.kind.name())
            .with("domain", entry.domain.as_str())
            .with("scope", entry.scope.as_str())
            .with("source", entry.source.as_str())
            .with("span", span)
            .with("location", location)
            .with("subject", entry.subject.as_str())
            .with("reason", reason)
            .with("status", status)
            .with("detail", entry.detail.as_str())
            .with("provenance", provenance)
            .with("operations", operations),
    )
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


fn module_source(bundle: &jet::AST::ProgramBundle, display: &str) -> String {
    bundle
        .modules
        .iter()
        .find(|module| module.display == display || module.path.to_string_lossy() == display)
        .map(|module| module.source.clone())
        .unwrap_or_default()
}

fn location(source_path: &str, source: &str, span: Span) -> String {
    let (line, column) = jet::Diagnostics::span_line_col(source, span.start);
    format!("{source_path}:{line}:{column}")
}

