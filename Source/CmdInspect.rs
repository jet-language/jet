//! Read-only projections owned by `jet inspect`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::Instant;

use jet::Diagnostics::Diagnostic;
use jet_foundation::MIROptimization::Acceleration::{
    AccelerationGate, AccelerationGateInput, AccelerationGateStatus, AccelerationProof,
    AccelerationWorkloadFacts, D_ACCEL_CACHE_BLOCK_BYTES, D_ACCEL_REQUIRED_GAIN_DENOMINATOR,
    D_ACCEL_REQUIRED_GAIN_NUMERATOR, D_ACCEL_STATIC_FLOOR_ITEMS,
};
use jet_foundation::Policy::{AppliedRule, PolicyScope, RuleResolution, RuleStatus};
use jet_foundation::Registry;
use jet_foundation::Report::{
    render_status, render_status_with_reports, StatusEnvelope, StatusFields, StatusValue,
};
use jet_foundation::Shape::{
    ShapeDefault, ShapeDimensions, ShapeFact, ShapeFieldFact, ShapeFieldNames, ShapeLayoutKind,
    ShapeOrigin, ShapeProjectionKind, ShapeType,
};
use jet_foundation::JSON::json_escape;
use jet_foundation::MIR::{
    MirDecisionDisposition, MirDecisionIdentity, MirDecisionKind, MirDecisionLedger, MirDecisionRow,
};

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
const CHECK_RESULT_CONTRACT: &str = "jet.check/v1";

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
                &[(
                    "detail",
                    "project check returned no final runtime effect facts",
                )],
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
        let (diagnostics, bundle, facts) =
            jet::Driver::check_file_with_effect_facts_profile_and_settings(
                &entry,
                None,
                false,
                profile,
                setting_overrides,
            );
        (
            diagnostics,
            bundle,
            facts,
            "Driver::check_file_with_effect_facts_profile_and_settings",
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
            &facts,
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

pub(crate) fn missing_project_context_diagnostic(path: &Path) -> Option<Diagnostic> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if jet::Loader::find_package_root_checked(parent)
        .ok()
        .flatten()
        .is_some()
        || jet::Loader::package_facts_for_entry(path)
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
    let source_for_parse = jet::Package::mask_inline_package_source(&source).ok()?.0;
    let (tokens, lex_diagnostics) = jet::Lexer::lex(&source_for_parse);
    if !lex_diagnostics.is_empty() {
        return None;
    }
    let parsed_span = jet::Parser::parse_with_source(&tokens, &source_for_parse)
        .ok()
        .and_then(|program| {
            program
                .imports
                .iter()
                .find_map(|import| match &import.kind {
                    jet::AST::ImportKind::Module(name, span)
                        if name.starts_with(jet::Syntax::PROJECT_IMPORT_PREFIX) =>
                    {
                        Some(*span)
                    }
                    jet::AST::ImportKind::Unqualified {
                        module_alias, span, ..
                    } if module_alias.starts_with(jet::Syntax::PROJECT_IMPORT_PREFIX) => {
                        Some(*span)
                    }
                    _ => None,
                })
        });
    let lexical_span = tokens.windows(4).find_map(|window| {
        match (
            &window[0].kind,
            &window[1].kind,
            &window[2].kind,
            &window[3].kind,
        ) {
            (
                jet::Lexer::TokKind::KwUse,
                jet::Lexer::TokKind::Ident(project),
                jet::Lexer::TokKind::Dot,
                jet::Lexer::TokKind::Ident(_),
            ) if project == jet::Syntax::PROJECT_IMPORT_ROOT => Some(jet::Diagnostics::Span::new(
                window[1].span.start,
                window[3].span.end,
            )),
            _ => None,
        }
    });
    let span = parsed_span.or(lexical_span)?;
    Some(Diagnostic::from_row(
        "E2393",
        &[("import", "use project.<module>")],
        Some(span),
    ))
}

fn explicit_file_proof_rows() -> Vec<CheckProofRow> {
    vec![
        CheckProofRow {
            output: "not-applicable".to_string(),
            name: "entry resolution",
            status: "not applicable",
            detail: "explicit-file checks resolve only the named source file".to_string(),
            diagnostic: "E2389",
        },
        CheckProofRow {
            output: "not-applicable".to_string(),
            name: "module graph",
            status: "not applicable",
            detail: "explicit-file checks keep semantic scope and do not promise a project graph"
                .to_string(),
            diagnostic: "E2390",
        },
        CheckProofRow {
            output: "not-applicable".to_string(),
            name: "Core closure",
            status: "not applicable",
            detail:
                "explicit-file checks keep semantic scope and do not promise project Core closure"
                    .to_string(),
            diagnostic: "E2391",
        },
        CheckProofRow {
            output: "not-applicable".to_string(),
            name: "tier lowering",
            status: "not applicable",
            detail:
                "explicit-file checks keep semantic scope and do not promise project tier lowering"
                    .to_string(),
            diagnostic: "E2392",
        },
    ]
}

fn is_project_runnable_output(kind: jet::AST::OutputKind) -> bool {
    matches!(
        kind,
        jet::AST::OutputKind::Executable | jet::AST::OutputKind::Service
    )
}

fn normalize_output_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn source_line(source: &str, offset: usize) -> usize {
    let end = offset.min(source.len());
    1 + source
        .get(..end)
        .unwrap_or_default()
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
}

fn project_output_specs(
    bundle: &jet::AST::ProgramBundle,
    package_facts: Option<&jet::Package::PackageFacts>,
) -> (Vec<ProjectOutputSpec>, Vec<Diagnostic>) {
    let mut specs = std::collections::BTreeMap::<String, ProjectOutputSpec>::new();
    let mut diagnostics = Vec::new();

    if let Some(package_facts) = package_facts {
        for (address, output) in &package_facts.outputs {
            if !output.kind.is_runnable() {
                continue;
            }
            let path = match package_facts.entry_path(&bundle.project_root, output) {
                Ok(path) => path.map(|path| normalize_output_path(&path)),
                Err(error) => {
                    diagnostics.push(Diagnostic::from_row(
                        "E2389",
                        &[(
                            "detail",
                            &format!("output `{address}` entry resolution failed: {error}"),
                        )],
                        None,
                    ));
                    None
                }
            };
            specs.insert(
                address.clone(),
                ProjectOutputSpec {
                    address: address.clone(),
                    name: output.name.clone(),
                    path,
                },
            );
        }
    }

    for module in &bundle.modules {
        for item in &module.items {
            let jet::AST::Item::Const(constant) = item else {
                continue;
            };
            let Some(output) = constant
                .resolved_output
                .as_ref()
                .filter(|output| is_project_runnable_output(output.kind))
            else {
                continue;
            };
            let module_path = normalize_output_path(&module.path);
            specs
                .entry(output.address.clone())
                .and_modify(|spec| {
                    if spec.path.is_none() {
                        spec.path = Some(module_path.clone());
                    }
                    if spec.name.is_empty() {
                        spec.name = output.output_name.clone();
                    }
                })
                .or_insert_with(|| ProjectOutputSpec {
                    address: output.address.clone(),
                    name: output.output_name.clone(),
                    path: Some(module_path.clone()),
                });
        }
    }

    if specs.is_empty() {
        let entry_module = bundle.modules.get(bundle.entry);
        let entry_path = entry_module.map(|module| normalize_output_path(&module.path));
        let has_run = entry_module.is_some_and(|module| {
            module.items.iter().any(|item| {
                matches!(
                    item,
                    jet::AST::Item::Func(function)
                        if function.name == "run"
                )
            })
        });
        specs.insert(
            if has_run {
                "run".to_string()
            } else {
                "default".to_string()
            },
            ProjectOutputSpec {
                address: if has_run {
                    "run".to_string()
                } else {
                    "default".to_string()
                },
                name: "run".to_string(),
                path: entry_path,
            },
        );
    }

    (specs.into_values().collect(), diagnostics)
}

#[derive(Clone, Debug)]
struct ProjectBundleProof {
    module_status: &'static str,
    module_detail: String,
    core_status: &'static str,
    core_detail: String,
    tier_status: &'static str,
    tier_detail: String,
    mir: jet_foundation::MIR::MirProgramIdentity,
}

