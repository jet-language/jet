//! Read-only projections owned by `jet inspect`.

use std::fmt::Write as _;
use std::process::exit;

use jet::Diagnostics::Diagnostic;
use jet::Sema::GateLedger::{GateKind, GateLedger};
use jet_foundation::JSON::json_escape;
use jet_foundation::Policy::{AppliedRule, PolicyScope, RuleResolution, RuleStatus};
use jet_foundation::Registry;

/// `jet inspect digest` — emit the one-file language surface used by agents.
/// Every registry-shaped section is projected from the same typed rows used by
/// the compiler's introspection and report surfaces.
pub(crate) fn run_digest(json: bool) {
    let digest = llm_digest();
    if json {
        println!(
            "{{\"schema_version\":1,\"digest\":\"{}\"}}",
            json_escape(&digest)
        );
    } else {
        print!("{digest}");
    }
}

fn llm_digest() -> String {
    let marker_text = digest_marker_text();
    let diagnostic_text = digest_diagnostic_text();
    let core_text = digest_core_module_text();
    let canonical = include_str!("../examples/canon.jet").trim();

    let first_program_body = [
        "A source file ends with one `fn run()` entry. `print` is a built-in.",
        "",
        "```jet",
        "fn run() {",
        "    greeting :: \"Hello, Jet\"",
        "    print(greeting)",
        "}",
        "```",
        "",
        "No semicolons. Comments start with `//`. Strings use double quotes and interpolate `{name}`.",
    ]
    .join("\n");
    let core_rules_body = [
        "Bindings: `name :: value` is immutable; `name := value` is mutable; `name = value` reassigns a mutable binding.",
        "Functions: `fn name(parameter: Type) => Return { ... }`; expression bodies use `:: expression`.",
        "Visibility: declarations are private by default; prefix an item with `pub` for package use.",
        "Types: `Int`, `Float`, `Bool`, `String`, `Char`; lists use `[T]`; optional values use `T?`; failures use `T ? E`.",
        "Errors: handle `T?` or `T ? E` with `?? fallback`, `?`, or a pattern test. Use `Ok(value)`, `Err(error)`, `Val(value)`, and `None`.",
        "Control: `if condition { ... } else { ... }`; collecting loops use `loop name, source { ... }`; exit with `break` and advance with `next`.",
        "Construction: use `Type.{ field: value }`; list literals use `[T].{ value1, value2 }`.",
        "Calls and member access use `name(args)` and `value.member(args)`. Core imports use `use core.module as alias`.",
        "Ownership is safe by default. `&T` writes, `^T` moves, and `~value` copies. Expert unsafe code needs `#Unsafe(\"reason\")`.",
    ]
    .join("\n");
    let canonical_body = [
        "Read this as working source syntax. It is the checked executable showcase in `examples/canon.jet`.",
        "",
        "```jet",
        canonical,
        "```",
    ]
    .join("\n");
    let marker_body = format!(
        "User marker spelling is `#Name(arguments)`; rows below are registry declarations.\n\n```text\n{marker_text}\n```"
    );
    let core_body = format!(
        "Use a module alias, then call an indexed item: `use core.term as term`; `term.eprint(\"hi\")`.\n\n```text\n{core_text}\n```"
    );
    let diagnostic_body = format!(
        "Diagnostic rows use current registry meaning. Match code first; follow `fix`. Rows marked retired or reserved are not current syntax.\n\n```text\n{diagnostic_text}\n```"
    );

    let out = [
        "# Jet LLM surface digest".to_string(),
        String::new(),
        "Generated. Current compiler registries own markers, diagnostics, syntax names, and Core items.".to_string(),
        "Regenerate with `jet inspect digest`; CI compares the bytes.".to_string(),
        "Use active rows only. Retired rows teach replacement; they are not valid current source.".to_string(),
        "Write one current program. Do not invent aliases, legacy spellings, or library namespaces.".to_string(),
        String::new(),
        digest_section("First program", &first_program_body),
        digest_section("Core source rules", &core_rules_body),
        digest_section("Canonical compiling example", &canonical_body),
        digest_section("Keywords", &digest_list(jet::Syntax::JET_KEYWORD_LIST)),
        digest_section("Built-in type names", &digest_list(jet::Syntax::JET_TYPE_LIST)),
        digest_section(
            "Reserved first-party names",
            &digest_list(jet::Syntax::FIRST_PARTY_RESERVED),
        ),
        digest_section("Markers", &marker_body),
        digest_section("Core module index", &core_body),
        digest_section("Diagnostics", &diagnostic_body),
    ]
    .join("\n");
    format!("{}\n", out.trim_end())
}

fn digest_section(title: &str, body: &str) -> String {
    format!("## {title}\n\n{}\n", body.trim())
}

