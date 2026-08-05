use std::fs;
use std::path::{Path, PathBuf};

use jet_driver::Diagnostics::{Diagnostic, Severity};
use jet_driver::SHA256;

use super::graph_projection::project_checked;
use super::project_transactions::{diagnostic_json, rel_path};
use super::schema_api::{Projection, source_revision};
use super::validation_json::{json_optional_str, json_str};

pub(super) fn project_file(path: &Path) -> Result<Projection, Vec<Diagnostic>> {
    project_file_with_runtime(path, None)
}

pub(super) fn project_file_with_runtime(
    path: &Path,
    runtime_events: Option<&str>,
) -> Result<Projection, Vec<Diagnostic>> {
    // Canvas is callable from test/UI threads with small default stacks, while
    // the authoritative loader and sema path can recurse through a whole
    // project. Reuse the compiler worker when already inside it.
    jet_driver::run_compiler_work(|| project_file_with_runtime_on_compiler_stack(path, runtime_events))
}

fn project_file_with_runtime_on_compiler_stack(
    path: &Path,
    runtime_events: Option<&str>,
) -> Result<Projection, Vec<Diagnostic>> {
    let path_str = path.to_string_lossy();
    let src = fs::read_to_string(path).unwrap_or_default();
    let package_facts = jet_semindex::package_facts_for_entry(path).map_err(|error| {
        vec![jet_semindex::package_facts_diagnostic(path, &error)]
    })?;
    let (diags, bundle, facts) = jet_driver::Driver::check_file_with_effect_facts(&path_str, None, true);
    let errors: Vec<Diagnostic> = diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .cloned()
        .collect();
    if !errors.is_empty() {
        return Err(errors);
    }
    let Some(bundle) = bundle else {
        return Err(diags);
    };
    let workspace_overlay_policy = jet_semindex::workspace_overlay_policy_for_entry(path)
        .map_err(|diagnostic| vec![diagnostic])?;
    Ok(project_checked(
        path,
        &src,
        &bundle,
        &facts,
        package_facts,
        workspace_overlay_policy,
        runtime_events,
    ))
}

#[derive(Clone)]
pub(super) struct ProjectFileRec {
    pub(super) path: String,
    pub(super) revision: String,
    pub(super) kind: String,
}

pub(super) struct ProjectContext {
    pub(super) entry_path: PathBuf,
    pub(super) project_root: PathBuf,
    pub(super) manifest_root: Option<PathBuf>,
    /// Canonical E5 Package root. This is separate from `manifest_root` so
    /// legacy `pkg.jet` compiler support cannot become a second authority for
    /// a project that already has `package.jet`.
    pub(super) ecosystem_root: Option<PathBuf>,
    pub(super) workspace_root: Option<PathBuf>,
    pub(super) files: Vec<ProjectFileRec>,
    pub(super) parts: jet_driver::ProjectParts::ProjectPartsReport,
    pub(super) project_revision: String,
}

pub(super) struct TouchedProjectFile {
    pub(super) path: String,
    pub(super) revision: String,
}

pub(super) struct ProjectChange {
    pub(super) path: PathBuf,
    pub(super) rel: String,
    pub(super) before: String,
    pub(super) after: String,
}

struct WorkspaceBoundary {
    root: PathBuf,
    member_root: Option<PathBuf>,
    malformed: bool,
}

pub(super) fn project_context_for_entry(path: &Path) -> ProjectContext {
    let entry_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let workspace_boundary = find_workspace_boundary(entry_dir);
    let workspace_root = workspace_boundary
        .as_ref()
        .filter(|boundary| boundary.malformed
            || boundary.member_root.is_some()
            || same_path(entry_dir, &boundary.root))
        .map(|boundary| boundary.root.clone());
    let manifest_root = jet_driver::Loader::find_manifest_root(entry_dir).filter(|manifest| {
        let Some(boundary) = &workspace_boundary else {
            return true;
        };
        match &boundary.member_root {
            Some(member_root) => path_is_within(manifest, member_root),
            None if same_path(entry_dir, &boundary.root) => same_path(manifest, &boundary.root),
            None => {
                path_is_within(manifest, &boundary.root)
                    && !same_path(manifest, &boundary.root)
            }
        }
    });
    let ecosystem_root = workspace_root
        .as_deref()
        .filter(|root| root.join("package.jet").is_file())
        .map(Path::to_path_buf)
        .or_else(|| find_package_root(entry_dir));
    let project_root = workspace_root
        .as_deref()
        .or(manifest_root.as_deref())
        .or(ecosystem_root.as_deref())
        .unwrap_or(entry_dir)
        .to_path_buf();
    let files = collect_project_files(
        &project_root,
        path,
        manifest_root.as_deref(),
        ecosystem_root.as_deref(),
        workspace_root.as_deref(),
    );
    let parts = jet_driver::ProjectParts::scan(&project_root);
    let project_revision = project_revision_from_files(&files);
    ProjectContext {
        entry_path: path.to_path_buf(),
        project_root,
        manifest_root,
        ecosystem_root,
        workspace_root,
        files,
        parts,
        project_revision,
    }
}