impl ProjectBundleProof {
    fn from_bundle(
        bundle: &jet::AST::ProgramBundle,
        _facts: &jet::Sema::SemIndexEffectFacts,
        target: Option<&str>,
        profile: &str,
    ) -> Self {
        let import_edges = bundle
            .modules
            .iter()
            .map(|module| module.imports.len())
            .sum::<usize>();
        let module_status = (!bundle.modules.is_empty())
            .then_some("proven")
            .unwrap_or("compiler defect");
        let module_detail = format!(
            "{} loaded module(s), {} resolved import edge(s); project root {}",
            bundle.modules.len(),
            import_edges,
            bundle.project_root.display(),
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
        let core_detail = format!(
            "used=[{used_core}] synthesized=[{synthesized_core}] routes=[{}] fingerprint={}",
            core.adapter_routes.join(","),
            core.fingerprint,
        );

        let artifact_target = if target == Some(jet::Syntax::BUILD_TARGET_WEB) {
            jet_foundation::MIR::MirArtifactTarget::Web
        } else {
            jet_foundation::MIR::MirArtifactTarget::Cranelift
        };
        let artifact_kind = if artifact_target == jet_foundation::MIR::MirArtifactTarget::Web {
            jet_foundation::MIR::MirArtifactKind::WebApplication
        } else {
            jet_foundation::MIR::MirArtifactKind::NativeExecutable
        };
        let (mir, artifact) = jet::lower_checked_semantic_mir_program_for(
            bundle,
            jet_foundation::MIR::MirArtifactRequest::new(
                artifact_target,
                artifact_kind,
                jet_foundation::MIR::MirArtifactBuildMode::Dev,
            ),
        );
        let mir_identity = mir
            .artifact_identity(artifact)
            .unwrap_or_else(|error| {
                jet_foundation::ice!(None, "canonical MIR identity failed: {error}")
            })
            .program_identity;
        let mir_detail = format!("mir-v1={}", mir_identity.canonical_json());
        let (tier_status, mut tier_detail) = if target == Some(jet::Syntax::BUILD_TARGET_WEB) {
            let web_target = jet::Codegen::MIRWeb::MirWebTarget {
                layout: if bundle.build_facts.target_triple.is_empty() {
                    jet_foundation::Layout::TargetLayout::host()
                } else {
                    jet_foundation::Layout::TargetLayout::from_triple(
                        bundle.build_facts.target_triple.clone(),
                    )
                },
                assets: jet::Codegen::MIRWeb::MirWebAssets::with_default_shell(),
                semantic_digest: jet_foundation::MIR::mir_program_digest(&mir),
                artifact,
                release_devtools_policy: jet::Driver::release_devtools_policy_for_bundle(
                    bundle, profile,
                ),
            };
            match jet::Codegen::MIRWeb::emit_web(&mir, &web_target) {
                Ok(_) => ("proven", "web=proven; checked MIR Web artifact".to_string()),
                Err(error) => ("compiler defect", format!("web=compiler defect; {error}")),
            }
        } else {
            let detail_prefix = target
                .map(|target| format!("target={target}; "))
                .unwrap_or_default();
            let jit_detail = jet_jit::resident_jit_safe_program_detail(&mir);
            if jit_detail.is_empty() {
                (
                    "proven",
                    format!(
                        "{detail_prefix}AOT=proven; JIT=proven; interpreter=shared checked MIR route"
                    ),
                )
            } else {
                (
                    "compiler defect",
                    format!("{detail_prefix}JIT=compiler defect; {jit_detail}"),
                )
            }
        };
        tier_detail.push(' ');
        tier_detail.push_str(&mir_detail);
        Self {
            module_status,
            module_detail,
            core_status: "proven",
            core_detail: format!("{core_detail} {mir_detail}"),
            tier_status,
            tier_detail,
            mir: mir_identity,
        }
    }
}

fn project_output_location(
    bundle: &jet::AST::ProgramBundle,
    output: &jet::AST::ResolvedOutput,
) -> Option<String> {
    let module = bundle.modules.get(output.module)?;
    Some(format!(
        "{}:{}",
        module.display,
        source_line(&module.source, output.definition.start)
    ))
}

fn legacy_entry_location(bundle: &jet::AST::ProgramBundle) -> Option<String> {
    let module = bundle.modules.get(bundle.entry)?;
    let function = module.items.iter().find_map(|item| match item {
        jet::AST::Item::Func(function) if function.name == "run" => Some(function),
        _ => None,
    })?;
    Some(format!(
        "{}:{}",
        module.display,
        source_line(&module.source, function.span.start)
    ))
}

fn output_proof_rows(
    bundle: &jet::AST::ProgramBundle,
    spec: &ProjectOutputSpec,
    entry_fn: Option<&str>,
    shared: &ProjectBundleProof,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<CheckProofRow> {
    let resolved = bundle
        .modules
        .iter()
        .flat_map(|module| &module.items)
        .find_map(|item| {
            let jet::AST::Item::Const(constant) = item else {
                return None;
            };
            constant.resolved_output.as_ref().filter(|output| {
                output.address == spec.address && is_project_runnable_output(output.kind)
            })
        });
    let location = resolved
        .and_then(|output| project_output_location(bundle, output))
        .or_else(|| {
            if matches!(spec.address.as_str(), "run" | "default") {
                legacy_entry_location(bundle)
            } else {
                None
            }
        });
    let output_name = if spec.name.is_empty() {
        spec.address.as_str()
    } else {
        spec.name.as_str()
    };
    let output_label = format!("{} `{}`", spec.address, output_name);
    let entry_detail = match location.as_deref() {
        Some(location) => {
            let requested = entry_fn
                .filter(|name| *name != "run")
                .map(|name| format!("; requested `{name}`"))
                .unwrap_or_default();
            format!("{output_label} resolved at {location}{requested}")
        }
        None => format!(
            "{output_label} is absent from the checked module graph ({}:1)",
            spec.path
                .as_deref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<unknown>".to_string())
        ),
    };
    let mut rows = Vec::new();
    let Some(location) = location else {
        for (name, diagnostic, detail) in [
            ("entry resolution", "E2389", entry_detail.clone()),
            (
                "module graph",
                "E2390",
                format!("{output_label} has no resolved source location"),
            ),
            (
                "Core closure",
                "E2391",
                format!("{output_label} has no resolved source location"),
            ),
            (
                "tier lowering",
                "E2392",
                format!("{output_label} has no resolved source location"),
            ),
        ] {
            push_proof_row(
                &mut rows,
                diagnostics,
                &spec.address,
                name,
                "compiler defect",
                detail,
                diagnostic,
            );
        }
        return rows;
    };

    push_proof_row(
        &mut rows,
        diagnostics,
        &spec.address,
        "entry resolution",
        "proven",
        entry_detail,
        "E2389",
    );
    push_proof_row(
        &mut rows,
        diagnostics,
        &spec.address,
        "module graph",
        shared.module_status,
        format!(
            "{}; entry={location}; mir_identity_digest={}",
            shared.module_detail,
            shared.mir.identity_digest()
        ),
        "E2390",
    );
    push_proof_row(
        &mut rows,
        diagnostics,
        &spec.address,
        "Core closure",
        shared.core_status,
        format!("{}; entry={location}", shared.core_detail),
        "E2391",
    );
    push_proof_row(
        &mut rows,
        diagnostics,
        &spec.address,
        "tier lowering",
        shared.tier_status,
        format!("{}; entry={location}", shared.tier_detail),
        "E2392",
    );
    rows
}

fn failed_output_rows(
    spec: &ProjectOutputSpec,
    reason: &str,
) -> (Vec<CheckProofRow>, Vec<Diagnostic>) {
    let location = spec
        .path
        .as_deref()
        .map(|path| format!("{}:1", path.display()))
        .unwrap_or_else(|| "<unknown>:1".to_string());
    let detail = format!(
        "output `{}` could not be checked at {location}: {reason}",
        spec.address
    );
    let mut rows = Vec::new();
    let mut diagnostics = Vec::new();
    for (name, diagnostic) in [
        ("entry resolution", "E2389"),
        ("module graph", "E2390"),
        ("Core closure", "E2391"),
        ("tier lowering", "E2392"),
    ] {
        push_proof_row(
            &mut rows,
            &mut diagnostics,
            &spec.address,
            name,
            "compiler defect",
            detail.clone(),
            diagnostic,
        );
    }
    (rows, diagnostics)
}

fn project_proof_rows(
    bundle: &jet::AST::ProgramBundle,
    facts: &jet::Sema::SemIndexEffectFacts,
    package_facts: Option<&jet::Package::PackageFacts>,
    entry_fn: Option<&str>,
    target: Option<&str>,
    profile: &str,
    setting_overrides: &std::collections::BTreeMap<String, String>,
) -> (Vec<CheckProofRow>, Vec<Diagnostic>) {
    let (specs, mut diagnostics) = project_output_specs(bundle, package_facts);
    let mut rows = Vec::new();

    for spec in &specs {
        let declared_output = package_facts
            .is_some_and(|facts| facts.outputs.contains_key(&spec.address))
            || bundle.modules.iter().any(|module| {
                module.items.iter().any(|item| {
                    let jet::AST::Item::Const(constant) = item else {
                        return false;
                    };
                    constant
                        .resolved_output
                        .as_ref()
                        .is_some_and(|output| output.address == spec.address)
                })
            });
        let checked_bundle_storage = declared_output.then(|| {
            spec.path.as_deref().and_then(|path| {
                let path = path.to_string_lossy().into_owned();
                let (check_diagnostics, checked_bundle, checked_facts) =
                    jet::Driver::check_file_with_effect_facts_for_output(
                        &path,
                        &spec.address,
                        profile,
                        setting_overrides,
                    );
                diagnostics.extend(check_diagnostics);
                checked_bundle.map(|bundle| (bundle, checked_facts))
            })
        });
        let checked = if declared_output {
            checked_bundle_storage
                .as_ref()
                .and_then(Option::as_ref)
                .map(|(bundle, facts)| (bundle, facts))
        } else {
            Some((bundle, facts))
        };

        let Some((checked_bundle, checked_facts)) = checked else {
            let reason = if spec.path.is_some() {
                "the output entry check returned no bundle"
            } else {
                "the manifest entry could not be resolved"
            };
            let (failed, failed_diagnostics) = failed_output_rows(spec, reason);
            rows.extend(failed);
            diagnostics.extend(failed_diagnostics);
            continue;
        };

        let shared =
            ProjectBundleProof::from_bundle(checked_bundle, checked_facts, target, profile);
        rows.extend(output_proof_rows(
            checked_bundle,
            spec,
            entry_fn,
            &shared,
            &mut diagnostics,
        ));
    }
    (rows, diagnostics)
}

fn push_proof_row(
    rows: &mut Vec<CheckProofRow>,
    diagnostics: &mut Vec<Diagnostic>,
    output: &str,
    name: &'static str,
    status: &'static str,
    detail: String,
    diagnostic: &'static str,
) {
    let row = CheckProofRow {
        output: output.to_string(),
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

/// Project one check result into the typed carrier used by status consumers.
pub(crate) fn check_result_value(check: &CheckResult) -> StatusValue {
    let rows = StatusValue::array(check.proof_rows.iter().map(|row| {
        StatusValue::object(
            StatusFields::new()
                .with("output", row.output.as_str())
                .with("name", row.name)
                .with("status", row.status)
                .with("detail", row.detail.as_str())
                .with("diagnostic", row.diagnostic),
        )
    }));
    StatusValue::object(
        StatusFields::new()
            .with("status", "passed")
            .with("contract", CHECK_RESULT_CONTRACT)
            .with("schema_version", CHECK_RESULT_SCHEMA_VERSION)
            .with("scope", scope_name(check.scope))
            .with("elapsed_ms", check.elapsed_ms)
            .with(
                "provenance",
                StatusValue::object(
                    StatusFields::new()
                        .with("source", check.source.as_str())
                        .with("profile", check.profile.as_str())
                        .with("front_end", check.front_end)
                        .with("programmable_build", check.programmable_build)
                        .with("diagnostics", check.diagnostics),
                ),
            )
            .with("rows", rows),
    )
}

/// Emit the versioned machine contract for `jet check --json`.
///
/// `contract` identifies the result shape and `rows` contains one deterministic
/// row per selected output and proof class. `elapsed_ms` measures the complete
/// check projection, including the proof rows.
pub(crate) fn check_result_json(check: &CheckResult) -> String {
    let rows = check
        .proof_rows
        .iter()
        .map(|row| {
            format!(
                "{{\"output\":\"{}\",\"name\":\"{}\",\"status\":\"{}\",\"detail\":\"{}\",\"diagnostic\":\"{}\"}}",
                json_escape(&row.output),
                json_escape(row.name),
                json_escape(row.status),
                json_escape(&row.detail),
                json_escape(row.diagnostic),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"status\":\"passed\",\"contract\":\"{}\",\"schema_version\":{},\"scope\":\"{}\",\"elapsed_ms\":{},\"provenance\":{{\"source\":\"{}\",\"profile\":\"{}\",\"front_end\":\"{}\",\"programmable_build\":\"{}\",\"diagnostics\":{}}},\"rows\":[{}]}}",
        CHECK_RESULT_CONTRACT,
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
            "proof: output={} {} [{}] {} (diagnostic={})",
            row.output, row.name, row.status, row.detail, row.diagnostic,
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
        let clears = jet::Diagnostics::report_clear_counts(diagnostics);
        let reports = diagnostics.iter().zip(clears).map(|(diagnostic, clears)| {
            diagnostic.to_report_with_clears(&machine_file, &source, clears)
        });
        let rendered = render_status_with_reports("inspect", false, reports, StatusFields::new());
        print!("{rendered}\n");
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
            let topics = StatusValue::array(
                slices
                    .iter()
                    .map(|slice| StatusValue::from(slice.topic.as_str())),
            );
            let payload = render_status(
                "inspect.digest",
                true,
                StatusFields::new().with(
                    "digest",
                    StatusValue::object(StatusFields::new().with("topics", topics)),
                ),
            );
            println!("{payload}");
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
    let manifest = match jet::Loader::package_facts_for_bundle(&projection.bundle) {
        Ok(manifest) => manifest,
        Err(diagnostics) => render_check_failure(path, &diagnostics, json, false),
    };
    let authority =
        jet::EffectBudget::project_application_effects(&required_effects, manifest.as_ref());

    if json {
        let output_value = StatusValue::object(
            StatusFields::new()
                .with("binding", binding)
                .with("kind", output.kind.as_str())
                .with("name", output.output_name.as_str())
                .with("entry", output.source_name.as_str())
                .with("source_path", output.source_path.as_str())
                .with("callable_identity", callable_identity.as_str())
                .with("failure_contract", failure_contract.clone())
                .with("failure_source", failure_source.clone())
                .with(
                    "effects",
                    StatusValue::array(
                        effects
                            .iter()
                            .map(|effect| StatusValue::from(effect.as_str())),
                    ),
                )
                .with(
                    "required_effects",
                    StatusValue::array(
                        effects
                            .iter()
                            .map(|effect| StatusValue::from(effect.as_str())),
                    ),
                )
                .with(
                    "granted_effects",
                    StatusValue::array(
                        authority
                            .granted_effects
                            .iter()
                            .map(|effect| StatusValue::from(effect.as_str())),
                    ),
                )
                .with(
                    "denied_effects",
                    StatusValue::array(
                        authority
                            .denied_effects
                            .iter()
                            .map(|effect| StatusValue::from(effect.as_str())),
                    ),
                )
                .with("authority", authority.authority.as_str())
                .with("selection_reason", output.selection_reason.as_str()),
        );
        let payload = render_status(
            "inspect.output",
            true,
            StatusFields::new().with("output", output_value),
        );
        println!("{payload}");
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
            if json {
                let machine_file = crate::machine_report_path_for_process(&file);
                let report = diagnostic.to_report(&machine_file, &source);
                let rendered = render_status_with_reports(
                    "inspect.env",
                    false,
                    std::iter::once(report),
                    StatusFields::new(),
                );
                print!("{rendered}\n");
            } else {
                eprint!(
                    "{}",
                    jet::render_diagnostics(&file, &source, std::slice::from_ref(&diagnostic))
                );
            }
            exit(jet::ExitCodes::USER_ERROR);
        }
    };
    if json {
        let reads = StatusValue::array(plan.environment_reads.iter().map(|read| {
            StatusValue::object(
                StatusFields::new()
                    .with("name", read.name.as_str())
                    .with("type", read.ty.as_str()),
            )
        }));
        let payload = render_status(
            "inspect.env",
            true,
            StatusFields::new().with(
                "env",
                StatusValue::object(
                    StatusFields::new()
                        .with("file", file.as_str())
                        .with("reads", reads),
                ),
            ),
        );
        println!("{payload}");
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
        let payload = render_status(
            "inspect.digest",
            true,
            StatusFields::new().with(
                "digest",
                StatusValue::object(StatusFields::new().with("value", digest)),
            ),
        );
        println!("{payload}");
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
        "Read on demand with `jet inspect digest`; do not maintain a checked-in copy.".to_string(),
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
    let entry = crate::find_project_entry(&root);
    let package = match jet::Loader::package_facts_for_entry(&entry) {
        Ok(Some(package)) => package,
        Ok(None) => {
            crate::cli_error!("E2105", "the project has no package facts");
            exit(jet::ExitCodes::USER_ERROR);
        }
        Err(diagnostics) => {
            let source = fs::read_to_string(&entry).unwrap_or_default();
            let file = entry.display().to_string();
            if json {
                let machine_file = crate::machine_report_path_for_process(&file);
                let clears = jet::Diagnostics::report_clear_counts(&diagnostics);
                let reports = diagnostics.iter().zip(clears).map(|(diagnostic, clears)| {
                    diagnostic.to_report_with_clears(&machine_file, &source, clears)
                });
                let rendered = render_status_with_reports(
                    "inspect.provenance",
                    false,
                    reports,
                    StatusFields::new(),
                );
                print!("{rendered}\n");
            } else {
                eprint!("{}", jet::render_diagnostics(&file, &source, &diagnostics));
            }
            exit(jet::ExitCodes::USER_ERROR);
        }
    };
    let requirement = package
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

fn render_provenance_json(
    requirement: jet::Package::ProvenanceRequirement,
    reports: &[jet::Lock::DependencyProvenanceReport],
) {
    let packages = StatusValue::array(reports.iter().map(render_provenance_json_package));
    let payload = render_status(
        "inspect.provenance",
        true,
        StatusFields::new().with(
            "provenance",
            StatusValue::object(
                StatusFields::new()
                    .with("require", requirement.label())
                    .with("packages", packages),
            ),
        ),
    );
    println!("{payload}");
}

fn render_provenance_json_package(report: &jet::Lock::DependencyProvenanceReport) -> StatusValue {
    let integrity_evidence = matches!(
        report.integrity.status,
        jet::Lock::ProvenanceStatus::Enforced
    )
    .then_some("E1204");
    StatusValue::object(
        StatusFields::new()
            .with("name", report.name.as_str())
            .with("version", report.version.as_str())
            .with(
                "integrity",
                render_provenance_json_field(&report.integrity, integrity_evidence),
            )
            .with(
                "transparency",
                render_provenance_json_field(&report.transparency, None),
            )
            .with(
                "publisher",
                render_provenance_json_field(&report.publisher, None),
            )
            .with("build", render_provenance_json_field(&report.build, None))
            .with(
                "required_effects",
                StatusValue::array(
                    report
                        .required_effects
                        .iter()
                        .map(|effect| StatusValue::from(effect.as_str())),
                ),
            )
            .with(
                "granted_effects",
                StatusValue::array(
                    report
                        .granted_effects
                        .iter()
                        .map(|effect| StatusValue::from(effect.as_str())),
                ),
            )
            .with(
                "denied_effects",
                StatusValue::array(
                    report
                        .denied_effects
                        .iter()
                        .map(|effect| StatusValue::from(effect.as_str())),
                ),
            )
            .with("authority", report.authority.as_str()),
    )
}

fn render_provenance_json_field(
    field: &jet::Lock::ProvenanceField,
    evidence: Option<&str>,
) -> StatusValue {
    let mut fields = StatusFields::new()
        .with("value", field.value.as_str())
        .with("status", field.status.label());
    if let Some(evidence) = evidence {
        fields = fields.with("evidence", evidence);
    }
    StatusValue::object(fields)
}

fn shape_type_label(ty: &ShapeType) -> String {
    match ty {
        ShapeType::Null => "Null".to_string(),
        ShapeType::Bool => "Bool".to_string(),
        ShapeType::Int => "Int".to_string(),
        ShapeType::Float => "Float".to_string(),
        ShapeType::Number => "Number".to_string(),
        ShapeType::Text => "Text".to_string(),
        ShapeType::Bytes => "Bytes".to_string(),
        ShapeType::Array(inner) => format!("[{}]", shape_type_label(inner)),
        ShapeType::Optional(inner) => format!("{}?", shape_type_label(inner)),
        ShapeType::Object(identity) => format!("Object({})", identity.key()),
        ShapeType::Unknown(value) => format!("Unknown({value})"),
    }
}

fn shape_default_value(default: &ShapeDefault) -> StatusValue {
    match default {
        ShapeDefault::Null => StatusValue::Null,
        ShapeDefault::Bool(value) => StatusValue::from(*value),
        ShapeDefault::Int(value) => StatusValue::from(*value),
        ShapeDefault::Float(value) => StatusValue::Float(value.clone()),
        ShapeDefault::Text(value) => StatusValue::from(value.as_str()),
        ShapeDefault::Bytes(value) => StatusValue::array(
            value
                .iter()
                .map(|byte| StatusValue::from(i128::from(*byte))),
        ),
        ShapeDefault::Unknown(value) => StatusValue::from(value.as_str()),
    }
}

fn shape_optional_name(
    value: Option<&String>,
    format: &'static str,
    ignored: &mut Vec<String>,
) -> StatusValue {
    match value {
        Some(value) => StatusValue::from(value.as_str()),
        None => {
            ignored.push(format.to_string());
            StatusValue::Null
        }
    }
}

fn shape_names_value(names: &ShapeFieldNames) -> StatusValue {
    let mut ignored = Vec::new();
    let value = StatusFields::new()
        .with("text", names.text.as_str())
        .with(
            "json",
            shape_optional_name(names.json.as_ref(), "json", &mut ignored),
        )
        .with(
            "cbor",
            shape_optional_name(names.cbor.as_ref(), "cbor", &mut ignored),
        )
        .with(
            "csv",
            shape_optional_name(names.csv.as_ref(), "csv", &mut ignored),
        )
        .with(
            "toml",
            shape_optional_name(names.toml.as_ref(), "toml", &mut ignored),
        )
        .with(
            "yaml",
            shape_optional_name(names.yaml.as_ref(), "yaml", &mut ignored),
        )
        .with(
            "xml",
            shape_optional_name(names.xml.as_ref(), "xml", &mut ignored),
        )
        .with("args", names.args.as_str())
        .with("env", names.env.as_str())
        .with(
            "db",
            shape_optional_name(names.db.as_ref(), "db", &mut ignored),
        )
        .with(
            "layout",
            shape_optional_name(names.layout.as_ref(), "layout", &mut ignored),
        )
        .with(
            "ignored",
            StatusValue::array(
                ignored
                    .iter()
                    .map(|format| StatusValue::from(format.as_str())),
            ),
        );
    StatusValue::object(value)
}

fn shape_layout_kind(kind: &ShapeLayoutKind) -> &'static str {
    match kind {
        ShapeLayoutKind::Default => "default",
        ShapeLayoutKind::C => "c",
        ShapeLayoutKind::Columnar => "columnar",
    }
}

fn shape_layout_value(fact: &ShapeFact) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("kind", shape_layout_kind(&fact.layout.kind))
            .with(
                "alignment",
                fact.layout
                    .alignment
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            ),
    )
}

fn shape_dimensions_value(dimensions: &ShapeDimensions) -> StatusValue {
    match dimensions {
        ShapeDimensions::Known { rank, width } => StatusValue::object(
            StatusFields::new()
                .with("status", "known")
                .with("rank", *rank)
                .with("width", *width),
        ),
        ShapeDimensions::Unknown { reason } => StatusValue::object(
            StatusFields::new()
                .with("status", "unknown")
                .with("reason", reason.as_str()),
        ),
    }
}

fn shape_provenance_value(fact: &ShapeFact) -> StatusValue {
    let origin = match &fact.provenance.origin {
        ShapeOrigin::Declared => StatusValue::from("declared"),
        ShapeOrigin::Derived { from } => StatusValue::object(
            StatusFields::new()
                .with("kind", "derived")
                .with("from", from.key()),
        ),
        ShapeOrigin::Imported { package } => StatusValue::object(
            StatusFields::new()
                .with("kind", "imported")
                .with("package", package.as_str()),
        ),
    };
    let span = fact
        .provenance
        .span
        .map(|span| {
            StatusValue::object(
                StatusFields::new()
                    .with("start", span.start)
                    .with("end", span.end),
            )
        })
        .unwrap_or(StatusValue::Null);
    StatusValue::object(
        StatusFields::new()
            .with("origin", origin)
            .with("span", span)
            .with(
                "declaration",
                fact.provenance
                    .declaration
                    .as_deref()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            ),
    )
}

fn shape_field_value(field: &ShapeFieldFact) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("source_name", field.name.as_str())
            .with("type", shape_type_label(&field.ty))
            .with("order", field.order)
            .with("encoding", field.encoding.as_str())
            .with("skipped", field.skip)
            .with(
                "default",
                field
                    .default
                    .as_ref()
                    .map(shape_default_value)
                    .unwrap_or(StatusValue::Null),
            )
            .with("names", shape_names_value(&field.names))
            .with(
                "short",
                field
                    .short
                    .as_deref()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            )
            .with(
                "doc",
                field
                    .doc
                    .as_deref()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            ),
    )
}

fn shape_projection_value(fact: &ShapeFact, kind: ShapeProjectionKind) -> StatusValue {
    match fact.project(kind) {
        Ok(projection) => {
            let fields = StatusValue::array(projection.fields.iter().map(|field| {
                StatusValue::object(
                    StatusFields::new()
                        .with("source_name", field.source_name.as_str())
                        .with("name", field.name.as_str())
                        .with("decode_name", field.decode_name.as_str())
                        .with("type", shape_type_label(&field.ty))
                        .with("order", field.order)
                        .with("encoding", field.encoding.as_str())
                        .with(
                            "default",
                            field
                                .default
                                .as_ref()
                                .map(shape_default_value)
                                .unwrap_or(StatusValue::Null),
                        )
                        .with(
                            "short",
                            field
                                .short
                                .as_deref()
                                .map(StatusValue::from)
                                .unwrap_or(StatusValue::Null),
                        )
                        .with(
                            "doc",
                            field
                                .doc
                                .as_deref()
                                .map(StatusValue::from)
                                .unwrap_or(StatusValue::Null),
                        ),
                )
            }));
            StatusValue::object(
                StatusFields::new()
                    .with("kind", kind.as_str())
                    .with("status", "supported")
                    .with("fields", fields),
            )
        }
        Err(error) => StatusValue::object(
            StatusFields::new()
                .with("kind", kind.as_str())
                .with("status", "unsupported")
                .with("ignored", true)
                .with("reason", error.to_string()),
        ),
    }
}

fn shape_fact_value(fact: &ShapeFact) -> StatusValue {
    let identity = StatusValue::object(
        StatusFields::new()
            .with("source", fact.identity.source.as_str())
            .with("type", fact.identity.type_name.as_str())
            .with("key", fact.identity.key()),
    );
    let fields = StatusValue::array(fact.fields.iter().map(shape_field_value));
    let projections = StatusValue::array(
        ShapeProjectionKind::ALL
            .into_iter()
            .map(|kind| shape_projection_value(fact, kind)),
    );
    StatusValue::object(
        StatusFields::new()
            .with("identity", identity)
            .with("dimensions", shape_dimensions_value(&fact.dimensions))
            .with("encoding", fact.encoding.as_str())
            .with("layout", shape_layout_value(fact))
            .with("provenance", shape_provenance_value(fact))
            .with("fields", fields)
            .with("projections", projections),
    )
}

fn shape_name_text(value: Option<&String>) -> &str {
    value.map(String::as_str).unwrap_or("ignored")
}

fn print_shape_fact(fact: &ShapeFact) {
    println!(
        "shape {} (encoding={}, layout={})",
        fact.identity.key(),
        fact.encoding.as_str(),
        shape_layout_kind(&fact.layout.kind),
    );
    println!("  dimensions: {:?}", fact.dimensions);
    for field in &fact.fields {
        println!(
            "  field {}: {} (order={}, encoding={}, skipped={})",
            field.name,
            shape_type_label(&field.ty),
            field.order,
            field.encoding.as_str(),
            field.skip,
        );
        println!(
            "    names: text={} json={} cbor={} csv={} toml={} yaml={} xml={} args={} env={} db={} layout={}",
            field.names.text,
            shape_name_text(field.names.json.as_ref()),
            shape_name_text(field.names.cbor.as_ref()),
            shape_name_text(field.names.csv.as_ref()),
            shape_name_text(field.names.toml.as_ref()),
            shape_name_text(field.names.yaml.as_ref()),
            shape_name_text(field.names.xml.as_ref()),
            field.names.args,
            field.names.env,
            shape_name_text(field.names.db.as_ref()),
            shape_name_text(field.names.layout.as_ref()),
        );
    }
    for kind in ShapeProjectionKind::ALL {
        match fact.project(kind) {
            Ok(projection) => println!(
                "  projection {}: supported ({} fields)",
                kind.as_str(),
                projection.fields.len()
            ),
            Err(error) => println!("  projection {}: ignored ({error})", kind.as_str()),
        }
    }
}

/// `jet inspect shapes <file.jet>` — expose one canonical shape fact and all
/// of its typed projections without rebuilding format-specific tables.
pub(crate) fn run_shapes(args: &[String], json: bool) {
    let Some(file) = entry_file(args) else {
        crate::cli_error!(
            @fix "E2104",
            "`jet inspect shapes` needs an entry file",
            "run `jet inspect shapes examples/features/encoding/records.jet`"
        );
        exit(jet::ExitCodes::USAGE);
    };
    let path = Path::new(&file);
    let projection = check_projection(path)
        .unwrap_or_else(|diagnostics| render_check_failure(path, &diagnostics, json, false));
    let mut facts = Vec::new();
    for module in &projection.bundle.modules {
        for item in &module.items {
            let jet::AST::Item::Struct(structure) = item else {
                continue;
            };
            let fact = match jet::Sema::Schema::shape_fact_from_struct(&module.display, structure) {
                Ok(fact) => fact,
                Err(error) => {
                    crate::emit_cli_report_for_action(
                        "inspect.shapes",
                        "E2105",
                        format!(
                            "could not project shape `{}` from `{}`: {error}",
                            structure.name, module.display
                        ),
                        "the checked declaration could not form one canonical shape fact"
                            .to_string(),
                        "fix the declaration's field names, types, or format markers".to_string(),
                        json,
                    );
                    exit(jet::ExitCodes::USER_ERROR);
                }
            };
            facts.push(fact);
        }
    }
    facts.sort_by(|left, right| left.identity.cmp(&right.identity));
    if json {
        let shapes = StatusValue::array(facts.iter().map(shape_fact_value));
        println!(
            "{}",
            render_status(
                "inspect.shapes",
                true,
                StatusFields::new().with("shapes", shapes),
            )
        );
    } else {
        if facts.is_empty() {
            println!("No shape facts declared.");
        }
        for fact in &facts {
            print_shape_fact(fact);
        }
    }
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

/// `jet inspect types <file.jet>` — project checked operator hooks and their
/// mixed-operand mirror rows from the same registry metadata used by sema.
pub(crate) fn run_types(args: &[String], json: bool) {
    let Some(file) = entry_file(args) else {
        crate::cli_error!(
            @fix "E2104",
            "`jet inspect types` needs an entry file",
            "run `jet inspect types examples/features/operators/mixed_types.jet`"
        );
        exit(jet::ExitCodes::USAGE);
    };
    let path = Path::new(&file);
    let projection = check_projection(path)
        .unwrap_or_else(|diagnostics| render_check_failure(path, &diagnostics, json, false));
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut registered = Vec::new();
    for module in &projection.bundle.modules {
        let mut registry =
            jet_foundation::Traits::TraitRegistry::auto_derives_for_items(&module.items);
        registry.register_synthetic_operators();
        registered.extend(registry.operator_impls);
    }
    for implementation in &registered {
        let Some(symbol) = operator_symbol(&implementation.trait_name) else {
            continue;
        };
        let rhs = implementation.rhs.name();
        let owner = if plain_number(&implementation.left) && !plain_number(&rhs) {
            rhs.clone()
        } else {
            implementation.left.clone()
        };
        let source = format!(
            "{}.{}<{}>",
            implementation.left, implementation.trait_name, rhs
        );
        let result = implementation.result.name();
        let kind = if implementation.explicit_rhs {
            "explicit hook"
        } else {
            "hook"
        };
        let direct = format!(
            "{} {} {} -> {}  {} {}",
            implementation.left, symbol, rhs, result, kind, source
        );
        grouped.entry(owner.clone()).or_default().push(direct);
        if plain_number(&rhs)
            && !plain_number(&implementation.left)
            && matches!(implementation.trait_name.as_str(), "Add" | "Mul")
        {
            let explicit_reverse = registered.iter().any(|candidate| {
                candidate.explicit_rhs
                    && candidate.left == rhs
                    && candidate.trait_name == implementation.trait_name
                    && candidate.rhs.name() == implementation.left
            });
            if !explicit_reverse {
                grouped.entry(owner).or_default().push(format!(
                    "{} {} {} -> {}  mirror of {}",
                    rhs, symbol, implementation.left, result, source
                ));
            }
        }
    }
    for rows in grouped.values_mut() {
        rows.sort();
        rows.dedup();
    }
    if json {
        let types = StatusValue::array(grouped.iter().map(|(owner, rows)| {
            StatusValue::object(StatusFields::new().with("type", owner.as_str()).with(
                "rows",
                StatusValue::array(rows.iter().map(|row| StatusValue::from(row.as_str()))),
            ))
        }));
        let payload = render_status(
            "inspect.types",
            true,
            StatusFields::new().with("types", types),
        );
        println!("{payload}");
    } else {
        for (owner, rows) in grouped {
            println!("type {owner}:");
            for row in rows {
                println!("  {row}");
            }
        }
    }
}

fn acceleration_scope_name(scope: &jet::CLI::InspectScope) -> &'static str {
    match scope {
        jet::CLI::InspectScope::Package => "package",
        jet::CLI::InspectScope::Target(_) => "target",
        jet::CLI::InspectScope::Live(_) => "live",
        jet::CLI::InspectScope::Replay(_) => "replay",
    }
}

fn acceleration_span_value(start: usize, end: usize) -> StatusValue {
    StatusValue::object(StatusFields::new().with("start", start).with("end", end))
}

struct AccelerationRuntimeReceipt {
    receipt_digest: String,
    receipt_verb: String,
    record_sequence: u64,
    row: jet::ReceiptStore::AccelerationReceiptRow,
}

fn acceleration_runtime_receipt(
    scope: &jet::CLI::InspectScope,
    file: &str,
    profile: &str,
) -> Vec<AccelerationRuntimeReceipt> {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => return Vec::new(),
    };
    let mut receipt_root = None;
    let mut target_identities = Vec::new();
    for verb in ["test", "prove", "build"] {
        let mut argv = vec![verb.to_string()];
        match scope {
            jet::CLI::InspectScope::Target(target) => argv.push(target.clone()),
            jet::CLI::InspectScope::Package
            | jet::CLI::InspectScope::Live(_)
            | jet::CLI::InspectScope::Replay(_) => {}
        }
        if !profile.is_empty() {
            argv.extend(["--profile".to_string(), profile.to_string()]);
        }
        let inputs = jet::ReceiptStore::input_paths_for(verb, &argv, &cwd);
        if inputs.is_empty() {
            continue;
        }
        let root = jet::ReceiptStore::receipt_root_for(verb, &argv, &cwd);
        if receipt_root.is_none() {
            receipt_root = Some(root.clone());
        }
        let store = jet::ReceiptStore::ReceiptStore::new(root);
        let Ok(claim) = store.claim(verb, &argv, &inputs) else {
            continue;
        };
        let members = claim
            .inputs
            .iter()
            .map(|input| {
                (
                    input.path.to_string_lossy().into_owned(),
                    input.digest.clone(),
                )
            })
            .collect::<Vec<_>>();
        let Ok(identity) = jet::ReceiptStore::canonical_target_identity(Path::new(file), &members)
        else {
            continue;
        };
        target_identities.push((identity.input_sha256, identity.authority_root));
    }
    let Some(receipt_root) = receipt_root else {
        return Vec::new();
    };
    if target_identities.is_empty() {
        return Vec::new();
    }
    let store = jet::ReceiptStore::ReceiptStore::new(receipt_root);
    let mut index_roots = vec![cwd];
    for (_, authority_root) in &target_identities {
        if !index_roots.contains(authority_root) {
            index_roots.push(authority_root.clone());
        }
    }
    let mut entries = Vec::new();
    for index_root in index_roots {
        let Ok(index) = jet::RecordIndex::RecordIndex::load_for_project(index_root) else {
            continue;
        };
        entries.extend(
            index
                .query_kind(jet::RecordIndex::RecordKind::Receipt, true)
                .into_iter()
                .filter(|entry| {
                    target_identities.iter().any(|(target, _)| {
                        entry.identity.target_inputs_sha256.as_str() == target.as_str()
                    }) && entry.identity.tool_version == env!("CARGO_PKG_VERSION")
                }),
        );
    }
    entries.sort_by(|left, right| {
        right
            .recorded_sequence
            .cmp(&left.recorded_sequence)
            .then_with(|| right.artifact_id.cmp(&left.artifact_id))
    });
    entries.dedup_by(|left, right| left.artifact_id == right.artifact_id);
    let receipts = match store.list() {
        Ok(receipts) => receipts
            .into_iter()
            .filter_map(|receipt| store.lookup(&receipt.claim).ok().flatten())
            .collect::<Vec<_>>(),
        Err(_) => return Vec::new(),
    };
    for entry in entries {
        let Some(receipt) = receipts
            .iter()
            .find(|receipt| receipt.claim.key == entry.artifact_id)
        else {
            continue;
        };
        let receipt_members = receipt
            .claim
            .inputs
            .iter()
            .map(|input| {
                (
                    input.path.to_string_lossy().into_owned(),
                    input.digest.clone(),
                )
            })
            .collect::<Vec<_>>();
        let Ok(receipt_identity) =
            jet::ReceiptStore::canonical_target_identity(Path::new(file), &receipt_members)
        else {
            continue;
        };
        if !target_identities
            .iter()
            .any(|(target, _)| receipt_identity.input_sha256.as_str() == target.as_str())
        {
            continue;
        }
        let Ok(rows) = jet::ReceiptStore::acceleration_decisions(receipt) else {
            continue;
        };
        return rows
            .into_iter()
            .map(|row| AccelerationRuntimeReceipt {
                receipt_digest: receipt.digest.clone(),
                receipt_verb: receipt.claim.verb.clone(),
                record_sequence: entry.recorded_sequence,
                row,
            })
            .collect();
    }
    Vec::new()
}

fn acceleration_runtime_measurement_value(
    measurement: jet_foundation::MIROptimization::Acceleration::AccelerationMeasurement,
) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("items", measurement.items)
            .with("sample_items", measurement.sample_items)
            .with("sample_nanos", measurement.sample_nanos)
            .with("spawn_nanos", measurement.spawn_nanos)
            .with("bandwidth_floor_nanos", measurement.bandwidth_floor_nanos)
            .with("floor_nanos", measurement.floor_nanos)
            .with(
                "projected_serial_nanos",
                measurement
                    .projected_serial_nanos
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            )
            .with(
                "required_serial_nanos",
                measurement
                    .required_serial_nanos
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            ),
    )
}

