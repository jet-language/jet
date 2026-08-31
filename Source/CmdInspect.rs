//! Read-only projections owned by `jet inspect`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::Instant;

use jet::Diagnostics::Diagnostic;
use jet::Sema::GateLedger::{GateKind, GateLedger};
use jet_foundation::Policy::{AppliedRule, PolicyScope, RuleResolution, RuleStatus};
use jet_foundation::Registry;
use jet_foundation::Report::render_status_json;
use jet_foundation::JSON::json_escape;

/// One checked source projection shared by inspect and compile handlers.
///
/// The programmable-build preflight and ordinary sema check both feed this
/// value. Consumers must project its bundle, facts, index, and check record;
/// they must not reopen the entry file to answer the same question.
pub(crate) struct CheckProjection {
    pub(crate) bundle: jet::AST::ProgramBundle,
    pub(crate) facts: jet::Sema::SemIndexEffectFacts,
    pub(crate) index: jet_semindex::SemIndex,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) check: CheckResult,
}

const CHECK_RESULT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CheckScope {
    Project,
    ExplicitFile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CheckProofRow {
    output: String,
    name: &'static str,
    status: &'static str,
    detail: String,
    diagnostic: &'static str,
}

#[derive(Clone, Debug)]
struct ProjectOutputSpec {
    address: String,
    name: String,
    path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
struct ProjectOutputGroup {
    key: String,
    path: Option<PathBuf>,
    addresses: Vec<String>,
}

pub(crate) struct CheckResult {
    source: String,
    profile: String,
    front_end: &'static str,
    programmable_build: &'static str,
    diagnostics: usize,
    scope: CheckScope,
    elapsed_ms: u64,
    proof_rows: Vec<CheckProofRow>,
}

pub(crate) fn check_projection(path: &Path) -> Result<CheckProjection, Vec<Diagnostic>> {
    check_projection_with_options(
        path,
        jet::Policy::GateSet::default(),
        "dev",
        &BTreeMap::new(),
    )
}

pub(crate) fn check_projection_with_options(
    path: &Path,
    gates: jet::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> Result<CheckProjection, Vec<Diagnostic>> {
    check_projection_with_options_and_preflight(
        path,
        gates,
        profile,
        setting_overrides,
        None,
        false,
        CheckScope::ExplicitFile,
        None,
        None,
    )
}

/// Check the target selected by the command dispatcher.  A bare or directory
/// command is a project promise; an explicitly named file keeps semantic
/// scope, even when it lives inside a package.
pub(crate) fn check_projection_for_command(
    path: &Path,
    gates: jet::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    scope: CheckScope,
    entry_fn: Option<&str>,
    target: Option<&str>,
) -> Result<CheckProjection, Vec<Diagnostic>> {
    check_projection_with_options_and_preflight(
        path,
        gates,
        profile,
        setting_overrides,
        None,
        false,
        scope,
        entry_fn,
        target,
    )
}

pub(crate) fn check_projection_for_effects(
    path: &Path,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> Result<CheckProjection, Vec<Diagnostic>> {
    check_projection_with_options_and_preflight(
        path,
        jet::Policy::GateSet::default(),
        profile,
        setting_overrides,
        None,
        false,
        CheckScope::ExplicitFile,
        None,
        None,
    )
}

pub(crate) fn check_projection_for_run(
    path: &Path,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> Result<CheckProjection, Vec<Diagnostic>> {
    check_projection_with_options_and_preflight(
        path,
        jet::Policy::GateSet::default(),
        profile,
        setting_overrides,
        None,
        true,
        CheckScope::ExplicitFile,
        None,
        None,
    )
}

pub(crate) fn check_projection_for_output_effects(
    path: &Path,
    output: &str,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> Result<CheckProjection, Vec<Diagnostic>> {
    check_projection_with_options_and_preflight(
        path,
        jet::Policy::GateSet::default(),
        profile,
        setting_overrides,
        Some(output),
        false,
        CheckScope::ExplicitFile,
        None,
        None,
    )
}

fn check_projection_with_options_and_preflight(
    path: &Path,
    gates: jet::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    output: Option<&str>,
    run_mode: bool,
    scope: CheckScope,
    entry_fn: Option<&str>,
    target: Option<&str>,
) -> Result<CheckProjection, Vec<Diagnostic>> {
    let started = Instant::now();
    let entry = path.display().to_string();
    if scope == CheckScope::ExplicitFile {
        if let Some(diagnostic) = missing_project_context_diagnostic(path) {
            return Err(vec![diagnostic]);
        }
    }
    let mut programmable_build = "not-selected";
    let project_build = if scope == CheckScope::Project {
        jet::check_project_build_for_tier(
            &entry,
            gates,
            profile,
            setting_overrides,
            target,
            entry_fn,
        )?
    } else {
        None
    };
    let (mut diagnostics, bundle, facts, front_end) = if let Some(output) = project_build {
        programmable_build = "checked";
        let jet::Driver::BuildCompileOutput {
            compile,
            runtime,
            runtime_effect_facts,
            ..
        } = output;
        let bundle = runtime.ok_or_else(|| {
            vec![Diagnostic::from_row(
                "E2390",
                &[("detail", "project check returned no final runtime graph")],
                None,
            )]
        })?;
        let facts = runtime_effect_facts.ok_or_else(|| {
            vec![Diagnostic::from_row(
                "E2391",
                &[("detail", "project check returned no final runtime effect facts")],
                None,
            )]
        })?;
        (
            compile.lints,
            Some(bundle),
            facts,
            "Driver::check_project_build_for_tier",
        )
    } else if scope == CheckScope::Project {
        let (diagnostics, bundle, facts) =
            jet::Driver::check_file_with_effect_facts_for_run_and_entry(
                &entry,
                profile,
                setting_overrides,
                entry_fn,
            );
        (
            diagnostics,
            bundle,
            facts,
            "Driver::check_file_with_effect_facts_for_run_and_entry",
        )
    } else if let Some(output) = output {
        let (diagnostics, bundle, facts) = jet::Driver::check_file_with_effect_facts_for_output(
            &entry,
            output,
            profile,
            setting_overrides,
        );
        (
            diagnostics,
            bundle,
            facts,
            "Driver::check_file_with_effect_facts_for_output",
        )
    } else if run_mode {
        let (diagnostics, bundle, facts) =
            jet::Driver::check_file_with_effect_facts_for_run(&entry, profile, setting_overrides);
        (
            diagnostics,
            bundle,
            facts,
            "Driver::check_file_with_effect_facts_for_run",
        )
    } else if setting_overrides.is_empty() {
        let (diagnostics, bundle, facts) =
            jet::Driver::check_file_with_effect_facts_profile(&entry, None, false, profile);
        (
            diagnostics,
            bundle,
            facts,
            "Driver::check_file_with_effect_facts_profile",
        )
    } else {
        let (diagnostics, bundle, facts) = jet::Driver::check_file_with_effect_facts_and_settings(
            &entry,
            None,
            false,
            setting_overrides,
        );
        (
            diagnostics,
            bundle,
            facts,
            "Driver::check_file_with_effect_facts_and_settings",
        )
    };
    if scope == CheckScope::ExplicitFile
        && diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
    {
        return Err(diagnostics);
    }
    let Some(bundle) = bundle else {
        return Err(diagnostics);
    };
    let package_facts = jet_semindex::package_facts_for_entry(path)
        .map_err(|error| vec![jet_semindex::package_facts_diagnostic(path, &error)])?;
    let mut index = jet_semindex::from_checked(&bundle, &facts);
    if let Some(package_facts) = package_facts.clone() {
        index.attach_package_facts(package_facts);
    }
    if let Some(policy) = jet_semindex::workspace_overlay_policy_for_entry(path)
        .map_err(|diagnostic| vec![diagnostic])?
    {
        index.attach_workspace_overlay_policy(policy);
    }
    let (proof_rows, proof_diagnostics) = match scope {
        CheckScope::Project => project_proof_rows(
            &bundle,
            package_facts.as_ref(),
            entry_fn,
            target,
            profile,
            setting_overrides,
        ),
        CheckScope::ExplicitFile => (explicit_file_proof_rows(), Vec::new()),
    };
    diagnostics.extend(proof_diagnostics);
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
    {
        return Err(diagnostics);
    }
    let check = CheckResult {
        source: entry,
        profile: profile.to_string(),
        front_end,
        programmable_build,
        diagnostics: diagnostics.len(),
        scope,
        elapsed_ms: started.elapsed().as_millis() as u64,
        proof_rows,
    };
    Ok(CheckProjection {
        bundle,
        facts,
        index,
        diagnostics,
        check,
    })
}

fn scope_name(scope: CheckScope) -> &'static str {
    match scope {
        CheckScope::Project => "project",
        CheckScope::ExplicitFile => "explicit-file",
    }
}

fn missing_project_context_diagnostic(path: &Path) -> Option<Diagnostic> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if jet::Loader::find_manifest_root_checked(parent)
        .ok()
        .flatten()
        .is_some()
        || jet::Loader::find_workspace_root_checked(parent)
            .ok()
            .flatten()
            .is_some()
    {
        return None;
    }
    let source = fs::read_to_string(path).ok()?;
    let (tokens, lex_diagnostics) = jet::Lexer::lex(&source);
    if !lex_diagnostics.is_empty() {
        return None;
    }
    let program = jet::Parser::parse(&tokens).ok()?;
    let span = program.imports.iter().find_map(|import| match &import.kind {
        jet::AST::ImportKind::Module(name, span)
            if name.starts_with(jet::Syntax::PROJECT_IMPORT_PREFIX) => Some(*span),
        jet::AST::ImportKind::Unqualified {
            module_alias, span, ..
        } if module_alias.starts_with(jet::Syntax::PROJECT_IMPORT_PREFIX) => Some(*span),
        _ => None,
    })?;
    Some(Diagnostic::from_row(
        "E2393",
        &[("import", "use project.<module>")],
        Some(span),
    ))
}

fn explicit_file_proof_rows() -> Vec<CheckProofRow> {
    vec![
        CheckProofRow {
            name: "entry resolution",
            status: "not applicable",
            detail: "explicit-file checks resolve only the named source file".to_string(),
            diagnostic: "E2389",
        },
        CheckProofRow {
            name: "module graph",
            status: "not applicable",
            detail: "explicit-file checks keep semantic scope and do not promise a project graph".to_string(),
            diagnostic: "E2390",
        },
        CheckProofRow {
            name: "Core closure",
            status: "not applicable",
            detail: "explicit-file checks keep semantic scope and do not promise project Core closure".to_string(),
            diagnostic: "E2391",
        },
        CheckProofRow {
            name: "tier lowering",
            status: "not applicable",
            detail: "explicit-file checks keep semantic scope and do not promise project tier lowering".to_string(),
            diagnostic: "E2392",
        },
    ]
}

fn project_proof_rows(
    bundle: &jet::AST::ProgramBundle,
    entry_fn: Option<&str>,
    target: Option<&str>,
) -> (Vec<CheckProofRow>, Vec<Diagnostic>) {
    let mut rows = Vec::new();
    let mut diagnostics = Vec::new();
    // Entry overrides are normalized by the driver into the canonical runtime
    // wrapper. Prove that wrapper in the actual entry module; a same-named
    // function in an imported module is not the callable the adapters receive.
    let selected_name = jet::Codegen::ENTRY_FN;
    let selected = bundle.modules.get(bundle.entry).and_then(|module| {
        module.items.iter().find_map(|item| match item {
            jet::AST::Item::Func(function) if function.name == selected_name => {
                Some((bundle.entry, function))
            }
            _ => None,
        })
    });
    let entry_detail = match selected.as_ref() {
        Some((module, function)) if entry_fn.is_some_and(|name| name != selected_name) => format!(
            "requested `{}` resolved through selected `{selected_name}` in {} ({})",
            entry_fn.unwrap_or_default(),
            bundle.modules[*module].display,
            function.span.start,
        ),
        Some((module, function)) => format!(
            "selected `{selected_name}` in {} ({})",
            bundle.modules[*module].display,
            function.span.start,
        ),
        None => format!("selected `{selected_name}` is absent from the checked module graph"),
    };
    push_proof_row(
        &mut rows,
        &mut diagnostics,
        "entry resolution",
        selected.is_some().then_some("proven").unwrap_or("compiler defect"),
        entry_detail,
        "E2389",
    );

    let import_edges = bundle
        .modules
        .iter()
        .map(|module| module.imports.len())
        .sum::<usize>();
    push_proof_row(
        &mut rows,
        &mut diagnostics,
        "module graph",
        (!bundle.modules.is_empty())
            .then_some("proven")
            .unwrap_or("compiler defect"),
        format!(
            "{} loaded module(s), {} resolved import edge(s); project root {}",
            bundle.modules.len(),
            import_edges,
            bundle.project_root.display(),
        ),
        "E2390",
    );

    let core = jet::Codegen::core_closure_proof(bundle, false);
    let used_core = if core.used_calls.is_empty() {
        "none".to_string()
    } else {
        core.used_calls.join(",")
    };
    let synthesized_core = if core.synthesized_calls.is_empty() {
        "none".to_string()
    } else {
        core.synthesized_calls.join(",")
    };
    push_proof_row(
        &mut rows,
        &mut diagnostics,
        "Core closure",
        "proven",
        format!(
            "used=[{used_core}] synthesized=[{synthesized_core}] routes=[{}] fingerprint={}",
            core.adapter_routes.join(","),
            core.fingerprint,
        ),
        "E2391",
    );

    let (tier_status, tier_detail) = if target == Some(jet::Syntax::BUILD_TARGET_WEB) {
        let misses = jet::Codegen::validate_web_tir_support(bundle, None);
        if misses.is_empty() {
            ("proven", "web=proven; structured web-TIR validator".to_string())
        } else {
            (
                "compiler defect",
                format!(
                    "web=compiler defect; {}",
                    misses
                        .iter()
                        .map(|miss| format!("{} at {}..{}", miss.func_name, miss.span.start, miss.span.end))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
        }
    } else if target.is_some() {
        (
            "toolchain unavailable",
            format!(
                "native target `{}` needs the build toolchain; check does not invoke rustc",
                target.unwrap_or_default()
            ),
        )
    } else {
        let misses = jet::Codegen::TIR::validate_tir_support(bundle);
        if misses.is_empty() {
            (
                "proven",
                "AOT=proven; JIT=proven; interpreter=shared checked TIR route".to_string(),
            )
        } else {
            (
                "compiler defect",
                format!(
                    "{}",
                    misses
                        .iter()
                        .map(|miss| format!(
                            "{} {}: {}",
                            miss.tier, miss.callable, miss.reason
                        ))
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
            )
        }
    };
    push_proof_row(
        &mut rows,
        &mut diagnostics,
        "tier lowering",
        tier_status,
        tier_detail,
        "E2392",
    );
    (rows, diagnostics)
}

fn push_proof_row(
    rows: &mut Vec<CheckProofRow>,
    diagnostics: &mut Vec<Diagnostic>,
    name: &'static str,
    status: &'static str,
    detail: String,
    diagnostic: &'static str,
) {
    let row = CheckProofRow {
        name,
        status,
        detail,
        diagnostic,
    };
    if !matches!(row.status, "proven" | "not applicable") {
        diagnostics.push(Diagnostic::from_row(
            row.diagnostic,
            &[("detail", row.detail.as_str())],
            None,
        ));
    }
    rows.push(row);
}

pub(crate) fn check_result_json(check: &CheckResult) -> String {
    let rows = check
        .proof_rows
        .iter()
        .map(|row| {
            format!(
                "{{\"name\":\"{}\",\"status\":\"{}\",\"detail\":\"{}\",\"diagnostic\":\"{}\"}}",
                json_escape(row.name),
                json_escape(row.status),
                json_escape(&row.detail),
                json_escape(row.diagnostic),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"status\":\"passed\",\"schema_version\":{},\"scope\":\"{}\",\"elapsed_ms\":{},\"provenance\":{{\"source\":\"{}\",\"profile\":\"{}\",\"front_end\":\"{}\",\"programmable_build\":\"{}\",\"diagnostics\":{}}},\"proof\":[{}]}}",
        CHECK_RESULT_SCHEMA_VERSION,
        scope_name(check.scope),
        check.elapsed_ms,
        json_escape(&check.source),
        json_escape(&check.profile),
        json_escape(check.front_end),
        json_escape(check.programmable_build),
        check.diagnostics,
        rows,
    )
}

pub(crate) fn with_check_json(mut document: String, check: &CheckResult) -> String {
    if let Some(index) = document.rfind('}') {
        document.insert_str(index, &format!(",\"check\":{}", check_result_json(check)));
    }
    document
}

pub(crate) fn check_result_text(check: &CheckResult) -> String {
    let mut text = format!(
        "check: passed (source={}, profile={}, scope={}, schema_version={}, elapsed_ms={}, front_end={}, programmable_build={}, diagnostics={})\n",
        check.source,
        check.profile,
        scope_name(check.scope),
        CHECK_RESULT_SCHEMA_VERSION,
        check.elapsed_ms,
        check.front_end,
        check.programmable_build,
        check.diagnostics,
    );
    for row in &check.proof_rows {
        let _ = writeln!(
            text,
            "proof: {} [{}] {} (diagnostic={})",
            row.name, row.status, row.detail, row.diagnostic,
        );
    }
    text
}

pub(crate) fn render_check_failure(
    path: &Path,
    diagnostics: &[Diagnostic],
    json: bool,
    color: bool,
) -> ! {
    let entry = path.display().to_string();
    let source = fs::read_to_string(path).unwrap_or_default();
    if json {
        let machine_file = crate::machine_report_path_for_process(&entry);
        print!(
            "{}",
            jet::render_all_json(&machine_file, &source, diagnostics)
        );
    } else {
        eprint!(
            "{}",
            jet::render_all_colored(&entry, &source, diagnostics, color)
        );
    }
    exit(jet::ExitCodes::USER_ERROR);
}

/// `jet inspect digest` — emit the one-file language surface used by agents.
/// Every registry-shaped section is projected from the same typed rows used by
/// the compiler's introspection and report surfaces.
pub(crate) fn run_digest(args: &[String], json: bool) {
    let (topic, list_topics) = parse_digest_args(args);
    if !list_topics && topic.is_none() {
        emit_digest(&llm_digest(), json);
        return;
    }
    let slices = digest_slices();
    if list_topics {
        if topic.is_some() {
            crate::cli_error!(
                @fix "E2104",
                "`--list-topics` cannot be combined with `--topic`",
                "use `jet inspect digest --list-topics` or `jet inspect digest --topic <name>`"
            );
            exit(jet::ExitCodes::USAGE);
        }
        if json {
            let topics = slices
                .iter()
                .map(|slice| format!("\"{}\"", json_escape(&slice.topic)))
                .collect::<Vec<_>>()
                .join(",");
            println!(
                "{}",
                render_status_json(
                    "ok",
                    true,
                    "inspect.digest",
                    &format!(",\"digest\":{{\"topics\":[{topics}]}}"),
                )
            );
        } else {
            for slice in &slices {
                println!("{}", slice.topic);
            }
        }
        return;
    }

    let digest = match topic {
        Some(topic) => match slices
            .iter()
            .find(|slice| slice.topic.as_str() == topic.as_str())
        {
            Some(slice) => slice.bytes.clone(),
            None => {
                let closest = slices
                    .iter()
                    .min_by_key(|slice| {
                        (
                            jet::Syntax::edit_distance(&topic, &slice.topic),
                            &slice.topic,
                        )
                    })
                    .map(|slice| slice.topic.as_str())
                    .unwrap_or("first-program");
                crate::cli_error!(
                    @fix "E2104",
                    format!("unknown digest topic `{topic}`"),
                    format!("use `--topic {closest}`, or list topics with `jet inspect digest --list-topics`")
                );
                exit(jet::ExitCodes::USAGE);
            }
        },
        None => llm_digest(),
    };
    emit_digest(&digest, json);
}

/// `jet inspect output <file.jet> [<address>]` — report one sema-selected
/// Output and its checked callable facts.
pub(crate) fn run_output(args: &[String], json: bool) {
    let positionals = args
        .iter()
        .filter(|arg| !arg.starts_with('-'))
        .collect::<Vec<_>>();
    let Some(file) = positionals.first() else {
        crate::cli_error!(
            @fix "E2104",
            "`jet inspect output` needs an entry file",
            "run `jet inspect output examples/features/tooling/output_callable.jet`"
        );
        exit(jet::ExitCodes::USAGE);
    };
    let requested = positionals.get(1).map(|value| value.as_str());
    let path = Path::new(file.as_str());
    let projection = match requested {
        Some(address) => {
            check_projection_for_output_effects(path, address, "dev", &BTreeMap::new())
        }
        None => check_projection_for_run(path, "dev", &BTreeMap::new()),
    }
    .unwrap_or_else(|diagnostics| render_check_failure(path, &diagnostics, json, false));

    let selected = projection
        .bundle
        .modules
        .iter()
        .flat_map(|module| module.items.iter())
        .find_map(|item| {
            let jet::AST::Item::Const(value) = item else {
                return None;
            };
            let output = value.resolved_output.as_ref()?;
            output.selected.then_some((value.name.as_str(), output))
        });
    let Some((binding, output)) = selected else {
        crate::cli_error!(
            @fix "E2104",
            "the checked source has no selected Output",
            "declare one Executable Output or select one by address"
        );
        exit(jet::ExitCodes::USER_ERROR);
    };
    let module = &projection.bundle.modules[output.module];
    let callable_identity = format!("{}::{}", module.alias, output.semantic_name);
    let failure = output.failure_contract();
    let failure_contract = failure.effective_type().name();
    let failure_source = failure.source();
    let mut effects = output.effects.clone();
    effects.sort();
    let required_effects = effects.iter().cloned().collect::<jet::Sema::EffectSet>();
    let manifest = path
        .parent()
        .and_then(jet::Loader::find_manifest_root)
        .and_then(|root| jet::Loader::manifest_path(&root))
        .and_then(|manifest_path| fs::read_to_string(&manifest_path).ok())
        .and_then(|raw| jet::Package::PackageFacts::parse(&raw, "package.jet").ok());
    let authority =
        jet::EffectBudget::project_application_effects(&required_effects, manifest.as_ref());

    if json {
        let effect_values = effects
            .iter()
            .map(|effect| format!("\"{}\"", json_escape(effect)))
            .collect::<Vec<_>>()
            .join(",");
        let granted_values = authority
            .granted_effects
            .iter()
            .map(|effect| format!("\"{}\"", json_escape(effect)))
            .collect::<Vec<_>>()
            .join(",");
        let denied_values = authority
            .denied_effects
            .iter()
            .map(|effect| format!("\"{}\"", json_escape(effect)))
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{}",
            render_status_json(
                "ok",
                true,
                "inspect.output",
                &format!(
                    ",\"output\":{{\"binding\":\"{}\",\"kind\":\"{}\",\"name\":\"{}\",\"entry\":\"{}\",\"source_path\":\"{}\",\"callable_identity\":\"{}\",\"failure_contract\":\"{}\",\"failure_source\":\"{}\",\"effects\":[{}],\"required_effects\":[{}],\"granted_effects\":[{}],\"denied_effects\":[{}],\"authority\":\"{}\",\"selection_reason\":\"{}\"}}",
                    json_escape(binding),
                    json_escape(output.kind.as_str()),
                    json_escape(&output.output_name),
                    json_escape(&output.source_name),
                    json_escape(&output.source_path),
                    json_escape(&callable_identity),
                    json_escape(&failure_contract),
                    json_escape(&failure_source),
                    effect_values,
                    effect_values,
                    granted_values,
                    denied_values,
                    json_escape(&authority.authority),
                    json_escape(&output.selection_reason),
                ),
            )
        );
    } else {
        println!(
            "selected: {} \"{}\"",
            output.kind.as_str(),
            output.output_name
        );
        println!("entry: {}", output.source_name);
        println!("source path: {}", output.source_path);
        println!("callable identity: {callable_identity}");
        println!(
            "failure contract: {} ({})",
            failure_contract, failure_source
        );
        println!(
            "required effects: {}",
            if effects.is_empty() {
                "none".to_string()
            } else {
                effects.join(", ")
            }
        );
        println!(
            "granted effects: {}",
            if authority.granted_effects.is_empty() {
                "none".to_string()
            } else {
                authority
                    .granted_effects
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        );
        println!(
            "denied effects: {}",
            if authority.denied_effects.is_empty() {
                "none".to_string()
            } else {
                authority
                    .denied_effects
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        );
        println!("authority: {}", authority.authority);
        println!("selection reason: {}", output.selection_reason);
    }
}

/// `jet inspect env [<env.jet|config.jet>]` — evaluate a config surface and
/// project the typed `$NAME` reads without exposing their values.
pub(crate) fn run_env(args: &[String], json: bool) {
    let file = entry_file(args).unwrap_or_else(|| {
        if Path::new(jet::Syntax::ENV_FILE).is_file() {
            jet::Syntax::ENV_FILE.to_string()
        } else {
            jet::Syntax::CONFIG_FILE.to_string()
        }
    });
    let source = match fs::read_to_string(&file) {
        Ok(source) => source,
        Err(error) => {
            crate::cli_error!(
                @fix "E2105",
                format!("can't read config surface `{file}`: {error}"),
                "pass a readable `env.jet` or `config.jet` file"
            );
            exit(jet::ExitCodes::USER_ERROR);
        }
    };
    let path = Path::new(&file);
    let base_dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let plan = match jet_env_model::ModuleEval::evaluate_env(&source, base_dir) {
        Ok(plan) => plan,
        Err(diagnostic) => {
            eprint!(
                "{}",
                jet::render_diagnostics(&file, &source, std::slice::from_ref(&diagnostic))
            );
            exit(jet::ExitCodes::USER_ERROR);
        }
    };
    if json {
        let reads = plan
            .environment_reads
            .iter()
            .map(|read| {
                format!(
                    "{{\"name\":\"{}\",\"type\":\"{}\"}}",
                    json_escape(&read.name),
                    json_escape(&read.ty)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let payload = format!(
            "{{\"file\":\"{}\",\"reads\":[{}]}}",
            json_escape(&file),
            reads
        );
        println!(
            "{}",
            render_status_json("ok", true, "inspect.env", &format!(",\"env\":{payload}"),)
        );
    } else {
        println!("environment");
        println!("file: {file}");
        if plan.environment_reads.is_empty() {
            println!("reads: none");
        } else {
            println!("reads:");
            for read in &plan.environment_reads {
                println!("  {}: {}", read.name, read.ty);
            }
        }
    }
}

fn emit_digest(digest: &str, json: bool) {
    if json {
        println!(
            "{}",
            render_status_json(
                "ok",
                true,
                "inspect.digest",
                &format!(",\"digest\":{{\"value\":\"{}\"}}", json_escape(digest)),
            )
        );
    } else {
        print!("{digest}");
    }
}

struct DigestSlice {
    topic: String,
    bytes: String,
}

fn parse_digest_args(args: &[String]) -> (Option<String>, bool) {
    let mut topic = None;
    let mut list_topics = false;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == jet::CLI::MACHINE_OUTPUT_FLAG {
            index += 1;
            continue;
        }
        if arg == "--list-topics" {
            list_topics = true;
        } else if let Some(value) = arg.strip_prefix("--topic=") {
            topic = Some(value.to_string());
        } else if arg == "--topic" {
            let Some(value) = args.get(index + 1).filter(|value| !value.starts_with('-')) else {
                crate::cli_error!(
                    @fix "E2104",
                    "`--topic` needs a value",
                    "write `--topic <name>`; use `--list-topics` to discover names"
                );
                exit(jet::ExitCodes::USAGE);
            };
            topic = Some(value.clone());
            index += 1;
        }
        index += 1;
    }
    (topic, list_topics)
}

/// Split the one rendered digest into byte-exact topic slices. The full digest
/// remains the only source: slices are ranges of its output, so concatenating
/// the listed slices reproduces the whole file exactly.
fn digest_slices() -> Vec<DigestSlice> {
    let digest = llm_digest();
    let headings = [
        ("first-program", "## First program"),
        ("core.source-rules", "## Core source rules"),
        ("canonical", "## Canonical compiling example"),
        ("idioms", "## Canonical idiom suites"),
        ("syntax.keywords", "## Keywords"),
        ("syntax.types", "## Built-in type names"),
        ("syntax.reserved", "## Reserved first-party names"),
        ("markers", "## Markers"),
        ("core", "## Core module index"),
        ("diagnostics", "## Diagnostics"),
    ];
    let starts = headings
        .iter()
        .map(|(_, heading)| digest.find(heading).expect("digest heading disappeared"))
        .collect::<Vec<_>>();
    let mut slices = Vec::new();
    for (index, (topic, _)) in headings.iter().enumerate() {
        let start = if index == 0 { 0 } else { starts[index] };
        let end = starts.get(index + 1).copied().unwrap_or(digest.len());
        if *topic != "core" {
            slices.push(DigestSlice {
                topic: (*topic).to_string(),
                bytes: digest[start..end].to_string(),
            });
            continue;
        }

        let header_end = digest[start..end]
            .find("module\titems\n")
            .map(|offset| start + offset + "module\titems\n".len())
            .expect("core digest header disappeared");
        let rows = digest[header_end..end]
            .split_inclusive('\n')
            .scan(header_end, |offset, line| {
                let start = *offset;
                *offset += line.len();
                Some((start, *offset, line.starts_with("core.")))
            })
            .filter_map(|(start, end, is_module)| is_module.then_some((start, end)))
            .collect::<Vec<_>>();
        for (row_index, (row_start, row_end)) in rows.iter().enumerate() {
            let topic = digest[*row_start..*row_end]
                .split_once('\t')
                .map(|(module, _)| module)
                .expect("core digest row lost module name");
            let slice_start = if row_index == 0 { start } else { *row_start };
            let slice_end = rows
                .get(row_index + 1)
                .map(|(next_start, _)| *next_start)
                .unwrap_or(end);
            slices.push(DigestSlice {
                topic: topic.to_string(),
                bytes: digest[slice_start..slice_end].to_string(),
            });
        }
    }
    slices
}

fn llm_digest() -> String {
    let marker_text = digest_marker_text();
    let diagnostic_text = digest_diagnostic_text();
    let core_text = digest_core_module_text();
    let canonical = include_str!("../examples/canon.jet").trim();
    let idiom_suites_body = [
        "Each idiom has one executable, golden-backed source of truth under `examples/suites/`.",
        "",
        "- Dispatch: `examples/suites/dispatch.jet` — ordered dispatch tables and grouped aliases.",
        "- Failure: `examples/suites/failure.jet` — implicit failure flow, typed expert contracts, and one conversion rail.",
        "- Finite state: `examples/suites/finite_state.jet` — enums, variant groups, tags, and typestate transitions.",
        "- Ownership: `examples/suites/ownership.jet` — reused views, explicit `~` boundaries, and cost visibility.",
        "- Wire output: `examples/suites/wire_output.jet` — canonical JSON writer bytes and a `#Codable` round trip.",
    ]
    .join("\n");

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
        "Functions: `fn name(parameter: Type) Return -[]> { ... }`; expression bodies use `-> expression`.",
        "Visibility: declarations are private by default; prefix an item with `pub` for package use.",
        "Types: `Int`, `Float`, `Bool`, `String`, `Char`; lists use `[T]`; optional values use `?T`; failures use `T !E`.",
        "Errors: handle `?T` or `T !E` with `?? fallback`, `?`, or a pattern test. Use `Ok(value)`, `Err(error)`, `Val(value)`, and `None`.",
        "Control: `if condition { ... } else { ... }`; collecting loops use `loop name in source { ... }`; exit with `break` and advance with `next`.",
        "Construction: use `Type{ field: value }`; list literals use `[T]{ value1, value2 }`.",
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
        "Use a module alias, then call an indexed item: `use core.term as term`; `term.print(\"hi\")`.\n\n```text\n{core_text}\n```"
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
        digest_section("Canonical idiom suites", &idiom_suites_body),
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
                .map_or(String::new(), |value| format!("{{{value}}}"));
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
        fields.push(format!("@retired: \"{}\"", digest_quote(replacement)));
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
        crate::cli_error!(@fix "E2104", "`jet inspect guarantees` needs an entry file", "jet inspect guarantees run.jet");
        exit(jet::ExitCodes::USAGE);
    };
    let checked = check_projection_with_options(Path::new(&file), gates, profile, &BTreeMap::new())
        .unwrap_or_else(|diagnostics| {
            render_check_failure(Path::new(&file), &diagnostics, json, color);
        });
    let bundle = &checked.bundle;
    let package = checked.index.package_facts();

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
    let report =
        jet::Driver::guarantee_report(package, dependencies, unsafe_gates, profile, freestanding);
    if json {
        render_json(&report, &checked.check);
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
    let manifest_path =
        jet::Loader::manifest_path(&root).expect("manifest root has a Package file");
    let manifest = match jet::Manifest::load(&root) {
        Some(Ok(manifest)) => manifest,
        Some(Err(diagnostic)) => {
            eprint!(
                "{}",
                jet::render_diagnostics(&manifest_path.display().to_string(), "", &[diagnostic],)
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
    reports.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.version.cmp(&right.version))
    });
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
        render_effect_provenance_text(report);
    }
}

fn render_provenance_field(label: &str, field: &jet::Lock::ProvenanceField) {
    println!("  {label:<12} {} ({})", field.value, field.status.label());
}

fn render_effect_provenance_text(report: &jet::Lock::DependencyProvenanceReport) {
    println!("  effect roles");
    println!(
        "    required effects: {}",
        effect_names(&report.required_effects)
    );
    println!(
        "    granted effects: {}",
        effect_names(&report.granted_effects)
    );
    println!(
        "    denied effects: {}",
        effect_names(&report.denied_effects)
    );
    println!("    authority: {}", report.authority);
}

fn effect_names(effects: &[String]) -> String {
    if effects.is_empty() {
        "none".to_string()
    } else {
        effects.join(", ")
    }
}

fn json_strings(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| format!("\"{}\"", json_escape(value)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn render_provenance_json(
    requirement: jet::Package::ProvenanceRequirement,
    reports: &[jet::Lock::DependencyProvenanceReport],
) {
    let packages = reports
        .iter()
        .map(render_provenance_json_package)
        .collect::<Vec<_>>()
        .join(",");
    let payload = format!(
        "{{\"require\":\"{}\",\"packages\":[{}]}}",
        json_escape(requirement.label()),
        packages
    );
    println!(
        "{}",
        render_status_json(
            "ok",
            true,
            "inspect.provenance",
            &format!(",\"provenance\":{payload}"),
        )
    );
}

fn render_provenance_json_package(report: &jet::Lock::DependencyProvenanceReport) -> String {
    let integrity_evidence = matches!(
        report.integrity.status,
        jet::Lock::ProvenanceStatus::Enforced
    )
    .then_some("E1204");
    let mut fields = vec![
        format!("\"name\":\"{}\"", json_escape(&report.name)),
        format!("\"version\":\"{}\"", json_escape(&report.version)),
        render_provenance_json_field("integrity", &report.integrity, integrity_evidence),
        render_provenance_json_field("transparency", &report.transparency, None),
        render_provenance_json_field("publisher", &report.publisher, None),
        render_provenance_json_field("build", &report.build, None),
    ];
    fields.push(format!(
        "\"required_effects\":{},\"granted_effects\":{},\"denied_effects\":{},\"authority\":\"{}\"",
        json_strings(&report.required_effects),
        json_strings(&report.granted_effects),
        json_strings(&report.denied_effects),
        json_escape(&report.authority),
    ));
    format!("{{{}}}", fields.join(","))
}

fn render_provenance_json_field(
    key: &str,
    field: &jet::Lock::ProvenanceField,
    evidence: Option<&str>,
) -> String {
    let mut fields = vec![
        format!("\"value\":\"{}\"", json_escape(&field.value)),
        format!("\"status\":\"{}\"", field.status.label()),
    ];
    if let Some(evidence) = evidence {
        fields.push(format!("\"evidence\":\"{}\"", json_escape(evidence)));
    }
    format!("\"{key}\":{{{}}}", fields.join(","))
}

fn entry_file(args: &[String]) -> Option<String> {
    let mut skip_value = false;
    for argument in args {
        if skip_value {
            skip_value = false;
            continue;
        }
        if matches!(
            argument.as_str(),
            "--profile" | "--target" | "--scope" | "--kind"
        ) {
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

fn render_human_policy(policy: &jet::Driver::GuaranteePolicyReport) {
    if policy.effects.is_empty() {
        println!("policy.effects: absent");
    } else {
        println!("policy.effects:");
        for (path, ceiling) in &policy.effects {
            println!(
                "  {path}: -[{}]>",
                ceiling.iter().cloned().collect::<Vec<_>>().join(",")
            );
        }
    }
    match &policy.unsafe_paths {
        None => println!("policy.unsafe: absent"),
        Some(paths) if paths.is_empty() => println!("policy.unsafe: .Deny"),
        Some(paths) => println!("policy.unsafe: .Paths({paths:?})"),
    }
    match policy.expert {
        None => println!("policy.expert: absent"),
        Some(false) => println!("policy.expert: .Deny"),
        Some(true) => println!("policy.expert: .Allow"),
    }
    match &policy.deps {
        None => println!("policy.deps: absent"),
        Some(deps) => println!("policy.deps: .List({deps:?})"),
    }
    match &policy.lints_deny {
        None => println!("policy.lints.deny: absent"),
        Some(classes) => println!("policy.lints.deny: {classes:?}"),
    }
}

fn render_policy_json(policy: &jet::Driver::GuaranteePolicyReport) -> String {
    let effects = policy
        .effects
        .iter()
        .map(|(path, ceiling)| {
            let ceiling = ceiling.iter().cloned().collect::<Vec<_>>();
            format!(
                "{{\"path\":\"{}\",\"ceiling\":{}}}",
                json_escape(path),
                json_strings(&ceiling)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let unsafe_policy = match &policy.unsafe_paths {
        None => "null".to_string(),
        Some(paths) if paths.is_empty() => "{\"mode\":\"deny\"}".to_string(),
        Some(paths) => format!("{{\"mode\":\"paths\",\"paths\":{}}}", json_strings(paths)),
    };
    let expert = policy
        .expert
        .map(|allowed| allowed.to_string())
        .unwrap_or_else(|| "null".to_string());
    let deps = policy
        .deps
        .as_deref()
        .map(json_strings)
        .unwrap_or_else(|| "null".to_string());
    let lints = policy
        .lints_deny
        .as_deref()
        .map(|classes| format!("{{\"deny\":{}}}", json_strings(classes)))
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"effects\":[{}],\"unsafe\":{},\"expert\":{},\"deps\":{},\"lints\":{}}}",
        effects, unsafe_policy, expert, deps, lints
    )
}

fn render_human(report: &jet::Driver::GuaranteeReport) {
    println!("guarantees");
    println!("profile: {}", report.profile);
    println!(
        "scope: {}",
        if report.package {
            "package"
        } else {
            "single-file"
        }
    );
    if report.freestanding {
        println!("target: freestanding");
    }
    render_human_policy(&report.policy);
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

fn render_json(report: &jet::Driver::GuaranteeReport, check: &CheckResult) {
    let policy = render_policy_json(&report.policy);
    let mut document = format!(
        "{{\"schema_version\":1,\"profile\":\"{}\",\"package\":{},\"freestanding\":{},\"policy\":{},\"components\":[",
        json_escape(&report.profile),
        report.package,
        report.freestanding,
        policy,
    );
    for (index, component) in report.components.iter().enumerate() {
        if index > 0 {
            document.push(',');
        }
        write!(
            document,
            "{{\"component\":\"{}\",\"guarantee\":\"{}\",\"evidence\":\"{}\"}}",
            json_escape(&component.name),
            component.status.label(),
            json_escape(&component.evidence),
        )
        .expect("write guarantee JSON");
    }
    document.push_str("],\"notes\":[");
    for (index, note) in report.notes.iter().enumerate() {
        if index > 0 {
            document.push(',');
        }
        write!(document, "\"{}\"", json_escape(note)).expect("write guarantee JSON");
    }
    document.push_str("]}");
    let document = with_check_json(document, check);
    println!(
        "{}",
        render_status_json(
            "ok",
            true,
            "inspect.guarantees",
            &format!(",\"guarantees\":{document}"),
        )
    );
}