fn find_package_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join("package.jet").is_file() {
            return Some(dir);
        }
        dir = dir.parent()?.to_path_buf();
    }
}

fn find_workspace_boundary(start: &Path) -> Option<WorkspaceBoundary> {
    let mut dir = start.to_path_buf();
    loop {
        match jet_env_model::WorkspaceFile::load(&dir) {
            Some(Ok(plan)) => {
                let member_root = matching_member_root(&dir, start, &plan);
                return Some(WorkspaceBoundary {
                    root: dir,
                    member_root,
                    malformed: false,
                });
            }
            Some(Err(_)) => {
                return Some(WorkspaceBoundary {
                    root: dir,
                    member_root: None,
                    malformed: true,
                });
            }
            None => {}
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => return None,
        }
    }
}

fn matching_member_root(
    workspace_root: &Path,
    entry_dir: &Path,
    plan: &jet_env_model::WorkspaceFile::WorkspacePlan,
) -> Option<PathBuf> {
    plan.members
        .iter()
        .map(|member| workspace_root.join(&member.path))
        .filter(|member_root| path_is_within(entry_dir, member_root))
        .max_by_key(|member_root| member_root.components().count())
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    comparable_path(path).starts_with(comparable_path(root))
}

fn same_path(left: &Path, right: &Path) -> bool {
    comparable_path(left) == comparable_path(right)
}

fn comparable_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        }
    })
}

fn collect_project_files(
    project_root: &Path,
    entry_path: &Path,
    manifest_root: Option<&Path>,
    ecosystem_root: Option<&Path>,
    workspace_root: Option<&Path>,
) -> Vec<ProjectFileRec> {
    let mut paths = Vec::new();
    push_existing(&mut paths, entry_path);
    push_existing(&mut paths, &project_root.join(jet_driver::Syntax::ENV_FILE));
    if let Some(root) = manifest_root {
        push_existing(&mut paths, &root.join(jet_driver::Syntax::PAYLOAD_FILE));
    }
    if let Some(root) = ecosystem_root {
        push_existing(&mut paths, &root.join("package.jet"));
    }
    if let Some(root) = workspace_root {
        push_existing(&mut paths, &root.join(jet_driver::Syntax::WORKSPACE_FILE));
        push_existing(&mut paths, &root.join(jet_driver::Syntax::UNIFIED_LOCK_FILE));
        if let Some(Ok(plan)) = jet_env_model::WorkspaceFile::load(root) {
            for member in plan.members {
                let member_dir = root.join(member.path);
                push_existing(&mut paths, &member_dir.join(jet_driver::Syntax::PAYLOAD_FILE));
                push_existing(&mut paths, &member_dir.join("package.jet"));
                collect_jet_files(&member_dir, &mut paths);
            }
        }
    }
    if workspace_root.is_none() {
        collect_jet_files(manifest_root.unwrap_or(project_root), &mut paths);
    }
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .filter_map(|path| {
            let bytes = fs::read(&path).ok()?;
            let kind = if path.file_name().and_then(|n| n.to_str())
                == Some(jet_driver::Syntax::PAYLOAD_FILE)
            {
                "manifest"
            } else if path.file_name().and_then(|n| n.to_str()) == Some("package.jet") {
                "package"
            } else if path.file_name().and_then(|n| n.to_str())
                == Some(jet_driver::Syntax::WORKSPACE_FILE)
            {
                "workspace"
            } else if path.file_name().and_then(|n| n.to_str()) == Some(jet_driver::Syntax::ENV_FILE) {
                "env"
            } else if rel_path(project_root, &path) == jet_driver::Syntax::UNIFIED_LOCK_FILE {
                "lock"
            } else {
                "source"
            };
            Some(ProjectFileRec {
                path: rel_path(project_root, &path),
                revision: format!("sha256-{}", SHA256::sha256_hex(&bytes)),
                kind: kind.to_string(),
            })
        })
        .collect()
}