fn acceleration_runtime_value(runtime: &AccelerationRuntimeReceipt) -> StatusValue {
    let decision = runtime.row.decision;
    let pin = match decision.status {
        AccelerationGateStatus::Pinned(pin) => StatusValue::from(pin.as_str()),
        _ => StatusValue::Null,
    };
    StatusValue::object(
        StatusFields::new()
            .with("record_sequence", runtime.record_sequence)
            .with("receipt_digest", runtime.receipt_digest.as_str())
            .with("receipt_verb", runtime.receipt_verb.as_str())
            .with("sequence", runtime.row.sequence)
            .with("function", runtime.row.function.as_str())
            .with("loop_header", runtime.row.loop_header)
            .with(
                "source",
                acceleration_span_value(runtime.row.source_start, runtime.row.source_end),
            )
            .with("transform", decision.transform.as_str())
            .with("status", decision.status.as_str())
            .with("pin", pin)
            .with("selected", decision.selected())
            .with("measurement_source", "receipt")
            .with(
                "measurement",
                acceleration_runtime_measurement_value(decision.measurement),
            ),
    )
}
fn acceleration_rejection_statement(
    reason: &jet_foundation::MIR::MirOptimizationRejection,
) -> &'static str {
    use jet_foundation::MIR::MirOptimizationRejection::*;
    match reason {
        MissingProof => "the loop lacks the required proof",
        MayTrap => "the loop may trap",
        HasEffects => "the loop body has effects",
        HasEarlyExit => "the loop has an early exit",
        MayAlias => "the loop may alias",
        CrossIterationDependency => "iterations depend on each other",
        DynamicTripCount => "the trip count is dynamic",
        UnknownCopyCost => "copy cost is unknown",
        ScalarBoundary => "the loop crosses a scalar boundary",
        ObservableLayout => "the layout is observable",
        OwnershipObligation => "the ownership proof is incomplete",
        UnsupportedOperation => "the operation is unsupported",
    }
}
fn acceleration_gate_statement(status: Option<AccelerationGateStatus>) -> String {
    match status {
        None => "the runtime profile has not been published".to_string(),
        Some(AccelerationGateStatus::BelowStaticFloor) => {
            "the source item count is at or below the static floor".to_string()
        }
        Some(AccelerationGateStatus::Pinned(pin)) => {
            format!("the loop is pinned by {}", pin.as_str())
        }
        Some(AccelerationGateStatus::NotReleaseBuild) => {
            "the build is not a release profile".to_string()
        }
        Some(AccelerationGateStatus::CrossModeParityMissing) => {
            "cross-mode parity proof is incomplete".to_string()
        }
        Some(AccelerationGateStatus::ColumnCopyNotApplicable) => {
            "nested-reuse facts do not permit a column copy".to_string()
        }
        Some(AccelerationGateStatus::ProofIncomplete) => {
            "the acceleration proof is incomplete".to_string()
        }
        Some(AccelerationGateStatus::SampleNotTimed) => {
            "the first serial chunk has not been timed".to_string()
        }
        Some(AccelerationGateStatus::SpawnCostNotMeasured) => {
            "the once-per-process spawn cost has not been measured".to_string()
        }
        Some(AccelerationGateStatus::BandwidthFloorNotMeasured) => {
            "the memory-bandwidth floor has not been measured".to_string()
        }
        Some(AccelerationGateStatus::InvalidMeasurement) => {
            "the supplied measurement is invalid".to_string()
        }
        Some(AccelerationGateStatus::ProjectedGainBelowThreshold) => {
            "projected gain is below the required threshold".to_string()
        }
        Some(AccelerationGateStatus::Selected) => {
            "the measured gate selected this transform".to_string()
        }
    }
}

