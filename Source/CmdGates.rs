//! D-FACT-GATE1=A: the full compile-time gate ledger and its projections.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::exit;

use jet::Diagnostics::Span;
use jet::Sema::GateLedger::{GateEntry, GateKind, GateLedger};
use jet_foundation::JSON::json_escape;

const LARGE_KIND_THRESHOLD: usize = 16;

pub(crate) fn run(
    args: &[String],
    json: bool,
    color: bool,
    gates: jet::Policy::GateSet,
    authority_only: bool,
) {
    let Some(file) = entry_file(args) else {
        crate::cli_error!(@fix "E2104", "`jet inspect gates` needs an entry file", "jet inspect gates Source/main.jet");
        exit(jet::ExitCodes::USAGE);
    };
    let bundle = jet::Loader::load_entry_with_diagnostics(&file).unwrap_or_else(|diagnostics| {
        if json {
            for entry in &diagnostics {
                let machine_file = crate::machine_report_path_for_entry(&file, &entry.file);
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

    let kind = option_value(args, "--kind");
    let kind = kind.as_deref().map(parse_kind).transpose().unwrap_or_else(|error| {
        crate::cli_error!(@fix "E2104", error, "use one of unsafe, impure, dependency_grant, build_flag, session_flag, trust_grant, force_pin, taint_scrub, duty_drop, precision_demotion, or nondeterministic");
        exit(jet::ExitCodes::USAGE);
    });
    let scope = option_value(args, "--scope").map(|value| value.to_ascii_lowercase());

    let mut ledger = GateLedger::collect(&bundle, gates);
    append_external_writers(&mut ledger, &bundle.project_root, args);
    if !ledger.diagnostics().is_empty() {
        render_diagnostics(&ledger, &bundle, json, color);
    }
    ledger.sort();
    let entries = ledger
        .entries()
        .iter()
        .filter(|entry| kind_matches(entry, kind, authority_only))
        .filter(|entry| scope_matches(entry, scope.as_deref()))
        .collect::<Vec<_>>();

    if json {
        render_json(&entries, &bundle);
    } else {
        render_human(&entries, &bundle, authority_only);
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
        if argument.starts_with("--scope=") || argument.starts_with("--kind=") || argument.starts_with("--gate=") {
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

fn kind_matches(entry: &GateEntry, kind: Option<GateKind>, authority_only: bool) -> bool {
    if authority_only && !entry.kind.is_rights_kind() {
        return false;
    }
    kind.is_none_or(|wanted| entry.kind == wanted)
}

fn scope_matches(entry: &GateEntry, scope: Option<&str>) -> bool {
    let Some(scope) = scope else { return true };
    scope == "all"
        || entry.scope.eq_ignore_ascii_case(scope)
        || entry.domain.eq_ignore_ascii_case(scope)
        || entry.source.eq_ignore_ascii_case(scope)
}

fn append_external_writers(ledger: &mut GateLedger, root: &Path, args: &[String]) {
    if let Some(Ok(facts)) = jet::Package::PackageFacts::load(root) {
        for (dependency, effects) in &facts.grants {
            let mut provenance = facts.field_provenance("grants").to_vec();
            if provenance.is_empty() {
                provenance.push("package.jet:grants".to_string());
            }
            ledger.push(external_entry(
                GateKind::DependencyGrant,
                "package",
                "package",
                "package.jet",
                dependency,
                &format!("effects: {}", effects.join(",")),
                provenance,
            ));
        }
        for effect in &facts.build_allow {
            let mut provenance = facts.field_provenance("build_allow").to_vec();
            if provenance.is_empty() {
                provenance.push("package.jet:build.allow".to_string());
            }
            ledger.push(external_entry(
                GateKind::BuildFlag,
                "security",
                "package",
                "package.jet",
                &format!("build:{effect}"),
                "package build capability",
                provenance,
            ));
        }
        if let Some(effects) = &facts.effects_allow {
            if !effects.is_empty() {
                let mut provenance = facts.field_provenance("effects_allow").to_vec();
                if provenance.is_empty() {
                    provenance.push("package.jet:effects.allow".to_string());
                }
                ledger.push(external_entry(
                    GateKind::BuildFlag,
                    "security",
                    "package",
                    "package.jet",
                    "effects:allow",
                    &format!("effects: {}", effects.join(",")),
                    provenance,
                ));
            }
        }
    }

    if let Some(lock) = jet::Lock::load(root) {
        for package in &lock.packages {
            if !package.effect_grants.is_empty() {
                ledger.push(external_entry(
                    GateKind::DependencyGrant,
                    "security",
                    "package",
                    jet::Syntax::UNIFIED_LOCK_FILE,
                    &package.name,
                    &format!("effects: {}", package.effect_grants.join(",")),
                    vec![format!("{}:dependency.effect-grants", jet::Syntax::UNIFIED_LOCK_FILE)],
                ));
            }
        }
        for (subject, effects) in &lock.workspace_overlay_policy.build_grants {
            ledger.push(external_entry(
                GateKind::DependencyGrant,
                "security",
                "package",
                jet::Syntax::UNIFIED_LOCK_FILE,
                &format!("build:{subject}"),
                &format!("workspace capabilities: {}", effects.join(",")),
                vec![format!("{}:workspace.build-grants", jet::Syntax::UNIFIED_LOCK_FILE)],
            ));
        }
        for overlay in &lock.workspace_overlay_policy.overlays {
            for package in &overlay.packages {
                let forced = package.priority >= 100
                    || package.field_priorities.values().any(|priority| *priority >= 100);
                if !forced {
                    continue;
                }
                let fields = package
                    .field_priorities
                    .iter()
                    .filter(|(_, priority)| **priority >= 100)
                    .map(|(field, _)| field.as_str())
                    .collect::<Vec<_>>();
                let detail = if fields.is_empty() {
                    "workspace override".to_string()
                } else {
                    format!("workspace override fields: {}", fields.join(","))
                };
                ledger.push(external_entry(
                    GateKind::ForcePin,
                    "security",
                    "package",
                    jet::Syntax::UNIFIED_LOCK_FILE,
                    &package.package,
                    &detail,
                    vec![format!(
                        "{}:overlay {} priority=Force",
                        jet::Syntax::UNIFIED_LOCK_FILE,
                        overlay.name
                    )],
                ));
            }
        }
    }

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
            jetpack::Trust::TrustRecord::Raw { line } => (
                line,
                "trust".to_string(),
                "raw trust record".to_string(),
            ),
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

fn append_invocation_flags(ledger: &mut GateLedger, args: &[String]) {
    let mut index = 0;
    while index < args.len() {
        let argument = &args[index];
        if let Some(spec) = argument.strip_prefix("--gate=") {
            ledger.push(external_entry(
                GateKind::BuildFlag,
                "security",
                "build",
                "command line",
                &format!("--gate {spec}"),
                "audited invocation allowance",
                vec!["command line".to_string()],
            ));
        } else if argument == "--gate" {
            if let Some(spec) = args.get(index + 1) {
                ledger.push(external_entry(
                    GateKind::BuildFlag,
                    "security",
                    "build",
                    "command line",
                    &format!("--gate {spec}"),
                    "audited invocation allowance",
                    vec!["command line".to_string()],
                ));
                index += 1;
            }
        } else if argument.starts_with("--allow-") {
            ledger.push(external_entry(
                GateKind::BuildFlag,
                "security",
                "build",
                "command line",
                argument,
                "build capability allowance",
                vec!["command line".to_string()],
            ));
        } else if argument.starts_with("--deny-")
            || matches!(argument.as_str(), "--trust" | "--online" | "--try-anyway" | "--interpret" | "--offline" | "--locked")
        {
            ledger.push(external_entry(
                GateKind::SessionFlag,
                "security",
                "session",
                "command line",
                argument,
                "session bypass or trust choice",
                vec!["command line".to_string()],
            ));
        } else if argument == "--freestanding"
            || argument == "--force"
            || argument == "--release"
            || argument.starts_with("--target=")
            || argument.starts_with("--profile=")
        {
            ledger.push(external_entry(
                GateKind::BuildFlag,
                "security",
                "build",
                "command line",
                argument,
                "build policy choice",
                vec!["command line".to_string()],
            ));
        }
        index += 1;
    }
}

fn external_entry(
    kind: GateKind,
    domain: &str,
    scope: &str,
    source: &str,
    subject: &str,
    detail: &str,
    provenance: Vec<String>,
) -> GateEntry {
    GateEntry {
        kind,
        domain: domain.to_string(),
        scope: scope.to_string(),
        source: source.to_string(),
        span: None,
        subject: subject.to_string(),
        reason: None,
        status: Some("recorded".to_string()),
        detail: detail.to_string(),
        provenance,
        operations: Vec::new(),
    }
}

fn render_diagnostics(
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

fn render_human(entries: &[&GateEntry], bundle: &jet::AST::ProgramBundle, authority_only: bool) {
    println!(
        "{} gates: {}",
        if authority_only { "authority" } else { "gates" },
        entries.len()
    );
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
            if operation.discharged { "discharged" } else { "missing" },
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
    print!("{{\"schema_version\":1,\"entries\":[");
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            print!(",");
        }
        render_json_entry(entry, bundle);
    }
    print!("],\"counts\":{{");
    for (index, (kind, count)) in counts.iter().enumerate() {
        if index > 0 {
            print!(",");
        }
        print!("\"{}\":{}", json_escape(kind), count);
    }
    println!("}}}}");
}

fn render_json_entry(entry: &GateEntry, bundle: &jet::AST::ProgramBundle) {
    let source = module_source(bundle, &entry.source);
    print!(
        "{{\"kind\":\"{}\",\"domain\":\"{}\",\"scope\":\"{}\",\"source\":\"{}\",\"span\":{},\"location\":{},\"subject\":\"{}\",\"reason\":{},\"status\":{},\"detail\":\"{}\",\"provenance\":[",
        json_escape(entry.kind.name()),
        json_escape(&entry.domain),
        json_escape(&entry.scope),
        json_escape(&entry.source),
        json_span(entry.span),
        json_location_option(&source, entry.span),
        json_escape(&entry.subject),
        json_option(entry.reason.as_deref()),
        json_option(entry.status.as_deref()),
        json_escape(&entry.detail),
    );
    strings(&entry.provenance);
    print!("],\"operations\":[");
    for (index, operation) in entry.operations.iter().enumerate() {
        if index > 0 {
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

fn strings(values: &[String]) {
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            print!(",");
        }
        print!("\"{}\"", json_escape(value));
    }
}

fn json_option(value: Option<&str>) -> String {
    value
        .map(|value| format!("\"{}\"", json_escape(value)))
        .unwrap_or_else(|| "null".to_string())
}

fn json_span(span: Option<Span>) -> String {
    span.map(|span| format!("{{\"start\":{},\"end\":{}}}", span.start, span.end))
        .unwrap_or_else(|| "null".to_string())
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

fn json_location_option(source: &str, span: Option<Span>) -> String {
    span.map(|span| json_location(source, span))
        .unwrap_or_else(|| "null".to_string())
}

fn json_location(source: &str, span: Span) -> String {
    let (start_line, start_column) = jet::Diagnostics::span_line_col(source, span.start);
    let (end_line, end_column) = jet::Diagnostics::span_line_col(source, span.end);
    format!(
        "{{\"start\":{{\"line\":{},\"column\":{}}},\"end\":{{\"line\":{},\"column\":{}}}}}",
        start_line, start_column, end_line, end_column
    )
}