fn push_existing(paths: &mut Vec<PathBuf>, path: &Path) {
    if path.is_file() {
        paths.push(path.to_path_buf());
    }
}

fn collect_jet_files(dir: &Path, out: &mut Vec<PathBuf>) {
    // Watch every source so import edits immediately change project-part state.
    // Semantic consumers filter with ProjectPartsReport::should_index.
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".git" || name == ".jet" || name == "target" || name == "build" {
            continue;
        }
        if path.is_dir() {
            collect_jet_files(&path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some(jet_driver::Syntax::FILE_EXT)
            && path.file_name().and_then(|n| n.to_str()) != Some(jet_driver::Syntax::PAYLOAD_FILE)
        {
            out.push(path);
        }
    }
}

pub(super) fn project_revision_from_files(files: &[ProjectFileRec]) -> String {
    let mut acc = String::new();
    for file in files {
        acc.push_str(&file.path);
        acc.push('\n');
        acc.push_str(&file.revision);
        acc.push('\n');
        acc.push_str(&file.kind);
        acc.push('\n');
    }
    source_revision(&acc)
}

pub(super) fn workspace_project_json(project_root: &Path, workspace_root: Option<&Path>) -> String {
    let Some(root) = workspace_root else {
        return "null".to_string();
    };
    let members = match jet_env_model::WorkspaceFile::load(root) {
        Some(Ok(plan)) => plan
            .members
            .iter()
            .map(|m| {
                format!(
                    "{{\"name\":{},\"path\":{}}}",
                    json_str(&m.name),
                    json_str(&m.path)
                )
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Err(d)) => {
            return format!(
                "{{\"path\":{},\"members\":[],\"diagnostics\":[{}]}}",
                json_str(&rel_path(project_root, &root.join(jet_driver::Syntax::WORKSPACE_FILE))),
                diagnostic_json(&d)
            );
        }
        None => String::new(),
    };
    format!(
        "{{\"path\":{},\"members\":[{}],\"diagnostics\":[]}}",
        json_str(&rel_path(project_root, &root.join(jet_driver::Syntax::WORKSPACE_FILE))),
        members
    )
}

pub(super) fn packages_project_json(
    project_root: &Path,
    entry_path: &Path,
    manifest_root: Option<&Path>,
    ecosystem_root: Option<&Path>,
    workspace_root: Option<&Path>,
) -> String {
    package_dirs(manifest_root, ecosystem_root, workspace_root)
        .iter()
        .filter_map(|dir| package_project_json(project_root, entry_path, dir))
        .collect::<Vec<_>>()
        .join(",")
}

fn package_dirs(
    manifest_root: Option<&Path>,
    ecosystem_root: Option<&Path>,
    workspace_root: Option<&Path>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(root) = manifest_root {
        dirs.push(root.to_path_buf());
    }
    if let Some(root) = ecosystem_root {
        dirs.push(root.to_path_buf());
    }
    if let Some(root) = workspace_root {
        if let Some(Ok(plan)) = jet_env_model::WorkspaceFile::load(root) {
            for member in plan.members {
                dirs.push(root.join(member.path));
            }
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

pub(super) fn targets_project_json(
    project_root: &Path,
    manifest_root: Option<&Path>,
    ecosystem_root: Option<&Path>,
    workspace_root: Option<&Path>,
) -> String {
    package_dirs(manifest_root, ecosystem_root, workspace_root)
        .iter()
        .filter_map(|dir| package_targets_project_json(project_root, dir))
        .flatten()
        .collect::<Vec<_>>()
        .join(",")
}

fn package_targets_project_json(project_root: &Path, dir: &Path) -> Option<Vec<String>> {
    if dir.join("package.jet").is_file() {
        return canonical_package_targets_project_json(project_root, dir);
    }
    let manifest_path = dir.join(jet_driver::Syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&manifest_path).ok()?;
    let manifest = jet_driver::PackageManifest::parse(&raw).ok()?;
    let package_path = rel_path(project_root, dir);
    let manifest_rel = rel_path(project_root, &manifest_path);
    Some(
        manifest
            .packages
            .iter()
            .flat_map(|package| {
                let package_path = package_path.clone();
                let manifest_rel = manifest_rel.clone();
                package.targets.iter().map(move |target| {
                    format!(
                        "{{\"package\":{},\"package_path\":{},\"manifest\":{},\"target\":{}}}",
                        json_str(&package.name),
                        json_str(&package_path),
                        json_str(&manifest_rel),
                        json_str(&target_label(target))
                    )
                })
            })
            .collect(),
    )
}

fn package_project_json(project_root: &Path, entry_path: &Path, dir: &Path) -> Option<String> {
    if dir.join("package.jet").is_file() {
        return canonical_package_project_json(project_root, entry_path, dir);
    }
    let manifest_path = dir.join(jet_driver::Syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&manifest_path).ok()?;
    match jet_driver::PackageManifest::parse(&raw) {
        Ok(manifest) => {
            let deps = manifest
                .deps
                .iter()
                .map(|d| {
                    format!(
                        "{{\"name\":{},\"source\":{}}}",
                        json_str(&d.name),
                        json_str(&dep_source_label(&d.source))
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let targets = manifest
                .packages
                .iter()
                .flat_map(|p| {
                    p.targets.iter().map(move |t| {
                        format!(
                            "{{\"package\":{},\"target\":{}}}",
                            json_str(&p.name),
                            json_str(&target_label(t))
                        )
                    })
                })
                .collect::<Vec<_>>()
                .join(",");
            Some(format!(
                "{{\"path\":{},\"manifest\":{},\"name\":{},\"version\":{},\"target\":{},\"deps\":[{}],\"targets\":[{}],\"effects_enabled\":{},\"diagnostics\":[]}}",
                json_str(&rel_path(project_root, dir)),
                json_str(&rel_path(project_root, &manifest_path)),
                json_str(&manifest.package.name),
                json_str(&manifest.package.version),
                json_str(manifest.package.target.as_deref().unwrap_or("native")),
                deps,
                targets,
                if manifest.effects_enabled { "true" } else { "false" }
            ))
        }
        Err(err) => Some(format!(
            "{{\"path\":{},\"manifest\":{},\"name\":{},\"version\":\"\",\"target\":\"native\",\"deps\":[],\"targets\":[],\"effects_enabled\":false,\"diagnostics\":[{{\"code\":\"manifest\",\"what\":{},\"why\":\"pkg.jet did not parse as a package manifest\",\"fix\":\"fix pkg.jet before Canvas uses package facts\"}}]}}",
            json_str(&rel_path(project_root, dir)),
            json_str(&rel_path(project_root, &manifest_path)),
            json_str(
                dir.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("package")
            ),
            json_str(&format!("{:?}", err))
        )),
    }
}

fn canonical_package_facts(
    dir: &Path,
) -> Result<jet_driver::Package::PackageFacts, String> {
    jet_driver::Package::PackageFacts::load(dir)
        .ok_or_else(|| format!("canonical Package `{}` is missing", dir.join("package.jet").display()))?
        .map_err(|error| error.to_string())
}

fn canonical_package_error(
    project_root: &Path,
    entry_path: &Path,
    dir: &Path,
    error: &str,
) -> String {
    let diagnostic = jet_semindex::package_facts_diagnostic(entry_path, error);
    format!(
        "{{\"path\":{},\"manifest\":{},\"name\":{},\"version\":\"\",\"target\":\"native\",\"deps\":[],\"targets\":[],\"outputs\":[],\"environments\":[],\"effects_enabled\":false,\"diagnostics\":[{}]}}",
        json_str(&rel_path(project_root, dir)),
        json_str(&rel_path(project_root, &dir.join("package.jet"))),
        json_str(dir.file_name().and_then(|name| name.to_str()).unwrap_or("package")),
        diagnostic_json(&diagnostic),
    )
}

fn output_kind_label(kind: &jet_driver::Package::PackageOutputKind) -> &'static str {
    use jet_driver::Package::PackageOutputKind;
    match kind {
        PackageOutputKind::Library => "library",
        PackageOutputKind::Executable => "executable",
        PackageOutputKind::Service => "service",
        PackageOutputKind::Check => "check",
        PackageOutputKind::Environment => "environment",
        PackageOutputKind::Image => "image",
        PackageOutputKind::Bundle => "bundle",
        PackageOutputKind::System => "system",
        PackageOutputKind::Fleet => "fleet",
    }
}

fn output_payload_json(payload: &jet_driver::Package::OutputPayload) -> String {
    use jet_driver::Package::OutputPayload;
    match payload {
        OutputPayload::Null => "null".to_string(),
        OutputPayload::Bool(value) => value.to_string(),
        OutputPayload::Number(value) => value.clone(),
        OutputPayload::String(value) => json_str(value),
        OutputPayload::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(output_payload_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        OutputPayload::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(name, value)| format!("{}:{}", json_str(name), output_payload_json(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn field_provenance_json(
    facts: &jet_driver::Package::PackageFacts,
    field: &str,
) -> String {
    format!(
        "[{}]",
        facts
            .field_provenance(field)
            .iter()
            .map(|origin| json_str(origin))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn canonical_outputs_json(facts: &jet_driver::Package::PackageFacts) -> String {
    facts
        .outputs
        .iter()
        .map(|(name, output)| {
            format!(
                "{{\"name\":{},\"kind\":{},\"entry\":{},\"payload\":{},\"provenance\":{}}}",
                json_str(&output.name),
                json_str(output_kind_label(&output.kind)),
                json_optional_str(output.entry.as_deref()),
                output_payload_json(&output.payload),
                field_provenance_json(facts, &format!("outputs.{name}")),
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn canonical_environments_json(facts: &jet_driver::Package::PackageFacts) -> String {
    facts
        .environments
        .values()
        .map(|environment| {
            let services = environment
                .services
                .iter()
                .map(|(name, service)| {
                    format!(
                        "{{\"name\":{},\"enable\":{},\"ports\":[{}],\"ready\":{}}}",
                        json_str(name),
                        if service.enable { "true" } else { "false" },
                        service
                            .ports
                            .iter()
                            .map(i64::to_string)
                            .collect::<Vec<_>>()
                            .join(","),
                        json_optional_str(service.ready.as_deref()),
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"name\":{},\"tools\":[{}],\"services\":[{}],\"secrets\":[{}]}}",
                json_str(&environment.name),
                environment
                    .tools
                    .iter()
                    .map(|tool| json_str(tool))
                    .collect::<Vec<_>>()
                    .join(","),
                services,
                environment
                    .secrets
                    .keys()
                    .map(|name| json_str(name))
                    .collect::<Vec<_>>()
                    .join(","),
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn canonical_package_targets_project_json(project_root: &Path, dir: &Path) -> Option<Vec<String>> {
    let facts = canonical_package_facts(dir).ok()?;
    let package_path = rel_path(project_root, dir);
    let manifest_rel = rel_path(project_root, &dir.join("package.jet"));
    Some(
        facts
            .outputs
            .iter()
            .map(|(name, output)| {
                format!(
                    "{{\"package\":{},\"package_path\":{},\"manifest\":{},\"target\":{},\"kind\":{},\"entry\":{},\"payload\":{},\"provenance\":{}}}",
                    json_str(&facts.name),
                    json_str(&package_path),
                    json_str(&manifest_rel),
                    json_str(&output.name),
                    json_str(output_kind_label(&output.kind)),
                    json_optional_str(output.entry.as_deref()),
                    output_payload_json(&output.payload),
                    field_provenance_json(&facts, &format!("outputs.{name}")),
                )
            })
            .collect(),
    )
}

fn canonical_package_project_json(
    project_root: &Path,
    entry_path: &Path,
    dir: &Path,
) -> Option<String> {
    let facts = match canonical_package_facts(dir) {
        Ok(facts) => facts,
        Err(error) => return Some(canonical_package_error(project_root, entry_path, dir, &error)),
    };
    let (workspace_overlays, diagnostics) = match
        jet_semindex::workspace_overlay_policy_for_entry(&dir.join("package.jet"))
    {
        Ok(Some(policy)) => (
            jet_semindex::workspace_overlay_policy_json(&policy),
            "[]".to_string(),
        ),
        Ok(None) => ("null".to_string(), "[]".to_string()),
        Err(diagnostic) => (
            "null".to_string(),
            format!("[{}]", diagnostic_json(&diagnostic)),
        ),
    };
    let deps = facts
        .deps
        .iter()
        .map(|(name, source)| {
            format!(
                "{{\"name\":{},\"source\":{}}}",
                json_str(name),
                json_str(source)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Some(format!(
        "{{\"path\":{},\"manifest\":{},\"name\":{},\"version\":{},\"target\":\"native\",\"deps\":[{}],\"targets\":[{}],\"outputs\":[{}],\"environments\":[{}],\"configs\":[{}],\"members\":[{}],\"package_facts\":{},\"workspace_overlays\":{},\"effects_enabled\":false,\"diagnostics\":{}}}",
        json_str(&rel_path(project_root, dir)),
        json_str(&rel_path(project_root, &dir.join("package.jet"))),
        json_str(&facts.name),
        json_optional_str(facts.version.as_deref()),
        deps,
        facts
            .outputs
            .values()
            .map(|output| json_str(&output.name))
            .collect::<Vec<_>>()
            .join(","),
        canonical_outputs_json(&facts),
        canonical_environments_json(&facts),
        facts.configs.iter().map(|name| json_str(name)).collect::<Vec<_>>().join(","),
        facts
            .members
            .iter()
            .map(|member| {
                let value = match member {
                    jet_driver::Package::MemberRef::Path(path) => path,
                    jet_driver::Package::MemberRef::Find(path) => path,
                };
                json_str(value)
            })
            .collect::<Vec<_>>()
            .join(","),
        jet_semindex::package_facts_json(&facts),
        workspace_overlays,
        diagnostics,
    ))
}

pub(super) struct EnvProjectJson {
    pub(super) envs: String,
    pub(super) services: String,
    pub(super) diagnostics: String,
}

pub(super) fn env_project_json(project_root: &Path) -> EnvProjectJson {
    let path = project_root.join(jet_driver::Syntax::ENV_FILE);
    let Ok(src) = fs::read_to_string(&path) else {
        return EnvProjectJson {
            envs: String::new(),
            services: String::new(),
            diagnostics: String::new(),
        };
    };
    match jet_env_model::ModuleEval::evaluate_env(&src, project_root) {
        Ok(plan) => {
            let packages = plan
                .package_refs
                .iter()
                .map(|p| json_str(p))
                .collect::<Vec<_>>()
                .join(",");
            let secrets = plan
                .secrets
                .iter()
                .map(|s| json_str(s))
                .collect::<Vec<_>>()
                .join(",");
            let services = plan
                .dev_services
                .iter()
                .map(dev_service_project_json)
                .collect::<Vec<_>>()
                .join(",");
            let environments = plan
                .environment_names
                .iter()
                .map(|name| json_str(name))
                .collect::<Vec<_>>()
                .join(",");
            let sources = plan
                .source_files
                .iter()
                .map(|path| json_str(path))
                .collect::<Vec<_>>()
                .join(",");
            let profiles = plan
                .profiles
                .iter()
                .map(|profile| json_str(&profile.name))
                .collect::<Vec<_>>()
                .join(",");
            let languages = plan
                .languages
                .iter()
                .map(|language| json_str(&language.name))
                .collect::<Vec<_>>()
                .join(",");
            let reload = match &plan.lifecycle.reload {
                jet_env_model::ModuleEval::ReloadPolicy::Never => "never".to_string(),
                jet_env_model::ModuleEval::ReloadPolicy::Prompt => "prompt".to_string(),
                jet_env_model::ModuleEval::ReloadPolicy::Watch { .. } => "watch".to_string(),
            };
            EnvProjectJson {
                envs: format!(
                    "{{\"path\":{},\"prompt\":{},\"environments\":[{}],\"sources\":[{}],\"profiles\":[{}],\"languages\":[{}],\"reload\":{},\"packages\":[{}],\"secrets\":[{}],\"diagnostics\":[]}}",
                    json_str(jet_driver::Syntax::ENV_FILE),
                    json_str(plan.prompt.as_deref().unwrap_or(jet_driver::Syntax::JETPACK_PROMPT_LABEL)),
                    environments,
                    sources,
                    profiles,
                    languages,
                    json_str(&reload),
                    packages,
                    secrets
                ),
                services,
                diagnostics: String::new(),
            }
        }
        Err(d) => EnvProjectJson {
            envs: String::new(),
            services: String::new(),
            diagnostics: diagnostic_json(&d),
        },
    }
}

fn dev_service_project_json(service: &jet_env_model::ModuleEval::DevServicePlan) -> String {
    let ports = service
        .ports
        .iter()
        .map(|port| port.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let extra = service
        .extra
        .iter()
        .map(|(key, value)| {
            format!(
                "{{\"key\":{},\"value\":{}}}",
                json_str(key),
                json_str(value)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let run = service
        .run
        .as_ref()
        .map(|args| {
            format!(
                "[{}]",
                args.iter()
                    .map(|arg| json_str(arg))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .unwrap_or_else(|| "null".to_string());
    let shutdown = service.shutdown.as_ref().map(|policy| match policy {
        jet_env_model::ModuleEval::ShutdownPolicy::Kill => "Kill",
        jet_env_model::ModuleEval::ShutdownPolicy::Term { .. } => "Term",
    });
    let after = format!(
        "[{}]",
        service
            .after
            .iter()
            .map(|name| json_str(name))
            .collect::<Vec<_>>()
            .join(",")
    );
    let before_start = format!(
        "[{}]",
        service
            .before_start
            .iter()
            .map(|name| json_str(name))
            .collect::<Vec<_>>()
            .join(",")
    );
    format!(
        "{{\"name\":{},\"enable\":{},\"ports\":[{}],\"run\":{},\"shutdown\":{},\"data_dir\":{},\"ready\":{},\"after\":{},\"before_start\":{},\"extra\":[{}],\"source\":{}}}",
        json_str(&service.name),
        if service.enable { "true" } else { "false" },
        ports,
        run,
        shutdown.map(json_str).unwrap_or_else(|| "null".to_string()),
        json_optional_str(service.data_dir.as_deref()),
        json_optional_str(service.ready.as_deref()),
        after,
        before_start,
        extra,
        json_str(jet_driver::Syntax::ENV_FILE)
    )
}

pub(super) fn lock_project_json(project_root: &Path) -> String {
    let lock_path = project_root.join(jet_driver::Syntax::UNIFIED_LOCK_FILE);
    if !lock_path.is_file() {
        return String::new();
    }
    let revision = fs::read(&lock_path)
        .map(|bytes| format!("sha256-{}", SHA256::sha256_hex(&bytes)))
        .unwrap_or_else(|_| source_revision(""));
    format!(
        "{{\"path\":{},\"revision\":{},\"kind\":\"unified\"}}",
        json_str(&rel_path(project_root, &lock_path)),
        json_str(&revision)
    )
}

fn dep_source_label(source: &jet_driver::PackageManifest::DepSource) -> String {
    match source {
        jet_driver::PackageManifest::DepSource::Version(v) => format!("version:{v}"),
        jet_driver::PackageManifest::DepSource::Provider { provider, target } => {
            format!("{provider:?}@{target}")
        }
        jet_driver::PackageManifest::DepSource::Git { url, selector } => {
            format!("git:{url}@{selector:?}")
        }
        jet_driver::PackageManifest::DepSource::CLib { target } => {
            format!("c:{target}")
        }
    }
}

fn target_label(target: &jet_driver::PackageManifest::Target) -> String {
    match target {
        jet_driver::PackageManifest::Target::Library => "library".to_string(),
        jet_driver::PackageManifest::Target::Executable => "executable".to_string(),
        jet_driver::PackageManifest::Target::Test => "test".to_string(),
        jet_driver::PackageManifest::Target::Example => "example".to_string(),
        jet_driver::PackageManifest::Target::Benchmark => "benchmark".to_string(),
        jet_driver::PackageManifest::Target::Plugin { .. } => "plugin".to_string(),
    }
}