fn acceleration_proof_value(proof: AccelerationProof) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("source_proven", proof.source_proven)
            .with("independent_iterations", proof.independent_iterations)
            .with("no_aliasing", proof.no_aliasing)
            .with(
                "no_cross_iteration_dependencies",
                proof.no_cross_iteration_dependencies,
            )
            .with("no_early_exit", proof.no_early_exit)
            .with("effect_free_body", proof.effect_free_body)
            .with("ownership_safe", proof.ownership_safe)
            .with("failure_order_preserved", proof.failure_order_preserved),
    )
}

fn acceleration_workload_value(workload: AccelerationWorkloadFacts) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("nested_reuse", workload.nested_reuse)
            .with("single_pass", workload.single_pass)
            .with("column_copy_applicable", workload.column_copy_applicable()),
    )
}

fn acceleration_vector_value(
    function: &str,
    fact: &jet_foundation::MIR::MirVectorFact,
) -> StatusValue {
    let (decision, accepted_rule, rejection_code, rejection_statement) = match &fact.decision {
        jet_foundation::MIR::MirOptimizationDecision::Eligible => (
            "accepted",
            StatusValue::from(fact.rule.as_str()),
            StatusValue::Null,
            StatusValue::Null,
        ),
        jet_foundation::MIR::MirOptimizationDecision::Rejected(reason) => (
            "rejected",
            StatusValue::Null,
            StatusValue::from(reason.as_str()),
            StatusValue::from(acceleration_rejection_statement(reason)),
        ),
    };
    let lane_width = fact
        .lane_width
        .map(|lane| StatusValue::from(u64::from(lane)))
        .unwrap_or(StatusValue::Null);
    StatusValue::object(
        StatusFields::new()
            .with("function", function)
            .with("loop_header", fact.loop_header.0)
            .with(
                "span",
                acceleration_span_value(fact.span.start, fact.span.end),
            )
            .with("rule", fact.rule.as_str())
            .with("accepted_rule", accepted_rule)
            .with("layout", fact.layout.as_str())
            .with("packed", fact.packed)
            .with("lane_width", lane_width)
            .with("access_count", fact.accesses.len())
            .with("no_aliasing", fact.no_aliasing)
            .with("no_early_exit", fact.no_early_exit)
            .with("effect_free_body", fact.effect_free_body)
            .with(
                "no_cross_iteration_dependencies",
                fact.no_cross_iteration_dependencies,
            )
            .with("decision", decision)
            .with("rejection_code", rejection_code)
            .with("rejection_statement", rejection_statement),
    )
}

fn acceleration_cross_mode_parity(
    fact: &jet_foundation::MIR::MirAccelerationFact,
    vector: Option<&jet_foundation::MIR::MirVectorFact>,
) -> bool {
    let Some(vector) = vector else {
        return false;
    };
    vector.no_aliasing
        && vector.no_early_exit
        && vector.effect_free_body
        && vector.no_cross_iteration_dependencies
        && fact.proof.proves(fact.transform)
}

fn acceleration_gate_value(
    function: &str,
    fact: &jet_foundation::MIR::MirAccelerationFact,
    loop_fact: Option<&jet_foundation::MIR::MirLoopFact>,
    vector: Option<&jet_foundation::MIR::MirVectorFact>,
    gate: AccelerationGate,
    release_build: bool,
    runtime: Option<&AccelerationRuntimeReceipt>,
) -> StatusValue {
    let cross_mode_parity_proven = acceleration_cross_mode_parity(fact, vector);
    let trip_count = loop_fact.and_then(|loop_fact| loop_fact.trip_count);
    let items = trip_count.and_then(|items| usize::try_from(items).ok());
    let deferred_decision = items.map(|items| {
        let mut input = AccelerationGateInput::deferred();
        input.items = items;
        input.release_build = release_build;
        input.cross_mode_parity_proven = cross_mode_parity_proven;
        fact.evaluate(gate, input)
    });
    let runtime_decision = runtime.map(|runtime| runtime.row.decision);
    let gate_status = runtime_decision
        .map(|decision| decision.status.as_str())
        .or_else(|| {
            deferred_decision
                .as_ref()
                .map(|decision| decision.status.as_str())
        })
        .unwrap_or("runtime-profile-required");
    let selected = runtime_decision
        .map(|decision| decision.selected())
        .or_else(|| {
            deferred_decision
                .as_ref()
                .map(|decision| decision.selected())
        })
        .unwrap_or(false);
    let gate_statement = acceleration_gate_statement(
        runtime_decision
            .map(|decision| decision.status)
            .or_else(|| deferred_decision.as_ref().map(|decision| decision.status)),
    );
    let trip_count = trip_count
        .map(StatusValue::from)
        .unwrap_or(StatusValue::Null);
    let runtime_profile = if runtime.is_some() {
        "receipt"
    } else {
        "not-published"
    };
    let measurement_source = if runtime.is_some() { "receipt" } else { "none" };
    let measurement = runtime_decision
        .map(|decision| acceleration_runtime_measurement_value(decision.measurement))
        .unwrap_or(StatusValue::Null);
    StatusValue::object(
        StatusFields::new()
            .with("function", function)
            .with("loop_header", fact.loop_header.0)
            .with(
                "span",
                acceleration_span_value(fact.span.start, fact.span.end),
            )
            .with("transform", fact.transform.as_str())
            .with("workload", acceleration_workload_value(fact.workload))
            .with("proof", acceleration_proof_value(fact.proof))
            .with("trip_count", trip_count)
            .with("release_build", release_build)
            .with("cross_mode_parity_proven", cross_mode_parity_proven)
            .with("gate_status", gate_status)
            .with("rejection_statement", gate_statement)
            .with("selected", selected)
            .with("runtime_profile", runtime_profile)
            .with("measurement_source", measurement_source)
            .with("measurement", measurement)
            .with(
                "gate",
                StatusValue::object(
                    StatusFields::new()
                        .with("static_floor_items", D_ACCEL_STATIC_FLOOR_ITEMS)
                        .with("cache_block_bytes", D_ACCEL_CACHE_BLOCK_BYTES)
                        .with("required_gain_numerator", D_ACCEL_REQUIRED_GAIN_NUMERATOR)
                        .with(
                            "required_gain_denominator",
                            D_ACCEL_REQUIRED_GAIN_DENOMINATOR,
                        ),
                ),
            ),
    )
}