fn digest_list(values: &[&str]) -> String {
    let mut unique = Vec::new();
    for value in values {
        if !unique.iter().any(|seen| seen == value) {
            unique.push(*value);
        }
    }
    unique
        .into_iter()
        .map(|value| format!("- {value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn digest_marker_text() -> String {
    let mut out = String::from("status\tname\tregistered declaration");
    for row in Registry::marker_rows() {
        let rule = row
            .rule
            .expect("every marker registry row has an applied rule");
        let status = match rule.status {
            RuleStatus::Active => "active",
            RuleStatus::Retired { .. } => "retired",
        };
        let _ = write!(
            out,
            "\n{}\t{}\t{}",
            status,
            row.name,
            digest_marker_declaration(rule)
        );
    }
    out
}

fn digest_marker_declaration(rule: &AppliedRule) -> String {
    let mut fields = rule
        .signature
        .params
        .iter()
        .map(|param| {
            let default = param
                .default
                .map_or(String::new(), |value| format!(" = {value}"));
            format!("{}: {}{default}", param.name, param.source_type)
        })
        .collect::<Vec<_>>();
    if let Some(source_type) = rule.signature.variadic_source_type {
        fields.push(format!("{source_type}..."));
    }
    fields.push(format!(
        "@sites: [{}]",
        rule.sites
            .iter()
            .map(|site| format!(".{}", site.name()))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    if rule.repeatable {
        fields.push("@repeatable: true".to_string());
    }
    if rule.owns_menu {
        fields.push("@owns_menu: true".to_string());
    }
    if rule.inherits {
        fields.push("@inherits: true".to_string());
    }
    if rule.resolution != RuleResolution::SiteBound {
        fields.push(format!(
            "@resolution: .{}",
            digest_resolution_name(rule.resolution)
        ));
    }
    if !rule.policy_scopes.is_empty() {
        fields.push(format!(
            "@scopes: [{}]",
            rule.policy_scopes
                .iter()
                .map(|scope| format!(".{}", digest_scope_name(*scope)))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(companion) = rule.companion_site {
        fields.push(format!(
            "@companion: [{}, .{}]",
            companion.rule,
            companion.site.name()
        ));
    }
    if let RuleStatus::Retired { replacement } = rule.status {
        fields.push(format!(
            "@retired: \"{}\"",
            digest_quote(replacement)
        ));
    }
    format!("marker {}({})", rule.name, fields.join(", "))
}

fn digest_scope_name(scope: PolicyScope) -> &'static str {
    match scope {
        PolicyScope::Organization => "Organization",
        PolicyScope::Package => "Package",
        PolicyScope::Module => "Module",
        PolicyScope::Function => "Function",
        PolicyScope::Block => "Block",
    }
}

fn digest_resolution_name(resolution: RuleResolution) -> &'static str {
    match resolution {
        RuleResolution::SiteBound => "SiteBound",
        RuleResolution::Override => "Override",
        RuleResolution::Merge => "Merge",
        RuleResolution::Tighten => "Tighten",
    }
}

fn digest_quote(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn digest_diagnostic_text() -> String {
    let mut out = String::from(
        "code\tstatus\tstage\tseverity\tmoment\tmeaning\twhat\twhy\tfix\tdetail\tstructured-fix",
    );
    for row in Registry::diagnostic_rows() {
        let severity = match row.severity {
            jet_foundation::Diagnostics::Severity::Error => "error",
            jet_foundation::Diagnostics::Severity::Lint => "lint",
        };
        let structured_fix = row
            .structured_fix
            .map_or_else(|| "-".to_string(), |fix| fix.source_marker());
        let _ = write!(
            out,
            "\n{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            row.code,
            row.status.name(),
            row.stage,
            severity,
            row.moment.as_str(),
            digest_one_line(row.meaning),
            digest_one_line(row.what),
            digest_one_line(row.why),
            digest_one_line(row.fix),
            row.detail,
            structured_fix,
        );
    }
    out
}

fn digest_one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn digest_core_module_text() -> String {
    let mut out = String::from("module\titems");
    for &module in jet::Syntax::KNOWN_CORE_MODULES {
        let items = jet::Sema::core_module_items(module);
        let rendered = digest_core_items(&items);
        let _ = write!(out, "\n{module}\t{rendered}");
    }
    out
}

fn digest_core_items(items: &[String]) -> String {
    let mut unique = Vec::new();
    for item in items {
        if !unique.iter().any(|seen| seen == item) {
            unique.push(item.clone());
        }
    }
    if unique.is_empty() {
        "(no indexed item)".to_string()
    } else {
        unique.join(", ")
    }
}

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
                        &jet::Diagnostics::ReportPath::from_path(&file),
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