fn run_accel(
    request: &jet::CLI::InspectRequest,
    mode: crate::OutputMode,
    gates: jet::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) {
    let file = inspect_entry_path(&request.scope);
    let inspect_scope = match &request.scope {
        jet::CLI::InspectScope::Package => CheckScope::Project,
        jet::CLI::InspectScope::Target(target) if Path::new(target).is_dir() => CheckScope::Project,
        jet::CLI::InspectScope::Target(_) => CheckScope::ExplicitFile,
        jet::CLI::InspectScope::Live(_) | jet::CLI::InspectScope::Replay(_) => CheckScope::Project,
    };
    let selected_target = match &request.scope {
        jet::CLI::InspectScope::Target(target) => Some(target.as_str()),
        _ => None,
    };
    let path = Path::new(&file);
    let projection = check_projection_for_command(
        path,
        gates,
        profile,
        setting_overrides,
        inspect_scope,
        None,
        selected_target,
    )
    .unwrap_or_else(|diagnostics| {
        render_check_failure(path, &diagnostics, mode.json, mode.color_stderr())
    });
    let build_profile = crate::CmdCompile::resolve_named_profile(profile, &file, mode);
    let release_build = build_profile.is_release();
    let build_mode = if release_build {
        jet_foundation::MIR::MirArtifactBuildMode::Release
    } else {
        jet_foundation::MIR::MirArtifactBuildMode::Dev
    };
    let (mir, _) = jet::lower_checked_semantic_mir_program_for(
        &projection.bundle,
        jet_foundation::MIR::MirArtifactRequest::new(
            jet_foundation::MIR::MirArtifactTarget::RustAot,
            jet_foundation::MIR::MirArtifactKind::NativeExecutable,
            build_mode,
        ),
    );
    let gate = AccelerationGate::d_accel1();
    let runtime_receipts = acceleration_runtime_receipt(&request.scope, &file, profile);
    let runtime_profile = if runtime_receipts.is_empty() {
        "not-published"
    } else {
        "receipt"
    };
    let measurement_source = if runtime_receipts.is_empty() {
        "none"
    } else {
        "receipt"
    };
    let runtime_rows = runtime_receipts
        .iter()
        .map(acceleration_runtime_value)
        .collect::<Vec<_>>();
    let mut functions = mir.functions.iter().collect::<Vec<_>>();
    functions.sort_by(|left, right| left.key.cmp(&right.key));
    let mut vector_rows = Vec::new();
    let mut acceleration_rows = Vec::new();
    let mut text = String::new();
    writeln!(
        text,
        "accel: scope={} source={} profile={} release_build={} runtime_profile={}",
        acceleration_scope_name(&request.scope),
        file,
        profile,
        release_build,
        runtime_profile
    )
    .expect("writing acceleration inspect output cannot fail");
    writeln!(
        text,
        "gate: static_floor_items={} cache_block_bytes={} required_gain={}/{} measurement_source={}",
        D_ACCEL_STATIC_FLOOR_ITEMS,
        D_ACCEL_CACHE_BLOCK_BYTES,
        D_ACCEL_REQUIRED_GAIN_NUMERATOR,
        D_ACCEL_REQUIRED_GAIN_DENOMINATOR,
        measurement_source
    )
    .expect("writing acceleration inspect output cannot fail");
    for runtime in &runtime_receipts {
        let decision = runtime.row.decision;
        let measurement = decision.measurement;
        writeln!(
            text,
            "runtime acceleration verb={} receipt={} sequence={} function={} loop={} source={}-{} transform={} status={} selected={} items={} sample_items={} sample_nanos={} spawn_nanos={} bandwidth_floor_nanos={} floor_nanos={} projected_serial_nanos={} required_serial_nanos={}",
            runtime.receipt_verb,
            runtime.receipt_digest,
            runtime.row.sequence,
            runtime.row.function,
            runtime.row.loop_header,
            runtime.row.source_start,
            runtime.row.source_end,
            decision.transform.as_str(),
            decision.status.as_str(),
            decision.selected(),
            measurement.items,
            measurement.sample_items,
            measurement.sample_nanos,
            measurement.spawn_nanos,
            measurement.bandwidth_floor_nanos,
            measurement.floor_nanos,
            measurement
                .projected_serial_nanos
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string()),
            measurement
                .required_serial_nanos
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string())
        )
        .expect("writing acceleration inspect output cannot fail");
    }
    if runtime_receipts.is_empty() {
        text.push_str("runtime profile: no matching authenticated receipt.\n");
    }
    for function in functions {
        for vector in &function.optimization.vector_facts {
            vector_rows.push(acceleration_vector_value(&function.key, vector));
            match &vector.decision {
                jet_foundation::MIR::MirOptimizationDecision::Eligible => {
                    writeln!(
                        text,
                        "vector {} loop={} span={}-{} rule={} accepted",
                        function.key,
                        vector.loop_header.0,
                        vector.span.start,
                        vector.span.end,
                        vector.rule.as_str()
                    )
                    .expect("writing acceleration inspect output cannot fail");
                }
                jet_foundation::MIR::MirOptimizationDecision::Rejected(reason) => {
                    writeln!(
                        text,
                        "vector {} loop={} span={}-{} rule={} rejected: {} ({})",
                        function.key,
                        vector.loop_header.0,
                        vector.span.start,
                        vector.span.end,
                        vector.rule.as_str(),
                        acceleration_rejection_statement(reason),
                        reason.as_str()
                    )
                    .expect("writing acceleration inspect output cannot fail");
                }
            }
        }
        for acceleration in &function.optimization.acceleration_facts {
            let loop_fact = function
                .optimization
                .loop_facts
                .iter()
                .find(|candidate| candidate.header == acceleration.loop_header);
            let vector = function
                .optimization
                .vector_facts
                .iter()
                .find(|candidate| candidate.loop_header == acceleration.loop_header);
            let runtime = runtime_receipts.iter().find(|runtime| {
                runtime.row.function == function.key
                    && u64::from(runtime.row.loop_header) == acceleration.loop_header.0
                    && runtime.row.source_start == acceleration.span.start
                    && runtime.row.source_end == acceleration.span.end
            });
            acceleration_rows.push(acceleration_gate_value(
                &function.key,
                acceleration,
                loop_fact,
                vector,
                gate,
                release_build,
                runtime,
            ));
            let trip_count = loop_fact
                .and_then(|loop_fact| loop_fact.trip_count)
                .map(|items| items.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let items = loop_fact
                .and_then(|loop_fact| loop_fact.trip_count)
                .and_then(|items| usize::try_from(items).ok());
            let deferred_status = items.map(|items| {
                let mut input = AccelerationGateInput::deferred();
                input.items = items;
                input.release_build = release_build;
                input.cross_mode_parity_proven =
                    acceleration_cross_mode_parity(acceleration, vector);
                acceleration.evaluate(gate, input).status
            });
            let gate_status = runtime
                .map(|runtime| runtime.row.decision.status)
                .or(deferred_status);
            let status = gate_status
                .map(|status| status.as_str())
                .unwrap_or("runtime-profile-required");
            let gate_statement = acceleration_gate_statement(gate_status);
            let selected = runtime
                .map(|runtime| runtime.row.decision.selected())
                .or_else(|| deferred_status.map(|status| status.is_selected()))
                .unwrap_or(false);
            let measurement_source = if runtime.is_some() { "receipt" } else { "none" };
            writeln!(
                text,
                "acceleration {} loop={} span={}-{} transform={} status={} reason={} trip_count={} selected={} measurement_source={}",
                function.key,
                acceleration.loop_header.0,
                acceleration.span.start,
                acceleration.span.end,
                acceleration.transform.as_str(),
                status,
                gate_statement,
                trip_count,
                selected,
                measurement_source
            )
            .expect("writing acceleration inspect output cannot fail");
        }
    }
    if vector_rows.is_empty() {
        text.push_str("No vector facts declared.\n");
    }
    if acceleration_rows.is_empty() {
        text.push_str("No acceleration facts declared.\n");
    }
    if mode.json {
        println!(
            "{}",
            render_status(
                "inspect.accel",
                true,
                StatusFields::new()
                    .with("scope", acceleration_scope_name(&request.scope))
                    .with("source", file)
                    .with("profile", profile)
                    .with("release_build", release_build)
                    .with("runtime_profile", runtime_profile)
                    .with("measurement_source", measurement_source)
                    .with("vectors", StatusValue::array(vector_rows))
                    .with("decisions", StatusValue::array(acceleration_rows))
                    .with("runtime_decisions", StatusValue::array(runtime_rows))
                    .with(
                        "gate",
                        StatusValue::object(
                            StatusFields::new()
                                .with("static_floor_items", D_ACCEL_STATIC_FLOOR_ITEMS)
                                .with("cache_block_bytes", D_ACCEL_CACHE_BLOCK_BYTES)
                                .with("required_gain_numerator", D_ACCEL_REQUIRED_GAIN_NUMERATOR,)
                                .with(
                                    "required_gain_denominator",
                                    D_ACCEL_REQUIRED_GAIN_DENOMINATOR,
                                ),
                        ),
                    ),
            )
        );
    } else {
        print!("{text}");
    }
}

/// Project one row from the compiler-owned decision ledger.
fn decision_row_value(row: &MirDecisionRow) -> StatusValue {
    let identity = row
        .identity
        .as_ref()
        .map(|identity| {
            StatusValue::object(
                StatusFields::new()
                    .with("source", identity.source.as_str())
                    .with("configuration", identity.configuration.as_str())
                    .with("profile", identity.profile.as_str())
                    .with("target", identity.target.as_str())
                    .with("implementation", identity.implementation.as_str())
                    .with("artifact", identity.artifact.as_str())
                    .with("run", identity.run.as_str()),
            )
        })
        .unwrap_or(StatusValue::Null);
    StatusValue::object(
        StatusFields::new()
            .with("id", row.id)
            .with("kind", row.kind.as_str())
            .with("disposition", row.disposition.as_str())
            .with(
                "function_id",
                row.function
                    .map(|function| StatusValue::from(function.0))
                    .unwrap_or(StatusValue::Null),
            )
            .with(
                "function",
                if row.function_name.is_empty() {
                    StatusValue::Null
                } else {
                    StatusValue::from(row.function_name.as_str())
                },
            )
            .with(
                "span",
                acceleration_span_value(row.span.start, row.span.end),
            )
            .with("rule", row.rule.as_str())
            .with("reason", row.reason.as_str())
            .with("producer", row.producer.as_str())
            .with("evidence", row.evidence.as_str())
            .with(
                "evidence_method",
                row.evidence_method
                    .map(|method| StatusValue::from(method.as_str()))
                    .unwrap_or(StatusValue::Null),
            )
            .with(
                "derivation_disposition",
                row.derivation_disposition
                    .map(|disposition| StatusValue::from(disposition.as_str()))
                    .unwrap_or(StatusValue::Null),
            )
            .with(
                "derivation_ref",
                row.derivation
                    .as_ref()
                    .map(|reference| StatusValue::from(reference.id.as_str()))
                    .unwrap_or(StatusValue::Null),
            )
            .with("identity", identity)
            .with(
                "edit",
                row.edit
                    .as_ref()
                    .map(|edit| {
                        StatusValue::object(
                            StatusFields::new()
                                .with(
                                    "span",
                                    acceleration_span_value(edit.span.start, edit.span.end),
                                )
                                .with("replacement", edit.replacement.as_str()),
                        )
                    })
                    .unwrap_or(StatusValue::Null),
            ),
    )
}

fn decision_kind_status_value(rows: &[MirDecisionRow]) -> StatusValue {
    let kinds = [
        MirDecisionKind::Tier,
        MirDecisionKind::Inline,
        MirDecisionKind::Vectorize,
        MirDecisionKind::Parallel,
        MirDecisionKind::Copy,
        MirDecisionKind::Bounds,
        MirDecisionKind::Deopt,
        MirDecisionKind::Unreachable,
        MirDecisionKind::LoopInvariant,
    ];
    let mut fields = StatusFields::new();
    for kind in kinds {
        let count = rows.iter().filter(|row| row.kind == kind).count();
        fields = fields.with(
            kind.as_str(),
            StatusValue::object(
                StatusFields::new()
                    .with(
                        "status",
                        if count == 0 {
                            "not-applicable"
                        } else {
                            "present"
                        },
                    )
                    .with("rows", count),
            ),
        );
    }
    StatusValue::object(fields)
}

fn canonical_pass_status_value(record: &jet_foundation::CanonicalPass::Record) -> StatusValue {
    fn snapshot(value: &str) -> StatusValue {
        StatusValue::parse(value).unwrap_or_else(|_| StatusValue::String(value.to_string()))
    }
    StatusValue::object(
        StatusFields::new()
            .with("schema", jet_foundation::CanonicalPass::SCHEMA)
            .with("protocol", jet_foundation::CanonicalPass::PROTOCOL)
            .with("stage", record.stage.as_str())
            .with("operation_id", record.operation_id.as_str())
            .with("occurrence", record.occurrence)
            .with("order", record.order)
            .with(
                "source",
                StatusValue::object(
                    StatusFields::new()
                        .with("path", record.source.as_str())
                        .with(
                            "process",
                            std::env::var("JET_ADAPTER_CANONICAL_PASS_PROCESS")
                                .unwrap_or_else(|_| "inspect".to_string()),
                        ),
                ),
            )
            .with(
                "input",
                StatusValue::object(
                    StatusFields::new()
                        .with("representation", record.input_representation.as_str())
                        .with("canonical_payload", snapshot(&record.input_payload))
                        .with("identity", snapshot(&record.input_identity)),
                ),
            )
            .with(
                "output",
                StatusValue::object(
                    StatusFields::new()
                        .with("representation", record.output_representation.as_str())
                        .with("canonical_payload", snapshot(&record.output_payload))
                        .with("identity", snapshot(&record.output_identity)),
                ),
            )
            .with(
                "premises",
                StatusValue::array(
                    record
                        .premises
                        .iter()
                        .map(|premise| StatusValue::from(premise.as_str())),
                ),
            )
            .with("disposition", record.disposition.as_str()),
    )
}

fn run_decisions(
    request: &jet::CLI::InspectRequest,
    mode: crate::OutputMode,
    gates: jet::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) {
    let file = inspect_entry_path(&request.scope);
    let inspect_scope = match &request.scope {
        jet::CLI::InspectScope::Package => CheckScope::Project,
        jet::CLI::InspectScope::Target(target) if Path::new(target).is_dir() => CheckScope::Project,
        jet::CLI::InspectScope::Target(_) => CheckScope::ExplicitFile,
        jet::CLI::InspectScope::Live(_) | jet::CLI::InspectScope::Replay(_) => CheckScope::Project,
    };
    jet_foundation::CanonicalPass::clear();
    let selected_target = match &request.scope {
        jet::CLI::InspectScope::Target(target) => Some(target.as_str()),
        _ => None,
    };
    let projection = check_projection_for_command(
        Path::new(&file),
        gates,
        profile,
        setting_overrides,
        inspect_scope,
        None,
        selected_target,
    )
    .unwrap_or_else(|diagnostics| {
        render_check_failure(
            Path::new(&file),
            &diagnostics,
            mode.json,
            mode.color_stderr(),
        )
    });
    let (mir, artifact) = jet::lower_checked_semantic_mir_program_for(
        &projection.bundle,
        jet_foundation::MIR::MirArtifactRequest::new(
            jet_foundation::MIR::MirArtifactTarget::RustAot,
            jet_foundation::MIR::MirArtifactKind::NativeExecutable,
            jet_foundation::MIR::MirArtifactBuildMode::Dev,
        ),
    );
    let mut additional_rows = projection
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.decision_row.clone())
        .collect::<Vec<_>>();
    additional_rows.extend(jet_jit::plan_mir_tiers(&mir, artifact).decision_ledger_rows(&mir));
    let ledger = jet_foundation::MIROptimization::decision_ledger(
        &mir,
        Some(artifact),
        profile,
        "not-observed",
        additional_rows,
    );
    let rows = ledger.rows;
    let canonical_passes = jet_foundation::CanonicalPass::take()
        .iter()
        .map(canonical_pass_status_value)
        .collect::<Vec<_>>();
    if mode.json {
        println!(
            "{}",
            render_status(
                "inspect.decisions",
                true,
                StatusFields::new()
                    .with("scope", acceleration_scope_name(&request.scope))
                    .with("source", file)
                    .with("profile", profile)
                    .with(
                        "rows",
                        StatusValue::array(rows.iter().map(decision_row_value)),
                    )
                    .with("kinds", decision_kind_status_value(&rows))
                    .with("canonical_routes", StatusValue::array(canonical_passes))
                    .with("record_status", "current")
                    .with("retention", "bounded")
                    .with("redaction", "none")
            )
        );
        return;
    }
    let mut text = String::new();
    writeln!(
        text,
        "decisions: scope={} source={} profile={} record_status=current retention=bounded redaction=none",
        acceleration_scope_name(&request.scope),
        file,
        profile
    )
    .expect("writing decision inspect output cannot fail");
    for row in &rows {
        let function = if row.function_name.is_empty() {
            "<package>"
        } else {
            row.function_name.as_str()
        };
        let method = row
            .evidence_method
            .map_or("unavailable", |method| method.as_str());
        let derivation_disposition = row
            .derivation_disposition
            .map_or("unknown", |disposition| disposition.as_str());
        let derivation_ref = row
            .derivation
            .as_ref()
            .map_or("unavailable", |reference| reference.id.as_str());
        let identity = row.identity.as_ref().map_or_else(
            || "identity=unavailable".to_string(),
            |identity| {
                format!(
                    "source={} configuration={} profile={} target={} implementation={} artifact={} run={}",
                    identity.source,
                    identity.configuration,
                    identity.profile,
                    identity.target,
                    identity.implementation,
                    identity.artifact,
                    identity.run,
                )
            },
        );
        writeln!(
            text,
            "decision id={} kind={} disposition={} derivation_disposition={} function={} span={}-{} rule={} reason={} producer={} evidence={} method={} derivation_ref={} {}",
            row.id,
            row.kind.as_str(),
            row.disposition.as_str(),
            derivation_disposition,
            function,
            row.span.start,
            row.span.end,
            row.rule,
            row.reason,
            row.producer,
            row.evidence,
            method,
            derivation_ref,
            identity,
        )
        .expect("writing decision inspect output cannot fail");
    }
    for kind in [
        MirDecisionKind::Tier,
        MirDecisionKind::Inline,
        MirDecisionKind::Vectorize,
        MirDecisionKind::Parallel,
        MirDecisionKind::Copy,
        MirDecisionKind::Bounds,
        MirDecisionKind::Deopt,
        MirDecisionKind::Unreachable,
        MirDecisionKind::LoopInvariant,
    ] {
        let present = rows.iter().any(|row| row.kind == kind);
        writeln!(
            text,
            "kind {}: {}",
            kind.as_str(),
            if present { "present" } else { "not-applicable" }
        )
        .expect("writing decision inspect output cannot fail");
    }
    print!("{text}");
}

/// Dispatch the canonical `jet inspect <plane>` grammar.  The parser lives in
/// `jet::CLI`; this function only selects the existing typed projection for
/// the already-parsed plane and scope.
pub(crate) fn run_inspect(
    request: &jet::CLI::InspectRequest,
    mode: crate::OutputMode,
    gates: jet::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    no_os: bool,
) {
    if request.plane == jet::CLI::InspectPlane::Decisions {
        match &request.scope {
            jet::CLI::InspectScope::Live(pid) => {
                run_inspect_live(request.plane, *pid, mode);
            }
            jet::CLI::InspectScope::Replay(artifact) => {
                run_inspect_replay(request.plane, artifact, profile, setting_overrides, mode);
            }
            jet::CLI::InspectScope::Package | jet::CLI::InspectScope::Target(_) => {
                run_decisions(request, mode, gates, profile, setting_overrides);
            }
        }
        return;
    }
    if request.plane == jet::CLI::InspectPlane::Accel {
        run_accel(request, mode, gates, profile, setting_overrides);
        return;
    }
    match &request.scope {
        jet::CLI::InspectScope::Live(pid) => {
            run_inspect_live(request.plane, *pid, mode);
            return;
        }
        jet::CLI::InspectScope::Replay(artifact) => {
            run_inspect_replay(request.plane, artifact, profile, setting_overrides, mode);
            return;
        }
        jet::CLI::InspectScope::Package | jet::CLI::InspectScope::Target(_) => {}
    }

    if request.plane == jet::CLI::InspectPlane::Build
        && request
            .options
            .iter()
            .any(|option| option == "--coverage" || option.starts_with("--coverage="))
    {
        run_build_coverage(mode);
        return;
    }

    let file = inspect_entry_path(&request.scope);
    let mut args = vec![file.clone()];
    args.extend(request.options.iter().cloned());
    match request.plane {
        jet::CLI::InspectPlane::Decisions => {
            run_decisions(request, mode, gates, profile, setting_overrides);
        }
        jet::CLI::InspectPlane::Types => {
            run_types(&args, mode.json);
        }
        jet::CLI::InspectPlane::Shapes => {
            run_shapes(&args, mode.json);
        }
        jet::CLI::InspectPlane::Accel => {
            unreachable!("accel inspect is routed before live/replay dispatch");
        }
        jet::CLI::InspectPlane::Rights => {
            let inspect_scope = match &request.scope {
                jet::CLI::InspectScope::Package => CheckScope::Project,
                jet::CLI::InspectScope::Target(target) if Path::new(target).is_dir() => {
                    CheckScope::Project
                }
                jet::CLI::InspectScope::Target(_) => CheckScope::ExplicitFile,
                _ => unreachable!("live and replay inspect scopes returned above"),
            };
            let selected_target = match &request.scope {
                jet::CLI::InspectScope::Target(target) => Some(target.as_str()),
                _ => None,
            };
            let projection = check_projection_for_command(
                Path::new(&file),
                gates,
                profile,
                setting_overrides,
                inspect_scope,
                None,
                selected_target,
            )
            .unwrap_or_else(|diagnostics| {
                render_check_failure(
                    Path::new(&file),
                    &diagnostics,
                    mode.json,
                    mode.color_stderr(),
                )
            });
            run_rights(&projection, inspect_scope, &file, mode.json);
        }
        jet::CLI::InspectPlane::Claims => {
            run_claims(&args[0], mode.json);
        }
        jet::CLI::InspectPlane::Structure => {
            crate::CmdStructure::run_structure(&args, mode.json, mode.color_stderr(), gates);
        }
        jet::CLI::InspectPlane::Build => {
            let query_args = args.iter().collect::<Vec<_>>();
            crate::CmdCompile::run_build_query("graph", &query_args, mode);
        }
        jet::CLI::InspectPlane::Gates => {
            crate::CmdGates::run(&args, mode.json, mode.color_stderr(), gates, false);
        }
    }
    let _ = (profile, no_os);
}

fn coverage_fact_value(fact: &jet_foundation::Coverage::CoverageFact) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("plane", fact.plane.as_str())
            .with("rows", fact.rows)
            .with("edit_covered", fact.edit_covered)
            .with("reason_covered", fact.reason_covered)
            .with("behavior_reasons", fact.behavior_reasons)
            .with("design_reasons", fact.design_reasons)
            .with("ambiguous_reasons", fact.ambiguous_reasons)
            .with("uncovered", fact.uncovered),
    )
}

fn run_build_coverage(mode: crate::OutputMode) {
    let entries = Registry::diagnostic_coverage_entries();
    let facts = jet_foundation::Coverage::try_facts(entries.clone())
        .expect("registered diagnostic coverage rows must be valid");
    let validation = Registry::validate_diagnostic_coverage(entries);
    if mode.json {
        let ok = validation.is_ok();
        let coverage = StatusValue::array(facts.iter().map(coverage_fact_value));
        let mut fields = StatusFields::new().with("coverage", coverage);
        if let Err(error) = validation {
            fields = fields.with("validation_error", error);
        }
        println!("{}", render_status("inspect.build", ok, fields));
        return;
    }
    println!("diagnostic coverage");
    println!("plane\trows\twith fix_edits\twith reason\tuncovered\tstate");
    for fact in facts {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            fact.plane,
            fact.rows,
            fact.edit_covered,
            fact.reason_covered,
            fact.uncovered,
            if fact.is_closed() { "closed" } else { "open" },
        );
    }
    if let Err(error) = validation {
        println!("coverage validation error: {error}");
    }
}

fn inspect_entry_path(scope: &jet::CLI::InspectScope) -> String {
    match scope {
        jet::CLI::InspectScope::Target(target) if !Path::new(target).is_dir() => target.clone(),
        jet::CLI::InspectScope::Target(target) => {
            let root = crate::require_manifest_root(
                Path::new(target),
                "error: no package.jet found for this inspect target",
            );
            crate::find_project_entry(&root)
                .to_string_lossy()
                .into_owned()
        }
        jet::CLI::InspectScope::Package
        | jet::CLI::InspectScope::Live(_)
        | jet::CLI::InspectScope::Replay(_) => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let root = crate::require_manifest_root(
                &cwd,
                "error: no package.jet found — run inspect inside a project",
            );
            crate::find_project_entry(&root)
                .to_string_lossy()
                .into_owned()
        }
    }
}

fn run_decisions_live(pid: u32, mode: crate::OutputMode) {
    let projection = match jet::DevServer::LiveInspect::read_plane(pid, "decisions") {
        Ok(projection) => projection,
        Err(message) => {
            crate::cli_error!(
                @fix "E2105",
                message,
                format!("start the program with --observe, then run `jet inspect decisions --live {pid}`")
            );
            exit(jet::ExitCodes::USER_ERROR);
        }
    };
    let has_ledger = projection.decision_ledger.is_some();
    let ledger = projection.decision_ledger.unwrap_or_else(|| {
        let identity = MirDecisionIdentity {
            source: projection.source_id.clone(),
            configuration: projection.build_id.clone(),
            profile: "live".to_string(),
            target: projection.world_id.clone(),
            implementation: projection.protocol.clone(),
            artifact: format!("pid:{pid}"),
            run: projection.revision.clone(),
        };
        MirDecisionLedger::from_rows(
            identity,
            [MirDecisionRow::new(
                MirDecisionKind::Tier,
                MirDecisionDisposition::Unavailable,
                None,
                "",
                jet::Diagnostics::Span::new(0, 0),
                "live-decision-ledger",
                "the live runtime produced no compiler decision ledger for this run",
                "live.inspect",
                "runtime-snapshot",
            )],
        )
    });
    let record_status = if has_ledger { "current" } else { "unavailable" };
    if mode.json {
        let fields = StatusFields::new()
            .with("scope", "live")
            .with("pid", projection.pid)
            .with("protocol", projection.protocol)
            .with("session_id", projection.session_id)
            .with("source_id", projection.source_id)
            .with("build_id", projection.build_id)
            .with("revision", projection.revision)
            .with("world_id", projection.world_id)
            .with("record_status", record_status)
            .with("retention", "bounded")
            .with("redaction", "transport-safe")
            .with(
                "rows",
                StatusValue::array(ledger.rows.iter().map(decision_row_value)),
            )
            .with("kinds", decision_kind_status_value(&ledger.rows));
        println!("{}", render_status("inspect.decisions", true, fields));
        return;
    }
    let mut text = String::new();
    writeln!(
        text,
        "decisions: scope=live pid={} source={} build={} revision={} world={} record_status={} retention=bounded redaction=transport-safe",
        projection.pid,
        projection.source_id,
        projection.build_id,
        projection.revision,
        projection.world_id,
        record_status,
    )
    .expect("writing live decision inspect output cannot fail");
    for row in &ledger.rows {
        let derivation_ref = row
            .derivation
            .as_ref()
            .map_or("unavailable", |reference| reference.id.as_str());
        let derivation_disposition = row
            .derivation_disposition
            .map_or("unknown", |disposition| disposition.as_str());
        writeln!(
            text,
            "decision kind={} disposition={} derivation_disposition={} derivation_ref={} reason={} producer={}",
            row.kind.as_str(),
            row.disposition.as_str(),
            derivation_disposition,
            derivation_ref,
            row.reason,
            row.producer,
        )
        .expect("writing live decision inspect output cannot fail");
    }
    print!("{text}");
}

fn run_inspect_live(plane: jet::CLI::InspectPlane, pid: u32, mode: crate::OutputMode) {
    if plane == jet::CLI::InspectPlane::Decisions {
        run_decisions_live(pid, mode);
        return;
    }
    if mode.json {
        let projection = match jet::DevServer::LiveInspect::read_plane(pid, plane.name()) {
            Ok(projection) => projection,
            Err(message) => {
                crate::cli_error!(
                    @fix "E2105",
                    message,
                    format!("start the program with --observe, then run `jet inspect {} --live {pid}`", plane.name())
                );
                exit(jet::ExitCodes::USER_ERROR);
            }
        };
        let values = StatusValue::array(projection.values.iter().map(|value| {
            StatusValue::object(
                StatusFields::new()
                    .with("value_id", value.value_id.as_str())
                    .with("type_identity", value.type_identity.as_str())
                    .with("disposition", value.disposition.as_str())
                    .with("reason", value.reason.as_str())
                    .with(
                        "rendered_value",
                        value
                            .rendered_value
                            .as_ref()
                            .map(|rendered| StatusValue::String(rendered.clone()))
                            .unwrap_or(StatusValue::Null),
                    ),
            )
        }));
        let shapes = StatusValue::array(projection.shapes.iter().map(|shape| {
            StatusValue::object(
                StatusFields::new()
                    .with("type_identity", shape.type_identity.as_str())
                    .with(
                        "value_ids",
                        StatusValue::array(
                            shape
                                .value_ids
                                .iter()
                                .map(|value_id| StatusValue::String(value_id.clone())),
                        ),
                    ),
            )
        }));
        let fields = StatusFields::new()
            .with("plane", projection.plane)
            .with("scope", projection.scope)
            .with("pid", projection.pid)
            .with("protocol", projection.protocol)
            .with("session_id", projection.session_id)
            .with("source_id", projection.source_id)
            .with("build_id", projection.build_id)
            .with("revision", projection.revision)
            .with("world_id", projection.world_id)
            .with("values", values)
            .with("shapes", shapes);
        println!(
            "{}",
            render_status(format!("inspect.{}", plane.name()), true, fields)
        );
    } else {
        let snapshot = match jet::DevServer::LiveInspect::read(pid) {
            Ok(snapshot) => snapshot,
            Err(message) => {
                crate::cli_error!(
                    @fix "E2105",
                    message,
                    format!("start the program with --observe, then run `jet inspect {} --live {pid}`", plane.name())
                );
                exit(jet::ExitCodes::USER_ERROR);
            }
        };
        print!("{}", jet::DevServer::LiveInspect::render(&snapshot));
    }
}
fn indexed_decision_record_state(artifact: &str) -> (&'static str, &'static str, &'static str) {
    let Ok(index) = jet::RecordIndex::RecordIndex::load_for_project(".") else {
        return ("unavailable", "unknown", "unknown");
    };
    let requested_path = Path::new(artifact);
    let requested_name = requested_path.file_name().and_then(|name| name.to_str());
    let matches_artifact = |entry: &jet::RecordIndex::RecordIndexEntry| {
        entry.artifact_id == artifact
            || entry.path == requested_path
            || requested_name.is_some_and(|name| {
                entry.path.file_name().and_then(|value| value.to_str()) == Some(name)
            })
    };
    let visible = index.query(false).into_iter().find(|entry| {
        entry.kind == jet::RecordIndex::RecordKind::Replay && matches_artifact(entry)
    });
    if let Some(entry) = visible {
        return (
            "current",
            if entry.saved { "saved" } else { "bounded" },
            entry.capture.as_str(),
        );
    }
    let hidden = index.query(true).into_iter().any(|entry| {
        entry.kind == jet::RecordIndex::RecordKind::Replay && matches_artifact(&entry)
    });
    if hidden {
        ("redacted", "retained", "sensitive")
    } else {
        ("unavailable", "unknown", "unknown")
    }
}

fn run_decisions_replay(
    artifact: &str,
    authority: &crate::ProveReplay::ReplayAuthority,
    mode: crate::OutputMode,
) {
    let has_ledger = authority.decision_ledger.is_some();
    let ledger = authority.decision_ledger.clone().unwrap_or_else(|| {
        let identity = MirDecisionIdentity {
            source: "replay-artifact".to_string(),
            configuration: "replay-receipt".to_string(),
            profile: "replay".to_string(),
            target: "replay".to_string(),
            implementation: "mir-v1".to_string(),
            artifact: artifact.to_string(),
            run: format!("replay:{artifact}"),
        };
        MirDecisionLedger::from_rows(
            identity,
            [MirDecisionRow::new(
                MirDecisionKind::Tier,
                MirDecisionDisposition::Unavailable,
                None,
                "",
                jet::Diagnostics::Span::new(0, 0),
                "replay-decision-ledger",
                "the replay artifact has no retained compiler decision section",
                "replay.inspect",
                "replay-receipt",
            )],
        )
    });
    let (indexed_status, retention, redaction) = indexed_decision_record_state(artifact);
    let record_status = if has_ledger {
        "current"
    } else {
        indexed_status
    };
    if mode.json {
        let fields = StatusFields::new()
            .with("plane", "decisions")
            .with("scope", "replay")
            .with("artifact", artifact)
            .with("expected_outcome", authority.expected_outcome.clone())
            .with("expected_status", authority.expected_status)
            .with("record_status", record_status)
            .with("retention", retention)
            .with("redaction", redaction)
            .with(
                "rows",
                StatusValue::array(ledger.rows.iter().map(decision_row_value)),
            )
            .with("kinds", decision_kind_status_value(&ledger.rows));
        println!(
            "{}",
            StatusEnvelope::new("inspect.decisions", true)
                .with_fields(fields)
                .json()
        );
    } else {
        println!("decisions replay");
        println!("artifact: {artifact}");
        println!("record_status: {record_status} retention: {retention} redaction: {redaction}");
        println!(
            "expected: {} ({})",
            authority.expected_outcome, authority.expected_status
        );
        for row in &ledger.rows {
            let derivation_ref = row
                .derivation
                .as_ref()
                .map_or("unavailable", |reference| reference.id.as_str());
            let derivation_disposition = row
                .derivation_disposition
                .map_or("unknown", |disposition| disposition.as_str());
            println!(
                "decision kind={} disposition={} derivation_disposition={} derivation_ref={} reason={} producer={}",
                row.kind.as_str(),
                row.disposition.as_str(),
                derivation_disposition,
                derivation_ref,
                row.reason,
                row.producer,
            );
        }
    }
}

fn run_inspect_replay(
    plane: jet::CLI::InspectPlane,
    artifact: &str,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    mode: crate::OutputMode,
) {
    let file = inspect_entry_path(&jet::CLI::InspectScope::Replay(artifact.to_string()));
    let authority = crate::ProveReplay::open_named_replay(
        &file,
        artifact,
        profile,
        setting_overrides,
        mode.json,
    )
    .unwrap_or_else(|status| exit(status));
    if plane == jet::CLI::InspectPlane::Decisions {
        run_decisions_replay(artifact, &authority, mode);
        return;
    }
    let acts = StatusValue::array(authority.recorded_run.acts.iter().map(|act| {
        StatusValue::object(
            StatusFields::new()
                .with("sequence", act.sequence)
                .with("function", act.function.as_str())
                .with("line", act.line)
                .with(
                    "locals",
                    StatusValue::array(act.locals.iter().map(|local| {
                        StatusValue::object(
                            StatusFields::new()
                                .with("name", local.name.as_str())
                                .with("type_name", local.type_name.as_str())
                                .with("value", local.value.as_str()),
                        )
                    })),
                ),
        )
    }));
    let values = StatusValue::array(
        authority
            .recorded_run
            .acts
            .iter()
            .flat_map(|act| act.locals.iter())
            .map(|local| {
                StatusValue::object(
                    StatusFields::new()
                        .with("value_id", local.name.as_str())
                        .with("type_identity", local.type_name.as_str())
                        .with("disposition", "captured")
                        .with("reason", "")
                        .with("rendered_value", local.value.as_str()),
                )
            }),
    );
    let mut shape_values = BTreeMap::<String, BTreeSet<String>>::new();
    for act in &authority.recorded_run.acts {
        for local in &act.locals {
            shape_values
                .entry(local.type_name.clone())
                .or_default()
                .insert(local.name.clone());
        }
    }
    let shapes = StatusValue::array(shape_values.into_iter().map(|(type_identity, value_ids)| {
        StatusValue::object(
            StatusFields::new()
                .with("type_identity", type_identity)
                .with(
                    "value_ids",
                    StatusValue::array(value_ids.into_iter().map(StatusValue::String)),
                ),
        )
    }));
    if mode.json {
        let fields = StatusFields::new()
            .with("plane", plane.name())
            .with("scope", "replay")
            .with("artifact", artifact)
            .with("expected_outcome", authority.expected_outcome.clone())
            .with("expected_status", authority.expected_status)
            .with("acts", acts)
            .with("values", values)
            .with("shapes", shapes);
        println!(
            "{}",
            StatusEnvelope::new(format!("inspect.{}", plane.name()), true)
                .with_fields(fields)
                .json()
        );
    } else {
        println!("{} replay", plane.name());
        println!("artifact: {artifact}");
        println!(
            "expected: {} ({})",
            authority.expected_outcome, authority.expected_status
        );
        for act in &authority.recorded_run.acts {
            println!("  #{} {}:{}", act.sequence, act.function, act.line);
            for local in &act.locals {
                println!("    {}: {} = {}", local.name, local.type_name, local.value);
            }
        }
    }
}

fn run_rights(projection: &CheckProjection, scope: CheckScope, target: &str, json: bool) {
    #[derive(Clone)]
    struct CallableView {
        key: String,
        name: String,
        source: String,
        span: jet_foundation::Diagnostics::Span,
        declared_effects: Option<Vec<String>>,
        is_pure: bool,
        is_comptime: bool,
        is_replayable: bool,
        inferred_seed: Option<String>,
    }

    struct CallableReport {
        view: CallableView,
        declared: jet_foundation::sema::RightsRow,
        inferred: jet_foundation::sema::RightsRow,
        inferred_effects: jet_foundation::Authority::Holds,
        direct_effects: jet_foundation::Authority::Holds,
        callees: Vec<String>,
        maximal: bool,
        walk: jet_foundation::sema::RightsWalk,
    }

    fn add_func(
        out: &mut Vec<CallableView>,
        alias: &str,
        namespace: &str,
        source: &str,
        owner: Option<&str>,
        function: &jet::AST::Func,
    ) {
        let identity = owner
            .map(|owner| format!("{namespace}{owner}::{}", function.name))
            .unwrap_or_else(|| format!("{namespace}{}", function.name));
        out.push(CallableView {
            key: format!("{alias}::{identity}"),
            name: function.name.clone(),
            source: source.to_string(),
            span: function.span,
            declared_effects: function
                .declared_effects
                .as_ref()
                .map(|effects| effects.iter().map(|(name, _)| name.clone()).collect()),
            is_pure: function.is_pure,
            is_comptime: function.is_comptime,
            is_replayable: function.is_replayable,
            inferred_seed: None,
        });
    }

    fn add_trait_method(
        out: &mut Vec<CallableView>,
        alias: &str,
        namespace: &str,
        source: &str,
        trait_name: &str,
        method: &jet::AST::TraitMethodSig,
    ) {
        let identity = format!("{namespace}{trait_name}::{}", method.name);
        out.push(CallableView {
            key: format!("{alias}::{identity}"),
            name: method.name.clone(),
            source: source.to_string(),
            span: method.span,
            declared_effects: method
                .declared_effects
                .as_ref()
                .map(|effects| effects.iter().map(|(name, _)| name.clone()).collect()),
            is_pure: method.is_pure,
            is_comptime: false,
            is_replayable: false,
            inferred_seed: None,
        });
    }

    fn add_extern(
        out: &mut Vec<CallableView>,
        alias: &str,
        namespace: &str,
        source: &str,
        function: &jet::AST::ExternFn,
    ) {
        out.push(CallableView {
            key: format!("{alias}::{namespace}{}", function.name),
            name: function.name.clone(),
            source: source.to_string(),
            span: function.span,
            declared_effects: None,
            is_pure: false,
            is_comptime: false,
            is_replayable: false,
            inferred_seed: Some(
                function
                    .effect_root
                    .clone()
                    .unwrap_or_else(|| "FFI".to_string()),
            ),
        });
    }

    fn collect_items(
        items: &[jet::AST::Item],
        alias: &str,
        namespace: &str,
        source: &str,
        out: &mut Vec<CallableView>,
    ) {
        use jet::AST::Item;
        for item in items {
            match item {
                Item::Func(function) => add_func(out, alias, namespace, source, None, function),
                Item::Impl(definition) => {
                    for method in &definition.methods {
                        add_func(
                            out,
                            alias,
                            namespace,
                            source,
                            Some(&definition.type_name),
                            method,
                        );
                    }
                }
                Item::Struct(definition) => {
                    for method in &definition.methods {
                        add_func(
                            out,
                            alias,
                            namespace,
                            source,
                            Some(&definition.name),
                            method,
                        );
                    }
                    for block in &definition.trait_impls {
                        for method in &block.methods {
                            add_func(
                                out,
                                alias,
                                namespace,
                                source,
                                Some(&definition.name),
                                method,
                            );
                        }
                    }
                }
                Item::Enum(definition) => {
                    for method in &definition.methods {
                        add_func(
                            out,
                            alias,
                            namespace,
                            source,
                            Some(&definition.name),
                            method,
                        );
                    }
                    for block in &definition.trait_impls {
                        for method in &block.methods {
                            add_func(
                                out,
                                alias,
                                namespace,
                                source,
                                Some(&definition.name),
                                method,
                            );
                        }
                    }
                }
                Item::Trait(definition) => {
                    for method in &definition.methods {
                        add_trait_method(out, alias, namespace, source, &definition.name, method);
                    }
                }
                Item::ExternRust(block) => {
                    for function in &block.functions {
                        add_extern(out, alias, namespace, source, function);
                    }
                }
                Item::CodeModule(module) => {
                    if let Some(body) = &module.body {
                        let nested = format!("{namespace}{}__", module.name);
                        collect_items(body, alias, &nested, source, out);
                    }
                }
                Item::GenericModule(module) => {
                    let nested = format!("{namespace}{}__", module.name);
                    collect_items(&module.body, alias, &nested, source, out);
                }
                _ => {}
            }
        }
    }

    fn holds_value(holds: &jet_foundation::Authority::Holds) -> StatusValue {
        StatusValue::array(holds.iter().cloned().map(StatusValue::String))
    }

    fn holds_text(holds: &jet_foundation::Authority::Holds) -> String {
        holds.iter().cloned().collect::<Vec<_>>().join(", ")
    }

    fn verdict_name(verdict: jet_foundation::Authority::Verdict) -> &'static str {
        match verdict {
            jet_foundation::Authority::Verdict::Allowed => "allowed",
            jet_foundation::Authority::Verdict::Denied => "denied",
            jet_foundation::Authority::Verdict::Missing => "missing",
        }
    }

    fn span_value(source: &str, start: usize, end: usize) -> StatusValue {
        StatusValue::object(
            StatusFields::new()
                .with("source", source)
                .with("start", start)
                .with("end", end),
        )
    }

    fn frame_value(frame: &jet_foundation::sema::ScopeFrame) -> StatusValue {
        StatusValue::object(
            StatusFields::new()
                .with("scope", frame.scope.name())
                .with("name", frame.name.clone())
                .with("grants", holds_value(&frame.grants))
                .with(
                    "authority",
                    StatusValue::object(
                        StatusFields::new()
                            .with("source", frame.provenance.source.clone())
                            .with("reason", frame.provenance.reason.clone()),
                    ),
                ),
        )
    }

    fn row_value(row: &jet_foundation::sema::RightsRow) -> StatusValue {
        let allow = row
            .allow
            .as_ref()
            .map(holds_value)
            .unwrap_or(StatusValue::Null);
        StatusValue::object(
            StatusFields::new()
                .with("kind", row.kind.name())
                .with("allow", allow)
                .with("deny", holds_value(&row.deny))
                .with(
                    "provenance",
                    StatusValue::object(
                        StatusFields::new()
                            .with("source", row.provenance.source.clone())
                            .with("reason", row.provenance.reason.clone()),
                    ),
                ),
        )
    }

    fn row_text(row: &jet_foundation::sema::RightsRow) -> String {
        let allow = row
            .allow
            .as_ref()
            .map(holds_text)
            .unwrap_or_else(|| "unbounded".to_string());
        format!(
            "kind={} allow=[{}] deny=[{}]",
            row.kind.name(),
            allow,
            holds_text(&row.deny)
        )
    }

    fn declared_row(callable: &CallableView) -> jet_foundation::sema::RightsRow {
        let provenance = jet_foundation::sema::RightsProvenance::new(
            callable.key.clone(),
            if callable.is_comptime {
                "comptime declaration"
            } else if callable.is_pure {
                "pure declaration"
            } else if callable.is_replayable {
                "replayable declaration"
            } else if callable.declared_effects.is_some() {
                "declared effect row"
            } else {
                "inferred callable declaration"
            },
        );
        if callable.is_comptime {
            jet_foundation::sema::RightsRow::comptime(provenance)
        } else if callable.is_pure {
            jet_foundation::sema::RightsRow::pure(provenance)
        } else if callable.is_replayable {
            jet_foundation::sema::RightsRow::replayable(provenance)
        } else if let Some(effects) = &callable.declared_effects {
            let allow = effects
                .iter()
                .filter(|name| !name.starts_with('!'))
                .map(String::as_str);
            let deny = effects.iter().filter_map(|name| name.strip_prefix('!'));
            jet_foundation::sema::RightsRow::invocation(allow, deny, provenance)
        } else {
            jet_foundation::sema::RightsRow::callable(provenance)
        }
    }

    let selected_path = Path::new(target).canonicalize().ok();
    let mut callables = Vec::new();
    for (module_index, module) in projection.bundle.modules.iter().enumerate() {
        if scope == CheckScope::ExplicitFile {
            let module_path = module
                .path
                .canonicalize()
                .unwrap_or_else(|_| module.path.clone());
            if module.display != target
                && selected_path
                    .as_ref()
                    .is_none_or(|selected| selected != &module_path)
            {
                continue;
            }
        }
        let alias = projection
            .facts
            .name_ledger
            .module_alias(module_index)
            .unwrap_or(&module.alias);
        collect_items(&module.items, alias, "", &module.display, &mut callables);
    }
    callables.sort_by(|left, right| {
        left.key
            .cmp(&right.key)
            .then(left.span.start.cmp(&right.span.start))
            .then(left.span.end.cmp(&right.span.end))
    });
    callables.dedup_by(|left, right| left.key == right.key);

    let application = &projection.bundle.package_guarantees.application_authority;
    let scope_chain = vec![jet_foundation::sema::ScopeFrame::new(
        jet_foundation::Authority::Scope::Package,
        application.authority.clone(),
        application.granted_effects.clone(),
        jet_foundation::sema::RightsProvenance::new(
            application.authority.clone(),
            "selected application authority",
        ),
    )];
    let reports = callables
        .into_iter()
        .map(|view| {
            let declared = declared_row(&view);
            let summary = projection.facts.summaries.get(&view.key);
            let mut inferred_effects = projection
                .facts
                .solved
                .get(&view.key)
                .cloned()
                .unwrap_or_default();
            if inferred_effects.is_empty() {
                if let Some(summary) = summary {
                    inferred_effects.extend(summary.direct.iter().cloned());
                }
            }
            if inferred_effects.is_empty() {
                if let Some(seed) = &view.inferred_seed {
                    inferred_effects.insert(seed.clone());
                }
            }
            let inferred = jet_foundation::sema::RightsRow::invocation(
                inferred_effects.iter().map(String::as_str),
                std::iter::empty::<&str>(),
                jet_foundation::sema::RightsProvenance::new(
                    view.key.clone(),
                    "sema inferred effect facts",
                ),
            );
            let call_chain = projection
                .index
                .effect_of(&view.key)
                .and_then(|fact| fact.provenance.first())
                .map(|witness| witness.call_path.clone())
                .filter(|path| !path.is_empty())
                .unwrap_or_else(|| vec![view.key.clone()]);
            let walk = declared.walk(&inferred_effects, &call_chain, &scope_chain);
            CallableReport {
                view,
                declared,
                inferred,
                inferred_effects,
                direct_effects: summary
                    .map(|summary| summary.direct.clone())
                    .unwrap_or_default(),
                callees: summary
                    .map(|summary| summary.edges.iter().cloned().collect())
                    .unwrap_or_default(),
                maximal: summary.is_some_and(|summary| summary.maximal),
                walk,
            }
        })
        .collect::<Vec<_>>();

    let authority_facts = jet_foundation::Authority::effect_declarations()
        .iter()
        .map(|declaration| {
            StatusValue::object(
                StatusFields::new()
                    .with("name", declaration.name)
                    .with("irreversible", declaration.irreversible),
            )
        })
        .collect::<Vec<_>>();
    let authority = StatusValue::object(
        StatusFields::new()
            .with("source", application.authority.clone())
            .with("required", holds_value(&application.required_effects))
            .with("granted", holds_value(&application.granted_effects))
            .with("denied", holds_value(&application.denied_effects)),
    );
    let callable_values = reports
        .iter()
        .map(|report| {
            let denials = report
                .walk
                .denials
                .iter()
                .map(|denial| {
                    let witness = projection
                        .index
                        .effect_of(&report.view.key)
                        .and_then(|fact| {
                            fact.provenance
                                .iter()
                                .find(|witness| witness.effect == denial.right)
                        });
                    let witness_spans =
                        witness
                            .map(|witness| {
                                StatusValue::array(witness.spans.iter().map(|span| {
                                    span_value(&report.view.source, span.start, span.end)
                                }))
                            })
                            .unwrap_or_else(|| StatusValue::array(Vec::<StatusValue>::new()));
                    let nearest = denial
                        .chain
                        .nearest_granting_scope
                        .as_ref()
                        .map(frame_value)
                        .unwrap_or(StatusValue::Null);
                    StatusValue::object(
                        StatusFields::new()
                            .with("right", denial.right.clone())
                            .with("verdict", verdict_name(denial.verdict))
                            .with(
                                "chain",
                                StatusValue::object(
                                    StatusFields::new()
                                        .with("right", denial.chain.right.clone())
                                        .with(
                                            "call_chain",
                                            StatusValue::array(
                                                denial
                                                    .chain
                                                    .call_chain
                                                    .iter()
                                                    .cloned()
                                                    .map(StatusValue::String),
                                            ),
                                        )
                                        .with(
                                            "scope_chain",
                                            StatusValue::array(
                                                denial.chain.scope_chain.iter().map(frame_value),
                                            ),
                                        )
                                        .with("nearest_granting_scope", nearest),
                                ),
                            )
                            .with("source_spans", witness_spans),
                    )
                })
                .collect::<Vec<_>>();
            let summary = StatusFields::new()
                .with("direct", holds_value(&report.direct_effects))
                .with(
                    "callees",
                    StatusValue::array(report.callees.iter().cloned().map(StatusValue::String)),
                )
                .with("maximal", report.maximal);
            StatusValue::object(
                StatusFields::new()
                    .with("callable", report.view.key.clone())
                    .with("name", report.view.name.clone())
                    .with(
                        "source_span",
                        span_value(
                            &report.view.source,
                            report.view.span.start,
                            report.view.span.end,
                        ),
                    )
                    .with("declared_row", row_value(&report.declared))
                    .with("inferred_row", row_value(&report.inferred))
                    .with("inferred_effects", holds_value(&report.inferred_effects))
                    .with("effects", StatusValue::object(summary))
                    .with("verdict", verdict_name(report.walk.verdict))
                    .with("denials", StatusValue::array(denials)),
            )
        })
        .collect::<Vec<_>>();
    if json {
        let fields = StatusFields::new()
            .with("scope", scope_name(scope))
            .with("target", target)
            .with("authority", authority)
            .with("authority_facts", StatusValue::array(authority_facts))
            .with("callables", StatusValue::array(callable_values));
        println!(
            "{}",
            StatusEnvelope::new("inspect.rights", true)
                .with_fields(fields)
                .json()
        );
    } else {
        println!("rights");
        println!("scope: {}", scope_name(scope));
        println!("target: {target}");
        println!("authority: {}", application.authority);
        println!("granted: {}", holds_text(&application.granted_effects));
        println!("denied: {}", holds_text(&application.denied_effects));
        println!("authority facts:");
        for declaration in jet_foundation::Authority::effect_declarations() {
            println!(
                "  {}{}",
                declaration.name,
                if declaration.irreversible {
                    " (irreversible)"
                } else {
                    ""
                }
            );
        }
        for report in reports {
            println!(
                "callable: {} @ {}:{}-{}",
                report.view.key, report.view.source, report.view.span.start, report.view.span.end
            );
            println!("  declared row: {}", row_text(&report.declared));
            println!("  inferred row: {}", row_text(&report.inferred));
            println!(
                "  inferred effects: {}",
                holds_text(&report.inferred_effects)
            );
            println!("  verdict: {}", verdict_name(report.walk.verdict));
            if report.walk.denials.is_empty() {
                println!("  denials: none");
            } else {
                for denial in report.walk.denials {
                    println!(
                        "  denial: {} ({}) via {}",
                        denial.right,
                        verdict_name(denial.verdict),
                        if denial.chain.call_chain.is_empty() {
                            "none".to_string()
                        } else {
                            denial.chain.call_chain.join(" -> ")
                        }
                    );
                }
            }
        }
    }
}

const CAPABILITY_RELATION_SCHEMA: &str = "jet.capability.relation.v1";

fn capability_status_field<'a>(value: &'a StatusValue, name: &str) -> Option<&'a StatusValue> {
    match value {
        StatusValue::Object(fields) => fields
            .iter()
            .find_map(|(field, value)| (field == name).then_some(value)),
        _ => None,
    }
}

fn capability_status_string(value: &StatusValue) -> Option<&str> {
    match value {
        StatusValue::String(value) => Some(value.as_str()),
        _ => None,
    }
}

fn capability_relation_status(value: &StatusValue) -> &str {
    capability_status_field(value, "status")
        .and_then(capability_status_string)
        .unwrap_or("unavailable")
}

pub(crate) fn capability_relation_projection(root: &Path) -> StatusValue {
    let path = root.join(".jet/hardening-manifest.json");
    let unavailable = |reason: String| {
        StatusValue::object(
            StatusFields::new()
                .with("schema", CAPABILITY_RELATION_SCHEMA)
                .with("status", "unavailable")
                .with("relation", "manifest.capability_relation.rows")
                .with("path", path.display().to_string())
                .with("reason", reason)
                .with("rows", StatusValue::array(Vec::<StatusValue>::new())),
        )
    };
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) => {
            return unavailable(format!(
                "can't read capability relation `{}`: {error}",
                path.display()
            ));
        }
    };
    let manifest = match StatusValue::parse(&raw) {
        Ok(value) => value,
        Err(error) => {
            return unavailable(format!(
                "can't parse capability relation source `{}`: {error}",
                path.display()
            ));
        }
    };
    let relation_fields = match capability_status_field(&manifest, "capability_relation") {
        Some(StatusValue::Object(fields)) => fields.clone(),
        Some(_) => {
            return unavailable("manifest capability_relation must be an object".to_string());
        }
        None => {
            return unavailable("manifest has no capability_relation".to_string());
        }
    };
    let relation = StatusValue::Object(relation_fields.clone());
    if capability_status_field(&relation, "schema").and_then(capability_status_string)
        != Some(CAPABILITY_RELATION_SCHEMA)
    {
        return unavailable("capability relation schema is missing or unsupported".to_string());
    }
    if !matches!(
        capability_status_field(&relation, "rows"),
        Some(StatusValue::Array(_))
    ) {
        return unavailable("capability relation rows are missing or not an array".to_string());
    }
    for field in ["source_snapshot_hash", "content_digest"] {
        if !matches!(
            capability_status_field(&relation, field),
            Some(StatusValue::String(value)) if !value.is_empty()
        ) {
            return unavailable(format!("capability relation {field} is missing or empty"));
        }
    }
    let relation_source = capability_status_field(&relation, "source_snapshot_hash")
        .and_then(capability_status_string);
    let manifest_source = capability_status_field(&manifest, "source_snapshot")
        .and_then(|snapshot| capability_status_field(snapshot, "hash"))
        .and_then(capability_status_string);
    if relation_source.is_none() || relation_source != manifest_source {
        return unavailable(
            "capability relation source snapshot does not match the manifest".to_string(),
        );
    }

    let mut projection = StatusFields::new();
    for (name, value) in relation_fields.iter() {
        if matches!(name, "status" | "relation" | "path") {
            continue;
        }
        projection
            .insert(name.to_string(), value.clone())
            .expect("manifest capability relation fields must be unique");
    }
    projection
        .insert("status", "available")
        .expect("capability relation status field must be unique");
    projection
        .insert("relation", "manifest.capability_relation.rows")
        .expect("capability relation route field must be unique");
    projection
        .insert("path", path.display().to_string())
        .expect("capability relation path field must be unique");
    StatusValue::object(projection)
}

/// `inspect claims --json` consumes this exact manifest relation without
/// rebuilding capability identities, dispositions, or evidence.

fn run_claims(file: &str, json: bool) {
    let source_path = Path::new(file);
    let base = source_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let root = jet::Loader::find_package_root_checked(base)
        .ok()
        .flatten()
        .unwrap_or_else(|| base.to_path_buf());
    let index = jet::RecordIndex::RecordIndex::load_for_project(&root).unwrap_or_else(|error| {
        crate::cli_error!(
            @fix "E2105",
            format!("can't read evidence index: {error}"),
            "repair `.jet/records/index.jsonl`, then rerun inspect claims"
        );
        exit(jet::ExitCodes::USER_ERROR);
    });
    let mut claims = Vec::new();
    let mut derivation_dispositions = BTreeMap::new();
    let mut derivation_values = BTreeMap::new();
    let mut derivation_summaries = BTreeMap::new();
    let mut records = Vec::new();
    for entry in index
        .query(false)
        .into_iter()
        .filter(|entry| entry.kind == jet::RecordKind::Evidence)
    {
        let path = index.root().join(&entry.path);
        let report =
            jet_foundation::Evidence::EvidenceReport::read(&path).unwrap_or_else(|error| {
                crate::cli_error!(
                    @fix "E2105",
                    format!("can't read evidence artifact `{}`: {error}", path.display()),
                    "repair the indexed artifact or rerun the producer"
                );
                exit(jet::ExitCodes::USER_ERROR);
            });
        for derivation in &report.derivations {
            derivation_dispositions.insert(
                derivation.id.clone(),
                derivation.disposition.as_str().to_string(),
            );
            derivation_values.insert(
                derivation.id.clone(),
                StatusValue::parse(&derivation.to_json()).unwrap_or(StatusValue::Null),
            );
            let observation = derivation
                .observation
                .as_ref()
                .map(|observation| {
                    format!(
                        "event={},counterexample={}",
                        observation.event_id.as_deref().unwrap_or("none"),
                        observation.counterexample_id.as_deref().unwrap_or("none")
                    )
                })
                .unwrap_or_else(|| "none".to_string());
            derivation_summaries.insert(
                derivation.id.clone(),
                format!(
                    "method={} disposition={} rule={} source={} build={} run={} target={} observation={}",
                    derivation.method.as_str(),
                    derivation.disposition.as_str(),
                    derivation.rule,
                    derivation.identity.source,
                    derivation.identity.build,
                    derivation.identity.run,
                    derivation.identity.target,
                    observation
                ),
            );
        }
        for claim in report.project() {
            let count = report
                .records
                .iter()
                .find(|record| record.identity.evidence_id == claim.evidence_id)
                .map_or(0, |record| record.count);
            claims.push((claim, count));
        }
        records.extend(report.records.iter().cloned());
    }
    claims.sort_by(|left, right| {
        left.0
            .claim_id
            .cmp(&right.0.claim_id)
            .then(left.0.evidence_id.cmp(&right.0.evidence_id))
    });
    let projection = jet::Package::ClaimsProjection::from_records(&records);
    let grade = projection.grade;
    let floor = jet::Loader::package_facts_for_entry(source_path)
        .ok()
        .flatten()
        .and_then(|facts| facts.policy.claims_min);
    let floor_met = projection.floor_met(floor);
    let capability_relation = capability_relation_projection(&root);

    if json {
        let rows = claims
            .iter()
            .map(|(claim, count)| {
                let derivation = claim
                    .derivation
                    .as_ref()
                    .map(|reference| {
                        derivation_values
                            .get(&reference.id)
                            .cloned()
                            .unwrap_or_else(|| {
                                let disposition = derivation_dispositions
                                    .get(&reference.id)
                                    .map(String::as_str)
                                    .unwrap_or("unknown");
                                StatusValue::object(
                                    StatusFields::new()
                                        .with("id", reference.id.clone())
                                        .with("disposition", disposition),
                                )
                            })
                    })
                    .unwrap_or(StatusValue::Null);
                StatusValue::object(
                    StatusFields::new()
                        .with("claim", claim.claim_id.clone())
                        .with("evidence", claim.evidence_id.clone())
                        .with("kind", claim.kind.as_str())
                        .with("facet", claim.facet.as_str())
                        .with("producer", claim.producer.as_str())
                        .with("outcome", claim.outcome.as_str())
                        .with("count", *count)
                        .with("path", claim.source.path.clone())
                        .with("line", claim.source.line)
                        .with("column", claim.source.column)
                        .with("completeness", claim.completeness.state())
                        .with("derivation", derivation),
                )
            })
            .collect::<Vec<_>>();
        let claims_floor = floor
            .map(|floor| StatusValue::String(floor.render()))
            .unwrap_or(StatusValue::Null);
        let fields = StatusFields::new()
            .with("file", file)
            .with("capability_relation", capability_relation.clone())
            .with("claims", StatusValue::array(rows))
            .with("grade", grade.render())
            .with("generated_attempts", projection.generated_attempts)
            .with("generated_successes", projection.generated_successes)
            .with("examples_successes", projection.examples_successes)
            .with("failed", projection.failed)
            .with("claims_floor", claims_floor)
            .with("floor_met", floor_met);
        println!(
            "{}",
            StatusEnvelope::new("inspect.claims", floor_met)
                .with_fields(fields)
                .json()
        );
    } else {
        println!("claims");
        println!(
            "capability relation: {}",
            capability_relation_status(&capability_relation)
        );
        println!("grade: {}", grade.render());
        println!(
            "generated: {} successful / {} attempted; examples: {}; failed: {}",
            projection.generated_successes,
            projection.generated_attempts,
            projection.examples_successes,
            projection.failed
        );
        println!(
            "package floor: {} ({})",
            floor
                .map(|floor| floor.render())
                .unwrap_or_else(|| "none".to_string()),
            if floor_met { "met" } else { "not met" }
        );
        if claims.is_empty() {
            println!("none");
        }
        for (claim, _) in claims {
            let derivation = claim.derivation.as_ref().map(|reference| {
                let summary = derivation_summaries
                    .get(&reference.id)
                    .map(String::as_str)
                    .unwrap_or("disposition=unknown");
                format!(" derivation={}({summary})", reference.id)
            });
            println!(
                "{} {} {} {}:{}{}",
                claim.claim_id,
                claim.kind.as_str(),
                claim.outcome.as_str(),
                claim.source.path,
                claim.source.line,
                derivation.as_deref().unwrap_or("")
            );
        }
    }
}

fn plain_number(name: &str) -> bool {
    matches!(name, "Int" | "Float" | "Decimal")
}

fn operator_symbol(trait_name: &str) -> Option<&'static str> {
    match trait_name {
        "Add" => Some("+"),
        "Sub" => Some("-"),
        "Mul" => Some("*"),
        "Div" => Some("/"),
        _ => None,
    }
}
